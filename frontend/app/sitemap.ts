import type { MetadataRoute } from "next";
import { nav } from "@/lib/api";
import { origin } from "@/lib/site";
import { leaves } from "@/lib/tree";

/**
 * The sitemap, read from the store on request.
 *
 * Not generated at build. The content of this site lives in TessariDB and is
 * edited through the API, so a sitemap baked into the image would list the pages
 * that existed on the day of the last deploy — and would keep listing them after
 * one was renamed, which is a 404 advertised to every crawler that reads it.
 *
 * `force-dynamic` rather than a revalidate window, and the reason is a failure
 * this very build showed: with ISR, Next renders the route once at build time
 * and ships the result in the image. There is no API during a container build,
 * so what it renders is the catch below — and the deployment starts life
 * serving a sitemap holding one URL, until the first revalidation replaces it.
 * A sitemap that is briefly empty is a sitemap a crawler read while it was.
 *
 * Rendering per request costs the store nothing extra: `nav()` is cached for
 * thirty seconds by the fetch layer, so a burst of crawlers shares one read.
 */
export const dynamic = "force-dynamic";

export default async function sitemap(): Promise<MetadataRoute.Sitemap> {
  const now = new Date();
  const home = {
    url: `${origin}/`,
    lastModified: now,
    changeFrequency: "weekly" as const,
    priority: 1,
  };

  // A tree that cannot be read is a sitemap with one entry, not a 500. The
  // alternative — failing the route — takes the sitemap out of the index
  // entirely for as long as the store is unreachable, which outlives the
  // outage.
  let pages: MetadataRoute.Sitemap = [];
  try {
    pages = leaves(await nav()).map((leaf) => ({
      url: `${origin}/${leaf.slug}`,
      lastModified: now,
      changeFrequency: "weekly" as const,
      // Depth is a reasonable proxy for importance in a documentation tree and
      // it is derived rather than maintained: a top-level page outranks one
      // three levels down, and nobody has to remember to say so.
      priority: Math.max(0.4, 0.9 - leaf.trail.length * 0.1),
    }));
  } catch (fault) {
    console.error("the tree could not be read for the sitemap", fault);
  }

  return [home, ...pages];
}
