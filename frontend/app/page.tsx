import { notFound } from "next/navigation";
import { connection } from "next/server";

import { Article } from "@/components/Article";
import { page as read } from "@/lib/api";

/**
 * The front door.
 *
 * It is a page in the store like any other, at the slug `index`, so the landing
 * page is edited the same way every other page is — through the API or in
 * `content/` — rather than living in this repository as a component nobody
 * without a checkout can change.
 */
export default async function Home() {
  // Never prerendered — see the layout for why.
  await connection();
  const found = await read("index");
  if (!found) notFound();
  return <Article page={found} />;
}
