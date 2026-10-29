const SCAN = {
  Escape:0x01,Digit1:0x02,Digit2:0x03,Digit3:0x04,Digit4:0x05,Digit5:0x06,Digit6:0x07,Digit7:0x08,Digit8:0x09,Digit9:0x0a,Digit0:0x0b,Minus:0x0c,Equal:0x0d,Backspace:0x0e,Tab:0x0f,
  KeyQ:0x10,KeyW:0x11,KeyE:0x12,KeyR:0x13,KeyT:0x14,KeyY:0x15,KeyU:0x16,KeyI:0x17,KeyO:0x18,KeyP:0x19,BracketLeft:0x1a,BracketRight:0x1b,Enter:0x1c,ControlLeft:0x1d,
  KeyA:0x1e,KeyS:0x1f,KeyD:0x20,KeyF:0x21,KeyG:0x22,KeyH:0x23,KeyJ:0x24,KeyK:0x25,KeyL:0x26,Semicolon:0x27,Quote:0x28,Backquote:0x29,ShiftLeft:0x2a,Backslash:0x2b,
  KeyZ:0x2c,KeyX:0x2d,KeyC:0x2e,KeyV:0x2f,KeyB:0x30,KeyN:0x31,KeyM:0x32,Comma:0x33,Period:0x34,Slash:0x35,ShiftRight:0x36,NumpadMultiply:0x37,AltLeft:0x38,Space:0x39,CapsLock:0x3a,
  F1:0x3b,F2:0x3c,F3:0x3d,F4:0x3e,F5:0x3f,F6:0x40,F7:0x41,F8:0x42,F9:0x43,F10:0x44,NumLock:0x45,ScrollLock:0x46,Numpad7:0x47,Numpad8:0x48,Numpad9:0x49,NumpadSubtract:0x4a,
  Numpad4:0x4b,Numpad5:0x4c,Numpad6:0x4d,NumpadAdd:0x4e,Numpad1:0x4f,Numpad2:0x50,Numpad3:0x51,Numpad0:0x52,NumpadDecimal:0x53,IntlBackslash:0x56,F11:0x57,F12:0x58,
  NumpadEnter:0x9c,ControlRight:0x9d,NumpadDivide:0xb5,PrintScreen:0xb7,AltRight:0xb8,Pause:0xc5,Home:0xc7,ArrowUp:0xc8,PageUp:0xc9,ArrowLeft:0xcb,ArrowRight:0xcd,End:0xcf,ArrowDown:0xd0,PageDown:0xd1,Insert:0xd2,Delete:0xd3,MetaLeft:0xdb,MetaRight:0xdc,ContextMenu:0xdd,
};
const mouseButton = [0,2,1,3,4];
const integer = value => Math.max(-0x80000000, Math.min(0x7fffffff, Math.round(value)));

export class InputHost {
  constructor(canvas, {send, viewport = () => ({canvas,x:0,y:0}), onCapture = () => {}, onDiagnostic = () => {}}) {
    this.canvas = canvas;
    this.send = send;
    this.viewport = viewport;
    this.onCapture = onCapture;
    this.onDiagnostic = onDiagnostic;
    this.enabled = false;
    this.preserveFocus = false;
    this.blocked = false;
    this.focused = false;
    this.windowFocused = document.hasFocus();
    this.keys = new Set();
    this.buttons = new Set();
    this.relative = {dx:0,dy:0,wheel:0};
    this.absolute = null;
    this.frame = 0;
    this.diagnostics = null;
    this.abort = new AbortController();
    const options = {signal:this.abort.signal};
    window.addEventListener('keydown', event => this.key(event,true), options);
    window.addEventListener('keyup', event => this.key(event,false), options);
    window.addEventListener('blur', () => { this.control('windowBlur'); this.windowFocused = false; this.release(true); }, options);
    window.addEventListener('focus', () => { this.control('windowFocus'); this.windowFocused = true; this.focus(); }, options);
    document.addEventListener('visibilitychange', () => {
      this.windowFocused = document.hasFocus();
      if (document.hidden) this.release(true);
      else this.focus();
    }, options);
    canvas.addEventListener('focus', () => { this.control('canvasFocus'); this.focus(); }, options);
    canvas.addEventListener('blur', () => { this.control('canvasBlur'); if (!this.locked) this.release(true); }, options);
    canvas.addEventListener('mousedown', event => {
      let accepted = false;
      try {
      if (!this.enabled || this.blocked) return;
      canvas.focus();
      this.focus();
      if (!this.active) return;
      const button = mouseButton[event.button];
      if (button === undefined || this.buttons.has(button)) return;
      const point = this.locked ? null : this.point(event);
      if (!this.locked && !point) return;
      event.preventDefault();
      this.flush(); this.buttons.add(button);
      this.sendTimed([...(point ? [{kind:'pointer',...point}] : []),{kind:'button',button,down:true}],this.timestamp(event));
      accepted = true;
      } finally { if (this.diagnostics) this.recordEvent('mouse', event, accepted); }
    }, options);
    window.addEventListener('mouseup', event => {
      let accepted = false;
      try {
      const button = mouseButton[event.button];
      if (!this.buttons.delete(button)) return;
      this.flush(); this.sendTimed([{kind:'button',button,down:false}],this.timestamp(event));
      accepted = true;
      } finally { if (this.diagnostics) this.recordEvent('mouse', event, accepted); }
    }, options);
    document.addEventListener('mousemove', event => this.move(event), options);
    canvas.addEventListener('wheel', event => {
      if (!this.active) return;
      event.preventDefault();
      const pixels = event.deltaY * (event.deltaMode === 1 ? 16 : event.deltaMode === 2 ? canvas.clientHeight : 1);
      this.relative.wheel += -pixels * 3;
      this.motionTimestampMs = this.timestamp(event);
      this.schedule();
    }, {...options,passive:false});
    canvas.addEventListener('contextmenu', event => event.preventDefault(), options);
    document.addEventListener('pointerlockchange', () => {
      if (!this.locked) this.release(false);
      if (!this.physicalAvailable) this.release(true);
      else this.focus();
      const captured = this.active && this.locked;
      if (this.enabled || this.preserveFocus) this.sendTimed([{kind:'capture',captured}]);
      this.onCapture(captured);
      if (this.locked && !captured) document.exitPointerLock();
    }, options);
    document.addEventListener('pointerlockerror', () => this.onDiagnostic('mouse capture was denied; click capture mouse to retry'), options);
  }
  get locked() { return document.pointerLockElement === this.canvas; }
  timestamp(event) {
    const now = performance.now(),source = event?.timeStamp;
    return performance.timeOrigin + (Number.isFinite(source) && source > 0 && source <= now ? source : now);
  }
  sendTimed(events,timestampMs = this.timestamp()) {
    this.send(events.map(event=>({...event,timestampMs})));
  }
  setDiagnostics(enabled) {
    if (!enabled) { this.diagnostics = null; return; }
    this.diagnostics ??= {
      keyEvents:0,keyAccepted:0,keyRejected:0,unknownKeyEvents:0,
      mouseEvents:0,mouseAccepted:0,mouseRejected:0,controls:{},
    };
  }
  snapshot() {
    const diagnostics = this.diagnostics;
    if (!diagnostics) return;
    return {
      enabled:this.enabled,blocked:this.blocked,windowFocused:this.windowFocused,focused:this.focused,
      documentFocused:document.hasFocus(),visible:!document.hidden,
      activeElement:document.activeElement?.id || document.activeElement?.tagName?.toLowerCase() || null,
      canvasFocused:document.activeElement===this.canvas,locked:this.locked,available:this.available,active:this.active,
      ...diagnostics,controls:{...diagnostics.controls},
      lastKey:diagnostics.lastKey && {...diagnostics.lastKey},
      lastMouse:diagnostics.lastMouse && {...diagnostics.lastMouse},
    };
  }
  control(action) {
    const diagnostics = this.diagnostics;
    if (!diagnostics) return;
    diagnostics.controls[action] = (diagnostics.controls[action] ?? 0) + 1;
    diagnostics.lastControl = action;
  }
  recordEvent(kind, event, accepted) {
    try {
      const diagnostics = this.diagnostics;
      const observation = {
        type:event.type,active:this.active,accepted,defaultPrevented:!!event.defaultPrevented,
        isTrusted:!!event.isTrusted,
      };
      if (kind === 'key') {
        diagnostics.keyEvents++;
        diagnostics[accepted ? 'keyAccepted' : 'keyRejected']++;
        if (SCAN[event.code] === undefined) diagnostics.unknownKeyEvents++;
        diagnostics.lastKey = {...observation,key:event.key ?? null,code:event.code ?? null};
      } else {
        diagnostics.mouseEvents++;
        diagnostics[accepted ? 'mouseAccepted' : 'mouseRejected']++;
        diagnostics.lastMouse = {...observation,button:event.button ?? null};
      }
    } catch {}
  }
  get physicalAvailable() { return !this.blocked && this.windowFocused && !document.hidden && document.hasFocus() && (this.locked || document.activeElement === this.canvas); }
  get available() { return this.enabled && this.physicalAvailable; }
  get active() { return this.focused && this.available; }
  enable() {
    this.control('enable'); this.enabled = true; this.preserveFocus = false; this.windowFocused = document.hasFocus();
    if (!document.hidden && this.windowFocused) this.canvas.focus();
    if (!this.physicalAvailable) this.release(true); else this.focus();
  }
  disable({preserveFocus = false} = {}) {
    this.control('disable'); this.release(!preserveFocus); this.enabled = false; this.preserveFocus = preserveFocus;
    if (this.locked) document.exitPointerLock();
  }
  focus() {
    if (!this.available || this.focused) return;
    this.sendTimed([{kind:'focus',focused:true}, ...(this.locked ? [{kind:'capture',captured:true}] : [])]);
    this.focused = true;
  }
  setBlocked(value) {
    this.control(value ? 'block' : 'unblock');
    this.blocked = value;
    if (value) { this.release(true); if (this.locked) document.exitPointerLock(); }
    else this.focus();
  }
  key(event, down) {
    let accepted = false;
    try {
    if (!this.active) return;
    const code = SCAN[event.code];
    if (code === undefined) return;
    event.preventDefault();
    if (down && this.keys.has(code)) return;
    if (!down && !this.keys.has(code)) return;
    this.flush();
    if (down) this.keys.add(code); else this.keys.delete(code);
    this.sendTimed([{kind:'key',code,down}],this.timestamp(event));
    accepted = true;
    } finally { if (this.diagnostics) this.recordEvent('key', event, accepted); }
  }
  move(event) {
    if (!this.active || (!this.locked && event.target !== this.canvas)) return;
    if (!this.locked) {
      const point = this.point(event);
      if (!point) return;
      this.absolute = point;
    }
    this.relative.dx += Number.isFinite(event.movementX) ? event.movementX : 0;
    this.relative.dy += Number.isFinite(event.movementY) ? event.movementY : 0;
    this.motionTimestampMs = this.timestamp(event);
    this.schedule();
  }
  point(event) {
    const {canvas,x,y,width = canvas.width,height = canvas.height} = this.viewport();
    const rectangle = canvas.getBoundingClientRect();
    const scale = Math.min(rectangle.width / canvas.width, rectangle.height / canvas.height);
    if (!Number.isFinite(scale) || scale <= 0) return null;
    const localX = (event.clientX - rectangle.left - (rectangle.width - canvas.width * scale) / 2) / scale;
    const localY = (event.clientY - rectangle.top - (rectangle.height - canvas.height * scale) / 2) / scale;
    if (localX < 0 || localY < 0 || localX >= canvas.width || localY >= canvas.height) return null;
    return {x:integer(x + Math.floor(localX * width / canvas.width)),y:integer(y + Math.floor(localY * height / canvas.height))};
  }
  schedule() { if (!this.frame) this.frame = requestAnimationFrame(() => this.flush()); }
  flush() {
    cancelAnimationFrame(this.frame); this.frame = 0;
    if (!this.active) { this.relative = {dx:0,dy:0,wheel:0}; this.absolute = null; this.motionTimestampMs = undefined; return; }
    const {dx,dy,wheel} = this.relative;
    const event = {kind:'relative',dx:integer(dx),dy:integer(dy),wheel:integer(wheel)};
    this.relative = {dx:dx-event.dx,dy:dy-event.dy,wheel:wheel-event.wheel};
    const events = [];
    if (event.dx || event.dy || event.wheel) events.push(event);
    if (this.absolute) events.push({kind:'pointer',...this.absolute});
    this.absolute = null;
    const timestampMs = this.motionTimestampMs;
    this.motionTimestampMs = undefined;
    if (events.length) this.sendTimed(events,timestampMs);
  }
  release(blur) {
    cancelAnimationFrame(this.frame); this.frame = 0;
    const events = [];
    for (const code of this.keys) events.push({kind:'key',code,down:false});
    for (const button of this.buttons) events.push({kind:'button',button,down:false});
    this.keys.clear(); this.buttons.clear();
    this.relative = {dx:0,dy:0,wheel:0};
    this.absolute = null;
    this.motionTimestampMs = undefined;
    if (blur && this.focused) events.push({kind:'focus',focused:false});
    if ((this.enabled || this.preserveFocus) && events.length) this.sendTimed(events);
    if (blur) this.focused = false;
  }
  async capture() {
    if (!this.enabled || this.blocked || !this.windowFocused || document.hidden || !document.hasFocus()) return;
    this.canvas.focus();
    this.focus();
    if (!this.active) return;
    if (!this.canvas.requestPointerLock) throw new Error('pointer lock is unavailable in this browser');
    await this.canvas.requestPointerLock();
  }
  applyCursor(cursor) {
    this.canvas.style.cursor = cursor.visible ? 'default' : 'none';
  }
  close() { this.disable(); this.abort.abort(); }
}
