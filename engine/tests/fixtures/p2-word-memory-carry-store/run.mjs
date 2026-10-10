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
  'engine/src/cpu/x86/decode/profile.rs', 'engine/src/cpu/dbt/region.rs',
  'engine/src/cpu/dbt/wasm/integer.rs', 'engine/src/cpu/dbt/wasm/memory/word_store.rs',
  'engine/src/cpu/dbt/wasm/memory/narrow.rs', 'engine/src/memory/space.rs',
  'engine/src/process/instance.rs', 'engine/src/process/resident.rs',
  'engine/tests/cpu_word_memory_carry_store_wasm.rs',
  'engine/tests/fixtures/p2-word-memory-carry-store/run.mjs',
];
const sources = () => Object.fromEntries(paths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = sources(), modules = [];
const counts = {contexts: 0, runs: 0, retired: 0, matrix: 0, anchors: 0, boundaries: 0, aliases: 0, faults: 0, retries: 0, chains: 0, controls: 0, self_code: 0};
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

// widened arithmetic is independent of the emitter's bitwise flag formulas.
function arithmetic(op, a, b, carry, oldFlags) {
  const signed = x => x >= 0x8000 ? x - 0x10000 : x;
  const wide = op === 'adc' ? a + b + carry : a - b - carry;
  const signedWide = op === 'adc' ? signed(a) + signed(b) + carry : signed(a) - signed(b) - carry;
  const nibble = op === 'adc' ? (a % 16) + (b % 16) + carry : (a % 16) - (b % 16) - carry;
  const result = (wide % 65536 + 65536) % 65536;
  let ones = 0; for (let value = result % 256; value; value = Math.floor(value / 2)) ones += value % 2;
  const flags = (oldFlags & 0x402) | (wide < 0 || wide > 65535 ? 1 : 0)
    | (ones % 2 === 0 ? 4 : 0) | (nibble < 0 || nibble > 15 ? 16 : 0)
    | (result === 0 ? 64 : 0) | (result >= 32768 ? 128 : 0)
    | (signedWide < -32768 || signedWide > 32767 ? 0x800 : 0);
  return {result, flags};
}
const anchors = {
  adc: [[0xffff, 0, 0, 0xffff, 0x486], [0xffff, 0, 1, 0, 0x457], [0, 0xffff, 1, 0, 0x457],
    [0x7fff, 0, 1, 0x8000, 0xc96], [0x8000, 0x8000, 0, 0, 0xc47],
    [15, 0, 1, 16, 0x412], [255, 0, 1, 256, 0x416], [0, 0, 0, 0, 0x446], [0, 2, 1, 3, 0x406]],
  sbb: [[0, 0, 0, 0, 0x446], [0, 0, 1, 0xffff, 0x497], [0xffff, 0xffff, 0, 0, 0x446],
    [0xffff, 0xffff, 1, 0xffff, 0x497], [0, 0xffff, 1, 0, 0x457],
    [0x8000, 0, 1, 0x7fff, 0xc16], [0x7fff, 0xffff, 0, 0x8000, 0xc87],
    [16, 0, 1, 15, 0x416], [256, 0, 1, 255, 0x416], [3, 1, 0, 2, 0x402]],
};
for (const [op, rows] of Object.entries(anchors)) for (const [a, b, carry, result, flags] of rows)
  assert.deepEqual(arithmetic(op, a, b, carry, 0xcd6 | carry), {result, flags}, 'literal ' + op + ' flag anchor');
const opcode = op => op === 'adc' ? 0x11 : 0x19;
const absolute = (op, source, address) => Buffer.concat([Buffer.from([0x66, opcode(op), 5 | source << 3]), words([address])]);
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
function store(c, module, instruction, op, address, a, label, reason = 1) {
  const before = cpu(c), source = instruction[2] >>> 3 & 7;
  const expected = arithmetic(op, a, before.registers[source] & 0xffff, before.flags & 1, before.flags);
  run(c, module, 1, {...before, pc: before.pc + instruction.length, flags: expected.flags}, {label, reason});
  checkRam(c, address, word(expected.result)); return expected;
}

for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries);
  for (const op of ['adc', 'sbb']) {
    const blocks = Array.from({length: 8}, (_, source) => block(PC + source * 16, absolute(op, source, DATA + 8)));
    const module = compile(c, blocks);
    for (let source = 0; source < 8; source++) for (const carry of [0, 1]) {
      const a = [0xffff, 0, 0x7fff, 0x8000, 1, 0xffff, 0xff00, 0xaaaa][source];
      const b = [0, 0xffff, 1, 0x8000, 0x7fff, 15, 255, 0x1234][source];
      const r = registers(); r[source] = ((r[source] & 0xffff0000) | b) >>> 0;
      upload(c, DATA + 7, Buffer.concat([Buffer.from([0x55]), word(a), Buffer.from([0xaa])]));
      seed(c, blocks[source].pc, r, (source % 2 ? 0xcd6 : 2) | carry);
      store(c, module, blocks[source].bytes.subarray(0, 7), op, DATA + 8, a, 'all8 current sources/bothCF/high16');
      checkRam(c, DATA + 7, Buffer.from([0x55])); checkRam(c, DATA + 10, Buffer.from([0xaa])); counts.matrix++;
    }
    for (const [a, b, carry, result, flags] of anchors[op]) {
      const r = registers(); r[0] = (0xbeef0000 | b) >>> 0; upload(c, DATA + 8, word(a)); seed(c, PC, r, 0xcd6 | carry);
      assert.deepEqual(store(c, module, blocks[0].bytes.subarray(0, 7), op, DATA + 8, a, 'literal arithmetic anchor'), {result, flags}); counts.anchors++;
    }
    const crossInstruction = absolute(op, 0, DATA + 4095), boundaryInstruction = absolute(op, 0, 0xfffffffe);
    const boundary = compile(c, [block(PC + 0x100, crossInstruction), block(PC + 0x120, boundaryInstruction)]);
    for (const [instruction, address, pc] of [[crossInstruction, DATA + 4095, PC + 0x100], [boundaryInstruction, 0xfffffffe, PC + 0x120]]) {
      upload(c, address, word(0xffff)); const r = registers(); r[0] = 0xbeef0000; seed(c, pc, r, 0xcd7);
      store(c, boundary, instruction, op, address, 0xffff, 'unaligned crosspage or last valid WORD span');
      counts.boundaries++;
    }
  }
  const aliases = [
    {op: 'adc', bytes: [0x66, 0x11, 0], r: {0: 0xa53c5008}, address: 0xa53c5008},
    {op: 'sbb', bytes: [0x66, 0x19, 0x24, 0x24], r: {4: 0xa53c5008}, address: 0xa53c5008},
    {op: 'adc', bytes: [0x66, 0x11, 0x4c, 0x8f, 0xfc], r: {1: 0x40000002, 7: DATA + 4}, address: DATA + 8},
    {op: 'sbb', bytes: [0x66, 0x19, 0x6d, 3], r: {5: 0xa53c5005}, address: 0xa53c5008},
  ];
  const aliasModule = compile(c, aliases.map((a, i) => block(PC + 0x200 + i * 16, Buffer.from(a.bytes))));
  for (const [i, a] of aliases.entries()) {
    const r = registers(); Object.assign(r, a.r); upload(c, a.address, word(0x8000)); seed(c, PC + 0x200 + i * 16, r);
    store(c, aliasModule, Buffer.from(a.bytes), a.op, a.address, 0x8000, 'full32 base/index source alias'); counts.aliases++;
  }
  const faultBlocks = ['adc', 'sbb'].map((op, i) => block(PC + 0x300 + i * 32, Buffer.from([0xbb, 0x44, 0x33, 0x22, 0x11, 0x66, opcode(op), 7])));
  const faultModule = compile(c, faultBlocks);
  const controlRegisters = registers(); controlRegisters[7] = 0x7008;
  seed(c, faultBlocks[0].pc + 5, controlRegisters);
  run(c, faultModule, 0, cpu(c), {label: 'zero budget avoids unmapped RMW', retired: 0, helperVersion: null});
  input(c, 96, words([1]));
  run(c, faultModule, 1, cpu(c), {label: 'cancellation avoids unmapped RMW', reason: 2, retired: 0, helperVersion: null});
  input(c, 96, words([0])); counts.controls += 2;
  for (const [opIndex, op] of ['adc', 'sbb'].entries()) for (const kind of ['unmapped', 'read-denied', 'write-denied', 'cross-unmapped', 'cross-read-denied', 'cross-write-denied', 'overflow']) {
    const crossing = kind.startsWith('cross'), overflow = kind === 'overflow', address = overflow ? 0xffffffff : crossing ? 0x7fff : 0x7008;
    const missing = kind.includes('unmapped'), readDenied = kind.includes('read-denied'), writeDenied = kind.includes('write-denied');
    const mapped = overflow ? 0 : crossing && !missing ? 2 : missing && !crossing ? 0 : 1;
    if (mapped) {host(c, 'map', [0x7000, mapped, 3]); upload(c, crossing ? 0x7ffe : 0x7007, Buffer.from(crossing && missing ? [0x55, 0x0f] : [0x55, 0x0f, 0x80, 0xaa]));}
    if (readDenied || writeDenied) host(c, 'protect', [crossing ? 0x8000 : 0x7000, 1, readDenied ? 2 : 1]);
    if (overflow) upload(c, 0xfffffffe, Buffer.from([0x55, 0x0f]));
    const r = registers(); r[0] = 0xbeef0000; r[7] = address;
    const before = seed(c, faultBlocks[opIndex].pc, r), stopped = {...before, registers: [...before.registers], pc: before.pc + 5}; stopped.registers[3] = 0x11223344;
    const fault = {detail: overflow ? 3 : missing ? 1 : 2, address: overflow ? address : crossing ? 0x8000 : address, access: writeDenied ? 2 : 1};
    const helperVersion = writeDenied ? 4 : 2;
    run(c, faultModule, 2, stopped, {label: kind + ' after one MOV', reason: 5, retired: 1, fault, helperVersion});
    run(c, faultModule, 1, stopped, {label: kind + ' unchanged CPU retry', reason: 5, retired: 0, fault, helperVersion}); counts.faults += 2;
    if (!readDenied && mapped) checkRam(c, crossing ? 0x7ffe : 0x7007, Buffer.from(crossing && missing ? [0x55, 0x0f] : [0x55, 0x0f, 0x80, 0xaa]));
    if (overflow) checkRam(c, 0xfffffffe, Buffer.from([0x55, 0x0f]));
    if (!overflow) {
      if (missing) host(c, 'map', [crossing ? 0x8000 : 0x7000, 1, 3]);
      else host(c, 'protect', [crossing ? 0x8000 : 0x7000, 1, 3]);
      assert.deepEqual(cpu(c), stopped, 'map/protect-only repair preserves CPU and current carry');
      const a = missing ? crossing ? 0x000f : 0 : 0x800f;
      checkRam(c, address, word(a));
      store(c, faultModule, Buffer.from([0x66, opcode(op), 7]), op, address, a, 'repair continuation without MOV replay'); counts.retries++;
      host(c, 'unmap', [0x7000, crossing ? 2 : 1]);
    }
  }
  const chainBlocks = ['adc', 'sbb'].map((op, i) => block(PC + 0x400 + i * 32, Buffer.concat([
    absolute(op, 1, DATA + 16), absolute(op, 2, DATA + 18), Buffer.from([0x0f, 0x92, 0xc3, 0x0f, 0x90, 0xc7]),
  ])));
  const chainModule = compile(c, chainBlocks);
  for (const [i, op] of ['adc', 'sbb'].entries()) {
    const r = registers(); r[1] = 0xbeef0000; r[2] = 0xdead0000;
    const low = op === 'adc' ? 0xffff : 0, high = op === 'adc' ? 0x7fff : 0x8000;
    upload(c, DATA + 16, Buffer.concat([word(low), word(high)])); seed(c, chainBlocks[i].pc, r, 0xcd7);
    store(c, chainModule, absolute(op, 1, DATA + 16), op, DATA + 16, low, 'low WORD produces carry');
    assert.equal(cpu(c).flags & 1, 1);
    store(c, chainModule, absolute(op, 2, DATA + 18), op, DATA + 18, high, 'high WORD consumes preceding carry without reseed');
    const before = cpu(c), next = {...before, registers: [...before.registers], pc: before.pc + 6};
    assert.equal(before.flags & 1, 0); assert.equal(before.flags & 0x800, 0x800);
    next.registers[3] = ((before.registers[3] & 0xffff0000) | 0x100) >>> 0;
    run(c, chainModule, 2, next, {label: 'SETB/SETO consume resulting flags', retired: 2, helperVersion: null}); counts.chains++;
  }
  assert.equal(c.api.close(), 0);
}
for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) for (const op of ['adc', 'sbb']) for (const same of [false, true]) {
  const c = open(owner, entries), instruction = Buffer.from([0x66, opcode(op), 7]);
  const module = compile(c, [block(PC, instruction)]), r = registers(); r[0] = (0xbeef0000 | (same ? 0xffff : 0)) >>> 0; r[7] = PC;
  seed(c, PC, r, 0xcd7); const a = instruction.readUInt16LE(0);
  const expected = store(c, module, instruction, op, PC, a, 'own-code commits once then invalidates', 6);
  assert.equal(expected.result === a, same);
  run(c, module, 1, cpu(c), {label: 'old owner stale and completely neutral', status: 4, retired: 0});
  const tail = compile(c, [{pc: PC + 3, bytes: Buffer.from([0xeb, 0]), memory: false}]);
  run(c, tail, 2, {...cpu(c), pc: PC + 5}, {label: 'fresh suffix continues without store replay', reason: 3, retired: 1, helperVersion: null, exitVersion: 1});
  checkRam(c, PC, word(expected.result)); assert.equal(c.api.close(), 0); counts.self_code++;
}
assert.deepEqual(sources(), sourcePins, 'risk-relevant sources stayed fixed during the run');
assert.equal(counts.contexts, 20); assert.equal(counts.matrix, 128); assert.equal(counts.anchors, 76);
assert.equal(counts.boundaries, 16); assert.equal(counts.runs, 476); assert.equal(counts.retired, 404);
assert.equal(counts.aliases, 16); assert.equal(counts.faults, 112); assert.equal(counts.retries, 48);
assert.equal(counts.chains, 8); assert.equal(counts.self_code, 16);
assert.equal(counts.controls, 8);
assert.equal(modules.length, 60);
const result = {status: 'ok', engine_sha256: engine.sha256, source_pins: sourcePins, counts, modules,
  limits: ['finite exact single66 WORD ADC11/SBB19 memory-destination register-source cases in four bound profiles',
    'real read16/store16 paths; no arbitrary-host memory rollback claim', 'full4364 arena comparisons preserve FP128; selected live RAM/canary checks',
    'no hardware, exhaustive ISA, throughput, browser or game claim']};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2) + '\n', {flag: 'wx'});
console.log(JSON.stringify({status: result.status, ...counts, modules: modules.length, output}));
