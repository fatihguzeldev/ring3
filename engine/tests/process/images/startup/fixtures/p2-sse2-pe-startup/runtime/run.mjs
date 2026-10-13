import {ARENA_SIZE} from '../../../../../../support/wasm/arena.mjs';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../../../../../../support/wasm/engine.mjs';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root, 'expected engine, output directory and repository root');
const fixture = 'engine/tests/process/images/startup/fixtures/p2-sse2-pe-startup';
const SIZE = ARENA_SIZE, T = 140, FP = 4236, SV = 4364;
const KEY_LOW = 0x53534532, KEY_HIGH = 0x50455354;
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const engine = readEngine(enginePath);
const initial = readFileSync(join(output, 'initial-arena.bin'));
const image = readFileSync(join(root, fixture, 'program.exe'));
const oracleBytes = readFileSync(join(root, fixture, 'oracle.json'));
const oracle = JSON.parse(oracleBytes);
const kernel = readFileSync(join(root, fixture, 'kernel.text.bin'));
assert.equal(initial.length, SIZE);
assert.equal(oracle.schema, 'ring3.sse2-pe-startup.v1');
assert.equal(image.length, oracle.image.bytes);
assert.equal(hash(image), oracle.image.sha256);
const sourceCaseBytes = readFileSync(join(root, 'engine/tests/programs/numeric/fixtures/p2-fp-compiled/sse2/cases.json'));
assert.equal(hash(sourceCaseBytes), oracle.source_case.sha256);
const sourceCase = JSON.parse(sourceCaseBytes).cases.find(row => row.name === oracle.source_case.name);
assert.ok(sourceCase);
assert.equal(sourceCase.count, 3);
assert.deepEqual(sourceCase.input_raw64, oracle.expected.input_raw64);
assert.deepEqual(sourceCase.coefficients_raw64, oracle.expected.coefficients_raw64);
assert.deepEqual(sourceCase.output_raw64, oracle.expected.vector_output_raw64);
assert.equal(sourceCase.scalar_output_raw64, oracle.expected.scalar_output_raw64);
assert.equal(hash(kernel), '08fad7ecb51ad9026481bf5dbe347b26ef413fc292d4b27599cc6f26a62b243d');
assert.equal(kernel.length, 116);
assert.deepEqual(WebAssembly.Module.imports(engine.module), []);

const artifacts = {}, contexts = [], modules = [], observations = [], helperTrace = [], diagnostics = [];
const counts = {contexts: 0, modules: 0, dispatcher_modules: 0, runs: 0, retired: 0,
  helper_calls: 0, read32: 0, store32: 0, cpu_read: 0, cpu_write: 0, simd_execute: 0,
  natural_returns: 0, cold_exits: 0, diagnostic_words: 0};
function save(name, bytes) {
  writeFileSync(join(output, name), bytes, {flag: 'wx'});
  artifacts[name] = {bytes: bytes.length, sha256: hash(bytes)};
  return name;
}
const sourcePaths = [
  `${fixture}/runtime/run.mjs`, `${fixture}/oracle.json`, `${fixture}/entry.c`, `${fixture}/build.py`,
  `${fixture}/program.exe`, `${fixture}/kernel.obj`, `${fixture}/kernel.text.bin`, `${fixture}/receipt.json`,
  'engine/tests/programs/numeric/fixtures/p2-fp-compiled/sse2/cases.json',
  'engine/tests/support/wasm/engine.mjs', 'engine/src/process/loading/input.rs', 'engine/src/process/loading/mod.rs', 'engine/src/process/mod.rs',
  'engine/src/process/loading/startup.rs', 'engine/src/process/execution/installation.rs', 'engine/src/process/execution/mod.rs',
  'engine/src/process/cpu/simd.rs', 'engine/src/process/cpu/mod.rs', 'engine/src/process/memory/span.rs', 'engine/src/process/memory/mod.rs',
  'engine/src/cpu/dbt/wasm/dispatcher.rs', 'engine/src/cpu/dbt/wasm/simd/direct.rs',
];
const sourceHashes = () => Object.fromEntries(sourcePaths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcesBefore = sourceHashes();
save('engine.wasm', engine.bytes);
save('input-program.exe', image);
save('oracle.json', oracleBytes);

function word(value) {
  const bytes = Buffer.alloc(4);
  bytes.writeUInt32LE(value >>> 0);
  return bytes;
}
const words = values => Buffer.concat(values.map(word));
function record(magic, version, size, values) {
  const bytes = Buffer.alloc(size);
  bytes.write(magic);
  bytes.writeUInt16LE(version, 4);
  bytes.writeUInt16LE(1, 6);
  bytes.writeUInt32LE(size, 8);
  values.forEach((value, index) => bytes.writeUInt32LE(value >>> 0, 16 + index * 4));
  return bytes;
}
const state = (registers, pc, flags) => record('R3ST', 1, 56, [...registers, pc, flags]);
const exit = (reason, retired) => record('R3EX', reason === 3 ? 3 : 6, 40, [reason, retired, 0, 0, 0, 0]);
const packet = (version, value = 0, length = 0) => record('R3MH', version, 40, [0, value, 0, 0, 0, length]);
const residentRecord = unit => words([1, 24, unit.low, unit.high, unit.pointer, unit.length]);
const installation = unit => record('R3IN', 1, 32, [unit.low, unit.high, unit.slot, 0]);
const arena = c => Buffer.from(new Uint8Array(c.memory.buffer, c.base, SIZE));
function input(c, bytes) {
  const expected = arena(c);
  expected.set(bytes, T);
  new Uint8Array(c.memory.buffer, c.base + T, bytes.length).set(bytes);
  assert.deepEqual(arena(c), expected, 'only declared transfer input changes');
}
function host(c, label, fn, patches = []) {
  const expected = arena(c);
  for (const [offset, bytes] of patches) expected.set(bytes, offset);
  assert.equal(fn(), 0, label);
  assert.deepEqual(arena(c), expected, `${label}: full4612`);
}
function cpu(c) {
  const bytes = arena(c);
  return {registers: Array.from({length: 8}, (_, index) => bytes.readUInt32LE(16 + index * 4)),
    pc: bytes.readUInt32LE(48), flags: bytes.readUInt32LE(52)};
}

// this is a literal expected loader projection, independent of guest execution.
function expectedImage(actualBase) {
  const pe = image.readUInt32LE(0x3c), optional = pe + 24;
  assert.equal(image.readUInt16LE(optional), 0x10b);
  const preferred = image.readUInt32LE(optional + 28), size = image.readUInt32LE(optional + 56);
  const bytes = Buffer.alloc(size);
  image.copy(bytes, 0, 0, image.readUInt32LE(optional + 60));
  const sections = image.readUInt16LE(pe + 6), table = optional + image.readUInt16LE(pe + 20);
  for (let index = 0; index < sections; index++) {
    const at = table + index * 40, rva = image.readUInt32LE(at + 12);
    const length = image.readUInt32LE(at + 16), start = image.readUInt32LE(at + 20);
    image.copy(bytes, rva, start, start + length);
  }
  assert.equal(preferred, 0x400000);
  assert.equal(size, 0x5000);
  for (const rva of [0x1001, 0x1008, 0x100d, 0x1012, 0x101f, 0x1025]) {
    bytes.writeUInt32LE((bytes.readUInt32LE(rva) + actualBase - preferred) >>> 0, rva);
  }
  assert.deepEqual(bytes.subarray(0x1030, 0x10a4), kernel, 'the linked original kernel remains whole and unchanged');
  return bytes;
}
function ramRead(c, address, length) {
  if (address >= c.actualBase && address + length <= c.actualBase + c.imageBytes.length) {
    return c.imageBytes.subarray(address - c.actualBase, address - c.actualBase + length);
  }
  assert.ok(address >= 0x8000 && address + length <= 0x9000, 'only engine-loaded image or startup stack');
  return c.stack.subarray(address - 0x8000, address - 0x8000 + length);
}
function ramWrite(c, address, bytes) {
  if (address >= c.actualBase && address + bytes.length <= c.actualBase + c.imageBytes.length) {
    bytes.copy(c.imageBytes, address - c.actualBase);
  } else {
    assert.ok(address >= 0x8000 && address + bytes.length <= 0x9000);
    bytes.copy(c.stack, address - 0x8000);
  }
}
function checkRam(c, address, expected, label) {
  assert.equal(expected.length % 4, 0);
  for (let offset = 0; offset < expected.length; offset += 4) {
    const value = expected.readUInt32LE(offset);
    host(c, label, () => c.api.read32(address + offset), [[100, packet(1, value)]]);
    diagnostics.push({context: c.id, label, address: address + offset, expected_value: value});
    counts.diagnostic_words++;
  }
}
function bindHelper(c, unit, name) {
  return (...raw) => {
    const before = arena(c), expected = Buffer.from(before);
    const args = raw.map(value => value >>> 0);
    if (name === 'guard_resident') {
      assert.deepEqual(args, [KEY_LOW, KEY_HIGH, unit.low, unit.high, c.base, c.base + 56, c.base + 96]);
      assert.equal(c.api[name](...raw), 0);
      assert.deepEqual(arena(c), before, 'resident guard is neutral');
      return 0;
    }
    const resident = ['cpu_store_resident32', 'cpu_write_resident', 'simd_execute_resident'].includes(name);
    if (resident) assert.deepEqual(args.slice(0, 4), [KEY_LOW, KEY_HIGH, unit.low, unit.high]);
    const payload = args.slice(resident ? 4 : 0);
    let helper, family;
    if (name === 'cpu_read32') {
      family = 'read32';
      const next = c.reads.shift();
      assert.deepEqual(payload, [next.address], 'ordered original stack read');
      helper = packet(10, next.value, 4);
      assert.equal(ramRead(c, next.address, 4).readUInt32LE(), next.value);
    } else if (name === 'cpu_store_resident32') {
      family = 'store32';
      const next = c.stores.shift();
      assert.deepEqual(payload, [next.address, next.value], 'ordered guest stack or completion write');
      ramWrite(c, next.address, word(next.value));
      helper = packet(10, 0, 4);
    } else if (name === 'cpu_read') {
      family = 'cpu_read';
      assert.equal(payload[1], 16);
      assert.equal(payload[2], 16);
      const next = c.spanReads.shift();
      assert.equal(payload[0], next, 'ordered original vector read');
      expected.set(ramRead(c, payload[0], 16), T);
      helper = packet(7, 0, 16);
    } else if (name === 'cpu_write_resident') {
      family = 'cpu_write';
      const next = c.spanWrites.shift();
      assert.deepEqual(payload, [next.address, next.bytes.length, next.bytes.length === 16 ? 16 : 1]);
      assert.deepEqual(before.subarray(T, T + next.bytes.length), next.bytes, 'literal original FP output before checked memory write');
      ramWrite(c, next.address, next.bytes);
      helper = packet(7, 0, next.bytes.length);
    } else {
      assert.equal(name, 'simd_execute_resident');
      family = 'simd_execute';
      helper = packet(7);
    }
    assert.equal(c.api[name](...raw), 0, name);
    const after = arena(c);
    if (family === 'simd_execute') {
      expected.set(after.subarray(SV, SV + 160), SV);
      expected.set(words([0, payload[5]]), T + 512);
    }
    expected.set(helper, 100);
    assert.deepEqual(after, expected, `${name}: exact declared helper mutations`);
    c.latest = after;
    counts.helper_calls++;
    counts[family]++;
    helperTrace.push({context: c.id, phase: c.phase, name, args, packet_hex: helper.toString('hex')});
    return 0;
  };
}
function capture(c, label, rvas, slot) {
  input(c, words(rvas.map(rva => c.actualBase + rva)));
  const before = arena(c);
  assert.equal(c.api.compile_resident_entries(rvas.length, 0), 0);
  const after = arena(c);
  const unit = {label, slot, entries: rvas.map(rva => c.actualBase + rva), low: after.readUInt32LE(T + 8),
    high: after.readUInt32LE(T + 12), pointer: after.readUInt32LE(T + 16), length: after.readUInt32LE(T + 20)};
  before.set(residentRecord(unit), T);
  assert.deepEqual(after, before, 'resident capture changes only its24-byte receipt');
  assert.ok(unit.length > 8 && unit.length <= 65536);
  const bytes = Buffer.from(new Uint8Array(c.memory.buffer, unit.pointer, unit.length));
  const module = new WebAssembly.Module(bytes), imports = WebAssembly.Module.imports(module), ring3 = {};
  assert.deepEqual(imports[0], {module: 'env', name: 'memory', kind: 'memory'});
  const allowed = new Set(['guard_resident', 'cpu_read32', 'cpu_store_resident32', 'cpu_read', 'cpu_write_resident', 'simd_execute_resident']);
  for (const item of imports.slice(1)) {
    assert.equal(item.module, 'ring3');
    assert.equal(item.kind, 'function');
    assert.ok(allowed.has(item.name), item.name);
    ring3[item.name] = bindHelper(c, unit, item.name);
  }
  const instance = new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3});
  assert.equal(instance.exports.run.length, 4);
  c.table.set(slot, instance.exports.run);
  host(c, 'acknowledge exact table installation', () => c.api.acknowledge_resident_installation(KEY_LOW, KEY_HIGH, unit.low, unit.high, slot), [[T, installation(unit)]]);
  unit.instance = instance;
  c.units.push(unit);
  const file = save(`context-${c.id}-${label}.wasm`, bytes);
  modules.push({context: c.id, label, file, sha256: hash(bytes), bytes: bytes.length,
    entries: unit.entries, id_low: unit.low, id_high: unit.high, slot, imports});
  counts.modules++;
  return unit;
}
function dispatcher(c) {
  const before = arena(c);
  assert.equal(c.api.dispatcher_module(KEY_LOW, KEY_HIGH), 0);
  const after = arena(c), pointer = after.readUInt32LE(T + 24), length = after.readUInt32LE(T + 28);
  before.set(record('R3DP', 1, 32, [KEY_LOW, KEY_HIGH, pointer, length]), T);
  assert.deepEqual(after, before, 'dispatcher metadata is its only mutation');
  const bytes = Buffer.from(new Uint8Array(c.memory.buffer, pointer, length)), module = new WebAssembly.Module(bytes);
  assert.deepEqual(WebAssembly.Module.imports(module), [
    {module: 'env', name: 'memory', kind: 'memory'}, {module: 'env', name: 'table', kind: 'table'},
    {module: 'ring3', name: 'guard_dispatch_entry', kind: 'function'},
    {module: 'ring3', name: 'find_installed_resident', kind: 'function'},
  ]);
  const ring3 = {
    guard_dispatch_entry: (...args) => {
      const snapshot = arena(c);
      assert.equal(c.api.guard_dispatch_entry(...args), 0);
      assert.deepEqual(arena(c), snapshot);
      return 0;
    },
    find_installed_resident: (...args) => {
      const snapshot = arena(c), unit = c.units.find(candidate => candidate.entries.includes(args[2] >>> 0));
      assert.deepEqual(args.slice(0, 2).map(value => value >>> 0), [KEY_LOW, KEY_HIGH]);
      const status = c.api.find_installed_resident(...args);
      assert.equal(status, unit ? 0 : 17);
      if (unit) snapshot.set(installation(unit), T);
      assert.deepEqual(arena(c), snapshot, 'finder changes only a successful installation receipt');
      c.latest = arena(c);
      return status;
    },
  };
  c.dispatcher = new WebAssembly.Instance(module, {env: {memory: c.memory, table: c.table}, ring3});
  save(`context-${c.id}-dispatcher.wasm`, bytes);
  counts.dispatcher_modules++;
}
function run(c, label, budget, retired, reason, registers, rva, flags, simd) {
  const before = arena(c), at = helperTrace.length;
  c.latest = before;
  c.phase = label;
  assert.equal(c.dispatcher.exports.run(c.base, c.base + 56, budget, c.base + 96), 0, label);
  const expected = Buffer.from(before);
  for (const [start, end] of [[100, 140], [T, T + 32], [T + 512, T + 520]]) {
    expected.set(c.latest.subarray(start, end), start);
  }
  expected.set(state(registers, c.actualBase + rva, flags), 0);
  expected.set(exit(reason, retired), 56);
  expected.set(initial.subarray(FP, FP + 128), FP);
  expected.set(simd, SV);
  assert.deepEqual(arena(c), expected, `${label}: whole4612 including initial FP/segments/work`);
  const file = save(`context-${c.id}-${label}-arena.bin`, arena(c));
  observations.push({context: c.id, label, budget, retired, reason, cpu: cpu(c), helper_calls: helperTrace.length - at, arena: file});
  counts.runs++;
  counts.retired += retired;
  if (reason === 3) counts.cold_exits++;
}

for (const actualBase of [0x400000, 0x500000]) {
  const instance = new WebAssembly.Instance(engine.module, {}), api = {};
  for (const [name, value] of Object.entries(instance.exports)) {
    if (name.startsWith('ring3_abi_v1_') && typeof value === 'function') api[name.slice(13)] = value;
  }
  const c = {id: ++counts.contexts, actualBase, api, memory: instance.exports.memory, units: [],
    imageBytes: expectedImage(actualBase), stack: Buffer.alloc(4096)};
  assert.equal(api.open(6, KEY_LOW, KEY_HIGH), 0);
  c.base = api.arena_ptr() >>> 0;
  assert.deepEqual(arena(c), initial, 'fresh engine owns canonical complete CPU arena');
  host(c, 'begin unchanged linker-produced image', () => api.begin_image_input(image.length));
  input(c, image);
  host(c, 'append complete staged image', () => api.append_image_input(0, image.length));
  host(c, 'load unlinked PE at explicit base', () => api.load_pe32_input_at(actualBase),
    [[T, record('R3PE', 1, 32, [actualBase, 0x5000, actualBase + 0x1000, 5])]]);
  const originalReadOnly = Buffer.from(c.imageBytes.subarray(0x2000, 0x2080));
  assert.deepEqual(originalReadOnly.subarray(0, 112), Buffer.from(oracle.expected.rdata_hex, 'hex'));
  checkRam(c, actualBase + 0x1000, c.imageBytes.subarray(0x1000, 0x10a4), 'exact relocated text and unchanged kernel');
  checkRam(c, actualBase + 0x2000, originalReadOnly, 'linker input coefficients and readonly canary');
  checkRam(c, actualBase + 0x3000, Buffer.alloc(0x60), 'loader zero-filled output and canaries');
  const initialRegisters = [0, 0, 0, 0, 0x9000, 0, 0, 0];
  host(c, 'startup owns entry CPU and zeroed stack', () => api.start_loaded_image(0x8000, 1),
    [[0, state(initialRegisters, actualBase + 0x1000, 2)], [56, exit(3, 0)]]);
  checkRam(c, 0x8fc0, Buffer.alloc(64), 'startup empty stack');
  c.table = new WebAssembly.Table({element: 'anyfunc', initial: 8, maximum: 8});
  dispatcher(c);
  const freshSimd = initial.subarray(SV, SV + 160), finalSimd = Buffer.from(freshSimd);
  oracle.expected.final_xmm_hex.forEach((value, index) => Buffer.from(value, 'hex').copy(finalSimd, 32 + index * 16));
  finalSimd.writeUInt32LE(oracle.expected.final_mxcsr, 16);
  const b = actualBase;
  c.stores = [
    [0x8ffc, b + 0x3008], [0x8ff8, 3], [0x8ff4, b + 0x2030], [0x8ff0, b + 0x2000],
    [0x8fec, b + 0x3010], [0x8fe8, b + 0x101b], [0x8fe4, 0], [0x8fe0, 0], [0x8fdc, 0], [0x8fd8, 0],
    [b + 0x3040, 3], [b + 0x3044, 0x52494e47],
  ].map(([address, value]) => ({address, value}));
  c.reads = [
    [0x8ffc, b + 0x3008], [0x8ff8, 3], [0x8ff4, b + 0x2030], [0x8ff0, b + 0x2000], [0x8fec, b + 0x3010],
    [0x8fd8, 0], [0x8fdc, 0], [0x8fe0, 0], [0x8fe4, 0], [0x8fe8, b + 0x101b],
  ].map(([address, value]) => ({address, value}));
  c.spanReads = [0x2030, 0x2040, 0x2050, 0x2000, 0x2010, 0x2020, 0x2060].map(rva => b + rva);
  const vectorBytes = Buffer.from(oracle.expected.vector_output_hex, 'hex');
  c.spanWrites = Array.from({length: 3}, (_, index) => ({address: b + 0x3010 + index * 16, bytes: vectorBytes.subarray(index * 16, index * 16 + 16)}));
  c.spanWrites.push({address: b + 0x3008, bytes: Buffer.from(oracle.expected.scalar_output_hex, 'hex')});
  run(c, 'initial-cold-entry', 66, 0, 3, initialRegisters, 0x1000, 2, freshSimd);
  capture(c, 'entry', [0x1000], 0);
  const called = [...initialRegisters]; called[4] = 0x8fe8;
  run(c, 'natural-call-cold-kernel', 66, 6, 3, called, 0x1030, 2, freshSimd);
  capture(c, 'whole-original-kernel', [0x1030, 0x1048, 0x105e, 0x107e], 1);
  const returned = [3, b + 0x3008, b + 0x2030, 0, 0x8fec, 0, 0, 0];
  run(c, 'natural-return-cold-continuation', 60, 56, 3, returned, 0x101b, 0x46, finalSimd);
  counts.natural_returns++;
  capture(c, 'guest-completion', [0x101b], 2);
  const finalRegisters = [...returned]; finalRegisters[4] = 0x9000;
  run(c, 'guest-completion-marker-and-stable-jump', 4, 4, 1, finalRegisters, 0x102d, 0x16, finalSimd);
  assert.equal(c.stores.length + c.reads.length + c.spanReads.length + c.spanWrites.length, 0);
  const finalData = Buffer.alloc(0x60);
  Buffer.from(oracle.expected.scalar_output_hex, 'hex').copy(finalData, 8);
  vectorBytes.copy(finalData, 16);
  Buffer.from(oracle.expected.completion_hex, 'hex').copy(finalData, 0x40);
  const finalStack = Buffer.alloc(64);
  for (const slot of oracle.expected.final_stack_words) {
    finalStack.writeUInt32LE(slot.value_rva === undefined ? slot.value : b + slot.value_rva, slot.address - 0x8fc0);
  }
  assert.deepEqual(c.stack.subarray(0xfc0), finalStack, 'independent final natural-call stack projection');
  checkRam(c, b + 0x3000, finalData, 'literal scalar vectors completion and zero canaries');
  checkRam(c, b + 0x2000, originalReadOnly, 'loaded inputs stay unchanged');
  checkRam(c, 0x8fc0, finalStack, 'natural cdecl argument return and callee-save stack');
  checkRam(c, b + 0x1000, c.imageBytes.subarray(0x1000, 0x10a4), 'linked executable bytes stay unchanged');
  save(`context-${c.id}-final-arena.bin`, arena(c));
  contexts.push({id: c.id, actual_base: actualBase, startup_entry: b + 0x1000,
    final_pc: b + 0x102d, final_esp: 0x9000, resident_owners: c.units.length, stack_base: 0x8000, stack_pages: 1});
  host(c, 'close completed private image', () => api.close());
}
assert.deepEqual(sourceHashes(), sourcesBefore);
assert.deepEqual(counts, {contexts: 2, modules: 6, dispatcher_modules: 2, runs: 8, retired: 132,
  helper_calls: 90, read32: 20, store32: 24, cpu_read: 14, cpu_write: 8, simd_execute: 24,
  natural_returns: 2, cold_exits: 6, diagnostic_words: 452});
const result = {status: 'PASS', engine: {path: enginePath, sha256: engine.sha256, bytes: engine.bytes.length},
  image: {bytes: image.length, sha256: hash(image)}, oracle_sha256: hash(oracleBytes), source_sha256: sourcesBefore,
  initial_arena_sha256: hash(initial), counts, contexts, modules, observations, helper_trace: helperTrace,
  diagnostics, artifacts, claim: 'linker-produced original SSE2 PE loaded and started at preferred and rebased addresses; engine-owned CPU/stack, three installed resident units, observed cold CALL/RET continuation, literal outputs and completion marker without host CPU/data reseeding; full4612 assertions and selected RAM only; no Windows CRT/import/TLS/OS or performance claim'};
save('result.json', Buffer.from(JSON.stringify(result, null, 2) + '\n'));
console.log(JSON.stringify({output, counts, result: join(output, 'result.json')}));
