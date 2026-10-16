import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {mkdirSync, readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {ARENA_SIZE} from '../../../../support/wasm/arena.mjs';
import {assertEngineExports} from '../../../../support/wasm/exports.mjs';
import {unwindImage} from '../producer/unwind.mjs';

const [enginePath, output, root] = process.argv.slice(2);
mkdirSync(output, {recursive: true});
const hash = value => createHash('sha256').update(value).digest('hex');
const engineBytes = readFileSync(enginePath), engineHash = hash(engineBytes);
assert.equal(engineHash, process.env.RING3_ENGINE_SHA256);
const module = new WebAssembly.Module(engineBytes);
assertEngineExports(module, root);
const instance = new WebAssembly.Instance(module, {}), memory = instance.exports.memory;
const api = Object.fromEntries(Object.entries(instance.exports).filter(([name]) => name.startsWith('ring3_abi_v1_')).map(([name, fn]) => [name.slice(13), fn]));
const key = [0x12131415, 0x91929394];
assert.equal(api.open(1024, ...key), 0);
const base = api.arena_ptr() >>> 0;
assert.equal(api.arena_len(), ARENA_SIZE);
const bytes = () => new Uint8Array(memory.buffer);
const get = offset => new DataView(memory.buffer).getUint32(base + offset, true);
const take = length => Buffer.from(bytes().slice(base + 140, base + 140 + length));
const request = data => {assert.ok(data.length <= 4096); bytes().set(data, base + 140);};
const name = value => {const data = Buffer.from(value); request(data); return data.length;};
const read = address => {assert.equal(api.read32(address), 0); assert.equal(get(116), 0); return get(120);};
assert.equal(api.configure_cpu_clock(1_000_000_000, 0, 0, 0), 0);
assert.equal(api.configure_module_limits(8, 0x100000, 16, 65536, 0x100000), 0);
assert.equal(api.register_kernel32_module(0x70000000), 0);
const guest = unwindImage();
writeFileSync(join(output, 'unwind-authored.exe'), guest.bytes);
assert.equal(api.begin_module_input(guest.bytes.length, name('unwind-authored.exe')), 0);
for (let offset = 0; offset < guest.bytes.length; offset += 4096) {
  const chunk = guest.bytes.subarray(offset, offset + 4096);
  request(chunk);
  assert.equal(api.append_module_input(offset, chunk.length), 0);
}
assert.equal(api.commit_module_input(), 0);
assert.equal(api.start_module_process(name('unwind-authored.exe'), 0, 0x7f000000, 4, 0x7e000000), 0);
const wanted = new Set([...guest.ids, 0x121000]), gates = new Map();
for (let start = 0, total = 1; start < total;) {
  assert.equal(api.module_gates_page(start, 128), 0);
  const page = take(4096), count = page.readUInt32LE(24);
  total = page.readUInt32LE(16);
  assert.ok(count > 0);
  for (let index = 0; index < count; index++) {
    const id = page.readUInt32LE(36 + index * 8);
    if (wanted.has(id)) {assert.ok(!gates.has(id)); gates.set(id, page.readUInt32LE(32 + index * 8));}
  }
  start += count;
}
assert.equal(gates.size, wanted.size);
const runs = [], calls = [], handlers = [];
let exited = false;
for (let iteration = 0; iteration < 96 && !exited; iteration++) {
  const entry = get(48), entries = [...new Set([entry, ...gates.values()])];
  assert.ok(entries.length <= 8);
  const data = Buffer.alloc(entries.length * 4 + gates.size * 8);
  entries.forEach((pc, index) => data.writeUInt32LE(pc, index * 4));
  let offset = entries.length * 4;
  for (const [id, gate] of gates) {data.writeUInt32LE(gate, offset); data.writeUInt32LE(id, offset + 4); offset += 8;}
  request(data);
  assert.equal(api.compile_entries(entries.length, gates.size), 0, `entry ${entry.toString(16)}`);
  const generation = api.generation();
  const compiled = bytes().slice(api.module_ptr() >>> 0, (api.module_ptr() >>> 0) + api.module_len());
  writeFileSync(join(output, `compiled-${iteration}.wasm`), compiled);
  const generatedModule = new WebAssembly.Module(compiled);
  const imports = Object.fromEntries(WebAssembly.Module.imports(generatedModule).filter(item => item.module === 'ring3').map(item => [item.name, api[item.name]]));
  const generated = new WebAssembly.Instance(generatedModule, {env: {memory}, ring3: imports}).exports;
  generated.run(base, base + 56, 1000, base + 96);
  const reason = get(72), detail = get(80);
  runs.push({entry, reason, detail, retired: get(76)});
  if (reason === 3) continue;
  assert.equal(reason, 8, `unexpected guest stop ${JSON.stringify(runs.at(-1))}`);
  if (detail === 0x121000) {
    assert.equal(api.pending_windows_exception(), 0);
    const packet = take(64), id = packet.readBigUInt64LE(16);
    assert.equal(packet.subarray(0, 4).toString(), 'R3EH');
    assert.equal(packet.readUInt32LE(12), 1);
    assert.equal(packet.readUInt32LE(24), 1);
    handlers.push({procedure: packet.readUInt32LE(32), flags: read(packet.readUInt32LE(36) + 4), code: read(packet.readUInt32LE(36))});
    assert.equal(api.finish_windows_exception_handler(...key, Number(id & 0xffffffffn), Number(id >> 32n)), 0);
  } else {
    assert.ok(guest.ids.includes(detail));
    assert.equal(api.capture_windows_call(...key, generation), 0);
    const token = take(112).readUInt32LE(16);
    if (detail === 0x10003) {
      const a = guest.addresses;
      assert.equal(read(a.nestedFlags), 2);
      assert.equal(read(a.outerFlags), 0);
      assert.equal(read(a.nestedCalls), 3);
      assert.equal(read(a.outerCalls), 2);
      assert.equal(read(a.autoFlags), 0x10);
      assert.equal(read(a.autoHandled), 1);
      assert.equal(read(a.parentAfter), 1);
      assert.equal(read(a.autoUnwindAfter), 1);
      assert.equal(read(a.autoUnwindResult), 0x11235813);
      assert.equal(read(a.unwindResult), 0x76543210);
      assert.equal(read(a.restoreResult), 0x24681357);
    }
    assert.equal(api.complete_windows_call(...key, generation, token), 0, `gate ${detail.toString(16)}`);
    calls.push(detail);
    exited = detail === 0x10003;
    if (exited) assert.equal(get(72), 9);
  }
}
assert.ok(exited);
assert.deepEqual(calls, guest.expectedCalls);
assert.deepEqual(handlers.map(row => row.procedure), [guest.procedures[0], guest.procedures[1], guest.procedures[2], guest.procedures[0], guest.procedures[0], guest.procedures[1], guest.procedures[2], guest.procedures[0], guest.procedures[1]]);
assert.deepEqual(handlers.map(row => row.flags), [0, 0x10, 0, 0x10, 0, 0, 0, 2, 2]);
const receipt = {engineHash, guestHash: hash(guest.bytes), calls, runs, handlers,
  claim: 'authored PE32 and generated Wasm exercise i386 nested-disposition flag lifetime, nested RaiseException parent restoration, unwind-in-unwind collision/parent retirement, explicit collided registration pop and cdecl context restore with an unreadable optional record; no compiler-produced or native-Windows claim'};
writeFileSync(join(output, 'receipt.json'), JSON.stringify(receipt, null, 2));
console.log(JSON.stringify({engineHash, guestCalls: calls.length, guestRuns: runs.length, handlers: handlers.length}));
