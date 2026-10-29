import assert from "node:assert/strict";
import { test } from "node:test";
import { Session } from "../session.js";

async function run(t, ids, { stepMs = 1, callbackAt = -1, blockedAt = -1, acceptedAt = -1 } = {}) {
  let now = 0, calls = 0, reason = 0;
  const events = [], waits = [], clocks = [];
  t.mock.method(performance, "now", () => now);
  const session = new Session(() => {});
  const frame = new Uint8Array(112);
  new DataView(frame.buffer).setUint32(16, 1, true);
  const h = {
    epoch: 1n,
    get: offset => offset === 72 ? reason : offset === 48 ? 0x1000 : 0,
    take: () => frame,
    api: { pending_windows_thread_schedule: () => 0 },
    ok(operation, low, high) {
      if (operation === "observe_cpu_clock") clocks.push(BigInt(low) | BigInt(high) << 32n);
    },
    status(operation) {
      assert.equal(operation, "complete_resident_windows_call");
      events.push(`complete:${calls}`);
      if (calls === blockedAt) return 12;
      if (calls !== acceptedAt) reason = 3;
      return 0;
    },
  };
  Object.assign(session, {
    engine: h, running: true, started: 0, pausedDuration: 0,
    lastSchedule: 0, lastPromotion: Infinity, lastStatus: Infinity,
    media: { update() {}, async graphicsOutput() {} },
    windows: { capture() {}, settle() {} },
    code: {
      entry: () => ({ id: 1n }),
      execute() {
        if (calls === ids.length) {
          reason = 9;
          return { reason: 9, owner: 1n };
        }
        now += stepMs;
        reason = 8;
        const detail = ids[calls++];
        events.push(`execute:${calls}`);
        return { reason: 8, pc: 0x1000, owner: 1n, detail };
      },
    },
    initializer: () => null,
    providers: async () => {
      if (calls === acceptedAt && reason === 8) {
        events.push("provider.ready");
        reason = 3;
      }
      return false;
    },
    beginCallback: async () => calls === callbackAt,
    wait: async () => {
      waits.push(calls);
      events.push("yield");
      if (calls === blockedAt) session.running = false;
    },
    status: () => {},
  });
  await session.loop();
  session.scheduler.close();
  return { calls, waits, events, clocks };
}

test("completed D3D calls share a slice and each completion observes current time", async t => {
  const result = await run(t, [0x40224, 0x40222, 0x40223]);
  assert.deepEqual(result.events.slice(0, 6), [
    "execute:1", "complete:1", "execute:2", "complete:2", "execute:3", "complete:3",
  ]);
  assert.deepEqual(result.waits, [3]);
  assert.deepEqual(result.clocks, [0n, 1000000n, 2000000n, 3000000n]);
});

for (const id of [0x1001e, 0x1001f]) {
  test(`Sleep boundary ${id.toString(16)} yields before the next guest call`, async t => {
    const result = await run(t, [id, 0x40224]);
    assert.equal(result.waits[0], 1);
    assert.ok(result.events.indexOf("yield") < result.events.indexOf("execute:2"));
  });
}

test("a successful Windows callback starts before guest continuation", async t => {
  const result = await run(t, [0x40224, 0x40223], { callbackAt: 1 });
  assert.equal(result.waits[0], 1);
});

test("pending provider completion yields without executing another guest call", async t => {
  const result = await run(t, [0x40224, 0x40223], { blockedAt: 1 });
  assert.equal(result.calls, 1);
  assert.deepEqual(result.waits, [1]);
});

test("successful calls still yield when the shared slice is consumed", async t => {
  const result = await run(t, [0x40224, 0x40222, 0x40223], { stepMs: 3 });
  assert.deepEqual(result.waits, [2, 3]);
});

test("accepted pending file I/O reaches provider progress before another guest instruction", async t => {
  const result = await run(t, [0xa0003, 0x40224], { acceptedAt: 1 });
  assert.equal(result.waits[0], 1);
  assert.ok(result.events.indexOf("provider.ready") > result.events.indexOf("yield"));
  assert.ok(result.events.indexOf("provider.ready") < result.events.indexOf("execute:2"));
});
