"use client";

import { useRouter } from "next/navigation";
import { useEffect, useRef, useState } from "react";

import { Search as SearchIcon } from "./icons";

/**
 * The field on the search page.
 *
 * A plain form rather than the header box's live dropdown: here the results are
 * the page, so a keystroke that replaced them under the reader's eyes while
 * they were reading one would be the opposite of useful. Submitting navigates,
 * which is what puts the query in the URL and the results in the back button.
 *
 * It focuses itself on arrival, because there is exactly one thing to do on
 * this page — and it selects what is already there, so a reader who followed
 * *see all results* can retype without clearing first.
 */
export function SearchAgain({ initial }: { initial: string }) {
  const [query, setQuery] = useState(initial);
  const field = useRef<HTMLInputElement>(null);
  const router = useRouter();

  useEffect(() => {
    field.current?.focus();
    field.current?.select();
  }, []);

  // The prop is the source of truth when the URL changes underneath — a back
  // button that restored the results but left the old text in the box would be
  // showing a query and its answer that do not match.
  useEffect(() => setQuery(initial), [initial]);

  return (
    <form
      className="searchpage-form"
      role="search"
      onSubmit={(event) => {
        event.preventDefault();
        const asked = query.trim();
        if (asked) router.push(`/search?q=${encodeURIComponent(asked)}`);
      }}
    >
      <SearchIcon size={17} />
      <input
        ref={field}
        type="search"
        value={query}
        placeholder="Search the documentation"
        aria-label="Search the documentation"
        autoComplete="off"
        spellCheck={false}
        onChange={(event) => setQuery(event.target.value)}
      />
      <button type="submit">Search</button>
    </form>
  );
}
