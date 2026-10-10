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
  'engine/src/cpu/x86/decode/operands.rs', 'engine/src/cpu/x86/decode/profile.rs', 'engine/src/cpu/dbt/region.rs',
  'engine/src/cpu/dbt/wasm/integer.rs', 'engine/src/cpu/dbt/wasm/memory/word_store.rs',
  'engine/src/cpu/dbt/wasm/memory/narrow.rs', 'engine/src/memory/space.rs',
  'engine/src/process/instance.rs', 'engine/src/process/resident.rs',
  'engine/tests/cpu_word_memory_immediate_arithmetic_store_wasm.rs',
  'engine/tests/fixtures/p2-word-memory-immediate-arithmetic-store/run.mjs',
];
const sources = () => Object.fromEntries(paths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = sources(), modules = [];
const counts = {contexts: 0, runs: 0, retired: 0, matrix: 0, anchors: 0, payloads: 0, boundaries: 0, faults: 0, retries: 0, packed: 0, chains: 0, controls: 0, self_code: 0};
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

// widened arithmetic; ADD/SUB deliberately ignore incoming CF.
function arithmetic(op, a, b, carry, oldFlags) {
  carry = op === 'adc' || op === 'sbb' ? carry : 0;
  const adding = op === 'add' || op === 'adc', signed = x => x >= 32768 ? x - 65536 : x;
  const wide = adding ? a + b + carry : a - b - carry;
  const signedWide = adding ? signed(a) + signed(b) + carry : signed(a) - signed(b) - carry;
  const nibble = adding ? a % 16 + b % 16 + carry : a % 16 - b % 16 - carry;
  const result = (wide % 65536 + 65536) % 65536;
  let ones = 0; for (let value = result % 256; value; value = Math.floor(value / 2)) ones += value % 2;
  return {result, flags: (oldFlags & 0x402) | (wide < 0 || wide > 65535 ? 1 : 0)
    | (ones % 2 === 0 ? 4 : 0) | (nibble < 0 || nibble > 15 ? 16 : 0)
    | (result === 0 ? 64 : 0) | (result >= 32768 ? 128 : 0)
    | (signedWide < -32768 || signedWide > 32767 ? 0x800 : 0)};
}
const extensions = {add: 0, adc: 2, sub: 5, sbb: 3};
const normalization = [[0, 0], [0x7f, 0x7f], [0x80, 0xff80], [0xff, 0xffff]];
for (const [raw, expected] of normalization)
  assert.equal(raw < 128 ? raw : raw + 0xff00, expected, 'literal signed imm8 normalization');
function instruction(op, encoding, raw, tail) {
  assert.ok(encoding === 0x81 || encoding === 0x83);
  return Buffer.concat([Buffer.from([0x66, encoding, tail[0] | extensions[op] << 3]),
    Buffer.from(tail.slice(1)), encoding === 0x81 ? word(raw) : Buffer.from([raw])]);
}
function absolute(op, encoding, raw, address) {
  return instruction(op, encoding, raw, [5, ...words([address])]);
}
const spec = (op, encoding, raw, immediate, address = DATA + 8) => ({op, encoding, raw, immediate, address, bytes: absolute(op, encoding, raw, address)});
const anchors = [
  {...spec('add', 0x81, 1, 1), a: 0x7fff, carry: 1, result: 0x8000, flags: 0xc96},
  {...spec('add', 0x83, 0x80, 0xff80), a: 0x7f, carry: 1, result: 0xffff, flags: 0x486},
  {...spec('add', 0x83, 0xff, 0xffff), a: 1, carry: 0, result: 0, flags: 0x457},
  {...spec('adc', 0x81, 0, 0), a: 0x7fff, carry: 1, result: 0x8000, flags: 0xc96},
  {...spec('adc', 0x83, 0xff, 0xffff), a: 0, carry: 1, result: 0, flags: 0x457},
  {...spec('adc', 0x83, 0x80, 0xff80), a: 0, carry: 0, result: 0xff80, flags: 0x482},
  {...spec('sub', 0x81, 1, 1), a: 0x8000, carry: 1, result: 0x7fff, flags: 0xc16},
  {...spec('sub', 0x83, 0xff, 0xffff), a: 0x7fff, carry: 0, result: 0x8000, flags: 0xc87},
  {...spec('sub', 0x83, 0x80, 0xff80), a: 0, carry: 1, result: 0x80, flags: 0x403},
  {...spec('sbb', 0x81, 0xffff, 0xffff), a: 0xffff, carry: 1, result: 0xffff, flags: 0x497},
  {...spec('sbb', 0x83, 0xff, 0xffff), a: 0, carry: 1, result: 0, flags: 0x457},
  {...spec('sbb', 0x83, 0, 0), a: 0x8000, carry: 1, result: 0x7fff, flags: 0xc16},
];
for (const row of anchors) assert.deepEqual(arithmetic(row.op, row.a, row.immediate, row.carry, 0xcd6 | row.carry),
  {result: row.result, flags: row.flags}, 'literal immediate arithmetic flag anchor');
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
  assert.equal(c.api.open(8, c.id, 0x57434152), 0); c.base = c.api.arena_ptr() >>> 0; input(c, 0, initial);
  for (const args of [[PC, 1, 7], [DATA, 2, 3], [0xa53c5000, 1, 3], [0xfffff000, 1, 3]]) host(c, 'map', args);
  return c;
}
function store(c, module, row, a, label, reason = 1) {
  const before = cpu(c), expected = arithmetic(row.op, a, row.immediate, before.flags & 1, before.flags);
  run(c, module, 1, {...before, pc: before.pc + row.bytes.length, flags: expected.flags}, {label, reason});
  checkRam(c, row.address, word(expected.result)); return expected;
}

for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries);
  for (const op of Object.keys(extensions)) {
    const vectors = [
      ...[[0xffff, 1], [1, 0x7fff], [0x8000, 0x8000], [0, 0xffff]]
        .map(([a, immediate]) => ({...spec(op, 0x81, immediate, immediate), a})),
      ...[[0x7fff, 0, 0], [0xff81, 0x7f, 0x7f], [0x7f, 0x80, 0xff80], [0, 0xff, 0xffff]]
        .map(([a, raw, immediate]) => ({...spec(op, 0x83, raw, immediate), a})),
    ];
    const blocks = vectors.map((row, i) => block(PC + i * 16, row.bytes)), module = compile(c, blocks);
    for (const [i, row] of vectors.entries()) for (const carry of [0, 1]) {
      upload(c, row.address - 1, Buffer.concat([Buffer.from([0x55]), word(row.a), Buffer.from([0xaa])]));
      seed(c, blocks[i].pc, registers(), (i % 2 ? 0xcd6 : 2) | carry);
      store(c, module, row, row.a, 'all8 immediate forms/bothCF/fullGPR preservation');
      checkRam(c, row.address - 1, Buffer.from([0x55])); checkRam(c, row.address + 2, Buffer.from([0xaa])); counts.matrix++;
    }
  }
  for (let start = 0; start < anchors.length; start += 8) {
    const rows = anchors.slice(start, start + 8), blocks = rows.map((row, i) => block(PC + 0x200 + i * 16, row.bytes));
    const module = compile(c, blocks);
    for (const [i, row] of rows.entries()) {
      upload(c, row.address, word(row.a)); seed(c, blocks[i].pc, registers(), 0xcd6 | row.carry);
      assert.deepEqual(store(c, module, row, row.a, 'literal imm16/signedimm8 carry/OF/AF/PF anchor'), {result: row.result, flags: row.flags}); counts.anchors++;
    }
  }
  const payloads = Object.keys(extensions).flatMap((op, i) => [
    {op, encoding: 0x81, raw: [0x6766, 0xf3f0, 0x6667, 0xf0f3][i], immediate: [0x6766, 0xf3f0, 0x6667, 0xf0f3][i]},
    {op, encoding: 0x83, raw: [0x66, 0x67, 0xf0, 0xf3][i], immediate: [0x66, 0x67, 0xfff0, 0xfff3][i]},
  ].map(row => ({...row, address: 0xa53c5008, bytes: instruction(row.op, row.encoding, row.raw, [0x44, 0x8d, 0xfc])})));
  const payloadModule = compile(c, payloads.map((row, i) => block(PC + 0x400 + i * 16, row.bytes)));
  for (const [i, row] of payloads.entries()) {
    const r = registers(); r[5] = 0xa53c5004; r[1] = 0x40000002;
    upload(c, row.address, word(0x1234)); seed(c, PC + 0x400 + i * 16, r);
    store(c, payloadModule, row, 0x1234, 'prefix-looking immediate payload/full32 scaled EA'); counts.payloads++;
  }
  const boundaries = [spec('adc', 0x81, 1, 1, DATA + 4095), spec('sbb', 0x83, 0x80, 0xff80, DATA + 4095),
    spec('adc', 0x81, 1, 1, 0xfffffffe), spec('sbb', 0x83, 0x80, 0xff80, 0xfffffffe)];
  const boundaryModule = compile(c, boundaries.map((row, i) => block(PC + 0x600 + i * 16, row.bytes)));
  for (const [i, row] of boundaries.entries()) {
    upload(c, row.address, word(0x800f)); seed(c, PC + 0x600 + i * 16);
    store(c, boundaryModule, row, 0x800f, 'unaligned crosspage/last valid WORD span'); counts.boundaries++;
  }
  assert.equal(c.api.close(), 0);
}

// separate contexts keep resident banks below the existing eight-unit capacity.
for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries), witnesses = [spec('adc', 0x81, 1, 1), spec('sbb', 0x83, 0x80, 0xff80)];
  for (const row of witnesses) row.bytes = instruction(row.op, row.encoding, row.raw, [7]);
  const faultBlocks = witnesses.map((row, i) => block(PC + i * 32, Buffer.concat([Buffer.from([0xbb, 0x44, 0x33, 0x22, 0x11]), row.bytes])));
  const faultModule = compile(c, faultBlocks), r = registers(); r[7] = 0x7008;
  seed(c, PC + 5, r);
  run(c, faultModule, 0, cpu(c), {label: 'zero budget avoids unmapped immediate RMW', retired: 0, helperVersion: null});
  input(c, 96, words([1]));
  run(c, faultModule, 1, cpu(c), {label: 'cancel avoids unmapped immediate RMW', reason: 2, retired: 0, helperVersion: null});
  input(c, 96, words([0])); counts.controls += 2;
  for (const [index, row] of witnesses.entries()) for (const kind of ['unmapped', 'read-denied', 'write-denied', 'cross-unmapped', 'cross-read-denied', 'cross-write-denied', 'overflow']) {
    const crossing = kind.startsWith('cross'), overflow = kind === 'overflow', address = overflow ? 0xffffffff : crossing ? 0x7fff : 0x7008;
    const missing = kind.includes('unmapped'), readDenied = kind.includes('read-denied'), writeDenied = kind.includes('write-denied');
    const mapped = overflow ? 0 : crossing && !missing ? 2 : missing && !crossing ? 0 : 1;
    if (mapped) {host(c, 'map', [0x7000, mapped, 3]); upload(c, crossing ? 0x7ffe : 0x7007, Buffer.from(crossing && missing ? [0x55, 15] : [0x55, 15, 0x80, 0xaa]));}
    if (readDenied || writeDenied) host(c, 'protect', [crossing ? 0x8000 : 0x7000, 1, readDenied ? 2 : 1]);
    if (overflow) upload(c, 0xfffffffe, Buffer.from([0x55, 15]));
    const r = registers(); r[7] = address;
    const before = seed(c, faultBlocks[index].pc, r), stopped = {...before, registers: [...before.registers], pc: before.pc + 5}; stopped.registers[3] = 0x11223344;
    const fault = {detail: overflow ? 3 : missing ? 1 : 2, address: overflow ? address : crossing ? 0x8000 : address, access: writeDenied ? 2 : 1};
    const helperVersion = writeDenied ? 4 : 2;
    run(c, faultModule, 2, stopped, {label: kind + ' after one MOV', reason: 5, retired: 1, fault, helperVersion});
    run(c, faultModule, 1, stopped, {label: kind + ' unchanged retry', reason: 5, retired: 0, fault, helperVersion}); counts.faults += 2;
    if (!readDenied && mapped) checkRam(c, crossing ? 0x7ffe : 0x7007, Buffer.from(crossing && missing ? [0x55, 15] : [0x55, 15, 0x80, 0xaa]));
    if (overflow) checkRam(c, 0xfffffffe, Buffer.from([0x55, 15]));
    if (!overflow) {
      if (missing) host(c, 'map', [crossing ? 0x8000 : 0x7000, 1, 3]);
      else host(c, 'protect', [crossing ? 0x8000 : 0x7000, 1, 3]);
      assert.deepEqual(cpu(c), stopped, 'map/protect-only repair preserves CPU and carry');
      const a = missing ? crossing ? 15 : 0 : 0x800f; checkRam(c, address, word(a));
      store(c, faultModule, {...row, address}, a, 'repair resumes same immediate RMW without MOV replay'); counts.retries++;
      host(c, 'unmap', [0x7000, crossing ? 2 : 1]);
    }
  }
  const packed = [spec('add', 0x81, 1, 1, 0x6ffd), spec('adc', 0x83, 0, 0, 0x6fff),
    spec('sub', 0x83, 1, 1, 0x6ffd), spec('sbb', 0x81, 0, 0, 0x6fff)];
  const ofChains = [
    [spec('adc', 0x81, 0xffff, 0xffff, DATA + 16), spec('adc', 0x83, 0, 0, DATA + 18)],
    [spec('sbb', 0x83, 0xff, 0xffff, DATA + 16), spec('sbb', 0x81, 0, 0, DATA + 18)],
  ];
  const chainBlocks = [block(PC + 0x100, Buffer.concat(packed.map(row => row.bytes))),
    ...ofChains.map((rows, i) => block(PC + 0x180 + i * 64,
      Buffer.concat([...rows.map(row => row.bytes), Buffer.from([0x0f, 0x92, 0xc3, 0x0f, 0x90, 0xc7])]))),
  ];
  const chainModule = compile(c, chainBlocks);
  host(c, 'map', [0x7000, 1, 3]); upload(c, 0x6ffd, words([0x800fffff])); host(c, 'protect', [0x7000, 1, 1]);
  seed(c, chainBlocks[0].pc);
  store(c, chainModule, packed[0], 0xffff, 'packed ADD low1 produces carry'); assert.equal(cpu(c).flags & 1, 1);
  const stopped = cpu(c), fault = {detail: 2, address: 0x7000, access: 2};
  run(c, chainModule, 1, stopped, {label: 'packed high ADC write fault retains low carry', reason: 5, retired: 0, fault});
  run(c, chainModule, 1, stopped, {label: 'packed unchanged high-limb retry', reason: 5, retired: 0, fault}); counts.faults += 2;
  checkRam(c, 0x6ffd, words([0x800f0000])); host(c, 'protect', [0x7000, 1, 3]); assert.deepEqual(cpu(c), stopped);
  store(c, chainModule, packed[1], 0x800f, 'packed high ADC consumes retained carry after permissions-only repair'); counts.retries++;
  checkRam(c, 0x6ffd, words([0x80100000]));
  store(c, chainModule, packed[2], 0, 'packed SUB low1 produces borrow'); assert.equal(cpu(c).flags & 1, 1);
  store(c, chainModule, packed[3], 0x8010, 'packed high SBB consumes borrow without reseed');
  checkRam(c, 0x6ffd, words([0x800fffff])); counts.packed++;
  for (const [i, rows] of ofChains.entries()) {
    const high = i === 0 ? 0x7fff : 0x8000;
    upload(c, DATA + 16, Buffer.concat([word(0), word(high)])); seed(c, chainBlocks[i + 1].pc);
    store(c, chainModule, rows[0], 0, 'mixed81/83 low ffff+CF equality wrap'); assert.equal(cpu(c).flags & 1, 1);
    store(c, chainModule, rows[1], high, 'mixed81/83 high limb current carry and OF');
    checkRam(c, DATA + 16, words([i === 0 ? 0x80000000 : 0x7fff0000]));
    const before = cpu(c), next = {...before, registers: [...before.registers], pc: before.pc + 6};
    assert.equal(before.flags & 1, 0); assert.equal(before.flags & 0x800, 0x800);
    next.registers[3] = ((before.registers[3] & 0xffff0000) | 0x100) >>> 0;
    run(c, chainModule, 2, next, {label: 'SETB/SETO consume current high-limb flags', retired: 2, helperVersion: null}); counts.chains++;
  }
  assert.equal(c.api.close(), 0);
}
for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) for (const op of ['adc', 'sbb']) for (const same of [false, true]) {
  const c = open(owner, entries), encoding = op === 'adc' ? 0x81 : 0x83;
  const raw = same ? (op === 'adc' ? 0xffff : 0xff) : (op === 'adc' ? 1 : 0x7f);
  const immediate = same ? 0xffff : raw, bytes = instruction(op, encoding, raw, [7]);
  const address = PC + (op === 'adc' ? 3 : 2), a = bytes.readUInt16LE(address - PC);
  const row = {op, encoding, raw, immediate, bytes, address}, module = compile(c, [block(PC, bytes)]), r = registers(); r[7] = address;
  seed(c, PC, r); const expected = store(c, module, row, a, 'own immediate bytes commit once using decoded original source', 6);
  assert.equal(expected.result === a, same);
  run(c, module, 1, cpu(c), {label: 'stale immediate owner completely neutral', status: 4, retired: 0});
  const nextPC = PC + bytes.length, tail = compile(c, [{pc: nextPC, bytes: Buffer.from([0xeb, 0]), memory: false}]);
  run(c, tail, 2, {...cpu(c), pc: nextPC + 2}, {label: 'fresh suffix continues without immediate RMW replay', reason: 3, retired: 1, helperVersion: null, exitVersion: 1});
  checkRam(c, address, word(expected.result)); assert.equal(c.api.close(), 0); counts.self_code++;
}
assert.deepEqual(sources(), sourcePins, 'risk-relevant sources stayed fixed');
assert.equal(counts.contexts, 24); assert.equal(counts.matrix, 256); assert.equal(counts.anchors, 48);
assert.equal(counts.runs, 616); assert.equal(counts.retired, 536);
assert.equal(counts.payloads, 32); assert.equal(counts.boundaries, 16); assert.equal(counts.faults, 120);
assert.equal(counts.retries, 52); assert.equal(counts.packed, 4); assert.equal(counts.chains, 8);
assert.equal(counts.controls, 8); assert.equal(counts.self_code, 16); assert.equal(modules.length, 72);
const result = {status: 'ok', engine_sha256: engine.sha256, source_pins: sourcePins, counts, modules,
  limits: ['finite exact single66 WORD memory immediate ADD/ADC/SUB/SBB81/83 in four bound profiles',
    'real read16/store16 paths and selected RAM/canary checks; no arbitrary-host rollback claim',
    'full4364 arena preserves FP128; decoded immediate remains fixed through own-code modification',
    'no exhaustive ISA, hardware, throughput, browser or game claim']};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2) + '\n', {flag: 'wx'});
console.log(JSON.stringify({status: result.status, ...counts, modules: modules.length, output}));
