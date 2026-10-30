import assert from 'node:assert/strict';
import test from 'node:test';
import {IndependentCodeCache} from '../runtime/independent.js';
import {KEY} from '../runtime/engine.js';

const uleb = value => {
  const bytes = [];
  do { const next = value & 127; value >>>= 7; bytes.push(next | (value ? 128 : 0)); } while (value);
  return bytes;
};
const text = value => [...uleb(value.length), ...new TextEncoder().encode(value)];
const section = (id, bytes) => [id, ...uleb(bytes.length), ...bytes];
function moduleBytes(dispatcher, body) {
  const imports = dispatcher
    ? [2, ...text('env'), ...text('memory'), 2, 0, 1,
      ...text('env'), ...text('functions'), 1, 0x70, 0, 1]
    : [1, ...text('env'), ...text('memory'), 2, 0, 1];
  const code = [0, ...body, 0x0b];
  return new Uint8Array([0, 97, 115, 109, 1, 0, 0, 0,
    ...section(1, [1, 0x60, 1, 0x7f, 1, 0x7f]),
    ...section(2, imports), ...section(3, [1, 0]),
    ...section(7, [1, ...text('run'), 0, 0]),
    ...section(10, [1, ...uleb(code.length), ...code])]);
}
const dispatcher = moduleBytes(true, [0x20, 0, 0x41, 1, 0x11, 0, 0]);
const gateUnit = moduleBytes(false, [
  0x20, 0, 0x41, 5, 0x36, 2, 52,
  0x20, 0, 0x41, 1, 0x36, 2, 56,
  0x20, 0, 0x41, 4, 0x36, 2, 48,
  0x41, 5,
]);

function fixture({version = 1, unit = gateUnit, publishStatus = 0} = {}) {
  const memory = new WebAssembly.Memory({initial: 1}), pointer = 1024, base = 4096;
  new Uint8Array(memory.buffer, base).set(dispatcher);
  new Uint8Array(memory.buffer, base + 512).set(unit);
  let active = false, begins = 0, publishes = 0, ends = 0, prepared = 0;
  const view = () => new DataView(memory.buffer);
  const engine = {memory, get: offset => offset === 48 ? 0x1000 : offset === 72 ? 8 : 0x10001,
    api: {
      cpu_abi: () => version, cpu_context_ptr: () => pointer,
      cpu_dispatch_code_ptr: () => base, cpu_dispatch_code_len: () => dispatcher.length,
      cpu_begin: (...key) => {
        assert.deepEqual(key, KEY); assert.equal(active, false); active = true; begins++;
        const d = view();
        for (const [at, value] of [[0, 1], [4, 160], [40, 0x1000], [52, 0], [56, 0]])
          d.setUint32(pointer + at, value, true);
        return 0;
      },
      cpu_prepare: (...args) => { assert.equal(active, true); assert.deepEqual(args.slice(0, 2), KEY); prepared++; return 0; },
      cpu_unit_slot: () => 1, cpu_code_ptr: () => base + 512, cpu_code_len: () => unit.length,
      cpu_runtime_read: () => 0, cpu_runtime_write: () => 0, cpu_instruction: () => 0,
      cpu_publish: (...key) => {assert.deepEqual(key, KEY); assert.equal(active, true); publishes++; active = false; return publishStatus;},
      cpu_end: (...key) => {assert.deepEqual(key, KEY); ends++; active = false; return 0;},
    },
  };
  return {engine, pointer, view, counters: () => ({active, begins, publishes, ends, prepared})};
}

test('independent backend rejects incompatible capability before creating an execution owner', () => {
  const f = fixture({version: 2});
  assert.throws(() => new IndependentCodeCache(f.engine), /unsupported independent CPU ABI/);
  assert.equal(f.counters().begins, 0);
});

test('real Wasm dispatch publishes an exact gate boundary and reuses its installed function', () => {
  const f = fixture(), cache = new IndependentCodeCache(f.engine, {budget: 5});
  cache.import();
  const selected = cache.select();
  const result = cache.execute(selected);
  assert.deepEqual({reason: result.reason, detail: result.detail, retired: result.retired, work: result.work},
    {reason: 8, detail: 0x10001, retired: 1, work: 1});
  assert.equal(cache.stats().installedUnits, 1);
  cache.import(); cache.execute(cache.select());
  assert.equal(cache.stats().installedUnits, 1);
  assert.deepEqual(f.counters(), {active: false, begins: 2, publishes: 2, ends: 0, prepared: 2});
});

test('malformed state closes its owner and permanently rejects subsequent guest execution', () => {
  const f = fixture(), begin = f.engine.api.cpu_begin;
  f.engine.api.cpu_begin = (...args) => {const result = begin(...args); f.view().setUint32(f.pointer + 4, 159, true); return result;};
  const cache = new IndependentCodeCache(f.engine);
  assert.throws(() => cache.import(), /invalid independent CPU state/);
  assert.equal(f.counters().ends, 1);
  assert.throws(() => cache.import(), /invalid independent CPU state/);
  assert.equal(f.counters().begins, 1);
});

test('a Wasm trap closes the owner without publishing or replaying guest execution', () => {
  const f = fixture({unit: moduleBytes(false, [0x00])});
  const cache = new IndependentCodeCache(f.engine);
  cache.import(); const selected = cache.select();
  assert.throws(() => cache.execute(selected), WebAssembly.RuntimeError);
  assert.deepEqual(f.counters(), {active: false, begins: 1, publishes: 0, ends: 1, prepared: 1});
  assert.throws(() => cache.select(), WebAssembly.RuntimeError);
  cache.close(); assert.equal(f.counters().ends, 1);
});

test('publication failure is terminal after completed guest effects', () => {
  const f = fixture({publishStatus: 18}), cache = new IndependentCodeCache(f.engine, {budget: 5});
  cache.import(); const selected = cache.select();
  assert.throws(() => cache.execute(selected), /cpu_publish returned 18/);
  assert.throws(() => cache.import(), /cpu_publish returned 18/);
  assert.equal(f.counters().begins, 1);
  assert.equal(f.counters().publishes, 1);
});

test('stop publishes prior guest state before rejecting another entry', () => {
  const f = fixture(), controller = new AbortController();
  f.engine.signal = controller.signal;
  const cache = new IndependentCodeCache(f.engine);
  cache.import(); controller.abort(new Error('stopped'));
  assert.throws(() => cache.select(), /stopped/);
  assert.deepEqual(f.counters(), {active: false, begins: 1, publishes: 1, ends: 0, prepared: 0});
  cache.close(); assert.equal(f.counters().publishes, 1);
});

test('a missing binding at the selected entry fails instead of repeatedly selecting the same code', () => {
  const missing = moduleBytes(false, [0x20, 0, 0x41, 1, 0x36, 2, 52, 0x41, 1]);
  const f = fixture({unit: missing}), cache = new IndependentCodeCache(f.engine);
  cache.import();
  assert.throws(() => cache.execute(cache.select()), /did not enter its selected unit/);
  assert.equal(f.counters().ends, 1);
});

test('an instruction continuation must consume work before compiled execution resumes', () => {
  const instruction = moduleBytes(false, [0x20, 0, 0x41, 7, 0x36, 2, 52, 0x41, 7]);
  const f = fixture({unit: instruction});
  f.engine.api.cpu_instruction = () => {f.view().setUint32(f.pointer + 52, 0, true); return 0;};
  const cache = new IndependentCodeCache(f.engine);
  cache.import();
  assert.throws(() => cache.execute(cache.select()), /made no bounded progress/);
  assert.equal(f.counters().ends, 1);
});

test('a precise prepare fault publishes without installing or executing a module', () => {
  const f = fixture(), cache = new IndependentCodeCache(f.engine);
  f.engine.api.cpu_prepare = () => {f.view().setUint32(f.pointer + 52, 3, true); return 20;};
  f.engine.api.cpu_unit_slot = () => {throw new Error('fault selection has no unit slot');};
  cache.import();
  const result = cache.execute(cache.select());
  assert.equal(result.coreReason, 3);
  assert.equal(result.retired, 0);
  assert.equal(result.work, 0);
  assert.equal(result.id, null);
  assert.equal(cache.stats().installedUnits, 0);
  assert.equal(f.counters().publishes, 1);
});

for (const boundary of ['import', 'prepare']) test(`native cache eviction releases the actual table function at ${boundary}`, () => {
  const f = fixture(), victims = [], cache = new IndependentCodeCache(f.engine);
  let slot = 1;
  f.engine.api.cpu_unit_slot = () => slot;
  f.engine.api.cpu_cache_victim = () => victims.shift() ?? 0;
  cache.import();
  const old = cache.select();
  assert.equal(typeof cache.table.get(1), 'function');
  cache.publish();
  slot = 2;
  if (boundary === 'import') victims.push(1);
  else {
    const prepare = f.engine.api.cpu_prepare;
    f.engine.api.cpu_prepare = (...args) => {victims.push(1); return prepare(...args);};
  }
  cache.import();
  const replacement = cache.select();
  assert.equal(cache.table.get(1), null);
  assert.equal(typeof cache.table.get(2), 'function');
  assert.equal(cache.units.has(1), false);
  assert.equal(cache.stats().installedUnits, 1);
  assert.equal(cache.stats().generatedModuleBytes, gateUnit.length * 2);
  assert.equal(cache.stats().retainedModuleBytes, gateUnit.length);
  assert.throws(() => cache.execute(old), /not selected by this owner/);
  assert.equal(replacement.slot, 2);
  cache.close();
});

test('close releases installed functions even when final publication fails', () => {
  const f = fixture({publishStatus: 18}), cache = new IndependentCodeCache(f.engine);
  cache.import(); cache.select();
  assert.throws(() => cache.close(), /cpu_publish returned 18/);
  assert.equal(cache.table.get(1), null);
  assert.equal(cache.units.size, 0);
  assert.equal(cache.closed, true);
  assert.equal(cache.stats().retainedModuleBytes, 0);
});
