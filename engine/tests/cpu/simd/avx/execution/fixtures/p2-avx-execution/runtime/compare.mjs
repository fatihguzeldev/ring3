import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync, mkdirSync} from 'node:fs';
import {join, resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
import os from 'node:os';
import {readEngine} from '../../../../../../../support/wasm/engine.mjs';
import {assertEngineExports} from '../../../../../../../support/wasm/exports.mjs';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root, 'usage: compare.mjs engine.wasm fresh-output source-root');
assert.match(process.env.RING3_ENGINE_SHA256 ?? '', /^[a-f0-9]{64}$/, 'exact requested engine hash required');
const engine = readEngine(enginePath);
const {sourcePaths: exportSources} = assertEngineExports(engine.module, root);
mkdirSync(output, {recursive: false});
const started = new Date().toISOString(), PC = 0x1000, T = 140, SV = 4364, YU = 4612, SIZE = 4756;
const UNROLL = 31, SAMPLES = 5, WARMUP = 3, owner = process.env.RING3_BENCH_OWNER ?? 'resident';
const directInput = process.env.RING3_BENCH_DIRECT_ITERATIONS ?? '1048575';
assert.match(directInput, /^[1-9][0-9]*$/, 'direct iterations must be a positive decimal integer');
const directIterations = Number(directInput);
assert.ok(Number.isSafeInteger(directIterations) && directIterations <= Math.floor(0xffffffff / (UNROLL + 2)),
  'direct iterations must fit the exact unsigned32 guest retirement budget');
assert.ok(['replacement', 'resident'].includes(owner), 'explicit supported binding');
const sourcePaths = [...exportSources, 'engine/tests/support/wasm/engine.mjs', 'Cargo.lock',
  'engine/src/cpu/x86/capabilities.rs', 'engine/src/cpu/x86/avx/operation.rs', 'engine/src/cpu/x86/avx/dispatch.rs',
  'engine/src/cpu/x86/avx/arithmetic.rs', 'engine/src/cpu/x86/avx/mod.rs', 'engine/src/cpu/x86/simd/float.rs',
  'engine/src/cpu/dbt/wasm/emitter.rs', 'engine/src/cpu/dbt/wasm/avx/direct.rs', 'engine/src/cpu/dbt/wasm/avx/integer.rs',
  'engine/src/cpu/dbt/wasm/avx/logical.rs', 'engine/src/cpu/dbt/wasm/avx/execute.rs', 'engine/src/cpu/dbt/wasm/simd.rs',
  'engine/src/cpu/dbt/wasm/simd/direct.rs', 'engine/src/cpu/dbt/wasm/cpu_result.rs', 'engine/src/process/cpu/avx.rs',
  'engine/src/process/cpu/simd.rs', 'engine/src/math/ieee.rs'];
assert.equal(new Set(sourcePaths).size, sourcePaths.length, 'unique production pins');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const pins = () => Object.fromEntries(sourcePaths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = pins(), scriptPath = fileURLToPath(import.meta.url), scriptHash = hash(readFileSync(scriptPath));
process.on('uncaughtException', error => {
  writeFileSync(join(output, 'failed.json'), JSON.stringify({status: 'FAIL', started, failed: new Date().toISOString(), owner,
    engineSha256: engine.sha256, sourcePins, benchmark: {path: scriptPath, sha256: scriptHash},
    error: {name: error.name, message: error.message, stack: error.stack}}, null, 2) + '\n', {flag: 'wx'});
  console.error(error); process.exitCode = 1;
});
const words = values => {const b = Buffer.alloc(values.length * 4); values.forEach((v, i) => b.writeUInt32LE(v >>> 0, i * 4)); return b;};
const record = (magic, size, values, version = 1) => {
  const b = Buffer.alloc(size); b.write(magic); b.writeUInt16LE(version, 4); b.writeUInt16LE(1, 6); b.writeUInt32LE(size, 8);
  b.set(words(values), 16); return b;
};
const arena = c => Buffer.from(new Uint8Array(c.memory.buffer, c.base, SIZE));
const input = (c, at, bytes) => new Uint8Array(c.memory.buffer).set(bytes, c.base + at);
const host = (c, name, args = []) => assert.equal(c.api[name](...args), 0, name);
function loop(opcode) {
  const instruction = Buffer.from(opcode, 'hex');
  const code = Buffer.concat([...Array(UNROLL).fill(instruction), Buffer.from([0x49, 0x0f, 0x85]), Buffer.alloc(4)]);
  code.writeInt32LE(-code.length, code.length - 4); return code;
}
function singleBits(integer) {
  if (integer === 0) return 0;
  const magnitude = Math.abs(integer), exponent = 31 - Math.clz32(magnitude);
  assert.ok(Number.isInteger(integer) && exponent <= 23, 'exact integer-to-binary32 oracle');
  return ((integer < 0 ? 0x80000000 : 0) | ((exponent + 127) << 23) | (magnitude * 2 ** (23 - exponent) - 2 ** 23)) >>> 0;
}
const pairs = [
  {name: 'packed-add', legacy: '660ffec1', avx: 'c5f9fec1', iterations: directIterations,
    first: [0xfffffffc, 0x7fffffff, 0x80000000, 0x12345678], second: [1, 2, 0xffffffff, 0x01020304],
    expected: (row, count) => row.first.map((value, i) => Number((BigInt(value) + BigInt(count) * BigInt(row.second[i])) & 0xffffffffn))},
  {name: 'logical-xor', legacy: '0f57c1', avx: 'c5f857c1', iterations: directIterations,
    first: [0x7f800001, 0x80000000, 0x7fc12345, 0xffffffff], second: [0x00200000, 0x80000000, 1, 0x55555555],
    expected: (row, count) => row.first.map((value, i) => (count % 2 ? value ^ row.second[i] : value) >>> 0)},
  {name: 'numeric-add', legacy: '0f58c1', avx: 'c5f858c1', iterations: 513, helper: true,
    first: [0xc0800000, 0xc1000000, 0xc1800000, 0xc2000000], second: [0x3f800000, 0x40000000, 0x40800000, 0x41000000],
    expected: (_row, count) => [1, 2, 4, 8].map(scale => singleBits((count - 4) * scale))},
];
let key = 0;
function compile(row, version) {
  const instance = new WebAssembly.Instance(engine.module, {}), c = {version, row, memory: instance.exports.memory, api: {}};
  for (const [name, value] of Object.entries(instance.exports)) if (name.startsWith('ring3_abi_v1_') && typeof value === 'function') c.api[name.slice(13)] = value;
  for (const [name, arity] of Object.entries({open: 3, map: 3, upload: 2, compile: 1, compile_resident: 1})) assert.equal(c.api[name]?.length, arity, name);
  c.key = ++key; host(c, 'open', [2, c.key, 0x415658]); c.base = c.api.arena_ptr() >>> 0;
  assert.equal(c.api.arena_len(), SIZE); host(c, 'map', [PC, 1, 7]); c.initial = arena(c);
  c.code = loop(row[version]); input(c, T, c.code); host(c, 'upload', [PC, c.code.length]);
  input(c, T, words([PC, c.code.length])); host(c, owner === 'resident' ? 'compile_resident' : 'compile', [1]);
  const a = arena(c), pointer = owner === 'resident' ? a.readUInt32LE(T + 16) : c.api.module_ptr() >>> 0;
  const length = owner === 'resident' ? a.readUInt32LE(T + 20) : c.api.module_len() >>> 0;
  c.binary = Buffer.from(new Uint8Array(c.memory.buffer, pointer, length)); c.module = new WebAssembly.Module(c.binary);
  c.helper = row.helper ? `${version === 'avx' ? 'avx' : 'simd'}_execute${owner === 'resident' ? '_resident' : ''}` : null;
  const imports = WebAssembly.Module.imports(c.module).map(i => `${i.module}:${i.name}:${i.kind}`).sort();
  assert.deepEqual(imports, ['env:memory:memory', `ring3:${owner === 'resident' ? 'guard_resident' : 'guard'}:function`,
    ...(c.helper ? [`ring3:${c.helper}:function`] : [])].sort(), `${row.name}/${version}: exact hot path`);
  assert.deepEqual(WebAssembly.Module.exports(c.module), [{name: 'run', kind: 'function'}]);
  if (c.helper) assert.equal(c.api[c.helper].length, owner === 'resident' ? 10 : 6);
  c.run = new WebAssembly.Instance(c.module, {env: {memory: c.memory}, ring3: c.api}).exports.run;
  writeFileSync(join(output, `${row.name}-${version}.wasm`), c.binary, {flag: 'wx'});
  return c;
}
function seed(c, iterations) {
  input(c, 0, c.initial);
  const registers = [0x12345678, iterations, 0x23456789, 0x3456789a, 0x7780, 0x456789ab, 0x56789abc, 0x6789abcd];
  input(c, 0, record('R3ST', 56, [...registers, PC, 3]));
  for (let i = 0; i < 8; i++) {
    input(c, SV + 32 + i * 16, Buffer.alloc(16, 0x21 + i));
    input(c, YU + 16 + i * 16, Buffer.alloc(16, i === 0 ? 0 : 0x91 + i));
  }
  input(c, SV + 16, words([0x1f80])); input(c, SV + 32, words(c.row.first)); input(c, SV + 48, words(c.row.second));
  return registers;
}
function execute(c, iterations, run = c.run) {
  const registers = seed(c, iterations), before = arena(c), count = iterations * UNROLL, retired = iterations * (UNROLL + 2);
  const expected = Buffer.from(before); registers[1] = 0;
  expected.set(record('R3ST', 56, [...registers, PC + c.code.length, 0x47]));
  expected.set(record('R3EX', 40, [1, retired, 0, 0, 0, 0], 6), 56);
  expected.set(words(c.row.expected(c.row, count)), SV + 32);
  if (c.row.helper) {
    expected.set(record('R3MH', 40, [0, 0, 0, 0, 0, 0], 7), 100);
    expected.set(words([0, 3]), T + 512);
  }
  const start = process.hrtime.bigint(), status = run(c.base, c.base + 56, retired, c.base + 96), elapsed = process.hrtime.bigint() - start;
  assert.equal(status, 0, `${c.row.name}/${c.version}: run transport`);
  assert.deepEqual(arena(c), expected, `${c.row.name}/${c.version}: all4756bytes, exactretirement/flags/MXCSR/upperstate`);
  return {nanoseconds: Number(elapsed), vectorInstructions: count, retired, frameSha256: hash(arena(c))};
}
const results = [];
for (const row of pairs) {
  const contexts = {legacy: compile(row, 'legacy'), avx: compile(row, 'avx')};
  // a separate untimed binding proves helper count; timed imports bind real Wasm exports directly.
  for (const c of Object.values(contexts)) {
    let calls = 0;
    const ring3 = {...c.api};
    if (c.helper) ring3[c.helper] = (...args) => {calls++; const status = c.api[c.helper](...args); assert.equal(status, 0); return status;};
    const control = new WebAssembly.Instance(c.module, {env: {memory: c.memory}, ring3}).exports.run;
    execute(c, 1, control); assert.equal(calls, row.helper ? UNROLL : 0);
  }
  const warmup = [];
  for (let round = 0; round < WARMUP; round++) for (const version of round % 2 ? ['avx', 'legacy'] : ['legacy', 'avx']) {
    warmup.push({round, version, ...execute(contexts[version], row.iterations)});
  }
  const samples = [];
  for (let sample = 0; sample < SAMPLES; sample++) {
    const order = sample % 2 ? ['avx', 'legacy'] : ['legacy', 'avx'], times = {};
    for (const version of order) times[version] = execute(contexts[version], row.iterations);
    samples.push({sample, order, ...times, avxToLegacy: times.avx.nanoseconds / times.legacy.nanoseconds});
    writeFileSync(join(output, 'partial.json'), JSON.stringify({started, owner, engineSha256: engine.sha256, completed: results, current: {name: row.name, warmup, samples}}, null, 2) + '\n');
  }
  const median = values => [...values].sort((a, b) => a - b)[Math.floor(values.length / 2)];
  results.push({name: row.name, iterations: row.iterations, unroll: UNROLL,
    modules: Object.fromEntries(Object.entries(contexts).map(([version, c]) => [version, {bytes: c.binary.length, sha256: hash(c.binary), guestCode: c.code.toString('hex'), imports: WebAssembly.Module.imports(c.module)}])),
    warmup, samples, medianAvxToLegacy: median(samples.map(s => s.avxToLegacy)),
    medianNanosecondsPerVector: Object.fromEntries(['legacy', 'avx'].map(version => [version, median(samples.map(s => s[version].nanoseconds / s[version].vectorInstructions))]))});
  for (const c of Object.values(contexts)) host(c, 'close');
}
assert.deepEqual(pins(), sourcePins, 'production source pins unchanged');
assert.equal(hash(readFileSync(scriptPath)), scriptHash, 'benchmark source unchanged');
const cpu = os.cpus();
const receipt = {status: 'PASS', started, finished: new Date().toISOString(), owner,
  engine: {path: resolve(enginePath), bytes: engine.bytes.length, sha256: engine.sha256},
  environment: {node: process.version, versions: process.versions, platform: os.platform(), release: os.release(), arch: os.arch(),
    cpuModels: [...new Set(cpu.map(c => c.model))], logicalCPUs: cpu.length, cpuReportedMHz: cpu.map(c => c.speed), totalMemory: os.totalmem(), execArgv: process.execArgv},
  method: {samples: SAMPLES, alternatingOrder: true, warmupPerVariant: WARMUP, clock: 'process.hrtime.bigint',
    timedRegion: 'only generated Wasm run; seed, oracles, hashes, logging and instrumented helper control are outside clock',
    numeric: 'exact powers-of-two additions below binary32 precision limit; noNaN/denormal/roundingexception workload',
    upper: 'destinationupper seededzero soSSEpreserve andVEX128zeroing produceequalbits; unrelateduppers nonzero andverified',
    verification: 'wholecanonicalarena all4756bytes aftereveryrun plus independent wrappedinteger/rawXOR/exactbinary32 bit oracle',
    scope: 'oneNode/V8host and three128bit register-only dependency chains; fixednonvectorloopoverhead included; no256bit/browser/game/wholeCPU throughput claim'},
  sourcePins, benchmark: {path: scriptPath, sha256: scriptHash}, results};
writeFileSync(join(output, 'result.json'), JSON.stringify(receipt, null, 2) + '\n', {flag: 'wx'});
console.log(JSON.stringify({status: 'PASS', owner, engineSha256: engine.sha256, ratios: results.map(r => [r.name, r.medianAvxToLegacy])}));
