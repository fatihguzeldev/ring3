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
    this.latency = engine.graphicsMeasurementsEnabled ? { count: 0, sumMs: 0, maximumMs: 0 } : null;
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
    if (this.pending || this.engine.signal?.aborted) return;
    const h = this.engine;
    // sealing can reclaim a retired device and shift the indexed inventory.
    this.inventory = this.snapshotInventory();
    const previous = this.inventory.findIndex((row) => row.epoch === this.lastEpoch);
    const start = (previous + 1) % (this.inventory.length || 1);
    for (let offset = 0; offset < this.inventory.length; offset++) {
      const device = this.inventory[(start + offset) % this.inventory.length];
      let batch = h.media("d3d8_batch_info", ...pair(device.epoch));
      if (!batch.values[0]) batch = h.media("d3d8_seal_batch", ...pair(device.epoch));
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
      const probe = this.probe?.();
      const frameProbe = probe?.batch(this.pending);
      this.probePending = frameProbe ? { probe, info: frameProbe, sent: frameProbeStamp() } : null;
      this.emit({ type: "graphics", requestId, packet: bytes.buffer,
        ...(frameProbe ? { frameProbe } : {}),
      }, [bytes.buffer]);
      return;
    }
  }
  complete(message) {
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
    return true;
  }
  status() {
    return {
      ...(this.latency ? { completionLatency: {
        semantics: "Worker batch post to successful engine ACK; includes browser GPU work and Worker scheduling delay",
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
