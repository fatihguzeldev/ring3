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
  } finally {f.close();}
});

const directServiceSkip = skip || (!WebAssembly.Module.exports(artifact.module)
  .some(item => item.name === 'ring3_abi_v1_cpu_runtime_instruction') && 'artifact lacks the direct instruction service');

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
