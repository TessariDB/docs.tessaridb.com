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
/// # Errors
///
/// Returns the [`Fault`] that stopped it — see that type. A file that parses
/// produces a page with every part present.
pub fn parse(slug: &str, source: &str) -> Result<Page, Fault> {
    let (front, markdown) = front::split(source)?;
    let (headings, fragments) = fragment::split(&front.title, markdown);
    Ok(Page {
        slug: slug.to_owned(),
        title: front.title.clone(),
        front,
        markdown: markdown.to_owned(),
        headings,
        fragments,
    })
}

#[cfg(test)]
mod tests {
    use super::{Fault, parse};

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
