import init, { Viewer } from '../pkg/isotrail_web.js';

const canvas = document.querySelector('canvas');
const status = document.querySelector('#status');
const progress = document.querySelector('#progress');
const yaw = document.querySelector('#yaw');
const backendPicker = document.querySelector('#backend');

const params = new URLSearchParams(location.search);
const backend = params.get('backend') === 'webgl' ? 'webgl' : 'auto';
backendPicker.value = backend;
backendPicker.addEventListener('change', () => {
  params.set('backend', backendPicker.value);
  location.search = params.toString();
});

function show(text, state) {
  status.textContent = text;
  document.body.dataset.state = state;
}

try {
  await init();
  const response = await fetch('hike.itb');
  if (!response.ok) {
    throw new Error(`hike.itb: ${response.status} ${response.statusText}`);
  }
  const bytes = new Uint8Array(await response.arrayBuffer());
  const viewer = await Viewer.load(canvas, bytes, backend);

  let queued = false;
  const redraw = () => {
    if (queued) return;
    queued = true;
    requestAnimationFrame(() => {
      queued = false;
      viewer.draw();
    });
  };

  new ResizeObserver(([entry]) => {
    const box = entry.devicePixelContentBoxSize?.[0];
    const width = box ? box.inlineSize : Math.round(entry.contentRect.width * devicePixelRatio);
    const height = box ? box.blockSize : Math.round(entry.contentRect.height * devicePixelRatio);
    canvas.width = width;
    canvas.height = height;
    viewer.resize(width, height);
    show(`Drawing with ${viewer.backend} into ${width} × ${height} pixels`, 'ready');
    redraw();
  }).observe(canvas);

  progress.addEventListener('input', () => {
    viewer.setProgress(progress.value / 1000);
    redraw();
  });
  yaw.addEventListener('input', () => {
    viewer.setYaw(Number(yaw.value));
    redraw();
  });
} catch (error) {
  show(`Could not start the renderer: ${error.message ?? error}`, 'error');
  console.error(error);
}
