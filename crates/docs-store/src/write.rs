//! Changing one page or one section, as an editor does through the API.
//!
//! Distinct from `ingest`, which rebuilds everything from `content/`. Both write
//! the same records through the same helpers; the difference is the unit. A
//! deploy replaces the site, an editor changes a page, and sharing one entry
//! point would mean either a rebuild that edits or an edit that empties the
//! site — only one of those is funny.
//!
//! # Who is allowed
//!
//! Nothing in this file decides. The [`Store`](crate::Store) these run on carries
//! the caller's credentials, and the node refuses a write the caller's role does
//! not permit. That is the whole authorization design: the database is the
//! authority, and a second authority kept in step by hand is a second authority
//! that can disagree.

use docs_content::Page;

use crate::{Fault, Store, ingest::Section, ingest::check, records};

impl Store {
    /// Writes one page, replacing whatever stood at its slug.
    ///
    /// # Errors
    ///
    /// [`Fault::UnsafeSlug`] for a slug that cannot be spelled into a record id,
    /// and [`Fault::Client`] when the node refuses — which is also how a caller
    /// without permission to write finds out.
    pub async fn put_page(&mut self, page: &Page) -> Result<(), Fault> {
        check(&page.slug)?;
        if let Some(section) = &page.front.section {
            check(section)?;
        }

        // The old version's fragments go first. They are keyed by position, so
        // an edit that shortens a page would otherwise leave the tail of the old
        // one behind — still findable by search, still pointing at a heading
        // that is no longer on the page.
        self.clear_fragments(&page.slug).await?;
        self.unlink(&page.slug, "page").await?;
        self.write_page(page).await?;
        Ok(())
    }

    /// Removes a page, its fragments and its place in the tree.
    ///
    /// Answers whether it was there, so a caller can tell a deletion from a
    /// missing page rather than reporting both as success.
    ///
    /// # Errors
    ///
    /// [`Fault::UnsafeSlug`] or [`Fault::Client`], as [`Store::put_page`].
    pub async fn delete_page(&mut self, slug: &str) -> Result<bool, Fault> {
        check(slug)?;
        let existed = self.article(slug).await?.is_some();
        self.clear_fragments(slug).await?;
        self.unlink(slug, "page").await?;
        self.run(&format!("DELETE page:'{slug}';")).await?;
        Ok(existed)
    }

    /// Writes one section, replacing whatever stood at its slug.
    ///
    /// # Errors
    ///
    /// [`Fault::UnsafeSlug`] or [`Fault::Client`].
    pub async fn put_section(&mut self, section: &Section) -> Result<(), Fault> {
        check(&section.slug)?;
        if let Some(parent) = &section.parent {
            check(parent)?;
        }
        self.unlink(&section.slug, "section").await?;
        self.write_section(section).await?;
        if let Some(parent) = &section.parent {
            self.relate(parent, "section", &section.slug).await?;
        }
        Ok(())
    }

    /// Removes a section.
    ///
    /// The pages beneath it are **not** removed. They lose their place in the
    /// tree and keep their content, because deleting a category should not be a
    /// way to delete twenty pages by accident — a caller that wants them gone
    /// deletes them, and says so twenty times.
    ///
    /// # Errors
    ///
    /// [`Fault::UnsafeSlug`] or [`Fault::Client`].
    pub async fn delete_section(&mut self, slug: &str) -> Result<bool, Fault> {
        check(slug)?;
        let answers = self
            .run(&format!("SELECT * FROM section:'{slug}';"))
            .await?;
        let existed = !records(answers.first())?.is_empty();
        self.unlink(slug, "section").await?;
        self.run(&format!("DELETE section:'{slug}';")).await?;
        Ok(existed)
    }

    /// Drops the fragments of one page.
    async fn clear_fragments(&mut self, slug: &str) -> Result<(), Fault> {
        self.run_with(
            "DELETE FROM fragment WHERE page = $page;",
            vec![("page".to_owned(), crate::text(slug))],
        )
        .await?;
        Ok(())
    }

    /// Drops the edge that gives a record its place in the tree, if it has one.
    ///
    /// Matched on the edge's **own** `child` field rather than on its endpoints,
    /// because `RELATE` names the target `in` and `IN` is an operator: the
    /// language cannot express "the edge pointing at this page". This is why
    /// `relate` writes the child onto the edge, and why the value is qualified
    /// by table — a section and a page may share a slug, and unlinking one must
    /// not unlink the other.
    ///
    /// It matters because deleting the page record does **not** help: the record
    /// id is the slug, so recreating the page makes the old edge live again and
    /// the page appears under both its old parent and its new one.
    async fn unlink(&mut self, slug: &str, table: &str) -> Result<(), Fault> {
        self.run_with(
            "DELETE FROM holds WHERE child = $child;",
            vec![("child".to_owned(), crate::text(&format!("{table}:{slug}")))],
        )
        .await?;
        Ok(())
    }
}
