import {view} from './engine.js';

const creates = new Set([0x20007, 0x20008]);
const mutations = new Set([0x20012,0x20013,0x20014,0x20023,0x20024,0x200de,0x20117]);
const deferredPositions = 0x200df;
const presentationChanges = new Set([0x4010f, 0x4020e]);
export class WindowPresentation {
  constructor(engine, emit) {
    this.engine = engine;
    this.emit = emit;
    this.known = new Map();
    this.pending = [];
  }
  capture(receipt, frame, thread) {
    if (!creates.has(receipt.id) && !mutations.has(receipt.id) && !presentationChanges.has(receipt.id) && receipt.id !== deferredPositions && receipt.id !== 0x20009) return;
    const words = frame.getUint32(28, true);
    this.pending.push({
      receipt, thread,
      returnPc: frame.getUint32(40, true),
      returnEsp: (frame.getUint32(36, true) + 4 + words * 4) >>> 0,
    });
  }
  settle(thread) {
    const h = this.engine;
    for (let i = this.pending.length - 1; i >= 0; i--) {
      const entry = this.pending[i];
      if (entry.thread !== thread || entry.returnPc !== h.get(48) || entry.returnEsp !== h.get(32)) continue;
      this.pending.splice(i, 1);
      const result = h.get(16);
      const {id,callArgs} = entry.receipt;
      if (creates.has(id)) { if (result) this.publish(result); }
      else if (presentationChanges.has(id)) {
        if (result === 0) for (const handle of [...this.known.keys()]) this.publish(handle);
      }
      else if (id === 0x20009) { if (result) this.remove(callArgs[0]); }
      else if (id === deferredPositions) {
        for (const handle of [...this.known.keys()]) this.publish(handle);
      } else this.refresh(callArgs[0]);
    }
  }
  refresh(handle) {
    if (!this.known.has(handle) || !this.publish(handle)) return;
    for (const item of [...this.known.values()]) if (item.parent === handle) this.refresh(item.handle);
  }
  read(handle) {
    const h = this.engine;
    h.ok('user32_window_info', handle);
    const d = view(h.take(80));
    if (d.getUint32(0, true) !== 0x57553352 || d.getUint32(8, true) !== 80) throw new Error('invalid window snapshot');
    if (!d.getUint32(12, true)) return null;
    const item = {
      handle: d.getUint32(16, true), parent: d.getUint32(20, true), owner: d.getUint32(24, true), thread: d.getUint32(28, true),
      style: d.getUint32(32, true), exStyle: d.getUint32(36, true),
      left: d.getInt32(40, true), top: d.getInt32(44, true), right: d.getInt32(48, true), bottom: d.getInt32(52, true),
      clientX: d.getInt32(56, true), clientY: d.getInt32(60, true),
      clientWidth: d.getUint32(64, true), clientHeight: d.getUint32(68, true),
      visible: !!d.getUint32(72, true), enabled: !!d.getUint32(76, true),
    };
    if (item.handle !== handle || item.right < item.left || item.bottom < item.top) throw new Error('invalid canonical window geometry');
    return item;
  }
  publish(handle, seen = new Set()) {
    if (seen.has(handle) || seen.size >= 256) throw new Error('cyclic or excessive window ancestry');
    seen.add(handle);
    const item = this.read(handle);
    if (!item) {this.remove(handle); return false;}
    if (item.parent && !this.publish(item.parent, seen)) {this.remove(handle); return false;}
    const parent = this.known.get(item.parent);
    item.x = item.left - (parent?.clientX ?? 0);
    item.y = item.top - (parent?.clientY ?? 0);
    item.width = item.right - item.left;
    item.height = item.bottom - item.top;
    const previous = this.known.get(handle);
    if (!previous || Object.keys(item).some(key => item[key] !== previous[key])) {
      this.known.set(handle, item);
      this.emit({type:'window', ...item});
    }
    return true;
  }
  remove(handle) {
    if (!this.known.delete(handle)) return;
    this.emit({type:'window', handle, destroyed:true});
    for (const item of [...this.known.values()]) if (item.parent === handle) this.remove(item.handle);
  }
}
