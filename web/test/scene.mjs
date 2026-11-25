// Headless browser check of a scene built in the browser from a GPX file
//
//   node web/test/scene.mjs [chrome|edge] [--webgl]
//
// Needs what check.mjs needs, and the scene worker built into web/scene/pkg
// (sh web/build-scene.sh). A local server plays the elevation, map and
// weather services from the recordings in fixtures/, and `isotrail bundle`
// builds each scene from the same server for comparison. Exits with 1 if any
// check fails

import { spawn, spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync } from 'node:fs';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { extname, join, normalize } from 'node:path';
import { fileURLToPath } from 'node:url';
import { gzipSync } from 'node:zlib';

import { connect, launch, quit, sleepFor } from './browser.mjs';

const ROOT = fileURLToPath(new URL('../..', import.meta.url));
const WEB = join(ROOT, 'web');
const FIXTURES = join(ROOT, 'fixtures');
const GPX = join(FIXTURES, 'gpx', 'snowdon.gpx');
const args = process.argv.slice(2);
const WEBGL = args.includes('--webgl');
const BROWSER = args.find((arg) => !arg.startsWith('--')) ?? 'chrome';
const TIMEOUT_MS = 60000;
const VIEWPORT = { width: 1000, height: 800, deviceScaleFactor: 1, mobile: false };

for (const [file, build] of [['pkg/isotrail_web_bg.wasm', 'build.sh'], ['scene/pkg/isotrail_scene_bg.wasm', 'build-scene.sh']]) {
  if (!existsSync(join(WEB, file))) {
    console.error(`web/${file} is missing: build it first with sh web/${build}`);
    process.exit(1);
  }
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

// The services' answers, named as RecordedFetcher names them
const recorded = (dir, query, body = '') => {
  const hash = createHash('sha256').update(query).update('\n').update(body).digest('hex');
  return `${dir}-${hash.slice(0, 16)}`;
};
const TYPES = {
  '.html': 'text/html',
  '.js': 'text/javascript',
  '.mjs': 'text/javascript',
  '.wasm': 'application/wasm',
};

const seen = [];
const server = createServer(async (req, res) => {
  const chunks = [];
  for await (const chunk of req) chunks.push(chunk);
  const body = Buffer.concat(chunks);
  seen.push({ method: req.method, url: req.url, body: body.toString('utf8') });
  const url = new URL(req.url, 'http://localhost');
  const path = decodeURIComponent(url.pathname);
  const query = url.search.slice(1);
  let file;
  let tile;
  if (path.startsWith('/web/')) {
    file = normalize(join(ROOT, path));
    if (!file.startsWith(WEB)) file = undefined;
  } else if ((tile = /^\/terrain\/(\d+)\/(\d+)\/(\d+)\.png$/.exec(path))) {
    file = join(FIXTURES, 'dem', 's3.amazonaws.com', 'elevation-tiles-prod', 'terrarium', ...tile.slice(1, 3), `${tile[3]}.png`);
  } else if (path === '/overpass/api/interpreter' && req.method === 'POST') {
    file = recorded(join(FIXTURES, 'osm', 'overpass-api.de', 'api', 'interpreter'), query, body);
  } else if (path === '/weather/v1/archive') {
    file = recorded(join(FIXTURES, 'weather', 'archive-api.open-meteo.com', 'v1', 'archive'), query);
  } else if (path === '/weather/v1/forecast') {
    file = recorded(join(FIXTURES, 'weather', 'api.open-meteo.com', 'v1', 'forecast'), query);
  }
  if (!file || !existsSync(file)) {
    res.writeHead(404).end();
    return;
  }
  const bytes = await readFile(file);
  const type = TYPES[extname(file)] ?? 'application/octet-stream';
  res.writeHead(200, { 'content-type': type, 'content-length': bytes.length }).end(bytes);
});
await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
const ORIGIN = `http://127.0.0.1:${server.address().port}`;
const SERVERS = {
  'terrain-url': `${ORIGIN}/terrain/{z}/{x}/{y}.png`,
  'overpass-url': `${ORIGIN}/overpass/api/interpreter`,
  'weather-url': `${ORIGIN}/weather`,
};

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
  server.close();
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

const results = [];
try {
  for (const [index, { name, flags, options }] of CASES.entries()) {
    const expected = await bundle(index, flags);
    asked.length = 0;
    seen.length = 0;
    await cdp.send('Page.navigate', { url: `${ORIGIN}/web/test/scene.html` });
    await cdp.waitFor("document.readyState === 'complete' && window.__scene?.state === 'idle'");
    const json = JSON.stringify({ detail: 32, ...SERVERS, ...options });
    await cdp.evaluate(`document.getElementById('options').value = ${JSON.stringify(json)}`);
    const { result } = await cdp.send('Runtime.evaluate', { expression: "document.getElementById('file')" });
    const started = Date.now();
    await cdp.send('DOM.setFileInputFiles', { files: [GPX], objectId: result.objectId });
    await cdp.waitFor("['built', 'failed'].includes(window.__scene.state)");
    const wall = Date.now() - started;
    const outcome = await cdp.evaluate(`(() => {
      const { state, error, warnings, passes, ms, scene } = window.__scene;
      let text = '';
      for (let i = 0; scene && i < scene.length; i += 0x8000) text += String.fromCharCode(...scene.subarray(i, i + 0x8000));
      return { state, error, warnings, passes, ms, scene: btoa(text) };
    })()`);
    const scene = Buffer.from(outcome.scene ?? '', 'base64');
    check(`${name}: the scene is built`, outcome.state === 'built', outcome.error);
    check(`${name}: it is byte for byte the one isotrail bundle makes`, scene.equals(expected), `${scene.length} against ${expected.length} bytes`);
    const ready = await cdp.waitFor("document.getElementById('hike').dataset.state === 'ready'");
    check(`${name}: the element shows it`, ready, await cdp.evaluate("document.getElementById('hike').dataset.state"));
    const elsewhere = asked.filter(({ url }) => !url.startsWith(`${ORIGIN}/`) && !/^(blob|data):/.test(url));
    check(`${name}: no request leaves for anywhere but the local server`, elsewhere.length === 0, elsewhere.map((r) => r.url).join(', '));
    const services = seen.filter(({ url }) => !url.startsWith('/web/'));
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
    results.push({ name, bytes: scene.length, gzipped: gzipSync(scene, { level: 9 }).length, passes: outcome.passes, worker_ms: Math.round(outcome.ms), wall_ms: wall, warnings: outcome.warnings, requests: services.length });
  }
  check('no script error on the page', cdp.exceptions.length === 0, cdp.exceptions.join(' | '));
} catch (error) {
  check('the check ran to the end', false, error.stack ?? String(error));
} finally {
  await quit(browser, cdp);
  server.close();
  await sleepFor(500);
  await rm(browser.profile, { recursive: true, force: true, maxRetries: 5 }).catch(() => {});
  await rm(temp, { recursive: true, force: true, maxRetries: 5 }).catch(() => {});
}

console.log('\nscene, bytes, gzipped, passes, worker ms, ms from the file, service requests, left out');
for (const r of results) {
  console.log(`${r.name}, ${r.bytes}, ${r.gzipped}, ${r.passes}, ${r.worker_ms}, ${r.wall_ms}, ${r.requests}, ${r.warnings.join(' ') || 'nothing'}`);
}
console.log(`\n${passed} passed, ${failures.length} failed (${BROWSER}${WEBGL ? ', WebGL2' : ''})`);
for (const failure of failures) console.log(`  ${failure}`);
process.exit(failures.length ? 1 : 0);
