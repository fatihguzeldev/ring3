import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../../../../../../../support/wasm/engine.mjs';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(process.env.RING3_ENGINE_SHA256, 'select an explicit engine SHA-256');
const engine = readEngine(enginePath), initial = readFileSync(join(output, 'initial-arena.bin'));
const SIZE = 4796, PC = 0x1000, SEG = 4524, keyHigh = 0x43534645;
const contexts = [], modules = [], observations = [], refusals = [], helpers = [];
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
assert.equal(initial.length, SIZE);
const sourcePaths = [
  'engine/src/cpu/x86/decode/decoder.rs', 'engine/src/cpu/x86/decode/fetch.rs', 'engine/src/cpu/x86/decode/mod.rs',
  'engine/src/cpu/dbt/region.rs', 'engine/src/cpu/dbt/cold.rs', 'engine/src/cpu/dbt/gate.rs',
  'engine/src/cpu/dbt/artifact.rs', 'engine/src/cpu/dbt/resident.rs', 'engine/src/cpu/dbt/wasm/emitter.rs',
  'engine/src/cpu/dbt/wasm/abi.rs', 'engine/src/cpu/dbt/wasm/segments/context.rs',
  'engine/src/cpu/dbt/wasm/code_fetch.rs', 'engine/src/cpu/dbt/wasm/dispatcher.rs',
  'engine/src/process/execution/compiled.rs', 'engine/src/process/execution/stopped.rs',
  'engine/src/process/execution/resident.rs', 'engine/src/process/execution/authority.rs',
  'engine/src/process/wasm/execution.rs', 'engine/src/process/wasm/mod.rs', 'engine/src/abi/wasm/exports/execution.rs',
  'engine/src/memory/space.rs', 'engine/src/memory/space/atomic.rs', 'engine/tests/support/wasm.rs', 'engine/tests/support/wasm/engine.mjs',
  'engine/tests/cpu/segments/code/fetch/native.rs', 'engine/tests/cpu/segments/code/fetch/wasm.rs',
  'engine/tests/cpu/segments/native.rs', 'engine/tests/cpu/segments/wasm.rs',
  'engine/tests/cpu/segments/code/fetch/fixtures/p2-code-fetch/runtime/run.mjs',
];
const pins = () => Object.fromEntries(sourcePaths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = pins();
const words = values => {const b = Buffer.alloc(values.length * 4); values.forEach((v, i) => b.writeUInt32LE(v >>> 0, i * 4)); return b;};
function record(magic, size, fields = [], version = 1) {
  const b = Buffer.alloc(size); b.write(magic); b.writeUInt16LE(version, 4); b.writeUInt16LE(1, 6);
  b.writeUInt32LE(size, 8); b.set(words(fields), 16); return b;
}
const arena = c => Buffer.from(new Uint8Array(c.memory.buffer, c.base, SIZE));
const input = (c, at, bytes) => new Uint8Array(c.memory.buffer).set(bytes, c.base + at);
function host(c, name, args = []) {assert.equal(c.api[name](...args), 0, name);}
function upload(c, address, bytes) {input(c, 140, bytes); host(c, 'upload', [address, bytes.length]);}
function map(c, page, permissions) {host(c, 'map', [page, 1, permissions]); c.pages.push(page); c.pages.sort((a, b) => a - b);}
function csLimit(c, limit) {
  const segment = record('R3SG', 64, [], 2);
  [0x23, 0x1b, 0x23, 0x23, 0x3b, 0].forEach((value, i) => segment.writeUInt16LE(value, 16 + 2 * i));
  [0xf33fffff, 0xf2b00000 | limit, 0xf33fffff, 0xf33fffff, 0xf33fffff, 0]
    .forEach((value, i) => segment.writeUInt32LE(value >>> 0, [28, 32, 36, 40, 44, 56][i]));
  input(c, SEG, segment);
}
function seed(c, pc = PC) {input(c, 0, record('R3ST', 56, [...c.registers, pc, 2])); input(c, 96, words([0]));}
function ram(c) {
  const saved = arena(c), result = Buffer.alloc(c.pages.length * 4096);
  c.pages.forEach((page, index) => {
    for (let offset = 0; offset < 4096; offset += 4) {
      host(c, 'read32', [page + offset]); const packet = new DataView(c.memory.buffer, c.base + 100, 40);
      assert.equal(packet.getUint32(16, true), 0); result.writeUInt32LE(packet.getUint32(20, true), index * 4096 + offset);
    }
  });
  const observed = arena(c), allowed = Buffer.from(saved); allowed.set(observed.subarray(100, 140), 100);
  assert.deepEqual(observed, allowed, 'RAM inspection changes only packet'); input(c, 100, saved.subarray(100, 140)); return result;
}
function open(owner, entries, label) {
  const instance = new WebAssembly.Instance(engine.module, {});
  const c = {id: contexts.length + 1, owner, entries, label, memory: instance.exports.memory, api: {}, calls: [], pages: [], registers: Array(8).fill(0x11223344)};
  for (const [name, value] of Object.entries(instance.exports))
    if (name.startsWith('ring3_abi_v1_') && typeof value === 'function') c.api[name.slice(13)] = value;
  host(c, 'open', [4, c.id, keyHigh]); c.base = c.api.arena_ptr() >>> 0; input(c, 0, initial); map(c, PC, 7);
  contexts.push({id: c.id, owner, entries, label}); return c;
}
function captureModule(c, label) {
  const resident = c.owner === 'resident', a = arena(c);
  const pointer = resident ? a.readUInt32LE(156) : c.api.module_ptr() >>> 0;
  const length = resident ? a.readUInt32LE(160) : c.api.module_len() >>> 0;
  c.binding = resident ? [c.id, keyHigh, a.readUInt32LE(148), a.readUInt32LE(152)] : [];
  const bytes = Buffer.from(new Uint8Array(c.memory.buffer, pointer, length)), module = new WebAssembly.Module(bytes), ring3 = {...c.api};
  const imports = WebAssembly.Module.imports(module);
  assert.deepEqual(imports.map(row => `${row.module}:${row.name}:${row.kind}`).sort(),
    ['env:memory:memory', `ring3:${resident ? 'guard_resident' : 'guard'}:function`].sort(), 'CS fetch adds no per-instruction host helper');
  for (const imp of imports.filter(row => row.module === 'ring3' && !row.name.startsWith('guard')))
    ring3[imp.name] = (...args) => {c.calls.push(imp.name); helpers.push({context: c.id, name: imp.name, args}); return c.api[imp.name](...args);};
  c.run = new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3}).exports.run;
  c.moduleInfo = {pointer, length, binding: [...c.binding]};
  const file = `module-${modules.length + 1}.wasm`; writeFileSync(join(output, file), bytes, {flag: 'wx'});
  modules.push({context: c.id, label, owner: c.owner, file, bytes: length, sha256: hash(bytes), imports});
}
function compile(c, bytes) {
  upload(c, PC, bytes); input(c, 140, words(c.entries ? [PC] : [PC, bytes.length]));
  host(c, c.owner === 'resident' ? c.entries ? 'compile_resident_entries' : 'compile_resident' : c.entries ? 'compile_entries' : 'compile', [1, ...(c.entries ? [0] : [])]);
  captureModule(c, 'initial material'); seed(c);
}
function run(c, label, {budget = 1, reason = 1, retired = budget, pc, version = reason === 11 ? 6 : 1, fn = c.run, expectedHelpers = []} = {}) {
  const before = arena(c), beforeRam = ram(c), expected = Buffer.from(before), at = c.calls.length;
  expected.set(record('R3ST', 56, [...c.registers, pc, 2])); expected.set(record('R3EX', 40, [reason, retired, 0, 0, 0, 0], version), 56);
  assert.equal(fn(c.base, c.base + 56, budget, c.base + 96), 0, label);
  const after = arena(c); assert.deepEqual(after, expected, label + ' complete arena'); assert.deepEqual(c.calls.slice(at), expectedHelpers, label + ' exact helpers');
  const afterRam = ram(c); assert.deepEqual(afterRam, beforeRam, label + ' every mapped RAM byte');
  const n = observations.length + 1, arenaFile = `arena-${n}.bin`, ramFile = `ram-${n}.bin`;
  writeFileSync(join(output, arenaFile), after, {flag: 'wx'}); writeFileSync(join(output, ramFile), afterRam, {flag: 'wx'});
  observations.push({context: c.id, label, budget, reason, retired, pc, version,
    arena: {file: arenaFile, bytes: SIZE, sha256: hash(after)}, ram: {file: ramFile, pages: [...c.pages], bytes: afterRam.length, sha256: hash(afterRam)}});
}
function neutral(c, label, fn, status) {
  const before = arena(c), beforeRam = ram(c), at = c.calls.length;
  assert.equal(fn(), status, label); assert.deepEqual(arena(c), before, label + ' neutral arena');
  assert.deepEqual(ram(c), beforeRam, label + ' neutral RAM'); assert.equal(c.calls.length, at, label + ' no helper');
  refusals.push({context: c.id, label, status});
}
function stopped(c, label) {
  const before = arena(c);
  if (c.owner === 'resident') {
    host(c, 'compile_stopped_resident_entry', [c.id, keyHigh]);
    const after = arena(c), allowed = Buffer.from(before); allowed.set(after.subarray(140, 164), 140);
    assert.deepEqual(after, allowed, label + ' only existing resident publication');
    assert.equal(after.readUInt32LE(140), 1); assert.equal(after.readUInt32LE(144), 24);
  } else {
    const generation = c.api.generation() >>> 0; host(c, 'compile_stopped_entry', [c.id, keyHigh, generation]);
    assert.equal(c.api.generation() >>> 0, generation + 1); assert.deepEqual(arena(c), before, label + ' neutral CPU records');
  }
  captureModule(c, label);
}
function prior(c, entry, partial, nonexec) {
  if (nonexec) map(c, 0x2000, 3);
  const branch = Buffer.concat([Buffer.from([0xe9]), words([entry - (PC + 5)])]);
  if (partial.length) upload(c, entry, Buffer.from(partial));
  compile(c, branch);
  run(c, 'real prior branch reaches code miss', {budget: 2, reason: 3, retired: 1, pc: entry});
  csLimit(c, 0x1fff); stopped(c, 'captured CS context fault');
}
function lookup(c, label, expectedBinding) {
  const before = arena(c), status = c.api.find_resident(0x2000);
  if (!expectedBinding) {assert.equal(status, 17, label); assert.deepEqual(arena(c), before, label + ' neutral miss'); return;}
  assert.equal(status, 0, label); const after = arena(c), allowed = Buffer.from(before); allowed.set(after.subarray(140, 164), 140);
  assert.deepEqual(after, allowed, label + ' resident record only');
  assert.deepEqual([after.readUInt32LE(148), after.readUInt32LE(152)], expectedBinding.slice(2), label);
}
function dispatcherDenied(c) {
  host(c, 'dispatcher_module', [c.id, keyHigh]); const a = arena(c), pointer = a.readUInt32LE(164), length = a.readUInt32LE(168);
  const bytes = Buffer.from(new Uint8Array(c.memory.buffer, pointer, length)), module = new WebAssembly.Module(bytes), ring3 = {...c.api};
  ring3.find_installed_resident = (...args) => {c.calls.push('find_installed_resident'); helpers.push({context: c.id, name: 'find_installed_resident', args}); return c.api.find_installed_resident(...args);};
  const table = new WebAssembly.Table({element: 'anyfunc', initial: 8, maximum: 8});
  const fn = new WebAssembly.Instance(module, {env: {memory: c.memory, table}, ring3}).exports.run;
  const file = `module-${modules.length + 1}.wasm`; writeFileSync(join(output, file), bytes, {flag: 'wx'});
  modules.push({context: c.id, label: 'resident dispatcher', owner: 'dispatcher', file, bytes: length, sha256: hash(bytes), imports: WebAssembly.Module.imports(module)});
  run(c, 'dispatcher discovers an uninstalled source-free fault unit', {reason: 3, version: 3, retired: 0, pc: 0x2000, fn, expectedHelpers: ['find_installed_resident']});
  table.set(0, c.run);
  host(c, 'acknowledge_resident_installation', [...c.binding, 0]);
  run(c, 'installed source-free child owns precise CS fault through dispatcher', {reason: 11, retired: 0, pc: 0x2000, fn, expectedHelpers: ['find_installed_resident']});
}

for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries, 'normal material CS narrowing'); compile(c, Buffer.from([0x90, 0xeb, 0])); csLimit(c, 0xfff);
  run(c, 'budget precedes normal CS check', {budget: 0, retired: 0, pc: PC});
  input(c, 96, words([1])); run(c, 'cancel precedes normal CS check', {reason: 2, retired: 0, pc: PC}); input(c, 96, words([0]));
  run(c, 'live CS narrowing denies NOP without retirement', {reason: 11, retired: 0, pc: PC});
  if (owner === 'replacement' && !entries) {
    seed(c, 0x2000); run(c, 'unmatched unmapped PC checks first CS byte before NeedCode', {reason: 11, retired: 0, pc: 0x2000}); seed(c);
  }
  csLimit(c, 0x1fff); run(c, 'same material bytes resume after CS repair', {pc: PC + 1}); host(c, 'close');
}
for (const owner of ['replacement', 'resident']) {
  const c = open(owner, false, 'source-free capture and repair'); prior(c, 0x2000, [], false);
  run(c, 'budget precedes source-free context fault', {budget: 0, retired: 0, pc: 0x2000});
  input(c, 96, words([1])); run(c, 'cancel precedes source-free context fault', {reason: 2, retired: 0, pc: 0x2000}); input(c, 96, words([0]));
  run(c, 'unmapped source-free entry produces GP', {reason: 11, retired: 0, pc: 0x2000});
  if (owner === 'resident') {lookup(c, 'denied synthetic resident point routes', c.binding); dispatcherDenied(c);}
  map(c, 0x2000, 3); run(c, 'new denied nonEXEC page is not a source dependency', {reason: 11, retired: 0, pc: 0x2000});
  host(c, 'protect', [0x2000, 1, 7]); upload(c, 0x2000, Buffer.from([0x90, 0xeb, 0])); csLimit(c, 0x2fff);
  const faultRun = c.run, faultBinding = [...c.binding];
  run(c, 'repaired synthetic span yields same-PC NeedCode', {reason: 3, retired: 0, pc: 0x2000});
  if (owner === 'resident') lookup(c, 'repaired synthetic point does not shadow material admission');
  stopped(c, 'same-PC material recompilation');
  if (owner === 'resident') {
    assert.notDeepEqual(c.binding, faultBinding); lookup(c, 'new material resident owner wins routing', c.binding);
    run(c, 'retained repaired synthetic unit stays NeedCode', {reason: 3, retired: 0, pc: 0x2000, fn: faultRun});
  } else neutral(c, 'old replacement generation is neutral', () => faultRun(c.base, c.base + 56, 1, c.base + 96), 3);
  run(c, 'newly compiled material executes at repaired PC', {pc: 0x2001}); host(c, 'close');
}
for (const [owner, nonexec, preceding] of [['replacement', false, false], ['resident', true, false], ['replacement', false, true]]) {
  const c = open(owner, false, preceding ? 'preceding retirement before partial CS denial' : 'real prefix capture');
  const entry = preceding ? 0x1ffe : 0x1fff; prior(c, entry, preceding ? [0x90, 0x0f] : [0x0f], nonexec);
  run(c, 'partial prefix GP precedes denied next-byte fetch', {budget: preceding ? 2 : 1, reason: 11, retired: Number(preceding), pc: 0x1fff});
  if (!preceding) {
    if (!nonexec) map(c, 0x2000, 3); else {host(c, 'protect', [0x2000, 1, 7]); upload(c, 0x2000, Buffer.from([0x1f, 0xc0, 0xeb, 0]));}
    run(c, 'denied tail mapping and bytes are not prefix dependencies', {reason: 11, retired: 0, pc: 0x1fff});
    const prefixFaultRun = c.run;
    if (nonexec) {
      csLimit(c, 0x2fff); run(c, 'repaired real-prefix span yields NeedCode', {reason: 3, retired: 0, pc: 0x1fff});
      stopped(c, 'real-prefix same-PC material recompilation');
      run(c, 'repaired crossing-page NOP executes from original prefix', {pc: 0x2002});
    }
    upload(c, 0x1fff, Buffer.from([0x0f]));
    neutral(c, 'equal write to consumed prefix invalidates retained execution', () => prefixFaultRun(c.base, c.base + 56, 1, c.base + 96), 4);
  }
  host(c, 'close');
}
assert.equal(contexts.length, 9); assert.equal(modules.length, 18); assert.equal(observations.length, 44); assert.equal(refusals.length, 3);
assert.deepEqual(pins(), sourcePins, 'CS authority source and literal oracles stayed fixed');
writeFileSync(join(output, 'result.json'), JSON.stringify({status: 'ok', engine: {path: enginePath, sha256: engine.sha256, bytes: engine.bytes.length},
  source_pins: sourcePins, arena_bytes: SIZE, contexts, modules, observations, refusals, helper_trace: helpers,
  limits: ['finite CS base-zero/D32 fetch authority, actual runtime capture and same-PC repair; no D16/nonflat CS or full control-transfer claim',
    'real prefix currentness remains page-granular; source-free fault owns memory identity but no denied bytes',
    'ordinary resident API/routing exercised; existing unchanged owner/busy/callback controls remain separate evidence',
    'pure module budget/cancel/NeedCode retain v1 headers; GP uses v6; dispatcher lookup reaches the installed source-free fault child',
    'no browser, performance, or game compatibility claim'],
}, null, 2) + '\n');
console.log(JSON.stringify({status: 'ok', contexts: contexts.length, modules: modules.length, runs: observations.length, refusals: refusals.length, output}));
