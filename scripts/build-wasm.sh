#!/usr/bin/env bash
# Builds packages/wasm into packages/wasm/pkg (JS glue, types, .wasm), which is committed so a
# fresh clone and the web image build without a Rust toolchain (docs/adr/0005).
#   scripts/build-wasm.sh          rebuild and restamp
#   scripts/build-wasm.sh --check  fail if pkg/ was built from different sources
# The stamp is a SHA-256 over everything the binary is built from.
set -euo pipefail
cd "$(dirname "$0")/../packages/wasm"
export PATH="$HOME/.cargo/bin:$PATH"

stamp() {
  cat Cargo.toml Cargo.lock rust-toolchain.toml src/*.rs | shasum -a 256 | cut -d' ' -f1
}

if [[ "${1:-}" == "--check" ]]; then
  if [[ "$(stamp)" != "$(cat pkg/SOURCE_HASH 2>/dev/null)" ]]; then
    echo "packages/wasm/pkg is stale: run scripts/build-wasm.sh and commit the result" >&2
    exit 1
  fi
  echo "✓ wasm package matches its sources"
  exit 0
fi

bindgen_version="$(grep -E '^wasm-bindgen = ' Cargo.toml | grep -oE '[0-9]+\.[0-9]+\.[0-9]+')"
if [[ "$(wasm-bindgen --version 2>/dev/null)" != "wasm-bindgen $bindgen_version" ]]; then
  echo "needs wasm-bindgen-cli $bindgen_version: cargo install wasm-bindgen-cli --version $bindgen_version --locked" >&2
  exit 1
fi

cargo build --release --target wasm32-unknown-unknown
rm -rf pkg
wasm-bindgen --target web --out-dir pkg --out-name lattice_wasm \
  target/wasm32-unknown-unknown/release/lattice_wasm.wasm
rm -f pkg/.gitignore pkg/package.json
stamp > pkg/SOURCE_HASH
echo "✓ packages/wasm/pkg ($(wc -c < pkg/lattice_wasm_bg.wasm | tr -d ' ') bytes of wasm)"
