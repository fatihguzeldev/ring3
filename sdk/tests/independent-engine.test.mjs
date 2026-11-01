import assert from 'node:assert/strict';
import test from 'node:test';
import {existsSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import {readEngine} from '../../engine/tests/support/wasm/engine.mjs';
import {Engine, KEY, record, view} from '../runtime/engine.js';
import {IndependentCodeCache} from '../runtime/independent.js';
import {callbackRecord} from '../runtime/callback.js';
import {builder} from '../../engine/tests/support/pe32/image.mjs';
import {configureFiles} from '../../engine/tests/windows/files/fixtures/runtime/setup.mjs';

const explicitPath = process.env.ENGINE_WASM ?? process.env.RING3_ENGINE_WASM;
const path = explicitPath ?? fileURLToPath(new URL('../../target/wasm32-unknown-unknown/release/ring3_engine.wasm', import.meta.url));
let artifact, skip;
if (explicitPath || existsSync(path)) {
  artifact = readEngine(path);
  const capable = WebAssembly.Module.exports(artifact.module).some(item => item.name === 'ring3_abi_v1_cpu_abi');
  if (explicitPath) assert(capable, 'the requested actual engine must provide the independent CPU');
  else if (!capable) skip = 'build the independent engine or set ENGINE_WASM';
} else skip = 'build the independent engine or set ENGINE_WASM';

function fixture(code = [0xeb, 0xfe], {pc = 0x1000, execution, data = [], dataPage = 0x2000,
  registers = [0, 0, 0, 0, 0x2f00, 0, 0, 0], fpControl, fpStatus,
  windowsMappings = false, moduleImage, physicalPages = 16, independent = true} = {}) {
  const instance = new WebAssembly.Instance(artifact.module, {ring3_observability: {now_ns: () => 0n}});
  const api = Object.fromEntries(Object.entries(instance.exports)
    .filter(([name]) => name.startsWith('ring3_abi_v1_')).map(([name, value]) => [name.slice(13), value]));
  const engine = new Engine(api, instance.exports.memory, artifact.sha256);
  engine.ok('open', physicalPages, ...KEY);
  engine.base = api.arena_ptr() >>> 0;
  if (windowsMappings) {
    engine.ok('configure_cpu_clock', 1_000_000_000, 0, 0, 0);
    engine.ok('configure_synchronization', 128, 256, 256, 16);
    engine.ok('configure_module_limits', 8, 0x100000, 16, 65536, 0x100000);
    engine.ok('register_kernel32_module', 0x70000000);
    configureFiles({api, request: bytes => engine.request(bytes), take: length => Buffer.from(engine.take(length))});
    const image = moduleImage ?? builder([['kernel32.dll', ['CreateFileMappingA', 'MapViewOfFile']]]).seal();
    engine.ok('begin_module_input', image.bytes.length, engine.name('memory-fixture.exe'));
    for (let offset = 0; offset < image.bytes.length; offset += 4096) {
      const chunk = image.bytes.subarray(offset, offset + 4096);
      engine.request(chunk); engine.ok('append_module_input', offset, chunk.length);
    }
    engine.ok('commit_module_input');
    engine.ok('start_module_process', engine.name('memory-fixture.exe'), 0, 0x7e000000, 4, 0x7f000000);
  }
  engine.ok('map', 0x1000, 1, 7);
  engine.request(Uint8Array.from(code)); engine.ok('upload', 0x1000, code.length);
  engine.ok('protect', 0x1000, 1, 5);
  engine.ok('map', dataPage, 1, 3);
  for (const [address, value] of data) engine.ok('write8', address, value);
  const scalar = record('R3ST', 56), d = view(scalar);
  registers.forEach((value, index) => d.setUint32(16 + index * 4, value, true));
  d.setUint32(48, pc, true); d.setUint32(52, 2, true);
  new Uint8Array(engine.memory.buffer, engine.base, scalar.length).set(scalar);
  const arena = new DataView(engine.memory.buffer);
  if (fpControl !== undefined) arena.setUint16(engine.base + 4236 + 16, fpControl, true);
  if (fpStatus !== undefined) arena.setUint16(engine.base + 4236 + 18, fpStatus, true);
  const cache = independent ? new IndependentCodeCache(engine, execution) : null;
  let closed = false;
  return {engine, cache, exports: instance.exports,
    slab: () => new DataView(engine.memory.buffer, api.cpu_context_ptr() >>> 0, 160),
    close() {
      if (closed) return;
      if (independent) assert.equal(api.core_context_ptr(), 0, 'independent execution does not create a legacy core session');
      cache?.close(); engine.ok('close'); closed = true;
    }};
}

function event(engine, expectedVector, expectedPc, expectedRetired) {
  engine.ok('take_cpu_event');
  const packet = engine.resultView(48);
  assert.deepEqual([...new Uint8Array(packet.buffer, packet.byteOffset, 4)], [0x52, 0x33, 0x43, 0x45]);
  assert.equal(packet.getUint32(8, true), 48);
  assert.equal(packet.getUint32(16, true), expectedVector);
  assert.equal(packet.getUint32(24, true), expectedPc);
  assert.equal(packet.getUint32(44, true), expectedRetired);
  assert.equal(engine.status('take_cpu_event'), 7);
}

function memoryCalls(f) {
  const calls = {read: 0, write: 0};
  for (const kind of ['read', 'write']) {
    const name = `cpu_runtime_${kind}`, native = f.engine.api[name];
    f.engine.api[name] = (...args) => {calls[kind]++; return native(...args);};
  }
  return calls;
}

function serviceCounts(f, expected) {
  const api = f.engine.api;
  const dispatcher = new WebAssembly.Module(new Uint8Array(f.engine.memory.buffer,
    api.cpu_dispatch_code_ptr() >>> 0, api.cpu_dispatch_code_len() >>> 0));
  const direct = WebAssembly.Module.imports(dispatcher).some(item => item.module === 'cpu' && item.name === 'instruction');
  const stats = f.cache.stats();
  assert.deepEqual([stats.instructionCalls, stats.jsInstructionCalls, stats.directInstructionCalls,
    stats.instructionCountersAvailable], [expected, direct ? 0 : expected, direct ? expected : 0, true]);
}

function rejectJsService(f) {
  if (f.cache.directService) f.engine.api.cpu_instruction = () => {throw new Error('the actual direct dispatcher must not call the JS instruction boundary');};
}

function sharedViews(f, protection, accesses) {
  const {engine, cache} = f, gates = new Map();
  cache.import(); cache.publish();
  for (let start = 0, total = 1; start < total;) {
    engine.ok('module_gates_page', start, 128);
    const page = engine.resultView(4096), count = page.getUint32(24, true);
    total = page.getUint32(16, true); assert(count > 0);
    for (let index = 0; index < count; index++)
      gates.set(page.getUint32(36 + index * 8, true), page.getUint32(32 + index * 8, true));
    start += count;
  }
  const initial = new Uint8Array(engine.memory.buffer, engine.base, 56).slice();
  const call = (id, args) => {
    const gate = gates.get(id); assert(gate, 'the setup provider uses its real loaded module gate');
    for (const [index, value] of [0x1000, ...args].entries()) engine.ok('write32', 0x2f00 + index * 4, value);
    const data = new DataView(engine.memory.buffer);
    data.setUint32(engine.base + 48, gate, true); data.setUint32(engine.base + 32, 0x2f00, true);
    const exit = record('R3EX', 40), e = view(exit);
    e.setUint16(4, 3, true); e.setUint32(16, 8, true); e.setUint32(24, id, true);
    new Uint8Array(engine.memory.buffer, engine.base + 56, 40).set(exit);
    engine.ok('cpu_capture_windows_call'); const token = engine.resultView(112).getUint32(16, true);
    engine.ok('cpu_complete_windows_call', token);
    const result = engine.get(16);
    new Uint8Array(engine.memory.buffer, engine.base, 56).set(initial);
    assert(result !== 0 && result !== 0xffffffff, 'the actual file-mapping provider completed setup');
    return result;
  };
  const section = call(0xa0080, [0xffffffff, 0, protection, 0, 4096, 0]);
  return accesses.map(access => call(0xa0084, [section, access, 0, 0, 4096]));
}

const dword = value => [value & 255, value >>> 8 & 255, value >>> 16 & 255, value >>> 24];
function replaceCode(f, code) {
  f.engine.ok('protect', 0x1000, 1, 7);
  f.engine.request(Uint8Array.from(code)); f.engine.ok('upload', 0x1000, code.length);
  f.engine.ok('protect', 0x1000, 1, 5);
}

for (const c of [
  {name: 'faulting RMW', code: [0x83, 0xc0, 1, 0x41, 0x83, 0x05, ...dword(0x9000), 1],
    count: 3, retired: 2, reason: 5, pc: 0x1004, flags: 0x897, services: 0},
  {name: 'numeric service', code: [0x83, 0xc0, 1, 0xdb, 0xe2, 0x41],
    count: 3, retired: 3, reason: 1, pc: 0x1006, flags: 0x897, services: 1},
  {name: 'completed SMC', code: [0x83, 0xc0, 1, 0x80, 0x05, ...dword(0x1000), 1],
    count: 2, retired: 2, reason: 6, pc: 0x100a, flags: 0x86, services: 0, smc: true},
]) test(`actual arithmetic flags survive ${c.name} with full architectural publication`, {skip}, () => {
  const registers = [0xffffffff, 0x7fffffff, 0xa1b2c3d4, 0x55667788, 0x2f00, 0x13579bdf, 0x2468ace0, 0x0f1e2d3c];
  const f = fixture(c.code, {registers, execution: {budget: c.count}});
  try {
    if (c.smc) f.engine.ok('protect', 0x1000, 1, 7);
    const scalar = new Uint8Array(f.engine.memory.buffer, f.engine.base, 56).slice(), banks = processBanks(f);
    f.cache.import(); let selected = f.cache.select(); const backing = f.slab().getUint32(84, true);
    const ram = new Uint8Array(f.engine.memory.buffer, backing, 65536).slice();
    view(scalar).setUint32(16, 0, true);
    if (!c.smc) view(scalar).setUint32(20, 0x80000000, true);
    view(scalar).setUint32(48, c.pc, true); view(scalar).setUint32(52, c.flags, true);
    if (c.smc) ram[0]++;
    let run, retired = 0, work = 0;
    for (let count = 0; count < 3; count++) {
      run = f.cache.execute(selected); retired += run.retired; work += run.work;
      if (run.coreReason !== 1) break;
      selected = f.cache.select();
    }
    assert.deepEqual([run.reason, retired, work, run.pc], [c.reason, c.retired, c.retired, c.pc]);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, f.engine.base, 56), scalar);
    assert.deepEqual(processBanks(f), banks);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, backing, 65536), ram);
    serviceCounts(f, c.services);
    if (c.reason === 5) event(f.engine, 0xffffffff, c.pc, c.retired);
  } finally {f.close();}
});

test('actual COW view uses the native split and refreshes the canonical mapping before later loads', {skip}, () => {
  const f = fixture(undefined, {windowsMappings: true, physicalPages: 1024, execution: {budget: 4}});
  try {
    const [shared, copy] = sharedViews(f, 4, [2, 1]);
    f.engine.ok('write32', shared, 0x11223344);
    replaceCode(f, [0xb8, 0x88, 0x77, 0x66, 0x55, 0xa3, ...dword(copy),
      0x8b, 0x1d, ...dword(shared), 0x8b, 0x0d, ...dword(copy)]);
    const calls = memoryCalls(f);
    f.cache.import(); const selected = f.cache.select();
    const state = f.slab(), backing = state.getUint32(84, true), length = state.getUint32(88, true) * 4096;
    const mappings = state.getUint32(76, true), before = new Uint8Array(f.engine.memory.buffer, backing, length).slice();
    const slot = address => new DataView(f.engine.memory.buffer).getUint32(mappings + (address >>> 12) * 4, true) - 1;
    const originalSlot = slot(shared); assert.equal(slot(copy), originalSlot);
    assert(new DataView(f.engine.memory.buffer).getUint32(state.getUint32(96, true) + (copy >>> 12) * 4, true) !== 0);
    const run = f.cache.execute(selected);
    assert.deepEqual([run.reason, run.retired, run.work, run.pc], [1, 4, 4, 0x1016]);
    assert.deepEqual(Array.from({length: 8}, (_, index) => f.engine.get(16 + index * 4)),
      [0x55667788, 0x55667788, 0, 0x11223344, 0x2f00, 0, 0, 0]);
    assert.equal(f.engine.get(52), 2);
    assert.equal(slot(shared), originalSlot); assert.notEqual(slot(copy), originalSlot);
    const copied = new Uint8Array(4096); new DataView(copied.buffer).setUint32(0, 0x55667788, true);
    before.set(copied, slot(copy) * 4096);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, backing, length), before);
    assert(calls.write > 0, 'COW stores retain the actual canonical native split');
  } finally {f.close();}
});

test('actual non-executable alias write commits once, exits before the next instruction and revokes stale code', {skip}, () => {
  const f = fixture(undefined, {windowsMappings: true, physicalPages: 1024, execution: {budget: 2}});
  try {
    const [writable, executable] = sharedViews(f, 0x40, [2, 0x24]);
    f.engine.request(Uint8Array.from([0xb8, 1, 0, 0, 0, 0xeb, 0xf9]));
    f.engine.ok('upload', writable, 7);
    replaceCode(f, [0xc7, 0x05, ...dword(writable + 1), 2, 0, 0, 0, 0x40]);
    new DataView(f.engine.memory.buffer).setUint32(f.engine.base + 48, executable, true);
    f.cache.import(); const old = f.cache.select();
    assert.equal(f.cache.execute(old).reason, 1); assert.equal(f.engine.get(16), 1);
    const calls = memoryCalls(f);
    new DataView(f.engine.memory.buffer).setUint32(f.engine.base + 48, 0x1000, true);
    f.cache.budget = 3; f.cache.import(); const writer = f.cache.select();
    const state = f.slab(), backing = state.getUint32(84, true), length = state.getUint32(88, true) * 4096;
    const all = new DataView(f.engine.memory.buffer), mappings = state.getUint32(76, true);
    const slot = all.getUint32(mappings + (writable >>> 12) * 4, true) - 1;
    assert.equal(all.getUint32(mappings + (executable >>> 12) * 4, true) - 1, slot);
    const expected = new Uint8Array(f.engine.memory.buffer, backing, length).slice();
    const run = f.cache.execute(writer);
    assert.deepEqual([run.coreReason, run.reason, run.retired, run.work, run.pc], [6, 6, 1, 1, 0x100a]);
    assert.deepEqual(Array.from({length: 8}, (_, index) => f.engine.get(16 + index * 4)),
      [1, 0, 0, 0, 0x2f00, 0, 0, 0], 'the following INC was not executed');
    assert.equal(f.engine.get(52), 2);
    new DataView(expected.buffer).setUint32(slot * 4096 + 1, 2, true);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, backing, length), expected);
    assert(calls.write > 0, 'executable aliases retain immediate native invalidation');
    new DataView(f.engine.memory.buffer).setUint32(f.engine.base + 48, executable, true);
    f.cache.budget = 2; f.cache.import();
    assert.equal(f.cache.table.get(old.slot), null);
    const replacement = f.cache.select(); assert(replacement.slot > old.slot);
    assert.equal(f.cache.execute(replacement).reason, 1);
    assert.deepEqual(Array.from({length: 8}, (_, index) => f.engine.get(16 + index * 4)), [2, 0, 0, 0, 0x2f00, 0, 0, 0]);
    assert.equal(f.engine.get(52), 2);
  } finally {f.close();}
});

test('actual ordinary load RMW and stack operations preserve complete state without native memory helpers', {skip}, () => {
  const code = [0xa1, 0, 0x20, 0, 0, 0x01, 0x05, 4, 0x20, 0, 0, 0x50, 0x5b];
  const f = fixture(code, {execution: {budget: 4}, data: [[0x2000, 0x78], [0x2001, 0x56], [0x2002, 0x34], [0x2003, 0x12], [0x2004, 8]]});
  const calls = memoryCalls(f);
  try {
    const banks = [[4236, 352], [4612, 184]].map(([offset, length]) =>
      new Uint8Array(f.engine.memory.buffer, f.engine.base + offset, length).slice());
    f.cache.import();
    const selected = f.cache.select();
    calls.read = 0; calls.write = 0;
    const run = f.cache.execute(selected);
    assert.deepEqual([run.reason, run.retired, run.work, run.pc], [1, 4, 4, 0x100d]);
    assert.deepEqual(Array.from({length: 8}, (_, index) => f.engine.get(16 + index * 4)),
      [0x12345678, 0, 0, 0x12345678, 0x2f00, 0, 0, 0]);
    assert.equal(f.engine.get(52), 0x12);
    const expected = new Uint8Array(65536); expected.set(code);
    const data = new DataView(expected.buffer);
    data.setUint32(4096, 0x12345678, true); data.setUint32(4100, 0x12345680, true);
    data.setUint32(4096 + 0xefc, 0x12345678, true);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, f.slab().getUint32(84, true), 65536), expected);
    assert.deepEqual([[4236, 352], [4612, 184]].map(([offset, length]) =>
      new Uint8Array(f.engine.memory.buffer, f.engine.base + offset, length)), banks);
    assert.deepEqual(calls, {read: 0, write: 0});
  } finally {f.close();}
});

test('actual reached BTS stack operand updates canonical RAM through the inline dirty path', {skip}, () => {
  const registers = [31, 0x11223344, 0x22334455, 0x33445566, 0x2f00, 0x44556677, 0x55667788, 0x66778899];
  const f = fixture([0x0f, 0xab, 0x04, 0x24], {registers, data: [[0x2f00, 1]], execution: {budget: 1}});
  const calls = memoryCalls(f);
  try {
    new DataView(f.engine.memory.buffer).setUint32(f.engine.base + 52, 0xcd7, true);
    const scalar = new Uint8Array(f.engine.memory.buffer, f.engine.base, 56).slice();
    view(scalar).setUint32(48, 0x1004, true); view(scalar).setUint32(52, 0xcd6, true);
    const banks = [[4236, 352], [4612, 184]].map(([offset, length]) =>
      new Uint8Array(f.engine.memory.buffer, f.engine.base + offset, length).slice());
    f.cache.import(); const selected = f.cache.select(), state = f.slab();
    const backing = state.getUint32(84, true), length = state.getUint32(88, true) * 4096;
    const expected = new Uint8Array(f.engine.memory.buffer, backing, length).slice();
    const slot = new DataView(f.engine.memory.buffer).getUint32(state.getUint32(76, true) + 2 * 4, true) - 1;
    new DataView(expected.buffer).setUint32(slot * 4096 + 0xf00, 0x80000001, true);
    calls.read = 0; calls.write = 0;
    const run = f.cache.execute(selected);
    assert.deepEqual([run.reason, run.retired, run.work, run.pc], [1, 1, 1, 0x1004]);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, f.engine.base, 56), scalar);
    assert.deepEqual([[4236, 352], [4612, 184]].map(([offset, size]) =>
      new Uint8Array(f.engine.memory.buffer, f.engine.base + offset, size)), banks);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, backing, length), expected);
    assert.deepEqual(calls, {read: 0, write: 0});
    assert.equal(f.slab().getUint32(48, true), 0);
    assert.equal(f.slab().getUint32(108, true), 0);
  } finally {f.close();}
});

test('actual read-only RMW preserves full state and RAM while retaining the native permission fault', {skip}, () => {
  const code = [0x01, 0x05, 0, 0x20, 0, 0];
  const registers = [5, 0, 0, 0, 0x2f00, 0, 0, 0];
  const f = fixture(code, {registers, data: [[0x2000, 7]], execution: {budget: 1}});
  const calls = memoryCalls(f);
  try {
    f.engine.ok('protect', 0x2000, 1, 1);
    f.cache.import(); const selected = f.cache.select();
    const run = f.cache.execute(selected);
    assert.deepEqual([run.reason, run.retired, run.work, run.pc], [5, 0, 0, 0x1000]);
    assert.deepEqual(registers.map((_, index) => f.engine.get(16 + index * 4)), registers);
    assert.equal(f.engine.get(52), 2);
    assert.deepEqual([f.engine.get(80), f.engine.get(84), f.engine.get(88), f.engine.get(92)], [2, 0x2000, 2, 4]);
    const expected = new Uint8Array(65536); expected.set(code); expected[4096] = 7;
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, f.slab().getUint32(84, true), 65536), expected);
    assert(calls.read > 0, 'read-modify-write permissions are checked by the real native slow path');
    assert.equal(calls.write, 0);
    event(f.engine, 0xffffffff, 0x1000, 0);
  } finally {f.close();}
});

test('actual canonical version exhaustion rejects a fast store before RAM or retirement changes', {skip}, () => {
  const registers = [0x11223344, 0, 0, 0, 0x2f00, 0, 0, 0];
  const f = fixture([0xa3, 0, 0x20, 0, 0], {registers, execution: {budget: 1}});
  const calls = memoryCalls(f);
  let versionPointer = 0, savedVersion;
  try {
    f.cache.import(); const selected = f.cache.select();
    const state = f.slab();
    versionPointer = state.getUint32(156, true);
    assert(versionPointer > 0 && versionPointer % 8 === 0, 'the actual CPU descriptor exposes canonical version authority');
    const memory = new DataView(f.engine.memory.buffer);
    savedVersion = memory.getBigUint64(versionPointer, true);
    const backing = state.getUint32(84, true), before = new Uint8Array(f.engine.memory.buffer, backing, 65536).slice();
    memory.setBigUint64(versionPointer, 0xffffffffffffffffn, true);
    assert.throws(() => f.cache.execute(selected), /independent CPU stop 8/);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, backing, 65536), before);
    assert.deepEqual(registers.map((_, index) => f.slab().getUint32(8 + index * 4, true)), registers);
    assert.deepEqual([f.slab().getUint32(40, true), f.slab().getUint32(48, true), f.slab().getUint32(56, true)], [0x1000, 1, 0]);
    assert.equal(f.slab().getUint32(108, true), 0, 'no uncompleted store enters the dirty batch');
    assert(calls.write > 0, 'exhausted version authority retains the native failure path');
    assert.throws(() => f.cache.import(), /independent CPU stop 8/);
  } finally {
    if (versionPointer && savedVersion !== undefined)
      new DataView(f.engine.memory.buffer).setBigUint64(versionPointer, savedVersion, true);
    f.close();
  }
});

test('actual full dirty queue private-protocol negative rejects an unrecordable store before effects', {skip}, () => {
  const registers = [0x11223344, 0, 0, 0, 0x2f00, 0, 0, 0];
  const f = fixture([0xa3, 0, 0x30, 0, 0], {registers, execution: {budget: 1}});
  const calls = memoryCalls(f);
  try {
    f.engine.ok('map', 0x3000, 1, 3);
    f.cache.import(); const selected = f.cache.select();
    const state = f.slab(), memory = new DataView(f.engine.memory.buffer);
    const mappings = state.getUint32(76, true), listed = memory.getUint32(mappings + 2 * 4, true) - 1;
    const target = memory.getUint32(mappings + 3 * 4, true) - 1;
    assert.notEqual(listed, target);
    const capacity = state.getUint32(112, true), slots = state.getUint32(104, true), bits = state.getUint32(100, true);
    for (let index = 0; index < capacity; index++) memory.setUint32(slots + index * 4, listed, true);
    memory.setUint8(bits + listed, 1); memory.setUint8(bits + target, 0);
    state.setUint32(108, capacity, true);
    const backing = state.getUint32(84, true), before = new Uint8Array(f.engine.memory.buffer, backing, 65536).slice();
    assert.throws(() => f.cache.execute(selected), /independent CPU stop 8/);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, backing, 65536), before);
    assert.deepEqual(registers.map((_, index) => f.slab().getUint32(8 + index * 4, true)), registers);
    assert.deepEqual([f.slab().getUint32(40, true), f.slab().getUint32(48, true), f.slab().getUint32(56, true)], [0x1000, 1, 0]);
    assert(f.slab().getUint32(108, true) <= capacity, 'the rejected append never exceeds its owned allocation');
    assert(calls.write > 0, 'the malformed private queue is rejected by the bounded native path');
    assert.throws(() => f.cache.import(), /independent CPU stop 8/);
  } finally {f.close();}
});

test('actual independent owner rejects foreign release and closes after a malformed slab', {skip}, () => {
  const f = fixture();
  try {
    f.engine.ok('cpu_begin', ...KEY);
    assert.equal(f.engine.status('write32', 0x2000, 17), 12);
    assert.equal(f.engine.status('close'), 12);
    assert.equal(f.engine.status('cpu_end', 0, 0), 3);
    f.slab().setUint32(0, 99, true);
    f.engine.ok('cpu_end', ...KEY);
    f.close();
  } finally {f.close();}
});

test('actual dispatcher inspection cannot attach an independent owner during an active legacy core lease', {skip}, () => {
  const f = fixture(undefined, {independent: false});
  let active = false;
  try {
    f.engine.ok('core_prepare', 0x1000, 1, 1);
    assert(f.engine.api.core_recovery_scratch_ptr() > 0);
    f.engine.ok('core_begin'); active = true;
    const before = new Uint8Array(f.engine.memory.buffer, f.engine.base, 56).slice();
    assert.equal(f.engine.api.cpu_dispatch_code_ptr(), 0);
    assert.equal(f.engine.api.cpu_context_ptr(), 0);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, f.engine.base, 56), before);
    f.engine.ok('core_end'); active = false;
    f.close();
  } finally {
    if (active) f.engine.api.core_end();
    f.engine.api.close();
  }
});

test('actual independent cache rejects lazy legacy owners and still resumes its own compiled loop', {skip}, () => {
  const f = fixture([0x43, 0xeb, 0xfd], {execution: {budget: 2}});
  try {
    const before = new Uint8Array(f.engine.memory.buffer, f.engine.base, 56).slice();
    assert.equal(f.engine.api.core_recovery_scratch_ptr(), 0);
    assert.equal(f.engine.api.core_dispatch_code_ptr(), 0);
    assert.equal(f.engine.api.core_code_ptr(), 0);
    assert.equal(f.engine.status('core_prepare', 0x1000, 2, 1), 12);
    assert.equal(f.engine.status('core_prepare_executable', 0x1000, 2, 1), 12);
    assert.equal(f.engine.api.core_cold_run(1), -12);
    assert.equal(f.engine.api.core_context_ptr(), 0);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, f.engine.base, 56), before);
    f.cache.import(); const selected = f.cache.select(), state = f.slab();
    const backing = state.getUint32(84, true), length = state.getUint32(88, true) * 4096;
    const expected = new Uint8Array(length); expected.set([0x43, 0xeb, 0xfd]);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, backing, length), expected);
    const run = f.cache.execute(selected);
    assert.deepEqual([run.reason, run.retired, run.work, run.pc], [1, 2, 2, 0x1000]);
    assert.deepEqual(Array.from({length: 8}, (_, index) => f.engine.get(16 + index * 4)),
      [0, 0, 0, 1, 0x2f00, 0, 0, 0]);
    assert.equal(f.engine.get(52), 2);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, backing, length), expected);
  } finally {f.close();}
});

const externalSkip = skip || (!WebAssembly.Module.exports(artifact.module)
  .some(item => item.name === 'ring3_abi_v1_component_memory_lease') && 'artifact lacks the external-cpu feature');

test('actual external memory lease excludes independent admission without state or RAM effects', {skip: externalSkip}, () => {
  const f = fixture(undefined, {independent: false});
  f.engine.ok('component_memory_lease', ...KEY);
  const descriptor = f.engine.resultView(64), pointer = descriptor.getUint32(16, true), length = descriptor.getUint32(20, true);
  const scalar = new Uint8Array(f.engine.memory.buffer, f.engine.base, 56).slice();
  const ram = new Uint8Array(f.engine.memory.buffer, pointer, length).slice();
  try {
    assert.equal(f.engine.status('cpu_begin', ...KEY), 12);
    assert.equal(f.engine.api.cpu_context_ptr(), 0);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, f.engine.base, 56), scalar);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, pointer, length), ram);
    assert.equal(f.engine.status('close'), 12, 'the original external lease remains pinned');
  } finally {
    if (f.engine.api.cpu_context_ptr()) f.engine.api.cpu_end(...KEY);
  }
});

test('actual attached external CPU excludes independent admission and remains releasable', {skip: externalSkip}, t => {
  const f = fixture(undefined, {independent: false});
  if (!(f.exports.__heap_base instanceof WebAssembly.Global) || f.exports.__heap_base.value < 512 * 1024 * 1024) {
    f.close(); t.skip('component attach requires the external-cpu high-memory artifact'); return;
  }
  let attached = false;
  try {
    f.engine.ok('component_cpu_view', ...KEY);
    const descriptor = f.engine.resultView(128), pointer = 16 * 1024 * 1024, length = descriptor.getUint32(20, true);
    f.engine.ok('component_cpu_attach', ...KEY, pointer, length); attached = true;
    const scalar = new Uint8Array(f.engine.memory.buffer, f.engine.base, 56).slice();
    const ram = new Uint8Array(f.engine.memory.buffer, pointer, length).slice();
    assert.equal(f.engine.status('cpu_begin', ...KEY), 12);
    assert.equal(f.engine.api.cpu_context_ptr(), 0);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, f.engine.base, 56), scalar);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, pointer, length), ram);
    f.engine.ok('component_cpu_release', ...KEY); attached = false;
    f.close();
  } finally {
    if (f.engine.api.cpu_context_ptr()) f.engine.api.cpu_end(...KEY);
    if (attached) f.engine.api.component_cpu_release(...KEY);
    f.engine.api.close();
  }
});

test('actual rejected read records infrastructure failure in its owning slab', {skip}, () => {
  const f = fixture();
  try {
    f.engine.ok('cpu_begin', ...KEY);
    const pointer = f.engine.api.cpu_context_ptr() >>> 0;
    f.engine.api.cpu_runtime_read(pointer + 8, 0x2000, 4);
    assert.equal(f.slab().getUint32(52, true), 8);
    f.engine.ok('cpu_end', ...KEY);
  } finally {f.close();}
});

test('actual compiler and SDK publish an ordinary dirty batch at the yield boundary', {skip}, () => {
  const f = fixture([0xb8, 0x78, 0x56, 0x34, 0x12, 0xa3, 0x00, 0x20, 0x00, 0x00, 0xeb, 0xfe],
    {execution: {budget: 5, instructions: 2}});
  try {
    f.cache.import();
    const run = f.cache.execute(f.cache.select());
    assert.deepEqual([run.coreReason, run.retired, run.work, run.pc], [1, 2, 2, 0x100a]);
    const state = f.slab(), epoch = state.getBigUint64(128, true);
    assert.equal(state.getUint32(108, true), 1);
    const all = new DataView(f.engine.memory.buffer);
    const slot = all.getUint32(state.getUint32(76, true) + 2 * 4, true) - 1;
    assert.equal(all.getUint32(state.getUint32(84, true) + slot * 4096, true), 0x12345678);
    f.cache.publish();
    assert.equal(f.slab().getUint32(108, true), 0);
    assert.equal(f.engine.get(48), 0x100a);
    assert.equal(f.engine.get(76), 2);
    f.engine.ok('read32', 0x2000); assert.equal(f.engine.get(120), 0x12345678);
    f.cache.import();
    assert(f.slab().getBigUint64(128, true) > epoch);
  } finally {f.close();}
});

test('actual compiled late-page store fault preserves its completed prefix and untouched RAM', {skip}, () => {
  const f = fixture([0xb8, 0x78, 0x56, 0x34, 0x12, 0xa3, 0xfe, 0x2f, 0x00, 0x00],
    {data: [[0x2ffe, 0x5a], [0x2fff, 0xa5]]});
  const calls = memoryCalls(f);
  try {
    f.cache.import();
    const run = f.cache.execute(f.cache.select());
    assert.deepEqual([run.reason, run.retired, run.work, run.pc], [5, 1, 1, 0x1005]);
    assert.equal(f.engine.get(16), 0x12345678);
    assert.deepEqual([f.engine.get(80), f.engine.get(84), f.engine.get(88), f.engine.get(92)], [1, 0x3000, 2, 4]);
    f.engine.ok('read16', 0x2ffe); assert.equal(f.engine.get(120), 0xa55a);
    event(f.engine, 0xffffffff, 0x1005, 1);
    assert(calls.write > 0, 'cross-page writes retain the real native slow path');
  } finally {f.close();}
});

test('actual strict numeric fault publishes its event before touching an unmapped operand', {skip}, () => {
  const f = fixture([0xd9, 0x05, 0x00, 0x40, 0x00, 0x00], {fpControl: 0x037e, fpStatus: 1});
  rejectJsService(f);
  try {
    const scalar = new Uint8Array(f.engine.memory.buffer, f.engine.base, 56).slice(), banks = processBanks(f);
    view(banks[0]).setUint16(18, 0x8081, true);
    f.cache.import();
    const selected = f.cache.select(), state = f.slab(), backing = state.getUint32(84, true), length = state.getUint32(88, true) * 4096;
    const ram = new Uint8Array(f.engine.memory.buffer, backing, length).slice(), run = f.cache.execute(selected);
    assert.deepEqual([run.reason, run.retired, run.work, run.pc], [12, 0, 0, 0x1000]);
    assert.equal(f.engine.get(80), 0x8081);
    assert.equal(new DataView(f.engine.memory.buffer).getUint16(f.engine.base + 4236 + 18, true), 0x8081);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, f.engine.base, 56), scalar);
    assert.deepEqual(processBanks(f), banks);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, backing, length), ram);
    serviceCounts(f, 1);
    event(f.engine, 16, 0x1000, 0);
  } finally {f.close();}
});

test('actual inline FLD1 reaches an exact budget boundary without executing the following integer instruction', {skip}, () => {
  const registers = [0x12345678, 0x89abcdef, 0xd5e6789a, 0x23456789, 0x2f00, 0x55443322, 0x66554433, 0x77665544];
  const f = fixture([0xd9, 0xe8, 0x43], {registers, execution: {budget: 1}});
  rejectJsService(f);
  try {
    const scalar = new Uint8Array(f.engine.memory.buffer, f.engine.base, 56).slice(), banks = processBanks(f);
    view(scalar).setUint32(48, 0x1002, true);
    const fp = view(banks[0]);
    fp.setUint16(18, 0x3800, true); fp.setUint16(20, 0x3fff, true); fp.setUint16(22, 0x1e8, true);
    fp.setUint32(24, 0x1000, true); fp.setUint16(32, 0x1b, true);
    banks[0].set([0, 0, 0, 0, 0, 0, 0, 0x80, 0xff, 0x3f], 110);
    const selected = f.cache.select(), state = f.slab(), backing = state.getUint32(84, true), length = state.getUint32(88, true) * 4096;
    const ram = new Uint8Array(f.engine.memory.buffer, backing, length).slice(), run = f.cache.execute(selected);
    assert.deepEqual([run.reason, run.retired, run.work, run.pc], [1, 1, 1, 0x1002]);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, f.engine.base, 56), scalar);
    assert.deepEqual(processBanks(f), banks);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, backing, length), ram);
    assert.equal(f.slab().getUint32(48, true), 0);
    serviceCounts(f, 0);
  } finally {f.close();}
});

test('actual inline x87 transport resumes across compiled selections and publishes the literal floating store', {skip}, () => {
  const registers = [0x12345678, 0x89abcdef, 0xd5e6789a, 0x23456789, 0x2f00, 0x55443322, 0x66554433, 0x77665544];
  const f = fixture([0xd9, 0xe8, 0xd9, 0x1d, 0, 0x20, 0, 0, 0x43], {registers, execution: {budget: 2}});
  rejectJsService(f);
  try {
    const scalar = new Uint8Array(f.engine.memory.buffer, f.engine.base, 56).slice(), banks = processBanks(f);
    view(scalar).setUint32(48, 0x1008, true);
    const fp = view(banks[0]);
    fp.setUint16(22, 0x11d, true); fp.setUint32(24, 0x1002, true); fp.setUint32(28, 0x2000, true);
    fp.setUint16(32, 0x1b, true); fp.setUint16(34, 0x23, true);
    banks[0].set([0, 0, 0, 0, 0, 0, 0, 0x80, 0xff, 0x3f], 110);
    let selected = f.cache.select(); const state = f.slab(), backing = state.getUint32(84, true), length = state.getUint32(88, true) * 4096;
    const ram = new Uint8Array(f.engine.memory.buffer, backing, length).slice();
    const slot = new DataView(f.engine.memory.buffer).getUint32(state.getUint32(76, true) + 2 * 4, true) - 1;
    new DataView(ram.buffer).setUint32(slot * 4096, 0x3f800000, true);
    let retired = 0, work = 0, run;
    for (let count = 0; count < 3; count++) {
      run = f.cache.execute(selected); retired += run.retired; work += run.work;
      if (run.coreReason !== 1) break;
      selected = f.cache.select();
    }
    assert.deepEqual([run.reason, retired, work, run.pc], [1, 2, 2, 0x1008]);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, f.engine.base, 56), scalar);
    assert.deepEqual(processBanks(f), banks);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, backing, length), ram);
    serviceCounts(f, 0);
  } finally {f.close();}
});

test('actual service trap publishes its completed instruction once and retains the pre-trap event site', {skip}, () => {
  const registers = [0x12345678, 0x89abcdef, 0xd5e6789a, 0x23456789, 0x2f00, 0x55443322, 0x66554433, 0x77665544];
  const f = fixture([0xcc, 0x43], {registers, execution: {budget: 2}});
  rejectJsService(f);
  try {
    const scalar = new Uint8Array(f.engine.memory.buffer, f.engine.base, 56).slice(), banks = processBanks(f);
    view(scalar).setUint32(48, 0x1001, true);
    const selected = f.cache.select(), state = f.slab(), backing = state.getUint32(84, true), length = state.getUint32(88, true) * 4096;
    const ram = new Uint8Array(f.engine.memory.buffer, backing, length).slice(), run = f.cache.execute(selected);
    assert.deepEqual([run.reason, run.retired, run.work, run.pc], [17, 1, 1, 0x1001]);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, f.engine.base, 56), scalar);
    assert.deepEqual(processBanks(f), banks);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, backing, length), ram);
    serviceCounts(f, 1);
    f.engine.ok('take_cpu_event'); const packet = f.engine.resultView(48);
    assert.deepEqual([packet.getUint32(16, true), packet.getUint32(20, true), packet.getUint32(24, true),
      packet.getUint32(28, true), packet.getUint32(44, true)], [3, 0x1000, 0x1001, 0, 1]);
  } finally {f.close();}
});

test('actual inline x87 store commits its executable alias before the completed SMC exit', {skip}, () => {
  const f = fixture(undefined, {windowsMappings: true, physicalPages: 1024, execution: {budget: 2}});
  rejectJsService(f);
  try {
    const [writable, executable] = sharedViews(f, 0x40, [2, 0x24]);
    f.engine.request(Uint8Array.from([0xb8, 1, 0, 0, 0, 0xeb, 0xf9])); f.engine.ok('upload', writable, 7);
    new DataView(f.engine.memory.buffer).setUint32(f.engine.base + 48, executable, true);
    const old = f.cache.select(); assert.equal(f.cache.execute(old).reason, 1);
    replaceCode(f, [0xd9, 0x1d, ...dword(writable), 0x43]);
    const arena = new DataView(f.engine.memory.buffer), bank = f.engine.base + 4236;
    arena.setUint32(f.engine.base + 28, 0x33445566, true); arena.setUint32(f.engine.base + 48, 0x1000, true);
    arena.setUint16(bank + 20, 0xfffc, true);
    new Uint8Array(f.engine.memory.buffer, bank + 40, 10).set([0, 0, 0, 0, 0, 0, 0, 0x80, 0xff, 0x3f]);
    const scalar = new Uint8Array(f.engine.memory.buffer, f.engine.base, 56).slice(), banks = processBanks(f);
    view(scalar).setUint32(48, 0x1006, true);
    const fp = view(banks[0]); fp.setUint16(18, 0x800, true); fp.setUint16(20, 0xffff, true);
    fp.setUint16(22, 0x11d, true); fp.setUint32(24, 0x1000, true); fp.setUint32(28, writable, true);
    fp.setUint16(32, 0x1b, true); fp.setUint16(34, 0x23, true);
    const writer = f.cache.select(), state = f.slab(), backing = state.getUint32(84, true), length = state.getUint32(88, true) * 4096;
    const slot = new DataView(f.engine.memory.buffer).getUint32(state.getUint32(76, true) + (writable >>> 12) * 4, true) - 1;
    const ram = new Uint8Array(f.engine.memory.buffer, backing, length).slice(); new DataView(ram.buffer).setUint32(slot * 4096, 0x3f800000, true);
    const run = f.cache.execute(writer);
    assert.deepEqual([run.coreReason, run.reason, run.retired, run.work, run.pc], [6, 6, 1, 1, 0x1006]);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, f.engine.base, 56), scalar);
    assert.deepEqual(processBanks(f), banks);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, backing, length), ram);
    f.cache.import(); assert.equal(f.cache.table.get(old.slot), null);
    serviceCounts(f, 0);
  } finally {f.close();}
});

test('actual inline x87 transport preserves every raw80 ghost and logical FS metadata at each budget boundary', {skip}, () => {
  const one = [0, 0, 0, 0, 0, 0, 0, 0x80, 0xff, 0x3f];
  const epsilon = [0, 0, 0, 0, 0, 1, 0, 0x80, 0xff, 0x3f];
  const registers = [0x12345678, 0x89abcdef, 0xd5e6789a, 0x23456789, 0x2f00, 0x55443322, 0x66554433, 0x77665544];
  const f = fixture([0x64, 0xd9, 0x05, ...dword(0x1000), 0xd9, 0xe8, 0xd9, 0xc9, 0xdd, 0xd8,
    0x64, 0xd9, 0x1d, ...dword(0x1004)], {registers, fpControl: 0x003f, fpStatus: 0x4220,
    data: dword(0x3f800001).map((value, index) => [0x2000 + index, value]), execution: {budget: 1}});
  try {
    f.engine.ok('set_segment_base', 4, 0x1000);
    const scalar = new Uint8Array(f.engine.memory.buffer, f.engine.base, 56).slice(), banks = processBanks(f), fp = view(banks[0]);
    let ram, backing, length, dataSlot;
    for (const step of [
      {pc: 0x1000, next: 0x1007, status: 0x7820, tag: 0x3fff, opcode: 0x105, data: 0x1000, raw: [[7, epsilon]]},
      {pc: 0x1007, next: 0x1009, status: 0x7020, tag: 0x0fff, opcode: 0x1e8, data: 0x1000, raw: [[6, one]]},
      {pc: 0x1009, next: 0x100b, status: 0x7020, tag: 0x0fff, opcode: 0x1c9, data: 0x1000, raw: [[6, epsilon], [7, one]]},
      {pc: 0x100b, next: 0x100d, status: 0x7820, tag: 0x3fff, opcode: 0x5d8, data: 0x1000, raw: []},
      {pc: 0x100d, next: 0x1014, status: 0x4020, tag: 0xffff, opcode: 0x11d, data: 0x1004, raw: []},
    ]) {
      const selected = f.cache.select(), state = f.slab();
      if (!ram) {
        backing = state.getUint32(84, true); length = state.getUint32(88, true) * 4096;
        ram = new Uint8Array(f.engine.memory.buffer, backing, length).slice();
        dataSlot = new DataView(f.engine.memory.buffer).getUint32(state.getUint32(76, true) + 2 * 4, true) - 1;
      }
      view(scalar).setUint32(48, step.next, true);
      fp.setUint16(18, step.status, true); fp.setUint16(20, step.tag, true); fp.setUint16(22, step.opcode, true);
      fp.setUint32(24, step.pc, true); fp.setUint32(28, step.data, true); fp.setUint16(32, 0x1b, true); fp.setUint16(34, 0x3b, true);
      for (const [slot, raw] of step.raw) banks[0].set(raw, 40 + slot * 10);
      if (step.next === 0x1014) new DataView(ram.buffer).setUint32(dataSlot * 4096 + 4, 0x3f800000, true);
      const run = f.cache.execute(selected);
      assert.deepEqual([run.reason, run.retired, run.work, run.pc], [1, 1, 1, step.next]);
      assert.deepEqual(new Uint8Array(f.engine.memory.buffer, f.engine.base, 56), scalar);
      assert.deepEqual(processBanks(f), banks);
      assert.deepEqual(new Uint8Array(f.engine.memory.buffer, backing, length), ram);
      assert.equal(f.slab().getUint32(48, true), 0);
    }
    serviceCounts(f, 0);
  } finally {f.close();}
});

test('actual inline x87 register exchange and pop retain normal raw80 values wider than binary32', {skip}, () => {
  const first = [1, 0, 0, 0, 0, 0, 0, 0x80, 0xff, 0x3f];
  const second = [1, 0, 0, 0, 0, 0, 0, 0xc0, 0, 0x40];
  const f = fixture([0xd9, 0xc9, 0xdd, 0xd8, 0xdd, 0xd8], {fpControl: 0x007f, fpStatus: 0x200, execution: {budget: 3}});
  try {
    const arena = new DataView(f.engine.memory.buffer), bank = f.engine.base + 4236;
    arena.setUint16(bank + 20, 0xfff0, true);
    new Uint8Array(f.engine.memory.buffer, bank + 40, 10).set(first);
    new Uint8Array(f.engine.memory.buffer, bank + 50, 10).set(second);
    const scalar = new Uint8Array(f.engine.memory.buffer, f.engine.base, 56).slice(), banks = processBanks(f), fp = view(banks[0]);
    view(scalar).setUint32(48, 0x1006, true); fp.setUint16(18, 0x1000, true); fp.setUint16(20, 0xffff, true);
    fp.setUint16(22, 0x5d8, true); fp.setUint32(24, 0x1004, true); fp.setUint16(32, 0x1b, true);
    banks[0].set(second, 40); banks[0].set(first, 50);
    let selected = f.cache.select(); const state = f.slab(), backing = state.getUint32(84, true), length = state.getUint32(88, true) * 4096;
    const ram = new Uint8Array(f.engine.memory.buffer, backing, length).slice();
    let run, work = 0, retired = 0;
    for (let count = 0; count < 4; count++) {
      run = f.cache.execute(selected); work += run.work; retired += run.retired;
      if (run.coreReason !== 1) break;
      selected = f.cache.select();
    }
    assert.deepEqual([run.reason, retired, work, run.pc], [1, 3, 3, 0x1006]);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, f.engine.base, 56), scalar);
    assert.deepEqual(processBanks(f), banks);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, backing, length), ram);
    serviceCounts(f, 0);
  } finally {f.close();}
});

test('actual inline x87 late store fault preserves the pending pop raw payload flags and metadata', {skip}, () => {
  const f = fixture([0xd9, 0x1d, 0xfe, 0x2f, 0, 0, 0x43], {fpControl: 0x007f, fpStatus: 0x200,
    data: [[0x2ffe, 0x5a], [0x2fff, 0xa5]], execution: {budget: 2}});
  try {
    const arena = new DataView(f.engine.memory.buffer), bank = f.engine.base + 4236;
    arena.setUint16(bank + 20, 0xfffc, true); arena.setUint16(bank + 22, 0x321, true);
    arena.setUint32(bank + 24, 0x1234, true); arena.setUint32(bank + 28, 0x4321, true);
    arena.setUint16(bank + 32, 0x1b, true); arena.setUint16(bank + 34, 0x3b, true);
    new Uint8Array(f.engine.memory.buffer, bank + 40, 10).set([0, 0, 0, 0, 0, 0, 0, 0x80, 0xff, 0x3f]);
    const scalar = new Uint8Array(f.engine.memory.buffer, f.engine.base, 56).slice(), banks = processBanks(f);
    const selected = f.cache.select(), state = f.slab(), backing = state.getUint32(84, true), length = state.getUint32(88, true) * 4096;
    const ram = new Uint8Array(f.engine.memory.buffer, backing, length).slice(), run = f.cache.execute(selected);
    assert.deepEqual([run.reason, run.retired, run.work, run.pc], [5, 0, 0, 0x1000]);
    assert.deepEqual([f.engine.get(80), f.engine.get(84), f.engine.get(88), f.engine.get(92)], [1, 0x3000, 2, 4]);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, f.engine.base, 56), scalar);
    assert.deepEqual(processBanks(f), banks);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, backing, length), ram);
    assert.equal(f.slab().getUint32(48, true), 2);
    event(f.engine, 0xffffffff, 0x1000, 0);
    serviceCounts(f, 0);
  } finally {f.close();}
});

for (const c of [
  {name: 'exact ADD', operation: 0x05, left: 0x40400000, right: 0x40000000, output: 0x40a00000,
    raw: [0, 0, 0, 0, 0, 0, 0, 0xa0, 1, 0x40], control: 0x003f, status: 0x4020, inline: true},
  {name: 'exact MUL', operation: 0x0d, left: 0x40400000, right: 0x40000000, output: 0x40c00000,
    raw: [0, 0, 0, 0, 0, 0, 0, 0xc0, 1, 0x40], control: 0x003f, status: 0x4020, inline: true},
  {name: 'exact DIV', operation: 0x35, left: 0x40400000, right: 0x40000000, output: 0x3fc00000,
    raw: [0, 0, 0, 0, 0, 0, 0, 0xc0, 0xff, 0x3f], control: 0x003f, status: 0x4020, inline: true},
  {name: 'non-PC24 ADD', operation: 0x05, left: 0x40400000, right: 0x40000000, output: 0x40a00000,
    raw: [0, 0, 0, 0, 0, 0, 0, 0xa0, 1, 0x40], control: 0x037f, status: 0x4020, inline: false},
  {name: 'unmasked ADD', operation: 0x05, left: 0x40400000, right: 0x40000000, output: 0x40a00000,
    raw: [0, 0, 0, 0, 0, 0, 0, 0xa0, 1, 0x40], control: 0x003e, status: 0x4020, inline: false},
  {name: 'inexact DIV', operation: 0x35, left: 0x3f800000, right: 0x40400000, output: 0x3eaaaaab,
    raw: [0, 0, 0, 0, 0, 0xab, 0xaa, 0xaa, 0xfd, 0x3f], control: 0x003f, status: 0x4000, inline: true},
]) test(`actual x87 ${c.name} preserves raw CW sticky PE C3 and the strict arithmetic result`, {skip}, () => {
  const registers = [0x12345678, 0x89abcdef, 0xd5e6789a, 0x23456789, 0x2f00, 0x55443322, 0x66554433, 0x77665544];
  const data = [...dword(c.left).map((value, index) => [0x2000 + index, value]),
    ...dword(c.right).map((value, index) => [0x2004 + index, value])];
  const f = fixture([0xd9, 0x05, ...dword(0x2000), 0xd8, c.operation, ...dword(0x2004),
    0xd9, 0x1d, ...dword(0x2008), 0x43], {registers, data, fpControl: c.control, fpStatus: c.status, execution: {budget: 3}});
  try {
    const scalar = new Uint8Array(f.engine.memory.buffer, f.engine.base, 56).slice(), banks = processBanks(f), fp = view(banks[0]);
    view(scalar).setUint32(48, 0x1012, true); fp.setUint16(18, 0x4020, true);
    fp.setUint16(22, 0x11d, true); fp.setUint32(24, 0x100c, true); fp.setUint32(28, 0x2008, true);
    fp.setUint16(32, 0x1b, true); fp.setUint16(34, 0x23, true); banks[0].set(c.raw, 110);
    let selected = f.cache.select(); const state = f.slab(), backing = state.getUint32(84, true), length = state.getUint32(88, true) * 4096;
    const ram = new Uint8Array(f.engine.memory.buffer, backing, length).slice();
    const slot = new DataView(f.engine.memory.buffer).getUint32(state.getUint32(76, true) + 2 * 4, true) - 1;
    new DataView(ram.buffer).setUint32(slot * 4096 + 8, c.output, true);
    let run, retired = 0, work = 0;
    for (let count = 0; count < 4; count++) {
      run = f.cache.execute(selected); retired += run.retired; work += run.work;
      if (run.coreReason !== 1) break;
      selected = f.cache.select();
    }
    assert.deepEqual([run.reason, retired, work, run.pc], [1, 3, 3, 0x1012]);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, f.engine.base, 56), scalar);
    assert.deepEqual(processBanks(f), banks);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, backing, length), ram);
    if (c.inline) serviceCounts(f, 0);
    else {
      assert(f.cache.stats().instructionCalls > 0, 'an input outside the proved arithmetic predicate retains the independent instruction service');
      serviceCounts(f, f.cache.stats().instructionCalls);
    }
  } finally {f.close();}
});

test('actual special x87 infinity transport retains the independent service and exact ghost payload', {skip}, () => {
  const f = fixture([0xd9, 0x05, ...dword(0x2000), 0xd9, 0x1d, ...dword(0x2004)], {
    data: dword(0x7f800000).map((value, index) => [0x2000 + index, value]), fpControl: 0x003f, fpStatus: 0x4020, execution: {budget: 2}});
  try {
    const scalar = new Uint8Array(f.engine.memory.buffer, f.engine.base, 56).slice(), banks = processBanks(f), fp = view(banks[0]);
    view(scalar).setUint32(48, 0x100c, true); fp.setUint16(22, 0x11d, true);
    fp.setUint32(24, 0x1006, true); fp.setUint32(28, 0x2004, true); fp.setUint16(32, 0x1b, true); fp.setUint16(34, 0x23, true);
    banks[0].set([0, 0, 0, 0, 0, 0, 0, 0x80, 0xff, 0x7f], 110);
    let selected = f.cache.select(); const state = f.slab(), backing = state.getUint32(84, true), length = state.getUint32(88, true) * 4096;
    const ram = new Uint8Array(f.engine.memory.buffer, backing, length).slice();
    const slot = new DataView(f.engine.memory.buffer).getUint32(state.getUint32(76, true) + 2 * 4, true) - 1;
    new DataView(ram.buffer).setUint32(slot * 4096 + 4, 0x7f800000, true);
    let run, retired = 0, work = 0;
    for (let count = 0; count < 3; count++) {
      run = f.cache.execute(selected); retired += run.retired; work += run.work;
      if (run.coreReason !== 1) break;
      selected = f.cache.select();
    }
    assert.deepEqual([run.reason, retired, work, run.pc], [1, 2, 2, 0x100c]);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, f.engine.base, 56), scalar);
    assert.deepEqual(processBanks(f), banks);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, backing, length), ram);
    serviceCounts(f, 2);
  } finally {f.close();}
});

test('actual MSVCRT rotate and XLAT sequence preserves CH AL aliases and exact budget progress', {skip}, () => {
  const registers = [0x12345678, 0x89abf034, 0xd5e6789a, 0x2000, 0x2f00, 0x55443322, 0x66554433, 0x77665544];
  const f = fixture([0xd0, 0xc5, 0x8a, 0xc5, 0x24, 0x0f, 0xd7], {registers, data: [[0x2001, 0x5a]], execution: {budget: 1}});
  try {
    const scalar = new Uint8Array(f.engine.memory.buffer, f.engine.base, 56).slice(), banks = processBanks(f);
    let ram, backing, length;
    for (const [pc, eax, ecx, flags] of [[0x1002, 0x12345678, 0x89abe134, 3], [0x1004, 0x123456e1, 0x89abe134, 3],
      [0x1006, 0x12345601, 0x89abe134, 2], [0x1007, 0x1234565a, 0x89abe134, 2]]) {
      const selected = f.cache.select(), state = f.slab();
      if (!ram) {
        backing = state.getUint32(84, true); length = state.getUint32(88, true) * 4096;
        ram = new Uint8Array(f.engine.memory.buffer, backing, length).slice();
      }
      view(scalar).setUint32(16, eax, true); view(scalar).setUint32(20, ecx, true);
      view(scalar).setUint32(48, pc, true); view(scalar).setUint32(52, flags, true);
      const run = f.cache.execute(selected);
      assert.deepEqual([run.reason, run.retired, run.work, run.pc], [1, 1, 1, pc]);
      assert.deepEqual(new Uint8Array(f.engine.memory.buffer, f.engine.base, 56), scalar);
      assert.deepEqual(processBanks(f), banks);
      assert.deepEqual(new Uint8Array(f.engine.memory.buffer, backing, length), ram);
    }
    serviceCounts(f, 0);
  } finally {f.close();}
});

test('actual binary64 transport preserves PC53 raw payload and FS metadata at separate budget boundaries', {skip}, () => {
  const payload = [0, 8, 0, 0, 0, 0, 0, 0x80, 0xff, 0x3f];
  const input = [1, 0, 0, 0, 0, 0, 0xf0, 0x3f];
  const f = fixture([0x64, 0xdd, 0x05, ...dword(0x1001), 0x64, 0xdd, 0x1d, ...dword(0x1011)], {
    fpControl: 0x023f, fpStatus: 0x4220, execution: {budget: 1}, data: input.map((v, i) => [0x2001 + i, v])});
  try {
    f.engine.ok('set_segment_base', 4, 0x1000);
    const scalar = new Uint8Array(f.engine.memory.buffer, f.engine.base, 56).slice(), banks = processBanks(f), fp = view(banks[0]);
    let ram, backing, length, slot;
    for (const [next, status, tag, opcode, pc, offset] of [[0x1007, 0x7820, 0x3fff, 0x505, 0x1000, 0x1001],
      [0x100e, 0x4020, 0xffff, 0x51d, 0x1007, 0x1011]]) {
      const selected = f.cache.select(), state = f.slab();
      if (!ram) {
        backing = state.getUint32(84, true); length = state.getUint32(88, true) * 4096;
        ram = new Uint8Array(f.engine.memory.buffer, backing, length).slice();
        slot = new DataView(f.engine.memory.buffer).getUint32(state.getUint32(76, true) + 8, true) - 1;
      }
      view(scalar).setUint32(48, next, true); fp.setUint16(18, status, true); fp.setUint16(20, tag, true);
      fp.setUint16(22, opcode, true); fp.setUint32(24, pc, true); fp.setUint32(28, offset, true);
      fp.setUint16(32, 0x1b, true); fp.setUint16(34, 0x3b, true); banks[0].set(payload, 110);
      if (next === 0x100e) ram.set(input, slot * 4096 + 0x11);
      const run = f.cache.execute(selected);
      assert.deepEqual([run.reason, run.retired, run.work, run.pc], [1, 1, 1, next]);
      assert.deepEqual(new Uint8Array(f.engine.memory.buffer, f.engine.base, 56), scalar);
      assert.deepEqual(processBanks(f), banks);
      assert.deepEqual(new Uint8Array(f.engine.memory.buffer, backing, length), ram);
    }
    serviceCounts(f, 0);
  } finally {f.close();}
});

test('actual binary64 cross-page store delegates the whole width before fault and pop', {skip}, () => {
  const f = fixture([0xdd, 0x1d, ...dword(0x2ffc)], {fpControl: 0x023f, fpStatus: 0x4200,
    data: [[0x2ffc, 0x5a], [0x2fff, 0xa5]], execution: {budget: 1}});
  try {
    const bank = f.engine.base + 4236, arena = new DataView(f.engine.memory.buffer);
    arena.setUint16(bank + 20, 0xfffc, true);
    new Uint8Array(f.engine.memory.buffer, bank + 40, 10).set([0, 8, 0, 0, 0, 0, 0, 0x80, 0xff, 0x3f]);
    const scalar = new Uint8Array(f.engine.memory.buffer, f.engine.base, 56).slice(), banks = processBanks(f);
    const selected = f.cache.select(), state = f.slab(), backing = state.getUint32(84, true), length = state.getUint32(88, true) * 4096;
    const ram = new Uint8Array(f.engine.memory.buffer, backing, length).slice(), run = f.cache.execute(selected);
    assert.deepEqual([run.reason, run.retired, run.work, run.pc], [5, 0, 0, 0x1000]);
    assert.deepEqual([f.engine.get(80), f.engine.get(84), f.engine.get(88), f.engine.get(92)], [1, 0x3000, 2, 8]);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, f.engine.base, 56), scalar);
    assert.deepEqual(processBanks(f), banks);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, backing, length), ram);
    serviceCounts(f, 1);
  } finally {f.close();}
});

test('actual inexact PC24 division publishes PE and C1 before a later store can clear C1', {skip}, () => {
  const f = fixture([0xd8, 0xf1], {fpControl: 0x003f, fpStatus: 0x4000, execution: {budget: 1}});
  try {
    const bank = f.engine.base + 4236, arena = new DataView(f.engine.memory.buffer);
    arena.setUint16(bank + 20, 0xfff0, true);
    new Uint8Array(f.engine.memory.buffer, bank + 40, 20).set([0, 0, 0, 0, 0, 0, 0, 0x80, 0xff, 0x3f,
      0, 0, 0, 0, 0, 0, 0, 0xc0, 0, 0x40]);
    const scalar = new Uint8Array(f.engine.memory.buffer, f.engine.base, 56).slice(), banks = processBanks(f), fp = view(banks[0]);
    view(scalar).setUint32(48, 0x1002, true); fp.setUint16(18, 0x4220, true); fp.setUint16(22, 0xf1, true);
    fp.setUint32(24, 0x1000, true); fp.setUint16(32, 0x1b, true);
    banks[0].set([0, 0, 0, 0, 0, 0xab, 0xaa, 0xaa, 0xfd, 0x3f], 40);
    const selected = f.cache.select(), state = f.slab(), backing = state.getUint32(84, true), length = state.getUint32(88, true) * 4096;
    const ram = new Uint8Array(f.engine.memory.buffer, backing, length).slice(), run = f.cache.execute(selected);
    assert.deepEqual([run.reason, run.retired, run.work, run.pc], [1, 1, 1, 0x1002]);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, f.engine.base, 56), scalar);
    assert.deepEqual(processBanks(f), banks);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, backing, length), ram);
    serviceCounts(f, 0);
  } finally {f.close();}
});

for (const c of [
  {name: 'one third', left: [0, 0, 0, 0, 0, 0, 0, 0x80, 0xff, 0x3f],
    right: [0, 0, 0, 0, 0, 0, 0, 0xc0, 0, 0x40], raw: [0, 0xa8, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xfd, 0x3f], status: 0x4020, services: 0},
  {name: 'one tenth', left: [0, 0, 0, 0, 0, 0, 0, 0x80, 0xff, 0x3f],
    right: [0, 0, 0, 0, 0, 0, 0, 0xa0, 2, 0x40], raw: [0, 0xd0, 0xcc, 0xcc, 0xcc, 0xcc, 0xcc, 0xcc, 0xfb, 0x3f], status: 0x4220, services: 0},
  {name: 'lower binary64 range tie', left: [0, 0xf8, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 1, 0x3c],
    right: [0, 0, 0, 0, 0, 0, 0, 0x80, 0, 0x40], raw: [0, 0xf8, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0, 0x3c], status: 0x4000, services: 1},
]) test(`actual PC53 division ${c.name} preserves the strict raw result and immediate PE C1`, {skip}, () => {
  const f = fixture([0xd8, 0xf1], {fpControl: 0x023f, fpStatus: 0x4200, execution: {budget: 1}});
  try {
    const bank = f.engine.base + 4236, arena = new DataView(f.engine.memory.buffer);
    arena.setUint16(bank + 20, 0xfff0, true);
    new Uint8Array(f.engine.memory.buffer, bank + 40, 20).set([...c.left, ...c.right]);
    const scalar = new Uint8Array(f.engine.memory.buffer, f.engine.base, 56).slice(), banks = processBanks(f), fp = view(banks[0]);
    view(scalar).setUint32(48, 0x1002, true); fp.setUint16(18, c.status, true);
    fp.setUint16(22, 0xf1, true); fp.setUint32(24, 0x1000, true); fp.setUint16(32, 0x1b, true); banks[0].set(c.raw, 40);
    const selected = f.cache.select(), state = f.slab(), backing = state.getUint32(84, true), length = state.getUint32(88, true) * 4096;
    const ram = new Uint8Array(f.engine.memory.buffer, backing, length).slice(), run = f.cache.execute(selected);
    assert.deepEqual([run.reason, run.retired, run.work, run.pc], [1, 1, 1, 0x1002]);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, f.engine.base, 56), scalar);
    assert.deepEqual(processBanks(f), banks);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, backing, length), ram);
    serviceCounts(f, c.services);
  } finally {f.close();}
});

for (const control of [0x003f, 0x023f]) for (const negative of [false, true]) for (const shift of [0, 2000])
test(`actual raw80 MUL preserves PC${control === 0x003f ? 24 : 53} sign ${negative} extended exponent ${shift} and immediate PE C1`, {skip}, () => {
  const f = fixture([0xd8, 0xc9], {fpControl: control, fpStatus: 0x4200, execution: {budget: 1}});
  try {
    const bank = f.engine.base + 4236, arena = new DataView(f.engine.memory.buffer);
    arena.setUint16(bank + 20, 0xfff0, true);
    const firstExponent = (0x3fff + shift) | (negative ? 0x8000 : 0), secondExponent = 0x3fff - shift;
    new Uint8Array(f.engine.memory.buffer, bank + 40, 20).set([0, 8, 0, 0, 0, 0, 0, 0x80, firstExponent & 255, firstExponent >>> 8,
      0, 8, 0, 0, 0, 0, 0, 0x80, secondExponent & 255, secondExponent >>> 8]);
    const scalar = new Uint8Array(f.engine.memory.buffer, f.engine.base, 56).slice(), banks = processBanks(f), fp = view(banks[0]);
    view(scalar).setUint32(48, 0x1002, true); fp.setUint16(18, 0x4020, true);
    fp.setUint16(22, 0xc9, true); fp.setUint32(24, 0x1000, true); fp.setUint16(32, 0x1b, true);
    banks[0].set([0, control === 0x003f ? 0 : 16, 0, 0, 0, 0, 0, 0x80, 0xff, negative ? 0xbf : 0x3f], 40);
    const selected = f.cache.select(), state = f.slab(), backing = state.getUint32(84, true), length = state.getUint32(88, true) * 4096;
    const ram = new Uint8Array(f.engine.memory.buffer, backing, length).slice(), run = f.cache.execute(selected);
    assert.deepEqual([run.reason, run.retired, run.work, run.pc], [1, 1, 1, 0x1002]);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, f.engine.base, 56), scalar);
    assert.deepEqual(processBanks(f), banks);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, backing, length), ram);
    serviceCounts(f, 0);
  } finally {f.close();}
});

for (const odd of [false, true]) test(`actual PC53 ADD rounds the ${odd ? 'odd' : 'even'} midpoint and publishes PE C1 at its budget boundary`, {skip}, () => {
  const f = fixture([0xd8, 0xc1], {fpControl: 0x023f, fpStatus: 0x4200, execution: {budget: 1}});
  try {
    const bank = f.engine.base + 4236, arena = new DataView(f.engine.memory.buffer);
    arena.setUint16(bank + 20, 0xfff0, true);
    new Uint8Array(f.engine.memory.buffer, bank + 40, 20).set([0, odd ? 8 : 0, 0, 0, 0, 0, 0, 0x80, 0xff, 0x3f,
      0, 0, 0, 0, 0, 0, 0, 0x80, 0xca, 0x3f]);
    const scalar = new Uint8Array(f.engine.memory.buffer, f.engine.base, 56).slice(), banks = processBanks(f), fp = view(banks[0]);
    view(scalar).setUint32(48, 0x1002, true); fp.setUint16(18, odd ? 0x4220 : 0x4020, true);
    fp.setUint16(22, 0xc1, true); fp.setUint32(24, 0x1000, true); fp.setUint16(32, 0x1b, true);
    banks[0].set([0, odd ? 16 : 0, 0, 0, 0, 0, 0, 0x80, 0xff, 0x3f], 40);
    const selected = f.cache.select(), state = f.slab(), backing = state.getUint32(84, true), length = state.getUint32(88, true) * 4096;
    const ram = new Uint8Array(f.engine.memory.buffer, backing, length).slice(), run = f.cache.execute(selected);
    assert.deepEqual([run.reason, run.retired, run.work, run.pc], [1, 1, 1, 0x1002]);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, f.engine.base, 56), scalar);
    assert.deepEqual(processBanks(f), banks);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, backing, length), ram);
    serviceCounts(f, 0);
  } finally {f.close();}
});

for (const control of [0x003f, 0x023f]) for (const reverse of [false, true])
test(`actual PC${control === 0x003f ? 24 : 53} ${reverse ? 'reverse subtraction' : 'subtraction'} preserves the lost-addend PE C1 state`, {skip}, () => {
  const opcode = reverse ? 0xe9 : 0xe1;
  const f = fixture([0xd8, opcode], {fpControl: control, fpStatus: 0x4000, execution: {budget: 1}});
  try {
    const bank = f.engine.base + 4236, arena = new DataView(f.engine.memory.buffer);
    arena.setUint16(bank + 20, 0xfff0, true);
    const tinyExponent = 0x3fff - (control === 0x003f ? 60 : 100);
    new Uint8Array(f.engine.memory.buffer, bank + 40, 20).set([
      0, 0, 0, 0, 0, 0, 0, 0x80, 0xff, 0x3f,
      0, 0, 0, 0, 0, 0, 0, 0x80, tinyExponent & 255, tinyExponent >>> 8]);
    const scalar = new Uint8Array(f.engine.memory.buffer, f.engine.base, 56).slice(), banks = processBanks(f), fp = view(banks[0]);
    view(scalar).setUint32(48, 0x1002, true);
    fp.setUint16(18, 0x4220, true); fp.setUint16(22, opcode, true);
    fp.setUint32(24, 0x1000, true); fp.setUint16(32, 0x1b, true);
    banks[0].set([0, 0, 0, 0, 0, 0, 0, 0x80, 0xff, reverse ? 0xbf : 0x3f], 40);
    const selected = f.cache.select(), state = f.slab(), backing = state.getUint32(84, true), length = state.getUint32(88, true) * 4096;
    const ram = new Uint8Array(f.engine.memory.buffer, backing, length).slice(), run = f.cache.execute(selected);
    assert.deepEqual([run.reason, run.retired, run.work, run.pc], [1, 1, 1, 0x1002]);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, f.engine.base, 56), scalar);
    assert.deepEqual(processBanks(f), banks);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, backing, length), ram);
    serviceCounts(f, 0);
  } finally {f.close();}
});

test('actual SIMD transport and exact ADDPS preserve sticky MXCSR and pending x87 state at each budget', {skip}, () => {
  const left = [0x3f800000, 0x40000000, 0x40400000, 0x40800000], right = [0x3f000000, 0x3f800000, 0x40000000, 0x40800000];
  const result = [0x3fc00000, 0x40400000, 0x40a00000, 0x41000000];
  const data = [...left, ...right].flatMap((word, i) => dword(word).map((v, j) => [0x2000 + i * 4 + j, v]));
  const f = fixture([0x0f, 0x28, 0x05, ...dword(0x2000), 0x0f, 0x58, 0x05, ...dword(0x2010),
    0x0f, 0x29, 0x05, ...dword(0x2020)], {data, fpControl: 0x003e, fpStatus: 1, execution: {budget: 1}});
  try {
    new DataView(f.engine.memory.buffer).setUint32(f.engine.base + 4236 + 144, 0x1fa0, true);
    const scalar = new Uint8Array(f.engine.memory.buffer, f.engine.base, 56).slice(), banks = processBanks(f);
    let ram, backing, length, slot;
    for (const [index, next] of [0x1007, 0x100e, 0x1015].entries()) {
      const selected = f.cache.select(), state = f.slab();
      if (!ram) {
        backing = state.getUint32(84, true); length = state.getUint32(88, true) * 4096;
        ram = new Uint8Array(f.engine.memory.buffer, backing, length).slice();
        slot = new DataView(f.engine.memory.buffer).getUint32(state.getUint32(76, true) + 8, true) - 1;
      }
      view(scalar).setUint32(48, next, true);
      (index ? result : left).forEach((word, lane) => view(banks[0]).setUint32(160 + lane * 4, word, true));
      if (index === 2) result.forEach((word, lane) => new DataView(ram.buffer).setUint32(slot * 4096 + 32 + lane * 4, word, true));
      const run = f.cache.execute(selected);
      assert.deepEqual([run.reason, run.retired, run.work, run.pc], [1, 1, 1, next]);
      assert.deepEqual(new Uint8Array(f.engine.memory.buffer, f.engine.base, 56), scalar);
      assert.deepEqual(processBanks(f), banks);
      assert.deepEqual(new Uint8Array(f.engine.memory.buffer, backing, length), ram);
    }
    serviceCounts(f, 0);
    assert.equal(f.cache.stats().installedUnits, 1, 'budget resumes reuse every already compiled instruction');
  } finally {f.close();}
});

const directServiceSkip = skip || (!WebAssembly.Module.exports(artifact.module)
  .some(item => item.name === 'ring3_abi_v1_cpu_runtime_instruction') && 'artifact lacks the direct instruction service');

test('actual numeric service resumes the compiled suffix without another admission', {skip}, () => {
  const left = Array(4).fill(0x3dcccccd), right = Array(4).fill(0x3e4ccccd), result = Array(4).fill(0x3e99999a);
  const data = [...left, ...right].flatMap((word, i) => dword(word).map((v, j) => [0x2000 + i * 4 + j, v]));
  const f = fixture([0x0f, 0x28, 0x05, ...dword(0x2000), 0x0f, 0x58, 0x05, ...dword(0x2010),
    0x0f, 0x29, 0x05, ...dword(0x2020)], {data, execution: {budget: 3, instructions: 3}});
  try {
    const scalar = new Uint8Array(f.engine.memory.buffer, f.engine.base, 56).slice(), banks = processBanks(f);
    view(scalar).setUint32(48, 0x1015, true);
    view(banks[0]).setUint32(144, 0x1fa0, true);
    result.forEach((word, lane) => view(banks[0]).setUint32(160 + lane * 4, word, true));
    const selected = f.cache.select(), state = f.slab();
    const backing = state.getUint32(84, true), length = state.getUint32(88, true) * 4096;
    const ram = new Uint8Array(f.engine.memory.buffer, backing, length).slice();
    const slot = new DataView(f.engine.memory.buffer).getUint32(state.getUint32(76, true) + 8, true) - 1;
    result.forEach((word, lane) => new DataView(ram.buffer).setUint32(slot * 4096 + 32 + lane * 4, word, true));
    let run, retired = 0, work = 0;
    for (let attempt = 0; attempt < 3; attempt++) {
      run = f.cache.execute(attempt ? f.cache.select() : selected);
      retired += run.retired; work += run.work;
      if (run.coreReason !== 1) break;
    }
    assert.deepEqual([run.reason, retired, work, run.pc], [1, 3, 3, 0x1015]);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, f.engine.base, 56), scalar);
    assert.deepEqual(processBanks(f), banks);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, backing, length), ram);
    serviceCounts(f, 1);
    assert.equal(f.cache.stats().installedUnits, 1, 'the exact service continuation is an existing landing');
  } finally {f.close();}
});

test('actual caller keeps its compiled prefix when a separately owned callee changes', {skip}, () => {
  const code = [0x40, 0xe9, ...dword(0x1040 - 0x1006), ...Array(58).fill(0x90), 0x42, 0xeb, 0xfd];
  const f = fixture(code, {execution: {budget: 3, instructions: 4}});
  try {
    f.engine.ok('map', 0x3000, 1, 7);
    const caller = [0x43, 0xe9, ...dword(0x1040 - 0x3006)];
    f.engine.request(Uint8Array.from(caller)); f.engine.ok('upload', 0x3000, caller.length);
    f.engine.ok('protect', 0x3000, 1, 5);
    const scalar = new Uint8Array(f.engine.memory.buffer, f.engine.base, 56).slice(), banks = processBanks(f);
    const first = f.cache.select(), state = f.slab();
    const backing = state.getUint32(84, true), length = state.getUint32(88, true) * 4096;
    const ram = new Uint8Array(f.engine.memory.buffer, backing, length).slice();
    const codeSlot = new DataView(f.engine.memory.buffer).getUint32(state.getUint32(76, true) + 4, true) - 1;
    const execute = selected => {
      let run, retired = 0, work = 0;
      for (let attempt = 0; attempt < 4; attempt++) {
        run = f.cache.execute(attempt ? f.cache.select() : selected);
        retired += run.retired; work += run.work;
        if (run.coreReason !== 1) break;
      }
      assert.deepEqual([run.reason, retired, work, run.pc], [1, 3, 3, 0x1041]);
      assert.deepEqual(new Uint8Array(f.engine.memory.buffer, f.engine.base, 56), scalar);
      assert.deepEqual(processBanks(f), banks);
      assert.deepEqual(new Uint8Array(f.engine.memory.buffer, backing, length), ram);
    };
    view(scalar).setUint32(16, 1, true); view(scalar).setUint32(24, 1, true); view(scalar).setUint32(48, 0x1041, true);
    execute(first);
    new DataView(f.engine.memory.buffer).setUint32(f.engine.base + 48, 0x3000, true);
    const second = f.cache.select(), secondFunction = f.cache.table.get(second.slot);
    view(scalar).setUint32(24, 2, true); view(scalar).setUint32(28, 1, true);
    execute(second);
    f.engine.ok('protect', 0x1000, 1, 7); f.engine.ok('write8', 0x1040, 0x46); f.engine.ok('protect', 0x1000, 1, 5);
    ram[codeSlot * 4096 + 0x40] = 0x46;
    new DataView(f.engine.memory.buffer).setUint32(f.engine.base + 48, 0x3000, true);
    view(scalar).setUint32(28, 2, true); view(scalar).setUint32(40, 1, true);
    execute(f.cache.select());
    serviceCounts(f, 0);
    assert.equal(f.cache.table.get(second.slot), secondFunction,
      'callee invalidation retains the caller function that did not compile those bytes');
  } finally {f.close();}
});

for (const foreign of [false, true]) test(`actual direct service rejects ${foreign ? 'foreign context without counting it' : 'zero-quota continuation before effects'}`, {skip: directServiceSkip}, () => {
  const f = fixture([0xd9, 0xe8]);
  try {
    f.engine.ok('cpu_begin', ...KEY);
    const state = f.slab(), pointer = f.engine.api.cpu_context_ptr() >>> 0;
    state.setUint32(48, foreign ? 1 : 0, true); state.setUint32(52, 7, true);
    const expected = new Uint8Array(f.engine.memory.buffer, pointer, 160).slice(); view(expected).setUint32(52, 8, true);
    const scalar = new Uint8Array(f.engine.memory.buffer, f.engine.base, 56).slice(), banks = processBanks(f);
    const backing = state.getUint32(84, true), length = state.getUint32(88, true) * 4096, ram = new Uint8Array(f.engine.memory.buffer, backing, length).slice();
    assert.equal(f.engine.api.cpu_runtime_instruction(pointer + (foreign ? 8 : 0)), 8);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, pointer, 160), expected);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, f.engine.base, 56), scalar);
    assert.deepEqual(processBanks(f), banks);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, backing, length), ram);
    assert.deepEqual([f.engine.api.cpu_runtime_instruction_count_low() >>> 0,
      f.engine.api.cpu_runtime_instruction_count_high() >>> 0], [foreign ? 0 : 1, 0]);
  } finally {f.engine.ok('cpu_end', ...KEY); f.close();}
});

test('actual prepare fetch fault reaches the guest fault boundary without installing a unit', {skip}, () => {
  const f = fixture(undefined, {pc: 0x5000});
  try {
    f.cache.import();
    const run = f.cache.execute(f.cache.select());
    assert.deepEqual([run.reason, run.retired, run.work, run.pc, run.id], [5, 0, 0, 0x5000, null]);
    assert.deepEqual([f.engine.get(80), f.engine.get(84), f.engine.get(88)], [1, 0x5000, 3]);
    assert.equal(f.cache.stats().installedUnits, 0);
    event(f.engine, 0xffffffff, 0x5000, 0);
  } finally {f.close();}
});

for (const [name, flag] of [['TF', 0x100], ['AC', 0x40000]]) test(`actual POPFD refuses ${name} before changing the instruction or stack state`, {skip}, () => {
  const f = fixture([0x9d], {data: dword(flag | 2).map((value, index) => [0x2f00 + index, value])});
  try {
    f.cache.import();
    assert.throws(() => f.cache.execute(f.cache.select()), /unsupported independent CPU instruction/);
    assert.deepEqual([f.engine.get(48), f.engine.get(32), f.engine.get(52), f.engine.get(76)], [0x1000, 0x2f00, 2, 0]);
    assert.equal(f.cache.stats().work, 0);
    assert.equal(f.slab().getUint32(48, true), 50000);
  } finally {f.close();}
});

test('actual independent entry rejects AC before guest execution or memory effects', {skip}, () => {
  const f = fixture([0xa1, 1, 0x20, 0, 0]);
  try {
    const state = new DataView(f.engine.memory.buffer);
    state.setUint16(f.engine.base + 4, 3, true);
    state.setUint32(f.engine.base + 52, 0x40002, true);
    const before = new Uint8Array(f.engine.memory.buffer, f.engine.base, 56).slice();
    assert.throws(() => f.cache.import(), /cpu_begin returned 7/);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, f.engine.base, 56), before);
    assert.equal(f.cache.stats().work, 0);
    f.engine.ok('read32', 0x2000); assert.equal(f.engine.get(120), 0);
  } finally {f.close();}
});

test('actual source ABA revokes each old Wasm table function before installing a fresh slot', {skip}, () => {
  const f = fixture([0xb8, 1, 0, 0, 0, 0xeb, 0xf9], {execution: {budget: 2, instructions: 2}});
  try {
    let previous;
    for (const value of [1, 2, 1]) {
      if (previous) {
        f.engine.ok('protect', 0x1000, 1, 7);
        f.engine.ok('write8', 0x1001, value);
        f.engine.ok('protect', 0x1000, 1, 5);
      }
      f.cache.import();
      if (previous) {
        assert.equal(f.cache.table.get(previous.slot), null);
        assert.equal(f.cache.units.has(previous.slot), false);
      }
      const selected = f.cache.select();
      if (previous) assert(selected.slot > previous.slot);
      const run = f.cache.execute(selected);
      assert.deepEqual([run.reason, run.retired, run.work, run.pc], [1, 2, 2, 0x1000]);
      assert.equal(f.engine.get(16), value);
      assert.equal(f.cache.stats().installedUnits, 1);
      assert.equal(f.cache.stats().retainedModuleBytes, selected.bytes);
      previous = selected;
    }
    assert.equal(f.cache.stats().evictions, 2);
  } finally {f.close();}
});

test('actual PUSHA and POPA preserve every register and the original ESP stack slot', {skip}, () => {
  const registers = [0x11111111, 0x22222222, 0x33333333, 0x44444444, 0x4800, 0x55555555, 0x66666666, 0x77777777];
  const f = fixture([0x60, 0x61], {registers, dataPage: 0x4000, execution: {budget: 2}});
  try {
    f.cache.import();
    const runs = [];
    for (let attempt = 0; attempt < 3; attempt++) {
      const run = f.cache.execute(f.cache.select()); runs.push(run);
      if (run.coreReason !== 1) break;
    }
    assert.deepEqual([runs.at(-1).reason, f.engine.get(48), f.engine.get(76)], [1, 0x1002, 2]);
    assert.equal(runs.reduce((sum, run) => sum + run.work, 0), 2);
    assert.deepEqual(registers.map((_, index) => f.engine.get(16 + index * 4)), registers);
    f.engine.ok('read32', 0x47ec); assert.equal(f.engine.get(120), 0x4800);
  } finally {f.close();}
});

test('actual late PUSHA fault leaves every register and the earlier mapped stack words unchanged', {skip}, () => {
  const registers = [0x11111111, 0x22222222, 0x33333333, 0x44444444, 0x3010, 0x55555555, 0x66666666, 0x77777777];
  const data = Array.from({length: 16}, (_, index) => [0x3000 + index, 0xa0 + index]);
  const f = fixture([0x60], {registers, data, dataPage: 0x3000});
  try {
    f.cache.import();
    const run = f.cache.execute(f.cache.select());
    assert.deepEqual([run.reason, run.retired, run.work, run.pc], [5, 0, 0, 0x1000]);
    assert.deepEqual(registers.map((_, index) => f.engine.get(16 + index * 4)), registers);
    assert.deepEqual([f.engine.get(80), f.engine.get(84), f.engine.get(88), f.engine.get(92)], [1, 0x2ffc, 2, 4]);
    for (const [index, expected] of [0xa3a2a1a0, 0xa7a6a5a4, 0xabaaa9a8, 0xafaeadac].entries()) {
      f.engine.ok('read32', 0x3000 + index * 4); assert.equal(f.engine.get(120), expected);
    }
    assert.equal(f.slab().getUint32(48, true), 50000);
    event(f.engine, 0xffffffff, 0x1000, 0);
  } finally {f.close();}
});

function arithmetic(code, registers, result, flags, fault) {
  const f = fixture(code, {registers, execution: {budget: 1, instructions: 1},
    data: Array.from({length: 64}, (_, index) => [0x2000 + index, index ^ 0xa5])});
  try {
    const arena = new DataView(f.engine.memory.buffer);
    arena.setUint16(f.engine.base + 4, 3, true);
    arena.setUint32(f.engine.base + 52, 0xcd7, true);
    const scalar = new Uint8Array(f.engine.memory.buffer, f.engine.base, 56).slice(), expected = view(scalar);
    expected.setUint16(4, 1, true);
    (result ?? registers).forEach((value, index) => expected.setUint32(16 + index * 4, value, true));
    expected.setUint32(48, fault ? 0x1000 : 0x1000 + code.length, true);
    expected.setUint32(52, flags ?? 0xcd7, true);
    const banks = [[4236, 352], [4612, 184]].map(([offset, length]) =>
      new Uint8Array(f.engine.memory.buffer, f.engine.base + offset, length).slice());
    f.cache.import(); const selected = f.cache.select(), state = f.slab();
    const backing = state.getUint32(84, true), length = state.getUint32(88, true) * 4096;
    const ram = new Uint8Array(f.engine.memory.buffer, backing, length).slice();
    const run = f.cache.execute(selected), retired = fault ? 0 : 1;
    assert.deepEqual([run.reason, run.retired, run.work, run.pc],
      [fault ?? 1, retired, retired, fault ? 0x1000 : 0x1000 + code.length]);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, f.engine.base, 56), scalar);
    assert.deepEqual([[4236, 352], [4612, 184]].map(([offset, size]) =>
      new Uint8Array(f.engine.memory.buffer, f.engine.base + offset, size)), banks);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, backing, length), ram);
    assert.equal(f.slab().getUint32(48, true), fault ? 1 : 0);
    assert.equal(f.slab().getUint32(108, true), 0);
    assert.equal(f.engine.get(76), retired);
    assert.equal(f.cache.stats().instructionCalls, 0);
    if (fault === 10) {
      assert.deepEqual([f.engine.get(80), f.engine.get(84), f.engine.get(88)], [0, 0, 0]);
      event(f.engine, 0, 0x1000, 0);
    } else if (fault === 5) {
      assert.deepEqual([f.engine.get(80), f.engine.get(84), f.engine.get(88), f.engine.get(92)], [1, 0x5000, 1, 1]);
      event(f.engine, 0xffffffff, 0x1000, 0);
    } else assert.equal(f.engine.status('take_cpu_event'), 7);
  } finally {f.close();}
}

for (const c of [
  {name: 'MUL8', code: [0xf6, 0xe3], before: [0xa1b2cc0f, 0x33221100, 0x76543210, 0xbbccdd10, 0x2f00, 0x55443322, 0x66554433, 0x77665544],
    after: [0xa1b200f0, 0x33221100, 0x76543210, 0xbbccdd10, 0x2f00, 0x55443322, 0x66554433, 0x77665544], flags: 0x402},
  {name: 'MUL16', code: [0x66, 0xf7, 0xe3], before: [0xa1b2fffe, 0x33221100, 0xd5e6abcd, 0xc3d40003, 0x2f00, 0x55443322, 0x66554433, 0x77665544],
    after: [0xa1b2fffa, 0x33221100, 0xd5e60002, 0xc3d40003, 0x2f00, 0x55443322, 0x66554433, 0x77665544], flags: 0xc03},
  {name: 'MUL32', code: [0xf7, 0xe3], before: [0xffffffff, 0x33221100, 0x76543210, 0xffffffff, 0x2f00, 0x55443322, 0x66554433, 0x77665544],
    after: [1, 0x33221100, 0xfffffffe, 0xffffffff, 0x2f00, 0x55443322, 0x66554433, 0x77665544], flags: 0xc03},
  {name: 'DIV8', code: [0xf6, 0xf3], before: [0xa1b21234, 0x33221100, 0x76543210, 0xbbccdd32, 0x2f00, 0x55443322, 0x66554433, 0x77665544],
    after: [0xa1b20a5d, 0x33221100, 0x76543210, 0xbbccdd32, 0x2f00, 0x55443322, 0x66554433, 0x77665544]},
  {name: 'DIV16', code: [0x66, 0xf7, 0xf3], before: [0xa1b20003, 0x33221100, 0xd5e60001, 0xc3d40101, 0x2f00, 0x55443322, 0x66554433, 0x77665544],
    after: [0xa1b200ff, 0x33221100, 0xd5e60004, 0xc3d40101, 0x2f00, 0x55443322, 0x66554433, 0x77665544]},
  {name: 'DIV32', code: [0xf7, 0xf3], before: [1, 0x33221100, 1, 3, 0x2f00, 0x55443322, 0x66554433, 0x77665544],
    after: [0x55555555, 0x33221100, 2, 3, 0x2f00, 0x55443322, 0x66554433, 0x77665544]},
]) test(`actual compiled ${c.name} publishes literal complete arithmetic state and unchanged RAM`, {skip}, () => {
  arithmetic(c.code, c.before, c.after, c.flags);
});

for (const c of [
  {name: 'DIV8 zero', code: [0xf6, 0xf3], eax: 0xa1b23456, edx: 0xd5e6789a, ebx: 0xbbccdd00},
  {name: 'DIV8 overflow', code: [0xf6, 0xf3], eax: 0xa1b20100, edx: 0xd5e6789a, ebx: 0xbbccdd01},
  {name: 'DIV16 zero', code: [0x66, 0xf7, 0xf3], eax: 0xa1b23456, edx: 0xd5e6789a, ebx: 0xbbcc0000},
  {name: 'DIV16 overflow', code: [0x66, 0xf7, 0xf3], eax: 0xa1b20000, edx: 0xd5e60001, ebx: 0xbbcc0001},
  {name: 'DIV32 zero', code: [0xf7, 0xf3], eax: 0x12345678, edx: 0x23456789, ebx: 0},
  {name: 'DIV32 overflow', code: [0xf7, 0xf3], eax: 0, edx: 1, ebx: 1},
  {name: 'IDIV32 minimum numerator divided by minus one', code: [0xf7, 0xfb], eax: 0, edx: 0x80000000, ebx: 0xffffffff},
]) test(`actual compiled ${c.name} raises precise DE without any arithmetic or RAM effect`, {skip}, () => {
  arithmetic(c.code, [c.eax, 0x33221100, c.edx, c.ebx, 0x2f00, 0x55443322, 0x66554433, 0x77665544], undefined, undefined, 10);
});

test('actual compiled DIV memory source fault precedes division and preserves complete state', {skip}, () => {
  arithmetic([0xf6, 0x35, 0, 0x50, 0, 0],
    [0xa1b20100, 0x33221100, 0xd5e6789a, 1, 0x2f00, 0x55443322, 0x66554433, 0x77665544], undefined, undefined, 5);
});

test('actual reached XCHG AL AH swaps only the aliased bytes and preserves FLAGS and RAM', {skip}, () => {
  arithmetic([0x86, 0xe0],
    [0xa1b2c3d4, 0x33221100, 0xd5e6789a, 0x12345678, 0x2f00, 0x55443322, 0x66554433, 0x77665544],
    [0xa1b2d4c3, 0x33221100, 0xd5e6789a, 0x12345678, 0x2f00, 0x55443322, 0x66554433, 0x77665544]);
});

test('actual reached SAHF ignores reserved AH bits and preserves overflow direction and every register', {skip}, () => {
  const registers = [0xa1b22bd4, 0x33221100, 0xd5e6789a, 0x12345678, 0x2f00, 0x55443322, 0x66554433, 0x77665544];
  arithmetic([0x9e], registers, registers, 0xc03);
});

test('actual LAHF encodes only status flags into AH and preserves FLAGS AL and the high word', {skip}, () => {
  arithmetic([0x9f],
    [0xa1b22bc3, 0x33221100, 0xd5e6789a, 0x12345678, 0x2f00, 0x55443322, 0x66554433, 0x77665544],
    [0xa1b2d7c3, 0x33221100, 0xd5e6789a, 0x12345678, 0x2f00, 0x55443322, 0x66554433, 0x77665544]);
});

test('actual reached SHR CH seven preserves the aliased register bytes and publishes literal FLAGS', {skip}, () => {
  arithmetic([0xc0, 0xed, 7],
    [0x12345678, 0x89abcdef, 0xd5e6789a, 0x23456789, 0x2f00, 0x55443322, 0x66554433, 0x77665544],
    [0x12345678, 0x89ab01ef, 0xd5e6789a, 0x23456789, 0x2f00, 0x55443322, 0x66554433, 0x77665544], 0x403);
});

test('actual zero-count memory SHR checks Modify permission before any state or RAM effects', {skip}, () => {
  const registers = [0x12345678, 0x89abcdef, 0xd5e6789a, 0x23456789, 0x2f00, 0x55443322, 0x66554433, 0x77665544];
  const f = fixture([0xc0, 0x2d, 0, 0x20, 0, 0, 0], {registers, data: [[0x2000, 0xcd]], execution: {budget: 1}});
  const calls = memoryCalls(f);
  try {
    new DataView(f.engine.memory.buffer).setUint32(f.engine.base + 52, 0xcd7, true);
    f.engine.ok('protect', 0x2000, 1, 1);
    const scalar = new Uint8Array(f.engine.memory.buffer, f.engine.base, 56).slice(), banks = processBanks(f);
    f.cache.import(); const selected = f.cache.select(), state = f.slab();
    const backing = state.getUint32(84, true), length = state.getUint32(88, true) * 4096;
    const ram = new Uint8Array(f.engine.memory.buffer, backing, length).slice();
    const run = f.cache.execute(selected);
    assert.deepEqual([run.reason, run.retired, run.work, run.pc], [5, 0, 0, 0x1000]);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, f.engine.base, 56), scalar);
    assert.deepEqual(processBanks(f), banks);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, backing, length), ram);
    assert.deepEqual([f.engine.get(80), f.engine.get(84), f.engine.get(88), f.engine.get(92)], [2, 0x2000, 2, 1]);
    assert.equal(f.slab().getUint32(48, true), 1);
    assert.equal(f.slab().getUint32(108, true), 0);
    assert(calls.read > 0); assert.equal(calls.write, 0);
    event(f.engine, 0xffffffff, 0x1000, 0);
  } finally {f.close();}
});

test('actual zero-count memory SHR still commits its same-value executable alias store before SMC exit', {skip}, () => {
  const f = fixture(undefined, {windowsMappings: true, physicalPages: 1024, execution: {budget: 2}});
  try {
    const [writable, executable] = sharedViews(f, 0x40, [2, 0x24]);
    f.engine.request(Uint8Array.from([0xb8, 1, 0, 0, 0, 0xeb, 0xf9])); f.engine.ok('upload', writable, 7);
    new DataView(f.engine.memory.buffer).setUint32(f.engine.base + 48, executable, true);
    const old = f.cache.select(); assert.equal(f.cache.execute(old).reason, 1);
    replaceCode(f, [0xc0, 0x2d, ...dword(writable + 1), 0, 0x43]);
    const arena = new DataView(f.engine.memory.buffer);
    arena.setUint32(f.engine.base + 28, 0x33445566, true);
    arena.setUint32(f.engine.base + 48, 0x1000, true); arena.setUint32(f.engine.base + 52, 0xcd7, true);
    const scalar = new Uint8Array(f.engine.memory.buffer, f.engine.base, 56).slice(), banks = processBanks(f);
    view(scalar).setUint32(48, 0x1007, true);
    const calls = memoryCalls(f), writer = f.cache.select(), state = f.slab();
    const backing = state.getUint32(84, true), length = state.getUint32(88, true) * 4096;
    const ram = new Uint8Array(f.engine.memory.buffer, backing, length).slice();
    const run = f.cache.execute(writer);
    assert.deepEqual([run.coreReason, run.reason, run.retired, run.work, run.pc], [6, 6, 1, 1, 0x1007]);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, f.engine.base, 56), scalar);
    assert.deepEqual(processBanks(f), banks);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, backing, length), ram);
    assert.equal(f.slab().getUint32(48, true), 1);
    assert.equal(calls.write, 1, 'zero count retains the one canonical same-value write');
    f.cache.import();
    assert.equal(f.cache.table.get(old.slot), null);
    assert.equal(f.cache.units.has(old.slot), false);
  } finally {f.close();}
});

for (const readOnly of [false, true]) test(`actual XCHG memory EAX ${readOnly ? 'fault preserves the aliased address and complete state' : 'uses the address before exchanging EAX'}`, {skip}, () => {
  const registers = [0x2000, 0x11223344, 0x22334455, 0x33445566, 0x2f00, 0x44556677, 0x55667788, 0x66778899];
  const f = fixture([0x87, 0x00], {registers, data: dword(0x12345678).map((value, index) => [0x2000 + index, value]), execution: {budget: 1}});
  try {
    new DataView(f.engine.memory.buffer).setUint32(f.engine.base + 52, 0xcd7, true);
    if (readOnly) f.engine.ok('protect', 0x2000, 1, 1);
    const scalar = new Uint8Array(f.engine.memory.buffer, f.engine.base, 56).slice(), banks = processBanks(f);
    if (!readOnly) {view(scalar).setUint32(16, 0x12345678, true); view(scalar).setUint32(48, 0x1002, true);}
    f.cache.import(); const selected = f.cache.select(), state = f.slab();
    const backing = state.getUint32(84, true), length = state.getUint32(88, true) * 4096;
    const expected = new Uint8Array(f.engine.memory.buffer, backing, length).slice();
    const slot = new DataView(f.engine.memory.buffer).getUint32(state.getUint32(76, true) + 2 * 4, true) - 1;
    if (!readOnly) new DataView(expected.buffer).setUint32(slot * 4096, 0x2000, true);
    const run = f.cache.execute(selected);
    assert.deepEqual([run.reason, run.retired, run.work, run.pc], readOnly ? [5, 0, 0, 0x1000] : [1, 1, 1, 0x1002]);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, f.engine.base, 56), scalar);
    assert.deepEqual(processBanks(f), banks);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, backing, length), expected);
    assert.equal(f.slab().getUint32(48, true), readOnly ? 1 : 0);
    assert.equal(f.slab().getUint32(108, true), 0);
    if (readOnly) {
      assert.deepEqual([f.engine.get(80), f.engine.get(84), f.engine.get(88), f.engine.get(92)], [2, 0x2000, 2, 4]);
      event(f.engine, 0xffffffff, 0x1000, 0);
    }
  } finally {f.close();}
});

test('actual XCHG through an executable alias commits once using its original address then invalidates code', {skip}, () => {
  const f = fixture(undefined, {windowsMappings: true, physicalPages: 1024, execution: {budget: 2}});
  try {
    const [writable, executable] = sharedViews(f, 0x40, [2, 0x24]);
    f.engine.request(Uint8Array.from([0xb8, 1, 0, 0, 0, 0xeb, 0xf9])); f.engine.ok('upload', writable, 7);
    new DataView(f.engine.memory.buffer).setUint32(f.engine.base + 48, executable, true);
    const old = f.cache.select(); assert.equal(f.cache.execute(old).reason, 1);
    replaceCode(f, [0x87, 0x00, 0x43]);
    const arena = new DataView(f.engine.memory.buffer);
    arena.setUint32(f.engine.base + 16, writable, true); arena.setUint32(f.engine.base + 28, 0x33445566, true);
    arena.setUint32(f.engine.base + 48, 0x1000, true); arena.setUint32(f.engine.base + 52, 0xcd7, true);
    const scalar = new Uint8Array(f.engine.memory.buffer, f.engine.base, 56).slice(), banks = processBanks(f);
    view(scalar).setUint32(16, 0x1b8, true); view(scalar).setUint32(48, 0x1002, true);
    const calls = memoryCalls(f);
    const writer = f.cache.select(), state = f.slab(), backing = state.getUint32(84, true), length = state.getUint32(88, true) * 4096;
    const mappings = state.getUint32(76, true), physical = new DataView(f.engine.memory.buffer);
    const slot = physical.getUint32(mappings + (writable >>> 12) * 4, true) - 1;
    assert.equal(physical.getUint32(mappings + (executable >>> 12) * 4, true) - 1, slot);
    const expected = new Uint8Array(f.engine.memory.buffer, backing, length).slice();
    new DataView(expected.buffer).setUint32(slot * 4096, writable, true);
    const run = f.cache.execute(writer);
    assert.deepEqual([run.coreReason, run.reason, run.retired, run.work, run.pc], [6, 6, 1, 1, 0x1002]);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, f.engine.base, 56), scalar);
    assert.deepEqual(processBanks(f), banks);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, backing, length), expected);
    assert(calls.write > 0, 'executable aliases retain the canonical helper and immediate source invalidation');
    f.cache.import();
    assert.equal(f.cache.table.get(old.slot), null);
    assert.equal(f.cache.units.has(old.slot), false);
  } finally {f.close();}
});

function processFixture(guest) {
  return fixture(undefined, {windowsMappings: true, moduleImage: guest, physicalPages: 1024,
    pc: guest.entry, registers: [0, 0x11223344, 0x22334455, 0x33445566, 0x7e004000, 0x44556677, 0x55667788, 0x66778899],
    execution: {budget: 1000}});
}

function processStop(f) {
  for (let count = 0; count < 32; count++) {
    const run = f.cache.execute(f.cache.select());
    if (![1, 2, 6].includes(run.coreReason)) return run;
  }
  assert.fail('the authored process must reach its next native or exception boundary');
}

function nativeCall(f, id) {
  const run = processStop(f);
  assert.deepEqual([run.reason, run.detail], [8, id]);
  f.engine.ok('cpu_capture_windows_call');
  const call = f.engine.take(112), token = view(call).getUint32(16, true);
  assert.equal(view(call).getUint32(20, true), id);
  f.engine.ok('cpu_complete_windows_call', token);
  return {call, token};
}

function processBanks(f) {
  return [[4236, 352], [4612, 184]].map(([offset, length]) =>
    new Uint8Array(f.engine.memory.buffer, f.engine.base + offset, length).slice());
}

function callbackReturnGate(engine) {
  for (let start = 0, total = 1; start < total;) {
    engine.ok('module_gates_page', start, 128);
    const page = engine.resultView(4096), count = page.getUint32(24, true);
    total = page.getUint32(16, true); assert(count > 0);
    for (let index = 0; index < count; index++)
      if (page.getUint32(36 + index * 8, true) === 0xffffe) return page.getUint32(32 + index * 8, true);
    start += count;
  }
  assert.fail('the loaded kernel32 module must own the canonical callback return gate');
}

test('actual independent NLS callback completes a nested native call and restores the suspended state banks', {skip}, () => {
  const b = builder([['kernel32.dll', ['SetLastError', 'EnumSystemLocalesEx', 'GetLastError']]],
    {callbacks: [{words: 3, result: 0, record: 'locale'}]});
  const nestedError = b.allocate('nestedError');
  b.external('SetLastError', 0x10002, [0x12345678]);
  b.external('EnumSystemLocalesEx', 0x150042, [b.callbacks[0].entry, 0x21, 0xabcdef, 0], 1);
  const guest = b.seal(), definition = guest.callbacks[0], file = 0x200 + definition.entry - guest.entry;
  const code = [...guest.bytes.subarray(file, file + definition.length - 8),
    0xd9, 0xe8, 0x66, 0x0f, 0x76, 0xc0,
    0xff, 0x15, ...dword(0x406408), 0xa3, ...dword(nestedError),
    0xbb, ...dword(0xdeadbeef), 0xbe, ...dword(0xcafebabe), 0xbf, ...dword(0xfedcba98),
    0x31, 0xc0, 0xc2, 12, 0];
  guest.bytes.set(code, file);
  const f = processFixture(guest);
  try {
    nativeCall(f, 0x10002);
    nativeCall(f, 0x150042);
    f.engine.ok('pending_windows_callback');
    const request = f.engine.resultView(96);
    assert.deepEqual([request.getUint32(12, true), request.getUint32(16, true), request.getUint32(20, true),
      request.getUint32(36, true), request.getUint32(40, true)], [7, definition.entry, 3, 0x21, 0xabcdef]);
    const scalar = new Uint8Array(f.engine.memory.buffer, f.engine.base, 56).slice(), banks = processBanks(f);
    const returnPc = callbackReturnGate(f.engine);
    f.engine.ok('cpu_begin_windows_callback', returnPc, 0xffffe);
    const callback = callbackRecord(f.engine.take(64), returnPc, 0xffffe);
    const nested = nativeCall(f, 0x10001);
    assert(nested.token > callback.token);
    assert.equal(f.engine.get(16), 0x12345678);
    assert.notDeepEqual(processBanks(f)[0], banks[0], 'the callback executed its x87 and SIMD instructions before the nested provider returned');
    const returned = processStop(f);
    assert.deepEqual([returned.reason, returned.pc, returned.detail, f.engine.get(32)], [8, returnPc, 0xffffe, callback.returnEsp]);
    assert.deepEqual([f.engine.get(28), f.engine.get(40), f.engine.get(44)], [0xdeadbeef, 0xcafebabe, 0xfedcba98]);
    f.engine.ok('read32', nestedError); assert.equal(f.engine.get(120), 0x12345678);
    f.engine.ok('cpu_finish_windows_callback', callback.token);
    const expected = view(scalar), continuation = guest.entry + guest.calls[1].end;
    expected.setUint32(16, 1, true); expected.setUint32(32, 0x7e004000, true); expected.setUint32(48, continuation, true);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, f.engine.base, 56), scalar);
    assert.deepEqual(processBanks(f), banks);
    f.engine.ok('pending_windows_callback'); assert.equal(f.engine.resultView(96).getUint32(12, true), 0);
    f.cache.budget = 1;
    const resumed = f.cache.execute(f.cache.select());
    assert.deepEqual([resumed.reason, resumed.retired, resumed.work, resumed.pc], [1, 1, 1, continuation + 5]);
    assert.deepEqual(processBanks(f), banks, 'the next independent import must retain the restored outer numerical banks');
    f.engine.ok('read32', guest.calls[1].status); assert.equal(f.engine.get(120), 1);
  } finally {f.close();}
});

test('actual independent DE handler returns through edited CONTEXT and restores the interrupted state banks', {skip}, () => {
  const b = builder([['kernel32.dll', ['AddVectoredExceptionHandler', 'ExitProcess']]],
    {callbacks: [{words: 1, result: 0xffffffff}]});
  const result = b.allocate('result');
  b.external('AddVectoredExceptionHandler', 0x120001, [1, b.callbacks[0].entry], 'handle');
  b.emit(0x31, 0xd2, 0xb8, 1, 0, 0, 0, 0x31, 0xc9, 0xf7, 0xf1);
  b.save(result); b.external('ExitProcess', 0x10003, [0], 'exit');
  const guest = b.seal(), handler = guest.callbacks[0], file = 0x200 + handler.entry - guest.entry;
  guest.bytes.set([0x8b, 0x44, 0x24, 4, 0x8b, 0x50, 4,
    0x83, 0x82, ...dword(184), 2, 0xc7, 0x82, ...dword(176), ...dword(42),
    0xd9, 0xe8, 0x66, 0x0f, 0x76, 0xc0, 0xbb, ...dword(0xdeadbeef),
    0xb8, ...dword(0xffffffff), 0xc2, 4, 0], file);
  const faultPc = guest.entry + guest.calls[0].end + 5 + 9;
  assert.deepEqual([...guest.bytes.subarray(0x200 + faultPc - guest.entry, 0x202 + faultPc - guest.entry)], [0xf7, 0xf1]);
  const f = processFixture(guest);
  try {
    nativeCall(f, 0x120001); assert(f.engine.get(16) > 0);
    const fault = processStop(f);
    assert.deepEqual([fault.reason, fault.pc, fault.detail], [10, faultPc, 0]);
    const scalar = new Uint8Array(f.engine.memory.buffer, f.engine.base, 56).slice(), banks = processBanks(f);
    assert.deepEqual(Array.from({length: 8}, (_, index) => f.engine.get(16 + index * 4)),
      [1, 0, 0, 0x33445566, 0x7e004000, 0x44556677, 0x55667788, 0x66778899]);
    f.engine.ok('cpu_dispatch_exception');
    f.engine.ok('pending_windows_exception');
    const delivery = f.engine.take(64), packet = view(delivery);
    assert.deepEqual([packet.getUint32(12, true), packet.getUint32(28, true), packet.getUint32(32, true)], [1, 1, handler.entry]);
    const record = packet.getUint32(36, true), context = packet.getUint32(40, true), returnPc = packet.getUint32(44, true), returnId = packet.getUint32(48, true);
    f.engine.ok('read32', record); assert.equal(f.engine.get(120), 0xc0000094);
    f.engine.ok('read32', context + 184); assert.equal(f.engine.get(120), faultPc);
    const returned = processStop(f);
    assert.deepEqual([returned.reason, returned.pc, returned.detail, f.engine.get(16)], [8, returnPc, returnId, 0xffffffff]);
    assert.notDeepEqual(processBanks(f)[0], banks[0], 'the actual handler changed its own numerical and integer state');
    f.engine.ok('read32', context + 176); assert.equal(f.engine.get(120), 42);
    f.engine.ok('read32', context + 184); assert.equal(f.engine.get(120), faultPc + 2);
    f.engine.ok('cpu_finish_exception');
    view(scalar).setUint32(16, 42, true); view(scalar).setUint32(48, faultPc + 2, true);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, f.engine.base, 56), scalar);
    assert.deepEqual(processBanks(f), banks);
    f.engine.ok('pending_windows_exception'); assert.equal(f.engine.resultView(64).getUint32(12, true), 0);
    nativeCall(f, 0x10003);
    f.engine.ok('read32', result); assert.equal(f.engine.get(120), 42);
    assert.equal(f.engine.get(72), 9);
    assert.deepEqual(processBanks(f), banks);
  } finally {f.close();}
});

test('actual pending FWAIT preserves its completed prefix and publishes the precise own MF event', {skip}, () => {
  const registers = [0xaabbccdd, 0x89abcdef, 0xd5e6789a, 0x23456789, 0x2f00, 0x55443322, 0x66554433, 0x77665544];
  const f = fixture([0xb8, 0x78, 0x56, 0x34, 0x12, 0x9b, 0x43], {
    registers, fpControl: 0x003e, fpStatus: 0x4201, execution: {budget: 2},
  });
  rejectJsService(f);
  const calls = memoryCalls(f);
  try {
    const scalar = new Uint8Array(f.engine.memory.buffer, f.engine.base, 56).slice(), banks = processBanks(f);
    view(scalar).setUint32(16, 0x12345678, true);
    view(scalar).setUint32(48, 0x1005, true);
    // the own service adds ES/B after the generated pending guard returns7.
    // C1/C3, CW, tags, raw ghosts, metadata and every other bank stay intact.
    view(banks[0]).setUint16(18, 0xc281, true);
    const selected = f.cache.select(), state = f.slab();
    const backing = state.getUint32(84, true), length = state.getUint32(88, true) * 4096;
    const ram = new Uint8Array(f.engine.memory.buffer, backing, length).slice();
    const run = f.cache.execute(selected);
    assert.deepEqual([run.coreReason, run.reason, run.retired, run.work, run.pc], [9, 12, 1, 1, 0x1005]);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, f.engine.base, 56), scalar);
    assert.deepEqual(processBanks(f), banks);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, backing, length), ram);
    assert.deepEqual([f.slab().getUint32(40, true), f.slab().getUint32(48, true), f.slab().getUint32(52, true),
      f.slab().getUint32(56, true), f.slab().getUint32(60, true), f.slab().getUint32(64, true), f.slab().getUint32(68, true)],
    [0x1005, 1, 9, 1, 0xc281, 0, 16]);
    assert.equal(f.engine.get(80), 0xc281);
    assert.deepEqual(calls, {read: 0, write: 0});
    serviceCounts(f, 1);
    event(f.engine, 16, 0x1005, 1);
  } finally {f.close();}
});

function seedInexactF32StoreBank(f, oldC1 = 0x200) {
  const bank = f.engine.base + 4236, fp = new DataView(f.engine.memory.buffer, bank, 128);
  fp.setUint16(16, 0x003f, true);
  fp.setUint16(18, 0x7d00 | oldC1, true); // top7, c0/c2/c3, fresh pe0.
  fp.setUint16(20, 0x3fff, true); // only physical7/st0 is occupied.
  fp.setUint16(22, 0x321, true);
  fp.setUint32(24, 0x12345678, true); fp.setUint32(28, 0x87654321, true);
  fp.setUint16(32, 0x71, true); fp.setUint16(34, 0x72, true);
  for (let index = 0; index < 8; index++) {
    fp.setBigUint64(40 + index * 10, 0x8000000000000000n + (BigInt(index) << 40n), true);
    fp.setUint16(48 + index * 10, 0x3fff, true);
  }
  // exactly representable f64 0x3ff0000030000000 = 1 + 3*2^-24.
  // nearest-even f32 is0x3f800002, PE1/C1=1; the source raw80 stays intact.
  fp.setBigUint64(110, 0x8000018000000000n, true);
}

test('actual inexact FSTP publishes the rounded FS store raw ghosts and immediate PE C1 after its prefix', {skip}, () => {
  const registers = [0xaabbccdd, 0x89abcdef, 0xd5e6789a, 0x23456789, 0x2f00, 0x55443322, 0x66554433, 0x77665544];
  const f = fixture([0xb8, 0x78, 0x56, 0x34, 0x12, 0x64, 0xd9, 0x1d, ...dword(0x1004), 0x43], {
    registers, data: dword(0xdeadbeef).map((value, index) => [0x2004 + index, value]), execution: {budget: 2},
  });
  rejectJsService(f);
  try {
    f.engine.ok('set_segment_base', 4, 0x1000);
    seedInexactF32StoreBank(f);
    const scalar = new Uint8Array(f.engine.memory.buffer, f.engine.base, 56).slice(), banks = processBanks(f);
    view(scalar).setUint32(16, 0x12345678, true); view(scalar).setUint32(48, 0x100c, true);
    const fp = view(banks[0]);
    fp.setUint16(18, 0x4720, true); fp.setUint16(20, 0xffff, true); fp.setUint16(22, 0x11d, true);
    fp.setUint32(24, 0x1005, true); fp.setUint32(28, 0x1004, true);
    fp.setUint16(32, 0x1b, true); fp.setUint16(34, 0x3b, true);
    const selected = f.cache.select(), state = f.slab();
    const backing = state.getUint32(84, true), length = state.getUint32(88, true) * 4096;
    const ram = new Uint8Array(f.engine.memory.buffer, backing, length).slice();
    const slot = new DataView(f.engine.memory.buffer).getUint32(state.getUint32(76, true) + 2 * 4, true) - 1;
    new DataView(ram.buffer).setUint32(slot * 4096 + 4, 0x3f800002, true);
    const run = f.cache.execute(selected);
    assert.deepEqual([run.coreReason, run.reason, run.retired, run.work, run.pc], [2, 1, 2, 2, 0x100c]);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, f.engine.base, 56), scalar);
    assert.deepEqual(processBanks(f), banks);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, backing, length), ram);
    assert.equal(f.slab().getUint32(48, true), 0);
    serviceCounts(f, 0);
  } finally {f.close();}
});

test('actual inexact FSTP late readonly-page fault retains PE C1 raw ghosts and metadata before its pop', {skip}, () => {
  for (const oldC1 of [0, 0x200]) {
    const f = fixture([0xb8, 0x78, 0x56, 0x34, 0x12, 0xd9, 0x1d, ...dword(0x2ffe), 0x43], {
      data: [[0x2ffe, 0x5a], [0x2fff, 0xa5]], execution: {budget: 2},
    });
    rejectJsService(f);
    const calls = memoryCalls(f);
    try {
      f.engine.ok('map', 0x3000, 1, 3);
      f.engine.ok('write8', 0x3000, 0x66); f.engine.ok('write8', 0x3001, 0x99);
      f.engine.ok('protect', 0x3000, 1, 1);
      seedInexactF32StoreBank(f, oldC1);
      const scalar = new Uint8Array(f.engine.memory.buffer, f.engine.base, 56).slice(), banks = processBanks(f);
      view(scalar).setUint32(16, 0x12345678, true); view(scalar).setUint32(48, 0x1005, true);
      const selected = f.cache.select(), state = f.slab();
      const backing = state.getUint32(84, true), length = state.getUint32(88, true) * 4096;
      const ram = new Uint8Array(f.engine.memory.buffer, backing, length).slice(), run = f.cache.execute(selected);
      assert.deepEqual([run.coreReason, run.reason, run.retired, run.work, run.pc], [3, 5, 1, 1, 0x1005]);
      assert.deepEqual([f.engine.get(80), f.engine.get(84), f.engine.get(88), f.engine.get(92)], [2, 0x3000, 2, 4]);
      assert.deepEqual(new Uint8Array(f.engine.memory.buffer, f.engine.base, 56), scalar);
      assert.deepEqual(processBanks(f), banks);
      assert.deepEqual(new Uint8Array(f.engine.memory.buffer, backing, length), ram);
      assert.equal(f.slab().getUint32(48, true), 1);
      event(f.engine, 0xffffffff, 0x1005, 1);
      serviceCounts(f, 0);
      assert(calls.write > 0, 'the full cross-page permission check retains the actual canonical write helper');
    } finally {f.close();}
  }
});

test('actual inexact FSTP executable-alias SMC commits PE C1 pop and metadata before leaving its prefix', {skip}, () => {
  const f = fixture(undefined, {windowsMappings: true, physicalPages: 1024, execution: {budget: 2}});
  rejectJsService(f);
  try {
    const [writable, executable] = sharedViews(f, 0x40, [2, 0x24]);
    f.engine.request(Uint8Array.from([0xb8, 1, 0, 0, 0, 0xeb, 0xf9])); f.engine.ok('upload', writable, 7);
    new DataView(f.engine.memory.buffer).setUint32(f.engine.base + 48, executable, true);
    const old = f.cache.select(); assert.equal(f.cache.execute(old).reason, 1);
    replaceCode(f, [0xb8, 0x78, 0x56, 0x34, 0x12, 0xd9, 0x1d, ...dword(writable), 0x43]);
    new DataView(f.engine.memory.buffer).setUint32(f.engine.base + 48, 0x1000, true);
    seedInexactF32StoreBank(f);
    const scalar = new Uint8Array(f.engine.memory.buffer, f.engine.base, 56).slice(), banks = processBanks(f);
    view(scalar).setUint32(16, 0x12345678, true); view(scalar).setUint32(48, 0x100b, true);
    const fp = view(banks[0]);
    fp.setUint16(18, 0x4720, true); fp.setUint16(20, 0xffff, true); fp.setUint16(22, 0x11d, true);
    fp.setUint32(24, 0x1005, true); fp.setUint32(28, writable, true);
    fp.setUint16(32, 0x1b, true); fp.setUint16(34, 0x23, true);
    const selected = f.cache.select(), state = f.slab();
    const backing = state.getUint32(84, true), length = state.getUint32(88, true) * 4096;
    const slot = new DataView(f.engine.memory.buffer).getUint32(state.getUint32(76, true) + (writable >>> 12) * 4, true) - 1;
    const ram = new Uint8Array(f.engine.memory.buffer, backing, length).slice();
    new DataView(ram.buffer).setUint32(slot * 4096 + (writable & 4095), 0x3f800002, true);
    const run = f.cache.execute(selected);
    assert.deepEqual([run.coreReason, run.reason, run.retired, run.work, run.pc], [6, 6, 2, 2, 0x100b]);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, f.engine.base, 56), scalar);
    assert.deepEqual(processBanks(f), banks);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, backing, length), ram);
    assert.equal(f.slab().getUint32(48, true), 0);
    f.cache.import(); assert.equal(f.cache.table.get(old.slot), null);
    serviceCounts(f, 0);
  } finally {f.close();}
});

function seedWideCompareBank(f, {empty = false, pending = false} = {}) {
  const fp = new DataView(f.engine.memory.buffer, f.engine.base + 4236, 128);
  fp.setUint16(16, pending ? 0x003e : 0x003f, true);
  fp.setUint16(18, pending ? 0x7f21 : 0x7f20, true); // top7, all conditions, sticky pe.
  fp.setUint16(20, empty ? 0xffff : 0x3fff, true);
  fp.setUint16(22, 0x321, true);
  fp.setUint32(24, 0x12345678, true); fp.setUint32(28, 0x87654321, true);
  fp.setUint16(32, 0x71, true); fp.setUint16(34, 0x72, true);
  for (let index = 0; index < 8; index++) {
    fp.setBigUint64(40 + index * 10, 0x8000000000000000n + (BigInt(index) << 40n), true);
    fp.setUint16(48 + index * 10, 0x3fff, true);
  }
  // st0 is1+2^-63, strictly greater than1; narrowing it to f64 would lose bit0.
  fp.setBigUint64(110, 0x8000000000000001n, true);
  new DataView(f.engine.memory.buffer).setUint32(f.engine.base + 52, 0xcd7, true);
}

test('actual legacy m64 FCOMP orders the full raw80 operand and preserves flags at its prefix budget boundary', {skip}, () => {
  const registers = [0xaabbccdd, 0x89abcdef, 0xd5e6789a, 0x23456789, 0x2f00, 0x55443322, 0x66554433, 0x77665544];
  const f = fixture([0xb8, 0x78, 0x56, 0x34, 0x12, 0x64, 0xdc, 0x1d, ...dword(0x1004), 0x43], {
    registers, data: [...dword(0), ...dword(0x3ff00000)].map((value, index) => [0x2004 + index, value]),
    execution: {budget: 2},
  });
  rejectJsService(f);
  const calls = memoryCalls(f);
  try {
    f.engine.ok('set_segment_base', 4, 0x1000);
    seedWideCompareBank(f);
    const scalar = new Uint8Array(f.engine.memory.buffer, f.engine.base, 56).slice(), banks = processBanks(f);
    view(scalar).setUint32(16, 0x12345678, true); view(scalar).setUint32(48, 0x100c, true);
    const fp = view(banks[0]);
    fp.setUint16(18, 0x0020, true); // greater clears C0/C1/C2/C3; PE remains, TOP wraps to0.
    fp.setUint16(20, 0xffff, true); fp.setUint16(22, 0x41d, true);
    fp.setUint32(24, 0x1005, true); fp.setUint32(28, 0x1004, true);
    fp.setUint16(32, 0x1b, true); fp.setUint16(34, 0x3b, true);
    const selected = f.cache.select(), state = f.slab();
    const backing = state.getUint32(84, true), length = state.getUint32(88, true) * 4096;
    const ram = new Uint8Array(f.engine.memory.buffer, backing, length).slice(), run = f.cache.execute(selected);
    assert.deepEqual([run.coreReason, run.reason, run.retired, run.work, run.pc], [2, 1, 2, 2, 0x100c]);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, f.engine.base, 56), scalar);
    assert.deepEqual(processBanks(f), banks);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, backing, length), ram);
    assert.equal(f.slab().getUint32(48, true), 0);
    assert.deepEqual(calls, {read: 0, write: 0});
    serviceCounts(f, 0);
  } finally {f.close();}
});

test('actual m64 comparison preserves whole-operand faults before empty-stack effects and pending MF priority', {skip}, () => {
  for (const pending of [false, true]) {
    const registers = [0xaabbccdd, 0x89abcdef, 0xd5e6789a, 0x23456789, 0x2f00, 0x55443322, 0x66554433, 0x77665544];
    const f = fixture([0xb8, 0x78, 0x56, 0x34, 0x12, 0xdc, 0x1d, ...dword(0x2ffc), 0x43], {
      registers, data: dword(0x11223344).map((value, index) => [0x2ffc + index, value]), execution: {budget: 2},
    });
    rejectJsService(f);
    const calls = memoryCalls(f);
    try {
      seedWideCompareBank(f, {empty: true, pending});
      const scalar = new Uint8Array(f.engine.memory.buffer, f.engine.base, 56).slice(), banks = processBanks(f);
      view(scalar).setUint32(16, 0x12345678, true); view(scalar).setUint32(48, 0x1005, true);
      if (pending) view(banks[0]).setUint16(18, 0xffa1, true); // only the own pending-MF service adds ES/B.
      const selected = f.cache.select(), state = f.slab();
      const backing = state.getUint32(84, true), length = state.getUint32(88, true) * 4096;
      const ram = new Uint8Array(f.engine.memory.buffer, backing, length).slice(), run = f.cache.execute(selected);
      assert.deepEqual([run.coreReason, run.reason, run.retired, run.work, run.pc],
        [pending ? 9 : 3, pending ? 12 : 5, 1, 1, 0x1005]);
      if (pending) assert.equal(f.engine.get(80), 0xffa1);
      else assert.deepEqual([f.engine.get(80), f.engine.get(84), f.engine.get(88), f.engine.get(92)], [1, 0x3000, 1, 8]);
      assert.deepEqual(new Uint8Array(f.engine.memory.buffer, f.engine.base, 56), scalar);
      assert.deepEqual(processBanks(f), banks);
      assert.deepEqual(new Uint8Array(f.engine.memory.buffer, backing, length), ram);
      assert.equal(f.slab().getUint32(48, true), 1);
      assert.deepEqual(calls, {read: 0, write: 0}, 'm64 remains one whole own-service operand, with no split cpu.read imports');
      event(f.engine, pending ? 16 : 0xffffffff, 0x1005, 1);
      // cross page m64 is deliberately outside read_words' same-page domain.
      // both artifacts retain exactly one own service; only the positive test
      // above has a new zero-service admission gate.
      serviceCounts(f, 1);
    } finally {f.close();}
  }
});

function multiplyRaw64(bits) {
  const exponent = Number(bits >> 52n & 0x7ffn), fraction = bits & 0xfffffffffffffn;
  const sign = Number(bits >> 48n & 0x8000n);
  assert(exponent < 0x7ff && (exponent !== 0 || fraction === 0n), 'test payload is a finite normal binary64 or signed zero');
  return {sig: exponent ? (0x10000000000000n | fraction) << 11n : 0n, exp: exponent ? (exponent + 15360) | sign : sign};
}

function seedMultiplyResidualBank(f, left, {control, status, top, tag = 0}) {
  const fp = new DataView(f.engine.memory.buffer, f.engine.base + 4236, 128);
  fp.setUint16(16, control, true); fp.setUint16(18, status | top << 11, true);
  fp.setUint16(20, (0xffff & ~(3 << (top * 2))) | tag << (top * 2), true);
  fp.setUint16(22, 0x321, true); fp.setUint32(24, 0x12345678, true); fp.setUint32(28, 0x87654321, true);
  fp.setUint16(32, 0x71, true); fp.setUint16(34, 0x72, true);
  for (let index = 0; index < 8; index++) {
    fp.setBigUint64(40 + index * 10, 0x8000000000000000n + (BigInt(index) << 40n), true);
    fp.setUint16(48 + index * 10, 0x3fff, true);
  }
  fp.setBigUint64(40 + top * 10, left.sig, true); fp.setUint16(48 + top * 10, left.exp, true);
  new DataView(f.engine.memory.buffer).setUint32(f.engine.base + 52, 0xcd7, true);
}

const multiplyResidualCases = [
  {name: 'exact PC53 product', left: 0x4008000000000000n, right: 0x40000000, result: 0x4018000000000000n, inexact: false, incremented: false},
  {name: 'negative residual', left: 0x3ff0000000000001n, right: 0x3f7fffff, result: 0x3fefffffe0000002n, inexact: true, incremented: true},
  {name: 'positive residual', left: 0x3ff0000000000001n, right: 0x3f800001, result: 0x3ff0000020000001n, inexact: true, incremented: false},
  {name: 'odd halfway product', left: 0x3ff0000000000001n, right: 0x3fc00000, result: 0x3ff8000000000002n, inexact: true, incremented: true},
  {name: 'even halfway product', left: 0x3ff0000000000003n, right: 0x3fc00000, result: 0x3ff8000000000004n, inexact: true, incremented: false},
  {name: 'exponent carry', left: 0x3ff5555555555555n, right: 0x3fc00000, result: 0x4000000000000000n, inexact: true, incremented: true},
  {name: 'minimum positive residual 2^-651', left: 0x23d0000000000001n, right: 0x00800001, result: 0x1bf0000020000001n, inexact: true, incremented: false},
  {name: 'minimum negative residual -2^-651', left: 0x23d0000000000001n, right: 0x00ffffff, result: 0x1bffffffe0000002n, inexact: true, incremented: true},
  {name: 'maximum admitted exponent', left: 0x5c10000000000001n, right: 0x7f7fffff, result: 0x640fffffe0000002n, inexact: true, incremented: true},
];

function multiplyResidualOracle(c, {negativeLeft = false, negativeRight = false, top = 7, control = 0x023f, status = 0x4700, tag = 0} = {}) {
  const registers = [0xaabbccdd, 0x89abcdef, 0xd5e6789a, 0x23456789, 0x2f00, 0x55443322, 0x66554433, 0x77665544];
  const right = c.right | (negativeRight ? 0x80000000 : 0);
  const right64 = c.right64 === undefined ? undefined : c.right64 | (negativeRight ? 1n << 63n : 0n);
  const data = right64 === undefined ? dword(right) : [...dword(Number(right64 & 0xffffffffn)), ...dword(Number(right64 >> 32n))];
  const f = fixture([0xb8, 0x78, 0x56, 0x34, 0x12, 0x64, c.right64 === undefined ? 0xd8 : 0xdc, 0x0d, ...dword(0x1004), 0x43], {
    registers, data: data.map((value, index) => [0x2004 + index, value]), execution: {budget: 2},
  });
  rejectJsService(f);
  const calls = memoryCalls(f);
  try {
    f.engine.ok('set_segment_base', 4, 0x1000);
    const left = c.rawLeft ? {...c.rawLeft} : multiplyRaw64(c.left);
    left.exp ^= negativeLeft ? 0x8000 : 0;
    seedMultiplyResidualBank(f, left, {control, status, top, tag});
    const scalar = new Uint8Array(f.engine.memory.buffer, f.engine.base, 56).slice(), banks = processBanks(f), fp = view(banks[0]);
    view(scalar).setUint32(16, 0x12345678, true); view(scalar).setUint32(48, 0x100c, true);
    const result = multiplyRaw64(c.result | (negativeLeft !== negativeRight ? 1n << 63n : 0n));
    fp.setUint16(18, (fp.getUint16(18, true) & ~0x200) | (c.inexact ? 0x20 : 0) | (c.incremented ? 0x200 : 0), true);
    fp.setUint16(20, (fp.getUint16(20, true) & ~(3 << (top * 2))) | (result.sig === 0n ? 1 << (top * 2) : 0), true);
    fp.setUint16(22, c.right64 === undefined ? 0x00d : 0x40d, true);
    fp.setUint32(24, 0x1005, true); fp.setUint32(28, 0x1004, true); fp.setUint16(32, 0x1b, true); fp.setUint16(34, 0x3b, true);
    fp.setBigUint64(40 + top * 10, result.sig, true); fp.setUint16(48 + top * 10, result.exp, true);
    const selected = f.cache.select(), state = f.slab();
    const backing = state.getUint32(84, true), length = state.getUint32(88, true) * 4096;
    const ram = new Uint8Array(f.engine.memory.buffer, backing, length).slice(), run = f.cache.execute(selected);
    assert.deepEqual([run.coreReason, run.reason, run.retired, run.work, run.pc], [2, 1, 2, 2, 0x100c]);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, f.engine.base, 56), scalar);
    assert.deepEqual(processBanks(f), banks);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, backing, length), ram);
    assert.equal(f.slab().getUint32(48, true), 0);
    assert.deepEqual(calls, {read: 0, write: 0});
    // the existing u128 implementation also admits these inputs. this gate
    // proves compatibility, not hardware-path execution or a performance gain.
    serviceCounts(f, c.services ?? 0);
  } finally {f.close();}
}

for (const [index, c] of multiplyResidualCases.entries()) test(`actual multiply residual oracle ${c.name} preserves every bank and source byte`, {skip}, () => {
  for (let signs = 0; signs < 4; signs++) multiplyResidualOracle(c, {
    negativeLeft: !!(signs & 1), negativeRight: !!(signs & 2), top: (index * 4 + signs) & 7,
    control: (index + signs) & 1 ? 0x027f : 0x023f,
    status: 0x4500 | (signs & 1 ? 0x200 : 0) | (signs & 2 ? 0x20 : 0) | (signs === 3 ? 0x8080 : 0), tag: (index + signs) % 3,
  });
});

for (const c of [
  {name: 'lower exponent outside hardware predicate', left: 0x23c0000000000001n, right: 0x00800001, result: 0x1be0000020000001n, inexact: true, incremented: false},
  {name: 'upper exponent outside hardware predicate', left: 0x5c20000000000001n, right: 0x7f7fffff, result: 0x641fffffe0000002n, inexact: true, incremented: true},
  {name: 'raw80 low bit outside binary64 precision', rawLeft: {sig: 0x8000000000000001n, exp: 0x3fff}, right: 0x3f800000, result: 0x3ff0000000000000n, inexact: true, incremented: false},
  {name: 'full raw80 significand and exponent carry', rawLeft: {sig: 0xffffffffffffffffn, exp: 0x3fff}, right: 0x3f800000, result: 0x4000000000000000n, inexact: true, incremented: true},
  {name: 'PC24 existing rounding', left: 0x3ff0000000000001n, right: 0x3f800001, result: 0x3ff0000020000000n, inexact: true, incremented: false, control: 0x003f},
  {name: 'memory64 existing u128 path', left: 0x3ff0000000000001n, right64: 0x3ff0000020000000n, result: 0x3ff0000020000001n, inexact: true, incremented: false},
  {name: 'signed zero existing path', left: 0n, right: 0x3f800000, result: 0n, inexact: false, incremented: false},
  {name: 'unsupported CW existing service', left: 0x4008000000000000n, right: 0x40000000, result: 0x4018000000000000n, inexact: false, incremented: false, control: 0x033f, services: 1},
]) test(`actual multiply residual fallback ${c.name} retains its previous admission`, {skip}, () => {
  multiplyResidualOracle(c, {negativeLeft: true, control: c.control ?? 0x023f});
});

test('actual multiply residual fault and pending MF preserve the prefix full banks and original raw operand', {skip}, () => {
  for (const pending of [false, true]) {
    const f = fixture([0xb8, 0x78, 0x56, 0x34, 0x12, 0xd8, 0x0d, ...dword(0x2ffe), 0x43], {
      data: [[0x2ffe, 0x5a], [0x2fff, 0xa5]], execution: {budget: 2},
    });
    rejectJsService(f);
    const calls = memoryCalls(f);
    try {
      seedMultiplyResidualBank(f, multiplyRaw64(0x3ff0000000000001n), {control: pending ? 0x023e : 0x023f, status: pending ? 0x4701 : 0x4700, top: 7});
      const scalar = new Uint8Array(f.engine.memory.buffer, f.engine.base, 56).slice(), banks = processBanks(f);
      view(scalar).setUint32(16, 0x12345678, true); view(scalar).setUint32(48, 0x1005, true);
      if (pending) view(banks[0]).setUint16(18, 0xff81, true);
      const selected = f.cache.select(), state = f.slab();
      const backing = state.getUint32(84, true), length = state.getUint32(88, true) * 4096;
      const ram = new Uint8Array(f.engine.memory.buffer, backing, length).slice(), run = f.cache.execute(selected);
      assert.deepEqual([run.coreReason, run.reason, run.retired, run.work, run.pc], [pending ? 9 : 3, pending ? 12 : 5, 1, 1, 0x1005]);
      if (pending) assert.equal(f.engine.get(80), 0xff81);
      else assert.deepEqual([f.engine.get(80), f.engine.get(84), f.engine.get(88), f.engine.get(92)], [1, 0x3000, 1, 4]);
      assert.deepEqual(new Uint8Array(f.engine.memory.buffer, f.engine.base, 56), scalar);
      assert.deepEqual(processBanks(f), banks);
      assert.deepEqual(new Uint8Array(f.engine.memory.buffer, backing, length), ram);
      assert.equal(f.slab().getUint32(48, true), 1);
      assert.equal(calls.write, 0);
      assert.equal(calls.read, pending ? 0 : 1);
      event(f.engine, pending ? 16 : 0xffffffff, 0x1005, 1);
      serviceCounts(f, pending ? 1 : 0);
    } finally {f.close();}
  }
});

// this encodes fixture values, without computing a product or rounding oracle.
function pc24Normal32As64Bits(bits) {
  const word = bits >>> 0, exponent = word >>> 23 & 0xff, fraction = word & 0x7fffff;
  assert(exponent < 0xff && (exponent !== 0 || fraction === 0), 'fixture is normal binary32 or signed zero');
  const sign = BigInt(word >>> 31) << 63n;
  return exponent ? sign | BigInt(exponent + 896) << 52n | BigInt(fraction) << 29n : sign;
}

const pc24NormalProductCases = [
  {name: 'exact ordinary', left32: 0x40400000, right: 0x40000000, result32: 0x40c00000, inexact: false, incremented: false},
  {name: 'odd lower tie', left32: 0x3f800001, right: 0x3fc00000, result32: 0x3fc00002, inexact: true, incremented: true},
  {name: 'even lower tie', left32: 0x3f800003, right: 0x3fc00000, result32: 0x3fc00004, inexact: true, incremented: false},
  {name: 'exponent carry', left32: 0x3f800001, right: 0x3ffffffe, result32: 0x40000000, inexact: true, incremented: true},
  {name: 'minimum normal exact', left32: 0x00800000, right: 0x3f800000, result32: 0x00800000, inexact: false, incremented: false},
  {name: 'minimum normal neighborhood inexact', left32: 0x00800001, right: 0x3f800001, result32: 0x00800002, inexact: true, incremented: false},
  {name: 'maximum finite exact', left32: 0x7f7fffff, right: 0x3f800000, result32: 0x7f7fffff, inexact: false, incremented: false},
  {name: 'maximum finite neighborhood inexact', left32: 0x7f7fffff, right: 0x3f7fffff, result32: 0x7f7ffffe, inexact: true, incremented: false},
];

for (const [index, row] of pc24NormalProductCases.entries()) test(`actual PC24 normal product ${row.name} preserves full banks RAM and literal rounding`, {skip}, () => {
  const c = {...row, left: pc24Normal32As64Bits(row.left32), result: pc24Normal32As64Bits(row.result32)};
  for (let signs = 0; signs < 4; signs++) multiplyResidualOracle(c, {
    negativeLeft: !!(signs & 1), negativeRight: !!(signs & 2), top: (index * 4 + signs) & 7,
    control: (index + signs) & 1 ? 0x007f : 0x003f,
    status: 0x4500 | (signs & 1 ? 0x200 : 0) | (signs & 2 ? 0x20 : 0) | (signs === 3 ? 0x8080 : 0),
    tag: (index + signs) % 3,
  });
});

const pc24NormalProductFallbacks = [
  // these extended-exponent results distinguish pc24 from binary32 arithmetic.
  {name: 'below binary32 normal bound remains exact extended normal', left: pc24Normal32As64Bits(0x00800000), right: 0x3f7fffff, result: 0x380fffffe0000000n, inexact: false, incremented: false},
  {name: 'above binary32 maximum remains finite extended normal', left: pc24Normal32As64Bits(0x7f7fffff), right: 0x3f800001, result: 0x47f0000000000000n, inexact: true, incremented: false},
  {name: 'minimum normal squared retains extended exponent', left: pc24Normal32As64Bits(0x00800000), right: 0x00800000, result: 0x3030000000000000n, inexact: false, incremented: false},
  {name: 'maximum finite squared retains extended exponent', left: pc24Normal32As64Bits(0x7f7fffff), right: 0x7f7fffff, result: 0x4fefffffc0000000n, inexact: true, incremented: false},
  {name: 'exact binary64 outside binary32 precision retains discarded bit', rawLeft: {sig: 0x8000000000800000n, exp: 0x3fff}, right: 0x3f800000, result: 0x3ff0000000000000n, inexact: true, incremented: false},
  {name: 'PC53 retains unrounded 48 bit product', left: pc24Normal32As64Bits(0x3f800001), right: 0x3fc00000, result: 0x3ff8000030000000n, inexact: false, incremented: false, control: 0x027f},
  {name: 'PC64 retains existing own service frontier', left: pc24Normal32As64Bits(0x3f800001), right: 0x3fc00000, result: 0x3ff8000030000000n, inexact: false, incremented: false, control: 0x037f, services: 1},
  {name: 'signed zero retains sign XOR', left: 0n, right: 0x3f800000, result: 0n, inexact: false, incremented: false},
  {name: 'memory64 retains its existing PC24 path', left: pc24Normal32As64Bits(0x3f800001), right64: pc24Normal32As64Bits(0x3fc00000), result: pc24Normal32As64Bits(0x3fc00002), inexact: true, incremented: true},
  {name: 'non nearest control retains existing own service frontier', left: pc24Normal32As64Bits(0x40400000), right: 0x40000000, result: pc24Normal32As64Bits(0x40c00000), inexact: false, incremented: false, control: 0x087f, services: 1},
];

for (const [index, c] of pc24NormalProductFallbacks.entries()) test(`actual PC24 normal product fallback ${c.name}`, {skip}, () => {
  multiplyResidualOracle(c, {
    negativeLeft: true, top: index & 7, control: c.control ?? (index & 1 ? 0x007f : 0x003f),
    status: 0x4700, tag: index % 3,
  });
});

test('actual PC24 normal product fault and pending MF preserve prefix effects full banks and source bytes', {skip}, () => {
  for (const reserved of [0, 0x40]) for (const pending of [false, true]) {
    const f = fixture([0xb8, 0x78, 0x56, 0x34, 0x12, 0xd8, 0x0d, ...dword(0x2ffe), 0x43], {
      data: [[0x2ffe, 0x5a], [0x2fff, 0xa5]], execution: {budget: 2},
    });
    rejectJsService(f);
    const calls = memoryCalls(f);
    try {
      seedMultiplyResidualBank(f, multiplyRaw64(pc24Normal32As64Bits(0x3f800001)), {
        control: (pending ? 0x003e : 0x003f) | reserved, status: pending ? 0x4701 : 0x4700, top: 7,
      });
      const scalar = new Uint8Array(f.engine.memory.buffer, f.engine.base, 56).slice(), banks = processBanks(f);
      view(scalar).setUint32(16, 0x12345678, true); view(scalar).setUint32(48, 0x1005, true);
      if (pending) view(banks[0]).setUint16(18, 0xff81, true);
      const selected = f.cache.select(), state = f.slab();
      const backing = state.getUint32(84, true), length = state.getUint32(88, true) * 4096;
      const ram = new Uint8Array(f.engine.memory.buffer, backing, length).slice(), run = f.cache.execute(selected);
      assert.deepEqual([run.coreReason, run.reason, run.retired, run.work, run.pc], [pending ? 9 : 3, pending ? 12 : 5, 1, 1, 0x1005]);
      if (pending) assert.equal(f.engine.get(80), 0xff81);
      else assert.deepEqual([f.engine.get(80), f.engine.get(84), f.engine.get(88), f.engine.get(92)], [1, 0x3000, 1, 4]);
      assert.deepEqual(new Uint8Array(f.engine.memory.buffer, f.engine.base, 56), scalar);
      assert.deepEqual(processBanks(f), banks);
      assert.deepEqual(new Uint8Array(f.engine.memory.buffer, backing, length), ram);
      assert.equal(f.slab().getUint32(48, true), 1);
      assert.deepEqual(calls, {read: pending ? 0 : 1, write: 0});
      event(f.engine, pending ? 16 : 0xffffffff, 0x1005, 1);
      serviceCounts(f, pending ? 1 : 0);
    } finally {f.close();}
  }
});

test('actual PC24 normal product quota stops before a pending exception or invalid operand', {skip}, () => {
  for (const pending of [false, true]) {
    const f = fixture([0xb8, 0x78, 0x56, 0x34, 0x12, 0xd8, 0x0d, ...dword(0x2ffe), 0x43], {
      data: [[0x2ffe, 0x5a], [0x2fff, 0xa5]], execution: {budget: 1},
    });
    rejectJsService(f);
    const calls = memoryCalls(f);
    try {
      seedMultiplyResidualBank(f, multiplyRaw64(pc24Normal32As64Bits(0x3f800001)), {
        control: pending ? 0x007e : 0x003f, status: pending ? 0x4701 : 0x4700, top: 7,
      });
      const scalar = new Uint8Array(f.engine.memory.buffer, f.engine.base, 56).slice(), banks = processBanks(f);
      view(scalar).setUint32(16, 0x12345678, true); view(scalar).setUint32(48, 0x1005, true);
      const selected = f.cache.select(), state = f.slab();
      const backing = state.getUint32(84, true), length = state.getUint32(88, true) * 4096;
      const ram = new Uint8Array(f.engine.memory.buffer, backing, length).slice(), run = f.cache.execute(selected);
      assert.deepEqual([run.coreReason, run.reason, run.retired, run.work, run.pc], [2, 1, 1, 1, 0x1005]);
      assert.deepEqual(new Uint8Array(f.engine.memory.buffer, f.engine.base, 56), scalar);
      assert.deepEqual(processBanks(f), banks);
      assert.deepEqual(new Uint8Array(f.engine.memory.buffer, backing, length), ram);
      assert.equal(f.slab().getUint32(48, true), 0);
      assert.deepEqual(calls, {read: 0, write: 0});
      serviceCounts(f, 0);
    } finally {f.close();}
  }
});

const fchsNormalBaselineServices = 0;

function fchsLiteralOracle(c, {negative = false, count = 1, services = count * fchsNormalBaselineServices} = {}) {
  const registers = [0xaabbccdd, 0x89abcdef, 0xd5e6789a, 0x23456789, 0x2f00, 0x55443322, 0x66554433, 0x77665544];
  const f = fixture([0xb8, 0x78, 0x56, 0x34, 0x12, ...Array.from({length: count}, () => [0xd9, 0xe0]).flat(), 0x43], {
    registers, execution: {budget: count + 1},
  });
  rejectJsService(f);
  const calls = memoryCalls(f);
  try {
    const left = {sig: c.sig, exp: c.exp ^ (negative ? 0x8000 : 0)};
    seedMultiplyResidualBank(f, left, {control: c.control, status: c.status, top: c.top, tag: c.tag});
    const scalar = new Uint8Array(f.engine.memory.buffer, f.engine.base, 56).slice(), banks = processBanks(f), fp = view(banks[0]);
    view(scalar).setUint32(16, 0x12345678, true); view(scalar).setUint32(48, 0x1005 + count * 2, true);
    fp.setUint16(18, fp.getUint16(18, true) & ~0x200, true);
    fp.setUint16(20, (fp.getUint16(20, true) & ~(3 << (c.top * 2))) | c.resultTag << (c.top * 2), true);
    fp.setUint16(22, 0x1e0, true); fp.setUint32(24, 0x1005 + (count - 1) * 2, true); fp.setUint16(32, 0x1b, true);
    fp.setUint16(48 + c.top * 10, left.exp ^ (count & 1 ? 0x8000 : 0), true);
    const selected = f.cache.select(), state = f.slab();
    const backing = state.getUint32(84, true), length = state.getUint32(88, true) * 4096;
    const ram = new Uint8Array(f.engine.memory.buffer, backing, length).slice();
    let unit = selected, run, retired = 0, work = 0;
    for (let attempts = 0; attempts < 4; attempts++) {
      run = f.cache.execute(unit); retired += run.retired; work += run.work;
      if (run.coreReason !== 1) break;
      unit = f.cache.select();
    }
    assert.deepEqual([run.coreReason, run.reason, retired, work, run.pc], [2, 1, count + 1, count + 1, 0x1005 + count * 2]);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, f.engine.base, 56), scalar);
    assert.deepEqual(processBanks(f), banks);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, backing, length), ram);
    assert.equal(f.slab().getUint32(48, true), 0);
    assert.deepEqual(calls, {read: 0, write: 0});
    serviceCounts(f, services);
  } finally {f.close();}
}

const fchsLiteralCases = [
  {name: 'wide normal with every exception unmasked', sig: 0x8000000000000001n, exp: 0x3fff, control: 0x0000, status: 0x4700, top: 0, tag: 2, resultTag: 0},
  {name: 'maximum normal raw exponent and all sticky bits', sig: 0xffffffffffffffffn, exp: 0x7ffe, control: 0xffff, status: 0xc7ff, top: 7, tag: 1, resultTag: 0},
  {name: 'minimum normal raw exponent under directed precision64', sig: 0x8000000000000001n, exp: 0x0001, control: 0x0b7f, status: 0x4720, top: 3, tag: 1, resultTag: 0},
  {name: 'zero retagged under default precision64', sig: 0n, exp: 0, control: 0x037f, status: 0x4720, top: 6, tag: 0, resultTag: 1},
  {name: 'zero with unmasked directed control', sig: 0n, exp: 0, control: 0x0840, status: 0x4700, top: 5, tag: 2, resultTag: 1},
  {name: 'normal under actual precision24 control', sig: 0x8000000000000000n, exp: 0x3fff, control: 0x007f, status: 0x4720, top: 1, tag: 1, resultTag: 0},
  {name: 'all raw significand bits under precision53', sig: 0xf23456789abcdef0n, exp: 0x4001, control: 0x023f, status: 0x4700, top: 2, tag: 2, resultTag: 0},
  {name: 'unmasked invalid with only masked PE sticky', sig: 0x8000000000000001n, exp: 0x3fff, control: 0x0c7e, status: 0x4720, top: 4, tag: 1, resultTag: 0},
];

for (const c of fchsLiteralCases) test(`actual FCHS baseline ${c.name} flips only the raw sign and canonical fields`, {skip}, () => {
  for (const negative of [false, true]) fchsLiteralOracle(c, {negative});
});

test('actual FCHS baseline double sign change retains original payload across service unit boundaries', {skip}, () => {
  fchsLiteralOracle(fchsLiteralCases[0], {negative: true, count: 2});
});

test('actual FCHS baseline special raw payloads retain the own service frontier', {skip}, () => {
  for (const c of [
    {sig: 0xc123456789abcdefn, exp: 0x7fff},
    {sig: 1n, exp: 0},
  ]) fchsLiteralOracle({...c, control: 0x003f, status: 0x4720, top: 7, tag: 0, resultTag: 2}, {negative: true, services: 1});
});

test('actual FCHS baseline pending MF precedes occupied or empty ST0 without changing metadata or raw bytes', {skip}, () => {
  for (const tag of [0, 3]) {
    const f = fixture([0xb8, 0x78, 0x56, 0x34, 0x12, 0xd9, 0xe0, 0x43], {execution: {budget: 2}});
    rejectJsService(f);
    const calls = memoryCalls(f);
    try {
      seedMultiplyResidualBank(f, {sig: 0x8000000000000001n, exp: 0x3fff}, {
        control: tag === 3 ? 0x007e : 0x003e, status: 0x4701, top: 7, tag,
      });
      const scalar = new Uint8Array(f.engine.memory.buffer, f.engine.base, 56).slice(), banks = processBanks(f);
      view(scalar).setUint32(16, 0x12345678, true); view(scalar).setUint32(48, 0x1005, true);
      view(banks[0]).setUint16(18, 0xff81, true);
      const selected = f.cache.select(), state = f.slab();
      const backing = state.getUint32(84, true), length = state.getUint32(88, true) * 4096;
      const ram = new Uint8Array(f.engine.memory.buffer, backing, length).slice(), run = f.cache.execute(selected);
      assert.deepEqual([run.coreReason, run.reason, run.retired, run.work, run.pc], [9, 12, 1, 1, 0x1005]);
      assert.equal(f.engine.get(80), 0xff81);
      assert.deepEqual(new Uint8Array(f.engine.memory.buffer, f.engine.base, 56), scalar);
      assert.deepEqual(processBanks(f), banks);
      assert.deepEqual(new Uint8Array(f.engine.memory.buffer, backing, length), ram);
      assert.equal(f.slab().getUint32(48, true), 1);
      assert.deepEqual(calls, {read: 0, write: 0});
      event(f.engine, 16, 0x1005, 1);
      serviceCounts(f, 1);
    } finally {f.close();}
  }
});

test('actual FCHS baseline prefix quota stops before pending exception or stack access', {skip}, () => {
  for (const pending of [false, true]) {
    const f = fixture([0xb8, 0x78, 0x56, 0x34, 0x12, 0xd9, 0xe0, 0x43], {execution: {budget: 1}});
    rejectJsService(f);
    const calls = memoryCalls(f);
    try {
      seedMultiplyResidualBank(f, {sig: 0x8000000000000001n, exp: 0x3fff}, {
        control: pending ? 0x003e : 0x0000, status: pending ? 0x4701 : 0x4700, top: 7, tag: 3,
      });
      const scalar = new Uint8Array(f.engine.memory.buffer, f.engine.base, 56).slice(), banks = processBanks(f);
      view(scalar).setUint32(16, 0x12345678, true); view(scalar).setUint32(48, 0x1005, true);
      const selected = f.cache.select(), state = f.slab();
      const backing = state.getUint32(84, true), length = state.getUint32(88, true) * 4096;
      const ram = new Uint8Array(f.engine.memory.buffer, backing, length).slice(), run = f.cache.execute(selected);
      assert.deepEqual([run.coreReason, run.reason, run.retired, run.work, run.pc], [2, 1, 1, 1, 0x1005]);
      assert.deepEqual(new Uint8Array(f.engine.memory.buffer, f.engine.base, 56), scalar);
      assert.deepEqual(processBanks(f), banks);
      assert.deepEqual(new Uint8Array(f.engine.memory.buffer, backing, length), ram);
      assert.equal(f.slab().getUint32(48, true), 0);
      assert.deepEqual(calls, {read: 0, write: 0});
      serviceCounts(f, 0);
    } finally {f.close();}
  }
});

const fist32NormalBaselineServices = 1;

function fist32ExpectedBank(fp, c, {top, pop, opcode, pc = 0x1005, address = 0x1004, selector = 0x3b}) {
  let status = (fp.getUint16(18, true) & ~0x200) | (c.exceptions ?? 0) | (c.incremented ? 0x200 : 0) | (c.extraStatus ?? 0);
  if (pop) {
    fp.setUint16(20, fp.getUint16(20, true) | 3 << (top * 2), true);
    status = (status & ~0x3800) | ((top + 1) & 7) << 11;
  }
  fp.setUint16(18, status, true); fp.setUint16(22, opcode, true);
  fp.setUint32(24, pc, true); fp.setUint32(28, address, true); fp.setUint16(32, 0x1b, true); fp.setUint16(34, selector, true);
}

function fist32LiteralOracle(c, {pop = true, top = 7, tag = 2, control = c.control ?? 0x003f, status = 0x4700} = {}) {
  const modrm = c.truncate ? 0x0d : pop ? 0x1d : 0x15, opcode = 0x300 | modrm;
  const f = fixture([0xb8, 0x78, 0x56, 0x34, 0x12, 0x64, 0xdb, modrm, ...dword(0x1004), 0x9b, 0x43], {
    registers: [0xaabbccdd, 0x89abcdef, 0xd5e6789a, 0x23456789, 0x2f00, 0x55443322, 0x66554433, 0x77665544],
    data: dword(0x5aa5c33c).map((value, index) => [0x2004 + index, value]), execution: {budget: 2},
  });
  rejectJsService(f);
  const calls = memoryCalls(f), services = c.services ?? fist32NormalBaselineServices;
  try {
    f.engine.ok('set_segment_base', 4, 0x1000);
    const source = c.raw ?? multiplyRaw64(c.bits);
    seedMultiplyResidualBank(f, source, {control, status, top, tag: c.empty ? 3 : tag});
    const scalar = new Uint8Array(f.engine.memory.buffer, f.engine.base, 56).slice(), banks = processBanks(f);
    view(scalar).setUint32(16, 0x12345678, true); view(scalar).setUint32(48, 0x100c, true);
    fist32ExpectedBank(view(banks[0]), c, {top, pop, opcode});
    const selected = f.cache.select(), state = f.slab();
    const backing = state.getUint32(84, true), length = state.getUint32(88, true) * 4096;
    const slot = new DataView(f.engine.memory.buffer).getUint32(state.getUint32(76, true) + 8, true) - 1;
    const ram = new Uint8Array(f.engine.memory.buffer, backing, length).slice();
    new DataView(ram.buffer).setUint32(slot * 4096 + 4, c.word >>> 0, true);
    const run = f.cache.execute(selected);
    assert.deepEqual([run.coreReason, run.reason, run.retired, run.work, run.pc], [2, 1, 2, 2, 0x100c]);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, f.engine.base, 56), scalar);
    assert.deepEqual(processBanks(f), banks);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, backing, length), ram);
    assert.equal(f.slab().getUint32(48, true), 0);
    serviceCounts(f, services);
    assert.deepEqual(calls, {read: 0, write: 0});
    if (c.deferred) {
      const pending = f.cache.execute(f.cache.select());
      assert.deepEqual([pending.coreReason, pending.reason, pending.retired, pending.work, pending.pc], [9, 12, 0, 0, 0x100c]);
      assert.equal(f.engine.get(80), view(banks[0]).getUint16(18, true));
      assert.deepEqual(new Uint8Array(f.engine.memory.buffer, f.engine.base, 56), scalar);
      assert.deepEqual(processBanks(f), banks);
      assert.deepEqual(new Uint8Array(f.engine.memory.buffer, backing, length), ram);
      assert.equal(f.slab().getUint32(48, true), 2);
      event(f.engine, 16, 0x100c, 0);
      serviceCounts(f, services + 1);
      assert.deepEqual(calls, {read: 0, write: 0});
    }
  } finally {f.close();}
}

const fist32NormalCases = [
  {name: 'positive zero', bits: 0n, word: 0},
  {name: 'negative zero', bits: 0x8000000000000000n, word: 0},
  {name: 'positive half even zero', bits: 0x3fe0000000000000n, word: 0, exceptions: 0x20},
  {name: 'negative half even zero', bits: 0xbfe0000000000000n, word: 0, exceptions: 0x20},
  {name: 'positive odd tie increments magnitude', bits: 0x3ff8000000000000n, word: 2, exceptions: 0x20, incremented: true},
  {name: 'negative odd tie increments magnitude', bits: 0xbff8000000000000n, word: 0xfffffffe, exceptions: 0x20, incremented: true},
  {name: 'positive even tie keeps magnitude', bits: 0x4004000000000000n, word: 2, exceptions: 0x20},
  {name: 'negative even tie keeps magnitude', bits: 0xc004000000000000n, word: 0xfffffffe, exceptions: 0x20},
  {name: 'maximum plus quarter rounds inside range', bits: 0x41dfffffffd00000n, word: 0x7fffffff, exceptions: 0x20},
  {name: 'minimum minus half rounds inside range', bits: 0xc1e0000000100000n, word: 0x80000000, exceptions: 0x20},
  {name: 'exact positive endpoint ignores RC and masks', bits: 0x41dfffffffc00000n, word: 0x7fffffff, control: 0x0000},
  {name: 'exact negative endpoint ignores RC and masks', bits: 0xc1e0000000000000n, word: 0x80000000, control: 0x0840},
  {name: 'binary64 just above half', bits: 0x3fe0000000000001n, word: 1, exceptions: 0x20, incremented: true},
  {name: 'binary64 just below half', bits: 0x3fdfffffffffffffn, word: 0, exceptions: 0x20},
  {name: 'negative near minimum increments magnitude', bits: 0xc1dfffffffe00000n, word: 0x80000000, exceptions: 0x20, incremented: true},
  {name: 'minimum normal binary64 rounds to zero', bits: 0x0010000000000000n, word: 0, exceptions: 0x20},
];

for (const [index, c] of fist32NormalCases.entries()) test(`actual FIST32 baseline ${c.name} preserves raw source ghosts and immediate flags`, {skip}, () => {
  for (const pop of [false, true]) {
    const control = c.control ?? [0x003f, 0x007f, 0x023f, 0x037f, 0x013f][index % 5];
    fist32LiteralOracle(c, {pop, top: (index * 2 + Number(pop)) & 7, tag: (index + Number(pop)) % 3, control,
      status: 0x4500 | (pop ? 0 : 0x200) | (!c.exceptions && control & 0x20 ? 0x20 : 0)});
  }
});

for (const [index, c] of [
  {name: 'maximum halfway overflows with IE and no new PE', bits: 0x41dfffffffe00000n, word: 0x80000000, exceptions: 1},
  {name: 'minimum minus three quarters overflows with IE', bits: 0xc1e0000000180000n, word: 0x80000000, exceptions: 1},
  {name: 'positive two to31 is invalid not valid negative minimum', bits: 0x41e0000000000000n, word: 0x80000000, exceptions: 1},
  {name: 'raw80 bit beyond binary64 retains new PE', raw: {sig: 0x8000000000000001n, exp: 0x3fff}, word: 1, exceptions: 0x20},
  {name: 'quiet NaN keeps source payload and raises IE', raw: {sig: 0xc123456789abcdefn, exp: 0x7fff}, word: 0x80000000, exceptions: 1},
  {name: 'empty stack stores masked indefinite and raises SF IE', bits: 0x3ff8000000000000n, word: 0x80000000, exceptions: 0x41, empty: true},
  {name: 'round down negative inexact', bits: 0xbff8000000000000n, word: 0xfffffffe, exceptions: 0x20, incremented: true, control: 0x047f},
  {name: 'round up negative inexact', bits: 0xbff8000000000000n, word: 0xffffffff, exceptions: 0x20, control: 0x087f},
  {name: 'round toward zero positive inexact', bits: 0x3ff8000000000000n, word: 1, exceptions: 0x20, control: 0x0c7f},
  {name: 'FISTTP ignores nearest control and keeps its service', bits: 0x3ff8000000000000n, word: 1, exceptions: 0x20, truncate: true},
].entries()) test(`actual FIST32 baseline fallback ${c.name}`, {skip}, () => {
  fist32LiteralOracle({...c, services: 1}, {pop: true, top: index & 7, tag: index % 3});
});

test('actual FIST32 baseline unmasked PE commits store and pop before a later waiting MF', {skip}, () => {
  fist32LiteralOracle({bits: 0x3ff8000000000000n, word: 2, exceptions: 0x20, incremented: true,
    extraStatus: 0x8080, services: 1, control: 0x001f, deferred: true});
});

test('actual FIST32 baseline destination faults precede empty range and conversion effects while pending MF comes first', {skip}, () => {
  for (const c of [
    {address: 0x2ffe, detail: 1, fault: 0x3000},
    {address: 0x2004, detail: 2, fault: 0x2004, readonly: true},
    {address: 0x2ffe, detail: 1, fault: 0x3000, empty: true, services: 1},
    {address: 0x2ffe, detail: 1, fault: 0x3000, bits: 0x41e0000000000000n, services: 1},
    {address: 0x2ffe, empty: true, pending: true, services: 1},
  ]) {
    const f = fixture([0xb8, 0x78, 0x56, 0x34, 0x12, 0xdb, 0x1d, ...dword(c.address), 0x43], {
      data: [[c.address, 0x5a], [c.address + 1, 0xa5]], execution: {budget: 2},
    });
    rejectJsService(f);
    const calls = memoryCalls(f), services = c.services ?? fist32NormalBaselineServices;
    try {
      if (c.readonly) f.engine.ok('protect', 0x2000, 1, 1);
      seedMultiplyResidualBank(f, multiplyRaw64(c.bits ?? 0x3ff8000000000000n), {
        control: c.pending ? 0x003e : 0x003f, status: c.pending ? 0x4701 : 0x4700, top: 7, tag: c.empty ? 3 : 2,
      });
      const scalar = new Uint8Array(f.engine.memory.buffer, f.engine.base, 56).slice(), banks = processBanks(f);
      view(scalar).setUint32(16, 0x12345678, true); view(scalar).setUint32(48, 0x1005, true);
      if (c.pending) view(banks[0]).setUint16(18, 0xff81, true);
      const selected = f.cache.select(), state = f.slab(), backing = state.getUint32(84, true), length = state.getUint32(88, true) * 4096;
      const ram = new Uint8Array(f.engine.memory.buffer, backing, length).slice(), run = f.cache.execute(selected);
      assert.deepEqual([run.coreReason, run.reason, run.retired, run.work, run.pc], [c.pending ? 9 : 3, c.pending ? 12 : 5, 1, 1, 0x1005]);
      if (c.pending) assert.equal(f.engine.get(80), 0xff81);
      else assert.deepEqual([f.engine.get(80), f.engine.get(84), f.engine.get(88), f.engine.get(92)], [c.detail, c.fault, 2, 4]);
      assert.deepEqual(new Uint8Array(f.engine.memory.buffer, f.engine.base, 56), scalar);
      assert.deepEqual(processBanks(f), banks);
      assert.deepEqual(new Uint8Array(f.engine.memory.buffer, backing, length), ram);
      assert.equal(f.slab().getUint32(48, true), 1);
      event(f.engine, c.pending ? 16 : 0xffffffff, 0x1005, 1);
      serviceCounts(f, services);
      assert.deepEqual(calls, {read: 0, write: services === 0 ? 1 : 0});
    } finally {f.close();}
  }
});

test('actual FIST32 baseline prefix quota prevents pending and destination effects', {skip}, () => {
  for (const pending of [false, true]) {
    const f = fixture([0xb8, 0x78, 0x56, 0x34, 0x12, 0xdb, 0x1d, ...dword(0x2ffe)], {execution: {budget: 1}});
    rejectJsService(f);
    const calls = memoryCalls(f);
    try {
      seedMultiplyResidualBank(f, multiplyRaw64(0x3ff8000000000000n), {control: pending ? 0x003e : 0x003f, status: pending ? 0x4701 : 0x4700, top: 7, tag: 3});
      const scalar = new Uint8Array(f.engine.memory.buffer, f.engine.base, 56).slice(), banks = processBanks(f);
      view(scalar).setUint32(16, 0x12345678, true); view(scalar).setUint32(48, 0x1005, true);
      const selected = f.cache.select(), state = f.slab(), backing = state.getUint32(84, true), length = state.getUint32(88, true) * 4096;
      const ram = new Uint8Array(f.engine.memory.buffer, backing, length).slice(), run = f.cache.execute(selected);
      assert.deepEqual([run.coreReason, run.reason, run.retired, run.work, run.pc], [2, 1, 1, 1, 0x1005]);
      assert.deepEqual(new Uint8Array(f.engine.memory.buffer, f.engine.base, 56), scalar);
      assert.deepEqual(processBanks(f), banks);
      assert.deepEqual(new Uint8Array(f.engine.memory.buffer, backing, length), ram);
      assert.equal(f.slab().getUint32(48, true), 0);
      serviceCounts(f, 0); assert.deepEqual(calls, {read: 0, write: 0});
    } finally {f.close();}
  }
});

test('actual FIST32 baseline completed executable alias store commits PE C1 pop and metadata once', {skip}, () => {
  const f = fixture(undefined, {windowsMappings: true, physicalPages: 1024, execution: {budget: 2}});
  rejectJsService(f);
  const calls = memoryCalls(f);
  try {
    const [writable, executable] = sharedViews(f, 0x40, [2, 0x24]);
    const oldCode = [0xb8, 1, 0, 0, 0, 0xeb, 0xf9];
    f.engine.request(Uint8Array.from(oldCode)); f.engine.ok('upload', writable, oldCode.length);
    new DataView(f.engine.memory.buffer).setUint32(f.engine.base + 48, executable, true);
    const old = f.cache.select(); assert.equal(f.cache.execute(old).reason, 1);
    replaceCode(f, [0xb8, 0x78, 0x56, 0x34, 0x12, 0xdb, 0x1d, ...dword(writable), 0x43]);
    new DataView(f.engine.memory.buffer).setUint32(f.engine.base + 48, 0x1000, true);
    seedMultiplyResidualBank(f, multiplyRaw64(0x3ff8000000000000n), {control: 0x003f, status: 0x4700, top: 7, tag: 2});
    const scalar = new Uint8Array(f.engine.memory.buffer, f.engine.base, 56).slice(), banks = processBanks(f);
    view(scalar).setUint32(16, 0x12345678, true); view(scalar).setUint32(48, 0x100b, true);
    fist32ExpectedBank(view(banks[0]), {exceptions: 0x20, incremented: true}, {top: 7, pop: true, opcode: 0x31d, address: writable, selector: 0x23});
    const selected = f.cache.select(), state = f.slab(), backing = state.getUint32(84, true), length = state.getUint32(88, true) * 4096;
    const slot = new DataView(f.engine.memory.buffer).getUint32(state.getUint32(76, true) + (writable >>> 12) * 4, true) - 1;
    const ram = new Uint8Array(f.engine.memory.buffer, backing, length).slice(); new DataView(ram.buffer).setUint32(slot * 4096 + (writable & 4095), 2, true);
    const run = f.cache.execute(selected);
    assert.deepEqual([run.coreReason, run.reason, run.retired, run.work, run.pc], [6, 6, 2, 2, 0x100b]);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, f.engine.base, 56), scalar);
    assert.deepEqual(processBanks(f), banks);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, backing, length), ram);
    assert.equal(f.slab().getUint32(48, true), 0);
    f.cache.import(); assert.equal(f.cache.table.get(old.slot), null);
    serviceCounts(f, fist32NormalBaselineServices);
    assert.deepEqual(calls, {read: 0, write: fist32NormalBaselineServices === 0 ? 1 : 0});
  } finally {f.close();}
});

for (const precision of [24, 53]) for (const odd of [false, true]) {
  test(`actual memory PC${precision} ADD rounds the ${odd ? 'odd' : 'even'} midpoint before a store can clear C1`, {skip}, () => {
    const left = precision === 24 ? dword(odd ? 0x3f800001 : 0x3f800000)
      : [...dword(odd ? 1 : 0), ...dword(0x3ff00000)];
    const right = precision === 24 ? dword(0x33800000) : [...dword(0), ...dword(0x3ca00000)];
    const data = [...left.map((value, index) => [0x2000 + index, value]),
      ...right.map((value, index) => [0x2008 + index, value])];
    const f = fixture([0x64, precision === 24 ? 0xd9 : 0xdd, 0x05, ...dword(0x1000),
      0x64, precision === 24 ? 0xd8 : 0xdc, 0x05, ...dword(0x1008), 0x43],
      {data, execution: {budget: 2}});
    try {
      f.engine.ok('set_segment_base', 4, 0x1000);
      seedMultiplyResidualBank(f, {sig: 0x8000000000000000n, exp: 0x3fff},
        {control: 0x003f, status: 0x4020, top: 0, tag: 3});
      const arenaFp = new DataView(f.engine.memory.buffer, f.engine.base + 4236, 128);
      // exercise both encodings of the reserved CW bit6 without normalizing it.
      arenaFp.setUint16(16, (precision === 24 ? 0x003f : 0x023f) | (odd ? 0x40 : 0), true);
      arenaFp.setUint16(18, 0x4700, true); // C0/C2/C3 and old C1, fresh PE0.
      const scalar = new Uint8Array(f.engine.memory.buffer, f.engine.base, 56).slice(), banks = processBanks(f), fp = view(banks[0]);
      view(scalar).setUint32(48, 0x100e, true);
      fp.setUint16(18, odd ? 0x7f20 : 0x7d20, true); fp.setUint16(20, 0x3fff, true);
      fp.setUint16(22, precision === 24 ? 0x005 : 0x405, true);
      fp.setUint32(24, 0x1007, true); fp.setUint32(28, 0x1008, true);
      fp.setUint16(32, 0x1b, true); fp.setUint16(34, 0x3b, true);
      const resultSignificand = !odd ? 0x8000000000000000n
        : precision === 24 ? 0x8000020000000000n : 0x8000000000001000n;
      fp.setBigUint64(110, resultSignificand, true); fp.setUint16(118, 0x3fff, true);
      const selected = f.cache.select(), state = f.slab();
      const backing = state.getUint32(84, true), length = state.getUint32(88, true) * 4096;
      const ram = new Uint8Array(f.engine.memory.buffer, backing, length).slice(), run = f.cache.execute(selected);
      assert.deepEqual([run.coreReason, run.reason, run.retired, run.work, run.pc], [2, 1, 2, 2, 0x100e]);
      assert.deepEqual(new Uint8Array(f.engine.memory.buffer, f.engine.base, 56), scalar);
      assert.deepEqual(processBanks(f), banks);
      assert.deepEqual(new Uint8Array(f.engine.memory.buffer, backing, length), ram);
      assert.equal(f.slab().getUint32(48, true), 0);
      serviceCounts(f, 0);
    } finally {f.close();}
  });
}

test('actual fused store proofs survive read-only helpers and expire across writes', {skip}, () => {
  for (const probe of [
    {address: 0x2000, width: 4, value: 0x12345678, status: 0},
    {address: 0x2000, width: 0x104, value: 0x12345678, status: 0},
    {address: 0x9000, width: 4, value: 0, status: 3},
    {address: 0x2000, width: 3, value: 0, status: 8},
  ]) {
    const f = fixture([0xa3, 0, 0x20, 0, 0], {independent: false,
      registers: [0x12345678, 0, 0, 0, 0x2f00, 0, 0, 0],
      data: [[0x2000, 0x78], [0x2001, 0x56], [0x2002, 0x34], [0x2003, 0x12]]});
    let active = false;
    try {
      f.engine.ok('cpu_begin', ...KEY); active = true;
      const api = f.engine.api, context = api.cpu_context_ptr() >>> 0;
      f.slab().setUint32(48, 2, true);
      f.engine.ok('cpu_prepare', ...KEY, 0x1000, 1);
      const bytes = new Uint8Array(f.engine.memory.buffer, api.cpu_code_ptr() >>> 0, api.cpu_code_len() >>> 0);
      const unit = new WebAssembly.Instance(new WebAssembly.Module(bytes), {env: {memory: f.engine.memory},
        cpu: {read: api.cpu_runtime_read, write: api.cpu_runtime_write}});
      const beforeWrongContext = new Uint8Array(f.engine.memory.buffer).slice();
      for (const pointer of [0, context + 8, 0xfffffff0]) {
        assert.equal(unit.exports.run(pointer), 8);
        assert.deepEqual(new Uint8Array(f.engine.memory.buffer), beforeWrongContext);
      }
      const expectedState = beforeWrongContext.slice(context, context + 160), scalar = new DataView(expectedState.buffer);
      for (const [at, value] of [[40, 0x1005], [48, 1], [52, 0], [56, 1], [108, 1]]) scalar.setUint32(at, value, true);
      assert.equal(unit.exports.run(context), 0);
      assert.deepEqual(new Uint8Array(f.engine.memory.buffer, context, 160), expectedState,
        'the same-byte seed store retires once and records one ordinary dirty slot');
      const all = new DataView(f.engine.memory.buffer), state = f.slab(), header = state.getUint32(72, true);
      assert.deepEqual([4, 8, 12, 20].map(at => all.getUint32(header + at, true)), [0x434d3352, 32, 2, 1 << 20]);
      const rows = all.getUint32(header + 16, true), epoch = all.getUint32(header, true);
      const row = all.getBigUint64(rows + 2 * 8, true);
      assert.equal(Number(row >> 32n), epoch);
      assert.equal(Number(row & 7n), 7, 'the native seed creates an ordinary store proof');
      const table = new Uint8Array(f.engine.memory.buffer, rows, 8 << 20).slice();
      const proof = new Uint8Array(f.engine.memory.buffer, header, 32).slice();
      const backing = state.getUint32(84, true), length = state.getUint32(88, true) * 4096;
      const ram = new Uint8Array(f.engine.memory.buffer, backing, length).slice(), banks = processBanks(f);
      assert.deepEqual(ram, beforeWrongContext.slice(backing, backing + length), 'the seed writes the same existing bytes across full physical RAM');
      assert.equal(api.cpu_runtime_read(context, probe.address, probe.width) >>> 0, probe.value);
      assert.equal(f.slab().getUint32(52, true), probe.status);
      scalar.setUint32(52, probe.status, true);
      if (probe.status === 3) for (const [at, value] of [[60, probe.address], [64, 0x10104], [68, 4]]) scalar.setUint32(at, value, true);
      assert.deepEqual(new Uint8Array(f.engine.memory.buffer, context, 160), expectedState, 'read-only helper preserves scalar progress and the dirty queue');
      assert.deepEqual(new Uint8Array(f.engine.memory.buffer, header, 32), proof);
      assert.deepEqual(new Uint8Array(f.engine.memory.buffer, rows, 8 << 20), table);
      assert.deepEqual(new Uint8Array(f.engine.memory.buffer, backing, length), ram);
      assert.deepEqual(processBanks(f), banks);
      if (probe.width === 4 && probe.status === 0) {
        assert.equal(api.cpu_runtime_write(context, 0x2000, 4, 0x87654321), 0);
        const current = new DataView(f.engine.memory.buffer);
        assert.notEqual(current.getUint32(header, true), epoch);
        assert.equal(current.getBigUint64(rows + 2 * 8, true), row);
        const slot = current.getUint32(f.slab().getUint32(76, true) + 2 * 4, true) - 1;
        new DataView(ram.buffer).setUint32(slot * 4096, 0x87654321, true);
        assert.deepEqual(new Uint8Array(f.engine.memory.buffer, backing, length), ram);
      }
    } finally {
      if (active) f.engine.ok('cpu_end', ...KEY);
      f.close();
    }
  }
});

test('actual pure FP services preserve page proofs while publishing dirty bytes', {skip}, () => {
  for (const mutating of [true, false]) {
    const code = mutating
      ? [0xa3, 0, 0x20, 0, 0, 0xd9, 0xe8, 0xdb, 0x3d, 0x10, 0x20, 0, 0]
      : [0xa3, 0, 0x20, 0, 0, 0xdb, 0xe3];
    const f = fixture(code, {independent: false, registers: [0x12345678, 0, 0, 0, 0x2f00, 0, 0, 0]});
    let active = false;
    try {
      const api = f.engine.api;
      f.engine.ok('cpu_begin', ...KEY); active = true;
      const context = api.cpu_context_ptr() >>> 0;
      f.slab().setUint32(48, 4, true);
      f.engine.ok('cpu_prepare', ...KEY, 0x1000, mutating ? 3 : 2);
      const bytes = new Uint8Array(f.engine.memory.buffer, api.cpu_code_ptr() >>> 0, api.cpu_code_len() >>> 0);
      const unit = new WebAssembly.Instance(new WebAssembly.Module(bytes), {env: {memory: f.engine.memory},
        cpu: {read: api.cpu_runtime_read, write: api.cpu_runtime_write}});
      assert.equal(unit.exports.run(context), 7);
      let all = new DataView(f.engine.memory.buffer), state = f.slab();
      const prefix = mutating ? 2 : 1, next = mutating ? 0x100d : 0x1007;
      assert.deepEqual([state.getUint32(40, true), state.getUint32(56, true), state.getUint32(108, true)],
        [mutating ? 0x1007 : 0x1005, prefix, 1]);
      const header = state.getUint32(72, true), rows = all.getUint32(header + 16, true);
      const epoch = all.getUint32(header, true), row = all.getBigUint64(rows + 2 * 8, true);
      assert.equal(Number(row >> 32n), epoch); assert.equal(Number(row & 7n), 7);
      const table = new Uint8Array(f.engine.memory.buffer, rows, 8 << 20).slice();
      const backing = state.getUint32(84, true), length = state.getUint32(88, true) * 4096;
      const ram = new Uint8Array(f.engine.memory.buffer, backing, length).slice();
      const versionPointer = state.getUint32(156, true), version = all.getBigUint64(versionPointer, true);
      assert.equal(api.cpu_runtime_instruction(context), 0);
      all = new DataView(f.engine.memory.buffer); state = f.slab();
      assert.deepEqual([state.getUint32(40, true), state.getUint32(56, true), state.getUint32(48, true), state.getUint32(108, true)],
        [next, prefix + 1, 3 - prefix, 0]);
      assert(all.getBigUint64(versionPointer, true) > version, 'the completed ordinary dirty batch was really published');
      assert.equal(all.getUint32(header, true) === epoch, !mutating);
      assert.deepEqual(new Uint8Array(f.engine.memory.buffer, rows, 8 << 20), table);
      if (mutating) {
        const slot = all.getUint32(state.getUint32(76, true) + 2 * 4, true) - 1;
        ram.set([0, 0, 0, 0, 0, 0, 0, 0x80, 0xff, 0x3f], slot * 4096 + 0x10);
      }
      assert.deepEqual(new Uint8Array(f.engine.memory.buffer, backing, length), ram);
    } finally {
      if (active) f.engine.ok('cpu_end', ...KEY);
      f.close();
    }
  }
});

test('actual ordinary store proofs expire when pure FP publication reaches version exhaustion', {skip}, () => {
  const registers = [0x11223344, 0x22334455, 0x33445566, 0x44556677, 0x2f00, 0x55667788, 0x66778899, 0x778899aa];
  const f = fixture([0xa3, 0, 0x20, 0, 0, 0xdb, 0xe3, 0xa3, 4, 0x20, 0, 0], {
    independent: false, registers, fpControl: 0x027f,
    data: [[0x2000, 0xde], [0x2001, 0xad], [0x2002, 0xbe], [0x2003, 0xef],
      [0x2004, 0xaa], [0x2005, 0xbb], [0x2006, 0xcc], [0x2007, 0xdd]],
  });
  let active = false, versionPointer = 0, savedVersion;
  try {
    const api = f.engine.api;
    f.engine.ok('cpu_begin', ...KEY); active = true;
    const context = api.cpu_context_ptr() >>> 0;
    f.slab().setUint32(48, 4, true);
    const prepare = (pc, maximum) => {
      f.slab().setUint32(40, pc, true);
      f.engine.ok('cpu_prepare', ...KEY, pc, maximum);
      const bytes = new Uint8Array(f.engine.memory.buffer, api.cpu_code_ptr() >>> 0, api.cpu_code_len() >>> 0);
      return new WebAssembly.Instance(new WebAssembly.Module(bytes), {env: {memory: f.engine.memory},
        cpu: {read: api.cpu_runtime_read, write: api.cpu_runtime_write}}).exports.run;
    };
    // both units exist before the first store. no prepare may revoke the proof
    // between the preserving fp service and the second same-page store.
    const suffix = prepare(0x1007, 1), prefix = prepare(0x1000, 2);
    let all = new DataView(f.engine.memory.buffer), state = f.slab();
    versionPointer = state.getUint32(156, true);
    assert(versionPointer > 0 && versionPointer % 8 === 0);
    savedVersion = all.getBigUint64(versionPointer, true);
    const backing = state.getUint32(84, true), length = state.getUint32(88, true) * 4096;
    const slot = all.getUint32(state.getUint32(76, true) + 2 * 4, true) - 1;
    const capacity = state.getUint32(112, true), bits = state.getUint32(100, true), slots = state.getUint32(104, true);
    const expectedRam = new Uint8Array(f.engine.memory.buffer, backing, length).slice();
    expectedRam.set([0x44, 0x33, 0x22, 0x11], slot * 4096);
    all.setBigUint64(versionPointer, 0xfffffffffffffffen, true);

    assert.equal(prefix(context), 7);
    all = new DataView(f.engine.memory.buffer); state = f.slab();
    assert.deepEqual([state.getUint32(40, true), state.getUint32(48, true), state.getUint32(56, true), state.getUint32(108, true)],
      [0x1005, 3, 1, 1], 'the first ordinary store commits and retires exactly once');
    assert.equal(all.getBigUint64(versionPointer, true), 0xfffffffffffffffen, 'generated ordinary stores still defer version publication');
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, backing, length), expectedRam);
    assert.equal(all.getUint8(bits + slot), 1);
    assert.equal(all.getUint32(slots, true), slot);
    const header = state.getUint32(72, true), rows = all.getUint32(header + 16, true), epoch = all.getUint32(header, true);
    const row = all.getBigUint64(rows + 2 * 8, true);
    assert.equal(Number(row >> 32n), epoch);
    assert.equal(Number(row & 7n), 7, 'the actual native-bound store installed its ordinary-store proof');

    assert.equal(api.cpu_runtime_instruction(context), 0);
    all = new DataView(f.engine.memory.buffer); state = f.slab();
    assert.equal(all.getBigUint64(versionPointer, true), 0xffffffffffffffffn, 'actual FNINIT service publishes the dirty batch to MAX');
    assert.deepEqual([state.getUint32(40, true), state.getUint32(48, true), state.getUint32(56, true), state.getUint32(108, true)],
      [0x1007, 2, 2, 0]);
    assert.equal(all.getUint8(bits + slot), 0, 'published dirty bits are cleared');
    const numeric = state.getUint32(124, true), fp = all.getUint32(numeric + 8, true);
    assert.equal(all.getUint16(fp, true), 0x037f, 'the real FNINIT operation completed');
    assert.notEqual(all.getUint32(header, true), epoch,
      'reaching MAX during preserving FP publication must revoke every cached ordinary-store proof before continuation');
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, backing, length), expectedRam);

    const expectedState = new Uint8Array(f.engine.memory.buffer, context, 160).slice();
    new DataView(expectedState.buffer).setUint32(52, 8, true);
    const cleanBits = new Uint8Array(f.engine.memory.buffer, bits, capacity).slice();
    const cleanSlots = new Uint8Array(f.engine.memory.buffer, slots, capacity * 4).slice();
    // use the retained suffix directly; cpu_prepare here would mask the owner bug.
    assert.equal(suffix(context), 8);
    all = new DataView(f.engine.memory.buffer); state = f.slab();
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, context, 160), expectedState,
      'the exhausted second store changes only its failure status, with no work, retirement or dirty append');
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, backing, length), expectedRam,
      'the first store remains committed and the later same-page destination remains untouched');
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, bits, capacity), cleanBits);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, slots, capacity * 4), cleanSlots);
    assert.equal(all.getBigUint64(versionPointer, true), 0xffffffffffffffffn);
    assert.deepEqual(registers.map((_, index) => state.getUint32(8 + index * 4, true)), registers);
  } finally {
    if (versionPointer && savedVersion !== undefined)
      new DataView(f.engine.memory.buffer).setBigUint64(versionPointer, savedVersion, true);
    if (active) f.engine.ok('cpu_end', ...KEY);
    f.close();
  }
});

test('actual warmed ordinary store proof still rejects a cross-page suffix before effects', {skip}, () => {
  const registers = [0x12345678, 0x76543210, 0xa1b2c3d4, 0x55667788, 0x2f00, 0x13579bdf, 0x2468ace0, 0x0f1e2d3c];
  const f = fixture([0xa3, 0, 0x20, 0, 0, 0xa3, 0xfe, 0x2f, 0, 0],
    {independent: false, registers, data: [[0x2ffe, 0x5a], [0x2fff, 0xa5]]});
  let active = false, writes = 0;
  try {
    const api = f.engine.api;
    f.engine.ok('cpu_begin', ...KEY); active = true;
    const context = api.cpu_context_ptr() >>> 0;
    f.slab().setUint32(48, 3, true);
    f.engine.ok('cpu_prepare', ...KEY, 0x1000, 2);
    const state = f.slab(), all = new DataView(f.engine.memory.buffer);
    const backing = state.getUint32(84, true), length = state.getUint32(88, true) * 4096;
    const slot = all.getUint32(state.getUint32(76, true) + 2 * 4, true) - 1;
    const header = state.getUint32(72, true), rows = all.getUint32(header + 16, true);
    const bits = state.getUint32(100, true), slots = state.getUint32(104, true), capacity = state.getUint32(112, true);
    const version = state.getUint32(156, true), beforeVersion = all.getBigUint64(version, true);
    const expectedState = new Uint8Array(f.engine.memory.buffer, context, 160).slice();
    for (const [at, value] of [[40, 0x1005], [48, 2], [52, 3], [56, 1], [60, 0x3000], [64, 0x10204], [68, 4], [108, 1]])
      new DataView(expectedState.buffer).setUint32(at, value, true);
    const expectedRam = new Uint8Array(f.engine.memory.buffer, backing, length).slice();
    expectedRam.set([0x78, 0x56, 0x34, 0x12], slot * 4096);
    const expectedBits = new Uint8Array(f.engine.memory.buffer, bits, capacity).slice(); expectedBits[slot] = 1;
    const expectedSlots = new Uint8Array(f.engine.memory.buffer, slots, capacity * 4).slice();
    new DataView(expectedSlots.buffer).setUint32(0, slot, true);
    const banks = processBanks(f), nativeWrite = api.cpu_runtime_write;
    const bytes = new Uint8Array(f.engine.memory.buffer, api.cpu_code_ptr() >>> 0, api.cpu_code_len() >>> 0);
    const unit = new WebAssembly.Instance(new WebAssembly.Module(bytes), {env: {memory: f.engine.memory}, cpu: {
      read: api.cpu_runtime_read,
      write: (pointer, address, width, value) => {
        writes++;
        assert.deepEqual([pointer >>> 0, address, width, value >>> 0], [context, 0x2ffe, 4, 0x12345678]);
        const current = new DataView(f.engine.memory.buffer), row = current.getBigUint64(rows + 2 * 8, true);
        assert.equal(Number(row >> 32n), current.getUint32(header, true));
        assert.equal(Number(row & 7n), 7, 'the completed first store leaves a current ordinary proof for this same page');
        assert.equal(f.slab().getUint32(108, true), 1);
        assert.deepEqual(new Uint8Array(f.engine.memory.buffer, backing, length), expectedRam);
        return nativeWrite(pointer, address, width, value);
      },
    }});
    assert.equal(unit.exports.run(context), 3);
    assert.equal(writes, 1, 'only the cross-page suffix enters the canonical write helper');
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, context, 160), expectedState);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, backing, length), expectedRam,
      'the first store remains committed and both late-page sentinel bytes remain untouched');
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, bits, capacity), expectedBits);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer, slots, capacity * 4), expectedSlots);
    assert.equal(new DataView(f.engine.memory.buffer).getBigUint64(version, true), beforeVersion);
    assert.deepEqual(processBanks(f), banks);
  } finally {
    if (active) f.engine.ok('cpu_end', ...KEY);
    f.close();
  }
});

test('actual unconditional service entry preserves canonical state before service at every cold frontier', {skip}, () => {
  const f = fixture([0xdb, 0xe3], {independent: false, fpControl: 0x127f, fpStatus: 0x4720,
    registers: [0xffffffff, 0x80000000, 0xa1b2c3d4, 0x55667788, 0x2f00, 0x13579bdf, 0x2468ace0, 0x0f1e2d3c]});
  let active = false;
  try {
    const api = f.engine.api;
    f.engine.ok('cpu_begin', ...KEY); active = true;
    const context = api.cpu_context_ptr() >>> 0;
    f.slab().setUint32(44, 0x80000800, true);
    f.slab().setUint32(48, 2, true);
    f.engine.ok('cpu_prepare', ...KEY, 0x1000, 1);
    const bytes = new Uint8Array(f.engine.memory.buffer, api.cpu_code_ptr() >>> 0, api.cpu_code_len() >>> 0);
    const unit = new WebAssembly.Instance(new WebAssembly.Module(bytes), {env: {memory: f.engine.memory},
      cpu: {read: api.cpu_runtime_read, write: api.cpu_runtime_write}});
    const expected = new Uint8Array(f.engine.memory.buffer).slice(), state = new DataView(expected.buffer, context, 160);
    const numeric = state.getUint32(124, true), all = new DataView(expected.buffer);
    assert.equal(all.getUint16(all.getUint32(numeric + 8, true), true), 0x127f,
      'the imported native FP bank makes an accidental FNINIT execution observable');
    for (const pointer of [0, context + 8, 0xfffffff0]) {
      assert.equal(unit.exports.run(pointer), 8);
      assert.deepEqual(new Uint8Array(f.engine.memory.buffer), expected);
    }
    for (const quota of [0, 2]) {
      f.slab().setUint32(48, quota, true); state.setUint32(48, quota, true);
      const status = quota === 0 ? 2 : 7;
      state.setUint32(52, status, true);
      assert.equal(unit.exports.run(context), status);
      assert.deepEqual(new Uint8Array(f.engine.memory.buffer), expected,
        'only status changes; every scalar byte, raw opaque FLAGS, native and process banks, cache proof, RAM and dirty record remain exact');
    }
    f.slab().setUint32(40, 0x10ff, true); state.setUint32(40, 0x10ff, true);
    f.slab().setUint32(48, 0, true); state.setUint32(48, 0, true);
    state.setUint32(52, 0, true);
    assert.equal(unit.exports.run(context), 0, 'an unknown PC retains unit Continue0 even with zero quota');
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer), expected);
  } finally {
    if (active) f.engine.ok('cpu_end', ...KEY);
    f.close();
  }
});

test('actual internal generic route publishes dirty scalar state before an unconditional service entry', {skip}, () => {
  const f = fixture([
    0x74, 0x0e,
    0x83, 0xc0, 1,
    0xb9, 0x10, 0x10, 0, 0,
    0xff, 0xe1,
    0x90, 0x90, 0x90, 0x90,
    0xdb, 0xe3,
  ], {independent: false, fpControl: 0x127f, fpStatus: 0x4720,
    registers: [0x7fffffff, 0xaabbccdd, 0xa1b2c3d4, 0x55667788, 0x2f00, 0x13579bdf, 0x2468ace0, 0x0f1e2d3c]});
  let active = false;
  try {
    const api = f.engine.api;
    f.engine.ok('cpu_begin', ...KEY); active = true;
    const context = api.cpu_context_ptr() >>> 0;
    f.slab().setUint32(44, 0x80000603, true);
    f.slab().setUint32(48, 8, true);
    f.engine.ok('cpu_prepare', ...KEY, 0x1000, 5);
    const bytes = new Uint8Array(f.engine.memory.buffer, api.cpu_code_ptr() >>> 0, api.cpu_code_len() >>> 0);
    const unit = new WebAssembly.Instance(new WebAssembly.Module(bytes), {env: {memory: f.engine.memory},
      cpu: {read: api.cpu_runtime_read, write: api.cpu_runtime_write}});
    const expected = new Uint8Array(f.engine.memory.buffer).slice(), state = new DataView(expected.buffer, context, 160);
    for (const [at, value] of [[8, 0x80000000], [12, 0x1010], [40, 0x1010], [44, 0x80000e96], [48, 4], [52, 7], [56, 4]])
      state.setUint32(at, value, true);
    assert.equal(unit.exports.run(context), 7);
    assert.deepEqual(new Uint8Array(f.engine.memory.buffer), expected,
      'the not-taken JZ retains FNINIT, while ADD/MOV/indirect JMP publish four retired instructions and exact FLAGS before service without executing FNINIT');
  } finally {
    if (active) f.engine.ok('cpu_end', ...KEY);
    f.close();
  }
});

test('actual native quota exits publish each pending predicate and preserve cold interior FLAGS', {skip}, () => {
  const f = fixture([0x39, 0xc8, 0x39, 0xda, 0x90], {independent: false, fpControl: 0x127f, fpStatus: 0x4720,
    registers: [1, 2, 0x80000000, 1, 0x2f00, 0x13579bdf, 0x2468ace0, 0x0f1e2d3c]});
  let active = false;
  try {
    const api = f.engine.api;
    f.engine.ok('cpu_begin', ...KEY); active = true;
    const context = api.cpu_context_ptr() >>> 0;
    f.slab().setUint32(48, 3, true);
    f.engine.ok('cpu_prepare', ...KEY, 0x1000, 3);
    const bytes = new Uint8Array(f.engine.memory.buffer, api.cpu_code_ptr() >>> 0, api.cpu_code_len() >>> 0);
    const unit = new WebAssembly.Instance(new WebAssembly.Module(bytes), {env: {memory: f.engine.memory},
      cpu: {read: api.cpu_runtime_read, write: api.cpu_runtime_write}});
    const expected = new Uint8Array(f.engine.memory.buffer).slice();
    const initialState = expected.slice(context, context + 160), state = new DataView(expected.buffer, context, 160);
    const rows = [
      {pc: 0x1000, quota: 0, flags: 0x80000603, next: 0x1000, retired: 0, after: 0x80000603, status: 2},
      {pc: 0x1000, quota: 1, flags: 0x80000603, next: 0x1002, retired: 1, after: 0x80000697, status: 2},
      {pc: 0x1000, quota: 2, flags: 0x80000603, next: 0x1004, retired: 2, after: 0x80000e16, status: 2},
      {pc: 0x1000, quota: 3, flags: 0x80000603, next: 0x1005, retired: 3, after: 0x80000e16, status: 0},
      {pc: 0x1002, quota: 0, flags: 0x80000800, next: 0x1002, retired: 0, after: 0x80000800, status: 2},
      {pc: 0x1004, quota: 0, flags: 0x80000800, next: 0x1004, retired: 0, after: 0x80000800, status: 2},
    ];
    for (const row of rows) {
      new Uint8Array(f.engine.memory.buffer, context, 160).set(initialState);
      new Uint8Array(expected.buffer, context, 160).set(initialState);
      for (const [at, value] of [[40, row.pc], [44, row.flags], [48, row.quota], [56, 0xfffffffe]]) {
        f.slab().setUint32(at, value, true); state.setUint32(at, value, true);
      }
      for (const [at, value] of [[40, row.next], [44, row.after], [48, 0], [52, row.status], [56, (0xfffffffe + row.retired) >>> 0]])
        state.setUint32(at, value, true);
      assert.equal(unit.exports.run(context), row.status);
      assert.deepEqual(new Uint8Array(f.engine.memory.buffer), expected,
        `PC${row.pc.toString(16)} quota${row.quota}: full scalar state, opaque FLAGS, native/process numeric banks, RAM, cache and dirty queue remain exact`);
    }
  } finally {
    if (active) f.engine.ok('cpu_end', ...KEY);
    f.close();
  }
});

test('actual retained native stores keep owned dirty descriptors across COW and published leases', {skip}, () => {
  const f = fixture(undefined, {windowsMappings: true, physicalPages: 1024, execution: {budget: 2, instructions: 2}});
  let nativePublish;
  try {
    const [shared, copy] = sharedViews(f, 4, [2, 1]);
    f.engine.ok('write32', shared, 0x11223344);
    replaceCode(f, [0xa3, ...dword(copy), 0xeb, 0xf9]);
    const calls = memoryCalls(f), values = [0xaabbccdd, 0x55667788, 0x87654321];
    let retained, descriptors, expectedRam, originalSlot, copiedSlot, publications = 0;
    const api = f.engine.api; nativePublish = api.cpu_publish;
    for (const [index, value] of values.entries()) {
      new DataView(f.engine.memory.buffer).setUint32(f.engine.base + 16, value, true);
      f.cache.import(); const unit = f.cache.select(), state = f.slab();
      const current = [api.cpu_context_ptr() >>> 0, state.getUint32(100, true), state.getUint32(104, true), state.getUint32(112, true)];
      if (index === 0) {retained = unit; descriptors = current;}
      else {
        assert.equal(unit, retained, 'each fresh lease executes the same previously instantiated native unit');
        assert.deepEqual(current, descriptors, 'the same owner keeps its bitmap, list and capacity across publication and begin');
      }
      const [, bits, slots, capacity] = current;
      const all = new DataView(f.engine.memory.buffer), backing = state.getUint32(84, true), length = state.getUint32(88, true) * 4096;
      const mappings = state.getUint32(76, true);
      const slot = address => new DataView(f.engine.memory.buffer).getUint32(mappings + (address >>> 12) * 4, true) - 1;
      const version = state.getUint32(156, true), beforeVersion = all.getBigUint64(version, true);
      if (index === 0) {
        originalSlot = slot(shared); assert.equal(slot(copy), originalSlot);
        expectedRam = new Uint8Array(f.engine.memory.buffer, backing, length).slice();
      } else assert.equal(slot(copy), copiedSlot);
      assert.equal(state.getUint32(108, true), 0);
      assert.deepEqual(new Uint8Array(f.engine.memory.buffer, bits, capacity), new Uint8Array(capacity));
      const banks = processBanks(f), numeric = state.getUint32(124, true);
      const numericRanges = [[all.getUint32(numeric + 8, true), 100], [all.getUint32(numeric + 36, true), 132]];
      const nativeBanks = numericRanges.map(([pointer, count]) => new Uint8Array(f.engine.memory.buffer, pointer, count).slice());
      const writesBefore = calls.write;
      api.cpu_publish = (...key) => {
        publications++;
        const pending = f.slab(), target = slot(copy);
        assert.deepEqual([pending.getUint32(40, true), pending.getUint32(44, true), pending.getUint32(48, true),
          pending.getUint32(52, true), pending.getUint32(56, true)], [0x1000, 2, 0, 2, 2]);
        assert.deepEqual(Array.from({length: 8}, (_, register) => pending.getUint32(8 + register * 4, true)),
          [value, 0, 0, 0, 0x2f00, 0, 0, 0]);
        assert.equal(pending.getUint32(108, true), index === 0 ? 0 : 1,
          'the split is canonical once; each later ordinary batch starts again at list index zero');
        if (index !== 0) {
          const memory = new DataView(f.engine.memory.buffer);
          assert.equal(memory.getUint8(bits + target), 1);
          assert.equal(memory.getUint32(slots, true), target);
        }
        return nativePublish(...key);
      };
      const run = f.cache.execute(unit);
      assert.deepEqual([run.reason, run.retired, run.work, run.pc], [1, 2, 2, 0x1000]);
      assert.equal(calls.write - writesBefore, index === 0 ? 1 : 0,
        'only the first COW split needs the real native write helper');
      assert.equal(slot(shared), originalSlot);
      if (index === 0) {
        copiedSlot = slot(copy); assert.notEqual(copiedSlot, originalSlot);
        expectedRam.set(expectedRam.slice(originalSlot * 4096, (originalSlot + 1) * 4096), copiedSlot * 4096);
      }
      new DataView(expectedRam.buffer).setUint32(copiedSlot * 4096, value, true);
      assert.deepEqual(new Uint8Array(f.engine.memory.buffer, f.slab().getUint32(84, true), length), expectedRam);
      assert.equal(f.slab().getUint32(108, true), 0);
      assert.deepEqual(new Uint8Array(f.engine.memory.buffer, bits, capacity), new Uint8Array(capacity));
      if (index !== 0) assert.equal(new DataView(f.engine.memory.buffer).getBigUint64(version, true), beforeVersion + 1n);
      assert.deepEqual(processBanks(f), banks);
      assert.deepEqual(numericRanges.map(([pointer, count]) => new Uint8Array(f.engine.memory.buffer, pointer, count)), nativeBanks);
    }
    assert.equal(publications, 3);
  } finally {
    if (nativePublish) f.engine.api.cpu_publish = nativePublish;
    f.close();
  }
});

test('actual native CMP branch consumers and cold entries publish canonical opaque FLAGS', {skip}, () => {
  const f = fixture([
    0x39, 0xc8, 0x72, 0x0a,
    0xba, 0x22, 0x22, 0x22, 0x22, 0xe9, 0xf2, 0xef, 1, 0,
    0xba, 0x11, 0x11, 0x11, 0x11, 0xe9, 0xe8, 0xef, 1, 0,
  ], {independent: false, fpControl: 0x127f, fpStatus: 0x4720,
    registers: [1, 2, 0xa1b2c3d4, 0x55667788, 0x2f00, 0x13579bdf, 0x2468ace0, 0x0f1e2d3c]});
  let active = false;
  try {
    const api = f.engine.api;
    f.engine.ok('cpu_begin', ...KEY); active = true;
    const context = api.cpu_context_ptr() >>> 0;
    f.slab().setUint32(48, 4, true);
    f.engine.ok('cpu_prepare', ...KEY, 0x1000, 6);
    const bytes = new Uint8Array(f.engine.memory.buffer, api.cpu_code_ptr() >>> 0, api.cpu_code_len() >>> 0);
    const unit = new WebAssembly.Instance(new WebAssembly.Module(bytes), {env: {memory: f.engine.memory},
      cpu: {read: api.cpu_runtime_read, write: api.cpu_runtime_write}});
    const expected = new Uint8Array(f.engine.memory.buffer).slice();
    const initial = expected.slice(context, context + 160), state = new DataView(expected.buffer, context, 160);
    const rows = [
      {pc: 0x1000, eax: 0, ecx: 1, flags: 0x80000602, quota: 4, edx: 0x11111111, after: 0x80000697, next: 0x20000, retired: 4, status: 0},
      {pc: 0x1000, eax: 0x80000000, ecx: 1, flags: 0x80000603, quota: 4, edx: 0x22222222, after: 0x80000e16, next: 0x20000, retired: 4, status: 0},
      {pc: 0x1002, eax: 0, ecx: 1, flags: 0x80000800, quota: 3, edx: 0x22222222, after: 0x80000800, next: 0x20000, retired: 3, status: 0},
      {pc: 0x1002, eax: 0x80000000, ecx: 1, flags: 0x80000801, quota: 3, edx: 0x11111111, after: 0x80000801, next: 0x20000, retired: 3, status: 0},
      {pc: 0x1000, eax: 0, ecx: 1, flags: 0x80000602, quota: 1, edx: 0xa1b2c3d4, after: 0x80000697, next: 0x1002, retired: 1, status: 2},
    ];
    for (const row of rows) {
      new Uint8Array(f.engine.memory.buffer, context, 160).set(initial);
      new Uint8Array(expected.buffer, context, 160).set(initial);
      for (const [at, value] of [[8, row.eax], [12, row.ecx], [40, row.pc], [44, row.flags], [48, row.quota]]) {
        f.slab().setUint32(at, value, true); state.setUint32(at, value, true);
      }
      for (const [at, value] of [[16, row.edx], [40, row.next], [44, row.after], [48, 0], [52, row.status], [56, row.retired]])
        state.setUint32(at, value, true);
      assert.equal(unit.exports.run(context), row.status);
      assert.deepEqual(new Uint8Array(f.engine.memory.buffer), expected,
        `PC${row.pc.toString(16)} quota${row.quota}: CMP/JB uses the executed producer or canonical cold FLAGS, preserving every scalar byte, RAM and bank`);
    }
  } finally {
    if (active) f.engine.ok('cpu_end', ...KEY);
    f.close();
  }
});

test('actual qualified native ADD SUB recipes preserve quota overflow carry borrow and cold state', {skip}, () => {
  for (const variant of [
    {name: 'ADD', opcode: 0xc0, input: 0x7fffffff, result: 0x80000000, flags: 0x80000e96,
      edgeInput: 0xffffffff, edgeResult: 0, edgeFlags: 0x80000657},
    {name: 'SUB', opcode: 0xe8, input: 0x80000000, result: 0x7fffffff, flags: 0x80000e16,
      edgeInput: 0, edgeResult: 0xffffffff, edgeFlags: 0x80000697},
  ]) {
    const f = fixture([0x83, variant.opcode, 1, 0x83, 0xf9, 0, 0x90],
      {independent: false, fpControl: 0x127f, fpStatus: 0x4720,
        registers: [variant.input, 1, 0xa1b2c3d4, 0x55667788, 0x2f00, 0x13579bdf, 0x2468ace0, 0x0f1e2d3c]});
    let active = false;
    try {
      const api = f.engine.api;
      f.engine.ok('cpu_begin', ...KEY); active = true;
      const context = api.cpu_context_ptr() >>> 0;
      f.slab().setUint32(48, 3, true);
      f.engine.ok('cpu_prepare', ...KEY, 0x1000, 3);
      const bytes = new Uint8Array(f.engine.memory.buffer, api.cpu_code_ptr() >>> 0, api.cpu_code_len() >>> 0);
      const unit = new WebAssembly.Instance(new WebAssembly.Module(bytes), {env: {memory: f.engine.memory},
        cpu: {read: api.cpu_runtime_read, write: api.cpu_runtime_write}});
      const expected = new Uint8Array(f.engine.memory.buffer).slice();
      const initial = expected.slice(context, context + 160), state = new DataView(expected.buffer, context, 160);
      const rows = [
        {pc: 0x1000, quota: 0, input: variant.input, before: 0x80000603, result: variant.input, after: 0x80000603, next: 0x1000, retired: 0, status: 2},
        {pc: 0x1000, quota: 1, input: variant.input, before: 0x80000603, result: variant.result, after: variant.flags, next: 0x1003, retired: 1, status: 2},
        {pc: 0x1000, quota: 2, input: variant.input, before: 0x80000603, result: variant.result, after: 0x80000602, next: 0x1006, retired: 2, status: 2},
        {pc: 0x1000, quota: 3, input: variant.input, before: 0x80000603, result: variant.result, after: 0x80000602, next: 0x1007, retired: 3, status: 0},
        {pc: 0x1003, quota: 0, input: 0x12345678, before: 0x80000800, result: 0x12345678, after: 0x80000800, next: 0x1003, retired: 0, status: 2},
        {pc: 0x1000, quota: 1, input: variant.edgeInput, before: 0x80000603, result: variant.edgeResult, after: variant.edgeFlags, next: 0x1003, retired: 1, status: 2},
      ];
      for (const row of rows) {
        new Uint8Array(f.engine.memory.buffer, context, 160).set(initial);
        new Uint8Array(expected.buffer, context, 160).set(initial);
        for (const [at, value] of [[8, row.input], [40, row.pc], [44, row.before], [48, row.quota]]) {
          f.slab().setUint32(at, value, true); state.setUint32(at, value, true);
        }
        for (const [at, value] of [[8, row.result], [40, row.next], [44, row.after], [48, 0], [52, row.status], [56, row.retired]])
          state.setUint32(at, value, true);
        assert.equal(unit.exports.run(context), row.status);
        assert.deepEqual(new Uint8Array(f.engine.memory.buffer), expected,
          `${variant.name} PC${row.pc.toString(16)} quota${row.quota}: exact scalar state, opaque FLAGS, RAM and all native/process numeric banks`);
      }
    } finally {
      if (active) f.engine.ok('cpu_end', ...KEY);
      f.close();
    }
  }
});
