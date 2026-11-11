// For the browser check: drawing fails once window.__breakDraw is set
import { Viewer } from '../pkg/isotrail_web.js';

const draw = Viewer.prototype.draw;
Viewer.prototype.draw = function () {
  if (window.__breakDraw) throw new Error('drawing failed on purpose');
  return draw.call(this);
};
