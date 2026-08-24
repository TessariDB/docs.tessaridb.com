<div align="center">

<img src="assets/logo/tessaridb-mark-256.png" alt="" width="88" height="88">

# docs.tessaridb.com

**The documentation site for [TessariDB](https://github.com/TessariDB/TessariDB)
— and the first thing built on it.**

[![licence](https://img.shields.io/badge/licence-Apache--2.0-6B5FD1)](LICENSE)
[![status](https://img.shields.io/badge/status-planned-9A93C4)](#status)

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
page in a few milliseconds, over content the same store holds as records, is a
more honest claim about the engine than any benchmark table.

## Shape

| | |
|---|---|
| **Engine** | Rust — a small HTTP server, not a static generator |
| **Content** | Markdown in `content/`, ingested into TessariDB as records |
| **Search** | TessariDB full-text: per-field analyzer, term index, ranked results |
| **Navigation** | the section tree is a graph in the store, not a hand-kept sidebar file |
| **Versions** | a namespace per released version, so the whole doc set is versioned the way the database is |
| **Deploy** | one binary and one store |

## Status

**Planned.** The repository exists, the shape is decided, and no engine code is
written yet. Nothing here works today.

## Licence

Apache-2.0. See [LICENSE](LICENSE).

The site content documents TessariDB, which is licensed separately under the
Business Source License 1.1.

Copyright (c) 2026 boogvar.
