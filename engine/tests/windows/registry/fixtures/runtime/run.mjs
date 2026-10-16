import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {mkdirSync, readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {ARENA_SIZE} from '../../../../support/wasm/arena.mjs';
import {assertEngineExports} from '../../../../support/wasm/exports.mjs';
import {image} from '../producer/scenario.mjs';
import {configureRegistry} from './setup.mjs';

const [enginePath, output, root] = process.argv.slice(2);
mkdirSync(output, {recursive: true});
const hash = value => createHash('sha256').update(value).digest('hex');
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
const key = [0x55565758, 0x95969798];
assert.equal(api.open(1024, ...key), 0);
const base = api.arena_ptr() >>> 0;
assert.equal(api.arena_len(), ARENA_SIZE);
const bytes = () => new Uint8Array(memory.buffer);
const get = offset => new DataView(memory.buffer).getUint32(base + offset, true);
const take = length => Buffer.from(bytes().slice(base + 140, base + 140 + length));
const request = value => {
  assert.ok(value.length <= 4096);
  bytes().set(value, base + 140);
};
const read = address => {
  assert.equal(api.read32(address >>> 0), 0);
  assert.equal(get(116), 0);
  return get(120);
};
const name = value => {
  const data = Buffer.from(value);
  request(data);
  return data.length;
};
assert.equal(api.configure_cpu_clock(1_000_000_000, 0, 0, 0), 0);
assert.equal(api.configure_synchronization(32, 64, 64, 16), 0);
assert.equal(api.configure_module_limits(8, 0x100000, 16, 65536, 0x100000), 0);
assert.equal(api.register_kernel32_module(0x70000000), 0);
assert.equal(api.register_advapi32_module(0x71000000), 0);
const registry = configureRegistry({api, request, take});
const guest = image();
writeFileSync(join(output, 'registry-authored.exe'), guest.bytes);
assert.equal(guest.calls.length, 44);
assert.equal(api.begin_module_input(guest.bytes.length, name('registry-authored.exe')), 0);
for (let offset = 0; offset < guest.bytes.length; offset += 4096) {
  const chunk = guest.bytes.subarray(offset, offset + 4096);
  request(chunk);
  assert.equal(api.append_module_input(offset, chunk.length), 0);
}
assert.equal(api.commit_module_input(), 0);
assert.equal(api.start_module_process(name('registry-authored.exe'), 0, 0x7e000000, 4, 0x7f000000), 0);
const rows = [];
for (let start = 0, total = 1; start < total;) {
  assert.equal(api.module_gates_page(start, 128), 0);
  const page = take(4096);
  assert.equal(page.subarray(0, 4).toString(), 'R3GP');
  const count = page.readUInt32LE(24);
  total = page.readUInt32LE(16);
  assert.ok(count > 0);
  for (let index = 0; index < count; index++) {
    rows.push([page.readUInt32LE(36 + index * 8), page.readUInt32LE(32 + index * 8)]);
  }
  start += count;
}
const gates = new Map(rows);
assert.equal(gates.size, rows.length);
assert.equal(new Set(rows.map(([, pc]) => pc)).size, rows.length);
const calls = [], runs = [], flushes = [];

function compile(call) {
  const pc = get(48), target = gates.get(call.id);
  assert.ok(target, `${call.name} gate`);
  const specs = [{entry: pc, length: guest.entry + call.end - pc}, {entry: target, length: 2}];
  assert.ok(specs[0].length > 0);
  const descriptors = Buffer.alloc(24);
  specs.forEach((item, index) => {
    descriptors.writeUInt32LE(item.entry, index * 8);
    descriptors.writeUInt32LE(item.length, index * 8 + 4);
  });
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
    case 'seeded':
      assert.equal(read(a.type), 4);
      assert.equal(read(a.size), 4);
      assert.equal(read(a.buffer), 0x12345678);
      break;
    case 'created':
      assert.equal(read(a.disposition), 1);
      assert.ok(read(a.key) >= 0x53000000);
      break;
    case 'unicode':
      assert.equal(read(a.type), 1);
      assert.equal(read(a.size), 6);
      assert.equal(read(a.buffer), 0x006b006f);
      assert.equal(read(a.buffer + 4) & 0xffff, 0);
      break;
    case 'more-data':
      assert.equal(read(a.type), 1);
      assert.equal(read(a.size), 3);
      assert.equal(read(a.buffer), 0xcccccccc);
      break;
    case 'terminated':
      assert.equal(read(a.size), 4);
      assert.equal(read(a.buffer), 65);
      break;
    case 'enumerated-value':
      assert.equal(read(a.nameSize), 4);
      assert.equal(read(a.enumName), 0x0061004e);
      assert.equal(read(a.type), 1);
      assert.equal(read(a.size), 6);
      break;
    case 'info':
      assert.equal(read(a.values), 2);
      break;
    case 'overridden':
    case 'copied':
      assert.equal(read(a.size), 3);
      assert.equal(read(a.buffer) & 0xffffff, 0x006b6f);
      break;
    case 'child-value':
      assert.equal(read(a.type), 4);
      assert.equal(read(a.size), 4);
      assert.equal(read(a.buffer), 0x78563412);
      break;
    case 'enumerated-key':
      assert.equal(read(a.nameSize), 5);
      assert.equal(read(a.enumName), 0x6c696843);
      break;
    case 'recreated':
      assert.equal(read(a.disposition), 1);
      assert.notEqual(read(a.replacement), read(a.key));
      break;
  }
}
for (const call of guest.calls) {
  const generated = compile(call), generation = api.generation();
  run(generated, call);
  assert.equal(api.capture_windows_call(...key, generation), 0);
  const token = take(112).readUInt32LE(16);
  assert.equal(api.complete_windows_call(...key, generation, token), 0, call.name);
  if (call.hook === 'flush') {
    assert.equal(api.pending_registry_call(), 0);
    let pending = take(40);
    assert.equal(pending.subarray(0, 4).toString(), 'R3RW');
    assert.equal(pending.readUInt32LE(12), 1);
    assert.equal(pending.readUInt32LE(24), token);
    assert.equal(pending.readUInt32LE(28), 0);
    assert.equal(api.resume_registry_call(...key, token), 12, 'dirty FlushKey remains parked');
    assert.equal(api.complete_call(...key, generation, token, 0), 12, 'generic completion cannot bypass registry ownership');
    const changes = registry.drain('flush');
    assert.ok(changes.length > 0, 'host receives actual dirty registry entries');
    assert.equal(api.pending_registry_call(), 0);
    pending = take(40);
    assert.equal(pending.readUInt32LE(28), 1, 'only exact host ACK makes FlushKey ready');
    assert.equal(api.resume_registry_call(...key, token), 0);
    assert.equal(api.pending_registry_call(), 0);
    assert.equal(take(40).readUInt32LE(12), 0);
    flushes.push({token, changes});
  }
  const result = get(16);
  if (call.expected === 'handle') assert.ok(result !== 0 && result !== 0xffffffff, call.name);
  else if (call.expected === 'exit') assert.equal(get(72), 9);
  else assert.equal(result, call.expected, call.name);
  if (call.expected !== 'exit') check(call);
  calls.push({name: call.name, id: call.id, result, generation});
  writeFileSync(join(output, 'progress.json'), JSON.stringify({calls, runs, flushes}, null, 2));
}
assert.equal(calls.length, 44);
assert.equal(flushes.length, 1);
const exported = registry.drain('exit');
assert.ok(exported.some(change => change.kind === 2 && change.name === 'Name' && change.path.toLowerCase().endsWith('\\software\\ring3\\game') && change.bytes === '09000000'), 'postexit persistence reads the replacement key value');
const receipt = {
  engineHash, guestHash: hash(guest.bytes), calls, runs, flushes, exported,
  claim: 'authored PE32 and generated Wasm prove virtual registry values, permissions, identity, one-shot event notifications and exact host persistence ownership; no host OS registry access is used',
};
writeFileSync(join(output, 'receipt.json'), JSON.stringify(receipt, null, 2));
console.log(JSON.stringify({engineHash, guestCalls: calls.length, guestRuns: runs.length, hostFlushes: flushes.length, exportedChanges: exported.length}));
