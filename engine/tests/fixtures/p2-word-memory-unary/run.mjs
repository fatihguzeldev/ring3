import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../support/engine.mjs';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root);
const engine = readEngine(enginePath), SIZE = 4364, PC = 0x1000, DATA = 0x5000;
const initial = readFileSync(join(output, 'initial-arena.bin'));
assert.equal(initial.length, SIZE);
assert.deepEqual(WebAssembly.Module.imports(engine.module), []);
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const paths = [
  'engine/src/abi/arena.rs', 'engine/src/abi/memory_helper.rs',
  'engine/src/cpu/x86/ir.rs', 'engine/src/cpu/x86/decode/lower.rs',
  'engine/src/cpu/x86/decode/operands.rs', 'engine/src/cpu/x86/decode/integer.rs', 'engine/src/cpu/x86/decode/profile.rs', 'engine/src/cpu/dbt/region.rs',
  'engine/src/cpu/dbt/wasm/integer.rs', 'engine/src/cpu/dbt/wasm/memory.rs', 'engine/src/cpu/dbt/wasm/memory/word_store.rs',
  'engine/src/cpu/dbt/wasm/memory/narrow.rs', 'engine/src/memory/space.rs',
  'engine/src/process/instance.rs', 'engine/src/process/resident.rs',
  'engine/tests/cpu_word_memory_unary_wasm.rs',
  'engine/tests/fixtures/p2-word-memory-unary/run.mjs',
];
const sources = () => Object.fromEntries(paths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = sources(), modules = [];
const counts = {contexts: 0, runs: 0, retired: 0, matrix: 0, anchors: 0, addresses: 0, boundaries: 0, registers: 0, faults: 0, retries: 0, current_operands: 0, carry_consumers: 0, neg_not_chains: 0, controls: 0, self_code: 0};
const word = value => Buffer.from([value & 255, value >>> 8 & 255]);
const words = values => {const b = Buffer.alloc(values.length * 4); values.forEach((v, i) => b.writeUInt32LE(v >>> 0, i * 4)); return b;};
function record(magic, size, fields, version = 1) {
  const b = Buffer.alloc(size); b.write(magic); b.writeUInt16LE(version, 4);
  b.writeUInt16LE(1, 6); b.writeUInt32LE(size, 8);
  fields.forEach((v, i) => b.writeUInt32LE(v >>> 0, 16 + i * 4)); return b;
}
const state = c => record('R3ST', 56, [...c.registers, c.pc, c.flags]);
const exit = (reason, n, f, version = 2) => record('R3EX', 40,
  [reason, n, f?.detail ?? 0, f?.address ?? 0, f?.access ?? 0, f ? 2 : 0], version);
const helper = (version, value = 0, f) => record('R3MH', 40,
  [f ? 1 : 0, value, f?.detail ?? 0, f?.address ?? 0, f?.access ?? 0, 2], version);
const arena = c => Buffer.from(new Uint8Array(c.memory.buffer, c.base, SIZE));
function input(c, offset, bytes) {
  const expected = arena(c); expected.set(bytes, offset);
  new Uint8Array(c.memory.buffer).set(bytes, c.base + offset);
  assert.deepEqual(arena(c), expected, 'declared host input preserves the remaining full arena');
}
function host(c, name, args) {
  const expected = arena(c); assert.equal(c.api[name](...args), 0, name);
  if (name.startsWith('compile_resident')) {
    const after = arena(c); assert.equal(after.readUInt32LE(140), 1); assert.equal(after.readUInt32LE(144), 24);
    expected.set(after.subarray(140, 164), 140);
  }
  assert.deepEqual(arena(c), expected, 'full arena host effect: ' + name);
}
function upload(c, address, bytes) {input(c, 140, bytes); host(c, 'upload', [address, bytes.length]);}
function cpu(c) {
  const b = arena(c); return {registers: Array.from({length: 8}, (_, i) => b.readUInt32LE(16 + i * 4)), pc: b.readUInt32LE(48), flags: b.readUInt32LE(52)};
}
const registers = () => Array.from({length: 8}, (_, i) => ((0xa100 + i) * 65536 + 0x1234 + i) >>> 0);
function seed(c, pc, r = registers(), flags = 0xcd7) {
  const b = Buffer.alloc(140); b.set(state({registers: r, pc, flags})); b.set(exit(3, 0), 56);
  b.set(record('R3MH', 40, [0, 0xdecafbad, 0, 0, 0, 0]), 100); input(c, 0, b); return cpu(c);
}
function run(c, module, budget, next, {label, reason = 1, retired = 1, fault, helperVersion = 4, helperValue = 0, status = 0, exitVersion = 2} = {}) {
  const expected = arena(c);
  if (status === 0) {
    expected.set(state(next)); expected.set(exit(reason, retired, fault, exitVersion), 56);
    if (helperVersion !== null) expected.set(helper(helperVersion, helperValue, fault), 100);
  }
  assert.equal(module.run(c.base, c.base + 56, budget, c.base + 96), status, label);
  assert.deepEqual(arena(c), expected, 'full4364 CPU/exit/helper/FP128 effect: ' + label);
  counts.runs++; if (status === 0) counts.retired += retired;
}
function checkRam(c, address, expected) {
  for (let i = 0; i < expected.length; i++) {
    const wanted = arena(c); wanted.set(record('R3MH', 40, [0, expected[i], 0, 0, 0, 1], 2), 100);
    assert.equal(c.api.read8((address + i) >>> 0), 0);
    assert.deepEqual(arena(c), wanted, 'real RAM byte and arena at ' + (address + i).toString(16));
  }
}

function compile(c, blocks) {
  for (const block of blocks) upload(c, block.pc, block.bytes);
  input(c, 140, words(blocks.flatMap(b => c.entries ? [b.pc] : [b.pc, b.bytes.length])));
  const method = c.owner === 'resident' ? (c.entries ? 'compile_resident_entries' : 'compile_resident') : (c.entries ? 'compile_entries' : 'compile');
  host(c, method, c.entries ? [blocks.length, 0] : [blocks.length]);
  const b = arena(c), binding = c.owner === 'resident'
    ? {low: b.readUInt32LE(148), high: b.readUInt32LE(152), pointer: b.readUInt32LE(156), length: b.readUInt32LE(160)}
    : {generation: c.api.generation(), pointer: c.api.module_ptr() >>> 0, length: c.api.module_len() >>> 0};
  const bytes = Buffer.from(new Uint8Array(c.memory.buffer, binding.pointer, binding.length));
  const module = new WebAssembly.Module(bytes), imports = WebAssembly.Module.imports(module);
  const memoryOps = blocks.some(b => b.memory !== false);
  const names = [c.owner === 'resident' ? 'guard_resident' : 'guard', ...(memoryOps ? ['read16', c.owner === 'resident' ? 'store_resident16' : 'store16'] : [])];
  assert.deepEqual(imports, [{module: 'env', name: 'memory', kind: 'memory'}, ...names.map(name => ({module: 'ring3', name, kind: 'function'}))]);
  assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  const file = `module-${modules.length + 1}.wasm`; writeFileSync(join(output, file), bytes, {flag: 'wx'});
  modules.push({context: c.id, owner: c.owner, entries: c.entries, file, sha256: hash(bytes), imports: names});
  return {run: new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3: c.api}).exports.run};
}
const block = (pc, instruction) => ({pc, bytes: Buffer.concat([instruction, Buffer.from([0xeb, 0])])});
function open(owner, entries) {
  const instance = new WebAssembly.Instance(engine.module, {});
  const c = {id: ++counts.contexts, owner, entries, memory: instance.exports.memory, api: {}};
  for (const name of ['open', 'close', 'arena_ptr', 'map', 'unmap', 'protect', 'upload', 'read8', 'read16', 'store16', 'store_resident16',
    'compile', 'compile_entries', 'compile_resident', 'compile_resident_entries', 'generation', 'module_ptr', 'module_len', 'guard', 'guard_resident'])
    c.api[name] = instance.exports['ring3_abi_v1_' + name];
  assert.equal(c.api.open(8, c.id, 0x57554e41), 0); c.base = c.api.arena_ptr() >>> 0; input(c, 0, initial);
  for (const args of [[PC, 1, 7], [DATA, 2, 3], [0xa53c5000, 1, 3], [0xfffff000, 1, 3]]) host(c, 'map', args);
  return c;
}

// widened unsigned/signed arithmetic models the flags without emitter bit formulas.
function arithmetic(adding, a, b, carry, oldFlags, preserveCarry = false) {
  const signed = x => x >= 32768 ? x - 65536 : x;
  const wide = adding ? a + b + carry : a - b - carry;
  const signedWide = adding ? signed(a) + signed(b) + carry : signed(a) - signed(b) - carry;
  const nibble = adding ? a % 16 + b % 16 + carry : a % 16 - b % 16 - carry;
  const result = (wide % 65536 + 65536) % 65536;
  let ones = 0; for (let value = result % 256; value; value = Math.floor(value / 2)) ones += value % 2;
  return {result, flags: (oldFlags & 0x402) | (preserveCarry ? oldFlags & 1 : wide < 0 || wide > 65535 ? 1 : 0)
    | (ones % 2 === 0 ? 4 : 0) | (nibble < 0 || nibble > 15 ? 16 : 0)
    | (result === 0 ? 64 : 0) | (result >= 32768 ? 128 : 0)
    | (signedWide < -32768 || signedWide > 32767 ? 0x800 : 0)};
}
function unary(op, a, oldFlags) {
  if (op === 'not') return {result: 65535 - a, flags: oldFlags};
  return op === 'neg' ? arithmetic(false, 0, a, 0, oldFlags)
    : arithmetic(op === 'inc', a, 1, 0, oldFlags, true);
}
const forms = {inc: [0xff, 0], dec: [0xff, 1], not: [0xf7, 2], neg: [0xf7, 3]};
function instruction(op, tail) {
  return Buffer.from([0x66, forms[op][0], tail[0] | forms[op][1] << 3, ...tail.slice(1)]);
}
const spec = (op, address = DATA + 8) => ({op, address, bytes: instruction(op, [5, ...words([address])])});
const anchors = [
  {op: 'inc', a: 0x7fff, old: 0xcd7, result: 0x8000, flags: 0xc97},
  {op: 'inc', a: 0xffff, old: 0xcd6, result: 0, flags: 0x456},
  {op: 'inc', a: 0x0f, old: 0xcd6, result: 0x10, flags: 0x412},
  {op: 'dec', a: 0, old: 0xcd6, result: 0xffff, flags: 0x496},
  {op: 'dec', a: 0x8000, old: 0xcd7, result: 0x7fff, flags: 0xc17},
  {op: 'neg', a: 0, old: 0xcd7, result: 0, flags: 0x446},
  {op: 'neg', a: 0x8000, old: 0xcd7, result: 0x8000, flags: 0xc87},
  {op: 'neg', a: 0xffff, old: 0xcd7, result: 1, flags: 0x413},
  {op: 'not', a: 0, old: 0xcd7, result: 0xffff, flags: 0xcd7},
  {op: 'not', a: 0xffff, old: 2, result: 0, flags: 2},
  {op: 'not', a: 0x8000, old: 0x813, result: 0x7fff, flags: 0x813},
];
for (const row of anchors) assert.deepEqual(unary(row.op, row.a, row.old),
  {result: row.result, flags: row.flags}, 'literal carry/overflow/auxiliary/parity/NOT raw FLAGS anchor');
function store(c, module, row, a, label, reason = 1) {
  const before = cpu(c), expected = unary(row.op, a, before.flags);
  run(c, module, 1, {...before, pc: before.pc + row.bytes.length, flags: expected.flags}, {label, reason});
  checkRam(c, row.address, word(expected.result)); return expected;
}
function immediateCarry(op, address) {
  return {op, address, bytes: Buffer.concat([Buffer.from([0x66, 0x83, op === 'adc' ? 0x15 : 0x1d]), words([address]), Buffer.from([0])])};
}
function carryStore(c, module, row, a, label) {
  const before = cpu(c), expected = arithmetic(row.op === 'adc', a, 0, before.flags & 1, before.flags);
  run(c, module, 1, {...before, pc: before.pc + row.bytes.length, flags: expected.flags}, {label});
  checkRam(c, row.address, word(expected.result)); return expected;
}
for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries), rows = Object.keys(forms).map(op => spec(op));
  const matrix = compile(c, rows.map((row, i) => block(PC + i * 16, row.bytes)));
  for (const [i, row] of rows.entries()) for (const a of [0, 0x0f, 0xff, 0x7fff, 0x8000, 0xffff]) for (const carry of [0, 1]) {
    upload(c, row.address - 1, Buffer.concat([Buffer.from([0x55]), word(a), Buffer.from([0xaa])]));
    seed(c, PC + i * 16, registers(), 0xcd6 | carry);
    store(c, matrix, row, a, 'all4 unary forms/16bit flags/live CF/allGPRs preserved');
    checkRam(c, row.address - 1, Buffer.from([0x55])); checkRam(c, row.address + 2, Buffer.from([0xaa])); counts.matrix++;
  }
  for (let start = 0; start < anchors.length; start += 8) {
    const rows = anchors.slice(start, start + 8).map(row => ({...spec(row.op), ...row}));
    const module = compile(c, rows.map((row, i) => block(PC + 0x200 + i * 16, row.bytes)));
    for (const [i, row] of rows.entries()) {
      upload(c, row.address, word(row.a)); seed(c, PC + 0x200 + i * 16, registers(), row.old);
      assert.deepEqual(store(c, module, row, row.a, 'literal unary result and complete FLAGS anchor'), {result: row.result, flags: row.flags}); counts.anchors++;
    }
  }
  const addresses = Object.keys(forms).map(op => ({op, address: 0xa53c5008, bytes: instruction(op, [0x44, 0x8d, 0xfc])}));
  const addressModule = compile(c, addresses.map((row, i) => block(PC + 0x400 + i * 16, row.bytes)));
  for (const [i, row] of addresses.entries()) {
    const r = registers(); r[5] = 0xa53c5004; r[1] = 0x40000002;
    upload(c, row.address, word(0x1234)); seed(c, PC + 0x400 + i * 16, r);
    store(c, addressModule, row, 0x1234, 'unary scaled full32 EA and wrapped index'); counts.addresses++;
  }
  const boundaries = Object.keys(forms).flatMap(op => [spec(op, DATA + 4095), spec(op, 0xfffffffe)]);
  const boundaryModule = compile(c, boundaries.map((row, i) => block(PC + 0x500 + i * 16, row.bytes)));
  for (const [i, row] of boundaries.entries()) {
    upload(c, row.address, word(0x800f)); seed(c, PC + 0x500 + i * 16);
    store(c, boundaryModule, row, 0x800f, 'unary unaligned crosspage/last valid WORD span'); counts.boundaries++;
  }
  const registerRows = Object.keys(forms).map(op => ({op, bytes: instruction(op, [0xc0])}));
  const registerModule = compile(c, registerRows.map((row, i) => ({...block(PC + 0x600 + i * 16, row.bytes), memory: false})));
  for (const [i, row] of registerRows.entries()) {
    const r = registers(); r[0] = 0xbeef8000;
    const before = seed(c, PC + 0x600 + i * 16, r), expected = unary(row.op, 0x8000, before.flags), next = {...before, registers: [...before.registers], pc: before.pc + row.bytes.length, flags: expected.flags};
    next.registers[0] = (0xbeef0000 | expected.result) >>> 0;
    run(c, registerModule, 1, next, {label: 'unchanged register WORD unary/high16', helperVersion: null, exitVersion: 1}); counts.registers++;
  }
  assert.equal(c.api.close(), 0);
}

// separate contexts keep resident banks below the existing eight-unit capacity.
for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries), witnesses = ['inc', 'neg'].map(op => ({op, bytes: instruction(op, [7])}));
  const faultBlocks = witnesses.map((row, i) => block(PC + i * 32, Buffer.concat([Buffer.from([0xbb, 0x44, 0x33, 0x22, 0x11]), row.bytes])));
  const faultModule = compile(c, faultBlocks), r = registers(); r[7] = 0x7008;
  seed(c, PC + 5, r);
  run(c, faultModule, 0, cpu(c), {label: 'zero budget avoids unmapped unary RMW', retired: 0, helperVersion: null});
  input(c, 96, words([1]));
  run(c, faultModule, 1, cpu(c), {label: 'cancel avoids unmapped unary RMW', reason: 2, retired: 0, helperVersion: null});
  input(c, 96, words([0])); counts.controls += 2;
  for (const [index, row] of witnesses.entries()) for (const kind of ['unmapped', 'read-denied', 'write-denied', 'cross-unmapped', 'cross-read-denied', 'cross-write-denied', 'overflow']) {
    const crossing = kind.startsWith('cross'), overflow = kind === 'overflow', address = overflow ? 0xffffffff : crossing ? 0x7fff : 0x7008;
    const missing = kind.includes('unmapped'), readDenied = kind.includes('read-denied'), writeDenied = kind.includes('write-denied');
    const mapped = overflow ? 0 : crossing && !missing ? 2 : missing && !crossing ? 0 : 1;
    const unchanged = Buffer.from(crossing && missing ? [0x55, 15] : [0x55, 15, 0x80, 0xaa]);
    if (mapped) {host(c, 'map', [0x7000, mapped, 3]); upload(c, crossing ? 0x7ffe : 0x7007, unchanged);}
    if (readDenied || writeDenied) host(c, 'protect', [crossing ? 0x8000 : 0x7000, 1, readDenied ? 2 : 1]);
    if (overflow) upload(c, 0xfffffffe, Buffer.from([0x55, 15]));
    const r = registers(); r[7] = address;
    const before = seed(c, faultBlocks[index].pc, r), stopped = {...before, registers: [...before.registers], pc: before.pc + 5}; stopped.registers[3] = 0x11223344;
    const fault = {detail: overflow ? 3 : missing ? 1 : 2, address: overflow ? address : crossing ? 0x8000 : address, access: writeDenied ? 2 : 1};
    const helperVersion = writeDenied ? 4 : 2;
    run(c, faultModule, 2, stopped, {label: kind + ' after one MOV; unary flags uncommitted', reason: 5, retired: 1, fault, helperVersion});
    run(c, faultModule, 1, stopped, {label: kind + ' unchanged unary retry', reason: 5, retired: 0, fault, helperVersion}); counts.faults += 2;
    if (!readDenied && mapped) checkRam(c, crossing ? 0x7ffe : 0x7007, unchanged);
    if (overflow) checkRam(c, 0xfffffffe, Buffer.from([0x55, 15]));
    if (!overflow) {
      if (missing) host(c, 'map', [crossing ? 0x8000 : 0x7000, 1, 3]);
      else host(c, 'protect', [crossing ? 0x8000 : 0x7000, 1, 3]);
      assert.deepEqual(cpu(c), stopped, 'map/protect-only repair preserves stopped CPU and old FLAGS');
      if (mapped) checkRam(c, crossing ? 0x7ffe : 0x7007, unchanged);
      const a = missing ? crossing ? 15 : 0 : 0x800f; checkRam(c, address, word(a));
      store(c, faultModule, {...row, address}, a, 'same unary RMW continues without MOV replay'); counts.retries++;
      host(c, 'unmap', [0x7000, crossing ? 2 : 1]);
    }
  }
  const changed = {...witnesses[1], address: DATA + 12}, freshR = registers(); freshR[7] = changed.address;
  upload(c, changed.address - 1, Buffer.from([0x55, 0x34, 0x12, 0xaa])); host(c, 'protect', [DATA, 1, 1]);
  const stoppedFresh = seed(c, faultBlocks[1].pc + 5, freshR), freshFault = {detail: 2, address: changed.address, access: 2};
  run(c, faultModule, 1, stoppedFresh, {label: 'NEG write fault before explicit data change', reason: 5, retired: 0, fault: freshFault}); counts.faults++;
  checkRam(c, changed.address - 1, Buffer.from([0x55, 0x34, 0x12, 0xaa])); host(c, 'protect', [DATA, 1, 3]); upload(c, changed.address, word(0xffff));
  assert.deepEqual(cpu(c), stoppedFresh, 'separate declared RAM change does not reseed stopped CPU');
  assert.deepEqual(store(c, faultModule, changed, 0xffff, 'NEG retry reads the current changed WORD'), {result: 1, flags: 0x413});
  checkRam(c, changed.address - 1, Buffer.from([0x55])); checkRam(c, changed.address + 2, Buffer.from([0xaa])); counts.retries++; counts.current_operands++;

  const inc = spec('inc'), adc = immediateCarry('adc', DATA + 10), dec = spec('dec'), sbb = immediateCarry('sbb', DATA + 10);
  const carryCode = Buffer.concat([Buffer.from([0xf9]), inc.bytes, Buffer.from([0x0f, 0x92, 0xc2]), adc.bytes,
    Buffer.from([0xf8]), dec.bytes, Buffer.from([0x0f, 0x92, 0xc6]), sbb.bytes]);
  const carryModule = compile(c, [block(PC + 0x100, carryCode)]);
  upload(c, DATA + 7, Buffer.from([0x55, 0xff, 0xff, 0, 0, 0xaa])); host(c, 'protect', [DATA, 1, 1]);
  const beforeCarry = seed(c, PC + 0x100), stopped = {...beforeCarry, pc: beforeCarry.pc + 1, flags: beforeCarry.flags | 1};
  const fault = {detail: 2, address: DATA + 8, access: 2};
  run(c, carryModule, 2, stopped, {label: 'STC retires then INC write fault preserves live CF and dirty FLAGS', reason: 5, retired: 1, fault});
  run(c, carryModule, 1, stopped, {label: 'unchanged INC consumer write-fault retry', reason: 5, retired: 0, fault}); counts.faults += 2;
  checkRam(c, DATA + 7, Buffer.from([0x55, 0xff, 0xff, 0, 0, 0xaa])); host(c, 'protect', [DATA, 1, 3]); assert.deepEqual(cpu(c), stopped); counts.retries++;
  assert.deepEqual(store(c, carryModule, inc, 0xffff, 'INC wrap preserves set CF for ADC'), {result: 0, flags: 0x457});
  let before = cpu(c), next = {...before, registers: [...before.registers], pc: before.pc + 3}; next.registers[2] = ((before.registers[2] & 0xffffff00) | 1) >>> 0;
  run(c, carryModule, 1, next, {label: 'SETB DL sees preserved INC carry', helperVersion: null});
  assert.deepEqual(carryStore(c, carryModule, adc, 0, 'ADC high consumes INC-preserved CF without reseed'), {result: 1, flags: 0x402});
  before = cpu(c); run(c, carryModule, 1, {...before, pc: before.pc + 1, flags: before.flags & ~1}, {label: 'CLC establishes clear live CF', helperVersion: null});
  assert.deepEqual(store(c, carryModule, dec, 0, 'DEC underflow preserves clear CF for SBB'), {result: 0xffff, flags: 0x496});
  before = cpu(c); next = {...before, registers: [...before.registers], pc: before.pc + 3}; next.registers[2] = (before.registers[2] & 0xffff00ff) >>> 0;
  run(c, carryModule, 1, next, {label: 'SETB DH sees preserved clear DEC carry', helperVersion: null});
  assert.deepEqual(carryStore(c, carryModule, sbb, 1, 'SBB high consumes DEC-preserved CF without reseed'), {result: 1, flags: 0x402});
  checkRam(c, DATA + 7, Buffer.from([0x55, 0xff, 0xff, 1, 0, 0xaa])); counts.carry_consumers++;

  const neg = spec('neg'), not = spec('not'), zero = spec('neg', DATA + 10);
  const observe = Buffer.from([0x0f, 0x90, 0xc0, 0x0f, 0x92, 0xc1]), observeZero = Buffer.from([0x0f, 0x94, 0xc2, 0x0f, 0x92, 0xc3]);
  const negModule = compile(c, [block(PC + 0x200, Buffer.concat([neg.bytes, not.bytes, observe, zero.bytes, observeZero]))]);
  upload(c, DATA + 7, Buffer.from([0x55, 0, 0x80, 0, 0, 0xaa])); seed(c, PC + 0x200);
  assert.deepEqual(store(c, negModule, neg, 0x8000, 'NEG minimum produces live CF and OF'), {result: 0x8000, flags: 0xc87});
  assert.deepEqual(store(c, negModule, not, 0x8000, 'NOT changes sign but preserves complete live NEG FLAGS'), {result: 0x7fff, flags: 0xc87});
  before = cpu(c); next = {...before, registers: [...before.registers], pc: before.pc + observe.length};
  for (const index of [0, 1]) next.registers[index] = ((before.registers[index] & 0xffffff00) | 1) >>> 0;
  run(c, negModule, 2, next, {label: 'SETO AL/SETB CL consume NOT-preserved live FLAGS', retired: 2, helperVersion: null});
  assert.deepEqual(store(c, negModule, zero, 0, 'NEG zero clears prior carry and overflow'), {result: 0, flags: 0x446});
  before = cpu(c); next = {...before, registers: [...before.registers], pc: before.pc + observeZero.length};
  next.registers[2] = ((before.registers[2] & 0xffffff00) | 1) >>> 0; next.registers[3] = (before.registers[3] & 0xffffff00) >>> 0;
  run(c, negModule, 2, next, {label: 'SETZ DL/SETB BL consume current NEG-zero FLAGS', retired: 2, helperVersion: null});
  checkRam(c, DATA + 7, Buffer.from([0x55, 0xff, 0x7f, 0, 0, 0xaa])); counts.neg_not_chains++;
  assert.equal(c.api.close(), 0);
}

// only NEG has unchanged whole-WORD outputs; INC/DEC/NOT always change a bit.
for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) for (const kind of ['inc', 'dec', 'not', 'neg', 'neg-zero', 'neg-min']) {
  const c = open(owner, entries), op = kind.startsWith('neg') ? 'neg' : kind, r = registers();
  let bytes = instruction(op, [7]), address = PC;
  if (kind === 'neg-zero') {address = PC + 5; bytes = instruction('neg', [5, ...words([address])]);}
  if (kind === 'neg-min') {address = PC + 5; bytes = instruction('neg', [0x9f, 0, 0, 0, 0x80]);}
  r[7] = kind === 'neg-min' ? (address - 0x80000000) >>> 0 : address;
  const a = bytes.readUInt16LE(address - PC), row = {op, bytes, address}, module = compile(c, [block(PC, bytes)]);
  if (kind === 'neg-zero' || kind === 'neg-min') assert.equal(a, kind === 'neg-zero' ? 0 : 0x8000);
  seed(c, PC, r); const expected = store(c, module, row, a, 'own-code unary write commits FLAGS and one retirement', 6);
  assert.equal(expected.result === a, kind === 'neg-zero' || kind === 'neg-min');
  run(c, module, 1, cpu(c), {label: 'stale unary owner completely neutral', status: 4, retired: 0});
  const nextPC = PC + bytes.length, tail = compile(c, [{pc: nextPC, bytes: Buffer.from([0xeb, 0]), memory: false}]);
  run(c, tail, 2, {...cpu(c), pc: nextPC + 2}, {label: 'fresh suffix continues without unary RMW replay', reason: 3, retired: 1, helperVersion: null, exitVersion: 1});
  checkRam(c, address, word(expected.result)); assert.equal(c.api.close(), 0); counts.self_code++;
}
assert.deepEqual(sources(), sourcePins, 'risk-relevant sources stayed fixed');
assert.equal(counts.contexts, 32); assert.equal(counts.runs, 604); assert.equal(counts.retired, 516);
assert.equal(counts.matrix, 192); assert.equal(counts.anchors, 44); assert.equal(counts.addresses, 16);
assert.equal(counts.boundaries, 32); assert.equal(counts.registers, 16); assert.equal(counts.faults, 124);
assert.equal(counts.retries, 56); assert.equal(counts.current_operands, 4); assert.equal(counts.carry_consumers, 4);
assert.equal(counts.neg_not_chains, 4); assert.equal(counts.controls, 8); assert.equal(counts.self_code, 24); assert.equal(modules.length, 84);
const result = {status: 'ok', engine_sha256: engine.sha256, source_pins: sourcePins, counts, modules,
  limits: ['finite exact single66 WORD memory INC/DEC FF/0,/1 and NOT/NEG F7/2,/3 in four bound profiles',
    'real read16/store16 paths and selected RAM/canary checks; no arbitrary-host rollback claim',
    'full4364 arena preserves FP128; NOT preserves complete admitted raw FLAGS, INC/DEC preserve CF',
    'changed code writes for all4; true same-byte NEG zero/minimum only, the other unary operations have no WORD fixed point',
    'no exhaustive ISA, hardware, throughput, browser or game claim']};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2) + '\n', {flag: 'wx'});
console.log(JSON.stringify({status: result.status, ...counts, modules: modules.length, output}));
