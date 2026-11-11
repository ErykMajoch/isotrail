// For the browser check: a browser with neither WebGPU nor WebGL2
delete Navigator.prototype.gpu;
const getContext = HTMLCanvasElement.prototype.getContext;
HTMLCanvasElement.prototype.getContext = function (type, ...rest) {
  return /webgl|webgpu/.test(type) ? null : getContext.call(this, type, ...rest);
};
