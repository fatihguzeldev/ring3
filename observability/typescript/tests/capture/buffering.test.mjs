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

test("flush joins a write admitted by an output flush callback", async (t) => {
  let release,
    injected = false;
  const gate = new Promise((resolve) => {
    release = resolve;
  });
  t.after(() => release());
  const written = [];
  const writer = new BatchWriter(
    {
      async write(bytes) {
        if (bytes[0] === 2) await gate;
        written.push(...bytes);
      },
      async flush() {
        if (!injected) {
          injected = true;
          writer.append(Uint8Array.of(2));
        }
      },
    },
    1024,
    1,
  );
  writer.append(Uint8Array.of(1));
  let finished = false;
  const flushing = writer.flush().then(() => {
    finished = true;
  });
  for (let index = 0; index < 20; index++) await Promise.resolve();
  assert.equal(finished, false);
  release();
  await flushing;
  assert.deepEqual(written, [1, 2]);
  assert.equal(writer.bufferedBytes, 0);
});
