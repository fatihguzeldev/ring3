import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, readdirSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(root, 'expected actual engine Wasm, output directory and repository root');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const engineBytes = readFileSync(enginePath), engineModule = new WebAssembly.Module(engineBytes);
assert.equal(hash(engineBytes), '0ac061154a23b56bfc1551540e2758282609c6f3936986ed6f583c070ffd6bef');
assert.deepEqual(WebAssembly.Module.imports(engineModule), []);
const tools = {node: process.version, v8: process.versions.v8};
assert.deepEqual(tools, {node: 'v22.16.0', v8: '12.4.254.21-node.26'});
const SIZE = 4236, TRANSFER = 140, PC = 0x1000, BODY = 112, COLD = PC + BODY, BASIS = [2, 0xcd7];
const GPRS = ['EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI'];
const VECTORS = [
  [0x1234807f, 0x234501fe, 0x3456aa55, 0x45665aff, 0x56789abc, 0x6789abcd, 0x789abcde, 0x89abcdef],
  [0x89ab817e, 0x9abc02fd, 0xabcd56a9, 0xbcdeff00, 0xcdef1357, 0xdef02468, 0xef012345, 0xf0126789]
];
const ANCHORS = [
  ['A0', 0, 0, [[0, 0xffffffff]], 0xffffffff, 0xffffffff, 0x46, 0xffffff01, 0xffffff01, 0xffffff01, 0xffffff01, 0xffffff01, 0x46],
  ['A1', 0, 1, [[0, 1], [1, 2]], 2, 2, 0x46, 1, 1, 2, 2, 2, 0x46],
  ['E0', 1, 0, [[0, 0], [1, 1]], 1, 1, 0x97, 0, 1, 0, 1, 1, 0x97],
  ['E1', 1, 0, [[0, 1], [1, 1]], 1, 1, 0x46, 1, 1, 1, 1, 1, 0x46],
  ['P0', 2, 1, [[0, 0x100], [2, 0x100], [1, 0x181]], 0x100, 0x181, 0x46, 0x101, 0x100, 0x181, 0x100, 0x100, 2],
  ['G0', 3, 1, [[0, 0], [3, 1], [1, 0x12345678]], 1, 1, 0x97, 0, 1, 0x12345678, 1, 1, 0x97],
  ['G1', 3, 1, [[0, 1], [3, 0], [1, 0x12345678]], 0, 0, 2, 0, 0, 0x12345678, 0, 0x12345678, 0x46],
  ['G2', 3, 1, [[0, 0x80000000], [3, 1], [1, 0x12345678]], 1, 1, 0x816, 0, 1, 0x12345678, 1, 1, 0x97],
  ['G3', 3, 1, [[0, 0x7fffffff], [3, 0xffffffff], [1, 0x12345678]], 0xffffffff, 0xffffffff, 0x887, 0xffffff00, 0xffffffff, 0x12345678, 0xffffffff, 0xffffffff, 0x93],
  ['G4', 3, 1, [[0, 0x10], [3, 1], [1, 0x12345678]], 1, 1, 0x16, 0, 1, 0x12345678, 1, 1, 0x97],
  ['G5', 3, 1, [[0, 0], [3, 0x80000000], [1, 0x12345678]], 0x80000000, 0x80000000, 0x887, 0x80000000, 0x80000000, 0x12345678, 0x80000000, 0x12345678, 0x46],
  ['G6', 3, 1, [[0, 0xffffffff], [3, 0], [1, 0x12345678]], 0, 0, 0x86, 0, 0, 0x12345678, 0, 0x12345678, 0x46],
  ['G7', 3, 1, [[0, 0x80000000], [3, 0xffffffff], [1, 0x12345678]], 0xffffffff, 0xffffffff, 0x93, 0xffffff00, 0xffffffff, 0x12345678, 0xffffffff, 0xffffffff, 0x93],
  ['G8', 3, 1, [[0, 1], [3, 1], [1, 1]], 1, 1, 0x46, 1, 1, 1, 1, 1, 0x46],
  ['S0', 3, 3, [[0, 0], [3, 0]], 0, 0, 0x46, 1, 0, 0, 0, 0, 2],
  ['S1', 3, 3, [[0, 1], [3, 1]], 1, 1, 0x46, 1, 1, 1, 1, 1, 0x46]
];
function compareExchange(registers, destination, source, oldFlags) {
  const a = registers[0], d = registers[destination], sourceValue = registers[source];
  const result = Number((BigInt(a) - BigInt(d) + 0x100000000n) % 0x100000000n), equal = a === d;
  const signed = value => value >= 0x80000000 ? value - 0x100000000 : value, difference = signed(a) - signed(d);
  let byte = result % 256, ones = 0; for (let bit = 0; bit < 8; bit++) {ones += byte % 2; byte = Math.floor(byte / 2);}
  const flags = (oldFlags & 0x400) + 2 + Number(a < d) + (ones % 2 === 0 ? 4 : 0)
    + (a % 16 < d % 16 ? 0x10 : 0) + (result === 0 ? 0x40 : 0) + (result >= 0x80000000 ? 0x80 : 0)
    + (difference < -0x80000000 || difference > 0x7fffffff ? 0x800 : 0);
  const next = [...registers]; if (equal) next[destination] = sourceValue; else next[0] = d;
  assert.equal((flags & 0x40) !== 0, equal); return {registers: next, flags, equal};
}
function lowByte(registers, parent, value) {const next = [...registers]; next[parent] += value - next[parent] % 256; return next;}
const SOURCE_HEX = [
  '0fb1c00f94c00f92c20fb1c0eb620fb1c80f94c00f92c20fb1c8eb540fb1d00f94c00f92c20fb1d0eb460fb1d80f94c00f92c20fb1d8eb380fb1e00f94c00f92c20fb1e0eb2a0fb1e80f94c00f92c20fb1e8eb1c0fb1f00f94c00f92c20fb1f0eb0e0fb1f80f94c00f92c20fb1f8eb000f0b',
  '0fb1c10f94c00f92c20fb1c1eb620fb1c90f94c00f92c20fb1c9eb540fb1d10f94c00f92c20fb1d1eb460fb1d90f94c00f92c20fb1d9eb380fb1e10f94c00f92c20fb1e1eb2a0fb1e90f94c00f92c20fb1e9eb1c0fb1f10f94c00f92c20fb1f1eb0e0fb1f90f94c00f92c20fb1f9eb000f0b',
  '0fb1c20f94c00f92c20fb1c2eb620fb1ca0f94c00f92c20fb1caeb540fb1d20f94c00f92c20fb1d2eb460fb1da0f94c00f92c20fb1daeb380fb1e20f94c00f92c20fb1e2eb2a0fb1ea0f94c00f92c20fb1eaeb1c0fb1f20f94c00f92c20fb1f2eb0e0fb1fa0f94c00f92c20fb1faeb000f0b',
  '0fb1c30f94c00f92c20fb1c3eb620fb1cb0f94c00f92c20fb1cbeb540fb1d30f94c00f92c20fb1d3eb460fb1db0f94c00f92c20fb1dbeb380fb1e30f94c00f92c20fb1e3eb2a0fb1eb0f94c00f92c20fb1ebeb1c0fb1f30f94c00f92c20fb1f3eb0e0fb1fb0f94c00f92c20fb1fbeb000f0b',
  '0fb1c40f94c00f92c20fb1c4eb620fb1cc0f94c00f92c20fb1cceb540fb1d40f94c00f92c20fb1d4eb460fb1dc0f94c00f92c20fb1dceb380fb1e40f94c00f92c20fb1e4eb2a0fb1ec0f94c00f92c20fb1eceb1c0fb1f40f94c00f92c20fb1f4eb0e0fb1fc0f94c00f92c20fb1fceb000f0b',
  '0fb1c50f94c00f92c20fb1c5eb620fb1cd0f94c00f92c20fb1cdeb540fb1d50f94c00f92c20fb1d5eb460fb1dd0f94c00f92c20fb1ddeb380fb1e50f94c00f92c20fb1e5eb2a0fb1ed0f94c00f92c20fb1edeb1c0fb1f50f94c00f92c20fb1f5eb0e0fb1fd0f94c00f92c20fb1fdeb000f0b',
  '0fb1c60f94c00f92c20fb1c6eb620fb1ce0f94c00f92c20fb1ceeb540fb1d60f94c00f92c20fb1d6eb460fb1de0f94c00f92c20fb1deeb380fb1e60f94c00f92c20fb1e6eb2a0fb1ee0f94c00f92c20fb1eeeb1c0fb1f60f94c00f92c20fb1f6eb0e0fb1fe0f94c00f92c20fb1feeb000f0b',
  '0fb1c70f94c00f92c20fb1c7eb620fb1cf0f94c00f92c20fb1cfeb540fb1d70f94c00f92c20fb1d7eb460fb1df0f94c00f92c20fb1dfeb380fb1e70f94c00f92c20fb1e7eb2a0fb1ef0f94c00f92c20fb1efeb1c0fb1f70f94c00f92c20fb1f7eb0e0fb1ff0f94c00f92c20fb1ffeb000f0b'
];
const SOURCE_HASHES = [
  '160c3ae362ba54a3ca18d14c342e911239af22e86b62b7100eed9150a1a31a11',
  '9f4635f04b8a7c1f85429f1c2216e6b46db3730ab9db0cf00f33de9e2f731530',
  'e945cd17a03e5f15b6c317d852600086e04d2fa63c11ed111fb6f34ed4dc64dc',
  'c65a52b5e17b8ffdc1380b6230d0f28ea8cfd170a8bbd5f54995065acc9392e6',
  '8cf13c0b90f146663f6cd460df3dc91bc14388fdc9d6067115189ba738dd87b3',
  'a7fbe7d066eb9f9acae80b0c53a1d1f563343be53bad936e50f5d2a1df21d63b',
  '2c9fa86c3d757c742635e2900dd61add26b2040d058ce305d44dc5aa6a090069',
  '1dbd5ffe0edb31d6a4ad71dd88fd69bf65a667d184ff4005c0bc32742cb603eb'
];
const PAGE_HASHES = [
  '9d4c2fb7e809eeaef6dc650df1c0211c3f07ba331ccae83bc92cef1471fa1b4c',
  '0f2e38fb357e23090539d00d40b2a3a4e09a9c76f9b48a5eb797843ceed1f51e',
  '87c8d7f4bb8056e04816b8de5501b047636900c8b1406303425a81a0f0ad8b8b',
  '7a720ced40d05c3c82ae200adc9dc008e767ca76ee3dd08bf90fbf24105672ad',
  '20aa038d534ba023849dfb0abab28dbaf9bc86393997add59789482b06f795ca',
  'efa919efaae50920022bcdca3565e9931b4d893650b2f52c9913039a15d84159',
  'eda8aca050d020f42a32cd59bf41bca9dd7f1934eaf23ff0ad65a0372c931fd7',
  'c7a402e2f486d844a95f0c7bd0f6594c04a310dbad2d31d4ed8e9105a7c55566'
];
const sources = Array.from({length: 8}, (_, bank) => {
  const bytes = Buffer.alloc(114);
  for (let source = 0; source < 8; source++) {
    const raw = 0xc0 + source * 8 + bank; bytes.set([0x0f, 0xb1, raw, 0x0f, 0x94, 0xc0, 0x0f, 0x92, 0xc2, 0x0f, 0xb1, raw, 0xeb, 112 - (source + 1) * 14], source * 14);
  }
  bytes.set([0x0f, 0x0b], BODY); assert.deepEqual(bytes, Buffer.from(SOURCE_HEX[bank], 'hex')); assert.equal(hash(bytes), SOURCE_HASHES[bank]); assert.deepEqual(readFileSync(join(output, `bank-${bank}.x86`)), bytes); return bytes;
});
const currentSource = Buffer.from(sources[0]); currentSource[16] = 0xd9;
assert.equal(hash(currentSource), '528c78c6595b670774884cffe7642eef5b019dfde319e4de3053188a283896e6');
assert.ok(BASIS.every(flags => (flags & ~0xcd7) === 0 && (flags & 2) !== 0));
const expectedCounts = {contexts: 24, modules: 26, seeds: 864, main_seeds: 768, literal_seeds: 96, pair_chains: 864,
  first_cmpxchgs: 864, second_cmpxchgs: 864, cmpxchgs: 1728, first_equal: 474, first_unequal: 390, second_equal: 144, second_unequal: 720,
  setz: 864, setb: 864, jumps: 864, literal_first_anchor_checks: 96, literal_second_anchor_checks: 96,
  positive_retirements: 4320, generated_calls: 4522, preflight: 168, owner_controls: 68, same_byte_invalidations: 16,
  consumed_modrm_invalidations: 2, current_continuations: 2, data_only_controls: 2, maps: 32, host_uploads: 52,
  host_uploaded_bytes: 67380, standalone_page_inputs: 16, pages: 128, engine_page_rows: 96, standalone_page_rows: 32,
  diagnostic_read32_calls: 98304, arena_checks: 206930};
const counts = Object.fromEntries(Object.keys(expectedCounts).map(name => [name, 0]));
const branchRows = {main: {first_equal: 0, first_unequal: 0, second_equal: 0, second_unequal: 0}, literal: {first_equal: 0, first_unequal: 0, second_equal: 0, second_unequal: 0}};
const transitions = {equal_equal: 0, equal_unequal: 0, unequal_equal: 0, unequal_unequal: 0};
const contexts = [], modules = [], pageRows = [], mutations = [], dataControls = [], invalidations = [], hostInputs = [];
function record(magic, size, fields) {
  const bytes = Buffer.alloc(size); bytes.write(magic); bytes.writeUInt16LE(1, 4); bytes.writeUInt16LE(1, 6); bytes.writeUInt32LE(size, 8);
  fields.forEach((value, index) => bytes.writeUInt32LE(value >>> 0, 16 + index * 4)); return bytes;
}
const state = (registers, pc, flags) => record('R3ST', 56, [...registers, pc, flags]);
const exit = (reason, retired) => record('R3EX', 40, [reason, retired, 0, 0, 0, 0]);
const helper = value => record('R3MH', 40, [0, value, 0, 0, 0, 0]);
const words = values => {const bytes = Buffer.alloc(values.length * 4); values.forEach((value, index) => bytes.writeUInt32LE(value >>> 0, index * 4)); return bytes;};
const pattern = Buffer.from(Array.from({length: 4096}, (_, index) => index % 251 + 1));
assert.equal(hash(pattern), 'fe3210fbc33c9e62bd9abb7526439cb3658c4d4fb4ec41d7ccbd7ad0e54975b5');
const tag = ctx => ({owner: ctx.owner, bank: ctx.bank, context: ctx.low});
function refresh(ctx) {ctx.bytes = new Uint8Array(ctx.memory.buffer); ctx.view = new DataView(ctx.memory.buffer); return ctx;}
function check(ctx, label) {refresh(ctx); assert.deepEqual(Buffer.from(ctx.bytes.subarray(ctx.base, ctx.base + SIZE)), ctx.expected, `${ctx.owner}/bank ${ctx.bank}/${label}: complete arena`); counts.arena_checks++;}
function pure(ctx, action, label, status = 0) {check(ctx, `before ${label}`); assert.equal(action(), status, label); check(ctx, label);}
function request(ctx, bytes) {refresh(ctx).bytes.set(bytes, ctx.base + TRANSFER); ctx.expected.set(bytes, TRANSFER); check(ctx, 'explicit host request');}
function upload(ctx, address, bytes, purpose) {
  request(ctx, bytes); pure(ctx, () => ctx.api.upload(address, bytes.length), 'explicit host upload'); counts.host_uploads++; counts.host_uploaded_bytes += bytes.length;
  hostInputs.push({kind: 'upload', ...tag(ctx), purpose, address, length: bytes.length, sha256: hash(bytes)});
}
function fresh(owner, bank) {
  const expected = Buffer.alloc(SIZE), ordinal = ++counts.contexts, codePage = Buffer.alloc(4096); codePage.set(sources[bank]); assert.equal(hash(codePage), PAGE_HASHES[bank]);
  expected.set(state(Array(8).fill(0), 0, 2)); expected.set(exit(1, 0), 56); expected.set(helper(0), 100);
  const ctx = {owner, bank, expected, codePage, dataPage: Buffer.from(pattern), low: ordinal, high: 0xc11f0000};
  if (owner === 'standalone') {
    ctx.memory = new WebAssembly.Memory({initial: 1}); ctx.base = 128; ctx.dataAddress = 0x8000; ctx.codeAddress = 0x6000;
    refresh(ctx).bytes.set(expected, ctx.base); ctx.bytes.set(ctx.dataPage, ctx.dataAddress); ctx.bytes.set(codePage, ctx.codeAddress); counts.standalone_page_inputs += 2;
    for (const [address, bytes] of [[ctx.dataAddress, ctx.dataPage], [ctx.codeAddress, codePage]]) hostInputs.push({kind: 'standalone_page', ...tag(ctx), address, length: bytes.length, sha256: hash(bytes)});
  } else {
    const instance = new WebAssembly.Instance(engineModule, {}), arities = {open: 3, close: 0, arena_ptr: 0, map: 3, upload: 2, read32: 1,
      compile_entries: 2, compile_resident: 1, generation: 0, module_ptr: 0, module_len: 0, guard: 6, guard_resident: 7};
    ctx.api = Object.fromEntries(Object.entries(arities).map(([name, arity]) => {
      const fn = instance.exports[`ring3_abi_v1_${name}`]; assert.equal(typeof fn, 'function', name); assert.equal(fn.length, arity, name); return [name, fn];
    }));
    ctx.memory = instance.exports.memory; assert.equal(ctx.api.open(3, ctx.low, ctx.high), 0);
    ctx.base = ctx.api.arena_ptr() >>> 0; ctx.dataAddress = 0x3000; ctx.codeAddress = PC; check(ctx, 'independently initialized arena');
    for (const [address, permissions, bytes, purpose] of [[ctx.dataAddress, 3, ctx.dataPage, 'initial_data'], [PC, 7, sources[bank], 'initial_code']]) {
      pure(ctx, () => ctx.api.map(address, 1, permissions), 'page map'); counts.maps++; hostInputs.push({kind: 'map', ...tag(ctx), address, pages: 1, permissions}); upload(ctx, address, bytes, purpose);
    }
  }
  ctx.expected.set(pattern, TRANSFER); refresh(ctx).bytes.set(pattern, ctx.base + TRANSFER); check(ctx, 'explicit patterned transfer');
  contexts.push({owner, bank, ordinal, arena_base: ctx.base, code_address: ctx.codeAddress, data_address: ctx.dataAddress,
    ...(ctx.api ? {key_low: ctx.low, key_high: ctx.high} : {})}); return ctx;
}
function compile(ctx, phase = 'initial') {
  let bytes, binding = {}; const file = `${ctx.owner}-${phase}-bank-${ctx.bank}.wasm`;
  if (ctx.owner === 'standalone') bytes = readFileSync(join(output, file));
  else {
    const entries = Array.from({length: 8}, (_, source) => PC + 14 * source);
    request(ctx, words(ctx.owner === 'replacement' ? entries : entries.flatMap(entry => [entry, 14])));
    if (ctx.owner === 'replacement') {
      pure(ctx, () => ctx.api.compile_entries(8, 0), 'eight-entry compiler');
      binding = {generation: ctx.api.generation(), pointer: ctx.api.module_ptr() >>> 0, length: ctx.api.module_len() >>> 0}; assert.ok(binding.generation > 0);
    } else {
      check(ctx, 'before eight-extent compiler'); assert.equal(ctx.api.compile_resident(8), 0); refresh(ctx);
      assert.equal(ctx.view.getUint32(ctx.base + TRANSFER, true), 1); assert.equal(ctx.view.getUint32(ctx.base + TRANSFER + 4, true), 24);
      binding = Object.fromEntries(['low', 'high', 'pointer', 'length'].map((name, index) => [name, ctx.view.getUint32(ctx.base + TRANSFER + 8 + index * 4, true)]));
      assert.ok(binding.low !== 0 || binding.high !== 0); ctx.expected.set(words([1, 24, binding.low, binding.high, binding.pointer, binding.length]), TRANSFER); check(ctx, 'resident metadata only with descriptor tail retained');
    }
    refresh(ctx); assert.ok(binding.pointer > 0 && binding.length > 8 && binding.length <= 65536 && binding.pointer + binding.length <= ctx.bytes.length);
    bytes = Buffer.from(ctx.bytes.subarray(binding.pointer, binding.pointer + binding.length)); writeFileSync(join(output, file), bytes, {flag: 'wx'});
  }
  assert.ok(bytes.length > 8 && bytes.length <= 65536); const module = new WebAssembly.Module(bytes), guards = ctx.owner === 'standalone' ? [] : [ctx.owner === 'replacement' ? 'guard' : 'guard_resident'];
  const declarations = [{module: 'env', name: 'memory', kind: 'memory'}, ...guards.map(name => ({module: 'ring3', name, kind: 'function'}))];
  assert.deepEqual(WebAssembly.Module.imports(module), declarations); assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  const child = new WebAssembly.Instance(module, {env: {memory: ctx.memory}, ...(ctx.api ? {ring3: ctx.api} : {})}); assert.equal(child.exports.run.length, 4); counts.modules++;
  modules.push({...tag(ctx), phase, file, source_sha256: hash(ctx.codePage.subarray(0, 114)), sha256: hash(bytes), length: bytes.length, imports: declarations,
    run_arity: child.exports.run.length, ...(ctx.api ? {key_low: ctx.low, key_high: ctx.high} : {}), ...binding}); return {phase, file, bytes, ...binding, run: child.exports.run};
}
function seed(ctx, registers, source, flags) {
  ctx.expected.set(state(registers, PC + source * 14, flags)); ctx.expected.set(exit(3, 0), 56); ctx.expected.writeUInt32LE(0, 96); ctx.expected.set(helper(0xdecafbad), 100);
  refresh(ctx).bytes.set(ctx.expected.subarray(0, 140), ctx.base); check(ctx, 'one initial CPU/helper seed'); counts.seeds++;
}
function cancel(ctx, value) {ctx.expected.writeUInt32LE(value, 96); refresh(ctx).view.setUint32(ctx.base + 96, value, true);}
function run(ctx, unit, budget, label, registers, pc, flags, reason = 1, retired = 1, status = 0, malformed = false) {
  check(ctx, `before ${label}`); if (status === 0) {ctx.expected.set(state(registers, pc, flags)); ctx.expected.set(exit(reason, retired), 56);}
  const pointers = malformed ? [0xffffffff, 0xffffffff, 0xffffffff] : [ctx.base, ctx.base + 56, ctx.base + 96];
  assert.equal(unit.run(pointers[0], pointers[1], budget, pointers[2]), status, label); check(ctx, label); counts.generated_calls++;
}
const identity = (ctx, unit) => ctx.owner === 'replacement' ? {generation: unit.generation} : {low: unit.low, high: unit.high};
const cpuEvidence = ctx => ({state_hex: ctx.expected.subarray(0, 56).toString('hex'), exit_hex: ctx.expected.subarray(56, 96).toString('hex'), helper_hex: ctx.expected.subarray(100, 140).toString('hex')});
function physical(ctx, unit) {refresh(ctx); const bytes = Buffer.from(ctx.bytes.subarray(unit.pointer, unit.pointer + unit.length)); assert.deepEqual(bytes, unit.bytes); return {pointer: unit.pointer, length: unit.length, sha256: hash(bytes)};}
function guard(ctx, unit, wrong) {
  const key = ctx.low ^ Number(wrong === 'key');
  return ctx.owner === 'replacement' ? ctx.api.guard(key, ctx.high, unit.generation + Number(wrong === 'identity'), ctx.base, ctx.base + 56, ctx.base + 96)
    : ctx.api.guard_resident(key, ctx.high, unit.low ^ Number(wrong === 'identity'), unit.high, ctx.base, ctx.base + 56, ctx.base + 96);
}
function mutateConsumed(ctx, unit, registers, flags) {
  assert.equal(ctx.bank, 0); assert.equal(ctx.expected.readUInt32LE(48), 0x1011); assert.equal(flags, 0x446);
  assert.deepEqual(registers, [2, 2, ...VECTORS[0].slice(2)]); assert.equal(ctx.codePage[16], 0xc8); assert.equal(ctx.dataPage[7], 8);
  const data = {...tag(ctx), address: ctx.dataAddress + 7, old_byte: 8, new_byte: 0xa5, upload_bytes: 1, before: cpuEvidence(ctx), identity: identity(ctx, unit), module: physical(ctx, unit)};
  upload(ctx, data.address, Buffer.from([0xa5]), 'data_only'); ctx.dataPage[7] = 0xa5;
  pure(ctx, () => guard(ctx, unit), 'correct owner after unrelated data upload'); counts.owner_controls++; counts.data_only_controls++;
  data.after = cpuEvidence(ctx); assert.deepEqual(data.after, data.before); assert.deepEqual(identity(ctx, unit), data.identity); assert.deepEqual(physical(ctx, unit), data.module);
  data.guard_status = 0; data.page_sha256 = hash(ctx.dataPage); assert.equal(data.page_sha256, '1c0e57497f7f02759fdd13d0ce4c2187808a9da9b0f22639eac1430002d7938d'); dataControls.push(data);
  const row = {...tag(ctx), address: PC + 16, byte_offset: 16, old_byte: 0xc8, new_byte: 0xd9, upload_bytes: 1,
    eip_before: 0x1011, flags, before: cpuEvidence(ctx), old_identity: identity(ctx, unit), old_module: physical(ctx, unit)};
  upload(ctx, row.address, Buffer.from([0xd9]), 'consumed_modrm'); ctx.codePage[16] = 0xd9;
  assert.deepEqual(ctx.codePage.subarray(0, 114), currentSource); assert.equal(hash(ctx.codePage), '2b06555a304c4fda459b7eed14387f7c9650db3b81e2711617e70d80db484f8c');
  row.source_sha256 = hash(currentSource); row.code_page_sha256 = hash(ctx.codePage); cancel(ctx, 1);
  run(ctx, unit, 0, 'consumed ModRM stale owner before cancelled malformed pointers', registers, 0x1011, flags, 0, 0, 4, true);
  assert.deepEqual(physical(ctx, unit), row.old_module, 'consumed upload and stale owner retain physical module bytes and pointer before fresh compilation');
  counts.owner_controls++; counts.consumed_modrm_invalidations++; row.stale_status = 4; cancel(ctx, 0); row.cancel_before_continuation = 0;
  const current = compile(ctx, 'current'); assert.notDeepEqual(identity(ctx, current), row.old_identity); row.new_identity = identity(ctx, current); row.current_module = physical(ctx, current);
  assert.deepEqual(cpuEvidence(ctx), row.before); ctx.pendingMutation = row; return current;
}
function pages(ctx, label) {
  for (const [address, expected] of [[ctx.dataAddress, ctx.dataPage], [ctx.codeAddress, ctx.codePage]]) {
    const actual = Buffer.alloc(4096);
    if (ctx.owner === 'standalone') {actual.set(refresh(ctx).bytes.subarray(address, address + 4096)); counts.standalone_page_rows++;}
    else {
      for (let offset = 0; offset < 4096; offset += 4) {
        check(ctx, 'before separate diagnostic Read32'); assert.equal(ctx.api.read32(address + offset), 0); refresh(ctx);
        actual.writeUInt32LE(ctx.view.getUint32(ctx.base + 120, true), offset); ctx.expected.set(helper(expected.readUInt32LE(offset)), 100); check(ctx, 'diagnostic Read32 changes helper v1 only'); counts.diagnostic_read32_calls++;
      }
      counts.engine_page_rows++;
    }
    assert.deepEqual(actual, expected, 'complete declared page follows only explicit host inputs'); counts.pages++; pageRows.push({...tag(ctx), label, address, sha256: hash(actual)});
  }
}
function literalInitial(row) {const registers = [...VECTORS[0]]; for (const [index, value] of row[3]) registers[index] = value; return registers;}
function anchor(registers, flags, row, initialFlags, second) {
  const [, bank, , , firstA, firstD, firstFlags, currentA, , , secondA, secondD, secondFlags] = row;
  const expected = literalInitial(row); expected[0] = firstA; expected[bank] = firstD;
  if (second) {expected[0] = currentA; expected[2] += firstFlags % 2 - expected[2] % 256; expected[0] = secondA; expected[bank] = secondD;}
  assert.deepEqual(registers, expected, 'literal full implicit EAX/source/destination and current second state');
  assert.equal(flags, (second ? secondFlags : firstFlags) + (initialFlags & 0x400));
  counts[second ? 'literal_second_anchor_checks' : 'literal_first_anchor_checks']++;
}
function observeBranch(literal, phase, equal) {
  const key = `${phase}_${equal ? 'equal' : 'unequal'}`; counts[key]++; branchRows[literal ? 'literal' : 'main'][key]++;
}
function chain(ctx, initialUnit, initial, source, initialFlags, literal = null, mutation = false) {
  let unit = initialUnit, registers = [...initial], flags = initialFlags; const entry = PC + source * 14;
  seed(ctx, registers, source, flags); counts[literal ? 'literal_seeds' : 'main_seeds']++; counts.pair_chains++;
  if (!ctx.preflightDone && literal === null) {
    run(ctx, unit, 0, 'zero budget before first target', registers, entry, flags, 1, 0); counts.preflight++;
    cancel(ctx, 1); run(ctx, unit, 1, 'cancel before first target', registers, entry, flags, 2, 0); counts.preflight++; cancel(ctx, 0); ctx.preflightDone = true;
  }
  const first = compareExchange(registers, ctx.bank, source, flags); ({registers, flags} = first);
  run(ctx, unit, 1, 'first CMPXCHG full current implicit EAX and captured operands', registers, entry + 3, flags);
  counts.first_cmpxchgs++; counts.cmpxchgs++; counts.positive_retirements++; observeBranch(literal, 'first', first.equal);
  if (literal) {
    anchor(registers, flags, literal, initialFlags, false);
    if (['A0', 'E1', 'G8', 'S0', 'S1'].includes(literal[0])) assert.deepEqual(registers, initial, 'fixed GPR first target still updates FLAGS and retires');
  }
  if (mutation) unit = mutateConsumed(ctx, unit, registers, flags);
  registers = lowByte(registers, 0, Number((flags & 0x40) !== 0)); run(ctx, unit, 1, 'live SETZ AL full CPU', registers, entry + 6, flags); counts.setz++; counts.positive_retirements++;
  if (ctx.pendingMutation) {
    assert.equal(entry, 0x100e); assert.deepEqual(registers, [1, 2, ...VECTORS[0].slice(2)]); assert.equal(flags, 0x446);
    ctx.pendingMutation.eip_after = entry + 6; ctx.pendingMutation.retired = 1; ctx.pendingMutation.after = cpuEvidence(ctx); mutations.push(ctx.pendingMutation); delete ctx.pendingMutation; counts.current_continuations++;
  }
  registers = lowByte(registers, 2, flags % 2); run(ctx, unit, 1, 'live SETB DL full CPU', registers, entry + 9, flags); counts.setb++; counts.positive_retirements++;
  if (literal) assert.deepEqual([registers[0], registers[ctx.bank], registers[source]], literal.slice(7, 10), 'literal current EAX/destination/source after both overlapping consumers');
  const beforeSecond = state(registers, entry + 9, flags), second = compareExchange(registers, ctx.bank, source, flags); ({registers, flags} = second);
  run(ctx, unit, 1, 'second CMPXCHG from current EAX and both current operands', registers, entry + 12, flags);
  counts.second_cmpxchgs++; counts.cmpxchgs++; counts.positive_retirements++; observeBranch(literal, 'second', second.equal);
  transitions[`${first.equal ? 'equal' : 'unequal'}_${second.equal ? 'equal' : 'unequal'}`]++;
  if (literal) {
    anchor(registers, flags, literal, initialFlags, true);
    if (['A0', 'E1', 'G8', 'S1'].includes(literal[0])) assert.deepEqual(state(registers, entry + 9, flags), beforeSecond, 'fixed current second target retains GPRs and FLAGS while retiring');
  }
  run(ctx, unit, 1, 'following short JMP', registers, COLD, flags); counts.jumps++; counts.positive_retirements++; return unit;
}
function numeric(ctx, initialUnit) {
  let unit = initialUnit;
  for (const [index, vector] of VECTORS.entries()) for (const flags of BASIS) for (let source = 0; source < 8; source++) {
    const registers = [...vector]; if (index === 1) registers[0] = vector[ctx.bank];
    unit = chain(ctx, unit, registers, source, flags);
  }
  for (const literal of ANCHORS.filter(row => row[1] === ctx.bank)) for (const flags of BASIS) {
    unit = chain(ctx, unit, literalInitial(literal), literal[2], flags, literal, Boolean(ctx.api && literal[0] === 'A1' && flags === 0xcd7));
  }
  return unit;
}
function finish(ctx, unit) {
  const registers = Array.from({length: 8}, (_, index) => ctx.expected.readUInt32LE(16 + index * 4)), flags = ctx.expected.readUInt32LE(52);
  for (const [name, cancelled, budget, reason] of [['zero budget', 0, 0, 1], ['cancel with budget', 1, 1, 2], ['cancel with zero budget', 1, 0, 2]]) {
    cancel(ctx, cancelled); run(ctx, unit, budget, name, registers, COLD, flags, reason, 0); counts.preflight++;
  }
  cancel(ctx, 0); run(ctx, unit, 1, 'cold entry', registers, COLD, flags, 3, 0); counts.preflight++;
  run(ctx, unit, 0, 'malformed pointers', registers, COLD, flags, 0, 0, 1, true); counts.preflight++; pages(ctx, 'final');
  if (ctx.owner === 'standalone') return;
  for (const wrong of ['key', 'identity']) {pure(ctx, () => guard(ctx, unit, wrong), `wrong ${wrong}`, 3); counts.owner_controls++;}
  assert.equal(ctx.codePage[0], 0x0f); const row = {...tag(ctx), address: PC, old_byte: 0x0f, new_byte: 0x0f, upload_bytes: 1, before: cpuEvidence(ctx), identity: identity(ctx, unit), module: physical(ctx, unit)};
  upload(ctx, PC, Buffer.from([0x0f]), 'same_opcode'); cancel(ctx, 1);
  run(ctx, unit, 0, 'same-byte stale current owner before cancelled malformed pointers', registers, COLD, flags, 0, 0, 4, true); counts.owner_controls++; counts.same_byte_invalidations++;
  assert.deepEqual(physical(ctx, unit), row.module, 'same-byte upload and stale owner retain physical module bytes and pointer before close');
  row.stale_status = 4; row.after_stale = cpuEvidence(ctx); assert.deepEqual(row.after_stale, row.before); pages(ctx, 'after_samebyte');
  pure(ctx, () => ctx.api.close(), 'close'); run(ctx, unit, 0, 'closed current owner before cancelled malformed pointers', registers, COLD, flags, 0, 0, 5, true);
  counts.owner_controls++; row.closed_status = 5; row.after_closed = cpuEvidence(ctx); invalidations.push(row);
}
for (const owner of ['standalone', 'replacement', 'resident']) for (let bank = 0; bank < 8; bank++) {
  const ctx = fresh(owner, bank), unit = compile(ctx); pages(ctx, 'initial'); const current = numeric(ctx, unit); finish(ctx, current);
}
assert.equal(Object.keys(counts).length, 35); assert.deepEqual(Object.keys(counts), Object.keys(expectedCounts));
for (const [name, expected] of Object.entries(expectedCounts)) assert.equal(counts[name], expected, `exact observed ${name} census`);
assert.deepEqual(branchRows, {main: {first_equal: 432, first_unequal: 336, second_equal: 96, second_unequal: 672}, literal: {first_equal: 42, first_unequal: 54, second_equal: 48, second_unequal: 48}});
assert.deepEqual(transitions, {equal_equal: 126, equal_unequal: 348, unequal_equal: 18, unequal_unequal: 372});
const roles = [];
for (const owner of ['standalone', 'replacement', 'resident']) for (let bank = 0; bank < 8; bank++) {
  roles.push([owner, bank, 'initial']); if (owner !== 'standalone' && bank === 0) roles.push([owner, bank, 'current']);
}
assert.deepEqual(modules.map(({owner, bank, phase}) => [owner, bank, phase]), roles);
for (const module of modules) {
  const matches = contexts.filter(ctx => ctx.owner === module.owner && ctx.bank === module.bank && ctx.ordinal === module.context);
  assert.equal(matches.length, 1, 'explicit unique module-context join'); const [ctx] = matches;
  if (module.owner !== 'standalone') assert.deepEqual([module.key_low, module.key_high], [ctx.key_low, ctx.key_high]);
  assert.equal(module.source_sha256, module.phase === 'current' ? hash(currentSource) : SOURCE_HASHES[module.bank]);
}
assert.equal(contexts.length, 24); assert.equal(mutations.length, 2); assert.equal(dataControls.length, 2); assert.equal(invalidations.length, 16); assert.equal(hostInputs.length, 100);
const artifactNames = [...sources.map((_, bank) => `bank-${bank}.x86`), ...modules.map(module => module.file), 'result.json'];
assert.equal(artifactNames.length, 35); assert.equal(new Set(artifactNames).size, 35);
const result = {status: 'ok', engine_sha256: hash(engineBytes), tools, counts, contexts, modules, pages: pageRows, data_only_controls: dataControls,
  consumed_modrm_mutations: mutations, code_invalidations: invalidations, host_inputs: hostInputs,
  source: {provenance: 'existing physically pinned local Intel primary; no fresh download or newest-edition claim', order: '253666-093US', edition: 'September 2026',
    pdf_path: 'target/p2-memory-binary-spec/253666-093-sdm-vol-2a.pdf', pdf_bytes: 3404694, pdf_sha256: '87c5acb6f27e24d9d364841a0d2346c91a8482e36f403409954e2d2a8c817bac',
    full_text_path: 'target/p2-memory-binary-spec/253666-093-sdm-vol-2a.txt', full_text_bytes: 1557075, full_text_sha256: 'fa7f27a4b5f830e007fa6a967e53436aa7051fe99d8d56303efcf4148b0a5eaf',
    chapter_sha256: 'ffa52e2a5725c967769683fc49278ea3de07f8a40935c82565e24d8ede183fa1', chapter_bytes: 4590, chapter_lines: [15502, 15590],
    chapter_separator_policy: 'includes internal PDF PAGE 313 and excludes surrounding separators', pdf_pages_zero_based: [311, 312], printed_pages: ['3-194', '3-195'],
    page_bytes: [2578, 1999], page_lines: [[15502, 15557], [15559, 15590]], page_separator_policy: 'exclude surrounding PDF PAGE separators',
    page_sha256: ['fa0acf7389b46652ab63f2009a323b40a4d8f1066b84da681732db85a68e01d3', '4d7ce3ae41ca41b6b47049fbff718e852b3ccb8c016cee7a145d3939c7a48def'],
    comparison_chapter: {lines: [13835, 13920], bytes: 4498, sha256: '0fbcc28b5a7b213891673a0a87a790ae996cb9200c1209631934528a5f457d34', pdf_pages_zero_based: [278, 279], printed_pages: ['3-161', '3-162']}},
  oracle: {gprs: GPRS, vectors: VECTORS, basis_flags: BASIS, ordered_pairs: GPRS.flatMap((destination, bank) => GPRS.map((source, index) => ({destination, source, modrm: 0xc0 + 8 * index + bank}))),
    equality_setup: 'V0 is unchanged; V1 replaces only EAX with that bank destination before the initial seed',
    literal_anchor_fields: ['name', 'bank', 'source', 'initial_register_overrides', 'first_eax', 'first_destination', 'first_flags', 'current_eax', 'current_destination', 'current_source', 'second_eax', 'second_destination', 'second_flags'], literal_anchors: ANCHORS, literal_anchor_count: 32,
    branch_counts: branchRows, chain_transitions: transitions,
    limits: {instructions: 64, blocks: 8, wasm_bytes: 65536}, bank: {specs: Array.from({length: 8}, (_, source) => [PC + source * 14, 14]), row_offsets: [0, 14, 28, 42, 56, 70, 84, 98],
      step_next_offsets: [3, 6, 9, 12], instructions: 40, body_bytes: BODY, source_bytes: 114, cold_target: COLD, jump_displacements: [98, 84, 70, 56, 42, 28, 14, 0]},
    programs: sources.map((bytes, bank) => ({bank, hex: bytes.toString('hex'), sha256: hash(bytes), code_page_sha256: PAGE_HASHES[bank]})),
    current_bank_0: {hex: currentSource.toString('hex'), sha256: hash(currentSource), code_page_sha256: '2b06555a304c4fda459b7eed14387f7c9650db3b81e2711617e70d80db484f8c'},
    arena_bytes: SIZE, pattern_sha256: hash(pattern), changed_data_sha256: '1c0e57497f7f02759fdd13d0ce4c2187808a9da9b0f22639eac1430002d7938d',
    classifications: {main_self_chains: 96, main_distinct_chains: 672, literal_self_chains: 18, literal_distinct_chains: 78, self_targets: 228, distinct_targets: 1500},
    fixed_points: {gpr_fixed_first_observations: 30, full_gpr_flags_fixed_second_observations: 24},
    policy: 'widened unsigned BigInt subtraction of current old EAX minus current destination and separate signed numeric-range overflow, nibble borrow and eight-bit division parity; equality captures old source before destination assignment, inequality captures old destination before implicit EAX assignment, including source EAX and self; numerical AL/DL insertion and current operand capture after both live consumers; no intermediate CPU repair; destination EAX always equals; changed consumed C8 to D9 stays behind EIP 1011, fresh SETZ retains ECX 2 rather than replaying the changed equality assignment of EBX 45665aff',
    diagnostic_helper_policy: 'guest modules import no data helpers and retain R3MHv1; separate complete page Read32 diagnostics change only helper v1 success value with length/reserved zero, with before/after whole-arena checks'},
  artifact_census: {x86_banks: 8, modules: 26, result_files: 1, total: 35, names: artifactNames},
  test_sha256: Object.fromEntries(['engine/tests/cpu_cmpxchg32.rs', 'engine/tests/cpu_cmpxchg32_wasm.rs', 'engine/tests/fixtures/p2-cmpxchg32/run.mjs'].map(path => [path, hash(readFileSync(join(root, path)))])),
  claim: 'prefix-free flat32 register-only CMPXCHG32 through native-generated standalone and actual engine-Wasm replacement/resident modules: all 64 ordered GPR pairs/eight self pairs, V0 inequality for non-EAX destinations and bank-adjusted V1 equality, complementary FLAGS 2/cd7 plus sixteen literal chains; independent current EAX-minus-destination subtraction oracle and 32 literal full target anchors, defined comparison flags with DF/fixed2 retained and incoming CF/ZF ignored, full CPU before SETZ AL/SETB DL and the current second target, implicit EAX/source/self/fixed cases; 864 seeds/1728 CMPXCHG/864 SETZ/864 SETB/864 JMP/4522 calls with exact observed branch transitions, pure ABI/default caps/4236-byte arenas/128 current complete pages, two unrelated data-validity controls, two consumed-ModRM stale owners and fresh no-replay same-CPU 1011 continuations retaining ECX 2, sixteen same-opcode stale4/closed5 controls before cancelled malformed arguments; no inequality for destination EAX, exhaustive operand-FLAGS Cartesian, memory/atomic/LOCK/byte/word/prefix/x64/RTM/guest SMC/new helpers-ABI-local-caps/PE/Windows/SDK/browser/performance/fullCI/game claim'};

const resultBytes = Buffer.from(JSON.stringify(result, null, 2)); writeFileSync(join(output, 'result.json'), resultBytes, {flag: 'wx'});
assert.deepEqual(readdirSync(output).sort(), [...artifactNames].sort(), 'exact saved artifact filenames');
console.log(JSON.stringify({status: result.status, engine_sha256: result.engine_sha256, result_sha256: hash(resultBytes), counts, output}));
