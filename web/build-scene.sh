#!/usr/bin/env sh
# Builds the scene worker's WebAssembly into web/scene/pkg, with the time
# zone data it fetches as a file of its own
set -eu
cd "$(dirname "$0")/.."

# wasm-pack runs wasm-opt -Oz itself, as set in crates/isotrail-scene/Cargo.toml
wasm-pack build crates/isotrail-scene --target web --profile web --out-dir ../../web/scene/pkg

# The same lite.tzb the command line has built in, from the locked tzf-dist
cargo metadata --format-version 1 --locked | node -e '
  const fs = require("node:fs");
  const path = require("node:path");
  const { packages } = JSON.parse(fs.readFileSync(0, "utf8"));
  const dist = packages.find((p) => p.name === "tzf-dist");
  fs.copyFileSync(path.join(path.dirname(dist.manifest_path), "lite.tzb"), "web/scene/pkg/lite.tzb");
'

for file in isotrail_scene_bg.wasm isotrail_scene.js lite.tzb; do
  echo "$file: $(wc -c < "web/scene/pkg/$file") bytes, $(gzip -9c "web/scene/pkg/$file" | wc -c) gzipped"
done
