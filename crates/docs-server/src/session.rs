//! Signing in, signing out, and the check every write goes through.
//!
//! # The order of the checks is the security property
//!
//! A bound that runs after the expensive part has bounded nothing. So a sign-in
//! goes: read the header, ask the throttle, *then* touch the store, *then* spend
//! an Argon2 verification. A caller who is guessing is turned away before they
//! have cost us anything worth having.
//!
//! # Why an unknown name costs the same as a wrong password
//!
//! Because otherwise the clock answers a question the API refuses to: a refusal
//! that comes back faster for a name nobody has lets a stranger read the list of
//! names out of the timings. When there is no such account the same work is
//! spent against a hash of a password nobody has, and the wording of the refusal
//! is identical.

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use docs_store::Store;

use crate::routes::{json, message, refused};
use crate::{Site, auth, identity};

/// What a successful sign-in hands back.
#[derive(serde::Serialize)]
struct Issued {
    /// The token, and the only time it is ever transmitted.
    token: String,
    /// When it stops working, in seconds since the epoch.
    expires: i64,
}

/// `POST /api/session` — a password in, a day-long token out.
pub async fn issue(State(site): State<Site>, headers: HeaderMap) -> Response {
    let header = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok());
    let Some(caller) = auth::caller(header) else {
        return challenge_basic();
    };

    // Before the store and before the hash, or it bounds nothing.
    if !site.throttle().permit(&caller.name) {
        log::warn!("sign-in refused: too many recent failures for that name");
        return waiting();
    }

    let mut store = match site.writer().await {
        Ok(store) => store,
        Err(fault) => return refused(&fault),
    };
    let found = match store.account(&caller.name).await {
        Ok(found) => found,
        Err(fault) => return refused(&fault),
    };

    let Some(account) = found else {
        // No such name. Spend the same work anyway, so the clock says nothing.
        identity::verifies_nothing(&caller.password).await;
        site.throttle().failed(&caller.name);
        log::warn!("sign-in refused");
        return refusal();
    };
    if !identity::verifies(&caller.password, &account.secret).await {
        site.throttle().failed(&caller.name);
        log::warn!("sign-in refused");
        return refusal();
    }
    site.throttle().succeeded(&caller.name);

    let issued_at = identity::now();
    // The only moment this table grows is the only moment it is worth tidying.
    if let Err(fault) = store.purge_tokens(issued_at).await {
        log::warn!("could not purge expired tokens: {fault}");
    }
    let expires =
        issued_at.saturating_add(i64::try_from(identity::LIFETIME.as_secs()).unwrap_or(i64::MAX));
    let secret = identity::mint();
    if let Err(fault) = store
        .put_token(&identity::digest(secret.reveal()), &account.name, expires)
        .await
    {
        return refused(&fault);
    }
    log::info!("signed in as {}", account.name);
    json(
        StatusCode::OK,
        &Issued {
            token: secret.reveal().to_owned(),
            expires,
        },
    )
}

/// `DELETE /api/session` — forget the token that was presented.
///
/// Answers the same way whether the token was real, so this is not a way to ask
/// whether one is.
pub async fn revoke(State(site): State<Site>, headers: HeaderMap) -> Response {
    let header = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok());
    let Some(presented) = auth::presented(header) else {
        return challenge_bearer();
    };
    let mut store = match site.writer().await {
        Ok(store) => store,
        Err(fault) => return refused(&fault),
    };
    match store
        .delete_token(&identity::digest(presented.reveal()))
        .await
    {
        Ok(()) | Err(docs_store::Fault::UnsafeName) => StatusCode::NO_CONTENT.into_response(),
        Err(fault) => refused(&fault),
    }
}

/// The connection a write route runs on, or the response that says why not.
///
/// Returns an **editor** connection, and only after a token has been verified.
/// The identity in the request and the identity on the connection are two
/// different things on purpose: this function answers "who is asking", and the
/// store still answers "what may travel here".
///
/// # Errors
///
/// The response to send: 401 when no usable token was presented, or whatever
/// the store said when it could not be reached.
pub async fn authorized(site: &Site, headers: &HeaderMap) -> Result<Store, Box<Response>> {
    let header = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok());
    let Some(presented) = auth::presented(header) else {
        return Err(Box::new(challenge_bearer()));
    };
    let mut store = site
        .writer()
        .await
        .map_err(|fault| Box::new(refused(&fault)))?;
    let digest = identity::digest(presented.reveal());
    let found = store
        .token(&digest)
        .await
        .map_err(|fault| Box::new(refused(&fault)))?;
    let Some(token) = found else {
        return Err(Box::new(challenge_bearer()));
    };
    if token.expires <= identity::now() {
        // Expired, so it is worth nothing and worth keeping less. Best effort:
        // failing to tidy is not a reason to let it through.
        let _ = store.delete_token(&digest).await;
        return Err(Box::new(challenge_bearer()));
    }
    Ok(store)
}

/// 401 for a route that wants a password.
fn challenge_basic() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [(header::WWW_AUTHENTICATE, "Basic realm=\"docs\"")],
        "credentials required",
    )
        .into_response()
}

/// 401 for a route that wants a token.
fn challenge_bearer() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [(header::WWW_AUTHENTICATE, "Bearer realm=\"docs\"")],
        "a valid token is required",
    )
        .into_response()
}

/// The one wording a failed sign-in ever gets.
///
/// Identical for a name nobody has and for a password that is wrong, because two
/// wordings are a way to ask which accounts exist.
fn refusal() -> Response {
    message(StatusCode::UNAUTHORIZED, "those are not valid credentials")
}

/// 429 for a caller the throttle is holding.
fn waiting() -> Response {
    message(
        StatusCode::TOO_MANY_REQUESTS,
        "too many recent attempts; try again shortly",
    )
}
