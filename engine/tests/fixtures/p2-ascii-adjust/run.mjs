import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../support/engine.mjs';

const [enginePath, output, root] = process.argv.slice(2), SIZE = 4364;
assert.ok(enginePath && output && root);
const engine = readEngine(enginePath), initial = readFileSync(join(output, 'initial-arena.bin'));
assert.equal(initial.length, SIZE); assert.deepEqual(WebAssembly.Module.imports(engine.module), []);
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const paths = ['engine/src/abi/arena.rs', 'engine/src/cpu/x86/ir.rs', 'engine/src/cpu/x86/decode/profile.rs',
  'engine/src/cpu/x86/decode/lower.rs', 'engine/src/cpu/x86/decode/decoder.rs', 'engine/src/cpu/dbt/region.rs',
  'engine/src/cpu/dbt/cold.rs', 'engine/src/cpu/dbt/artifact.rs', 'engine/src/cpu/dbt/wasm/abi.rs',
  'engine/src/cpu/dbt/wasm/integer.rs', 'engine/src/cpu/dbt/wasm/emitter.rs', 'engine/src/process/instance.rs',
  'engine/src/process/resident.rs', 'engine/src/process/wasm.rs', 'engine/src/memory/space.rs',
  'engine/tests/cpu_decimal_adjust.rs', 'engine/tests/cpu_ascii_adjust_wasm.rs',
  'engine/tests/fixtures/p2-ascii-adjust/run.mjs', 'engine/tests/fixtures/support/engine.mjs'];
assert.equal(new Set(paths).size, paths.length);
const sources = () => Object.fromEntries(paths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = sources(), modules = [], observations = [], contexts = [];
const counts = {contexts: 0, runs: 0, retired: 0, exhaustive: 0, flag_inputs: 0, anchors: 0,
  repeats: 0, current_inputs: 0, controls: 0, retained: 0, consumers: 0, invalidated: 0, helper_calls: 0};
const allFlags = Array.from({length: 128}, (_, n) => 2 + [1, 4, 16, 64, 128, 1024, 2048]
  .reduce((sum, bit, i) => sum + Math.floor(n / 2 ** i) % 2 * bit, 0));
assert.equal(new Set(allFlags).size, 128); assert.ok(allFlags.every(f => (f & 2) && !(f & ~0xcd7)));
const modulo = (n, size) => (n % size + size) % size;
// independent byte carry/borrow arithmetic models AH separately from the AL correction.
function oracle(kind, ax, f) {
  const al = ax % 256, ah = Math.floor(ax / 256), low = al % 16 > 9 || Math.floor(f / 16) % 2 === 1;
  const direction = kind === 'AAA' ? 1 : -1, corrected = al + direction * 6 * low;
  const nextAh = modulo(ah + direction * low + Math.floor(corrected / 256), 256);
  return {result: nextAh * 256 + modulo(corrected, 16), flags: Math.floor(f / 1024) % 2 * 1024 + 2 + 17 * low};
}
const literals = [
  ['AAA', 0x0000, 2, 0x0000, 2], ['AAA', 0x0000, 0x12, 0x0106, 0x13],
  ['AAA', 0x00fa, 2, 0x0200, 0x13], ['AAA', 0xfffa, 2, 0x0100, 0x13],
  ['AAA', 0x7ffa, 2, 0x8100, 0x13], ['AAA', 0x80fa, 2, 0x8200, 0x13],
  ['AAA', 0x1295, 2, 0x1205, 2], ['AAA', 0x12f9, 3, 0x1209, 2],
  ['AAS', 0x0000, 0x12, 0xfe0a, 0x13], ['AAS', 0x0100, 0x12, 0xff0a, 0x13],
  ['AAS', 0x0006, 0x12, 0xff00, 0x13], ['AAS', 0x0005, 0x12, 0xfe0f, 0x13],
  ['AAS', 0x8000, 0x12, 0x7e0a, 0x13], ['AAS', 0x1200, 0x12, 0x100a, 0x13],
  ['AAS', 0x12f9, 3, 0x1209, 2], ['AAS', 0x12fa, 2, 0x1104, 0x13],
];
for (const [kind, ax, f, result, expectedFlags] of literals) {
  assert.deepEqual(oracle(kind, ax, f + 0xc00), {result, flags: expectedFlags + 0x400}, 'literal fullAX carry/borrow/undefined-flags anchor');
}
const main = [{kind: 'AAA', pc: 0x1000, bytes: Buffer.from([0x37, 0xeb, 0]), instructions: 2},
  {kind: 'AAS', pc: 0x1020, bytes: Buffer.from([0x3f, 0xeb, 0]), instructions: 2}];
const live = [{kind: 'AAA', pc: 0x1400, bytes: Buffer.from([0x00, 0xd8, 0x37, 0x0f, 0x92, 0xc2, 0x0f, 0x90, 0xc6, 0x80, 0xd1, 0, 0xeb, 0]), instructions: 6},
  {kind: 'AAS', pc: 0x1440, bytes: Buffer.from([0x28, 0xd8, 0x3f, 0x0f, 0x92, 0xc2, 0x0f, 0x90, 0xc6, 0x80, 0xd1, 0, 0xeb, 0]), instructions: 6}];
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
const setWord = (parent, word) => Math.floor(parent / 65536) * 65536 + word;
function seed(c, pc, f = 0xcd7, r = parents(0)) {
  const b = Buffer.alloc(140); b.set(state({registers: r, pc, flags: f})); b.set(exit(3, 0), 56);
  b.set(record('R3MH', 40, [0, 0xdecafbad, 0, 0, 0, 0]), 100); input(c, 0, b); return cpu(c);
}
function adjusted(s, kind, nextPc) {
  const expected = oracle(kind, s.registers[0] % 65536, s.flags), r = [...s.registers];
  r[0] = setWord(r[0], expected.result); return {registers: r, pc: nextPc, flags: expected.flags};
}
function run(c, unit, budget, next, label, {reason = 1, retired = 1, status = 0, category = label} = {}) {
  const before = cpu(c), expected = arena(c);
  if (status === 0) {expected.set(state(next)); expected.set(exit(reason, retired), 56);}
  const observedStatus = unit.run(c.base, c.base + 56, budget, c.base + 96);
  assert.equal(observedStatus, status, label); assert.deepEqual(arena(c), expected, 'full4364/GPR/AX-high16/FLAGS/helper/dirtyFP: ' + label);
  const after = arena(c), accepted = observedStatus === 0;
  observations.push({context: c.id, category, label, input_pc: before.pc, input_ax: before.registers[0] % 65536, input_flags: before.flags,
    expected_status: status, observed_status: observedStatus, expected_reason: status === 0 ? reason : null,
    observed_reason: accepted ? after.readUInt32LE(72) : null, expected_retired: status === 0 ? retired : null,
    observed_retired: accepted ? after.readUInt32LE(76) : null, expected_pc: status === 0 ? next.pc : null,
    observed_pc: accepted ? after.readUInt32LE(48) : null, expected_flags: status === 0 ? next.flags : null,
    observed_flags: accepted ? after.readUInt32LE(52) : null, expected_ax: status === 0 ? next.registers[0] % 65536 : null,
    observed_ax: accepted ? after.readUInt32LE(16) % 65536 : null, helper_trace: []});
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
  assert.ok(bytes.length <= 65536); const module = new WebAssembly.Module(bytes), guard = c.owner === 'resident' ? 'guard_resident' : 'guard';
  const imports = [{module: 'env', name: 'memory', kind: 'memory'}, ...(c.owner === 'standalone' ? [] : [{module: 'ring3', name: guard, kind: 'function'}])];
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
  assert.equal(c.api.open(2, c.id, 0x41414153), 0); c.base = c.api.arena_ptr() >>> 0; input(c, 0, initial);
  for (const page of [0x1000, 0x7000]) host(c, 'map', [page, 1, 7]);
  for (const b of [...main, ...live]) upload(c, b.pc, b.bytes);
  upload(c, 0x7010, Buffer.from([0x55, 0xa5, 0x5a, 0xaa])); host(c, 'protect', [0x7000, 1, 1]); return c;
}
for (const owner of ['standalone', 'replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries), unit = compile(c, 'main', main);
  for (const row of main) {
    for (let a = 0; a < 256; a++) for (let af = 0; af < 2; af++) for (const ah of [0, 255]) {
      const f = 0x8c6 + af * 16 + a % 2 + Math.floor(a / 2) % 2 * 1024, r = parents(a + af * 256 + ah); r[0] = setWord(r[0], ah * 256 + a);
      seed(c, row.pc, f, r); run(c, unit, 1, adjusted(cpu(c), row.kind, row.pc + 1), row.kind, {category: 'exhaustive'}); counts.exhaustive++;
    }
    for (const [i, f] of allFlags.entries()) {
      const pair = Math.floor(i / 2), a = [0, 5, 6, 9, 10, 0x95, 0x99, 0x9a, 0xfa, 0xff][pair % 10];
      const ah = [0, 255, 0x7f, 0x80][pair % 4], r = parents(pair); r[0] = setWord(r[0], ah * 256 + a);
      seed(c, row.pc, f, r); run(c, unit, 1, adjusted(cpu(c), row.kind, row.pc + 1), row.kind, {category: 'all128FLAGS'}); counts.flag_inputs++;
    }
    for (const [kind, ax, f, result, expectedFlags] of literals.filter(l => l[0] === row.kind)) {
      const r = parents(ax); r[0] = setWord(r[0], ax); seed(c, row.pc, f + 0xc00, r);
      const next = cpu(c); next.pc++; next.registers[0] = setWord(next.registers[0], result); next.flags = expectedFlags + 0x400;
      run(c, unit, 1, next, kind, {category: 'literal-anchor'}); counts.anchors++;
    }
    const r = parents(37); r[0] = setWord(r[0], row.kind === 'AAA' ? 0x0095 : 0x0000); seed(c, row.pc, 0xc12, r);
    for (let i = 0; i < 2; i++) {input(c, 0, state({...cpu(c), pc: row.pc})); run(c, unit, 1, adjusted(cpu(c), row.kind, row.pc + 1), 'only PC rewound; returned AX/AF/CF are current'); counts.repeats++;}
    const current = cpu(c); current.pc = row.pc; current.flags = 0x813; current.registers = parents(731); current.registers[0] = setWord(current.registers[0], 0x8006);
    input(c, 0, state(current)); run(c, unit, 1, adjusted(cpu(c), row.kind, row.pc + 1), 'explicit current AX/high16/FLAGS/allparents'); counts.current_inputs++;
    seed(c, row.pc); run(c, unit, 0, cpu(c), 'zero budget neutral', {retired: 0});
    input(c, 96, words([1])); run(c, unit, 1, cpu(c), 'cancel neutral', {reason: 2, retired: 0});
    run(c, unit, 0, cpu(c), 'cancel precedes zero budget', {reason: 2, retired: 0}); input(c, 96, words([0]));
    run(c, unit, 3, adjusted(cpu(c), row.kind, row.pc + 3), 'adjust and JMP retire before cold lookup', {reason: 3, retired: 2});
    run(c, unit, 1, cpu(c), 'cold entry neutral', {reason: 3, retired: 0}); run(c, unit, 0, cpu(c), 'budget before cold lookup', {retired: 0}); counts.controls += 6;
  }
  const continuation = compile(c, 'live', live); seed(c, main[0].pc);
  if (owner === 'replacement') run(c, unit, 1, cpu(c), 'old replacement generation neutral InvalidArtifact3', {status: 3, retired: 0});
  else run(c, unit, 1, adjusted(cpu(c), 'AAA', main[0].pc + 1), 'older unit remains usable'); counts.retained++;
  for (const row of live) {
    const r = parents(0); r[0] = setWord(r[0], row.kind === 'AAA' ? 0x0009 : 0x0003); r[3] = setByte(r[3], row.kind === 'AAA' ? 0x09 : 0x07); r[1] = setByte(r[1], 0);
    seed(c, row.pc, 0x402, r);
    let next = cpu(c); next.pc += 2; next.registers[0] = setByte(next.registers[0], row.kind === 'AAA' ? 0x12 : 0xfc); next.flags = row.kind === 'AAA' ? 0x416 : 0x497;
    run(c, continuation, 1, next, 'live ' + row.kind + ' ordinary byte producer');
    next = cpu(c); next.pc++; next.registers[0] = setWord(next.registers[0], row.kind === 'AAA' ? 0x0108 : 0xff06); next.flags = 0x413;
    run(c, continuation, 1, next, 'live ' + row.kind + ' consumes producer AL/AF and clears four undefined flags');
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
  literal_anchors: literals, limits: ['finite exact37/3F pure six-profile proof; exhaustiveAL256×AF2×AH00/FF per form/profile plus all128 admittedFLAGS at selected thresholds',
    'independent original-AH and AL-correction carry/borrow oracle; literal fullAX carry/borrow/wrap and opposite incomingCF pairs distinguish operand capture and CF independence',
    'CF=AF=adjustmentcondition; SF/ZF/PF/OF deterministic clear is Ring3 undefined-flag policy, including ALzero withZF0; DF/bit1 and EAX high16/allotherGPR/helper/dirtyFP128 retained',
    'full4364 arena equality and selected bound readonly RAM canaries; no guest memory operand, generated helper, arbitrary-host rollback or wholeRAM snapshot claim',
    'live real byteADD/SUB→adjust→SETB/SETO→ADC has no CPU reseeding; observed per-row status/reason/retirement/PC/FLAGS/AX are persisted and neutral3/4 observations are null',
    'two blocks/four instructions main and two blocks/twelve instructions live, two captures/context; allcode/protections before captures; default8/64/65536/sixcapture unchanged',
    'unbound saved modules have no runtime source guard; bound replacement generation3 and same-byte RWX source CodeInvalidated4 are neutral; empty guest-memory-helper trace is structural import absence, not guard-call telemetry',
    'selected AH values rather than exhaustive wholeAX; no hardware completeness, prefixed aliases, performance, SDK/browser/game, dispatcher expansion or hostedCI claim']};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2), {flag: 'wx'});
console.log(JSON.stringify({status: 'ok', ...counts, modules: modules.length, output}));
