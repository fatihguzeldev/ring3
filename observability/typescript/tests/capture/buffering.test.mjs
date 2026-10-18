import assert from "node:assert/strict";
import test from "node:test";
import { BatchWriter } from "../../dist/capture/buffer.js";

test("flush drains arrivals queued at the completion microtask boundary", async () => {
  const written = [];
  const writer = new BatchWriter(
    {
      async write(bytes) {
        written.push(...bytes);
      },
      async flush() {},
    },
    1024,
    1,
  );
  writer.append(Uint8Array.of(1));
  queueMicrotask(() => writer.append(Uint8Array.of(2)));
  await writer.flush();
  assert.deepEqual(written, [1, 2]);
  assert.equal(writer.bufferedBytes, 0);
});
