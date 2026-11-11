# Changelog

All notable changes to `isotrail` are documented here.

## [Unreleased]

### Added

- **`--no-map-data`** on `render`, `bundle`, `embed` and `style-lab` builds the diorama from the elevation alone, without OpenStreetMap's lakes, woods and paths, for when the map server is busy or out of reach. The OpenStreetMap credit is left out of such a scene.
- **`isotrail cache clear --older-than DAYS`** deletes only downloads made more than that many days ago. Map data stays cached until it is cleared, so this is the way to pick up newer maps for an area.
- **Data credit in the web embed.** Once the interactive diorama replaces its poster, a small line along its bottom edge credits OpenStreetMap, the terrain tiles and, for hikes with weather, Open-Meteo, as the stills do, with links to OpenStreetMap's copyright page and Open-Meteo's site. It can be restyled with `::part(credit)`; `credit="none"` hides it for pages that credit the data themselves.
- **Events from `<isotrail-hike>`:** `isotrail-ready` once the diorama shows, and `isotrail-error` with a `reason` (`load`, `damaged`, `version`, `gpu` or `runtime`) when it gives up.
- The web runtime's `Viewer` has a `credit` getter, and its load errors carry a `reason`.
- A browser check of the embed (`web/test/check.mjs`) runs in CI.
- **The look of your choice in the web embed.** `embed` takes `render`'s look options (`--palette`, `--mesher`, `--block-height`, `--background`, `--time`, `--occlusion`, `--soft-shadows`, `--god-rays`) and `--scale` for the poster. The poster is drawn with them, and each one that isn't the default becomes an attribute of `<isotrail-hike>` in the snippet, so the live diorama matches its poster. A palette file is copied beside the bundle. The element reads the same attributes on any page and redraws when one changes; the runtime's `Viewer.load` takes the look and `setOptions` changes it.
- **Config files for every command.** `embed`, `bundle` and `style-lab` read config files and take `--config`. Each takes the `[render]` settings that are its own options, and its own `[embed]`, `[bundle]` or `[style-lab]` table wins over `[render]`.
- `style-lab` takes `--aspect`, `--exaggeration`, and lists for `--scale` and `--palette` (palette files and `none` included).
- `bundle --title` saves another title in the bundle.

### Changed

- **Patient map downloads.** When OpenStreetMap's Overpass server is busy (an error status, a time-out, a cut-off or unreadable answer, or a "timed out" or rate-limit note), isotrail now tries six times over about two minutes, saying each time how long it will wait, instead of giving up within seconds. If it still fails, the message suggests waiting, another server with `--overpass-url`, or `--no-map-data`. A server that can't be reached at all is given up on after a few seconds.
- A download that takes longer than 10 seconds prints a line saying it is still waiting, and answers larger than 512 MB are refused.
- Weather warnings name the server and Open-Meteo's own reason instead of printing the whole request address, and hikes from before 1940 or more than 16 days ahead skip the weather with a plain message.

- **Outputs are written safely.** Every file is written under a temporary name and only replaces the previous one once complete, so a render that fails or is stopped with Ctrl+C leaves the old file as it was, with no half-written file behind.
- Missing output folders are created, and outputs are checked before anything is downloaded or rendered: an output that is one of the command's own inputs (such as `isotrail bundle walk.gpx -o walk.gpx`) or an existing folder is refused, and so is an `embed --runtime-dir` that is the output folder.
- `embed` given a bundle that already sits in its output folder leaves it untouched, and refuses when `--season`, `--no-weather` or `--exaggeration` would change it.
- Mistakes in how a command is called, such as `--step-fps` above `--fps` or a GIF without a palette, exit with code 2 and a usage hint, like unknown options. Ctrl+C exits with 130. The exit codes are listed in the usage guide.
- `--block-height` with `--mesher smooth` does nothing, with a warning, instead of stopping the render.
- A GPX file with a few bad points is read instead of refused: a point without a valid position is skipped, and an elevation or time that can't be read (such as `nan`) is treated as missing, with one warning that counts them. Files saved as UTF-16 with a byte-order mark and times with offsets such as `+0000` are read too, and a file with several tracks says that they were joined.
- **`embed` snippets keep a poster without the script.** The element now holds an image of the poster and is sized before the script runs, so pages show the poster where the script is blocked or missing, and nothing jumps when it loads.
- The snippet's script address ends in `?v=<version>`, and the script passes it on to the renderer files, so a browser never runs a cached old renderer after an update.
- An embed well off screen frees its renderer and shows its poster, and starts again when it comes back, so pages with many hikes stay within the browser's limit on WebGL contexts.
- The embed's styles no longer need `'unsafe-inline'` in a page's Content-Security-Policy. `web/README.md` has a hosting checklist (MIME types, caching, Content-Security-Policy).

### Fixed

- `--overlay none` combined with other parts, a stat listed twice in `--stats`, a GIF palette with more than 255 colours and `--time` on a track without times are reported at once, instead of after the download.
- Options typed on the command line win over config file settings they conflict with: `--date` no longer fails when a config file sets `date-format`.
- A missing file named by `ISOTRAIL_CONFIG` is reported instead of ignored.
- `time` in a config file accepts a TOML time such as `07:30:00`.
- `embed --title` reaches the live diorama: the title is saved in the bundle, so the 3D view is labelled with it as well as the poster.
- `--detail`, `--offline` and `--overpass-url` typed for a scene bundle, which they don't affect, give a warning.
- Animations of a recording whose times run backwards no longer crash, and its stats no longer show a negative elapsed time: the hike runs from its earliest to its latest time, with a warning.
- A wrong timestamp, such as a first point stamped 1970, is ignored with a warning, instead of making a bundle of many megabytes with the wrong date, season and weather.
- A stray point far from the rest of the recording, such as one at 0°, 0°, is dropped with a warning, instead of stretching the diorama over hundreds of kilometres.
- Moving time is no longer blank for devices that log a point once a minute or less often.
- Routes that cross the 180° meridian ask for the map data on each side of it, instead of a band round the whole Earth.
- A cached elevation tile or weather answer that has been damaged is downloaded again, instead of failing every run until the cache is cleared.
- `cache clear` only deletes files that isotrail wrote, and says how many other files it left alone. It used to delete any `.bin`, `.json` or `.tmp` file in the cache folder.
- Scene bundles record only the scheme and host of `--overpass-url`, so a user name, password or path in it is never published with a bundle.
- With `--offline`, the weather of a hike bundled within a few days of the walk is found in the cache again.
- A track whose first or last time has a fraction of a second could give a bundle that `render` and the web embed then refused as damaged. isotrail now checks every bundle as it writes it, so what it writes can always be read back.
- A damaged or hand-edited bundle is refused as damaged, instead of crashing or freezing the renderer, and a bundle made by a newer isotrail asks for an update instead of being made again.
- Very tall terrain, such as `--exaggeration 30 --block-height 0.25` on a mountain hike, renders instead of crashing: the slab's sides use far less graphics memory. Terrain that is still too big for the GPU, such as `--detail 1024 --exaggeration 100`, stops at once with a message naming the options to lower, instead of using gigabytes of memory and crashing.
- `isotrail bundle` given a scene bundle says that it already is one, instead of reporting a text-encoding error.
- An embed whose WebGL context was lost no longer stays blank with working-looking buttons: it starts again, and keeps its poster if the context is lost again within a minute.
- The embed's keyboard focus ring is visible on the diorama, and its buttons and progress line are easier to hit on touch screens.
- An embed that can't show a hike says why (it couldn't be loaded, was saved by another version of isotrail, or the browser can't show 3D) instead of always blaming the browser.
- **Overlays fit at every pixel scale.** At `--scale 6` and `10`, panels no longer overlap each other or the diorama, and text no longer runs off the image. When the overlays don't fit, they give way one step at a time with a warning: a short date, a small title, no subtitle, closer stats, no profile, no credit (printed for the caption instead), then fewer stats. If even that doesn't fit, the command stops with a usage error before rendering.
- Short animations and those with a low `--step-fps` always end on the finished hike, with the route drawn to the finish flag and the camera at rest; a 5-second clip at one image a second used to stop short of the finish.
- MP4s are encoded at H.264 level 4.0 (4.2 above 30 frames a second) instead of 5.0, which some older phones refuse, and carry all their colour tags (BT.709 primaries and transfer were missing).
- An ffmpeg that stops taking frames without exiting is stopped after a minute with its last message, instead of leaving the render waiting for ever.
- Titles with accents typed as separate marks, as macOS file names store them, are drawn with their accented letters, and characters the font lacks, such as emoji, are left out with a warning instead of drawn as `?`.
- An elevation just below sea level prints as `0 m`, not `-0 m`.
- `.hex` palette files saved with a byte-order mark, as PowerShell 5.1 writes them, are read.
- An error inside the web renderer ends in the poster instead of a frozen picture, and numbers that aren't finite passed to the `Viewer` API are ignored.
- A renderer download that failed once is tried again by the next embed instead of failing every embed until the page is reloaded, and playback no longer jumps ahead after the tab was in the background.
- `embed` percent-encodes `:` in file names, which made a browser read `Rysy:2025.itb` as an address with its own scheme.


## [1.3.0] - 2026-11-10

Light, weather and seasons: every hike is lit by the sun and moon of its day, in the weather it had and the season it was in.

### Added

- **Time-of-day lighting:** hikes are lit by the sun as it stood when they started, worked out from the time and place of the track. Midday is white and bright, evenings turn golden and orange, and dusk and night go deep blue, with the sky changing to match. Animations and the web embed use the same light.
- **`--time HH:MM`** on `render` relights the scene at another local time on the day of the hike, such as `--time 20:30` for a sunset.
- **Night sky:** after dusk, stars come out and the moon rises, in its real place and phase for the date. A bright moon lights the terrain with a cool light and casts shadows; on a moonless night the scene is lit by a dim blue glow.
- **Weather during the hike:** `bundle`, `render` and `embed` look up the hour-by-hour weather on [Open-Meteo](https://open-meteo.com/) (temperature, conditions, cloud at three heights, rain, snow, snow depth and wind) and keep it in the scene bundle, ready to be drawn. If it can't be found, the bundle is made without it; `--no-weather` skips it.
- **Weather on the diorama:** the weather during the hike is drawn in stills, animations and the web embed. Clouds drift across the sky, and on a partly cloudy day their shade passes over the ground. Overcast skies turn grey and dull the light, rain falls in streaks and snow in flakes, both slanted by the wind, and fog and heavy rain fade the far side of the slab into haze. In the web embed, rain and snow keep falling while the diorama is on screen, unless the visitor prefers reduced motion.
- **Seasons:** the landscape matches the time of year. In spring, woods come into fresh leaf and blossom and meadows flower; in autumn, broadleaf woods turn yellow, orange and rust, grass turns golden and moorland tan; in winter, trees stand bare over brown grass. When there was snow on the ground, the slopes above the snowline are white, with patches of snow below it, and lakes that froze in a cold spell before the hike are drawn as ice. The seasons are turned round south of the equator.
- **`--season spring|summer|autumn|winter`** on `render`, `embed` and `bundle` shows a hike in another season. A forced winter covers the whole slab in snow and freezes every lake.
- `bundle` reports the season it found, such as `Season: autumn colours, snow above 1650 m`.
- **Light with depth:** hollows, valley floors and the feet of slopes and steps see less of the sky, so the terrain keeps its shape in the shade, and shadow edges soften into a few steps. `--occlusion` and `--soft-shadows` set how much.
- **God rays:** when the sun breaks through a partly cloudy sky, its light falls in beams. Shafts slant across the sky, and the ground they reach is lit brightly while the ground between them lies in the cloud's shade. `--god-rays on` shows them whenever the sun is up, and `--god-rays off` never.
- **`stats --weather`** prints the weather during the hike, as a summary and hour by hour in local time, and adds it to `--json`.
- **The sun moves in animations:** GIFs, MP4s and the web embed light each frame by the time the route's head was walked, from the start to the finish, with either `--pace`. On multi-day treks, each night becomes a short pause in which the head waits while the sun sets, the night passes and the sun rises again. The elapsed time keeps counting through it.

### Changed

- `--no-attribution` now prints Open-Meteo's credit along with the others when the bundle holds weather, and the credit drawn on the image ends in "Open-Meteo.com" when weather is drawn.
- The default look has deeper light, softer shadows and god rays in partly cloudy weather, in stills, animations and the web embed. `--occlusion 0 --soft-shadows 0 --god-rays off` gives the earlier look.
- `--no-weather` on `render` and `embed` also leaves out the weather a scene bundle holds, for a clear-sky render of a rainy hike, along with its snow and ice.
- Looking up the weather also fetches the daily temperatures and snowfall of the fortnight before the hike, to tell whether lakes had frozen.
- Scene bundles now carry the sun's and the moon's paths across the sky, the weather during the hike and the season. Bundles made by earlier versions can't be read any more; make them again with `isotrail bundle` (or `isotrail embed`).
- Animations end on the light at the finish of the hike, while the still image keeps the light at the start. The web embed opens under the start's light, like its poster, and follows the route's head once it is played or scrubbed.
- `--pace time` no longer cuts the nights of a multi-day trek to a minute: they become the night pauses above. Other stops are still cut to a minute.

## [1.2.0] - 2026-11-10

Hikes on the web: put an interactive diorama of any hike on your own site, with one command.

### Added

- **Renderer in the browser:** the renderer compiles to WebAssembly and draws a scene bundle into a canvas, on WebGPU with a WebGL2 fallback. A JavaScript API sets the route's progress and the viewing angle, and a demo page in `web/demo/` shows it working. See [`web/README.md`](web/README.md).
- **`<isotrail-hike>` web component:** a drop-in element for any web page, with `src`, `poster`, `aspect` and `autoplay` attributes. The poster shows at once and the renderer loads when the element comes into view. Visitors can drag to turn the diorama, zoom up to 3× toward the pointer (Ctrl/⌘ + scroll, pinch or buttons) and move around, and play or scrub the route as it draws itself. It works from the keyboard, plays nothing on its own for visitors who prefer reduced motion, and keeps the poster in browsers without WebGPU or WebGL2.
- **`embed` command:** `isotrail embed hike.gpx -o DIR` writes the scene bundle, a poster image and an HTML snippet for the `<isotrail-hike>` element in one go. `--with-runtime` also copies the web runtime, and `--base-url` and `--runtime-url` set the addresses in the snippet. [`web/README.md`](web/README.md) has examples for Hugo and Astro.
- The release downloads include the web runtime, in a `web` folder next to `isotrail`.

## [1.1.0] - 2026-10-26

Hikes come to life: render any hike as a looping GIF or an MP4 video for stories.

### Added

- **Animated GIFs:** `isotrail render hike.gpx -o hike.gif` renders a looping animation. The route draws itself with a marker at its head, the elevation profile fills in and the stats count up, while the camera eases round to the still image's view. `--duration`, `--fps`, `--step-fps`, `--orbit` and `--pace distance|time` control the motion, and time pacing cuts long stops short.
- **MP4 videos:** `isotrail render hike.gpx -o hike.mp4` renders the same animation as an H.264 video, ready to post as a story. It needs [ffmpeg](https://ffmpeg.org/download.html) on your `PATH`.

## [1.0.0] - 2026-10-25

The first release: render any recorded hike as a pixel-art diorama PNG.

### Added

- **Hikes in:** GPX 1.0 and 1.1 tracks and routes, from AllTrails and other apps. Recordings with pauses, gaps, single-point segments, non-ASCII names or missing timestamps or elevations are all handled.
- **Stats:** `isotrail stats` prints distance, smoothed elevation gain and loss, elevation range, elapsed and moving time, and start and end in the local time zone. `--json` prints SI values for scripts.
- **Real terrain:** elevation from AWS Terrain Tiles, and landcover, lakes, rivers, paths and roads from OpenStreetMap through the Overpass API. The ground is classified from the terrain where the map is empty, and lakes are levelled.
- **Scene bundles:** `isotrail bundle` saves everything a render needs in one compact `.itb` file, for repeatable offline renders. `--debug-dir` writes heightmap, hillshade, landcover, water and path images.
- **Caching:** downloaded data is cached on disk. `--offline` uses only cached data, and `isotrail cache path|clear` manages the cache.
- **Diorama renderer:** a GPU renderer (wgpu) that runs without a window, with a software fallback. The slab is turned to the route, and the camera follows the hike's direction. Terrain comes from the `voxel` (stepped) or `smooth` (low-poly) mesher, with stratified slab sides, cel lighting with shadows, outlines, trees and boulders, levelled water, and a draped route that shows through ridges as a dashed line.
- **Pixel-art look:** palette quantisation in OKLab with three built-in palettes (`apollo`, `resurrect-64`, `aap-64`) or any Lospec `.hex` file. Pixel scales 2, 3, 5, 6 and 10 with crisp nearest-neighbour enlargement. Sky, transparent or solid-colour backgrounds.
- **Overlays:** title, subtitle, date, stats and an elevation profile in the monogram pixel font (with Polish and other Latin Extended-A characters), on frosted panels. Choose which parts appear, which stats, the units, and the date format.
- **Presets:** `story` (1080 × 1920), `feed` (1080 × 1350), `square` (1080 × 1080) and `wide` (1920 × 1080), with Instagram safe areas, rendered one at a time or all at once.
- **Attribution:** a data credit on every render, or printed for the caption with `--no-attribution`.
- **Config files:** a `[render]` table in a user config file or a `--config` file, with flags taking precedence.
- **Style lab:** `isotrail style-lab` renders every mesher, scale and palette on one labelled contact sheet.
- Prebuilt binaries for Windows, Linux and macOS.

[Unreleased]: https://github.com/ErykMajoch/isotrail/compare/v1.3.0...HEAD
[1.3.0]: https://github.com/ErykMajoch/isotrail/compare/v1.2.0...v1.3.0
[1.2.0]: https://github.com/ErykMajoch/isotrail/compare/v1.1.0...v1.2.0
[1.1.0]: https://github.com/ErykMajoch/isotrail/compare/v1.0.0...v1.1.0
[1.0.0]: https://github.com/ErykMajoch/isotrail/releases/tag/v1.0.0
