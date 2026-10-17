import assert from 'node:assert/strict';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {ARENA_SIZE} from '../../../../support/wasm/arena.mjs';
import {assertEngineExports} from '../../../../support/wasm/exports.mjs';

export function host(enginePath, root, output, guest, options = {}) {
  const engine = new WebAssembly.Module(readFileSync(enginePath));
  assertEngineExports(engine, root);
  const instance = new WebAssembly.Instance(engine, {}), memory = instance.exports.memory;
  const api = Object.fromEntries(Object.entries(instance.exports)
    .filter(([name]) => name.startsWith('ring3_abi_v1_'))
    .map(([name, fn]) => [name.slice(13), fn]));
  const key = [0x82838485, 0xa2a3a4a5];
  assert.equal(api.open(1024, ...key), 0);
  const base = api.arena_ptr() >>> 0;
  assert.equal(api.arena_len(), ARENA_SIZE);
  const bytes = () => new Uint8Array(memory.buffer);
  const get = offset => new DataView(memory.buffer).getUint32(base + offset, true);
  const take = length => Buffer.from(bytes().slice(base + 140, base + 140 + length));
  const request = data => {
    assert.ok(data.length <= 4096);
    bytes().set(data, base + 140);
  };
  const name = value => {const data = Buffer.from(value); request(data); return data.length;};
  const read = address => {
    assert.equal(api.read32(address >>> 0), 0);
    assert.equal(get(116), 0);
    return get(120);
  };
  assert.equal(api.configure_cpu_clock(1_000_000_000, 0, 0, 0), 0);
  assert.equal(api.configure_execution(options.residentSlots ?? 64, 8 * 1024 * 1024, 1), 0);
  assert.equal(api.configure_synchronization(32, 64, 64, 16), 0);
  assert.equal(api.configure_threads(4, 16, 16, 4, 256), 0);
  assert.equal(api.configure_module_limits(options.modules ?? 8, 0x100000, 16, 65536, 0x100000), 0);
  assert.equal(api.register_kernel32_module(0x70000000), 0);
  options.configure?.({api, name, request});
  assert.equal(api.begin_module_input(guest.bytes.length, name('threads-authored.exe')), 0);
  for (let offset = 0; offset < guest.bytes.length; offset += 4096) {
    const chunk = guest.bytes.subarray(offset, offset + 4096);
    request(chunk);
    assert.equal(api.append_module_input(offset, chunk.length), 0);
  }
  assert.equal(api.commit_module_input(), 0);
  assert.equal(api.start_module_process(name('threads-authored.exe'), 0, 0x7e000000, 4, 0x7f000000), 0);
  const rows = [];
  for (let start = 0, total = 1; start < total;) {
    assert.equal(api.module_gates_page(start, 128), 0);
    const page = take(4096), count = page.readUInt32LE(24);
    assert.equal(page.subarray(0, 4).toString(), 'R3GP');
    total = page.readUInt32LE(16);
    assert.ok(count > 0);
    for (let index = 0; index < count; index++) {
      rows.push([page.readUInt32LE(36 + index * 8), page.readUInt32LE(32 + index * 8)]);
    }
    start += count;
  }
  const gates = new Map(rows);
  assert.equal(gates.size, rows.length);
  assert.equal(new Set(rows.map(([, entry]) => entry)).size, rows.length);
  let lastEpoch = 0n;
  function compile(name, specs, selected) {
    const descriptors = Buffer.alloc((specs.length + selected.length) * 8);
    specs.forEach((item, index) => {
      descriptors.writeUInt32LE(item.entry, index * 8);
      descriptors.writeUInt32LE(item.length, index * 8 + 4);
    });
    selected.forEach((item, index) => {
      const offset = (specs.length + index) * 8;
      descriptors.writeUInt32LE(item.entry, offset);
      descriptors.writeUInt32LE(item.id, offset + 4);
    });
    request(descriptors);
    assert.equal(api.compile_resident_with_gates(specs.length, selected.length), 0, name);
    const record = take(24), low = record.readUInt32LE(8), high = record.readUInt32LE(12);
    const pointer = record.readUInt32LE(16), length = record.readUInt32LE(20);
    const compiled = bytes().slice(pointer, pointer + length), module = new WebAssembly.Module(compiled);
    const imports = Object.fromEntries(WebAssembly.Module.imports(module)
      .filter(item => item.module === 'ring3')
      .map(item => {
        assert.equal(typeof api[item.name], 'function');
        if (item.name === 'guard_detect_resident') {
          return [item.name, (...args) => {lastEpoch = api[item.name](...args); return lastEpoch;}];
        }
        return [item.name, api[item.name]];
      }));
    writeFileSync(join(output, `compiled-${name}.wasm`), compiled);
    return {low, high, generated: new WebAssembly.Instance(module, {env: {memory}, ring3: imports}).exports};
  }
  const gateOwners = new Map();
  for (const segment of guest.segments) {
    if (gateOwners.has(segment.id)) continue;
    const target = gates.get(segment.id);
    assert.ok(target, segment.name);
    const owner = compile(`gate-${segment.id.toString(16)}`,
      [{entry: target, length: 2}], [{entry: target, id: segment.id}]);
    gateOwners.set(segment.id, {...owner, target});
  }
  assert.equal(gateOwners.size, options.gateOwners ?? 23);
  const units = guest.segments.map((segment, index) => ({
    ...segment,
    ...compile(`code-${index + 1}`, [{entry: segment.start, length: segment.limit - segment.start}], []),
    gateOwner: gateOwners.get(segment.id),
  }));
  const runs = [], durations = new Map();
  function run(unit) {
    let active = unit;
    for (let iteration = 0; iteration < 8; iteration++) {
      lastEpoch = 0n;
      const result = active.generated.run(base, base + 56, 1000, base + 96);
      assert.ok(lastEpoch > 0n, unit.name);
      const low = Number(lastEpoch & 0xffffffffn), high = Number(lastEpoch >> 32n);
      assert.equal(api.report_windows_thread_execution(low, high, 123400, 0), 0);
      durations.set(unit.thread, (durations.get(unit.thread) ?? 0n) + 123400n);
      const reason = get(72), detail = get(80), retired = get(76);
      runs.push({name: unit.name, thread: unit.thread, phase: active === unit ? 'code' : 'gate',
        resident: [active.low, active.high], result, reason, detail, retired,
        executionEpoch: lastEpoch.toString(), virtualDurationNs: 123400});
      if (reason === 1) continue;
      if (reason === 3) {
        assert.equal(active, unit, 'only the authored code may transfer into its shared gate');
        assert.equal(get(48), unit.gateOwner.target, unit.name);
        assert.ok(retired > 0, 'the authored CALL/RET must retire before gate dispatch');
        active = unit.gateOwner;
        continue;
      }
      assert.equal(active, unit.gateOwner);
      assert.equal(reason, 8, unit.name);
      assert.equal(detail, unit.id);
      assert.equal(retired, 0, 'gate dispatch is an engine control boundary');
      return active;
    }
    assert.fail(`${unit.name}: bounded guest execution`);
  }
  return {api, key, get, take, read, units, runs, durations, run, compile};
}
