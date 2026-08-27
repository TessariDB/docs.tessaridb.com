//! The site's content, in TessariDB.
//!
//! Everything here goes over the wire through `tessaridb-client`. There is no
//! path to the engine's own crates and there is not meant to be: this site is a
//! demonstration of what an ordinary client can do, and a privileged one would
//! demonstrate nothing (LR-DOCS-003).
//!
//! The division against `docs-content` is that this crate holds no parsing
//! rules and that crate holds no connection. What can be got wrong here is
//! sequencing and statements; what can be got wrong there is meaning.

pub mod accounts;
pub mod ingest;
pub mod read;
pub mod schema;
pub mod write;

use tessaridb_client::{Answer, Client, Value};

/// What can go wrong between this site and its store.
#[derive(Debug, thiserror::Error)]
pub enum Fault {
    /// The client could not reach the node, or the node refused.
    #[error("store: {0}")]
    Client(#[from] tessaridb_client::Error),

    /// A slug that cannot be spelled into a record id safely.
    ///
    /// Reported rather than escaped: a slug arrives from a file path in this
    /// repository, so one that fails the check is a mistake to fix at the source
    /// and not an input to sanitise.
    #[error("unsafe slug, which should be impossible from a content path: {0}")]
    UnsafeSlug(String),

    /// An account name that cannot be spelled into a record id safely.
    ///
    /// Separate from [`Fault::UnsafeSlug`] because this one arrives from a
    /// request rather than from a file this repository controls, so it is an
    /// input to refuse and not a mistake to fix at the source. The name is not
    /// repeated back: a refusal that echoes what was tried is a refusal an
    /// attacker can read their own probe out of.
    #[error("that is not a usable account name")]
    UnsafeName,

    /// The node answered, but not with the shape this code reads.
    #[error("the node answered a {found} where {wanted} was expected")]
    Unexpected {
        /// What this code needed.
        wanted: &'static str,
        /// What arrived.
        found: &'static str,
    },
}

impl Fault {
    /// The store's own words when it **refused**, or `None` when the failure was
    /// the transport.
    ///
    /// The distinction is the one a caller acts on and the one most easily
    /// flattened: a refusal is about this caller and this statement, and a
    /// transport failure is about the network. Answering both the same way sends
    /// somebody to the wrong place. Exposed here so callers need not depend on
    /// the client crate to tell them apart.
    #[must_use]
    pub fn refusal(&self) -> Option<&str> {
        match self {
            Self::Client(tessaridb_client::Error::Refused { message }) => Some(message),
            _ => None,
        }
    }
}

/// A connection to the store, already pointed at one version's namespace.
pub struct Store {
    client: Client,
    credentials: Option<(String, String)>,
    namespace: String,
}

impl Store {
    /// Opens a connection and selects the namespace for `namespace`.
    ///
    /// The address is a bare `host:port`; there is no scheme, because the
    /// protocol is not carried over HTTP.
    ///
    /// # Errors
    ///
    /// Returns [`Fault::Client`] when the node is unreachable or refuses.
    pub async fn connect(
        address: &str,
        namespace: &str,
        credentials: Option<(String, String)>,
    ) -> Result<Self, Fault> {
        let client = Client::connect(address).await?;
        let mut store = Self {
            client,
            credentials,
            namespace: namespace.to_owned(),
        };
        let script = schema::use_namespace(&store.namespace);
        store.run(&script).await?;
        Ok(store)
    }

    /// Applies the schema. Safe to call on every start.
    ///
    /// # Why the namespace is asked about rather than declared
    ///
    /// Declaring a namespace needs authority over the **store**, because a
    /// namespace is a sibling of every other one — and the account this server
    /// runs as is deliberately narrower, scoped to a single database. Issuing
    /// `DEFINE NAMESPACE IF NOT EXISTS` unconditionally would therefore ask for
    /// authority this server should not hold, and be refused on every start
    /// once the store is closed, in the ordinary case where the namespace has
    /// existed since the deployment was set up.
    ///
    /// So it is looked for first, and declared only when genuinely absent —
    /// where the refusal, if it comes, names a real misconfiguration rather
    /// than an over-broad request.
    ///
    /// # Errors
    ///
    /// Returns [`Fault::Client`] when a definition is refused.
    pub async fn migrate(&mut self) -> Result<(), Fault> {
        if !self.has_namespace().await? {
            log::info!(
                "namespace {} is not there yet — declaring it",
                self.namespace
            );
            let script = schema::define_namespace(&self.namespace);
            self.run(&script).await?;
        }
        let script = schema::statements(&self.namespace);
        self.run(&script).await?;
        log::info!("schema applied in namespace {}", self.namespace);
        Ok(())
    }

    /// Whether the store already holds this store's namespace.
    ///
    /// `INFO FOR STORE` is a **read**, and it reports only what the caller could
    /// have found out anyway — so asking costs no authority beyond what this
    /// server already has.
    async fn has_namespace(&mut self) -> Result<bool, Fault> {
        let answers = self.run("INFO FOR STORE;").await?;
        // `INFO FOR STORE` answers one value — an object with a `namespaces`
        // array — rather than a record set, so `records` is the wrong reader.
        // Matched on the shape rather than searched for in a rendering: a
        // namespace called `docs` must not be found because some other field
        // happens to contain the word.
        let Some(Answer::Value { value, .. }) = answers.last() else {
            return Err(Fault::Unexpected {
                wanted: "the store's namespaces",
                found: "something else",
            });
        };
        let Value::Object(fields) = value else {
            return Err(Fault::Unexpected {
                wanted: "an object",
                found: "another kind of value",
            });
        };
        let Some(Value::Array(namespaces)) = fields.get("namespaces") else {
            return Err(Fault::Unexpected {
                wanted: "a `namespaces` array",
                found: "an object without one",
            });
        };
        Ok(namespaces
            .iter()
            .any(|held| matches!(held, Value::String(name) if *name == self.namespace)))
    }

    /// Runs a script with no bound parameters.
    ///
    /// # Errors
    ///
    /// Returns [`Fault::Client`] when the node refuses.
    pub async fn run(&mut self, script: &str) -> Result<Vec<Answer>, Fault> {
        let credentials = self
            .credentials
            .as_ref()
            .map(|(name, password)| (name.as_str(), password.as_str()));
        Ok(self.client.run(script, credentials).await?)
    }

    /// Runs a script whose `$parameters` take the given values.
    ///
    /// Everything a reader supplies travels this way. A search term is a bound
    /// value and never syntax, which is the whole reason the search route is
    /// allowed to take one from a query string.
    ///
    /// # Errors
    ///
    /// Returns [`Fault::Client`] when the node refuses.
    pub async fn run_with(
        &mut self,
        script: &str,
        parameters: Vec<(String, Value)>,
    ) -> Result<Vec<Answer>, Fault> {
        let credentials = self
            .credentials
            .as_ref()
            .map(|(name, password)| (name.as_str(), password.as_str()));
        Ok(self
            .client
            .run_with(script, credentials, parameters)
            .await?)
    }

    /// Which version's namespace this connection is pointed at.
    #[must_use]
    pub fn namespace(&self) -> &str {
        &self.namespace
    }
}

/// The records of an answer, or a fault naming what arrived instead.
///
/// # Errors
///
/// Returns [`Fault::Unexpected`] when the answer is not a record set.
pub fn records(answer: Option<&Answer>) -> Result<&[(String, Value)], Fault> {
    match answer {
        Some(Answer::Records { records, .. }) => Ok(records),
        Some(Answer::Done) => Err(Fault::Unexpected {
            wanted: "records",
            found: "an acknowledgement",
        }),
        Some(Answer::Value { .. }) => Err(Fault::Unexpected {
            wanted: "records",
            found: "a single value",
        }),
        _ => Err(Fault::Unexpected {
            wanted: "records",
            found: "something else",
        }),
    }
}

/// How the node said it found the records of an answer.
///
/// Reported by the node, not requested. Used by the search tests to hold the
/// claim that ranking runs off the index — a search that quietly degrades to a
/// scan answers the same rows and is the regression nothing else would catch.
#[must_use]
pub fn access_path(answer: Option<&Answer>) -> Option<&str> {
    match answer {
        Some(Answer::Records { path, .. }) => Some(path.as_str()),
        _ => None,
    }
}

/// A bound string value.
pub(crate) fn text(value: &str) -> Value {
    Value::String(value.to_owned())
}

/// A bound integer value.
pub(crate) fn integer(value: i64) -> Value {
    Value::Number(tessaridb_client::Number::Integer(value))
}

/// A bound boolean value.
pub(crate) fn boolean(value: bool) -> Value {
    Value::Bool(value)
}

/// An absent optional binds as `Null` and not as the empty string, because the
/// store distinguishes them and a reader of the data should be able to as well.
pub(crate) fn optional(value: Option<&str>) -> Value {
    match value {
        Some(found) => text(found),
        None => Value::Null,
    }
}

/// A string field of a record, or `""` when it is absent.
#[must_use]
pub fn text_of(value: &Value, field: &str) -> String {
    match value {
        Value::Object(fields) => match fields.get(field) {
            Some(Value::String(text)) => text.clone(),
            _ => String::new(),
        },
        _ => String::new(),
    }
}

/// An integer field of a record, or `0` when it is absent.
#[must_use]
pub fn number_of(value: &Value, field: &str) -> i64 {
    match value {
        Value::Object(fields) => match fields.get(field) {
            Some(Value::Number(tessaridb_client::Number::Integer(found))) => *found,
            _ => 0,
        },
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use tessaridb_client::{Number, Value};

    use super::{number_of, text_of};

    fn record() -> Value {
        let mut fields = BTreeMap::new();
        fields.insert("title".to_owned(), Value::String("Graphs".to_owned()));
        fields.insert("order".to_owned(), Value::Number(Number::Integer(60)));
        fields.insert("summary".to_owned(), Value::Null);
        Value::Object(fields)
    }

    #[test]
    fn a_present_field_comes_back_as_written() {
        assert_eq!(text_of(&record(), "title"), "Graphs");
        assert_eq!(number_of(&record(), "order"), 60);
    }

    #[test]
    fn an_absent_or_empty_field_is_a_default_and_not_a_panic() {
        // These run on every rendered page, so a page missing an optional field
        // must render without it rather than take the site down.
        assert_eq!(text_of(&record(), "icon"), "");
        assert_eq!(text_of(&record(), "summary"), "");
        assert_eq!(number_of(&record(), "missing"), 0);
        assert_eq!(text_of(&Value::Null, "title"), "");
    }
}
