// Headless browser check of a scene built in the browser from a GPX file
//
//   node web/test/scene.mjs [chrome|edge] [--webgl] [--site DIR]
//
// Needs what check.mjs needs, and the scene worker built into web/scene/pkg
// (sh web/build-scene.sh). With --site, the same checks run on the page of a
// built site (site/dist after sh site/build.sh), served with the headers in
// its _headers file. A local server plays the elevation, map and
// weather services from the recordings in fixtures/, and `isotrail bundle`
// builds each scene from the same server for comparison. The server can also
// be busy, hold an answer back or stop answering, for the cache, the waits,
// cancel and the failures. Exits with 1 if any check fails

import { spawn, spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { gzipSync } from 'node:zlib';

import { connect, launch, quit, sleepFor } from './browser.mjs';
import { SERVICE, closedPort, serve } from './services.mjs';

const ROOT = fileURLToPath(new URL('../..', import.meta.url));
const WEB = join(ROOT, 'web');
const GPX = join(ROOT, 'fixtures', 'gpx', 'snowdon.gpx');
const args = process.argv.slice(2);
const WEBGL = args.includes('--webgl');
const siteAt = args.indexOf('--site');
const SITE = siteAt >= 0 ? resolve(args[siteAt + 1] ?? '') : undefined;
const BROWSER = args.find((arg, i) => !arg.startsWith('--') && (siteAt < 0 || i !== siteAt + 1)) ?? 'chrome';
const TIMEOUT_MS = 60000;
const VIEWPORT = { width: 1000, height: 800, deviceScaleFactor: 1, mobile: false };

for (const [file, build] of [['pkg/isotrail_web_bg.wasm', 'build.sh'], ['scene/pkg/isotrail_scene_bg.wasm', 'build-scene.sh']]) {
  if (!existsSync(join(WEB, file))) {
    console.error(`web/${file} is missing: build it first with sh web/${build}`);
    process.exit(1);
  }
}
if (SITE && !existsSync(join(SITE, 'index.html'))) {
  console.error(`${SITE} holds no built site: build it first with sh site/build.sh`);
  process.exit(1);
}
const built = spawnSync('cargo', ['build', '--quiet', '-p', 'isotrail-cli'], { cwd: ROOT, stdio: 'inherit' });
if (built.status !== 0) {
  console.error('could not build isotrail with cargo');
  process.exit(1);
}

const gpx = await readFile(GPX);
const trackName = /<name>(?:<!\[CDATA\[)?([^<\]]+)/.exec(gpx.toString('utf8'))[1].trim();
// Things that would give the file away in a request
const MARKERS = ['<gpx', 'trkpt', trackName, gpx.subarray(0, 64).toString('utf8')];

const services = await serve({ site: SITE });
const { origin: ORIGIN, servers: SERVERS, state } = services;
const { seen, mode } = state;
const CLOSED = await closedPort();

// Each case as flags for `isotrail bundle` and as the page's JSON options
const CASES = [
  { name: 'recorded times', flags: [], options: {} },
  { name: 'coarse times', flags: ['--times', 'coarse'], options: { times: 'coarse' } },
  { name: 'no map data', flags: ['--no-map-data'], options: { 'no-map-data': true } },
  // A trimmed slab asks Overpass a query that has no recording, so it goes without map data
  {
    name: 'trimmed',
    flags: ['--trim-start', '300m', '--trim-end', '0.2km', '--no-map-data'],
    options: { 'trim-start': '300m', 'trim-end': '0.2km', 'no-map-data': true },
  },
];

const temp = await mkdtemp(join(tmpdir(), 'isotrail-scene-'));
const failures = [];
let passed = 0;
function check(name, ok, detail = '') {
  if (ok) passed++;
  else failures.push(`${name}${detail ? `: ${detail}` : ''}`);
  console.log(`${ok ? 'ok  ' : 'FAIL'} ${name}${!ok && detail ? ` (${detail})` : ''}`);
}

// The command line, run while this process serves its requests
async function bundle(index, flags) {
  const output = join(temp, `cli-${index}.itb`);
  const serverFlags = Object.entries(SERVERS).flatMap(([key, value]) => [`--${key}`, value]);
  const env = { ...process.env, ISOTRAIL_CONFIG: '' };
  for (const name of ['ISOTRAIL_OVERPASS_URL', 'ISOTRAIL_TERRAIN_URL', 'ISOTRAIL_WEATHER_URL', 'ISOTRAIL_OPEN_METEO_KEY', 'ISOTRAIL_CACHE_DIR']) {
    delete env[name];
  }
  const child = spawn(
    'cargo',
    [
      'run', '--quiet', '-p', 'isotrail-cli', '--', 'bundle', GPX, '-o', output, '--detail', '32',
      '--cache-dir', join(temp, `cache-${index}`), '--quiet', ...serverFlags, ...flags,
    ],
    { cwd: ROOT, env, stdio: ['ignore', 'ignore', 'inherit'] },
  );
  const code = await new Promise((resolve) => child.on('exit', resolve));
  if (code !== 0) throw new Error(`isotrail bundle exited with ${code}`);
  return readFile(output);
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
cdp.on('Network.requestWillBeSent', ({ request }) => asked.push({ url: request.url, body: request.postData ?? '' }));
cdp.on('Target.attachedToTarget', async ({ sessionId }) => {
  await cdp.send('Network.enable', {}, sessionId).catch(() => {});
  await cdp.send('Runtime.runIfWaitingForDebugger', {}, sessionId).catch(() => {});
});
await cdp.send('Runtime.enable');
await cdp.send('Page.enable');
await cdp.send('DOM.enable');
await cdp.send('Network.enable');
await cdp.send('Target.setAutoAttach', { autoAttach: true, waitForDebuggerOnStart: true, flatten: true });
await cdp.send('Emulation.setDeviceMetricsOverride', VIEWPORT);

async function page() {
  await cdp.send('Page.navigate', { url: SITE ? `${ORIGIN}/` : `${ORIGIN}/web/test/scene.html` });
  await cdp.waitFor("document.readyState === 'complete' && window.__scene?.state === 'idle' && !!window.__scenes");
}

// Builds `file` on the test page with these options and says how it went
//
// `during` runs once the build has started
async function build(file, options, { clear = false, during } = {}) {
  if (clear) await cdp.evaluate('window.__scenes.clearCache()');
  asked.length = 0;
  seen.length = 0;
  // The site's page has no options box: it adds `window.__options` to each build
  await cdp.evaluate(SITE
    ? `window.__options = ${JSON.stringify(options)}`
    : `document.getElementById('options').value = ${JSON.stringify(JSON.stringify(options))}`);
  const { result } = await cdp.send('Runtime.evaluate', { expression: "document.getElementById('file')" });
  const started = Date.now();
  await cdp.send('DOM.setFileInputFiles', { files: [file], objectId: result.objectId });
  if (during) {
    await cdp.waitFor("window.__scene.state === 'building'", 5000);
    await during();
  }
  await cdp.waitFor("['built', 'failed', 'cancelled'].includes(window.__scene.state)");
  const wall = Date.now() - started;
  const outcome = await cdp.evaluate(`(() => {
    const { scene, ...rest } = window.__scene;
    let text = '';
    for (let i = 0; scene && i < scene.length; i += 0x8000) text += String.fromCharCode(...scene.subarray(i, i + 0x8000));
    return { ...rest, scene: btoa(text) };
  })()`);
  const services = seen.filter(({ url }) => SERVICE.test(new URL(url, ORIGIN).pathname));
  return { ...outcome, scene: Buffer.from(outcome.scene ?? '', 'base64'), wall, services };
}

const results = [];
const row = (name, outcome) => ({
  name,
  bytes: outcome.scene.length,
  gzipped: gzipSync(outcome.scene, { level: 9 }).length,
  passes: outcome.passes,
  worker_ms: Math.round(outcome.ms),
  wall_ms: outcome.wall,
  warnings: outcome.warnings ?? [],
  requests: outcome.services.length,
});
try {
  for (const [index, { name, flags, options }] of CASES.entries()) {
    const expected = await bundle(index, flags);
    await page();
    const outcome = await build(GPX, { detail: 32, ...SERVERS, ...options }, { clear: true });
    const { scene, services } = outcome;
    check(`${name}: the scene is built`, outcome.state === 'built', outcome.error);
    check(`${name}: it is byte for byte the one isotrail bundle makes`, scene.equals(expected), `${scene.length} against ${expected.length} bytes`);
    const ready = await cdp.waitFor("document.getElementById('hike').dataset.state === 'ready'");
    check(`${name}: the element shows it`, ready, await cdp.evaluate("document.getElementById('hike').dataset.state"));
    const elsewhere = asked.filter(({ url }) => !url.startsWith(`${ORIGIN}/`) && !/^(blob|data):/.test(url));
    check(`${name}: no request leaves for anywhere but the local server`, elsewhere.length === 0, elsewhere.map((r) => r.url).join(', '));
    check(`${name}: the services were asked`, services.length > 0);
    const decoded = (text) => {
      try {
        return decodeURIComponent(text.replaceAll('+', ' '));
      } catch {
        return text;
      }
    };
    const giving = [...asked, ...seen].filter(({ url, body }) => {
      const text = `${url}\n${body}\n${decoded(url)}\n${decoded(body)}`;
      return MARKERS.some((marker) => text.includes(marker));
    });
    check(`${name}: no request holds the GPX`, giving.length === 0, giving.map((r) => r.url).join(', '));
    results.push(row(name, outcome));
  }

  const OPTIONS = { detail: 32, ...SERVERS };
  const expected = await bundle(CASES.length, []);
  await page();

  // The cache, cold then warm
  const cold = await build(GPX, OPTIONS, { clear: true });
  check('cold: the scene is built', cold.state === 'built' && cold.scene.equals(expected), cold.error);
  const tiles = cold.services.filter(({ url }) => url.startsWith('/terrain/')).length;
  const counts = cold.events.filter((e) => e.type === 'progress' && e.step === 'elevation');
  const last = counts.at(-1);
  check('cold: progress counts the tiles up to their number', tiles > 0 && last?.done === tiles && last?.total === tiles && counts.some((e) => e.done === 1), JSON.stringify(last));
  check('cold: progress names every step', ['elevation', 'map-data', 'weather', 'building'].every((step) => cold.events.some((e) => e.type === 'progress' && e.step === step)));
  const cache = await cdp.evaluate('window.__scenes.cacheSize()');
  check('cold: the answers are kept in the cache', cache.entries === cold.fetched && cache.bytes > 0, JSON.stringify(cache));
  const warm = await build(GPX, OPTIONS);
  check('warm: a second build makes no request', warm.state === 'built' && warm.services.length === 0, warm.services.map((r) => r.url).join(', '));
  check('warm: it is the same scene', warm.scene.equals(expected));
  results.push(row('cold', cold), row('warm', warm));

  // The services stopped after the first build
  mode.services = false;
  const stopped = await build(GPX, OPTIONS);
  mode.services = true;
  check('services stopped: the cache still builds the scene', stopped.state === 'built' && stopped.scene.equals(expected), stopped.error);

  // A busy Overpass, once
  mode.overpass = 'busy';
  const busy = await build(GPX, OPTIONS, { clear: true });
  const wait = busy.events.find((e) => e.type === 'wait');
  check('busy: the wait is told', wait?.kind === 'map-data' && wait.why === 'busy' && wait.attempt === 2 && wait.attempts === 3, JSON.stringify(wait));
  // The worker's own figure: the page hears the message late while its main thread draws
  check('busy: the wait is 5s', wait?.ms === 5000, `${wait?.ms}ms`);
  check('busy: then the scene is built', busy.state === 'built' && busy.scene.equals(expected), busy.error);
  check('busy: it took the wait', busy.wall >= 5000, `${busy.wall}ms`);
  results.push(row('busy once', busy));

  // The page's own list of Overpass servers, the first busy or empty
  const { 'overpass-url': _, ...LISTED } = OPTIONS;
  for (const first of ['busy', 'empty']) {
    await cdp.evaluate(`window.__servers = ['${ORIGIN}/${first}/api/interpreter', '${ORIGIN}/overpass/api/interpreter']`);
    const handed = await build(GPX, LISTED, { clear: true });
    const asked = (prefix) => handed.services.filter(({ url }) => url.startsWith(prefix)).length;
    check(`servers, ${first} first: the next one gives the scene`, handed.state === 'built' && handed.scene.equals(expected), handed.error);
    check(`servers, ${first} first: at once, each asked once`, !handed.events.some((e) => e.type === 'wait') && asked(`/${first}/`) === 1 && asked('/overpass/') === 1);
    check(`servers, ${first} first: progress names the server`, handed.events.some((e) => e.type === 'progress' && e.step === 'map-data' && e.server === new URL(ORIGIN).host));
    const kept = await build(GPX, LISTED);
    check(`servers, ${first} first: a second build asks nothing`, kept.state === 'built' && kept.services.length === 0 && kept.scene.equals(expected), kept.services.map((r) => r.url).join(', '));
  }
  await cdp.evaluate('window.__servers = undefined');

  // Built-in servers left out by name: all of them, then one that doesn't exist
  await cdp.evaluate("window.__skip = ['dresel', 'vk-maps', 'overpass-api']");
  const none = await build(GPX, LISTED);
  check('skip all: refused as no server', none.state === 'failed' && none.reason === 'no-server', `${none.reason}: ${none.error}`);
  check('skip all: before any request', none.services.length === 0 && !asked.some(({ url }) => /dresel|mail\.ru|overpass-api/.test(url)), none.services.map((r) => r.url).join(', '));
  await cdp.evaluate("window.__skip = ['vk']");
  const unknown = await build(GPX, LISTED);
  check('skip an unknown name: a usage mistake', unknown.reason === 'usage' && unknown.error.includes('called vk'), `${unknown.reason}: ${unknown.error}`);
  await cdp.evaluate('window.__skip = undefined');

  // Overpass at an address where nothing answers
  const unreachable = await build(GPX, { ...OPTIONS, 'overpass-url': `http://127.0.0.1:${CLOSED}/api/interpreter` });
  check('unreachable: the build fails as unreachable', unreachable.state === 'failed' && unreachable.reason === 'unreachable', `${unreachable.reason}: ${unreachable.error}`);
  check('unreachable: it names the server', unreachable.error?.startsWith(`could not reach http://127.0.0.1:${CLOSED}; `), unreachable.error);
  check('unreachable: within 15s', unreachable.wall < 15000, `${unreachable.wall}ms`);

  // A full storage quota
  await cdp.send('Storage.overrideQuotaForOrigin', { origin: ORIGIN, quotaSize: 1 });
  const full = await build(GPX, OPTIONS, { clear: true });
  await cdp.send('Storage.overrideQuotaForOrigin', { origin: ORIGIN });
  check('full quota: the scene is still built', full.state === 'built' && full.scene.equals(expected), full.error);
  check('full quota: the answers it couldn\'t keep are counted', full.unsaved > 0, `${full.unsaved}`);

  // Cancel while Overpass holds its answer back
  mode.overpass = 'hold';
  const cancelled = await build(GPX, OPTIONS, {
    clear: true,
    during: async () => {
      for (let i = 0; i < 100 && state.held === 0; i++) await sleepFor(100);
      check('cancel: the map data request was under way', state.held === 1, `${state.held}`);
      await cdp.evaluate("document.getElementById('cancel').click()");
    },
  });
  mode.overpass = 'normal';
  check('cancel: the build says it was cancelled', cancelled.state === 'cancelled', cancelled.state);
  for (let i = 0; i < 50 && state.open > 0; i++) await sleepFor(100);
  check('cancel: no request is left running', state.held === 0 && state.open === 0, `${state.held} held, ${state.open} open`);

  // Routes and files the page refuses
  const long = join(temp, 'long.gpx');
  const points = Array.from({ length: 11 }, (_, i) => `<trkpt lat="${(52 + (150 / 111.2) * i / 10).toFixed(6)}" lon="-1.5"><ele>200</ele></trkpt>`).join('');
  await writeFile(long, `<?xml version="1.0"?><gpx version="1.1" creator="test"><trk><name>Long walk</name><trkseg>${points}</trkseg></trk></gpx>`);
  const large = await build(long, OPTIONS);
  check('too large: refused', large.state === 'failed' && large.reason === 'too-large', `${large.reason}: ${large.error}`);
  check('too large: before any request', large.services.length === 0, large.services.map((r) => r.url).join(', '));
  const picture = join(temp, 'photo.png');
  await writeFile(picture, Buffer.from('89504e470d0a1a0a0000000d49484452', 'hex'));
  const png = await build(picture, OPTIONS);
  check('a picture: refused as input', png.reason === 'input' && png.error === 'photo.png is an image, not a GPX file', `${png.reason}: ${png.error}`);
  const spot = join(temp, 'spot.gpx');
  await writeFile(spot, '<gpx version="1.1"><trk><trkseg><trkpt lat="52" lon="-1.5"></trkpt><trkpt lat="52" lon="-1.5"></trkpt></trkseg></trk></gpx>');
  const ground = await build(spot, { ...OPTIONS, padding: '0,0' });
  check('one spot: refused as no ground', ground.reason === 'no-ground', `${ground.reason}: ${ground.error}`);
  check('refusals: no service asked', [png, ground].every((r) => r.services.length === 0));
  const after = await build(GPX, OPTIONS);
  check('after them all: the page still builds', after.state === 'built' && after.scene.equals(expected), after.error);

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

console.log('\nscene, bytes, gzipped, passes, worker ms, ms from the file, service requests, left out');
for (const r of results) {
  console.log(`${r.name}, ${r.bytes}, ${r.gzipped}, ${r.passes}, ${r.worker_ms}, ${r.wall_ms}, ${r.requests}, ${r.warnings.join(' ') || 'nothing'}`);
}
console.log(`\n${passed} passed, ${failures.length} failed (${BROWSER}${WEBGL ? ', WebGL2' : ''}${SITE ? ', on the site' : ''})`);
for (const failure of failures) console.log(`  ${failure}`);
process.exit(failures.length ? 1 : 0);
