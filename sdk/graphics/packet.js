export function require(condition, message) {
  if (!condition) throw new Error(`D3D8 host: ${message}`);
}

export class Reader {
  constructor(bytes) {
    this.bytes = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes);
    this.view = new DataView(this.bytes.buffer, this.bytes.byteOffset, this.bytes.byteLength);
    this.at = 0;
  }
  advance(length) {
    require(Number.isSafeInteger(length) && length >= 0 && this.at + length <= this.bytes.length, 'truncated payload');
    const at = this.at;
    this.at += length;
    return at;
  }
  take(length) { const at = this.advance(length); return this.bytes.subarray(at, this.at); }
  u32() { return this.view.getUint32(this.advance(4), true); }
  u64() { return this.view.getBigUint64(this.advance(8), true); }
  f32() { return this.view.getFloat32(this.advance(4), true); }
  words(count) {
    require(count <= 65536, 'word array limit');
    const values = new Array(Math.max(0, Math.trunc(count))), at = this.advance(values.length * 4);
    for (let i = 0; i < values.length; i++) values[i] = this.view.getUint32(at + i * 4, true);
    return values;
  }
  floats(count) {
    require(count <= 65536, 'float array limit');
    const values = new Array(Math.max(0, Math.trunc(count))), at = this.advance(values.length * 4);
    for (let i = 0; i < values.length; i++) values[i] = this.view.getFloat32(at + i * 4, true);
    return values;
  }
  count(max = 65536) { const n = this.u32(); require(n <= max, 'array limit'); return n; }
  resource() { const epoch = this.u64(), slot = this.u32(), generation = this.u32(); require(epoch && generation, 'zero resource identity'); return {epoch, slot, generation, key: `${epoch}:${slot}:${generation}`}; }
  version() { const id = this.resource(), version = this.u64(); require(version, 'zero resource version'); return {...id, version}; }
  surface() { const result = {...this.version(), subresource: this.u32()}; require(this.u32() === 0, 'surface reserved word'); return result; }
  targets() { const color = this.surface(), present = this.u32(); require(present <= 1, 'depth presence'); return {color, depth: present ? this.surface() : null}; }
  rect() { return this.words(4); }
  end() { require(this.at === this.bytes.length, `trailing payload bytes ${this.bytes.length - this.at}`); }
}

export class PacketStream {
  constructor() { this.epoch = null; this.sequence = 0n; this.command = 0n; this.pending = null; }
  decode(packet) {
    const r = new Reader(packet), header = r.take(64), v = new DataView(header.buffer, header.byteOffset, 64);
    require(String.fromCharCode(...header.subarray(0, 4)) === 'R3D8' && v.getUint16(4, true) === 2 && v.getUint16(6, true) === 8, 'packet magic/version/profile');
    require(v.getUint32(8, true) === r.bytes.length && header.subarray(48).every(x => x === 0), 'packet length/reserved');
    const count = v.getUint32(12, true), epoch = v.getBigUint64(16, true), sequence = v.getBigUint64(24, true);
    const first = v.getBigUint64(32, true), last = v.getBigUint64(40, true);
    require(epoch && first && first <= last && count > 0 && count <= (r.bytes.length - 64) / 32, 'packet identities');
    require((this.epoch === null || epoch === this.epoch) && sequence === this.sequence + 1n, 'stale/out-of-order batch');
    const commands = [];
    for (let i = 0; i < count; i++) {
      const start = r.at, h = r.take(32), q = new DataView(h.buffer, h.byteOffset, 32);
      const opcode = q.getUint16(0, true), flags = q.getUint16(2, true), size = q.getUint32(4, true), id = q.getBigUint64(8, true);
      const offset = q.getUint32(16, true), total = q.getUint32(20, true);
      require(opcode >= 1 && opcode <= 26 && !(flags & ~3) && size >= 32 && total <= 256 * 1024 * 1024 && h.subarray(24).every(x => x === 0), 'fragment header');
      const part = r.take(size - 32);
      require(offset + part.length <= total && Boolean(flags & 1) === (offset === 0) && Boolean(flags & 2) === (offset + part.length === total), 'fragment extent/flags');
      require(i !== 0 || id === first, 'first command mismatch');
      if (flags & 1) {
        require(!this.pending && id === this.command + 1n, 'command replay/order');
        this.pending = {opcode, id, bytes: new Uint8Array(total), received: 0};
      }
      const p = this.pending;
      require(p && p.id === id && p.opcode === opcode && p.bytes.length === total && p.received === offset, 'fragment continuation');
      p.bytes.set(part, offset); p.received += part.length;
      if (flags & 2) { commands.push(p); this.pending = null; this.command = id; }
      require(r.at === start + size, 'fragment length');
      if (i === count - 1) require(id === last, 'last command mismatch');
    }
    r.end(); this.epoch = epoch; this.sequence = sequence;
    return {epoch, sequence, commands};
  }
}

export function bindings(r) {
  const textures = Array.from({length: 8}, () => r.u32() ? {...r.version(), lod: r.u32()} : null);
  const streams = Array.from({length: 16}, () => r.u32() ? {...r.version(), stride: r.u32()} : null);
  const indices = r.u32() ? {...r.version(), format: r.u32(), baseVertex: r.u32()} : null;
  return {textures, streams, indices};
}

const identity = () => [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1];
export function pipeline(r) {
  require(r.u32() === 2, 'pipeline version');
  const render = r.words(r.count(174)); require(render.length === 174, 'render state size');
  const stages = Array.from({length: r.count(8)}, () => r.words(r.count(29))); require(stages.length === 8 && stages.every(s => s.length === 29), 'texture stage size');
  const count = r.count(266), mask = r.words(r.count(9)); require(count === 266 && mask.length === 9, 'transform mask');
  const transforms = Array.from({length: count}, (_, i) => mask[i >> 5] & (1 << (i & 31)) ? r.floats(16) : identity());
  const material = r.floats(17);
  const lights = Array.from({length: r.count(256)}, () => ({index: r.u32(), enabled: r.u32(), kind: r.u32(), diffuse: r.floats(4), specular: r.floats(4), ambient: r.floats(4), position: r.floats(3), direction: r.floats(3), range: r.f32(), falloff: r.f32(), attenuation: r.floats(3), theta: r.f32(), phi: r.f32()}));
  const planes = Array.from({length: r.count(6)}, () => r.floats(4)), clipStatus = r.words(2);
  const viewport = {x: r.u32(), y: r.u32(), width: r.u32(), height: r.u32(), min: r.f32(), max: r.f32()};
  const vertexShader = r.u32(), pixelShader = r.u32(), palette = r.u32();
  const vc = Array.from({length: r.count(96)}, () => r.floats(4)), pc = Array.from({length: r.count(8)}, () => r.floats(4));
  require(vc.length === 96 && pc.length === 8, 'constant counts');
  return {render, stages, transforms, material, lights, planes, clipStatus, viewport, vertexShader, pixelShader, palette, vc, pc};
}

export function shader(r) {
  const stage = r.u32(); require(stage === 1 || stage === 2, 'shader stage');
  let declaration = null;
  if (stage === 1) {
    declaration = {elements: Array.from({length: r.count(17)}, () => ({stream: r.u32(), offset: r.u32(), register: r.u32(), kind: r.u32()})), strides: r.words(16), constants: Array.from({length: r.count(96)}, () => [r.u32(), r.floats(4)]), tessellation: Array.from({length: r.count(17)}, () => r.words(3))};
    if (!r.u32()) return {stage, declaration, program: null};
  }
  require(r.u32() === stage, 'IR stage'); const minor = r.u32();
  const constants = Array.from({length: r.count(96)}, () => [r.u32(), r.floats(4)]);
  const instructions = Array.from({length: r.count(512)}, () => {
    const operation = r.u32(), coissue = r.u32();
    const destination = r.u32() ? {file: r.u32(), index: r.u32(), mask: r.u32(), saturate: r.u32(), shift: r.u32() | 0} : null;
    const sources = Array.from({length: 3}, () => r.u32() ? {file: r.u32(), index: r.u32(), swizzle: r.u32(), modifier: r.u32(), relative: r.u32()} : null);
    const immediate = r.u32() ? r.floats(4) : null;
    return {operation, coissue, destination, sources, immediate};
  });
  return {stage, declaration, program: {stage, minor, constants, instructions}};
}
