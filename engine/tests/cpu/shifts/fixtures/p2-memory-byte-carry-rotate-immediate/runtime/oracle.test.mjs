import assert from "node:assert/strict";
import test from "node:test";
import { ANCHORS, rotate } from "./oracle.mjs";

test("literal ring and flag anchors distinguish masked count from distance", () => {
  for (const [kind, value, raw, incoming, expected, flags] of ANCHORS) {
    const actual = rotate(kind, value, raw, incoming);
    assert.equal(actual.value, expected);
    assert.equal(actual.flags, flags);
  }
});

test("effective zero preserves all incoming flag bits", () => {
  for (const kind of ["left", "right"])
    for (const raw of [0, 9, 18, 27, 32, 41, 255 - 4]) {
      const actual = rotate(kind, 0x81, raw, 0xa5a50cd7);
      assert.equal(actual.value, 0x81);
      assert.equal(actual.flags, 0xa5a50cd7);
    }
});

test("multi-count policy clears only CF and OF before publishing ring carry", () => {
  const result = rotate("left", 0x81, 10, 0xa5a50cd6);
  assert.equal(result.value, 2);
  assert.equal(result.flags, 0xa5a504d7);
  assert.equal(rotate("left", 0x81, 1, 0xa5a50cd6).flags, 0xa5a50cd7);
});
