import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../../../../../../../support/wasm/engine.mjs';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root);
const engine = readEngine(enginePath), SIZE = 4612, PC = 0x1000, DATA = 0x5000;
const initial = readFileSync(join(output, 'initial-arena.bin'));
assert.equal(initial.length, SIZE);
assert.deepEqual(WebAssembly.Module.imports(engine.module), []);
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const paths = [
  'engine/src/abi/arena.rs', 'engine/src/abi/memory_helper.rs',
  'engine/src/cpu/x86/ir.rs', 'engine/src/cpu/x86/decode/lower.rs',
  'engine/src/cpu/x86/decode/operands.rs', 'engine/src/cpu/x86/decode/integer.rs', 'engine/src/cpu/x86/decode/profile.rs', 'engine/src/cpu/dbt/region.rs',
  'engine/src/cpu/dbt/wasm/integer/arithmetic.rs', 'engine/src/cpu/dbt/wasm/integer/bits.rs', 'engine/src/cpu/dbt/wasm/integer/carry_rotates.rs', 'engine/src/cpu/dbt/wasm/integer/decimal.rs', 'engine/src/cpu/dbt/wasm/integer/dispatch.rs', 'engine/src/cpu/dbt/wasm/integer/flags.rs', 'engine/src/cpu/dbt/wasm/integer/mod.rs', 'engine/src/cpu/dbt/wasm/integer/multiply_divide.rs', 'engine/src/cpu/dbt/wasm/integer/operands.rs', 'engine/src/cpu/dbt/wasm/integer/rotates.rs', 'engine/src/cpu/dbt/wasm/integer/shifts.rs', 'engine/src/cpu/dbt/wasm/memory.rs', 'engine/src/cpu/dbt/wasm/memory/word_store.rs',
  'engine/src/cpu/dbt/wasm/memory/narrow.rs', 'engine/src/memory/space.rs',
  'engine/src/process/instance.rs', 'engine/src/process/execution/compiled.rs', 'engine/src/process/execution/authority.rs', 'engine/src/process/memory/address_space.rs', 'engine/src/process/memory/scalar.rs', 'engine/src/process/memory/scalar_tests.rs', 'engine/src/process/execution/mod.rs', 'engine/src/process/memory/mod.rs', 'engine/src/process/mod.rs', 'engine/src/process/execution/resident.rs', 'engine/src/process/execution/authority.rs', 'engine/src/process/execution/mod.rs', 'engine/src/process/mod.rs',
  'engine/tests/cpu/shifts/double/memory/wasm/word.rs', 'engine/tests/cpu/shifts/wasm.rs', 'engine/tests/support/wasm.rs', 'engine/tests/cpu/shifts/wasm.rs', 'engine/tests/support/wasm.rs',
  'engine/tests/cpu/shifts/double/memory/fixtures/p2-word-memory-double-shift/runtime/run.mjs',
];
const sources = () => Object.fromEntries(paths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = sources(), modules = [];
const counts = {contexts: 0, runs: 0, retired: 0, matrix: 0, anchors: 0, addresses: 0, boundaries: 0, faults: 0, retries: 0, current_inputs: 0, consumers: 0, controls: 0, self_code: 0};
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
  assert.deepEqual(arena(c), expected, 'full4612 CPU/exit/helper/FP128 effect: ' + label);
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
  assert.equal(c.api.open(8, c.id, 0x57445348), 0); c.base = c.api.arena_ptr() >>> 0; input(c, 0, initial);
  for (const args of [[PC, 1, 7], [DATA, 2, 3], [0xa53c5000, 1, 3], [0xfffff000, 1, 3]]) host(c, 'map', args);
  return c;
}

// one-bit concatenation arithmetic models defined counts independently of the emitted shift formulas.
function shifted(op, a, b, raw, oldFlags) {
  const q = raw % 32;
  if (q === 0) return {result: a, flags: oldFlags};
  if (q > 16) return {result: 0, flags: 2 + (oldFlags & 0x400)};
  let joined = op === 'shld' ? a * 65536 + b : b * 65536 + a, carry = 0;
  for (let i = 0; i < q; i++) {
    if (op === 'shld') {carry = Number(joined >= 2147483648); joined = joined * 2 % 4294967296;}
    else {carry = joined % 2; joined = Math.floor(joined / 2);}
  }
  const result = op === 'shld' ? Math.floor(joined / 65536) : joined % 65536;
  const parity = (result % 256).toString(2).replaceAll('0', '').length % 2 === 0;
  return {result, flags: 2 + (oldFlags & 0x400) + carry + Number(parity) * 4
    + Number(result === 0) * 0x40 + Number(result >= 32768) * 0x80
    + Number(q === 1 && (a >= 32768) !== (result >= 32768)) * 0x800};
}
const anchors = [
  {op: 'shld', a: 0x8001, b: 0x7fff, raw: 15, old: 0xcd7, result: 0xbfff, flags: 0x486},
  {op: 'shld', a: 0x8001, b: 0xffff, raw: 31, old: 0xcd7, result: 0, flags: 0x402},
  {op: 'shld', a: 0x8001, b: 0x7fff, raw: 32, old: 0xcd7, result: 0x8001, flags: 0xcd7},
  {op: 'shld', a: 0x4000, b: 0x8000, raw: 33, old: 0xcd7, result: 0x8001, flags: 0xc82},
  {op: 'shld', a: 1, b: 0xffff, raw: 255, old: 2, result: 0, flags: 2},
  {op: 'shld', a: 0x8001, b: 0x1234, raw: 16, old: 0xcd7, result: 0x1234, flags: 0x403},
  {op: 'shrd', a: 0x8001, b: 0x7fff, raw: 15, old: 0xcd7, result: 0xffff, flags: 0x486},
  {op: 'shrd', a: 0x8001, b: 0xffff, raw: 31, old: 0xcd7, result: 0, flags: 0x402},
  {op: 'shrd', a: 0x8001, b: 0x7fff, raw: 32, old: 0xcd7, result: 0x8001, flags: 0xcd7},
  {op: 'shrd', a: 0x8000, b: 0, raw: 33, old: 0xcd7, result: 0x4000, flags: 0xc06},
  {op: 'shrd', a: 1, b: 0xffff, raw: 255, old: 2, result: 0, flags: 2},
  {op: 'shrd', a: 0x8001, b: 0x1234, raw: 16, old: 0xcd7, result: 0x1234, flags: 0x403},
];
for (const row of anchors) assert.deepEqual(shifted(row.op, row.a, row.b, row.raw, row.old),
  {result: row.result, flags: row.flags}, 'literal concatenation/count-policy FLAGS anchor');
const lowWord = (parent, value) => Math.floor(parent / 65536) * 65536 + value;
const lowByte = (parent, value) => Math.floor(parent / 256) * 256 + value;
function instruction(op, family, source, raw, tail) {
  const opcode = {shld: {immediate: 0xa4, cl: 0xa5}, shrd: {immediate: 0xac, cl: 0xad}}[op][family];
  return Buffer.from([0x66, 0x0f, opcode, tail[0] | source << 3, ...tail.slice(1), ...(family === 'immediate' ? [raw] : [])]);
}
function spec(op, family, source = 2, raw = 1, address = DATA + 8) {
  return {op, family, source, raw, address, bytes: instruction(op, family, source, raw, [5, ...words([address])])};
}
function store(c, module, row, a, label, reason = 1) {
  const before = cpu(c), raw = row.family === 'cl' ? before.registers[1] % 256 : row.raw;
  const expected = shifted(row.op, a, before.registers[row.source] % 65536, raw, before.flags);
  run(c, module, 1, {...before, pc: before.pc + row.bytes.length, flags: expected.flags}, {label, reason});
  checkRam(c, row.address, word(expected.result)); return expected;
}
const vectors = [
  {raw: 0, a: 0x8001, b: 0x7fff, flags: 0xcd7},
  {raw: 1, a: 0x8000, b: 0x8000, flags: 0xcd7},
  {raw: 16, a: 0x8001, b: 0x1234, flags: 2},
  {raw: 17, a: 0x800f, b: 0xffff, flags: 0xcd7},
];
for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) for (const op of ['shld', 'shrd']) {
  const c = open(owner, entries), immediate = Array.from({length: 8}, (_, source) => vectors.map(vector => ({...spec(op, 'immediate', source, vector.raw), vector}))).flat();
  for (let start = 0; start < immediate.length; start += 8) {
    const rows = immediate.slice(start, start + 8), module = compile(c, rows.map((row, i) => block(PC + i * 16, row.bytes)));
    for (const [i, row] of rows.entries()) {
      const v = row.vector, r = registers(); r[row.source] = lowWord(r[row.source], v.b);
      upload(c, row.address - 1, Buffer.concat([Buffer.from([0x55]), word(v.a), Buffer.from([0xaa])])); seed(c, PC + i * 16, r, v.flags);
      store(c, module, row, v.a, 'all eight imm8 sources/high16/GPR retention with q0/1/16/17');
      checkRam(c, row.address - 1, Buffer.from([0x55])); checkRam(c, row.address + 2, Buffer.from([0xaa])); counts.matrix++;
    }
  }
  const cl = Array.from({length: 8}, (_, source) => spec(op, 'cl', source)), clModule = compile(c, cl.map((row, i) => block(PC + 0x100 + i * 16, row.bytes)));
  for (const [i, row] of cl.entries()) for (const v of vectors) {
    const r = registers(); r[row.source] = lowWord(r[row.source], v.b); r[1] = lowByte(r[1], v.raw);
    upload(c, row.address, word(v.a)); seed(c, PC + 0x100 + i * 16, r, v.flags);
    store(c, clModule, row, v.a, 'all eight CL sources including sourceCX/countCL alias'); counts.matrix++;
  }
  const literals = anchors.filter(row => row.op === op).map(row => ({...spec(op, 'immediate', 2, row.raw), ...row}));
  const anchorModule = compile(c, literals.map((row, i) => block(PC + 0x200 + i * 16, row.bytes)));
  for (const [i, row] of literals.entries()) {
    const r = registers(); r[2] = lowWord(r[2], row.b); upload(c, row.address, word(row.a)); seed(c, PC + 0x200 + i * 16, r, row.old);
    assert.deepEqual(store(c, anchorModule, row, row.a, 'literal q15/16/31/32/33/255 complete FLAGS'), {result: row.result, flags: row.flags}); counts.anchors++;
  }
  assert.equal(c.api.close(), 0);
}

// separate contexts keep every resident module bank below the eight-unit limit.
for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries), forms = [['shld', 'immediate'], ['shld', 'cl'], ['shrd', 'immediate'], ['shrd', 'cl']];
  const addresses = Array.from({length: 8}, (_, source) => {
    const [op, family] = forms[source % 4], raw = source % 2 ? 0xf3 : 0x66;
    const r = registers(); r[1] = lowByte(r[1], source === 3 ? 16 : source === 5 ? 32 : 17); r[source] = 0xa53c5008;
    const tail = source === 4 ? [4, 0x24] : source === 5 ? [0x45, 0] : [source];
    return {op, family, source, raw, address: 0xa53c5008, r, bytes: instruction(op, family, source, raw, tail)};
  });
  for (const op of ['shld', 'shrd']) {
    const r = registers(); r[1] = 0xa53c500b;
    addresses.push({op, family: 'cl', source: 1, raw: 11, address: 0xa53c5008, r, bytes: instruction(op, 'cl', 1, 11, [0x41, 0xfd])});
  }
  const scaled = [
    {op: 'shld', family: 'immediate', source: 0, raw: 0x66, tail: [0x44, 0x8d, 0xfc], values: {0: 0xa1008000, 1: 0x40000002, 5: 0xa53c5004}},
    {op: 'shrd', family: 'cl', source: 6, raw: 1, tail: [4, 0x85, ...words([0xa53c5000])], values: {0: 0x40000002, 1: 0xa1011201, 6: 0xa1061357}},
    {op: 'shld', family: 'cl', source: 1, raw: 33, tail: [0x84, 0x80, ...words([0x053c5003])], values: {0: 0x20000001, 1: 0x80000021}},
  ];
  for (const row of scaled) {
    const r = registers(); for (const [index, value] of Object.entries(row.values)) r[index] = value;
    addresses.push({...row, address: 0xa53c5008, r, bytes: instruction(row.op, row.family, row.source, row.raw, row.tail)});
  }
  for (let start = 0; start < addresses.length; start += 8) {
    const rows = addresses.slice(start, start + 8), module = compile(c, rows.map((row, i) => block(PC + i * 16, row.bytes)));
    for (const [i, row] of rows.entries()) {
      upload(c, row.address - 1, Buffer.from([0x55, 1, 0x80, 0xaa])); seed(c, PC + i * 16, row.r);
      store(c, module, row, 0x8001, 'old full32 source/base/index/ECX count aliases and prefix-looking immediate');
      checkRam(c, row.address - 1, Buffer.from([0x55])); checkRam(c, row.address + 2, Buffer.from([0xaa])); counts.addresses++;
    }
  }
  const boundaries = [spec('shld', 'immediate', 2, 0, DATA + 4095), spec('shld', 'cl', 2, 16, 0xfffffffe),
    spec('shrd', 'immediate', 2, 17, DATA + 4095), spec('shrd', 'cl', 2, 32, 0xfffffffe)];
  const boundaryModule = compile(c, boundaries.map((row, i) => block(PC + 0x200 + i * 16, row.bytes)));
  for (const [i, row] of boundaries.entries()) {
    const r = registers(); r[2] = lowWord(r[2], 0x8000); r[1] = lowByte(r[1], row.raw);
    upload(c, row.address, word(0x800f)); seed(c, PC + 0x200 + i * 16, r);
    store(c, boundaryModule, row, 0x800f, 'unaligned crosspage and last valid WORD under q0/q16/q17'); counts.boundaries++;
  }
  assert.equal(c.api.close(), 0);
}

for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries), witnesses = [spec('shld', 'immediate', 2, 32), spec('shrd', 'immediate', 2, 17)];
  for (const row of witnesses) row.bytes = instruction(row.op, row.family, row.source, row.raw, [7]);
  const one = {op: 'shld', family: 'immediate', source: 2, raw: 1, address: DATA + 8, bytes: instruction('shld', 'immediate', 2, 1, [7])};
  const changed = {op: 'shld', family: 'cl', source: 2, address: DATA + 12, bytes: instruction('shld', 'cl', 2, 0, [7])};
  const shld = spec('shld', 'cl'), shrd = spec('shrd', 'cl'), high = DATA + 10;
  const adc = Buffer.concat([Buffer.from([0x66, 0x83, 0x15]), words([high]), Buffer.from([0])]);
  const live = Buffer.concat([Buffer.from([0xb1, 1]), shld.bytes, Buffer.from([0xb1, 32]), shrd.bytes,
    Buffer.from([0x0f, 0x92, 0xc3, 0x0f, 0x90, 0xc7]), adc,
    Buffer.from([0xb1, 17]), shrd.bytes, Buffer.from([0x0f, 0x94, 0xc3, 0x0f, 0x9a, 0xc7])]);
  const mov = Buffer.from([0xbb, 0x44, 0x33, 0x22, 0x11]);
  const faultBlocks = [...witnesses, one].map((row, i) => block(PC + i * 32, Buffer.concat([mov, row.bytes])));
  faultBlocks.push(block(PC + 96, Buffer.concat([Buffer.from([0xb1, 32]), changed.bytes])));
  faultBlocks.push(block(PC + 0x100, live));
  const faultModule = compile(c, faultBlocks), control = registers(); control[7] = 0x7008;
  seed(c, PC + 5, control);
  run(c, faultModule, 0, cpu(c), {label: 'zero budget avoids unmapped q0 double-shift RMW', retired: 0, helperVersion: null});
  input(c, 96, words([1]));
  run(c, faultModule, 1, cpu(c), {label: 'cancel avoids unmapped q0 double-shift RMW', reason: 2, retired: 0, helperVersion: null});
  input(c, 96, words([0])); counts.controls += 2;

  for (const [index, row] of witnesses.entries()) for (const kind of ['unmapped', 'read-denied', 'write-denied', 'cross-unmapped', 'cross-read-denied', 'cross-write-denied', 'overflow']) {
    const crossing = kind.startsWith('cross'), overflow = kind === 'overflow', address = overflow ? 0xffffffff : crossing ? 0x7fff : 0x7008;
    const missing = kind.includes('unmapped'), readDenied = kind.includes('read-denied'), writeDenied = kind.includes('write-denied');
    const mapped = overflow ? 0 : crossing && !missing ? 2 : missing && !crossing ? 0 : 1;
    const unchanged = Buffer.from(crossing && missing ? [0x55, 15] : [0x55, 15, 0x80, 0xaa]);
    if (mapped) {host(c, 'map', [0x7000, mapped, 3]); upload(c, crossing ? 0x7ffe : 0x7007, unchanged);}
    if (readDenied || writeDenied) host(c, 'protect', [crossing ? 0x8000 : 0x7000, 1, readDenied ? 2 : 1]);
    if (overflow) upload(c, 0xfffffffe, Buffer.from([0x55, 15]));
    const r = registers(); r[7] = address; r[2] = lowWord(r[2], 0x8000);
    const before = seed(c, faultBlocks[index].pc, r), stopped = {...before, registers: [...before.registers], pc: before.pc + 5}; stopped.registers[3] = 0x11223344;
    const fault = {detail: overflow ? 3 : missing ? 1 : 2, address: overflow ? address : crossing ? 0x8000 : address, access: writeDenied ? 2 : 1};
    const helperVersion = writeDenied ? 4 : 2;
    run(c, faultModule, 2, stopped, {label: kind + ' after MOV; q0/q17 FLAGS and GPRs uncommitted', reason: 5, retired: 1, fault, helperVersion});
    run(c, faultModule, 1, stopped, {label: kind + ' unchanged double-shift retry', reason: 5, retired: 0, fault, helperVersion}); counts.faults += 2;
    if (!readDenied && mapped) checkRam(c, crossing ? 0x7ffe : 0x7007, unchanged);
    if (overflow) checkRam(c, 0xfffffffe, Buffer.from([0x55, 15]));
    if (!overflow) {
      if (missing) host(c, 'map', [crossing ? 0x8000 : 0x7000, 1, 3]);
      else host(c, 'protect', [crossing ? 0x8000 : 0x7000, 1, 3]);
      assert.deepEqual(cpu(c), stopped, 'map/protect-only repair preserves current WORD-source-count CPU');
      if (mapped) checkRam(c, crossing ? 0x7ffe : 0x7007, unchanged);
      const a = missing ? crossing ? 15 : 0 : 0x800f; checkRam(c, address, word(a));
      store(c, faultModule, {...row, address}, a, 'same q0/q17 RMW continues without replay or CPU reseed'); counts.retries++;
      host(c, 'unmap', [0x7000, crossing ? 2 : 1]);
    }
  }
  const oneR = registers(); oneR[7] = one.address; oneR[2] = lowWord(oneR[2], 0x8000);
  upload(c, one.address - 1, Buffer.from([0x55, 1, 0x80, 0xaa])); host(c, 'protect', [DATA, 1, 1]);
  const beforeOne = seed(c, faultBlocks[2].pc, oneR, 2), stoppedOne = {...beforeOne, registers: [...beforeOne.registers], pc: beforeOne.pc + 5}; stoppedOne.registers[3] = 0x11223344;
  const oneFault = {detail: 2, address: one.address, access: 2};
  run(c, faultModule, 2, stoppedOne, {label: 'q1 write fault retains old CF/OF despite candidate3', reason: 5, retired: 1, fault: oneFault});
  run(c, faultModule, 1, stoppedOne, {label: 'unchanged q1 store-fault retry', reason: 5, retired: 0, fault: oneFault}); counts.faults += 2;
  checkRam(c, one.address - 1, Buffer.from([0x55, 1, 0x80, 0xaa])); host(c, 'protect', [DATA, 1, 3]); assert.deepEqual(cpu(c), stoppedOne);
  assert.deepEqual(store(c, faultModule, one, 0x8001, 'q1 permission-only repair publishes current CF/OF'), {result: 3, flags: 0x807}); counts.retries++;
  checkRam(c, one.address - 1, Buffer.from([0x55])); checkRam(c, one.address + 2, Buffer.from([0xaa]));

  const changedR = registers(); changedR[7] = changed.address; changedR[2] = lowWord(changedR[2], 0);
  upload(c, changed.address - 1, Buffer.from([0x55, 0x34, 0x12, 0xaa])); host(c, 'protect', [DATA, 1, 1]);
  const before = seed(c, faultBlocks[3].pc, changedR), stopped = {...before, registers: [...before.registers], pc: before.pc + 2}; stopped.registers[1] = lowByte(before.registers[1], 32);
  const fault = {detail: 2, address: changed.address, access: 2};
  run(c, faultModule, 2, stopped, {label: 'MOV CL32 retires then q0 checked store faults', reason: 5, retired: 1, fault});
  run(c, faultModule, 1, stopped, {label: 'unchanged CL q0 retry still writes', reason: 5, retired: 0, fault}); counts.faults += 2;
  checkRam(c, changed.address - 1, Buffer.from([0x55, 0x34, 0x12, 0xaa])); host(c, 'protect', [DATA, 1, 3]); upload(c, changed.address, word(0x8001));
  const current = {...stopped, registers: [...stopped.registers]}; current.registers[1] = lowByte(stopped.registers[1], 2); current.registers[2] = lowWord(stopped.registers[2], 0x8000);
  input(c, 20, words([current.registers[1], current.registers[2]])); assert.deepEqual(cpu(c), current, 'declared WORD/source/CL edits preserve the remaining stopped CPU');
  assert.deepEqual(store(c, faultModule, changed, 0x8001, 'CL retry consumes all three current inputs; cached source gives4, count gives8001, WORD gives48d2'), {result: 6, flags: 0x406});
  checkRam(c, changed.address - 1, Buffer.from([0x55])); checkRam(c, changed.address + 2, Buffer.from([0xaa])); counts.retries++; counts.current_inputs++;

  const liveR = registers(); liveR[2] = lowWord(liveR[2], 0x8000);
  upload(c, DATA + 7, Buffer.from([0x55, 0, 0x80, 2, 0, 0xaa])); seed(c, PC + 0x100, liveR, 0xcd6);
  function liveCount(raw) {
    const old = cpu(c), next = {...old, registers: [...old.registers], pc: old.pc + 2}; next.registers[1] = lowByte(old.registers[1], raw);
    run(c, faultModule, 1, next, {label: 'live MOV CL' + raw + ' retains preceding FLAGS', helperVersion: null});
  }
  liveCount(1);
  assert.deepEqual(store(c, faultModule, shld, 0x8000, 'live SHLD q1 produces carry and overflow'), {result: 1, flags: 0xc03});
  liveCount(32);
  assert.deepEqual(store(c, faultModule, shrd, 1, 'live SHRD q0 retains exact CF/OF and original destination'), {result: 1, flags: 0xc03});
  let old = cpu(c), next = {...old, registers: [...old.registers], pc: old.pc + 6}; next.registers[3] = lowWord(old.registers[3], 0x0101);
  run(c, faultModule, 2, next, {label: 'SETB BL/SETO BH observe live q0 retained CF1/OF1', retired: 2, helperVersion: null});
  old = cpu(c); run(c, faultModule, 1, {...old, pc: old.pc + adc.length, flags: 0x406}, {label: 'ADC high0 consumes live q0 carry and stores3'});
  liveCount(17);
  assert.deepEqual(store(c, faultModule, shrd, 1, 'live SHRD q17 stores0 but clears arithmetic flags under Ring3 policy'), {result: 0, flags: 0x402});
  old = cpu(c); next = {...old, registers: [...old.registers], pc: old.pc + 6}; next.registers[3] = lowWord(old.registers[3], 0);
  run(c, faultModule, 2, next, {label: 'SETZ BL/SETP BH observe q17 ZF0/PF0 despite stored zero', retired: 2, helperVersion: null});
  checkRam(c, DATA + 7, Buffer.from([0x55, 0, 0, 3, 0, 0xaa])); counts.consumers++;
  assert.equal(c.api.close(), 0);
}

for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) for (const kind of [
  ...['shld', 'shrd'].flatMap(op => ['immediate', 'cl'].map(family => ({op, family, raw: 32}))),
  ...['shld', 'shrd'].map(op => ({op, family: 'immediate', raw: 16, identitySource: true})),
  ...['shld', 'shrd'].map(op => ({op, family: 'immediate', raw: 1})),
  {op: 'shld', family: 'immediate', raw: 0x81, ownCount: true},
]) {
  const c = open(owner, entries), bytes = instruction(kind.op, kind.family, 2, kind.raw, [7]);
  const address = kind.ownCount ? PC + 3 : PC, a = bytes.readUInt16LE(address - PC), row = {...kind, source: 2, bytes, address};
  const r = registers(); r[7] = address; r[1] = lowByte(r[1], kind.raw); r[2] = lowWord(r[2], kind.identitySource ? a : 0x8000);
  const module = compile(c, [block(PC, bytes)]); seed(c, PC, r);
  const expected = store(c, module, row, a, 'own-code double-shift commits FLAGS and one retirement before SMC', 6);
  assert.equal(expected.result === a, kind.raw === 32 || Boolean(kind.identitySource));
  if (kind.raw === 32) assert.equal(expected.flags, 0xcd7, 'q0 same-code writes preserve complete FLAGS but stale the code owner');
  if (kind.identitySource) assert.deepEqual(expected, {result: a, flags: 0x406}, 'q16 same-code source identity still derives current FLAGS');
  if (kind.ownCount) assert.deepEqual({a, ...expected}, {a: 0x8117, result: 0x022f, flags: 0xc03}, 'original source8000/count81 define CF1/OF1; rewritten count02 would clear OF');
  run(c, module, 1, cpu(c), {label: 'stale double-shift owner completely neutral', status: 4, retired: 0});
  const nextPC = PC + bytes.length, tail = compile(c, [{pc: nextPC, bytes: Buffer.from([0xeb, 0]), memory: false}]);
  run(c, tail, 2, {...cpu(c), pc: nextPC + 2}, {label: 'fresh suffix continues without double-shift RMW replay', reason: 3, retired: 1, helperVersion: null, exitVersion: 1});
  checkRam(c, address, word(expected.result)); assert.equal(c.api.close(), 0); counts.self_code++;
}
assert.deepEqual(sources(), sourcePins, 'risk-relevant sources stayed fixed');
assert.equal(counts.contexts, 52); assert.equal(counts.runs, 964); assert.equal(counts.retired, 864);
assert.equal(counts.matrix, 512); assert.equal(counts.anchors, 48); assert.equal(counts.addresses, 52);
assert.equal(counts.boundaries, 16); assert.equal(counts.faults, 128); assert.equal(counts.retries, 56);
assert.equal(counts.current_inputs, 4); assert.equal(counts.consumers, 4); assert.equal(counts.controls, 8);
assert.equal(counts.self_code, 36); assert.equal(modules.length, 136);
const result = {status: 'ok', engine_sha256: engine.sha256, source_pins: sourcePins, counts, modules,
  limits: ['finite exact single66 WORD memory SHLD/SHRD imm8/CL in four bound profiles with all eight sources',
    'q=raw&31; q0 checked RMW retains complete FLAGS and original WORD under the existing Ring3 memory contract',
    'q1..16 uses concatenation arithmetic; q1 defines OF and other defined counts clear OF under this profile',
    'q17..31 result0/zero arithmetic flags is Ring3 policy, without hardware undefined-bit equality or zero-derived ZF/PF claims',
    'real read16/store16 paths and selected RAM/canary checks; no arbitrary-host rollback claim',
    'full4612 arena preserves FP128; original destination/source/count survive checked-store scratch and own-count writes',
    'no exhaustive ISA, throughput, PE/browser/SDK/game or CI claim']};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2) + '\n', {flag: 'wx'});
console.log(JSON.stringify({status: result.status, ...counts, modules: modules.length, output}));
