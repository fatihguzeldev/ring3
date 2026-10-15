import {generatedImports} from '../../../../../../../support/wasm/binding.mjs';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../../../../../../../support/wasm/engine.mjs';
import {ARENA_SIZE} from '../../../../../../../support/wasm/arena.mjs';

const [enginePath, output, root] = process.argv.slice(2);
const engine = readEngine(enginePath);
const initial = readFileSync(join(output, 'initial-arena.bin'));
assert.equal(initial.length, ARENA_SIZE, 'current complete arena');
const SIZE = ARENA_SIZE, T = 140, FP = 4236, SV = 4364, BASE = 0x5000, SEG = 4524;
const defaults = [0xf33fffff, 0xf3bfffff, 0xf33fffff, 0xf33fffff, 0xf33fffff, 0];
const cacheOffsets = [28, 32, 36, 40, 44, 56];
const hex = value => Buffer.from(value, 'hex');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const dwords = values => {
  const bytes = Buffer.alloc(values.length * 4);
  values.forEach((value, index) => bytes.writeUInt32LE(value >>> 0, index * 4));
  return bytes;
};
const record = (magic, version, size, values) => {
  const bytes = Buffer.alloc(size);
  bytes.write(magic);
  bytes.writeUInt16LE(version, 4);
  bytes.writeUInt16LE(1, 6);
  bytes.writeUInt32LE(size, 8);
  values.forEach((value, index) => bytes.writeUInt32LE(value >>> 0, 16 + index * 4));
  return bytes;
};
const state = (registers, pc) => record('R3ST', 1, 56, [...registers, pc, 0xcd7]);
const legacyExit = (reason, retired, detail = 0, address = 0, access = 0, length = 0) =>
  record('R3EX', 9, 40, [reason, retired, detail, address, access, length]);
const exit = (reason, retired, detail = 0, address = 0, access = 0, length = 0) =>
  reason === 5 ? record('R3EX', 12, 40, [23, retired, 4 | (access === 2 ? 2 : 0) | (detail === 2 ? 1 : 0), address, access, length])
    : legacyExit(reason, retired, detail, address, access, length);
const legacyPacket = (status = 0, detail = 0, address = 0, access = 0, value = 0) =>
  record('R3MH', 8, 40, [status, value, detail, address, access, 832]);
const packet = (status = 0, detail = 0, address = 0, access = 0, value = 0) =>
  status === 1 ? record('R3MH', 15, 40, [9, value, 4 | (access === 2 ? 2 : 0) | (detail === 2 ? 1 : 0), address, access, 832])
    : legacyPacket(status, detail, address, access, value);
const parents = () => [3, 0x11223344, 0, 0x44556677, 0x7080, 0x66778899, 0x778899aa, BASE];

// fixed physical tags: valid, zero, special, empty, valid, special, valid, empty.
const raw80 = [
  hex('0100000000000080ff3f'), hex('00000000000000000000'),
  hex('34120000000000c0ff7f'), hex('05040302010000000000'),
  hex('02000000000000800040'), hex('01000000000000000000'),
  hex('030000000000008001c0'), hex('112233445566778899aa'),
];
function floating(status, control, pointer) {
  const bytes = Buffer.from(initial.subarray(FP, FP + 128));
  bytes.writeUInt16LE(control, 16);
  bytes.writeUInt16LE(status, 18);
  bytes.writeUInt16LE(0xc8e4, 20);
  bytes.writeUInt16LE(0x371, 22);
  bytes.writeUInt32LE(pointer, 24);
  bytes.writeUInt32LE(0x87654321, 28);
  bytes.writeUInt16LE(0x1b, 32);
  bytes.writeUInt16LE(0x23, 34);
  raw80.forEach((raw, index) => raw.copy(bytes, 40 + index * 10));
  return bytes;
}
const currentFP = floating(0x1801, 0x027e, 0x12345678);
const restoredFP = floating(0x2800, 0x037f, 0xaabbccdd);
const currentXmm = Buffer.concat(Array.from({length: 8}, (_, index) => Buffer.alloc(16, 0x70 + index)));
const restoredXmm = Buffer.concat(Array.from({length: 8}, (_, index) => Buffer.alloc(16, 0x30 + index)));

function legacyImage(fp, xmm, mxcsr, physicalOrder) {
  const bytes = Buffer.alloc(576, 0x9b);
  bytes.fill(0, 0, 24);
  bytes.fill(0, 32, 160);
  bytes.writeUInt16LE(fp.readUInt16LE(16), 0);
  bytes.writeUInt16LE(fp.readUInt16LE(18), 2);
  bytes[4] = 0x77;
  bytes.writeUInt16LE(0x371, 6);
  bytes.writeUInt32LE(fp.readUInt32LE(24), 8);
  bytes.writeUInt16LE(0x1b, 12);
  bytes.writeUInt32LE(0x87654321, 16);
  bytes.writeUInt16LE(0x23, 20);
  bytes.writeUInt32LE(mxcsr, 24);
  bytes.writeUInt32LE(0xffff, 28);
  physicalOrder.forEach((physical, logical) => raw80[physical].copy(bytes, 32 + logical * 16));
  xmm.copy(bytes, 160);
  bytes.fill(0, 512, 536);
  return bytes;
}
const savedLegacy = legacyImage(currentFP, currentXmm, 0xdfe5, [3, 4, 5, 6, 7, 0, 1, 2]);
const restoredLegacy = legacyImage(restoredFP, restoredXmm, 0x5f80, [5, 6, 7, 0, 1, 2, 3, 4]);
function savedImage(before, mask) {
  const after = Buffer.from(before);
  for (const [selected, start, end] of [
    [mask & 1, 0, 24], [mask & 2, 24, 32], [mask & 1, 32, 160], [mask & 2, 160, 288],
  ]) if (selected) savedLegacy.copy(after, start, start, end);
  after.writeBigUInt64LE(before.readBigUInt64LE(512) | BigInt(mask), 512);
  return after;
}
const saveCode = '0fae27', restoreCode = '0fae2f';
const rows = [];
// literal segment scenarios are independent of the production selected-span table.
for (const code of [saveCode, restoreCode]) {
  const save = code === saveCode, label = save ? 'XSAVE' : 'XRSTOR';
  for (const row of [
    {name: 'header-last-byte-over-limit', cache: {3: 0xf2300000 | (BASE + (save ? 518 : 534))}},
    {name: 'default-SS-header-limit', code: save ? '0fae6500' : '0fae6d00', registers: {5: BASE}, cache: {2: 0xf2305000}, ss: true},
    {name: 'explicit-SS-header-limit', code: '36' + code, cache: {2: 0xf2305000}, ss: true},
    {name: 'null-DS', nullDs: true},
    {name: 'FS-logical-span-before-wrapping-base', code: '64' + code, address: 0x6000, linear: BASE, fsBase: 0xfffff000,
      cache: {4: 0xf2300000 | (0x6000 + (save ? 518 : 534))}},
    {name: 'expand-down-selected-x87-payload-denied', mask: 1, cache: {3: 0xf27051ff}},
    {name: 'expand-down-selected-SSE-payload-denied', mask: 2, cache: {3: 0xf27051ff}},
    {name: 'segment-policy-before-unmapped-header', address: 0x5e00, cache: {3: 0xf2305fff}},
    {name: 'SS-segment-policy-before-linear-alignment', code: '36' + code, address: BASE + 1, cache: {2: 0xf2305000}, ss: true},
    {name: 'segment-policy-before-invalid-header', cache: {3: 0xf2305000}, badHeader: true},
  ]) {
    const image = Buffer.from(restoredLegacy); image.writeBigUInt64LE(0n, 512);
    if (row.badHeader) image[520] = 1;
    rows.push({...row, code: row.code ?? code, name: `${label}-${row.name}`, image: row.address === 0x5e00 ? undefined : image,
      reason: row.ss ? 15 : 11, status: row.ss ? 7 : 3});
  }
  for (const row of [
    {name: 'exact-header-end', cache: {3: 0xf2300000 | (BASE + (save ? 519 : 535))}},
    {name: 'expand-down-empty-only-authorizes-header', mask: 0, cache: {3: 0xf27051ff}},
    {name: 'high-mask-selects-no-extra-state', mask: 0xfffffff8, high: 0xffffffff, cache: {3: 0xf27051ff}},
    {name: 'FS-wrap-with-valid-logical-span', code: '64' + code, address: 0x6000, linear: BASE, fsBase: 0xfffff000,
      cache: {4: 0xf2300000 | (0x6000 + (save ? 519 : 535))}},
    {name: 'address16-internal-header-does-not-wrap', code: '67' + (save ? '0fae25' : '0fae2d'), address: 0xdeadffc0,
      linear: 0xffc0, cache: {3: 0xf23101ff}},
  ]) {
    const image = Buffer.from(restoredLegacy); image.writeBigUInt64LE(0n, 512);
    const mask = row.mask ?? 3, selected = mask & 3;
    rows.push({...row, code: row.code ?? code, name: `${label}-${row.name}`, image,
      ...(save ? {written: savedImage(image, selected)} : {
        afterFP: selected & 1 ? initial.subarray(FP, FP + 128) : currentFP,
        afterXmm: selected & 2 ? Buffer.alloc(128) : currentXmm,
        afterMxcsr: selected & 2 ? 0x5f80 : 0xdfe5})});
  }
  for (const row of [
    {name: 'budget-zero-before-cache', budget: 0, reason: 1, retired: 0},
    {name: 'cancel-before-cache', cancel: 1, reason: 2, retired: 0},
  ]) rows.push({...row, name: `${label}-${row.name}`, code, cache: {3: 0xf2300000}, direct: true});
}
rows.unshift({name: 'XSAVE-readonly-cache-refuses-before-header-read', code: saveCode, mask: 0, cache: {3: 0xf31fffff}, reason: 11, status: 3});
rows.push({name: 'XSAVE-readable-CS-cannot-write', code: '2e' + saveCode, reason: 11, status: 3},
  {name: 'XRSTOR-execute-only-CS-cannot-read', code: '2e' + restoreCode, cache: {1: 0xf39fffff}, reason: 11, status: 3});
for (let mask = 0; mask < 4; mask++) {
  const image = Buffer.alloc(576, 0xa5);
  image.writeBigUInt64LE(0x8000000000000040n, 512);
  rows.push({name: `save-selected-${mask}-pending-x87-and-reserved-bytes`, code: saveCode,
    mask, high: 0xffffffff, image, written: savedImage(image, mask)});
}
for (let mask = 0; mask < 4; mask++) for (let present = 0; present < 4; present++) {
  const image = Buffer.from(restoredLegacy);
  image.writeBigUInt64LE(BigInt(present), 512);
  rows.push({name: `restore-request-${mask}-present-${present}`, code: restoreCode, mask, image,
    afterFP: mask & 1 ? (present & 1 ? restoredFP : initial.subarray(FP, FP + 128)) : currentFP,
    afterXmm: mask & 2 ? (present & 2 ? restoredXmm : Buffer.alloc(128)) : currentXmm,
    afterMxcsr: mask & 2 ? 0x5f80 : 0xdfe5});
}
for (const offset of [512, 520, 527, 528, 535]) {
  const image = Buffer.from(restoredLegacy);
  image[offset] = offset === 512 ? 8 : 1;
  rows.push({name: `restore-rejects-header-byte-${offset}-even-empty-request`, code: restoreCode,
    mask: 0, image, reason: 11, status: 3});
}
for (const [mask, present] of [[2, 0], [2, 3], [0, 3], [1, 3]]) {
  const image = Buffer.from(restoredLegacy);
  image.writeBigUInt64LE(BigInt(present), 512);
  image.writeUInt32LE(0x10000, 24);
  const fault = Boolean(mask & 2);
  rows.push({name: `restore-mxcsr-validation-request-${mask}-present-${present}`, code: restoreCode,
    mask, image, ...(fault ? {reason: 11, status: 3} : {afterFP: mask & 1 ? restoredFP : currentFP})});
}
for (const code of [saveCode, restoreCode]) {
  rows.push({name: `${code}-alignment-before-unmapped`, code, address: 0x9001, reason: 11, status: 3});
  rows.push({name: `${code}-header-page-fault-before-components`, code, address: 0x5e00,
    seeds: [[0x5e00, Buffer.alloc(512, 0x9b)]], unchanged: [[0x5e00, Buffer.alloc(512, 0x9b)]],
    reason: 5, status: 1, detail: 1, fault: 0x6000, access: code === saveCode ? 2 : 1});
  rows.push({name: `${code}-header-address-overflow`, code, address: 0xfffffe00,
    reason: 11, status: 3});
}
rows.push({name: 'save-late-readonly-header-write-preserves-earlier-component-bytes', code: saveCode,
  address: 0x5e00, extra: true, protects: [[0x6000, 1]],
  seeds: [[0x5e00, Buffer.alloc(512, 0x9b)], [0x6000, Buffer.alloc(8)]],
  unchanged: [[0x5e00, Buffer.alloc(512, 0x9b)], [0x6000, Buffer.alloc(8)]],
  reason: 5, status: 1, detail: 2, fault: 0x6000, access: 2});
rows.push({name: 'save-writeonly-header-still-requires-read-before-write', code: saveCode, mask: 0,
  image: Buffer.alloc(576, 0x9b), protects: [[BASE, 2]],
  reason: 5, status: 1, detail: 2, fault: BASE + 512, access: 2});
const header = Buffer.alloc(24);
const badHeader = Buffer.from(header); badHeader[0] = 8;
rows.push({name: 'restore-invalid-header-precedes-unmapped-mxcsr', code: restoreCode, address: 0x4e00,
  seeds: [[BASE, badHeader]], reason: 11, status: 3});
rows.push({name: 'restore-empty-request-only-needs-header', code: restoreCode, address: 0x4e00,
  mask: 0, seeds: [[BASE, header]]});
rows.push({name: 'restore-absent-x87-initializes-without-payload-access', code: restoreCode, address: 0x4e00,
  mask: 1, seeds: [[BASE, header]], afterFP: initial.subarray(FP, FP + 128)});
rows.push({name: 'restore-absent-sse-still-reads-mxcsr', code: restoreCode, address: 0x4e00,
  mask: 2, seeds: [[BASE, header]], reason: 5, status: 1, detail: 1, fault: 0x4e18, access: 1});
rows.push({name: 'xgetbv-zero-direct-preserves-flags', code: '0f01d0', control: 0, direct: true, result: [7, 0]});
rows.push({name: 'xgetbv-one-gp-without-publication', code: '0f01d0', control: 1, direct: true, reason: 11});
rows.push({name: 'xgetbv-high-index-gp-without-publication', code: '0f01d0', control: 0xffffffff, direct: true, reason: 11});

const sourcePaths = ['engine/tests/support/wasm/arena.mjs', 'engine/tests/support/wasm/binding.mjs',
  'engine/src/cpu/x86/xstate.rs', 'engine/src/cpu/x86/xstate/legacy.rs',
  'engine/src/cpu/x86/xstate/access.rs', 'engine/src/cpu/x86/xstate/access/tests.rs',
  'engine/src/cpu/dbt/wasm/segments/access.rs', 'engine/src/cpu/x86/segments/cache.rs',
  'engine/src/cpu/x86/decode/xstate.rs', 'engine/src/cpu/dbt/wasm/xstate.rs',
  'engine/src/process/cpu/xstate.rs', 'engine/src/process/cpu/xstate/upper.rs',
  'engine/src/cpu/x86/capabilities.rs', 'engine/src/cpu/x86/xstate/upper.rs',
  'engine/src/cpu/x86/xstate/upper/access.rs', 'engine/src/cpu/dbt/wasm/xstate/upper.rs', 'engine/tests/cpu/simd/state/segments/native.rs',
  'engine/tests/cpu/simd/state/segments/wasm.rs',
  'engine/tests/cpu/simd/state/segments/fixtures/p2-xstate-segments/runtime/run.mjs',
];
const sourceHashes = () => Object.fromEntries(sourcePaths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = sourceHashes();
const contexts = [], modules = [], observations = [], helpers = [], exitRecords = [];
let identity = 0;
const arena = context => Buffer.from(new Uint8Array(context.memory.buffer, context.base, SIZE));
const input = (context, at, bytes) => new Uint8Array(context.memory.buffer, context.base + at, bytes.length).set(bytes);
const host = (context, name, args) => assert.equal(context.api[name](...args), 0, name);
const upload = (context, address, bytes) => {input(context, T, bytes); host(context, 'upload', [address, bytes.length]);};
function open(owner, entries, batch) {
  const instance = new WebAssembly.Instance(engine.module, {});
  const context = {owner, entries, memory: instance.exports.memory, api: {}, id: ++identity};
  for (const [name, fn] of Object.entries(instance.exports))
    if (name.startsWith('ring3_abi_v1_') && typeof fn === 'function') context.api[name.slice(13)] = fn;
  host(context, 'open', [8, context.id, 0x58535441]);
  context.base = context.api.arena_ptr() >>> 0;
  input(context, 0, initial);
  for (const page of [0x1000, BASE, 0x7000, 0xf000, 0x10000]) host(context, 'map', [page, 1, 7]);
  const blocks = batch.map((row, index) => ({pc: 0x1000 + index * 32, bytes: Buffer.concat([hex(row.code), hex('ebfe')])}));
  blocks.forEach(block => upload(context, block.pc, block.bytes));
  input(context, T, dwords(blocks.flatMap(block => entries ? [block.pc] : [block.pc, block.bytes.length])));
  const compile = owner === 'resident' ? (entries ? 'compile_resident_entries' : 'compile_resident') : (entries ? 'compile_entries' : 'compile');
  assert.equal(context.api[compile](...(entries ? [blocks.length, 0] : [blocks.length])), 0,
    `${compile}: ${batch.map(row => row.name).join(', ')}`);
  const snapshot = arena(context);
  const binding = owner === 'resident'
    ? {pointer: snapshot.readUInt32LE(T + 16), length: snapshot.readUInt32LE(T + 20)}
    : {pointer: context.api.module_ptr() >>> 0, length: context.api.module_len() >>> 0};
  const bytes = Buffer.from(new Uint8Array(context.memory.buffer, binding.pointer, binding.length));
  const module = new WebAssembly.Module(bytes), ring3 = {}, imports = WebAssembly.Module.imports(module);
  const operations = ['xsave', 'xrstor'].filter(name => batch.some(row => {
    const operand = row.code.match(/0fae([0-9a-f]{2})/);
    return operand !== null && (((parseInt(operand[1], 16) >>> 3) & 7) === 4) === (name === 'xsave');
  }));
  assert.deepEqual(imports, generatedImports({resident: owner === 'resident', helpers: operations.map(name =>
    `cpu_${name}_avx${owner === 'resident' ? '_resident' : ''}`)}));
  for (const imported of imports.slice(1)) {
    assert.equal(imported.kind, 'function'); assert.equal(imported.module, 'ring3');
    ring3[imported.name] = (...args) => {
      let status;
      if (context.raw && imported.name.startsWith('cpu_x')) {
        input(context, 100, context.raw.packet);
        status = context.raw.returnStatus ?? 0;
      } else status = context.api[imported.name](...args);
      if (imported.name.startsWith('guard')) {
        assert.equal(typeof status, 'bigint'); assert.ok(status > 0n, 'positive bound entry epoch'); context.epoch = status;
      } else if (imported.name === 'finish_exception_detection') {
        assert.equal(status, 0, 'actual stop finalizer');
        const after = arena(context);
        assert.deepEqual(args, [context.epoch, ...[72, 80, 84, 88, 92].map(at => after.readUInt32LE(at)), 0, 0, 0, 0, 0, 0]);
      } else helpers.push({context: context.id, name: imported.name,
        args: args.map(value => value >>> 0), status, packet: arena(context).subarray(100, 140).toString('hex'),
        nonforwarded: Boolean(context.raw)});
      return status;
    };
  }
  context.run = new WebAssembly.Instance(module, {env: {memory: context.memory}, ring3}).exports.run;
  writeFileSync(join(output, `module-${context.id}.wasm`), bytes);
  contexts.push({context: context.id, owner, entries});
  modules.push({context: context.id, sha256: hash(bytes), bytes: bytes.length, imports: WebAssembly.Module.imports(module)});
  return [context, blocks];
}
function verifyMemory(context, address, wanted, name) {
  for (let index = 0; index < wanted.length; index++) {
    host(context, 'read8', [address + index]);
    assert.equal(arena(context).readUInt32LE(120), wanted[index], `${name} memory byte${index}`);
  }
}
function test(context, block, row) {
  input(context, 0, initial);
  if (row.extra) host(context, 'map', [0x6000, 1, 3]);
  const registers = parents();
  registers[0] = row.mask ?? 3; registers[2] = row.high ?? 0; registers[7] = row.address ?? BASE;
  if (row.control !== undefined) registers[1] = row.control;
  for (const [index, value] of Object.entries(row.registers ?? {})) registers[+index] = value;
  const segments = Buffer.from(initial.subarray(SEG, SEG + 64));
  if (row.cache) {
    segments.writeUInt16LE(2, 4);
    defaults.forEach((value, i) => segments.writeUInt32LE(value >>> 0, cacheOffsets[i]));
    for (const [index, value] of Object.entries(row.cache)) segments.writeUInt32LE(value >>> 0, cacheOffsets[+index]);
  }
  if (row.fsBase !== undefined) segments.writeUInt32LE(row.fsBase >>> 0, 48);
  if (row.nullDs) {segments.writeUInt16LE(0, 22); if (row.cache) segments.writeUInt32LE(0, cacheOffsets[3]);}
  input(context, SEG, segments);
  input(context, 96, dwords([row.cancel ?? 0]));
  input(context, 0, state(registers, block.pc));
  input(context, 56, exit(1, 0));
  input(context, FP, currentFP); input(context, SV + 16, dwords([0xdfe5])); input(context, SV + 32, currentXmm);
  if (row.image) upload(context, row.linear ?? registers[7], row.image);
  for (const [address, bytes] of row.seeds ?? []) upload(context, address, bytes);
  for (const [address, bits] of row.protects ?? []) host(context, 'protect', [address, 1, bits]);
  // the whole transfer slab is a canary; xstate owns no transfer scratch.
  input(context, T, Buffer.from(Array.from({length: 4096}, (_, index) => (index * 29 + 0x63) % 255 + 1)));
  const expected = arena(context), reason = row.reason ?? 1, retired = row.retired ?? (reason === 1 ? 1 : 0);
  if (row.result) {registers[0] = row.result[0]; registers[2] = row.result[1];}
  expected.set(state(registers, block.pc + (retired ? hex(row.code).length : 0)), 0);
  expected.set(exit(reason, retired, row.detail ?? 0, row.fault ?? 0, row.access ?? 0, reason === 5 ? 832 : 0), 56);
  if (!row.direct) expected.set(packet(row.status ?? 0, row.detail ?? 0, row.fault ?? 0, row.access ?? 0), 100);
  if (row.afterFP) expected.set(row.afterFP, FP);
  if (row.afterXmm) expected.set(row.afterXmm, SV + 32);
  if (row.afterMxcsr !== undefined) expected.writeUInt32LE(row.afterMxcsr, SV + 16);
  const start = helpers.length;
  assert.equal(context.run(context.base, context.base + 56, row.budget ?? 1, context.base + 96), 0, row.name);
  const actual = arena(context), events = helpers.slice(start);
  assert.equal(events.length, row.direct ? 0 : 1, row.name + ' exact helper count');
  if (!row.direct) {
    const event = events[0];
    assert.equal(event.name, `cpu_${row.code.endsWith('27') || row.code.endsWith('6500') || row.code.endsWith('25') ? 'xsave' : 'xrstor'}_avx${context.owner === 'resident' ? '_resident' : ''}`);
    const segment = row.ss || row.code.startsWith('36') ? 2 : row.code.startsWith('64') ? 4 : row.code.startsWith('2e') ? 1 : 3;
    const offset = row.code.startsWith('67') ? (row.address ?? BASE) & 0xffff : row.address ?? BASE;
    assert.deepEqual(event.args.slice(-4), [segment, offset, row.mask ?? 3, row.high ?? 0]);
    assert.equal(event.packet, expected.subarray(100, 140).toString('hex'));
    assert.equal(event.status, reason === 5 ? 23 : 0, 'real native helper transport');
  }
  assert.deepEqual(actual, expected, row.name + ' complete arena');
  exitRecords.push(actual.subarray(56, 96));
  for (const [address] of row.protects ?? []) host(context, 'protect', [address, 1, 3]);
  if (row.image) verifyMemory(context, row.linear ?? row.address ?? BASE, row.written ?? row.image, row.name);
  for (const [address, bytes] of row.unchanged ?? []) verifyMemory(context, address, bytes, row.name);
  observations.push({context: context.id, name: row.name, reason, retired, helper_calls: events.length,
    arena_sha256: hash(actual), state: actual.subarray(0, 56).toString('hex'), exit: actual.subarray(56, 96).toString('hex')});
  if (row.extra) host(context, 'unmap', [0x6000, 1]);
}
for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  for (let start = 0; start < rows.length; start += 8) {
    const batch = rows.slice(start, start + 8), [context, blocks] = open(owner, entries, batch);
    batch.forEach((row, index) => test(context, blocks[index], row));
    host(context, 'close', []);
  }
  const smc = {name: 'save-code-write-retires-before-currentness-exit', code: saveCode};
  const [context, blocks] = open(owner, entries, [smc]);
  const original = Buffer.alloc(576); blocks[0].bytes.copy(original);
  const expected = savedImage(original, 1);
  input(context, 0, initial);
  const registers = parents(); registers[0] = 1; registers[7] = 0x1000;
  input(context, 0, state(registers, 0x1000)); input(context, 56, exit(1, 0));
  input(context, FP, currentFP); input(context, SV + 16, dwords([0xdfe5])); input(context, SV + 32, currentXmm);
  const wantedArena = arena(context); wantedArena.set(state(registers, 0x1003)); wantedArena.set(exit(6, 1), 56); wantedArena.set(packet(), 100);
  const start = helpers.length;
  assert.equal(context.run(context.base, context.base + 56, 1, context.base + 96), 0);
  const actual = arena(context);
  assert.deepEqual(actual, wantedArena, smc.name + ' complete arena');
  assert.equal(helpers.length - start, 1); assert.equal(helpers.at(-1).status, 11);
  verifyMemory(context, 0x1000, expected, smc.name);
  exitRecords.push(actual.subarray(56, 96));
  observations.push({context: context.id, name: smc.name, reason: 6, retired: 1, helper_calls: 1, arena_sha256: hash(actual)});
  host(context, 'close', []);
}

// injected legacy packets test the lowerer protocol, not native page-fault certification.
const rawRows = [
  {name: 'success-value-must-be-zero', packet: legacyPacket(0, 0, 0, 0, 1)},
  {name: 'legacy-packet-version7-is-rejected-by-active-generated-path', packet: record('R3MH', 7, 40, [0, 0, 0, 0, 0, 576])},
  {name: 'unrequested-upper-payload-fault-is-rejected', packet: legacyPacket(1, 1, BASE + 576, 1)},
  {name: 'selected-upper-payload-read-fault-is-canonical', mask: 4, packet: legacyPacket(1, 1, BASE + 576, 1), reason: 5, detail: 1, fault: BASE + 576, access: 1, upper: true},
  {name: 'upper-reserved-tail-fault-is-rejected', mask: 4, packet: legacyPacket(1, 1, BASE + 704, 1)},
  {name: 'floating-exception-is-not-xstate-status', packet: legacyPacket(5, 0x1f81)},
  {name: 'gp-error-code-must-be-zero', packet: legacyPacket(3, 1)},
  {name: 'fault-in-reserved-gap-is-rejected', packet: legacyPacket(1, 1, BASE + 300, 1)},
  {name: 'fault-in-unrequested-mxcsr-is-rejected', mask: 1, packet: legacyPacket(1, 1, BASE + 24, 1)},
  {name: 'success-at-misaligned-address-is-rejected', address: BASE + 1, packet: legacyPacket()},
  {name: 'header-read-fault-is-canonical', packet: legacyPacket(1, 1, BASE + 512, 1), reason: 5, detail: 1, fault: BASE + 512, access: 1},
  {name: 'canonical-gp-zero-is-accepted', packet: legacyPacket(3), reason: 11},
];
for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) for (const code of [saveCode, restoreCode]) {
  const [context, blocks] = open(owner, entries, [{code}]);
  for (const original of rawRows) {
    const row = original.upper && code === saveCode ? {...original, packet: legacyPacket(1, 1, BASE + 576, 2), access: 2} : original;
    input(context, 0, initial);
    const registers = parents(); registers[0] = row.mask ?? 3; registers[7] = row.address ?? BASE;
    input(context, 0, state(registers, blocks[0].pc)); input(context, 56, exit(1, 0));
    const expected = arena(context), reason = row.reason ?? 7;
    expected.set(legacyExit(reason, 0, row.detail ?? (reason === 7 ? 2 : 0), row.fault ?? 0, row.access ?? 0, reason === 5 ? 832 : 0), 56);
    expected.set(row.packet, 100);
    const start = helpers.length; context.raw = row;
    assert.equal(context.run(context.base, context.base + 56, row.budget ?? 1, context.base + 96), 0, row.name);
    delete context.raw;
    const actual = arena(context);
    assert.deepEqual(actual, expected, row.name + ' raw complete arena');
    assert.equal(helpers.length - start, 1); assert.equal(helpers.at(-1).nonforwarded, true);
    exitRecords.push(actual.subarray(56, 96));
    observations.push({context: context.id, name: `raw-${code}-${row.name}`, reason, retired: 0, helper_calls: 1, arena_sha256: hash(actual)});
  }
  host(context, 'close', []);
}
assert.deepEqual(sourceHashes(), sourcePins);
writeFileSync(join(output, 'exit-records.bin'), Buffer.concat(exitRecords));
const result = {status: 'PASS', engine: {path: enginePath, sha256: engine.sha256, bytes: engine.bytes.length},
  arena_bytes: SIZE, counts: {contexts: contexts.length, modules: modules.length, runs: observations.length, helpers: helpers.length},
  contexts, modules, observations, helpers, source_sha256: sourcePins,
  scope: 'standard xstate components0/1 regression through active segmented helper8/832 and exit9 under four bound profiles; exact whole arena and selected guest bytes, strict upper selected-packet controls, native decode of every actual exit; legacy576 helpers remain covered separately, no performance claim'};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2));
console.log(JSON.stringify({status: 'PASS', ...result.counts, output}));
