//! `content/` on disk, read into the corpus the store ingests.
//!
//! # The tree is the directory tree
//!
//! A section is a directory and a page is a `.md` file, so the left-hand
//! navigation and the file layout are the same fact and cannot disagree. The
//! alternative — a sidebar file listing what should appear — is one more thing
//! to keep in step, and the day it slips a page exists and is unreachable.
//!
//! A directory says what it is in `_section.toml`. A directory that holds pages
//! and does not carry one is **refused** rather than given a title guessed from
//! its name: a guessed title reads as deliberate and nobody goes looking for it.
//!
//! # Two ways to name a section, and only one of them wins
//!
//! A page's front matter may carry `section`, because a page written through the
//! API has no directory to be in. For a page read from disk the directory is the
//! answer, and front matter naming a *different* section is a fault. Picking one
//! silently would put the page somewhere the author cannot see from the file
//! tree, which is the one place they will look.

use std::path::{Path, PathBuf};

use docs_content::Page;
use docs_store::ingest::{Corpus, Section};
use serde::Deserialize;

/// The file a directory declares itself in.
const DECLARATION: &str = "_section.toml";

/// What can be wrong with the content tree.
///
/// Each names the file, because the reader of this message is about to open it.
#[derive(Debug, thiserror::Error)]
pub enum Fault {
    /// The content directory is not where it was said to be.
    #[error("no content directory at {0}")]
    NoContent(PathBuf),

    /// A file or directory would not open.
    #[error("{path}: {fault}")]
    Unreadable {
        /// The file.
        path: PathBuf,
        /// What the filesystem said.
        fault: std::io::Error,
    },

    /// A page did not parse.
    #[error("{path}: {fault}")]
    Page {
        /// The file.
        path: PathBuf,
        /// What was wrong with it.
        fault: docs_content::Fault,
    },

    /// A directory holds content and does not say what it is.
    #[error("{0} holds content but has no {DECLARATION} saying what it is called")]
    Undeclared(PathBuf),

    /// A declaration is not the TOML this expects.
    #[error("{path}: {fault}")]
    Declaration {
        /// The declaration file.
        path: PathBuf,
        /// What was wrong with it.
        fault: String,
    },

    /// A page's front matter names a section other than the directory it is in.
    #[error("{path}: front matter says section = \"{declared}\", but the file is in {found}")]
    Misplaced {
        /// The file.
        path: PathBuf,
        /// What the front matter said.
        declared: String,
        /// Where the file actually is.
        found: String,
    },
}

/// What a directory declares about itself.
#[derive(Debug, Deserialize)]
struct Declaration {
    title: String,
    #[serde(default)]
    order: i64,
    #[serde(default)]
    icon: Option<String>,
}

/// Reads a content directory into a corpus.
///
/// Sections come out before the pages that sit in them, and both come out in
/// path order — not in the order the filesystem happened to hand them over, so
/// that two runs over the same tree write the same thing in the same sequence.
///
/// # Errors
///
/// Returns the first [`Fault`] found. It stops at the first rather than
/// collecting every one, because the faults here are usually one mistake seen
/// several times and a wall of them buries the first.
pub fn read(root: &Path) -> Result<Corpus, Fault> {
    if !root.is_dir() {
        return Err(Fault::NoContent(root.to_owned()));
    }
    let mut corpus = Corpus::default();
    walk(root, root, &mut corpus)?;
    Ok(corpus)
}

fn walk(root: &Path, here: &Path, corpus: &mut Corpus) -> Result<(), Fault> {
    let (directories, files) = entries(here)?;

    let slug = relative(root, here);
    if let Some(slug) = &slug {
        let declaration = here.join(DECLARATION);
        if !declaration.is_file() {
            return Err(Fault::Undeclared(here.to_owned()));
        }
        let source = std::fs::read_to_string(&declaration).map_err(|fault| Fault::Unreadable {
            path: declaration.clone(),
            fault,
        })?;
        let declared: Declaration =
            toml::from_str(&source).map_err(|fault| Fault::Declaration {
                path: declaration,
                fault: fault.to_string(),
            })?;
        corpus.sections.push(Section {
            slug: slug.clone(),
            title: declared.title,
            parent: here.parent().and_then(|above| relative(root, above)),
            order: declared.order,
            icon: declared.icon,
        });
    }

    for path in files {
        corpus.pages.push(page(&path, slug.as_deref(), root)?);
    }
    for path in directories {
        walk(root, &path, corpus)?;
    }
    Ok(())
}

/// One page, with its section taken from the directory it is in.
fn page(path: &Path, section: Option<&str>, root: &Path) -> Result<Page, Fault> {
    let source = std::fs::read_to_string(path).map_err(|fault| Fault::Unreadable {
        path: path.to_owned(),
        fault,
    })?;
    let slug = relative(root, &path.with_extension("")).unwrap_or_default();
    let mut page = docs_content::parse(&slug, &source).map_err(|fault| Fault::Page {
        path: path.to_owned(),
        fault,
    })?;

    match (page.front.section.as_deref(), section) {
        (Some(declared), Some(found)) if declared != found => {
            return Err(Fault::Misplaced {
                path: path.to_owned(),
                declared: declared.to_owned(),
                found: found.to_owned(),
            });
        }
        _ => {}
    }
    page.front.section = section.map(str::to_owned);
    Ok(page)
}

/// The directories and the `.md` files here, each sorted, and the declaration
/// left out of both because it is not a page.
type Entries = (Vec<PathBuf>, Vec<PathBuf>);

fn entries(here: &Path) -> Result<Entries, Fault> {
    let reading = std::fs::read_dir(here).map_err(|fault| Fault::Unreadable {
        path: here.to_owned(),
        fault,
    })?;
    let mut directories = Vec::new();
    let mut files = Vec::new();
    for entry in reading {
        let entry = entry.map_err(|fault| Fault::Unreadable {
            path: here.to_owned(),
            fault,
        })?;
        let path = entry.path();
        // A name beginning with a dot is the editor's business, not the site's.
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with('.'))
        {
            continue;
        }
        if path.is_dir() {
            directories.push(path);
        } else if path.extension().and_then(|kind| kind.to_str()) == Some("md") {
            files.push(path);
        }
    }
    directories.sort();
    files.sort();
    Ok((directories, files))
}

/// A path below `root` as a slug, or `None` for `root` itself.
///
/// Separators become `/` on every platform, because the slug is a record id and
/// a URL and neither of those is spelled with a backslash.
fn relative(root: &Path, path: &Path) -> Option<String> {
    let rest = path.strip_prefix(root).ok()?;
    let slug: Vec<String> = rest
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect();
    if slug.is_empty() {
        None
    } else {
        Some(slug.join("/"))
    }
}

#[cfg(test)]
#[allow(clippy::panic, clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::{Fault, read};
    use std::path::{Path, PathBuf};

    /// A content tree under a directory of this test's own, removed after.
    struct Tree(PathBuf);

    impl Tree {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!("docs-cli-{name}"));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("a temporary directory");
            Self(path)
        }

        fn write(&self, at: &str, contents: &str) -> &Self {
            let path = self.0.join(at);
            if let Some(above) = path.parent() {
                std::fs::create_dir_all(above).expect("a directory");
            }
            std::fs::write(path, contents).expect("a file");
            self
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Tree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    const SECTION: &str = "title = \"Query language\"\norder = 30\nicon = \"terminal\"\n";
    const PAGE: &str = "+++\ntitle = \"Records\"\n+++\n\nA record has an id.\n";

    #[test]
    fn a_directory_becomes_a_section_and_the_files_in_it_become_its_pages() {
        let tree = Tree::new("plain");
        tree.write("query-language/_section.toml", SECTION)
            .write("query-language/records.md", PAGE);

        let corpus = read(tree.path()).expect("reads");
        assert_eq!(corpus.sections.len(), 1);
        assert_eq!(corpus.sections[0].slug, "query-language");
        assert_eq!(corpus.sections[0].title, "Query language");
        assert_eq!(corpus.sections[0].order, 30);
        assert_eq!(corpus.sections[0].parent, None, "a top-level group");
        assert_eq!(corpus.pages.len(), 1);
        assert_eq!(corpus.pages[0].slug, "query-language/records");
        assert_eq!(
            corpus.pages[0].front.section.as_deref(),
            Some("query-language"),
            "taken from the directory, which the front matter did not name"
        );
    }

    #[test]
    fn a_subdirectory_is_a_subsection_and_names_its_parent() {
        let tree = Tree::new("nested");
        tree.write("query-language/_section.toml", SECTION)
            .write(
                "query-language/reads/_section.toml",
                "title = \"Reads\"\norder = 10\n",
            )
            .write("query-language/reads/select.md", PAGE);

        let corpus = read(tree.path()).expect("reads");
        let deep = corpus
            .sections
            .iter()
            .find(|section| section.slug == "query-language/reads")
            .expect("the subsection");
        assert_eq!(deep.parent.as_deref(), Some("query-language"));
        assert_eq!(corpus.pages[0].slug, "query-language/reads/select");
    }

    #[test]
    fn a_page_at_the_root_belongs_to_no_section() {
        let tree = Tree::new("root-page");
        tree.write("index.md", PAGE);
        let corpus = read(tree.path()).expect("reads");
        assert!(corpus.sections.is_empty());
        assert_eq!(corpus.pages[0].slug, "index");
        assert_eq!(corpus.pages[0].front.section, None);
    }

    #[test]
    fn a_directory_that_does_not_say_what_it_is_stops_the_read() {
        // Rather than a title guessed from the folder name: a guessed title
        // reads as deliberate, so nobody goes looking for the file that would
        // have set it.
        let tree = Tree::new("undeclared");
        tree.write("orphans/records.md", PAGE);
        assert!(matches!(read(tree.path()), Err(Fault::Undeclared(_)),));
    }

    #[test]
    fn front_matter_naming_another_section_is_a_fault_and_not_a_quiet_move() {
        let tree = Tree::new("misplaced");
        tree.write("query-language/_section.toml", SECTION).write(
            "query-language/records.md",
            "+++\ntitle = \"Records\"\nsection = \"guides\"\n+++\n\nbody\n",
        );
        let Err(Fault::Misplaced {
            declared, found, ..
        }) = read(tree.path())
        else {
            panic!("the mismatch should stop the read")
        };
        assert_eq!(declared, "guides");
        assert_eq!(found, "query-language");
    }

    #[test]
    fn a_page_that_will_not_parse_names_its_file() {
        let tree = Tree::new("unparsed");
        tree.write("index.md", "# no front matter\n");
        let Err(Fault::Page { path, .. }) = read(tree.path()) else {
            panic!("a page with no front matter should stop the read")
        };
        assert!(path.ends_with("index.md"), "{path:?}");
    }

    #[test]
    fn the_order_is_the_path_order_and_not_the_filesystems() {
        // Two runs must write the same sequence, or a rebuild is a diff against
        // itself and nobody can tell a real change from the order shifting.
        let tree = Tree::new("ordering");
        tree.write("a/_section.toml", "title = \"A\"\n")
            .write("a/z.md", PAGE)
            .write("a/m.md", PAGE)
            .write("b/_section.toml", "title = \"B\"\n")
            .write("b/c.md", PAGE);

        let corpus = read(tree.path()).expect("reads");
        let slugs: Vec<&str> = corpus.pages.iter().map(|page| page.slug.as_str()).collect();
        assert_eq!(slugs, ["a/m", "a/z", "b/c"]);
        let sections: Vec<&str> = corpus
            .sections
            .iter()
            .map(|section| section.slug.as_str())
            .collect();
        assert_eq!(sections, ["a", "b"]);
    }

    #[test]
    fn dotfiles_are_the_editors_business_and_not_the_sites() {
        let tree = Tree::new("dotfiles");
        tree.write("a/_section.toml", "title = \"A\"\n")
            .write("a/.keep.md", PAGE)
            .write("a/real.md", PAGE);
        let corpus = read(tree.path()).expect("reads");
        assert_eq!(corpus.pages.len(), 1);
        assert_eq!(corpus.pages[0].slug, "a/real");
    }

    #[test]
    fn a_content_directory_that_is_not_there_is_named_as_such() {
        assert!(matches!(
            read(Path::new("/nowhere/at/all")),
            Err(Fault::NoContent(_))
        ));
    }
}
