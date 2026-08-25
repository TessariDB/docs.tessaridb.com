import type { Metadata } from "next";
import { notFound } from "next/navigation";
import { connection } from "next/server";

import { Article } from "@/components/Article";
import { page as read } from "@/lib/api";

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
  const found = await read(slug.join("/"));
  if (!found) notFound();
  return <Article page={found} />;
}

export async function generateMetadata({ params }: Asked): Promise<Metadata> {
  const { slug } = await params;
  const found = await read(slug.join("/"));
  if (!found) return { title: "Not found" };
  return {
    title: found.title,
    description: found.summary ?? undefined,
  };
}
