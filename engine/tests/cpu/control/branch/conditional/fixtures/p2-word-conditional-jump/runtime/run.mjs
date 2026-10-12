import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../../../../../../../support/wasm/engine.mjs';

const [enginePath, output, root] = process.argv.slice(2), SIZE = 4612;
assert.ok(enginePath && output && root);
const engine = readEngine(enginePath), initial = readFileSync(join(output, 'initial-arena.bin'));
assert.equal(initial.length, SIZE); assert.deepEqual(WebAssembly.Module.imports(engine.module), []);
const hash = b => createHash('sha256').update(b).digest('hex');
const paths = ['engine/src/abi/arena.rs', 'engine/src/cpu/x86/decode/profile.rs', 'engine/src/cpu/x86/decode/flow.rs',
  'engine/src/cpu/x86/decode/decoder.rs', 'engine/src/cpu/dbt/region.rs', 'engine/src/cpu/dbt/cold.rs',
  'engine/src/cpu/dbt/artifact.rs', 'engine/src/cpu/dbt/wasm/abi.rs', 'engine/src/cpu/dbt/wasm/integer/arithmetic.rs', 'engine/src/cpu/dbt/wasm/integer/bits.rs', 'engine/src/cpu/dbt/wasm/integer/carry_rotates.rs', 'engine/src/cpu/dbt/wasm/integer/decimal.rs', 'engine/src/cpu/dbt/wasm/integer/dispatch.rs', 'engine/src/cpu/dbt/wasm/integer/flags.rs', 'engine/src/cpu/dbt/wasm/integer/mod.rs', 'engine/src/cpu/dbt/wasm/integer/multiply_divide.rs', 'engine/src/cpu/dbt/wasm/integer/operands.rs', 'engine/src/cpu/dbt/wasm/integer/rotates.rs', 'engine/src/cpu/dbt/wasm/integer/shifts.rs',
  'engine/src/cpu/dbt/wasm/control.rs', 'engine/src/cpu/dbt/wasm/emitter.rs', 'engine/src/process/instance.rs', 'engine/src/process/execution/compiled.rs', 'engine/src/process/execution/authority.rs', 'engine/src/process/memory/address_space.rs', 'engine/src/process/memory/scalar.rs', 'engine/src/process/memory/scalar/tests.rs', 'engine/src/process/execution/mod.rs', 'engine/src/process/memory/mod.rs', 'engine/src/process/mod.rs',
  'engine/src/process/execution/resident.rs', 'engine/src/process/wasm/mod.rs', 'engine/src/process/wasm/loading.rs', 'engine/src/process/wasm/execution.rs', 'engine/src/process/wasm/calls.rs', 'engine/src/process/wasm/memory.rs', 'engine/src/process/wasm/cpu.rs', 'engine/src/memory/space.rs', 'engine/tests/cpu/control/branch/direct/native/word.rs', 'engine/tests/cpu/control/native.rs', 'engine/tests/support/cpu/prefix/admission.rs', 'engine/tests/support/cpu/prefix/word.rs', 'engine/tests/cpu/control/branch/conditional/wasm/word.rs', 'engine/tests/cpu/control/wasm.rs', 'engine/tests/support/wasm.rs', 'engine/tests/cpu/control/branch/conditional/fixtures/p2-word-conditional-jump/runtime/run.mjs', 'engine/tests/support/wasm/engine.mjs'];
assert.equal(new Set(paths).size, paths.length);
const sources = () => Object.fromEntries(paths.map(p => [p, hash(readFileSync(join(root, p)))]));
const sourcePins = sources(), modules = [], observations = [];
const counts = {contexts: 0, runs: 0, retired: 0, flag_inputs: 0, boundaries: 0, repeats: 0, current_inputs: 0,
  controls: 0, consumers: 0, retained: 0, invalidated: 0};
const wrap = n => Number((BigInt(n) % 4294967296n + 4294967296n) % 4294967296n);
const target = (pc, raw) => Number((BigInt(pc) + 5n + BigInt(raw < 32768 ? raw : raw - 65536) + 4294967296n) % 65536n);
const flags = Array.from({length: 128}, (_, n) => 2 + [1, 4, 16, 64, 128, 1024, 2048]
  .reduce((sum, bit, i) => sum + Math.floor(n / 2 ** i) % 2 * bit, 0));
assert.equal(new Set(flags).size, 128); assert.ok(flags.every(n => (n & 2) && !(n & ~0xcd7)));
function taken(cc, f) {
  const bit = weight => Math.floor(f / weight) % 2 === 1;
  const cf = bit(1), pf = bit(4), zf = bit(64), sf = bit(128), of = bit(2048);
  return [of, !of, cf, !cf, zf, !zf, cf || zf, !cf && !zf,
    sf, !sf, pf, !pf, sf !== of, sf === of, zf || sf !== of, !zf && sf === of][cc];
}
const trueFlags = [0x802, 2, 3, 2, 0x42, 2, 3, 2, 0x82, 2, 6, 2, 0x82, 2, 0x42, 2];
const falseFlags = [2, 0x802, 2, 3, 2, 0x42, 2, 3, 2, 0x82, 2, 6, 2, 0x82, 2, 0x42];
for (let cc = 0; cc < 16; cc++) {assert.equal(taken(cc, trueFlags[cc]), true); assert.equal(taken(cc, falseFlags[cc]), false);}
const raw = [0, 1, 0xffff, 0x8000, 0x7fff, 0x6666, 0x6767, 0xf0f0, 0xf2f2, 0xf3f3, 0x66f0, 0xf066, 2, 0xfffe, 0x8001, 0x7ffe];
const literalTargets = [0x1005, 0x1026, 0x1044, 0x9065, 0x9084, 0x770b, 0x782c, 0x1d5,
  0x3f7, 0x518, 0x7835, 0x1cb, 0x1187, 0x11a3, 0x91c6, 0x91e3];
const conditions = raw.map((value, cc) => {
  const pc = 0x12341000 + cc * 0x20, bytes = readFileSync(join(output, `conditions-${Math.floor(cc / 8)}-${cc % 8}.x86`));
  assert.deepEqual(bytes, Buffer.from([0x66, 0x0f, 0x80 + cc, value % 256, Math.floor(value / 256)]));
  assert.equal(target(pc, value), literalTargets[cc], 'independent signed rel16 target literal');
  return {pc, cc, raw: value, target: literalTargets[cc], bytes, instructions: 1};
});
const boundaryLive = [
  [0x1234fffb, [0x66, 0x0f, 0x84, 2, 0], 1], [0xfffffffb, [0x66, 0x0f, 0x84, 2, 0], 1],
  [0x12341a00, [0x66, 0x39, 0xc8, 0x66, 0x0f, 0x84, 0xf8, 0xfd], 2],
  [0x12341a08, [0x66, 0xba, 2, 0, 0x66, 0xe9, 0xf0, 0xfe], 2],
  [0x1800, [0x66, 0xba, 1, 0, 0x66, 0xe9, 0xf8, 0], 2],
  [0x1900, [0x66, 0x11, 0xd0, 0x0f, 0x92, 0xc3, 0x0f, 0x94, 0xc7, 0xeb, 0], 4],
].map(([pc, literal, instructions], i) => {
  const bytes = readFileSync(join(output, `boundary-live-${i}.x86`)); assert.deepEqual(bytes, Buffer.from(literal)); return {pc, bytes, instructions};
});
assert.equal(target(0x1234fffb, 2), 2); assert.equal(wrap(0x1234fffb + 5), 0x12350000);
assert.equal(target(0xfffffffb, 2), 2); assert.equal(wrap(0xfffffffb + 5), 0);
assert.deepEqual(JSON.parse(readFileSync(join(output, 'standalone-currentness.json'))),
  {execute_only_units: 6, protection_rejects_retrieval: true, same_byte_probe_units: 1, same_byte_write_rejects_retrieval: true});
const words = values => {const b = Buffer.alloc(values.length * 4); values.forEach((v, i) => b.writeUInt32LE(v >>> 0, i * 4)); return b;};
function record(magic, size, fields) {
  const b = Buffer.alloc(size); b.write(magic); b.writeUInt16LE(1, 4); b.writeUInt16LE(1, 6); b.writeUInt32LE(size, 8);
  fields.forEach((v, i) => b.writeUInt32LE(v >>> 0, 16 + i * 4)); return b;
}
const state = s => record('R3ST', 56, [...s.registers, s.pc, s.flags]);
const exit = (reason, retired) => record('R3EX', 40, [reason, retired, 0, 0, 0, 0]);
const arena = c => Buffer.from(new Uint8Array(c.memory.buffer, c.base, SIZE));
function input(c, offset, bytes) {
  const expected = arena(c); expected.set(bytes, offset); new Uint8Array(c.memory.buffer).set(bytes, c.base + offset);
  assert.deepEqual(arena(c), expected, 'declared input leaves remaining full arena intact');
}
function host(c, name, args) {
  const expected = arena(c); assert.equal(c.api[name](...args), 0, name);
  if (name.startsWith('compile_resident')) {
    const b = arena(c); assert.equal(b.readUInt32LE(140), 1); assert.equal(b.readUInt32LE(144), 24); expected.set(b.subarray(140, 164), 140);
  }
  assert.deepEqual(arena(c), expected, 'host full arena effect: ' + name);
}
function upload(c, pc, bytes) {input(c, 140, bytes); host(c, 'upload', [pc, bytes.length]);}
const cpu = c => {const b = arena(c); return {registers: Array.from({length: 8}, (_, i) => b.readUInt32LE(16 + i * 4)), pc: b.readUInt32LE(48), flags: b.readUInt32LE(52)};};
const registers = () => Array.from({length: 8}, (_, i) => (0xa100 + i) * 65536 + 0x1234 + i);
function seed(c, pc, f = 0xcd7, r = registers()) {
  const b = Buffer.alloc(140); b.set(state({registers: r, pc, flags: f})); b.set(exit(3, 0), 56);
  b.set(record('R3MH', 40, [0, 0xdecafbad, 0, 0, 0, 0]), 100); input(c, 0, b); return cpu(c);
}
function run(c, unit, budget, next, label, {reason = 1, retired = 1, status = 0} = {}) {
  const expected = arena(c); if (status === 0) {expected.set(state(next)); expected.set(exit(reason, retired), 56);}
  assert.equal(unit.run(c.base, c.base + 56, budget, c.base + 96), status, label);
  assert.deepEqual(arena(c), expected, 'full4612 GPR/FLAGS/EIP/exit/helper/dirtyFP: ' + label);
  const after = arena(c); observations.push({context: c.id, label, status, expected_reason: status === 0 ? reason : null,
    expected_retired: status === 0 ? retired : null, observed_reason: status === 0 ? after.readUInt32LE(72) : null,
    observed_retired: status === 0 ? after.readUInt32LE(76) : null});
  counts.runs++; if (status === 0) counts.retired += retired;
}
function compile(c, name, blocks) {
  const instructions = blocks.reduce((sum, b) => sum + b.instructions, 0);
  assert.ok(blocks.length > 0 && blocks.length <= 8 && instructions <= 64);
  assert.ok(blocks.reduce((sum, b) => sum + b.bytes.length, 0) <= 65536); assert.ok(++c.captures <= 6);
  let bytes, binding = {};
  if (c.owner === 'standalone') bytes = readFileSync(join(output, `standalone-${c.entries ? 'entries' : 'extent'}-${name}.wasm`));
  else {
    input(c, 140, words(blocks.flatMap(b => c.entries ? [b.pc] : [b.pc, b.bytes.length])));
    const method = c.owner === 'resident' ? (c.entries ? 'compile_resident_entries' : 'compile_resident') : (c.entries ? 'compile_entries' : 'compile');
    host(c, method, c.entries ? [blocks.length, 0] : [blocks.length]); const b = arena(c);
    binding = c.owner === 'resident' ? {low: b.readUInt32LE(148), high: b.readUInt32LE(152), pointer: b.readUInt32LE(156), length: b.readUInt32LE(160)}
      : {generation: c.api.generation(), pointer: c.api.module_ptr() >>> 0, length: c.api.module_len() >>> 0};
    bytes = Buffer.from(new Uint8Array(c.memory.buffer, binding.pointer, binding.length));
  }
  assert.ok(bytes.length <= 65536); const module = new WebAssembly.Module(bytes), guard = c.owner === 'resident' ? 'guard_resident' : 'guard';
  const imports = [{module: 'env', name: 'memory', kind: 'memory'}, ...(c.owner === 'standalone' ? [] : [{module: 'ring3', name: guard, kind: 'function'}])];
  assert.deepEqual(WebAssembly.Module.imports(module), imports); assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  const file = `module-${modules.length + 1}.wasm`; writeFileSync(join(output, file), bytes, {flag: 'wx'});
  modules.push({context: c.id, owner: c.owner, entries: c.entries, name, blocks: blocks.length, instructions, file, sha256: hash(bytes), imports, ...binding});
  return {run: new WebAssembly.Instance(module, {env: {memory: c.memory}, ...(c.api ? {ring3: c.api} : {})}).exports.run};
}
function open(owner, entries) {
  const c = {id: ++counts.contexts, owner, entries, captures: 0};
  if (owner === 'standalone') {c.memory = new WebAssembly.Memory({initial: 1}); c.base = 128; input(c, 0, initial); return c;}
  const instance = new WebAssembly.Instance(engine.module, {}); c.memory = instance.exports.memory; c.api = {};
  for (const name of ['open', 'close', 'arena_ptr', 'map', 'protect', 'upload', 'read8', 'compile', 'compile_entries',
    'compile_resident', 'compile_resident_entries', 'generation', 'module_ptr', 'module_len', 'guard', 'guard_resident']) c.api[name] = instance.exports['ring3_abi_v1_' + name];
  assert.equal(c.api.open(5, c.id, 0x574a4343), 0); c.base = c.api.arena_ptr() >>> 0; input(c, 0, initial);
  for (const page of [0x1000, 0x12341000, 0x1234f000, 0xfffff000, 0x7000]) host(c, 'map', [page, 1, 7]);
  for (const b of [...conditions, ...boundaryLive]) upload(c, b.pc, b.bytes);
  for (const [address, value] of [[0x770b, 0x55], [0x782c, 0xa5], [0x7835, 0xf0]]) upload(c, address, Buffer.from([value]));
  host(c, 'protect', [0x7000, 1, 1]); return c;
}
function branch(c, unit, row, label, budget = 1) {
  const before = cpu(c), next = {...before, pc: taken(row.cc, before.flags) ? row.target : wrap(BigInt(before.pc) + 5n)};
  run(c, unit, budget, next, label, {reason: budget === 1 ? 1 : 3});
}
for (const owner of ['standalone', 'replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries); let oldest;
  for (let bank = 0; bank < 2; bank++) {
    const selected = conditions.slice(bank * 8, bank * 8 + 8), unit = compile(c, `conditions-${bank}`, selected);
    if (bank === 0) oldest = unit;
    for (const row of selected) for (const f of flags) {
      seed(c, row.pc, f); branch(c, unit, row, 'all16 conditions/all128 raw FLAGS, taken low16 versus full false PC'); counts.flag_inputs++;
    }
    if (bank === 0) {
      const row = selected[0]; seed(c, row.pc, trueFlags[0]);
      for (let i = 0; i < 2; i++) {input(c, 0, state({...cpu(c), pc: row.pc})); branch(c, unit, row, 'only EIP rewound, live GPR/FLAGS retained'); counts.repeats++;}
      const current = cpu(c); current.pc = row.pc; current.flags = falseFlags[0]; current.registers = current.registers.map((r, i) => wrap(BigInt(r) + BigInt(0x12340000 + i * 257)));
      input(c, 0, state(current)); branch(c, unit, row, 'explicit current parents and opposite condition FLAGS'); counts.current_inputs++;
      seed(c, row.pc, falseFlags[0]); run(c, unit, 0, cpu(c), 'zero budget neutral', {retired: 0});
      input(c, 96, words([1])); run(c, unit, 1, cpu(c), 'cancel neutral', {reason: 2, retired: 0});
      run(c, unit, 0, cpu(c), 'cancel precedes zero budget', {reason: 2, retired: 0}); input(c, 96, words([0]));
      branch(c, unit, row, 'false full-PC successor then cold lookup without Execute fetch', 2);
      run(c, unit, 1, cpu(c), 'cold entry keeps all returned state', {reason: 3, retired: 0});
      run(c, unit, 0, cpu(c), 'budget before cold lookup', {retired: 0}); counts.controls += 6;
    }
  }
  const continuation = compile(c, 'boundary-live', boundaryLive);
  for (const row of boundaryLive.slice(0, 2)) for (const f of [2, 0x42, 0x443, 0x892]) {
    seed(c, row.pc, f); branch(c, continuation, {...row, cc: 4, target: 2}, 'literal high-boundary/top-wrap taken and false successors are cold, not fetched', 2); counts.boundaries++;
  }
  seed(c, conditions[0].pc);
  if (owner === 'replacement') run(c, oldest, 1, cpu(c), 'old replacement generation neutral InvalidArtifact3', {status: 3, retired: 0});
  else branch(c, oldest, conditions[0], 'older pure unit remains usable'); counts.retained++;
  for (const equal of [true, false]) {
    const r = registers(); r[0] = 0xa1000000 + (equal ? 1 : 0); r[1] = 0xa1010001; seed(c, 0x12341a00, 0x443, r);
    let next = cpu(c); next.pc = 0x12341a03; next.flags = equal ? 0x446 : 0x497;
    run(c, continuation, 1, next, 'live CMP produces current equality/borrow FLAGS');
    next = cpu(c); next.pc = equal ? 0x1800 : 0x12341a08;
    run(c, continuation, 1, next, 'live WORD JZ chooses low taken or full high false successor');
    next = cpu(c); next.pc = 0x1900; next.registers[2] = 0xa1020000 + (equal ? 1 : 2);
    run(c, continuation, 2, next, 'selected successor supplies returned WORD and JMP retains comparison carry', {retired: 2});
    next = cpu(c); next.pc = 0x1903; next.registers[0] = 0xa1000000 + (equal ? 2 : 3); next.flags = equal ? 0x402 : 0x406;
    run(c, continuation, 1, next, 'ADC consumes branch-selected WORD and live borrow without CPU repair');
    next = cpu(c); next.pc = 0x190b; next.registers[3] = 0xa1030000;
    run(c, continuation, 4, next, 'SETB/SETZ consume returned ADC flags and cold tail avoids replay', {reason: 3, retired: 3}); counts.consumers += 5;
  }
  if (c.api) {
    for (const [address, value] of [[0x770b, 0x55], [0x782c, 0xa5], [0x7835, 0xf0]]) {
      const expected = arena(c), helper = record('R3MH', 40, [0, value, 0, 0, 0, 1]); helper.writeUInt16LE(2, 4); expected.set(helper, 100);
      assert.equal(c.api.read8(address), 0); assert.deepEqual(arena(c), expected, 'readonly branch target canary unchanged');
    }
    upload(c, conditions[0].pc, conditions[0].bytes.subarray(0, 1));
    run(c, continuation, 1, cpu(c), 'same-byte executable source mutation neutral CodeInvalidated4', {status: 4, retired: 0}); counts.invalidated++;
    assert.equal(c.api.close(), 0);
  }
}
assert.deepEqual(sources(), sourcePins);
assert.deepEqual(counts, {contexts: 6, runs: 12460, retired: 12460, flag_inputs: 12288, boundaries: 48, repeats: 12, current_inputs: 6,
  controls: 36, consumers: 60, retained: 6, invalidated: 4}); assert.equal(modules.length, 18);
const result = {status: 'ok', engine_sha256: engine.sha256, source_pins: sourcePins, counts, modules, observations,
  limits: ['finite16 exact66 nearJcc forms in six standalone/replacement/resident explicit/entry profiles; arithmetic predicate and signed-modulo oracles plus literal truth/target anchors',
    'all16 conditions/all128 valid FLAGS, eight GPRs and full4612 arena including dirtyFP128/helper retained; pure memory-only standalone or guard-only bound imports, ExitV1',
    'taken low16 target versus false full32 wrapping nextPC; absent/readonly successors are not Execute fetched; live CMP/JZ/WORD-JMP/ADC/SETcc uses returned state without CPU repair',
    'two banks eight blocks/eight instructions each; third bank six blocks/twelve instructions; three captures/context, all code and permissions fixed before capture',
    'native standalone source currentness checks byte retrieval; saved unbound modules have no runtime source guard; bound generation3 and codeinvalidated4 are neutral',
    'selected readonly RAM canaries; no memory operand/helper/SMC execution, exhaustiveISA, performance, segmentation, SDK/browser/game or hostedCI claim']};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2), {flag: 'wx'});
console.log(JSON.stringify({status: 'ok', ...counts, modules: modules.length, output}));
