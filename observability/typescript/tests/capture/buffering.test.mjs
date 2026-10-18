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

test("reentrant output callbacks cannot start concurrent writes", async () => {
  const written = [];
  let concurrent = 0,
    maximum = 0,
    writer;
  writer = new BatchWriter(
    {
      async write(bytes) {
        concurrent++;
        maximum = Math.max(maximum, concurrent);
        written.push([...bytes]);
        if (bytes[0] === 1) writer.append(Uint8Array.of(2));
        await Promise.resolve();
        concurrent--;
      },
      async flush() {},
    },
    1024,
    1,
  );
  writer.append(Uint8Array.of(1));
  await writer.flush();
  assert.equal(maximum, 1);
  assert.deepEqual(written, [[1], [2]]);
  assert.equal(writer.bufferedBytes, 0);
});
