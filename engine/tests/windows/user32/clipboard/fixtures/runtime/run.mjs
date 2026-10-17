import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {mkdirSync, readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {host} from '../../../../threads/fixtures/runtime/host.mjs';
import {image} from '../producer/scenario.mjs';

const [enginePath, output, root] = process.argv.slice(2);
mkdirSync(output, {recursive: true});
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const engineHash = hash(readFileSync(enginePath));
assert.equal(engineHash, process.env.RING3_ENGINE_SHA256);
const guest = image();
writeFileSync(join(output, 'clipboard-authored.exe'), guest.bytes);
const engine = host(enginePath, root, output, guest, {
  gateOwners: new Set(guest.calls.map(call => call.id)).size,
  configure({api}) {
    assert.equal(api.configure_user32(16, 16, 64, 16, 0x2d000000, 16), 0);
    assert.equal(api.configure_gdi(64, 16, 16, 65536, 0x2c000000, 16, 16, 16), 0);
    assert.equal(api.register_user32_module(0x72000000), 0);
  },
});
const {api, key, get, take, read} = engine;
const calls = [];
for (const unit of engine.units) {
  const owner = engine.run(unit);
  assert.equal(api.capture_resident_call(...key, owner.low, owner.high, 2, unit.words), 0, unit.name);
  const token = take(112).readUInt32LE(16);
  assert.equal(api.complete_resident_windows_call(...key, owner.low, owner.high, token), 0, unit.name);
  let finished = unit.expected === 'exit';
  for (let step = 0; !finished && step < 16; ++step) {
    assert.equal(api.waiting_user32_message(), 0);
    const wait = take(64).readBigUInt64LE(24);
    if (wait === 0n) { finished = true; break; }
    assert.equal(api.resume_user32_wait(...key, Number(wait & 0xffffffffn), Number(wait >> 32n)), 0, unit.name);
  }
  assert.ok(finished, `${unit.name}: bounded owner continuation`);
  const result = get(16);
  writeFileSync(join(output, 'progress.json'), JSON.stringify({calls, current: {name: unit.name, result}}, null, 2));
  if (unit.expected === 'exit') {
    assert.equal(get(72), 9);
    assert.equal(get(80), 0);
  } else if (unit.expected === 'nonzero') assert.notEqual(result, 0, unit.name);
  else if (typeof unit.expected === 'object') assert.equal(result, read(guest.addresses[unit.expected.ref]), unit.name);
  else assert.equal(result, unit.expected, unit.name);
  if (unit.hook === 'utf16') assert.equal(read(read(guest.addresses.pointer)), 0x00df0041);
  if (unit.hook === 'ansi') assert.equal(read(result) & 0xffffff, 0xdf41);
  calls.push({name: unit.name, gate: unit.id, token, result});
}
const receipt = {engineHash, guestHash: hash(guest.bytes), calls, runs: engine.runs,
  claim: 'authored PE32 executes actual generated Wasm for transferred Global ownership, cached text conversion, clipboard retirement and canonical system colors; delayed rendering and foreign window callbacks are separate evidence'};
writeFileSync(join(output, 'receipt.json'), JSON.stringify(receipt, null, 2));
console.log(JSON.stringify({engineHash, calls: calls.length, generatedRuns: engine.runs.length}));
