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
  'engine/tests/abi/cases/native/byte_exchange_helper.rs', 'engine/tests/abi/native.rs', 'engine/tests/abi/native.rs', 'engine/tests/process/memory/cases/native/engine_exchange8_helpers.rs', 'engine/tests/process/memory/native.rs', 'engine/tests/process/memory/native.rs', 'engine/tests/cpu/atomic/cases/native/cpu_memory_xchg8.rs', 'engine/tests/cpu/atomic/native.rs', 'engine/tests/support/cpu/prefix_admission.rs', 'engine/tests/cpu/atomic/native.rs', 'engine/tests/cpu/atomic/cases/wasm/cpu_memory_xchg8_wasm.rs', 'engine/tests/cpu/atomic/wasm.rs', 'engine/tests/support/wasm.rs', 'engine/tests/cpu/atomic/wasm.rs', 'engine/tests/support/wasm.rs',
  'engine/tests/cpu/atomic/fixtures/p2-memory-xchg8/runtime/run.mjs', 'engine/tests/cpu/atomic/fixtures/p2-memory-xchg8/kernel.c',
  'engine/tests/cpu/atomic/fixtures/p2-memory-xchg8/build.py', 'engine/tests/cpu/atomic/fixtures/p2-memory-xchg8/kernel.obj', 'engine/tests/cpu/atomic/fixtures/p2-memory-xchg8/kernel.disasm.txt', 'engine/tests/cpu/atomic/fixtures/p2-memory-xchg8/kernel.text.bin', 'engine/tests/cpu/atomic/fixtures/p2-memory-xchg8/receipt.json', 'engine/tests/cpu/atomic/fixtures/p2-memory-xchg8/PROVENANCE.md',
  'engine/tests/support/wasm/engine.mjs'];
const sourceHashes = () => Object.fromEntries(sourcePaths.map(p => [p, hash(readFileSync(join(root, p)))]));
const sourcesBefore = sourceHashes(), contexts = [], modules = [], observations = [], helperTrace = [], diagnostics = [], controls = [];
const counts = {contexts: 0, modules: 0, runs: 0, retired: 0, exchanges: 0, helper_calls: 0, read32: 0, read8: 0, store32: 0,
  matrix: 0, flag_rows: 0, aliases: 0, boundary: 0, faults: 0, repair_only: 0, current_inputs: 0, synthetic: 0, smc: 0, compiler_calls: 0};
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
const kernel = readFileSync(join(root, 'engine/tests/cpu/atomic/fixtures/p2-memory-xchg8/kernel.text.bin'));
assert.equal(kernel.toString('hex'), '8b4424040fb64c240886080fb6c1c3');
assert.equal(hash(kernel), 'bff846b26485bf378e247d0680815eaaa1e46e628c32e2dd7f966c9167570663');
const normal = Array.from({length: 8}, (_, source) => ({pc: 0x1000 + source * 16,
  bytes: Buffer.from([0x86, source << 3 | 5, 0x10, 0x50, 0, 0, 0xeb, 0]), instructions: 2, source, next: 0x1006 + source * 16}));
const aliasDefinitions = [
  {bytes: [0x86, 0x20], source: 4, base: 0, set: {0: DATA}},
  {bytes: [0x86, 0x29], source: 5, base: 1, set: {1: DATA}},
  {bytes: [0x86, 0x24, 0x24], source: 4, base: 4, set: {4: DATA}},
  {bytes: [0x86, 0x6d, 0], source: 5, base: 5, set: {5: DATA}},
  {bytes: [0x86, 0x54, 0xbe, 0xf0], source: 2, base: 6, index: 7, scale: 4, displacement: -16, set: {6: 0x5000, 7: 8}},
  {bytes: [0x86, 0x5c, 0x9b, 1], source: 3, base: 3, index: 3, scale: 4, displacement: 1, set: {3: 0x1003}},
  {bytes: [0x86, 0x35, 0x10, 0x50, 0, 0], source: 6, displacement: DATA, set: {}},
  {bytes: [0x86, 0x34, 0xf5, 0x10, 0x50, 0, 0], source: 6, index: 6, scale: 8, displacement: DATA, set: {6: 0x20000000}},
  {bytes: [0x86, 0x2c, 0x48], source: 5, base: 0, index: 1, scale: 2, set: {0: 0x5000, 1: 8}},
  {bytes: [0x86, 0x64, 0x84, 0x7f], source: 4, base: 4, index: 0, scale: 4, displacement: 127, set: {0: 1, 4: 0x4f8d}},
  {bytes: [0x86, 0xbf, 0xf0, 0xff, 0xff, 0xff], source: 7, base: 7, displacement: -16, set: {7: 0x5020}},
  {bytes: [0x86, 0x7c, 0x1d, 0], source: 7, base: 5, index: 3, scale: 1, set: {5: 0x5000, 3: 16}},
  {bytes: [0x86, 0x84, 0xc0, 0x10, 0x50, 0, 0xe0], source: 0, base: 0, index: 0, scale: 8, displacement: 0xe0005010, set: {0: 0x20000000}},
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
const faultBlock = {pc: 0x1700, bytes: Buffer.from([0xba, 0x78, 0x56, 0x34, 0x12, 0x86, 0x06, 0xeb, 0]), instructions: 3};
const live = [{pc: 0x1800, bytes: Buffer.from([0x86, 0x06, 0x14, 0, 0x86, 0x06, 0x0f, 0x90, 0xc3, 0x0f, 0x92, 0xc2, 0x3c, 0x80, 0x0f, 0x94, 0xc7, 0xeb, 0]), instructions: 8}];
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
const arities = {open: 3, close: 0, arena_ptr: 0, map: 3, protect: 3, unmap: 2, upload: 2, read32: 1, read8: 1,
  store32: 2, store_resident32: 6, exchange8: 2, exchange_resident8: 6, compile: 1, compile_entries: 2,
  compile_resident: 1, compile_resident_entries: 2, generation: 0, module_ptr: 0, module_len: 0, guard: 6, guard_resident: 7};
function open(owner, entries, purpose) {
  const instance = new WebAssembly.Instance(engine.module, {}), c = {id: ++counts.contexts, owner, entries, purpose, memory: instance.exports.memory, api: {}, high: 0x58434847, captures: 0, pages: new Set(), maxPages: 0};
  for (const [name, arity] of Object.entries(arities)) {c.api[name] = instance.exports['ring3_abi_v1_' + name]; assert.equal(c.api[name]?.length, arity, name);}
  assert.equal(c.api.open(12, c.id, c.high), 0); c.base = c.api.arena_ptr() >>> 0; input(c, 0, initial);
  for (const [page, permission] of [[0, 3], [0x1000, 7], [0x2000, 7], [0x3000, 7], [0x5000, 3], [0x6000, 3], [0x7000, 3], [0xfffff000, 3]]) map(c, page, permission);
  for (const block of [...normal, ...aliases, faultBlock, ...live, ...caller]) upload(c, block.pc, block.bytes);
  upload(c, 0x2000, kernel); protect(c, 0x2000, 5); protect(c, 0x3000, 5);
  if (purpose.startsWith('smc')) {map(c, 0xc000, 7); upload(c, 0xc000, Buffer.from([0x86, 0x08, 0xeb, 0, 0xeb, 0, 0xcc, 0xcc]));}
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
  const names = helpers.map(n => n === 'exchange' ? (c.owner === 'resident' ? 'exchange_resident8' : 'exchange8') : n === 'store' ? (c.owner === 'resident' ? 'store_resident32' : 'store32') : n);
  const imports = [{module: 'env', name: 'memory', kind: 'memory'}, ...[guard, ...names].map(name => ({module: 'ring3', name, kind: 'function'}))];
  assert.ok(bytes.length <= 65536); assert.deepEqual(WebAssembly.Module.imports(module), imports); assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  const ring3 = {...c.api};
  for (const name of names) ring3[name] = (...args) => {
    const before = arena(c), status = c.api[name](...args), after = arena(c), expected = Buffer.from(before); expected.set(after.subarray(100, 140), 100);
    assert.deepEqual(after, expected, 'canonical helper changes only its40B record');
    const address = args[name.includes('resident') ? 4 : 0] >>> 0;
    helperTrace.push({context: c.id, call: counts.runs + 1, name, address, input_value: name.startsWith('read') ? null : args.at(-1) >>> 0, status, helper_hex: after.subarray(100, 140).toString('hex')});
    counts.helper_calls++; counts[name.startsWith('exchange') ? 'exchanges' : name.startsWith('read') ? name : 'store32']++; return status;
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
function exchanged(start, source, old, pc) {const registers = [...start.registers]; setSource(registers, source, old); return {...start, registers, pc};}
function exchangeCase(c, unit, pc, source, address, old, registers, flags, label, f = frame(c, address, old)) {
  const start = seed(c, pc, flags, registers), value = byteOf(start.registers[source % 4], source);
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
  run(c, unit, 2, stopped, label, {reason: 5, retired: prior ? 1 : 0, fault, packet: helper(0, fault)}); counts.faults++;
  return stopped;
}
function core(owner, entries) {
  const c = open(owner, entries, 'core'), a = compile(c, 'all-eight-byte-aliases', normal);
  const pairs = [[0, 255], [128, 127], [103, 239], [165, 165]];
  for (const row of normal) for (const [old, value] of pairs) {
    const registers = parents(); setSource(registers, row.source, value);
    const first = exchangeCase(c, a, row.pc, row.source, DATA, old, registers, FLAGS[(row.source * 13 + value) % 128], 'source-pair'); counts.matrix++;
    input(c, 48, word(row.pc)); const start = cpu(c);
    run(c, a, 1, exchanged(start, row.source, value, row.next), 'pc-only-repeat', {packet: helper(value)});
    checkRam(c, first.frame.address, changedFrame(first.frame, DATA, old).bytes); counts.matrix++;
  }
  for (const [index, flags] of FLAGS.entries()) {
    const row = normal[index % 8], registers = parents(); setSource(registers, row.source, (0x78 ^ index) % 256);
    exchangeCase(c, a, row.pc, row.source, DATA, (0xef ^ index) % 256, registers, flags, 'all-valid-flags'); counts.flag_rows++;
  }
  const b = compile(c, 'ea-boundary-fault', [...aliases, faultBlock]);
  for (const row of aliasRows) for (const identity of [false, true]) {
    const registers = parents(); for (const [r, value] of Object.entries(row.set)) registers[Number(r)] = value;
    assert.equal(addressOf(row, registers), DATA); const value = byteOf(registers[row.source % 4], row.source), old = identity ? value : 255 - value;
    exchangeCase(c, b, row.pc, row.source, DATA, old, registers, 0xcd7, identity ? 'ea-identity' : 'ea-changed'); counts.aliases++;
  }
  for (const address of [0, 1, 0x5ffe, 0x5fff, 0x6000, 0xfffffffe, 0xffffffff]) for (const identity of [false, true]) {
    const registers = parents(); registers[6] = address; setSource(registers, 0, identity ? 0x11 : 0xef);
    exchangeCase(c, b, FAULT, 0, address, 0x11, registers, 0x402, 'valid-one-byte-boundary'); counts.boundary++;
  }
  const registers = parents(); registers[6] = 0x9008;
  for (const [budget, cancel, reason] of [[0, 0, 1], [1, 1, 2], [0, 1, 2]]) {
    const start = seed(c, FAULT, 0xcd7, registers); input(c, 96, word(cancel));
    run(c, b, budget, start, 'helper-free-budget-cancel', {reason, retired: 0}); controls.push({context: c.id, kind: 'budget-cancel', budget, cancel});
  }
  input(c, 96, word(0)); let start = seed(c, 0x1900, 0x402);
  run(c, b, 2, start, 'cold-pc', {reason: 3, retired: 0}); controls.push({context: c.id, kind: 'cold-pc'});
  const faultCases = [
    {name: 'unmapped', address: 0x9010, page: 0x9000, permission: null, access: 1, detail: 1},
    {name: 'none', address: 0x9010, page: 0x9000, permission: 0, access: 1, detail: 2},
    {name: 'write-only', address: 0x9010, page: 0x9000, permission: 2, access: 1, detail: 2},
    {name: 'execute-only', address: 0x9010, page: 0x9000, permission: 4, access: 1, detail: 2},
    {name: 'read-only', address: 0x9010, page: 0x9000, permission: 1, access: 2, detail: 2},
    {name: 'read-execute', address: 0x9010, page: 0x9000, permission: 5, access: 2, detail: 2},
    {name: 'top-read-denied', address: 0xffffffff, page: 0xfffff000, permission: 2, access: 1, detail: 2},
    {name: 'top-write-denied', address: 0xffffffff, page: 0xfffff000, permission: 1, access: 2, detail: 2},
  ];
  for (const row of faultCases) {
    if (!c.pages.has(row.page)) map(c, row.page);
    let f = frame(c, row.address, 0x11);
    if (row.permission === null) unmap(c, row.page); else protect(c, row.page, row.permission);
    const r = parents(); r[0] = 0x89abcdef; r[6] = row.address;
    const fault = {detail: row.detail, address: row.address, access: row.access};
    stopped(c, b, r, 0xcd7, fault, row.name);
    if (row.access === 2) checkRam(c, f.address, f.bytes);
    stopped(c, b, r, 0xcd7, fault, row.name + '-unchanged', false);
    if (row.access === 2) checkRam(c, f.address, f.bytes);
    if (row.permission === null) {map(c, row.page); f = {...f, bytes: Buffer.alloc(32)};} else protect(c, row.page, 3);
    checkRam(c, f.address, f.bytes);
    const current = cpu(c), old = f.bytes[row.address - f.address];
    run(c, b, 1, exchanged(current, 0, old, FAULT + 2), row.name + '-repair-only', {packet: helper(old)}); counts.repair_only++;
    checkRam(c, f.address, changedFrame(f, row.address, byteOf(current.registers[0], 0)).bytes);
    if (row.page === 0x9000) unmap(c, row.page);
  }
  const f = frame(c, DATA, 0x11); protect(c, 0x5000, 1); const r = parents(); r[0] = 0x89abcdef; r[6] = DATA;
  const currentFault = {detail: 2, address: DATA, access: 2};
  stopped(c, b, r, 0xcd7, currentFault, 'current-input-stopped'); checkRam(c, f.address, f.bytes);
  stopped(c, b, r, 0xcd7, currentFault, 'current-input-unchanged', false); checkRam(c, f.address, f.bytes);
  protect(c, 0x5000, 3); const changed = frame(c, DATA + 64, 0xa0);
  input(c, 16, word(0x0f1e2d3c)); input(c, 40, word(DATA + 64)); input(c, 52, word(0x402)); start = cpu(c);
  run(c, b, 1, exchanged(start, 0, 0xa0, FAULT + 2), 'explicit-current-byte-parent-ea-flags', {packet: helper(0xa0)}); counts.current_inputs++;
  checkRam(c, f.address, f.bytes); checkRam(c, changed.address, changedFrame(changed, DATA + 64, 0x3c).bytes);
  synthetic(c, b);
  const retained = frame(c, DATA, 0x10), retainedRegisters = parents(); retainedRegisters[0] = 0x12345678; start = seed(c, normal[0].pc, 0x402, retainedRegisters);
  if (owner === 'replacement') run(c, a, 1, start, 'old-generation-neutral', {status: 3});
  else {run(c, a, 1, exchanged(start, 0, 0x10, normal[0].next), 'retained-current-owner', {packet: helper(0x10)}); checkRam(c, retained.address, changedFrame(retained, DATA, 0x78).bytes);}
  const l = compile(c, 'live-returned-byte-flags', live); liveProof(c, l);
  upload(c, normal[0].pc, normal[0].bytes.subarray(0, 1)); start = seed(c, live[0].pc, 0x402);
  run(c, l, 1, start, 'same-byte-source-invalidated', {status: 4}); controls.push({context: c.id, kind: 'same-byte-source-rwx-alone'});
  const compiled = compile(c, 'original-c11-two-live-calls', [...caller, {pc: 0x2000, bytes: kernel, instructions: 5}], ['read32', 'store', 'read8', 'exchange']);
  compilerProof(c, compiled); close(c);
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
    {name: 'canonical-version-exhaustion', packet: record('R3MH', 5, 40, [2, 0, 1, 0, 0, 1]), detail: 1},
    {name: 'canonical-generic-infrastructure', packet: record('R3MH', 5, 40, [2, 0, 2, 0, 0, 1]), detail: 3},
  ];
  assert.equal(malformed.length, 25);
  for (const row of malformed) {
    const address = row.address ?? DATA, r = parents(); r[6] = address; r[0] = 0x89abcdef;
    const f = frame(c, address, 0x11), packet = Buffer.from(row.packet ?? helper(0x11)); row.edit?.(packet);
    let calls = 0; const ring3 = {...unit.ring3}, name = c.owner === 'resident' ? 'exchange_resident8' : 'exchange8';
    ring3[name] = (...args) => {
      assert.equal(args[name.includes('resident') ? 4 : 0] >>> 0, address); assert.equal(args.at(-1) >>> 0, 0xef);
      input(c, 100, packet); calls++; return row.status ?? 0;
    };
    const fake = {...unit, name: unit.name + '-synthetic', run: new WebAssembly.Instance(unit.module, {env: {memory: c.memory}, ring3}).exports.run};
    const start = seed(c, FAULT, 0xcd7, r);
    run(c, fake, 1, start, 'nonforwarding-' + row.name, {reason: 7, retired: 0, fault: {detail: row.detail ?? 2}, packet, synthetic: true});
    assert.equal(calls, 1); checkRam(c, f.address, f.bytes); counts.synthetic++;
  }
}
function liveProof(c, unit) {
  const f = frame(c, DATA, 0x7f), r = parents(); setSource(r, 0, 0x80); r[6] = DATA;
  let s = seed(c, 0x1800, 0xcd7, r);
  s = exchanged(s, 0, 0x7f, 0x1802); run(c, unit, 1, s, 'live-old-byte', {packet: helper(0x7f)});
  checkRam(c, f.address, changedFrame(f, DATA, 0x80).bytes);
  s = {...s, registers: [...s.registers], pc: 0x1804, flags: 0xc92}; setSource(s.registers, 0, 0x80);
  run(c, unit, 1, s, 'live-adc-byte-and-preserved-carry');
  s = {...s, pc: 0x1806}; run(c, unit, 1, s, 'live-identity-byte-flags', {packet: helper(0x80)});
  s = {...s, registers: [...s.registers], pc: 0x1809}; setSource(s.registers, 3, 1);
  run(c, unit, 1, s, 'live-overflow-consumer');
  s = {...s, registers: [...s.registers], pc: 0x180c}; setSource(s.registers, 2, 0);
  run(c, unit, 1, s, 'live-carry-consumer');
  s = {...s, pc: 0x180e, flags: 0x446}; run(c, unit, 1, s, 'live-returned-byte-compare');
  s = {...s, registers: [...s.registers], pc: 0x1811}; setSource(s.registers, 7, 1);
  run(c, unit, 1, s, 'live-zero-consumer');
  s = {...s, pc: 0x1813}; run(c, unit, 2, s, 'live-cold-successor', {reason: 3});
  checkRam(c, f.address, changedFrame(f, DATA, 0x80).bytes);
}
function compilerProof(c, unit) {
  const cell = frame(c, DATA, 0xef), stack = frame(c, 0x6ff4, 0x5a), r = parents(); r[4] = 0x7000;
  let s = seed(c, 0x3000, 0xcd7, r), trace = helperTrace.length;
  const snapshots = [{label: 'entry', cpu: s, arena_hex: arena(c).toString('hex')}];
  s = {...s, registers: [...s.registers], pc: 0x2000}; s.registers[4] = 0x6ff4;
  run(c, unit, 3, s, 'compiler-first-cdecl-call', {retired: 3, packet: helper32(0)});
  let bytes = Buffer.from(stack.bytes); bytes.set(words([0x300f, DATA, 0x12345678]), 0x6ff4 - stack.address); checkRam(c, stack.address, bytes);
  s = {...s, registers: [...s.registers], pc: 0x300f}; s.registers[0] = 0xef; s.registers[1] = 0xef; s.registers[4] = 0x6ff8;
  run(c, unit, 5, s, 'compiler-first-original-leaf-return', {retired: 5, packet: helper32(0x300f)}); counts.compiler_calls++;
  for (const index of [3, 5, 6, 7]) assert.equal(s.registers[index], r[index]); checkRam(c, cell.address, changedFrame(cell, DATA, 0x78).bytes);
  snapshots.push({label: 'first-return', cpu: cpu(c), arena_hex: arena(c).toString('hex'), cell: 0x78, returned: s.registers[0]});
  s = {...s, registers: [...s.registers], pc: 0x2000, flags: 0x416}; s.registers[4] = 0x6ff4;
  run(c, unit, 4, s, 'compiler-second-cdecl-call-from-return', {retired: 4, packet: helper32(0)});
  bytes = Buffer.from(stack.bytes); bytes.set(words([0x301d, DATA, 0xef]), 0x6ff4 - stack.address); checkRam(c, stack.address, bytes);
  s = {...s, registers: [...s.registers], pc: 0x301d}; s.registers[0] = 0x78; s.registers[1] = 0x78; s.registers[4] = 0x6ff8;
  run(c, unit, 5, s, 'compiler-second-original-leaf-return', {retired: 5, packet: helper32(0x301d)}); counts.compiler_calls++;
  for (const index of [3, 5, 6, 7]) assert.equal(s.registers[index], r[index]); checkRam(c, cell.address, cell.bytes);
  snapshots.push({label: 'second-return', cpu: cpu(c), arena_hex: arena(c).toString('hex'), cell: 0xef, returned: s.registers[0]});
  s = {...s, registers: [...s.registers], pc: 0x3028}; s.registers[4] = 0x7000; setSource(s.registers, 3, 0); setSource(s.registers, 2, 0);
  run(c, unit, 5, s, 'compiler-real-ret-flags-consumers-cold', {reason: 3, retired: 4});
  const expected = [
    ['store', 0x6ffc, 0x12345678, 0, 'dword'], ['store', 0x6ff8, DATA, 0, 'dword'], ['store', 0x6ff4, 0x300f, 0, 'dword'],
    ['read32', 0x6ff8, null, DATA, 'dword'], ['read8', 0x6ffc, null, 0x78, 'byte-read'], ['exchange', DATA, 0x78, 0xef, 'byte-exchange'], ['read32', 0x6ff4, null, 0x300f, 'dword'],
    ['store', 0x6ffc, 0xef, 0, 'dword'], ['store', 0x6ff8, DATA, 0, 'dword'], ['store', 0x6ff4, 0x301d, 0, 'dword'],
    ['read32', 0x6ff8, null, DATA, 'dword'], ['read8', 0x6ffc, null, 0xef, 'byte-read'], ['exchange', DATA, 0xef, 0x78, 'byte-exchange'], ['read32', 0x6ff4, null, 0x301d, 'dword'],
  ];
  const events = helperTrace.slice(trace); assert.equal(events.length, expected.length);
  for (const [i, [kind, address, value, old, width]] of expected.entries()) {
    assert.ok(events[i].name.startsWith(kind)); assert.equal(events[i].address, address); assert.equal(events[i].input_value, value);
    assert.equal(events[i].status, 0); assert.equal(events[i].helper_hex, (width === 'dword' ? helper32(old) : width === 'byte-read' ? helper8(old) : helper(old)).toString('hex'));
  }
  writeFileSync(join(output, `context-${c.id}-compiler-milestones.json`), JSON.stringify({snapshots, expected_helper_chronology: expected}, null, 2) + '\n', {flag: 'wx'});
}
function smc(owner, entries, identity) {
  const c = open(owner, entries, identity ? 'smc-identity' : 'smc-changed'), block = {pc: 0xc000, bytes: Buffer.from([0x86, 0x08, 0xeb, 0]), instructions: 2};
  const head = compile(c, 'consumed-code-exchange', [block]), old = 0x86, value = identity ? old : 0x90, r = parents(); r[0] = block.pc; setSource(r, 1, value);
  const start = seed(c, block.pc, 0xcd7, r), next = exchanged(start, 1, old, 0xc002);
  run(c, head, 1, next, identity ? 'identity-code-exchange-commits-once' : 'changed-code-exchange-commits-once', {reason: 6, packet: helper(old)}); counts.smc++;
  checkRam(c, 0xc000, Buffer.from([value, 0x08, 0xeb, 0, 0xeb, 0, 0xcc, 0xcc]));
  run(c, head, 1, cpu(c), 'consumed-owner-stale-neutral', {status: 4});
  const suffix = compile(c, 'fresh-untouched-suffix', [{pc: 0xc004, bytes: Buffer.from([0xeb, 0]), instructions: 1}], []);
  input(c, 48, word(0xc004)); run(c, suffix, 2, {...cpu(c), pc: 0xc006}, 'fresh-suffix-no-exchange-replay', {reason: 3});
  run(c, head, 1, cpu(c), 'old-owner-after-fresh-capture-neutral', {status: owner === 'resident' ? 4 : 3}); close(c);
}
for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  core(owner, entries); for (const identity of [false, true]) smc(owner, entries, identity);
}
assert.deepEqual(sourceHashes(), sourcesBefore);
assert.deepEqual(counts, {contexts: 12, modules: 32, runs: 1244, retired: 1134, exchanges: 1062, helper_calls: 1110, read32: 16, read8: 8, store32: 24,
  matrix: 256, flag_rows: 512, aliases: 104, boundary: 56, faults: 72, repair_only: 32, current_inputs: 4, synthetic: 100, smc: 8, compiler_calls: 8});
const result = {engine: {path: enginePath, sha256: engine.sha256, bytes: engine.bytes.length}, source_pins: sourcesBefore,
  initial_arena_sha256: hash(initial), compiler: {text_hex: kernel.toString('hex'), text_sha256: hash(kernel), calls: counts.compiler_calls},
  counts, contexts, modules, observations, helper_trace: helperTrace, public_diagnostics: diagnostics, controls,
  limits: {blocks: 8, instructions: 64, module_bytes: 65536, captures_per_context: 6, mapped_pages: 12},
  claim: 'exact unprefixed BYTE memory86, all eight legacy byte aliases/four bound profiles; full4612 comparisons include dirtyFP128; selected32B data and selected stack/code RAM checks only; one forwarded canonical exchange per real instruction. synthetic malformed packets never forward or commit an exchange. synchronous exclusive private-flat32 transaction, no hardware/shared-memory/thread/fence/general C11 concurrency or performance claim.'};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2) + '\n', {flag: 'wx'});
console.log(JSON.stringify({output, counts, result: join(output, 'result.json')}));
