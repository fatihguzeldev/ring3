import assert from "node:assert/strict";
import { test } from "node:test";
import { FrameProbe } from "./frame-probe.js";
import { Session } from "../session.js";

function fixture(enabled = true) {
  const calls = [], messages = [];
  const session = new Session((message) => messages.push(message));
  session.frameProbeEnabled = true;
  session.frameProbeCensusEnabled = enabled;
  session.running = true;
  session.receiptModeControl = true;
  const packet = new Uint8Array(4096), view = new DataView(packet.buffer);
  packet.set(new TextEncoder().encode("R3CE"));
  for (const [offset, value] of [[4, 65537], [8, 4096], [24, 2], [28, 32768], [44, 80]])
    view.setUint32(offset, value, true);
  view.setBigUint64(16, 1n, true);
  let scratch = new Uint8Array(4096).fill(91);
  session.engine = {
    hash: "fixture", get: () => 0,
    api: Object.fromEntries(["begin_execution_census", "stop_execution_census", "execution_census_page"]
      .map((name) => [name, () => 0])),
    ok(name) { calls.push(name); if (name === "execution_census_page") scratch = packet.slice(); },
    take: () => scratch.slice(), request: (bytes) => { scratch = bytes.slice(); },
  };
  session.code = {
    receiptMode: "compiled", sampleUsage() {}, stats: () => ({}),
    setReceiptMode(mode) { calls.push("mode"); this.receiptMode = mode; },
  };
  session.observation = {
    capture: { observer: () => ({ enabled: true, event(name) { calls.push(name); } }) },
    flush: async () => ({ droppedEvents: 0, droppedChunks: 0 }),
  };
  session.frameProbe = new FrameProbe("fixture-proof", session);
  const present = (status = 0, result = 0) => session.frameProbe.present(
    { id: 0x4020f, pc: 0x1000, callArgs: [1] }, status, result, 1n, 2n, 3n,
  );
  return { session, calls, messages, present };
}

test("census starts at first accepted return, freezes at ninth, then drains outside frame proof", async () => {
  const { session, calls, messages, present } = fixture();
  present(12); present(0, 1);
  assert.deepEqual(calls, []);
  present();
  assert.deepEqual(calls, ["begin_execution_census"]);
  for (let i = 0; i < 7; i++) present();
  assert.equal(calls.includes("stop_execution_census"), false);
  present(12);
  assert.equal(calls.includes("stop_execution_census"), false);
  present();
  assert.equal(calls.filter((name) => name === "stop_execution_census").length, 1);
  assert.equal(calls.includes("execution_census_page"), false);
  session.frameProbe.completed = 9;
  session.frameProbe.maybeFinish();
  await session.censusDrain;
  assert.equal(session.frameProbe, null);
  assert.equal(messages.filter((m) => m.message === "frame-probe").at(-1).detail.event, "finished");
  assert.equal(messages.find((m) => m.message === "execution-census-ready").detail.complete, true);
});

test("default-off frame proof never calls census exports", () => {
  const { session, calls, present } = fixture(false);
  for (let i = 0; i < 9; i++) present();
  session.frameProbe.completed = 9;
  session.frameProbe.maybeFinish();
  assert.deepEqual(calls, []);
});

test("settlement stops and joins a partial census before engine close", async () => {
  const { session, calls, messages, present } = fixture();
  let release;
  session.observation.flush = () => new Promise((resolve) => { release = resolve; });
  present();
  let settled = false;
  const pending = session.settled().then(() => { settled = true; });
  for (let i = 0; i < 5; i++) await Promise.resolve();
  assert.equal(settled, false);
  assert.equal(calls.includes("stop_execution_census"), true);
  release({ droppedEvents: 0, droppedChunks: 0 });
  await pending;
  assert.equal(messages.find((m) => m.message === "execution-census-ready").detail.complete, false);
});

test("mode change and rearm are held while frozen census pages drain", () => {
  const { session, calls } = fixture();
  session.frameProbe = null;
  session.censusDrain = new Promise(() => {});
  session.setReceiptMode("checked");
  session.armFrameProbe("next-proof");
  assert.equal(session.requestedFrameProbe, undefined);
  assert.equal(calls.includes("mode"), false);
});
