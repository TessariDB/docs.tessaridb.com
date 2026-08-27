import type { Metadata } from "next";
import Link from "next/link";
import { connection } from "next/server";

import { SearchAgain } from "@/components/SearchAgain";
import { type Hit, search } from "@/lib/api";

type Asked = { searchParams: Promise<{ q?: string }> };

export async function generateMetadata({ searchParams }: Asked): Promise<Metadata> {
  const { q } = await searchParams;
  /*
   * `noindex, follow`. robots.txt already disallows this path, but the two rules
   * do different jobs: robots.txt stops the crawl, and a URL that is merely
   * uncrawled can still be indexed from an inbound link — as a bare title.
   * `noindex` is what keeps it out; `follow` means a shared result page still
   * passes the reader through to the real page.
   *
   * Without both, every distinct `?q=` is a URL carrying the same words as the
   * pages it is searching, competing with them.
   */
  return { title: q ? `Search: ${q}` : "Search", robots: { index: false, follow: true } };
}

/**
 * A whole page of results.
 *
 * The header box answers a question you already know how to ask — you type a
 * word, you recognise the page, you go. This is for the other case: not knowing
 * which page holds a thing, and needing to read the passages to tell. A
 * dropdown is the wrong shape for that. It holds a handful of hits, it covers
 * the page it is searching, and on a phone it covers all of it.
 *
 * Being a page rather than a bigger dropdown buys three things a dropdown
 * cannot have: the query is in the URL, so a search can be linked, shared and
 * gone back to; the back button returns to the results instead of to an empty
 * box; and the results have room to show enough of each passage to be judged.
 *
 * Rendered on the server. The ranking is the store's own index — the same
 * index, over the same fragments, that the header box reads.
 */
export default async function SearchPage({ searchParams }: Asked) {
  await connection();
  const { q } = await searchParams;
  const asked = (q ?? "").trim();

  let hits: Hit[] = [];
  let broke = false;
  if (asked.length >= 2) {
    try {
      hits = await search(asked, 60);
    } catch {
      broke = true;
    }
  }

  return (
    <div className="searchpage">
      <h1>Search</h1>
      <SearchAgain initial={asked} />

      {asked.length < 2 ? (
        <p className="searchpage-note">
          Two letters or more. Terms are whole words here — the index holds the
          words a page contains, not fragments of them, so <code>analyz</code>{" "}
          finds nothing and <code>analyzer</code> finds the pages about one.
        </p>
      ) : broke ? (
        <p className="searchpage-note">
          The index could not be reached. The pages are still where they were —
          the <Link href="/reference/statements">reference</Link> lists the
          language in full.
        </p>
      ) : hits.length === 0 ? (
        <p className="searchpage-note">
          Nothing matches <strong>{asked}</strong>.
        </p>
      ) : (
        <>
          <p className="searchpage-count">
            {hits.length === 60 ? "The first 60 passages" : `${hits.length} passages`} for{" "}
            <strong>{asked}</strong>
          </p>
          <ol className="searchpage-hits">
            {hits.map((hit) => (
              <li key={`${hit.page}#${hit.anchor}`}>
                <Link href={hit.anchor ? `/${hit.page}#${hit.anchor}` : `/${hit.page}`}>
                  <span className="searchpage-where">{hit.page}</span>
                  <span className="searchpage-heading">{hit.heading}</span>
                  <span className="searchpage-text">{hit.text}</span>
                </Link>
              </li>
            ))}
          </ol>
        </>
      )}
    </div>
  );
}
