# Governance

Folio is maintained by a single maintainer, [@08820048](https://github.com/08820048).
The project is intentionally small; governance is kept in proportion.

## Decisions

- Small fixes and documentation changes can go straight to a pull request.
- A change to the product shape starts as an issue or a note in `docs/`
  before code. `docs/Folio需求文档.md` is the current specification.
- The maintainer has the final say on scope, dependencies and releases.
- Security reports follow [SECURITY.md](SECURITY.md) and are handled privately
  until a fix is available.

## Review and merge

- CI must pass: formatting, library tests, desktop tests and clippy with
  warnings denied.
- One job per pull request. A PR that changes GPUI pins is reviewed and tested
  separately from feature work.
- The maintainer merges. There is no merge queue or auto-merge.

## Releases

- A release is a version bump in `Cargo.toml` and `packaging/release.env`, a
  changelog entry, and a tag. macOS is built and notarized locally; Windows is
  built by GitHub Actions from a `v*` tag. See `docs/mac-release.md` and
  `docs/windows.md`.
- Release artifacts live in R2, not in GitHub Releases.

## Becoming a maintainer

Sustained, careful contributions and review participation are the path. A
second maintainer is added by invitation and recorded in this file and in
`.github/CODEOWNERS`.

## License and conduct

The license is the [PolyForm Noncommercial License 1.0.0](LICENSE); it is
source-available, not OSI-approved open source, and commercial use needs a
separate agreement. Participation is covered by
[CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).
