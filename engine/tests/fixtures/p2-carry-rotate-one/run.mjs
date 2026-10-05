import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(root, 'expected actual engine Wasm, output directory and repository root');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const engineBytes = readFileSync(enginePath), engineModule = new WebAssembly.Module(engineBytes);
assert.deepEqual(WebAssembly.Module.imports(engineModule), []);
const SIZE = 4236, TRANSFER = 140, PC = 0x1000, COLD = 0x1080;
const REG = [0x1234807f, 0x23457f80, 0x34560f10, 0x4567ff00, 0x56789abc, 0x6789abcd, 0x789abcde, 0x89abcdef];
const INPUTS = [0, 1, 2, 0x100, 0x40000000, 0x80000000, 0x7fffffff, 0xffffffff, 0x80000001, 0x12345678, 0xaaaaaaaa, 0x55555555];
const ECX = [0xa1b2c300, 0xa1b2c301, 0xa1b2c31f, 0xa1b2c320, 0xa1b2c3ff];
const FLAG_BITS = [1, 4, 0x10, 0x40, 0x80, 0x400, 0x800];
const FLAGS = Array.from({length: 128}, (_, seed) => 2 + FLAG_BITS.reduce((flags, bit, index) => flags + (seed & 2 ** index ? bit : 0), 0));
const ANCHORS = [[0, 0, 0, 0, 0, 0, 0, 0], [0, 1, 1, 0, 0, 0x80000000, 0, 1],
  [1, 0, 2, 0, 0, 0, 1, 0], [1, 1, 3, 0, 0, 0x80000000, 1, 1],
  [2, 0, 4, 0, 0, 1, 0, 0], [2, 1, 5, 0, 0, 0x80000001, 0, 1],
  [0x100, 0, 0x200, 0, 0, 0x80, 0, 0], [0x100, 1, 0x201, 0, 0, 0x80000080, 0, 1],
  [0x40000000, 0, 0x80000000, 0, 1, 0x20000000, 0, 0], [0x40000000, 1, 0x80000001, 0, 1, 0xa0000000, 0, 1],
  [0x80000000, 0, 0, 1, 1, 0x40000000, 0, 1], [0x80000000, 1, 1, 1, 1, 0xc0000000, 0, 0],
  [0x7fffffff, 0, 0xfffffffe, 0, 1, 0x3fffffff, 1, 0], [0x7fffffff, 1, 0xffffffff, 0, 1, 0xbfffffff, 1, 1],
  [0xffffffff, 0, 0xfffffffe, 1, 0, 0x7fffffff, 1, 1], [0xffffffff, 1, 0xffffffff, 1, 0, 0xffffffff, 1, 0],
  [0x80000001, 0, 2, 1, 1, 0x40000000, 1, 1], [0x80000001, 1, 3, 1, 1, 0xc0000000, 1, 0],
  [0x12345678, 0, 0x2468acf0, 0, 0, 0x091a2b3c, 0, 0], [0x12345678, 1, 0x2468acf1, 0, 0, 0x891a2b3c, 0, 1],
  [0xaaaaaaaa, 0, 0x55555554, 1, 1, 0x55555555, 0, 1], [0xaaaaaaaa, 1, 0x55555555, 1, 1, 0xd5555555, 0, 0],
  [0x55555555, 0, 0xaaaaaaaa, 0, 1, 0x2aaaaaaa, 1, 0], [0x55555555, 1, 0xaaaaaaab, 0, 1, 0xaaaaaaaa, 1, 1]];
function rotate(kind, value, incoming) {
  const old = String(incoming) + value.toString(2).padStart(32, '0');
  const rotated = kind === 'left' ? old.slice(1) + old[0] : old[32] + old.slice(0, 32);
  const cf = Number(rotated[0]), result = rotated.slice(1);
  return {value: parseInt(result, 2), cf, of: Number(kind === 'left' ? result[0] !== rotated[0] : old[1] !== old[0])};
}
const rotateFlags = (before, result) => [2, 4, 0x10, 0x40, 0x80, 0x400].reduce((flags, bit) => flags + (before & bit ? bit : 0), 0) + result.cf + result.of * 0x800;
const PRODUCERS = [{name: 'CLC', destination: 0, opcode: 0xf8, value: 0x80000001},
  {name: 'STC', destination: 1, opcode: 0xf9, value: 0x40000000}, {name: 'CMC', destination: 2, opcode: 0xf5, value: 0x12345678}];
for (const [value, incoming, left, leftCf, leftOf, right, rightCf, rightOf] of ANCHORS) {
  assert.deepEqual(rotate('left', value, incoming), {value: left, cf: leftCf, of: leftOf});
  assert.deepEqual(rotate('right', value, incoming), {value: right, cf: rightCf, of: rightOf});
}
assert.equal(new Set(INPUTS).size, 12); assert.equal(new Set(FLAGS).size, 128);
assert.ok(FLAGS.every(flags => (flags & ~0xcd7) === 0 && (flags & 2) !== 0));
const specs = [], scans = [];
const programs = Object.fromEntries(['left', 'right'].map(kind => {
  const bytes = Buffer.alloc(130, 0xcc); let offset = 0;
  for (let destination = 0; destination < 8; destination++) {
    const entry = offset, prefix = destination < 3 ? [PRODUCERS[destination].opcode] : [], operand = 0xc0 | (kind === 'left' ? 0x10 : 0x18) | destination;
    const length = prefix.length + 12;
    bytes.set([...prefix, 0xd1, operand, 0x0f, 0x92, 0xc0, 0x0f, 0x90, 0xc2, 0xd1, operand, 0xeb, 128 - offset - length], offset);
    if (kind === 'left') {specs.push([PC + entry, length]); scans.push({entry: PC + entry, rotate: PC + entry + prefix.length, destination});}
    offset += length;
  }
  assert.equal(offset, 99); bytes.set([0x0f, 0x0b], 128); assert.deepEqual(readFileSync(join(output, `${kind}.x86`)), bytes); return [kind, bytes];
}));
const counts = {contexts: 0, modules: 0, seeds: 0, rcl: 0, rcr: 0, first_rotates: 0, second_rotates: 0, setb: 0, seto: 0, jumps: 0,
  clc: 0, stc: 0, cmc: 0, generated_runs: 0, preflight_controls: 0, owner_controls: 0, pages: 0, arena_checks: 0};
const modules = [], observedCl = new Set();
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
function check(ctx, label) {assert.deepEqual(arena(ctx), ctx.expected, `${ctx.owner}/${ctx.kind}/${label}: complete arena`); counts.arena_checks++;}
function pure(ctx, action, label, status = 0) {check(ctx, `before ${label}`); assert.equal(action(), status, label); check(ctx, label);}
function request(ctx, bytes) {refresh(ctx).bytes.set(bytes, ctx.base + TRANSFER); ctx.expected.set(bytes, TRANSFER); check(ctx, 'host request');}
function fresh(owner, kind) {
  const expected = Buffer.alloc(SIZE), ordinal = ++counts.contexts, codePage = Buffer.alloc(4096);
  expected.set(state(Array(8).fill(0), 0, 2)); expected.set(exit(1, 0), 56); expected.set(helper(0), 100); codePage.set(programs[kind]);
  const ctx = {owner, kind, expected, codePage, low: ordinal, high: 0xc8402000};
  if (owner === 'standalone') {
    ctx.memory = new WebAssembly.Memory({initial: 1}); ctx.base = 128; ctx.dataAddress = 0x8000; ctx.codeAddress = 0x6000;
    refresh(ctx).bytes.set(expected, ctx.base); ctx.bytes.set(pattern, ctx.dataAddress); ctx.bytes.set(codePage, ctx.codeAddress);
  } else {
    const instance = new WebAssembly.Instance(engineModule, {});
    const arities = {open: 3, close: 0, arena_ptr: 0, map: 3, upload: 2, read32: 1, write8: 2, compile_entries: 2,
      compile_resident: 1, generation: 0, module_ptr: 0, module_len: 0, guard: 6, guard_resident: 7};
    ctx.api = Object.fromEntries(Object.entries(arities).map(([name, arity]) => {
      const fn = instance.exports[`ring3_abi_v1_${name}`]; assert.equal(typeof fn, 'function', name); assert.equal(fn.length, arity, name); return [name, fn];
    }));
    ctx.memory = instance.exports.memory; assert.equal(ctx.api.open(3, ctx.low, ctx.high), 0);
    ctx.base = ctx.api.arena_ptr() >>> 0; ctx.dataAddress = 0x3000; ctx.codeAddress = PC; check(ctx, 'independently initialized arena');
    pure(ctx, () => ctx.api.map(ctx.dataAddress, 1, 3), 'data map'); request(ctx, pattern);
    pure(ctx, () => ctx.api.upload(ctx.dataAddress, pattern.length), 'pattern upload');
  }
  ctx.expected.set(pattern, TRANSFER); refresh(ctx).bytes.set(pattern, ctx.base + TRANSFER); check(ctx, 'patterned transfer'); return ctx;
}
function compile(ctx) {
  let bytes, binding = {};
  if (ctx.owner === 'standalone') bytes = readFileSync(join(output, `standalone-${ctx.kind}.wasm`));
  else {
    pure(ctx, () => ctx.api.map(PC, 1, 7), 'code map'); request(ctx, programs[ctx.kind]);
    pure(ctx, () => ctx.api.upload(PC, programs[ctx.kind].length), 'code upload');
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
    writeFileSync(join(output, `${ctx.owner}-${ctx.kind}.wasm`), bytes, {flag: 'wx'});
  }
  const module = new WebAssembly.Module(bytes), guard = ctx.owner === 'replacement' ? 'guard' : 'guard_resident';
  assert.deepEqual(WebAssembly.Module.imports(module), [{module: 'env', name: 'memory', kind: 'memory'},
    ...(ctx.owner === 'standalone' ? [] : [{module: 'ring3', name: guard, kind: 'function'}])]);
  assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  const child = new WebAssembly.Instance(module, {env: {memory: ctx.memory}, ...(ctx.owner === 'standalone' ? {} : {ring3: ctx.api})});
  assert.equal(child.exports.run.length, 4); counts.modules++;
  modules.push({owner: ctx.owner, kind: ctx.kind, sha256: hash(bytes), length: bytes.length, ...binding}); return {...binding, run: child.exports.run};
}
function seed(ctx, registers, pc, flags) {
  ctx.expected.set(state(registers, pc, flags)); ctx.expected.set(exit(3, 0), 56); ctx.expected.writeUInt32LE(0, 96);
  refresh(ctx).bytes.set(ctx.expected.subarray(0, 100), ctx.base); check(ctx, 'initial CPU'); counts.seeds++;
}
function cancel(ctx, value) {ctx.expected.writeUInt32LE(value, 96); refresh(ctx).view.setUint32(ctx.base + 96, value, true);}
function run(ctx, unit, budget, label, registers, pc, flags, reason = 1, retired = 1, status = 0, malformed = false) {
  check(ctx, `before ${label}`);
  if (status === 0) {ctx.expected.set(state(registers, pc, flags)); ctx.expected.set(exit(reason, retired), 56);}
  const pointers = malformed ? [0xffffffff, 0xffffffff, 0xffffffff] : [ctx.base, ctx.base + 56, ctx.base + 96];
  assert.equal(unit.run(pointers[0], pointers[1], budget, pointers[2]), status, label); check(ctx, label); counts.generated_runs++;
}
function pages(ctx) {
  for (const [address, expected] of [[ctx.dataAddress, pattern], [ctx.codeAddress, ctx.codePage]]) {
    const actual = Buffer.alloc(4096);
    if (ctx.owner === 'standalone') actual.set(refresh(ctx).bytes.subarray(address, address + 4096));
    else for (let offset = 0; offset < 4096; offset += 4) {
      check(ctx, 'before diagnostic read'); assert.equal(ctx.api.read32(address + offset), 0); refresh(ctx);
      actual.writeUInt32LE(ctx.view.getUint32(ctx.base + 120, true), offset);
      ctx.expected.set(helper(expected.readUInt32LE(offset)), 100); check(ctx, 'diagnostic read changes helper only');
    }
    assert.deepEqual(actual, expected, 'complete declared page unchanged'); counts.pages++;
  }
}
function continuation(ctx, unit, registers, scan, beforeFlags) {
  const {rotate: pc, destination} = scan, first = rotate(ctx.kind, registers[destination], beforeFlags % 2), flags = rotateFlags(beforeFlags, first);
  registers[destination] = first.value;
  run(ctx, unit, 1, 'first carry rotate full state before AL/DL consumers', registers, pc + 2, flags); counts.first_rotates++; counts[ctx.kind === 'left' ? 'rcl' : 'rcr']++;
  registers[0] = registers[0] - registers[0] % 256 + first.cf;
  run(ctx, unit, 1, 'SETB observes guest-produced CF', registers, pc + 5, flags); counts.setb++;
  registers[2] = registers[2] - registers[2] % 256 + first.of;
  run(ctx, unit, 1, 'SETO observes guest-produced OF', registers, pc + 8, flags); counts.seto++;
  const second = rotate(ctx.kind, registers[destination], first.cf), secondFlags = rotateFlags(flags, second);
  registers[destination] = second.value;
  run(ctx, unit, 1, 'second carry rotate consumes actual outgoing CF and current destination', registers, pc + 10, secondFlags); counts.second_rotates++; counts[ctx.kind === 'left' ? 'rcl' : 'rcr']++;
  run(ctx, unit, 1, 'following jump', registers, COLD, secondFlags); counts.jumps++;
}
function numeric(ctx, unit) {
  for (const [inputIndex, value] of INPUTS.entries()) for (const scan of scans) for (const [flagIndex, flags] of FLAGS.entries()) {
    const registers = [...REG]; registers[1] = ECX[(inputIndex + flagIndex) % ECX.length]; registers[scan.destination] = value;
    if (scan.destination !== 1) observedCl.add(registers[1] % 256);
    seed(ctx, registers, scan.rotate, flags);
    if (inputIndex === 0 && scan.destination === 0 && flagIndex === 0) {
      run(ctx, unit, 0, 'zero budget before carry rotate', registers, scan.rotate, flags, 1, 0); counts.preflight_controls++;
      cancel(ctx, 1); run(ctx, unit, 1, 'cancel before carry rotate', registers, scan.rotate, flags, 2, 0); counts.preflight_controls++; cancel(ctx, 0);
    }
    continuation(ctx, unit, registers, scan, flags);
  }
  for (const producer of PRODUCERS) for (const beforeFlags of [2, 0xcd7]) {
    const scan = scans[producer.destination], registers = [...REG]; registers[1] = ECX[4]; registers[producer.destination] = producer.value;
    seed(ctx, registers, scan.entry, beforeFlags);
    if (producer.name === 'CLC' && beforeFlags === 2) {
      run(ctx, unit, 0, 'zero budget before known-entry CF producer', registers, scan.entry, beforeFlags, 1, 0); counts.preflight_controls++;
      cancel(ctx, 1); run(ctx, unit, 1, 'cancel before known-entry CF producer', registers, scan.entry, beforeFlags, 2, 0); counts.preflight_controls++; cancel(ctx, 0);
    }
    const incoming = producer.name === 'CLC' ? 0 : producer.name === 'STC' ? 1 : 1 - beforeFlags % 2;
    const producedFlags = beforeFlags - beforeFlags % 2 + incoming;
    run(ctx, unit, 1, `${producer.name} produces live CF and preserves full remaining CPU`, registers, scan.rotate, producedFlags); counts[producer.name.toLowerCase()]++;
    continuation(ctx, unit, registers, scan, producedFlags);
  }
}
function finish(ctx, unit) {
  const registers = Array.from({length: 8}, (_, index) => ctx.expected.readUInt32LE(16 + index * 4)), flags = ctx.expected.readUInt32LE(52);
  for (const [name, cancelled, budget, reason] of [['zero budget', 0, 0, 1], ['cancel with budget', 1, 1, 2], ['cancel with zero budget', 1, 0, 2]]) {
    cancel(ctx, cancelled); run(ctx, unit, budget, name, registers, COLD, flags, reason, 0); counts.preflight_controls++;
  }
  cancel(ctx, 0); run(ctx, unit, 1, 'cold entry', registers, COLD, flags, 3, 0); counts.preflight_controls++;
  run(ctx, unit, 0, 'malformed pointers', registers, COLD, flags, 0, 0, 1, true); counts.preflight_controls++; pages(ctx);
  if (ctx.owner === 'standalone') return;
  for (const wrong of ['key', 'identity']) {
    const low = wrong === 'key' ? ctx.low ^ 1 : ctx.low;
    const action = ctx.owner === 'replacement'
      ? () => ctx.api.guard(low, ctx.high, unit.generation + (wrong === 'identity' ? 1 : 0), ctx.base, ctx.base + 56, ctx.base + 96)
      : () => ctx.api.guard_resident(low, ctx.high, unit.low ^ (wrong === 'identity' ? 1 : 0), unit.high, ctx.base, ctx.base + 56, ctx.base + 96);
    pure(ctx, action, `wrong ${wrong}`, 3); counts.owner_controls++;
  }
  check(ctx, 'before same-byte invalidation'); assert.equal(ctx.api.write8(PC, programs[ctx.kind][0]), 0);
  const updatedHelper = record('R3MH', 40, [0, 0, 0, 0, 0, 1]); updatedHelper.writeUInt16LE(3, 4);
  ctx.expected.set(updatedHelper, 100); check(ctx, 'same-byte invalidation changes helper only');
  run(ctx, unit, 0, 'stale before pointer validation', registers, COLD, flags, 0, 0, 4, true); counts.owner_controls++;
  pure(ctx, () => ctx.api.close(), 'close');
  run(ctx, unit, 0, 'closed before pointer validation', registers, COLD, flags, 0, 0, 5, true); counts.owner_controls++;
}
for (const owner of ['standalone', 'replacement', 'resident']) for (const kind of ['left', 'right']) {
  const ctx = fresh(owner, kind), unit = compile(ctx); pages(ctx); numeric(ctx, unit); finish(ctx, unit);
}
assert.equal(counts.contexts, 6); assert.equal(counts.modules, 6); assert.equal(counts.seeds, 73764);
assert.equal(counts.rcl, 73764); assert.equal(counts.rcr, 73764); assert.equal(counts.first_rotates, 73764); assert.equal(counts.second_rotates, 73764);
assert.equal(counts.clc, 12); assert.equal(counts.stc, 12); assert.equal(counts.cmc, 12);
assert.equal(counts.setb, 73764); assert.equal(counts.seto, 73764); assert.equal(counts.jumps, 73764);
assert.equal(counts.generated_runs, 368918); assert.equal(counts.preflight_controls, 54); assert.equal(counts.owner_controls, 16); assert.equal(counts.pages, 24);
assert.deepEqual([...observedCl].sort((a, b) => a - b), [0, 1, 31, 32, 255]);
const result = {status: 'ok', engine_sha256: hash(engineBytes), tools: {node: process.version, v8: process.versions.v8}, counts, modules,
  source: {url: 'https://cdrdv2-public.intel.com/929354/253667-093-sdm-vol-2b.pdf', order: '253667-093US', edition: 'September 2026',
    pdf_sha256: 'a261998ace8e07f624bf3e2486bb6cfe950fb8be8d0dc9631bcecb4c5ec953e4',
    extract_sha256: '7a0e03302580204f99e8283af389f84ec082701807f1a9615a028af4d6f849d0', pages: [541, 542, 543, 544, 545]},
  oracle: {sources: INPUTS, flag_seeds: FLAGS, literal_anchors: ANCHORS, live_cf_producers: PRODUCERS, registers: REG, ecx_seeds: ECX, observed_non_destination_cl: [...observedCl].sort((a, b) => a - b),
    specs, scans, instructions: 43, cold_target: COLD, flag_policy: 'CF from old destination MSB for RCL or LSB for RCR; OF RCL result MSB xor outgoing CF, RCR old MSB xor incoming CF; SF, ZF, AF, PF, DF and fixed bit 1 preserved',
    programs: Object.fromEntries(Object.entries(programs).map(([name, bytes]) => [name, {hex: bytes.toString('hex'), sha256: hash(bytes)}])),
    arena_bytes: SIZE, data_page_sha256: hash(pattern)},
  test_sha256: Object.fromEntries(['engine/tests/cpu_carry_rotate_one_wasm.rs', 'engine/tests/fixtures/p2-carry-rotate-one/run.mjs'].map(path => [path, hash(readFileSync(join(root, path)))])),
  claim: 'flat32 count-one register RCL/RCR in native-generated standalone and actual engine-Wasm-generated replacement/resident modules; declared 12 source values, all 128 valid FLAGS including both incoming CF values, all eight destinations and literal anchors for every source/carry pair; independent 33-bit binary-string rotation, fixed count 1 despite observed CL 0/1/31/32/255, full CPU before SETB AL/SETO DL, second carry rotate uses the outgoing guest CF and current destination including AL/DL overlap; real CLC/STC/CMC producers each start with CF 0 and 1 and continue through both rotations without intermediate CPU writes; full 4236-byte arena including untouched helper during guest runs, complete declared code and patterned data pages, exact pure imports and focused preflight/owner guards; no exhaustive operands, memory carry rotate, other counts, wider ISA, ABI, PE/provider, SDK/browser, performance or game claim'};
const resultBytes = Buffer.from(JSON.stringify(result, null, 2));
writeFileSync(join(output, 'result.json'), resultBytes, {flag: 'wx'});
console.log(JSON.stringify({status: result.status, engine_sha256: result.engine_sha256, result_sha256: hash(resultBytes), counts, output}));
