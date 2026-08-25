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
//!
//! An editor changing one page does **not** come through here — see `write`.
//! Both write the same records through the same two helpers below; what differs
//! is the unit.

use docs_content::Page;

use crate::{Fault, Store, boolean, integer, optional, schema::is_safe_slug, text};

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

        // `WHERE true` rather than a bare `DELETE FROM t`: the language reads a
        // delete with no condition as naming one record by identity and refuses
        // it, deliberately, so that a mistyped `DELETE FROM page` cannot empty a
        // table. Emptying one is therefore always asked for out loud.
        self.run(
            "DELETE FROM holds WHERE true;\nDELETE FROM fragment WHERE true;\nDELETE FROM page WHERE true;\nDELETE FROM section WHERE true;\n",
        )
        .await?;

        let mut written = Written::default();

        for section in &corpus.sections {
            self.write_section(section).await?;
            written.sections = written.sections.saturating_add(1);
        }

        // The edges come after every section exists, because an edge to a record
        // that is not there yet is a dangling one and the tree read would skip it.
        for section in &corpus.sections {
            if let Some(parent) = &section.parent {
                self.relate(parent, "section", &section.slug).await?;
                written.edges = written.edges.saturating_add(1);
            }
        }

        for page in &corpus.pages {
            let fragments = self.write_page(page).await?;
            written.pages = written.pages.saturating_add(1);
            written.fragments = written.fragments.saturating_add(fragments);
            if page.front.section.is_some() {
                written.edges = written.edges.saturating_add(1);
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

    /// One section, replacing whatever stood at its slug.
    pub(crate) async fn write_section(&mut self, section: &Section) -> Result<(), Fault> {
        let script = format!(
            "DELETE section:'{slug}';\nCREATE section:'{slug}' = {{ slug: $slug, title: $title, order: $order, icon: $icon, root: $root }};",
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
                // roots in one statement instead of asking every section whether
                // anything points at it.
                ("root".to_owned(), boolean(section.parent.is_none())),
            ],
        )
        .await?;
        Ok(())
    }

    /// One page and its fragments, replacing whatever stood at its slug.
    ///
    /// Answers how many fragments it wrote.
    pub(crate) async fn write_page(&mut self, page: &Page) -> Result<u64, Fault> {
        let script = format!(
            "DELETE page:'{slug}';\nCREATE page:'{slug}' = {{ slug: $slug, title: $title, summary: $summary, markdown: $markdown, order: $order, unreleased: $unreleased }};",
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
                ("unreleased".to_owned(), boolean(page.front.unreleased)),
            ],
        )
        .await?;

        if let Some(section) = &page.front.section {
            self.relate(section, "page", &page.slug).await?;
        }

        let mut written = 0_u64;
        for fragment in &page.fragments {
            // The id is the page slug and the position, so it is stable across a
            // rebuild and unique without a counter.
            let script = format!(
                "CREATE fragment:'{slug}#{order}' = {{ page: $page, anchor: $anchor, heading: $heading, depth: $depth, order: $order, text: $text, body: $body }};",
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
                    // Stored and not analyzed: this one is for showing, and a
                    // second analyzed field would be a second collection that
                    // `search::score` could not honestly rank against the first.
                    ("body".to_owned(), text(&fragment.body)),
                ],
            )
            .await?;
            written = written.saturating_add(1);
        }
        Ok(written)
    }

    /// A tree edge from a section to a section or a page.
    ///
    /// The edge carries the child it points at as **its own field**, which looks
    /// redundant and is not. `RELATE` names the endpoints `in` and `out`, and
    /// `IN` is an operator in this language, so an edge cannot be filtered by
    /// its target — there is no way to write "the edge pointing at this page".
    /// Without a field of our own, moving a page to another section would leave
    /// the old edge in place and the page would appear under both parents.
    /// See `write::unlink`.
    pub(crate) async fn relate(
        &mut self,
        parent: &str,
        table: &str,
        child: &str,
    ) -> Result<(), Fault> {
        self.run_with(
            &format!("RELATE section:'{parent}'->holds->{table}:'{child}' = {{ child: $child }};"),
            vec![("child".to_owned(), text(&format!("{table}:{child}")))],
        )
        .await?;
        Ok(())
    }
}

pub(crate) fn check(slug: &str) -> Result<(), Fault> {
    if is_safe_slug(slug) {
        Ok(())
    } else {
        Err(Fault::UnsafeSlug(slug.to_owned()))
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
