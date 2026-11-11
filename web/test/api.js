// For the browser check: the Viewer API given numbers that aren't finite,
// and bundles it can't open
import init, { Viewer } from '../pkg/isotrail_web.js';

const backend = new URL(import.meta.url).search === '?webgl' ? 'webgl' : 'auto';
await init();
const bytes = async (name) => new Uint8Array(await (await fetch(`/data/${name}`)).arrayBuffer());
const reason = async (name) => {
  try {
    (await Viewer.load(document.createElement('canvas'), await bytes(name), backend)).free();
    return 'loaded';
  } catch (error) {
    return error.reason;
  }
};

const canvas = document.getElementById('c');
const viewer = await Viewer.load(canvas, await bytes('hike.itb'), backend);
viewer.resize(canvas.width, canvas.height);
viewer.draw();
viewer.zoomBy(Number.NaN, 0, 0);
viewer.zoomBy(2, Number.NaN, 0);
viewer.pan(Number.POSITIVE_INFINITY, 0);
viewer.setYaw(Number.NaN);
viewer.setProgress(Number.NaN);
viewer.setTime(Number.NEGATIVE_INFINITY);
viewer.draw();
const zoomBefore = viewer.zoom;
viewer.zoomBy(2, canvas.width / 2, canvas.height / 2);
viewer.draw();
window.__api = {
  zoomBefore,
  zoomAfter: viewer.zoom,
  credit: viewer.credit,
  olderReason: await reason('older.itb'),
  damagedReason: await reason('damaged.itb'),
};
