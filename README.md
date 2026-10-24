# isotrail

The goal of this project is to turn a recorded hike (GPX file) into a pixel-art diorama: a floating slab of the real terrain, cut out of the world with soil and rock layers on its sides and your route drawn on top.

Early development.

## Usage

```bash
isotrail stats hike.gpx [--json] [--units metric|imperial]
```

Prints distance, elevation gain and loss, elapsed and moving time, start and end times in the local time zone, and the bounding box. `--json` always reports metres and seconds.

```bash
isotrail bundle hike.gpx --debug-dir DIR [--detail N] [--offline]
```

Downloads the elevation data around the route and writes `heightmap.png` (lowest ground black, highest white) and `hillshade.png` (shaded relief with the route in red) to `DIR`, to check the terrain before rendering. The terrain slab is turned to follow the route, so the images are not north-up. `--detail` sets the number of grid cells along the slab's long side (default 256). `--offline` uses only data already in the cache.

```bash
isotrail cache path
isotrail cache clear [--yes]
```

Downloaded elevation and map data is cached on disk, so repeat renders of the same area work offline. The cache lives in `%LOCALAPPDATA%\isotrail\cache` on Windows, `~/.cache/isotrail` on Linux and `~/Library/Caches/isotrail` on macOS. Set `ISOTRAIL_CACHE_DIR` to use another directory.

## Credits

Time zones are looked up offline with [tzf-rs](https://github.com/ringsaturn/tzf-rs), using boundary data from [timezone-boundary-builder](https://github.com/evansiroky/timezone-boundary-builder), © OpenStreetMap contributors, available under the [ODbL](https://opendatacommons.org/licenses/odbl/).

Elevation data comes from [Terrain Tiles](https://registry.opendata.aws/terrain-tiles/) on AWS Open Data, assembled by [Tilezen joerd](https://github.com/tilezen/joerd) from sources that include SRTM, GMTED2010 and 3DEP (courtesy of the U.S. Geological Survey), ETOPO1 (NOAA), EU-DEM (produced using Copernicus data and information funded by the European Union) and national datasets such as © Environment Agency copyright and/or database right 2015. See the [full attribution list](https://github.com/tilezen/joerd/blob/master/docs/attribution.md).
