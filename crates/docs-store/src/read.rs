//! The three reads the site is made of: the tree, a page, and a search.

use serde::Serialize;

use crate::{Fault, Store, records, schema::is_safe_slug, text_of};

/// A node of the left-hand tree, with whatever hangs beneath it.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Node {
    /// The slug — a section's own, or a page's path.
    pub slug: String,
    /// What the tree shows.
    pub title: String,
    /// `section` or `page`. A section is a heading in the tree; a page is a link.
    pub kind: &'static str,
    /// An icon name from this project's own set.
    pub icon: Option<String>,
    /// Sections and pages beneath this one, already ordered.
    pub children: Vec<Node>,
}

/// A page as the site renders it.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Article {
    /// The path this page answers.
    pub slug: String,
    /// The title.
    pub title: String,
    /// One sentence under the title.
    pub summary: Option<String>,
    /// The Markdown body. Rendered by the front end.
    pub markdown: String,
    /// Whether the page describes something the engine does not do yet.
    pub unreleased: bool,
}

/// One search result — a fragment, with the page it belongs to.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Hit {
    /// The page's path.
    pub page: String,
    /// The heading the match sits under.
    pub heading: String,
    /// The anchor to link to, so the reader lands on the passage and not the top.
    pub anchor: String,
    /// The matching text, for the snippet.
    pub text: String,
    /// What the store scored it. Carried through rather than recomputed.
    pub relevance: f64,
}

impl Store {
    /// The whole left-hand tree.
    ///
    /// Read as a graph walk from the roots down, which is what the `holds`
    /// edge is for: the depth of the tree lives in the data rather than in a
    /// fixed set of columns, so a fourth level is content and not a migration.
    ///
    /// # Errors
    ///
    /// Returns [`Fault::Client`] when the node refuses, and
    /// [`Fault::Unexpected`] when an answer is not a record set.
    pub async fn tree(&mut self) -> Result<Vec<Node>, Fault> {
        let answers = self
            .run("SELECT * FROM section WHERE root = true ORDER BY order;")
            .await?;
        let roots = records(answers.first())?
            .iter()
            .map(|(_, value)| text_of(value, "slug"))
            .collect::<Vec<_>>();

        let mut tree = Vec::with_capacity(roots.len());
        for slug in roots {
            tree.push(self.subtree(&slug).await?);
        }
        Ok(tree)
    }

    /// One section and everything beneath it.
    ///
    /// # Errors
    ///
    /// Returns [`Fault::UnsafeSlug`] for a slug that cannot be spelled into an
    /// id, and [`Fault::Client`] when the node refuses.
    pub async fn subtree(&mut self, slug: &str) -> Result<Node, Fault> {
        if !is_safe_slug(slug) {
            return Err(Fault::UnsafeSlug(slug.to_owned()));
        }

        let answers = self
            .run(&format!("SELECT * FROM section:'{slug}';"))
            .await?;
        let found = records(answers.first())?;
        let (title, icon) = match found.first() {
            Some((_, value)) => (text_of(value, "title"), non_empty(text_of(value, "icon"))),
            None => (slug.to_owned(), None),
        };

        let answers = self
            .run(&format!(
                "SELECT * FROM section:'{slug}'->holds->section ORDER BY order;\nSELECT * FROM section:'{slug}'->holds->page ORDER BY order;"
            ))
            .await?;

        let mut children = Vec::new();
        let child_sections: Vec<String> = records(answers.first())?
            .iter()
            .map(|(_, value)| text_of(value, "slug"))
            .collect();
        for child in child_sections {
            children.push(Box::pin(self.subtree(&child)).await?);
        }
        for (_, value) in records(answers.get(1))? {
            children.push(Node {
                slug: text_of(value, "slug"),
                title: text_of(value, "title"),
                kind: "page",
                icon: None,
                children: Vec::new(),
            });
        }

        Ok(Node {
            slug: slug.to_owned(),
            title,
            kind: "section",
            icon,
            children,
        })
    }

    /// How many pages the store is holding.
    ///
    /// Asked at start to decide whether the store needs seeding. A count rather
    /// than a "has anything ever been ingested" marker, because a marker is a
    /// second fact to keep true and this one is derived from the thing itself.
    ///
    /// # Errors
    ///
    /// Returns [`Fault::Client`] when the node refuses.
    pub async fn pages_held(&mut self) -> Result<usize, Fault> {
        let answers = self.run("SELECT slug FROM page;").await?;
        Ok(records(answers.first())?.len())
    }

    /// One page.
    ///
    /// # Errors
    ///
    /// Returns [`Fault::UnsafeSlug`] for an unusable slug and [`Fault::Client`]
    /// when the node refuses. A slug that names no page is `Ok(None)`, because a
    /// missing page is a 404 and not a fault.
    pub async fn article(&mut self, slug: &str) -> Result<Option<Article>, Fault> {
        if !is_safe_slug(slug) {
            return Err(Fault::UnsafeSlug(slug.to_owned()));
        }
        let answers = self.run(&format!("SELECT * FROM page:'{slug}';")).await?;
        let found = records(answers.first())?;
        Ok(found.first().map(|(_, value)| Article {
            slug: text_of(value, "slug"),
            title: text_of(value, "title"),
            summary: non_empty(text_of(value, "summary")),
            markdown: text_of(value, "markdown"),
            unreleased: matches!(
                value,
                tessaridb_client::Value::Object(fields)
                    if matches!(fields.get("unreleased"), Some(tessaridb_client::Value::Bool(true)))
            ),
        }))
    }

    /// A ranked search.
    ///
    /// The query is a **bound value**, never spelled into the statement. That is
    /// what makes it safe to take one from a query string: a supplied value
    /// cannot become syntax, and being remote does not give that back.
    ///
    /// The ordering is the store's — `search::score` over the one indexed field
    /// — and this code does no scoring of its own. See `schema` for why there is
    /// exactly one such field.
    ///
    /// # Errors
    ///
    /// Returns [`Fault::Client`] when the node refuses.
    pub async fn search(&mut self, query: &str, limit: u32) -> Result<Vec<Hit>, Fault> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(Vec::new());
        }
        let limit = limit.clamp(1, 50);
        // The projection is a list rather than `*` because the language has no
        // `SELECT *, expr` form — and it is the better statement regardless: a
        // fragment's `markdown`-sized neighbours have no business travelling on
        // a search result. `ORDER BY` repeats the call rather than naming the
        // alias, so the ordering does not depend on where aliases resolve.
        // `body` is projected and `text` is not: the match is made against
        // `text`, which opens with the page title and the heading so that one
        // index can rank both — and a snippet drawn from it would begin by
        // repeating the two lines the result already shows above it.
        let script = format!(
            "SELECT page, heading, anchor, body, search::score(text, $q) AS relevance
             FROM fragment
             WHERE text MATCHES $q
             ORDER BY search::score(text, $q) DESC
             LIMIT {limit};"
        );
        let answers = self
            .run_with(
                &script,
                vec![(
                    "q".to_owned(),
                    tessaridb_client::Value::String(query.to_owned()),
                )],
            )
            .await?;

        Ok(records(answers.first())?
            .iter()
            .map(|(_, value)| Hit {
                page: text_of(value, "page"),
                heading: text_of(value, "heading"),
                anchor: text_of(value, "anchor"),
                text: text_of(value, "body"),
                relevance: relevance_of(value),
            })
            .collect())
    }
}

fn non_empty(text: String) -> Option<String> {
    if text.is_empty() { None } else { Some(text) }
}

/// The score, whichever numeric shape the node used for it.
fn relevance_of(value: &tessaridb_client::Value) -> f64 {
    let tessaridb_client::Value::Object(fields) = value else {
        return 0.0;
    };
    match fields.get("relevance") {
        Some(tessaridb_client::Value::Number(tessaridb_client::Number::Float(found))) => *found,
        Some(tessaridb_client::Value::Number(tessaridb_client::Number::Integer(found))) => {
            f64::from(i32::try_from(*found).unwrap_or(0))
        }
        _ => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use tessaridb_client::{Number, Value};

    use super::{non_empty, relevance_of};

    #[test]
    fn an_empty_optional_reads_as_absent_rather_than_as_an_empty_line() {
        assert_eq!(non_empty(String::new()), None);
        assert_eq!(
            non_empty("a summary".to_owned()),
            Some("a summary".to_owned())
        );
    }

    #[test]
    fn a_score_is_read_whichever_numeric_shape_it_arrives_in() {
        // A whole-numbered score may arrive as an integer. Reading only the
        // float case would silently rank every such result last.
        let mut float = BTreeMap::new();
        float.insert("relevance".to_owned(), Value::Number(Number::Float(1.75)));
        assert!((relevance_of(&Value::Object(float)) - 1.75).abs() < f64::EPSILON);

        let mut integer = BTreeMap::new();
        integer.insert("relevance".to_owned(), Value::Number(Number::Integer(2)));
        assert!((relevance_of(&Value::Object(integer)) - 2.0).abs() < f64::EPSILON);

        let mut absent = BTreeMap::new();
        absent.insert("page".to_owned(), Value::String("x".to_owned()));
        assert!(relevance_of(&Value::Object(absent)).abs() < f64::EPSILON);
    }
}
