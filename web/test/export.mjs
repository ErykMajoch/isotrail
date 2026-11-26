// Headless browser check of reading stills and animation frames back in the
// web app's renderer
//
//   node web/test/export.mjs [chrome|edge] [--webgl | --webgl-gpu]
//
// Needs what web/test/check.mjs needs, with the web app's renderer built into
// web/app/pkg (sh web/build-app.sh) in place of web/pkg. It reads stills and
// frames in the page and compares them with the ones isotrail draws natively
// through the graphics API the browser uses (Direct3D 12 on Windows). On
// WebGPU they must be the same, byte for byte. --webgl draws on WebGL2 with
// SwiftShader, compared with native drawing in software, and --webgl-gpu on
// WebGL2 with the graphics card. WebGL2 has no native twin, so up to 0.5% of
// the pixels may differ there, as between the goldens of two graphics cards
// (the check prints the count). It also prints how long each read takes,
// and exits with 1 if any check fails

import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { extname, join, normalize } from 'node:path';
import { fileURLToPath } from 'node:url';

import { connect, launch, quit, sleepFor } from './browser.mjs';
import { decodePng } from './png.mjs';

const ROOT = fileURLToPath(new URL('../..', import.meta.url));
const APP = join(ROOT, 'web', 'app', 'pkg');
const args = process.argv.slice(2);
const SWIFTSHADER = args.includes('--webgl');
const WEBGL = SWIFTSHADER || args.includes('--webgl-gpu');
// The share of pixels that may differ from the native ones
const ALLOWED = WEBGL ? 0.005 : 0;
const BROWSER = args.find((arg) => !arg.startsWith('--')) ?? 'chrome';
const TIMEOUT_MS = 60000;
const VIEWPORT = { width: 1000, height: 800, deviceScaleFactor: 1, mobile: false };
const SIZE = { width: 640, height: 360 };
const BACKGROUNDS = { sky: 'sky', flat: '#1e2a33', transparent: 'transparent' };
const SAMPLED = [0, 75, 150, 225, 299];

if (!existsSync(join(APP, 'isotrail_web_bg.wasm'))) {
  console.error('web/app/pkg has no renderer: build it first with sh web/build-app.sh');
  process.exit(1);
}

const data = await mkdtemp(join(tmpdir(), 'isotrail-export-'));
// Chrome and Edge draw through Direct3D on Windows, where isotrail would pick
// Vulkan. SwiftShader draws in software, so the native side does too
const env = SWIFTSHADER
  ? { ...process.env, ISOTRAIL_GPU: 'software' }
  : process.platform === 'win32'
    ? { ...process.env, WGPU_BACKEND: 'dx12' }
    : process.env;
const cargo = (what, list) => {
  const run = spawnSync('cargo', ['run', '--quiet', ...list], { cwd: ROOT, stdio: 'inherit', env });
  if (run.status !== 0) {
    console.error(`could not ${what} with cargo`);
    process.exit(1);
  }
};
cargo('make the test bundles', ['-p', 'isotrail-testkit', '--example', 'web_check_bundles', '--', data]);
for (const [name, background] of Object.entries(BACKGROUNDS)) {
  cargo(`render the ${name} still`, [
    '-p', 'isotrail-cli', '--', 'render', join(data, 'hike.itb'), '-o', join(data, `still-${name}.png`),
    '--size', `${SIZE.width}x${SIZE.height}`, '--scale', '1', '--overlay', 'none',
    '--background', background, '--quiet',
  ]);
}
cargo('render the animation frames', [
  '-p', 'isotrail-export', '--example', 'clip_frames', '--',
  join(data, 'timed.itb'), `${SIZE.width}x${SIZE.height}`, join(data, 'frames'), SAMPLED.join(','),
]);

const TYPES = { '.html': 'text/html', '.js': 'text/javascript', '.wasm': 'application/wasm' };
const PAGE = `<!doctype html><meta charset="utf-8"><script type="module">
import init, { Viewer } from '/app/isotrail_web.js';
const { width, height } = ${JSON.stringify(SIZE)};
const base64 = (bytes) => {
  let text = '';
  for (let i = 0; i < bytes.length; i += 0x8000) text += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(text);
};
let viewer;
window.openHike = async (file, backend) => {
  await init();
  viewer?.free();
  const bytes = new Uint8Array(await (await fetch('/data/' + file)).arrayBuffer());
  const canvas = Object.assign(document.createElement('canvas'), { width, height });
  document.body.replaceChildren(canvas);
  viewer = await Viewer.load(canvas, bytes, backend, {});
  viewer.resize(width, height);
  viewer.draw();
  return viewer.backend;
};
window.still = async (background) => {
  viewer.setOptions({ background });
  const start = performance.now();
  const still = viewer.still(width, height);
  const drawn = performance.now();
  const rgba = await still.read();
  const read = performance.now();
  still.free();
  viewer.draw();
  return { draw: drawn - start, read: read - drawn, rgba: base64(rgba) };
};
window.clip = async (sampled) => {
  const tasks = [];
  const observer = new PerformanceObserver((list) => tasks.push(...list.getEntries().map((e) => e.duration)));
  try { observer.observe({ type: 'longtask' }); } catch {}
  const clip = viewer.clip(width, height);
  const frames = clip.frames;
  const times = [];
  const kept = {};
  let madeAtFirst = 0;
  const start = performance.now();
  for (let i = 0; ; i++) {
    const before = performance.now();
    const rgba = await clip.next();
    if (!rgba) break;
    times.push(performance.now() - before);
    if (i === 0) madeAtFirst = __gpu.made;
    if (sampled.includes(i)) kept[i] = base64(rgba);
    // The live view keeps drawing between reads
    if (i % 10 === 0) viewer.draw();
  }
  const total = performance.now() - start;
  clip.free();
  await new Promise((resolve) => setTimeout(resolve, 100));
  observer.disconnect();
  return { frames, times, total, made: __gpu.made - madeAtFirst, kept, longest: Math.max(0, ...tasks) };
};
window.overlap = async () => {
  const clip = viewer.clip(width, height, 1, 2);
  const first = clip.next();
  const second = await clip.next().then(() => '', (error) => error.message);
  await first;
  const after = await clip.next().then((rgba) => rgba?.length ?? 0);
  clip.free();
  return { second, after };
};
window.tooLarge = () => {
  try { viewer.still(100000, 10).free(); return ''; } catch (error) { return error.reason + ': ' + error.message; }
};
window.ready = true;
</script>`;

const INSTRUMENT = `
  window.__gpu = { made: 0 };
  const count = (proto, names) => {
    if (!proto) return;
    for (const name of names) {
      const original = proto[name];
      proto[name] = function (...a) { __gpu.made++; return original.apply(this, a); };
    }
  };
  count(window.GPUDevice?.prototype, ['createTexture', 'createBuffer']);
  count(window.WebGL2RenderingContext?.prototype, ['createTexture', 'createBuffer', 'createFramebuffer']);
`;

const server = createServer(async (req, res) => {
  const path = decodeURIComponent(new URL(req.url, 'http://localhost').pathname);
  if (path === '/') {
    res.writeHead(200, { 'content-type': 'text/html' });
    res.end(PAGE);
    return;
  }
  const file = path.startsWith('/app/')
    ? join(APP, normalize(path.slice(5)))
    : path.startsWith('/data/')
      ? join(data, normalize(path.slice(6)))
      : null;
  try {
    if (!file) throw new Error('not found');
    const body = await readFile(file);
    res.writeHead(200, { 'content-type': TYPES[extname(file)] ?? 'application/octet-stream', 'cache-control': 'no-store' });
    res.end(body);
  } catch {
    res.writeHead(404);
    res.end();
  }
});
await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
const origin = `http://127.0.0.1:${server.address().port}`;

const failures = [];
let passed = 0;
function check(name, ok, detail = '') {
  if (ok) passed++;
  else failures.push(`${name}${detail ? `: ${detail}` : ''}`);
  console.log(`${ok ? 'ok  ' : 'FAIL'} ${name}${!ok && detail ? ` (${detail})` : ''}`);
}

// How many pixels of two RGBA images of the same size differ, and whether
// that is within what's allowed
function compare(rgba, native) {
  if (rgba.length !== native.length) return { ok: false, detail: `${rgba.length} bytes, expected ${native.length}` };
  let pixels = 0;
  for (let i = 0; i < rgba.length; i += 4) {
    if (rgba[i] !== native[i] || rgba[i + 1] !== native[i + 1] || rgba[i + 2] !== native[i + 2] || rgba[i + 3] !== native[i + 3]) pixels++;
  }
  const detail = `${pixels} of ${rgba.length / 4} pixels differ`;
  if (pixels && ALLOWED) console.log(`     ${detail}`);
  return { ok: pixels <= ALLOWED * (rgba.length / 4), detail };
}
const same = ALLOWED ? 'is the native one, within 0.5%' : 'is the native one';
const ms = (value) => `${value.toFixed(1)}ms`;
const quantile = (sorted, share) => sorted[Math.min(sorted.length - 1, Math.floor(share * sorted.length))];

const browser = await launch(BROWSER, { webgl: SWIFTSHADER, viewport: VIEWPORT }).catch(async (error) => {
  console.error(error.message);
  await rm(data, { recursive: true, force: true }).catch(() => {});
  process.exit(1);
});
const cdp = await connect(browser.port, TIMEOUT_MS);
await cdp.send('Runtime.enable');
await cdp.send('Page.enable');
await cdp.send('Page.addScriptToEvaluateOnNewDocument', { source: INSTRUMENT });
const backend = JSON.stringify(WEBGL ? 'webgl' : 'auto');

try {
  await cdp.send('Emulation.setDeviceMetricsOverride', VIEWPORT);
  await cdp.send('Page.navigate', { url: `${origin}/` });
  check('the page loads the web app\'s renderer', await cdp.waitFor('window.ready === true'));
  const used = await cdp.evaluate(`openHike('hike.itb', ${backend})`);
  console.log(`     drawing with ${used}`);
  check('the backend is the one asked for', WEBGL ? used === 'gl' : used !== '', used);

  for (const [name, background] of Object.entries(BACKGROUNDS)) {
    const still = await cdp.evaluate(`still(${JSON.stringify(background)})`);
    const native = decodePng(await readFile(join(data, `still-${name}.png`)));
    const { ok, detail } = compare(Buffer.from(still.rgba, 'base64'), native.rgba);
    check(`the ${name} still read in the browser ${same}`, ok, detail);
    console.log(`     ${name} still: drawn in ${ms(still.draw)}, read in ${ms(still.read)}`);
  }
  check('a still too large for the GPU is refused', (await cdp.evaluate('tooLarge()')).startsWith('gpu: '));

  await cdp.evaluate(`openHike('timed.itb', ${backend})`);
  const clip = await cdp.evaluate(`clip(${JSON.stringify(SAMPLED)})`);
  check('the default animation has 300 frames', clip.frames === 300 && clip.times.length === 300, `${clip.frames}, read ${clip.times.length}`);
  for (const index of SAMPLED) {
    const native = decodePng(await readFile(join(data, 'frames', `frame-${index}.png`)));
    const { ok, detail } = clip.kept[index]
      ? compare(Buffer.from(clip.kept[index], 'base64'), native.rgba)
      : { ok: false, detail: 'not read' };
    check(`frame ${index} read in the browser ${same}`, ok, detail);
  }
  check('reading 300 frames makes no GPU objects after the first', clip.made === 0, `${clip.made} made`);
  const sorted = [...clip.times].sort((a, b) => a - b);
  console.log(
    `     300 frames in ${ms(clip.total)}: median ${ms(quantile(sorted, 0.5))}, 95th percentile ${ms(quantile(sorted, 0.95))}, slowest ${ms(sorted.at(-1))}; longest task ${ms(clip.longest)}`,
  );

  const overlap = await cdp.evaluate('overlap()');
  check('a second read before the first has finished is refused', overlap.second.includes('wait for it'), overlap.second);
  check('reading goes on after a refused read', overlap.after === SIZE.width * SIZE.height * 4, String(overlap.after));
  check('nothing threw in the page', cdp.exceptions.length === 0, cdp.exceptions.join(' | '));
} catch (error) {
  check('the check ran to the end', false, error.stack ?? String(error));
} finally {
  await quit(browser, cdp);
  server.close();
  await sleepFor(500);
  await rm(browser.profile, { recursive: true, force: true, maxRetries: 5 }).catch(() => {});
  await rm(data, { recursive: true, force: true }).catch(() => {});
}

console.log(`\n${passed} passed, ${failures.length} failed (${BROWSER}${SWIFTSHADER ? ', WebGL2 on SwiftShader' : WEBGL ? ', WebGL2' : ''})`);
for (const failure of failures) console.log(`  ${failure}`);
process.exit(failures.length ? 1 : 0);
