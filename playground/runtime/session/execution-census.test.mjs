import assert from "node:assert/strict";
import { test } from "node:test";
import { ExecutionCensus } from "./execution-census.js";

const KEY = [0x50334731, 0x62726f77];
const proofId = "authored-census-proof";
const goodStatus = () => ({ state: "recording", droppedEvents: 0, droppedChunks: 0 });

function page(offset, { rows = 2, capacity = 32768, generation = 7n } = {}) {
  const raw = new Uint8Array(4096), d = new DataView(raw.buffer);
  raw.set(new TextEncoder().encode("R3CE"));
  const u32 = (at, value) => d.setUint32(at, value, true);
  const u64 = (at, value) => d.setBigUint64(at, BigInt(value), true);
  d.setUint16(4, 1, true); d.setUint16(6, 1, true); u32(8, 4096);
  u64(16, generation); u32(24, 2); u32(28, capacity); u32(32, rows);
  const count = Math.min(49, rows - offset);
  u32(36, offset); u32(40, count); u32(44, 80);
  u64(48, rows); u64(56, rows); u64(64, 2 * rows); u64(72, 3 * rows);
  for (let i = 0; i < count; i++) {
    const p = 128 + i * 80, owner = offset + i + 1;
    u64(p, owner); u32(p + 8, 0x1000 + 4 * owner); u32(p + 12, 0x2000);
    u32(p + 16, 3); u32(p + 20, 1); u64(p + 24, 1);
    u64(p + 32, 2); u64(p + 40, 3); u32(p + 48, 2); u32(p + 52, 2);
    u32(p + 56, 1); u32(p + 60, 2); u32(p + 64, 400); u32(p + 72, 1);
  }
  return raw;
}

function fixture(options = {}) {
  const calls = [], events = [], diagnostics = [], categories = [], offsets = [];
  const scratch = Uint8Array.from({ length: 4096 }, (_, i) => (i * 17 + 3) & 255);
  const initial = scratch.slice();
  const engine = {
    api: {},
    status(name, ...args) { return this.api[name](...args); },
    ok(name, ...args) {
      const status = this.status(name, ...args);
      if (status !== 0) throw new Error(`${name} returned ${status}`);
    },
    take(length) { return scratch.slice(0, length); },
    request(bytes) { scratch.set(bytes); },
  };
  for (const name of ["begin_execution_census", "stop_execution_census", "execution_census_page"])
    engine.api[name] = (...args) => {
      calls.push({ name, args });
      assert.deepEqual(args.slice(0, 2), KEY, "canonical native key");
      if (name === "execution_census_page") {
        const offset = args[2]; offsets.push(offset);
        scratch.set(options.page?.(offset) ?? page(offset, options));
      }
      return options.nativeStatus?.(name) ?? 0;
    };
  let flushes = 0;
  const session = {
    engine,
    observation: {
      capture: {
        observer(category) {
          categories.push(category);
          return { enabled: true, event: (name, detail) => events.push({ name, detail }) };
        },
      },
      async flush() {
        flushes++;
        return options.flush ? await options.flush(flushes, { scratch, events }) : goodStatus();
      },
    },
    emit(message) { diagnostics.push(message); },
  };
  return { session, calls, events, diagnostics, categories, offsets, scratch, initial,
    get flushes() { return flushes; } };
}

function ready(f, summary) {
  const messages = f.diagnostics.filter(m => m.type === "diagnostic" && m.message === "execution-census-ready");
  assert.equal(messages.length, 1, "one final small diagnostic");
  assert.deepEqual(messages[0].detail, summary);
  assert.equal(summary.proof_id, proofId);
  assert.ok(!("packet_hex" in summary), "bulk packets belong only to central trace events");
}

test("begin/stop are single native transitions and finish retains central packet once", async () => {
  const f = fixture(), census = new ExecutionCensus(f.session, proofId);
  census.begin(); census.begin(); census.stop(); census.stop();
  const [a, b] = await Promise.all([census.finish("complete"), census.finish("complete")]);
  assert.deepEqual(a, b); assert.equal(a.complete, true); assert.equal(a.pages, 1);
  assert.deepEqual(await census.finish("complete"), a);
  assert.equal(f.calls.filter(c => c.name === "begin_execution_census").length, 1);
  assert.deepEqual(f.calls.find(c => c.name === "begin_execution_census").args, [...KEY, 32768]);
  assert.equal(f.calls.filter(c => c.name === "stop_execution_census").length, 1);
  assert.deepEqual(f.offsets, [0]); assert.equal(f.flushes, 1);
  assert.deepEqual(f.scratch, f.initial);
  assert.deepEqual(f.categories, ["engine.cpu.cache"]);
  assert.equal(f.events.length, 1); assert.equal(f.events[0].name, "execution.census.page");
  assert.equal(f.events[0].detail.proof_id, proofId);
  assert.equal(f.events[0].detail.packet_hex, Buffer.from(page(0)).toString("hex"));
  ready(f, a);
});

test("each page restores scratch before awaiting flush and preserves intervening guest writes", async () => {
  let release, notify;
  const pending = new Promise(resolve => { release = resolve; });
  const entered = new Promise(resolve => { notify = resolve; });
  const guest = new Uint8Array(4096).fill(0xa7);
  const f = fixture({ rows: 53, capacity: 64, async flush(index, state) {
    if (index === 1) {
      assert.deepEqual(state.scratch, f.initial, "first transfer loan already restored");
      notify(); await pending; state.scratch.set(guest);
    } else assert.deepEqual(state.scratch, guest, "second transfer loan restores newer guest scratch");
    return goodStatus();
  } });
  const census = new ExecutionCensus(f.session, proofId, { capacity: 64 }); census.begin();
  const finishing = census.finish("complete"); await entered;
  assert.deepEqual(f.offsets, [0]); assert.equal(f.diagnostics.length, 0);
  release(); const summary = await finishing;
  assert.equal(summary.complete, true); assert.equal(summary.pages, 2);
  assert.deepEqual(f.offsets, [0, 49]); assert.equal(f.flushes, 2);
  assert.deepEqual(f.scratch, guest, "no stale whole-drain scratch restore after an await");
  assert.deepEqual(f.events.map(e => e.detail.packet_hex), [0, 49].map(offset =>
    Buffer.from(page(offset, { rows: 53, capacity: 64 })).toString("hex")));
  ready(f, summary);
});

test("empty frozen census still emits its header page", async () => {
  const f = fixture({ rows: 0 }), census = new ExecutionCensus(f.session, proofId); census.begin();
  const summary = await census.finish("complete");
  assert.equal(summary.complete, true); assert.equal(summary.pages, 1);
  assert.deepEqual(f.offsets, [0]); assert.deepEqual(f.scratch, f.initial); ready(f, summary);
});

test("early finish stops collection and reports an incomplete capture", async () => {
  const f = fixture(), census = new ExecutionCensus(f.session, proofId); census.begin();
  const summary = await census.finish("cancelled");
  assert.equal(summary.complete, false);
  assert.equal(f.calls.filter(c => c.name === "stop_execution_census").length, 1);
  assert.deepEqual(f.scratch, f.initial); ready(f, summary);
});

test("malformed fixed headers cannot produce a complete census", async t => {
  const cases = [
    ["magic", d => d.setUint32(0, 0, true)],
    ["version", d => d.setUint16(4, 2, true)],
    ["profile", d => d.setUint16(6, 2, true)],
    ["size", d => d.setUint32(8, 4080, true)],
    ["reserved", d => d.setUint32(12, 1, true)],
    ["generation", d => d.setBigUint64(16, 0n, true)],
    ["collecting state", d => d.setUint32(24, 1, true)],
    ["capacity", d => d.setUint32(28, 32769, true)],
    ["offset", d => d.setUint32(36, 1, true)],
    ["count", d => d.setUint32(40, 50, true)],
    ["row size", d => d.setUint32(44, 79, true)],
    ["reserved tail", d => d.setUint32(124, 1, true)],
  ];
  for (const [name, change] of cases) await t.test(name, async () => {
    const f = fixture({ page(offset) { const raw = page(offset); change(new DataView(raw.buffer)); return raw; } });
    const census = new ExecutionCensus(f.session, proofId); census.begin();
    const summary = await census.finish("complete");
    assert.equal(summary.complete, false); assert.equal(typeof summary.error, "string");
    assert.deepEqual(f.scratch, f.initial); ready(f, summary);
  });
});

test("pagination rejects changed frozen generation or aggregate header", async t => {
  for (const [name, change] of [
    ["generation", d => d.setBigUint64(16, 8n, true)],
    ["retired total", d => d.setBigUint64(64, 107n, true)],
  ]) await t.test(name, async () => {
    const f = fixture({ rows: 53, capacity: 64, page(offset) {
      const raw = page(offset, { rows: 53, capacity: 64 });
      if (offset) change(new DataView(raw.buffer)); return raw;
    } });
    const census = new ExecutionCensus(f.session, proofId, { capacity: 64 }); census.begin();
    const summary = await census.finish("complete");
    assert.equal(summary.complete, false); assert.equal(typeof summary.error, "string");
    assert.deepEqual(f.offsets, [0, 49]); assert.deepEqual(f.scratch, f.initial); ready(f, summary);
  });
});

test("loss, unresolved entries, flags or broken conservation stay incomplete", async t => {
  const cases = [
    ["dropped row", d => { d.setBigUint64(48, 3n, true); d.setBigUint64(56, 3n, true); d.setBigUint64(80, 1n, true); }],
    ["unresolved entry", d => { d.setBigUint64(48, 3n, true); d.setBigUint64(104, 1n, true); d.setUint32(120, 4, true); }],
    ["flags", d => d.setUint32(120, 1, true)],
    ["count conservation", d => d.setBigUint64(56, 3n, true)],
    ["retirement conservation", d => d.setBigUint64(64, 5n, true)],
    ["work conservation", d => d.setBigUint64(72, 7n, true)],
  ];
  for (const [name, change] of cases) await t.test(name, async () => {
    const f = fixture({ page(offset) { const raw = page(offset); change(new DataView(raw.buffer)); return raw; } });
    const census = new ExecutionCensus(f.session, proofId); census.begin();
    const summary = await census.finish("complete");
    assert.equal(summary.complete, false); assert.deepEqual(f.scratch, f.initial); ready(f, summary);
  });
});

test("flush drops, recording/sink errors and rejection cannot report retained completion", async t => {
  const cases = [
    ["event drops", () => ({ ...goodStatus(), droppedEvents: 1 })],
    ["chunk drops", () => ({ ...goodStatus(), droppedChunks: 1 })],
    ["recording error", () => ({ ...goodStatus(), recordingError: "trace incomplete" })],
    ["sink error", () => ({ ...goodStatus(), sinkError: "disk refused" })],
    ["rejected flush", () => { throw new Error("transport refused"); }],
  ];
  for (const [name, flush] of cases) await t.test(name, async () => {
    const f = fixture({ flush }), census = new ExecutionCensus(f.session, proofId); census.begin();
    const summary = await census.finish("complete");
    assert.equal(summary.complete, false); assert.equal(typeof summary.error, "string");
    assert.deepEqual(f.scratch, f.initial); ready(f, summary);
  });
});
