// Builds a scene from the chosen GPX file in the scene worker and shows it
// in the <isotrail-hike> element
//
// The browser check reads and drives the page through `window.__scenes` and
// `window.__scene`. It may set `window.__servers` to its own Overpass servers
// and `window.__skip` to the names of built-in ones to leave out

const runtime = document.body.dataset.runtime;
const { SceneWorker } = await import(/* @vite-ignore */ `${runtime}scene/scene.js`);

const scenes = new SceneWorker();
const $ = (id) => document.getElementById(id);
const NAMES = { elevation: 'elevation tiles', 'map-data': 'map data', weather: 'weather' };
let last;
let build;
let countdown;

window.__scenes = scenes;
window.__scene = { state: 'idle', events: [] };

function show(text) {
  $('status').textContent = text;
}

function heard(event) {
  window.__scene.events.push({ ...event, at: Date.now() });
  clearInterval(countdown);
  if (event.type === 'progress') {
    const from = event.server ? ` from ${event.server}` : '';
    show(event.step === 'building' ? 'Building the scene…' : `Fetching ${NAMES[event.step]}${from}: ${event.done} of ${event.total}`);
  } else if (event.type === 'slow') {
    show(`Still waiting for ${NAMES[event.kind]} after ${event.seconds}s…`);
  } else if (event.type === 'wait') {
    const tick = () => {
      const seconds = Math.max(0, Math.ceil((event.until - Date.now()) / 1000));
      const server = event.why === 'busy' ? 'is busy' : "can't be reached";
      show(`The ${NAMES[event.kind]} server ${server}. Trying again in ${seconds}s (attempt ${event.attempt} of ${event.attempts}).`);
    };
    tick();
    countdown = setInterval(tick, 250);
  }
}

async function showCache() {
  const { bytes, entries } = await scenes.cacheSize();
  $('cache').textContent = `Cache: ${(bytes / 1e6).toFixed(1)}MB in ${entries} answers.`;
  window.__scene.cache = { bytes, entries };
}

$('file').addEventListener('change', async () => {
  const file = $('file').files[0];
  if (!file) return;
  build?.abort();
  build = new AbortController();
  const signal = build.signal;
  window.__scene = { state: 'building', events: [] };
  show(`Building ${file.name}…`);
  $('download').hidden = true;
  $('cancel').disabled = false;
  try {
    const { scene, warnings, passes, ms, fetched, cached, unsaved, memory } = await scenes.build(file, {
      options: $('options').value,
      servers: window.__servers,
      skip: window.__skip,
      onProgress: heard,
      signal,
    });
    clearInterval(countdown);
    window.__scene = { ...window.__scene, state: 'built', scene, warnings, passes, ms, fetched, cached, unsaved, memory };
    const left = warnings.length ? ` Left out: ${warnings.join(', ')}.` : '';
    const kept = unsaved ? ` ${unsaved} answers couldn't be kept.` : '';
    show(`Built in ${(ms / 1000).toFixed(1)}s, ${scene.length} bytes, ${fetched} downloads, ${cached} from the cache.${kept}${left}`);
    if (last) URL.revokeObjectURL(last);
    last = URL.createObjectURL(new Blob([scene], { type: 'application/octet-stream' }));
    $('download').href = last;
    $('download').download = file.name.replace(/\.gpx$/i, '') + '.itb';
    $('download').hidden = false;
    $('hike').setAttribute('src', last);
  } catch (error) {
    clearInterval(countdown);
    if (signal !== build.signal) return;
    const state = error.reason === 'cancelled' ? 'cancelled' : 'failed';
    window.__scene = { ...window.__scene, state, reason: error.reason, error: error.message };
    show(state === 'cancelled' ? 'Cancelled.' : `Can't build the scene (${error.reason}): ${error.message}`);
  } finally {
    if (signal === build.signal) $('cancel').disabled = true;
    $('file').value = '';
    showCache();
  }
});

$('cancel').addEventListener('click', () => build?.abort());
$('clear').addEventListener('click', async () => {
  await scenes.clearCache();
  showCache();
});
showCache();
