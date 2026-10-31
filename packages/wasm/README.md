# packages/wasm (phase 2)

Rust compiled to WebAssembly, run in a Web Worker so heavy client work never blocks the main
thread: local filtering of already-loaded Arrow batches, binning for histograms, and LTTB/M4
downsampling for series beyond what the gateway already reduced. Built with `wasm-pack` and
imported by `apps/web` as a worker module. Created when the sheet canvas lands.
