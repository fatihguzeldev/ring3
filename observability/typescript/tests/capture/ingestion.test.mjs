import assert from "node:assert/strict";
import test from "node:test";
import { categories, createCapture } from "../../dist/index.js";
import { event } from "../../dist/perfetto/events.js";
import { get, memoryOutput, packets } from "../perfetto/decode.mjs";

const packet = () =>
  event(88, 1n, 1n, 3, categories.hostWorker, "fixture", { payload: "x".repeat(600) });
const recorded = (sink) =>
  packets(sink.chunks).filter((value) => get(value.packet, 10) === 88n).length;

async function settle() {
  for (let index = 0; index < 12; index++) await Promise.resolve();
}

test("asynchronous ingestion waits for output capacity without losing admitted packets", async () => {
  const sink = memoryOutput();
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
  assert.equal(await capture.ingestAsync(packet()), true);
  let accepted = false;
  const second = capture.ingestAsync(packet()).then((result) => {
    accepted = result;
  });
  await settle();
  assert.equal(accepted, false);
  release();
  await second;
  assert.equal(accepted, true);
  const result = await capture.finish();
  assert.equal(result.complete, true, JSON.stringify(result));
  assert.equal(recorded(sink), 2);
});

test("finish joins accepted asynchronous input and preserves caller buffer ownership", async () => {
  const sink = memoryOutput(),
    capture = createCapture({ level: "basic", output: sink });
  const input = packet();
  const accepted = capture.ingestAsync(input);
  input.fill(0);
  const result = await capture.finish("cancelled");
  assert.equal(await accepted, true);
  assert.equal(result.complete, true, JSON.stringify(result));
  assert.equal(recorded(sink), 1);
});

test("concurrent asynchronous inputs are bounded and rejected bytes mark the capture incomplete", async () => {
  const capture = createCapture({ level: "basic", output: memoryOutput(), maxBufferedBytes: 1024 });
  const first = capture.ingestAsync(packet());
  const rejected = capture.ingestAsync(packet());
  assert.equal(await rejected, false);
  assert.equal(await first, true);
  const result = await capture.finish();
  assert.equal(result.complete, false);
  assert.match(result.recordingError, /input byte budget/);
  assert.equal(result.droppedChunks, 1);
});

test("off asynchronous ingestion does not read the clock or write output", async () => {
  const capture = createCapture({
    level: "off",
    clock: () => assert.fail("clock read"),
    output: { write: () => assert.fail("output write"), flush: () => assert.fail("output flush") },
  });
  assert.equal(await capture.ingestAsync(packet()), false);
  assert.equal((await capture.finish()).complete, true);
});

test("ordered asynchronous fragments survive concurrent finish without interleaving packets", async () => {
  const sink = memoryOutput();
  const capture = createCapture({ level: "basic", output: sink, maxBufferedBytes: 2048 });
  const bytes = packet();
  const first = capture.ingestAsync(bytes.subarray(0, 100));
  const second = capture.ingestAsync(bytes.subarray(100));
  const result = await capture.finish();
  assert.deepEqual(await Promise.all([first, second]), [true, true]);
  assert.equal(result.complete, true, JSON.stringify(result));
  assert.equal(result.queuedInputBytes, 0);
  assert.equal(recorded(sink), 1);
});

test("synchronous input cannot splice into a pending asynchronous source packet", async () => {
  const capture = createCapture({ level: "basic", output: memoryOutput() });
  const input = capture.ingestAsync(packet());
  assert.equal(capture.ingest(packet()), false);
  assert.equal(await input, true);
  const result = await capture.finish();
  assert.equal(result.complete, false);
  assert.match(result.recordingError, /cannot interleave/);
});
