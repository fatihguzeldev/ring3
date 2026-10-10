import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../support/engine.mjs';

const [enginePath, output, root] = process.argv.slice(2), SIZE = 4364;
assert.ok(enginePath && output && root);
const engine = readEngine(enginePath), initial = readFileSync(join(output, 'initial-arena.bin'));
assert.equal(initial.length, SIZE); assert.deepEqual(WebAssembly.Module.imports(engine.module), []);
const hash = b => createHash('sha256').update(b).digest('hex');
const paths = ['engine/src/abi/arena.rs', 'engine/src/cpu/x86/decode/profile.rs', 'engine/src/cpu/x86/decode/flow.rs',
  'engine/src/cpu/x86/decode/decoder.rs', 'engine/src/cpu/dbt/region.rs', 'engine/src/cpu/dbt/cold.rs',
  'engine/src/cpu/dbt/artifact.rs', 'engine/src/cpu/dbt/wasm/abi.rs', 'engine/src/cpu/dbt/wasm/integer.rs',
  'engine/src/cpu/dbt/wasm/emitter.rs', 'engine/src/process/instance.rs', 'engine/src/process/resident.rs',
  'engine/src/process/wasm.rs', 'engine/src/memory/space.rs', 'engine/tests/cpu_word_jump.rs',
  'engine/tests/cpu_word_jump_wasm.rs', 'engine/tests/fixtures/p2-word-jump/run.mjs', 'engine/tests/fixtures/support/engine.mjs'];
assert.equal(new Set(paths).size, paths.length);
const sources = () => Object.fromEntries(paths.map(p => [p, hash(readFileSync(join(root, p)))]));
const sourcePins = sources(), modules = [], observations = [];
const counts = {contexts: 0, runs: 0, retired: 0, flag_inputs: 0, anchors: 0, repeats: 0, current_inputs: 0,
  controls: 0, consumers: 0, retained: 0, invalidated: 0};
const wrap = n => Number((BigInt(n) % 4294967296n + 4294967296n) % 4294967296n);
const target = (pc, raw) => Number((BigInt(pc) + 4n + BigInt(raw < 32768 ? raw : raw - 65536) + 4294967296n) % 65536n);
const flags = Array.from({length: 128}, (_, n) => 2 + [1, 4, 16, 64, 128, 1024, 2048]
  .reduce((sum, bit, i) => sum + Math.floor(n / 2 ** i) % 2 * bit, 0));
assert.equal(new Set(flags).size, 128); assert.ok(flags.every(n => (n & 2) && !(n & ~0xcd7)));
const rows = [[0x1000, 0, 0x1004], [0x1020, 0x7fff, 0x9023], [0x1040, 0x8000, 0x9044],
  [0x1060, 0xffff, 0x1063], [0x1080, 0xf066, 0xea], [0x10a0, 0x6766, 0x780a],
  [0x1234fffc, 2, 2], [0xfffffffc, 2, 2]];
const jumps = rows.map(([pc, raw, expected], i) => {
  assert.equal(target(pc, raw), expected, 'independent signed rel16/modulo literal');
  const bytes = readFileSync(join(output, `jumps-${i}.x86`));
  assert.deepEqual(bytes, Buffer.from([0x66, 0xe9, raw % 256, Math.floor(raw / 256)]));
  return {pc, raw, target: expected, bytes, instructions: 1};
});
assert.equal(wrap(0x1234fffc + 4), 0x12350000); assert.equal(wrap(0xfffffffc + 4), 0);
const live = [[0x1400, [0x66, 0xe9, 0x1c, 0], 1],
  [0x1420, [0x66, 0x11, 0xd0, 0x0f, 0x90, 0xc3, 0x0f, 0x92, 0xc7, 0x66, 0xe9, 0xc3, 0], 4],
  [0x14f0, [0x66, 0xb9, 0x44, 0x33, 0xeb, 0], 2]].map(([pc, literal, instructions], i) => {
  const bytes = readFileSync(join(output, `live-${i}.x86`)); assert.deepEqual(bytes, Buffer.from(literal)); return {pc, bytes, instructions};
});
assert.deepEqual(JSON.parse(readFileSync(join(output, 'standalone-currentness.json'))),
  {execute_only_units: 4, protection_rejects_retrieval: true, same_byte_probe_units: 1, same_byte_write_rejects_retrieval: true});
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
  assert.deepEqual(arena(c), expected, 'full4364 GPR/FLAGS/EIP/exit/helper/dirtyFP: ' + label);
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
  assert.equal(c.api.open(4, c.id, 0x574a4d50), 0); c.base = c.api.arena_ptr() >>> 0; input(c, 0, initial);
  for (const page of [0x1000, 0x1234f000, 0xfffff000, 0x7000]) host(c, 'map', [page, 1, 7]);
  for (const b of [...jumps, ...live]) upload(c, b.pc, b.bytes);
  upload(c, 0x780a, Buffer.from([0x55, 0xa5])); host(c, 'protect', [0x7000, 1, 1]);
  return c;
}
for (const owner of ['standalone', 'replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries), unit = compile(c, 'jumps', jumps);
  for (const f of flags) {seed(c, jumps[0].pc, f); run(c, unit, 1, {...cpu(c), pc: jumps[0].target}, 'all128 valid FLAGS retained'); counts.flag_inputs++;}
  for (const row of jumps) for (const f of [2, 0x443, 0x892, 0xcd7]) {
    seed(c, row.pc, f); run(c, unit, 2, {...cpu(c), pc: row.target}, 'literal signed/high/wrapped target, no target Execute fetch', {reason: 3}); counts.anchors++;
  }
  seed(c, jumps[0].pc);
  for (let i = 0; i < 2; i++) {input(c, 0, state({...cpu(c), pc: jumps[0].pc})); run(c, unit, 1, {...cpu(c), pc: jumps[0].target}, 'only EIP rewound, returned GPR/FLAGS retained'); counts.repeats++;}
  const current = cpu(c); current.pc = jumps[0].pc; current.flags = 0x892; current.registers = current.registers.map((r, i) => wrap(BigInt(r) + BigInt(0x12340000 + i * 257)));
  input(c, 0, state(current)); run(c, unit, 1, {...cpu(c), pc: jumps[0].target}, 'explicit current parents and FLAGS'); counts.current_inputs++;
  seed(c, jumps[0].pc); run(c, unit, 0, cpu(c), 'zero budget neutral', {retired: 0});
  input(c, 96, words([1])); run(c, unit, 1, cpu(c), 'cancel neutral', {reason: 2, retired: 0});
  run(c, unit, 0, cpu(c), 'cancel precedes zero budget', {reason: 2, retired: 0}); input(c, 96, words([0]));
  run(c, unit, 2, {...cpu(c), pc: jumps[0].target}, 'JMP then cold lookup', {reason: 3});
  run(c, unit, 1, cpu(c), 'cold entry keeps all returned state', {reason: 3, retired: 0});
  run(c, unit, 0, cpu(c), 'budget before cold lookup', {retired: 0}); counts.controls += 6;
  const continuation = compile(c, 'live', live); seed(c, jumps[0].pc);
  if (owner === 'replacement') run(c, unit, 1, cpu(c), 'old replacement generation neutral InvalidArtifact3', {status: 3, retired: 0});
  else run(c, unit, 1, {...cpu(c), pc: jumps[0].target}, 'older pure unit remains usable'); counts.retained++;
  const r = registers(); r[0] = 0xa1007fff; r[2] = 0xa1020000; seed(c, 0x1400, 0x443, r);
  run(c, continuation, 1, {...cpu(c), pc: 0x1420}, 'live WORD JMP retains incoming carry');
  let next = cpu(c); next.pc = 0x1423; next.registers[0] = 0xa1008000; next.flags = 0xc96;
  run(c, continuation, 1, next, 'ADC consumes preserved carry without CPU reseeding');
  next = cpu(c); next.pc = 0x1429; next.registers[3] = 0xa1030001;
  run(c, continuation, 2, next, 'SETO/SETB observe returned arithmetic flags', {retired: 2});
  next = cpu(c); next.pc = 0x14f6; next.registers[1] = 0xa1013344;
  run(c, continuation, 4, next, 'second WORD JMP reaches current WORD consumer then cold tail', {reason: 3, retired: 3}); counts.consumers += 4;
  if (c.api) {
    for (const [i, value] of [0x55, 0xa5].entries()) {
      const expected = arena(c), helper = record('R3MH', 40, [0, value, 0, 0, 0, 1]); helper.writeUInt16LE(2, 4); expected.set(helper, 100);
      assert.equal(c.api.read8(0x780a + i), 0); assert.deepEqual(arena(c), expected, 'readonly unmapped-execution target canary unchanged');
    }
    upload(c, jumps[0].pc, jumps[0].bytes.subarray(0, 1));
    run(c, continuation, 1, cpu(c), 'same-byte executable source mutation neutral CodeInvalidated4', {status: 4, retired: 0}); counts.invalidated++;
    assert.equal(c.api.close(), 0);
  }
}
assert.deepEqual(sources(), sourcePins);
assert.deepEqual(counts, {contexts: 6, runs: 1048, retired: 1030, flag_inputs: 768, anchors: 192, repeats: 12, current_inputs: 6,
  controls: 36, consumers: 24, retained: 6, invalidated: 4}); assert.equal(modules.length, 12);
const result = {status: 'ok', engine_sha256: engine.sha256, source_pins: sourcePins, counts, modules, observations,
  limits: ['finite exact66E9 in six standalone/replacement/resident explicit/entry profiles; eight independent signed/high/wrap target literals',
    'all128 valid FLAGS, eight GPRs and full4364 arena including dirtyFP128/helper retained; pure memory-only standalone or guard-only bound imports and ExitV1',
    'literal target2 absent and target780a readonly do not require Execute fetch; live pure WORD jumps/carry/arithmetic/SETcc consumers have no CPU reseeding',
    'eight blocks/eight instructions initial and three blocks/seven instructions live, two captures/context; fixed code/protections precede captures',
    'native standalone source currentness checks byte retrieval; saved unbound modules have no runtime source guard; bound generation3 and codeinvalidated4 are neutral',
    'selected readonly RAM canaries; no memory operand/helper/SMC execution, exhaustiveISA, performance, segmentation, SDK/browser/game or hostedCI claim']};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2), {flag: 'wx'});
console.log(JSON.stringify({status: 'ok', ...counts, modules: modules.length, output}));
