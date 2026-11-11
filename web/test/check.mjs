// Headless browser check of the <isotrail-hike> element
//
//   node web/test/check.mjs [chrome|edge] [--webgl]
//
// Needs Node 22 or later, Chrome or Edge, cargo, and the runtime built into
// web/pkg (sh web/build.sh). --webgl forces every embed onto WebGL2, drawn by
// SwiftShader, so it also runs on machines without a GPU. ISOTRAIL_BROWSER
// overrides the browser's path. Exits with 1 if any check fails

import { spawn, spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { extname, join, normalize } from 'node:path';
import { fileURLToPath } from 'node:url';
import { crc32, deflateSync } from 'node:zlib';

const ROOT = fileURLToPath(new URL('../..', import.meta.url));
const WEB = join(ROOT, 'web');
const args = process.argv.slice(2);
const WEBGL = args.includes('--webgl');
const BROWSER = args.find((arg) => !arg.startsWith('--')) ?? 'chrome';
const TIMEOUT_MS = 60000;
const VIEWPORT = { width: 1000, height: 800, deviceScaleFactor: 1, mobile: false };

if (!existsSync(join(WEB, 'pkg', 'isotrail_web_bg.wasm'))) {
  console.error('web/pkg has no renderer: build it first with sh web/build.sh');
  process.exit(1);
}

// Synthetic bundles, and copies that are too old, too new or cut short
const data = await mkdtemp(join(tmpdir(), 'isotrail-check-'));
const made = spawnSync(
  'cargo',
  ['run', '--quiet', '-p', 'isotrail-testkit', '--example', 'web_check_bundles', '--', data],
  { cwd: ROOT, stdio: 'inherit' },
);
if (made.status !== 0) {
  console.error('could not make the test bundles with cargo');
  process.exit(1);
}
const hike = await readFile(join(data, 'hike.itb'));
const withVersion = (version) => {
  const bytes = Buffer.from(hike);
  bytes.writeUInt16LE(version, 4);
  return bytes;
};
await writeFile(join(data, 'older.itb'), withVersion(1));
await writeFile(join(data, 'newer.itb'), withVersion(999));
await writeFile(join(data, 'damaged.itb'), hike.subarray(0, 40));
await writeFile(join(data, 'poster.png'), png(160, 90, [58, 122, 90]));

// Pages, the web folder under /web/ and the bundles under /data/
const requests = [];
let failRuntimeOnce = false;
const TYPES = {
  '.html': 'text/html',
  '.js': 'text/javascript',
  '.wasm': 'application/wasm',
  '.png': 'image/png',
  '.itb': 'application/octet-stream',
};
const STRICT_CSP =
  "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self'; img-src 'self'; connect-src 'self'";
const pages = {};
const server = createServer(async (req, res) => {
  requests.push(req.url);
  const path = decodeURIComponent(new URL(req.url, 'http://localhost').pathname);
  if (pages[path]) {
    const headers = { 'content-type': 'text/html' };
    if (path === '/csp.html') headers['content-security-policy'] = STRICT_CSP;
    res.writeHead(200, headers);
    res.end(pages[path]);
    return;
  }
  if (path === '/web/pkg/isotrail_web.js' && failRuntimeOnce) {
    failRuntimeOnce = false;
    res.writeHead(503);
    res.end();
    return;
  }
  const file = path.startsWith('/web/')
    ? join(WEB, normalize(path.slice(5)))
    : path.startsWith('/data/')
      ? join(data, normalize(path.slice(6)))
      : null;
  try {
    if (!file) throw new Error('not found');
    const body = await readFile(file);
    res.writeHead(200, {
      'content-type': TYPES[extname(file)] ?? 'application/octet-stream',
      'cache-control': 'no-store',
    });
    res.end(body);
  } catch {
    res.writeHead(404);
    res.end();
  }
});
await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
const origin = `http://127.0.0.1:${server.address().port}`;

const backend = WEBGL ? ' backend="webgl"' : '';
const embed = (attrs) => `<isotrail-hike ${attrs}${backend}></isotrail-hike>`;
const grid = (items) =>
  `<div class="grid">${items.join('')}</div>`;
const page = (body, { head = '', script = '/web/isotrail-embed.js', style = true } = {}) =>
  `<!doctype html><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">` +
  (style
    ? '<style>body{margin:0;font:16px system-ui}main{max-width:900px;margin:auto}.grid{display:grid;grid-template-columns:repeat(3,1fr);gap:8px}</style>'
    : '') +
  `${head}<script type="module" src="${script}"></script><main>${body}</main>`;
const snippetMarkup = (poster) =>
  `<isotrail-hike src="/data/hike.itb"${poster ? ' poster="/data/poster.png"' : ''} aspect="wide"${backend} style="display:block;aspect-ratio:16/9"><img src="/data/poster.png" alt="Diorama of a test hike" style="display:block;width:100%"></isotrail-hike>`;

Object.assign(pages, {
  '/ready.html': page(
    grid([
      embed('id="a" src="/data/hike.itb" poster="/data/poster.png" aspect="wide"'),
      embed('id="w" src="/data/weather.itb" aspect="wide"'),
      embed('id="n" src="/data/hike.itb" aspect="wide" credit="none"'),
    ]),
  ),
  '/lost.html': page(embed('id="a" src="/data/hike.itb" poster="/data/poster.png" aspect="wide"')),
  '/fail.html': page(
    grid([
      embed('id="e404" src="/data/missing.itb" aspect="wide"'),
      embed('id="p404" src="/data/missing.itb" poster="/data/poster.png" aspect="wide"'),
      embed('id="older" src="/data/older.itb" aspect="wide"'),
      embed('id="newer" src="/data/newer.itb" aspect="wide"'),
      embed('id="damaged" src="/data/damaged.itb" aspect="wide"'),
    ]),
  ),
  '/nogpu.html': page(embed('id="a" src="/data/hike.itb" aspect="wide"'), {
    head: '<script src="/web/test/no-gpu.js"></script>',
  }),
  '/runtime.html': page(embed('id="a" src="/data/hike.itb" aspect="wide"'), {
    head: '<script type="module" src="/web/test/break-draw.js"></script>',
  }),
  '/api.html': page(`<canvas id="c" width="320" height="180"></canvas>`, {
    script: `/web/test/api.js${WEBGL ? '?webgl' : ''}`,
  }),
  '/retry.html': page(embed('id="a" src="/data/hike.itb" aspect="wide"')),
  '/snippet.html': page(snippetMarkup(false)),
  '/noscript.html': page(snippetMarkup(true), { script: '/web/missing.js' }),
  '/versioned.html': page(embed('id="a" src="/data/hike.itb" aspect="wide"'), {
    script: '/web/isotrail-embed.js?v=test',
  }),
  '/csp.html': page(embed('id="a" src="/data/hike.itb" aspect="wide"'), { style: false }),
  '/touch.html': page(embed('id="a" src="/data/hike.itb" aspect="wide"')),
  '/many.html': page(
    Array.from({ length: 20 }, (_, i) =>
      `<isotrail-hike id="m${i}" src="/data/hike.itb" aspect="wide" backend="webgl"></isotrail-hike><p>gap</p>`,
    ).join(''),
  ),
});

// Counts GPU objects, records the element's events and CSP reports, and on
// WebGPU lets the check lose a device on purpose
const INSTRUMENT = `
  window.__events = [];
  for (const name of ['isotrail-ready', 'isotrail-error']) {
    addEventListener(name, (e) => __events.push({ name, id: e.target.id, ...e.detail }));
  }
  window.__csp = [];
  document.addEventListener('securitypolicyviolation', (e) => __csp.push(e.violatedDirective + ' ' + e.blockedURI));
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
  window.__devices = [];
  if (window.GPUDevice) {
    Object.defineProperty(GPUDevice.prototype, 'lost', {
      get() { return this.__lost ??= new Promise((resolve) => { this.__loseNow = resolve; }); },
    });
    const request = GPUAdapter.prototype.requestDevice;
    GPUAdapter.prototype.requestDevice = async function (d) {
      const device = await request.call(this, d);
      __devices.push(device);
      return device;
    };
  }
`;

const failures = [];
let passed = 0;
function check(name, ok, detail = '') {
  if (ok) passed++;
  else failures.push(`${name}${detail ? `: ${detail}` : ''}`);
  console.log(`${ok ? 'ok  ' : 'FAIL'} ${name}${!ok && detail ? ` (${detail})` : ''}`);
}

const browser = await launch();
const cdp = await connect(browser.port);
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
await cdp.send('Runtime.enable');
await cdp.send('Page.enable');
await cdp.send('Page.addScriptToEvaluateOnNewDocument', { source: INSTRUMENT });

const el = (id) => `document.getElementById(${JSON.stringify(id)})`;
const stateOf = (id) => cdp.evaluate(`${el(id)}?.dataset.state`);
const errorsOf = (id) => cdp.evaluate(`__events.filter((e) => e.name === 'isotrail-error' && e.id === ${JSON.stringify(id)}).map((e) => e.reason)`);
const visible = (expression) =>
  cdp.evaluate(`(() => { const e = ${expression}; if (!e) return false; const r = e.getBoundingClientRect(); return r.width > 0 && r.height > 0 && e.checkVisibility({ opacityProperty: true, visibilityProperty: true }); })()`);

async function go(path, metrics = VIEWPORT) {
  await cdp.send('Emulation.setDeviceMetricsOverride', metrics);
  await cdp.send('Page.navigate', { url: origin + path });
  await cdp.waitFor(`document.readyState === 'complete'`);
  cdp.exceptions.length = 0;
}

async function loseGpu(id) {
  const kind = await cdp.evaluate(`__events.findLast((e) => e.name === 'isotrail-ready' && e.id === ${JSON.stringify(id)})?.backend`);
  if (kind === 'gl') {
    await cdp.evaluate(`${el(id)}.shadowRoot.querySelector('canvas').getContext('webgl2').getExtension('WEBGL_lose_context').loseContext()`);
  } else {
    await cdp.evaluate(`__devices.forEach((d) => { if (!d.__gone) { d.__gone = true; d.__loseNow?.({ reason: 'unknown', message: 'lost on purpose' }); } })`);
  }
}

try {
  // Ready, the credit, the focus ring, and no GPU objects made while playing
  await go('/ready.html');
  const ready = await cdp.waitFor(`['a', 'w', 'n'].every((id) => document.getElementById(id)?.dataset.state === 'ready')`);
  check('three embeds become ready', ready, JSON.stringify(await cdp.evaluate(`['a','w','n'].map((id) => document.getElementById(id).dataset.state)`)));
  check(
    'each sends isotrail-ready with its backend',
    await cdp.evaluate(`['a','w','n'].every((id) => __events.some((e) => e.name === 'isotrail-ready' && e.id === id && e.backend))`),
  );
  console.log(`     drawing with ${await cdp.evaluate(`__events.find((e) => e.name === 'isotrail-ready')?.backend`)}`);
  if (!WEBGL) {
    check('without --webgl, the browser offers WebGPU', await cdp.evaluate(`__events.every((e) => e.backend !== 'gl')`));
  }
  const credit = (id) => `${el(id)}.shadowRoot.querySelector('[part="credit"]')`;
  check('the credit line shows', await visible(credit('a')));
  check(
    'the credit sits inside the element',
    await cdp.evaluate(`(() => { const c = ${credit('a')}.getBoundingClientRect(); const h = ${el('a')}.getBoundingClientRect(); return c.left >= h.left && c.right <= h.right && c.top >= h.top && c.bottom <= h.bottom; })()`),
  );
  const creditA = await cdp.evaluate(`${credit('a')}.textContent`);
  check('the credit names the bundle\'s sources', /Synthetic contributors/.test(creditA) && /Made-up Tiles/.test(creditA), creditA);
  check('no weather credit without weather', !/Open-Meteo/.test(creditA), creditA);
  const creditW = await cdp.evaluate(`${credit('w')}.textContent`);
  check('a bundle with weather credits Open-Meteo', /Open-Meteo\.com/.test(creditW), creditW);
  check(
    'OpenStreetMap and Open-Meteo link to their pages',
    await cdp.evaluate(`['https://www.openstreetmap.org/copyright', 'https://open-meteo.com/'].every((href) => ${credit('w')}.querySelector('a[href="' + href + '"]'))`),
  );
  check('credit="none" hides the line', !(await visible(credit('n'))));

  const edge = await cdp.evaluate(`(() => { const r = ${el('a')}.getBoundingClientRect(); return { x: r.x, y: r.y, width: r.width, height: 6, scale: 1 }; })()`);
  const before = await cdp.screenshot(edge);
  await cdp.evaluate(`${el('a')}.shadowRoot.querySelector('canvas').focus({ focusVisible: true })`);
  await cdp.send('Input.dispatchKeyEvent', { type: 'keyDown', key: 'Shift', code: 'ShiftLeft', windowsVirtualKeyCode: 16 });
  await cdp.send('Input.dispatchKeyEvent', { type: 'keyUp', key: 'Shift', code: 'ShiftLeft', windowsVirtualKeyCode: 16 });
  await sleep(300);
  check('the focus ring changes pixels', before !== (await cdp.screenshot(edge)));

  const madeBefore = await cdp.evaluate('__gpu.made');
  await cdp.evaluate(`${el('a')}.play()`);
  await sleep(4000);
  const madeAfter = await cdp.evaluate('__gpu.made');
  check('playing creates no GPU objects', madeAfter === madeBefore, `${madeBefore} -> ${madeAfter}`);
  check('the route is still playing after 4 s', await cdp.evaluate(`${el('a')}.playing`));
  check('no uncaught errors', cdp.exceptions.length === 0, cdp.exceptions.join(' | '));

  // A lost GPU: one restart, then the poster
  await go('/lost.html');
  await cdp.waitFor(`${el('a')}?.dataset.state === 'ready'`);
  await loseGpu('a');
  const restarted = await cdp.waitFor(
    `${el('a')}.dataset.state === 'ready' && __events.filter((e) => e.name === 'isotrail-ready').length === 2`,
  );
  check('a lost GPU restarts the diorama', restarted, await stateOf('a'));
  await loseGpu('a');
  const gaveUp = await cdp.waitFor(`${el('a')}.dataset.state === 'fallback'`);
  check('a second loss keeps the poster', gaveUp, await stateOf('a'));
  check('and reports reason gpu', JSON.stringify(await errorsOf('a')) === '["gpu"]', JSON.stringify(await errorsOf('a')));
  check('the poster shows', await visible(`${el('a')}.shadowRoot.querySelector('img')`));
  check('no canvas is left behind', await cdp.evaluate(`!${el('a')}.shadowRoot.querySelector('canvas')`));

  // Each failure: its message and its event
  await go('/fail.html');
  await cdp.waitFor(`['e404', 'p404', 'older', 'newer', 'damaged'].every((id) => document.getElementById(id).dataset.state === 'fallback')`);
  const message = (id) => `${el(id)}.shadowRoot.querySelector('.message')`;
  for (const [id, reason, text] of [
    ['e404', 'load', "This hike couldn't be loaded."],
    ['older', 'version', 'This hike was saved by a different version of isotrail.'],
    ['newer', 'version', 'This hike was saved by a different version of isotrail.'],
    ['damaged', 'damaged', "This hike couldn't be loaded."],
  ]) {
    const reasons = await errorsOf(id);
    check(`${id}: isotrail-error with reason ${reason}`, JSON.stringify(reasons) === JSON.stringify([reason]), JSON.stringify(reasons));
    const shown = await cdp.evaluate(`${message(id)}.textContent`);
    check(`${id}: says "${text}"`, shown === text && (await visible(message(id))), shown);
  }
  check('with a poster, the poster stays instead of a message', (await visible(`${el('p404')}.shadowRoot.querySelector('img')`)) && !(await visible(message('p404'))));

  await go('/nogpu.html');
  await cdp.waitFor(`${el('a')}?.dataset.state === 'fallback'`);
  check('no GPU: reason gpu', JSON.stringify(await errorsOf('a')) === '["gpu"]', JSON.stringify(await errorsOf('a')));
  check("no GPU: says the browser can't show it", (await cdp.evaluate(`${message('a')}.textContent`)) === "This browser can't show the 3D view.");

  await go('/runtime.html');
  await cdp.waitFor(`${el('a')}?.dataset.state === 'ready'`);
  await cdp.evaluate(`window.__breakDraw = true; ${el('a')}.play()`);
  const broke = await cdp.waitFor(`${el('a')}.dataset.state === 'fallback'`, 10000);
  check('an error in the renderer ends in the fallback', broke, await stateOf('a'));
  check('and reports reason runtime', JSON.stringify(await errorsOf('a')) === '["runtime"]', JSON.stringify(await errorsOf('a')));
  check('without an uncaught error', cdp.exceptions.length === 0, cdp.exceptions.join(' | '));

  // The JavaScript API ignores numbers that aren't finite
  await go('/api.html');
  await cdp.waitFor('window.__api');
  const api = await cdp.evaluate('window.__api');
  check('the API ignores NaN and infinite arguments', api.zoomBefore === 1 && api.zoomAfter === 2, JSON.stringify(api));
  check('Viewer.load reports why it failed', api.olderReason === 'version' && api.damagedReason === 'damaged', JSON.stringify(api));
  check('Viewer.credit reads the bundle', /Synthetic contributors/.test(api.credit), api.credit);

  // A failed download of the renderer is tried again
  failRuntimeOnce = true;
  await go('/retry.html');
  await cdp.waitFor(`${el('a')}?.dataset.state === 'fallback'`);
  check('a failed renderer download reports reason load', JSON.stringify(await errorsOf('a')) === '["load"]', JSON.stringify(await errorsOf('a')));
  await cdp.evaluate(`(() => { const h = ${el('a')}; const parent = h.parentNode; h.remove(); parent.append(h); })()`);
  check('putting the element back loads it', await cdp.waitFor(`${el('a')}.dataset.state === 'ready'`), await stateOf('a'));

  // The snippet's poster, with and without the script
  await go('/noscript.html');
  await sleep(500);
  const bare = await cdp.evaluate(`(() => { const h = document.querySelector('isotrail-hike'); const r = h.getBoundingClientRect(); const img = h.querySelector('img'); return { width: r.width, height: r.height, img: img.complete && img.naturalWidth > 0 && img.getBoundingClientRect().height > 0 }; })()`);
  check('without the script, the element keeps its size', Math.abs(bare.height - (bare.width * 9) / 16) < 1, JSON.stringify(bare));
  check('without the script, the poster shows', bare.img, JSON.stringify(bare));
  await go('/snippet.html');
  const posterSrc = await cdp.evaluate(`document.querySelector('isotrail-hike').shadowRoot.querySelector('img').src`);
  check('the image inside the element becomes the poster', posterSrc.endsWith('/data/poster.png'), posterSrc);
  await cdp.waitFor(`document.querySelector('isotrail-hike').dataset.state === 'ready'`);
  const sized = await cdp.evaluate(`document.querySelector('isotrail-hike').getBoundingClientRect().height`);
  check('and the element keeps the same height once running', Math.abs(sized - bare.height) < 1, `${bare.height} -> ${sized}`);

  // The script's query reaches the renderer's files
  requests.length = 0;
  await go('/versioned.html');
  await cdp.waitFor(`${el('a')}?.dataset.state === 'ready'`);
  check(
    'the script passes ?v= on to pkg/',
    requests.includes('/web/pkg/isotrail_web.js?v=test') && requests.includes('/web/pkg/isotrail_web_bg.wasm?v=test'),
    requests.filter((url) => url.includes('/pkg/')).join(', '),
  );

  // A strict Content-Security-Policy, as the README lists it
  await go('/csp.html');
  const cspReady = await cdp.waitFor(`${el('a')}?.dataset.state === 'ready'`);
  check('ready under the README\'s Content-Security-Policy', cspReady, await stateOf('a'));
  const violations = await cdp.evaluate('__csp');
  check('with no CSP violations', violations.length === 0, violations.join(' | '));

  // Twenty WebGL2 embeds: none left blank after scrolling through them
  await go('/many.html');
  const height = await cdp.evaluate('document.documentElement.scrollHeight');
  for (let y = 0; y <= height; y += 400) {
    await cdp.evaluate(`scrollTo(0, ${y})`);
    await sleep(400);
  }
  for (let y = height; y >= 0; y -= 400) {
    await cdp.evaluate(`scrollTo(0, ${y})`);
    await sleep(400);
  }
  await cdp.evaluate('scrollTo(0, 0)');
  const blank = `[...document.querySelectorAll('isotrail-hike')].filter((h) => h.dataset.state === 'ready' && h.shadowRoot.querySelector('canvas')?.getContext('webgl2')?.isContextLost())`;
  const inView = `[...document.querySelectorAll('isotrail-hike')].filter((h) => { const r = h.getBoundingClientRect(); return r.bottom > 0 && r.top < innerHeight; })`;
  const settled = await cdp.waitFor(`${inView}.every((h) => h.dataset.state === 'ready')`);
  check('the embeds in view are ready after scrolling', settled, JSON.stringify(await cdp.evaluate(`${inView}.map((h) => h.dataset.state)`)));
  check('no ready embed has a lost context', (await cdp.evaluate(`${blank}.length`)) === 0);
  await sleep(4000);
  const live = await cdp.evaluate(`[...document.querySelectorAll('isotrail-hike')].filter((h) => h.dataset.state === 'ready').length`);
  check('embeds far off screen give their context back', live <= 6, `${live} still ready`);
  check('no failures among the twenty', (await cdp.evaluate(`__events.filter((e) => e.name === 'isotrail-error').length`)) === 0);

  // Finger-sized targets
  await cdp.send('Emulation.setTouchEmulationEnabled', { enabled: true, maxTouchPoints: 5 });
  await go('/touch.html', { width: 390, height: 844, deviceScaleFactor: 3, mobile: true });
  await cdp.waitFor(`${el('a')}?.dataset.state === 'ready'`);
  check('touch emulation matches pointer: coarse', await cdp.evaluate(`matchMedia('(pointer: coarse)').matches`));
  const targets = await cdp.evaluate(`(() => {
    const root = ${el('a')}.shadowRoot;
    return [...root.querySelectorAll('button')].map((b) => {
      const r = b.getBoundingClientRect();
      const corners = [[r.left + 1, r.top + 1], [r.right - 1, r.top + 1], [r.left + 1, r.bottom - 1], [r.right - 1, r.bottom - 1]];
      return { name: b.getAttribute('part'), width: r.width, height: r.height, hit: corners.every(([x, y]) => b.contains(root.elementFromPoint(x, y))) };
    });
  })()`);
  check('buttons have 44-pixel hit boxes', targets.every((t) => t.width >= 44 && t.height >= 44 && t.hit), JSON.stringify(targets));
  const scrubber = await cdp.evaluate(`${el('a')}.shadowRoot.querySelector('input').getBoundingClientRect().height`);
  check('the scrubber is 24 pixels high to touch', scrubber >= 24, String(scrubber));
  await cdp.send('Emulation.setTouchEmulationEnabled', { enabled: false });
} catch (error) {
  check('the check ran to the end', false, error.stack ?? String(error));
} finally {
  cdp.close();
  browser.process.kill();
  server.close();
  await sleep(500);
  await rm(browser.profile, { recursive: true, force: true, maxRetries: 5 }).catch(() => {});
  await rm(data, { recursive: true, force: true }).catch(() => {});
}

console.log(`\n${passed} passed, ${failures.length} failed (${BROWSER}${WEBGL ? ', WebGL2' : ''})`);
for (const failure of failures) console.log(`  ${failure}`);
process.exit(failures.length ? 1 : 0);

function browserPath() {
  if (process.env.ISOTRAIL_BROWSER) return process.env.ISOTRAIL_BROWSER;
  const candidates = {
    win32: {
      chrome: [
        'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe',
        `${process.env.LOCALAPPDATA}\\Google\\Chrome\\Application\\chrome.exe`,
      ],
      edge: ['C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe'],
    },
    darwin: {
      chrome: ['/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'],
      edge: ['/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge'],
    },
    linux: {
      chrome: ['/usr/bin/google-chrome', '/usr/bin/google-chrome-stable', '/usr/bin/chromium', '/usr/bin/chromium-browser'],
      edge: ['/usr/bin/microsoft-edge', '/usr/bin/microsoft-edge-stable'],
    },
  }[process.platform]?.[BROWSER] ?? [];
  const found = candidates.find((path) => existsSync(path));
  if (!found) {
    console.error(`could not find ${BROWSER}: set ISOTRAIL_BROWSER to its path`);
    process.exit(1);
  }
  return found;
}

async function launch() {
  const profile = await mkdtemp(join(tmpdir(), 'isotrail-browser-'));
  const flags = [
    '--headless=new',
    '--remote-debugging-port=0',
    `--user-data-dir=${profile}`,
    '--no-first-run',
    '--no-default-browser-check',
    `--window-size=${VIEWPORT.width},${VIEWPORT.height}`,
    ...(WEBGL ? ['--use-angle=swiftshader', '--enable-unsafe-swiftshader'] : ['--enable-unsafe-webgpu']),
    ...(process.platform === 'linux' ? ['--no-sandbox'] : []),
    'about:blank',
  ];
  const child = spawn(browserPath(), flags, { stdio: 'ignore' });
  const portFile = join(profile, 'DevToolsActivePort');
  for (let i = 0; i < 150 && !existsSync(portFile); i++) await sleepFor(100);
  const port = Number((await readFile(portFile, 'utf8')).split('\n')[0]);
  return { process: child, profile, port };
}

function sleepFor(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function connect(port) {
  let target;
  for (let i = 0; i < 50 && !target; i++) {
    try {
      const list = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
      target = list.find((t) => t.type === 'page');
    } catch {
      // not listening yet
    }
    if (!target) await sleepFor(100);
  }
  const ws = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((resolve, reject) => {
    ws.addEventListener('open', resolve);
    ws.addEventListener('error', reject);
  });
  let next = 1;
  const pending = new Map();
  const exceptions = [];
  ws.addEventListener('message', ({ data: text }) => {
    const message = JSON.parse(text);
    if (message.id && pending.has(message.id)) {
      pending.get(message.id)(message);
      pending.delete(message.id);
    } else if (message.method === 'Runtime.exceptionThrown') {
      const details = message.params.exceptionDetails;
      exceptions.push((details.exception?.description ?? details.text).slice(0, 300));
    }
  });
  const send = (method, params = {}) =>
    new Promise((resolve, reject) => {
      const id = next++;
      pending.set(id, (message) =>
        message.error ? reject(new Error(`${method}: ${message.error.message}`)) : resolve(message.result),
      );
      ws.send(JSON.stringify({ id, method, params }));
    });
  const evaluate = async (expression) => {
    const result = await send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
    if (result.exceptionDetails) {
      throw new Error(result.exceptionDetails.exception?.description ?? result.exceptionDetails.text);
    }
    return result.result.value;
  };
  const waitFor = async (expression, ms = TIMEOUT_MS) => {
    const end = Date.now() + ms;
    while (Date.now() < end) {
      if (await evaluate(expression).catch(() => false)) return true;
      await sleepFor(100);
    }
    return false;
  };
  const screenshot = async (clip) => (await send('Page.captureScreenshot', { format: 'png', clip })).data;
  return { send, evaluate, waitFor, screenshot, exceptions, close: () => ws.close() };
}

// A plain RGB image, for the poster
function png(width, height, [r, g, b]) {
  const row = width * 3 + 1;
  const raw = Buffer.alloc(row * height);
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) raw.set([r, g, b], y * row + 1 + x * 3);
  }
  const chunk = (type, body) => {
    const length = Buffer.alloc(4);
    length.writeUInt32BE(body.length);
    const typed = Buffer.concat([Buffer.from(type), body]);
    const crc = Buffer.alloc(4);
    crc.writeUInt32BE(crc32(typed));
    return Buffer.concat([length, typed, crc]);
  };
  const header = Buffer.alloc(13);
  header.writeUInt32BE(width, 0);
  header.writeUInt32BE(height, 4);
  header.set([8, 2, 0, 0, 0], 8);
  return Buffer.concat([
    Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]),
    chunk('IHDR', header),
    chunk('IDAT', deflateSync(raw)),
    chunk('IEND', Buffer.alloc(0)),
  ]);
}
