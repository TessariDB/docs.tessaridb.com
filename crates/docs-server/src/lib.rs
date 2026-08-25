//! The site's HTTP surface.
//!
//! Three reads anyone may make, and writes for whoever the store lets write.
//! The server holds no content and caches none: every route is a statement
//! against TessariDB, which is the point of the site.
//!
//! # Two questions, answered in two different places
//!
//! **Who is asking** is this API's question, and it is answered against the
//! `account` and `token` tables: a person signs in with a password and is given
//! a token that works for a day. A person is not a database user.
//!
//! **What may travel on this connection** stays the store's question. The store
//! holds two service accounts and neither is a person: the read routes run as a
//! `viewer` and the write routes as an `editor`. So a routing mistake that sent
//! a write down the read path would still be refused — by the database, with
//! "a viewer may not write", whatever this server believed it was doing.
//!
//! Keeping the second half is the whole reason the first half is safe to add.
//! An API that authenticated its own users *and* held one all-powerful database
//! connection would have exactly one thing standing between a reader and a
//! write, and it would be this code.
//!
//! # A connection per request
//!
//! Each request opens its own connection to the node. That is deliberate for
//! now: a connection **is** a session in this protocol — `USE NAMESPACE` stays in
//! force on it, and so do credentials — so a pool shared between an anonymous
//! reader and a signed-in editor would be a pool where identity leaks between
//! requests. Pooling per identity is the eventual answer; it is not worth its
//! complexity before there is a measurement saying so.

pub mod auth;
pub mod identity;
pub mod routes;
pub mod session;

use std::sync::Arc;

use docs_store::{Fault, Store};

pub use crate::auth::{Caller, Presented};
use crate::identity::Throttle;

/// What every handler needs: where the node is, which version to read, the two
/// service accounts, and the bound on password guessing.
///
/// The two accounts are set by two different calls rather than by two arguments
/// of the same type, because a pair of `Option<(String, String)>` parameters is
/// a pair that can be swapped — and swapping these two would hand the public
/// read path an editor's connection while compiling perfectly.
#[derive(Clone, Debug)]
pub struct Site {
    node: Arc<str>,
    namespace: Arc<str>,
    reading_as: Option<Arc<(String, String)>>,
    writing_as: Option<Arc<(String, String)>>,
    throttle: Arc<Throttle>,
}

impl Site {
    /// A site served from the node at `node`, out of `namespace`.
    ///
    /// `reading_as` is the account the **public** read routes use. It should be
    /// a `viewer`, and it is `None` only against a store that has no users at
    /// all — see [`Site::reader`] for why it exists.
    #[must_use]
    pub fn new(node: &str, namespace: &str, reading_as: Option<(String, String)>) -> Self {
        Self {
            node: Arc::from(node),
            namespace: Arc::from(namespace),
            reading_as: reading_as.map(Arc::new),
            writing_as: None,
            throttle: Arc::new(Throttle::new()),
        }
    }

    /// The account the write routes run as. An `editor`.
    ///
    /// Set separately and not in [`Site::new`], for the reason on the struct.
    /// Left unset, every write route refuses — which is the right way round: a
    /// misconfigured deployment serves the site read-only rather than serving it
    /// writable to nobody in particular.
    #[must_use]
    pub fn writing_as(mut self, credentials: (String, String)) -> Self {
        self.writing_as = Some(Arc::new(credentials));
        self
    }

    /// The bound on how fast a password can be guessed. One per site, shared by
    /// every request, which is the only way it bounds anything.
    #[must_use]
    pub fn throttle(&self) -> &Throttle {
        &self.throttle
    }

    /// A connection for the read routes.
    ///
    /// # Why this is not an anonymous connection
    ///
    /// It was, and that was wrong, and the store said so the first time one was
    /// tried against a store with users in it. **A store with no user is open;
    /// declaring the first user closes it**, and a closed store refuses an
    /// anonymous session outright — every read, not merely every write. So the
    /// moment this site has an editor, an anonymous read path serves nothing.
    ///
    /// The account here is therefore a `viewer`, and the guarantee that a
    /// routing mistake cannot turn a reader into a writer is *stronger* than the
    /// one it replaces: it was a property of this code holding no credentials,
    /// and it is now a property the **store enforces** — a viewer that attempts
    /// a write is refused with "a viewer may not write", whatever this server
    /// believes it is doing.
    ///
    /// `None` means the store has no users and is open, which is a development
    /// store and not a deployed one.
    ///
    /// # Errors
    ///
    /// Returns [`Fault::Client`] when the node is unreachable or refuses the
    /// reading account.
    pub async fn reader(&self) -> Result<Store, Fault> {
        let credentials = self
            .reading_as
            .as_ref()
            .map(|pair| (pair.0.clone(), pair.1.clone()));
        Store::connect(&self.node, &self.namespace, credentials).await
    }

    /// A connection for the write routes, as the `editor` service account.
    ///
    /// It does not carry the caller's identity, because the caller no longer has
    /// one the database knows about. Reaching this function at all is the proof
    /// that a token was presented and verified; what it adds on top is the
    /// store's own refusal for anything an `editor` may not do.
    ///
    /// # Errors
    ///
    /// [`Fault::Client`] when the node is unreachable or refuses the account.
    /// `None` credentials means no writing account was configured, and the
    /// connection is opened anonymously — which a closed store refuses, so the
    /// deployment fails closed rather than open.
    pub async fn writer(&self) -> Result<Store, Fault> {
        let credentials = self
            .writing_as
            .as_ref()
            .map(|pair| (pair.0.clone(), pair.1.clone()));
        Store::connect(&self.node, &self.namespace, credentials).await
    }
}
