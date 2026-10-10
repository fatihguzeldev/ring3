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
  'engine/tests/cpu_word_memory_carry_rotate_wasm.rs',
  'engine/tests/fixtures/p2-word-memory-carry-rotate/run.mjs',
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
  assert.equal(c.api.open(8, c.id, 0x57435254), 0); c.base = c.api.arena_ptr() >>> 0; input(c, 0, initial);
  for (const args of [[PC, 1, 7], [DATA, 2, 3], [0xa53c5000, 1, 3], [0xfffff000, 1, 3]]) host(c, 'map', args);
  return c;
}

// repeated one-bit arithmetic moves the current WORD and carry around a 17-bit ring.
function rotated(op, a, raw, oldFlags) {
  const q = raw % 32, distance = q % 17;
  if (distance === 0) return {result: a, flags: oldFlags};
  let result = a, carry = oldFlags % 2;
  for (let step = 0; step < distance; step++) {
    if (op === 'rcl') {
      const nextCarry = Number(result >= 32768); result = result * 2 % 65536 + carry; carry = nextCarry;
    } else {
      const nextCarry = result % 2; result = Math.floor(result / 2) + carry * 32768; carry = nextCarry;
    }
  }
  const top = Number(result >= 32768), second = Math.floor(result / 16384) % 2;
  const overflow = q === 1 && (op === 'rcl' ? top !== carry : top !== second);
  return {result, flags: (oldFlags & ~0x801) | carry | (overflow ? 0x800 : 0)};
}
const extensions = {rcl: 2, rcr: 3}, rawCounts = [0, 1, 2, 16, 17, 18, 31, 32, 33, 255];
const operands = [0x8000, 0x8001, 0x7fff, 0xffff, 0x8001, 0x4000, 1, 0x8001, 0x4000, 0];
function instruction(op, family, raw, tail) {
  return Buffer.from([0x66, {one: 0xd1, immediate: 0xc1, cl: 0xd3}[family], tail[0] | extensions[op] << 3,
    ...tail.slice(1), ...(family === 'immediate' ? [raw] : [])]);
}
function spec(op, family, raw = 1, address = DATA + 8) {
  return {op, family, raw, address, bytes: instruction(op, family, raw, [5, ...words([address])])};
}
const anchors = [
  {op: 'rcl', a: 0x8001, raw: 0, old: 0xcd7, result: 0x8001, flags: 0xcd7},
  {op: 'rcr', a: 0x8000, raw: 32, old: 0xcd7, result: 0x8000, flags: 0xcd7},
  {op: 'rcl', a: 0x4000, raw: 1, old: 0xcd7, result: 0x8001, flags: 0xcd6},
  {op: 'rcr', a: 1, raw: 1, old: 0xcd6, result: 0, flags: 0x4d7},
  {op: 'rcr', a: 0, raw: 1, old: 0xcd7, result: 0x8000, flags: 0xcd6},
  {op: 'rcl', a: 0xffff, raw: 1, old: 0xcd6, result: 0xfffe, flags: 0x4d7},
  {op: 'rcl', a: 0x8000, raw: 16, old: 0xcd7, result: 0xc000, flags: 0x4d6},
  {op: 'rcr', a: 0x8000, raw: 16, old: 0xcd7, result: 1, flags: 0x4d7},
  {op: 'rcl', a: 0x8001, raw: 17, old: 0xcd7, result: 0x8001, flags: 0xcd7},
  {op: 'rcr', a: 0x8001, raw: 49, old: 0xcd6, result: 0x8001, flags: 0xcd6},
  {op: 'rcl', a: 0x4000, raw: 18, old: 0xcd7, result: 0x8001, flags: 0x4d6},
  {op: 'rcr', a: 1, raw: 18, old: 0xcd7, result: 0x8000, flags: 0x4d7},
  {op: 'rcl', a: 0x4000, raw: 33, old: 0xcd7, result: 0x8001, flags: 0xcd6},
  {op: 'rcr', a: 0, raw: 255, old: 0xcd7, result: 4, flags: 0x4d6},
  {op: 'rcl', a: 0, raw: 2, old: 0xcd6, result: 0, flags: 0x4d6},
  {op: 'rcr', a: 0xffff, raw: 18, old: 0xcd7, result: 0xffff, flags: 0x4d7},
];
for (const row of anchors) assert.deepEqual(rotated(row.op, row.a, row.raw, row.old),
  {result: row.result, flags: row.flags}, 'literal 17-bit ring/whole-FLAGS identity/OF-count anchor');
function store(c, module, row, a, label, reason = 1) {
  const before = cpu(c), raw = row.family === 'one' ? 1 : row.family === 'cl' ? before.registers[1] % 256 : row.raw;
  const expected = rotated(row.op, a, raw, before.flags);
  run(c, module, 1, {...before, pc: before.pc + row.bytes.length, flags: expected.flags}, {label, reason});
  checkRam(c, row.address, word(expected.result)); return expected;
}
function countRegisters(raw) {
  const r = registers(); r[1] = ((r[1] & 0xffffff00) | raw) >>> 0; return r;
}

for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries), one = Object.keys(extensions).map(op => spec(op, 'one'));
  const oneModule = compile(c, one.map((row, i) => block(PC + i * 16, row.bytes)));
  for (const [i, row] of one.entries()) for (const a of [0x8000, 0]) for (const carry of [0, 1]) {
    upload(c, row.address - 1, Buffer.concat([Buffer.from([0x55]), word(a), Buffer.from([0xaa])])); seed(c, PC + i * 16, registers(), 0xcd6 | carry);
    store(c, oneModule, row, a, 'D1 all2 carry-rotate kinds/both incoming CF values');
    checkRam(c, row.address - 1, Buffer.from([0x55])); checkRam(c, row.address + 2, Buffer.from([0xaa])); counts.matrix++;
  }
  const immediate = Object.keys(extensions).flatMap(op => rawCounts.map(raw => spec(op, 'immediate', raw)));
  for (let start = 0; start < immediate.length; start += 8) {
    const rows = immediate.slice(start, start + 8), module = compile(c, rows.map((row, i) => block(PC + 0x100 + i * 16, row.bytes)));
    for (const [i, row] of rows.entries()) for (const carry of [0, 1]) {
      const a = operands[rawCounts.indexOf(row.raw)];
      upload(c, row.address, word(a)); seed(c, PC + 0x100 + i * 16, registers(), 0xcd6 | carry);
      store(c, module, row, a, 'C1 literal raw count/17-bit distance/both incoming CF values'); counts.matrix++;
    }
  }
  const cl = Object.keys(extensions).map(op => spec(op, 'cl')), clModule = compile(c, cl.map((row, i) => block(PC + 0x300 + i * 16, row.bytes)));
  for (const [i, row] of cl.entries()) for (const raw of rawCounts) for (const carry of [0, 1]) {
    const a = operands[rawCounts.indexOf(raw)];
    upload(c, row.address, word(a)); seed(c, PC + 0x300 + i * 16, countRegisters(raw), 0xcd6 | carry);
    store(c, clModule, row, a, 'D3 current lowCL/current CF/count mask/allGPRs preserved'); counts.matrix++;
  }
  for (let start = 0; start < anchors.length; start += 8) {
    const rows = anchors.slice(start, start + 8).map(row => ({...spec(row.op, 'immediate', row.raw), ...row}));
    const module = compile(c, rows.map((row, i) => block(PC + 0x400 + i * 16, row.bytes)));
    for (const [i, row] of rows.entries()) {
      upload(c, row.address, word(row.a)); seed(c, PC + 0x400 + i * 16, registers(), row.old);
      assert.deepEqual(store(c, module, row, row.a, 'literal complete carry-rotate FLAGS anchor'), {result: row.result, flags: row.flags}); counts.anchors++;
    }
  }
  assert.equal(c.api.close(), 0);
}

// separate address contexts avoid exhausting the eight-unit resident bank.
for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries), addresses = Object.keys(extensions).flatMap((op, index) => ['one', 'immediate', 'cl'].map(family => {
    const raw = [0x66, 0xf3][index], alias = family === 'cl';
    return {op, family, raw, address: 0xa53c5008, bytes: instruction(op, family, raw, alias ? [0x41, 0xfd] : [0x44, 0x8d, 0xfc])};
  }));
  for (let start = 0; start < addresses.length; start += 8) {
    const rows = addresses.slice(start, start + 8), module = compile(c, rows.map((row, i) => block(PC + i * 16, row.bytes)));
    for (const [i, row] of rows.entries()) {
      const r = registers(); r[5] = 0xa53c5004; r[1] = row.family === 'cl' ? 0xa53c500b : 0x40000002;
      upload(c, row.address, word(0x8001)); seed(c, PC + i * 16, r);
      store(c, module, row, 0x8001, 'full32 scaled EA/prefix-looking immediate/ECX address-CL alias'); counts.addresses++;
    }
  }
  const boundaries = Object.keys(extensions).flatMap(op => [spec(op, 'immediate', 17, DATA + 4095), spec(op, 'cl', 32, 0xfffffffe)]);
  const boundaryModule = compile(c, boundaries.map((row, i) => block(PC + 0x200 + i * 16, row.bytes)));
  for (const [i, row] of boundaries.entries()) {
    upload(c, row.address, word(0x800f)); seed(c, PC + 0x200 + i * 16, countRegisters(32));
    store(c, boundaryModule, row, 0x800f, 'carry-rotate unaligned crosspage/zero-count last valid WORD'); counts.boundaries++;
  }
  assert.equal(c.api.close(), 0);
}

for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries), witnesses = [spec('rcl', 'immediate', 32), spec('rcr', 'immediate', 17)];
  witnesses[0].old = 0xcd7; witnesses[1].old = 0xcd7;
  for (const row of witnesses) row.bytes = instruction(row.op, row.family, row.raw, [7]);
  const one = {op: 'rcl', family: 'one', address: DATA + 8, bytes: instruction('rcl', 'one', 1, [7])};
  const changed = {op: 'rcl', family: 'cl', address: DATA + 12, bytes: instruction('rcl', 'cl', 0, [7])};
  const mov = Buffer.from([0xbb, 0x44, 0x33, 0x22, 0x11]);
  const faultBlocks = [...witnesses, one].map((row, i) => block(PC + i * 32, Buffer.concat([mov, row.bytes])));
  faultBlocks.push(block(PC + 96, Buffer.concat([Buffer.from([0xb1, 32]), changed.bytes])));
  const faultModule = compile(c, faultBlocks), r = registers(); r[7] = 0x7008;
  seed(c, PC + 5, r);
  run(c, faultModule, 0, cpu(c), {label: 'zero budget avoids unmapped carry-rotate RMW', retired: 0, helperVersion: null});
  input(c, 96, words([1]));
  run(c, faultModule, 1, cpu(c), {label: 'cancel avoids unmapped carry-rotate RMW', reason: 2, retired: 0, helperVersion: null});
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
    const before = seed(c, faultBlocks[index].pc, r, row.old), stopped = {...before, registers: [...before.registers], pc: before.pc + 5}; stopped.registers[3] = 0x11223344;
    const fault = {detail: overflow ? 3 : missing ? 1 : 2, address: overflow ? address : crossing ? 0x8000 : address, access: writeDenied ? 2 : 1};
    const helperVersion = writeDenied ? 4 : 2;
    run(c, faultModule, 2, stopped, {label: kind + ' after MOV; q0/q17 complete FLAGS uncommitted', reason: 5, retired: 1, fault, helperVersion});
    run(c, faultModule, 1, stopped, {label: kind + ' unchanged carry-rotate retry', reason: 5, retired: 0, fault, helperVersion}); counts.faults += 2;
    if (!readDenied && mapped) checkRam(c, crossing ? 0x7ffe : 0x7007, unchanged);
    if (overflow) checkRam(c, 0xfffffffe, Buffer.from([0x55, 15]));
    if (!overflow) {
      if (missing) host(c, 'map', [crossing ? 0x8000 : 0x7000, 1, 3]);
      else host(c, 'protect', [crossing ? 0x8000 : 0x7000, 1, 3]);
      assert.deepEqual(cpu(c), stopped, 'map/protect-only repair preserves current CPU/count/old FLAGS');
      if (mapped) checkRam(c, crossing ? 0x7ffe : 0x7007, unchanged);
      const a = missing ? crossing ? 15 : 0 : 0x800f; checkRam(c, address, word(a));
      store(c, faultModule, {...row, address}, a, 'same carry-rotate RMW continues without MOV replay'); counts.retries++;
      host(c, 'unmap', [0x7000, crossing ? 2 : 1]);
    }
  }
  const oneR = registers(); oneR[7] = one.address;
  upload(c, one.address - 1, Buffer.from([0x55, 1, 0x80, 0xaa])); host(c, 'protect', [DATA, 1, 1]);
  const beforeOne = seed(c, faultBlocks[2].pc, oneR, 2), stoppedOne = {...beforeOne, registers: [...beforeOne.registers], pc: beforeOne.pc + 5}; stoppedOne.registers[3] = 0x11223344;
  const oneFault = {detail: 2, address: one.address, access: 2};
  run(c, faultModule, 2, stoppedOne, {label: 'q1 D1 store faults after MOV without publishing CF/OF', reason: 5, retired: 1, fault: oneFault});
  run(c, faultModule, 1, stoppedOne, {label: 'unchanged q1 store-fault retry', reason: 5, retired: 0, fault: oneFault}); counts.faults += 2;
  checkRam(c, one.address - 1, Buffer.from([0x55, 1, 0x80, 0xaa])); host(c, 'protect', [DATA, 1, 3]); assert.deepEqual(cpu(c), stoppedOne);
  assert.deepEqual(store(c, faultModule, one, 0x8001, 'q1 permission-only repair publishes current CF/OF'), {result: 2, flags: 0x803}); counts.retries++;
  checkRam(c, one.address - 1, Buffer.from([0x55])); checkRam(c, one.address + 2, Buffer.from([0xaa]));

  const changedR = registers(); changedR[7] = changed.address;
  upload(c, changed.address - 1, Buffer.from([0x55, 0x34, 0x12, 0xaa])); host(c, 'protect', [DATA, 1, 1]);
  const before = seed(c, faultBlocks[3].pc, changedR), stopped = {...before, registers: [...before.registers], pc: before.pc + 2};
  stopped.registers[1] = ((before.registers[1] & 0xffffff00) | 32) >>> 0;
  const fault = {detail: 2, address: changed.address, access: 2};
  run(c, faultModule, 2, stopped, {label: 'MOV CL32 retires then D3 q0 carry-rotate store faults', reason: 5, retired: 1, fault});
  run(c, faultModule, 1, stopped, {label: 'unchanged D3 q0 retry still stores', reason: 5, retired: 0, fault}); counts.faults += 2;
  checkRam(c, changed.address - 1, Buffer.from([0x55, 0x34, 0x12, 0xaa])); host(c, 'protect', [DATA, 1, 3]); upload(c, changed.address, word(0x8001));
  const current = {...stopped, registers: [...stopped.registers], flags: stopped.flags & ~1}; current.registers[1] = ((stopped.registers[1] & 0xffffff00) | 2) >>> 0;
  input(c, 20, words([current.registers[1]])); input(c, 52, words([current.flags]));
  assert.deepEqual(cpu(c), current, 'declared CL and CF edits preserve the remaining stopped CPU');
  assert.deepEqual(store(c, faultModule, changed, 0x8001, 'D3 retry consumes changed current WORD/CL/CF; cached CF1 would yield7'), {result: 5, flags: 0x4d6});
  checkRam(c, changed.address - 1, Buffer.from([0x55])); checkRam(c, changed.address + 2, Buffer.from([0xaa])); counts.retries++; counts.current_inputs++;

  const rcl = spec('rcl', 'cl'), rcr = spec('rcr', 'cl'), high = DATA + 10;
  const adc = Buffer.concat([Buffer.from([0x66, 0x83, 0x15]), words([high]), Buffer.from([0])]);
  const code = Buffer.concat([Buffer.from([0xf9, 0xb1, 1]), rcl.bytes, Buffer.from([0xb1, 17]), rcr.bytes,
    Buffer.from([0xb1, 18]), rcr.bytes, Buffer.from([0xb1, 33]), rcl.bytes,
    Buffer.from([0x0f, 0x92, 0xc0, 0x0f, 0x90, 0xc3]), adc]);
  const module = compile(c, [block(PC + 0x100, code)]);
  upload(c, DATA + 7, Buffer.from([0x55, 0, 0x40, 0, 0, 0xaa])); seed(c, PC + 0x100, registers(), 0xcd6);
  let old = cpu(c);
  run(c, module, 1, {...old, pc: old.pc + 1, flags: old.flags | 1}, {label: 'STC creates live carry without CPU reseed', helperVersion: null});
  function liveCount(raw) {
    old = cpu(c); const next = {...old, registers: [...old.registers], pc: old.pc + 2};
    next.registers[1] = ((old.registers[1] & 0xffffff00) | raw) >>> 0;
    run(c, module, 1, next, {label: 'live MOV CL' + raw + ' preserves preceding FLAGS', helperVersion: null});
  }
  liveCount(1);
  assert.deepEqual(store(c, module, rcl, 0x4000, 'RCL q1 consumes live CF1 and produces CF0/OF1'), {result: 0x8001, flags: 0xcd6});
  liveCount(17);
  assert.deepEqual(store(c, module, rcr, 0x8001, 'RCR q17 identity retains preceding CF0/OF1 and every status flag'), {result: 0x8001, flags: 0xcd6});
  liveCount(18);
  assert.deepEqual(store(c, module, rcr, 0x8001, 'RCR q18 distance1 produces CF1 and clears OF under profile policy'), {result: 0x4000, flags: 0x4d7});
  liveCount(33);
  assert.deepEqual(store(c, module, rcl, 0x4000, 'RCL raw33 masked q1 consumes current CF1 and defines OF1'), {result: 0x8001, flags: 0xcd6});
  old = cpu(c); const next = {...old, registers: [...old.registers], pc: old.pc + 6};
  next.registers[0] = (old.registers[0] & 0xffffff00) >>> 0;
  next.registers[3] = ((old.registers[3] & 0xffffff00) | 1) >>> 0;
  run(c, module, 2, next, {label: 'SETB AL/SETO BL consume current CF0/OF1', retired: 2, helperVersion: null});
  old = cpu(c);
  run(c, module, 1, {...old, pc: old.pc + adc.length, flags: 0x446}, {label: 'ADC high0 consumes clear carry from live ring chain'});
  checkRam(c, DATA + 7, Buffer.from([0x55, 1, 0x80, 0, 0, 0xaa])); counts.consumers++;
  assert.equal(c.api.close(), 0);
}

for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) for (const kind of [
  ...Object.keys(extensions).map(op => ({op, family: 'one', raw: 1})),
  ...Object.keys(extensions).map(op => ({op, family: 'immediate', raw: 32})),
  ...Object.keys(extensions).map(op => ({op, family: 'cl', raw: 32})),
  ...Object.keys(extensions).map(op => ({op, family: 'immediate', raw: 17})),
  ...Object.keys(extensions).map(op => ({op, family: 'cl', raw: 17})),
  {op: 'rcl', family: 'immediate', raw: 0x81, ownCount: true},
]) {
  const c = open(owner, entries), bytes = instruction(kind.op, kind.family, kind.raw, [7]);
  const address = kind.ownCount ? PC + 2 : PC, a = bytes.readUInt16LE(address - PC), row = {...kind, bytes, address};
  const r = countRegisters(kind.raw); r[7] = address;
  const oldFlags = kind.ownCount || kind.op === 'rcl' && kind.family === 'one' ? 0xcd6 : 0xcd7;
  const module = compile(c, [block(PC, bytes)]); seed(c, PC, r, oldFlags);
  const expected = store(c, module, row, a, 'own-code carry-rotate write commits FLAGS and one retirement', 6);
  const identity = kind.raw === 32 || kind.raw === 17;
  assert.equal(expected.result === a, identity);
  if (identity) assert.equal(expected.flags, oldFlags, 'q0/q17 identity code writes preserve complete FLAGS but stale the code owner');
  if (kind.ownCount) assert.deepEqual({a, ...expected}, {a: 0x8117, result: 0x022e, flags: 0xcd7}, 'original count81 and CF0 produce new CF1/OF1; rewritten count02 would mean q2/OF0');
  run(c, module, 1, cpu(c), {label: 'stale carry-rotate owner completely neutral', status: 4, retired: 0});
  const nextPC = PC + bytes.length, tail = compile(c, [{pc: nextPC, bytes: Buffer.from([0xeb, 0]), memory: false}]);
  run(c, tail, 2, {...cpu(c), pc: nextPC + 2}, {label: 'fresh suffix continues without carry-rotate RMW replay', reason: 3, retired: 1, helperVersion: null, exitVersion: 1});
  checkRam(c, address, word(expected.result)); assert.equal(c.api.close(), 0); counts.self_code++;
}
assert.deepEqual(sources(), sourcePins, 'risk-relevant sources stayed fixed');
assert.equal(counts.contexts, 56); assert.equal(counts.runs, 824); assert.equal(counts.retired, 712);
assert.equal(counts.matrix, 352); assert.equal(counts.anchors, 64); assert.equal(counts.addresses, 24);
assert.equal(counts.boundaries, 16); assert.equal(counts.faults, 128); assert.equal(counts.retries, 56);
assert.equal(counts.current_inputs, 4); assert.equal(counts.consumers, 4); assert.equal(counts.controls, 8);
assert.equal(counts.self_code, 44); assert.equal(modules.length, 132);
const result = {status: 'ok', engine_sha256: engine.sha256, source_pins: sourcePins, counts, modules,
  limits: ['finite exact single66 WORD memory RCL/RCR D1/C1/D3 /2,/3 in four bound profiles',
    'q=raw&31 differs from distance=q%17; q0/q17 preserve complete FLAGS while retaining checked RMW',
    'nonzero distance preserves SF/ZF/AF/PF/DF; q1 defines OF, other effective counts explicitly clear OF under this profile, no hardware undefined-bit equality claim',
    'real read16/store16 paths and selected RAM/canary checks; no arbitrary-host rollback claim',
    'full4364 arena preserves FP128; current CF/full17bit candidate/original count survive checked-store scratch and own-count writes',
    'no exhaustive ISA, hardware, throughput, browser or game claim']};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2) + '\n', {flag: 'wx'});
console.log(JSON.stringify({status: result.status, ...counts, modules: modules.length, output}));
