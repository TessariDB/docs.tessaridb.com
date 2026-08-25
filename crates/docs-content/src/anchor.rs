//! Heading text to the fragment identifier a link can carry.
//!
//! The rule is the ordinary one — lowercase, keep letters and digits, turn every
//! run of anything else into a single hyphen — with one addition that matters
//! more than the rule itself: **anchors are made unique within a page**.
//!
//! A documentation page repeats headings. `Examples`, `Errors` and `See also`
//! appear under every statement in a reference page, and a search result points
//! at an anchor. If two headings produce one anchor then a result for the second
//! scrolls the reader to the first, which is a wrong answer that looks exactly
//! like a right one — the browser jumps, the page moves, and nothing reports a
//! fault. So the second occurrence becomes `examples-1`, the third `examples-2`,
//! which is the convention a reader's existing bookmarks already expect.

use std::collections::HashMap;

/// Hands out anchors that are unique within one page.
#[derive(Debug, Default)]
pub struct Anchors {
    seen: HashMap<String, u32>,
}

impl Anchors {
    /// A fresh page, with nothing yet claimed.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The anchor for `heading`, distinct from every anchor already handed out.
    pub fn of(&mut self, heading: &str) -> String {
        let base = slug(heading);
        let taken = self.seen.entry(base.clone()).or_insert(0);
        let anchor = if *taken == 0 {
            base.clone()
        } else {
            format!("{base}-{taken}")
        };
        *taken = taken.saturating_add(1);
        anchor
    }
}

/// The plain slug of a string, before uniqueness is considered.
///
/// Kept public because a page's own path is slugged by the same rule, and two
/// rules that are meant to agree should be one function.
#[must_use]
pub fn slug(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pending_hyphen = false;
    for character in text.chars() {
        if character.is_ascii_alphanumeric() {
            if pending_hyphen && !out.is_empty() {
                out.push('-');
            }
            pending_hyphen = false;
            out.extend(character.to_lowercase());
        } else if character.is_alphanumeric() {
            // A letter this project cannot fold to ASCII — a Cyrillic or Greek
            // heading. Kept rather than dropped, because dropping it turns a
            // whole heading into an empty anchor.
            if pending_hyphen && !out.is_empty() {
                out.push('-');
            }
            pending_hyphen = false;
            out.extend(character.to_lowercase());
        } else {
            pending_hyphen = true;
        }
    }
    if out.is_empty() {
        // A heading of nothing but punctuation still needs an identifier, and an
        // empty one would collide with every other empty one.
        out.push_str("section");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{Anchors, slug};

    #[test]
    fn punctuation_becomes_one_hyphen_and_never_a_trailing_one() {
        assert_eq!(slug("Full-text search"), "full-text-search");
        assert_eq!(slug("What is refused, and why?"), "what-is-refused-and-why");
        assert_eq!(slug("  leading and trailing  "), "leading-and-trailing");
        assert_eq!(
            slug("DEFINE ANALYZER — the field's"),
            "define-analyzer-the-field-s"
        );
    }

    #[test]
    fn a_heading_with_no_letters_still_gets_an_identifier() {
        // Otherwise every such heading shares the empty anchor, which is the
        // collision this module exists to prevent, in its worst form.
        assert_eq!(slug("!!!"), "section");
        assert_eq!(slug(""), "section");
    }

    #[test]
    fn a_repeated_heading_does_not_point_at_the_first_one() {
        // The whole reason this is a struct and not a function.
        let mut anchors = Anchors::new();
        assert_eq!(anchors.of("Examples"), "examples");
        assert_eq!(anchors.of("Examples"), "examples-1");
        assert_eq!(anchors.of("Examples"), "examples-2");
        assert_eq!(anchors.of("Errors"), "errors");
    }

    #[test]
    fn two_headings_that_differ_only_in_punctuation_still_differ() {
        let mut anchors = Anchors::new();
        assert_eq!(anchors.of("See also"), "see-also");
        assert_eq!(anchors.of("See also:"), "see-also-1");
    }

    #[test]
    fn a_letter_outside_ascii_is_kept_rather_than_dropped() {
        // Dropping it would leave `section`, and a page of such headings would
        // be one anchor repeated.
        assert_eq!(slug("Запросы"), "запросы");
    }
}
