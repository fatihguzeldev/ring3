import assert from "node:assert/strict";
import { test } from "node:test";
import { PhaseMeasurements } from "./measurement.js";

function fixture(t, detailed = false) {
  let now = 0;
  t.mock.method(performance, "now", () => now);
  const counters = new Map(),
    scopes = [];
  const measurements = new PhaseMeasurements({
    observer: {
      event() {},
      counter(name, value) {
        counters.set(name, value);
      },
    },
    capture: {
      observer: () => ({
        enabled: detailed,
        scope(name, args, fn) {
          scopes.push(name);
          return fn();
        },
        scopeAsync(name, args, fn) {
          scopes.push(name);
          return fn();
        },
      }),
    },
  });
  return {
    measurements,
    counters,
    scopes,
    advance(milliseconds) {
      now += milliseconds;
    },
  };
}

test("basic phase accounting is cumulative and uses elapsed nanoseconds", (t) => {
  const f = fixture(t);
  assert.equal(
    f.measurements.run("execution", () => {
      f.advance(2.5);
      return 9;
    }),
    9,
  );
  f.measurements.run("execution", () => f.advance(1));
  f.measurements.report();
  assert.equal(f.counters.get("execution.ns"), 3500000);
  assert.equal(f.counters.get("execution.count"), 2);
  assert.equal(f.counters.get("execution.max.ns"), 2500000);
  assert.deepEqual(f.measurements.snapshot().execution, {
    elapsedNs: 3500000,
    maximumNs: 2500000,
    count: 2,
  });
  assert.deepEqual(f.scopes, []);
});

test("async waits and failing operations retain their results and errors", async (t) => {
  const f = fixture(t, true);
  assert.equal(
    await f.measurements.runAsync("wait", async () => {
      f.advance(4);
      return "ready";
    }),
    "ready",
  );
  const failure = new Error("guest failure");
  await assert.rejects(
    f.measurements.runAsync("providers", async () => {
      f.advance(3);
      throw failure;
    }),
    (error) => error === failure,
  );
  f.measurements.report();
  assert.equal(f.counters.get("wait.ns"), 4000000);
  assert.equal(f.counters.get("providers.ns"), 3000000);
  assert.equal(f.counters.get("providers.count"), 1);
  assert.deepEqual(f.scopes, ["wait", "providers"]);
});
