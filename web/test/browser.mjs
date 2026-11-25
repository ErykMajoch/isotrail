// Starts Chrome or Edge headless and drives it over the DevTools protocol,
// for the browser checks

import { spawn, spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { basename, join } from 'node:path';

export function browserPath(name) {
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
  }[process.platform]?.[name] ?? [];
  const found = candidates.find((path) => existsSync(path));
  if (!found) {
    console.error(`could not find ${name}: set ISOTRAIL_BROWSER to its path`);
    process.exit(1);
  }
  return found;
}

// A cold browser on a busy CI runner sometimes fails to start, so it gets three tries, and each failure prints its output
export async function launch(name, { webgl, viewport }, attempt = 1) {
  const profile = await mkdtemp(join(tmpdir(), 'isotrail-browser-'));
  const flags = [
    '--headless=new',
    '--remote-debugging-port=0',
    `--user-data-dir=${profile}`,
    '--no-first-run',
    '--no-default-browser-check',
    `--window-size=${viewport.width},${viewport.height}`,
    ...(webgl ? ['--use-angle=swiftshader', '--enable-unsafe-swiftshader'] : ['--enable-unsafe-webgpu']),
    ...(process.platform === 'linux' ? ['--no-sandbox'] : []),
    'about:blank',
  ];
  const child = spawn(browserPath(name), flags, { stdio: ['ignore', 'ignore', 'pipe'] });
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
  console.error(`${name} (${browserPath(name)}) ${why}${output.trim() ? `. Its last output:\n${output.trim()}` : ', with no output'}`);
  if (attempt < 3) return launch(name, { webgl, viewport }, attempt + 1);
  throw new Error(`could not start ${name} after 3 tries`);
}

export async function quit(browser, cdp) {
  await Promise.race([cdp.send('Browser.close').catch(() => {}), sleepFor(5000)]);
  cdp.close();
  await sleepFor(1000);
  stopBrowser(browser.process, browser.profile);
}

// On Windows the browser runs as processes other than the one started, so whatever is left is found by its profile
export function stopBrowser(child, profile) {
  if (process.platform === 'win32') {
    const script = `Get-CimInstance Win32_Process | Where-Object { $_.ProcessId -ne $PID -and $_.CommandLine -like '*${basename(profile)}*' } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }`;
    spawnSync('powershell', ['-NoProfile', '-NonInteractive', '-Command', script], { stdio: 'ignore' });
  } else if (child.exitCode === null && child.signalCode === null) {
    child.kill();
  }
}

export function sleepFor(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

// `on` hears events, with the session id of a worker attached to the page. `send` takes one to reach that worker
export async function connect(port, timeoutMs) {
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
  const listeners = new Map();
  const exceptions = [];
  ws.addEventListener('message', ({ data: text }) => {
    const message = JSON.parse(text);
    if (message.id && pending.has(message.id)) {
      pending.get(message.id)(message);
      pending.delete(message.id);
      return;
    }
    if (message.method === 'Runtime.exceptionThrown') {
      const details = message.params.exceptionDetails;
      exceptions.push((details.exception?.description ?? details.text).slice(0, 300));
    }
    for (const listener of listeners.get(message.method) ?? []) listener(message.params, message.sessionId);
  });
  const on = (method, listener) => listeners.set(method, [...(listeners.get(method) ?? []), listener]);
  const send = (method, params = {}, sessionId = undefined) =>
    new Promise((resolve, reject) => {
      const id = next++;
      pending.set(id, (message) =>
        message.error ? reject(new Error(`${method}: ${message.error.message}`)) : resolve(message.result),
      );
      ws.send(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) }));
    });
  const evaluate = async (expression) => {
    const result = await send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
    if (result.exceptionDetails) {
      throw new Error(result.exceptionDetails.exception?.description ?? result.exceptionDetails.text);
    }
    return result.result.value;
  };
  const waitFor = async (expression, ms = timeoutMs) => {
    const end = Date.now() + ms;
    while (Date.now() < end) {
      if (await evaluate(expression).catch(() => false)) return true;
      await sleepFor(100);
    }
    return false;
  };
  const screenshot = async (clip) => (await send('Page.captureScreenshot', { format: 'png', clip })).data;
  return { send, on, evaluate, waitFor, screenshot, exceptions, close: () => ws.close() };
}
