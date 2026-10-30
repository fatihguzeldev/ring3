import assert from 'node:assert/strict';
import test from 'node:test';
import {existsSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import {readEngine} from '../../engine/tests/support/wasm/engine.mjs';
import {Engine, KEY, record, view} from '../runtime/engine.js';
import {IndependentCodeCache} from '../runtime/independent.js';

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
  registers = [0, 0, 0, 0, 0x2f00, 0, 0, 0], fpControl, fpStatus} = {}) {
  const instance = new WebAssembly.Instance(artifact.module, {ring3_observability: {now_ns: () => 0n}});
  const api = Object.fromEntries(Object.entries(instance.exports)
    .filter(([name]) => name.startsWith('ring3_abi_v1_')).map(([name, value]) => [name.slice(13), value]));
  const engine = new Engine(api, instance.exports.memory, artifact.sha256);
  engine.ok('open', 16, ...KEY);
  engine.base = api.arena_ptr() >>> 0;
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
  const cache = new IndependentCodeCache(engine, execution);
  let closed = false;
  return {engine, cache, slab: () => new DataView(engine.memory.buffer, api.cpu_context_ptr() >>> 0, 160),
    close() {
      if (closed) return;
      assert.equal(api.core_context_ptr(), 0, 'independent execution does not create a legacy core session');
      cache.close(); engine.ok('close'); closed = true;
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
  try {
    f.cache.import();
    const run = f.cache.execute(f.cache.select());
    assert.deepEqual([run.reason, run.retired, run.work, run.pc], [5, 1, 1, 0x1005]);
    assert.equal(f.engine.get(16), 0x12345678);
    assert.deepEqual([f.engine.get(80), f.engine.get(84), f.engine.get(88), f.engine.get(92)], [1, 0x3000, 2, 4]);
    f.engine.ok('read16', 0x2ffe); assert.equal(f.engine.get(120), 0xa55a);
    event(f.engine, 0xffffffff, 0x1005, 1);
  } finally {f.close();}
});

test('actual strict numeric fault publishes its event before touching an unmapped operand', {skip}, () => {
  const f = fixture([0xd9, 0x05, 0x00, 0x40, 0x00, 0x00], {fpControl: 0x037e, fpStatus: 1});
  try {
    f.cache.import();
    const run = f.cache.execute(f.cache.select());
    assert.deepEqual([run.reason, run.retired, run.work, run.pc], [12, 0, 0, 0x1000]);
    assert.equal(f.engine.get(80), 0x8081);
    assert.equal(new DataView(f.engine.memory.buffer).getUint16(f.engine.base + 4236 + 18, true), 0x8081);
    event(f.engine, 16, 0x1000, 0);
  } finally {f.close();}
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

test('actual POPFD refuses TF before changing the instruction or stack state', {skip}, () => {
  const f = fixture([0x9d], {data: [[0x2f00, 2], [0x2f01, 1]]});
  try {
    f.cache.import();
    assert.throws(() => f.cache.execute(f.cache.select()), /unsupported independent CPU instruction/);
    assert.deepEqual([f.engine.get(48), f.engine.get(32), f.engine.get(52), f.engine.get(76)], [0x1000, 0x2f00, 2, 0]);
    assert.equal(f.cache.stats().work, 0);
    assert.equal(f.slab().getUint32(48, true), 50000);
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
