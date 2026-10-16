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
  'engine/tests/cpu/moves/exchange/register/native/word.rs', 'engine/tests/cpu/moves/native.rs', 'engine/tests/support/cpu/prefix/admission.rs', 'engine/tests/support/cpu/prefix/word.rs', 'engine/src/cpu/dbt/wasm/abi.rs',
  'engine/src/cpu/x86/decode/profile.rs', 'engine/src/cpu/x86/decode/families.rs', 'engine/src/cpu/dbt/region.rs',
  'engine/src/cpu/dbt/wasm/instructions/integer/arithmetic.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/bits.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/carry_rotates.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/decimal.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/mod.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/arithmetic.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/moves.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/exchange.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/predicates.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/shifts.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/control.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/extended.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/flags.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/mod.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/multiply_divide.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/operands.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/rotates.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/shifts.rs', 'engine/src/cpu/dbt/wasm/instructions/control.rs',
  'engine/src/cpu/dbt/wasm/emitter/mod.rs', 'engine/src/cpu/dbt/wasm/emitter/module.rs', 'engine/src/cpu/dbt/wasm/emitter/body.rs', 'engine/src/cpu/dbt/wasm/requirements.rs', 'engine/src/cpu/dbt/wasm/imports/wiring.rs', 'engine/src/process/wasm/mod.rs', 'engine/src/process/wasm/loading.rs', 'engine/src/abi/image.rs', 'engine/src/process/wasm/execution.rs', 'engine/src/abi/execution.rs', 'engine/src/process/wasm/calls.rs', 'engine/src/process/wasm/memory.rs', 'engine/src/process/wasm/cpu.rs', 'engine/src/process/mod.rs',
  'engine/src/process/instance.rs', 'engine/src/process/execution/compiled.rs', 'engine/src/process/execution/authority.rs', 'engine/src/process/memory/address_space.rs', 'engine/src/process/memory/scalar.rs', 'engine/src/process/memory/scalar/tests.rs', 'engine/src/process/execution/mod.rs', 'engine/src/process/memory/mod.rs', 'engine/src/process/execution/resident.rs',
  'engine/tests/cpu/moves/exchange/register/wasm/word.rs', 'engine/tests/cpu/moves/wasm.rs', 'engine/tests/support/wasm.rs',
  'engine/tests/cpu/moves/exchange/register/fixtures/p2-word-register-exchange/runtime/run.mjs', 'engine/tests/support/wasm/engine.mjs', 'engine/tests/support/wasm/binding.mjs'];
assert.equal(new Set(paths).size, paths.length);
const sources = () => Object.fromEntries(paths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = sources(), modules = [];
const counts = {contexts: 0, runs: 0, retired: 0, pairs: 0, short_forms: 0, flag_inputs: 0, anchors: 0, repeats: 0, current_inputs: 0, controls: 0, retained_units: 0, consumers: 0};
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
  assert.equal(c.api.open(4, c.id, 0x57584348), 0); c.base = c.api.arena_ptr() >>> 0; input(c, 0, initial);
  for (const args of [[PC, 1, 7], [DATA, 1, 3]]) host(c, 'map', args);
  upload(c, DATA, canary); host(c, 'protect', [DATA, 1, 1]); return c;
}

// modulo and division independently recover both old low halves before either expected parent changes.
const lowWord = (parent, value) => Math.floor(parent / 65536) * 65536 + value;
function exchanged(before, row) {
  const next = {...before, registers: [...before.registers], pc: before.pc + row.bytes.length};
  const a = before.registers[row.left] % 65536, b = before.registers[row.right] % 65536;
  next.registers[row.left] = lowWord(before.registers[row.left], b);
  next.registers[row.right] = lowWord(before.registers[row.right], a);
  return next;
}
const flags = Array.from({length: 128}, (_, n) => 2 + [1, 4, 16, 64, 128, 1024, 2048].reduce((sum, bit, i) => sum + Math.floor(n / 2 ** i) % 2 * bit, 0));
assert.equal(new Set(flags).size, 128); assert.ok(flags.every(value => (value & 2) && !(value & ~0xcd7)));
const anchorFlags = [2, 0xcd7, 0x443, 0x892];
const pair = (left, right, pc) => ({left, right, pc, bytes: Buffer.from([0x66, 0x87, 0xc0 | right << 3 | left])});
const short = (right, pc) => ({left: 0, right, pc, bytes: Buffer.from([0x66, 0x90 + right])});
const literalAnchors = [
  {family: 'pair', left: 0, right: 2, a: 0xabcd8001, b: 0x13577ffe, outA: 0xabcd7ffe, outB: 0x13578001},
  {family: 'pair', left: 0, right: 2, a: 0x1234ffff, b: 0xfedc0000, outA: 0x12340000, outB: 0xfedcffff},
  {family: 'pair', left: 4, right: 4, a: 0xcafe1234, b: 0xcafe1234, outA: 0xcafe1234, outB: 0xcafe1234},
  {family: 'pair', left: 4, right: 5, a: 0x80010000, b: 0x7fffffff, outA: 0x8001ffff, outB: 0x7fff0000},
  {family: 'short', left: 0, right: 1, a: 0xcafe5aa5, b: 0x1234a55a, outA: 0xcafea55a, outB: 0x12345aa5},
  {family: 'short', left: 0, right: 4, a: 0x8001ffff, b: 0x7fff0000, outA: 0x80010000, outB: 0x7fffffff},
  {family: 'short', left: 0, right: 5, a: 0x00008000, b: 0xffff007f, outA: 0x0000007f, outB: 0xffff8000},
  {family: 'short', left: 0, right: 0, a: 0x89abcdef, b: 0x89abcdef, outA: 0x89abcdef, outB: 0x89abcdef},
];
for (const a of literalAnchors) {
  const r = registers(); r[a.left] = a.a; r[a.right] = a.b;
  const row = a.family === 'pair' ? pair(a.left, a.right, PC) : short(a.right, PC);
  const next = exchanged({registers: r, pc: PC, flags: 0xcd7}, row);
  assert.equal(next.registers[a.left], a.outA); assert.equal(next.registers[a.right], a.outB);
}
function operation(c, module, row, label) {
  const next = exchanged(cpu(c), row); run(c, module, 1, next, {label}); return next;
}
function parents(row, variant) {
  const r = registers().map((value, i) => ((0x8001 + i * 0x1357 + variant * 0x4321) % 65536) * 65536 + value % 65536);
  const [a, b] = [[0, 0xffff], [0x8001, 0x7ffe], [0xa55a, 0x5aa5]][variant];
  r[row.left] = lowWord(r[row.left], a); r[row.right] = lowWord(r[row.right], b); return r;
}
function anchors(c, module, row, family) {
  for (const [i, a] of literalAnchors.entries()) if (a.family === family && a.left === row.left && a.right === row.right) {
    const r = registers(); r[a.left] = a.a; r[a.right] = a.b; seed(c, row.pc, r, anchorFlags[i % 4]);
    const next = operation(c, module, row, 'literal original-low/own-high/self-alias merged-parent anchor');
    assert.equal(next.registers[a.left], a.outA); assert.equal(next.registers[a.right], a.outB); counts.anchors++;
  }
}
function repeatAndEdit(c, module, row) {
  seed(c, row.pc, parents(row, 1)); operation(c, module, row, 'first exchange of current parents');
  input(c, 0, state({...cpu(c), pc: row.pc})); operation(c, module, row, 'only PC rewound; second exchange consumes returned parents'); counts.repeats += 2;
  const current = cpu(c); current.pc = row.pc; current.flags = 0x892;
  current.registers[row.left] = 0xc0de8000; current.registers[row.right] = 0x7fff007f;
  input(c, 0, state(current)); operation(c, module, row, 'declared current low/high parents and FLAGS'); counts.current_inputs++;
}
function controls(c, module, row) {
  seed(c, row.pc, parents(row, 0)); run(c, module, 0, cpu(c), {label: 'zero budget preserves full CPU/FLAGS/helper/FP', retired: 0});
  input(c, 96, words([1])); run(c, module, 1, cpu(c), {label: 'cancel before exchange/NOP is neutral', reason: 2, retired: 0});
  run(c, module, 0, cpu(c), {label: 'cancel precedes zero budget', reason: 2, retired: 0}); input(c, 96, words([0]));
  input(c, 0, state({...cpu(c), pc: row.pc + row.bytes.length}));
  run(c, module, 2, {...cpu(c), pc: row.pc + row.bytes.length + 2}, {label: 'suffix JMP has budget remaining for unavailable lookup', reason: 3});
  run(c, module, 1, cpu(c), {label: 'cold lookup preserves returned parents/FLAGS', reason: 3, retired: 0}); counts.controls += 5;
}
const live = Buffer.from([
  0x66, 0x87, 0xd8, // xchg ax,bx returns8 in AX and8000 in BX.
  0x66, 0x01, 0xc8, // add ax,cx returns7 with carry and auxiliary carry.
  0x66, 0x0f, 0xbc, 0xd3, // bsf dx,bx returns15 from the exchanged bitmap.
  0x66, 0x92, 0x66, 0x98, // xchg ax,dx; cbw consumes returned AL15.
  0x66, 0x87, 0xd2, 0x66, 0x90, // xchg dx,dx; exact WORD NOP each retire once.
  0x66, 0x0f, 0xba, 0xfb, 15, // btc bx,15 clears the bitmap and publishes old bit1.
  0x0f, 0x92, 0xc1, 0x0f, 0x94, 0xc5, 0xeb, 0, // setb cl; setz ch; suffix JMP.
]);
for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) for (const group of [0, 1]) {
  const c = open(owner, entries);
  const rows = Array.from({length: 32}, (_, i) => pair(group * 4 + Math.floor(i / 8), i % 8, PC + i * 16));
  const shorts = Array.from({length: 8}, (_, i) => short(i, PC + 0x300 + i * 16)), livePc = PC + 0x400;
  // all code uploads and RX permissions precede every capture to retain older resident snapshots.
  for (const row of rows) upload(c, row.pc, block(row.pc, row.bytes).bytes);
  if (group === 1) {for (const row of shorts) upload(c, row.pc, block(row.pc, row.bytes).bytes); upload(c, livePc, live);}
  host(c, 'protect', [PC, 1, 5]); let firstModule;
  for (let start = 0; start < 32; start += 8) {
    const selected = rows.slice(start, start + 8), module = compile(c, selected.map(row => block(row.pc, row.bytes)));
    if (start === 0) firstModule = module;
    for (const row of selected) {
      for (let variant = 0; variant < 2; variant++) {
        seed(c, row.pc, parents(row, variant), anchorFlags[(row.left + row.right + variant) % 4]);
        operation(c, module, row, 'all64 ModRM pairs with opposite lowWORD values and independently dirty parent halves'); counts.pairs++;
      }
      anchors(c, module, row, 'pair');
    }
    if (group === 0 && start === 0) {
      const row = rows[2];
      for (const value of flags) {seed(c, row.pc, parents(row, 1), value); operation(c, module, row, 'all128 valid FLAGS in a distinguishing ModRM pair'); counts.flag_inputs++;}
      repeatAndEdit(c, module, row); controls(c, module, row);
    }
  }
  if (group === 1) {
    const shortModule = compile(c, shorts.map(row => block(row.pc, row.bytes)));
    for (const row of shorts) {
      for (let variant = 0; variant < 3; variant++) {
        seed(c, row.pc, parents(row, variant), anchorFlags[(row.right + variant) % 4]);
        operation(c, shortModule, row, 'all eight short encodings including NOP alias retirement'); counts.short_forms++;
      }
      anchors(c, shortModule, row, 'short');
    }
    for (const value of flags) {seed(c, shorts[1].pc, parents(shorts[1], 2), value); operation(c, shortModule, shorts[1], 'all128 valid FLAGS in a distinguishing AX short exchange'); counts.flag_inputs++;}
    for (const row of [shorts[1], shorts[0]]) {repeatAndEdit(c, shortModule, row); controls(c, shortModule, row);}
    const mixed = compile(c, [{pc: livePc, bytes: live}]); seed(c, rows[0].pc, parents(rows[0], 1));
    if (owner === 'resident') operation(c, firstModule, rows[0], 'older resident exchange unit remains current after five later captures');
    else run(c, firstModule, 1, cpu(c), {label: 'replacement generation mismatch3 is neutral before instruction execution', status: 3, retired: 0});
    counts.retained_units++;
    const r = registers(); r[0] = lowWord(r[0], 0x8000); r[3] = lowWord(r[3], 8); r[1] = lowWord(r[1], 0xffff); seed(c, livePc, r);
    function step(length, nextFlags, changes = {}, budget = 1, retired = 1) {
      const before = cpu(c), next = {...before, registers: [...before.registers], pc: before.pc + length, flags: nextFlags};
      for (const [index, value] of Object.entries(changes)) next.registers[index] = lowWord(before.registers[index], value);
      run(c, mixed, budget, next, {label: 'live exchanged bitmap/sum/index/flags consumer without CPU reseeding', retired});
    }
    step(3, 0xcd7, {0: 8, 3: 0x8000}); step(3, 0x413, {0: 7}); step(4, 0x402, {2: 15});
    step(2, 0x402, {0: 15, 2: 7}); step(2, 0x402); step(3, 0x402); step(2, 0x402);
    step(5, 0x403, {3: 0}); step(6, 0x403, {1: 1}, 2, 2);
    run(c, mixed, 2, {...cpu(c), pc: livePc + live.length}, {label: 'live suffix retains exchanged/derived parents through unavailable lookup', reason: 3});
    run(c, mixed, 1, cpu(c), {label: 'live cold result preserves all returned CPU/FLAGS', reason: 3, retired: 0}); counts.consumers += 11;
    checkRam(c, livePc, live);
  }
  checkRam(c, DATA, canary); checkRam(c, rows[0].pc, block(rows[0].pc, rows[0].bytes).bytes);
  checkRam(c, rows[31].pc, block(rows[31].pc, rows[31].bytes).bytes);
  if (group === 1) checkRam(c, shorts[0].pc, block(shorts[0].pc, shorts[0].bytes).bytes);
  assert.equal(c.api.close(), 0);
}
assert.deepEqual(sources(), sourcePins);
assert.deepEqual(counts, {contexts: 8, runs: 1808, retired: 1758, pairs: 512, short_forms: 96, flag_inputs: 1024, anchors: 32, repeats: 24, current_inputs: 12, controls: 60, retained_units: 4, consumers: 44});
assert.equal(modules.length, 40);
const result = {status: 'ok', engine_sha256: engine.sha256, source_pins: sourcePins, counts, modules,
  limits: [
    'finite exact single66 WORD register XCHG64 ModRM pairs and eight short encodings including NOP in four actual bound profiles; native six-profile coverage is separate',
    'independent arithmetic lowWORD/own-high oracle and eight literal anchors; both original lows captured, self aliases preserve whole parents',
    'all128 valid raw FLAGS per ModRM/short family through distinguishing selected pairs rather than a Cartesian bank',
    'guard-only pure V1 units; entire helper/FP128 and remaining4612 arena preserved, no new helper/local/ABI/V5 change',
    'maxsix units/context; fixed code/RX permissions before all captures, retained resident success versus neutral replacement generation mismatch3',
    'returned register/bitmap/sum/index/FLAGS consumers without CPU reseeding; declared current-parent witnesses are separate',
    'read-only RAM/RX code canaries; no memory XCHG/implicit locking/atomicity/fault/SMC, exhaustiveISA, speed, PE/browser/SDK/game or hostedCI claim',
  ]};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2), {flag: 'wx'});
console.log(JSON.stringify({status: 'ok', ...counts, modules: modules.length, output}));

