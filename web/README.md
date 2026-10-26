# isotrail on the web

The `isotrail-web` crate compiles the renderer to WebAssembly, so a scene bundle (`.itb`) can be drawn into a canvas on a web page. It uses WebGPU where the browser has it, and WebGL2 otherwise.

## Building

You need the Rust toolchain from the repository (it includes the `wasm32-unknown-unknown` target) and [wasm-pack](https://rustwasm.github.io/wasm-pack/):

```bash
cargo install wasm-pack
```

Then, from the repository root (on Windows, in Git Bash):

```bash
sh web/build.sh
```

This runs two steps, which you can also run by hand:

```bash
# The renderer, optimised with wasm-opt -Oz, into web/pkg/
wasm-pack build crates/isotrail-web --target web --release --out-dir ../../web/pkg

# A synthetic hike for the demo, into web/demo/hike.itb
cargo run -p isotrail-testkit --example demo_bundle -- web/demo/hike.itb
```

## The demo

Serve the `web/` folder with any static file server, and open `/demo/`:

```bash
python -m http.server -d web 8000    # then open http://localhost:8000/demo/
```

Sliders draw part of the route and turn the diorama. The graphics menu can force WebGL2, to try the fallback in a browser that has WebGPU. The same switch is `?backend=webgl` in the address.

## JavaScript API

```js
import init, { Viewer } from './pkg/isotrail_web.js';

await init();
const bytes = new Uint8Array(await (await fetch('hike.itb')).arrayBuffer());
const viewer = await Viewer.load(canvas, bytes, 'auto');   // or 'webgl'

viewer.resize(canvas.width, canvas.height);   // the drawing buffer, in device pixels
viewer.setProgress(0.5);                      // share of the route drawn, 0 to 1
viewer.setYaw(30);                            // degrees turned from the still image's view
viewer.draw();
console.log(viewer.backend);                  // "webgpu" or "gl"
viewer.free();                                // releases the GPU resources
```

Nothing is drawn until the first `resize`. Each art pixel covers a whole number of device pixels, chosen so that the frame's long side is at most 640 pixels, as in a still rendered at `--scale 3`. Call `resize` again whenever the canvas changes size, and `draw` after any change.
