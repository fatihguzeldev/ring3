import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {ARENA_SIZE, createArena} from '../../../../../support/wasm/arena.mjs';
import {readEngine} from '../../../../../support/wasm/engine.mjs';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root, 'run.mjs ENGINE OUTPUT ROOT');
const T = 140, FP = 4236, SV = 4364, ENTRY = 0x2000, CALLER = 0x3000;
const COEFF = 0x5010, INPUT = 0x6010, VECTOR = 0x7010, STACK = 0x9020;
const ESP = 0x9080, CHECKSUM = 0xa030, KEY_HIGH = 0x53494d43;
const fixture = 'engine/tests/programs/numeric/fixtures/p2-simd-compiled';
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const hex = value => Buffer.from(value, 'hex');
const words = values => {
  const bytes = Buffer.alloc(values.length * 4);
  values.forEach((value, index) => bytes.writeUInt32LE(value >>> 0, index * 4));
  return bytes;
};
const pattern = length => Buffer.from(Array.from({length}, (_, index) => (index * 17 + 23) & 255));
const record = (magic, version, size, values) => {
  const bytes = Buffer.alloc(size);
  bytes.write(magic); bytes.writeUInt16LE(version, 4); bytes.writeUInt16LE(1, 6);
  bytes.writeUInt32LE(size, 8); words(values).copy(bytes, 16);
  return bytes;
};
const state = (registers, pc, flags) => record('R3ST', 1, 56, [...registers, pc, flags]);
const exit = retired => record('R3EX', 6, 40, [1, retired, 0, 0, 0, 0]);
const packet = (version, value = 0, length = 0) => record('R3MH', version, 40, [0, value, 0, 0, 0, length]);
const engine = readEngine(enginePath), initial = readFileSync(join(output, 'initial-arena.bin'));
assert.deepEqual(initial, createArena(), 'native canonical complete4612 input');
const spec = JSON.parse(readFileSync(join(root, fixture, 'cases.json'), 'utf8'));
const text = readFileSync(join(root, fixture, 'kernel.text.bin'));
assert.equal(hash(text), 'b29856c68e7bc4609fb8a307512ce1d6ed823978a400945b76c502e353e8a1ad');
assert.equal(hash(readFileSync(join(root, fixture, 'kernel.c'))), spec.source_sha256);
assert.deepEqual(spec.cases.map(row => row.count), [0, 1, 17]);
const sourcePaths = ['kernel.c', 'kernel.obj', 'kernel.text.bin', 'kernel.disasm.txt', 'receipt.json', 'cases.json', 'runtime/run.mjs'].map(file => fixture + '/' + file);
sourcePaths.push('engine/tests/support/wasm/engine.mjs', 'engine/tests/support/wasm/arena.mjs',
  'engine/src/cpu/dbt/wasm/instructions/simd/direct.rs', 'engine/src/cpu/dbt/wasm/instructions/simd.rs', 'engine/src/process/memory/span.rs', 'engine/src/process/memory/mod.rs', 'engine/src/process/mod.rs');
const sourceHashes = () => Object.fromEntries(sourcePaths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourceBefore = sourceHashes(), contexts = [], modules = [], observations = [], helpers = [], cases = [], diagnostics = [];
const prefix = Buffer.concat([hex('5068'), words([COEFF]), hex('68'), words([INPUT]), hex('68'), words([VECTOR])]);
const RETURN = CALLER + prefix.length + 5;
const caller = [{pc: CALLER, bytes: Buffer.concat([prefix, hex('e8'), words([ENTRY - RETURN])]), instructions: 5},
  {pc: RETURN, bytes: Buffer.concat([hex('83c410a3'), words([CHECKSUM]), hex('ebfe')]), instructions: 3}];
assert.equal(RETURN, 0x3015);
const layout = [[0, 17, 6], [17, 20, 5], [37, 47, 12], [84, 33, 9]];
const arena = c => Buffer.from(new Uint8Array(c.memory.buffer, c.base, ARENA_SIZE));
function input(c, offset, bytes) {
  new Uint8Array(c.memory.buffer, c.base + offset, bytes.length).set(bytes);
}
function host(c, name, args, span) {
  const expected = arena(c);
  assert.equal(c.api[name](...args), 0, name);
  if (span) expected.set(arena(c).subarray(...span), span[0]);
  assert.deepEqual(arena(c), expected, name + ' declared arena mutation');
}
function upload(c, address, bytes) {
  input(c, T, bytes); host(c, 'upload', [address, bytes.length]);
}
function frame(bytes) { return Buffer.concat([pattern(16), bytes, pattern(16)]); }
function open(owner, entries) {
  const instance = new WebAssembly.Instance(engine.module, {});
  const c = {id: contexts.length + 1, owner, entries, memory: instance.exports.memory, api: {}};
  for (const [name, fn] of Object.entries(instance.exports)) {
    if (name.startsWith('ring3_abi_v1_') && typeof fn === 'function') c.api[name.slice(13)] = fn;
  }
  assert.equal(c.api.open(8, c.id, KEY_HIGH), 0); c.base = c.api.arena_ptr() >>> 0;
  assert.deepEqual(arena(c), initial, 'unseeded engine-created canonical complete4612');
  for (const [page, bits] of [[ENTRY, 7], [CALLER, 7], [0x5000, 3], [0x6000, 3], [0x7000, 3], [0x9000, 3], [0xa000, 3]]) host(c, 'map', [page, 1, bits]);
  upload(c, ENTRY, text); for (const block of caller) upload(c, block.pc, block.bytes);
  upload(c, COEFF - 16, frame(hex(spec.coefficients_hex)));
  upload(c, INPUT - 16, frame(hex(spec.input_hex)));
  for (const page of [ENTRY, CALLER]) host(c, 'protect', [page, 1, 5]);
  for (const page of [0x5000, 0x6000]) host(c, 'protect', [page, 1, 1]);
  const blocks = [...layout.map(([offset, length, instructions]) => ({pc: ENTRY + offset, bytes: text.subarray(offset, offset + length), instructions})), ...caller];
  input(c, T, words(blocks.flatMap(block => entries ? [block.pc] : [block.pc, block.bytes.length])));
  const compile = owner === 'resident' ? (entries ? 'compile_resident_entries' : 'compile_resident') : (entries ? 'compile_entries' : 'compile');
  host(c, compile, entries ? [blocks.length, 0] : [blocks.length], owner === 'resident' ? [T, T + 24] : null);
  const a = arena(c);
  c.binding = owner === 'resident' ? {id_low: a.readUInt32LE(T + 8), id_high: a.readUInt32LE(T + 12), pointer: a.readUInt32LE(T + 16), length: a.readUInt32LE(T + 20)} : {pointer: c.api.module_ptr() >>> 0, length: c.api.module_len() >>> 0};
  const bytes = Buffer.from(new Uint8Array(c.memory.buffer, c.binding.pointer, c.binding.length));
  const module = new WebAssembly.Module(bytes), imports = WebAssembly.Module.imports(module), ring3 = {};
  assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  assert.deepEqual(imports[0], {module: 'env', name: 'memory', kind: 'memory'});
  const allowed = new Set(['guard', 'guard_resident', 'read32', 'store32', 'store_resident32', 'cpu_read', 'cpu_write', 'cpu_write_resident', 'simd_execute', 'simd_execute_resident']);
  for (const item of imports.slice(1)) {
    assert.equal(item.module, 'ring3'); assert.equal(item.kind, 'function'); assert.ok(allowed.has(item.name), item.name);
    ring3[item.name] = (...args) => forward(c, item.name, args);
  }
  c.semantic = imports.some(item => item.name.startsWith('simd_execute'));
  c.run = new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3}).exports.run;
  const file = 'module-' + c.id + '.wasm'; writeFileSync(join(output, file), bytes, {flag: 'wx'});
  modules.push({context: c.id, owner, entries, file, bytes: bytes.length, sha256: hash(bytes), imports, blocks: blocks.map(block => ({pc: block.pc, length: block.bytes.length, instructions: block.instructions}))});
  contexts.push({context: c.id, owner, entries, mapped_pages: 7, semantic_pmuludq: c.semantic});
  return c;
}
function productBytes(inputWords, scale, odd) {
  const bytes = Buffer.alloc(16);
  for (let lane = 0; lane < 2; lane++) {
    const index = lane * 2 + Number(odd);
    bytes.writeBigUInt64LE(BigInt(inputWords[index]) * BigInt(scale[index]), lane * 8);
  }
  return bytes;
}
function expectedQueue(c, row, registers) {
  const q = [], add = (name, args, bytes) => q.push({name, args, ...(bytes ? {bytes} : {})});
  for (const [address, value] of [[0x907c, row.count], [0x9078, COEFF], [0x9074, INPUT], [0x9070, VECTOR], [0x906c, RETURN], [0x9068, registers[6]]]) add('store32', [address, value]);
  add('read32', [0x907c, row.count]); add('read32', [0x9078, COEFF]);
  add('cpu_read', [COEFF, 16, 16], hex(spec.coefficients_hex).subarray(0, 16));
  if (row.count) {
    add('read32', [0x9074, INPUT]); add('read32', [0x9070, VECTOR]);
    add('cpu_read', [COEFF + 16, 16, 16], hex(spec.coefficients_hex).subarray(16));
    for (let index = 0; index < row.count; index++) {
      add('cpu_read', [INPUT + index * 16, 16, 16], hex(spec.input_hex).subarray(index * 16, (index + 1) * 16));
      if (c.semantic) {
        add('simd_execute', [0x2cf4, 0x090b, 0, 0, 0], productBytes(spec.input_u32[index], spec.scale_u32, false));
        add('simd_execute', [0x2cf4, 0x0a0c, 0, 0, 0], productBytes(spec.input_u32[index], spec.scale_u32, true));
      }
      add('cpu_write', [VECTOR + index * 16, 16, 16], hex(row.expected_output_hex).subarray(index * 16, (index + 1) * 16));
    }
  }
  add('read32', [0x9068, registers[6]]); add('read32', [0x906c, RETURN]);
  add('store32', [CHECKSUM, row.expected_eax_u32]);
  return q;
}
function forward(c, name, args) {
  const before = arena(c), status = c.api[name](...args), actual = arena(c), expected = Buffer.from(before);
  assert.equal(status, 0, name);
  if (name.startsWith('guard')) { assert.deepEqual(actual, before, 'guard neutrality'); return status; }
  const resident = name === 'store_resident32' || name.endsWith('_resident');
  if (resident) assert.deepEqual(args.slice(0, 4).map(value => value >>> 0), [c.id, KEY_HIGH, c.binding.id_low, c.binding.id_high]);
  const p = args.slice(resident ? 4 : 0).map(value => value >>> 0);
  const family = name === 'store_resident32' ? 'store32' : name.replace(/_resident$/, '');
  const want = c.queue.shift(); assert.ok(want, name + ' has an original instruction'); assert.equal(family, want.name, 'ordered helper family');
  let result;
  if (family === 'read32') { assert.deepEqual(p, want.args.slice(0, 1)); result = packet(1, want.args[1]); }
  else if (family === 'store32') { assert.deepEqual(p, want.args); result = packet(1); }
  else if (family === 'cpu_read') {
    assert.deepEqual(p, want.args); assert.deepEqual(actual.subarray(T, T + 16), want.bytes);
    expected.set(want.bytes, T); result = packet(7, 0, 16);
  } else if (family === 'cpu_write') {
    assert.deepEqual(p, want.args); assert.deepEqual(before.subarray(T, T + 16), want.bytes, 'literal output at checked write'); result = packet(7, 0, 16);
  } else {
    assert.equal(family, 'simd_execute'); assert.deepEqual(p.slice(0, 5), want.args);
    const destination = p[1] === 0x090b ? 3 : 4;
    expected.set(want.bytes, SV + 32 + destination * 16); expected.set(words([0, p[5]]), T + 512);
    result = packet(7);
  }
  expected.set(result, 100); assert.deepEqual(actual, expected, name + ' full4612 declared mutations');
  helpers.push({context: c.id, case: c.case, name, args: args.map(value => value >>> 0), status, packet_hex: result.toString('hex'), ...(want.bytes ? {bytes_hex: want.bytes.toString('hex')} : {})});
  return status;
}
function dirtyState() {
  const bytes = createArena();
  bytes.writeUInt32LE(0xdfe5, SV + 16);
  for (let index = 0; index < 8; index++) {
    bytes.writeBigUInt64LE(0x8000000000000100n + BigInt(index), FP + 40 + index * 10);
    bytes.writeUInt16LE(0x3fff, FP + 48 + index * 10);
    for (let lane = 0; lane < 16; lane++) bytes[SV + 32 + index * 16 + lane] = (lane * 13 + index * 19 + 11) & 255;
  }
  return bytes;
}
function expectedSimd(seed, row) {
  const bytes = Buffer.from(seed.subarray(SV, SV + 160)), a = row.expected_accumulator_u32;
  words(a).copy(bytes, 32);
  words([row.expected_eax_u32, 0, a[1] ^ a[3], a[1] ^ a[3]]).copy(bytes, 48);
  words([a[1] ^ a[2], a[1] ^ a[3], a[1] ^ a[2], a[1] ^ a[3]]).copy(bytes, 64);
  if (row.count) {
    const values = spec.input_u32[row.count - 1], products = values.map((value, index) => BigInt(value) * BigInt(spec.scale_u32[index]));
    words(products.map(value => Number(value & 0xffffffffn))).copy(bytes, 80);
    words([Number(products[1] & 0xffffffffn), Number(products[3] & 0xffffffffn), Number(products[3] & 0xffffffffn), Number(products[3] >> 32n)]).copy(bytes, 96);
  }
  return bytes;
}
function run(c, label, budget, registers, pc, flags, seed, simd) {
  const before = arena(c), expected = Buffer.from(before), first = helpers.length;
  expected.set(state(registers, pc, flags)); expected.set(exit(budget), 56);
  expected.set(seed.subarray(FP, FP + 128), FP); expected.set(simd, SV);
  assert.equal(c.run(c.base, c.base + 56, budget, c.base + 96), 0, label);
  const actual = arena(c);
  // checked packets and 16-byte memory/semantic scratch are the only mutable spans.
  expected.set(actual.subarray(100, 156), 100);
  expected.set(actual.subarray(652, 660), 652);
  assert.deepEqual(actual, expected, label + ' complete4612');
  const file = 'arena-' + c.id + '-' + c.case + '-' + label + '.bin'; writeFileSync(join(output, file), actual, {flag: 'wx'});
  observations.push({context: c.id, case: c.case, label, budget, retired: actual.readUInt32LE(76), reason: actual.readUInt32LE(72), cpu_hex: actual.subarray(0, 56).toString('hex'), simd_hex: actual.subarray(SV, SV + 160).toString('hex'), x87_hex: actual.subarray(FP, FP + 128).toString('hex'), helper_calls: helpers.length - first, arena_file: file, arena_sha256: hash(actual)});
}
function checkRam(c, address, bytes, label) {
  for (let index = 0; index < bytes.length; index++) {
    const expected = arena(c); assert.equal(c.api.read8(address + index), 0, label);
    expected.set(packet(2, bytes[index], 1), 100); assert.deepEqual(arena(c), expected, label + ' diagnostic arena');
    diagnostics.push({context: c.id, case: c.case, label, address: address + index, expected: bytes[index]});
  }
}
for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries);
  for (const row of spec.cases) {
    c.case = row.name;
    const seed = dirtyState(), registers = [row.count, 0xa1012345, 0xa1023456, 0xa1034567, ESP, 0xa1056789, 0xa106789a, 0xa10789ab];
    seed.set(state(registers, CALLER, 0xcd7)); seed.set(exit(0), 56);
    const stack = pattern(128), outputBytes = frame(hex(spec.output_fill_hex)), checksum = pattern(64);
    upload(c, STACK, stack); upload(c, VECTOR - 16, outputBytes); upload(c, CHECKSUM - 16, checksum);
    input(c, 0, seed); c.queue = expectedQueue(c, row, registers); const first = helpers.length;
    const returned = [...registers]; returned[0] = row.expected_eax_u32; returned[4] = 0x9070;
    if (row.count) { returned[1] = INPUT; returned[2] = VECTOR; }
    const simd = expectedSimd(seed, row);
    run(c, 'natural-return', row.count ? 25 + 12 * row.count : 20, returned, RETURN, 0x446, seed, simd);
    const finished = [...returned]; finished[4] = ESP;
    run(c, 'caller-consumes-checksum', 3, finished, RETURN + 8, 0x402, seed, simd);
    assert.equal(c.queue.length, 0, 'all original helper sites consumed');
    for (const [address, value] of [[0x907c, row.count], [0x9078, COEFF], [0x9074, INPUT], [0x9070, VECTOR], [0x906c, RETURN], [0x9068, registers[6]]]) stack.writeUInt32LE(value >>> 0, address - STACK);
    hex(row.expected_output_hex).copy(outputBytes, 16); checksum.writeUInt32LE(row.expected_eax_u32, 16);
    for (const [address, bytes, label] of [[COEFF - 16, frame(hex(spec.coefficients_hex)), 'coefficients-and-canaries'], [INPUT - 16, frame(hex(spec.input_hex)), 'input-and-canaries'], [VECTOR - 16, outputBytes, 'literal-output-and-canaries'], [STACK, stack, 'natural-cdecl-stack'], [CHECKSUM - 16, checksum, 'guest-checksum-and-canaries']]) checkRam(c, address, bytes, label);
    const events = helpers.slice(first), count = name => events.filter(event => (event.name === 'store_resident32' ? 'store32' : event.name.replace(/_resident$/, '')) === name).length;
    assert.equal(count('read32'), row.count ? 6 : 4); assert.equal(count('store32'), 7);
    assert.equal(count('cpu_read'), row.count ? row.count + 2 : 1); assert.equal(count('cpu_write'), row.count);
    assert.equal(count('simd_execute'), c.semantic ? row.count * 2 : 0);
    cases.push({context: c.id, owner, entries, name: row.name, count: row.count, expected_eax_u32: row.expected_eax_u32, output_hex: outputBytes.toString('hex'), stack_hex: stack.toString('hex'), checksum_hex: checksum.toString('hex'), helper_calls: events.length, semantic_helpers: count('simd_execute')});
  }
  host(c, 'close', []);
}
assert.deepEqual(sourceHashes(), sourceBefore);
const result = {status: 'PASS', engine: {path: enginePath, sha256: engine.sha256, bytes: engine.bytes.length}, arena_bytes: ARENA_SIZE, initial_arena_sha256: hash(initial), source_sha256: sourceBefore, counts: {contexts: contexts.length, modules: modules.length, cases: cases.length, runs: observations.length, retired: observations.reduce((sum, item) => sum + item.retired, 0), helpers: helpers.length, semantic_helpers: helpers.filter(item => item.name.startsWith('simd_execute')).length, diagnostic_bytes: diagnostics.length}, contexts, modules, cases, observations, helpers, diagnostics, scope: 'One unchanged compiler-produced unsigned integer SIMD function, counts0/1/17, four bound profiles, natural CALL/RET and guest checksum consumption. Literal selected RAM/canaries and complete4612 arena assertions; checked scratch100..660 is separate from architectural state. No timing, wholeRAM, exhaustive SIMD, or arbitrary compiler compatibility claim.'};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2) + '\n', {flag: 'wx'});
console.log(JSON.stringify({status: 'PASS', ...result.counts, output}));
