# isotrail on the web

The `<isotrail-hike>` element puts an interactive diorama of a hike on any web page, static sites included. Visitors can turn it, zoom in, and watch the route draw itself. Under the hood, the `isotrail-web` crate compiles the renderer to WebAssembly and draws a scene bundle (`.itb`) into a canvas. It uses WebGPU where the browser has it, and WebGL2 otherwise.

## Building

You need the Rust toolchain from the repository (it includes the `wasm32-unknown-unknown` target) and [wasm-pack](https://rustwasm.github.io/wasm-pack/):

```bash
cargo install wasm-pack
```

Then, from the repository root (on Windows, in Git Bash):

```bash
sh web/build.sh
```

This runs three steps, which you can also run by hand:

```bash
# The renderer, optimised with wasm-opt -Oz, into web/pkg/
wasm-pack build crates/isotrail-web --target web --release --out-dir ../../web/pkg

# A synthetic hike for the demo, into web/demo/hike.itb
cargo run -p isotrail-testkit --example demo_bundle -- web/demo/hike.itb

# Its poster image
cargo run -p isotrail-cli -- render web/demo/hike.itb -o web/demo/hike.png --aspect wide --overlay none
```

## Embedding a hike

`isotrail embed` makes everything a page needs from a GPX file (or a `.itb` bundle):

```bash
isotrail embed derwent-edge.gpx -o static/hikes/derwent --base-url /hikes/derwent/ --with-runtime
```

```text
static/hikes/derwent/
├─ derwent-edge.itb      the scene bundle
├─ derwent-edge.png      the poster
├─ snippet.html          the HTML to paste into a page
├─ isotrail-embed.js     the element, with --with-runtime
└─ pkg/                  the renderer, with --with-runtime
```

`snippet.html` holds the two lines that show the hike, also printed by the command:

```html
<script type="module" src="/hikes/derwent/isotrail-embed.js?v=1.3.1"></script>
<isotrail-hike src="/hikes/derwent/derwent-edge.itb" poster="/hikes/derwent/derwent-edge.png" aspect="wide" style="display:block;aspect-ratio:16/9"><img src="/hikes/derwent/derwent-edge.png" alt="Diorama of Derwent Edge" style="display:block;width:100%"></isotrail-hike>
```

The image inside the element is the poster for pages where the script doesn't run, because it is blocked, missing or still loading: the element already has its size and shows the poster, so nothing jumps when the script arrives. Once it runs, the element shows its own copy of the poster and hides this one. The `?v=` on the script's address is the isotrail version; the script passes it on to the renderer files in `pkg/`, so after you update isotrail and its snippets, browsers fetch the new runtime instead of mixing cached old files with new ones.

`--base-url` is the address the directory will be served at. Without it the snippet uses bare file names, so serving the directory and opening `snippet.html` shows a preview. `--aspect` picks the element's shape and its poster's, and the poster takes `render`'s overlay options, such as `--title` or `--overlay none`. [The usage guide](../docs/usage.md#embed) lists every option.

`--with-runtime` copies the web runtime from the `web` folder that comes next to `isotrail` in the release downloads, or from `--runtime-dir` (`--runtime-dir web` in the repository, after building it). The runtime is the same for every hike, so a site with several hikes only needs it once. Copy that `web` folder (`isotrail-embed.js` and `pkg/`) to one place, such as `static/isotrail/`, and point each hike's snippet at it instead:

```bash
isotrail embed derwent-edge.gpx -o static/hikes/derwent --base-url /hikes/derwent/ --runtime-url /isotrail/isotrail-embed.js
```

`isotrail-embed.js` loads the renderer from the `pkg/` folder next to it. A page that shows several hikes only loads the script and the renderer once, as long as every snippet uses the same address for the script, `?v=` included. `--runtime-url` gets `?v=` too, unless the address you give already has a query.

### Hugo

Put the files under `static/`, with the runtime in `static/isotrail/`, and add a shortcode in `layouts/shortcodes/hike.html`:

```go-html-template
<script type="module" src="/isotrail/isotrail-embed.js"></script>
<isotrail-hike src="{{ .Get "src" }}" poster="{{ .Get "poster" }}" aspect="{{ .Get "aspect" | default "wide" }}"{{ if .Get "autoplay" }} autoplay{{ end }}></isotrail-hike>
```

Then use it in any post:

```markdown
{{< hike src="/hikes/derwent/derwent-edge.itb" poster="/hikes/derwent/derwent-edge.png" >}}
```

### Astro

Put the files under `public/`, with the runtime in `public/isotrail/`, and add a component in `src/components/Hike.astro`:

```astro
---
const { src, poster, aspect = 'wide', autoplay = false } = Astro.props;
---
<script is:inline type="module" src="/isotrail/isotrail-embed.js"></script>
<isotrail-hike src={src} poster={poster} aspect={aspect} autoplay={autoplay}></isotrail-hike>
```

`is:inline` keeps Astro from bundling the script, which has to stay next to its `pkg/` folder. Use it in a page or an MDX post:

```astro
---
import Hike from '../components/Hike.astro';
---
<Hike src="/hikes/derwent/derwent-edge.itb" poster="/hikes/derwent/derwent-edge.png" />
```

### Attributes

| Attribute | Meaning |
|---|---|
| `src` | The scene bundle (`.itb`) to show |
| `poster` | An image shown straight away, and kept if the browser can't draw the diorama |
| `aspect` | `story` or `video` (9:16), `portrait` (3:4), `feed` (4:5), `square`, `wide` (16:9, the default), or a width and height such as `3:2`. The element fills its container's width at this aspect |
| `autoplay` | Plays the route once as soon as the diorama is ready, unless the visitor prefers reduced motion |
| `backend` | `webgl` forces the WebGL2 fallback, for testing it in a browser that has WebGPU |
| `credit` | `none` hides the data credit line. Only use it when the page credits the data itself (see below) |
| `credit-extra` | A line of the page's own, such as a name or site, shown under the data credit; `isotrail embed --credit-extra` writes it |

The look attributes choose how the diorama is drawn, like `isotrail render`'s options of the same names. `isotrail embed` writes one into the snippet for each look option you give it that isn't the default, so the live diorama matches its poster.

| Attribute | Values (default first) |
|---|---|
| `palette` | `apollo`, `resurrect-64`, `aap-64` or `none`, or the address of a Lospec `.hex` file, such as `/hikes/warm.hex` |
| `mesher` | `voxel` or `smooth` |
| `block-height` | `1`, or `0.25` to `4`: voxel block height as a share of its width |
| `occlusion` | `1`, or `0` to `1` |
| `soft-shadows` | `1.5`, or `0` to `4` grid cells |
| `god-rays` | `auto`, `on` or `off` |
| `background` | `sky`, `transparent` (the page shows through) or a colour such as `#1e2a33` |
| `time` | the start of the hike, or a local time such as `20:30` on the day of the hike, which then lights the diorama while it plays |

The scene, light and route attributes work the same way, for `isotrail render`'s [scene, light and route options](../docs/usage.md#scene-light-and-route):

| Attribute | Values (default first) |
|---|---|
| `prop-density` | `1`, or `0` (no trees, bushes or boulders) to `2` |
| `paths`, `streams`, `strata` | `on` or `off`: paths and roads from the map, streams, and the layers on the cut faces |
| `base-blocks` | `6`, or `1` to `24`: how far the base sits below the lowest ground, in blocks |
| `outlines` | `1`, or `0` to `2` |
| `shadows`, `night-sky` | `on` or `off` |
| `weather-effects` | `clouds,shade,rain,snow,haze`, fewer of them, or `none` |
| `weather` | the recorded weather, or `clear`, `cloudy`, `overcast`, `rain`, `snow` or `fog` |
| `sun` | the hike's sun, or a compass bearing and elevation in degrees such as `250,8`, which then lights the diorama while it plays |
| `route-colour` | `#d64834`, or another colour; the progress line follows it unless the page sets `--isotrail-progress` |
| `route-width` | `2`, or `1` to `4` pixels |
| `route-hidden` | `dashed`, `dim`, `solid` or `off`: the parts of the route behind ridges |
| `markers` | `start,finish,head`, fewer of them, or `none` |

The view attributes set where the diorama starts, and where the reset button, 0 and Home take it back to, like `isotrail render`'s camera options. `isotrail embed` writes them for its `--turn`, `--heading`, `--pitch` and `--zoom`.

| Attribute | Values (default first) |
|---|---|
| `turn` | `0`, or `-360` to `360`: degrees the camera moves anticlockwise round the slab, as seen from above, from the view along the hike |
| `pitch` | `30`, or `10` to `80`: how far the camera looks down, in degrees |
| `zoom` | `1`, or `0.5` to `2`: above 1 the diorama starts zoomed in; below 1 it leaves more room round the slab |

Changing one of them on a running element redraws it; a new `mesher`, `block-height`, `prop-density`, `paths`, `streams`, `strata` or `base-blocks` builds the terrain again, and a new `turn`, `pitch` or `zoom` goes to the new starting view. A value the element can't use, such as an unknown palette name or a `.hex` file that doesn't load, is left at its default, with a warning in the console.

The poster shows at once. The renderer (about 1.2 MB gzipped) only downloads when the element comes near the screen, and it downloads once however many hikes are on the page. When an element has been well off screen for a few seconds it frees its renderer and shows its poster, and starts again when it comes back; browsers only give a page about 16 WebGL contexts, so a page with many hikes keeps working as it scrolls. A view the visitor turned or zoomed starts again from the starting view when this happens.

If the GPU is lost while the page is open, for instance when its driver resets or the browser reclaims a WebGL context, the element starts again. If that happens again within a minute, it keeps the poster.

### When something fails

The element keeps its poster when it can't show the diorama. Without a poster, it shows one of three messages instead:

| `reason` | Message |
|---|---|
| `load` | This hike couldn't be loaded. (the bundle or the renderer didn't download) |
| `damaged` | This hike couldn't be loaded. (the file isn't a scene bundle, or is cut short) |
| `version` | This hike was saved by a different version of isotrail. (make the bundle again with the isotrail that matches the runtime) |
| `gpu` | This browser can't show the 3D view. (no WebGPU or WebGL2, or the GPU failed twice) |
| `runtime` | This browser can't show the 3D view. (an error inside the renderer) |

A warning with the details also goes to the console. A failed element stays as it is until its `src` changes or it is taken out of the page and put back; a renderer that failed to download is downloaded again then.

### Events

| Event | When | `detail` |
|---|---|---|
| `isotrail-ready` | the diorama has drawn its first frame and replaced the poster | `{ backend }`: `"webgpu"` or `"gl"` |
| `isotrail-error` | the element gave up and shows its poster or a message | `{ reason, message }`, with `reason` from the table above |

Both bubble, and cross shadow roots:

```js
document.addEventListener('isotrail-error', (event) => {
  console.log(event.target.getAttribute('src'), event.detail.reason);
});
```

### Data credit

Once the diorama replaces the poster, a small line along its bottom edge credits the data, as the licences require: OpenStreetMap (ODbL), the terrain tiles, and Open-Meteo (CC BY 4.0) when the hike has weather. The text comes from the bundle, so it is the same as the credit drawn on stills. "OpenStreetMap" links to its copyright page and "Open-Meteo.com" to its site.

The line can be restyled with `::part(credit)`. If you hide it, with `credit="none"` or with CSS, the page must credit the same sources elsewhere, close to the embed, for instance in a caption under it. `isotrail embed --no-attribution` prints the text to use.

### Controls

| | Mouse and trackpad | Touch | Keyboard (the diorama focused) |
|---|---|---|---|
| Turn | drag (Shift-drag or right-drag when zoomed in) | drag, or twist two fingers | ← → |
| Zoom (up to 3×) | Ctrl/⌘ + scroll, pinch, or the − + buttons | pinch | + − |
| Move around when zoomed in | drag | drag | Shift + arrows |
| Back to the start view | the reset button | the reset button | 0 or Home |
| Play and pause | the play button | the play button | Space or K |

A plain mouse wheel keeps scrolling the page, and shows a reminder to hold Ctrl (⌘ on a Mac). Playing draws the route over about eight seconds, while the camera settles back to the start view as in the animated exports. The line along the diorama's bottom edge shows how much of the route is drawn, in the route's colour; click or drag along it to scrub, or focus it and use the arrow keys. The light follows the time at the route's head, through any nights of a multi-day trek, as in the animated exports; until the visitor plays or scrubs, the diorama shows the light at the start, as on the poster. The weather follows the same clock, and rain and snow keep falling while the diorama is on screen, unless the visitor prefers reduced motion. The season, with its snow and ice, is the one saved in the bundle; `isotrail embed --season` saves another. The light has the same depth as in the stills, with beams of sunlight when the sun breaks through a partly cloudy sky. The play button sits in the bottom-left corner, and the zoom and reset buttons in the bottom-right. Zooming never shows more than the whole slab, and the zoom centres on the pointer.

Scripts can call `play()` and `pause()` on the element, and read `playing`.

### Styling

The element is a block that fills its container's width. These custom properties change its colours:

```css
isotrail-hike {
  --isotrail-background: transparent;   /* behind the diorama */
  --isotrail-panel: rgb(32 46 55 / 0.78); /* buttons */
  --isotrail-edge: #819796;             /* the buttons' lit top edge */
  --isotrail-text: #ebede9;             /* icons, and the head of the progress line */
  --isotrail-accent: #a4dddb;           /* focus rings and the data credit */
  --isotrail-progress: #d64834;         /* the route drawn so far; route-colour by default */
  --isotrail-track: rgb(32 46 55 / 0.45); /* the route still to come */
  border-radius: 6px;
}
```

The parts `canvas`, `play`, `scrubber` (the progress line), `zoom-in`, `zoom-out`, `reset` and `credit` can be styled with `::part()`. The element sets `data-state` to `poster`, `loading`, `ready` or `fallback`.

On touch screens the buttons and the progress line take touches across 44 and 24 pixels, though they look the same size.

## Hosting checklist

- **MIME types.** Serve `.wasm` as `application/wasm` and `.js` as `text/javascript`. Most servers and static hosts already do; with another type for `.wasm`, the renderer still loads, but more slowly and with a warning in the console. `.itb` can be `application/octet-stream`.
- **Caching.** `isotrail-embed.js`, `pkg/` and the bundles can be cached for a long time, because the snippet's `?v=` changes with every isotrail version. When you update isotrail, update the runtime and remake the snippets (and bundles, if the bundle format changed) together.
- **Content-Security-Policy.** The element needs `script-src` to allow the script's origin and `'wasm-unsafe-eval'` (to compile the renderer), `connect-src` to allow the origins of the bundle and of `pkg/`, and `img-src` the poster's. It needs no inline scripts, and its styles need no `'unsafe-inline'`. This policy works when everything is served by the site itself:

  ```text
  default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self'; img-src 'self'; connect-src 'self'
  ```

  The `style` attributes in the snippet are inline styles, which `style-src 'self'` blocks: the element then works as usual, but a page without the script doesn't reserve its height. Remove the two `style` attributes and add a rule like `isotrail-hike { display: block; aspect-ratio: 16 / 9 } isotrail-hike > img { display: block; width: 100% }` to your stylesheet instead.
- **Secure pages.** Browsers only offer WebGPU on HTTPS pages (and `localhost`); elsewhere the element uses WebGL2.

## The demo

Serve the `web/` folder with any static file server, and open `/demo/`:

```bash
python -m http.server -d web 8000    # then open http://localhost:8000/demo/
```

The page shows the synthetic hike twice: wide with a poster, and square, playing on its own. A link at the top switches both to the WebGL2 fallback (`?backend=webgl` in the address).

To try it on a phone on the same network, serve it to the network with `python -m http.server -d web 8000 --bind 0.0.0.0` and open `http://<your computer's address>:8000/demo/` on the phone. Browsers only offer WebGPU on secure pages (HTTPS or `localhost`), so a plain `http://` address like this uses WebGL2.

## JavaScript API

The element is built on this lower-level API, which draws into a canvas you manage yourself:

```js
import init, { Viewer } from './pkg/isotrail_web.js';

await init();
const bytes = new Uint8Array(await (await fetch('hike.itb')).arrayBuffer());
const viewer = await Viewer.load(canvas, bytes, 'auto', { palette: 'aap-64' });   // or 'webgl'; the look is optional

viewer.resize(canvas.width, canvas.height);   // the drawing buffer, in device pixels
viewer.setProgress(0.5);                      // how far through drawing the route, 0 to 1; sets the light too
viewer.setYaw(30);                            // degrees turned from the still image's view
viewer.zoomBy(1.5, x, y);                     // zoom, keeping canvas pixel (x, y) in place
viewer.pan(dx, dy);                           // move with a drag of (dx, dy) canvas pixels
viewer.resetView();                           // back to the starting view
viewer.setTime(performance.now() / 1000);     // moves rain, snow and clouds on: any steadily rising clock, in seconds
viewer.setOptions({ mesher: 'smooth', time: '20:30' });   // changes the look and starting view
viewer.draw();
console.log(viewer.backend, viewer.zoom, viewer.title);   // "webgpu" or "gl", 1 to 3, the hike's name
console.log(viewer.credit);                   // the data credits, joined with " · "
console.log(viewer.weatherMoves);             // true while rain or snow falls, so frames keep changing
viewer.onLost((message) => { ... });          // the GPU was lost: free the viewer and load again
viewer.free();                                // releases the GPU memory at once
```

Nothing is drawn until the first `resize`. Until the first `setProgress`, the whole route shows under the light at the start, as on the poster; `setProgress` draws the route at an even speed with a pause for each night of a multi-day trek, lit by the time at its head. Each art pixel covers a whole number of device pixels, chosen so that the frame's long side is at most 640 pixels, as in a still rendered at `--scale 3`. Call `resize` again whenever the canvas changes size, and `draw` after any change. While `weatherMoves` is true, keep calling `setTime` and `draw` every frame for the rain and snow to fall.

The look is an object with any of `palette` (a built-in name, `none`, or the text of a `.hex` file: the element fetches files for you, the API doesn't), `mesher`, `blockHeight`, `occlusion`, `softShadows`, `godRays`, `background` and `time`, and the starting view's `turn`, `pitch` and `zoom`, taking the same values as the element's attributes, as strings or numbers. Each one left out or unusable takes its default; unusable ones also warn in the console. `setOptions` replaces the whole look, and throws an error with reason `"gpu"` if the GPU can't hold the terrain a new mesher builds.

Arguments that aren't finite numbers, such as `NaN`, are ignored. `Viewer.load` rejects with an `Error` whose `reason` is `"version"`, `"damaged"` or `"gpu"`, as in [When something fails](#when-something-fails); `resize` throws one with reason `"gpu"` if the GPU can't hold the diorama at that size. After any error from the viewer, free it. `onLost` is only called on WebGPU; on WebGL2, listen for `webglcontextlost` on the canvas.

## Testing the element

`web/test/check.mjs` loads synthetic hikes into headless Chrome or Edge and checks the element: the credit, the focus ring, each failure's message and event, a lost GPU, twenty embeds on one page, touch targets, a strict Content-Security-Policy, and that playing creates no GPU objects. It needs Node 22 or later and the runtime built with `sh web/build.sh`:

```bash
node web/test/check.mjs chrome           # WebGPU where the browser has it
node web/test/check.mjs edge --webgl     # every embed on WebGL2, drawn by SwiftShader
```

`ISOTRAIL_BROWSER` sets the browser's path if it isn't in the usual place. CI runs it in Chrome with `--webgl`.
