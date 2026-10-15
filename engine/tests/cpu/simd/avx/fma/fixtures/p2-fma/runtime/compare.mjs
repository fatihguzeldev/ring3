import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import os from 'node:os';
import { readEngine } from '../../../../../../../support/wasm/engine.mjs';
import { assertEngineExports } from '../../../../../../../support/wasm/exports.mjs';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root, 'usage: compare.mjs engine.wasm fresh-output source-root');
assert.match(
  process.env.RING3_ENGINE_SHA256 ?? '',
  /^[a-f0-9]{64}$/,
  'exact requested engine hash required',
);
const engine = readEngine(enginePath);
const { sourcePaths: exportSources } = assertEngineExports(engine.module, root);
mkdirSync(output, { recursive: false });
const started = new Date().toISOString(),
  PC = 0x1000,
  T = 140,
  SV = 4364,
  YU = 4612,
  SIZE = 4756;
const UNROLL = 20,
  SAMPLES = 5,
  WARMUP = 3,
  MIN_SAMPLE_NS = 10000000;
const iterationInput = process.env.RING3_BENCH_ITERATIONS ?? '24575';
assert.match(iterationInput, /^[1-9][0-9]*$/, 'iterations must be a positive decimal integer');
const iterations = Number(iterationInput),
  increments = [2, 6, 12, 20];
assert.ok(
  Number.isSafeInteger(iterations) && iterations <= Math.floor(0xffffff / (UNROLL * 20)),
  'every intermediate accumulator must remain an exact positive binary32 integer',
);
assert.ok(iterations * (UNROLL * 3 + 2) <= 0xffffffff, 'exact unsigned32 retirement budget');
const sourcePaths = [
  ...exportSources,
  'engine/tests/support/wasm/engine.mjs',
  'Cargo.lock',
  'engine/src/cpu/x86/capabilities.rs',
  'engine/src/cpu/x86/decode/avx/fma.rs',
  'engine/src/cpu/x86/avx/operation.rs',
  'engine/src/cpu/x86/avx/operation/factory/fma.rs',
  'engine/src/cpu/x86/avx/dispatch.rs',
  'engine/src/cpu/x86/avx/fma/mod.rs',
  'engine/src/cpu/x86/avx/arithmetic.rs',
  'engine/src/cpu/x86/simd/float.rs',
  'engine/src/cpu/x86/simd/float/fused.rs',
  'engine/src/cpu/dbt/region.rs',
  'engine/src/cpu/dbt/wasm/emitter/mod.rs', 'engine/src/cpu/dbt/wasm/emitter/module.rs', 'engine/src/cpu/dbt/wasm/emitter/body.rs', 'engine/src/cpu/dbt/wasm/requirements.rs', 'engine/src/cpu/dbt/wasm/imports/wiring.rs',
  'engine/src/cpu/dbt/wasm/instructions/avx/execute.rs',
  'engine/src/cpu/dbt/wasm/instructions/simd.rs',
  'engine/src/cpu/dbt/wasm/instructions/simd/direct.rs',
  'engine/src/cpu/dbt/wasm/helpers/result.rs',
  'engine/src/process/cpu/avx.rs',
  'engine/src/process/cpu/simd.rs',
  'engine/src/math/ieee.rs',
  'engine/src/math/ieee/fused.rs',
];
assert.equal(new Set(sourcePaths).size, sourcePaths.length, 'unique production pins');
const hash = (bytes) => createHash('sha256').update(bytes).digest('hex');
const pins = () =>
  Object.fromEntries(sourcePaths.map((path) => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = pins(),
  scriptPath = fileURLToPath(import.meta.url),
  scriptHash = hash(readFileSync(scriptPath));
process.on('uncaughtException', (error) => {
  writeFileSync(
    join(output, 'failed.json'),
    JSON.stringify(
      {
        status: 'FAIL',
        started,
        failed: new Date().toISOString(),
        engineSha256: engine.sha256,
        sourcePins,
        benchmark: { path: scriptPath, sha256: scriptHash },
        error: { name: error.name, message: error.message, stack: error.stack },
      },
      null,
      2,
    ) + '\n',
    { flag: 'wx' },
  );
  console.error(error);
  process.exitCode = 1;
});
const words = (values) => {
  const b = Buffer.alloc(values.length * 4);
  values.forEach((v, i) => b.writeUInt32LE(v >>> 0, i * 4));
  return b;
};
const record = (magic, size, values, version = 1) => {
  const b = Buffer.alloc(size);
  b.write(magic);
  b.writeUInt16LE(version, 4);
  b.writeUInt16LE(1, 6);
  b.writeUInt32LE(size, 8);
  b.set(words(values), 16);
  return b;
};
const arena = (c) => Buffer.from(new Uint8Array(c.memory.buffer, c.base, SIZE));
const input = (c, at, bytes) => new Uint8Array(c.memory.buffer).set(bytes, c.base + at);
const host = (c, name, args = []) => assert.equal(c.api[name](...args), 0, name);
function loop(instructions) {
  const update = Buffer.from(instructions.join(''), 'hex');
  const code = Buffer.concat([
    ...Array(UNROLL).fill(update),
    Buffer.from([0x49, 0x0f, 0x85]),
    Buffer.alloc(4),
  ]);
  code.writeInt32LE(-code.length, code.length - 4);
  return code;
}
function singleBits(integer) {
  if (integer === 0) return 0;
  const exponent = 31 - Math.clz32(integer);
  assert.ok(
    Number.isInteger(integer) && integer > 0 && exponent <= 23,
    'exact integer-to-binary32 oracle',
  );
  return (((exponent + 127) << 23) | (integer * 2 ** (23 - exponent) - 2 ** 23)) >>> 0;
}
const fused = { label: 'fma231', instructions: ['c4e271b8c2'], helper: 'avx', helpersPerUpdate: 1 };
const pairs = [
  {
    name: 'sse-split',
    reference: {
      label: 'sse-mul-add',
      instructions: ['0f28d9', '0f59da', '0f58c3'],
      helper: 'simd',
      helpersPerUpdate: 2,
    },
    fused,
  },
  {
    name: 'vex-split',
    reference: {
      label: 'vex-mul-add',
      instructions: ['c5f059da', 'c5f858c3'],
      helper: 'avx',
      helpersPerUpdate: 2,
    },
    fused,
  },
];
let key = 0;
function compile(row, version) {
  const variant = row[version],
    instance = new WebAssembly.Instance(engine.module, {});
  const c = { row, version, variant, memory: instance.exports.memory, api: {} };
  for (const [name, value] of Object.entries(instance.exports)) {
    if (name.startsWith('ring3_abi_v1_') && typeof value === 'function')
      c.api[name.slice(13)] = value;
  }
  for (const [name, arity] of Object.entries({ open: 3, map: 3, upload: 2, compile_resident: 1 }))
    assert.equal(c.api[name]?.length, arity, name);
  c.key = ++key;
  host(c, 'open', [2, c.key, 0x464d41]);
  c.base = c.api.arena_ptr() >>> 0;
  assert.equal(c.api.arena_len(), SIZE);
  host(c, 'map', [PC, 1, 7]);
  c.initial = arena(c);
  c.code = loop(variant.instructions);
  input(c, T, c.code);
  host(c, 'upload', [PC, c.code.length]);
  input(c, T, words([PC, c.code.length]));
  host(c, 'compile_resident', [1]);
  const a = arena(c),
    pointer = a.readUInt32LE(T + 16),
    length = a.readUInt32LE(T + 20);
  c.binary = Buffer.from(new Uint8Array(c.memory.buffer, pointer, length));
  c.module = new WebAssembly.Module(c.binary);
  c.helper = `${variant.helper}_execute_resident`;
  assert.deepEqual(
    WebAssembly.Module.imports(c.module)
      .map((i) => `${i.module}:${i.name}:${i.kind}`)
      .sort(),
    ['env:memory:memory', 'ring3:guard_resident:function', `ring3:${c.helper}:function`].sort(),
    `${row.name}/${version}: exact register-only hot path`,
  );
  assert.deepEqual(WebAssembly.Module.exports(c.module), [{ name: 'run', kind: 'function' }]);
  assert.equal(c.api[c.helper].length, 10);
  c.run = new WebAssembly.Instance(c.module, {
    env: { memory: c.memory },
    ring3: c.api,
  }).exports.run;
  writeFileSync(join(output, `${row.name}-${version}.wasm`), c.binary, { flag: 'wx' });
  return c;
}
function seed(c, count) {
  input(c, 0, c.initial);
  const registers = [
    0x12345678,
    count,
    0x23456789,
    0x3456789a,
    0x7780,
    0x456789ab,
    0x56789abc,
    0x6789abcd,
  ];
  input(c, 0, record('R3ST', 56, [...registers, PC, 3]));
  for (let i = 0; i < 8; i++) {
    input(c, SV + 32 + i * 16, Buffer.alloc(16, 0x21 + i));
    input(c, YU + 16 + i * 16, Buffer.alloc(16, i === 0 || i === 3 ? 0 : 0x91 + i));
  }
  input(c, SV + 16, words([0x1f80]));
  input(c, SV + 32, words([0, 0, 0, 0]));
  input(c, SV + 48, words([0x3f800000, 0x40000000, 0x40400000, 0x40800000]));
  input(c, SV + 64, words([0x40000000, 0x40400000, 0x40800000, 0x40a00000]));
  // both split loops recompute the product; this seed makes the unused FMA scratch match too.
  input(c, SV + 80, words([0x40000000, 0x40c00000, 0x41400000, 0x41a00000]));
  return registers;
}
function execute(c, count, run = c.run) {
  const registers = seed(c, count),
    before = arena(c),
    updates = count * UNROLL;
  const guestVectorInstructions = updates * c.variant.instructions.length;
  const retired = guestVectorInstructions + count * 2,
    expected = Buffer.from(before);
  registers[1] = 0;
  expected.set(record('R3ST', 56, [...registers, PC + c.code.length, 0x47]));
  expected.set(record('R3EX', 40, [1, retired, 0, 0, 0, 0], 6), 56);
  expected.set(words(increments.map((increment) => singleBits(updates * increment))), SV + 32);
  expected.set(record('R3MH', 40, [0, 0, 0, 0, 0, 0], 7), 100);
  expected.set(words([0, 3]), T + 512);
  const start = process.hrtime.bigint(),
    status = run(c.base, c.base + 56, retired, c.base + 96);
  const elapsed = process.hrtime.bigint() - start;
  assert.equal(status, 0, `${c.row.name}/${c.version}: run transport`);
  assert.deepEqual(
    arena(c),
    expected,
    `${c.row.name}/${c.version}: all4756bytes, retirement/flags/MXCSR/upperstate`,
  );
  return {
    nanoseconds: Number(elapsed),
    logicalMultiplyAdds: updates,
    scalarFloatingOperations: updates * 4 * 2,
    guestVectorInstructions,
    retired,
    frameSha256: hash(arena(c)),
  };
}
const results = [],
  median = (values) => [...values].sort((a, b) => a - b)[Math.floor(values.length / 2)];
for (const row of pairs) {
  const contexts = { reference: compile(row, 'reference'), fused: compile(row, 'fused') };
  // untimed instrumentation proves both operations remain inside every split update.
  for (const c of Object.values(contexts)) {
    let calls = 0;
    const ring3 = {
      ...c.api,
      [c.helper]: (...args) => {
        calls++;
        const status = c.api[c.helper](...args);
        assert.equal(status, 0);
        return status;
      },
    };
    const control = new WebAssembly.Instance(c.module, { env: { memory: c.memory }, ring3 }).exports
      .run;
    execute(c, 1, control);
    assert.equal(calls, UNROLL * c.variant.helpersPerUpdate);
  }
  const warmup = [];
  for (let round = 0; round < WARMUP; round++) {
    for (const version of round % 2 ? ['fused', 'reference'] : ['reference', 'fused']) {
      warmup.push({ round, version, ...execute(contexts[version], iterations) });
    }
  }
  const samples = [];
  for (let sample = 0; sample < SAMPLES; sample++) {
    const order = sample % 2 ? ['fused', 'reference'] : ['reference', 'fused'],
      times = {};
    for (const version of order) times[version] = execute(contexts[version], iterations);
    samples.push({
      sample,
      order,
      ...times,
      fusedToReference: times.fused.nanoseconds / times.reference.nanoseconds,
    });
    writeFileSync(
      join(output, 'partial.json'),
      JSON.stringify(
        {
          started,
          engineSha256: engine.sha256,
          completed: results,
          current: { name: row.name, warmup, samples },
        },
        null,
        2,
      ) + '\n',
    );
  }
  const minimumNanoseconds = Math.min(
    ...samples.flatMap((s) => [s.reference.nanoseconds, s.fused.nanoseconds]),
  );
  results.push({
    name: row.name,
    iterations,
    unroll: UNROLL,
    labels: { reference: row.reference.label, fused: row.fused.label },
    modules: Object.fromEntries(
      Object.entries(contexts).map(([version, c]) => [
        version,
        {
          bytes: c.binary.length,
          sha256: hash(c.binary),
          guestCode: c.code.toString('hex'),
          instructionsPerUpdate: c.variant.instructions.length,
          helperCallsPerUpdate: c.variant.helpersPerUpdate,
          imports: WebAssembly.Module.imports(c.module),
        },
      ]),
    ),
    warmup,
    samples,
    minimumNanoseconds,
    sufficientDuration: minimumNanoseconds >= MIN_SAMPLE_NS,
    medianFusedToReference: median(samples.map((s) => s.fusedToReference)),
    medians: Object.fromEntries(
      ['reference', 'fused'].map((version) => [
        version,
        {
          nanoseconds: median(samples.map((s) => s[version].nanoseconds)),
          nanosecondsPerMultiplyAdd: median(
            samples.map((s) => s[version].nanoseconds / s[version].logicalMultiplyAdds),
          ),
        },
      ]),
    ),
  });
  for (const c of Object.values(contexts)) host(c, 'close');
}
assert.deepEqual(pins(), sourcePins, 'production source pins unchanged');
assert.equal(hash(readFileSync(scriptPath)), scriptHash, 'benchmark source unchanged');
const cpu = os.cpus();
const receipt = {
  status: 'PASS',
  started,
  finished: new Date().toISOString(),
  binding: 'resident',
  engine: { path: resolve(enginePath), bytes: engine.bytes.length, sha256: engine.sha256 },
  environment: {
    node: process.version,
    versions: process.versions,
    platform: os.platform(),
    release: os.release(),
    arch: os.arch(),
    cpuModels: [...new Set(cpu.map((c) => c.model))],
    logicalCPUs: cpu.length,
    cpuReportedMHz: cpu.map((c) => c.speed),
    totalMemory: os.totalmem(),
    execArgv: process.execArgv,
  },
  method: {
    samples: SAMPLES,
    alternatingOrder: true,
    warmupPerVariant: WARMUP,
    clock: 'process.hrtime.bigint',
    minimumSampleNanoseconds: MIN_SAMPLE_NS,
    sufficientDuration: results.every((row) => row.sufficientDuration),
    timedRegion:
      'generated Wasm run only; seed, oracles, hashing, logging and helper instrumentation outside clock',
    arithmetic:
      'each update adds [1,2,3,4]*[2,3,4,5]; every product and intermediate accumulator is an exact binary32 integer below2^24',
    retainedWork:
      'no multiply hoisting; untimed helper-count proof checks two split calls or one FMA call perupdate',
    retirement:
      'FMA1, VEXsplit2, SSEsplit3 vectorinstructions perupdate, plus DEC/JNE perloop; SSE requires MOVAPS scratchcopy',
    upper:
      'destination0/scratch3 upper seededzero; source1/2 and unrelateduppers nonzero and verified',
    verification:
      'whole4756-byte canonical arena aftereveryrun, exactresultbits, MXCSR1f80, flags47, pervariantEIP/retirement/helperpacket',
    scope:
      'oneNode/V8host; register-only packed128 PS accumulator dependency chain; no256bit/memory/browser/game/wholeCPU throughput claim',
  },
  sourcePins,
  benchmark: { path: scriptPath, sha256: scriptHash },
  results,
};
writeFileSync(join(output, 'result.json'), JSON.stringify(receipt, null, 2) + '\n', { flag: 'wx' });
console.log(
  JSON.stringify({
    status: 'PASS',
    engineSha256: engine.sha256,
    sufficientDuration: receipt.method.sufficientDuration,
    ratios: results.map((row) => [row.name, row.medianFusedToReference]),
  }),
);
