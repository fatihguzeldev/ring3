import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {mkdirSync, readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {ARENA_SIZE} from '../../../../support/wasm/arena.mjs';
import {assertEngineExports} from '../../../../support/wasm/exports.mjs';
import {image} from '../producer/scenario.mjs';

const [enginePath, output, root] = process.argv.slice(2);
mkdirSync(output, {recursive: true});
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const engineBytes = readFileSync(enginePath);
const engineHash = hash(engineBytes);
assert.equal(engineHash, process.env.RING3_ENGINE_SHA256);
const engineModule = new WebAssembly.Module(engineBytes);
assertEngineExports(engineModule, root);
const instance = new WebAssembly.Instance(engineModule, {});
const memory = instance.exports.memory;
const api = Object.fromEntries(Object.entries(instance.exports)
  .filter(([name]) => name.startsWith('ring3_abi_v1_'))
  .map(([name, fn]) => [name.slice(13), fn]));
const key = [0x76777879, 0x96979899];
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
const name = text => {
  const data = Buffer.from(text);
  request(data);
  return data.length;
};
const read = address => {
  assert.equal(api.read32(address >>> 0), 0);
  assert.equal(get(116), 0);
  return get(120);
};
const readWords = (address, length) => Array.from({length}, (_, index) => read(address + index * 4));
function wide(address, count) {
  const units = [];
  for (let index = 0; index < count; index++) {
    const word = read(address + (index & ~1) * 2);
    units.push((word >>> ((index & 1) * 16)) & 0xffff);
  }
  return String.fromCharCode(...units);
}
assert.equal(api.configure_cpu_clock(1_000_000_000, 0, 0, 0), 0);
assert.equal(api.configure_module_limits(8, 0x100000, 16, 65536, 0x100000), 0);
assert.equal(api.configure_resources(0x30000000, 16, 32, 8, 0x100000), 0);
assert.equal(api.configure_activation(16, 64, 16, 0x10000, 4096, 16, 0x100000, 0x6a000000, 256), 0);
assert.equal(api.register_kernel32_module(0x70000000), 0);
const guest = image();
assert.equal(guest.calls.length, 36);
writeFileSync(join(output, 'activation-authored.exe'), guest.bytes);
assert.equal(api.begin_module_input(guest.bytes.length, name('activation-authored.exe')), 0);
for (let offset = 0; offset < guest.bytes.length; offset += 4096) {
  const chunk = guest.bytes.subarray(offset, offset + 4096);
  request(chunk);
  assert.equal(api.append_module_input(offset, chunk.length), 0);
}
assert.equal(api.commit_module_input(), 0);
assert.equal(api.start_module_process(name('activation-authored.exe'), 0, 0x7e000000, 4, 0x7f000000), 0);
const rows = [];
for (let start = 0, total = 1; start < total;) {
  assert.equal(api.module_gates_page(start, 128), 0);
  const page = take(4096);
  assert.equal(page.subarray(0, 4).toString(), 'R3GP');
  assert.equal(page.readUInt32LE(20), start);
  const count = page.readUInt32LE(24);
  total = page.readUInt32LE(16);
  assert.ok(count > 0 && count <= 128 && start + count <= total);
  for (let index = 0; index < count; index++) {
    rows.push([page.readUInt32LE(36 + index * 8), page.readUInt32LE(32 + index * 8)]);
  }
  start += count;
}
const gates = new Map(rows);
assert.equal(gates.size, rows.length);
assert.equal(new Set(rows.map(([, entry]) => entry)).size, rows.length);
const calls = [], runs = [];
let alias;
function compile(call) {
  const pc = get(48), target = gates.get(call.id);
  assert.ok(target, call.name);
  const descriptors = Buffer.alloc(24);
  descriptors.writeUInt32LE(pc, 0);
  descriptors.writeUInt32LE(guest.entry + call.end - pc, 4);
  descriptors.writeUInt32LE(target, 8);
  descriptors.writeUInt32LE(2, 12);
  descriptors.writeUInt32LE(target, 16);
  descriptors.writeUInt32LE(call.id, 20);
  request(descriptors);
  assert.equal(api.compile_with_gates(2, 1), 0, call.name);
  const compiled = bytes().slice(api.module_ptr() >>> 0, (api.module_ptr() >>> 0) + api.module_len());
  const module = new WebAssembly.Module(compiled);
  const bindings = Object.fromEntries(WebAssembly.Module.imports(module)
    .filter(item => item.module === 'ring3')
    .map(item => {
      assert.equal(typeof api[item.name], 'function');
      return [item.name, api[item.name]];
    }));
  writeFileSync(join(output, `compiled-${calls.length + 1}.wasm`), compiled);
  return new WebAssembly.Instance(module, {env: {memory}, ring3: bindings}).exports;
}
function run(generated, call) {
  let retired = 0;
  for (let iteration = 0; iteration < 8; iteration++) {
    const result = generated.run(base, base + 56, 1000, base + 96);
    const reason = get(72), detail = get(80);
    retired += get(76);
    runs.push({name: call.name, result, reason, detail, retired: get(76)});
    if (reason === 3) continue;
    assert.equal(reason, 8, call.name);
    assert.equal(detail, call.id);
    assert.ok(retired > 0);
    return;
  }
  assert.fail(`${call.name}: bounded guest execution`);
}
function check(call) {
  const a = guest.addresses;
  switch (call.hook) {
    case 'no-current': assert.equal(read(a.current), 0); break;
    case 'cookie': assert.ok(read(a.cookie) > 0); break;
    case 'null-cookie': assert.notEqual(read(a.nullCookie), read(a.cookie)); break;
    case 'current': assert.equal(read(a.current), read(a.context)); break;
    case 'com': {
      const section = read(a.find + 8);
      assert.deepEqual(readWords(section, 7), [120, 0, 4, 0x12345678, 0xdef09abc, 0x04030201, 0x08070605]);
      alias = readWords(section + 28, 4);
      assert.notDeepEqual(alias, [0x12345678, 0xdef09abc, 0x04030201, 0x08070605]);
      assert.equal(read(a.find + 32), read(a.context));
      assert.equal(read(a.find + 36), 1);
      break;
    }
    case 'dll': assert.deepEqual(readWords(read(a.find + 8), 5), [20, 2, 0, 0, 0]); break;
    case 'progid': {
      const section = read(a.find + 8), base = read(a.find + 24);
      assert.equal(read(section), 12);
      assert.deepEqual(readWords(base + read(section + 8), 4), alias);
      break;
    }
    case 'window': {
      const section = read(a.find + 8);
      assert.equal(read(section), 24);
      assert.equal(wide(section + read(section + 12), read(section + 8) / 2), '1.2.3.4!Ring3.Window');
      break;
    }
    case 'basic': assert.deepEqual(readWords(a.query, 2), [read(a.context), 0]); break;
    case 'required': assert.ok(read(a.written) > 48); break;
    case 'detailed':
      assert.equal(read(a.query + 8), 1);
      assert.equal(read(a.query + 12), 1);
      assert.ok(read(a.written) > 48);
      break;
    case 'assembly':
      assert.equal(read(a.query + 44), 1);
      assert.equal(read(a.query + 80), 1);
      assert.equal(wide(read(a.query + 64), read(a.query + 4) / 2), 'Ring3.Activation,type="win32",version="1.2.3.4"');
      break;
    case 'file':
      assert.deepEqual(readWords(a.query, 5), [2, 22, 0, a.query + 20, 0]);
      assert.equal(wide(read(a.query + 12), 11), 'private.dll');
      assert.equal(read(a.written), 0);
      break;
    case 'runlevel': assert.deepEqual(readWords(a.query, 3), [0, 1, 0]); break;
    case 'compatibility': assert.deepEqual(readWords(a.query, 6), [1, 0x8e0f7a12, 0x4fe8bfb3, 0xfd48a5b9, 0x9a5aa150, 1]); break;
    case 'setting':
      assert.equal(wide(a.settings, 5), 'true\0');
      assert.equal(read(a.written), 5);
      break;
  }
}
for (const call of guest.calls) {
  const generated = compile(call), generation = api.generation();
  run(generated, call);
  assert.equal(api.capture_windows_call(...key, generation), 0);
  const token = take(112).readUInt32LE(16);
  assert.equal(api.complete_windows_call(...key, generation, token), 0, call.name);
  if (call.expected === 'handle') assert.ok(get(16) >= 0x58000000 && get(16) !== 0xffffffff, call.name);
  else if (call.expected === 'exit') assert.equal(get(72), 9);
  else if (call.expected !== 'void') assert.equal(get(16), call.expected, call.name);
  if (call.expected !== 'exit') check(call);
  calls.push({name: call.name, id: call.id, result: get(16), generation});
  writeFileSync(join(output, 'progress.json'), JSON.stringify({calls, runs}, null, 2));
}
assert.equal(calls.length, 36);
const receipt = {engineHash, guestHash: hash(guest.bytes), calls, runs,
  claim: 'authored PE32/generated Wasm exercise resource-backed activation, per-thread stacks, native section layouts, COM aliases, queries, settings and reference lifetime without host registry or filesystem access'};
writeFileSync(join(output, 'receipt.json'), JSON.stringify(receipt, null, 2));
console.log(JSON.stringify({engineHash, guestCalls: calls.length, guestRuns: runs.length}));
