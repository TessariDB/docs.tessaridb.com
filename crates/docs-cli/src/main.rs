//! The site's binary.
//!
//! ```text
//! docs check      read content/ and say what is wrong with it
//! docs serve      serve the API, seeding the store only if it is empty
//! docs ingest     replace what the store holds with content/. Destructive.
//! ```
//!
//! # Why `check` needs no node
//!
//! Nearly everything that goes wrong with a documentation tree goes wrong on
//! disk: an unclosed front-matter fence, a directory that never said what it is
//! called, a page filed under a section it is not in. None of that needs a
//! database to find, so `check` does not open one — which makes it a thing CI
//! can run on a pull request without standing a node up first.
//!
//! # Why `serve` applies the schema and `ingest` does too
//!
//! Both, every time. Every definition carries `IF NOT EXISTS`, so applying it
//! to a store that already has it is a few statements and no change; the
//! alternative is remembering whether this deployment has run before, which is
//! state nobody wants to keep and everybody gets wrong on the fresh host folder
//! the containers mount.
//!
//! # Who owns the content, and why `serve` does not rebuild
//!
//! **The store owns it.** `content/` is where the corpus is written and reviewed
//! and it is what an empty store is seeded from, but once a page is in the store
//! the store is the answer — because pages are also edited through the API, by
//! whoever the store lets write, and a restart that rebuilt from disk would
//! throw those edits away without saying so. A write surface whose writes do not
//! survive a deployment is not a write surface.
//!
//! So `serve` seeds **only an empty store**, and rebuilding from disk over a
//! populated one is `--ingest`, or the `ingest` command: destructive, and asked
//! for out loud. This is the same reasoning the language applies to
//! `DELETE FROM page` — the destructive reading is never the default one.

mod arguments;
mod corpus;

use std::process::ExitCode;

use arguments::{Asked, Read, Task};
use docs_server::{Site, routes};
use docs_store::Store;

fn main() -> ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let words: Vec<String> = std::env::args().skip(1).collect();
    let asked = match arguments::read(&words, &|name| std::env::var(name).ok()) {
        Read::Do(asked) => *asked,
        Read::Say(said, 0) => {
            print!("{said}");
            return ExitCode::SUCCESS;
        }
        Read::Say(said, _) => {
            eprint!("{said}");
            return ExitCode::from(2);
        }
    };

    // The runtime is built here rather than by an attribute on `main`, so that a
    // command line that does not parse is answered without starting one.
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(runtime) => runtime,
        Err(fault) => {
            eprintln!("could not start: {fault}");
            return ExitCode::FAILURE;
        }
    };
    match runtime.block_on(run(&asked)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(said) => {
            eprintln!("{said}");
            ExitCode::FAILURE
        }
    }
}

async fn run(asked: &Asked) -> Result<(), String> {
    match asked.task {
        Task::Check => check(asked),
        Task::Ingest => ingest(asked).await,
        Task::Serve => serve(asked).await,
    }
}

/// Reads the content tree and reports it. Opens no connection.
fn check(asked: &Asked) -> Result<(), String> {
    let corpus = corpus::read(&asked.content).map_err(|fault| fault.to_string())?;
    let fragments: usize = corpus.pages.iter().map(|page| page.fragments.len()).sum();
    println!(
        "{} sections, {} pages, {fragments} searchable fragments in {}",
        corpus.sections.len(),
        corpus.pages.len(),
        asked.content.display()
    );
    Ok(())
}

/// Rebuilds the store from the content tree.
async fn ingest(asked: &Asked) -> Result<(), String> {
    // Read before connecting. A tree that will not parse should say so without
    // a node in the message, and without having emptied four tables first.
    let corpus = corpus::read(&asked.content).map_err(|fault| fault.to_string())?;
    let mut store = open(asked).await?;
    store
        .migrate()
        .await
        .map_err(|fault| format!("the schema would not apply: {fault}"))?;
    let written = store
        .ingest(&corpus)
        .await
        .map_err(|fault| format!("the ingest stopped: {fault}"))?;
    println!(
        "wrote {} sections, {} pages, {} fragments, {} edges into {}",
        written.sections, written.pages, written.fragments, written.edges, asked.namespace
    );
    Ok(())
}

/// Serves the API.
async fn serve(asked: &Asked) -> Result<(), String> {
    // The content is read before the store is opened, so that a tree that will
    // not parse says so without a node in the message — and, when it is about to
    // replace what is there, before four tables have been emptied.
    let corpus = if asked.ingest_first {
        Some(corpus::read(&asked.content).map_err(|fault| fault.to_string())?)
    } else {
        None
    };

    let mut store = open(asked).await?;

    // The schema is applied first, and whether it *can* be applied is the
    // question that decides everything after it.
    //
    // First, because on a brand-new store the namespace does not exist yet, and
    // asking an absent namespace how many pages it holds is a refusal rather
    // than the answer "none". Allowed to fail, because the account a deployed
    // site reads with is a viewer and a viewer may not `DEFINE` anything — so
    // the refusal is the signal, not a fault: a process that cannot define the
    // schema is not the process that set this store up, and its job is to read
    // what is there rather than to build it.
    let prepared = match store.migrate().await {
        Ok(()) => true,
        Err(fault) if fault.refusal().is_some() => {
            log::info!(
                "not applying the schema: {}. This is the ordinary case for a \
                 deployed site, which reads as a viewer.",
                fault.refusal().unwrap_or("refused")
            );
            false
        }
        Err(fault) => return Err(format!("the schema would not apply: {fault}")),
    };

    let held = store
        .pages_held()
        .await
        .map_err(|fault| format!("could not ask the store what it holds: {fault}"))?;

    match (corpus, held, prepared) {
        (Some(corpus), _, _) => {
            log::warn!(
                "--ingest: replacing the {held} pages in the store with {}",
                asked.content.display()
            );
            rebuild(&mut store, &corpus).await?;
        }
        (None, 0, true) => {
            // Seeded rather than rebuilt: an empty store has no edits to lose,
            // so this is the one moment where disk may speak for the store.
            //
            // No tree on disk is not a fault here. A deployment ships the schema
            // and the accounts and nothing else, and its pages arrive through the
            // API afterwards — so an empty store with no tree beside it is the
            // ordinary first minute of a new site, not a misconfiguration.
            match corpus::read(&asked.content) {
                Ok(corpus) => {
                    log::info!(
                        "the store is empty — seeding it from {}",
                        asked.content.display()
                    );
                    rebuild(&mut store, &corpus).await?;
                }
                Err(corpus::Fault::NoContent(_)) => log::info!(
                    "the store is empty and there is no tree at {} to seed it from, \
                     so its pages are the ones written through the API",
                    asked.content.display()
                ),
                Err(fault) => return Err(fault.to_string()),
            }
        }
        (None, 0, false) => {
            // Served rather than refused. An empty site is a poor page; a front
            // end that cannot start at all is a worse one, and the reason it is
            // empty is right here in the log.
            log::warn!(
                "the store holds no pages and this process may not write — run \
                 `docs ingest` with an account that can"
            );
        }
        (None, held, _) => {
            log::info!(
                "the store holds {held} pages and is the source; {} was not read",
                asked.content.display()
            );
        }
    }
    // Dropped before the first request, so the process does not hold a
    // privileged connection open for the whole of its life just because it
    // needed one at start.
    drop(store);

    let listener = tokio::net::TcpListener::bind(&asked.bind)
        .await
        .map_err(|fault| format!("could not listen on {}: {fault}", asked.bind))?;
    log::info!(
        "serving {} from the node at {} on {}",
        asked.namespace,
        asked.node,
        asked.bind
    );
    // The account the public reads run as. Absent only against an open store,
    // which is a development store: a store with any user in it refuses an
    // anonymous session outright, reads included.
    let reading_as = credentials("DOCS_READER_USER", "DOCS_READER_PASSWORD")?;
    match &reading_as {
        Some((name, _)) => log::info!("public reads run as {name}"),
        None => log::warn!(
            "DOCS_READER_USER is not set, so reads are anonymous — which works only \
             while the store has no users at all"
        ),
    }
    let site = Site::new(&asked.node, &asked.namespace, reading_as);
    axum::serve(listener, routes::router(site))
        .with_graceful_shutdown(stopped())
        .await
        .map_err(|fault| format!("the server stopped: {fault}"))
}

/// Replaces the store's contents, and says what it wrote.
async fn rebuild(store: &mut Store, corpus: &docs_store::ingest::Corpus) -> Result<(), String> {
    let written = store
        .ingest(corpus)
        .await
        .map_err(|fault| format!("the ingest stopped: {fault}"))?;
    log::info!(
        "wrote {} sections, {} pages, {} fragments, {} edges",
        written.sections,
        written.pages,
        written.fragments,
        written.edges
    );
    Ok(())
}

/// A connection carrying whatever credentials the environment supplied.
///
/// Both writing commands need one, and neither invents it: a store with users
/// defined refuses an anonymous write, which is the answer we want it to give.
async fn open(asked: &Asked) -> Result<Store, String> {
    // Whoever we can be. The first thing this connection does is ask a read-only
    // question, so on a deployed store where only the reading account is
    // configured it should be answered rather than refused; the writes that may
    // follow are the store's to allow, and it will say so plainly if it does not.
    let credentials = match credentials("DOCS_USER", "DOCS_PASSWORD")? {
        Some(pair) => Some(pair),
        None => credentials("DOCS_READER_USER", "DOCS_READER_PASSWORD")?,
    };
    Store::connect(&asked.node, &asked.namespace, credentials)
        .await
        .map_err(|fault| format!("could not reach the node at {}: {fault}", asked.node))
}

/// A name-and-password pair out of the environment, or nothing.
///
/// A name with no password is refused rather than treated as no credentials at
/// all: a deployment that meant to sign in and lost half its configuration
/// should say so, not quietly become anonymous and fail later with a message
/// about permissions.
fn credentials(name_of: &str, password_of: &str) -> Result<Option<(String, String)>, String> {
    let name = std::env::var(name_of).unwrap_or_default();
    if name.is_empty() {
        return Ok(None);
    }
    match std::env::var(password_of) {
        Ok(password) => Ok(Some((name, password))),
        Err(_) => Err(format!(
            "{name_of} is set to {name} and {password_of} is not"
        )),
    }
}

/// Resolves when the process is asked to stop.
///
/// Both signals, because a container sends `TERM` and a terminal sends `INT`,
/// and a server that only knows one of them is killed rather than stopped in
/// whichever case it does not know.
async fn stopped() {
    let interrupt = tokio::signal::ctrl_c();
    #[cfg(unix)]
    {
        let mut terminate =
            match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                Ok(terminate) => terminate,
                Err(fault) => {
                    log::warn!("no TERM handler, so only ctrl-c will stop this: {fault}");
                    let _ = interrupt.await;
                    return;
                }
            };
        tokio::select! {
            _ = interrupt => log::info!("interrupted"),
            _ = terminate.recv() => log::info!("asked to stop"),
        }
    }
    #[cfg(not(unix))]
    {
        let _ = interrupt.await;
    }
}
