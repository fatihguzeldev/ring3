import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {execFileSync} from 'node:child_process';
import {mkdirSync, readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {ARENA_SIZE} from '../../../../../support/wasm/arena.mjs';
import {assertEngineExports} from '../../../../../support/wasm/exports.mjs';
import {image} from '../producer/image.mjs';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root); mkdirSync(output, {recursive: true});
const hash = value => createHash('sha256').update(value).digest('hex');
const engineBytes = readFileSync(enginePath), engineHash = hash(engineBytes);
assert.equal(engineHash, process.env.RING3_ENGINE_SHA256, 'exact selected engine artifact');
const engineModule = new WebAssembly.Module(engineBytes);
assert.deepEqual(WebAssembly.Module.imports(engineModule), []); assertEngineExports(engineModule, root);
const instance = new WebAssembly.Instance(engineModule, {}), memory = instance.exports.memory;
const api = Object.fromEntries(Object.entries(instance.exports).filter(([name]) => name.startsWith('ring3_abi_v1_')).map(([name, fn]) => [name.slice(13), fn]));
const key = [0x51525354, 0x91929394], transfer = 140;
assert.equal(api.open(1024, ...key), 0);
const base = api.arena_ptr() >>> 0;
assert.equal(api.arena_len(), ARENA_SIZE);
const bytes = () => new Uint8Array(memory.buffer);
const get = offset => new DataView(memory.buffer).getUint32(base + offset, true);
const take = length => Buffer.from(bytes().slice(base + transfer, base + transfer + length));
const request = value => {assert.ok(value.length <= 4096); bytes().set(value, base + transfer);};
const name = value => {const data = Buffer.from(value); request(data); return data.length;};
const pair = value => [Number(value & 0xffffffffn), Number(value >> 32n)];
const read = address => {assert.equal(api.read32(address >>> 0), 0); assert.equal(get(116), 0, `mapped read ${address.toString(16)}`); return get(120);};
const sourcePaths = execFileSync('rg', ['--files', 'engine/src'], {cwd: root, encoding: 'utf8'}).trim().split('\n').concat([
  'engine/Cargo.toml', 'engine/tests/windows/graphics/d3d8/wasm.rs',
  'engine/tests/windows/graphics/d3d8/fixtures/runtime/run.mjs', 'engine/tests/windows/graphics/d3d8/fixtures/producer/image.mjs',
  'engine/tests/support/wasm.rs', 'engine/tests/support/wasm/arena.mjs', 'engine/tests/support/wasm/exports.mjs',
]).sort();
writeFileSync(join(output, 'source-sha256.json'), JSON.stringify(Object.fromEntries(sourcePaths.map(path => [path, hash(readFileSync(join(root, path)))])), null, 2));

assert.equal(api.configure_module_limits(16, 0x100000, 16, 65536, 0x100000), 0);
assert.equal(api.register_kernel32_module(0x70000000), 0);
assert.equal(api.configure_user32(16, 16, 64, 16, 0x30000000, 16), 0);
assert.equal(api.register_user32_module(0x72000000), 0);
const config = Buffer.alloc(144); config.write('R3C8'); config.writeUInt16LE(1, 4); config.writeUInt16LE(8, 6);
const settings = new Map([[2, 144], [4, 1], [5, 0x10000000], [6, 256], [7, 0x20000000], [8, 128],
  [9, 128], [10, 4], [11, 16], [12, 1024], [13, 256 * 1024 * 1024], [14, 256],
  [15, 64], [16, 0x100000], [17, 32], [18, 16], [19, 0x100000],
  [20, 64], [21, 64], [22, 22], [23, 60], [24, 4096], [25, 256], [26, 16], [27, 1], [28, 0x8000000f]]);
for (const [index, value] of settings) config.writeUInt32LE(value >>> 0, index * 4);
[64, 64, 60, 22].forEach((value, index) => config.writeUInt32LE(value, 128 + index * 4));
const invalid = Buffer.from(config); invalid.writeUInt32LE(1, 12); request(invalid);
assert.equal(api.configure_d3d8(invalid.length), 7, 'reserved provisioning word rejected');
request(config); assert.equal(api.configure_d3d8(config.length), 0);
assert.equal(api.register_d3d8_module(0x71000000), 0);
const guest = image(); writeFileSync(join(output, 'd3d8-authored.exe'), guest.bytes);
assert.equal(api.begin_module_input(guest.bytes.length, name('d3d8-authored.exe')), 0);
for (let offset = 0; offset < guest.bytes.length; offset += 4096) {
  const chunk = guest.bytes.subarray(offset, offset + 4096); request(chunk);
  assert.equal(api.append_module_input(offset, chunk.length), 0);
}
assert.equal(api.commit_module_input(), 0);
assert.equal(api.start_module_process(name('d3d8-authored.exe'), 0, 0x7e000000, 4, 0x7f000000), 0);
assert.equal(take(72).subarray(0, 4).toString(), 'R3MO'); assert.equal(get(48), guest.entry);
const gateRows = [];
let gateCount;
for (let start = 0; gateCount === undefined || start < gateCount;) {
  assert.equal(api.module_gates_page(start, 128), 0);
  const page = take(4096), total = page.readUInt32LE(16), count = page.readUInt32LE(24);
  assert.deepEqual(page.subarray(0, 8), Buffer.from([0x52, 0x33, 0x47, 0x50, 1, 0, 1, 0]));
  assert.equal(page.readUInt32LE(8), 32 + count * 8);
  assert.equal(page.readUInt32LE(12), 0);
  assert.equal(page.readUInt32LE(20), start);
  assert.equal(page.readUInt32LE(28), 0);
  if (gateCount === undefined) gateCount = total;
  assert.equal(total, gateCount, 'gate inventory remains stable across pages');
  assert.ok(count > 0 && count <= 128 && start + count <= total);
  for (let i = 0; i < count; i++) gateRows.push([page.readUInt32LE(36 + i * 8), page.readUInt32LE(32 + i * 8)]);
  start += count;
}
assert.equal(gateRows.length, gateCount, 'every advertised gate is retained');
const gates = new Map(gateRows);
assert.equal(gates.size, gateCount, 'all public, private COM and initializer gate IDs are distinct');
assert.equal(new Set(gateRows.map(([, entry]) => entry)).size, gateCount, 'each gate owns one executable entry');
writeFileSync(join(output, 'gates.json'), JSON.stringify(gateRows));
const runs = [], calls = [], callbacks = [], packets = [], commands = [], receipts = [], fragments = new Map();
let staleBatchChecked = false, fragmentedCommands = 0, previousCommand = 0n;
function metadata(kind) {
  const data = take(64); assert.equal(data.subarray(0, 4).toString(), 'R3MR');
  assert.equal(data.readUInt16LE(6), kind); assert.equal(data.readUInt32LE(8), 64); assert.equal(data.readUInt32LE(12), 0);
  return {epoch: data.readBigUInt64LE(16), values: Array.from({length: 5}, (_, i) => data.readBigUInt64LE(24 + i * 8))};
}
function devices() {
  const result = [];
  for (let i = 0; i < 16; i++) {
    assert.equal(api.d3d8_device(i), 0); const item = metadata(12);
    if (!item.epoch) return result;
    result.push({epoch: item.epoch, generation: item.values[1], pointer: Number(item.values[0]), retired: item.values[2] !== 0n});
  }
  assert.fail('device inventory bounded');
}
function parsePacket(packet) {
  assert.equal(packet.subarray(0, 4).toString(), 'R3D8'); assert.equal(packet.readUInt16LE(4), 2); assert.equal(packet.readUInt16LE(6), 8);
  assert.equal(packet.readUInt32LE(8), packet.length); assert.deepEqual([...packet.subarray(48, 64)], Array(16).fill(0));
  const epoch = packet.readBigUInt64LE(16), sequence = packet.readBigUInt64LE(24), count = packet.readUInt32LE(12);
  let at = 64;
  for (let i = 0; i < count; i++) {
    const opcode = packet.readUInt16LE(at), flags = packet.readUInt16LE(at + 2), length = packet.readUInt32LE(at + 4);
    const id = packet.readBigUInt64LE(at + 8), offset = packet.readUInt32LE(at + 16), total = packet.readUInt32LE(at + 20);
    assert.deepEqual([...packet.subarray(at + 24, at + 32)], Array(8).fill(0));
    const key = `${epoch}:${id}`, chunk = packet.subarray(at + 32, at + length);
    if (flags & 1) {assert.ok(!fragments.has(key)); assert.equal(offset, 0); fragments.set(key, {opcode, total, bytes: [], length: 0, pieces: 0});}
    const item = fragments.get(key); assert.ok(item); assert.equal(item.opcode, opcode); assert.equal(item.total, total); assert.equal(item.length, offset);
    item.bytes.push(chunk); item.length += chunk.length; item.pieces++;
    if (flags & 2) {
      assert.equal(item.length, total); const payload = Buffer.concat(item.bytes);
      assert.equal(id, previousCommand + 1n, 'logical GPU jobs appear once in exact order'); previousCommand = id;
      commands.push({epoch: epoch.toString(), id: id.toString(), opcode, payload: payload.toString('hex')});
      if (item.pieces > 1) fragmentedCommands++;
      if (opcode === 14) receipts.push({epoch, payload});
      fragments.delete(key);
    }
    at += length;
  }
  assert.equal(at, packet.length); packets.push({epoch: epoch.toString(), sequence: sequence.toString(), bytes: packet.length, fragments: count});
}
function completeReadback({epoch, payload}) {
  assert.equal(payload.length, 84); assert.equal(payload.readUInt32LE(52), 1);
  const length = payload.readUInt32LE(80), header = Buffer.alloc(64);
  header.write('R3R8'); header.writeUInt16LE(2, 4); header.writeUInt16LE(8, 6); header.writeUInt32LE(64 + length, 8); header.writeUInt32LE(length, 12);
  payload.copy(header, 16, 0, 16); payload.copy(header, 32, 16, 40); header.writeUInt32LE(payload.readUInt32LE(40), 56);
  const malformed = Buffer.from(header); malformed[0] = 0; request(malformed); assert.equal(api.begin_d3d8_readback_input(), 7);
  const stale = Buffer.from(header); stale.writeUInt32LE(stale.readUInt32LE(44) + 1, 44); request(stale); assert.equal(api.begin_d3d8_readback_input(), 40, 'source generation is part of the receipt identity');
  request(header); assert.equal(api.begin_d3d8_readback_input(), 0); assert.equal(api.begin_d3d8_readback_input(), 12, 'one staged receipt owns the pending CPU call');
  assert.equal(api.finish_d3d8_readback_input(), 7, 'incomplete receipt is neutral');
  const pixels = Buffer.alloc(length, 0x5a);
  for (let offset = 0; offset < length; offset += 4096) {
    const chunk = pixels.subarray(offset, offset + 4096); request(chunk);
    if (!offset) assert.equal(api.append_d3d8_readback_input(1, chunk.length), 7, 'sequential chunks only');
    assert.equal(api.append_d3d8_readback_input(offset, chunk.length), 0);
  }
  assert.equal(api.finish_d3d8_readback_input(), 0); assert.equal(api.finish_d3d8_readback_input(), 40, 'consumed receipt cannot be replayed');
  writeFileSync(join(output, `readback-${epoch}-${payload.readBigUInt64LE(8)}.bin`), Buffer.concat([header, pixels]));
}
function drain() {
  for (const device of devices()) {
    for (let iteration = 0; iteration < 2048; iteration++) {
      const status = api.d3d8_seal_batch(...pair(device.epoch));
      if (status === 40) break;
      assert.equal(status, 0); const batch = metadata(13), [sequence, length] = batch.values;
      if (!sequence) break;
      const packet = Buffer.alloc(Number(length));
      for (let offset = 0; offset < packet.length; offset += 4096) {
        const size = Math.min(4096, packet.length - offset);
        if (!staleBatchChecked) {assert.equal(api.d3d8_read_batch(...pair(device.epoch), ...pair(sequence + 1n), offset, size), 40); staleBatchChecked = true;}
        assert.equal(api.d3d8_read_batch(...pair(device.epoch), ...pair(sequence), offset, size), 0); take(size).copy(packet, offset);
      }
      parsePacket(packet);
      assert.equal(api.d3d8_acknowledge_batch(...pair(device.epoch), ...pair(sequence + 1n)), 40, 'wrong sequence cannot consume a packet');
      assert.equal(api.d3d8_acknowledge_batch(...pair(device.epoch), ...pair(sequence)), 0);
      assert.equal(api.d3d8_read_batch(...pair(device.epoch), ...pair(sequence), 0, 1), 40, 'consumed batch cannot be read again');
      if (iteration === 2047) assert.fail('queue must drain within bounded packets');
    }
  }
  while (receipts.length) completeReadback(receipts.shift());
}
function compile(call) {
  const pc = get(48), target = gates.get(call.id); assert.ok(target, `gate ${call.id.toString(16)} exists`);
  const specs = [{entry: pc, length: guest.entry + call.end - pc}, {entry: target, length: 2}, {entry: guest.callback, length: 8}, {entry: guest.returnPc, length: 2}];
  assert.ok(specs[0].length > 0);
  const selected = [{entry: target, id: call.id}, {entry: guest.returnPc, id: guest.returnId}];
  const descriptors = Buffer.alloc(specs.length * 8 + selected.length * 8);
  specs.forEach((item, i) => {descriptors.writeUInt32LE(item.entry, i * 8); descriptors.writeUInt32LE(item.length, i * 8 + 4);});
  selected.forEach((item, i) => {descriptors.writeUInt32LE(item.entry, specs.length * 8 + i * 8); descriptors.writeUInt32LE(item.id, specs.length * 8 + i * 8 + 4);});
  request(descriptors); assert.equal(api.compile_with_gates(specs.length, selected.length), 0, `compile ${call.name}`);
  writeFileSync(join(output, `descriptors-${calls.length + 1}.json`), JSON.stringify({specs, selected}));
  const compiled = bytes().slice(api.module_ptr() >>> 0, (api.module_ptr() >>> 0) + api.module_len()), module = new WebAssembly.Module(compiled);
  const bindings = Object.fromEntries(WebAssembly.Module.imports(module).filter(item => item.module === 'ring3').map(item => {assert.equal(typeof api[item.name], 'function'); return [item.name, api[item.name]];}));
  writeFileSync(join(output, `compiled-${calls.length + 1}.wasm`), compiled);
  return new WebAssembly.Instance(module, {env: {memory}, ring3: bindings}).exports;
}
function run(generated, label, expectedGate) {
  let totalRetired = 0;
  for (let iteration = 0; iteration < 8; iteration++) {
    const pc = get(48), result = generated.run(base, base + 56, 1000, base + 96), reason = get(72), detail = get(80), retired = get(76);
    runs.push({label, pc, result, reason, detail, retired}); totalRetired += retired;
    writeFileSync(join(output, 'runs.json'), JSON.stringify(runs, null, 2));
    if (reason === 3) continue;
    assert.equal(reason, 8, `${label}: gate exit`); assert.equal(detail, expectedGate);
    assert.ok(totalRetired > 0, `${label}: guest instructions retire before gate`); return;
  }
  assert.fail(`${label}: bounded generated execution`);
}
function finishCallbacks(generated, generation) {
  for (let iteration = 0; iteration < 64; iteration++) {
    assert.equal(api.pending_windows_callback(), 0); const request = take(96);
    assert.equal(request.subarray(0, 4).toString(), 'R3WC'); assert.equal(request.readUInt32LE(8), 96);
    const provider = request.readUInt32LE(12); if (!provider) return;
    assert.ok([1, 2].includes(provider)); assert.equal(request.readUInt32LE(16), guest.callback); assert.ok(request.readUInt32LE(20) <= 16);
    assert.equal(api.begin_windows_callback(...key, generation, guest.returnPc, guest.returnId), 0);
    const frame = take(64); assert.equal(frame.subarray(0, 4).toString(), 'R3CB'); const token = frame.readUInt32LE(16);
    run(generated, 'WndProc', guest.returnId);
    assert.equal(api.finish_windows_callback(...key, generation, token), 0);
    callbacks.push({provider, message: request.readUInt32LE(36), continuation: request.readBigUInt64LE(24).toString()});
  }
  assert.fail('bounded WndProc continuations');
}
for (const call of guest.calls) {
  drain();
  if (call.hook) {
    const device = devices().find(item => !item.retired); assert.ok(device);
    assert.equal(api.d3d8_observe_device(...pair(device.epoch), ...pair(device.generation), call.hook === 'lost' ? 1 : 2), 0); drain();
  }
  const generated = compile(call), generation = api.generation(); run(generated, call.name, call.id);
  assert.equal(api.windows_gate_info(call.id), 0); assert.equal(metadata(15).values[0], BigInt(call.id));
  assert.equal(api.capture_windows_call(...key, generation), 0); const token = take(112).readUInt32LE(16);
  let complete;
  for (let retry = 0; retry < 64; retry++) {
    complete = api.complete_windows_call(...key, generation, token);
    assert.ok([0, 12].includes(complete), `${call.name}: completion transport ${complete}`);
    if (call.expected === 'exit' && complete === 0) {
      assert.equal(api.pending_windows_callback(), 21, 'exited process cannot request callbacks');
      assert.equal(api.d3d8_device(0), 0, 'final host output inventory remains available');
      assert.equal(metadata(12).epoch, 0n, 'released and acknowledged device is reclaimed');
      assert.equal(api.d3d8_observe_device(0, 0, 0, 0, 1), 21, 'exited process rejects new device input');
      break;
    }
    finishCallbacks(generated, generation); drain();
    if (complete === 0) break;
    if (retry === 63) assert.fail(`${call.name}: must complete after host replies`);
  }
  const result = get(16);
  if (call.expected === 'pointer') assert.ok(result !== 0, `${call.name}: valid guest identity`);
  else if (call.expected === 'atom') assert.ok(result >= 0xc000, 'registered class atom');
  else if (call.expected === 'exit') {assert.equal(get(72), 9); assert.equal(get(80), 0);}
  else assert.equal(result, call.expected, `${call.name}: HRESULT/refcount`);
  calls.push({name: call.name, id: call.id, result, generation});
  writeFileSync(join(output, 'progress.json'), JSON.stringify({calls, runs, callbacks, packets, commands}, null, 2));
}
assert.equal(read(guest.addresses.observed), 0x5a5a5a5a, 'host receipt bytes reach the guest through the real lock mapping');
assert.equal(read(guest.addresses.observedBack), 0x5a5a5a5a, 'a GPU-dirty lock resumes through its own staged readback receipt');
assert.equal(read(guest.addresses.size), 1, 'too-small shader output preserves caller size');
assert.equal(read(guest.addresses.small), 0xdddddddd, 'too-small shader output preserves destination');
assert.ok(callbacks.some(item => item.message === 0x81), 'WM_NCCREATE executes authored WndProc');
assert.ok(callbacks.some(item => item.message === 1), 'WM_CREATE executes authored WndProc');
assert.ok(fragmentedCommands > 0, 'bounded packets fragment logical state snapshots'); assert.equal(fragments.size, 0);
for (const opcode of [1, 2, 3, 4, 5, 6, 8, 9, 10, 11, 12, 13, 14]) assert.ok(commands.some(item => item.opcode === opcode), `semantic command ${opcode} emitted`);
assert.equal(commands.filter(item => item.opcode === 8).length, 2, 'fixed-function and programmable draws each submit exactly once');
const logical = opcode => commands.filter(item => item.opcode === opcode).map(item => Buffer.from(item.payload, 'hex'));
assert.equal(logical(13).filter(payload => payload.readUInt32LE(8) === 3).length, 1, 'device removal is submitted exactly once');
assert.equal(commands.at(-1).opcode, 13, 'final removal follows all queued graphics work');
const draws = logical(8);
assert.deepEqual(draws.map(draw => draw.readBigUInt64LE(32)), [3n, 4n], 'clear and each accepted draw advance the target exactly once across packet retries');
assert.equal(logical(9)[0].readBigUInt64LE(16), 2n, 'clear pins its output content version');
assert.equal(logical(11)[0].readBigUInt64LE(40), 4n, 'Present consumes the exact completed draw version');
function pipelineState(payload) {
  let at = 8;
  const word = () => {const result = payload.readUInt32LE(at); at += 4; return result;};
  const words = count => Array.from({length: count}, word);
  assert.equal(word(), 2); const render = words(word()), stages = [];
  for (let count = word(); count; count--) stages.push(words(word()));
  assert.equal(word(), 266); const masks = words(word());
  for (const mask of masks) {let value = mask; while (value) {at += 64; value &= value - 1;}}
  const material = words(17), lights = [];
  for (let count = word(); count; count--) lights.push(words(28));
  const clipPlanes = word(); at += clipPlanes * 16 + 8;
  const viewport = words(6), vertex = word(), pixel = word(), palette = word();
  const vertexConstants = words(word() * 4), pixelConstants = words(word() * 4);
  assert.equal(at, payload.length);
  return {render, stages, material, lights, viewport, vertex, pixel, palette, vertexConstants, pixelConstants};
}
const pipelines = new Map(logical(6).map(payload => [payload.readBigUInt64LE(0), pipelineState(payload)]));
const fixed = pipelines.get(draws[0].readBigUInt64LE(0)), shader = pipelines.get(draws[1].readBigUInt64LE(0));
assert.equal(fixed.vertex, 0x144); assert.equal(fixed.pixel, 0);
assert.equal(fixed.render[27], 1, 'recorded alpha blend state reaches the immutable draw');
assert.equal(fixed.render[137], 0); assert.deepEqual(fixed.viewport.slice(0, 4), [0, 0, 16, 16]);
assert.deepEqual(fixed.material.slice(0, 4), [0x3f800000, 0x3f800000, 0x3f800000, 0x3f800000]);
assert.deepEqual(fixed.lights[0].slice(0, 3), [0, 1, 3]);
assert.ok(shader.vertex >= 0x80000000 && shader.pixel >= 0x80000000);
assert.deepEqual(shader.vertexConstants.slice(0, 4), [0x3f800000, 0, 0, 0x3f800000]);
assert.deepEqual(shader.pixelConstants.slice(0, 4), [0x3f800000, 0, 0, 0x3f800000]);
assert.deepEqual(logical(14).map(payload => [payload.readUInt32LE(48), payload.readUInt32LE(80)]), [[3, 16384], [1, 1024], [2, 16384]], 'GPU copy, GPU-dirty lock, and front-buffer capture retain distinct jobs and exact extents');
const receipt = {engineHash, guestHash: hash(guest.bytes), calls, runs, callbacks, packets, commands, fragmentedCommands,
  claim: 'authored PE32 and actual generated Wasm exercise engine callbacks, COM and D3D8 semantic transport; synthetic host readback validates protocol ownership, not GPU rendering'};
writeFileSync(join(output, 'receipt.json'), JSON.stringify(receipt, null, 2));
console.log(JSON.stringify({engineHash, guestCalls: calls.length, guestRuns: runs.length, callbacks: callbacks.length, packets: packets.length, fragmentedCommands}));
