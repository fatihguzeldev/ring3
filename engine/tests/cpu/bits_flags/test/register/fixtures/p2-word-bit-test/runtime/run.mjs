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
  'engine/tests/cpu/bits_flags/test/register/native/word.rs', 'engine/tests/cpu/bits_flags/native.rs', 'engine/tests/support/cpu/prefix/admission.rs', 'engine/tests/support/cpu/prefix/word.rs', 'engine/src/cpu/x86/decode/integer.rs',
  'engine/src/cpu/x86/decode/profile.rs', 'engine/src/cpu/x86/decode/families.rs', 'engine/src/cpu/dbt/region.rs',
  'engine/src/cpu/dbt/wasm/instructions/integer/arithmetic.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/bits.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/carry_rotates.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/decimal.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/mod.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/arithmetic.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/moves.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/exchange.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/predicates.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/shifts.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/control.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/extended.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/flags.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/mod.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/multiply_divide.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/operands.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/rotates.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/shifts.rs', 'engine/src/cpu/dbt/wasm/instructions/control.rs',
  'engine/src/cpu/dbt/wasm/emitter/mod.rs', 'engine/src/cpu/dbt/wasm/emitter/module.rs', 'engine/src/cpu/dbt/wasm/emitter/body.rs', 'engine/src/cpu/dbt/wasm/requirements.rs', 'engine/src/cpu/dbt/wasm/imports/wiring.rs', 'engine/src/memory/space.rs', 'engine/src/memory/space/atomic.rs',
  'engine/src/process/instance.rs', 'engine/src/process/execution/compiled.rs', 'engine/src/process/execution/authority.rs', 'engine/src/process/memory/address_space.rs', 'engine/src/process/memory/scalar.rs', 'engine/src/process/memory/scalar/tests.rs', 'engine/src/process/execution/mod.rs', 'engine/src/process/memory/mod.rs', 'engine/src/process/mod.rs', 'engine/src/process/execution/resident.rs', 'engine/tests/cpu/bits_flags/test/register/wasm/word.rs', 'engine/tests/cpu/bits_flags/wasm.rs', 'engine/tests/support/wasm.rs', 'engine/tests/cpu/bits_flags/test/register/fixtures/p2-word-bit-test/runtime/run.mjs', 'engine/tests/support/wasm/engine.mjs', 'engine/tests/support/wasm/binding.mjs',
];
const sources = () => Object.fromEntries(paths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = sources(), modules = [];
const counts = {contexts: 0, runs: 0, retired: 0, matrix: 0, anchors: 0, repeats: 0, current_inputs: 0, controls: 0, retained_units: 0, consumers: 0};
const words = values => {const b = Buffer.alloc(values.length * 4); values.forEach((v, i) => b.writeUInt32LE(v >>> 0, i * 4)); return b;};
function record(magic, size, fields, version = 1) {
  const b = Buffer.alloc(size); b.write(magic); b.writeUInt16LE(version, 4);
  b.writeUInt16LE(1, 6); b.writeUInt32LE(size, 8);
  fields.forEach((v, i) => b.writeUInt32LE(v >>> 0, 16 + i * 4)); return b;
}
const state = c => record('R3ST', 56, [...c.registers, c.pc, c.flags]);
const exit = (reason, n) => record('R3EX', 40, [reason, n, 0, 0, 0, 0]);
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
function run(c, module, budget, next, {label, reason = 1, retired = 1, status = 0} = {}) {
  const expected = arena(c);
  if (status === 0) {expected.set(state(next)); expected.set(exit(reason, retired), 56);}
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
  modules.push({context: c.id, owner: c.owner, entries: c.entries, file, sha256: hash(bytes), imports: names});
  return {run: new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3: c.api}).exports.run};
}
const block = (pc, bytes) => ({pc, bytes: Buffer.concat([bytes, Buffer.from([0xeb, 0])])});
const canary = Buffer.from([0x55, 0xa5, 0x66, 0x67, 0xf0, 0xf2, 0xf3, 0xaa]);
function open(owner, entries) {
  const instance = new WebAssembly.Instance(engine.module, {});
  const c = {id: ++counts.contexts, owner, entries, memory: instance.exports.memory, api: {}};
  for (const name of ['open', 'close', 'arena_ptr', 'map', 'protect', 'upload', 'read8',
    'compile', 'compile_entries', 'compile_resident', 'compile_resident_entries', 'generation', 'module_ptr', 'module_len', 'guard', 'guard_resident'])
    c.api[name] = instance.exports['ring3_abi_v1_' + name];
 attachDetectionApi(c.api, instance.exports);
  assert.equal(c.api.open(4, c.id, 0x57425431), 0); c.base = c.api.arena_ptr() >>> 0; input(c, 0, initial);
  for (const args of [[PC, 1, 7], [DATA, 1, 3]]) host(c, 'map', args);
  upload(c, DATA, canary); host(c, 'protect', [DATA, 1, 1]); return c;
}

// arithmetic digit extraction/mutation is independent of Wasm shifts and bitwise updates.
function changed(kind, value, raw, flags) {
  const bit = raw % 16, mask = 2 ** bit, oldBit = Math.floor(value / mask) % 2;
  const delta = kind === 'bts' ? 1 - oldBit : kind === 'btr' ? -oldBit : kind === 'btc' ? 1 - 2 * oldBit : 0;
  return {value: value + delta * mask, flags: (flags & 0x442) | oldBit};
}
const kinds = [
  {name: 'bt', opcode: 0xa3, extension: 4}, {name: 'bts', opcode: 0xab, extension: 5},
  {name: 'btr', opcode: 0xb3, extension: 6}, {name: 'btc', opcode: 0xbb, extension: 7},
];
const anchorInputs = [
  {value: 0, raw: 15, flags: 0xcd7, nextFlags: 0x442},
  {value: 0x8000, raw: 31, flags: 2, nextFlags: 3},
  {value: 0xffff, raw: 32, flags: 0x42, nextFlags: 0x43},
  {value: 15, raw: 15, flags: 0x803, nextFlags: 2, same: true},
];
const anchorValues = {bt: [0, 0x8000, 0xffff, 15], bts: [0x8000, 0x8000, 0xffff, 0x800f], btr: [0, 0, 0xfffe, 15], btc: [0x8000, 0, 0xfffe, 0x800f]};
for (const kind of kinds) for (const [i, a] of anchorInputs.entries())
  assert.deepEqual(changed(kind.name, a.value, a.raw, a.flags), {value: anchorValues[kind.name][i], flags: a.nextFlags}, 'literal old-CF/modulo16/identity/same-parent anchor');
const lowWord = (parent, value) => Math.floor(parent / 65536) * 65536 + value;
const registerInstruction = (kind, destination, index) => Buffer.from([0x66, 0x0f, kind.opcode, 0xc0 | index << 3 | destination]);
const immediateInstruction = (kind, destination, raw) => Buffer.from([0x66, 0x0f, 0xba, 0xc0 | kind.extension << 3 | destination, raw]);
function spec(kind, destination, index, pc, immediate = false) {
  return {kind, destination, index, pc, immediate, bytes: immediate ? immediateInstruction(kind, destination, index) : registerInstruction(kind, destination, index)};
}
function operation(c, module, row, label) {
  const before = cpu(c), raw = row.immediate ? row.index : before.registers[row.index];
  const result = changed(row.kind.name, before.registers[row.destination] % 65536, raw, before.flags);
  const next = {...before, registers: [...before.registers], pc: before.pc + row.bytes.length, flags: result.flags};
  if (row.kind.name !== 'bt') next.registers[row.destination] = lowWord(before.registers[row.destination], result.value);
  run(c, module, 1, next, {label}); return result;
}
function flagsFor(value, raw, index) {
  const oldBit = Math.floor(value / (2 ** (raw % 16))) % 2;
  return [0xcd6, 2, 0x42, 0x402][index % 4] | (1 - oldBit);
}
function repeatAndEdit(c, module, row) {
  const r = registers(); r[row.destination] = lowWord(r[row.destination], 15);
  if (!row.immediate && row.index !== row.destination) r[row.index] = 0xbeef000f;
  seed(c, row.pc, r); operation(c, module, row, 'first pure-register operation');
  input(c, 0, state({...cpu(c), pc: row.pc})); operation(c, module, row, 'same live parent/index with only EIP rewound'); counts.repeats += 2;
  const current = cpu(c); current.pc = row.pc; current.registers[row.destination] = 0xc0de8000;
  if (!row.immediate && row.index !== row.destination) current.registers[row.index] = 0xface0010;
  input(c, 0, state(current)); operation(c, module, row, 'explicit current high/low parent and original current index'); counts.current_inputs++;
}

for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) for (const kind of kinds) {
  const c = open(owner, entries);
  const registerRows = Array.from({length: 16}, (_, i) => spec(kind, Math.floor(i / 2), i % 2 ? (Math.floor(i / 2) + 1) % 8 : Math.floor(i / 2), PC + Math.floor(i / 8) * 0x100 + i % 8 * 16));
  const immediateRows = [0, 15, 16, 31, 32, 255, 0x66, 0xf3].map((raw, destination) => spec(kind, destination, raw, PC + 0x200 + destination * 16, true));
  const payloadRows = [0x66, 0x67, 0xf0, 0xf2, 0xf3].map((raw, i) => spec(kind, i + 1, raw, PC + 0x300 + i * 16, true));
  const all = [...registerRows, ...immediateRows, ...payloadRows];
  // every code upload and permission change precedes every capture, preserving older resident snapshots.
  for (const row of all) upload(c, row.pc, block(row.pc, row.bytes).bytes);
  host(c, 'protect', [PC, 1, 5]); checkRam(c, DATA, canary);
  let firstModule;
  for (let start = 0; start < 16; start += 8) {
    const rows = registerRows.slice(start, start + 8), module = compile(c, rows.map(row => block(row.pc, row.bytes)));
    if (start === 0) firstModule = module;
    for (const [i, row] of rows.entries()) for (const [variant, value] of [0, 0xffff].entries()) {
      const r = registers(); r[row.destination] = lowWord(r[row.destination], value);
      if (row.index !== row.destination) r[row.index] = 0xbeef0000 + (start + i) * 7 % 16;
      const raw = r[row.index]; seed(c, row.pc, r, flagsFor(value, raw, i + variant));
      operation(c, module, row, 'all eight destination/index parents including original same-parent index'); counts.matrix++;
    }
    if (start === 8) {
      const row = rows[1];
      for (let bit = 0; bit < 16; bit++) for (const [variant, value] of [0, 2 ** bit].entries()) {
        const r = registers(); r[row.destination] = lowWord(r[row.destination], value); r[row.index] = 0xface0000 + bit;
        seed(c, row.pc, r, flagsFor(value, bit, bit + variant)); operation(c, module, row, 'every selected WORD bit with incoming CF opposite old bit'); counts.matrix++;
      }
      for (const raw of [16, 31, 32, 255, 0x10010, 0xffff]) for (const [variant, value] of [0xa55a, 0x5aa5].entries()) {
        const r = registers(); r[row.destination] = lowWord(r[row.destination], value); r[row.index] = 0x98760000 + raw;
        seed(c, row.pc, r, flagsFor(value, raw, variant)); operation(c, module, row, 'raw register index modulo16 ignores dirty parent high16'); counts.matrix++;
      }
      for (const [i, a] of anchorInputs.entries()) {
        const literalRow = a.same ? rows[0] : row, r = registers(); r[literalRow.destination] = lowWord(r[literalRow.destination], a.value);
        if (!a.same) r[literalRow.index] = 0xbeef0000 + a.raw;
        seed(c, literalRow.pc, r, a.flags);
        assert.deepEqual(operation(c, module, literalRow, 'literal flags/value and original same-parent BTC15 witness'), {value: anchorValues[kind.name][i], flags: a.nextFlags}); counts.anchors++;
      }
      repeatAndEdit(c, module, row);
    }
  }
  const immediateModule = compile(c, immediateRows.map(row => block(row.pc, row.bytes)));
  for (const row of immediateRows) for (const [variant, value] of [0, 0xffff].entries()) {
    const r = registers(); r[row.destination] = lowWord(r[row.destination], value);
    seed(c, row.pc, r, flagsFor(value, row.index, row.destination + variant));
    operation(c, immediateModule, row, 'all eight immediate destinations/low4 modulo/identity set-reset'); counts.matrix++;
  }
  repeatAndEdit(c, immediateModule, immediateRows[1]);
  const payloadModule = compile(c, payloadRows.map(row => block(row.pc, row.bytes)));
  for (const row of payloadRows) for (const [variant, value] of [0x8001, 0x7ffe].entries()) {
    const r = registers(); r[row.destination] = lowWord(r[row.destination], value);
    seed(c, row.pc, r, flagsFor(value, row.index, row.destination + variant));
    operation(c, payloadModule, row, 'prefix-looking raw byte remains immediate payload'); counts.matrix++;
  }
  const currentRow = payloadRows[0]; seed(c, currentRow.pc);
  run(c, payloadModule, 0, cpu(c), {label: 'zero budget preserves all parents/FLAGS/helper/FP', retired: 0});
  input(c, 96, words([1])); run(c, payloadModule, 1, cpu(c), {label: 'cancel before pure target', reason: 2, retired: 0}); input(c, 96, words([0]));
  input(c, 0, state({...cpu(c), pc: currentRow.pc + currentRow.bytes.length}));
  run(c, payloadModule, 2, {...cpu(c), pc: currentRow.pc + currentRow.bytes.length + 2}, {label: 'retained suffix JMP reaches unavailable lookup with remaining budget', reason: 3});
  run(c, payloadModule, 1, cpu(c), {label: 'cold lookup is CPU/FLAGS neutral', reason: 3, retired: 0}); counts.controls += 4;
  const oldest = registerRows[0], r = registers(); r[oldest.destination] = lowWord(r[oldest.destination], 15); seed(c, oldest.pc, r);
  if (owner === 'resident') operation(c, firstModule, oldest, 'older resident unit stays live across later captures with unchanged code');
  else run(c, firstModule, 1, cpu(c), {label: 'replaced unit has generation mismatch3 and is full-arena neutral', status: 3, retired: 0});
  counts.retained_units++;
  checkRam(c, DATA, canary); checkRam(c, registerRows[0].pc, block(registerRows[0].pc, registerRows[0].bytes).bytes);
  checkRam(c, payloadRows[4].pc, block(payloadRows[4].pc, payloadRows[4].bytes).bytes);
  assert.equal(c.api.close(), 0);
}

for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries);
  const code = Buffer.from([
    0x66, 0x0f, 0xba, 0xf8, 15, // btc ax,15: old bit1, zero result with old ZF1 retained.
    0x0f, 0x92, 0xc2, 0x0f, 0x94, 0xc3, // setb dl; setz bl.
    0x66, 0x83, 0xd7, 0, // adc di,0 consumes live carry into ffff.
    0x66, 0x0f, 0xbc, 0xc8, // bsf cx,ax: zero preserves CX=9, sets ZF.
    0x66, 0x0f, 0xab, 0xc8, // bts ax,cx creates bit9 and retains ZF1.
    0x0f, 0x92, 0xc6, 0x0f, 0x94, 0xc7, // setb dh; setz bh.
    0x66, 0x0f, 0xbc, 0xc8, // bsf cx,ax returns bit9, clears ZF.
    0x66, 0x0f, 0xa3, 0xc8, // bt ax,cx publishes CF1 with ZF0 retained.
    0x66, 0x0f, 0xb3, 0xc8, // btr ax,cx clears bitmap while preserving ZF0.
    0x0f, 0x92, 0xc6, 0x0f, 0x94, 0xc7, // setb dh; setz bh consume retained flags.
    0x66, 0x83, 0xd7, 0, 0xeb, 0, // adc di,0 consumes carry again, then unavailable JMP.
  ]);
  upload(c, PC, code); host(c, 'protect', [PC, 1, 5]); const module = compile(c, [{pc: PC, bytes: code}]);
  const r = registers(); r[0] = lowWord(r[0], 0x8000); r[1] = lowWord(r[1], 9); r[7] = lowWord(r[7], 0xffff); seed(c, PC, r);
  function step(length, flags, changes = {}, budget = 1, retired = 1) {
    const before = cpu(c), next = {...before, registers: [...before.registers], pc: before.pc + length, flags};
    for (const [index, value] of Object.entries(changes)) next.registers[index] = lowWord(before.registers[index], value);
    run(c, module, budget, next, {label: 'live bitmap/returned index/CF/ZF consumer with no CPU reseed', retired});
  }
  step(5, 0x443, {0: 0}); step(6, 0x443, {2: 0x1201, 3: 0x1201}, 2, 2);
  step(4, 0x457, {7: 0}); step(4, 0x446); step(4, 0x442, {0: 0x0200});
  step(6, 0x442, {2: 1, 3: 0x0101}, 2, 2); step(4, 0x402, {1: 9});
  step(4, 0x403); step(4, 0x403, {0: 0}); step(6, 0x403, {2: 0x0101, 3: 1}, 2, 2);
  step(4, 0x402, {7: 1});
  run(c, module, 2, {...cpu(c), pc: PC + code.length}, {label: 'live suffix retains returned CPU through cold lookup', reason: 3});
  run(c, module, 1, cpu(c), {label: 'live cold result retains all returned parents/FLAGS', reason: 3, retired: 0}); counts.consumers += 13;
  checkRam(c, DATA, canary); checkRam(c, PC, code); assert.equal(c.api.close(), 0);
}
assert.deepEqual(sources(), sourcePins);
assert.deepEqual(counts, {contexts: 20, runs: 1924, retired: 1876, matrix: 1632, anchors: 64, repeats: 64, current_inputs: 32, controls: 64, retained_units: 16, consumers: 52});
assert.equal(modules.length, 68);
const result = {status: 'ok', engine_sha256: engine.sha256, source_pins: sourcePins, counts, modules,
  limits: [
    'finite exact single66 WORD register BT/BTS/BTR/BTC register-index/imm8 in four actual bound profiles; native standalone six-profile coverage separate',
    'arithmetic bit-digit oracle and16 independent literal cases; original index modulo16, old selected CF, mutation low16 with all parent high16 retained',
    'existing Ring3 FLAGS policy retains ZF/DF/reserved2 and clears undefined arithmetic bits; no hardware undefined-bit equality claim',
    'pure units import guard only and use exitV1; entire helper/FP128 and remaining4612 arena stay unchanged by generated calls',
    'all code uploaded before capture; older resident units remain valid, replacement generation mismatch returns neutral status3; explicit current inputs and live consumers are separate',
    'read-only RAM/RX code canaries, no memory operand fault/RMW/SMC, exhaustiveISA, speed, PE/browser/SDK/game or hostedCI claim',
  ]};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2), {flag: 'wx'});
console.log(JSON.stringify({status: 'ok', ...counts, modules: modules.length, output}));
