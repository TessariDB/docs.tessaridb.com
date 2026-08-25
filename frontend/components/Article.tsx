import type { Page } from "@/lib/api";
import { Alert } from "./icons";

/**
 * A page and its outline.
 *
 * The HTML arrives rendered from the API, and so do the outline's anchors. This
 * component does not parse Markdown and must not start: the ids in the body and
 * the anchors in the outline agree only because one piece of code produced both.
 */
export function Article({ page }: { page: Page }) {
  return (
    <>
      <article className="article">
        <h1>{page.title}</h1>
        {page.summary ? <p className="summary">{page.summary}</p> : null}

        {page.unreleased ? (
          <div className="notice" role="note">
            <Alert size={17} />
            <div>
              <strong>This describes something the engine does not do yet.</strong>{" "}
              It is written down so the intended shape is on the record, and it
              will not run against a node today.
            </div>
          </div>
        ) : null}

        {/* The source is this repository's own content and pages written by
            users the store lets write — see the API's renderer for what that
            assumption rests on. */}
        <div className="prose" dangerouslySetInnerHTML={{ __html: page.html }} />
      </article>

      <Outline headings={page.outline} />
    </>
  );
}

function Outline({ headings }: { headings: Page["outline"] }) {
  // Depth 1 is the page title's level and is not repeated here; below depth 4
  // the outline stops being one, so it is cut rather than nested indefinitely.
  const shown = headings.filter((heading) => heading.depth >= 2 && heading.depth <= 4);
  if (shown.length === 0) return <aside className="outline" />;

  return (
    <aside className="outline" aria-label="On this page">
      <p className="outline-title">On this page</p>
      <ul>
        {shown.map((heading) => (
          <li key={heading.anchor} className={`depth-${heading.depth}`}>
            <a href={`#${heading.anchor}`}>{heading.text}</a>
          </li>
        ))}
      </ul>
    </aside>
  );
}
