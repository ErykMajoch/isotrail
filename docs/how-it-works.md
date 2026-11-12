# How it works

`isotrail` turns a hike into an image in two stages. First it **prepares a scene**: it reads the GPX file, downloads the terrain around it and works out what covers the ground. Then it **renders** that scene on the GPU as low-resolution pixel art and enlarges it.

`isotrail render hike.gpx` runs both stages. `isotrail bundle` stops after the first and saves the scene, and `isotrail render hike.itb` starts from a saved scene.

## Preparing the scene

**The track.** isotrail reads every track point and drops segments too short to draw. It computes distance, elevation gain and loss (smoothed, so that GPS noise doesn't inflate the climb), moving and elapsed time, and the local time zone of the start, which is used for the date.

**The slab.** The diorama is a rectangle of terrain around the route, turned to line up with the direction the route mostly runs in, so diagonal routes don't waste space. It is padded by 15% on each side (at least 500 m) and widened so that it is never more than twice as long as it is wide. The slab is divided into a grid of about 256 cells along its long side (`--detail`): about 25 m per cell for a day hike, a few hundred metres for a multi-day trek.

**Elevation.** Heights come from the open [Terrain Tiles](https://registry.opendata.aws/terrain-tiles/) dataset. isotrail picks the zoom level whose resolution matches the grid and samples each cell from it. Heights are then exaggerated so the relief reads at diorama scale: gentle moors get up to 5× and steep mountains less (`--exaggeration` overrides this).

**Landcover.** One [Overpass](https://overpass-api.de/) query fetches OpenStreetMap's forests, moorland, rock, scree, grass, farmland, wetlands, water, paths, roads, streams and coastlines for the slab. These are drawn onto the grid. Cells that the map leaves empty are classified from the terrain: steep slopes become bare rock, and high ground becomes grass or rock. Lakes and reservoirs are flattened to a single water level. OpenStreetMap draws coastlines with the land on their left, so the cells on their right are the sea, which lies flat at 0 m whatever the depth of the sea bed below it; the first cells of land beside it are raised to at least 0 m, so the shore never dips under the water. A slab with no coastline near it is all sea only when none of it is above sea level and the map shows nothing on land there, so ground below sea level inland stays land. With `--no-map-data` there is no query, and every cell is classified from the terrain. A slab that crosses the 180° meridian is queried as two boxes, one on each side.

<p align="center">
  <img src="images/debug-hillshade.png" alt="Shaded relief of the slab with the route" width="45%">
  <img src="images/debug-landcover.png" alt="Landcover of the slab with the route" width="45%">
</p>

The route is then draped onto the terrain. The sun's position over the slab is worked out every five minutes from the start of the hike to its end, with the [NOAA solar calculator](https://gml.noaa.gov/grad/solcalc/) equations, which follow Jean Meeus's *Astronomical Algorithms*. The moon's position and phase come from the main terms of Meeus's lunar theory, good to a few tenths of a degree.

**Weather.** The hourly weather during the hike comes from [Open-Meteo](https://open-meteo.com/): temperature, the WMO weather code, cloud cover at low, middle and high levels, rain, snowfall, snow depth and wind, from the hour before the start to the hour after the end. Hikes that ended at least five days ago use the historical archive (ERA5 reanalysis and related models); more recent ones, or any the archive hasn't filled in yet, use the forecast models' own past hours. The weather is read in the middle of the route, at the track's mean height, so the temperature is for the height walked. Long routes are split into stretches of about 20 km (at most eight), and each hour reads the weather on the stretch the hiker was on. In between hours, values are interpolated; rain, snow and the weather code hold through the hour they were measured over.

**Season.** The date of the hike sets how far the plants have come through the year, counted half a year on south of the equator and not at all in the tropics. Snow and ice come from the weather. Where there was snow on the ground at the height walked, the snowline sits just below the lowest snowy reading, lower for deep snow, and never below snow-free ground the route crossed. Where none lay but snow fell in the fortnight before or during the hike, the snowline sits 300 m above the average freezing level of that fortnight. For ice, isotrail also fetches the daily lows and highs of the 14 days before the hike and adds up the frost: each day's mean temperature is carried up or down at 6.5 °C per kilometre, and lakes freeze from the height where the frost below zero adds up to 30 degree-days. The sea never freezes.

Everything is saved together as a scene bundle: a compact, versioned binary file of about 100 KB.

## Rendering

The scene is drawn with [wgpu](https://wgpu.rs/) at a small internal resolution: 360 × 640 for a story at the default `--scale 3`.

1. **Mesh.** The `voxel` mesher builds one column of blocks per cell, with the height rounded to whole blocks. The `smooth` mesher builds a low-poly surface with one flat colour per triangle. Trees and boulders are placed by a random generator seeded from the scene, so the same hike always renders the same way. The season colours the cells and trees before anything is lit: each broadleaf tree draws its own days for coming into leaf, turning and dropping its leaves, two days later in spring and earlier in autumn for every 100 m above the slab's lowest ground. Grass, fields, moorland and wetland are blended towards their season's colour by a share that varies a little from cell to cell, and a few meadow cells flower in spring. Cells above the snowline are white unless they are steeper than 50°, and below it each cell has its own reach, up to 150 m and mostly short, so the snow breaks up into patches; the trees standing in snow are frosted. Frozen lakes are ice with a few cracks, or snow when they lie above the snowline. The sea is a darker blue than lakes, with surf along its shore: a broken line of foam on the water against the land, and scattered flecks over lighter, shallow water just beyond it.
2. **Slab sides.** The cut faces show strata that follow the surface: a darker topsoil, a few blocks of dirt, then stone, with a band of deep stone at the base and seeded pebbles and ore. Where a lake or the sea reaches the edge, its water shows in cross-section, under a block of ice when a lake is frozen. Blocks of the same colour stacked in a wall are drawn as one face, which keeps very tall slabs light enough for the GPU. Under snow, the topsoil is a layer of snow.
3. **Light.** The sun stands where it was when the hike started (or at `--time`), and casts shadows from a shadow map. Its height sets the mood: white daylight above 20°, warmer and dimmer golden light as it sinks, orange at sunset, then the cool blue of dusk and night, with the sky's gradient following along. The light never comes from lower than 10°, so shadows stay long but bounded, and the scene still reads by night. After dusk the moon takes over: the stronger its light (a full moon high in the sky), the more of the direct light and shadows come from it, in a cool blue-white. Tracks without times get a late-morning sun from the south-south-east.

**Depth.** Each grid cell looks out along eight directions for the highest terrain within a few dozen cells, and the higher its horizons, the less of the sky it sees. Faces take less of the sky's light, in the same bands as the sun's, from the cell in front of them, so step walls darken at their feet; the slab's cut faces look out into open air. Shadow edges are softened by sampling the shadow map at twelve points around each lookup, and the bands turn the softening into a few steps.

**Weather.** The weather at the same moment changes the light and adds to the scene. Low and middle cloud dims the sun and greys the sky's colours, while thin high cloud barely does; cloud also hides the stars and the moon. Clouds are drawn in the sky from smooth noise, thresholded by the cloud cover, with a lit top and a shaded underside, and they drift with the wind across the view. On a partly cloudy day, the same kind of noise over the ground, block by block, takes the sunlight away under passing clouds. Rain and snow are drawn over the finished scene in two layers, near and far: each particle falls through a grid of cells, slanted by the wind blowing across the view, so the same moment always looks the same. Fog codes, heavy rain and low cloud fade the terrain towards a haze colour, more at the back of the slab than at the front.

**God rays.** When the sun breaks through a partly cloudy sky, its light falls in beams: sheets of light along the sun's direction, separated by gaps of shade drawn from smooth noise and drifting with the wind. The sheets are laid so that each one also holds the view's direction. A view ray never passes from one sheet to the next, so the shaft seen against the sky lines up with the ground it lights. Ground in a beam gets about twice the sunlight and ground in a gap about half, snapped to whole cells, so the terrain is lit by the beams rather than covered by them; the shafts themselves are only drawn against the sky, dithered and fading as they go. They are strongest from a low sun and don't show under an overcast sky or at night.

The sky behind the diorama is a band of the real sky, half a turn either side of the view across the frame's width, with the horizon a little above the middle and the zenith at the top. The moon is drawn there as a disc, sunlit on the side facing the sun, and stars fade in as the sun sinks below 6°. The stars are scattered from the bundle's seed, so every render of a hike shows the same sky, and they move with the camera as it turns. Lighting is cel-shaded into a few flat bands, and a soft fill from the viewer's side keeps faces that point away from the sun readable.
4. **Outlines.** Dark lines mark the slab's silhouette and the places where one surface passes in front of another. Sharp outward folds, such as ridges and the rim of the slab, are picked out in a lighter shade.
5. **Route.** The route is a bold line with a dark border that climbs every block step it crosses. Where a ridge hides it, it shows through as a faint dashed line. Flags mark the start and finish.
6. **Palette.** Every pixel is snapped to the nearest colour of the palette, compared in [OKLab](https://bottosson.github.io/posts/oklab/), a colour space that matches how alike colours look to the eye. The sky gradient is dithered with a fine checker pattern.
7. **Text.** The title, date, stats and elevation profile are drawn in the [monogram](https://datagoblin.itch.io/monogram) pixel font, on frosted panels that blur and tint the scene behind them. The panels are laid out clear of the parts that social apps cover with their own buttons and captions.
8. **Enlarge.** The frame is scaled up by a whole number without smoothing, so every drawn pixel becomes a crisp square.

## Animation

An animation renders the same scene once per frame. The mesh, trees and route are built once, and the camera keeps one scale for the whole clip, wide enough to fit the slab from every angle of its turn, so the diorama never grows or shrinks. The camera's position is rounded to whole pixels, so the terrain doesn't shimmer as it moves.

Each frame draws the route up to a point: by distance, or by the recorded time with long stops cut short (`--pace time`). Stops of 6 hours or more are nights instead: the head waits for a set share of the drawing while the clock runs through the gap. The same point fills the profile and sets the stats, which count up from what the hike had covered so far, and the time there sets the sun, moon and weather. Rain, snow and clouds move with the clip's own clock, so they keep moving through the opening and the hold. The finished frames are written to the GIF one by one, each keeping only the area that changed since the frame before. Frames that look the same as the one before are merged into one longer frame. For an MP4, each frame is enlarged to the full output size and piped to ffmpeg, which encodes it as H.264.

## In the browser

The same renderer compiles to WebAssembly and draws into a web page's canvas: with WebGPU where the browser has it, and WebGL2 otherwise. It loads a scene bundle, builds the mesh once, and redraws when the route's progress or the viewing angle changes, and all the time while rain or snow is falling. Its pixels are as large as keep the frame's long side within 640 pixels, matching a still at `--scale 3`, and one scale fits the slab from every angle, so the diorama can be turned all the way round. Building it is described in [`web/README.md`](../web/README.md).

## Styles

The meshers and palettes can be mixed freely. `isotrail style-lab` renders every combination for your hike on one contact sheet.

<table>
  <tr>
    <td width="25%"><img src="images/style-voxel-apollo.png" alt="Voxel mesher with the apollo palette"></td>
    <td width="25%"><img src="images/style-smooth-apollo.png" alt="Smooth mesher with the apollo palette"></td>
    <td width="25%"><img src="images/style-voxel-resurrect-64.png" alt="Voxel mesher with the resurrect-64 palette"></td>
    <td width="25%"><img src="images/style-voxel-aap-64.png" alt="Voxel mesher with the aap-64 palette"></td>
  </tr>
  <tr>
    <td align="center"><code>voxel</code> · <code>apollo</code><br>(default)</td>
    <td align="center"><code>smooth</code> · <code>apollo</code></td>
    <td align="center"><code>voxel</code> · <code>resurrect-64</code></td>
    <td align="center"><code>voxel</code> · <code>aap-64</code></td>
  </tr>
</table>

## Data sources and fair use

isotrail downloads its data from three free services that others run for everyone to share. It only contacts them while it prepares a scene from a GPX file (`render`, `bundle` and `embed`, and `stats --weather`). A scene bundle renders without the network.

| Data | Service | Requests for one hike |
| --- | --- | --- |
| Elevation | [Terrain Tiles](https://registry.opendata.aws/terrain-tiles/) on AWS Open Data (`s3.amazonaws.com/elevation-tiles-prod`) | The tiles that cover the slab, usually a handful |
| Map data | The public [Overpass API](https://overpass-api.de/) (`overpass-api.de`) | One query for the whole slab, or two when it crosses the 180° meridian |
| Weather | [Open-Meteo](https://open-meteo.com/) (`archive-api.open-meteo.com`, or `api.open-meteo.com` for recent hikes) | Two: the hours of the hike, and the daily lows and highs of the fortnight before it. A long route asks for up to eight places in each |

Every answer is kept in the [cache](usage.md#cache), and nothing in it expires. Rendering a hike again, in any look or shape, sends no requests, and neither does a bundle. `--offline` makes sure of it. Each request names isotrail and this repository in its `User-Agent`, so the services' operators can tell where it came from.

**Overpass.** The public server is shared by everyone who works with OpenStreetMap data. Its [usage policy](https://dev.overpass-api.de/overpass-doc/en/preface/commons.html) asks each user to stay below about 10,000 queries and 1 GB of downloads a day. Rendering your own hikes stays far below that, and when the server is busy, isotrail waits as it asks (see [data options](usage.md#data)). For many hikes in one go, such as a script over a whole archive of recordings, use another public instance or [run your own](https://dev.overpass-api.de/overpass-doc/en/more_info/setup.html), and point isotrail at it with `--overpass-url` or `ISOTRAIL_OVERPASS_URL`. `--no-map-data` leaves Overpass out altogether.

**Open-Meteo.** The free API is for [non-commercial use only](https://open-meteo.com/en/terms), with fewer than 10,000 calls a day. Renders for a business, a client or a site with ads or subscriptions are commercial use and need one of [Open-Meteo's paid plans](https://open-meteo.com/en/pricing). isotrail can't use an API key yet, so for commercial work, leave the weather out with `--no-weather`.

**Terrain Tiles.** The tiles are free on AWS Open Data. Their sources ask to be credited, which the "Terrain Tiles (AWS)" in the credit line does; the full list is in the [README](../README.md#credits).

**Credit.** All three services ask to be credited. Every render and embed carries a small credit line by default. If you leave it off with `--no-attribution`, isotrail prints the text, so you can put it in your post's caption or next to the image on your page instead.
