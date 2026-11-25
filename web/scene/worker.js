// Builds a scene file from a GPX file, as `isotrail bundle` does, fetching
// what the elevation, map and weather services answer.
// In: { gpx, name, options, now }. Out: { scene, warnings, passes, ms } or { error }
import init, { SceneBuilder, loadTimezones } from './pkg/isotrail_scene.js';

// A build asks for more after its first pass only where the archive has
// gaps or an answer can't be read
const PASSES = 6;

const ready = (async () => {
  await init();
  const zones = await fetch(new URL('./pkg/lite.tzb', import.meta.url));
  if (!zones.ok) throw new Error(`can't fetch the time zones: HTTP ${zones.status}`);
  loadTimezones(new Uint8Array(await zones.arrayBuffer()));
})();
ready.catch(() => {});

async function fetchInto(builder, wanted) {
  await Promise.all(wanted.map(async (request) => {
    const { key, method, url, body } = request;
    request.free();
    const sent = method === 'POST'
      ? { method, body, headers: { 'content-type': 'application/x-www-form-urlencoded' } }
      : {};
    try {
      const response = await fetch(url, sent);
      builder.hold(key, response.status, new Uint8Array(await response.arrayBuffer()));
    } catch (error) {
      builder.failed(key, String(error?.message ?? error), false);
    }
  }));
}

onmessage = async ({ data }) => {
  const started = performance.now();
  let builder;
  try {
    await ready;
    const now = data.now ?? Date.now() / 1000;
    builder = new SceneBuilder(new Uint8Array(data.gpx), data.name ?? '', data.options ?? '{}', now);
    await fetchInto(builder, builder.requests());
    for (let pass = 1; pass <= PASSES; pass++) {
      const scene = builder.build();
      if (scene) {
        const ms = performance.now() - started;
        postMessage({ scene, warnings: builder.warnings, passes: pass, ms }, [scene.buffer]);
        return;
      }
      await fetchInto(builder, builder.missing);
    }
    throw new Error(`the scene still needs more data after ${PASSES} tries`);
  } catch (error) {
    postMessage({ error: String(error?.message ?? error) });
  } finally {
    builder?.free();
  }
};
