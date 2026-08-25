//! Who is asking, taken from the request and handed to the store unexamined.
//!
//! # This module does not decide anything
//!
//! It reads a name and a password out of an `Authorization` header and stops.
//! Whether that name may write is settled by the store, which already has users
//! and roles, and which will refuse a statement the caller's role does not
//! permit. A second authority here — a table of who-may-do-what, a role check
//! before the call — would be a second authority to keep in step by hand, and
//! the day the two disagree is the day somebody edits a page they should not.
//!
//! So the only failure this module reports is *no credentials at all*, and even
//! that is a convenience: without it the store would refuse anyway, one round
//! trip later, with a message about the store rather than about the request.
//!
//! # Basic, and what that costs
//!
//! Basic rather than a bespoke token, because the mapping onto `DEFINE USER` is
//! exact and a token would be a second secret to mint, store, expire and revoke
//! — cost without a benefit for an admin surface with a handful of editors.
//!
//! **There is no TLS on the store's wire protocol and Basic sends the password
//! as given.** This API belongs behind something that terminates TLS. It is said
//! here, in the deployment, and on the site's own operations page, rather than
//! left to be discovered.

/// A name and a password, on their way to be checked.
///
/// `Debug` is written by hand. Nothing in this crate prints a `Caller`, and that
/// is exactly the state in which a derived `Debug` is a loaded gun: the day
/// somebody adds a trace line to a handler, the password goes to the log with
/// it. The client SDK writes its own for the same reason on the same kind of
/// type.
#[derive(Clone, PartialEq, Eq)]
pub struct Caller {
    /// The user name, as it is typed at the API.
    pub name: String,
    /// The password, as given.
    pub password: String,
}

impl std::fmt::Debug for Caller {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Caller")
            .field("name", &self.name)
            .field("password", &"<redacted>")
            .finish()
    }
}

/// A bearer token as it arrived, before anything has been decided about it.
///
/// Same reason for the hand-written `Debug`: this one *is* the credential.
#[derive(Clone, PartialEq, Eq)]
pub struct Presented(String);

impl Presented {
    /// The token as presented.
    #[must_use]
    pub fn reveal(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Presented {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Presented(<redacted>)")
    }
}

/// Reads `Authorization: Bearer …`.
///
/// Returns `None` when the header is absent, is not Bearer, or carries nothing
/// after the scheme — all of which are the same thing to a caller: they did not
/// present a token.
///
/// The value is not validated here beyond being non-empty. What a token *is* is
/// `identity`'s business, and what it grants is the store's.
#[must_use]
pub fn presented(header: Option<&str>) -> Option<Presented> {
    let value = header?;
    let (scheme, token) = value.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }
    let token = token.trim();
    if token.is_empty() {
        return None;
    }
    Some(Presented(token.to_owned()))
}

/// Reads `Authorization: Basic …`.
///
/// Returns `None` when the header is absent, is not Basic, is not valid base64,
/// or does not hold a `name:password` pair — all of which are the same thing to
/// a caller: they did not present credentials.
#[must_use]
pub fn caller(header: Option<&str>) -> Option<Caller> {
    let value = header?;
    // The scheme is matched case-insensitively because the specification says
    // it is case-insensitive, and clients differ.
    let (scheme, encoded) = value.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("basic") {
        return None;
    }
    let decoded = base64(encoded.trim())?;
    let text = String::from_utf8(decoded).ok()?;
    // Split at the *first* colon: a password may contain one, a user name may
    // not. Splitting at the last would quietly move part of the password into
    // the name and produce a refusal nobody could explain.
    let (name, password) = text.split_once(':')?;
    if name.is_empty() {
        return None;
    }
    Some(Caller {
        name: name.to_owned(),
        password: password.to_owned(),
    })
}

/// Standard base64 with padding, decoded.
///
/// Written out rather than taken as a dependency: it is twenty lines, it is the
/// only base64 this crate will ever do, and a decoder that is wrong is wrong in
/// a way the tests below catch.
fn base64(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len().saturating_mul(3).saturating_div(4));
    let mut held: u32 = 0;
    let mut bits: u32 = 0;
    for character in text.bytes() {
        let value = match character {
            b'A'..=b'Z' => u32::from(character.saturating_sub(b'A')),
            b'a'..=b'z' => u32::from(character.saturating_sub(b'a')).saturating_add(26),
            b'0'..=b'9' => u32::from(character.saturating_sub(b'0')).saturating_add(52),
            b'+' => 62,
            b'/' => 63,
            // Padding ends the meaningful input; anything else is not base64.
            b'=' => break,
            _ => return None,
        };
        held = held.checked_shl(6)?.checked_add(value)?;
        bits = bits.saturating_add(6);
        if bits >= 8 {
            bits = bits.saturating_sub(8);
            let byte = held.checked_shr(bits)? & 0xFF;
            out.push(u8::try_from(byte).ok()?);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::{Caller, base64, caller};

    #[test]
    fn a_name_and_password_come_back_as_given() {
        // `ada:correct horse` — a password with a space, which is legal.
        let header = "Basic YWRhOmNvcnJlY3QgaG9yc2U=";
        assert_eq!(
            caller(Some(header)),
            Some(Caller {
                name: "ada".to_owned(),
                password: "correct horse".to_owned(),
            })
        );
    }

    #[test]
    fn a_password_holding_a_colon_survives_intact() {
        // `ada:a:b` — split at the first colon, not the last. Splitting at the
        // last would move `a:` into the name and produce a refusal that looks
        // like a wrong password.
        let header = "Basic YWRhOmE6Yg==";
        let found = caller(Some(header)).expect("credentials");
        assert_eq!(found.name, "ada");
        assert_eq!(found.password, "a:b");
    }

    #[test]
    fn the_scheme_is_matched_however_the_client_spelled_it() {
        assert!(caller(Some("basic YWRhOng=")).is_some());
        assert!(caller(Some("BASIC YWRhOng=")).is_some());
        assert!(caller(Some("Bearer YWRhOng=")).is_none());
    }

    #[test]
    fn anything_that_is_not_a_pair_is_simply_no_credentials() {
        // Every one of these means the same thing to the caller, so they answer
        // the same way rather than leaking which part was wrong.
        assert_eq!(caller(None), None);
        assert_eq!(caller(Some("Basic")), None, "no space, no value");
        assert_eq!(caller(Some("Basic !!!!")), None, "not base64");
        assert_eq!(caller(Some("Basic bm9jb2xvbg==")), None, "no colon");
        assert_eq!(caller(Some("Basic OnBhc3N3b3Jk")), None, "empty name");
    }

    #[test]
    fn the_decoder_handles_every_padding_length() {
        // The three cases that differ, plus the empty one. A decoder that gets
        // padding wrong drops the last byte of a password and produces a
        // refusal nobody can explain.
        assert_eq!(base64("YQ==").as_deref(), Some(b"a".as_slice()));
        assert_eq!(base64("YWI=").as_deref(), Some(b"ab".as_slice()));
        assert_eq!(base64("YWJj").as_deref(), Some(b"abc".as_slice()));
        assert_eq!(base64("").as_deref(), Some(b"".as_slice()));
        assert_eq!(base64("YWJjZA==").as_deref(), Some(b"abcd".as_slice()));
    }

    #[test]
    fn a_character_that_is_not_base64_is_refused_rather_than_skipped() {
        // Skipping it would decode a password that was never sent.
        assert_eq!(base64("YWJ|j"), None);
        assert_eq!(base64("a b"), None);
    }
}
