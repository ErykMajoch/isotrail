#!/usr/bin/env sh
# Builds the WebAssembly renderer into web/pkg, and the demo's synthetic
# bundle and posters
set -eu
cd "$(dirname "$0")/.."

# wasm-pack runs wasm-opt -Oz itself, as set in crates/isotrail-web/Cargo.toml
wasm-pack build crates/isotrail-web --target web --profile web --out-dir ../../web/pkg
cargo run --quiet -p isotrail-testkit --example demo_bundle -- web/demo/hike.itb
cargo run --quiet -p isotrail-cli -- render web/demo/hike.itb -o web/demo/hike.png \
  --aspect wide --overlay none > /dev/null
cargo run --quiet -p isotrail-cli -- render web/demo/hike.itb -o web/demo/press.png \
  --size 640x360 --scale 1 --overlay none > /dev/null
