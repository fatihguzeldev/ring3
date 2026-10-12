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
  'engine/tests/cpu/multiply/divide/register/native/word.rs', 'engine/tests/cpu/multiply/native.rs', 'engine/tests/support/cpu/prefix/admission.rs', 'engine/tests/support/cpu/prefix/word.rs', 'engine/src/cpu/x86/decode/integer.rs', 'engine/src/cpu/x86/decode/profile.rs', 'engine/src/cpu/dbt/region.rs',
  'engine/src/cpu/dbt/wasm/integer/arithmetic.rs', 'engine/src/cpu/dbt/wasm/integer/bits.rs', 'engine/src/cpu/dbt/wasm/integer/carry_rotates.rs', 'engine/src/cpu/dbt/wasm/integer/decimal.rs', 'engine/src/cpu/dbt/wasm/integer/dispatch.rs', 'engine/src/cpu/dbt/wasm/integer/flags.rs', 'engine/src/cpu/dbt/wasm/integer/mod.rs', 'engine/src/cpu/dbt/wasm/integer/multiply_divide.rs', 'engine/src/cpu/dbt/wasm/integer/operands.rs', 'engine/src/cpu/dbt/wasm/integer/rotates.rs', 'engine/src/cpu/dbt/wasm/integer/shifts.rs', 'engine/src/cpu/dbt/wasm/memory.rs', 'engine/src/cpu/dbt/wasm/control.rs', 'engine/src/cpu/dbt/wasm/emitter.rs',
  'engine/src/cpu/dbt/wasm/memory/narrow.rs', 'engine/src/memory/space.rs',
  'engine/src/process/instance.rs', 'engine/src/process/execution/compiled.rs', 'engine/src/process/execution/authority.rs', 'engine/src/process/memory/address_space.rs', 'engine/src/process/memory/scalar.rs', 'engine/src/process/memory/scalar_tests.rs', 'engine/src/process/execution/mod.rs', 'engine/src/process/memory/mod.rs', 'engine/src/process/mod.rs', 'engine/src/process/execution/resident.rs',
  'engine/tests/cpu/multiply/divide/register/wasm/word.rs', 'engine/tests/cpu/multiply/wasm.rs', 'engine/tests/support/wasm.rs',
  'engine/tests/cpu/multiply/divide/register/fixtures/p2-word-division/runtime/run.mjs'];
assert.equal(new Set(paths).size, paths.length);
const sources = () => Object.fromEntries(paths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = sources(), modules = [];
const counts = {contexts: 0, runs: 0, retired: 0, matrix: 0, anchors: 0, addresses: 0, boundaries: 0, faults: 0, retries: 0, current_inputs: 0, consumers: 0, controls: 0, code_reads: 0, divide_errors: 0, read_repairs: 0, divisor_repairs: 0};
const word = value => Buffer.from([value & 255, value >>> 8 & 255]);
const words = values => {const b = Buffer.alloc(values.length * 4); values.forEach((v, i) => b.writeUInt32LE(v >>> 0, i * 4)); return b;};
function record(magic, size, fields, version = 1) {
  const b = Buffer.alloc(size); b.write(magic); b.writeUInt16LE(version, 4);
  b.writeUInt16LE(1, 6); b.writeUInt32LE(size, 8);
  fields.forEach((v, i) => b.writeUInt32LE(v >>> 0, 16 + i * 4)); return b;
}
const state = c => record('R3ST', 56, [...c.registers, c.pc, c.flags]);
const exit = (reason, n, f, version = 5) => record('R3EX', 40,
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
function run(c, module, budget, next, {label, reason = 1, retired = 1, fault, helperVersion = null, helperValue = 0, status = 0, exitVersion = 5} = {}) {
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

function compile(c, blocks, codePermissions) {
  for (const block of blocks) upload(c, block.pc, block.bytes);
  if (codePermissions !== undefined) host(c, 'protect', [PC, 1, codePermissions]);
  input(c, 140, words(blocks.flatMap(b => c.entries ? [b.pc] : [b.pc, b.bytes.length])));
  const method = c.owner === 'resident' ? (c.entries ? 'compile_resident_entries' : 'compile_resident') : (c.entries ? 'compile_entries' : 'compile');
  host(c, method, c.entries ? [blocks.length, 0] : [blocks.length]);
  const b = arena(c), binding = c.owner === 'resident'
    ? {low: b.readUInt32LE(148), high: b.readUInt32LE(152), pointer: b.readUInt32LE(156), length: b.readUInt32LE(160)}
    : {generation: c.api.generation(), pointer: c.api.module_ptr() >>> 0, length: c.api.module_len() >>> 0};
  const bytes = Buffer.from(new Uint8Array(c.memory.buffer, binding.pointer, binding.length));
  const module = new WebAssembly.Module(bytes), imports = WebAssembly.Module.imports(module);
  const memoryOps = blocks.some(b => b.memory !== false);
  const names = [c.owner === 'resident' ? 'guard_resident' : 'guard', ...(memoryOps ? ['read16'] : [])];
  assert.deepEqual(imports, [{module: 'env', name: 'memory', kind: 'memory'}, ...names.map(name => ({module: 'ring3', name, kind: 'function'}))]);
  assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  const file = `module-${modules.length + 1}.wasm`; writeFileSync(join(output, file), bytes, {flag: 'wx'});
  modules.push({context: c.id, owner: c.owner, entries: c.entries, file, sha256: hash(bytes), imports: names});
  return {memoryOps, run: new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3: c.api}).exports.run};
}
const block = (pc, instruction, memory = true) => ({pc, memory, bytes: Buffer.concat([instruction, Buffer.from([0xeb, 0])])});
function open(owner, entries) {
  const instance = new WebAssembly.Instance(engine.module, {});
  const c = {id: ++counts.contexts, owner, entries, memory: instance.exports.memory, api: {}};
  for (const name of ['open', 'close', 'arena_ptr', 'map', 'unmap', 'protect', 'upload', 'read8', 'read16',
    'compile', 'compile_entries', 'compile_resident', 'compile_resident_entries', 'generation', 'module_ptr', 'module_len', 'guard', 'guard_resident'])
    c.api[name] = instance.exports['ring3_abi_v1_' + name];
  assert.equal(c.api.open(8, c.id, 0x57444956), 0); c.base = c.api.arena_ptr() >>> 0; input(c, 0, initial);
  for (const args of [[PC, 1, 7], [DATA, 2, 3], [0xa53c5000, 1, 3], [0xfffff000, 1, 3]]) host(c, 'map', args);
  return c;
}

// bigint division/remainder truncate toward zero and keep the quotient wide until its WORD range check.
function divided(kind, dx, ax, value) {
  const bits = BigInt(dx) * 65536n + BigInt(ax);
  const dividend = kind === 'signed' ? BigInt.asIntN(32, bits) : bits;
  const divisor = kind === 'signed' ? BigInt.asIntN(16, BigInt(value)) : BigInt(value);
  if (divisor === 0n) return null;
  const quotient = dividend / divisor;
  if (kind === 'signed' ? quotient < -32768n || quotient > 32767n : quotient > 65535n) return null;
  return {ax: Number(BigInt.asUintN(16, quotient)), dx: Number(BigInt.asUintN(16, dividend % divisor))};
}
const kinds = ['unsigned', 'signed'];
const anchors = {
  unsigned: [
    {dx: 0, ax: 17, divisor: 5, result: {ax: 3, dx: 2}},
    {dx: 0, ax: 0xffff, divisor: 1, result: {ax: 0xffff, dx: 0}},
    {dx: 1, ax: 0, divisor: 2, result: {ax: 0x8000, dx: 0}},
    {dx: 1, ax: 0xffff, divisor: 2, result: {ax: 0xffff, dx: 1}},
    {dx: 2, ax: 0, divisor: 2, result: null},
    {dx: 0, ax: 0, divisor: 0, result: null},
    {dx: 0xffff, ax: 0xffff, divisor: 0xffff, result: null},
    {dx: 0xffff, ax: 0, divisor: 0xffff, result: null},
  ],
  signed: [
    {dx: 0xffff, ax: 0xfff9, divisor: 3, result: {ax: 0xfffe, dx: 0xffff}},
    {dx: 0, ax: 7, divisor: 0xfffd, result: {ax: 0xfffe, dx: 1}},
    {dx: 0xffff, ax: 0x8000, divisor: 1, result: {ax: 0x8000, dx: 0}},
    {dx: 0, ax: 0x8000, divisor: 1, result: null},
    {dx: 0xffff, ax: 0x8000, divisor: 0xffff, result: null},
    {dx: 0x8000, ax: 0, divisor: 0xffff, result: null},
    {dx: 0xffff, ax: 0xffff, divisor: 0x8000, result: {ax: 0, dx: 0xffff}},
    {dx: 1, ax: 0x7ffe, divisor: 3, result: {ax: 0x7fff, dx: 1}},
  ],
};
for (const kind of kinds) for (const row of anchors[kind]) assert.deepEqual(divided(kind, row.dx, row.ax, row.divisor), row.result, 'literal quotient/remainder/zero/range/min32 anchor');
const lowWord = (parent, value) => Math.floor(parent / 65536) * 65536 + value;
const group = kind => kind === 'unsigned' ? 0x30 : 0x38;
const registerInstruction = (kind, source) => Buffer.from([0x66, 0xf7, 0xc0 | group(kind) | source]);
const memoryInstruction = (kind, tail) => Buffer.from([0x66, 0xf7, tail[0] | group(kind), ...tail.slice(1)]);
function memorySpec(kind, address = DATA + 8) {
  return {kind, address, memory: true, bytes: memoryInstruction(kind, [5, ...words([address])])};
}
function registerSpec(kind, source) {return {kind, source, memory: false, bytes: registerInstruction(kind, source)};}
function divide(c, module, row, value, label) {
  const before = cpu(c), expected = divided(row.kind, before.registers[2] % 65536, before.registers[0] % 65536, value);
  const next = {...before, registers: [...before.registers]};
  if (expected !== null) {
    next.pc += row.bytes.length; next.registers[0] = lowWord(before.registers[0], expected.ax); next.registers[2] = lowWord(before.registers[2], expected.dx);
  }
  run(c, module, 1, next, {label, reason: expected === null ? 10 : 1, retired: expected === null ? 0 : 1, helperVersion: row.memory ? 2 : null, helperValue: value});
  if (expected === null) counts.divide_errors++;
  return expected;
}

for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries);
  for (const kind of kinds) {
    const rows = Array.from({length: 8}, (_, source) => registerSpec(kind, source));
    const module = compile(c, rows.map((row, i) => block(PC + i * 16, row.bytes, false)));
    for (const [i, row] of rows.entries()) for (const pair of anchors[kind]) for (const flags of [2, 0xcd7]) {
      const r = registers(); r[0] = lowWord(r[0], pair.ax); r[2] = lowWord(r[2], pair.dx);
      if (i !== 0 && i !== 2) r[i] = lowWord(r[i], pair.divisor);
      const divisor = r[i] % 65536; seed(c, PC + i * 16, r, flags);
      const result = divide(c, module, row, divisor, 'all eight original divisors including AX/DX aliases preserve raw FLAGS');
      if (kind === 'unsigned' && i === 2) assert.equal(result, null, 'unsigned DIV DX cannot fit a WORD quotient'); counts.matrix++;
    }
    const literalRows = anchors[kind].map(() => registerSpec(kind, 1));
    const literalModule = compile(c, literalRows.map((row, i) => block(PC + 0x100 + i * 16, row.bytes, false)));
    for (const [i, row] of literalRows.entries()) {
      const pair = anchors[kind][i], r = registers(); r[0] = lowWord(r[0], pair.ax); r[2] = lowWord(r[2], pair.dx); r[1] = lowWord(r[1], pair.divisor);
      seed(c, PC + 0x100 + i * 16, r);
      assert.deepEqual(divide(c, literalModule, row, pair.divisor, 'literal WORD quotient bounds and signed remainder signs; no Wasm trap'), pair.result); counts.anchors++;
    }
  }
  assert.equal(c.api.close(), 0);
}

for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries);
  for (const [index, kind] of kinds.entries()) {
    const row = memorySpec(kind), module = compile(c, [block(PC + index * 16, row.bytes)]);
    for (const pair of anchors[kind]) for (const flags of [2, 0xcd7]) {
      const bytes = Buffer.concat([Buffer.from([0x55]), word(pair.divisor), Buffer.from([0xaa])]); upload(c, row.address - 1, bytes); host(c, 'protect', [DATA, 1, 1]);
      const r = registers(); r[0] = lowWord(r[0], pair.ax); r[2] = lowWord(r[2], pair.dx); seed(c, PC + index * 16, r, flags);
      assert.deepEqual(divide(c, module, row, pair.divisor, 'read-only real divisor produces quotient/remainder or precise V5 divide error'), pair.result);
      checkRam(c, row.address - 1, bytes); host(c, 'protect', [DATA, 1, 3]); counts.matrix++;
    }
  }
  assert.equal(c.api.close(), 0);
}

for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries);
  for (const kind of kinds) {
    const rows = Array.from({length: 8}, (_, carrier) => {
      const r = registers(); r[carrier] = 0xa53c5008;
      const tail = carrier === 4 ? [4, 0x24] : carrier === 5 ? [0x45, 0] : [carrier];
      return {kind, memory: true, address: 0xa53c5008, r, bytes: memoryInstruction(kind, tail)};
    });
    const scaled = [
      {tail: [0x44, 0x8d, 0xfc], values: {1: 0x40000002, 5: 0xa53c5004}},
      {tail: [4, 0x85, ...words([0xa53c5000])], values: {0: 0x40000002}},
      {tail: [0x84, 0x80, ...words([0x053c5003])], values: {0: 0x20000001}},
      {tail: [0x44, 0x24, 0xff], values: {4: 0xa53c5009}},
    ];
    for (const row of scaled) {
      const r = registers(); for (const [index, value] of Object.entries(row.values)) r[index] = value;
      rows.push({...row, kind, address: 0xa53c5008, memory: true, r, bytes: memoryInstruction(kind, row.tail)});
    }
    for (let start = 0; start < rows.length; start += 8) {
      const selected = rows.slice(start, start + 8), module = compile(c, selected.map((row, i) => block(PC + i * 16, row.bytes)));
      for (const [i, row] of selected.entries()) for (const value of [0, 0x8000]) {
        const bytes = Buffer.concat([Buffer.from([0x55]), word(value), Buffer.from([0xaa])]); upload(c, row.address - 1, bytes); seed(c, PC + i * 16, row.r);
        divide(c, module, row, value, 'old full32 AX/DX/other base/index/both/scaled/wrapped EA precedes quotient/remainder publication');
        checkRam(c, row.address - 1, bytes); counts.addresses++;
      }
    }
  }
  const boundaries = kinds.flatMap(kind => [memorySpec(kind, DATA + 4095), memorySpec(kind, 0xfffffffe)]);
  const boundaryModule = compile(c, boundaries.map((row, i) => block(PC + 0x200 + i * 16, row.bytes)));
  for (const [i, row] of boundaries.entries()) for (const value of [0, 3]) {
    upload(c, row.address, word(value)); const r = registers(); r[0] = lowWord(r[0], 17); r[2] = lowWord(r[2], 0); seed(c, PC + 0x200 + i * 16, r);
    assert.deepEqual(divide(c, boundaryModule, row, value, 'crosspage/last valid WORD read before zero-divisor handling'), value === 0 ? null : {ax: 5, dx: 2});
    checkRam(c, row.address, word(value)); counts.boundaries++;
  }
  const codeRows = kinds.map((kind, i) => ({...memorySpec(kind, PC + 0x300 + i * 32), pc: PC + 0x300 + i * 32}));
  const codeModule = compile(c, codeRows.map(row => block(row.pc, row.bytes)), 5);
  for (const row of codeRows) {
    const r = registers(); r[0] = lowWord(r[0], 1); r[2] = lowWord(r[2], 0); seed(c, row.pc, r);
    assert.deepEqual(divide(c, codeModule, row, 0xf766, 'readable executable own prefix WORD is the divisor without SMC'), {ax: 0, dx: 1});
    input(c, 48, words([row.pc])); assert.deepEqual(divide(c, codeModule, row, 0xf766, 'same current owner consumes returned1:0 dividend with code divisor'), row.kind === 'unsigned' ? {ax: 1, dx: 2202} : {ax: 0xffe3, dx: 1678});
    run(c, codeModule, 2, {...cpu(c), pc: row.pc + row.bytes.length + 2}, {label: 'V5 same-owner JMP suffix retains budget for cold lookup', reason: 3});
    checkRam(c, row.pc, Buffer.concat([row.bytes, Buffer.from([0xeb, 0])])); counts.code_reads++;
  }
  assert.equal(c.api.close(), 0);
}

for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries), mov = Buffer.from([0xbb, 0x44, 0x33, 0x22, 0x11]);
  const rows = [...kinds, ...kinds].map(kind => ({...memorySpec(kind), bytes: memoryInstruction(kind, [7])}));
  const module = compile(c, rows.map((row, i) => block(PC + i * 32, Buffer.concat([mov, row.bytes]))));
  const control = registers(); control[7] = 0x7008;
  seed(c, PC + 5, control); run(c, module, 0, cpu(c), {label: 'V5 zero budget precedes unmapped divisor read', retired: 0});
  input(c, 96, words([1])); run(c, module, 1, cpu(c), {label: 'V5 cancel precedes unmapped divisor read', reason: 2, retired: 0}); input(c, 96, words([0])); counts.controls += 2;
  for (const [index, row] of rows.slice(0, 2).entries()) for (const kind of ['unmapped', 'none', 'write-only', 'execute-only', 'cross-unmapped', 'cross-write-only', 'overflow']) {
    const crossing = kind.startsWith('cross'), overflow = kind === 'overflow', missing = kind.includes('unmapped');
    const address = overflow ? 0xffffffff : crossing ? 0x7fff : 0x7008, mapped = overflow ? 0 : crossing && !missing ? 2 : missing && !crossing ? 0 : 1;
    const unchanged = Buffer.from(crossing && missing ? [0x55, 0] : [0x55, 0, 0, 0xaa]);
    if (mapped) {host(c, 'map', [0x7000, mapped, 3]); upload(c, crossing ? 0x7ffe : 0x7007, unchanged);}
    if (!missing && !overflow) host(c, 'protect', [crossing ? 0x8000 : 0x7000, 1, kind === 'none' ? 0 : kind === 'execute-only' ? 4 : 2]);
    if (overflow) upload(c, 0xfffffffe, Buffer.from([0x55, 0]));
    const r = registers(); r[0] = lowWord(r[0], 17); r[2] = lowWord(r[2], 0); r[7] = address;
    const before = seed(c, PC + index * 32, r), stopped = {...before, registers: [...before.registers], pc: before.pc + 5}; stopped.registers[3] = 0x11223344;
    const fault = overflow ? undefined : {detail: missing ? 1 : 2, address: crossing ? 0x8000 : address, access: 1};
    run(c, module, 2, stopped, {label: row.kind + ' read fault precedes zero-divisor error after committed MOV', reason: overflow ? 11 : 5, retired: 1, fault, helperVersion: overflow ? null : 2, exitVersion: overflow ? 6 : 5});
    run(c, module, 1, stopped, {label: kind + ' unchanged read retry keeps both parents/FLAGS', reason: overflow ? 11 : 5, retired: 0, fault, helperVersion: overflow ? null : 2, exitVersion: overflow ? 6 : 5}); counts.faults += 2;
    if (crossing && missing) checkRam(c, 0x7ffe, unchanged);
    if (overflow) checkRam(c, 0xfffffffe, Buffer.from([0x55, 0]));
    if (!overflow) {
      if (missing) host(c, 'map', [crossing ? 0x8000 : 0x7000, 1, 1]);
      else host(c, 'protect', [crossing ? 0x8000 : 0x7000, 1, 1]);
      assert.deepEqual(cpu(c), stopped, 'map/protect-only repair preserves stopped CPU');
      assert.equal(divide(c, module, {...row, address}, 0, 'permission/map repair exposes successful read16 zero before precise divide error'), null);
      assert.deepEqual(cpu(c), stopped); counts.read_repairs++; counts.retries++;
      host(c, 'protect', [0x7000, crossing ? 2 : 1, 3]); upload(c, address, word(5)); host(c, 'protect', [0x7000, crossing ? 2 : 1, 1]);
      assert.deepEqual(cpu(c), stopped, 'declared RAM-only divisor repair preserves stopped CPU');
      assert.deepEqual(divide(c, module, {...row, address}, 5, 'RAM-only divisor repair resumes17/5 without replay or CPU reseeding'), {ax: 3, dx: 2});
      checkRam(c, address, word(5)); counts.divisor_repairs++; counts.retries++; host(c, 'unmap', [0x7000, crossing ? 2 : 1]);
    }
  }
  const arithmeticCases = [
    {kind: 'unsigned', dx: 0, ax: 17, divisor: 0, repair: 5},
    {kind: 'unsigned', dx: 1, ax: 0, divisor: 1, repair: 2},
    {kind: 'unsigned', dx: 0xffff, ax: 0xffff, divisor: 0xffff},
    {kind: 'signed', dx: 0, ax: 7, divisor: 0, repair: 3},
    {kind: 'signed', dx: 0, ax: 0x8000, divisor: 1, repair: 2},
    {kind: 'signed', dx: 0xffff, ax: 0x8000, divisor: 0xffff, repair: 1},
    {kind: 'signed', dx: 0x8000, ax: 0, divisor: 0xffff},
  ];
  for (const witness of arithmeticCases) {
    const index = kinds.indexOf(witness.kind), row = rows[index], address = DATA + 8, r = registers(); r[0] = lowWord(r[0], witness.ax); r[2] = lowWord(r[2], witness.dx); r[7] = address;
    assert.equal(divided(witness.kind, witness.dx, witness.ax, witness.divisor), null);
    upload(c, address - 1, Buffer.concat([Buffer.from([0x55]), word(witness.divisor), Buffer.from([0xaa])])); host(c, 'protect', [DATA, 1, 1]);
    const before = seed(c, PC + index * 32, r), stopped = {...before, registers: [...before.registers], pc: before.pc + 5}; stopped.registers[3] = 0x11223344;
    run(c, module, 2, stopped, {label: 'successful divisor read precedes zero/range/min32 divide error without Wasm trap', reason: 10, retired: 1, helperVersion: 2, helperValue: witness.divisor});
    run(c, module, 1, stopped, {label: 'unchanged arithmetic error retains raw FLAGS and both parents', reason: 10, retired: 0, helperVersion: 2, helperValue: witness.divisor}); counts.divide_errors += 2;
    let value = witness.divisor;
    if (witness.repair !== undefined) {
      host(c, 'protect', [DATA, 1, 3]); upload(c, address, word(witness.repair)); host(c, 'protect', [DATA, 1, 1]);
      assert.deepEqual(cpu(c), stopped, 'divisor-only repair keeps current dividend and stopped CPU');
      assert.notEqual(divide(c, module, {...row, address}, witness.repair, 'divisor-only quotient-range repair resumes the stopped instruction'), null);
      counts.divisor_repairs++; counts.retries++; value = witness.repair;
    }
    checkRam(c, address - 1, Buffer.concat([Buffer.from([0x55]), word(value), Buffer.from([0xaa])])); host(c, 'protect', [DATA, 1, 3]);
  }
  for (const [offset, row] of rows.slice(2).entries()) {
    const address = DATA + 12, nextAddress = DATA + 14, r = registers(); r[0] = lowWord(r[0], 17); r[2] = lowWord(r[2], row.kind === 'unsigned' ? 1 : 0); r[7] = address;
    upload(c, address - 1, Buffer.from([0x55, 0, 0, 0, 0, 0xaa])); host(c, 'protect', [DATA, 1, 2]);
    const before = seed(c, PC + (2 + offset) * 32, r), stopped = {...before, registers: [...before.registers], pc: before.pc + 5}; stopped.registers[3] = 0x11223344;
    const fault = {detail: 2, address, access: 1};
    run(c, module, 2, stopped, {label: 'current-input witness first faults before zero-divisor handling', reason: 5, retired: 1, fault, helperVersion: 2});
    run(c, module, 1, stopped, {label: 'current-input witness unchanged read retry', reason: 5, retired: 0, fault, helperVersion: 2}); counts.faults += 2;
    const value = row.kind === 'unsigned' ? 5 : 3;
    host(c, 'protect', [DATA, 1, 3]); upload(c, nextAddress, word(value)); host(c, 'protect', [DATA, 1, 1]);
    const current = {...stopped, registers: [...stopped.registers]}; current.registers[0] = row.kind === 'unsigned' ? 0xbaff0017 : 0xbafffff9; current.registers[2] = row.kind === 'unsigned' ? 0xcafe0000 : 0xcafeffff; current.registers[7] = nextAddress;
    input(c, 16, words([current.registers[0]])); input(c, 24, words([current.registers[2]])); input(c, 44, words([current.registers[7]]));
    assert.deepEqual(cpu(c), current, 'declared current divisor/EA/DXAX/both-high16 edits preserve the remaining stopped CPU');
    const wanted = row.kind === 'unsigned' ? {ax: 4, dx: 3} : {ax: 0xfffe, dx: 0xffff};
    assert.deepEqual(divide(c, module, {...row, address: nextAddress}, value, 'retry uses both current dividend halves/new divisor/EA and both current parent highs'), wanted);
    assert.equal(cpu(c).registers[0], lowWord(current.registers[0], wanted.ax)); assert.equal(cpu(c).registers[2], lowWord(current.registers[2], wanted.dx));
    checkRam(c, address - 1, Buffer.from([0x55, 0, 0, value, 0, 0xaa])); host(c, 'protect', [DATA, 1, 3]); counts.retries++; counts.current_inputs++;
  }

  const first = registerSpec('signed', 1), second = registerSpec('unsigned', 1), third = registerSpec('signed', 2);
  const setOverflowCarry = Buffer.from([0x0f, 0x90, 0xc3, 0x0f, 0x92, 0xc7]);
  const live = Buffer.concat([first.bytes, second.bytes, setOverflowCarry, third.bytes]);
  const liveModule = compile(c, [block(PC + 0x200, live, false)]);
  const liveRegisters = registers(); liveRegisters[0] = lowWord(liveRegisters[0], 17); liveRegisters[2] = lowWord(liveRegisters[2], 0); liveRegisters[1] = lowWord(liveRegisters[1], 5); seed(c, PC + 0x200, liveRegisters);
  assert.deepEqual(divide(c, liveModule, first, 5, 'live signed17/5 returns quotient3/remainder2 with entire FLAGS retained'), {ax: 3, dx: 2});
  assert.deepEqual(divide(c, liveModule, second, 5, 'live unsigned current2:3 dividend uses both returned halves'), {ax: 0x6667, dx: 0});
  let old = cpu(c), next = {...old, registers: [...old.registers], pc: old.pc + 6}; next.registers[3] = lowWord(old.registers[3], 0x0101);
  run(c, liveModule, 2, next, {label: 'SETO BL/SETB BH consume preserved raw FLAGS after both divisions', retired: 2});
  assert.equal(divide(c, liveModule, third, 0, 'live returned remainder DX0 is the next divisor and produces precise divide error'), null);
  assert.equal(divide(c, liveModule, third, 0, 'unchanged current-remainder zero-divisor retry retains quotient/FLAGS/retirement'), null);
  counts.consumers++; assert.equal(c.api.close(), 0);
}
assert.deepEqual(sources(), sourcePins, 'risk-relevant sources stayed fixed');
assert.equal(counts.contexts, 16); assert.equal(counts.runs, 1800); assert.equal(counts.retired, 952);
assert.equal(counts.matrix, 1152); assert.equal(counts.anchors, 64); assert.equal(counts.addresses, 192);
assert.equal(counts.boundaries, 32); assert.equal(counts.faults, 128); assert.equal(counts.retries, 124);
assert.equal(counts.current_inputs, 8); assert.equal(counts.consumers, 4); assert.equal(counts.controls, 8);
assert.equal(counts.code_reads, 8); assert.equal(counts.divide_errors, 808); assert.equal(counts.read_repairs, 48);
assert.equal(counts.divisor_repairs, 68); assert.equal(modules.length, 56);
const result = {status: 'ok', engine_sha256: engine.sha256, source_pins: sourcePins, counts, modules,
  limits: ['finite exact single66 WORD F7/6 DIV and F7/7 IDIV register/memory in four actual bound profiles, with native standalone register coverage separate',
    'independent BigInt32 DX:AX dividend/unsigned16 or signed16 divisor, truncated-toward-zero quotient/remainder and WORD quotient bounds',
    'both parent high16 halves and entire valid raw FLAGS retained; undefined hardware flags follow existing Ring3 preserve policy',
    'all division-unit exits use existingV5; zero/range/min32 errors return reason10 without a Wasm trap or retiring the faulting instruction',
    'memory imports only guard/read16; read fault precedes divide error, successful read helpers retain actual divisor even on arithmetic failure',
    'map/protect-only read repair, RAM-only divisor repair and current-dividend/divisor/EA/high16 edits are distinct; pure live quotient/remainder/flag chain avoids reseeding',
    'full4612 arena preserves FP128; selected readonly RAM/RX code canaries never store or cause SMC; suffix budget2 preserves cold-lookup priority',
    'no exhaustive ISA, hardware undefined-bit equivalence, exception-delivery OS, throughput, PE/browser/SDK/game or CI claim']};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2) + '\n', {flag: 'wx'});
console.log(JSON.stringify({status: result.status, ...counts, modules: modules.length, output}));
