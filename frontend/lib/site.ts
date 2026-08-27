/**
 * Where this site is, from the site's own point of view.
 *
 * A canonical URL, a sitemap entry and an Open Graph card all have to be
 * absolute, and none of them can be derived from the request: the front end sits
 * behind a TLS terminator, so the `Host` it sees is a loopback address and the
 * scheme it sees is `http`. Anything built from the incoming request would emit
 * `http://127.0.0.1:3000/...` as this page's canonical URL — which is not
 * wrong-looking enough to notice and is wrong everywhere it matters.
 *
 * Overridable by environment for a staging host, defaulting to production
 * because that is where it runs.
 */
export const origin = (process.env.DOCS_ORIGIN ?? "https://docs.tessaridb.com").replace(
  /\/$/,
  "",
);

/** The product site next door. */
export const product = "https://tessaridb.com";

/** What this site is, in one sentence, wherever one is needed. */
export const tagline =
  "The documentation for TessariDB — a multi-model database engine in Rust: documents, graph, full-text, vectors, geometry, key-value, files and time over one store, reached by one language.";
