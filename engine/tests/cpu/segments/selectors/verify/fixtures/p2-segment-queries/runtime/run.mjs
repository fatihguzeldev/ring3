import {generatedImports, attachDetectionApi} from '../../../../../../../support/wasm/binding.mjs';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../../../../../../../support/wasm/engine.mjs';

const [enginePath, output, root] = process.argv.slice(2), engine = readEngine(enginePath);
const PC = 0x1000, SEG = 4524, SIZE = 4612, keyHigh = 0x565252;
const counts = {contexts: 0, modules: 0, runs: 0, registers: 0, memory: 0, faults: 0, controls: 0};
const paths = [
  'engine/src/cpu/x86/segments/descriptors.rs', 'engine/src/cpu/x86/decode/segment_queries.rs', 'engine/src/cpu/x86/decode/decoder.rs',
  'engine/src/cpu/x86/ir.rs', 'engine/src/cpu/dbt/region.rs', 'engine/src/cpu/dbt/wasm/memory.rs', 'engine/src/cpu/dbt/wasm/memory/narrow.rs',
  'engine/src/cpu/dbt/wasm/segments.rs', 'engine/src/cpu/dbt/wasm/segment_queries.rs', 'engine/src/cpu/dbt/wasm/integer/dispatch.rs',
  'engine/src/process/cpu/integer.rs', 'engine/src/process/cpu/segment_queries.rs',
  'engine/tests/cpu/segments/selectors/verify/native.rs', 'engine/tests/cpu/segments/selectors/verify/wasm.rs', 'engine/tests/cpu/segments/selectors/verify/fixtures/p2-segment-queries/runtime/run.mjs', 'engine/tests/support/wasm/binding.mjs',
];
const pins = () => Object.fromEntries(paths.map(path => [path, createHash('sha256').update(readFileSync(join(root, path))).digest('hex')]));
const sourcePins = pins(), words = values => {const b = Buffer.alloc(values.length * 4); values.forEach((v, i) => b.writeUInt32LE(v >>> 0, i * 4)); return b;};
function record(magic, size, fields, version = 1) {
  const b = Buffer.alloc(size); b.write(magic); b.writeUInt16LE(version, 4); b.writeUInt16LE(1, 6); b.writeUInt32LE(size, 8);
  fields.forEach((v, i) => b.writeUInt32LE(v >>> 0, 16 + i * 4)); return b;
}
const cpu = s => record('R3ST', 56, [...s.r, s.pc, s.flags], s.flags & ~0xcd7 ? 2 : 1);
const arena = c => Buffer.from(new Uint8Array(c.memory.buffer, c.base, SIZE));
function input(c, at, b) {new Uint8Array(c.memory.buffer).set(b, c.base + at);}
function host(c, name, args = []) {assert.equal(c.api[name](...args), 0, name);}
function upload(c, address, b) {input(c, 140, b); host(c, 'upload', [address, b.length]);}
function put(c, address, selector) {const b = Buffer.alloc(2); b.writeUInt16LE(selector); upload(c, address, b);}
function seed(c, updates = {}, flags = 0x244cd7) {
  const s = {r: Array.from({length: 8}, (_, i) => (0xa5101234 + i * 0x10001) >>> 0), pc: PC, flags};
  for (const [index, value] of Object.entries(updates)) s.r[+index] = value >>> 0;
  input(c, 0, cpu(s)); input(c, 96, words([0])); return s;
}
function open(owner, entries) {
  const instance = new WebAssembly.Instance(engine.module, {}), c = {owner, entries, memory: instance.exports.memory, api: {}, calls: []};
  for (const [name, value] of Object.entries(instance.exports)) if (name.startsWith('ring3_abi_v1_') && typeof value === 'function') c.api[name.slice(13)] = value;
 attachDetectionApi(c.api, instance.exports);
  c.key = ++counts.contexts; host(c, 'open', [8, c.key, keyHigh]); c.base = c.api.arena_ptr() >>> 0;
  for (const args of [[PC, 1, 7], [0x5000, 1, 3], [0xf000, 1, 3]]) host(c, 'map', args);
  c.initialSegments = arena(c).subarray(SEG, SEG + 64); return c;
}
function compile(c, instruction, write, memory = false) {
  if (c.previousResident) {input(c, 96, words([0])); host(c, 'discard_unacknowledged_resident', [c.key, keyHigh, ...c.previousResident]);}
  upload(c, PC, Buffer.from([...instruction, 0xeb, 0])); input(c, 140, words(c.entries ? [PC] : [PC, instruction.length + 2]));
  const method = c.owner === 'resident' ? (c.entries ? 'compile_resident_entries' : 'compile_resident') : (c.entries ? 'compile_entries' : 'compile');
  host(c, method, [1, ...(c.entries ? [0] : [])]);
  const b = arena(c), pointer = c.owner === 'resident' ? b.readUInt32LE(156) : c.api.module_ptr() >>> 0;
  if (c.owner === 'resident') c.previousResident = [b.readUInt32LE(148), b.readUInt32LE(152)];
  c.binding = c.owner === 'resident' ? [c.key, keyHigh, ...c.previousResident] : [];
  const length = c.owner === 'resident' ? b.readUInt32LE(160) : c.api.module_len() >>> 0;
  const bytes = Buffer.from(new Uint8Array(c.memory.buffer, pointer, length)), module = new WebAssembly.Module(bytes);
  const name = c.owner === 'resident' ? 'cpu_integer_resident' : 'cpu_integer';
  assert.deepEqual(WebAssembly.Module.imports(module).map(i => `${i.module}:${i.name}:${i.kind}`).sort(),
    generatedImports({resident: c.owner === 'resident', helpers: [name, ...(memory ? ["cpu_read16"] : [])]}).map(item => `${item.module}:${item.name}:${item.kind}`).sort());
  writeFileSync(join(output, `module-${++counts.modules}.wasm`), bytes, {flag: 'wx'});
  const ring3 = {...c.api};
  for (const helper of [name, ...(memory ? ['cpu_read16'] : [])]) ring3[helper] = (...args) => {
    const before = arena(c), status = c.api[helper](...args), after = arena(c), allowed = Buffer.from(before);
    allowed.set(after.subarray(100, helper === 'cpu_read16' ? 140 : 164), 100);
    assert.deepEqual(after, allowed, 'query helper publication is restricted to packet and payload');
    c.calls.push({name: helper, status, args: args.map(v => v >>> 0), input: before.subarray(140, 164)}); return status;
  };
  return {run: new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3}).exports.run, instructionLength: instruction.length, write, memory};
}
function execute(c, fn, s, {success, selector, budget = 1, reason = 1, retired = 1, calls, packet, fault = [0, 0, 0, 0]} = {}) {
  const before = arena(c), expected = Buffer.from(before), at = c.calls.length;
  const n = {...s, r: [...s.r], pc: retired ? PC + fn.instructionLength : PC,
    flags: success === undefined ? s.flags : ((s.flags & ~0x40) | (success ? 0x40 : 0)) >>> 0};
  expected.set(cpu(n)); expected.set(record('R3EX', 40, [reason, retired, ...fault], reason === 16 ? 7 : 6), 56);
  const completed = success !== undefined;
  if (completed) {
    expected.set(record('R3MH', 40, [0, 0, 0, 0, 0, 0], 7), 100);
    expected.set(words([selector, n.flags, s.r[0], s.r[2], s.r[3], s.r[1]]), 140);
  } else if (packet) expected.set(record('R3MH', 40, packet, 10), 100);
  assert.equal(fn.run(c.base, c.base + 56, budget, c.base + 96), 0);
  assert.deepEqual(arena(c), expected, `whole arena write=${fn.write} selector=${selector} reason=${reason}`);
  const observed = c.calls.slice(at); assert.equal(observed.length, calls ?? (completed ? fn.memory ? 2 : 1 : 0));
  for (const call of observed) {
    assert.equal(call.status, 0);
    if (call.name !== 'cpu_read16') {
      assert.deepEqual(call.args.slice(-2), [0x30001 + Number(fn.write), 0]);
      assert.deepEqual(call.input, words([selector, s.flags, s.r[0], s.r[2], s.r[3], s.r[1]]));
    }
  }
  counts.runs++;
}
// Independent access matrix in ABI-kind order: readonly/writable data, execute-only/readable nonconforming,
// execute-only/readable conforming, and system. Presence deliberately does not enter the oracle.
const access = [1, 3, 0, 1, 0, 1, 0];
function oracle(kind, dpl, rpl, write) {return Boolean(access[kind] & (write ? 2 : 1)) && (kind >= 4 && kind <= 5 || dpl >= Math.max(3, rpl));}
function cached(c, segment, bits) {
  const b = Buffer.from(c.initialSegments); b.writeUInt16LE(2, 4);
  [0xf33fffff, 0xf3bfffff, 0xf33fffff, 0xf33fffff, 0xf33fffff, 0].forEach((v, i) => b.writeUInt32LE(v >>> 0, [28, 32, 36, 40, 44, 56][i]));
  b.writeUInt32LE(bits >>> 0, [28, 32, 36, 40, 44, 56][segment]); input(c, SEG, b);
}
for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries);
  for (const write of [false, true]) for (let source = 0; source < 8; source++) {
    const field = write ? 0x28 : 0x20, fn = compile(c, [0x0f, 0x00, 0xc0 | field | source], write);
    for (let kind = 0; kind < 7; kind++) for (let dpl = 0; dpl < 4; dpl++) for (const present of [0, 1]) for (const ti of [0, 4]) {
      host(c, 'configure_segment_descriptor', [0x40 | ti, 0x12340000, dpl, present, kind]);
      for (let rpl = 0; rpl < 4; rpl++) {
        const selector = 0x40 | ti | rpl, s = seed(c, {[source]: 0xa5b60000 | selector}, rpl % 2 ? 2 : 0x244cd7);
        execute(c, fn, s, {success: oracle(kind, dpl, rpl, write), selector}); counts.registers++;
      }
    }
    for (const selector of [0, 1, 2, 3, 0x53, 0xffff]) {
      const s = seed(c, {[source]: 0x9abc0000 | selector}); execute(c, fn, s, {success: false, selector}); counts.registers++;
    }
    for (const prefix of [[0x66], [0x67], [0x2e]]) {
      const ignored = compile(c, [...prefix, 0x0f, 0x00, 0xc0 | field | source], write);
      const s = seed(c, {[source]: 0x9abc0023}); execute(c, ignored, s, {success: true, selector: 0x23}); counts.registers++;
    }
  }
  for (const write of [false, true]) {
    const field = write ? 0x28 : 0x20;
    for (const row of [
      {bytes: [0x0f, 0x00, field | 7], updates: {7: 0x5010}, address: 0x5010},
      {bytes: [0x66, 0x0f, 0x00, field | 7], updates: {7: 0x5010}, address: 0x5010},
      {bytes: [0x0f, 0x00, field | 0x45, 0], updates: {5: 0x5010}, address: 0x5010},
      {bytes: [0x64, 0x0f, 0x00, field | 7], updates: {7: 0x10}, address: 0x5010, fs: true},
      {bytes: [0x67, 0x0f, 0x00, field], updates: {3: 0xabcd5000, 6: 0x12340010}, address: 0x5010},
      {bytes: [0x65, 0x67, 0x0f, 0x00, field | 6, 0x10, 0x50], updates: {}, address: 0x5010, gs: true},
    ]) {
      input(c, SEG, c.initialSegments); if (row.fs) host(c, 'set_segment_base', [4, 0x5000]);
      if (row.gs) host(c, c.owner === 'resident' ? 'segment_load_resident' : 'segment_load', [...c.binding, 5, 0x23]);
      const fn = compile(c, row.bytes, write, true); put(c, row.address, 0x23);
      let s = seed(c, row.updates); execute(c, fn, s, {success: true, selector: 0x23}); counts.memory++;
      s = seed(c, row.updates, 0x244dd7); execute(c, fn, s, {success: true, selector: 0x23, reason: 16}); counts.controls++;
      s = seed(c, row.updates); execute(c, fn, s, {budget: 0, retired: 0}); input(c, 96, words([1])); execute(c, fn, s, {reason: 2, retired: 0}); counts.controls += 2;
    }
  }
  input(c, SEG, c.initialSegments);
  let fn = compile(c, [0x0f, 0x00, 0x27], false, true), s;
  put(c, 0x5ffe, 0x23); s = seed(c, {7: 0x5ffe}); execute(c, fn, s, {success: true, selector: 0x23}); counts.memory++;
  upload(c, 0x5fff, Buffer.from([0x23])); s = seed(c, {7: 0x5fff});
  execute(c, fn, s, {reason: 5, retired: 0, calls: 1, packet: [1, 0, 1, 0x6000, 1, 2], fault: [1, 0x6000, 1, 2]}); counts.faults++;
  host(c, 'map', [0x6000, 1, 3]); upload(c, 0x6000, Buffer.from([0]));
  s = seed(c, {7: 0x5fff}); execute(c, fn, s, {success: true, selector: 0x23}); counts.memory++;
  host(c, 'protect', [0x5000, 1, 0]); s = seed(c, {7: 0x5010});
  execute(c, fn, s, {reason: 5, retired: 0, calls: 1, packet: [1, 0, 2, 0x5010, 1, 2], fault: [2, 0x5010, 1, 2]}); counts.faults++;
  host(c, 'protect', [0x5000, 1, 3]);
  for (const row of [
    {bytes: [0x0f, 0x00, 0x27], segment: 3, updates: {7: 0x7000}, reason: 11},
    {bytes: [0x0f, 0x00, 0x65, 0], segment: 2, updates: {5: 0x7000}, reason: 15},
  ]) {
    cached(c, row.segment, 0xf2305000); fn = compile(c, row.bytes, false, true); s = seed(c, row.updates);
    execute(c, fn, s, {reason: row.reason, retired: 0}); counts.faults++;
  }
  input(c, SEG, c.initialSegments); fn = compile(c, [0x0f, 0x00, 0x27], false, true);
  host(c, c.owner === 'resident' ? 'segment_load_resident' : 'segment_load', [...c.binding, 3, 0]);
  s = seed(c, {7: 0x7000}); execute(c, fn, s, {reason: 11, retired: 0}); counts.faults++;
  input(c, SEG, c.initialSegments);
  // Query the current table after loading a writable cache, then changing that table entry to readonly/not-present.
  host(c, 'configure_segment_descriptor', [0x43, 0, 3, 1, 1]);
  host(c, c.owner === 'resident' ? 'segment_load_resident' : 'segment_load', [...c.binding, 3, 0x43]);
  const loaded = arena(c).subarray(SEG, SEG + 64);
  host(c, 'configure_segment_descriptor', [0x43, 0, 3, 0, 0]);
  fn = compile(c, [0x0f, 0x00, 0xe8], true); s = seed(c, {0: 0x43}); execute(c, fn, s, {success: false, selector: 0x43});
  assert.deepEqual(arena(c).subarray(SEG, SEG + 64), loaded); counts.controls++;
  fn = compile(c, [0x0f, 0x00, 0xe0], false); s = seed(c, {0: 0x43}); execute(c, fn, s, {success: true, selector: 0x43}); counts.controls++;
  for (const [descriptor, selector, address] of [[0x30001, 0x23, 1], [0x30002, 0x10023, 0], [0x30005, 0x23, 0]]) {
    input(c, 140, words([selector, 0x244cd7, 1, 2, 3, 4])); const before = arena(c);
    assert.notEqual(c.api[c.owner === 'resident' ? 'cpu_integer_resident' : 'cpu_integer'](...c.binding, descriptor, address), 0);
    assert.deepEqual(arena(c), before); counts.controls++;
  }
  host(c, 'close');
}
assert.equal(counts.contexts, 4); assert.equal(counts.registers, 29248); assert.equal(counts.memory, 56); assert.equal(counts.faults, 20);
assert.deepEqual(pins(), sourcePins, 'query sources stayed fixed');
writeFileSync(join(output, 'result.json'), JSON.stringify({status: 'ok', engine_sha256: engine.sha256, source_pins: sourcePins, counts,
  limits: ['configured software descriptor table, finite VERR/VERW proof; AM0 environment; no guest-GDT memory or kernel execution claim']}, null, 2) + '\n');
console.log(JSON.stringify({status: 'ok', ...counts, output}));
