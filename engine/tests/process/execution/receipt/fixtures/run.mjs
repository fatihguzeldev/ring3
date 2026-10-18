import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

const [enginePath, output, root] = process.argv.slice(2);
const { readEngine } = await import(
  pathToFileURL(join(root, "engine/tests/support/wasm/engine.mjs"))
);
const engine = readEngine(enginePath),
  key = [0x81342715, 0x62594837],
  cases = [];
const pair = (value) => [Number(value & 0xffffffffn), Number(value >> 32n)];
function fresh() {
  const instance = new WebAssembly.Instance(engine.module, {
    ring3_observability: { now_ns: () => BigInt(Date.now()) * 1000000n },
  });
  const api = Object.fromEntries(
    Object.entries(instance.exports)
      .filter(([name]) => name.startsWith("ring3_abi_v1_"))
      .map(([name, value]) => [name.slice(13), value]),
  );
  const e = {
    api,
    memory: instance.exports.memory,
    table: new WebAssembly.Table({ element: "anyfunc", initial: 16, maximum: 16 }),
    installed: [],
  };
  ok(e, "open", 16, ...key);
  e.base = api.arena_ptr() >>> 0;
  ok(e, "map", 0x1000, 3, 7);
  ok(e, "map", 0x8000, 1, 3);
  ok(e, "configure_execution", 16, 1024 * 1024, 8);
  for (const [pc, source] of [
    [0x1000, "40e9fa0f0000"],
    [0x2000, "490f85f9efffff"],
    [0x2200, "0f0b"],
    [0x2210, "8b03"],
    [0x2300, "f3a4"],
    [0x2400, "0f0b"],
    [0x2500, "b807000000c3"],
    [0x2600, "0f0b"],
  ]) {
    request(e, Buffer.from(source, "hex"));
    ok(e, "upload", pc, source.length / 2);
  }
  ok(e, "dispatcher_module", ...key);
  const descriptor = transfer(e, 32),
    pointer = descriptor.getUint32(24, true),
    size = descriptor.getUint32(28, true);
  e.dispatch = new WebAssembly.Instance(
    new WebAssembly.Module(new Uint8Array(e.memory.buffer).slice(pointer, pointer + size)),
    {
      env: { memory: e.memory, table: e.table },
      ring3: {
        guard_dispatch_entry: api.guard_dispatch_entry,
        find_installed_resident: api.find_installed_resident,
      },
    },
  ).exports.run;
  return e;
}
function ok(e, name, ...args) {
  assert.equal(e.api[name](...args), 0, name);
}
function transfer(e, size) {
  return new DataView(e.memory.buffer, e.base + 140, size);
}
function request(e, bytes) {
  new Uint8Array(e.memory.buffer, e.base + 140, bytes.length).set(bytes);
}
function arena(e) {
  return new Uint8Array(e.memory.buffer).slice(e.base, e.base + 4796);
}
function install(e, pc, length, slot, gates = [], extra = []) {
  const block = new Uint32Array([pc, length, ...extra.flat(), ...gates.flat()]);
  request(e, new Uint8Array(block.buffer));
  ok(e, "compile_resident_with_gates", 1 + extra.length, gates.length);
  const d = transfer(e, 24),
    id = d.getBigUint64(8, true),
    pointer = d.getUint32(16, true),
    size = d.getUint32(20, true);
  const module = new WebAssembly.Module(
      new Uint8Array(e.memory.buffer).slice(pointer, pointer + size),
    ),
    ring3 = {};
  for (const entry of WebAssembly.Module.imports(module))
    if (entry.module === "ring3") ring3[entry.name] = e.api[entry.name];
  const run = new WebAssembly.Instance(module, { env: { memory: e.memory }, ring3 }).exports.run;
  e.table.set(slot, run);
  ok(e, "acknowledge_resident_installation", ...key, ...pair(id), slot);
  const unit = { id, slot, run, imports: ring3 };
  e.installed.push(unit);
  return unit;
}
function reset(e, pc = 0x1000, ecx = 3, cancel = 0) {
  const bytes = new Uint8Array(e.memory.buffer, e.base, 56),
    v = new DataView(e.memory.buffer);
  bytes.fill(0);
  bytes.set(Buffer.from("R3ST"));
  v.setUint16(e.base + 4, 3, true);
  v.setUint16(e.base + 6, 1, true);
  v.setUint32(e.base + 8, 56, true);
  [5, ecx, 0, 0, 0x8800, 0, 0x8000, 0x8100].forEach((value, i) =>
    v.setUint32(e.base + 16 + i * 4, value, true),
  );
  v.setUint32(e.base + 48, pc, true);
  v.setUint32(e.base + 52, 2, true);
  v.setUint32(e.base + 96, cancel, true);
}
function header(v, magic, size) {
  assert.equal(v.getUint32(0, true), Buffer.from(magic).readUInt32LE());
  assert.equal(v.getUint32(4, true), 0x10001);
  assert.equal(v.getUint32(8, true), size);
  assert.equal(v.getUint32(12, true), 0);
}
function begin(e, unit, budget, dispatch = true) {
  ok(e, "begin_execution_receipt", ...key, budget, ...pair(unit.id), dispatch ? 1 : 0);
  const v = transfer(e, 32);
  header(v, "R3EF", 32);
  const token = v.getBigUint64(16, true);
  assert.ok(token > 0n);
  return token;
}
function finish(e, token) {
  ok(e, "finish_execution_receipt", ...key, ...pair(token));
  const v = transfer(e, 64);
  header(v, "R3ER", 64);
  const result = {
    epoch: v.getBigUint64(16, true),
    owner: v.getBigUint64(24, true),
    slot: v.getUint32(32, true),
    retired: v.getUint32(36, true),
    child: v.getUint32(40, true),
    reason: v.getUint32(44, true),
    kind: v.getUint32(48, true),
  };
  assert.deepEqual(new Uint8Array(e.memory.buffer, e.base + 140 + 52, 12), new Uint8Array(12));
  return result;
}
function execute(e, unit, budget, dispatch = true) {
  const token = begin(e, unit, budget, dispatch);
  assert.equal((dispatch ? e.dispatch : unit.run)(e.base, e.base + 56, budget, e.base + 96), 0);
  const v = new DataView(e.memory.buffer),
    outer = v.getUint32(e.base + 72, true),
    retired = v.getUint32(e.base + 76, true);
  const receipt = finish(e, token);
  assert.equal(receipt.reason, outer);
  assert.equal(receipt.retired, retired);
  assert.notEqual(e.api.finish_execution_receipt(...key, ...pair(token)), 0);
  return receipt;
}
function record(label, receipt) {
  cases.push({
    label,
    ...receipt,
    epoch: receipt.epoch.toString(),
    owner: receipt.owner.toString(),
  });
}

// Cold miss, budget boundaries, full warm loop, no-child cancellation, direct entry.
for (const budget of [0, 1, 2, 3, 4, 12, 20]) {
  const e = fresh(),
    a = install(e, 0x1000, 6, 0),
    b = install(e, 0x2000, 7, 1);
  reset(e);
  const result = execute(e, a, budget);
  assert.ok(result.retired <= budget);
  if (budget === 0) assert.equal(result.kind, 0);
  if (budget === 12 || budget === 20)
    assert.deepEqual(
      [result.retired, result.reason, result.owner],
      [12, budget === 12 ? 1 : 3, b.id],
    );
  record(`warm-${budget}`, result);
  ok(e, "close");
}
{
  const e = fresh(),
    a = install(e, 0x1000, 6, 0);
  reset(e);
  const result = execute(e, a, 20);
  assert.deepEqual([result.retired, result.child, result.reason, result.kind], [2, 2, 3, 2]);
  record("cold-lookup-miss-version-restored", result);
  reset(e, 0x1000, 3, 1);
  record("cancel-no-child", execute(e, a, 20));
  reset(e);
  const direct = execute(e, a, 20, false);
  assert.deepEqual([direct.retired, direct.reason, direct.kind], [2, 3, 2]);
  record("direct", direct);
  ok(e, "close");
}
// A previous REP's work cannot make a fresh direct non-REP budget fail.
{
  const e = fresh(),
    a = install(e, 0x1000, 6, 0);
  reset(e);
  new DataView(e.memory.buffer).setUint32(e.base + 4604, 100, true);
  const result = execute(e, a, 1, false);
  assert.deepEqual([result.retired, result.reason, result.kind], [1, 1, 1]);
  record("direct-resets-previous-work", result);
  ok(e, "close");
}
// Cancellation arriving after a genuine sealed child follows the synthetic outer stop path.
{
  const e = fresh(),
    seal = e.api.finish_exception_detection;
  e.api.finish_exception_detection = (...args) => {
    const status = seal(...args);
    if (status === 0) new DataView(e.memory.buffer).setUint32(e.base + 96, 1, true);
    return status;
  };
  const a = install(e, 0x1000, 6, 0);
  reset(e);
  const result = execute(e, a, 20);
  assert.deepEqual([result.reason, result.retired, result.child, result.kind], [2, 2, 2, 2]);
  record("cancel-after-sealed-child", result);
  ok(e, "close");
}
// Invalid-token/material/context attempts preserve the live frame, output and arena.
{
  const e = fresh(),
    a = install(e, 0x1000, 6, 0);
  reset(e);
  const token = begin(e, a, 20, false);
  assert.equal(a.run(e.base, e.base + 56, 20, e.base + 96), 0);
  for (const args of [
    [key[0] ^ 1, key[1], ...pair(token)],
    [...key, ...pair(token + 1n)],
  ]) {
    const before = arena(e);
    assert.notEqual(e.api.finish_execution_receipt(...args), 0);
    assert.deepEqual(arena(e), before);
  }
  for (const offset of [16, 4236, 4612]) {
    const bytes = new Uint8Array(e.memory.buffer);
    bytes[e.base + offset] ^= 1;
    const before = arena(e);
    assert.notEqual(e.api.finish_execution_receipt(...key, ...pair(token)), 0);
    assert.deepEqual(arena(e), before);
    bytes[e.base + offset] ^= 1;
  }
  record("retry-after-invalid-finish", finish(e, token));
  ok(e, "close");
}
// A table entry with valid signature but the wrong embedded owner cannot enter.
{
  const e = fresh(),
    a = install(e, 0x1000, 6, 0),
    b = install(e, 0x2000, 7, 1);
  reset(e);
  e.table.set(0, b.run);
  begin(e, a, 20);
  const before = new DataView(e.memory.buffer).getBigUint64(e.base + 140 + 24, true);
  assert.notEqual(e.dispatch(e.base, e.base + 56, 20, e.base + 96), 0);
  assert.equal(before, 0n);
  ok(e, "close");
  cases.push({ label: "swapped-table-owner-rejected" });
}
// Actual generated exception and partial REP produce terminal cpu_event rather than Returned.
for (const [pc, length, budget, ecx, label] of [
  [0x2200, 2, 20, 0, "generated-invalid-opcode"],
  [0x2210, 2, 20, 0, "generated-page-fault"],
  [0x2300, 2, 1, 4, "partial-rep-budget"],
]) {
  const e = fresh(),
    unit = install(e, pc, length, 0);
  reset(e, pc, ecx);
  const result = execute(e, unit, budget);
  if (pc === 0x2200) assert.deepEqual([result.reason, result.kind, result.retired], [20, 3, 0]);
  else if (pc === 0x2210)
    assert.deepEqual([result.reason, result.kind, result.retired], [23, 3, 0]);
  else
    assert.ok(
      result.retired === 0 && result.reason === 1,
      `${result.reason}/${result.retired}/${result.kind}`,
    );
  record(label, result);
  if (result.kind === 3) ok(e, "take_cpu_event");
  ok(e, "close");
}
// Real resident callback authorization survives engine-owned direct and dispatcher receipts.
for (const dispatch of [false, true]) {
  const e = fresh(),
    outer = install(e, 0x2400, 2, 0, [[0x2400, 17]]),
    callback = install(e, 0x2500, 6, 1, [[0x2600, 18]], [[0x2600, 2]]);
  ok(e, "write32", 0x8800, 0x2700);
  reset(e, 0x2400);
  const stopped = execute(e, outer, 20, false);
  assert.equal(stopped.reason, 8);
  ok(e, "capture_resident_call", ...key, ...pair(outer.id), 1, 0);
  const outerToken = transfer(e, 112).getUint32(16, true);
  ok(
    e,
    "begin_resident_callback",
    ...key,
    ...pair(outer.id),
    ...pair(callback.id),
    outerToken,
    0x2500,
    0x2600,
    18,
    0,
  );
  const callbackToken = transfer(e, 72).getUint32(16, true);
  ok(e, "authorize_resident_callback", ...key, ...pair(callback.id), callbackToken);
  const result = execute(e, callback, 20, dispatch);
  assert.deepEqual([result.owner, result.reason, result.retired], [callback.id, 8, 2]);
  assert.equal(new DataView(e.memory.buffer).getUint32(e.base + 16, true), 7);
  ok(e, "abort_callback", ...key, callbackToken);
  ok(e, "complete_resident_call", ...key, ...pair(outer.id), outerToken, 7);
  record(dispatch ? "callback-dispatch" : "callback-direct", result);
  ok(e, "close");
}
writeFileSync(
  join(output, "result.json"),
  JSON.stringify({ engine_sha256: engine.sha256, cases }, null, 2),
);
console.log(JSON.stringify({ engine: engine.sha256, cases: cases.length, output }));
