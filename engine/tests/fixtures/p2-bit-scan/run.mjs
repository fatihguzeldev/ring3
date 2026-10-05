import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(root, 'expected actual engine wasm, output directory and repository root');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const engineBytes = readFileSync(enginePath), engineModule = new WebAssembly.Module(engineBytes);
assert.deepEqual(WebAssembly.Module.imports(engineModule), []);
const SIZE = 4236, TRANSFER = 140, PC = 0x1000, COLD = 0x1080;
const REG = [0x1234807f, 0x23457f80, 0x34560f10, 0x4567ff00, 0x56789abc, 0x6789abcd, 0x789abcde, 0x89abcdef];
const INPUTS = [0, ...Array.from({length: 32}, (_, bit) => 2 ** bit), 0x101, 0x80000001, 0x01010101, 0xffffffff, 0xdeadbeef];
const FLAGS = [0, 0x400].flatMap(df => [0, 0x40].flatMap(zf => [0, 4].flatMap(pf => [0, 0x891].map(other => 2 + df + zf + pf + other))));
const ANCHORS = [[0, null, null, true], [1, 0, 0, false], [0x100, 8, 8, false], [0x101, 0, 8, true],
  [0x80000000, 31, 31, false], [0x80000001, 0, 31, true], [0x01010101, 0, 24, true], [0xffffffff, 0, 31, true], [0xdeadbeef, 0, 31, true]];
function scan(kind, value) {
  const bits = value.toString(2).padStart(32, '0'), ones = bits.replaceAll('0', '').length;
  return {index: value === 0 ? null : 31 - (kind === 'forward' ? bits.lastIndexOf('1') : bits.indexOf('1')), even: ones % 2 === 0, zero: value === 0};
}
for (const [value, low, high, even] of ANCHORS) {
  assert.deepEqual(scan('forward', value), {index: low, even, zero: value === 0});
  assert.deepEqual(scan('reverse', value), {index: high, even, zero: value === 0});
}
assert.equal(new Set(INPUTS).size, 38); assert.equal(new Set(FLAGS).size, 16);
assert.ok(FLAGS.every(flags => (flags & ~0xcd7) === 0 && (flags & 2) !== 0));
const programs = Object.fromEntries(['forward', 'reverse'].flatMap(kind => ['alias', 'rotation'].map(mode => {
  const bytes = Buffer.alloc(130, 0xcc), opcode = kind === 'forward' ? 0xbc : 0xbd;
  for (let destination = 0; destination < 8; destination++) {
    const source = mode === 'alias' ? destination : (destination + 1) % 8, offset = destination * 11;
    bytes.set([0x0f, opcode, 0xc0 | destination << 3 | source, 0x0f, 0x94, 0xc0, 0x0f, 0x9a, 0xc2, 0xeb, 0x80 - offset - 11], offset);
  }
  bytes.set([0x0f, 0x0b], 128); const name = `${kind}-${mode}`;
  assert.deepEqual(readFileSync(join(output, `${name}.x86`)), bytes); return [name, bytes];
})));
const specs = Array.from({length: 8}, (_, destination) => [PC + destination * 11, 11]);
const counts = {contexts: 0, modules: 0, seeds: 0, bsf: 0, bsr: 0, zero_sources: 0, rotated_zero_destination_preserved: 0,
  setz: 0, setp: 0, jumps: 0, generated_runs: 0, guard_controls: 0, pages: 0, arena_checks: 0};
const modules = [];
function record(magic, size, fields) {
  const bytes = Buffer.alloc(size); bytes.write(magic); bytes.writeUInt16LE(1, 4); bytes.writeUInt16LE(1, 6); bytes.writeUInt32LE(size, 8);
  fields.forEach((value, index) => bytes.writeUInt32LE(value >>> 0, 16 + index * 4)); return bytes;
}
const state = (registers, pc, flags) => record('R3ST', 56, [...registers, pc, flags]);
const exit = (reason, retired) => record('R3EX', 40, [reason, retired, 0, 0, 0, 0]);
const helper = value => record('R3MH', 40, [0, value, 0, 0, 0, 0]);
const words = values => {const bytes = Buffer.alloc(values.length * 4); values.forEach((value, index) => bytes.writeUInt32LE(value >>> 0, index * 4)); return bytes;};
const pattern = Buffer.from(Array.from({length: 4096}, (_, index) => index % 251 + 1));
function refresh(ctx) {ctx.bytes = new Uint8Array(ctx.memory.buffer); ctx.view = new DataView(ctx.memory.buffer); return ctx;}
function arena(ctx) {return Buffer.from(refresh(ctx).bytes.subarray(ctx.base, ctx.base + SIZE));}
function check(ctx, label) {assert.deepEqual(arena(ctx), ctx.expected, `${ctx.owner}/${ctx.name}/${label}: complete arena`); counts.arena_checks++;}
function pure(ctx, action, label, status = 0) {check(ctx, `before ${label}`); assert.equal(action(), status, label); check(ctx, label);}
function request(ctx, bytes) {refresh(ctx).bytes.set(bytes, ctx.base + TRANSFER); ctx.expected.set(bytes, TRANSFER); check(ctx, 'host request');}
function fresh(owner, kind, mode) {
  const expected = Buffer.alloc(SIZE), ordinal = ++counts.contexts;
  expected.set(state(Array(8).fill(0), 0, 2)); expected.set(exit(1, 0), 56); expected.set(helper(0), 100);
  const ctx = {owner, kind, mode, name: `${kind}-${mode}`, expected, low: ordinal, high: 0xc8398000};
  if (owner === 'standalone') {
    ctx.memory = new WebAssembly.Memory({initial: 1}); ctx.base = 128; ctx.dataAddress = 0x8000;
    refresh(ctx).bytes.set(expected, ctx.base); ctx.bytes.set(pattern, ctx.dataAddress);
  } else {
    const instance = new WebAssembly.Instance(engineModule, {});
    const arities = {open: 3, close: 0, arena_ptr: 0, map: 3, upload: 2, read32: 1, write8: 2, compile_entries: 2,
      compile_resident: 1, generation: 0, module_ptr: 0, module_len: 0, guard: 6, guard_resident: 7};
    ctx.api = Object.fromEntries(Object.entries(arities).map(([name, arity]) => {
      const fn = instance.exports[`ring3_abi_v1_${name}`]; assert.equal(typeof fn, 'function', name); assert.equal(fn.length, arity, name); return [name, fn];
    }));
    ctx.memory = instance.exports.memory; assert.equal(ctx.api.open(3, ctx.low, ctx.high), 0);
    ctx.base = ctx.api.arena_ptr() >>> 0; ctx.dataAddress = 0x3000; check(ctx, 'independently initialized arena');
    pure(ctx, () => ctx.api.map(ctx.dataAddress, 1, 3), 'data map'); request(ctx, pattern);
    pure(ctx, () => ctx.api.upload(ctx.dataAddress, pattern.length), 'pattern upload');
  }
  ctx.expected.set(pattern, TRANSFER); refresh(ctx).bytes.set(pattern, ctx.base + TRANSFER); check(ctx, 'patterned transfer'); return ctx;
}
function compile(ctx) {
  let bytes, binding = {};
  if (ctx.owner === 'standalone') bytes = readFileSync(join(output, `standalone-${ctx.name}.wasm`));
  else {
    pure(ctx, () => ctx.api.map(PC, 1, 7), 'code map'); request(ctx, programs[ctx.name]);
    pure(ctx, () => ctx.api.upload(PC, programs[ctx.name].length), 'code upload');
    request(ctx, words(ctx.owner === 'replacement' ? specs.map(([entry]) => entry) : specs.flat()));
    if (ctx.owner === 'replacement') {
      pure(ctx, () => ctx.api.compile_entries(specs.length, 0), 'eight-entry compiler');
      binding = {generation: ctx.api.generation(), pointer: ctx.api.module_ptr() >>> 0, length: ctx.api.module_len() >>> 0}; assert.ok(binding.generation > 0);
    } else {
      check(ctx, 'before eight-extent compiler'); assert.equal(ctx.api.compile_resident(specs.length), 0); refresh(ctx);
      assert.equal(ctx.view.getUint32(ctx.base + TRANSFER, true), 1); assert.equal(ctx.view.getUint32(ctx.base + TRANSFER + 4, true), 24);
      binding = Object.fromEntries(['low', 'high', 'pointer', 'length'].map((name, index) => [name, ctx.view.getUint32(ctx.base + TRANSFER + 8 + index * 4, true)]));
      assert.ok(binding.low !== 0 || binding.high !== 0);
      ctx.expected.set(words([1, 24, binding.low, binding.high, binding.pointer, binding.length]), TRANSFER); check(ctx, 'resident metadata only');
    }
    refresh(ctx); assert.ok(binding.pointer > 0 && binding.length > 8 && binding.pointer + binding.length <= ctx.bytes.length);
    bytes = Buffer.from(ctx.bytes.subarray(binding.pointer, binding.pointer + binding.length));
    writeFileSync(join(output, `${ctx.owner}-${ctx.name}.wasm`), bytes, {flag: 'wx'});
  }
  const module = new WebAssembly.Module(bytes), guard = ctx.owner === 'replacement' ? 'guard' : 'guard_resident';
  assert.deepEqual(WebAssembly.Module.imports(module), [{module: 'env', name: 'memory', kind: 'memory'},
    ...(ctx.owner === 'standalone' ? [] : [{module: 'ring3', name: guard, kind: 'function'}])]);
  assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  const child = new WebAssembly.Instance(module, {env: {memory: ctx.memory}, ...(ctx.owner === 'standalone' ? {} : {ring3: ctx.api})});
  assert.equal(child.exports.run.length, 4); counts.modules++;
  modules.push({owner: ctx.owner, kind: ctx.kind, mode: ctx.mode, sha256: hash(bytes), length: bytes.length, ...binding}); return {...binding, run: child.exports.run};
}
function seed(ctx, registers, pc, flags) {
  ctx.expected.set(state(registers, pc, flags)); ctx.expected.set(exit(3, 0), 56); ctx.expected.writeUInt32LE(0, 96);
  refresh(ctx).bytes.set(ctx.expected.subarray(0, 100), ctx.base); check(ctx, 'initial cpu'); counts.seeds++;
}
function cancel(ctx, value) {ctx.expected.writeUInt32LE(value, 96); refresh(ctx).view.setUint32(ctx.base + 96, value, true);}
function run(ctx, unit, budget, label, registers, pc, flags, reason = 1, retired = 1, status = 0, malformed = false) {
  check(ctx, `before ${label}`);
  if (status === 0) {ctx.expected.set(state(registers, pc, flags)); ctx.expected.set(exit(reason, retired), 56);}
  const pointers = malformed ? [0xffffffff, 0xffffffff, 0xffffffff] : [ctx.base, ctx.base + 56, ctx.base + 96];
  assert.equal(unit.run(pointers[0], pointers[1], budget, pointers[2]), status, label); check(ctx, label); counts.generated_runs++;
}
function page(ctx) {
  const actual = Buffer.alloc(4096);
  if (ctx.owner === 'standalone') actual.set(refresh(ctx).bytes.subarray(ctx.dataAddress, ctx.dataAddress + 4096));
  else for (let offset = 0; offset < 4096; offset += 4) {
    check(ctx, 'before diagnostic read'); assert.equal(ctx.api.read32(ctx.dataAddress + offset), 0); refresh(ctx);
    actual.writeUInt32LE(ctx.view.getUint32(ctx.base + 120, true), offset);
    ctx.expected.set(helper(pattern.readUInt32LE(offset)), 100); check(ctx, 'diagnostic read changes helper only');
  }
  assert.deepEqual(actual, pattern, 'complete patterned page unchanged'); counts.pages++;
}
function numeric(ctx, unit) {
  for (const value of INPUTS) for (let destination = 0; destination < 8; destination++) for (const initialFlags of FLAGS) {
    const registers = [...REG], source = ctx.mode === 'alias' ? destination : (destination + 1) % 8, entry = PC + destination * 11;
    registers[source] = value; seed(ctx, registers, entry, initialFlags);
    if (value === 0 && destination === 0 && initialFlags === FLAGS[0]) {
      run(ctx, unit, 0, 'zero budget before first scan', registers, entry, initialFlags, 1, 0); counts.guard_controls++;
      cancel(ctx, 1); run(ctx, unit, 1, 'cancel before first scan', registers, entry, initialFlags, 2, 0); counts.guard_controls++; cancel(ctx, 0);
    }
    const result = scan(ctx.kind, value), flags = 2 + (initialFlags & 0x400) + (result.zero ? 0x40 : 0) + (result.even ? 4 : 0);
    if (!result.zero) registers[destination] = result.index;
    else {counts.zero_sources++; if (ctx.mode === 'rotation') {assert.equal(registers[destination], REG[destination]); assert.notEqual(registers[destination], 0); counts.rotated_zero_destination_preserved++;}}
    run(ctx, unit, 1, `source-${value}-dst-${destination}-flags-${initialFlags}-scan`, registers, entry + 3, flags); counts[ctx.kind === 'forward' ? 'bsf' : 'bsr']++;
    registers[0] = registers[0] - registers[0] % 256 + Number(result.zero);
    run(ctx, unit, 1, 'SETZ observes guest-produced ZF', registers, entry + 6, flags); counts.setz++;
    registers[2] = registers[2] - registers[2] % 256 + Number(result.even);
    run(ctx, unit, 1, 'SETP observes whole-source parity', registers, entry + 9, flags); counts.setp++;
    run(ctx, unit, 1, 'following jump', registers, COLD, flags); counts.jumps++;
  }
}
function finish(ctx, unit) {
  const registers = Array.from({length: 8}, (_, index) => ctx.expected.readUInt32LE(16 + index * 4));
  const flags = ctx.expected.readUInt32LE(52);
  for (const [name, cancelled, budget, reason] of [['zero-budget', 0, 0, 1], ['cancel-positive-budget', 1, 1, 2], ['cancel-zero-budget', 1, 0, 2]]) {
    cancel(ctx, cancelled); run(ctx, unit, budget, name, registers, COLD, flags, reason, 0); counts.guard_controls++;
  }
  cancel(ctx, 0); run(ctx, unit, 1, 'cold-entry', registers, COLD, flags, 3, 0); counts.guard_controls++;
  run(ctx, unit, 0, 'malformed-pointers', registers, COLD, flags, 0, 0, 1, true); counts.guard_controls++; page(ctx);
  if (ctx.owner === 'standalone') return;
  for (const wrong of ['key', 'identity']) {
    const low = wrong === 'key' ? ctx.low ^ 1 : ctx.low;
    const action = ctx.owner === 'replacement'
      ? () => ctx.api.guard(low, ctx.high, unit.generation + (wrong === 'identity' ? 1 : 0), ctx.base, ctx.base + 56, ctx.base + 96)
      : () => ctx.api.guard_resident(low, ctx.high, unit.low ^ (wrong === 'identity' ? 1 : 0), unit.high, ctx.base, ctx.base + 56, ctx.base + 96);
    pure(ctx, action, `wrong-${wrong}`, 3); counts.guard_controls++;
  }
  check(ctx, 'before same-byte invalidation'); assert.equal(ctx.api.write8(PC, programs[ctx.name][0]), 0);
  const updatedHelper = record('R3MH', 40, [0, 0, 0, 0, 0, 1]); updatedHelper.writeUInt16LE(3, 4);
  ctx.expected.set(updatedHelper, 100); check(ctx, 'same-byte invalidation changes helper only');
  run(ctx, unit, 0, 'stale-before-pointer-validation', registers, COLD, flags, 0, 0, 4, true); counts.guard_controls++;
  pure(ctx, () => ctx.api.close(), 'close');
  run(ctx, unit, 0, 'closed-before-pointer-validation', registers, COLD, flags, 0, 0, 5, true); counts.guard_controls++;
}
for (const owner of ['standalone', 'replacement', 'resident']) for (const kind of ['forward', 'reverse']) for (const mode of ['alias', 'rotation']) {
  const ctx = fresh(owner, kind, mode), unit = compile(ctx); page(ctx); numeric(ctx, unit); finish(ctx, unit);
}
assert.equal(counts.contexts, 12); assert.equal(counts.modules, 12); assert.equal(counts.seeds, 58368);
assert.equal(counts.bsf, 29184); assert.equal(counts.bsr, 29184); assert.equal(counts.zero_sources, 1536); assert.equal(counts.rotated_zero_destination_preserved, 768);
assert.equal(counts.setz, 58368); assert.equal(counts.setp, 58368); assert.equal(counts.jumps, 58368);
assert.equal(counts.generated_runs, 233572); assert.equal(counts.guard_controls, 116); assert.equal(counts.pages, 24);
const result = {status: 'ok', engine_sha256: hash(engineBytes), tools: {node: process.version, v8: process.versions.v8}, counts, modules,
  oracle: {sources: INPUTS, flag_seeds: FLAGS, literal_anchors: ANCHORS, registers: REG, specs, cold_target: COLD,
    flag_policy: 'ZF iffzero; PF whole32 source even ones count; CF/OF/SF/AF0; DF/fixed2 preserved; zero source retains destination',
    programs: Object.fromEntries(Object.entries(programs).map(([name, bytes]) => [name, bytes.toString('hex')])), arena_bytes: SIZE, page_sha256: hash(pattern)},
  test_sha256: Object.fromEntries(['engine/tests/cpu_bit_scan_wasm.rs', 'engine/tests/fixtures/p2-bit-scan/run.mjs'].map(path => [path, hash(readFileSync(join(root, path)))])),
  claim: 'flat32 register-source BSF/BSR in native-generated standalone and actual engine-Wasm-generated replacement/resident modules;zero/all32 single bits/five mixed source rows,16 FLAGS basis, all8 destinations and self-alias/rotated source; independent bit-string first/last-one and whole-source parity oracle with literal anchors; first scan full-state observation precedes SETZAL/SETPDL/JMP, no intermediate CPU repair, dirty rotated destination preserved on zero, full arena and complete untouched patterned4KB page; focused known-entry preflight and stale/closed guards; no exhaustive operands/128FLAGS/memory-source/POPCNT/TZCNT/LZCNT/wider ISA, PE/provider, SDK/browser, performance or game claim'};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2), {flag: 'wx'});
console.log(JSON.stringify({status: result.status, engine_sha256: result.engine_sha256, counts, output}));
