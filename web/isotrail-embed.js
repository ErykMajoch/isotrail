// <isotrail-hike>: an interactive diorama of a hike for any web page

const RUNTIME = new URL('./pkg/isotrail_web.js', import.meta.url);

const ASPECTS = { story: '9 / 16', feed: '4 / 5', square: '1 / 1', wide: '16 / 9' };
const INTRO_S = 0.5;
const DRAW_S = 8;
const ORBIT_DEG = 30;
const MAX_ZOOM = 3;
const TURN_PER_PX = 0.4;
const KEY_TURN_DEG = 10;
const KEY_PAN_PX = 40;
const KEY_ZOOM = 1.25;
const HINT_MS = 1500;

const MAC = /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent);
const ZOOM_KEY = MAC ? '⌘' : 'Ctrl';

let runtime;
function loadRuntime() {
  runtime ??= import(RUNTIME.href).then(async (module) => {
    await module.default();
    return module;
  });
  return runtime;
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

// Slow at both ends, as the animation's camera settles
function ease(x) {
  return (1 - Math.cos(Math.PI * Math.min(Math.max(x, 0), 1))) / 2;
}

function orbit(time) {
  return -ORBIT_DEG * (1 - ease(time / (INTRO_S + DRAW_S)));
}

const icon = (body) =>
  `<svg viewBox="0 0 16 16" aria-hidden="true" shape-rendering="crispEdges">${body}</svg>`;
const ICONS = {
  play: icon('<path d="M5 3h2v1h2v1h2v1h2v4h-2v1H9v1H7v1H5z" fill="currentColor"/>'),
  pause: icon('<path d="M4 3h3v10H4zM9 3h3v10H9z" fill="currentColor"/>'),
  zoomIn: icon('<path d="M7 3h2v4h4v2H9v4H7V9H3V7h4z" fill="currentColor"/>'),
  zoomOut: icon('<path d="M3 7h10v2H3z" fill="currentColor"/>'),
  reset: icon(
    '<path d="M2 2h5v2H4v3H2zM9 2h5v5h-2V4H9zM12 9h2v5H9v-2h3zM2 9h2v3h3v2H2z" fill="currentColor"/>',
  ),
};

const template = document.createElement('template');
template.innerHTML = `
<style>
  :host {
    --isotrail-panel: rgb(32 46 55 / 0.78);
    --isotrail-text: #ebede9;
    --isotrail-accent: #a4dddb;
    --isotrail-background: transparent;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    contain: content;
  }
  :host([hidden]) { display: none; }
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
  }
  canvas.dragging { cursor: grabbing; }
  canvas:focus-visible { box-shadow: inset 0 0 0 2px var(--isotrail-accent); }
  :host([data-state="ready"]) canvas { opacity: 1; }
  :host([data-state="ready"]) img { visibility: hidden; }
  .message {
    display: none;
    place-items: center;
    padding: 16px;
    box-sizing: border-box;
    text-align: center;
    font: 14px/1.4 system-ui, sans-serif;
    color: var(--isotrail-text);
    background: var(--isotrail-panel);
  }
  :host([data-state="fallback"]:not([poster])) .message { display: grid; }
  .controls {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 4px;
    background: var(--isotrail-panel);
    color: var(--isotrail-text);
  }
  :host(:not([data-state="ready"])) .controls > * { visibility: hidden; }
  :host([data-state="fallback"]) .controls { display: none; }
  button {
    flex: none;
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    padding: 0;
    border: 0;
    border-radius: 4px;
    background: transparent;
    color: inherit;
    cursor: pointer;
  }
  button:hover { background: rgb(255 255 255 / 0.12); }
  button[aria-disabled="true"] { opacity: 0.4; cursor: default; }
  button:focus-visible, input:focus-visible {
    outline: 2px solid var(--isotrail-accent);
    outline-offset: 1px;
  }
  svg { width: 16px; height: 16px; }
  input {
    flex: 1;
    min-width: 40px;
    margin: 0 4px;
    accent-color: var(--isotrail-accent);
  }
  .hint {
    position: absolute;
    left: 50%;
    top: 50%;
    transform: translate(-50%, -50%);
    padding: 6px 10px;
    border-radius: 4px;
    background: var(--isotrail-panel);
    color: var(--isotrail-text);
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
</style>
<div class="frame">
  <img alt="Hike diorama">
  <div class="message">This browser can't show the 3D view.</div>
  <div class="hint" aria-live="polite"></div>
</div>
<div class="controls">
  <button type="button" part="play" data-action="play" aria-label="Play">${ICONS.play}</button>
  <input type="range" part="scrubber" min="0" max="1000" value="1000" aria-label="Route drawn">
  <button type="button" part="zoom-out" data-action="zoom-out" aria-label="Zoom out">${ICONS.zoomOut}</button>
  <button type="button" part="zoom-in" data-action="zoom-in" aria-label="Zoom in">${ICONS.zoomIn}</button>
  <button type="button" part="reset" data-action="reset" aria-label="Reset view">${ICONS.reset}</button>
</div>
<p class="help" id="help">Drag to turn the diorama. Pinch, or hold ${ZOOM_KEY} and scroll, to zoom; once zoomed in, drag to move around and Shift-drag to turn. Keys: left and right arrows turn, plus and minus zoom, Shift with the arrows moves, 0 resets the view, Space plays and pauses.</p>
`;

class IsotrailHike extends HTMLElement {
  static observedAttributes = ['src', 'poster', 'aspect'];

  #poster;
  #canvas = null;
  #controls;
  #playButton;
  #scrubber;
  #zoomIn;
  #zoomOut;
  #hint;
  #hintTimer = 0;

  #viewer = null;
  #generation = 0;
  #started = false;
  #near = false;
  #intersection = null;
  #resize = null;
  #frame = 0;
  #drawn = false;

  #progress = 1;
  #yaw = 0;
  #time = INTRO_S + DRAW_S;
  #playing = false;
  #settling = false;
  #last = null;
  #pointers = new Map();

  constructor() {
    super();
    const root = this.attachShadow({ mode: 'open' });
    root.append(template.content.cloneNode(true));
    this.#poster = root.querySelector('img');
    this.#controls = root.querySelector('.controls');
    this.#playButton = root.querySelector('[data-action="play"]');
    this.#scrubber = root.querySelector('input');
    this.#zoomIn = root.querySelector('[data-action="zoom-in"]');
    this.#zoomOut = root.querySelector('[data-action="zoom-out"]');
    this.#hint = root.querySelector('.hint');

    this.#controls.addEventListener('click', (event) => this.#onButton(event));
    this.#scrubber.addEventListener('input', () => {
      this.pause();
      this.#cancelSettle();
      this.#progress = this.#scrubber.value / 1000;
      this.#time = INTRO_S + this.#progress * DRAW_S;
      this.#syncScrubber();
      this.#schedule();
    });
  }

  connectedCallback() {
    if (!this.hasAttribute('data-state')) this.#setState('poster');
    this.#intersection = new IntersectionObserver(
      ([entry]) => {
        this.#near = entry.isIntersecting;
        if (this.#near && !this.#started) this.#start();
        if (this.#near && this.#playing) this.#schedule();
      },
      { rootMargin: '200px' },
    );
    this.#intersection.observe(this);
  }

  disconnectedCallback() {
    this.#intersection?.disconnect();
    this.#intersection = null;
    this.#teardown();
    this.#setState('poster');
  }

  attributeChangedCallback(name, _old, value) {
    if (name === 'aspect') {
      const ratio = ASPECTS[value] ?? ASPECTS.wide;
      this.style.setProperty('--isotrail-aspect', ratio);
    } else if (name === 'poster') {
      if (value) this.#poster.src = value;
      else this.#poster.removeAttribute('src');
    } else if (name === 'src' && this.#started) {
      this.#teardown();
      this.#setState('poster');
      if (this.#near) this.#start();
    }
  }

  get playing() {
    return this.#playing;
  }

  play() {
    if (!this.#viewer || this.#playing) return;
    if (this.#progress >= 1) {
      this.#time = 0;
      this.#progress = 0;
      this.#settling = !reducedMotion();
    } else {
      this.#time = INTRO_S + this.#progress * DRAW_S;
    }
    this.#playing = true;
    this.#last = null;
    this.#syncPlayButton();
    this.#schedule();
  }

  pause() {
    if (!this.#playing) return;
    this.#playing = false;
    this.#syncPlayButton();
  }

  async #start() {
    this.#started = true;
    const generation = ++this.#generation;
    const src = this.getAttribute('src');
    if (!src) return;
    if (!hasGpu()) {
      this.#setState('fallback');
      return;
    }
    this.#setState('loading');
    try {
      const [{ Viewer }, bytes] = await Promise.all([loadRuntime(), fetchBytes(src)]);
      if (generation !== this.#generation) return;
      const canvas = this.#makeCanvas();
      const viewer = await Viewer.load(canvas, bytes, this.getAttribute('backend') || 'auto');
      if (generation !== this.#generation) {
        viewer.free();
        return;
      }
      this.#viewer = viewer;
      canvas.setAttribute('aria-label', `3D diorama of ${viewer.title || 'a hike'}`);
      this.#resize = new ResizeObserver(([entry]) => this.#onResize(entry));
      this.#resize.observe(canvas);
    } catch (error) {
      if (generation !== this.#generation) return;
      console.warn(`<isotrail-hike> could not show ${src}:`, error);
      this.#teardown();
      this.#setState('fallback');
    }
  }

  #teardown() {
    this.#generation++;
    this.#started = false;
    this.#playing = false;
    this.#settling = false;
    cancelAnimationFrame(this.#frame);
    this.#frame = 0;
    this.#resize?.disconnect();
    this.#resize = null;
    this.#viewer?.free();
    this.#viewer = null;
    this.#canvas?.remove();
    this.#canvas = null;
    this.#drawn = false;
    this.#pointers.clear();
    this.#progress = 1;
    this.#time = INTRO_S + DRAW_S;
    this.#yaw = 0;
    this.#syncPlayButton();
    this.#syncScrubber();
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

  #setState(state) {
    this.setAttribute('data-state', state);
  }

  #onResize(entry) {
    const box = entry.devicePixelContentBoxSize?.[0];
    const width = box ? box.inlineSize : Math.round(entry.contentRect.width * devicePixelRatio);
    const height = box ? box.blockSize : Math.round(entry.contentRect.height * devicePixelRatio);
    this.#canvas.width = width;
    this.#canvas.height = height;
    this.#viewer.resize(width, height);
    this.#syncZoom();
    if (!this.#drawn && width > 0 && this.hasAttribute('autoplay') && !reducedMotion()) {
      this.play();
    }
    this.#schedule();
  }

  #schedule() {
    if (this.#frame || !this.#viewer) return;
    this.#frame = requestAnimationFrame((now) => this.#draw(now));
  }

  #draw(now) {
    this.#frame = 0;
    const viewer = this.#viewer;
    if (!viewer) return;
    if (this.#playing) {
      if (this.#last !== null) this.#time += (now - this.#last) / 1000;
      this.#last = now;
      this.#progress = Math.min(Math.max((this.#time - INTRO_S) / DRAW_S, 0), 1);
      if (this.#time >= INTRO_S + DRAW_S) {
        this.#time = INTRO_S + DRAW_S;
        this.#playing = false;
        this.#settling = false;
        this.#syncPlayButton();
      }
      this.#syncScrubber();
    }
    viewer.setProgress(this.#progress);
    viewer.setYaw(this.#yaw + (this.#settling ? orbit(this.#time) : 0));
    viewer.draw();
    if (!this.#drawn && this.#canvas.width > 0) {
      this.#drawn = true;
      this.#setState('ready');
    }
    if (this.#playing) {
      if (this.#near) this.#schedule();
      else this.#last = null;
    }
  }

  #cancelSettle() {
    if (!this.#settling) return;
    this.#yaw += orbit(this.#time);
    this.#settling = false;
  }

  #turn(degrees) {
    this.#cancelSettle();
    this.#yaw += degrees;
    this.#schedule();
  }

  #pan(dx, dy) {
    this.#viewer.pan(dx, dy);
    this.#schedule();
  }

  #zoom(factor, x, y) {
    const canvas = this.#canvas;
    this.#viewer.zoomBy(factor, x ?? canvas.width / 2, y ?? canvas.height / 2);
    this.#syncZoom();
    this.#schedule();
  }

  #resetView() {
    this.#settling = false;
    this.#yaw = 0;
    this.#viewer.resetView();
    this.#syncZoom();
    this.#schedule();
  }

  #zoomed() {
    return this.#viewer.zoom > 1.001;
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
    if (!button || button.getAttribute('aria-disabled') === 'true' || !this.#viewer) return;
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
      this.#showHint(`Hold ${ZOOM_KEY} and scroll to zoom`);
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
    this.#playButton.innerHTML = this.#playing ? ICONS.pause : ICONS.play;
    this.#playButton.setAttribute('aria-label', this.#playing ? 'Pause' : 'Play');
  }

  #syncScrubber() {
    this.#scrubber.value = Math.round(this.#progress * 1000);
    this.#scrubber.setAttribute('aria-valuetext', `${Math.round(this.#progress * 100)}%`);
  }

  #syncZoom() {
    if (!this.#viewer) return;
    const zoom = this.#viewer.zoom;
    this.#zoomIn.setAttribute('aria-disabled', String(zoom >= MAX_ZOOM - 0.001));
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

async function fetchBytes(src) {
  const response = await fetch(new URL(src, document.baseURI));
  if (!response.ok) throw new Error(`${src}: ${response.status} ${response.statusText}`);
  return new Uint8Array(await response.arrayBuffer());
}

if (!customElements.get('isotrail-hike')) {
  customElements.define('isotrail-hike', IsotrailHike);
}
