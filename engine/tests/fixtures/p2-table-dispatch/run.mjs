import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';

const [outputDir, root] = process.argv.slice(2);
assert.ok(root, 'expected generated artifact directory and repository root');
const STATE = 128, EXIT = 184, CANCEL = 224, CALLS = 240, READY = 244;
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const encoded = Object.fromEntries(['a', 'b', 'dispatcher', 'wrong'].map(name => [name, readFileSync(join(outputDir, `${name}.wasm`))]));
const modules = Object.fromEntries(Object.entries(encoded).map(([name, bytes]) => { assert.ok(WebAssembly.validate(bytes), `${name} validates`); return [name, new WebAssembly.Module(bytes)]; }));
for (const name of ['a', 'b']) {
  assert.deepEqual(WebAssembly.Module.imports(modules[name]), [{module: 'env', name: 'memory', kind: 'memory'}], 'actual standalone units have no function imports');
  assert.deepEqual(WebAssembly.Module.exports(modules[name]), [{name: 'run', kind: 'function'}]);
}
assert.deepEqual(WebAssembly.Module.imports(modules.dispatcher), [{module: 'env', name: 'memory', kind: 'memory'}, {module: 'env', name: 'table', kind: 'table'}], 'dispatcher imports no JavaScript callback');
assert.deepEqual(WebAssembly.Module.exports(modules.dispatcher), [{name: 'run', kind: 'function'}, {name: 'probe', kind: 'function'}]);
assert.deepEqual(WebAssembly.Module.imports(modules.wrong), []);
assert.deepEqual(WebAssembly.Module.exports(modules.wrong), [{name: 'wrong', kind: 'function'}]);
assert.equal(readFileSync(join(outputDir, 'a.x86')).toString('hex'), '40e9fa0f0000', 'authored INC EAX/JMP B');
assert.equal(readFileSync(join(outputDir, 'b.x86')).toString('hex'), '490f85f9efffff', 'authored DEC ECX/JNZ A/done');
const stats = {dispatcher_calls: 0, dispatcher_child_calls_observed: 0, probe_calls: 0, probe_successes: 0, probe_traps: 0, controlled_misses: 0, cancelled_calls: 0, budget_exits: 0, done_pc_exits: 0, continuation_initial_calls: 0, continuation_resumes: 0, cold_b_installations: 0, full_memory_snapshots: 0};

function header(bytes, view, pointer, magic, size) {
  bytes.set(Buffer.from(magic, 'ascii'), pointer);
  view.setUint16(pointer + 4, 1, true); view.setUint16(pointer + 6, 1, true);
  view.setUint32(pointer + 8, size, true); view.setUint32(pointer + 12, 0, true);
}

function initial(eax = 0x89abcdef, ecx = 3, eflags = 0xcd7) {
  return {registers: [eax, ecx, 0x23456789, 0x3456789a, 0x456789ab, 0x56789abc, 0x6789abcd, 0x789abcde], eip: 0x1000, eflags};
}

function fresh(loadA = true, loadB = true) {
  const memory = new WebAssembly.Memory({initial: 1, maximum: 1});
  const table = new WebAssembly.Table({element: 'anyfunc', initial: 2, maximum: 2});
  const ctx = {memory, table, bytes: new Uint8Array(memory.buffer), view: new DataView(memory.buffer)};
  ctx.bytes.fill(0x5a);
  ctx.dispatcher = new WebAssembly.Instance(modules.dispatcher, {env: {memory, table}});
  assert.equal(ctx.dispatcher.exports.run.length, 4); assert.equal(ctx.dispatcher.exports.probe.length, 5);
  if (loadA) install(ctx, 'a', 0);
  if (loadB) install(ctx, 'b', 1);
  return ctx;
}

function install(ctx, name, index) {
  assert.equal(ctx[name], undefined, 'each actual unit instance installed once per test context');
  const instance = new WebAssembly.Instance(modules[name], {env: {memory: ctx.memory}});
  assert.equal(instance.exports.run.length, 4);
  ctx[name] = instance; ctx.table.set(index, instance.exports.run);
  assert.equal(ctx.table.get(index), instance.exports.run, 'table retains actual engine-generated function reference');
}

function reset(ctx, value, cancel = 0, ready = 3) {
  header(ctx.bytes, ctx.view, STATE, 'R3ST', 56);
  value.registers.forEach((register, index) => ctx.view.setUint32(STATE + 16 + index * 4, register, true));
  ctx.view.setUint32(STATE + 48, value.eip, true); ctx.view.setUint32(STATE + 52, value.eflags, true);
  ctx.bytes.fill(0xa5, EXIT, EXIT + 40);
  ctx.view.setUint32(CANCEL, cancel, true); ctx.view.setUint32(CALLS, 0xdeadbeef, true); ctx.view.setUint32(READY, ready, true);
}

function flags(op, operand, oldFlags) {
  const a = BigInt(operand), raw = op === 'inc' ? a + 1n : a - 1n;
  const value = Number(BigInt.asUintN(32, raw));
  let result = (oldFlags & 0x401) | 2, parity = 0;
  for (let byte = value & 255; byte !== 0; byte >>>= 1) parity += byte & 1;
  if (parity % 2 === 0) result |= 4;
  if (value === 0) result |= 0x40;
  if (value >= 0x80000000) result |= 0x80;
  if (op === 'inc' ? (operand & 15) === 15 : (operand & 15) === 0) result |= 0x10;
  const signed = op === 'inc' ? BigInt.asIntN(32, a) + 1n : BigInt.asIntN(32, a) - 1n;
  if (signed < -2147483648n || signed > 2147483647n) result |= 0x800;
  return result;
}
for (const [op, operand, oldFlags, expected] of [['inc', 0, 2, 2], ['inc', 0xffffffff, 3, 0x57], ['inc', 0x7fffffff, 2, 0x896], ['dec', 1, 0x403, 0x447], ['dec', 0, 2, 0x96], ['dec', 0x80000000, 2, 0x816]]) assert.equal(flags(op, operand, oldFlags), expected, 'independent literal flags');

// four authored instructions per cycle determine registers and the last arithmetic flags.
function trace(start, steps) {
  const n = start.registers[1], cycles = Math.floor(steps / 4), phase = steps % 4;
  assert.ok(steps >= 0 && steps <= n * 4);
  const expected = {...start, registers: [...start.registers]};
  expected.registers[0] = Number(BigInt.asUintN(32, BigInt(start.registers[0]) + BigInt(cycles + (phase > 0 ? 1 : 0))));
  expected.registers[1] = n - cycles - (phase === 3 ? 1 : 0);
  expected.eip = steps === n * 4 ? 0x2007 : [0x1000, 0x1001, 0x2000, 0x2001][phase];
  if (steps !== 0) {
    const lastInc = phase === 1 || phase === 2;
    const operand = lastInc ? Number(BigInt.asUintN(32, BigInt(start.registers[0]) + BigInt(cycles))) : n - cycles + (phase === 0 ? 1 : 0);
    expected.eflags = flags(lastInc ? 'inc' : 'dec', operand, start.eflags);
  }
  return expected;
}

function exit(reason, retired) {
  const bytes = new Uint8Array(40), view = new DataView(bytes.buffer);
  header(bytes, view, 0, 'R3EX', 40); view.setUint32(16, reason, true); view.setUint32(20, retired, true);
  return bytes;
}

function expectedMemory(before, value, reason, retired, calls) {
  const wanted = before.slice(), view = new DataView(wanted.buffer);
  value.registers.forEach((register, index) => view.setUint32(STATE + 16 + index * 4, register, true));
  view.setUint32(STATE + 48, value.eip, true); view.setUint32(STATE + 52, value.eflags, true);
  wanted.set(exit(reason, retired), EXIT);
  if (calls !== undefined) view.setUint32(CALLS, calls, true);
  return wanted;
}

function call(ctx, budget, expected, reason, retired, calls, label, continuation) {
  const before = ctx.bytes.slice(), refs = [ctx.table.get(0), ctx.table.get(1)];
  assert.equal(ctx.dispatcher.exports.run(STATE, EXIT, budget, CANCEL), 0, `${label}: controlled dispatcher exit`);
  stats.dispatcher_calls++; stats.dispatcher_child_calls_observed += ctx.view.getUint32(CALLS, true); stats.full_memory_snapshots++;
  assert.equal(ctx.view.getUint32(CALLS, true), calls, `${label}: actual Wasm child-call counter`);
  assert.deepEqual(ctx.bytes, expectedMemory(before, expected, reason, retired, calls), `${label}: complete 65536-byte memory/state/exit, only counter may additionally change`);
  assert.equal(ctx.table.get(0), refs[0]); assert.equal(ctx.table.get(1), refs[1]);
  if (continuation === 'initial') stats.continuation_initial_calls++;
  else if (continuation === 'resume') stats.continuation_resumes++;
  if (reason === 1) stats.budget_exits++; else if (reason === 2) stats.cancelled_calls++;
  else if (expected.eip === 0x2007) stats.done_pc_exits++; else stats.controlled_misses++;
}

function slice(ctx, start, from, budget, label, continuation) {
  const to = Math.min(start.registers[1] * 4, from + budget), retired = to - from;
  const reason = budget <= start.registers[1] * 4 - from ? 1 : 3;
  const calls = retired === 0 ? 0 : Math.ceil(to / 2) - Math.floor(from / 2);
  call(ctx, budget, trace(start, to), reason, retired, calls, label, continuation);
  return to;
}

const cold = fresh(true, false), coldStart = initial(0xfffffffd, 5, 0x403);
reset(cold, coldStart, 0, 1);
const stable = {memory: cold.memory, table: cold.table, a: cold.a, dispatcher: cold.dispatcher};
call(cold, 21, trace(coldStart, 2), 3, 2, 1, 'cold A commits two instructions then missing B', 'initial');
const cpuBeforeInstall = cold.bytes.slice(STATE, STATE + 56);
install(cold, 'b', 1); stats.cold_b_installations++;
cold.view.setUint32(READY, 3, true);
assert.deepEqual(cold.bytes.slice(STATE, STATE + 56), cpuBeforeInstall, 'cold B installation leaves actual continuation CPU state intact');
slice(cold, coldStart, 2, 19, 'cold continuation after one B installation', 'resume');
stable.b = cold.b;
for (const [name, reference] of Object.entries(stable)) assert.equal(cold[name], reference);
assert.equal(stats.cold_b_installations, 1);

const warmInputs = [[1, 0, 2], [2, 0xffffffff, 3], [3, 0x7fffffff, 0x402], [17, 0x80000000, 0x403], [5, 0xfffffffd, 0xcd7], [4, 0xf, 2]];
let warmCases = 0;
for (const [n, eax, oldFlags] of warmInputs) for (const budget of new Set([0, 1, 2, 3, 4, n * 4 - 1, n * 4, n * 4 + 1, 0xffffffff])) {
  const start = initial(eax, n, oldFlags); reset(cold, start);
  slice(cold, start, 0, budget, `warm mathematical trace/${n}/${eax}/${oldFlags}/budget${budget}`); warmCases++;
  for (const [name, reference] of Object.entries(stable)) assert.equal(cold[name], reference, 'warm execution reuses module instances/memory/table');
  assert.equal(cold.table.get(0), stable.a.exports.run); assert.equal(cold.table.get(1), stable.b.exports.run);
}

const resumedStart = initial(0x7ffffffd, 7, 0x403);
reset(cold, resumedStart);
let position = 0;
for (const [index, budget] of [1, 0, 1, 1, 0, 2, 3, 1, 4, 5, 2, 3, 1, 4].entries()) {
  position = slice(cold, resumedStart, position, budget, `true split continuation/${index}`, index === 0 ? 'initial' : 'resume');
  if (index === 2 || index === 6) {
    cold.view.setUint32(CANCEL, 1, true);
    call(cold, 0, trace(resumedStart, position), 2, 0, 0, 'cancel between true resumes outranks zero budget', 'resume');
    cold.view.setUint32(CANCEL, 0, true);
  }
}
assert.equal(position, resumedStart.registers[1] * 4);
slice(cold, resumedStart, position, 1, 'completed continuation repeats no instruction', 'resume');

for (const phase of [0, 1, 2, 3, 12]) for (const budget of [0, 1, 0xffffffff]) {
  const start = initial(0x12345678, 3, 0x403), stopped = trace(start, phase);
  reset(cold, stopped, 1, 0);
  call(cold, budget, stopped, 2, 0, 0, `preset cancellation precedes missing readiness/budget at phase${phase}`);
}

for (const pc of [0x1002, 0x2002, 0x3000, 0x2007]) for (const budget of [0, 1]) {
  const start = initial(); start.eip = pc; reset(cold, start);
  call(cold, budget, start, budget === 0 ? 1 : 3, 0, 0, 'exact instruction-entry routing and budget before unknown PC');
}
const missing = fresh(true, false), missingStart = initial();
reset(missing, missingStart, 0, 3);
call(missing, 13, trace(missingStart, 2), 3, 2, 1, 'ready B bit with null slot gives controlled NeedCode');
reset(missing, missingStart, 0, 0);
call(missing, 13, missingStart, 3, 0, 0, 'populated A with absent readiness gives controlled NeedCode');
missing.table.set(0, null); reset(missing, missingStart, 0, 3);
call(missing, 13, missingStart, 3, 0, 0, 'ready A bit with null slot gives controlled NeedCode');

const largeCycles = 1_000_000, largeStart = initial(0xffff0000, largeCycles, 0x403);
reset(cold, largeStart);
slice(cold, largeStart, 0, largeCycles * 4 + 1, 'million cycles complete in one JS dispatcher call with returning Wasm child calls');
assert.equal(cold.view.getUint32(CALLS, true), largeCycles * 2);
for (const [name, reference] of Object.entries(stable)) assert.equal(cold[name], reference);

function probeTrap(ctx, index, label) {
  const before = ctx.bytes.slice(); stats.probe_calls++;
  assert.throws(() => ctx.dispatcher.exports.probe(STATE, EXIT, 2, CANCEL, index), WebAssembly.RuntimeError, label);
  assert.deepEqual(ctx.bytes, before, `${label}: failed call_indirect preserves complete memory`);
  stats.probe_traps++; stats.full_memory_snapshots++;
}
reset(missing, missingStart);
probeTrap(missing, 0, 'null funcref is a platform trap');
probeTrap(missing, 2, 'table upper-bound index is a platform trap');
probeTrap(missing, 0xffffffff, 'unsigned table index is a platform trap');
const wrong = new WebAssembly.Instance(modules.wrong, {});
assert.equal(wrong.exports.wrong.length, 0);
missing.table.set(1, wrong.exports.wrong);
probeTrap(missing, 1, 'wrong Wasm function signature is a platform trap');
assert.throws(() => missing.table.set(0, () => 0), TypeError, 'plain JavaScript callback cannot replace a Wasm funcref');
assert.equal(missing.table.get(0), null);
assert.throws(() => missing.table.set(2, wrong.exports.wrong), RangeError, 'Table.set rejects out-of-bounds host installation');
assert.throws(() => new WebAssembly.Instance(modules.dispatcher, {env: {memory: cold.memory, table: new WebAssembly.Table({element: 'anyfunc', initial: 1, maximum: 2})}}), WebAssembly.LinkError, 'imported table requires two slots');
assert.throws(() => new WebAssembly.Instance(modules.dispatcher, {env: {memory: cold.memory, table: new WebAssembly.Table({element: 'externref', initial: 2, maximum: 2})}}), WebAssembly.LinkError, 'imported table requires funcref');

const probeStart = initial(0xffffffff, 3, 0x403); reset(cold, probeStart);
for (const [from, to, index, budget] of [[0, 1, 0, 1], [1, 2, 0, 1], [2, 4, 1, 2]]) {
  const before = cold.bytes.slice(); stats.probe_calls++;
  assert.equal(cold.dispatcher.exports.probe(STATE, EXIT, budget, CANCEL, index), 0, 'typed cross-module call_indirect succeeds');
  assert.deepEqual(cold.bytes, expectedMemory(before, trace(probeStart, to), 1, to - from, undefined), 'direct typed probe resumes actual child without dispatcher counter mutation');
  stats.probe_successes++; stats.full_memory_snapshots++;
}
assert.equal(stats.probe_calls, stats.probe_successes + stats.probe_traps);
assert.equal(stats.dispatcher_calls, stats.controlled_misses + stats.cancelled_calls + stats.budget_exits + stats.done_pc_exits);
const productionPaths = execFileSync('git', ['ls-files', 'engine/src'], {cwd: root, encoding: 'utf8'}).trim().split('\n').sort();
const sourcePaths = ['engine/tests/dbt_table_dispatch.rs', 'engine/tests/fixtures/p2-table-dispatch/run.mjs', 'engine/Cargo.toml', 'Cargo.lock', ...productionPaths];
const artifactHashes = Object.fromEntries(['a.wasm', 'b.wasm', 'dispatcher.wasm', 'wrong.wasm', 'a.x86', 'b.x86'].map(name => [name, hash(readFileSync(join(outputDir, name)))]));
const provenance = {
  stats, generated_modules: Object.keys(modules).length, actual_child_calls: stats.dispatcher_child_calls_observed + stats.probe_successes, artifact_sha256: artifactHashes,
  module_set_sha256: hash(['a', 'b', 'dispatcher', 'wrong'].map(name => `${name}:${artifactHashes[`${name}.wasm`]}`).join('\n')),
  sources: Object.fromEntries(sourcePaths.map(path => [path, hash(readFileSync(join(root, path)))])),
  production: {git_head: execFileSync('git', ['rev-parse', 'HEAD'], {cwd: root, encoding: 'utf8'}).trim(), source_files: productionPaths.length, source_set_sha256: hash(productionPaths.map(path => `${path}:${hash(readFileSync(join(root, path)))}`).join('\n'))},
  mathematical_inputs: {cold: coldStart, warm: warmInputs, warm_cases: warmCases, split: resumedStart, large: {cycles: largeCycles, retired: largeCycles * 4, child_calls: largeCycles * 2}},
  tools: {node: process.version, v8: process.versions.v8, platform: process.platform, architecture: process.arch, rustc: execFileSync('rustc', ['--version'], {encoding: 'utf8'}).trim(), cargo: execFileSync('cargo', ['--version'], {encoding: 'utf8'}).trim()},
  commands: {run: [process.execPath, process.argv[1], outputDir, root], generate_and_run: ['cargo', 'test', '-p', 'ring3-engine', '--test', 'dbt_table_dispatch', '--', '--nocapture']},
  claim: 'test-only Node platform feasibility: two actual standalone engine-generated register-only Wasm units share memory and funcref table; returning call_indirect loop has no JavaScript function imports on warm transfers, one cold B installation, exact aggregate budget/cancel/interior continuations and full memory preservation, independent mathematical four-instruction cycle/INC/DEC flag oracle, million cycles through one dispatcher invocation, controlled missing units plus explicit null/bounds/signature platform traps. Fixed valid buffers only. No product cache/process guard/SMC/callback pinning/lifetime/browser/asynchronous cancellation/stack-size/performance claim.',
};
writeFileSync(join(outputDir, 'provenance.json'), JSON.stringify(provenance, null, 2));
console.log(JSON.stringify({...provenance, sources: Object.keys(provenance.sources).length, artifacts: outputDir}));
