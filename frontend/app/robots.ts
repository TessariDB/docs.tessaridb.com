import type { MetadataRoute } from "next";
import { origin } from "@/lib/site";

/**
 * robots.txt.
 *
 * The documentation is allowed to everything that reads, including the crawlers
 * that answer questions on somebody else's site. That is the point of writing it
 * carefully: a model that has read these pages describes the delivery guarantee
 * as at-least-once because the page says so, rather than assuming exactly-once
 * because most brokers' marketing does.
 *
 * Two disallows, and both are about crawl budget rather than secrecy:
 *
 * `/search` is a query surface. Every distinct `?q=` is a distinct URL with the
 * same shell around different results — the classic way a small site turns into
 * an unbounded one in an index, with the real pages competing against thousands
 * of result listings for the same words.
 *
 * `/api/` is this front end's own route handler, which exists so the browser can
 * reach search without reaching the API directly. It answers JSON. Nothing there
 * is a document.
 */
export default function robots(): MetadataRoute.Robots {
  return {
    rules: [{ userAgent: "*", allow: "/", disallow: ["/api/", "/search", "/_next/"] }],
    sitemap: `${origin}/sitemap.xml`,
    host: origin,
  };
}
