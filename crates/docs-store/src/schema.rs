//! The shape the site keeps its content in.
//!
//! Written once at start and safe to re-run, because every statement carries
//! `IF NOT EXISTS`. A deployment that restarts is not a deployment that has to
//! remember whether it has run this before.
//!
//! # The one decision worth reading
//!
//! `fragment.text` is the only analyzed field and carries the only search index.
//! That is not an economy, it is a correctness requirement: `search::score` is
//! refused over a field with no search index, and two scores drawn from two
//! indexes measure against two different collections, so merging them into one
//! ordering produces a ranking whose numbers do not mean what they appear to.
//! `docs-content` composes the page title and the heading into `text` for
//! exactly this reason, so one index ranks title hits and body hits together.

/// A record id is spelled into a statement rather than bound, so the characters
/// allowed in a slug are fixed here and checked before anything is written.
///
/// Slugs come from file paths under this repository's control and from
/// [`docs_content::slug`], which emits only lowercase alphanumerics and hyphens.
/// The check is kept anyway: "the input is trusted" is the assumption every
/// injection begins with, and the cost of holding the line here is one pass over
/// a short string.
#[must_use]
pub fn is_safe_slug(slug: &str) -> bool {
    !slug.is_empty()
        && slug.len() <= 512
        && slug
            .chars()
            .all(|character| character.is_alphanumeric() || matches!(character, '-' | '_' | '/'))
}

/// Every statement the site's schema is made of, in order.
///
/// `namespace` is the version this doc set belongs to — a namespace per released
/// version, so switching version is switching namespace and an old version is
/// never a filter somebody forgets to apply.
#[must_use]
pub fn statements(namespace: &str) -> String {
    format!(
        "DEFINE NAMESPACE IF NOT EXISTS {namespace};
USE NAMESPACE {namespace};
DEFINE DATABASE IF NOT EXISTS docs;
USE DATABASE docs;

DEFINE ANALYZER IF NOT EXISTS prose FILTERS lowercase, ascii;

DEFINE TABLE IF NOT EXISTS section;
DEFINE TABLE IF NOT EXISTS page;
DEFINE TABLE IF NOT EXISTS fragment;
DEFINE TABLE IF NOT EXISTS contains EDGE;
DEFINE BUCKET IF NOT EXISTS asset;

DEFINE FIELD IF NOT EXISTS text ON fragment TYPE string ANALYZER prose;
DEFINE INDEX IF NOT EXISTS by_text ON fragment FIELDS text SEARCH;
"
    )
}

/// Selecting the namespace and database on a connection that is already open.
#[must_use]
pub fn use_namespace(namespace: &str) -> String {
    format!("USE NAMESPACE {namespace};\nUSE DATABASE docs;\n")
}

#[cfg(test)]
mod tests {
    use super::{is_safe_slug, statements, use_namespace};

    #[test]
    fn a_slug_that_could_close_a_quoted_id_is_refused() {
        // The property that lets a record id be spelled into a statement at all.
        assert!(!is_safe_slug("query'; DELETE FROM page; --"));
        assert!(!is_safe_slug("has space"));
        assert!(!is_safe_slug("quote'inside"));
        assert!(!is_safe_slug(""));
    }

    #[test]
    fn the_slugs_this_project_actually_produces_are_accepted() {
        assert!(is_safe_slug("query-language/full-text-search"));
        assert!(is_safe_slug("index"));
        assert!(is_safe_slug("sdk/rust_client"));
        assert!(is_safe_slug("запросы/поиск"));
    }

    #[test]
    fn every_definition_is_re_runnable_so_a_restart_is_not_a_special_case() {
        let script = statements("v0_0_1_alpha");
        for line in script.lines() {
            let line = line.trim();
            if line.starts_with("DEFINE") {
                assert!(
                    line.contains("IF NOT EXISTS"),
                    "a restart would fail on: {line}"
                );
            }
        }
    }

    #[test]
    fn the_only_analyzed_field_is_the_only_indexed_one() {
        // If these ever disagree, ranking silently stops meaning what the search
        // route says it means. Cheaper to assert than to notice in production.
        let script = statements("v1");
        assert!(
            script
                .contains("DEFINE FIELD IF NOT EXISTS text ON fragment TYPE string ANALYZER prose")
        );
        assert!(
            script.contains("DEFINE INDEX IF NOT EXISTS by_text ON fragment FIELDS text SEARCH")
        );
        // Exactly one field is analyzed and exactly one index searches, and they
        // are the same field. A second analyzed field would be the drift this
        // asserts against: it would look harmless and would quietly make two
        // scores that cannot be ordered against each other.
        let analyzed: Vec<&str> = script
            .lines()
            .filter(|line| line.starts_with("DEFINE FIELD") && line.contains("ANALYZER"))
            .collect();
        assert_eq!(analyzed.len(), 1, "{analyzed:?}");
        let searched: Vec<&str> = script
            .lines()
            .filter(|line| line.starts_with("DEFINE INDEX") && line.contains("SEARCH"))
            .collect();
        assert_eq!(searched.len(), 1, "{searched:?}");
        assert!(analyzed[0].contains("text ON fragment"));
        assert!(searched[0].contains("ON fragment FIELDS text"));
    }

    #[test]
    fn the_namespace_is_the_version() {
        assert!(statements("v0_2_0").contains("DEFINE NAMESPACE IF NOT EXISTS v0_2_0"));
        assert_eq!(
            use_namespace("v0_2_0"),
            "USE NAMESPACE v0_2_0;\nUSE DATABASE docs;\n"
        );
    }
}
