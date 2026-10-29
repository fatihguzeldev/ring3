import assert from "node:assert/strict";
import { test } from "node:test";
import { finishSession } from "./finalization.js";
import { WorkerCapture } from "../observability.js";

async function settle() {
  for (let index = 0; index < 12; index++) await Promise.resolve();
}

test("finalization joins real asynchronous scopes before reporting complete and closing the engine", async (t) => {
  t.mock.method(globalThis, "setInterval", () => 1);
  t.mock.method(globalThis, "clearInterval", () => {});
  let release, observation;
  const gate = new Promise((resolve) => {
    release = resolve;
  });
  const calls = [];
  observation = new WorkerCapture({ level: "basic" }, (message) => {
    if (message.type === "observability-data")
      queueMicrotask(() => observation.acknowledge({ sequence: message.sequence, accepted: true }));
    if (message.type === "observability-result") calls.push("capture.finished");
  });
  const work = observation.observer.scopeAsync("provider", {}, async () => {
    await gate;
    calls.push("provider.finished");
  });
  const session = {
    cancel() {
      calls.push("cancel");
    },
    settled() {
      return work;
    },
    stop() {
      calls.push("engine.closed");
    },
  };
  const first = finishSession(session, observation, "cancelled");
  assert.equal(finishSession(session, observation, "failed"), first);
  await settle();
  assert.equal(calls.includes("capture.finished"), false);
  release();
  const result = await first;
  assert.equal(result.complete, true, JSON.stringify(result));
  assert.equal(result.outcome, "cancelled");
  assert.ok(calls.indexOf("provider.finished") < calls.indexOf("capture.finished"));
  assert.ok(calls.indexOf("capture.finished") < calls.indexOf("engine.closed"));
  assert.equal(calls.filter((value) => value === "engine.closed").length, 1);
});

test("terminal work retains the captured session and cannot finish a replacement capture", async () => {
  let release;
  const gate = new Promise((resolve) => {
    release = resolve;
  });
  const calls = [];
  const previous = { cancel() {}, settled: () => gate, stop: () => calls.push("previous.closed") };
  const current = {
    cancel() {},
    settled: async () => {},
    stop: () => calls.push("current.closed"),
  };
  const previousCapture = { finish: async () => calls.push("previous.capture") };
  const currentCapture = { finish: async () => calls.push("current.capture") };
  const pending = finishSession(previous, previousCapture, "cancelled");
  await finishSession(current, currentCapture, "complete");
  release();
  await pending;
  assert.deepEqual(calls, [
    "current.capture",
    "current.closed",
    "previous.capture",
    "previous.closed",
  ]);
});
