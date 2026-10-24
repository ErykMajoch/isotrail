# Recorded map data

`overpass-api.de/api/interpreter-c23d5e7ac6be28ee` is the [Overpass API](https://overpass-api.de/) response to the map-data query for the synthetic route in `../gpx/snowdon.gpx` at `--detail 32`, recorded on 2026-10-24. Its 118 m cells are coarse enough that the query leaves out paths, streams and minor roads. The directory layout mirrors the request URL, and the file name ends in a hash of the POST body, so tests can serve it with `RecordedFetcher`. If the query changes, the hash changes too: the failing test names the file it expected, and the response has to be recorded again.

To keep the repository small, the response is trimmed to 27 of its 458 elements: those with a point within roughly 330 m of the middle of the route, plus the one main road. The JSON whitespace is also removed. The elements themselves are unchanged.

The data is © [OpenStreetMap contributors](https://www.openstreetmap.org/copyright) and available under the [Open Database License](https://opendatacommons.org/licenses/odbl/).

`multipolygon.json` is hand-made: made-up features on a 0.001° grid that cover the awkward cases of multipolygon assembly.
