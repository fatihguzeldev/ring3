import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { readEngine } from '../../../../../../../support/wasm/engine.mjs';
import { assertEngineExports } from '../../../../../../../support/wasm/exports.mjs';

const [enginePath, output, root] = process.argv.slice(2),
  engine = readEngine(enginePath);
const exportsReceipt = assertEngineExports(engine.module, root);
const PC = 0x1000,
  T = 140,
  SV = 4364,
  YU = 4612,
  SG = 4524,
  SIZE = 4756,
  KEY_HIGH = 0x46313643;
const counts = {
  contexts: 0,
  modules: 0,
  runs: 0,
  controls: 0,
  faults: 0,
  neutral: 0,
  fullRamPairs: 0,
};
const sourcePaths = [
  ...exportsReceipt.sourcePaths,
  'engine/tests/support/wasm/engine.mjs',
  'engine/tests/cpu/simd/avx/f16c/native.rs',
  'engine/tests/cpu/simd/avx/f16c/wasm.rs',
  'engine/tests/cpu/simd/avx/f16c/fixtures/p2-f16c/runtime/run.mjs',
  'engine/src/math/ieee.rs',
  'engine/src/math/ieee/half.rs',
  'engine/src/math/ieee/half/tests.rs',
  'engine/src/cpu/x86/avx/f16c.rs',
  'engine/src/cpu/x86/avx/f16c/tests.rs',
  'engine/src/cpu/x86/avx/operation.rs',
  'engine/src/cpu/x86/avx/operation/factory.rs',
  'engine/src/cpu/x86/avx/operation/factory/f16c.rs',
  'engine/src/cpu/x86/avx/dispatch.rs',
  'engine/src/cpu/x86/decode/avx/mod.rs',
  'engine/src/cpu/x86/decode/avx/f16c.rs',
  'engine/src/cpu/x86/decode/lower.rs',
  'engine/src/cpu/x86/capabilities.rs',
  'engine/src/cpu/dbt/wasm/avx/execute.rs',
  'engine/src/cpu/dbt/wasm/cpu_result.rs',
  'engine/src/process/cpu/avx.rs',
  'engine/src/memory/space.rs', 'engine/src/memory/space/atomic.rs',
  'engine/src/abi/x86/avx.rs',
  'engine/src/abi/x86/simd.rs',
  'engine/src/abi/x86/ymm.rs',
];
assert.equal(new Set(sourcePaths).size, sourcePaths.length);
const hash = (b) => createHash('sha256').update(b).digest('hex');
const pins = () =>
  Object.fromEntries(sourcePaths.map((path) => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = pins(),
  rows = [],
  frames = [];
function words(values, size = 4) {
  const b = Buffer.alloc(values.length * size);
  values.forEach((v, i) =>
    size === 4 ? b.writeUInt32LE(v >>> 0, i * size) : b.writeUInt16LE(v, i * size),
  );
  return b;
}
function record(magic, size, values, version = 1) {
  const b = Buffer.alloc(size);
  b.write(magic);
  b.writeUInt16LE(version, 4);
  b.writeUInt16LE(1, 6);
  b.writeUInt32LE(size, 8);
  b.set(words(values), 16);
  return b;
}
const state = (r, pc, flags) => record('R3ST', 56, [...r, pc, flags], 2);
const arena = (c) => Buffer.from(new Uint8Array(c.memory.buffer, c.base, SIZE));
const input = (c, offset, bytes) => new Uint8Array(c.memory.buffer).set(bytes, c.base + offset);
function host(c, name, args = []) {
  assert.equal(c.api[name](...args), 0, name);
}
function helper(c, ...args) {
  return c.owner === 'resident'
    ? c.api.avx_execute_resident(c.key, KEY_HIGH, ...c.resident, ...args)
    : c.api.avx_execute(...args);
}
function upload(c, address, bytes) {
  input(c, T, bytes);
  host(c, 'upload', [address, bytes.length]);
}
function vector(a, index) {
  return Buffer.concat([
    a.subarray(SV + 32 + index * 16, SV + 48 + index * 16),
    a.subarray(YU + 16 + index * 16, YU + 32 + index * 16),
  ]);
}
function publish(a, index, bytes) {
  const full = Buffer.alloc(32);
  bytes.copy(full);
  a.set(full.subarray(0, 16), SV + 32 + index * 16);
  a.set(full.subarray(16), YU + 16 + index * 16);
}
// integer decomposition, independent of the Rust conversion adapter.
function widen(bits) {
  const sign = (bits & 0x8000) << 16,
    e = (bits >>> 10) & 31,
    f = bits & 1023;
  if (e === 31) return (sign | 0x7f800000 | (f << 13) | (f ? 0x400000 : 0)) >>> 0;
  if (e) return (sign | ((e + 112) << 23) | (f << 13)) >>> 0;
  if (!f) return sign >>> 0;
  const top = 31 - Math.clz32(f);
  return (sign | ((top + 103) << 23) | ((f - (1 << top)) << (23 - top))) >>> 0;
}
function open(owner, entries) {
  const instance = new WebAssembly.Instance(engine.module, {}),
    c = { owner, entries, memory: instance.exports.memory, api: {}, calls: [] };
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
  for (let at = 0; at < 4096; at += 256)
    upload(
      c,
      0x5000 + at,
      Buffer.from(Array.from({ length: 256 }, (_, i) => (i * 29 + at + 7) & 255)),
    );
  c.initial = arena(c);
  return c;
}
function compile(c, row) {
  if (c.resident) {
    input(c, 96, words([0]));
    host(c, 'discard_unacknowledged_resident', [c.key, KEY_HIGH, ...c.resident]);
  }
  const control = row.width === 16 ? 0x79 : 0x7d,
    modrm = row.memory
      ? (row.narrow ? row.source : row.destination) << 3
      : 0xc0 |
        ((row.narrow ? row.source : row.destination) << 3) |
        (row.narrow ? row.destination : row.source);
  const code = Buffer.from([
    ...(row.prefix ?? []),
    0xc4,
    row.narrow ? 0xe3 : 0xe2,
    control,
    row.narrow ? 0x1d : 0x13,
    modrm,
    ...(row.narrow ? [row.immediate ?? 0] : []),
  ]);
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
  const a = arena(c);
  if (c.owner === 'resident') c.resident = [a.readUInt32LE(T + 8), a.readUInt32LE(T + 12)];
  const pointer = c.owner === 'resident' ? a.readUInt32LE(T + 16) : c.api.module_ptr() >>> 0,
    length = c.owner === 'resident' ? a.readUInt32LE(T + 20) : c.api.module_len() >>> 0;
  const binary = Buffer.from(new Uint8Array(c.memory.buffer, pointer, length)),
    module = new WebAssembly.Module(binary),
    helper = c.owner === 'resident' ? 'avx_execute_resident' : 'avx_execute';
  assert.deepEqual(
    WebAssembly.Module.imports(module)
      .map((i) => `${i.module}:${i.name}:${i.kind}`)
      .sort(),
    [
      'env:memory:memory',
      `ring3:${c.owner === 'resident' ? 'guard_resident' : 'guard'}:function`,
      `ring3:${helper}:function`,
    ].sort(),
    row.name,
  );
  assert.deepEqual(WebAssembly.Module.exports(module), [{ name: 'run', kind: 'function' }]);
  const ring3 = {
    ...c.api,
    [helper]: (...args) => {
      const status = c.api[helper](...args);
      c.calls.push({ helper, args, status });
      return status;
    },
  };
  writeFileSync(join(output, `module-${++counts.modules}.wasm`), binary, { flag: 'wx' });
  return {
    run: new WebAssembly.Instance(module, { env: { memory: c.memory }, ring3 }).exports.run,
    length: code.length,
  };
}
function ram(c) {
  const saved = arena(c),
    result = Buffer.alloc(8192);
  for (const [i, address] of [PC, 0x5000].entries())
    for (let at = 0; at < 4096; at += 512) {
      host(c, 'cpu_read', [address + at, 512, 1]);
      const a = arena(c);
      assert.equal(a.readUInt32LE(116), 0);
      a.copy(result, i * 4096 + at, T, T + 512);
    }
  input(c, 0, saved);
  return result;
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
    ],
    flags = 0x244cd7;
  input(c, 0, state(r, PC, flags));
  for (let i = 0; i < 8; i++) {
    input(c, SV + 32 + i * 16, Buffer.alloc(16, 0x21 + i));
    input(c, YU + 16 + i * 16, Buffer.alloc(16, 0x91 + i));
  }
  const src = Buffer.alloc(32, 0xa5);
  row.input.copy(src);
  input(c, SV + 32 + row.source * 16, src.subarray(0, 16));
  input(c, YU + 16 + row.source * 16, src.subarray(16));
  input(c, SV + 16, words([row.mxcsr ?? 0x1f80]));
  input(c, T + 512, Buffer.alloc(32, 0xa5));
  input(c, T + 544, Buffer.alloc(24, 0xff));
  if (row.nullDs) input(c, SG + 22, Buffer.alloc(2));
  if (row.cache) {
    input(c, SG + 4, Buffer.from([2, 0]));
    const defaults = [0xf33fffff, 0xf3bfffff, 0xf33fffff, 0xf33fffff, 0xf33fffff, 0];
    [28, 32, 36, 40, 44, 56].forEach((at, i) =>
      input(c, SG + at, words([row.cache[i] ?? defaults[i]])),
    );
  }
  return { r, flags };
}
function execute(c, fn, row, budget = 1, cancel = false) {
  const s = seed(c, row);
  if (cancel) input(c, 96, words([1]));
  const before = arena(c),
    expected = Buffer.from(before),
    beforeRam = ram(c),
    first = c.calls.length;
  const active = budget > 0 && !cancel,
    reason = !active ? (cancel ? 2 : 1) : (row.reason ?? 1),
    retired = active && (row.committed ?? !row.reason) ? 1 : 0,
    length = row.memory ? row.width / 2 : 0;
  expected.set(state(s.r, retired ? PC + fn.length : PC, s.flags));
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
        active && reason === 5 ? length : 0,
      ],
      6,
    ),
    56,
  );
  if (active) {
    if (row.memory)
      expected.set(record('R3AM', 24, [row.segment ?? 3, row.address ?? 0x5020]), T + 544);
    const status = reason === 5 ? 1 : reason === 13 ? 5 : reason === 15 ? 7 : reason === 11 ? 3 : 0;
    expected.set(
      record('R3MH', 40, [status, 0, row.detail ?? 0, row.fault ?? 0, row.access ?? 0, length], 7),
      100,
    );
    if (retired) {
      expected.fill(0, T + 512, T + 544);
      expected.set(words([0, s.flags]), T + 512);
      expected.set(words([row.after ?? row.mxcsr ?? 0x1f80]), SV + 16);
      if (!row.memory || !row.narrow) publish(expected, row.destination, row.output);
    } else if (reason === 13) expected.set(words([row.detail]), SV + 16);
  }
  assert.equal(fn.run(c.base, c.base + 56, budget, c.base + 96), 0, row.name);
  const after = arena(c);
  assert.deepEqual(
    after,
    expected,
    `${row.name}: full4756B arena budget=${budget} cancel=${cancel}`,
  );
  const observed = c.calls.slice(first);
  assert.equal(observed.length, active ? 1 : 0, row.name);
  if (active) {
    assert.equal(observed[0].status, reason === 6 ? 11 : 0);
    const args = observed[0].args.slice(-6);
    const descriptor =
      0x01001200 + (row.narrow ? (row.width === 16 ? 3 : 4) : row.width === 16 ? 1 : 2);
    const operands = row.memory
      ? row.narrow
        ? (row.source << 6) | (1 << 13)
        : row.destination | (1 << 12)
      : row.destination | (row.source << 6);
    assert.deepEqual(
      args,
      [descriptor, operands, row.narrow ? (row.immediate ?? 0) : 0, 0, 0, s.flags],
      `${row.name}: exact canonical helper arguments`,
    );
  }
  const expectedRam = Buffer.from(beforeRam);
  if (retired && row.memory && row.narrow)
    row.output.copy(
      expectedRam,
      (row.address ?? 0x5020) < 0x2000
        ? (row.address ?? 0x5020) - PC
        : 4096 + (row.address ?? 0x5020) - 0x5000,
    );
  assert.deepEqual(ram(c), expectedRam, `${row.name}: all mapped RAM unchanged or exact store`);
  assert.deepEqual(arena(c), expected);
  rows.push({
    context: c.key,
    profile: c.owner,
    entries: c.entries,
    name: row.name,
    budget,
    cancel,
    reason,
    retired,
  });
  frames.push(before, after);
  counts.runs++;
  counts.fullRamPairs++;
  if (!active) counts.controls++;
  if (active && row.reason) counts.faults++;
}
const half = [0x3c00, 0xc000, 1, 0x0400, 0x7bff, 0x8000, 0x7c00, 0xfc00],
  single = half.map(widen);
const literals = [
  ['exact-tiny', 0x33800000, 1, 0x1f80, 0x1f80, 0],
  ['FTZ-ignored', 0x33800000, 1, 0x9f80, 0x9f80, 0],
  ['tiny-midpoint', 0x33000000, 0, 0x1f80, 0x1fb0, 0],
  ['overflow-zero', 0x47800000, 0x7bff, 0x1f80, 0x1fa8, 3],
  ['DAZ-signed-zero', 0x80000001, 0x8000, 0x1fc0, 0x1fc0, 0],
  ['denormal-masked', 1, 0, 0x1f80, 0x1fb2, 0],
  ['sNaN-quiet', 0x7f800001, 0x7e00, 0x1f80, 0x1f81, 0],
  ['qNaN-payload', 0xffcaa000, 0xfe55, 0x1f80, 0x1f80, 0],
  ['unmasked-invalid', 0x7f800001, 0, 0x1f00, 0x1f01, 0, 13],
  ['unmasked-denormal', 1, 0, 0x1e80, 0x1e82, 0, 13],
  ['unmasked-underflow', 0x33800000, 0, 0x1780, 0x1790, 0, 13],
  ['unmasked-overflow', 0x47800000, 0, 0x1b80, 0x1b88, 0, 13],
  ['unmasked-precision', 0x33000000, 0, 0x0f80, 0x0fb0, 0, 13],
  ['denormal-UM-special', 1, 0, 0x1780, 0x17b2, 0, 13],
];
for (const owner of ['replacement', 'resident'])
  for (const entries of [false, true]) {
    const c = open(owner, entries);
    for (const width of [16, 32])
      for (const narrow of [false, true])
        for (const [destination, source] of [
          [0, 1],
          [1, 1],
          [7, 0],
          [0, 7],
        ]) {
          const row = {
            name: `aliases-${width}-${narrow}-${destination}-${source}`,
            width,
            narrow,
            destination,
            source,
            input: words((narrow ? single : half).slice(0, width / 4), narrow ? 4 : 2),
            output: words((narrow ? half : single).slice(0, width / 4), narrow ? 2 : 4),
          };
          const fn = compile(c, row);
          execute(c, fn, row);
          if (destination === source) {
            execute(c, fn, row, 0);
            execute(c, fn, row, 1, true);
          }
        }
    for (let immediate = 0; immediate < 256; immediate++) {
      const row = {
        name: `immediate-${immediate}`,
        width: 16,
        narrow: true,
        destination: 1,
        source: 1,
        input: words([0x3f801000, 0xbf801000, 0, 0]),
        immediate,
      };
      const fn = compile(c, row);
      for (let mode = 0; mode < 4; mode++) {
        const rc = immediate & 4 ? mode : immediate & 3,
          output = [
            [0x3c00, 0xbc00],
            [0x3c00, 0xbc01],
            [0x3c01, 0xbc00],
            [0x3c00, 0xbc00],
          ][rc];
        execute(c, fn, {
          ...row,
          mxcsr: 0x1f80 | (mode << 13),
          after: 0x1f80 | (mode << 13) | 0x20,
          output: words([...output, 0, 0], 2),
        });
      }
    }
    for (const width of [16, 32])
      for (const [name, bits, result, mxcsr, after, immediate, reason] of literals)
        for (const memory of [false, true]) {
          const row = {
            name: `${name}-${width}-${memory}`,
            width,
            narrow: true,
            destination: 1,
            source: 1,
            input: words([bits, ...Array(width / 4 - 1).fill(0)]),
            output: words([result, ...Array(width / 4 - 1).fill(0)], 2),
            mxcsr,
            after,
            immediate,
            memory,
            reason,
            detail: reason ? after : 0,
          };
          execute(c, compile(c, row), row);
        }
    for (const width of [16, 32])
      for (const memory of [false, true])
        for (const mxcsr of [0x1f80, 0x1fc0, 0x1f00]) {
          const values = [0x7c01, 1, 0x8001, 0x7e55, ...Array(width / 4 - 4).fill(1)],
            row = {
              name: `widen-NaN-denormal-${width}-${memory}-${mxcsr}`,
              width,
              narrow: false,
              source: 1,
              destination: 1,
              memory,
              input: words(values, 2),
              output: words(values.map(widen)),
              mxcsr,
              after: mxcsr | 1,
              ...(mxcsr === 0x1f00 ? { reason: 13, detail: 0x1f01 } : {}),
            };
          if (memory) upload(c, 0x5020, row.input);
          execute(c, compile(c, row), row);
        }
    for (const width of [16, 32])
      for (const narrow of [false, true]) {
        const row = {
          name: `exact-page-end-${width}-${narrow}`,
          width,
          narrow,
          source: 1,
          destination: 1,
          memory: true,
          address: 0x6000 - width / 2,
          input: words((narrow ? single : half).slice(0, width / 4), narrow ? 4 : 2),
          output: words((narrow ? half : single).slice(0, width / 4), narrow ? 2 : 4),
        };
        if (!narrow) upload(c, row.address, row.input);
        execute(c, compile(c, row), row);
        const fault = {
          ...row,
          name: `late-page-fault-${width}-${narrow}`,
          address: 0x5ffc,
          reason: 5,
          detail: 1,
          fault: 0x6000,
          access: narrow ? 2 : 1,
        };
        execute(c, compile(c, fault), fault);
        const denied = {
          ...row,
          name: `cached-limit-${width}-${narrow}`,
          address: 0x5020,
          cache: { 3: 0xf2305023 },
          reason: 11,
          detail: 0,
        };
        execute(c, compile(c, denied), denied);
        const ss = {
          ...denied,
          name: `SS-limit-${width}-${narrow}`,
          prefix: [0x36],
          segment: 2,
          reason: 15,
          cache: { 2: 0xf2305023 },
        };
        execute(c, compile(c, ss), ss);
        const overflow = {
          ...row,
          name: `logical-overflow-${width}-${narrow}`,
          address: 0xfffffffc,
          reason: 11,
          detail: 0,
        };
        execute(c, compile(c, overflow), overflow);
        const nullDs = {
          ...row,
          name: `null-DS-${width}-${narrow}`,
          nullDs: true,
          reason: 11,
          detail: 0,
        };
        execute(c, compile(c, nullDs), nullDs);
      }
    for (const width of [16, 32])
      for (const narrow of [false, true]) {
        const row = {
          name: `unaligned-${width}-${narrow}`,
          width,
          narrow,
          source: 1,
          destination: 1,
          memory: true,
          address: 0x5021,
          input: words((narrow ? single : half).slice(0, width / 4), narrow ? 4 : 2),
          output: words((narrow ? half : single).slice(0, width / 4), narrow ? 2 : 4),
        };
        if (!narrow) upload(c, row.address, row.input);
        execute(c, compile(c, row), row);
      }
    for (const width of [16, 32]) {
      const row = {
        name: `page-authority-before-would-trap-XM-${width}`,
        width,
        narrow: true,
        source: 1,
        destination: 1,
        memory: true,
        address: 0x5ffc,
        input: words([0x7f800001, ...Array(width / 4 - 1).fill(0)]),
        output: Buffer.alloc(width / 2),
        mxcsr: 0x1f00,
        reason: 5,
        detail: 1,
        fault: 0x6000,
        access: 2,
      };
      execute(c, compile(c, row), row);
      host(c, 'protect', [0x5000, 1, 1]);
      const permission = {
        ...row,
        name: `write-permission-before-would-trap-XM-${width}`,
        address: 0x5020,
        detail: 2,
        fault: 0x5020,
      };
      execute(c, compile(c, permission), permission);
      host(c, 'protect', [0x5000, 1, 3]);
      const xm = {
        ...row,
        name: `accessible-target-XM-neutral-${width}`,
        address: 0x5020,
        reason: 13,
        detail: 0x1f01,
        fault: 0,
        access: 0,
      };
      execute(c, compile(c, xm), xm);
    }
    const authorityRow = {
      name: 'transport-controls',
      width: 16,
      narrow: false,
      destination: 1,
      source: 1,
      input: words(half.slice(0, 4), 2),
      output: words(single.slice(0, 4)),
    };
    compile(c, authorityRow);
    seed(c, authorityRow);
    for (const args of [
      [0x02001201, 0, 0, 0, 0, 2],
      [0x01001201, 1 << 3, 0, 0, 0, 2],
      [0x01001201, 0, 1, 0, 0, 2],
      [0x01001203, 0, 256, 0, 0, 2],
    ]) {
      const before = arena(c),
        beforeRam = ram(c);
      assert.equal(helper(c, ...args), 7);
      assert.deepEqual(arena(c), before);
      assert.deepEqual(ram(c), beforeRam);
      counts.neutral++;
    }
    input(c, T + 544, record('R3AM', 24, [3, 0x5020]));
    input(c, T + 548, Buffer.from([2, 0]));
    {
      const before = arena(c),
        beforeRam = ram(c);
      assert.equal(helper(c, 0x01001201, 1 | (1 << 12), 0, 0, 0, 2), 7);
      assert.deepEqual(arena(c), before);
      assert.deepEqual(ram(c), beforeRam);
      counts.neutral++;
    }
    const smc = {
      name: 'successful-numerical-store-invalidates-code-after-publication',
      width: 16,
      narrow: true,
      source: 1,
      destination: 1,
      memory: true,
      address: PC,
      input: words(single.slice(0, 4)),
      output: words(half.slice(0, 4), 2),
      reason: 6,
      committed: true,
    };
    execute(c, compile(c, smc), smc);
  }
assert.deepEqual(pins(), sourcePins);
assert.equal(counts.contexts, 4);
assert.ok(counts.runs > 0);
assert.equal(counts.fullRamPairs, counts.runs);
writeFileSync(join(output, 'frames.bin'), Buffer.concat(frames));
writeFileSync(
  join(output, 'result.json'),
  JSON.stringify(
    {
      engine: { path: enginePath, sha256: engine.sha256 },
      counts,
      sourcePins,
      rows,
      scope:
        'four canonical F16C guest forms, exact helper+arena+RAM; deterministic authority-first numerical-store profile; retained private exhaustive numerical proof remains separate',
    },
    null,
    2,
  ) + '\n',
);
console.log(JSON.stringify(counts));
