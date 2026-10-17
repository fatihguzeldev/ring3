import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {mkdirSync, readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {host} from '../../../threads/fixtures/runtime/host.mjs';
import {image} from '../producer/scenario.mjs';

const [enginePath, output, root] = process.argv.slice(2);
mkdirSync(output, {recursive: true});
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const engineHash = hash(readFileSync(enginePath));
assert.equal(engineHash, process.env.RING3_ENGINE_SHA256);
const guest = image();
writeFileSync(join(output, 'foundations-authored.exe'), guest.bytes);
const engine = host(enginePath, root, output, guest, {gateOwners: new Set(guest.calls.map(call => call.id)).size, configure({api}) {
  assert.equal(api.configure_gdi(64, 16, 16, 65536, 0x2c000000, 16, 16, 16), 0);
  assert.equal(api.register_user32_module(0x72000000), 0);
  assert.equal(api.register_gdi32_module(0x73000000), 0);
  assert.equal(api.register_oleaut32_module(0x74000000), 0);
  assert.equal(api.register_shlwapi_module(0x75000000), 0);
  assert.equal(api.register_shell32_module(0x76000000), 0);
}});
const {api, key, get, take, read} = engine;
const pair = value => [Number(value & 0xffffffffn), Number(value >> 32n)];
let frames = 0, dibPointer;
for (const unit of engine.units) {
  const owner = engine.run(unit);
  assert.equal(api.capture_resident_call(...key, owner.low, owner.high, 2, unit.words), 0, unit.name);
  const token = take(112).readUInt32LE(16);
  assert.equal(api.complete_resident_windows_call(...key, owner.low, owner.high, token), 0, unit.name);
  if (unit.expected === 'exit') {
    assert.equal(get(72), 9); assert.equal(get(80), 0);
  } else if (unit.expected === 'nonzero') assert.notEqual(get(16), 0, unit.name);
  else assert.equal(get(16), unit.expected, unit.name);
  if (unit.name === 'CreateDIBSection') dibPointer = read(guest.addresses.bits);
  if (unit.hook === 'message') {
    const pointer = read(guest.addresses.message);
    assert.notEqual(pointer, 0);
    assert.deepEqual([0, 4, 8, 12].map(offset => read(pointer + offset)), [0x00650068, 0x006c006c, 0x0020006f, 0x00000037]);
  }
  if (unit.hook === 'ansi-message') {
    assert.equal(read(guest.addresses.message_buffer), 0xe9636163);
    assert.equal(read(guest.addresses.message_buffer + 4) & 255, 0);
  }
  if (unit.hook === 'short-message') {
    assert.equal(read(guest.addresses.short_output), 0x00320031);
    assert.equal(read(guest.addresses.short_output + 4), 0x45670000);
  }
  if (unit.hook === 'frame' || unit.hook === 'text') {
    assert.equal(api.pending_gdi_frame(0), 0);
    const frame = take(80);
    assert.equal(frame.subarray(0, 4).toString(), 'R3GD');
    assert.equal(frame.readUInt32LE(12), 1);
    assert.equal(frame.readUInt32LE(40), 16); assert.equal(frame.readUInt32LE(44), 16);
    assert.equal(frame.readUInt32LE(72), 1, 'top-down BGRX32');
    const id = frame.readBigUInt64LE(16), revision = frame.readBigUInt64LE(24);
    assert.notEqual(api.read_gdi_frame(...pair(id), ...pair(revision + 1n), 0, 4), 0);
    assert.equal(api.read_gdi_frame(...pair(id), ...pair(revision), 0, 1024), 0);
    const pixels = take(1024);
    if (unit.hook === 'frame') assert.deepEqual([...pixels.subarray(2 * 64 + 4, 2 * 64 + 8)], [0x11, 0x22, 0x33, 0]);
    else assert.ok(pixels.filter((byte, i) => i % 4 !== 3 && byte !== 0).length > 10, 'real font pixels beyond the earlier single pixel');
    assert.notEqual(api.acknowledge_gdi_frame(...pair(id), ...pair(revision + 1n)), 0);
    assert.equal(api.acknowledge_gdi_frame(...pair(id), ...pair(revision)), 0);
    assert.equal(api.pending_gdi_frame(0), 0); assert.equal(take(80).readUInt32LE(12), 0);
    frames++;
  }
}
assert.equal(read(guest.addresses.argc), 4);
assert.ok(read(guest.addresses.extent) > 0); assert.ok(read(guest.addresses.extent + 4) > 0);
assert.ok(dibPointer >= 0x2c000000 && dibPointer < 0x2c010000);
assert.equal(api.read32(dibPointer), 0); assert.notEqual(get(116), 0, 'process exit unmaps the still-live DIB');
assert.equal(api.pending_gdi_frame(0), 21, 'frame transport closes after process exit');
const receipt = {engineHash, guestHash: hash(guest.bytes), frames, calls: guest.calls.length,
  dibPointer, runs: engine.runs, claim: 'authored PE32 executes actual generated Wasm across USER32/GDI fonts/frame transport, BSTR, bounded ANSI comparison, shell heap ownership, ANSI/UTF16 message formatting with LocalFree and short-buffer LastError, and process exit with a live DIB'};
writeFileSync(join(output, 'receipt.json'), JSON.stringify(receipt, null, 2));
console.log(JSON.stringify({engineHash, frames, calls: guest.calls.length, dibPointer}));
