# Usage

isotrail has six commands. Each one prints its options with `--help`, for example `isotrail render --help`. The global flag `-v` (repeatable, up to `-vvv`) shows more log output, and `-q` (`--quiet`) shows only errors.

- [`render`](#render): render a hike as a PNG, an animated GIF or an MP4 video
- [`bundle`](#bundle): save a hike's scene as a `.itb` file
- [`embed`](#embed): put an interactive diorama of a hike on a web page
- [`stats`](#stats): print a hike's statistics
- [`style-lab`](#style-lab): compare every look on one contact sheet
- [`cache`](#cache): manage downloaded data

The rest of this page also covers [output for scripts](#output-for-scripts), [how outputs are written](#writing-files) and [exit codes](#exit-codes).

The commands that draw or prepare a hike share their options. They are described under `render`, and this table shows which command takes which:

| Options | `render` | `embed` | `bundle` | `style-lab` |
| --- | --- | --- | --- | --- |
| `--aspect` ([image](#image)) | several | one | – | one |
| `--scale` ([image](#image)) | yes | for the poster | – | several |
| `--size`, `--safe-area` ([image](#image)) | yes | for the poster | – | – |
| `--pitch`, `--turn`, `--heading`, `--zoom` ([camera](#camera)) | yes | yes | – | – |
| `--palette` ([look](#terrain-and-look)) | yes | yes | – | several |
| `--mesher`, `--block-height`, `--background`, `--time`, `--occlusion`, `--soft-shadows`, `--god-rays` ([look](#terrain-and-look)) | yes | yes | – | – |
| `--exaggeration` ([look](#terrain-and-look)) | yes | yes | yes | yes |
| [Scene, light and route](#scene-light-and-route) | yes | yes | – | – |
| [Text and overlays](#text-and-overlays) | yes | for the poster | `--title` only | – |
| [Animation](#animation), [encoding](#encoding) | yes | – | – | – |
| [Data](#data) | yes | yes | yes | yes |
| `--config FILE` ([configuration](configuration.md)) | yes | yes | yes | yes |
| `--json` ([output for scripts](#output-for-scripts)) | yes | yes | yes | yes |

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
| `--aspect LIST` | `story` | One or more of `story` (1080 x 1920), `video` (1080 x 1920), `portrait` (1080 x 1440), `feed` (1080 x 1350), `square` (1080 x 1080) and `wide` (1920 x 1080), separated by commas |
| `--size WxH` | none | Any size instead of an aspect, such as `1200x800`, each side from 16 to 8192 pixels. It can't be typed with `--aspect`, and it replaces an aspect from a config file |
| `--safe-area T,R,B,L` | the aspect's | Margins the diorama and text keep clear of, in percent of the image: top, right, bottom and left, or one number for all four, each up to 45 |
| `--scale N` | `3` | Output pixels per drawn pixel, any whole number from `1` to `16` that divides both sides of the image (`2`, `3`, `5`, `6` and `10` divide every aspect). Lower values give finer detail; higher values give chunkier pixels |
| `--background VALUE` | `sky` | `sky` for a dithered gradient, `transparent` to place the diorama over a photo, or an exact colour such as `#1e2a33` to match a web page |

The scene is drawn at the output size divided by `--scale` (360 x 640 for a story at the default scale), then enlarged without smoothing, so every pixel stays crisp.

<p align="center"><img src="images/wide.png" alt="A wide render, with the title, stats and profile in a column on the left" width="80%"></p>

Each aspect has a safe area that the diorama and text stay inside. Stories leave the top 14% and bottom 20% of the image free, because social apps cover those parts with their own header and reply bar. `video` has the same shape as `story` but keeps the top 14%, the bottom 35% and 6% at each side clear, for short-video feeds that lay a caption and a column of buttons over the picture. `portrait` is a tall 3:4 post. The other aspects, and `--size`, keep 5% clear on every side. `--safe-area` replaces the margins of whichever shape you render.

Images wider than they are tall put the title, stats and profile in one column on the left, beside the diorama; the others put the title at the top and the stats at the bottom. [`--layout`](#text-and-overlays) chooses either for any shape. An MP4 needs an even number of pixels on each side, which isotrail checks before it starts.

### Camera

| Option | Default | Description |
| --- | --- | --- |
| `--pitch DEG` | `30` | How far the camera looks down, from `10` (almost level) to `80` (almost straight down) |
| `--turn DEG` | `0` | Moves the camera this many degrees anticlockwise round the slab, as seen from above, from `-360` to `360` |
| `--heading DEG` | along the hike | The compass bearing the camera looks towards, from `0` (north) through `90` (east) to `360`. `--turn` is added to it |
| `--zoom X` | `1` | The diorama's size, from `0.5` to `2` times the size that fits the safe area. Above `1`, its edges are cut off |

By default the camera looks along the direction of the hike, turned a little so the trail runs up and to the right with its start in front. Animations use the same view: the orbit settles on it, so the last frames match the still.

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

### Scene, light and route

These switch parts of the scene on and off, change the light and style the route. Options that take `on` or `off` take `true` or `false` in a [config file](configuration.md) too.

| Option | Default | Description |
| --- | --- | --- |
| `--prop-density X` | `1` | How many trees, bushes and boulders stand on the terrain, as a share of the usual number, from `0` (none) to `2` |
| `--paths on|off` | `on` | Paths and roads from the map, drawn as lighter ground. Off, the ground under them takes its own colour, and trees may grow there |
| `--streams on|off` | `on` | Streams from the map. Rivers and lakes stay |
| `--strata on|off` | `on` | Soil and rock layers on the slab's cut faces. Off, the faces are plain stone, with any lake or sea still showing at the edge |
| `--base-blocks N` | `6` | How far the slab's base sits below the lowest ground, in blocks, from `1` to `24` |
| `--outlines X` | `1` | How strongly edges are outlined, from `0` (none) to `2` |
| `--shadows on|off` | `on` | Shadows cast by the terrain and props |
| `--night-sky on|off` | `on` | Stars and the moon after dusk. Moonlight still lights the terrain |
| `--weather-effects LIST|none` | `clouds,shade,rain,snow,haze` | Which of the weather's effects to draw: `clouds` in the sky (with the dimmer, greyer light under them), `shade` from passing clouds on the ground, `rain`, `snow` and `haze` (with fog). Without `clouds`, `--god-rays auto` sees a clear sky |
| `--weather SKY` | the recorded weather | Draw `clear`, `cloudy`, `overcast`, `rain`, `snow` or `fog` instead, with the recorded wind. It holds through a whole animation, and the data credit stays as the bundle has it |
| `--sun AZIMUTH,ELEVATION` | from the hike's time | Set the sun by hand: its compass bearing (`0` to `360`) and its elevation above the horizon (`-90` to `90`), such as `250,8` for a low evening light from the west-south-west. It holds through a whole animation, and can't be typed with `--time` |
| `--route-colour #RRGGBB` | `#d64834` | The route's colour; the profile's walked part and the web embed's progress line follow it. `--route-color` works too |
| `--route-width PX` | `2` | The route's width in drawn pixels, from `1` to `4` |
| `--route-hidden dashed|dim|solid|off` | `dashed` | How the parts of the route behind ridges show: faded and dashed, faded, like the rest of the route, or not at all |
| `--markers LIST|none` | `start,finish,head` | Which markers to draw: the `start` and `finish` flags and, in animations, the route's `head` |

Like the palette, colours are snapped to the nearest one in the palette, so an unusual `--route-colour` may come out a little different. `--weather-effects none --shadows off --outlines 0 --strata off --prop-density 0` gives a bare, schematic slab.

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
| `--layout auto|stacked|column` | `auto` | Where the text goes: `auto` puts it in one column on the left of images wider than they are tall, and the title on top with the stats at the bottom otherwise. `stacked` and `column` choose one for every shape |
| `--credit-extra TEXT` | none | A line of your own under the data credit, such as your name or site. It goes with the credit when there is no room for it, and isn't printed for the caption |

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

The text is printed in a pixel font with Latin letters and their accents (Polish, Czech, German, French and so on). Characters it doesn't have, such as emoji, are left out with a warning.

At the coarser scales (`6` and `10`) the frame has fewer pixels, and the overlays may not fit. They then give way, one step at a time and only as far as needed, with a warning that names each step:

1. a long date that doesn't fit its line is printed short (`14 Jun 2025`);
2. while the panels leave the diorama less than a sixth of the frame's height, the title is printed small, then without its subtitle, the stats move closer together, the profile is left out, and then the data credit, which is printed for the caption instead;
3. finally, stats are left out from the end of the list, keeping at least one.

If even that doesn't fit, nothing is rendered and the command exits with code 2: choose a lower `--scale`, fewer `--stats` or fewer `--overlay` parts. The default `--scale 3` always has room for everything.

The data credit under the stats is required by the map and elevation data licences. When you hide it with `--no-attribution` or `--overlay none`, or there is no room for it, isotrail prints it instead:

```text
Credit for the caption: © OpenStreetMap contributors · Terrain Tiles (AWS)
```

When the weather is drawn, the credit on the image ends in "Open-Meteo.com", and the printed one in "Weather data by Open-Meteo.com".

### Weather

When the bundle holds the weather during the hike, the render shows it as it was at the start (or at `--time`): clouds in the sky and, on a partly cloudy day, their shade drifting over the ground; a grey sky and duller light when it is overcast; rain or snow slanted by the wind; and haze in fog and heavy rain. Thin high cloud shows in the sky but doesn't dim the light. Animations follow the weather hour by hour as the route draws, with the rain, snow and clouds moving from frame to frame. `--no-weather` draws a clear sky instead, `--weather-effects` leaves out single effects, and `--weather` draws a sky of your choice (see [Scene, light and route](#scene-light-and-route)).

### Season

The landscape matches the time of year, across the whole slab:

- **Spring:** broadleaf woods and bushes come into pale fresh leaf, some of them in white or pink blossom first, conifers show light new growth, meadows are dotted with wildflowers and fields are green with young crops.
- **Summer:** the colours of the map.
- **Autumn:** broadleaf woods and bushes turn yellow, orange and rust, tree by tree, before they drop their leaves; grass turns golden, moorland turns tan with dying bracken, and fields are ploughed brown.
- **Winter:** broadleaf trees stand bare over fallen leaves, grass and fields are a dull winter brown and moorland darkens.

Each cell takes a little more or less of its season's colour than the next, so the ground looks mottled rather than painted. Higher ground runs a little behind in spring and ahead in autumn. South of the equator the seasons are turned round, and in the tropics the trees keep their summer look.

When there was snow on the ground, the slopes above the snowline are white, with snow lying in patches that thin out for 150 m below it, and lakes that froze over in a cold spell are drawn as ice (see [How it works](how-it-works.md#preparing-the-scene)). Snow and ice come with the weather, so `--no-weather` leaves them out too.

`--season spring`, `summer`, `autumn` or `winter` shows the hike in another season. A forced winter covers the whole slab in snow, except for cliffs, and freezes every lake; the sea stays open. Animations and the web embed keep one season throughout.

### Animation

A `.gif` or `.mp4` output renders an animation in three parts. It opens on the scene with only the start flag (the first 5%, or `--intro` seconds). Then the route draws itself with a marker at its head, while the elevation profile fills in and the stats count up (the time left over). It ends on the finished hike (15%, or `--hold` seconds), however short the clip or low its `--step-fps`. The camera starts turned away by `--orbit` and eases round to the same view as the PNG by the time the route reaches the finish.

With `--loop-back`, a fourth part follows the hold (15% of the clip, taken from the drawing): the route undraws back to the start, the stats count down and the camera turns back to where it began, so a looping GIF or video runs on without a jump.

The light follows the hike's clock: each frame is lit by the sun (and moon) at the time the route's head was walked, so the animation opens under the light at the start and ends under the light at the finish. A stop of 6 hours or more, such as a night on a multi-day trek, becomes a short pause: the head waits while the sun sets, the stars and moon cross the sky and the sun rises again. Each night takes 6% of the drawing time, and all of them together at most 30%. With `--time`, one light holds through the whole animation.

| Option | Default | Description |
| --- | --- | --- |
| `--duration SECONDS` | `10s` | Length of the animation, such as `8s` or `2.5`: from `1s` to `60s` for a GIF, up to `180s` for an MP4 |
| `--intro SECONDS` | 5% of the clip | Time on the scene before the route starts to draw; `0` starts at once |
| `--hold SECONDS` | 15% of the clip | Time on the finished hike. The intro, hold and rewind must leave at least half a second to draw the route |
| `--loop-back` | off | Undraw the route and turn the camera back after the hold, for a seamless loop |
| `--fps N` | `30` | Frames per second: up to `50` for a GIF, `60` for an MP4 |
| `--step-fps N` | off | Different images per second, at most `--fps`; fewer gives choppy, game-like motion |
| `--orbit DEG` | `30` | How far the camera turns, from `0` (a still camera) to `360` (a full turn); a negative angle turns the other way |
| `--pace distance\|time` | `distance` | Draw the route at an even speed, or follow the recorded times, so slow climbs take longer than quick descents. Stops are cut to a minute, and nights become pauses as described above. Without timestamps, the route draws at an even speed |

GIFs use the render's palette, so `--palette none` can't be used with them. By default they loop forever and are the same size as the PNG, and they store only what changes from one frame to the next. A turning camera changes nearly every pixel, so a 10-second story is several megabytes, while `--orbit 0` makes it a fraction of that, and so does `--gif-scale 1`.

MP4s are H.264 videos without sound, for stories, posts and short videos. They are encoded at level 4.0 (4.2 above 30 frames a second) and tagged as BT.709 colour, so they also play as they are on older phones and in a web page's `<video>`. They are encoded by [ffmpeg](https://ffmpeg.org/download.html), which must be on your `PATH` unless you name it with `--ffmpeg`. Any palette works, including `none`. A video can't be transparent, so `--background transparent` can't be used with them; choose `sky` or a colour instead.

The animation options only apply to `.gif` and `.mp4` output. PNG renders ignore them, so they can sit in a [config file](configuration.md) that you use for both.

### Encoding

| Option | Default | Description |
| --- | --- | --- |
| `--gif-scale N` | `--scale` | Output pixels per drawn pixel in a GIF, from `1` up to `--scale`. `--gif-scale 1` writes the drawn size (360 × 640 for a story), about a quarter of the bytes, for a web page that enlarges it with CSS `image-rendering: pixelated` |
| `--loop forever\|once\|N` | `forever` | How many times a GIF plays. `once` writes no loop at all; `N` plays it `N` times in all |
| `--crf N` | `18` | MP4 quality, from `0` to `51`: lower is better and larger. `0` is lossless, which many players can't play |
| `--preset NAME` | x264's own (`medium`) | How hard the MP4 encoder works: `ultrafast`, `superfast`, `veryfast`, `faster`, `fast`, `medium`, `slow`, `slower` or `veryslow`. Slower presets make smaller files at the same quality |
| `--ffmpeg PATH` | the `ffmpeg` on `PATH` | The ffmpeg program to make MP4s with (also `ISOTRAIL_FFMPEG`) |

The GIF options do nothing for an MP4 and the MP4 options nothing for a GIF; isotrail warns when you type one that the output doesn't use.

### Data

| Option | Default | Description |
| --- | --- | --- |
| `--detail N` | `256` | Grid cells along the slab's long side, from `8` to `1024` |
| `--offline` | off | Only use data that is already in the cache |
| `--overpass-url URL` | public Overpass API | Another [Overpass](https://overpass-api.de/) endpoint for map data (also `ISOTRAIL_OVERPASS_URL`) |
| `--terrain-url URL` | AWS Terrain Tiles | Elevation tiles in the terrarium format from another server, as an address with `{z}`, `{x}` and `{y}` (also `ISOTRAIL_TERRAIN_URL`) |
| `--weather-url URL` | Open-Meteo | Another server with [Open-Meteo](https://open-meteo.com/)'s API, such as one you host yourself; isotrail adds `/v1/archive` and `/v1/forecast` (also `ISOTRAIL_WEATHER_URL`) |
| `--cache-dir DIR` | see [cache](#cache) | The folder for the cache of downloads (also `ISOTRAIL_CACHE_DIR`) |
| `--no-map-data` | off | Build the diorama from the elevation alone, without OpenStreetMap's lakes, woods and paths |
| `--padding P[,M]` | `15,500` | Ground added round the route on each side: `P` percent of the route's length and width, and at least `M` metres |
| `--footprint route\|north` | `route` | Which way the slab's sides run: along the route's main direction, or north-south and east-west |
| `--no-weather` | off | Leave the weather out: don't look it up, or draw the weather a bundle holds, along with its snow and ice |
| `--season SEASON` | `auto` | The [season](#season) to show: the hike's own, or `spring`, `summer`, `autumn` or `winter` |

`--config FILE` reads settings from a TOML file; see [Configuration](configuration.md).

A scene bundle already holds its terrain and map data, so `--detail`, `--offline`, `--overpass-url`, `--terrain-url`, `--weather-url`, `--cache-dir`, `--no-map-data`, `--padding` and `--footprint` do nothing with one; isotrail warns when you type them for a bundle.

With an Open-Meteo API key in `ISOTRAIL_OPEN_METEO_KEY`, the weather comes from Open-Meteo's servers for customers (or from `--weather-url` with the key added). The key is only read from the environment, never from a flag or a config file, and it is left out of the cache, the bundle and every message.

`--terrain-url` is meant for mirrors of the same elevation data, so the drawn credit still names Terrain Tiles. If your server has other data, credit it yourself, for example with `--credit-extra`. A key in the address (`key=`, `apikey=`, `token=`…) is masked in the bundle and in messages.

The public Overpass server is sometimes busy. isotrail then tries again six times over about two minutes, printing a line before each wait. If it still can't get the map data, it stops and suggests trying later, another server with `--overpass-url`, or `--no-map-data`, which makes the diorama from the slope and height of the ground alone: no lakes, woods, fields or paths, and no OpenStreetMap credit. A server that can't be reached at all, such as a mistyped `--overpass-url`, is given up on after a few seconds. Any download that takes longer than 10 seconds prints a line saying it is still waiting.

## bundle

```text
isotrail bundle <hike.gpx> -o <hike.itb> [--debug-dir DIR] [--title TEXT] [--exaggeration X] [--config FILE] [--json] [data options]
```

Downloads the elevation and OpenStreetMap data around the route and works out what covers every grid cell: from the map data where there is some, and from the slope and height of the ground elsewhere. Lakes and reservoirs get a level surface, and the sea lies flat at sea level. The result is saved with the route, its elevation profile, the hike's statistics, the sun's and moon's paths across the sky and the hour-by-hour weather during the hike as a **scene bundle**: one compact file (usually under 100 KB) that holds everything a render needs, with no further downloads.

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

On a coast, the water line names the sea too (`Water: the sea and 2 lakes levelled`). `-v` also lists the map features by type. `--title TEXT` saves another title in the bundle instead of the GPX file's name; renders and the web embed then show it.

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
| `water.png` | Levelled lakes and the sea (white) and rivers (grey) |
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
| `DIR/<name>.png` | The poster, shown until the diorama is ready and kept in browsers that can't draw it. It is the same image as `isotrail render --aspect <aspect>` with the same options, overlays included |
| `DIR/snippet.html` | The two lines to paste into a page, also printed when the command finishes |
| `DIR/<palette>.hex` | A copy of the palette file, with `--palette FILE` only |
| `DIR/isotrail-embed.js`, `DIR/pkg/` | The web runtime, with `--with-runtime` only |

`<name>` is the input's file name without its extension. A GPX file is first prepared in the same way as [`bundle`](#bundle), with the same data options.

```html
<script type="module" src="/hikes/derwent/isotrail-embed.js?v=1.3.1"></script>
<isotrail-hike src="/hikes/derwent/derwent-edge.itb" poster="/hikes/derwent/derwent-edge.png" aspect="wide" style="display:block;aspect-ratio:16/9"><img src="/hikes/derwent/derwent-edge.png" alt="Diorama of Derwent Edge" style="display:block;width:100%"></isotrail-hike>
```

The image inside the element shows the poster, at the element's size, on pages where the script can't run. `?v=` carries the isotrail version, so browsers load the new runtime after an update instead of a cached old one; `--runtime-url` gets it too unless that address already has a query. Once the diorama is running, it shows the same data credit as the stills along its bottom edge. A page that hides it with the element's `credit="none"` must credit the data itself; `--no-attribution` prints the text.

| Option | Default | Description |
| --- | --- | --- |
| `-o`, `--output DIR` | required | The directory to write to, created if needed |
| `--aspect SHAPE` | `wide` | `story` or `video` (9:16), `portrait` (3:4), `feed` (4:5), `square` or `wide` (16:9), for the element and its poster |
| `--size WxH` | none | The poster's size instead of an aspect; the element takes its shape, as in `aspect="3:2"` for `1200x800` |
| `--safe-area T,R,B,L` | the aspect's | The poster's margins, as for [`render`](#image) |
| `--scale N` | `3` | The poster's pixel scale, as for [`render`](#image). The live diorama picks its own to suit the page |
| `--with-runtime` | off | Also copy the web runtime into `DIR` |
| `--runtime-dir DIR` | the `web` folder next to `isotrail` | Where to copy the runtime from: a folder with `isotrail-embed.js` and `pkg/` |
| `--base-url URL` | none | The address `DIR` will be served at, such as `/hikes/derwent/`. Without it, the snippet uses bare file names, relative to the page |
| `--runtime-url URL` | `isotrail-embed.js` in `DIR` | The address of `isotrail-embed.js`, when one copy serves every hike on the site |
| `--exaggeration X` | automatic | As for [`render`](#terrain-and-look) |

`embed` also takes `render`'s [terrain and look options](#terrain-and-look), [scene, light and route options](#scene-light-and-route), [camera options](#camera), [text and overlay options](#text-and-overlays) and [data options](#data), and reads [config files](configuration.md). The poster is drawn with all of them. The look, scene, light and route options reach the live diorama too: each one that isn't the default becomes an attribute of the element in the snippet, as in `--palette resurrect-64 --mesher smooth`:

```html
<isotrail-hike src="derwent-edge.itb" poster="derwent-edge.png" aspect="wide" palette="resurrect-64" mesher="smooth" …>
```

The camera options become the `turn`, `pitch` and `zoom` attributes, which set the view the diorama starts from and returns to when it is reset; a `--heading` is written as the `turn` that faces the same way. With `--zoom` above 1 the diorama starts zoomed in, and below 1 it leaves more room round the slab. A palette file is copied into `DIR` and the `palette` attribute points at the copy. With `--time` or `--sun`, the diorama keeps that light while the route plays, as an animation does. `--credit-extra` becomes the `credit-extra` attribute, and the live diorama shows the line under its data credit; `--layout` only shapes the poster. `--title` is saved in the bundle, so the live diorama is labelled with it as well as the poster.

When the input is a bundle that already sits in `DIR` under the name `embed` would give it, as in `isotrail embed site/hike.itb -o site`, it is left as it is and only the poster and snippet are written. If `--title`, `--season`, `--no-weather` or `--exaggeration` would change it, the command stops instead, so the bundle you started from is never replaced; write to another folder.

In Git Bash on Windows, an argument starting with `/` is turned into a Windows path before isotrail sees it, so `--base-url /hikes/derwent/` becomes `C:/Program Files/Git/hikes/derwent/`. isotrail warns when an address looks like that. Run the command as `MSYS_NO_PATHCONV=1 isotrail embed …`, or from PowerShell or Command Prompt.

The release downloads have the runtime in a `web` folder next to the `isotrail` executable. If you built isotrail yourself, build the runtime with `sh web/build.sh` and pass `--runtime-dir web` from the repository. See [`web/README.md`](../web/README.md) for the element's attributes, Hugo and Astro examples, and how to share one runtime between hikes.

## stats

```text
isotrail stats <hike.gpx | hike.itb> [--json] [--units metric|imperial] [--weather [--offline]] [--weather-url URL] [--cache-dir DIR]
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
isotrail style-lab <hike.gpx | hike.itb> -o DIR [--aspect SHAPE] [--scale LIST] [--palette LIST] [--exaggeration X] [--config FILE] [--json] [data options]
```

Renders the hike once for every combination of mesher (`voxel`, `smooth`), pixel scale and palette, as `DIR/<mesher>-<scale>-<palette>.png`. It also writes `DIR/contact-sheet.png`, which lays them all out side by side with a label above each, so you can compare looks before choosing your `render` options.

| Option | Default | Description |
| --- | --- | --- |
| `-o`, `--output DIR` | required | The directory to write to, created if needed |
| `--aspect SHAPE` | `story` | `story`, `video`, `portrait`, `feed`, `square` or `wide`, for every image |
| `--scale LIST` | `3,5,6` | The pixel scales to compare, each from `1` to `16` and dividing both sides of the image |
| `--palette LIST` | every built-in palette | The palettes to compare: built-in names, `.hex` files or `none`. A file is labelled with its name without `.hex` |
| `--exaggeration X` | automatic | As for [`render`](#terrain-and-look) |

It also takes the [data options](#data). From config files it takes its own `[style-lab]` table, but only the data settings and `exaggeration` from `[render]`, so a palette or scale you render with doesn't narrow the comparison.

## cache

```text
isotrail cache path [--cache-dir DIR]
isotrail cache clear [--yes] [--older-than DAYS] [--cache-dir DIR]
```

Downloaded elevation tiles, map data and weather are cached on disk, so later renders of the same area are fast and can work offline. Nothing expires on its own: map data stays as it was downloaded until you clear it. `cache path` prints where the cache is, and `cache clear` deletes it after asking (`--yes` skips the question). `--older-than DAYS` only deletes downloads made more than that many days ago, for example `--older-than 90` to pick up newer maps for areas you rendered long ago. Only files that isotrail wrote are deleted; anything else in the folder is left alone and counted in the message.

A cached download that turns out to be damaged is dropped and fetched once more.

| System | Cache directory |
| --- | --- |
| Windows | `%LOCALAPPDATA%\isotrail\cache` |
| Linux | `~/.cache/isotrail` |
| macOS | `~/Library/Caches/isotrail` |

Pass `--cache-dir DIR` to any command that downloads, or set `ISOTRAIL_CACHE_DIR`, to use another directory. The flag wins over the variable.

## Output for scripts

`render`, `embed`, `bundle` and `style-lab` print a line for each file they write. With `--json` they print one JSON object at the end instead, and nothing else on standard output:

```json
{
  "files": [
    { "path": "hike.gif", "bytes": 4183920, "width": 1080, "height": 1920, "frames": 300 }
  ],
  "caption_credit": null
}
```

Each file has its `path` as written and its size in `bytes`; images add `width` and `height`, and animations their number of `frames`. `caption_credit` is the data credit to paste into a caption when the image doesn't show it (with `--no-attribution`, `--overlay none`, or when it doesn't fit), and `null` otherwise. `stats --json` keeps its own format.

`-q` (`--quiet`) prints only errors: no warnings and no lines about files written, not even the caption credit, which `--json` still reports. A command's own output still prints: the `stats` table, `cache path` and the JSON object.

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
| `2` | The command line or a config file asks for something isotrail can't do, such as an unknown option, a bad value, options that can't go together or an output that would overwrite an input. Nothing has been downloaded or written. Overlays that don't fit the frame at all are found once the hike is prepared, so a GPX file's data may have been downloaded (and cached), but nothing is written |
| `130` | Stopped with Ctrl+C |
