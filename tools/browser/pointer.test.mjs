import assert from "node:assert/strict";
import test from "node:test";
import { pointerDelta } from "../../runtime/dist/browser/pointer.js";

test("locked pointer keeps moving when client coordinates cannot move", () => {
  const sample = { clientX: 320, clientY: 240, movementX: 14, movementY: -7 };
  assert.deepEqual(pointerDelta(sample, undefined, true, 640, 480, 320, 240), [28, -14]);
});

test("unlocked pointer uses client movement and discards the first sample", () => {
  const sample = { clientX: 90, clientY: 70, movementX: 999, movementY: 999 };
  assert.equal(pointerDelta(sample, undefined, false, 640, 480, 320, 240), undefined);
  assert.deepEqual(pointerDelta(sample, { x: 80, y: 75 }, false, 640, 480, 320, 240), [20, -10]);
  assert.equal(pointerDelta(sample, { x: 80, y: 75 }, false, 640, 480, 0, 240), undefined);
});
