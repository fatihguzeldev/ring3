import {pageFaultExit, pageFaultPacket} from '../../../../../../../support/wasm/page-fault.mjs';
import {ARENA_SIZE} from '../../../../../../../support/wasm/arena.mjs';
import {generatedImports} from '../../../../../../../support/wasm/binding.mjs';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { readEngine } from '../../../../../../../support/wasm/engine.mjs';
import { assertEngineExports } from '../../../../../../../support/wasm/exports.mjs';
import { buildCases, descriptor, encoding, memoryBytes } from './cases.mjs';

const [enginePath, output, root] = process.argv.slice(2);
const engine = readEngine(enginePath);
const exportsReceipt = assertEngineExports(engine.module, root);
const PC = 0x1000;
const T = 140;
const SV = 4364;
const YU = 4612;
const SG = 4524;
const SIZE = ARENA_SIZE;
const KEY_HIGH = 0x464d4133;
const counts = {
  contexts: 0,
  modules: 0,
  runs: 0,
  faults: 0,
  controls: 0,
  refusals: 0,
  authorityRefusals: 0,
  fullRamPairs: 0,
};
const observations = [];
const frames = [];
const sourcePaths = ['engine/tests/support/wasm/page-fault.mjs', 'engine/tests/support/wasm/arena.mjs', 'engine/tests/support/wasm/binding.mjs',
  ...exportsReceipt.sourcePaths,
  'engine/tests/support/wasm/engine.mjs',
  'engine/tests/cpu/simd/avx/fma/native.rs',
  'engine/tests/cpu/simd/avx/fma/wasm.rs',
  'engine/tests/cpu/simd/avx/fma/fixtures/p2-fma/runtime/run.mjs',
  'engine/tests/cpu/simd/avx/fma/fixtures/p2-fma/runtime/cases.mjs',
  'engine/src/math/ieee.rs',
  'engine/src/math/ieee/fused.rs',
  'engine/src/math/ieee/fused/native.rs',
  'engine/src/cpu/x86/simd/float.rs',
  'engine/src/cpu/x86/simd/float/fused.rs',
  'engine/src/cpu/x86/avx/fma/mod.rs',
  'engine/src/cpu/x86/avx/fma/native.rs',
  'engine/src/cpu/x86/avx/fma/numeric/native.rs',
  'engine/src/cpu/x86/avx/fma/state/native.rs',
  'engine/src/cpu/x86/avx/fma/exceptions/native.rs',
  'engine/src/cpu/x86/avx/operation.rs',
  'engine/src/cpu/x86/avx/operation/factory.rs',
  'engine/src/cpu/x86/avx/operation/factory/fma.rs',
  'engine/src/cpu/x86/avx/dispatch.rs',
  'engine/src/cpu/x86/decode/avx/mod.rs',
  'engine/src/cpu/x86/decode/avx/encoding.rs',
  'engine/src/cpu/x86/decode/avx/fma.rs',
  'engine/src/cpu/x86/decode/lower.rs',
  'engine/src/cpu/x86/capabilities.rs',
  'engine/src/cpu/dbt/wasm/avx/execute.rs',
  'engine/src/cpu/dbt/wasm/cpu_result.rs',
  'engine/src/process/cpu/avx.rs',
  'engine/src/abi/x86/avx.rs',
  'engine/src/abi/x86/simd.rs',
  'engine/src/abi/x86/ymm.rs',
];
assert.equal(new Set(sourcePaths).size, sourcePaths.length);
const hash = (bytes) => createHash('sha256').update(bytes).digest('hex');
const pins = () =>
  Object.fromEntries(sourcePaths.map((path) => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = pins();

function words(values) {
  const bytes = Buffer.alloc(values.length * 4);
  values.forEach((value, index) => bytes.writeUInt32LE(value >>> 0, index * 4));
  return bytes;
}

function record(magic, size, values, version = 1) {
  if (magic === 'R3EX' && values[0] === 5) return pageFaultExit({retired: values[1],
    errorCode: 4 | (values[4] === 2 ? 2 : 0) | (values[2] === 2 ? 1 : 0), address: values[3], access: values[4], length: values[5]});
  if (magic === 'R3MH' && values[0] === 1) return pageFaultPacket({
    errorCode: 4 | (values[4] === 2 ? 2 : 0) | (values[2] === 2 ? 1 : 0), address: values[3], access: values[4], length: values[5]});

  const bytes = Buffer.alloc(size);
  bytes.write(magic);
  bytes.writeUInt16LE(version, 4);
  bytes.writeUInt16LE(1, 6);
  bytes.writeUInt32LE(size, 8);
  bytes.set(words(values), 16);
  return bytes;
}

const state = (registers, pc, flags) => record('R3ST', 56, [...registers, pc, flags], 2);
const arena = (c) => Buffer.from(new Uint8Array(c.memory.buffer, c.base, SIZE));
const input = (c, offset, bytes) => new Uint8Array(c.memory.buffer).set(bytes, c.base + offset);

function host(c, name, args = []) {
  assert.equal(c.api[name](...args), 0, name);
}

function upload(c, address, bytes) {
  input(c, T, bytes);
  host(c, 'upload', [address, bytes.length]);
}

function helper(c, ...args) {
  return c.owner === 'resident'
    ? c.api.avx_execute_resident(c.key, KEY_HIGH, ...c.resident, ...args)
    : c.api.avx_execute(...args);
}

function vector(frame, register) {
  return Buffer.concat([
    frame.subarray(SV + 32 + register * 16, SV + 48 + register * 16),
    frame.subarray(YU + 16 + register * 16, YU + 32 + register * 16),
  ]);
}

function publish(frame, register, value) {
  assert.equal(value.length, 32);
  frame.set(value.subarray(0, 16), SV + 32 + register * 16);
  frame.set(value.subarray(16), YU + 16 + register * 16);
}

function open(owner, entries) {
  const instance = new WebAssembly.Instance(engine.module, {});
  const c = { owner, entries, memory: instance.exports.memory, api: {}, calls: [] };
  for (const { name, arity } of exportsReceipt.functions) {
    assert.equal(instance.exports[name].length, arity, name);
    c.api[name.slice(13)] = instance.exports[name];
  }
  c.key = ++counts.contexts;
  host(c, 'open', [4, c.key, KEY_HIGH]);
  c.base = c.api.arena_ptr() >>> 0;
  assert.equal(c.api.arena_len(), SIZE);
  host(c, 'map', [PC, 1, 7]);
  host(c, 'map', [0x5000, 1, 3]);
  c.permissions = 3;
  for (let at = 0; at < 4096; at += 256) {
    upload(
      c,
      0x5000 + at,
      Buffer.from(Array.from({ length: 256 }, (_, index) => (index * 29 + at + 7) & 255)),
    );
  }
  c.initial = arena(c);
  return c;
}

function compile(c, row) {
  if (c.resident) {
    input(c, 96, words([0]));
    host(c, 'discard_unacknowledged_resident', [c.key, KEY_HIGH, ...c.resident]);
  }
  const code = encoding(row);
  upload(c, PC, Buffer.concat([code, Buffer.from([0xeb, 0xfe])]));
  input(c, T, words(c.entries ? [PC] : [PC, code.length + 2]));
  const name =
    c.owner === 'resident'
      ? c.entries
        ? 'compile_resident_entries'
        : 'compile_resident'
      : c.entries
        ? 'compile_entries'
        : 'compile';
  assert.equal(c.api[name](1, ...(c.entries ? [0] : [])), 0, `${row.name}: compile`);
  const frame = arena(c);
  if (c.owner === 'resident') {
    c.resident = [frame.readUInt32LE(T + 8), frame.readUInt32LE(T + 12)];
  }
  const pointer = c.owner === 'resident' ? frame.readUInt32LE(T + 16) : c.api.module_ptr() >>> 0;
  const length = c.owner === 'resident' ? frame.readUInt32LE(T + 20) : c.api.module_len() >>> 0;
  const binary = Buffer.from(new Uint8Array(c.memory.buffer, pointer, length));
  const module = new WebAssembly.Module(binary);
  const helperName = c.owner === 'resident' ? 'avx_execute_resident' : 'avx_execute';
  assert.deepEqual(
    WebAssembly.Module.imports(module)
      .map((item) => `${item.module}:${item.name}:${item.kind}`)
      .sort(),
    generatedImports({resident: c.owner === 'resident', helpers: [helperName]}).map(i => `${i.module}:${i.name}:${i.kind}`).sort(),
    row.name,
  );
  assert.deepEqual(WebAssembly.Module.exports(module), [{ name: 'run', kind: 'function' }]);
  const ring3 = {
    ...c.api,
    [helperName]: (...args) => {
      const status = c.api[helperName](...args);
      c.calls.push({ args, status });
      return status;
    },
  };
  writeFileSync(join(output, `module-${++counts.modules}.wasm`), binary, { flag: 'wx' });
  return {
    run: new WebAssembly.Instance(module, { env: { memory: c.memory }, ring3 }).exports.run,
    length: code.length,
  };
}

function permissions(c, value) {
  if (c.permissions === value) return;
  host(c, 'protect', [0x5000, 1, value]);
  c.permissions = value;
}

function ram(c) {
  const saved = arena(c);
  const savedPermissions = c.permissions;
  const result = Buffer.alloc(8192);
  // Diagnostics restore both permission and arena; denied guest reads remain denied.
  permissions(c, 3);
  for (const [page, address] of [PC, 0x5000].entries()) {
    for (let at = 0; at < 4096; at += 512) {
      host(c, 'cpu_read', [address + at, 512, 1]);
      const frame = arena(c);
      assert.equal(frame.readUInt32LE(116), 0);
      frame.copy(result, page * 4096 + at, T, T + 512);
    }
  }
  permissions(c, savedPermissions);
  input(c, 0, saved);
  return result;
}

function seed(c, row) {
  input(c, 0, c.initial);
  const registers = [
    row.address ?? 0x5020,
    0x11223344,
    0x22334455,
    0x33445566,
    0x7780,
    0x55667788,
    0x66778899,
    0x778899aa,
  ];
  const flags = 0x244cd7;
  input(c, 0, state(registers, PC, flags));
  for (let index = 0; index < 8; index++) {
    input(c, SV + 32 + index * 16, Buffer.alloc(16, 0x21 + index));
    input(c, YU + 16 + index * 16, Buffer.alloc(16, 0x91 + index));
  }
  const frame = new Uint8Array(c.memory.buffer, c.base, SIZE);
  for (const [index, bytes] of Object.entries(row.vectors)) publish(frame, +index, bytes);
  input(c, SV + 16, words([row.mxcsr ?? 0x1f80]));
  input(c, T + 512, Buffer.alloc(32, 0xa5));
  input(c, T + 544, Buffer.alloc(24, 0xff));
  if (row.nullDs) input(c, SG + 22, Buffer.alloc(2));
  if (row.cache) {
    input(c, SG + 4, Buffer.from([2, 0]));
    const defaults = [0xf33fffff, 0xf3bfffff, 0xf33fffff, 0xf33fffff, 0xf33fffff, 0];
    [28, 32, 36, 40, 44, 56].forEach((at, index) =>
      input(c, SG + at, words([row.cache[index] ?? defaults[index]])),
    );
  }
  return { registers, flags };
}

function execute(c, compiled, row, budget = 1, cancel = false) {
  const stateInput = seed(c, row);
  if (cancel) input(c, 96, words([1]));
  const before = arena(c);
  const expected = Buffer.from(before);
  const beforeRam = ram(c);
  const first = c.calls.length;
  const active = budget > 0 && !cancel;
  const reason = !active ? (cancel ? 2 : 1) : (row.reason ?? 1);
  const retired = active && !row.reason ? 1 : 0;
  const bytes = row.memory ? memoryBytes(row) : 0;
  expected.set(state(stateInput.registers, retired ? PC + compiled.length : PC, stateInput.flags));
  expected.set(
    record(
      'R3EX',
      40,
      [
        reason,
        retired,
        active ? (row.detail ?? 0) : 0,
        active ? (row.fault ?? 0) : 0,
        active ? (row.access ?? 0) : 0,
        active && reason === 5 ? bytes : 0,
      ],
      6,
    ),
    56,
  );
  if (active) {
    if (row.memory) {
      expected.set(record('R3AM', 24, [row.segment ?? 3, row.address ?? 0x5020]), T + 544);
    }
    const status = reason === 5 ? 1 : reason === 13 ? 5 : reason === 15 ? 7 : reason === 11 ? 3 : 0;
    expected.set(
      record('R3MH', 40, [status, 0, row.detail ?? 0, row.fault ?? 0, row.access ?? 0, bytes], 7),
      100,
    );
    if (retired) {
      expected.fill(0, T + 512, T + 544);
      expected.set(words([0, stateInput.flags]), T + 512);
      expected.set(words([row.after ?? row.mxcsr ?? 0x1f80]), SV + 16);
      const result = Buffer.alloc(32);
      if (row.scalar) vector(before, row.destination).copy(result, 0, 0, 16);
      row.result.copy(result);
      publish(expected, row.destination, result);
    } else if (reason === 13) {
      expected.set(words([row.detail]), SV + 16);
    }
  }
  assert.equal(compiled.run(c.base, c.base + 56, budget, c.base + 96), 0, row.name);
  const after = arena(c);
  assert.deepEqual(
    after,
    expected,
    `${row.name}: full4796B arena budget=${budget} cancel=${cancel}`,
  );
  const observed = c.calls.slice(first);
  assert.equal(observed.length, Number(active), row.name);
  if (active) {
    assert.equal(observed[0].status, reason === 5 ? 23 : 0, row.name);
    const operands = row.destination | (row.second << 3) | (row.memory ? 1 << 12 : row.third << 6);
    assert.deepEqual(
      observed[0].args.slice(-6),
      [descriptor(row), operands, 0, 0, 0, stateInput.flags],
      `${row.name}: canonical helper arguments`,
    );
    if (c.owner === 'resident') {
      assert.deepEqual(observed[0].args.slice(0, 4), [c.key, KEY_HIGH, ...c.resident]);
    }
  }
  assert.deepEqual(ram(c), beforeRam, `${row.name}: all8192 mapped RAM bytes unchanged`);
  assert.deepEqual(arena(c), expected, `${row.name}: diagnostic reads preserve full arena`);
  observations.push({
    context: c.key,
    owner: c.owner,
    entries: c.entries,
    name: row.name,
    group: row.group,
    reason,
    retired,
    budget,
    cancel,
  });
  frames.push(before, after);
  counts.runs++;
  counts.fullRamPairs++;
  if (!active) counts.controls++;
  if (active && row.reason) counts.faults++;
}

function run(c, row) {
  permissions(c, 3);
  if (row.memory && row.initialize !== false) {
    const address = row.address ?? 0x5020;
    assert.ok(address >= 0x5000 && address + row.payload.length <= 0x6000, row.name);
    upload(c, address, row.payload);
  }
  permissions(c, row.permissions ?? 3);
  const compiled = compile(c, row);
  execute(c, compiled, row);
  if (row.controls) {
    execute(c, compiled, row, 0);
    execute(c, compiled, row, 1, true);
  }
  permissions(c, 3);
}

function neutral(c, label, action, expectedStatus = 7) {
  const before = arena(c);
  const beforeRam = ram(c);
  assert.equal(action(), expectedStatus, label);
  assert.deepEqual(arena(c), before, `${label}: whole arena unchanged`);
  assert.deepEqual(ram(c), beforeRam, `${label}: whole RAM unchanged`);
  assert.deepEqual(arena(c), before);
  counts.refusals++;
}

const cases = buildCases();
const planned = {
  modules: cases.length + 1,
  runs: cases.reduce((sum, row) => sum + (row.controls ? 3 : 1), 0),
  controls: cases.filter((row) => row.controls).length * 2,
  faults: cases.filter((row) => row.reason).length,
};
for (const owner of ['replacement', 'resident']) {
  for (const entries of [false, true]) {
    const c = open(owner, entries);
    for (const row of cases) run(c, row);

    const control = cases.find((row) => row.group === 'family' && row.scalar && !row.memory);
    compile(c, control);
    seed(c, control);
    assert.ok(
      c.calls.some((call) => call.status === 0),
      'authorized helper already executed',
    );
    const malformed = [
      [0x02001301, 0, 0, 0, 0, 2],
      [0x01001300, 0, 0, 0, 0, 2],
      [0x01001361, 0, 0, 0, 0, 2],
      [0x01011301, 0, 0, 0, 0, 2],
      [0x01001301, 1 << 9, 0, 0, 0, 2],
      [0x01001301, 1 << 13, 0, 0, 0, 2],
      [0x01001301, (1 << 12) | (1 << 6), 0, 0, 0, 2],
      [0x01001301, 0, 1, 0, 0, 2],
      [0x01001301, 0, 256, 0, 0, 2],
      [0x01001301, 0, 0, 1, 0, 2],
      [0x01001301, 0, 0, 0, 1, 2],
      [0x01001301, 0, 0, 0, 0, 0],
    ];
    malformed.forEach((args, index) =>
      neutral(c, `strict-descriptor-live-flags-${index}`, () => helper(c, ...args)),
    );
    for (const [label, offset, bytes] of [
      ['magic', 0, Buffer.from('R3AX')],
      ['version', 4, Buffer.from([2, 0])],
      ['reserved', 12, words([1])],
      ['segment', 16, words([6])],
    ]) {
      input(c, T + 544, record('R3AM', 24, [3, 0x5020]));
      input(c, T + 544 + offset, bytes);
      neutral(c, `strict-R3AM-${label}`, () => helper(c, 0x01001301, 1 << 12, 0, 0, 0, 2));
    }
    if (owner === 'resident') {
      neutral(
        c,
        'wrong-resident-key-precedes-descriptor',
        () => c.api.avx_execute_resident(c.key + 1, KEY_HIGH, ...c.resident, 0, 0, 0, 0, 0, 2),
        3,
      );
      counts.authorityRefusals++;
    }
    host(c, 'close');
  }
}

assert.deepEqual(pins(), sourcePins);
assert.equal(counts.contexts, 4);
assert.equal(counts.modules, planned.modules * 4);
assert.equal(counts.runs, planned.runs * 4);
assert.equal(counts.controls, planned.controls * 4);
assert.equal(counts.faults, planned.faults * 4);
assert.equal(counts.refusals, 16 * 4 + 2);
assert.equal(counts.authorityRefusals, 2);
assert.equal(counts.fullRamPairs, counts.runs);
writeFileSync(join(output, 'frames.bin'), Buffer.concat(frames));
writeFileSync(
  join(output, 'result.json'),
  JSON.stringify(
    {
      engine: { path: enginePath, sha256: engine.sha256 },
      counts,
      planned,
      sourcePins,
      observations,
      scope:
        'all96 canonical FMA forms in four capture contexts; integer/literal independent numeric oracles; full4796B arena and all8192 mapped RAM; deterministic Ring3 cached-span then page then numeric authority policy',
    },
    null,
    2,
  ) + '\n',
);
console.log(JSON.stringify(counts));
