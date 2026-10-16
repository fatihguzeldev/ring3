import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {writeFileSync} from 'node:fs';
import {join} from 'node:path';

export const pair = value => [Number(value & 0xffffffffn), Number(value >> 32n)];
export function cache(host, output) {
  const {api, memory, key, base, take, get} = host;
  const slots = new Map(), digest = createHash('sha256');
  let admissions = 0, evictions = 0, runs = 0;
  function entry(pc) {
    for (let attempt = 0; attempt < 16; attempt++) {
      host.ok('prepare_cache_entry', ...key, pc);
      const packet = take(64);
      assert.equal(packet.subarray(0, 4).toString(), 'R3CQ');
      const kind = packet.readUInt32LE(24), serial = packet.readBigUInt64LE(16);
      const id = packet.readBigUInt64LE(32), slot = packet.readUInt32LE(48);
      if (kind === 0) {
        const existing = slots.get(slot);
        assert.ok(existing && existing.id === id, 'cache hit must name the exact installed host function');
        return existing;
      }
      if (kind === 2) {
        const victim = packet.readBigUInt64LE(40), oldSlot = packet.readUInt32LE(52);
        assert.equal(slots.get(oldSlot)?.id, victim);
        slots.delete(oldSlot);
        host.ok('acknowledge_cache_eviction', ...key, ...pair(serial), ...pair(victim), oldSlot);
        evictions++;
        continue;
      }
      assert.equal(kind, 1);
      host.ok('resident_module', ...pair(id));
      const record = take(24), pointer = record.readUInt32LE(16), length = record.readUInt32LE(20);
      const bytes = new Uint8Array(memory.buffer).slice(pointer, pointer + length);
      const module = new WebAssembly.Module(bytes), bindings = {};
      for (const item of WebAssembly.Module.imports(module)) {
        if (item.module === 'ring3') {
          assert.equal(typeof api[item.name], 'function', item.name);
          bindings[item.name] = (...args) => {
            const value = api[item.name](...args);
            if (item.name === 'guard_detect_resident') host.epoch = value;
            return value;
          };
        } else assert.ok(item.module === 'env' && item.name === 'memory');
      }
      const generated = new WebAssembly.Instance(module, {env: {memory}, ring3: bindings}).exports;
      const value = {id, slot, generated};
      slots.set(slot, value);
      host.ok('install_cached_entry', ...key, ...pair(serial), ...pair(id), slot);
      digest.update(bytes);
      if (admissions < 128) writeFileSync(join(output, `compiled-${admissions}-${pc.toString(16)}.wasm`), bytes);
      admissions++;
      return value;
    }
    assert.fail('bounded cache eviction/install exchange');
  }
  function execute(value) {
    host.epoch = 0n;
    const status = value.generated.run(base, base + 56, 10000, base + 96);
    assert.equal(status, 0, `generated transport at ${get(48).toString(16)}`);
    assert.ok(host.epoch > 0n, 'every generated execution owns a fresh detector epoch');
    runs++;
    return {reason: get(72), detail: get(80), retired: get(76), pc: get(48), id: value.id.toString()};
  }
  return {
    entry,
    execute,
    isInstalled: unit => slots.get(unit.slot)?.id === unit.id,
    stats: () => ({admissions, evictions, runs}),
    finish: () => digest.digest('hex'),
  };
}
