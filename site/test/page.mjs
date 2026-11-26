// Headless browser check of the web app: a GPX file dropped or chosen, the
// sample walks, the keyboard, reduced motion, every failure the page can
// show, the scene file it hands back, the controls, the settings it keeps,
// phone and desktop sizes with an accessibility scan, touch, and memory
//
//   node site/test/page.mjs [chrome|edge] [--webgl] [--site DIR] [--shots DIR]
//
// Needs a built site (sh site/build.sh), served with the headers in its
// _headers file. The elevation, map and weather services are played from the
// recordings in fixtures/ by a local server. --shots saves a screenshot of
// each state of the page. Exits with 1 if any check fails

import { createHash } from 'node:crypto';
import { existsSync } from 'node:fs';
import { mkdir, mkdtemp, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { connect, launch, quit, sleepFor } from '../../web/test/browser.mjs';
import { SERVICE, closedPort, serve } from '../../web/test/services.mjs';

const ROOT = fileURLToPath(new URL('../..', import.meta.url));
const GPX = join(ROOT, 'fixtures', 'gpx', 'snowdon.gpx');
const args = process.argv.slice(2);
const option = (name) => {
  const at = args.indexOf(name);
  return at >= 0 ? args[at + 1] : undefined;
};
const WEBGL = args.includes('--webgl');
const SITE = resolve(option('--site') ?? join(ROOT, 'site', 'dist'));
const SHOTS = option('--shots') && resolve(option('--shots'));
const BROWSER = args.find((arg, i) => !arg.startsWith('--') && !['--site', '--shots'].includes(args[i - 1])) ?? 'chrome';
const TIMEOUT_MS = 60000;
const VIEWPORT = { width: 1100, height: 1200, deviceScaleFactor: 1, mobile: false };
const PHONE = { width: 390, height: 844, deviceScaleFactor: 1, mobile: true };
const DESKTOP = { width: 1280, height: 800, deviceScaleFactor: 1, mobile: false };
const AXE = await readFile(join(ROOT, 'site', 'node_modules', 'axe-core', 'axe.min.js'), 'utf8');

if (!existsSync(join(SITE, 'index.html'))) {
  console.error(`${SITE} holds no built site: build it first with sh site/build.sh`);
  process.exit(1);
}
const samples = JSON.parse(await readFile(join(ROOT, 'site', 'src', 'samples.json'), 'utf8'));

const services = await serve({ site: SITE });
const { origin: ORIGIN, servers: SERVERS, state } = services;
const OPTIONS = { detail: 32, ...SERVERS };
const CLOSED = await closedPort();

const temp = await mkdtemp(join(tmpdir(), 'isotrail-page-'));
const downloads = join(temp, 'downloads');
await mkdir(downloads);
if (SHOTS) await mkdir(SHOTS, { recursive: true });

// Files to drop: the fixture, the fixture without times, a picture, a route
// on one spot, a route too long for map data, and a folder
const gpx = await readFile(GPX, 'utf8');
const files = {
  walk: join(temp, 'walk.gpx'),
  untimed: join(temp, 'untimed.gpx'),
  photo: join(temp, 'photo.png'),
  spot: join(temp, 'spot.gpx'),
  long: join(temp, 'long.gpx'),
  folder: join(temp, 'Hikes'),
};
await writeFile(files.walk, gpx);
await writeFile(files.untimed, gpx.split('\n').filter((line) => !line.includes('<time>')).join('\n'));
await writeFile(files.photo, Buffer.from('89504e470d0a1a0a0000000d49484452', 'hex'));
await writeFile(files.spot, '<gpx version="1.1"><trk><trkseg><trkpt lat="52" lon="-1.5"></trkpt><trkpt lat="52" lon="-1.5"></trkpt></trkseg></trk></gpx>');
const points = Array.from({ length: 11 }, (_, i) => `<trkpt lat="${(52 + (150 / 111.2) * i / 10).toFixed(6)}" lon="-1.5"><ele>200</ele></trkpt>`).join('');
await writeFile(files.long, `<?xml version="1.0"?><gpx version="1.1" creator="test"><trk><name>Long walk</name><trkseg>${points}</trkseg></trk></gpx>`);
await mkdir(files.folder);

const failures = [];
let passed = 0;
function check(name, ok, detail = '') {
  if (ok) passed++;
  else failures.push(`${name}${detail ? `: ${detail}` : ''}`);
  console.log(`${ok ? 'ok  ' : 'FAIL'} ${name}${!ok && detail ? ` (${detail})` : ''}`);
}

const browser = await launch(BROWSER, { webgl: WEBGL, viewport: VIEWPORT }).catch(async (error) => {
  console.error(error.message);
  services.close();
  await rm(temp, { recursive: true, force: true }).catch(() => {});
  process.exit(1);
});
const cdp = await connect(browser.port, TIMEOUT_MS);

// Requests from the page and from its workers, which attach as they start
const asked = [];
const sessions = new Set();
let offline = false;
const conditions = () => ({ offline, latency: 0, downloadThroughput: -1, uploadThroughput: -1 });
cdp.on('Network.requestWillBeSent', ({ request }) => asked.push(request.url));
cdp.on('Target.attachedToTarget', async ({ sessionId }) => {
  sessions.add(sessionId);
  await cdp.send('Network.enable', {}, sessionId).catch(() => {});
  await cdp.send('Network.emulateNetworkConditions', conditions(), sessionId).catch(() => {});
  await cdp.send('Runtime.runIfWaitingForDebugger', {}, sessionId).catch(() => {});
});
cdp.on('Target.detachedFromTarget', ({ sessionId }) => sessions.delete(sessionId));
let chooser;
cdp.on('Page.fileChooserOpened', (event) => (chooser = event));
await cdp.send('Runtime.enable');
await cdp.send('Page.enable');
await cdp.send('DOM.enable');
await cdp.send('Network.enable');
await cdp.send('Target.setAutoAttach', { autoAttach: true, waitForDebuggerOnStart: true, flatten: true });
await cdp.send('Emulation.setDeviceMetricsOverride', VIEWPORT);
await cdp.send('Page.setDownloadBehavior', { behavior: 'allow', downloadPath: downloads });

async function setOffline(value) {
  offline = value;
  for (const sessionId of [undefined, ...sessions]) {
    await cdp.send('Network.emulateNetworkConditions', conditions(), sessionId).catch(() => {});
  }
}

async function reducedMotion(on) {
  await cdp.send('Emulation.setEmulatedMedia', { features: [{ name: 'prefers-reduced-motion', value: on ? 'reduce' : 'no-preference' }] });
}

// The page, fresh, with every build pointed at the local services. Settings
// kept from an earlier visit are cleared unless `keep` is set
async function open(options = OPTIONS, { keep = false } = {}) {
  if (!keep) await cdp.send('Storage.clearDataForOrigin', { origin: ORIGIN, storageTypes: 'local_storage' });
  await cdp.send('Page.navigate', { url: `${ORIGIN}/` });
  await cdp.waitFor("document.readyState === 'complete' && window.__scene?.state === 'idle' && !!window.__scenes");
  // A reload keeps the scroll, which could leave the diorama off screen and not loading
  await cdp.evaluate('window.__controls.settled().then(() => window.scrollTo(0, 0))');
  await cdp.evaluate(`window.__options = ${JSON.stringify(options)}; window.__events = [];
    for (const type of ['isotrail-play', 'isotrail-ready']) document.getElementById('hike').addEventListener(type, () => window.__events.push(type))`);
}

async function shot(name) {
  if (SHOTS) await writeFile(join(SHOTS, `${name}.png`), Buffer.from(await cdp.screenshot(), 'base64'));
}

// What the page shows: its error, notes and what it shows in the frame
const shown = () => cdp.evaluate(`(() => {
  const $ = (id) => document.getElementById(id);
  return {
    error: $('error').hidden ? null : { lead: $('error-lead').textContent, text: $('error-text').textContent },
    retry: !$('retry').hidden,
    noMapData: !$('no-map-data').hidden,
    notes: [...document.querySelectorAll('#notes li')].map((li) => li.textContent),
    building: !$('build').hidden,
    hike: $('hike').hidden ? null : $('hike').dataset.state,
    empty: !$('empty').hidden,
  };
})()`);

// Drops these files on the drop zone as a person would
async function drop(paths) {
  const box = await cdp.evaluate(`(() => {
    document.getElementById('drop').scrollIntoView({ block: 'center' });
    const { x, y, width, height } = document.getElementById('drop').getBoundingClientRect();
    return { x: x + width / 2, y: y + height / 2 };
  })()`);
  await cdp.evaluate("window.__scene.state = 'dropping'");
  const data = { items: [], files: paths, dragOperationsMask: 1 };
  for (const type of ['dragEnter', 'dragOver', 'drop']) {
    await cdp.send('Input.dispatchDragEvent', { type, x: box.x, y: box.y, data });
  }
}

const settled = () => cdp.waitFor("['built', 'failed', 'cancelled'].includes(window.__scene.state)");
const ready = () => cdp.waitFor("document.getElementById('hike').dataset.state === 'ready'");
const scene = async () => Buffer.from(await cdp.evaluate(`(() => {
  const bytes = window.__scene.scene ?? new Uint8Array();
  let text = '';
  for (let i = 0; i < bytes.length; i += 0x8000) text += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(text);
})()`), 'base64');
const services_asked = () => state.seen.filter(({ url }) => SERVICE.test(new URL(url, ORIGIN).pathname));
const hash = async () => createHash('sha256').update(await cdp.screenshot()).digest('hex');

// The diorama once it holds still for a second, without the page round it
async function picture() {
  await cdp.evaluate('window.__controls.settled()');
  await ready();
  const clip = await cdp.evaluate(`(() => {
    const { x, y, width, height } = document.getElementById('frame').getBoundingClientRect();
    return { x: x + scrollX, y: y + scrollY, width, height, scale: 1 };
  })()`);
  const framed = async () => createHash('sha256').update(await cdp.screenshot(clip)).digest('hex');
  let seen = await framed();
  let still = 0;
  for (let i = 0; i < 60 && still < 4; i++) {
    await sleepFor(250);
    const now = await framed();
    still = now === seen ? still + 1 : 0;
    seen = now;
  }
  return seen;
}

// Sets a control as a visitor would, and waits for it to show
const set = (name, value) => cdp.evaluate(`window.__controls.set(${JSON.stringify(name)}, ${JSON.stringify(value)})`);
const shownScene = async () => Buffer.from(await cdp.evaluate(`(() => {
  const bytes = window.__scene.shown ?? new Uint8Array();
  let text = '';
  for (let i = 0; i < bytes.length; i += 0x8000) text += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(text);
})()`), 'base64');

// A value for each control that shows on the diorama, with what it needs set
// first to show. Data options that would ask for map data or weather the
// recordings don't hold leave them out
const TRIES = {
  aspect: { value: 'square' },
  palette: { value: 'aap-64' },
  season: { value: 'winter' },
  weather: { value: 'overcast' },
  time: { value: '21:30' },
  mesher: { value: 'smooth' },
  'block-height': { value: 2 },
  occlusion: { value: 0 },
  'soft-shadows': { value: 0 },
  'god-rays': { value: 'on', with: { weather: 'clear' } },
  exaggeration: { value: 12 },
  'prop-density': { value: 0 },
  paths: { value: 'off' },
  streams: { value: 'off' },
  strata: { value: 'off' },
  'base-blocks': { value: 12 },
  outlines: { value: 0 },
  shadows: { value: 'off' },
  'night-sky': { value: 'off', with: { time: '23:30', weather: 'clear' } },
  'weather-effects': { value: 'none', with: { weather: 'rain' } },
  sun: { value: '90,20' },
  'route-colour': { value: '#00ff00' },
  'route-width': { value: 4 },
  'route-hidden': { value: 'off', with: { pitch: 12 } },
  markers: { value: 'none' },
  pitch: { value: 60 },
  turn: { value: 90 },
  zoom: { value: 1.5 },
  detail: { value: 40, with: { 'no-map-data': true } },
  'no-map-data': { value: true },
  padding: { value: '30,800', with: { 'no-map-data': true } },
  footprint: { value: 'north', with: { 'no-map-data': true } },
  'no-weather': { value: true },
  times: { value: 'none' },
  'trim-start': { value: '300m', with: { 'no-map-data': true, 'no-weather': true } },
  'trim-end': { value: '300m', with: { 'no-map-data': true, 'no-weather': true } },
};
const REBUILDS = ['detail', 'no-map-data', 'padding', 'footprint', 'times', 'trim-start', 'trim-end'];

async function key(key, code = key, keyCode = 0, text) {
  await cdp.send('Input.dispatchKeyEvent', { type: 'keyDown', key, code, windowsVirtualKeyCode: keyCode, text });
  await cdp.send('Input.dispatchKeyEvent', { type: 'keyUp', key, code, windowsVirtualKeyCode: keyCode });
}

// Presses Tab until `focused` holds
async function tabTo(focused, presses = 40) {
  for (let i = 0; i < presses; i++) {
    await key('Tab', 'Tab', 9);
    if (await cdp.evaluate(focused)) return true;
  }
  return false;
}

// Unticks a map data server's box as a visitor would
async function untick(server) {
  await cdp.evaluate(`(() => {
    const box = document.querySelector('[data-server="${server}"]');
    box.checked = false;
    box.dispatchEvent(new Event('change'));
  })()`);
  await cdp.evaluate('window.__controls.settled()');
}

// The window's size, and the browser's text size in pixels. A phone has a
// touch screen, so its pointer is coarse
async function size(screen, text) {
  await cdp.send('Emulation.setDeviceMetricsOverride', screen);
  await cdp.send('Emulation.setTouchEmulationEnabled', { enabled: screen.mobile, maxTouchPoints: screen.mobile ? 5 : 1 });
  await cdp.send('Page.setFontSizes', { fontSizes: { standard: text, fixed: Math.round(text * 0.8125) } });
}

const sideways = () => cdp.evaluate('document.documentElement.scrollWidth - document.documentElement.clientWidth');

// The accessibility scan's serious and critical findings, shadow roots
// included. With --shots every finding is printed
async function scan() {
  await cdp.evaluate(`${AXE}\n;true`);
  const found = await cdp.evaluate(`axe.run(document, {
    runOnly: { type: 'tag', values: ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'wcag22aa', 'best-practice'] },
  }).then(({ violations }) => violations.map(({ id, impact, nodes }) => ({ id, impact, nodes: nodes.slice(0, 3).map(({ target }) => target.flat().join(' ')) })))`);
  if (SHOTS) for (const { id, impact, nodes } of found) console.log(`     scan: ${impact} ${id} (${nodes.join(', ')})`);
  return found.filter(({ impact }) => ['serious', 'critical'].includes(impact));
}

const scanned = (found) => [found.length === 0, found.map(({ id, nodes }) => `${id}: ${nodes.join(', ')}`).join(' | ')];

// What has focus, in words, whether it shows an outline, and whether it is
// on screen and not covered
const FOCUSED = `(() => {
  const element = document.activeElement;
  if (!element || element === document.body) return { name: 'body' };
  if (element.id === 'hike') return { name: 'diorama', outline: true, seen: true, inside: true };
  const option = element.closest('[data-option]')?.dataset.option;
  const name = element.dataset.server ? 'server ' + element.dataset.server
    : element.dataset.sample ? 'sample ' + element.dataset.sample
    : option ? 'option ' + option + (element.type === 'checkbox' && element.closest('fieldset') ? ' ' + element.value : '')
    : element.tagName === 'SUMMARY' ? 'group ' + element.textContent
    : element.id || element.tagName.toLowerCase();
  const ring = element.id === 'file' ? document.getElementById('drop') : element;
  const style = getComputedStyle(ring);
  const box = ring.getBoundingClientRect();
  const hit = document.elementFromPoint(box.x + box.width / 2, box.y + box.height / 2);
  return {
    name,
    closed: element.tagName === 'SUMMARY' && !element.parentElement.open,
    // A time field's clock button has the focus inside it, and the browser rings it
    outline: (style.outlineStyle !== 'none' && parseFloat(style.outlineWidth) > 0) || (element.type === 'time' && !element.matches(':focus')),
    seen: !!hit && (ring.contains(hit) || hit.contains(ring) || !!element.labels?.[0]?.contains(hit)),
    inside: box.bottom > 0 && box.top < innerHeight,
  };
})()`;

// Tabs through the whole page with a hike shown, opening each group on the
// way. Every control must be reached, show its focus and not be covered
async function tabWalk(at) {
  await open();
  await cdp.evaluate(`document.querySelector('[data-sample="${samples[0].slug}"]').click()`);
  check(`${at}: the diorama shows for the walk`, await ready(), await cdp.evaluate("document.getElementById('hike').dataset.state"));
  await cdp.evaluate('document.activeElement?.blur(); window.scrollTo(0, 0)');
  const order = [];
  const unseen = [];
  for (let i = 0; i < 400; i++) {
    await key('Tab', 'Tab', 9);
    await sleepFor(30);
    const focused = await cdp.evaluate(FOCUSED);
    if (focused.name === 'body' || (order.length && focused.name === order[0])) break;
    order.push(focused.name);
    if (!focused.outline || !focused.seen || !focused.inside) {
      unseen.push(`${focused.name}${focused.outline ? '' : ' (no outline)'}${focused.seen && focused.inside ? '' : ' (covered)'}`);
    }
    if (focused.closed) await key('Enter', 'Enter', 13, '\r');
  }
  const wanted = await cdp.evaluate(`[...document.querySelectorAll('#controls [data-option]')].filter((control) => control.offsetParent)
    .flatMap((control) => control.dataset.kind === 'choices'
      ? [...control.querySelectorAll('input')].map((box) => 'option ' + control.dataset.option + ' ' + box.value)
      : ['option ' + control.dataset.option])
    .concat([...document.querySelectorAll('[data-server]')].map((box) => 'server ' + box.dataset.server), ['remember', 'reset-all', 'diorama'])`);
  const missed = wanted.filter((name) => !order.includes(name));
  check(`${at}: Tab reaches every control`, missed.length === 0, missed.join(', '));
  check(`${at}: each one shows its focus on screen`, unseen.length === 0, unseen.join(', '));
  return order;
}

// A one-finger swipe from this point, by dx and dy
async function swipe({ x, y }, dx, dy) {
  const steps = 10;
  await cdp.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: [{ x, y }] });
  for (let i = 1; i <= steps; i++) {
    await cdp.send('Input.dispatchTouchEvent', { type: 'touchMove', touchPoints: [{ x: x + (dx * i) / steps, y: y + (dy * i) / steps }] });
    await sleepFor(16);
  }
  await cdp.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });
  await sleepFor(400);
}

async function downloaded(name) {
  for (let i = 0; i < 100; i++) {
    const found = (await readdir(downloads)).includes(name);
    if (found) {
      await sleepFor(200);
      return readFile(join(downloads, name));
    }
    await sleepFor(100);
  }
  return undefined;
}

try {
  // The first view
  await reducedMotion(false);
  await open();
  const first = await shown();
  check('first view: an empty frame and nothing to build', first.empty && first.hike === null && !first.building && !first.error);
  check('first view: nothing is fetched but the page', state.seen.every(({ url }) => !SERVICE.test(new URL(url, ORIGIN).pathname) && !url.includes('/samples/')));
  await shot('1-empty');

  // A dropped GPX file
  state.seen.length = 0;
  await drop([files.walk]);
  await settled();
  const walk = await scene();
  check('drop: the scene is built', (await cdp.evaluate('window.__scene.state')) === 'built' && walk.length > 0);
  check('drop: it shows live', await ready());
  check('drop: the services were asked', services_asked().length > 0);
  check('drop: it plays by itself', await cdp.waitFor("window.__events.includes('isotrail-play')", 5000));
  const built = await shown();
  check('drop: no notes for a complete track', built.notes.length === 0, built.notes.join(' | '));
  await shot('2-built');

  // The scene file it hands back
  await cdp.evaluate("document.getElementById('download').click()");
  const file = await downloaded('walk.itb');
  check('download: named after the file, the scene\'s own bytes', file?.equals(walk), file ? `${file.length} against ${walk.length} bytes` : 'nothing downloaded');

  // A track without times, with notes
  await drop([files.untimed]);
  await settled();
  await ready();
  const untimed = await shown();
  check('notes: a track without times says so', untimed.notes.includes('The track has no timestamps, so time-based features are off.') &&
    untimed.notes.includes('The track has no times, so the weather is left out.'), untimed.notes.join(' | '));
  await shot('3-notes');

  // The samples, from the site alone
  for (const sample of samples) {
    await open();
    state.seen.length = 0;
    asked.length = 0;
    await cdp.evaluate(`document.querySelector('[data-sample="${sample.slug}"]').click()`);
    check(`sample ${sample.slug}: shows live`, await ready());
    const elsewhere = asked.filter((url) => !url.startsWith(`${ORIGIN}/`) && !/^(blob|data):/.test(url));
    check(`sample ${sample.slug}: no data service is asked`, services_asked().length === 0 && elsewhere.length === 0, elsewhere.join(', '));
    check(`sample ${sample.slug}: its scene file comes from the site`, state.seen.some(({ url }) => url === `/samples/${sample.slug}.itb`));
    const links = await cdp.evaluate("[document.getElementById('download').getAttribute('href'), document.getElementById('gpx').hidden ? null : document.getElementById('gpx').getAttribute('href')]");
    check(`sample ${sample.slug}: its scene and GPX files can be downloaded`, links[0] === `/samples/${sample.slug}.itb` && links[1] === `/samples/${sample.slug}.gpx`, JSON.stringify(links));
    if (sample === samples[0]) await shot('4-sample');
  }

  // The keyboard alone, under reduced motion
  await reducedMotion(true);
  await open();
  await cdp.send('Page.setInterceptFileChooserDialog', { enabled: true });
  chooser = undefined;
  check('keyboard: Tab reaches the drop zone', await tabTo("document.activeElement?.id === 'file'"));
  await key('Enter', 'Enter', 13, '\r');
  for (let i = 0; i < 30 && !chooser; i++) await sleepFor(100);
  if (!chooser) {
    await key(' ', 'Space', 32, ' ');
    for (let i = 0; i < 30 && !chooser; i++) await sleepFor(100);
  }
  check('keyboard: a key opens the file chooser', !!chooser);
  if (chooser) await cdp.send('DOM.setFileInputFiles', { files: [files.walk], backendNodeId: chooser.backendNodeId });
  await cdp.send('Page.setInterceptFileChooserDialog', { enabled: false });
  await settled();
  check('keyboard: the chosen file is built and shown', await ready());
  await sleepFor(3000);
  check('reduced motion: a built hike doesn\'t play by itself', !(await cdp.evaluate("window.__events.includes('isotrail-play')")));
  check('keyboard: Tab reaches the diorama', await tabTo("document.getElementById('hike').shadowRoot?.activeElement?.tagName === 'CANVAS'"));
  const before = await hash();
  await key('ArrowLeft', 'ArrowLeft', 37);
  await key('ArrowLeft', 'ArrowLeft', 37);
  await sleepFor(500);
  check('keyboard: arrows turn it', (await hash()) !== before);
  await key(' ', 'Space', 32, ' ');
  check('keyboard: Space plays it', await cdp.waitFor("window.__events.includes('isotrail-play')", 5000));
  await rm(join(downloads, 'walk.itb'), { force: true });
  check('keyboard: Tab reaches the download', await tabTo("document.activeElement?.id === 'download'"));
  await key('Enter', 'Enter', 13, '\r');
  check('keyboard: Enter downloads the scene file', (await downloaded('walk.itb'))?.equals(await scene()));
  await open();
  check('keyboard: Tab reaches a sample', await tabTo(`document.activeElement?.dataset?.sample === '${samples[1].slug}'`));
  await key('Enter', 'Enter', 13, '\r');
  check('keyboard: Enter shows the sample', await ready());
  await sleepFor(3000);
  check('reduced motion: a sample doesn\'t play by itself', !(await cdp.evaluate("window.__events.includes('isotrail-play')")));
  await reducedMotion(false);

  // The controls, under reduced motion so the picture holds still
  await reducedMotion(true);
  await open();
  const described = JSON.parse(await readFile(join(ROOT, 'site', 'src', 'options.json'), 'utf8')).map(({ name }) => name);
  const placed = await cdp.evaluate('({ shown: window.__controls.shown, leftOut: window.__controls.leftOut, later: window.__controls.later })');
  const every = [...placed.shown, ...placed.leftOut, ...placed.later];
  check('controls: every option is a control, left out or for later, and nothing else',
    every.length === described.length && new Set(every).size === every.length && described.every((name) => every.includes(name)),
    JSON.stringify({ missing: described.filter((name) => !every.includes(name)), extra: every.filter((name) => !described.includes(name)) }));
  check('controls: the left-out list is the page\'s own',
    JSON.stringify(placed.leftOut.toSorted()) === JSON.stringify(['cache-dir', 'ffmpeg', 'heading', 'offline', 'overpass-url', 'terrain-url', 'weather-url']),
    placed.leftOut.join(', '));

  // Each control that changes a finished scene is tried on a sample walk,
  // which has paths, streams and lakes and asks no data service. The ones
  // that build the scene again are tried on the dropped file, whose
  // services are the recordings
  const tryEach = async (names) => {
    for (const name of names) {
      const tried = TRIES[name];
      check(`control ${name}: has a value to try`, !!tried);
      if (!tried) continue;
      await cdp.evaluate('window.__controls.reset()');
      for (const [other, value] of Object.entries(tried.with ?? {})) await set(other, value);
      const before = await picture();
      const sceneBefore = await shownScene();
      const seen = services_asked().length;
      await set(name, tried.value);
      const problem = await cdp.evaluate(`window.__controls.problem(${JSON.stringify(name)})`);
      const after = await picture();
      check(`control ${name}: changes the picture`, after !== before && !problem, problem);
      const rebuilt = services_asked().length - seen;
      if (REBUILDS.includes(name)) {
        check(`control ${name}: builds the scene again`, !(await shownScene()).equals(sceneBefore));
        await cdp.evaluate('window.__controls.reset()');
        for (const [other, value] of Object.entries(tried.with ?? {})) await set(other, value);
        const warm = services_asked().length;
        await set(name, tried.value);
        check(`control ${name}: again with a warm cache, with no request`, services_asked().length === warm && (await picture()) === after,
          `${services_asked().length - warm} requests, ${rebuilt} the first time`);
      } else {
        check(`control ${name}: with no request`, rebuilt === 0, `${rebuilt} requests`);
      }
    }
  };
  check('controls: those that build again are controls', REBUILDS.every((name) => placed.shown.includes(name)));
  await cdp.evaluate(`document.querySelector('[data-sample="${samples[0].slug}"]').click()`);
  await ready();
  const samplePicture = await picture();
  await shot('8-controls');
  await tryEach(placed.shown.filter((name) => name !== 'theme' && !REBUILDS.includes(name)));
  await cdp.evaluate('window.__controls.reset()');
  check('controls: reset gives the sample\'s first picture back exactly', (await picture()) === samplePicture);
  await drop([files.walk]);
  await settled();
  await ready();
  const firstPicture = await picture();
  await tryEach(REBUILDS);
  await cdp.evaluate('window.__controls.reset()');
  check('controls: reset gives the first picture back exactly', (await picture()) === firstPicture);

  // The view, from the keyboard
  await tabTo("document.getElementById('hike').shadowRoot?.activeElement?.tagName === 'CANVAS'");
  await key('ArrowLeft', 'ArrowLeft', 37);
  check('camera: the turn field follows the diorama',
    await cdp.waitFor("window.__controls.value('turn') === 10", 3000), String(await cdp.evaluate("window.__controls.value('turn')")));
  await cdp.evaluate("document.getElementById('hike').shadowRoot.activeElement?.blur()");
  await cdp.evaluate('window.__controls.reset()');
  check('camera: reset turns it back', (await picture()) === firstPicture);

  // Palette and theme files
  const bad = { palette: join(temp, 'bad.hex'), theme: join(temp, 'bad.toml') };
  const good = { palette: join(temp, 'lospec.hex'), theme: join(temp, 'green.toml') };
  await writeFile(bad.palette, 'ff0000\nzzzzzz\n');
  await writeFile(good.palette, '000000\n1d2b53\n7e2553\n008751\nab5236\n5f574f\nc2c3c7\nfff1e8\nff004d\nffa300\nffec27\n00e436\n29adff\n83769c\nff77a8\nffccaa\n');
  await writeFile(bad.theme, '[route]\nshade = 1\n');
  await writeFile(good.theme, '[route]\ncolour = "#00ff00"\n');
  const choose = async (input, path) => {
    const { root } = await cdp.send('DOM.getDocument', { depth: 0 });
    const { nodeId } = await cdp.send('DOM.querySelector', { nodeId: root.nodeId, selector: input });
    await cdp.send('DOM.setFileInputFiles', { files: [path], nodeId });
    await cdp.evaluate('new Promise((resolve) => setTimeout(resolve, 100)).then(() => window.__controls.settled())');
  };
  await choose('#palette-input', bad.palette);
  check('palette file: a bad one gets the tool\'s message',
    (await cdp.evaluate("window.__controls.problem('palette')")) === 'Can\'t read the palette bad.hex: line 2: expected a colour as RRGGBB, found "zzzzzz".',
    await cdp.evaluate("window.__controls.problem('palette')"));
  check('palette file: a bad one changes nothing', (await picture()) === firstPicture);
  await choose('#palette-input', good.palette);
  check('palette file: a good one is used', (await picture()) !== firstPicture && (await cdp.evaluate("window.__controls.problem('palette')")) === '');
  await cdp.evaluate('window.__controls.reset()');
  await choose('#option-theme', bad.theme);
  check('theme file: a bad one gets the tool\'s message',
    (await cdp.evaluate("window.__controls.problem('theme')")) === 'The theme bad.toml: unknown key "shade" in the [route] table.',
    await cdp.evaluate("window.__controls.problem('theme')"));
  check('theme file: a bad one changes nothing', (await picture()) === firstPicture);
  await choose('#option-theme', good.theme);
  check('theme file: a good one is used', (await picture()) !== firstPicture && (await cdp.evaluate("window.__controls.problem('theme')")) === '');
  await cdp.evaluate('window.__controls.reset()');
  check('files: reset leaves neither', (await picture()) === firstPicture);

  // A sample changed without a download, and servers left out
  const { 'overpass-url': _ours, ...builtIn } = OPTIONS;
  await open(builtIn);
  await cdp.evaluate(`document.querySelector('[data-sample="${samples[0].slug}"]').click()`);
  await ready();
  state.seen.length = 0;
  asked.length = 0;
  const sample = await picture();
  for (const [name, value] of [['season', 'winter'], ['exaggeration', 3], ['no-weather', true]]) {
    await set(name, value);
    check(`sample: ${name} changes it`, (await picture()) !== sample);
    await set(name, undefined);
  }
  for (const box of ['dresel', 'vk-maps', 'overpass-api']) {
    await cdp.evaluate(`(() => { const box = document.querySelector('[data-server="${box}"]'); box.checked = false; box.dispatchEvent(new Event('change')); })()`);
  }
  await cdp.evaluate('window.__controls.settled()');
  check('servers: none left is said at once',
    (await cdp.evaluate("window.__controls.problem('servers')")).startsWith('None of the Overpass servers left can fetch the map data here'),
    await cdp.evaluate("window.__controls.problem('servers')"));
  const strays = asked.filter((url) => !url.startsWith(`${ORIGIN}/`) && !/^(blob|data):/.test(url));
  check('sample: nothing asked of a data service', services_asked().length === 0 && strays.length === 0, strays.join(', '));
  await shot('9-sample');
  await reducedMotion(false);

  // The settings this browser keeps
  await reducedMotion(true);
  await open();
  for (const [name, value] of [['aspect', 'square'], ['season', 'winter'], ['paths', 'off'], ['trim-start', '300m'], ['turn', 90]]) await set(name, value);
  await untick('vk-maps');
  const kept = await cdp.evaluate('window.__controls.kept()');
  check('kept: the settings set, without the camera', JSON.stringify(kept) === JSON.stringify({
    version: 1, remember: true, options: { aspect: 'square', season: 'winter', paths: 'off', 'trim-start': '300m' }, skip: ['vk-maps'],
  }), JSON.stringify(kept));
  await open(OPTIONS, { keep: true });
  const back = await cdp.evaluate(`(() => {
    const value = window.__controls.value;
    return {
      values: ['aspect', 'season', 'paths', 'trim-start', 'turn'].map(value),
      shape: getComputedStyle(document.getElementById('frame')).getPropertyValue('--shape').trim(),
      attribute: document.getElementById('hike').getAttribute('aspect'),
      box: document.querySelector('[data-server="vk-maps"]').checked,
      notice: document.getElementById('kept').hidden ? null : document.getElementById('kept-text').textContent,
    };
  })()`);
  check('kept: they come back after a reload', JSON.stringify(back.values) === JSON.stringify(['square', 'winter', 'off', '300m', null]) &&
    back.shape === '1 / 1' && back.attribute === 'square' && back.box === false, JSON.stringify(back));
  check('kept: the first screen names them',
    back.notice === 'This browser kept your settings from last time: Shape, Season, Paths, Trim the start and Map data servers.', back.notice);
  await cdp.evaluate("document.getElementById('remember').click()");
  const off = JSON.stringify({ version: 1, remember: false });
  check('kept: turning remembering off keeps only that', JSON.stringify(await cdp.evaluate('window.__controls.kept()')) === off,
    JSON.stringify(await cdp.evaluate('window.__controls.kept()')));
  await open(OPTIONS, { keep: true });
  const forgotten = await cdp.evaluate(`({
    aspect: window.__controls.value('aspect') ?? null,
    box: document.querySelector('[data-server="vk-maps"]').checked,
    remember: document.getElementById('remember').checked,
    notice: !document.getElementById('kept').hidden,
  })`);
  check('kept: after a reload, nothing comes back and the switch stays off',
    JSON.stringify(forgotten) === JSON.stringify({ aspect: null, box: true, remember: false, notice: false }), JSON.stringify(forgotten));
  await set('season', 'summer');
  check('kept: nothing is saved while it is off', JSON.stringify(await cdp.evaluate('window.__controls.kept()')) === off);
  await cdp.evaluate("document.getElementById('remember').click()");
  check('kept: turning it on again saves what shows', JSON.stringify((await cdp.evaluate('window.__controls.kept()'))?.options) === JSON.stringify({ season: 'summer' }),
    JSON.stringify(await cdp.evaluate('window.__controls.kept()')));
  await cdp.evaluate('window.__controls.reset()');
  check('kept: reset leaves nothing kept', (await cdp.evaluate('window.__controls.kept()')) === null);
  const stored = [
    ['another version', JSON.stringify({ version: 0, options: { aspect: 'square' } })],
    ['a broken one', '{"version": 1, "options": {'],
    ['values the page no longer takes', JSON.stringify({
      version: 1, remember: true, options: { aspect: 'triangle', 'block-height': 99, turn: 5, nonsense: 1 }, skip: ['nowhere'],
    })],
  ];
  for (const [what, text] of stored) {
    await cdp.evaluate(`localStorage.setItem('isotrail.settings', ${JSON.stringify(text)})`);
    const errors = cdp.exceptions.length;
    await open(OPTIONS, { keep: true });
    const fresh = await cdp.evaluate(`({
      aspect: window.__controls.value('aspect') ?? null,
      height: window.__controls.value('block-height') ?? null,
      notice: !document.getElementById('kept').hidden,
      kept: window.__controls.kept(),
    })`);
    check(`kept: ${what} is dropped without an error`,
      fresh.aspect === null && fresh.height === null && !fresh.notice && fresh.kept === null && cdp.exceptions.length === errors, JSON.stringify(fresh));
  }
  await reducedMotion(false);

  // Phone and desktop sizes, at the browser's own text size and at twice it
  await reducedMotion(true);
  const walks = {};
  for (const screen of [PHONE, DESKTOP]) {
    for (const text of [16, 32]) {
      const at = `${screen.width}px${text === 32 ? ', large text' : ''}`;
      await size(screen, text);
      await open();
      check(`${at}: no sideways scroll on the first screen`, (await sideways()) === 0, `${await sideways()}px`);
      check(`${at}: the first screen passes the accessibility scan`, ...scanned(await scan()));
      await cdp.evaluate(`document.querySelector('[data-sample="${samples[0].slug}"]').click()`);
      check(`${at}: the diorama shows`, await ready(), await cdp.evaluate(`JSON.stringify({
        state: document.getElementById('hike').dataset.state,
        frame: document.getElementById('frame').getBoundingClientRect(),
        scrolled: scrollY, height: innerHeight, focus: document.hasFocus(),
      })`));
      await cdp.evaluate("document.querySelectorAll('#controls details').forEach((details) => (details.open = true))");
      check(`${at}: no sideways scroll with every group open`, (await sideways()) === 0, `${await sideways()}px`);
      check(`${at}: the page with a hike passes the accessibility scan`, ...scanned(await scan()));
      await cdp.evaluate("[...document.querySelectorAll('#controls [data-option]')].at(-1).scrollIntoView({ block: 'end' })");
      await sleepFor(300);
      const view = await cdp.evaluate(`(() => {
        const { top, bottom } = document.getElementById('frame').getBoundingClientRect();
        return { top, bottom, height: innerHeight, scrolled: scrollY };
      })()`);
      check(`${at}: the diorama stays in view at the last control`, view.scrolled > 0 && view.top >= 0 && view.bottom <= view.height, JSON.stringify(view));
      await drop([files.photo]);
      await settled();
      check(`${at}: the error shows`, (await shown()).error?.text === 'photo.png is an image, not a GPX file.', JSON.stringify((await shown()).error));
      check(`${at}: no sideways scroll with an error`, (await sideways()) === 0, `${await sideways()}px`);
      check(`${at}: the page with an error passes the accessibility scan`, ...scanned(await scan()));
      if (text === 16) walks[screen.width] = await tabWalk(at);
    }
  }
  if (SHOTS) for (const [width, order] of Object.entries(walks)) await writeFile(join(SHOTS, `walk-${width}.json`), `${JSON.stringify(order, null, 1)}\n`);
  await size(VIEWPORT, 16);

  // Touch on a phone: a swipe up the diorama scrolls the page, a swipe across
  // turns the diorama
  await reducedMotion(true);
  await size(PHONE, 16);
  await open();
  await cdp.evaluate(`document.querySelector('[data-sample="${samples[0].slug}"]').click()`);
  await ready();
  await cdp.evaluate("window.scrollTo(0, document.getElementById('controls').offsetTop - 600)");
  await sleepFor(300);
  const centre = () => cdp.evaluate(`(() => {
    const { x, y, width, height } = document.getElementById('frame').getBoundingClientRect();
    return { x: x + width / 2, y: y + height / 2, scrolled: scrollY };
  })()`);
  const start = await centre();
  await swipe(start, 0, -150);
  const up = await cdp.evaluate("({ scrolled: scrollY, turn: window.__controls.value('turn') ?? null })");
  check('touch: a swipe up the diorama scrolls the page', up.scrolled > start.scrolled + 50 && up.turn === null, JSON.stringify({ before: start.scrolled, ...up }));
  const across = await centre();
  await swipe({ x: across.x - 80, y: across.y }, 160, 0);
  await sleepFor(500);
  const turned = await cdp.evaluate("({ scrolled: scrollY, turn: window.__controls.value('turn') ?? null })");
  check('touch: a swipe across turns the diorama and leaves the page', Math.abs(turned.scrolled - across.scrolled) <= 2 && turned.turn !== null,
    JSON.stringify({ before: across.scrolled, ...turned }));
  await size(VIEWPORT, 16);
  await reducedMotion(false);

  // Memory: a worker grown by a build is replaced, and the page goes on
  await open();
  await cdp.evaluate('window.__recycle = 0; window.__scenes.first = true');
  await drop([files.walk]);
  await settled();
  check('memory: a worker grown past the bar is replaced', await cdp.evaluate('window.__scene.recycled === true && !window.__scenes.first'));
  await set('block-height', 2);
  check('memory: the fresh worker checks options', (await cdp.evaluate("window.__controls.problem('block-height')")) === '');
  await drop([files.walk]);
  await settled();
  check('memory: and builds again', (await cdp.evaluate('window.__scene.state')) === 'built' && (await scene()).equals(walk) && (await ready()));
  await cdp.evaluate('window.__recycle = undefined');

  // Each change of scene starts a renderer of its own, and frees the last
  const devicesCounted = await cdp.send('Page.addScriptToEvaluateOnNewDocument', {
    source: `window.__devices = { made: 0, destroyed: 0 };
      if (window.GPUAdapter) {
        const request = GPUAdapter.prototype.requestDevice;
        GPUAdapter.prototype.requestDevice = function (...args) { window.__devices.made++; return request.apply(this, args); };
        const destroy = GPUDevice.prototype.destroy;
        GPUDevice.prototype.destroy = function (...args) { window.__devices.destroyed++; return destroy.apply(this, args); };
      }`,
  });
  await reducedMotion(true);
  await open();
  await cdp.evaluate(`document.querySelector('[data-sample="${samples[0].slug}"]').click()`);
  await ready();
  for (let i = 0; i < 20; i++) await set('season', i % 2 ? 'summer' : 'winter');
  check('memory: twenty changes in a row still show', await ready());
  const devices = await cdp.evaluate('window.__devices');
  if (!WEBGL) check('memory: one renderer is left after twenty changes', devices.made > 20 && devices.made - devices.destroyed === 1, JSON.stringify(devices));
  await cdp.send('Page.removeScriptToEvaluateOnNewDocument', { identifier: devicesCounted.identifier });
  await reducedMotion(false);

  // What the page refuses, and what fails on the way
  await open();
  const failing = async (name, run, expect) => {
    await run();
    await settled();
    const seen = await shown();
    check(`${name}: says what happened`, seen.error?.lead === expect.lead && expect.text(seen.error?.text ?? ''), JSON.stringify(seen.error));
    return seen;
  };

  await failing('a picture', () => drop([files.photo]), {
    lead: "This file can't be used.",
    text: (text) => text === 'photo.png is an image, not a GPX file.',
  });
  await shot('5-error');
  const several = await (async () => {
    await drop([files.walk, files.untimed]);
    await sleepFor(300);
    return shown();
  })();
  check('several files: one at a time', several.error?.text === 'Drop one GPX file at a time.', JSON.stringify(several.error));
  const folder = await (async () => {
    await drop([files.folder]);
    await sleepFor(300);
    return shown();
  })();
  check('a folder: named as one', folder.error?.text === 'Hikes is a folder, not a GPX file.', JSON.stringify(folder.error));

  await cdp.evaluate(`window.__options = ${JSON.stringify({ ...OPTIONS, padding: '0,0' })}`);
  await failing('one spot', () => drop([files.spot]), {
    lead: "There's no ground to draw.",
    text: (text) => text.startsWith('The route stays on one spot'),
  });

  await cdp.evaluate(`window.__options = ${JSON.stringify(OPTIONS)}`);
  const large = await failing('too large', () => drop([files.long]), {
    lead: 'This route covers too much land for map data in a browser.',
    text: (text) => text.includes('without lakes, woods and paths'),
  });
  check('too large: offers a build without map data', large.noMapData);
  state.seen.length = 0;
  await cdp.evaluate("document.getElementById('no-map-data').click()");
  await settled();
  const kinds = services_asked().map(({ url }) => url.split('/')[1]);
  check('too large: that build asks no map data', kinds.includes('terrain') && !kinds.includes('overpass'), kinds.join(', '));

  // A fresh page, as that button left map data out of the page's options
  await open();
  const { 'overpass-url': _, ...BUILT_IN } = OPTIONS;
  await cdp.evaluate("window.__skip = ['dresel', 'vk-maps', 'overpass-api']");
  await cdp.evaluate(`window.__options = ${JSON.stringify(BUILT_IN)}`);
  const none = await failing('no server', () => drop([files.walk]), {
    lead: 'No map data server is left to ask.',
    text: (text) => text.includes('without lakes, woods and paths'),
  });
  check('no server: offers a build without map data', none.noMapData);
  await cdp.evaluate('window.__skip = undefined');

  await cdp.evaluate(`window.__options = ${JSON.stringify({ ...OPTIONS, 'overpass-url': `http://127.0.0.1:${CLOSED}/api/interpreter` })}`);
  const away = await failing('unreachable', () => drop([files.walk]), {
    lead: "A data service can't be reached.",
    text: (text) => text.startsWith(`Could not reach http://127.0.0.1:${CLOSED}`),
  });
  check('unreachable: offers another try', away.retry);
  await cdp.evaluate(`window.__options = ${JSON.stringify(OPTIONS)}`);
  await cdp.evaluate("document.getElementById('retry').click()");
  await settled();
  check('unreachable: another try builds once the server answers', (await cdp.evaluate('window.__scene.state')) === 'built');

  await cdp.evaluate('window.__scenes.clearCache()');
  await setOffline(true);
  const cut = await failing('offline', () => drop([files.walk]), {
    lead: 'This browser is offline.',
    text: (text) => text.startsWith("There's no network connection"),
  });
  check('offline: offers another try', cut.retry);
  await setOffline(false);

  // A busy map data server, with the countdown
  state.mode.overpass = 'busy';
  await drop([files.walk]);
  const counting = await cdp.waitFor("!document.getElementById('wait').hidden && document.getElementById('wait').textContent.includes('Trying again in')", 10000);
  check('busy: the wait is counted down', counting, await cdp.evaluate("document.getElementById('wait').textContent"));
  await shot('6-waiting');
  await settled();
  check('busy: then the scene is built', (await cdp.evaluate('window.__scene.state')) === 'built');

  // Cancel while Overpass holds its answer back
  await cdp.evaluate('window.__scenes.clearCache()');
  state.mode.overpass = 'hold';
  await drop([files.walk]);
  for (let i = 0; i < 100 && state.held === 0; i++) await sleepFor(100);
  await shot('7-building');
  await cdp.evaluate("document.getElementById('cancel').click()");
  await settled();
  state.mode.overpass = 'normal';
  const cancelled = await shown();
  check('cancel: the build stops without an error', (await cdp.evaluate('window.__scene.state')) === 'cancelled' && !cancelled.error && !cancelled.building);
  for (let i = 0; i < 50 && state.open > 0; i++) await sleepFor(100);
  check('cancel: no request is left running', state.held === 0);

  // After all that, the page still works
  await drop([files.walk]);
  await settled();
  check('after them all: a dropped file still builds', (await cdp.evaluate('window.__scene.state')) === 'built' && (await scene()).equals(walk));
  check('after them all: and shows', await ready());
  check('no script error on the page', cdp.exceptions.length === 0, cdp.exceptions.join(' | '));
} catch (error) {
  check('the check ran to the end', false, error.stack ?? String(error));
} finally {
  await quit(browser, cdp);
  services.close();
  await sleepFor(500);
  await rm(browser.profile, { recursive: true, force: true, maxRetries: 5 }).catch(() => {});
  await rm(temp, { recursive: true, force: true, maxRetries: 5 }).catch(() => {});
}

console.log(`\n${passed} passed, ${failures.length} failed (${BROWSER}${WEBGL ? ', WebGL2' : ''})`);
for (const failure of failures) console.log(`  ${failure}`);
process.exit(failures.length ? 1 : 0);
