//! The site's HTTP surface.
//!
//! Three reads anyone may make, and writes for whoever the store lets write.
//! The server holds no content and caches none: every route is a statement
//! against TessariDB, which is the point of the site.
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
pub mod routes;

use std::sync::Arc;

use docs_store::{Fault, Store};

pub use crate::auth::Caller;

/// What every handler needs: where the node is, and which version to read.
#[derive(Clone, Debug)]
pub struct Site {
    node: Arc<str>,
    namespace: Arc<str>,
}

impl Site {
    /// A site served from the node at `node`, out of `namespace`.
    #[must_use]
    pub fn new(node: &str, namespace: &str) -> Self {
        Self {
            node: Arc::from(node),
            namespace: Arc::from(namespace),
        }
    }

    /// A connection with **no identity**, for the read routes.
    ///
    /// Anonymous by construction rather than by convention: the read path holds
    /// no credentials, so a mistake in routing cannot turn a reader into a
    /// writer — there is nothing there to escalate.
    ///
    /// # Errors
    ///
    /// Returns [`Fault::Client`] when the node is unreachable.
    pub async fn reader(&self) -> Result<Store, Fault> {
        Store::connect(&self.node, &self.namespace, None).await
    }

    /// A connection carrying the caller's credentials, for the write routes.
    ///
    /// The store decides whether they may write. This function does not look at
    /// the name it is given.
    ///
    /// # Errors
    ///
    /// Returns [`Fault::Client`] when the node is unreachable **or** when it
    /// refuses the credentials — which is the same answer to this code and a
    /// different one to the caller, sorted out in `routes`.
    pub async fn writer(&self, caller: &Caller) -> Result<Store, Fault> {
        Store::connect(
            &self.node,
            &self.namespace,
            Some((caller.name.clone(), caller.password.clone())),
        )
        .await
    }
}
