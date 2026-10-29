import assert from "node:assert/strict";
import { test } from "node:test";
import { Session } from "../session.js";

function stoppingSession(t) {
  const timers = [];
  t.mock.method(globalThis, "setTimeout", (callback) => {
    timers.push(callback);
    return timers.length;
  });
  t.mock.method(globalThis, "clearTimeout", () => {});
  const messages = [],
    captures = [],
    terminated = [],
    cleanup = [];
  const worker = {
    postMessage: (message) => messages.push(message),
    terminate: () => terminated.push("original"),
  };
  const capture = {
    finish: async (outcome) => {
      captures.push({ capture: "original", outcome });
    },
  };
  const session = Object.assign(Object.create(Session.prototype), {
    state: "running",
    generation: 1,
    failed: false,
    worker,
    capture,
    input: { disable() {} },
    dialogs: { close() {} },
    onDiagnostic() {},
    audio: { close: async () => cleanup.push("audio") },
    graphicsQueue: Promise.resolve(),
    graphics: { close: () => cleanup.push("graphics") },
    windows: { reset: () => cleanup.push("windows") },
    gdi: { clear: () => cleanup.push("gdi") },
    status(state) {
      this.state = state;
    },
  });
  return { session, messages, captures, terminated, cleanup, timers };
}

test("concurrent stops share one cleanup and finish their capture once", async (t) => {
  const { session, messages, captures, terminated } = stoppingSession(t);
  const first = session.stop();
  const second = session.stop();
  assert.equal(first, second);
  assert.deepEqual(messages, [{ type: "stop" }]);
  session.stopped();
  await first;
  assert.deepEqual(captures, [{ capture: "original", outcome: "cancelled" }]);
  assert.deepEqual(terminated, ["original"]);
  assert.equal(session.worker, null);
  assert.equal(session.stopping, null);
});

test("old stop cannot finish or clear a replacement run", async (t) => {
  const { session, captures, terminated, cleanup } = stoppingSession(t);
  const stopped = session.stop();
  const acknowledge = session.stopped;
  const replacement = { postMessage() {}, terminate: () => terminated.push("replacement") };
  session.generation++;
  session.worker = replacement;
  session.capture = { finish: async () => captures.push({ capture: "replacement" }) };
  acknowledge();
  await stopped;
  assert.deepEqual(captures, [{ capture: "original", outcome: "cancelled" }]);
  assert.deepEqual(terminated, ["original"]);
  assert.equal(session.worker, replacement);
  assert.deepEqual(cleanup, []);
});

test("a new start cannot enter while stop cleanup is pending", async (t) => {
  const { session } = stoppingSession(t);
  const stopped = session.stop();
  session.worker = null;
  session.state = "stopped";
  await assert.rejects(session.start({}), /stop the current session/);
  session.stopped();
  await stopped;
});

test("capture finalization waits for the current graphics work to settle", async (t) => {
  const { session, captures } = stoppingSession(t);
  let release;
  session.graphicsQueue = new Promise((resolve) => { release = resolve; });
  const stopped = session.stop();
  session.stopped();
  for (let index = 0; index < 12; index++) await Promise.resolve();
  assert.deepEqual(captures, []);
  release();
  await stopped;
  assert.deepEqual(captures, [{ capture: "original", outcome: "cancelled" }]);
});
