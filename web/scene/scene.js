// Builds scene files in a worker: a promise per build, its progress as
// events, and the browser's cache of what the services answered
//
//   const scenes = new SceneWorker();
//   const { scene, warnings } = await scenes.build(file, { options, onProgress, signal });
//
// `warnings` names what the scene left out, such as `weather`, and `notes`
// says in words what the command line would warn about, such as a track
// with no times
//
// A failed build rejects with an Error whose `reason` is one of `usage`,
// `input`, `no-ground`, `too-large`, `no-server`, `busy`, `unreachable`, `network`,
// `offline` or `other`, and a cancelled one with the reason `cancelled`
//
// `onProgress` hears `progress`, `slow` and `wait` messages (see worker.js)

export class SceneWorker {
  #worker;
  #next = 1;
  #pending = new Map();

  constructor(url = new URL('./worker.js', import.meta.url)) {
    this.#worker = new Worker(url, { type: 'module' });
    this.#worker.addEventListener('message', ({ data }) => this.#heard(data));
  }

  // `gpx` is a File, a Blob, an ArrayBuffer or bytes. `options` is an object
  // or a JSON string keyed by the long flag names of `isotrail render`
  //
  // `servers` lists Overpass servers to ask in turn in place of the built-in
  // ones, unless the options name an `overpass-url`. `skip` leaves built-in
  // ones out by name: `dresel`, `vk-maps` or `overpass-api`
  async build(gpx, { name, options = {}, now, servers, skip, onProgress, signal } = {}) {
    signal?.throwIfAborted();
    const bytes = gpx instanceof Blob ? await gpx.arrayBuffer() : copy(gpx);
    const json = typeof options === 'string' ? options : JSON.stringify(options);
    const id = this.#next++;
    const message = { type: 'build', gpx: bytes, name: name ?? gpx.name ?? '', options: json, now, servers, skip };
    const done = this.#ask(id, message, [bytes], onProgress);
    const cancel = () => this.#worker.postMessage({ type: 'cancel', id });
    signal?.addEventListener('abort', cancel, { once: true });
    try {
      return await done;
    } finally {
      signal?.removeEventListener('abort', cancel);
    }
  }

  // Checks options, a palette file's text or a theme file's text, rejecting
  // with the command line's message and the reason `usage`. `what` is
  // `options`, `palette` or `theme`; `options` may be an object
  async check(what, text, name = '') {
    const json = what === 'options' && typeof text !== 'string' ? JSON.stringify(text) : text;
    await this.#ask(this.#next++, { type: 'check', what, name, text: json });
  }

  // Rejects with the reason `no-server` when leaving out the Overpass
  // servers in `skip` leaves none that can serve this GPX file's slab,
  // without fetching anything
  async checkServers(gpx, { name, options = {}, now, skip = [] } = {}) {
    const bytes = gpx instanceof Blob ? await gpx.arrayBuffer() : copy(gpx);
    const json = typeof options === 'string' ? options : JSON.stringify(options);
    await this.#ask(this.#next++, { type: 'servers', gpx: bytes, name: name ?? gpx.name ?? '', options: json, now, skip }, [bytes]);
  }

  // The scene file's bytes with `exaggeration`, `season` and `no-weather`
  // applied, as the command line applies them to a scene file
  async adjust(scene, options = {}) {
    const bytes = scene instanceof Blob ? await scene.arrayBuffer() : copy(scene);
    const json = typeof options === 'string' ? options : JSON.stringify(options);
    const { scene: adjusted } = await this.#ask(this.#next++, { type: 'adjust', scene: bytes, options: json }, [bytes]);
    return adjusted;
  }

  // What the cache holds: `{ bytes, entries }`
  cacheSize() {
    return this.#ask(this.#next++, { type: 'cache-size' });
  }

  clearCache() {
    return this.#ask(this.#next++, { type: 'clear-cache' });
  }

  terminate() {
    this.#worker.terminate();
    for (const { reject } of this.#pending.values()) reject(failure('cancelled', 'the scene worker was stopped'));
    this.#pending.clear();
  }

  #ask(id, message, transfer = [], onProgress) {
    return new Promise((resolve, reject) => {
      this.#pending.set(id, { resolve, reject, onProgress });
      this.#worker.postMessage({ id, ...message }, transfer);
    });
  }

  #heard(data) {
    const job = this.#pending.get(data.id);
    if (!job) return;
    const { type, id, ...rest } = data;
    if (['progress', 'slow', 'wait'].includes(type)) {
      job.onProgress?.({ type, ...rest });
      return;
    }
    this.#pending.delete(id);
    if (type === 'failed') job.reject(failure(rest.reason, rest.message));
    else if (type === 'cancelled') job.reject(failure('cancelled', 'the build was cancelled'));
    else job.resolve(rest);
  }
}

function copy(bytes) {
  const view = ArrayBuffer.isView(bytes) ? bytes : new Uint8Array(bytes);
  return view.buffer.slice(view.byteOffset, view.byteOffset + view.byteLength);
}

function failure(reason, message) {
  return Object.assign(new Error(message), { reason });
}
