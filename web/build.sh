#!/usr/bin/env sh
# Builds the WebAssembly renderer into web/pkg, and the demo's synthetic
# bundle and poster
set -eu
cd "$(dirname "$0")/.."

# wasm-pack runs wasm-opt -Oz itself, as set in crates/isotrail-web/Cargo.toml
wasm-pack build crates/isotrail-web --target web --release --out-dir ../../web/pkg
cargo run --quiet -p isotrail-testkit --example demo_bundle -- web/demo/hike.itb
cargo run --quiet -p isotrail-cli -- render web/demo/hike.itb -o web/demo/hike.png \
  --aspect wide --overlay none > /dev/null
