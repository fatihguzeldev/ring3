import assert from "node:assert/strict";
import { test } from "node:test";
import { FrameMeasurements } from "./frames.js";

test("present intervals retain stalls, bounded history and an explicit last-frame age", () => {
  const counters = new Map();
  const frames = new FrameMeasurements({ counter: (key, value) => counters.set(key, value) });
  assert.equal(frames.snapshot(0).intervalFps, null);
  frames.present(10);
  frames.present(30);
  frames.present(230);
  const status = frames.snapshot(300);
  assert.equal(status.frames, 3);
  assert.equal(status.meanIntervalMs, 110);
  assert.equal(status.p95IntervalMs, 200);
  assert.equal(status.sinceLastPresentMs, 70);
  assert.equal(counters.get("present.count"), 3);
  assert.equal(counters.get("present.p95-interval.ns"), 200000000);
  for (let i = 0; i < 400; i++) frames.present(400 + i * 10);
  assert.equal(frames.snapshot(4500).samples, 240);
  assert.equal(frames.snapshot(4500).meanIntervalMs, 10);
});
