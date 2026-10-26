// ?backend=webgl forces the WebGL2 fallback in browsers that have WebGPU
const webgl = new URLSearchParams(location.search).get('backend') === 'webgl';
const link = document.querySelector('#backend');

if (webgl) {
  for (const hike of document.querySelectorAll('isotrail-hike')) {
    hike.setAttribute('backend', 'webgl');
  }
  link.href = '?';
  link.textContent = 'Back to WebGPU';
}
