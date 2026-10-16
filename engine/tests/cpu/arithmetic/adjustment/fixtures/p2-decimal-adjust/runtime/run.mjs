import {detectionGuardName, generatedImports, attachDetectionApi} from '../../../../../../support/wasm/binding.mjs';
import {ARENA_SIZE} from '../../../../../../support/wasm/arena.mjs';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../../../../../../support/wasm/engine.mjs';

const [enginePath, output, root] = process.argv.slice(2), SIZE = ARENA_SIZE;
assert.ok(enginePath && output && root);
const engine = readEngine(enginePath), initial = readFileSync(join(output, 'initial-arena.bin'));
assert.equal(initial.length, SIZE); assert.deepEqual(WebAssembly.Module.imports(engine.module), []);
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const paths = ['engine/src/abi/arena.rs', 'engine/src/cpu/x86/ir.rs', 'engine/src/cpu/x86/decode/profile.rs', 'engine/src/cpu/x86/decode/families.rs',
  'engine/src/cpu/x86/decode/lower.rs',  'engine/src/cpu/x86/decode/decoder.rs', 'engine/src/cpu/x86/decode/instruction.rs', 'engine/src/cpu/dbt/region.rs',
  'engine/src/cpu/dbt/cold.rs', 'engine/src/cpu/dbt/artifact.rs', 'engine/src/cpu/dbt/wasm/abi.rs',
  'engine/src/cpu/dbt/wasm/instructions/integer/arithmetic.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/bits.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/carry_rotates.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/decimal.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/mod.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/arithmetic.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/moves.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/exchange.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/predicates.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/shifts.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/control.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/extended.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/flags.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/mod.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/multiply_divide.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/operands.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/rotates.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/shifts.rs', 'engine/src/cpu/dbt/wasm/emitter/mod.rs', 'engine/src/cpu/dbt/wasm/emitter/module.rs', 'engine/src/cpu/dbt/wasm/emitter/body.rs', 'engine/src/cpu/dbt/wasm/requirements.rs', 'engine/src/cpu/dbt/wasm/imports/wiring.rs', 'engine/src/process/instance.rs', 'engine/src/process/execution/compiled.rs', 'engine/src/process/execution/authority.rs', 'engine/src/process/memory/address_space.rs', 'engine/src/process/memory/scalar.rs', 'engine/src/process/memory/scalar/tests.rs', 'engine/src/process/execution/mod.rs', 'engine/src/process/memory/mod.rs', 'engine/src/process/mod.rs',
  'engine/src/process/execution/resident.rs', 'engine/src/process/wasm/mod.rs', 'engine/src/process/wasm/loading.rs', 'engine/src/abi/image.rs', 'engine/src/process/wasm/execution.rs', 'engine/src/abi/execution.rs', 'engine/src/process/wasm/calls.rs', 'engine/src/process/wasm/memory.rs', 'engine/src/process/wasm/cpu.rs', 'engine/src/memory/space.rs', 'engine/src/memory/space/atomic.rs',
  'engine/tests/cpu/arithmetic/adjustment/native/decimal.rs', 'engine/tests/cpu/arithmetic/native.rs', 'engine/tests/support/cpu/prefix/admission.rs', 'engine/tests/cpu/arithmetic/adjustment/wasm/decimal.rs', 'engine/tests/cpu/arithmetic/wasm.rs', 'engine/tests/support/wasm.rs',
  'engine/tests/cpu/arithmetic/adjustment/fixtures/p2-decimal-adjust/runtime/run.mjs', 'engine/tests/support/wasm/engine.mjs', 'engine/tests/support/wasm/binding.mjs'];
assert.equal(new Set(paths).size, paths.length);
const sources = () => Object.fromEntries(paths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = sources(), modules = [], observations = [], contexts = [];
const counts = {contexts: 0, runs: 0, retired: 0, exhaustive: 0, flag_inputs: 0, anchors: 0,
  repeats: 0, current_inputs: 0, controls: 0, retained: 0, consumers: 0, invalidated: 0, helper_calls: 0};
const allFlags = Array.from({length: 128}, (_, n) => 2 + [1, 4, 16, 64, 128, 1024, 2048]
  .reduce((sum, bit, i) => sum + Math.floor(n / 2 ** i) % 2 * bit, 0));
assert.equal(new Set(allFlags).size, 128); assert.ok(allFlags.every(f => (f & 2) && !(f & ~0xcd7)));
const modulo = (n, size) => (n % size + size) % size;
const parity = n => {let ones = 0; for (let i = 0; i < 8; i++) ones += Math.floor(n / 2 ** i) % 2; return ones % 2 === 0;};
// independent closed-form decimal corrections use the original byte and input flags.
function oracle(kind, a, f) {
  const low = a % 16 > 9 || Math.floor(f / 16) % 2 === 1, high = a > 153 || f % 2 === 1;
  const result = modulo(a + (kind === 'DAA' ? 1 : -1) * (6 * low + 96 * high), 256);
  const carry = high || (kind === 'DAS' && low && a < 6);
  return {result, flags: Math.floor(f / 1024) % 2 * 1024 + 2 + Number(carry) + 16 * low
    + 4 * parity(result) + 64 * (result === 0) + 128 * (result >= 128)};
}
const literals = [
  ['DAA', 0, 2, 0, 0x46], ['DAA', 0, 0x12, 6, 0x16], ['DAA', 0x95, 0x12, 0x9b, 0x92],
  ['DAA', 0xfa, 0x12, 0x60, 0x17], ['DAA', 0x99, 2, 0x99, 0x86], ['DAA', 0x9a, 2, 0, 0x57],
  ['DAA', 0, 3, 0x60, 7], ['DAA', 0xff, 2, 0x65, 0x17],
  ['DAS', 0, 0x12, 0xfa, 0x97], ['DAS', 5, 0x12, 0xff, 0x97], ['DAS', 6, 0x12, 0, 0x56],
  ['DAS', 0x9a, 2, 0x34, 0x13], ['DAS', 0, 3, 0xa0, 0x87], ['DAS', 0x99, 2, 0x99, 0x86],
  ['DAS', 0xa0, 2, 0x40, 3], ['DAS', 0xff, 2, 0x99, 0x97],
];
for (const [kind, a, f, result, expectedFlags] of literals) {
  assert.deepEqual(oracle(kind, a, f + 0xc00), {result, flags: expectedFlags + 0x400}, 'literal correction/borrow/OF-clear anchor');
}
const main = [{kind: 'DAA', pc: 0x1000, bytes: Buffer.from([0x27, 0xeb, 0]), instructions: 2},
  {kind: 'DAS', pc: 0x1020, bytes: Buffer.from([0x2f, 0xeb, 0]), instructions: 2}];
const live = [{kind: 'DAA', pc: 0x1400, bytes: Buffer.from([0x00, 0xd8, 0x27, 0x0f, 0x92, 0xc2, 0x0f, 0x90, 0xc6, 0x80, 0xd1, 0, 0xeb, 0]), instructions: 6},
  {kind: 'DAS', pc: 0x1440, bytes: Buffer.from([0x28, 0xd8, 0x2f, 0x0f, 0x92, 0xc2, 0x0f, 0x90, 0xc6, 0x80, 0xd1, 0, 0xeb, 0]), instructions: 6}];
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
  assert.deepEqual(arena(c), expected, 'declared input preserves the rest of the full arena');
}
function host(c, name, args) {
  const expected = arena(c); assert.equal(c.api[name](...args), 0, name);
  if (name.startsWith('compile_resident')) {const b = arena(c); assert.equal(b.readUInt32LE(140), 1); assert.equal(b.readUInt32LE(144), 24); expected.set(b.subarray(140, 164), 140);}
  assert.deepEqual(arena(c), expected, 'host full arena effect: ' + name);
}
function upload(c, pc, bytes) {input(c, 140, bytes); host(c, 'upload', [pc, bytes.length]);}
const cpu = c => {const b = arena(c); return {registers: Array.from({length: 8}, (_, i) => b.readUInt32LE(16 + i * 4)), pc: b.readUInt32LE(48), flags: b.readUInt32LE(52)};};
const parents = n => Array.from({length: 8}, (_, i) => (0xa100 + i) * 65536 + modulo(0x7e00 + n * (17 + i), 65536));
const setByte = (parent, byte) => Math.floor(parent / 256) * 256 + byte;
function seed(c, pc, f = 0xcd7, r = parents(0)) {
  const b = Buffer.alloc(140); b.set(state({registers: r, pc, flags: f})); b.set(exit(3, 0), 56);
  b.set(record('R3MH', 40, [0, 0xdecafbad, 0, 0, 0, 0]), 100); input(c, 0, b); return cpu(c);
}
function adjusted(s, kind, nextPc) {
  const expected = oracle(kind, s.registers[0] % 256, s.flags), r = [...s.registers];
  r[0] = setByte(r[0], expected.result); return {registers: r, pc: nextPc, flags: expected.flags};
}
function run(c, unit, budget, next, label, {reason = 1, retired = 1, status = 0, category = label} = {}) {
  const before = cpu(c), expected = arena(c);
  if (status === 0) {expected.set(state(next)); expected.set(exit(reason, retired), 56);}
  const observedStatus = unit.run(c.base, c.base + 56, budget, c.base + 96);
  assert.equal(observedStatus, status, label); assert.deepEqual(arena(c), expected, 'complete arena/GPR/AL-high24/FLAGS/helper/dirtyFP: ' + label);
  const after = arena(c), accepted = observedStatus === 0;
  observations.push({context: c.id, category, label, input_pc: before.pc, input_al: before.registers[0] % 256, input_flags: before.flags,
    expected_status: status, observed_status: observedStatus, expected_reason: status === 0 ? reason : null,
    observed_reason: accepted ? after.readUInt32LE(72) : null, expected_retired: status === 0 ? retired : null,
    observed_retired: accepted ? after.readUInt32LE(76) : null, expected_pc: status === 0 ? next.pc : null,
    observed_pc: accepted ? after.readUInt32LE(48) : null, expected_flags: status === 0 ? next.flags : null,
    observed_flags: accepted ? after.readUInt32LE(52) : null, helper_trace: []});
  counts.runs++; if (accepted) counts.retired += after.readUInt32LE(76);
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
  assert.ok(bytes.length <= 65536); const module = new WebAssembly.Module(bytes), guard = detectionGuardName(c.owner === 'resident');
  const imports = generatedImports({bound: !(c.owner === 'standalone'), resident: c.owner === 'resident', helpers: []});
  assert.deepEqual(WebAssembly.Module.imports(module), imports); assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  const file = `module-${modules.length + 1}.wasm`; writeFileSync(join(output, file), bytes, {flag: 'wx'});
  modules.push({context: c.id, owner: c.owner, entries: c.entries, name, blocks: blocks.length, instructions, file, sha256: hash(bytes), imports, exit_version: 1, ...binding});
  return {run: new WebAssembly.Instance(module, {env: {memory: c.memory}, ...(c.api ? {ring3: c.api} : {})}).exports.run};
}
function open(owner, entries) {
  const c = {id: ++counts.contexts, owner, entries, captures: 0};
  if (owner === 'standalone') {c.memory = new WebAssembly.Memory({initial: 1}); c.base = 128; input(c, 0, initial); return c;}
  const instance = new WebAssembly.Instance(engine.module, {}); c.memory = instance.exports.memory; c.api = {};
  for (const name of ['open', 'close', 'arena_ptr', 'map', 'protect', 'upload', 'read8', 'compile', 'compile_entries',
    'compile_resident', 'compile_resident_entries', 'generation', 'module_ptr', 'module_len', 'guard', 'guard_resident']) c.api[name] = instance.exports['ring3_abi_v1_' + name];
 attachDetectionApi(c.api, instance.exports);
  assert.equal(c.api.open(2, c.id, 0x44414441), 0); c.base = c.api.arena_ptr() >>> 0; input(c, 0, initial);
  for (const page of [0x1000, 0x7000]) host(c, 'map', [page, 1, 7]);
  for (const b of [...main, ...live]) upload(c, b.pc, b.bytes);
  upload(c, 0x7010, Buffer.from([0x55, 0xa5, 0x5a, 0xaa])); host(c, 'protect', [0x7000, 1, 1]); return c;
}
for (const owner of ['standalone', 'replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries), unit = compile(c, 'main', main);
  for (const row of main) {
    for (let a = 0; a < 256; a++) for (let af = 0; af < 2; af++) for (let cf = 0; cf < 2; cf++) {
      const f = 0x8c6 + af * 16 + cf + a % 2 * 1024, r = parents(a + af * 256 + cf * 512); r[0] = setByte(r[0], a);
      seed(c, row.pc, f, r); run(c, unit, 1, adjusted(cpu(c), row.kind, row.pc + 1), row.kind, {category: 'exhaustive'}); counts.exhaustive++;
    }
    for (const [i, f] of allFlags.entries()) {
      const a = [0, 5, 6, 9, 10, 0x95, 0x99, 0x9a, 0xfa, 0xff][i % 10], r = parents(i); r[0] = setByte(r[0], a);
      seed(c, row.pc, f, r); run(c, unit, 1, adjusted(cpu(c), row.kind, row.pc + 1), row.kind, {category: 'all128FLAGS'}); counts.flag_inputs++;
    }
    for (const [kind, a, f, result, expectedFlags] of literals.filter(l => l[0] === row.kind)) {
      const r = parents(a); r[0] = setByte(r[0], a); seed(c, row.pc, f + 0xc00, r);
      const next = cpu(c); next.pc++; next.registers[0] = setByte(next.registers[0], result); next.flags = expectedFlags + 0x400;
      run(c, unit, 1, next, kind, {category: 'literal-anchor'}); counts.anchors++;
    }
    const r = parents(37); r[0] = setByte(r[0], row.kind === 'DAA' ? 0x95 : 0); seed(c, row.pc, 0xc12, r);
    for (let i = 0; i < 2; i++) {input(c, 0, state({...cpu(c), pc: row.pc})); run(c, unit, 1, adjusted(cpu(c), row.kind, row.pc + 1), 'only PC rewound; returned AL/AF/CF are current'); counts.repeats++;}
    const current = cpu(c); current.pc = row.pc; current.flags = 0x813; current.registers = parents(731); current.registers[0] = setByte(current.registers[0], 0x06);
    input(c, 0, state(current)); run(c, unit, 1, adjusted(cpu(c), row.kind, row.pc + 1), 'explicit current AL/high24/FLAGS/allparents'); counts.current_inputs++;
    seed(c, row.pc); run(c, unit, 0, cpu(c), 'zero budget neutral', {retired: 0});
    input(c, 96, words([1])); run(c, unit, 1, cpu(c), 'cancel neutral', {reason: 2, retired: 0});
    run(c, unit, 0, cpu(c), 'cancel precedes zero budget', {reason: 2, retired: 0}); input(c, 96, words([0]));
    run(c, unit, 3, adjusted(cpu(c), row.kind, row.pc + 3), 'adjust and JMP retire before cold lookup', {reason: 3, retired: 2});
    run(c, unit, 1, cpu(c), 'cold entry neutral', {reason: 3, retired: 0}); run(c, unit, 0, cpu(c), 'budget before cold lookup', {retired: 0}); counts.controls += 6;
  }
  const continuation = compile(c, 'live', live); seed(c, main[0].pc);
  if (owner === 'replacement') run(c, unit, 1, cpu(c), 'old replacement generation neutral InvalidArtifact3', {status: 3, retired: 0});
  else run(c, unit, 1, adjusted(cpu(c), 'DAA', main[0].pc + 1), 'older unit remains usable'); counts.retained++;
  for (const row of live) {
    const r = parents(0); r[0] = setByte(r[0], row.kind === 'DAA' ? 0x79 : 0x35); r[3] = setByte(r[3], row.kind === 'DAA' ? 0x35 : 0x47); r[1] = setByte(r[1], 0);
    seed(c, row.pc, 0x402, r);
    let next = cpu(c); next.pc += 2; next.registers[0] = setByte(next.registers[0], row.kind === 'DAA' ? 0xae : 0xee); next.flags = row.kind === 'DAA' ? 0xc82 : 0x497;
    run(c, continuation, 1, next, 'live ' + row.kind + ' ordinary byte producer');
    next = cpu(c); next.pc++; next.registers[0] = setByte(next.registers[0], row.kind === 'DAA' ? 0x14 : 0x88); next.flags = row.kind === 'DAA' ? 0x417 : 0x497;
    run(c, continuation, 1, next, 'live ' + row.kind + ' consumes producer AL/AF/CF and clears undefined OF');
    next = cpu(c); next.pc += 6; next.registers[2] = Math.floor(next.registers[2] / 65536) * 65536 + 1;
    run(c, continuation, 2, next, 'live SETB/SETO observe adjusted carry/cleared overflow', {retired: 2});
    next = cpu(c); next.pc += 3; next.registers[1] = setByte(next.registers[1], 1); next.flags = 0x402;
    run(c, continuation, 1, next, 'live ADC consumes adjusted carry without CPU reseeding');
    run(c, continuation, 2, {...cpu(c), pc: row.pc + 14}, 'live cold tail preserves returned state', {reason: 3}); counts.consumers += 5;
  }
  if (c.api) {
    for (const [i, value] of [0x55, 0xa5, 0x5a, 0xaa].entries()) {
      const expected = arena(c), helper = record('R3MH', 40, [0, value, 0, 0, 0, 1]); helper.writeUInt16LE(2, 4); expected.set(helper, 100);
      assert.equal(c.api.read8(0x7010 + i), 0); assert.deepEqual(arena(c), expected, 'selected readonly RAM canaries unchanged');
    }
    upload(c, main[0].pc, main[0].bytes.subarray(0, 1));
    run(c, continuation, 1, cpu(c), 'same-byte RWX source mutation neutral CodeInvalidated4', {status: 4, retired: 0}); counts.invalidated++;
    assert.equal(c.api.close(), 0);
  }
  contexts.push({id: c.id, owner, entries, captures: c.captures, maximum_blocks: 2, maximum_instructions: 12, maximum_mapped_pages: c.api ? 2 : null});
}
assert.deepEqual(sources(), sourcePins);
assert.deepEqual(counts, {contexts: 6, runs: 14098, retired: 14056, exhaustive: 12288, flag_inputs: 1536, anchors: 96,
  repeats: 24, current_inputs: 12, controls: 72, retained: 6, consumers: 60, invalidated: 4, helper_calls: 0}); assert.equal(modules.length, 12);
const result = {status: 'ok', engine_sha256: engine.sha256, source_pins: sourcePins, counts, contexts, modules, observations,
  literal_anchors: literals, limits: ['finite exact27/2F pure six-profile proof; exhaustiveAL256×AF/CF4 per form/profile plus all128 admittedFLAGS at selected thresholds',
    'independent closed-form corrections preserve DAS low borrow and use original AL/CF; literal anchors distinguish DAA high-false CF reset and original-byte high conditions',
    'OF deterministic clear is Ring3 undefined-flag policy; SF/ZF/PF derive from final byte, DF/bit1 and EAX high24/allotherGPR/helper/dirtyFP128 retained',
    'complete arena equality and selected bound readonly RAM canaries; no guest memory operand, generated helper, arbitrary-host rollback or wholeRAM snapshot claim',
    'live real byteADD/SUB→adjust→SETB/SETO→ADC has no CPU reseeding; observed per-row status/reason/retirement/PC/FLAGS are persisted and neutral3/4 observations are null',
    'two blocks/four instructions main and two blocks/twelve instructions live, two captures/context; allcode/protections before captures; default8/64/65536/sixcapture unchanged',
    'unbound saved modules have no runtime source guard; bound replacement generation3 and same-byte RWX source CodeInvalidated4 are neutral; helper trace empty by exact pure imports',
    'no hardware completeness, prefixed aliases, performance, SDK/browser/game, dispatcher expansion or hostedCI claim']};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2), {flag: 'wx'});
console.log(JSON.stringify({status: 'ok', ...counts, modules: modules.length, output}));
