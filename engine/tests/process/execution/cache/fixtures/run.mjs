import assert from 'node:assert/strict';
import {writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {pathToFileURL} from 'node:url';
import {createHash} from 'node:crypto';

const [enginePath, output, root] = process.argv.slice(2);
const {readEngine} = await import(pathToFileURL(join(root, 'engine/tests/support/wasm/engine.mjs')));
const engine = readEngine(enginePath);
const KEY = 0x8172635445362718n, TRANSFER = 140, CODE = 0x1000;
const low = value => Number(value & 0xffffffffn), high = value => Number(value >> 32n);
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const source = Uint8Array.from([...Array(48).fill(0x90), 0x8d, 0x40, 1, 0x49, 0x75, 0xea]);
const faultSource = Uint8Array.from([...Array(20).fill(0x90), 0x8b, 3, 0xc3]);
const receipts = [], cases = [];
function refresh(e) {
  if (e.bytes?.buffer !== e.memory.buffer) {
    e.bytes = new Uint8Array(e.memory.buffer);
    e.view = new DataView(e.memory.buffer);
  }
  return e;
}
function ok(e, name, ...args) {
  const result = e.api[name](...args);
  assert.equal(result, 0, `${name}(${args.join(',')}) status`);
  refresh(e);
}
function request(e) {
  refresh(e);
  const at = e.base + TRANSFER, v = e.view;
  assert.equal(Buffer.from(e.bytes.slice(at, at + 4)).toString(), 'R3CQ');
  assert.equal(v.getUint16(at + 4, true), 1);
  assert.equal(v.getUint16(at + 6, true), 1);
  assert.equal(v.getUint32(at + 8, true), 64);
  assert.equal(v.getUint32(at + 60, true), 0);
  if (v.getUint32(at + 12, true) === 0) {
    assert.deepEqual(e.bytes.slice(at + 16, at + 64), new Uint8Array(48));
    return undefined;
  }
  assert.equal(v.getUint32(at + 12, true), 1);
  return {serial: v.getBigUint64(at + 16, true), kind: v.getUint32(at + 24, true), pc: v.getUint32(at + 28, true), id: v.getBigUint64(at + 32, true), victim: v.getBigUint64(at + 40, true), slot: v.getUint32(at + 48, true), victimSlot: v.getUint32(at + 52, true), tier: v.getUint32(at + 56, true)};
}
function fresh(label) {
  const instance = new WebAssembly.Instance(engine.module, {});
  const api = Object.fromEntries(Object.entries(instance.exports).filter(([name, fn]) => name.startsWith('ring3_abi_v1_') && typeof fn === 'function').map(([name, fn]) => [name.slice(13), fn]));
  const e = {api, memory: instance.exports.memory, label, table: new WebAssembly.Table({element: 'anyfunc', initial: 16, maximum: 16})};
  ok(e, 'open', 16, low(KEY), high(KEY));
  e.base = api.arena_ptr() >>> 0;
  ok(e, 'map', CODE, 1, 7);
  ok(e, 'map', 0x8000, 1, 3);
  ok(e, 'configure_execution', 16, 1024 * 1024, 8);
  for (const [pc, bytes] of [[CODE, source], [0x1200, faultSource]]) {
    refresh(e).bytes.set(bytes, e.base + TRANSFER);
    ok(e, 'upload', pc, bytes.length);
  }
  return e;
}
function install(e, pc, exitVersion) {
  ok(e, 'prepare_cache_entry', low(KEY), high(KEY), pc);
  const receipt = request(e);
  assert.equal(receipt.kind, 1);
  ok(e, 'resident_module', low(receipt.id), high(receipt.id));
  const at = e.base + TRANSFER, ptr = e.view.getUint32(at + 16, true), length = e.view.getUint32(at + 20, true);
  const bytes = e.bytes.slice(ptr, ptr + length), module = new WebAssembly.Module(bytes);
  const imports = {env: {memory: e.memory}, ring3: {}};
  for (const entry of WebAssembly.Module.imports(module)) {
    if (entry.kind === 'function') {
      assert.equal(typeof e.api[entry.name], 'function', entry.name);
      imports.ring3[entry.name] = e.api[entry.name];
    }
  }
  const run = new WebAssembly.Instance(module, imports).exports.run;
  e.table.set(receipt.slot, run);
  ok(e, 'install_cached_entry', low(KEY), high(KEY), low(receipt.serial), high(receipt.serial), low(receipt.id), high(receipt.id), receipt.slot);
  const filename = `${e.label}-${pc.toString(16)}-tier${receipt.tier}.wasm`;
  writeFileSync(join(output, filename), bytes);
  receipts.push({...receipt, id: receipt.id.toString(), serial: receipt.serial.toString(), victim: receipt.victim.toString(), filename, bytes: length, sha256: hash(bytes)});
  return {...receipt, run, bytes, exitVersion};
}
function promote(e, unit) {
  for (let index = 0; index < 8; index++) ok(e, 'find_installed_resident', low(KEY), high(KEY), unit.pc);
  ok(e, 'prepare_cache_promotion', low(KEY), high(KEY));
  const receipt = request(e);
  assert.equal(receipt.kind, 2);
  assert.equal(receipt.tier, 1);
  assert.equal(receipt.victim, unit.id);
  e.table.set(receipt.victimSlot, null);
  ok(e, 'acknowledge_cache_eviction', low(KEY), high(KEY), low(receipt.serial), high(receipt.serial), low(receipt.victim), high(receipt.victim), receipt.victimSlot);
  // formation includes the branch fallthrough's memory instruction, so its
  // module-wide exit schema supports memory faults even when this run never falls through.
  const optimized = install(e, unit.pc, 6);
  assert.equal(optimized.tier, 1);
  assert.notEqual(optimized.id, unit.id);
  assert.equal(optimized.slot, unit.slot);
  return optimized;
}
function reset(e, {pc = CODE, ecx = 100, ebx = 0x8000, flags = 2, cancel = 0} = {}) {
  refresh(e);
  const at = e.base, v = e.view;
  e.bytes.fill(0, at, at + 140);
  e.bytes.set(Buffer.from('R3ST'), at);
  v.setUint16(at + 4, 3, true); v.setUint16(at + 6, 1, true); v.setUint32(at + 8, 56, true);
  [7, ecx, 0x23456789, ebx, 0x8ff0, 0x56789abc, 0x6789abcd, 0x789abcde].forEach((value, index) => v.setUint32(at + 16 + index * 4, value, true));
  v.setUint32(at + 48, pc, true); v.setUint32(at + 52, flags, true); v.setUint32(at + 96, cancel, true);
}
function execute(e, unit, options, budget) {
  reset(e, options);
  const status = unit.run(e.base, e.base + 56, budget, e.base + 96);
  refresh(e);
  const row = {status, snapshot: e.bytes.slice(e.base, e.base + 140), reason: e.view.getUint32(e.base + 72, true), retired: e.view.getUint32(e.base + 76, true), pc: e.view.getUint32(e.base + 48, true)};
  if (status === 0 && [16, 23].includes(row.reason)) ok(e, 'take_cpu_event');
  return row;
}
const baseline = fresh('baseline'), indexed = fresh('indexed');
const base = install(baseline, CODE, 1), opt = promote(indexed, install(indexed, CODE, 1));
assert.equal(base.tier, 0); assert.equal(opt.tier, 1);
const faultBase = install(baseline, 0x1200, 6), faultOpt = promote(indexed, install(indexed, 0x1200, 6));
function validateExit(row, unit, label) {
  const exit = new DataView(row.snapshot.buffer, row.snapshot.byteOffset + 56, 40);
  assert.equal(exit.getUint32(0, true), 0x58453352, `${label}: exit magic`);
  assert.equal(exit.getUint16(6, true), 1, `${label}: exit profile`);
  assert.equal(exit.getUint32(8, true), 40, `${label}: exit size`);
  assert.equal(exit.getUint32(12, true), 0, `${label}: exit reserved`);
  const expected = row.reason === 23 ? 12 : row.reason === 16 ? 7 : unit.exitVersion;
  assert.equal(exit.getUint16(4, true), expected, `${label}: literal module-specific exit schema`);
  row.exitVersion = expected;
}
function comparePayload(label, a, b) {
  const left = a.snapshot.slice(),
    right = b.snapshot.slice();
  // versions advertise module capabilities; validate them literally before
  // comparing every remaining state, exit-payload, cancellation and helper byte.
  left[60] = right[60] = 0;
  left[61] = right[61] = 0;
  assert.deepEqual(left, right, `${label}: guest state, exit payload, cancel, helper`);
}
function compare(label, left, right, options, budget, oracle) {
  const a = execute(baseline, left, options, budget),
    b = execute(indexed, right, options, budget);
  assert.equal(a.status, b.status, `${label}: status`);
  validateExit(a, left, `${label}/baseline`);
  validateExit(b, right, `${label}/formed`);
  comparePayload(label, a, b);
  if (oracle) oracle(a);
  cases.push({
    label,
    budget,
    status: a.status,
    reason: a.reason,
    retired: a.retired,
    pc: a.pc,
    baselineExitVersion: a.exitVersion,
    formedExitVersion: b.exitVersion,
  });
}
// ecx100 keeps the untaken fallthrough outside every bounded safepoint case.
for (const start of [0, 16, 32, 48, 51, 52]) {
  for (let budget = 0; budget <= 64; budget++) compare(`entry+${start}/budget${budget}`, base, opt, {pc: CODE + start}, budget, row => {
    assert.equal(row.status, 0);
    assert.equal(row.retired, budget);
    assert.equal(row.reason, 1);
  });
}
compare('interior LEA byte is not an instruction boundary', base, opt, {pc: CODE + 49}, 10, row => {assert.equal(row.retired, 0); assert.equal(row.reason, 3);});
compare('unsigned entry lower bound', base, opt, {pc: CODE - 1}, 10, row => {assert.equal(row.retired, 0); assert.equal(row.reason, 3);});
compare('trap flag after exactly one instruction', base, opt, {pc: CODE + 32, flags: 0x102}, 10, row => {assert.equal(row.retired, 1); assert.equal(row.reason, 16);});
compare('cancellation precedes zero budget', base, opt, {cancel: 1}, 0, row => {assert.equal(row.retired, 0); assert.equal(row.reason, 2);});
compare('memory fault retains committed prefix', faultBase, faultOpt, {pc: 0x1200, ebx: 0xdead0000}, 25, row => {assert.equal(row.retired, 20); assert.equal(row.reason, 23); assert.equal(row.pc, 0x1214);});
compare('memory read at middle entry', faultBase, faultOpt, {pc: 0x1214}, 1, row => {assert.equal(row.retired, 1); assert.equal(row.reason, 1); assert.equal(row.pc, 0x1216);});

const rounds = [];
function timed(e, unit) {
  // both tiers reach the same budget frontier before the final fallthrough.
  reset(e, { pc: CODE + 32, ecx: 20_001 });
  const begin = process.hrtime.bigint();
  const status = unit.run(e.base, e.base + 56, 380_000, e.base + 96);
  const elapsed = Number(process.hrtime.bigint() - begin);
  assert.equal(status, 0);
  refresh(e);
  assert.equal(e.view.getUint32(e.base + 76, true), 380_000);
  assert.equal(e.view.getUint32(e.base + 16, true), 20_007);
  assert.equal(e.view.getUint32(e.base + 20, true), 1);
  assert.equal(e.view.getUint32(e.base + 48, true), CODE + 32);
  assert.equal(e.view.getUint32(e.base + 72, true), 1);
  const row = { elapsed, snapshot: e.bytes.slice(e.base, e.base + 140), reason: 1 };
  validateExit(row, unit, `${e.label}/warm`);
  return row;
}
function timedPair(reverse) {
  let a, b;
  if (reverse) {
    b = timed(indexed, opt);
    a = timed(baseline, base);
  } else {
    a = timed(baseline, base);
    b = timed(indexed, opt);
  }
  comparePayload("warm matching budget frontier", a, b);
  return { baseline: a.elapsed, indexed: b.elapsed };
}
for (let index = 0; index < 12; index++) timedPair(index % 2);
for (let index = 0; index < 28; index++) {
  rounds.push(timedPair(index % 2));
}
function median(values) {values.sort((a, b) => a - b); return (values[13] + values[14]) / 2;}
const baseMedian = median(rounds.map(row => row.baseline)), optMedian = median(rounds.map(row => row.indexed));
for (const [e, unit] of [[baseline, base], [indexed, opt]]) {
  ok(e, 'write32', CODE, 0x90909091);
  reset(e, {});
  const before = e.bytes.slice(e.base, e.base + 140);
  assert.notEqual(unit.run(e.base, e.base + 56, 10, e.base + 96), 0, 'stale code must reject entry');
  assert.deepEqual(refresh(e).bytes.slice(e.base, e.base + 140), before, 'stale entry preserves state');
  ok(e, 'close');
}
const report = {engine_sha256: engine.sha256, source_hex: Buffer.from(source).toString('hex'), receipts, cases, warm: {rounds, baseline_median_ns: baseMedian, indexed_median_ns: optMedian, ratio: optMedian / baseMedian, retired_per_round: 380_000}, claim: 'actual Node WebAssembly engine and generated code; 396 bounded entry safepoints keep ECX100 branch fallthrough out of range; literal module-specific exit versions are checked before only their two bytes are normalized; all remaining payload bytes and warm matching-budget frontiers are equal; stale rejection retained; no browser or game throughput claim'};
writeFileSync(join(output, 'result.json'), JSON.stringify(report, null, 2));
console.log(JSON.stringify({engine_sha256: report.engine_sha256, differential_cases: cases.length, rounds: rounds.length, baseline_median_ns: baseMedian, indexed_median_ns: optMedian, ratio: report.warm.ratio, output}));
