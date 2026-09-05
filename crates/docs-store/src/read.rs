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

    /// A search: the words as typed, then the word being typed.
    ///
    /// The query is a **bound value**, never spelled into the statement. That is
    /// what makes it safe to take one from a query string: a supplied value
    /// cannot become syntax, and being remote does not give that back.
    ///
    /// # Why a half-typed word needs anything extra
    ///
    /// `MATCHES` asks whether the analyzed text holds every **word** of the
    /// query, and that is the right question once a reader has finished typing
    /// one. It is the wrong one twice over: while they are still typing a word,
    /// `vecto` is not the word `vector`; and when they have finished typing it
    /// wrongly, `vecter` is not one either. Either way a search over whole words
    /// answers with nothing at all, which reads as "this site has nothing about
    /// vectors" rather than as "finish the word" or "check the spelling".
    ///
    /// So when the ranked pass has not filled the page, three further passes run
    /// in order, each answering a question the one before it could not, and each
    /// worth less than the one before it:
    ///
    /// 1. **the word being typed** — the index is asked for the terms beginning
    ///    with the last word (`MATCHES PREFIX`), and the words in the fragments
    ///    that come back say which word the reader was most likely typing. The
    ///    index is then asked for *that* whole word, which is what puts the page
    ///    about vectors above the page that mentions a test vector in passing;
    /// 2. anything the completed word did not account for follows, unscored;
    /// 3. **the word they meant** — `MATCHES FUZZY` over the query as typed,
    ///    for the case the first two cannot reach: a finished word that is
    ///    misspelled rather than half-typed.
    ///
    /// Ranked results keep their order and stay first throughout. The passes are
    /// kept apart rather than merged into one statement because they answer
    /// different questions and are worth different amounts — an exact hit must
    /// never sort below a guess at what the reader meant.
    ///
    /// # The floor is the engine's, and it is a refusal
    ///
    /// Both `PREFIX` and `FUZZY` refuse a term shorter than three characters
    /// (`PrefixTooShort`), and they refuse it **per word**: `vector se` is
    /// refused for `se` even though `vector` is long enough. So a stub shorter
    /// than the floor is dropped from the query rather than sent — a reader
    /// part-way through typing `se` is shown what `vector` alone ranks, which is
    /// both a better answer than an error and a better one than nothing.
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
        let room = usize::try_from(limit).unwrap_or(usize::MAX);
        let mut hits = self.ranked(query, limit).await?;
        if hits.len() >= room {
            return Ok(hits);
        }

        let Some((finished, partial)) = split_last_word(query) else {
            return Ok(hits);
        };

        if long_enough(partial) {
            let prefixed = self.prefixed(finished, partial, limit).await?;

            // Finish the word, then search for it. The prefix pass says which
            // fragments hold a word starting with those characters; the words in
            // those fragments say which word the reader was most likely typing;
            // and asking the index for *that* gets back an ordering the prefix
            // pass cannot produce, because `search::score` scores the string it
            // is handed and `vecto` is not a term the collection holds — it
            // scores zero for every record that matched.
            if let Some(word) = completion(&prefixed, partial) {
                let completed = if finished.is_empty() {
                    word
                } else {
                    format!("{finished} {word}")
                };
                for hit in self.ranked(&completed, limit).await? {
                    push(&mut hits, hit, room);
                }
            }

            // Whatever the completed word did not account for. A fragment can
            // hold a different word under the same prefix — `vectors` where the
            // word chosen was `vector` — and dropping it because the completion
            // went the other way would lose a result the reader can plainly see
            // is there.
            for found in prefixed {
                push(&mut hits, found.hit, room);
            }
        } else if !finished.is_empty() {
            // The stub is below the engine's floor, so it is dropped rather than
            // sent. Ranking the finished words alone is what the reader is part
            // of the way towards asking for.
            for hit in self.ranked(finished, limit).await? {
                push(&mut hits, hit, room);
            }
        }

        if hits.len() < room {
            for hit in self.corrected(query, limit).await? {
                push(&mut hits, hit, room);
            }
        }
        Ok(hits)
    }

    /// The words the store indexed, ranked against the collection.
    ///
    /// The ordering is the store's — `search::score` over the one indexed field
    /// — and this code does no scoring of its own. See `schema` for why there is
    /// exactly one such field.
    async fn ranked(&mut self, query: &str, limit: u32) -> Result<Vec<Hit>, Fault> {
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
        hits_of(answers.first())
    }

    /// The fragments holding a word that **begins with** the characters as typed.
    ///
    /// Every word before the partial one is finished, and is still asked of the
    /// index as a whole word, so `vector sea` looks only inside what holds the
    /// word `vector`.
    ///
    /// This is an index read. It used to be a `text ILIKE '%partial%'` scan —
    /// not for want of trying, but because the store had no prefix operator when
    /// it was written; the scan stood in for one and the comment here said so.
    /// `MATCHES PREFIX` is that operator, and swapping to it changes two things
    /// beyond the access path. A scan matched the characters **anywhere in a
    /// word**, so typing `earch` found `search` — an infix hit the reader cannot
    /// see the logic of, since they are typing the start of a word and not the
    /// middle of one. And it read every fragment: affordable on a site of a few
    /// hundred, and a full walk on any collection that grows.
    ///
    /// The analyzed `text` travels back as well as the `body`, because it is
    /// what the completion is read out of: it opens with the page title and the
    /// heading, so a word a reader is typing is found there whether they are
    /// typing a title or a sentence.
    async fn prefixed(
        &mut self,
        finished: &str,
        partial: &str,
        limit: u32,
    ) -> Result<Vec<Found>, Fault> {
        // The partial word is a **bound value**, exactly as the finished ones
        // are. It reaches the store as a term to expand and never as syntax.
        let mut parameters = vec![(
            "p".to_owned(),
            tessaridb_client::Value::String(partial.to_owned()),
        )];
        // No order to impose. `search::score` scores the literal it is given, so
        // scoring the prefix would return zero for every record — measured, not
        // assumed. Inventing an order here instead would be this code scoring,
        // which `ranked` deliberately does not do. The store's own order is
        // stable, which is what matters: the same query twice returns the same
        // page twice.
        let script = if finished.is_empty() {
            format!(
                "SELECT page, heading, anchor, body, text
                 FROM fragment
                 WHERE text MATCHES PREFIX $p
                 LIMIT {limit};"
            )
        } else {
            parameters.push((
                "q".to_owned(),
                tessaridb_client::Value::String(finished.to_owned()),
            ));
            format!(
                "SELECT page, heading, anchor, body, text
                 FROM fragment
                 WHERE text MATCHES $q AND text MATCHES PREFIX $p
                 LIMIT {limit};"
            )
        };

        let answers = self.run_with(&script, parameters).await?;
        Ok(records(answers.first())?
            .iter()
            .map(|(_, value)| Found {
                hit: hit_of(value),
                text: text_of(value, "text"),
            })
            .collect())
    }

    /// The fragments holding the word the reader probably meant.
    ///
    /// `MATCHES FUZZY` accepts a term within two edits of each word given, with
    /// the first three characters held exact. It is the last pass and its
    /// results are appended unscored, which is the whole of its precedence: a
    /// guess at a misspelling must never displace a word the reader actually
    /// typed and the collection actually holds.
    ///
    /// It is asked for only when every word clears the engine's three-character
    /// floor, because that floor is a **refusal** rather than a narrowing — one
    /// short word turns the statement into an error for the whole query. A query
    /// that does not clear it simply gets no fuzzy pass.
    ///
    /// This is what answers `vecter`, which the prefix pass cannot: a misspelled
    /// word is not a prefix of the right one.
    async fn corrected(&mut self, query: &str, limit: u32) -> Result<Vec<Hit>, Fault> {
        if !query.split_whitespace().all(long_enough) {
            return Ok(Vec::new());
        }
        let script = format!(
            "SELECT page, heading, anchor, body
             FROM fragment
             WHERE text MATCHES FUZZY $q
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
        hits_of(answers.first())
    }
}

/// Whether a word clears the store's three-character floor for `PREFIX` and
/// `FUZZY`.
///
/// Below it the store **refuses** the statement rather than narrowing it, so
/// this is asked before the term is sent and not after the error comes back.
/// Counted in characters rather than bytes: the floor is on the term as the
/// store reads it, and `длин` is four characters and eight bytes.
fn long_enough(word: &str) -> bool {
    word.chars()
        .filter(|character| character.is_alphanumeric())
        .count()
        >= 3
}

/// A fragment found by prefix: the hit it makes, and the text it was found in.
struct Found {
    hit: Hit,
    text: String,
}

/// Add a hit unless the page already holds it or has no room for it.
///
/// A fragment two passes both find would otherwise arrive twice — the second
/// time unscored, below results that scored lower than it did.
fn push(hits: &mut Vec<Hit>, hit: Hit, room: usize) {
    if hits.len() >= room {
        return;
    }
    if hits
        .iter()
        .any(|held| held.page == hit.page && held.anchor == hit.anchor)
    {
        return;
    }
    hits.push(hit);
}

/// The word the reader is most likely part-way through typing.
///
/// Read out of the text the prefix pass found rather than out of a dictionary,
/// because this code cannot ask the store for its terms — the statement it can
/// write returns records, not the vocabulary behind them. A word counts when it
/// **begins** with what was typed, which is the same question the prefix pass
/// asked; completing `earch` to `search` would be a guess about what the reader
/// meant rather than a reading of what they wrote.
///
/// Ties break on the shorter word and then alphabetically, so the same corpus
/// and the same keystrokes always choose the same word — a completion that
/// wobbled between two spellings would reorder the page under the reader's
/// hands as they typed.
fn completion(prefixed: &[Found], partial: &str) -> Option<String> {
    let typed = partial.to_lowercase();
    let mut counted: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for found in prefixed {
        for word in found
            .text
            .split(|character: char| !character.is_alphanumeric())
        {
            let word = word.to_lowercase();
            if word.len() > typed.len() && word.starts_with(&typed) {
                let seen = counted.entry(word).or_default();
                *seen = seen.saturating_add(1);
            }
        }
    }
    counted
        .into_iter()
        .max_by(|(left, left_count), (right, right_count)| {
            left_count
                .cmp(right_count)
                .then_with(|| right.len().cmp(&left.len()))
                .then_with(|| right.cmp(left))
        })
        .map(|(word, _)| word)
}

/// The finished words of a query, and the one still being typed.
///
/// Split on anything that is not a letter or a digit, the way the analyzer's
/// tokenizer splits — so what comes back as the partial word is a word, and the
/// pattern built from it cannot hold a wildcard. `None` when there are no
/// letters or digits in the query at all.
fn split_last_word(query: &str) -> Option<(&str, &str)> {
    let end = query
        .char_indices()
        .rev()
        .find(|(_, character)| character.is_alphanumeric())
        .map(|(at, character)| at.saturating_add(character.len_utf8()))?;
    let start = query[..end]
        .char_indices()
        .rev()
        .find(|(_, character)| !character.is_alphanumeric())
        .map_or(0, |(at, character)| at.saturating_add(character.len_utf8()));
    Some((query[..start].trim(), &query[start..end]))
}

fn hits_of(answer: Option<&crate::Answer>) -> Result<Vec<Hit>, Fault> {
    Ok(records(answer)?
        .iter()
        .map(|(_, value)| hit_of(value))
        .collect())
}

fn hit_of(value: &tessaridb_client::Value) -> Hit {
    Hit {
        page: text_of(value, "page"),
        heading: text_of(value, "heading"),
        anchor: text_of(value, "anchor"),
        // The snippet is the `body`, never the analyzed `text`: `text` opens
        // with the page title and the heading, which the result already shows
        // above it.
        text: text_of(value, "body"),
        relevance: relevance_of(value),
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

    use super::{Found, Hit, completion, long_enough, non_empty, relevance_of, split_last_word};

    fn prefixed(texts: &[&str]) -> Vec<Found> {
        texts
            .iter()
            .map(|text| Found {
                hit: Hit {
                    page: String::new(),
                    heading: String::new(),
                    anchor: String::new(),
                    text: String::new(),
                    relevance: 0.0,
                },
                text: (*text).to_owned(),
            })
            .collect()
    }

    #[test]
    fn the_word_being_typed_is_read_out_of_the_text_it_was_found_in() {
        // `vector` twice against `vectors` once, so `vector` is the word to ask
        // the index for.
        let found = prefixed(&["Vectors and a vector index", "a vector is a list"]);
        assert_eq!(completion(&found, "vecto"), Some("vector".to_owned()));
        // Case is not a difference: the analyzer folds it, and so does this.
        assert_eq!(completion(&found, "VECT"), Some("vector".to_owned()));
    }

    #[test]
    fn a_tie_is_broken_the_same_way_every_time() {
        // Once each, so the count decides nothing. Without a tie-break the
        // choice would follow map order and the page would reorder itself as
        // the reader typed — the same keystrokes, a different answer.
        let found = prefixed(&["a vector and a vectorised list"]);
        assert_eq!(completion(&found, "vecto"), Some("vector".to_owned()));
    }

    #[test]
    fn the_letters_inside_a_word_do_not_complete_it() {
        // Both the prefix pass and the completion ask the same question, which
        // is the point of the pair: turning `earch` into `search` would be a
        // guess about what the reader meant rather than a reading of what they
        // wrote. The scan this replaced did match an infix, so this assertion
        // used to describe a difference between the two passes and now
        // describes their agreement.
        let found = prefixed(&["full-text search"]);
        assert_eq!(completion(&found, "earch"), None);
        // A word that is exactly what was typed is not a completion either: the
        // ranked pass has already asked the index for that word and come back
        // short, so asking again is a second round trip for the same answer.
        assert_eq!(completion(&found, "search"), None);
    }

    #[test]
    fn the_engines_three_character_floor_is_checked_before_a_term_is_sent() {
        // `PREFIX` and `FUZZY` REFUSE a term below three characters rather than
        // narrowing it, and they refuse per word — so one short word turns the
        // whole statement into an error. Asking here is what keeps a reader
        // part-way through typing from being shown a refusal.
        assert!(!long_enough("a"));
        assert!(!long_enough("se"));
        assert!(long_enough("vec"));
        assert!(long_enough("vector"));
    }

    #[test]
    fn the_floor_counts_characters_and_not_bytes() {
        // The store's floor is on the term as it reads it. Counting bytes would
        // let a three-character Cyrillic word through as if it were six and get
        // the statement refused, and would reject nothing that should pass —
        // the failure would look like an error on one language only.
        assert!(long_enough("длин"));
        assert!(!long_enough("до"));
    }

    #[test]
    fn punctuation_does_not_lift_a_word_over_the_floor() {
        // The analyzer splits on non-alphanumerics, so `s.e.` is two one-letter
        // terms and not a four-character word. Counting the punctuation would
        // send a term the store then refuses.
        assert!(!long_enough("s.e."));
        assert!(!long_enough("a-b"));
    }

    #[test]
    fn the_word_being_typed_is_split_from_the_ones_that_are_finished() {
        assert_eq!(split_last_word("vecto"), Some(("", "vecto")));
        assert_eq!(split_last_word("vector sea"), Some(("vector", "sea")));
        assert_eq!(
            split_last_word("full-text sear"),
            Some(("full-text", "sear"))
        );
        // Trailing punctuation is not a word being typed: the word before it is.
        assert_eq!(split_last_word("vector,"), Some(("", "vector")));
        assert_eq!(
            split_last_word("what is a graph?"),
            Some(("what is a", "graph"))
        );
    }

    #[test]
    fn a_query_with_no_word_in_it_yields_no_partial_pass() {
        // The property the pattern's safety rests on: what comes back is made of
        // letters and digits, so `%` and `_` cannot reach `ILIKE` as wildcards.
        assert_eq!(split_last_word("%"), None);
        assert_eq!(split_last_word("%_%"), None);
        assert_eq!(split_last_word("   "), None);
        assert_eq!(split_last_word("100%"), Some(("", "100")));
    }

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
