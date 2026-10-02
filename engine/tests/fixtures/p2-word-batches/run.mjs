import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';

const [enginePath, outputDir, root] = process.argv.slice(2);
assert.ok(root, 'expected actual engine, output directory and repository root');
const engineBytes = readFileSync(enginePath);
assert.ok(WebAssembly.validate(engineBytes), 'actual engine validates');
const engineModule = new WebAssembly.Module(engineBytes);
assert.deepEqual(WebAssembly.Module.imports(engineModule), [], 'engine has no JS imports');
const names = ['open', 'close', 'arena_ptr', 'map', 'protect', 'unmap', 'upload', 'compile', 'compile_with_gates', 'capture_call', 'complete_call', 'abandon_call', 'generation', 'module_ptr', 'module_len', 'guard', 'read32', 'write32', 'store32', 'write_words32'];
let keys = 0n, modules = 0, actualRuns = 0;
const generatedHashes = [];

function hash(bytes) {
  return createHash('sha256').update(bytes).digest('hex');
}

function sortedImports(module) {
  return WebAssembly.Module.imports(module).sort((a, b) => `${a.module}.${a.name}`.localeCompare(`${b.module}.${b.name}`));
}

function refresh(engine) {
  if (engine.buffer !== engine.memory.buffer) {
    engine.buffer = engine.memory.buffer;
    engine.bytes = new Uint8Array(engine.buffer);
    engine.view = new DataView(engine.buffer);
  }
  return engine;
}

function fresh() {
  const instance = new WebAssembly.Instance(engineModule, {});
  const api = Object.fromEntries(names.map(name => {
    const fn = instance.exports[`ring3_abi_v1_${name}`];
    assert.equal(typeof fn, 'function', `actual export ${name}`);
    return [name, fn];
  }));
  const memory = instance.exports.memory;
  assert.ok(memory instanceof WebAssembly.Memory);
  const key = 0x8123456700000000n + ++keys;
  const engine = refresh({api, memory, key, low: Number(key & 0xffffffffn), high: Number(key >> 32n)});
  assert.equal(api.open(12, engine.low, engine.high), 0);
  engine.base = api.arena_ptr() >>> 0;
  refresh(engine);
  assert.ok(engine.base > 0 && engine.base + 4236 <= engine.bytes.length);
  assert.equal(api.map(0x1000, 1, 7), 0);
  return engine;
}

function arena(engine) {
  return refresh(engine).bytes.slice(engine.base, engine.base + 4236);
}

function upload(engine, address, bytes) {
  assert.ok(bytes.length <= 4096);
  refresh(engine).bytes.set(bytes, engine.base + 140);
  assert.equal(engine.api.upload(address, bytes.length), 0, `upload ${address}`);
  refresh(engine);
}

function header(bytes, view, pointer, magic, length, version = 1) {
  bytes.set(Buffer.from(magic, 'ascii'), pointer);
  view.setUint16(pointer + 4, version, true);
  view.setUint16(pointer + 6, 1, true);
  view.setUint32(pointer + 8, length, true);
  view.setUint32(pointer + 12, 0, true);
}

function helperRecord(tag, value = 0, detail = 0, address = 0, access = 0, length = tag === 1 ? 4 : 0) {
  const bytes = new Uint8Array(40), view = new DataView(bytes.buffer);
  header(bytes, view, 0, 'R3MH', 40);
  [tag, value, detail, address, access, length].forEach((field, index) => view.setUint32(16 + index * 4, field, true));
  return bytes;
}

function guestValue(engine, address) {
  assert.equal(engine.api.read32(address), 0, 'actual guest observation helper');
  refresh(engine);
  assert.equal(engine.view.getUint32(engine.base + 116, true), 0, `guest observation ${address} succeeds`);
  return engine.view.getUint32(engine.base + 120, true);
}

function word(engine, address, value) {
  assert.equal(engine.api.write32(address, value), 0, `initialize guest word ${address}`);
  assert.deepEqual(arena(engine).slice(100, 140), helperRecord(0));
}

function descriptors(engine, blocks, gates = []) {
  refresh(engine);
  [...blocks, ...gates].forEach(([entry, value], index) => {
    engine.view.setUint32(engine.base + 140 + index * 8, entry, true);
    engine.view.setUint32(engine.base + 144 + index * 8, value, true);
  });
}

function compile(engine, blocks, gates, expectedHelpers = []) {
  descriptors(engine, blocks, gates);
  assert.equal(engine.api.compile_with_gates(blocks.length, gates.length), 0, 'actual engine compilation');
  refresh(engine);
  const pointer = engine.api.module_ptr() >>> 0, length = engine.api.module_len() >>> 0;
  assert.ok(length > 8);
  const encoded = engine.bytes.slice(pointer, pointer + length);
  assert.ok(WebAssembly.validate(encoded));
  const module = new WebAssembly.Module(encoded);
  const expected = [
    {module: 'env', name: 'memory', kind: 'memory'},
    {module: 'ring3', name: 'guard', kind: 'function'},
    ...expectedHelpers.map(name => ({module: 'ring3', name, kind: 'function'})),
  ].sort((a, b) => `${a.module}.${a.name}`.localeCompare(`${b.module}.${b.name}`));
  assert.deepEqual(sortedImports(module), expected, 'only actual memory/guard/instruction helpers; no hot gate import');
  const instance = new WebAssembly.Instance(module, {
    env: {memory: engine.memory},
    ring3: {guard: engine.api.guard, read32: engine.api.read32, store32: engine.api.store32},
  });
  modules++;
  generatedHashes.push(hash(encoded));
  writeFileSync(join(outputDir, `generated-${modules}.wasm`), encoded);
  return {module, run: instance.exports.run, generation: engine.api.generation() >>> 0};
}

function state(eip = 0x1000, eflags = 0xcd7) {
  return {registers: [0x89abcdef, 0x13579bdf, 0x23456789, 0x3456789a, 0x456789ab, 0x56789abc, 0x6789abcd, 0x789abcde], eip, eflags};
}

function copyState(value) {
  return {...value, registers: [...value.registers]};
}

function writeState(engine, value, cancel = 0) {
  refresh(engine);
  header(engine.bytes, engine.view, engine.base, 'R3ST', 56);
  value.registers.forEach((register, index) => engine.view.setUint32(engine.base + 16 + index * 4, register, true));
  engine.view.setUint32(engine.base + 48, value.eip, true);
  engine.view.setUint32(engine.base + 52, value.eflags, true);
  engine.view.setUint32(engine.base + 96, cancel, true);
  engine.bytes.fill(0xa5, engine.base + 56, engine.base + 96);
  engine.bytes.fill(0x5a, engine.base + 100, engine.base + 140);
}

function readState(engine) {
  refresh(engine);
  assert.deepEqual(engine.bytes.slice(engine.base, engine.base + 16), Uint8Array.from([0x52, 0x33, 0x53, 0x54, 1, 0, 1, 0, 56, 0, 0, 0, 0, 0, 0, 0]));
  return {
    registers: Array.from({length: 8}, (_, index) => engine.view.getUint32(engine.base + 16 + index * 4, true)),
    eip: engine.view.getUint32(engine.base + 48, true),
    eflags: engine.view.getUint32(engine.base + 52, true),
  };
}

function exit(reason, retired, detail = 0, address = 0, access = 0, length = reason === 5 ? 4 : 0, version = 3) {
  const bytes = new Uint8Array(40), view = new DataView(bytes.buffer);
  header(bytes, view, 0, 'R3EX', 40, version);
  [reason, retired, detail, address, access, length].forEach((field, index) => view.setUint32(16 + index * 4, field, true));
  return bytes;
}

function run(engine, child, budget, expected, expectedExit, label, expectedHelper = undefined) {
  const before = arena(engine);
  assert.equal(child.run(engine.base, engine.base + 56, budget, engine.base + 96), 0, `${label}: canonical runtime exit status`);
  actualRuns++;
  assert.deepEqual(readState(engine), expected, `${label}: complete CPU state`);
  const after = arena(engine);
  assert.deepEqual(after.slice(56, 96), expectedExit, `${label}: canonical exit`);
  assert.deepEqual(after.slice(96, 100), before.slice(96, 100), `${label}: cancellation unchanged`);
  assert.deepEqual(after.slice(140), before.slice(140), `${label}: transfer unchanged`);
  if (expectedHelper !== undefined) assert.deepEqual(after.slice(100, 140), expectedHelper, `${label}: canonical helper result`);
}

const gateId = 0xfedcba98;
let batchCalls = 0, nonemptyBatches = 0, emptyBatches = 0, rejectedBatches = 0;
let guardRejections = 0, currentGuards = 0, captures = 0, completions = 0;

function batch(engine, pairs, status, label, count = pairs.length) {
  refresh(engine);
  pairs.forEach(([address, value], index) => {
    engine.view.setUint32(engine.base + 140 + 8 * index, address, true);
    engine.view.setUint32(engine.base + 144 + 8 * index, value, true);
  });
  const before = arena(engine);
  assert.equal(engine.api.write_words32(count), status, label);
  batchCalls++;
  if (status !== 0) rejectedBatches++;
  else if (count === 0) emptyBatches++;
  else nonemptyBatches++;
  assert.deepEqual(arena(engine), before, `${label}: full arena/helper/transfer unchanged`);
}

function current(engine, child, label) {
  assert.equal(engine.api.guard(engine.low, engine.high, child.generation, engine.base, engine.base + 56, engine.base + 96), 0, label);
  currentGuards++;
}

function rejectedGuard(engine, child, status, label) {
  const before = arena(engine);
  assert.equal(child.run(0xffffffff, 0xffffffff, 1, 0xffffffff), status, label);
  guardRejections++;
  assert.deepEqual(arena(engine), before, `${label}: rejects before invalid pointer access/writes`);
}

const engine = fresh();
refresh(engine).bytes.fill(0xa5, engine.base + 140, engine.base + 284);
batch(engine, [], 0, 'empty batch ignores garbage descriptors without an artifact');
batch(engine, [], 7, 'count eighteen rejected before descriptor access', 18);
assert.equal(engine.api.map(0x4000, 1, 3), 0);
const seventeen = Array.from({length: 17}, (_, index) => [0x4000 + 4 * index, 0x81000000 + index]);
batch(engine, seventeen, 0, 'all seventeen descriptor pairs consumed without an artifact');
for (const [address, value] of seventeen) assert.equal(guestValue(engine, address), value, 'independent full-word value including seventeenth pair');

upload(engine, 0x1000, Uint8Array.from([0x0f, 0x0b, 0, 0]));
const gate = compile(engine, [[0x1000, 2]], [[0x1000, gateId]]);
const stopped = state();
stopped.registers[4] = 0xffffffff;
writeState(engine, stopped);
run(engine, gate, 1, stopped, exit(8, 0, gateId), 'actual helper-free gate snapshot before host batches', arena(engine).slice(100, 140));

word(engine, 0x4004, 0xcafebabe);
batch(engine, [[0x4000, 0x11223344], [0x4001, 0xaabbccdd], [0x4000, 0x55667788]], 0, 'exact/partial aliases obey descriptor-order last-byte wins');
assert.equal(guestValue(engine, 0x4000), 0x55667788);
assert.equal(guestValue(engine, 0x4001), 0xaa556677, 'literal overlapped LE bytes 77 66 55 AA');
assert.equal(guestValue(engine, 0x4004), 0xcafebaaa, 'bytes outside overlap preserved');

for (const address of [0, 0xfffff000]) assert.equal(engine.api.map(address, 1, 3), 0);
batch(engine, [[0xfffffffc, 0xaabbccdd], [0, 0x11223344]], 0, 'independent last/high and zero words support explicit wrap');
assert.equal(guestValue(engine, 0xfffffffc), 0xaabbccdd);
assert.equal(guestValue(engine, 0), 0x11223344);
batch(engine, [[0x4000, 0xdeadc0de], [0x1000, 0x00000b0f], [0x9000, 1]], 8, 'later unmapped word preserves data and code prefixes atomically');
assert.equal(guestValue(engine, 0x4000), 0x55667788);
assert.equal(guestValue(engine, 0x1000), 0x00000b0f);
current(engine, gate, 'failed code-prefix batch consumes no observable code stamp');
run(engine, gate, 1, stopped, exit(8, 0, gateId), 'failed code-prefix batch retains actual executable gate', arena(engine).slice(100, 140));
batch(engine, [[0x4000, 0xdeadc0de], [0xfffffffd, 1]], 8, 'later individual width4 overflow prevents earlier write');
assert.equal(guestValue(engine, 0x4000), 0x55667788);
assert.equal(guestValue(engine, 0xfffffffc), 0xaabbccdd);
assert.equal(guestValue(engine, 0), 0x11223344);

assert.equal(engine.api.map(0x5000, 1, 3), 0);
assert.equal(engine.api.protect(0x4000, 2, 2), 0);
batch(engine, [[0x4ffe, 0x44332211]], 0, 'unaligned cross-page batch requires Write only');
assert.equal(engine.api.read32(0x4ffe), 0, 'observation helper returns canonical permission fault');
assert.deepEqual(arena(engine).slice(100, 140), helperRecord(1, 0, 2, 0x4ffe, 1));
assert.equal(engine.api.protect(0x4000, 2, 3), 0);
assert.equal(guestValue(engine, 0x4ffe), 0x44332211);
current(engine, gate, 'unrelated write-only data pages preserve gate stamps');

batch(engine, [[0x1000, 0x00000b0f]], 0, 'same-byte code host write commits with ordinary success zero');
assert.equal(guestValue(engine, 0x1000), 0x00000b0f);
rejectedGuard(engine, gate, 4, 'same-byte code invalidation precedes generated bad pointers');
const replacement = compile(engine, [[0x1000, 2]], [[0x1000, gateId]]);
batch(engine, [[0x1000, 0x0000900f]], 0, 'changed code host write also commits success zero');
assert.equal(guestValue(engine, 0x1000), 0x0000900f);
rejectedGuard(engine, replacement, 4, 'changed code invalidation precedes generated bad pointers');
batch(engine, [[0x4040, 0xdeadbeef]], 0, 'host batch remains available while installed code is stale');
assert.equal(guestValue(engine, 0x4040), 0xdeadbeef);

{
  const closed = fresh();
  assert.equal(closed.api.close(), 0);
  batch(closed, [], 5, 'closed context precedes empty success');
  batch(closed, [], 5, 'closed context precedes count validation', 18);
}

// reuse R3-299's LLVM-verified stdcall_2 caller: PUSH -1; PUSH 0;
// CALL [EBX]; NOP. no new assembler/native arithmetic oracle is claimed.
const caller = Uint8Array.from([0x6a, 0xff, 0x6a, 0, 0xff, 0x13, 0x90]);
{
  const parked = fresh();
  for (const address of [0x4000, 0x8000]) assert.equal(parked.api.map(address, 1, 3), 0);
  word(parked, 0x4000, 0x1100);
  upload(parked, 0x1000, caller);
  upload(parked, 0x1100, Uint8Array.from([0x0f, 0x0b]));
  const child = compile(parked, [[0x1000, 6], [0x1006, 1], [0x1100, 2]], [[0x1100, gateId]], ['read32', 'store32']);
  const start = state();
  start.registers[3] = 0x4000;
  start.registers[4] = 0x9000;
  const called = copyState(start);
  called.registers[4] = 0x8ff4;
  called.eip = 0x1100;
  writeState(parked, start);
  run(parked, child, 4, called, exit(8, 3, gateId), 'actual authored two-argument CALL reaches Gate', helperRecord(0));
  const beforeCapture = arena(parked);
  assert.equal(parked.api.capture_call(parked.low, parked.high, child.generation, 2, 2), 0);
  captures++;
  const golden = new Uint8Array(112), view = new DataView(golden.buffer);
  header(golden, view, 0, 'R3CF', 112);
  [1, gateId, 2, 2, 0x1100, 0x8ff4, 0x1006, 0, 0, 0xffffffff].forEach((value, index) => view.setUint32(16 + 4 * index, value, true));
  assert.deepEqual(arena(parked).slice(140, 252), golden, 'literal private-frame inputs captured by actual engine');
  assert.deepEqual(arena(parked).slice(0, 140), beforeCapture.slice(0, 140));
  assert.deepEqual(arena(parked).slice(252), beforeCapture.slice(252));

  batch(parked, [[0x8ff4, 0xf1234567], [0x9000, 1]], 8, 'pending later-fault batch retains return and parked frame');
  assert.equal(guestValue(parked, 0x8ff4), 0x1006);
  assert.equal(guestValue(parked, 0x8ff8), 0);
  assert.equal(guestValue(parked, 0x8ffc), 0xffffffff);
  rejectedGuard(parked, child, 12, 'failed pending data batch retains parked CPU guard');
  batch(parked, [[0x8ff4, 0xf1234567], [0x8ff8, 0xdeadbeef], [0x8ffc, 0x80000000]], 0, 'pending batch commits return/arguments without changing private capture');
  const changedWords = [[0x8ff4, 0xf1234567], [0x8ff8, 0xdeadbeef], [0x8ffc, 0x80000000]];
  for (const [address, value] of changedWords) assert.equal(guestValue(parked, address), value);
  const beforeComplete = arena(parked), completed = copyState(start);
  completed.registers[0] = 0xcafebabe; // explicit scalar host choice, no provider implementation.
  completed.eip = 0x1006;
  assert.equal(parked.api.complete_call(parked.low, parked.high, child.generation, 1, 0xcafebabe), 0);
  completions++;
  assert.deepEqual(readState(parked), completed, 'private captured return/cleanup survives batch RAM/transfer mutation');
  assert.deepEqual(arena(parked).slice(56, 96), exit(3, 0));
  assert.deepEqual(arena(parked).slice(96), beforeComplete.slice(96), 'completion preserves batch descriptors/helper/cancel');
  const resumed = copyState(completed);
  resumed.eip = 0x1007;
  run(parked, child, 2, resumed, exit(3, 1), 'actual caller NOP resumes once after batch and completion', arena(parked).slice(100, 140));
  for (const [address, value] of changedWords) assert.equal(guestValue(parked, address), value, 'completion/caller do not replay stack writes');
}

const reusedAssembly = join(root, 'engine/tests/fixtures/p2-process-call/integer.S');
const provenance = {
  engine_sha256: hash(engineBytes), generated_modules: modules, actual_engine_runs: actualRuns,
  batch_calls: batchCalls, successful_nonempty_batches: nonemptyBatches, successful_empty_batches: emptyBatches,
  rejected_batches: rejectedBatches, current_guard_checks: currentGuards, generated_guard_rejections: guardRejections,
  successful_captures: captures, successful_completions: completions, synthetic_helper_runs: 0,
  generated_set_sha256: hash(generatedHashes.join('\n')),
  source_sha256: hash(readFileSync(join(root, 'engine/tests/fixtures/p2-word-batches/run.mjs'))),
  reused_llvm_fixture: {path: 'engine/tests/fixtures/p2-process-call/integer.S', source_sha256: hash(readFileSync(reusedAssembly)), symbol: 'stdcall_2', caller_hex: Buffer.from(caller).toString('hex'), proof: 'prior R3-299 independent LLVM verification; not reassembled in this test'},
  claim: 'bounded atomic host word publication and existing code/pending/private-frame consequences; no callback lifecycle, guest instruction batch, shared-thread atomicity, provider or performance claim; Memory8 cases prove atomic status, native tests own precise fault/version evidence',
};
writeFileSync(join(outputDir, 'provenance.json'), JSON.stringify(provenance, null, 2));
console.log(JSON.stringify({...provenance, artifacts: outputDir}));
