import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';

const [enginePath, output, root, arenaSize] = process.argv.slice(2);
const SIZE = Number(arenaSize), TRANSFER = 140, PC = 0x1000, COLD = 0x1800, DATA = 0x5000, ALIAS = 0x50000000, TOP = 0xfffff000, CHAIN = 0x1400;
const hash = b => createHash('sha256').update(b).digest('hex');
const engineBytes = readFileSync(enginePath), expectedHash = process.env.RING3_ENGINE_SHA256;
assert.match(expectedHash ?? '', /^[a-f0-9]{64}$/); assert.equal(hash(engineBytes), expectedHash);
const engineModule = new WebAssembly.Module(engineBytes); assert.deepEqual(WebAssembly.Module.imports(engineModule), []);
const initial = readFileSync(join(output, 'initial-arena.bin')); assert.equal(SIZE, 4364); assert.equal(initial.length, SIZE);
const REG = [0xa53c0011, 0x23456789, 0x3456789a, DATA + 24, 0x9000, 0x56789abc, 0x6789abcd, 0x789abcde], FLAGS = [2, 0xcd7];
const rows = [[0x11, 5], [0, 0xff], [0xff, 0], [0xff, 0xff], [0x7f, 1], [0x7f, 2], [0x80, 1], [0x80, 0xff],
  [0x80, 0x80], [0xff, 2], [0xfe, 0xff], [0x40, 4], [0x81, 2], [1, 0x80], [0x7e, 1], [0xfe, 1]];
const tails = [[3], [0], [2], [4, 0x24], [0x44, 0x8f, 0x80], [0x45, 0x80], [0x83, 0x10, 0, 0, 0], [5, 0x10, 0x50, 0, 0]];
const stats = {contexts: 0, numeric_cases: 0, success: 0, ea_cases: 0, boundaries: 0, read_faults: 0, repairs: 0, chains: 0, runs: 0, preflight: 0, controls: 0};
const observations = [], modules = [], frames = [];
function record(magic, size, fields, version = 1) {
  const b = Buffer.alloc(size); b.write(magic); b.writeUInt16LE(version, 4); b.writeUInt16LE(1, 6); b.writeUInt32LE(size, 8);
  fields.forEach((v, i) => b.writeUInt32LE(v >>> 0, 16 + i * 4)); return b;
}
const state = (r, pc, f) => record('R3ST', 56, [...r, pc, f]);
const exit = (reason, retired, detail = 0, address = 0) => record('R3EX', 40, [reason, retired, detail, address, reason === 5 ? 1 : 0, reason === 5 ? 1 : 0], 2);
const helper = (value = 0, detail = 0, address = 0) => record('R3MH', 40, [detail ? 1 : 0, detail ? 0 : value, detail, address, detail ? 1 : 0, 1], 2);
const words = a => {const b = Buffer.alloc(a.length * 4); a.forEach((v, i) => b.writeUInt32LE(v >>> 0, i * 4)); return b;};
function refresh(c) {c.bytes = new Uint8Array(c.memory.buffer); c.view = new DataView(c.memory.buffer); return c;}
function arena(c) {return Buffer.from(refresh(c).bytes.subarray(c.base, c.base + SIZE));}
function capture(c, before, after, data) {
  const beforeFrame = frames.length; frames.push(before, after);
  observations.push({context: c.label, before_frame: beforeFrame, after_frame: beforeFrame + 1, ...data});
}
function pure(c, fn) {const before = arena(c); assert.equal(fn(), 0); assert.deepEqual(arena(c), before);}
function request(c, b) {assert.ok(b.length <= 4096); refresh(c).bytes.set(b, c.base + TRANSFER);}
function upload(c, address, b) {request(c, b); pure(c, () => c.api.upload(address, b.length));}
function put(c, address, byte) {upload(c, address, Buffer.from([byte]));}
function map(c, address) {pure(c, () => c.api.map(address, 1, 3));}
function protect(c, address, permission) {pure(c, () => c.api.protect(address, 1, permission));}
function seed(c, r, pc, f) {
  refresh(c).bytes.set(state(r, pc, f), c.base); c.bytes.set(exit(3, 0), c.base + 56);
  c.view.setUint32(c.base + 96, 0, true); c.bytes.set(helper(0xad), c.base + 100);
}
function program(name) {
  const saved = readFileSync(join(output, `${name}.x86`));
  if (name === 'chain') {assert.deepEqual(saved, Buffer.from([0xb8, 0x11, 0, 0, 0x50, 0xf6, 0x20, 0xf6, 0x23, 0xf6, 0x23, 0xeb, 0])); return {saved, pc: CHAIN, specs: [[CHAIN, 13]]};}
  const wanted = Buffer.alloc(128, 0xcc), specs = [];
  for (const [index, tail] of tails.entries()) {
    const at = index * 16, form = Buffer.from([0xf6, ...tail]); form[1] |= name === 'unsigned' ? 0x20 : 0x28;
    wanted.set(form, at); wanted[at + form.length] = 0xe9; wanted.writeInt32LE(COLD - (PC + at + form.length + 5), at + form.length + 1);
    specs.push([PC + at, form.length + 5]);
  }
  assert.deepEqual(saved, wanted); assert.equal(specs.length, 8); return {saved, pc: PC, specs};
}
function fresh(owner, entries, name) {
  const instance = new WebAssembly.Instance(engineModule, {}), names = ['open', 'close', 'arena_ptr', 'map', 'unmap', 'protect', 'upload', 'compile', 'compile_entries', 'compile_resident', 'compile_resident_entries', 'generation', 'module_ptr', 'module_len', 'guard', 'guard_resident', 'read8'];
  const api = Object.fromEntries(names.map(n => [n, instance.exports[`ring3_abi_v1_${n}`]])); for (const fn of Object.values(api)) assert.equal(typeof fn, 'function');
  const c = {owner, entries, name, label: `${owner}-${entries ? 'entries' : 'extent'}-${name}`, api, memory: instance.exports.memory, low: ++stats.contexts, high: 0xc9486000};
  assert.equal(api.open(5, c.low, c.high), 0); c.base = api.arena_ptr() >>> 0; assert.deepEqual(arena(c), initial);
  pure(c, () => api.map(PC, 1, 7)); for (const at of [DATA, ALIAS, TOP, 0]) map(c, at);
  const p = program(name); upload(c, p.pc, p.saved); put(c, 0x1fff, 5); protect(c, PC, 5);
  request(c, words(entries ? p.specs.map(s => s[0]) : p.specs.flat())); let binding;
  if (owner === 'replacement') {
    pure(c, () => entries ? api.compile_entries(p.specs.length, 0) : api.compile(p.specs.length));
    binding = {generation: api.generation(), pointer: api.module_ptr() >>> 0, length: api.module_len() >>> 0};
  } else {
    const before = arena(c); assert.equal(entries ? api.compile_resident_entries(p.specs.length, 0) : api.compile_resident(p.specs.length), 0); refresh(c);
    binding = Object.fromEntries(['low', 'high', 'pointer', 'length'].map((n, i) => [n, c.view.getUint32(c.base + TRANSFER + 8 + i * 4, true)]));
    before.set(words([1, 24, binding.low, binding.high, binding.pointer, binding.length]), TRANSFER); assert.deepEqual(arena(c), before);
  }
  const generated = Buffer.from(refresh(c).bytes.subarray(binding.pointer, binding.pointer + binding.length)), module = new WebAssembly.Module(generated);
  assert.deepEqual(WebAssembly.Module.imports(module), [{module: 'env', name: 'memory', kind: 'memory'}, {module: 'ring3', name: owner === 'replacement' ? 'guard' : 'guard_resident', kind: 'function'}, {module: 'ring3', name: 'read8', kind: 'function'}]);
  writeFileSync(join(output, `actual-${c.label}.wasm`), generated, {flag: 'wx'}); modules.push({context: c.label, bytes: generated.length, sha256: hash(generated), specs: p.specs});
  return [c, {...binding, run: new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3: api}).exports.run}];
}
function run(c, u, budget, r, pc, f, reason = 1, retired = 1, value, detail = 0, address = 0, input = {}) {
  const before = arena(c), wanted = Buffer.from(before); wanted.set(state(r, pc, f)); wanted.set(exit(reason, retired, detail, address), 56);
  if (value !== undefined || reason === 5) wanted.set(helper(value, detail, address), 100);
  const status = u.run(c.base, c.base + 56, budget, c.base + 96), after = arena(c);
  assert.equal(status, 0); assert.deepEqual(after, wanted, 'full arena: precise retirement, current AL product, defined CF/OF, other GPR/transfer/x87 preserved');
  capture(c, before, after, {type: 'run', budget, status, input}); stats.runs++;
}
function neutral(c, u, status, statePointer, label) {
  const before = arena(c), actual = u.run(statePointer, c.base + 56, 1, c.base + 96), after = arena(c);
  assert.equal(actual, status); assert.deepEqual(after, before); capture(c, before, after, {type: label, status: actual});
}
function multiply(kind, r, byte, incoming) {
  const al = r[0] & 255, left = BigInt(kind === 'signed' && al >= 128 ? al - 256 : al), right = BigInt(kind === 'signed' && byte >= 128 ? byte - 256 : byte);
  const product = left * right, overflow = kind === 'unsigned' ? product > 255n : product < -128n || product > 127n, registers = [...r];
  registers[0] = Number((BigInt(r[0]) & 0xffff0000n) | BigInt.asUintN(16, product));
  return {registers, flags: (incoming & 0x402) | Number(overflow) * 0x801, product: product.toString(), overflow: Number(overflow)};
}
for (const [kind, al, byte, ax, flags] of [['unsigned', 0, 0xff, 0, 0x402], ['unsigned', 0xff, 0xff, 0xfe01, 0xc03],
  ['signed', 0x80, 1, 0xff80, 0x402], ['signed', 0x80, 0xff, 0x80, 0xc03], ['signed', 0x7f, 2, 0xfe, 0xc03],
  ['signed', 0xff, 2, 0xfffe, 0x402], ['signed', 0x80, 0x80, 0x4000, 0xc03], ['unsigned', 0x7f, 2, 0xfe, 0x402]]) {
  const r = [...REG]; r[0] = (0xa53c5a00 | al) >>> 0; const result = multiply(kind, r, byte, 0xcd7);
  assert.deepEqual([result.registers[0] & 65535, result.flags], [ax, flags], 'literal signed/unsigned full-product and CF/OF anchor');
}
for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  for (const kind of ['unsigned', 'signed']) {
    const [c, u] = fresh(owner, entries, kind); seed(c, REG, PC, 2);
    neutral(c, u, 1, 0xffffffff, 'invalid-pointer'); stats.preflight++;
    refresh(c).view.setUint32(c.base + 12, 1, true); neutral(c, u, 2, c.base, 'invalid-state'); refresh(c).view.setUint32(c.base + 12, 0, true); stats.preflight++;
    run(c, u, 0, REG, PC, 2, 1, 0); refresh(c).view.setUint32(c.base + 96, 1, true); run(c, u, 1, REG, PC, 2, 2, 0); refresh(c).view.setUint32(c.base + 96, 0, true);
    for (const [row, [al, byte]] of rows.entries()) for (const flags of FLAGS) {
      const r = [...REG]; r[0] = (0xa53c5a00 | al) >>> 0; put(c, DATA + 24, byte); seed(c, r, PC, flags);
      const result = multiply(kind, r, byte, flags), input = {kind, row, registers: r, flags, source_byte: byte, address: DATA + 24}; stats.numeric_cases++;
      run(c, u, 1, result.registers, PC + 2, result.flags, 1, 1, byte, 0, 0, input);
      run(c, u, 1, result.registers, COLD, result.flags); run(c, u, 1, result.registers, COLD, result.flags, 3, 0); stats.success++;
    }
    for (let form = 0; form < 8; form++) {
      const r = [...REG]; let address = DATA + 24;
      if (form === 1) {r[0] = ALIAS + 17; address = ALIAS + 17;}
      if (form === 2) r[2] = DATA + 24;
      if (form === 3) r[4] = DATA + 24;
      if (form === 4) {r[7] = DATA + 24; r[1] = 32;}
      if (form === 5) r[5] = DATA + 152;
      if (form === 6) r[3] = DATA + 8;
      if (form === 7) address = DATA + 16;
      put(c, address, 5); seed(c, r, PC + form * 16, 0xcd7); const result = multiply(kind, r, 5, 0xcd7);
      run(c, u, 1, result.registers, PC + form * 16 + 1 + tails[form].length, result.flags, 1, 1, 5, 0, 0, {kind, form, registers: r, address, source_byte: 5}); stats.ea_cases++;
    }
    const wrapped = [...REG]; wrapped[3] = 0xfffffff8; put(c, 8, 0xff); seed(c, wrapped, PC + 96, 0xcd7); const wrapResult = multiply(kind, wrapped, 0xff, 0xcd7);
    run(c, u, 1, wrapResult.registers, PC + 102, wrapResult.flags, 1, 1, 0xff, 0, 0, {kind, wrap: true, form: 6, registers: wrapped, address: 8, source_byte: 0xff}); stats.ea_cases++;
    for (const address of [0x1fff, 0xffffffff]) {
      const r = [...REG]; r[3] = address; if (address !== 0x1fff) put(c, address, 5);
      seed(c, r, PC, 0xcd7); const result = multiply(kind, r, 5, 0xcd7);
      run(c, u, 1, result.registers, PC + 2, result.flags, 1, 1, 5, 0, 0, {kind, boundary: address, registers: r, source_byte: 5}); stats.boundaries++;
    }
    for (const detail of [1, 2]) {
      const r = [...REG]; put(c, DATA + 24, 0xff); if (detail === 1) pure(c, () => c.api.unmap(DATA, 1)); else protect(c, DATA, 2);
      seed(c, r, PC, 0xcd7); run(c, u, 1, r, PC, 0xcd7, 5, 0, undefined, detail, DATA + 24, {kind, read_fault: detail, registers: r});
      run(c, u, 1, r, PC, 0xcd7, 5, 0, undefined, detail, DATA + 24); stats.read_faults++;
      if (detail === 1) map(c, DATA); else protect(c, DATA, 3);
      put(c, DATA + 24, 5); const result = multiply(kind, r, 5, 0xcd7);
      run(c, u, 1, result.registers, PC + 2, result.flags, 1, 1, 5); stats.repairs++;
    }
    protect(c, PC, 7); upload(c, PC, Buffer.from([0xf6])); neutral(c, u, 4, c.base, 'stale'); stats.controls++;
    assert.equal(c.api.close(), 0); neutral(c, u, 5, c.base, 'closed'); stats.controls++;
  }
  const [c, u] = fresh(owner, entries, 'chain'), r = [...REG]; put(c, ALIAS + 17, 5); pure(c, () => c.api.unmap(DATA, 1)); seed(c, r, CHAIN, 0xcd7);
  r[0] = ALIAS + 17; run(c, u, 1, r, CHAIN + 5, 0xcd7);
  r[0] = 0x50000055; run(c, u, 9, r, CHAIN + 7, 0x402, 5, 1, undefined, 1, DATA + 24, {phase: 'completed-eax-ea-prefix'});
  run(c, u, 9, r, CHAIN + 7, 0x402, 5, 0, undefined, 1, DATA + 24);
  map(c, DATA); const beforeRepair = arena(c); put(c, DATA + 24, 5); const afterRepair = arena(c), wantedRepair = Buffer.from(beforeRepair); wantedRepair[TRANSFER] = 5;
  assert.deepEqual(afterRepair, wantedRepair, 'RAM-only source repair leaves retained CPU/exit/helper/tail unchanged'); capture(c, beforeRepair, afterRepair, {type: 'host-ram-repair', address: DATA + 24, value: 5});
  r[0] = 0x500001a9; run(c, u, 1, r, CHAIN + 9, 0xc03, 1, 1, 5, 0, 0, {phase: 'repaired-current-al'});
  r[0] = 0x5000034d; run(c, u, 1, r, CHAIN + 11, 0xc03, 1, 1, 5, 0, 0, {phase: 'next-current-al'});
  run(c, u, 1, r, CHAIN + 13, 0xc03); run(c, u, 1, r, CHAIN + 13, 0xc03, 3, 0); stats.chains++; assert.equal(c.api.close(), 0);
}
assert.equal(stats.contexts, 12); assert.equal(stats.numeric_cases, 4 * 2 * 16 * 2); assert.equal(stats.success, stats.numeric_cases);
assert.equal(stats.ea_cases, 4 * 2 * 9); assert.equal(stats.boundaries, 4 * 2 * 2); assert.equal(stats.read_faults, 4 * 2 * 2); assert.equal(stats.repairs, 4 * 2 * 2);
assert.equal(stats.chains, 4); assert.equal(stats.runs, 948); assert.equal(stats.preflight, 16); assert.equal(stats.controls, 16); assert.equal(observations.length, 984);
const raw = Buffer.concat(frames); writeFileSync(join(output, 'arenas.bin'), raw, {flag: 'wx'});
const result = {status: 'ok', stats, modules, observations, raw: {file: 'arenas.bin', frames: frames.length, bytes: raw.length, sha256: hash(raw)}, engine_sha256: hash(engineBytes), arena_bytes: SIZE,
  tools: {node: process.version, v8: process.versions.v8}, sources: Object.fromEntries(['engine/tests/cpu_memory_byte_accumulator_multiply.rs', 'engine/tests/cpu_memory_byte_accumulator_multiply_wasm.rs', 'engine/tests/fixtures/p2-memory-byte-accumulator-multiply/run.mjs'].map(p => [p, hash(readFileSync(join(root, p)))])),
  scope: 'Memory BYTE MUL/one-operand IMUL F6/4,/5 only; four bound extent/entry replacement/resident profiles, sixteen finite AL/source rows/two FLAGS seeds, nine32EA witnesses including successful oldEAX/EDX and wrap, single-byte1FFF/FFFFFFFF endpoints, Read8 faults before arithmetic, sameCPU/module RAM-only repair/currentAL prefix chain. Full4364 arena pairs for every generated run/preflight/stale/closed attempt and four captured hostrepair pairs. Default opaque initialized x87 tail preserved. No arbitraryFPseed/directguard/rawRAMdump/hardware undefinedFLAGS/browser/game/performance/fullCI claim.'};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2), {flag: 'wx'}); console.log(JSON.stringify({status: result.status, stats, engine_sha256: result.engine_sha256, output}));
