import {instantiateCore} from './link.mjs';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync, openSync, closeSync} from 'node:fs';
import {readEngine} from '../../../support/wasm/engine.mjs';

const [enginePath, reportPath] = process.argv.slice(2);
assert(enginePath && reportPath, 'usage: numeric-direct-simd.mjs engine.wasm fresh-report.json');
const engine = readEngine(enginePath), output = openSync(reportPath, 'wx');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const unhex = value => Uint8Array.from(Buffer.from(value.replaceAll(' ', ''), 'hex'));
const words = values => {
  const bytes = new Uint8Array(values.length * 4), view = new DataView(bytes.buffer);
  values.forEach((value, index) => view.setUint32(index * 4, value >>> 0, true));
  return bytes;
};
const report = {schema: 1, complete: false, engineSha256: engine.sha256,
  sourceSha256: hash(readFileSync(new URL(import.meta.url))), cases: [],
  limits: ['authored actual wasm numerical and memory boundaries; no game or performance claim',
    'helper call counts distinguish direct execution from canonical fallback; this does not prove all SIMD instructions']};
const checkpoint = () => writeFileSync(reportPath, JSON.stringify(report, null, 2));
const FP = 4236, SIMD = 4364, YMM = 4612;
let stage = 'initialization';

function runCase(name, options) {
  stage = name;
  const instance = instantiateCore(engine.module, {ring3_observability: {now_ns: () => 0n}});
  const memory = instance.exports.memory;
  const api = Object.fromEntries(Object.entries(instance.exports).filter(([name]) => name.startsWith('ring3_abi_v1_'))
    .map(([name, value]) => [name.slice(13), value]));
  const view = () => new DataView(memory.buffer);
  const slice = (at, length) => new Uint8Array(memory.buffer).slice(at, at + length);
  const word = (at, offset = 0) => view().getUint32(at + offset, true);
  const put = (at, offset, value) => view().setUint32(at + offset, value >>> 0, true);
  const budget = options.budget ?? 1, retired = options.retired ?? budget, stop = options.stop ?? 2;
  const code = unhex(options.code ?? '0f58c1');
  const pages = new Map([[0x1000, {permission: 7, bytes: new Uint8Array(4096)}],
    [0x3000, {permission: 3, bytes: new Uint8Array(4096)}]]);
  for (const [address, permission] of options.maps ?? []) pages.set(address, {permission, bytes: new Uint8Array(4096)});
  pages.get(0x1000).bytes.set(code); pages.get(0x1000).bytes.set(unhex('0f0b'), code.length);
  for (const [address, bytes] of options.data ?? []) for (let i = 0; i < bytes.length; i++) {
    pages.get((address + i) & ~4095).bytes[(address + i) & 4095] = bytes[i];
  }
  const expectedPages = new Map([...pages].map(([address, page]) => [address, page.bytes.slice()]));
  for (const [address, bytes] of options.writes ?? []) for (let i = 0; i < bytes.length; i++) {
    expectedPages.get((address + i) & ~4095)[(address + i) & 4095] = bytes[i];
  }
  const initialXmm = Array.from({length: 8}, (_, index) => words(options.xmm?.[index] ?? [index, index + 1, index + 2, index + 3]));
  const expectedXmm = initialXmm.map(bytes => bytes.slice());
  for (const [index, values] of Object.entries(options.expectedXmm ?? {})) expectedXmm[Number(index)] = words(values);
  let arena, context, opened = false, active = false, helperCalls = 0, readCalls = 0, writeCalls = 0;
  const row = {name, codeHex: Buffer.from(code).toString('hex'), controls: []}; report.cases.push(row);
  const status = (name, wanted, ...args) => {
    const actual = api[name](...args) >>> 0; row.controls.push({name, wanted, actual});
    assert.equal(actual, wanted, `${stage}: ${name}`);
  };
  try {
    status('open', 0, 16, 97, 0); opened = true; arena = api.arena_ptr() >>> 0;
    for (const [address, page] of pages) {
      status('map', 0, address, 1, page.permission | 2);
      new Uint8Array(memory.buffer, arena + 140, 4096).set(page.bytes);
      status('upload', 0, address, 4096);
      if (!(page.permission & 2)) status('protect', 0, address, 1, page.permission);
    }
    const registers = [5, 0xfeedbeef, 7, options.base ?? 0x3000, 0x4800, 0, 0, 0];
    new Uint8Array(memory.buffer, arena, 56).fill(0);
    new Uint8Array(memory.buffer, arena, 4).set([82, 51, 83, 84]);
    view().setUint16(arena + 4, 3, true); view().setUint16(arena + 6, 1, true); put(arena, 8, 56);
    registers.forEach((value, index) => put(arena, 16 + index * 4, value));
    put(arena, 48, 0x1000); put(arena, 52, 0x403);
    put(arena, SIMD + 16, options.mxcsr ?? 0x1f80);
    initialXmm.forEach((bytes, index) => new Uint8Array(memory.buffer, arena + SIMD + 32 + index * 16, 16).set(bytes));
    const initialFp = slice(arena + FP, 128), initialSimd = slice(arena + SIMD, 160), initialYmm = slice(arena + YMM, 144);
    status('core_prepare', 0, 0x1000, 256, 32); context = api.core_context_ptr() >>> 0;
    const generated = slice(api.core_code_ptr() >>> 0, api.core_code_len() >>> 0);
    const module = new WebAssembly.Module(generated);
    const run = instantiateCore(module, {ring3: {memory,
      numeric: (...args) => {helperCalls++; return api.core_numeric(...args);},
      read: (...args) => {readCalls++; return api.core_read(...args);},
      write: (...args) => {writeCalls++; return api.core_write(...args);}}}).exports.run;
    const physical = address => {
      const slot = word(word(context, 56) + (address >>> 12) * 4);
      assert(slot > 0 && slot <= word(context, 68));
      return word(context, 64) + (slot - 1) * 4096 + (address & 4095);
    };
    status('core_begin', 0); active = true;
    const initialVersion = view().getBigUint64(word(context, 80), true);
    const pageVersions = slice(word(context, 84), word(context, 68) * 16);
    row.stop = run(context, budget) >>> 0;
    row.helperCalls = helperCalls; row.readCalls = readCalls; row.writeCalls = writeCalls;
    row.state = {pc: word(context, 32), flags: word(context, 36), retired: word(context, 40),
      remaining: word(context, 44), detail: word(context, 48), metadata: word(context, 52), vector: word(context, 116)};
    row.moduleSha256 = hash(generated); checkpoint();
    assert.equal(row.stop, stop, `${name}: stop`);
    assert.equal(helperCalls, options.helpers ?? 0, `${name}: numerical helper calls`);
    assert.equal(readCalls, 0, `${name}: scalar read helper`); assert.equal(writeCalls, 0, `${name}: scalar write helper`);
    assert.equal(row.state.retired, retired, `${name}: retired`);
    assert.equal(row.state.remaining, budget - retired, `${name}: remaining`);
    assert.equal(row.state.pc, options.pc ?? 0x1000 + code.length, `${name}: pc`);
    assert.equal(row.state.flags, 0x403, `${name}: flags`);
    if (options.vector !== undefined) assert.equal(row.state.vector, options.vector, `${name}: vector`);
    if (options.detail !== undefined) assert.equal(row.state.detail, options.detail, `${name}: detail`);
    status('core_end', 0); active = false;
    assert.deepEqual(slice(arena + SIMD, 160), initialSimd, `${name}: private resident bank`);
    status('core_export', 0, stop, retired);
    assert.deepEqual(Array.from({length: 8}, (_, i) => word(context, i * 4)), registers, `${name}: general registers`);
    const wantedSimd = initialSimd.slice(), expectedView = new DataView(wantedSimd.buffer);
    expectedView.setUint32(16, options.expectedMxcsr ?? options.mxcsr ?? 0x1f80, true);
    expectedXmm.forEach((bytes, index) => wantedSimd.set(bytes, 32 + index * 16));
    assert.deepEqual(slice(arena + SIMD, 160), wantedSimd, `${name}: full SIMD160`);
    assert.deepEqual(slice(arena + FP, 128), initialFp, `${name}: full FP128`);
    assert.deepEqual(slice(arena + YMM, 144), initialYmm, `${name}: full YMM144`);
    for (const [address, bytes] of expectedPages) assert.deepEqual(slice(physical(address), 4096), bytes, `${name}: full RAM page ${address.toString(16)}`);
    const versionDelta = BigInt(options.versionDelta ?? 0), expectedVersions = pageVersions.slice();
    const changedPages = new Set((options.writes ?? []).flatMap(([address, bytes]) =>
      [address & ~4095, (address + bytes.length - 1) & ~4095]));
    for (const address of changedPages) {
      const slot = word(word(context, 56) + (address >>> 12) * 4) - 1;
      new DataView(expectedVersions.buffer).setBigUint64(slot * 16 + 8, initialVersion + versionDelta, true);
    }
    assert.equal(view().getBigUint64(word(context, 80), true), initialVersion + versionDelta, `${name}: transaction version`);
    assert.deepEqual(slice(word(context, 84), expectedVersions.length), expectedVersions, `${name}: all physical page versions`);
    row.final = {simdSha256: hash(wantedSimd), ram: Object.fromEntries([...expectedPages].map(([address, bytes]) => [address.toString(16), hash(bytes)])),
      transactionVersion: String(initialVersion + versionDelta)};
    row.complete = true; checkpoint();
  } finally {
    if (active) api.core_end(); if (opened) api.close();
  }
}

const one = 0x3f800000, two = 0x40000000, three = 0x40400000, four = 0x40800000;
const a = [one, two, three, four], b = [0x3f000000, one, two, four], sum = [0x3fc00000, three, 0x40a00000, 0x41000000];
const payload = [0x76543210, 0xfedcba98, 0x00ff55aa, 0x80000000];
try {
  runCase('direct-xmm-copy-and-nonoverlap-store', {code: '0f2803 0f28c8 0f294b20', budget: 3,
    data: [[0x3000, words(payload)]], expectedXmm: {0: payload, 1: payload}, writes: [[0x3020, words(payload)]], versionDelta: 1});
  runCase('direct-unaligned-move', {code: '0f1003 0f114320', budget: 2, base: 0x3001,
    data: [[0x3001, words(payload)]], expectedXmm: {0: payload}, writes: [[0x3021, words(payload)]], versionDelta: 1});
  runCase('direct-addps-registers', {xmm: {0: a, 1: b}, expectedXmm: {0: sum}});
  runCase('direct-addps-memory', {code: '0f5803', xmm: {0: a}, data: [[0x3000, words(b)]], expectedXmm: {0: sum}});
  runCase('direct-addps-source-alias', {code: '0f58c0', xmm: {0: a}, expectedXmm: {0: [two, four, 0x40c00000, 0x41000000]}});
  runCase('direct-addps-halfway-and-sticky-precision', {mxcsr: 0x1f85,
    xmm: {0: [one, one + 1, one, 0x80000000], 1: [0x33800000, 0x33800000, 0xbf800000, 0x80000000]},
    expectedXmm: {0: [one, one + 2, 0, 0x80000000]}, expectedMxcsr: 0x1fa5});
  runCase('direct-addps-signed-zero', {xmm: {0: [0x80000000, 0, 0x80000000, one], 1: [0x80000000, 0x80000000, 0, 0xbf800000]}, expectedXmm: {0: [0x80000000, 0, 0, 0]}});
  for (const [gap, bits, helpers] of [[29, 0x31000000, 0], [30, 0x30800000, 1]]) {
    runCase(`addps-exponent-gap-${gap}`, {xmm: {0: [one, one, one, one], 1: [bits, bits, bits, bits]}, helpers,
      expectedXmm: {0: [one, one, one, one]}, expectedMxcsr: 0x1fa0});
  }
  for (const [rounding, mxcsr, result] of [['down', 0x3f80, one], ['up', 0x5f80, one + 1], ['zero', 0x7f80, one]]) {
    runCase(`addps-fallback-round-${rounding}`, {mxcsr, xmm: {0: [one, one, one, one], 1: [0x33800000, 0x33800000, 0x33800000, 0x33800000]},
      helpers: 1, expectedXmm: {0: [result, result, result, result]}, expectedMxcsr: mxcsr | 0x20});
  }
  runCase('addps-last-lane-unmasked-invalid-does-not-commit-earlier-lanes', {mxcsr: 0x1f00,
    xmm: {0: [one, two, three, 0x7f800001], 1: b}, helpers: 1, stop: 9, budget: 8, retired: 0, pc: 0x1000,
    expectedMxcsr: 0x1f01, vector: 19, detail: 0x1f01});
  runCase('addps-unmasked-precision', {mxcsr: 0x0f80, xmm: {0: a, 1: [0x33800000, 0, 0, 0]}, helpers: 1,
    stop: 9, budget: 8, retired: 0, pc: 0x1000, expectedMxcsr: 0x0fa0, vector: 19, detail: 0x0fa0});
  runCase('addps-fallback-denormal-input', {xmm: {0: [one, 0, 0, 0], 1: [1, 0, 0, 0]}, helpers: 1,
    expectedXmm: {0: [one, 0, 0, 0]}, expectedMxcsr: 0x1fa2});
  runCase('addps-fallback-daz', {mxcsr: 0x1fc0, xmm: {0: [one, 0, 0, 0], 1: [1, 0, 0, 0]}, helpers: 1,
    expectedXmm: {0: [one, 0, 0, 0]}});
  runCase('addps-fallback-exact-denormal-output', {xmm: {0: [0x00800001, one, one, one], 1: [0x80800000, 0, 0, 0]}, helpers: 1,
    expectedXmm: {0: [1, one, one, one]}});
  runCase('addps-fallback-ftz', {mxcsr: 0x9f80, xmm: {0: [0x00800001, one, one, one], 1: [0x80800000, 0, 0, 0]}, helpers: 1,
    expectedXmm: {0: [0, one, one, one]}, expectedMxcsr: 0x9fb0});
  runCase('addps-fallback-overflow', {xmm: {0: [0x7f7fffff, 0, 0, 0], 1: [0x7f7fffff, 0, 0, 0]}, helpers: 1,
    expectedXmm: {0: [0x7f800000, 0, 0, 0]}, expectedMxcsr: 0x1fa8});
  runCase('move-cross-page-read', {code: '0f1003', base: 0x3ff8, maps: [[0x4000, 3]], data: [[0x3ff8, words(payload)]], helpers: 1, expectedXmm: {0: payload}});
  runCase('move-cross-page-store', {code: '0f1103', base: 0x3ff8, maps: [[0x4000, 3]], xmm: {0: payload}, helpers: 1,
    writes: [[0x3ff8, words(payload)]], versionDelta: 1});
  runCase('move-late-read-fault-is-neutral', {code: '0f1003', base: 0x3ff8, helpers: 1, stop: 3,
    budget: 8, retired: 0, pc: 0x1000, vector: 0x100e, detail: 0x4000});
  runCase('move-late-store-fault-has-no-earlier-bytes-or-version', {code: '0f1103', base: 0x3ff8, maps: [[0x4000, 1]], xmm: {0: payload}, helpers: 1,
    stop: 3, budget: 8, retired: 0, pc: 0x1000, vector: 0x100e, detail: 0x4000});
  runCase('move-alignment-fault', {code: '0f2803', base: 0x3001, helpers: 1, stop: 9, budget: 8, retired: 0,
    pc: 0x1000, vector: 13, detail: 0});
  runCase('move-executable-page-store-invalidates-code', {code: '0f2903', base: 0x1100, xmm: {0: payload}, helpers: 1,
    stop: 6, budget: 8, retired: 1, writes: [[0x1100, words(payload)]], versionDelta: 1});
  report.complete = true; checkpoint(); console.log(JSON.stringify({complete: true, cases: report.cases.length, engineSha256: engine.sha256}));
} catch (error) {
  report.failure = {stage, message: String(error), stack: error.stack}; checkpoint(); throw error;
} finally {
  closeSync(output);
}
