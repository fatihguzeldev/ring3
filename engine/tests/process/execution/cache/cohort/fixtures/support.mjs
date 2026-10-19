import assert from "node:assert/strict";
import { createHash } from "node:crypto";
export const KEY = [97, 0];
export const pair = (value) => [Number(value & 0xffffffffn), Number(value >> 32n)];
const hash = (bytes) => createHash("sha256").update(bytes).digest("hex");

export class Fixture {
  constructor(module, sources, limits) {
    const instance = new WebAssembly.Instance(module, {
      ring3_observability: { now_ns: () => BigInt(Date.now()) * 1000000n },
    });
    this.api = Object.fromEntries(
      Object.entries(instance.exports)
        .filter(([name]) => name.startsWith("ring3_abi_v1_"))
        .map(([name, value]) => [name.slice(13), value]),
    );
    this.memory = instance.exports.memory;
    this.ok("open", 16, ...KEY);
    this.base = this.api.arena_ptr() >>> 0;
    this.ok("map", 0x1000, 1, 7);
    this.ok("map", 0x8000, 1, 3);
    if (limits === undefined) this.ok("configure_execution", 8, 1048576, 8);
    else this.ok("configure_execution_with_limits", 8, 1048576, 8, limits.instructions, limits.wasmBytes);
    this.table = new WebAssembly.Table({ element: "anyfunc", initial: 8, maximum: 8 });
    for (const [pc, bytes] of sources) {
      this.put(Uint8Array.from(bytes));
      this.ok("upload", pc, bytes.length);
    }
    this.ok("dispatcher_module", ...KEY);
    const d = this.transfer(32),
      pointer = d.getUint32(24, true),
      length = d.getUint32(28, true);
    this.dispatch = this.instantiate(this.bytes(pointer, length), { table: this.table });
  }
  ok(name, ...args) {
    assert.equal(this.api[name](...args), 0, name);
  }
  bytes(pointer, size) {
    return new Uint8Array(this.memory.buffer).slice(pointer, pointer + size);
  }
  transfer(size) {
    return new DataView(this.memory.buffer, this.base + 140, size);
  }
  put(bytes) {
    new Uint8Array(this.memory.buffer, this.base + 140, bytes.length).set(bytes);
  }
  word(offset) {
    return new DataView(this.memory.buffer).getUint32(this.base + offset, true);
  }
  set(offset, value) {
    new DataView(this.memory.buffer).setUint32(this.base + offset, value, true);
  }
  request(name, ...args) {
    this.ok(name, ...args);
    const v = this.transfer(64);
    assert.equal(v.getUint32(0, true), 0x51433352);
    assert.equal(v.getUint32(8, true), 64);
    return {
      serial: v.getBigUint64(16, true),
      kind: v.getUint32(24, true),
      pc: v.getUint32(28, true),
      id: v.getBigUint64(32, true),
      victim: v.getBigUint64(40, true),
      slot: v.getUint32(48, true),
      victimSlot: v.getUint32(52, true),
      tier: v.getUint32(56, true),
    };
  }
  instantiate(bytes, extra = {}) {
    const module = new WebAssembly.Module(bytes),
      ring3 = {};
    for (const entry of WebAssembly.Module.imports(module))
      if (entry.module === "ring3") ring3[entry.name] = this.api[entry.name];
    return new WebAssembly.Instance(module, { env: { memory: this.memory, ...extra }, ring3 })
      .exports.run;
  }
  material(id) {
    this.ok("resident_module", ...pair(id));
    const d = this.transfer(24),
      pointer = d.getUint32(16, true),
      length = d.getUint32(20, true);
    const bytes = this.bytes(pointer, length);
    return { pointer, length, sha256: hash(bytes), bytes };
  }
  install(pc, request = this.request("prepare_cache_entry", ...KEY, pc)) {
    assert.equal(request.kind, 1);
    const material = this.material(request.id),
      run = this.instantiate(material.bytes);
    this.table.set(request.slot, run);
    this.ok(
      "install_cached_entry",
      ...KEY,
      ...pair(request.serial),
      ...pair(request.id),
      request.slot,
    );
    return { ...request, run, material };
  }
  legacy(entries, slot, gates = []) {
    this.put(new Uint8Array(new Uint32Array([...entries, ...gates.flat()]).buffer));
    this.ok("compile_resident_entries", entries.length, gates.length);
    const id = this.transfer(24).getBigUint64(8, true),
      material = this.material(id),
      run = this.instantiate(material.bytes);
    this.table.set(slot, run);
    this.ok("acknowledge_resident_installation", ...KEY, ...pair(id), slot);
    return { id, slot, run, material };
  }
  find(pc) {
    const status = this.api.find_installed_resident(...KEY, pc);
    if (status !== 0) return { status };
    const v = this.transfer(32);
    return { status, id: v.getBigUint64(16, true), slot: v.getUint32(24, true) };
  }
  promotion(pc = 0x1000) {
    for (let i = 0; i < 8; i++) assert.equal(this.find(pc).status, 0);
    return this.request("prepare_cache_promotion", ...KEY);
  }
  acknowledge(request) {
    this.table.set(request.victimSlot, null);
    this.ok(
      "acknowledge_cache_eviction",
      ...KEY,
      ...pair(request.serial),
      ...pair(request.victim),
      request.victimSlot,
    );
  }
  usage() {
    this.ok("cache_usage");
    const v = this.transfer(80);
    return {
      units: v.getUint32(16, true),
      installed: v.getUint32(24, true),
      evictions: v.getBigUint64(48, true),
    };
  }
  reset(pc = 0x1000, cancel = 0) {
    const bytes = new Uint8Array(this.memory.buffer, this.base, 140),
      v = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    bytes.fill(0);
    bytes.set(Buffer.from("R3ST"));
    v.setUint16(4, 3, true);
    v.setUint16(6, 1, true);
    v.setUint32(8, 56, true);
    [5, 0, 7, 0xdead0000, 0x8800, 0, 0, 0].forEach((value, i) =>
      v.setUint32(16 + i * 4, value, true),
    );
    v.setUint32(48, pc, true);
    v.setUint32(52, 2, true);
    v.setUint32(96, cancel, true);
  }
  execute(unit, budget, dispatch = true) {
    this.ok("begin_execution_receipt", ...KEY, budget, ...pair(unit.id), Number(dispatch));
    const token = this.transfer(32).getBigUint64(16, true);
    assert.equal(
      (dispatch ? this.dispatch : unit.run)(this.base, this.base + 56, budget, this.base + 96),
      0,
    );
    const state = this.bytes(this.base, 56),
      exit = this.bytes(this.base + 56, 40);
    this.ok("finish_execution_receipt", ...KEY, ...pair(token));
    const v = this.transfer(64);
    return {
      state,
      exit,
      retired: v.getUint32(36, true),
      reason: v.getUint32(44, true),
      owner: v.getBigUint64(24, true),
    };
  }
  close() {
    this.ok("close");
  }
}
