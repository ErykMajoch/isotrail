# isotrail

The goal of this project is to turn a recorded hike (GPX file) into a pixel-art diorama: a floating slab of the real terrain, cut out of the world with soil and rock layers on its sides and your route drawn on top.

Early development.

## Usage

```bash
isotrail stats hike.gpx [--json] [--units metric|imperial]
```

Prints distance, elevation gain and loss, elapsed and moving time, start and end times in the local time zone, and the bounding box. `--json` always reports metres and seconds.

```bash
isotrail cache path
isotrail cache clear [--yes]
```

Downloaded elevation and map data is cached on disk, so repeat renders of the same area work offline. The cache lives in `%LOCALAPPDATA%\isotrail\cache` on Windows, `~/.cache/isotrail` on Linux and `~/Library/Caches/isotrail` on macOS. Set `ISOTRAIL_CACHE_DIR` to use another directory.

## Credits

Time zones are looked up offline with [tzf-rs](https://github.com/ringsaturn/tzf-rs), using boundary data from [timezone-boundary-builder](https://github.com/evansiroky/timezone-boundary-builder), © OpenStreetMap contributors, available under the [ODbL](https://opendatacommons.org/licenses/odbl/).
