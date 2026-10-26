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

Serve `isotrail-embed.js` with the `pkg/` folder next to it, and add the element wherever the hike should appear:

```html
<script type="module" src="/isotrail/isotrail-embed.js"></script>

<isotrail-hike src="/hikes/derwent-edge.itb" poster="/hikes/derwent-edge.png" aspect="wide"></isotrail-hike>
```

Make the bundle with `isotrail bundle hike.gpx -o hike.itb`, and the poster with `isotrail render hike.itb -o hike.png --aspect wide`.

| Attribute | Meaning |
|---|---|
| `src` | The scene bundle (`.itb`) to show |
| `poster` | An image shown straight away, and kept if the browser can't draw the diorama |
| `aspect` | `story` (9:16), `feed` (4:5), `square` or `wide` (16:9, the default). The element fills its container's width at this aspect, with the control bar underneath |
| `autoplay` | Plays the route once as soon as the diorama is ready, unless the visitor prefers reduced motion |
| `backend` | `webgl` forces the WebGL2 fallback, for testing it in a browser that has WebGPU |

The poster shows at once. The renderer (about 3 MB) only downloads when the element comes near the screen, and it downloads once however many hikes are on the page. If the browser has neither WebGPU nor WebGL2, or the bundle can't be loaded, the poster stays and a warning goes to the console.

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
viewer.free();                                // releases the GPU resources
```

Nothing is drawn until the first `resize`. Each art pixel covers a whole number of device pixels, chosen so that the frame's long side is at most 640 pixels, as in a still rendered at `--scale 3`. Call `resize` again whenever the canvas changes size, and `draw` after any change.
