import assert from "node:assert/strict";
import test from "node:test";
import { GuestClock } from "../../runtime/dist/browser/guest-clock.js";

test("guest time excludes a long pause and resumes from the same elapsed value", () => {
  const clock = new GuestClock(100);
  assert.equal(clock.elapsed(120), 20);
  clock.stop(120);
  assert.equal(clock.elapsed(29_320), 20);
  clock.stop(29_330);
  clock.resume(29_320);
  assert.equal(clock.elapsed(29_336), 36);
});

test("guest time advances during active waits and excludes repeated inactive intervals", () => {
  const clock = new GuestClock(0);
  assert.equal(clock.elapsed(16), 16);
  assert.equal(clock.elapsed(32), 32);
  clock.stop(32);
  clock.resume(5_032);
  assert.equal(clock.elapsed(5_048), 48);
  clock.stop(5_048);
  clock.resume(15_048);
  assert.equal(clock.elapsed(15_064), 64);
});
