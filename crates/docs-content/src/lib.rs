//! Markdown in, a page and its searchable fragments out.
//!
//! This crate holds no database handle and opens no socket. That is deliberate:
//! the rules that decide what a reader can find — where a section ends, which
//! anchor a result points at, what text is indexed — are the ones with subtle
//! failures, and they should be provable without a running node. Everything that
//! needs a node lives in `docs-store`.
//!
//! ```
//! let source = "+++\ntitle = \"Full-text search\"\n+++\n\n## Analyzers\n\nAn analyzer belongs to the field.\n";
//! let page = docs_content::parse("query-language/search", source).expect("a page");
//! assert_eq!(page.title, "Full-text search");
//! assert_eq!(page.fragments.len(), 1);
//! assert!(page.fragments[0].text.starts_with("Full-text search"));
//! ```

pub mod anchor;
pub mod fragment;
pub mod front;
pub mod render;

pub use crate::anchor::slug;
pub use crate::fragment::{Fragment, Heading};
pub use crate::front::FrontMatter;
pub use crate::render::to_html;

/// What can be wrong with a source file.
///
/// Each of these stops the ingest for that file and names it, rather than
/// producing a page with a missing part. A documentation site that silently
/// drops a page is worse than one that refuses to build.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Fault {
    /// The file does not open with a `+++` fence.
    #[error("no front matter: the file must open with a `+++` line")]
    NoFrontMatter,

    /// The file opens a front-matter block and never closes it.
    #[error("the front matter opens with `+++` and is never closed")]
    UnclosedFrontMatter,

    /// The block is not the TOML this expects.
    #[error("front matter: {0}")]
    FrontMatter(String),
}

/// A page, as the store will hold it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    /// The path this page answers, without a leading slash: `query-language/search`.
    pub slug: String,
    /// Everything the page declared about itself.
    pub front: FrontMatter,
    /// The title, lifted out of the front matter because every caller wants it.
    pub title: String,
    /// The Markdown body, with the front matter removed. Rendered at read time.
    pub markdown: String,
    /// The right-hand outline.
    pub headings: Vec<Heading>,
    /// The units of search.
    pub fragments: Vec<Fragment>,
}

/// Reads one source file into a [`Page`].
///
/// # The section comes from the slug when the front matter does not name one
///
/// A page is placed in the tree by a `holds` edge from its section, and that
/// edge is written only when the page knows which section it belongs to. The
/// **disk** path supplied that from the directory the file was found in, so
/// almost no page in `content/` declares `section` — and the **API** path had
/// nothing to supply it from, so every page written through the API was stored
/// correctly, served correctly, and vanished from the navigation.
///
/// It is derived here rather than at each caller so the two paths agree by
/// construction. A slug already carries the answer: `query-language/graphs`
/// belongs to `query-language`, and a slug with no `/` — the home page — belongs
/// to no section, which is why it is absent from the tree and correct to be.
///
/// An explicitly declared section still wins, so the corpus reader's
/// mismatch check keeps its subject.
///
/// # Errors
///
/// Returns the [`Fault`] that stopped it — see that type. A file that parses
/// produces a page with every part present.
pub fn parse(slug: &str, source: &str) -> Result<Page, Fault> {
    let (mut front, markdown) = front::split(source)?;
    let (headings, fragments) = fragment::split(&front.title, markdown);
    if front.section.is_none() {
        front.section = section_of(slug);
    }
    Ok(Page {
        slug: slug.to_owned(),
        title: front.title.clone(),
        front,
        markdown: markdown.to_owned(),
        headings,
        fragments,
    })
}

/// The section a slug sits in, or nothing when it sits at the root.
///
/// The **immediate** parent, because the tree nests sections inside sections:
/// `a/b/c` is held by `a/b`, which is in turn held by `a`.
fn section_of(slug: &str) -> Option<String> {
    let (section, _) = slug.rsplit_once('/')?;
    (!section.is_empty()).then(|| section.to_owned())
}

#[cfg(test)]
mod tests {
    use super::{Fault, parse};

    #[test]
    fn a_page_that_declares_no_section_takes_it_from_its_slug() {
        // The defect this closes: a page written through the API was stored and
        // served but never linked into the tree, because nothing told it which
        // section it belonged to. The disk path took that from the directory;
        // the API had no directory. The slug has the answer either way.
        let source = "+++\ntitle = \"Stream ingestion\"\norder = 58\n+++\n\nbody\n";
        let page = parse("query-language/stream-ingestion", source).expect("a page");
        assert_eq!(page.front.section.as_deref(), Some("query-language"));
    }

    #[test]
    fn a_declared_section_still_wins_so_the_mismatch_check_keeps_its_subject() {
        // The corpus reader refuses a page whose front matter names a section
        // other than the directory it was found in. Deriving over the top of a
        // declaration would make that check unable to fire.
        let source = "+++\ntitle = \"Records\"\nsection = \"guides\"\n+++\n\nbody\n";
        let page = parse("query-language/records", source).expect("a page");
        assert_eq!(page.front.section.as_deref(), Some("guides"));
    }

    #[test]
    fn a_page_at_the_root_belongs_to_no_section() {
        // The home page. It is absent from the tree on purpose — it is not a
        // link in its own navigation — and this is why that is correct rather
        // than the same bug wearing a different hat.
        let source = "+++\ntitle = \"TessariDB\"\n+++\n\nbody\n";
        let page = parse("index", source).expect("a page");
        assert_eq!(page.front.section, None);
    }

    #[test]
    fn a_nested_page_belongs_to_its_immediate_section() {
        let source = "+++\ntitle = \"Deep\"\n+++\n\nbody\n";
        let page = parse("a/b/c", source).expect("a page");
        assert_eq!(page.front.section.as_deref(), Some("a/b"));
    }

    #[test]
    fn a_page_arrives_with_every_part_present() {
        let source = "+++\ntitle = \"Graphs\"\nsection = \"query-language\"\norder = 60\n+++\n\nEdges are records.\n\n## RELATE\n\nA relation is written once.\n";
        let page = parse("query-language/graphs", source).expect("a page");
        assert_eq!(page.slug, "query-language/graphs");
        assert_eq!(page.title, "Graphs");
        assert_eq!(page.front.section.as_deref(), Some("query-language"));
        assert_eq!(page.front.order, 60);
        assert_eq!(page.headings.len(), 1);
        assert_eq!(page.fragments.len(), 2, "the lead and the section");
        assert!(page.markdown.starts_with("Edges are records."));
    }

    #[test]
    fn a_file_that_cannot_be_read_is_refused_rather_than_half_built() {
        assert_eq!(parse("x", "# no front matter\n"), Err(Fault::NoFrontMatter));
    }
}
