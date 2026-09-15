/**
 * The website's copy of the release history. Edit this by hand when a version
 * ships; it is the page's only source of truth.
 *
 * Keep it in step with the appcast that `scripts/release.sh` writes: the
 * `--notes` given on the command line end up in the Sparkle feed, and the
 * newest entry here should say the same thing in the same words. Version and
 * build live in `packaging/release.env`.
 */

export type ReleaseKind = "added" | "improved" | "fixed";

export type ReleaseNote = {
  kind: ReleaseKind;
  text: string;
};

export type Release = {
  /** Marketing version, e.g. "0.1.1" — matches `VERSION` in release.env. */
  version: string;
  /** Sparkle build number (`sparkle:version`), when it is known. */
  build?: number;
  /** Release date, `YYYY-MM-DD`. Rendered without the system locale so the
   *  prerendered HTML and the hydrated page always agree. */
  date: string;
  latest?: boolean;
  summary?: string;
  notes: ReleaseNote[];
};

/** Newest first. */
export const releases: Release[] = [
  {
    version: "0.1.2",
    build: 5,
    date: "2026-09-15",
    latest: true,
    summary: "Run code from the editor, and a few places that used to get in the way.",
    notes: [
      {
        kind: "added",
        text: "Run the current file, test, or main from the gutter or ⌘R, in a dedicated Run tab. While it is running, Play becomes Stop.",
      },
      {
        kind: "added",
        text: "Rainbow brackets and matching, so nested pairs are easier to follow.",
      },
      {
        kind: "added",
        text: "Click a printed URL in the terminal to open it in the browser.",
      },
      {
        kind: "fixed",
        text: "Typing in the in-file search box no longer selects the whole query after every character.",
      },
    ],
  },
  {
    version: "0.1.1",
    build: 4,
    date: "2026-09-14",
    summary: "A quieter second build: fewer places where the app got in the way.",
    notes: [
      {
        kind: "improved",
        text: "Notices match the editor and slide in from the right.",
      },
      {
        kind: "improved",
        text: "The file tree and activity bar are toggled from the window, not from Settings.",
      },
      {
        kind: "fixed",
        text: "Opening a project no longer runs its Git hooks; copies no longer follow dangling links.",
      },
    ],
  },
  {
    version: "0.1.0",
    date: "2026-09-13",
    summary:
      "The first public macOS release: a quiet, native place to read and lightly edit code.",
    notes: [
      {
        kind: "added",
        text: "A native editor built with Rust and GPUI — the interface is drawn directly, with no browser engine and no WebView.",
      },
      {
        kind: "added",
        text: "A multi-project workspace: several folders open in one window, each keeping its own buffers, tabs and expansion state.",
      },
      {
        kind: "added",
        text: "Session restore: projects, tab order, the file on screen and unsaved edits all come back on the next launch.",
      },
      {
        kind: "added",
        text: "Syntax highlighting for 60+ languages through Tree-sitter, with the grammars compiled into the binary.",
      },
      {
        kind: "added",
        text: "Tabbed editing with code folding, bracket pairing, line comments and image preview.",
      },
      {
        kind: "added",
        text: "Multi-cursor editing, including column selection and select-all-occurrences.",
      },
      {
        kind: "added",
        text: "Project-wide find and replace with case, whole-word and regular-expression filters.",
      },
      {
        kind: "added",
        text: "Quick open by file name, and `:123` to jump straight to a line.",
      },
      {
        kind: "added",
        text: "Git changes list, a unified or split diff against HEAD or the main branch, and line-by-line blame.",
      },
      {
        kind: "added",
        text: "A two-pane split view, side by side or stacked.",
      },
      {
        kind: "added",
        text: "A built-in terminal per project, on the same PTY and VT stack Zed uses.",
      },
      {
        kind: "added",
        text: "A settings window, `.editorconfig` support, and light and dark themes that follow the system.",
      },
      {
        kind: "added",
        text: "A notarized DMG, and automatic updates through Sparkle.",
      },
    ],
  },
];

export const latestRelease = releases.find((release) => release.latest) ?? releases[0];

const MONTHS = [
  "January",
  "February",
  "March",
  "April",
  "May",
  "June",
  "July",
  "August",
  "September",
  "October",
  "November",
  "December",
];

/** Deterministic English date, so SSR and hydration never disagree. */
export function formatReleaseDate(iso: string): string {
  const [year, month, day] = iso.split("-").map(Number);
  return `${MONTHS[month - 1]} ${day}, ${year}`;
}
