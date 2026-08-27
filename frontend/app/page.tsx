import type { Metadata } from "next";
import { notFound } from "next/navigation";
import { connection } from "next/server";

import { Article } from "@/components/Article";
import { page as read } from "@/lib/api";
import { origin, tagline } from "@/lib/site";

/**
 * The front door.
 *
 * It is a page in the store like any other, at the slug `index`, so the landing
 * page is edited the same way every other page is — through the API or in
 * `content/` — rather than living in this repository as a component nobody
 * without a checkout can change.
 */
/**
 * The front door's description is the `index` page's own summary, so whoever
 * edits the landing page also edits what a search result says about it — rather
 * than the description living here, in a repository they may not have.
 *
 * The canonical is stated because this page is reachable both as `/` and, since
 * it is a record like any other, as `/index`. Two URLs for one page is duplicate
 * content competing with itself for the same words.
 */
export async function generateMetadata(): Promise<Metadata> {
  const found = await read("index");
  const description = found?.summary ?? tagline;
  return {
    description,
    alternates: { canonical: "/" },
    openGraph: {
      type: "website",
      siteName: "TessariDB documentation",
      title: "TessariDB documentation",
      description,
      url: origin,
    },
  };
}

export default async function Home() {
  // Never prerendered — see the layout for why.
  await connection();
  const found = await read("index");
  if (!found) notFound();
  return <Article page={found} />;
}
