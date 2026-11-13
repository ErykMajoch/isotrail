# isotrail on the web

The `<isotrail-hike>` element puts an interactive diorama of a hike on any web page, static sites included. Visitors can turn it, zoom in, and watch the route draw itself. It draws with WebGPU where the browser has it, and with WebGL2 otherwise.

## Embed a hike

`isotrail embed` makes everything a page needs from a GPX file or a `.itb` scene bundle:

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

`snippet.html` holds the two lines that show the hike. The command prints them too:

```html
<script type="module" src="/hikes/derwent/isotrail-embed.js?v=1.4.1"></script>
<isotrail-hike src="/hikes/derwent/derwent-edge.itb" poster="/hikes/derwent/derwent-edge.png" aspect="wide" style="display:block;aspect-ratio:16/9"><img src="/hikes/derwent/derwent-edge.png" alt="Diorama of Derwent Edge" style="display:block;width:100%"></isotrail-hike>
```

The image inside the element is for pages where the script doesn't run, because it is blocked, missing or still loading. The element already has its size and shows the poster, so nothing jumps when the script arrives. Once the script runs, the element shows its own copy of the poster and hides this one.

The `?v=` on the script's address is the isotrail version, and the script passes it on to the renderer in `pkg/`. After you update isotrail and remake the snippets, browsers fetch the new runtime instead of mixing old cached files with new ones.

`--base-url` is the address the folder will be served at. Without it, the snippet uses bare file names, so you can serve the folder and open `snippet.html` to preview it. `--aspect` sets the shape of the element and its poster. The poster also takes `render`'s overlay options, such as `--title` or `--overlay none`. [The usage guide](../docs/usage.md#embed) lists every option.

### A poster the live view replaces without a jump

The live diorama starts at the poster's size. It shrinks only as far as each angle needs while it turns. With `--overlay none` and a shape with 5% margins, such as `wide`, the poster is the live view's first frame. Both must be drawn at the same size for that.

The live view draws one pixel for each device pixel while the element's longer side is at most 640, and coarser pixels above that. For an element shown at 640 x 360 on a screen with one device pixel per CSS pixel, make the poster like this:

```sh
isotrail embed hike.gpx -o site --size 640x360 --scale 1 --overlay none
```

A still you make with `render` and the same options matches too. A poster with the title and stats panels doesn't, because the panels move its diorama.

### One runtime for several hikes

`--with-runtime` copies the web runtime from the `web` folder that sits next to `isotrail` in the release downloads. In the repository, build it first and add `--runtime-dir web`.

The runtime is the same for every hike. On a site with several hikes, copy that `web` folder (`isotrail-embed.js` and `pkg/`) to one place, such as `static/isotrail/`, and point each snippet at it:

```bash
isotrail embed derwent-edge.gpx -o static/hikes/derwent --base-url /hikes/derwent/ --runtime-url /isotrail/isotrail-embed.js
```

`isotrail-embed.js` loads the renderer from the `pkg/` folder next to it. A page with several hikes loads the script and the renderer once, as long as every snippet uses the same script address, `?v=` included. `--runtime-url` gets `?v=` too, unless the address you give already has a query.

### Hugo

Put the files under `static/`, with the runtime in `static/isotrail/`. Then add a shortcode in `layouts/shortcodes/hike.html`:

```go-html-template
{{- $version := "1.4.1" -}}
{{- $aspect := .Get "aspect" | default "wide" -}}
{{- $ratios := dict "story" "9/16" "video" "9/16" "portrait" "3/4" "feed" "4/5" "square" "1/1" "wide" "16/9" -}}
{{- $ratio := index $ratios $aspect | default (replace $aspect ":" "/") -}}
<script type="module" src="/isotrail/isotrail-embed.js?v={{ $version }}"></script>
<isotrail-hike src="{{ .Get "src" }}" poster="{{ .Get "poster" }}" aspect="{{ $aspect }}"{{ if .Get "autoplay" }} autoplay{{ end }} style="{{ printf "display:block;aspect-ratio:%s" $ratio | safeCSS }}"><img src="{{ .Get "poster" }}" alt="{{ .Get "alt" | default "Hike diorama" }}" style="display:block;width:100%"></isotrail-hike>
```

Use it in any post:

```markdown
{{< hike src="/hikes/derwent/derwent-edge.itb" poster="/hikes/derwent/derwent-edge.png" alt="Diorama of Derwent Edge" >}}
```

When you update the runtime, change `$version` to the new isotrail version.

### Astro

Put the files under `public/`, with the runtime in `public/isotrail/`. Then add a component in `src/components/Hike.astro`:

```astro
---
const { src, poster, alt = 'Hike diorama', aspect = 'wide', autoplay = false } = Astro.props;
const version = '1.4.1';
const ratios = { story: '9/16', video: '9/16', portrait: '3/4', feed: '4/5', square: '1/1', wide: '16/9' };
const ratio = ratios[aspect] ?? aspect.replace(':', '/');
---
<script is:inline type="module" src={`/isotrail/isotrail-embed.js?v=${version}`}></script>
<isotrail-hike src={src} poster={poster} aspect={aspect} autoplay={autoplay} style={`display:block;aspect-ratio:${ratio}`}><img src={poster} alt={alt} style="display:block;width:100%"></isotrail-hike>
```

`is:inline` stops Astro from bundling the script, which has to stay next to its `pkg/` folder. Use the component in a page or an MDX post:

```astro
---
import Hike from '../components/Hike.astro';
---
<Hike src="/hikes/derwent/derwent-edge.itb" poster="/hikes/derwent/derwent-edge.png" alt="Diorama of Derwent Edge" />
```

When you update the runtime, change `version` to the new isotrail version.

## Attributes

| Attribute | Meaning |
|---|---|
| `src` | The scene bundle (`.itb`) to show |
| `poster` | An image shown at once, and kept if the browser can't draw the diorama |
| `aspect` | The shape: `story`, `video`, `portrait`, `feed`, `square`, `wide` (the default) or a ratio such as `3:2` |
| `autoplay` | Plays as soon as the diorama is ready |
| `load` | `press` waits for the play button before it downloads anything |
| `backend` | `webgl` forces the WebGL2 fallback, to test it in a browser that has WebGPU |
| `credit` | Where the data credit goes: `overlay` (the default), `below` or `none` |
| `credit-extra` | A line of your own under the data credit, such as your name or site |

`story` and `video` are 9:16, `portrait` is 3:4, `feed` is 4:5 and `wide` is 16:9. The element fills its container's width at that shape.

`autoplay` does nothing for visitors who prefer reduced motion. `credit="below"` puts the credit under the diorama, and `credit="none"` hides it. Only use `none` when the page credits the data itself, as [Data credit](#data-credit) explains. `isotrail embed` writes `autoplay`, `load`, `credit` and `credit-extra` from its options of the same names.

### Loading on a press

With `load="press"`, the element shows its poster and a play button, and downloads nothing else. Pressing play downloads the renderer and the scene, then plays. Meanwhile the button shows three dots and keeps the keyboard focus.

The downloads start a little early, when the pointer comes to the button or the button takes focus. Browsers set to save data skip this. With reduced motion, a press shows the diorama without playing it.

Once pressed, the element behaves as it would without `load`. It frees its renderer off screen and starts again by itself. A new `src` waits for a press again. If `controls` leaves out `play`, start it from a script with `load()` or `play()`.

### Look

The look attributes choose how the diorama is drawn, like `isotrail render`'s options of the same names. `isotrail embed` writes one for each look option you give it that isn't the default, so the live diorama matches its poster.

| Attribute | Values (default first) |
|---|---|
| `palette` | `apollo`, `resurrect-64`, `aap-64`, `none`, or the address of a `.hex` file |
| `mesher` | `voxel` or `smooth` |
| `block-height` | `1`, or from `0.25` to `4` |
| `occlusion` | `1`, or from `0` to `1` |
| `soft-shadows` | `1.5`, or from `0` to `4` grid cells |
| `god-rays` | `auto`, `on` or `off` |
| `background` | `sky`, `transparent`, or a colour such as `#1e2a33` |
| `time` | The start of the hike, or a local time such as `20:30` |

A `.hex` file is a Lospec palette, such as `/hikes/warm.hex`. `block-height` is the height of a voxel block as a share of its width. With `background="transparent"`, the page shows through. `time` lights the diorama at that time on the day of the hike, and keeps that light while it plays.

### Scene, light and route

These work like `isotrail render`'s [scene, light and route options](../docs/usage.md#scene-light-and-route).

| Attribute | Values (default first) |
|---|---|
| `prop-density` | `1`, or from `0` (no trees, bushes or boulders) to `2` |
| `paths`, `streams`, `strata` | `on` or `off` |
| `base-blocks` | `6`, or from `1` to `24` |
| `outlines` | `1`, or from `0` to `2` |
| `shadows`, `night-sky` | `on` or `off` |
| `weather-effects` | `clouds,shade,rain,snow,haze`, fewer of them, or `none` |
| `weather` | The recorded weather, or `clear`, `cloudy`, `overcast`, `rain`, `snow` or `fog` |
| `sun` | The hike's sun, or a bearing and elevation in degrees, such as `250,8` |
| `route-colour` | `#d64834`, or another colour |
| `route-width` | `2`, or from `1` to `4` pixels |
| `route-hidden` | `dashed`, `dim`, `solid` or `off` |
| `markers` | `start,finish,head`, fewer of them, or `none` |

`paths` covers paths and roads from the map, and `strata` the layers on the slab's cut faces. `base-blocks` is how far the base sits below the lowest ground, in blocks. A `sun` lights the diorama the same way while it plays.

`route-color` works too. The progress line takes the route's colour, unless the page sets `--isotrail-progress`. `route-hidden` styles the parts of the route behind ridges.

### View

The view attributes set where the diorama starts. The reset button, 0 and Home go back there. `isotrail embed` writes them from its `--turn`, `--heading`, `--pitch` and `--zoom`, like `isotrail render`'s camera options.

| Attribute | Values (default first) |
|---|---|
| `turn` | `0`, or from `-360` to `360` degrees |
| `pitch` | `30`, or from `10` to `80` degrees |
| `zoom` | `1`, or from `0.5` to `2` |

`turn` moves the camera anticlockwise round the slab, as seen from above, from the view along the hike. `pitch` is how far the camera looks down. A `zoom` above 1 starts zoomed in, and below 1 it leaves more room round the slab.

### Playback

The playback attributes set how the route plays, like `isotrail render`'s [animation options](../docs/usage.md#animation). `isotrail embed` writes them from its [playback options](../docs/usage.md#playback-and-controls).

| Attribute | Values (default first) |
|---|---|
| `duration` | `10`, or from `1` to `180` seconds |
| `intro`, `hold` | 5% and 15% of `duration`, or from `0` to `180` seconds |
| `loop-back` | Between plays, the route undraws and the camera turns back |
| `rewind` | `1`, or from `0` to `10` seconds |
| `orbit` | `30`, or from `-360` to `360` degrees |
| `pace` | `distance`, or `time` to follow the recorded times |
| `loop` | How many times it plays: once when not set, until paused for `loop` or `loop="forever"`, or a number |

`duration` is one play, split as in an animation. Out of every 10s, 0.5s show the scene, 8s draw the route and 1.5s rest on the finished hike. That rest, the `hold`, only comes between loops, so a single play stops when the route is drawn. `intro` and `hold` must leave half a second to draw the route.

`orbit` is how far the camera turns while the route draws, easing back to the starting view. `0` keeps it still. With `loop-back`, one loop runs into the next without a jump. A hike without times draws at an even speed, even with `pace="time"`.

The diorama opens with the whole route drawn, like the poster. Pressing play first undraws it over `rewind` seconds, while the camera turns to where the play starts. The light stays as it was until the route draws again. `rewind="0"` empties the route at once. With reduced motion, the route undraws without the turn.

### The element

| Attribute | Values (default first) |
|---|---|
| `controls` | `play,zoom,reset,scrubber`, fewer of them, or `none` |
| `max-zoom` | `3`, or from `1` to `6` |
| `lang` | The page's language, or another |
| `strings` | A JSON object of [words](#words) to use instead of the built-in ones |
| `theme` | The address of a [theme file](../docs/configuration.md#theme-files), such as `/hikes/autumn.toml` |

`controls` picks the buttons and the progress line to show. Turning and zooming by hand, and the keys, work without them. `max-zoom` is the largest zoom a visitor can reach.

`lang` sets the language of the [words](#words) the element shows and reads out. English and Polish (`en` and `pl`) are built in. Any other language gets English, unless `strings` gives its words. Like any `lang`, the element takes it from the page, such as `<html lang="pl">`, when it has none of its own.

`theme` sets the diorama's colours and light. Its `[labels]` aren't used, because the live diorama draws no text. `isotrail embed --theme` copies the file and writes the attribute, and `--strings FILE` writes `strings` from a file.

### Changing attributes

Changing an attribute on a running element redraws it. A new `mesher`, `block-height`, `prop-density`, `paths`, `streams`, `strata` or `base-blocks` builds the terrain again. A new `turn`, `pitch` or `zoom` moves to the new starting view, and a new playback attribute applies from the next play.

A value the element can't use is left at its default, with a warning in the console. That includes an unknown palette name, and a `.hex` or theme file that doesn't load.

### Words

`strings` can replace every word the element shows, or reads out to screen readers. `{key}` stands for Ctrl, or ⌘ on a Mac. The other names in braces are filled in where the word is used.

| Word | English |
|---|---|
| `play`, `pause`, `loading` | Play, Pause, Loading (the play button's label) |
| `zoomIn`, `zoomOut`, `reset` | Zoom in, Zoom out, Reset view |
| `scrubber` | Route drawn (the progress line's label) |
| `progress` | `{percent}%` (how the progress line reads out) |
| `help` | How to turn, zoom and move the diorama, read out with it |
| `wheelHint` | Hold `{key}` and scroll to zoom |
| `poster` | Hike diorama (the poster's description, unless the image inside the element has one) |
| `diorama` | 3D diorama of `{title}` (the diorama's description) |
| `untitled` | a hike (the title of a hike without one) |
| `load`, `damaged`, `version`, `gpu`, `runtime` | The messages in [When something fails](#when-something-fails) |

```html
<isotrail-hike src="/hikes/rysy.itb" lang="de"
  strings='{"play": "Abspielen", "pause": "Anhalten", "reset": "Ansicht zurücksetzen"}'></isotrail-hike>
```

Words left out keep the built-in ones for the language, or English.

## Controls

| | Mouse and trackpad | Touch | Keyboard (the diorama focused) |
|---|---|---|---|
| Turn | drag (Shift-drag or right-drag when zoomed in) | drag, or twist two fingers | ← → |
| Zoom (up to 3x, or `max-zoom`) | Ctrl/⌘ + scroll, pinch, or the − + buttons | pinch | + − |
| Move around when zoomed in | drag | drag | Shift + arrows |
| Back to the start view | the reset button | the reset button | 0 or Home |
| Play and pause | the play button | the play button | Space or K |

A plain mouse wheel keeps scrolling the page, and shows a reminder to hold Ctrl (⌘ on a Mac). Zooming centres on the pointer, and never shows more than the whole slab.

Playing draws the route over about eight seconds, or as `duration`, `intro` and `orbit` say. Meanwhile the camera settles back to the start view, as in the animated exports. The diorama stops on the finished hike, unless `loop` plays it again. Turning it by hand while it plays stops the camera's own turn.

The light follows the time at the route's head, through any nights of a multi-day trek, as in the animated exports. Until the visitor plays or scrubs, the diorama shows the light at the start, as on the poster. The light has the same depth as in the stills, with beams of sunlight when the sun breaks through a partly cloudy sky.

The weather follows the same clock. Rain and snow keep falling while the diorama is on screen, unless the visitor prefers reduced motion. The season, with its snow and ice, is the one saved in the bundle, and `isotrail embed --season` saves another.

The play button sits in the bottom-left corner, and the zoom and reset buttons in the bottom-right. The thin line along the bottom edge shows how much of the route is drawn, in the route's colour. Click or drag along it to scrub, or focus it and use the arrow keys.

While the element loads, the same line shows how much of the renderer and the scene has arrived. It only shows when the server sends their true sizes. A compressed file's size is not the size that arrives, so most servers show it only once the renderer is cached.

## Scripting

| Member | What it does |
|---|---|
| `play()` | Plays from the start, or from where it was paused or scrubbed to |
| `pause()` | Pauses |
| `playing` | `true` while it plays |
| `progress` | The share of the route drawn, from 0 to 1 |
| `reset()` | Goes back to the starting view, like the reset button |
| `ready` | A promise that settles once the diorama replaces the poster, or fails to |
| `load()` | Downloads the renderer and the scene now, and returns `ready` |

`play()` called before the diorama is ready plays as soon as it is. On an element waiting for a press, it starts the download too. `load()` works whatever `load` says, and tries again after a failure. Setting `progress` moves the progress line like scrubbing, and playing carries on from there. `reset()` leaves playback as it is.

`ready` resolves with `{ backend }`. If the element gives up, it rejects with an `Error` whose `reason` is one of those in [When something fails](#when-something-fails). A new `src` gives a new promise.

Wait for the element to be defined before you call its methods from a module that may run first:

```js
await customElements.whenDefined('isotrail-hike');
const hike = document.querySelector('isotrail-hike');
hike.addEventListener('isotrail-ended', () => console.log('finished'));
try {
  await hike.ready;
  hike.progress = 0.5;   // halfway along the route
  hike.play();
} catch (error) {
  console.log('no diorama:', error.reason);
}
```

### Events

| Event | When | `detail` |
|---|---|---|
| `isotrail-state` | `data-state` changes | `{ state }`: `poster`, `loading`, `ready` or `fallback` |
| `isotrail-ready` | The diorama has drawn its first frame and replaced the poster | `{ backend }`: `"webgpu"` or `"gl"` |
| `isotrail-error` | The element gave up and shows its poster or a message | `{ reason, message }` |
| `isotrail-play` | Playing starts | `{ progress }` |
| `isotrail-pause` | Playing stops | `{ progress }` |
| `isotrail-ended` | The last play has reached the finish | `{ progress }` |
| `isotrail-progress` | The share of the route drawn changes | `{ progress }`, from 0 to 1 |

`isotrail-state` comes once for each change, never twice in a row for the same state. Playing starts from the play button, the keyboard, `autoplay` or `play()`. It stops when the visitor pauses or scrubs, and at the end of the last play, right before `isotrail-ended`. `isotrail-progress` comes on every frame while the route plays, and when it is scrubbed or set. The `reason` of `isotrail-error` is one of those in [When something fails](#when-something-fails).

All the events bubble, and cross shadow roots:

```js
document.addEventListener('isotrail-error', (event) => {
  console.log(event.target.getAttribute('src'), event.detail.reason);
});
```

### When something fails

The element keeps its poster when it can't show the diorama. Without a poster, it shows a message instead. The five reasons share three messages:

| `reason` | Message | Cause |
|---|---|---|
| `load` | This hike couldn't be loaded. | The bundle or the renderer didn't download |
| `damaged` | This hike couldn't be loaded. | The file isn't a scene bundle, or is cut short |
| `version` | This hike was saved by a different version of isotrail. | The bundle and the runtime come from different versions |
| `gpu` | This browser can't show the 3D view. | No WebGPU or WebGL2, or the GPU failed twice |
| `runtime` | This browser can't show the 3D view. | An error inside the renderer |

For `version`, make the bundle again with the isotrail that matches the runtime. A warning with the details also goes to the console.

A failed element stays as it is until its `src` changes, or until it is taken out of the page and put back. A renderer that failed to download is then downloaded again.

## Styling

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
  --isotrail-hover: rgb(52 70 80 / 0.9); /* buttons under the pointer */
  --isotrail-radius: 4px;               /* rounds the buttons, whose corners are cut without it */
  --isotrail-credit-outline: #10141f;   /* the outline of the credit over the diorama */
  border-radius: 6px;
}
```

Set them on the element or on a class it has. A page's own `--route` doesn't reach the progress line.

You can style the parts `canvas`, `play`, `scrubber` (the progress line), `zoom-in`, `zoom-out`, `reset` and `credit` with `::part()`. Each button draws its square in `::before`, so `::part(play)::before` restyles it. The element sets `data-state` to `poster`, `loading`, `ready` or `fallback`, and sends `isotrail-state` each time.

On touch screens, the buttons take touches across 44 pixels and the progress line across 24, though they look the same size.

## Hosting checklist

- Serve `.wasm` as `application/wasm` and `.js` as `text/javascript`
- Let browsers cache `isotrail-embed.js`, `pkg/` and the bundles for a long time
- Serve the page over HTTPS, so browsers offer WebGPU
- If the site has a Content-Security-Policy, allow the element in it, as [shown below](#content-security-policy)
- Check what the bundle says about the hike, as [shown below](#what-a-scene-file-contains)

Most servers and static hosts already send the right types. With another type for `.wasm`, the renderer still loads, but more slowly and with a warning in the console. `.itb` can be `application/octet-stream`.

Long caching is safe because the snippet's `?v=` changes with every isotrail version. When you update isotrail, update the runtime and remake the snippets together. Remake the bundles too if the bundle format changed.

Browsers only offer WebGPU on secure pages and on `localhost`. Elsewhere the element uses WebGL2.

### Content-Security-Policy

The element needs:

- `script-src` to allow the script's origin and `'wasm-unsafe-eval'`, to compile the renderer
- `connect-src` to allow the origins of the bundle and of `pkg/`
- `img-src` to allow the poster's origin

It needs no inline scripts, and its styles need no `'unsafe-inline'`. This policy works when the site serves everything itself:

```text
default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self'; img-src 'self'; connect-src 'self'
```

The `style` attributes in the snippet are inline styles, which `style-src 'self'` blocks. The element still works, but a page without the script doesn't keep room for it. Remove the `style` attributes and add these rules to your stylesheet instead:

```css
isotrail-hike { display: block; aspect-ratio: 16 / 9 }
isotrail-hike > img { display: block; width: 100% }
```

For a snippet with `credit="below"`, the ratio goes on the image and the credit has a style of its own:

```css
isotrail-hike { display: block }
isotrail-hike > img { display: block; width: 100%; aspect-ratio: 16 / 9 }
isotrail-hike > p { margin: 4px 0 0; font: 11px/1.3 system-ui, sans-serif; text-align: center; overflow-wrap: anywhere }
isotrail-hike > p a { color: inherit }
```

### What a scene file contains

Anyone can download the `.itb` file a page uses and read it. Besides the terrain, it holds the GPX file's name and description, the hike's statistics and its route. Each point of the route keeps its time, to the second.

Make the bundle with `--times coarse` to round the times, or with `--times none` to leave them out. `--no-desc` leaves out the description. `--trim-start` and `--trim-end` cut the ends off the route, for a walk that starts at your door:

```sh
isotrail embed hike.gpx -o site/hikes --times coarse --no-desc --trim-start 500m --trim-end 500m
```

The times and trim options need the GPX file, because a bundle keeps the times and route it was made with. The [usage guide](../docs/usage.md#what-a-scene-file-contains) lists everything a bundle holds and what each option changes.

### Data credit

Once the diorama replaces the poster, a small line along its bottom edge credits the data, as the licences require. It names OpenStreetMap (ODbL), the terrain tiles, and Open-Meteo (CC BY 4.0) when the hike has weather. The text comes from the bundle, so it matches the credit on stills. "OpenStreetMap" links to its copyright page and "Open-Meteo.com" to its site.

Over a light page or a transparent background, the outlined line can be hard to read. `credit="below"` moves it under the diorama as ordinary small text without an outline. It takes the page's text colour, or `--isotrail-text` when that is set on the element. The diorama keeps its shape, and the element grows by the line. `isotrail embed --credit below` writes a snippet for it:

```html
<isotrail-hike src="derwent-edge.itb" poster="derwent-edge.png" aspect="wide" credit="below" style="display:block"><img src="derwent-edge.png" alt="Diorama of Derwent Edge" style="display:block;width:100%;aspect-ratio:16/9"><p slot="credit" style="…">© <a href="https://www.openstreetmap.org/copyright" …>OpenStreetMap</a> contributors · Terrain Tiles (AWS)</p></isotrail-hike>
```

The shape moves from the element to the image, and the credit follows it. The line shows where the script doesn't run, and until the diorama replaces it with its own.

You can restyle the line with `::part(credit)`. **If you hide it, with `credit="none"` or with CSS, credit the same sources near the embed**, for example in a caption under it. `isotrail embed --no-attribution` prints the text to use.

### Many hikes on one page

The poster shows at once. The renderer, about 1.2MB gzipped, only downloads when the element comes near the screen, and only once for the whole page. With `load="press"`, it waits for a press instead. Use that for a hike at the top of a page that most visitors scroll past.

Browsers give a page about 16 WebGL contexts. So when an element has been well off screen for a few seconds, it frees its renderer and shows its poster. It starts again when it comes back, from the starting view.

If the GPU is lost while the page is open, the element starts again. This happens when a driver resets, or when the browser takes back a WebGL context. If it happens again within a minute, the element keeps its poster.

## The demo

Serve the `web/` folder with any static file server, and open `/demo/`:

```bash
python -m http.server -d web 8000    # then open http://localhost:8000/demo/
```

The page shows a synthetic hike four times: waiting for a press, with a poster, looping on its own, and without controls. A link at the top switches all four to the WebGL2 fallback (`?backend=webgl` in the address).

To try it on a phone on the same network, serve it with `python -m http.server -d web 8000 --bind 0.0.0.0`. Then open `http://<your computer's address>:8000/demo/` on the phone. A plain `http://` address like this isn't secure, so the phone uses WebGL2.

## Building

The runtime is the `isotrail-web` crate, compiled to WebAssembly. You need the Rust toolchain from the repository, which includes the `wasm32-unknown-unknown` target, and [wasm-pack](https://drager.github.io/wasm-pack/):

```bash
cargo install wasm-pack
```

Then, from the repository root (in Git Bash on Windows):

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
viewer.setOptions({ mesher: 'smooth', time: '20:30', pace: 'time', maxZoom: 6, theme: text });   // changes the look, pace and starting view
viewer.draw();
console.log(viewer.backend, viewer.zoom, viewer.maxZoom, viewer.title);   // "webgpu" or "gl", 1 to maxZoom, 3 to 6, the hike's name
console.log(viewer.routeColour);              // "#d64834", or the colour the options or theme give the route
console.log(viewer.credit);                   // the data credits, joined with " · "
console.log(viewer.weatherMoves);             // true while rain or snow falls, so frames keep changing
viewer.onLost((message) => { ... });          // the GPU was lost: free the viewer and load again
viewer.free();                                // releases the GPU memory at once
```

Nothing is drawn until the first `resize`. Call `resize` again whenever the canvas changes size, and `draw` after any change. While `weatherMoves` is true, call `setTime` and `draw` on every frame, so the rain and snow fall.

Until the first `setProgress`, the whole route shows under the light at the start, as on the poster. `setProgress` draws the route at an even speed, with a pause for each night of a multi-day trek, lit by the time at its head.

Each art pixel covers a whole number of device pixels, chosen so that the frame's long side is at most 640 pixels. That matches a still rendered at `--scale 3`.

### The look object

The look takes the same values as the element's attributes, as strings or numbers. Its keys are:

- `palette`: a built-in name, `none`, or the text of a `.hex` file, which the API doesn't fetch for you as the element does
- `mesher`, `blockHeight`, `occlusion`, `softShadows`, `godRays`, `background` and `time`
- the scene, light and route options, such as `routeColour` (`routeColor` works too)
- the starting view's `turn`, `pitch` and `zoom`, and `maxZoom`
- `pace`, for `setProgress`
- `theme`: the text of a theme file, under all the others

Each key left out or unusable takes its default, and unusable ones warn in the console. `setOptions` replaces the whole look. It throws an error with reason `"gpu"` if the GPU can't hold the terrain that a new mesher builds.

### Errors

Arguments that aren't finite numbers, such as `NaN`, are ignored. `Viewer.load` rejects with an `Error` whose `reason` is `"version"`, `"damaged"` or `"gpu"`, as in [When something fails](#when-something-fails). `resize` throws one with reason `"gpu"` if the GPU can't hold the diorama at that size. After any error from the viewer, free it.

`onLost` is only called on WebGPU. On WebGL2, listen for `webglcontextlost` on the canvas.

## Testing the element

`web/test/check.mjs` loads synthetic hikes into headless Chrome or Edge and checks the element. It covers:

- the credit, the focus ring, and each failure's message and event
- a lost GPU, every attribute, and playback with its events and loops
- the script API, the words in Polish and from `strings`, and a theme file
- twenty embeds on one page, touch targets and a strict Content-Security-Policy
- that playing creates no GPU objects
- that a poster without overlays covers the same pixels as the first live frame

It needs Node 22 or later and the runtime built with `sh web/build.sh`. It renders the poster with `cargo`, which needs a graphics card. On a machine without one, set `ISOTRAIL_GPU=software`.

```bash
node web/test/check.mjs chrome           # WebGPU where the browser has it
node web/test/check.mjs edge --webgl     # every embed on WebGL2, drawn by SwiftShader
```

`ISOTRAIL_BROWSER` sets the browser's path if it isn't in the usual place. CI runs the check in Chrome with `--webgl`.
