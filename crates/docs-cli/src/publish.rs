//! `docs publish` — writing the corpus to a site over its own API.
//!
//! # Why this exists
//!
//! The other way to move a corpus is `docs ingest`, which runs **beside the
//! store**: it needs the machine, which in a deployment means a shell on the
//! host that runs the database. That makes editing a page a privilege somebody
//! holds over the whole machine, and it makes every runbook describing the
//! procedure a document about infrastructure rather than about publishing.
//!
//! This does the same job from a laptop, over the API the front end already
//! uses, with a token the API already issues. Nothing on the far side is
//! touched but the routes any client may call.
//!
//! # It says what it will do, and then does nothing
//!
//! A publish is a **dry run** unless `--apply` is given. That is not caution for
//! its own sake: the corpus on disk and the corpus in the store drift — pages
//! are edited through the API too — and the useful question before a publish is
//! *which pages would change*, which nothing else can answer.
//!
//! "Changed" here means **what a reader would see changes**. The comparison
//! renders the local Markdown with the same renderer the site uses and compares
//! that to what the site is serving, so a reformatted paragraph that renders
//! identically is not a change and a one-word edit is.
//!
//! # Removal is never a side effect of absence
//!
//! A page in the store and not on disk is *reported* and left alone. It is
//! removed only with `--prune`, and then only after being listed. `ingest`
//! replaces the whole corpus, which is right for seeding an empty store and
//! wrong for a site people edit: a stale checkout would silently delete a
//! month of work.

use std::collections::BTreeMap;
use std::path::Path;

use docs_store::ingest::Section;

use crate::arguments::Asked;
use crate::corpus;

/// Where the password is read from.
///
/// Not an argument, for the same reason it is not one anywhere else in this
/// project: an argument is in the process table for anybody on the machine to
/// read, and in the shell history afterwards.
pub const PASSWORD: &str = "DOCS_PUBLISH_PASSWORD";

/// What a publish would do, page by page.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Plan {
    /// Pages the site does not have.
    pub added: Vec<String>,
    /// Pages whose rendering would change.
    pub changed: Vec<String>,
    /// Pages that are identical and will be skipped.
    pub unchanged: Vec<String>,
    /// Pages the site has and the corpus does not.
    pub absent: Vec<String>,
}

impl Plan {
    /// Whether applying this would write anything.
    #[must_use]
    pub fn writes(&self) -> bool {
        !self.added.is_empty() || !self.changed.is_empty()
    }

    /// The plan as lines to print, most consequential first.
    #[must_use]
    pub fn lines(&self, pruning: bool) -> Vec<String> {
        let mut said = Vec::new();
        for slug in &self.added {
            said.push(format!("  new      /{slug}"));
        }
        for slug in &self.changed {
            said.push(format!("  changed  /{slug}"));
        }
        for slug in &self.absent {
            // Named either way. A page about to be deleted and a page about to
            // be left behind are both things the person running this needs to
            // see, and the word is the only difference between them.
            said.push(if pruning {
                format!("  REMOVED  /{slug}")
            } else {
                format!("  on the site only, kept  /{slug}  (--prune removes it)")
            });
        }
        said.push(format!(
            "  {} new, {} changed, {} unchanged, {} on the site only",
            self.added.len(),
            self.changed.len(),
            self.unchanged.len(),
            self.absent.len()
        ));
        said
    }
}

/// Compare what the corpus holds against what the site is serving.
///
/// `local` maps a slug to the HTML that slug would render to; `remote` maps a
/// slug to the HTML the site is serving for it. Taking both as already-rendered
/// text is what makes this testable without a network: the interesting logic is
/// the set arithmetic and it should not need a server to exercise.
#[must_use]
pub fn compare(local: &BTreeMap<String, String>, remote: &BTreeMap<String, String>) -> Plan {
    let mut plan = Plan::default();
    for (slug, rendering) in local {
        match remote.get(slug) {
            None => plan.added.push(slug.clone()),
            Some(serving) if serving == rendering => plan.unchanged.push(slug.clone()),
            Some(_) => plan.changed.push(slug.clone()),
        }
    }
    for slug in remote.keys() {
        if !local.contains_key(slug) {
            plan.absent.push(slug.clone());
        }
    }
    plan
}

/// The slugs a nav tree holds, flattened.
///
/// Only the pages. A section is not a page and cannot be compared with one —
/// they are separate routes on the API and separate rows in the store.
pub fn pages_in(tree: &[Node]) -> Vec<String> {
    let mut found = Vec::new();
    gather(tree, &mut found);
    found
}

fn gather(nodes: &[Node], into: &mut Vec<String>) {
    for node in nodes {
        if node.kind == "page" {
            into.push(node.slug.clone());
        }
        gather(&node.children, into);
    }
}

/// One entry in the site's navigation tree, as the API answers it.
///
/// Declared here rather than borrowed from `docs-store` because this is a
/// *client*: what it must agree with is the JSON on the wire, and a type shared
/// with the server would let a change to the server's internals look like
/// agreement it had not actually kept.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Node {
    /// A section's own slug, or a page's path.
    pub slug: String,
    /// `section` or `page`.
    pub kind: String,
    /// Sections and pages beneath this one.
    #[serde(default)]
    pub children: Vec<Node>,
}

/// A page as the site is serving it.
#[derive(Debug, Clone, serde::Deserialize)]
struct Serving {
    html: String,
}

/// What `PUT /api/section/{slug}` takes.
#[derive(Debug, serde::Serialize)]
struct SectionBody<'a> {
    title: &'a str,
    parent: Option<&'a str>,
    order: i64,
    icon: Option<&'a str>,
}

/// Publish the corpus to the site named on the command line.
///
/// # Errors
///
/// Returns a message when the corpus does not parse, the site cannot be
/// reached, the credential is refused, or any write is refused.
pub async fn publish(asked: &Asked) -> Result<(), String> {
    let site = asked.site.trim_end_matches('/').to_owned();
    if site.is_empty() {
        return Err("publish wants --to <https://host> — the site to write to".to_owned());
    }
    let user = asked
        .user
        .clone()
        .ok_or_else(|| format!("publish wants --user <name>, and the password in {PASSWORD}"))?;
    let password = std::env::var(PASSWORD)
        .ok()
        .filter(|held| !held.is_empty())
        .ok_or_else(|| format!("{PASSWORD} is not set"))?;

    // Read and validate before anything is sent. A corpus that does not parse
    // is a corpus that must not be half-published, and finding that out after
    // twenty pages have been written is the worst moment to find it out.
    let corpus = corpus::read(&asked.content).map_err(|fault| fault.to_string())?;
    log::info!(
        "{} sections, {} pages in {}",
        corpus.sections.len(),
        corpus.pages.len(),
        asked.content.display()
    );

    let client = reqwest::Client::builder()
        .user_agent(concat!("docs/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|fault| format!("could not start a client: {fault}"))?;

    let token = open_session(&client, &site, &user, &password).await?;
    // From here on every exit hands the token back rather than leaving it live
    // until it expires — a publish that ran from somebody's laptop should not
    // leave a credential behind on the site.
    let outcome = run(&client, &site, &token, asked, &corpus).await;
    close_session(&client, &site, &token).await;
    outcome
}

/// The publish itself, between opening the session and handing it back.
async fn run(
    client: &reqwest::Client,
    site: &str,
    token: &str,
    asked: &Asked,
    corpus: &docs_store::ingest::Corpus,
) -> Result<(), String> {
    let mut local = BTreeMap::new();
    for page in &corpus.pages {
        local.insert(page.slug.clone(), docs_content::to_html(&page.markdown));
    }

    // Asked page by page rather than read out of the navigation tree. The tree
    // holds what the tree *shows*, and the home page is not a link in its own
    // navigation — deriving the remote set from it reported `/index` as new on
    // every publish and rewrote it every time.
    let mut remote = BTreeMap::new();
    for slug in local.keys() {
        if let Ok(serving) = fetch::<Serving>(client, &format!("{site}/api/page/{slug}")).await {
            remote.insert(slug.clone(), serving.html);
        }
    }
    // The tree is still what answers the other direction — a page the site has
    // and the corpus does not, which by definition is not in `local` to ask
    // about. A page in the tree that will not load is left out rather than
    // crashing the publish: the tree and the pages are separate rows and one
    // can outlive the other.
    let tree: Vec<Node> = fetch(client, &format!("{site}/api/nav")).await?;
    for slug in pages_in(&tree) {
        if local.contains_key(&slug) || remote.contains_key(&slug) {
            continue;
        }
        if let Ok(serving) = fetch::<Serving>(client, &format!("{site}/api/page/{slug}")).await {
            remote.insert(slug, serving.html);
        }
    }

    let plan = compare(&local, &remote);
    println!("{site}");
    for line in plan.lines(asked.prune) {
        println!("{line}");
    }

    if !asked.apply {
        println!("\nnothing was written — add --apply to do it");
        return Ok(());
    }
    if !plan.writes() && !(asked.prune && !plan.absent.is_empty()) {
        println!("\nnothing to do");
        return Ok(());
    }

    // Sections first. A page whose section does not exist yet is a page the
    // tree cannot place, and the order here is the only thing that prevents it.
    for section in &corpus.sections {
        put_section(client, site, token, section).await?;
    }
    for slug in plan.added.iter().chain(plan.changed.iter()) {
        let source = std::fs::read_to_string(source_of(&asked.content, slug))
            .map_err(|fault| format!("/{slug}: {fault}"))?;
        put_page(client, site, token, slug, &source).await?;
        println!("  wrote    /{slug}");
    }
    if asked.prune {
        for slug in &plan.absent {
            delete_page(client, site, token, slug).await?;
            println!("  removed  /{slug}");
        }
    }
    println!("\ndone");
    Ok(())
}

/// The file a slug was read from.
///
/// The exact inverse of how `corpus` derives a slug — the path under the content
/// root, with `.md` back on.
fn source_of(root: &Path, slug: &str) -> std::path::PathBuf {
    root.join(slug).with_extension("md")
}

/// Sign in, and answer with the token.
async fn open_session(
    client: &reqwest::Client,
    site: &str,
    user: &str,
    password: &str,
) -> Result<String, String> {
    #[derive(serde::Deserialize)]
    struct Issued {
        token: String,
    }
    let reply = client
        .post(format!("{site}/api/session"))
        // Basic, because that is what the route reads. The password is spent
        // here once and a token carries the rest of the publish.
        .basic_auth(user, Some(password))
        .send()
        .await
        .map_err(|fault| format!("{site}: {fault}"))?;
    let status = reply.status();
    let body = reply.text().await.unwrap_or_default();
    if !status.is_success() {
        // The site's own words. A publish refused for a reason the site
        // explained should not have that explanation replaced by a guess.
        return Err(format!("{site} refused the sign-in: {status} {body}"));
    }
    let issued: Issued = serde_json::from_str(&body)
        .map_err(|fault| format!("{site} answered with no token: {fault}"))?;
    Ok(issued.token)
}

/// Hand the token back. Failures are logged, never raised.
///
/// The publish has already happened or already failed by the time this runs, and
/// turning "the token could not be revoked" into the command's exit status would
/// report a successful publish as a failure.
async fn close_session(client: &reqwest::Client, site: &str, token: &str) {
    let sent = client
        .delete(format!("{site}/api/session"))
        .bearer_auth(token)
        .send()
        .await;
    if let Err(fault) = sent {
        log::warn!("the session could not be handed back, and will expire: {fault}");
    }
}

/// A `GET` answering JSON.
async fn fetch<T: serde::de::DeserializeOwned>(
    client: &reqwest::Client,
    url: &str,
) -> Result<T, String> {
    let reply = client
        .get(url)
        .send()
        .await
        .map_err(|fault| format!("{url}: {fault}"))?;
    let status = reply.status();
    let body = reply.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!("{url}: {status} {body}"));
    }
    serde_json::from_str(&body).map_err(|fault| format!("{url}: {fault}"))
}

async fn put_page(
    client: &reqwest::Client,
    site: &str,
    token: &str,
    slug: &str,
    source: &str,
) -> Result<(), String> {
    written(
        client
            .put(format!("{site}/api/page/{slug}"))
            .bearer_auth(token)
            .header("Content-Type", "text/plain; charset=utf-8")
            .body(source.to_owned()),
        &format!("/{slug}"),
    )
    .await
}

async fn put_section(
    client: &reqwest::Client,
    site: &str,
    token: &str,
    section: &Section,
) -> Result<(), String> {
    written(
        client
            .put(format!("{site}/api/section/{}", section.slug))
            .bearer_auth(token)
            .json(&SectionBody {
                title: &section.title,
                parent: section.parent.as_deref(),
                order: section.order,
                icon: section.icon.as_deref(),
            }),
        &format!("section {}", section.slug),
    )
    .await
}

async fn delete_page(
    client: &reqwest::Client,
    site: &str,
    token: &str,
    slug: &str,
) -> Result<(), String> {
    written(
        client
            .delete(format!("{site}/api/page/{slug}"))
            .bearer_auth(token),
        &format!("/{slug}"),
    )
    .await
}

/// Send a write and turn anything but success into the site's own message.
async fn written(request: reqwest::RequestBuilder, what: &str) -> Result<(), String> {
    let reply = request
        .send()
        .await
        .map_err(|fault| format!("{what}: {fault}"))?;
    let status = reply.status();
    if status.is_success() {
        return Ok(());
    }
    let body = reply.text().await.unwrap_or_default();
    Err(format!("{what}: {status} {body}"))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{Node, compare, pages_in, source_of};

    fn corpus(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
        entries
            .iter()
            .map(|(slug, html)| ((*slug).to_owned(), (*html).to_owned()))
            .collect()
    }

    #[test]
    fn a_page_the_site_lacks_is_new_and_one_that_renders_the_same_is_not_a_change() {
        let plan = compare(
            &corpus(&[("a", "<p>one</p>"), ("b", "<p>two</p>")]),
            &corpus(&[("a", "<p>one</p>")]),
        );
        assert_eq!(plan.added, ["b"]);
        assert_eq!(plan.unchanged, ["a"]);
        assert!(plan.changed.is_empty());
        assert!(plan.absent.is_empty());
    }

    #[test]
    fn a_different_rendering_is_a_change() {
        let plan = compare(
            &corpus(&[("a", "<p>one</p>")]),
            &corpus(&[("a", "<p>ONE</p>")]),
        );
        assert_eq!(plan.changed, ["a"]);
        assert!(plan.unchanged.is_empty());
    }

    #[test]
    fn a_page_only_the_site_has_is_reported_and_not_written_off() {
        // The property that makes this safe to run from a stale checkout: a
        // page edited through the API and never pulled down is *named*, not
        // deleted, and `writes()` does not count it.
        let plan = compare(&corpus(&[]), &corpus(&[("gone", "<p>x</p>")]));
        assert_eq!(plan.absent, ["gone"]);
        assert!(!plan.writes());
    }

    #[test]
    fn what_is_kept_and_what_is_removed_read_differently() {
        let plan = compare(&corpus(&[]), &corpus(&[("gone", "<p>x</p>")]));
        let kept = plan.lines(false).join("\n");
        let pruned = plan.lines(true).join("\n");
        assert!(kept.contains("kept"), "{kept}");
        assert!(kept.contains("--prune"), "{kept}");
        assert!(pruned.contains("REMOVED"), "{pruned}");
        assert!(!pruned.contains("kept"), "{pruned}");
    }

    #[test]
    fn an_unchanged_corpus_writes_nothing() {
        let both = corpus(&[("a", "<p>one</p>")]);
        assert!(!compare(&both, &both).writes());
    }

    #[test]
    fn only_pages_are_compared_and_sections_are_not() {
        // They are separate routes and separate rows; a section slug arriving
        // in the page comparison would look like a page the corpus had lost.
        let tree = vec![Node {
            slug: "security".to_owned(),
            kind: "section".to_owned(),
            children: vec![Node {
                slug: "security/users".to_owned(),
                kind: "page".to_owned(),
                children: Vec::new(),
            }],
        }];
        assert_eq!(pages_in(&tree), ["security/users"]);
    }

    #[test]
    fn a_slug_names_the_file_it_was_read_from() {
        let root = std::path::Path::new("content");
        assert_eq!(
            source_of(root, "security/users"),
            std::path::Path::new("content/security/users.md")
        );
        assert_eq!(
            source_of(root, "index"),
            std::path::Path::new("content/index.md")
        );
    }
}
