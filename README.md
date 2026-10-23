# isotrail

The goal of this project is to turn a recorded hike (GPX file) into a pixel-art diorama: a floating slab of the real terrain, cut out of the world with soil and rock layers on its sides and your route drawn on top.

Early development.

## Usage

```
isotrail stats hike.gpx [--json] [--units metric|imperial]
```

Prints distance, elevation gain and loss, elapsed and moving time, start and end times in the local time zone, and the bounding box. `--json` always reports metres and seconds.

## Credits

Time zones are looked up offline with [tzf-rs](https://github.com/ringsaturn/tzf-rs), using boundary data from [timezone-boundary-builder](https://github.com/evansiroky/timezone-boundary-builder), © OpenStreetMap contributors, available under the [ODbL](https://opendatacommons.org/licenses/odbl/).
