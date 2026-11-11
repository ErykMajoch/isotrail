# Configuration

## Config files

Settings you use every time can go in a TOML file instead of on the command line. Settings for `render` go in a `[render]` table, named after its long [options](usage.md#render) without the leading dashes:

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
season = "autumn"
god-rays = "on"
orbit = 0                      # animation settings only apply to .gif and .mp4 output
```

### The other commands

`embed`, `bundle` and `style-lab` read the same files. Each takes the `[render]` settings that are its own options too, so one file gives your web embeds and renders the same look, and ignores the rest:

| Command | Takes from `[render]` |
| --- | --- |
| `embed` | Everything that is also an `embed` option: the look, text and overlays, data, `scale` (for the poster) and `aspect` when it names one shape |
| `bundle` | `title`, `exaggeration` and the data settings |
| `style-lab` | `exaggeration` and the data settings only, so the palette and scale you render with don't narrow its comparison |

A command's own table, `[embed]`, `[bundle]` or `[style-lab]`, holds its own options and wins over `[render]` for that command:

```toml
[render]
palette = "resurrect-64"       # renders and embeds
aspect = ["story", "feed"]     # renders only: an embed has one shape

[embed]
aspect = "wide"
base-url = "/hikes/"
with-runtime = true

[style-lab]
palette = ["apollo", "resurrect-64", "warm.hex"]
scale = [3, 5]
```

isotrail reads two files, if they exist:

1. **Your user config file**, used for every command:

   | System | Location |
   | --- | --- |
   | Windows | `%APPDATA%\isotrail\config.toml` |
   | Linux | `~/.config/isotrail/config.toml` (or `$XDG_CONFIG_HOME/isotrail/config.toml`) |
   | macOS | `~/Library/Application Support/isotrail/config.toml` |

   Set `ISOTRAIL_CONFIG` to use a different file, or set it to an empty value to ignore the user file. A file named by `ISOTRAIL_CONFIG` must exist.

2. **A file given with `--config`**, for one run. This is handy for keeping one file per project or per hike.

### Precedence

From highest to lowest:

1. Options on the command line, and environment variables such as `ISOTRAIL_OVERPASS_URL`
2. The `--config` file
3. The user config file
4. Built-in defaults

Within one file, the command's own table wins over `[render]`. A switch set to `true` in the user file can be turned off again with `false` in the `--config` file.

An option you type also wins over settings it can't be combined with: with `date-format = "short"` in a file, `--date "June 2025"` on the command line prints that text, and the setting is left out.

### Rules

- Only the `[render]`, `[embed]`, `[bundle]` and `[style-lab]` tables are allowed, so typos don't go unnoticed.
- Each table only takes its command's options: an unknown key is an error, and so are `output`, `config`, `debug-dir`, `verbose`, `help` and `version`, which only make sense for one run.
- A bad value is reported with the name of the file it came from, for example `scale in hike.toml: must be 2, 3, 5, 6 or 10`.
- `--background` in a config file keeps its exact colour, just as it does on the command line.
- `time` can be a string (`"07:30"`) or a TOML time (`07:30:00`).
- Mistakes in a config file stop the command with exit code 2, like mistakes on the command line.

## Environment variables

| Variable | Effect |
| --- | --- |
| `ISOTRAIL_CONFIG` | Path of the user config file, which must exist. Empty means none |
| `ISOTRAIL_CACHE_DIR` | Directory for downloaded elevation, map data and weather (see [`cache`](usage.md#cache)) |
| `ISOTRAIL_OVERPASS_URL` | Overpass API endpoint for OpenStreetMap data, the same as `--overpass-url` |
| `ISOTRAIL_GPU` | `auto` (default), `hardware` or `software`. `software` forces a software renderer (WARP on Windows, lavapipe on Linux), which is useful on servers without a GPU |
