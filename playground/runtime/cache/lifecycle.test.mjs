import assert from "node:assert/strict";
import { test } from "node:test";
import { CodeCache } from "../cache.js";

const wasm = new Uint8Array([
  0, 97, 115, 109, 1, 0, 0, 0, 1, 5, 1, 96, 0, 1, 127, 3, 2, 1, 0, 7, 7, 1, 3, 114, 117, 110, 0, 0,
  10, 6, 1, 4, 0, 65, 0, 11,
]);
function fixture({
  installFailure,
  invalidModule = false,
  promotionStatus = 0,
  noPromotion = false,
  wrongVictim = false,
  cancelFailure,
} = {}) {
  let response,
    installed = false,
    id = 1n;
  const calls = [];
  const engine = {
    memory: new WebAssembly.Memory({ initial: 1 }),
    api: { guard_dispatch_entry() {}, find_installed_resident() {} },
    take: (length) => response.slice(0, length),
    status(operation, ...args) {
      calls.push({ operation, args });
      return promotionStatus;
    },
    ok(operation, ...args) {
      calls.push({ operation, args });
      if (operation === "prepare_cache_promotion" && promotionStatus)
        throw Object.assign(new Error("promotion rejected"), { status: promotionStatus });
      if (operation === "cancel_cache_request" && cancelFailure) throw cancelFailure;
      response = new Uint8Array(80);
      const d = new DataView(response.buffer);
      if (operation === "dispatcher_module") {
        d.setUint32(0, 0x50443352, true);
        d.setUint32(8, 32, true);
        d.setUint32(24, 1024, true);
        d.setUint32(28, wasm.length, true);
        new Uint8Array(this.memory.buffer, 1024, wasm.length).set(wasm);
      } else if (operation === "resident_module") {
        const bytes = invalidModule ? new Uint8Array([1, 2, 3]) : wasm;
        new Uint8Array(this.memory.buffer, 2048, bytes.length).set(bytes);
        d.setUint32(16, 2048, true);
        d.setUint32(20, bytes.length, true);
      } else if (operation === "prepare_cache_entry" || operation === "prepare_cache_promotion") {
        d.setUint32(0, 0x51433352, true);
        d.setUint32(8, 64, true);
        d.setUint32(12, operation === "prepare_cache_promotion" && noPromotion ? 0 : 1, true);
        d.setBigUint64(16, 7n, true);
        const promotion = operation === "prepare_cache_promotion";
        d.setUint32(24, promotion ? 2 : installed ? 0 : 1, true);
        d.setUint32(28, 0x1000, true);
        d.setBigUint64(32, id, true);
        d.setBigUint64(40, wrongVictim ? id + 99n : id, true);
        d.setUint32(56, promotion ? 1 : 0, true);
      } else if (operation === "install_cached_entry") {
        if (installFailure) throw installFailure;
        installed = true;
      } else if (operation === "acknowledge_cache_eviction") {
        assert.equal(cache.table.get(0), null);
        installed = false;
        id++;
      } else if (operation === "cache_usage") {
        d.setUint32(0, 0x55433352, true);
        d.setUint32(8, 80, true);
        d.setUint32(16, 1, true);
        d.setUint32(20, wasm.length, true);
        d.setUint32(24, 1, true);
        d.setBigUint64(32, 1n << 40n, true);
      }
    },
  };
  const cache = new CodeCache(engine, { slots: 1 });
  return { cache, engine, calls };
}

test("promotion replaces the exact victim before installing its optimized successor", () => {
  const { cache, calls } = fixture();
  const before = cache.entry(0x1000);
  const after = cache.promote();
  assert.notEqual(after.id, before.id);
  assert.equal(cache.slots.get(0), after);
  assert.equal(cache.table.get(0), after.generated.run);
  assert.equal(calls.filter((call) => call.operation === "acknowledge_cache_eviction").length, 1);
});

test("coarse engine samples remain readable after the engine is closed", () => {
  const { cache, engine, calls } = fixture();
  cache.entry(0x1000);
  cache.sampleUsage();
  const sampled = cache.stats();
  assert.equal(sampled.residentUnits, 1);
  assert.equal(sampled.residentWasmBytes, wasm.length);
  assert.equal(sampled.engineInstalledUnits, sampled.installedUnits);
  assert.equal(sampled.engineCacheHits, 2 ** 40);
  const before = calls.length;
  engine.ok = () => assert.fail("engine queried after close");
  assert.deepEqual(cache.stats(), sampled);
  assert.equal(calls.length, before);
});

test("failed installation releases the generated function and cancels its reserved request", () => {
  const failure = new Error("installation rejected");
  const { cache, calls } = fixture({ installFailure: failure });
  assert.throws(
    () => cache.entry(0x1000),
    (error) => error === failure,
  );
  assert.equal(cache.table.get(0), null);
  assert.equal(cache.slots.size, 0);
  assert.equal(calls.filter((call) => call.operation === "cancel_cache_request").length, 1);
});

test("compile failure cancels the request without publishing a function", () => {
  const { cache, calls } = fixture({ invalidModule: true });
  assert.throws(() => cache.entry(0x1000), WebAssembly.CompileError);
  assert.equal(cache.table.get(0), null);
  assert.equal(cache.slots.size, 0);
  assert.equal(calls.filter((call) => call.operation === "cancel_cache_request").length, 1);
});

test("an absent promotion leaves the installed unit untouched", () => {
  const { cache } = fixture({ noPromotion: true });
  const unit = cache.entry(0x1000);
  assert.equal(cache.promote(), null);
  assert.equal(cache.slots.get(0), unit);
  assert.equal(cache.table.get(0), unit.generated.run);
});

test("known busy maintenance is counted while unexpected errors stay visible", () => {
  const busy = fixture({ promotionStatus: 12 });
  const unit = busy.cache.entry(0x1000);
  assert.equal(busy.cache.promote(), null);
  assert.equal(busy.cache.promotionDeferrals, 1);
  assert.equal(busy.cache.slots.get(0), unit);
  const invalid = fixture({ promotionStatus: 13 });
  assert.throws(() => invalid.cache.promote(), { status: 13 });
});

test("promotion rejects a mismatched victim without clearing the table", () => {
  const { cache, calls } = fixture({ wrongVictim: true });
  const unit = cache.entry(0x1000);
  assert.throws(() => cache.promote(), /eviction identity mismatch/);
  assert.equal(cache.table.get(0), unit.generated.run);
  assert.equal(calls.filter((call) => call.operation === "acknowledge_cache_eviction").length, 0);
});

test("cleanup failure preserves the original installation error", () => {
  const failure = new Error("installation failed");
  const { cache } = fixture({
    installFailure: failure,
    cancelFailure: new Error("cleanup failed"),
  });
  assert.throws(
    () => cache.entry(0x1000),
    (error) => error === failure,
  );
  assert.equal(failure.cleanupError, "cleanup failed");
  assert.equal(cache.table.get(0), null);
});
