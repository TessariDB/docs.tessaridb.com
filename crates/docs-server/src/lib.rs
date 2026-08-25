//! The site's HTTP surface.
//!
//! Three reads anyone may make, and writes for whoever the store lets write.
//! The server holds no content and caches none: every route is a statement
//! against TessariDB, which is the point of the site.
//!
//! # Two accounts, not one and not none
//!
//! "Anyone may read" is a fact about **this API**, not about the store. A store
//! with users in it has no anonymous access at all, so the public read routes
//! run as a `viewer` account this process holds, and the write routes run as
//! whoever the caller says they are. Both refusals then come from the store.
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

/// What every handler needs: where the node is, which version to read, and the
/// account the public reads run as.
#[derive(Clone, Debug)]
pub struct Site {
    node: Arc<str>,
    namespace: Arc<str>,
    reading_as: Option<Arc<(String, String)>>,
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
        }
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
