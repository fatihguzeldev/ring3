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
  'engine/tests/cpu_word_memory_immediate_logical_store_wasm.rs',
  'engine/tests/fixtures/p2-word-memory-immediate-logical-store/run.mjs',
];
const sources = () => Object.fromEntries(paths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = sources(), modules = [];
const counts = {contexts: 0, runs: 0, retired: 0, matrix: 0, anchors: 0, payloads: 0, registers: 0, boundaries: 0, faults: 0, retries: 0, consumer_steps: 0, consumers: 0, controls: 0, self_code: 0};
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

// arithmetic bit digits keep the oracle independent of emitted Wasm bitwise operations.
function logical(op, a, b, oldFlags) {
  let result = 0, ones = 0;
  for (let bit = 0, weight = 1; bit < 16; bit++, weight *= 2) {
    const left = Math.floor(a / weight) % 2, right = Math.floor(b / weight) % 2;
    const set = op === 'and' ? left === 1 && right === 1 : op === 'or' ? left === 1 || right === 1 : left !== right;
    if (set) {result += weight; if (bit < 8) ones++;}
  }
  // af is undefined on x86; this engine profile deterministically clears it with CF/OF.
  return {result, flags: (oldFlags & 0x402) | (ones % 2 === 0 ? 4 : 0)
    | (result === 0 ? 64 : 0) | (result >= 32768 ? 128 : 0)};
}
const extensions = {and: 4, or: 1, xor: 6};
for (const [raw, expected] of [[0, 0], [0x7f, 0x7f], [0x80, 0xff80], [0xff, 0xffff], [0xf0, 0xfff0]])
  assert.equal(raw < 128 ? raw : raw + 0xff00, expected, 'literal signed imm8 mask');
function instruction(op, encoding, raw, tail) {
  return Buffer.concat([Buffer.from([0x66, encoding, tail[0] | extensions[op] << 3]),
    Buffer.from(tail.slice(1)), encoding === 0x81 ? word(raw) : Buffer.from([raw])]);
}
function spec(op, encoding, raw, right, address = DATA + 8) {
  return {op, encoding, raw, right, address, bytes: instruction(op, encoding, raw, [5, ...words([address])])};
}
const anchors = [
  {...spec('and', 0x83, 0x80, 0xff80), a: 0xffff, result: 0xff80, flags: 0x482},
  {...spec('and', 0x83, 0x80, 0xff80), a: 0x1234, result: 0x1200, flags: 0x406},
  {...spec('and', 0x83, 0x80, 0xff80), a: 0x7f, result: 0, flags: 0x446},
  {...spec('or', 0x83, 0x80, 0xff80), a: 1, result: 0xff81, flags: 0x486},
  {...spec('or', 0x83, 0x7f, 0x7f), a: 0, result: 0x7f, flags: 0x402},
  {...spec('or', 0x83, 0x80, 0xff80), a: 0x7fff, result: 0xffff, flags: 0x486},
  {...spec('xor', 0x83, 0xff, 0xffff), a: 0x8000, result: 0x7fff, flags: 0x406},
  {...spec('xor', 0x83, 0x80, 0xff80), a: 0xff80, result: 0, flags: 0x446},
  {...spec('xor', 0x83, 0x7f, 0x7f), a: 0xffff, result: 0xff80, flags: 0x482},
];
for (const row of anchors) assert.deepEqual(logical(row.op, row.a, row.right, 0xcd7),
  {result: row.result, flags: row.flags}, 'literal zero/sign/parity mask anchor');
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
  const before = cpu(c), expected = logical(row.op, a, row.right, before.flags);
  run(c, module, 1, {...before, pc: before.pc + row.bytes.length, flags: expected.flags}, {label, reason});
  checkRam(c, row.address, word(expected.result)); return expected;
}
for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries);
  for (const op of Object.keys(extensions)) {
    const vectors = [
      ...[[0xffff, 0], [0x1234, 0xffff], [0x7fff, 0x8000], [0xa5af, 0x0f0f]]
        .map(([a, right]) => ({...spec(op, 0x81, right, right), a})),
      ...[[0x8001, 0, 0], [0xffff, 0x7f, 0x7f], [0x1234, 0x80, 0xff80], [0xaaaa, 0xff, 0xffff]]
        .map(([a, raw, right]) => ({...spec(op, 0x83, raw, right), a})),
    ];
    const blocks = vectors.map((row, i) => block(PC + i * 16, row.bytes)), module = compile(c, blocks);
    for (const [i, row] of vectors.entries()) for (const flags of [2, 0xcd7]) {
      upload(c, row.address - 1, Buffer.concat([Buffer.from([0x55]), word(row.a), Buffer.from([0xaa])]));
      seed(c, blocks[i].pc, registers(), flags);
      store(c, module, row, row.a, 'all6 immediate forms/old flag independence/allGPRs preserved');
      checkRam(c, row.address - 1, Buffer.from([0x55])); checkRam(c, row.address + 2, Buffer.from([0xaa])); counts.matrix++;
    }
  }
  for (let start = 0; start < anchors.length; start += 8) {
    const rows = anchors.slice(start, start + 8), blocks = rows.map((row, i) => block(PC + 0x200 + i * 16, row.bytes));
    const module = compile(c, blocks);
    for (const [i, row] of rows.entries()) {
      upload(c, row.address, word(row.a)); seed(c, blocks[i].pc);
      assert.deepEqual(store(c, module, row, row.a, 'literal signed8 zero/sign/parity anchor'), {result: row.result, flags: row.flags}); counts.anchors++;
    }
  }
  const payloads = Object.keys(extensions).flatMap((op, i) => [
    {op, encoding: 0x81, raw: [0x6766, 0xf3f0, 0x6667][i], right: [0x6766, 0xf3f0, 0x6667][i]},
    {op, encoding: 0x83, raw: [0xf0, 0x66, 0xf3][i], right: [0xfff0, 0x66, 0xfff3][i]},
  ].map(row => ({...row, address: 0xa53c5008, bytes: instruction(row.op, row.encoding, row.raw, [0x44, 0x8d, 0xfc])})));
  const payloadModule = compile(c, payloads.map((row, i) => block(PC + 0x400 + i * 16, row.bytes)));
  for (const [i, row] of payloads.entries()) {
    const r = registers(); r[5] = 0xa53c5004; r[1] = 0x40000002;
    upload(c, row.address, word(0x1234)); seed(c, PC + 0x400 + i * 16, r);
    store(c, payloadModule, row, 0x1234, 'prefix-looking immediate/full32 scaled EA'); counts.payloads++;
  }
  const registerRows = [
    {op: 'and', right: 0xff80, a: 0x1234, bytes: Buffer.from([0x66, 0x21, 7])},
    {op: 'or', right: 0x8000, a: 1, bytes: Buffer.from([0x66, 0x09, 7])},
    {op: 'xor', right: 0x5555, a: 0xaaaa, bytes: Buffer.from([0x66, 0x31, 7])},
  ].map(row => ({...row, address: DATA + 8}));
  const registerModule = compile(c, registerRows.map((row, i) => block(PC + 0x500 + i * 16, row.bytes)));
  for (const [i, row] of registerRows.entries()) {
    const r = registers(); r[0] = (0xbeef0000 | row.right) >>> 0; r[7] = row.address;
    upload(c, row.address, word(row.a)); seed(c, PC + 0x500 + i * 16, r);
    store(c, registerModule, row, row.a, 'unchanged register-source logical store'); counts.registers++;
  }
  const boundaries = [spec('and', 0x83, 0xf0, 0xfff0, DATA + 4095), spec('xor', 0x81, 0xffff, 0xffff, DATA + 4095),
    spec('and', 0x83, 0xf0, 0xfff0, 0xfffffffe), spec('xor', 0x81, 0xffff, 0xffff, 0xfffffffe)];
  const boundaryModule = compile(c, boundaries.map((row, i) => block(PC + 0x600 + i * 16, row.bytes)));
  for (const [i, row] of boundaries.entries()) {
    upload(c, row.address, word(0x800f)); seed(c, PC + 0x600 + i * 16);
    store(c, boundaryModule, row, 0x800f, 'unaligned crosspage/last valid WORD span'); counts.boundaries++;
  }
  assert.equal(c.api.close(), 0);
}

// separate contexts keep resident banks below the existing eight-unit capacity.
for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries), witnesses = [spec('and', 0x83, 0xf0, 0xfff0), spec('xor', 0x81, 1, 1)];
  for (const row of witnesses) row.bytes = instruction(row.op, row.encoding, row.raw, [7]);
  const faultBlocks = witnesses.map((row, i) => block(PC + i * 32, Buffer.concat([Buffer.from([0xbb, 0x44, 0x33, 0x22, 0x11]), row.bytes])));
  const faultModule = compile(c, faultBlocks), r = registers(); r[7] = 0x7008;
  seed(c, PC + 5, r);
  run(c, faultModule, 0, cpu(c), {label: 'zero budget avoids unmapped logical RMW', retired: 0, helperVersion: null});
  input(c, 96, words([1]));
  run(c, faultModule, 1, cpu(c), {label: 'cancel avoids unmapped logical RMW', reason: 2, retired: 0, helperVersion: null});
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
    run(c, faultModule, 2, stopped, {label: kind + ' after one MOV; flags uncommitted', reason: 5, retired: 1, fault, helperVersion});
    run(c, faultModule, 1, stopped, {label: kind + ' unchanged retry', reason: 5, retired: 0, fault, helperVersion}); counts.faults += 2;
    if (!readDenied && mapped) checkRam(c, crossing ? 0x7ffe : 0x7007, Buffer.from(crossing && missing ? [0x55, 15] : [0x55, 15, 0x80, 0xaa]));
    if (overflow) checkRam(c, 0xfffffffe, Buffer.from([0x55, 15]));
    if (!overflow) {
      if (missing) host(c, 'map', [crossing ? 0x8000 : 0x7000, 1, 3]);
      else host(c, 'protect', [crossing ? 0x8000 : 0x7000, 1, 3]);
      assert.deepEqual(cpu(c), stopped, 'map/protect-only repair preserves CPU and old logical flags');
      const a = missing ? crossing ? 15 : 0 : 0x800f; checkRam(c, address, word(a));
      store(c, faultModule, {...row, address}, a, 'same logical RMW continues without MOV replay'); counts.retries++;
      host(c, 'unmap', [0x7000, crossing ? 2 : 1]);
    }
  }
  const consumer = [
    {...spec('and', 0x83, 0xf0, 0xfff0), result: 0xa5a0, flags: 0x486},
    {...spec('or', 0x83, 1, 1), result: 0xa5a1, flags: 0x482},
    {...spec('xor', 0x83, 0xff, 0xffff), result: 0x5a5e, flags: 0x402},
    {...spec('and', 0x81, 0x0fff, 0x0fff), result: 0x0a5e, flags: 0x402},
    {...spec('or', 0x81, 0x8000, 0x8000), result: 0x8a5e, flags: 0x482},
    {...spec('xor', 0x81, 0x50, 0x50), result: 0x8a0e, flags: 0x482},
  ];
  const observers = Buffer.from([0x0f, 0x94, 0xc0, 0x0f, 0x98, 0xc1, 0x0f, 0x9a, 0xc2]);
  const module = compile(c, [block(PC + 0x100, Buffer.concat(consumer.flatMap(row => [row.bytes, observers])))]);
  upload(c, DATA + 8, word(0xa5af)); host(c, 'protect', [DATA, 1, 1]); seed(c, PC + 0x100);
  const stopped = cpu(c), fault = {detail: 2, address: DATA + 8, access: 2};
  assert.equal(stopped.flags & 0x811, 0x811, 'AF/CF/OF begin set before first logical write');
  run(c, module, 1, stopped, {label: 'first logical write fault preserves AF/CF/OF', reason: 5, retired: 0, fault});
  run(c, module, 1, stopped, {label: 'unchanged consumer write-fault retry', reason: 5, retired: 0, fault}); counts.faults += 2;
  checkRam(c, DATA + 8, word(0xa5af)); host(c, 'protect', [DATA, 1, 3]); assert.deepEqual(cpu(c), stopped); counts.retries++;
  let a = 0xa5af;
  for (const row of consumer) {
    assert.deepEqual(store(c, module, row, a, 'retained mask/set/toggle without CPU reseed'), {result: row.result, flags: row.flags});
    const before = cpu(c), next = {...before, registers: [...before.registers], pc: before.pc + observers.length};
    for (const [index, bit] of [[0, 6], [1, 7], [2, 2]]) next.registers[index] = ((before.registers[index] & 0xffffff00) | (row.flags >>> bit & 1)) >>> 0;
    run(c, module, 3, next, {label: 'SETZ/SETS/SETP observe preceding logical flags', retired: 3, helperVersion: null});
    a = row.result; counts.consumer_steps++;
  }
  assert.equal(a, 0x8a0e); counts.consumers++; assert.equal(c.api.close(), 0);
}
for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) for (const op of ['and', 'or', 'xor']) for (const same of [false, true]) {
  const c = open(owner, entries), encoding = op === 'or' ? 0x83 : 0x81;
  const raw = op === 'and' ? (same ? 0xffff : 0) : op === 'or' ? (same ? 0 : 0x80) : (same ? 0 : 0xffff);
  const right = op === 'or' && !same ? 0xff80 : raw, bytes = instruction(op, encoding, raw, [7]);
  const address = op === 'xor' ? PC + 3 : PC, a = bytes.readUInt16LE(address - PC);
  const row = {op, right, bytes, address}, module = compile(c, [block(PC, bytes)]), r = registers(); r[7] = address;
  seed(c, PC, r); const expected = store(c, module, row, a, 'own-code logical write commits flags and one retirement', 6);
  assert.equal(expected.result === a, same);
  run(c, module, 1, cpu(c), {label: 'stale logical owner completely neutral', status: 4, retired: 0});
  const nextPC = PC + bytes.length, tail = compile(c, [{pc: nextPC, bytes: Buffer.from([0xeb, 0]), memory: false}]);
  run(c, tail, 2, {...cpu(c), pc: nextPC + 2}, {label: 'fresh suffix continues without logical RMW replay', reason: 3, retired: 1, helperVersion: null, exitVersion: 1});
  checkRam(c, address, word(expected.result)); assert.equal(c.api.close(), 0); counts.self_code++;
}
assert.deepEqual(sources(), sourcePins, 'risk-relevant sources stayed fixed');
assert.equal(counts.contexts, 32); assert.equal(counts.runs, 576); assert.equal(counts.retired, 528);
assert.equal(counts.matrix, 192); assert.equal(counts.anchors, 36); assert.equal(counts.payloads, 24);
assert.equal(counts.registers, 12); assert.equal(counts.boundaries, 16); assert.equal(counts.faults, 120);
assert.equal(counts.retries, 52); assert.equal(counts.consumer_steps, 24); assert.equal(counts.consumers, 4);
assert.equal(counts.controls, 8); assert.equal(counts.self_code, 24); assert.equal(modules.length, 88);
const result = {status: 'ok', engine_sha256: engine.sha256, source_pins: sourcePins, counts, modules,
  limits: ['finite exact single66 WORD memory immediate AND/OR/XOR81/83 in four bound profiles',
    'real read16/store16 paths and selected RAM/canary checks; no arbitrary-host rollback claim',
    'full4364 arena preserves FP128; logical AF is deterministically cleared by this engine profile',
    'no exhaustive ISA, hardware, throughput, browser or game claim']};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2) + '\n', {flag: 'wx'});
console.log(JSON.stringify({status: result.status, ...counts, modules: modules.length, output}));
