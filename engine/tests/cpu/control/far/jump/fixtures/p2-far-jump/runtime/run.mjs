import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {mkdirSync, readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {createArena, ARENA_SIZE} from '../../../../../../../support/wasm/arena.mjs';
import {readEngine} from '../../../../../../../support/wasm/engine.mjs';
import {assertEngineExports} from '../../../../../../../support/wasm/exports.mjs';
import * as transfer from '../../../../support/transport.mjs';

assert.ok(process.env.RING3_ENGINE_SHA256, 'select an explicit engine SHA-256');
const [enginePath, output, root, mode] = process.argv.slice(2), engine = readEngine(enginePath);
const PC = 0x1000, TARGET = 0x2000, POINTER = 0x5010, SEG = 4524, KEY_HIGH = 0x464152;
const counts = {contexts: 0, modules: 0, runs: 0, success: 0, faults: 0, refusals: 0, controls: 0};
const observations = [];
const sources = [
  "engine/src/abi/wasm/exports/calls.rs",
  "engine/src/abi/wasm/exports/execution.rs",
  "engine/src/abi/wasm/exports/image.rs",
  "engine/src/abi/wasm/exports/instance.rs",
  "engine/src/abi/wasm/exports/memory.rs",
  "engine/src/abi/wasm/exports/xstate.rs",
  "engine/tests/support/wasm/arena.mjs",
  "engine/tests/support/wasm/engine.mjs",
  "engine/tests/support/wasm/exports.mjs",
  "engine/tests/cpu/control/far/support/transport.mjs",
  "engine/src/cpu/x86/segments/transfers.rs",
  "engine/src/cpu/x86/segments/transfers/gates.rs",
  "engine/src/abi/far.rs",
  "engine/src/cpu/dbt/wasm/far/gate.rs",
  "engine/src/cpu/x86/decode/fetch.rs",
  "engine/src/cpu/x86/decode/decoder.rs",
  "engine/src/cpu/dbt/wasm/code_fetch.rs",
  "engine/src/cpu/x86/segments/descriptors.rs",
  "engine/src/cpu/x86/segments/descriptor_record.rs",
  "engine/src/cpu/x86/segments/cache.rs",
  "engine/src/cpu/x86/decode/far.rs",
  "engine/src/cpu/x86/decode/profile.rs",
  "engine/src/cpu/x86/decode/lower.rs",
  "engine/src/cpu/x86/ir.rs",
  "engine/src/cpu/dbt/region.rs",
  "engine/src/cpu/dbt/wasm/far.rs",
  "engine/src/cpu/dbt/wasm/emitter.rs",
  "engine/src/cpu/dbt/wasm/memory.rs",
  "engine/src/cpu/dbt/wasm/integer/dispatch.rs",
  "engine/src/cpu/dbt/wasm/cpu_result.rs",
  "engine/src/process/memory/scalar.rs",
  "engine/src/abi/memory_helper.rs",
  "engine/src/cpu/dbt/wasm/segments.rs",
  "engine/src/cpu/dbt/wasm/segments/access.rs",
  "engine/src/cpu/dbt/wasm/abi.rs",
  "engine/src/cpu/dbt/wasm/locals.rs",
  "engine/src/process/cpu/far.rs",
  "engine/src/process/execution/authority.rs",
  "engine/src/process/wasm/cpu.rs",
  "engine/src/process/wasm/mod.rs",
  "engine/src/abi/wasm/exports/cpu.rs",
  "engine/src/abi/wasm/exports/mod.rs",
  "engine/src/abi/x86/segments.rs",
  "engine/tests/cpu/control/native.rs",
  "engine/tests/cpu/control/wasm.rs",
  "engine/tests/cpu/control/far/jump/native.rs",
  "engine/tests/cpu/control/far/jump/wasm.rs",
  "engine/tests/cpu/control/far/jump/fixtures/p2-far-jump/runtime/run.mjs"
];
const pins = () => Object.fromEntries(sources.map(path => [path, createHash('sha256').update(readFileSync(join(root, path))).digest('hex')]));
const sourcePins = pins();
const words = values => {const bytes = Buffer.alloc(values.length * 4); values.forEach((v, i) => bytes.writeUInt32LE(v >>> 0, i * 4)); return bytes;};
function packet(magic, size, fields, version) {const bytes = Buffer.alloc(size); bytes.write(magic); bytes.writeUInt16LE(version, 4); bytes.writeUInt16LE(1, 6); bytes.writeUInt32LE(size, 8); bytes.set(words(fields), 16); return bytes;}
const arena = c => Buffer.from(new Uint8Array(c.memory.buffer, c.base, ARENA_SIZE));
function put(c, offset, bytes) {new Uint8Array(c.memory.buffer).set(bytes, c.base + offset);}
function host(c, name, args = []) {assert.equal(c.api[name](...args), 0, name);}
function upload(c, address, bytes) {put(c, 140, bytes); host(c, 'upload', [address, bytes.length]);}
function configure(c, selector, raw) {const before = arena(c); host(c, 'configure_segment_descriptor_raw', [selector, raw.low, raw.high]); assert.deepEqual(arena(c), before, 'table configuration leaves loaded state and arena unchanged');}
function descriptor({type = 10, dpl = 3, present = true, codeData = true, base = 0, limit = 0xfffff, pages = true, d32 = true, long = false, avl = false} = {}) {
  return {type, dpl, present, codeData, base, limit, pages, d32, long, avl,
    low: (((base & 0xffff) * 65536) + (limit & 0xffff)) >>> 0,
    high: ((base & 0xff000000) + ((base >>> 16) & 255) + type * 256 + Number(codeData) * 4096 + dpl * 8192 + Number(present) * 32768 + (limit & 0xf0000) + Number(avl) * 0x100000 + Number(long) * 0x200000 + Number(d32) * 0x400000 + Number(pages) * 0x800000) >>> 0};
}
const cache = raw => (raw.limit + (raw.type | 1) * 0x100000 + Number(raw.pages) * 0x1000000 + Number(raw.d32) * 0x2000000 + Number(raw.avl) * 0x8000000 + 0x10000000 + raw.dpl * 0x20000000 + 0x80000000) >>> 0;
const cacheOffsets = [28, 32, 36, 40, 44, 56];
function decodeSegments(bytes) {
  const selectors = Array.from({length: 6}, (_, i) => bytes.readUInt16LE(16 + i * 2));
  const version = bytes.readUInt16LE(4);
  const caches = version === 1 ? selectors.map((selector, i) => (selector & 0xfffc) === 0 ? 0 : i === 1 ? 0xf3bfffff : 0xf33fffff) : cacheOffsets.map(at => bytes.readUInt32LE(at));
  const bases = version === 3 ? Array.from({length: 6}, (_, i) => bytes.readUInt32LE(4756 - SEG + 16 + i * 4)) : [0, 0, 0, 0, bytes.readUInt32LE(48), bytes.readUInt32LE(52)];
  return {selectors, caches, bases, fs: bases[4], gs: bases[5]};
}
function contextBases(context) {
  const bases = [...(context.bases ?? [0, 0, 0, 0, 0, 0])];
  bases[4] = context.fs ?? bases[4]; bases[5] = context.gs ?? bases[5];
  return bases;
}
function encodeSegments(context) {
  const bases = contextBases(context), bytes = packet('R3SG', 64, [], 1);
  context.selectors.forEach((value, i) => bytes.writeUInt16LE(value, 16 + i * 2));
  const legacy = context.selectors.map((selector, i) => (selector & 0xfffc) === 0 ? 0 : i === 1 ? 0xf3bfffff : 0xf33fffff);
  const nonflat = bases.slice(0, 4).some(value => value !== 0) || (context.caches[1] & 0x02000000) === 0;
  if (nonflat || !context.caches.every((value, i) => value === legacy[i])) {
    bytes.writeUInt16LE(nonflat ? 3 : 2, 4);
    context.caches.forEach((value, i) => bytes.writeUInt32LE(value, cacheOffsets[i]));
  }
  if (!nonflat) {bytes.writeUInt32LE(bases[4], 48); bytes.writeUInt32LE(bases[5], 52);}
  return bytes;
}
function publishSegments(bytes, context) {
  const segments = encodeSegments(context), companion = packet('R3SB', 40, [], 1);
  if (segments.readUInt16LE(4) === 3) contextBases(context).forEach((value, i) => companion.writeUInt32LE(value >>> 0, 16 + i * 4));
  bytes.set(segments, SEG); bytes.set(companion, 4756);
}
function oracle(selector, raw, offset, word) {
  const code = selector & 0xfffc, target = word ? offset & 0xffff : offset >>> 0;
  if (code === 0 || !raw) return {reason: 11, status: 3, detail: code};
  if (!raw.codeData) {
    const eligible = [4, 5, 12].includes(raw.type) || ((selector & 4) === 0 && [1, 9].includes(raw.type));
    if (!eligible || raw.dpl < 3) return {reason: 11, status: 3, detail: code};
    if (!raw.present) return {reason: 14, status: 6, detail: code};
    if (raw.type === 4 || raw.type === 12) {
      if ((raw.high & 0xe0) !== 0 || (raw.type === 4 && (raw.high & 0xffff0000) !== 0))
        return {reason: 7, host: 7, detail: 3};
      const embedded = (raw.low >>> 16) & 0xfffc;
      assert.equal(embedded, 0, 'these frozen ordinary-descriptor rows encode a null gate target');
      return {reason: 11, status: 3, detail: embedded, gate: true};
    }
    return {reason: 7, host: 7, detail: 3};
  }
  if (raw.type < 8 || ((raw.type & 4) === 0 && raw.dpl !== 3)) return {reason: 11, status: 3, detail: code};
  if (!raw.present) return {reason: 14, status: 6, detail: code};
  const limit = raw.pages ? raw.limit * 4096 + 4095 : raw.limit;
  if (target > limit) return {reason: 11, status: 3, detail: 0};
  if (raw.long) return {reason: 7, host: 7, detail: 3};
  return {reason: 1, status: 0, target, selector: code | 3, cache: cache(raw), base: raw.base};
}
function pointer(word, offset, selector) {const bytes = Buffer.alloc(word ? 4 : 6); if (word) bytes.writeUInt16LE(offset & 0xffff); else bytes.writeUInt32LE(offset >>> 0); bytes.writeUInt16LE(selector, bytes.length - 2); return bytes;}
function open(resident, entries, word, source) {
  const instance = new WebAssembly.Instance(engine.module, {}), api = Object.fromEntries(Object.entries(instance.exports).filter(([name]) => name.startsWith('ring3_abi_v1_')).map(([name, fn]) => [name.slice(13), fn]));
  const c = {resident, entries, word, source, api, memory: instance.exports.memory, key: ++counts.contexts, calls: []};
  host(c, 'open', [8, c.key, KEY_HIGH]); c.base = api.arena_ptr() >>> 0;
  for (const [address, permissions] of [[PC, 7], [TARGET, 7], [0x5000, 3]]) host(c, 'map', [address, 1, permissions]);
  upload(c, TARGET, Buffer.from([0xeb, 0xfe]));
  return c;
}
// inspect exact generated imports, run signature/locals and the optional pure segment callee.
function validateModule(bytes, expectedImports, hasGuard) {
  const module = new WebAssembly.Module(bytes);
  assert.deepEqual(WebAssembly.Module.imports(module).map(row => `${row.module}:${row.name}:${row.kind}`).sort(), expectedImports.sort());
  assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  let at = 8; const types = [], imported = [], functions = [], bodies = [];
  const leb = () => {let n = 0, shift = 0, v; do {v = bytes[at++]; assert.ok(v !== undefined && shift < 35); n += (v & 127) * 2 ** shift; shift += 7;} while (v & 128); return n;};
  const vector = () => {const n = leb(), b = bytes.subarray(at, at + n); at += n; return [...b];};
  const string = () => Buffer.from(vector()).toString();
  while (at < bytes.length) {
    const section = bytes[at++], length = leb(), end = at + length;
    if (section === 1) for (let n = leb(); n; n--) {assert.equal(bytes[at++], 0x60); types.push([vector(), vector()]);}
    else if (section === 2) for (let n = leb(); n; n--) {
      string(); const name = string(), kind = bytes[at++]; if (kind === 2) {assert.equal(leb(), 0); assert.equal(leb(), 1);} else {assert.equal(kind, 0); imported.push([name, leb()]);}
    }
    else if (section === 3) for (let n = leb(); n; n--) functions.push(leb());
    else if (section === 10) for (let n = leb(); n; n--) {const size = leb(); bodies.push(bytes.subarray(at, at + size)); at += size;}
    if ([1, 2, 3, 10].includes(section)) assert.equal(at, end);
    at = end;
  }
  assert.deepEqual(types[0], [[127, 127, 127, 127], [127]]);
  for (const [name, type] of imported) {
    const arity = name === 'guard' ? 6 : name === 'guard_resident' || name === 'far_jump_resident' ? 7 : 3;
    assert.deepEqual(types[type], [Array(arity).fill(127), [127]], name);
  }
  assert.equal(functions.length, hasGuard ? 2 : 1); assert.equal(bodies.length, functions.length); assert.equal(functions[0], 0);
  const locals = body => {let p = 0; const read = () => {let n = 0, shift = 0, v; do {v = body[p++]; n += (v & 127) * 2 ** shift; shift += 7;} while (v & 128); return n;}; return Array.from({length: read()}, () => [read(), body[p++]]);};
  // run-local allocation is internal to the emitter; Wasm validation owns it.
  if (hasGuard) {assert.deepEqual(types[functions[1]], [Array(6).fill(127), [127]]); assert.deepEqual(locals(bodies[1]), [[3, 127]]); assert.equal(bodies[1].at(-1), 11);}
  return module;
}
function compile(c, {twoBlocks = false, bytes} = {}) {
  if (c.id) {put(c, 96, words([0])); host(c, 'discard_unacknowledged_resident', [c.key, KEY_HIGH, ...c.id]);}
  bytes ??= [...(c.word ? [0x66] : []), ...(c.source === 'immediate' ? c.word ? [0xea, 0, 0x20, 0x43, 0] : [0xea, 0, 0x20, 0, 0, 0x43, 0] : [0xff, 0x2f])];
  c.bytes = Buffer.from(bytes); upload(c, PC, c.bytes);
  const specs = c.entries ? [PC, ...(twoBlocks ? [TARGET] : [])] : [PC, bytes.length, ...(twoBlocks ? [TARGET, 2] : [])];
  put(c, 140, words(specs)); const name = c.resident ? c.entries ? 'compile_resident_entries' : 'compile_resident' : c.entries ? 'compile_entries' : 'compile';
  host(c, name, [twoBlocks ? 2 : 1, ...(c.entries ? [0] : [])]);
  // On the retained pre-feature engine the first real far-JMP compile above is the semantic RED.
  const exports = assertEngineExports(engine.module, root);
  assert.equal(exports.functions.find(row => row.name === 'ring3_abi_v1_far_jump').arity, 3);
  assert.equal(exports.functions.find(row => row.name === 'ring3_abi_v1_far_jump_resident').arity, 7);
  const a = arena(c); c.id = c.resident ? [a.readUInt32LE(148), a.readUInt32LE(152)] : null;
  c.binding = c.id ? [c.key, KEY_HIGH, ...c.id] : [];
  const address = c.id ? a.readUInt32LE(156) : c.api.module_ptr() >>> 0, size = c.id ? a.readUInt32LE(160) : c.api.module_len() >>> 0;
  const generated = Buffer.from(new Uint8Array(c.memory.buffer, address, size));
  const helper = c.resident ? 'far_jump_resident' : 'far_jump';
  const memory = c.source !== 'immediate';
  const module = validateModule(generated, ['env:memory:memory', `ring3:${c.resident ? 'guard_resident' : 'guard'}:function`, `ring3:${helper}:function`, ...(memory ? ['ring3:cpu_read:function'] : [])], memory);
  writeFileSync(join(output, `module-${++counts.modules}.wasm`), generated, {flag: 'wx'});
  const ring3 = Object.fromEntries(WebAssembly.Module.imports(module).filter(row => row.kind === 'function').map(row => [row.name, (...args) => {
    c.calls.push([row.name, args]);
    if (row.name.startsWith('far_jump') && c.beforeJump) {const hook = c.beforeJump; c.beforeJump = null; hook();}
    if (row.name.startsWith('far_jump') && c.reply) {put(c, 100, c.reply.packet); return c.reply.transport;}
    return c.api[row.name](...args);
  }]));
  c.run = new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3}).exports.run; c.twoBlocks = twoBlocks;
}
function execute(c, label, expected, {address = POINTER, sourceSegment, logical = address, flags = 0x244cd7, budget = 1, cancel = 0, pc = PC, preserveSegments = false, helper = true, pointerBytes, context, faultRead, injectedHelper} = {}) {
  const seed = createArena(); seed.writeUInt16LE(2, 4); seed.writeUInt32LE(flags, 52); seed.writeUInt32LE(pc, 48);
  const registers = [0x12345678, 0x23456789, 0x3456789a, 0x456789ab, 0x6040, logical, 0x56789abc, logical];
  registers.forEach((value, i) => seed.writeUInt32LE(value >>> 0, 16 + i * 4)); seed.writeUInt32LE(cancel, 96);
  if (preserveSegments) {seed.set(arena(c).subarray(SEG, SEG + 64), SEG); seed.set(arena(c).subarray(4756, 4796), 4756);}
  if (context) publishSegments(seed, context);
  if (sourceSegment) {
    const state = decodeSegments(seed.subarray(SEG)); state.selectors[sourceSegment.index] = sourceSegment.selector ?? state.selectors[sourceSegment.index]; state.caches[sourceSegment.index] = sourceSegment.cache;
    if (sourceSegment.index === 4) state.fs = sourceSegment.base ?? state.fs;
    if (sourceSegment.index === 5) state.gs = sourceSegment.base ?? state.gs;
    publishSegments(seed, state);
  }
  put(c, 0, seed); const want = Buffer.from(seed), success = expected.status === 0 && !faultRead && (helper || pc === TARGET) && budget > 0 && !cancel;
  let reason = expected.reason, retired = Number(success), detail = expected.detail ?? 0, at = 0, access = 0, length = 0;
  if (cancel || budget === 0) {reason = cancel ? 2 : 1; retired = 0; detail = 0; helper = false;}
  if (success) {
    want.writeUInt32LE(expected.target, 48);
    const state = decodeSegments(want.subarray(SEG)); state.selectors[1] = expected.selector; state.caches[1] = expected.cache; state.bases[1] = expected.base; publishSegments(want, state);
    if (flags & 0x100) reason = 16;
    else if (budget > 1) {reason = c.twoBlocks && !expected.base ? 1 : 3; retired = c.twoBlocks && !expected.base ? 2 : 1;}
  }
  if (faultRead) {reason = 5; retired = 0; detail = faultRead.detail; at = faultRead.address; access = 1; length = c.word ? 4 : 6;}
  want.set(packet('R3EX', 40, [reason, retired, detail, at, access, length], reason >= 16 ? 7 : 6), 56);
  if (helper) {
    if (c.source !== 'immediate' && pointerBytes && !faultRead) want.set(pointerBytes, 140);
    if (faultRead) want.set(packet('R3MH', 40, [1, 0, detail, at, 1, length], 7), 100);
    else if (!expected.host) want.set(packet('R3MH', 40, [expected.status, success ? expected.target : 0, expected.detail ?? 0, 0, 0, 0], expected.gate ? 9 : 7), 100);
    else if (c.source !== 'immediate' && pointerBytes) want.set(packet('R3MH', 40, [0, 0, 0, 0, 0, pointerBytes.length], 7), 100);
  }
  if (injectedHelper) want.set(injectedHelper, 100);
  c.calls = []; assert.equal(c.run(c.base, c.base + 56, budget, c.base + 96), 0, label);
  assert.deepEqual(arena(c), want, label);
  const expectedCalls = !helper ? 0 : faultRead ? 1 : c.source === 'immediate' ? 1 : 2;
  assert.equal(c.calls.filter(([name]) => !name.startsWith('guard')).length, expectedCalls, `${label}: exact helper schedule`);
  if (helper && c.source !== 'immediate') assert.deepEqual(c.calls.find(([name]) => name === 'cpu_read')[1], [address, c.word ? 4 : 6, 1], `${label}: exact pointer width/alignment`);
  counts.runs++; if (success) counts.success++; else if (expected.host && helper) counts.refusals++; else if (reason >= 5) counts.faults++; else counts.controls++;
  observations.push({label, reason, retired, detail, calls: c.calls.filter(([name]) => !name.startsWith('guard')).map(([name]) => name)});
}
if (mode !== 'protocol') for (const resident of [false, true]) for (const entries of [false, true]) for (const word of [false, true]) for (const source of ['immediate', 'memory']) {
  const c = open(resident, entries, word, source); compile(c);
  const rows = [];
  for (let type = 0; type < 16; type++) for (const present of [false, true]) for (const dpl of source === 'memory' ? [0, 3] : [3]) rows.push(descriptor({type, present, dpl}));
  for (let type = 0; type < 16; type++) rows.push(descriptor({type, codeData: false}));
  for (const type of [1, 3, 4, 5, 9, 11, 12]) rows.push(descriptor({type, codeData: false, dpl: 0}), descriptor({type, codeData: false, present: false}));
  rows.push(descriptor({limit: 0x1fff, pages: false}), descriptor({limit: 0x2000, pages: false}), descriptor({base: 0x1000}), descriptor({d32: false}), descriptor({long: true}), descriptor({type: 14, dpl: 1, avl: true}));
  for (const [index, raw] of rows.entries()) {
    configure(c, 0x43, raw); const bytes = pointer(word, TARGET, 0x43); if (source === 'memory') upload(c, POINTER, bytes);
    execute(c, `descriptor-${resident}-${entries}-${word}-${source}-${index}`, oracle(0x43, raw, TARGET, word), {pointerBytes: bytes});
  }
  const raw = descriptor({type: 12, dpl: 0}); configure(c, 0x43, raw); const expected = oracle(0x43, raw, TARGET, word), bytes = pointer(word, TARGET, 0x43);
  if (source === 'memory') upload(c, POINTER, bytes);
  execute(c, 'trap-after-committed-far-target', expected, {flags: 0x244dd7, pointerBytes: bytes});
  execute(c, 'zero-budget-no-read-or-publication', expected, {budget: 0, pointerBytes: bytes});
  execute(c, 'cancel-no-read-or-publication', expected, {cancel: 1, pointerBytes: bytes});
  execute(c, 'cold-target-lookup-after-one-retirement', expected, {budget: 2, pointerBytes: bytes});
  const boundedSource = decodeSegments(createArena().subarray(SEG)); boundedSource.caches[1] = 0xf2b0ffff;
  execute(c, 'bounded-source-CS-transfers-to-flat-target', expected, {context: boundedSource, pointerBytes: bytes});
  if (source === 'memory') {
    for (const selector of [0, 1, 2, 3, 0x4b]) {const b = pointer(word, TARGET, selector); upload(c, POINTER, b); execute(c, `selector-${selector}`, oracle(selector, null, TARGET, word), {pointerBytes: b});}
    for (const selector of [0x40, 0x41, 0x42, 0x43, 7]) {configure(c, selector & 0xfffc, raw); const b = pointer(word, TARGET, selector); upload(c, POINTER, b); execute(c, `normalized-CS-${selector}`, oracle(selector, raw, TARGET, word), {pointerBytes: b});}
    for (const type of [1, 3, 4, 5, 9, 11, 12]) {
      const system = descriptor({type, codeData: false}); configure(c, 0x47, system);
      const b = pointer(word, TARGET, 0x47); upload(c, POINTER, b);
      execute(c, `LDT-outer-system-${type}`, oracle(0x47, system, TARGET, word), {pointerBytes: b});
    }
    const b = pointer(word, TARGET, 0x43); upload(c, POINTER, b);
    host(c, 'protect', [0x5000, 1, 2]); execute(c, 'pointer-read-permission', {}, {faultRead: {detail: 2, address: POINTER}}); host(c, 'protect', [0x5000, 1, 3]);
    upload(c, 0x5fff, b.subarray(0, 1)); execute(c, 'pointer-read-cross-page', {}, {address: 0x5fff, logical: 0x5fff, faultRead: {detail: 1, address: 0x6000}});
    execute(c, 'logical-pointer-span-overflow-before-RAM', {reason: 11, detail: 0}, {logical: 0xfffffffd, helper: false});
    execute(c, 'null-data-segment-before-pointer-read', {reason: 11, detail: 0}, {sourceSegment: {index: 3, selector: 0, cache: 0}, helper: false});
    const bounded = (0xf2305011) >>> 0;
    execute(c, 'bounded-data-pointer-span-before-RAM', {reason: 11, detail: 0}, {sourceSegment: {index: 3, cache: bounded}, helper: false});
    for (const [prefix, opcode, logical, address, sourceSegment] of [
      [[0x64], [0xff, 0x2f], 0x1010, POINTER, {index: 4, cache: 0xf33fffff, base: 0x4000}],
      [[0x67], [0xff, 0x2e, 0x10, 0x50], 0xabcd5010, POINTER, undefined],
      [[], [0xff, 0x6d, 0], POINTER, POINTER, undefined],
    ]) {
      compile(c, {bytes: [...(word ? [0x66] : []), ...prefix, ...opcode]}); upload(c, POINTER, b);
      execute(c, 'selected-pointer-context', expected, {logical, address, sourceSegment, pointerBytes: b});
    }
    execute(c, 'SS-logical-pointer-span-overflow', {reason: 15, detail: 0}, {logical: 0xfffffffd, helper: false});
    compile(c);
  }
  // Reuse one captured region after CS selector/rights change; table mutation must not rewrite its cache.
  configure(c, 0x43, raw); if (source === 'memory') upload(c, POINTER, bytes); compile(c, {twoBlocks: true});
  execute(c, 'retained-region-enters-captured-target', expected, {budget: 2, pointerBytes: bytes});
  configure(c, 0x43, descriptor({type: 2, present: false}));
  const retained = {...expected, reason: 1};
  execute(c, 'retained-target-uses-loaded-CS-after-table-mutation', retained, {pc: TARGET, preserveSegments: true, helper: false});
  // The currentness guard is checked again at the helper seam, even if host code invalidates capture after entry guard.
  configure(c, 0x43, raw); if (source === 'memory') upload(c, POINTER, bytes);
  // write8 owns a version-3 byte-store packet even when the following far helper
  // refuses stale capture. Its exact side effect is distinct from CS publication.
  const injectedHelper = packet('R3MH', 40, [0, 0, 0, 0, 0, 1], 3);
  c.beforeJump = () => {
    const before = arena(c); host(c, 'write8', [PC, 0x90]);
    before.set(injectedHelper, 100);
    assert.deepEqual(arena(c), before, 'write8 injection owns only its exact helper packet in the arena');
  };
  execute(c, 'late-code-invalidation-prevents-CS-publication', {reason: 7, host: 4, detail: 3}, {pointerBytes: bytes, injectedHelper});
  const before = arena(c); assert.notEqual(c.api[c.id ? 'far_jump_resident' : 'far_jump'](...c.binding, TARGET, 0x43, Number(word)), 0); assert.deepEqual(arena(c), before, 'stale retained artifact rejects direct helper neutrally'); counts.controls++;
  host(c, 'close');
}

// A synthetic bridge replaces only the descriptor helper: no real CS/table
// publication occurs, so every malformed result must retain the full input arena.
function protocolProof() {
  for (const resident of [false, true]) for (const entries of [false, true]) for (const word of [false, true]) for (const source of ['immediate', 'memory']) {
    const c = open(resident, entries, word, source); compile(c);
    const raw = descriptor(); configure(c, 0x43, raw);
    const pointerBytes = pointer(word, TARGET, 0x43);
    if (source === 'memory') upload(c, POINTER, pointerBytes);
    const rows = [
      {label: 'foreign-MF', fields: [4, 0, 1, 0, 0, 0]},
      {label: 'foreign-XM', fields: [5, 0, 1, 0, 0, 0]},
      {label: 'foreign-SS', fields: [7, 0, 0, 0, 0, 0]},
      {label: 'foreign-rejected', fields: [2, 0, 1, 0, 0, 0]},
      {label: 'GP-wrong-selector', fields: [3, 0, 0x48, 0, 0, 0]},
      {label: 'NP-zero-selector', fields: [6, 0, 0, 0, 0, 0]},
      {label: 'NP-wrong-selector', fields: [6, 0, 0x48, 0, 0, 0]},
      {label: 'success-wrong-target', fields: [0, TARGET + 1, 0, 0, 0, 0]},
      {label: 'unknown-status', fields: [8, 0, 0, 0, 0, 0]},
      {label: 'foreign-MF-malformed-header', fields: [4, 0, 1, 0, 0, 0], corrupt: true},
      {label: 'success-malformed-header', fields: [0, TARGET, 0, 0, 0, 0], corrupt: true},
      {label: 'valid-GP-zero', fields: [3, 0, 0, 0, 0, 0], reason: 11, detail: 0},
      {label: 'valid-GP-selector', fields: [3, 0, 0x40, 0, 0, 0], reason: 11, detail: 0x40},
      {label: 'valid-NP-selector', fields: [6, 0, 0x40, 0, 0, 0], reason: 14, detail: 0x40},
      {label: 'nonzero-transport-preserved', fields: [4, 0, 1, 0, 0, 0], transport: 7, corrupt: true, detail: 3},
      {label: 'SMC-transport-refused', fields: [0, TARGET, 0, 0, 0, 0], transport: 11, detail: 3},
    ];
    for (const row of rows) {
      const seed = createArena(); seed.writeUInt16LE(2, 4); seed.writeUInt32LE(0x244cd7, 52); seed.writeUInt32LE(PC, 48);
      [0x12345678, 0x23456789, 0x3456789a, 0x456789ab, 0x6040, POINTER, 0x56789abc, POINTER].forEach((value, i) => seed.writeUInt32LE(value, 16 + i * 4));
      const response = packet('R3MH', 40, row.fields, 7);
      if (row.corrupt) response.writeUInt32LE(0, 0);
      c.reply = {packet: response, transport: row.transport ?? 0};
      put(c, 0, seed); c.calls = [];
      const reason = row.reason ?? 7, detail = row.detail ?? 2;
      const want = Buffer.from(seed); want.set(response, 100);
      if (source === 'memory') want.set(pointerBytes, 140);
      want.set(packet('R3EX', 40, [reason, 0, detail, 0, 0, 0], 6), 56);
      assert.equal(c.run(c.base, c.base + 56, 1, c.base + 96), 0, row.label);
      assert.deepEqual(arena(c), want, `${row.label}: exact packet and zero architectural publication`);
      const calls = c.calls.filter(([name]) => !name.startsWith('guard'));
      assert.deepEqual(calls.map(([name]) => name), source === 'memory' ? ['cpu_read', resident ? 'far_jump_resident' : 'far_jump'] : [resident ? 'far_jump_resident' : 'far_jump']);
      if (source === 'memory') assert.deepEqual(calls[0][1], [POINTER, word ? 4 : 6, 1]);
      counts.runs++; counts.controls++;
      observations.push({label: row.label, resident, entries, word, source, reason, retired: 0, detail, synthetic_descriptor_only: true});
    }
    c.reply = null;
    // A genuine pointer PF occurs before the descriptor seam and remains a PF.
    if (source === 'memory') {
      host(c, 'protect', [0x5000, 1, 2]);
      execute(c, 'protocol-control-pointer-PF-precedes-descriptor', {}, {faultRead: {detail: 2, address: POINTER}});
      host(c, 'protect', [0x5000, 1, 3]);
    }
    host(c, 'close');
  }
}
protocolProof();

// Reuse the established far transaction transport for this one RAM-backed
// acceptance row; the existing jump cases retain their original helper oracle.
if (mode !== 'protocol') {
  const boundedOutput = join(output, 'bounded-cs'); mkdirSync(boundedOutput, {recursive: true});
  for (const resident of [false, true]) for (const entries of [false, true]) {
    const c = transfer.open(engine, resident, entries), raw = transfer.descriptor({limit: TARGET, pages: false});
    transfer.configure(c, 0x43, raw);
    transfer.compile(c, Buffer.from([0xea, 0, 0x20, 0, 0, 0x43, 0]),
      {kind: 'far_jump', guard: false, targetBytes: Buffer.from([0x90, 0x90, 0xeb, 0xfe])}, root, boundedOutput);
    const context = transfer.decodeSegments(transfer.seed().subarray(SEG)); context.caches[1] = 0xf2b0ffff;
    const input = transfer.seed({context}), want = Buffer.from(input), ram = transfer.snapshotRAM(c);
    transfer.publishCode(want, raw, 0x43, TARGET + 1);
    want.set(transfer.packet('R3MH', 40, [0, TARGET, 0, 0, 0, 0], 7), 100); transfer.exit(want, 11, 2);
    transfer.run(c, 'bounded-JMP-refresh-NOP-at-limit-then-GP', input, want, ram,
      {budget: 3, helpers: [c.helper], argumentsList: [[...c.binding, TARGET, 0x43, 0]]});
    counts.contexts++; counts.modules++; counts.runs++; counts.success++;
    observations.push({label: 'bounded-JMP-refresh-NOP-at-limit-then-GP', resident, entries, reason: 11, retired: 2, helpers: [c.helper]});
    transfer.host(c, 'close');
  }
}

assert.deepEqual(pins(), sourcePins, 'all pinned source bytes remain frozen');
writeFileSync(join(output, 'result.json'), JSON.stringify({status: 'ok', engine_sha256: engine.sha256, source_pins: sourcePins, counts,
  limits: ['fixed protected32 CPL3, D16/D32 L0 CS destinations with logical target and current v3 base/mode publication; all four direct far JMP encodings only', 'known outer system descriptor faults are architectural; canonical outer-admitted gate32 rows reach embedded-null GP0 with exact v9 slot0 packets; gate16 rows with reserved nonzero high16 and outer-admissible tasks remain neutral profile refusals; full gate success/width semantics are owned by the separate call-gates fixture; L1 direct CS stays refused; nonflat and D16 CS publish current v3 records; D16 instruction execution is owned by the defaults fixture', 'CALL/RETF/INTn/IRET and generic CS fetch-limit enforcement remain separate', 'native process tests own raw A-bit readback; actual proof owns cache publication, retained capture, flags, exact pointer reads and helper currentness']}, null, 2) + '\n');
writeFileSync(join(output, 'observations.json'), JSON.stringify(observations, null, 2) + '\n');
console.log(JSON.stringify({status: 'ok', ...counts, output}));
