import assert from "node:assert/strict";
import { test } from "node:test";
import { CompletedWindowsCalls } from "../diagnostics.js";

function frame({
  id = 0x90907,
  token = 1,
  convention = 2,
  args = [0x4000],
  esp = 0x8000,
  pc = 0x1234,
} = {}) {
  const view = new DataView(new ArrayBuffer(112));
  [0x46433352, 0x10001, 112, 0, token, id, convention, args.length, 0x70000000, esp, pc, 0].forEach(
    (value, index) => view.setUint32(index * 4, value, true),
  );
  args.forEach((value, index) => view.setUint32(48 + index * 4, value, true));
  return view;
}
function fixture(options = {}) {
  const state = new Map([
    [48, 0],
    [32, 0],
    [16, 0],
  ]);
  let now = 10;
  const engine = { api: {}, get: (offset) => state.get(offset) };
  const tracker = new CompletedWindowsCalls(engine, { now: () => now++, ...options });
  const capture = (settings = {}, thread = 1) => {
    const packet = frame(settings);
    tracker.capture({ id: packet.getUint32(20, true), pc: 0x70000000 }, packet, thread);
    return packet;
  };
  const returned = (pc, esp, result) => {
    state.set(48, pc);
    state.set(32, esp);
    state.set(16, result);
  };
  return { engine, tracker, capture, returned };
}

test("completed calls require their exact thread, return PC and convention-specific ESP", () => {
  const f = fixture();
  const input = f.capture({ args: [0x4000, 0x5000, 6] });
  input.setUint32(48, 0xffff, true);
  f.returned(0x1234, 0x8008, 0x80070005);
  f.tracker.settle(1);
  assert.equal(f.tracker.snapshot().lastCompletedWindowsCall, undefined);
  f.returned(0x1234, 0x8010, 0x80070005);
  f.tracker.settle(2);
  assert.equal(f.tracker.snapshot().pendingReturns, 1);
  f.tracker.settle(1);
  const record = f.tracker.snapshot().lastCompletedWindowsCall;
  assert.deepEqual(record.args, [0x4000, 0x5000, 6]);
  assert.equal(record.result, 0x80070005);
  assert.equal(record.resultSigned, -2147024891);
  assert.equal(record.resultHex, "0x80070005");
  assert.equal(record.completedAtMs, 10);
  assert.equal(f.tracker.snapshot().pendingReturns, 0);

  for (const [convention, expected] of [
    [1, 0x8004],
    [3, 0x8010],
  ]) {
    f.capture({ convention, args: [1, 2, 3] });
    f.returned(0x1234, expected, 0);
    f.tracker.settle(1);
    assert.equal(f.tracker.snapshot().pendingReturns, 0);
  }
});

test("nested callbacks and another thread retain independent return correlations", () => {
  const f = fixture();
  f.capture({ id: 0x9090d, args: [0x4000, 1, 6], token: 1, pc: 0x1000, esp: 0x8000 });
  f.capture({ id: 0x90907, args: [0x4000], token: 2, pc: 0x2000, esp: 0x7f00 });
  f.capture({ id: 0x90909, args: [0x5000, 20, 0x9000], token: 3, pc: 0x2000, esp: 0x7f00 }, 2);
  f.returned(0x2000, 0x7f08, 1);
  f.tracker.settle(1);
  assert.equal(f.tracker.snapshot().lastCompletedWindowsCall.token, 2);
  assert.equal(f.tracker.snapshot().pendingReturns, 2);
  f.returned(0x2000, 0x7f10, 0);
  f.tracker.settle(2);
  assert.equal(f.tracker.snapshot().lastCompletedWindowsCall.token, 3);
  f.returned(0x1000, 0x8010, 0);
  f.tracker.settle(1);
  const snapshot = f.tracker.snapshot();
  assert.equal(snapshot.pendingReturns, 0);
  assert.deepEqual(
    snapshot.directInputCalls.map(({ method, result }) => [method, result]),
    [
      ["Acquire", 1],
      ["GetDeviceState", 0],
      ["SetCooperativeLevel", 0],
    ],
  );
  snapshot.directInputCalls[0].args[0] = 99;
  assert.equal(f.tracker.snapshot().directInputCalls[0].args[0], 0x4000);
});

test("diagnostic pending and sticky bounds are visible without failing guest control", () => {
  const f = fixture({ pendingLimit: 2, stickyLimit: 2 });
  for (let token = 1; token <= 3; token++)
    f.capture({ token, pc: 0x1000 + token, args: [0x4000 + token] });
  assert.equal(f.tracker.snapshot().droppedReturns, 1);
  assert.equal(f.tracker.snapshot().pendingReturns, 2);
  for (let token = 2; token <= 4; token++) {
    if (token === 4) f.capture({ token, pc: 0x1000 + token, args: [0x4000 + token] });
    f.returned(0x1000 + token, 0x8008, 0);
    f.tracker.settle(1);
  }
  const snapshot = f.tracker.snapshot();
  assert.equal(snapshot.directInputCalls.length, 2);
  assert.equal(snapshot.droppedDirectInput, 1);
  const invalid = frame();
  invalid.setUint32(24, 4, true);
  f.tracker.capture({ id: 0x90907 }, invalid, 1);
  assert.equal(f.tracker.snapshot().malformedCalls, 1);
});

test("coarse input samples validate the existing media schema and never query a closed engine", () => {
  const f = fixture();
  const packet = new Uint8Array(96),
    data = new DataView(packet.buffer);
  data.setUint32(0, 0x524d3352, true);
  data.setUint32(4, 0x90001, true);
  data.setUint32(8, 64, true);
  data.setBigUint64(16, 7n, true);
  data.setBigUint64(24, 3n, true);
  packet[64 + (0xd0 >>> 3)] = 1 << (0xd0 & 7);
  let queries = 0;
  f.engine.api.input_state = () => {};
  f.engine.ok = (operation) => {
    assert.equal(operation, "input_state");
    queries++;
  };
  f.engine.take = () => packet;
  f.tracker.sampleInput();
  assert.deepEqual(f.tracker.snapshot().inputState, {
    effectiveFocused: true,
    effectiveCaptured: true,
    epoch: "7",
    pressedKeys: [0xd0],
    pressedKeyCount: 1,
    clippedKeys: 0,
    sampledAtMs: 10,
  });
  f.tracker.snapshot().inputState.pressedKeys[0] = 0;
  assert.deepEqual(f.tracker.snapshot().inputState.pressedKeys, [0xd0]);
  f.tracker.sampleInput({ closed: true });
  f.tracker.snapshot();
  assert.equal(queries, 1);
  data.setUint32(4, 0x80001, true);
  f.tracker.sampleInput();
  assert.equal(f.tracker.snapshot().inputState.error, "invalid input state descriptor");
  assert.equal(queries, 2);
  delete f.engine.api.input_state;
  f.tracker.sampleInput();
  assert.equal(queries, 2);
});

test("raw negative return values are bounded observations rather than inferred API errors", () => {
  const f = fixture();
  for (let index = 0; index < 20; index++) {
    f.capture({ id: 0xf000b, token: index + 1 });
    f.returned(0x1234, 0x8008, 0x80000000 + index);
    f.tracker.settle(1);
  }
  f.capture({ id: 0xf000b, token: 21 });
  f.returned(0x1234, 0x8008, 1);
  f.tracker.settle(1);
  const snapshot = f.tracker.snapshot();
  assert.equal(snapshot.recentNegativeWindowsCalls.length, 16);
  assert.equal(snapshot.overwrittenNegativeResults, 4);
  assert.equal(snapshot.recentNegativeWindowsCalls[0].resultHex, "0x80000004");
  assert.equal(snapshot.recentNegativeWindowsCalls.at(-1).resultHex, "0x80000013");
  assert.equal(
    snapshot.recentNegativeWindowsCalls.some((record) => "error" in record),
    false,
  );
  snapshot.recentNegativeWindowsCalls[0].args[0] = 0;
  assert.equal(f.tracker.snapshot().recentNegativeWindowsCalls[0].args[0], 0x4000);
});

test("D3D sticky records remain bounded while retaining cooperative, reset and present evidence", () => {
  const f = fixture();
  const ids = [0x40203, 0x4020e, 0x4020f];
  for (const id of ids) {
    f.capture({ id, token: id });
    f.returned(0x1234, 0x8008, 0);
    f.tracker.settle(1);
  }
  for (let index = 0; index < 40; index++) {
    f.capture({ id: 0x4050c, token: index + 1, args: [0x5000 + index] });
    f.returned(0x1234, 0x8008, 0);
    f.tracker.settle(1);
  }
  const snapshot = f.tracker.snapshot();
  assert.equal(snapshot.recentD3DCalls.length, 32);
  assert.equal(snapshot.droppedD3D, 11);
  assert.deepEqual(
    snapshot.recentD3DCalls.filter(({ id }) => ids.includes(id)).map(({ method }) => method),
    ["TestCooperativeLevel", "Reset", "Present"],
  );
  assert.equal(snapshot.recentD3DCalls.at(-1).args[0], 0x5000 + 39);
});

test("clock call metadata is sampled once per second without reading guest output memory", () => {
  let now = 0;
  const f = fixture({ now: () => now });
  f.engine.api.read32 = () => assert.fail("clock telemetry must not modify the memory helper");
  for (const [time, id, pointer] of [
    [0, 0xc0010, 0x8000],
    [500, 0xc0010, 0x9000],
    [500, 0xc0011, 0xa000],
    [1000, 0xc0010, 0xb000],
  ]) {
    now = time;
    f.capture({ id, token: time + id, args: [pointer] });
    f.returned(0x1234, 0x8008, 1);
    f.tracker.settle(1);
    if (time === 500 && id === 0xc0010)
      assert.equal(f.tracker.snapshot().clockCalls[0].args[0], 0x8000);
  }
  const snapshot = f.tracker.snapshot();
  assert.equal(snapshot.clockOutputRead, false);
  assert.equal(snapshot.clockCalls.length, 2);
  assert.equal(snapshot.clockCalls.find(({ id }) => id === 0xc0010).args[0], 0xb000);
  assert.equal(snapshot.clockCalls.find(({ id }) => id === 0xc0011).args[0], 0xa000);
});
