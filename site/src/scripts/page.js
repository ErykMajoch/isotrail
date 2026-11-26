// The page: a GPX file dropped or chosen is built into a scene in the scene
// worker, and the scene or a sample walk shows in the <isotrail-hike>
// element. The controls below it change the diorama as they change
//
// The browser checks read and drive the page through `window.__scenes`,
// `window.__scene` and `window.__controls`. They may set `window.__options`
// to options added to every build, `window.__servers` to their own Overpass
// servers, `window.__skip` to the names of built-in ones to leave out and
// `window.__recycle` to the worker memory a build may leave behind

import {
  ADJUST, CAMERA, Control, FIRST, GROUPS, LEFT_OUT, OPTIONS, REBUILD, SHAPES, label, place, route, serverBoxes,
} from './controls.js';
import { Settings } from './settings.js';

const runtime = document.body.dataset.runtime;
const { SceneWorker } = await import(/* @vite-ignore */ `${runtime}scene/scene.js`);

let scenes = new SceneWorker();
const $ = (id) => document.getElementById(id);
// A reload starts with no hike, so the place it was scrolled to means nothing
history.scrollRestoration = 'manual';
const hike = $('hike');

const STEPS = {
  elevation: 'Fetching elevation tiles',
  'map-data': 'Fetching map data',
  weather: 'Fetching the weather',
  building: 'Building the scene',
};
const KINDS = { elevation: 'elevation', 'map-data': 'map data', weather: 'weather' };

// What went wrong, in a line above the message
const LEADS = {
  input: "This file can't be used.",
  usage: "An option isn't right.",
  'no-ground': "There's no ground to draw.",
  'too-large': 'This route covers too much land for map data in a browser.',
  'no-server': 'No map data server is left to ask.',
  busy: 'A data service is busy.',
  unreachable: "A data service can't be reached.",
  network: 'A download failed.',
  offline: 'This browser is offline.',
  other: 'The scene could not be built.',
};
// Failures that may go away on another try
const RETRY = ['busy', 'unreachable', 'network', 'offline', 'other'];
// Messages that offer a build without map data end with this
const WITHOUT_MAP_DATA = 'without lakes, woods and paths';
// The value of the palette list for a palette file
const PALETTE_FILE = 'file';
// Options never kept between visits: the camera follows each hike
const NOT_KEPT = [...CAMERA, 'theme'];
// The scene worker's WebAssembly memory never shrinks, so a worker that
// grew past this for a large route is replaced by a fresh one
const RECYCLE_BYTES = 96 * 1024 * 1024;

let build;
let last;
let countdown;
let step;
// The scene shown: `original` is the scene as built or as the sample ships,
// `built` the options it was built with, `bytes` and `src` what shows now
let current;
let shownUrl;
let paletteFile;
let themeUrl;
let adjusting = 0;
let work = Promise.resolve();

window.__scenes = scenes;
window.__scene = { state: 'idle', events: [] };

// A message as a sentence, keeping the case of a file name that starts it
function sentence(text, name) {
  const start = name && text.startsWith(name) ? text : text.charAt(0).toUpperCase() + text.slice(1);
  return /[.?!]$/.test(start) ? start : `${start}.`;
}

function announce(text) {
  $('status').textContent = text;
}

function showError(reason, text) {
  $('error-lead').textContent = LEADS[reason] ?? LEADS.other;
  $('error-text').textContent = text;
  $('retry').hidden = !(last && RETRY.includes(reason));
  $('no-map-data').hidden = !(last && text.includes(WITHOUT_MAP_DATA));
  $('error').hidden = false;
}

function hideError() {
  $('error').hidden = true;
}

function setStep(name, count) {
  for (const item of document.querySelectorAll('.steps li')) {
    const order = Object.keys(STEPS);
    const mine = order.indexOf(item.dataset.step);
    const now = order.indexOf(name);
    item.dataset.state = mine < now ? 'done' : mine === now ? 'now' : 'next';
    if (item.dataset.step === name && count !== undefined) item.querySelector('.step-count').textContent = count;
  }
  if (name !== step) {
    step = name;
    announce(`${STEPS[name]}…`);
  }
}

function heard(event) {
  window.__scene.events.push({ ...event, at: Date.now() });
  clearInterval(countdown);
  $('wait').hidden = true;
  if (event.type === 'progress') {
    if (event.step === 'building') {
      setStep('building', '');
    } else {
      const from = event.server ? ` from ${event.server}` : '';
      setStep(event.step, `${event.done} of ${event.total}${from}`);
    }
  } else if (event.type === 'slow') {
    $('wait').textContent = `Still waiting for the ${KINDS[event.kind]} after ${event.seconds}s. A large area can take a minute.`;
    $('wait').hidden = false;
  } else if (event.type === 'wait') {
    setStep(event.kind);
    const server = event.why === 'busy' ? 'is busy' : "can't be reached";
    const tick = () => {
      const seconds = Math.max(0, Math.ceil((event.until - Date.now()) / 1000));
      $('wait').textContent = `The ${KINDS[event.kind]} server ${server}. Trying again in ${seconds}s (attempt ${event.attempt} of ${event.attempts}).`;
    };
    tick();
    $('wait').hidden = false;
    announce(`The ${KINDS[event.kind]} server ${server}. Trying again soon.`);
    countdown = setInterval(tick, 250);
  }
}

function resetSteps() {
  step = undefined;
  for (const item of document.querySelectorAll('.steps li')) {
    item.dataset.state = 'next';
    item.querySelector('.step-count').textContent = '';
  }
}

// --- The controls

const controls = new Map();
const servers = serverBoxes();

function makeControls() {
  for (const name of FIRST) $('first').append(add(OPTIONS.get(name)));
  for (const group of GROUPS) {
    const shown = [...OPTIONS.values()].filter((option) => option.group === group && place(option) === 'group');
    if (shown.length === 0) continue;
    const details = document.createElement('details');
    details.className = 'group';
    details.dataset.group = group;
    const summary = document.createElement('summary');
    summary.textContent = group;
    const body = Object.assign(document.createElement('div'), { className: 'group-body' });
    details.append(summary, body);
    const note = NOTES[group];
    if (note) body.append(Object.assign(document.createElement('p'), { className: 'group-note', textContent: note }));
    for (const option of shown) body.append(add(option));
    if (group === 'Data') body.append(servers.set);
    $('groups').append(details);
  }
  const palette = controls.get('palette');
  palette.element.append($('palette-file'));
  controls.get('theme').element.append($('theme-remove'));
}

const NOTES = {
  Camera: 'Turn and zoom the diorama, or type the numbers here.',
  Data: 'Some of these build the scene again from the GPX file, as each one says. What this browser keeps is used again, so only new data is downloaded. A sample walk downloads its data the first time.',
};

function add(option) {
  const control = new Control(option);
  controls.set(option.name, control);
  if (route(option.name) === 'rebuild') {
    control.element.querySelector('.hint').append(' Changing it builds the scene again.');
  }
  control.element.addEventListener('change', (event) => {
    if (event.target.type === 'file') return;
    queue(() => changed(control));
  });
  return control.element;
}

function valueOf(name) {
  return controls.get(name)?.value;
}

function valuesOf(names) {
  return Object.fromEntries(names.map((name) => [name, valueOf(name)]).filter(([, value]) => value !== undefined));
}

// One change at a time, in order
function queue(job) {
  work = work.then(job).catch((error) => console.error(error));
  return work;
}

async function changed(control) {
  const { name } = control;
  control.say();
  const value = control.value;
  const typed = ['number', 'text', 'time', 'colour'].includes(control.element.dataset.kind);
  if (typed && value !== undefined) {
    try {
      await scenes.check('options', { [name]: value });
    } catch (error) {
      // Under its own control, the option needn't be named
      control.say(error.message.replace(`${name} in the options: `, ''));
      return;
    }
  }
  // sun and time can't both be set: the one set last wins
  const other = { sun: 'time', time: 'sun' }[name];
  if (other && value !== undefined && valueOf(other) !== undefined) {
    controls.get(other).value = undefined;
    setLive(other);
  }
  await apply(name);
  save();
}

async function apply(name) {
  switch (route(name)) {
    case 'camera':
      setCamera();
      break;
    case 'adjust':
    case 'rebuild':
      await refresh();
      break;
    default:
      setLive(name);
  }
}

// An attribute of <isotrail-hike>, removed at the default
function setLive(name) {
  const value = valueOf(name);
  if (name === 'aspect') {
    const shape = value ?? controls.get('aspect').default;
    hike.setAttribute('aspect', shape);
    $('frame').style.setProperty('--shape', SHAPES[shape] ?? SHAPES.wide);
    return;
  }
  const text = name === 'palette' && value === PALETTE_FILE ? paletteFile?.text : value;
  if (text === undefined) hike.removeAttribute(name);
  else hike.setAttribute(name, String(text));
}

// The camera fields set the starting view, and the view starts again there
function setCamera() {
  for (const name of CAMERA) setLive(name);
  hike.reset();
}

hike.addEventListener('isotrail-view', ({ detail }) => {
  for (const name of CAMERA) {
    const control = controls.get(name);
    control.value = detail[name];
    control.say();
  }
});

// --- Building and changing the scene

function skipped() {
  return [...(window.__skip ?? []), ...unticked()];
}

function buildKey() {
  return JSON.stringify(valuesOf(REBUILD));
}

function buildOptions() {
  const options = { ...(window.__options ?? {}), ...valuesOf(REBUILD) };
  if (valueOf('no-weather')) options['no-weather'] = true;
  return options;
}

// The GPX file the scene is made from: the visitor's, or a sample's
async function gpxOf(scene) {
  if (scene.file) return scene.file;
  const { gpx } = scene.meta;
  const response = await fetch(gpx);
  if (!response.ok) throw Object.assign(new Error(`can't fetch ${gpx}: HTTP ${response.status}`), { reason: 'other' });
  scene.file = new File([await response.blob()], gpx.split('/').pop(), { type: 'application/gpx+xml' });
  return scene.file;
}

// What the shown scene's controls ask for, built or changed as they need
async function refresh() {
  if (!current) return;
  const key = buildKey();
  const weatherBack = current.built.noWeather && !valueOf('no-weather');
  if (current.sample && key === '{}' && current.built.key !== '{}') {
    current = sampleScene(current.meta);
  } else if (key !== current.built.key || weatherBack) {
    await buildScene(await gpxOf(current), { ...current.meta, fresh: false });
    return;
  }
  await showAdjusted(false);
}

// The scene with the options that change a finished scene applied
async function showAdjusted(fresh) {
  const ticket = ++adjusting;
  const options = valuesOf(ADJUST);
  let bytes = current.original;
  try {
    if (Object.keys(options).length) {
      current.original ??= new Uint8Array(await (await fetch(current.meta.src)).arrayBuffer());
      bytes = await scenes.adjust(current.original, options);
    }
  } catch (error) {
    showError(error.reason ?? 'other', sentence(error.message));
    return;
  }
  if (ticket !== adjusting) return;
  hideError();
  swap(bytes, fresh);
}

function same(a, b) {
  return a === b || (a && b && a.length === b.length && a.every((byte, at) => byte === b[at]));
}

// Shows these bytes, or the sample file as it ships when there are none
function swap(bytes, fresh) {
  if (!fresh && current.src && same(bytes, current.bytes)) return;
  const old = shownUrl;
  shownUrl = bytes ? URL.createObjectURL(new Blob([bytes], { type: 'application/octet-stream' })) : undefined;
  current.bytes = bytes;
  current.src = shownUrl ?? current.meta.src;
  window.__scene.shown = bytes;
  // A change of option shows the finished hike; a new hike plays
  if (fresh) hike.setAttribute('autoplay', '');
  else hike.removeAttribute('autoplay');
  if (hike.getAttribute('src') !== current.src) hike.setAttribute('src', current.src);
  $('download').href = current.src;
  if (old) URL.revokeObjectURL(old);
}

// Shows a scene's name, notes and files below the element
function show({ name, download, gpx, notes = [] }) {
  $('empty').hidden = true;
  hike.hidden = false;
  $('shown').textContent = name;
  const list = $('notes');
  list.replaceChildren(...notes.map((note) => Object.assign(document.createElement('li'), { textContent: note })));
  list.hidden = notes.length === 0;
  $('download').download = download;
  $('gpx').hidden = !gpx;
  if (gpx) {
    $('gpx').href = gpx;
    $('gpx').download = gpx.split('/').pop();
  }
  $('after').hidden = false;
  $('controls').hidden = false;
  $('workspace').classList.add('shown');
  // On a small screen or with large text, the choices can push it out of view
  const { top, bottom } = $('frame').getBoundingClientRect();
  if (Math.min(bottom, innerHeight) - Math.max(top, 0) < (bottom - top) / 2) {
    const still = matchMedia('(prefers-reduced-motion: reduce)').matches;
    $('frame').scrollIntoView({ block: 'nearest', behavior: still ? 'auto' : 'smooth' });
  }
}

// The downloads stay in the browser's cache, so a fresh worker loses none
function recycle() {
  scenes.terminate();
  scenes = new SceneWorker();
  window.__scenes = scenes;
  window.__scene.recycled = true;
}

async function buildScene(file, meta) {
  build?.abort();
  const controller = new AbortController();
  build = controller;
  last = { file, meta };
  window.__scene = { state: 'building', events: [] };
  hideError();
  resetSteps();
  $('build-name').textContent = meta.name;
  $('build').hidden = false;
  $('cancel').disabled = false;
  announce(`Building ${meta.name}…`);
  const key = buildKey();
  const options = buildOptions();
  try {
    const built = await scenes.build(file, {
      name: file.name,
      options,
      servers: window.__servers,
      skip: skipped(),
      onProgress: heard,
      signal: controller.signal,
    });
    if (build !== controller) return;
    const notes = built.notes.map((note) => sentence(note, file.name));
    window.__scene = { ...window.__scene, ...built, state: 'built' };
    if (built.memory > (window.__recycle ?? RECYCLE_BYTES)) recycle();
    $('build').hidden = true;
    announce(`Built ${meta.name}.`);
    current = {
      meta,
      file,
      sample: meta.sample,
      original: built.scene,
      built: { key, noWeather: Boolean(options['no-weather']) },
    };
    show({ ...meta, notes });
    await showAdjusted(meta.fresh);
  } catch (error) {
    if (build !== controller) return;
    const state = error.reason === 'cancelled' ? 'cancelled' : 'failed';
    window.__scene = { ...window.__scene, state, reason: error.reason, error: error.message };
    $('build').hidden = true;
    if (state === 'cancelled') {
      announce('Cancelled.');
    } else {
      showError(error.reason, sentence(error.message, file.name));
      announce(`${LEADS[error.reason] ?? LEADS.other} ${sentence(error.message, file.name)}`);
    }
  } finally {
    clearInterval(countdown);
    if (build === controller) $('cancel').disabled = true;
    showCache();
  }
}

function buildFile(file) {
  const name = file.name;
  return buildScene(file, { name, download: `${name.replace(/\.gpx$/i, '')}.itb`, fresh: true });
}

function sampleScene(meta) {
  return { meta, sample: meta.sample, file: undefined, original: undefined, built: { key: '{}', noWeather: false } };
}

async function showSample(button) {
  build?.abort();
  build = undefined;
  last = undefined;
  hideError();
  $('build').hidden = true;
  const slug = button.dataset.sample;
  window.__scene = { state: 'sample', sample: slug, events: [] };
  announce(`Showing the sample walk ${button.dataset.name}.`);
  const meta = {
    sample: slug,
    name: `Sample walk: ${button.dataset.name}`,
    download: `${slug}.itb`,
    gpx: `/samples/${slug}.gpx`,
    src: `/samples/${slug}.itb`,
    fresh: true,
  };
  if (buildKey() !== '{}') {
    current = sampleScene(meta);
    await buildScene(await gpxOf(current), meta);
    return;
  }
  current = sampleScene(meta);
  show(meta);
  await showAdjusted(true);
}

// A page refuses what it can tell from the drop itself, the way the worker
// refuses a file that isn't a GPX file
function refuse(text) {
  build?.abort();
  last = undefined;
  window.__scene = { state: 'failed', reason: 'input', error: text, events: [] };
  $('build').hidden = true;
  showError('input', sentence(text));
  announce(`${LEADS.input} ${sentence(text)}`);
}

function dropped(transfer) {
  const items = [...(transfer.items ?? [])].filter((item) => item.kind === 'file');
  const files = [...transfer.files];
  if (files.length === 0) return;
  if (files.length > 1) {
    refuse('drop one GPX file at a time');
    return;
  }
  if (items[0]?.webkitGetAsEntry?.()?.isDirectory) {
    refuse(`${files[0].name} is a folder, not a GPX file`);
    return;
  }
  build?.abort();
  queue(() => buildFile(files[0]));
}

$('file').addEventListener('change', () => {
  const file = $('file').files[0];
  $('file').value = '';
  if (!file) return;
  build?.abort();
  queue(() => buildFile(file));
});

// Dropping a file anywhere else would open it in place of the page
for (const type of ['dragover', 'drop']) window.addEventListener(type, (event) => event.preventDefault());
for (const zone of [$('drop'), $('frame')]) {
  zone.addEventListener('dragover', (event) => {
    event.preventDefault();
    event.dataTransfer.dropEffect = 'copy';
    $('drop').classList.add('over');
  });
  zone.addEventListener('dragleave', () => $('drop').classList.remove('over'));
  zone.addEventListener('drop', (event) => {
    event.preventDefault();
    $('drop').classList.remove('over');
    dropped(event.dataTransfer);
  });
}

for (const button of document.querySelectorAll('[data-sample]')) {
  button.addEventListener('click', () => {
    build?.abort();
    queue(() => showSample(button));
  });
}

$('cancel').addEventListener('click', () => build?.abort());
$('retry').addEventListener('click', () => last && queue(() => buildScene(last.file, last.meta)));
$('no-map-data').addEventListener('click', () => {
  if (!last) return;
  controls.get('no-map-data').value = true;
  save();
  queue(() => buildScene(last.file, last.meta));
});

// --- Files: a palette from Lospec and a theme

$('palette-choose').addEventListener('click', () => $('palette-input').click());
$('palette-input').addEventListener('change', () => {
  const file = $('palette-input').files[0];
  $('palette-input').value = '';
  if (file) queue(() => choosePalette(file));
});

async function choosePalette(file) {
  const control = controls.get('palette');
  control.say();
  const text = await file.text();
  try {
    await scenes.check('palette', text, file.name);
  } catch (error) {
    control.say(error.message);
    return;
  }
  paletteFile = { name: file.name, text };
  const list = control.input;
  let option = list.querySelector(`option[value="${PALETTE_FILE}"]`);
  if (!option) {
    option = Object.assign(document.createElement('option'), { value: PALETTE_FILE });
    list.append(option);
  }
  option.textContent = file.name;
  list.value = PALETTE_FILE;
  setLive('palette');
}


async function chooseTheme(file) {
  const control = controls.get('theme');
  control.say();
  const text = await file.text();
  try {
    await scenes.check('theme', text, file.name);
  } catch (error) {
    control.input.value = '';
    control.say(error.message);
    return;
  }
  setTheme(URL.createObjectURL(new Blob([text], { type: 'text/plain' })));
}

// The element keeps each address it read, so every file gets a fresh one
function setTheme(url) {
  const old = themeUrl;
  themeUrl = url;
  if (url) hike.setAttribute('theme', url);
  else hike.removeAttribute('theme');
  $('theme-remove').hidden = !url;
  if (old) URL.revokeObjectURL(old);
}

$('theme-remove').addEventListener('click', () => {
  controls.get('theme').value = undefined;
  setTheme(undefined);
});

// --- Map data servers

for (const { box } of servers.boxes) {
  box.addEventListener('change', () => {
    save();
    queue(checkServers);
  });
}

// None left that can serve this slab is said at once, before any build
async function checkServers() {
  servers.problem.hidden = true;
  if (!current) return;
  try {
    await scenes.checkServers(await gpxOf(current), { options: buildOptions(), skip: skipped() });
  } catch (error) {
    servers.problem.textContent = sentence(error.message);
    servers.problem.hidden = false;
  }
}

// --- Back to the defaults

$('reset-all').addEventListener('click', () => queue(resetAll));

async function resetAll() {
  for (const control of controls.values()) {
    control.value = undefined;
    control.say();
  }
  paletteFile = undefined;
  controls.get('palette').input.querySelector(`option[value="${PALETTE_FILE}"]`)?.remove();
  setTheme(undefined);
  for (const { box } of servers.boxes) box.checked = true;
  servers.problem.hidden = true;
  for (const [name] of controls) if (['live', 'camera'].includes(route(name))) setLive(name);
  hike.reset();
  save();
  $('kept').hidden = true;
  announce('Every option is back to its default.');
  await refresh();
}

// --- Settings kept between visits

const settings = new Settings();

function unticked() {
  return servers.boxes.filter(({ box }) => !box.checked).map(({ box }) => box.value);
}

function save() {
  const options = {};
  for (const [name, control] of controls) {
    const value = control.value;
    if (value === undefined || NOT_KEPT.includes(name) || (name === 'palette' && value === PALETTE_FILE)) continue;
    options[name] = value;
  }
  settings.save(options, unticked());
}

// The kept settings back on the controls. A value the controls or the
// tool's checks no longer take is dropped
async function restore() {
  $('remember-set').hidden = !settings.store;
  const { options, skip } = settings.load();
  $('remember').checked = settings.remember;
  let back = [];
  for (const [name, value] of Object.entries(options)) {
    const control = controls.get(name);
    if (!control || NOT_KEPT.includes(name)) continue;
    control.value = value;
    if (control.value === value) back.push(name);
    else control.value = undefined;
  }
  if (back.length) {
    try {
      await scenes.check('options', valuesOf(back));
    } catch {
      const taken = [];
      for (const name of back) {
        try {
          await scenes.check('options', { [name]: valueOf(name) });
          taken.push(name);
        } catch (error) {
          // A worker that failed to start says nothing about the value
          if (error.reason === 'usage') controls.get(name).value = undefined;
          else taken.push(name);
        }
      }
      back = taken;
    }
  }
  for (const { box } of servers.boxes) box.checked = !skip.includes(box.value);
  for (const name of back) if (route(name) === 'live') setLive(name);
  save();
  const names = [...controls.keys()].filter((name) => back.includes(name)).map(label);
  if (unticked().length) names.push('Map data servers');
  if (names.length) {
    $('kept-text').textContent = `This browser kept your settings from last time: ${list(names)}.`;
    $('kept').hidden = false;
  }
}

function list(names) {
  return names.length < 2 ? names.join('') : `${names.slice(0, -1).join(', ')} and ${names.at(-1)}`;
}

$('remember').addEventListener('change', () => {
  settings.remember = $('remember').checked;
  save();
  announce(settings.remember ? 'Your settings are kept in this browser.' : 'Your settings are no longer kept in this browser.');
});
$('kept-reset').addEventListener('click', () => queue(resetAll));

// --- The cache

async function showCache() {
  const { bytes, entries } = await scenes.cacheSize();
  window.__scene.cache = { bytes, entries };
  $('cache').textContent = entries
    ? `This browser keeps ${(bytes / 1e6).toFixed(1)}MB of what the services answered, so a second build of the same hike downloads nothing.`
    : 'This browser keeps what the services answer, so a second build of the same hike downloads nothing.';
  $('clear').hidden = entries === 0;
}

$('clear').addEventListener('click', async () => {
  await scenes.clearCache();
  showCache();
});

makeControls();
controls.get('theme').input.addEventListener('change', () => {
  const file = controls.get('theme').input.files[0];
  if (file) queue(() => chooseTheme(file));
});
for (const name of FIRST) if (route(name) === 'live') setLive(name);
queue(restore);

window.__controls = {
  shown: [...document.querySelectorAll('[data-option]')].map((element) => element.dataset.option),
  leftOut: Object.keys(LEFT_OUT),
  later: [...OPTIONS.values()].filter((option) => place(option) === 'later').map((option) => option.name),
  // Sets a control as a visitor would, and settles once the change shows
  set(name, value) {
    const control = controls.get(name);
    control.value = value;
    control.element.querySelector('input, select').dispatchEvent(new Event('change', { bubbles: true }));
    return this.settled();
  },
  value: valueOf,
  // What this browser keeps, as stored
  kept: () => settings.kept(),
  // The message shown under a control, or under the servers
  problem(name) {
    const shown = name === 'servers' ? servers.problem : controls.get(name).problem;
    return shown.hidden ? '' : shown.textContent;
  },
  reset() {
    $('reset-all').click();
    return this.settled();
  },
  // Waits for every change asked so far
  async settled() {
    let seen;
    do {
      seen = work;
      await work;
    } while (seen !== work);
  },
};
showCache();
