# Support

Folio is a single-maintainer project. There is no paid support and no
response-time guarantee, but questions and reports are welcome.

## Where to go

- **A bug** — open an issue with the **Bug report** template. Include the
  commit or release, the OS, and the steps that reproduce it.
- **A feature or a product question** — open an issue with the **Feature
  request** template. Folio is deliberately small; read
  `docs/Folio需求文档.md` and `CONTRIBUTING.md` first.
- **A security vulnerability** — do **not** open a public issue. Follow
  [SECURITY.md](SECURITY.md).
- **Build or release questions** — `README.md`, `docs/windows.md` and
  `docs/mac-release.md` are the runbooks.
- **What changed** — [CHANGELOG.md](CHANGELOG.md) and
  `docs/开发进度.md`.

## What to include

A useful report has the Folio version (`cargo run --locked` prints it in About),
the operating system and version, the smallest project or file that shows the
problem, and what you expected instead. Logs and screenshots help when the
problem is visual.

Please keep reports about Folio itself. A bug in a project you opened, in a
third-party crate, or in the macOS/Windows toolchain belongs upstream.
