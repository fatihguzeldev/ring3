import { KEY } from "../engine.js";

const size = 256;
const operations = ["begin_x87_precision_probe", "stop_x87_precision_probe", "x87_precision_probe"];
const fields = ["attempts", "unmasked", "nonPC24", "nonNearest", "rawOperand", "exponentGap", "exactZero", "outputRange", "accepted"];
const sum = values => values.reduce((total, value) => total + BigInt(value), 0n);
const hex = bytes => Array.from(bytes, byte => byte.toString(16).padStart(2, "0")).join("");

function decode(bytes, previousGeneration) {
  const d = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const u32 = offset => d.getUint32(offset, true), u64 = offset => d.getBigUint64(offset, true);
  if (bytes.length !== size || String.fromCharCode(...bytes.subarray(0, 4)) !== "R3XP" ||
      u32(4) !== 0x10001 || u32(8) !== size || u32(12) !== 0 || u32(24) > 1 || u32(28) !== 0 ||
      bytes.subarray(240).some(byte => byte !== 0)) throw new Error("invalid x87 precision probe header");
  const generation = u64(16);
  if (generation === 0n || (previousGeneration !== undefined && generation <= BigInt(previousGeneration)))
    throw new Error("invalid x87 precision probe generation");
  const family = offset => Object.fromEntries(fields.map((name, i) => [name, String(u64(offset + i * 8))]));
  const add = family(32), multiply = family(104);
  const pc = Array.from({ length: 4 }, (_, i) => String(u64(176 + i * 8)));
  const rc = Array.from({ length: 4 }, (_, i) => String(u64(208 + i * 8)));
  if (multiply.exponentGap !== "0") throw new Error("invalid x87 precision multiply gap counter");
  if (u32(24) === 0) {
    for (const values of [add, multiply])
      if (BigInt(values.attempts) !== sum(fields.slice(1).map(name => values[name])))
        throw new Error("x87 precision probe partition mismatch");
    const attempts = BigInt(add.attempts) + BigInt(multiply.attempts);
    if (sum(pc) !== attempts || sum(rc) !== attempts)
      throw new Error("x87 precision probe histogram mismatch");
  }
  return { generation: String(generation), flags: u32(24), add_sub: add, multiply,
    pc_histogram: pc, rc_histogram: rc };
}

// Only the first/ninth successful Present hooks call these cold APIs. No helper is wrapped.
export class X87PrecisionProbe {
  constructor(session, now) {
    this.session = session;
    this.now = now;
    this.started = false;
    this.stopped = false;
  }
  call(name, read = false) {
    const engine = this.session.engine, saved = engine.take(size);
    try {
      engine.ok(name, ...KEY);
      return read ? engine.take(size).slice() : null;
    } finally {
      // Restore the exact transfer loan before decoding, emitting or returning to the loop.
      engine.request(saved);
    }
  }
  timed(name, detail, prefix, read = false) {
    detail[`${prefix}_before_ns`] = String(this.now());
    try {
      return this.call(name, read);
    } finally {
      detail[`${prefix}_after_ns`] = String(this.now());
    }
  }
  begin() {
    if (this.beginInfo) return this.beginInfo;
    const detail = this.beginInfo = { event: "begin", started: false };
    try {
      if (operations.some(name => typeof this.session.engine.api[name] !== "function"))
        throw new Error("x87 precision probe exports are unavailable");
      this.timed(operations[0], detail, "begin");
      this.started = detail.started = true;
    } catch (error) {
      detail.error = error.message;
    }
    return detail;
  }
  stopAndRead() {
    if (this.result) return this.result;
    const detail = this.result = { event: "stop-read", ...this.beginInfo,
      started: this.started, stopped: false };
    detail.event = "stop-read";
    try {
      if (!this.started) throw new Error(this.beginInfo?.error ?? "x87 precision probe did not start");
      this.timed(operations[1], detail, "stop");
      this.stopped = detail.stopped = true;
      const bytes = this.timed(operations[2], detail, "read", true);
      detail.packet_hex = hex(bytes);
      Object.assign(detail, decode(bytes, this.session.x87PrecisionProbeGeneration));
      this.session.x87PrecisionProbeGeneration = detail.generation;
    } catch (error) {
      detail.error = error.message;
    }
    return detail;
  }
  stopAtNinth() {
    this.ninth = true;
    return this.stopAndRead();
  }
  finish(reason) {
    const result = this.stopAndRead();
    const summary = { ...result, event: "finished", proof_reason: reason,
      partial: !this.ninth || reason !== "complete",
      complete: reason === "complete" && this.ninth === true && this.stopped &&
        !result.error && result.flags === 0 };
    if (this.ninth) {
      delete summary.packet_hex;
      summary.packet_location = "producer-present-9";
    }
    return summary;
  }
}
