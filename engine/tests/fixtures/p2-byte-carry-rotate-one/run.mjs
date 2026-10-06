import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(root, 'expected actual engine Wasm, output directory and repository root');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const engineBytes = readFileSync(enginePath), engineModule = new WebAssembly.Module(engineBytes);
assert.deepEqual(WebAssembly.Module.imports(engineModule), []);
const SIZE = 4236, TRANSFER = 140, PC = 0x1000, COLD = 0x10c0;
const REG = [0x1234807f, 0x23457f80, 0x34560f10, 0x4567ff00, 0x56789abc, 0x6789abcd, 0x789abcde, 0x89abcdef];
const ALIASES = [['AL', 0, 1], ['CL', 1, 1], ['DL', 2, 1], ['BL', 3, 1], ['AH', 0, 256], ['CH', 1, 256], ['DH', 2, 256], ['BH', 3, 256]];
const INPUTS = Array.from({length: 256}, (_, value) => value), BOUNDARIES = [0, 1, 0x7f, 0x80, 0x81, 0xff], BASIS = [2, 0xcd7];
const ECX = [0xa1b2c300, 0xa1b2c301, 0xa1b2c31f, 0xa1b2c320, 0xa1b2c3ff], FLAG_BITS = [1, 4, 0x10, 0x40, 0x80, 0x400, 0x800];
const FLAGS = Array.from({length: 128}, (_, seed) => 2 + FLAG_BITS.reduce((flags, bit, index) => flags + (seed & 2 ** index ? bit : 0), 0));
const EXTRA_FLAGS = FLAGS.filter(flags => !BASIS.includes(flags));
const ANCHORS = [[0, 0, 0, 0, 0, 0, 0, 0], [0, 1, 1, 0, 0, 0x80, 0, 1], [1, 0, 2, 0, 0, 0, 1, 0],
  [1, 1, 3, 0, 0, 0x80, 1, 1], [0x7f, 0, 0xfe, 0, 1, 0x3f, 1, 0], [0x7f, 1, 0xff, 0, 1, 0xbf, 1, 1],
  [0x80, 0, 0, 1, 1, 0x40, 0, 1], [0x80, 1, 1, 1, 1, 0xc0, 0, 0], [0x81, 0, 2, 1, 1, 0x40, 1, 1],
  [0x81, 1, 3, 1, 1, 0xc0, 1, 0], [0xff, 0, 0xfe, 1, 0, 0x7f, 1, 1], [0xff, 1, 0xff, 1, 0, 0xff, 1, 0]];
function rotate(kind, value, incoming) {
  const old = String(incoming) + value.toString(2).padStart(8, '0'); assert.equal(old.length, 9); assert.ok(incoming === 0 || incoming === 1);
  const rotated = kind === 'left' ? old.slice(1) + old[0] : old[8] + old.slice(0, 8), cf = Number(rotated[0]);
  return {value: parseInt(rotated.slice(1), 2), cf, of: Number(kind === 'left' ? rotated[1] !== rotated[0] : old[1] !== old[0])};
}
const rotateFlags = (before, result) => [2, 4, 0x10, 0x40, 0x80, 0x400].reduce((flags, bit) => flags + (before & bit ? bit : 0), 0) + result.cf + result.of * 0x800;
function getByte(registers, alias) {const [, parent, factor] = ALIASES[alias]; return Math.floor(registers[parent] / factor) % 256;}
function putByte(registers, alias, value) {const [, parent, factor] = ALIASES[alias]; registers[parent] += (value - getByte(registers, alias)) * factor;}
const PRODUCERS = [{name: 'clc', alias: 0, opcode: 0xf8, byte: 0x81, flags: [2, 0xcd6]},
  {name: 'stc', alias: 1, opcode: 0xf9, byte: 0x80, flags: [3, 0xcd7]}, {name: 'cmc', alias: 2, opcode: 0xf5, byte: 0x81, flags: [3, 0xcd6]}];
for (const [before, incoming, left, leftCf, leftOf, right, rightCf, rightOf] of ANCHORS) {
  assert.deepEqual(rotate('left', before, incoming), {value: left, cf: leftCf, of: leftOf});
  assert.deepEqual(rotate('right', before, incoming), {value: right, cf: rightCf, of: rightOf});
}
assert.equal(rotateFlags(0xcd7, rotate('left', 0, 1)), 0x4d6); assert.equal(rotateFlags(0xcd7, rotate('right', 0xff, 1)), 0x4d7);
assert.equal(rotateFlags(2, rotate('left', 0x80, 0)), 0x803); assert.equal(rotateFlags(2, rotate('right', 0x80, 0)), 0x802);
const insertionCheck = [...REG]; putByte(insertionCheck, 4, 0x81); assert.equal(insertionCheck[0], 0x1234817f); putByte(insertionCheck, 0, 0x42); assert.equal(insertionCheck[0], 0x12348142);
assert.equal(new Set(FLAGS).size, 128); assert.equal(EXTRA_FLAGS.length, 126); assert.ok(FLAGS.every(flags => (flags & ~0xcd7) === 0 && (flags & 2) !== 0));
const specs = [], scans = [];
const programs = Object.fromEntries(['left', 'right'].map(kind => {
  const bytes = Buffer.alloc(194, 0xcc); let offset = 0;
  for (let alias = 0; alias < 8; alias++) {
    const entry = offset; if (alias < PRODUCERS.length) bytes[offset++] = PRODUCERS[alias].opcode;
    const first = offset, operand = 0xc0 | (kind === 'right' ? 0x18 : 0x10) | alias;
    for (let step = 0; step < 2; step++) {bytes.set([0xd0, operand, 0x0f, 0x92, 0xc0, 0x0f, 0x90, 0xc2], offset); offset += 8;}
    if (alias === 7) {bytes.set([0xbf, 0xef, 0xbe, 0xad, 0xde], offset); offset += 5;}
    bytes[offset] = 0xe9; bytes.writeInt32LE(192 - offset - 5, offset + 1); offset += 5;
    if (kind === 'left') {specs.push([PC + entry, offset - entry]); scans.push({entry: PC + entry, first: PC + first, alias, canary: alias === 7});}
  }
  assert.equal(offset, 176); bytes.set([0x0f, 0x0b], 192); assert.deepEqual(readFileSync(join(output, `${kind}.x86`)), bytes); return [kind, bytes];
}));
assert.deepEqual(specs.map(([entry]) => entry - PC), [0, 22, 44, 66, 87, 108, 129, 150]);
const counts = {contexts: 0, modules: 0, seeds: 0, byte_matrix: 0, extra_flag_rows: 0, skipped_overlap: 0, first_rotates: 0, second_rotates: 0,
  rcl: 0, rcr: 0, setb: 0, seto: 0, jumps: 0, canaries: 0, live_producers: 0, clc: 0, stc: 0, cmc: 0, generated_calls: 0, preflight: 0, owner_controls: 0,
  maps: 0, host_uploads: 0, host_uploaded_bytes: 0, standalone_page_inputs: 0, pages: 0, arena_checks: 0};
const modules = [], pageRows = [], invalidations = [], observedCl = new Set();
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
function check(ctx, label) {refresh(ctx); assert.deepEqual(Buffer.from(ctx.bytes.subarray(ctx.base, ctx.base + SIZE)), ctx.expected, `${ctx.owner}/${ctx.kind}/${label}: complete arena`); counts.arena_checks++;}
function pure(ctx, action, label, status = 0) {check(ctx, `before ${label}`); assert.equal(action(), status, label); check(ctx, label);}
function request(ctx, bytes) {refresh(ctx).bytes.set(bytes, ctx.base + TRANSFER); ctx.expected.set(bytes, TRANSFER); check(ctx, 'explicit host request');}
function upload(ctx, address, bytes) {request(ctx, bytes); pure(ctx, () => ctx.api.upload(address, bytes.length), 'explicit host upload'); counts.host_uploads++; counts.host_uploaded_bytes += bytes.length;}
function fresh(owner, kind) {
  const expected = Buffer.alloc(SIZE), ordinal = ++counts.contexts, codePage = Buffer.alloc(4096); codePage.set(programs[kind]);
  expected.set(state(Array(8).fill(0), 0, 2)); expected.set(exit(1, 0), 56); expected.set(helper(0), 100);
  const ctx = {owner, kind, expected, codePage, low: ordinal, high: 0xc8410000};
  if (owner === 'standalone') {
    ctx.memory = new WebAssembly.Memory({initial: 1}); ctx.base = 128; ctx.dataAddress = 0x8000; ctx.codeAddress = 0x6000;
    refresh(ctx).bytes.set(expected, ctx.base); ctx.bytes.set(pattern, ctx.dataAddress); ctx.bytes.set(codePage, ctx.codeAddress); counts.standalone_page_inputs += 2;
  } else {
    const instance = new WebAssembly.Instance(engineModule, {}), arities = {open: 3, close: 0, arena_ptr: 0, map: 3, upload: 2, read32: 1,
      compile_entries: 2, compile_resident: 1, generation: 0, module_ptr: 0, module_len: 0, guard: 6, guard_resident: 7};
    ctx.api = Object.fromEntries(Object.entries(arities).map(([name, arity]) => {
      const fn = instance.exports[`ring3_abi_v1_${name}`]; assert.equal(typeof fn, 'function', name); assert.equal(fn.length, arity, name); return [name, fn];
    }));
    ctx.memory = instance.exports.memory; assert.equal(ctx.api.open(3, ctx.low, ctx.high), 0);
    ctx.base = ctx.api.arena_ptr() >>> 0; ctx.dataAddress = 0x3000; ctx.codeAddress = PC; check(ctx, 'independently initialized arena');
    for (const [address, permissions, bytes] of [[ctx.dataAddress, 3, pattern], [PC, 7, programs[kind]]]) {
      pure(ctx, () => ctx.api.map(address, 1, permissions), 'page map'); counts.maps++; upload(ctx, address, bytes);
    }
  }
  ctx.expected.set(pattern, TRANSFER); refresh(ctx).bytes.set(pattern, ctx.base + TRANSFER); check(ctx, 'explicit patterned transfer'); return ctx;
}
function compile(ctx) {
  let bytes, binding = {};
  if (ctx.owner === 'standalone') bytes = readFileSync(join(output, `standalone-${ctx.kind}.wasm`));
  else {
    request(ctx, words(ctx.owner === 'replacement' ? specs.map(([entry]) => entry) : specs.flat()));
    if (ctx.owner === 'replacement') {
      pure(ctx, () => ctx.api.compile_entries(specs.length, 0), 'eight-entry compiler');
      binding = {generation: ctx.api.generation(), pointer: ctx.api.module_ptr() >>> 0, length: ctx.api.module_len() >>> 0}; assert.ok(binding.generation > 0);
    } else {
      check(ctx, 'before eight-extent compiler'); assert.equal(ctx.api.compile_resident(specs.length), 0); refresh(ctx);
      assert.equal(ctx.view.getUint32(ctx.base + TRANSFER, true), 1); assert.equal(ctx.view.getUint32(ctx.base + TRANSFER + 4, true), 24);
      binding = Object.fromEntries(['low', 'high', 'pointer', 'length'].map((name, index) => [name, ctx.view.getUint32(ctx.base + TRANSFER + 8 + index * 4, true)]));
      assert.ok(binding.low !== 0 || binding.high !== 0); ctx.expected.set(words([1, 24, binding.low, binding.high, binding.pointer, binding.length]), TRANSFER); check(ctx, 'resident metadata only');
    }
    refresh(ctx); assert.ok(binding.pointer > 0 && binding.length > 8 && binding.length <= 65536 && binding.pointer + binding.length <= ctx.bytes.length);
    bytes = Buffer.from(ctx.bytes.subarray(binding.pointer, binding.pointer + binding.length)); writeFileSync(join(output, `${ctx.owner}-${ctx.kind}.wasm`), bytes, {flag: 'wx'});
  }
  assert.ok(bytes.length <= 65536); const module = new WebAssembly.Module(bytes), imports = ctx.owner === 'standalone' ? [] : [ctx.owner === 'replacement' ? 'guard' : 'guard_resident'];
  assert.deepEqual(WebAssembly.Module.imports(module), [{module: 'env', name: 'memory', kind: 'memory'}, ...imports.map(name => ({module: 'ring3', name, kind: 'function'}))]);
  assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  const child = new WebAssembly.Instance(module, {env: {memory: ctx.memory}, ...(ctx.api ? {ring3: ctx.api} : {})}); assert.equal(child.exports.run.length, 4); counts.modules++;
  modules.push({owner: ctx.owner, kind: ctx.kind, sha256: hash(bytes), length: bytes.length, imports, ...binding}); return {...binding, run: child.exports.run};
}
function seed(ctx, registers, pc, flags) {
  ctx.expected.set(state(registers, pc, flags)); ctx.expected.set(exit(3, 0), 56); ctx.expected.writeUInt32LE(0, 96); ctx.expected.set(helper(0xdecafbad), 100);
  refresh(ctx).bytes.set(ctx.expected.subarray(0, 140), ctx.base); check(ctx, 'one initial CPU/helper seed'); counts.seeds++;
}
function cancel(ctx, value) {ctx.expected.writeUInt32LE(value, 96); refresh(ctx).view.setUint32(ctx.base + 96, value, true);}
function run(ctx, unit, budget, label, registers, pc, flags, reason = 1, retired = 1, status = 0, malformed = false) {
  check(ctx, `before ${label}`); if (status === 0) {ctx.expected.set(state(registers, pc, flags)); ctx.expected.set(exit(reason, retired), 56);}
  const pointers = malformed ? [0xffffffff, 0xffffffff, 0xffffffff] : [ctx.base, ctx.base + 56, ctx.base + 96];
  assert.equal(unit.run(pointers[0], pointers[1], budget, pointers[2]), status, label); check(ctx, label); counts.generated_calls++;
}
function preflight(ctx, unit, registers, pc, flags) {
  run(ctx, unit, 0, 'zero budget at known entry', registers, pc, flags, 1, 0); counts.preflight++;
  cancel(ctx, 1); run(ctx, unit, 1, 'cancel before guest instruction', registers, pc, flags, 2, 0); counts.preflight++; cancel(ctx, 0);
}
function pages(ctx, label) {
  for (const [address, expected] of [[ctx.dataAddress, pattern], [ctx.codeAddress, ctx.codePage]]) {
    const actual = Buffer.alloc(4096);
    if (ctx.owner === 'standalone') actual.set(refresh(ctx).bytes.subarray(address, address + 4096));
    else for (let offset = 0; offset < 4096; offset += 4) {
      check(ctx, 'before separate diagnostic Read32'); assert.equal(ctx.api.read32(address + offset), 0); refresh(ctx);
      actual.writeUInt32LE(ctx.view.getUint32(ctx.base + 120, true), offset); ctx.expected.set(helper(expected.readUInt32LE(offset)), 100); check(ctx, 'diagnostic Read32 changes helper v1 only');
    }
    assert.deepEqual(actual, expected, 'complete declared page unchanged'); counts.pages++; pageRows.push({owner: ctx.owner, kind: ctx.kind, label, address, sha256: hash(actual)});
  }
}
function chain(ctx, unit, registers, scan, flags) {
  for (let step = 0; step < 2; step++) {
    const result = rotate(ctx.kind, getByte(registers, scan.alias), flags % 2); putByte(registers, scan.alias, result.value); flags = rotateFlags(flags, result);
    const next = scan.first + step * 8 + 2;
    run(ctx, unit, 1, 'complete carry-rotated parent and CPU before partial consumers', registers, next, flags); counts[ctx.kind === 'left' ? 'rcl' : 'rcr']++; counts[step === 0 ? 'first_rotates' : 'second_rotates']++;
    putByte(registers, 0, result.cf); run(ctx, unit, 1, 'SETB observes current guest CF', registers, next + 3, flags); counts.setb++;
    putByte(registers, 2, result.of); run(ctx, unit, 1, 'SETO observes current guest OF', registers, next + 6, flags); counts.seto++;
  }
  if (scan.canary) {registers[7] = 0xdeadbeef; run(ctx, unit, 1, 'following MOV EDI canary', registers, scan.first + 21, flags); counts.canaries++;}
  run(ctx, unit, 1, 'following near JMP', registers, COLD, flags); counts.jumps++;
}
function numeric(ctx, unit) {
  for (const [values, flagRows, countName] of [[INPUTS, BASIS, 'byte_matrix'], [BOUNDARIES, EXTRA_FLAGS, 'extra_flag_rows']])
    for (const value of values) for (const scan of scans) for (const flags of flagRows) {
      const registers = [...REG]; registers[1] = ECX[(value + scan.alias + FLAGS.indexOf(flags)) % ECX.length]; putByte(registers, scan.alias, value);
      if (scan.alias !== 1) observedCl.add(registers[1] % 256); seed(ctx, registers, scan.first, flags);
      if (countName === 'byte_matrix' && value === 0 && scan.alias === 0 && flags === 2) preflight(ctx, unit, registers, scan.first, flags);
      chain(ctx, unit, registers, scan, flags); counts[countName]++;
    }
  counts.skipped_overlap += BOUNDARIES.length * BASIS.length * scans.length;
  for (const producer of PRODUCERS) for (const [index, beforeFlags] of BASIS.entries()) {
    const registers = [...REG], scan = scans[producer.alias]; registers[1] = ECX[4]; putByte(registers, scan.alias, producer.byte);
    seed(ctx, registers, scan.entry, beforeFlags);
    if (producer.name === 'clc' && beforeFlags === 0xcd7) preflight(ctx, unit, registers, scan.entry, beforeFlags);
    const producedFlags = producer.flags[index];
    run(ctx, unit, 1, `complete ${producer.name.toUpperCase()} producer before byte carry rotate`, registers, scan.first, producedFlags);
    counts.live_producers++; counts[producer.name]++; chain(ctx, unit, registers, scan, producedFlags);
  }
}
function finish(ctx, unit) {
  const registers = Array.from({length: 8}, (_, index) => ctx.expected.readUInt32LE(16 + index * 4)), flags = ctx.expected.readUInt32LE(52);
  for (const [name, cancelled, budget, reason] of [['zero budget', 0, 0, 1], ['cancel with budget', 1, 1, 2], ['cancel with zero budget', 1, 0, 2]]) {
    cancel(ctx, cancelled); run(ctx, unit, budget, name, registers, COLD, flags, reason, 0); counts.preflight++;
  }
  cancel(ctx, 0); run(ctx, unit, 1, 'cold entry', registers, COLD, flags, 3, 0); counts.preflight++;
  run(ctx, unit, 0, 'malformed pointers', registers, COLD, flags, 0, 0, 1, true); counts.preflight++; pages(ctx, 'complete pages after live guest chains');
  if (ctx.owner === 'standalone') return;
  for (const wrong of ['key', 'identity']) {
    const low = ctx.low ^ Number(wrong === 'key');
    const action = ctx.owner === 'replacement' ? () => ctx.api.guard(low, ctx.high, unit.generation + Number(wrong === 'identity'), ctx.base, ctx.base + 56, ctx.base + 96)
      : () => ctx.api.guard_resident(low, ctx.high, unit.low ^ Number(wrong === 'identity'), unit.high, ctx.base, ctx.base + 56, ctx.base + 96);
    pure(ctx, action, `wrong ${wrong}`, 3); counts.owner_controls++;
  }
  const sameByte = ctx.codePage[0]; upload(ctx, PC, Buffer.from([sameByte])); cancel(ctx, 1);
  run(ctx, unit, 0, 'same-byte stale owner before cancelled malformed pointers', registers, COLD, flags, 0, 0, 4, true); counts.owner_controls++;
  invalidations.push({owner: ctx.owner, kind: ctx.kind, address: PC, old_byte: sameByte, new_byte: sameByte, upload_bytes: 1, stale_status: 4}); pages(ctx, 'current complete pages after same-byte invalidation');
  pure(ctx, () => ctx.api.close(), 'close'); run(ctx, unit, 0, 'closed owner before cancelled malformed pointers', registers, COLD, flags, 0, 0, 5, true); counts.owner_controls++;
}
for (const owner of ['standalone', 'replacement', 'resident']) for (const kind of ['left', 'right']) {
  const ctx = fresh(owner, kind), unit = compile(ctx); pages(ctx, 'initial complete code and patterned data'); numeric(ctx, unit); finish(ctx, unit);
}
const expectedCounts = {contexts: 6, modules: 6, seeds: 60900, byte_matrix: 24576, extra_flag_rows: 36288, skipped_overlap: 576,
  first_rotates: 60900, second_rotates: 60900, rcl: 60900, rcr: 60900, setb: 121800, seto: 121800, jumps: 60900, canaries: 7608,
  live_producers: 36, clc: 12, stc: 12, cmc: 12, generated_calls: 434006, preflight: 54, owner_controls: 16,
  maps: 8, host_uploads: 12, host_uploaded_bytes: 17164, standalone_page_inputs: 4, pages: 32, arena_checks: 978162};
for (const [name, expected] of Object.entries(expectedCounts)) assert.equal(counts[name], expected, `exact observed ${name} census`);
assert.deepEqual([...observedCl].sort((a, b) => a - b), [0, 1, 31, 32, 255]);
const result = {status: 'ok', engine_sha256: hash(engineBytes), tools: {node: process.version, v8: process.versions.v8}, counts, modules, pages: pageRows, code_invalidations: invalidations,
  source: {url: 'https://cdrdv2-public.intel.com/929354/253667-093-sdm-vol-2b.pdf', order: '253667-093US', edition: 'September 2026',
    pdf_sha256: 'a261998ace8e07f624bf3e2486bb6cfe950fb8be8d0dc9631bcecb4c5ec953e4', extract_sha256: '7a0e03302580204f99e8283af389f84ec082701807f1a9615a028af4d6f849d0', pages: [541, 542, 543, 544, 545]},
  oracle: {sources: INPUTS, basis_flags: BASIS, boundary_bytes: BOUNDARIES, extra_flags: EXTRA_FLAGS, all_boundary_flags: FLAGS, aliases: ALIASES, literal_anchors: ANCHORS,
    live_carry_producers: PRODUCERS, literal_direction_anchors: 24, registers: REG, ecx_seeds: ECX, observed_non_destination_cl: [...observedCl].sort((a, b) => a - b),
    specs, scans, cold_target: COLD, limits: {instructions: 64, blocks: 8, wasm_bytes: 65536}, bank_instructions: 60, body_bytes: 176, source_bytes: 194,
    overlap: {byte_flag_pairs_per_alias: 12, skipped_rows: 576, unique_numeric_rows: 60864, rule: 'boundary bytes reuse BASIS rows and add only the remaining 126 FLAGS'},
    flag_policy: 'only CF/OF change; retain SF/ZF/AF/PF/DF/fixed bit 1, including zero operands; first and second rotate capture old byte and incoming CF; RCR OF is OLD byte MSB xor OLD CF; second same-alias rotate reads current byte after AL/DL consumers and live first-rotate CF',
    programs: Object.fromEntries(Object.entries(programs).map(([name, bytes]) => [name, {hex: bytes.toString('hex'), sha256: hash(bytes)}])), arena_bytes: SIZE, pattern_sha256: hash(pattern)},
  artifact_census: {x86_banks: 2, modules: 6, result_files: 1, total: 9},
  test_sha256: Object.fromEntries(['engine/tests/cpu_byte_carry_rotate_one_wasm.rs', 'engine/tests/fixtures/p2-byte-carry-rotate-one/run.mjs'].map(path => [path, hash(readFileSync(join(root, path)))])),
  claim: 'flat32 register-only D0 count-one RCL/RCR8 through native-generated standalone and actual engine-Wasm-generated replacement/resident modules: all AL/CL/DL/BL/AH/CH/DH/BH aliases, all 256 byte sources with complementary FLAGS 2/cd7 plus six declared boundary bytes with all 128 legal FLAGS represented once; 576 corpus overlaps explicitly skipped, 60864 numeric initial rows and 36 literal live CLC/STC/CMC rows from both old CF states; independent nine-character oldCF+byte bitstring and 24 literal direction anchors, numerical byte extraction/insertion, complete parent and CPU observed before partial flag consumers; two real carry rotates without CPU repair, second AL/DL uses current SETB/SETO outputs and live outgoing CF, other parent bits and GPRs modeled exactly; zero operand retires with incoming CF affecting its result and retains unaffected FLAGS; literal one despite CL 0/1/31/32/255, guest SETB/SETO twice and near JMP plus BH EDI canary; exact pure imports, default caps and 4236-byte arena with unchanged helper during guest execution, complete current code/patterned data pages; focused known-entry budget/cancel before numeric AL and CLC, cold/malformed controls, wrong owner key/identity, same-byte code upload stale4 and closed5 before cancelled malformed arguments; no exhaustive full byte×FLAGS Cartesian product, memory carry rotation, C0/D2 or other counts, ABI, PE/browser/performance/game claim'};
const resultBytes = Buffer.from(JSON.stringify(result, null, 2)); writeFileSync(join(output, 'result.json'), resultBytes, {flag: 'wx'});
console.log(JSON.stringify({status: result.status, engine_sha256: result.engine_sha256, result_sha256: hash(resultBytes), counts, output}));
