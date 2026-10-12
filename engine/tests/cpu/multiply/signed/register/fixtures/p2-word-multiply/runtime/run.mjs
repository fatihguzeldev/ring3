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
  'engine/tests/cpu/multiply/signed/register/native/word.rs', 'engine/tests/cpu/multiply/native.rs', 'engine/tests/support/cpu/prefix_admission.rs', 'engine/tests/support/cpu/word_prefix.rs', 'engine/tests/cpu/multiply/native.rs', 'engine/src/cpu/x86/decode/integer.rs', 'engine/src/cpu/x86/decode/profile.rs', 'engine/src/cpu/dbt/region.rs',
  'engine/src/cpu/dbt/wasm/integer/arithmetic.rs', 'engine/src/cpu/dbt/wasm/integer/bits.rs', 'engine/src/cpu/dbt/wasm/integer/carry_rotates.rs', 'engine/src/cpu/dbt/wasm/integer/decimal.rs', 'engine/src/cpu/dbt/wasm/integer/dispatch.rs', 'engine/src/cpu/dbt/wasm/integer/flags.rs', 'engine/src/cpu/dbt/wasm/integer/mod.rs', 'engine/src/cpu/dbt/wasm/integer/multiply_divide.rs', 'engine/src/cpu/dbt/wasm/integer/operands.rs', 'engine/src/cpu/dbt/wasm/integer/rotates.rs', 'engine/src/cpu/dbt/wasm/integer/shifts.rs', 'engine/src/cpu/dbt/wasm/memory.rs', 'engine/src/cpu/dbt/wasm/control.rs',
  'engine/src/cpu/dbt/wasm/memory/narrow.rs', 'engine/src/memory/space.rs',
  'engine/src/process/instance.rs', 'engine/src/process/execution/compiled.rs', 'engine/src/process/execution/authority.rs', 'engine/src/process/memory/address_space.rs', 'engine/src/process/memory/scalar.rs', 'engine/src/process/memory/scalar_tests.rs', 'engine/src/process/execution/mod.rs', 'engine/src/process/memory/mod.rs', 'engine/src/process/mod.rs', 'engine/src/process/execution/resident.rs', 'engine/src/process/execution/authority.rs', 'engine/src/process/execution/mod.rs', 'engine/src/process/mod.rs',
  'engine/tests/cpu/multiply/signed/register/wasm/word.rs', 'engine/tests/cpu/multiply/wasm.rs', 'engine/tests/support/wasm.rs', 'engine/tests/cpu/multiply/wasm.rs', 'engine/tests/support/wasm.rs',
  'engine/tests/cpu/multiply/signed/register/fixtures/p2-word-multiply/runtime/run.mjs',
];
const sources = () => Object.fromEntries(paths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = sources(), modules = [];
const counts = {contexts: 0, runs: 0, retired: 0, matrix: 0, anchors: 0, addresses: 0, boundaries: 0, faults: 0, retries: 0, current_inputs: 0, consumers: 0, controls: 0, code_reads: 0};
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
function run(c, module, budget, next, {label, reason = 1, retired = 1, fault, helperVersion = null, helperValue = 0, status = 0, exitVersion = 2} = {}) {
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
  assert.equal(c.api.open(8, c.id, 0x574d554c), 0); c.base = c.api.arena_ptr() >>> 0; input(c, 0, initial);
  for (const args of [[PC, 1, 7], [DATA, 2, 3], [0xa53c5000, 1, 3], [0xfffff000, 1, 3]]) host(c, 'map', args);
  return c;
}

// widened signed arithmetic and range comparison avoid the emitted narrow multiply/fit sequence.
const signed16 = value => value < 32768 ? value : value - 65536;
function multiplied(a, b, oldFlags) {
  const product = signed16(a) * signed16(b), overflow = product < -32768 || product > 32767;
  return {value: (product % 65536 + 65536) % 65536, flags: (oldFlags & 0x402) + (overflow ? 0x801 : 0)};
}
const anchors = [
  {a: 0, b: 0xffff, value: 0, flags: 0x402},
  {a: 1, b: 0x8000, value: 0x8000, flags: 0x402},
  {a: 0xffff, b: 0x8000, value: 0x8000, flags: 0xc03},
  {a: 0x7fff, b: 2, value: 0xfffe, flags: 0xc03},
  {a: 0x8000, b: 2, value: 0, flags: 0xc03},
  {a: 0xffff, b: 0xffff, value: 1, flags: 0x402},
  {a: 0x0100, b: 0xff80, value: 0x8000, flags: 0x402},
  {a: 0x0101, b: 0xff80, value: 0x7f80, flags: 0xc03},
  {a: 0x8000, b: 0x8000, value: 0, flags: 0xc03},
  {a: 0x7fff, b: 0xffff, value: 0x8001, flags: 0x402},
  {a: 0x8001, b: 0xffff, value: 0x7fff, flags: 0x402},
  {a: 0x00ff, b: 0xff80, value: 0x8080, flags: 0x402},
  {a: 0x7fff, b: 1, value: 0x7fff, flags: 0x402},
  {a: 0x8000, b: 1, value: 0x8000, flags: 0x402},
  {a: 0xfff9, b: 300, value: 0xf7cc, flags: 0x402},
  {a: 0xf7cc, b: 16, value: 0x7cc0, flags: 0xc03},
];
for (const row of anchors) assert.deepEqual(multiplied(row.a, row.b, 0xcd7), {value: row.value, flags: row.flags}, 'literal signed16 product/fit/zero-policy anchor');
const lowWord = (parent, value) => Math.floor(parent / 65536) * 65536 + value;
const registerInstruction = (destination, source) => Buffer.from([0x66, 0x0f, 0xaf, 0xc0 | destination << 3 | source]);
const memoryInstruction = (destination, tail) => Buffer.from([0x66, 0x0f, 0xaf, tail[0] | destination << 3, ...tail.slice(1)]);
function memorySpec(destination, address = DATA + 8) {
  return {destination, address, memory: true, bytes: memoryInstruction(destination, [5, ...words([address])])};
}
function registerSpec(destination, source) {return {destination, source, memory: false, bytes: registerInstruction(destination, source)};}
function multiply(c, module, row, value, label) {
  const before = cpu(c), expected = multiplied(before.registers[row.destination] % 65536, value, before.flags);
  const next = {...before, registers: [...before.registers], pc: before.pc + row.bytes.length, flags: expected.flags};
  next.registers[row.destination] = lowWord(before.registers[row.destination], expected.value);
  run(c, module, 1, next, {label, helperVersion: row.memory ? 2 : null, helperValue: value, exitVersion: module.memoryOps ? 2 : 1});
  return expected;
}

for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries), rows = Array.from({length: 8}, (_, destination) => registerSpec(destination, (destination + 1) % 8));
  const module = compile(c, rows.map((row, i) => block(PC + i * 16, row.bytes, false)));
  for (const [i, row] of rows.entries()) for (const pair of anchors.slice(0, 8)) for (const flags of [2, 0xcd7]) {
    const r = registers(); r[row.destination] = lowWord(r[row.destination], pair.a); r[row.source] = lowWord(r[row.source], pair.b);
    seed(c, PC + i * 16, r, flags); multiply(c, module, row, pair.b, 'all eight register destinations/sources retain dirty high16'); counts.matrix++;
  }
  const aliases = Array.from({length: 8}, (_, destination) => registerSpec(destination, destination));
  const aliasModule = compile(c, aliases.map((row, i) => block(PC + 0x100 + i * 16, row.bytes, false)));
  for (const [i, row] of aliases.entries()) for (const value of [0, 1, 0xffff, 0x8000]) for (const flags of [2, 0xcd7]) {
    const r = registers(); r[i] = lowWord(r[i], value); seed(c, PC + 0x100 + i * 16, r, flags);
    multiply(c, aliasModule, row, value, 'same-register source uses old low16 for both signed factors'); counts.matrix++;
  }
  for (let start = 0; start < anchors.length; start += 8) {
    const group = anchors.slice(start, start + 8), row = registerSpec(0, 2);
    const literalModule = compile(c, group.map((_, i) => block(PC + 0x200 + i * 16, row.bytes, false)));
    for (const [i, pair] of group.entries()) {
      const r = registers(); r[0] = lowWord(r[0], pair.a); r[2] = lowWord(r[2], pair.b); seed(c, PC + 0x200 + i * 16, r);
      assert.deepEqual(multiply(c, literalModule, row, pair.b, 'literal signed16 fit boundaries and truncated-zero overflow'), {value: pair.value, flags: pair.flags}); counts.anchors++;
    }
  }
  assert.equal(c.api.close(), 0);
}

for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries), rows = Array.from({length: 8}, (_, destination) => memorySpec(destination));
  const module = compile(c, rows.map((row, i) => block(PC + i * 16, row.bytes)));
  for (const [i, row] of rows.entries()) for (const pair of anchors.slice(0, 8)) for (const flags of [2, 0xcd7]) {
    const bytes = Buffer.concat([Buffer.from([0x55]), word(pair.b), Buffer.from([0xaa])]);
    upload(c, row.address - 1, bytes); host(c, 'protect', [DATA, 1, 1]);
    const r = registers(); r[row.destination] = lowWord(r[row.destination], pair.a); seed(c, PC + i * 16, r, flags);
    multiply(c, module, row, pair.b, 'read-only WORD source including zero destination still uses checked read16');
    checkRam(c, row.address - 1, bytes); host(c, 'protect', [DATA, 1, 3]); counts.matrix++;
  }
  assert.equal(c.api.close(), 0);
}

for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries), rows = Array.from({length: 8}, (_, destination) => {
    const r = registers(); r[destination] = 0xa53c5008;
    const tail = destination === 4 ? [4, 0x24] : destination === 5 ? [0x45, 0] : [destination];
    return {destination, memory: true, address: 0xa53c5008, r, bytes: memoryInstruction(destination, tail)};
  });
  const scaled = [
    {destination: 1, tail: [0x44, 0x8d, 0xfc], values: {1: 0x40000002, 5: 0xa53c5004}},
    {destination: 0, tail: [4, 0x85, ...words([0xa53c5000])], values: {0: 0x40000002}},
    {destination: 0, tail: [0x84, 0x80, ...words([0x053c5003])], values: {0: 0x20000001}},
    {destination: 4, tail: [0x44, 0x24, 0xff], values: {4: 0xa53c5009}},
  ];
  for (const row of scaled) {
    const r = registers(); for (const [index, value] of Object.entries(row.values)) r[index] = value;
    rows.push({...row, address: 0xa53c5008, memory: true, r, bytes: memoryInstruction(row.destination, row.tail)});
  }
  for (let start = 0; start < rows.length; start += 8) {
    const group = rows.slice(start, start + 8), module = compile(c, group.map((row, i) => block(PC + i * 16, row.bytes)));
    for (const [i, row] of group.entries()) for (const value of [0, 0xffff]) {
      const bytes = Buffer.concat([Buffer.from([0x55]), word(value), Buffer.from([0xaa])]); upload(c, row.address - 1, bytes); seed(c, PC + i * 16, row.r);
      multiply(c, module, row, value, 'old full32 destination base/index/both/scaled/wrapped EA precedes low16 writeback');
      checkRam(c, row.address - 1, bytes); counts.addresses++;
    }
  }
  const boundaries = [DATA + 4095, 0xfffffffe].flatMap(address => [memorySpec(0, address), memorySpec(2, address)]);
  const boundaryModule = compile(c, boundaries.map((row, i) => block(PC + 0x200 + i * 16, row.bytes)));
  for (const [i, row] of boundaries.entries()) for (const value of [0, 0x8001]) {
    upload(c, row.address, word(value)); const r = registers(); r[row.destination] = lowWord(r[row.destination], row.destination);
    seed(c, PC + 0x200 + i * 16, r); multiply(c, boundaryModule, row, value, 'crosspage/last valid WORD read including zero destination');
    checkRam(c, row.address, word(value)); counts.boundaries++;
  }
  const codeRows = [0, 1].map((destination, i) => memorySpec(destination, PC + 0x300 + i * 32));
  const codeModule = compile(c, codeRows.map(row => block(row.address, row.bytes)), 5);
  for (const row of codeRows) {
    const r = registers(); r[row.destination] = lowWord(r[row.destination], row.destination * 2); seed(c, row.address, r);
    multiply(c, codeModule, row, 0x0f66, 'readable executable own-code is a factor with no SMC exit');
    input(c, 48, words([row.address])); multiply(c, codeModule, row, 0x0f66, 'same compiled owner stays current and uses returned destination as next factor');
    run(c, codeModule, 2, {...cpu(c), pc: row.address + row.bytes.length + 2}, {label: 'same owner JMP suffix has budget remaining before cold lookup', reason: 3});
    checkRam(c, row.address, row.bytes); counts.code_reads++;
  }
  assert.equal(c.api.close(), 0);
}

for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries), row = {...memorySpec(0), bytes: memoryInstruction(0, [7])};
  const mov = Buffer.from([0xbb, 0x44, 0x33, 0x22, 0x11]);
  const module = compile(c, [block(PC, Buffer.concat([mov, row.bytes])), block(PC + 32, Buffer.concat([mov, row.bytes]))]);
  const control = registers(); control[7] = 0x7008;
  seed(c, PC + 5, control); run(c, module, 0, cpu(c), {label: 'zero budget avoids unmapped multiply source', retired: 0});
  input(c, 96, words([1])); run(c, module, 1, cpu(c), {label: 'cancel avoids unmapped multiply source', reason: 2, retired: 0}); input(c, 96, words([0])); counts.controls += 2;
  for (const a of [0, 0x7fff]) for (const kind of ['unmapped', 'none', 'write-only', 'execute-only', 'cross-unmapped', 'cross-write-only', 'overflow']) {
    const crossing = kind.startsWith('cross'), overflow = kind === 'overflow', missing = kind.includes('unmapped');
    const address = overflow ? 0xffffffff : crossing ? 0x7fff : 0x7008, mapped = overflow ? 0 : crossing && !missing ? 2 : missing && !crossing ? 0 : 1;
    const unchanged = Buffer.from(crossing && missing ? [0x55, 1] : [0x55, 1, 0x80, 0xaa]);
    if (mapped) {host(c, 'map', [0x7000, mapped, 3]); upload(c, crossing ? 0x7ffe : 0x7007, unchanged);}
    if (!missing && !overflow) host(c, 'protect', [crossing ? 0x8000 : 0x7000, 1, kind === 'none' ? 0 : kind === 'execute-only' ? 4 : 2]);
    if (overflow) upload(c, 0xfffffffe, Buffer.from([0x55, 1]));
    const r = registers(); r[0] = lowWord(r[0], a); r[7] = address;
    const before = seed(c, PC, r), stopped = {...before, registers: [...before.registers], pc: before.pc + 5}; stopped.registers[3] = 0x11223344;
    const fault = {detail: overflow ? 3 : missing ? 1 : 2, address: overflow ? address : crossing ? 0x8000 : address, access: 1};
    run(c, module, 2, stopped, {label: kind + ' read failure retains zero/nonzero destination and dirty FLAGS after MOV', reason: 5, retired: 1, fault, helperVersion: 2});
    run(c, module, 1, stopped, {label: kind + ' unchanged read retry', reason: 5, retired: 0, fault, helperVersion: 2}); counts.faults += 2;
    if (crossing && missing) checkRam(c, 0x7ffe, unchanged);
    if (overflow) checkRam(c, 0xfffffffe, Buffer.from([0x55, 1]));
    if (!overflow) {
      if (missing) host(c, 'map', [crossing ? 0x8000 : 0x7000, 1, 1]);
      else host(c, 'protect', [crossing ? 0x8000 : 0x7000, 1, 1]);
      assert.deepEqual(cpu(c), stopped, 'map/protect-only repair preserves stopped CPU');
      const value = missing ? crossing ? 1 : 0 : 0x8001;
      multiply(c, module, {...row, address}, value, 'repaired read consumes current RAM without replay or CPU reseeding');
      checkRam(c, address, word(value)); counts.retries++; host(c, 'unmap', [0x7000, crossing ? 2 : 1]);
    }
  }
  const address = DATA + 12, nextAddress = DATA + 14, r = registers(); r[0] = lowWord(r[0], 2); r[7] = address;
  upload(c, address - 1, Buffer.from([0x55, 0, 0, 0, 0, 0xaa])); host(c, 'protect', [DATA, 1, 2]);
  const before = seed(c, PC + 32, r), stopped = {...before, registers: [...before.registers], pc: before.pc + 5}; stopped.registers[3] = 0x11223344;
  const fault = {detail: 2, address, access: 1};
  run(c, module, 2, stopped, {label: 'current-input witness initially faults with zero RAM factor', reason: 5, retired: 1, fault, helperVersion: 2});
  run(c, module, 1, stopped, {label: 'current-input witness unchanged retry', reason: 5, retired: 0, fault, helperVersion: 2}); counts.faults += 2;
  host(c, 'protect', [DATA, 1, 3]); upload(c, nextAddress, word(0x8000)); host(c, 'protect', [DATA, 1, 1]);
  const current = {...stopped, registers: [...stopped.registers]}; current.registers[0] = 0xbaffffff; current.registers[7] = nextAddress;
  input(c, 16, words([current.registers[0]])); input(c, 44, words([current.registers[7]])); assert.deepEqual(cpu(c), current, 'declared current factor/EA/destination edits preserve the remaining stopped CPU');
  assert.deepEqual(multiply(c, module, {...row, address: nextAddress}, 0x8000, 'retry consumes both current signed factors, new EA and new high16'), {value: 0x8000, flags: 0xc03});
  assert.equal(cpu(c).registers[0], 0xbaff8000); checkRam(c, address - 1, Buffer.from([0x55, 0, 0, 0, 0x80, 0xaa]));
  host(c, 'protect', [DATA, 1, 3]); counts.retries++; counts.current_inputs++;

  const twice = memorySpec(0, DATA + 32), negate = registerSpec(0, 2), zero = memorySpec(0, DATA + 34);
  const setOverflowCarry = Buffer.from([0x0f, 0x90, 0xc3, 0x0f, 0x92, 0xc7]);
  const setZeroParity = Buffer.from([0x0f, 0x94, 0xc3, 0x0f, 0x9a, 0xc7]);
  const live = Buffer.concat([twice.bytes, setOverflowCarry, negate.bytes, setOverflowCarry, zero.bytes, setZeroParity]);
  const liveModule = compile(c, [block(PC + 0x100, live)]);
  upload(c, DATA + 31, Buffer.from([0x55, 2, 0, 0, 0, 0xaa])); host(c, 'protect', [DATA, 1, 1]);
  const liveRegisters = registers(); liveRegisters[0] = lowWord(liveRegisters[0], 0x7fff); liveRegisters[2] = lowWord(liveRegisters[2], 0xffff);
  seed(c, PC + 0x100, liveRegisters);
  assert.deepEqual(multiply(c, liveModule, twice, 2, 'live signed overflow returns low16 FFFE and CF/OF'), {value: 0xfffe, flags: 0xc03});
  let old = cpu(c), next = {...old, registers: [...old.registers], pc: old.pc + 6}; next.registers[3] = lowWord(old.registers[3], 0x0101);
  run(c, liveModule, 2, next, {label: 'SETO BL/SETB BH consume current overflow flags without touching AX', retired: 2});
  assert.deepEqual(multiply(c, liveModule, negate, 0xffff, 'live returned AX=-2 times current DX=-1 fits as positive2'), {value: 2, flags: 0x402});
  old = cpu(c); next = {...old, registers: [...old.registers], pc: old.pc + 6}; next.registers[3] = lowWord(old.registers[3], 0);
  run(c, liveModule, 2, next, {label: 'SETO/SETB observe cleared overflow after current-AX fit', retired: 2});
  assert.deepEqual(multiply(c, liveModule, zero, 0, 'live current AX times zero produces zero with undefined arithmetic flags cleared'), {value: 0, flags: 0x402});
  old = cpu(c); next = {...old, registers: [...old.registers], pc: old.pc + 6}; next.registers[3] = lowWord(old.registers[3], 0);
  run(c, liveModule, 2, next, {label: 'SETZ BL/SETP BH observe Ring3 cleared bits even after a zero product', retired: 2});
  checkRam(c, DATA + 31, Buffer.from([0x55, 2, 0, 0, 0, 0xaa])); counts.consumers++;
  assert.equal(c.api.close(), 0);
}
assert.deepEqual(sources(), sourcePins, 'risk-relevant sources stayed fixed');
assert.equal(counts.contexts, 16); assert.equal(counts.runs, 1700); assert.equal(counts.retired, 1644);
assert.equal(counts.matrix, 1280); assert.equal(counts.anchors, 64); assert.equal(counts.addresses, 96);
assert.equal(counts.boundaries, 32); assert.equal(counts.faults, 120); assert.equal(counts.retries, 52);
assert.equal(counts.current_inputs, 4); assert.equal(counts.consumers, 4); assert.equal(counts.controls, 8);
assert.equal(counts.code_reads, 8); assert.equal(modules.length, 44);
const result = {status: 'ok', engine_sha256: engine.sha256, source_pins: sourcePins, counts, modules,
  limits: ['finite exact single66 two-operand WORD IMUL register/memory in four actual bound profiles, with native standalone register coverage separate',
    'independent widened signed16 product/fit oracle and literal signed boundary/truncated-zero anchors; low16 result retains old destination high16',
    'CF/OF report signed16 fit; clearing PF/AF/ZF/SF follows Ring3 policy for undefined arithmetic flags, without hardware undefined-bit equality claim',
    'memory modules import only guard/read16, including zero destinations; read-only data and readable code never store or cause SMC',
    'real read16 failures and selected RAM/canary checks; map/protect-only repair and declared current-input retry are separate witnesses',
    'full4612 arena preserves FP128; live returned-AX and overflow/fit/zero flag consumers avoid hidden CPU reseeding',
    'no immediate or one-operand multiply, exhaustive ISA, throughput, PE/browser/SDK/game or CI claim']};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2) + '\n', {flag: 'wx'});
console.log(JSON.stringify({status: result.status, ...counts, modules: modules.length, output}));
