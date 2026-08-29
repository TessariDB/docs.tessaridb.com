//! The site covers every type, function and statement the database has.
//!
//! # Why a manifest rather than an import
//!
//! This site reaches the database over the wire, like any other caller, and
//! depends on the engine's repository by no mechanism at all — that is what
//! makes it a demonstration of what an ordinary consumer can do. So it cannot
//! import the engine's type table, and the sets it must cover arrive as a
//! generated file instead.
//!
//! `reference-units.tsv` is produced by the engine's own suite:
//!
//! ```text
//! cargo test -p tessari-conformance --test documented \
//!     the_units_the_documentation_must_cover_can_be_printed -- --nocapture
//! ```
//!
//! It is copied, never typed. The engine's suite fails first when the language
//! grows, and its message says to regenerate this file.
//!
//! # Where this actually runs
//!
//! `content/` is deliberately not in the repository — the store owns the pages,
//! and a tree here is a local working copy. So these tests check a working copy
//! when there is one and say so when there is not, which is honest rather than
//! silent: the moment that matters is `cargo test` before `docs publish`, and
//! nobody publishes without a working copy.
//!
//! # Why the three are checked differently
//!
//! Measured on 2026-08-29, this site was missing twelve of the twenty
//! declarable types and thirty-one of its functions — whole families, not
//! stragglers. It was missing no statement. So types and functions are checked
//! **strictly**, because that is where the site actually decayed, and a
//! statement's coverage name is a label rather than syntax (`ALTER TABLE ALTER
//! FIELD` is not something anybody writes), so requiring it verbatim would fail
//! on prose that covers it perfectly well.

#![allow(clippy::panic, clippy::unwrap_used)]

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// The repository root — this crate is `crates/docs-content`, two levels down.
fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the repository root")
}

/// Every page of the local working copy, concatenated.
///
/// `None` when there is no working copy to check.
fn site() -> Option<String> {
    let root = repo().join("content");
    if !root.is_dir() {
        println!(
            "no working copy at {} — nothing to check. Run `docs ingest` or \
             draft some pages first.",
            root.display()
        );
        return None;
    }
    let mut pages = Vec::new();
    let mut stack = vec![root];
    while let Some(directory) = stack.pop() {
        let entries = fs::read_dir(&directory)
            .unwrap_or_else(|error| panic!("{}: {error}", directory.display()));
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|kind| kind == "md") {
                pages.push(fs::read_to_string(&path).unwrap_or_default());
            }
        }
    }
    assert!(pages.len() > 10, "only {} pages found", pages.len());
    Some(pages.join("\n"))
}

/// One row of the generated manifest.
struct Unit {
    kind: String,
    name: String,
    excused: bool,
}

fn manifest() -> Vec<Unit> {
    let path = repo().join("reference-units.tsv");
    let text =
        fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let units: Vec<Unit> = text
        .lines()
        .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
        .map(|line| {
            let mut columns = line.split('\t');
            Unit {
                kind: columns.next().unwrap_or_default().to_owned(),
                name: columns.next().unwrap_or_default().to_owned(),
                excused: columns.next() == Some("excused"),
            }
        })
        .collect();
    assert!(units.len() > 100, "the manifest holds only {}", units.len());
    units
}

/// The units of one kind that the site does not cover.
fn missing(kind: &str, covered: impl Fn(&str) -> bool) -> Vec<String> {
    manifest()
        .into_iter()
        .filter(|unit| unit.kind == kind && !unit.excused && !covered(&unit.name))
        .map(|unit| unit.name)
        .collect()
}

fn report(what: &str, missing: &[String]) {
    assert!(
        missing.is_empty(),
        "{} {what} the database has appear nowhere on the site: {missing:?}\n\
         Write them up, or — if the absence is deliberate — excuse them in the \
         engine's UNDOCUMENTED list with a reason and regenerate \
         reference-units.tsv.",
        missing.len(),
    );
}

#[test]
fn every_declarable_type_is_written_up() {
    let Some(text) = site() else {
        return;
    };
    report(
        "types",
        &missing("kind", |name| {
            // A union has no single spelling, so what is checked is that the
            // site shows one being declared.
            if name == "literal union" {
                return text.contains("TYPE '");
            }
            text.contains(&format!("TYPE {name}")) || text.contains(&format!("| `{name}` |"))
        }),
    );
}

#[test]
fn every_function_is_shown_being_called() {
    let Some(text) = site() else {
        return;
    };
    report(
        "functions",
        &missing("function", |name| text.contains(&format!("{name}("))),
    );
}

#[test]
fn every_statement_form_is_mentioned() {
    let Some(text) = site() else {
        return;
    };
    report(
        "statements",
        &missing("form", |name| {
            text.contains(name)
                || name.split_whitespace().all(|word| {
                    text.split_whitespace()
                        .any(|found| found.trim_matches('`') == word)
                })
        }),
    );
}

#[test]
fn the_site_names_no_function_the_database_does_not_have() {
    // The inverse, and the more embarrassing failure: a gap sends a reader
    // elsewhere, while an invented function sends them to `NoSuchFunction`.
    // This site carried two — `string::length` and a bare `len` — until
    // something checked.
    let Some(text) = site() else {
        return;
    };
    let known: BTreeSet<String> = manifest()
        .into_iter()
        .filter(|unit| unit.kind == "function")
        .map(|unit| unit.name)
        .collect();
    let mut invented = BTreeSet::new();
    for (at, _) in text.match_indices("::") {
        let Some(before) = text.get(..at) else {
            continue;
        };
        let group: String = before
            .chars()
            .rev()
            .take_while(char::is_ascii_lowercase)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        let Some(after) = text.get(at.saturating_add(2)..) else {
            continue;
        };
        let name: String = after
            .chars()
            .take_while(|character| character.is_ascii_lowercase() || *character == '_')
            .collect();
        if group.is_empty() || name.is_empty() {
            continue;
        }
        // The group must be a whole word, or a Rust name the prose legitimately
        // mentions arrives here with its capital shaved off.
        let whole = before
            .get(..before.len().saturating_sub(group.len()))
            .and_then(|earlier| earlier.chars().next_back())
            .is_none_or(|character| !character.is_alphanumeric() && character != '_');
        let called = after
            .get(name.len()..)
            .is_some_and(|rest| rest.starts_with('('));
        let spelling = format!("{group}::{name}");
        if whole && called && !known.contains(&spelling) {
            invented.insert(spelling);
        }
    }
    assert!(
        invented.is_empty(),
        "the site shows {invented:?} being called, and the database has no such \
         function — a reader following the page gets NoSuchFunction"
    );
}
