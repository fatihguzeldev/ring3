import { epochNowNs, encodeCategorySelection } from "../../observability/typescript/dist/index.js";
import { loadEngineModule } from "./engine/module.js";

const executionFactories = new WeakMap();

export function ownedExecutionBindings(engine, required = false) {
  const record = executionFactories.get(engine);
  const valid = record && record.safe && engine.memory === record.memory && engine.base === record.base &&
    engine.api === record.publicApi && Object.entries(record.native).every(([name, value]) => engine.api[name] === value);
  if (!valid) {
    if (required) throw new Error("owned execution requires an unchanged factory engine with native helpers and unshared memory");
    return null;
  }
  return record.native;
}

export const KEY = [0x50334731, 0x62726f77];
export const RESIDENT_PAGES = 32768;
export const pair = (value) => [Number(BigInt(value) & 0xffffffffn), Number(BigInt(value) >> 32n)];
export const view = (bytes) => new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
export const utf8 = (text) => new TextEncoder().encode(text);
export const utf16 = (text) => {
  const bytes = new Uint8Array(text.length * 2),
    data = view(bytes);
  for (let i = 0; i < text.length; i++) data.setUint16(i * 2, text.charCodeAt(i), true);
  return bytes;
};
export function record(magic, size, profile = 1) {
  const bytes = new Uint8Array(size),
    data = view(bytes);
  bytes.set(utf8(magic));
  data.setUint16(4, 1, true);
  data.setUint16(6, profile, true);
  data.setUint32(8, size, true);
  return bytes;
}
export class Engine {
  static async create(url, pages = RESIDENT_PAGES, signal, compiled) {
    const { module, hash } = await loadEngineModule(url, signal, compiled);
    const instance = await WebAssembly.instantiate(module, {
      ring3_observability: { now_ns: epochNowNs },
    });
    const api = Object.fromEntries(
      Object.entries(instance.exports)
        .filter(([name]) => name.startsWith("ring3_abi_v1_"))
        .map(([name, value]) => [name.slice(13), value]),
    );
    signal?.throwIfAborted();
    const engine = new Engine(api, instance.exports.memory, hash);
    engine.signal = signal;
    engine.module = module;
    engine.ok("open", pages, ...KEY);
    engine.base = api.arena_ptr() >>> 0;
    const imports = WebAssembly.Module.imports(module);
    executionFactories.set(engine, Object.freeze({
      publicApi: api, native: Object.freeze({ ...api }), memory: engine.memory, base: engine.base,
      safe: imports.every(item => item.kind === "function" && item.module === "ring3_observability" && item.name === "now_ns") &&
        !(typeof SharedArrayBuffer !== "undefined" && engine.memory.buffer instanceof SharedArrayBuffer) &&
        ["begin_owned_execution_receipt", "finish_execution_receipt"].every(name => typeof api[name] === "function"),
    }));
    return engine;
  }
  constructor(api, memory, hash) {
    this.api = api;
    this.memory = memory;
    this.hash = hash;
    this.base = 0;
    this.epoch = 0n;
  }
  observabilityPort() {
    const host = this;
    const read = (operation) => {
      host.ok(operation);
      const packet = host.take(4096),
        header = view(packet),
        size = header.getUint32(0, true);
      if (size > 4080) throw new Error("invalid engine trace chunk");
      const chunk = {
        bytes: packet.slice(16, 16 + size),
        finished: header.getUint32(4, true) !== 0,
        droppedEvents: header.getBigUint64(8, true),
      };
      if (chunk.finished) {
        host.ok("observability_status");
        chunk.openScopes = view(host.take(40)).getBigUint64(32, true);
      }
      return chunk;
    };
    return {
      supported: typeof host.api.configure_observability === "function",
      configure({ level, maxBytes, sequenceId, trackUuid, categories }) {
        const selection = utf8(encodeCategorySelection(categories));
        host.request(selection);
        host.ok(
          "configure_observability",
          ["off", "basic", "detailed"].indexOf(level),
          maxBytes,
          sequenceId,
          ...pair(trackUuid),
          selection.length,
        );
      },
      drain: () => read("observability_drain"),
      finish: () => read("observability_finish"),
    };
  }
  get(offset) {
    return new DataView(this.memory.buffer).getUint32(this.base + offset, true);
  }
  take(length) {
    return new Uint8Array(this.memory.buffer).slice(this.base + 140, this.base + 140 + length);
  }
  request(bytes) {
    if (bytes.byteLength > 4096)
      throw new Error(`transport payload ${bytes.byteLength} exceeds4096`);
    new Uint8Array(this.memory.buffer, this.base + 140, bytes.byteLength).set(bytes);
  }
  name(text) {
    const bytes = utf8(text);
    this.request(bytes);
    return bytes.length;
  }
  status(operation, ...args) {
    const fn = this.api[operation];
    if (typeof fn !== "function") throw new Error(`missing engine export ${operation}`);
    return fn(...args);
  }
  ok(operation, ...args) {
    const status = this.status(operation, ...args);
    if (status !== 0)
      throw Object.assign(
        new Error(`${operation} returned${status} at0x${this.get(48).toString(16)}`),
        { operation, status, pc: this.get(48), reason: this.get(72), detail: this.get(80) },
      );
  }
  media(operation, ...args) {
    this.ok(operation, ...args);
    const data = view(this.take(64));
    return {
      epoch: data.getBigUint64(16, true),
      values: Array.from({ length: 5 }, (_, i) => data.getBigUint64(24 + i * 8, true)),
    };
  }
  async bytes(url, id, offset, length) {
    const read = () => this.readBytes(url, id, offset, length);
    return this.filesObserver?.enabled
      ? this.filesObserver.scopeAsync("read", { asset_id: id, offset, length }, read)
      : read();
  }
  async readBytes(url, id, offset, length) {
    if (this.localFiles) {
      this.signal?.throwIfAborted();
      const file = this.localFiles.get(BigInt(id)),
        start = Number(offset);
      if (
        !file ||
        !Number.isSafeInteger(start) ||
        !Number.isSafeInteger(length) ||
        start < 0 ||
        length < 0 ||
        start + length > file.size
      )
        throw new Error(`invalid local file span: ${id}@${offset}+${length}`);
      const bytes = new Uint8Array(await file.slice(start, start + length).arrayBuffer());
      this.signal?.throwIfAborted();
      if (bytes.length !== length) throw new Error(`local file ${id}: short read`);
      return bytes;
    }
    const endpoint = new URL(url, self.location.href);
    endpoint.searchParams.set("id", id.toString());
    endpoint.searchParams.set("offset", offset.toString());
    endpoint.searchParams.set("length", length.toString());
    const response = await fetch(endpoint, { signal: this.signal });
    if (!response.ok) throw new Error(`asset${id}@${offset}: HTTP${response.status}`);
    const bytes = new Uint8Array(await response.arrayBuffer());
    this.signal?.throwIfAborted();
    if (bytes.length !== length)
      throw new Error(`asset${id}: requested${length}, received${bytes.length}`);
    return bytes;
  }
}
