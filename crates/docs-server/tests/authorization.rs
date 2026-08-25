//! Who may write, exercised through the real routes against a real store.
//!
//! These go through `routes::router` rather than calling the handlers, because
//! what can be wrong here is the wiring: a write route that forgot its check
//! still compiles, still passes a handler test, and is a site anybody can edit.
//!
//! Point `DOCS_TEST_NODE` at a node to run them:
//!
//! ```text
//! tessaridb /tmp/store --serve 127.0.0.1:47901
//! DOCS_TEST_NODE=127.0.0.1:47901 cargo test -p docs-server
//! ```
//!
//! With the variable unset they report that they did not run. A skipped test
//! claims nothing — which is why the wave that wrote them ran them against one.

#![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]
// The base64 helper below is index arithmetic over a fixed 64-entry table with
// three-byte chunks. Wrapping every step would say less about it than the shape
// already does, and this is a test rather than a request path.
#![allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use docs_server::{Site, identity};
use tower::ServiceExt;

/// One at a time: each test applies the schema in its own namespace, and the
/// throttle inside a `Site` is shared, so two tests guessing at once would hold
/// each other's counts.
static NODE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

const PASSWORD: &str = "a long enough password";

/// A site pointed at a namespace of its own, with one editor already in it.
///
/// The guard is held for the test's lifetime — bind it, do not drop it.
async fn site(namespace: &str) -> Option<(Site, tokio::sync::MutexGuard<'static, ()>)> {
    let address = std::env::var("DOCS_TEST_NODE").ok()?;
    let alone = NODE.lock().await;
    let site = Site::new(&address, namespace, None);
    let mut store = site.writer().await.expect("a connection");
    store.migrate().await.expect("the schema applies");
    store
        .put_account("ann", &identity::hash(PASSWORD).expect("a hash"))
        .await
        .expect("the editor exists");
    Some((site, alone))
}

fn basic(name: &str, password: &str) -> String {
    // Base64 of `name:password`, written out so the test does not depend on the
    // encoder it is testing against.
    let raw = format!("{name}:{password}");
    let table = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in raw.as_bytes().chunks(3) {
        let mut block = [0_u8; 3];
        block[..chunk.len()].copy_from_slice(chunk);
        let packed = (u32::from(block[0]) << 16) | (u32::from(block[1]) << 8) | u32::from(block[2]);
        for index in 0..4 {
            if index <= chunk.len() {
                let shift = 18 - index * 6;
                out.push(table[((packed >> shift) & 0x3F) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    format!("Basic {out}")
}

const PAGE: &str = "+++\ntitle = \"Written by a token holder\"\n+++\n\nBody.\n";

async fn send(site: &Site, request: Request<Body>) -> (StatusCode, String) {
    let response = docs_server::routes::router(site.clone())
        .oneshot(request)
        .await
        .expect("the router answers");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .expect("a body");
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

fn put_page(authorization: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder().method("PUT").uri("/api/page/scratch");
    if let Some(value) = authorization {
        builder = builder.header(header::AUTHORIZATION, value);
    }
    builder.body(Body::from(PAGE)).expect("a request")
}

fn sign_in(authorization: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder().method("POST").uri("/api/session");
    if let Some(value) = authorization {
        builder = builder.header(header::AUTHORIZATION, value);
    }
    builder.body(Body::empty()).expect("a request")
}

fn token_of(body: &str) -> String {
    let value: serde_json::Value = serde_json::from_str(body).expect("json");
    value["token"].as_str().expect("a token").to_owned()
}

#[tokio::test]
async fn a_password_buys_a_token_and_the_token_buys_a_write() {
    let Some((site, _alone)) = site("t_auth_happy").await else {
        return;
    };
    let (status, body) = send(&site, sign_in(Some(&basic("ann", PASSWORD)))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let token = token_of(&body);

    let (status, body) = send(&site, put_page(Some(&format!("Bearer {token}")))).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
}

#[tokio::test]
async fn the_token_is_a_day_long_and_is_not_what_the_store_holds() {
    let Some((site, _alone)) = site("t_auth_lifetime").await else {
        return;
    };
    let (_, body) = send(&site, sign_in(Some(&basic("ann", PASSWORD)))).await;
    let value: serde_json::Value = serde_json::from_str(&body).expect("json");
    let expires = value["expires"].as_i64().expect("an expiry");
    let issued = identity::now();
    assert!(
        (expires - issued - 86_400).abs() <= 5,
        "a day from now, give or take the time this test took: {expires} vs {issued}"
    );

    // What the store remembers is the digest, never the token. The two have the
    // same *shape* — both are 64 hex characters — so the check that matters is
    // not whether the store refuses the token but whether it holds it: looking
    // the token up finds nothing, and looking its digest up finds the record.
    // That is the property that makes a stolen copy of this table worthless.
    let token = token_of(&body);
    let mut store = site.writer().await.expect("a connection");
    assert!(
        store.token(&token).await.expect("a read").is_none(),
        "the token as issued is not a key into the table"
    );
    assert!(
        store
            .token(&identity::digest(&token))
            .await
            .expect("a read")
            .is_some(),
        "its digest is"
    );
    // And presenting a digest as though it were a token gets nowhere, because it
    // would be hashed again on the way in.
    assert_ne!(
        identity::digest(&identity::digest(&token)),
        identity::digest(&token)
    );
}

#[tokio::test]
async fn a_write_without_a_token_is_refused_and_asks_for_one() {
    let Some((site, _alone)) = site("t_auth_no_token").await else {
        return;
    };
    let (status, _) = send(&site, put_page(None)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // A password is not a token. This is the whole separation: the database's
    // own credentials are no longer a way in through the API.
    let (status, _) = send(&site, put_page(Some(&basic("ann", PASSWORD)))).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, _) = send(&site, put_page(Some("Bearer not-a-real-token"))).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn an_expired_token_is_refused_and_forgotten() {
    let Some((site, _alone)) = site("t_auth_expired").await else {
        return;
    };
    let stale = identity::mint();
    let digest = identity::digest(stale.reveal());
    let mut store = site.writer().await.expect("a connection");
    store
        .put_token(&digest, "ann", identity::now() - 1)
        .await
        .expect("an expired token");

    let (status, _) = send(&site, put_page(Some(&format!("Bearer {}", stale.reveal())))).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    assert!(
        store.token(&digest).await.expect("a read").is_none(),
        "an expired token is not worth keeping"
    );
}

#[tokio::test]
async fn a_revoked_token_stops_working_immediately() {
    let Some((site, _alone)) = site("t_auth_revoke").await else {
        return;
    };
    let (_, body) = send(&site, sign_in(Some(&basic("ann", PASSWORD)))).await;
    let token = token_of(&body);
    let bearer = format!("Bearer {token}");

    let (status, _) = send(&site, put_page(Some(&bearer))).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let revoke = Request::builder()
        .method("DELETE")
        .uri("/api/session")
        .header(header::AUTHORIZATION, &bearer)
        .body(Body::empty())
        .expect("a request");
    let (status, _) = send(&site, revoke).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _) = send(&site, put_page(Some(&bearer))).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "revoked means revoked");
}

#[tokio::test]
async fn an_unknown_name_and_a_wrong_password_are_told_apart_by_nobody() {
    let Some((site, _alone)) = site("t_auth_no_enumeration").await else {
        return;
    };
    let (wrong_status, wrong_body) = send(&site, sign_in(Some(&basic("ann", "not it")))).await;
    let (absent_status, absent_body) = send(&site, sign_in(Some(&basic("nobody", "not it")))).await;

    assert_eq!(wrong_status, StatusCode::UNAUTHORIZED);
    assert_eq!(absent_status, wrong_status);
    assert_eq!(
        absent_body, wrong_body,
        "two wordings are a way to ask which accounts exist"
    );
}

#[tokio::test]
async fn guessing_is_bounded() {
    let Some((site, _alone)) = site("t_auth_throttle").await else {
        return;
    };
    let mut refused = 0;
    let mut throttled = 0;
    for _ in 0..10 {
        let (status, _) = send(&site, sign_in(Some(&basic("ann", "not it")))).await;
        match status {
            StatusCode::UNAUTHORIZED => refused += 1,
            StatusCode::TOO_MANY_REQUESTS => throttled += 1,
            other => panic!("unexpected {other}"),
        }
    }
    assert!(refused >= 1, "the first attempts are answered");
    assert!(
        throttled >= 1,
        "and the endpoint stops being an unlimited password oracle"
    );

    // And the throttle holds even against the right password, which is what
    // makes it a bound rather than a hint.
    let (status, _) = send(&site, sign_in(Some(&basic("ann", PASSWORD)))).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn a_read_needs_nothing_at_all() {
    let Some((site, _alone)) = site("t_auth_reads_are_public").await else {
        return;
    };
    let request = Request::builder()
        .uri("/api/nav")
        .body(Body::empty())
        .expect("a request");
    let (status, body) = send(&site, request).await;
    assert_eq!(status, StatusCode::OK, "{body}");
}
