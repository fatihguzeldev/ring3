import { categories } from "../../../observability/typescript/dist/index.js";
import { KEY } from "../engine.js";

const bytesPerPage = 4096, headerBytes = 128, rowBytes = 80, rowsPerPage = 49;
const exports = ["begin_execution_census", "stop_execution_census", "execution_census_page"];
const hex = (bytes) => Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");

function packet(bytes, offset, capacity) {
  const d = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const u32 = (at) => d.getUint32(at, true), u64 = (at) => d.getBigUint64(at, true);
  if (bytes.length !== bytesPerPage || String.fromCharCode(...bytes.subarray(0, 4)) !== "R3CE" ||
      u32(4) !== 0x10001 || u32(8) !== bytesPerPage || u32(12) !== 0 || u64(16) === 0n ||
      u32(24) !== 2 || u32(28) !== capacity || capacity < 1 || capacity > 32768 ||
      u32(32) > capacity || u32(36) !== offset || offset > u32(32) ||
      u32(40) !== Math.min(rowsPerPage, u32(32) - offset) || u32(44) !== rowBytes || u32(124) !== 0)
    throw new Error("invalid execution census page header");
  if (bytes.subarray(headerBytes + u32(40) * rowBytes).some((byte) => byte !== 0))
    throw new Error("nonzero unused execution census bytes");
  const frozen = bytes.slice(0, headerBytes);
  frozen.fill(0, 36, 44);
  return { d, u32, u64, frozen, rows: u32(32), count: u32(40) };
}

// collection has no per-child host calls; fixed raw pages are read only after the window freezes.
export class ExecutionCensus {
  constructor(session, proofId, { capacity = 32768 } = {}) {
    this.session = session; this.proofId = proofId; this.capacity = capacity;
    this.observer = session.observation?.capture.observer(categories.engineCpuCache);
    if (!this.observer?.enabled || exports.some((name) => typeof session.engine.api[name] !== "function"))
      throw new Error("execution census requires engine.cpu.cache recording and an observability engine");
  }
  call(name, ...args) {
    const engine = this.session.engine, saved = engine.take(bytesPerPage);
    try {
      engine.ok(name, ...KEY, ...args);
      return name === "execution_census_page" ? engine.take(bytesPerPage) : null;
    } finally {
      // no await may occur while transfer scratch is borrowed from a live engine.
      engine.request(saved);
    }
  }
  begin() {
    if (this.started || this.finishing) return;
    const before = performance.now();
    this.call("begin_execution_census", this.capacity);
    this.started = true;
    this.beginWallMs = performance.now() - before;
  }
  stop() {
    if (!this.started || this.stopped) return;
    const before = performance.now();
    this.call("stop_execution_census");
    this.stopped = true;
    this.stopWallMs = performance.now() - before;
  }
  finish(reason) {
    this.finishing ??= this.drain(reason);
    return this.finishing;
  }
  async drain(reason) {
    const summary = { proof_id: this.proofId, complete: false, reason, pages: 0 };
    const started = performance.now();
    try {
      if (!this.started) throw new Error("execution census did not start");
      this.stop();
      let first, offset = 0, occurrences = 0n, retired = 0n, work = 0n;
      const keys = new Set();
      do {
        const bytes = this.call("execution_census_page", offset);
        const current = packet(bytes, offset, this.capacity);
        first ??= current;
        if (current.frozen.some((value, i) => value !== first.frozen[i]))
          throw new Error("execution census changed during readout");
        for (let i = 0; i < current.count; i++) {
          const at = headerBytes + rowBytes * i;
          const owner = current.u64(at), count = current.u64(at + 24);
          const rowRetired = current.u64(at + 32), rowWork = current.u64(at + 40);
          const minimum = current.u32(at + 48), maximum = current.u32(at + 52);
          const coverage = current.u32(at + 20);
          const key = [owner, current.u32(at + 8), current.u32(at + 12), current.u32(at + 16), coverage].join(":");
          if (owner === 0n || count === 0n || coverage < 1 || coverage > 3 || keys.has(key) ||
              minimum > maximum || rowRetired < BigInt(minimum) * count || rowRetired > BigInt(maximum) * count ||
              rowWork < rowRetired || current.u32(at + 68) > 1 || current.u32(at + 72) > 1 || current.u32(at + 76) !== 0)
            throw new Error("invalid execution census row");
          keys.add(key); occurrences += count; retired += rowRetired; work += rowWork;
        }
        this.observer.event("execution.census.page", { proof_id: this.proofId,
          generation: String(current.u64(16)), offset, packet_hex: hex(bytes) });
        const status = await this.session.observation.flush();
        if (!status || status.droppedEvents || status.droppedChunks || status.sinkError || status.recordingError)
          throw new Error(status?.sinkError ?? status?.recordingError ?? "execution census trace lost data");
        summary.pages++;
        offset += current.count;
      } while (offset < first.rows);
      const h = first;
      Object.assign(summary, { generation: String(h.u64(16)), rows: h.rows, flags: h.u32(120),
        entries: String(h.u64(48)), finalized: String(h.u64(56)), retired: String(h.u64(64)),
        work: String(h.u64(72)), dropped: String(h.u64(80)), unresolved: String(h.u64(104)),
        zero_child_receipts: String(h.u64(112)) });
      if (h.u64(48) !== h.u64(56) + h.u64(104) || occurrences + h.u64(80) !== h.u64(56) ||
          retired + h.u64(88) !== h.u64(64) || work + h.u64(96) !== h.u64(72))
        throw new Error("execution census totals do not conserve validated children");
      summary.complete = reason === "complete" && h.u32(120) === 0 && h.u64(80) === 0n && h.u64(104) === 0n;
    } catch (error) {
      summary.error = error.message;
    }
    Object.assign(summary, { begin_wall_ms: this.beginWallMs ?? 0, stop_wall_ms: this.stopWallMs ?? 0,
      readout_wall_ms: performance.now() - started,
      retention: "page flush acknowledgements; final capture completeness must also be checked" });
    this.session.emit({ type: "diagnostic", message: "execution-census-ready", detail: summary });
    return summary;
  }
}
