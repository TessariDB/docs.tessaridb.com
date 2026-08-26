"use client";

import { usePathname } from "next/navigation";
import { useEffect } from "react";

/**
 * A new page starts at the top of it.
 *
 * Measured before this existed: `/reference/refusals` scrolled to 1031, a tap
 * on a tree link to `/query-language/graphs`, and the reader arrives at **1440**
 * — in the middle of a page they have never seen. It is worst on a phone,
 * where the tree closing as the link is followed changes the height of the
 * document underneath the browser's own scroll anchoring, which then helpfully
 * keeps a position that no longer means anything.
 *
 * Two details matter more than the `scrollTo`:
 *
 * A **hash still wins.** Every search result links to `page#anchor`, so
 * scrolling to the top unconditionally would break the one navigation on this
 * site that is deliberately not to the top of a page.
 *
 * It runs **after paint**, in a `requestAnimationFrame`. Scrolling to zero
 * before the new article has laid out is scrolling a document that is about to
 * change height, and scroll anchoring puts it back — which is exactly the bug,
 * reintroduced by the fix for it.
 */
export function ScrollReset() {
  const here = usePathname();

  useEffect(() => {
    const frame = requestAnimationFrame(() => {
      const hash = window.location.hash.slice(1);
      if (hash) {
        const target = document.getElementById(decodeURIComponent(hash));
        if (target) {
          target.scrollIntoView();
          return;
        }
      }
      window.scrollTo(0, 0);
    });
    return () => cancelAnimationFrame(frame);
  }, [here]);

  return null;
}
