export class WindowHost {
  constructor(canvas, title, {onDiagnostic = () => {}} = {}) {
    this.canvas = canvas;
    this.title = title;
    this.onDiagnostic = onDiagnostic;
    this.windows = new Map();
  }
  apply(info) {
    const handle = info.handle ?? info.window;
    if (info.destroyed || info.action === 'destroy') this.windows.delete(handle);
    else if (handle !== undefined) this.windows.set(handle, info);
    if (info.title) {
      this.title.textContent = info.title;
      this.canvas.setAttribute('aria-label', `${info.title} game display`);
    }
    if (info.action) this.onDiagnostic(`guest window ${info.action}${handle === undefined ? '' : ` 0x${Number(handle).toString(16)}`}${info.title ? `: ${info.title}` : ''}`);
  }
  reset() { this.windows.clear(); this.title.textContent = 'game display'; }
}
