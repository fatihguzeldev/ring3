import {generatedImports, attachDetectionApi} from '../../../../../../../support/wasm/binding.mjs';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../../../../../../../support/wasm/engine.mjs';

const [enginePath, output, root] = process.argv.slice(2), engine = readEngine(enginePath);
const PC = 0x1000, SEG = 4524, SIZE = 4612, keyHigh = 0x4152504c;
const counts = {contexts: 0, modules: 0, runs: 0, registers: 0, memory: 0, faults: 0, repairs: 0, controls: 0, selfWrites: 0};
const paths = [
  'engine/src/cpu/x86/decode/segment_queries.rs', 'engine/src/cpu/x86/decode/decoder.rs', 'engine/src/cpu/x86/decode/instruction.rs', 'engine/src/cpu/x86/decode/profile.rs', 'engine/src/cpu/x86/decode/families.rs', 'engine/src/cpu/x86/decode/lower.rs', 'engine/src/cpu/x86/decode/families.rs', 'engine/src/cpu/x86/ir.rs',
  'engine/src/cpu/x86/segments.rs', 'engine/src/cpu/x86/segments/cache.rs', 'engine/src/cpu/x86/segments/descriptors.rs', 'engine/src/abi/x86/segments.rs',
  'engine/src/cpu/dbt/region.rs', 'engine/src/cpu/dbt/wasm/instructions/segment_queries.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/mod.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/arithmetic.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/moves.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/exchange.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/predicates.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/shifts.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/control.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/extended.rs', 'engine/src/cpu/dbt/wasm/instructions/atomic.rs', 'engine/src/cpu/dbt/wasm/helpers/integer.rs', 'engine/src/cpu/dbt/wasm/helpers/result.rs',
  'engine/src/cpu/dbt/wasm/imports/mod.rs', 'engine/src/cpu/dbt/wasm/imports/binding.rs', 'engine/src/cpu/dbt/wasm/imports/context.rs', 'engine/src/cpu/dbt/wasm/imports/needs.rs', 'engine/src/cpu/dbt/wasm/helpers/protocol.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/mod.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/address.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/scalar.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/stack.rs', 'engine/src/cpu/dbt/wasm/instructions/segments.rs', 'engine/src/process/cpu/integer.rs', 'engine/src/process/cpu/segment_queries.rs', 'engine/src/memory/space.rs', 'engine/src/memory/space/atomic.rs', 'engine/src/abi/cpu_helper.rs',
  'engine/tests/cpu/segments/selectors/privilege/native.rs', 'engine/tests/cpu/segments/selectors/privilege/wasm.rs', 'engine/tests/cpu/segments/selectors/privilege/fixtures/p2-arpl/runtime/run.mjs', 'engine/tests/support/wasm/binding.mjs',
];
const pins = () => Object.fromEntries(paths.map(path => [path, createHash('sha256').update(readFileSync(join(root, path))).digest('hex')])), sourcePins = pins();
const words = values => {const b = Buffer.alloc(values.length * 4); values.forEach((v, i) => b.writeUInt32LE(v >>> 0, i * 4)); return b;};
function record(magic, size, fields, version = 1) {
  const b = Buffer.alloc(size); b.write(magic); b.writeUInt16LE(version, 4); b.writeUInt16LE(1, 6); b.writeUInt32LE(size, 8);
  fields.forEach((value, i) => b.writeUInt32LE(value >>> 0, 16 + i * 4)); return b;
}
const cpu = s => record('R3ST', 56, [...s.r, s.pc, s.flags], s.flags & ~0xcd7 ? 2 : 1);
const arena = c => Buffer.from(new Uint8Array(c.memory.buffer, c.base, SIZE));
function input(c, offset, b) {new Uint8Array(c.memory.buffer).set(b, c.base + offset);}
function host(c, name, args = []) {assert.equal(c.api[name](...args), 0, name);}
function upload(c, address, b) {input(c, 140, b); host(c, 'upload', [address, b.length]);}
function put(c, address, value) {const b = Buffer.alloc(2); b.writeUInt16LE(value); upload(c, address, b);}
function read(c, address) {host(c, 'read16', [address]); return arena(c).readUInt32LE(120);}
function seed(c, updates = {}, flags = 0x244cd7) {
  const s = {r: Array.from({length: 8}, (_, i) => (0xa5101234 + i * 0x10001) >>> 0), pc: PC, flags};
  for (const [i, v] of Object.entries(updates)) s.r[+i] = v >>> 0;
  input(c, 0, cpu(s)); input(c, 96, words([0])); return s;
}
function next(s, length, destination, value, adjusted) {
  const n = {...s, r: [...s.r], pc: PC + length, flags: ((s.flags & ~0x40) | (adjusted ? 0x40 : 0)) >>> 0};
  if (destination !== null) n.r[destination] = value >>> 0; return n;
}
function open(owner, entries) {
  const instance = new WebAssembly.Instance(engine.module, {}), c = {owner, entries, memory: instance.exports.memory, api: {}, calls: []};
  for (const [name, value] of Object.entries(instance.exports)) if (name.startsWith('ring3_abi_v1_') && typeof value === 'function') c.api[name.slice(13)] = value;
 attachDetectionApi(c.api, instance.exports);
  c.key = ++counts.contexts; host(c, 'open', [8, c.key, keyHigh]); c.base = c.api.arena_ptr() >>> 0;
  for (const args of [[PC, 1, 7], [0x5000, 1, 3], [0xf000, 1, 3]]) host(c, 'map', args);
  c.initialSegments = arena(c).subarray(SEG, SEG + 64); return c;
}
function compile(c, instruction, memory = false) {
  if (c.previousResident) {input(c, 96, words([0])); host(c, c.staleResident ? 'retire_stale_resident' : 'discard_unacknowledged_resident', [c.key, keyHigh, ...c.previousResident]); c.staleResident = false;}
  upload(c, PC, Buffer.from([...instruction, 0xeb, 0])); input(c, 140, words(c.entries ? [PC] : [PC, instruction.length + 2]));
  const method = c.owner === 'resident' ? (c.entries ? 'compile_resident_entries' : 'compile_resident') : (c.entries ? 'compile_entries' : 'compile');
  host(c, method, [1, ...(c.entries ? [0] : [])]);
  const b = arena(c), pointer = c.owner === 'resident' ? b.readUInt32LE(156) : c.api.module_ptr() >>> 0;
  if (c.owner === 'resident') c.previousResident = [b.readUInt32LE(148), b.readUInt32LE(152)];
  c.binding = c.owner === 'resident' ? [c.key, keyHigh, ...c.previousResident] : [];
  const length = c.owner === 'resident' ? b.readUInt32LE(160) : c.api.module_len() >>> 0;
  const bytes = Buffer.from(new Uint8Array(c.memory.buffer, pointer, length)), module = new WebAssembly.Module(bytes);
  const name = c.owner === 'resident' ? 'cpu_integer_resident' : 'cpu_integer';
  assert.deepEqual(WebAssembly.Module.imports(module).map(i => `${i.module}:${i.name}:${i.kind}`).sort(), generatedImports({resident: c.owner === 'resident', helpers: [...(memory ? [name] : [])]}).map(item => `${item.module}:${item.name}:${item.kind}`).sort());
  writeFileSync(join(output, `module-${++counts.modules}.wasm`), bytes, {flag: 'wx'});
  const ring3 = {...c.api};
  ring3[name] = (...args) => {
    const before = arena(c), status = c.api[name](...args), after = arena(c), allowed = Buffer.from(before);
    allowed.set(after.subarray(100, 164), 100); assert.deepEqual(after, allowed, 'helper publication is restricted to packet and payload');
    c.calls.push({status, args: args.map(v => v >>> 0), payload: Array.from({length: 6}, (_, i) => before.readUInt32LE(140 + i * 4))}); return status;
  };
  const fn = new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3}).exports.run;
  fn.memory = memory; fn.instructionLength = instruction.length; return fn;
}
function run(c, fn, state, {budget = 1, reason = 1, retired = 1, packet, payload, fault = [0, 0, 0, 0], calls = packet ? 1 : 0, helperStatus = 0} = {}) {
  const expected = arena(c), at = c.calls.length; expected.set(cpu(state));
  expected.set(record('R3EX', 40, [reason, retired, ...fault], reason === 16 ? 7 : fn.memory || reason === 11 || reason === 15 ? 6 : 1), 56);
  if (packet) expected.set(record('R3MH', 40, packet, 7), 100);
  if (payload) expected.set(words(payload), 140);
  assert.equal(fn(c.base, c.base + 56, budget, c.base + 96), 0);
  assert.deepEqual(arena(c), expected, `whole arena reason=${reason}, pc=${state.pc.toString(16)}`);
  const observed = c.calls.slice(at); assert.equal(observed.length, calls);
  for (const call of observed) {assert.equal(call.status, helperStatus); assert.equal(call.args.at(-2), 0x30000);}
  counts.runs++;
}
const payload = (s, source, offset, segment, flags = s.flags) => [s.r[source] & 0xffff, flags, offset, segment, s.r[3], s.r[1]];
const success = [0, 0, 0, 0, 0, 2];
function memoryCase(c, fn, s, source, offset, segment, old, label = '') {
  const adjust = old % 4 < s.r[source] % 4, value = adjust ? old - old % 4 + s.r[source] % 4 : old;
  const n = next(s, fn.instructionLength, null, 0, adjust);
  run(c, fn, n, {packet: success, payload: payload(s, source, offset, segment, n.flags)});
  assert.deepEqual(c.calls.at(-1).payload, payload(s, source, offset, segment), label + ' captured inputs');
  assert.equal(read(c, c.calls.at(-1).args.at(-1)), value, label + ' memory value'); counts.memory++;
}
function cached(c, segment, bits) {
  const b = Buffer.from(c.initialSegments); b.writeUInt16LE(2, 4);
  [0xf33fffff, 0xf3bfffff, 0xf33fffff, 0xf33fffff, 0xf33fffff, 0].forEach((v, i) => b.writeUInt32LE(v >>> 0, [28, 32, 36, 40, 44, 56][i]));
  b.writeUInt32LE(bits >>> 0, [28, 32, 36, 40, 44, 56][segment]); input(c, SEG, b);
}
for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries);
  for (const prefix of [[], [0x66]]) for (let destination = 0; destination < 8; destination++) for (let source = 0; source < 8; source++) {
    const fn = compile(c, [...prefix, 0x63, 0xc0 | source << 3 | destination]);
    for (let drpl = 0; drpl < 4; drpl++) for (let srpl = 0; srpl < 4; srpl++) {
      const s = seed(c, {[destination]: 0xa5b6c7d0 + drpl, [source]: 0x9abc1230 + srpl}, (drpl + srpl) % 2 ? 2 : 0x244cd7);
      const adjust = s.r[destination] % 4 < s.r[source] % 4;
      const value = adjust ? s.r[destination] - s.r[destination] % 4 + s.r[source] % 4 : s.r[destination];
      run(c, fn, next(s, fn.instructionLength, destination, value, adjust)); counts.registers++;
    }
  }
  const forms = [
    {bytes: [0x63, 0x0f], updates: {7: 0x5010}, offset: 0x5010, segment: 3, address: 0x5010},
    {bytes: [0x66, 0x63, 0x0f], updates: {7: 0x5010}, offset: 0x5010, segment: 3, address: 0x5010},
    {bytes: [0x63, 0x4d, 0], updates: {5: 0x5010}, offset: 0x5010, segment: 2, address: 0x5010},
    {bytes: [0x64, 0x63, 0x0f], updates: {7: 0x10}, offset: 0x10, segment: 4, address: 0x5010, fs: true},
    {bytes: [0x67, 0x63, 0x08], updates: {3: 0xabcd5000, 6: 0x12340010}, offset: 0x5010, segment: 3, address: 0x5010},
    {bytes: [0x63, 0x3f], updates: {7: 0x5013}, offset: 0x5013, segment: 3, address: 0x5013, source: 7},
  ];
  for (const row of forms) {
    input(c, SEG, c.initialSegments); if (row.fs) host(c, 'set_segment_base', [4, 0x5000]);
    const fn = compile(c, row.bytes, true), source = row.source ?? 1;
    for (const old of [0xabc0, 0xabc3]) {
      put(c, row.address, old); const s = seed(c, {1: 3, ...row.updates}); memoryCase(c, fn, s, source, row.offset, row.segment, old);
    }
    const stopped = seed(c, {1: 3, ...row.updates}); run(c, fn, stopped, {budget: 0, retired: 0});
    input(c, 96, words([1])); run(c, fn, stopped, {reason: 2, retired: 0}); counts.controls += 2;
  }
  input(c, SEG, c.initialSegments);
  let fn = compile(c, [0x63, 0x0f], true); put(c, 0x5010, 0xabc3); host(c, 'protect', [0x5000, 1, 1]);
  let s = seed(c, {1: 0, 7: 0x5010});
  run(c, fn, s, {reason: 5, retired: 0, packet: [1, 0, 2, 0x5010, 2, 2], payload: payload(s, 1, 0x5010, 3), fault: [2, 0x5010, 2, 2]}); counts.faults++;
  host(c, 'protect', [0x5000, 1, 3]); memoryCase(c, fn, s, 1, 0x5010, 3, 0xabc3); counts.repairs++;
  upload(c, 0x5fff, Buffer.from([0]));
  s = seed(c, {1: 3, 7: 0x5fff});
  run(c, fn, s, {reason: 5, retired: 0, packet: [1, 0, 1, 0x6000, 2, 2], payload: payload(s, 1, 0x5fff, 3), fault: [1, 0x6000, 2, 2]}); counts.faults++;
  host(c, 'read8', [0x5fff]); assert.equal(arena(c).readUInt32LE(120), 0);
  host(c, 'map', [0x6000, 1, 3]); upload(c, 0x6000, Buffer.from([0x55]));
  memoryCase(c, fn, s, 1, 0x5fff, 3, 0x5500); counts.repairs++;

  host(c, 'configure_segment_descriptor', [0x43, 0, 3, 1, 0]);
  host(c, c.owner === 'resident' ? 'segment_load_resident' : 'segment_load', [...c.binding, 3, 0x43]);
  host(c, 'configure_segment_descriptor', [0x43, 0, 3, 1, 1]);
  s = seed(c, {1: 0, 7: 0x5010});
  run(c, fn, s, {reason: 11, retired: 0, packet: [3, 0, 0, 0, 0, 2], payload: payload(s, 1, 0x5010, 3)}); counts.faults++;
  host(c, c.owner === 'resident' ? 'segment_load_resident' : 'segment_load', [...c.binding, 3, 0x43]);
  put(c, 0x5010, 0xabc0); s = seed(c, {1: 3, 7: 0x5010}); memoryCase(c, fn, s, 1, 0x5010, 3, 0xabc0); counts.repairs++;

  for (const row of [{segment: 3, bytes: [0x63, 0x0f], updates: {7: 0x7000}, reason: 11}, {segment: 2, bytes: [0x63, 0x4d, 0], updates: {5: 0x7000}, reason: 15}]) {
    cached(c, row.segment, 0xf2307000); fn = compile(c, row.bytes, true); s = seed(c, {1: 3, ...row.updates});
    run(c, fn, s, {reason: row.reason, retired: 0, packet: [row.segment === 2 ? 7 : 3, 0, 0, 0, 0, 2], payload: payload(s, 1, 0x7000, row.segment)}); counts.faults++;
  }
  cached(c, 3, 0xf2305011); fn = compile(c, [0x63, 0x0f], true); put(c, 0x5010, 0xabc0); s = seed(c, {1: 3, 7: 0x5010}); memoryCase(c, fn, s, 1, 0x5010, 3, 0xabc0);
  cached(c, 3, 0xf2305010); input(c, 0, cpu(s));
  run(c, fn, s, {reason: 11, retired: 0, packet: [3, 0, 0, 0, 0, 2], payload: payload(s, 1, 0x5010, 3)}); counts.faults++;
  cached(c, 3, 0xf0700fff); put(c, 0xfffe, 0xabc0); s = seed(c, {1: 3, 7: 0xfffe}); memoryCase(c, fn, s, 1, 0xfffe, 3, 0xabc0);
  s = seed(c, {1: 3, 7: 0xffff});
  run(c, fn, s, {reason: 11, retired: 0, packet: [3, 0, 0, 0, 0, 2], payload: payload(s, 1, 0xffff, 3)}); counts.faults++;
  input(c, SEG, c.initialSegments); s = seed(c, {1: 3, 7: 0xffffffff});
  run(c, fn, s, {reason: 11, retired: 0, packet: [3, 0, 0, 0, 0, 2], payload: payload(s, 1, 0xffffffff, 3)}); counts.faults++;

  fn = compile(c, [0x2e, 0x63, 0x0f], true); s = seed(c, {1: 0, 7: 0x5010});
  run(c, fn, s, {reason: 11, retired: 0, packet: [3, 0, 0, 0, 0, 2], payload: payload(s, 1, 0x5010, 1)}); counts.faults++;
  fn = compile(c, [0x65, 0x67, 0x63, 0x0e, 0x10, 0x50], true);
  host(c, c.owner === 'resident' ? 'segment_load_resident' : 'segment_load', [...c.binding, 5, 0x23]);
  put(c, 0x5010, 0xabc0); s = seed(c, {1: 3}); memoryCase(c, fn, s, 1, 0x5010, 5, 0xabc0);
  input(c, SEG, c.initialSegments); fn = compile(c, [0x63, 0x0f], true);
  host(c, c.owner === 'resident' ? 'segment_load_resident' : 'segment_load', [...c.binding, 3, 0]); s = seed(c, {1: 3, 7: 0x5010});
  run(c, fn, s, {reason: 11, retired: 0}); counts.faults++;
  input(c, SEG, c.initialSegments);

  // A conditional no-store keeps executable identity; an actual changed word retires before SMC/debug exit.
  for (const mode of ['unchanged', 'changed', 'debug']) {
    const unchanged = mode === 'unchanged'; fn = compile(c, [0x63, 0x0f, unchanged ? 0x93 : 0x90, 0x90], true);
    s = seed(c, {1: unchanged ? 0 : 3, 7: PC + 2}, mode === 'debug' ? 0x244dd7 : 0x244cd7);
    const n = next(s, 2, null, 0, !unchanged);
    run(c, fn, n, {reason: unchanged ? 1 : mode === 'debug' ? 16 : 6, packet: success, payload: payload(s, 1, PC + 2, 3, n.flags), helperStatus: unchanged ? 0 : 11});
    counts.selfWrites++;
    assert.equal(read(c, PC + 2), 0x9093);
    if (unchanged) {input(c, 0, cpu(s)); run(c, fn, n, {packet: success, payload: payload(s, 1, PC + 2, 3, n.flags)}); counts.selfWrites++;}
    else {const before = arena(c); assert.equal(fn(c.base, c.base + 56, 1, c.base + 96), 4); assert.deepEqual(arena(c), before); c.staleResident = true; counts.controls++;}
  }
  host(c, 'close');
}
assert.deepEqual(counts, {contexts: 4, modules: 576, runs: 8368, registers: 8192, memory: 72, faults: 40, repairs: 12, controls: 56, selfWrites: 16});
assert.deepEqual(pins(), sourcePins, 'ARPL risk-relevant sources stayed fixed');
writeFileSync(join(output, 'result.json'), JSON.stringify({status: 'ok', engine_sha256: engine.sha256, source_pins: sourcePins, counts, limits: ['finite IA32 ARPL register/memory proof; cached permissions/ranges are enforced for ARPL, generic memory integration and descriptor-query instructions remain separate; no hardware bus/cache or throughput claim']}, null, 2) + '\n');
console.log(JSON.stringify({status: 'ok', ...counts, output}));
