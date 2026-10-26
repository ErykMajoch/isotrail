# Configuration

## Config files

Settings you use for every render can go in a TOML file instead of on the command line. Settings go in a `[render]` table, named after the long [`render` options](usage.md#render) without the leading dashes:

```toml
[render]
aspect = ["story", "square"]   # comma-separated options become lists
scale = 3                      # numbers stay numbers
palette = "resurrect-64"
units = "imperial"
date-format = "short"
stats = ["distance", "gain", "elapsed"]
no-attribution = true          # switches are true or false
background = "#1e2a33"
orbit = 0                      # animation settings only apply to .gif and .mp4 output
```

isotrail reads two files, if they exist:

1. **Your user config file**, used for every render:

   | System | Location |
   | --- | --- |
   | Windows | `%APPDATA%\isotrail\config.toml` |
   | Linux | `~/.config/isotrail/config.toml` (or `$XDG_CONFIG_HOME/isotrail/config.toml`) |
   | macOS | `~/Library/Application Support/isotrail/config.toml` |

   Set `ISOTRAIL_CONFIG` to use a different file, or set it to an empty value to ignore the user file.

2. **A file given with `--config`**, for one render. This is handy for keeping one file per project or per hike.

### Precedence

From highest to lowest:

1. Options on the command line, and environment variables such as `ISOTRAIL_OVERPASS_URL`
2. The `--config` file
3. The user config file
4. Built-in defaults

A switch set to `true` in the user file can be turned off again with `false` in the `--config` file.

### Rules

- Anything outside the `[render]` table is an error, so typos don't go unnoticed.
- Unknown keys are errors, and so are `output` and `config`, which only make sense for one run.
- A bad value is reported with the name of the file it came from, for example `scale in hike.toml: must be 2, 3, 5, 6 or 10`.
- `--background` in a config file keeps its exact colour, just as it does on the command line.

## Environment variables

| Variable | Effect |
| --- | --- |
| `ISOTRAIL_CONFIG` | Path of the user config file. Empty means none |
| `ISOTRAIL_CACHE_DIR` | Directory for downloaded elevation and map data (see [`cache`](usage.md#cache)) |
| `ISOTRAIL_OVERPASS_URL` | Overpass API endpoint for OpenStreetMap data, the same as `--overpass-url` |
| `ISOTRAIL_GPU` | `auto` (default), `hardware` or `software`. `software` forces a software renderer (WARP on Windows, lavapipe on Linux), which is useful on servers without a GPU |
