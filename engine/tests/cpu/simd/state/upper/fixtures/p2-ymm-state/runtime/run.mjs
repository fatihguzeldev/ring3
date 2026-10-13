import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../../../../../../../support/wasm/engine.mjs';
import {assertEngineExports} from '../../../../../../../support/wasm/exports.mjs';

const [enginePath, output, root] = process.argv.slice(2);
const engine = readEngine(enginePath);
const initial = readFileSync(join(output, 'initial-arena.bin'));
const descriptors = readFileSync(join(output, 'fx-descriptors.bin'));
const SIZE = 4756, UPPER = 4612, PAYLOAD = 4628, SV = 4364, T = 140;
const hash = value => createHash('sha256').update(value).digest('hex');
const first = new WebAssembly.Instance(engine.module, {});
assert.equal(typeof first.exports.ring3_abi_v1_arena_len, 'function', 'versioned upper arena length discovery exists');
assert.equal(first.exports.ring3_abi_v1_arena_len(), 0, 'no arena before open');
const exportsReceipt = assertEngineExports(engine.module, root);
assert.equal(initial.length, SIZE, 'native current complete arena');
assert.equal(descriptors.length, 24, 'two legacy FX descriptors');
const sourcePaths = [...new Set([
  ...exportsReceipt.sourcePaths,
  'engine/tests/support/wasm/engine.mjs',
  'engine/tests/cpu/simd/state/upper/native.rs',
  'engine/tests/cpu/simd/state/upper/wasm.rs',
  'engine/tests/cpu/simd/state/upper/fixtures/p2-ymm-state/runtime/run.mjs',
  'engine/src/abi/arena.rs', 'engine/src/abi/header.rs', 'engine/src/abi/x86/mod.rs',
  'engine/src/abi/x86/ymm.rs', 'engine/src/abi/x86/simd.rs',
  'engine/src/cpu/x86/simd.rs', 'engine/src/cpu/x86/simd/ymm.rs',
  'engine/src/cpu/x86/capabilities.rs', 'engine/src/cpu/x86/xstate.rs',
  'engine/src/cpu/dbt/wasm/abi.rs', 'engine/src/cpu/dbt/wasm/simd.rs',
  'engine/src/cpu/dbt/wasm/simd/direct.rs', 'engine/src/cpu/dbt/wasm/xstate.rs',
  'engine/src/process/instance.rs', 'engine/src/process/wasm/mod.rs',
  'engine/src/process/calls/context.rs', 'engine/src/process/calls/call.rs',
  'engine/src/process/calls/callback.rs', 'engine/src/process/calls/resident_callback.rs',
  'engine/src/process/loading/startup.rs', 'engine/src/process/cpu/simd.rs',
  'engine/src/process/memory/span.rs',
])];
const pinSources = () => Object.fromEntries(sourcePaths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sources = pinSources();
const rows = [], frames = [], modules = [];
let contextCount = 0, generated = 0;
const record = (magic, version, size, fields = []) => {
  const bytes = Buffer.alloc(size);
  bytes.write(magic); bytes.writeUInt16LE(version, 4); bytes.writeUInt16LE(1, 6); bytes.writeUInt32LE(size, 8);
  fields.forEach((value, index) => bytes.writeUInt32LE(value >>> 0, 16 + index * 4));
  return bytes;
};
const state = (registers, pc, flags = 0xcd7) => record('R3ST', 1, 56, [...registers, pc, flags]);
const exit = (version, reason, retired, detail = 0) => record('R3EX', version, 40, [reason, retired, detail, 0, 0, 0]);
const packet = (length = 0, status = 0, detail = 0, value = 0) => record('R3MH', 7, 40, [status, value, detail, 0, 0, length]);
const words = values => { const bytes = Buffer.alloc(values.length * 4); values.forEach((v, i) => bytes.writeUInt32LE(v >>> 0, i * 4)); return bytes; };
const upper = seed => {
  const bytes = record('R3YU', 1, 144);
  for (let i = 0; i < 128; i++) bytes[16 + i] = seed === 0 ? 0 : seed === 1 ? 255 : (i * 73 + seed * 11) & 255;
  return bytes;
};
function fresh(seed) {
  const instance = new WebAssembly.Instance(engine.module, {});
  const api = Object.fromEntries(exportsReceipt.functions.map(({name, arity}) => {
    assert.equal(instance.exports[name].length, arity, name);
    return [name.slice('ring3_abi_v1_'.length), instance.exports[name]];
  }));
  const c = {api, memory: instance.exports.memory, low: ++contextCount, high: 0x59554d4d, id: contextCount, seed};
  assert.equal(api.arena_len(), 0);
  assert.equal(api.open(4, c.low, c.high), 0);
  c.base = api.arena_ptr() >>> 0;
  assert.equal(api.arena_len(), SIZE);
  assert.ok(c.base > 0 && c.base + SIZE <= c.memory.buffer.byteLength);
  assert.deepEqual(arena(c), initial, 'exact initialized current arena');
  for (const [address, permissions] of [[0x1000, 7], [0x5000, 3], [0x8000, 3]]) assert.equal(api.map(address, 1, permissions), 0);
  input(c, UPPER, upper(seed));
  input(c, SV + 32, Buffer.from(Array.from({length: 128}, (_, i) => (i * 17 + 0x31) & 255)));
  upload(c, 0x8010, words([0x2000]));
  return c;
}
function arena(c) { return Buffer.from(new Uint8Array(c.memory.buffer, c.base, SIZE)); }
function input(c, offset, bytes) { new Uint8Array(c.memory.buffer, c.base + offset, bytes.length).set(bytes); }
function checked(c, label, expected, action, status = 0) {
  const before = arena(c); const actual = action(); assert.equal(actual, status, label); const after = arena(c);
  assert.deepEqual(after, expected, `${label}: all4756 bytes`);
  rows.push({context: c.id, label, status: actual, before: frames.length, after: frames.length + 1}); frames.push(before, after);
}
function unchanged(c, label, action, status = 0) { checked(c, label, arena(c), action, status); }
function upload(c, address, bytes) { input(c, T, bytes); unchanged(c, `upload${address.toString(16)}`, () => c.api.upload(address, bytes.length)); }
function compile(c, code) {
  upload(c, 0x1000, Buffer.from(code, 'hex')); input(c, T, words([0x1000, code.length / 2]));
  unchanged(c, `compile${code}`, () => c.api.compile(1));
  const pointer = c.api.module_ptr() >>> 0, length = c.api.module_len() >>> 0;
  assert.ok(pointer > 0 && length > 8 && pointer + length <= c.memory.buffer.byteLength);
  const bytes = Buffer.from(new Uint8Array(c.memory.buffer, pointer, length)); const module = new WebAssembly.Module(bytes);
  const imports = WebAssembly.Module.imports(module);
  assert.ok(imports.every(row => row.module === 'env' && row.name === 'memory' && row.kind === 'memory' || row.module === 'ring3' && row.kind === 'function' && typeof c.api[row.name] === 'function'));
  assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  const run = new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3: c.api}).exports.run;
  modules.push({context: c.id, code, bytes: length, sha256: hash(bytes), imports});
  return run;
}
function seedState(c, pc = 0x1000, changes = {}) {
  const registers = [0x12345678, 0, 0x9abcdef0, 0x23456789, 0x8010, 0x3456789a, 0x456789ab, 0x5000];
  for (const [index, value] of Object.entries(changes)) registers[Number(index)] = value;
  input(c, 0, state(registers, pc)); return registers;
}
function run(c, label, fn, registers, codeLength, version, amend = () => {}) {
  const expected = arena(c); expected.set(state(registers, 0x1000 + codeLength), 0); expected.set(exit(version, 1, 1), 56); amend(expected);
  checked(c, label, expected, () => fn(c.base, c.base + 56, 1, c.base + 96)); generated++;
}
function legacyFx(c) {
  upload(c, 0x5000, Buffer.alloc(512, 0x9b));
  const before = arena(c), image = Buffer.from(before.subarray(T, T + 512)); image.fill(0, 0, 464);
  image.writeUInt16LE(0x037f, 0); image.writeUInt32LE(0x1f80, 24); image.writeUInt32LE(0xffff, 28); before.copy(image, 160, SV + 32, SV + 160);
  let expected = Buffer.from(before); expected.set(image, T); expected.set(words([0, 0xcd7]), T + 512); expected.set(packet(), 100);
  const values = [0, 4, 8].map(offset => descriptors.readUInt32LE(offset));
  checked(c, 'legacyFXSAVEcodec', expected, () => c.api.simd_execute(...values, 0, 0, 0xcd7));
  expected = arena(c); expected.set(packet(512), 100);
  checked(c, 'legacyFXSAVEmemory', expected, () => c.api.cpu_fxsave(0x5000));
  const beforeRead = arena(c); const readExpected = Buffer.from(beforeRead); const savedRam = Buffer.alloc(512, 0x9b); image.copy(savedRam, 0, 0, 464); readExpected.set(savedRam, T); readExpected.set(packet(512), 100);
  checked(c, 'legacyFXSAVEsoftwaretail', readExpected, () => c.api.cpu_read(0x5000, 512, 1));
  input(c, SV + 32, Buffer.alloc(128, 0x5a));
  expected = arena(c); expected.set(before.subarray(SV, SV + 160), SV); expected.set(packet(), 100); expected.set(words([0, 0xcd7]), T + 512);
  const restoreValues = [12, 16, 20].map(offset => descriptors.readUInt32LE(offset));
  checked(c, 'legacyFXRSTORcodec', expected, () => c.api.simd_execute(...restoreValues, 0, 0, 0xcd7));
  input(c, T + 24, words([0x10000])); expected = arena(c); expected.set(packet(0, 3), 100);
  checked(c, 'legacyFXRSTORinvalidMXCSR', expected, () => c.api.simd_execute(...restoreValues, 0, 0, 0xcd7));
}
function callback(c, finish) {
  for (const [address, bytes] of [[0x1000, '0f0b'], [0x1100, '90'], [0x1200, '0f0b']]) upload(c, address, Buffer.from(bytes, 'hex'));
  input(c, T, words([0x1000, 2, 0x1100, 1, 0x1200, 2, 0x1000, 17, 0x1200, 18]));
  unchanged(c, 'compileCallbackGates', () => c.api.compile_with_gates(3, 2)); const generation = c.api.generation();
  const registers = seedState(c); input(c, 56, exit(3, 8, 1, 17));
  const outerState = arena(c).subarray(0, 96); const outerUpper = arena(c).subarray(UPPER);
  // stop injection exercises the host lifecycle boundary; it does not claim VEX execution.
  let expected = arena(c); const outerToken = finish ? 3 : 1;
  expected.set(record('R3CF', 1, 112, [outerToken, 17, 1, 0, 0x1000, 0x8010, 0x2000, 0]), T);
  checked(c, 'captureOuter', expected, () => c.api.capture_call(c.low, c.high, generation, 1, 0));
  input(c, PAYLOAD, Buffer.from([arena(c)[PAYLOAD] ^ 0x80]));
  unchanged(c, 'changedUpperRejectsCompletion', () => c.api.complete_call(c.low, c.high, generation, outerToken, 42), 15);
  input(c, UPPER, outerUpper);
  const callbackToken = outerToken + 1, beginRegisters = [...registers]; beginRegisters[4] = 0x800c;
  expected = arena(c); expected.set(state(beginRegisters, 0x1100), 0); expected.set(exit(3, 3, 0), 56);
  expected.set(record('R3CB', 1, 64, [callbackToken, outerToken, 1, 0, 0x1100, 0x800c, 0x1200, 18, 0, 0, generation]), T);
  checked(c, 'beginCallback', expected, () => c.api.begin_callback(c.low, c.high, generation, outerToken, 0x1100, 0x1200, 18, 0));
  input(c, PAYLOAD, Buffer.alloc(128, 0x6d));
  if (finish) {
    const returned = [...registers]; returned[0] = 0xcafebabe; input(c, 0, state(returned, 0x1200)); input(c, 56, exit(3, 8, 0, 18));
    input(c, UPPER + 12, words([1])); unchanged(c, 'malformedUpperRejectsFinish', () => c.api.finish_callback(c.low, c.high, generation, callbackToken), 13); input(c, UPPER + 12, words([0]));
    expected = arena(c); expected.set(outerState, 0); expected.set(outerUpper, UPPER);
    expected.set(record('R3CB', 1, 64, [callbackToken, outerToken, 2, 0, 0x1100, 0x800c, 0x1200, 18, 0, 0xcafebabe, generation]), T);
    checked(c, 'finishRestoresExactOuter', expected, () => c.api.finish_callback(c.low, c.high, generation, callbackToken));
  } else {
    expected = arena(c); expected.set(outerState, 0); expected.set(outerUpper, UPPER);
    checked(c, 'abortRestoresExactOuter', expected, () => c.api.abort_callback(c.low, c.high, callbackToken));
  }
  const completed = [...registers]; completed[0] = 42; completed[4] = 0x8014;
  expected = arena(c); expected.set(state(completed, 0x2000), 0); expected.set(exit(3, 3, 0), 56);
  checked(c, 'completeKeepsUpper', expected, () => c.api.complete_call(c.low, c.high, generation, outerToken, 42));
}
for (const seed of [0, 1, 2, 3]) {
  const c = fresh(seed);
  let fn = compile(c, '0f28c1'), registers = seedState(c);
  run(c, 'legacyMOVAPSkeepsUpper', fn, registers, 3, 6, expected => expected.copy(expected, SV + 32, SV + 48, SV + 64));
  input(c, UPPER + 4, words([0x10002])); registers = seedState(c);
  run(c, 'legacyMOVAPSdoesNotConsumeUpperHeader', fn, registers, 3, 6, expected => expected.copy(expected, SV + 32, SV + 48, SV + 64));
  input(c, UPPER, upper(seed)); fn = compile(c, '0f01d0'); registers = seedState(c); registers[0] = 7; registers[2] = 0;
  run(c, 'XGETBVactiveMask7PreservesUpper', fn, registers, 3, 9);
  legacyFx(c); callback(c, false); callback(c, true);
  const closed = arena(c); assert.equal(c.api.close(), 0); assert.equal(c.api.arena_len(), 0); assert.equal(c.api.arena_ptr(), 0);
  assert.deepEqual(arena(c), closed, 'close retains current allocation bytes');
}
assert.deepEqual(pinSources(), sources, 'all owner pins stable');
writeFileSync(join(output, 'arenas.bin'), Buffer.concat(frames), {flag: 'wx'});
const result = {status: 'PASS', engine: {path: enginePath, sha256: engine.sha256}, counts: {contexts: contextCount, generated, modules: modules.length, full_frame_pairs: rows.length, frame_bytes: SIZE}, sources, modules, rows, limits: ['upper transport and legacy preservation; XGETBV follows the active CPU mask, guest AVX execution is covered separately', 'callback stops are explicit lifecycle inputs; no new VEX execution or general callback coverage claim', 'all observed input/output frames are4756 bytes; the legacy4612 prefix stays byteidentical at initialization']};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2), {flag: 'wx'});
console.log(JSON.stringify({status: result.status, ...result.counts, output}));
