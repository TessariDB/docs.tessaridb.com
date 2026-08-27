import type { Page } from "@/lib/api";
import { origin } from "@/lib/site";

/**
 * JSON-LD for one documentation page.
 *
 * Two nodes, and the second is the one that pays.
 *
 * `TechArticle` says what the page is — reference documentation for software,
 * not a blog post and not a product listing. It carries no fabricated dates:
 * the store records when a page was written, but this front end is not told, and
 * a `datePublished` invented at render time is a machine-readable claim that
 * every page was published the moment it was crawled.
 *
 * `BreadcrumbList` is the one that changes what a reader sees. Without it a
 * result for a deep page shows the raw URL path; with it the section trail shows
 * instead, so somebody searching for a statement sees which part of the
 * documentation the answer lives in before they click. The trail comes from the
 * navigation tree, so it is the same trail the sidebar draws — a hand-built one
 * would be a second source of truth for the same fact.
 *
 * A section is not a URL on this site, so its crumb carries a name and no
 * `item`. That is the schema's own allowance for an intermediate crumb that is
 * not a page, and it is honest: linking `/reference` would advertise a 404.
 */
export function PageData({ page, trail }: { page: Page; trail: { title: string }[] }) {
  const url = `${origin}/${page.slug}`;

  const graph = {
    "@context": "https://schema.org",
    "@graph": [
      {
        "@type": "TechArticle",
        "@id": `${url}#article`,
        headline: page.title,
        ...(page.summary ? { description: page.summary } : {}),
        url,
        inLanguage: "en",
        isPartOf: {
          "@type": "WebSite",
          "@id": `${origin}/#website`,
          url: origin,
        },
        about: {
          "@type": "SoftwareApplication",
          name: "TessariDB",
          applicationCategory: "DeveloperApplication",
        },
        publisher: { "@type": "Organization", name: "TessariDB", url: origin },
      },
      {
        "@type": "BreadcrumbList",
        "@id": `${url}#breadcrumbs`,
        itemListElement: [
          {
            "@type": "ListItem",
            position: 1,
            name: "Documentation",
            item: `${origin}/`,
          },
          ...trail.map((step, index) => ({
            "@type": "ListItem",
            position: index + 2,
            name: step.title,
          })),
          {
            "@type": "ListItem",
            position: trail.length + 2,
            name: page.title,
            item: url,
          },
        ],
      },
    ],
  };

  return (
    <script
      type="application/ld+json"
      // Built here from the store's own strings. A title carrying `</script>`
      // would break out of this tag, so the payload is escaped rather than
      // trusted — `JSON.stringify` does not escape it, and the store does not
      // forbid it.
      dangerouslySetInnerHTML={{
        __html: JSON.stringify(graph).replaceAll("<", "\\u003c"),
      }}
    />
  );
}
