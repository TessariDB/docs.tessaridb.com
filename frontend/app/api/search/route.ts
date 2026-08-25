import { NextResponse } from "next/server";

import { search } from "@/lib/api";

/**
 * The search box's route.
 *
 * A proxy, and deliberately nothing more. Its reason to exist is that the
 * documentation API sits on an internal network in the deployment, so the
 * browser cannot reach it — this runs on the server that can.
 *
 * It does no ranking. The order is the store's, out of `search::score` over the
 * one indexed field, and reordering it here would replace a ranking that
 * measures against the whole collection with one that measures against twenty
 * rows.
 */
export const dynamic = "force-dynamic";

export async function GET(request: Request) {
  const asked = new URL(request.url).searchParams.get("q") ?? "";
  if (asked.trim().length < 2) return NextResponse.json([]);

  try {
    return NextResponse.json(await search(asked));
  } catch (fault) {
    // The reader gets no results rather than an error page: a search box that
    // breaks the page it sits in is worse than one that finds nothing, and the
    // detail belongs in the log where somebody can act on it.
    console.error("search failed", fault);
    return NextResponse.json([], { status: 200 });
  }
}
