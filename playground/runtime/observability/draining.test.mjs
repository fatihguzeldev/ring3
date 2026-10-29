import assert from "node:assert/strict";
import { test } from "node:test";
import { WorkerCapture } from "../observability.js";

function recording(t, level = "basic") {
  t.mock.method(globalThis, "setInterval", () => 1);
  t.mock.method(globalThis, "clearInterval", () => {});
  let worker;
  worker = new WorkerCapture({ level }, (message) => {
    if (message.type === "observability-data")
      queueMicrotask(() => worker.acknowledge({ sequence: message.sequence, accepted: true }));
  });
  t.after(() => worker.finish("cancelled"));
  return worker;
}

test("basic checkpoints do not poll a hot clock or create a Promise", async (t) => {
  const worker = recording(t);
  t.mock.method(performance, "now", () => assert.fail("hot checkpoint clock"));
  assert.equal(worker.checkpoint(), undefined);
  t.mock.restoreAll();
});

test("worker finalization lets already rejected operations close their scopes", async (t) => {
  const worker = recording(t);
  let reject;
  const operation = worker.observer.scopeAsync(
    "provider",
    {},
    () =>
      new Promise((_, fail) => {
        reject = fail;
      }),
  );
  const rejected = assert.rejects(operation, /cancelled provider/);
  reject(new Error("cancelled provider"));
  const result = await worker.finish("cancelled");
  await rejected;
  assert.equal(result.complete, true, JSON.stringify(result));
});

test("detailed checkpoints accumulate inclusive export wait without counting skipped polls", async (t) => {
  const worker = recording(t, "detailed");
  const clock = [10, 13, 15];
  t.mock.method(performance, "now", () => clock.shift());
  t.mock.method(worker, "flush", async () => {});
  await worker.checkpoint();
  assert.equal(worker.checkpoint(), undefined);
  assert.equal(worker.checkpointCount, 1);
  assert.equal(worker.checkpointWaitMs, 3);
  t.mock.restoreAll();
});
