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
const sourcePaths = ['engine/src/memory/space.rs', 'engine/src/abi/memory_helper.rs', 'engine/src/abi/arena.rs',
  'engine/src/abi/wasm/exports/calls.rs', 'engine/src/abi/wasm/exports/cpu.rs', 'engine/src/abi/wasm/exports/execution.rs', 'engine/src/abi/wasm/exports/image.rs', 'engine/src/abi/wasm/exports/instance.rs', 'engine/src/abi/wasm/exports/memory.rs', 'engine/src/abi/wasm/exports/mod.rs', 'engine/src/process/instance.rs', 'engine/src/process/execution/compiled.rs', 'engine/src/process/execution/authority.rs', 'engine/src/process/memory/address_space.rs', 'engine/src/process/memory/scalar.rs', 'engine/src/process/memory/scalar/tests.rs', 'engine/src/process/execution/mod.rs', 'engine/src/process/memory/mod.rs', 'engine/src/process/mod.rs', 'engine/src/process/wasm/mod.rs', 'engine/src/process/wasm/loading.rs', 'engine/src/process/wasm/execution.rs', 'engine/src/process/wasm/calls.rs', 'engine/src/process/wasm/memory.rs', 'engine/src/process/wasm/cpu.rs', 'engine/src/process/mod.rs', 'engine/src/process/execution/resident.rs', 'engine/src/process/execution/authority.rs', 'engine/src/process/execution/mod.rs', 'engine/src/process/mod.rs',
  'engine/src/cpu/x86/ir.rs', 'engine/src/cpu/x86/decode/profile.rs', 'engine/src/cpu/x86/decode/lower.rs',
  'engine/src/cpu/dbt/region.rs', 'engine/src/cpu/dbt/wasm/emitter.rs', 'engine/src/cpu/dbt/wasm/integer/arithmetic.rs', 'engine/src/cpu/dbt/wasm/integer/bits.rs', 'engine/src/cpu/dbt/wasm/integer/carry_rotates.rs', 'engine/src/cpu/dbt/wasm/integer/decimal.rs', 'engine/src/cpu/dbt/wasm/integer/dispatch.rs', 'engine/src/cpu/dbt/wasm/integer/flags.rs', 'engine/src/cpu/dbt/wasm/integer/mod.rs', 'engine/src/cpu/dbt/wasm/integer/multiply_divide.rs', 'engine/src/cpu/dbt/wasm/integer/operands.rs', 'engine/src/cpu/dbt/wasm/integer/rotates.rs', 'engine/src/cpu/dbt/wasm/integer/shifts.rs',
  'engine/src/cpu/dbt/wasm/memory.rs', 'engine/src/cpu/dbt/wasm/memory/exchange.rs', 'engine/src/cpu/dbt/wasm/locals.rs',
  'engine/tests/process/memory/exchange/native/dword.rs', 'engine/tests/process/memory/native.rs', 'engine/tests/process/memory/native.rs', 'engine/tests/cpu/atomic/exchange/memory/native/dword.rs', 'engine/tests/cpu/atomic/native.rs', 'engine/tests/support/cpu/prefix/admission.rs', 'engine/tests/cpu/atomic/native.rs', 'engine/tests/cpu/atomic/exchange/memory/wasm/dword.rs', 'engine/tests/cpu/atomic/wasm.rs', 'engine/tests/support/wasm.rs', 'engine/tests/cpu/atomic/wasm.rs', 'engine/tests/support/wasm.rs',
  'engine/tests/cpu/atomic/exchange/memory/fixtures/p2-memory-xchg32/runtime/run.mjs', 'engine/tests/cpu/atomic/exchange/memory/fixtures/p2-memory-xchg32/kernel.c',
  'engine/tests/cpu/atomic/exchange/memory/fixtures/p2-memory-xchg32/kernel.text.bin', 'engine/tests/cpu/atomic/exchange/memory/fixtures/p2-memory-xchg32/receipt.json', 'engine/tests/cpu/atomic/exchange/memory/fixtures/p2-memory-xchg32/PROVENANCE.md',
  'engine/tests/support/wasm/engine.mjs'];
const sourceHashes = () => Object.fromEntries(sourcePaths.map(p => [p, hash(readFileSync(join(root, p)))]));
const sourcesBefore = sourceHashes(), contexts = [], modules = [], observations = [], helperTrace = [], diagnostics = [], controls = [];
const counts = {contexts: 0, modules: 0, runs: 0, retired: 0, exchanges: 0, helper_calls: 0, read32: 0, store32: 0,
  matrix: 0, flag_rows: 0, aliases: 0, boundary: 0, faults: 0, repair_only: 0, current_inputs: 0, synthetic: 0, smc: 0, compiler_calls: 0};
const word = n => {const b = Buffer.alloc(4); b.writeUInt32LE(n >>> 0); return b;};
const words = values => Buffer.concat(values.map(word));
function record(magic, version, size, fields) {
  const b = Buffer.alloc(size); b.write(magic); b.writeUInt16LE(version, 4); b.writeUInt16LE(1, 6); b.writeUInt32LE(size, 8);
  fields.forEach((v, i) => b.writeUInt32LE(v >>> 0, 16 + i * 4)); return b;
}
const state = s => record('R3ST', 1, 56, [...s.registers, s.pc, s.flags]);
const exit = (reason, retired, {detail = 0, address = 0, access = 0} = {}, version = 2) => record('R3EX', reason === 11 ? 6 : version, 40, [reason, retired, detail, address, access, access ? 4 : 0]);
const helper = (value = 0, {detail = 0, address = 0, access = 0} = {}) => record('R3MH', 1, 40, [access ? 1 : 0, value, detail, address, access, access ? 4 : 0]);
const parents = () => Array.from({length: 8}, (_, i) => (0xa100 + i) * 65536 + 0x1234 + i);
const FLAGS = Array.from({length: 128}, (_, n) => [0, 2, 4, 6, 7, 10, 11].reduce((f, bit, i) => f + ((n >>> i & 1) * 2 ** bit), 2));
assert.equal(new Set(FLAGS).size, 128);
const kernel = readFileSync(join(root, 'engine/tests/cpu/atomic/exchange/memory/fixtures/p2-memory-xchg32/kernel.text.bin'));
assert.equal(kernel.toString('hex'), '8b4c24048b4424088701c3');
assert.equal(hash(kernel), '66fb4a5aaae183d35069621ea1b973ad550f0ce9516415542c74b98f4374c6db');
const normal = Array.from({length: 8}, (_, source) => ({pc: 0x1000 + source * 16,
  bytes: Buffer.from([0x87, source << 3 | 5, 0x10, 0x50, 0, 0, 0xeb, 0]), instructions: 2, source, next: 0x1006 + source * 16}));
const aliasDefinitions = [
  {bytes: [0x87, 0x08], source: 1, base: 0, set: {0: DATA}},
  {bytes: [0x87, 0x09], source: 1, base: 1, set: {1: DATA}},
  {bytes: [0x87, 0x24, 0x24], source: 4, base: 4, set: {4: DATA}},
  {bytes: [0x87, 0x6d, 0], source: 5, base: 5, set: {5: DATA}},
  {bytes: [0x87, 0x54, 0xbe, 0xf0], source: 2, base: 6, index: 7, scale: 4, displacement: -16, set: {6: 0x5000, 7: 8}},
  {bytes: [0x87, 0x5c, 0x9b, 1], source: 3, base: 3, index: 3, scale: 4, displacement: 1, set: {3: 0x1003}},
  {bytes: [0x87, 0x35, 0x10, 0x50, 0, 0], source: 6, displacement: DATA, set: {}},
  {bytes: [0x87, 0x34, 0xf5, 0x10, 0x50, 0, 0], source: 6, index: 6, scale: 8, displacement: DATA, set: {6: 0x20000000}},
  {bytes: [0x87, 0x0c, 0x48], source: 1, base: 0, index: 1, scale: 2, set: {0: 0x5000, 1: 8}},
  {bytes: [0x87, 0x44, 0x84, 0x7f], source: 0, base: 4, index: 0, scale: 4, displacement: 127, set: {0: 1, 4: 0x4f8d}},
  {bytes: [0x87, 0xbf, 0xf0, 0xff, 0xff, 0xff], source: 7, base: 7, displacement: -16, set: {7: 0x5020}},
  {bytes: [0x87, 0x6c, 0x35, 0], source: 5, base: 5, index: 6, scale: 1, set: {5: 0x5000, 6: 16}},
  {bytes: [0x87, 0x84, 0xc0, 0x10, 0x50, 0, 0xe0], source: 0, base: 0, index: 0, scale: 8, displacement: 0xe0005010, set: {0: 0x20000000}},
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
const faultBlock = {pc: 0x1700, bytes: Buffer.from([0xba, 0x78, 0x56, 0x34, 0x12, 0x87, 0x06, 0xeb, 0]), instructions: 3};
const live = [{pc: 0x1800, bytes: Buffer.from([0x87, 0x06, 0x83, 0xd0, 0, 0x87, 0x06, 0x0f, 0x90, 0xc3, 0x0f, 0x92, 0xc2, 0x3d, 0, 0, 0, 0x80, 0x0f, 0x94, 0xc7, 0xeb, 0]), instructions: 8}];
const caller = [
  {pc: 0x3000, bytes: Buffer.concat([Buffer.from([0x68]), word(0x12345678), Buffer.from([0x68]), word(DATA), Buffer.from([0xe8]), word(0x2000 - 0x300f)]), instructions: 3},
  {pc: 0x300f, bytes: Buffer.concat([Buffer.from([0x83, 0xc4, 8, 0x50, 0x68]), word(DATA), Buffer.from([0xe8]), word(0x2000 - 0x301d)]), instructions: 4},
  {pc: 0x301d, bytes: Buffer.from([0x83, 0xc4, 8, 0x0f, 0x90, 0xc3, 0x0f, 0x92, 0xc2, 0xeb, 0]), instructions: 4},
];
assert.deepEqual(caller.map(b => b.pc + b.bytes.length), [0x300f, 0x301d, 0x3028]);
const arena = c => Buffer.from(new Uint8Array(c.memory.buffer, c.base, SIZE));
const cpu = c => {const b = arena(c); return {registers: Array.from({length: 8}, (_, i) => b.readUInt32LE(16 + 4 * i)), pc: b.readUInt32LE(48), flags: b.readUInt32LE(52)};};
function input(c, offset, bytes) {const expected = arena(c); expected.set(bytes, offset); new Uint8Array(c.memory.buffer).set(bytes, c.base + offset); assert.deepEqual(arena(c), expected);}
function host(c, name, args, span) {const expected = arena(c); assert.equal(c.api[name](...args), 0, name); if (span) expected.set(arena(c).subarray(...span), span[0]); assert.deepEqual(arena(c), expected, 'host full4612 ' + name);}
function map(c, page, permission = 3) {host(c, 'map', [page, 1, permission]); c.pages.add(page); c.maxPages = Math.max(c.maxPages, c.pages.size); assert.ok(c.maxPages <= 12);}
function protect(c, page, permission) {host(c, 'protect', [page, 1, permission]);}
function unmap(c, page) {host(c, 'unmap', [page, 1]); c.pages.delete(page);}
function upload(c, address, bytes) {assert.ok(address >= 0 && address + bytes.length <= 4294967296); input(c, 140, bytes); host(c, 'upload', [address, bytes.length]);}
function seed(c, pc, flags = 0xcd7, registers = parents()) {input(c, 0, Buffer.concat([state({registers, pc, flags}), exit(1, 0), word(0), helper(0xdecafbad)])); return cpu(c);}
const arities = {open: 3, close: 0, arena_ptr: 0, map: 3, protect: 3, unmap: 2, upload: 2, read32: 1,
  store32: 2, store_resident32: 6, exchange32: 2, exchange_resident32: 6, compile: 1, compile_entries: 2,
  compile_resident: 1, compile_resident_entries: 2, generation: 0, module_ptr: 0, module_len: 0, guard: 6, guard_resident: 7};
function open(owner, entries, purpose) {
  const instance = new WebAssembly.Instance(engine.module, {}), c = {id: ++counts.contexts, owner, entries, purpose, memory: instance.exports.memory, api: {}, high: 0x58434847, captures: 0, pages: new Set(), maxPages: 0};
  for (const [name, arity] of Object.entries(arities)) {c.api[name] = instance.exports['ring3_abi_v1_' + name]; assert.equal(c.api[name]?.length, arity, name);}
  assert.equal(c.api.open(12, c.id, c.high), 0); c.base = c.api.arena_ptr() >>> 0; input(c, 0, initial);
  for (const [page, permission] of [[0, 3], [0x1000, 7], [0x2000, 7], [0x3000, 7], [0x5000, 3], [0x6000, 3], [0x7000, 3], [0xfffff000, 3]]) map(c, page, permission);
  for (const block of [...normal, ...aliases, faultBlock, ...live, ...caller]) upload(c, block.pc, block.bytes);
  upload(c, 0x2000, kernel); protect(c, 0x2000, 5); protect(c, 0x3000, 5);
  if (purpose.startsWith('smc')) {map(c, 0xc000, 7); upload(c, 0xc000, Buffer.from([0x87, 0x08, 0xeb, 0, 0xeb, 0, 0xcc, 0xcc]));}
  return c;
}
function compile(c, name, blocks, helpers = ['exchange']) {
  const instructionCount = blocks.reduce((n, b) => n + b.instructions, 0);
  assert.ok(blocks.length > 0 && blocks.length <= 8 && instructionCount <= 64); assert.ok(++c.captures <= 6);
  input(c, 140, words(blocks.flatMap(b => c.entries ? [b.pc] : [b.pc, b.bytes.length])));
  const method = c.owner === 'resident' ? (c.entries ? 'compile_resident_entries' : 'compile_resident') : (c.entries ? 'compile_entries' : 'compile');
  host(c, method, c.entries ? [blocks.length, 0] : [blocks.length], c.owner === 'resident' ? [140, 164] : null);
  const b = arena(c), binding = c.owner === 'resident' ? {low: b.readUInt32LE(148), high: b.readUInt32LE(152), pointer: b.readUInt32LE(156), length: b.readUInt32LE(160)} : {generation: c.api.generation(), pointer: c.api.module_ptr() >>> 0, length: c.api.module_len() >>> 0};
  const bytes = Buffer.from(new Uint8Array(c.memory.buffer, binding.pointer, binding.length)), module = new WebAssembly.Module(bytes), guard = c.owner === 'resident' ? 'guard_resident' : 'guard';
  const names = helpers.map(n => n === 'exchange' ? (c.owner === 'resident' ? 'exchange_resident32' : 'exchange32') : n === 'store' ? (c.owner === 'resident' ? 'store_resident32' : 'store32') : n);
  const imports = [{module: 'env', name: 'memory', kind: 'memory'}, ...[guard, ...names].map(name => ({module: 'ring3', name, kind: 'function'}))];
  assert.ok(bytes.length <= 65536); assert.deepEqual(WebAssembly.Module.imports(module), imports); assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  const ring3 = {...c.api};
  for (const name of names) ring3[name] = (...args) => {
    const before = arena(c), status = c.api[name](...args), after = arena(c), expected = Buffer.from(before); expected.set(after.subarray(100, 140), 100);
    assert.deepEqual(after, expected, 'canonical helper changes only its40B record');
    const address = args[name.includes('resident') ? 4 : 0] >>> 0;
    helperTrace.push({context: c.id, call: counts.runs + 1, name, address, input_value: name === 'read32' ? null : args.at(-1) >>> 0, status, helper_hex: after.subarray(100, 140).toString('hex')});
    counts.helper_calls++; counts[name.startsWith('exchange') ? 'exchanges' : name === 'read32' ? 'read32' : 'store32']++; return status;
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
  for (let i = 0; i < expected.length; i += 4) {const before = arena(c), value = expected.readUInt32LE(i); assert.equal(c.api.read32(address + i), 0); before.set(helper(value), 100); assert.deepEqual(arena(c), before, 'diagnostic selectedRAM'); diagnostics.push({context: c.id, address: address + i, value});}
}
function frame(c, address, old) {const start = Math.max(0, Math.min(address - 12, 4294967296 - 32)), bytes = Buffer.from(Array.from({length: 32}, (_, i) => (i * 17 + 23) & 255)); bytes.set(word(old), address - start); upload(c, start, bytes); return {address: start, bytes};}
function changedFrame(f, address, value) {const bytes = Buffer.from(f.bytes); bytes.set(word(value), address - f.address); return {...f, bytes};}
function exchanged(start, source, old, pc) {const registers = [...start.registers]; registers[source] = old >>> 0; return {...start, registers, pc};}
function exchangeCase(c, unit, pc, source, address, old, registers, flags, label, f = frame(c, address, old)) {
  const start = seed(c, pc, flags, registers), value = start.registers[source];
  const next = pc === FAULT ? FAULT + 2 : normal.find(r => r.pc === pc)?.next ?? aliasRows.find(r => r.pc === pc)?.next;
  assert.ok(Number.isInteger(next) && next >= 0 && next <= 0xffffffff); run(c, unit, 1, exchanged(start, source, old, next), label, {packet: helper(old)});
  checkRam(c, f.address, changedFrame(f, address, value).bytes); return {start, frame: changedFrame(f, address, value), old, value};
}
function close(c) {
  writeFileSync(join(output, `context-${c.id}-final-arena.bin`), arena(c), {flag: 'wx'});
  contexts.push({id: c.id, owner: c.owner, entries: c.entries, purpose: c.purpose, captures: c.captures, maximum_mapped_pages: c.maxPages});
  host(c, 'close', []);
}
function stopped(c, unit, registers, flags, fault, label, prior = true) {
  const start = prior ? seed(c, 0x1700, flags, registers) : cpu(c), stopped = {...start, registers: [...start.registers], pc: FAULT};
  if (prior) stopped.registers[2] = 0x12345678;
  run(c, unit, 2, stopped, label, {reason: fault.detail === 3 ? 11 : 5, retired: prior ? 1 : 0, fault: fault.detail === 3 ? {} : fault, packet: fault.detail === 3 ? undefined : helper(0, fault)}); counts.faults++;
  return stopped;
}
function core(owner, entries) {
  const c = open(owner, entries, 'core'), a = compile(c, 'all-eight-sources', normal);
  const pairs = [[0, 0xffffffff], [0x80000000, 0x7fffffff], [0x01234567, 0x89abcdef], [0xa5a5a5a5, 0xa5a5a5a5]];
  for (const row of normal) for (const [old, value] of pairs) {
    const registers = parents(); registers[row.source] = value;
    const first = exchangeCase(c, a, row.pc, row.source, DATA, old, registers, FLAGS[(row.source * 13 + value) % 128], 'source-pair'); counts.matrix++;
    input(c, 48, word(row.pc)); const start = cpu(c);
    run(c, a, 1, exchanged(start, row.source, value, row.next), 'pc-only-repeat', {packet: helper(value)});
    checkRam(c, first.frame.address, changedFrame(first.frame, DATA, old).bytes); counts.matrix++;
  }
  for (const [index, flags] of FLAGS.entries()) {
    const row = normal[index % 8], registers = parents(); registers[row.source] = (0x12345678 ^ index) >>> 0;
    exchangeCase(c, a, row.pc, row.source, DATA, (0x89abcdef ^ index) >>> 0, registers, flags, 'all-valid-flags'); counts.flag_rows++;
  }
  const b = compile(c, 'ea-boundary-fault', [...aliases, faultBlock]);
  for (const row of aliasRows) for (const identity of [false, true]) {
    const registers = parents(); for (const [r, value] of Object.entries(row.set)) registers[Number(r)] = value;
    assert.equal(addressOf(row, registers), DATA); const value = registers[row.source], old = identity ? value : (value ^ 0xffffffff) >>> 0;
    exchangeCase(c, b, row.pc, row.source, DATA, old, registers, 0xcd7, identity ? 'ea-identity' : 'ea-changed'); counts.aliases++;
  }
  for (const address of [0, 1, 0x5ffd, 0x5fff, 0xfffffffc]) for (const identity of [false, true]) {
    const registers = parents(); registers[6] = address; registers[0] = identity ? 0x44332211 : 0x89abcdef;
    exchangeCase(c, b, FAULT, 0, address, 0x44332211, registers, 0x402, 'valid-span-boundary'); counts.boundary++;
  }
  const registers = parents(); registers[6] = 0x9008;
  for (const [budget, cancel, reason] of [[0, 0, 1], [1, 1, 2], [0, 1, 2]]) {
    const start = seed(c, FAULT, 0xcd7, registers); input(c, 96, word(cancel));
    run(c, b, budget, start, 'helper-free-budget-cancel', {reason, retired: 0}); controls.push({context: c.id, kind: 'budget-cancel', budget, cancel});
  }
  input(c, 96, word(0)); let start = seed(c, 0x1900, 0x402);
  run(c, b, 2, start, 'cold-pc', {reason: 3, retired: 0}); controls.push({context: c.id, kind: 'cold-pc'});
  const faultCases = [
    {name: 'first-unmapped', address: 0x9010, page: 0x9000, permission: null, access: 1, detail: 1, at: 0x9010},
    {name: 'first-none', address: 0x9010, page: 0x9000, permission: 0, access: 1, detail: 2, at: 0x9010},
    {name: 'first-write-only', address: 0x9010, page: 0x9000, permission: 2, access: 1, detail: 2, at: 0x9010},
    {name: 'first-execute-only', address: 0x9010, page: 0x9000, permission: 4, access: 1, detail: 2, at: 0x9010},
    {name: 'cross-second-unmapped', address: 0x5ffe, page: 0x6000, permission: null, access: 1, detail: 1, at: 0x6000},
    {name: 'cross-second-none', address: 0x5ffe, page: 0x6000, permission: 0, access: 1, detail: 2, at: 0x6000},
    {name: 'overflow', address: 0xffffffff, access: 1, detail: 3, at: 0xffffffff},
    {name: 'first-read-only', address: 0x9010, page: 0x9000, permission: 1, access: 2, detail: 2, at: 0x9010},
    {name: 'first-read-execute', address: 0x9010, page: 0x9000, permission: 5, access: 2, detail: 2, at: 0x9010},
    {name: 'cross-first-read-only', address: 0x5ffe, page: 0x5000, permission: 1, access: 2, detail: 2, at: 0x5ffe},
    {name: 'cross-second-read-only', address: 0x5ffe, page: 0x6000, permission: 1, access: 2, detail: 2, at: 0x6000},
  ];
  for (const row of faultCases) {
    if (row.page === 0x9000 && !c.pages.has(0x9000)) map(c, 0x9000);
    let f;
    if (row.name === 'overflow') {
      upload(c, 0xffffffe0, Buffer.alloc(32, 0x5a)); upload(c, 0, Buffer.alloc(32, 0xa5));
    } else {
      f = frame(c, row.address, 0x44332211);
      if (row.permission === null) unmap(c, row.page); else protect(c, row.page, row.permission);
    }
    const r = parents(); r[0] = 0x89abcdef; r[6] = row.address;
    const fault = {detail: row.detail, address: row.at, access: row.access};
    stopped(c, b, r, 0xcd7, fault, row.name);
    if (row.name === 'overflow') {checkRam(c, 0xffffffe0, Buffer.alloc(32, 0x5a)); checkRam(c, 0, Buffer.alloc(32, 0xa5));}
    else if (row.access === 2) checkRam(c, f.address, f.bytes);
    stopped(c, b, r, 0xcd7, fault, row.name + '-unchanged', false);
    if (row.name === 'overflow') {checkRam(c, 0xffffffe0, Buffer.alloc(32, 0x5a)); checkRam(c, 0, Buffer.alloc(32, 0xa5)); continue;}
    if (row.access === 2) checkRam(c, f.address, f.bytes);
    if (row.permission === null) {
      map(c, row.page); const bytes = Buffer.from(f.bytes);
      for (let i = 0; i < bytes.length; i++) if (f.address + i >= row.page && f.address + i < row.page + 4096) bytes[i] = 0;
      f = {...f, bytes};
    } else protect(c, row.page, 3);
    checkRam(c, f.address, f.bytes);
    const current = cpu(c), old = f.bytes.readUInt32LE(row.address - f.address);
    run(c, b, 1, exchanged(current, 0, old, FAULT + 2), row.name + '-repair-only', {packet: helper(old)}); counts.repair_only++;
    checkRam(c, f.address, changedFrame(f, row.address, current.registers[0]).bytes);
    if (row.page === 0x9000) unmap(c, 0x9000);
  }
  let f = frame(c, 0x5ffe, 0x44332211); protect(c, 0x5000, 1); protect(c, 0x6000, 2);
  let r = parents(); r[0] = 0x89abcdef; r[6] = 0x5ffe;
  const readFault = {detail: 2, address: 0x6000, access: 1}, writeFault = {detail: 2, address: 0x5ffe, access: 2};
  stopped(c, b, r, 0x402, readFault, 'later-read-before-earlier-write'); stopped(c, b, r, 0x402, readFault, 'later-read-repeat', false);
  protect(c, 0x6000, 1); checkRam(c, f.address, f.bytes);
  stopped(c, b, r, 0x402, writeFault, 'read-repair-exposes-write', false); checkRam(c, f.address, f.bytes);
  stopped(c, b, r, 0x402, writeFault, 'write-repeat', false); checkRam(c, f.address, f.bytes);
  protect(c, 0x5000, 3); protect(c, 0x6000, 3); start = cpu(c);
  run(c, b, 1, exchanged(start, 0, 0x44332211, FAULT + 2), 'both-accesses-repair-only', {packet: helper(0x44332211)}); counts.repair_only++;
  checkRam(c, f.address, changedFrame(f, 0x5ffe, 0x89abcdef).bytes);
  f = frame(c, DATA, 0x44332211); protect(c, 0x5000, 1); r = parents(); r[0] = 0x89abcdef; r[6] = DATA;
  const currentFault = {detail: 2, address: DATA, access: 2};
  stopped(c, b, r, 0xcd7, currentFault, 'current-input-stopped'); checkRam(c, f.address, f.bytes);
  stopped(c, b, r, 0xcd7, currentFault, 'current-input-unchanged', false); checkRam(c, f.address, f.bytes);
  protect(c, 0x5000, 3); const changed = frame(c, DATA + 64, 0x76543210);
  input(c, 16, word(0x0f1e2d3c)); input(c, 40, word(DATA + 64)); input(c, 52, word(0x402)); start = cpu(c);
  run(c, b, 1, exchanged(start, 0, 0x76543210, FAULT + 2), 'explicit-current-ram-register-ea-flags', {packet: helper(0x76543210)}); counts.current_inputs++;
  checkRam(c, f.address, f.bytes); checkRam(c, changed.address, changedFrame(changed, DATA + 64, 0x0f1e2d3c).bytes);
  synthetic(c, b);
  f = frame(c, DATA, 0x76543210); r = parents(); r[0] = 0x12345678; start = seed(c, normal[0].pc, 0x402, r);
  if (owner === 'replacement') run(c, a, 1, start, 'old-generation-neutral', {status: 3});
  else {run(c, a, 1, exchanged(start, 0, 0x76543210, normal[0].next), 'retained-current-owner', {packet: helper(0x76543210)}); checkRam(c, f.address, changedFrame(f, DATA, 0x12345678).bytes);}
  const l = compile(c, 'live-returned-value-flags', live); liveProof(c, l);
  upload(c, normal[0].pc, normal[0].bytes.subarray(0, 1)); start = seed(c, live[0].pc, 0x402);
  run(c, l, 1, start, 'same-byte-source-invalidated', {status: 4}); controls.push({context: c.id, kind: 'same-byte-source-rwx-alone'});
  const compiled = compile(c, 'original-c11-two-live-calls', [...caller, {pc: 0x2000, bytes: kernel, instructions: 4}], ['read32', 'store', 'exchange']);
  compilerProof(c, compiled); close(c);
}
function synthetic(c, unit) {
  const malformed = [
    {name: 'magic', edit: b => b.write('BAD!')}, {name: 'version', edit: b => b.writeUInt16LE(2, 4)},
    {name: 'size', edit: b => b.writeUInt32LE(36, 8)}, {name: 'reserved', edit: b => b.writeUInt32LE(1, 12)},
    {name: 'success-fault-field', edit: b => b.writeUInt32LE(2, 24)},
    {name: 'overflow-success', address: 0xffffffff},
    {name: 'fault-value', packet: helper(1, {detail: 2, address: DATA, access: 1})},
    {name: 'write-unmapped', packet: helper(0, {detail: 1, address: DATA, access: 2})},
    {name: 'write-overflow', address:0xfffffffc, packet: helper(0, {detail: 3, address: 0xffffffff, access: 2})},
    {name: 'invalidated-fault', status: 11, packet: helper(0, {detail: 2, address: DATA, access: 1})},
    {name: 'fault-outside-span', packet: helper(0, {detail: 2, address: DATA + 4, access: 1})},
    {name: 'fault-width', packet: helper(0, {detail: 2, address: DATA, access: 1}), edit: b => b.writeUInt32LE(2, 36)},
    {name: 'unknown-status', status: 99, detail: 3},
    ...[1, 2].flatMap(access => [0xfffffffd, 0xfffffffe, 0xffffffff].map(address => ({name: `top-fault-${access}-${address.toString(16)}`, address: 0xfffffffc, packet: helper(0, {detail: 2, address, access})}))),
  ];
  for (const row of malformed) {
    const address = row.address ?? DATA, r = parents(); r[6] = address; r[0] = 0x89abcdef;
    const f = frame(c, address > 0xfffffffc ? 0xfffffffc : address, 0x44332211), packet = Buffer.from(row.packet ?? helper(0x44332211)); row.edit?.(packet);
    let calls = 0; const ring3 = {...unit.ring3}, name = c.owner === 'resident' ? 'exchange_resident32' : 'exchange32';
    ring3[name] = (...args) => {
      assert.equal(args[name.includes('resident') ? 4 : 0] >>> 0, address); assert.equal(args.at(-1) >>> 0, 0x89abcdef);
      input(c, 100, packet); calls++; return row.status ?? 0;
    };
    const fake = {...unit, name: unit.name + '-synthetic', run: new WebAssembly.Instance(unit.module, {env: {memory: c.memory}, ring3}).exports.run};
    const start = seed(c, FAULT, 0xcd7, r);
    // flat-4GiB logical span overflow is stopped before any injected helper packet.
    const segmentOverflow = address > 0xfffffffc;
    run(c, fake, 1, start, 'nonforwarding-' + row.name, segmentOverflow ? {reason: 11, retired: 0, synthetic: true} : {reason: 7, retired: 0, fault: {detail: row.detail ?? 2}, packet, synthetic: true});
    assert.equal(calls, segmentOverflow ? 0 : 1); checkRam(c, f.address, f.bytes); counts.synthetic++;
  }
}
function liveProof(c, unit) {
  const f = frame(c, DATA, 0x7fffffff), r = parents(); r[0] = 0x80000000; r[6] = DATA;
  let s = seed(c, 0x1800, 0xcd7, r);
  s = exchanged(s, 0, 0x7fffffff, 0x1802); run(c, unit, 1, s, 'live-old-value', {packet: helper(0x7fffffff)});
  checkRam(c, f.address, changedFrame(f, DATA, 0x80000000).bytes);
  s = {...s, registers: [...s.registers], pc: 0x1805, flags: 0xc96}; s.registers[0] = 0x80000000;
  run(c, unit, 1, s, 'live-adc-old-value-and-preserved-carry');
  s = {...s, pc: 0x1807}; run(c, unit, 1, s, 'live-identity-exchange-flags', {packet: helper(0x80000000)});
  s = {...s, registers: [...s.registers], pc: 0x180a}; s.registers[3] = (s.registers[3] & 0xffffff00 | 1) >>> 0;
  run(c, unit, 1, s, 'live-overflow-consumer');
  s = {...s, registers: [...s.registers], pc: 0x180d}; s.registers[2] = (s.registers[2] & 0xffffff00) >>> 0;
  run(c, unit, 1, s, 'live-carry-consumer');
  s = {...s, pc: 0x1812, flags: 0x446}; run(c, unit, 1, s, 'live-returned-value-compare');
  s = {...s, registers: [...s.registers], pc: 0x1815}; s.registers[3] = (s.registers[3] & 0xffff00ff | 0x100) >>> 0;
  run(c, unit, 1, s, 'live-zero-consumer');
  s = {...s, pc: 0x1817}; run(c, unit, 2, s, 'live-cold-successor', {reason: 3});
  checkRam(c, f.address, changedFrame(f, DATA, 0x80000000).bytes);
}
function compilerProof(c, unit) {
  const cell = frame(c, DATA, 0x89abcdef), stack = frame(c, 0x6ff4, 0x5a5a5a5a), r = parents(); r[4] = 0x7000;
  let s = seed(c, 0x3000, 0xcd7, r), trace = helperTrace.length;
  const snapshots = [{label: 'entry', cpu: s, arena_hex: arena(c).toString('hex')}];
  s = {...s, registers: [...s.registers], pc: 0x2000}; s.registers[4] = 0x6ff4;
  run(c, unit, 3, s, 'compiler-first-cdecl-call', {retired: 3, packet: helper(0)});
  let bytes = Buffer.from(stack.bytes); bytes.set(words([0x300f, DATA, 0x12345678]), 0x6ff4 - stack.address); checkRam(c, stack.address, bytes);
  s = {...s, registers: [...s.registers], pc: 0x300f}; s.registers[0] = 0x89abcdef; s.registers[1] = DATA; s.registers[4] = 0x6ff8;
  run(c, unit, 4, s, 'compiler-first-original-leaf-return', {retired: 4, packet: helper(0x300f)}); counts.compiler_calls++;
  for (const index of [3, 5, 6, 7]) assert.equal(s.registers[index], r[index]); checkRam(c, cell.address, changedFrame(cell, DATA, 0x12345678).bytes);
  snapshots.push({label: 'first-return', cpu: cpu(c), arena_hex: arena(c).toString('hex'), cell: 0x12345678, returned: s.registers[0]});
  s = {...s, registers: [...s.registers], pc: 0x2000, flags: 0x416}; s.registers[4] = 0x6ff4;
  run(c, unit, 4, s, 'compiler-second-cdecl-call-seeded-from-return', {retired: 4, packet: helper(0)});
  bytes = Buffer.from(stack.bytes); bytes.set(words([0x301d, DATA, 0x89abcdef]), 0x6ff4 - stack.address); checkRam(c, stack.address, bytes);
  s = {...s, registers: [...s.registers], pc: 0x301d}; s.registers[0] = 0x12345678; s.registers[1] = DATA; s.registers[4] = 0x6ff8;
  run(c, unit, 4, s, 'compiler-second-original-leaf-return', {retired: 4, packet: helper(0x301d)}); counts.compiler_calls++;
  for (const index of [3, 5, 6, 7]) assert.equal(s.registers[index], r[index]); checkRam(c, cell.address, cell.bytes);
  snapshots.push({label: 'second-return', cpu: cpu(c), arena_hex: arena(c).toString('hex'), cell: 0x89abcdef, returned: s.registers[0]});
  s = {...s, registers: [...s.registers], pc: 0x3028}; s.registers[4] = 0x7000; s.registers[3] = (s.registers[3] & 0xffffff00) >>> 0; s.registers[2] = (s.registers[2] & 0xffffff00) >>> 0;
  run(c, unit, 5, s, 'compiler-real-ret-flags-consumers-cold', {reason: 3, retired: 4});
  const expected = [
    ['store', 0x6ffc, 0x12345678, 0], ['store', 0x6ff8, DATA, 0], ['store', 0x6ff4, 0x300f, 0],
    ['read', 0x6ff8, null, DATA], ['read', 0x6ffc, null, 0x12345678], ['exchange', DATA, 0x12345678, 0x89abcdef], ['read', 0x6ff4, null, 0x300f],
    ['store', 0x6ffc, 0x89abcdef, 0], ['store', 0x6ff8, DATA, 0], ['store', 0x6ff4, 0x301d, 0],
    ['read', 0x6ff8, null, DATA], ['read', 0x6ffc, null, 0x89abcdef], ['exchange', DATA, 0x89abcdef, 0x12345678], ['read', 0x6ff4, null, 0x301d],
  ];
  const events = helperTrace.slice(trace); assert.equal(events.length, expected.length);
  for (const [i, [kind, address, value, old]] of expected.entries()) {
    assert.ok(events[i].name.startsWith(kind)); assert.equal(events[i].address, address); assert.equal(events[i].input_value, value);
    assert.equal(events[i].status, 0); assert.equal(events[i].helper_hex, helper(old).toString('hex'));
  }
  writeFileSync(join(output, `context-${c.id}-compiler-milestones.json`), JSON.stringify({snapshots, expected_helper_chronology: expected}, null, 2) + '\n', {flag: 'wx'});
}
function smc(owner, entries, identity) {
  const c = open(owner, entries, identity ? 'smc-identity' : 'smc-changed'), block = {pc: 0xc000, bytes: Buffer.from([0x87, 0x08, 0xeb, 0]), instructions: 2};
  const head = compile(c, 'consumed-code-exchange', [block]), old = 0x00eb0887, value = identity ? old : 0x90909090, r = parents(); r[0] = block.pc; r[1] = value;
  const start = seed(c, block.pc, 0xcd7, r), next = exchanged(start, 1, old, 0xc002);
  run(c, head, 1, next, identity ? 'identity-code-exchange-commits-once' : 'changed-code-exchange-commits-once', {reason: 6, packet: helper(old)}); counts.smc++;
  checkRam(c, 0xc000, words([value, 0xcccc00eb]));
  run(c, head, 1, cpu(c), 'consumed-owner-stale-neutral', {status: 4});
  const suffix = compile(c, 'fresh-untouched-suffix', [{pc: 0xc004, bytes: Buffer.from([0xeb, 0]), instructions: 1}], []);
  input(c, 48, word(0xc004)); run(c, suffix, 2, {...cpu(c), pc: 0xc006}, 'fresh-suffix-no-exchange-replay', {reason: 3});
  run(c, head, 1, cpu(c), 'old-owner-after-fresh-capture-neutral', {status: owner === 'resident' ? 4 : 3}); close(c);
}
for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  core(owner, entries); for (const identity of [false, true]) smc(owner, entries, identity);
}
assert.deepEqual(sourceHashes(), sourcesBefore);
assert.deepEqual(counts, {contexts: 12, modules: 32, runs: 1256, retired: 1138, exchanges: 1090, helper_calls: 1138, read32: 24, store32: 24,
  matrix: 256, flag_rows: 512, aliases: 104, boundary: 40, faults: 112, repair_only: 44, current_inputs: 4, synthetic: 76, smc: 8, compiler_calls: 8});
const result = {engine: {path: enginePath, sha256: engine.sha256, bytes: engine.bytes.length}, source_pins: sourcesBefore,
  initial_arena_sha256: hash(initial), compiler: {text_hex: kernel.toString('hex'), text_sha256: hash(kernel), calls: counts.compiler_calls},
  counts, contexts, modules, observations, helper_trace: helperTrace, public_diagnostics: diagnostics, controls,
  limits: {blocks: 8, instructions: 64, module_bytes: 65536, captures_per_context: 6, mapped_pages: 12},
  claim: 'four bound profiles; full4612 comparisons include dirtyFP128; selected32B data and selected stack/code RAM checks only; one forwarded canonical exchange per real instruction. synthetic malformed packets never forward or commit an exchange. synchronous exclusive private-flat32 transaction, no hardware/shared-memory/thread/fence/general C11 concurrency or performance claim.'};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2) + '\n', {flag: 'wx'});
console.log(JSON.stringify({output, counts, result: join(output, 'result.json')}));
