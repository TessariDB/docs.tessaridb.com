"use client";

import { useEffect, useState } from "react";

import { Moon, Sun } from "./icons";

/**
 * The script that decides the theme before the page paints.
 *
 * This has to run in `<head>`, synchronously, ahead of React. A theme applied
 * in an effect applies after first paint, so a reader who chose dark gets a
 * white flash on every navigation — brief, and the single most noticeable
 * defect a themed site can have.
 *
 * It reads the stored choice, falls back to the system setting, and touches
 * nothing else. `try` around storage because a browser refusing site data
 * throws on access rather than returning null, and a theme is not worth an
 * exception on the first line of the document.
 */
export const BEFORE_PAINT = `
(function () {
  try {
    var stored = localStorage.getItem("theme");
    var dark = stored ? stored === "dark"
      : matchMedia("(prefers-color-scheme: dark)").matches;
    document.documentElement.dataset.theme = dark ? "dark" : "light";
  } catch (_) {
    document.documentElement.dataset.theme = "light";
  }
})();
`;

/** The light/dark switch. */
export function ThemeToggle() {
  // Starts unset rather than guessing: the true value lives on the document,
  // put there by the script above, and rendering a guess on the server would
  // hydrate the wrong icon for half the readers.
  const [theme, setTheme] = useState<"light" | "dark" | null>(null);

  useEffect(() => {
    const found = document.documentElement.dataset.theme;
    setTheme(found === "dark" ? "dark" : "light");
  }, []);

  function toggle() {
    const next = theme === "dark" ? "light" : "dark";
    document.documentElement.dataset.theme = next;
    setTheme(next);
    try {
      localStorage.setItem("theme", next);
    } catch {
      // A browser that will not store it still gets the theme for this visit.
    }
  }

  return (
    <button
      type="button"
      className="icon-button"
      onClick={toggle}
      aria-label={theme === "dark" ? "Switch to the light theme" : "Switch to the dark theme"}
      // Empty until the effect has read the document, so nothing flickers
      // between a server guess and the truth.
      suppressHydrationWarning
    >
      {theme === null ? null : theme === "dark" ? <Sun size={17} /> : <Moon size={17} />}
    </button>
  );
}
