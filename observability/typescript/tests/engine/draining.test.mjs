import assert from "node:assert/strict";
import test from "node:test";
import { categories, createCapture } from "../../dist/index.js";
import { event } from "../../dist/perfetto/events.js";
import { get, memoryOutput, packets } from "../perfetto/decode.mjs";

function source(count, payload = "") {
  let next = 0;
  let terminal = false;
  return {
    supported: true,
    reads: 0,
    finishes: 0,
    configure() {},
    drain() {
      this.reads++;
      return {
        bytes:
          next < count
            ? event(88, BigInt(++next), 1n, 3, categories.engineCpu, "fixture", { payload })
            : new Uint8Array(),
        finished: terminal && next === count,
        droppedEvents: 0n,
      };
    },
    finish() {
      this.finishes++;
      terminal = true;
      return this.drain();
    },
  };
}

const recorded = (sink) =>
  packets(sink.chunks).filter((value) => get(value.packet, 10) === 88n).length;

test("one normal flush drains a multi-chunk engine backlog before the next interval", async (t) => {
  const sink = memoryOutput(),
    port = source(300);
  const capture = createCapture({ level: "detailed", output: sink });
  t.after(() => capture.finish());
  capture.attachEngine(port, { producerId: 88 });
  await capture.flush();
  assert.equal(recorded(sink), 300);
  assert.equal(port.reads, 301);
  assert.equal((await capture.finish()).complete, true);
});

test("nonblocking engine drain batches stop at their caller's chunk budget", async (t) => {
  const sink = memoryOutput(),
    port = source(100);
  const capture = createCapture({ level: "detailed", output: sink });
  t.after(() => capture.finish());
  capture.attachEngine(port, { producerId: 88 });
  const drained = capture.drainEngine({ maxChunks: 7 });
  assert.equal(port.reads, 7);
  assert.equal(drained.chunks, 7);
  assert.equal(drained.pending, true);
  assert.equal((await capture.finish()).complete, true);
  assert.equal(recorded(sink), 100);
});

test("engine flush waits for a small output budget instead of discarding later packets", async (t) => {
  const sink = memoryOutput(),
    port = source(200, "x".repeat(600));
  const capture = createCapture({
    level: "detailed",
    output: sink,
    maxBufferedBytes: 1024,
    batchBytes: 256,
  });
  t.after(() => capture.finish());
  capture.attachEngine(port, { producerId: 88 });
  await capture.flush();
  assert.equal(recorded(sink), 200);
  const result = await capture.finish();
  assert.equal(result.complete, true, JSON.stringify(result));
  assert.equal(result.droppedEvents, 0);
  assert.equal(result.droppedChunks, 0);
});

test("finish joins an active engine flush and records each packet exactly once", async (t) => {
  const sink = memoryOutput(),
    port = source(200, "x".repeat(600));
  let release;
  const gate = new Promise((resolve) => {
    release = resolve;
  });
  let first = true;
  const capture = createCapture({
    level: "detailed",
    maxBufferedBytes: 1024,
    batchBytes: 256,
    output: {
      async write(bytes) {
        if (first) {
          first = false;
          await gate;
        }
        await sink.write(bytes);
      },
      async flush() {},
    },
  });
  t.after(() => capture.finish());
  capture.attachEngine(port, { producerId: 88 });
  const flushing = capture.flush();
  await Promise.resolve();
  const finishing = capture.finish("cancelled");
  release();
  await flushing;
  const result = await finishing;
  assert.equal(result.complete, true, JSON.stringify(result));
  assert.equal(recorded(sink), 200);
  assert.equal(port.finishes, 1);
});

test("drain budgets reject zero and asynchronous draining yields to control tasks", async (t) => {
  const sink = memoryOutput(),
    port = source(1000);
  const capture = createCapture({ level: "detailed", output: sink });
  t.after(() => capture.finish());
  capture.attachEngine(port, { producerId: 88 });
  for (const options of [{ maxChunks: 0 }, { maxBytes: 0 }, { budgetMs: 0 }])
    assert.throws(() => capture.drainEngine(options), RangeError);
  assert.equal(port.reads, 0);
  let readsAtControlTask;
  const control = new Promise((resolve) =>
    setTimeout(() => {
      readsAtControlTask = port.reads;
      resolve();
    }, 0),
  );
  await capture.flush();
  await control;
  assert.ok(readsAtControlTask > 0 && readsAtControlTask < 1000);
  assert.equal(recorded(sink), 1000);
});

test("a trace packet larger than a tiny capture budget finishes explicitly incomplete", async () => {
  const capture = createCapture({
    level: "detailed",
    output: memoryOutput(),
    maxBufferedBytes: 1024,
  });
  capture.attachEngine(source(1, "x".repeat(1500)), { producerId: 88 });
  await capture.flush();
  const result = await capture.finish("cancelled");
  assert.equal(result.complete, false);
  assert.match(result.recordingError, /packet exceeds capture byte budget/);
});

test("a failed output stops the pump and retains an incomplete terminal result", async () => {
  let aborted = false;
  const port = source(1000, "x".repeat(600));
  const capture = createCapture({
    level: "detailed",
    maxBufferedBytes: 1024,
    batchBytes: 256,
    output: {
      async write() {
        throw new Error("sink is unavailable");
      },
      async flush() {},
      async abort() {
        aborted = true;
      },
    },
  });
  capture.attachEngine(port, { producerId: 88 });
  await capture.flush();
  assert.ok(port.reads < 1000);
  const result = await capture.finish("failed");
  assert.equal(result.complete, false);
  assert.equal(result.outcome, "failed");
  assert.match(result.sinkError, /sink is unavailable/);
  assert.equal(aborted, true);
  assert.equal(await capture.finish(), result);
});

test("a fragmented oversized packet is rejected from its header before reassembly allocation", async () => {
  const oversized = event(88, 1n, 1n, 3, categories.engineCpu, "fixture", {
    payload: "x".repeat(9000),
  });
  let offset = 0,
    finalized = false;
  const port = {
    supported: true,
    maxChunkBytes: 128,
    configure() {},
    drain() {
      const bytes = oversized.slice(offset, offset + 128);
      offset += bytes.length;
      return { bytes, finished: finalized && offset === oversized.length, droppedEvents: 0n };
    },
    finish() {
      finalized = true;
      return this.drain();
    },
  };
  const capture = createCapture({
    level: "detailed",
    output: memoryOutput(),
    maxBufferedBytes: 8192,
  });
  capture.attachEngine(port, { producerId: 88 });
  await capture.flush();
  assert.equal(offset, 128, "the header rejects the declared length before body collection");
  assert.ok(capture.status().bufferedBytes <= 8192);
  const result = await capture.finish("cancelled");
  assert.equal(result.complete, false);
  assert.match(result.recordingError, /packet exceeds capture byte budget/);
});
