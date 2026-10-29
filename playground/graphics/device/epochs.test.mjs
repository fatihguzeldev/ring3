import assert from "node:assert/strict";
import { test } from "node:test";
import { ClosedEpochs } from "./epochs.js";

test("dense retired epochs merge without rejecting a previously unseen older epoch", () => {
  const epochs = new ClosedEpochs(2);
  for (let epoch = 2n; epoch <= 100n; epoch++) epochs.add(epoch);
  assert.deepEqual(epochs.intervals, [[2n, 100n]]);
  assert.equal(epochs.has(1n), false);
  epochs.add(1n);
  assert.deepEqual(epochs.intervals, [[1n, 100n]]);
  assert.equal(epochs.has(101n), false);
});

test("out-of-order sparse intervals merge only when the missing identity is actually retired", () => {
  const epochs = new ClosedEpochs(2);
  epochs.add(8n);
  epochs.add(2n);
  assert.equal(epochs.has(4n), false);
  assert.throws(() => epochs.add(4n), /interval budget/);
  assert.deepEqual(epochs.intervals, [
    [2n, 2n],
    [8n, 8n],
  ]);
  for (let epoch = 3n; epoch <= 7n; epoch++) epochs.add(epoch);
  assert.deepEqual(epochs.intervals, [[2n, 8n]]);
});
