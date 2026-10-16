import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {mkdirSync, readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {ARENA_SIZE} from '../../../../support/wasm/arena.mjs';
import {assertEngineExports} from '../../../../support/wasm/exports.mjs';
import {image} from '../producer/scenario.mjs';
import {configureFiles, pair} from './setup.mjs';

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
const key = [0x51525354, 0x91929394];
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

assert.equal(api.configure_cpu_clock(1000000000, 0, 0, 0), 0);
assert.equal(api.configure_synchronization(128, 256, 256, 16), 0);
assert.equal(api.configure_module_limits(8, 0x100000, 16, 65536, 0x100000), 0);
assert.equal(api.register_kernel32_module(0x70000000), 0);
const files = configureFiles({api, request, take});
const guest = image();
writeFileSync(join(output, 'files-authored.exe'), guest.bytes);
assert.equal(api.begin_module_input(guest.bytes.length, name('files-authored.exe')), 0);
for (let offset = 0; offset < guest.bytes.length; offset += 4096) {
  const chunk = guest.bytes.subarray(offset, offset + 4096);
  request(chunk);
  assert.equal(api.append_module_input(offset, chunk.length), 0);
}
assert.equal(api.commit_module_input(), 0);
assert.equal(api.start_module_process(name('files-authored.exe'), 0, 0x7e000000, 4, 0x7f000000), 0);

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
const calls = [], runs = [], waits = [], callbacks = [];

function compile(call) {
  const pc = get(48), target = gates.get(call.id);
  assert.ok(target, `${call.name} gate`);
  const needsCallbacks = ['CopyFileExA', 'CopyFileExW'].includes(call.name);
  const specs = [
    {entry: pc, length: guest.entry + call.end - pc},
    {entry: target, length: 2},
    ...(needsCallbacks ? [
      ...guest.callbacks.map(({entry, length}) => ({entry, length})),
      {entry: guest.returnPc, length: 2},
    ] : []),
  ];
  const selected = [
    {entry: target, id: call.id},
    ...(needsCallbacks ? [{entry: guest.returnPc, id: guest.returnId}] : []),
  ];
  assert.ok(specs[0].length > 0);
  const descriptors = Buffer.alloc(specs.length * 8 + selected.length * 8);
  specs.forEach((item, index) => {
    descriptors.writeUInt32LE(item.entry, index * 8);
    descriptors.writeUInt32LE(item.length, index * 8 + 4);
  });
  selected.forEach((item, index) => {
    descriptors.writeUInt32LE(item.entry, specs.length * 8 + index * 8);
    descriptors.writeUInt32LE(item.id, specs.length * 8 + index * 8 + 4);
  });
  request(descriptors);
  const compileStatus = api.compile_with_gates(specs.length, selected.length);
  if (compileStatus !== 0) {
    const arena = Buffer.from(bytes().slice(base, base + 140));
    const mainBytes = [];
    let memoryReadStatus = 0;
    for (let offset = 0; offset < specs[0].length; offset += 4) {
      memoryReadStatus = api.read32((pc + offset) >>> 0);
      if (memoryReadStatus !== 0) break;
      const word = get(120);
      for (let index = 0; index < Math.min(4, specs[0].length - offset); index++) {
        mainBytes.push((word >>> (index * 8)) & 255);
      }
    }
    const rawOffset = 0x200 + pc - guest.entry;
    const diagnostic = {
      engineHash,
      guestHash: hash(guest.bytes),
      callIndex: calls.length,
      call,
      status: compileStatus,
      pc,
      specs,
      selected,
      descriptorsHex: descriptors.toString('hex'),
      arenaHex: arena.toString('hex'),
      mainBytesHex: Buffer.from(mainBytes).toString('hex'),
      producerBytesHex: guest.bytes.subarray(rawOffset, rawOffset + specs[0].length).toString('hex'),
      memoryReadStatus,
      addresses: guest.addresses,
    };
    writeFileSync(join(output, `compile-failure-${calls.length + 1}.json`), JSON.stringify(diagnostic, null, 2));
  }
  assert.equal(compileStatus, 0, call.name);
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

function run(generated, label, expectedGate) {
  let retired = 0;
  for (let iteration = 0; iteration < 8; iteration++) {
    const result = generated.run(base, base + 56, 1000, base + 96);
    const reason = get(72), detail = get(80);
    retired += get(76);
    runs.push({label, result, reason, detail, retired: get(76)});
    if (reason === 3) continue;
    assert.equal(reason, 8, label);
    assert.equal(detail, expectedGate);
    assert.ok(retired > 0);
    return;
  }
  assert.fail(`${label}: bounded guest execution`);
}

function finishCallbacks(generated, generation) {
  for (let iteration = 0; iteration < 64; iteration++) {
    assert.equal(api.pending_windows_callback(), 0);
    const pending = take(96);
    assert.equal(pending.subarray(0, 4).toString(), 'R3WC');
    assert.equal(pending.readUInt32LE(8), 96);
    const provider = pending.readUInt32LE(12);
    if (!provider) return;
    assert.equal(provider, 5);
    assert.equal(pending.readUInt32LE(16), guest.callbacks[0].entry);
    assert.equal(pending.readUInt32LE(20), 13);
    const args = Array.from({length: 13}, (_, index) => pending.readUInt32LE(32 + index * 4));
    assert.equal(args[0], 8);
    assert.equal(args[1], 0);
    assert.equal(args[4], 8);
    assert.equal(args[5], 0);
    assert.equal(args[8], 1);
    assert.equal(args[12], 0xfeedbeef);
    assert.deepEqual(args.slice(2, 4), args.slice(6, 8));
    assert.ok([0, 1].includes(args[9]));
    assert.ok(args[10] >= 0x80000000 && args[10] !== 0xffffffff);
    assert.ok(args[11] >= 0x80000000 && args[11] !== 0xffffffff);
    assert.equal(api.begin_windows_callback(...key, generation, guest.returnPc, guest.returnId), 0);
    const frame = take(64);
    assert.equal(frame.subarray(0, 4).toString(), 'R3CB');
    const token = frame.readUInt32LE(16);
    run(generated, 'CopyProgressRoutine', guest.returnId);
    assert.equal(read(guest.addresses.copyProgressCount), callbacks.length + 1);
    assert.deepEqual(
      Array.from({length: 13}, (_, index) => read(guest.addresses.copyProgressArguments + index * 4)),
      args,
      'actual guest callback receives all 13 stack words',
    );
    assert.equal(get(16), 0, 'guest returns PROGRESS_CONTINUE');
    assert.equal(api.finish_windows_callback(...key, generation, token), 0);
    callbacks.push({provider, continuation: pending.readBigUInt64LE(24).toString(), arguments: args});
  }
  assert.fail('bounded copy progress callbacks');
}

for (const call of guest.calls) {
  if (call.expected === 'exit') {
    assert.equal(api.progress_file_io(...pair(files.epoch + 1n)), 40);
  }
  const generated = compile(call), generation = api.generation();
  run(generated, call.name, call.id);
  assert.equal(api.capture_windows_call(...key, generation), 0);
  const token = take(112).readUInt32LE(16);
  assert.equal(api.complete_windows_call(...key, generation, token), 0, call.name);
  if (call.expected !== 'exit') finishCallbacks(generated, generation);
  for (let retry = 0; retry < 64; retry++) {
    if (call.expected === 'exit' && get(72) === 9) break;
    assert.equal(api.pending_file_call(), 0);
    const pending = take(64);
    if (pending.readBigUInt64LE(16) === 0n) break;
    assert.equal(Number(pending.readBigUInt64LE(24)), token);
    assert.equal(api.complete_call(...key, generation, token, 1), 12, 'generic completion cannot bypass file provider');
    if (!files.supply()) {
      assert.equal(api.progress_file_io(...pair(files.epoch)), 0);
      const progress = take(64);
      assert.ok(
        progress.readBigUInt64LE(24) !== 0n || progress.readBigUInt64LE(48) !== 0n,
        'accepted I/O advances or asks for bytes',
      );
    }
    assert.equal(api.resume_file_call(...key, token), 0, call.name);
    waits.push({name: call.name, token, retry});
    assert.ok(retry < 63, 'bounded host progress');
  }
  const result = get(16);
  if (call.expected === 'handle') {
    assert.ok(result !== 0 && result !== 0xffffffff, call.name);
  } else if (call.expected === 'pointer') {
    assert.ok(result >= 0x10000000 && result < 0x70000000, call.name);
  } else if (call.expected === 'exit') {
    assert.equal(get(72), 9);
  } else {
    assert.equal(result, call.expected, call.name);
  }
  if (call.hook === 'asset-read') {
    assert.equal(read(guest.addresses.buffer), 0x04030201);
    assert.equal(read(guest.addresses.count), 8);
  }
  if (call.hook === 'large-size') {
    assert.equal(read(guest.addresses.largeHigh), 1, '4 GiB asset metadata retains high DWORD');
  }
  if (call.hook === 'large-read') {
    assert.equal(read(guest.addresses.buffer), 0x03020100);
    assert.equal(read(guest.addresses.count), 4);
  }
  if (call.hook === 'copy-progress') {
    assert.equal(callbacks.length, 2);
    assert.deepEqual(callbacks.map(item => item.arguments[9]), [1, 0]);
    assert.deepEqual(callbacks.map(item => item.arguments[2]), [0, 8]);
  }
  if (call.hook === 'mapped-coherence') {
    assert.equal(read(guest.addresses.readWord), 0x11223344);
    assert.equal(read(guest.addresses.copyWord), 0x11223344);
    assert.equal(read(read(guest.addresses.alias)), 0x11223344);
    assert.equal(read(read(guest.addresses.copy)), 0x55667788);
  }
  if (call.hook === 'save-read') {
    assert.equal(read(guest.addresses.buffer), 0x12345678);
    assert.equal(read(guest.addresses.buffer + 4), 0x90abcdef);
    assert.equal(read(guest.addresses.count), 8);
  }
  if (call.hook === 'anonymous-read' || call.hook === 'null-eof') {
    assert.equal(read(guest.addresses.endpointBuffer), 0x64636261);
    assert.equal(read(guest.addresses.endpointBuffer + 4) & 255, 0x65);
    assert.equal(read(guest.addresses.count), call.hook === 'null-eof' ? 0 : 5);
  }
  if (call.hook === 'null-write') assert.equal(read(guest.addresses.count), 8);
  if (call.hook === 'pipe-partial-read') {
    assert.equal(read(guest.addresses.endpointBuffer) & 0xffff, 0x6261);
    assert.equal(read(guest.addresses.count), 2);
  }
  if (call.hook === 'pipe-peek' || call.hook === 'pipe-remaining-read') {
    assert.equal(read(guest.addresses.endpointBuffer) & 0xffffff, 0x656463);
    assert.equal(read(guest.addresses.count), 3);
    if (call.hook === 'pipe-peek') {
      assert.equal(read(guest.addresses.packetCount), 3);
      assert.equal(read(guest.addresses.packetKey), 0);
    }
  }
  if (['pipe-connect-packet', 'pipe-partial-packet', 'pipe-remaining-packet', 'pipe-broken-packet'].includes(call.hook)) {
    const counts = {'pipe-connect-packet': 0, 'pipe-partial-packet': 2, 'pipe-remaining-packet': 3, 'pipe-broken-packet': 0};
    assert.equal(read(guest.addresses.packetCount), counts[call.hook]);
    assert.equal(read(guest.addresses.packetKey), 0x53594e43);
    assert.equal(read(guest.addresses.packetOverlapped), guest.addresses.pipeOverlap);
  }
  if (call.hook === 'posted-completion') {
    assert.equal(read(guest.addresses.packetCount), 19);
    assert.equal(read(guest.addresses.packetKey), 0xabc);
    assert.equal(read(guest.addresses.packetOverlapped), 0x7777);
  }
  calls.push({name: call.name, id: call.id, result, generation});
  writeFileSync(join(output, 'progress.json'), JSON.stringify({calls, runs, waits, callbacks}, null, 2));
}

assert.equal(calls.length, 96);
const overlays = [];
for (let index = 0; index < 32; index++) {
  assert.equal(api.file_overlay_entry(...pair(files.epoch), index), 0);
  const header = take(96);
  if (header.readBigUInt64LE(16) === 0n) break;
  const units = header.readUInt32LE(56);
  const entry = take(96 + units * 2);
  const path = entry.subarray(96).toString('utf16le');
  const file = entry.readBigUInt64LE(32);
  const revision = entry.readBigUInt64LE(24);
  const size = entry.readBigUInt64LE(40);
  const kind = entry.readUInt32LE(52);
  overlays.push({path, file: String(file), revision: String(revision), size: String(size), kind});
  if (kind === 1 && ['c:\\saves\\renamed.dat', 'c:\\saves\\copied.dat'].includes(path.toLowerCase())) {
    assert.equal(api.read_file_overlay(...pair(files.epoch), ...pair(file), ...pair(revision), 0, 0, 8), 0);
    const data = take(40);
    assert.equal(data.subarray(0, 4).toString(), 'R3FD');
    assert.equal(data.readUInt32LE(16), 8);
    assert.equal(data.readUInt32LE(32), 0x12345678);
    assert.equal(data.readUInt32LE(36), 0x90abcdef);
  }
  if (kind === 1 && path.toLowerCase() === 'c:\\assets\\data.bin') {
    assert.equal(api.read_file_overlay(...pair(files.epoch), ...pair(file), ...pair(revision), 0, 0, 8), 0);
    const data = take(40);
    assert.equal(data.subarray(0, 4).toString(), 'R3FD');
    assert.equal(data.readUInt32LE(16), 8);
    assert.equal(data.readUInt32LE(32), 0x44332211, 'exit flushes dirty guest mapping');
    assert.equal(data.readUInt32LE(36), 0x08070605);
  }
}
assert.ok(!overlays.some(item => /(?:nul|conout\$|authored-ring3)/i.test(item.path)), 'devices and pipes never become overlay files');
assert.ok(overlays.some(item => item.path.toLowerCase() === 'c:\\saves\\renamed.dat'), 'save exported with rename');
assert.ok(overlays.some(item => item.path.toLowerCase() === 'c:\\saves\\copied.dat' && item.size === '8'), 'callback copy exported as real destination');
assert.ok(overlays.some(item => item.path.toLowerCase() === 'c:\\assets\\data.bin' && item.kind === 1), 'exit retains flushed asset export');
assert.ok(overlays.some(item => item.path.toLowerCase() === 'c:\\saves\\scratch.tmp' && item.kind === 3), 'exit deletes open delete-on-close scratch');
assert.ok(!overlays.some(item => item.path.toLowerCase() === 'c:\\saves\\scratch.tmp' && item.kind === 1), 'scratch has no live file export');
assert.equal(api.progress_file_io(...pair(files.epoch + 1n)), 21);
const receipt = {
  engineHash,
  guestHash: hash(guest.bytes),
  calls,
  runs,
  waits,
  callbacks,
  overlays,
  claim: 'authored PE32/generated Wasm validate engine asset IO including 4 GiB metadata, coherent mapping/COW, save export, overlapped cancellation and real guest copy progress callbacks and exit-owned file/mapping finalization, anonymous/named pipe aliases and partial-message EOF, IOCP packets, NUL discard and headless console refusal; browser persistence is outside this fixture',
};
writeFileSync(join(output, 'receipt.json'), JSON.stringify(receipt, null, 2));
console.log(JSON.stringify({
  engineHash,
  guestCalls: calls.length,
  guestRuns: runs.length,
  guestCallbacks: callbacks.length,
  hostWaits: waits.length,
  overlayEntries: overlays.length,
}));
