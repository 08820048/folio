# Contributing

Thanks for wanting to help. Folio is a small native reader: keep changes
thin, and keep them in the same voice as the code around them.

## License

By opening a pull request you offer your work under the same
[PolyForm Noncommercial License 1.0.0](LICENSE) as the rest of Folio.
Commercial use is not allowed. Do not send code you cannot put under
those terms.

## What belongs here

Reading, a little editing, and the chrome that makes that comfortable.
Not an IDE, not an agent host, not a plugin market. If the change needs
a new pillar, write it down in `docs/` first.

## Before you start

- Read `AGENTS.md` for the UI rules (Lucide icons, no background boxes) and
  `docs/Folio需求文档.md` for what the product is and is not.
- For a change that adds a pillar, write the case in `docs/` first. For a bug,
  open an issue with a reproduction.
- Participation is covered by [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md). Usage
  and build questions go through [SUPPORT.md](SUPPORT.md), and vulnerabilities
  through [SECURITY.md](SECURITY.md) — never a public issue.

## Setup

Rust is pinned in `rust-toolchain.toml`. On macOS, with the Xcode Command
Line Tools:

```sh
cargo run --locked
cargo fmt --check
cargo test --locked --no-default-features
cargo test --locked --features desktop-tests
cargo clippy --locked --all-targets --features desktop-tests -- -D warnings
```

CI runs the same checks on macOS for every push and pull request. Do not
upgrade GPUI or gpui-component in the same PR as a feature: they are pinned
together in `Cargo.lock`, and a blanket `cargo update` would drift the git
sources. Use a targeted `cargo update -p <crate> --precise <version>` and say
why in the PR.

## Pull requests

- One job per PR.
- Say why, not a file list.
- Run the checks above. The PR template repeats the two most important ones.
- The UI is English. Product notes in `docs/` are Chinese. Match the
  file you are in.
- Do not commit secrets, machine paths, `target/`, or `.DS_Store`.
- Icons stay Lucide, with no background box. See `AGENTS.md`.
- If an AI assistant wrote or shaped the change, say so in the PR. You are
  still responsible for understanding and testing it.
- By opening a PR you agree to the [Code of Conduct](CODE_OF_CONDUCT.md).

Open an issue first if the change is large or changes the product shape.
