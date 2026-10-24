# isotrail

The goal of this project is to turn a recorded hike (GPX file) into a pixel-art diorama: a floating slab of the real terrain, cut out of the world with soil and rock layers on its sides and your route drawn on top.

Early development.

## Usage

```bash
isotrail stats hike.gpx|hike.itb [--json] [--units metric|imperial]
```

Prints distance, elevation gain and loss, elapsed and moving time, start and end times in the local time zone, and the bounding box, for a GPX file or a scene bundle made from one. `--json` always reports metres and seconds.

```bash
isotrail bundle hike.gpx -o hike.itb [--debug-dir DIR] [--detail N] [--offline] [--overpass-url URL]
```

Downloads the elevation and OpenStreetMap data around the route, reports how many map features it found (`-v` lists them by type) and works out what covers every grid cell from the map data where it has any and from the slope and height of the ground elsewhere. Lakes and reservoirs are given a level surface. The result is saved with the route, its elevation profile and the hike's statistics as a scene bundle (`.itb`): one compact file that holds everything needed to render the hike, with no further downloads. With `--debug-dir`, it also writes these images to `DIR`, to check the terrain before rendering:

- `heightmap.png`: lowest ground black, highest white
- `hillshade.png`: shaded relief with the route in red
- `landcover.png`: one colour per kind of ground, with the route in red; `landcover-legend.txt` lists the colours and how much of the slab each covers
- `water.png`: levelled lakes white, rivers grey
- `paths.png`: footpaths and tracks white, roads orange, streams blue

The terrain slab is turned to follow the route, so the images are not north-up. `--detail` sets the number of grid cells along the slab's long side (default 256). `--offline` uses only data already in the cache. Map data comes from the public [Overpass API](https://overpass-api.de/). To use another instance, pass `--overpass-url` or set `ISOTRAIL_OVERPASS_URL`. On long routes, where a grid cell is wider than 100 m, footpaths, streams and minor roads are left out because they would be too small to show.

```bash
isotrail cache path
isotrail cache clear [--yes]
```

Downloaded elevation and map data is cached on disk, so repeat renders of the same area work offline. The cache lives in `%LOCALAPPDATA%\isotrail\cache` on Windows, `~/.cache/isotrail` on Linux and `~/Library/Caches/isotrail` on macOS. Set `ISOTRAIL_CACHE_DIR` to use another directory.

## Credits

Time zones are looked up offline with [tzf-rs](https://github.com/ringsaturn/tzf-rs), using boundary data from [timezone-boundary-builder](https://github.com/evansiroky/timezone-boundary-builder), © OpenStreetMap contributors, available under the [ODbL](https://opendatacommons.org/licenses/odbl/).

Elevation data comes from [Terrain Tiles](https://registry.opendata.aws/terrain-tiles/) on AWS Open Data, assembled by [Tilezen joerd](https://github.com/tilezen/joerd) from sources that include SRTM, GMTED2010 and 3DEP (courtesy of the U.S. Geological Survey), ETOPO1 (NOAA), EU-DEM (produced using Copernicus data and information funded by the European Union) and national datasets such as © Environment Agency copyright and/or database right 2015. See the [full attribution list](https://github.com/tilezen/joerd/blob/master/docs/attribution.md).

Map data © [OpenStreetMap contributors](https://www.openstreetmap.org/copyright), available under the [ODbL](https://opendatacommons.org/licenses/odbl/), fetched through the [Overpass API](https://overpass-api.de/).
