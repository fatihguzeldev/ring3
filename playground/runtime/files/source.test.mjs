import assert from "node:assert/strict";
import { test } from "node:test";
import { SOURCE_READ_BYTES, sourceChunks } from "./source.js";

async function collect(iterator) {
  const parts = [];
  for await (const part of iterator) parts.push(part);
  return parts;
}

test("source batching is bounded and never reads beyond the requested span", async () => {
  const content = Uint8Array.from({ length: SOURCE_READ_BYTES + 50 }, (_, i) => i % 251);
  const reads = [];
  const engine = {
    async bytes(url, id, offset, length) {
      reads.push({ id, offset, length });
      return content.slice(Number(offset), Number(offset) + length);
    },
  };
  const parts = await collect(
    sourceChunks(engine, "/bytes", { id: 9, size: content.length }, 17n, SOURCE_READ_BYTES + 15),
  );
  assert.deepEqual(reads, [
    { id: 9n, offset: 17n, length: SOURCE_READ_BYTES },
    { id: 9n, offset: BigInt(17 + SOURCE_READ_BYTES), length: 15 },
  ]);
  assert.deepEqual(
    parts.map((part) => part.offset),
    [0, SOURCE_READ_BYTES],
  );
  assert.deepEqual(
    new Uint8Array(Buffer.concat(parts.map((part) => part.bytes))),
    content.slice(17, 17 + SOURCE_READ_BYTES + 15),
  );
});

test("invalid asset spans fail before upstream access", async () => {
  const engine = {
    bytes() {
      assert.fail("invalid span must not be read");
    },
  };
  for (const [offset, length] of [
    [-1n, 1],
    [0n, -1],
    [0n, 0.5],
    [99n, 2],
  ])
    await assert.rejects(
      collect(sourceChunks(engine, "/bytes", { id: 1, size: 100 }, offset, length)),
      RangeError,
    );
});

test("zero-length requests make no upstream call", async () => {
  const engine = {
    bytes() {
      assert.fail("empty span must not be read");
    },
  };
  assert.deepEqual(
    await collect(sourceChunks(engine, "/bytes", { id: 1, size: 100 }, 100n, 0)),
    [],
  );
});

test("short reads are rejected before any bytes are yielded", async () => {
  const engine = {
    async bytes() {
      return new Uint8Array(2);
    },
  };
  await assert.rejects(
    collect(sourceChunks(engine, "/bytes", { id: 1, size: 4 }, 0n, 4)),
    /short or invalid read/,
  );
});

test("cancellation after upstream I/O rejects with the original reason", async () => {
  const abort = new AbortController();
  let complete;
  const engine = {
    signal: abort.signal,
    bytes: () =>
      new Promise((resolve) => {
        complete = resolve;
      }),
  };
  const next = sourceChunks(engine, "/bytes", { id: 1, size: 4 }, 0n, 4).next();
  const reason = new Error("cancel file read");
  abort.abort(reason);
  complete(new Uint8Array(4));
  await assert.rejects(next, (error) => error === reason);
});

test("upstream failures retain their original error", async () => {
  const failure = new Error("asset identity changed");
  const engine = {
    async bytes() {
      throw failure;
    },
  };
  await assert.rejects(
    collect(sourceChunks(engine, "/bytes", { id: 1, size: 4 }, 0n, 4)),
    (error) => error === failure,
  );
});
