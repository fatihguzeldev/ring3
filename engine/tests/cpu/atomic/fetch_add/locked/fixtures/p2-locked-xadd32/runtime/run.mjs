import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../../../../../../../support/wasm/engine.mjs';

const [enginePath, output, root] = process.argv.slice(2), SIZE = 4612, DATA = 0x5010, FAULT = 0x1705;
assert.ok(enginePath && output && root);
const engine = readEngine(enginePath), initial = readFileSync(join(output, 'initial-arena.bin'));
assert.equal(initial.length, SIZE);
const hash = b => createHash('sha256').update(b).digest('hex');
const sourcePaths = ['engine/src/memory/space.rs', 'engine/src/abi/memory_helper.rs', 'engine/src/abi/arena.rs',
  'engine/src/abi/wasm/exports/calls.rs', 'engine/src/abi/wasm/exports/cpu.rs', 'engine/src/abi/wasm/exports/execution.rs', 'engine/src/abi/wasm/exports/image.rs', 'engine/src/abi/wasm/exports/instance.rs', 'engine/src/abi/wasm/exports/memory.rs', 'engine/src/abi/wasm/exports/mod.rs', 'engine/src/process/instance.rs', 'engine/src/process/execution/compiled.rs', 'engine/src/process/execution/authority.rs', 'engine/src/process/memory/address_space.rs', 'engine/src/process/memory/scalar.rs', 'engine/src/process/memory/scalar_tests.rs', 'engine/src/process/execution/mod.rs', 'engine/src/process/memory/mod.rs', 'engine/src/process/mod.rs', 'engine/src/process/wasm/mod.rs', 'engine/src/process/wasm/loading.rs', 'engine/src/process/wasm/execution.rs', 'engine/src/process/wasm/calls.rs', 'engine/src/process/wasm/memory.rs', 'engine/src/process/wasm/cpu.rs', 'engine/src/process/mod.rs', 'engine/src/process/execution/resident.rs', 'engine/src/process/execution/authority.rs', 'engine/src/process/execution/mod.rs', 'engine/src/process/mod.rs',
  'engine/src/cpu/x86/ir.rs', 'engine/src/cpu/x86/decode/profile.rs', 'engine/src/cpu/x86/decode/lower.rs',
  'engine/src/cpu/dbt/region.rs', 'engine/src/cpu/dbt/wasm/emitter.rs', 'engine/src/cpu/dbt/wasm/integer/arithmetic.rs', 'engine/src/cpu/dbt/wasm/integer/bits.rs', 'engine/src/cpu/dbt/wasm/integer/carry_rotates.rs', 'engine/src/cpu/dbt/wasm/integer/decimal.rs', 'engine/src/cpu/dbt/wasm/integer/dispatch.rs', 'engine/src/cpu/dbt/wasm/integer/flags.rs', 'engine/src/cpu/dbt/wasm/integer/mod.rs', 'engine/src/cpu/dbt/wasm/integer/multiply_divide.rs', 'engine/src/cpu/dbt/wasm/integer/operands.rs', 'engine/src/cpu/dbt/wasm/integer/rotates.rs', 'engine/src/cpu/dbt/wasm/integer/shifts.rs',
  'engine/src/cpu/dbt/wasm/memory.rs', 'engine/src/cpu/dbt/wasm/memory/exchange.rs', 'engine/src/cpu/dbt/wasm/locals.rs',
  'engine/tests/process/memory/exchange_add/native/dword.rs', 'engine/tests/process/memory/native.rs', 'engine/tests/process/memory/native.rs', 'engine/tests/cpu/atomic/fetch_add/locked/native/dword.rs', 'engine/tests/cpu/atomic/native.rs', 'engine/tests/support/cpu/prefix_admission.rs', 'engine/tests/cpu/atomic/native.rs', 'engine/tests/cpu/atomic/fetch_add/locked/wasm/dword.rs', 'engine/tests/cpu/atomic/wasm.rs', 'engine/tests/support/wasm.rs', 'engine/tests/cpu/atomic/wasm.rs', 'engine/tests/support/wasm.rs',
  ...['runtime/run.mjs', 'kernel.c', 'build.py', 'kernel.obj', 'kernel.text.bin', 'kernel.disasm.txt', 'receipt.json', 'PROVENANCE.md']
    .map(p => 'engine/tests/cpu/atomic/fetch_add/locked/fixtures/p2-locked-xadd32/' + p),
  'engine/tests/programs/fixtures/p2-compiled-records/build.py', 'engine/tests/support/wasm/engine.mjs'];
const sourceHashes = () => Object.fromEntries(sourcePaths.map(p => [p, hash(readFileSync(join(root, p)))]));
const sourcesBefore = sourceHashes(), contexts = [], modules = [], observations = [], helperTrace = [], diagnostics = [], controls = [];
const counts = {contexts: 0, modules: 0, runs: 0, retired: 0, exchange_adds: 0, helper_calls: 0, read32: 0, store32: 0,
  sources: 0, flag_rows: 0, aliases: 0, boundary: 0, faults: 0, repair_only: 0, current_inputs: 0, synthetic: 0, smc: 0, compiler_calls: 0};
const word = n => {const b = Buffer.alloc(4); b.writeUInt32LE(n >>> 0); return b;};
const words = values => Buffer.concat(values.map(word));
function record(magic, version, size, fields) {
  const b = Buffer.alloc(size); b.write(magic); b.writeUInt16LE(version, 4); b.writeUInt16LE(1, 6); b.writeUInt32LE(size, 8);
  fields.forEach((v, i) => b.writeUInt32LE(v >>> 0, 16 + i * 4)); return b;
}
const state = s => record('R3ST', 1, 56, [...s.registers, s.pc, s.flags]);
const exit = (reason, retired, {detail = 0, address = 0, access = 0} = {}, version = 2) => record('R3EX', version, 40, [reason, retired, detail, address, access, access ? 4 : 0]);
const helper = (value = 0, {detail = 0, address = 0, access = 0} = {}) => record('R3MH', 1, 40, [access ? 1 : 0, value, detail, address, access, access ? 4 : 0]);
const parents = () => Array.from({length: 8}, (_, i) => (0xa100 + i) * 65536 + 0x1234 + i);
const FLAGS = Array.from({length: 128}, (_, n) => [0, 2, 4, 6, 7, 10, 11].reduce((f, bit, i) => f + ((n >>> i & 1) * 2 ** bit), 2));
assert.equal(new Set(FLAGS).size, 128);
const kernel = readFileSync(join(root, 'engine/tests/cpu/atomic/fetch_add/locked/fixtures/p2-locked-xadd32/kernel.text.bin'));
assert.equal(kernel.toString('hex'), '8b4c24048b442408f00fc101c3');
assert.equal(hash(kernel), 'a4d5d1af2490c9dad170c6b7dd92015d07d7e6adfcbcd7962b0abbe078583505');
const normal = Array.from({length: 8}, (_, source) => ({pc: 0x1000 + source * 16,
  bytes: Buffer.from([0xf0, 0x0f, 0xc1, source << 3 | 5, 0x10, 0x50, 0, 0, 0xeb, 0]), instructions: 2, source, next: 0x1008 + source * 16}));
const definitions = [
  {bytes: [0xf0, 0x0f, 0xc1, 0x08], source: 1, set: {0: DATA}},
  {bytes: [0xf0, 0x0f, 0xc1, 0x01], source: 0, set: {1: DATA}},
  {bytes: [0xf0, 0x0f, 0xc1, 0x24, 0x24], source: 4, set: {4: DATA}},
  {bytes: [0xf0, 0x0f, 0xc1, 0x6d, 0], source: 5, set: {5: DATA}},
  {bytes: [0xf0, 0x0f, 0xc1, 0x4c, 0x80, 1], source: 1, set: {0: 0x1003}},
  {bytes: [0xf0, 0x0f, 0xc1, 0xb4, 0xf6, 0x10, 0x50, 0, 0xe0], source: 6, set: {6: 0x20000000}},
  {bytes: [0xf0, 0x0f, 0xc1, 0x44, 0x84, 0x7f], source: 0, set: {0: 1, 4: 0x4f8d}},
  {bytes: [0xf0, 0x0f, 0xc1, 0x7f, 0xf0], source: 7, set: {7: 0x5020}},
];
const aliasRows = [], aliases = [];
for (let first = 0; first < definitions.length; first += 4) {
  const pc = 0x1400 + aliases.length * 64; let at = pc; const bytes = [];
  for (const row of definitions.slice(first, first + 4)) {aliasRows.push({...row, pc: at, next: at + row.bytes.length}); bytes.push(...row.bytes); at += row.bytes.length;}
  bytes.push(0xeb, 0); aliases.push({pc, bytes: Buffer.from(bytes), instructions: 5});
}
const faultBlock = {pc: 0x1700, bytes: Buffer.from([0xbb, 0xbe, 0xba, 0xfe, 0xca, 0xf0, 0x0f, 0xc1, 0x0e, 0xeb, 0]), instructions: 3};
const live = {pc: 0x1800, bytes: Buffer.from([0xf0, 0x0f, 0xc1, 0x0e, 0x11, 0xc8, 0x0f, 0x92, 0xc3, 0x0f, 0x90, 0xc7, 0xeb, 0]), instructions: 5};
const caller = [
  {pc: 0x3000, bytes: Buffer.concat([Buffer.from([0x6a, 4, 0x68]), word(DATA), Buffer.from([0xe8]), word(0x2000 - 0x300c)]), instructions: 3},
  {pc: 0x300c, bytes: Buffer.concat([Buffer.from([0x83, 0xc4, 8, 0x89, 0xc5, 0x50, 0x68]), word(DATA), Buffer.from([0xe8]), word(0x2000 - 0x301c)]), instructions: 5},
  {pc: 0x301c, bytes: Buffer.concat([Buffer.from([0x83, 0xc4, 8, 0x31, 0xe8, 0xa3]), word(0x5030), Buffer.from([0xeb, 0xfe])]), instructions: 4},
];
const kernelBlocks = [{pc: 0x2000, bytes: kernel, instructions: 4}];
assert.deepEqual(caller.map(b => b.pc + b.bytes.length), [0x300c, 0x301c, 0x3028]);
const u32 = n => Number((n % 4294967296n + 4294967296n) % 4294967296n);
function addFlags(a, b, flags, carry = 0) {
  const total = BigInt(a) + BigInt(b) + BigInt(carry), result = u32(total), low = result & 255;
  let parity = 0; for (let bit = 0; bit < 8; bit++) parity += low >>> bit & 1;
  return (flags & 0x400) | 2 | (total > 0xffffffffn ? 1 : 0) | (parity % 2 === 0 ? 4 : 0) |
    ((a & 15) + (b & 15) + carry > 15 ? 16 : 0) | (result === 0 ? 64 : 0) | (result >= 0x80000000 ? 128 : 0) |
    ((~(a ^ b) & (a ^ result) & 0x80000000) !== 0 ? 0x800 : 0);
}
for (const [a, b, flags] of [[0, 0, 0x46], [0xffffffff, 1, 0x57], [0x7ffffffe, 4, 0x892], [0x80000002, 0x7ffffffe, 0x57], [0x0f, 1, 0x12]]) assert.equal(addFlags(a, b, 2), flags);
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
  store32: 2, store_resident32: 6, exchange_add32: 2, exchange_add_resident32: 6, compile: 1, compile_entries: 2,
  compile_resident: 1, compile_resident_entries: 2, generation: 0, module_ptr: 0, module_len: 0, guard: 6, guard_resident: 7};

function open(owner, entries, purpose) {
  const instance = new WebAssembly.Instance(engine.module, {}), c = {id: ++counts.contexts, owner, entries, purpose, memory: instance.exports.memory, api: {}, high: 0x4341534c, captures: 0, pages: new Set(), maxPages: 0};
  for (const [name, arity] of Object.entries(arities)) {c.api[name] = instance.exports['ring3_abi_v1_' + name]; assert.equal(c.api[name]?.length, arity, name);}
  assert.equal(c.api.open(12, c.id, c.high), 0); c.base = c.api.arena_ptr() >>> 0; input(c, 0, initial);
  for (const [page, permission] of [[0, 3], [0x1000, 7], [0x2000, 7], [0x3000, 7], [0x5000, 3], [0x6000, 3], [0x7000, 3], [0xfffff000, 3]]) map(c, page, permission);
  for (const block of [...normal, ...aliases, faultBlock, live, ...caller]) upload(c, block.pc, block.bytes);
  upload(c, 0x2000, kernel); protect(c, 0x2000, 5); protect(c, 0x3000, 5);
  if (purpose.startsWith('smc')) {map(c, 0xc000, 7); upload(c, 0xc000, Buffer.from([0xf0,0x0f,0xc1,0x0e,0xba,0xff,0xff,0xff,0x7f,0xeb,0]));}
  return c;
}
function compile(c, name, blocks, helpers = ['add']) {
  const instructionCount = blocks.reduce((n, b) => n + b.instructions, 0);
  assert.ok(blocks.length > 0 && blocks.length <= 8 && instructionCount <= 64); assert.ok(++c.captures <= 6);
  input(c, 140, words(blocks.flatMap(b => c.entries ? [b.pc] : [b.pc, b.bytes.length])));
  const method = c.owner === 'resident' ? (c.entries ? 'compile_resident_entries' : 'compile_resident') : (c.entries ? 'compile_entries' : 'compile');
  host(c, method, c.entries ? [blocks.length, 0] : [blocks.length], c.owner === 'resident' ? [140, 164] : null);
  const b = arena(c), binding = c.owner === 'resident' ? {low: b.readUInt32LE(148), high: b.readUInt32LE(152), pointer: b.readUInt32LE(156), length: b.readUInt32LE(160)} : {generation: c.api.generation(), pointer: c.api.module_ptr() >>> 0, length: c.api.module_len() >>> 0};
  const bytes = Buffer.from(new Uint8Array(c.memory.buffer, binding.pointer, binding.length)), module = new WebAssembly.Module(bytes), guard = c.owner === 'resident' ? 'guard_resident' : 'guard';
  const names = helpers.map(n => n === 'add' ? (c.owner === 'resident' ? 'exchange_add_resident32' : 'exchange_add32') : n === 'store' ? (c.owner === 'resident' ? 'store_resident32' : 'store32') : n);
  const imports = [{module: 'env', name: 'memory', kind: 'memory'}, ...[guard, ...names].map(name => ({module: 'ring3', name, kind: 'function'}))];
  assert.ok(bytes.length <= 65536); assert.deepEqual(WebAssembly.Module.imports(module), imports); assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  const ring3 = {...c.api};
  for (const name of names) ring3[name] = (...args) => {
    const before = arena(c), status = c.api[name](...args), after = arena(c), expected = Buffer.from(before); expected.set(after.subarray(100, 140), 100);
    assert.deepEqual(after, expected, 'canonical helper changes only its 40-byte record');
    const payload = args.slice(name.includes('resident') ? 4 : 0).map(v => v >>> 0);
    helperTrace.push({context: c.id, call: counts.runs + 1, name, address: payload[0],
      input_value: name === 'read32' ? null : payload.at(-1), status, helper_hex: after.subarray(100, 140).toString('hex')});
    counts.helper_calls++; counts[name.startsWith('exchange_add') ? 'exchange_adds' : name === 'read32' ? 'read32' : 'store32']++; return status;
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

function added(start, old, source, next) {const registers = [...start.registers], delta = registers[source]; registers[source] = old; return {...start, registers, pc: next, flags: addFlags(old, delta, start.flags)};}
function addCase(c, unit, row, address, old, registers, flags, label) {
  const f = frame(c, address, old), start = seed(c, row.pc, flags, registers), delta = registers[row.source];
  run(c, unit, 1, added(start, old, row.source, row.next), label, {packet: helper(old)});
  checkRam(c, f.address, changedFrame(f, address, u32(BigInt(old) + BigInt(delta))).bytes);
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
  const c = open(owner, entries, 'core'), a = compile(c, 'all-eight-original-sources', normal);
  for (const row of normal) for (const old of [0x7fffffff, 0xffffffff]) {
    const r = parents(); r[row.source] = 0x80000000;
    addCase(c, a, row, DATA, old, r, 0xcd7, 'source-old-return-and-sum'); counts.sources++;
  }
  const pairs = [[0,0],[0xffffffff,1],[1,0],[0x80000000,0x80000000],[0x7ffffffe,4],[0x0f,1],[0x7fffffff,1],[0x80000002,0x7ffffffe]];
  for (const [index, flags] of FLAGS.entries()) {
    const row = normal[index % 8], r = parents(), [old, delta] = pairs[index % pairs.length];
    r[row.source] = delta;
    addCase(c, a, row, DATA, old, r, flags, 'all-valid-input-flags'); counts.flag_rows++;
  }
  const b = compile(c, 'aliases-boundary-fault', [...aliases, faultBlock]);
  for (const row of aliasRows) for (const old of [0x7fffffff, 0xffffffff]) {
    const r = parents(); for (const [i, value] of Object.entries(row.set)) r[Number(i)] = value;
    addCase(c, b, row, DATA, old, r, 0xcd7, 'original-ea-source-alias'); counts.aliases++;
  }
  const row = {pc: FAULT, source: 1, next: FAULT + 4};
  for (const address of [0,1,0x5ffd,0x5fff,0xfffffffc]) for (const old of [0x44332211,0xffffffff]) {
    const r = parents(); r[1] = 0x76543210; r[6] = address;
    addCase(c, b, row, address, old, r, 0x402, 'valid-span-boundary'); counts.boundary++;
  }
  const r = parents(); r[6] = 0x9010;
  for (const [budget, cancel, reason] of [[0,0,1],[1,1,2],[0,1,2]]) {
    const s = seed(c, FAULT, 0xcd7, r); input(c, 96, word(cancel)); run(c, b, budget, s, 'helper-free-budget-cancel', {reason, retired: 0});
    controls.push({context: c.id, kind: 'budget-cancel', budget, cancel});
  }
  input(c, 96, word(0)); let start = seed(c, 0x1900, 0x402);
  run(c, b, 2, start, 'cold-pc', {reason: 3, retired: 0}); controls.push({context: c.id, kind: 'cold-pc'});
  const faultCases = [
    {name:'first-unmapped', address:0x9010, page:0x9000, permission:null, access:1, detail:1, at:0x9010},
    {name:'first-none', address:0x9010, page:0x9000, permission:0, access:1, detail:2, at:0x9010},
    {name:'first-write-only', address:0x9010, page:0x9000, permission:2, access:1, detail:2, at:0x9010},
    {name:'first-execute-only', address:0x9010, page:0x9000, permission:4, access:1, detail:2, at:0x9010},
    {name:'cross-second-unmapped', address:0x5ffe, page:0x6000, permission:null, access:1, detail:1, at:0x6000},
    {name:'cross-second-none', address:0x5ffe, page:0x6000, permission:0, access:1, detail:2, at:0x6000},
    {name:'read-only', address:0x9010, page:0x9000, permission:1, access:2, detail:2, at:0x9010},
    {name:'read-execute', address:0x9010, page:0x9000, permission:5, access:2, detail:2, at:0x9010},
    {name:'cross-first-read-only', address:0x5ffe, page:0x5000, permission:1, access:2, detail:2, at:0x5ffe},
    {name:'cross-second-read-only', address:0x5ffe, page:0x6000, permission:1, access:2, detail:2, at:0x6000},
    ...[0xfffffffd,0xfffffffe,0xffffffff].map(address => ({name:'overflow-'+address.toString(16), address, access:1, detail:3, at:address})),
  ];
  for (const item of faultCases) {
    if (item.page === 0x9000 && !c.pages.has(0x9000)) map(c, 0x9000);
    let f;
    if (item.detail === 3) {upload(c, 0xffffffe0, Buffer.alloc(32,0x5a)); upload(c, 0, Buffer.alloc(32,0xa5));}
    else {f = frame(c,item.address,0x44332211); if (item.permission === null) unmap(c,item.page); else protect(c,item.page,item.permission);}
    const registers = parents(); registers[1] = 0x76543210; registers[6] = item.address;
    const fault = {detail:item.detail,address:item.at,access:item.access};
    const failureRam = () => {if (item.detail === 3) {checkRam(c,0xffffffe0,Buffer.alloc(32,0x5a));checkRam(c,0,Buffer.alloc(32,0xa5));} else if (item.access === 2) checkRam(c,f.address,f.bytes);};
    stopped(c,b,registers,0xcd7,fault,item.name); failureRam();
    stopped(c,b,registers,0xcd7,fault,item.name+'-unchanged',false); failureRam();
    if (item.detail === 3) continue;
    if (item.permission === null) {
      map(c,item.page); const bytes = Buffer.from(f.bytes);
      for (let i=0;i<bytes.length;i++) if (f.address+i>=item.page && f.address+i<item.page+4096) bytes[i]=0;
      f={...f,bytes};
    } else protect(c,item.page,3);
    checkRam(c,f.address,f.bytes);
    const s=cpu(c), old=f.bytes.readUInt32LE(item.address-f.address), value=u32(BigInt(old)+BigInt(s.registers[1]));
    run(c,b,1,added(s,old,1,FAULT+4),item.name+'-repair-only',{packet:helper(old)}); counts.repair_only++;
    checkRam(c,f.address,changedFrame(f,item.address,value).bytes); if (item.page===0x9000) unmap(c,0x9000);
  }
  let f=frame(c,0x5ffe,0x44332211); protect(c,0x5000,1); protect(c,0x6000,2);
  let registers=parents(); registers[1]=0x76543210; registers[6]=0x5ffe;
  const readFault={detail:2,address:0x6000,access:1}, writeFault={detail:2,address:0x5ffe,access:2};
  stopped(c,b,registers,0x402,readFault,'later-read-before-earlier-write'); stopped(c,b,registers,0x402,readFault,'later-read-unchanged',false);
  protect(c,0x6000,1); checkRam(c,f.address,f.bytes);
  stopped(c,b,registers,0x402,writeFault,'read-repair-exposes-write',false); checkRam(c,f.address,f.bytes);
  stopped(c,b,registers,0x402,writeFault,'write-unchanged',false); checkRam(c,f.address,f.bytes);
  protect(c,0x5000,3);protect(c,0x6000,3);start=cpu(c);
  run(c,b,1,added(start,0x44332211,1,FAULT+4),'both-accesses-repair-only',{packet:helper(0x44332211)}); counts.repair_only++;
  checkRam(c,f.address,changedFrame(f,0x5ffe,0xba875421).bytes);
  f=frame(c,DATA,0x44332211);protect(c,0x5000,1);registers=parents();registers[1]=0x76543210;registers[6]=DATA;
  const fault={detail:2,address:DATA,access:2}; stopped(c,b,registers,0xcd7,fault,'current-input-stopped');checkRam(c,f.address,f.bytes);
  stopped(c,b,registers,0xcd7,fault,'current-input-unchanged',false);checkRam(c,f.address,f.bytes);
  protect(c,0x5000,3);const changed=frame(c,DATA+64,0x10203040);
  input(c,16,word(0x0badcafe));input(c,20,word(0xaabbccdd));input(c,40,word(DATA+64));input(c,52,word(0x402));start=cpu(c);
  run(c,b,1,added(start,0x10203040,1,FAULT+4),'explicit-current-source-ea-flags-ram',{packet:helper(0x10203040)});counts.current_inputs++;
  checkRam(c,f.address,f.bytes);checkRam(c,changed.address,changedFrame(changed,DATA+64,0xbadbfd1d).bytes);
  synthetic(c,b);
  f=frame(c,DATA,0x44332211);registers=parents();registers[0]=0x80000000;start=seed(c,normal[0].pc,0x402,registers);
  if (owner==='replacement') run(c,a,1,start,'old-generation-neutral',{status:3});
  else {run(c,a,1,added(start,0x44332211,0,normal[0].next),'retained-current-owner',{packet:helper(0x44332211)});checkRam(c,f.address,changedFrame(f,DATA,0xc4332211).bytes);}
  const l=compile(c,'live-add-flags-old-value-consumer',[live]);liveProof(c,l);
  upload(c,normal[0].pc,normal[0].bytes.subarray(0,1));start=seed(c,live.pc,0x402);
  run(c,l,1,start,'same-byte-source-invalidated',{status:4});controls.push({context:c.id,kind:'same-byte-rwx-source-alone'});
  const compiled=compile(c,'original-c11-two-live-calls',[...kernelBlocks,...caller],['read32','store','add']);
  compilerProof(c,compiled);close(c);
}
function synthetic(c,unit) {
  const infra=record('R3MH',1,40,[2,0,1,0,0,0]);
  const rows=[
    {name:'magic',edit:b=>b.write('BAD!')},{name:'version',edit:b=>b.writeUInt16LE(2,4)},
    {name:'size',edit:b=>b.writeUInt32LE(36,8)},{name:'reserved',edit:b=>b.writeUInt32LE(1,12)},
    {name:'success-fault-field',edit:b=>b.writeUInt32LE(2,24)},{name:'overflow-success',address:0xffffffff},
    {name:'fault-value',packet:helper(1,{detail:2,address:DATA,access:1})},
    {name:'write-unmapped',packet:helper(0,{detail:1,address:DATA,access:2})},
    {name:'write-overflow',address:0xffffffff,packet:helper(0,{detail:3,address:0xffffffff,access:2})},
    {name:'invalidated-fault',status:11,packet:helper(0,{detail:2,address:DATA,access:1})},
    {name:'fault-outside-span',packet:helper(0,{detail:2,address:DATA+4,access:1})},
    {name:'fault-width',packet:helper(0,{detail:2,address:DATA,access:1}),edit:b=>b.writeUInt32LE(2,36)},
    {name:'unknown-status',status:99,detail:3},
    ...[1,2].flatMap(access=>[0xfffffffd,0xfffffffe,0xffffffff].map(address=>({name:`top-fault-${access}-${address.toString(16)}`,address:0xfffffffc,packet:helper(0,{detail:2,address,access})}))),
    {name:'infra-value',packet:infra,edit:b=>b.writeUInt32LE(1,20)},
    {name:'invalidated-infra',packet:infra,status:11},
    {name:'canonical-version-exhausted',packet:infra,detail:1},
  ];
  for (const row of rows) {
    const address=row.address??DATA,r=parents();r[0]=0x12345678;r[1]=0x89abcdef;r[6]=address;
    const f=frame(c,address>0xfffffffc?0xfffffffc:address,0x44332211),packet=Buffer.from(row.packet??helper(0x44332211));row.edit?.(packet);
    let calls=0;const ring3={...unit.ring3},name=c.owner==='resident'?'exchange_add_resident32':'exchange_add32';
    ring3[name]=(...args)=>{assert.deepEqual(args.slice(name.includes('resident')?4:0).map(v=>v>>>0),[address,0x89abcdef]);input(c,100,packet);calls++;return row.status??0;};
    const fake={...unit,name:unit.name+'-synthetic',run:new WebAssembly.Instance(unit.module,{env:{memory:c.memory},ring3}).exports.run};
    const start=seed(c,FAULT,0xcd7,r);
    run(c,fake,1,start,'nonforwarding-'+row.name,{reason:7,retired:0,fault:{detail:row.detail??2},packet,synthetic:true});
    assert.equal(calls,1);checkRam(c,f.address,f.bytes);counts.synthetic++;
  }
}
function liveProof(c,unit) {
  const f=frame(c,DATA,0xffffffff),r=parents();r[0]=1;r[1]=1;r[6]=DATA;
  let s=seed(c,live.pc,0xcd7,r);s=added(s,0xffffffff,1,0x1804);
  assert.equal(s.flags,0x457);run(c,unit,1,s,'live-add-publishes-carry-and-old-source',{packet:helper(0xffffffff)});
  const value=u32(BigInt(s.registers[0])+BigInt(s.registers[1])+1n);
  assert.equal(value,1);assert.equal(addFlags(s.registers[0],s.registers[1],s.flags,1),0x413);
  s={...s,registers:[...s.registers],pc:0x1806,flags:0x413};s.registers[0]=value;
  run(c,unit,1,s,'live-adc-consumes-add-carry-and-returned-old');
  s={...s,registers:[...s.registers],pc:0x1809};s.registers[3]=(s.registers[3]&0xffffff00|1)>>>0;
  run(c,unit,1,s,'live-setb-consumes-adc-carry');
  s={...s,registers:[...s.registers],pc:0x180c};s.registers[3]=(s.registers[3]&0xffff00ff)>>>0;
  run(c,unit,1,s,'live-seto-consumes-adc-overflow');
  s={...s,pc:0x180e};run(c,unit,2,s,'live-cold-successor',{reason:3});checkRam(c,f.address,changedFrame(f,DATA,0).bytes);
}
function compilerProof(c,unit) {
  const data=Buffer.from(Array.from({length:64},(_,i)=>(i*17+23)&255));data.set(word(0x7ffffffe),16);upload(c,0x5000,data);
  const stack=frame(c,0x7074,0x55555555),r=parents();r[4]=0x7080;
  let s=seed(c,0x3000,0xcd7,r),trace=helperTrace.length;
  const snapshots=[{label:'entry',cpu:s,arena_hex:arena(c).toString('hex')}];
  s={...s,registers:[...s.registers],pc:0x2000};s.registers[4]=0x7074;
  run(c,unit,3,s,'compiler-first-cdecl-call',{retired:3,packet:helper(0)});
  s={...s,registers:[...s.registers],pc:0x300c,flags:0xc92};s.registers[0]=0x7ffffffe;s.registers[1]=DATA;s.registers[4]=0x7078;
  run(c,unit,4,s,'compiler-first-natural-old-return-overflow',{retired:4,packet:helper(0x300c)});counts.compiler_calls++;
  for(const i of [3,5,6,7])assert.equal(s.registers[i],r[i]);data.set(word(0x80000002),16);checkRam(c,0x5000,data);
  snapshots.push({label:'first-return',cpu:cpu(c),arena_hex:arena(c).toString('hex'),cell:0x80000002,returned:0x7ffffffe});
  s={...s,registers:[...s.registers],pc:0x2000,flags:0x412};s.registers[4]=0x7074;s.registers[5]=0x7ffffffe;
  run(c,unit,5,s,'compiler-second-call-consumes-first-old-increment',{retired:5,packet:helper(0)});
  s={...s,registers:[...s.registers],pc:0x301c,flags:0x457};s.registers[0]=0x80000002;s.registers[4]=0x7078;
  run(c,unit,4,s,'compiler-second-natural-old-return-wrap',{retired:4,packet:helper(0x301c)});counts.compiler_calls++;
  for(const i of [3,6,7])assert.equal(s.registers[i],r[i]);assert.equal(s.registers[5],0x7ffffffe);data.set(word(0),16);checkRam(c,0x5000,data);
  snapshots.push({label:'second-return',cpu:cpu(c),arena_hex:arena(c).toString('hex'),cell:0,returned:0x80000002});
  s={...s,registers:[...s.registers],pc:0x3026,flags:0x486};s.registers[0]=0xfffffffc;s.registers[4]=0x7080;
  run(c,unit,4,s,'compiler-both-old-returns-consumed-guest-self-loop',{retired:4,packet:helper(0)});data.set(word(0xfffffffc),48);checkRam(c,0x5000,data);
  const stackBytes=Buffer.from(stack.bytes);stackBytes.set(words([0x301c,DATA,0x7ffffffe]),12);checkRam(c,stack.address,stackBytes);
  const expected=[
    ['store',0x707c,4,0],['store',0x7078,DATA,0],['store',0x7074,0x300c,0],
    ['read',0x7078,null,DATA],['read',0x707c,null,4],['exchange_add',DATA,4,0x7ffffffe],['read',0x7074,null,0x300c],
    ['store',0x707c,0x7ffffffe,0],['store',0x7078,DATA,0],['store',0x7074,0x301c,0],
    ['read',0x7078,null,DATA],['read',0x707c,null,0x7ffffffe],['exchange_add',DATA,0x7ffffffe,0x80000002],['read',0x7074,null,0x301c],
    ['store',0x5030,0xfffffffc,0],
  ];
  const events=helperTrace.slice(trace);assert.equal(events.length,expected.length);
  for(const [i,[kind,address,inputValue,old]] of expected.entries()){
    const e=events[i];assert.ok(e.name.startsWith(kind));assert.equal(e.address,address);assert.equal(e.input_value,inputValue);
    assert.equal(e.status,0);assert.equal(e.helper_hex,helper(old).toString('hex'));
  }
  snapshots.push({label:'final',cpu:cpu(c),arena_hex:arena(c).toString('hex'),cell:0,consumed_old_returns:0xfffffffc});
  writeFileSync(join(output,`context-${c.id}-compiler-milestones.json`),JSON.stringify({snapshots,expected_helper_chronology:expected},null,2)+'\n',{flag:'wx'});
}
function smc(owner,entries,identity) {
  const c=open(owner,entries,identity?'smc-zero-identity':'smc-changed');
  const bytes=Buffer.from([0xf0,0x0f,0xc1,0x0e,0xba,0xff,0xff,0xff,0x7f,0xeb,0]);
  const head=compile(c,'consumed-code-exchange-add',[{pc:0xc000,bytes,instructions:3}]),r=parents();
  r[1]=identity?0:1;r[6]=0xc005;
  const start=seed(c,0xc000,0xcd7,r),next=added(start,0x7fffffff,1,0xc004),value=identity?0x7fffffff:0x80000000;
  assert.equal(next.flags,identity?0x406:0xc96);
  const at=helperTrace.length;run(c,head,1,next,identity?'zero-source-identity-code-write':'changed-code-write',{reason:6,packet:helper(0x7fffffff)});counts.smc++;
  assert.equal(helperTrace.length-at,1);assert.equal(helperTrace.at(-1).status,11);
  const code=Buffer.from(bytes);code.set(word(value),5);const framed=Buffer.alloc(12);code.copy(framed);checkRam(c,0xc000,framed);
  run(c,head,1,cpu(c),'consumed-owner-stale-neutral',{status:4});
  const suffix=compile(c,'fresh-suffix-no-xadd-replay',[{pc:0xc004,bytes:code.subarray(4),instructions:2}],[]);
  const tail={...cpu(c),registers:[...cpu(c).registers],pc:0xc00b};tail.registers[2]=value;
  run(c,suffix,3,tail,'fresh-suffix-uses-committed-code',{reason:3,retired:2});
  run(c,head,1,cpu(c),'old-owner-after-fresh-capture-neutral',{status:owner==='resident'?4:3});close(c);
}
for(const owner of ['replacement','resident'])for(const entries of [false,true]){core(owner,entries);for(const identity of [false,true])smc(owner,entries,identity);}
assert.deepEqual(sourceHashes(),sourcesBefore);
assert.deepEqual(counts,{contexts:12,modules:32,runs:1040,retired:914,exchange_adds:878,helper_calls:930,read32:24,store32:28,
  sources:64,flag_rows:512,aliases:64,boundary:40,faults:128,repair_only:44,current_inputs:4,synthetic:88,smc:8,compiler_calls:8});
const result={engine:{path:enginePath,sha256:engine.sha256,bytes:engine.bytes.length},source_pins:sourcesBefore,initial_arena_sha256:hash(initial),
  compiler:{text_hex:kernel.toString('hex'),text_sha256:hash(kernel),calls:counts.compiler_calls,combined_blocks:4,combined_instructions:16},
  counts,contexts,modules,observations,helper_trace:helperTrace,public_diagnostics:diagnostics,controls,
  limits:{blocks:8,instructions:64,module_bytes:65536,captures_per_context:6,mapped_pages:12},
  claim:'four bound profiles; full 4612-byte arena comparisons include dirty FP128. Selected 32-byte data frames, 64-byte compiler data, 32-byte compiler stack and 12-byte SMC code only; no whole-RAM claim. One forwarded canonical exchange_add helper per real locked instruction, with old DWORD retained in success packet. Malformed synthetic helpers never forward or commit. The original leaf and authored caller run through two natural returns without intercall CPU/RAM reseeding; the whole leaf has no rollback guarantee. Private flat32 synchronous unshared execution only; no hardware lock, multithread C11, Windows/provider or performance claim.'};
writeFileSync(join(output,'result.json'),JSON.stringify(result,null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({output,counts,result:join(output,'result.json')}));
