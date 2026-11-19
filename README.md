# lsdocs-apidoc-rust

Part of [lsdocs](https://github.com/lsdocs), a static-site generator whose
parsers, transformers and renderers are plugins.

`crates/apidoc-rust` (`lsdocs-apidoc-rust`, builtin `apidoc-rust`):
documents Rust crates from rustdoc's JSON output, in the `rs` domain. It
runs `cargo rustdoc` with a nightly toolchain, since the JSON format is
unstable: the plugin names the one that writes the format it reads
(`NIGHTLY` in `src/lib.rs`, and `rustdoc-types` pinned to match). Install
it with `rustup toolchain install nightly-2026-11-14 --profile minimal`
(`mise install` runs this). Its tests run rustdoc on `tests/fixture`.

The plugin's settings and output are described in the lsdocs
documentation (https://lsdocs.github.io, "apidoc-rust").

## Known limitations

- Types in signatures are text, not links to their items (rustdoc's
  JSON has the ids to link them).
- Links to unpublished crates go to docs.rs pages that do not exist
  yet.

## TODO

- [ ] Link types in signatures to their items.
