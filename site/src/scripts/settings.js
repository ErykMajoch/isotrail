// The settings this browser keeps between visits: the options set on the
// page and the map data servers left out. Never the GPX file, the scene or
// its title. A stored shape from another version is dropped

const KEY = 'isotrail.settings';
const VERSION = 1;

// Local storage, or none where the browser refuses it
function available() {
  try {
    const store = window.localStorage;
    store.setItem(`${KEY}.probe`, '1');
    store.removeItem(`${KEY}.probe`);
    return store;
  } catch {
    return undefined;
  }
}

export class Settings {
  constructor() {
    this.store = available();
    this.remember = true;
  }

  // What was kept: `{ options, skip }`, empty when nothing was or the
  // stored shape isn't this version's
  load() {
    const empty = { options: {}, skip: [] };
    const kept = this.kept();
    if (kept === null) return empty;
    if (!isObject(kept) || kept.version !== VERSION) {
      this.#write(undefined);
      return empty;
    }
    this.remember = kept.remember !== false;
    if (!this.remember) return empty;
    return {
      options: isObject(kept.options) ? kept.options : {},
      skip: Array.isArray(kept.skip) ? kept.skip.filter((name) => typeof name === 'string') : [],
    };
  }

  // Keeps these, or only that nothing is to be kept when remembering is off
  save(options, skip) {
    if (!this.remember) {
      this.#write({ version: VERSION, remember: false });
    } else if (Object.keys(options).length === 0 && skip.length === 0) {
      this.#write(undefined);
    } else {
      this.#write({ version: VERSION, remember: true, options, skip });
    }
  }

  // The stored object as it is, `null` when there is none
  kept() {
    try {
      return JSON.parse(this.store?.getItem(KEY) ?? 'null');
    } catch {
      return undefined;
    }
  }

  // A full or refused storage only means nothing is kept
  #write(kept) {
    try {
      if (kept === undefined) this.store?.removeItem(KEY);
      else this.store?.setItem(KEY, JSON.stringify(kept));
    } catch {}
  }
}

function isObject(value) {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}
