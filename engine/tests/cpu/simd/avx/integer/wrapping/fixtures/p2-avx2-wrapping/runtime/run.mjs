import {pageFaultExit, pageFaultPacket} from '../../../../../../../../support/wasm/page-fault.mjs';
import {ARENA_SIZE} from '../../../../../../../../support/wasm/arena.mjs';
import {generatedImports} from '../../../../../../../../support/wasm/binding.mjs';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { readEngine } from '../../../../../../../../support/wasm/engine.mjs';
import { assertEngineExports } from '../../../../../../../../support/wasm/exports.mjs';

const [enginePath, output, root] = process.argv.slice(2),
  engine = readEngine(enginePath);
const { sourcePaths: exportsSources } = assertEngineExports(engine.module, root);
const PC = 0x1000,
  T = 140,
  SV = 4364,
  YU = 4612,
  SIZE = ARENA_SIZE,
  keyHigh = 0x415658;
const counts = { contexts: 0, modules: 0, runs: 0, controls: 0, faults: 0, fullRamPairs: 0 };
const sourcePaths = ['engine/tests/support/wasm/page-fault.mjs', 'engine/tests/support/wasm/arena.mjs', 'engine/tests/support/wasm/binding.mjs',
  ...exportsSources,
  'engine/tests/support/wasm/engine.mjs',
  'engine/src/cpu/x86/capabilities.rs',
  'engine/src/cpu/x86/decode/avx/mod.rs',
  'engine/src/cpu/x86/decode/avx/encoding.rs',
  'engine/src/cpu/x86/decode/avx/integer.rs',
  'engine/src/cpu/x86/avx/integer.rs',
  'engine/src/cpu/x86/avx/operation.rs',
  'engine/src/cpu/x86/avx/operation/factory/integer.rs',
  'engine/src/cpu/x86/simd/integer.rs',
  'engine/src/cpu/x86/simd/packed.rs',
  'engine/src/cpu/dbt/wasm/instructions/avx/integer.rs',
  'engine/src/cpu/dbt/wasm/instructions/avx/direct.rs',
  'engine/src/cpu/dbt/wasm/instructions/avx/preflight.rs',
  'engine/src/cpu/dbt/wasm/instructions/avx/locals.rs',
  'engine/src/cpu/dbt/wasm/emitter/mod.rs', 'engine/src/cpu/dbt/wasm/emitter/module.rs', 'engine/src/cpu/dbt/wasm/emitter/body.rs', 'engine/src/cpu/dbt/wasm/requirements.rs', 'engine/src/cpu/dbt/wasm/imports/wiring.rs',
  'engine/src/cpu/dbt/wasm/instructions/simd/direct.rs',
  'engine/src/cpu/dbt/wasm/imports/mod.rs', 'engine/src/cpu/dbt/wasm/imports/binding.rs', 'engine/src/cpu/dbt/wasm/imports/context.rs', 'engine/src/cpu/dbt/wasm/imports/needs.rs', 'engine/src/cpu/dbt/wasm/helpers/protocol.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/mod.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/address.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/scalar.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/stack.rs',
  'engine/src/cpu/dbt/wasm/instructions/segments/access.rs',
  'engine/src/process/memory/span.rs',
  'engine/src/process/cpu/avx.rs',
  'engine/src/abi/arena.rs',
  'engine/src/abi/x86/ymm.rs',
  'engine/tests/cpu/simd/avx/integer/wrapping/wasm.rs',
  'engine/tests/cpu/simd/avx/integer/wrapping/fixtures/p2-avx2-wrapping/runtime/run.mjs',
];
assert.equal(new Set(sourcePaths).size, sourcePaths.length);
const hash = (b) => createHash('sha256').update(b).digest('hex');
const pins = () =>
  Object.fromEntries(sourcePaths.map((path) => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = pins();
const bytes = (s) => Buffer.from(s, 'hex'),
  fill = (n, size = 16) => Buffer.alloc(size, n);
const words = (values) => {
  const b = Buffer.alloc(values.length * 4);
  values.forEach((v, i) => b.writeUInt32LE(v >>> 0, i * 4));
  return b;
};
const qwords = (values) => {
  const b = Buffer.alloc(values.length * 8);
  values.forEach((v, i) => b.writeBigUInt64LE(v, i * 8));
  return b;
};
function record(magic, size, values, version = 1) {
  if (magic === 'R3EX' && values[0] === 5) return pageFaultExit({retired: values[1],
    errorCode: 4 | (values[4] === 2 ? 2 : 0) | (values[2] === 2 ? 1 : 0), address: values[3], access: values[4], length: values[5]});
  if (magic === 'R3MH' && values[0] === 1) return pageFaultPacket({
    errorCode: 4 | (values[4] === 2 ? 2 : 0) | (values[2] === 2 ? 1 : 0), address: values[3], access: values[4], length: values[5]});

  const b = Buffer.alloc(size);
  b.write(magic);
  b.writeUInt16LE(version, 4);
  b.writeUInt16LE(1, 6);
  b.writeUInt32LE(size, 8);
  values.forEach((v, i) => b.writeUInt32LE(v >>> 0, 16 + i * 4));
  return b;
}
const state = (r, pc, flags) => record('R3ST', 56, [...r, pc, flags], flags & ~0xcd7 ? 2 : 1);
const arena = (c) => Buffer.from(new Uint8Array(c.memory.buffer, c.base, SIZE));
function input(c, at, value) {
  new Uint8Array(c.memory.buffer).set(value, c.base + at);
}
function host(c, name, args = []) {
  assert.equal(c.api[name](...args), 0, name);
}
function upload(c, address, value) {
  input(c, T, value);
  host(c, 'upload', [address, value.length]);
}
function open(owner, entries) {
  const instance = new WebAssembly.Instance(engine.module, {}),
    c = { owner, entries, memory: instance.exports.memory, api: {}, calls: [] };
  for (const [name, value] of Object.entries(instance.exports))
    if (name.startsWith('ring3_abi_v1_') && typeof value === 'function')
      c.api[name.slice(13)] = value;
  for (const [name, arity] of Object.entries({
    open: 3,
    map: 3,
    upload: 2,
    compile: 1,
    compile_entries: 2,
    compile_resident: 1,
    compile_resident_entries: 2,
    cpu_read: 3,
    cpu_write: 3,
    cpu_write_resident: 7,
    avx_execute: 6,
    avx_execute_resident: 10,
  }))
    assert.equal(c.api[name]?.length, arity, name);
  c.key = ++counts.contexts;
  host(c, 'open', [8, c.key, keyHigh]);
  c.base = c.api.arena_ptr() >>> 0;
  assert.equal(c.api.arena_len(), SIZE, 'canonical upper-vector arena');
  for (const args of [
    [PC, 1, 7],
    [0x5000, 1, 3],
  ])
    host(c, 'map', args);
  c.initial = arena(c);
  return c;
}
function compile(c, row) {
  if (c.resident) {
    input(c, 96, words([0]));
    host(c, 'discard_unacknowledged_resident', [c.key, keyHigh, ...c.resident]);
  }
  const code = bytes(row.code);
  upload(c, PC, Buffer.concat([code, bytes('ebfe')]));
  input(c, T, words(c.entries ? [PC] : [PC, code.length + 2]));
  const name =
    c.owner === 'resident'
      ? c.entries
        ? 'compile_resident_entries'
        : 'compile_resident'
      : c.entries
        ? 'compile_entries'
        : 'compile';
  host(c, name, [1, ...(c.entries ? [0] : [])]);
  const a = arena(c);
  if (c.owner === 'resident') c.resident = [a.readUInt32LE(T + 8), a.readUInt32LE(T + 12)];
  const pointer = c.owner === 'resident' ? a.readUInt32LE(T + 16) : c.api.module_ptr() >>> 0;
  const length = c.owner === 'resident' ? a.readUInt32LE(T + 20) : c.api.module_len() >>> 0;
  const binary = Buffer.from(new Uint8Array(c.memory.buffer, pointer, length)),
    module = new WebAssembly.Module(binary);
  const actual = WebAssembly.Module.imports(module)
    .map((i) => `${i.module}:${i.name}:${i.kind}`)
    .sort();
  const expected = generatedImports({resident: c.owner === 'resident', helpers: [
    ...(row.read ? ['cpu_read'] : []),
    ...(row.write ? [c.owner === 'resident' ? 'cpu_write_resident' : 'cpu_write'] : [])
  ]}).map(i => `${i.module}:${i.name}:${i.kind}`).sort();
  assert.deepEqual(actual, expected, `${row.name}: exact direct-path imports`);
  assert.deepEqual(WebAssembly.Module.exports(module), [{ name: 'run', kind: 'function' }]);
  writeFileSync(join(output, `module-${++counts.modules}.wasm`), binary, { flag: 'wx' });
  const ring3 = { ...c.api };
  for (const helper of ['cpu_read', 'cpu_write', 'cpu_write_resident'])
    ring3[helper] = (...args) => {
      const status = c.api[helper](...args);
      c.calls.push({ helper, status, args: args.map((v) => v >>> 0) });
      return status;
    };
  return {
    run: new WebAssembly.Instance(module, { env: { memory: c.memory }, ring3 }).exports.run,
    codeLength: code.length,
  };
}
function seed(c, row) {
  input(c, 0, c.initial);
  const r = [
    row.address ?? 0x5020,
    0x11223344,
    0x22334455,
    0x33445566,
    0x7780,
    0x55667788,
    0x66778899,
    0x778899aa,
  ];
  if (row.r) for (const [index, value] of Object.entries(row.r)) r[+index] = value >>> 0;
  const flags = 0x244cd7;
  input(c, 0, state(r, PC, flags));
  for (let i = 0; i < 8; i++) {
    input(c, SV + 32 + i * 16, fill(0x21 + i));
    input(c, YU + 16 + i * 16, fill(0x91 + i));
  }
  for (const [index, value] of Object.entries(row.lower ?? {}))
    input(c, SV + 32 + +index * 16, value);
  for (const [index, value] of Object.entries(row.upper ?? {}))
    input(c, YU + 16 + +index * 16, value);
  input(c, SV + 16, words([0xe5af]));
  return { r, flags };
}
function ram(c) {
  const saved = arena(c),
    result = Buffer.alloc(8192);
  for (const [page, address] of [PC, 0x5000].entries())
    for (let offset = 0; offset < 4096; offset += 512) {
      host(c, 'cpu_read', [address + offset, 512, 1]);
      const frame = arena(c);
      assert.equal(frame.readUInt32LE(116), 0, 'mapped RAM snapshot');
      frame.copy(result, page * 4096 + offset, T, T + 512);
    }
  input(c, 0, saved);
  return result;
}
function execute(c, fn, row, { budget = row.retired ?? 1, cancel = false } = {}) {
  const s = seed(c, row);
  if (cancel) input(c, 96, words([1]));
  const before = arena(c),
    expected = Buffer.from(before),
    beforeRam = ram(c),
    first = c.calls.length;
  const retired = budget === 0 || cancel ? 0 : (row.retired ?? (row.reason ? 0 : 1));
  const reason = cancel ? 2 : budget === 0 ? 1 : (row.reason ?? 1);
  const changed = retired > 0;
  const r = [...s.r];
  if (changed) for (const [index, value] of Object.entries(row.outR ?? {})) r[+index] = value >>> 0;
  expected.set(state(r, changed ? PC + fn.codeLength : PC, s.flags));
  expected.set(
    record(
      'R3EX',
      40,
      [
        reason,
        retired,
        row.reason && !cancel && budget ? (row.detail ?? 0) : 0,
        row.reason === 5 && !cancel && budget ? row.fault : 0,
        row.reason === 5 && !cancel && budget ? row.access : 0,
        row.reason === 5 && !cancel && budget ? row.length : 0,
      ],
      6,
    ),
    56,
  );
  if (changed) {
    for (const [index, value] of Object.entries(row.outLower ?? {}))
      expected.set(value, SV + 32 + +index * 16);
    for (const [index, value] of Object.entries(row.outUpper ?? {}))
      expected.set(value, YU + 16 + +index * 16);
  }
  const helpers =
    budget === 0 || cancel ? 0 : (row.helpers ?? Number(Boolean(row.read || row.write)));
  if (helpers) {
    const fault = row.reason === 5;
    expected.set(
      record(
        'R3MH',
        40,
        [
          fault ? 1 : 0,
          0,
          fault ? row.detail : 0,
          fault ? row.fault : 0,
          fault ? row.access : 0,
          row.length ?? row.read ?? row.write,
        ],
        7,
      ),
      100,
    );
    if (!row.reason && row.read) expected.set(row.memory, T);
    if (row.write) expected.set(row.staged, T);
  }
  assert.equal(fn.run(c.base, c.base + 56, budget, c.base + 96), 0, row.name);
  assert.deepEqual(
    arena(c),
    expected,
    `${row.name}: complete canonical arena budget=${budget} cancel=${cancel}`,
  );
  assert.equal(c.calls.length - first, helpers, `${row.name}: exact helper count`);
  for (const call of c.calls.slice(first)) {
    assert.equal(call.status, reason === 5 ? 23 : 0, `${row.name}: helper transport`);
    assert.deepEqual(call.args.slice(-3), [
      row.address ?? s.r[0],
      row.read ?? row.write,
      row.alignment === 32 ? 1 : (row.alignment ?? 1),
    ]);
  }
  const expectedRam = Buffer.from(beforeRam);
  if (row.write && changed) {
    assert.ok(row.checkRam, `${row.name}: literal committed store oracle`);
    assert.ok(row.address >= 0x5000 && row.address + row.checkRam.length <= 0x6000);
    row.checkRam.copy(expectedRam, 4096 + row.address - 0x5000);
  }
  assert.deepEqual(
    ram(c),
    expectedRam,
    `${row.name}: complete mapped RAM budget=${budget} cancel=${cancel}`,
  );
  assert.deepEqual(arena(c), expected, `${row.name}: RAM observations preserve the complete arena`);
  counts.fullRamPairs++;
  if (row.checkRam && changed) {
    host(c, 'cpu_read', [row.address, row.checkRam.length, 1]);
    assert.deepEqual(
      arena(c).subarray(T, T + row.checkRam.length),
      row.checkRam,
      `${row.name}: committed guest RAM`,
    );
  }
  counts.runs++;
  if (row.reason && budget && !cancel) counts.faults++;
  if (!budget || cancel) counts.controls++;
}
const forms = [
  { opcode: 0xfc, kind: 26, lane: 1, subtract: false },
  { opcode: 0xfd, kind: 27, lane: 2, subtract: false },
  { opcode: 0xfe, kind: 28, lane: 4, subtract: false },
  { opcode: 0xd4, kind: 7, lane: 8, subtract: false },
  { opcode: 0xf8, kind: 22, lane: 1, subtract: true },
  { opcode: 0xf9, kind: 23, lane: 2, subtract: true },
  { opcode: 0xfa, kind: 24, lane: 4, subtract: true },
  { opcode: 0xfb, kind: 25, lane: 8, subtract: true },
];
function pattern(lane, seed) {
  const modulus = 1n << BigInt(lane * 8),
    sign = modulus / 2n;
  const values = [0n, modulus - 1n, sign, sign - 1n, 1n, modulus - 2n, sign + 1n, modulus / 3n];
  const value = Buffer.alloc(32);
  for (let at = 0; at < 32; at += lane) {
    let number = values[(at / lane + seed) % values.length];
    for (let byte = 0; byte < lane; byte++) {
      value[at + byte] = Number(number & 255n);
      number >>= 8n;
    }
  }
  return value;
}
function wrapping(form, first, second, width) {
  const value = Buffer.alloc(32),
    modulus = 1n << BigInt(form.lane * 8);
  function read(bytes, offset) {
    let value = 0n;
    for (let byte = form.lane - 1; byte >= 0; byte--)
      value = value * 256n + BigInt(bytes[offset + byte]);
    return value;
  }
  for (let at = 0; at < width; at += form.lane) {
    const a = read(first, at),
      b = read(second, at);
    let number = (form.subtract ? a + modulus - b : a + b) % modulus;
    for (let byte = 0; byte < form.lane; byte++) {
      value[at + byte] = Number(number & 255n);
      number >>= 8n;
    }
  }
  return value;
}
function vectorRows(form, destination, first, second, width, encoding) {
  const values = Object.fromEntries(
    Array.from({ length: 8 }, (_, r) => [r, pattern(form.lane, r * 3 + 1)]),
  );
  const result = wrapping(form, values[first], values[second], width);
  const l = width === 32 ? 4 : 0,
    control = 0x81 | ((15 - first) << 3) | l;
  const prefix =
    encoding === 'c5'
      ? [0xc5, control]
      : [0xc4, 0xe1, (control & 0x7f) | (encoding === 'w1' ? 0x80 : 0)];
  return {
    name: `${form.subtract ? 'sub' : 'add'}${form.lane}-width${width}-${destination}/${first}/${second}-${encoding}`,
    code: Buffer.from([...prefix, form.opcode, 0xc0 | (destination << 3) | second]).toString('hex'),
    lower: Object.fromEntries(Object.entries(values).map(([r, v]) => [r, v.subarray(0, 16)])),
    upper: Object.fromEntries(Object.entries(values).map(([r, v]) => [r, v.subarray(16)])),
    outLower: { [destination]: result.subarray(0, 16) },
    outUpper: { [destination]: result.subarray(16) },
  };
}
const rows = [];
for (const form of forms) {
  for (const [d, a, b] of [
    [0, 1, 2],
    [0, 0, 2],
    [0, 1, 0],
    [0, 1, 1],
    [0, 0, 0],
    [7, 6, 7],
  ]) {
    for (const encoding of ['c5', 'w0', 'w1']) rows.push(vectorRows(form, d, a, b, 32, encoding));
  }
  for (const encoding of ['w0', 'w1']) rows.push(vectorRows(form, 7, 6, 7, 16, encoding));
  for (const address of [0x5011, 0x5fe0, 0x5fe1]) {
    const first = pattern(form.lane, 1),
      second = pattern(form.lane, 7);
    const result = wrapping(form, first, second, 32),
      fault = address === 0x5fe1;
    rows.push({
      name: `memory${form.lane}-${form.subtract}-${address.toString(16)}`,
      code: Buffer.from([0xc5, 0xfd, form.opcode, 0x00]).toString('hex'),
      address,
      read: 32,
      memory: second.subarray(0, fault ? 31 : 32),
      lower: { 0: first.subarray(0, 16) },
      upper: { 0: first.subarray(16) },
      ...(fault
        ? { reason: 5, detail: 1, fault: 0x6000, access: 1, length: 32 }
        : { outLower: { 0: result.subarray(0, 16) }, outUpper: { 0: result.subarray(16) } }),
    });
  }
}
assert.equal(rows.length, 184);
for (const owner of ['replacement', 'resident'])
  for (const entries of [false, true]) {
    const c = open(owner, entries);
    for (const row of rows) {
      const fn = compile(c, row);
      if (row.memory) upload(c, row.address, row.memory);
      if (row.write && !row.memory && row.address < 0x6000)
        upload(c, row.address, fill(0x66, row.write));
      execute(c, fn, row);
      execute(c, fn, row, { budget: 0 });
      execute(c, fn, row, { budget: row.retired ?? 1, cancel: true });
    }
    host(c, 'close');
  }
assert.deepEqual(counts, {
  contexts: 4,
  modules: 736,
  runs: 2208,
  controls: 1472,
  faults: 32,
  fullRamPairs: 2208,
});
assert.deepEqual(pins(), sourcePins, 'pinned sources unchanged during proof');
writeFileSync(
  join(output, 'result.json'),
  JSON.stringify(
    {
      scope:
        'private enabled AVX2 wrapping direct integration; modular BigInt oracle; full arena and mapped RAM',
      engineSha256: engine.sha256,
      counts,
      sourcePins,
    },
    null,
    2,
  ) + '\n',
);
console.log(JSON.stringify({ status: 'PASS', ...counts }));
