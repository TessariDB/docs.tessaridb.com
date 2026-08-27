import type { Metadata } from "next";
import { notFound } from "next/navigation";
import { connection } from "next/server";

import { Article } from "@/components/Article";
import { PageData } from "@/components/PageData";
import { nav, page as read } from "@/lib/api";
import { origin, tagline } from "@/lib/site";
import { leaves } from "@/lib/tree";

type Asked = { params: Promise<{ slug: string[] }> };

/**
 * Every documentation page.
 *
 * One route rather than one per level, because the depth of the tree is a
 * property of the content — a fourth level of nesting is a directory somebody
 * added, and should not be a route somebody has to add too.
 */
export default async function Documentation({ params }: Asked) {
  // Never prerendered — see the layout for why.
  await connection();
  const { slug } = await params;
  const path = slug.join("/");
  const found = await read(path);
  if (!found) notFound();
  return (
    <>
      <PageData page={found} trail={await trail(path)} />
      <Article page={found} />
    </>
  );
}

/**
 * The sections above a page, for the breadcrumb.
 *
 * Read from the same tree the sidebar is built from, so the crumb a search
 * result shows and the crumb the sidebar draws cannot disagree. A tree that
 * cannot be read gives a two-step breadcrumb rather than a failed page: a
 * poorer result snippet is not worth trading a working page for.
 */
async function trail(path: string): Promise<{ title: string }[]> {
  try {
    return leaves(await nav()).find((leaf) => leaf.slug === path)?.trail ?? [];
  } catch (fault) {
    console.error("the tree could not be read for a breadcrumb", fault);
    return [];
  }
}

export async function generateMetadata({ params }: Asked): Promise<Metadata> {
  const { slug } = await params;
  const path = slug.join("/");
  const found = await read(path);
  /*
   * `noindex` on the miss. Without it the 404 this route renders is, as far as
   * metadata is concerned, an ordinary page — and a crawler following a stale
   * link indexes "Not found" under the old URL.
   */
  if (!found) return { title: "Not found", robots: { index: false, follow: true } };

  const description = found.summary ?? tagline;
  return {
    title: found.title,
    description,
    alternates: { canonical: `/${path}` },
    openGraph: {
      /*
       * `article`, not `website` — and declared per page rather than inherited.
       * Next MERGES metadata rather than deriving it, so a page that sets only
       * a title keeps the layout's `og:title` and `og:url`: the product site
       * next door had exactly that, and every deep link unfurled as its home
       * page.
       */
      type: "article",
      siteName: "TessariDB documentation",
      title: found.title,
      description,
      url: `${origin}/${path}`,
    },
    twitter: { card: "summary", title: found.title, description },
  };
}
