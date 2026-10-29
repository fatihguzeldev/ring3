import assert from "node:assert/strict";
import { Media } from "../../media.js";

export function fixture(rows, measurements = false) {
  const inventory = rows.map((row) => ({
    epoch: BigInt(row.epoch),
    retired: false,
    sequence: 1n,
    batches: 0,
    ...row,
  }));
  const emitted = [],
    operations = [],
    controller = new AbortController();
  let fail, read;
  const engine = {
    graphicsMeasurementsEnabled: measurements,
    signal: controller.signal,
    media(operation, ...args) {
      operations.push(operation);
      if (operation === "d3d8_device") {
        const row = inventory[args[0]];
        return row
          ? { epoch: row.epoch, values: [100n + row.epoch, 1n, BigInt(row.retired), 0n] }
          : { epoch: 0n, values: [0n, 0n] };
      }
      const epoch = BigInt(args[0]) | (BigInt(args[1]) << 32n);
      const row = inventory.find((item) => item.epoch === epoch);
      assert.ok(row, "inventory epoch remains addressable");
      if (operation === "d3d8_seal_batch" && row.retired && !row.batches)
        inventory.splice(inventory.indexOf(row), 1);
      return { epoch, values: row.batches ? [row.sequence, 64n] : [0n, 0n] };
    },
    request() {},
    take(length) {
      return new Uint8Array(length);
    },
    ok(operation, ...args) {
      operations.push(operation);
      if (operation === "d3d8_read_batch") read?.();
      if (fail?.operation === operation) throw fail.error;
      if (operation === "d3d8_acknowledge_batch") {
        const row = inventory.find((item) => item.epoch === BigInt(args[0]));
        assert.equal(row.sequence, BigInt(args[2]));
        row.batches--;
        row.sequence++;
      }
    },
  };
  const media = new Media(engine, 7n, (message) => emitted.push(message));
  return {
    media,
    engine,
    emitted,
    operations,
    inventory,
    abort: () => controller.abort(),
    onRead: (callback) => {
      read = callback;
    },
    fail: (operation, error) => {
      fail = { operation, error };
    },
  };
}

export function complete(f, extra = {}) {
  const pending = f.media.graphicsStatus().pending;
  f.media.graphicsComplete({
    requestId: pending.requestId,
    epoch: pending.epoch,
    sequence: pending.sequence,
    ...extra,
  });
}
