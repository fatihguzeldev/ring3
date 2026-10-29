import { Engine } from "../../runtime/engine.js";
import { CodeCache } from "../../runtime/cache.js";
import { WorkerCapture } from "../../runtime/observability.js";
import { TaskScheduler } from "../../runtime/scheduler/index.js";

const configuration = Object.freeze({
  entries: 512,
  slots: 128,
  wasmBytes: 8 * 1024 * 1024,
  batchMs: 6,
  engineTraceBytes: 256 * 1024,
});
let active;
function emit(message, transfer = []) {
  postMessage(message, transfer);
}

async function start(options) {
  if (active) {
    emit({ type: "proof-error", message: "a proof is already active" });
    return;
  }
  const run = {
    abort: new AbortController(),
    iterations: 0,
    pings: 0,
    phase: "loading",
    started: performance.now(),
  };
  active = run;
  run.scheduler = new TaskScheduler(run.abort.signal);
  let engine, cache, observation, error, result;
  try {
    if (!["off", "basic", "detailed"].includes(options.level))
      throw new Error("invalid proof level");
    if (!Number.isFinite(options.seconds) || options.seconds < 3 || options.seconds > 90)
      throw new Error("invalid proof duration");
    observation =
      options.level === "off" ? null : new WorkerCapture({ level: options.level }, emit);
    run.observation = observation;
    engine = await Engine.create("/engine.wasm", 32, run.abort.signal);
    observation?.bind(engine);
    run.engineHash = engine.hash;
    engine.ok("map", 0x10000, 1, 7);
    engine.ok("configure_execution", configuration.slots, configuration.wasmBytes, 0xffffffff);
    const bytes = new Uint8Array(4096).fill(0x90);
    for (let index = 0; index < configuration.entries; index++) bytes[index * 8 + 1] = 0xc3;
    engine.request(bytes);
    engine.ok("upload", 0x10000, bytes.length);
    cache = new CodeCache(engine, { slots: configuration.slots });
    run.workStarted = performance.now();
    run.phase = "working";
    let nextProgress = run.workStarted;
    emit({
      type: "proof-started",
      configuration,
      engineHash: engine.hash,
      level: options.level,
      timestamp: run.workStarted,
    });
    while (
      !run.abort.signal.aborted &&
      performance.now() - run.workStarted < options.seconds * 1000
    ) {
      const deadline = performance.now() + configuration.batchMs;
      do {
        cache.entry(0x10000 + (run.iterations % configuration.entries) * 8);
        run.iterations++;
      } while (!run.abort.signal.aborted && performance.now() < deadline);
      const checkpoint = observation?.checkpoint();
      if (checkpoint) await checkpoint;
      if (performance.now() >= nextProgress) {
        cache.sampleUsage();
        observation?.counters(cache.stats(), 0);
        emit({
          type: "proof-progress",
          elapsedMs: performance.now() - run.workStarted,
          iterations: run.iterations,
          pings: run.pings,
          stats: cache.stats(),
          observation: observation?.capture.status(),
          exportWaitMs: observation?.checkpointWaitMs ?? 0,
          checkpointCount: observation?.checkpointCount ?? 0,
        });
        nextProgress = performance.now() + 1000;
      }
      if (run.abort.signal.aborted) break;
      await run.scheduler.yield();
    }
  } catch (failure) {
    if (!(run.abort.signal.aborted && failure === run.abort.signal.reason))
      error = failure instanceof Error ? failure.message : String(failure);
  } finally {
    run.workEnded = performance.now();
    run.phase = "finalizing";
    run.scheduler.close();
    if (cache) {
      cache.sampleUsage();
      observation?.counters(cache.stats(), 0);
    }
    const outcome = error ? "failed" : run.abort.signal.aborted ? "cancelled" : "complete";
    try {
      result = await observation?.finish(outcome, error);
    } catch (failure) {
      error ??= failure instanceof Error ? failure.message : String(failure);
    }
    engine?.status("close");
    active = null;
    emit({
      type: "proof-result",
      outcome: error ? "failed" : outcome,
      error,
      configuration,
      level: options.level,
      engineHash: run.engineHash,
      elapsedMs: performance.now() - run.started,
      loopWorkElapsedMs: run.workStarted === undefined ? 0 : run.workEnded - run.workStarted,
      finalizationElapsedMs: performance.now() - run.workEnded,
      iterations: run.iterations,
      pings: run.pings,
      stopReceivedAt: run.stopReceivedAt,
      stats: cache?.stats(),
      capture: result ?? null,
      exportWaitMs: observation?.checkpointWaitMs ?? 0,
      checkpointCount: observation?.checkpointCount ?? 0,
    });
  }
}

self.onmessage = ({ data }) => {
  if (data.type === "observability-ack") {
    active?.observation?.acknowledge(data);
    return;
  }
  if (data.type === "start") {
    void start(data);
    return;
  }
  if (data.type === "ping") {
    if (active) active.pings++;
    emit({
      type: "proof-pong",
      id: data.id,
      sentAt: data.sentAt,
      receivedAt: performance.now(),
      phase: active?.phase ?? "idle",
      duringLoop: active?.phase === "working",
      iterations: active?.iterations ?? 0,
    });
    return;
  }
  if (data.type === "stop" && active) {
    active.stopReceivedAt ??= performance.now();
    emit({
      type: "proof-stop-received",
      sentAt: data.sentAt,
      receivedAt: active.stopReceivedAt,
      iterations: active.iterations,
    });
    active.abort.abort();
  }
};
