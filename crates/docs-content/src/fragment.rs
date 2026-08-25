//! A page cut into the pieces search actually returns.
//!
//! # Why a page is the wrong unit
//!
//! A reader searching `ANALYZER` wants the paragraph under *Full-text search*,
//! not the top of a four-thousand-word reference page. So the unit of search is
//! a **fragment** — the text under one heading, carrying that heading's anchor,
//! so a result links to the exact place rather than to the top of a page the
//! reader must then search again by eye.
//!
//! # Why the text of a fragment is composed rather than copied
//!
//! Ranking in the store is `search::score(field, terms)`, and it is refused over
//! a field with no search index. Two scores from two indexes measure a document
//! against two different collections, so they cannot honestly be merged into one
//! ordering — the numbers are not on the same scale and nothing about them says
//! so.
//!
//! The consequence is that everything a reader might match must live in **one**
//! field. So a fragment's `text` is composed here as the page's title, then the
//! heading, then the body. A title hit and a body hit then compete inside one
//! collection through one index, and the ranking means what it appears to mean.
//! Repeating the title in every fragment of a page is not redundancy, it is what
//! makes "the page about search" rank for `search`.

use pulldown_cmark::{Event, HeadingLevel, Parser, Tag, TagEnd};

use crate::anchor::Anchors;
use crate::render::options;

/// One heading-delimited piece of a page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fragment {
    /// The anchor to link to. Empty for the lead, which is the top of the page.
    pub anchor: String,
    /// The heading this piece sits under, or the page title for the lead.
    pub heading: String,
    /// Heading depth, 1 to 6. The lead is 0.
    pub depth: u8,
    /// Position within the page, so a result can say where in the page it is.
    pub order: i64,
    /// What the store indexes: page title, heading and body, in that order.
    pub text: String,
    /// The passage alone, for showing a reader.
    ///
    /// Separate from `text` because the two have different jobs. `text` carries
    /// the page title and the heading so that one index can rank a title hit
    /// against a body hit — and a snippet built from it therefore opens by
    /// repeating the title and the heading the result already displays above it,
    /// which reads as a stutter and pushes the words the reader searched for off
    /// the end of the line.
    pub body: String,
}

/// An entry in the right-hand outline.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Heading {
    /// Depth, 1 to 6.
    pub depth: u8,
    /// The heading as written.
    pub text: String,
    /// The anchor, the same one the matching fragment carries.
    pub anchor: String,
}

/// Cuts `markdown` into fragments and collects the outline.
///
/// `title` is the page's own title, prefixed to every fragment's text for the
/// reason the module documentation gives.
#[must_use]
pub fn split(title: &str, markdown: &str) -> (Vec<Heading>, Vec<Fragment>) {
    let mut anchors = Anchors::new();
    let mut headings = Vec::new();
    let mut fragments = Vec::new();

    // The piece being accumulated. It starts as the lead — whatever sits above
    // the first heading — which is real content on most pages and is the only
    // part of a page that no heading would otherwise cover.
    let mut current = Open::lead();
    let mut in_heading = false;
    let mut heading_text = String::new();

    // The same configuration the renderer uses, so that what is indexed and what
    // is displayed are one document read one way.
    for event in Parser::new_ext(markdown, options()) {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                in_heading = true;
                heading_text.clear();
                let _ = level;
            }
            Event::End(TagEnd::Heading(level)) => {
                in_heading = false;
                let anchor = anchors.of(&heading_text);
                let depth = depth_of(level);
                headings.push(Heading {
                    depth,
                    text: heading_text.clone(),
                    anchor: anchor.clone(),
                });
                // The previous piece ends where this heading begins.
                finish(current, title, &mut fragments);
                current = Open {
                    anchor,
                    heading: heading_text.clone(),
                    depth,
                    body: String::new(),
                };
            }
            Event::Text(text) | Event::Code(text) => {
                if in_heading {
                    heading_text.push_str(&text);
                } else {
                    push_spaced(&mut current.body, &text);
                }
            }
            Event::SoftBreak | Event::HardBreak if !in_heading => {
                push_spaced(&mut current.body, " ");
            }
            _ => {}
        }
    }
    finish(current, title, &mut fragments);

    for (position, fragment) in fragments.iter_mut().enumerate() {
        fragment.order = i64::try_from(position).unwrap_or(i64::MAX);
    }
    (headings, fragments)
}

/// A fragment still being accumulated.
struct Open {
    anchor: String,
    heading: String,
    depth: u8,
    body: String,
}

impl Open {
    fn lead() -> Self {
        Self {
            anchor: String::new(),
            heading: String::new(),
            depth: 0,
            body: String::new(),
        }
    }
}

/// Closes an open piece, dropping it when it holds nothing a reader could match.
///
/// A heading immediately followed by another heading — a section that only
/// groups its subsections — would otherwise become a fragment whose whole text
/// is the page title, and would then match every query that named the page.
fn finish(open: Open, title: &str, into: &mut Vec<Fragment>) {
    let body = open.body.trim();
    if body.is_empty() {
        // Dropped from search, not from the outline — the outline is collected
        // separately, so the heading still appears on the right and still has
        // an anchor. What it does not do is compete as a search result.
        return;
    }
    let mut text = String::with_capacity(
        title
            .len()
            .saturating_add(open.heading.len())
            .saturating_add(body.len())
            .saturating_add(2),
    );
    text.push_str(title);
    if !open.heading.is_empty() {
        text.push(' ');
        text.push_str(&open.heading);
    }
    if !body.is_empty() {
        text.push(' ');
        text.push_str(body);
    }
    into.push(Fragment {
        anchor: open.anchor,
        heading: if open.heading.is_empty() {
            title.to_owned()
        } else {
            open.heading
        },
        depth: open.depth,
        order: 0,
        text,
        body: body.to_owned(),
    });
}

/// Appends with a single separating space, so words never run together.
fn push_spaced(into: &mut String, text: &str) {
    if !into.is_empty() && !into.ends_with(' ') && !text.starts_with(' ') {
        into.push(' ');
    }
    into.push_str(text);
}

fn depth_of(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

#[cfg(test)]
mod tests {
    use super::split;

    const PAGE: &str = "Terms, not patterns.\n\n## Analyzers\n\nAn analyzer belongs to the field.\n\n### Filters\n\nLowercase and ascii.\n\n## Ranking\n\nA score measures one against the collection.\n";

    #[test]
    fn the_lead_is_kept_because_no_heading_covers_it() {
        let (_, fragments) = split("Full-text search", PAGE);
        let lead = fragments.first().expect("a lead");
        assert_eq!(lead.depth, 0);
        assert_eq!(lead.anchor, "");
        assert!(lead.text.contains("Terms, not patterns."), "{}", lead.text);
    }

    #[test]
    fn every_fragment_carries_the_page_title_so_one_index_can_rank_both() {
        // The property the module is built around: a title hit and a body hit
        // must be comparable, which they are only if they are the same field.
        let (_, fragments) = split("Full-text search", PAGE);
        assert!(!fragments.is_empty());
        for fragment in &fragments {
            assert!(
                fragment.text.starts_with("Full-text search"),
                "{:?} does not carry the title",
                fragment.text
            );
        }
    }

    #[test]
    fn a_fragment_holds_its_own_section_and_not_the_next_one() {
        let (_, fragments) = split("Full-text search", PAGE);
        let filters = fragments
            .iter()
            .find(|fragment| fragment.heading == "Filters")
            .expect("a Filters fragment");
        assert!(filters.text.contains("Lowercase and ascii."));
        assert!(
            !filters.text.contains("A score measures"),
            "it swallowed the next section: {}",
            filters.text
        );
    }

    #[test]
    fn the_outline_records_depth_and_the_anchor_the_fragment_uses() {
        let (headings, fragments) = split("Full-text search", PAGE);
        let depths: Vec<u8> = headings.iter().map(|heading| heading.depth).collect();
        assert_eq!(depths, vec![2, 3, 2]);
        // The outline and the search result have to agree, or a hit scrolls
        // somewhere the outline never points.
        for heading in &headings {
            assert!(
                fragments
                    .iter()
                    .any(|fragment| fragment.anchor == heading.anchor),
                "no fragment for {}",
                heading.anchor
            );
        }
    }

    #[test]
    fn a_heading_that_only_groups_its_subsections_is_not_a_fragment() {
        // Otherwise its whole text is the page title, and it matches every query
        // that names the page while carrying nothing a reader wanted.
        let (_, fragments) = split(
            "Reference",
            "## Statements\n\n### SELECT\n\nReads records.\n",
        );
        let headings: Vec<&str> = fragments
            .iter()
            .map(|fragment| fragment.heading.as_str())
            .collect();
        assert_eq!(headings, vec!["SELECT"]);
    }

    #[test]
    fn code_inside_a_page_is_searchable_because_it_is_what_readers_search_for() {
        let (_, fragments) = split(
            "Search",
            "## Score\n\nUse `search::score(body, 'ada')` here.\n",
        );
        let fragment = fragments.first().expect("one fragment");
        assert!(
            fragment.text.contains("search::score(body, 'ada')"),
            "{}",
            fragment.text
        );
    }

    #[test]
    fn words_across_a_line_break_do_not_run_together() {
        let (_, fragments) = split("Page", "## H\n\nan analyzer\nbelongs to the field\n");
        let text = &fragments.first().expect("one").text;
        assert!(text.contains("analyzer belongs"), "{text}");
    }

    #[test]
    fn the_shown_body_carries_no_title_and_no_heading() {
        // What a search result displays under its own heading line. Drawing it
        // from `text` instead would open every snippet by repeating the page
        // title and the heading printed directly above it.
        let (_, fragments) = split("Full-text search", PAGE);
        let ranking = fragments
            .iter()
            .find(|fragment| fragment.heading == "Ranking")
            .expect("a Ranking fragment");
        assert_eq!(ranking.body, "A score measures one against the collection.");
        assert!(
            ranking.text.starts_with("Full-text search Ranking"),
            "the indexed text still carries both: {}",
            ranking.text
        );
    }

    #[test]
    fn order_is_the_position_in_the_page() {
        let (_, fragments) = split("Full-text search", PAGE);
        let orders: Vec<i64> = fragments.iter().map(|fragment| fragment.order).collect();
        assert_eq!(orders, vec![0, 1, 2, 3]);
    }
}
