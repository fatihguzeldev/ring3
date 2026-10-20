export class GdiHost {
  constructor(canvas, windows) {
    this.canvas = canvas;
    this.windows = windows;
    this.context = canvas.getContext('2d');
    if (!this.context) throw new Error('GDI canvas is unavailable');
    this.surfaces = new Map();
  }
  root(window) {
    const seen = new Set();
    while (!seen.has(window)) {
      seen.add(window);
      const item = this.windows.get(window);
      if (!item || !(item.style & 0x40000000) || !item.parent) return window;
      window = item.parent;
    }
    throw new Error('cyclic guest window hierarchy');
  }
  accept(frame, pixels) {
    if (frame.format !== 1 || frame.stride !== frame.width * 4 || pixels.length !== frame.stride * frame.height) throw new Error('unsupported GDI frame layout');
    const rgba = new Uint8ClampedArray(pixels.buffer, pixels.byteOffset, pixels.byteLength);
    for (let i = 0; i < rgba.length; i += 4) {
      const blue = rgba[i]; rgba[i] = rgba[i + 2]; rgba[i + 2] = blue; rgba[i + 3] = 255;
    }
    const key = `${frame.window}:${frame.windowDc}`;
    let surface = this.surfaces.get(key);
    if (!surface) surface = {canvas: document.createElement('canvas')};
    Object.assign(surface, frame);
    surface.canvas.width = frame.width; surface.canvas.height = frame.height;
    surface.canvas.getContext('2d').putImageData(new ImageData(rgba, frame.width, frame.height), 0, 0);
    this.surfaces.set(key, surface);
    if(this.windows.get(frame.window)?.visible!==false)this.active = this.root(frame.window);
    this.draw();
  }
  draw() {
    const surfaces = [...this.surfaces.values()].filter(frame => this.root(frame.window) === this.active && this.windows.get(frame.window)?.visible!==false);
    const root=this.windows.get(this.active),windowDc=surfaces.some(frame=>frame.window===this.active&&frame.windowDc);
    const origin=root&&root.clientX!==undefined?{x:windowDc?root.left:root.clientX,y:windowDc?root.top:root.clientY}:null;
    this.origin = origin;
    const position = frame => {
      const canonical=this.windows.get(frame.window);
      if(origin&&canonical?.clientX!==undefined)return {frame,x:(frame.windowDc?canonical.left:canonical.clientX)-origin.x,y:(frame.windowDc?canonical.top:canonical.clientY)-origin.y,depth:this.depth(frame.window)};
      let x = 0, y = 0, depth = 0, window = frame.window;
      while (window !== this.active) {
        const item = this.windows.get(window);
        if (!item) break;
        x += item.x; y += item.y; depth++; window = item.parent;
      }
      return {frame, x, y, depth};
    };
    const positioned = surfaces.map(position).sort((a, b) => a.depth - b.depth || Number(b.frame.windowDc) - Number(a.frame.windowDc));
    const roots = positioned.filter(item => item.frame.window === this.active);
    this.canvas.width = origin?Math.max(1,windowDc?root.right-root.left:root.clientWidth):Math.max(1, ...roots.map(item => item.frame.width), ...positioned.map(item => item.x + item.frame.width));
    this.canvas.height = origin?Math.max(1,windowDc?root.bottom-root.top:root.clientHeight):Math.max(1, ...roots.map(item => item.frame.height), ...positioned.map(item => item.y + item.frame.height));
    for (const {frame, x, y} of positioned) {
      this.context.save();
      if(origin) {
        for(let parent=this.windows.get(frame.window)?.parent;parent;parent=this.windows.get(parent)?.parent) {
          const bounds=this.windows.get(parent);if(!bounds)break;
          this.context.beginPath();this.context.rect(bounds.clientX-origin.x,bounds.clientY-origin.y,bounds.clientWidth,bounds.clientHeight);this.context.clip();
        }
      }
      this.context.drawImage(frame.canvas, x, y);this.context.restore();
    }
    this.canvas.hidden = positioned.length === 0;
  }
  depth(window) {let depth=0;for(let parent=this.windows.get(window)?.parent;parent;parent=this.windows.get(parent)?.parent){if(++depth>256)throw new Error('excessive window depth');}return depth;}
  presented(window) {
    this.active = window;
    for (const [key, frame] of this.surfaces) if (frame.window === window) this.surfaces.delete(key);
    this.draw();
  }
  changed(info) {
    if (info.destroyed) for (const [key, frame] of this.surfaces) if (frame.window === info.handle) this.surfaces.delete(key);
    if (!this.windows.has(this.active) || this.windows.get(this.active)?.visible === false) {
      const visible = [...this.surfaces.values()].find(frame => this.windows.get(frame.window)?.visible !== false);
      this.active = visible ? this.root(visible.window) : undefined;
    }
    this.draw();
  }
  clear() { this.surfaces.clear(); this.active = undefined; this.canvas.hidden = true; }
}
