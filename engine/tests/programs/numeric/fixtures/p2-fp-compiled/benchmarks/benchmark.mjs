import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {mkdirSync, readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {fileURLToPath} from 'node:url';
import {ARENA_SIZE, createArena} from '../../../../../support/wasm/arena.mjs';
import {readEngine} from '../../../../../support/wasm/engine.mjs';

const [enginePath, output, label = 'unspecified'] = process.argv.slice(2);
assert.ok(enginePath && output, 'benchmark.mjs ENGINE_WASM OUTPUT_DIRECTORY [LABEL]');
mkdirSync(output, {recursive: true});

const directory = fileURLToPath(new URL('..', import.meta.url));
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const hex = value => Buffer.from(value, 'hex');
const now = () => process.hrtime.bigint();
const elapsed = (start, end) => Number(end - start);
const T = 140, FP = 4236, SV = 4364;
const ENTRY = 0x2000, CALLER = 0x3000, COEFF = 0x5010, INPUT = 0x6010;
const VECTOR = 0x7010, STACK = 0x9020, ESP = 0x9080, SCALAR = 0xa010, COUNT_OUT = 0xa030;
const CYCLES = 4, WARMUP = 20, BATCH_CALLS = 100, SAMPLES = 5;
const oracleBytes = readFileSync(join(directory, 'benchmark-cases.json'));
const oracle = JSON.parse(oracleBytes);
assert.equal(oracle.count, 96);
assert.equal(oracle.pattern_repetitions, 32);

const engineStart = now();
const engine = readEngine(enginePath);
const engineEnd = now();
const textHashes = {
  x87: 'd3af81f96a04d0a997ab841ce5ddfeac833f8cfe182d9473a99ec125c5dd6aed',
  sse2: '08fad7ecb51ad9026481bf5dbe347b26ef413fc292d4b27599cc6f26a62b243d',
};
const layouts = {
  x87: [[0, 21, 9], [21, 3, 1], [24, 16, 7], [40, 7, 4]],
  sse2: [[0, 24, 10], [24, 22, 6], [46, 32, 9], [78, 38, 13]],
};

function words(values) {
  const bytes = Buffer.alloc(values.length * 4);
  values.forEach((value, index) => bytes.writeUInt32LE(value >>> 0, index * 4));
  return bytes;
}

function caller(kind) {
  // edi survives both original functions, unlike x87's incidental eax result.
  const prefix = kind === 'x87' ? Buffer.concat([hex('5768'), words([INPUT])]) :
    Buffer.concat([hex('68'), words([SCALAR]), hex('5768'), words([COEFF]),
      hex('68'), words([INPUT]), hex('68'), words([VECTOR])]);
  const returnPc = CALLER + prefix.length + 5;
  const entry = Buffer.concat([prefix, hex('e8'), words([ENTRY - returnPc])]);
  const consume = kind === 'x87' ? Buffer.concat([hex('83c408db3d'), words([SCALAR])]) :
    Buffer.concat([hex('83c414a3'), words([COUNT_OUT])]);
  const jump = CALLER - (returnPc + consume.length + 2);
  assert.ok(jump >= -128 && jump < 0);
  return {returnPc, blocks: [
    {pc: CALLER, bytes: entry, instructions: kind === 'x87' ? 3 : 6},
    {pc: returnPc, bytes: Buffer.concat([consume, Buffer.from([0xeb, jump & 255])]), instructions: 3},
  ]};
}

function dirtyState() {
  const bytes = createArena();
  bytes.writeUInt32LE(0xcd7, 52);
  for (let index = 0; index < 8; index++) {
    bytes.writeBigUInt64LE(0x8000000000000100n + BigInt(index), FP + 40 + index * 10);
    bytes.writeUInt16LE(0x3fff, FP + 48 + index * 10);
    for (let lane = 0; lane < 16; lane++) bytes[SV + 32 + index * 16 + lane] = (lane * 13 + index * 19 + 11) & 255;
  }
  return bytes;
}

function benchmark(kind) {
  const row = oracle[kind], text = readFileSync(join(directory, kind, 'kernel.text.bin'));
  assert.equal(hash(text), textHashes[kind], 'complete retained original text');
  const instantiateStart = now();
  const instance = new WebAssembly.Instance(engine.module, {});
  const instantiateEnd = now();
  const memory = instance.exports.memory, api = {};
  for (const [name, fn] of Object.entries(instance.exports)) {
    if (name.startsWith('ring3_abi_v1_') && typeof fn === 'function') api[name.slice(13)] = fn;
  }
  const host = (name, args) => assert.equal(api[name](...args), 0, name);
  host('open', [12, kind === 'x87' ? 1 : 2, 0x4650424d]);
  const base = api.arena_ptr() >>> 0;
  const input = (offset, bytes) => new Uint8Array(memory.buffer, base + offset, bytes.length).set(bytes);
  const arena = () => Buffer.from(new Uint8Array(memory.buffer, base, ARENA_SIZE));
  const upload = (address, bytes) => { input(T, bytes); host('upload', [address, bytes.length]); };
  for (const [page, permission] of [[ENTRY, 7], [CALLER, 7], [0x5000, 3], [0x6000, 3],
    [0x7000, 3], [0x9000, 3], [0xa000, 3]]) host('map', [page, 1, permission]);

  const authored = caller(kind);
  const blocks = [...layouts[kind].map(([offset, length, instructions]) =>
    ({pc: ENTRY + offset, bytes: text.subarray(offset, offset + length), instructions})), ...authored.blocks];
  upload(ENTRY, text);
  for (const block of authored.blocks) upload(block.pc, block.bytes);
  host('protect', [ENTRY, 1, 5]);
  host('protect', [CALLER, 1, 5]);
  input(T, words(blocks.flatMap(block => [block.pc, block.bytes.length])));
  const captureStart = now();
  host('compile', [blocks.length]);
  const captureEnd = now();
  const moduleBytes = Buffer.from(new Uint8Array(memory.buffer, api.module_ptr() >>> 0, api.module_len() >>> 0));
  const module = new WebAssembly.Module(moduleBytes), imports = WebAssembly.Module.imports(module);
  assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  const counters = Object.create(null), ring3 = {};
  let scalarPreparations = 0;
  const allowed = new Set(['guard', 'read32', 'store32', 'x87_op', 'cpu_read', 'cpu_write', 'simd_execute']);
  for (const item of imports) {
    if (item.kind === 'memory') {
      assert.deepEqual(item, {module: 'env', name: 'memory', kind: 'memory'});
      continue;
    }
    assert.equal(item.module, 'ring3');
    assert.equal(item.kind, 'function');
    assert.ok(allowed.has(item.name), item.name);
    counters[item.name] = 0;
    ring3[item.name] = (...args) => {
      counters[item.name]++;
      if (item.name === 'simd_execute' && args[0] === 0x223001 && args[1] === 0x818) scalarPreparations++;
      return api[item.name](...args);
    };
  }
  const run = new WebAssembly.Instance(module, {env: {memory}, ring3}).exports.run;
  const generatedEnd = now();
  assert.equal(run.length, 4);

  const pattern = hex(row.input_pattern_hex);
  assert.equal(pattern.length, 48);
  const values = Buffer.concat(Array.from({length: oracle.pattern_repetitions}, () => pattern));
  const frame = bytes => Buffer.concat([Buffer.alloc(16, 0xa5), bytes, Buffer.alloc(16, 0x5a)]);
  const inputFrame = frame(values);
  const vectorFrame = frame(Buffer.alloc(values.length, 0xcc));
  const scalarFrame = Buffer.alloc(64, 0x79), stackFrame = Buffer.alloc(128, 0x6d);
  const coefficientFrame = kind === 'sse2' ? frame(hex(row.coefficients_hex)) : null;
  upload(INPUT - 16, inputFrame);
  upload(VECTOR - 16, vectorFrame);
  upload(SCALAR - 16, scalarFrame);
  upload(STACK, stackFrame);
  if (coefficientFrame) upload(COEFF - 16, coefficientFrame);

  const seeded = dirtyState();
  const registers = [0xa1001234, 0xa1012345, 0xa1023456, 0xa1034567, ESP, 0xa1056789, 0xa106789a, oracle.count];
  seeded.set(words([...registers, CALLER, 0xcd7]), 16);
  if (kind === 'x87') seeded.writeUInt16LE(row.control, FP + 16);
  else seeded.writeUInt32LE(row.expected_mxcsr, SV + 16);
  input(0, seeded);

  const cycleRetired = kind === 'x87' ? 20 + 7 * oracle.count : 38 + 9 * oracle.count;
  const budget = CYCLES * cycleRetired;
  const expectedRegisters = [...registers];
  expectedRegisters[0] = kind === 'x87' ? 0 : oracle.count;
  expectedRegisters[1] = kind === 'x87' ? INPUT + values.length : SCALAR;
  if (kind === 'sse2') expectedRegisters[2] = COEFF;
  const expectedFp = Buffer.from(seeded.subarray(FP, FP + 128));
  const expectedSimd = Buffer.from(seeded.subarray(SV, SV + 160));
  if (kind === 'x87') {
    expectedFp.writeUInt16LE(row.status & ~0x3800, 18);
    expectedFp.writeUInt16LE(0xffff, 20);
    expectedFp.writeUInt16LE(0x33d, 22);
    expectedFp.writeUInt32LE(authored.returnPc + 3, 24);
    expectedFp.writeUInt32LE(SCALAR, 28);
    expectedFp.writeUInt16LE(0x1b, 32);
    expectedFp.writeUInt16LE(0x23, 34);
    pattern.subarray(32, 42).copy(expectedFp, 40 + 6 * 10);
    hex(row.expected_output_hex).copy(expectedFp, 40 + 7 * 10);
    hex(row.expected_output_hex).copy(scalarFrame, 16);
    for (const [address, value] of [[0x907c, oracle.count], [0x9078, INPUT],
      [0x9074, authored.returnPc], [0x9070, registers[5]]]) stackFrame.writeUInt32LE(value >>> 0, address - STACK);
    hex(row.expected_output_hex).copy(stackFrame, 0x9050 - STACK);
  } else {
    assert.equal(row.expected_output_vectors_hex.length, oracle.count);
    Buffer.concat(row.expected_output_vectors_hex.map(hex)).copy(vectorFrame, 16);
    hex(row.expected_scalar_hex).copy(scalarFrame, 16);
    scalarFrame.writeUInt32LE(oracle.count, COUNT_OUT - (SCALAR - 16));
    row.expected_final_xmm_hex.forEach((value, index) => hex(value).copy(expectedSimd, 32 + index * 16));
    for (const [address, value] of [[0x907c, SCALAR], [0x9078, oracle.count], [0x9074, COEFF],
      [0x9070, INPUT], [0x906c, VECTOR], [0x9068, authored.returnPc], [0x9064, registers[5]],
      [0x9060, registers[3]], [0x905c, registers[7]], [0x9058, registers[6]]]) {
      stackFrame.writeUInt32LE(value >>> 0, address - STACK);
    }
  }

  let diagnosticReads = 0;
  function checkRam(address, expected, name) {
    for (let index = 0; index < expected.length; index++) {
      assert.equal(api.read8(address + index), 0, name);
      assert.equal(arena().readUInt32LE(120), expected[index], name + ' byte ' + index);
      diagnosticReads++;
    }
  }
  function check() {
    const actual = arena();
    assert.deepEqual(actual.subarray(16, 56), words([...expectedRegisters, CALLER, 0x412]), 'natural call cycle CPU and callee-preserved registers');
    assert.equal(actual.readUInt16LE(60), 6, 'CPU-family exit version');
    assert.equal(actual.readUInt32LE(72), 1, 'whole-cycle budget exit');
    assert.equal(actual.readUInt32LE(76), budget, 'whole original and caller retirement');
    assert.deepEqual(actual.subarray(FP, FP + 128), expectedFp, 'complete x87 state and balanced TOP');
    assert.deepEqual(actual.subarray(SV, SV + 160), expectedSimd, 'literal XMM values, preserved lanes and MXCSR');
    assert.deepEqual(actual.subarray(4524), seeded.subarray(4524), 'segments and work record unchanged');
    checkRam(INPUT - 16, inputFrame, 'unchanged original input/padding/canaries');
    checkRam(VECTOR - 16, vectorFrame, 'literal vector output and canaries');
    checkRam(SCALAR - 16, scalarFrame, 'literal consumed result and canaries');
    checkRam(STACK, stackFrame, 'natural cdecl stack and local total');
    if (coefficientFrame) checkRam(COEFF - 16, coefficientFrame, 'unchanged coefficients and canaries');
  }
  function invoke() {
    assert.equal(run(base, base + 56, budget, base + 96), 0, 'generated whole-function run');
  }
  const firstStart = now();
  invoke();
  const firstEnd = now();
  check();
  const firstCounters = {...counters};
  assert.ok(scalarPreparations === 0 || scalarPreparations === CYCLES, 'zero or one original scalar preparation per function');
  const preparationsPerCycle = scalarPreparations / CYCLES;
  const perCycle = kind === 'x87' ? {read32: 4, store32: 4, x87_op: 4 + 4 * oracle.count} :
    {read32: 10, store32: 11, cpu_read: 4 + oracle.count, cpu_write: oracle.count + 1,
      simd_execute: 3 * oracle.count + 3 + preparationsPerCycle};
  assert.deepEqual(firstCounters, {guard: 1, ...Object.fromEntries(Object.entries(perCycle).map(([name, count]) => [name, count * CYCLES]))});

  for (let index = 0; index < WARMUP; index++) invoke();
  check();
  const samples = [];
  for (let sample = 0; sample < SAMPLES; sample++) {
    const before = {...counters}, start = now();
    for (let call = 0; call < BATCH_CALLS; call++) invoke();
    const end = now();
    check();
    const functions = BATCH_CALLS * CYCLES, elements = functions * oracle.count;
    const crossings = Object.fromEntries(Object.entries(counters).map(([name, count]) => [name, count - before[name]]));
    assert.deepEqual(crossings, Object.fromEntries(Object.entries(firstCounters).map(([name, count]) => [name, count * BATCH_CALLS])));
    samples.push({sample, elapsed_ns: elapsed(start, end), run_calls: BATCH_CALLS, functions, elements,
      retired: BATCH_CALLS * budget, helper_calls: crossings,
      ns_per_function: elapsed(start, end) / functions, ns_per_element: elapsed(start, end) / elements,
      ns_per_guest_instruction: elapsed(start, end) / (BATCH_CALLS * budget)});
  }
  const totalRuns = 1 + WARMUP + SAMPLES * BATCH_CALLS;
  assert.deepEqual({...counters}, Object.fromEntries(Object.entries(firstCounters).map(([name, count]) => [name, count * totalRuns])));
  assert.equal(scalarPreparations, preparationsPerCycle * totalRuns * CYCLES);
  const median = key => samples.map(sample => sample[key]).sort((a, b) => a - b)[2];
  const final = arena();
  writeFileSync(join(output, kind + '-module.wasm'), moduleBytes, {flag: 'wx'});
  writeFileSync(join(output, kind + '-final-arena.bin'), final, {flag: 'wx'});
  host('close', []);
  return {kind, count: oracle.count, original_text_sha256: hash(text), caller_hex: authored.blocks.map(block => block.bytes.toString('hex')),
    cycle_retired: cycleRetired, cycles_per_run: CYCLES, budget, preparations_per_cycle: preparationsPerCycle,
    generated: {sha256: hash(moduleBytes), bytes: moduleBytes.length, imports,
      blocks: blocks.map(block => ({pc: block.pc, length: block.bytes.length, instructions: block.instructions}))},
    cold: {engine_instantiate_ns: elapsed(instantiateStart, instantiateEnd), guest_capture_ns: elapsed(captureStart, captureEnd),
      generated_compile_and_instantiate_ns: elapsed(captureEnd, generatedEnd), first_run_ns: elapsed(firstStart, firstEnd),
      first_run_functions: CYCLES, first_run_helpers: firstCounters},
    samples, median_ns_per_function: median('ns_per_function'), median_ns_per_element: median('ns_per_element'),
    median_ns_per_guest_instruction: median('ns_per_guest_instruction'),
    total: {runs: totalRuns, functions: totalRuns * CYCLES, elements: totalRuns * CYCLES * oracle.count,
      retired: totalRuns * budget, helper_calls: {...counters}, scalar_preparations: scalarPreparations},
    validated: {checks: 2 + SAMPLES, diagnostic_byte_reads: diagnosticReads, final_arena_sha256: hash(final),
      output_hex: kind === 'x87' ? row.expected_output_hex : row.expected_scalar_hex,
      x87_hex: expectedFp.toString('hex'), simd_hex: expectedSimd.toString('hex')}};
}

const workloads = ['x87', 'sse2'].map(benchmark);
const result = {status: 'PASS', label, benchmark_source_sha256: hash(readFileSync(new URL(import.meta.url))),
  oracle_sha256: hash(oracleBytes), engine: {path: enginePath, sha256: engine.sha256, bytes: engine.bytes.length},
  engine_compile_ns: elapsed(engineStart, engineEnd),
  protocol: {profile: 'replacement explicit; complete original function and natural caller loop',
    cycles_per_run: CYCLES, warmup_calls: WARMUP, batch_calls: BATCH_CALLS, samples: SAMPLES,
    clock: 'process.hrtime.bigint; monotonic nanoseconds',
    timed: 'generated run calls including canonical forwarding, aggregate helper counters and run status assertion; all state/RAM validation outside timing; no host reseed between calls'},
  workloads,
  scope: 'two distinct retained compiler-produced functions with exact count96 inputs; ns/function and ns/element are reported separately, not equivalent algorithm comparisons. Cold stages and warm samples are separate. Existing four-profile numerical/fault proof is reused; selected RAM, complete FP/SIMD state and natural cdecl preservation are checked outside timing. No application, whole CPU, native hardware equivalence or helper time-attribution claim'};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2) + '\n', {flag: 'wx'});
console.log(JSON.stringify({status: result.status, output,
  workloads: workloads.map(({kind, median_ns_per_function, median_ns_per_element, total}) =>
    ({kind, median_ns_per_function, median_ns_per_element, total}))}));
