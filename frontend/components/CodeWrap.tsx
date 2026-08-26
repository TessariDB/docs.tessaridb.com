"use client";

import { usePathname } from "next/navigation";
import { useEffect } from "react";

/**
 * A code block that can be told to wrap.
 *
 * The blocks scroll sideways, which is right for a wide diagram and wrong for a
 * long statement somebody wants to *read* — on a phone a 140-character `SELECT`
 * is a 40-character window onto it. So each block that actually overflows gets
 * a button at its top right, and pressing it turns the horizontal scroll into
 * wrapped lines.
 *
 * **Off by default**, because the two shapes are each right for something: a
 * wrapped block loses the alignment that makes a formatted statement readable,
 * and a scrolling one keeps it. The reader decides per block, and the decision
 * is theirs alone — nothing here is remembered, because a preference silently
 * carried from one page to the next is a surprise on the page that inherits it.
 *
 * # Why this is DOM surgery and not markup
 *
 * The body arrives from the API as rendered HTML and this component must not
 * parse it — see `Article`. So the button is added to what the renderer emitted
 * rather than emitted by it, which has the property that matters most here:
 * with no JavaScript the page is exactly what it is today, a readable code
 * block that scrolls. The affordance is added when it can work and is absent
 * when it cannot, rather than being a dead control.
 */
export function CodeWrap() {
  const here = usePathname();

  useEffect(() => {
    let live = true;
    const shells: HTMLElement[] = [];

    const frame = requestAnimationFrame(() => {
      if (!live) return;
      for (const pre of document.querySelectorAll<HTMLPreElement>(".prose pre")) {
        shells.push(decorate(pre));
      }
      // A block that fits needs no button. Whether it fits is a property of the
      // viewport, so it is asked again when the viewport changes — the case
      // that matters is turning a phone sideways, where half the blocks on a
      // page stop overflowing at once.
      shells.forEach(offer);
    });

    const onResize = () => shells.forEach(offer);
    window.addEventListener("resize", onResize);

    return () => {
      live = false;
      cancelAnimationFrame(frame);
      window.removeEventListener("resize", onResize);
    };
  }, [here]);

  return null;
}

/**
 * The button's glyph — three lines, the middle one turning back on itself.
 *
 * Markup rather than a component from `icons`, because this button is built as
 * DOM and never passes through React. Drawn on the same 24 grid at the same
 * 1.7 stroke, so it belongs to the set even though it cannot live in it.
 */
const GLYPH =
  '<svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor"' +
  ' stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">' +
  '<path d="M4 6h16"/><path d="M4 12h13a3 3 0 1 1 0 6h-4"/>' +
  '<path d="M16 16l-2 2 2 2"/><path d="M4 18h5"/></svg>';

/**
 * Put one code block in a positioned shell with a button in the corner.
 *
 * Idempotent: a block already shelled is returned as it is, because this runs
 * again on every navigation and a second shell around the first would push the
 * button off the block it belongs to.
 */
function decorate(pre: HTMLPreElement): HTMLElement {
  const existing = pre.parentElement;
  if (existing?.classList.contains("code-block")) return existing;

  const shell = document.createElement("div");
  shell.className = "code-block";
  pre.replaceWith(shell);
  shell.append(pre);

  const button = document.createElement("button");
  button.type = "button";
  button.className = "code-wrap";
  button.innerHTML = GLYPH;
  // The name says what pressing it does; `aria-pressed` says which way it is
  // now. One control with a state, rather than two labels for one button.
  button.setAttribute("aria-label", "Wrap long lines");
  button.title = "Wrap long lines";
  button.setAttribute("aria-pressed", "false");
  button.addEventListener("click", () => {
    const wrapped = shell.classList.toggle("wrapped");
    button.setAttribute("aria-pressed", String(wrapped));
    // Once wrapped there is nothing to overflow, so the test below would take
    // the button away and leave no way back. It stays while it is pressed.
    offer(shell);
  });
  shell.append(button);

  return shell;
}

/** Show the button on a block that has something to wrap, and only then. */
function offer(shell: HTMLElement): void {
  const pre = shell.querySelector("pre");
  if (!pre) return;
  const wrapped = shell.classList.contains("wrapped");
  // A pixel of slack: `scrollWidth` and `clientWidth` are rounded independently,
  // so an exactly-fitting block can report one more than the other.
  const overflows = pre.scrollWidth > pre.clientWidth + 1;
  shell.classList.toggle("wrappable", wrapped || overflows);
}
