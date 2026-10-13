import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {createArena, ARENA_SIZE} from '../../../../../../../support/wasm/arena.mjs';
import {readEngine} from '../../../../../../../support/wasm/engine.mjs';
import {assertEngineExports} from '../../../../../../../support/wasm/exports.mjs';

const [enginePath, output, root] = process.argv.slice(2);
const engine = readEngine(enginePath), {functions: exportFunctions, sourcePaths: exportSources} = assertEngineExports(engine.module, root);
const keyHigh = 0x55443031, transfer = 140;
const counts = {contexts: 0, modules: 0, faults: 0, controls: 0, refusals: 0, fullRamPairs: 0};
const observations = [];
const sourcePaths = [...exportSources,
  'engine/tests/support/wasm/arena.mjs', 'engine/tests/support/wasm/engine.mjs',
  'engine/src/cpu/x86/decode/control.rs', 'engine/src/cpu/x86/decode/decoder.rs', 'engine/src/cpu/x86/decode/profile.rs',
  'engine/src/cpu/x86/control.rs', 'engine/src/cpu/dbt/region.rs', 'engine/src/cpu/dbt/wasm/control_instructions.rs',
  'engine/src/cpu/dbt/wasm/integer/dispatch.rs', 'engine/src/cpu/dbt/wasm/emitter.rs', 'engine/src/cpu/dbt/wasm/abi.rs',
  'engine/src/abi/x86/state.rs', 'engine/src/abi/x86/exit.rs',
  'engine/tests/cpu/control/exceptions/undefined/native.rs', 'engine/tests/cpu/control/exceptions/undefined/wasm.rs',
  'engine/tests/cpu/control/exceptions/undefined/fixtures/p2-undefined/runtime/run.mjs'];
assert.equal(new Set(sourcePaths).size, sourcePaths.length);
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const pins = () => Object.fromEntries(sourcePaths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = pins();
const words = values => {const bytes = Buffer.alloc(values.length * 4); values.forEach((value, i) => bytes.writeUInt32LE(value >>> 0, i * 4)); return bytes;};
const arena = c => Buffer.from(new Uint8Array(c.memory.buffer, c.base, ARENA_SIZE));
const put = (c, at, bytes) => new Uint8Array(c.memory.buffer).set(bytes, c.base + at);
function host(c, name, args = []) {assert.equal(c.api[name](...args), 0, name);}
function upload(c, address, bytes) {put(c, transfer, bytes); host(c, 'upload', [address, bytes.length]);}
function open(resident, entries) {
  const instance = new WebAssembly.Instance(engine.module, {}), c = {resident, entries, api: {}, memory: instance.exports.memory};
  for (const [name, value] of Object.entries(instance.exports)) if (name.startsWith('ring3_abi_v1_')) c.api[name.slice(13)] = value;
  const arities = {open: 3, close: 0, arena_ptr: 0, arena_len: 0, map: 3, upload: 2, compile: 1, compile_entries: 2,
    compile_resident: 1, compile_resident_entries: 2, discard_unacknowledged_resident: 4, module_ptr: 0, module_len: 0,
    guard: 6, guard_resident: 7, cpu_read: 3};
  for (const [name, arity] of Object.entries(arities)) {
    assert.equal(c.api[name].length, arity, name);
    assert.equal(exportFunctions.find(row => row.name === `ring3_abi_v1_${name}`)?.arity, arity, `${name}: declared arity`);
  }
  c.key = ++counts.contexts; host(c, 'open', [4, c.key, keyHigh]); c.base = c.api.arena_ptr() >>> 0;
  assert.equal(c.api.arena_len(), ARENA_SIZE);
  for (const args of [[0x1000, 1, 7], [0x5000, 1, 3]]) host(c, 'map', args);
  upload(c, 0x5000, Buffer.alloc(4096, 0x59));
  return c;
}
function compile(c, row) {
  put(c, 96, words([0]));
  if (c.unit) host(c, 'discard_unacknowledged_resident', [c.key, keyHigh, ...c.unit]);
  const code = Buffer.from(row.code, 'hex'), pc = row.boundary ? 0x2000 - code.length : 0x1000;
  upload(c, pc, code);
  put(c, transfer, words(c.entries ? [pc] : [pc, code.length]));
  const name = c.resident ? c.entries ? 'compile_resident_entries' : 'compile_resident' : c.entries ? 'compile_entries' : 'compile';
  assert.equal(c.api[name](1, ...(c.entries ? [0] : [])), 0, `${row.name}: defined undefined instruction admission`);
  const frame = arena(c);
  if (c.resident) c.unit = [frame.readUInt32LE(transfer + 8), frame.readUInt32LE(transfer + 12)];
  const pointer = c.resident ? frame.readUInt32LE(transfer + 16) : c.api.module_ptr() >>> 0;
  const length = c.resident ? frame.readUInt32LE(transfer + 20) : c.api.module_len() >>> 0;
  const binary = Buffer.from(new Uint8Array(c.memory.buffer, pointer, length));
  writeFileSync(join(output, `module-${++counts.modules}.wasm`), binary, {flag: 'wx'});
  const module = new WebAssembly.Module(binary);
  const guard = c.resident ? 'guard_resident' : 'guard';
  assert.deepEqual(WebAssembly.Module.imports(module).map(item => `${item.module}:${item.name}:${item.kind}`).sort(),
    ['env:memory:memory', `ring3:${guard}:function`].sort(), `${row.name}: no operand helper or segment import`);
  assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  let guards = 0;
  const ring3 = {[guard]: (...args) => {guards++; return c.api[guard](...args);}};
  return {run: new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3}).exports.run, pc, guards: () => guards};
}
function ram(c) {
  const saved = arena(c), result = Buffer.alloc(8192);
  for (const [page, address] of [0x1000, 0x5000].entries()) for (let offset = 0; offset < 4096; offset += 512) {
    host(c, 'cpu_read', [address + offset, 512, 1]);
    const frame = arena(c); assert.equal(frame.readUInt32LE(116), 0);
    frame.copy(result, page * 4096 + offset, transfer, transfer + 512);
  }
  put(c, 0, saved); return result;
}
function execute(c, compiled, row, mode) {
  const seed = createArena();
  const flags = mode === 'step' ? 0x244fd7 : 2, shadow = mode === 'fault' ? 0 : 1;
  [0xffffffff, 0xfffeffff, 0x89abcdef, 0xffffffff, 0xffffffff, 0xabcdef12, 0xffffffff, 0xffff].forEach((value, i) => seed.writeUInt32LE(value >>> 0, 16 + i * 4));
  seed.writeUInt32LE(compiled.pc, 48); seed.writeUInt32LE(flags, 52); seed.writeUInt32LE(shadow, 12);
  if (flags & ~0xcd7 || shadow) seed.writeUInt16LE(2, 4);
  for (const segment of [3, 4, 5]) seed.writeUInt16LE(0, 4540 + segment * 2);
  for (let i = 0; i < 8; i++) {seed.fill(0x30 + i, 4396 + i * 16, 4412 + i * 16); seed.fill(0x70 + i, 4628 + i * 16, 4644 + i * 16);}
  seed.fill(0xa5, 140, 4236);
  if (mode === 'cancel') seed.writeUInt32LE(1, 96);
  put(c, 0, seed);
  const before = arena(c), beforeRam = ram(c), expected = Buffer.from(before), guards = compiled.guards();
  const control = mode === 'budget' || mode === 'cancel', reason = mode === 'cancel' ? 2 : mode === 'budget' ? 1 : 20;
  const exit = Buffer.alloc(40); exit.write('R3EX'); exit.writeUInt16LE(control ? 1 : 7, 4); exit.writeUInt16LE(1, 6);
  exit.writeUInt32LE(40, 8); exit.writeUInt32LE(reason, 16); expected.set(exit, 56);
  if (!control) expected.writeUInt32LE(0, 12);
  assert.equal(compiled.run(c.base, c.base + 56, mode === 'budget' ? 0 : 1, c.base + 96), 0, `${row.name}/${mode}`);
  assert.deepEqual(arena(c), expected, `${row.name}/${mode}: complete CPU/exit/helper/extended arena`);
  assert.equal(compiled.guards() - guards, 1);
  assert.deepEqual(ram(c), beforeRam, `${row.name}/${mode}: all mapped guest bytes unchanged`);
  assert.deepEqual(arena(c), expected, `${row.name}/${mode}: diagnostics preserve complete arena`);
  observations.push({name: row.name, mode, resident: c.resident, entries: c.entries, reason, pc: compiled.pc, retired: 0});
  counts.fullRamPairs++; if (control) counts.controls++; else counts.faults++;
}
const rows = [{name: 'unchanged-ud2-control', code: '0f0b'}];
for (const [name, opcode] of [['ud0', 'ff'], ['ud1', 'b9']]) {
  rows.push(...[
    ['register32', `0f${opcode}c1`], ['register16', `660f${opcode}c1`],
    ['null-ds-unmapped-base', `0f${opcode}03`], ['unmapped-ss-base', `0f${opcode}0424`],
    ['null-fs', `640f${opcode}00`], ['null-gs-word', `65660f${opcode}00`],
    ['address16-word', `67660f${opcode}00`], ['full-offset-overflow-operand-ignored', `0f${opcode}05ffffffff`],
    ['single-repeat-prefix', `f30f${opcode}c0`],
  ].map(([kind, code]) => ({name: `${name}-${kind}`, code})));
  rows.push({name: `${name}-exact-last-code-byte`, code: `0f${opcode}c0`, boundary: true});
}
for (const resident of [false, true]) for (const entries of [false, true]) {
  const c = open(resident, entries);
  for (const row of rows) {
    const compiled = compile(c, row);
    for (const mode of ['fault', 'step', 'budget', 'cancel']) execute(c, compiled, row, mode);
  }
  for (const opcode of ['ff', 'b9']) for (const code of [`660f${opcode}c0`, `f20f${opcode}c0`]) {
    put(c, 96, words([0]));
    const bad = Buffer.from(`${code.slice(0, 2)}${code}`, 'hex');
    upload(c, 0x1000, bad); put(c, transfer, words(c.entries ? [0x1000] : [0x1000, bad.length]));
    const before = arena(c), name = c.resident ? c.entries ? 'compile_resident_entries' : 'compile_resident' : c.entries ? 'compile_entries' : 'compile';
    assert.equal(c.api[name](1, ...(c.entries ? [0] : [])), 10, 'duplicate profile prefix stays strict');
    assert.deepEqual(arena(c), before, 'failed preparation leaves complete arena unchanged'); counts.refusals++;
  }
  host(c, 'close');
}
assert.equal(counts.modules, rows.length * 4); assert.equal(counts.faults, rows.length * 8);
assert.equal(counts.controls, rows.length * 8); assert.equal(counts.fullRamPairs, rows.length * 16);
assert.deepEqual(pins(), sourcePins);
const result = {engineSha256: engine.sha256, counts, sourcePins, observations,
  claim: 'UD0/UD1 defined traps retire zero instructions at their current EIP, without operand/segment effects; complete encoding fetch is the deterministic software policy'};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2) + '\n', {flag: 'wx'});
console.log(JSON.stringify({engineSha256: engine.sha256, counts, rows: rows.length}));
