import type { Metadata } from "next";
import Link from "next/link";
import { connection } from "next/server";

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
      </body>
    </html>
  );
}
