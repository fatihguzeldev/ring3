import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../../../../../support/wasm/engine.mjs';

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
  'engine/src/cpu/x86/decode/profile.rs', 'engine/src/cpu/dbt/region.rs',
  'engine/src/cpu/dbt/wasm/integer/arithmetic.rs', 'engine/src/cpu/dbt/wasm/integer/bits.rs', 'engine/src/cpu/dbt/wasm/integer/carry_rotates.rs', 'engine/src/cpu/dbt/wasm/integer/decimal.rs', 'engine/src/cpu/dbt/wasm/integer/dispatch.rs', 'engine/src/cpu/dbt/wasm/integer/flags.rs', 'engine/src/cpu/dbt/wasm/integer/mod.rs', 'engine/src/cpu/dbt/wasm/integer/multiply_divide.rs', 'engine/src/cpu/dbt/wasm/integer/operands.rs', 'engine/src/cpu/dbt/wasm/integer/rotates.rs', 'engine/src/cpu/dbt/wasm/integer/shifts.rs', 'engine/src/cpu/dbt/wasm/memory.rs',
  'engine/src/cpu/dbt/wasm/memory/narrow.rs', 'engine/src/cpu/dbt/wasm/abi.rs',
  'engine/src/cpu/dbt/wasm/emitter.rs', 'engine/src/process/wasm/mod.rs', 'engine/src/process/wasm/loading.rs', 'engine/src/process/wasm/execution.rs', 'engine/src/process/wasm/calls.rs', 'engine/src/process/wasm/memory.rs', 'engine/src/process/wasm/cpu.rs', 'engine/src/process/mod.rs',
  'engine/src/process/instance.rs', 'engine/src/process/execution/compiled.rs', 'engine/src/process/execution/authority.rs', 'engine/src/process/memory/address_space.rs', 'engine/src/process/memory/scalar.rs', 'engine/src/process/memory/scalar_tests.rs', 'engine/src/process/execution/mod.rs', 'engine/src/process/memory/mod.rs', 'engine/src/process/mod.rs', 'engine/src/process/execution/resident.rs', 'engine/src/process/execution/authority.rs', 'engine/src/process/execution/mod.rs', 'engine/src/process/mod.rs',
  'engine/tests/cpu/moves/cases/native/cpu_word_byte_extend.rs', 'engine/tests/cpu/moves/native.rs', 'engine/tests/support/cpu/prefix_admission.rs', 'engine/tests/support/cpu/word_prefix.rs', 'engine/tests/cpu/moves/native.rs', 'engine/tests/cpu/moves/cases/wasm/cpu_word_byte_extend_wasm.rs', 'engine/tests/cpu/moves/wasm.rs', 'engine/tests/support/wasm.rs', 'engine/tests/cpu/moves/wasm.rs', 'engine/tests/support/wasm.rs',
  'engine/tests/cpu/moves/fixtures/p2-word-byte-extend/runtime/run.mjs', 'engine/tests/support/wasm/engine.mjs',
];
const sources = () => Object.fromEntries(paths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = sources(), modules = [];
const counts = {contexts: 0, runs: 0, retired: 0, register_pairs: 0, byte_inputs: 0, flag_inputs: 0, anchors: 0, memory_destinations: 0, addresses: 0, boundaries: 0, faults: 0, retries: 0, current_inputs: 0, repeats: 0, controls: 0, retained_units: 0, code_reads: 0, consumers: 0};
const words = values => {const b = Buffer.alloc(values.length * 4); values.forEach((v, i) => b.writeUInt32LE(v >>> 0, i * 4)); return b;};
function record(magic, size, fields, version = 1) {
  const b = Buffer.alloc(size); b.write(magic); b.writeUInt16LE(version, 4);
  b.writeUInt16LE(1, 6); b.writeUInt32LE(size, 8);
  fields.forEach((v, i) => b.writeUInt32LE(v >>> 0, 16 + i * 4)); return b;
}
const state = c => record('R3ST', 56, [...c.registers, c.pc, c.flags]);
const exit = (reason, n, f, version = 2) => record('R3EX', 40,
  [reason, n, f?.detail ?? 0, f?.address ?? 0, f?.access ?? 0, f ? 1 : 0], version);
const helper = (version, value = 0, f) => record('R3MH', 40,
  [f ? 1 : 0, value, f?.detail ?? 0, f?.address ?? 0, f?.access ?? 0, 1], version);
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
function run(c, module, budget, next, {label, reason = 1, retired = 1, fault, helperVersion = null, helperValue = 0, status = 0, exitVersion = module.memoryOps ? 2 : 1} = {}) {
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
  input(c, 140, words(blocks.flatMap(b => c.entries ? [b.pc] : [b.pc, b.bytes.length])));
  const method = c.owner === 'resident' ? (c.entries ? 'compile_resident_entries' : 'compile_resident') : (c.entries ? 'compile_entries' : 'compile');
  host(c, method, c.entries ? [blocks.length, 0] : [blocks.length]);
  const b = arena(c), binding = c.owner === 'resident'
    ? {low: b.readUInt32LE(148), high: b.readUInt32LE(152), pointer: b.readUInt32LE(156), length: b.readUInt32LE(160)}
    : {generation: c.api.generation(), pointer: c.api.module_ptr() >>> 0, length: c.api.module_len() >>> 0};
  const bytes = Buffer.from(new Uint8Array(c.memory.buffer, binding.pointer, binding.length));
  const module = new WebAssembly.Module(bytes), imports = WebAssembly.Module.imports(module);
  const memoryOps = blocks.some(b => b.memory !== false);
  const names = [c.owner === 'resident' ? 'guard_resident' : 'guard', ...(memoryOps ? ['read8'] : [])];
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
  for (const name of ['open', 'close', 'arena_ptr', 'map', 'unmap', 'protect', 'upload', 'read8',
    'compile', 'compile_entries', 'compile_resident', 'compile_resident_entries', 'generation', 'module_ptr', 'module_len', 'guard', 'guard_resident'])
    c.api[name] = instance.exports['ring3_abi_v1_' + name];
  assert.equal(c.api.open(8, c.id, 0x57425854), 0); c.base = c.api.arena_ptr() >>> 0; input(c, 0, initial);
  for (const args of [[PC, 1, 7], [DATA, 1, 3], [0xa53c5000, 1, 3], [0xfffff000, 1, 3]]) host(c, 'map', args);
  return c;
}

// arithmetic range and digit extraction avoid reusing Wasm masks, shifts or sign-extension instructions.
const lowWord = (parent, value) => Math.floor(parent / 65536) * 65536 + value;
const extended = (kind, value) => kind === 'sign' && value >= 128 ? value + 65280 : value;
const sourceParent = source => source % 4;
const sourceByte = (r, source) => Math.floor(r[sourceParent(source)] / (source < 4 ? 1 : 256)) % 256;
function setByte(r, source, value) {
  const index = sourceParent(source), scale = source < 4 ? 1 : 256;
  r[index] += (value - sourceByte(r, source)) * scale;
}
const kinds = ['zero', 'sign'], opcode = kind => kind === 'zero' ? 0xb6 : 0xbe;
const flags = Array.from({length: 128}, (_, n) => 2 + [1, 4, 16, 64, 128, 1024, 2048].reduce((sum, bit, i) => sum + Math.floor(n / 2 ** i) % 2 * bit, 0));
assert.equal(new Set(flags).size, 128); assert.ok(flags.every(value => (value & 2) && !(value & ~0xcd7)));
const anchorFlags = [2, 0xcd7, 0x443, 0x892], anchorBytes = [0, 0x7f, 0x80, 0xff];
const anchorValues = {zero: [0, 0x7f, 0x80, 0xff], sign: [0, 0x7f, 0xff80, 0xffff]};
for (const kind of kinds) for (const [i, value] of anchorBytes.entries()) assert.equal(extended(kind, value), anchorValues[kind][i], 'literal unsigned/signed BYTE-to-WORD anchor');
const registerSpec = (kind, destination, source, pc) => ({kind, destination, source, pc, memory: false, bytes: Buffer.from([0x66, 0x0f, opcode(kind), 0xc0 | destination << 3 | source])});
const memorySpec = (kind, destination, tail, pc) => ({kind, destination, pc, memory: true, bytes: Buffer.from([0x66, 0x0f, opcode(kind), tail[0] | destination << 3, ...tail.slice(1)])});
const absolute = (kind, destination, address, pc) => ({...memorySpec(kind, destination, [5, ...words([address])], pc), address});
function operation(c, module, row, value, label, exitVersion = module.memoryOps ? 2 : 1) {
  const before = cpu(c), actualByte = row.memory ? value : sourceByte(before.registers, row.source);
  if (value !== undefined) assert.equal(actualByte, value, 'authored source byte matches independent original-parent extraction');
  const next = {...before, registers: [...before.registers], pc: before.pc + row.bytes.length};
  next.registers[row.destination] = lowWord(before.registers[row.destination], extended(row.kind, actualByte));
  run(c, module, 1, next, {label, helperVersion: row.memory ? 2 : null, helperValue: actualByte, exitVersion}); return next;
}
const canary = Buffer.from([0x55, 0xa5, 0x66, 0x67, 0xf0, 0xf2, 0xf3, 0xaa]);
const live = Buffer.from([
  0x66, 0x0f, 0xbe, 0x0e, // movsx cx,byte [esi] returns -3.
  0x66, 0x0f, 0xb6, 0xc4, // movzx ax,ah captures original AH80 before replacing AX.
  0x66, 0xf7, 0xe9, 0x66, 0xf7, 0xf9, // imul cx returns -384; idiv cx consumes it and returns128/remainder0.
  0x0f, 0x92, 0xc3, 0x0f, 0x90, 0xc7, 0xeb, 0, // setb bl; seto bh; suffix JMP.
]);

for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries), livePc = PC + 0x500, allRows = [];
  for (const [k, kind] of kinds.entries()) for (let destination = 0; destination < 8; destination++) for (let source = 0; source < 8; source++)
    allRows.push(registerSpec(kind, destination, source, PC + k * 0x200 + destination * 0x40 + source * 4));
  const blocks = kinds.map((kind, k) => Array.from({length: 8}, (_, destination) => block(PC + k * 0x200 + destination * 0x40,
    Buffer.concat(allRows.filter(row => row.kind === kind && row.destination === destination).map(row => row.bytes)), false)));
  for (const b of blocks.flat()) upload(c, b.pc, b.bytes); upload(c, livePc, live);
  upload(c, DATA, canary); upload(c, DATA + 0x80, Buffer.from([0xfd])); host(c, 'protect', [DATA, 1, 1]); host(c, 'protect', [PC, 1, 5]); let firstModule;
  for (const [k, kind] of kinds.entries()) for (const [groupIndex, group] of [blocks[k].slice(0, 4), blocks[k].slice(4)].entries()) {
    assert.equal(group.reduce((sum, b) => sum + (b.bytes.length - 2) / 4 + 1, 0), 36, 'four destination blocks fit the unchanged64-instruction unit limit');
    const module = compile(c, group); if (k === 0 && groupIndex === 0) firstModule = module;
    for (const row of allRows.filter(row => row.kind === kind && Math.floor(row.destination / 4) === groupIndex)) {
      const r = registers().map((value, i) => ((0x8001 + i * 0x1357) % 65536) * 65536 + value % 65536), value = (row.destination + row.source) % 2 ? 0x80 : 0x7f;
      r[row.destination] = lowWord(r[row.destination], 0xa55a); setByte(r, row.source, value);
      seed(c, row.pc, r, anchorFlags[(row.destination + row.source) % 4]); operation(c, module, row, value, 'all128 typed source/destination/kind identities including high-byte and same-parent aliases'); counts.register_pairs++;
    }
    if (groupIndex !== 0) continue;
    const row = allRows.find(row => row.kind === kind && row.destination === k && row.source === k + 4);
    for (let value = 0; value < 256; value++) {
      const r = registers(); r[row.destination] = (0x8123 + value) * 65536 + 0x5aa5; setByte(r, row.source, value);
      seed(c, row.pc, r, flags[value % 128]); operation(c, module, row, value, 'all256 original AH/CH bytes and all128 valid FLAGS with own-high retention'); counts.byte_inputs++; if (value < 128) counts.flag_inputs++;
    }
    for (const [i, value] of anchorBytes.entries()) {
      const r = registers(); r[row.destination] = 0xcafe5aa5; setByte(r, row.source, value); seed(c, row.pc, r, anchorFlags[i]);
      assert.equal(operation(c, module, row, value, 'literal alias-source BYTE-to-WORD result').registers[row.destination], 0xcafe0000 + anchorValues[kind][i]); counts.anchors++;
    }
    const r = registers(); r[row.destination] = 0xcafe80a5; seed(c, row.pc, r);
    operation(c, module, row, 0x80, 'first same-parent high-byte capture'); input(c, 48, words([row.pc]));
    operation(c, module, row, kind === 'zero' ? 0 : 0xff, 'only PC rewound; current AH/CH is consumed after prior publication'); counts.repeats += 2;
    const current = cpu(c); current.pc = row.pc; current.flags = 0x892; current.registers[row.destination] = 0x7fff7f33;
    input(c, 0, state(current)); operation(c, module, row, 0x7f, 'explicit current source byte/destination-high/FLAGS inputs'); counts.current_inputs++;
    seed(c, row.pc, r); run(c, module, 0, cpu(c), {label: 'pure zero budget preserves full CPU/helper/FP', retired: 0});
    input(c, 96, words([1])); run(c, module, 1, cpu(c), {label: 'pure cancel before target', reason: 2, retired: 0});
    run(c, module, 0, cpu(c), {label: 'pure cancel precedes exhausted budget', reason: 2, retired: 0}); input(c, 96, words([0]));
    const suffix = blocks[k][k].pc + 32; input(c, 48, words([suffix]));
    run(c, module, 2, {...cpu(c), pc: suffix + 2}, {label: 'pure suffix has remaining budget for unavailable lookup', reason: 3});
    run(c, module, 1, cpu(c), {label: 'pure cold lookup retains live parents/FLAGS', reason: 3, retired: 0}); counts.controls += 5;
  }
  const mixed = compile(c, [{pc: livePc, bytes: live, memory: true}]), oldest = allRows[0]; seed(c, oldest.pc);
  if (owner === 'resident') operation(c, firstModule, oldest, undefined, 'older pure resident remains current after later captures');
  else run(c, firstModule, 1, cpu(c), {label: 'replacement generation mismatch3 is full-arena neutral', status: 3, retired: 0}); counts.retained_units++;
  const r = registers(); r[0] = 0xcafe80f9; r[1] = 0xbeef1234; r[2] = 0xface5678; r[6] = DATA + 0x80; seed(c, livePc, r);
  const memoryRow = {...memorySpec('sign', 1, [6], livePc), address: DATA + 0x80}; operation(c, mixed, memoryRow, 0xfd, 'live signed byte becomes current CX divisor', 5);
  operation(c, mixed, registerSpec('zero', 0, 4, livePc + 4), 0x80, 'live original AH becomes AX128 without modifying CX or FLAGS', 5);
  function step(length, nextFlags, changes, budget = 1, retired = 1) {
    const before = cpu(c), next = {...before, registers: [...before.registers], pc: before.pc + length, flags: nextFlags};
    for (const [index, value] of Object.entries(changes)) next.registers[index] = lowWord(before.registers[index], value);
    run(c, mixed, budget, next, {label: 'live returned factors/full product/quotient/FLAGS without CPU reseeding', retired, exitVersion: 5});
  }
  step(3, 0x402, {0: 0xfe80, 2: 0xffff}); step(3, 0x402, {0: 0x0080, 2: 0}); step(6, 0x402, {3: 0}, 2, 2);
  run(c, mixed, 2, {...cpu(c), pc: livePc + live.length}, {label: 'live V5 suffix reaches cold lookup with returned quotient', reason: 3, exitVersion: 5});
  run(c, mixed, 1, cpu(c), {label: 'live V5 cold result retains all returned state', reason: 3, retired: 0, exitVersion: 5}); counts.consumers += 7;
  checkRam(c, DATA, canary); checkRam(c, DATA + 0x80, Buffer.from([0xfd])); checkRam(c, livePc, live); assert.equal(c.api.close(), 0);
}

for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries), matrix = kinds.map((kind, k) => Array.from({length: 8}, (_, destination) => absolute(kind, destination, DATA + 8, PC + k * 0x100 + destination * 16)));
  const eaRows = kinds.map((kind, k) => {
    let offset = 0;
    const rows = Array.from({length: 8}, (_, destination) => {
      const tail = destination === 4 ? [4, 0x24] : destination === 5 ? [0x45, 0] : [destination];
      const row = {...memorySpec(kind, destination, tail, PC + 0x200 + k * 0x100 + offset), address: 0xa53c5008, parents: {[destination]: 0xa53c5008}}; offset += row.bytes.length; return row;
    });
    for (const a of [
      {destination: 1, tail: [0x44, 0x8d, 0xfc], parents: {1: 0x40000002, 5: 0xa53c5004}},
      {destination: 0, tail: [4, 0x8d, ...words([0xa53c5000])], parents: {1: 0x40000002}},
      {destination: 0, tail: [0x84, 0x80, ...words([0x053c5003])], parents: {0: 0x20000001}},
      {destination: 4, tail: [0x44, 0x24, 0xff], parents: {4: 0xa53c5009}},
    ]) {
      const row = {...memorySpec(kind, a.destination, a.tail, PC + 0x200 + k * 0x100 + offset), address: 0xa53c5008, parents: a.parents}; offset += row.bytes.length; rows.push(row);
    }
    for (const [destination, address] of [[0, DATA + 4095], [4, 0xffffffff]]) {
      const row = absolute(kind, destination, address, PC + 0x200 + k * 0x100 + offset); offset += row.bytes.length; rows.push(row);
    }
    return rows;
  });
  const mov = Buffer.from([0xbb, 0x44, 0x33, 0x22, 0x11]);
  const faultRows = kinds.map((kind, k) => memorySpec(kind, 0, [7], PC + 0x500 + k * 32 + 5));
  const faultBlocks = faultRows.map(row => block(row.pc - 5, Buffer.concat([mov, row.bytes])));
  const codeRows = kinds.map((kind, k) => absolute(kind, k * 2, PC + 0x600 + k * 32 + 2, PC + 0x600 + k * 32));
  const eaBlocks = eaRows.map(rows => block(rows[0].pc, Buffer.concat(rows.map(row => row.bytes))));
  const codeBlocks = codeRows.map(row => block(row.pc, row.bytes));
  for (const b of [...matrix.flat().map(row => block(row.pc, row.bytes)), ...eaBlocks, ...faultBlocks, ...codeBlocks]) upload(c, b.pc, b.bytes);
  host(c, 'protect', [PC, 1, 5]);
  for (const rows of matrix) {
    const module = compile(c, rows.map(row => block(row.pc, row.bytes)));
    for (const row of rows) for (const [i, value] of [0, 0x80].entries()) {
      upload(c, DATA + 7, Buffer.from([0x55, value, 0xaa])); host(c, 'protect', [DATA, 1, 1]);
      const r = registers(); r[row.destination] = (i ? 0x7fff : 0xcafe) * 65536 + 0xdead; seed(c, row.pc, r, anchorFlags[(row.destination + i) % 4]);
      operation(c, module, row, value, 'all memory destinations read zero/nonzero BYTE and retain current high16/FLAGS'); checkRam(c, DATA + 7, Buffer.from([0x55, value, 0xaa]));
      host(c, 'protect', [DATA, 1, 3]); counts.memory_destinations++;
    }
  }
  const eaModule = compile(c, eaBlocks);
  for (const rows of eaRows) for (const [i, row] of rows.entries()) {
    if (i < 12) {
      upload(c, row.address - 1, Buffer.from([0x55, 0xfd, 0xaa])); host(c, 'protect', [0xa53c5000, 1, 1]);
      const r = registers(); for (const [index, value] of Object.entries(row.parents)) r[index] = value;
      seed(c, row.pc, r, anchorFlags[i % 4]); operation(c, eaModule, row, 0xfd, 'old full destination/base/index/scale/wrap forms EA before publication');
      checkRam(c, row.address - 1, Buffer.from([0x55, 0xfd, 0xaa])); host(c, 'protect', [0xa53c5000, 1, 3]); counts.addresses++;
    } else for (const value of [0, 0xff]) {
      upload(c, row.address - 1, Buffer.from([0x55, value])); const page = row.address === 0xffffffff ? 0xfffff000 : DATA; host(c, 'protect', [page, 1, 1]);
      seed(c, row.pc); operation(c, eaModule, row, value, 'final byte of page and flat32 ffffffff are valid one-byte reads'); checkRam(c, row.address - 1, Buffer.from([0x55, value]));
      host(c, 'protect', [page, 1, 3]); counts.boundaries++;
    }
  }
  const faultModule = compile(c, faultBlocks);
  for (const row of faultRows) {
    const control = registers(); control[7] = 0x9008; seed(c, row.pc - 5, control);
    run(c, faultModule, 0, cpu(c), {label: 'zero budget avoids prefix and unmapped byte read', retired: 0});
    input(c, 96, words([1])); run(c, faultModule, 1, cpu(c), {label: 'cancel avoids prefix and byte read', reason: 2, retired: 0}); input(c, 96, words([0])); counts.controls += 2;
    for (const failure of [
      {name: 'unmapped', permission: null, value: 0}, {name: 'none', permission: 0, value: 0x80},
      {name: 'write-only zero', permission: 2, value: 0}, {name: 'execute-only', permission: 4, value: 0x80},
    ]) {
      if (failure.permission !== null) {host(c, 'map', [0x9000, 1, 3]); upload(c, 0x9007, Buffer.from([0x55, failure.value, 0xaa])); host(c, 'protect', [0x9000, 1, failure.permission]);}
      const r = registers(); r[0] = 0xcafe7777; r[7] = 0x9008;
      const before = seed(c, row.pc - 5, r), stopped = {...before, registers: [...before.registers], pc: row.pc}; stopped.registers[3] = 0x11223344;
      const fault = {detail: failure.permission === null ? 1 : 2, address: 0x9008, access: 1};
      run(c, faultModule, 2, stopped, {label: failure.name + ' after MOV retains destination/FLAGS/FP', reason: 5, retired: 1, fault, helperVersion: 2});
      run(c, faultModule, 1, stopped, {label: failure.name + ' unchanged retry', reason: 5, retired: 0, fault, helperVersion: 2}); counts.faults += 2;
      if (failure.permission === null) host(c, 'map', [0x9000, 1, 1]); else host(c, 'protect', [0x9000, 1, 1]);
      assert.deepEqual(cpu(c), stopped, 'map/protect-only repair retains the stopped CPU');
      operation(c, faultModule, row, failure.value, 'repair reads current zero-filled/original BYTE without CPU or RAM reseed');
      checkRam(c, 0x9007, failure.permission === null ? Buffer.from([0, 0, 0]) : Buffer.from([0x55, failure.value, 0xaa])); counts.retries++; host(c, 'unmap', [0x9000, 1]);
    }
    upload(c, DATA + 11, Buffer.from([0x55, 0, 0x66, 0, 0xaa])); host(c, 'protect', [DATA, 1, 2]);
    const r = registers(); r[0] = 0xcafe7777; r[7] = DATA + 12;
    const before = seed(c, row.pc - 5, r), stopped = {...before, registers: [...before.registers], pc: row.pc}; stopped.registers[3] = 0x11223344;
    const fault = {detail: 2, address: DATA + 12, access: 1};
    run(c, faultModule, 2, stopped, {label: 'current-input witness starts with zero-byte read fault', reason: 5, retired: 1, fault, helperVersion: 2});
    run(c, faultModule, 1, stopped, {label: 'current-input witness unchanged retry', reason: 5, retired: 0, fault, helperVersion: 2}); counts.faults += 2;
    host(c, 'protect', [DATA, 1, 3]); upload(c, DATA + 14, Buffer.from([0xfd])); host(c, 'protect', [DATA, 1, 1]);
    const current = {...stopped, registers: [...stopped.registers], flags: 0x892}; current.registers[0] = 0xbaffdead; current.registers[7] = DATA + 14;
    input(c, 0, state(current)); assert.deepEqual(cpu(c), current, 'explicit current byte/EA/destination-high/FLAGS witness is separate from repair-only cases');
    assert.equal(operation(c, faultModule, row, 0xfd, 'retry uses current byte/new EA/current high16 and FLAGS').registers[0], row.kind === 'zero' ? 0xbaff00fd : 0xbafffffd);
    checkRam(c, DATA + 11, Buffer.from([0x55, 0, 0x66, 0xfd, 0xaa])); host(c, 'protect', [DATA, 1, 3]); counts.retries++; counts.current_inputs++;
  }
  const codeModule = compile(c, codeBlocks);
  for (const row of codeRows) {
    seed(c, row.pc); operation(c, codeModule, row, opcode(row.kind), 'RX own opcode byte is readable data without stores/SMC');
    input(c, 48, words([row.pc])); operation(c, codeModule, row, opcode(row.kind), 'same owner remains current after own-code read');
    run(c, codeModule, 2, {...cpu(c), pc: row.pc + row.bytes.length + 2}, {label: 'same owner reaches JMP suffix after RX read with budget2', reason: 3});
    checkRam(c, row.pc, block(row.pc, row.bytes).bytes); counts.code_reads += 3;
  }
  assert.equal(c.api.close(), 0);
}
assert.deepEqual(sources(), sourcePins);
assert.deepEqual(counts, {contexts: 8, runs: 3104, retired: 3014, register_pairs: 512, byte_inputs: 2048, flag_inputs: 1024, anchors: 32, memory_destinations: 128, addresses: 96, boundaries: 32, faults: 80, retries: 40, current_inputs: 16, repeats: 16, controls: 56, retained_units: 4, code_reads: 24, consumers: 28});
assert.equal(modules.length, 40);
const result = {status: 'ok', engine_sha256: engine.sha256, source_pins: sourcePins, counts, modules,
  limits: [
    'finite exact single66 BYTE-to-WORD MOVZX/MOVSX register/memory in four actual bound profiles; native register six-profile coverage is separate',
    'arithmetic byte-digit/range/high-half oracle,128 typed pairs, all256 bytes per kind, eight literal anchors and all128 valid FLAGS per kind; flag-input count is a subset of byte-input rows',
    'original AL/CL/DL/BL/AH/CH/DH/BH capture and own destination.high16 preservation; full original EA captured before checked read8 and publication',
    'pure V1, read8 V2 and existing mixed-division V5; no stores/new helper family/local/ABI/emitter change; full4612/helper/FP preservation asserted',
    'four byte-read fault classes, map/protect-only repair and separate explicit current BYTE/EA/high16/FLAGS witnesses; finalffffffff is valid without invented WORD overflow',
    'maxfive resident units/context; four pure register units each contain36instructions within unchanged64 limit; fixed code/RX inputs before captures, retained resident success versus replacement generation mismatch3, live factor/product/quotient/FLAGS without reseeding',
    'read-only RAM/RX own-opcode canaries; no SMC/atomicity, exhaustiveISA, throughput, PE/browser/SDK/game or hostedCI claim',
  ]};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2), {flag: 'wx'});
console.log(JSON.stringify({status: 'ok', ...counts, modules: modules.length, output}));

