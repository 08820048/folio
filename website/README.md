# Folio website

React + Vite + Tailwind CSS v4, components from [coss ui](https://coss.com/ui/docs)
(added through the shadcn CLI with `@coss/style`, built on Base UI). The editor
illustration is a labeled interactive mock, not a screenshot or a live editor.
Production: https://folio-website-7j4.pages.dev
Only `dist/` (generated) plus the static files in `public/` are deployed. App
releases remain in R2.

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
The source license is PolyForm Noncommercial 1.0.0; do not advertise commercial use as free.

## Design notes

- Palette: the coss neutral system for surfaces; the app icon's green lives
  only in `--brand` / `--primary` (deep `#0c4f41` on light, mint `#8de3bc` on
  dark) and the dark CTA card.
- The editor mock's colors are the real app theme: `sync_appearance` in
  `src/app.rs` for the UI, gpui-component's default theme JSON for syntax, and
  the One-family ANSI colors from `src/terminal_view.rs` for the terminal.
