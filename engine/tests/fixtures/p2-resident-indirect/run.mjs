import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';

const [enginePath, outputDir, root] = process.argv.slice(2);
assert.ok(root);
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const engineBytes = readFileSync(enginePath), engineModule = new WebAssembly.Module(engineBytes);
assert.deepEqual(WebAssembly.Module.imports(engineModule), []);
const SIZE = 4236, TRANSFER = 140, DATA = 0x5000;
const names = ['open', 'close', 'arena_ptr', 'map', 'protect', 'unmap', 'upload', 'generation', 'module_ptr', 'module_len', 'guard_resident', 'compile_resident', 'resident_module', 'find_resident', 'read32', 'write32'];
const stats = {engine_instances: 0, compiled_units: 0, canonical_runs: 0, rejected_runs: 0, full_arena_checks: 0, metadata_checks: 0, successful_guards: 0, failed_guards: 0, find_success: 0, find_failure: 0, continuation_initial_calls: 0, continuation_resumes: 0, target_cases: 0, algorithm_cases: 0, algorithm_retired: 0, faults: 0, repairs: 0, cancellations: 0, host_reads: 0, host_writes: 0, guest_read32: 0};
const units = [], artifacts = {}, metadata = [];

function refresh(engine) {
  if (engine.buffer !== engine.memory.buffer) { engine.buffer = engine.memory.buffer; engine.bytes = new Uint8Array(engine.buffer); engine.view = new DataView(engine.buffer); }
  return engine;
}

function arena(engine) { return refresh(engine).bytes.slice(engine.base, engine.base + SIZE); }

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

function noLegacy(engine) {
  assert.equal(engine.api.generation(), 0); assert.equal(engine.api.module_ptr(), 0); assert.equal(engine.api.module_len(), 0);
}
function fresh() {
  const instance = new WebAssembly.Instance(engineModule, {});
  const api = Object.fromEntries(names.map(name => [name, instance.exports[`ring3_abi_v1_${name}`]]));
  for (const [name, fn] of Object.entries(api)) assert.equal(typeof fn, 'function', name);
  const key = 0x9223456780000000n + BigInt(++stats.engine_instances);
  const engine = refresh({instance, api, memory: instance.exports.memory, key, low: Number(key & 0xffffffffn), high: Number(key >> 32n)});
  assert.equal(api.open(24, engine.low, engine.high), 0); engine.base = api.arena_ptr() >>> 0; refresh(engine);
  assert.ok(engine.base > 0 && engine.base + SIZE <= engine.bytes.length); noLegacy(engine); return engine;
}
function record(engine, before, label) {
  const after = arena(engine), view = new DataView(after.buffer), fields = Array.from({length: 6}, (_, i) => view.getUint32(TRANSFER + i * 4, true));
  assert.equal(fields[0], 1); assert.equal(fields[1], 24);
  const [,, low, high, pointer, length] = fields, id = BigInt(low) | (BigInt(high) << 32n);
  assert.ok(id > 0n && pointer > 0 && length > 8 && pointer + length <= refresh(engine).bytes.length);
  const wanted = before.slice(); wanted.set(after.subarray(TRANSFER, TRANSFER + 24), TRANSFER); assert.deepEqual(after, wanted, `${label}: only metadata24`);
  const bytes = engine.bytes.slice(pointer, pointer + length); assert.ok(WebAssembly.validate(bytes)); bakedPrefix(bytes, engine, id); noLegacy(engine);
  stats.metadata_checks++; metadata.push({label, id: id.toString(), pointer, length, sha256: hash(bytes)}); return {id, low, high, pointer, length, bytes};
}
function compile(engine, blocks, reads, label) {
  descriptors(engine, blocks); const before = arena(engine); assert.equal(engine.api.compile_resident(blocks.length), 0, label);
  const selected = record(engine, before, label), module = new WebAssembly.Module(selected.bytes);
  const expected = [{module: 'env', name: 'memory', kind: 'memory'}, {module: 'ring3', name: 'guard_resident', kind: 'function'}, ...reads.map(name => ({module: 'ring3', name, kind: 'function'}))];
  assert.deepEqual(WebAssembly.Module.imports(module), expected, 'exact direct guard/read imports, no store'); assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  const imports = {guard_resident: engine.api.guard_resident, ...Object.fromEntries(reads.map(name => [name, engine.api[name]]))};
  const instance = new WebAssembly.Instance(module, {env: {memory: engine.memory}, ring3: imports}); assert.equal(instance.exports.run.length, 4);
  const filename = `unit-${++stats.compiled_units}.wasm`; writeFileSync(join(outputDir, filename), selected.bytes); artifacts[filename] = hash(selected.bytes);
  const unit = {...selected, module, instance, run: instance.exports.run, blocks, reads, filename, label}; units.push(unit); return unit;
}
function select(engine, pc, unit) {
  const before = arena(engine); assert.equal(engine.api.find_resident(pc), 0); const selected = record(engine, before, 'exact-PC selection');
  assert.equal(selected.id, unit.id); assert.equal(selected.pointer, unit.pointer); assert.deepEqual(selected.bytes, unit.bytes); stats.find_success++;
}
function unchanged(engine, action, status, label) {
  const before = arena(engine); assert.equal(action(), status, label); assert.deepEqual(arena(engine), before, `${label}: full arena preserved`); stats.full_arena_checks++; noLegacy(engine);
}
function guard(engine, unit, status = 0) {
  unchanged(engine, () => engine.api.guard_resident(engine.low, engine.high, unit.low, unit.high, engine.base, engine.base + 56, engine.base + 96), status, 'direct resident guard'); stats[status === 0 ? 'successful_guards' : 'failed_guards']++;
}
function rejected(engine, unit, status, label) {
  unchanged(engine, () => unit.run(engine.base, engine.base + 56, 4, engine.base + 96), status, label); stats.rejected_runs++;
}
function initial(eip) {
  return {registers: [0x7fffffff, 0x13579bdf, 0x23456789, 0x3456789a, DATA, 0x56789abc, DATA, 15], eip, eflags: 0xcd7};
}
function copy(value) { return {...value, registers: [...value.registers]}; }
function reset(engine, value, cancel = 0) {
  refresh(engine); header(engine.bytes, engine.view, engine.base, 'R3ST', 56);
  value.registers.forEach((r, i) => engine.view.setUint32(engine.base + 16 + i * 4, r, true)); engine.view.setUint32(engine.base + 48, value.eip, true); engine.view.setUint32(engine.base + 52, value.eflags, true);
  engine.bytes.fill(0xa5, engine.base + 56, engine.base + 96); engine.view.setUint32(engine.base + 96, cancel, true); engine.bytes.fill(0x5a, engine.base + 100, engine.base + 140);
}
function unary(kind, input, oldFlags) {
  const a = BigInt(input), increment = kind === 'inc', value = Number(BigInt.asUintN(32, a + (increment ? 1n : -1n)));
  let result = (oldFlags & 0x401) | 2;
  if (increment ? (a & 15n) === 15n : (a & 15n) === 0n) result |= 0x10;
  if (increment ? a === 0x7fffffffn : a === 0x80000000n) result |= 0x800;
  if (value === 0) result |= 0x40; if (value >= 0x80000000) result |= 0x80;
  let ones = 0; for (let byte = value & 255; byte; byte >>>= 1) ones += byte & 1; if (ones % 2 === 0) result |= 4;
  return {value, flags: result};
}
for (const [kind, input, oldFlags, value, flags] of [['inc', 15, 0xcd7, 16, 0x413], ['inc', 0xffffffff, 2, 0, 0x56], ['inc', 0x7fffffff, 0x403, 0x80000000, 0xc97], ['dec', 1, 0xcd7, 0, 0x447], ['dec', 0x80000000, 2, 0x7fffffff, 0x816]]) assert.deepEqual(unary(kind, input, oldFlags), {value, flags}, 'literal INC/DEC flag anchor');
function exit(reason, retired, detail = 0, address = 0, width = 0, version = 2) {
  const bytes = new Uint8Array(40), view = new DataView(bytes.buffer); header(bytes, view, 0, 'R3EX', 40, version);
  [reason, retired, detail, address, reason === 5 ? 1 : 0, width].forEach((v, i) => view.setUint32(16 + i * 4, v, true)); return bytes;
}
function helper(width, value, detail = 0, address = 0) {
  const bytes = new Uint8Array(40), view = new DataView(bytes.buffer); header(bytes, view, 0, 'R3MH', 40, width === 4 ? 1 : 2);
  [detail ? 1 : 0, detail ? 0 : value, detail, address, detail ? 1 : 0, width === 4 && detail === 0 ? 0 : width].forEach((v, i) => view.setUint32(16 + i * 4, v, true)); return bytes;
}
function run(engine, unit, budget, expected, expectedExit, expectedHelper, label, continuation, reads = 0) {
  const before = arena(engine); assert.equal(unit.run(engine.base, engine.base + 56, budget, engine.base + 96), 0, label);
  const wanted = before.slice(), view = new DataView(wanted.buffer);
  expected.registers.forEach((r, i) => view.setUint32(16 + i * 4, r, true)); view.setUint32(48, expected.eip, true); view.setUint32(52, expected.eflags, true); wanted.set(expectedExit, 56);
  if (expectedHelper !== undefined) wanted.set(expectedHelper, 100);
  assert.deepEqual(arena(engine), wanted, `${label}: all4236 state/exit/helper/cancel/transfer bytes`); stats.canonical_runs++; stats.full_arena_checks++; noLegacy(engine);
  if (continuation === 'initial') stats.continuation_initial_calls++; else if (continuation === 'resume') stats.continuation_resumes++;
  stats.guest_read32 += reads;
}
function word(engine, address, value) { assert.equal(engine.api.write32(address, value), 0); stats.host_writes++; refresh(engine); }
function observe(engine, address, width, expected) {
  const before = arena(engine); assert.equal(engine.api[`read${width * 8}`](address), 0); stats.host_reads++;
  const wanted = before.slice(); wanted.set(helper(width, expected), 100); assert.deepEqual(arena(engine), wanted, 'host observation only replaces helper40'); stats.full_arena_checks++;
}
function miss(engine, pc, status = 17) { unchanged(engine, () => engine.api.find_resident(pc), status, 'finder failure preserves arena'); stats.find_failure++; }
function prefix(start) { const value = copy(start), result = unary('inc', start.registers[7], start.eflags); value.registers[7] = result.value; value.eflags = result.flags; value.eip = 0x1001; return value; }
function at(start, pc) { const value = copy(start); value.eip = pc; return value; }
function loopStart(n, edi = 15, flags = 0xcd7) { const value = initial(0x1000); value.registers[0] = 0x1000; value.registers[1] = n; value.registers[3] = DATA; value.registers[7] = edi; value.eflags = flags; return value; }
function trace(start, retired) {
  const n = start.registers[1], limit = 5 * n - 1; assert.ok(n > 0 && retired >= 0 && retired <= limit);
  const value = copy(start), cycles = Math.floor(retired / 5), phase = retired % 5;
  value.registers[7] = Number(BigInt.asUintN(32, BigInt(start.registers[7]) + BigInt(cycles + (phase >= 1 ? 1 : 0))));
  value.registers[1] = n - cycles - (phase >= 3 ? 1 : 0);
  value.eip = phase === 0 ? 0x1000 : phase === 1 ? 0x1001 : phase === 2 ? 0x2000 : phase === 3 ? 0x2001 : value.registers[1] === 0 ? 0x2007 : 0x1100;
  if (phase === 1 || phase === 2) value.eflags = unary('inc', Number(BigInt.asUintN(32, BigInt(start.registers[7]) + BigInt(cycles))), start.eflags).flags;
  else if (retired > 0) value.eflags = unary('dec', n - cycles + (phase === 0 ? 1 : 0), start.eflags).flags;
  return value;
}
const authored = {a_memory: Buffer.from('47ff23', 'hex'), a_register: Buffer.from('ffe0', 'hex'), b: Buffer.from('490f85f9f0ffff', 'hex'), esp: Buffer.from('ffe4', 'hex')};
const engine = fresh(); for (const pc of [0, 0x1000, 0xffffffff]) miss(engine, pc);
for (const pc of [0x1000, 0x2000, 0x3000]) assert.equal(engine.api.map(pc, 1, 7), 0);
for (const [name, pc] of [['a_memory', 0x1000], ['a_register', 0x1100], ['b', 0x2000], ['esp', 0x3000]]) { upload(engine, pc, authored[name]); const filename = `${name}.x86`; writeFileSync(join(outputDir, filename), authored[name]); artifacts[filename] = hash(authored[name]); }
assert.equal(engine.api.map(DATA, 2, 3), 0);
const a = compile(engine, [[0x1000, 3], [0x1100, 2]], ['read32'], 'mixed memory/register A'), b = compile(engine, [[0x2000, 7]], [], 'register-only B'), r = compile(engine, [[0x3000, 2]], [], 'pure JMP ESP');
assert.ok(a.id !== b.id && b.id !== r.id && a.id !== r.id);
for (const [pc, unit] of [[0x1000, a], [0x1001, a], [0x1100, a], [0x2000, b], [0x2001, b], [0x3000, r]]) select(engine, pc, unit);
for (const pc of [0x1002, 0x1101, 0x2002, 0x3001, 0x2007, DATA, 0, 0xffffffff]) miss(engine, pc);
const targetRows = [
  {target: 0x1000, budget: 2, retired: 2, reason: 1, pc: 0x1001, increment: true, unit: a},
  {target: 0x1001, budget: 1, retired: 1, reason: 1, pc: 0x1001, unit: a},
  {target: 0x1100, budget: 3, retired: 2, reason: 3, pc: 0x4000, unit: a},
  ...[0x1002, 0x1101, 0x2000, 0x2001, 0x2002, 0x3000, DATA, 0x4000, 0, 0xffffffff].map(target => ({target, budget: 2, retired: 1, reason: 3, pc: target, unit: target === 0x2000 || target === 0x2001 ? b : target === 0x3000 ? r : undefined})),
];
for (const row of targetRows) {
  word(engine, DATA, row.target); const start = loopStart(2); start.eip = 0x1001; start.registers[0] = 0x4000; reset(engine, start);
  const expected = row.increment ? prefix(start) : copy(start); expected.eip = row.pc;
  run(engine, a, row.budget, expected, exit(row.reason, row.retired), helper(4, row.target), 'numeric target uses exact local start or NeedCode without target fetch', 'initial', 1);
  if (row.unit) select(engine, row.target, row.unit); else miss(engine, row.target); observe(engine, DATA, 4, row.target); stats.target_cases++;
}
for (const target of [0, 0xffffffff, 0x2000, 0x3001, 0x3000]) {
  const start = initial(0x3000); start.registers[4] = target; reset(engine, start); const self = target === 0x3000;
  run(engine, r, self ? 1 : 2, at(start, target), exit(self ? 1 : 3, 1, 0, 0, 0, 1), undefined, 'JMP ESP preserves GPR/flags and helper with Exitv1', 'initial'); stats.target_cases++;
  if (target === 0x2000) select(engine, target, b); else if (self) select(engine, target, r); else miss(engine, target);
}
for (const budget of [0, 1]) { const start = initial(0x1100); start.registers[0] = 0x2000; reset(engine, start); run(engine, a, budget, at(start, budget ? 0x2000 : 0x1100), exit(1, budget), undefined, 'mixed A register entry retains Exitv2 and makes no helper call', 'initial'); }
const pure = initial(0x3000); pure.registers[4] = 0x2000; reset(engine, pure); run(engine, r, 0, pure, exit(1, 0, 0, 0, 0, 1), undefined, 'pure register zero budget Exitv1', 'initial');
reset(engine, pure, 1); run(engine, r, 0, pure, exit(2, 0, 0, 0, 0, 1), undefined, 'pure register cancel outranks zero budget', 'initial'); stats.cancellations++;

const algorithmRows = [];
for (const [n, edi, oldFlags] of [[1, 0xfffffffe, 0x403], [3, 0x7ffffffe, 0xcd7], [5, 15, 3]]) {
  word(engine, DATA, 0x2000); const start = loopStart(n, edi, oldFlags); reset(engine, start); const limit = 5 * n - 1; let retired = 0, calls = 0;
  while (retired < limit) {
    const unit = trace(start, retired).eip === 0x2000 ? b : a, count = unit === b ? 2 : retired === 0 ? 2 : 3; retired += count;
    run(engine, unit, limit + 1, trace(start, retired), exit(3, count, 0, 0, 0, unit === b ? 1 : 2), unit === a ? helper(4, 0x2000) : undefined, 'bounded two-unit loop agrees with closed-form five-phase arithmetic', calls++ === 0 ? 'initial' : 'resume', unit === a ? 1 : 0);
  }
  assert.equal(retired, limit); assert.equal(trace(start, retired).registers[1], 0); assert.equal(trace(start, retired).eip, 0x2007);
  algorithmRows.push({n, edi, old_flags: oldFlags, retired, calls, final_edi: trace(start, retired).registers[7], final_flags: trace(start, retired).eflags}); stats.algorithm_cases++; stats.algorithm_retired += retired; observe(engine, DATA, 4, 0x2000);
}
const split = loopStart(2); word(engine, DATA, 0x2000); reset(engine, split);
run(engine, a, 0, trace(split, 0), exit(1, 0), undefined, 'zero budget precedes target read', 'initial');
run(engine, a, 1, trace(split, 1), exit(1, 1), undefined, 'prefix commits once before exact interior JMP', 'resume'); select(engine, 0x1001, a);
engine.view.setUint32(engine.base + 96, 1, true); run(engine, a, 0, trace(split, 1), exit(2, 0), undefined, 'between-resume cancel precedes budget and read', 'resume'); stats.cancellations++; engine.view.setUint32(engine.base + 96, 0, true);
run(engine, a, 1, trace(split, 2), exit(1, 1), helper(4, 0x2000), 'exact JMP budget commits target before NeedCode', 'resume', 1);
word(engine, DATA, 0xffffffff); run(engine, a, 4, trace(split, 2), exit(3, 0), undefined, 'external continuation never rereads retired JMP pointer', 'resume'); select(engine, 0x2000, b);
run(engine, b, 1, trace(split, 3), exit(1, 1, 0, 0, 0, 1), undefined, 'B DEC exact budget', 'resume'); run(engine, b, 1, trace(split, 4), exit(1, 1, 0, 0, 0, 1), undefined, 'B JNZ selects mixed A register entry', 'resume'); select(engine, 0x1100, a);
run(engine, a, 1, trace(split, 5), exit(1, 1), undefined, 'register JMP interior continuation never reads RAM', 'resume'); word(engine, DATA, 0x2000);
run(engine, a, 2, trace(split, 7), exit(1, 2), helper(4, 0x2000), 'next iteration reads current pointer', 'resume', 1); run(engine, b, 2, trace(split, 9), exit(1, 2, 0, 0, 0, 1), undefined, 'final B exact budget', 'resume'); run(engine, b, 1, trace(split, 9), exit(3, 0, 0, 0, 0, 1), undefined, 'completed B repeats no instruction', 'resume');
const preset = loopStart(1); reset(engine, preset, 1); run(engine, a, 9, preset, exit(2, 0), undefined, 'preset cancel precedes INC and target read', 'initial'); stats.cancellations++;

assert.equal(engine.api.map(0x8000, 1, 3), 0); word(engine, 0x8000, 0x11111111); assert.equal(engine.api.protect(0x8000, 1, 4), 0); word(engine, 0x6ffc, 0x11223344);
const faultRows = [{address: 0x9000, at: 0x9000, detail: 1, repair: 'map', target: 0x2000}, {address: 0x8000, at: 0x8000, detail: 2, repair: 'permission', target: 0x1100}, {address: 0x6fff, at: 0x7000, detail: 1, repair: 'cross', target: 0x2001}, {address: 0xffffffff, at: 0xffffffff, detail: 3}];
for (const fault of faultRows) {
  const start = loopStart(2); start.registers[3] = fault.address; start.registers[0] = 0x4000; const stopped = prefix(start); reset(engine, start);
  run(engine, a, 4, stopped, exit(5, 1, fault.detail, fault.at, 4), helper(4, 0, fault.detail, fault.at), 'target read fault commits only INC prefix', 'initial', 1); stats.faults++;
  if (fault.repair) {
    if (fault.repair === 'permission') assert.equal(engine.api.protect(fault.address, 1, 3), 0); else assert.equal(engine.api.map(fault.repair === 'cross' ? 0x7000 : fault.address, 1, 3), 0);
    word(engine, fault.address, fault.target); if (fault.repair === 'permission') assert.equal(engine.api.protect(fault.address, 1, 1), 0);
    const registerReentry = fault.target === 0x1100, expected = at(stopped, registerReentry ? start.registers[0] : fault.target);
    run(engine, a, 4, expected, exit(3, registerReentry ? 2 : 1), helper(4, fault.target), 'same CPU retries repaired current target without repeating prefix', 'resume', 1); stats.repairs++; observe(engine, fault.address, 4, fault.target);
  } else { run(engine, a, 4, stopped, exit(5, 0, fault.detail, fault.at, 4), helper(4, 0, fault.detail, fault.at), 'overflow retry has zero retirement', 'resume', 1); stats.faults++; }
  guard(engine, a); guard(engine, b);
}
assert.equal(engine.api.map(0xfffff000, 1, 3), 0); word(engine, 0xfffffffc, 0xffffffff); const top = loopStart(1); top.registers[3] = 0xfffffffc; reset(engine, top);
run(engine, a, 4, at(prefix(top), 0xffffffff), exit(3, 2), helper(4, 0xffffffff), 'last valid source word reads maximum numeric target', 'initial', 1); observe(engine, 0xfffffffc, 4, 0xffffffff);
observe(engine, 0x6ffc, 4, 0x01223344); observe(engine, 0x7000, 4, 0x00000020);

word(engine, DATA, 0x2000); guard(engine, a); guard(engine, b); guard(engine, r); select(engine, 0x2000, b);
word(engine, 0x1000, 0x0023ff47); guard(engine, a, 4); guard(engine, b); guard(engine, r); rejected(engine, a, 4, 'stale A rejects before target read'); miss(engine, 0x1001, 4);
const copied = new WebAssembly.Instance(new WebAssembly.Module(a.bytes.slice()), {env: {memory: engine.memory}, ring3: {guard_resident: engine.api.guard_resident, read32: engine.api.read32}}); rejected(engine, {...a, run: copied.exports.run}, 4, 'copied stale A retains baked ID');
const independentB = loopStart(1); independentB.eip = 0x2000; reset(engine, independentB); const doneB = copy(independentB), dec = unary('dec', 1, independentB.eflags); doneB.registers[1] = dec.value; doneB.eflags = dec.flags; doneB.eip = 0x2007;
run(engine, b, 3, doneB, exit(3, 2, 0, 0, 0, 1), undefined, 'current B remains executable after A invalidation', 'initial');
const freshA = compile(engine, a.blocks, ['read32'], 'fresh same-PC A'); assert.ok(freshA.id > a.id && freshA.id > r.id); select(engine, 0x1000, freshA); select(engine, 0x1001, freshA); select(engine, 0x1100, freshA); select(engine, 0x2000, b); guard(engine, a, 4); guard(engine, freshA);
const replacement = loopStart(1); reset(engine, replacement); run(engine, freshA, 3, trace(replacement, 2), exit(3, 2), helper(4, 0x2000), 'fresh A transfers under new ID', 'initial', 1); run(engine, b, 3, trace(replacement, 4), exit(3, 2, 0, 0, 0, 1), undefined, 'B true continuation survives A replacement', 'resume');
const beforeClose = arena(engine); assert.equal(engine.api.close(), 0); assert.deepEqual(arena(engine), beforeClose); stats.full_arena_checks++;
for (const unit of [freshA, b, r]) rejected(engine, unit, 5, 'closed unit rejects before CPU and helper');

assert.equal(stats.algorithm_cases, 3); assert.equal(stats.algorithm_retired, 42); assert.equal(stats.target_cases, 18); assert.equal(stats.compiled_units, 4);
const productionPaths = execFileSync('rg', ['--files', 'engine/src'], {cwd: root, encoding: 'utf8'}).trim().split('\n').sort();
const sourcePaths = ['engine/tests/process_resident_indirect_wasm.rs', 'engine/tests/fixtures/p2-resident-indirect/run.mjs', 'engine/tests/process_resident_indirect.rs', 'engine/tests/process_resident_memory_wasm.rs', 'engine/tests/fixtures/p2-resident-memory/run.mjs', 'engine/tests/process_resident_memory.rs', 'engine/tests/process_resident_wasm.rs', 'engine/tests/fixtures/p2-resident-process/run.mjs', 'engine/tests/process_resident.rs', 'Cargo.toml', 'engine/Cargo.toml', 'Cargo.lock', ...productionPaths];
const provenance = {
  stats, actual_resident_runs: stats.canonical_runs + stats.rejected_runs, actual_resident_guard_entries: stats.canonical_runs + stats.rejected_runs + stats.successful_guards + stats.failed_guards, guest_helper_calls_from_authored_execution: stats.guest_read32, host_observation_reads: stats.host_reads,
  engine_sha256: hash(engineBytes), artifact_sha256: artifacts, module_set_sha256: hash(Object.keys(artifacts).filter(name => name.endsWith('.wasm')).sort().map(name => `${name}:${artifacts[name]}`).join('\n')),
  sources: Object.fromEntries(sourcePaths.map(path => [path, hash(readFileSync(join(root, path)))])), production: {git_head: execFileSync('git', ['rev-parse', 'HEAD'], {cwd: root, encoding: 'utf8'}).trim(), source_files: productionPaths.length, source_set_sha256: hash(productionPaths.map(path => `${path}:${hash(readFileSync(join(root, path)))}`).join('\n'))},
  authored: Object.fromEntries(Object.entries(authored).map(([name, bytes]) => [name, bytes.toString('hex')])), unit_bindings: units.map(unit => ({label: unit.label, id: unit.id.toString(), pointer: unit.pointer, length: unit.length, filename: unit.filename, reads: unit.reads})), algorithm_rows: algorithmRows, metadata_observations: metadata,
  tools: {node: process.version, v8: process.versions.v8, platform: process.platform, architecture: process.arch, rustc: execFileSync('rustc', ['--version'], {encoding: 'utf8'}).trim(), cargo: execFileSync('cargo', ['--version'], {encoding: 'utf8'}).trim()},
  commands: {run: [process.execPath, process.argv[1], enginePath, outputDir, root], generate_and_run: ['cargo', 'test', '-p', 'ring3-engine', '--test', 'process_resident_indirect_wasm', '--', '--nocapture']},
  claim: 'actual Node engine-Wasm resident near indirect JMP r32/m32 through direct Wasm guard/read32 and exact same engine memory with no legacy artifact or store. Two mixed-A/register-B units plus pure ESP and fresh A replacement; independent literal/BigInt INC/DEC flags and bounded five-phase 5*n-1 arithmetic, exact numeric/local/interior/middle/external/0/MAX target and finder pairing, full4236 arena/metadata/Exitv1-v2/helperv1 oracles, live RAM source faults/repair, budget/cancel and current-CPU continuation without rereading retired pointer, stale/copied/current B/fresh/closed binding. Helper counts authored-path derived, not instrumentation. No ISA interpreter, table/dispatcher/installer, Store4/active SMC, callbacks, browser/async cancellation/concurrency/performance/game/full P2-V0 claim.',
};
writeFileSync(join(outputDir, 'provenance.json'), JSON.stringify(provenance, null, 2)); console.log(JSON.stringify({...provenance, sources: Object.keys(provenance.sources).length, metadata_observations: metadata.length, artifacts: outputDir}));
