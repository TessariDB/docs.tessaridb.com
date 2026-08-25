//! Markdown to HTML, with the heading ids the rest of this crate promised.
//!
//! # Why this is in Rust and not in the front end
//!
//! Because the anchors have to match, and two implementations of "turn a heading
//! into an id" will not stay matched. [`crate::fragment::split`] hands every
//! heading an anchor, a search result carries that anchor, and the outline links
//! to it. If the page's HTML were produced somewhere else by a different
//! slugifier — a different rule for punctuation, or no de-duplication of repeated
//! headings — then every one of those links would point at an id the page does
//! not have.
//!
//! And it would fail *quietly*. A browser given a fragment it cannot find does
//! not report anything; it simply leaves the reader at the top of the page. The
//! search box would look like it worked and would be wrong on every result.
//!
//! So the ids come from the same [`Anchors`] in the same document order, and a
//! test below holds the two against each other on a page with repeated headings —
//! which is the case where a naive slugifier and this one first disagree.

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd, html};

use crate::anchor::Anchors;

/// The parser configuration, used by every reader of Markdown in this crate.
///
/// One function rather than one call each, so that what is rendered and what is
/// indexed are the same document. Tables are enabled because the documentation
/// uses them; without it a table is one paragraph of pipes, which renders as
/// pipes and indexes as pipes.
#[must_use]
pub fn options() -> Options {
    Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH
}

/// Renders `markdown` to HTML, giving each heading the anchor the outline uses.
///
/// The HTML is trusted: it comes from this repository's own content, and from
/// pages written through the API by users the store lets write. Markdown's own
/// inline HTML therefore passes through, which is what lets a page carry a
/// diagram. A site taking Markdown from the public would need a sanitiser here,
/// and would need to decide what it strips before it accepted the first page.
#[must_use]
pub fn to_html(markdown: &str) -> String {
    let mut anchors = Anchors::new();
    let mut events: Vec<Event> = Parser::new_ext(markdown, options()).collect();

    // Two passes, because a heading's id is decided by text that has not been
    // read yet when its opening tag goes past.
    let mut at = 0;
    while at < events.len() {
        let is_heading = matches!(events.get(at), Some(Event::Start(Tag::Heading { .. })));
        if is_heading {
            let anchor = anchors.of(&heading_text(&events, at));
            if let Some(Event::Start(Tag::Heading { id, .. })) = events.get_mut(at) {
                *id = Some(anchor.into());
            }
        }
        at = at.saturating_add(1);
    }

    let mut html = String::with_capacity(markdown.len().saturating_mul(3).saturating_div(2));
    html::push_html(&mut html, events.into_iter());
    html
}

/// The text of the heading whose opening tag is at `from`.
///
/// Text and code only, exactly as `fragment::split` gathers it — an emphasised
/// word inside a heading contributes its text and not its markup, in both.
fn heading_text(events: &[Event], from: usize) -> String {
    let mut text = String::new();
    for event in events.iter().skip(from.saturating_add(1)) {
        match event {
            Event::End(TagEnd::Heading(_)) => break,
            Event::Text(found) | Event::Code(found) => text.push_str(found),
            _ => {}
        }
    }
    text
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::to_html;
    use crate::fragment::split;

    #[test]
    fn a_heading_carries_the_id_a_link_to_it_would_use() {
        let html = to_html("## Full-text search\n\nTerms.\n");
        assert!(
            html.contains("<h2 id=\"full-text-search\">Full-text search</h2>"),
            "{html}"
        );
    }

    #[test]
    fn every_anchor_the_outline_points_at_exists_in_the_rendered_page() {
        // The property this module exists for, on the page that breaks a naive
        // slugifier: three headings that slug identically. A renderer without
        // de-duplication emits `examples` three times, so the outline's second
        // and third entries scroll to the first — a wrong answer that looks
        // exactly like a right one.
        let markdown = "\
## SELECT

Reads records.

### Examples

`SELECT * FROM users;`

## DELETE

Removes them.

### Examples

`DELETE users:1;`

## CREATE

Writes them.

### Examples

`CREATE users:1 = {};`
";
        let (headings, _) = split("Reference", markdown);
        let html = to_html(markdown);
        assert_eq!(headings.len(), 6);
        for heading in &headings {
            assert!(
                html.contains(&format!("id=\"{}\"", heading.anchor)),
                "the outline points at {} and the page has no such id\n{html}",
                heading.anchor
            );
        }
        // And they are distinct, which is the half an existence check misses:
        // three identical ids would satisfy the loop above.
        assert!(html.contains("id=\"examples\""));
        assert!(html.contains("id=\"examples-1\""));
        assert!(html.contains("id=\"examples-2\""));
    }

    #[test]
    fn a_fenced_block_keeps_its_language_so_it_can_be_styled_as_one() {
        let html = to_html("```tessariql\nSELECT * FROM users;\n```\n");
        assert!(html.contains("class=\"language-tessariql\""), "{html}");
        assert!(html.contains("SELECT * FROM users;"), "{html}");
    }

    #[test]
    fn a_table_is_a_table_and_not_a_paragraph_of_pipes() {
        let html = to_html("| a | b |\n|---|---|\n| 1 | 2 |\n");
        assert!(html.contains("<table>"), "{html}");
        assert!(!html.contains("|---|"), "{html}");
    }

    #[test]
    fn the_text_of_a_heading_ignores_its_markup_the_same_way_the_outline_does() {
        // `**Where** a score is refused` must produce the same anchor in both,
        // or the outline entry and the rendered id disagree on this one page.
        let markdown = "### **Where** a score is `refused`\n\nbody\n";
        let (headings, _) = split("Search", markdown);
        let anchor = &headings.first().expect("one heading").anchor;
        assert_eq!(anchor, "where-a-score-is-refused");
        assert!(to_html(markdown).contains("id=\"where-a-score-is-refused\""));
    }
}
