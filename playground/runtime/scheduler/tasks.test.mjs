import assert from "node:assert/strict";
import { test } from "node:test";
import { TaskScheduler } from "./index.js";

test("repeated task yields service independent input messages", async () => {
  const scheduler = new TaskScheduler();
  const input = new MessageChannel();
  let received = false;
  input.port1.onmessage = () => {
    received = true;
  };
  try {
    input.port2.postMessage("input");
    for (let i = 0; i < 100 && !received; i++) await scheduler.yield();
    assert.equal(received, true);
  } finally {
    scheduler.close();
    input.port1.close();
    input.port2.close();
  }
});

test("positive waits preserve their requested duration", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const scheduler = new TaskScheduler();
  let finished = false;
  try {
    const pending = scheduler.wait(20).then(() => {
      finished = true;
    });
    t.mock.timers.tick(19);
    await Promise.resolve();
    assert.equal(finished, false);
    t.mock.timers.tick(1);
    await pending;
    assert.equal(finished, true);
  } finally {
    scheduler.close();
  }
});

test("abort rejects an outstanding yield without losing the original reason", async () => {
  const abort = new AbortController();
  const scheduler = new TaskScheduler(abort.signal);
  const pending = scheduler.yield();
  const reason = new Error("session stopped");
  abort.abort(reason);
  await assert.rejects(pending, (error) => error === reason);
  await assert.rejects(scheduler.yield(), (error) => error === reason);
});

test("closing cancels a wait and releases the pending operation", async () => {
  const scheduler = new TaskScheduler();
  const pending = scheduler.wait(1000);
  scheduler.close();
  await assert.rejects(pending, { name: "AbortError" });
});

test("pending work is bounded to one task", async () => {
  const scheduler = new TaskScheduler();
  try {
    const pending = scheduler.yield();
    await assert.rejects(scheduler.wait(1), /already pending/);
    await pending;
    await scheduler.yield();
  } finally {
    scheduler.close();
  }
});
