<div align="center">

<img src="assets/logo/tessaridb-mark-256.png" alt="" width="88" height="88">

# docs.tessaridb.com

**The documentation site for [TessariDB](https://github.com/TessariDB/TessariDB)
— and the first thing built on it.**

[![status](https://img.shields.io/badge/status-in%20development-9A93C4?style=flat-square)](#status)
[![licence](https://img.shields.io/badge/licence-Apache--2.0-6B5FD1?style=flat-square)](LICENSE)

</div>

---

An ordinary documentation site: a left-hand tree of sections, a page, a
right-hand outline, search at the top, dark and light, and versions.

What is not ordinary is where it keeps things. Most documentation engines
generate a pile of static HTML at build time and bolt on a hosted search
service, because a static site cannot search itself. This one stores its content
in **TessariDB** and serves search from the database's own full-text index —
which makes the site both the documentation for the database and a working
demonstration of it.

That is the point of building it this way. A search box that returns the right
passage over content the same store holds as records is a more honest claim
about the engine than any benchmark table.

## Shape

| | |
|---|---|
| **Engine** | Rust — a small HTTP server, not a static generator |
| **Content** | records in TessariDB, written through the API. Not in this repository |
| **Search** | TessariDB full-text: one analyzed field, one index, `search::score` ranking |
| **Navigation** | the section tree is a graph in the store, walked with `RELATE` edges |
| **Versions** | a namespace per released version, so the doc set is versioned the way the database is |
| **Front end** | Next.js, server-rendered per request. It holds no content and never talks to the store |
| **Deploy** | three containers — database, API, front end — and a folder on the host |

## Running it

Three passwords, then one command.

```sh
cp deploy/.env.example deploy/.env    # set the three passwords
docker compose -f deploy/compose.yaml --env-file deploy/.env up --build
```

The site is then on <http://localhost:3000>. A fresh store declares its three
accounts and applies the schema on the first start, and comes up with no pages;
they are written through the API. Every start after that reads what is there.

### Without containers

```sh
tessaridb /tmp/store --serve 127.0.0.1:7654    # a node
cargo run -p docs-cli -- serve                 # the API, on :8080
cd frontend && DOCS_API=http://127.0.0.1:8080 npm run dev
```

### The public edge

Neither container publishes a port to the world: the front end binds loopback
and the API is only on the compose network. Something in front terminates TLS
and proxies to `127.0.0.1:3000`, and the nginx that does it here is in
[`deploy/nginx/`](deploy/nginx) rather than only on the host — an edge that
exists nowhere but on the machine is an edge that a rebuild loses.

## Who owns the content

**The store does**, and it is the only thing that does. The pages are records; a
deployment ships the schema, the accounts and no corpus, and every page arrives
through the API afterwards.

There is still a way to write a tree of Markdown in bulk, for drafting and for
moving a corpus between stores. A `content/` directory — untracked, because a
published site rebuilt out of somebody's stale checkout is exactly the failure
this avoids — is read by two commands:

```sh
docs check         # parse the tree and report what is wrong with it. No connection.
docs ingest        # replace what the store holds with it. Destructive.
```

`serve` seeds from such a tree only when the store is **empty** and one happens
to be there. It never rebuilds a populated store, because pages are edited
through the API and a restart that rebuilt from disk would discard those edits
in silence.

## Who may write

The database decides. There is no second authority.

The write routes take the caller's credentials and pass them to the store as
that connection's credentials, so the refusal — when it comes — is the store's
own permission check. `401` means *identify yourself*; `403` means *you did, and
the answer is no*.

Three accounts, because a store with no user is **open** and declaring the first
one closes it:

| account | role | used for |
|---|---|---|
| `owner` | `owner` | declaring the other two, once, on a fresh store |
| `reader` | `viewer` | the public read path — a closed store has no anonymous access |
| `editor` | `editor` | the seed, and the API's `PUT` and `DELETE` |

The reading account is a `viewer` on purpose: a write attempted on the public
path is then refused by the *store*, whatever this server believes it is doing.

> [!WARNING]
> The wire protocol carries no TLS, and Basic sends the password as given. The
> compose file puts the database and the API on a network declared `internal`
> and publishes only the front end. If you publish the API, put something that
> terminates TLS in front of it.

## What is true here

Every query-language example on this site is lifted from the engine's
conformance corpus — the suite that runs on every commit. An example that
appears here is an assertion that passes, not a sketch of an intended feature.

A page describing something the engine does not do yet says so at the top. There
is no third category.

## Status

**In development.** The workspace, the content pipeline, the store layer, the
API with authorization, the front end and the deployment are built and run. What
is not written yet is most of the content, and installation instructions.

## Licence

Apache-2.0. See [LICENSE](LICENSE).

The site content documents TessariDB, which is licensed separately under the
Business Source License 1.1.

Copyright (c) 2026 boogvar.
