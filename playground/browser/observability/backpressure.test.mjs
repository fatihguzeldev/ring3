import assert from "node:assert/strict";
import { test } from "node:test";
import { BrowserCapture } from "../observability.js";
import { categories } from "../../../observability/typescript/dist/index.js";
import { event } from "../../../observability/typescript/dist/perfetto/events.js";

async function settle() {
  for (let index = 0; index < 20; index++) await Promise.resolve();
}

test("browser ACK waits for bounded capture output rather than accepting an unflushed burst", async (t) => {
  let release;
  const gate = new Promise((resolve) => {
    release = resolve;
  });
  const capture = new BrowserCapture(
    { level: "basic" },
    {
      id: "fixture",
      async write() {
        await gate;
      },
      async flush() {},
      async close() {},
    },
    () => {},
  );
  t.after(async () => {
    release();
    await capture.finish("cancelled");
  });
  const acknowledgements = [];
  const packet = event(2, 1n, 1n, 3, categories.hostWorker, "fixture", {});
  assert.equal(
    capture.receive({ type: "observability-data", sequence: 0, bytes: packet.buffer }, (ack) =>
      acknowledgements.push(ack),
    ),
    true,
  );
  await settle();
  assert.equal(acknowledgements.length, 0);
  release();
  await settle();
  assert.deepEqual(acknowledgements, [{ type: "observability-ack", sequence: 0, accepted: true }]);
});

test("a failed browser output rejects the worker ACK and preserves an incomplete capture", async (t) => {
  const capture = new BrowserCapture(
    { level: "basic" },
    {
      id: "fixture",
      async write() {
        throw new Error("disk failed");
      },
      async flush() {},
      async abort() {},
    },
    () => {},
  );
  t.after(() => capture.finish("cancelled"));
  const acknowledgements = [];
  const packet = event(2, 1n, 1n, 3, categories.hostWorker, "fixture", {});
  await capture.receiveBytes({ sequence: 0, bytes: packet.buffer }, (ack) =>
    acknowledgements.push(ack),
  );
  assert.deepEqual(acknowledgements, [
    { type: "observability-ack", sequence: 0, accepted: false, error: "disk failed" },
  ]);
  assert.equal((await capture.finish("cancelled")).complete, false);
});
