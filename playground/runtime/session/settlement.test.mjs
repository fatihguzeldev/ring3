import assert from "node:assert/strict";
import { test } from "node:test";
import { Engine } from "../engine.js";
import { Session } from "../session.js";

async function settle() {
  for (let index = 0; index < 12; index++) await Promise.resolve();
}

test("stop during engine initialization waits for scoped startup before closing the engine", async (t) => {
  let release;
  const gate = new Promise((resolve) => {
    release = resolve;
  });
  const calls = [];
  const engine = {
    status(operation) {
      calls.push(operation);
    },
    get() {
      return 0;
    },
  };
  t.mock.method(Engine, "create", async () => {
    await gate;
    return engine;
  });
  const session = new Session((message) => calls.push(message.state));
  const starting = session.start({
    observation: {
      observer: {
        async scopeAsync(name, args, operation) {
          calls.push("scope.begin");
          try {
            return await operation();
          } finally {
            calls.push("scope.end");
          }
        },
      },
      bind() {
        assert.fail("cancelled startup cannot bind the engine");
      },
    },
  });
  session.cancel();
  let done = false;
  const settled = session.settled().then(() => {
    done = true;
    session.stop();
  });
  await settle();
  assert.equal(done, false);
  assert.equal(calls.includes("close"), false);
  release();
  await assert.rejects(starting, { name: "AbortError" });
  await settled;
  assert.ok(calls.indexOf("scope.end") < calls.indexOf("close"));
  assert.equal(calls.filter((value) => value === "close").length, 1);
});

test("settlement joins the running provider scope after cancellation", async () => {
  let release;
  const gate = new Promise((resolve) => {
    release = resolve;
  });
  const session = new Session(() => {});
  let ended = false,
    closed = false;
  session.initializing = Promise.resolve();
  session.executing = gate.finally(() => {
    ended = true;
  });
  session.engine = {
    status() {
      assert.equal(ended, true);
      closed = true;
    },
    get() {
      return 0;
    },
  };
  session.running = true;
  session.cancel();
  const settled = session.settled().then(() => session.stop());
  await settle();
  assert.equal(closed, false);
  release();
  await settled;
  assert.equal(closed, true);
});
