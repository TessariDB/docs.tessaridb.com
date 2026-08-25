//! Who may edit this site — which is not the same question as who may reach the
//! database.
//!
//! The store's own users are two service accounts: a `viewer` the read routes
//! run as and an `editor` the write routes run as. Neither is a person. A person
//! is a row in `account` here, and holds a token rather than a database
//! password.
//!
//! # What this table is worth to somebody who steals it
//!
//! As little as it can be. `account.secret` is an Argon2id PHC string, so a
//! password is not recoverable from it in any useful time. `token`'s record id
//! is the **SHA-256 of the token that was issued**, never the token, so a dump
//! of this table holds nothing that can be presented to anything — it is a list
//! of expiry times and account names.
//!
//! # Nothing here decides anything either
//!
//! Same rule as the rest of this crate: these are reads and writes. Whether a
//! password matches, whether a token has expired and whether a caller has
//! guessed too often are decisions, and they live in `docs-server`.

use crate::{Fault, Store, integer, number_of, records, text, text_of};

/// A person who may edit the site.
///
/// `Debug` is written by hand further down. The secret is a hash rather than a
/// password, but a type holding a credential and answering `{:?}` with it is
/// how credentials reach log lines, and the hash is the thing an offline attack
/// is run against.
#[derive(Clone, PartialEq, Eq)]
pub struct Account {
    /// The name, as it is typed at the API.
    pub name: String,
    /// An Argon2id PHC string.
    pub secret: String,
}

impl std::fmt::Debug for Account {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Account")
            .field("name", &self.name)
            .field("secret", &"<hash>")
            .finish()
    }
}

/// An issued token, as the store remembers it.
///
/// There is no field holding the token: it is the record's id, hashed. This type
/// is what is *left* of a token once it has been handed to somebody.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    /// The account it was issued to.
    pub account: String,
    /// When it stops working, in seconds since the epoch.
    pub expires: i64,
}

/// Whether a name can be spelled into a record id without escaping.
///
/// An allowlist rather than a denylist, so a character nobody thought about is
/// refused rather than admitted. `'` and `\` are the two that would matter and
/// neither is on the list, but the point of the shape is that the next one is
/// not on it either.
#[must_use]
pub fn is_safe_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.' | '@')
        })
}

/// Whether a string is the hex digest this module writes ids from.
///
/// Checked even though every caller passes the output of our own hash: a value
/// derived from something a stranger sent is still a value from a stranger.
#[must_use]
fn is_digest(digest: &str) -> bool {
    digest.len() == 64
        && digest
            .chars()
            .all(|character| character.is_ascii_hexdigit())
}

impl Store {
    /// Writes an account, replacing whatever stood under that name.
    ///
    /// `secret` is stored exactly as given; producing it is `docs-server`'s job.
    ///
    /// # Errors
    ///
    /// [`Fault::UnsafeName`] for a name that cannot be an id, [`Fault::Client`]
    /// when the node refuses — which is how a connection without permission to
    /// write finds out.
    pub async fn put_account(&mut self, name: &str, secret: &str) -> Result<(), Fault> {
        if !is_safe_name(name) {
            return Err(Fault::UnsafeName);
        }
        self.run_with(
            &format!(
                "DELETE account:'{name}';\n\
                 CREATE account:'{name}' = {{ name: $name, secret: $secret }};"
            ),
            vec![
                ("name".to_owned(), text(name)),
                ("secret".to_owned(), text(secret)),
            ],
        )
        .await?;
        Ok(())
    }

    /// Reads one account, or `None` when there is no such name.
    ///
    /// # Errors
    ///
    /// [`Fault::UnsafeName`] or [`Fault::Client`].
    pub async fn account(&mut self, name: &str) -> Result<Option<Account>, Fault> {
        if !is_safe_name(name) {
            return Err(Fault::UnsafeName);
        }
        let answers = self
            .run(&format!("SELECT * FROM account:'{name}';"))
            .await?;
        let found = records(answers.first())?;
        Ok(found.first().map(|(_, value)| Account {
            name: text_of(value, "name"),
            secret: text_of(value, "secret"),
        }))
    }

    /// Removes an account and every token issued to it.
    ///
    /// The tokens go too, and in that order matters less than that it happens at
    /// all: an account nobody can sign in to whose tokens still work is an
    /// account that has not been removed.
    ///
    /// # Errors
    ///
    /// [`Fault::UnsafeName`] or [`Fault::Client`].
    pub async fn delete_account(&mut self, name: &str) -> Result<(), Fault> {
        if !is_safe_name(name) {
            return Err(Fault::UnsafeName);
        }
        self.run_with(
            &format!("DELETE FROM token WHERE account = $name;\nDELETE account:'{name}';"),
            vec![("name".to_owned(), text(name))],
        )
        .await?;
        Ok(())
    }

    /// Records an issued token under the SHA-256 of the token itself.
    ///
    /// # Errors
    ///
    /// [`Fault::UnsafeName`] when the digest is not one, or the name is not one;
    /// [`Fault::Client`] when the node refuses.
    pub async fn put_token(
        &mut self,
        digest: &str,
        account: &str,
        expires: i64,
    ) -> Result<(), Fault> {
        if !is_digest(digest) || !is_safe_name(account) {
            return Err(Fault::UnsafeName);
        }
        self.run_with(
            &format!("CREATE token:'{digest}' = {{ account: $account, expires: $expires }};"),
            vec![
                ("account".to_owned(), text(account)),
                ("expires".to_owned(), integer(expires)),
            ],
        )
        .await?;
        Ok(())
    }

    /// Reads a token by its digest, or `None` when there is no such record.
    ///
    /// Answers nothing about expiry — that is a decision, and the caller makes
    /// it. A lookup by record id, so this costs one key read and not a scan.
    ///
    /// # Errors
    ///
    /// [`Fault::UnsafeName`] for a digest that is not one, or [`Fault::Client`].
    pub async fn token(&mut self, digest: &str) -> Result<Option<Token>, Fault> {
        if !is_digest(digest) {
            return Err(Fault::UnsafeName);
        }
        let answers = self
            .run(&format!("SELECT * FROM token:'{digest}';"))
            .await?;
        let found = records(answers.first())?;
        Ok(found.first().map(|(_, value)| Token {
            account: text_of(value, "account"),
            expires: number_of(value, "expires"),
        }))
    }

    /// Forgets one token.
    ///
    /// # Errors
    ///
    /// [`Fault::UnsafeName`] or [`Fault::Client`].
    pub async fn delete_token(&mut self, digest: &str) -> Result<(), Fault> {
        if !is_digest(digest) {
            return Err(Fault::UnsafeName);
        }
        self.run(&format!("DELETE token:'{digest}';")).await?;
        Ok(())
    }

    /// Forgets every token that stopped working before `moment`.
    ///
    /// Run when a token is issued, which is the only moment this table grows.
    /// There is no scheduler here and a table that only ever grows is a table
    /// that eventually matters.
    ///
    /// # Errors
    ///
    /// [`Fault::Client`] when the node refuses.
    pub async fn purge_tokens(&mut self, moment: i64) -> Result<(), Fault> {
        self.run_with(
            "DELETE FROM token WHERE expires < $moment;",
            vec![("moment".to_owned(), integer(moment))],
        )
        .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{Account, is_digest, is_safe_name};

    #[test]
    fn a_name_that_could_break_out_of_a_record_id_is_refused() {
        assert!(!is_safe_name("ann'; DELETE FROM account WHERE true; --"));
        assert!(!is_safe_name("ann'"));
        assert!(!is_safe_name("ann\\"));
        assert!(!is_safe_name(""));
        assert!(!is_safe_name(&"a".repeat(65)));
    }

    #[test]
    fn an_ordinary_name_is_allowed() {
        assert!(is_safe_name("ann"));
        assert!(is_safe_name("ann.smith"));
        assert!(is_safe_name("ann@example.com"));
        assert!(is_safe_name("ann-smith_2"));
    }

    #[test]
    fn a_digest_is_sixty_four_hex_characters_and_nothing_else() {
        assert!(is_digest(&"a".repeat(64)));
        assert!(is_digest(&"0123456789abcdef".repeat(4)));
        assert!(!is_digest(&"a".repeat(63)));
        assert!(!is_digest(&"a".repeat(65)));
        assert!(!is_digest(&"g".repeat(64)));
        assert!(!is_digest("'; DELETE FROM token WHERE true; --"));
    }

    #[test]
    fn an_account_does_not_print_its_secret() {
        let account = Account {
            name: "ann".to_owned(),
            secret: "$argon2id$v=19$m=19456,t=2,p=1$c2FsdA$aGFzaA".to_owned(),
        };
        let printed = format!("{account:?}");
        assert!(printed.contains("ann"));
        assert!(!printed.contains("argon2id"));
        assert!(printed.contains("<hash>"));
    }
}
