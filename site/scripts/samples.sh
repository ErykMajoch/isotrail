#!/usr/bin/env sh
# Builds the sample walks' scene files in site/public/samples from their GPX
# files, with the default options. Run it after a change to the scene file
# format. It asks the elevation, map and weather services, so set
# ISOTRAIL_OVERPASS_URL to another server when the usual one is busy
set -eu
cd "$(dirname "$0")/.."

for gpx in public/samples/*.gpx; do
  cargo run --quiet --release -p isotrail-cli -- bundle "$gpx" -o "${gpx%.gpx}.itb"
done
