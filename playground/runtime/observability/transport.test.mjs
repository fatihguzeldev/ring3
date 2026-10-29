import assert from "node:assert/strict";
import { test } from "node:test";
import { WorkerCapture } from "../observability.js";
import { BrowserCapture } from "../../browser/observability.js";
import { Session } from "../../browser/session.js";

function recording(t, emit) {
  const deadlines = [];
  t.mock.method(globalThis, "setInterval", () => 1);
  t.mock.method(globalThis, "clearInterval", () => undefined);
  t.mock.method(globalThis, "setTimeout", (callback) => {
    deadlines.push(callback);
    return deadlines.length;
  });
  t.mock.method(globalThis, "clearTimeout", () => undefined);
  return { worker: new WorkerCapture({ level: "basic" }, emit), deadlines };
}

async function settle() {
  for (let index = 0; index < 12; index++) await Promise.resolve();
}

test("a stalled main thread can retain only one unacknowledged worker chunk", async (t) => {
  const messages = [];
  const { worker } = recording(t, (message) => messages.push(message));
  for (let index = 0; index < 100; index++)
    worker.observer.event("fixture", { payload: "x".repeat(4096) });
  await settle();
  assert.equal(messages.filter((message) => message.type === "observability-data").length, 1);
  const finished = worker.finish("complete");
  let acknowledged = 0;
  for (let index = 0; index < 100; index++) {
    for (const message of messages.filter((message) => message.type === "observability-data")) {
      if (message.sequence < acknowledged) continue;
      assert.equal(message.sequence, acknowledged++);
      worker.acknowledge({ type: "observability-ack", sequence: message.sequence, accepted: true });
    }
    await settle();
    if (messages.some((message) => message.type === "observability-result")) break;
  }
  assert.equal((await finished).complete, true);
  assert.ok(acknowledged > 1);
});

test("an ACK timeout ends recording as incomplete while retaining the application outcome", async (t) => {
  const messages = [];
  const { worker, deadlines } = recording(t, (message) => messages.push(message));
  worker.observer.event("fixture");
  const finished = worker.finish("failed", new Error("original application failure"));
  await settle();
  assert.equal(messages.filter((message) => message.type === "observability-data").length, 1);
  worker.acknowledge({ type: "observability-ack", sequence: 99, accepted: true });
  deadlines[0]();
  const result = await finished;
  assert.equal(result.outcome, "failed");
  assert.equal(result.complete, false);
  assert.match(result.sinkError, /acknowledgement timed out/);
  assert.match(
    messages.find((message) => message.type === "observability-error").error,
    /acknowledgement timed out/,
  );
  assert.equal(messages.filter((message) => message.type === "observability-data").length, 1);
});

test("the browser acknowledges rejected bytes without failing the application", async () => {
  const incomplete = [],
    acknowledgements = [];
  const capture = Object.assign(Object.create(BrowserCapture.prototype), {
    workerSequence: 0,
    capture: {
      ingestAsync: async () => false,
      markIncomplete: (reason) => incomplete.push(reason),
      status: () => ({ recordingError: "trace packet rejected" }),
    },
  });
  const worker = { postMessage: (message) => acknowledgements.push(message) };
  const session = Object.assign(Object.create(Session.prototype), {
    capture,
    worker,
    failed: true,
    state: "error",
  });
  await session.message(
    { type: "observability-data", sequence: 0, bytes: new ArrayBuffer(1) },
    worker,
  );
  await settle();
  assert.deepEqual(acknowledgements, [
    { type: "observability-ack", sequence: 0, accepted: false, error: "trace packet rejected" },
  ]);
  assert.deepEqual(incomplete, ["trace packet rejected"]);
  assert.equal(session.state, "error");
});

test("stop can finish its recorder while the main session is already stopped", async (t) => {
  const messages = [];
  const { worker } = recording(t, (message) => messages.push(message));
  const accepted = [];
  const capture = Object.assign(Object.create(BrowserCapture.prototype), {
    workerSequence: 0,
    capture: {
      ingestAsync: async (bytes) => {
        accepted.push(bytes.length);
        return true;
      },
      flush: async () => ({}),
      markIncomplete: assert.fail,
    },
  });
  const sessionWorker = { postMessage: (message) => worker.acknowledge(message) };
  const session = Object.assign(Object.create(Session.prototype), {
    capture,
    worker: sessionWorker,
    state: "stopped",
  });
  worker.observer.event("fixture");
  const finished = worker.finish("cancelled");
  await settle();
  for (let index = 0; index < 20; index++) {
    const chunk = messages.shift();
    if (chunk?.type === "observability-data") await session.message(chunk, sessionWorker);
    await settle();
    if (messages.some((message) => message.type === "observability-result")) break;
  }
  assert.equal((await finished).complete, true);
  assert.ok(accepted.length > 0);
  assert.equal(session.state, "stopped");
});
