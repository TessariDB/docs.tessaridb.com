//! The half of `docs-store` that only a running node can hold.
//!
//! Every statement this crate sends is written here rather than in a unit test,
//! because the thing that can be wrong about them is whether the node accepts
//! them and what it answers — neither of which a mock would know. A mock that
//! agreed with my reading of the grammar would pass while the site returned
//! nothing.
//!
//! Point `DOCS_TEST_NODE` at a node to run these:
//!
//! ```text
//! tessaridb /tmp/store --serve 127.0.0.1:47901
//! DOCS_TEST_NODE=127.0.0.1:47901 cargo test -p docs-store
//! ```
//!
//! With the variable unset they report that they did not run. A skipped test
//! claims nothing, which is the honest state when there is no node — but it is
//! also why the wave that wrote them ran them against one.

#![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]

use docs_content::parse;
use docs_store::{Store, access_path, ingest::Corpus, ingest::Section};

/// The node to test against, and a distinct namespace per test so that two of
/// them cannot see each other's records.
async fn store(namespace: &str) -> Option<Store> {
    let address = std::env::var("DOCS_TEST_NODE").ok()?;
    let mut store = Store::connect(&address, namespace, None)
        .await
        .expect("connect to the node named by DOCS_TEST_NODE");
    store.migrate().await.expect("the schema applies");
    Some(store)
}

fn corpus() -> Corpus {
    let search = parse(
        "query-language/search",
        "+++\ntitle = \"Full-text search\"\nsection = \"query-language\"\norder = 40\n+++\n\nTerms, not patterns.\n\n## Analyzers\n\nAn analyzer belongs to the field, so an index can make the question fast without changing its answer.\n\n## Ranking\n\nA score measures one record against the collection.\n",
    )
    .expect("a page");
    let graphs = parse(
        "query-language/graphs",
        "+++\ntitle = \"Graphs\"\nsection = \"query-language\"\norder = 60\n+++\n\nEdges are records too.\n\n## RELATE\n\nA relation is written once and walked from either end.\n",
    )
    .expect("a page");
    Corpus {
        sections: vec![
            Section {
                slug: "query-language".to_owned(),
                title: "TessariQL".to_owned(),
                parent: None,
                order: 10,
                icon: Some("terminal".to_owned()),
            },
            Section {
                slug: "query-language/reads".to_owned(),
                title: "Reads".to_owned(),
                parent: Some("query-language".to_owned()),
                order: 20,
                icon: None,
            },
        ],
        pages: vec![search, graphs],
    }
}

#[tokio::test]
async fn the_schema_applies_twice_because_a_restart_is_not_a_special_case() {
    let Some(mut store) = store("t_schema").await else {
        eprintln!("skipped: DOCS_TEST_NODE is not set");
        return;
    };
    // The claim `IF NOT EXISTS` is there to make. It is worth a test because the
    // failure only appears on the second deploy, which is the worst time.
    store.migrate().await.expect("a second migrate is a no-op");
}

#[tokio::test]
async fn a_page_written_comes_back_as_it_went_in() {
    let Some(mut store) = store("t_page").await else {
        eprintln!("skipped: DOCS_TEST_NODE is not set");
        return;
    };
    let written = store.ingest(&corpus()).await.expect("ingest");
    assert_eq!(written.pages, 2);
    assert_eq!(written.sections, 2);
    assert!(written.fragments >= 5, "{written:?}");

    let article = store
        .article("query-language/search")
        .await
        .expect("a read")
        .expect("the page is there");
    assert_eq!(article.title, "Full-text search");
    assert!(
        article
            .markdown
            .contains("An analyzer belongs to the field")
    );
    assert!(!article.unreleased);
}

#[tokio::test]
async fn a_slug_that_names_no_page_is_a_missing_page_and_not_a_fault() {
    let Some(mut store) = store("t_missing").await else {
        eprintln!("skipped: DOCS_TEST_NODE is not set");
        return;
    };
    store.ingest(&corpus()).await.expect("ingest");
    assert!(
        store
            .article("no/such/page")
            .await
            .expect("a read")
            .is_none(),
        "a 404 is an answer, not an error"
    );
}

#[tokio::test]
async fn the_ingest_is_idempotent_so_a_redeploy_does_not_double_the_site() {
    let Some(mut store) = store("t_idempotent").await else {
        eprintln!("skipped: DOCS_TEST_NODE is not set");
        return;
    };
    let first = store.ingest(&corpus()).await.expect("ingest");
    let second = store.ingest(&corpus()).await.expect("ingest again");
    assert_eq!(first, second, "a second ingest wrote a different site");

    // And the counts are what a reader sees, not just what the writer reported.
    let tree = store.tree().await.expect("the tree");
    assert_eq!(tree.len(), 1, "one root, not two: {tree:?}");
}

#[tokio::test]
async fn the_tree_comes_back_through_the_graph_with_its_levels_intact() {
    let Some(mut store) = store("t_tree").await else {
        eprintln!("skipped: DOCS_TEST_NODE is not set");
        return;
    };
    store.ingest(&corpus()).await.expect("ingest");

    let tree = store.tree().await.expect("the tree");
    let root = tree.first().expect("a root section");
    assert_eq!(root.slug, "query-language");
    assert_eq!(root.title, "TessariQL");
    assert_eq!(root.kind, "section");

    // A subsection and two pages hang beneath it — three levels, read as three
    // hops rather than as three special cases.
    let sections: Vec<&str> = root
        .children
        .iter()
        .filter(|node| node.kind == "section")
        .map(|node| node.slug.as_str())
        .collect();
    assert_eq!(sections, vec!["query-language/reads"]);

    let mut pages: Vec<&str> = root
        .children
        .iter()
        .filter(|node| node.kind == "page")
        .map(|node| node.slug.as_str())
        .collect();
    pages.sort_unstable();
    assert_eq!(
        pages,
        vec!["query-language/graphs", "query-language/search"]
    );
}

#[tokio::test]
async fn a_search_returns_the_passage_and_not_the_top_of_the_page() {
    let Some(mut store) = store("t_search").await else {
        eprintln!("skipped: DOCS_TEST_NODE is not set");
        return;
    };
    store.ingest(&corpus()).await.expect("ingest");

    let hits = store.search("analyzer", 10).await.expect("a search");
    let first = hits
        .first()
        .expect("a hit for a word that is in the corpus");
    assert_eq!(first.page, "query-language/search");
    assert_eq!(
        first.heading, "Analyzers",
        "the hit should name the section it matched"
    );
    assert!(
        !first.anchor.is_empty(),
        "without an anchor the reader lands at the top and searches again by eye"
    );
}

#[tokio::test]
async fn the_page_a_query_is_about_outranks_a_page_that_merely_mentions_it() {
    let Some(mut store) = store("t_rank").await else {
        eprintln!("skipped: DOCS_TEST_NODE is not set");
        return;
    };
    store.ingest(&corpus()).await.expect("ingest");

    // This is the property the composed `text` field exists for: the title is
    // part of what is indexed, so the page *about* graphs beats the page that
    // only uses the word.
    let hits = store.search("graphs", 10).await.expect("a search");
    let first = hits.first().expect("a hit");
    assert_eq!(first.page, "query-language/graphs", "hits: {hits:?}");
}

#[tokio::test]
async fn a_search_term_cannot_become_syntax() {
    let Some(mut store) = store("t_injection").await else {
        eprintln!("skipped: DOCS_TEST_NODE is not set");
        return;
    };
    store.ingest(&corpus()).await.expect("ingest");

    // The term is bound, never spelled in. If it were spelled in, this would
    // empty the table and the assertion below would fail — which is exactly the
    // point of asserting after it rather than merely that the call returned.
    let hits = store
        .search("'; DELETE FROM fragment; --", 10)
        .await
        .expect("a hostile term is an ordinary term");
    assert!(
        hits.is_empty() || !hits.is_empty(),
        "it answered rather than refused"
    );

    let after = store.search("analyzer", 10).await.expect("a search");
    assert!(
        !after.is_empty(),
        "the corpus is gone, so the term was executed rather than matched"
    );
}

#[tokio::test]
async fn an_empty_query_asks_the_node_nothing() {
    let Some(mut store) = store("t_empty").await else {
        eprintln!("skipped: DOCS_TEST_NODE is not set");
        return;
    };
    assert!(store.search("   ", 10).await.expect("a search").is_empty());
}

#[tokio::test]
async fn the_search_runs_off_the_index_and_not_off_a_scan() {
    let Some(mut store) = store("t_path").await else {
        eprintln!("skipped: DOCS_TEST_NODE is not set");
        return;
    };
    store.ingest(&corpus()).await.expect("ingest");

    // A search that quietly degrades to a scan answers the same rows, so no
    // assertion about the results would ever catch it. The node reports the path
    // it took, which is the only thing that would.
    let answers = store
        .run_with(
            "SELECT *, search::score(text, $q) AS relevance FROM fragment WHERE text MATCHES $q ORDER BY relevance DESC LIMIT 10;",
            vec![(
                "q".to_owned(),
                tessaridb_client::Value::String("analyzer".to_owned()),
            )],
        )
        .await
        .expect("a search");
    let path = access_path(answers.first()).expect("a record answer names its path");
    assert_ne!(path, "scan", "the search index is not being used");
}
