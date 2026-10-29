import assert from "node:assert/strict";
import { test } from "node:test";
import { fixture, complete } from "./support.mjs";

test("an idle first live device cannot hide another device's queued batch", async () => {
  const f = fixture([{ epoch: 11n }, { epoch: 22n, batches: 1 }]);
  await f.media.graphicsOutput();
  assert.equal(f.emitted.length, 1);
  assert.equal(f.media.graphicsStatus().pending.epoch, "22");
  complete(f);
  assert.equal(f.inventory[1].batches, 0);
});

test("retired reclamation cannot skip the next snapshotted inventory epoch", async () => {
  const f = fixture([
    { epoch: 11n },
    { epoch: 22n, retired: true },
    { epoch: 33n, retired: true, batches: 1 },
  ]);
  await f.media.graphicsOutput();
  assert.equal(f.emitted.length, 1);
  assert.deepEqual(
    f.inventory.map((row) => row.epoch),
    [11n, 33n],
  );
  assert.equal(f.media.graphicsStatus().pending.epoch, "33");
  complete(f);
});

test("a continuously busy first device shares batches with later live and retired devices", async () => {
  const f = fixture([
    { epoch: 11n, batches: 4 },
    { epoch: 22n, batches: 4 },
    { epoch: 33n, retired: true, batches: 4 },
  ]);
  const selected = [];
  for (let i = 0; i < 6; i++) {
    await f.media.graphicsOutput();
    selected.push(f.media.graphicsStatus().pending.epoch);
    complete(f);
  }
  assert.deepEqual(selected, ["11", "22", "33", "11", "22", "33"]);
  const before = f.operations.length,
    diagnostic = f.media.graphicsStatus();
  assert.equal(f.operations.length, before, "diagnostics never query the engine");
  assert.deepEqual(
    diagnostic.batchesByEpoch.map(({ epoch, acknowledged }) => [epoch, acknowledged]),
    [
      ["11", 2],
      ["22", 2],
      ["33", 2],
    ],
  );
});

test("inventory scanning is bounded even for a malformed nonterminating producer", async () => {
  const f = fixture(Array.from({ length: 65 }, (_, i) => ({ epoch: BigInt(i + 1) })));
  await assert.rejects(f.media.graphicsOutput(), /inventory.*64/i);
  assert.equal(f.emitted.length, 0);
});

test("passive inventory snapshots cannot mutate transport ownership", async () => {
  const f = fixture([{ epoch: 11n, batches: 1, retired: true }]);
  await f.media.graphicsOutput();
  const status = f.media.graphicsStatus();
  assert.deepEqual(status.inventory, [
    { epoch: "11", pointer: 111, generation: "1", retired: true, status: 0 },
  ]);
  status.pending.epoch = "99";
  status.inventory.length = 0;
  status.batchesByEpoch[0].acknowledged = 9;
  assert.equal(f.media.graphicsStatus().pending.epoch, "11");
  assert.equal(f.media.graphicsStatus().inventory.length, 1);
  assert.equal(f.media.graphicsStatus().batchesByEpoch[0].acknowledged, 0);
  complete(f);
});
