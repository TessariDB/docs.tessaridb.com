"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { useEffect, useState } from "react";

import type { TreeNode } from "@/lib/api";
import { useNav } from "./Nav";
import { Chevron, Close, named } from "./icons";

/**
 * The left-hand tree.
 *
 * Rendered from the graph the store walked, so the levels here are the levels
 * in the data: a group, a category, a subcategory and anything below are the
 * same component recursing, not three hand-written cases.
 *
 * A client component because it marks the current page, which needs the path,
 * and because on a narrow screen it is a drawer.
 *
 * # Why a drawer rather than the expansion it replaced
 *
 * It used to be a list that appeared *above* the article, pushing it down by a
 * full screen of links — so reaching the first paragraph of each page meant
 * scrolling past the whole table of contents, on every page in turn, and the
 * only way to dismiss it was the button that opened it, by then off-screen.
 * Over the article instead: the article stays where it is, a tap anywhere else
 * closes it, and `Escape` closes it.
 *
 * # Why the sections collapse only once it is open
 *
 * All of them are expanded in the markup, which is what a wide screen wants and
 * what a reader with no JavaScript gets. Collapsing happens in an effect when
 * the drawer opens, so it is an interaction rather than a render — no media
 * query, no hydration mismatch, and the wide-screen column is untouched. The
 * section holding the current page stays open, because a reader who opens the
 * navigation is asking *where am I*, not only *where else could I go*.
 */
export function Tree({ nodes }: { nodes: TreeNode[] }) {
  const { open, setOpen } = useNav();
  const here = usePathname();
  const [folded, setFolded] = useState<ReadonlySet<string>>(new Set());

  useEffect(() => {
    if (!open) return;
    setFolded(
      new Set(
        nodes
          .filter((node) => node.kind !== "page" && !holds(node, here))
          .map((node) => node.slug),
      ),
    );
  }, [open, nodes, here]);

  function fold(slug: string) {
    setFolded((was) => {
      const next = new Set(was);
      if (!next.delete(slug)) next.add(slug);
      return next;
    });
  }

  return (
    <nav
      id="documentation-tree"
      className={open ? "sidebar open" : "sidebar"}
      aria-label="Documentation"
    >
      <div className="drawer-head">
        <span>Contents</span>
        <button
          type="button"
          className="drawer-close"
          onClick={() => setOpen(false)}
          aria-label="Close navigation"
        >
          <Close size={18} />
        </button>
      </div>
      <ul className="tree">
        {nodes.map((node) => (
          <Branch key={node.slug} node={node} depth={0} folded={folded} fold={fold} />
        ))}
      </ul>
    </nav>
  );
}

/** Whether this section, at any depth, contains the page being read. */
function holds(node: TreeNode, here: string): boolean {
  if (node.kind === "page") return `/${node.slug}` === here;
  return node.children.some((child) => holds(child, here));
}

function Branch({
  node,
  depth,
  folded,
  fold,
}: {
  node: TreeNode;
  depth: number;
  folded: ReadonlySet<string>;
  fold: (slug: string) => void;
}) {
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
  const shut = folded.has(node.slug);

  return (
    <li
      className={depth === 0 ? "tree-section" : "tree-section tree-sub"}
      data-folded={shut ? "yes" : undefined}
    >
      <button
        type="button"
        className="tree-label"
        aria-expanded={!shut}
        onClick={() => fold(node.slug)}
      >
        {depth === 0 ? <Icon size={14} /> : null}
        <span>{node.title}</span>
        <Chevron size={14} />
      </button>
      <ul>
        {node.children.map((child) => (
          <Branch key={child.slug} node={child} depth={depth + 1} folded={folded} fold={fold} />
        ))}
      </ul>
    </li>
  );
}
