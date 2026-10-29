import assert from "node:assert/strict";
import { test } from "node:test";
import { fixture, moduleBytes } from "./support.mjs";

function retainedUnrelated({ cache, before }) {
  assert.equal(cache.slots.get(3), before[3]);
  assert.equal(cache.table.get(3), before[3].generated.run);
}

test("promotion acknowledges serial cohort evictions before publishing one replacement", () => {
  const f = fixture();
  const replacement = f.cache.promote();
  assert.equal(replacement.tier, 1);
  assert.equal(replacement.id, 5n);
  assert.equal(replacement.slot, 0);
  assert.equal(f.cache.table.get(0), replacement.generated.run);
  assert.equal(f.cache.slots.get(0), replacement);
  assert.equal(f.cache.table.get(1), null);
  assert.equal(f.cache.table.get(2), null);
  assert.equal(f.cache.slots.size, 2);
  assert.equal(f.cache.retainedModuleBytes, moduleBytes.length * 2);
  assert.equal(f.cache.evictions, 3);
  assert.equal(f.cache.admissions, 5);
  assert.equal(f.cache.promotions, 1);
  retainedUnrelated(f);
  assert.deepEqual(
    f.calls.map(({ operation }) => operation),
    [
      "prepare_cache_promotion",
      "acknowledge_cache_eviction",
      "prepare_cache_entry",
      "acknowledge_cache_eviction",
      "prepare_cache_entry",
      "acknowledge_cache_eviction",
      "prepare_cache_entry",
      "resident_module",
      "install_cached_entry",
    ],
  );
  assert.deepEqual(
    f.calls
      .filter(({ operation }) => operation === "acknowledge_cache_eviction")
      .map(({ args }) => [args[2], args[4], args[6]]),
    [
      [5, 1, 0],
      [6, 2, 1],
      [7, 3, 2],
    ],
  );
  assert.equal(f.pending(), undefined);
});

test("cancelled admission after one acknowledgement keeps remaining owners and exposes the cause", () => {
  const failure = Object.assign(new Error("guest cancelled"), { status: 16 });
  const f = fixture({ prepareFailure: failure });
  assert.throws(
    () => f.cache.promote(),
    (error) => error === failure,
  );
  assert.equal(f.cache.table.get(0), null);
  assert.equal(f.cache.slots.has(0), false);
  for (const slot of [1, 2, 3]) {
    assert.equal(f.cache.table.get(slot), f.before[slot].generated.run);
    assert.equal(f.cache.slots.get(slot), f.before[slot]);
  }
  assert.equal(f.cache.evictions, 1);
  assert.equal(f.cache.retainedModuleBytes, moduleBytes.length * 3);
  assert.equal(
    f.calls.some(({ operation }) => operation === "install_cached_entry"),
    false,
  );
});

test("failed replacement install clears its publication and cancels only the reserved request", () => {
  const failure = new Error("replacement installation rejected");
  const f = fixture({ installFailure: failure });
  assert.throws(
    () => f.cache.promote(),
    (error) => error === failure,
  );
  for (const slot of [0, 1, 2]) {
    assert.equal(f.cache.table.get(slot), null);
    assert.equal(f.cache.slots.has(slot), false);
  }
  retainedUnrelated(f);
  assert.equal(f.cache.retainedModuleBytes, moduleBytes.length);
  assert.equal(f.cache.promotions, 0);
  assert.equal(f.cache.admissions, 4);
  const cancellations = f.calls.filter(({ operation }) => operation === "cancel_cache_request");
  assert.equal(cancellations.length, 1);
  assert.equal(cancellations[0].args[2], 8);
  assert.equal(f.pending(), undefined);
});

test("compile failure after cohort acknowledgements leaves no replacement and preserves unrelated owners", () => {
  const f = fixture({ invalidModule: true });
  assert.throws(() => f.cache.promote(), WebAssembly.CompileError);
  assert.equal(f.cache.slots.size, 1);
  retainedUnrelated(f);
  assert.equal(f.cache.retainedModuleBytes, moduleBytes.length);
  assert.equal(f.calls.filter(({ operation }) => operation === "cancel_cache_request").length, 1);
  assert.equal(f.pending(), undefined);
});

test("cohort cleanup failure does not mask the replacement installation failure", () => {
  const failure = new Error("replacement installation rejected");
  const f = fixture({ installFailure: failure, cancelFailure: new Error("cleanup rejected") });
  assert.throws(
    () => f.cache.promote(),
    (error) => error === failure,
  );
  assert.equal(failure.cleanupError, "cleanup rejected");
  assert.equal(f.cache.slots.size, 1);
  retainedUnrelated(f);
});

test("a failed middle acknowledgement stops the cohort before clearing later owners", () => {
  const failure = new Error("eviction acknowledgement rejected");
  const f = fixture({ acknowledgementFailure: failure });
  assert.throws(
    () => f.cache.promote(),
    (error) => error === failure,
  );
  assert.equal(f.cache.table.get(0), null);
  assert.equal(f.cache.table.get(1), null);
  assert.equal(f.cache.table.get(2), f.before[2].generated.run);
  assert.equal(f.cache.slots.get(2), f.before[2]);
  retainedUnrelated(f);
  assert.equal(f.cache.evictions, 1);
  assert.equal(
    f.calls.some(({ operation }) => operation === "install_cached_entry"),
    false,
  );
});
