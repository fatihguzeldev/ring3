import {generatedImports, attachDetectionApi} from '../../../../../../../support/wasm/binding.mjs';
import {ARENA_SIZE} from '../../../../../../../support/wasm/arena.mjs';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../../../../../../../support/wasm/engine.mjs';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root);
const engine = readEngine(enginePath), SIZE = ARENA_SIZE, PC = 0x1000, DATA = 0x5000;
const initial = readFileSync(join(output, 'initial-arena.bin'));
assert.equal(initial.length, SIZE);
assert.deepEqual(WebAssembly.Module.imports(engine.module), []);
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const paths = [
  'engine/src/abi/arena.rs', 'engine/src/abi/memory_helper.rs',
  'engine/src/cpu/x86/ir.rs', 'engine/src/cpu/x86/decode/lower.rs', 'engine/src/cpu/x86/decode/families.rs',
  'engine/tests/cpu/moves/addressing/lea/native/word.rs', 'engine/tests/cpu/moves/native.rs', 'engine/tests/support/cpu/prefix/admission.rs', 'engine/tests/support/cpu/prefix/word.rs', 'engine/src/cpu/dbt/wasm/abi.rs',
  'engine/src/cpu/x86/decode/profile.rs', 'engine/src/cpu/x86/decode/families.rs', 'engine/src/cpu/dbt/region.rs',
  'engine/src/cpu/dbt/wasm/instructions/integer/arithmetic.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/bits.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/carry_rotates.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/decimal.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/mod.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/arithmetic.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/moves.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/exchange.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/predicates.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/shifts.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/control.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/extended.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/flags.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/mod.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/multiply_divide.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/operands.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/rotates.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/shifts.rs', 'engine/src/cpu/dbt/wasm/imports/mod.rs', 'engine/src/cpu/dbt/wasm/imports/binding.rs', 'engine/src/cpu/dbt/wasm/imports/context.rs', 'engine/src/cpu/dbt/wasm/imports/needs.rs', 'engine/src/cpu/dbt/wasm/helpers/protocol.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/mod.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/address.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/scalar.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/stack.rs',
  'engine/src/cpu/dbt/wasm/instructions/control.rs',
  'engine/src/cpu/dbt/wasm/emitter/mod.rs', 'engine/src/cpu/dbt/wasm/emitter/module.rs', 'engine/src/cpu/dbt/wasm/emitter/body.rs', 'engine/src/cpu/dbt/wasm/requirements.rs', 'engine/src/cpu/dbt/wasm/imports/wiring.rs', 'engine/src/process/wasm/mod.rs', 'engine/src/process/wasm/loading.rs', 'engine/src/abi/image.rs', 'engine/src/process/wasm/execution.rs', 'engine/src/abi/execution.rs', 'engine/src/process/wasm/calls.rs', 'engine/src/process/wasm/memory.rs', 'engine/src/process/wasm/cpu.rs', 'engine/src/process/mod.rs',
  'engine/src/process/instance.rs', 'engine/src/process/execution/compiled.rs', 'engine/src/process/execution/authority.rs', 'engine/src/process/memory/address_space.rs', 'engine/src/process/memory/scalar.rs', 'engine/src/process/memory/scalar/tests.rs', 'engine/src/process/execution/mod.rs', 'engine/src/process/memory/mod.rs', 'engine/src/process/execution/resident.rs',
  'engine/tests/cpu/moves/addressing/lea/wasm/word.rs', 'engine/tests/cpu/moves/wasm.rs', 'engine/tests/support/wasm.rs',
  'engine/tests/cpu/moves/addressing/lea/fixtures/p2-word-lea/runtime/run.mjs', 'engine/tests/support/wasm/engine.mjs', 'engine/tests/support/wasm/binding.mjs'];
assert.equal(new Set(paths).size, paths.length);
const sources = () => Object.fromEntries(paths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = sources(), modules = [];
const counts = {contexts: 0, runs: 0, retired: 0, address_rows: 0, flag_inputs: 0, anchors: 0, repeats: 0, current_inputs: 0, controls: 0, retained_units: 0, consumers: 0};
const words = values => {const b = Buffer.alloc(values.length * 4); values.forEach((v, i) => b.writeUInt32LE(v >>> 0, i * 4)); return b;};
function record(magic, size, fields, version = 1) {
  const b = Buffer.alloc(size); b.write(magic); b.writeUInt16LE(version, 4);
  b.writeUInt16LE(1, 6); b.writeUInt32LE(size, 8);
  fields.forEach((v, i) => b.writeUInt32LE(v >>> 0, 16 + i * 4)); return b;
}
const state = c => record('R3ST', 56, [...c.registers, c.pc, c.flags]);
const exit = (reason, n, version = 1) => record('R3EX', 40, [reason, n, 0, 0, 0, 0], version);
const arena = c => Buffer.from(new Uint8Array(c.memory.buffer, c.base, SIZE));
function input(c, offset, bytes) {
  const expected = arena(c); expected.set(bytes, offset);
  new Uint8Array(c.memory.buffer).set(bytes, c.base + offset);
  assert.deepEqual(arena(c), expected, 'declared input preserves the remaining full arena');
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
function run(c, module, budget, next, {label, reason = 1, retired = 1, status = 0, exitVersion = 1} = {}) {
  const expected = arena(c);
  if (status === 0) {expected.set(state(next)); expected.set(exit(reason, retired, exitVersion), 56);}
  assert.equal(module.run(c.base, c.base + 56, budget, c.base + 96), status, label);
  assert.deepEqual(arena(c), expected, 'complete arena CPU/exit/unchanged helper/FP128 effect: ' + label);
  counts.runs++; if (status === 0) counts.retired += retired;
}
function checkRam(c, address, expected) {
  for (let i = 0; i < expected.length; i++) {
    const wanted = arena(c); wanted.set(record('R3MH', 40, [0, expected[i], 0, 0, 0, 1], 2), 100);
    assert.equal(c.api.read8(address + i), 0);
    assert.deepEqual(arena(c), wanted, 'real unchanged code/data byte at ' + (address + i).toString(16));
  }
}
function compile(c, blocks) {
  const instructions = blocks.reduce((sum, b) => sum + b.instructions, 0);
  assert.ok(Number.isInteger(instructions) && instructions <= 64, 'authored aggregate unit fits unchanged64-instruction limit');
  assert.ok(++c.captures <= 3, 'at most three captures/resident units in this context');
  input(c, 140, words(blocks.flatMap(b => c.entries ? [b.pc] : [b.pc, b.bytes.length])));
  const method = c.owner === 'resident' ? (c.entries ? 'compile_resident_entries' : 'compile_resident') : (c.entries ? 'compile_entries' : 'compile');
  host(c, method, c.entries ? [blocks.length, 0] : [blocks.length]);
  const b = arena(c), binding = c.owner === 'resident'
    ? {low: b.readUInt32LE(148), high: b.readUInt32LE(152), pointer: b.readUInt32LE(156), length: b.readUInt32LE(160)}
    : {generation: c.api.generation(), pointer: c.api.module_ptr() >>> 0, length: c.api.module_len() >>> 0};
  const bytes = Buffer.from(new Uint8Array(c.memory.buffer, binding.pointer, binding.length));
  const module = new WebAssembly.Module(bytes), names = generatedImports({resident: c.owner === 'resident', helpers: []}).slice(1).map(item => item.name);
  assert.deepEqual(WebAssembly.Module.imports(module), [{module: 'env', name: 'memory', kind: 'memory'}, ...names.map(name => ({module: 'ring3', name, kind: 'function'}))]);
  assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  const file = `module-${modules.length + 1}.wasm`; writeFileSync(join(output, file), bytes, {flag: 'wx'});
  modules.push({context: c.id, owner: c.owner, entries: c.entries, file, sha256: hash(bytes), imports: names, instructions});
  return {run: new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3: c.api}).exports.run};
}
const block = (pc, bytes, instructions) => ({pc, instructions: instructions + 1, bytes: Buffer.concat([bytes, Buffer.from([0xeb, 0])])});
const canary = Buffer.from([0x55, 0xa5, 0x66, 0x67, 0xf0, 0xf2, 0xf3, 0xaa]);
function open(owner, entries) {
  const instance = new WebAssembly.Instance(engine.module, {});
  const c = {id: ++counts.contexts, owner, entries, memory: instance.exports.memory, api: {}, captures: 0};
  for (const name of ['open', 'close', 'arena_ptr', 'map', 'protect', 'upload', 'read8',
    'compile', 'compile_entries', 'compile_resident', 'compile_resident_entries', 'generation', 'module_ptr', 'module_len', 'guard', 'guard_resident'])
    c.api[name] = instance.exports['ring3_abi_v1_' + name];
 attachDetectionApi(c.api, instance.exports);
  assert.equal(c.api.open(4, c.id, 0x574c4541), 0); c.base = c.api.arena_ptr() >>> 0; input(c, 0, initial);
  for (const args of [[PC, 1, 7], [DATA, 1, 3], [0x6000, 1, 0]]) host(c, 'map', args);
  upload(c, DATA, canary); host(c, 'protect', [DATA, 1, 1]); return c;
}

// exact integer addition and modulo independently model original full parents and the returned low WORD.
const lowWord = (parent, value) => Math.floor(parent / 65536) * 65536 + value;
function address(r, row) {
  const full = (row.base === null ? 0n : BigInt(r[row.base]))
    + (row.index === null ? 0n : BigInt(r[row.index]) * BigInt(row.scale)) + BigInt(row.displacement);
  return Number((full % 4294967296n + 4294967296n) % 4294967296n);
}
function computed(before, row) {
  const next = {...before, registers: [...before.registers], pc: before.pc + row.bytes.length};
  next.registers[row.destination] = lowWord(before.registers[row.destination], address(before.registers, row) % 65536);
  return next;
}
const flags = Array.from({length: 128}, (_, n) => 2 + [1, 4, 16, 64, 128, 1024, 2048].reduce((sum, bit, i) => sum + Math.floor(n / 2 ** i) % 2 * bit, 0));
assert.equal(new Set(flags).size, 128); assert.ok(flags.every(value => (value & 2) && !(value & ~0xcd7)));
const anchorFlags = [2, 0xcd7, 0x443, 0x892];
function forms(destination) {
  const ownBase = destination === 4 ? [4, 0x24] : destination === 5 ? [0x45, 0] : [destination];
  const ownIndex = destination === 4 ? 6 : destination, bothIndex = destination === 4 ? 0 : destination;
  const form = (tail, base, index = null, scale = 1, displacement = 0) => ({tail, base, index, scale, displacement});
  return [
    form(ownBase, destination),
    form([0x44, 0x24, 0x80], 4, null, 1, -128),
    form([0x45, 0x7f], 5, null, 1, 127),
    form([0x83, ...words([0xf3f06766])], 3, null, 1, 0xf3f06766),
    ...[1, 2, 4, 8].map((scale, i) => form([4, i * 64 + 0x0b], 3, 1, scale)),
    form([4, 0x85 | ownIndex << 3, ...words([0x92345678])], null, ownIndex, 4, 0x92345678),
    form([0x84, 0x80 | bothIndex << 3 | destination, ...words([0x053c5003])], destination, bothIndex, 4, 0x053c5003),
    ...[0x9008, 0x6008, 0xffffffff].map(value => form([5, ...words([value])], null, null, 1, value)),
  ];
}
function rowsFor(destination) {
  let pc = PC + destination * 0x80;
  return forms(destination).map((form, formIndex) => {
    const row = {...form, formIndex, destination, pc, bytes: Buffer.from([0x66, 0x8d, form.tail[0] | destination << 3, ...form.tail.slice(1)])};
    pc += row.bytes.length; return row;
  });
}
function parents(variant) {
  return variant === 0
    ? [0xffffffff, 0x40000002, 0x80018001, 0xfffffffe, 0x80000040, 0xfffffff0, 0xcafe6789, 0x7fff8000]
    : [0x20000001, 0xc000ffff, 0x7fff7ffe, 0x92345678, 0xffff8000, 0x80000080, 0x1357ffff, 0xfedc0000];
}
const literalAnchors = [
  {destination: 0, formIndex: 0, inputs: {0: 0xa53c5008}, full: 0xa53c5008, low: 0x5008},
  {destination: 2, formIndex: 1, inputs: {4: 0x40}, full: 0xffffffc0, low: 0xffc0},
  {destination: 3, formIndex: 2, inputs: {5: 0xfffffff0}, full: 0x6f, low: 0x6f},
  {destination: 7, formIndex: 3, inputs: {3: 0x0c0f989a}, full: 0, low: 0},
  {destination: 4, formIndex: 6, inputs: {3: 0xfffffffe, 1: 0x40000002}, full: 6, low: 6},
  {destination: 5, formIndex: 7, inputs: {3: 0x10, 1: 0x20000003}, full: 0x28, low: 0x28},
  {destination: 0, formIndex: 9, inputs: {0: 0x20000001}, full: 0xa53c5008, low: 0x5008},
  {destination: 6, formIndex: 12, inputs: {}, full: 0xffffffff, low: 0xffff},
];
for (const a of literalAnchors) {
  const r = registers(); for (const [index, value] of Object.entries(a.inputs)) r[index] = value;
  const row = rowsFor(a.destination)[a.formIndex]; assert.equal(address(r, row), a.full);
  const next = computed({registers: r, pc: row.pc, flags: 0xcd7}, row);
  assert.equal(next.registers[a.destination], lowWord(r[a.destination], a.low));
}
function operation(c, module, row, label) {
  const next = computed(cpu(c), row); run(c, module, 1, next, {label}); return next;
}
function repeatAndEdit(c, module, row) {
  seed(c, row.pc, parents(1)); operation(c, module, row, 'first original destination/base/index capture');
  input(c, 48, words([row.pc])); operation(c, module, row, 'only PC rewound; returned parent now supplies the current EA'); counts.repeats += 2;
  const current = cpu(c); current.pc = row.pc; current.flags = 0x892;
  current.registers[row.destination] = 0xbaff8001;
  if (row.index !== null && row.index !== row.destination) current.registers[row.index] = 0xc000ffff;
  input(c, 0, state(current)); operation(c, module, row, 'explicit current full parents/own-high/FLAGS inputs'); counts.current_inputs++;
}
function controls(c, module, row, suffix) {
  seed(c, row.pc, parents(0)); run(c, module, 0, cpu(c), {label: 'zero budget is neutral before address arithmetic', retired: 0});
  input(c, 96, words([1])); run(c, module, 1, cpu(c), {label: 'cancel is neutral before LEA', reason: 2, retired: 0});
  run(c, module, 0, cpu(c), {label: 'cancel precedes exhausted budget', reason: 2, retired: 0}); input(c, 96, words([0]));
  input(c, 48, words([suffix])); run(c, module, 2, {...cpu(c), pc: suffix + 2}, {label: 'suffix JMP leaves budget for unavailable lookup', reason: 3});
  run(c, module, 1, cpu(c), {label: 'cold lookup retains all returned parents/FLAGS', reason: 3, retired: 0}); counts.controls += 5;
}
const live = Buffer.from([
  0x66, 0x0f, 0xba, 0xfb, 0, // btc bx,0 publishes original bit1 as CF.
  0x66, 0x8d, 0x14, 0x88, // lea dx,[eax+ecx*4] returns6 while retaining CF.
  0x66, 0x11, 0xd0, // adc ax,dx consumes returned6 and live carry to produce5.
  0x66, 0x0f, 0xbc, 0xc8, // bsf cx,ax consumes returned bitmap5 and returns index0/parity1.
  0x0f, 0x92, 0xc2, 0x0f, 0x9a, 0xc6, 0xeb, 0, // setb dl; setp dh; suffix JMP.
]);
for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries), banks = Array.from({length: 8}, (_, destination) => rowsFor(destination));
  const blocks = banks.map(rows => block(rows[0].pc, Buffer.concat(rows.map(row => row.bytes)), rows.length));
  const livePc = PC + 0x800, liveBlock = {pc: livePc, bytes: live, instructions: 7};
  // all uploads and RX code permissions precede captures so older resident units stay current.
  for (const b of [...blocks, liveBlock]) upload(c, b.pc, b.bytes); host(c, 'protect', [PC, 1, 5]); let firstModule;
  for (const group of [0, 1]) {
    const selected = blocks.slice(group * 4, group * 4 + 4); assert.equal(selected.reduce((sum, b) => sum + b.instructions, 0), 56);
    const module = compile(c, selected); if (group === 0) firstModule = module;
    for (const rows of banks.slice(group * 4, group * 4 + 4)) for (const row of rows) for (const variant of [0, 1]) {
      seed(c, row.pc, parents(variant), anchorFlags[(row.destination + row.formIndex + variant) % 4]);
      operation(c, module, row, 'all8 destinations/13 EAs: old full parents, signed displacement, scale/wrap, and inaccessible computed addresses'); counts.address_rows++;
    }
    for (const [i, a] of literalAnchors.entries()) if (Math.floor(a.destination / 4) === group) {
      const row = banks[a.destination][a.formIndex], r = registers(); for (const [index, value] of Object.entries(a.inputs)) r[index] = value;
      seed(c, row.pc, r, anchorFlags[i % 4]); assert.equal(operation(c, module, row, 'literal full32 arithmetic/lowWORD/own-high anchor').registers[a.destination], lowWord(r[a.destination], a.low)); counts.anchors++;
    }
    const selectedRow = banks[group * 4][9];
    if (group === 0) for (const value of flags) {
      seed(c, selectedRow.pc, parents(1), value); operation(c, module, selectedRow, 'all128 valid FLAGS on original own-base/index alias'); counts.flag_inputs++;
    }
    repeatAndEdit(c, module, selectedRow); controls(c, module, selectedRow, blocks[group * 4].pc + blocks[group * 4].bytes.length - 2);
  }
  const mixed = compile(c, [liveBlock]), oldest = banks[0][0]; seed(c, oldest.pc, parents(0));
  if (owner === 'resident') operation(c, firstModule, oldest, 'older resident arithmetic remains current after later captures');
  else run(c, firstModule, 1, cpu(c), {label: 'replacement generation mismatch3 is neutral', status: 3, retired: 0}); counts.retained_units++;
  const r = registers(); r[0] = 0xcafefffe; r[1] = 0x40000002; r[3] = 0xbeef0001; seed(c, livePc, r);
  function step(length, nextFlags, changes = {}, budget = 1, retired = 1) {
    const before = cpu(c), next = {...before, registers: [...before.registers], pc: before.pc + length, flags: nextFlags};
    for (const [index, value] of Object.entries(changes)) next.registers[index] = lowWord(before.registers[index], value);
    run(c, mixed, budget, next, {label: 'live returned address/carry/bitmap/index/FLAGS consumer without CPU reseeding', retired});
  }
  step(5, 0x443, {3: 0}); step(4, 0x443, {2: 6}); step(3, 0x417, {0: 5}); step(4, 0x406, {1: 0}); step(6, 0x406, {2: 0x0100}, 2, 2);
  run(c, mixed, 2, {...cpu(c), pc: livePc + live.length}, {label: 'live suffix reaches unavailable lookup with returned state', reason: 3});
  run(c, mixed, 1, cpu(c), {label: 'live cold result retains all returned CPU/FLAGS', reason: 3, retired: 0}); counts.consumers += 7;
  checkRam(c, DATA, canary); checkRam(c, blocks[0].pc, blocks[0].bytes); checkRam(c, blocks[7].pc, blocks[7].bytes); checkRam(c, livePc, live);
  assert.equal(c.api.close(), 0);
}
assert.deepEqual(sources(), sourcePins);
assert.deepEqual(counts, {contexts: 4, runs: 1472, retired: 1438, address_rows: 832, flag_inputs: 512, anchors: 32, repeats: 16, current_inputs: 8, controls: 40, retained_units: 4, consumers: 28});
assert.equal(modules.length, 12);
const result = {status: 'ok', engine_sha256: engine.sha256, source_pins: sourcePins, counts, modules,
  limits: [
    'finite exact single66 WORD LEA in four actual bound profiles; native pure six-profile coverage is separate',
    'independent BigInt original-parent addition and modulo oracle,104 destination/EA identities with two dirty-parent variants, eight literal full32/low16 anchors',
    'base/index/both destination aliases, signed displacements/scales/wrap, all128 valid FLAGS on a distinguishing own-base/index alias',
    'unmapped9008, denied6008 and ffffffff are arithmetic outputs with no data read: guard-only pure V1 imports and entire helper/FP128/remaining4612 preserved',
    'two56-instruction pure units plus seven-instruction live unit; maxthree captures/resident units/context, unchanged64-instruction limit asserted before compilation',
    'PC-only repeated parent use, separate explicit current-parent/own-high/FLAGS inputs, controls and retained resident success versus replacement generation mismatch3',
    'returned address/carry/bitmap/index consumers without CPU reseeding; RO/RX code/data canaries, no stores/SMC/atomicity/exhaustiveISA/speed/PE/browser/SDK/game/hostedCI claim',
  ]};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2), {flag: 'wx'});
console.log(JSON.stringify({status: 'ok', ...counts, modules: modules.length, output}));

