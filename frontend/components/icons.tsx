/**
 * The icon set, drawn to match the mark.
 *
 * The mark is two rounded squares with a 4px corner radius on a 100 viewBox,
 * stroked at 2.5. Scaled to a 24 box that is a radius of about 1 and a stroke of
 * 1.7, so these are drawn at `stroke-width: 1.7`, `linecap: round`,
 * `linejoin: round`, on a 24 grid — geometric, open, and light enough to sit
 * beside 12px uppercase labels without shouting.
 *
 * Written out rather than taken from an icon package: there are fourteen of
 * them, a package would be a dependency and a licence for shapes that then look
 * like everybody else's, and the whole point of the set is that it looks like
 * ours.
 */

type Props = {
  /** Pixel size. The grid is 24, so anything else is a clean scale. */
  size?: number;
  /** For a decorative icon, leave this off — it is then hidden from readers. */
  label?: string;
};

function Svg({ size = 16, label, children }: Props & { children: React.ReactNode }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.7}
      strokeLinecap="round"
      strokeLinejoin="round"
      role={label ? "img" : undefined}
      aria-label={label}
      aria-hidden={label ? undefined : true}
    >
      {children}
    </svg>
  );
}

/** The mark itself: two offset rounded squares, the front one holding a T. */
export function Mark({ size = 22 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 100 100" role="img" aria-label="TessariDB">
      <rect
        x="8"
        y="8"
        width="58"
        height="58"
        rx="4"
        transform="rotate(-6 37 37)"
        fill="none"
        stroke="var(--brand-bright)"
        strokeWidth="2.5"
        opacity="0.8"
      />
      <path
        fill="var(--brand-bright)"
        opacity="0.42"
        fillRule="evenodd"
        d="M25 21 H75 A4 4 0 0 1 79 25 V75 A4 4 0 0 1 75 79 H25 A4 4 0 0 1 21 75 V25 A4 4 0 0 1 25 21 Z M 45.73 42.42 L 79.00 45.33 L 79.00 56.37 L 69.67 55.55 L 67.62 79.00 L 55.58 79.00 L 57.72 54.51 L 44.77 53.37 Z"
      />
      <g transform="rotate(5 63 63)">
        <path
          fill="var(--brand-bright)"
          fillRule="evenodd"
          d="M38 34 H88 A4 4 0 0 1 92 38 V88 A4 4 0 0 1 88 92 H38 A4 4 0 0 1 34 88 V38 A4 4 0 0 1 38 34 Z M44 44 H82 V55 H69 V82 H57 V55 H44 Z"
        />
      </g>
    </svg>
  );
}

/** A magnifier, squared off at the handle to echo the mark's corners. */
export function Search(props: Props) {
  return (
    <Svg {...props}>
      <circle cx="10.5" cy="10.5" r="6.5" />
      <path d="M15.5 15.5 L20 20" />
    </Svg>
  );
}

/** Overview: a compass rose in a rounded square. */
export function Compass(props: Props) {
  return (
    <Svg {...props}>
      <rect x="3" y="3" width="18" height="18" rx="4" />
      <path d="M15.2 8.8 L13.4 13.4 L8.8 15.2 L10.6 10.6 Z" />
    </Svg>
  );
}

/** The query language: a prompt. */
export function Terminal(props: Props) {
  return (
    <Svg {...props}>
      <rect x="3" y="4" width="18" height="16" rx="4" />
      <path d="M7.5 9.5 L10.5 12 L7.5 14.5" />
      <path d="M13 15.2 H16.5" />
    </Svg>
  );
}

/** A stack of records — the default for a section that named no icon. */
export function Records(props: Props) {
  return (
    <Svg {...props}>
      <rect x="3" y="3" width="18" height="7" rx="3" />
      <rect x="3" y="14" width="18" height="7" rx="3" />
    </Svg>
  );
}

/** Two nodes and the edge between them. */
export function Graph(props: Props) {
  return (
    <Svg {...props}>
      <rect x="3" y="3" width="7" height="7" rx="2.5" />
      <rect x="14" y="14" width="7" height="7" rx="2.5" />
      <path d="M10 6.5 H17.5 V14" />
    </Svg>
  );
}

/** Light theme. */
export function Sun(props: Props) {
  return (
    <Svg {...props}>
      <circle cx="12" cy="12" r="4.2" />
      <path d="M12 2.6 V4.6 M12 19.4 V21.4 M2.6 12 H4.6 M19.4 12 H21.4 M5.4 5.4 L6.8 6.8 M17.2 17.2 L18.6 18.6 M18.6 5.4 L17.2 6.8 M6.8 17.2 L5.4 18.6" />
    </Svg>
  );
}

/** Dark theme. */
export function Moon(props: Props) {
  return (
    <Svg {...props}>
      <path d="M20 14.2 A8.4 8.4 0 0 1 9.8 4 A8.4 8.4 0 1 0 20 14.2 Z" />
    </Svg>
  );
}

/** Getting started: the same frame as the prompt, with a run triangle in it. */
export function Start(props: Props) {
  return (
    <Svg {...props}>
      <rect x="3" y="3" width="18" height="18" rx="4" />
      <path d="M10 8.2 L16 12 L10 15.8 Z" />
    </Svg>
  );
}

/** Keys and files: a box with a lid and a latch. */
export function Box(props: Props) {
  return (
    <Svg {...props}>
      <rect x="3" y="6" width="18" height="14" rx="3" />
      <path d="M3 10.5 H21" />
      <path d="M10.2 14.6 H13.8" />
    </Svg>
  );
}

/** Users and access. */
export function Shield(props: Props) {
  return (
    <Svg {...props}>
      <path d="M12 3 L19.5 6 V12 C19.5 16.3 16.4 19.5 12 21 C7.6 19.5 4.5 16.3 4.5 12 V6 Z" />
    </Svg>
  );
}

/**
 * A running node: the frame, the node inside it, and the four ways in.
 *
 * Not the usual stack of rack units, because `Records` is already two stacked
 * rounded rectangles and at 16px the two would be the same icon.
 */
export function Node(props: Props) {
  return (
    <Svg {...props}>
      <rect x="3" y="3" width="18" height="18" rx="4" />
      <circle cx="12" cy="12" r="2.7" />
      <path d="M12 3 V6.3 M12 17.7 V21 M3 12 H6.3 M17.7 12 H21" />
    </Svg>
  );
}

/** Clients: a request out and an answer back. */
export function Exchange(props: Props) {
  return (
    <Svg {...props}>
      <rect x="3" y="3" width="18" height="18" rx="4" />
      <path d="M7.4 9.6 H16.6 L13.9 6.9" />
      <path d="M16.6 14.4 H7.4 L10.1 17.1" />
    </Svg>
  );
}

/** Reference: a bound volume, spine on the left. */
export function Book(props: Props) {
  return (
    <Svg {...props}>
      <rect x="4" y="3" width="16" height="18" rx="3" />
      <path d="M8.2 3 V21" />
      <path d="M11.6 8.4 H16.6 M11.6 12 H16.6" />
    </Svg>
  );
}

/** Something worth stopping for — used by the unreleased notice. */
export function Alert(props: Props) {
  return (
    <Svg {...props}>
      <rect x="3" y="3" width="18" height="18" rx="4" />
      <path d="M12 7.6 V13" />
      <path d="M12 16.3 V16.4" />
    </Svg>
  );
}

/**
 * The icon a section asked for by name, or a default.
 *
 * A name the set does not have falls back rather than rendering nothing: a
 * missing icon would misalign the label, which is a visible defect for what is
 * a typo in a `_section.toml`.
 */
export function named(icon: string | null): (props: Props) => React.ReactElement {
  switch (icon) {
    case "compass":
      return Compass;
    case "terminal":
      return Terminal;
    case "graph":
      return Graph;
    case "search":
      return Search;
    case "start":
      return Start;
    case "box":
      return Box;
    case "shield":
      return Shield;
    case "node":
      return Node;
    case "exchange":
      return Exchange;
    case "book":
      return Book;
    default:
      return Records;
  }
}

/** The navigation control on a narrow screen: three rules, thumb-sized. */
export function Menu(props: Props) {
  return (
    <Svg {...props}>
      <path d="M4 7 H20" />
      <path d="M4 12 H20" />
      <path d="M4 17 H20" />
    </Svg>
  );
}

/** Dismissing something that opened over the page. */
export function Close(props: Props) {
  return (
    <Svg {...props}>
      <path d="M6 6 L18 18" />
      <path d="M18 6 L6 18" />
    </Svg>
  );
}

/**
 * A section that opens. Drawn pointing right and rotated by the stylesheet when
 * its section is open, so the two states are one shape and cannot disagree.
 */
export function Chevron(props: Props) {
  return (
    <Svg {...props}>
      <path d="M9.5 5 L16.5 12 L9.5 19" />
    </Svg>
  );
}
