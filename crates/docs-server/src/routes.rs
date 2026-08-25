//! The routes, and the one rule that decides every status code.
//!
//! # What a status means here
//!
//! A refusal from the store is **not** a server error, and reporting it as one
//! would send an operator to the logs when the answer is in the request. So:
//!
//! | what happened | status |
//! |---|---|
//! | the slug names nothing | 404 |
//! | no credentials on a write | 401, with a challenge |
//! | the store refused the statement | 403 — the caller may not do this |
//! | the slug could not be a record id | 400 |
//! | the node is unreachable | 503 |
//!
//! The 401/403 split is the part worth being careful about: 401 says *identify
//! yourself*, 403 says *you did, and the answer is no*. Collapsing them makes a
//! client retry credentials that will never work.

use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use docs_content::parse;
use docs_store::ingest::Section;
use docs_store::{Fault, Store};
use serde::Deserialize;

use crate::{Site, auth};

/// Every route the site serves.
pub fn router(site: Site) -> Router {
    Router::new()
        .route("/api/nav", get(nav))
        .route("/api/search", get(search))
        .route(
            "/api/page/{*slug}",
            get(page).put(put_page).delete(delete_page),
        )
        .route(
            "/api/section/{slug}",
            get(section).put(put_section).delete(delete_section),
        )
        .route("/api/health", get(health))
        .with_state(site)
}

/// What a search asks for.
#[derive(Debug, Deserialize)]
pub struct Asked {
    /// The terms.
    #[serde(default)]
    q: String,
    /// How many results. Clamped by the store.
    #[serde(default = "twenty")]
    limit: u32,
}

const fn twenty() -> u32 {
    20
}

/// What a section looks like on the wire.
#[derive(Debug, Deserialize)]
pub struct SectionBody {
    title: String,
    #[serde(default)]
    parent: Option<String>,
    #[serde(default)]
    order: i64,
    #[serde(default)]
    icon: Option<String>,
}

async fn health() -> &'static str {
    "ok"
}

async fn nav(State(site): State<Site>) -> Response {
    match site.reader().await {
        Ok(mut store) => match store.tree().await {
            Ok(tree) => json(StatusCode::OK, &tree),
            Err(fault) => refused(&fault),
        },
        Err(fault) => refused(&fault),
    }
}

async fn page(State(site): State<Site>, Path(slug): Path<String>) -> Response {
    match site.reader().await {
        Ok(mut store) => match store.article(&slug).await {
            Ok(Some(article)) => json(StatusCode::OK, &article),
            Ok(None) => message(StatusCode::NOT_FOUND, "no such page"),
            Err(fault) => refused(&fault),
        },
        Err(fault) => refused(&fault),
    }
}

async fn section(State(site): State<Site>, Path(slug): Path<String>) -> Response {
    match site.reader().await {
        Ok(mut store) => match store.subtree(&slug).await {
            Ok(found) => json(StatusCode::OK, &found),
            Err(fault) => refused(&fault),
        },
        Err(fault) => refused(&fault),
    }
}

async fn search(State(site): State<Site>, Query(asked): Query<Asked>) -> Response {
    match site.reader().await {
        Ok(mut store) => match store.search(&asked.q, asked.limit).await {
            Ok(hits) => json(StatusCode::OK, &hits),
            Err(fault) => refused(&fault),
        },
        Err(fault) => refused(&fault),
    }
}

async fn put_page(
    State(site): State<Site>,
    Path(slug): Path<String>,
    headers: HeaderMap,
    source: String,
) -> Response {
    // The body is the page's source — front matter and Markdown, exactly what
    // lives in `content/`. One format for an editor and for the repository, so a
    // page written through the API can be committed and a page committed can be
    // edited, without a converter in between that would be a third thing to keep
    // correct.
    let page = match parse(&slug, &source) {
        Ok(page) => page,
        Err(fault) => return message(StatusCode::BAD_REQUEST, &fault.to_string()),
    };
    let mut store = match writer(&site, &headers).await {
        Ok(store) => store,
        Err(refusal) => return *refusal,
    };
    match store.put_page(&page).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(fault) => refused(&fault),
    }
}

async fn delete_page(
    State(site): State<Site>,
    Path(slug): Path<String>,
    headers: HeaderMap,
) -> Response {
    let mut store = match writer(&site, &headers).await {
        Ok(store) => store,
        Err(refusal) => return *refusal,
    };
    match store.delete_page(&slug).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => message(StatusCode::NOT_FOUND, "no such page"),
        Err(fault) => refused(&fault),
    }
}

async fn put_section(
    State(site): State<Site>,
    Path(slug): Path<String>,
    headers: HeaderMap,
    body: String,
) -> Response {
    let asked: SectionBody = match serde_json::from_str(&body) {
        Ok(asked) => asked,
        Err(fault) => return message(StatusCode::BAD_REQUEST, &fault.to_string()),
    };
    let section = Section {
        slug,
        title: asked.title,
        parent: asked.parent,
        order: asked.order,
        icon: asked.icon,
    };
    let mut store = match writer(&site, &headers).await {
        Ok(store) => store,
        Err(refusal) => return *refusal,
    };
    match store.put_section(&section).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(fault) => refused(&fault),
    }
}

async fn delete_section(
    State(site): State<Site>,
    Path(slug): Path<String>,
    headers: HeaderMap,
) -> Response {
    let mut store = match writer(&site, &headers).await {
        Ok(store) => store,
        Err(refusal) => return *refusal,
    };
    match store.delete_section(&slug).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => message(StatusCode::NOT_FOUND, "no such section"),
        Err(fault) => refused(&fault),
    }
}

/// A connection carrying the caller's credentials, or the response that says why
/// there is not one.
///
/// The only thing decided here is whether credentials were *presented*. Whether
/// they are good, and whether that user may write, is the store's answer.
async fn writer(site: &Site, headers: &HeaderMap) -> Result<Store, Box<Response>> {
    let header = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok());
    let Some(caller) = auth::caller(header) else {
        return Err(Box::new(challenge()));
    };
    site.writer(&caller)
        .await
        .map_err(|fault| Box::new(refused(&fault)))
}

/// 401, with the challenge a client needs in order to ask for credentials.
fn challenge() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [(header::WWW_AUTHENTICATE, "Basic realm=\"docs\"")],
        "credentials required",
    )
        .into_response()
}

/// Turns a store fault into the status that tells the caller what to do next.
fn refused(fault: &Fault) -> Response {
    match fault {
        // A slug that cannot be a record id came from the request, so it is the
        // request that is wrong.
        Fault::UnsafeSlug(slug) => message(
            StatusCode::BAD_REQUEST,
            &format!("not a usable path: {slug}"),
        ),
        Fault::Client(_) if fault.refusal().is_some() => {
            // The store said no. That is about this caller and this statement,
            // never about the server being broken — 403, and the store's own
            // words, which already name the place in the script.
            let said = fault.refusal().unwrap_or("refused");
            log::info!("the store refused: {said}");
            message(StatusCode::FORBIDDEN, said)
        }
        Fault::Client(_) => {
            log::warn!("the store is unreachable: {fault}");
            message(StatusCode::SERVICE_UNAVAILABLE, "the store is unavailable")
        }
        Fault::Unexpected { .. } => {
            log::error!("the store answered a shape this build does not read: {fault}");
            message(StatusCode::BAD_GATEWAY, "the store answered unexpectedly")
        }
    }
}

fn json<T: serde::Serialize>(status: StatusCode, body: &T) -> Response {
    match serde_json::to_string(body) {
        Ok(text) => (
            status,
            [(header::CONTENT_TYPE, "application/json; charset=utf-8")],
            text,
        )
            .into_response(),
        Err(fault) => {
            log::error!("an answer would not serialise: {fault}");
            message(StatusCode::INTERNAL_SERVER_ERROR, "could not answer")
        }
    }
}

fn message(status: StatusCode, said: &str) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        said.to_owned(),
    )
        .into_response()
}
