//! Passwords, tokens, and the bounds that keep guessing at them expensive.
//!
//! Nothing here talks to the store and nothing here decides who may do what.
//! What lives here is the arithmetic that has to be right: hashing a password,
//! minting a token, reducing a token to the thing the store is allowed to
//! remember, and refusing a caller who is guessing.
//!
//! # Why each choice is the choice
//!
//! **Argon2id for passwords.** Memory-hard, so an attacker's graphics card
//! stops being worth so much more than our processor. The crate's defaults are
//! used deliberately rather than tuned: they track the OWASP guidance, and a
//! number invented here would be a number nobody revisits.
//!
//! **SHA-256 for tokens.** A slow hash buys nothing against an input with no
//! guessable structure — a token is 256 bits from the operating system, so
//! there is no dictionary to run. It would only be a cost paid on every single
//! request, which is the cost this whole change exists to remove.
//!
//! **The operating system's generator for the token.** Not a pseudo-random
//! number generator seeded from a clock, which is how tokens become predictable
//! in ways nobody notices until somebody predicts one.
//!
//! # Two bounds, and both are load-bearing
//!
//! Argon2 is deliberately slow and deliberately hungry: about 19 MiB per
//! verification. Two things follow, and neither is optional on a small machine.
//!
//! It must not run on the async runtime — a blocking call inside `async` stalls
//! every other request on that thread — so verification goes through
//! `spawn_blocking`. And the number running at once must be capped, or fifty
//! simultaneous sign-in attempts are a gigabyte of memory and a denial of
//! service that costs the attacker nothing.

use std::sync::{LazyLock, Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use argon2::Argon2;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use sha2::{Digest, Sha256};

/// How long an issued token works for.
///
/// A day. Long enough that an editor is not signing in between paragraphs,
/// short enough that a token found in somebody's shell history next week is
/// already useless.
pub const LIFETIME: Duration = Duration::from_secs(24 * 60 * 60);

/// A token, on its way to the one person who is ever allowed to see it.
///
/// `Debug` is written by hand. This is the one value in the system that is worth
/// stealing outright, and a type that answers `{:?}` with its own secret is how
/// such a value reaches a log line.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    /// The token as the caller must present it.
    ///
    /// Named for what it does rather than spelled `as_str`, so that reaching for
    /// it reads like the deliberate act it is.
    #[must_use]
    pub fn reveal(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Secret(<redacted>)")
    }
}

/// A fresh token: 256 bits from the operating system, written as hex.
///
/// Hex rather than base64 because it survives every transport without an
/// encoding question, and 64 characters is not a hardship for a machine.
#[must_use]
pub fn mint() -> Secret {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes).expect("the operating system's random generator");
    Secret(hex(&bytes))
}

/// What the store is allowed to remember about a token: its SHA-256, as hex.
///
/// The store never holds the token itself, so a copy of that table cannot be
/// presented to anything.
#[must_use]
pub fn digest(token: &str) -> String {
    hex(&Sha256::digest(token.as_bytes()))
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(
        String::with_capacity(bytes.len().saturating_mul(2)),
        |mut text, byte| {
            // Cannot fail: writing to a String.
            let _ = write!(text, "{byte:02x}");
            text
        },
    )
}

/// Hashes a password for storage. Argon2id, with a fresh salt.
///
/// # Errors
///
/// Returns the hasher's own message when it refuses, which in practice means a
/// parameter set this build cannot honour.
pub fn hash(password: &str) -> Result<String, String> {
    // The salt comes straight from the operating system rather than from the
    // hasher crate's own generator, so that the whole module has exactly one
    // source of randomness and it is the one the module note claims.
    let mut raw = [0_u8; 16];
    getrandom::fill(&mut raw).map_err(|fault| fault.to_string())?;
    let salt = SaltString::encode_b64(&raw).map_err(|fault| fault.to_string())?;
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hashed| hashed.to_string())
        .map_err(|fault| fault.to_string())
}

/// A hash of a password nobody has, computed once.
///
/// Verified against when the account does not exist, so that "no such name" and
/// "wrong password" take the same time. Without it, the time a refusal takes
/// says whether the name was real, and an attacker enumerates the account list
/// with a stopwatch.
static ABSENT: LazyLock<Option<String>> =
    LazyLock::new(|| hash("no account has this password").ok());

/// Whether `password` is the one behind `stored`.
///
/// Runs off the async runtime and behind the concurrency bound — see the module
/// note. `stored` is a PHC string; anything else is a refusal rather than a
/// panic, because a malformed row should not take the process down.
pub async fn verifies(password: &str, stored: &str) -> bool {
    let password = password.to_owned();
    let stored = stored.to_owned();
    let permit = verifying().await;
    let outcome = tokio::task::spawn_blocking(move || match PasswordHash::new(&stored) {
        Ok(parsed) => Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    })
    .await;
    drop(permit);
    outcome.unwrap_or(false)
}

/// Spends the same work as [`verifies`] and answers `false`.
///
/// Called when there is no such account. The work is what matters, not the
/// answer.
pub async fn verifies_nothing(password: &str) {
    let against = ABSENT.clone().unwrap_or_default();
    if against.is_empty() {
        // The fixed hash could not be built, which should not happen. Hashing
        // the presented password costs the same as verifying it would have, so
        // the timing property survives the unlikely case.
        let password = password.to_owned();
        let permit = verifying().await;
        let _ = tokio::task::spawn_blocking(move || hash(&password)).await;
        drop(permit);
        return;
    }
    let _ = verifies(password, &against).await;
}

/// The bound on how many verifications run at once.
///
/// Sized to the machine, because what it is really bounding is memory: each
/// verification wants about 19 MiB, and a small host has a small number of those
/// to spare. At least one, so a single-core machine still works.
static VERIFYING: LazyLock<tokio::sync::Semaphore> = LazyLock::new(|| {
    let places = std::thread::available_parallelism().map_or(2, std::num::NonZero::get);
    tokio::sync::Semaphore::new(places.max(1))
});

async fn verifying() -> tokio::sync::SemaphorePermit<'static> {
    VERIFYING
        .acquire()
        .await
        .expect("the verification semaphore is never closed")
}

/// Seconds since the epoch, as the store records them.
///
/// `try_from` rather than `as`: a cast that silently wraps a time is the kind of
/// arithmetic that produces a token which expired in 1970 or never.
#[must_use]
pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|since| i64::try_from(since.as_secs()).ok())
        .unwrap_or(0)
}

/// How many failures a name may accumulate before it has to wait.
const FREE: u32 = 5;
/// The first wait, doubling with each further failure.
const FIRST_WAIT: Duration = Duration::from_secs(2);
/// The longest wait. Deliberately not longer — see [`Throttle`].
const LONGEST_WAIT: Duration = Duration::from_secs(300);
/// How many names the throttle can be tracking at once.
const BUCKETS: usize = 256;

#[derive(Default, Clone, Copy)]
struct Bucket {
    /// Consecutive failures, cleared by a success.
    missed: u32,
    /// When attempts may resume, while that is in the future.
    until: Option<Instant>,
}

/// A bound on how fast a password can be guessed.
///
/// # Why a fixed array and not a map
///
/// The key is a name a stranger chose. A map keyed by it grows as fast as they
/// can invent names, which turns a defence into a way to exhaust memory. A fixed
/// array indexed by a hash of the name cannot grow at all.
///
/// # What the collisions cost, stated plainly
///
/// Two names can share a bucket, so a stranger guessing at one can make another
/// wait. That is worth knowing and it is not a new capability: anyone who wants
/// to make an account wait can guess at *that account* directly. Every per-name
/// throttle has this shape. It is why the wait is capped at five minutes and
/// clears itself — locking an editor out of a documentation site for a while is
/// an annoyance, and it is a smaller one than an unbounded password oracle.
pub struct Throttle {
    buckets: Mutex<[Bucket; BUCKETS]>,
}

impl std::fmt::Debug for Throttle {
    /// Says how many names are currently waiting and nothing else. Which names
    /// are being guessed at is not something a log line should hand over.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let waiting = self
            .buckets
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|bucket| bucket.until.is_some_and(|until| Instant::now() < until))
            .count();
        formatter
            .debug_struct("Throttle")
            .field("waiting", &waiting)
            .finish()
    }
}

impl Default for Throttle {
    fn default() -> Self {
        Self::new()
    }
}

impl Throttle {
    /// A throttle with nothing held against anybody.
    #[must_use]
    pub fn new() -> Self {
        Self {
            buckets: Mutex::new([Bucket::default(); BUCKETS]),
        }
    }

    /// Whether an attempt for `name` may be made at all.
    ///
    /// Asked **before** the verification, not after: a bound that is checked
    /// after the expensive part has run has bounded nothing.
    pub fn permit(&self, name: &str) -> bool {
        let mut buckets = self.buckets.lock().unwrap_or_else(PoisonError::into_inner);
        let bucket = &mut buckets[index_of(name)];
        match bucket.until {
            None => true,
            Some(waiting) if Instant::now() < waiting => false,
            // The wait is served, so one attempt is allowed through. The count
            // is *not* cleared: only a success clears it, or each wait would
            // hand back a fresh allowance of five and the delay would be a fixed
            // guess rate rather than a growing one.
            Some(_) => {
                bucket.until = None;
                true
            }
        }
    }

    /// Records that an attempt for `name` failed.
    pub fn failed(&self, name: &str) {
        let mut buckets = self.buckets.lock().unwrap_or_else(PoisonError::into_inner);
        let bucket = &mut buckets[index_of(name)];
        bucket.missed = bucket.missed.saturating_add(1);
        if bucket.missed > FREE {
            let doubling = bucket.missed.saturating_sub(FREE).saturating_sub(1);
            let wait = FIRST_WAIT
                .checked_mul(1_u32.checked_shl(doubling.min(16)).unwrap_or(u32::MAX))
                .unwrap_or(LONGEST_WAIT)
                .min(LONGEST_WAIT);
            // `checked_add`, so a wait the clock cannot represent becomes no wait
            // rather than a panic. It is not reachable with the cap above; it is
            // written this way because the cap is the only thing making it so.
            bucket.until = Instant::now().checked_add(wait);
        }
    }

    /// Records that an attempt for `name` succeeded, clearing what was held.
    pub fn succeeded(&self, name: &str) {
        let mut buckets = self.buckets.lock().unwrap_or_else(PoisonError::into_inner);
        buckets[index_of(name)] = Bucket::default();
    }
}

/// FNV-1a, folded into the bucket array.
fn index_of(name: &str) -> usize {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in name.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    let count = u64::try_from(BUCKETS).unwrap_or(1);
    usize::try_from(hash.checked_rem(count).unwrap_or(0)).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::{BUCKETS, FREE, Secret, Throttle, digest, hash, index_of, mint, now, verifies};

    #[test]
    fn a_token_does_not_print_itself() {
        let secret = Secret("deadbeef".to_owned());
        assert_eq!(format!("{secret:?}"), "Secret(<redacted>)");
        assert_eq!(secret.reveal(), "deadbeef");
    }

    #[test]
    fn two_tokens_are_not_the_same_token() {
        let one = mint();
        let two = mint();
        assert_ne!(one.reveal(), two.reveal());
        assert_eq!(one.reveal().len(), 64);
        assert!(one.reveal().chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn the_digest_is_hex_and_is_not_the_token() {
        let secret = mint();
        let hashed = digest(secret.reveal());
        assert_eq!(hashed.len(), 64);
        assert!(hashed.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(hashed, secret.reveal());
    }

    #[test]
    fn a_known_digest_matches_the_published_one() {
        // SHA-256 of the empty string, so this test fails if the hash is ever
        // quietly swapped for something else.
        assert_eq!(
            digest(""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn the_same_password_hashes_differently_every_time() {
        let one = hash("correct horse").expect("a hash");
        let two = hash("correct horse").expect("a hash");
        assert_ne!(
            one, two,
            "a fresh salt each time, or the hashes are a lookup table"
        );
        assert!(one.starts_with("$argon2id$"));
    }

    #[tokio::test]
    async fn a_password_verifies_against_its_own_hash_and_not_another() {
        let stored = hash("correct horse").expect("a hash");
        assert!(verifies("correct horse", &stored).await);
        assert!(!verifies("Correct horse", &stored).await);
        assert!(!verifies("", &stored).await);
    }

    #[tokio::test]
    async fn a_malformed_stored_hash_refuses_rather_than_panics() {
        assert!(!verifies("anything", "not a PHC string").await);
        assert!(!verifies("anything", "").await);
    }

    #[test]
    fn guessing_is_free_for_a_while_and_then_is_not() {
        let throttle = Throttle::new();
        for _ in 0..FREE {
            assert!(throttle.permit("ann"));
            throttle.failed("ann");
        }
        assert!(
            throttle.permit("ann"),
            "the fifth failure is the last free one"
        );
        throttle.failed("ann");
        assert!(!throttle.permit("ann"), "and the sixth has to wait");
    }

    #[test]
    fn a_success_clears_what_was_held() {
        let throttle = Throttle::new();
        for _ in 0..=FREE {
            throttle.failed("ann");
        }
        assert!(!throttle.permit("ann"));
        throttle.succeeded("ann");
        assert!(throttle.permit("ann"));
    }

    #[test]
    fn one_name_being_throttled_does_not_throttle_every_name() {
        let throttle = Throttle::new();
        for _ in 0..=FREE {
            throttle.failed("ann");
        }
        assert!(!throttle.permit("ann"));
        // Not "no name collides" — some will, by construction. This holds that
        // the throttle is per-bucket and not global.
        let elsewhere = (0..BUCKETS)
            .map(|n| format!("other{n}"))
            .find(|name| index_of(name) != index_of("ann"))
            .expect("a name in another bucket");
        assert!(throttle.permit(&elsewhere));
    }

    #[test]
    fn the_bucket_index_is_inside_the_array() {
        for n in 0..1000 {
            assert!(index_of(&format!("name{n}")) < BUCKETS);
        }
        assert!(index_of("") < BUCKETS);
    }

    #[test]
    fn the_clock_is_a_plausible_one() {
        // After 2020 and before 2100, which is enough to catch a wrapped cast
        // without pinning the test to a date.
        let seconds = now();
        assert!(seconds > 1_577_836_800, "before 2020");
        assert!(seconds < 4_102_444_800, "after 2100");
    }
}
