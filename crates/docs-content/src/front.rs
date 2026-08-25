//! The block at the top of a page that says what the page is.
//!
//! Delimited by `+++` and written in TOML. TOML rather than YAML because the
//! maintained YAML crates in this ecosystem are a moving target and because a
//! page's front matter is a handful of scalars — the place where YAML earns its
//! complexity is not here.
//!
//! Everything the navigation needs comes from here rather than from a separate
//! sidebar file, so that adding a page is one file and not two, and so that a
//! page can never be present in one and absent from the other.

use serde::Deserialize;

use crate::Fault;

/// What a page declares about itself.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct FrontMatter {
    /// The page's title, as it appears in the tree, the tab and the search result.
    pub title: String,

    /// One sentence under the title, and the fallback snippet for a search hit
    /// that matched the lead rather than a heading.
    #[serde(default)]
    pub summary: Option<String>,

    /// The section this page belongs to, as a section slug. Absent means the
    /// root of the tree.
    #[serde(default)]
    pub section: Option<String>,

    /// Position among siblings. Equal values fall back to title order, so a set
    /// of pages that all forget to say is at least stable.
    #[serde(default)]
    pub order: i64,

    /// An icon name from this project's own set, for the few entries that carry one.
    #[serde(default)]
    pub icon: Option<String>,

    /// A page that describes something the engine does not do yet says so here,
    /// and the page renders a notice. LR-DOCS-002 in a field: the alternative is
    /// a claim in prose that nothing checks.
    #[serde(default)]
    pub unreleased: bool,
}

/// Splits a source file into its front matter and its body.
///
/// # Errors
///
/// Returns [`Fault::NoFrontMatter`] when the file does not open with a `+++`
/// fence, [`Fault::UnclosedFrontMatter`] when it opens one and never closes it,
/// and [`Fault::FrontMatter`] when the block is not the TOML this expects.
pub fn split(source: &str) -> Result<(FrontMatter, &str), Fault> {
    let after_open = source
        .strip_prefix("+++\n")
        .or_else(|| source.strip_prefix("+++\r\n"))
        .ok_or(Fault::NoFrontMatter)?;

    // Searching for the fence at a line start, not anywhere: `+++` inside a
    // fenced code block in the front matter would otherwise end it early.
    let close = after_open
        .match_indices("\n+++")
        .find(|(at, _)| {
            let rest = after_open.get(at.saturating_add(4)..).unwrap_or("");
            rest.is_empty() || rest.starts_with('\n') || rest.starts_with("\r\n")
        })
        .map(|(at, _)| at)
        .ok_or(Fault::UnclosedFrontMatter)?;

    let block = after_open.get(..close).unwrap_or("");
    let body = after_open
        .get(close.saturating_add(4)..)
        .unwrap_or("")
        .trim_start_matches(['\r', '\n']);

    let front = toml::from_str(block).map_err(|fault| Fault::FrontMatter(fault.to_string()))?;
    Ok((front, body))
}

#[cfg(test)]
mod tests {
    use super::split;
    use crate::Fault;

    const PAGE: &str = "+++\ntitle = \"Full-text search\"\nsummary = \"Terms, not patterns.\"\nsection = \"query-language\"\norder = 40\n+++\n\n# Full-text search\n\nA term is a whole word.\n";

    #[test]
    fn the_declared_fields_come_back_and_the_body_starts_at_the_markdown() {
        let (front, body) = split(PAGE).expect("splits");
        assert_eq!(front.title, "Full-text search");
        assert_eq!(front.summary.as_deref(), Some("Terms, not patterns."));
        assert_eq!(front.section.as_deref(), Some("query-language"));
        assert_eq!(front.order, 40);
        assert!(!front.unreleased);
        assert!(body.starts_with("# Full-text search"), "{body:?}");
    }

    #[test]
    fn the_optional_fields_have_defaults_so_a_short_page_is_legal() {
        let (front, body) = split("+++\ntitle = \"Index\"\n+++\ntext\n").expect("splits");
        assert_eq!(front.title, "Index");
        assert_eq!(front.summary, None);
        assert_eq!(front.order, 0);
        assert_eq!(body, "text\n");
    }

    #[test]
    fn a_fence_inside_the_block_does_not_end_it_early() {
        // The reason the close is matched at a line start with a line end after
        // it, rather than by the first `+++` anywhere.
        let source = "+++\ntitle = \"Signs\"\nsummary = \"a +++ b\"\n+++\nbody\n";
        let (front, body) = split(source).expect("splits");
        assert_eq!(front.summary.as_deref(), Some("a +++ b"));
        assert_eq!(body, "body\n");
    }

    #[test]
    fn a_page_with_no_front_matter_is_a_fault_and_not_an_empty_title() {
        // Defaulting here would put a page called "" in the tree and leave the
        // author looking for it.
        assert!(matches!(
            split("# Just markdown\n"),
            Err(Fault::NoFrontMatter)
        ));
    }

    #[test]
    fn an_unclosed_block_is_named_as_such_rather_than_read_as_toml() {
        assert!(matches!(
            split("+++\ntitle = \"Open\"\n"),
            Err(Fault::UnclosedFrontMatter)
        ));
    }

    #[test]
    fn a_missing_title_is_refused_because_nothing_downstream_can_supply_one() {
        assert!(matches!(
            split("+++\norder = 3\n+++\nbody\n"),
            Err(Fault::FrontMatter(_))
        ));
    }
}
