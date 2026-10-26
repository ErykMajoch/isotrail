# Contributing to isotrail

Thanks for your interest in `isotrail`! Bug reports, ideas and pull requests are all welcome.

## Reporting a bug

Please [open an issue](https://github.com/ErykMajoch/isotrail/issues/new/choose) with the isotrail version (`isotrail --version`), your system, the command you ran, and the output with `-v`. A render showing the problem helps a lot. If you can, attach the scene bundle (`isotrail bundle hike.gpx -o hike.itb`) rather than the GPX file. You don't have to share your recording.

## Building

Install Rust with [rustup](https://rustup.rs/): the repository's `rust-toolchain.toml` selects the right toolchain, components and targets. On Windows, the build also needs the Visual Studio C++ build tools.

```bash
cargo build                       # everything
cargo run -p isotrail-cli -- render fixtures/gpx/snowdon.gpx -o out.png
```

The workspace is split into crates:

| Crate | Contents |
| --- | --- |
| `isotrail-track` | GPX parsing, cleaning and statistics |
| `isotrail-bundle` | The scene bundle (`.itb`) types and file format |
| `isotrail-geo` | Projection, slab and grid, elevation and OpenStreetMap sources, landcover, caching |
| `isotrail-render` | The wgpu renderer. Meshers, camera, lighting, post-processing, text and overlays |
| `isotrail-export` | Rendering frames without a window, and PNG, GIF and MP4 output |
| `isotrail-cli` | The `isotrail` command |
| `isotrail-web` | The future web embed |
| `isotrail-testkit` | Synthetic scenes, golden-image, GIF and video helpers for tests |

`isotrail-track`, `isotrail-bundle` and `isotrail-render` must stay WebAssembly-safe, meaning no filesystem, network or threads.

## Checks

Pull requests must pass the same checks as CI:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo check --target wasm32-unknown-unknown -p isotrail-track -p isotrail-bundle -p isotrail-render
```

Please don't add real recordings to the repository.

## Rendering in tests

Rendering runs on the GPU without a window. It uses a real GPU when there is one, and falls back to a software renderer otherwise. Set `ISOTRAIL_GPU` to `hardware` or `software` to choose. CI uses `software`: WARP on Windows, and Mesa lavapipe on Linux (the `mesa-vulkan-drivers` package).

Render tests compare their output with golden images in `fixtures/golden/`, and allow up to 0.5% of pixels to differ. When a comparison fails, the actual image and a diff are written to `target/golden-failures/`. After an intended change to the look, regenerate the goldens and review the new images before committing them:

```bash
UPDATE_GOLDENS=1 cargo test
```

The MP4 tests need `ffmpeg` and `ffprobe` on your `PATH`. Without them, they print a note and pass. CI installs ffmpeg on Linux.
