import assert from "node:assert/strict";
import { test } from "node:test";
import { CodeCache } from "../cache.js";

function fixture({ status = 0, failure, receipt = {}, rewriteExit = false } = {}) {
  const calls = [],
    memory = new WebAssembly.Memory({ initial: 1 }),
    bytes = new Uint8Array(memory.buffer),
    data = new DataView(memory.buffer);
  const cache = Object.assign(Object.create(CodeCache.prototype), {
    budget: 10,
    runs: 0,
    retired: 0,
    dispatches: 0,
    directRuns: 0,
    restoredSeals: 0,
  });
  const unit = { id: 9n, slot: 2, generated: {} },
    final = { id: 17n, slot: 3, generated: {} };
  unit.generated.run = () => {
    calls.push("generated");
    if (failure) throw failure;
    data.setUint32(48, 0x2000, true);
    data.setUint32(72, 1, true);
    data.setUint32(80, 0, true);
    return status;
  };
  final.generated.run = () => {};
  cache.dispatcher = unit.generated.run;
  cache.slots = new Map([
    [2, unit],
    [3, final],
  ]);
  cache.table = { get: (slot) => cache.slots.get(slot)?.generated.run };
  cache.engine = {
    base: 0,
    memory,
    epoch: 99n,
    get: (offset) => data.getUint32(offset, true),
    take: (size) => bytes.slice(140, 140 + size),
    ok(operation) {
      calls.push(operation);
      bytes.fill(0, 140, 204);
      const output = new DataView(memory.buffer, 140, 64);
      output.setUint32(4, 0x10001, true);
      if (operation === "begin_execution_receipt") {
        output.setUint32(0, 0x46453352, true);
        output.setUint32(8, 32, true);
        output.setBigUint64(16, 7n, true);
      } else {
        output.setUint32(0, receipt.magic ?? 0x52453352, true);
        output.setUint32(8, 64, true);
        output.setBigUint64(16, receipt.epoch ?? 4n, true);
        output.setBigUint64(24, receipt.owner ?? 17n, true);
        for (const [offset, value] of [
          [32, receipt.slot ?? 3],
          [36, receipt.retired ?? 5],
          [40, receipt.child ?? 2],
          [44, 1],
          [48, receipt.kind ?? 2],
          [52, receipt.reserved ?? 0],
        ])
          output.setUint32(offset, value, true);
        if (rewriteExit) data.setUint32(72, 3, true);
      }
    },
  };
  return { cache, unit, calls };
}

test("terminal receipt identifies the final child and keeps the outer stop before seal restoration", () => {
  const { cache, unit, calls } = fixture({ rewriteExit: true });
  assert.deepEqual(cache.execute(unit), {
    pc: 0x2000,
    reason: 1,
    detail: 0,
    retired: 5,
    owner: 17n,
  });
  assert.deepEqual(calls, ["begin_execution_receipt", "generated", "finish_execution_receipt"]);
  assert.equal(cache.engine.epoch, 4n);
  assert.equal(cache.restoredSeals, 1);
  assert.equal(cache.runs, 1);
  assert.equal(cache.retired, 5);
  assert.equal(cache.dispatches, 1);
});

test("empty dispatcher receipt has no child owner or detector epoch", () => {
  const { cache, unit } = fixture({
    receipt: { kind: 0, owner: 0n, epoch: 0n, slot: 0xffffffff, retired: 0, child: 0 },
  });
  assert.equal(cache.execute(unit).owner, unit.id);
  assert.equal(cache.engine.epoch, 0n);
  assert.equal(cache.restoredSeals, 0);
});

test("terminal identity, wire and retirement failures never publish a host epoch or run count", () => {
  for (const receipt of [
    { owner: 99n },
    { slot: 99 },
    { reserved: 1 },
    { magic: 0 },
    { kind: 4 },
    { retired: 11 },
    { child: 6 },
  ]) {
    const { cache, unit } = fixture({ receipt });
    assert.throws(() => cache.execute(unit));
    assert.equal(cache.engine.epoch, 0n);
    assert.equal(cache.runs, 0);
  }
});

test("generated failures retain the original error and leave engine closure to the session", () => {
  const failure = new Error("generated trap"),
    thrown = fixture({ failure });
  assert.throws(
    () => thrown.cache.execute(thrown.unit),
    (error) => error === failure,
  );
  assert.deepEqual(thrown.calls, ["begin_execution_receipt", "generated"]);
  const rejected = fixture({ status: 3 });
  assert.throws(() => rejected.cache.execute(rejected.unit), { status: 3, owner: "9" });
  assert.deepEqual(rejected.calls, ["begin_execution_receipt", "generated"]);
  assert.equal(rejected.cache.runs, 0);
});
