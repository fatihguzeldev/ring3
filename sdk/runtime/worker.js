import { Engine } from "./engine.js";
import { configure } from "./configuration.js";
import { Session } from "./session.js";
import { WorkerCapture } from "./observability.js";
import { finishSession } from "./session/finalization.js";
let session = null,
  control = Promise.resolve(),
  observation = null;
const emit = (message, transfer = [], owner = session, capture = observation) => {
  capture?.record(message);
  postMessage(message, transfer);
  if (message.type === "status" && ["exited", "error"].includes(message.state)) {
    queueMicrotask(() => {void finalize(owner, capture, message.state === "exited" ? "complete" : "failed", message.message).catch(() => {});});
  }
};
const finalize = (owner, capture, outcome, error) =>
  finishSession(owner, capture, outcome, error).catch((failure) => {
    capture?.transportError(failure);
    postMessage({type:'diagnostic',message:'session finalization failed',detail:{message:failure.message}});
    throw failure;
  });
async function probe(message) {
  let engine;
  try {
    engine = await Engine.create(
      message.engineUrl ?? "/engine.wasm",
      undefined,
      undefined,
      message.compiledEngine ?? undefined,
    );
    configure(engine);
    emit({
      type: "probe-result",
      ok: true,
      engineHash: engine.hash,
      compiledEngine: { module: engine.module, hash: engine.hash },
      message: "engine Wasm instantiated, opened and host services configured; no game started",
    });
  } catch (error) {
    emit({ type: "probe-result", ok: false, message: error.message });
  } finally {
    if (engine) engine.status("close");
  }
}
self.onmessage = (event) => {
  const message = event.data;
  if (message.type === "observability-ack") {
    observation?.acknowledge(message);
    return;
  }
  if (message.type === "probe") {
    control = control.then(() => probe(message));
    return;
  }
  if (message.type === "start") {
    control = control.then(async () => {
      if (session) await finalize(session, observation, "cancelled");
      let capture;
      const current = new Session((message, transfer) => emit(message, transfer, current, capture));
      session = current;
      try {
        capture =
          message.observability?.level && message.observability.level !== "off"
            ? new WorkerCapture(message.observability, (message, transfer) =>
                emit(message, transfer, current, capture),
              )
            : null;
        observation = capture;
        await current.start({ ...message, observation: capture });
      } catch (error) {
        if (!current.abort.signal.aborted) current.fail(error);
      }
    });
    return;
  }
  if (message.type === "observability-finish") {
    if (session) void finalize(session, observation, message.outcome ?? "failed", message.error).catch(() => {});
    return;
  }
  if (message.type === "stop" && session) {
    const owner = session;
    void finalize(owner, observation, "cancelled").then(() => {
      if (owner.stopReported) return;
      owner.stopReported = true;
      owner.status("stopped", "stopped");
    }).catch(() => {});
    return;
  }
  if (!session || session.stopped) return;
  try {
    if (message.type === "frame-probe-arm") session.armFrameProbe(message.id);
    else if (message.type === "pause") session.pause();
    else if (message.type === "resume") session.resume();
    else if (message.type === "input") session.media?.enqueue(message.events ?? [], message.sentAtMs);
    else if (message.type === "pcm-accepted") session.media?.acceptedAudio(message.sequence);
    else if (message.type === "graphics-complete") session.media?.graphicsComplete(message);
    else if (message.type === "gdi-complete") session.media?.gdiComplete(message);
    else if (message.type === "graphics-error")
      throw new Error(message.error ?? message.message ?? "graphics executor failed");
    else if (message.type === "dialog-result") session.media?.dialogResult(message);
    else if (message.type === "dialog-help") session.media?.dialogHelp(message);
    else if (message.type === "cursor-applied") session.media?.cursorApplied(message.revision);
  } catch (error) {
    session.fail(error);
  }
};
