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

## Setup

Rust is pinned in `rust-toolchain.toml`. On macOS, with the Xcode Command
Line Tools:

```sh
cargo run --locked
cargo test --locked --features desktop-tests
cargo clippy --locked --all-targets --features desktop-tests -- -D warnings
```

Do not upgrade GPUI or gpui-component in the same PR as a feature.

## Pull requests

- One job per PR.
- Say why, not a file list.
- Run the tests above.
- The UI is English. Product notes in `docs/` are Chinese. Match the
  file you are in.
- Do not commit secrets, machine paths, or `target/`.
- Icons stay Lucide, with no background box. See `AGENTS.md`.

Open an issue first if the change is large or changes the product shape.
