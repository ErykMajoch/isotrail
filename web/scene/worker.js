// Builds a scene file from a GPX file, as `isotrail bundle` does, fetching
// what the elevation, map and weather services answer and keeping it in the
// browser's cache. Every message carries the `id` of its build
//
// In:  { type: 'build', id, gpx, name, options, now, servers, skip }, { type: 'cancel', id },
//      { type: 'cache-size', id }, { type: 'clear-cache', id },
//      { type: 'check', id, what: 'options' | 'palette' | 'theme', name, text },
//      { type: 'servers', id, gpx, name, options, now, skip }, { type: 'adjust', id, scene, options }
// Out: { type: 'progress', step, done, total, server }, { type: 'slow', kind, seconds },
//      { type: 'wait', kind, why, attempt, attempts, ms, until },
//      { type: 'built', scene, warnings, notes, passes, ms, fetched, cached, unsaved, memory },
//      { type: 'failed', reason, message }, { type: 'cancelled' },
//      { type: 'cache', bytes, entries }, { type: 'checked' }, { type: 'adjusted', scene }
import init, {
  SceneBuilder, adjust, checkOptions, checkPalette, checkTheme, loadTimezones, offline,
} from './pkg/isotrail_scene.js';

// Each pass fetches what the last one missed or waited for
const PASSES = 20;
// As long as the command line waits for an answer
const TIMEOUT_MS = 300000;
// When the command line says it is still waiting
const SLOW_MS = 10000;
const CACHE = 'isotrail-data';

let memory;
const loaded = init().then((exports) => {
  ({ memory } = exports);
});
// Checks need only the code; reading a GPX file needs the time zones too
const ready = (async () => {
  await loaded;
  const zones = await fetch(new URL('./pkg/lite.tzb', import.meta.url));
  if (!zones.ok) throw new Error(`can't fetch the time zones: HTTP ${zones.status}`);
  loadTimezones(new Uint8Array(await zones.arrayBuffer()));
})();
ready.catch(() => {});

const builds = new Map();

const cacheUrl = (key) => `https://cache.isotrail.invalid/${key}`;

// The cache is a help, never a need: outside a secure context there is none,
// and a full quota or a damaged entry only means fetching again
async function openCache() {
  try {
    return await caches.open(CACHE);
  } catch {
    return undefined;
  }
}

async function fromCache(cache, key) {
  try {
    const stored = await cache?.match(cacheUrl(key));
    return stored ? new Uint8Array(await stored.arrayBuffer()) : undefined;
  } catch {
    return undefined;
  }
}

async function keep(cache, key, bytes) {
  if (!cache) return false;
  try {
    const headers = { 'content-type': 'application/octet-stream', 'x-isotrail-bytes': String(bytes.length) };
    await cache.put(cacheUrl(key), new Response(bytes, { headers }));
    return true;
  } catch {
    return false;
  }
}

async function cacheSize() {
  const cache = await openCache();
  let bytes = 0;
  let entries = 0;
  for (const request of (await cache?.keys()) ?? []) {
    const stored = await cache.match(request);
    bytes += Number(stored?.headers.get('x-isotrail-bytes') ?? 0);
    entries++;
  }
  return { bytes, entries };
}

function failure(reason, message) {
  return Object.assign(new Error(message), { reason });
}

function sleep(ms, signal) {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(resolve, ms);
    signal.addEventListener('abort', () => {
      clearTimeout(timer);
      reject(signal.reason);
    }, { once: true });
  });
}

class Build {
  constructor(id) {
    this.id = id;
    this.abort = new AbortController();
    this.asked = new Set();
    this.counts = {};
    this.fetched = 0;
    this.cached = 0;
    this.unsaved = 0;
  }

  post(message, transfer = []) {
    postMessage({ id: this.id, ...message }, transfer);
  }

  // Map data names the Overpass server asked
  count(kind, done, url) {
    const count = (this.counts[kind] ??= { done: 0, total: 0 });
    if (done) count.done++;
    else count.total++;
    const server = kind === 'map-data' ? new URL(url).host : undefined;
    this.post({ type: 'progress', step: kind, done: count.done, total: count.total, server });
  }

  async fetchAll(builder, wanted) {
    const requests = wanted.map((request) => {
      const { key, method, url, body, kind, aliases } = request;
      request.free();
      this.count(kind, false, url);
      return { key, method, url, body, kind, aliases };
    });
    await Promise.all(requests.map((request) => this.fetchOne(builder, request)));
  }

  async fetchOne(builder, { key, method, url, body, kind, aliases }) {
    // Asked again in one build: the answer held was unusable or failed, so it
    // goes to the network, and a stored copy goes
    const again = this.asked.has(key);
    this.asked.add(key);
    // Map data kept from another Overpass server serves as well
    for (const stored of again ? [] : [key, ...aliases]) {
      const bytes = await fromCache(this.cache, stored);
      this.abort.signal.throwIfAborted();
      if (bytes) {
        builder.hold(stored, 200, bytes);
        this.cached++;
        this.count(kind, true, url);
        return;
      }
    }
    if (again) await this.cache?.delete(cacheUrl(key)).catch(() => {});
    const timeout = AbortSignal.timeout(TIMEOUT_MS);
    const signal = AbortSignal.any([this.abort.signal, timeout]);
    const started = performance.now();
    const slow = setInterval(() => {
      this.post({ type: 'slow', kind, seconds: Math.round((performance.now() - started) / 1000) });
    }, SLOW_MS);
    const sent = method === 'POST'
      ? { method, body, headers: { 'content-type': 'application/x-www-form-urlencoded' } }
      : {};
    try {
      const response = await fetch(url, { ...sent, signal });
      const bytes = new Uint8Array(await response.arrayBuffer());
      builder.hold(key, response.status, bytes);
      this.fetched++;
      if (response.ok && !(await keep(this.cache, key, bytes))) this.unsaved++;
    } catch (error) {
      this.abort.signal.throwIfAborted();
      if (navigator.onLine === false) throw offline();
      builder.failed(key, String(error?.message ?? error), timeout.aborted);
    } finally {
      clearInterval(slow);
    }
    this.count(kind, true, url);
  }

  async run({ gpx, name, options, now, servers, skip }) {
    const started = performance.now();
    let builder;
    try {
      await ready;
      this.abort.signal.throwIfAborted();
      this.cache = await openCache();
      builder = new SceneBuilder(new Uint8Array(gpx), name ?? '', options ?? '{}', now ?? Date.now() / 1000);
      if (servers?.length) builder.useServers(servers);
      if (skip?.length) builder.skipServers(skip);
      await this.fetchAll(builder, builder.requests());
      for (let pass = 1; pass <= PASSES; pass++) {
        this.post({ type: 'progress', step: 'building', done: pass - 1, total: pass });
        const scene = builder.build();
        if (scene) {
          this.post({
            type: 'built',
            scene,
            warnings: builder.warnings,
            notes: builder.notes,
            passes: pass,
            ms: performance.now() - started,
            fetched: this.fetched,
            cached: this.cached,
            unsaved: this.unsaved,
            memory: memory?.buffer.byteLength,
          }, [scene.buffer]);
          return;
        }
        // An unusable answer kept in the cache would come back from it
        for (const key of builder.dropped) await this.cache?.delete(cacheUrl(key)).catch(() => {});
        const wait = builder.wait;
        if (wait) {
          const { kind, why, attempt, attempts, ms } = wait;
          this.post({ type: 'wait', kind, why, attempt, attempts, ms, until: Date.now() + ms });
          await sleep(ms, this.abort.signal);
        }
        await this.fetchAll(builder, builder.missing);
      }
      throw failure('other', `the scene still needs more data after ${PASSES} tries`);
    } catch (error) {
      if (this.abort.signal.aborted) {
        this.post({ type: 'cancelled' });
      } else {
        this.post({ type: 'failed', reason: error?.reason ?? 'other', message: String(error?.message ?? error) });
      }
    } finally {
      builder?.free();
      builds.delete(this.id);
    }
  }
}

onmessage = async ({ data }) => {
  const { type, id } = data;
  if (type === 'build') {
    const build = new Build(id);
    builds.set(id, build);
    await build.run(data);
  } else if (type === 'cancel') {
    builds.get(id)?.abort.abort();
  } else if (type === 'cache-size') {
    postMessage({ id, type: 'cache', ...(await cacheSize()) });
  } else if (type === 'clear-cache') {
    await caches.delete(CACHE).catch(() => {});
    postMessage({ id, type: 'cache', bytes: 0, entries: 0 });
  } else if (['check', 'servers', 'adjust'].includes(type)) {
    try {
      await (type === 'servers' ? ready : loaded);
      const answer = quick(data);
      postMessage({ id, ...answer }, answer.scene ? [answer.scene.buffer] : []);
    } catch (error) {
      postMessage({ id, type: 'failed', reason: error?.reason ?? 'other', message: String(error?.message ?? error) });
    }
  }
};

// What the page asks without fetching anything
function quick({ type, what, name, text, gpx, options, now, skip, scene }) {
  if (type === 'adjust') return { type: 'adjusted', scene: adjust(new Uint8Array(scene), options ?? '{}') };
  if (type === 'servers') {
    // Building the scene's plan names the servers that can serve its slab
    const builder = new SceneBuilder(new Uint8Array(gpx), name ?? '', options ?? '{}', now ?? Date.now() / 1000);
    try {
      builder.skipServers(skip ?? []);
    } finally {
      builder.free();
    }
  } else if (what === 'options') {
    checkOptions(text ?? '{}');
  } else if (what === 'palette') {
    checkPalette(name ?? '', text ?? '');
  } else if (what === 'theme') {
    checkTheme(name ?? '', text ?? '');
  } else {
    throw failure('other', `nothing to check called ${what}`);
  }
  return { type: 'checked' };
}
