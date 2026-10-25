# isotrail

The goal of this project is to turn a recorded hike (GPX file) into a pixel-art diorama: a floating slab of the real terrain, cut out of the world with soil and rock layers on its sides and your route drawn on top.

Early development.

## Installation

isotrail is written in Rust. Install the toolchain with [rustup](https://rustup.rs/) (Rust 1.99 or newer), then build and install the `isotrail` command:

```bash
cargo install --git https://github.com/ErykMajoch/isotrail isotrail-cli
```

or, from a clone of this repository, `cargo install --path crates/isotrail-cli`. On Windows the build needs the Visual Studio C++ build tools. Rendering uses the GPU through Vulkan, Metal or Direct3D 12, and falls back to a software renderer when there is no usable GPU, so it also works on servers and in CI.

## Quick start

Export your recorded hike as a GPX file (AllTrails, for example, exports recordings as a *GPX Track*) and run:

```bash
isotrail render hike.gpx
```

The first run downloads elevation and map data for the area, which takes a little while; later runs use the cache. This writes `hike.png`, a 1080 × 1920 image sized for an Instagram story. A few more examples:

```bash
# A story, a feed post and a website banner in one go: hike-story.png, hike-feed.png, hike-wide.png
isotrail render hike.gpx --aspect story,feed,wide

# A diorama with no sky, to place over a photo; credit the data in the caption instead
isotrail render hike.gpx --background transparent --no-attribution

# Prepare the scene once, then render it as often as you like without the network
isotrail bundle hike.gpx -o hike.itb
isotrail render hike.itb --palette aap-64 --stats distance,gain,elapsed
```

Settings you use every time can go in a config file instead of on the command line. isotrail reads `%APPDATA%\isotrail\config.toml` on Windows, `~/.config/isotrail/config.toml` on Linux and `~/Library/Application Support/isotrail/config.toml` on macOS (set `ISOTRAIL_CONFIG` to use another file), and then any file given with `--config`. Settings go in a `[render]` table and are named like the `render` options:

```toml
[render]
aspect = ["story", "square"]
palette = "resurrect-64"
units = "imperial"
date-format = "short"
no-attribution = true
```

Options given on the command line take precedence over the `--config` file, which takes precedence over the user config file.

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
isotrail render hike.gpx|hike.itb [-o hike.png] [--aspect story|feed|square|wide[,…]] [--scale 2|3|5|6|10] [--background sky|transparent|#RRGGBB] [--mesher voxel|smooth] [--block-height F] [--exaggeration X] [--palette NAME|FILE] [--overlay title,date,stats,profile|none] [--panel frosted|none] [--no-attribution] [--title TEXT] [--title-size large|small] [--subtitle TEXT|none] [--date TEXT] [--date-format long|short|iso] [--stats LIST] [--units metric|imperial] [--config FILE] [--detail N] [--offline] [--overpass-url URL]
```

Renders the terrain as a PNG. `--aspect` picks the shape: `story` (1080 × 1920, the default), `feed` (1080 × 1350), `square` (1080 × 1080) or `wide` (1920 × 1080). List several, separated by commas, to render them all at once; each image is then named after the output with the aspect added, as in `hike-story.png`. The slab is seen from above at an angle, looking the way the hike goes, so the start is in front; for loops and out-and-backs, that means towards the halfway point. It is drawn at a third of the final size and enlarged so every pixel stays crisp; `--scale` sets how many image pixels each drawn pixel becomes, from 2 for finer detail to 10 for chunky pixels. `--mesher voxel` (the default) builds the terrain from stepped blocks one grid cell tall; `--block-height` changes their height as a share of the cell size, from 0.25 for fine terraces to 4 for chunky steps. `--mesher smooth` draws a smooth low-poly surface. Heights are exaggerated so the relief reads at diorama scale: by default about 15% of the slab's length, between 1× and 5×. `--exaggeration` sets it by hand, here or when making a bundle. Both meshers cut the slab out of the ground with layered sides: topsoil, dirt, stone and a band of deep stone at the base, with lakes showing in cross-section where they reach the edge. For now, each kind of ground has a flat colour. A late-morning sun from the south-south-east lights it in a few flat bands and casts shadows, and a soft fill from the viewer's side keeps the sides that face away from the sun readable. Dark lines mark the outline of the slab and the places where one surface passes in front of another, and sharp outward folds such as ridges and the slab's rim are picked out in a lighter shade. The route is laid on the terrain as a bold line with a dark border, climbing each block step it crosses; where a ridge hides it, it shows through as a faint dashed line. Where the recording jumps more than 50 m between segments, the line is left open. Flags mark the start and finish, side by side when the hike ends where it began. Every pixel is then snapped to the nearest colour of a pixel-art palette, judged by how alike the colours look rather than by their RGB values, and the sky's gradient is drawn with a fine checker pattern between neighbouring palette colours. `--background transparent` leaves the sky out, for placing the diorama over a photo, and `--background` with a colour such as `#1e2a33` fills it with that exact colour, to match a web page. `--palette` picks one of the built-in palettes (`apollo`, the default, `resurrect-64` or `aap-64`), loads any Lospec `.hex` file, or with `none` keeps the original colours. The hike's name and date are printed in pixel type on a frosted panel near the top, and its distance, elevation gain, moving time and high point on another near the bottom, above an elevation profile of the whole route. A story keeps them clear of the parts of the screen that Instagram covers; the diorama is scaled to fit between them. In the wide format, they share one panel on the left, beside the diorama. Under the panel, a small line credits the map and elevation data, as their licences ask; with `--no-attribution` it is left off and the credit is printed instead, to paste into the post's caption. A name too long for two lines is split at its first ` - ` or ` | ` into a title and a smaller subtitle. `--overlay` picks which of the title, date, stats and profile appear, or `none` for the diorama alone, which also prints the data credit for the caption; the diorama grows into the space left free. `--panel none` drops the panels and outlines the text instead. `--title`, `--subtitle` and `--date` replace the printed text (`--subtitle none` keeps a long name in the title, cut short with an ellipsis), `--title-size small` prints the title at the size of the subtitle, and `--date-format` chooses `long` (Saturday 14 June 2025), `short` (14 Jun 2025) or `iso` (2025-06-14). `--stats` chooses which stats appear and in what order, from `distance`, `gain`, `loss`, `time` (moving time), `elapsed`, `high`, `low`, `speed` and `pace`; they are laid out in as few rows as fit. `--units imperial` shows miles and feet. A GPX file is first prepared the same way as `isotrail bundle`, with the same options; a scene bundle is rendered as is. Without `-o`, the image is named after the input and written to the current directory. `--config` reads settings from a file, as described under [Quick start](#quick-start).

```bash
isotrail style-lab hike.gpx|hike.itb -o DIR [--detail N] [--offline] [--overpass-url URL]
```

Renders the hike once for every combination of mesher (`voxel`, `smooth`), pixel scale (3, 5 or 6 output pixels per drawn pixel) and built-in palette, and writes each as `DIR/<mesher>-<scale>-<palette>.png`. It also writes `DIR/contact-sheet.png`, which lays them all out side by side with a label above each, so you can compare looks before picking `render` options.

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
