//! What the binary was asked to do.
//!
//! Parsed by hand rather than by a crate. Three verbs and five options do not
//! earn a dependency, and the whole surface is asserted below — which is more
//! than a derive would give, because what can be wrong here is not the shape of
//! the flags but which defaults apply and where they come from.
//!
//! # Why the password is environment-only
//!
//! There is no `--password`. A password on a command line is in `ps`, in the
//! shell's history, and in the container's inspect output — three places nobody
//! meant to put it. `DOCS_PASSWORD` is one place, and it is the one an
//! orchestrator already knows how to fill.

use std::path::PathBuf;

// The environment variables that supply the defaults, named once so that the
// usage text below and the parser cannot drift apart.
const NODE: &str = "TESSARIDB_ADDRESS";
const NAMESPACE: &str = "DOCS_NAMESPACE";
const BIND: &str = "DOCS_BIND";
const CONTENT: &str = "DOCS_CONTENT";

/// The three things this binary does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Task {
    /// Read `content/` and report what is wrong with it. Needs no node.
    Check,
    /// Rebuild the store from `content/`.
    Ingest,
    /// Serve the API.
    Serve,
    /// Write the content tree to a running site, over its API.
    Publish,
}

/// A parsed command line, with every default already applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asked {
    /// Which verb.
    pub task: Task,
    /// The content directory.
    pub content: PathBuf,
    /// The node's wire address — a bare `host:port`, no scheme.
    pub node: String,
    /// The namespace, which is the documentation version.
    pub namespace: String,
    /// Where the API listens.
    pub bind: String,
    /// Whether `serve` rebuilds the store before it takes traffic.
    pub ingest_first: bool,
    /// `publish`: the site to write to, as a URL.
    pub site: String,
    /// `publish`: the account to sign in as.
    pub user: Option<String>,
    /// `publish`: whether to write, rather than only say what would be written.
    pub apply: bool,
    /// `publish`: whether a page the site has and the corpus lacks is removed.
    pub prune: bool,
}

/// What to print when the command line does not parse, and for `--help`.
/// The default for `--to`.
pub const SITE: &str = "DOCS_SITE";
/// The default for `--user` on `publish`.
pub const PUBLISH_USER: &str = "DOCS_PUBLISH_USER";

pub const USAGE: &str = "\
docs — the documentation site for TessariDB

  docs check                     read content/ and report what is wrong with it
  docs ingest                    replace what the store holds with content/
  docs serve                     serve the API, seeding the store only if empty
  docs publish --to <url>        write content/ to a running site, over its API

The store owns the content. content/ is where it is written and reviewed, and
what an empty store is seeded from — but pages are also edited through the API,
so a populated store is the answer and is never rebuilt from disk unless asked.

Options
  --content <dir>    where the Markdown lives          (default: content)
  --at <host:port>   the node's wire address           (default: 127.0.0.1:7654)
  --namespace <ns>   the documentation version         (default: v0_0_2_alpha)
  --bind <host:port> where the API listens, serve only (default: 127.0.0.1:8080)
  --ingest           serve: replace what the store holds first. Destructive.
  --to <url>         publish: the site to write to, e.g. https://docs.example
  --user <name>      publish: the account to sign in as
  --apply            publish: write. Without it, publish only says what it would
  --prune            publish: also remove pages the site has and content/ lacks
  --help             this
  --version          the version this was built from

Environment
  TESSARIDB_ADDRESS  the default for --at
  DOCS_NAMESPACE     the default for --namespace
  DOCS_BIND          the default for --bind
  DOCS_CONTENT       the default for --content
  DOCS_USER          the store user that seeds and ingests (needs to write)
  DOCS_PASSWORD      that user's password
  DOCS_READER_USER   the store user the public reads run as — a viewer
  DOCS_READER_PASSWORD  that user's password
  DOCS_SITE          the default for --to
  DOCS_PUBLISH_USER  the default for --user
  DOCS_PUBLISH_PASSWORD the publishing account's password, for publish

A store with no users is open; declaring the first one closes it, and a closed
store refuses anonymous sessions for reads as well as writes. So a deployed
site needs DOCS_READER_USER, and it should name a viewer: the store is then
what refuses a write on the public path, rather than this server's routing.

The address is a bare host:port. There is no URL scheme, because the protocol
is not carried over HTTP. --to is different and is a URL, because that one is
HTTP and needs to say whether it is TLS.

publish and ingest do the same job from opposite sides. ingest runs beside the
store and replaces everything it holds; publish runs anywhere, writes only what
differs, and removes nothing unless asked. A deployment seeds with ingest and is
maintained with publish.
";

/// Either the parsed arguments, or something to print and the code to exit with.
#[derive(Debug, PartialEq, Eq)]
pub enum Read {
    /// Do this.
    Do(Box<Asked>),
    /// Print this and stop. `0` for `--help`, `2` for a bad command line.
    Say(String, i32),
}

/// Reads a command line.
///
/// `environment` is passed in rather than read here so that the defaults can be
/// asserted without setting process-wide state, which two tests running at once
/// would fight over.
pub fn read(words: &[String], environment: &dyn Fn(&str) -> Option<String>) -> Read {
    let mut rest = words.iter();
    let Some(first) = rest.next() else {
        return Read::Say(USAGE.to_owned(), 2);
    };
    let task = match first.as_str() {
        "check" => Task::Check,
        "ingest" => Task::Ingest,
        "serve" => Task::Serve,
        "publish" => Task::Publish,
        "--help" | "-h" | "help" => return Read::Say(USAGE.to_owned(), 0),
        "--version" => return Read::Say(format!("docs {}\n", env!("CARGO_PKG_VERSION")), 0),
        other => {
            return Read::Say(
                format!("{other} is not one of check, ingest, serve or publish\n\n{USAGE}"),
                2,
            );
        }
    };

    let mut asked = Asked {
        task,
        content: PathBuf::from(setting(environment, CONTENT, "content")),
        node: setting(environment, NODE, "127.0.0.1:7654"),
        namespace: setting(environment, NAMESPACE, "v0_0_2_alpha"),
        bind: setting(environment, BIND, "127.0.0.1:8080"),
        ingest_first: false,
        site: setting(environment, SITE, ""),
        user: environment(PUBLISH_USER).filter(|held| !held.is_empty()),
        apply: false,
        prune: false,
    };

    while let Some(word) = rest.next() {
        match word.as_str() {
            "--help" | "-h" => return Read::Say(USAGE.to_owned(), 0),
            "--ingest" => asked.ingest_first = true,
            "--apply" => asked.apply = true,
            "--prune" => asked.prune = true,
            "--to" => match rest.next() {
                Some(value) => asked.site = value.clone(),
                None => return Read::Say(format!("--to wants a URL\n\n{USAGE}"), 2),
            },
            "--user" => match rest.next() {
                Some(value) => asked.user = Some(value.clone()),
                None => return Read::Say(format!("--user wants a name\n\n{USAGE}"), 2),
            },
            "--content" => match rest.next() {
                Some(value) => asked.content = PathBuf::from(value),
                None => return missing("--content"),
            },
            "--at" => match rest.next() {
                Some(value) => asked.node = value.clone(),
                None => return missing("--at"),
            },
            "--namespace" => match rest.next() {
                Some(value) => asked.namespace = value.clone(),
                None => return missing("--namespace"),
            },
            "--bind" => match rest.next() {
                Some(value) => asked.bind = value.clone(),
                None => return missing("--bind"),
            },
            other => {
                // Named rather than ignored. A mistyped flag that is skipped
                // runs the command with a default the caller did not want and
                // says nothing, which is the failure this refusal prevents.
                return Read::Say(format!("{other} is not an option\n\n{USAGE}"), 2);
            }
        }
    }

    if asked.ingest_first && task != Task::Serve {
        return Read::Say(
            "--ingest belongs to serve; ingest already does\n".to_owned(),
            2,
        );
    }
    Read::Do(Box::new(asked))
}

fn setting(environment: &dyn Fn(&str) -> Option<String>, name: &str, fallback: &str) -> String {
    environment(name)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| fallback.to_owned())
}

fn missing(flag: &str) -> Read {
    Read::Say(format!("{flag} wants a value\n\n{USAGE}"), 2)
}

#[cfg(test)]
#[allow(clippy::panic, clippy::expect_used)]
mod tests {
    use super::{Asked, Read, Task, read};
    use std::path::PathBuf;

    fn nothing(_: &str) -> Option<String> {
        None
    }

    fn asked(words: &[&str]) -> Result<Asked, String> {
        let words: Vec<String> = words.iter().map(|word| (*word).to_owned()).collect();
        match read(&words, &nothing) {
            Read::Do(asked) => Ok(*asked),
            Read::Say(said, _) => Err(said),
        }
    }

    #[test]
    fn a_bare_verb_carries_every_default() {
        let held = asked(&["serve"]).expect("serve");
        assert_eq!(held.task, Task::Serve);
        assert_eq!(held.content, PathBuf::from("content"));
        assert_eq!(held.node, "127.0.0.1:7654");
        assert_eq!(held.namespace, "v0_0_2_alpha");
        assert_eq!(held.bind, "127.0.0.1:8080");
        assert!(!held.ingest_first);
    }

    #[test]
    fn the_environment_supplies_the_defaults_and_a_flag_beats_it() {
        let environment = |name: &str| match name {
            "TESSARIDB_ADDRESS" => Some("db:7654".to_owned()),
            "DOCS_NAMESPACE" => Some("v0_2_0".to_owned()),
            "DOCS_BIND" => Some("0.0.0.0:8080".to_owned()),
            _ => None,
        };
        let words = [
            "serve".to_owned(),
            "--namespace".to_owned(),
            "v9".to_owned(),
        ];
        let Read::Do(held) = read(&words, &environment) else {
            panic!("parses")
        };
        assert_eq!(held.node, "db:7654", "from the environment");
        assert_eq!(held.bind, "0.0.0.0:8080", "from the environment");
        assert_eq!(held.namespace, "v9", "the flag wins");
    }

    #[test]
    fn an_environment_variable_set_to_nothing_is_the_same_as_unset() {
        // A compose file that writes `DOCS_BIND=` should get the default, not an
        // empty address that fails to bind with a message about "".
        let environment = |name: &str| (name == "DOCS_BIND").then(|| "  ".to_owned());
        let words = ["serve".to_owned()];
        let Read::Do(held) = read(&words, &environment) else {
            panic!("parses")
        };
        assert_eq!(held.bind, "127.0.0.1:8080");
    }

    #[test]
    fn a_mistyped_flag_is_named_rather_than_skipped() {
        let complaint = asked(&["serve", "--bnid", "x"]).expect_err("a typo");
        assert!(
            complaint.starts_with("--bnid is not an option"),
            "{complaint}"
        );
    }

    #[test]
    fn a_flag_with_no_value_does_not_swallow_the_next_one() {
        assert!(asked(&["ingest", "--at"]).is_err());
        assert!(asked(&["ingest", "--namespace"]).is_err());
    }

    #[test]
    fn ingest_first_belongs_to_serve_alone() {
        assert!(asked(&["serve", "--ingest"]).expect("serve").ingest_first);
        let complaint = asked(&["ingest", "--ingest"]).expect_err("refused");
        assert!(complaint.contains("belongs to serve"), "{complaint}");
    }

    #[test]
    fn help_and_version_leave_with_nothing_wrong() {
        let words = ["--help".to_owned()];
        assert_eq!(
            read(&words, &nothing),
            Read::Say(super::USAGE.to_owned(), 0)
        );
        let words = ["--version".to_owned()];
        let Read::Say(said, code) = read(&words, &nothing) else {
            panic!("says")
        };
        assert_eq!(code, 0);
        assert!(said.starts_with("docs 0."), "{said}");
    }

    #[test]
    fn no_arguments_at_all_is_the_usage_and_a_failure() {
        assert_eq!(read(&[], &nothing), Read::Say(super::USAGE.to_owned(), 2));
    }
}
