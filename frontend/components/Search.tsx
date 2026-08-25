"use client";

import Link from "next/link";
import { useEffect, useRef, useState } from "react";

import type { Hit } from "@/lib/api";
import { Search as SearchIcon } from "./icons";

/**
 * The search box.
 *
 * It calls a route handler of ours rather than the API directly, because in the
 * deployment the API is on an internal network and only this front end is
 * exposed. The handler is a proxy and nothing more — the ranking is the store's,
 * and nothing here reorders what comes back.
 *
 * A result links to `page#anchor`, and the anchor is the one the API assigned
 * when it rendered the page, so the reader lands on the passage that matched.
 */
export function Search() {
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<Hit[] | null>(null);
  const [active, setActive] = useState(0);
  const box = useRef<HTMLDivElement>(null);
  const field = useRef<HTMLInputElement>(null);

  // Debounced, and every in-flight request is abandoned when a newer one
  // starts — otherwise a slow answer for `an` can arrive after the answer for
  // `analyzer` and replace it, showing results for a query nobody is looking at.
  useEffect(() => {
    const asked = query.trim();
    if (asked.length < 2) {
      setHits(null);
      return;
    }
    const stop = new AbortController();
    const timer = setTimeout(() => {
      fetch(`/api/search?q=${encodeURIComponent(asked)}`, { signal: stop.signal })
        .then((response) => (response.ok ? response.json() : []))
        .then((found: Hit[]) => {
          setHits(found);
          setActive(0);
        })
        .catch(() => {
          // An abort is the ordinary case here, not a failure worth reporting.
        });
    }, 140);
    return () => {
      clearTimeout(timer);
      stop.abort();
    };
  }, [query]);

  // `/` focuses the box, the way every documentation site does — but not while
  // the reader is already typing somewhere, which would swallow the character.
  useEffect(() => {
    function onKey(event: KeyboardEvent) {
      const target = event.target as HTMLElement | null;
      const typing =
        target?.tagName === "INPUT" ||
        target?.tagName === "TEXTAREA" ||
        target?.isContentEditable === true;
      if (event.key === "/" && !typing) {
        event.preventDefault();
        field.current?.focus();
      }
      if (event.key === "Escape") {
        setHits(null);
        field.current?.blur();
      }
    }
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, []);

  useEffect(() => {
    function onClick(event: MouseEvent) {
      if (!box.current?.contains(event.target as globalThis.Node)) setHits(null);
    }
    document.addEventListener("mousedown", onClick);
    return () => document.removeEventListener("mousedown", onClick);
  }, []);

  function onKeyDown(event: React.KeyboardEvent) {
    if (!hits || hits.length === 0) return;
    if (event.key === "ArrowDown") {
      event.preventDefault();
      setActive((at) => (at + 1) % hits.length);
    }
    if (event.key === "ArrowUp") {
      event.preventDefault();
      setActive((at) => (at - 1 + hits.length) % hits.length);
    }
    if (event.key === "Enter") {
      const chosen = hits[active];
      if (chosen) {
        event.preventDefault();
        window.location.href = link(chosen);
        setHits(null);
      }
    }
  }

  return (
    <div className="search" ref={box}>
      <div className="search-field">
        <SearchIcon size={15} />
        <input
          ref={field}
          type="search"
          value={query}
          placeholder="Search the documentation"
          aria-label="Search the documentation"
          autoComplete="off"
          spellCheck={false}
          onChange={(event) => setQuery(event.target.value)}
          onKeyDown={onKeyDown}
        />
        <span className="search-key" aria-hidden="true">
          /
        </span>
      </div>

      {hits === null ? null : hits.length === 0 ? (
        <div className="results">
          <p className="results-empty">
            Nothing matches <strong>{query.trim()}</strong>. Terms are whole
            words here, not fragments of them.
          </p>
        </div>
      ) : (
        <div className="results" role="listbox">
          {hits.map((hit, at) => (
            <Link
              key={`${hit.page}#${hit.anchor}`}
              href={link(hit)}
              className="result"
              role="option"
              aria-selected={at === active}
              data-active={at === active}
              onMouseEnter={() => setActive(at)}
              onClick={() => setHits(null)}
            >
              <div className="result-heading">{hit.heading}</div>
              <div className="result-where">{hit.page}</div>
              <div className="result-text">{hit.text}</div>
            </Link>
          ))}
        </div>
      )}
    </div>
  );
}

/** A hit's destination. The lead of a page has no anchor, so it has no hash. */
function link(hit: Hit): string {
  return hit.anchor ? `/${hit.page}#${hit.anchor}` : `/${hit.page}`;
}
