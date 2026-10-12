import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../../../../../support/wasm/engine.mjs';

const [enginePath, output, root] = process.argv.slice(2), SIZE = 4612, DATA = 0x5010, FAULT = 0x1705;
assert.ok(enginePath && output && root);
const engine = readEngine(enginePath), initial = readFileSync(join(output, 'initial-arena.bin'));
assert.equal(initial.length, SIZE);
const hash = b => createHash('sha256').update(b).digest('hex');
const sourcePaths = ['engine/src/memory/space.rs', 'engine/src/abi/memory_helper.rs', 'engine/src/abi/arena.rs',
  'engine/src/abi/wasm/exports/calls.rs', 'engine/src/abi/wasm/exports/cpu.rs', 'engine/src/abi/wasm/exports/execution.rs', 'engine/src/abi/wasm/exports/image.rs', 'engine/src/abi/wasm/exports/instance.rs', 'engine/src/abi/wasm/exports/memory.rs', 'engine/src/abi/wasm/exports/mod.rs', 'engine/src/process/instance.rs', 'engine/src/process/execution/compiled.rs', 'engine/src/process/execution/authority.rs', 'engine/src/process/memory/address_space.rs', 'engine/src/process/memory/scalar.rs', 'engine/src/process/memory/scalar_tests.rs', 'engine/src/process/execution/mod.rs', 'engine/src/process/memory/mod.rs', 'engine/src/process/mod.rs', 'engine/src/process/wasm/mod.rs', 'engine/src/process/wasm/loading.rs', 'engine/src/process/wasm/execution.rs', 'engine/src/process/wasm/calls.rs', 'engine/src/process/wasm/memory.rs', 'engine/src/process/wasm/cpu.rs', 'engine/src/process/mod.rs', 'engine/src/process/execution/resident.rs', 'engine/src/process/execution/authority.rs', 'engine/src/process/execution/mod.rs', 'engine/src/process/mod.rs',
  'engine/src/cpu/x86/ir.rs', 'engine/src/cpu/x86/decode/profile.rs', 'engine/src/cpu/x86/decode/lower.rs',
  'engine/src/cpu/dbt/region.rs', 'engine/src/cpu/dbt/wasm/emitter.rs', 'engine/src/cpu/dbt/wasm/integer/arithmetic.rs', 'engine/src/cpu/dbt/wasm/integer/bits.rs', 'engine/src/cpu/dbt/wasm/integer/carry_rotates.rs', 'engine/src/cpu/dbt/wasm/integer/decimal.rs', 'engine/src/cpu/dbt/wasm/integer/dispatch.rs', 'engine/src/cpu/dbt/wasm/integer/flags.rs', 'engine/src/cpu/dbt/wasm/integer/mod.rs', 'engine/src/cpu/dbt/wasm/integer/multiply_divide.rs', 'engine/src/cpu/dbt/wasm/integer/operands.rs', 'engine/src/cpu/dbt/wasm/integer/rotates.rs', 'engine/src/cpu/dbt/wasm/integer/shifts.rs',
  'engine/src/cpu/dbt/wasm/memory.rs', 'engine/src/cpu/dbt/wasm/memory/exchange.rs', 'engine/src/cpu/dbt/wasm/memory/byte_exchange.rs', 'engine/src/cpu/dbt/wasm/locals.rs',
  'engine/tests/process/memory/cases/native/engine_compare_exchange8_helpers.rs', 'engine/tests/process/memory/native.rs', 'engine/tests/process/memory/native.rs', 'engine/tests/cpu/atomic/cases/native/cpu_locked_cmpxchg8.rs', 'engine/tests/cpu/atomic/native.rs', 'engine/tests/support/cpu/prefix_admission.rs', 'engine/tests/cpu/atomic/native.rs', 'engine/tests/cpu/atomic/cases/wasm/cpu_locked_cmpxchg8_wasm.rs', 'engine/tests/cpu/atomic/wasm.rs', 'engine/tests/support/wasm.rs', 'engine/tests/cpu/atomic/wasm.rs', 'engine/tests/support/wasm.rs', 'engine/tests/cpu/atomic/cases/native/locked_cmpxchg8_original.rs', 'engine/tests/cpu/atomic/native.rs', 'engine/tests/cpu/atomic/native.rs',
  ...['runtime/run.mjs', 'kernel.c', 'build.py', 'kernel.obj', 'kernel.text.bin', 'kernel.disasm.txt', 'receipt.json', 'PROVENANCE.md']
    .map(p => 'engine/tests/cpu/atomic/fixtures/p2-locked-cmpxchg8/' + p),
  'engine/tests/programs/fixtures/p2-compiled-records/build.py', 'engine/tests/support/wasm/engine.mjs'];
const sourceHashes = () => Object.fromEntries(sourcePaths.map(p => [p, hash(readFileSync(join(root, p)))]));
const sourcesBefore = sourceHashes(), contexts = [], modules = [], observations = [], helperTrace = [], diagnostics = [], controls = [];
const counts = {contexts: 0, modules: 0, runs: 0, retired: 0, compares: 0, helper_calls: 0, read32: 0, read8: 0, store32: 0, store8: 0,
  sources: 0, flag_rows: 0, aliases: 0, boundary: 0, faults: 0, repair_only: 0, current_inputs: 0, synthetic: 0, smc: 0, compiler_calls: 0};
const word = n => {const b = Buffer.alloc(4); b.writeUInt32LE(n >>> 0); return b;};
const words = values => Buffer.concat(values.map(word));
function record(magic, version, size, fields) {
  const b = Buffer.alloc(size); b.write(magic); b.writeUInt16LE(version, 4); b.writeUInt16LE(1, 6); b.writeUInt32LE(size, 8);
  fields.forEach((v, i) => b.writeUInt32LE(v >>> 0, 16 + i * 4)); return b;
}
const state = s => record('R3ST', 1, 56, [...s.registers, s.pc, s.flags]);
const exit = (reason, retired, {detail = 0, address = 0, access = 0} = {}, version = 2) => record('R3EX', version, 40, [reason, retired, detail, address, access, access ? 1 : 0]);
const helper = (value = 0, {detail = 0, address = 0, access = 0} = {}) => record('R3MH', 5, 40, [access ? 1 : 0, value, detail, address, access, 1]);
const helper32 = value => record('R3MH', 1, 40, [0, value, 0, 0, 0, 0]);
const helper8 = value => record('R3MH', 2, 40, [0, value, 0, 0, 0, 1]);
const laneFactor = source => source < 4 ? 1 : 256;
const byteOf = (parent, source) => Math.floor(parent / laneFactor(source)) % 256;
const mergeByte = (parent, source, value) => parent - byteOf(parent, source) * laneFactor(source) + value * laneFactor(source);
const setSource = (registers, source, value) => {registers[source % 4] = mergeByte(registers[source % 4], source, value);};
const literalParents = [0xa1b2c300, 0xa1b2c37f, 0xa1b2c380, 0xa1b2c3ff, 0xa1b200d4, 0xa1b27fd4, 0xa1b280d4, 0xa1b2ffd4];
for (const [source, value] of [0, 127, 128, 255, 0, 127, 128, 255].entries()) {
  assert.equal(mergeByte(0xa1b2c3d4, source, value), literalParents[source]);
  assert.equal(byteOf(0xa1b2c3d4, source), source < 4 ? 0xd4 : 0xc3);
}
const parents = () => Array.from({length: 8}, (_, i) => (0xa100 + i) * 65536 + 0x1234 + i);
const FLAGS = Array.from({length: 128}, (_, n) => [0, 2, 4, 6, 7, 10, 11].reduce((f, bit, i) => f + ((n >>> i & 1) * 2 ** bit), 2));
assert.equal(new Set(FLAGS).size, 128);

const kernel = readFileSync(join(root, 'engine/tests/cpu/atomic/fixtures/p2-locked-cmpxchg8/kernel.text.bin'));
assert.equal(kernel.toString('hex'), '568b54240c8a6424108b7424088a0231c9f00fb0260f94c47402880288e189c85ec3');
assert.equal(hash(kernel), '1a0c6a5c52530f6aa5b2781a5dab5bff28f2d8330ca257a7b478de669a93115f');
const normal = Array.from({length: 8}, (_, source) => ({pc: 0x1000 + source * 16,
  bytes: Buffer.from([0xf0, 0x0f, 0xb0, source << 3 | 5, 0x10, 0x50, 0, 0, 0xeb, 0]), instructions: 2, source, next: 0x1008 + source * 16}));
const aliasDefinitions = [
  {bytes: [0xf0, 0x0f, 0xb0, 0x20], source: 4, base: 0, set: {0: DATA}},
  {bytes: [0xf0, 0x0f, 0xb0, 0x29], source: 5, base: 1, set: {1: DATA}},
  {bytes: [0xf0, 0x0f, 0xb0, 0x24, 0x24], source: 4, base: 4, set: {4: DATA}},
  {bytes: [0xf0, 0x0f, 0xb0, 0x6d, 0], source: 5, base: 5, set: {5: DATA}},
  {bytes: [0xf0, 0x0f, 0xb0, 0x54, 0xbe, 0xf0], source: 2, base: 6, index: 7, scale: 4, displacement: -16, set: {6: 0x5000, 7: 8}},
  {bytes: [0xf0, 0x0f, 0xb0, 0x5c, 0x9b, 1], source: 3, base: 3, index: 3, scale: 4, displacement: 1, set: {3: 0x1003}},
  {bytes: [0xf0, 0x0f, 0xb0, 0x35, 0x10, 0x50, 0, 0], source: 6, displacement: DATA, set: {}},
  {bytes: [0xf0, 0x0f, 0xb0, 0x34, 0xf5, 0x10, 0x50, 0, 0], source: 6, index: 6, scale: 8, displacement: DATA, set: {6: 0x20000000}},
  {bytes: [0xf0, 0x0f, 0xb0, 0x2c, 0x48], source: 5, base: 0, index: 1, scale: 2, set: {0: 0x5000, 1: 8}},
  {bytes: [0xf0, 0x0f, 0xb0, 0x64, 0x84, 0x7f], source: 4, base: 4, index: 0, scale: 4, displacement: 127, set: {0: 1, 4: 0x4f8d}},
  {bytes: [0xf0, 0x0f, 0xb0, 0xbf, 0xf0, 0xff, 0xff, 0xff], source: 7, base: 7, displacement: -16, set: {7: 0x5020}},
  {bytes: [0xf0, 0x0f, 0xb0, 0x7c, 0x1d, 0], source: 7, base: 5, index: 3, scale: 1, set: {5: 0x5000, 3: 16}},
  {bytes: [0xf0, 0x0f, 0xb0, 0x84, 0xc0, 0x10, 0x50, 0, 0xe0], source: 0, base: 0, index: 0, scale: 8, displacement: 0xe0005010, set: {0: 0x20000000}},
];
const modulo32 = n => Number((n % 4294967296n + 4294967296n) % 4294967296n);
function addressOf(row, registers) {return modulo32(BigInt(row.displacement ?? 0) + (row.base === undefined ? 0n : BigInt(registers[row.base])) + (row.index === undefined ? 0n : BigInt(registers[row.index]) * BigInt(row.scale ?? 1)));}
const aliasRows = [], aliases = [];
for (let first = 0; first < aliasDefinitions.length; first += 4) {
  const pc = 0x1400 + aliases.length * 64; let at = pc; const bytes = [];
  for (const definition of aliasDefinitions.slice(first, first + 4)) {
    aliasRows.push({...definition, pc: at, next: at + definition.bytes.length}); bytes.push(...definition.bytes); at += definition.bytes.length;
  }
  bytes.push(0xeb, 0); aliases.push({pc, bytes: Buffer.from(bytes), instructions: Math.min(4, aliasDefinitions.length - first) + 1});
}
const faultBlock = {pc: 0x1700, bytes: Buffer.from([0xbb, 0xbe, 0xba, 0xfe, 0xca, 0xf0, 0x0f, 0xb0, 0x0e, 0xeb, 0]), instructions: 3};
const live = {pc: 0x1800, bytes: Buffer.from([0xf0, 0x0f, 0xb0, 0x0e, 0x14, 0, 0x0f, 0x92, 0xc3, 0x0f, 0x90, 0xc7, 0xeb, 0]), instructions: 5};
const caller = [
  {pc: 0x3000, bytes: Buffer.concat([Buffer.from([0x68]), word(0xa1b2c3d3), Buffer.from([0x68]), word(0x5020), Buffer.from([0x68]), word(DATA), Buffer.from([0xe8]), word(0x2000 - 0x3014)]), instructions: 4},
  {pc: 0x3014, bytes: Buffer.concat([Buffer.from([0x83, 0xc4, 12, 0x89, 0xc5, 0x8d, 0x80]), word(0xa1b2c3d3), Buffer.from([0x50, 0x68]), word(0x5020), Buffer.from([0x68]), word(DATA), Buffer.from([0xe8]), word(0x2000 - 0x302f)]), instructions: 7},
  {pc: 0x302f, bytes: Buffer.concat([Buffer.from([0x83, 0xc4, 12, 0xc1, 0xe0, 1, 0x09, 0xe8, 0xa3]), word(0x5030), Buffer.from([0xeb, 0xfe])]), instructions: 5},
];
const kernelBlocks = [{pc: 0x2000, bytes: kernel.subarray(0, 26), instructions: 9},
  {pc: 0x201a, bytes: kernel.subarray(26, 28), instructions: 1}, {pc: 0x201c, bytes: kernel.subarray(28), instructions: 4}];
assert.deepEqual(caller.map(b => b.pc + b.bytes.length), [0x3014, 0x302f, 0x303e]);
function cmpFlags(expected, old, flags) {
  const a = BigInt(expected), b = BigInt(old), result = Number((a - b + 256n) % 256n);
  let parity = 0; for (let bit = 0; bit < 8; bit++) parity += result >>> bit & 1;
  return (flags & 0x400) | 2 | (a < b ? 1 : 0) | (parity % 2 === 0 ? 4 : 0) |
    ((expected & 15) < (old & 15) ? 16 : 0) | (result === 0 ? 64 : 0) | (result >= 128 ? 128 : 0) |
    (((expected ^ old) & (expected ^ result) & 128) !== 0 ? 0x800 : 0);
}
for (const [a,b,flags] of [[0,0,0x46],[0,1,0x97],[0x80,1,0x812],[0x7f,0xff,0x883],[0x34,0xa7,0x897]]) assert.equal(cmpFlags(a,b,2),flags);
const arena = c => Buffer.from(new Uint8Array(c.memory.buffer, c.base, SIZE));
const cpu = c => {const b = arena(c); return {registers: Array.from({length: 8}, (_, i) => b.readUInt32LE(16 + 4 * i)), pc: b.readUInt32LE(48), flags: b.readUInt32LE(52)};};
function input(c, offset, bytes) {const expected = arena(c); expected.set(bytes, offset); new Uint8Array(c.memory.buffer).set(bytes, c.base + offset); assert.deepEqual(arena(c), expected);}
function host(c, name, args, span) {const expected = arena(c); assert.equal(c.api[name](...args), 0, name); if (span) expected.set(arena(c).subarray(...span), span[0]); assert.deepEqual(arena(c), expected, 'host full4612 ' + name);}
function map(c, page, permission = 3) {host(c, 'map', [page, 1, permission]); c.pages.add(page); c.maxPages = Math.max(c.maxPages, c.pages.size); assert.ok(c.maxPages <= 12);}
function protect(c, page, permission) {host(c, 'protect', [page, 1, permission]);}
function unmap(c, page) {host(c, 'unmap', [page, 1]); c.pages.delete(page);}
function upload(c, address, bytes) {assert.ok(address >= 0 && address + bytes.length <= 4294967296); input(c, 140, bytes); host(c, 'upload', [address, bytes.length]);}
function seed(c, pc, flags = 0xcd7, registers = parents()) {input(c, 0, Buffer.concat([state({registers, pc, flags}), exit(1, 0), word(0), helper(0xad)])); return cpu(c);}
const arities = {open: 3, close: 0, arena_ptr: 0, map: 3, protect: 3, unmap: 2, upload: 2, read32: 1, read8: 1, store8: 2, store_resident8: 6,
  store32: 2, store_resident32: 6, compare_exchange8: 3, compare_exchange_resident8: 7, compile: 1, compile_entries: 2,
  compile_resident: 1, compile_resident_entries: 2, generation: 0, module_ptr: 0, module_len: 0, guard: 6, guard_resident: 7};

function open(owner, entries, purpose) {
  const instance = new WebAssembly.Instance(engine.module, {}), c = {id: ++counts.contexts, owner, entries, purpose, memory: instance.exports.memory, api: {}, high: 0x4341534c, captures: 0, pages: new Set(), maxPages: 0};
  for (const [name, arity] of Object.entries(arities)) {c.api[name] = instance.exports['ring3_abi_v1_' + name]; assert.equal(c.api[name]?.length, arity, name);}
  assert.equal(c.api.open(12, c.id, c.high), 0); c.base = c.api.arena_ptr() >>> 0; input(c, 0, initial);
  for (const [page, permission] of [[0, 3], [0x1000, 7], [0x2000, 7], [0x3000, 7], [0x5000, 3], [0x6000, 3], [0x7000, 3], [0xfffff000, 3]]) map(c, page, permission);
  for (const block of [...normal, ...aliases, faultBlock, live, ...caller]) upload(c, block.pc, block.bytes);
  upload(c, 0x2000, kernel); protect(c, 0x2000, 5); protect(c, 0x3000, 5);
  if (purpose.startsWith('smc')) {map(c, 0xc000, 7); upload(c, 0xc000, Buffer.from([0xf0,0x0f,0xb0,0x0e,0xba,0x7f,0x56,0x34,0x12,0xeb,0]));}
  return c;
}
function compile(c, name, blocks, helpers = ['compare']) {
  const instructionCount = blocks.reduce((n, b) => n + b.instructions, 0);
  assert.ok(blocks.length > 0 && blocks.length <= 8 && instructionCount <= 64); assert.ok(++c.captures <= 6);
  input(c, 140, words(blocks.flatMap(b => c.entries ? [b.pc] : [b.pc, b.bytes.length])));
  const method = c.owner === 'resident' ? (c.entries ? 'compile_resident_entries' : 'compile_resident') : (c.entries ? 'compile_entries' : 'compile');
  host(c, method, c.entries ? [blocks.length, 0] : [blocks.length], c.owner === 'resident' ? [140, 164] : null);
  const b = arena(c), binding = c.owner === 'resident' ? {low: b.readUInt32LE(148), high: b.readUInt32LE(152), pointer: b.readUInt32LE(156), length: b.readUInt32LE(160)} : {generation: c.api.generation(), pointer: c.api.module_ptr() >>> 0, length: c.api.module_len() >>> 0};
  const bytes = Buffer.from(new Uint8Array(c.memory.buffer, binding.pointer, binding.length)), module = new WebAssembly.Module(bytes), guard = c.owner === 'resident' ? 'guard_resident' : 'guard';
  const names = helpers.map(n => n === 'compare' ? (c.owner === 'resident' ? 'compare_exchange_resident8' : 'compare_exchange8') : n === 'store' ? (c.owner === 'resident' ? 'store_resident32' : 'store32') : n === 'store8' ? (c.owner === 'resident' ? 'store_resident8' : 'store8') : n);
  const imports = [{module: 'env', name: 'memory', kind: 'memory'}, ...[guard, ...names].map(name => ({module: 'ring3', name, kind: 'function'}))];
  assert.ok(bytes.length <= 65536); assert.deepEqual(WebAssembly.Module.imports(module), imports); assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  const ring3 = {...c.api};
  for (const name of names) ring3[name] = (...args) => {
    const before = arena(c), status = c.api[name](...args), after = arena(c), expected = Buffer.from(before); expected.set(after.subarray(100, 140), 100);
    assert.deepEqual(after, expected, 'canonical helper changes only its 40-byte record');
    const payload = args.slice(name.includes('resident') ? 4 : 0).map(v => v >>> 0);
    helperTrace.push({context: c.id, call: counts.runs + 1, name, address: payload[0], expected: name.startsWith('compare') ? payload[1] : null,
      input_value: name.startsWith('read') ? null : payload.at(-1), status, helper_hex: after.subarray(100, 140).toString('hex')});
    counts.helper_calls++; counts[name.startsWith('compare') ? 'compares' : name.startsWith('read') ? name : name.endsWith('8') ? 'store8' : 'store32']++; return status;
  };
  const run = new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3}).exports.run; assert.equal(run.length, 4);
  const file = `module-${++counts.modules}.wasm`; writeFileSync(join(output, file), bytes, {flag: 'wx'});
  const unit = {...binding, name, module, ring3, run, exitVersion: names.length ? 2 : 1};
  modules.push({context: c.id, owner: c.owner, entries: c.entries, name, file, bytes: bytes.length, sha256: hash(bytes), blocks: blocks.length, instructions: instructionCount, imports, ...binding}); return unit;
}
function run(c, unit, budget, next, label, {status = 0, reason = 1, retired = 1, packet, fault = {}, synthetic = false} = {}) {
  const before = arena(c), expected = Buffer.from(before), at = helperTrace.length;
  if (status === 0) {expected.set(state(next)); expected.set(exit(reason, retired, fault, unit.exitVersion), 56); if (packet) expected.set(packet, 100);}
  const observed = unit.run(c.base, c.base + 56, budget, c.base + 96); assert.equal(observed, status, label); assert.deepEqual(arena(c), expected, 'full4612 ' + label);
  const after = arena(c); observations.push({context: c.id, module: unit.name, label, budget, synthetic, expected_status: status, observed_status: observed,
    expected_reason: status ? null : reason, observed_reason: status ? null : after.readUInt32LE(72), expected_retired: status ? null : retired, observed_retired: status ? null : after.readUInt32LE(76),
    expected_pc: status ? null : next.pc, observed_pc: status ? null : after.readUInt32LE(48), expected_flags: status ? null : next.flags, observed_flags: status ? null : after.readUInt32LE(52),
    expected_cpu: status ? null : next, observed_cpu: status ? null : cpu(c), helper_calls: helperTrace.length - at, helper_hex: after.subarray(100, 140).toString('hex')});
  counts.runs++; if (!status) counts.retired += retired;
}
function checkRam(c, address, expected) {
  assert.ok(address + expected.length <= 4294967296 && expected.length % 4 === 0);
  for (let i = 0; i < expected.length; i += 4) {const before = arena(c), value = expected.readUInt32LE(i); assert.equal(c.api.read32(address + i), 0); before.set(helper32(value), 100); assert.deepEqual(arena(c), before, 'diagnostic selectedRAM'); diagnostics.push({context: c.id, address: address + i, value});}
}
function frame(c, address, old) {const start = Math.max(0, Math.min(address - 12, 4294967296 - 32)), bytes = Buffer.from(Array.from({length: 32}, (_, i) => (i * 17 + 23) & 255)); bytes[address - start] = old; upload(c, start, bytes); return {address: start, bytes};}
function changedFrame(f, address, value) {const bytes = Buffer.from(f.bytes); bytes[address - f.address] = value; return {...f, bytes};}

function compared(start, old, next) {
  const registers = [...start.registers], expected = byteOf(registers[0], 0);
  if (expected !== old) registers[0] = mergeByte(registers[0], 0, old);
  return {...start, registers, pc: next, flags: cmpFlags(expected, old, start.flags)};
}
function compareCase(c, unit, row, address, old, registers, flags, label, f = frame(c, address, old)) {
  const start = seed(c, row.pc, flags, registers), desired = byteOf(registers[row.source % 4], row.source), expected = byteOf(registers[0], 0);
  run(c, unit, 1, compared(start, old, row.next), label, {packet: helper(old)});
  const value = expected === old ? desired : old; checkRam(c, f.address, changedFrame(f, address, value).bytes);
  return {start, frame: changedFrame(f, address, value), old, value};
}
function close(c) {
  writeFileSync(join(output, `context-${c.id}-final-arena.bin`), arena(c), {flag: 'wx'});
  contexts.push({id: c.id, owner: c.owner, entries: c.entries, purpose: c.purpose, captures: c.captures, maximum_mapped_pages: c.maxPages});
  host(c, 'close', []);
}
function stopped(c, unit, registers, flags, fault, label, prior = true) {
  const start = prior ? seed(c, 0x1700, flags, registers) : cpu(c), next = {...start, registers: [...start.registers], pc: FAULT};
  if (prior) next.registers[3] = 0xcafebabe;
  run(c, unit, 2, next, label, {reason: 5, retired: prior ? 1 : 0, fault, packet: helper(0, fault)}); counts.faults++; return next;
}
function core(owner, entries) {
  const c = open(owner, entries, 'core'), a = compile(c, 'all-eight-original-byte-sources', normal);
  for (const row of normal) for (const match of [false, true]) {
    const r = parents(); setSource(r, 0, 0x80); if (row.source !== 0) setSource(r, row.source, 0xd3);
    compareCase(c, a, row, DATA, match ? byteOf(r[0], 0) : 0x7f, r, 0xcd7, match ? 'source-match' : 'source-mismatch'); counts.sources++;
  }
  const pairs = [[0,0],[0,1],[1,0],[0x80,1],[0x7f,0xff],[0x10,1],[0x80,0xff],[0x34,0xa7]];
  for (const [index, flags] of FLAGS.entries()) {
    const row = normal[index % 8], r = parents(), [expected, old] = pairs[index % pairs.length];
    setSource(r, 0, expected); if (row.source !== 0) setSource(r, row.source, 0xd3);
    compareCase(c, a, row, DATA, old, r, flags, 'all-valid-input-flags'); counts.flag_rows++;
  }
  const b = compile(c, 'aliases-boundary-fault', [...aliases, faultBlock]);
  for (const row of aliasRows) for (const match of [false, true]) {
    const r = parents(); for (const [i, value] of Object.entries(row.set)) r[Number(i)] = value;
    assert.equal(addressOf(row, r), DATA);
    const expected = byteOf(r[0], 0);
    compareCase(c, b, row, DATA, match ? expected : expected ^ 0xff, r, 0xcd7, match ? 'ea-match' : 'ea-mismatch'); counts.aliases++;
  }
  const row = {pc: FAULT, source: 1, next: FAULT + 4};
  for (const address of [0,1,0x5ffe,0x5fff,0x6000,0xfffffffe,0xffffffff]) for (const match of [false, true]) {
    const r = parents(); setSource(r, 0, match ? 0xa7 : 0x34); setSource(r, 1, 0xd3); r[6] = address;
    compareCase(c, b, row, address, 0xa7, r, 0x402, 'valid-one-byte-boundary'); counts.boundary++;
  }
  const r = parents(); r[6] = 0x9010;
  for (const [budget, cancel, reason] of [[0,0,1],[1,1,2],[0,1,2]]) {
    const s = seed(c, FAULT, 0xcd7, r); input(c, 96, word(cancel)); run(c, b, budget, s, 'helper-free-budget-cancel', {reason, retired: 0});
    controls.push({context: c.id, kind: 'budget-cancel', budget, cancel});
  }
  input(c, 96, word(0)); let start = seed(c, 0x1900, 0x402);
  run(c, b, 2, start, 'cold-pc', {reason: 3, retired: 0}); controls.push({context: c.id, kind: 'cold-pc'});
  const faultCases = [
    {name:'unmapped', address:0x9010, page:0x9000, permission:null, access:1, detail:1},
    {name:'none', address:0x9010, page:0x9000, permission:0, access:1, detail:2},
    {name:'write-only', address:0x9010, page:0x9000, permission:2, access:1, detail:2},
    {name:'execute-only', address:0x9010, page:0x9000, permission:4, access:1, detail:2},
    {name:'match-read-only', address:0x9010, page:0x9000, permission:1, access:2, detail:2},
    {name:'mismatch-read-execute', address:0x9010, page:0x9000, permission:5, access:2, detail:2, mismatch:true},
    {name:'top-read-denied', address:0xffffffff, page:0xfffff000, permission:2, access:1, detail:2},
    {name:'top-mismatch-write-denied', address:0xffffffff, page:0xfffff000, permission:1, access:2, detail:2, mismatch:true},
  ];
  for (const item of faultCases) {
    if (!c.pages.has(item.page)) map(c, item.page);
    let f = frame(c, item.address, 0xa7);
    if (item.permission === null) unmap(c, item.page); else protect(c, item.page, item.permission);
    const registers = parents(); setSource(registers, 0, item.mismatch ? 0x34 : 0xa7); setSource(registers, 1, 0xd3); registers[6] = item.address;
    const fault = {detail:item.detail,address:item.address,access:item.access};
    stopped(c,b,registers,0xcd7,fault,item.name); if (item.access === 2) checkRam(c,f.address,f.bytes);
    stopped(c,b,registers,0xcd7,fault,item.name+'-unchanged',false); if (item.access === 2) checkRam(c,f.address,f.bytes);
    if (item.permission === null) {map(c,item.page); f={...f,bytes:Buffer.alloc(32)};} else protect(c,item.page,3);
    checkRam(c,f.address,f.bytes);
    const s=cpu(c), old=f.bytes[item.address-f.address], value=byteOf(s.registers[0],0)===old?byteOf(s.registers[1],1):old;
    run(c,b,1,compared(s,old,FAULT+4),item.name+'-repair-only',{packet:helper(old)}); counts.repair_only++;
    checkRam(c,f.address,changedFrame(f,item.address,value).bytes); if (item.page===0x9000) unmap(c,0x9000);
  }
  const f=frame(c,DATA,0xa7); protect(c,0x5000,1); const registers=parents(); setSource(registers,0,0x34); setSource(registers,1,0xd3); registers[6]=DATA;
  const fault={detail:2,address:DATA,access:2}; stopped(c,b,registers,0xcd7,fault,'current-input-stopped');checkRam(c,f.address,f.bytes);
  stopped(c,b,registers,0xcd7,fault,'current-input-unchanged',false);checkRam(c,f.address,f.bytes);
  protect(c,0x5000,3);const changed=frame(c,DATA+64,0xa0);
  input(c,16,word(0x0f1e2da0));input(c,20,word(0xa1b2c3d3));input(c,40,word(DATA+64));input(c,52,word(0x402));start=cpu(c);
  run(c,b,1,compared(start,0xa0,FAULT+4),'explicit-current-al-source-ea-flags-ram',{packet:helper(0xa0)});counts.current_inputs++;
  checkRam(c,f.address,f.bytes);checkRam(c,changed.address,changedFrame(changed,DATA+64,0xd3).bytes);
  synthetic(c,b);
  const retained=frame(c,DATA,0xa7), retainedRegisters=parents();setSource(retainedRegisters,0,0xa7);start=seed(c,normal[0].pc,0x402,retainedRegisters);
  if (owner==='replacement') run(c,a,1,start,'old-generation-neutral',{status:3});
  else {run(c,a,1,compared(start,0xa7,normal[0].next),'retained-current-owner',{packet:helper(0xa7)});checkRam(c,retained.address,retained.bytes);}
  const l=compile(c,'live-byte-cmp-flags-old-value-consumer',[live]);liveProof(c,l);
  upload(c,normal[0].pc,normal[0].bytes.subarray(0,1));start=seed(c,live.pc,0x402);
  run(c,l,1,start,'same-byte-source-invalidated',{status:4});controls.push({context:c.id,kind:'same-byte-rwx-source-alone'});
  const compiled=compile(c,'original-c11-two-live-calls',[...kernelBlocks,...caller],['read32','store','read8','store8','compare']);
  compilerProof(c,compiled);close(c);
}
function synthetic(c, unit) {
  const malformed = [
    {name: 'magic', edit: b => b.write('BAD!')}, {name: 'old-version', edit: b => b.writeUInt16LE(1, 4)},
    {name: 'size', edit: b => b.writeUInt32LE(36, 8)}, {name: 'reserved', edit: b => b.writeUInt32LE(1, 12)},
    {name: 'success-fault-field', edit: b => b.writeUInt32LE(2, 24)}, {name: 'old-value-too-wide', packet: helper(0x100)},
    {name: 'success-length-zero', edit: b => b.writeUInt32LE(0, 36)}, {name: 'success-length-dword', edit: b => b.writeUInt32LE(4, 36)},
    {name: 'fault-value', packet: helper(1, {detail: 2, address: DATA, access: 1})},
    {name: 'write-unmapped', packet: helper(0, {detail: 1, address: DATA, access: 2})},
    {name: 'read-overflow', address: 0xffffffff, packet: helper(0, {detail: 3, address: 0xffffffff, access: 1})},
    {name: 'write-overflow', address: 0xffffffff, packet: helper(0, {detail: 3, address: 0xffffffff, access: 2})},
    {name: 'invalidated-fault', status: 11, packet: helper(0, {detail: 2, address: DATA, access: 1})},
    {name: 'fault-outside-byte', packet: helper(0, {detail: 2, address: DATA + 1, access: 1})},
    {name: 'fault-width', packet: helper(0, {detail: 2, address: DATA, access: 1}), edit: b => b.writeUInt32LE(2, 36)},
    {name: 'fault-access', packet: helper(0, {detail: 2, address: DATA, access: 3})},
    {name: 'fault-reason', packet: helper(0, {detail: 4, address: DATA, access: 1})},
    {name: 'infrastructure-value', packet: record('R3MH', 5, 40, [2, 1, 1, 0, 0, 1])},
    {name: 'infrastructure-reason', packet: record('R3MH', 5, 40, [2, 0, 3, 0, 0, 1])},
    {name: 'infrastructure-address', packet: record('R3MH', 5, 40, [2, 0, 1, DATA, 0, 1])},
    {name: 'unknown-status', status: 99, detail: 3},
    {name: 'top-wrapped-fault', address: 0xffffffff, packet: helper(0, {detail: 2, address: 0, access: 1})},
    {name: 'top-preceding-fault', address: 0xffffffff, packet: helper(0, {detail: 2, address: 0xfffffffe, access: 2})},
    {name: 'invalidated-infrastructure', status: 11, packet: record('R3MH', 5, 40, [2, 0, 1, 0, 0, 1])},
    {name: 'infrastructure-length', packet: record('R3MH', 5, 40, [2, 0, 1, 0, 0, 4])},
    {name: 'canonical-version-exhaustion', packet: record('R3MH', 5, 40, [2, 0, 1, 0, 0, 1]), detail: 1},
    {name: 'canonical-generic-infrastructure', packet: record('R3MH', 5, 40, [2, 0, 2, 0, 0, 1]), detail: 3},
  ];
  assert.equal(malformed.length, 27);
  for (const row of malformed) {
    const address = row.address ?? DATA, r = parents(); r[6] = address; r[0] = 0x89abcdef; r[1] = 0x12345678;
    const f = frame(c, address, 0x11), packet = Buffer.from(row.packet ?? helper(0x11)); row.edit?.(packet);
    let calls = 0; const ring3 = {...unit.ring3}, name = c.owner === 'resident' ? 'compare_exchange_resident8' : 'compare_exchange8';
    ring3[name] = (...args) => {
      assert.deepEqual(args.slice(name.includes('resident') ? 4 : 0).map(v => v >>> 0), [address, 0xef, 0x78]);
      input(c, 100, packet); calls++; return row.status ?? 0;
    };
    const fake = {...unit, name: unit.name + '-synthetic', run: new WebAssembly.Instance(unit.module, {env: {memory: c.memory}, ring3}).exports.run};
    const start = seed(c, FAULT, 0xcd7, r);
    run(c, fake, 1, start, 'nonforwarding-' + row.name, {reason: 7, retired: 0, fault: {detail: row.detail ?? 2}, packet, synthetic: true});
    assert.equal(calls, 1); checkRam(c, f.address, f.bytes); counts.synthetic++;
  }
}
function liveProof(c,unit) {
  const f=frame(c,DATA,0x7f),r=parents();setSource(r,0,0);setSource(r,1,0xd3);r[6]=DATA;
  let s=seed(c,live.pc,0xcd7,r);s=compared(s,0x7f,0x1804);
  assert.equal(s.flags,0x497);run(c,unit,1,s,'live-mismatch-cmp-old-byte',{packet:helper(0x7f)});
  s={...s,registers:[...s.registers],pc:0x1806,flags:0xc92};setSource(s.registers,0,0x80);
  run(c,unit,1,s,'live-adc-consumes-cmp-carry-and-old-byte');
  s={...s,registers:[...s.registers],pc:0x1809};setSource(s.registers,3,0);
  run(c,unit,1,s,'live-setb-consumes-adc-carry');
  s={...s,registers:[...s.registers],pc:0x180c};setSource(s.registers,7,1);
  run(c,unit,1,s,'live-seto-consumes-adc-overflow');
  s={...s,pc:0x180e};run(c,unit,2,s,'live-cold-successor',{reason:3});checkRam(c,f.address,f.bytes);
}
function compilerProof(c,unit) {
  const data=Buffer.from(Array.from({length:64},(_,i)=>(i*17+23)&255));data[16]=0xa7;data[32]=0x34;upload(c,0x5000,data);
  const stack=frame(c,0x706c,0x55),r=parents();r[4]=0x7080;
  let s=seed(c,0x3000,0xcd7,r),trace=helperTrace.length;
  const snapshots=[{label:'entry',cpu:s,arena_hex:arena(c).toString('hex')}];
  s={...s,registers:[...s.registers],pc:0x2000};s.registers[4]=0x7070;
  run(c,unit,4,s,'compiler-first-cdecl-call',{retired:4,packet:helper32(0)});
  s={...s,registers:[...s.registers],pc:0x3014,flags:0xc97};s.registers[0]=0;s.registers[1]=0;s.registers[2]=0x5020;s.registers[4]=0x7074;
  run(c,unit,14,s,'compiler-mismatch-natural-expected-update-return0',{retired:14,packet:helper32(0x3014)});counts.compiler_calls++;
  for(const i of [3,5,6,7])assert.equal(s.registers[i],r[i]);data[32]=0xa7;checkRam(c,0x5000,data);
  snapshots.push({label:'first-return',cpu:cpu(c),arena_hex:arena(c).toString('hex'),cell:0xa7,expected:0xa7,returned:0});
  s={...s,registers:[...s.registers],pc:0x2000,flags:0x412};s.registers[0]=0xa1b2c3d3;s.registers[4]=0x7070;s.registers[5]=0;
  run(c,unit,7,s,'compiler-second-call-consumes-updated-expected',{retired:7,packet:helper32(0)});
  s={...s,registers:[...s.registers],pc:0x302f,flags:0x446};s.registers[0]=1;s.registers[1]=1;s.registers[4]=0x7074;
  run(c,unit,13,s,'compiler-match-natural-return1',{retired:13,packet:helper32(0x302f)});counts.compiler_calls++;
  for(const i of [3,6,7])assert.equal(s.registers[i],r[i]);assert.equal(s.registers[5],0);data[16]=0xd3;checkRam(c,0x5000,data);
  snapshots.push({label:'second-return',cpu:cpu(c),arena_hex:arena(c).toString('hex'),cell:0xd3,expected:0xa7,returned:1});
  s={...s,registers:[...s.registers],pc:0x303c,flags:0x402};s.registers[0]=2;s.registers[4]=0x7080;
  run(c,unit,5,s,'compiler-both-bools-consumed-guest-self-loop',{retired:5,packet:helper32(0)});data.set(word(2),48);checkRam(c,0x5000,data);
  const stackBytes=Buffer.from(stack.bytes);stackBytes.set(words([r[6],0x302f,DATA,0x5020,0xa1b2c3d3]),12);checkRam(c,stack.address,stackBytes);
  const expected=[
    ['store32',0x707c,null,0xa1b2c3d3,0],['store32',0x7078,null,0x5020,0],['store32',0x7074,null,DATA,0],['store32',0x7070,null,0x3014,0],
    ['store32',0x706c,null,r[6],0],['read32',0x7078,null,null,0x5020],['read8',0x707c,null,null,0xd3],['read32',0x7074,null,null,DATA],['read8',0x5020,null,null,0x34],
    ['compare',DATA,0x34,0xd3,0xa7],['store8',0x5020,null,0xa7,0],['read32',0x706c,null,null,r[6]],['read32',0x7070,null,null,0x3014],
    ['store32',0x707c,null,0xa1b2c3d3,0],['store32',0x7078,null,0x5020,0],['store32',0x7074,null,DATA,0],['store32',0x7070,null,0x302f,0],
    ['store32',0x706c,null,r[6],0],['read32',0x7078,null,null,0x5020],['read8',0x707c,null,null,0xd3],['read32',0x7074,null,null,DATA],['read8',0x5020,null,null,0xa7],
    ['compare',DATA,0xa7,0xd3,0xa7],['read32',0x706c,null,null,r[6]],['read32',0x7070,null,null,0x302f],['store32',0x5030,null,2,0],
  ];
  const events=helperTrace.slice(trace);assert.equal(events.length,expected.length);
  for(const [i,[kind,address,compare,inputValue,old]] of expected.entries()){
    const e=events[i],name=kind==='compare'?(c.owner==='resident'?'compare_exchange_resident8':'compare_exchange8'):
      kind==='store32'?(c.owner==='resident'?'store_resident32':'store32'):kind==='store8'?(c.owner==='resident'?'store_resident8':'store8'):kind;
    assert.equal(e.name,name);assert.equal(e.address,address);assert.equal(e.expected,compare);assert.equal(e.input_value,inputValue);assert.equal(e.status,0);
    const packet=kind==='compare'?helper(old):kind==='read8'?helper8(old):kind==='store8'?record('R3MH',3,40,[0,0,0,0,0,1]):helper32(old);
    assert.equal(e.helper_hex,packet.toString('hex'));
  }
  snapshots.push({label:'final',cpu:cpu(c),arena_hex:arena(c).toString('hex'),cell:0xd3,expected:0xa7,consumed_bools:2});
  writeFileSync(join(output,`context-${c.id}-compiler-milestones.json`),JSON.stringify({snapshots,expected_helper_chronology:expected},null,2)+'\n',{flag:'wx'});
}
function smc(owner,entries,identity) {
  const c=open(owner,entries,identity?'smc-mismatch-identity':'smc-match-changed');
  const bytes=Buffer.from([0xf0,0x0f,0xb0,0x0e,0xba,0x7f,0x56,0x34,0x12,0xeb,0]);
  const head=compile(c,'consumed-code-byte-compare-exchange',[{pc:0xc000,bytes,instructions:3}]),r=parents();
  setSource(r,0,identity?0x80:0x7f);setSource(r,1,identity?0xd3:0x80);r[6]=0xc005;
  const start=seed(c,0xc000,0xcd7,r),next=compared(start,0x7f,0xc004),value=identity?0x7f:0x80;
  assert.equal(next.flags,identity?0xc12:0x446);
  const at=helperTrace.length;run(c,head,1,next,identity?'mandatory-mismatch-identity-code-write':'matched-changed-code-write',{reason:6,packet:helper(0x7f)});counts.smc++;
  assert.equal(helperTrace.length-at,1);assert.equal(helperTrace.at(-1).status,11);
  const code=Buffer.from(bytes);code[5]=value;const framed=Buffer.alloc(12);code.copy(framed);checkRam(c,0xc000,framed);
  run(c,head,1,cpu(c),'consumed-owner-stale-neutral',{status:4});
  const suffix=compile(c,'fresh-suffix-no-cas-replay',[{pc:0xc004,bytes:code.subarray(4),instructions:2}],[]);
  const tail={...cpu(c),registers:[...cpu(c).registers],pc:0xc00b};tail.registers[2]=0x12345600+value;
  run(c,suffix,3,tail,'fresh-suffix-uses-committed-code',{reason:3,retired:2});
  run(c,head,1,cpu(c),'old-owner-after-fresh-capture-neutral',{status:owner==='resident'?4:3});close(c);
}
for(const owner of ['replacement','resident'])for(const entries of [false,true]){core(owner,entries);for(const identity of [false,true])smc(owner,entries,identity);}
assert.deepEqual(sourceHashes(),sourcesBefore);
assert.deepEqual(counts,{contexts:12,modules:32,runs:1048,retired:1026,compares:866,helper_calls:962,read32:32,read8:16,store32:44,store8:4,
  sources:64,flag_rows:512,aliases:104,boundary:56,faults:72,repair_only:32,current_inputs:4,synthetic:108,smc:8,compiler_calls:8});
const result={engine:{path:enginePath,sha256:engine.sha256,bytes:engine.bytes.length},source_pins:sourcesBefore,initial_arena_sha256:hash(initial),
  compiler:{text_hex:kernel.toString('hex'),text_sha256:hash(kernel),calls:counts.compiler_calls,combined_blocks:6,combined_instructions:30},
  counts,contexts,modules,observations,helper_trace:helperTrace,public_diagnostics:diagnostics,controls,
  limits:{blocks:8,instructions:64,module_bytes:65536,captures_per_context:6,mapped_pages:12},
  claim:'four bound profiles; full 4612-byte arena comparisons include dirty FP128. Selected 32-byte data frames, 64-byte compiler data, 32-byte compiler stack and 12-byte SMC code only; no whole-RAM claim. One forwarded canonical compare_exchange8 helper per real locked instruction, with old BYTE retained in V5 success packet. Malformed synthetic helpers never forward or commit. Original failure-output store is separate from the CAS transaction. Private flat32 synchronous unshared execution only; no hardware lock, multithread C11, Windows/provider or performance claim.'};
writeFileSync(join(output,'result.json'),JSON.stringify(result,null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({output,counts,result:join(output,'result.json')}));
