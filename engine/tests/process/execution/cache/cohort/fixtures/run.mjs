import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import { Fixture, KEY, pair } from "./support.mjs";
const [artifact, output, root] = process.argv.slice(2);
const { readEngine } = await import(
  pathToFileURL(join(root, "engine/tests/support/wasm/engine.mjs"))
);
const engine = readEngine(artifact),
  cases = [];
const faultCode = [
  [0x1000, [0x40, 0xeb, 0x1d]],
  [0x1020, [0x42, 0xeb, 0x1d]],
  [0x1040, [0x8b, 3, 0xc3]],
  [0x1100, [0x90, 0xc3]],
];
function warm() {
  const e = new Fixture(engine.module, faultCode);
  const c = e.install(0x1040),
    b = e.install(0x1020),
    a = e.install(0x1000),
    unrelated = e.install(0x1100);
  return { e, a, b, c, unrelated };
}
function preserve(e, unit) {
  const material = e.material(unit.id);
  assert.equal(material.pointer, unit.material.pointer);
  assert.equal(material.sha256, unit.material.sha256);
  assert.deepEqual(e.find(unit.pc ?? 0x1100), { status: 0, id: unit.id, slot: unit.slot });
}
function fuse(context) {
  const { e, a } = context;
  let request = e.promotion();
  const victims = [];
  while (request.kind === 2) {
    assert.ok(victims.length < 8);
    victims.push(request.victim);
    e.acknowledge(request);
    request = e.request("prepare_cache_entry", ...KEY, a.pc);
  }
  assert.deepEqual(victims, [context.a.id, context.b.id, context.c.id]);
  const formed = e.install(a.pc, request);
  assert.equal(formed.tier, 1);
  for (const pc of [context.a.pc, context.b.pc, context.c.pc])
    assert.deepEqual(e.find(pc), { status: 0, id: formed.id, slot: formed.slot });
  return formed;
}

// real public ACK/cancel controls preserve unacknowledged artifacts and support retry.
{
  const { e, a, b, c, unrelated } = warm();
  const request = e.promotion();
  e.acknowledge(request);
  assert.notEqual(e.api.cancel_cache_request(...KEY, ...pair(request.serial + 1n)), 0);
  e.ok("cancel_cache_request", ...KEY, ...pair(request.serial));
  assert.notEqual(e.api.cancel_cache_request(...KEY, ...pair(request.serial)), 0);
  preserve(e, b);
  preserve(e, c);
  preserve(e, unrelated);
  assert.equal(e.find(a.pc).status, 17);
  const cold = e.install(a.pc);
  assert.equal(cold.tier, 0);
  assert.equal(e.usage().evictions, 1n);
  cases.push({ label: "partial-ACK-cancel-cold-rebuild", evictions: "1" });
  e.close();
}
// code-version change in the ACK gap abandons the plan rather than blocking all future requests.
{
  const { e, a, b, c, unrelated } = warm();
  const request = e.promotion();
  e.acknowledge(request);
  e.ok("write32", 0x1ff0, 1);
  assert.equal(e.api.prepare_cache_entry(...KEY, a.pc), 15);
  for (const unit of [b, c, unrelated]) {
    // page invalidation preserves physical bytes/table ownership but correctly rejects stale entry.
    assert.deepEqual(e.bytes(unit.material.pointer, unit.material.length), unit.material.bytes);
    assert.equal(e.table.get(unit.slot), unit.run);
    assert.equal(e.find(unit.pc).status, 4);
  }
  const freshUnrelated = e.install(unrelated.pc);
  assert.notEqual(freshUnrelated.id, unrelated.id);
  const cold = e.install(a.pc);
  assert.equal(cold.tier, 0);
  cases.push({ label: "stale-version-gap-recovers" });
  e.close();
}
// a genuine generated CPU event pins a planned successor before a destructive ACK.
{
  const { e, a, b, c, unrelated } = warm();
  const request = e.promotion();
  const before = e.usage();
  e.reset(c.pc);
  const fault = e.execute(c, 20, false);
  assert.equal(fault.reason, 23);
  assert.equal(
    e.api.acknowledge_cache_eviction(
      ...KEY,
      ...pair(request.serial),
      ...pair(request.victim),
      request.victimSlot,
    ),
    12,
  );
  assert.deepEqual(e.usage(), before);
  preserve(e, a);
  preserve(e, b);
  preserve(e, c);
  preserve(e, unrelated);
  e.ok("take_cpu_event");
  e.ok("cancel_cache_request", ...KEY, ...pair(request.serial));
  cases.push({ label: "new-fault-owner-pin-rejects-ACK" });
  e.close();
}
// fusing warm owners preserves a real memory fault's committed prefix and literal stop geometry.
{
  const base = warm(),
    formed = warm(),
    unit = fuse(formed);
  preserve(formed.e, formed.unrelated);
  for (const budget of [0, 1, 3, 4, 5, 20]) {
    base.e.reset();
    formed.e.reset();
    const a = base.e.execute(base.a, budget),
      b = formed.e.execute(unit, budget);
    assert.deepEqual(a.state, b.state);
    assert.equal(a.retired, b.retired);
    assert.equal(a.reason, b.reason);
    if (budget >= 5) {
      assert.equal(b.reason, 23);
      assert.equal(b.retired, 4);
      assert.equal(formed.e.word(16), 6);
      assert.equal(formed.e.word(24), 8);
      assert.equal(formed.e.word(48), 0x1040);
      assert.deepEqual(a.exit, b.exit);
      const exit = new DataView(b.exit.buffer);
      assert.deepEqual(
        [
          exit.getUint32(24, true),
          exit.getUint32(28, true),
          exit.getUint32(32, true),
          exit.getUint32(36, true),
        ],
        [4, 0xdead0000, 1, 4],
      );
      base.e.ok("take_cpu_event");
      formed.e.ok("take_cpu_event");
    }
  }
  base.e.reset(0x1000, 1);
  formed.e.reset(0x1000, 1);
  const baseCancelled = base.e.execute(base.a, 20),
    formedCancelled = formed.e.execute(unit, 20);
  assert.equal(baseCancelled.reason, 2);
  assert.equal(formedCancelled.reason, 2);
  assert.equal(baseCancelled.retired, 0);
  assert.equal(formedCancelled.retired, 0);
  assert.deepEqual(baseCancelled.state, formedCancelled.state);
  cases.push({
    label: "warm-fusion-fault-prefix-budget-cancel",
    budgets: [0, 1, 3, 4, 5, 20],
    faultRetired: 4,
    faultPc: 0x1040,
  });
  base.e.close();
  formed.e.close();
}
// a multi-block owner with a disconnected indexed side entry cannot be partially retired.
{
  const e = new Fixture(engine.module, [
    [0x1000, [0x90, 0xeb, 0x1d]],
    [0x1020, [0x90, 0xc3]],
    [0x1100, [0x90, 0xc3]],
  ]);
  const side = e.legacy([0x1020, 0x1100], 0),
    a = e.install(0x1000),
    old = e.material(side.id);
  const request = e.promotion();
  assert.equal(e.transfer(64).getUint32(12, true), 0);
  assert.equal(request.victim, 0n);
  preserve(e, a);
  assert.deepEqual(e.find(0x1100), { status: 0, id: side.id, slot: side.slot });
  assert.equal(e.material(side.id).pointer, old.pointer);
  assert.equal(e.material(side.id).sha256, old.sha256);
  assert.equal(e.usage().evictions, 0n);
  cases.push({ label: "disconnected-side-entry-preserved" });
  e.close();
}
// registered gate owners remain joins and retain their immutable code/slot identity.
{
  const e = new Fixture(engine.module, [
    [0x1000, [0x90, 0xeb, 0x1d]],
    [0x1020, [0x0f, 0x0b]],
  ]);
  const gate = e.legacy([0x1020], 0, [[0x1020, 17]]),
    a = e.install(0x1000),
    old = e.material(gate.id);
  const request = e.promotion();
  assert.equal(request.victim, a.id);
  e.acknowledge(request);
  e.install(a.pc);
  assert.deepEqual(e.find(0x1020), { status: 0, id: gate.id, slot: gate.slot });
  assert.equal(e.material(gate.id).sha256, old.sha256);
  cases.push({ label: "gate-owner-remains-a-join" });
  e.close();
}
writeFileSync(
  join(output, "result.json"),
  JSON.stringify({ engine_sha256: engine.sha256, cases }, null, 2),
);
console.log(JSON.stringify({ engine_sha256: engine.sha256, cases: cases.length, output }));
