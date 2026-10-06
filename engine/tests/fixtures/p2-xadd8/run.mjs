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
  ['S0', 0, 0, [[0, 0]], 0, 0, 0x46, 0, 0, 0, 0, 0x46],
  ['S1', 0, 0, [[0, 1]], 2, 2, 2, 0, 0, 0, 0, 0x46],
  ['S2', 0, 0, [[0, 0x7f]], 0xfe, 0xfe, 0x892, 0, 0, 0, 0, 0x46],
  ['S3', 0, 0, [[0, 0x80]], 0, 0, 0x847, 1, 1, 2, 2, 2],
  ['S4', 0, 0, [[0, 0xff]], 0xfe, 0xfe, 0x93, 1, 1, 2, 2, 2],
  ['L0', 0, 4, [[0, 0x7f], [4, 1]], 0x80, 0x7f, 0x892, 0, 0x7f, 0x7f, 0, 2],
  ['L1', 0, 4, [[0, 0xff], [4, 0x80]], 0x7f, 0xff, 0x803, 1, 0xff, 0, 1, 0x57],
  ['D0', 0, 1, [[0, 0], [1, 0]], 0, 0, 0x46, 0, 0, 0, 0, 0x46],
  ['D1', 0, 1, [[0, 0xff], [1, 1]], 0, 0xff, 0x57, 1, 0xff, 0, 1, 0x57],
  ['D2', 0, 1, [[0, 0x7f], [1, 1]], 0x80, 0x7f, 0x892, 0, 0x7f, 0x7f, 0, 2],
  ['D3', 0, 1, [[0, 0x80], [1, 0x80]], 0, 0x80, 0x847, 1, 0x80, 0x81, 1, 0x86],
  ['D4', 0, 1, [[0, 0x0f], [1, 1]], 0x10, 0x0f, 0x12, 0, 0x0f, 0x0f, 0, 6],
  ['D5', 0, 1, [[0, 1], [1, 2]], 3, 1, 6, 0, 1, 1, 0, 2],
  ['P0', 2, 1, [[2, 0], [1, 0x81]], 0x81, 0, 0x86, 0, 0, 0, 0, 0x46],
  ['P1', 2, 1, [[2, 0x7f], [1, 1]], 0x80, 0x7f, 0x892, 1, 0x7f, 0x80, 1, 0x892],
  ['P2', 2, 1, [[2, 0xff], [1, 1]], 0, 0xff, 0x57, 0, 0xff, 0xff, 0, 0x86],
  ['T0', 3, 3, [[3, 0]], 0, 0, 0x46, 0, 0, 0, 0, 0x46],
  ['T1', 3, 3, [[3, 1]], 2, 2, 2, 2, 2, 4, 4, 2],
  ['H0', 4, 0, [[4, 0x7f], [0, 1]], 0x80, 0x7f, 0x892, 0x80, 0, 0x80, 0x80, 0x82],
  ['H1', 4, 0, [[4, 0xff], [0, 0x80]], 0x7f, 0xff, 0x803, 0x7f, 1, 0x80, 0x7f, 0x892]
];
const PARENT_ANCHORS = [
  ['S0', [0x12347f00, 0x23458120, 0x34568040, 0x4566ff60], [0x12347f00, 0x23458120, 0x34568000, 0x4566ff60], [0x12347f00, 0x23458120, 0x34568000, 0x4566ff60]],
  ['S1', [0x12347f02, 0x23458120, 0x34568040, 0x4566ff60], [0x12347f00, 0x23458120, 0x34568000, 0x4566ff60], [0x12347f00, 0x23458120, 0x34568000, 0x4566ff60]],
  ['S2', [0x12347ffe, 0x23458120, 0x34568040, 0x4566ff60], [0x12347f00, 0x23458120, 0x34568001, 0x4566ff60], [0x12347f00, 0x23458120, 0x34568001, 0x4566ff60]],
  ['S3', [0x12347f00, 0x23458120, 0x34568040, 0x4566ff60], [0x12347f01, 0x23458120, 0x34568001, 0x4566ff60], [0x12347f02, 0x23458120, 0x34568001, 0x4566ff60]],
  ['S4', [0x12347ffe, 0x23458120, 0x34568040, 0x4566ff60], [0x12347f01, 0x23458120, 0x34568000, 0x4566ff60], [0x12347f02, 0x23458120, 0x34568000, 0x4566ff60]],
  ['L0', [0x12347f80, 0x23458120, 0x34568040, 0x4566ff60], [0x12347f00, 0x23458120, 0x34568001, 0x4566ff60], [0x1234007f, 0x23458120, 0x34568001, 0x4566ff60]],
  ['L1', [0x1234ff7f, 0x23458120, 0x34568040, 0x4566ff60], [0x1234ff01, 0x23458120, 0x34568001, 0x4566ff60], [0x12340100, 0x23458120, 0x34568001, 0x4566ff60]],
  ['D0', [0x12347f00, 0x23458100, 0x34568040, 0x4566ff60], [0x12347f00, 0x23458100, 0x34568000, 0x4566ff60], [0x12347f00, 0x23458100, 0x34568000, 0x4566ff60]],
  ['D1', [0x12347f00, 0x234581ff, 0x34568040, 0x4566ff60], [0x12347f01, 0x234581ff, 0x34568000, 0x4566ff60], [0x12347f00, 0x23458101, 0x34568000, 0x4566ff60]],
  ['D2', [0x12347f80, 0x2345817f, 0x34568040, 0x4566ff60], [0x12347f00, 0x2345817f, 0x34568001, 0x4566ff60], [0x12347f7f, 0x23458100, 0x34568001, 0x4566ff60]],
  ['D3', [0x12347f00, 0x23458180, 0x34568040, 0x4566ff60], [0x12347f01, 0x23458180, 0x34568001, 0x4566ff60], [0x12347f81, 0x23458101, 0x34568001, 0x4566ff60]],
  ['D4', [0x12347f10, 0x2345810f, 0x34568040, 0x4566ff60], [0x12347f00, 0x2345810f, 0x34568000, 0x4566ff60], [0x12347f0f, 0x23458100, 0x34568000, 0x4566ff60]],
  ['D5', [0x12347f03, 0x23458101, 0x34568040, 0x4566ff60], [0x12347f00, 0x23458101, 0x34568000, 0x4566ff60], [0x12347f01, 0x23458100, 0x34568000, 0x4566ff60]],
  ['P0', [0x12347f02, 0x23458100, 0x34568081, 0x4566ff60], [0x12347f00, 0x23458100, 0x34568000, 0x4566ff60], [0x12347f00, 0x23458100, 0x34568000, 0x4566ff60]],
  ['P1', [0x12347f02, 0x2345817f, 0x34568080, 0x4566ff60], [0x12347f00, 0x2345817f, 0x34568001, 0x4566ff60], [0x12347f00, 0x23458101, 0x34568080, 0x4566ff60]],
  ['P2', [0x12347f02, 0x234581ff, 0x34568000, 0x4566ff60], [0x12347f01, 0x234581ff, 0x34568000, 0x4566ff60], [0x12347f01, 0x23458100, 0x345680ff, 0x4566ff60]],
  ['T0', [0x12347f02, 0x23458120, 0x34568040, 0x4566ff00], [0x12347f00, 0x23458120, 0x34568000, 0x4566ff00], [0x12347f00, 0x23458120, 0x34568000, 0x4566ff00]],
  ['T1', [0x12347f02, 0x23458120, 0x34568040, 0x4566ff02], [0x12347f00, 0x23458120, 0x34568000, 0x4566ff02], [0x12347f00, 0x23458120, 0x34568000, 0x4566ff04]],
  ['H0', [0x1234807f, 0x23458120, 0x34568040, 0x4566ff60], [0x12348000, 0x23458120, 0x34568001, 0x4566ff60], [0x12348080, 0x23458120, 0x34568001, 0x4566ff60]],
  ['H1', [0x12347fff, 0x23458120, 0x34568040, 0x4566ff60], [0x12347f01, 0x23458120, 0x34568001, 0x4566ff60], [0x1234807f, 0x23458120, 0x34568001, 0x4566ff60]]
];
function parentBytes(registers, alias) {return registers[PARENTS[alias]].toString(16).padStart(8, '0').match(/../g);}
function byteValue(registers, alias) {return Number.parseInt(parentBytes(registers, alias)[BYTE_POSITIONS[alias]], 16);}
function insertByte(registers, alias, value) {
  assert.ok(Number.isInteger(value) && value >= 0 && value < 256);
  const next = [...registers], bytes = parentBytes(registers, alias); bytes[BYTE_POSITIONS[alias]] = value.toString(16).padStart(2, '0');
  next[PARENTS[alias]] = Number.parseInt(bytes.join(''), 16); return next;
}
function add(registers, destination, source, oldFlags) {
  const a = byteValue(registers, destination), b = byteValue(registers, source), wide = a + b, result = wide % 256;
  const signed = value => value >= 128 ? value - 256 : value, sum = signed(a) + signed(b);
  let byte = result, ones = 0; for (let bit = 0; bit < 8; bit++) {ones += byte % 2; byte = Math.floor(byte / 2);}
  const flags = (oldFlags & 0x400) + 2 + Number(wide > 255) + (ones % 2 === 0 ? 4 : 0)
    + (a % 16 + b % 16 > 15 ? 0x10 : 0) + (result === 0 ? 0x40 : 0) + (result >= 128 ? 0x80 : 0)
    + (sum < -128 || sum > 127 ? 0x800 : 0);
  return {registers: insertByte(insertByte(registers, source, a), destination, result), flags};
}
const SOURCE_HEX = [
  '0fc0c00f92c00f90c20fc0c0eb620fc0c80f92c00f90c20fc0c8eb540fc0d00f92c00f90c20fc0d0eb460fc0d80f92c00f90c20fc0d8eb380fc0e00f92c00f90c20fc0e0eb2a0fc0e80f92c00f90c20fc0e8eb1c0fc0f00f92c00f90c20fc0f0eb0e0fc0f80f92c00f90c20fc0f8eb000f0b',
  '0fc0c10f92c00f90c20fc0c1eb620fc0c90f92c00f90c20fc0c9eb540fc0d10f92c00f90c20fc0d1eb460fc0d90f92c00f90c20fc0d9eb380fc0e10f92c00f90c20fc0e1eb2a0fc0e90f92c00f90c20fc0e9eb1c0fc0f10f92c00f90c20fc0f1eb0e0fc0f90f92c00f90c20fc0f9eb000f0b',
  '0fc0c20f92c00f90c20fc0c2eb620fc0ca0f92c00f90c20fc0caeb540fc0d20f92c00f90c20fc0d2eb460fc0da0f92c00f90c20fc0daeb380fc0e20f92c00f90c20fc0e2eb2a0fc0ea0f92c00f90c20fc0eaeb1c0fc0f20f92c00f90c20fc0f2eb0e0fc0fa0f92c00f90c20fc0faeb000f0b',
  '0fc0c30f92c00f90c20fc0c3eb620fc0cb0f92c00f90c20fc0cbeb540fc0d30f92c00f90c20fc0d3eb460fc0db0f92c00f90c20fc0dbeb380fc0e30f92c00f90c20fc0e3eb2a0fc0eb0f92c00f90c20fc0ebeb1c0fc0f30f92c00f90c20fc0f3eb0e0fc0fb0f92c00f90c20fc0fbeb000f0b',
  '0fc0c40f92c00f90c20fc0c4eb620fc0cc0f92c00f90c20fc0cceb540fc0d40f92c00f90c20fc0d4eb460fc0dc0f92c00f90c20fc0dceb380fc0e40f92c00f90c20fc0e4eb2a0fc0ec0f92c00f90c20fc0eceb1c0fc0f40f92c00f90c20fc0f4eb0e0fc0fc0f92c00f90c20fc0fceb000f0b',
  '0fc0c50f92c00f90c20fc0c5eb620fc0cd0f92c00f90c20fc0cdeb540fc0d50f92c00f90c20fc0d5eb460fc0dd0f92c00f90c20fc0ddeb380fc0e50f92c00f90c20fc0e5eb2a0fc0ed0f92c00f90c20fc0edeb1c0fc0f50f92c00f90c20fc0f5eb0e0fc0fd0f92c00f90c20fc0fdeb000f0b',
  '0fc0c60f92c00f90c20fc0c6eb620fc0ce0f92c00f90c20fc0ceeb540fc0d60f92c00f90c20fc0d6eb460fc0de0f92c00f90c20fc0deeb380fc0e60f92c00f90c20fc0e6eb2a0fc0ee0f92c00f90c20fc0eeeb1c0fc0f60f92c00f90c20fc0f6eb0e0fc0fe0f92c00f90c20fc0feeb000f0b',
  '0fc0c70f92c00f90c20fc0c7eb620fc0cf0f92c00f90c20fc0cfeb540fc0d70f92c00f90c20fc0d7eb460fc0df0f92c00f90c20fc0dfeb380fc0e70f92c00f90c20fc0e7eb2a0fc0ef0f92c00f90c20fc0efeb1c0fc0f70f92c00f90c20fc0f7eb0e0fc0ff0f92c00f90c20fc0ffeb000f0b'
];
const SOURCE_HASHES = [
  'fe93ab54563f5b2846689c4ff40ebccd3a6288c50d8817b8a750f17fc22049d7',
  '2887871c5c4d2d332214fccef4be72988a122f38b289db5c90e07811db2b158e',
  '27f1658c0535508ef52ad30df8a876990610bf82efe5aa103c431386f12ab7b0',
  '8634fd27744674b758f84fb211b8992363e1d3a438df0dbdb9614af89c05eb22',
  '55d899f5ee6bbbdc41c29fad51398f2c5e2af7ed478d1a6a7252ca45458c72a6',
  '2b5dfd3bc857d6b96f55c16c627b84ae9f4c8b538f5b7dd69392ad989fb56349',
  '09d170b7bf10a9efd83d80c3039ddf3e8345997df713a03dcfbf827113aca77b',
  '11c2c8b11cb80a1f24027d06fb17a1987e7ffa1bfebe0679cd32752d27edd455'
];
const PAGE_HASHES = [
  'c4705037f1abbeeb207cc1ad5a0d06d5e1dd11a78c97be8b01caf0d3b98860de',
  '9479375df083574a5a0fba3bde841c0860ebc1d35b513b922e4cf60eadc903e0',
  '790871a0888cafa5d1cf495f3897cce5038206d67938e4bea12f7c366ea81358',
  'b468d7f5f371d6d926d6a8523850b493c7ba72e30a84217ec6c4d70fec70912c',
  '64ebe41601d078df52dc9df58d21c80ddcb8cb958a46e2a7bcf6ebdc2da8cf08',
  '25168af820732713b2261216ef77863736cf2d8a77435ee5914c10693331f27a',
  'be4c6f8848fb3cdc3e57ba1b3bcc12f4bfd12b81dfd0ccc24b74012b15ecb3aa',
  '2a8b6b843b647d7e51b761b7f0e631fc5888cf1f7f10c44f20df26c2473b0839'
];
const sources = Array.from({length: 8}, (_, bank) => {
  const bytes = Buffer.alloc(114);
  for (let source = 0; source < 8; source++) {
    const raw = 0xc0 + source * 8 + bank; bytes.set([0x0f, 0xc0, raw, 0x0f, 0x92, 0xc0, 0x0f, 0x90, 0xc2, 0x0f, 0xc0, raw, 0xeb, 112 - (source + 1) * 14], source * 14);
  }
  bytes.set([0x0f, 0x0b], BODY); assert.deepEqual(bytes, Buffer.from(SOURCE_HEX[bank], 'hex')); assert.equal(hash(bytes), SOURCE_HASHES[bank]); assert.deepEqual(readFileSync(join(output, `bank-${bank}.x86`)), bytes); return bytes;
});
const currentSource = Buffer.from(sources[0]); currentSource[16] = 0xd9;
assert.equal(hash(currentSource), '699eb0e35ded05d1f2a4c999ef7aaf74c9e96fafe39907a28814a4e04c14f148');
assert.ok(BASIS.every(flags => (flags & ~0xcd7) === 0 && (flags & 2) !== 0));
const expectedCounts = {contexts: 24, modules: 26, seeds: 888, main_seeds: 768, literal_seeds: 120, pair_chains: 888,
  first_xadds: 888, second_xadds: 888, xadds: 1776, setb: 888, seto: 888, jumps: 888, literal_first_anchor_checks: 120,
  literal_second_anchor_checks: 120, positive_retirements: 4440, generated_calls: 4642, preflight: 168, owner_controls: 68,
  same_byte_invalidations: 16, consumed_modrm_invalidations: 2, current_continuations: 2, data_only_controls: 2, maps: 32,
  host_uploads: 52, host_uploaded_bytes: 67380, standalone_page_inputs: 16, pages: 128, engine_page_rows: 96,
  standalone_page_rows: 32, diagnostic_read32_calls: 98304, arena_checks: 207194};
const counts = Object.fromEntries(Object.keys(expectedCounts).map(name => [name, 0]));
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
  const ctx = {owner, bank, expected, codePage, dataPage: Buffer.from(pattern), low: ordinal, high: 0xc0a80000};
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
  assert.equal(ctx.bank, 0); assert.equal(ctx.expected.readUInt32LE(48), 0x1011); assert.equal(flags, 0x406);
  assert.deepEqual(registers, [0x12347f03, 0x23458101, ...VECTORS[0].slice(2)]); assert.equal(ctx.codePage[16], 0xc8); assert.equal(ctx.dataPage[7], 8);
  const data = {...tag(ctx), address: ctx.dataAddress + 7, old_byte: 8, new_byte: 0xa5, upload_bytes: 1, before: cpuEvidence(ctx), identity: identity(ctx, unit), module: physical(ctx, unit)};
  upload(ctx, data.address, Buffer.from([0xa5]), 'data_only'); ctx.dataPage[7] = 0xa5;
  pure(ctx, () => guard(ctx, unit), 'correct owner after unrelated data upload'); counts.owner_controls++; counts.data_only_controls++;
  data.after = cpuEvidence(ctx); assert.deepEqual(data.after, data.before); assert.deepEqual(identity(ctx, unit), data.identity); assert.deepEqual(physical(ctx, unit), data.module);
  data.guard_status = 0; data.page_sha256 = hash(ctx.dataPage); assert.equal(data.page_sha256, '1c0e57497f7f02759fdd13d0ce4c2187808a9da9b0f22639eac1430002d7938d'); dataControls.push(data);
  const row = {...tag(ctx), address: PC + 16, byte_offset: 16, old_byte: 0xc8, new_byte: 0xd9, upload_bytes: 1,
    eip_before: 0x1011, flags, before: cpuEvidence(ctx), old_identity: identity(ctx, unit), old_module: physical(ctx, unit)};
  upload(ctx, row.address, Buffer.from([0xd9]), 'consumed_modrm'); ctx.codePage[16] = 0xd9;
  assert.deepEqual(ctx.codePage.subarray(0, 114), currentSource); assert.equal(hash(ctx.codePage), 'eeb5800f88d48c06fa2b4871eee1c20ebf98584360b5b677643243e0a240ee64');
  row.source_sha256 = hash(currentSource); row.code_page_sha256 = hash(ctx.codePage);
  row.replay_discriminator = {replayed_ecx: 0x23458161, replayed_ebx: 0x4566ff01, replayed_flags: 0x402, canonical_ecx: 0x23458101, canonical_ebx: 0x4566ff60}; cancel(ctx, 1);
  run(ctx, unit, 0, 'consumed ModRM stale owner before cancelled malformed pointers', registers, 0x1011, flags, 0, 0, 4, true);
  assert.deepEqual(physical(ctx, unit), row.old_module, 'whole old module retained after stale and before fresh publication');
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
function anchor(registers, flags, row, initialFlags, phase) {
  const [name, destination, source, , firstDestination, firstSource, firstFlags, currentDestination, currentSourceValue, secondDestination, secondSource, secondFlags] = row;
  const parent = PARENT_ANCHORS.find(item => item[0] === name); assert.ok(parent);
  const first = [...parent[1], ...VECTORS[0].slice(4)], current = [...parent[2], ...VECTORS[0].slice(4)], second = [...parent[3], ...VECTORS[0].slice(4)];
  const expected = phase === 'first' ? first : phase === 'second' ? second : phase === 'setb' ? [current[0], ...first.slice(1)] : current;
  assert.deepEqual(registers, expected, `literal ${name} complete ${phase} parents and unrelated GPRs`);
  assert.equal(flags, (phase === 'second' ? secondFlags : firstFlags) + (initialFlags & 0x400));
  if (phase !== 'setb') {
    const values = phase === 'first' ? [firstDestination, firstSource] : phase === 'second' ? [secondDestination, secondSource] : [currentDestination, currentSourceValue];
    assert.deepEqual([byteValue(registers, destination), byteValue(registers, source)], values, `literal ${name} simultaneous byte assignments`);
  }
  if (phase === 'first' || phase === 'second') counts[phase === 'first' ? 'literal_first_anchor_checks' : 'literal_second_anchor_checks']++;
}
function chain(ctx, initialUnit, initial, source, initialFlags, literal = null, mutation = false) {
  let unit = initialUnit, registers = [...initial], flags = initialFlags; const entry = PC + source * 14;
  seed(ctx, registers, source, flags); counts[literal ? 'literal_seeds' : 'main_seeds']++; counts.pair_chains++;
  if (!ctx.preflightDone && literal === null) {
    run(ctx, unit, 0, 'zero budget before first target', registers, entry, flags, 1, 0); counts.preflight++;
    cancel(ctx, 1); run(ctx, unit, 1, 'cancel before first target', registers, entry, flags, 2, 0); counts.preflight++; cancel(ctx, 0); ctx.preflightDone = true;
  }
  ({registers, flags} = add(registers, ctx.bank, source, flags));
  run(ctx, unit, 1, 'first XADD8 full captured source and destination', registers, entry + 3, flags); counts.first_xadds++; counts.xadds++; counts.positive_retirements++;
  if (literal) {
    anchor(registers, flags, literal, initialFlags, 'first');
    if (['S0', 'D0', 'T0'].includes(literal[0])) assert.deepEqual(registers, initial, 'fixed-value first target retains all GPRs while flags and retirement update');
  }
  if (mutation) unit = mutateConsumed(ctx, unit, registers, flags);
  registers = insertByte(registers, 0, flags % 2); run(ctx, unit, 1, 'live SETB AL full CPU', registers, entry + 6, flags); counts.setb++; counts.positive_retirements++;
  if (literal) anchor(registers, flags, literal, initialFlags, 'setb');
  if (ctx.pendingMutation) {
    assert.equal(entry, 0x100e); assert.deepEqual(registers, [0x12347f00, 0x23458101, ...VECTORS[0].slice(2)]); assert.equal(flags, 0x406);
    ctx.pendingMutation.eip_after = entry + 6; ctx.pendingMutation.retired = 1; ctx.pendingMutation.after = cpuEvidence(ctx); mutations.push(ctx.pendingMutation); delete ctx.pendingMutation; counts.current_continuations++;
  }
  registers = insertByte(registers, 2, Number((flags & 0x800) !== 0)); run(ctx, unit, 1, 'live SETO DL full CPU', registers, entry + 9, flags); counts.seto++; counts.positive_retirements++;
  if (literal) anchor(registers, flags, literal, initialFlags, 'current');
  const beforeSecond = state(registers, entry + 9, flags); ({registers, flags} = add(registers, ctx.bank, source, flags));
  run(ctx, unit, 1, 'second XADD8 from current operands after both consumers', registers, entry + 12, flags); counts.second_xadds++; counts.xadds++; counts.positive_retirements++;
  if (literal) {
    anchor(registers, flags, literal, initialFlags, 'second');
    if (['S0', 'D0', 'T0'].includes(literal[0])) assert.deepEqual(state(registers, entry + 9, flags), beforeSecond, 'fixed-value second target retains GPRs and FLAGS while retiring');
  }
  run(ctx, unit, 1, 'following short JMP', registers, COLD, flags); counts.jumps++; counts.positive_retirements++; return unit;
}
function numeric(ctx, initialUnit) {
  let unit = initialUnit;
  for (const vector of VECTORS) for (const flags of BASIS) for (let source = 0; source < 8; source++) unit = chain(ctx, unit, vector, source, flags);
  for (const literal of ANCHORS.filter(row => row[1] === ctx.bank)) for (const flags of BASIS) {
    const [, , source, overrides] = literal; let registers = [...VECTORS[0]];
    for (const [alias, value] of overrides) registers = insertByte(registers, alias, value);
    assert.ok(VECTORS.every(vector => !vector.every((value, index) => value === registers[index])), 'literal seed is disjoint from the declared main vectors');
    unit = chain(ctx, unit, registers, source, flags, literal, Boolean(ctx.api && literal[0] === 'D5' && flags === 0xcd7));
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
  assert.deepEqual(physical(ctx, unit), row.module, 'whole current module retained after same-byte stale and before close');
  row.stale_status = 4; row.after_stale = cpuEvidence(ctx); assert.deepEqual(row.after_stale, row.before); pages(ctx, 'after_samebyte');
  pure(ctx, () => ctx.api.close(), 'close'); run(ctx, unit, 0, 'closed current owner before cancelled malformed pointers', registers, COLD, flags, 0, 0, 5, true);
  counts.owner_controls++; row.closed_status = 5; row.after_closed = cpuEvidence(ctx); invalidations.push(row);
}
for (const owner of ['standalone', 'replacement', 'resident']) for (let bank = 0; bank < 8; bank++) {
  const ctx = fresh(owner, bank), unit = compile(ctx); pages(ctx, 'initial'); const current = numeric(ctx, unit); finish(ctx, current);
}
assert.equal(Object.keys(counts).length, 31); assert.deepEqual(Object.keys(counts), Object.keys(expectedCounts));
for (const [name, expected] of Object.entries(expectedCounts)) assert.equal(counts[name], expected, `exact observed ${name} census`);
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
  source: {url: 'https://cdrdv2-public.intel.com/929356/334569-093-sdm-vol-2d.pdf', order: '334569-093US', edition: 'September 2026',
    pdf_path: 'target/p2-memory-binary-spec/334569-093-sdm-vol-2d.pdf', pdf_bytes: 1814965, pdf_sha256: '17b632da847e4757448e8afc950487b5d489da6c238de4551dc8cf6f799d3dcd',
    full_text_path: 'target/p2-memory-binary-spec/334569-093-sdm-vol-2d.txt', full_text_bytes: 721198, full_text_sha256: '1bdcfb8396e09e84cb008ed0bb07e08d38ca9d30e39b44db695f3ef43508013d',
    chapter_sha256: '407080e07d2686e251c4cd2e9cee8f000278b7ed1c1b3f8303d7d63c596e6f34', chapter_bytes: 3756, chapter_lines: [979, 1048],
    chapter_separator_policy: 'includes internal PDF PAGE 31 separator and excludes surrounding PAGE 30 / next PAGE 32 separators', pdf_pages_zero_based: [29, 30], printed_pages: ['6-27', '6-28'],
    page_bytes: [2478, 1266], page_lines: [[979, 1026], [1028, 1048]], page_separator_policy: 'exclude surrounding PDF PAGE separators',
    page_sha256: ['5efe959e67b3e3a4f30eb755eeb23028e088e668022ccb098d39ecba7c8b76d7', 'dc2c95631046ef0f1b7510c7592921f719d10ed910d91f4df47b7f1c1aa805f2']},
  oracle: {gprs: GPRS, aliases: ALIASES, parents: PARENTS, byte_positions: BYTE_POSITIONS, vectors: VECTORS, basis_flags: BASIS,
    ordered_pairs: ALIASES.flatMap((destination, bank) => ALIASES.map((source, index) => ({destination, source, modrm: 0xc0 + 8 * index + bank}))),
    literal_anchor_fields: ['name', 'destination', 'source', 'initial_byte_overrides', 'first_destination', 'first_source', 'first_flags', 'current_destination', 'current_source', 'second_destination', 'second_source', 'second_flags'],
    literal_anchors: ANCHORS, parent_anchor_fields: ['name', 'first_EAX_ECX_EDX_EBX', 'current_EAX_ECX_EDX_EBX', 'second_EAX_ECX_EDX_EBX'], parent_anchors: PARENT_ANCHORS, literal_anchor_count: 40,
    literal_setb_parent_policy: 'SETB-only frame uses literal current EAX and literal first other seven GPRs; current frame then uses the full literal table after SETO DL',
    limits: {instructions: 64, blocks: 8, wasm_bytes: 65536}, bank: {specs: Array.from({length: 8}, (_, source) => [PC + source * 14, 14]), row_offsets: [0, 14, 28, 42, 56, 70, 84, 98],
      step_next_offsets: [3, 6, 9, 12], instructions: 40, body_bytes: BODY, source_bytes: 114, cold_target: COLD, jump_displacements: [98, 84, 70, 56, 42, 28, 14, 0]},
    programs: sources.map((bytes, bank) => ({bank, hex: bytes.toString('hex'), sha256: hash(bytes), code_page_sha256: PAGE_HASHES[bank]})),
    current_bank_0: {hex: currentSource.toString('hex'), sha256: hash(currentSource), code_page_sha256: 'eeb5800f88d48c06fa2b4871eee1c20ebf98584360b5b677643243e0a240ee64'},
    arena_bytes: SIZE, pattern_sha256: hash(pattern), changed_data_sha256: '1c0e57497f7f02759fdd13d0ce4c2187808a9da9b0f22639eac1430002d7938d',
    classifications: {main_self_chains: 96, main_sibling_chains: 96, main_different_parent_chains: 576, literal_self_chains: 42, literal_sibling_chains: 24, literal_different_parent_chains: 54,
      self_targets: 276, sibling_targets: 240, different_parent_targets: 1260},
    literal_flag_observations: {first_cf1: 42, first_of1: 54, second_cf1: 12, second_of1: 12},
    fixed_points: {names: ['S0', 'D0', 'T0'], full_gpr_fixed_first_observations: 18, full_gpr_flags_fixed_second_observations: 18},
    policy: 'widened unsigned byte sum and separate signed numeric-range overflow, nibble carry and eight-bit division parity; hex-byte capture/current-parent insertion, captured old destination goes to source before result goes to destination including self final priority and sibling merges; current operand capture after both live consumers with no intermediate CPU repair; changed consumed C8 to D9 stays behind EIP 1011, fresh SETB continues at 1011 without replay',
    diagnostic_helper_policy: 'guest modules import no data helpers and retain R3MHv1; separate complete page Read32 diagnostics change only helper v1 success value with length/reserved zero, with before/after whole-arena checks'},
  artifact_census: {x86_banks: 8, modules: 26, result_files: 1, total: 35, names: artifactNames},
  test_sha256: Object.fromEntries(['engine/tests/cpu_xadd8.rs', 'engine/tests/cpu_xadd8_wasm.rs', 'engine/tests/fixtures/p2-xadd8/run.mjs', 'engine/tests/fixtures/support/engine.mjs'].map(path => [path, hash(readFileSync(join(root, path)))])),
  claim: 'prefix-free flat32 register-only byte XADD through native-generated standalone and actual engine-Wasm replacement/resident modules: all 64 ordered byte aliases/eight self/eight siblings/48 different-parent pairs, two declared asymmetric vectors and complementary FLAGS 2/cd7 plus twenty literal chains; independent byte addition/source/self/current-parent consumer oracle and 40 target anchors, full CPU before both SETcc and current second XADD, defined ADD flags with DF/fixed2 retained and incoming CF ignored; 888 seeds/1776 XADD8/888 SETB/888 SETO/888 JMP, exact pure ABI/default caps/4236-byte arenas/128 current complete pages, two unrelated data-validity controls, two consumed-ModRM stale owners and fresh no-replay same-CPU 1011 continuations with whole physical module retention after stale before fresh/close, sixteen same-opcode stale4/closed5 controls before cancelled malformed arguments; no exhaustive byte-pair/FLAGS Cartesian, memory/atomic/LOCK/HLE/word/dword/prefix/x64/REX/RTM/guest SMC/new helpers-ABI-local-caps/PE/Windows/SDK/browser/performance/fullCI/game claim'};
const resultBytes = Buffer.from(JSON.stringify(result, null, 2)); writeFileSync(join(output, 'result.json'), resultBytes, {flag: 'wx'});
assert.deepEqual(readdirSync(output).sort(), [...artifactNames].sort(), 'exact saved artifact filenames');
console.log(JSON.stringify({status: result.status, engine_sha256: result.engine_sha256, result_sha256: hash(resultBytes), counts, output}));
