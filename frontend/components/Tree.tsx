"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { useEffect, useState } from "react";

import type { TreeNode } from "@/lib/api";
import { named } from "./icons";

/**
 * The left-hand tree.
 *
 * Rendered from the graph the store walked, so the levels here are the levels
 * in the data: a group, a category, a subcategory and anything below are the
 * same component recursing, not three hand-written cases.
 *
 * A client component because it marks the current page, which needs the path,
 * and because on a narrow screen it collapses. The data itself is fetched on
 * the server and passed in.
 *
 * The collapse is state rather than a `<details>` element because the tree must
 * be a plain always-open list on a wide screen, and a `<details>` is either open
 * or closed for everybody: forcing a closed one visible with CSS depends on
 * overriding the way the browser hides its own children, which is not a
 * guarantee worth resting a navigation on. Here the button is hidden above
 * 860px and the list is unconditionally shown, so the desktop tree does not
 * depend on state at all.
 */
export function Tree({ nodes }: { nodes: TreeNode[] }) {
  const [open, setOpen] = useState(false);
  const here = usePathname();

  // Following a link closes it. Otherwise the reader taps a page and lands
  // behind the same wall of navigation they just tapped through.
  useEffect(() => setOpen(false), [here]);

  return (
    <nav className={open ? "sidebar open" : "sidebar"} aria-label="Documentation">
      <button
        type="button"
        className="tree-toggle"
        aria-expanded={open}
        onClick={() => setOpen((was) => !was)}
      >
        Contents
      </button>
      <ul className="tree">
        {nodes.map((node) => (
          <Branch key={node.slug} node={node} depth={0} />
        ))}
      </ul>
    </nav>
  );
}

function Branch({ node, depth }: { node: TreeNode; depth: number }) {
  const here = usePathname();

  if (node.kind === "page") {
    const href = `/${node.slug}`;
    return (
      <li>
        <Link href={href} aria-current={here === href ? "page" : undefined}>
          {node.title}
        </Link>
      </li>
    );
  }

  const Icon = named(node.icon);
  return (
    <li className={depth === 0 ? "tree-section" : "tree-section tree-sub"}>
      <div className="tree-label">
        {depth === 0 ? <Icon size={14} /> : null}
        {node.title}
      </div>
      <ul>
        {node.children.map((child) => (
          <Branch key={child.slug} node={child} depth={depth + 1} />
        ))}
      </ul>
    </li>
  );
}
