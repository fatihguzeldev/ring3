import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {execFileSync} from 'node:child_process';
import {mkdirSync, readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {ARENA_SIZE} from '../../../../support/wasm/arena.mjs';
import {assertEngineExports} from '../../../../support/wasm/exports.mjs';
import {image} from '../producer/scenario.mjs';
import {configure} from './setup.mjs';
import {beforeActivationCall, afterActivationCall} from './activation.mjs';
import {beforeRawInputCall, afterRawInputCall} from './rawinput.mjs';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root); mkdirSync(output, {recursive: true});
const hash = value => createHash('sha256').update(value).digest('hex');
const engineBytes = readFileSync(enginePath), engineHash = hash(engineBytes);
assert.equal(engineHash, process.env.RING3_ENGINE_SHA256, 'exact selected engine artifact');
const engineModule = new WebAssembly.Module(engineBytes); assertEngineExports(engineModule, root);
const instance = new WebAssembly.Instance(engineModule, {}), memory = instance.exports.memory;
const api = Object.fromEntries(Object.entries(instance.exports).filter(([name]) => name.startsWith('ring3_abi_v1_')).map(([name, fn]) => [name.slice(13), fn]));
const key = [0x51525354, 0x91929394], transfer = 140;
assert.equal(api.open(1024, ...key), 0);
const base = api.arena_ptr() >>> 0; assert.equal(api.arena_len(), ARENA_SIZE);
const bytes = () => new Uint8Array(memory.buffer);
const get = offset => new DataView(memory.buffer).getUint32(base + offset, true);
const take = length => Buffer.from(bytes().slice(base + transfer, base + transfer + length));
const request = value => {assert.ok(value.length <= 4096); bytes().set(value, base + transfer);};
const name = value => {const data = Buffer.from(value); request(data); return data.length;};
const pair = value => [Number(value & 0xffffffffn), Number(value >> 32n)];
const read = address => {assert.equal(api.read32(address >>> 0), 0); assert.equal(get(116), 0); return get(120);};
function metadata(kind) {
  const data = take(64); assert.equal(data.subarray(0, 4).toString(), 'R3MR');
  assert.equal(data.readUInt16LE(6), kind); assert.equal(data.readUInt32LE(8), 64); assert.equal(data.readUInt32LE(12), 0);
  return {epoch: data.readBigUInt64LE(16), values: Array.from({length: 5}, (_, i) => data.readBigUInt64LE(24 + i * 8))};
}
const context = configure({api, request, metadata});
const guest = image().seal(); assert.equal(guest.calls.length, 143); writeFileSync(join(output, 'windows-media-authored.exe'), guest.bytes);
assert.equal(api.begin_module_input(guest.bytes.length, name('windows-media-authored.exe')), 0);
for (let offset = 0; offset < guest.bytes.length; offset += 4096) {
  const chunk = guest.bytes.subarray(offset, offset + 4096); request(chunk);
  assert.equal(api.append_module_input(offset, chunk.length), 0);
}
assert.equal(api.commit_module_input(), 0);
assert.equal(api.start_module_process(name('windows-media-authored.exe'), 0, 0x7e000000, 4, 0x7f000000), 0);
assert.equal(get(48), guest.entry);
const gateRows = [];
for (let start = 0, total = 1; start < total;) {
  assert.equal(api.module_gates_page(start, 128), 0); const data = take(4096);
  assert.equal(data.subarray(0, 4).toString(), 'R3GP'); assert.equal(data.readUInt32LE(20), start);
  const count = data.readUInt32LE(24); total = data.readUInt32LE(16);
  assert.equal(data.readUInt32LE(8), 32 + 8 * count); assert.equal(data.readUInt32LE(28), 0);
  assert.ok(count > 0 && count <= 128);
  for (let i = 0; i < count; i++) gateRows.push([data.readUInt32LE(36 + i * 8), data.readUInt32LE(32 + i * 8)]);
  start += count;
}
const gates = new Map(gateRows);
assert.equal(gates.size, gateRows.length, 'public, COM and initializer gate IDs are distinct');
assert.equal(new Set(gateRows.map(([, entry]) => entry)).size, gateRows.length);
writeFileSync(join(output, 'gates.json'), JSON.stringify(gateRows));
const runs = [], calls = [], callbacks = [], waits = [], pcm = [];
let inputSequence = 0n;
function input(events) {
  const batch = Buffer.alloc(32 + 32 * events.length); batch.write('R3IB'); batch.writeUInt16LE(1, 4); batch.writeUInt16LE(32, 6);
  batch.writeBigUInt64LE(context.mediaEpoch, 8); batch.writeBigUInt64LE(++inputSequence, 16); batch.writeUInt32LE(events.length, 24);
  events.forEach(([kind, a, b = 0, c = 0], index) => {
    const at = 32 + index * 32; batch.writeBigUInt64LE(context.now, at);
    [kind, a, b, c].forEach((value, j) => batch.writeUInt32LE(value >>> 0, at + 8 + j * 4));
  });
  request(batch); assert.equal(api.submit_windows_input(batch.length), 0);
}
function compile(call) {
  const pc = get(48), target = gates.get(call.id); assert.ok(target, `gate ${call.id.toString(16)} exists`);
  const specs = [{entry: pc, length: guest.entry + call.end - pc}, {entry: target, length: 2},
    ...guest.callbacks.map(({entry}) => ({entry, length: 8})), {entry: guest.returnPc, length: 2}];
  const selected = [{entry: target, id: call.id}, {entry: guest.returnPc, id: guest.returnId}];
  const descriptors = Buffer.alloc(specs.length * 8 + selected.length * 8);
  specs.forEach((item, i) => {descriptors.writeUInt32LE(item.entry, i * 8); descriptors.writeUInt32LE(item.length, i * 8 + 4);});
  selected.forEach((item, i) => {descriptors.writeUInt32LE(item.entry, specs.length * 8 + i * 8); descriptors.writeUInt32LE(item.id, specs.length * 8 + i * 8 + 4);});
  request(descriptors); assert.equal(api.compile_with_gates(specs.length, selected.length), 0, call.name);
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
    if (reason === 3) continue;
    assert.equal(reason, 8, `${label}: gate exit`); assert.equal(detail, expectedGate); assert.ok(totalRetired > 0); return;
  }
  assert.fail(`${label}: bounded generated execution`);
}
function finishCallbacks(generated, generation) {
  for (let iteration = 0; iteration < 64; iteration++) {
    assert.equal(api.pending_windows_callback(), 0); const request = take(96);
    const provider = request.readUInt32LE(12); if (!provider) return;
    assert.ok([1, 3, 4].includes(provider)); assert.ok(guest.callbacks.some(({entry}) => entry === request.readUInt32LE(16)));
    assert.equal(api.begin_windows_callback(...key, generation, guest.returnPc, guest.returnId), 0);
    const token = take(64).readUInt32LE(16); run(generated, `callback-${provider}`, guest.returnId);
    assert.equal(api.finish_windows_callback(...key, generation, token), 0);
    callbacks.push({provider, continuation: request.readBigUInt64LE(24).toString()});
  }
  assert.fail('bounded callback continuations');
}
function wake(call, token) {
  assert.equal(api.pending_windows_wait(), 0); const wait = metadata(16);
  assert.equal(wait.values[0], BigInt(token)); assert.equal(wait.values[4] & 1n, 0n, 'wait initially yields');
  assert.equal(api.resume_windows_wait(...key, token), 12, 'unready wait remains parked');
  if (call.hook === 'timeout') {
    context.now += 5_000_000n; assert.equal(api.observe_cpu_clock(...pair(context.now)), 0);
  } else if (call.hook === 'host-event') {
    assert.equal(api.signal_windows_event(...pair(wait.epoch + 1n), read(guest.addresses.event)), 40, 'stale host epoch cannot signal');
    assert.equal(api.signal_windows_event(...pair(wait.epoch), read(guest.addresses.event)), 0);
  } else if (call.hook === 'audio-notification') {
    assert.equal(api.audio_submit(8), 0); const packet = metadata(6), sequence = packet.values[0];
    assert.equal(api.audio_read_submission(...pair(packet.epoch), ...pair(sequence), 0, 8), 0);
    const output = take(64), samples = Array.from({length: 16}, (_, i) => output.readFloatLE(i * 4)); pcm.push(samples);
    const gain = 10 ** (-602 / 2000), expected = [0, 0.25, 0.5, 0.25, 0, -0.25, -0.5, -0.25];
    samples.forEach((sample, i) => assert.ok(Math.abs(sample - expected[i >> 1] * gain) < 1e-6, `literal resampled sample ${i}`));
    assert.equal(api.audio_acknowledge(...pair(packet.epoch), ...pair(sequence), 3), 0);
    assert.equal(api.pending_windows_wait(), 0); assert.equal(metadata(16).values[4] & 1n, 0n, 'production and partial consumption before offset do not notify');
    assert.equal(api.audio_acknowledge(...pair(packet.epoch), ...pair(sequence), 4), 0);
    assert.equal(api.audio_acknowledge(...pair(packet.epoch), ...pair(sequence), 8), 0);
  } else if (call.hook === 'input-notification') {
    input([[1, 0x1e, 1], [3, 7, -3, 120]]);
  } else if (call.hook === 'media-reset') {
    const oldEpoch = context.mediaEpoch;
    assert.equal(api.reset_media(), 0);
    context.mediaEpoch = metadata(1).epoch; inputSequence = 0n;
    assert.ok(context.mediaEpoch > oldEpoch, 'reset replaces the media authority');
    assert.equal(api.sync_windows_input(), 0, 'new epoch has no stale User32 journal cursor');
    assert.equal(api.pending_user32_input(), 0); assert.equal(metadata(18).values[0], 0n);
    const stale = Buffer.alloc(64); stale.write('R3IB'); stale.writeUInt16LE(1, 4); stale.writeUInt16LE(32, 6);
    stale.writeBigUInt64LE(oldEpoch, 8); stale.writeBigUInt64LE(1n, 16); stale.writeUInt32LE(1, 24);
    stale.writeBigUInt64LE(context.now, 32); stale.writeUInt32LE(4, 40); stale.writeUInt32LE(1, 44);
    request(stale); assert.equal(api.submit_windows_input(stale.length), 40, 'old host input cannot enter replacement media');
  } else if (call.hook === 'capture-notification') {
    assert.equal(api.dsound_capture_request(0), 0); const captured = metadata(17), receipt = take(64);
    assert.equal(captured.epoch, context.mediaEpoch); assert.ok(captured.values[0] !== 0n);
    const payload = Buffer.concat([receipt, Buffer.from([0x11, 0x22, 0x33, 0x44])]);
    const stale = Buffer.from(payload); stale.writeBigUInt64LE(captured.values[2] + 1n, 40); request(stale);
    assert.equal(api.dsound_receive_capture(stale.length), 7, 'wrong capture frame is neutral');
    request(payload); assert.equal(api.dsound_receive_capture(payload.length), 0);
    request(payload); assert.equal(api.dsound_receive_capture(payload.length), 7, 'capture packet cannot replay');
  } else assert.fail(`unknown wait hook ${call.hook}`);
  assert.equal(api.pending_windows_wait(), 0); const ready = metadata(16);
  assert.equal(ready.values[4] & 1n, 1n, 'accepted host progress makes wait ready');
  assert.equal(api.resume_windows_wait(...key, token), 0);
  assert.equal(api.pending_windows_wait(), 0); assert.equal(metadata(16).epoch, 0n);
  waits.push({hook: call.hook, token, wait: wait.values[1].toString()});
}
for (const call of guest.calls) {
  beforeActivationCall(call, {api, context, pair});
  beforeRawInputCall(call, input);
  if (call.hook === 'focus') input([[4, 1], [5, 1]]);
  const generated = compile(call), generation = api.generation(); run(generated, call.name, call.id);
  assert.equal(api.capture_windows_call(...key, generation), 0); const token = take(112).readUInt32LE(16);
  assert.equal(api.complete_windows_call(...key, generation, token), 0, `${call.name}: engine completion`);
  if (call.expected !== 'exit') {
    finishCallbacks(generated, generation);
    if (['timeout', 'host-event', 'audio-notification', 'input-notification', 'capture-notification', 'media-reset'].includes(call.hook)) wake(call, token);
  }
  const result = get(16);
  if (call.expected === 'pointer') assert.ok(result !== 0, `${call.name}: identity`);
  else if (call.expected === 'atom') assert.ok(result >= 0xc000);
  else if (call.expected === 'exit') assert.equal(get(72), 9);
  else if (typeof call.expected === 'object') assert.equal(result & call.expected.mask, call.expected.value, `${call.name}: low return bits`);
  else assert.equal(result, call.expected, `${call.name}: result`);
  if (call.name === 'KeyboardState') assert.equal(read(guest.addresses.keyState), 0x80000000, 'custom offset3 observes physical A');
  if (call.name === 'QueryBuffer8') assert.equal(read(guest.addresses.soundBuffer), read(guest.addresses.buffer8), 'Buffer and Buffer8 alias the native shared pointer');
  if (call.name === 'QueryCapture8') assert.equal(read(guest.addresses.captureBuffer), read(guest.addresses.capture8), 'CaptureBuffer and CaptureBuffer8 share pointer');
  if (call.name === 'UnlockCapture') assert.equal(read(guest.addresses.capturedWord), 0x44332211, 'provided PCM reaches guest lock mapping');
  if (['PeekKeyboardData', 'ReadKeyboardData'].includes(call.name)) {
    assert.equal(read(guest.addresses.eventCount), 1); assert.equal(read(guest.addresses.eventData), 3);
    assert.equal(read(guest.addresses.eventData + 4), 0x80);
  }
  if (call.name === 'EmptyKeyboardData') assert.equal(read(guest.addresses.eventCount), 0);
  afterActivationCall(call, {guest, read});
  afterRawInputCall(call, {guest, read});
  calls.push({name: call.name, id: call.id, result, generation});
  writeFileSync(join(output, 'progress.json'), JSON.stringify({calls, runs, callbacks, waits, pcm}, null, 2));
}
const sourcePaths = execFileSync('rg', ['--files', 'engine/src', 'engine/tests/windows/media'], {cwd: root, encoding: 'utf8'}).trim().split('\n').sort();
writeFileSync(join(output, 'source-sha256.json'), JSON.stringify(Object.fromEntries(sourcePaths.map(path => [path, hash(readFileSync(join(root, path)))])), null, 2));
const receipt = {engineHash, guestHash: hash(guest.bytes), calls, runs, callbacks, waits, pcm,
  claim: 'authored PE32/generated Wasm prove engine PCM, input and wait semantics; host input is synthetic and PCM is inspected rather than played by a browser'};
writeFileSync(join(output, 'receipt.json'), JSON.stringify(receipt, null, 2));
console.log(JSON.stringify({engineHash, guestCalls: calls.length, guestRuns: runs.length, callbacks: callbacks.length, waits: waits.length, pcmPackets: pcm.length}));
