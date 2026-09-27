import assert from "node:assert/strict";
import test from "node:test";
import { KeyboardState, scanCode } from "../../runtime/dist/browser/keyboard.js";

test("physical game controls map to directinput scan codes", () => {
  for (const [code, expected] of [
    ["KeyW", 0x11],
    ["KeyA", 0x1e],
    ["KeyS", 0x1f],
    ["KeyD", 0x20],
    ["KeyE", 0x12],
    ["Digit1", 0x02],
    ["Space", 0x39],
    ["ArrowUp", 0xc8],
    ["ControlRight", 0x9d],
  ])
    assert.equal(scanCode(code), expected, code);
  assert.equal(scanCode("Unidentified"), undefined);
});

test("keyboard snapshots keep chords and release on focus loss", () => {
  const keyboard = new KeyboardState();
  assert.deepEqual(keyboard.set("KeyW", true), [0x11]);
  assert.equal(keyboard.set("KeyW", true), undefined);
  assert.deepEqual(keyboard.set("KeyE", true), [0x11, 0x12]);
  assert.deepEqual(keyboard.set("KeyW", false), [0x12]);
  assert.deepEqual(keyboard.clear(), []);
  assert.equal(keyboard.clear(), undefined);
  assert.equal(keyboard.set("Unidentified", true), undefined);
});
