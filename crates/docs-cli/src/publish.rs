//! `docs publish` — writing the corpus to a site over its own API.
//!
//! # Why this exists
//!
//! The other way to move a corpus is `docs ingest`, which runs **beside the
//! store**: it needs the machine, which in a deployment means a shell on the
//! host that runs the database. That makes editing a page a privilege somebody
//! holds over the whole machine, and it makes every runbook describing the
//! procedure a document about infrastructure rather than about publishing.
//!
//! This does the same job from a laptop, over the API the front end already
//! uses, with a token the API already issues. Nothing on the far side is
//! touched but the routes any client may call.
//!
//! # It says what it will do, and then does nothing
//!
//! A publish is a **dry run** unless `--apply` is given. That is not caution for
//! its own sake: the corpus on disk and the corpus in the store drift — pages
//! are edited through the API too — and the useful question before a publish is
//! *which pages would change*, which nothing else can answer.
//!
//! "Changed" here means **what a reader would see changes**. The comparison
//! renders the local Markdown with the same renderer the site uses and compares
//! that to what the site is serving, so a reformatted paragraph that renders
//! identically is not a change and a one-word edit is.
//!
//! A body is not all a reader sees. The comparison was the rendered HTML alone
//! until it was caught missing a move: adding `section` to four pages changed
//! where each one sits in the tree and changed not one word of prose, so the
//! publish reported nothing to do and wrote nothing. Everything that *places or
//! labels* a page — its title, its summary, its section, its unreleased notice
//! — is compared alongside the body, and a changed page names which of them
//! moved.
//!
//! # Removal is never a side effect of absence
//!
//! A page in the store and not on disk is *reported* and left alone. It is
//! removed only with `--prune`, and then only after being listed. `ingest`
//! replaces the whole corpus, which is right for seeding an empty store and
//! wrong for a site people edit: a stale checkout would silently delete a
//! month of work.

use std::collections::BTreeMap;
use std::path::Path;

use docs_store::ingest::Section;

use crate::arguments::Asked;
use crate::corpus;

/// Where the password is read from.
///
/// Not an argument, for the same reason it is not one anywhere else in this
/// project: an argument is in the process table for anybody on the machine to
/// read, and in the shell history afterwards.
pub const PASSWORD: &str = "DOCS_PUBLISH_PASSWORD";

/// A page as the site presents it — the body, and everything around the body.
///
/// The `section` half does not come from the same place as the rest: the page
/// route answers what a page *is*, and the navigation tree answers where it
/// *sits*. Both are needed to tell whether a publish would change anything, so
/// both are gathered into one value before the comparison sees them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Shown {
    /// The rendered body.
    pub html: String,
    /// The title, in the tree, the tab and every search result.
    pub title: String,
    /// The sentence under the title.
    pub summary: Option<String>,
    /// The section slug this page sits under; `None` at the root of the tree.
    pub section: Option<String>,
    /// Whether the page carries the not-yet-released notice.
    pub unreleased: bool,
}

impl Shown {
    /// Which fields differ, named, in a fixed order.
    ///
    /// Fixed order and `&'static str` so the printed line is stable between runs
    /// and the comparison allocates nothing per page.
    #[must_use]
    pub fn differences(&self, other: &Self) -> Vec<&'static str> {
        let mut moved = Vec::new();
        if self.html != other.html {
            moved.push("body");
        }
        if self.title != other.title {
            moved.push("title");
        }
        if self.summary != other.summary {
            moved.push("summary");
        }
        if self.section != other.section {
            moved.push("section");
        }
        if self.unreleased != other.unreleased {
            moved.push("unreleased");
        }
        moved
    }
}

/// A page that differs, and in what.
///
/// The field names are the point. A publish that says only *"changed"* about a
/// page whose body is untouched reads as a spurious rewrite, and the person
/// running it has no way to tell a moved page from a re-worded one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    /// The page.
    pub slug: String,
    /// What differs about it.
    pub fields: Vec<&'static str>,
}

/// What a publish would do, page by page.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Plan {
    /// Pages the site does not have.
    pub added: Vec<String>,
    /// Pages that differ, and in which fields.
    pub changed: Vec<Change>,
    /// Pages that are identical and will be skipped.
    pub unchanged: Vec<String>,
    /// Pages the site has and the corpus does not.
    pub absent: Vec<String>,
}

impl Plan {
    /// Whether applying this would write anything.
    #[must_use]
    pub fn writes(&self) -> bool {
        !self.added.is_empty() || !self.changed.is_empty()
    }

    /// The plan as lines to print, most consequential first.
    #[must_use]
    pub fn lines(&self, pruning: bool) -> Vec<String> {
        let mut said = Vec::new();
        for slug in &self.added {
            said.push(format!("  new      /{slug}"));
        }
        for change in &self.changed {
            said.push(format!(
                "  changed  /{}  ({})",
                change.slug,
                change.fields.join(", ")
            ));
        }
        for slug in &self.absent {
            // Named either way. A page about to be deleted and a page about to
            // be left behind are both things the person running this needs to
            // see, and the word is the only difference between them.
            said.push(if pruning {
                format!("  REMOVED  /{slug}")
            } else {
                format!("  on the site only, kept  /{slug}  (--prune removes it)")
            });
        }
        said.push(format!(
            "  {} new, {} changed, {} unchanged, {} on the site only",
            self.added.len(),
            self.changed.len(),
            self.unchanged.len(),
            self.absent.len()
        ));
        said
    }
}

/// Compare what the corpus holds against what the site is serving.
///
/// `local` maps a slug to what that page would be; `remote` maps a slug to what
/// the site is presenting for it. Taking both as already-gathered values is what
/// makes this testable without a network: the interesting logic is the set
/// arithmetic and the field comparison, and neither should need a server.
#[must_use]
pub fn compare(local: &BTreeMap<String, Shown>, remote: &BTreeMap<String, Shown>) -> Plan {
    let mut plan = Plan::default();
    for (slug, page) in local {
        match remote.get(slug) {
            None => plan.added.push(slug.clone()),
            Some(serving) => {
                let fields = page.differences(serving);
                if fields.is_empty() {
                    plan.unchanged.push(slug.clone());
                } else {
                    plan.changed.push(Change {
                        slug: slug.clone(),
                        fields,
                    });
                }
            }
        }
    }
    for slug in remote.keys() {
        if !local.contains_key(slug) {
            plan.absent.push(slug.clone());
        }
    }
    plan
}

/// The pages a nav tree holds, each with the section it sits under.
///
/// Only the pages. A section is not a page and cannot be compared with one —
/// they are separate routes on the API and separate rows in the store.
///
/// The enclosing section is the whole reason the tree is walked rather than
/// merely listed: no route answers a page's section, so where the tree puts it
/// is the only statement the site makes about where it belongs.
pub fn pages_in(tree: &[Node]) -> Vec<(String, Option<String>)> {
    let mut found = Vec::new();
    gather(tree, None, &mut found);
    found
}

fn gather(nodes: &[Node], within: Option<&str>, into: &mut Vec<(String, Option<String>)>) {
    for node in nodes {
        if node.kind == "page" {
            into.push((node.slug.clone(), within.map(str::to_owned)));
        }
        // A nested section becomes the enclosing one for everything beneath it,
        // so a page reports its immediate parent rather than the outermost group.
        let inside = if node.kind == "section" {
            Some(node.slug.as_str())
        } else {
            within
        };
        gather(&node.children, inside, into);
    }
}

/// One entry in the site's navigation tree, as the API answers it.
///
/// Declared here rather than borrowed from `docs-store` because this is a
/// *client*: what it must agree with is the JSON on the wire, and a type shared
/// with the server would let a change to the server's internals look like
/// agreement it had not actually kept.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Node {
    /// A section's own slug, or a page's path.
    pub slug: String,
    /// `section` or `page`.
    pub kind: String,
    /// Sections and pages beneath this one.
    #[serde(default)]
    pub children: Vec<Node>,
}

/// A page as the site is serving it.
///
/// A subset of what the route answers — the outline is derived from the body
/// and the slug is how the page was asked for, so neither can differ on its own.
#[derive(Debug, Clone, serde::Deserialize)]
struct Serving {
    html: String,
    title: String,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    unreleased: bool,
}

/// What `PUT /api/section/{slug}` takes.
#[derive(Debug, serde::Serialize)]
struct SectionBody<'a> {
    title: &'a str,
    parent: Option<&'a str>,
    order: i64,
    icon: Option<&'a str>,
}

/// Publish the corpus to the site named on the command line.
///
/// # Errors
///
/// Returns a message when the corpus does not parse, the site cannot be
/// reached, the credential is refused, or any write is refused.
pub async fn publish(asked: &Asked) -> Result<(), String> {
    let site = asked.site.trim_end_matches('/').to_owned();
    if site.is_empty() {
        return Err("publish wants --to <https://host> — the site to write to".to_owned());
    }
    let user = asked
        .user
        .clone()
        .ok_or_else(|| format!("publish wants --user <name>, and the password in {PASSWORD}"))?;
    let password = std::env::var(PASSWORD)
        .ok()
        .filter(|held| !held.is_empty())
        .ok_or_else(|| format!("{PASSWORD} is not set"))?;

    // Read and validate before anything is sent. A corpus that does not parse
    // is a corpus that must not be half-published, and finding that out after
    // twenty pages have been written is the worst moment to find it out.
    let corpus = corpus::read(&asked.content).map_err(|fault| fault.to_string())?;
    log::info!(
        "{} sections, {} pages in {}",
        corpus.sections.len(),
        corpus.pages.len(),
        asked.content.display()
    );

    let client = reqwest::Client::builder()
        .user_agent(concat!("docs/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|fault| format!("could not start a client: {fault}"))?;

    let token = open_session(&client, &site, &user, &password).await?;
    // From here on every exit hands the token back rather than leaving it live
    // until it expires — a publish that ran from somebody's laptop should not
    // leave a credential behind on the site.
    let outcome = run(&client, &site, &token, asked, &corpus).await;
    close_session(&client, &site, &token).await;
    outcome
}

/// The publish itself, between opening the session and handing it back.
async fn run(
    client: &reqwest::Client,
    site: &str,
    token: &str,
    asked: &Asked,
    corpus: &docs_store::ingest::Corpus,
) -> Result<(), String> {
    let mut local = BTreeMap::new();
    for page in &corpus.pages {
        local.insert(
            page.slug.clone(),
            Shown {
                html: docs_content::to_html(&page.markdown),
                title: page.front.title.clone(),
                summary: page.front.summary.clone(),
                // Resolved from the directory the file was read from, so this is
                // where the corpus says the page belongs.
                section: page.front.section.clone(),
                unreleased: page.front.unreleased,
            },
        );
    }

    // The tree is read first because the page route does not answer a section
    // and the tree is the only place the site says where a page sits.
    let tree: Vec<Node> = fetch(client, &format!("{site}/api/nav")).await?;
    let placed: BTreeMap<String, Option<String>> = pages_in(&tree).into_iter().collect();

    // Pages are still asked for one at a time rather than read out of the tree.
    // The tree holds what the tree *shows*, and the home page is not a link in
    // its own navigation — deriving the remote set from it reported `/index` as
    // new on every publish and rewrote it every time.
    let mut remote = BTreeMap::new();
    for slug in local.keys() {
        if let Ok(serving) = fetch::<Serving>(client, &format!("{site}/api/page/{slug}")).await {
            remote.insert(
                slug.clone(),
                showing(serving, placed.get(slug).cloned().flatten()),
            );
        }
    }
    // The tree answers the other direction — a page the site has and the corpus
    // does not, which by definition is not in `local` to ask about. A page in
    // the tree that will not load is left out rather than crashing the publish:
    // the tree and the pages are separate rows and one can outlive the other.
    for (slug, section) in pages_in(&tree) {
        if local.contains_key(&slug) || remote.contains_key(&slug) {
            continue;
        }
        if let Ok(serving) = fetch::<Serving>(client, &format!("{site}/api/page/{slug}")).await {
            remote.insert(slug, showing(serving, section));
        }
    }

    let plan = compare(&local, &remote);
    println!("{site}");
    for line in plan.lines(asked.prune) {
        println!("{line}");
    }

    if !asked.apply {
        println!("\nnothing was written — add --apply to do it");
        return Ok(());
    }
    if !plan.writes() && !(asked.prune && !plan.absent.is_empty()) {
        println!("\nnothing to do");
        return Ok(());
    }

    // Sections first. A page whose section does not exist yet is a page the
    // tree cannot place, and the order here is the only thing that prevents it.
    for section in &corpus.sections {
        put_section(client, site, token, section).await?;
    }
    for slug in plan
        .added
        .iter()
        .chain(plan.changed.iter().map(|change| &change.slug))
    {
        let source = std::fs::read_to_string(source_of(&asked.content, slug))
            .map_err(|fault| format!("/{slug}: {fault}"))?;
        put_page(client, site, token, slug, &source).await?;
        println!("  wrote    /{slug}");
    }
    if asked.prune {
        for slug in &plan.absent {
            delete_page(client, site, token, slug).await?;
            println!("  removed  /{slug}");
        }
    }
    println!("\ndone");
    Ok(())
}

/// What the site is presenting, from the page route plus the page's place in
/// the navigation tree.
///
/// A page the tree does not show reads as sitting at the root — which is what
/// the corpus says about the home page too, so the two agree rather than
/// reporting a change on every publish.
fn showing(serving: Serving, section: Option<String>) -> Shown {
    Shown {
        html: serving.html,
        title: serving.title,
        summary: serving.summary,
        section,
        unreleased: serving.unreleased,
    }
}

/// The file a slug was read from.
///
/// The exact inverse of how `corpus` derives a slug — the path under the content
/// root, with `.md` back on.
fn source_of(root: &Path, slug: &str) -> std::path::PathBuf {
    root.join(slug).with_extension("md")
}

/// Sign in, and answer with the token.
async fn open_session(
    client: &reqwest::Client,
    site: &str,
    user: &str,
    password: &str,
) -> Result<String, String> {
    #[derive(serde::Deserialize)]
    struct Issued {
        token: String,
    }
    let reply = client
        .post(format!("{site}/api/session"))
        // Basic, because that is what the route reads. The password is spent
        // here once and a token carries the rest of the publish.
        .basic_auth(user, Some(password))
        .send()
        .await
        .map_err(|fault| format!("{site}: {fault}"))?;
    let status = reply.status();
    let body = reply.text().await.unwrap_or_default();
    if !status.is_success() {
        // The site's own words. A publish refused for a reason the site
        // explained should not have that explanation replaced by a guess.
        return Err(format!("{site} refused the sign-in: {status} {body}"));
    }
    let issued: Issued = serde_json::from_str(&body)
        .map_err(|fault| format!("{site} answered with no token: {fault}"))?;
    Ok(issued.token)
}

/// Hand the token back. Failures are logged, never raised.
///
/// The publish has already happened or already failed by the time this runs, and
/// turning "the token could not be revoked" into the command's exit status would
/// report a successful publish as a failure.
async fn close_session(client: &reqwest::Client, site: &str, token: &str) {
    let sent = client
        .delete(format!("{site}/api/session"))
        .bearer_auth(token)
        .send()
        .await;
    if let Err(fault) = sent {
        log::warn!("the session could not be handed back, and will expire: {fault}");
    }
}

/// A `GET` answering JSON.
async fn fetch<T: serde::de::DeserializeOwned>(
    client: &reqwest::Client,
    url: &str,
) -> Result<T, String> {
    let reply = client
        .get(url)
        .send()
        .await
        .map_err(|fault| format!("{url}: {fault}"))?;
    let status = reply.status();
    let body = reply.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!("{url}: {status} {body}"));
    }
    serde_json::from_str(&body).map_err(|fault| format!("{url}: {fault}"))
}

async fn put_page(
    client: &reqwest::Client,
    site: &str,
    token: &str,
    slug: &str,
    source: &str,
) -> Result<(), String> {
    written(
        client
            .put(format!("{site}/api/page/{slug}"))
            .bearer_auth(token)
            .header("Content-Type", "text/plain; charset=utf-8")
            .body(source.to_owned()),
        &format!("/{slug}"),
    )
    .await
}

async fn put_section(
    client: &reqwest::Client,
    site: &str,
    token: &str,
    section: &Section,
) -> Result<(), String> {
    written(
        client
            .put(format!("{site}/api/section/{}", section.slug))
            .bearer_auth(token)
            .json(&SectionBody {
                title: &section.title,
                parent: section.parent.as_deref(),
                order: section.order,
                icon: section.icon.as_deref(),
            }),
        &format!("section {}", section.slug),
    )
    .await
}

async fn delete_page(
    client: &reqwest::Client,
    site: &str,
    token: &str,
    slug: &str,
) -> Result<(), String> {
    written(
        client
            .delete(format!("{site}/api/page/{slug}"))
            .bearer_auth(token),
        &format!("/{slug}"),
    )
    .await
}

/// Send a write and turn anything but success into the site's own message.
async fn written(request: reqwest::RequestBuilder, what: &str) -> Result<(), String> {
    let reply = request
        .send()
        .await
        .map_err(|fault| format!("{what}: {fault}"))?;
    let status = reply.status();
    if status.is_success() {
        return Ok(());
    }
    let body = reply.text().await.unwrap_or_default();
    Err(format!("{what}: {status} {body}"))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{Node, Shown, compare, pages_in, source_of};

    /// A page with nothing remarkable about it, to vary one field at a time.
    fn page(html: &str) -> Shown {
        Shown {
            html: html.to_owned(),
            title: "A page".to_owned(),
            summary: None,
            section: None,
            unreleased: false,
        }
    }

    fn corpus(entries: &[(&str, &str)]) -> BTreeMap<String, Shown> {
        entries
            .iter()
            .map(|(slug, html)| ((*slug).to_owned(), page(html)))
            .collect()
    }

    /// The corpus and the site agreeing about one page, ready to disagree about
    /// exactly one field.
    fn pair() -> (BTreeMap<String, Shown>, BTreeMap<String, Shown>) {
        (
            corpus(&[("a", "<p>one</p>")]),
            corpus(&[("a", "<p>one</p>")]),
        )
    }

    /// The fields a plan reports as changed for the one page in it.
    fn moved(plan: &super::Plan) -> Vec<&'static str> {
        plan.changed
            .first()
            .map(|change| change.fields.clone())
            .unwrap_or_default()
    }

    #[test]
    fn a_page_the_site_lacks_is_new_and_one_that_renders_the_same_is_not_a_change() {
        let plan = compare(
            &corpus(&[("a", "<p>one</p>"), ("b", "<p>two</p>")]),
            &corpus(&[("a", "<p>one</p>")]),
        );
        assert_eq!(plan.added, ["b"]);
        assert_eq!(plan.unchanged, ["a"]);
        assert!(plan.changed.is_empty());
        assert!(plan.absent.is_empty());
    }

    #[test]
    fn a_different_rendering_is_a_change() {
        let plan = compare(
            &corpus(&[("a", "<p>one</p>")]),
            &corpus(&[("a", "<p>ONE</p>")]),
        );
        assert_eq!(moved(&plan), ["body"]);
        assert!(plan.unchanged.is_empty());
    }

    // The four below are the defect this comparison was widened for: each moves
    // one field that leaves the rendered body byte-identical, and each was
    // reported `unchanged` — so the page was never rewritten and the site kept
    // serving the old value with nothing saying so. One field per test, so a
    // regression names which one came back.

    #[test]
    fn a_page_that_moved_section_is_a_change_even_though_it_reads_the_same() {
        let (mut local, remote) = pair();
        local.get_mut("a").expect("the page").section = Some("guides".to_owned());
        let plan = compare(&local, &remote);
        assert_eq!(moved(&plan), ["section"]);
    }

    #[test]
    fn a_retitled_page_is_a_change() {
        let (mut local, remote) = pair();
        local.get_mut("a").expect("the page").title = "Another page".to_owned();
        assert_eq!(moved(&compare(&local, &remote)), ["title"]);
    }

    #[test]
    fn a_rewritten_summary_is_a_change() {
        let (mut local, remote) = pair();
        local.get_mut("a").expect("the page").summary = Some("what it is".to_owned());
        assert_eq!(moved(&compare(&local, &remote)), ["summary"]);
    }

    #[test]
    fn gaining_or_losing_the_unreleased_notice_is_a_change() {
        let (mut local, remote) = pair();
        local.get_mut("a").expect("the page").unreleased = true;
        assert_eq!(moved(&compare(&local, &remote)), ["unreleased"]);
    }

    #[test]
    fn a_changed_page_says_what_moved() {
        // The half of the defect that is about the output rather than the
        // comparison: "changed" alone reads as a spurious rewrite when the body
        // is untouched, and gives the person running it nothing to check.
        let (mut local, remote) = pair();
        let page = local.get_mut("a").expect("the page");
        page.section = Some("guides".to_owned());
        page.title = "Another page".to_owned();
        let printed = compare(&local, &remote).lines(false).join("\n");
        assert!(
            printed.contains("changed  /a  (title, section)"),
            "{printed}"
        );
    }

    #[test]
    fn a_page_only_the_site_has_is_reported_and_not_written_off() {
        // The property that makes this safe to run from a stale checkout: a
        // page edited through the API and never pulled down is *named*, not
        // deleted, and `writes()` does not count it.
        let plan = compare(&corpus(&[]), &corpus(&[("gone", "<p>x</p>")]));
        assert_eq!(plan.absent, ["gone"]);
        assert!(!plan.writes());
    }

    #[test]
    fn what_is_kept_and_what_is_removed_read_differently() {
        let plan = compare(&corpus(&[]), &corpus(&[("gone", "<p>x</p>")]));
        let kept = plan.lines(false).join("\n");
        let pruned = plan.lines(true).join("\n");
        assert!(kept.contains("kept"), "{kept}");
        assert!(kept.contains("--prune"), "{kept}");
        assert!(pruned.contains("REMOVED"), "{pruned}");
        assert!(!pruned.contains("kept"), "{pruned}");
    }

    #[test]
    fn an_unchanged_corpus_writes_nothing() {
        let both = corpus(&[("a", "<p>one</p>")]);
        assert!(!compare(&both, &both).writes());
    }

    #[test]
    fn only_pages_are_compared_and_sections_are_not() {
        // They are separate routes and separate rows; a section slug arriving
        // in the page comparison would look like a page the corpus had lost.
        let tree = vec![Node {
            slug: "security".to_owned(),
            kind: "section".to_owned(),
            children: vec![Node {
                slug: "security/users".to_owned(),
                kind: "page".to_owned(),
                children: Vec::new(),
            }],
        }];
        assert_eq!(
            pages_in(&tree),
            [("security/users".to_owned(), Some("security".to_owned()))]
        );
    }

    #[test]
    fn a_page_reports_the_section_it_is_immediately_inside_and_a_root_page_reports_none() {
        // A nested section, because the useful answer is the page's own parent
        // rather than the outermost group it happens to sit beneath — the tree
        // is the only statement the site makes about where a page belongs.
        let tree = vec![
            Node {
                slug: "index".to_owned(),
                kind: "page".to_owned(),
                children: Vec::new(),
            },
            Node {
                slug: "query-language".to_owned(),
                kind: "section".to_owned(),
                children: vec![Node {
                    slug: "query-language/reads".to_owned(),
                    kind: "section".to_owned(),
                    children: vec![Node {
                        slug: "query-language/reads/select".to_owned(),
                        kind: "page".to_owned(),
                        children: Vec::new(),
                    }],
                }],
            },
        ];
        assert_eq!(
            pages_in(&tree),
            [
                ("index".to_owned(), None),
                (
                    "query-language/reads/select".to_owned(),
                    Some("query-language/reads".to_owned())
                ),
            ]
        );
    }

    #[test]
    fn a_slug_names_the_file_it_was_read_from() {
        let root = std::path::Path::new("content");
        assert_eq!(
            source_of(root, "security/users"),
            std::path::Path::new("content/security/users.md")
        );
        assert_eq!(
            source_of(root, "index"),
            std::path::Path::new("content/index.md")
        );
    }
}
