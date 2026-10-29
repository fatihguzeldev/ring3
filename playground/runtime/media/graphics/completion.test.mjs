import assert from "node:assert/strict";
import { test } from "node:test";
import { fixture, complete } from "./support.mjs";

test("one in-flight batch blocks further transport until its exact completion", async () => {
  const f = fixture([
    { epoch: 11n, batches: 2 },
    { epoch: 22n, batches: 1 },
  ]);
  await f.media.graphicsOutput();
  const pending = f.media.graphicsStatus().pending,
    count = f.operations.length;
  await f.media.graphicsOutput();
  assert.equal(f.operations.length, count);
  f.media.graphicsComplete({ requestId: pending.requestId + 1 });
  assert.throws(() => complete(f, { epoch: "22" }), /identity mismatch/);
  assert.throws(() => complete(f, { sequence: "2" }), /identity mismatch/);
  assert.equal(f.inventory[0].batches, 2);
  complete(f);
  await f.media.graphicsOutput();
  assert.equal(f.media.graphicsStatus().pending.epoch, "22");
});

test("readback completion precedes batch ACK and a rejected readback aborts its staged input", async () => {
  const f = fixture([{ epoch: 11n, batches: 1 }]);
  await f.media.graphicsOutput();
  const readback = { receipt: new Uint8Array(64), bytes: new Uint8Array(5000) };
  const error = new Error("readback rejected");
  f.fail("append_d3d8_readback_input", error);
  assert.throws(
    () => complete(f, { readbacks: [readback] }),
    (actual) => actual === error,
  );
  assert.equal(f.operations.at(-1), "abort_d3d8_readback_input");
  assert.equal(f.operations.includes("d3d8_acknowledge_batch"), false);
  assert.equal(f.media.graphicsStatus().pending.epoch, "11");
});

test("all successful readback chunks finish before the batch is acknowledged once", async () => {
  const f = fixture([{ epoch: 11n, batches: 1 }]);
  await f.media.graphicsOutput();
  complete(f, { readbacks: [{ receipt: new Uint8Array(64), bytes: new Uint8Array(5000) }] });
  assert.deepEqual(f.operations.slice(-5), [
    "begin_d3d8_readback_input",
    "append_d3d8_readback_input",
    "append_d3d8_readback_input",
    "finish_d3d8_readback_input",
    "d3d8_acknowledge_batch",
  ]);
  assert.equal(f.media.graphicsStatus().pending, null);
});

test("cancelled transport neither polls new work nor ACKs a late in-flight completion", async () => {
  const f = fixture([{ epoch: 11n, batches: 1 }]);
  await f.media.graphicsOutput();
  f.abort();
  const count = f.operations.length;
  complete(f);
  await f.media.graphicsOutput();
  assert.equal(f.operations.length, count);
  assert.equal(f.inventory[0].batches, 1);
});

test("cancellation while copying a batch cannot emit a new in-flight request", async () => {
  const f = fixture([{ epoch: 11n, batches: 1 }]);
  f.onRead(f.abort);
  await f.media.graphicsOutput();
  assert.equal(f.emitted.length, 0);
  assert.equal(f.media.graphicsStatus().pending, null);
  assert.equal(f.inventory[0].batches, 1);
});


test("completion latency excludes rejected identities and failed ACKs", async (t) => {
  let clock = 10;
  t.mock.method(performance, "now", () => clock);
  const f = fixture([{ epoch: 11n, batches: 1 }], true);
  await f.media.graphicsOutput();
  clock = 25;
  assert.equal(f.media.graphicsStatus().completionLatency.pendingAgeMs, 15);
  assert.throws(() => complete(f, { sequence: "2" }), /identity mismatch/);
  const error = new Error("ACK rejected");
  f.fail("d3d8_acknowledge_batch", error);
  assert.throws(() => complete(f), actual => actual === error);
  assert.equal(f.media.graphicsStatus().completionLatency.count, 0);
  f.fail(null);
  clock = 40;
  complete(f);
  assert.deepEqual(f.media.graphicsStatus().completionLatency, {
    semantics: "Worker batch post to successful engine ACK; includes browser GPU work and Worker scheduling delay",
    count: 1, sumMs: 30, maximumMs: 30, pendingAgeMs: null,
  });
});
