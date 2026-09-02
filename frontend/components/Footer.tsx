import Link from "next/link";

import { Mark } from "@/components/icons";
import { version } from "@/lib/site";

/**
 * The bottom of every page.
 *
 * Three things belong here and nothing else does: what this site is, where the
 * public repositories are, and the licence. The licence in particular is not
 * decoration on a database — somebody evaluating one needs to know before they
 * build on it whether their use is free, and burying that costs them a
 * conversation later that neither side wanted.
 *
 * The engine's own repository is deliberately not linked. It is private, and a
 * link that answers `404` reads as a broken site rather than as a closed
 * source; the address to write to is in the licence line instead.
 */
export function Footer() {
  return (
    <footer className="footer">
      <div className="footer-grid">
        <div className="footer-about">
          <Link href="/" className="footer-brand">
            <Mark size={20} />
            TessariDB
          </Link>
          <p>
            Records, graphs, full-text, vectors and geometry in one store, and one
            language over all of them.
          </p>
          <p className="footer-note">
            This site is served out of TessariDB, and the search box is the
            engine&rsquo;s own index.
          </p>
        </div>

        <nav className="footer-links" aria-label="Documentation">
          <h2>Documentation</h2>
          <ul>
            <li>
              <Link href="/start/install">Getting started</Link>
            </li>
            <li>
              <Link href="/query-language/records">TessariQL</Link>
            </li>
            <li>
              <Link href="/overview/engines">The engines</Link>
            </li>
            <li>
              <Link href="/reference/statements">Reference</Link>
            </li>
          </ul>
        </nav>

        <nav className="footer-links" aria-label="Project">
          <h2>Project</h2>
          <ul>
            <li>
              <a href="https://github.com/TessariDB/TessariDB-protocol">
                Protocol specification
              </a>
            </li>
            <li>
              <a href="https://github.com/TessariDB/TessariDB-sdk-rust">Rust client</a>
            </li>
            <li>
              <Link href="/clients/protocol">Writing a client</Link>
            </li>
            <li>
              <Link href="/operations/serving">Running a node</Link>
            </li>
          </ul>
        </nav>

        <nav className="footer-links" aria-label="Contact">
          <h2>Contact</h2>
          <ul>
            <li>
              <a href="mailto:hello@tessaridb.com">hello@tessaridb.com</a>
            </li>
            <li>
              <a href="mailto:licensing@tessaridb.com">licensing@tessaridb.com</a>
            </li>
            <li>
              <a href="mailto:security@tessaridb.com">security@tessaridb.com</a>
            </li>
          </ul>
        </nav>
      </div>

      <div className="footer-legal">
        <p>
          <strong>TessariDB {version}</strong> &middot; &copy; 2026 boogvar
        </p>
        <p>
          Business Source License 1.1. Free in production, including
          commercially. The one restriction is offering TessariDB to third
          parties as a database service, which needs a commercial licence
          &mdash;{" "}
          <a href="mailto:licensing@tessaridb.com">licensing@tessaridb.com</a>.
        </p>
      </div>
    </footer>
  );
}
