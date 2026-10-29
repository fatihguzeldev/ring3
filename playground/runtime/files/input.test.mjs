import assert from "node:assert/strict";
import { test } from "node:test";
import { Files } from "../files.js";

test("a file request batches upstream reads while preserving bounded ABI delivery", async () => {
  const source = Uint8Array.from({ length: 9000 }, (_, i) => i % 251);
  const request = new Uint8Array(80),
    header = new DataView(request.buffer);
  header.setBigUint64(16, 1n, true);
  header.setBigUint64(48, 7n, true);
  header.setBigUint64(56, 10n, true);
  header.setUint32(64, source.length, true);
  const reads = [],
    delivered = [];
  let input;
  const engine = {
    fileMeasurementsEnabled: true,
    take: () => request.slice(),
    request(bytes) {
      assert.ok(bytes.length <= 4096);
      input = bytes.slice();
    },
    async bytes(url, id, offset, length) {
      reads.push({ id, offset, length });
      return source.slice(Number(offset - 10n), Number(offset - 10n) + length);
    },
    ok(operation, offset, length) {
      if (operation !== "supply_file_bytes") return;
      assert.deepEqual(input.subarray(0, 80), request);
      delivered.push({ offset, bytes: input.slice(80, 80 + length) });
    },
  };
  const files = new Files(engine, { files: [{ id: 7, size: 9010, path: "C:\\data" }] }, "/bytes");
  assert.equal(await files.supplyPending(), true);
  assert.equal(reads.length, 1);
  assert.deepEqual(reads[0], { id: 7n, offset: 10n, length: 9000 });
  assert.deepEqual(
    delivered.map((part) => [part.offset, part.bytes.length]),
    [
      [0, 4016],
      [4016, 4016],
      [8032, 968],
    ],
  );
  assert.deepEqual(new Uint8Array(Buffer.concat(delivered.map((part) => part.bytes))), source);
  assert.equal(files.fetched, source.length);
  assert.deepEqual(files.snapshot().lastRead, {
    asset: "7", path: "C:\\data", offset: "10", length: 9000, kind: "file",
  });
  assert.equal(files.snapshot().sourceReadBytes, files.fetched);
});

test("executable staging separates source batches from 4 KiB module payloads", async () => {
  const source = new Uint8Array(1024 * 1024 + 17),
    pe = new DataView(source.buffer);
  pe.setUint16(0, 0x5a4d, true);
  pe.setUint32(60, 64, true);
  pe.setUint32(64, 0x4550, true);
  pe.setUint16(68, 0x14c, true);
  pe.setUint16(84, 112, true);
  pe.setUint16(88, 0x10b, true);
  const reads = [],
    appended = [];
  let input,
    committed = false;
  const engine = {
    async bytes(url, id, offset, length) {
      reads.push(length);
      return source.slice(Number(offset), Number(offset) + length);
    },
    name: () => 7,
    request(bytes) {
      assert.ok(bytes.length <= 4096);
      input = bytes.slice();
    },
    ok(operation, offset, length) {
      if (operation === "append_module_input")
        appended.push({ offset, bytes: input.slice(0, length) });
      if (operation === "commit_module_input") committed = true;
    },
  };
  const files = new Files(
    engine,
    { files: [{ id: 1, size: source.length, path: "C:\\game.exe" }] },
    "/bytes",
  );
  assert.equal(files.snapshot(), undefined);
  await files.stageExecutable("C:\\game.exe");
  assert.deepEqual(reads, [4096, 1024 * 1024, 17]);
  assert.equal(appended.length, 257);
  assert.deepEqual(
    appended.map((part) => part.offset),
    Array.from({ length: 257 }, (_, i) => i * 4096),
  );
  assert.deepEqual(new Uint8Array(Buffer.concat(appended.map((part) => part.bytes))), source);
  assert.equal(committed, true);
  assert.equal(files.fetched, source.length);
});
