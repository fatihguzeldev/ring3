import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {execFileSync} from 'node:child_process';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(root, 'expected engine wasm, output directory and repository root');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const save = (name, bytes) => writeFileSync(join(output, name), bytes);
const engineBytes = readFileSync(enginePath), engineModule = new WebAssembly.Module(engineBytes);
assert.deepEqual(WebAssembly.Module.imports(engineModule), []);
const declarations = WebAssembly.Module.exports(engineModule);
assert.equal(declarations.length, 71);
assert.equal(declarations.filter(row => row.kind === 'function').length, 70);
save('engine.wasm', engineBytes);

const CODE = 0x1000, GATE = 0x2000, STACK = 0x8000, TOP = 0x9000, OBS = 0x9000;
const ALLOCATION = 0x10000000, ARENA = 4236, TRANSFER = 140;
const FLAG = 0xcd7, FIRST = 0x13579bdf, SECOND = 0x2468ace0, CANARY = 0xfacecafe;
const program = Uint8Array.from(Buffer.from(
  '6a5ae8f90f0000' +
  '6a04680030000068011000006a00e8f60f0000' +
  '89c7c707df9b57138b1fc78700100000e0ac68248baf0010000068008000006a0057e8df0f0000' +
  '6a04680030000068011000006a00e8bc0f0000' +
  '89c68b168b9e00100000e8cd0f0000' +
  '890500900000891504900000891d08900000eb00', 'hex'));
assert.equal(program.length, 119);
const initialRegisters = [0x11223344, 0x23456789, 0x3456789a, 0x456789ab, TOP, 0x56789abc, 0x6789abcd, 0x789abcde];
const gateSpecs = [[GATE, 0x10002], [GATE + 16, 0x10005], [GATE + 32, 0x10006], [GATE + 48, 0x10001]];
const stages = [
  {start: 0, next: 7, retired: 2, gate: 0, arguments: [0x5a], result: 0x11223344},
  {start: 7, next: 26, retired: 5, gate: 1, arguments: [0, 4097, 0x3000, 4], result: ALLOCATION},
  {start: 26, next: 65, retired: 9, gate: 2, arguments: [ALLOCATION, 0, 0x8000], result: 1},
  {start: 65, next: 84, retired: 5, gate: 1, arguments: [0, 4097, 0x3000, 4], result: ALLOCATION},
  {start: 84, next: 99, retired: 4, gate: 3, arguments: [], result: 0x5a},
];
const apiNames = ['open', 'close', 'arena_ptr', 'map', 'protect', 'upload', 'read32', 'compile_entries', 'compile_resident_entries', 'generation', 'module_ptr', 'module_len', 'store32', 'store_resident32', 'guard', 'guard_resident', 'capture_call', 'capture_resident_call', 'complete_windows_call', 'complete_resident_windows_call'];
const modules = [], rows = [], controls = [], observations = [], pages = [];
let generatedCalls = 0, guestRetired = 0, captures = 0, completions = 0, hostReads = 0;

function record(magic, length, fields, version = 1) {
  const bytes = new Uint8Array(length), view = new DataView(bytes.buffer);
  bytes.set(Buffer.from(magic)); view.setUint16(4, version, true); view.setUint16(6, 1, true); view.setUint32(8, length, true);
  fields.forEach((value, index) => view.setUint32(16 + index * 4, value, true));
  return bytes;
}
function words(fields) {
  const bytes = new Uint8Array(fields.length * 4), view = new DataView(bytes.buffer);
  fields.forEach((value, index) => view.setUint32(index * 4, value, true));
  return bytes;
}
function snapshot(ctx) { return new Uint8Array(ctx.memory.buffer).slice(ctx.base, ctx.base + ARENA); }
function memoryView(ctx) { return new DataView(ctx.memory.buffer); }
function observe(ctx, expected, name) {
  const actual = snapshot(ctx), label = `${ctx.owner}-${name}`;
  assert.deepEqual(actual, expected, `${label}: complete arena`);
  save(`${label}-arena.bin`, actual); save(`${label}-arena-expected.bin`, expected);
  observations.push({owner: ctx.owner, name, actual: `${label}-arena.bin`, expected: `${label}-arena-expected.bin`, sha256: hash(actual)});
}
function request(ctx, bytes) {
  const before = snapshot(ctx), expected = before.slice();
  assert.ok(bytes.length <= 4096); expected.set(bytes, TRANSFER);
  new Uint8Array(ctx.memory.buffer).set(bytes, ctx.base + TRANSFER);
  assert.deepEqual(snapshot(ctx), expected, 'host input writes only Transfer');
}
function sourceMap() {
  const physical = execFileSync('rg', ['--files', 'engine/src'], {cwd: root, encoding: 'utf8'}).trim().split('\n').sort();
  assert.equal(physical.length, 84);
  const paths = ['Cargo.toml', 'Cargo.lock', 'engine/Cargo.toml', ...physical, 'engine/tests/windows_virtual_free_wasm.rs', 'engine/tests/fixtures/p2-virtual-free/run.mjs'];
  assert.equal(paths.length, 89);
  return Object.fromEntries(paths.map(path => {
    const bytes = readFileSync(join(root, path)); return [path, {bytes: bytes.length, sha256: hash(bytes)}];
  }));
}
const sourcesBefore = sourceMap();
save('source-hashes-before.json', JSON.stringify(sourcesBefore, null, 2));
const codePage = new Uint8Array(4096).fill(0xcc); codePage.set(program);
const gatePage = new Uint8Array(4096).fill(0xcc); for (const [address] of gateSpecs) gatePage.set([0x0f, 0x0b], address - GATE);
const stackPage = new Uint8Array(4096), observerPage = new Uint8Array(4096); observerPage.set(words([CANARY]), 16);
for (const [name, bytes] of [['program.bin', program], ['code-page.bin', codePage], ['gate-page.bin', gatePage], ['stack-input.bin', stackPage], ['observer-input.bin', observerPage]]) save(name, bytes);

function upload(ctx, address, bytes) {
  request(ctx, bytes); const before = snapshot(ctx);
  assert.equal(ctx.api.upload(address, bytes.length), 0); assert.deepEqual(snapshot(ctx), before);
}
function fresh(owner, ordinal) {
  const instance = new WebAssembly.Instance(engineModule, {});
  const api = Object.fromEntries(apiNames.map(name => [name, instance.exports[`ring3_abi_v1_${name}`]]));
  for (const [name, fn] of Object.entries(api)) assert.equal(typeof fn, 'function', name);
  const ctx = {owner, api, memory: instance.exports.memory, low: ordinal, high: 0xa3810000, compileOrdinal: 0};
  assert.equal(api.open(6, ctx.low, ctx.high), 0); ctx.base = api.arena_ptr() >>> 0;
  const expected = new Uint8Array(ARENA);
  expected.set(record('R3ST', 56, [...Array(8).fill(0), 0, 2]));
  expected.set(record('R3EX', 40, [1, 0, 0, 0, 0, 0]), 56);
  expected.set(record('R3MH', 40, [0, 0, 0, 0, 0, 0]), 100);
  observe(ctx, expected, 'initial');
  for (const [address, permissions, bytes] of [[CODE, 5, codePage], [GATE, 5, gatePage], [STACK, 3, stackPage], [OBS, 3, observerPage]]) {
    const before = snapshot(ctx); assert.equal(api.map(address, 1, 3), 0); assert.deepEqual(snapshot(ctx), before);
    upload(ctx, address, bytes);
    if (permissions !== 3) {
      const beforeProtect = snapshot(ctx); assert.equal(api.protect(address, 1, permissions), 0); assert.deepEqual(snapshot(ctx), beforeProtect);
    }
  }
  return ctx;
}
function compile(ctx, entries, gates, name, helpers) {
  request(ctx, words([...entries, ...gates.flat()])); const before = snapshot(ctx), expected = before.slice();
  const ordinal = ++ctx.compileOrdinal;
  let unit;
  if (ctx.owner === 'replacement') {
    assert.equal(ctx.api.compile_entries(entries.length, gates.length), 0);
    unit = {generation: ctx.api.generation(), pointer: ctx.api.module_ptr() >>> 0, length: ctx.api.module_len() >>> 0};
    assert.equal(unit.generation, ordinal);
  } else {
    assert.equal(ctx.api.compile_resident_entries(entries.length, gates.length), 0);
    const view = memoryView(ctx), at = ctx.base + TRANSFER;
    assert.equal(view.getUint32(at, true), 1); assert.equal(view.getUint32(at + 4, true), 24);
    unit = {low: view.getUint32(at + 8, true), high: view.getUint32(at + 12, true), pointer: view.getUint32(at + 16, true), length: view.getUint32(at + 20, true)};
    assert.equal(unit.low, ordinal); assert.equal(unit.high, 0);
    expected.set(words([1, 24, unit.low, unit.high, unit.pointer, unit.length]), TRANSFER);
  }
  assert.ok(unit.pointer && unit.length > 8 && unit.pointer + unit.length <= ctx.memory.buffer.byteLength);
  observe(ctx, expected, `compile-${name}`);
  const bytes = new Uint8Array(ctx.memory.buffer).slice(unit.pointer, unit.pointer + unit.length), module = new WebAssembly.Module(bytes);
  const guard = ctx.owner === 'replacement' ? 'guard' : 'guard_resident';
  assert.deepEqual(WebAssembly.Module.imports(module), [{module: 'env', name: 'memory', kind: 'memory'}, ...[guard, ...helpers].map(name => ({module: 'ring3', name, kind: 'function'}))]);
  assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  const generated = new WebAssembly.Instance(module, {env: {memory: ctx.memory}, ring3: ctx.api});
  assert.equal(generated.exports.run.length, 4); unit.run = generated.exports.run;
  const filename = `${ctx.owner}-${name}.wasm`; save(filename, bytes);
  modules.push({owner: ctx.owner, name, filename, sha256: hash(bytes), helpers, generation: unit.generation ?? 0, id: unit.low ?? 0, key_low: ctx.low, key_high: ctx.high, pointer: unit.pointer, length: unit.length});
  return unit;
}
function seed(ctx) {
  const expected = snapshot(ctx); expected.set(record('R3ST', 56, [...initialRegisters, CODE, FLAG]));
  new Uint8Array(ctx.memory.buffer).set(expected.subarray(0, 56), ctx.base);
  observe(ctx, expected, 'only-cpu-seed');
}
function gateRegisters(index) {
  const registers = [...initialRegisters]; registers[4] = TOP - 4 * (stages[index].arguments.length + 1);
  if (index >= 2) { registers[0] = index === 3 ? 1 : ALLOCATION; registers[3] = FIRST; registers[5] = SECOND; registers[7] = ALLOCATION; }
  if (index === 4) { registers[0] = ALLOCATION; registers[2] = 0; registers[3] = 0; registers[6] = ALLOCATION; }
  return registers;
}
function generatedRun(ctx, unit, registers, pc, reason, retired, detail, helper, version, name) {
  const expected = snapshot(ctx); expected.set(record('R3ST', 56, [...registers, pc, FLAG]));
  expected.set(record('R3EX', 40, [reason, retired, detail, 0, 0, 0], version), 56);
  expected.set(record('R3MH', 40, [0, helper, 0, 0, 0, 0]), 100);
  assert.equal(unit.run(ctx.base, ctx.base + 56, 64, ctx.base + 96), 0);
  observe(ctx, expected, name); generatedCalls++; guestRetired += retired;
}
function hostRead(ctx, address, expectedValue) {
  const before = snapshot(ctx), expected = before.slice(); expected.set(record('R3MH', 40, [0, expectedValue, 0, 0, 0, 0]), 100);
  assert.equal(ctx.api.read32(address), 0); assert.deepEqual(snapshot(ctx), expected);
  hostReads++; return memoryView(ctx).getUint32(ctx.base + 120, true);
}
function wholePage(ctx, address, expected, name) {
  const before = snapshot(ctx), actual = new Uint8Array(4096), view = new DataView(actual.buffer);
  const expectedView = new DataView(expected.buffer, expected.byteOffset, expected.byteLength);
  for (let index = 0; index < 1024; index++) view.setUint32(index * 4, hostRead(ctx, address + index * 4, expectedView.getUint32(index * 4, true)), true);
  assert.deepEqual(actual, expected); const after = before.slice(); after.set(record('R3MH', 40, [0, expectedView.getUint32(4092, true), 0, 0, 0, 0]), 100);
  observe(ctx, after, `page-${name}`);
  const label = `${ctx.owner}-${name}`; save(`${label}-page.bin`, actual); save(`${label}-page-expected.bin`, expected);
  pages.push({owner: ctx.owner, name, address, actual: `${label}-page.bin`, expected: `${label}-page-expected.bin`, sha256: hash(actual)});
}
function unmappedDiagnostic(ctx, address, name) {
  const before = snapshot(ctx), expected = before.slice(); expected.set(record('R3MH', 40, [1, 0, 1, address, 1, 4]), 100);
  assert.equal(ctx.api.read32(address), 0); hostReads++; observe(ctx, expected, name);
  assert.deepEqual(snapshot(ctx).subarray(0, 100), before.subarray(0, 100), 'host unmapped diagnostic leaves CPU/Exit/cancel exact');
}
function capture(ctx, unit, index) {
  const stage = stages[index], [pc, id] = gateSpecs[stage.gate], registers = gateRegisters(index);
  hostRead(ctx, registers[4], CODE + stage.next);
  stage.arguments.forEach((value, slot) => hostRead(ctx, registers[4] + 4 + slot * 4, value));
  const expected = snapshot(ctx), token = index + 1;
  expected.set(record('R3CF', 112, [token, id, 2, stage.arguments.length, pc, registers[4], CODE + stage.next, 0, ...stage.arguments, ...Array(16 - stage.arguments.length).fill(0)]), TRANSFER);
  const status = ctx.owner === 'replacement' ? ctx.api.capture_call(ctx.low, ctx.high, unit.generation, 2, stage.arguments.length)
    : ctx.api.capture_resident_call(ctx.low, ctx.high, unit.low, unit.high, 2, stage.arguments.length);
  assert.equal(status, 0); observe(ctx, expected, `capture-${index}`); captures++;
  return token;
}
function complete(ctx, unit, token) {
  return ctx.owner === 'replacement' ? ctx.api.complete_windows_call(ctx.low, ctx.high, unit.generation, token)
    : ctx.api.complete_resident_windows_call(ctx.low, ctx.high, unit.low, unit.high, token);
}
function rejection(ctx, unit, token, status, name) {
  const expected = snapshot(ctx); assert.equal(complete(ctx, unit, token), status);
  observe(ctx, expected, name); controls.push({owner: ctx.owner, name, status});
}
function finish(ctx, unit, token, index) {
  const stage = stages[index], registers = gateRegisters(index); registers[0] = stage.result; registers[4] = TOP;
  const expected = snapshot(ctx); expected.set(record('R3ST', 56, [...registers, CODE + stage.next, FLAG]));
  expected.set(record('R3EX', 40, [3, 0, 0, 0, 0, 0], 3), 56);
  assert.equal(complete(ctx, unit, token), 0); observe(ctx, expected, `complete-${index}`); completions++;
}

for (const [ordinal, owner] of [[1, 'replacement'], [2, 'resident']]) {
  const ctx = fresh(owner, ordinal), store = owner === 'replacement' ? 'store32' : 'store_resident32';
  const callerHelpers = [[store], [store], ['read32', store], [store], ['read32', store], [store]];
  let gateUnit, callers = [];
  if (owner === 'resident') {
    gateUnit = compile(ctx, gateSpecs.map(row => row[0]), gateSpecs, 'gates', []);
    callers = [...stages.map(stage => stage.start), 99].map((start, index) => compile(ctx, [CODE + start], [], `caller-${index}`, callerHelpers[index]));
  } else callers[0] = compile(ctx, [CODE, GATE], [gateSpecs[0]], 'caller-0', callerHelpers[0]);
  seed(ctx);
  for (let index = 0; index < stages.length; index++) {
    const stage = stages[index], gate = gateSpecs[stage.gate];
    const caller = owner === 'resident' || index === 0 ? callers[index]
      : compile(ctx, [CODE + stage.start, gate[0]], [gate], `caller-${index}`, callerHelpers[index]);
    const registers = gateRegisters(index);
    generatedRun(ctx, caller, registers, gate[0], owner === 'replacement' ? 8 : 3, stage.retired, owner === 'replacement' ? gate[1] : 0, 0, owner === 'replacement' ? 3 : 2, `caller-run-${index}`);
    const unit = owner === 'resident' ? gateUnit : caller;
    if (owner === 'resident') generatedRun(ctx, gateUnit, registers, gate[0], 8, 0, gate[1], 0, 3, `gate-run-${index}`);
    const token = capture(ctx, unit, index);
    if (index === 2) {
      const page0 = new Uint8Array(4096), page1 = new Uint8Array(4096); page0.set(words([FIRST])); page1.set(words([SECOND]));
      wholePage(ctx, ALLOCATION, page0, 'written-first'); wholePage(ctx, ALLOCATION + 4096, page1, 'written-second');
      rejection(ctx, unit, token + 1, 14, 'wrong-free-token');
      memoryView(ctx).setUint32(ctx.base + 96, 1, true); rejection(ctx, unit, token, 16, 'cancelled-free');
      memoryView(ctx).setUint32(ctx.base + 96, 0, true);
    }
    finish(ctx, unit, token, index);
    if (index === 1 || index === 3) {
      wholePage(ctx, ALLOCATION, new Uint8Array(4096), `zero-${index}-first`);
      wholePage(ctx, ALLOCATION + 4096, new Uint8Array(4096), `zero-${index}-second`);
    }
    if (index === 2) {
      rejection(ctx, unit, token, 14, 'consumed-free-token');
      unmappedDiagnostic(ctx, ALLOCATION, 'released-first-host-diagnostic');
      unmappedDiagnostic(ctx, ALLOCATION + 4096, 'released-second-host-diagnostic');
    }
  }
  const finalCaller = owner === 'resident' ? callers[5] : compile(ctx, [CODE + 99], [], 'caller-5', callerHelpers[5]);
  const finalRegisters = [0x5a, initialRegisters[1], 0, 0, TOP, SECOND, ALLOCATION, ALLOCATION];
  generatedRun(ctx, finalCaller, finalRegisters, CODE + 119, 3, 4, 0, 0, 2, 'caller-final');
  const finalStack = new Uint8Array(4096); finalStack.set(words([CODE + 84, 0, 4097, 0x3000, CODE + 99]), 4096 - 20);
  const finalObserver = observerPage.slice(); finalObserver.set(words([0x5a, 0, 0]));
  for (const [address, expected, name] of [[CODE, codePage, 'final-code'], [GATE, gatePage, 'final-gates'], [STACK, finalStack, 'final-stack'], [OBS, finalObserver, 'final-observer']]) wholePage(ctx, address, expected, name);
  rows.push({owner, initial_seed_count: 1, initial_registers: initialRegisters, initial_flags: FLAG, allocations: [ALLOCATION, ALLOCATION], allocation_size: 4097, allocation_pages: 2, released_host_faults: 2, genuine_guest_loads: 4, genuine_guest_data_stores: 5, last_error: 0x5a, final_registers: finalRegisters, final_pc: CODE + 119, final_flags: FLAG, guest_retired: 29});
  assert.equal(ctx.api.close(), 0);
  assert.equal(finalCaller.run(0xffffffff, 0xffffffff, 0, 0xffffffff), 5);
  controls.push({owner, name: 'closed-retained-generated-function', status: 5});
}
assert.equal(modules.length, 13); assert.equal(generatedCalls, 17); assert.equal(guestRetired, 58);
assert.equal(captures, 10); assert.equal(completions, 10); assert.equal(controls.length, 8);
assert.equal(observations.length, 84); assert.equal(pages.length, 20); assert.equal(hostReads, 20518);
const sourcesAfter = sourceMap(); assert.deepEqual(sourcesAfter, sourcesBefore);
save('source-hashes-after.json', JSON.stringify(sourcesAfter, null, 2));
const result = {status: 'PASS', engine_sha256: hash(engineBytes), program_sha256: hash(program), sources_before: sourcesBefore, sources_after: sourcesAfter, rows, modules, observations, pages, controls, counts: {contexts: 2, initial_cpu_seeds: 2, generated_calls: 17, guest_retired: 58, captures: 10, completions: 10, modules: 13, resident_peak: 7, controls: 8, whole_arena_observations: 84, arena_files: 168, whole_pages: 20, host_read32: 20518, host_unmapped_faults: 4, source_routes: 89}, claim: 'two authored numeric-Gate owners, one initial CPU seed each; genuine guest PUSH/CALL/allocation/data stores+loads/free/reallocation/zero loads+caller continuation; four released-address faults are host read32 diagnostics, not faulting guest instructions or guest fault recovery; complete saved arena/page pairs backed by live literal assertions; no named VirtualFree PE profile, general Windows allocator, callback-release matrix, SDK/browser/performance/game claim'};
save('result.json', JSON.stringify(result, null, 2));
console.log(JSON.stringify({status: result.status, engine_sha256: result.engine_sha256, counts: result.counts, output}));
