import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';

const [enginePath, outputDir, root] = process.argv.slice(2);
assert.ok(root, 'expected engine, output directory and repository root');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const engineBytes = readFileSync(enginePath), dispatcherBytes = readFileSync(join(outputDir, 'dispatcher.wasm'));
assert.ok(WebAssembly.validate(engineBytes)); assert.ok(WebAssembly.validate(dispatcherBytes));
const engineModule = new WebAssembly.Module(engineBytes), dispatcherModule = new WebAssembly.Module(dispatcherBytes);
assert.deepEqual(WebAssembly.Module.imports(engineModule), []);
assert.deepEqual(WebAssembly.Module.imports(dispatcherModule), [{module: 'env', name: 'memory', kind: 'memory'}, {module: 'env', name: 'table', kind: 'table'}]);
assert.deepEqual(WebAssembly.Module.exports(dispatcherModule), [{name: 'run', kind: 'function'}]);
const names = ['open', 'close', 'arena_ptr', 'map', 'protect', 'unmap', 'upload', 'compile', 'compile_with_gates', 'generation', 'module_ptr', 'module_len', 'guard', 'read32', 'write32', 'capture_call', 'abandon_call', 'compile_resident', 'resident_module', 'guard_resident'];
const SIZE = 4236, TRANSFER = 140, READY = 4224, CALLS = 4228;
const authored = {a: Buffer.from('40e9fa0f0000', 'hex'), b: Buffer.from('490f85f9efffff', 'hex'), gate: Buffer.from('0f0b', 'hex'), unsupported: Buffer.from('8b00', 'hex')};
for (const [name, bytes] of Object.entries(authored)) writeFileSync(join(outputDir, `${name}.x86`), bytes);
const artifacts = {'dispatcher.wasm': hash(dispatcherBytes)}, units = [], metadata = [];
const stats = {engine_instances: 0, resident_compiles: 0, resident_getters: 0, dispatcher_calls: 0, dispatcher_child_calls: 0, direct_unit_calls: 0, direct_unit_canonical: 0, direct_unit_rejections: 0, direct_guard_calls: 0, direct_guard_successes: 0, direct_guard_rejections: 0, compile_rejections: 0, getter_rejections: 0, full_arena_snapshots: 0, metadata_only_snapshots: 0, continuation_initial_calls: 0, continuation_resumes: 0, cold_b_installations: 0, budget_exits: 0, cancelled_exits: 0, need_code_exits: 0, legacy_runs: 0, pending_captures: 0, pending_abandons: 0};

function refresh(engine) {
  if (engine.buffer !== engine.memory.buffer) { engine.buffer = engine.memory.buffer; engine.bytes = new Uint8Array(engine.buffer); engine.view = new DataView(engine.buffer); }
  return engine;
}
function arena(engine) { return refresh(engine).bytes.slice(engine.base, engine.base + SIZE); }
function fresh() {
  const instance = new WebAssembly.Instance(engineModule, {});
  const api = Object.fromEntries(names.map(name => { const fn = instance.exports[`ring3_abi_v1_${name}`]; assert.equal(typeof fn, 'function', name); return [name, fn]; }));
  const key = 0x81234567f0000000n + BigInt(++stats.engine_instances);
  const engine = refresh({instance, api, memory: instance.exports.memory, key, low: Number(key & 0xffffffffn), high: Number(key >> 32n)});
  assert.equal(api.guard_resident.length, 7); assert.equal(api.compile_resident.length, 1); assert.equal(api.resident_module.length, 2);
  assert.equal(api.open(16, engine.low, engine.high), 0);
  engine.base = api.arena_ptr() >>> 0; refresh(engine);
  assert.ok(engine.base > 0 && engine.base + SIZE <= engine.bytes.length);
  return engine;
}
function upload(engine, address, bytes) {
  refresh(engine).bytes.set(bytes, engine.base + TRANSFER);
  assert.equal(engine.api.upload(address, bytes.length), 0); refresh(engine);
}
function descriptors(engine, blocks) {
  refresh(engine); blocks.forEach(([pc, length], index) => { engine.view.setUint32(engine.base + TRANSFER + index * 8, pc, true); engine.view.setUint32(engine.base + TRANSFER + index * 8 + 4, length, true); });
}
function header(bytes, view, pointer, magic, size, version = 1) {
  bytes.set(Buffer.from(magic, 'ascii'), pointer); view.setUint16(pointer + 4, version, true); view.setUint16(pointer + 6, 1, true); view.setUint32(pointer + 8, size, true); view.setUint32(pointer + 12, 0, true);
}
function initial(eax = 0x89abcdef, ecx = 3, eflags = 0x403) {
  return {registers: [eax, ecx, 0x23456789, 0x3456789a, 0x5000, 0x56789abc, 0x6789abcd, 0x789abcde], eip: 0x1000, eflags};
}
function reset(engine, value, cancel = 0, ready = 3) {
  refresh(engine); header(engine.bytes, engine.view, engine.base, 'R3ST', 56);
  value.registers.forEach((r, i) => engine.view.setUint32(engine.base + 16 + 4 * i, r, true));
  engine.view.setUint32(engine.base + 48, value.eip, true); engine.view.setUint32(engine.base + 52, value.eflags, true);
  engine.bytes.fill(0xa5, engine.base + 56, engine.base + 96); engine.view.setUint32(engine.base + 96, cancel, true);
  engine.bytes.fill(0x5a, engine.base + 100, engine.base + 140); engine.view.setUint32(engine.base + READY, ready, true); engine.view.setUint32(engine.base + CALLS, 0xdeadbeef, true);
}
function flags(op, operand, oldFlags) {
  const a = BigInt(operand), delta = op === 'inc' ? 1n : -1n, value = Number(BigInt.asUintN(32, a + delta));
  let out = (oldFlags & 0x401) | 2, parity = 0;
  for (let byte = value & 255; byte; byte >>>= 1) parity += byte & 1;
  if (parity % 2 === 0) out |= 4; if (value === 0) out |= 0x40; if (value >= 0x80000000) out |= 0x80;
  if (op === 'inc' ? (operand & 15) === 15 : (operand & 15) === 0) out |= 0x10;
  const signed = BigInt.asIntN(32, a) + delta; if (signed < -2147483648n || signed > 2147483647n) out |= 0x800;
  return out;
}
for (const [op, operand, oldFlags, expected] of [['inc', 0, 2, 2], ['inc', 0xffffffff, 3, 0x57], ['inc', 0x7fffffff, 2, 0x896], ['dec', 1, 0x403, 0x447], ['dec', 0, 2, 0x96], ['dec', 0x80000000, 2, 0x816]]) assert.equal(flags(op, operand, oldFlags), expected);

// the authored four-instruction cycle gives closed-form state and last arithmetic flags.
function trace(start, steps) {
  const n = start.registers[1], cycles = Math.floor(steps / 4), phase = steps % 4;
  assert.ok(steps >= 0 && steps <= n * 4);
  const value = {...start, registers: [...start.registers]};
  value.registers[0] = Number(BigInt.asUintN(32, BigInt(start.registers[0]) + BigInt(cycles + (phase > 0 ? 1 : 0))));
  value.registers[1] = n - cycles - (phase === 3 ? 1 : 0);
  value.eip = steps === n * 4 ? 0x2007 : [0x1000, 0x1001, 0x2000, 0x2001][phase];
  if (steps) {
    const inc = phase === 1 || phase === 2;
    const operand = inc ? Number(BigInt.asUintN(32, BigInt(start.registers[0]) + BigInt(cycles))) : n - cycles + (phase === 0 ? 1 : 0);
    value.eflags = flags(inc ? 'inc' : 'dec', operand, start.eflags);
  }
  return value;
}
function exit(reason, retired, version = 1, detail = 0) {
  const bytes = new Uint8Array(40), view = new DataView(bytes.buffer); header(bytes, view, 0, 'R3EX', 40, version);
  view.setUint32(16, reason, true); view.setUint32(20, retired, true); view.setUint32(24, detail, true); return bytes;
}
function expectedArena(before, value, exitBytes, calls) {
  const wanted = before.slice(), view = new DataView(wanted.buffer);
  value.registers.forEach((r, i) => view.setUint32(16 + i * 4, r, true)); view.setUint32(48, value.eip, true); view.setUint32(52, value.eflags, true); wanted.set(exitBytes, 56);
  if (calls !== undefined) view.setUint32(CALLS, calls, true); return wanted;
}
function unchanged(engine, action, status, label, counter) {
  const before = arena(engine); assert.equal(action(), status, label); assert.deepEqual(arena(engine), before, `${label}: full arena unchanged`); stats.full_arena_snapshots++;
  if (counter) stats[counter]++;
}
function guard(engine, unit, status = 0, options = {}) {
  const args = [options.low ?? engine.low, options.high ?? engine.high, options.idLow ?? unit.low, options.idHigh ?? unit.high, options.state ?? engine.base, options.exit ?? engine.base + 56, options.cancel ?? engine.base + 96];
  unchanged(engine, () => engine.api.guard_resident(...args), status, 'direct resident guard priority', status === 0 ? 'direct_guard_successes' : 'direct_guard_rejections'); stats.direct_guard_calls++;
}
function rejectUnit(engine, unit, status, label, pointers = [engine.base, engine.base + 56, engine.base + 96]) {
  unchanged(engine, () => unit.run(pointers[0], pointers[1], 2, pointers[2]), status, label, 'direct_unit_rejections'); stats.direct_unit_calls++;
}
function direct(engine, unit, budget, value, reason, retired, label) {
  const before = arena(engine); assert.equal(unit.run(engine.base, engine.base + 56, budget, engine.base + 96), 0, label);
  assert.deepEqual(arena(engine), expectedArena(before, value, exit(reason, retired)), `${label}: full arena exact`);
  stats.direct_unit_calls++; stats.direct_unit_canonical++; stats.full_arena_snapshots++;
}
function uleb(bytes, cursor) {
  let out = 0, shift = 0, byte;
  do { byte = bytes[cursor.i++]; assert.ok(byte !== undefined && shift <= 28); out += (byte & 127) * 2 ** shift; shift += 7; } while (byte & 128);
  return out;
}
function sleb32(bytes, cursor) {
  let out = 0n, shift = 0n, byte;
  do { byte = bytes[cursor.i++]; assert.ok(byte !== undefined && shift <= 28n); out |= BigInt(byte & 127) << shift; shift += 7n; } while (byte & 128);
  if (byte & 64) out -= 1n << shift;
  return Number(BigInt.asUintN(32, out));
}
function bakedPrefix(bytes, engine, id) {
  assert.deepEqual([...bytes.subarray(0, 8)], [0, 97, 115, 109, 1, 0, 0, 0]);
  const cursor = {i: 8}; let found = false;
  while (cursor.i < bytes.length) {
    const section = bytes[cursor.i++], length = uleb(bytes, cursor), end = cursor.i + length;
    if (section === 10) {
      assert.equal(uleb(bytes, cursor), 1); const bodyLength = uleb(bytes, cursor), bodyEnd = cursor.i + bodyLength; assert.equal(bodyEnd, end);
      const groups = uleb(bytes, cursor); for (let i = 0; i < groups; i++) { uleb(bytes, cursor); assert.ok([0x7f, 0x7e].includes(bytes[cursor.i++])); }
      const constants = []; for (let i = 0; i < 4; i++) { assert.equal(bytes[cursor.i++], 0x41); constants.push(sleb32(bytes, cursor)); }
      assert.deepEqual(constants, [engine.low, engine.high, Number(id & 0xffffffffn), Number(id >> 32n)], 'immutable four-limb binding independently decoded from actual function start');
      assert.deepEqual([...bytes.subarray(cursor.i, cursor.i + 8)], [0x20, 0, 0x20, 1, 0x20, 3, 0x10, 0], 'direct guard call precedes CPU preflight'); found = true;
    }
    cursor.i = end;
  }
  assert.ok(found);
}
function readMetadata(engine, before, label) {
  const after = arena(engine), view = new DataView(after.buffer), fields = Array.from({length: 6}, (_, i) => view.getUint32(TRANSFER + i * 4, true));
  assert.equal(fields[0], 1); assert.equal(fields[1], 24);
  const [,, low, high, pointer, length] = fields, id = BigInt(low) | (BigInt(high) << 32n); assert.ok(id > 0n);
  refresh(engine); assert.ok(pointer > 0 && length > 8 && pointer + length <= engine.bytes.length);
  const wanted = before.slice(); wanted.set(after.subarray(TRANSFER, TRANSFER + 24), TRANSFER); assert.deepEqual(after, wanted, `${label}: only metadata24 changes`);
  const bytes = engine.bytes.slice(pointer, pointer + length); assert.ok(WebAssembly.validate(bytes)); bakedPrefix(bytes, engine, id);
  stats.metadata_only_snapshots++; metadata.push({label, id: id.toString(), low, high, pointer, length, sha256: hash(bytes)}); return {id, low, high, pointer, length, bytes};
}
function compile(engine, pc, length, label) {
  descriptors(engine, [[pc, length]]); const before = arena(engine); assert.equal(engine.api.compile_resident(1), 0, label); stats.resident_compiles++;
  const record = readMetadata(engine, before, label), module = new WebAssembly.Module(record.bytes);
  assert.deepEqual(WebAssembly.Module.imports(module), [{module: 'env', name: 'memory', kind: 'memory'}, {module: 'ring3', name: 'guard_resident', kind: 'function'}]);
  assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  const instance = new WebAssembly.Instance(module, {env: {memory: engine.memory}, ring3: {guard_resident: engine.api.guard_resident}}); assert.equal(instance.exports.run.length, 4);
  const filename = `unit-${units.length + 1}.wasm`; writeFileSync(join(outputDir, filename), record.bytes); artifacts[filename] = hash(record.bytes);
  const unit = {...record, module, instance, run: instance.exports.run, label, filename}; units.push(unit); return unit;
}
function getter(engine, unit, label) {
  const before = arena(engine); assert.equal(engine.api.resident_module(unit.low, unit.high), 0, label); stats.resident_getters++;
  const record = readMetadata(engine, before, label); assert.equal(record.id, unit.id); assert.equal(record.pointer, unit.pointer); assert.equal(record.length, unit.length); assert.deepEqual(record.bytes, unit.bytes);
}
function failedCompile(engine, blocks, count, status, label) {
  descriptors(engine, blocks); unchanged(engine, () => engine.api.compile_resident(count), status, label, 'compile_rejections');
}
function legacy(engine) {
  descriptors(engine, [[0x3000, 2]]); engine.view.setUint32(engine.base + TRANSFER + 8, 0x3000, true); engine.view.setUint32(engine.base + TRANSFER + 12, 77, true);
  assert.equal(engine.api.compile_with_gates(1, 1), 0); refresh(engine);
  const generation = engine.api.generation() >>> 0, pointer = engine.api.module_ptr() >>> 0, length = engine.api.module_len() >>> 0, bytes = engine.bytes.slice(pointer, pointer + length);
  const module = new WebAssembly.Module(bytes); assert.deepEqual(WebAssembly.Module.imports(module), [{module: 'env', name: 'memory', kind: 'memory'}, {module: 'ring3', name: 'guard', kind: 'function'}]);
  const instance = new WebAssembly.Instance(module, {env: {memory: engine.memory}, ring3: {guard: engine.api.guard}});
  writeFileSync(join(outputDir, 'legacy.wasm'), bytes); artifacts['legacy.wasm'] = hash(bytes); return {generation, pointer, length, bytes, run: instance.exports.run};
}
function legacyStable(engine, child) {
  assert.equal(engine.api.generation() >>> 0, child.generation); assert.equal(engine.api.module_ptr() >>> 0, child.pointer); assert.equal(engine.api.module_len() >>> 0, child.length);
  assert.deepEqual(refresh(engine).bytes.slice(child.pointer, child.pointer + child.length), child.bytes);
}
function tableCall(engine, budget, value, reason, retired, calls, label, continuation) {
  const before = arena(engine), refs = [engine.table.get(0), engine.table.get(1)];
  assert.equal(engine.dispatcher.exports.run(engine.base, engine.base + 56, budget, engine.base + 96), 0, label); refresh(engine);
  const actualCalls = engine.view.getUint32(engine.base + CALLS, true); assert.equal(actualCalls, calls);
  assert.deepEqual(arena(engine), expectedArena(before, value, exit(reason, retired), calls), `${label}: full arena including helper/transfer`);
  assert.equal(engine.table.get(0), refs[0]); assert.equal(engine.table.get(1), refs[1]); stats.dispatcher_calls++; stats.dispatcher_child_calls += actualCalls; stats.full_arena_snapshots++;
  if (continuation === 'initial') stats.continuation_initial_calls++; else if (continuation === 'resume') stats.continuation_resumes++;
  stats[reason === 1 ? 'budget_exits' : reason === 2 ? 'cancelled_exits' : 'need_code_exits']++;
}
function slice(engine, start, from, budget, label, continuation) {
  const total = start.registers[1] * 4, to = Math.min(total, from + budget), retired = to - from;
  tableCall(engine, budget, trace(start, to), budget <= total - from ? 1 : 3, retired, retired ? Math.ceil(to / 2) - Math.floor(from / 2) : 0, label, continuation); return to;
}

const engine = fresh(); assert.equal(engine.api.map(0x1000, 5, 7), 0);
for (const [name, pc] of [['a', 0x1000], ['b', 0x2000], ['gate', 0x3000], ['unsupported', 0x4000]]) upload(engine, pc, authored[name]);
const oldLegacy = legacy(engine), a = compile(engine, 0x1000, authored.a.length, 'cold A'); legacyStable(engine, oldLegacy);
engine.table = new WebAssembly.Table({element: 'anyfunc', initial: 2, maximum: 2}); engine.table.set(0, a.run);
engine.dispatcher = new WebAssembly.Instance(dispatcherModule, {env: {memory: engine.memory, table: engine.table}}); assert.equal(engine.dispatcher.exports.run.length, 4);
const coldStart = initial(0xfffffffd, 5, 0x403); reset(engine, coldStart, 0, 1);
tableCall(engine, 21, trace(coldStart, 2), 3, 2, 1, 'cold A commits before missing B', 'initial');
const b = compile(engine, 0x2000, authored.b.length, 'cold B after actual missing exit'); assert.notEqual(a.id, b.id);
engine.table.set(1, b.run); engine.view.setUint32(engine.base + READY, 3, true); stats.cold_b_installations++;
slice(engine, coldStart, 2, 19, 'true cold continuation without CPU patch', 'resume'); getter(engine, a, 'A pointer/ID bytes stable after B'); getter(engine, b, 'B metadata pairing'); legacyStable(engine, oldLegacy);
const stable = [engine.memory, engine.table, engine.dispatcher, a.module, a.instance, b.module, b.instance, a.run, b.run];
const warmInputs = [[1, 0xffffffff, 3], [3, 0x7fffffff, 0x402], [7, 0xfffffffd, 0xcd7]]; let warmCases = 0;
for (const [n, eax, oldFlags] of warmInputs) for (const budget of new Set([0, 1, 3, n * 4, n * 4 + 1])) {
  const start = initial(eax, n, oldFlags); reset(engine, start); slice(engine, start, 0, budget, 'warm closed-form trace'); warmCases++;
  assert.deepEqual([engine.memory, engine.table, engine.dispatcher, a.module, a.instance, b.module, b.instance, a.run, b.run], stable); assert.equal(engine.table.get(0), a.run); assert.equal(engine.table.get(1), b.run);
}
const resumedStart = initial(0x7ffffffe, 3, 0x403); reset(engine, resumedStart); let position = 0;
for (const [index, budget] of [1, 1, 1, 2, 0, 3, 5].entries()) {
  position = slice(engine, resumedStart, position, budget, `true budget/interior continuation ${index}`, index === 0 ? 'initial' : 'resume');
  if (index === 2) { engine.view.setUint32(engine.base + 96, 1, true); tableCall(engine, 0, trace(resumedStart, position), 2, 0, 0, 'cancel between resumes before zero budget', 'resume'); engine.view.setUint32(engine.base + 96, 0, true); }
}
assert.equal(position, 12); slice(engine, resumedStart, position, 1, 'done continuation repeats no instruction', 'resume');
const directStart = initial(0xffffffff, 2, 0x403); reset(engine, trace(directStart, 1)); direct(engine, a, 1, trace(directStart, 2), 1, 1, 'actual A interior entry'); direct(engine, b, 2, trace(directStart, 4), 1, 2, 'actual B entry continues');
reset(engine, trace(directStart, 3)); direct(engine, b, 1, trace(directStart, 4), 1, 1, 'actual B interior JNZ');
reset(engine, trace(directStart, 2), 1); direct(engine, b, 0, trace(directStart, 2), 2, 0, 'valid unit cancellation precedes budget');
reset(engine, directStart); guard(engine, a); guard(engine, b);
guard(engine, b, 3, {high: engine.high ^ 1}); guard(engine, b, 3, {idLow: 0, idHigh: 0}); guard(engine, b, 3, {idLow: 0xffffffff, idHigh: 0xffffffff});
for (const options of [{state: 0}, {exit: engine.base + 57}, {cancel: 0}]) guard(engine, b, 1, options);
rejectUnit(engine, b, 1, 'unit pointer guard before CPU preflight', [0, engine.base + 56, engine.base + 96]);
engine.bytes[engine.base] ^= 1; rejectUnit(engine, b, 2, 'valid unit guard then malformed CPU header'); engine.bytes[engine.base] ^= 1;
failedCompile(engine, [], 0, 7, 'zero descriptors'); failedCompile(engine, [], 9, 7, 'too many descriptors'); failedCompile(engine, [[0x1000, authored.a.length]], 1, 7, 'current instruction-start overlap'); failedCompile(engine, [[0x4000, authored.unsupported.length]], 1, 10, 'memory operand remains unsupported');
for (const [low, high] of [[0, 0], [0xffffffff, 0xffffffff]]) unchanged(engine, () => engine.api.resident_module(low, high), 3, 'unknown module ID preserves transfer', 'getter_rejections');
getter(engine, a, 'failed admissions retain A bytes and pointer'); getter(engine, b, 'failed admissions retain B bytes and pointer'); legacyStable(engine, oldLegacy);

// a legacy captured call owns the pending state; resident inspection only rewrites metadata.
const gateState = initial(); gateState.eip = 0x3000; reset(engine, gateState); assert.equal(engine.api.write32(0x5000, 0x2000), 0);
const gateBefore = arena(engine); assert.equal(oldLegacy.run(engine.base, engine.base + 56, 1, engine.base + 96), 0); stats.legacy_runs++; stats.full_arena_snapshots++;
assert.deepEqual(arena(engine), expectedArena(gateBefore, gateState, exit(8, 0, 3, 77)));
const captureBefore = arena(engine); assert.equal(engine.api.capture_call(engine.low, engine.high, oldLegacy.generation, 1, 0), 0); stats.pending_captures++;
const captured = arena(engine), captureWanted = captureBefore.slice(); captureWanted.set(captured.subarray(140, 252), 140); assert.deepEqual(captured, captureWanted); stats.full_arena_snapshots++;
const token = new DataView(captured.buffer).getUint32(156, true); assert.ok(token > 0);
failedCompile(engine, [], 0, 12, 'pending call busy before descriptor validation'); guard(engine, b, 12, {state: 0}); guard(engine, b, 3, {high: engine.high ^ 1}); rejectUnit(engine, b, 12, 'pending call blocks actual resident entry'); getter(engine, b, 'module inspection allowed while pending'); guard(engine, b, 12);
unchanged(engine, () => engine.api.abandon_call(engine.low, engine.high, token), 0, 'abandon pending without arena mutation'); stats.pending_abandons++; guard(engine, b);

assert.equal(engine.api.write32(0x1000, authored.a.readUInt32LE(0)), 0, 'real same-byte host code write');
guard(engine, a, 4, {state: 0}); guard(engine, b); rejectUnit(engine, a, 4, 'stale A cannot execute');
unchanged(engine, () => engine.api.resident_module(a.low, a.high), 4, 'stale module retrieval', 'getter_rejections'); getter(engine, b, 'B current after unrelated A mutation');
const copiedA = new WebAssembly.Instance(new WebAssembly.Module(a.bytes.slice()), {env: {memory: engine.memory}, ring3: {guard_resident: engine.api.guard_resident}});
rejectUnit(engine, {...a, run: copiedA.exports.run}, 4, 'reinstantiating old bytes cannot change baked ID');
const replacement = compile(engine, 0x1000, authored.a.length, 'fresh same-PC A'); assert.ok(replacement.id > a.id && replacement.id > b.id); assert.notDeepEqual(replacement.bytes, a.bytes);
guard(engine, a, 4); rejectUnit(engine, a, 4, 'old A rejects even when fresh A exists at same PC'); guard(engine, replacement); getter(engine, b, 'B pointer stable after replacement'); legacyStable(engine, oldLegacy);
engine.table.set(0, replacement.run); const replacementStart = initial(0xffffffff, 2, 0x403); reset(engine, replacementStart); slice(engine, replacementStart, 0, 9, 'fresh A and retained B warm completion');
const foreign = fresh(); assert.equal(foreign.api.map(0x1000, 1, 7), 0); upload(foreign, 0x1000, authored.a); const foreignA = compile(foreign, 0x1000, authored.a.length, 'separate engine A');
const rebound = new WebAssembly.Instance(foreignA.module, {env: {memory: engine.memory}, ring3: {guard_resident: engine.api.guard_resident}}); rejectUnit(engine, {...foreignA, run: rebound.exports.run}, 3, 'foreign key baked into unit rejects installation into main engine');
assert.notEqual(foreign.key, engine.key); guard(foreign, foreignA); const foreignBeforeClose = arena(foreign); assert.equal(foreign.api.close(), 0); assert.deepEqual(arena(foreign), foreignBeforeClose); stats.full_arena_snapshots++;
rejectUnit(foreign, foreignA, 5, 'copied installed unit rejects closed engine'); guard(foreign, foreignA, 5, {low: 0, idLow: 0, state: 0}); unchanged(foreign, () => foreign.api.resident_module(foreignA.low, foreignA.high), 5, 'closed metadata getter', 'getter_rejections');

const capacity = fresh(); assert.equal(capacity.api.map(0x7000, 1, 7), 0);
const nopJump = Buffer.from('90e900000000', 'hex');
for (let i = 0; i < 9; i++) upload(capacity, 0x7000 + i * 16, nopJump);
const retained = Array.from({length: 8}, (_, i) => compile(capacity, 0x7000 + i * 16, nopJump.length, `capacity unit ${i}`)); assert.equal(new Set(retained.map(unit => unit.id.toString())).size, 8);
failedCompile(capacity, [[0x7080, nopJump.length]], 1, 18, 'ninth distinct unit capacity'); for (const unit of retained) getter(capacity, unit, 'capacity failure retains prior publication');
const beforeClose = arena(engine); assert.equal(engine.api.close(), 0); assert.deepEqual(arena(engine), beforeClose); stats.full_arena_snapshots++; assert.equal(engine.api.arena_ptr(), 0);
rejectUnit(engine, a, 5, 'closed outranks stale old A'); rejectUnit(engine, replacement, 5, 'closed fresh installed A'); rejectUnit(engine, b, 5, 'closed retained B'); guard(engine, b, 5, {low: 0, idLow: 0, state: 0}); unchanged(engine, () => engine.api.resident_module(b.low, b.high), 5, 'closed B metadata', 'getter_rejections'); failedCompile(engine, [], 0, 5, 'closed before invalid compile count');

assert.equal(stats.direct_unit_calls, stats.direct_unit_canonical + stats.direct_unit_rejections); assert.equal(stats.direct_guard_calls, stats.direct_guard_successes + stats.direct_guard_rejections);
assert.equal(stats.dispatcher_calls, stats.budget_exits + stats.cancelled_exits + stats.need_code_exits); assert.equal(stats.cold_b_installations, 1);
for (const [name, bytes] of Object.entries(authored)) artifacts[`${name}.x86`] = hash(bytes);
const productionPaths = execFileSync('rg', ['--files', 'engine/src'], {cwd: root, encoding: 'utf8'}).trim().split('\n').sort();
const sourcePaths = ['engine/tests/process_resident_wasm.rs', 'engine/tests/fixtures/p2-resident-process/run.mjs', 'engine/tests/process_resident.rs', 'engine/Cargo.toml', 'Cargo.lock', ...productionPaths];
const provenance = {
  stats, actual_resident_run_calls: stats.dispatcher_child_calls + stats.direct_unit_calls, actual_resident_guard_calls: stats.dispatcher_child_calls + stats.direct_unit_calls + stats.direct_guard_calls,
  engine_sha256: hash(engineBytes), artifact_sha256: artifacts, module_set_sha256: hash(Object.keys(artifacts).filter(name => name.endsWith('.wasm')).sort().map(name => `${name}:${artifacts[name]}`).join('\n')),
  generated_resident_units: units.length, authored_dispatchers: 1, legacy_modules: 1, unit_bindings: units.map(unit => ({label: unit.label, id: unit.id.toString(), low: unit.low, high: unit.high, pointer: unit.pointer, length: unit.length, filename: unit.filename})), metadata_observations: metadata,
  sources: Object.fromEntries(sourcePaths.map(path => [path, hash(readFileSync(join(root, path)))])), production: {git_head: execFileSync('git', ['rev-parse', 'HEAD'], {cwd: root, encoding: 'utf8'}).trim(), source_files: productionPaths.length, source_set_sha256: hash(productionPaths.map(path => `${path}:${hash(readFileSync(join(root, path)))}`).join('\n'))},
  mathematical_inputs: {cold: coldStart, warm: warmInputs, warm_cases: warmCases, split: resumedStart, replacement: replacementStart},
  tools: {node: process.version, v8: process.versions.v8, platform: process.platform, architecture: process.arch, rustc: execFileSync('rustc', ['--version'], {encoding: 'utf8'}).trim(), cargo: execFileSync('cargo', ['--version'], {encoding: 'utf8'}).trim()},
  commands: {run: [process.execPath, process.argv[1], enginePath, outputDir, root], generate_and_run: ['cargo', 'test', '-p', 'ring3-engine', '--test', 'process_resident_wasm', '--', '--nocapture']},
  claim: 'actual engine-Wasm register-only process-bound resident units; direct exported Wasm guard and exact engine memory; independently decoded baked key/ID limbs; metadata24/pointer/copy pairing; two-unit cold installation and bounded warm returning table traces, exact flags/state/retirement/budget/interior/cancel continuations without intermediate CPU patches; full 4236-byte arena preservation on rejection; pending legacy call Busy and metadata inspection; same-byte host mutation invalidates only A, fresh same-PC replacement retains B, re-instantiated stale A rejects; wrong key/unregistered IDs/pointers/header/close/compile admission/capacity proof. IDs use BigInt and are allocation-domain local. Test-only dispatcher counter/readiness occupy transfer tail. No product dispatcher/lookup/installer, callback migration, active SMC Store4, browser/asynchronous cancellation/performance/game claim.',
};
writeFileSync(join(outputDir, 'provenance.json'), JSON.stringify(provenance, null, 2));
console.log(JSON.stringify({...provenance, sources: Object.keys(provenance.sources).length, metadata_observations: metadata.length, artifacts: outputDir}));
