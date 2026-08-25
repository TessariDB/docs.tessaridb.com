"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";

import type { TreeNode } from "@/lib/api";
import { named } from "./icons";

/**
 * The left-hand tree.
 *
 * Rendered from the graph the store walked, so the levels here are the levels
 * in the data: a group, a category, a subcategory and anything below are the
 * same component recursing, not three hand-written cases.
 *
 * A client component only because it marks the current page, which needs the
 * path. The data itself is fetched on the server and passed in.
 */
export function Tree({ nodes }: { nodes: TreeNode[] }) {
  return (
    <nav className="sidebar" aria-label="Documentation">
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
