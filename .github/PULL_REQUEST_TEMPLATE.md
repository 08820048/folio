## Why

<!-- One or two sentences. Not a file list. Link the issue if there is one. -->

## How I checked

- [ ] `cargo fmt --check`
- [ ] `cargo test --locked --no-default-features`
- [ ] `cargo test --locked --features desktop-tests`
- [ ] `cargo clippy --locked --all-targets --features desktop-tests -- -D warnings`
- [ ] Checked on macOS / Windows / Linux (say which)
- [ ] For UI changes, walked the affected flow in a real window

## Notes

<!-- Anything the reviewer should know: a trade-off, a dependency, a follow-up. -->

By opening this pull request I offer the change under the
[PolyForm Noncommercial License 1.0.0](../LICENSE) and agree to the
[Code of Conduct](../CODE_OF_CONDUCT.md).
