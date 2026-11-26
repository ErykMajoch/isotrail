# Contributing to isotrail

Bug reports, ideas and pull requests are welcome.

## Reporting a bug

Please [open an issue](https://github.com/ErykMajoch/isotrail/issues/new/choose) with the isotrail version (`isotrail --version`), your system, the command you ran, and the output with `-v`. A render that shows the problem helps a lot.

If you can, attach the scene bundle (`isotrail bundle hike.gpx -o hike.itb`) rather than the GPX file. You don't have to share your recording.

## Building

Install Rust with [rustup](https://rustup.rs/). The repository's `rust-toolchain.toml` selects the right toolchain, components and targets. On Windows, the build also needs the Visual Studio C++ build tools.

```bash
cargo build
cargo run -p isotrail-cli -- render fixtures/gpx/snowdon.gpx -o out/snowdon.png
```

The first render downloads a few elevation tiles and map data, so it takes longer. Anything you make while working goes in `out/`, which git ignores.

The workspace is split into crates:

| Crate | Contents |
| --- | --- |
| `isotrail-track` | GPX parsing, cleaning and statistics |
| `isotrail-bundle` | The scene bundle (`.itb`) types and file format |
| `isotrail-geo` | Projection, slab and grid, elevation, map and weather sources, landcover, caching, scene building |
| `isotrail-render` | The wgpu renderer: meshers, camera, lighting, post-processing, text and overlays |
| `isotrail-export` | Finished stills and clips, and PNG, GIF and MP4 output |
| `isotrail-options` | The render options, read from flags, config files or JSON |
| `isotrail-cli` | The `isotrail` command |
| `isotrail-web` | The WebAssembly renderer behind the `<isotrail-hike>` element in `web/` ([`web/README.md`](web/README.md)) |
| `isotrail-scene` | The WebAssembly scene builder that runs in a browser worker, in `web/scene/` |
| `isotrail-testkit` | Synthetic scenes, golden-image, GIF and video helpers for tests |

`isotrail-track`, `isotrail-bundle` and `isotrail-render` must stay WebAssembly-safe. They can't use the filesystem, the network or threads. `isotrail-geo` builds for WebAssembly too, without its HTTP client and disk cache. So does `isotrail-export`, without reading frames back from the GPU. `isotrail-options` builds for WebAssembly without reading palette and theme files.

The steps from a track to a scene bundle live in `isotrail-geo`. The command adds the disk cache, the server options and its own messages. In a browser, `isotrail-scene` runs the same steps in a worker, with the answers the page fetches. The worker keeps those answers in the browser's cache, under the keys the disk cache uses. It asks several Overpass servers in turn, listed in `crates/isotrail-scene/src/servers.rs`. The text over the diorama is chosen in `isotrail-render`, and `isotrail-export` draws the finished stills and clips. The render options are defined once, in `isotrail-options`. Flags, config files and JSON go through the same checks there. The command adds its other flags and words the warnings.

Code that builds a scene takes its sines, cosines and the rest from the `libm` crate. A scene then comes out bit for bit the same on every system and in a browser. A `clippy.toml` in `isotrail-track`, `isotrail-geo` and `isotrail-bundle` rejects the standard library's versions.

Please don't add real recordings to the repository. Tests use synthetic routes only.

## Before a pull request

1. Build, as above.
2. Run the same checks as CI:

   ```bash
   cargo fmt --all --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   cargo clippy --target wasm32-unknown-unknown -p isotrail-track -p isotrail-bundle -p isotrail-geo -p isotrail-render -p isotrail-export -p isotrail-options -p isotrail-web -p isotrail-scene -- -D warnings
   cargo clippy --target wasm32-unknown-unknown -p isotrail-web --features export -- -D warnings
   cargo deny check
   ```

3. If you changed the renderer or anything in `web/`, build the web runtime and run the browser check (see [Testing the element](web/README.md#testing-the-element)):

   ```bash
   sh web/build.sh
   node web/test/check.mjs chrome
   ```

4. If you changed how a scene is built, or anything in `web/scene/`, build the scene worker and run its check. A local server plays the elevation, map and weather services from `fixtures/`. The check builds each scene in the browser and compares it with the one `isotrail bundle` makes. It also makes the server busy, slow or silent, to check the browser's cache, the waits, cancel and the messages.

   ```bash
   sh web/build-scene.sh
   node web/test/scene.mjs chrome
   ```

5. If you changed the renderer or how stills and clips are drawn, build the web app's renderer and run its check. It reads stills and animation frames in the browser and compares them with the ones isotrail draws natively. On Windows the native ones are drawn through Direct3D 12, as the browser draws them.

   ```bash
   sh web/build-app.sh
   node web/test/export.mjs chrome
   ```

6. If you changed anything in `site/`, build the web app and run the same check on its page, as [The web app](#the-web-app) explains.
7. Open the pull request.

### Notes on the checks

[`cargo deny`](https://github.com/EmbarkStudios/cargo-deny) looks for security advisories, licences that aren't listed in `deny.toml`, and crates from outside crates.io. Install it with `cargo install --locked cargo-deny`. A new dependency with a licence that isn't listed yet needs it added to both `deny.toml` and `about.toml`.

Release archives carry `THIRD-PARTY-NOTICES.md`, with the licence of every crate in the binary and the web runtime. [`cargo about`](https://github.com/EmbarkStudios/cargo-about) makes it from `about.toml` and `about.hbs`. To see it, install it with `cargo install --locked cargo-about --features cli` and run:

```bash
cargo about generate --locked --fail about.hbs -o out/THIRD-PARTY-NOTICES.md
```

CI fails if `isotrail-embed.js`, `pkg/isotrail_web.js` and `pkg/isotrail_web_bg.wasm` come to more than 1.1MB gzipped. After `sh web/build.sh`, a local build of `isotrail embed` finds the runtime with `--runtime-dir web`. The scene worker and the web app's renderer have no limit, but CI reports their sizes.

isotrail asks the GPU for WebGL2's limits on every platform, so the native tests also catch anything the browser fallback couldn't draw.

The toolchain is pinned to one Rust release, so CI and the release builds don't change compiler on their own. To move to a newer one, change `channel` in `rust-toolchain.toml` and run all of the steps above. A new release often brings new lints.

## The web app

The web app in `site/` builds and draws a hike in the browser. It is an [Astro](https://astro.build/) project with plain scripts, and needs Node 24 and [wasm-pack](https://rustwasm.github.io/wasm-pack/). Install its packages once:

```bash
cd site
npm ci
```

Then build it from the repository's root. This builds the web runtime and the scene worker, then the pages, into `site/dist`:

```bash
sh site/build.sh
```

The build copies the runtime into `site/public/runtime/`, in a folder named by a hash of its files. Browsers can then keep those files for a year. `site/public/_headers` sets that cache, the type of the `.wasm` files and the Content-Security-Policy. The policy only lets the page reach the elevation, map and weather services.

The page's controls are made from the tool's own option definitions. The build writes them to `site/src/options.json` first, so a new option reaches the page with the next build. `site/src/scripts/controls.js` says where each option goes on the page. An option it doesn't place fails the page check:

```bash
cargo run -p isotrail-options --example describe -- site/src/options.json
```

While working on the pages, run `npm run dev` in `site/`. It serves the pages and reloads them as you edit. `npm run preview` serves the built site instead, and `npm run size` lists its files with their sizes.

The scene check runs on the built site's page too. Its local server sends the headers from `_headers`, so a policy that blocks the page fails the check:

```bash
node web/test/scene.mjs chrome --site site/dist
```

A second check works through the page as a visitor would. It drops GPX files on it, opens the sample walks and uses the keyboard alone. It also checks that each failure shows its message, and that each control changes the diorama:

```bash
node site/test/page.mjs chrome
```

Add `--shots out/page` to save a screenshot of each state of the page.

The sample walks in `site/public/samples/` are made up. Each was traced along paths mapped on OpenStreetMap, with invented times. Each comes as a GPX file and a scene file built from it with the default options. After a change to the scene file format, build the scene files again:

```bash
sh site/scripts/samples.sh
```

CI builds the site, reports its size and runs both checks. The site is deployed by hand only. The Site workflow on GitHub builds it and uploads it to Cloudflare Pages, as a preview or as the public site.

## Rendering in tests

Rendering runs on the GPU without a window. It uses a real GPU when there is one, and a software renderer otherwise. Set `ISOTRAIL_GPU` to `hardware` or `software` to choose.

CI uses `software`: WARP on Windows, and Mesa lavapipe on Linux (the `mesa-vulkan-drivers` package). Tests take the GPU from `isotrail_testkit::gpu()`, which opens one device per test binary and lends it to one test at a time. Lavapipe has crashed when every test opened its own.

Render tests compare their output with golden images in `fixtures/golden/`, and allow up to 0.5% of pixels to differ. When a comparison fails, the actual image and a diff are written to `target/golden-failures/`. After an intended change to the look, regenerate the goldens and review the new images before committing them:

```bash
UPDATE_GOLDENS=1 cargo test
```

The MP4 tests need `ffmpeg` and `ffprobe` on your `PATH`. Without them, they print a note and pass. CI installs ffmpeg on Linux.

## Changing the scene file format

Websites keep serving scene files after isotrail is updated. So isotrail and the web runtime read the format before the current one too, and convert it on load. The payload has no field names, so any change to the bundle's types is a new format. It needs these steps:

1. Replace `crates/isotrail-bundle/src/previous.rs` with a copy of the current types, the whole bundle as it is now. Convert them to the new types in its `From` implementation.
2. Raise `FORMAT_VERSION` in `format.rs`. `OLDEST_FORMAT_VERSION` follows `previous.rs`.
3. Delete the oldest file in `fixtures/bundles/`. Write the new one with `cargo run -p isotrail-testkit --example format_fixture`.
4. Say in `CHANGELOG.md` that the format changed.

The tests in `crates/isotrail-bundle/tests/formats.rs` fail until all of this is done.
