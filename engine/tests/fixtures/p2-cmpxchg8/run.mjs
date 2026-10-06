import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, readdirSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../support/engine.mjs';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(root, 'expected actual engine Wasm, output directory and repository root');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const {bytes: engineBytes, module: engineModule, sha256: engineSha256} = readEngine(enginePath);
assert.deepEqual(WebAssembly.Module.imports(engineModule), []);
const tools = {node: process.version, v8: process.versions.v8};
assert.deepEqual(tools, {node: 'v22.16.0', v8: '12.4.254.21-node.26'});
const SIZE = 4236, TRANSFER = 140, PC = 0x1000, BODY = 112, COLD = PC + BODY, BASIS = [2, 0xcd7];
const GPRS = ['EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI'];
const ALIASES = ['AL', 'CL', 'DL', 'BL', 'AH', 'CH', 'DH', 'BH'];
const PARENTS = [0, 1, 2, 3, 0, 1, 2, 3], BYTE_POSITIONS = [3, 3, 3, 3, 2, 2, 2, 2];
const VECTORS = [
  [0x12347f02, 0x23458120, 0x34568040, 0x4566ff60, 0x56789abc, 0x6789abcd, 0x789abcde, 0x89abcdef],
  [0x89ab0281, 0x9abc4080, 0xabcd607f, 0xbcde20ff, 0xcdef1357, 0xdef02468, 0xef012345, 0xf0126789]
];
const ANCHORS = [
  ['A0', 0, 0, [[0, 0xff]], 0xff, 0xff, 0x46, 1, 1, 1, 1, 1, 0x46],
  ['H3', 0, 4, [[0, 0x80], [4, 0x7f]], 0x7f, 0x7f, 0x46, 1, 1, 0x7f, 0x7f, 0x7f, 0x46],
  ['A1', 0, 1, [[0, 1], [1, 2]], 2, 2, 0x46, 1, 1, 2, 2, 2, 0x46],
  ['E0', 1, 0, [[0, 0], [1, 1]], 1, 1, 0x97, 0, 1, 0, 1, 1, 0x97],
  ['E1', 1, 0, [[0, 1], [1, 1]], 1, 1, 0x46, 1, 1, 1, 1, 1, 0x46],
  ['P0', 2, 1, [[0, 0], [2, 0], [1, 0x81]], 0, 0x81, 0x46, 1, 0, 0x81, 0, 0, 2],
  ['G0', 3, 1, [[0, 0], [3, 1], [1, 0x23]], 1, 1, 0x97, 0, 1, 0x23, 1, 1, 0x97],
  ['G1', 3, 1, [[0, 1], [3, 0], [1, 0x23]], 0, 0, 2, 0, 0, 0x23, 0, 0x23, 0x46],
  ['G2', 3, 1, [[0, 0x80], [3, 1], [1, 0x23]], 1, 1, 0x812, 0, 1, 0x23, 1, 1, 0x97],
  ['G3', 3, 1, [[0, 0x7f], [3, 0xff], [1, 0x23]], 0xff, 0xff, 0x883, 0, 0xff, 0x23, 0xff, 0xff, 0x13],
  ['G4', 3, 1, [[0, 0x10], [3, 1], [1, 0x23]], 1, 1, 0x16, 0, 1, 0x23, 1, 1, 0x97],
  ['G5', 3, 1, [[0, 0], [3, 0x80], [1, 0x23]], 0x80, 0x80, 0x883, 0, 0x80, 0x23, 0x80, 0x80, 0x883],
  ['G6', 3, 1, [[0, 0xff], [3, 0], [1, 0x23]], 0, 0, 0x86, 0, 0, 0x23, 0, 0x23, 0x46],
  ['G7', 3, 1, [[0, 0x80], [3, 0xff], [1, 0x23]], 0xff, 0xff, 0x97, 0, 0xff, 0x23, 0xff, 0xff, 0x13],
  ['G8', 3, 1, [[0, 1], [3, 1], [1, 1]], 1, 1, 0x46, 1, 1, 1, 1, 1, 0x46],
  ['S0', 3, 3, [[0, 0], [3, 0]], 0, 0, 0x46, 1, 0, 0, 0, 0, 2],
  ['S1', 3, 3, [[0, 1], [3, 1]], 1, 1, 0x46, 1, 1, 1, 1, 1, 0x46],
  ['H0', 4, 0, [[0, 1], [4, 1]], 1, 1, 0x46, 1, 1, 1, 1, 1, 0x46],
  ['H1', 4, 1, [[0, 0x80], [4, 0x80], [1, 0x7f]], 0x80, 0x7f, 0x46, 1, 0x7f, 0x7f, 0x7f, 0x7f, 0x97],
  ['H2', 4, 0, [[0, 0], [4, 0x80]], 0x80, 0x80, 0x883, 0, 0x80, 0, 0x80, 0x80, 0x883]
];
const PARENT_ANCHORS = [
  ['A0', 0x12347fff, 0x12347fff, 0x12347f01, 0x12347f01, 0x34568000, 0x12347f01, 0x12347f01],
  ['H3', 0x12347f7f, 0x12347f7f, 0x12347f01, 0x12347f01, 0x34568000, 0x12347f7f, 0x12347f7f],
  ['A1', 0x12347f02, 0x12347f02, 0x12347f01, 0x12347f01, 0x34568000, 0x12347f02, 0x12347f02],
  ['E0', 0x12347f01, 0x23458101, 0x12347f00, 0x23458101, 0x34568001, 0x12347f01, 0x23458101],
  ['E1', 0x12347f01, 0x23458101, 0x12347f01, 0x23458101, 0x34568000, 0x12347f01, 0x23458101],
  ['P0', 0x12347f00, 0x34568081, 0x12347f01, 0x34568000, 0x34568000, 0x12347f00, 0x34568000],
  ['G0', 0x12347f01, 0x4566ff01, 0x12347f00, 0x4566ff01, 0x34568001, 0x12347f01, 0x4566ff01],
  ['G1', 0x12347f00, 0x4566ff00, 0x12347f00, 0x4566ff00, 0x34568000, 0x12347f00, 0x4566ff23],
  ['G2', 0x12347f01, 0x4566ff01, 0x12347f00, 0x4566ff01, 0x34568000, 0x12347f01, 0x4566ff01],
  ['G3', 0x12347fff, 0x4566ffff, 0x12347f00, 0x4566ffff, 0x34568001, 0x12347fff, 0x4566ffff],
  ['G4', 0x12347f01, 0x4566ff01, 0x12347f00, 0x4566ff01, 0x34568000, 0x12347f01, 0x4566ff01],
  ['G5', 0x12347f80, 0x4566ff80, 0x12347f00, 0x4566ff80, 0x34568001, 0x12347f80, 0x4566ff80],
  ['G6', 0x12347f00, 0x4566ff00, 0x12347f00, 0x4566ff00, 0x34568000, 0x12347f00, 0x4566ff23],
  ['G7', 0x12347fff, 0x4566ffff, 0x12347f00, 0x4566ffff, 0x34568001, 0x12347fff, 0x4566ffff],
  ['G8', 0x12347f01, 0x4566ff01, 0x12347f01, 0x4566ff01, 0x34568000, 0x12347f01, 0x4566ff01],
  ['S0', 0x12347f00, 0x4566ff00, 0x12347f01, 0x4566ff00, 0x34568000, 0x12347f00, 0x4566ff00],
  ['S1', 0x12347f01, 0x4566ff01, 0x12347f01, 0x4566ff01, 0x34568000, 0x12347f01, 0x4566ff01],
  ['H0', 0x12340101, 0x12340101, 0x12340101, 0x12340101, 0x34568000, 0x12340101, 0x12340101],
  ['H1', 0x12347f80, 0x12347f80, 0x12347f01, 0x12347f01, 0x34568000, 0x12347f7f, 0x12347f7f],
  ['H2', 0x12348080, 0x12348080, 0x12348000, 0x12348000, 0x34568001, 0x12348080, 0x12348080]
];
function parentBytes(registers, alias) {return registers[PARENTS[alias]].toString(16).padStart(8, '0').match(/../g);}
function byteValue(registers, alias) {return Number.parseInt(parentBytes(registers, alias)[BYTE_POSITIONS[alias]], 16);}
function insertByte(registers, alias, value) {
  assert.ok(Number.isInteger(value) && value >= 0 && value < 256);
  const next = [...registers], bytes = parentBytes(registers, alias); bytes[BYTE_POSITIONS[alias]] = value.toString(16).padStart(2, '0');
  next[PARENTS[alias]] = Number.parseInt(bytes.join(''), 16); return next;
}
function compareExchange(registers, destination, source, oldFlags) {
  const a = byteValue(registers, 0), d = byteValue(registers, destination), sourceValue = byteValue(registers, source);
  const result = (a - d + 256) % 256, equal = a === d;
  const signed = value => value >= 128 ? value - 256 : value, difference = signed(a) - signed(d);
  let byte = result % 256, ones = 0; for (let bit = 0; bit < 8; bit++) {ones += byte % 2; byte = Math.floor(byte / 2);}
  const flags = (oldFlags & 0x400) + 2 + Number(a < d) + (ones % 2 === 0 ? 4 : 0)
    + (a % 16 < d % 16 ? 0x10 : 0) + (result === 0 ? 0x40 : 0) + (result >= 128 ? 0x80 : 0)
    + (difference < -128 || difference > 127 ? 0x800 : 0);
  const next = insertByte(registers, equal ? destination : 0, equal ? sourceValue : d);
  assert.equal((flags & 0x40) !== 0, equal); return {registers: next, flags, equal};
}
const SOURCE_HEX = [
  '0fb0c00f94c00f92c20fb0c0eb620fb0c80f94c00f92c20fb0c8eb540fb0d00f94c00f92c20fb0d0eb460fb0d80f94c00f92c20fb0d8eb380fb0e00f94c00f92c20fb0e0eb2a0fb0e80f94c00f92c20fb0e8eb1c0fb0f00f94c00f92c20fb0f0eb0e0fb0f80f94c00f92c20fb0f8eb000f0b',
  '0fb0c10f94c00f92c20fb0c1eb620fb0c90f94c00f92c20fb0c9eb540fb0d10f94c00f92c20fb0d1eb460fb0d90f94c00f92c20fb0d9eb380fb0e10f94c00f92c20fb0e1eb2a0fb0e90f94c00f92c20fb0e9eb1c0fb0f10f94c00f92c20fb0f1eb0e0fb0f90f94c00f92c20fb0f9eb000f0b',
  '0fb0c20f94c00f92c20fb0c2eb620fb0ca0f94c00f92c20fb0caeb540fb0d20f94c00f92c20fb0d2eb460fb0da0f94c00f92c20fb0daeb380fb0e20f94c00f92c20fb0e2eb2a0fb0ea0f94c00f92c20fb0eaeb1c0fb0f20f94c00f92c20fb0f2eb0e0fb0fa0f94c00f92c20fb0faeb000f0b',
  '0fb0c30f94c00f92c20fb0c3eb620fb0cb0f94c00f92c20fb0cbeb540fb0d30f94c00f92c20fb0d3eb460fb0db0f94c00f92c20fb0dbeb380fb0e30f94c00f92c20fb0e3eb2a0fb0eb0f94c00f92c20fb0ebeb1c0fb0f30f94c00f92c20fb0f3eb0e0fb0fb0f94c00f92c20fb0fbeb000f0b',
  '0fb0c40f94c00f92c20fb0c4eb620fb0cc0f94c00f92c20fb0cceb540fb0d40f94c00f92c20fb0d4eb460fb0dc0f94c00f92c20fb0dceb380fb0e40f94c00f92c20fb0e4eb2a0fb0ec0f94c00f92c20fb0eceb1c0fb0f40f94c00f92c20fb0f4eb0e0fb0fc0f94c00f92c20fb0fceb000f0b',
  '0fb0c50f94c00f92c20fb0c5eb620fb0cd0f94c00f92c20fb0cdeb540fb0d50f94c00f92c20fb0d5eb460fb0dd0f94c00f92c20fb0ddeb380fb0e50f94c00f92c20fb0e5eb2a0fb0ed0f94c00f92c20fb0edeb1c0fb0f50f94c00f92c20fb0f5eb0e0fb0fd0f94c00f92c20fb0fdeb000f0b',
  '0fb0c60f94c00f92c20fb0c6eb620fb0ce0f94c00f92c20fb0ceeb540fb0d60f94c00f92c20fb0d6eb460fb0de0f94c00f92c20fb0deeb380fb0e60f94c00f92c20fb0e6eb2a0fb0ee0f94c00f92c20fb0eeeb1c0fb0f60f94c00f92c20fb0f6eb0e0fb0fe0f94c00f92c20fb0feeb000f0b',
  '0fb0c70f94c00f92c20fb0c7eb620fb0cf0f94c00f92c20fb0cfeb540fb0d70f94c00f92c20fb0d7eb460fb0df0f94c00f92c20fb0dfeb380fb0e70f94c00f92c20fb0e7eb2a0fb0ef0f94c00f92c20fb0efeb1c0fb0f70f94c00f92c20fb0f7eb0e0fb0ff0f94c00f92c20fb0ffeb000f0b'
];
const SOURCE_HASHES = [
  '9c9e76ce3303b57d09ebc22a06df7b98a808c9635687a5df6fb47bd9b8e0e647',
  '8c94ab03348a844399d9b6da7ad78d83713bd64caabec9150f10ba982bfe36c7',
  '451f13ecf859f91834be065509098568a53e073bf9fa39d7c4c5f3044b62581f',
  '482e4334875264e9923a7d1e47939f30a44f1d9818b9844f4d6676388f39fd97',
  '140e48954163677d767ff53b4df9aa82140c3318374f377653255e4b5a3d96fe',
  'c1a79560dea176a4fedbd3422b5df0a0eb6ac1a28250773ae8057d2ba6b580d5',
  '33660c164be84d23d376e1e0aecfe5928cf662069631aa37c37fe900d1184ffe',
  '55ac1e278a2b3efcbd2de36979f191e36cfec8fbdb10e2518cea5c4751dec91f'
];
const PAGE_HASHES = [
  '73fd80f94ab991ce0c521f0089d75dcd1340b81dcdd179b9f4762a34bb2601a4',
  'b26707c65e9e26912c4282cad3a3efcc528773eddfc651bd775d9d9c9fff9fee',
  '28ded732449f8bb32d3a02cde928dc1d2cfb807213c7de94317b4cc6b233d767',
  '92767afe57e8f312afd09357113179ef02997f216428d533a6039305f978702f',
  'e3df88fbcff3c953b00d5e72b212ad64d79ff17244b44009d6d82d36d9e5d2bf',
  'f8b7f0e2384de7d056930d0e3554d0e5c077c3453b4a35dfe0b26ddb9f79bbf5',
  '9be223aaabc308f17dbf90cbf7e6c9fb0a58ec7b8eb997bd6649b52901bddfe3',
  'dad279bb5e00b3c108aaab7f7040bafe2b45a20a3696d5181feba8f5783ba02d'
];
const sources = Array.from({length: 8}, (_, bank) => {
  const bytes = Buffer.alloc(114);
  for (let source = 0; source < 8; source++) {
    const raw = 0xc0 + source * 8 + bank; bytes.set([0x0f, 0xb0, raw, 0x0f, 0x94, 0xc0, 0x0f, 0x92, 0xc2, 0x0f, 0xb0, raw, 0xeb, 112 - (source + 1) * 14], source * 14);
  }
  bytes.set([0x0f, 0x0b], BODY); assert.deepEqual(bytes, Buffer.from(SOURCE_HEX[bank], 'hex')); assert.equal(hash(bytes), SOURCE_HASHES[bank]); assert.deepEqual(readFileSync(join(output, `bank-${bank}.x86`)), bytes); return bytes;
});
const currentSource = Buffer.from(sources[0]); currentSource[16] = 0xd9;
assert.equal(hash(currentSource), 'd449a2abe5d42eda1f010331fbda1661efb6f36bebf9717b42bd45d427ad5745');
assert.ok(BASIS.every(flags => (flags & ~0xcd7) === 0 && (flags & 2) !== 0));
const expectedCounts = {contexts: 24, modules: 26, seeds: 888, main_seeds: 768, literal_seeds: 120, pair_chains: 888,
  first_cmpxchgs: 888, second_cmpxchgs: 888, cmpxchgs: 1776, first_equal: 492, first_unequal: 396, second_equal: 150, second_unequal: 738,
  setz: 888, setb: 888, jumps: 888, literal_first_anchor_checks: 120, literal_second_anchor_checks: 120,
  positive_retirements: 4440, generated_calls: 4642, preflight: 168, owner_controls: 68, same_byte_invalidations: 16,
  consumed_modrm_invalidations: 2, current_continuations: 2, data_only_controls: 2, maps: 32, host_uploads: 52,
  host_uploaded_bytes: 67380, standalone_page_inputs: 16, pages: 128, engine_page_rows: 96, standalone_page_rows: 32,
  diagnostic_read32_calls: 98304, arena_checks: 207194};
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
  const ctx = {owner, bank, expected, codePage, dataPage: Buffer.from(pattern), low: ordinal, high: 0xc10b0000};
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
  assert.deepEqual(registers, [0x12347f02, 0x23458102, ...VECTORS[0].slice(2)]); assert.equal(ctx.codePage[16], 0xc8); assert.equal(ctx.dataPage[7], 8);
  const data = {...tag(ctx), address: ctx.dataAddress + 7, old_byte: 8, new_byte: 0xa5, upload_bytes: 1, before: cpuEvidence(ctx), identity: identity(ctx, unit), module: physical(ctx, unit)};
  upload(ctx, data.address, Buffer.from([0xa5]), 'data_only'); ctx.dataPage[7] = 0xa5;
  pure(ctx, () => guard(ctx, unit), 'correct owner after unrelated data upload'); counts.owner_controls++; counts.data_only_controls++;
  data.after = cpuEvidence(ctx); assert.deepEqual(data.after, data.before); assert.deepEqual(identity(ctx, unit), data.identity); assert.deepEqual(physical(ctx, unit), data.module);
  data.guard_status = 0; data.page_sha256 = hash(ctx.dataPage); assert.equal(data.page_sha256, '1c0e57497f7f02759fdd13d0ce4c2187808a9da9b0f22639eac1430002d7938d'); dataControls.push(data);
  const row = {...tag(ctx), address: PC + 16, byte_offset: 16, old_byte: 0xc8, new_byte: 0xd9, upload_bytes: 1,
    eip_before: 0x1011, flags, before: cpuEvidence(ctx), old_identity: identity(ctx, unit), old_module: physical(ctx, unit)};
  upload(ctx, row.address, Buffer.from([0xd9]), 'consumed_modrm'); ctx.codePage[16] = 0xd9;
  assert.deepEqual(ctx.codePage.subarray(0, 114), currentSource); assert.equal(hash(ctx.codePage), 'c2d7101781aca65e9a2e15f382e6aff5030cbf01244fc825969c8eac87774338');
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
function literalInitial(row) {let registers = [...VECTORS[0]]; for (const [alias, value] of row[3]) registers = insertByte(registers, alias, value); return registers;}
assert.equal(ANCHORS.length, 20); assert.deepEqual(PARENT_ANCHORS.map(row => row[0]), ANCHORS.map(row => row[0]));
function literalFull(row, stage) {
  const expected = literalInitial(row), parent = PARENTS[row[1]], p = PARENT_ANCHORS.find(candidate => candidate[0] === row[0]);
  expected[0] = p[1]; expected[parent] = p[2];
  if (stage !== 'first') expected[0] = p[3];
  if (stage === 'current' || stage === 'second') {expected[2] = p[5]; expected[parent] = p[4];}
  if (stage === 'second') {expected[0] = p[6]; expected[parent] = p[7];}
  return expected;
}
function anchor(registers, flags, row, initialFlags, second) {
  assert.deepEqual(registers, literalFull(row, second ? 'second' : 'first'), 'literal full implicit AL/parent preservation/current second state');
  assert.deepEqual([byteValue(registers, 0), byteValue(registers, row[1])], second ? row.slice(10, 12) : row.slice(4, 6), 'literal target byte lanes');
  assert.equal(flags, row[second ? 12 : 6] + (initialFlags & 0x400));
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
  run(ctx, unit, 1, 'first CMPXCHG8 full current implicit AL and captured byte parents', registers, entry + 3, flags);
  counts.first_cmpxchgs++; counts.cmpxchgs++; counts.positive_retirements++; observeBranch(literal, 'first', first.equal);
  if (literal) {
    anchor(registers, flags, literal, initialFlags, false);
    if (['A0', 'E1', 'G8', 'S0', 'S1', 'H0'].includes(literal[0])) assert.deepEqual(registers, initial, 'fixed GPR first target still updates FLAGS and retires');
  }
  if (mutation) unit = mutateConsumed(ctx, unit, registers, flags);
  registers = insertByte(registers, 0, Number((flags & 0x40) !== 0)); run(ctx, unit, 1, 'live SETZ AL full CPU', registers, entry + 6, flags); counts.setz++; counts.positive_retirements++;
  if (literal) assert.deepEqual(registers, literalFull(literal, 'setz'), 'literal SETZ-only full parent state before SETB');
  if (ctx.pendingMutation) {
    assert.equal(entry, 0x100e); assert.deepEqual(registers, [0x12347f01, 0x23458102, ...VECTORS[0].slice(2)]); assert.equal(flags, 0x446);
    ctx.pendingMutation.eip_after = entry + 6; ctx.pendingMutation.retired = 1; ctx.pendingMutation.after = cpuEvidence(ctx); mutations.push(ctx.pendingMutation); delete ctx.pendingMutation; counts.current_continuations++;
  }
  registers = insertByte(registers, 2, flags % 2); run(ctx, unit, 1, 'live SETB DL full CPU', registers, entry + 9, flags); counts.setb++; counts.positive_retirements++;
  if (literal) {
    assert.deepEqual(registers, literalFull(literal, 'current'), 'literal full current parent state after both consumers');
    assert.deepEqual([byteValue(registers, 0), byteValue(registers, ctx.bank), byteValue(registers, source)], literal.slice(7, 10), 'literal current AL/destination/source byte lanes');
  }
  const beforeSecond = state(registers, entry + 9, flags), second = compareExchange(registers, ctx.bank, source, flags); ({registers, flags} = second);
  run(ctx, unit, 1, 'second CMPXCHG8 from current AL and both current byte parents', registers, entry + 12, flags);
  counts.second_cmpxchgs++; counts.cmpxchgs++; counts.positive_retirements++; observeBranch(literal, 'second', second.equal);
  transitions[`${first.equal ? 'equal' : 'unequal'}_${second.equal ? 'equal' : 'unequal'}`]++;
  if (literal) {
    anchor(registers, flags, literal, initialFlags, true);
    if (['A0', 'E1', 'G8', 'S1', 'H0'].includes(literal[0])) assert.deepEqual(state(registers, entry + 9, flags), beforeSecond, 'fixed current second target retains GPRs and FLAGS while retiring');
  }
  run(ctx, unit, 1, 'following short JMP', registers, COLD, flags); counts.jumps++; counts.positive_retirements++; return unit;
}
function numeric(ctx, initialUnit) {
  let unit = initialUnit;
  for (const [index, vector] of VECTORS.entries()) for (const flags of BASIS) for (let source = 0; source < 8; source++) {
    const registers = index === 1 ? insertByte(vector, 0, byteValue(vector, ctx.bank)) : [...vector];
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
assert.deepEqual(branchRows, {main: {first_equal: 432, first_unequal: 336, second_equal: 96, second_unequal: 672}, literal: {first_equal: 60, first_unequal: 60, second_equal: 54, second_unequal: 66}});
assert.deepEqual(transitions, {equal_equal: 138, equal_unequal: 354, unequal_equal: 12, unequal_unequal: 384});
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
const result = {status: 'ok', engine_sha256: engineSha256, engine_bytes: engineBytes.length, tools, counts, contexts, modules, pages: pageRows, data_only_controls: dataControls,
  consumed_modrm_mutations: mutations, code_invalidations: invalidations, host_inputs: hostInputs,
  source: {provenance: 'existing physically pinned local Intel primary; no fresh download or newest-edition claim', order: '253666-093US', edition: 'September 2026',
    pdf_path: 'target/p2-memory-binary-spec/253666-093-sdm-vol-2a.pdf', pdf_bytes: 3404694, pdf_sha256: '87c5acb6f27e24d9d364841a0d2346c91a8482e36f403409954e2d2a8c817bac',
    full_text_path: 'target/p2-memory-binary-spec/253666-093-sdm-vol-2a.txt', full_text_bytes: 1557075, full_text_sha256: 'fa7f27a4b5f830e007fa6a967e53436aa7051fe99d8d56303efcf4148b0a5eaf',
    chapter_sha256: 'ffa52e2a5725c967769683fc49278ea3de07f8a40935c82565e24d8ede183fa1', chapter_bytes: 4590, chapter_lines: [15502, 15590],
    chapter_separator_policy: 'includes internal PDF PAGE 313 and excludes surrounding separators', pdf_pages_zero_based: [311, 312], printed_pages: ['3-194', '3-195'],
    page_bytes: [2578, 1999], page_lines: [[15502, 15557], [15559, 15590]], page_separator_policy: 'exclude surrounding PDF PAGE separators',
    page_sha256: ['fa0acf7389b46652ab63f2009a323b40a4d8f1066b84da681732db85a68e01d3', '4d7ce3ae41ca41b6b47049fbff718e852b3ccb8c016cee7a145d3939c7a48def'],
    comparison_chapter: {lines: [13835, 13920], bytes: 4498, sha256: '0fbcc28b5a7b213891673a0a87a790ae996cb9200c1209631934528a5f457d34', pdf_pages_zero_based: [278, 279], printed_pages: ['3-161', '3-162']}},
  oracle: {gprs: GPRS, byte_aliases: ALIASES, parent_indices: PARENTS, byte_hex_positions: BYTE_POSITIONS, vectors: VECTORS, basis_flags: BASIS,
    ordered_pairs: ALIASES.flatMap((destination, bank) => ALIASES.map((source, index) => ({destination, source, modrm: 0xc0 + 8 * index + bank}))),
    equality_setup: 'V0 is unchanged; V1 replaces only initial AL with that bank original destination byte, preserving the other 24 EAX bits and all other GPRs',
    literal_anchor_fields: ['name', 'bank', 'source', 'initial_byte_overrides', 'first_al', 'first_destination', 'first_flags', 'current_al', 'current_destination', 'current_source', 'second_al', 'second_destination', 'second_flags'], literal_anchors: ANCHORS, literal_anchor_count: 40,
    literal_parent_fields: ['name', 'first_eax', 'first_destination_parent', 'current_eax', 'current_destination_parent', 'current_second_edx', 'second_eax', 'second_destination_parent'], literal_parent_anchors: PARENT_ANCHORS,
    branch_counts: branchRows, chain_transitions: transitions,
    limits: {instructions: 64, blocks: 8, wasm_bytes: 65536}, bank: {specs: Array.from({length: 8}, (_, source) => [PC + source * 14, 14]), row_offsets: [0, 14, 28, 42, 56, 70, 84, 98],
      step_next_offsets: [3, 6, 9, 12], instructions: 40, body_bytes: BODY, source_bytes: 114, cold_target: COLD, jump_displacements: [98, 84, 70, 56, 42, 28, 14, 0]},
    programs: sources.map((bytes, bank) => ({bank, hex: bytes.toString('hex'), sha256: hash(bytes), code_page_sha256: PAGE_HASHES[bank]})),
    current_bank_0: {hex: currentSource.toString('hex'), sha256: hash(currentSource), code_page_sha256: 'c2d7101781aca65e9a2e15f382e6aff5030cbf01244fc825969c8eac87774338'},
    arena_bytes: SIZE, pattern_sha256: hash(pattern), changed_data_sha256: '1c0e57497f7f02759fdd13d0ce4c2187808a9da9b0f22639eac1430002d7938d',
    classifications: {main_self_chains: 96, main_sibling_chains: 96, main_different_parent_chains: 576, literal_self_chains: 18, literal_sibling_chains: 18, literal_different_parent_chains: 84,
      self_targets: 228, sibling_targets: 228, different_parent_targets: 1320},
    fixed_points: {gpr_fixed_first_observations: 36, full_gpr_flags_fixed_second_observations: 30},
    policy: 'current old AL minus old destination byte with independent numeric modulo-256 wrapping, unsigned borrow, signed-byte numeric-range overflow, nibble borrow and eight-bit division parity; captured old source/destination before a single partial-parent insertion; four hex byte strings preserve all other parent bits including AL/AH siblings; current AL/destination/source after both live SETZ AL and SETB DL consumers, no intermediate CPU repair; destination AL always equals; changed consumed C8 to D9 stays behind EIP 1011, fresh SETZ retains full ECX 23458102 rather than replaying the changed equality insertion of old BL 60 into CL',
    diagnostic_helper_policy: 'guest modules import no data helpers and retain R3MHv1; separate complete page Read32 diagnostics change only helper v1 success value with length/reserved zero, with before/after whole-arena checks'},
  artifact_census: {x86_banks: 8, modules: 26, result_files: 1, total: 35, names: artifactNames},
  test_sha256: Object.fromEntries(['engine/tests/cpu_cmpxchg8.rs', 'engine/tests/cpu_cmpxchg8_wasm.rs', 'engine/tests/fixtures/p2-cmpxchg8/run.mjs', 'engine/tests/fixtures/support/engine.mjs'].map(path => [path, hash(readFileSync(join(root, path)))])),
  claim: 'prefix-free flat32 register-only CMPXCHG8 through native-generated standalone and actual engine-Wasm replacement/resident modules: all 64 ordered byte pairs including eight self and eight same-parent siblings, V0 inequality for non-AL destinations and initial-AL-adjusted V1 equality, complementary FLAGS 2/cd7 plus twenty literal chains; independent current AL-minus-destination subtraction and hex-parent insertion oracle with 40 literal full target anchors, defined comparison flags with DF/fixed2 retained and incoming CF/ZF ignored, full CPU before SETZ AL/SETB DL and the current second target, implicit AL/source/AH/self/fixed cases; 888 seeds/1776 CMPXCHG8/888 SETZ/888 SETB/888 JMP/4642 calls with exact observed branches/transitions, pure ABI/default caps/4236-byte arenas/128 current complete pages, two unrelated data-validity controls, two consumed-ModRM stale owners and fresh same-CPU 1011 continuations retaining ECX 23458102 without replay, sixteen same-opcode stale4/closed5 controls with both complete post-stale physical module comparisons before fresh compilation or close; no inequality for destination AL, exhaustive byte-state/all-FLAGS Cartesian, memory/atomic/LOCK/word/prefix/x64/REX/RTM/guest SMC/new helpers-ABI-local-caps/PE/Windows/SDK/browser/performance/fullCI/game claim'};

const resultBytes = Buffer.from(JSON.stringify(result, null, 2)); writeFileSync(join(output, 'result.json'), resultBytes, {flag: 'wx'});
assert.deepEqual(readdirSync(output).sort(), [...artifactNames].sort(), 'exact saved artifact filenames');
console.log(JSON.stringify({status: result.status, engine_sha256: result.engine_sha256, result_sha256: hash(resultBytes), counts, output}));
