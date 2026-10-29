// Browser-compatible counterpart of frozen ring3-v1.mjs. No engine is run on import.
// Parent supplies the pinned WebAssembly.Module and shared makeKernel() result.
const assert = {
  ok(value, label = 'condition') { if (!value) throw new Error(label); },
  equal(actual, expected, label = 'equal') { if (actual !== expected) throw new Error(`${label}: ${String(actual)} != ${String(expected)}`); },
  deepEqual(actual, expected, label = 'exact bytes/array') {
    if (actual.length !== expected.length || !actual.every((value, i) => value === expected[i])) throw new Error(label);
  },
};
const KEY = [0x81342715, 0x62594837], ARENA = 4796, FP = 4236, TRANSFER = 140, WORK = 4588;
const pair = n => [Number(n & 0xffffffffn), Number(n >> 32n)];
const hex = bytes => Array.from(bytes, b => b.toString(16).padStart(2, '0')).join('');

export async function createRing3(module, k, { mode = 'compiled' } = {}) {
  assert.ok(module instanceof WebAssembly.Module, 'compiled engine module');
  assert.ok(['checked', 'compiled'].includes(mode), 'receipt mode');
  assert.ok(Number.isInteger(k.N) && k.N >= 1 && k.N <= 131072, 'bounded same fixed work');
  function header(raw, magic, size, versions = [1]) {
    const d = new DataView(raw.buffer, raw.byteOffset, raw.byteLength);
    assert.equal(raw.length, size); assert.equal(String.fromCharCode(...raw.subarray(0, 4)), magic);
    assert.ok(versions.includes(d.getUint16(4, true))); assert.equal(d.getUint16(6, true), 1);
    assert.equal(d.getUint32(8, true), size); assert.equal(d.getUint32(12, true), 0); return d;
  }
  async function fresh(k) {
    const instance = await WebAssembly.instantiate(module, { ring3_observability: { now_ns: () => 0n } });
    const native = instance.exports, api = Object.freeze(Object.fromEntries(Object.entries(native)
      .filter(([n]) => n.startsWith('ring3_abi_v1_')).map(([n, v]) => [n.slice(13), v])));
    const c = { api, native, memory: native.memory, table: new WebAssembly.Table({ element: 'anyfunc', initial: 16, maximum: 16 }), units: [] };
    c.ok = (n, ...a) => assert.equal(api[n](...a), 0, n);
    c.bytes = (offset, len) => new Uint8Array(c.memory.buffer, c.base + offset, len).slice();
    c.put = b => new Uint8Array(c.memory.buffer, c.base + TRANSFER, b.length).set(b);
    try {
      c.ok('open', 16, ...KEY); c.open = true; c.base = api.arena_ptr() >>> 0;
      assert.equal(api.arena_len() >>> 0, ARENA);
      for (const [address, perms] of [[0x1000, 7], [0x3000, 3], [0x4000, 3]]) c.ok('map', address, 1, perms);
      c.ok('configure_execution', 16, 1048576, 8);
      for (const [address, data] of [[0x1000, k.code], [0x3000, k.data], [0x4000, k.stack]]) { c.put(data); c.ok('upload', address, data.length); }
      function install(specs, slot) {
        const payload = new Uint8Array(specs.length * 8), v = new DataView(payload.buffer);
        specs.flat().forEach((n, i) => v.setUint32(i * 4, n, true)); c.put(payload);
        c.ok('compile_resident_with_gates', specs.length, 0);
        const descriptor = c.bytes(TRANSFER, 24), d = new DataView(descriptor.buffer);
        assert.equal(d.getUint32(0, true), 1); assert.equal(d.getUint32(4, true), 24);
        const id = d.getBigUint64(8, true), pointer = d.getUint32(16, true), size = d.getUint32(20, true);
        const raw = new Uint8Array(c.memory.buffer).slice(pointer, pointer + size), m = new WebAssembly.Module(raw), ring3 = {};
        const imports = WebAssembly.Module.imports(m);
        for (const imp of imports) {
          assert.ok(imp.module === 'ring3' && imp.kind === 'function' || imp.module === 'env' && imp.name === 'memory' && imp.kind === 'memory');
          if (imp.module === 'ring3') { ring3[imp.name] = api[imp.name]; assert.equal(ring3[imp.name], native['ring3_abi_v1_' + imp.name]); }
        }
        const run = new WebAssembly.Instance(m, { env: { memory: c.memory }, ring3 }).exports.run; c.table.set(slot, run);
        c.ok('acknowledge_resident_installation', ...KEY, ...pair(id), slot);
        const admitted = header(c.bytes(TRANSFER, 32), 'R3IN', 32);
        assert.equal(admitted.getBigUint64(16, true), id); assert.equal(admitted.getUint32(24, true), slot);
        const unit = { id, slot, descriptor_hex: hex(descriptor), wasm_hex: hex(raw), specs, imports };
        c.units.push(unit); return unit;
      }
      c.terminal = install([[k.terminal, 2]], 0); c.entry = install(k.blockSpecs, 1);
      c.ok('dispatcher_module', ...KEY); const descriptor = c.bytes(TRANSFER, 32), d = header(descriptor, 'R3DP', 32);
      const raw = new Uint8Array(c.memory.buffer).slice(d.getUint32(24, true), d.getUint32(24, true) + d.getUint32(28, true));
      const dm = new WebAssembly.Module(raw), ring3 = {};
      for (const imp of WebAssembly.Module.imports(dm)) if (imp.module === 'ring3') { ring3[imp.name] = api[imp.name]; assert.equal(ring3[imp.name], native['ring3_abi_v1_' + imp.name]); }
      c.dispatch = new WebAssembly.Instance(dm, { env: { memory: c.memory, table: c.table }, ring3 }).exports.run;
      c.dispatcher = { wasm_hex: hex(raw), imports: WebAssembly.Module.imports(dm) };
      c.initial = c.bytes(0, ARENA); return c;
    } catch (e) { if (c.open) c.ok('close'); throw e; }
  }
  function expectedFp(k) {
    const out = new Uint8Array(128), d = new DataView(out.buffer); out.set([0x52, 0x33, 0x46, 0x50]);
    d.setUint16(4, 1, true); d.setUint16(6, 1, true); d.setUint32(8, 128, true);
    d.setUint16(16, k.cw, true); d.setUint16(20, 0xffff, true);
    if (k.fpLast) {
      assert.equal(k.kind, 'fp'); d.setUint16(22, k.fpLast.fop, true); d.setUint32(24, k.fpLast.fip, true); d.setUint32(28, k.fpLast.fdp, true);
      d.setUint16(32, 0x1b, true); d.setUint16(34, 0x23, true);
      d.setBigUint64(110, 0x8000000000000000n, true); d.setUint16(118, 0x4002, true); // stale physical ST7 contains exact8 after pop
    }
    return out;
  }
  function readPages(c) {
    const arena = c.bytes(0, ARENA), pages = [];
    try {
      for (const address of [0x1000, 0x3000, 0x4000]) {
        const out = new Uint8Array(4096), d = new DataView(out.buffer);
        for (let i = 0; i < 4096; i += 4) { c.ok('read32', address + i); const r = new DataView(c.bytes(100, 40).buffer); assert.equal(r.getUint32(16, true), 0); d.setUint32(i, r.getUint32(20, true), true); }
        pages.push(out);
      }
    } finally { new Uint8Array(c.memory.buffer, c.base, ARENA).set(arena); }
    return pages;
  }
  function execute(c, k, phase, index) {
    for (const [address, data] of [[0x3000, k.data], [0x4000, k.stack]]) { c.put(data); c.ok('upload', address, data.length); }
    new Uint8Array(c.memory.buffer, c.base, ARENA).set(c.initial);
    const d = new DataView(c.memory.buffer, c.base, ARENA), state = new Uint8Array(c.memory.buffer, c.base, 56); state.fill(0); state.set([0x52, 0x33, 0x53, 0x54]);
    d.setUint16(4, 3, true); d.setUint16(6, 1, true); d.setUint32(8, 56, true);
    k.regs.forEach((n, i) => d.setUint32(16 + i * 4, n, true)); d.setUint32(48, k.entry, true); d.setUint32(52, 2, true); d.setUint32(96, 0, true);
    new Uint8Array(c.memory.buffer, c.base + FP, 128).set(expectedFp({ ...k, fpLast: null }));
    const budget = k.retired + 16, t0 = performance.now();
    c.ok(mode === 'compiled' ? 'begin_compiled_execution_receipt' : 'begin_execution_receipt', ...KEY, budget, ...pair(c.entry.id), 1);
    const t1 = performance.now(), frame = c.bytes(TRANSFER, 32), ef = header(frame, 'R3EF', 32), token = ef.getBigUint64(16, true);
    const start = performance.now(), status = c.dispatch(c.base, c.base + 56, budget, c.base + 96), end = performance.now();
    const outerExit = c.bytes(56, 40), work = c.bytes(WORK, 24), f0 = performance.now();
    const finish = c.api.finish_execution_receipt(...KEY, ...pair(token)), f1 = performance.now();
    const receipt = c.bytes(TRANSFER, 64), arena = c.bytes(0, ARENA), pages = readPages(c);
    const raw = { phase, index, status, finish, begin_ms: t1 - t0, dispatch_ms: end - start, finish_ms: f1 - f0,
      frame_hex: hex(frame), receipt_hex: hex(receipt), outer_exit_hex: hex(outerExit), work_hex: hex(work), arena_hex: hex(arena),
      code_hex: hex(pages[0]), data_hex: hex(pages[1]), stack_hex: hex(pages[2]) };
    c.lastRun = raw; // retain failed outputs before any semantic assertion
    assert.equal(status, 0); assert.equal(finish, 0);
    const er = header(receipt, 'R3ER', 64), ex = header(outerExit, 'R3EX', 40, [7]);
    assert.equal(er.getUint32(36, true), k.retired); assert.equal(er.getUint32(40, true), 0);
    assert.equal(er.getUint32(44, true), 20); assert.equal(er.getBigUint64(24, true), c.terminal.id); assert.equal(er.getUint32(32, true), 0);
    assert.equal(er.getUint32(48, true), 3); assert.ok(er.getBigUint64(16, true) > ef.getBigUint64(24, true));
    assert.equal(ex.getUint32(16, true), 20); assert.equal(ex.getUint32(20, true), k.retired);
    assert.deepEqual(outerExit.subarray(24), new Uint8Array(16)); header(work, 'R3WK', 24); assert.deepEqual(work.subarray(16), new Uint8Array(8));
    const actualState = new DataView(arena.buffer, arena.byteOffset, 56); header(arena.subarray(0, 56), 'R3ST', 56, [3]);
    assert.deepEqual(Array.from({ length: 8 }, (_, i) => actualState.getUint32(16 + i * 4, true)), k.expectedRegs);
    assert.equal(actualState.getUint32(48, true), k.terminal); assert.equal(actualState.getUint32(52, true), k.expectedFlags);
    assert.deepEqual(arena.subarray(FP, FP + 128), expectedFp(k), 'complete literal FP80 bank/metadata');
    assert.deepEqual(arena.subarray(FP + 128, WORK), c.initial.subarray(FP + 128, WORK)); assert.deepEqual(arena.subarray(WORK + 24), c.initial.subarray(WORK + 24));
    assert.deepEqual(pages[0], k.code); assert.deepEqual(pages[1], k.expectedData); assert.deepEqual(pages[2], k.expectedStack);
    raw.entered_children = String(er.getBigUint64(16, true) - ef.getBigUint64(24, true)); raw.retired = k.retired; raw.validation = 'PASS'; return raw;
  }

  const c = await fresh(k);
  return {
    run(phase = 'timed', index = 0) { assert.ok(c.open, 'open context'); return execute(c, k, phase, index); },
    close() { if (c.open) { c.ok('close'); c.open = false; } },
    get lastRun() { return c.lastRun ?? null; },
    units: c.units.map(u => ({ ...u, id: String(u.id) })),
    dispatcher: c.dispatcher,
    mode,
  };
}
