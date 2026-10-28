import assert from 'node:assert/strict';
import test from 'node:test';
import {V86CodeCache} from '../runtime/v86/index.js';

function fixture(traps, gate = 0x40232) {
  const memory = new WebAssembly.Memory({initial: 1}), data = new DataView(memory.buffer);
  const events = [], base = 0x4000, result = new DataView(new ArrayBuffer(80));
  let currentGate = gate, currentFlags = 1;
  const cache = Object.assign(Object.create(V86CodeCache.prototype), {
    memory, budget: 1000, imported: true, closed: false, open: false,
    deferDirty: true, dirtyPending: false, deferredDirtyPublications: 0,
    runs: 0, regions: 0, retired: 0, work: 0, gates: 0, faults: 0,
    dirtyPages: 0, cowFaults: 0, traps: {pending: null},
  });
  cache.engine = {
    memory, base,
    get(offset) {return data.getUint32(base + offset, true);},
    resultView() {return result;},
    ok(operation, ...args) {
      if (operation === 'component_cpu_gate') {
        result.setUint32(0, currentGate, true);result.setUint32(4, currentFlags, true);
      }
      else if (operation === 'component_cpu_write_fault') events.push('cow');
      else if (operation === 'component_cpu_publish') {
        events.push('publish');
        data.setUint32(base + 72, args[2] === 7 ? 8 : 0, true);
        data.setUint32(base + 80, args[5], true);
      } else throw new Error('unexpected operation ' + operation);
    },
  };
  cache.tables = {
    dirtyGateFlagsSupported: true,
    calls: 0, released: 0, fail: false,
    publishDirty() {
      events.push('dirty');this.calls++;
      if (this.fail) throw new Error('publication failed');
      return 1;
    },
    isCopyOnWrite: () => true,
    faultReason: () => 2,
    release() {events.push('release');this.released++;},
  };
  cache.cpu = {
    wm: {exports: {
      get_hypercall_page_ptr: () => 0x200,
      get_eflags: () => 2,
      fpu_load_status_word: () => 0,
      fpu_load_tag_word: () => 0xffff,
    }},
    main_loop() {
      events.push('cpu');data.setUint32(664, data.getUint32(664, true) + 3, true);
      cache.traps.pending = traps.shift() ?? null;
    },
  };
  // the fixture supplies the next canonical state without running another CPU batch.
  cache.import = function() {events.push('import');this.traps.pending = null;this.imported = true;};
  return {cache, events, setGate(value, flags = 0) {currentGate = value;currentFlags = flags;}};
}

const gateTrap = () => ({vector: 6, pc: 0x1000, flags: 2, esp: 0, cs: 0, ss: 0});

test('SetRenderState defers dirty publication until the next non-allowlisted gate', () => {
  const f = fixture([gateTrap(), gateTrap()]);
  const first = f.cache.execute();
  assert.equal(first.coreReason, 7);
  assert.equal(f.cache.tables.calls, 0);
  assert.equal(f.cache.dirtyPending, true);
  assert.equal(f.cache.deferredDirtyPublications, 1);
  assert.deepEqual(f.events, ['cpu', 'publish']);
  f.setGate(0x10001, 0);
  f.cache.imported = true;
  f.cache.execute();
  assert.equal(f.cache.tables.calls, 1);
  assert.equal(f.cache.dirtyPending, false);
  assert.equal(f.cache.deferredDirtyPublications, 1);
  assert.deepEqual(f.events.slice(2), ['cpu', 'dirty', 'publish']);
});

test('copy-on-write publishes old dirty evidence before the native split and reimport', () => {
  const cow = {vector: 14, pc: 0x1000, flags: 2, esp: 0, cs: 0, ss: 0, address: 0x7000, error: 2};
  const f = fixture([cow, gateTrap()]);
  f.cache.execute();
  assert.deepEqual(f.events, ['cpu', 'dirty', 'cow', 'import', 'cpu', 'publish']);
  assert.equal(f.cache.cowFaults, 1);
  assert.equal(f.cache.tables.calls, 1);
  assert.equal(f.cache.dirtyPending, true, 'the later allowlisted batch has its own pending evidence');
  assert.equal(f.cache.deferredDirtyPublications, 1);
  f.cache.publish();
  assert.equal(f.cache.tables.calls, 2);
  assert.equal(f.cache.dirtyPending, false);
});

test('a known gate without the native safety flag cannot defer publication', () => {
  const f = fixture([gateTrap()]);
  f.setGate(0x40232, 0);
  f.cache.execute();
  assert.equal(f.cache.tables.calls, 1);
  assert.equal(f.cache.dirtyPending, false);
  assert.equal(f.cache.deferredDirtyPublications, 0);
  assert.deepEqual(f.events, ['cpu', 'dirty', 'publish']);
});

test('a legacy host cannot authorize deferral through stale transfer bytes', () => {
  const f = fixture([gateTrap()]);
  f.cache.tables.dirtyGateFlagsSupported = false;
  f.setGate(0x40232, 1);
  f.cache.execute();
  assert.equal(f.cache.tables.calls, 1);
  assert.equal(f.cache.dirtyPending, false);
  assert.equal(f.cache.deferredDirtyPublications, 0);
});

test('mode changes and close preserve pending evidence when publication fails', () => {
  const f = fixture([]);
  f.cache.dirtyPending = true;
  for (const owner of [undefined, {paused: false, open: false}, {paused: true, open: true}])
    assert.throws(() => f.cache.configureDirtyDeferral(false, owner), /paused/);
  assert.equal(f.cache.tables.calls, 0);
  f.cache.tables.fail = true;
  assert.throws(() => f.cache.configureDirtyDeferral(false, {paused: true, open: false}), /publication failed/);
  assert.equal(f.cache.deferDirty, true);
  assert.equal(f.cache.dirtyPending, true);
  assert.throws(() => f.cache.close(), /publication failed/);
  assert.equal(f.cache.closed, false);
  assert.equal(f.cache.tables.released, 0);
  f.cache.tables.fail = false;
  assert.equal(f.cache.configureDirtyDeferral(false, {paused: true, open: false}), false);
  assert.equal(f.cache.dirtyPending, false);
  f.cache.dirtyPending = true;
  f.cache.close();f.cache.close();
  assert.equal(f.cache.tables.released, 1);
  assert.equal(f.cache.closed, true);
  assert.equal(f.cache.dirtyPending, false);
  assert.deepEqual(f.events.slice(-2), ['dirty', 'release']);
});
