import { PhaseMeasurements } from "./measurement.js";
import { ExecutionAttribution } from "./frame-probe/execution.js";
import { liveModules } from "./diagnostics/modules.js";

// successful Present returns bracket bounded host observations; draining enqueues can precede them.
const origin = BigInt(Math.floor(performance.timeOrigin * 1e6));
const coreStopNames = Object.freeze([null, "needCode", "budget", "memoryFault", "unsupported", "debug",
  "codeInvalidated", "host", "infrastructure", "cpuFault", "systemEntry"]);
export const frameProbeNow = () => origin + BigInt(Math.floor(performance.now() * 1e6));
export const frameProbeStamp = () => ({ origin_ns: String(origin), timestamp_ns: String(frameProbeNow()) });
export class FrameProbe {
  constructor(id, session) {
    this.id = id; this.session = session; this.accepted = 0; this.completed = 0;
    this.sequence = 0; this.batches = 0; this.attempts = 0; this.retries = 0;
    this.closed = false; this.openTaggedBatches = 0;
    this.executionWork = {
      cold: { count: 0, work: 0, retired: 0, zeroWorkCount: 0, positiveWorkCount: 0, zeroWorkNeedCodeCount: 0,
        reasons: { needCode: 0, budget: 0, memoryFault: 0, unsupported: 0, debug: 0,
          codeInvalidated: 0, host: 0, infrastructure: 0, cpuFault: 0, systemEntry: 0 } },
      compiled: { count: 0, work: 0, retired: 0 },
      chainedCold: { runs: 0, work: 0, retired: 0 },
    };
    this.executionAttribution = new ExecutionAttribution();
    this.ownsMeasurements = session.measurements == null;
    this.measurements = session.measurements ?? new PhaseMeasurements();
    if (this.ownsMeasurements) session.measurements = this.measurements;
    try {
      this.event("armed", { target_intervals: 8, required_producer_edges: 9,
        engine_sha256: session.engine.hash, backend: "core", callback_backend: "core", clean_graphics_boundary: true,
        live_modules: liveModules(session.engine),
        measurement_enabled: true,
        phase_semantics: "cumulative inclusive host elapsed time; async waits include event-loop work; paused-loop waits excluded; in-flight async spans may cross pause",
        semantics: "successful native Present return brackets; enqueue may precede Busy retry completion; GPU completion is Promise-observed, not compositor visibility" });
    } catch (error) {
      this.executionAttribution.finish();
      this.releaseMeasurements();
      throw error;
    }
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
    return { counters_sampled_at_ns: String(frameProbeNow()), phases: this.measurements.snapshot(),
      stats: this.session.code.stats(), cold_diagnostics: this.session.code.coldDiagnostics?.() ?? null,
      pc: this.session.engine.get(48), thread: this.session.thread,
      execution_work: {
        semantics: "successful code.execute results by entry tier; chained_cold included in compiled; counts are not additive or time",
        cold: { ...this.executionWork.cold, reasons: { ...this.executionWork.cold.reasons } },
        compiled: { ...this.executionWork.compiled },
        chained_cold: { ...this.executionWork.chainedCold },
      },
      execution_attribution: this.executionAttribution.snapshot(),
      physicalInputFocus: this.session.media?.lastPhysicalFocus
        ? { ...this.session.media.lastPhysicalFocus } : null };
  }
  recordExecution(run, startPc, selected, milliseconds) {
    if (this.closed || this.session.measurements !== this.measurements) return;
    const cold = run?.kind === "cold";
    if (run !== undefined && !cold && run.kind !== "compiled") throw new Error("invalid frame probe execution kind");
    const reason = cold ? coreStopNames[run.coreReason] : null;
    if (cold && typeof reason !== "string") throw new Error("invalid frame probe cold stop");
    if (startPc !== undefined && milliseconds != null)
      this.executionAttribution.record(run, startPc, selected, milliseconds);
    if (run === undefined) return;
    const counters = cold ? this.executionWork.cold : this.executionWork.compiled;
    counters.count++; counters.work += run.work; counters.retired += run.retired;
    if (!cold) {
      const chained = this.executionWork.chainedCold;
      chained.runs += run.chainedColdRuns ?? 0;
      chained.work += run.chainedColdWork ?? 0;
      chained.retired += run.chainedColdRetired ?? 0;
    }
    if (cold) {
      if (run.work === 0) {
        counters.zeroWorkCount++;
        if (run.coreReason === 1) counters.zeroWorkNeedCodeCount++;
      } else counters.positiveWorkCount++;
      counters.reasons[reason]++;
    }
  }
  present(receipt, status, hresult, before, after, guestClock) {
    if (this.closed) return;
    if (++this.attempts > 4096) { this.finish("attempt-overflow"); return; }
    if (status === 12) this.retries++;
    const accepted = status === 0 && hresult === 0;
    if (accepted) this.accepted++;
    this.event("producer-present", {
      producer_index: accepted ? this.accepted : null, accepted, abi_status: status,
      boundary_semantics: "successful-present-return", attempt_index: this.attempts, retry_count: this.retries,
      hresult: status === 0 ? hresult : null,
      api_id: receipt.id, chain: String(receipt.callArgs[0]), call_pc: receipt.pc,
      before_ns: String(before), after_ns: String(after), guest_clock_ns: String(guestClock),
      closed_phase_boundary: true, ...this.snapshot(),
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
    // GPU completion can precede a Busy native reply; equality belongs to the final boundary.
  }
  maybeFinish() {
    if (!this.closed && this.accepted >= 9 && this.completed === this.accepted && this.openTaggedBatches === 0)
      this.finish("complete");
  }
  finish(reason) {
    if (this.closed) return;
    this.closed = true;
    try {
      this.executionAttribution.finish();
      this.event("finished", { reason, accepted_producer_edges: this.accepted,
        completed_presents: this.completed, attempts: this.attempts, retries: this.retries,
        batches: this.batches, open_tagged_batches: this.openTaggedBatches,
        execution_attribution: this.executionAttribution.snapshot(),
      });
    } finally {
      this.executionAttribution.finish();
      this.releaseMeasurements();
      if (this.session.frameProbe === this) this.session.frameProbe = null;
    }
  }
  releaseMeasurements() {
    if (this.ownsMeasurements && this.session.measurements === this.measurements)
      this.session.measurements = null;
  }
}
