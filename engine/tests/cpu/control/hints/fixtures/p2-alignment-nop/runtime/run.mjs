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
const hash = b => createHash('sha256').update(b).digest('hex');
const paths = ['engine/src/abi/arena.rs', 'engine/src/cpu/x86/decode/profile.rs', 'engine/src/cpu/x86/decode/families.rs', 'engine/src/cpu/x86/decode/lower.rs', 'engine/src/cpu/x86/decode/families.rs',
  'engine/src/cpu/x86/decode/decoder.rs', 'engine/src/cpu/x86/decode/instruction.rs', 'engine/src/cpu/dbt/region.rs', 'engine/src/cpu/dbt/cold.rs',
  'engine/src/cpu/dbt/artifact.rs', 'engine/src/cpu/dbt/wasm/abi.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/arithmetic.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/bits.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/carry_rotates.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/decimal.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/mod.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/arithmetic.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/moves.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/exchange.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/predicates.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/shifts.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/control.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/extended.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/flags.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/mod.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/multiply_divide.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/operands.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/rotates.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/shifts.rs',
  'engine/src/cpu/dbt/wasm/emitter/mod.rs', 'engine/src/cpu/dbt/wasm/emitter/module.rs', 'engine/src/cpu/dbt/wasm/emitter/body.rs', 'engine/src/cpu/dbt/wasm/requirements.rs', 'engine/src/cpu/dbt/wasm/imports/wiring.rs', 'engine/src/process/instance.rs', 'engine/src/process/execution/compiled.rs', 'engine/src/process/execution/authority.rs', 'engine/src/process/memory/address_space.rs', 'engine/src/process/memory/scalar.rs', 'engine/src/process/memory/scalar/tests.rs', 'engine/src/process/execution/mod.rs', 'engine/src/process/memory/mod.rs', 'engine/src/process/mod.rs', 'engine/src/process/execution/resident.rs', 'engine/src/process/wasm/mod.rs', 'engine/src/process/wasm/loading.rs', 'engine/src/abi/image.rs', 'engine/src/process/wasm/execution.rs', 'engine/src/abi/execution.rs', 'engine/src/process/wasm/calls.rs', 'engine/src/process/wasm/memory.rs', 'engine/src/process/wasm/cpu.rs', 'engine/src/memory/space.rs', 'engine/src/memory/space/atomic.rs', 'engine/tests/cpu/control/hints/native/nop.rs', 'engine/tests/cpu/control/native.rs', 'engine/tests/support/cpu/prefix/admission.rs', 'engine/tests/cpu/control/hints/wasm/nop.rs', 'engine/tests/cpu/control/wasm.rs', 'engine/tests/support/wasm.rs', 'engine/tests/cpu/control/hints/fixtures/p2-alignment-nop/runtime/run.mjs', 'engine/tests/support/wasm/engine.mjs', 'engine/tests/support/wasm/binding.mjs'];
paths.push('engine/tests/support/wasm/arena.mjs', 'engine/src/cpu/x86/environment.rs',
  'engine/src/cpu/x86/capabilities.rs');
assert.equal(new Set(paths).size, paths.length);
const sources = () => Object.fromEntries(paths.map(p => [p, hash(readFileSync(join(root, p)))]));
const sourcePins = sources(), modules = [], observations = [], contexts = [];
const counts = {contexts: 0, runs: 0, retired: 0, flag_inputs: 0, apparent_addresses: 0,
  repeats: 0, current_inputs: 0, controls: 0, data_currentness: 0, retained: 0, consumers: 0, invalidated: 0};
const wrap = n => Number((BigInt(n) % 4294967296n + 4294967296n) % 4294967296n);
const flags = Array.from({length: 128}, (_, n) => 2 + [1, 4, 16, 64, 128, 1024, 2048]
  .reduce((sum, bit, i) => sum + Math.floor(n / 2 ** i) % 2 * bit, 0));
assert.equal(new Set(flags).size, 128); assert.ok(flags.every(n => (n & 2) && !(n & ~0xcd7)));
const nops = [[0x1000, [0x66, 0x0f, 0x1f, 0x44, 0, 0]],
  [0x1020, [0x66, 0x0f, 0x1f, 0x84, 0, 0, 0, 0, 0]],
  [0x1040, [0x66, 0x2e, 0x0f, 0x1f, 0x84, 0, 0, 0, 0, 0]]].map(([pc, literal], i) => {
  const bytes = readFileSync(join(output, `nops-${i}.x86`));
  assert.deepEqual(bytes, Buffer.from([...literal, 0xeb, 0]));
  return {pc, bytes, length: literal.length, instructions: 2};
});
const live = [{pc: 0x1400, bytes: readFileSync(join(output, 'live-0.x86')), instructions: 6}];
const endbr = [
  [0x1500, [0xf3, 0x0f, 0x1e, 0xfb]],
  [0x1520, [0xf3, 0x0f, 0x1e, 0xfa]],
].map(([pc, literal], index) => {
  const bytes = readFileSync(join(output, `endbr-${index}.x86`));
  assert.deepEqual(bytes, Buffer.from([...literal, 0xeb, 0]));
  return { pc, bytes, length: 4, instructions: 2 };
});
assert.deepEqual(live[0].bytes, Buffer.from([0x04, 1, 0x66, 0x2e, 0x0f, 0x1f, 0x84, 0, 0, 0, 0, 0,
  0x66, 0x11, 0xd0, 0x0f, 0x92, 0xc3, 0x0f, 0x90, 0xc7, 0xeb, 0]));
assert.deepEqual(JSON.parse(readFileSync(join(output, 'standalone-currentness.json'))),
  {execute_only_units: 4, protection_rejects_retrieval: true, same_byte_final_displacement_rejects_retrieval: true});
const apparent = [[0, 0], [0x4800, 0x9000], [0x70000000, 0xe0000000], [0x7fffffff, 0xfffffffe], [0x3800, 0x7000]];
for (const [eax, address] of apparent) assert.equal(wrap(BigInt(eax) * 2n), address);
const words = values => {const b = Buffer.alloc(values.length * 4); values.forEach((v, i) => b.writeUInt32LE(v >>> 0, i * 4)); return b;};
function record(magic, size, fields) {
  const b = Buffer.alloc(size); b.write(magic); b.writeUInt16LE(1, 4); b.writeUInt16LE(1, 6); b.writeUInt32LE(size, 8);
  fields.forEach((v, i) => b.writeUInt32LE(v >>> 0, 16 + i * 4)); return b;
}
const state = s => record('R3ST', 56, [...s.registers, s.pc, s.flags]);
const exit = (reason, retired) => record('R3EX', 40, [reason, retired, 0, 0, 0, 0]);
const arena = c => Buffer.from(new Uint8Array(c.memory.buffer, c.base, SIZE));
const cpu = c => {const b = arena(c); return {registers: Array.from({length: 8}, (_, i) => b.readUInt32LE(16 + i * 4)), pc: b.readUInt32LE(48), flags: b.readUInt32LE(52)};};
const registers = () => Array.from({length: 8}, (_, i) => (0xa100 + i) * 65536 + 0x1234 + i);
function input(c, offset, bytes) {
  const expected = arena(c); expected.set(bytes, offset); new Uint8Array(c.memory.buffer).set(bytes, c.base + offset);
  assert.deepEqual(arena(c), expected, 'declared input preserves the remaining full arena');
}
function host(c, name, args) {
  const expected = arena(c); assert.equal(c.api[name](...args), 0, name);
  if (name.startsWith('compile_resident')) {
    const b = arena(c); assert.equal(b.readUInt32LE(140), 1); assert.equal(b.readUInt32LE(144), 24); expected.set(b.subarray(140, 164), 140);
  }
  assert.deepEqual(arena(c), expected, 'host full arena effect: ' + name);
}
function upload(c, pc, bytes) {input(c, 140, bytes); host(c, 'upload', [pc, bytes.length]);}
function seed(c, pc, f = 0xcd7, r = registers()) {
  const b = Buffer.alloc(140); b.set(state({registers: r, pc, flags: f})); b.set(exit(3, 0), 56);
  b.set(record('R3MH', 40, [0, 0xdecafbad, 0, 0, 0, 0]), 100); input(c, 0, b); return cpu(c);
}
function run(c, unit, budget, next, label, {reason = 1, retired = 1, status = 0} = {}) {
  const expected = arena(c); if (status === 0) {expected.set(state(next)); expected.set(exit(reason, retired), 56);}
  const observedStatus = unit.run(c.base, c.base + 56, budget, c.base + 96);
  assert.equal(observedStatus, status, label);
  assert.deepEqual(arena(c), expected, 'complete arena GPR/FLAGS/EIP/exit/helper/dirtyFP: ' + label);
  const after = arena(c); observations.push({context: c.id, label, expected_status: status, observed_status: observedStatus,
    expected_reason: status === 0 ? reason : null, observed_reason: status === 0 ? after.readUInt32LE(72) : null,
    expected_retired: status === 0 ? retired : null, observed_retired: status === 0 ? after.readUInt32LE(76) : null,
    expected_pc: status === 0 ? next.pc : null, observed_pc: status === 0 ? after.readUInt32LE(48) : null,
    expected_flags: status === 0 ? next.flags : null, observed_flags: status === 0 ? after.readUInt32LE(52) : null});
  counts.runs++; if (status === 0) counts.retired += after.readUInt32LE(76);
}
function compile(c, name, blocks) {
  const instructions = blocks.reduce((sum, b) => sum + b.instructions, 0);
  assert.ok(blocks.length > 0 && blocks.length <= 8 && instructions <= 64); assert.ok(++c.captures <= 6);
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
  const instance = new WebAssembly.Instance(module, {env: {memory: c.memory}, ...(c.api ? {ring3: c.api} : {})});
  assert.equal(instance.exports.run.length, 4);
  const file = `module-${modules.length + 1}.wasm`; writeFileSync(join(output, file), bytes, {flag: 'wx'});
  modules.push({context: c.id, owner: c.owner, entries: c.entries, name, blocks: blocks.length, instructions,
    file, sha256: hash(bytes), bytes: bytes.length, imports, ...binding});
  return {run: instance.exports.run};
}
function open(owner, entries) {
  const c = {id: ++counts.contexts, owner, entries, captures: 0};
  if (owner === 'standalone') {c.memory = new WebAssembly.Memory({initial: 1}); c.base = 128; input(c, 0, initial); return c;}
  const instance = new WebAssembly.Instance(engine.module, {}); c.memory = instance.exports.memory; c.api = {};
  for (const name of ['open', 'close', 'arena_ptr', 'map', 'protect', 'upload', 'read8', 'compile', 'compile_entries',
    'compile_resident', 'compile_resident_entries', 'generation', 'module_ptr', 'module_len', 'guard', 'guard_resident']) c.api[name] = instance.exports['ring3_abi_v1_' + name];
 attachDetectionApi(c.api, instance.exports);
  assert.equal(c.api.open(3, c.id, 0x414e4f50), 0); c.base = c.api.arena_ptr() >>> 0; input(c, 0, initial);
  host(c, 'map', [0x1000, 1, 7]); host(c, 'map', [0x7000, 1, 3]); host(c, 'map', [0x9000, 1, 3]);
  for (const b of [...nops, ...live]) upload(c, b.pc, b.bytes);
  upload(c, 0x7000, Buffer.from([0x55, 0xa5, 0x19])); upload(c, 0x9000, Buffer.from([0x5a, 0xaa]));
  host(c, 'protect', [0x9000, 1, 0]); return c;
}
function checkRam(c, address, values) {
  for (const [i, value] of values.entries()) {
    const expected = arena(c), helper = record('R3MH', 40, [0, value, 0, 0, 0, 1]); helper.writeUInt16LE(2, 4); expected.set(helper, 100);
    assert.equal(c.api.read8(address + i), 0); assert.deepEqual(arena(c), expected, 'selected RAM unchanged; diagnostic helper only');
  }
}
for (const owner of ['standalone', 'replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries), unit = compile(c, 'nops', nops);
  for (const row of nops) for (const f of flags) {
    seed(c, row.pc, f); run(c, unit, 1, {...cpu(c), pc: row.pc + row.length}, 'all128 valid FLAGS and dirty parents preserved'); counts.flag_inputs++;
  }
  for (const row of nops) for (const [eax, address] of apparent) {
    const r = registers(); r[0] = eax; seed(c, row.pc, 0xcd7, r);
    run(c, unit, 1, {...cpu(c), pc: row.pc + row.length}, `apparent address ${address.toString(16)} is not resolved`); counts.apparent_addresses++;
  }
  seed(c, nops[0].pc);
  for (let i = 0; i < 2; i++) {
    input(c, 0, state({...cpu(c), pc: nops[0].pc}));
    run(c, unit, 1, {...cpu(c), pc: nops[0].pc + nops[0].length}, 'only EIP rewound; returned full parents and FLAGS retained'); counts.repeats++;
  }
  const current = cpu(c); current.pc = nops[2].pc; current.flags = 0x892;
  current.registers = current.registers.map((r, i) => wrap(BigInt(r) + BigInt(0x12340000 + i * 257)));
  input(c, 0, state(current)); run(c, unit, 1, {...cpu(c), pc: nops[2].pc + nops[2].length}, 'explicit current parents and FLAGS'); counts.current_inputs++;
  seed(c, nops[0].pc); run(c, unit, 0, cpu(c), 'zero budget neutral', {retired: 0});
  input(c, 96, words([1])); run(c, unit, 1, cpu(c), 'cancel neutral', {reason: 2, retired: 0});
  run(c, unit, 0, cpu(c), 'cancel precedes zero budget', {reason: 2, retired: 0}); input(c, 96, words([0]));
  run(c, unit, 3, {...cpu(c), pc: nops[0].pc + nops[0].bytes.length}, 'NOP and tail then cold lookup', {reason: 3, retired: 2});
  run(c, unit, 1, cpu(c), 'cold entry preserves returned state', {reason: 3, retired: 0});
  run(c, unit, 0, cpu(c), 'budget before cold lookup', {retired: 0}); counts.controls += 6;
  if (c.api) upload(c, 0x7002, Buffer.from([0x37]));
  seed(c, nops[1].pc); run(c, unit, 1, {...cpu(c), pc: nops[1].pc + nops[1].length},
    c.api ? 'unrelated data write leaves source owner current' : 'saved unbound artifact remains executable'); counts.data_currentness++;
  const continuation = compile(c, 'live', live); seed(c, nops[0].pc);
  if (owner === 'replacement') run(c, unit, 1, cpu(c), 'older replacement generation neutral InvalidArtifact3', {status: 3, retired: 0});
  else run(c, unit, 1, {...cpu(c), pc: nops[0].pc + nops[0].length}, 'older unit retained after later capture'); counts.retained++;
  const r = registers(); r[0] = 0xa10000ff; r[2] = 0xa1020000; seed(c, 0x1400, 0xcd7, r);
  let next = cpu(c); next.pc = 0x1402; next.registers[0] = 0xa1000000; next.flags = 0x457;
  run(c, continuation, 1, next, 'live ADD byte publishes carry/auxiliary/zero/parity flags');
  run(c, continuation, 1, {...cpu(c), pc: 0x140c}, 'live ten-byte NOP preserves producer state without CPU reseeding');
  next = cpu(c); next.pc = 0x140f; next.registers[0] = 0xa1000001; next.flags = 0x402;
  run(c, continuation, 1, next, 'ADC consumes preserved carry and current WORD parents');
  next = cpu(c); next.pc = 0x1415; next.registers[3] = 0xa1030000;
  run(c, continuation, 2, next, 'SETB/SETO consume returned arithmetic flags', {retired: 2});
  run(c, continuation, 2, {...cpu(c), pc: 0x1417}, 'live tail reaches cold lookup', {reason: 3}); counts.consumers += 5;
  if (c.api) {
    checkRam(c, 0x7000, [0x55, 0xa5, 0x37]);
    upload(c, 0x1049, Buffer.from([0]));
    run(c, continuation, 1, cpu(c), 'same-byte last displacement source mutation neutral CodeInvalidated4', {status: 4, retired: 0}); counts.invalidated++;
    host(c, 'protect', [0x9000, 1, 1]); checkRam(c, 0x9000, [0x5a, 0xaa]); assert.equal(c.api.close(), 0);
  }
  contexts.push({id: c.id, owner, entries, captures: c.captures, mapped_pages: c.api ? 3 : null});
}
assert.deepEqual(sources(), sourcePins);
assert.deepEqual(counts, {contexts: 6, runs: 2494, retired: 2470, flag_inputs: 2304, apparent_addresses: 90,
  repeats: 12, current_inputs: 6, controls: 36, data_currentness: 6, retained: 6, consumers: 30, invalidated: 4});
assert.equal(modules.length, 12);
const alignmentCounts = { ...counts };
const endbrCounts = { contexts: 0, runs: 0, retired: 0, flags: 0, controls: 0 };
for (const owner of ['standalone', 'replacement', 'resident'])
  for (const entries of [false, true]) {
    const c = open(owner, entries);
    if (c.api) for (const row of endbr) upload(c, row.pc, row.bytes);
    for (const offset of [4396, 4628]) {
      input(
        c,
        offset,
        Buffer.from(Array.from({ length: 128 }, (_, index) => (index * 29 + offset) & 255)),
      );
    }
    const unit = compile(c, 'endbr', endbr);
    for (const row of endbr) {
      for (const f of [2, 0xcd7]) {
        seed(c, row.pc, f);
        run(
          c,
          unit,
          1,
          { ...cpu(c), pc: row.pc + 4 },
          'CET-disabled ENDBR preserves flags and dirty FP/XMM/YMM',
        );
        endbrCounts.flags++;
        endbrCounts.runs++;
        endbrCounts.retired++;
      }
      seed(c, row.pc);
      run(c, unit, 0, cpu(c), 'budget precedes ENDBR', { retired: 0 });
      input(c, 96, words([1]));
      run(c, unit, 1, cpu(c), 'cancel precedes ENDBR', { reason: 2, retired: 0 });
      run(c, unit, 0, cpu(c), 'cancel precedes ENDBR zero budget', { reason: 2, retired: 0 });
      input(c, 96, words([0]));
      run(c, unit, 1, { ...cpu(c), pc: row.pc + 4 }, 'same ENDBR resumes and retires once');
      endbrCounts.controls += 4;
      endbrCounts.runs += 4;
      endbrCounts.retired++;
    }
    if (c.api) assert.equal(c.api.close(), 0);
    contexts.push({
      id: c.id,
      owner,
      entries,
      purpose: 'endbr',
      captures: c.captures,
      mapped_pages: c.api ? 3 : null,
    });
    endbrCounts.contexts++;
  }
assert.deepEqual(endbrCounts, { contexts: 6, runs: 72, retired: 36, flags: 24, controls: 48 });
assert.deepEqual(counts, { ...alignmentCounts, contexts: 12, runs: 2566, retired: 2506 });
assert.equal(modules.length, 18);
assert.deepEqual(sources(), sourcePins);
const result = {status: 'ok', engine_sha256: engine.sha256, source_pins: sourcePins, counts, contexts, modules, observations,
  alignment_counts: alignmentCounts, endbr_counts: endbrCounts,
  helper_calls: 0, helper_trace: [],
  limits: ['exact canonical6/9/10-byte alignment NOPs in six standalone/replacement/resident explicit/entry profiles; finite selected inputs',
    'all128 valid FLAGS for each encoding; complete arena expected images preserve all GPR parents, helper and dirtyFP128 except declared EIP/exit effects',
    'apparent addresses zero/denied/unmapped/high/top are not resolved; no guest data helper imports, so helper_calls0 is structural absence rather than instrumented guard counts',
    'two captures/context; NOP bank3blocks6instructions and live1block6instructions; source code and protections precede captures, unchanged8/64/65536 limits',
    'live byte ADD to ten-byte NOP to WORD ADC and SETcc consumes returned state without CPU reseeding; selected accessible/denied RAM canaries remain unchanged',
    'native standalone source currentness guards byte retrieval; saved unbound modules have no runtime source guard; bound generation3 and source CodeInvalidated4 reject neutrally',
    'same-byte final displacement write alone invalidates preexisting RWX source; no general prefixes/segments/compiler/ISA/performance/browser/SDK/game/hostedCI claim',
    'ENDBR32 and ENDBR64 are four-byte integer NOP aliases in the fixed CET-disabled protected32 profile; separate2block bank in sixprofiles adds72calls and preserves complete4756-byte frames including dirtyFP/XMM/YMM',
    'shared OperationNop TF/SS-shadow behavior reuses existing native/control proofs; this extra bank claims no CET tracking, shadow stack, IBT enforcement or additional debug policy']};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2), {flag: 'wx'});
console.log(JSON.stringify({status: 'ok', ...counts, modules: modules.length, output}));
