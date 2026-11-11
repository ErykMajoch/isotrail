# Usage

isotrail has six commands. Each one prints its options with `--help`, for example `isotrail render --help`. The global flag `-v` (repeatable, up to `-vvv`) shows more log output.

- [`render`](#render): render a hike as a PNG, an animated GIF or an MP4 video
- [`bundle`](#bundle): save a hike's scene as a `.itb` file
- [`embed`](#embed): put an interactive diorama of a hike on a web page
- [`stats`](#stats): print a hike's statistics
- [`style-lab`](#style-lab): compare every look on one contact sheet
- [`cache`](#cache): manage downloaded data

The rest of this page also covers [how outputs are written](#writing-files) and [exit codes](#exit-codes).

isotrail reads GPX 1.0 and 1.1 files, with tracks (`<trk>`) or routes (`<rte>`), saved as UTF-8 or as UTF-16 with a byte-order mark. Every pause in a recording starts a new track segment. Where two segments are more than 50 m apart, the route is drawn with a gap instead of a straight jump. Several tracks in one file are joined into one hike, with a note. Files without timestamps still render, but without time-based stats. Files without elevations take their heights from the terrain.

A recording with a few bad points still renders, and each kind of repair is reported once as a warning:

- A point without a valid latitude and longitude is skipped. An elevation or time that can't be read is treated as missing.
- A time before 1990, or weeks away from the points around it and from the rest of the hike, is ignored. Hikes that span more than 31 days get no sun, moon or weather.
- A stray point far from both its neighbours, while they lie close together (faster than 100 m/s to reach, or for points without times a jump 50 times the usual step), is dropped.
- When the times run backwards, the hike starts at its earliest time and ends at its latest, and animations keep their clock from going back.

## render

```text
isotrail render <hike.gpx | hike.itb> [options]
```

Renders the hike as a PNG, as an animated GIF when the output name ends in `.gif`, or as a video when it ends in `.mp4`. A GPX file is first prepared in the same way as [`bundle`](#bundle), with the same data options. A scene bundle is rendered as it is, without the network.

Without `-o`, the image is a PNG named after the input and written to the current directory. When you ask for several aspects, each file gets the aspect added to its name: `-o hike.png --aspect story,wide` writes `hike-story.png` and `hike-wide.png`.

### Image

| Option | Default | Description |
| --- | --- | --- |
| `-o, --output FILE` | `<input>.png` | Where to write the image: `.png` for a still image, `.gif` or `.mp4` for an [animation](#animation) |
| `--aspect LIST` | `story` | One or more of `story` (1080 x 1920), `feed` (1080 x 1350), `square` (1080 x 1080) and `wide` (1920 x 1080), separated by commas |
| `--scale N` | `3` | Output pixels per drawn pixel: `2`, `3`, `5`, `6` or `10`. Lower values give finer detail; higher values give chunkier pixels |
| `--background VALUE` | `sky` | `sky` for a dithered gradient, `transparent` to place the diorama over a photo, or an exact colour such as `#1e2a33` to match a web page |

The scene is drawn at the output size divided by `--scale` (360 x 640 for a story at the default scale), then enlarged without smoothing, so every pixel stays crisp.

<p align="center"><img src="images/wide.png" alt="A wide render, with the title, stats and profile in a column on the left" width="80%"></p>

Stories leave the top 14% and bottom 20% of the image free of text, because Instagram covers those parts with its own interface. In the `wide` format, the title, stats and profile share one column on the left, beside the diorama.

### Terrain and look

| Option | Default | Description |
| --- | --- | --- |
| `--mesher voxel\|smooth` | `voxel` | Stepped blocks, or a smooth low-poly surface |
| `--block-height F` | `1` | Voxel block height as a share of its width, from `0.25` (fine terraces) to `4` (chunky steps). Voxel mesher only: with `--mesher smooth` it does nothing |
| `--exaggeration X` | automatic | How many times taller than life the terrain is drawn, from `0.1` to `100`. By default, the relief is scaled to about 15% of the slab's length, between 1× and 5× |
| `--palette NAME\|FILE` | `apollo` | A built-in palette (`apollo`, `resurrect-64`, `aap-64`), any [Lospec](https://lospec.com/palette-list) `.hex` file, or `none` to keep the unquantised colours |
| `--time HH:MM` | start of the hike | Light the scene as at this local time on the day of the hike, such as `20:30` for a sunset. Needs a track with times |
| `--occlusion X` | `1` | How much hollows, valley floors and the feet of slopes lose of the sky's light, from `0` (none) to `1` |
| `--soft-shadows CELLS` | `1.5` | How wide shadow edges soften, in grid cells, up to `4`; `0` keeps them hard |
| `--god-rays auto\|on\|off` | `auto` | Beams of sunlight through the scene: when the sun breaks through a partly cloudy sky (`auto`), whenever the sun is up (`on`), or never |

The scene is lit by the sun as it stood when the hike started: high and white at midday, low and golden in the evening, deep blue after dusk. The sky changes with it, and after dusk the stars come out and the moon shows in its phase for the date, where it stood in the sky. A bright moon lights the terrain. Tracks without times are lit by a late-morning sun.

The light gives the terrain depth: hollows, valley floors and the feet of steps and cliffs see less of the sky, so they stay darker in the shade, and shadow edges soften into a few steps. When the sun breaks through a partly cloudy sky, it falls in beams: shafts slant across the sky, and the ground they reach is lit brightly while the ground between them lies in the cloud's shade. `--occlusion 0 --soft-shadows 0 --god-rays off` gives the plainer look of earlier versions.

Very tall or very detailed terrain, such as a large `--exaggeration` with a small `--block-height` or `--detail 1024`, can need more graphics memory than the GPU gives one model (256 MB). isotrail then stops before drawing anything and says how much the terrain would need and which options to lower.

The slab is turned to follow the route, and the camera looks the way the hike goes, so the start is in front. For loops and out-and-backs, the camera looks towards the halfway point. See [How it works](how-it-works.md) for what each part of the look does.

### Text and overlays

| Option | Default | Description |
| --- | --- | --- |
| `--overlay LIST` | `title,date,stats,profile` | Which parts to print, or `none` for the diorama alone. The diorama grows into any space left free |
| `--panel frosted\|none` | `frosted` | Print the text on frosted panels, or straight on the scene with a dark outline |
| `--title TEXT` | GPX name | Title to print instead of the hike's name |
| `--title-size large\|small` | `large` | Print the title large, or at the size of the subtitle |
| `--subtitle TEXT\|none` | automatic | Line under the title. By default, a name too long for two lines is split at its first ` - ` or ` \| `. `none` never splits it and shortens a long title with an ellipsis instead |
| `--date TEXT` | hike's date | Text to print instead of the date |
| `--date-format long\|short\|iso` | `long` | `Saturday 14 June 2025`, `14 Jun 2025` or `2025-06-14`. The date is the local date where the hike started |
| `--stats LIST` | `distance,gain,time,high` | Stats to print, in order. See the list below |
| `--units metric\|imperial` | `metric` | Kilometres and metres, or miles and feet |
| `--no-attribution` | off | Leave the data credit off the image, and print it to paste into the caption |

The available stats are:

| Stat | Shows |
| --- | --- |
| `distance` | Total distance |
| `gain`, `loss` | Elevation gained and lost, smoothed to ignore GPS noise |
| `time` | Moving time, without pauses: gaps between points longer than a minute, or than three logging intervals for devices that log less often |
| `elapsed` | Time from start to finish, as `3d 5h` for multi-day trips |
| `high`, `low` | Highest and lowest point |
| `speed`, `pace` | Average speed or pace over the moving time |

Stats are laid out in as few rows as fit. Any stat the recording can't provide (moving time for a file without timestamps, for example) is skipped with a warning.

The data credit under the stats is required by the map and elevation data licences. When you hide it with `--no-attribution` or `--overlay none`, isotrail prints it instead:

```text
Credit for the caption: © OpenStreetMap contributors · Terrain Tiles (AWS)
```

When the weather is drawn, the credit on the image ends in "Open-Meteo.com", and the printed one in "Weather data by Open-Meteo.com".

### Weather

When the bundle holds the weather during the hike, the render shows it as it was at the start (or at `--time`): clouds in the sky and, on a partly cloudy day, their shade drifting over the ground; a grey sky and duller light when it is overcast; rain or snow slanted by the wind; and haze in fog and heavy rain. Thin high cloud shows in the sky but doesn't dim the light. Animations follow the weather hour by hour as the route draws, with the rain, snow and clouds moving from frame to frame. `--no-weather` draws a clear sky instead.

### Season

The landscape matches the time of year, across the whole slab:

- **Spring:** broadleaf woods and bushes come into pale fresh leaf, some of them in white or pink blossom first, conifers show light new growth, meadows are dotted with wildflowers and fields are green with young crops.
- **Summer:** the colours of the map.
- **Autumn:** broadleaf woods and bushes turn yellow, orange and rust, tree by tree, before they drop their leaves; grass turns golden, moorland turns tan with dying bracken, and fields are ploughed brown.
- **Winter:** broadleaf trees stand bare over fallen leaves, grass and fields are a dull winter brown and moorland darkens.

Each cell takes a little more or less of its season's colour than the next, so the ground looks mottled rather than painted. Higher ground runs a little behind in spring and ahead in autumn. South of the equator the seasons are turned round, and in the tropics the trees keep their summer look.

When there was snow on the ground, the slopes above the snowline are white, with snow lying in patches that thin out for 150 m below it, and lakes that froze over in a cold spell are drawn as ice (see [How it works](how-it-works.md#preparing-the-scene)). Snow and ice come with the weather, so `--no-weather` leaves them out too.

`--season spring`, `summer`, `autumn` or `winter` shows the hike in another season. A forced winter covers the whole slab in snow, except for cliffs, and freezes every lake. Animations and the web embed keep one season throughout.

### Animation

A `.gif` or `.mp4` output renders an animation in three parts. It opens on the scene with only the start flag (the first 5%). Then the route draws itself with a marker at its head, while the elevation profile fills in and the stats count up (80%). It ends on the finished hike (15%). The camera starts turned away by `--orbit` and eases round to the same view as the PNG by the time the route reaches the finish.

The light follows the hike's clock: each frame is lit by the sun (and moon) at the time the route's head was walked, so the animation opens under the light at the start and ends under the light at the finish. A stop of 6 hours or more, such as a night on a multi-day trek, becomes a short pause: the head waits while the sun sets, the stars and moon cross the sky and the sun rises again. Each night takes 6% of the drawing time, and all of them together at most 30%. With `--time`, one light holds through the whole animation.

| Option | Default | Description |
| --- | --- | --- |
| `--duration SECONDS` | `10s` | Length of the animation, from `1s` to `60s`, such as `8s` or `2.5` |
| `--fps N` | `30` | Frames per second, up to `50` |
| `--step-fps N` | off | Different images per second, at most `--fps`; fewer gives choppy, game-like motion |
| `--orbit DEG` | `30` | How far the camera turns, from `0` (a still camera) to `360` (a full turn) |
| `--pace distance\|time` | `distance` | Draw the route at an even speed, or follow the recorded times, so slow climbs take longer than quick descents. Stops are cut to a minute, and nights become pauses as described above. Without timestamps, the route draws at an even speed |

GIFs loop forever and use the render's palette, so `--palette none` can't be used with them. They are the same size as the PNG and store only what changes from one frame to the next. A turning camera changes nearly every pixel, so a 10-second story is several megabytes, while `--orbit 0` makes it a fraction of that.

MP4s are H.264 videos without sound, made to post as stories, which can be up to 60 seconds long. They are encoded by [ffmpeg](https://ffmpeg.org/download.html), which must be on your `PATH`. Any palette works, including `none`. A video can't be transparent, so `--background transparent` can't be used with them; choose `sky` or a colour instead.

The animation options only apply to `.gif` and `.mp4` output. PNG renders ignore them, so they can sit in a [config file](configuration.md) that you use for both.

### Data

| Option | Default | Description |
| --- | --- | --- |
| `--detail N` | `256` | Grid cells along the slab's long side, from `8` to `1024` |
| `--offline` | off | Only use data that is already in the cache |
| `--overpass-url URL` | public Overpass API | Another [Overpass](https://overpass-api.de/) endpoint for map data (also `ISOTRAIL_OVERPASS_URL`) |
| `--no-map-data` | off | Build the diorama from the elevation alone, without OpenStreetMap's lakes, woods and paths |
| `--no-weather` | off | Leave the weather out: don't look it up, or draw the weather a bundle holds, along with its snow and ice |
| `--season SEASON` | `auto` | The [season](#season) to show: the hike's own, or `spring`, `summer`, `autumn` or `winter` |
| `--config FILE` | none | Read settings from a TOML file (see [Configuration](configuration.md)) |

A scene bundle already holds its terrain and map data, so `--detail`, `--offline`, `--overpass-url` and `--no-map-data` do nothing with one; isotrail warns when you type them for a bundle.

The public Overpass server is sometimes busy. isotrail then tries again six times over about two minutes, printing a line before each wait. If it still can't get the map data, it stops and suggests trying later, another server with `--overpass-url`, or `--no-map-data`, which makes the diorama from the slope and height of the ground alone: no lakes, woods, fields or paths, and no OpenStreetMap credit. A server that can't be reached at all, such as a mistyped `--overpass-url`, is given up on after a few seconds. Any download that takes longer than 10 seconds prints a line saying it is still waiting.

## bundle

```text
isotrail bundle <hike.gpx> -o <hike.itb> [--debug-dir DIR] [--exaggeration X] [--detail N] [--offline] [--overpass-url URL] [--no-map-data] [--no-weather] [--season SEASON]
```

Downloads the elevation and OpenStreetMap data around the route and works out what covers every grid cell: from the map data where there is some, and from the slope and height of the ground elsewhere. Lakes and reservoirs get a level surface. The result is saved with the route, its elevation profile, the hike's statistics, the sun's and moon's paths across the sky and the hour-by-hour weather during the hike as a **scene bundle**: one compact file (usually under 100 KB) that holds everything a render needs, with no further downloads.

Bundles are useful when you want to try many looks for one hike, to render offline, or to keep a hike's scene as it was even after the map data changes. A bundle made by an older version of isotrail has to be made again from the GPX file, and one made by a newer version needs isotrail updated; isotrail says which. A damaged bundle, or one holding values no render could draw, is refused with a message instead of being drawn. `bundle` reads GPX files only: give a bundle straight to `render`, `embed` or `stats`.

The command reports what it found:

```text
Heightmap 256 x 128 cells of 27.0 m from 6 terrain tiles at zoom 13
Elevation 130 – 524 m, drawn 2.4× taller
Map data: 1840 areas and 2210 lines from OpenStreetMap
Landcover: heath and moorland 52%, grassland 18%, broadleaf forest 11%, water 7%, ...
Water: 3 lakes levelled
Weather: partly cloudy, 11 – 16 °C, 0.4 mm rain (Open-Meteo archive)
Season: spring leaves
Route: 1650 points in 1 piece
Wrote hike.itb (92 KB)
```

`-v` also lists the map features by type.

On long routes, where a grid cell is wider than 100 m, footpaths, streams and minor roads are left out, because they would be too small to show and would make the download much larger.

The weather comes from [Open-Meteo](https://open-meteo.com/) (see [How it works](how-it-works.md#preparing-the-scene)). It needs the track's times, so tracks without them are bundled without weather. If it can't be fetched, for example offline with nothing in the cache, isotrail says why and makes the bundle without it; `--no-weather` skips it on purpose. `isotrail stats --weather` shows what a bundle holds.

The season line names how the trees look, and where there is snow or ice, the heights above which the ground is white (`snow above 650 m`) and the lakes frozen (`lakes frozen above 400 m`). `--season` saves another season in the bundle instead.

### Debug images

With `--debug-dir DIR`, `bundle` also writes images of the prepared terrain, to check it before rendering. Like the render, these are turned to follow the route, so they are not north-up.

| File | Shows |
| --- | --- |
| `heightmap.png` | Elevation, from the lowest ground (black) to the highest (white) |
| `hillshade.png` | Shaded relief, with the route in red |
| `landcover.png` | One colour per kind of ground, with the route in red |
| `landcover-legend.txt` | The landcover colours and how much of the slab each covers |
| `water.png` | Levelled lakes (white) and rivers (grey) |
| `paths.png` | Footpaths and tracks (white), roads (orange) and streams (blue) |

<p align="center">
  <img src="images/debug-hillshade.png" alt="Hillshade debug image" width="45%">
  <img src="images/debug-landcover.png" alt="Landcover debug image" width="45%">
</p>

## embed

```text
isotrail embed <hike.gpx | hike.itb> -o DIR [options]
```

Writes everything a web page needs to show the hike as an interactive diorama with the [`<isotrail-hike>`](../web/README.md) element:

| File | What it is |
| --- | --- |
| `DIR/<name>.itb` | The scene bundle the element draws |
| `DIR/<name>.png` | The poster, shown until the diorama is ready and kept in browsers that can't draw it. It is the same image as `isotrail render --aspect <aspect>`, overlays included |
| `DIR/snippet.html` | The two lines to paste into a page, also printed when the command finishes |
| `DIR/isotrail-embed.js`, `DIR/pkg/` | The web runtime, with `--with-runtime` only |

`<name>` is the input's file name without its extension. A GPX file is first prepared in the same way as [`bundle`](#bundle), with the same data options.

```html
<script type="module" src="/hikes/derwent/isotrail-embed.js"></script>
<isotrail-hike src="/hikes/derwent/derwent-edge.itb" poster="/hikes/derwent/derwent-edge.png" aspect="wide"></isotrail-hike>
```

| Option | Default | Description |
| --- | --- | --- |
| `-o`, `--output DIR` | required | The directory to write to, created if needed |
| `--aspect SHAPE` | `wide` | `story` (9:16), `feed` (4:5), `square` or `wide` (16:9), for the element and its poster |
| `--with-runtime` | off | Also copy the web runtime into `DIR` |
| `--runtime-dir DIR` | the `web` folder next to `isotrail` | Where to copy the runtime from: a folder with `isotrail-embed.js` and `pkg/` |
| `--base-url URL` | none | The address `DIR` will be served at, such as `/hikes/derwent/`. Without it, the snippet uses bare file names, relative to the page |
| `--runtime-url URL` | `isotrail-embed.js` in `DIR` | The address of `isotrail-embed.js`, when one copy serves every hike on the site |
| `--exaggeration X` | automatic | As for [`render`](#terrain-and-look) |

When the input is a bundle that already sits in `DIR` under the name `embed` would give it, as in `isotrail embed site/hike.itb -o site`, it is left as it is and only the poster and snippet are written. If `--season`, `--no-weather` or `--exaggeration` would change it, the command stops instead, so the bundle you started from is never replaced; write to another folder.

The poster also takes the [text and overlay options](#text-and-overlays) of `render`, such as `--title` or `--overlay none`, and the [data options](#data). Settings from config files apply to `render` only.

In Git Bash on Windows, an argument starting with `/` is turned into a Windows path before isotrail sees it, so `--base-url /hikes/derwent/` becomes `C:/Program Files/Git/hikes/derwent/`. isotrail warns when an address looks like that. Run the command as `MSYS_NO_PATHCONV=1 isotrail embed …`, or from PowerShell or Command Prompt.

The release downloads have the runtime in a `web` folder next to the `isotrail` executable. If you built isotrail yourself, build the runtime with `sh web/build.sh` and pass `--runtime-dir web` from the repository. See [`web/README.md`](../web/README.md) for the element's attributes, Hugo and Astro examples, and how to share one runtime between hikes.

## stats

```text
isotrail stats <hike.gpx | hike.itb> [--json] [--units metric|imperial] [--weather [--offline]]
```

Prints a hike's statistics: distance, elevation gain and loss, the elevation range, elapsed and moving time, start and end times in the local time zone and in UTC, the bounding box, and the number of points and segments.

```text
Morning ridge walk

  Distance   12.40 km
  Gain       620 m
  Loss       618 m
  Elevation  210 – 745 m
  Elapsed    4h 10m
  Moving     3h 32m
  Start      2025-06-14 09:05 BST · 2025-06-14 08:05 UTC
  ...
```

`--json` prints the same values as JSON for scripts, always in metres and seconds (`distance_m`, `gain_m`, `moving_s` and so on), whatever `--units` says.

`--weather` adds the weather during the hike: from the bundle, or looked up on [Open-Meteo](https://open-meteo.com/) for a GPX file (`--offline` only uses the cache). A summary comes first, then one row per hour in local time. Temperatures and wind are values at the hour; rain and snow are what fell in the hour before it.

```text
  Weather    light rain (Open-Meteo archive)
  Temp       11 – 14 °C
  Rain       1.2 mm
  Snowfall   none
  Wind       up to 32 km/h WSW
  Credit     Weather data by Open-Meteo.com

  Hour       Temp  Conditions              Cloud l/m/h %    Precip  Wind
  Sat 14 Jun 2025
  09:00   11.2 °C  overcast                     95/40/10         —  25 km/h SW
  10:00   12.0 °C  light rain                   90/60/20    0.8 mm  29 km/h WSW
  ...
```

With `--json`, the bundle's hourly samples are under `weather.samples`, with keys such as `temperature_c`, `cloud_cover_low_pct`, `precipitation_mm`, `snowfall_cm`, `snow_depth_m`, `wind_speed_m_s` and `wind_direction_deg` (where the wind blows from), and the WMO `weather_code` with its `condition`.

## style-lab

```text
isotrail style-lab <hike.gpx | hike.itb> -o DIR [--detail N] [--offline] [--overpass-url URL] [--no-map-data]
```

Renders the hike once for every combination of mesher (`voxel`, `smooth`), pixel scale (3, 5, 6) and built-in palette, as `DIR/<mesher>-<scale>-<palette>.png`. It also writes `DIR/contact-sheet.png`, which lays them all out side by side with a label above each, so you can compare looks before choosing your `render` options.

## cache

```text
isotrail cache path
isotrail cache clear [--yes] [--older-than DAYS]
```

Downloaded elevation tiles, map data and weather are cached on disk, so later renders of the same area are fast and can work offline. Nothing expires on its own: map data stays as it was downloaded until you clear it. `cache path` prints where the cache is, and `cache clear` deletes it after asking (`--yes` skips the question). `--older-than DAYS` only deletes downloads made more than that many days ago, for example `--older-than 90` to pick up newer maps for areas you rendered long ago. Only files that isotrail wrote are deleted; anything else in the folder is left alone and counted in the message.

A cached download that turns out to be damaged is dropped and fetched once more.

| System | Cache directory |
| --- | --- |
| Windows | `%LOCALAPPDATA%\isotrail\cache` |
| Linux | `~/.cache/isotrail` |
| macOS | `~/Library/Caches/isotrail` |

Set `ISOTRAIL_CACHE_DIR` to use another directory.

## Writing files

Every command writes each file under a temporary name beside it, such as `hike.gif.4120.tmp`, and only puts it in place once it is complete. A render that fails or is stopped with Ctrl+C leaves the previous file as it was, and removes its temporary file. Existing files are overwritten without asking.

Before any download or render, isotrail checks where it will write:

- Missing folders are created.
- An output can't be one of the run's own inputs, however the path is spelt: `isotrail bundle walk.gpx -o walk.gpx` is refused, and so is a `--runtime-dir` that is the `embed` output folder.
- An output name can't be an existing folder, and an output folder can't be an existing file.

## Exit codes

| Code | Meaning |
| --- | --- |
| `0` | Success |
| `1` | The command failed: a file couldn't be read or written, data couldn't be downloaded, the render failed |
| `2` | The command line or a config file asks for something isotrail can't do, such as an unknown option, a bad value, options that can't go together or an output that would overwrite an input. Nothing has been downloaded or written |
| `130` | Stopped with Ctrl+C |
