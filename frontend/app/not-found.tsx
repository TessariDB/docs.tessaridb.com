import Link from "next/link";

export default function NotFound() {
  return (
    <article className="article">
      <h1>No such page</h1>
      <p className="summary">
        Nothing is stored at that path. It may have been renamed, or the link may
        predate this version of the documentation.
      </p>
      <p>
        Try the search box — it looks inside pages rather than at their titles,
        so a phrase you remember from the text will usually find it. Or start
        from <Link href="/">the beginning</Link>.
      </p>
    </article>
  );
}
