import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {mkdirSync, readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {ARENA_SIZE} from '../../../../support/wasm/arena.mjs';
import {assertEngineExports} from '../../../../support/wasm/exports.mjs';
import {image} from '../producer/scenario.mjs';
import {libraryImage} from '../producer/datafiles.mjs';
import {checkDatafile} from './datafiles.mjs';

const [enginePath, output, root] = process.argv.slice(2);
mkdirSync(output, {recursive: true});
const hash = value => createHash('sha256').update(value).digest('hex');
const engineBytes = readFileSync(enginePath), engineHash = hash(engineBytes);
assert.equal(engineHash, process.env.RING3_ENGINE_SHA256);
const engineModule = new WebAssembly.Module(engineBytes);
assertEngineExports(engineModule, root);
const instance = new WebAssembly.Instance(engineModule, {}), memory = instance.exports.memory;
const api = Object.fromEntries(Object.entries(instance.exports)
  .filter(([name]) => name.startsWith('ring3_abi_v1_'))
  .map(([name, fn]) => [name.slice(13), fn]));
const key = [0x62636465, 0x92939495];
assert.equal(api.open(1024, ...key), 0);
const base = api.arena_ptr() >>> 0;
assert.equal(api.arena_len(), ARENA_SIZE);
const bytes = () => new Uint8Array(memory.buffer);
const get = offset => new DataView(memory.buffer).getUint32(base + offset, true);
const take = length => Buffer.from(bytes().slice(base + 140, base + 140 + length));
const request = data => {assert.ok(data.length <= 4096); bytes().set(data, base + 140);};
const name = value => {const data = Buffer.from(value); request(data); return data.length;};
const read = address => {
  assert.equal(api.read32(address >>> 0), 0);
  assert.equal(get(116), 0);
  return get(120);
};
assert.equal(api.configure_cpu_clock(1_000_000_000, 0, 0, 0), 0);
assert.equal(api.configure_module_limits(8, 0x100000, 16, 65536, 0x100000), 0);
assert.equal(api.configure_resources(0x30000000, 16, 32, 8, 0x100000), 0);
assert.equal(api.register_kernel32_module(0x70000000), 0);
assert.equal(api.register_user32_module(0x72000000), 0);
const guest = image();
assert.equal(guest.calls.length, 51);
writeFileSync(join(output, 'resources-authored.exe'), guest.bytes);
const library = libraryImage(guest);
writeFileSync(join(output, 'data-resource.dll'), library);
assert.equal(api.begin_module_input(library.length, name('data-resource.dll')), 0);
for (let offset = 0; offset < library.length; offset += 4096) {
  const chunk = library.subarray(offset, offset + 4096);
  request(chunk); assert.equal(api.append_module_input(offset, chunk.length), 0);
}
assert.equal(api.commit_module_input(), 0);
assert.equal(api.begin_module_input(guest.bytes.length, name('resources-authored.exe')), 0);
for (let offset = 0; offset < guest.bytes.length; offset += 4096) {
  const chunk = guest.bytes.subarray(offset, offset + 4096);
  request(chunk); assert.equal(api.append_module_input(offset, chunk.length), 0);
}
assert.equal(api.commit_module_input(), 0);
assert.equal(api.start_module_process(name('resources-authored.exe'), 0, 0x7e000000, 4, 0x7f000000), 0);
const rows = [];
for (let start = 0, total = 1; start < total;) {
  assert.equal(api.module_gates_page(start, 128), 0);
  const page = take(4096), count = page.readUInt32LE(24);
  assert.equal(page.subarray(0, 4).toString(), 'R3GP');
  total = page.readUInt32LE(16); assert.ok(count > 0);
  for (let index = 0; index < count; index++) rows.push([
    page.readUInt32LE(36 + index * 8), page.readUInt32LE(32 + index * 8),
  ]);
  start += count;
}
const gates = new Map(rows), calls = [], runs = [], callbacks = [];
assert.equal(gates.size, rows.length);
assert.equal(new Set(rows.map(([, entry]) => entry)).size, rows.length);
function compile(call) {
  const pc = get(48), target = gates.get(call.id);
  assert.ok(target, call.name);
  const selectedCallback = call.hook === 'types' ? guest.callbacks[0]
    : call.hook === 'languages' ? guest.callbacks[1] : undefined;
  const specs = [{entry: pc, length: guest.entry + call.end - pc}, {entry: target, length: 2}];
  const selected = [{entry: target, id: call.id}];
  if (selectedCallback) {
    specs.push({entry: selectedCallback.entry, length: selectedCallback.length}, {entry: guest.returnPc, length: 2});
    selected.push({entry: guest.returnPc, id: guest.returnId});
  }
  const descriptors = Buffer.alloc(8 * (specs.length + selected.length));
  specs.forEach((item, index) => {
    descriptors.writeUInt32LE(item.entry, index * 8);
    descriptors.writeUInt32LE(item.length, index * 8 + 4);
  });
  selected.forEach((item, index) => {
    descriptors.writeUInt32LE(item.entry, (specs.length + index) * 8);
    descriptors.writeUInt32LE(item.id, (specs.length + index) * 8 + 4);
  });
  request(descriptors);
  assert.equal(api.compile_with_gates(specs.length, selected.length), 0, call.name);
  const compiled = bytes().slice(api.module_ptr() >>> 0, (api.module_ptr() >>> 0) + api.module_len());
  const module = new WebAssembly.Module(compiled);
  const bindings = Object.fromEntries(WebAssembly.Module.imports(module)
    .filter(item => item.module === 'ring3')
    .map(item => {assert.equal(typeof api[item.name], 'function'); return [item.name, api[item.name]];}));
  writeFileSync(join(output, `compiled-${calls.length + 1}.wasm`), compiled);
  return new WebAssembly.Instance(module, {env: {memory}, ring3: bindings}).exports;
}
function run(generated, name, gate) {
  for (let iteration = 0; iteration < 8; iteration++) {
    const result = generated.run(base, base + 56, 1000, base + 96);
    const reason = get(72), detail = get(80), retired = get(76);
    runs.push({name, result, reason, detail, retired});
    if (reason === 3) continue;
    assert.equal(reason, 8, name); assert.equal(detail, gate); assert.ok(retired > 0); return;
  }
  assert.fail(`${name}: bounded guest execution`);
}
function finishCallbacks(generated, generation) {
  for (let index = 0; index < 8; index++) {
    assert.equal(api.pending_windows_callback(), 0);
    const record = take(96), provider = record.readUInt32LE(12);
    if (!provider) return;
    assert.equal(provider, 6);
    const count = record.readUInt32LE(20);
    assert.ok(count === 3 || count === 5);
    const args = Array.from({length: count}, (_, i) => record.readUInt32LE(32 + i * 4));
    assert.equal(args[0], 0x400000);
    if (count === 3) {
      assert.equal(args[2], 0x12345678);
      if (args[1] > 0xffff) {
        assert.equal(read(args[1]), 0x00590054);
        assert.equal(read(args[1] + 4), 0x00450050);
        assert.equal(read(args[1] + 8) & 0xffff, 0);
      } else assert.ok(args[1] === 6 || args[1] === 10);
    } else assert.deepEqual(args.slice(1), [10, 7, 0x409, 0x87654321]);
    assert.equal(api.begin_windows_callback(...key, generation, guest.returnPc, guest.returnId), 0);
    const token = take(64).readUInt32LE(16);
    run(generated, `resource-callback-${count}`, guest.returnId);
    assert.equal(api.finish_windows_callback(...key, generation, token), 0);
    callbacks.push({count, args});
  }
  assert.fail('bounded resource callback sequence');
}
function check(call) {
  const a = guest.addresses;
  switch (call.hook) {
    case 'payload': assert.equal(get(16), a.resourceDirectory + 0x180); break;
    case 'copied': assert.equal(read(a.copiedWord), 0x44332211); break;
    case 'same-resource': assert.equal(get(16), read(a.resource)); break;
    case 'wide-string':
      assert.equal(read(a.stringBuffer), 0x00650068);
      assert.equal(read(a.stringBuffer + 4), 0x006c);
      break;
    case 'string-pointer':
      assert.equal(read(a.stringPointer), a.resourceDirectory + 0x102);
      assert.equal(read(read(a.stringPointer)), 0x00650068);
      break;
    case 'ansi-string':
      assert.equal(read(a.stringBuffer), 0x6c6c6568);
      assert.equal(read(a.stringBuffer + 4) & 0xffff, 0x6f);
      break;
    case 'types': assert.equal(read(a.typeCallbackCount), 3); break;
    case 'languages':
      assert.equal(read(a.languageCallbackCount), 1);
      assert.equal(read(a.languageCallbackArguments + 12), 0x409);
      break;
  }
}
for (const call of guest.calls) {
  const generated = compile(call), generation = api.generation();
  run(generated, call.name, call.id);
  assert.equal(api.capture_windows_call(...key, generation), 0);
  const token = take(112).readUInt32LE(16);
  assert.equal(api.complete_windows_call(...key, generation, token), 0, call.name);
  if (call.expected !== 'exit') finishCallbacks(generated, generation);
  if (call.expected === 'pointer') assert.ok(get(16) !== 0);
  else if (call.expected === 'exit') assert.equal(get(72), 9);
  else assert.equal(get(16), call.expected, call.name);
  if (call.expected !== 'exit') check(call);
  if (call.expected !== 'exit') checkDatafile(call, {guest, read, result: get(16)});
  calls.push({name: call.name, result: get(16), generation});
  writeFileSync(join(output, 'progress.json'), JSON.stringify({calls, runs, callbacks}, null, 2));
}
assert.equal(calls.length, 51); assert.equal(callbacks.length, 4);
const receipt = {engineHash, guestHash: hash(guest.bytes), calls, runs, callbacks,
  claim: 'authored PE32/generated Wasm prove resource lookup, counted string tables and actual enumeration callbacks against retained module memory'};
writeFileSync(join(output, 'receipt.json'), JSON.stringify(receipt, null, 2));
console.log(JSON.stringify({engineHash, guestCalls: calls.length, guestRuns: runs.length, callbacks: callbacks.length}));
