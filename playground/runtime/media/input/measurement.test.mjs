import assert from "node:assert/strict";
import { test } from "node:test";
import { InputMeasurements } from "./measurement.js";
import { Media } from "../../media.js";

function fixture(enabled = true) {
  let failure;
  const requests = [];
  const engine = {
    inputMeasurementsEnabled: enabled,
    request: (bytes) => requests.push(bytes.slice()),
    ok(operation) { if (operation === "submit_windows_input" && failure) throw failure; },
    media: () => ({ values: [0n] }),
  };
  const media = new Media(engine, 7n, () => {});
  media.audio = {};
  media.publishDialogs = () => {};
  media.publishGdi = () => {};
  return { media, requests, fail: (error) => { failure = error; } };
}

test("successful submissions expose both extended-key edges and their actual wire sequence", () => {
  const { media, requests } = fixture();
  media.enqueue([{ kind: "key", code: 0xd0, down: true }]);
  media.update(12n);
  media.enqueue([{ kind: "key", code: 0xd0, down: false }]);
  media.update(19n);
  const status = media.inputStatus();
  assert.equal(status.enqueuedEvents, 2);
  assert.equal(status.submittedEvents, 2);
  assert.equal(status.submittedBatches, 2);
  assert.equal(status.queuedEvents, 0);
  assert.deepEqual(status.history.map(({ kind, code, down, sequence, timestampNs }) =>
    ({ kind, code, down, sequence, timestampNs })), [
    { kind: "key", code: 0xd0, down: true, sequence: "1", timestampNs: "12" },
    { kind: "key", code: 0xd0, down: false, sequence: "2", timestampNs: "19" },
  ]);
  assert.equal(new DataView(requests[0].buffer).getUint32(44, true), 0xd0);
  assert.equal(new DataView(requests[0].buffer).getUint32(48, true), 1);
  assert.equal(new DataView(requests[1].buffer).getUint32(48, true), 0);
});

test("failed submissions are not reported as accepted and history stays bounded", () => {
  const f = fixture();
  for (let at = 0; at < 12; at++) {
    f.media.enqueue([{ kind: "key", code: 0xd0, down: at % 2 === 0 }]);
    f.media.update(BigInt(at));
  }
  const before = f.media.inputStatus();
  assert.equal(before.history.length, 8);
  const failure = new Error("engine rejected input");
  f.fail(failure);
  f.media.enqueue([{ kind: "key", code: 0xd0, down: true }]);
  assert.throws(() => f.media.update(15n), (error) => error === failure);
  const after = f.media.inputStatus();
  assert.equal(after.enqueuedEvents, 13);
  assert.equal(after.submittedEvents, 12);
  assert.deepEqual(after.history, before.history);
});

test("off input measurements allocate no tracker and return no snapshot", () => {
  const { media } = fixture(false);
  media.enqueue([{ kind: "key", code: 0xd0, down: true }]);
  media.update(12n);
  assert.equal(media.inputMeasurements, null);
  assert.equal(media.inputStatus(), undefined);
});


test("latency follows accepted event ownership across partial batches and uses separate clocks", (t) => {
  let clock = 10;
  t.mock.method(performance, "now", () => clock);
  const tracker = new InputMeasurements();
  tracker.enqueue(2, performance.timeOrigin + 6);
  clock = 20;
  tracker.enqueue(1, performance.timeOrigin + 19);
  clock = 30;
  tracker.submitted([{ kind: "key", code: 1, down: true }], 1n, 50n);
  clock = 40;
  tracker.pointer(2, 3, 60n);
  clock = 50;
  tracker.submitted([{ kind: "key", code: 1, down: false }], 2n, 70n);
  const status = tracker.snapshot(0);
  assert.deepEqual(status.latency.transport, { count: 2, sumNs: 5_000_000, maximumNs: 4_000_000 });
  assert.deepEqual(status.latency.workerQueue, { count: 3, sumNs: 80_000_000, maximumNs: 30_000_000 });
  assert.equal(tracker.pending.length, 0);
  assert.equal(status.history[0].timestampNs, "50");
});
