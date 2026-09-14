import { createHash } from "node:crypto";
import { readFileSync, rmSync, writeFileSync } from "node:fs";

const here = (relative) => new URL(relative, import.meta.url);

const { render, renderUpdates, latestRelease } = await import(
  here("../.prerender/ssr/entry-server.js"),
);

// The structured data travels with the page; its exact bytes get hashed into
// the CSP in dist/_headers, so keep this block the single source of truth.
// `softwareVersion` is filled from the site's release data below.
const structuredData = {
  "@context": "https://schema.org",
  "@graph": [
    {
      "@type": "SoftwareApplication",
      name: "Folio",
      applicationCategory: "DeveloperApplication",
      operatingSystem: "macOS 12+ (Apple silicon)",
      url: "https://folio-website-7j4.pages.dev/",
      softwareVersion: latestRelease.version,
      releaseNotes: "https://folio-website-7j4.pages.dev/updates",
      description:
        "Folio is a light, focused native code editor for reading. Open a local project, read the code, review Git changes, make a few edits. Built with Rust + GPUI for macOS.",
      image: "https://folio-website-7j4.pages.dev/assets/folio.png",
      offers: {
        "@type": "Offer",
        price: "0",
        priceCurrency: "USD",
        description:
          "Free for personal, educational, and other noncommercial use. Not open-source software.",
      },
      featureList:
        "Syntax highlighting for 60+ languages, split editors, code folding, multi-cursor editing, project-wide search, Git diff, line-by-line blame, built-in terminal, session restore",
      author: { "@type": "Person", name: "xuyi" },
    },
    {
      "@type": "FAQPage",
      mainEntity: [
        {
          "@type": "Question",
          name: "What is Folio good for?",
          acceptedAnswer: {
            "@type": "Answer",
            text: "Reading local code, reviewing AI-generated projects, checking Git changes, and editing configs or small bits of logic. It focuses on reading and light editing — no debugger, plugin marketplace, or LSP like a full IDE.",
          },
        },
        {
          "@type": "Question",
          name: "Which Macs are supported?",
          acceptedAnswer: {
            "@type": "Answer",
            text: "Apple silicon (M-series) builds, requiring macOS 12 or newer. Intel Macs, Windows, and Linux have no official builds yet.",
          },
        },
        {
          "@type": "Question",
          name: "Is it free to use?",
          acceptedAnswer: {
            "@type": "Answer",
            text: "Yes, Folio is free — and I hope she stays free forever!",
          },
        },
        {
          "@type": "Question",
          name: "Does it upload my code? Do I need an AI account?",
          acceptedAnswer: {
            "@type": "Answer",
            text: "Folio reads and writes files locally, ships no cloud AI, needs no account, and never uploads your project in order to read or edit it. The macOS app's update check contacts the update service; commands you run in the terminal are your own.",
          },
        },
        {
          "@type": "Question",
          name: "How do I install and update?",
          acceptedAnswer: {
            "@type": "Answer",
            text: "Open the DMG and drag Folio into Applications. Later, use Check for Updates in the app menu.",
          },
        },
      ],
    },
  ],
};

// 1. Render each page into its client shell. The root marker carries the
//    page name the client entry reads, so it must survive the injection.
const pages = [
  {
    file: "../dist/index.html",
    open: '<div id="root">',
    render,
    structuredData: true,
  },
  {
    file: "../dist/updates.html",
    open: '<div id="root" data-page="updates">',
    render: renderUpdates,
    structuredData: false,
  },
];

for (const page of pages) {
  const shell = readFileSync(here(page.file), "utf8");
  if (!shell.includes(page.open)) {
    throw new Error(`${page.file} has no empty #root to inject into`);
  }
  const markup = page.render();
  // A function replacement, so a `$` in the markup is never read as a
  // capture-group reference.
  let html = shell.replace(page.open, () => page.open + markup);
  if (page.structuredData) {
    const script = `<script type="application/ld+json">${JSON.stringify(structuredData)}</script>`;
    html = html.replace("</head>", `  ${script}\n  </head>`);
  }
  writeFileSync(here(page.file), html);
  console.log(
    `prerendered ${page.file.replace("../", "")} (+${markup.length} bytes of markup)`,
  );
}

// 2. Let the home page's inline structured data through the CSP.
const headersPath = here("../dist/_headers");
const hash = createHash("sha256").update(JSON.stringify(structuredData)).digest("base64");
const headers = readFileSync(headersPath, "utf8").replace(
  "script-src 'self'",
  `script-src 'self' 'sha256-${hash}'`,
);
if (!headers.includes(hash)) throw new Error("could not patch script-src in _headers");
writeFileSync(headersPath, headers);

rmSync(here("../.prerender"), { recursive: true, force: true });
console.log("ld+json hashed into the CSP in dist/_headers");
