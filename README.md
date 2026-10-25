# isotrail

The goal of this project is to turn a recorded hike (GPX file) into a pixel-art diorama: a floating slab of the real terrain, cut out of the world with soil and rock layers on its sides and your route drawn on top.

Early development.

## Usage

```bash
isotrail stats hike.gpx|hike.itb [--json] [--units metric|imperial]
```

Prints distance, elevation gain and loss, elapsed and moving time, start and end times in the local time zone, and the bounding box, for a GPX file or a scene bundle made from one. `--json` always reports metres and seconds.

```bash
isotrail bundle hike.gpx -o hike.itb [--debug-dir DIR] [--exaggeration X] [--detail N] [--offline] [--overpass-url URL]
```

Downloads the elevation and OpenStreetMap data around the route, reports how many map features it found (`-v` lists them by type) and works out what covers every grid cell from the map data where it has any and from the slope and height of the ground elsewhere. Lakes and reservoirs are given a level surface. The result is saved with the route, its elevation profile and the hike's statistics as a scene bundle (`.itb`): one compact file that holds everything needed to render the hike, with no further downloads. With `--debug-dir`, it also writes these images to `DIR`, to check the terrain before rendering:

- `heightmap.png`: lowest ground black, highest white
- `hillshade.png`: shaded relief with the route in red
- `landcover.png`: one colour per kind of ground, with the route in red; `landcover-legend.txt` lists the colours and how much of the slab each covers
- `water.png`: levelled lakes white, rivers grey
- `paths.png`: footpaths and tracks white, roads orange, streams blue

The terrain slab is turned to follow the route, so the images are not north-up. `--detail` sets the number of grid cells along the slab's long side (default 256). `--offline` uses only data already in the cache. Map data comes from the public [Overpass API](https://overpass-api.de/). To use another instance, pass `--overpass-url` or set `ISOTRAIL_OVERPASS_URL`. On long routes, where a grid cell is wider than 100 m, footpaths, streams and minor roads are left out because they would be too small to show.

```bash
isotrail render hike.gpx|hike.itb [-o hike.png] [--mesher voxel|smooth] [--block-height F] [--exaggeration X] [--palette NAME|FILE] [--detail N] [--offline] [--overpass-url URL]
```

Renders the terrain as a 1080 × 1920 PNG. The slab is seen from above at an angle, looking the way the hike goes, so the start is in front; for loops and out-and-backs, that means towards the halfway point. It is drawn at a third of the final size and enlarged so every pixel stays crisp. `--mesher voxel` (the default) builds the terrain from stepped blocks one grid cell tall; `--block-height` changes their height as a share of the cell size, from 0.25 for fine terraces to 4 for chunky steps. `--mesher smooth` draws a smooth low-poly surface. Heights are exaggerated so the relief reads at diorama scale: by default about 15% of the slab's length, between 1× and 5×. `--exaggeration` sets it by hand, here or when making a bundle. Both meshers cut the slab out of the ground with layered sides: topsoil, dirt, stone and a band of deep stone at the base, with lakes showing in cross-section where they reach the edge. For now, each kind of ground has a flat colour. A late-morning sun from the south-south-east lights it in a few flat bands and casts shadows, and a soft fill from the viewer's side keeps the sides that face away from the sun readable. Dark lines mark the outline of the slab and the places where one surface passes in front of another, and sharp outward folds such as ridges and the slab's rim are picked out in a lighter shade. The route is laid on the terrain as a bold line with a dark border, climbing each block step it crosses; where a ridge hides it, it shows through as a faint dashed line. Where the recording jumps more than 50 m between segments, the line is left open. Flags mark the start and finish, side by side when the hike ends where it began. Every pixel is then snapped to the nearest colour of a pixel-art palette, judged by how alike the colours look rather than by their RGB values, and the sky's gradient is drawn with a fine checker pattern between neighbouring palette colours. `--palette` picks one of the built-in palettes (`apollo`, the default, `resurrect-64` or `aap-64`), loads any Lospec `.hex` file, or with `none` keeps the original colours. A GPX file is first prepared the same way as `isotrail bundle`, with the same options; a scene bundle is rendered as is. Without `-o`, the image is named after the input and written to the current directory.

```bash
isotrail cache path
isotrail cache clear [--yes]
```

Downloaded elevation and map data is cached on disk, so repeat renders of the same area work offline. The cache lives in `%LOCALAPPDATA%\isotrail\cache` on Windows, `~/.cache/isotrail` on Linux and `~/Library/Caches/isotrail` on macOS. Set `ISOTRAIL_CACHE_DIR` to use another directory.

## Development

Rendering runs on the GPU without a window. It uses a real GPU when there is one and falls back to a software renderer otherwise. Set `ISOTRAIL_GPU` to `hardware` or `software` to choose (CI uses `software`: WARP on Windows, Mesa lavapipe on Linux via `mesa-vulkan-drivers`).

Render tests compare their output with golden images in `fixtures/golden/`, allowing up to 0.5% of pixels to differ. After an intended change to the look, regenerate them with `UPDATE_GOLDENS=1 cargo test` and review the new images. A failing comparison writes the actual image and a diff to `target/golden-failures/`.

## Credits

Time zones are looked up offline with [tzf-rs](https://github.com/ringsaturn/tzf-rs), using boundary data from [timezone-boundary-builder](https://github.com/evansiroky/timezone-boundary-builder), © OpenStreetMap contributors, available under the [ODbL](https://opendatacommons.org/licenses/odbl/).

Elevation data comes from [Terrain Tiles](https://registry.opendata.aws/terrain-tiles/) on AWS Open Data, assembled by [Tilezen joerd](https://github.com/tilezen/joerd) from sources that include SRTM, GMTED2010 and 3DEP (courtesy of the U.S. Geological Survey), ETOPO1 (NOAA), EU-DEM (produced using Copernicus data and information funded by the European Union) and national datasets such as © Environment Agency copyright and/or database right 2015. See the [full attribution list](https://github.com/tilezen/joerd/blob/master/docs/attribution.md).

The built-in colour palettes are by AdamCYounis, Kerrie Lake and Adigun A. Polack; see [assets/README.md](assets/README.md).

Map data © [OpenStreetMap contributors](https://www.openstreetmap.org/copyright), available under the [ODbL](https://opendatacommons.org/licenses/odbl/), fetched through the [Overpass API](https://overpass-api.de/).
