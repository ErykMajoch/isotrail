#!/usr/bin/env sh
# Builds the web app's renderer into web/app/pkg: the web runtime with the
# reading of stills and animation frames, which the embed leaves out
set -eu
cd "$(dirname "$0")/.."

wasm-pack build crates/isotrail-web --target web --profile web --out-dir ../../web/app/pkg \
  -- --features export
