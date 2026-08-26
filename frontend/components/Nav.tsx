"use client";

import { usePathname } from "next/navigation";
import { createContext, useContext, useEffect, useState } from "react";

import { Menu } from "./icons";

/**
 * Whether the navigation drawer is open, shared by the button that opens it and
 * the tree that is opened.
 *
 * A context rather than props because the two live in different parts of the
 * layout — the button belongs in the header, beside the other controls a reader
 * looks for, and the tree belongs in the grid column it occupies on a wide
 * screen. Lifting the state to a common ancestor would mean making the whole
 * layout a client component, which would cost the server render of the page
 * content for the sake of one boolean.
 */
const NavContext = createContext<{
  open: boolean;
  setOpen: (open: boolean) => void;
}>({ open: false, setOpen: () => {} });

export function useNav() {
  return useContext(NavContext);
}

export function NavProvider({ children }: { children: React.ReactNode }) {
  const [open, setOpen] = useState(false);
  const here = usePathname();

  // Following a link closes it. Otherwise the reader taps a page and is left
  // looking at the same wall of navigation they just tapped through.
  useEffect(() => setOpen(false), [here]);

  useEffect(() => {
    if (!open) return;

    function onKey(event: KeyboardEvent) {
      if (event.key === "Escape") setOpen(false);
    }
    document.addEventListener("keydown", onKey);

    // The page behind must not scroll while the drawer is over it. Without this
    // a swipe inside the drawer that reaches its end carries on scrolling the
    // article underneath, so the reader closes the drawer and finds themselves
    // somewhere else entirely — which is the same complaint as landing
    // half-way down a page, arrived at by a different route.
    const held = document.body.style.overflow;
    document.body.style.overflow = "hidden";

    return () => {
      document.removeEventListener("keydown", onKey);
      document.body.style.overflow = held;
    };
  }, [open]);

  return <NavContext.Provider value={{ open, setOpen }}>{children}</NavContext.Provider>;
}

/**
 * The control that opens the drawer.
 *
 * Hidden above 860px by the stylesheet, where the tree is a column that is
 * always there and needs no affordance at all.
 */
export function NavButton() {
  const { open, setOpen } = useNav();
  return (
    <button
      type="button"
      className="nav-button"
      aria-expanded={open}
      aria-controls="documentation-tree"
      aria-label={open ? "Close navigation" : "Open navigation"}
      onClick={() => setOpen(!open)}
    >
      <Menu size={20} />
    </button>
  );
}

/**
 * What dims the article while the drawer is over it.
 *
 * Rendered always and made visible by a class, so opening and closing are one
 * transition rather than an element appearing and then fading.
 */
export function NavBackdrop() {
  const { open, setOpen } = useNav();
  return (
    <div
      className={open ? "nav-backdrop open" : "nav-backdrop"}
      onClick={() => setOpen(false)}
      aria-hidden="true"
    />
  );
}
