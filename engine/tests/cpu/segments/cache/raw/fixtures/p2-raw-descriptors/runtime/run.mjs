import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../../../../../../../support/wasm/engine.mjs';
import {assertEngineExports} from '../../../../../../../support/wasm/exports.mjs';

const [enginePath, output, root] = process.argv.slice(2), engine = readEngine(enginePath);
const {functions, sourcePaths: exportSources} = assertEngineExports(engine.module, root);
assert.equal(functions.find(row => row.name === 'ring3_abi_v1_configure_segment_descriptor_raw').arity, 3);
const SIZE = 4612, SEG = 4524, PC = 0x1000, keyHigh = 0x524157;
const counts = {contexts: 0, modules: 0, runs: 0, loads: 0, queries: 0, controls: 0};
const sourcePaths = [...new Set([...exportSources, 'engine/tests/support/wasm/engine.mjs',
  'engine/src/cpu/x86/segments/descriptor_record.rs', 'engine/src/cpu/x86/segments/descriptors.rs',
  'engine/src/cpu/x86/segments/cache.rs', 'engine/src/abi/x86/segments.rs', 'engine/src/process/cpu/segments.rs',
  'engine/src/process/wasm/cpu.rs', 'engine/src/process/wasm/mod.rs', 'engine/src/cpu/dbt/wasm/segments.rs', 'engine/src/cpu/dbt/wasm/segments/context.rs',
  'engine/src/process/cpu/segment_queries.rs', 'engine/src/process/cpu/integer.rs',
  'engine/src/cpu/dbt/wasm/segment_queries.rs',
  'engine/tests/cpu/segments/cache/raw/wasm.rs', 'engine/tests/cpu/segments/wasm.rs', 'engine/tests/support/wasm.rs',
  'engine/tests/cpu/segments/cache/raw/fixtures/p2-raw-descriptors/runtime/run.mjs'])];
const pins = () => Object.fromEntries(sourcePaths.map(path => [path, createHash('sha256').update(readFileSync(join(root, path))).digest('hex')]));
const sourcePins = pins(), words = values => {const b = Buffer.alloc(values.length * 4); values.forEach((v, i) => b.writeUInt32LE(v >>> 0, i * 4)); return b;};
function record(magic, size, fields, version) {
  const b = Buffer.alloc(size); b.write(magic); b.writeUInt16LE(version, 4); b.writeUInt16LE(1, 6); b.writeUInt32LE(size, 8);
  b.set(words(fields), 16); return b;
}
const packet = fields => record('R3MH', 40, fields, 7);
const cpu = s => record('R3ST', 56, [...s.r, s.pc, s.flags], 2);
const arena = c => Buffer.from(new Uint8Array(c.memory.buffer, c.base, SIZE));
function input(c, at, b) {new Uint8Array(c.memory.buffer).set(b, c.base + at);}
function host(c, name, args = []) {assert.equal(c.api[name](...args), 0, name);}
function upload(c, address, b) {input(c, 140, b); host(c, 'upload', [address, b.length]);}
function seed(c, selector) {
  const s = {r: [0xabcd0000 | selector, 0x12345678, 0x23456789, 0x3456789a, 0x6040, 0x456789ab, 0x5010, 0x6010], pc: PC, flags: 0x244cd7};
  input(c, 0, cpu(s)); input(c, 96, words([0])); return s;
}
function open(owner, entries) {
  const instance = new WebAssembly.Instance(engine.module, {}), c = {owner, entries, memory: instance.exports.memory, api: {}};
  for (const [name, value] of Object.entries(instance.exports)) if (name.startsWith('ring3_abi_v1_') && typeof value === 'function') c.api[name.slice(13)] = value;
  c.key = ++counts.contexts; host(c, 'open', [8, c.key, keyHigh]); c.base = c.api.arena_ptr() >>> 0;
  host(c, 'map', [PC, 1, 7]); return c;
}
function configure(c, low, high) {const before = arena(c); host(c, 'configure_segment_descriptor_raw', [0x43, low, high]); assert.deepEqual(arena(c), before);}
function compile(c, bytes, helper) {
  if (c.id) {input(c, 96, words([0])); host(c, 'discard_unacknowledged_resident', [c.key, keyHigh, ...c.id]);}
  upload(c, PC, Buffer.from([...bytes, 0xeb, 0])); input(c, 140, words(c.entries ? [PC] : [PC, bytes.length + 2]));
  const method = c.owner === 'resident' ? c.entries ? 'compile_resident_entries' : 'compile_resident' : c.entries ? 'compile_entries' : 'compile';
  host(c, method, [1, ...(c.entries ? [0] : [])]);
  const b = arena(c), pointer = c.owner === 'resident' ? b.readUInt32LE(156) : c.api.module_ptr() >>> 0;
  c.id = c.owner === 'resident' ? [b.readUInt32LE(148), b.readUInt32LE(152)] : null;
  c.binding = c.id ? [c.key, keyHigh, ...c.id] : [];
  const length = c.id ? b.readUInt32LE(160) : c.api.module_len() >>> 0;
  const moduleBytes = Buffer.from(new Uint8Array(c.memory.buffer, pointer, length)), module = new WebAssembly.Module(moduleBytes);
  const name = helper + (c.id ? '_resident' : '');
  assert.deepEqual(WebAssembly.Module.imports(module).map(row => `${row.module}:${row.name}:${row.kind}`).sort(),
    ['env:memory:memory', `ring3:${c.id ? 'guard_resident' : 'guard'}:function`, `ring3:${name}:function`].sort());
  writeFileSync(join(output, `module-${++counts.modules}.wasm`), moduleBytes, {flag: 'wx'});
  return {run: new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3: c.api}).exports.run, length: bytes.length};
}
function loaded(c, bits, base) {
  const b = Buffer.from(arena(c).subarray(SEG, SEG + 64)); b.writeUInt16LE(2, 4);
  for (const [offset, value] of [[28, 0xf33fffff], [32, 0xf3bfffff], [36, 0xf33fffff], [40, 0xf33fffff], [44, bits], [56, 0]]) b.writeUInt32LE(value >>> 0, offset);
  b.writeUInt16LE(0x43, 24); b.writeUInt32LE(base, 48); return b;
}
function runLoad(c, fn, selector, {bits, base = 0, status = 0, reason = 1} = {}) {
  const s = seed(c, selector), expected = arena(c), success = status === 0;
  expected.set(cpu({...s, pc: success ? PC + fn.length : PC}));
  expected.set(record('R3EX', 40, [reason, Number(success), success ? 0 : 0x40, 0, 0, 0], 6), 56);
  expected.set(packet([status, base, success ? 0 : 0x40, 0, 0, 0]), 100);
  if (success) expected.set(loaded(c, bits, base), SEG);
  assert.equal(fn.run(c.base, c.base + 56, 1, c.base + 96), 0);
  assert.deepEqual(arena(c), expected, 'raw load publishes only exact CPU, exit, packet and loaded cache'); counts.runs++; counts.loads++;
}
function verify(c, fn, selector, success) {
  const s = seed(c, selector), expected = arena(c), flags = ((s.flags & ~0x40) | (success ? 0x40 : 0)) >>> 0;
  expected.set(cpu({...s, pc: PC + fn.length, flags})); expected.set(record('R3EX', 40, [1, 1, 0, 0, 0, 0], 6), 56);
  expected.set(packet([0, 0, 0, 0, 0, 0]), 100); expected.set(words([selector, flags, s.r[0], s.r[2], s.r[3], s.r[1]]), 140);
  assert.equal(fn.run(c.base, c.base + 56, 1, c.base + 96), 0); assert.deepEqual(arena(c), expected, 'raw query changes only ZF and query payload, never loaded cache'); counts.runs++; counts.queries++;
}
for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries);
  let fn = compile(c, [0x8e, 0xe0], 'segment_load');
  configure(c, 0x50004321, 0x0050f200); runLoad(c, fn, 0x43, {bits: 0xfa304321, base: 0x5000});
  const first = arena(c).subarray(SEG, SEG + 64);
  configure(c, 0x60002345, 0x00c1f000); assert.deepEqual(arena(c).subarray(SEG, SEG + 64), first);
  runLoad(c, fn, 0x43, {bits: 0xf3112345, base: 0x6000});
  for (const [high, status, reason] of [[0x00407200, 6, 14], [0x0040d200, 3, 11], [0x0040e800, 3, 11], [0x0040e000, 3, 11]]) {
    configure(c, 0xffff, high); runLoad(c, fn, 0x43, {status, reason});
  }
  fn = compile(c, [0x8e, 0xd0], 'segment_load'); configure(c, 0xffff, 0x00407200);
  const s = seed(c, 0x43), expected = arena(c); expected.set(record('R3EX', 40, [15, 0, 0x40, 0, 0, 0], 6), 56); expected.set(packet([7, 0, 0x40, 0, 0, 0]), 100);
  assert.equal(fn.run(c.base, c.base + 56, 1, c.base + 96), 0); assert.deepEqual(arena(c), expected); counts.runs++; counts.loads++;
  for (const [target, low, high] of [[4, 0xffff, 0x0060f200]]) {
    configure(c, low, high); const before = arena(c);
    assert.notEqual(c.api[c.id ? 'segment_load_resident' : 'segment_load'](...c.binding, target, 0x43), 0);
    assert.deepEqual(arena(c), before, 'unsupported execution profiles are neutral host refusals'); counts.controls++;
  }
  const read = compile(c, [0x0f, 0x00, 0xe0], 'cpu_integer'); configure(c, 0x50004321, 0x00507200); verify(c, read, 0x43, true);
  configure(c, 0xdeadbeef, 0x89abec76); verify(c, read, 0x43, false);
  const write = compile(c, [0x0f, 0x00, 0xe8], 'cpu_integer'); configure(c, 0x50004321, 0x00507200); verify(c, write, 0x43, true);
  configure(c, 0xdeadbeef, 0x89abec76); verify(c, write, 0x43, false);
  for (const selector of [0, 1, 2, 3, 0x10043, 0xffffffff]) {
    const before = arena(c); assert.notEqual(c.api.configure_segment_descriptor_raw(selector, 0xffffffff, 0xffffffff), 0);
    assert.deepEqual(arena(c), before, 'malformed raw export selector leaves all arena bytes neutral'); counts.controls++;
  }
  host(c, 'close');
}
assert.equal(counts.contexts, 4); assert.equal(counts.modules, 16); assert.equal(counts.loads, 28); assert.equal(counts.queries, 16); assert.equal(counts.controls, 28);
assert.deepEqual(pins(), sourcePins);
writeFileSync(join(output, 'result.json'), JSON.stringify({status: 'ok', engine_sha256: engine.sha256, source_pins: sourcePins, counts,
  limits: ['raw table transport with actual emitted loads and VERR/VERW; native tests own A-bit table readback; no guest descriptor-memory or raw gates execution; LAR/LSL remains separate']}, null, 2) + '\n');
console.log(JSON.stringify({status: 'ok', ...counts, output}));
