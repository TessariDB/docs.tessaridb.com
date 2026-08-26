import type { Metadata, Viewport } from "next";
import Link from "next/link";
import { connection } from "next/server";

import { Footer } from "@/components/Footer";
import { Search } from "@/components/Search";
import { Tree } from "@/components/Tree";
import { BEFORE_PAINT, ThemeToggle } from "@/components/Theme";
import { Mark } from "@/components/icons";
import { type TreeNode, nav } from "@/lib/api";
import "./globals.css";

export const metadata: Metadata = {
  title: { default: "TessariDB", template: "%s — TessariDB" },
  description:
    "Records, graphs, full-text and vectors in one store, and one language over all of them.",
};

/**
 * Without this a phone lays the page out in a virtual viewport of about 980px
 * and then zooms out to fit, so a `max-width: 860px` rule never matches
 * anything and every narrow-screen style in `globals.css` is dead code. The
 * failure is quiet in exactly the wrong way: the site looks like it was never
 * made responsive rather than like it is missing one line.
 */
export const viewport: Viewport = {
  width: "device-width",
  initialScale: 1,
  // Named so the browser's own chrome matches the theme rather than framing a
  // dark page in a light bar.
  themeColor: [
    { media: "(prefers-color-scheme: light)", color: "#ffffff" },
    { media: "(prefers-color-scheme: dark)", color: "#0d0d13" },
  ],
};

export default async function Layout({ children }: { children: React.ReactNode }) {
  // Nothing here is prerendered at build time, and that is the point of the
  // whole project rather than a configuration detail: the content lives in
  // TessariDB, so baking it into the image at build would make this the static
  // site it exists not to be — pages edited through the API would not appear
  // until somebody rebuilt and redeployed the front end.
  //
  // `connection()` marks the render as per-request without touching the fetch
  // cache, so the reads in `lib/api` still hold for their thirty seconds and
  // the store is not asked to verify a password for every reader.
  await connection();

  // The tree comes from the store on every render of the shell. If that read
  // fails the site still serves the page — a reader who followed a link should
  // get what they came for even when the navigation cannot be built.
  let tree: TreeNode[];
  try {
    tree = await nav();
  } catch (fault) {
    console.error("the tree could not be read", fault);
    tree = [];
  }

  return (
    // `suppressHydrationWarning` because the script below writes `data-theme`
    // onto this element before React sees it, which is the whole point of it.
    <html lang="en" suppressHydrationWarning>
      <head>
        <script dangerouslySetInnerHTML={{ __html: BEFORE_PAINT }} />
      </head>
      <body>
        <header className="header">
          <Link href="/" className="brand">
            <Mark />
            TessariDB
            <small>0.0.1-alpha</small>
          </Link>
          <div className="header-spacer" />
          <Search />
          <ThemeToggle />
        </header>

        <div className="layout">
          <Tree nodes={tree} />
          <main className="main">{children}</main>
        </div>

        <Footer />
      </body>
    </html>
  );
}
