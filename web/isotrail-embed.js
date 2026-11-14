// <isotrail-hike>: an interactive diorama of a hike for any web page

// The script's own query (such as ?v=1.5.0) goes on to the files it loads,
// so a browser never pairs a cached old renderer with a new script
const QUERY = new URL(import.meta.url).search;
const RUNTIME = new URL(`./pkg/isotrail_web.js${QUERY}`, import.meta.url);
const WASM = new URL(`./pkg/isotrail_web_bg.wasm${QUERY}`, import.meta.url);

const ASPECTS = {
  story: '9 / 16',
  video: '9 / 16',
  portrait: '3 / 4',
  feed: '4 / 5',
  square: '1 / 1',
  wide: '16 / 9',
};
// The look and starting-view attributes; the renderer takes them under
// these names, and names them in its warnings
const LOOK = [
  'palette',
  'mesher',
  'block-height',
  'occlusion',
  'soft-shadows',
  'god-rays',
  'background',
  'time',
  'turn',
  'pitch',
  'zoom',
  'prop-density',
  'paths',
  'streams',
  'strata',
  'base-blocks',
  'outlines',
  'shadows',
  'night-sky',
  'weather-effects',
  'weather',
  'sun',
  'route-colour',
  'route-color',
  'route-width',
  'route-hidden',
  'markers',
];
// Attributes the renderer takes besides the look
const VIEWER = [...LOOK, 'pace', 'max-zoom'];
const CLIP = ['duration', 'intro', 'hold', 'loop-back', 'rewind', 'orbit', 'loop'];
const CONTROLS = ['play', 'zoom', 'reset', 'scrubber'];
const ROUTE_COLOUR = /^#[0-9a-f]{6}$/i;
// The parts of a clip, as the animated exports split it
const DURATION_S = 10;
const MAX_DURATION_S = 180;
const INTRO_SHARE = 0.05;
const DRAW_SHARE = 0.8;
const HOLD_SHARE = 0.15;
const REWIND_SHARE = 0.15;
const MIN_DRAW_S = 0.5;
// A play from the finished hike first undraws the route, so nothing jumps
const REWIND_S = 1;
const MAX_REWIND_S = 10;
const ORBIT_DEG = 30;
const MAX_PLAYS = 1000;
const TURN_PER_PX = 0.4;
const KEY_TURN_DEG = 10;
const KEY_PAN_PX = 40;
const KEY_ZOOM = 1.25;
const HINT_MS = 1500;
// A hidden tab resumes where it was rather than jumping ahead
const MAX_STEP_S = 0.1;
// Browsers keep only about 16 WebGL contexts per page, so embeds far off
// screen give theirs back
const RELEASE_MS = 3000;
// A second lost GPU this soon after the first keeps the poster
const RESTART_WINDOW_MS = 60000;

// Every word the element shows or reads out, by language; {key} is Ctrl
// or ⌘, and the other {names} are filled in where they are used
const WORDS = {
  en: {
    play: 'Play',
    pause: 'Pause',
    loading: 'Loading',
    zoomIn: 'Zoom in',
    zoomOut: 'Zoom out',
    reset: 'Reset view',
    scrubber: 'Route drawn',
    progress: '{percent}%',
    help: 'Drag to turn the diorama. Pinch, or hold {key} and scroll, to zoom; once zoomed in, drag to move around and Shift-drag to turn. Keys: left and right arrows turn, plus and minus zoom, Shift with the arrows moves, 0 resets the view, Space plays and pauses.',
    wheelHint: 'Hold {key} and scroll to zoom',
    poster: 'Hike diorama',
    diorama: '3D diorama of {title}',
    untitled: 'a hike',
    load: "This hike couldn't be loaded.",
    damaged: "This hike couldn't be loaded.",
    version: 'This hike was saved by a different version of isotrail.',
    gpu: "This browser can't show the 3D view.",
    runtime: "This browser can't show the 3D view.",
  },
  pl: {
    play: 'Odtwórz',
    pause: 'Wstrzymaj',
    loading: 'Wczytywanie',
    zoomIn: 'Przybliż',
    zoomOut: 'Oddal',
    reset: 'Przywróć widok',
    scrubber: 'Narysowana część trasy',
    progress: '{percent}%',
    help: 'Przeciągnij, aby obrócić dioramę. Aby przybliżyć, rozsuń palce albo przytrzymaj {key} i przewiń; po przybliżeniu przeciągaj, aby się przesuwać, a z wciśniętym Shiftem, aby obracać. Klawisze: strzałki w lewo i w prawo obracają, plus i minus przybliżają i oddalają, Shift ze strzałkami przesuwa, 0 przywraca widok, spacja odtwarza i wstrzymuje.',
    wheelHint: 'Przytrzymaj {key} i przewiń, aby przybliżyć',
    poster: 'Diorama wędrówki',
    diorama: 'Diorama 3D: {title}',
    untitled: 'wędrówka',
    load: 'Nie udało się wczytać tej wędrówki.',
    damaged: 'Nie udało się wczytać tej wędrówki.',
    version: 'Tę wędrówkę zapisała inna wersja isotrail.',
    gpu: 'Ta przeglądarka nie może pokazać widoku 3D.',
    runtime: 'Ta przeglądarka nie może pokazać widoku 3D.',
  },
};
const CREDIT_LINKS = {
  OpenStreetMap: 'https://www.openstreetmap.org/copyright',
  'Open-Meteo.com': 'https://open-meteo.com/',
};

const MAC = /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent);
const ZOOM_KEY = MAC ? '⌘' : 'Ctrl';

// A download's bytes so far; total stays null when the server's size can't
// be trusted, as a compressed file's size is not the size that arrives
function download() {
  return { loaded: 0, total: null, done: false };
}

function counted(response, progress) {
  const length = Number(response.headers.get('content-length'));
  const known =
    response.ok &&
    response.body &&
    response.type === 'basic' &&
    length > 0 &&
    !response.headers.get('content-encoding');
  if (!known) return response;
  progress.total = length;
  const reader = response.body.getReader();
  const body = new ReadableStream({
    async pull(controller) {
      const { done, value } = await reader.read();
      if (done) {
        controller.close();
        return;
      }
      progress.loaded += value.byteLength;
      if (progress.loaded > progress.total) progress.total = null;
      controller.enqueue(value);
    },
    cancel: (reason) => reader.cancel(reason),
  });
  const { status, statusText, headers } = response;
  return new Response(body, { status, statusText, headers });
}

let runtime = null;
let runtimeProgress = download();
function loadRuntime() {
  if (!runtime) {
    const progress = download();
    // The renderer's own loader warns about a .wasm of the wrong type, so
    // it gets that one as it came
    const wasm = fetch(WASM).then((response) =>
      response.headers.get('content-type') === 'application/wasm' ? counted(response, progress) : response,
    );
    const loading = Promise.all([import(RUNTIME.href), wasm]).then(async ([module, response]) => {
      await module.default({ module_or_path: response });
      progress.done = true;
      return module;
    });
    // A failed download is tried again by the next embed that needs it
    loading.catch(() => {
      if (runtime === loading) runtime = null;
    });
    runtime = loading;
    runtimeProgress = progress;
  }
  return runtime;
}

function saveData() {
  return navigator.connection?.saveData === true;
}

function hasGpu() {
  if ('gpu' in navigator) return true;
  try {
    const gl = document.createElement('canvas').getContext('webgl2');
    gl?.getExtension('WEBGL_lose_context')?.loseContext();
    return Boolean(gl);
  } catch {
    return false;
  }
}

function reducedMotion() {
  return matchMedia('(prefers-reduced-motion: reduce)').matches;
}

// A preset's name, or a width and height such as 3:2
function aspectRatio(value) {
  const ratio = /^\s*(\d+)\s*:\s*(\d+)\s*$/.exec(value ?? '');
  if (ratio && Number(ratio[1]) > 0 && Number(ratio[2]) > 0) return `${ratio[1]} / ${ratio[2]}`;
  if (Object.hasOwn(ASPECTS, value ?? '')) return ASPECTS[value];
  if (value?.trim()) {
    ignored('aspect', value, `expected ${Object.keys(ASPECTS).join(', ')} or a width and height such as 3:2`);
  }
  return ASPECTS.wide;
}

// Slow at both ends, as the animation's camera settles
function ease(x) {
  return (1 - Math.cos(Math.PI * Math.min(Math.max(x, 0), 1))) / 2;
}

function fill(text, values) {
  return text.replace(/\{(\w+)\}/g, (match, name) => values[name] ?? match);
}

function ignored(name, value, reason) {
  const shown = String(value).slice(0, 40);
  console.warn(`<isotrail-hike> ignored ${name} ${JSON.stringify(shown)}, so it uses the default: ${reason}`);
}

// Seconds such as 10 or 2.5s, within a range
function seconds(element, name, min, fallback, max = MAX_DURATION_S) {
  const value = element.getAttribute(name);
  if (value === null || value.trim() === '') return fallback;
  const number = Number(value.trim().replace(/s$/, ''));
  if (Number.isFinite(number) && number >= min && number <= max) return number;
  ignored(name, value, `must be between ${min} and ${max} seconds`);
  return fallback;
}

// How a play runs, in seconds, split as in the animated exports: the intro
// showing the scene, the route drawing itself, the hold on the finished
// hike and, with loop-back, the rewind; the hold and the rewind only play
// between loops. A play that starts on the finished hike first undraws it
// and turns away over `lead` seconds, before the clip's time 0
function readClip(element) {
  const duration = seconds(element, 'duration', 1, DURATION_S);
  let intro = seconds(element, 'intro', 0, null);
  let hold = seconds(element, 'hold', 0, null);
  const loopBack = element.hasAttribute('loop-back');
  const parts = () => {
    const share = (s, fallback) => (s === null ? fallback : s / duration);
    const rewind = loopBack ? REWIND_SHARE : 0;
    const draw =
      intro === null && hold === null && !loopBack
        ? DRAW_SHARE
        : 1 - share(intro, INTRO_SHARE) - share(hold, HOLD_SHARE) - rewind;
    return [share(intro, INTRO_SHARE), draw, share(hold, HOLD_SHARE)].map((s) => s * duration);
  };
  let [introS, drawS, holdS] = parts();
  if (drawS < MIN_DRAW_S - 1e-4) {
    console.warn(
      `<isotrail-hike> ignored intro and hold, so they use the default: a ${duration}s clip would leave ${Math.max(drawS, 0).toFixed(2)}s to draw the route, and it needs at least ${MIN_DRAW_S}s`,
    );
    intro = null;
    hold = null;
    [introS, drawS, holdS] = parts();
  }
  let orbit = ORBIT_DEG;
  const orbitValue = element.getAttribute('orbit');
  if (orbitValue !== null && orbitValue.trim() !== '') {
    const number = Number(orbitValue);
    if (Number.isFinite(number) && Math.abs(number) <= 360) orbit = number;
    else ignored('orbit', orbitValue, 'must be between -360 and 360');
  }
  let plays = 1;
  const loop = element.getAttribute('loop');
  if (loop !== null) {
    const word = loop.trim().toLowerCase();
    const count = Number(word);
    if (word === '' || word === 'forever') plays = Infinity;
    else if (word === 'once') plays = 1;
    else if (Number.isInteger(count) && count >= 1 && count <= MAX_PLAYS) plays = count;
    else ignored('loop', loop, `expected forever, once or a number of plays from 1 to ${MAX_PLAYS}`);
  }
  const lead = seconds(element, 'rewind', 0, REWIND_S, MAX_REWIND_S);
  const leading = (time) => (time < 0 && lead > 0 ? ease((time + lead) / lead) : null);
  const settled = introS + drawS;
  const rewindStart = settled + holdS;
  const rewound = (time) =>
    loopBack && time >= rewindStart && duration > rewindStart
      ? Math.min((time - rewindStart) / (duration - rewindStart), 1)
      : null;
  return {
    duration,
    intro: introS,
    draw: drawS,
    settled,
    plays,
    lead,
    // Share of the route drawn at a time in the clip
    share(time) {
      const undrawn = leading(time);
      if (undrawn !== null) return 1 - undrawn;
      const back = rewound(time);
      if (back !== null) return 1 - ease(back);
      return Math.min(Math.max((time - introS) / drawS, 0), 1);
    },
    // Degrees turned from the still's view, easing back to it as the route
    // finishes, and away again as it rewinds
    yaw(time) {
      const undrawn = leading(time);
      if (undrawn !== null) return -orbit * undrawn;
      const back = rewound(time);
      if (back !== null) return -orbit * ease(back);
      return time >= settled ? 0 : -orbit * (1 - ease(time / settled));
    },
  };
}

// The built-in words for the element's language, or the page's, under any
// the strings attribute gives
function readWords(element) {
  const tag = (element.closest('[lang]')?.getAttribute('lang') ?? '').trim().toLowerCase();
  const words = { ...WORDS.en, ...(WORDS[tag.split('-')[0]] ?? {}) };
  const strings = element.getAttribute('strings');
  if (strings === null || strings.trim() === '') return words;
  let given;
  try {
    given = JSON.parse(strings);
  } catch (error) {
    ignored('strings', strings, `not JSON: ${error.message}`);
    return words;
  }
  if (typeof given !== 'object' || given === null || Array.isArray(given)) {
    ignored('strings', strings, 'expected a JSON object of words');
    return words;
  }
  for (const [name, text] of Object.entries(given)) {
    if (!(name in WORDS.en)) {
      ignored('strings', name, `unknown word; the words are ${Object.keys(WORDS.en).join(', ')}`);
    } else if (typeof text !== 'string') {
      ignored(`strings.${name}`, JSON.stringify(text), 'expected text');
    } else {
      words[name] = text;
    }
  }
  return words;
}

// When the element downloads the renderer and the scene: once near the
// screen, or once play is pressed
function readLoad(element) {
  const value = element.getAttribute('load');
  if (value === null || value.trim() === '') return 'visible';
  const word = value.trim().toLowerCase();
  if (word === 'visible' || word === 'press') return word;
  ignored('load', value, 'expected visible or press');
  return 'visible';
}

// The controls to show, all by default
function readControls(element) {
  const value = element.getAttribute('controls');
  if (value === null || value.trim() === '') return new Set(CONTROLS);
  if (value.trim().toLowerCase() === 'none') return new Set();
  const shown = new Set();
  for (const name of value.split(',').map((part) => part.trim().toLowerCase())) {
    if (CONTROLS.includes(name)) shown.add(name);
    else ignored('controls', name, `expected ${CONTROLS.join(', ')} or none`);
  }
  return shown;
}

const icon = (body) =>
  `<svg viewBox="0 0 16 16" aria-hidden="true" shape-rendering="crispEdges">${body}</svg>`;
const ICONS = {
  play: icon('<path d="M5 3h2v1h2v1h2v1h2v4h-2v1H9v1H7v1H5z" fill="currentColor"/>'),
  pause: icon('<path d="M4 3h3v10H4zM9 3h3v10H9z" fill="currentColor"/>'),
  busy: icon(
    '<path class="dot" d="M2 7h2v2H2z" fill="currentColor"/><path class="dot" d="M7 7h2v2H7z" fill="currentColor"/><path class="dot" d="M12 7h2v2h-2z" fill="currentColor"/>',
  ),
  zoomIn: icon('<path d="M7 3h2v4h4v2H9v4H7V9H3V7h4z" fill="currentColor"/>'),
  zoomOut: icon('<path d="M3 7h10v2H3z" fill="currentColor"/>'),
  reset: icon(
    '<path d="M2 2h5v2H4v3H2zM9 2h5v5h-2V4H9zM12 9h2v5H9v-2h3zM2 9h2v3h3v2H2z" fill="currentColor"/>',
  ),
};

const CSS = `
  :host {
    --isotrail-panel: rgb(32 46 55 / 0.78);
    --isotrail-edge: #819796;
    --isotrail-accent: #a4dddb;
    --isotrail-track: rgb(32 46 55 / 0.45);
    --isotrail-background: transparent;
    --isotrail-hover: rgb(52 70 80 / 0.9);
    --isotrail-credit-outline: #10141f;
    /* Unset unless the page sets them on the element itself: the credit below
       the frame takes the page's colour, and the buttons keep their cut corners */
    --isotrail-text: initial;
    --isotrail-radius: initial;
    display: block;
    overflow: hidden;
    contain: content;
  }
  :host([hidden]) { display: none; }
  /* The frame keeps its shape and the element grows by the line */
  :host([credit="below"]) { aspect-ratio: auto !important; }
  .frame {
    position: relative;
    aspect-ratio: var(--isotrail-aspect, 16 / 9);
    overflow: hidden;
    background: var(--isotrail-background);
  }
  img, canvas, .message {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
  }
  img {
    object-fit: contain;
    image-rendering: pixelated;
  }
  img:not([src]) { display: none; }
  canvas {
    display: block;
    opacity: 0;
    touch-action: pan-y;
    cursor: grab;
    outline: none;
    image-rendering: pixelated;
  }
  canvas.dragging { cursor: grabbing; }
  /* An outline paints over the canvas's own pixels, an inset shadow doesn't */
  canvas:focus-visible {
    outline: 2px solid var(--isotrail-accent);
    outline-offset: -2px;
  }
  :host([data-state="ready"]) canvas { opacity: 1; }
  :host([data-state="ready"]) img { visibility: hidden; }
  .message {
    display: none;
    place-items: center;
    padding: 16px;
    box-sizing: border-box;
    text-align: center;
    font: 14px/1.4 system-ui, sans-serif;
    color: var(--isotrail-text, #ebede9);
    background: var(--isotrail-panel);
  }
  :host([data-state="fallback"]) .frame:not(.has-poster) .message { display: grid; }
  .controls {
    --hit: 28px;
    --gap: 2px;
    position: absolute;
    inset: 0;
    pointer-events: none;
    color: var(--isotrail-text, #ebede9);
  }
  /* Finger-sized targets around buttons that look the same */
  @media (pointer: coarse) {
    .controls { --hit: 44px; --gap: 0px; }
    input { --scrub: 24px; }
  }
  :host(:not([data-state="ready"])) .controls { visibility: hidden; }
  /* Waiting for a press, and loading after one, the play button shows alone */
  .frame.asking [part="play"] { visibility: visible; }
  .bar {
    --spare: calc((var(--hit) - 28px) / 2);
    position: absolute;
    left: calc(8px - var(--spare));
    right: calc(8px - var(--spare));
    bottom: calc(16px - var(--spare));
    display: flex;
    align-items: flex-end;
    gap: 8px;
  }
  .bar > *, input { pointer-events: auto; }
  .buttons {
    display: flex;
    gap: var(--gap);
  }
  /* At the right edge, also when the credit isn't between the groups */
  .buttons:last-child { margin-left: auto; }
  button {
    position: relative;
    display: grid;
    place-items: center;
    width: var(--hit);
    height: var(--hit);
    padding: 0;
    border: 0;
    background: none;
    color: inherit;
    cursor: pointer;
    outline: none;
    -webkit-tap-highlight-color: transparent;
  }
  /* Square, with the corners cut and a lit top edge, like the panels in the stills */
  button::before {
    content: '';
    position: absolute;
    inset: calc((var(--hit) - 28px) / 2);
    background: var(--isotrail-panel);
    box-shadow: inset 0 2px 0 -1px var(--isotrail-edge);
    /* Without --isotrail-radius, --round is invalid and the cut corners apply */
    --round: inset(0 round var(--isotrail-radius));
    clip-path: var(--round, polygon(2px 0, calc(100% - 2px) 0, 100% 2px, 100% calc(100% - 2px),
      calc(100% - 2px) 100%, 2px 100%, 0 calc(100% - 2px), 0 2px));
  }
  button:hover::before { background-color: var(--isotrail-hover); }
  button:focus-visible::before { box-shadow: inset 0 0 0 2px var(--isotrail-accent); }
  button[aria-disabled="true"] { cursor: default; }
  button[hidden], input[hidden] { display: none; }
  button[aria-disabled="true"] svg { opacity: 0.4; }
  svg {
    position: relative;
    width: 16px;
    height: 16px;
  }
  @media (prefers-reduced-motion: no-preference) {
    .dot { animation: isotrail-busy 0.9s infinite; }
    .dot:nth-child(2) { animation-delay: 0.3s; }
    .dot:nth-child(3) { animation-delay: 0.6s; }
  }
  @keyframes isotrail-busy {
    0%, 33% { opacity: 1; }
    34%, 100% { opacity: 0.35; }
  }
  .credit {
    flex: 1;
    min-width: 0;
    margin: 0 0 calc((var(--hit) - 28px) / 2);
    font: 11px/1.3 system-ui, sans-serif;
    text-align: center;
    color: var(--isotrail-accent);
    text-shadow: 1px 0 var(--isotrail-credit-outline), -1px 0 var(--isotrail-credit-outline),
      0 1px var(--isotrail-credit-outline), 0 -1px var(--isotrail-credit-outline);
    overflow-wrap: anywhere;
  }
  .credit span {
    display: inline-block;
    max-width: 100%;
  }
  .credit a { color: inherit; }
  .credit .extra { display: block; }
  :host([credit="none"]) .credit { visibility: hidden; }
  /* Under the frame, as ordinary text in the page's colour; the snippet's own
     line shows there until the diorama is up */
  .below .credit {
    margin: 4px 0 0;
    color: var(--isotrail-text, currentColor);
    text-shadow: none;
  }
  .below slot { display: none; }
  :host([credit="below"]:not([data-state="ready"])) .below slot { display: contents; }
  :host(:not([data-state="ready"])) .below .credit { display: none; }
  /* The route's colour, set on the lines themselves so a page's own
     variables can't reach them; route-colour replaces it inline */
  input, .loaded { --_route: #d64834; }
  /* The route drawn so far, as a line along the bottom edge with a light
     tick at its head; the input is taller than the line, to be easy to grab */
  input {
    position: absolute;
    left: 0;
    bottom: 0;
    width: 100%;
    height: var(--scrub, 12px);
    margin: 0;
    appearance: none;
    -webkit-appearance: none;
    --walked: 100%;
    background: linear-gradient(to right,
        var(--isotrail-progress, var(--_route)) var(--walked),
        var(--isotrail-text, #ebede9) var(--walked) calc(var(--walked) + 2px),
        var(--isotrail-track) calc(var(--walked) + 2px))
      bottom / 100% 4px no-repeat;
    cursor: pointer;
    outline: none;
  }
  /* How much of the renderer and the scene has arrived, in the same place */
  .loaded {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    height: 4px;
    background: linear-gradient(to right,
      var(--isotrail-progress, var(--_route)) var(--loaded, 0%),
      var(--isotrail-text, #ebede9) var(--loaded, 0%) calc(var(--loaded, 0%) + 2px),
      var(--isotrail-track) calc(var(--loaded, 0%) + 2px));
    pointer-events: none;
  }
  .loaded[hidden] { display: none; }
  input:hover, input:focus-visible { background-size: 100% 6px; }
  input:focus-visible { box-shadow: inset 0 -8px 0 -6px var(--isotrail-accent); }
  input::-webkit-slider-runnable-track { height: 100%; background: transparent; }
  input::-moz-range-track { height: 100%; background: transparent; }
  input::-webkit-slider-thumb {
    -webkit-appearance: none;
    width: 2px;
    height: var(--scrub, 12px);
    border: 0;
    background: transparent;
  }
  input::-moz-range-thumb {
    width: 2px;
    height: var(--scrub, 12px);
    border: 0;
    border-radius: 0;
    background: transparent;
  }
  .hint {
    position: absolute;
    left: 50%;
    top: 50%;
    transform: translate(-50%, -50%);
    padding: 6px 10px;
    border-radius: 4px;
    background: var(--isotrail-panel);
    color: var(--isotrail-text, #ebede9);
    font: 14px/1.4 system-ui, sans-serif;
    white-space: nowrap;
    pointer-events: none;
    opacity: 0;
  }
  .hint.shown { opacity: 1; }
  .help {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
`;

// A constructed style sheet needs no 'unsafe-inline' in a page's
// Content-Security-Policy, unlike a <style> element
let sheet = null;
function adoptStyles(root) {
  if ('adoptedStyleSheets' in Document.prototype && 'replaceSync' in CSSStyleSheet.prototype) {
    if (!sheet) {
      sheet = new CSSStyleSheet();
      sheet.replaceSync(CSS);
    }
    root.adoptedStyleSheets = [sheet];
  } else {
    const style = document.createElement('style');
    style.textContent = CSS;
    root.prepend(style);
  }
}

const template = document.createElement('template');
template.innerHTML = `
<div class="frame">
  <img alt="">
  <div class="message"></div>
  <div class="hint" aria-live="polite"></div>
  <div class="loaded" aria-hidden="true" hidden></div>
  <div class="controls">
    <input type="range" part="scrubber" min="0" max="1000" value="1000">
    <div class="bar">
      <div class="buttons">
        <button type="button" part="play" data-action="play">${ICONS.play}</button>
      </div>
      <p class="credit" part="credit"></p>
      <div class="buttons">
        <button type="button" part="zoom-out" data-action="zoom-out">${ICONS.zoomOut}</button>
        <button type="button" part="zoom-in" data-action="zoom-in">${ICONS.zoomIn}</button>
        <button type="button" part="reset" data-action="reset">${ICONS.reset}</button>
      </div>
    </div>
  </div>
</div>
<div class="below"><slot name="credit"></slot></div>
<p class="help" id="help"></p>
`;

class IsotrailHike extends HTMLElement {
  static observedAttributes = [
    'src',
    'poster',
    'aspect',
    'backend',
    'load',
    'credit',
    'credit-extra',
    'theme',
    'controls',
    'lang',
    'strings',
    ...CLIP,
    ...VIEWER,
  ];

  #frameBox;
  #poster;
  #message;
  #credit;
  #below;
  #creditText = '';
  #canvas = null;
  #controls;
  #playButton;
  #scrubber;
  #loaded;
  #zoomIn;
  #zoomOut;
  #resetButton;
  #help;
  #hint;
  #hintTimer = 0;
  #words = WORDS.en;
  #failure = null;
  #title = '';

  #viewer = null;
  #webgl = false;
  #generation = 0;
  #started = false;
  #near = false;
  #intersection = null;
  #releaseTimer = 0;
  #resize = null;
  #frame = 0;
  #drawn = false;
  #sized = false;
  // The weather's clock starts at 0 on the first frame, where the poster has it
  #clockStart = null;
  #restartedAt = null;
  #lookChanges = 0;
  #ready = null;
  #loadMode = 'visible';
  // A press, load() or play() lets a load="press" element load, until src
  // changes
  #pressed = false;
  // Loading after a press, with the play button showing it
  #busy = false;
  // The scene, fetched ahead when the pointer comes to the play button
  #prefetch = null;
  #downloads = [];
  #loadingFrame = 0;
  #icon = null;

  #clip = null;
  #progress = 1;
  // Until played, scrubbed or set, the viewer shows the poster's light
  #moved = false;
  #yaw = 0;
  #time = 0;
  #playsLeft = 1;
  #playing = false;
  // play() before the diorama is up
  #wantsPlay = false;
  #settling = false;
  #holdLight = false;
  #last = null;
  #pointers = new Map();

  constructor() {
    super();
    const root = this.attachShadow({ mode: 'open' });
    adoptStyles(root);
    root.append(template.content.cloneNode(true));
    this.#frameBox = root.querySelector('.frame');
    this.#poster = root.querySelector('img');
    this.#message = root.querySelector('.message');
    this.#credit = root.querySelector('.credit');
    this.#below = root.querySelector('.below');
    this.#controls = root.querySelector('.controls');
    this.#playButton = root.querySelector('[data-action="play"]');
    this.#scrubber = root.querySelector('input');
    this.#loaded = root.querySelector('.loaded');
    this.#zoomIn = root.querySelector('[data-action="zoom-in"]');
    this.#zoomOut = root.querySelector('[data-action="zoom-out"]');
    this.#resetButton = root.querySelector('[data-action="reset"]');
    this.#help = root.querySelector('.help');
    this.#hint = root.querySelector('.hint');
    this.#newReady();
    this.#clip = readClip(this);
    this.#time = this.#clip.settled;

    this.#controls.addEventListener('click', (event) => this.#onButton(event));
    this.#playButton.addEventListener('pointerenter', () => this.#headStart());
    this.#playButton.addEventListener('focus', () => this.#headStart());
    this.#scrubber.addEventListener('input', () => {
      this.pause();
      this.#seek(this.#scrubber.value / 1000);
    });
  }

  connectedCallback() {
    this.#syncPoster();
    this.#applyWords();
    this.#applyControls();
    this.#syncAsking();
    if (!this.hasAttribute('data-state')) this.#setState('poster');
    this.#intersection = new IntersectionObserver((entries) => this.#onIntersect(entries), {
      rootMargin: '200px',
    });
    this.#intersection.observe(this);
  }

  disconnectedCallback() {
    this.#intersection?.disconnect();
    this.#intersection = null;
    this.#near = false;
    clearTimeout(this.#releaseTimer);
    this.#teardown();
    this.#setState('poster');
  }

  attributeChangedCallback(name, old, value) {
    if (name === 'aspect') {
      const ratio = aspectRatio(value);
      this.style.setProperty('--isotrail-aspect', ratio);
      // The snippet sizes the element before the script runs; keep it in step
      if (this.style.aspectRatio) this.style.aspectRatio = ratio;
    } else if (name === 'poster') {
      this.#syncPoster();
    } else if (name === 'src') {
      if (old !== value && this.#ready.state !== 'pending') this.#newReady();
      if (old !== value) {
        this.#pressed = false;
        this.#prefetch = null;
      }
      if (this.#started) {
        this.#teardown();
        this.#setState('poster');
        if (this.#near && !this.#waiting()) this.#start();
      }
      this.#syncAsking();
    } else if (name === 'load') {
      this.#loadMode = readLoad(this);
      this.#syncAsking();
      if (this.#near && !this.#started && !this.#waiting()) this.#start();
    } else if (name === 'backend') {
      if (value?.trim() && !['auto', 'webgl'].includes(value)) ignored(name, value, 'expected auto or webgl');
    } else if (name === 'credit') {
      if (value?.trim() && !['overlay', 'below', 'none'].includes(value)) {
        ignored(name, value, 'expected overlay, below or none');
      }
      this.#placeCredit();
    } else if (name === 'credit-extra') {
      if (this.#viewer) this.#showCredit(this.#creditText);
    } else if (name === 'lang' || name === 'strings') {
      if (this.isConnected) this.#applyWords();
    } else if (name === 'controls') {
      if (this.isConnected) this.#applyControls();
    } else if (CLIP.includes(name)) {
      this.#clip = readClip(this);
      this.#playsLeft = Math.min(this.#playsLeft, this.#clip.plays);
      this.#time = this.#playing
        ? Math.min(this.#time, this.#clip.duration)
        : this.#clip.intro + this.#progress * this.#clip.draw;
    } else if (VIEWER.includes(name) || name === 'theme') {
      if (name === 'route-colour' || name === 'route-color') {
        // The progress line follows the route unless the page sets its own
        const given = this.getAttribute('route-colour')?.trim() || this.getAttribute('route-color');
        this.#paintRoute(ROUTE_COLOUR.test(given ?? '') ? given : null);
      }
      if (this.#viewer) this.#changeLook();
    }
  }

  get playing() {
    return this.#playing;
  }

  // How much of the route is drawn, from 0 to 1, as the progress line shows
  get progress() {
    return this.#progress;
  }

  set progress(value) {
    const share = Number(value);
    if (!Number.isFinite(share)) return;
    this.#seek(Math.min(Math.max(share, 0), 1));
  }

  // Settles once the diorama has replaced the poster, or the element gives up
  get ready() {
    return this.#ready.promise;
  }

  play() {
    if (this.#playing) return;
    if (!this.#viewer) {
      this.#wantsPlay = true;
      if (this.#waiting()) this.load();
      return;
    }
    this.#wantsPlay = false;
    const clip = this.#clip;
    const restart = this.#progress >= 1 && !(this.#time > clip.settled && this.#playsLeft > 1);
    const cut = restart && clip.lead === 0;
    if (restart) {
      this.#time = -clip.lead;
      this.#playsLeft = clip.plays;
      this.#settling = !reducedMotion();
      // Under the poster's light the route undraws without moving the sun
      this.#holdLight = clip.lead > 0 && !this.#moved;
    }
    this.#moved = true;
    this.#playing = true;
    this.#last = null;
    this.#syncPlayButton();
    this.#emit('isotrail-play', { progress: cut ? 0 : this.#progress });
    if (cut) this.#setProgress(0);
    this.#schedule();
  }

  pause() {
    this.#wantsPlay = false;
    if (!this.#playing) return;
    this.#playing = false;
    this.#syncPlayButton();
    this.#emit('isotrail-pause', { progress: this.#progress });
  }

  // Back to the starting view, as the reset button does
  reset() {
    this.#resetView();
  }

  // Downloads the renderer and the scene now, whatever load says, or tries
  // again after a failure
  load() {
    if (this.getAttribute('data-state') === 'fallback') {
      this.#teardown();
      this.#setState('poster');
    }
    if (!this.#started && this.#waiting()) this.#busy = true;
    this.#pressed = true;
    if (!this.#started) this.#start();
    return this.ready;
  }

  #waiting() {
    return this.#loadMode === 'press' && !this.#pressed && !this.#started;
  }

  // A press of play on a load="press" element; with reduced motion it shows
  // the diorama without playing
  #press() {
    if (!reducedMotion()) this.#wantsPlay = true;
    this.load();
  }

  // The pointer on the play button, or focus on it, starts the downloads
  // before the press
  #headStart() {
    const src = this.getAttribute('src');
    if (!this.#waiting() || !src || saveData()) return;
    loadRuntime();
    this.#fetchScene(src);
  }

  #fetchScene(src) {
    const url = new URL(src, document.baseURI).href;
    if (this.#prefetch?.url !== url) {
      const progress = download();
      const scene = { url, progress, bytes: fetchBytes(src, progress) };
      scene.bytes.catch(() => {
        if (this.#prefetch === scene) this.#prefetch = null;
      });
      this.#prefetch = scene;
    }
    return this.#prefetch;
  }

  #syncAsking() {
    this.#frameBox.classList.toggle('asking', this.#busy || this.#waiting());
    this.#syncPlayButton();
  }

  // Between the buttons over the frame, or under it
  #placeCredit() {
    if (this.getAttribute('credit') === 'below') {
      if (this.#credit.parentElement !== this.#below) this.#below.append(this.#credit);
    } else if (this.#credit.parentElement === this.#below) {
      this.#playButton.parentElement.after(this.#credit);
    }
  }

  #paintRoute(colour) {
    for (const line of [this.#scrubber, this.#loaded]) {
      if (colour) line.style.setProperty('--_route', colour);
      else line.style.removeProperty('--_route');
    }
  }

  // The loading line follows the downloads, when all their sizes are known
  #watchLoading() {
    cancelAnimationFrame(this.#loadingFrame);
    const step = () => {
      this.#loadingFrame = 0;
      const loading = this.getAttribute('data-state') === 'loading';
      const parts = this.#downloads;
      const known = loading && parts.length > 0 && parts.every((part) => part.total !== null);
      this.#loaded.hidden = !known || this.#scrubber.hidden;
      if (known) {
        const sum = (key) => parts.reduce((total, part) => total + part[key], 0);
        this.#loaded.style.setProperty('--loaded', `${(sum('loaded') / sum('total')) * 100}%`);
      }
      if (loading) this.#loadingFrame = requestAnimationFrame(step);
    };
    step();
  }

  #newReady() {
    const ready = { state: 'pending' };
    ready.promise = new Promise((resolve, reject) => {
      ready.resolve = resolve;
      ready.reject = reject;
    });
    // A page that never reads ready sees no unhandled rejection
    ready.promise.catch(() => {});
    this.#ready = ready;
  }

  #settleReady(state, value) {
    const ready = this.#ready;
    if (ready.state !== 'pending') return;
    ready.state = state;
    if (state === 'resolved') ready.resolve(value);
    else ready.reject(value);
  }

  // Moves the drawing to `share` of the way, like the progress line
  #seek(share) {
    this.#cancelSettle();
    this.#moved = true;
    this.#holdLight = false;
    this.#time = this.#clip.intro + share * this.#clip.draw;
    this.#setProgress(share);
    this.#schedule();
  }

  #setProgress(share) {
    if (share === this.#progress) return;
    this.#progress = share;
    this.#syncScrubber();
    this.#emit('isotrail-progress', { progress: share });
  }

  // The poster attribute, or else the image the snippet puts inside the
  // element for pages where the script doesn't run
  #syncPoster() {
    const fallback = this.querySelector(':scope > img');
    const src = this.getAttribute('poster') || fallback?.getAttribute('src');
    if (src) this.#poster.src = new URL(src, document.baseURI).href;
    else this.#poster.removeAttribute('src');
    this.#poster.alt = fallback?.alt || this.#words.poster;
    this.#frameBox.classList.toggle('has-poster', Boolean(src));
  }

  #applyWords() {
    const words = readWords(this);
    this.#words = words;
    this.#syncPlayButton();
    this.#zoomIn.setAttribute('aria-label', words.zoomIn);
    this.#zoomOut.setAttribute('aria-label', words.zoomOut);
    this.#resetButton.setAttribute('aria-label', words.reset);
    this.#scrubber.setAttribute('aria-label', words.scrubber);
    this.#help.textContent = fill(words.help, { key: ZOOM_KEY });
    this.#message.textContent = words[this.#failure ?? 'gpu'] ?? words.runtime;
    this.#syncPoster();
    this.#syncScrubber();
    this.#labelCanvas();
  }

  #applyControls() {
    const shown = readControls(this);
    this.#playButton.hidden = !shown.has('play');
    this.#zoomIn.hidden = !shown.has('zoom');
    this.#zoomOut.hidden = !shown.has('zoom');
    this.#resetButton.hidden = !shown.has('reset');
    this.#scrubber.hidden = !shown.has('scrubber');
  }

  #labelCanvas() {
    this.#canvas?.setAttribute(
      'aria-label',
      fill(this.#words.diorama, { title: this.#title || this.#words.untitled }),
    );
  }

  #onIntersect(entries) {
    this.#near = entries[entries.length - 1].isIntersecting;
    clearTimeout(this.#releaseTimer);
    if (this.#near) {
      if (!this.#started) {
        if (!this.#waiting()) this.#start();
      } else if (this.#playing || this.#weatherMoves()) this.#schedule();
    } else if (this.#viewer) {
      this.#releaseTimer = setTimeout(() => this.#release(), RELEASE_MS);
    }
  }

  #release() {
    if (this.#near || !this.#viewer) return;
    this.#teardown();
    this.#setState('poster');
  }

  async #start() {
    this.#started = true;
    const generation = ++this.#generation;
    const src = this.getAttribute('src');
    if (!src) {
      this.#busy = false;
      this.#syncAsking();
      return;
    }
    this.#syncAsking();
    if (this.#ready.state === 'rejected') this.#newReady();
    if (!hasGpu()) {
      this.#fail('gpu', new Error('the browser has neither WebGPU nor WebGL2'));
      return;
    }
    const renderer = loadRuntime();
    const scene = this.#fetchScene(src);
    // Each start downloads the scene again; only a head start's is reused
    this.#prefetch = null;
    // Only what still has to arrive counts on the loading line
    this.#downloads = [runtimeProgress, scene.progress].filter((part) => !part.done);
    this.#setState('loading');
    this.#watchLoading();
    let module;
    let bytes;
    let look;
    try {
      [module, bytes, look] = await Promise.all([renderer, scene.bytes, this.#look()]);
    } catch (error) {
      if (generation === this.#generation) this.#fail('load', error);
      return;
    }
    if (generation !== this.#generation) return;
    const canvas = this.#makeCanvas();
    const backend = this.getAttribute('backend') === 'webgl' ? 'webgl' : 'auto';
    let viewer;
    try {
      viewer = await module.Viewer.load(canvas, bytes, backend, look);
    } catch (error) {
      if (generation === this.#generation) this.#fail(error?.reason ?? 'runtime', error);
      return;
    }
    if (generation !== this.#generation) {
      viewer.free();
      return;
    }
    this.#viewer = viewer;
    const lost = (message) => {
      if (this.#viewer === viewer) this.#onLost(message);
    };
    canvas.addEventListener('webglcontextlost', () => lost('the WebGL context was lost'));
    this.#guard(() => {
      viewer.onLost(lost);
      this.#webgl = viewer.backend === 'gl';
      this.#title = viewer.title;
      this.#labelCanvas();
      this.#showCredit(viewer.credit);
      this.#followRoute();
    });
    if (this.#viewer !== viewer) return;
    this.#resize = new ResizeObserver(([entry]) => this.#onResize(entry));
    this.#resize.observe(canvas);
  }

  // The look attributes as the renderer takes them, with the text of a .hex
  // palette and a theme file in place of their addresses
  async #look() {
    const look = {};
    for (const attribute of VIEWER) {
      const value = this.getAttribute(attribute);
      if (value !== null) look[attribute] = value;
    }
    const theme = this.getAttribute('theme');
    const [palette, text] = await Promise.all([
      look.palette ? readPalette(look.palette) : undefined,
      theme?.trim() ? fetchText(theme, 'theme') : undefined,
    ]);
    if (look.palette) look.palette = palette;
    if (text !== undefined) look.theme = text;
    return look;
  }

  // Only the latest of several quick changes is applied
  async #changeLook() {
    const viewer = this.#viewer;
    const change = ++this.#lookChanges;
    const look = await this.#look();
    if (this.#viewer !== viewer || change !== this.#lookChanges) return;
    this.#guard(() => viewer.setOptions(look));
    this.#followRoute();
    this.#syncZoom();
    this.#schedule();
  }

  // The progress line takes the route's colour, from its attribute or the
  // theme
  #followRoute() {
    const colour = this.#guard((viewer) => viewer.routeColour);
    if (colour) this.#paintRoute(colour);
  }

  // One fresh start, in case the browser reset its GPU; the poster if it
  // happens again within a minute
  #onLost(message) {
    console.warn('<isotrail-hike> lost the GPU:', message);
    const now = performance.now();
    if (this.#restartedAt !== null && now - this.#restartedAt < RESTART_WINDOW_MS) {
      this.#fail('gpu', new Error(message));
      return;
    }
    this.#restartedAt = now;
    this.#teardown();
    this.#setState('poster');
    if (this.#near) this.#start();
  }

  #fail(reason, error) {
    console.warn(`<isotrail-hike> could not show ${this.getAttribute('src')}:`, error);
    this.#teardown();
    // Stays put until src changes, load() is called or the element is put
    // back in the page
    this.#started = true;
    this.#syncAsking();
    this.#wantsPlay = false;
    this.#failure = reason in this.#words ? reason : 'runtime';
    this.#message.textContent = this.#words[this.#failure];
    this.#setState('fallback');
    const message = String(error?.message ?? error ?? '');
    this.#emit('isotrail-error', { reason, message });
    const failure = new Error(message);
    failure.reason = reason;
    this.#settleReady('rejected', failure);
  }

  #emit(name, detail) {
    this.dispatchEvent(new CustomEvent(name, { detail, bubbles: true, composed: true }));
  }

  // Calls into the renderer; an error there ends in the fallback rather
  // than a frozen picture with live controls
  #guard(call) {
    const viewer = this.#viewer;
    if (!viewer) return undefined;
    try {
      return call(viewer);
    } catch (error) {
      if (this.#viewer === viewer) this.#fail(error?.reason ?? 'runtime', error);
      return undefined;
    }
  }

  #teardown() {
    const wasPlaying = this.#playing;
    this.#generation++;
    this.#started = false;
    this.#playing = false;
    this.#settling = false;
    cancelAnimationFrame(this.#frame);
    this.#frame = 0;
    this.#resize?.disconnect();
    this.#resize = null;
    try {
      this.#viewer?.free();
    } catch {
      // A renderer that has already failed may not free cleanly
    }
    this.#viewer = null;
    // Gives the context back now, rather than whenever it is collected
    if (this.#webgl) this.#canvas?.getContext('webgl2')?.getExtension('WEBGL_lose_context')?.loseContext();
    this.#webgl = false;
    this.#canvas?.remove();
    this.#canvas = null;
    this.#drawn = false;
    this.#sized = false;
    this.#clockStart = null;
    this.#pointers.clear();
    this.#progress = 1;
    this.#moved = false;
    this.#holdLight = false;
    this.#time = this.#clip.settled;
    this.#playsLeft = 1;
    this.#yaw = 0;
    this.#failure = null;
    this.#busy = false;
    this.#downloads = [];
    cancelAnimationFrame(this.#loadingFrame);
    this.#loadingFrame = 0;
    this.#loaded.hidden = true;
    this.#syncAsking();
    this.#syncScrubber();
    if (wasPlaying) this.#emit('isotrail-pause', { progress: this.#progress });
  }

  // A fresh canvas for each load, as a canvas keeps the first kind of
  // context it gives out
  #makeCanvas() {
    const canvas = document.createElement('canvas');
    canvas.setAttribute('part', 'canvas');
    canvas.tabIndex = 0;
    canvas.setAttribute('role', 'img');
    canvas.setAttribute('aria-describedby', 'help');
    canvas.addEventListener('pointerdown', (event) => this.#onPointerDown(event));
    canvas.addEventListener('pointermove', (event) => this.#onPointerMove(event));
    canvas.addEventListener('pointerup', (event) => this.#onPointerUp(event));
    canvas.addEventListener('pointercancel', (event) => this.#onPointerUp(event));
    canvas.addEventListener('contextmenu', (event) => event.preventDefault());
    canvas.addEventListener('wheel', (event) => this.#onWheel(event), { passive: false });
    canvas.addEventListener('keydown', (event) => this.#onKey(event));
    this.#poster.after(canvas);
    this.#canvas = canvas;
    return canvas;
  }

  // The data credits, each kept on one line, with links where the licences
  // ask for them, and the page's own line under them
  #showCredit(text) {
    this.#creditText = text;
    const names = Object.keys(CREDIT_LINKS).map((name) => name.replace(/[.]/g, '\\.'));
    const linked = new RegExp(`(${names.join('|')})`);
    const sources = text ? text.split(' · ') : [];
    this.#credit.replaceChildren(
      ...sources.flatMap((source, i) => {
        const span = document.createElement('span');
        span.append(
          ...source.split(linked).map((part, j) => {
            if (j % 2 === 0) return part;
            const link = document.createElement('a');
            link.href = CREDIT_LINKS[part];
            link.target = '_blank';
            link.rel = 'noopener';
            link.textContent = part;
            return link;
          }),
        );
        return i === 0 ? [span] : [' · ', span];
      }),
    );
    const extra = this.getAttribute('credit-extra')?.trim();
    if (extra) {
      const line = document.createElement('span');
      line.className = 'extra';
      line.textContent = extra;
      this.#credit.append(line);
    }
  }

  #setState(state) {
    if (this.getAttribute('data-state') === state) return;
    this.setAttribute('data-state', state);
    this.#emit('isotrail-state', { state });
  }

  #onResize(entry) {
    const box = entry.devicePixelContentBoxSize?.[0];
    const width = box ? box.inlineSize : Math.round(entry.contentRect.width * devicePixelRatio);
    const height = box ? box.blockSize : Math.round(entry.contentRect.height * devicePixelRatio);
    if (!this.#viewer) return;
    // The viewer sizes the canvas's buffer, within the GPU's largest texture
    this.#sized = width > 0 && height > 0;
    this.#guard((viewer) => viewer.resize(width, height));
    if (!this.#viewer) return;
    this.#syncZoom();
    const autoplay = this.hasAttribute('autoplay') && !reducedMotion();
    if (!this.#drawn && width > 0 && (this.#wantsPlay || autoplay)) this.play();
    this.#schedule();
  }

  #schedule() {
    if (this.#frame || !this.#viewer) return;
    this.#frame = requestAnimationFrame((now) => this.#draw(now));
  }

  #draw(now) {
    this.#frame = 0;
    if (!this.#viewer) return;
    if (this.#playing) this.#advance(now);
    const drawn = this.#guard((viewer) => {
      if (this.#holdLight) viewer.setRoute(this.#progress);
      else if (this.#moved) viewer.setProgress(this.#progress);
      viewer.setYaw(this.#yaw + (this.#settling ? this.#clip.yaw(this.#time) : 0));
      this.#clockStart ??= now;
      viewer.setTime(reducedMotion() ? 0 : (now - this.#clockStart) / 1000);
      viewer.draw();
      return true;
    });
    if (!drawn) return;
    if (!this.#drawn && this.#sized) {
      this.#drawn = true;
      // The play button stays shown through the change, keeping its focus
      this.#setState('ready');
      this.#busy = false;
      this.#syncAsking();
      const detail = { backend: this.#guard((viewer) => viewer.backend) };
      this.#emit('isotrail-ready', detail);
      this.#settleReady('resolved', detail);
    }
    if (this.#playing) {
      if (this.#near) this.#schedule();
      else this.#last = null;
    } else if (this.#near && this.#weatherMoves()) {
      this.#schedule();
    }
  }

  // Moves the clip on to `now`, into the next play when it loops, and stops
  // on the finished hike after the last
  #advance(now) {
    const clip = this.#clip;
    if (this.#last !== null) this.#time += Math.min((now - this.#last) / 1000, MAX_STEP_S);
    this.#last = now;
    if (this.#playsLeft > 1 && this.#time >= clip.duration) {
      this.#time = Math.min(this.#time - clip.duration, clip.duration);
      this.#playsLeft -= 1;
    }
    if (this.#time >= 0) this.#holdLight = false;
    const ended = this.#playsLeft <= 1 && this.#time >= clip.settled;
    if (ended) this.#time = clip.settled;
    this.#setProgress(clip.share(this.#time));
    if (ended) {
      this.#playing = false;
      this.#settling = false;
      this.#syncPlayButton();
      this.#emit('isotrail-pause', { progress: this.#progress });
      this.#emit('isotrail-ended', { progress: this.#progress });
    }
  }

  // Rain and snow keep falling while the hike is on screen
  #weatherMoves() {
    return !reducedMotion() && Boolean(this.#guard((viewer) => viewer.weatherMoves));
  }

  #cancelSettle() {
    if (!this.#settling) return;
    this.#yaw += this.#clip.yaw(this.#time);
    this.#settling = false;
  }

  #turn(degrees) {
    this.#cancelSettle();
    this.#yaw += degrees;
    this.#schedule();
  }

  #pan(dx, dy) {
    this.#guard((viewer) => viewer.pan(dx, dy));
    this.#schedule();
  }

  #zoom(factor, x, y) {
    const canvas = this.#canvas;
    if (!canvas) return;
    this.#guard((viewer) => viewer.zoomBy(factor, x ?? canvas.width / 2, y ?? canvas.height / 2));
    this.#syncZoom();
    this.#schedule();
  }

  #resetView() {
    this.#settling = false;
    this.#yaw = 0;
    this.#guard((viewer) => viewer.resetView());
    this.#syncZoom();
    this.#schedule();
  }

  #zoomed() {
    return (this.#guard((viewer) => viewer.zoom) ?? 1) > 1.001;
  }

  // Canvas pixels per CSS pixel, and a pointer's position in canvas pixels
  #canvasScale() {
    const rect = this.#canvas.getBoundingClientRect();
    return { rect, scale: rect.width > 0 ? this.#canvas.width / rect.width : devicePixelRatio };
  }

  #toCanvas(clientX, clientY) {
    const { rect, scale } = this.#canvasScale();
    return [(clientX - rect.left) * scale, (clientY - rect.top) * scale];
  }

  #onButton(event) {
    const button = event.target.closest('button');
    if (!button || button.getAttribute('aria-disabled') === 'true') return;
    if (button.dataset.action === 'play' && this.#waiting()) {
      this.#press();
      return;
    }
    if (!this.#viewer) return;
    switch (button.dataset.action) {
      case 'play':
        if (this.#playing) this.pause();
        else this.play();
        break;
      case 'zoom-in':
        this.#zoom(KEY_ZOOM);
        break;
      case 'zoom-out':
        this.#zoom(1 / KEY_ZOOM);
        break;
      case 'reset':
        this.#resetView();
        break;
    }
  }

  #onPointerDown(event) {
    if (event.pointerType === 'mouse' && event.button !== 0 && event.button !== 2) return;
    this.#canvas.setPointerCapture(event.pointerId);
    this.#canvas.classList.add('dragging');
    this.#pointers.set(event.pointerId, { x: event.clientX, y: event.clientY });
  }

  #onPointerMove(event) {
    const previous = this.#pointers.get(event.pointerId);
    if (!previous || !this.#viewer) return;
    const current = { x: event.clientX, y: event.clientY };
    if (this.#pointers.size === 1) {
      const dx = current.x - previous.x;
      const dy = current.y - previous.y;
      if (!this.#zoomed() || event.shiftKey || event.buttons & 2) {
        this.#turn(-dx * TURN_PER_PX);
      } else {
        const { scale } = this.#canvasScale();
        this.#pan(dx * scale, dy * scale);
      }
    } else if (this.#pointers.size === 2) {
      const other = [...this.#pointers].find(([id]) => id !== event.pointerId)[1];
      const before = pinch(previous, other);
      const after = pinch(current, other);
      if (before.distance > 0) {
        const [x, y] = this.#toCanvas(after.x, after.y);
        this.#zoom(after.distance / before.distance, x, y);
      }
      if (!this.#canvas) return;
      const { scale } = this.#canvasScale();
      this.#pan((after.x - before.x) * scale, (after.y - before.y) * scale);
      const twist = Math.atan2(Math.sin(after.angle - before.angle), Math.cos(after.angle - before.angle));
      this.#turn(twist * (180 / Math.PI));
    }
    this.#pointers.set(event.pointerId, current);
  }

  #onPointerUp(event) {
    this.#pointers.delete(event.pointerId);
    if (this.#pointers.size === 0) this.#canvas?.classList.remove('dragging');
  }

  #onWheel(event) {
    if (!this.#viewer) return;
    if (!event.ctrlKey && !event.metaKey) {
      this.#showHint(fill(this.#words.wheelHint, { key: ZOOM_KEY }));
      return;
    }
    event.preventDefault();
    const step = event.deltaMode === WheelEvent.DOM_DELTA_LINE ? 0.05 : 0.002;
    const [x, y] = this.#toCanvas(event.clientX, event.clientY);
    this.#zoom(Math.exp(-event.deltaY * step), x, y);
  }

  #onKey(event) {
    if (!this.#viewer || event.altKey || event.ctrlKey || event.metaKey) return;
    const { scale } = this.#canvasScale();
    const pan = KEY_PAN_PX * scale;
    const moving = event.shiftKey && this.#zoomed();
    switch (event.key) {
      case 'ArrowLeft':
        if (moving) this.#pan(pan, 0);
        else this.#turn(KEY_TURN_DEG);
        break;
      case 'ArrowRight':
        if (moving) this.#pan(-pan, 0);
        else this.#turn(-KEY_TURN_DEG);
        break;
      case 'ArrowUp':
        if (!moving) return;
        this.#pan(0, pan);
        break;
      case 'ArrowDown':
        if (!moving) return;
        this.#pan(0, -pan);
        break;
      case '+':
      case '=':
        this.#zoom(KEY_ZOOM);
        break;
      case '-':
      case '_':
        this.#zoom(1 / KEY_ZOOM);
        break;
      case '0':
      case 'Home':
        this.#resetView();
        break;
      case ' ':
      case 'k':
      case 'K':
        if (this.#playing) this.pause();
        else this.play();
        break;
      default:
        return;
    }
    event.preventDefault();
  }

  #showHint(text) {
    if (!this.#hint.classList.contains('shown')) this.#hint.textContent = text;
    this.#hint.classList.add('shown');
    clearTimeout(this.#hintTimer);
    this.#hintTimer = setTimeout(() => this.#hint.classList.remove('shown'), HINT_MS);
  }

  #syncPlayButton() {
    const icon = this.#busy ? 'busy' : this.#playing ? 'pause' : 'play';
    // A fresh icon would restart the busy dots
    if (icon !== this.#icon) this.#playButton.innerHTML = ICONS[icon];
    this.#icon = icon;
    this.#playButton.setAttribute('aria-label', this.#words[icon === 'busy' ? 'loading' : icon]);
  }

  #syncScrubber() {
    this.#scrubber.value = Math.round(this.#progress * 1000);
    this.#scrubber.style.setProperty('--walked', `${this.#progress * 100}%`);
    this.#scrubber.setAttribute(
      'aria-valuetext',
      fill(this.#words.progress, { percent: Math.round(this.#progress * 100) }),
    );
  }

  #syncZoom() {
    const zoom = this.#guard((viewer) => viewer.zoom);
    const max = this.#guard((viewer) => viewer.maxZoom);
    if (zoom === undefined || !this.#canvas) return;
    this.#zoomIn.setAttribute('aria-disabled', String(zoom >= max - 0.001));
    this.#zoomOut.setAttribute('aria-disabled', String(zoom <= 1.001));
    this.#canvas.style.touchAction = zoom > 1.001 ? 'none' : 'pan-y';
  }
}

function pinch(a, b) {
  return {
    x: (a.x + b.x) / 2,
    y: (a.y + b.y) / 2,
    distance: Math.hypot(b.x - a.x, b.y - a.y),
    angle: Math.atan2(b.y - a.y, b.x - a.x),
  };
}

// Texts fetched once for every element that names them
const texts = new Map();
async function fetchText(value, what) {
  const url = new URL(value, document.baseURI).href;
  if (!texts.has(url)) {
    const loading = fetch(url).then(async (response) => {
      if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
      return response.text();
    });
    loading.catch(() => texts.delete(url));
    texts.set(url, loading);
  }
  try {
    return await texts.get(url);
  } catch (error) {
    console.warn(`<isotrail-hike> could not load the ${what} ${value}, so it uses the default:`, error);
    return undefined;
  }
}

// A palette's name as it is, or the text of a .hex file at that address
async function readPalette(value) {
  const url = new URL(value, document.baseURI);
  if (!url.pathname.toLowerCase().endsWith('.hex')) return value;
  return fetchText(value, 'palette');
}

async function fetchBytes(src, progress) {
  const response = counted(await fetch(new URL(src, document.baseURI)), progress);
  if (!response.ok) throw new Error(`${src}: ${response.status} ${response.statusText}`);
  const bytes = new Uint8Array(await response.arrayBuffer());
  progress.done = true;
  return bytes;
}

if (!customElements.get('isotrail-hike')) {
  customElements.define('isotrail-hike', IsotrailHike);
}
