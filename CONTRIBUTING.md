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
| `isotrail-geo` | Projection, slab and grid, elevation and OpenStreetMap sources, landcover, caching |
| `isotrail-render` | The wgpu renderer: meshers, camera, lighting, post-processing, text and overlays |
| `isotrail-export` | Rendering frames without a window, and PNG, GIF and MP4 output |
| `isotrail-cli` | The `isotrail` command |
| `isotrail-web` | The WebAssembly renderer behind the `<isotrail-hike>` element in `web/` ([`web/README.md`](web/README.md)) |
| `isotrail-testkit` | Synthetic scenes, golden-image, GIF and video helpers for tests |

`isotrail-track`, `isotrail-bundle` and `isotrail-render` must stay WebAssembly-safe. They can't use the filesystem, the network or threads.

Please don't add real recordings to the repository. Tests use synthetic routes only.

## Before a pull request

1. Build, as above.
2. Run the same checks as CI:

   ```bash
   cargo fmt --all --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   cargo clippy --target wasm32-unknown-unknown -p isotrail-track -p isotrail-bundle -p isotrail-render -p isotrail-web -- -D warnings
   cargo deny check
   ```

3. If you changed the renderer or anything in `web/`, build the web runtime and run the browser check (see [Testing the element](web/README.md#testing-the-element)):

   ```bash
   sh web/build.sh
   node web/test/check.mjs chrome
   ```

4. Open the pull request.

### Notes on the checks

[`cargo deny`](https://github.com/EmbarkStudios/cargo-deny) looks for security advisories, licences that aren't listed in `deny.toml`, and crates from outside crates.io. Install it with `cargo install --locked cargo-deny`. A new dependency with a licence that isn't listed yet needs it added to both `deny.toml` and `about.toml`.

Release archives carry `THIRD-PARTY-NOTICES.md`, with the licence of every crate in the binary and the web runtime. [`cargo about`](https://github.com/EmbarkStudios/cargo-about) makes it from `about.toml` and `about.hbs`. To see it, install it with `cargo install --locked cargo-about --features cli` and run:

```bash
cargo about generate --locked --fail about.hbs -o out/THIRD-PARTY-NOTICES.md
```

CI fails if `isotrail-embed.js`, `pkg/isotrail_web.js` and `pkg/isotrail_web_bg.wasm` come to more than 3MB gzipped. After `sh web/build.sh`, a local build of `isotrail embed` finds the runtime with `--runtime-dir web`.

isotrail asks the GPU for WebGL2's limits on every platform, so the native tests also catch anything the browser fallback couldn't draw.

The toolchain is pinned to one Rust release, so CI and the release builds don't change compiler on their own. To move to a newer one, change `channel` in `rust-toolchain.toml` and run all of the steps above. A new release often brings new lints.

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
