# packages/wasm

Rust compiled to WebAssembly for the web app's compute worker
(`apps/web/src/lib/compute`), so heavy client work never blocks the main thread:

- `lttb` / `lttb_range` — downsample a line series to what the chart can show at its zoom;
- `PointIndex` — a grid over a scatter plot's points: nearest-point hover and brush counts
  over a million points;
- `histogram` — equal-width bins of values already in the browser.

`pkg/` is generated and committed (a fresh clone needs no Rust toolchain to build the web app).
After changing anything here, run `scripts/build-wasm.sh` (needs `wasm-bindgen-cli` at the
version pinned in `Cargo.toml`) and commit `pkg/`; CI fails if `pkg/SOURCE_HASH` doesn't match
the sources. The algorithms are tested natively: `cargo test`.
