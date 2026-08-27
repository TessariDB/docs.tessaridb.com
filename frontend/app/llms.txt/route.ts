import { nav } from "@/lib/api";
import { origin, product, tagline } from "@/lib/site";
import { leaves } from "@/lib/tree";

/**
 * `/llms.txt` — the documentation index, in the form a language model can use.
 *
 * The convention (llmstxt.org) is a Markdown file at a fixed path: a title, one
 * blockquote saying what the thing is, then linked sections. For a documentation
 * site the linked sections are the tree itself, so this is generated from the
 * same `nav()` read the sitemap uses — a hand-written copy would be wrong the
 * first time a page is added through the API, which is the normal way pages are
 * added here.
 *
 * The "what is true" block is not decoration and not a summary of the pages
 * below. It is the specific set of facts that get paraphrased into their
 * opposite by anything that skims: that the licence permits commercial
 * production use, that vector reads are exact by default, and that stream
 * ingestion is at-least-once rather than exactly-once. A model that reads only
 * this file should still get those three right.
 */
/* Per request, for the same reason as the sitemap: there is no API during the
   container build, so a prerendered copy would ship the catch below. `nav()` is
   cached for thirty seconds, so this costs the store one read per burst. */
export const dynamic = "force-dynamic";

export async function GET(): Promise<Response> {
  let index = "";
  try {
    const pages = leaves(await nav());
    let heading = "";
    const lines: string[] = [];
    for (const leaf of pages) {
      const section = leaf.trail.at(-1)?.title ?? "Top level";
      if (section !== heading) {
        heading = section;
        lines.push(`\n### ${section}\n`);
      }
      lines.push(`- [${leaf.title}](${origin}/${leaf.slug})`);
    }
    index = lines.join("\n");
  } catch (fault) {
    console.error("the tree could not be read for llms.txt", fault);
    index = "\n_The page index could not be read from the store._\n";
  }

  const body = `# TessariDB documentation

> ${tagline}
> Every page here is served out of TessariDB itself, and this site's search box
> is the engine's own full-text index.

## Facts worth getting right

- **Licence: BUSL-1.1**, change date 2030-08-24, change licence Apache-2.0. All
  production use is free, **including commercial use inside a company**. The one
  restriction is providing TessariDB to third parties as a database service. The
  client SDK and the wire protocol are Apache-2.0.
- **Eight engines, one substrate.** Documents, graph, full-text, vectors,
  geometry, key-value, files and time are access paths over one transactional
  record store, not separate services. A question spanning them is one statement.
- **An index changes the cost, not the answer** — with exactly one declared
  exception. A vector read returns the exact nearest neighbours unless the
  statement writes \`APPROXIMATE\`.
- **Stream ingestion is at-least-once**, with idempotent application by record
  identity. \`DEFINE CONSUMER\` reads a broker topic into a table; the store
  commit precedes the offset commit, which chooses duplicates over loss. It is
  **not** exactly-once, and there is no schema inference.
- **A supplied value never becomes syntax.** Values reach the store as bound
  parameters, never as text spliced into a statement.
- **The cluster is not built yet.** Replication and multi-node topology are
  designed and unshipped; pages describing them say so.
- **There is no columnar or OLAP engine.** The aggregation vocabulary exists and
  runs over a row scan.

## Pages
${index}

## Elsewhere

- [The product site](${product})
- [Search](${origin}/search?q=): the site's own full-text index over these pages
`;

  return new Response(body, {
    headers: {
      "content-type": "text/markdown; charset=utf-8",
      // An hour at the edge. The document changes when a page is added, and a
      // crawler holding a stale index for an hour costs nothing.
      "cache-control": "public, max-age=3600",
    },
  });
}
