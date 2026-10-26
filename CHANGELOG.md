# Changelog

All notable changes to `isotrail` are documented here.

## [Unreleased]

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

[Unreleased]: https://github.com/ErykMajoch/isotrail/compare/v1.1.0...HEAD
[1.1.0]: https://github.com/ErykMajoch/isotrail/compare/v1.0.0...v1.1.0
[1.0.0]: https://github.com/ErykMajoch/isotrail/releases/tag/v1.0.0
