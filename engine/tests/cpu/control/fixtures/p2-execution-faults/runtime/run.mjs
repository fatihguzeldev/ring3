import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../../../../../support/wasm/engine.mjs';
import {createArena, ARENA_SIZE} from '../../../../../support/wasm/arena.mjs';

const [enginePath, output, root, suite = 'basic'] = process.argv.slice(2), engine = readEngine(enginePath);
assert.ok(['basic', 'privileged'].includes(suite));
const scenario = suite === 'privileged' ? await import('./privileged-cases.mjs') : undefined;
const PC = 0x1000, KEY_HIGH = 0x4641554c, observations = [], modules = []; let contexts = 0, runs = 0, refusals = 0;
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const paths = ['engine/src/cpu/x86/decode/decoder.rs', 'engine/src/cpu/x86/decode/execution_faults.rs',
  'engine/src/cpu/x86/control.rs', 'engine/src/cpu/dbt/cold.rs', 'engine/src/cpu/dbt/artifact.rs',
  'engine/src/cpu/dbt/region.rs', 'engine/src/cpu/dbt/wasm/control_instructions.rs', 'engine/src/cpu/dbt/wasm/emitter.rs',
  'engine/src/process/execution/stopped.rs', 'engine/src/process/wasm/execution.rs', 'engine/src/abi/wasm/exports/execution.rs',
  'engine/tests/cpu/control/fixtures/p2-execution-faults/runtime/run.mjs'];
if (scenario) paths.push('engine/src/cpu/x86/decode/execution_faults_tests.rs',
  'engine/tests/cpu/control/fixtures/p2-execution-faults/runtime/privileged-cases.mjs',
  'engine/tests/cpu/control/fixtures/p2-execution-faults/runtime/privileged.mjs');
const pins = () => Object.fromEntries(paths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = pins();
const words = values => {const bytes = Buffer.alloc(values.length * 4); values.forEach((value, i) => bytes.writeUInt32LE(value >>> 0, i * 4)); return bytes;};
function exit(reason, retired, version = 1) {
  const bytes = Buffer.alloc(40); bytes.write('R3EX'); bytes.writeUInt16LE(version, 4);
  bytes.writeUInt16LE(1, 6); bytes.writeUInt32LE(40, 8); bytes.set(words([reason, retired, 0, 0, 0, 0]), 16); return bytes;
}
function open(row, entries) {
  const instance = new WebAssembly.Instance(engine.module, {}), api = {};
  for (const [name, fn] of Object.entries(instance.exports)) if (name.startsWith('ring3_abi_v1_') && typeof fn === 'function') api[name.slice(13)] = fn;
  assert.equal(typeof api.compile_stopped_entry, 'function', 'integrated engine exports the explicit stopped-entry endpoint');
  const id = ++contexts, memory = instance.exports.memory;
  assert.equal(api.open(2, id, KEY_HIGH), 0); const base = api.arena_ptr() >>> 0;
  const arena = () => Buffer.from(new Uint8Array(memory.buffer, base, ARENA_SIZE));
  const put = (offset, bytes) => new Uint8Array(memory.buffer).set(bytes, base + offset);
  const upload = (address, bytes) => {put(140, bytes); assert.equal(api.upload(address, bytes.length), 0);};
  const target = row.target ?? 0x1100;
  assert.equal(api.map(PC, 1, 7), 0);
  const jump = Buffer.alloc(5); jump[0] = 0xe9; jump.writeInt32LE(target - PC - 5, 1);
  upload(PC, jump); upload(target, row.bytes);
  put(140, words(entries ? [PC] : [PC, 5]));
  assert.equal(api[entries ? 'compile_entries' : 'compile'](1, ...(entries ? [0] : [])), 0);
  const moduleBytes = () => Buffer.from(new Uint8Array(memory.buffer, api.module_ptr() >>> 0, api.module_len() >>> 0));
  let helpers = 0;
  function load() {
    const bytes = moduleBytes(), module = new WebAssembly.Module(bytes);
    assert.deepEqual(WebAssembly.Module.imports(module), [{module: 'env', name: 'memory', kind: 'memory'}, {module: 'ring3', name: 'guard', kind: 'function'}], 'synthetic faults never call a memory, platform, or numerical helper');
    const file = `module-${modules.length + 1}.wasm`; writeFileSync(join(output, file), bytes, {flag: 'wx'});
    modules.push({label: row.name, entries, file, bytes: bytes.length, sha256: hash(bytes)});
    const ring3 = Object.fromEntries(WebAssembly.Module.imports(module).filter(item => item.kind === 'function').map(item => [item.name, (...args) => {if (item.name !== 'guard') helpers++; return api[item.name](...args);} ]));
    return new WebAssembly.Instance(module, {env: {memory}, ring3}).exports.run;
  }
  const initial = createArena();
  [0xa1b2c3d4, 0x23456789, 0x3456789a, 0x456789ab, 0x7000, 0x56789abc, 0x6789abcd, 0x789abcde].forEach((value, i) => initial.writeUInt32LE(value, 16 + i * 4));
  initial.writeUInt32LE(PC, 48); put(0, initial);
  assert.equal(load()(base, base + 56, 8, base + 96), 0);
  const stopped = arena(); assert.equal(stopped.readUInt32LE(48), target);
  assert.deepEqual(stopped.subarray(56, 96), exit(3, 1), 'bootstrap reaches a real missing-code stop');
  return {api, base, memory, arena, put, upload, target, id, entries, load, moduleBytes, helpers: () => helpers, label: `${row.name}/${entries ? 'entry' : 'extent'}`};
}
function assertRefusal(c, invoke, label) {
  const before = c.arena(), generation = c.api.generation(), bytes = c.moduleBytes();
  assert.notEqual(invoke(), 0, c.label + '/' + label + ' remains a host preparation error');
  assert.deepEqual(c.arena(), before, c.label + '/' + label + ' preserves CPU, stop and numeric records');
  assert.equal(c.api.generation(), generation); assert.deepEqual(c.moduleBytes(), bytes); refusals++;
}
function prepare(c, row) {
  for (const entries of [false, true]) {
    c.put(140, words(entries ? [c.target] : [c.target, row.bytes.length]));
    assertRefusal(c, () => c.api[entries ? 'compile_entries' : 'compile'](1, ...(entries ? [0] : [])), 'public-strict-' + entries);
  }
  c.put(140, words([0xdeadbeef, 0xffffffff]));
  const before = c.arena(), generation = c.api.generation();
  assert.equal(c.api.compile_stopped_entry(c.id, KEY_HIGH, generation), 0, c.label + ' runtime entry preparation');
  assert.equal(c.api.generation(), generation + 1);
  assert.deepEqual(c.arena(), before, 'runtime preparation uses state EIP and ignores poisoned descriptors without publishing CPU changes');
  return c.load();
}
function execute(c, fn, row, {flags = 2, shadow = 0, budget = 8, cancel = 0, reason = row.reason, retired = row.retired ?? 0, pc = c.target + (row.offset ?? 0)} = {}) {
  const before = c.arena(); before.writeUInt16LE(flags & ~0xcd7 || shadow ? 2 : 1, 4);
  before.writeUInt32LE(shadow, 12); before.writeUInt32LE(c.target, 48); before.writeUInt32LE(flags, 52); before.writeUInt32LE(cancel, 96);
  for (const offset of [80, 84, 88, 92]) before.writeUInt32LE(0xdeadbeef, offset);
  c.put(0, before); const expected = Buffer.from(before);
  expected.writeUInt32LE(retired === 0 && [1, 2].includes(reason) ? shadow : 0, 12);
  expected.writeUInt32LE(pc, 48);
  expected.set(exit(reason, retired, reason === 20 ? 7 : row.baseVersion), 56);
  const calls = c.helpers(); assert.equal(fn(c.base, c.base + 56, budget, c.base + 96), 0, c.label + ' run status');
  assert.deepEqual(c.arena(), expected, c.label + ' precise fault/stop, full CPU, FLAGS, shadow and unrelated arena preservation');
  assert.equal(c.helpers(), calls, 'no guest operand/helper effect before synthetic fault');
  observations.push({label: c.label, flags, shadow, budget, cancel, reason, retired, pc}); runs++;
}
const positive = scenario?.positive ?? [
  {name: 'lock-nop-invalid-opcode', bytes: Buffer.from([0xf0, 0x90]), reason: 20, baseVersion: 1},
  {name: 'lock-hlt-invalid-before-privilege', bytes: Buffer.from([0xf0, 0xf4]), reason: 20, baseVersion: 1},
  {name: 'reserved-opcode', bytes: Buffer.from([0x0f, 0x0a]), reason: 20, baseVersion: 1},
  {name: 'cpl3-hlt-general-protection-zero', bytes: Buffer.from([0xf4]), reason: 11, baseVersion: 6},
  {name: 'ignored66-hlt-general-protection-zero', bytes: Buffer.from([0x66, 0xf4]), reason: 11, baseVersion: 6},
  {name: '15-prefixes-general-protection-before-unmapped-successor', bytes: Buffer.alloc(15, 0x2e), target: 0x1ff1, reason: 11, baseVersion: 6},
];
for (const entries of [false, true]) for (const row of positive) {
  const c = open(row, entries), fn = prepare(c, row);
  execute(c, fn, row);
  execute(c, fn, row, {flags: 0x102});
  execute(c, fn, row, {flags: 0x244cd7, shadow: 1});
  execute(c, fn, row, {flags: 0x102, shadow: 1, budget: 0, reason: 1, retired: 0, pc: c.target});
  execute(c, fn, row, {flags: 0x102, shadow: 1, cancel: 1, reason: 2, retired: 0, pc: c.target});
  const before = c.arena(); before.writeUInt32LE(0, 96);
  c.upload(c.target, Buffer.from([0x90]));
  const current = c.arena(); current.set(before.subarray(0, 140)); c.put(0, current);
  assert.notEqual(fn(c.base, c.base + 56, 8, c.base + 96), 0, 'modified executable bytes invalidate the synthetic fault artifact');
  assert.deepEqual(c.arena(), current, 'stale fault guard publishes no guest exception'); refusals++;
}
for (const entries of [false, true]) {
  const row = {name: 'completed-nop-before-invalid-fault', bytes: Buffer.from([0x90, 0xf0, 0x90]), reason: 20, baseVersion: 1, retired: 1, offset: 1};
  const c = open(row, entries), fn = prepare(c, row); execute(c, fn, row);
}
const negatives = scenario?.negatives ?? [
  {name: 'valid-unsupported-int80', bytes: Buffer.from([0xcd, 0x80])},
  {name: 'environment-dependent-cli', bytes: Buffer.from([0xfa])},
  {name: 'environment-dependent-sti', bytes: Buffer.from([0xfb])},
  {name: 'reserved-repne-hlt', bytes: Buffer.from([0xf2, 0xf4])},
  {name: 'reserved-rep-hlt', bytes: Buffer.from([0xf3, 0xf4])},
  {name: 'unspecified-repeated66', bytes: Buffer.from([0x66, 0x66, 0x90])},
  {name: 'ambiguous15-reserved-opcode', bytes: Buffer.from([...Array(13).fill(0x2e), 0x0f, 0x0a]), target: 0x1ff1},
  {name: 'ambiguous15-needs16th-immediate-byte', bytes: Buffer.from([...Array(11).fill(0x2e), 0xb8, 1, 2, 3]), target: 0x1ff1},
  {name: 'incomplete-fetch-remains-typed-preparation-error', bytes: Buffer.from([0x0f]), target: 0x1fff},
];
for (const entries of [false, true]) for (const row of negatives) {
  const c = open(row, entries); c.put(140, words([0xdeadbeef, 0xffffffff]));
  assertRefusal(c, () => c.api.compile_stopped_entry(c.id, KEY_HIGH, c.api.generation()), 'runtime-nonarchitectural-error');
  if (row.name.startsWith('incomplete-fetch')) {
    assert.equal(c.api.map(0x2000, 1, 7), 0); c.upload(0x2000, Buffer.from([0x0a]));
    const before = c.arena(); assert.equal(c.api.compile_stopped_entry(c.id, KEY_HIGH, c.api.generation()), 0);
    assert.deepEqual(c.arena(), before); execute(c, c.load(), {reason: 20, baseVersion: 1});
  }
}
assert.deepEqual(pins(), sourcePins, 'execution fault proof sources remain frozen');
const result = {schema: 1, suite, engineSha256: engine.sha256, sourcePins, contexts, runs, refusals, modules, observations,
  claim: scenario?.claim ?? 'bounded execution-only invalid<15,15legacy-prefix and CPL3HLT faults; strict APIs, other privileged instructions, ambiguous15, fetch and host failures retain errors'};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2));
console.log(JSON.stringify({contexts, runs, refusals, engineSha256: engine.sha256}));
