# Dependency audit — 2026-10-27

Scope: `Cargo.lock` and `website/package-lock.json` on `main`, against the
GitHub Dependabot alerts open at the start of the audit.

## Before

42 open alerts: 41 npm, 1 Rust.

| Severity | Count |
| --- | ---: |
| Critical | 4 |
| High | 13 |
| Moderate | 21 |
| Low | 4 |

The npm alerts were all in `website/package-lock.json`; the Rust alert was
`rustls 0.23.38` (GHSA for TLS 1.3 handshake messages across encrypted
boundaries).

## Root causes and fixes

### website: two dependency chains, not the site itself

- **`geist` was unused.** The site uses `@fontsource-variable/geist` and
  `@fontsource-variable/geist-mono`; the `geist` package is a Next.js font
  helper. It pulled `next` and `sharp` into the production lockfile, which
  accounted for the four critical Next.js RCE alerts and the `sharp` advisory.
  Removed.
- **`shadcn` was a runtime dependency for one CSS import.** The site needs
  shadcn's Tailwind base (`@import "shadcn/tailwind.css"`), not its CLI or
  server/MCP stack. The CSS is now vendored at
  `website/src/shadcn-tailwind.css` (MIT, copied from `shadcn@4.21.0`) and the
  CLI is a dev-time `npx shadcn` call. This removed `@modelcontextprotocol/sdk`,
  `hono`, `undici`, `zod`, `fast-uri`, `ip-address`, `proxy-addr`,
  `body-parser`, `brace-expansion` and their dependents from the lockfile.
- **`vite` 7.3.6 → 7.3.7** for the launch-editor advisories.
- **`source-map-js`** was updated to 1.2.2 by `npm audit fix`.

### Rust: rustls

- **`rustls` 0.23.38 → 0.23.45** (and `rustls-webpki` 0.103.13 → 0.103.16)
  with a targeted `cargo update -p rustls --precise 0.23.45`.
- It is reached through `ureq` (Windows updater) and the pinned
  `zed-reqwest` used by `gpui-component-assets`; the update is a patch-level
  change and does not move the GPUI git pin.

## Verification

```sh
cd website
npm install
npm audit --registry=https://registry.npmjs.org   # found 0 vulnerabilities
npm run build                                     # landing + updates prerendered

cd ..
cargo check --locked
cargo test --locked --features desktop-tests
cargo test --locked --no-default-features
cargo clippy --locked --all-targets --features desktop-tests -- -D warnings
```

## Residual risk

- GPUI, `gpui_platform`, `gpui-component` and `alacritty_terminal` are pinned
  by design. `.github/dependabot.yml` ignores them, so a security fix there is
  a deliberate upgrade with a full build/test pass, not an automatic PR.
- Dependabot recounts against the branch after this change is pushed. Any
  remaining alert is expected to be either a recount of a fixed package or a
  new advisory that arrived after this audit.
- `npm audit` uses the public npm registry because the local mirror does not
  implement the advisories endpoint.
