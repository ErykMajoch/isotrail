// The first screen: a GPX file dropped or chosen is built into a scene in the
// scene worker, and the scene or a sample walk shows in the <isotrail-hike>
// element
//
// The browser checks read and drive the page through `window.__scenes` and
// `window.__scene`. They may set `window.__options` to options added to every
// build, `window.__servers` to their own Overpass servers and `window.__skip`
// to the names of built-in ones to leave out

const runtime = document.body.dataset.runtime;
const { SceneWorker } = await import(/* @vite-ignore */ `${runtime}scene/scene.js`);

const scenes = new SceneWorker();
const $ = (id) => document.getElementById(id);

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

let build;
let last;
let scene;
let countdown;
let step;

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

// Shows a scene in the element, with its files and notes below
function show({ src, name, download, gpx, notes = [] }) {
  $('empty').hidden = true;
  $('hike').hidden = false;
  if ($('hike').getAttribute('src') !== src) $('hike').setAttribute('src', src);
  $('shown').textContent = name;
  const list = $('notes');
  list.replaceChildren(...notes.map((note) => Object.assign(document.createElement('li'), { textContent: note })));
  list.hidden = notes.length === 0;
  $('download').href = src;
  $('download').download = download;
  $('gpx').hidden = !gpx;
  if (gpx) {
    $('gpx').href = gpx;
    $('gpx').download = gpx.split('/').pop();
  }
  $('after').hidden = false;
}

async function buildFile(file, extra = {}) {
  build?.abort();
  const controller = new AbortController();
  build = controller;
  last = { file, extra };
  window.__scene = { state: 'building', events: [] };
  hideError();
  resetSteps();
  $('build-name').textContent = file.name;
  $('build').hidden = false;
  $('cancel').disabled = false;
  announce(`Building ${file.name}…`);
  const options = { ...(window.__options ?? {}), ...extra };
  try {
    const built = await scenes.build(file, {
      options,
      servers: window.__servers,
      skip: window.__skip,
      onProgress: heard,
      signal: controller.signal,
    });
    if (build !== controller) return;
    const notes = built.notes.map((note) => sentence(note, file.name));
    window.__scene = { ...window.__scene, ...built, state: 'built' };
    if (scene) URL.revokeObjectURL(scene);
    scene = URL.createObjectURL(new Blob([built.scene], { type: 'application/octet-stream' }));
    $('build').hidden = true;
    announce(`Built ${file.name}.`);
    show({ src: scene, name: file.name, download: `${file.name.replace(/\.gpx$/i, '')}.itb`, notes });
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
  buildFile(files[0]);
}

$('file').addEventListener('change', () => {
  const file = $('file').files[0];
  $('file').value = '';
  if (file) buildFile(file);
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
    build = undefined;
    last = undefined;
    hideError();
    $('build').hidden = true;
    const slug = button.dataset.sample;
    window.__scene = { state: 'sample', sample: slug, events: [] };
    announce(`Showing the sample walk ${button.dataset.name}.`);
    show({
      src: `/samples/${slug}.itb`,
      name: `Sample walk: ${button.dataset.name}`,
      download: `${slug}.itb`,
      gpx: `/samples/${slug}.gpx`,
    });
  });
}

$('cancel').addEventListener('click', () => build?.abort());
$('retry').addEventListener('click', () => last && buildFile(last.file, last.extra));
$('no-map-data').addEventListener('click', () => last && buildFile(last.file, { ...last.extra, 'no-map-data': true }));

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
showCache();
