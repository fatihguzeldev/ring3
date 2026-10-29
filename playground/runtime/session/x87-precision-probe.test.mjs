import test from "node:test";
import assert from "node:assert/strict";
import { X87PrecisionProbe } from "./x87-precision-probe.js";
import { FrameProbe } from "./frame-probe.js";
import { KEY } from "../engine.js";

function packet(generation = 1n) {
  const bytes = new Uint8Array(256), d = new DataView(bytes.buffer);
  bytes.set(new TextEncoder().encode("R3XP"));
  d.setUint32(4, 0x10001, true); d.setUint32(8, 256, true);
  d.setBigUint64(16, generation, true);
  const values = [10, 2, 1, 1, 1, 1, 1, 1, 2, 6, 0, 1, 1, 1, 0, 0, 1, 2,
    10, 0, 3, 3, 10, 2, 2, 2];
  values.forEach((value, i) => d.setBigUint64(32 + i * 8, BigInt(value), true));
  return bytes;
}

function fixture(options = {}) {
  const scratch = new Uint8Array(4096).fill(0xa5), original = scratch.slice();
  const calls = [], events = [];
  let collecting = false, tick = 1000n;
  const engine = {
    hash: "fixture-engine", get: () => 0x1234,
    api: Object.fromEntries(["begin_x87_precision_probe", "stop_x87_precision_probe", "x87_precision_probe"]
      .map(name => [name, () => {}])),
    take: length => scratch.slice(0, length),
    request: bytes => scratch.set(bytes),
    ok(name, ...args) {
      calls.push(name); assert.deepEqual(args, KEY);
      if (options.fail === name) { scratch.fill(0xcc, 0, 256); throw new Error(`${name} returned7`); }
      if (name === "begin_x87_precision_probe") { assert.equal(collecting, false); collecting = true; }
      else if (name === "stop_x87_precision_probe") { assert.equal(collecting, true); collecting = false; }
      else { assert.equal(collecting, false); scratch.set(options.packet ?? packet()); }
    },
  };
  const session = { engine, emit: message => events.push(message),
    code: { receiptMode: "checked", sampleUsage() {}, stats: () => ({}) }, thread: 1 };
  return { session, engine, calls, events, scratch, original, now: () => tick++ };
}

test("cold begin and stopped fixed packet preserve transfer and retain clock brackets", () => {
  const f = fixture(), p = new X87PrecisionProbe(f.session, f.now);
  const begin = p.begin();
  assert.equal(begin.started, true); assert.deepEqual(f.scratch, f.original);
  assert.ok(BigInt(begin.begin_after_ns) >= BigInt(begin.begin_before_ns));
  const result = p.stopAtNinth();
  assert.equal(result.generation, "1"); assert.equal(result.packet_hex.length, 512);
  assert.equal(result.add_sub.attempts, "10"); assert.equal(result.multiply.accepted, "2");
  assert.deepEqual(result.pc_histogram, ["10", "0", "3", "3"]);
  assert.ok(BigInt(result.read_before_ns) >= BigInt(result.stop_after_ns));
  assert.deepEqual(f.scratch, f.original);
  assert.deepEqual(f.calls, ["begin_x87_precision_probe", "stop_x87_precision_probe", "x87_precision_probe"]);
  const summary = p.finish("complete");
  assert.equal(summary.complete, true); assert.equal(summary.partial, false);
  assert.equal(summary.packet_location, "producer-present-9");
  assert.equal("packet_hex" in summary, false);
  p.finish("complete"); assert.equal(f.calls.length, 3);
});

test("partial finish stops collection, retains raw packet and never claims full coverage", () => {
  const f = fixture(), p = new X87PrecisionProbe(f.session, f.now);
  p.begin();
  const result = p.finish("session-ended");
  assert.equal(result.complete, false); assert.equal(result.partial, true);
  assert.equal(result.packet_hex.length, 512); assert.equal(result.proof_reason, "session-ended");
  assert.deepEqual(f.scratch, f.original); assert.equal(f.calls.length, 3);
});

test("unavailable diagnostic fails only its capture and does not call stop after failed begin", () => {
  const f = fixture({ fail: "begin_x87_precision_probe" }), p = new X87PrecisionProbe(f.session, f.now);
  assert.equal(p.begin().started, false);
  const result = p.finish("complete");
  assert.equal(result.complete, false); assert.match(result.error, /returned7/);
  assert.deepEqual(f.calls, ["begin_x87_precision_probe"]); assert.deepEqual(f.scratch, f.original);
});

test("malformed read retains raw bytes, restores scratch and marks capture incomplete", () => {
  const bytes = packet(); bytes[0] = 0;
  const f = fixture({ packet: bytes }), p = new X87PrecisionProbe(f.session, f.now);
  p.begin(); const result = p.stopAtNinth();
  assert.equal(result.packet_hex.length, 512); assert.match(result.error, /header/);
  assert.equal(p.finish("complete").complete, false); assert.deepEqual(f.scratch, f.original);
});

test("partition, histogram, reserved and impossible multiply-gap errors reject", () => {
  for (const [at, value] of [[32, 11n], [176, 11n], [144, 1n], [240, 1n]]) {
    const bytes = packet(); new DataView(bytes.buffer).setBigUint64(at, value, true);
    const f = fixture({ packet: bytes }), p = new X87PrecisionProbe(f.session, f.now);
    p.begin(); const result = p.stopAtNinth();
    assert.equal(typeof result.error, "string", `offset${at}`);
    assert.equal(p.finish("complete").complete, false); assert.deepEqual(f.scratch, f.original);
  }
});

test("generation cannot repeat across captures in one session", () => {
  const f = fixture(), first = new X87PrecisionProbe(f.session, f.now);
  first.begin(); first.stopAtNinth(); assert.equal(first.finish("complete").complete, true);
  const second = new X87PrecisionProbe(f.session, f.now);
  second.begin(); assert.match(second.stopAtNinth().error, /generation/);
  assert.equal(second.finish("complete").complete, false);
});

test("saturation remains visible and cannot claim exact partition coverage", () => {
  const bytes = packet(), d = new DataView(bytes.buffer);
  d.setUint32(24, 1, true); d.setBigUint64(32, 0xffffffffffffffffn, true);
  const f = fixture({ packet: bytes }), p = new X87PrecisionProbe(f.session, f.now);
  p.begin(); const result = p.stopAtNinth();
  assert.equal(result.flags, 1); assert.equal(result.add_sub.attempts, "18446744073709551615");
  assert.equal(p.finish("complete").complete, false);
});

test("read failure restores transfer and does not restart or stop a second time", () => {
  const f = fixture({ fail: "x87_precision_probe" }), p = new X87PrecisionProbe(f.session, f.now);
  p.begin(); assert.match(p.stopAtNinth().error, /returned7/);
  assert.equal(p.finish("complete").complete, false);
  assert.deepEqual(f.scratch, f.original); assert.equal(f.calls.length, 3);
});

test("FrameProbe hooks only successful first/ninth edges and keeps finished terminal", () => {
  const f = fixture(); f.session.frameProbeMathEnabled = true;
  const probe = new FrameProbe("math-fixture", f.session); f.session.frameProbe = probe;
  const receipt = { id: 0x4020f, callArgs: [1], pc: 0x1000 };
  probe.present(receipt, 12, 0, 1n, 2n, 3n); assert.equal(f.calls.length, 0);
  probe.present(receipt, 0, 1, 1n, 2n, 3n); assert.equal(f.calls.length, 0);
  for (let i = 0; i < 9; i++) probe.present(receipt, 0, 0, 1n, 2n, 3n);
  assert.deepEqual(f.calls, ["begin_x87_precision_probe", "stop_x87_precision_probe", "x87_precision_probe"]);
  const edges = f.events.filter(e => e.detail.event === "producer-present" && e.detail.accepted);
  assert.equal(edges[0].detail.x87_precision_probe.event, "begin");
  assert.equal(edges[8].detail.x87_precision_probe.packet_hex.length, 512);
  probe.finish("session-ended");
  assert.equal(f.events.at(-1).detail.event, "finished");
  assert.equal(f.events.at(-1).detail.x87_precision_probe.complete, false);
});

test("flag-off FrameProbe never requires native math exports or changes event fields", () => {
  const f = fixture(); f.engine.api = {};
  const probe = new FrameProbe("off-fixture", f.session); f.session.frameProbe = probe;
  for (let i = 0; i < 9; i++) probe.present({ id: 0x4020f, callArgs: [1], pc: 0x1000 }, 0, 0, 1n, 2n, 3n);
  probe.finish("complete");
  assert.equal(f.calls.length, 0);
  for (const event of f.events) {
    assert.equal("x87_precision_probe" in event.detail, false);
    assert.equal("x87_precision_probe_enabled" in event.detail, false);
  }
});
