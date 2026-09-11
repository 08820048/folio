# Vendored crates

`gpui-base/` is a copy of the crate of that name from
`longbridge/gpui-component`, taken at the revision the root manifest pins.
It is here because the editor's input — its text, its selections, the element
that draws them — lives in it, and the editor is the one thing this project
needs to change. A `[patch]` in the root manifest points the dependency at
this copy:

```toml
[patch."https://github.com/longbridge/gpui-component"]
gpui-base = { path = "vendor/gpui-base" }
```

`gpui-component` itself still comes from the pinned revision, so exactly one
package in the graph is local. Nothing else about the dependency set changes.

## Why a patch works here

`gpui-base` is not a package published anywhere. It is a path dependency
inside gpui-component's own workspace, which is why a `[patch]` for it looked
like it should not work — the source being replaced is a path, not a registry
or a git URL. It works because the git source the patch names *contains* a
package called `gpui-base`, and a patch replaces the package of that name
within that source. Only the local copy's manifest is different: inside its
own workspace it inherited its dependencies, and outside one it cannot.

## Re-vendoring, which an upgrade requires

1. Copy the crate out of the new revision:

   ```sh
   cp -r ~/.cargo/git/checkouts/gpui-component-*/<rev>/crates/base vendor/gpui-base
   ```

2. Make its manifest standalone. It inherits from its workspace, so:
   - `edition.workspace = true` becomes the edition its workspace declares;
   - every `name.workspace = true` becomes the entry of that name from the
     workspace's `[workspace.dependencies]`, feature list and all;
   - `[lints] workspace = true` is deleted.

   The header comment in the copied manifest marks where its dependencies came
   from; the values in it are the ones to copy.

3. Build and test, with `--locked`:

   ```sh
   cargo build --locked
   cargo test --locked --features desktop-tests
   ```

4. Re-apply this project's changes to the crate. They are a diff against the
   revision that was copied, so `diff -ru` against a fresh copy of that
   revision is the way to see them and the way to carry them forward.
