import assert from "node:assert/strict";
import { test } from "node:test";
import { Session } from "../session.js";

for (const boundary of ["checkpoint", "graphics", "providers"]) {
  test(`stop during ${boundary} cannot resume engine work`, async () => {
    const session = new Session(() => {});
    let unblock, entered;
    const blocked = new Promise((resolve) => {
      unblock = resolve;
    });
    const reached = new Promise((resolve) => {
      entered = resolve;
    });
    const gate = () => {
      entered();
      return blocked;
    };
    const operations = [];
    let closed = false;
    session.engine = {
      get: () => 0,
      api: { pending_windows_thread_schedule: () => 0 },
      ok(operation) {
        assert.equal(closed, false, "engine operation after close");
        operations.push(operation);
      },
      status(operation) {
        operations.push(operation);
        closed = true;
        return 0;
      },
    };
    session.code = {
      stats: () => ({}),
      entry() {
        assert.fail("stopped session must not prepare code");
      },
    };
    session.observation = { checkpoint: boundary === "checkpoint" ? gate : () => undefined };
    session.media = {
      update() {},
      graphicsOutput: boundary === "graphics" ? gate : async () => {},
    };
    session.providers = boundary === "providers" ? gate : async () => false;
    session.started = performance.now();
    session.lastSchedule = session.started;
    session.pausedDuration = 0;
    session.running = true;
    const loop = session.loop();
    await reached;
    session.stop();
    unblock(false);
    await assert.doesNotReject(loop);
    assert.equal(operations.at(-1), "close");
    assert.equal(session.running, false);
  });
}
