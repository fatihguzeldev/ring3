import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {ARENA_SIZE} from '../../../../../support/wasm/arena.mjs';
import {readEngine} from '../../../../../support/wasm/engine.mjs';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root);
const engine = readEngine(enginePath);
const initial = readFileSync(join(output, 'initial-arena.bin'));
assert.equal(initial.length, ARENA_SIZE);
const T = 140, FP = 4236, SV = 4364, CODE = 0x1000, DATA = 0x5011;
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const hex = value => Buffer.from(value, 'hex');
const words = values => {
  const bytes = Buffer.alloc(values.length * 4);
  values.forEach((value, index) => bytes.writeUInt32LE(value >>> 0, index * 4));
  return bytes;
};
const pattern = (length, multiplier = 17, add = 23) =>
  Buffer.from(Array.from({length}, (_, index) => (index * multiplier + add) & 255));
const record = (magic, version, size, values) => {
  const bytes = Buffer.alloc(size);
  bytes.write(magic);
  bytes.writeUInt16LE(version, 4);
  bytes.writeUInt16LE(1, 6);
  bytes.writeUInt32LE(size, 8);
  values.forEach((value, index) => bytes.writeUInt32LE(value >>> 0, 16 + index * 4));
  return bytes;
};
const state = (registers, pc) => record('R3ST', 1, 56, [...registers, pc, 0xcd7]);
const exit = (reason, retired, detail = 0, address = 0, access = 0, length = 0) =>
  record('R3EX', 6, 40, [reason, retired, detail, address, access, length]);
const packet = (version, length = 0, status = 0, detail = 0, address = 0, access = 0, value = 0) =>
  record('R3MH', version, 40, [status, value, detail, address, access, length]);
const parents = address => [address, 0x21314151, 0x32425262, 0x43536373,
  0x7080, 0x65758595, 0x768696a6, 0x8797a7b7];
const raw = {
  4: {snan: hex('3412807f'), zero: hex('00000080'), denormal: hex('01000000'), infinity: hex('0000807f')},
  8: {snan: hex('341200000000f07f'), zero: hex('0000000000000080'),
    denormal: hex('0100000000000000'), infinity: hex('000000000000f07f')},
};
const rows = [4, 8].flatMap(width => {
  const prefix = width === 4 ? 'f3' : 'f2';
  return [
    {name: `mov${width === 4 ? 'ss' : 'sd'}-register-snan-upper-preserved`, width, code: prefix + '0f10c8', kind: 'register', value: raw[width].snan},
    {name: `mov${width === 4 ? 'ss' : 'sd'}-self-alias-signed-zero`, width, code: prefix + '0f10c0', kind: 'alias', value: raw[width].zero},
    {name: `mov${width === 4 ? 'ss' : 'sd'}-memory-denormal-upper-zero`, width, code: prefix + '0f1010', kind: 'read', value: raw[width].denormal},
    {name: `mov${width === 4 ? 'ss' : 'sd'}-memory-infinity-exact-width`, width, code: prefix + '0f1110', kind: 'write', value: raw[width].infinity},
  ];
});
const sources = ['engine/tests/cpu/simd/fixtures/p2-simd-scalar-move/runtime/run.mjs',
  'engine/tests/support/wasm/engine.mjs', 'engine/tests/support/wasm/arena.mjs',
  'engine/src/cpu/x86/decode/simd.rs', 'engine/src/cpu/dbt/wasm/simd.rs',
  'engine/src/cpu/dbt/wasm/simd/direct.rs', 'engine/src/process/memory/span.rs', 'engine/src/process/memory/mod.rs', 'engine/src/process/mod.rs'];
const sourceHashes = () => Object.fromEntries(sources.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourceBefore = sourceHashes();
const contexts = [], modules = [], observations = [], helpers = [], ramChecks = [], repairs = [];
const arena = context => Buffer.from(new Uint8Array(context.memory.buffer, context.base, ARENA_SIZE));
function input(context, offset, bytes) {
  const expected = arena(context);
  expected.set(bytes, offset);
  new Uint8Array(context.memory.buffer, context.base + offset, bytes.length).set(bytes);
  assert.deepEqual(arena(context), expected, 'declared arena input');
}
function host(context, name, args, changedSpan) {
  const expected = arena(context);
  assert.equal(context.api[name](...args), 0, name);
  if (changedSpan) expected.set(arena(context).subarray(...changedSpan), changedSpan[0]);
  assert.deepEqual(arena(context), expected, name + ' full arena');
}
function upload(context, address, bytes) {
  input(context, T, bytes);
  host(context, 'upload', [address, bytes.length]);
}
function checkRam(context, address, bytes, label) {
  for (let offset = 0; offset < bytes.length; offset++) {
    const expected = arena(context);
    assert.equal(context.api.read8(address + offset), 0, label);
    expected.set(packet(2, 1, 0, 0, 0, 0, bytes[offset]), 100);
    assert.deepEqual(arena(context), expected, label + ' byte ' + offset);
  }
  ramChecks.push({context: context.id, label, address, expected_hex: bytes.toString('hex')});
}
function dirtyFp() {
  const bytes = Buffer.from(initial.subarray(FP, FP + 128));
  bytes.writeUInt16LE(0x2800, 18);
  bytes.writeUInt32LE(0xaabbccdd, 24);
  bytes.writeUInt32LE(0x11223344, 28);
  for (let index = 0; index < 8; index++) {
    bytes.writeBigUInt64LE(0x8000000000000010n + BigInt(index), 40 + index * 10);
    bytes.writeUInt16LE(0x3fff, 48 + index * 10);
  }
  return bytes;
}
function seed(context, block, value, address = DATA) {
  input(context, 0, initial);
  const registers = parents(address);
  input(context, 0, state(registers, block.pc));
  input(context, 56, exit(1, 0));
  input(context, T, pattern(520, 29, 11));
  input(context, FP, dirtyFp());
  const simd = Buffer.from(initial.subarray(SV, SV + 160));
  // DAZ, FTZ, directed rounding and existing sticky flags cannot transform a move.
  simd.writeUInt32LE(0xdfc4, 16);
  for (let index = 0; index < 8; index++) pattern(16, 13, 7 + 19 * index).copy(simd, 32 + 16 * index);
  value.copy(simd, 32);
  value.copy(simd, 64);
  input(context, SV, simd);
  return registers;
}
function forward(context, name, args) {
  const before = arena(context), status = context.api[name](...args), after = arena(context);
  if (name.startsWith('guard')) {
    assert.equal(status, context.stale ? 4 : 0, 'canonical bound guard');
    assert.deepEqual(after, before, 'guard is arena-neutral');
    return status;
  }
  assert.ok(['cpu_read', 'cpu_write', 'cpu_write_resident'].includes(name), name);
  const payload = args.slice(name === 'cpu_write_resident' ? 4 : 0).map(value => value >>> 0);
  assert.deepEqual(payload, [context.access.address, context.access.value.length, 1]);
  const expected = Buffer.from(before), fault = context.access.fault;
  expected.set(packet(7, payload[1], fault ? 1 : 0, fault ? 2 : 0,
    fault ? 0x6000 : 0, fault ? (name === 'cpu_read' ? 1 : 2) : 0), 100);
  if (name === 'cpu_read' && !fault) expected.set(context.access.value, T);
  if (name !== 'cpu_read') assert.deepEqual(before.subarray(T, T + payload[1]), context.access.value);
  assert.equal(status, context.access.smc ? 11 : 0, 'canonical checked helper status');
  assert.deepEqual(after, expected, name + ' exact helper mutation');
  helpers.push({context: context.id, name, args: args.map(value => value >>> 0), status,
    packet_hex: after.subarray(100, 140).toString('hex')});
  return status;
}
function open(owner, entries) {
  const instance = new WebAssembly.Instance(engine.module, {});
  const context = {id: contexts.length + 1, owner, entries, memory: instance.exports.memory, api: {}};
  for (const [name, fn] of Object.entries(instance.exports)) {
    if (name.startsWith('ring3_abi_v1_') && typeof fn === 'function') context.api[name.slice(13)] = fn;
  }
  assert.equal(context.api.open(4, context.id, 0x5343414c), 0);
  context.base = context.api.arena_ptr() >>> 0;
  input(context, 0, initial);
  for (const [page, permission] of [[CODE, 7], [0x5000, 3], [0x6000, 3]]) host(context, 'map', [page, 1, permission]);
  context.blocks = rows.map((row, index) => ({...row, pc: CODE + index * 16, bytes: Buffer.concat([hex(row.code), hex('ebfe')])}));
  for (const block of context.blocks) upload(context, block.pc, block.bytes);
  input(context, T, words(context.blocks.flatMap(block => entries ? [block.pc] : [block.pc, block.bytes.length])));
  const compile = owner === 'resident' ? (entries ? 'compile_resident_entries' : 'compile_resident') : (entries ? 'compile_entries' : 'compile');
  host(context, compile, entries ? [context.blocks.length, 0] : [context.blocks.length], owner === 'resident' ? [T, T + 24] : undefined);
  const current = arena(context);
  context.binding = owner === 'resident' ? {id_low: current.readUInt32LE(T + 8), id_high: current.readUInt32LE(T + 12),
    pointer: current.readUInt32LE(T + 16), length: current.readUInt32LE(T + 20)} :
    {generation: context.api.generation(), pointer: context.api.module_ptr() >>> 0, length: context.api.module_len() >>> 0};
  const bytes = Buffer.from(new Uint8Array(context.memory.buffer, context.binding.pointer, context.binding.length));
  const module = new WebAssembly.Module(bytes), imports = WebAssembly.Module.imports(module), ring3 = {};
  assert.deepEqual(imports[0], {module: 'env', name: 'memory', kind: 'memory'});
  for (const item of imports.slice(1)) {
    assert.equal(item.module, 'ring3');
    assert.equal(item.kind, 'function');
    assert.ok(['guard', 'guard_resident', 'cpu_read', 'cpu_write', 'cpu_write_resident'].includes(item.name), 'scalar move has no execution helper: ' + item.name);
    ring3[item.name] = (...args) => forward(context, item.name, args);
  }
  assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  context.run = new WebAssembly.Instance(module, {env: {memory: context.memory}, ring3}).exports.run;
  assert.equal(context.run.length, 4);
  const file = `module-${context.id}.wasm`;
  writeFileSync(join(output, file), bytes, {flag: 'wx'});
  modules.push({context: context.id, owner, entries, file, sha256: hash(bytes), bytes: bytes.length, imports,
    blocks: context.blocks.map(block => ({pc: block.pc, length: block.bytes.length, instructions: 2})), ...context.binding});
  contexts.push({context: context.id, owner, entries, mapped_pages: 3});
  return context;
}
function run(context, block, label, {kind = block.kind, value = block.value, address = DATA,
  fault = false, smc = false, registers} = {}) {
  const before = arena(context), expected = Buffer.from(before), at = helpers.length;
  const retired = fault ? 0 : 1, reason = fault ? 5 : smc ? 6 : 1;
  expected.set(state(registers, block.pc + (retired ? 4 : 0)));
  expected.set(exit(reason, retired, fault ? 2 : 0, fault ? 0x6000 : 0,
    fault ? (kind === 'read' ? 1 : 2) : 0, fault ? value.length : 0), 56);
  if (kind === 'register') expected.set(value, SV + 48);
  if (kind === 'read' && !fault) { expected.fill(0, SV + 64, SV + 80); expected.set(value, SV + 64); }
  if (kind === 'read' || kind === 'write') {
    expected.set(packet(7, value.length, fault ? 1 : 0, fault ? 2 : 0,
      fault ? 0x6000 : 0, fault ? (kind === 'read' ? 1 : 2) : 0), 100);
    if (kind === 'write' || !fault) expected.set(value, T);
    context.access = {address, value, fault, smc};
  }
  assert.equal(context.run(context.base, context.base + 56, 1, context.base + 96), 0, label);
  const actual = arena(context);
  assert.deepEqual(actual, expected, label + ' complete arena');
  const events = helpers.slice(at);
  assert.equal(events.length, kind === 'read' || kind === 'write' ? 1 : 0, label + ' exact memory-only helper count');
  observations.push({context: context.id, label, status: 0, reason, retired,
    cpu_hex: actual.subarray(0, 56).toString('hex'), exit_hex: actual.subarray(56, 96).toString('hex'),
    arena_hex: actual.toString('hex'), helper_calls: events.length});
  delete context.access;
}
for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const context = open(owner, entries);
  for (const block of context.blocks) {
    const frame = pattern(20, 23, 3);
    if (block.kind === 'read') block.value.copy(frame, 4);
    upload(context, DATA - 4, frame);
    const registers = seed(context, block, block.value);
    run(context, block, block.name, {registers});
    if (block.kind === 'write') block.value.copy(frame, 4);
    if (block.kind === 'read' || block.kind === 'write') checkRam(context, DATA - 4, frame, block.name + ' data/canaries');
  }
  if (!entries) {
    const read = context.blocks[2], write = context.blocks[7];
    for (const [block, address, value, permission] of [[read, 0x5ffe, raw[4].snan, 2], [write, 0x5ffc, raw[8].snan, 1]]) {
      const frame = pattern(20, 19, 31), frameAddress = address - 4;
      if (block.kind === 'read') value.copy(frame, 4);
      upload(context, frameAddress, frame);
      const registers = seed(context, block, value, address);
      host(context, 'protect', [0x6000, 1, permission]);
      run(context, block, block.name + '-split-permission-fault', {registers, value, address, fault: true});
      if (block.kind === 'write') checkRam(context, frameAddress, frame, 'split write fault has no partial RAM commit');
      const stopped = arena(context);
      host(context, 'protect', [0x6000, 1, 3]);
      assert.deepEqual(arena(context), stopped, 'permission-only repair preserves stopped arena');
      run(context, block, block.name + '-protect-only-retry', {registers, value, address});
      if (block.kind === 'write') value.copy(frame, 4);
      checkRam(context, frameAddress, frame, 'split retry exact bytes and canaries');
      repairs.push({context: context.id, kind: block.kind, address, width: value.length,
        repair: 'protect only; no CPU/SIMD seed, RAM upload, remap or recapture'});
    }
    const block = context.blocks[7], value = raw[8].infinity;
    const registers = seed(context, block, value, CODE);
    run(context, block, 'scalar-store-commits-consumed-executable-page', {registers, value, address: CODE, smc: true});
    checkRam(context, CODE, value, 'committed executable scalar bytes');
    const stopped = arena(context), at = helpers.length;
    context.stale = true;
    assert.equal(context.run(context.base, context.base + 56, 1, context.base + 96), 4, 'consumed owner is stale after code write');
    assert.deepEqual(arena(context), stopped, 'stale owner refusal is complete-arena neutral');
    assert.equal(helpers.length, at, 'stale owner runs no memory helper');
    observations.push({context: context.id, label: 'consumed-owner-stale-neutral', status: 4,
      reason: stopped.readUInt32LE(72), retired: 0, retained_exit_retired: stopped.readUInt32LE(76),
      arena_hex: stopped.toString('hex'), helper_calls: 0});
  }
  writeFileSync(join(output, `arena-${context.id}.bin`), arena(context), {flag: 'wx'});
  host(context, 'close', []);
}
assert.deepEqual(sourceHashes(), sourceBefore);
const counts = {contexts: contexts.length, modules: modules.length, runs: observations.length,
  retired: observations.reduce((sum, row) => sum + row.retired, 0), helpers: helpers.length,
  read_faults: repairs.filter(row => row.kind === 'read').length,
  write_faults: repairs.filter(row => row.kind === 'write').length, protect_only_retries: repairs.length,
  executable_writes: observations.filter(row => row.label === 'scalar-store-commits-consumed-executable-page').length,
  stale_refusals: observations.filter(row => row.status === 4).length};
assert.deepEqual(counts, {contexts: 4, modules: 4, runs: 44, retired: 38, helpers: 26,
  read_faults: 2, write_faults: 2, protect_only_retries: 4, executable_writes: 2, stale_refusals: 2});
const result = {status: 'PASS', engine: {path: enginePath, sha256: engine.sha256, bytes: engine.bytes.length},
  arena_bytes: ARENA_SIZE, initial_arena_sha256: hash(initial), source_sha256: sourceBefore,
  counts, contexts, modules, observations, helpers, selected_ram: ramChecks, repairs,
  scope: 'authored scalar MOVSS/MOVSD raw-bit witnesses; four bound profiles for register merge/alias/load/store, explicit replacement/resident cross-page fault/retry and committed executable write; exact full arenas including scratch and unchanged FLAGS/MXCSR/x87; only canonical cpu_read/write imports and calls, no simd_execute; selected RAM/canaries only, no whole RAM, hardware numeric or performance claim'};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2) + '\n', {flag: 'wx'});
console.log(JSON.stringify({status: result.status, output, ...counts}));
