//! Putting `content/` into the store.
//!
//! # Why this rebuilds rather than reconciles
//!
//! The ingest deletes every page, fragment, section and edge and writes them
//! again. A reconciling ingest — work out what changed, write only that — is the
//! obvious improvement and is the wrong trade here: it has to be right about
//! deletions and renames to avoid leaving a page that no longer exists in the
//! tree, and it would be exercised only on the rare edit while the rebuild is
//! exercised on every deploy. A rebuild of a few hundred pages is a second.
//!
//! It runs at start, before the server takes traffic, so the window where the
//! store is half-written is a window with no readers in it.

use docs_content::Page;

use crate::{Fault, Store, schema::is_safe_slug};

/// A node of the left-hand tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    /// The slug, which is also the path prefix of the pages beneath it.
    pub slug: String,
    /// What the tree shows.
    pub title: String,
    /// The parent section, or `None` for a top-level group.
    pub parent: Option<String>,
    /// Position among siblings.
    pub order: i64,
    /// An icon name from this project's own set.
    pub icon: Option<String>,
}

/// Everything the site is, ready to be written.
#[derive(Debug, Default)]
pub struct Corpus {
    /// The tree.
    pub sections: Vec<Section>,
    /// The pages.
    pub pages: Vec<Page>,
}

impl Store {
    /// Replaces the store's contents with `corpus`.
    ///
    /// # Errors
    ///
    /// Returns [`Fault::UnsafeSlug`] when a slug could not be spelled into a
    /// record id, and [`Fault::Client`] when the node refuses a statement.
    pub async fn ingest(&mut self, corpus: &Corpus) -> Result<Written, Fault> {
        for section in &corpus.sections {
            check(&section.slug)?;
            if let Some(parent) = &section.parent {
                check(parent)?;
            }
        }
        for page in &corpus.pages {
            check(&page.slug)?;
        }

        self.run("DELETE FROM contains;\nDELETE FROM fragment;\nDELETE FROM page;\nDELETE FROM section;\n")
            .await?;

        let mut written = Written::default();

        for section in &corpus.sections {
            let script = format!(
                "CREATE section:'{slug}' = {{ slug: $slug, title: $title, order: $order, icon: $icon, root: $root }};",
                slug = section.slug
            );
            self.run_with(
                &script,
                vec![
                    ("slug".to_owned(), text(&section.slug)),
                    ("title".to_owned(), text(&section.title)),
                    ("order".to_owned(), integer(section.order)),
                    ("icon".to_owned(), optional(section.icon.as_deref())),
                    // Written rather than derived, so the tree read can find the
                    // roots in one statement instead of asking every section
                    // whether anything points at it.
                    (
                        "root".to_owned(),
                        tessaridb_client::Value::Bool(section.parent.is_none()),
                    ),
                ],
            )
            .await?;
            written.sections = written.sections.saturating_add(1);
        }

        // The edges come after every section exists, because an edge to a record
        // that is not there yet is a dangling one and the tree read would skip it.
        for section in &corpus.sections {
            if let Some(parent) = &section.parent {
                let script = format!(
                    "RELATE section:'{parent}'->contains->section:'{child}';",
                    child = section.slug
                );
                self.run(&script).await?;
                written.edges = written.edges.saturating_add(1);
            }
        }

        for page in &corpus.pages {
            let script = format!(
                "CREATE page:'{slug}' = {{ slug: $slug, title: $title, summary: $summary, markdown: $markdown, order: $order, unreleased: $unreleased }};",
                slug = page.slug
            );
            self.run_with(
                &script,
                vec![
                    ("slug".to_owned(), text(&page.slug)),
                    ("title".to_owned(), text(&page.title)),
                    (
                        "summary".to_owned(),
                        optional(page.front.summary.as_deref()),
                    ),
                    ("markdown".to_owned(), text(&page.markdown)),
                    ("order".to_owned(), integer(page.front.order)),
                    (
                        "unreleased".to_owned(),
                        tessaridb_client::Value::Bool(page.front.unreleased),
                    ),
                ],
            )
            .await?;
            written.pages = written.pages.saturating_add(1);

            if let Some(section) = &page.front.section {
                check(section)?;
                let script = format!(
                    "RELATE section:'{section}'->contains->page:'{slug}';",
                    slug = page.slug
                );
                self.run(&script).await?;
                written.edges = written.edges.saturating_add(1);
            }

            for fragment in &page.fragments {
                // The id is the page slug and the position, so it is stable
                // across a rebuild and unique without a counter.
                let script = format!(
                    "CREATE fragment:'{slug}#{order}' = {{ page: $page, anchor: $anchor, heading: $heading, depth: $depth, order: $order, text: $text }};",
                    slug = page.slug,
                    order = fragment.order
                );
                self.run_with(
                    &script,
                    vec![
                        ("page".to_owned(), text(&page.slug)),
                        ("anchor".to_owned(), text(&fragment.anchor)),
                        ("heading".to_owned(), text(&fragment.heading)),
                        ("depth".to_owned(), integer(i64::from(fragment.depth))),
                        ("order".to_owned(), integer(fragment.order)),
                        ("text".to_owned(), text(&fragment.text)),
                    ],
                )
                .await?;
                written.fragments = written.fragments.saturating_add(1);
            }
        }

        log::info!(
            "ingested {} sections, {} pages, {} fragments, {} edges",
            written.sections,
            written.pages,
            written.fragments,
            written.edges
        );
        Ok(written)
    }
}

/// What an ingest wrote, so a caller can report it rather than assume it.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Written {
    /// Sections written.
    pub sections: u64,
    /// Pages written.
    pub pages: u64,
    /// Fragments written.
    pub fragments: u64,
    /// Tree edges written.
    pub edges: u64,
}

fn check(slug: &str) -> Result<(), Fault> {
    if is_safe_slug(slug) {
        Ok(())
    } else {
        Err(Fault::UnsafeSlug(slug.to_owned()))
    }
}

fn text(value: &str) -> tessaridb_client::Value {
    tessaridb_client::Value::String(value.to_owned())
}

fn integer(value: i64) -> tessaridb_client::Value {
    tessaridb_client::Value::Number(tessaridb_client::Number::Integer(value))
}

/// An absent optional is `Null` and not the empty string, because the store
/// distinguishes them and a reader of the data should be able to as well.
fn optional(value: Option<&str>) -> tessaridb_client::Value {
    match value {
        Some(found) => text(found),
        None => tessaridb_client::Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::Fault;

    #[test]
    fn a_slug_that_cannot_be_spelled_into_an_id_stops_the_ingest() {
        // It stops rather than escapes: such a slug is a mistake in the content
        // tree, and quietly rewriting it would produce a page at a path nobody
        // linked to.
        assert!(matches!(check("fine/path"), Ok(())));
        assert!(matches!(check("has'quote"), Err(Fault::UnsafeSlug(_))));
    }
}
