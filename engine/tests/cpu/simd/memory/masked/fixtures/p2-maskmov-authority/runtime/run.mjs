import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../../../../../../../support/wasm/engine.mjs';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(process.env.RING3_ENGINE_SHA256, 'select an explicit engine SHA-256');
const engine = readEngine(enginePath), initial = readFileSync(join(output, 'initial-arena.bin'));
const SIZE = 4796, PC = 0x1000, SEG = 4524, SIMD = 4364, FP = 4236, DATA = 0x5010;
const pages = [PC, 0x5000], keyHigh = 0x4d41534b, observations = [], modules = [], helperTrace = [];
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
assert.equal(initial.length, SIZE, 'complete current arena required');
assert.equal(initial.toString('ascii', 4756, 4760), 'R3SB');
assert.equal(initial.readUInt16LE(FP + 18), 0x2800, 'nonzero x87 TOP makes MMX transition observable');
const sourcePaths = [
  'engine/src/abi/arena.rs', 'engine/src/abi/cpu_helper.rs',
  'engine/src/cpu/dbt/wasm/simd.rs', 'engine/src/cpu/dbt/wasm/memory.rs',
  'engine/src/cpu/dbt/wasm/segments/access.rs', 'engine/src/cpu/dbt/wasm/segments/context.rs',
  'engine/src/cpu/x86/simd.rs', 'engine/src/cpu/x86/decode/simd.rs',
  'engine/src/process/cpu/simd.rs', 'engine/src/process/memory/span.rs', 'engine/src/memory/space.rs', 'engine/src/memory/space/atomic.rs',
  'engine/tests/support/wasm.rs', 'engine/tests/support/wasm/engine.mjs',
  'engine/tests/cpu/simd/wasm.rs', 'engine/tests/cpu/simd/memory/masked/wasm.rs',
  'engine/tests/cpu/simd/memory/masked/fixtures/p2-maskmov-authority/runtime/run.mjs',
];
const pins = () => Object.fromEntries(sourcePaths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = pins();
const words = values => {const bytes = Buffer.alloc(values.length * 4); values.forEach((v, i) => bytes.writeUInt32LE(v >>> 0, i * 4)); return bytes;};
function record(magic, size, fields = [], version = 1) {
  const bytes = Buffer.alloc(size); bytes.write(magic); bytes.writeUInt16LE(version, 4);
  bytes.writeUInt16LE(1, 6); bytes.writeUInt32LE(size, 8); bytes.set(words(fields), 16); return bytes;
}
const rows = [
  {name: 'selected-byte-above-limit', cache: 0xf230500f, address: DATA, mask: 1, reason: 11},
  {name: 'operand-crosses-limit-selected-byte-stays-inside', cache: 0xf2305010, address: DATA, mask: 1, reason: 11},
  {name: 'selected-write-to-readonly-segment', cache: 0xf31fffff, address: DATA, mask: 1, reason: 11},
  {name: 'zero-mask-null-unmapped-policy-control', cache: 0, address: 0xffffffff, mask: 0, nullDs: true, reason: 1},
  {name: 'sparse-mask-unmapped-unselected-page-policy-control', cache: 0xf33fffff, address: 0x5fff, mask: 1, reason: 1},
];
const arena = c => Buffer.from(new Uint8Array(c.memory.buffer, c.base, SIZE));
const input = (c, offset, bytes) => new Uint8Array(c.memory.buffer).set(bytes, c.base + offset);
function host(c, name, args = []) {assert.equal(c.api[name](...args), 0, name);}
function upload(c, address, bytes) {input(c, 140, bytes); host(c, 'upload', [address, bytes.length]);}
function ram(c) {
  const saved = arena(c), bytes = Buffer.alloc(pages.length * 4096);
  pages.forEach((page, index) => {
    for (let offset = 0; offset < 4096; offset += 4) {
      host(c, 'read32', [page + offset]);
      const packet = new DataView(c.memory.buffer, c.base + 100, 40);
      assert.equal(packet.getUint32(16, true), 0, 'mapped RAM snapshot read succeeds');
      bytes.writeUInt32LE(packet.getUint32(20, true), index * 4096 + offset);
    }
  });
  const observed = arena(c), allowed = Buffer.from(saved); allowed.set(observed.subarray(100, 140), 100);
  assert.deepEqual(observed, allowed, 'mapped RAM inspection changes only the helper packet');
  input(c, 100, saved.subarray(100, 140)); return bytes;
}
function open(owner, entries, mmx, row) {
  const instance = new WebAssembly.Instance(engine.module, {});
  const c = {id: modules.length + 1, owner, entries, mmx, row, memory: instance.exports.memory, api: {}, calls: []};
  for (const [name, value] of Object.entries(instance.exports))
    if (name.startsWith('ring3_abi_v1_') && typeof value === 'function') c.api[name.slice(13)] = value;
  host(c, 'open', [4, c.id, keyHigh]); c.base = c.api.arena_ptr() >>> 0; input(c, 0, initial);
  host(c, 'map', [PC, 1, 7]); host(c, 'map', [0x5000, 1, 3]);
  c.instruction = Buffer.from(mmx ? '0ff7c1' : '660ff7c1', 'hex');
  const code = Buffer.concat([c.instruction, Buffer.from('ebfe', 'hex')]); upload(c, PC, code);
  input(c, 140, words(entries ? [PC] : [PC, code.length]));
  const compile = owner === 'resident' ? entries ? 'compile_resident_entries' : 'compile_resident' : entries ? 'compile_entries' : 'compile';
  host(c, compile, [1, ...(entries ? [0] : [])]);
  const compiled = arena(c), resident = owner === 'resident';
  c.binding = resident ? [c.id, keyHigh, compiled.readUInt32LE(148), compiled.readUInt32LE(152)] : [];
  const pointer = resident ? compiled.readUInt32LE(156) : c.api.module_ptr() >>> 0;
  const length = resident ? compiled.readUInt32LE(160) : c.api.module_len() >>> 0;
  const bytes = Buffer.from(new Uint8Array(c.memory.buffer, pointer, length)), module = new WebAssembly.Module(bytes), ring3 = {...c.api};
  for (const item of WebAssembly.Module.imports(module).filter(item => item.module === 'ring3' && !item.name.startsWith('guard') && item.name !== 'finish_exception_detection')) {
    ring3[item.name] = (...args) => {
      const before = arena(c), status = c.api[item.name](...args), after = arena(c), allowed = Buffer.from(before);
      allowed.set(after.subarray(100, 140), 100);
      if (item.name.startsWith('simd_execute')) {
        allowed.set(after.subarray(140, 156), 140); allowed.set(after.subarray(652, 660), 652);
      }
      assert.deepEqual(after, allowed, item.name + ' restricted helper publication');
      const call = {name: item.name, args: args.map(value => value >>> 0), status, packet: after.subarray(100, 140).toString('hex')};
      c.calls.push(call); helperTrace.push({context: c.id, ...call}); return status;
    };
  }
  c.run = new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3}).exports.run;
  const file = `module-${c.id}.wasm`; writeFileSync(join(output, file), bytes, {flag: 'wx'});
  modules.push({context: c.id, owner, entries, form: mmx ? 'MASKMOVQ' : 'MASKMOVDQU', row: row.name, file, bytes: bytes.length, sha256: hash(bytes), imports: WebAssembly.Module.imports(module)});
  input(c, 0, initial);
  c.registers = [0x12345678, 0x23456789, 0x3456789a, 0x456789ab, 0x7080, 0x56789abc, 0x6789abcd, row.address];
  input(c, 0, record('R3ST', 56, [...c.registers, PC, 0xcd7]));
  const segments = record('R3SG', 64, [], 2);
  [0x23, 0x1b, 0x23, row.nullDs ? 0 : 0x23, 0x3b, 0].forEach((value, i) => segments.writeUInt16LE(value, 16 + i * 2));
  [0xf33fffff, 0xf3bfffff, 0xf33fffff, row.cache, 0xf33fffff, 0]
    .forEach((value, i) => segments.writeUInt32LE(value >>> 0, [28, 32, 36, 40, 44, 56][i]));
  input(c, SEG, segments);
  c.width = mmx ? 8 : 16;
  const data = Buffer.alloc(c.width, 0x51), mask = Buffer.alloc(c.width);
  for (let i = 0; i < mask.length; i++) if (row.mask & (1 << i)) mask[i] = 0x80;
  input(c, mmx ? FP + 40 : SIMD + 32, data); input(c, mmx ? FP + 50 : SIMD + 48, mask);
  upload(c, 0x5000, Buffer.alloc(32, 0x66)); upload(c, 0x5ff0, Buffer.alloc(16, 0x66)); return c;
}
function run(c) {
  const {row, mmx, owner, entries} = c, retired = Number(row.reason === 1);
  const name = `${mmx ? 'maskmovq' : 'maskmovdqu'}-${row.name}`, before = arena(c), beforeRam = ram(c);
  const expected = Buffer.from(before), expectedRam = Buffer.from(beforeRam), at = c.calls.length;
  expected.set(record('R3ST', 56, [...c.registers, PC + retired * c.instruction.length, 0xcd7]));
  expected.set(record('R3EX', 40, [row.reason, retired, 0, 0, 0, 0], 6), 56);
  expected.set(record('R3MH', 40, [0, 0, 0, 0, 0, retired ? c.width : 0], 7), 100);
  const stagedData = Buffer.alloc(16); stagedData.fill(0x51, 0, c.width);
  expected.set(stagedData, 140); expected.set(words([row.mask, 0xcd7]), 652);
  if (retired && mmx) {expected.writeUInt16LE(0, FP + 20); expected.writeUInt16LE(0, FP + 18);}
  if (retired && row.mask) expectedRam[4096 + row.address - 0x5000] = 0x51;
  const status = c.run(c.base, c.base + 56, 1, c.base + 96), after = arena(c), afterRam = ram(c);
  const actualCalls = c.calls.slice(at), errors = [];
  const check = (description, assertion) => {try {assertion();} catch {errors.push(description);}};
  check('run status', () => assert.equal(status, 0));
  check('complete arena', () => assert.deepEqual(after, expected));
  check('every mapped RAM byte', () => assert.deepEqual(afterRam, expectedRam));
  check('helper count', () => assert.equal(actualCalls.length, retired ? 2 : 1));
  const executeName = owner === 'resident' ? 'simd_execute_resident' : 'simd_execute';
  check('SIMD execute helper', () => assert.equal(actualCalls[0]?.name, executeName));
  check('SIMD execute operands', () => assert.deepEqual(actualCalls[0]?.args, [...c.binding, mmx ? 132109 : 173069, mmx ? 256 : 2312, 0, 0, 0, 0xcd7]));
  check('SIMD execute packet', () => assert.equal(actualCalls[0]?.packet, record('R3MH', 40, [0, 0, 0, 0, 0, 0], 7).toString('hex')));
  check('helper success', () => assert.ok(actualCalls.every(call => call.status === 0)));
  if (!retired) {
    check('GP before masked RAM helper', () => assert.ok(!actualCalls.some(call => call.name.startsWith('cpu_mask_write'))));
    check('fault preserves complete x87/MMX state', () => assert.deepEqual(after.subarray(FP, FP + 128), before.subarray(FP, FP + 128)));
  } else {
    check('masked write helper', () => assert.equal(actualCalls[1]?.name, owner === 'resident' ? 'cpu_mask_write_resident' : 'cpu_mask_write'));
    check('masked write operands', () => assert.deepEqual(actualCalls[1]?.args, [...c.binding, row.address, c.width, row.mask]));
    check('masked write packet', () => assert.equal(actualCalls[1]?.packet, record('R3MH', 40, [0, 0, 0, 0, 0, c.width], 7).toString('hex')));
    if (mmx) check('successful MMX transition', () => {assert.equal(after.readUInt16LE(FP + 20), 0); assert.equal(after.readUInt16LE(FP + 18), 0);});
  }
  const ordinal = observations.length + 1, arenaFile = `arena-${ordinal}.bin`, ramFile = `ram-${ordinal}.bin`;
  writeFileSync(join(output, arenaFile), after, {flag: 'wx'}); writeFileSync(join(output, ramFile), afterRam, {flag: 'wx'});
  observations.push({context: c.id, owner, entries, name, passed: errors.length === 0, errors,
    expected: {reason: row.reason, retired, pc: PC + retired * c.instruction.length},
    actual: {status, reason: after.readUInt32LE(72), retired: after.readUInt32LE(76), pc: after.readUInt32LE(48)}, helpers: actualCalls,
    arena: {file: arenaFile, bytes: after.length, sha256: hash(after)}, ram: {file: ramFile, pages, bytes: afterRam.length, sha256: hash(afterRam)}});
}
for (const owner of ['replacement', 'resident']) for (const entries of [false, true])
  for (const mmx of [false, true]) for (const row of rows) {
    const c = open(owner, entries, mmx, row); run(c); host(c, 'close');
  }
assert.equal(observations.length, 40); assert.equal(modules.length, 40);
assert.deepEqual(pins(), sourcePins, 'MASKMOV authority source and literal oracles stayed fixed');
const failed = observations.filter(row => !row.passed), counts = {cases: 40, modules: 40, passed: 40 - failed.length, failed: failed.length,
  segment_faults: observations.filter(row => row.expected.reason === 11).length, policy_controls: observations.filter(row => row.expected.reason === 1).length};
writeFileSync(join(output, 'result.json'), JSON.stringify({status: failed.length ? 'FAIL' : 'PASS', engine: {path: enginePath, sha256: engine.sha256, bytes: engine.bytes.length},
  source_pins: sourcePins, arena_bytes: SIZE, mapped_ram_pages: pages, counts, semantic_rows: rows, modules, observations, helper_trace: helperTrace,
  limits: ['finite legacy MASKMOVQ/MASKMOVDQU cached operand-wide write authority for nonzero masks in four replacement/resident extent/entry profiles',
    'zero mask skips null/address/page checks as the retained Ring3 implementation-dependent policy; MASKMOVQ still enters MMX state',
    'sparse nonzero masks retain selected-byte RAM paging/write policy; unselected missing pages are not a universal hardware suppression claim',
    'no AVX masked-memory, additional prefix/operand combinations, browser, throughput, or game compatibility claim'],
}, null, 2) + '\n');
console.log(JSON.stringify({status: failed.length ? 'FAIL' : 'PASS', ...counts, output}));
assert.equal(failed.length, 0, failed.map(row => `${row.owner}/${row.entries}/${row.name}: ${row.errors.join(', ')}`).join('\n'));
