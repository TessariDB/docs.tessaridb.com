import type { TreeNode } from "@/lib/api";

/** A page in the tree, with the sections above it. */
export type Leaf = {
  slug: string;
  title: string;
  /** The section titles from the root down, excluding the page itself. */
  trail: { slug: string; title: string }[];
};

/**
 * Every page in the navigation tree, depth-first, in the order it is shown.
 *
 * The sitemap and `llms.txt` both need this, and both need it to come from the
 * store rather than from a list in the repository — the whole design of this
 * site is that content lives in the database, so a page added through the API
 * must appear in the sitemap without anybody redeploying the front end.
 *
 * A section that holds pages is not itself a URL here. Sections are navigation
 * containers; there is no route that renders one, and advertising `/reference`
 * as a page would put a 404 in the sitemap.
 */
export function leaves(nodes: TreeNode[], trail: Leaf["trail"] = []): Leaf[] {
  const out: Leaf[] = [];
  for (const node of nodes) {
    if (node.kind === "page") {
      out.push({ slug: node.slug, title: node.title, trail });
    }
    if (node.children.length > 0) {
      out.push(
        ...leaves(node.children, [...trail, { slug: node.slug, title: node.title }]),
      );
    }
  }
  return out;
}
