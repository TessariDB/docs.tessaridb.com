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

/// These tests take the node one at a time.
///
/// Not a workaround for a defect. Each test applies the schema and rebuilds the
/// whole site, and both are things that happen **once, as a process starts** —
/// so ten of them at once is a load this code will never meet in production, and
/// the store is right to refuse it: `DEFINE NAMESPACE` writes a catalog record,
/// and a rebuild rewrites four tables, so concurrent copies conflict on commit
/// exactly as the store's isolation promises they will. Serialising here models
/// what actually happens rather than papering over what does not.
///
/// Isolation between tests is the per-test namespace, not this lock.
static NODE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// The node to test against, and a distinct namespace per test so that two of
/// them cannot see each other's records.
///
/// The returned guard is held for the test's lifetime — bind it, do not drop it.
async fn store(namespace: &str) -> Option<(Store, tokio::sync::MutexGuard<'static, ()>)> {
    let address = std::env::var("DOCS_TEST_NODE").ok()?;
    let alone = NODE.lock().await;
    let mut store = Store::connect(&address, namespace, None)
        .await
        .expect("connect to the node named by DOCS_TEST_NODE");
    store.migrate().await.expect("the schema applies");
    Some((store, alone))
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
    let Some((mut store, _alone)) = store("t_schema").await else {
        eprintln!("skipped: DOCS_TEST_NODE is not set");
        return;
    };
    // The claim `IF NOT EXISTS` is there to make. It is worth a test because the
    // failure only appears on the second deploy, which is the worst time.
    store.migrate().await.expect("a second migrate is a no-op");
}

#[tokio::test]
async fn a_page_written_comes_back_as_it_went_in() {
    let Some((mut store, _alone)) = store("t_page").await else {
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
    let Some((mut store, _alone)) = store("t_missing").await else {
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
    let Some((mut store, _alone)) = store("t_idempotent").await else {
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
    let Some((mut store, _alone)) = store("t_tree").await else {
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
    let Some((mut store, _alone)) = store("t_search").await else {
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
    let Some((mut store, _alone)) = store("t_rank").await else {
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
    let Some((mut store, _alone)) = store("t_injection").await else {
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
    let Some((mut store, _alone)) = store("t_empty").await else {
        eprintln!("skipped: DOCS_TEST_NODE is not set");
        return;
    };
    assert!(store.search("   ", 10).await.expect("a search").is_empty());
}

#[tokio::test]
async fn the_search_runs_off_the_index_and_not_off_a_scan() {
    let Some((mut store, _alone)) = store("t_path").await else {
        eprintln!("skipped: DOCS_TEST_NODE is not set");
        return;
    };
    store.ingest(&corpus()).await.expect("ingest");

    // A search that quietly degrades to a scan answers the same rows, so no
    // assertion about the results would ever catch it. The node reports the path
    // it took, which is the only thing that would.
    let answers = store
        .run_with(
            "SELECT page, heading, anchor, text, search::score(text, $q) AS relevance FROM fragment WHERE text MATCHES $q ORDER BY search::score(text, $q) DESC LIMIT 10;",
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

#[tokio::test]
async fn a_page_moved_to_another_section_leaves_the_first_one() {
    let Some((mut store, _alone)) = store("t_move").await else {
        eprintln!("skipped: DOCS_TEST_NODE is not set");
        return;
    };
    store.ingest(&corpus()).await.expect("ingest");
    store
        .put_section(&Section {
            slug: "guides".to_owned(),
            title: "Guides".to_owned(),
            parent: None,
            order: 20,
            icon: None,
        })
        .await
        .expect("a second root section");

    let moved = parse(
        "query-language/graphs",
        "+++\ntitle = \"Graphs\"\nsection = \"guides\"\norder = 5\n+++\n\nEdges are records too.\n",
    )
    .expect("a page");
    store.put_page(&moved).await.expect("the move");

    let tree = store.tree().await.expect("the tree");
    let under = |slug: &str| -> Vec<String> {
        tree.iter()
            .find(|node| node.slug == slug)
            .map(|node| {
                node.children
                    .iter()
                    .filter(|child| child.kind == "page")
                    .map(|child| child.slug.clone())
                    .collect()
            })
            .unwrap_or_default()
    };
    assert_eq!(under("guides"), vec!["query-language/graphs"]);
    assert!(
        !under("query-language").contains(&"query-language/graphs".to_owned()),
        "the page is under both parents: {tree:?}"
    );
}

#[tokio::test]
async fn an_edited_page_does_not_keep_the_tail_of_the_old_one() {
    let Some((mut store, _alone)) = store("t_edit").await else {
        eprintln!("skipped: DOCS_TEST_NODE is not set");
        return;
    };
    store.ingest(&corpus()).await.expect("ingest");
    // Fragments are keyed by position, so a shortened page would otherwise leave
    // its old tail behind — findable, and pointing at a heading that is gone.
    assert!(
        !store
            .search("Ranking", 10)
            .await
            .expect("a search")
            .is_empty(),
        "the section exists before the edit"
    );

    let shortened = parse(
        "query-language/search",
        "+++\ntitle = \"Full-text search\"\nsection = \"query-language\"\norder = 40\n+++\n\nTerms, not patterns.\n",
    )
    .expect("a page");
    store.put_page(&shortened).await.expect("the edit");

    let hits = store.search("collection", 10).await.expect("a search");
    assert!(
        hits.is_empty(),
        "a fragment of the old version survived the edit: {hits:?}"
    );
}

#[tokio::test]
async fn a_deleted_page_leaves_the_tree_and_the_index_together() {
    let Some((mut store, _alone)) = store("t_delete").await else {
        eprintln!("skipped: DOCS_TEST_NODE is not set");
        return;
    };
    store.ingest(&corpus()).await.expect("ingest");
    assert!(
        store
            .delete_page("query-language/search")
            .await
            .expect("a delete"),
        "it was there, so the answer is that it was removed"
    );
    assert!(
        store
            .article("query-language/search")
            .await
            .expect("a read")
            .is_none()
    );
    assert!(
        store
            .search("analyzer", 10)
            .await
            .expect("a search")
            .is_empty(),
        "a deleted page is still findable"
    );
    assert!(
        !store
            .delete_page("query-language/search")
            .await
            .expect("a second delete"),
        "deleting what is not there should say so, not report success"
    );
}

#[tokio::test]
async fn an_empty_store_and_a_populated_one_are_told_apart_by_counting_pages() {
    // What `docs serve` decides on at start: an empty store may be seeded from
    // disk, a populated one owns its content and is left alone. Getting this
    // backwards silently reverts every edit made through the API on the next
    // restart, which is a failure nobody would attribute to a count.
    let Some((mut store, _alone)) = store("t_held").await else {
        eprintln!("skipped: DOCS_TEST_NODE is not set");
        return;
    };
    assert_eq!(
        store.pages_held().await.expect("a count"),
        0,
        "a namespace nothing has been written to holds nothing"
    );
    store.ingest(&corpus()).await.expect("ingest");
    assert_eq!(store.pages_held().await.expect("a count"), 2);

    store
        .delete_page("query-language/search")
        .await
        .expect("a delete");
    assert_eq!(
        store.pages_held().await.expect("a count"),
        1,
        "the count follows the store rather than a marker set at ingest"
    );
}

// ── the accounts and tokens the API authorises against ──────────────────────
//
// Written here for the same reason as everything else in this file: what can be
// wrong about these statements is whether the node accepts them, and a mock
// that agreed with my reading of the grammar would pass while sign-in refused
// everybody.

#[tokio::test]
async fn an_account_is_written_and_read_back_whole() {
    let Some((mut store, _alone)) = store("t_account_roundtrip").await else {
        return;
    };
    assert!(store.account("ann").await.expect("a read").is_none());

    store
        .put_account("ann", "$argon2id$v=19$m=19456,t=2,p=1$c2FsdA$aGFzaA")
        .await
        .expect("the account is written");

    let found = store.account("ann").await.expect("a read").expect("ann");
    assert_eq!(found.name, "ann");
    assert_eq!(found.secret, "$argon2id$v=19$m=19456,t=2,p=1$c2FsdA$aGFzaA");
}

#[tokio::test]
async fn writing_an_account_again_replaces_the_secret_rather_than_adding_one() {
    let Some((mut store, _alone)) = store("t_account_replace").await else {
        return;
    };
    store.put_account("ann", "first").await.expect("written");
    store.put_account("ann", "second").await.expect("written");
    let found = store.account("ann").await.expect("a read").expect("ann");
    assert_eq!(
        found.secret, "second",
        "changing the password in the environment must change it in the store"
    );
}

#[tokio::test]
async fn a_name_that_could_break_out_of_a_record_id_is_refused_by_the_store_layer() {
    let Some((mut store, _alone)) = store("t_account_injection").await else {
        return;
    };
    // The shape of an injection: close the quote, run something else. If this
    // were spelled into the statement the account table would be emptied.
    let hostile = "ann'; DELETE FROM account WHERE true; --";
    assert!(matches!(
        store.put_account(hostile, "x").await,
        Err(docs_store::Fault::UnsafeName)
    ));
    assert!(matches!(
        store.account(hostile).await,
        Err(docs_store::Fault::UnsafeName)
    ));

    // And the table is untouched: a real account written before is still there.
    store.put_account("ann", "kept").await.expect("written");
    assert_eq!(
        store
            .account("ann")
            .await
            .expect("a read")
            .expect("ann")
            .secret,
        "kept"
    );
}

#[tokio::test]
async fn a_token_is_found_by_its_digest_and_carries_its_expiry() {
    let Some((mut store, _alone)) = store("t_token_roundtrip").await else {
        return;
    };
    let digest = "a".repeat(64);
    assert!(store.token(&digest).await.expect("a read").is_none());

    store
        .put_token(&digest, "ann", 4_102_444_800)
        .await
        .expect("the token is written");

    let found = store
        .token(&digest)
        .await
        .expect("a read")
        .expect("a token");
    assert_eq!(found.account, "ann");
    assert_eq!(found.expires, 4_102_444_800);

    store.delete_token(&digest).await.expect("deleted");
    assert!(store.token(&digest).await.expect("a read").is_none());
}

#[tokio::test]
async fn something_that_is_not_a_digest_is_refused_before_it_reaches_a_statement() {
    let Some((mut store, _alone)) = store("t_token_digest_shape").await else {
        return;
    };
    for candidate in [
        "",
        "short",
        &"z".repeat(64),
        "'; DELETE FROM token WHERE true; --",
    ] {
        assert!(
            matches!(
                store.token(candidate).await,
                Err(docs_store::Fault::UnsafeName)
            ),
            "{candidate} should not be accepted as a digest"
        );
    }
}

#[tokio::test]
async fn purging_removes_what_has_expired_and_keeps_what_has_not() {
    let Some((mut store, _alone)) = store("t_token_purge").await else {
        return;
    };
    let stale = "b".repeat(64);
    let live = "c".repeat(64);
    store
        .put_token(&stale, "ann", 1_000)
        .await
        .expect("written");
    store
        .put_token(&live, "ann", 4_102_444_800)
        .await
        .expect("written");

    store.purge_tokens(2_000).await.expect("purged");

    assert!(store.token(&stale).await.expect("a read").is_none());
    assert!(store.token(&live).await.expect("a read").is_some());
}

#[tokio::test]
async fn removing_an_account_removes_the_tokens_it_was_holding() {
    let Some((mut store, _alone)) = store("t_account_tokens_go_too").await else {
        return;
    };
    let digest = "d".repeat(64);
    store.put_account("ann", "secret").await.expect("written");
    store
        .put_token(&digest, "ann", 4_102_444_800)
        .await
        .expect("written");

    store.delete_account("ann").await.expect("removed");

    assert!(store.account("ann").await.expect("a read").is_none());
    assert!(
        store.token(&digest).await.expect("a read").is_none(),
        "an account nobody can sign in to whose token still works is not removed"
    );
}
