import {pageFaultExit, pageFaultPacket} from '../../../../../../../support/wasm/page-fault.mjs';
import {detectionGuardName, generatedImports, attachDetectionApi} from '../../../../../../../support/wasm/binding.mjs';
import {ARENA_SIZE} from '../../../../../../../support/wasm/arena.mjs';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../../../../../../../support/wasm/engine.mjs';

const [enginePath, output, root] = process.argv.slice(2), SIZE = ARENA_SIZE, DATA = 0x5010, FAULT = 0x1705;
assert.ok(enginePath && output && root);
const engine = readEngine(enginePath), initial = readFileSync(join(output, 'initial-arena.bin'));
assert.equal(initial.length, SIZE);
const hash = b => createHash('sha256').update(b).digest('hex');
const sourcePaths = ['engine/src/memory/space.rs', 'engine/src/memory/space/atomic.rs', 'engine/src/abi/memory_helper.rs', 'engine/src/abi/arena.rs',
  'engine/src/abi/wasm/exports/calls.rs', 'engine/src/abi/wasm/exports/cpu.rs', 'engine/src/abi/wasm/exports/execution.rs', 'engine/src/abi/wasm/exports/image.rs', 'engine/src/abi/wasm/exports/instance.rs', 'engine/src/abi/wasm/exports/memory.rs', 'engine/src/abi/wasm/exports/mod.rs', 'engine/src/process/instance.rs', 'engine/src/process/execution/compiled.rs', 'engine/src/process/execution/authority.rs', 'engine/src/process/memory/address_space.rs', 'engine/src/process/memory/scalar.rs', 'engine/src/process/memory/scalar/tests.rs', 'engine/src/process/execution/mod.rs', 'engine/src/process/memory/mod.rs', 'engine/src/process/mod.rs', 'engine/src/process/wasm/mod.rs', 'engine/src/process/wasm/loading.rs', 'engine/src/abi/image.rs', 'engine/src/process/wasm/execution.rs', 'engine/src/abi/execution.rs', 'engine/src/process/wasm/calls.rs', 'engine/src/process/wasm/memory.rs', 'engine/src/process/wasm/cpu.rs', 'engine/src/process/mod.rs', 'engine/src/process/execution/resident.rs', 'engine/src/process/execution/authority.rs', 'engine/src/process/execution/mod.rs', 'engine/src/process/mod.rs',
  'engine/src/cpu/x86/ir.rs', 'engine/src/cpu/x86/decode/profile.rs', 'engine/src/cpu/x86/decode/families.rs', 'engine/src/cpu/x86/decode/lower.rs', 'engine/src/cpu/x86/decode/families.rs',
  'engine/src/cpu/dbt/region.rs', 'engine/src/cpu/dbt/wasm/emitter/mod.rs', 'engine/src/cpu/dbt/wasm/emitter/module.rs', 'engine/src/cpu/dbt/wasm/emitter/body.rs', 'engine/src/cpu/dbt/wasm/requirements.rs', 'engine/src/cpu/dbt/wasm/imports/wiring.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/arithmetic.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/bits.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/carry_rotates.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/decimal.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/mod.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/arithmetic.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/moves.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/exchange.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/predicates.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/shifts.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/control.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/extended.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/flags.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/mod.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/multiply_divide.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/operands.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/rotates.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/shifts.rs',
  'engine/src/cpu/dbt/wasm/imports/mod.rs', 'engine/src/cpu/dbt/wasm/imports/binding.rs', 'engine/src/cpu/dbt/wasm/imports/context.rs', 'engine/src/cpu/dbt/wasm/imports/needs.rs', 'engine/src/cpu/dbt/wasm/helpers/protocol.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/mod.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/address.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/scalar.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/stack.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/exchange.rs', 'engine/src/cpu/dbt/wasm/locals.rs',
  'engine/tests/process/memory/compare_exchange/native/dword.rs', 'engine/tests/process/memory/native.rs', 'engine/tests/process/memory/native.rs', 'engine/tests/cpu/atomic/compare_exchange/locked/native/dword.rs', 'engine/tests/cpu/atomic/native.rs', 'engine/tests/support/cpu/prefix/admission.rs', 'engine/tests/cpu/atomic/native.rs', 'engine/tests/cpu/atomic/compare_exchange/locked/wasm/dword.rs', 'engine/tests/cpu/atomic/wasm.rs', 'engine/tests/support/wasm.rs', 'engine/tests/cpu/atomic/wasm.rs', 'engine/tests/support/wasm.rs',
  ...['runtime/run.mjs', 'kernel.c', 'build.py', 'kernel.obj', 'kernel.text.bin', 'kernel.disasm.txt', 'receipt.json', 'PROVENANCE.md']
    .map(p => 'engine/tests/cpu/atomic/compare_exchange/locked/fixtures/p2-locked-cmpxchg32/' + p),
  'engine/tests/programs/records/fixtures/p2-compiled-records/build.py', 'engine/tests/support/wasm/engine.mjs', 'engine/tests/support/wasm/binding.mjs', 'engine/tests/support/wasm/page-fault.mjs'];
sourcePaths.push('engine/src/abi/memory_helper/cpu.rs','engine/src/cpu/dbt/wasm/instructions/memory/cpu_scalar.rs','engine/src/cpu/dbt/wasm/emitter/mod.rs', 'engine/src/cpu/dbt/wasm/emitter/module.rs', 'engine/src/cpu/dbt/wasm/emitter/body.rs', 'engine/src/cpu/dbt/wasm/requirements.rs', 'engine/src/cpu/dbt/wasm/imports/wiring.rs','engine/src/process/memory/cpu_scalar.rs','engine/src/process/memory/scalar/cpu_write.rs','engine/src/process/wasm/memory/cpu_write.rs','engine/src/abi/wasm/exports/memory/cpu_write.rs','engine/src/memory/space/cpu/atomic.rs');
const sourceHashes = () => Object.fromEntries(sourcePaths.map(p => [p, hash(readFileSync(join(root, p)))]));
const sourcesBefore = sourceHashes(), contexts = [], modules = [], observations = [], helperTrace = [], diagnostics = [], controls = [];
const counts = {contexts: 0, modules: 0, runs: 0, retired: 0, compares: 0, helper_calls: 0, read32: 0, store32: 0,
  sources: 0, flag_rows: 0, aliases: 0, boundary: 0, faults: 0, repair_only: 0, current_inputs: 0, synthetic: 0, smc: 0, compiler_calls: 0};
const word = n => {const b = Buffer.alloc(4); b.writeUInt32LE(n >>> 0); return b;};
const words = values => Buffer.concat(values.map(word));
function record(magic, version, size, fields) {
  const b = Buffer.alloc(size); b.write(magic); b.writeUInt16LE(version, 4); b.writeUInt16LE(1, 6); b.writeUInt32LE(size, 8);
  fields.forEach((v, i) => b.writeUInt32LE(v >>> 0, 16 + i * 4)); return b;
}
const state = s => record('R3ST', 1, 56, [...s.registers, s.pc, s.flags]);
const legacyExit = (reason, retired, {detail = 0, address = 0, access = 0} = {}, version = 2) => record('R3EX', reason === 11 ? 6 : version, 40, [reason, retired, detail, address, access, access ? 4 : 0]);
const exit = (reason, retired, { detail = 0, address = 0, access = 0 } = {}, version = 2) =>
  reason === 5
    ? pageFaultExit({
        retired,
        errorCode: 6 | (detail === 2 ? 1 : 0),
        address,
        access: 2,
        length: access ? 4 : 0
      })
    : record('R3EX', reason === 11 ? 6 : version, 40, [
        reason,
        retired,
        detail,
        address,
        access,
        access ? 4 : 0
      ]);
const initialHelper = (value = 0, {detail = 0, address = 0, access = 0} = {}) => record('R3MH', 1, 40, [access ? 1 : 0, value, detail, address, access, access ? 4 : 0]);
const legacyHelper = (value = 0, {detail = 0, address = 0, access = 0} = {}) => record('R3MH', 10, 40, [access ? 1 : 0, value, detail, address, access, 4]);
const helper = (value = 0, { detail = 0, address = 0, access = 0 } = {}) =>
  Boolean(access) && (detail === 1 || detail === 2)
    ? pageFaultPacket({ errorCode: 6 | (detail === 2 ? 1 : 0), address, access: 2, length: 4 })
    : record('R3MH', 10, 40, [access ? 1 : 0, value, detail, address, access, 4]);
const parents = () => Array.from({length: 8}, (_, i) => (0xa100 + i) * 65536 + 0x1234 + i);
const FLAGS = Array.from({length: 128}, (_, n) => [0, 2, 4, 6, 7, 10, 11].reduce((f, bit, i) => f + ((n >>> i & 1) * 2 ** bit), 2));
assert.equal(new Set(FLAGS).size, 128);

const kernel = readFileSync(join(root, 'engine/tests/cpu/atomic/compare_exchange/locked/fixtures/p2-locked-cmpxchg32/kernel.text.bin'));
assert.equal(kernel.toString('hex'), '5357568b5424148b7424188b7c24108b0231c9f00fb1370f94c37402890288d989c85e5f5bc3');
assert.equal(hash(kernel), '70f01b37d55146abb897906886842c6f0eca4b69457ae12cd220aefddb15803d');
const normal = Array.from({length: 8}, (_, source) => ({pc: 0x1000 + source * 16,
  bytes: Buffer.from([0xf0, 0x0f, 0xb1, source << 3 | 5, 0x10, 0x50, 0, 0, 0xeb, 0]), instructions: 2, source, next: 0x1008 + source * 16}));
const definitions = [
  {bytes: [0xf0, 0x0f, 0xb1, 0x08], source: 1, set: {0: DATA}},
  {bytes: [0xf0, 0x0f, 0xb1, 0x01], source: 0, set: {1: DATA}},
  {bytes: [0xf0, 0x0f, 0xb1, 0x24, 0x24], source: 4, set: {4: DATA}},
  {bytes: [0xf0, 0x0f, 0xb1, 0x6d, 0], source: 5, set: {5: DATA}},
  {bytes: [0xf0, 0x0f, 0xb1, 0x4c, 0x80, 1], source: 1, set: {0: 0x1003}},
  {bytes: [0xf0, 0x0f, 0xb1, 0xb4, 0xf6, 0x10, 0x50, 0, 0xe0], source: 6, set: {6: 0x20000000}},
  {bytes: [0xf0, 0x0f, 0xb1, 0x44, 0x84, 0x7f], source: 0, set: {0: 1, 4: 0x4f8d}},
  {bytes: [0xf0, 0x0f, 0xb1, 0x7f, 0xf0], source: 7, set: {7: 0x5020}},
];
const aliasRows = [], aliases = [];
for (let first = 0; first < definitions.length; first += 4) {
  const pc = 0x1400 + aliases.length * 64; let at = pc; const bytes = [];
  for (const row of definitions.slice(first, first + 4)) {aliasRows.push({...row, pc: at, next: at + row.bytes.length}); bytes.push(...row.bytes); at += row.bytes.length;}
  bytes.push(0xeb, 0); aliases.push({pc, bytes: Buffer.from(bytes), instructions: 5});
}
const faultBlock = {pc: 0x1700, bytes: Buffer.from([0xbb, 0xbe, 0xba, 0xfe, 0xca, 0xf0, 0x0f, 0xb1, 0x0e, 0xeb, 0]), instructions: 3};
const live = {pc: 0x1800, bytes: Buffer.from([0xf0, 0x0f, 0xb1, 0x0e, 0x83, 0xd0, 0, 0x0f, 0x92, 0xc3, 0x0f, 0x90, 0xc7, 0xeb, 0]), instructions: 5};
const caller = [
  {pc: 0x3000, bytes: Buffer.concat([Buffer.from([0x68]), word(0xaabbccdd), Buffer.from([0x68]), word(0x5020), Buffer.from([0x68]), word(DATA), Buffer.from([0xe8]), word(0x2000 - 0x3014)]), instructions: 4},
  {pc: 0x3014, bytes: Buffer.concat([Buffer.from([0x83, 0xc4, 12, 0x89, 0xc5, 0x8d, 0x80]), word(0xaabbccdd), Buffer.from([0x50, 0x68]), word(0x5020), Buffer.from([0x68]), word(DATA), Buffer.from([0xe8]), word(0x2000 - 0x302f)]), instructions: 7},
  {pc: 0x302f, bytes: Buffer.concat([Buffer.from([0x83, 0xc4, 12, 0xc1, 0xe0, 1, 0x09, 0xe8, 0xa3]), word(0x5030), Buffer.from([0xeb, 0xfe])]), instructions: 5},
];
const kernelBlocks = [{pc: 0x2000, bytes: kernel.subarray(0, 28), instructions: 11},
  {pc: 0x201c, bytes: kernel.subarray(28, 30), instructions: 1}, {pc: 0x201e, bytes: kernel.subarray(30), instructions: 6}];
assert.deepEqual(caller.map(b => b.pc + b.bytes.length), [0x3014, 0x302f, 0x303e]);
const u32 = n => Number((n % 4294967296n + 4294967296n) % 4294967296n);
function cmpFlags(expected, old, flags) {
  const a = BigInt(expected), b = BigInt(old), result = u32(a - b), low = result & 255;
  let parity = 0; for (let bit = 0; bit < 8; bit++) parity += low >>> bit & 1;
  return (flags & 0x400) | 2 | (a < b ? 1 : 0) | (parity % 2 === 0 ? 4 : 0) |
    ((expected & 15) < (old & 15) ? 16 : 0) | (result === 0 ? 64 : 0) | (result >= 0x80000000 ? 128 : 0) |
    (((expected ^ old) & (expected ^ result) & 0x80000000) !== 0 ? 0x800 : 0);
}
for (const [a,b,flags] of [[0,0,0x46],[0,1,0x97],[0x80000000,1,0x816],[0x7fffffff,0xffffffff,0x887],[0,0x7fffffff,0x93]]) assert.equal(cmpFlags(a,b,2),flags);
const arena = c => Buffer.from(new Uint8Array(c.memory.buffer, c.base, SIZE));
const cpu = c => {const b = arena(c); return {registers: Array.from({length: 8}, (_, i) => b.readUInt32LE(16 + 4 * i)), pc: b.readUInt32LE(48), flags: b.readUInt32LE(52)};};
function input(c, offset, bytes) {const expected = arena(c); expected.set(bytes, offset); new Uint8Array(c.memory.buffer).set(bytes, c.base + offset); assert.deepEqual(arena(c), expected);}
function host(c, name, args, span) {const expected = arena(c); assert.equal(c.api[name](...args), 0, name); if (span) expected.set(arena(c).subarray(...span), span[0]); assert.deepEqual(arena(c), expected, 'host complete arena ' + name);}
function map(c, page, permission = 3) {host(c, 'map', [page, 1, permission]); c.pages.add(page); c.maxPages = Math.max(c.maxPages, c.pages.size); assert.ok(c.maxPages <= 12);}
function protect(c, page, permission) {host(c, 'protect', [page, 1, permission]);}
function unmap(c, page) {host(c, 'unmap', [page, 1]); c.pages.delete(page);}
function upload(c, address, bytes) {assert.ok(address >= 0 && address + bytes.length <= 4294967296); input(c, 140, bytes); host(c, 'upload', [address, bytes.length]);}
function seed(c, pc, flags = 0xcd7, registers = parents()) {input(c, 0, Buffer.concat([state({registers, pc, flags}), exit(1, 0), word(0), initialHelper(0xdecafbad)])); return cpu(c);}
const arities = {open: 3, close: 0, arena_ptr: 0, map: 3, protect: 3, unmap: 2, upload: 2, read32: 1,
  store32: 2, store_resident32: 6, compare_exchange32: 3, compare_exchange_resident32: 7, compile: 1, compile_entries: 2,
  compile_resident: 1, compile_resident_entries: 2, generation: 0, module_ptr: 0, module_len: 0, guard: 6, guard_resident: 7};

function open(owner, entries, purpose) {
  const instance = new WebAssembly.Instance(engine.module, {}), c = {id: ++counts.contexts, owner, entries, purpose, memory: instance.exports.memory, api: {}, high: 0x4341534c, captures: 0, pages: new Set(), maxPages: 0};
  for (const [name, arity] of Object.entries(arities)) {c.api[name] = instance.exports['ring3_abi_v1_' + name]; assert.equal(c.api[name]?.length, arity, name);}
 attachDetectionApi(c.api, instance.exports);
  for (const name of ['read32', 'store32', 'store_resident32', 'compare_exchange32', 'compare_exchange_resident32']) {c.api['cpu_'+name] = instance.exports['ring3_abi_v1_cpu_'+name]; assert.equal(c.api['cpu_'+name]?.length, arities[name], 'cpu_'+name);}
  assert.equal(c.api.open(12, c.id, c.high), 0); c.base = c.api.arena_ptr() >>> 0; input(c, 0, initial);
  for (const [page, permission] of [[0, 3], [0x1000, 7], [0x2000, 7], [0x3000, 7], [0x5000, 3], [0x6000, 3], [0x7000, 3], [0xfffff000, 3]]) map(c, page, permission);
  for (const block of [...normal, ...aliases, faultBlock, live, ...caller]) upload(c, block.pc, block.bytes);
  upload(c, 0x2000, kernel); protect(c, 0x2000, 5); protect(c, 0x3000, 5);
  if (purpose.startsWith('smc')) {map(c, 0xc000, 7); upload(c, 0xc000, Buffer.from([0xf0,0x0f,0xb1,0x0e,0xba,0xff,0xff,0xff,0x7f,0xeb,0]));}
  return c;
}
function compile(c, name, blocks, helpers = ['compare']) {
  const instructionCount = blocks.reduce((n, b) => n + b.instructions, 0);
  assert.ok(blocks.length > 0 && blocks.length <= 8 && instructionCount <= 64); assert.ok(++c.captures <= 6);
  input(c, 140, words(blocks.flatMap(b => c.entries ? [b.pc] : [b.pc, b.bytes.length])));
  const method = c.owner === 'resident' ? (c.entries ? 'compile_resident_entries' : 'compile_resident') : (c.entries ? 'compile_entries' : 'compile');
  host(c, method, c.entries ? [blocks.length, 0] : [blocks.length], c.owner === 'resident' ? [140, 164] : null);
  const b = arena(c), binding = c.owner === 'resident' ? {low: b.readUInt32LE(148), high: b.readUInt32LE(152), pointer: b.readUInt32LE(156), length: b.readUInt32LE(160)} : {generation: c.api.generation(), pointer: c.api.module_ptr() >>> 0, length: c.api.module_len() >>> 0};
  const bytes = Buffer.from(new Uint8Array(c.memory.buffer, binding.pointer, binding.length)), module = new WebAssembly.Module(bytes), guard = detectionGuardName(c.owner === 'resident');
  const names = helpers.map(n => n === 'compare' ? (c.owner === 'resident' ? 'compare_exchange_resident32' : 'compare_exchange32') : n === 'store' ? (c.owner === 'resident' ? 'store_resident32' : 'store32') : n).map(n => 'cpu_'+n);
  const imports = [{module: 'env', name: 'memory', kind: 'memory'}, ...generatedImports({resident: c.owner === 'resident', helpers: [...names]}).slice(1).map(item => item.name).map(name => ({module: 'ring3', name, kind: 'function'}))];
  assert.ok(bytes.length <= 65536); assert.deepEqual(WebAssembly.Module.imports(module), imports); assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  const ring3 = {...c.api};
  for (const name of names) ring3[name] = (...args) => {
    const legacyName = name.slice(4);
    const before = arena(c), status = c.api[name](...args), after = arena(c), expected = Buffer.from(before); expected.set(after.subarray(100, 140), 100);
    assert.deepEqual(after, expected, 'canonical helper changes only its 40-byte record');
    const payload = args.slice(name.includes('resident') ? 4 : 0).map(v => v >>> 0);
    helperTrace.push({context: c.id, call: counts.runs + 1, name, address: payload[0], expected: legacyName.startsWith('compare') ? payload[1] : null,
      input_value: legacyName === 'read32' ? null : payload.at(-1), status, helper_hex: after.subarray(100, 140).toString('hex')});
    counts.helper_calls++; counts[legacyName.startsWith('compare') ? 'compares' : legacyName === 'read32' ? 'read32' : 'store32']++; return status;
  };
  const run = new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3}).exports.run; assert.equal(run.length, 4);
  const file = `module-${++counts.modules}.wasm`; writeFileSync(join(output, file), bytes, {flag: 'wx'});
  const unit = {...binding, name, module, ring3, run, exitVersion: names.length ? 6 : 1};
  modules.push({context: c.id, owner: c.owner, entries: c.entries, name, file, bytes: bytes.length, sha256: hash(bytes), blocks: blocks.length, instructions: instructionCount, imports, ...binding}); return unit;
}
function run(c, unit, budget, next, label, {status = 0, reason = 1, retired = 1, packet, fault = {}, synthetic = false} = {}) {
  const before = arena(c), expected = Buffer.from(before), at = helperTrace.length;
  if (status === 0) {expected.set(state(next)); expected.set((synthetic ? legacyExit : exit)(reason, retired, fault, unit.exitVersion), 56); if (packet) expected.set(packet, 100);}
  const observed = unit.run(c.base, c.base + 56, budget, c.base + 96); assert.equal(observed, status, label); assert.deepEqual(arena(c), expected, 'complete arena ' + label);
  const after = arena(c); observations.push({context: c.id, module: unit.name, label, budget, synthetic, expected_status: status, observed_status: observed,
    model_reason: reason, expected_reason: status ? null : reason === 5 && !synthetic ? 23 : reason, observed_reason: status ? null : after.readUInt32LE(72), expected_retired: status ? null : retired, observed_retired: status ? null : after.readUInt32LE(76),
    expected_pc: status ? null : next.pc, observed_pc: status ? null : after.readUInt32LE(48), expected_flags: status ? null : next.flags, observed_flags: status ? null : after.readUInt32LE(52),
    expected_cpu: status ? null : next, observed_cpu: status ? null : cpu(c), helper_calls: helperTrace.length - at, helper_hex: after.subarray(100, 140).toString('hex')});
  counts.runs++; if (!status) counts.retired += retired;
}
function checkRam(c, address, expected) {
  assert.ok(address + expected.length <= 4294967296 && expected.length % 4 === 0);
  for (let i = 0; i < expected.length; i += 4) {const before = arena(c), value = expected.readUInt32LE(i); assert.equal(c.api.read32(address + i), 0); before.set(initialHelper(value), 100); assert.deepEqual(arena(c), before, 'diagnostic selectedRAM'); diagnostics.push({context: c.id, address: address + i, value});}
}
function frame(c, address, old) {const start = Math.max(0, Math.min(address - 12, 4294967296 - 32)), bytes = Buffer.from(Array.from({length: 32}, (_, i) => (i * 17 + 23) & 255)); bytes.set(word(old), address - start); upload(c, start, bytes); return {address: start, bytes};}
function changedFrame(f, address, value) {const bytes = Buffer.from(f.bytes); bytes.set(word(value), address - f.address); return {...f, bytes};}

function compared(start, old, next) {const registers = [...start.registers]; if (registers[0] !== old) registers[0] = old; return {...start, registers, pc: next, flags: cmpFlags(start.registers[0], old, start.flags)};}
function compareCase(c, unit, row, address, old, registers, flags, label, f = frame(c, address, old)) {
  const start = seed(c, row.pc, flags, registers), desired = registers[row.source], expected = registers[0];
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
  run(c, unit, 2, next, label, {reason: fault.detail === 3 ? 11 : 5, retired: prior ? 1 : 0, fault: fault.detail === 3 ? {} : fault, packet: fault.detail === 3 ? undefined : helper(0, fault)}); counts.faults++; return next;
}
function core(owner, entries) {
  const c = open(owner, entries, 'core'), a = compile(c, 'all-eight-original-sources', normal);
  for (const row of normal) for (const match of [false, true]) {
    const r = parents(); r[0] = 0x80000000; if (row.source !== 0) r[row.source] = 0x12345678;
    compareCase(c, a, row, DATA, match ? r[0] : 0x7fffffff, r, 0xcd7, match ? 'source-match' : 'source-mismatch'); counts.sources++;
  }
  const pairs = [[0,0],[0,1],[1,0],[0x80000000,1],[0x7fffffff,0xffffffff],[0x10,1],[0x80000000,0xffffffff],[0,0x7fffffff]];
  for (const [index, flags] of FLAGS.entries()) {
    const row = normal[index % 8], r = parents(), [expected, old] = pairs[index % pairs.length];
    r[0] = expected; if (row.source !== 0) r[row.source] = 0x89abcdef;
    compareCase(c, a, row, DATA, old, r, flags, 'all-valid-input-flags'); counts.flag_rows++;
  }
  const b = compile(c, 'aliases-boundary-fault', [...aliases, faultBlock]);
  for (const row of aliasRows) for (const match of [false, true]) {
    const r = parents(); r[0] = 0x80000000; for (const [i, value] of Object.entries(row.set)) r[Number(i)] = value;
    compareCase(c, b, row, DATA, match ? r[0] : (r[0] ^ 0xffffffff) >>> 0, r, 0xcd7, match ? 'ea-match' : 'ea-mismatch'); counts.aliases++;
  }
  const row = {pc: FAULT, source: 1, next: FAULT + 4};
  for (const address of [0,1,0x5ffd,0x5fff,0xfffffffc]) for (const match of [false, true]) {
    const r = parents(); r[0] = match ? 0x44332211 : 0x89abcdef; r[1] = 0x76543210; r[6] = address;
    compareCase(c, b, row, address, 0x44332211, r, 0x402, 'valid-span-boundary'); counts.boundary++;
  }
  const r = parents(); r[6] = 0x9010;
  for (const [budget, cancel, reason] of [[0,0,1],[1,1,2],[0,1,2]]) {
    const s = seed(c, FAULT, 0xcd7, r); input(c, 96, word(cancel)); run(c, b, budget, s, 'helper-free-budget-cancel', {reason, retired: 0});
    controls.push({context: c.id, kind: 'budget-cancel', budget, cancel});
  }
  input(c, 96, word(0)); let start = seed(c, 0x1900, 0x402);
  run(c, b, 2, start, 'cold-pc', {reason: 3, retired: 0}); controls.push({context: c.id, kind: 'cold-pc'});
  const faultCases = [
    {name:'first-unmapped', address:0x9010, page:0x9000, permission:null, access: 2, detail:1, at:0x9010},
    {name:'first-none', address:0x9010, page:0x9000, permission:0, access: 2, detail:2, at:0x9010},
    {name:'first-write-only', address:0x9010, page:0x9000, permission:2, access: 2, detail:2, at:0x9010},
    {name:'first-execute-only', address:0x9010, page:0x9000, permission:4, access: 2, detail:2, at:0x9010},
    {name:'cross-second-unmapped', address:0x5ffe, page:0x6000, permission:null, access: 2, detail:1, at:0x6000},
    {name:'cross-second-none', address:0x5ffe, page:0x6000, permission:0, access: 2, detail:2, at:0x6000},
    {name:'match-read-only', address:0x9010, page:0x9000, permission:1, access:2, detail:2, at:0x9010},
    {name:'mismatch-read-execute', address:0x9010, page:0x9000, permission:5, access:2, detail:2, at:0x9010, mismatch:true},
    {name:'cross-first-read-only', address:0x5ffe, page:0x5000, permission:1, access:2, detail:2, at:0x5ffe},
    {name:'cross-second-read-only', address:0x5ffe, page:0x6000, permission:1, access:2, detail:2, at:0x6000},
    ...[0xfffffffd,0xfffffffe,0xffffffff].map(address => ({name:'overflow-'+address.toString(16), address, access: 2, detail:3, at:address})),
  ];
  for (const item of faultCases) {
    if (item.page === 0x9000 && !c.pages.has(0x9000)) map(c, 0x9000);
    let f;
    if (item.detail === 3) {upload(c, 0xffffffe0, Buffer.alloc(32,0x5a)); upload(c, 0, Buffer.alloc(32,0xa5));}
    else {f = frame(c,item.address,0x44332211); if (item.permission === null) unmap(c,item.page); else protect(c,item.page,item.permission);}
    const registers = parents(); registers[0] = item.mismatch ? 0x89abcdef : 0x44332211; registers[1] = 0x76543210; registers[6] = item.address;
    const fault = {detail:item.detail,address:item.at,access:item.access};
    const failureRam = () => {if (item.detail === 3) {checkRam(c,0xffffffe0,Buffer.alloc(32,0x5a));checkRam(c,0,Buffer.alloc(32,0xa5));} else if (item.permission === 1) checkRam(c,f.address,f.bytes);};
    stopped(c,b,registers,0xcd7,fault,item.name); failureRam();
    stopped(c,b,registers,0xcd7,fault,item.name+'-unchanged',false); failureRam();
    if (item.detail === 3) continue;
    if (item.permission === null) {
      map(c,item.page); const bytes = Buffer.from(f.bytes);
      for (let i=0;i<bytes.length;i++) if (f.address+i>=item.page && f.address+i<item.page+4096) bytes[i]=0;
      f={...f,bytes};
    } else protect(c,item.page,3);
    checkRam(c,f.address,f.bytes);
    const s=cpu(c), old=f.bytes.readUInt32LE(item.address-f.address), value=s.registers[0]===old?s.registers[1]:old;
    run(c,b,1,compared(s,old,FAULT+4),item.name+'-repair-only',{packet:helper(old)}); counts.repair_only++;
    checkRam(c,f.address,changedFrame(f,item.address,value).bytes); if (item.page===0x9000) unmap(c,0x9000);
  }
  let f=frame(c,0x5ffe,0x44332211); protect(c,0x5000,1); protect(c,0x6000,2);
  let registers=parents(); registers[0]=0x44332211; registers[1]=0x76543210; registers[6]=0x5ffe;
  const writeFault ={detail:2,address:0x5ffe,access:2};
  stopped(c,b,registers,0x402,writeFault,'earlier-write-before-later-read'); stopped(c,b,registers,0x402,writeFault,'earlier-write-unchanged',false);
  protect(c,0x6000,1); checkRam(c,f.address,f.bytes);
  stopped(c,b,registers,0x402,writeFault,'later-page-repair-retains-earlier-write',false); checkRam(c,f.address,f.bytes);
  stopped(c,b,registers,0x402,writeFault,'write-unchanged',false); checkRam(c,f.address,f.bytes);
  protect(c,0x5000,3);protect(c,0x6000,3);start=cpu(c);
  run(c,b,1,compared(start,0x44332211,FAULT+4),'both-accesses-repair-only',{packet:helper(0x44332211)}); counts.repair_only++;
  checkRam(c,f.address,changedFrame(f,0x5ffe,0x76543210).bytes);
  f=frame(c,DATA,0x44332211);protect(c,0x5000,1);registers=parents();registers[0]=0x44332211;registers[1]=0x76543210;registers[6]=DATA;
  const fault={detail:2,address:DATA,access:2}; stopped(c,b,registers,0xcd7,fault,'current-input-stopped');checkRam(c,f.address,f.bytes);
  stopped(c,b,registers,0xcd7,fault,'current-input-unchanged',false);checkRam(c,f.address,f.bytes);
  protect(c,0x5000,3);const changed=frame(c,DATA+64,0x10203040);
  input(c,16,word(0x10203040));input(c,20,word(0xaabbccdd));input(c,40,word(DATA+64));input(c,52,word(0x402));start=cpu(c);
  run(c,b,1,compared(start,0x10203040,FAULT+4),'explicit-current-expected-source-ea-flags-ram',{packet:helper(0x10203040)});counts.current_inputs++;
  checkRam(c,f.address,f.bytes);checkRam(c,changed.address,changedFrame(changed,DATA+64,0xaabbccdd).bytes);
  synthetic(c,b);
  f=frame(c,DATA,0x44332211);registers=parents();registers[0]=0x44332211;start=seed(c,normal[0].pc,0x402,registers);
  if (owner==='replacement') run(c,a,1,start,'old-generation-neutral',{status:3});
  else {run(c,a,1,compared(start,0x44332211,normal[0].next),'retained-current-owner',{packet:helper(0x44332211)});checkRam(c,f.address,f.bytes);}
  const l=compile(c,'live-cmp-flags-old-value-consumer',[live]);liveProof(c,l);
  upload(c,normal[0].pc,normal[0].bytes.subarray(0,1));start=seed(c,live.pc,0x402);
  run(c,l,1,start,'same-byte-source-invalidated',{status:4});controls.push({context:c.id,kind:'same-byte-rwx-source-alone'});
  const compiled=compile(c,'original-c11-two-live-calls',[...kernelBlocks,...caller],['read32','store','compare']);
  compilerProof(c,compiled);close(c);
}
function synthetic(c,unit) {
  const infra=record('R3MH',10,40,[2,0,1,0,0,4]);
  const rows=[
    {name:'magic',edit:b=>b.write('BAD!')},{name:'legacy-reply-version',edit:b=>b.writeUInt16LE(1,4)},
    {name:'size',edit:b=>b.writeUInt32LE(36,8)},{name:'reserved',edit:b=>b.writeUInt32LE(1,12)},
    {name:'success-fault-field',edit:b=>b.writeUInt32LE(2,24)},{name:'overflow-success',address:0xffffffff},
    {name:'fault-value',packet:legacyHelper(1,{detail:2,address:DATA,access:1})},
    {name:'write-unmapped',packet:legacyHelper(0,{detail:1,address:DATA,access:2})},
    {name:'write-overflow',address:0xfffffffc,packet:legacyHelper(0,{detail:3,address:0xffffffff,access:2})},
    {name:'invalidated-fault',status:11,packet:legacyHelper(0,{detail:2,address:DATA,access:1})},
    {name:'fault-outside-span',packet:legacyHelper(0,{detail:2,address:DATA+4,access:1})},
    {name:'fault-width',packet:legacyHelper(0,{detail:2,address:DATA,access:1}),edit:b=>b.writeUInt32LE(2,36)},
    {name:'unknown-status',status:99,detail:3},
    ...[1,2].flatMap(access=>[0xfffffffd,0xfffffffe,0xffffffff].map(address=>({name:`top-fault-${access}-${address.toString(16)}`,address:0xfffffffc,validFault:{detail:2,address,access},packet:legacyHelper(0,{detail:2,address,access})}))),
    {name:'infra-value',packet:infra,edit:b=>b.writeUInt32LE(1,20)},
    {name:'invalidated-infra',packet:infra,status:11},
    {name:'canonical-version-exhausted',packet:infra,detail:1},
  ];
  for (const row of rows) {
    const address=row.address??DATA,r=parents();r[0]=0x12345678;r[1]=0x89abcdef;r[6]=address;
    const f=frame(c,address>0xfffffffc?0xfffffffc:address,0x44332211),packet=Buffer.from(row.packet??legacyHelper(0x44332211));row.edit?.(packet);
    let calls=0;const ring3={...unit.ring3},name=c.owner==='resident'?'compare_exchange_resident32':'compare_exchange32';
    ring3['cpu_'+name]=(...args)=>{assert.deepEqual(args.slice(name.includes('resident')?4:0).map(v=>v>>>0),[address,0x12345678,0x89abcdef]);input(c,100,packet);calls++;return row.status??0;};
    const fake={...unit,name:unit.name+'-synthetic',run:new WebAssembly.Instance(unit.module,{env:{memory:c.memory},ring3}).exports.run};
    const start=seed(c,FAULT,0xcd7,r);
    // flat-4GiB logical span overflow is stopped before any injected helper packet.
    const segmentOverflow=address>0xfffffffc;
    run(c,fake,1,start,'nonforwarding-'+row.name,segmentOverflow?{reason:11,retired:0,synthetic:true}:{reason:row.validFault?5:7,retired:0,fault:row.validFault??{detail:row.detail??2},packet,synthetic:true});
    assert.equal(calls,segmentOverflow?0:1);checkRam(c,f.address,f.bytes);counts.synthetic++;
  }
}
function liveProof(c,unit) {
  const f=frame(c,DATA,0x7fffffff),r=parents();r[0]=0;r[1]=0x12345678;r[6]=DATA;
  let s=seed(c,live.pc,0xcd7,r);s=compared(s,0x7fffffff,0x1804);
  assert.equal(s.flags,0x493);run(c,unit,1,s,'live-mismatch-cmp-old-value',{packet:helper(0x7fffffff)});
  s={...s,registers:[...s.registers],pc:0x1807,flags:0xc96};s.registers[0]=0x80000000;
  run(c,unit,1,s,'live-adc-consumes-cmp-carry-and-old-value');
  s={...s,registers:[...s.registers],pc:0x180a};s.registers[3]=(s.registers[3]&0xffffff00)>>>0;
  run(c,unit,1,s,'live-setb-consumes-adc-carry');
  s={...s,registers:[...s.registers],pc:0x180d};s.registers[3]=(s.registers[3]&0xffff00ff|0x100)>>>0;
  run(c,unit,1,s,'live-seto-consumes-adc-overflow');
  s={...s,pc:0x180f};run(c,unit,2,s,'live-cold-successor',{reason:3});checkRam(c,f.address,f.bytes);
}
function compilerProof(c,unit) {
  const data=Buffer.from(Array.from({length:64},(_,i)=>(i*17+23)&255));data.set(word(0x11223344),16);data.set(word(0x55667788),32);upload(c,0x5000,data);
  const stack=frame(c,0x706c,0x55555555),r=parents();r[4]=0x7080;
  let s=seed(c,0x3000,0xcd7,r),trace=helperTrace.length;
  const snapshots=[{label:'entry',cpu:s,arena_hex:arena(c).toString('hex')}];
  s={...s,registers:[...s.registers],pc:0x2000};s.registers[4]=0x7070;
  run(c,unit,4,s,'compiler-first-cdecl-call',{retired:4,packet:helper(0)});
  s={...s,registers:[...s.registers],pc:0x3014,flags:0x406};s.registers[0]=0;s.registers[1]=0;s.registers[2]=0x5020;s.registers[4]=0x7074;
  run(c,unit,18,s,'compiler-mismatch-natural-expected-update-return0',{retired:18,packet:helper(0x3014)});counts.compiler_calls++;
  for(const i of [3,5,6,7])assert.equal(s.registers[i],r[i]);data.set(word(0x11223344),32);checkRam(c,0x5000,data);
  snapshots.push({label:'first-return',cpu:cpu(c),arena_hex:arena(c).toString('hex'),cell:0x11223344,expected:0x11223344,returned:0});
  s={...s,registers:[...s.registers],pc:0x2000,flags:0x412};s.registers[0]=0xaabbccdd;s.registers[4]=0x7070;s.registers[5]=0;
  run(c,unit,7,s,'compiler-second-call-consumes-updated-expected',{retired:7,packet:helper(0)});
  s={...s,registers:[...s.registers],pc:0x302f,flags:0x446};s.registers[0]=1;s.registers[1]=1;s.registers[4]=0x7074;
  run(c,unit,17,s,'compiler-match-natural-return1',{retired:17,packet:helper(0x302f)});counts.compiler_calls++;
  for(const i of [3,6,7])assert.equal(s.registers[i],r[i]);assert.equal(s.registers[5],0);data.set(word(0xaabbccdd),16);checkRam(c,0x5000,data);
  snapshots.push({label:'second-return',cpu:cpu(c),arena_hex:arena(c).toString('hex'),cell:0xaabbccdd,expected:0x11223344,returned:1});
  s={...s,registers:[...s.registers],pc:0x303c,flags:0x402};s.registers[0]=2;s.registers[4]=0x7080;
  run(c,unit,5,s,'compiler-both-bools-consumed-guest-self-loop',{retired:5,packet:helper(0)});data.set(word(2),48);checkRam(c,0x5000,data);
  const stackBytes=Buffer.from(stack.bytes);stackBytes.set(words([r[6],r[7],r[3],0x302f,DATA,0x5020,0xaabbccdd]),4);checkRam(c,stack.address,stackBytes);
  const expected=[
    ['store',0x707c,null,0xaabbccdd,0],['store',0x7078,null,0x5020,0],['store',0x7074,null,DATA,0],['store',0x7070,null,0x3014,0],
    ['store',0x706c,null,r[3],0],['store',0x7068,null,r[7],0],['store',0x7064,null,r[6],0],
    ['read',0x7078,null,null,0x5020],['read',0x707c,null,null,0xaabbccdd],['read',0x7074,null,null,DATA],['read',0x5020,null,null,0x55667788],
    ['compare',DATA,0x55667788,0xaabbccdd,0x11223344],['store',0x5020,null,0x11223344,0],
    ['read',0x7064,null,null,r[6]],['read',0x7068,null,null,r[7]],['read',0x706c,null,null,r[3]],['read',0x7070,null,null,0x3014],
    ['store',0x707c,null,0xaabbccdd,0],['store',0x7078,null,0x5020,0],['store',0x7074,null,DATA,0],['store',0x7070,null,0x302f,0],
    ['store',0x706c,null,r[3],0],['store',0x7068,null,r[7],0],['store',0x7064,null,r[6],0],
    ['read',0x7078,null,null,0x5020],['read',0x707c,null,null,0xaabbccdd],['read',0x7074,null,null,DATA],['read',0x5020,null,null,0x11223344],
    ['compare',DATA,0x11223344,0xaabbccdd,0x11223344],
    ['read',0x7064,null,null,r[6]],['read',0x7068,null,null,r[7]],['read',0x706c,null,null,r[3]],['read',0x7070,null,null,0x302f],
    ['store',0x5030,null,2,0],
  ];
  const events=helperTrace.slice(trace);assert.equal(events.length,expected.length);
  for(const [i,[kind,address,compare,inputValue,old]] of expected.entries()){
    const e=events[i];assert.ok(e.name.slice(4).startsWith(kind));assert.equal(e.address,address);assert.equal(e.expected,compare);assert.equal(e.input_value,inputValue);
    assert.equal(e.status,0);assert.equal(e.helper_hex,helper(old).toString('hex'));
  }
  snapshots.push({label:'final',cpu:cpu(c),arena_hex:arena(c).toString('hex'),cell:0xaabbccdd,expected:0x11223344,consumed_bools:2});
  writeFileSync(join(output,`context-${c.id}-compiler-milestones.json`),JSON.stringify({snapshots,expected_helper_chronology:expected},null,2)+'\n',{flag:'wx'});
}
function smc(owner,entries,identity) {
  const c=open(owner,entries,identity?'smc-mismatch-identity':'smc-match-changed');
  const bytes=Buffer.from([0xf0,0x0f,0xb1,0x0e,0xba,0xff,0xff,0xff,0x7f,0xeb,0]);
  const head=compile(c,'consumed-code-compare-exchange',[{pc:0xc000,bytes,instructions:3}]),r=parents();
  r[0]=identity?0x80000000:0x7fffffff;r[1]=identity?0xdeadbeef:0x80000000;r[6]=0xc005;
  const start=seed(c,0xc000,0xcd7,r),next=compared(start,0x7fffffff,0xc004),value=identity?0x7fffffff:0x80000000;
  assert.equal(next.flags,identity?0xc12:0x446);
  const at=helperTrace.length;run(c,head,1,next,identity?'mandatory-mismatch-identity-code-write':'matched-changed-code-write',{reason:6,packet:helper(0x7fffffff)});counts.smc++;
  assert.equal(helperTrace.length-at,1);assert.equal(helperTrace.at(-1).status,11);
  const code=Buffer.from(bytes);code.set(word(value),5);const framed=Buffer.alloc(12);code.copy(framed);checkRam(c,0xc000,framed);
  run(c,head,1,cpu(c),'consumed-owner-stale-neutral',{status:4});
  const suffix=compile(c,'fresh-suffix-no-cas-replay',[{pc:0xc004,bytes:code.subarray(4),instructions:2}],[]);
  const tail={...cpu(c),registers:[...cpu(c).registers],pc:0xc00b};tail.registers[2]=value;
  run(c,suffix,3,tail,'fresh-suffix-uses-committed-code',{reason:3,retired:2});
  run(c,head,1,cpu(c),'old-owner-after-fresh-capture-neutral',{status:owner==='resident'?4:3});close(c);
}
for(const owner of ['replacement','resident'])for(const entries of [false,true]){core(owner,entries);for(const identity of [false,true])smc(owner,entries,identity);}
assert.deepEqual(sourceHashes(),sourcesBefore);
assert.deepEqual(counts,{contexts:12,modules:32,runs:1040,retired:1038,compares:854,helper_calls:982,read32:64,store32:64,
  sources:64,flag_rows:512,aliases:64,boundary:40,faults:128,repair_only:44,current_inputs:4,synthetic:88,smc:8,compiler_calls:8});
const result={engine:{path:enginePath,sha256:engine.sha256,bytes:engine.bytes.length},source_pins:sourcesBefore,initial_arena_sha256:hash(initial),
  compiler:{text_hex:kernel.toString('hex'),text_sha256:hash(kernel),calls:counts.compiler_calls,combined_blocks:6,combined_instructions:34},
  counts,contexts,modules,observations,helper_trace:helperTrace,public_diagnostics:diagnostics,controls,
  limits:{blocks:8,instructions:64,module_bytes:65536,captures_per_context:6,mapped_pages:12},
  claim:'four bound profiles; full 4612-byte arena comparisons include dirty FP128. Selected 32-byte data frames, 64-byte compiler data, 32-byte compiler stack and 12-byte SMC code only; no whole-RAM claim. One forwarded canonical compare_exchange helper per real locked instruction, with old DWORD retained in success packet. Malformed synthetic helpers never forward or commit. Original failure-output store is separate from the CAS transaction. Private flat32 synchronous unshared execution only; no hardware lock, multithread C11, Windows/provider or performance claim.'};
writeFileSync(join(output,'result.json'),JSON.stringify(result,null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({output,counts,result:join(output,'result.json')}));
