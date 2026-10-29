import { pair } from "../engine.js";
import { frameProbeStamp } from "../session/frame-probe.js";

const MAX_DEVICES = 64;
const MAX_BATCH_BYTES = 32 * 1024 * 1024;

export class GraphicsTransport {
  constructor(engine, emit) {
    this.engine = engine;
    this.emit = emit;
    this.pending = null;
    this.nextRequest = 1;
    this.lastEpoch = null;
    this.selectedBatch = null;
    this.inventory = [];
    this.batches = new Map();
    this.deferredSealing = true;
    this.forceDemand = false;
    this.draining = null;
    this.failure = null;
    this.sealing = { flushRequests: 0, sealRequests: 0, partialSeals: 0, skippedSealChecks: 0,
      drainsStarted: 0, drainsCompleted: 0, drainsFailed: 0 };
    this.latency = engine.graphicsMeasurementsEnabled ? { count: 0, sumMs: 0, maximumMs: 0 } : null;
  }
  configureDeferredSealing(enabled, owner) {
    if (typeof enabled !== "boolean" || owner?.paused !== true || owner?.open !== false ||
        this.pending || this.draining || this.forceDemand)
      throw new Error("deferred sealing changes require a drained closed paused owner");
    this.engine.signal?.throwIfAborted();
    if (this.failure) throw this.failure;
    this.deferredSealing = enabled;
    return enabled;
  }
  requestFlush() {
    this.forceDemand = true;
    this.sealing.flushRequests++;
  }
  drain() {
    if (this.draining) return this.draining.promise;
    if (this.engine.signal?.aborted) return Promise.reject(this.engine.signal.reason);
    if (this.failure) return Promise.reject(this.failure);
    let resolve, reject;
    const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
    const onAbort = () => this.settleDrain(false, this.engine.signal.reason);
    this.draining = { promise, resolve, reject, onAbort, timer: null };
    this.draining.timer = setTimeout(() => this.fail(new Error("native graphics drain timed out")), 5000);
    this.engine.signal?.addEventListener("abort", onAbort, { once: true });
    this.sealing.drainsStarted++;
    this.requestFlush();
    void this.output().catch(() => {});
    return promise;
  }
  settleDrain(completed, error) {
    const draining = this.draining;
    if (!draining) return;
    this.draining = null;
    clearTimeout(draining.timer);
    this.engine.signal?.removeEventListener("abort", draining.onAbort);
    if (completed) { this.sealing.drainsCompleted++; draining.resolve(); }
    else { this.sealing.drainsFailed++; draining.reject(error); }
  }
  fail(error) {
    this.failure ??= error;
    this.settleDrain(false, error);
  }
  snapshotInventory() {
    const rows = [];
    for (let index = 0; index <= MAX_DEVICES; index++) {
      const device = this.engine.media("d3d8_device", index);
      if (!device.epoch) return rows;
      if (index === MAX_DEVICES) throw new Error("D3D device inventory exceeds64");
      if (rows.some((row) => row.epoch === device.epoch))
        throw new Error("duplicate D3D device inventory epoch");
      rows.push({
        epoch: device.epoch,
        pointer: Number(device.values[0]),
        generation: device.values[1],
        retired: !!device.values[2],
        status: Number(device.values[3]),
      });
    }
    return rows;
  }
  async output() {
    if (this.failure) throw this.failure;
    if (this.pending || this.engine.signal?.aborted) return;
    try {
      const h = this.engine;
      const probe = this.probe?.();
      // sealing can reclaim a retired device and shift the indexed inventory.
      this.inventory = this.snapshotInventory();
      const previous = this.inventory.findIndex((row) => row.epoch === this.lastEpoch);
      const start = (previous + 1) % (this.inventory.length || 1);
      for (let offset = 0; offset < this.inventory.length; offset++) {
        const device = this.inventory[(start + offset) % this.inventory.length];
        let batch = h.media("d3d8_batch_info", ...pair(device.epoch));
        if (!batch.values[0]) {
          if (!this.deferredSealing || this.forceDemand || device.retired || probe) {
            this.sealing.sealRequests++;
            batch = h.media("d3d8_seal_batch", ...pair(device.epoch));
            if (batch.values[0]) this.sealing.partialSeals++;
          } else this.sealing.skippedSealChecks++;
        }
        if (!batch.values[0]) continue;
        const sequence = batch.values[0],
          size = Number(batch.values[1]);
        if (!Number.isSafeInteger(size) || size < 64 || size > MAX_BATCH_BYTES)
          throw new Error("D3D batch exceeds playground transport budget");
        const bytes = new Uint8Array(size);
        for (let at = 0; at < size; at += 4096) {
          if (h.signal?.aborted) return;
          const length = Math.min(4096, size - at);
          h.ok("d3d8_read_batch", ...pair(device.epoch), ...pair(sequence), at, length);
          bytes.set(h.take(length), at);
        }
        if (h.signal?.aborted) return;
        const requestId = this.nextRequest++;
        this.pending = { requestId, epoch: device.epoch, sequence };
        if (this.latency) this.pending.sentAtMs = performance.now();
        this.selectedBatch = { ...this.pending, bytes: size };
        this.lastEpoch = device.epoch;
        if (!this.batches.has(device.epoch)) {
          if (this.batches.size === MAX_DEVICES)
            this.batches.delete(this.batches.keys().next().value);
          this.batches.set(device.epoch, { sent: 0, acknowledged: 0, bytes: 0 });
        }
        const counters = this.batches.get(device.epoch);
        counters.sent++;
        counters.bytes += size;
        const frameProbe = probe?.batch(this.pending);
        this.probePending = frameProbe ? { probe, info: frameProbe, sent: frameProbeStamp() } : null;
        this.emit({ type: "graphics", requestId, packet: bytes.buffer,
          ...(frameProbe ? { frameProbe } : {}),
        }, [bytes.buffer]);
        return;
      }
      this.forceDemand = false;
      this.settleDrain(true);
    } catch (error) {
      this.fail(error);
      throw error;
    }
  }
  complete(message) {
    try {
      const h = this.engine,
        pending = this.pending;
      if (!pending || h.signal?.aborted || message.requestId !== pending.requestId) return false;
      if (BigInt(message.epoch) !== pending.epoch || BigInt(message.sequence) !== pending.sequence)
        throw new Error("GPU completion identity mismatch");
      const probe = this.probePending?.probe;
      const received = probe ? frameProbeStamp() : null;
      for (const readback of message.readbacks ?? []) {
        h.request(readback.receipt);
        h.ok("begin_d3d8_readback_input");
        try {
          for (let at = 0; at < readback.bytes.length; at += 4096) {
            const bytes = readback.bytes.subarray(at, at + 4096);
            h.request(bytes);
            h.ok("append_d3d8_readback_input", at, bytes.length);
          }
          h.ok("finish_d3d8_readback_input");
        } catch (error) {
          try {
            h.ok("abort_d3d8_readback_input");
          } catch {}
          throw error;
        }
      }
      h.ok("d3d8_acknowledge_batch", ...pair(pending.epoch), ...pair(pending.sequence));
      this.batches.get(pending.epoch).acknowledged++;
      if (this.latency) {
        const elapsed = performance.now() - pending.sentAtMs;
        this.latency.count++;
        this.latency.sumMs += elapsed;
        this.latency.maximumMs = Math.max(this.latency.maximumMs, elapsed);
      }
      if (probe) {
        probe.acknowledged(this.probePending, message.frameProbeTimeline, received,
          frameProbeStamp(), message.presentCommands ?? []);
      }
      this.probePending = null;
      this.pending = null;
      if (this.draining) void this.output().catch(() => {});
      return true;
    } catch (error) {
      this.fail(error);
      throw error;
    }
  }
  status() {
    return {
      deferredSealing: this.deferredSealing,
      forceDemand: this.forceDemand,
      draining: !!this.draining,
      sealing: { ...this.sealing,
        skippedSealChecksSemantics: "normal device checks where sealing was skipped; queues may be empty" },
      ...(this.latency ? { completionLatency: {
        semantics: "Worker batch post to successful engine ACK; host consume and scheduling delay, with GPU completion tracked separately",
        ...this.latency,
        pendingAgeMs: this.pending ? performance.now() - this.pending.sentAtMs : null,
      } } : {}),
      inventory: this.inventory.map((row) => ({
        ...row,
        epoch: String(row.epoch),
        generation: String(row.generation),
      })),
      pending: this.pending
        ? {
            ...this.pending,
            epoch: String(this.pending.epoch),
            sequence: String(this.pending.sequence),
          }
        : null,
      selectedBatch: this.selectedBatch
        ? {
            ...this.selectedBatch,
            epoch: String(this.selectedBatch.epoch),
            sequence: String(this.selectedBatch.sequence),
          }
        : null,
      batchesByEpoch: Array.from(this.batches, ([epoch, counters]) => ({
        epoch: String(epoch),
        ...counters,
      })),
    };
  }
}
