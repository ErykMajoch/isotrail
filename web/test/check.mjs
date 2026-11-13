// Headless browser check of the <isotrail-hike> element
//
//   node web/test/check.mjs [chrome|edge] [--webgl]
//
// Needs Node 22 or later, Chrome or Edge, cargo, a graphics card for one
// native still (ISOTRAIL_GPU=software uses a software one), and the runtime
// built into web/pkg (sh web/build.sh). --webgl forces every embed onto
// WebGL2, drawn by SwiftShader, so the browser needs no GPU. ISOTRAIL_BROWSER
// overrides the browser's path. Exits with 1 if any check fails

import { spawn, spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { basename, extname, join, normalize } from 'node:path';
import { fileURLToPath } from 'node:url';
import { crc32, deflateSync, inflateSync } from 'node:zlib';

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
// A still at the size a 640 x 360 element draws, which the live view starts from
const FIT = { width: 640, height: 360, page: [255, 0, 255] };
const posterMade = spawnSync(
  'cargo',
  [
    'run', '--quiet', '-p', 'isotrail-cli', '--', 'render', join(data, 'hike.itb'),
    '-o', join(data, 'still.png'), '--size', `${FIT.width}x${FIT.height}`, '--scale', '1',
    '--overlay', 'none', '--background', 'transparent', '--quiet',
  ],
  { cwd: ROOT, stdio: 'inherit' },
);
if (posterMade.status !== 0) {
  console.error('could not render the poster with cargo');
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
await writeFile(join(data, 'warm.hex'), '1a1c2c\n5d275d\nb13e53\nef7d57\nffcd75\na7f070\n');
await writeFile(
  join(data, 'loud.toml'),
  '[landcover]\ngrassland = "#e040c0"\nconiferous = "#f0e020"\n\n[route]\ncolour = "#2050e0"\n',
);
await writeFile(join(data, 'bad.toml'), '[landcover]\nlawn = "#e040c0"\n');

// Pages, the web folder under /web/ and the bundles under /data/
const requests = [];
let failRuntimeOnce = false;
const TYPES = {
  '.html': 'text/html',
  '.js': 'text/javascript',
  '.wasm': 'application/wasm',
  '.png': 'image/png',
  '.itb': 'application/octet-stream',
  '.hex': 'text/plain',
  '.toml': 'text/plain',
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
  '/look.html': page(
    grid([
      embed('id="d" src="/data/hike.itb" aspect="wide"'),
      embed('id="p" src="/data/hike.itb" aspect="wide" palette="aap-64"'),
      embed('id="u" src="/data/hike.itb" aspect="wide" palette="nope" god-rays="sometimes"'),
      embed('id="h" src="/data/hike.itb" aspect="wide" palette="/data/warm.hex"'),
      embed('id="t" src="/data/timed.itb" aspect="wide"'),
      embed('id="tn" src="/data/timed.itb" aspect="wide" time="22:30"'),
    ]),
    // Whole-pixel places, so equal frames give equal screenshots
    { head: '<style>.grid{grid-template-columns:repeat(3,288px)}</style>' },
  ),
  '/view.html': page(
    grid([
      embed('id="d" src="/data/hike.itb" aspect="wide"'),
      embed('id="v" src="/data/hike.itb" aspect="wide" turn="-45" pitch="55" zoom="1.5"'),
      embed('id="f" src="/data/hike.itb" aspect="wide" zoom="0.6"'),
      embed('id="b" src="/data/hike.itb" aspect="wide" pitch="5" zoom="big"'),
      embed('id="r" src="/data/hike.itb" aspect="3:2"'),
      embed('id="q" src="/data/hike.itb" aspect="portrait"'),
    ]),
    { head: '<style>.grid{grid-template-columns:repeat(3,288px)}</style>' },
  ),
  '/scene.html': page(
    grid([
      embed('id="d" src="/data/hike.itb" aspect="wide"'),
      embed('id="s" src="/data/hike.itb" aspect="wide" strata="off" prop-density="0" paths="off" base-blocks="2"'),
      embed('id="w" src="/data/hike.itb" aspect="wide" weather="fog" outlines="0" shadows="off"'),
      embed('id="r" src="/data/hike.itb" aspect="wide" route-colour="#2050e0" route-width="4" markers="none" credit-extra="Walked by A. Hiker"'),
      embed('id="u" src="/data/hike.itb" aspect="wide" paths="maybe" weather="sunny" route-colour="red" markers="flag"'),
      embed('id="n" src="/data/hike.itb" aspect="wide" sun="200,-20" weather-effects="none"'),
      embed('id="rc" src="/data/hike.itb" aspect="wide" route-color="#2050e0" route-width="4" markers="none" credit-extra="Walked by A. Hiker"'),
      embed('id="rb" src="/data/hike.itb" aspect="wide" route-colour="#2050e0" route-color="#00ff00" route-width="4" markers="none" credit-extra="Walked by A. Hiker"'),
      embed('id="ua" src="/data/hike.itb" aspect="wide" route-color="pink"'),
    ]),
    { head: '<style>.grid{grid-template-columns:repeat(3,288px)}</style>' },
  ),
  '/play.html': page(
    grid([
      embed('id="s" src="/data/hike.itb" aspect="wide" duration="1.5"'),
      embed('id="l2" src="/data/hike.itb" aspect="wide" duration="1" loop="2" rewind="0"'),
      embed('id="lf" src="/data/hike.itb" aspect="wide" duration="1" loop loop-back'),
      embed('id="o0" src="/data/hike.itb" aspect="wide" orbit="0" rewind="0"'),
      embed('id="od" src="/data/hike.itb" aspect="wide" rewind="0"'),
      embed('id="tw" src="/data/hike.itb" aspect="wide"'),
      embed('id="pt" src="/data/paced.itb" aspect="wide" pace="time"'),
      embed('id="pd" src="/data/paced.itb" aspect="wide"'),
      embed('id="pu" src="/data/hike.itb" aspect="wide" pace="time"'),
      embed('id="b" src="/data/hike.itb" aspect="wide" duration="0" orbit="400" loop="many" pace="walk" max-zoom="9" controls="play,map" rewind="20"'),
      embed('id="i" src="/data/hike.itb" aspect="wide" duration="2" intro="1" hold="0.8"'),
      embed('id="nj" src="/data/timed.itb" aspect="wide"'),
    ]),
    { head: '<style>.grid{grid-template-columns:repeat(3,288px)}</style>' },
  ),
  '/script.html': page(
    grid([
      embed('id="early" src="/data/hike.itb" aspect="wide" duration="2"'),
      embed('id="preset" src="/data/hike.itb" aspect="wide"'),
      embed('id="a" src="/data/hike.itb" aspect="wide"'),
      embed('id="bad" src="/data/missing.itb" aspect="wide"'),
    ]),
    {
      head:
        '<style>.grid{grid-template-columns:repeat(3,288px)}</style>' +
        `<script type="module">
          await customElements.whenDefined('isotrail-hike');
          const early = document.getElementById('early');
          early.play();
          document.getElementById('preset').progress = 0.25;
          window.__early = early.ready.then((detail) => detail.backend);
          window.__rejected = document.getElementById('bad').ready.then(() => 'resolved', (error) => error.reason);
        </script>`,
    },
  ),
  '/controls.html': page(
    grid([
      embed('id="d" src="/data/hike.itb" aspect="wide"'),
      embed('id="none" src="/data/hike.itb" aspect="wide" controls="none"'),
      embed('id="ps" src="/data/hike.itb" aspect="wide" controls="play,scrubber"'),
      embed('id="mz" src="/data/hike.itb" aspect="wide" max-zoom="6"'),
    ]),
    { head: '<style>.grid{grid-template-columns:repeat(3,288px)}</style>' },
  ),
  '/words.html': page(
    grid([
      embed('id="pl" src="/data/hike.itb" aspect="wide" lang="pl"'),
      `<div lang="pl-PL">${embed('id="inh" src="/data/hike.itb" aspect="wide"')}</div>`,
      embed('id="de" src="/data/hike.itb" aspect="wide" lang="de"'),
      embed(`id="st" src="/data/hike.itb" aspect="wide" strings='{"play":"Start","zoomIn":"Closer"}'`),
      embed(`id="bs" src="/data/hike.itb" aspect="wide" strings='{"play":1,"nope":"x"}'`),
      embed('id="bj" src="/data/hike.itb" aspect="wide" strings="not json"'),
      embed('id="plf" src="/data/missing.itb" aspect="wide" lang="pl"'),
    ]),
  ),
  '/theme.html': page(
    grid([
      embed('id="d" src="/data/hike.itb" aspect="wide"'),
      embed('id="t" src="/data/hike.itb" aspect="wide" theme="/data/loud.toml"'),
      embed('id="bt" src="/data/hike.itb" aspect="wide" theme="/data/bad.toml"'),
      embed('id="mt" src="/data/hike.itb" aspect="wide" theme="/data/missing.toml"'),
      embed('id="tr" src="/data/hike.itb" aspect="wide" theme="/data/loud.toml" route-colour="#e0a020"'),
    ]),
    { head: '<style>.grid{grid-template-columns:repeat(3,288px)}</style>' },
  ),
  '/sizes.html': page(
    grid([
      embed('id="abc" src="/data/hike.itb" aspect="abc"'),
      embed('id="obj" src="/data/hike.itb" aspect="constructor"'),
      embed('id="zero" src="/data/hike.itb" aspect="0:0"'),
      embed('id="ch" src="/data/hike.itb" aspect="wide" credit="hidden"'),
      '<isotrail-hike id="bm" src="/data/hike.itb" aspect="wide" backend="metal"></isotrail-hike>',
    ]) + `<div style="width:300px">${embed('id="tall" src="/data/hike.itb" aspect="1:1000000"')}</div>`,
    { head: '<style>.grid{grid-template-columns:repeat(3,288px)}</style>' },
  ),
  '/fit.html': page(
    embed(`id="f" src="/data/hike.itb" poster="/data/still.png" aspect="wide" background="transparent" controls="none" credit="none" style="display:block;width:${FIT.width}px"`),
    { head: `<style>body{background:rgb(${FIT.page.join(' ')})}</style>` },
  ),
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
  window.__warnings = [];
  const warn = console.warn;
  console.warn = function (...a) { __warnings.push(a.map(String).join(' ')); return warn.apply(this, a); };
  window.__errors = [];
  const error = console.error;
  console.error = function (...a) { __errors.push(a.map(String).join(' ')); return error.apply(this, a); };
  for (const name of ['isotrail-ready', 'isotrail-error', 'isotrail-play', 'isotrail-pause', 'isotrail-ended', 'isotrail-progress']) {
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
  check('the route is still playing after 4s', await cdp.evaluate(`${el('a')}.playing`));
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

  // Look attributes: a palette by name or .hex address, unknown values, a
  // change on a live element, and a time of day
  await go('/look.html');
  const looked = await cdp.waitFor(`['d', 'p', 'u', 'h', 't', 'tn'].every((id) => document.getElementById(id).dataset.state === 'ready')`);
  check('six look embeds become ready', looked, JSON.stringify(await cdp.evaluate(`['d','p','u','h','t','tn'].map((id) => document.getElementById(id).dataset.state)`)));
  await sleep(300);
  const shot = async (id) =>
    cdp.screenshot(await cdp.evaluate(`(() => { const r = ${el(id)}.shadowRoot.querySelector('canvas').getBoundingClientRect(); return { x: Math.round(r.x), y: Math.round(r.y), width: Math.floor(r.width) - 1, height: Math.floor(r.height) - 1, scale: 1 }; })()`));
  const plainShot = await shot('d');
  check('palette="aap-64" draws different pixels', plainShot !== (await shot('p')));
  check('a palette from a .hex address draws different pixels', ![plainShot, await shot('p')].includes(await shot('h')));
  check('unknown values draw the default', plainShot === (await shot('u')));
  const warnings = await cdp.evaluate('__warnings');
  check(
    'and each warns on the console',
    ['palette "nope"', 'god-rays "sometimes"'].every((part) => warnings.some((w) => w.includes(part))),
    warnings.join(' | '),
  );
  check('time="22:30" relights the diorama', (await shot('t')) !== (await shot('tn')));
  const paletteShot = await shot('p');
  await cdp.evaluate(`${el('d')}.setAttribute('palette', 'aap-64')`);
  let redrawn = false;
  for (let i = 0; i < 50 && !redrawn; i++) {
    await sleep(100);
    redrawn = (await shot('d')) === paletteShot;
  }
  check('changing the palette attribute redraws in it', redrawn);
  await cdp.evaluate(`${el('d')}.setAttribute('mesher', 'smooth')`);
  await sleep(1000);
  check('changing the mesher builds the terrain again', (await shot('d')) !== paletteShot && (await stateOf('d')) === 'ready');
  check('no uncaught errors on the look page', cdp.exceptions.length === 0, cdp.exceptions.join(' | '));

  // The starting view: turn, pitch and zoom attributes, bad values, reset,
  // and the new aspects
  await go('/view.html');
  const viewed = await cdp.waitFor(`['d', 'v', 'f', 'b', 'r', 'q'].every((id) => document.getElementById(id).dataset.state === 'ready')`);
  check('six view embeds become ready', viewed);
  await sleep(300);
  const defaultShot = await shot('d');
  const viewShot = await shot('v');
  check('turn, pitch and zoom change the starting view', viewShot !== defaultShot);
  check('zoom below 1 leaves more room round the slab', (await shot('f')) !== defaultShot);
  check('bad view values draw the default view', (await shot('b')) === defaultShot);
  const viewWarnings = await cdp.evaluate('__warnings');
  check(
    'and each warns on the console',
    ['pitch "5"', 'zoom "big"'].every((part) => viewWarnings.some((w) => w.includes(part))),
    viewWarnings.join(' | '),
  );
  const zoomOutDisabled = (id) => cdp.evaluate(`${el(id)}.shadowRoot.querySelector('[part=zoom-out]').getAttribute('aria-disabled')`);
  check('zoom="1.5" starts zoomed in', (await zoomOutDisabled('v')) === 'false' && (await zoomOutDisabled('d')) === 'true');
  await cdp.evaluate(`${el('v')}.shadowRoot.querySelector('[part=zoom-in]').click()`);
  await sleep(300);
  check('zooming in moves away from the starting view', (await shot('v')) !== viewShot);
  await cdp.evaluate(`${el('v')}.shadowRoot.querySelector('[part=reset]').click()`);
  let back = false;
  for (let i = 0; i < 30 && !back; i++) {
    await sleep(100);
    back = (await shot('v')) === viewShot;
  }
  check('reset returns to the starting view', back);
  await cdp.evaluate(`${el('d')}.setAttribute('pitch', '55')`);
  await sleep(1000);
  check('changing the pitch attribute redraws the view', (await shot('d')) !== defaultShot && (await stateOf('d')) === 'ready');
  // The grid stretches a row to its tallest element, so read the ratio set
  const ratio = (id) => cdp.evaluate(`${el(id)}.style.getPropertyValue('--isotrail-aspect')`);
  check('aspect="3:2" sets that ratio', (await ratio('r')) === '3 / 2', await ratio('r'));
  check('aspect="portrait" is 3:4', (await ratio('q')) === '3 / 4', await ratio('q'));
  check('no uncaught errors on the view page', cdp.exceptions.length === 0, cdp.exceptions.join(' | '));

  // Scene, light and route attributes, the extra credit line, bad values
  // and a change on a live element
  await go('/scene.html');
  const scened = await cdp.waitFor(`['d', 's', 'w', 'r', 'u', 'n', 'rc', 'rb', 'ua'].every((id) => document.getElementById(id).dataset.state === 'ready')`);
  check('nine scene embeds become ready', scened);
  await sleep(300);
  const sceneShot = await shot('d');
  check('strata, props, paths and base-blocks change the terrain', (await shot('s')) !== sceneShot);
  check('weather, outlines and shadows change the light', (await shot('w')) !== sceneShot);
  check('route-colour, route-width and markers change the route', (await shot('r')) !== sceneShot);
  check('a hand-set sun relights the diorama', (await shot('n')) !== sceneShot);
  check('route-color draws the same pixels as route-colour', (await shot('rc')) === (await shot('r')));
  check('with both, route-colour wins', (await shot('rb')) === (await shot('r')));
  check('a bad route-color draws the default', (await shot('ua')) === sceneShot);
  check('bad scene values draw the default', (await shot('u')) === sceneShot);
  const sceneWarnings = await cdp.evaluate('__warnings');
  check(
    'and each warns on the console',
    ['paths "maybe"', 'weather "sunny"', 'route-colour "red"', 'route-color "pink"', 'route-color "#00ff00", as route-colour is set too', 'markers "flag"'].every((part) => sceneWarnings.some((w) => w.includes(part))),
    sceneWarnings.join(' | '),
  );
  const extraOf = (id) => cdp.evaluate(`${el(id)}.shadowRoot.querySelector('[part="credit"] .extra')?.textContent ?? null`);
  check('credit-extra adds a line under the credit', (await extraOf('r')) === 'Walked by A. Hiker', String(await extraOf('r')));
  check('without it there is no extra line', (await extraOf('d')) === null);
  const routeOf = (id) => cdp.evaluate(`getComputedStyle(${el(id)}.shadowRoot.querySelector('input')).getPropertyValue('--route').trim()`);
  check('the progress line takes the route colour', (await routeOf('r')) === '#2050e0', await routeOf('r'));
  check('a bad route colour leaves the progress line in the default colour', (await routeOf('u')) === '#d64834', await routeOf('u'));
  check('the progress line takes route-color too', (await routeOf('rc')) === '#2050e0', await routeOf('rc'));
  check('and route-colour over route-color', (await routeOf('rb')) === '#2050e0', await routeOf('rb'));
  await cdp.evaluate(`${el('d')}.setAttribute('credit-extra', 'My site')`);
  check('changing credit-extra updates the line', (await extraOf('d')) === 'My site', String(await extraOf('d')));
  await cdp.evaluate(`${el('d')}.setAttribute('strata', 'off')`);
  await sleep(1000);
  check('changing a scene attribute builds the terrain again', (await shot('d')) !== sceneShot && (await stateOf('d')) === 'ready');
  check('no uncaught errors on the scene page', cdp.exceptions.length === 0, cdp.exceptions.join(' | '));

  // Playback: events in order, loops, the orbit, the pace and bad values
  const eventsOf = (id, names) =>
    cdp.evaluate(`__events.filter((e) => e.id === ${JSON.stringify(id)} && ${JSON.stringify(names)}.includes(e.name)).map((e) => e.name)`);
  const allReady = (ids) => cdp.waitFor(`${JSON.stringify(ids)}.every((id) => document.getElementById(id).dataset.state === 'ready')`);
  await go('/play.html');
  const playIds = ['s', 'l2', 'lf', 'o0', 'od', 'tw', 'pt', 'pd', 'pu', 'b', 'i', 'nj'];
  check('twelve playback embeds become ready', await allReady(playIds));
  await sleep(300);
  // Waits for events rather than a time: SwiftShader draws slowly, and a
  // slow frame moves the clip on by at most a tenth of a second
  const hasEvent = (id, name) => `__events.some((e) => e.id === ${JSON.stringify(id)} && e.name === ${JSON.stringify(name)})`;
  await cdp.evaluate(`['s', 'l2', 'lf'].forEach((id) => document.getElementById(id).play())`);
  const bothEnded = await cdp.waitFor(`${hasEvent('s', 'isotrail-ended')} && ${hasEvent('l2', 'isotrail-ended')}`);
  check('short plays end', bothEnded);
  const lfUndrew = `(() => { const p = __events.filter((e) => e.id === 'lf' && e.name === 'isotrail-progress').map((e) => e.progress); return p.some((v, i) => i > 0 && v < p[i - 1] && v > 0.05); })()`;
  await cdp.waitFor(lfUndrew);
  const order = (await eventsOf('s', ['isotrail-play', 'isotrail-progress', 'isotrail-pause', 'isotrail-ended'])).filter(
    (name, i, all) => name !== 'isotrail-progress' || all[i - 1] !== 'isotrail-progress',
  );
  check(
    'a play sends play, progress, pause and ended in order',
    JSON.stringify(order) === JSON.stringify(['isotrail-play', 'isotrail-progress', 'isotrail-pause', 'isotrail-ended']),
    order.join(', '),
  );
  const progressS = await cdp.evaluate(`__events.filter((e) => e.id === 's' && e.name === 'isotrail-progress').map((e) => e.progress)`);
  // A slow frame can step over the moment the route is empty
  const lowest = progressS.indexOf(Math.min(...progressS));
  check(
    'a play first undraws the route, then draws it from 0 to 1',
    lowest > 0 &&
      progressS[lowest] < 0.05 &&
      progressS.slice(0, lowest + 1).every((p, i) => i === 0 || p <= progressS[i - 1]) &&
      progressS.slice(lowest).every((p, i, rest) => i === 0 || p >= rest[i - 1]) &&
      progressS.at(-1) === 1,
    progressS.slice(0, 3).concat(progressS.slice(-2)).join(', '),
  );
  const firstL2 = await cdp.evaluate(`__events.find((e) => e.id === 'l2' && e.name === 'isotrail-progress')?.progress`);
  check('rewind="0" starts the route from nothing at once', firstL2 === 0, String(firstL2));
  check('an ended play stops on the finished hike', !(await cdp.evaluate(`${el('s')}.playing`)) && (await cdp.evaluate(`${el('s')}.progress`)) === 1);
  check('loop="2" plays twice, then ends', JSON.stringify(await eventsOf('l2', ['isotrail-ended'])) === '["isotrail-ended"]' && !(await cdp.evaluate(`${el('l2')}.playing`)));
  const l2Progress = await cdp.evaluate(`__events.filter((e) => e.id === 'l2' && e.name === 'isotrail-progress').map((e) => e.progress)`);
  check('and starts the second play from the start', l2Progress.filter((p, i) => i > 0 && p < l2Progress[i - 1]).length === 1, l2Progress.join(','));
  check('loop keeps playing past its duration', await cdp.evaluate(`${el('lf')}.playing`));
  const lfProgress = await cdp.evaluate(`__events.filter((e) => e.id === 'lf' && e.name === 'isotrail-progress').map((e) => e.progress)`);
  check('loop-back undraws the route before the next play', lfProgress.some((p, i) => i > 0 && p < lfProgress[i - 1] && p > 0.05), lfProgress.slice(0, 40).join(','));
  await cdp.evaluate(`${el('lf')}.pause()`);
  check('pause() stops a loop and sends pause', !(await cdp.evaluate(`${el('lf')}.playing`)) && (await eventsOf('lf', ['isotrail-pause'])).length === 1);

  // A paused frame with no orbit matches a twin set to the same progress,
  // and one with the default orbit doesn't
  await cdp.evaluate(`['o0', 'od'].forEach((id) => document.getElementById(id).play())`);
  await cdp.waitFor(`['o0', 'od'].every((id) => document.getElementById(id).progress > 0.1)`);
  await cdp.evaluate(`['o0', 'od'].forEach((id) => document.getElementById(id).pause())`);
  await sleep(300);
  const twinShot = async (id) => {
    await cdp.evaluate(`${el('tw')}.progress = ${el(id)}.progress`);
    await sleep(300);
    return shot('tw');
  };
  const o0Progress = await cdp.evaluate(`${el('o0')}.progress`);
  check('the paused embeds are part-way', o0Progress > 0 && o0Progress < 1, String(o0Progress));
  check('orbit="0" plays without turning', (await shot('o0')) === (await twinShot('o0')));
  check('the default orbit turns while it plays', (await shot('od')) !== (await twinShot('od')));
  await cdp.evaluate(`['pt', 'pd'].forEach((id) => { document.getElementById(id).progress = 0.5; })`);
  await sleep(400);
  check('pace="time" follows the recorded times', (await shot('pt')) !== (await shot('pd')));
  const playWarnings = await cdp.evaluate('__warnings');
  check(
    'bad playback values warn on the console',
    ['duration "0"', 'orbit "400"', 'loop "many"', 'pace "walk"', 'max-zoom "9"', 'controls "map"', 'rewind "20"', 'ignored intro and hold', 'paces by distance'].every((part) =>
      playWarnings.some((w) => w.includes(part)),
    ),
    playWarnings.join(' | '),
  );
  check('and the bad embed keeps every control', (await visible(`${el('b')}.shadowRoot.querySelector('[part=play]')`)) && !(await visible(`${el('b')}.shadowRoot.querySelector('[part=reset]')`)));
  // Pressing play shows the same frame first: the route, the view and the
  // light of the poster, on a hike whose sun moves
  const beforePlay = await shot('nj');
  await cdp.evaluate(`(() => { const e = ${el('nj')}; e.play(); e.pause(); })()`);
  await sleep(300);
  check('pressing play starts from the frame before it', (await shot('nj')) === beforePlay);
  await cdp.evaluate(`${el('nj')}.play()`);
  await cdp.waitFor(`${el('nj')}.progress < 0.9`);
  await cdp.evaluate(`${el('nj')}.pause()`);
  const undrawing = await cdp.evaluate(`${el('nj')}.progress`);
  check('then the route undraws', undrawing > 0 && undrawing < 0.9, String(undrawing));
  check('no uncaught errors on the playback page', cdp.exceptions.length === 0, cdp.exceptions.join(' | '));

  // The script API: play() and progress before ready, ready, progress and reset()
  await go('/script.html');
  check('four script embeds settle', await cdp.waitFor(`['early', 'preset', 'a'].every((id) => document.getElementById(id).dataset.state === 'ready') && ${el('bad')}.dataset.state === 'fallback'`));
  check('ready resolves with the backend', typeof (await cdp.evaluate('window.__early')) === 'string', String(await cdp.evaluate('window.__early')));
  check('ready rejects with the reason', (await cdp.evaluate('window.__rejected')) === 'load', String(await cdp.evaluate('window.__rejected')));
  check('play() before ready plays once ready', await cdp.waitFor(hasEvent('early', 'isotrail-play')));
  check('progress set before ready is kept', (await cdp.evaluate(`[${el('preset')}.progress, ${el('preset')}.shadowRoot.querySelector('input').value]`)).join() === '0.25,250');
  await cdp.evaluate(`${el('a')}.progress = 0.5`);
  check('setting progress moves the progress line', (await cdp.evaluate(`[${el('a')}.progress, ${el('a')}.shadowRoot.querySelector('input').value]`)).join() === '0.5,500');
  check('and sends isotrail-progress', await cdp.evaluate(`__events.some((e) => e.id === 'a' && e.name === 'isotrail-progress' && e.progress === 0.5)`));
  await cdp.evaluate(`${el('a')}.progress = NaN; ${el('a')}.progress = 'x'`);
  check('progress ignores values that are not numbers', (await cdp.evaluate(`${el('a')}.progress`)) === 0.5);
  await cdp.evaluate(`${el('a')}.progress = 7`);
  check('progress stays within 0 and 1', (await cdp.evaluate(`${el('a')}.progress`)) === 1);
  await sleep(300);
  const startShot = await shot('a');
  await cdp.evaluate(`${el('a')}.shadowRoot.querySelector('[part=zoom-in]').click()`);
  await sleep(300);
  await cdp.evaluate(`${el('a')}.reset()`);
  let resetBack = false;
  for (let i = 0; i < 30 && !resetBack; i++) {
    await sleep(100);
    resetBack = (await shot('a')) === startShot;
  }
  check('reset() returns to the starting view', resetBack);
  const firstReady = await cdp.evaluate(`${el('a')}.ready === ${el('a')}.ready`);
  await cdp.evaluate(`window.__oldReady = ${el('a')}.ready; ${el('a')}.setAttribute('src', '/data/timed.itb')`);
  check('a new src gives a new ready promise', firstReady && (await cdp.evaluate(`window.__oldReady !== ${el('a')}.ready`)));
  check('which resolves when it shows', await cdp.waitFor(`${el('a')}.dataset.state === 'ready'`));
  check('no uncaught errors on the script page', cdp.exceptions.length === 0, cdp.exceptions.join(' | '));

  // Which controls show, and how far the zoom goes
  await go('/controls.html');
  check('four controls embeds become ready', await allReady(['d', 'none', 'ps', 'mz']));
  const part = (id, name) => `${el(id)}.shadowRoot.querySelector('[part=${name}]')`;
  const shown = async (id) => {
    const parts = [];
    for (const name of ['play', 'zoom-in', 'zoom-out', 'reset', 'scrubber', 'credit']) {
      if (await visible(part(id, name))) parts.push(name);
    }
    return parts.join(',');
  };
  check('every control shows by default', (await shown('d')) === 'play,zoom-in,zoom-out,reset,scrubber,credit', await shown('d'));
  check('controls="none" shows only the credit', (await shown('none')) === 'credit', await shown('none'));
  check('controls="play,scrubber" shows those', (await shown('ps')) === 'play,scrubber,credit', await shown('ps'));
  const zoomInDisabled = (id) => cdp.evaluate(`${part(id, 'zoom-in')}.getAttribute('aria-disabled')`);
  const clickZoom = (id, times) => cdp.evaluate(`for (let i = 0; i < ${times}; i++) ${part(id, 'zoom-in')}.click()`);
  await clickZoom('d', 5);
  await clickZoom('mz', 5);
  check('the default zoom stops at 3x', (await zoomInDisabled('d')) === 'true');
  check('max-zoom="6" goes further', (await zoomInDisabled('mz')) === 'false');
  await clickZoom('mz', 4);
  check('and stops at 6x', (await zoomInDisabled('mz')) === 'true');
  await cdp.evaluate(`${el('ps')}.setAttribute('controls', 'zoom')`);
  check('changing controls updates them', (await shown('ps')) === 'zoom-in,zoom-out,credit', await shown('ps'));

  // Words: built-in Polish, inherited from the page, an override, bad values
  await go('/words.html');
  await cdp.waitFor(`['pl', 'inh', 'de', 'st', 'bs', 'bj'].every((id) => document.getElementById(id).dataset.state === 'ready') && ${el('plf')}.dataset.state === 'fallback'`);
  const label = (id, name) => cdp.evaluate(`${part(id, name)}.getAttribute('aria-label')`);
  check('lang="pl" speaks Polish', (await label('pl', 'play')) === 'Odtwórz' && (await label('pl', 'reset')) === 'Przywróć widok');
  check('the canvas is labelled in Polish', /^Diorama 3D: Zażółć/.test(await cdp.evaluate(`${el('pl')}.shadowRoot.querySelector('canvas').getAttribute('aria-label')`)));
  check('lang is taken from the page', (await label('inh', 'play')) === 'Odtwórz');
  check('a language without words stays English', (await label('de', 'play')) === 'Play');
  check('strings overrides words', (await label('st', 'play')) === 'Start' && (await label('st', 'zoom-in')) === 'Closer' && (await label('st', 'reset')) === 'Reset view');
  check('a failure message in Polish', (await cdp.evaluate(`${el('plf')}.shadowRoot.querySelector('.message').textContent`)) === 'Nie udało się wczytać tej wędrówki.');
  const wordWarnings = await cdp.evaluate('__warnings');
  check(
    'bad strings warn on the console',
    ['strings.play "1"', 'strings "nope"', 'strings "not json"'].every((p) => wordWarnings.some((w) => w.includes(p))),
    wordWarnings.join(' | '),
  );
  check('and keep the built-in words', (await label('bs', 'play')) === 'Play' && (await label('bj', 'play')) === 'Play');
  await cdp.evaluate(`${el('de')}.setAttribute('lang', 'pl')`);
  check('changing lang changes the words', (await label('de', 'play')) === 'Odtwórz');
  await cdp.evaluate(`${el('pl')}.play()`);
  check('the pause label is Polish too', (await label('pl', 'play')) === 'Wstrzymaj');

  // A theme file: different pixels, the progress line in its route colour,
  // and the built-in theme for a bad or missing file
  await go('/theme.html');
  check('five theme embeds become ready', await allReady(['d', 't', 'bt', 'mt', 'tr']));
  await sleep(300);
  const themeDefault = await shot('d');
  check('theme changes the colours', (await shot('t')) !== themeDefault);
  check('a bad theme draws the built-in one', (await shot('bt')) === themeDefault);
  check('a missing theme draws the built-in one', (await shot('mt')) === themeDefault);
  const themeWarnings = await cdp.evaluate('__warnings');
  check(
    'and each warns on the console',
    ['ignored the theme', 'could not load the theme /data/missing.toml'].every((p) => themeWarnings.some((w) => w.includes(p))) &&
      themeWarnings.some((w) => w.includes('lawn')),
    themeWarnings.join(' | '),
  );
  check('the progress line takes the theme\'s route colour', (await routeOf('t')) === '#2050e0', await routeOf('t'));
  check('route-colour beats the theme', (await routeOf('tr')) === '#e0a020', await routeOf('tr'));
  await cdp.evaluate(`${el('d')}.setAttribute('theme', '/data/loud.toml')`);
  let themed = false;
  for (let i = 0; i < 50 && !themed; i++) {
    await sleep(100);
    themed = (await shot('d')) === (await shot('t'));
  }
  check('setting theme on a live element redraws in it', themed);
  check('no uncaught errors on the theme page', cdp.exceptions.length === 0, cdp.exceptions.join(' | '));

  // Sizes past the GPU's largest texture, and shapes and values that can't
  // be read
  await go('/sizes.html');
  const tallReady = await cdp.waitFor(`${el('tall')}.dataset.state === 'ready'`);
  const tall = await cdp.evaluate(`(() => { const e = ${el('tall')}; const c = e.shadowRoot.querySelector('canvas'); return { css: e.getBoundingClientRect().height, width: c?.width, height: c?.height }; })()`);
  check('a 1:1000000 embed ends ready', tallReady && (await errorsOf('tall')).length === 0, `${await stateOf('tall')} ${JSON.stringify(await errorsOf('tall'))}`);
  check('its canvas is drawn within the largest texture', tall.css > 1e6 && tall.height > 0 && tall.height <= 32768 && tall.width >= 1, JSON.stringify(tall));
  const sizeLogs = [...(await cdp.evaluate('__warnings')), ...(await cdp.evaluate('__errors'))];
  check('and nothing panics', !sizeLogs.some((line) => line.includes('panicked')), sizeLogs.join(' | '));
  const sizeWarnings = await cdp.evaluate('__warnings');
  const warnedFor = (part) => sizeWarnings.filter((w) => w.includes(part)).length;
  check('aspect="abc" warns once', warnedFor('aspect "abc"') === 1, sizeWarnings.join(' | '));
  check(
    'aspect="constructor" and aspect="0:0" warn and fall back to wide',
    warnedFor('aspect "constructor"') === 1 &&
      warnedFor('aspect "0:0"') === 1 &&
      (await ratio('obj')) === '16 / 9' &&
      (await ratio('zero')) === '16 / 9' &&
      (await ratio('abc')) === '16 / 9',
    `${await ratio('obj')} ${await ratio('zero')} | ${sizeWarnings.join(' | ')}`,
  );
  check('credit="hidden" and backend="metal" warn once each', warnedFor('credit "hidden"') === 1 && warnedFor('backend "metal"') === 1, sizeWarnings.join(' | '));
  check('no uncaught errors on the sizes page', cdp.exceptions.length === 0, cdp.exceptions.join(' | '));

  // A still at the element's size is its first frame
  await go('/fit.html');
  const fitReady = await cdp.waitFor(`${el('f')}?.dataset.state === 'ready'`);
  check('the embed for the still becomes ready', fitReady, await stateOf('f'));
  await sleep(300);
  const fitBox = await cdp.evaluate(`(() => { const r = ${el('f')}.shadowRoot.querySelector('canvas').getBoundingClientRect(); return { x: r.x, y: r.y, width: r.width, height: r.height, scale: 1 }; })()`);
  const liveFrame = decodePng(Buffer.from(await cdp.screenshot(fitBox), 'base64'));
  const posterFrame = decodePng(await readFile(join(data, 'still.png')));
  // Where the canvas can't be transparent, its empty pixels are black
  const empty = (rgba, i) => [FIT.page, [0, 0, 0]].some((colour) => colour.every((value, c) => rgba[i + c] === value));
  const inLive = (rgba, i) => !empty(rgba, i);
  const inPoster = (rgba, i) => rgba[i + 3] > 0;
  const liveBox = boundingBox(liveFrame, inLive);
  const posterBox = boundingBox(posterFrame, inPoster);
  check(
    'the poster and the first live frame are the same size',
    liveFrame.width === posterFrame.width && liveFrame.height === posterFrame.height,
    `${liveFrame.width} x ${liveFrame.height} live, ${posterFrame.width} x ${posterFrame.height} poster`,
  );
  check(
    'the slab sits on the same pixels in the poster and the first live frame',
    JSON.stringify(liveBox) === JSON.stringify(posterBox),
    `live ${JSON.stringify(liveBox)}, poster ${JSON.stringify(posterBox)}`,
  );
  if (liveFrame.width === posterFrame.width && liveFrame.height === posterFrame.height) {
    let differing = 0;
    for (let i = 0; i < liveFrame.rgba.length; i += 4) {
      const [live, poster] = [inLive(liveFrame.rgba, i), inPoster(posterFrame.rgba, i)];
      if (live !== poster || (live && [0, 1, 2].some((c) => posterFrame.rgba[i + c] !== liveFrame.rgba[i + c]))) differing++;
    }
    console.log(`     ${((differing / (liveFrame.width * liveFrame.height)) * 100).toFixed(2)}% of the pixels differ between the poster and the first live frame`);
  }

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
  await quit(browser, cdp);
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

// A cold browser on a busy CI runner sometimes fails to start, so it gets three tries, and each failure prints its output
async function launch(attempt = 1) {
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
  const child = spawn(browserPath(), flags, { stdio: ['ignore', 'ignore', 'pipe'] });
  let output = '';
  child.stderr.on('data', (chunk) => (output = (output + chunk).slice(-4000)));
  // Edge's launcher on Windows hands over to another process and exits with 0, so only a failing exit ends the wait
  let failed = null;
  child.on('exit', (code, signal) => (failed = signal ?? (code === 0 ? null : code)));
  const portFile = join(profile, 'DevToolsActivePort');
  for (let i = 0; i < 600 && failed === null; i++) {
    const port = existsSync(portFile) ? Number((await readFile(portFile, 'utf8')).split('\n')[0]) : 0;
    if (port > 0) return { process: child, profile, port };
    await sleepFor(100);
  }
  stopBrowser(child, profile);
  await rm(profile, { recursive: true, force: true, maxRetries: 5 }).catch(() => {});
  const why = failed === null ? 'opened no DevTools port within 60s' : `exited with ${failed} before opening its DevTools port`;
  console.error(`${BROWSER} (${browserPath()}) ${why}${output.trim() ? `. Its last output:\n${output.trim()}` : ', with no output'}`);
  if (attempt < 3) return launch(attempt + 1);
  console.error(`could not start ${BROWSER} after 3 tries`);
  await rm(data, { recursive: true, force: true }).catch(() => {});
  process.exit(1);
}

async function quit(browser, cdp) {
  await Promise.race([cdp.send('Browser.close').catch(() => {}), sleepFor(5000)]);
  cdp.close();
  await sleepFor(1000);
  stopBrowser(browser.process, browser.profile);
}

// On Windows the browser runs as processes other than the one started, so whatever is left is found by its profile
function stopBrowser(child, profile) {
  if (process.platform === 'win32') {
    const script = `Get-CimInstance Win32_Process | Where-Object { $_.ProcessId -ne $PID -and $_.CommandLine -like '*${basename(profile)}*' } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }`;
    spawnSync('powershell', ['-NoProfile', '-NonInteractive', '-Command', script], { stdio: 'ignore' });
  } else if (child.exitCode === null && child.signalCode === null) {
    child.kill();
  }
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

// An 8-bit RGB or RGBA PNG without interlacing, as RGBA
function decodePng(bytes) {
  let at = 8;
  let width = 0;
  let height = 0;
  let channels = 0;
  const idat = [];
  while (at < bytes.length) {
    const length = bytes.readUInt32BE(at);
    const type = bytes.toString('latin1', at + 4, at + 8);
    const body = bytes.subarray(at + 8, at + 8 + length);
    if (type === 'IHDR') {
      width = body.readUInt32BE(0);
      height = body.readUInt32BE(4);
      const [depth, colour, , , interlace] = body.subarray(8, 13);
      channels = { 2: 3, 6: 4 }[colour];
      if (depth !== 8 || !channels || interlace) throw new Error(`can't decode a PNG of depth ${depth}, colour type ${colour}`);
    } else if (type === 'IDAT') {
      idat.push(body);
    }
    at += 12 + length;
  }
  const raw = inflateSync(Buffer.concat(idat));
  const stride = width * channels;
  const rows = Buffer.alloc(stride * height);
  for (let y = 0; y < height; y++) {
    const filter = raw[y * (stride + 1)];
    const line = raw.subarray(y * (stride + 1) + 1, (y + 1) * (stride + 1));
    for (let x = 0; x < stride; x++) {
      const left = x >= channels ? rows[y * stride + x - channels] : 0;
      const up = y > 0 ? rows[(y - 1) * stride + x] : 0;
      const corner = x >= channels && y > 0 ? rows[(y - 1) * stride + x - channels] : 0;
      const guess = [0, left, up, (left + up) >> 1, paeth(left, up, corner)][filter];
      rows[y * stride + x] = (line[x] + guess) & 0xff;
    }
  }
  const rgba = Buffer.alloc(width * height * 4);
  for (let i = 0; i < width * height; i++) {
    for (let c = 0; c < 3; c++) rgba[i * 4 + c] = rows[i * channels + c];
    rgba[i * 4 + 3] = channels === 4 ? rows[i * channels + 3] : 255;
  }
  return { width, height, rgba };
}

function paeth(a, b, c) {
  const p = a + b - c;
  const [pa, pb, pc] = [Math.abs(p - a), Math.abs(p - b), Math.abs(p - c)];
  return pa <= pb && pa <= pc ? a : pb <= pc ? b : c;
}

// The smallest box round the pixels `inside` accepts
function boundingBox({ width, height, rgba }, inside) {
  const box = { left: width, top: height, right: -1, bottom: -1 };
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      if (!inside(rgba, (y * width + x) * 4)) continue;
      box.left = Math.min(box.left, x);
      box.right = Math.max(box.right, x);
      box.top = Math.min(box.top, y);
      box.bottom = Math.max(box.bottom, y);
    }
  }
  return box;
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
