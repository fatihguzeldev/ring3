import { ExecutionCensus } from "./execution-census.js";
import { X87PrecisionProbe } from "./x87-precision-probe.js";
// Default-off, bounded host observation. Successful Present return is bracketed. A draining enqueue can precede this return.
const origin = BigInt(Math.floor(performance.timeOrigin * 1e6));
export const frameProbeNow = () => origin + BigInt(Math.floor(performance.now() * 1e6));
export const frameProbeStamp = () => ({ origin_ns: String(origin), timestamp_ns: String(frameProbeNow()) });
export class FrameProbe {
  constructor(id, session) {
    this.id = id; this.session = session; this.accepted = 0; this.completed = 0;
    this.sequence = 0; this.batches = 0; this.attempts = 0; this.retries = 0;
    this.closed = false; this.openTaggedBatches = 0;
    this.census = session.frameProbeCensusEnabled ? new ExecutionCensus(session, id) : null;
    this.math = session.frameProbeMathEnabled ? new X87PrecisionProbe(session, frameProbeNow) : null;
    this.event("armed", { target_intervals: 8, required_producer_edges: 9,
      engine_sha256: session.engine.hash, receipt_mode: session.code.receiptMode, callback_receipt_mode: "checked", clean_graphics_boundary: true,
      measurement_enabled: !!session.measurements, execution_census_enabled: !!this.census,
      ...(this.math ? { x87_precision_probe_enabled: true } : {}),
      semantics: "successful native Present return brackets; enqueue may precede Busy retry completion; GPU completion is Promise-observed, not compositor visibility" });
  }
  event(event, detail = {}) {
    if (this.closed && event !== "finished") return;
    this.session.emit({ type: "diagnostic", message: "frame-probe", detail: {
      schema: 1, proof_id: this.id, realm: "worker", local_sequence: ++this.sequence,
      ...frameProbeStamp(), event, ...detail,
    } });
  }
  snapshot() {
    this.session.code.sampleUsage();
    const compiledUsage = this.session.code.sampleCompiledUsage?.() ?? null;
    return { counters_sampled_at_ns: String(frameProbeNow()), phases: this.session.measurements?.snapshot() ?? null,
      compiled_execution_usage: compiledUsage,
      stats: this.session.code.stats(), pc: this.session.engine.get(48), thread: this.session.thread,
      physicalInputFocus: this.session.media?.lastPhysicalFocus
        ? { ...this.session.media.lastPhysicalFocus } : null };
  }
  present(receipt, status, hresult, before, after, guestClock) {
    if (this.closed) return;
    if (++this.attempts > 4096) { this.finish("attempt-overflow"); return; }
    if (status === 12) this.retries++;
    const accepted = status === 0 && hresult === 0;
    if (accepted) this.accepted++;
    if (accepted && this.accepted === 1) this.census?.begin();
    if (accepted && this.accepted === 9) this.census?.stop();
    const math = accepted && this.accepted === 1 ? this.math?.begin()
      : accepted && this.accepted === 9 ? this.math?.stopAtNinth() : null;
    this.event("producer-present", {
      producer_index: accepted ? this.accepted : null, accepted, abi_status: status,
      boundary_semantics: "successful-present-return", attempt_index: this.attempts, retry_count: this.retries,
      hresult: status === 0 ? hresult : null,
      api_id: receipt.id, chain: String(receipt.callArgs[0]), call_pc: receipt.pc,
      before_ns: String(before), after_ns: String(after), guest_clock_ns: String(guestClock),
      closed_phase_boundary: true, ...this.snapshot(),
      ...(math ? { x87_precision_probe: math } : {}),
    });
    if (this.accepted > 64) this.finish("producer-overflow");
    else this.maybeFinish();
  }
  batch(pending) {
    if (this.closed) return null;
    if (this.batches >= 2048) { this.finish("batch-overflow"); return null; }
    this.batches++; this.openTaggedBatches++;
    return { proof_id: this.id, request_id: String(pending.requestId),
      epoch: String(pending.epoch), sequence: String(pending.sequence) };
  }
  acknowledged(pending, browser, received, applied, presents) {
    if (this.closed) return;
    if (this.openTaggedBatches === 0) { this.finish("batch-identity-mismatch"); return; }
    this.openTaggedBatches--;
    this.completed += presents.length;
    this.event("batch-complete", { ...pending.info, worker_send: pending.sent,
      browser, worker_ack_receive: received, worker_ack_applied: applied,
      present_commands: presents, accepted_producer_edges: this.accepted,
      completed_presents: this.completed, open_tagged_batches: this.openTaggedBatches });
    this.maybeFinish();
    // A queued Present can finish on GPU while native reply still returns Busy.
    // Equality is required only at the stable final return/ACK boundary.
  }
  maybeFinish() {
    if (!this.closed && this.accepted >= 9 && this.completed === this.accepted && this.openTaggedBatches === 0)
      this.finish("complete");
  }
  finish(reason) {
    if (this.closed) return;
    const math = this.math?.finish(reason);
    this.closed = true;
    this.event("finished", { reason, accepted_producer_edges: this.accepted,
      completed_presents: this.completed, attempts: this.attempts, retries: this.retries,
      batches: this.batches, open_tagged_batches: this.openTaggedBatches,
      ...(math ? { x87_precision_probe: math } : {}) });
    if (this.session.frameProbe === this) this.session.frameProbe = null;
    if (this.census) {
      const draining = this.census.finish(reason);
      this.session.censusDrain = draining;
      const clear = () => { if (this.session.censusDrain === draining) this.session.censusDrain = null; };
      void draining.then(clear, clear);
    }
  }
}
