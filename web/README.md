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
<script type="module" src="/hikes/derwent/isotrail-embed.js"></script>
<isotrail-hike src="/hikes/derwent/derwent-edge.itb" poster="/hikes/derwent/derwent-edge.png" aspect="wide"></isotrail-hike>
```

`--base-url` is the address the directory will be served at. Without it the snippet uses bare file names, so serving the directory and opening `snippet.html` shows a preview. `--aspect` picks the element's shape and its poster's, and the poster takes `render`'s overlay options, such as `--title` or `--overlay none`. [The usage guide](../docs/usage.md#embed) lists every option.

`--with-runtime` copies the web runtime from the `web` folder that comes next to `isotrail` in the release downloads, or from `--runtime-dir` (`--runtime-dir web` in the repository, after building it). The runtime is the same for every hike, so a site with several hikes only needs it once. Copy that `web` folder (`isotrail-embed.js` and `pkg/`) to one place, such as `static/isotrail/`, and point each hike's snippet at it instead:

```bash
isotrail embed derwent-edge.gpx -o static/hikes/derwent --base-url /hikes/derwent/ --runtime-url /isotrail/isotrail-embed.js
```

`isotrail-embed.js` loads the renderer from the `pkg/` folder next to it. A page that shows several hikes only loads the script and the renderer once.

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
| `aspect` | `story` (9:16), `feed` (4:5), `square` or `wide` (16:9, the default). The element fills its container's width at this aspect, with the control bar underneath |
| `autoplay` | Plays the route once as soon as the diorama is ready, unless the visitor prefers reduced motion |
| `backend` | `webgl` forces the WebGL2 fallback, for testing it in a browser that has WebGPU |

The poster shows at once. The renderer (about 3 MB) only downloads when the element comes near the screen, and it downloads once however many hikes are on the page. If the browser has neither WebGPU nor WebGL2, or the bundle can't be loaded, the poster stays and a warning goes to the console. If the GPU is lost while the page is open, for instance when its driver resets, the element starts again once and keeps the poster if that fails too.

### Controls

| | Mouse and trackpad | Touch | Keyboard (the diorama focused) |
|---|---|---|---|
| Turn | drag (Shift-drag or right-drag when zoomed in) | drag, or twist two fingers | ← → |
| Zoom (up to 3×) | Ctrl/⌘ + scroll, pinch, or the − + buttons | pinch | + − |
| Move around when zoomed in | drag | drag | Shift + arrows |
| Back to the start view | the reset button | the reset button | 0 or Home |
| Play and pause | the play button | the play button | Space or K |

A plain mouse wheel keeps scrolling the page, and shows a reminder to hold Ctrl (⌘ on a Mac). Playing draws the route over about eight seconds, while the camera settles back to the start view as in the animated exports. The slider under the diorama scrubs along the route. Zooming never shows more than the whole slab, and the zoom centres on the pointer.

Scripts can call `play()` and `pause()` on the element, and read `playing`.

### Styling

The element is a block that fills its container's width. These custom properties change its colours:

```css
isotrail-hike {
  --isotrail-background: transparent;   /* behind the diorama */
  --isotrail-panel: rgb(32 46 55 / 0.78); /* control bar */
  --isotrail-text: #ebede9;             /* icons */
  --isotrail-accent: #a4dddb;           /* slider and focus rings */
  border-radius: 6px;
}
```

The parts `canvas`, `play`, `scrubber`, `zoom-in`, `zoom-out` and `reset` can be styled with `::part()`. The element sets `data-state` to `poster`, `loading`, `ready` or `fallback`.

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
const viewer = await Viewer.load(canvas, bytes, 'auto');   // or 'webgl'

viewer.resize(canvas.width, canvas.height);   // the drawing buffer, in device pixels
viewer.setProgress(0.5);                      // share of the route drawn, 0 to 1
viewer.setYaw(30);                            // degrees turned from the still image's view
viewer.zoomBy(1.5, x, y);                     // zoom, keeping canvas pixel (x, y) in place
viewer.pan(dx, dy);                           // move with a drag of (dx, dy) canvas pixels
viewer.resetView();                           // the whole slab, from the still image's view
viewer.draw();
console.log(viewer.backend, viewer.zoom, viewer.title);   // "webgpu" or "gl", 1 to 3, the hike's name
viewer.onLost((message) => { ... });          // the GPU was lost: free the viewer and load again
viewer.free();                                // releases the GPU memory at once
```

Nothing is drawn until the first `resize`. Each art pixel covers a whole number of device pixels, chosen so that the frame's long side is at most 640 pixels, as in a still rendered at `--scale 3`. Call `resize` again whenever the canvas changes size, and `draw` after any change.
