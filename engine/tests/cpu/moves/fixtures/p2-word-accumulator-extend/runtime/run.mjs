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
  'engine/tests/cpu/moves/cases/native/cpu_word_accumulator_extend.rs', 'engine/tests/cpu/moves/native.rs', 'engine/tests/support/cpu/prefix_admission.rs', 'engine/tests/support/cpu/word_prefix.rs', 'engine/tests/cpu/moves/native.rs', 'engine/src/cpu/dbt/wasm/abi.rs',
  'engine/src/cpu/x86/decode/profile.rs', 'engine/src/cpu/dbt/region.rs',
  'engine/src/cpu/dbt/wasm/integer/arithmetic.rs', 'engine/src/cpu/dbt/wasm/integer/bits.rs', 'engine/src/cpu/dbt/wasm/integer/carry_rotates.rs', 'engine/src/cpu/dbt/wasm/integer/decimal.rs', 'engine/src/cpu/dbt/wasm/integer/dispatch.rs', 'engine/src/cpu/dbt/wasm/integer/flags.rs', 'engine/src/cpu/dbt/wasm/integer/mod.rs', 'engine/src/cpu/dbt/wasm/integer/multiply_divide.rs', 'engine/src/cpu/dbt/wasm/integer/operands.rs', 'engine/src/cpu/dbt/wasm/integer/rotates.rs', 'engine/src/cpu/dbt/wasm/integer/shifts.rs', 'engine/src/cpu/dbt/wasm/control.rs',
  'engine/src/cpu/dbt/wasm/emitter.rs', 'engine/src/process/wasm/mod.rs', 'engine/src/process/wasm/loading.rs', 'engine/src/process/wasm/execution.rs', 'engine/src/process/wasm/calls.rs', 'engine/src/process/wasm/memory.rs', 'engine/src/process/wasm/cpu.rs', 'engine/src/process/mod.rs',
  'engine/src/process/instance.rs', 'engine/src/process/execution/compiled.rs', 'engine/src/process/execution/authority.rs', 'engine/src/process/memory/address_space.rs', 'engine/src/process/memory/scalar.rs', 'engine/src/process/memory/scalar_tests.rs', 'engine/src/process/execution/mod.rs', 'engine/src/process/memory/mod.rs', 'engine/src/process/mod.rs', 'engine/src/process/execution/resident.rs', 'engine/src/process/execution/authority.rs', 'engine/src/process/execution/mod.rs', 'engine/src/process/mod.rs',
  'engine/tests/cpu/moves/cases/wasm/cpu_word_accumulator_extend_wasm.rs', 'engine/tests/cpu/moves/wasm.rs', 'engine/tests/support/wasm.rs', 'engine/tests/cpu/moves/wasm.rs', 'engine/tests/support/wasm.rs',
  'engine/tests/cpu/moves/fixtures/p2-word-accumulator-extend/runtime/run.mjs', 'engine/tests/support/wasm/engine.mjs',
];
const sources = () => Object.fromEntries(paths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = sources(), modules = [];
const counts = {contexts: 0, runs: 0, retired: 0, byte_inputs: 0, word_inputs: 0, flag_inputs: 0, anchors: 0, repeats: 0, current_inputs: 0, controls: 0, retained_units: 0, consumers: 0};
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
  assert.deepEqual(arena(c), expected, 'full4612 CPU/exit/unchanged helper/FP128 effect: ' + label);
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
  const module = new WebAssembly.Module(bytes), names = [c.owner === 'resident' ? 'guard_resident' : 'guard'];
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
  assert.equal(c.api.open(4, c.id, 0x57414343), 0); c.base = c.api.arena_ptr() >>> 0; input(c, 0, initial);
  for (const args of [[PC, 1, 7], [DATA, 1, 3]]) host(c, 'map', args);
  upload(c, DATA, canary); host(c, 'protect', [DATA, 1, 1]); return c;
}

// arithmetic range tests and modulo define the oracle without Wasm shift/sign-extension operations.
const lowWord = (parent, value) => Math.floor(parent / 65536) * 65536 + value;
function extended(kind, before) {
  const next = {...before, registers: [...before.registers], pc: before.pc + 2};
  if (kind === 'cbw') {
    const al = before.registers[0] % 256;
    next.registers[0] = lowWord(before.registers[0], al < 128 ? al : al + 65280);
  } else next.registers[2] = lowWord(before.registers[2], before.registers[0] % 65536 < 32768 ? 0 : 65535);
  return next;
}
const forms = [{name: 'cbw', pc: PC, bytes: Buffer.from([0x66, 0x98])}, {name: 'cwd', pc: PC + 0x20, bytes: Buffer.from([0x66, 0x99])}];
const flags = Array.from({length: 128}, (_, n) => 2 + [1, 4, 16, 64, 128, 1024, 2048].reduce((sum, bit, i) => sum + Math.floor(n / 2 ** i) % 2 * bit, 0));
assert.equal(new Set(flags).size, 128);
assert.ok(flags.every(value => (value & 2) !== 0 && (value & ~0xcd7) === 0));
const anchorFlags = [2, 0xcd7, 0x443, 0x892];
const cbwAnchors = [
  [0xabcd0080, 0xabcdff80], [0xabcdff7f, 0xabcd007f], [0x80010000, 0x80010000], [0x7fff7fff, 0x7fffffff],
  [0xffff1280, 0xffffff80], [0x1234aa01, 0x12340001], [0xabcd0081, 0xabcdff81], [0x0000ffff, 0x0000ffff],
];
const cwdAnchors = [
  [0xabcd0000, 0x13575678, 0x13570000], [0xabcd7fff, 0xfedc5678, 0xfedc0000],
  [0xabcd8000, 0x13575678, 0x1357ffff], [0xabcdffff, 0xfedc5678, 0xfedcffff],
  [0xabcd0080, 0x80015678, 0x80010000], [0xabcdff7f, 0x7fff5678, 0x7fffffff],
  [0x80010080, 0xffff5678, 0xffff0000], [0x7fff807f, 0x00005678, 0x0000ffff],
];
for (const [eax, expected] of cbwAnchors) {
  const r = registers(); r[0] = eax;
  assert.equal(extended('cbw', {registers: r, pc: PC, flags: 0xcd7}).registers[0], expected, 'literal AL sign and EAX.high16 anchor');
}
for (const [eax, edx, expected] of cwdAnchors) {
  const r = registers(); r[0] = eax; r[2] = edx;
  const next = extended('cwd', {registers: r, pc: PC, flags: 0xcd7});
  assert.equal(next.registers[2], expected, 'literal AX sign and EDX.high16 anchor'); assert.equal(next.registers[0], eax);
}
function operation(c, module, form, label) {
  const next = extended(form.name, cpu(c)); run(c, module, 1, next, {label}); return next;
}
const live = Buffer.from([
  0x66, 0x98, 0x66, 0x99, // cbw; cwd derive a signed DX:AX dividend from original AL.
  0x0f, 0x92, 0xc3, 0x0f, 0x90, 0xc7, // setb bl; seto bh leave the dividend and divisor untouched.
  0x66, 0xf7, 0xf9, // idiv cx returns quotient -2 and remainder -1.
  0x66, 0xf7, 0xe9, // imul cx consumes returned AX and produces the full DX:AX result -6.
  0x0f, 0x92, 0xc3, 0x0f, 0x90, 0xc7, // setb bl; seto bh observe the fit multiply flags.
  0x66, 0xf7, 0xf9, 0xeb, 0, // idiv cx consumes the live product, then the suffix reaches cold lookup.
]);

for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries), livePc = PC + 0x100;
  // all code and permission inputs precede every capture so older resident units stay current.
  for (const form of forms) upload(c, form.pc, block(form.pc, form.bytes).bytes);
  upload(c, livePc, live); host(c, 'protect', [PC, 1, 5]);
  const pure = compile(c, forms.map(form => block(form.pc, form.bytes)));
  for (let al = 0; al < 256; al++) {
    const r = registers(), high = (0x8001 + al * 257) % 65536, ah = al < 128 ? 0xaa : 0x55;
    r[0] = high * 65536 + ah * 256 + al;
    assert.notEqual(ah, al < 128 ? 0 : 255, 'AH is poisoned rather than already sign-extended');
    seed(c, forms[0].pc, r, anchorFlags[al % 4]); operation(c, pure, forms[0], 'all256 AL values with poisoned AH and varied EAX.high16'); counts.byte_inputs++;
  }
  for (const [i, ax] of [0, 1, 0x7f, 0x80, 0xff, 0x100, 0x7f00, 0x7f7f, 0x7f80, 0x7fff, 0x8000, 0x807f, 0x80ff, 0xff00, 0xff7f, 0xffff].entries())
    for (const high of [0x1357, 0xfedc]) {
      const r = registers(); r[0] = (i % 2 ? 0x1234 : 0xcafe) * 65536 + ax; r[2] = high * 65536 + 0x5aa5;
      seed(c, forms[1].pc, r, anchorFlags[i % 4]); operation(c, pure, forms[1], 'AX sign boundaries and AL/AX sign disagreement with independent EDX.high16'); counts.word_inputs++;
    }
  for (const form of forms) for (const [i, value] of flags.entries()) {
    const r = registers(); r[0] = (0x8123 + i) * 65536 + (i % 2 ? 0xff7f : 0x0080); r[2] = (0xfedc - i) * 65536 + 0x1357;
    seed(c, form.pc, r, value); operation(c, pure, form, 'all128 independently valid raw FLAGS retained exactly by each form'); counts.flag_inputs++;
  }
  for (const [i, [eax, expected]] of cbwAnchors.entries()) {
    const r = registers(); r[0] = eax; seed(c, forms[0].pc, r, anchorFlags[i % 4]);
    assert.equal(operation(c, pure, forms[0], 'literal CBW anchor').registers[0], expected); counts.anchors++;
  }
  for (const [i, [eax, edx, expected]] of cwdAnchors.entries()) {
    const r = registers(); r[0] = eax; r[2] = edx; seed(c, forms[1].pc, r, anchorFlags[i % 4]);
    assert.equal(operation(c, pure, forms[1], 'literal CWD anchor').registers[2], expected); counts.anchors++;
  }
  for (const form of forms) {
    const r = registers(); r[0] = 0xcafe7f80; r[2] = 0xface5678; seed(c, form.pc, r);
    operation(c, pure, form, 'first current-parent operation');
    input(c, 0, state({...cpu(c), pc: form.pc})); operation(c, pure, form, 'only EIP rewound, live parent and FLAGS retained'); counts.repeats += 2;
    const current = cpu(c); current.pc = form.pc; current.flags = 0x892; current.registers[0] = 0x8001ff7f; current.registers[2] = 0x7fff1357;
    input(c, 0, state(current)); operation(c, pure, form, 'declared current AL/AX and both destination-high inputs'); counts.current_inputs++;
    seed(c, form.pc, r);
    run(c, pure, 0, cpu(c), {label: 'zero budget leaves full CPU/FLAGS/helper/FP untouched', retired: 0});
    input(c, 96, words([1]));
    run(c, pure, 1, cpu(c), {label: 'cancel before target leaves all state untouched', reason: 2, retired: 0});
    run(c, pure, 0, cpu(c), {label: 'cancel takes precedence over zero budget', reason: 2, retired: 0}); input(c, 96, words([0]));
    input(c, 0, state({...cpu(c), pc: form.pc + 2}));
    run(c, pure, 2, {...cpu(c), pc: form.pc + 4}, {label: 'pure suffix uses remaining budget before unavailable lookup', reason: 3});
    run(c, pure, 1, cpu(c), {label: 'cold entry preserves live returned parents/FLAGS', reason: 3, retired: 0}); counts.controls += 5;
  }
  checkRam(c, DATA, canary); for (const form of forms) checkRam(c, form.pc, block(form.pc, form.bytes).bytes);
  const mixed = compile(c, [{pc: livePc, bytes: live}]);
  seed(c, forms[0].pc, registers());
  if (owner === 'resident') operation(c, pure, forms[0], 'older resident pure unit remains valid after the mixed capture');
  else run(c, pure, 1, cpu(c), {label: 'replaced generation returns neutral InvalidArtifact3 before execution', status: 3, retired: 0});
  counts.retained_units++;
  const r = registers(); r[0] = 0xcafe12f9; r[1] = 0xbeef0003; r[2] = 0xface5678; seed(c, livePc, r);
  function step(length, nextFlags, changes = {}, budget = 1, retired = 1) {
    const before = cpu(c), next = {...before, registers: [...before.registers], pc: before.pc + length, flags: nextFlags};
    for (const [index, value] of Object.entries(changes)) next.registers[index] = lowWord(before.registers[index], value);
    run(c, mixed, budget, next, {label: 'live extension/dividend/quotient/remainder/product/flags chain without CPU reseeding', retired, exitVersion: 5});
  }
  step(2, 0xcd7, {0: 0xfff9}); step(2, 0xcd7, {2: 0xffff}); step(6, 0xcd7, {3: 0x0101}, 2, 2);
  step(3, 0xcd7, {0: 0xfffe, 2: 0xffff}); step(3, 0x402, {0: 0xfffa, 2: 0xffff});
  step(6, 0x402, {3: 0}, 2, 2); step(3, 0x402, {0: 0xfffe, 2: 0});
  run(c, mixed, 2, {...cpu(c), pc: livePc + live.length}, {label: 'mixed suffix uses budget2 and retains returned CPU through cold lookup', reason: 3, exitVersion: 5});
  run(c, mixed, 1, cpu(c), {label: 'mixed cold entry keeps live quotient/remainder/FLAGS', reason: 3, retired: 0, exitVersion: 5}); counts.consumers += 9;
  checkRam(c, DATA, canary); checkRam(c, livePc, live); assert.equal(c.api.close(), 0);
}
assert.deepEqual(sources(), sourcePins);
assert.deepEqual(counts, {contexts: 4, runs: 2344, retired: 2314, byte_inputs: 1024, word_inputs: 128, flag_inputs: 1024, anchors: 64, repeats: 16, current_inputs: 8, controls: 40, retained_units: 4, consumers: 36});
assert.equal(modules.length, 8);
const result = {status: 'ok', engine_sha256: engine.sha256, source_pins: sourcePins, counts, modules,
  limits: [
    'finite exact single66 CBW/CWD in four actual bound profiles; six-profile native admission is separate',
    'arithmetic modulo/range oracle, all256 AL, poisoned AH, AX sign boundaries,16 literal anchors and all128 valid FLAGS for each form',
    'CBW merges AX into current EAX.high16; CWD merges DX into current EDX.high16 without changing EAX; other GPRs and full valid FLAGS unchanged',
    'pure guard-only V1 units and existing mixed-division V5; entire helper/FP128 and remaining4612 arena preserved by generated calls',
    'two units/context, all code and permissions fixed before capture; replacement generation mismatch3 versus successful retained resident unit',
    'live signed dividend/quotient/remainder/full product/SETcc consumers use returned state without CPU reseeding; explicit host-input witnesses are separate',
    'read-only RAM/RX code canaries; no memory operand fault/SMC, exhaustiveISA, speed, PE/browser/SDK/game or hostedCI claim',
  ]};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2), {flag: 'wx'});
console.log(JSON.stringify({status: 'ok', ...counts, modules: modules.length, output}));

