import assert from 'node:assert/strict';
import test from 'node:test';
import { liveModules } from '../runtime/session/diagnostics/modules.js';

function fixture(count = 2, grow = false) {
  const memory = new WebAssembly.Memory({ initial: 1 }), base = 4096, guest = new Map(), handles = new Map(), calls = [];
  const all = () => new Uint8Array(memory.buffer), data = () => new DataView(memory.buffer);
  all().set(Uint8Array.from({ length: 4796 }, (_, i) => i * 7 + 3), base);
  const put = (address, value, width = 4) => {
    for (let i = 0; i < width; i++) guest.set(address + i, (value >>> (8 * i)) & 255);
  };
  const wide = (address, text) => {
    for (let i = 0; i < text.length; i++) put(address + i * 2, text.charCodeAt(i), 2);
    return text.length * 2;
  };
  const descriptor = (address, pointer, text) => {
    const length = wide(pointer, text); put(address, length | ((length + 2) << 16)); put(address + 4, pointer);
  };
  const head = 0x3020c, teb = 0x10000, peb = 0x20000, ldr = 0x30200, entries = [], expected = [];
  put(teb + 0x30, peb); put(peb + 0x0c, ldr);
  for (let i = 0; i < count; i++) {
    const entry = 0x40000 + i * 64, image = 0x10000000 + i * 0x10000, name = `module${i}.dll`, path = `C:\\app\\${name}`;
    entries.push(entry); expected.push({ name, path, base: image, imageSize: 0x3000 });
    put(entry + 24, image); put(entry + 32, 0x3000);
    descriptor(entry + 36, 0x100000 + i * 8192, path);
    descriptor(entry + 44, 0x101000 + i * 8192, name);
    handles.set(path, image); handles.set(name, image);
    put(image, 0x5a4d); put(image + 0x3c, 0x80); put(image + 0x80, 0x4550);
    put(image + 0x98, 0x10b); put(image + 0xd0, 0x3000);
  }
  put(head, entries[0] ?? head); put(head + 4, entries.at(-1) ?? head);
  entries.forEach((entry, i) => { put(entry, entries[i + 1] ?? head); put(entry + 4, entries[i - 1] ?? head); });
  let didGrow = false;
  const engine = { memory, base,
    get: offset => data().getUint32(base + offset, true),
    request: bytes => all().set(bytes, base + 140),
    take: length => all().slice(base + 140, base + 140 + length),
    status(name, address) {
      calls.push([name, address]);
      if (grow && !didGrow) { memory.grow(1); didGrow = true; }
      if (name === 'module_teb' || name === 'module_handle') {
        const handle = name === 'module_teb' ? teb : handles.get(new TextDecoder().decode(all().slice(base + 140, base + 140 + address))) ?? 0;
        all().fill(0, base + 140, base + 212);
        for (const [offset, value] of [[0, 0x4f4d3352], [4, 0x10001], [8, 72], [16, 2], [20, handle]]) data().setUint32(base + 140 + offset, value, true);
        return 0;
      }
      assert(['read8', 'read32'].includes(name));
      const width = name === 'read8' ? 1 : 4;
      let value = 0, valid = true;
      for (let i = 0; i < width; i++) { if (!guest.has(address + i)) valid = false; value |= (guest.get(address + i) ?? 0) << (8 * i); }
      all().fill(0, base + 100, base + 140);
      for (const [offset, word] of [[0, 0x484d3352], [4, width === 1 ? 0x10002 : 0x10001], [8, 40], [16, valid ? 0 : 1], [20, value >>> 0]]) data().setUint32(base + 100 + offset, word, true);
      return 0;
    },
  };
  return { engine, guest, handles, calls, put, head, entries, expected, arena: () => all().slice(base, base + 4796) };
}

test('live modules corroborate actual loader bases and restore the full IO loan after memory growth', () => {
  const f = fixture(2, true), before = f.arena(), ram = [...f.guest];
  const result = liveModules(f.engine);
  assert.equal(result.scope, 'point-in-time loader snapshot; not historical ownership');
  assert.equal(result.status, 'complete'); assert.equal(result.truncated, false);
  assert.deepEqual(result.modules, f.expected); assert.deepEqual(result.errors, []);
  assert.deepEqual(f.arena(), before); assert.deepEqual([...f.guest], ram);
  assert(f.calls.some(([name]) => name === 'module_teb')); assert.equal(f.calls.filter(([name]) => name === 'module_handle').length, 2);
});

test('an empty valid loader list is complete', () => {
  const f = fixture(0), before = f.arena(), result = liveModules(f.engine);
  assert.equal(result.status, 'complete'); assert.deepEqual(result.modules, []); assert.deepEqual(result.errors, []);
  assert.deepEqual(f.arena(), before);
});

test('a cyclic loader list reports partial results without retaining scratch or following the cycle', () => {
  const f = fixture(); f.put(f.entries[1], f.entries[1]);
  const before = f.arena(), result = liveModules(f.engine);
  assert.equal(result.status, 'partial'); assert.equal(result.modules.length, 2);
  assert.match(result.errors.map(error => error.message).join(' '), /cycle/);
  assert(f.calls.length < 256); assert.deepEqual(f.arena(), before);
});

test('wrong owner bases and live PE extents are excluded rather than guessed', () => {
  const f = fixture(3); f.handles.set(f.expected[0].path, 0x70000000); f.handles.set(f.expected[0].name, 0x70000000);
  f.put(f.expected[1].base + 0xd0, 0x4000);
  const before = f.arena(), result = liveModules(f.engine);
  assert.equal(result.status, 'partial'); assert.deepEqual(result.modules, [f.expected[2]]); assert.equal(result.errors.length, 2);
  assert.deepEqual(f.arena(), before);
});

test('invalid string spans, UTF16 and back links fail closed without poisoning the caller', () => {
  for (const corrupt of [
    f => f.put(f.entries[0] + 36, 3 | (4 << 16)),
    f => f.put(f.entries[0] + 40, 0xfffffff0),
    f => f.put(0x100000, 0xd800, 2),
    f => f.put(f.entries[0] + 4, f.entries[0]),
  ]) {
    const f = fixture(); corrupt(f); const before = f.arena(), result = liveModules(f.engine);
    assert.notEqual(result.status, 'complete'); assert(result.errors.length > 0); assert.deepEqual(f.arena(), before);
  }
});

test('module traversal is bounded at1024 entries and marks truncation', () => {
  const f = fixture(1025), before = f.arena(), result = liveModules(f.engine);
  assert.equal(result.status, 'partial'); assert.equal(result.truncated, true); assert.equal(result.modules.length, 1024);
  assert.equal(f.calls.filter(([name]) => name === 'module_handle').length, 1024); assert.deepEqual(f.arena(), before);
});

test('native rejection and malformed packets become unavailable diagnostics and restore scratch', () => {
  for (const malformed of [false, true]) {
    const f = fixture(), before = f.arena(), original = f.engine.status;
    f.engine.status = (...args) => { if (!malformed) return 12; original(...args); new DataView(f.engine.memory.buffer).setUint32(f.engine.base + 140, 0, true); return 0; };
    const result = liveModules(f.engine);
    assert.equal(result.status, 'unavailable'); assert.deepEqual(result.modules, []); assert(result.errors.length > 0);
    assert.deepEqual(f.arena(), before);
  }
});
