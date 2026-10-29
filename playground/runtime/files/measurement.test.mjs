import assert from "node:assert/strict";
import { test } from "node:test";
import { FileReadMeasurements } from "./measurement.js";

test("completed source reads keep exact offsets and bounded per-asset statistics", () => {
  const stats = new FileReadMeasurements();
  stats.record({ id: 1, path: "C:\\game\\archive.ras" }, 9007199254740997n, 4096, "file");
  stats.record({ id: 1, path: "C:\\game\\archive.ras" }, 9007199254745093n, 8, "file");
  const first = stats.snapshot();
  assert.equal(first.lastRead.offset, "9007199254745093");
  assert.deepEqual([first.sourceReads, first.sourceReadBytes], [2, 4104]);
  assert.deepEqual([first.recentAssets[0].reads, first.recentAssets[0].bytes], [2, 4104]);
  first.recentAssets[0].bytes = -1;
  first.lastRead.length = -1;
  assert.equal(stats.snapshot().lastRead.length, 8);
  assert.equal(stats.snapshot().recentAssets[0].bytes, 4104);
  for (let id = 2; id <= 17; id++) stats.record({ id, path: "x".repeat(600) }, 0n, 1, "module");
  const last = stats.snapshot();
  assert.equal(last.recentAssets.length, 16);
  assert.equal(last.evictedAssetStats, 1);
  assert.deepEqual([last.sourceReads, last.sourceReadBytes], [18, 4120]);
  assert.equal(last.recentAssets[0].path.length, 512);
  assert.equal(last.recentAssets[0].pathTruncated, true);
  assert.equal(last.recentAssets.some((entry) => entry.asset === "1"), false);
});
