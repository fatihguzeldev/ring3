import {
  createCapture,
  categories,
  resolveCategorySelection,
} from "../../observability/typescript/dist/index.js";

export class WorkerCapture {
  constructor(options, emit) {
    this.emit = emit;
    this.sequence = 0;
    this.pending = null;
    this.capture = createCapture({
      level: options?.level ?? "off",
      categories: resolveCategorySelection(options?.categories),
      producerId: 2,
      output: {
        write: (bytes) => this.write(bytes),
        async flush() {},
      },
    });
    this.observer = this.capture.observer(categories.hostWorker);
    this.detailed = options?.level === "detailed";
    this.nextCheckpoint = 0;
    this.checkpointWaitMs = 0;
    this.checkpointCount = 0;
    this.timer = setInterval(() => this.flush(), this.detailed ? 100 : 500);
  }
  write(bytes) {
    if (this.pending) return Promise.reject(new Error("worker trace write is already pending"));
    const sequence = this.sequence++;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        if (this.pending?.sequence !== sequence) return;
        this.pending = null;
        const error = new Error("worker trace acknowledgement timed out");
        this.transportError(error);
        reject(error);
      }, 2000);
      this.pending = { sequence, timer, resolve, reject };
      try {
        this.emit({ type: "observability-data", sequence, bytes: bytes.buffer }, [bytes.buffer]);
      } catch (error) {
        clearTimeout(timer);
        this.pending = null;
        reject(error);
      }
    });
  }
  acknowledge(message) {
    const pending = this.pending;
    if (!pending || message.sequence !== pending.sequence) return;
    clearTimeout(pending.timer);
    this.pending = null;
    if (message.accepted === true) pending.resolve();
    else {
      const error = new Error(
        String(message.error ?? "browser rejected worker trace bytes").slice(0, 512),
      );
      this.transportError(error);
      pending.reject(error);
    }
  }
  transportError(error) {
    try {
      this.emit({ type: "observability-error", error: error.message });
    } catch {}
  }
  bind(engine) {
    engine.observer = this.capture.observer(categories.hostWasm, "detailed");
    engine.generatedModuleNames = this.capture.observer(categories.hostWasm).enabled;
    engine.cacheMeasurementsEnabled = this.capture.observer(categories.hostWasm).enabled;
    engine.filesObserver = this.capture.observer(categories.hostFiles, "detailed");
    engine.fileMeasurementsEnabled = this.capture.observer(categories.hostFiles).enabled;
    engine.inputMeasurementsEnabled = this.observer.enabled;
    engine.graphicsMeasurementsEnabled = this.capture.observer(categories.hostGraphics).enabled;
    this.capture.attachEngine(engine.observabilityPort(), { producerId: 3 });
    this.observer.event("engine.loaded", { sha256: engine.hash });
    this.emit({
      type: "observability-engine",
      engine: this.capture.status().engine,
      sha256: engine.hash,
    });
  }
  counters(stats, bytesFetched) {
    for (const [name, value] of Object.entries(stats))
      if (typeof value === "number" && Number.isFinite(value)) this.observer.counter(name, value);
    for (const [name, value] of Object.entries(stats.cacheDiagnostics ?? {}))
      if (typeof value === "number") this.observer.counter(`cache.${name}`, value);
    this.observer.counter("bytes-fetched", bytesFetched);
    if (this.detailed) {
      this.observer.counter("export.checkpoint.ns", Math.round(this.checkpointWaitMs * 1e6));
      this.observer.counter("export.checkpoint.count", this.checkpointCount);
    }
  }
  record(message) {
    if (message.type === "status")
      this.observer.event("status", { state: message.state, message: message.message });
    if (message.type === "diagnostic")
      this.observer.event("diagnostic", { message: message.message });
  }
  checkpoint() {
    if (!this.detailed || this.finishing) return;
    const now = performance.now();
    if (now < this.nextCheckpoint) return;
    this.nextCheckpoint = now + 20;
    this.checkpointCount++;
    return this.flush().finally(() => {
      this.checkpointWaitMs += performance.now() - now;
    });
  }
  flush() {
    return this.capture.flush();
  }
  finish(outcome, error) {
    if (this.finishing) return this.finishing;
    clearInterval(this.timer);
    this.finishing = (async () => {
      await this.capture.flush();
      const result = await this.capture.finish(outcome, error);
      this.emit({ type: "observability-result", result });
      return result;
    })();
    return this.finishing;
  }
}
