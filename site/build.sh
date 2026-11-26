#!/usr/bin/env sh
# Builds the web app into site/dist: the WebAssembly renderer, the scene
# worker, then the pages. Run `npm ci` in site/ once before
set -eu
cd "$(dirname "$0")/.."

# wasm-pack runs wasm-opt -Oz itself, as set in crates/isotrail-web/Cargo.toml
wasm-pack build crates/isotrail-web --target web --profile web --out-dir ../../web/pkg
sh web/build-scene.sh
npm --prefix site run build
