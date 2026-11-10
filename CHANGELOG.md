# Changelog

All notable changes to `isotrail` are documented here.

## [Unreleased]

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

[Unreleased]: https://github.com/ErykMajoch/isotrail/compare/v1.2.0...HEAD
[1.2.0]: https://github.com/ErykMajoch/isotrail/compare/v1.1.0...v1.2.0
[1.1.0]: https://github.com/ErykMajoch/isotrail/compare/v1.0.0...v1.1.0
[1.0.0]: https://github.com/ErykMajoch/isotrail/releases/tag/v1.0.0
