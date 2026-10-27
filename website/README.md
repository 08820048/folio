# Folio website

React + Vite + Tailwind CSS v4, components from [coss ui](https://coss.com/ui/docs)
(added through the shadcn CLI with `@coss/style`, built on Base UI). The CLI is
a dev-time `npx shadcn` call, not a dependency; its Tailwind base is vendored
at `src/shadcn-tailwind.css` (MIT) so the production lockfile does not pull in
the CLI's server/MCP stack. The editor
illustration is a labeled interactive mock, not a screenshot or a live editor.
Production: https://folio-website-7j4.pages.dev
Only `dist/` (generated) plus the static files in `public/` are deployed. App
releases remain in R2.

## Pages

Two prerendered HTML entries share one client bundle, which picks the page from
the `data-page` marker on `#root`:

| URL | HTML | Component |
| --- | --- | --- |
| `/` | `index.html` | `src/App.tsx` |
| `/updates` | `updates.html` | `src/components/updates-page.tsx` |

Adding another page means a third entry in `vite.config.ts`'s
`rollupOptions.input` and another item in `scripts/prerender.mjs`'s `pages`
list (with the matching `open` marker), plus the client branch in
`src/main.tsx`.

`src/data/releases.ts` is the changelog's source of truth. Edit it by hand when
a version ships, keeping the newest entry in step with the `--notes` passed to
`scripts/release.sh` — those become the Sparkle appcast. The prerender also
reads its `latestRelease` to fill `softwareVersion` in the home page's JSON-LD.

## Develop

```sh
npm install
npm run dev        # vite dev server
```

`npm run check` validates links and assets in `dist/`; run `npm run build`
first.

## Deploy

```sh
npx wrangler@4.127.1 whoami
npm run deploy     # vite build + wrangler pages deploy dist
```

The project uses Direct Upload. This command publishes production updates;
there is no automatic deployment on Git pushes.

`public/_redirects` sends `/download` to the verified R2 `Folio-latest.dmg` URL.
This route and security headers are applied by Pages, not the local dev server.
Bind a domain in Cloudflare Pages → folio-website → Custom domains.
Update the download platform requirements if future release targets change.
The hero keeps a disabled **Windows · Coming soon** button until a signed
Windows installer ships. CI already uploads an unsigned `.exe` installer to R2
on a `v*` tag; that is not the site download. The source license is PolyForm Noncommercial 1.0.0; do not advertise commercial use as free.

## Design notes

- Palette: the coss neutral system for surfaces; the app icon's green lives
  only in `--brand` / `--primary` (deep `#0c4f41` on light, mint `#8de3bc` on
  dark) and the dark CTA card.
- The editor mock's colors are the real app theme: `sync_appearance` in
  `src/app.rs` for the UI, gpui-component's default theme JSON for syntax, and
  the One-family ANSI colors from `src/terminal_view.rs` for the terminal.
