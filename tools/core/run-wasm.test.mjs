import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { runWasmSmoke } from "./run-wasm.mjs";

function section(id, bytes) {
  assert.ok(bytes.length < 128);
  return [id, bytes.length, ...bytes];
}

const sentinel = [0x41, 0x81, 0x80, 0xcc, 0x91, 0x05];
function moduleBytes(body, { exports = [1, 3, 114, 117, 110, 0, 0], start = false } = {}) {
  return Buffer.from([
    0, 97, 115, 109, 1, 0, 0, 0,
    ...section(1, [1, 0x60, 0, ...(start ? [0] : [1, 0x7f])]),
    ...section(3, [1, 0]),
    ...section(6, [1, 0x7f, 1, 0x41, 0, 0x0b]),
    ...section(7, exports),
    ...(start ? section(8, [0]) : []),
    ...section(10, [1, body.length + 2, 0, ...body, 0x0b]),
  ]);
}

const increment = [0x23, 0, 0x41, 1, 0x6a, 0x24, 0];
const loop = [0x03, 0x40, 0x0c, 0, 0x0b];
const fixtures = {
  healthy: moduleBytes([...increment, 0x23, 0, 0x41, 2, 0x4b, 0x04, 0x40, 0, 0x0b, ...sentinel]),
  wrong: moduleBytes([0x41, 0]),
  second: moduleBytes([...increment, 0x23, 0, 0x41, 1, 0x46, 0x04, 0x7f, ...sentinel, 0x05, 0x41, 0, 0x0b]),
  trap: moduleBytes([0]),
  missing: moduleBytes(sentinel, { exports: [0] }),
  nonfunction: moduleBytes(sentinel, { exports: [1, 3, 114, 117, 110, 3, 0] }),
  imported: Buffer.from([0, 97, 115, 109, 1, 0, 0, 0,
    ...section(1, [1, 0x60, 0, 0]), ...section(2, [1, 1, 104, 1, 102, 0, 0])]),
  invalid: Buffer.from([0]),
  loop: moduleBytes([...loop, ...sentinel]),
  startLoop: moduleBytes(loop, { start: true, exports: [0] }),
};

function withFixture(t, bytes) {
  const directory = mkdtempSync(join(tmpdir(), "ring3-wasm-"));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  const file = join(directory, "smoke.wasm");
  writeFileSync(file, bytes);
  return file;
}

test("smoke completes exactly two calls on one instance", (t) => {
  const bytes = fixtures.healthy;
  assert.deepEqual(runWasmSmoke(withFixture(t, bytes)), {
    node: process.versions.node, imports: 0, calls: 2, sentinel: "0x52330001",
    wasmSha256: createHash("sha256").update(bytes).digest("hex"),
  });
});

for (const [name, message] of [
  ["wrong", /all core smoke assertions must finish/],
  ["second", /all core smoke assertions must finish/],
  ["trap", /unreachable/],
  ["missing", /run export must be a function/],
  ["nonfunction", /run export must be a function/],
  ["imported", /smoke must have no host imports/],
  ["invalid", /CompileError/],
]) {
  test(`smoke rejects ${name}`, (t) => {
    assert.throws(() => runWasmSmoke(withFixture(t, fixtures[name])), (error) => {
      assert.match(error.message, message);
      assert.equal(error.cause.stdout, "");
      return true;
    });
  });
}

test("smoke rejects invalid deadlines before starting a child", () => {
  for (const timeout of [0, -1, NaN, Infinity, 1.5]) {
    assert.throws(() => runWasmSmoke("unused", timeout), /positive safe integer/);
  }
});

for (const name of ["loop", "startLoop"]) {
  test(`smoke terminates and reaps ${name}`, (t) => {
    assert.throws(() => runWasmSmoke(withFixture(t, fixtures[name]), 500), (error) => {
      assert.match(error.message, /timed out after 500 ms/);
      assert.equal(error.cause.error.code, "ETIMEDOUT");
      assert.equal(error.cause.signal, "SIGKILL");
      assert.equal(error.cause.status, null);
      assert.equal(error.cause.stdout, "");
      assert.ok(error.cause.pid > 0);
      assert.throws(() => process.kill(error.cause.pid, 0), { code: "ESRCH" });
      return true;
    });
  });
}

test("smoke refuses a missing module", (t) => {
  const file = withFixture(t, fixtures.healthy);
  rmSync(file);
  assert.throws(() => runWasmSmoke(file), /ENOENT/);
});
