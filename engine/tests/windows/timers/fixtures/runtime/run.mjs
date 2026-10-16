import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {mkdirSync, readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {image} from '../producer/scenario.mjs';
import {host} from '../../../threads/fixtures/runtime/host.mjs';

const [enginePath, output, root] = process.argv.slice(2);
mkdirSync(output, {recursive: true});
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const engineHash = hash(readFileSync(enginePath));
assert.equal(engineHash, process.env.RING3_ENGINE_SHA256);
const guest = image(), a = guest.addresses;
assert.equal(guest.calls.length, 29);
assert.equal(guest.segments.length, 33);
writeFileSync(join(output, 'timers-authored.exe'), guest.bytes);
const engine = host(enginePath, root, output, guest, {
  gateOwners: 18,
  configure({api}) {
    assert.equal(api.configure_resources(0x31000000, 16, 32, 8, 0x100000), 0);
    assert.equal(api.configure_activation(16, 64, 16, 0x10000, 4096, 16, 0x100000, 0x6a000000, 256), 0);
    assert.equal(api.configure_user32(8, 8, 32, 8, 0x30000000, 8), 0);
    assert.equal(api.register_user32_module(0x72000000), 0);
    assert.equal(api.register_winmm_module(0x77000000), 0);
  },
});
const {api, key, get, take, read} = engine;
const callback = engine.compile('window-timer-callback', [
  {entry: guest.windowCallback.entry, length: guest.windowCallback.contextEnd},
  {entry: guest.windowCallback.entry + guest.windowCallback.contextEnd,
    length: guest.windowCallback.releaseEnd - guest.windowCallback.contextEnd},
  {entry: guest.windowCallback.entry + guest.windowCallback.releaseEnd,
    length: guest.windowCallback.length - guest.windowCallback.releaseEnd},
  {entry: guest.returnPc, length: 2},
], [{entry: guest.returnPc, id: guest.returnId}]);
const callbackTable = new WebAssembly.Table({element: 'anyfunc', initial: 8, maximum: 8});
const callbackUnits = [callback, ...[0x180007, 0x180004].map(id => {
  const unit = engine.units.find(unit => unit.id === id)?.gateOwner;
  assert.ok(unit, `canonical callback service ${id.toString(16)}`);
  return unit;
})];
callbackUnits.forEach((unit, slot) => {
  callbackTable.set(slot, unit.generated.run);
  assert.equal(api.acknowledge_resident_installation(...key, unit.low, unit.high, slot), 0);
  const installed = take(32);
  assert.equal(installed.subarray(0, 4).toString(), 'R3IN');
  assert.deepEqual([16, 20, 24, 28].map(offset => installed.readUInt32LE(offset)),
    [unit.low, unit.high, slot, 0]);
});
const calls = [], schedules = [], callbacks = [], callbackCalls = [], executed = new Set();
let exited = false;
function schedule(expectedThread) {
  for (let iteration = 0; iteration < 8; iteration++) {
    assert.equal(api.schedule_windows_thread(), 0);
    const record = take(40);
    assert.equal(record.subarray(0, 4).toString(), 'R3TS');
    const state = record.readUInt32LE(12), thread = record.readUInt32LE(16);
    if (state === 2) {
      assert.equal(record.readUInt32LE(32), 1);
      const deadline = record.readBigUInt64LE(24);
      assert.ok([10_000_000n, 15_000_000n, 20_000_000n, 30_000_000n].includes(deadline));
      schedules.push({state, deadline: deadline.toString()});
      assert.equal(api.observe_cpu_clock(Number(deadline), Number(deadline >> 32n)), 0);
      continue;
    }
    assert.equal(state, 1);
    assert.equal(thread, expectedThread);
    schedules.push({state, thread, pc: get(48)});
    return;
  }
  assert.fail('bounded timer scheduling');
}
function finishWindowCallback() {
  assert.equal(api.pending_windows_callback(), 0);
  const pending = take(96);
  assert.equal(pending.readUInt32LE(12), 1);
  assert.equal(pending.readUInt32LE(16), guest.windowCallback.entry);
  assert.equal(api.begin_windows_resident_callback(...key, callback.low, callback.high,
    guest.returnPc, guest.returnId), 0);
  const record = take(72);
  assert.equal(record.subarray(0, 4).toString(), 'R3RC');
  assert.equal(record.readUInt32LE(48), 4);
  const token = record.readUInt32LE(16);
  const home = [record.readUInt32LE(64), record.readUInt32LE(68)];
  assert.deepEqual(home, [callback.low, callback.high]);
  assert.equal(record.readUInt32LE(28), 0, 'callback and suspended outer unit remain current');
  assert.equal(api.authorize_resident_callback(...key, ...home, token), 0);
  let completed = false, retired = 0;
  let active, select = true;
  const runs = [];
  for (let iteration = 0; iteration < 16; iteration++) {
    const pc = get(48);
    if (select) {
      active = pc >= guest.windowCallback.entry && pc < guest.windowCallback.entry + guest.windowCallback.length || pc === guest.returnPc
        ? callback : engine.units.find(unit => unit.gateOwner.target === pc)?.gateOwner;
      assert.ok(active, `declared window callback entry ${pc.toString(16)}`);
      assert.equal(api.select_resident_callback_unit(...key, ...home, token, active.low, active.high), 0);
    }
    active.generated.run(api.arena_ptr(), api.arena_ptr() + 56, 1000, api.arena_ptr() + 96);
    const reason = get(72), count = get(76), detail = get(80);
    retired += count;
    runs.push({reason, retired: count, pc: get(48), detail});
    if (reason === 1 || reason === 3) {
      select = reason === 3;
      continue;
    }
    assert.equal(reason, 8);
    if ([0x180007, 0x180004].includes(detail)) {
      assert.equal(api.capture_active_resident_callback_windows_call(...key, active.low, active.high, token), 0);
      const callToken = take(112).readUInt32LE(16);
      assert.equal(api.complete_active_resident_callback_windows_call(...key, active.low, active.high, token, callToken), 0);
      if (detail === 0x180007) {
        assert.equal(get(16), 1);
        assert.equal(read(a.windowContext), read(a.activation));
      }
      callbackCalls.push({id: detail, token: callToken});
      select = true;
      continue;
    }
    assert.equal(detail, guest.returnId);
    assert.equal(get(48), guest.returnPc);
    assert.ok(retired > 0);
    completed = true;
    break;
  }
  assert.ok(completed);
  assert.equal(api.finish_windows_resident_callback(...key, ...home, token), 0);
  callbacks.push({provider: 1, token, runs});
}
function finalOracles() {
  for (const call of guest.calls.filter(call => call.expected !== 'exit')) {
    const result = read(call.status);
    if (call.expected === 'void') continue;
    if (call.expected === 'pointer') assert.ok(result !== 0 && result !== 0xffffffff, call.name);
    else assert.equal(result, call.expected, call.name);
  }
  assert.equal(read(a.mainTid), 1);
  assert.equal(read(a.multimediaCount), 1);
  assert.equal(read(a.multimediaTid), 2);
  assert.notEqual(read(a.mainTeb), read(a.multimediaTeb));
  assert.equal(read(a.multimediaArguments), read(a.multimediaTimer));
  assert.equal(read(a.multimediaArguments + 4), 0);
  assert.equal(read(a.multimediaArguments + 8), 0x12345678);
  assert.equal(read(a.multimediaArguments + 12), 0);
  assert.equal(read(a.multimediaArguments + 16), 0);
  assert.equal(read(a.multimediaSleepStatus), 0);
  assert.equal(read(a.windowCount), 1);
  assert.equal(read(a.windowTid), 1);
  assert.equal(read(a.windowTeb), read(a.mainTeb));
  assert.equal(read(a.windowArguments), 0);
  assert.equal(read(a.windowArguments + 4), 0x113);
  assert.equal(read(a.windowArguments + 8), read(a.windowTimer));
  assert.equal(read(a.windowArguments + 12), 30);
  assert.equal(read(a.message + 4), 0x113);
  assert.equal(read(a.message + 8), read(a.windowTimer));
  assert.equal(read(a.message + 12), guest.windowCallback.entry);
  assert.equal(callbacks.length, 1);
  assert.deepEqual(callbackCalls.map(call => call.id), [0x180007, 0x180004]);
  assert.equal(read(a.mainContext), 0);
  assert.equal(read(a.windowContext), read(a.activation));
  assert.equal(read(a.multimediaContext), read(a.activation));
}
for (let iteration = 0; iteration < 64 && !exited; iteration++) {
  const pc = get(48), unit = engine.units.find(unit => unit.start <= pc && pc < unit.limit);
  assert.ok(unit, `authored timer entry ${pc.toString(16)}`);
  assert.ok(!executed.has(unit.start), `guest segment replay ${unit.name}`);
  const owner = engine.run(unit);
  assert.equal(api.capture_resident_call(...key, owner.low, owner.high, 2, unit.words), 0, unit.name);
  const token = take(112).readUInt32LE(16);
  if (unit.expected === 'exit') finalOracles();
  assert.equal(api.complete_resident_windows_call(...key, owner.low, owner.high, token), 0, unit.name);
  if (unit.name === 'multimedia-context') {
    assert.equal(get(16), unit.expected, unit.name);
  }
  executed.add(unit.start);
  calls.push({name: unit.name, thread: unit.thread, token});
  if (unit.expected === 'exit') {
    assert.equal(get(72), 9);
    exited = true;
    break;
  }
  if (unit.hook === 'dispatch-timer') finishWindowCallback();
  if (unit.hook === 'main-context-cleared') assert.equal(read(a.mainContext), 0);
  if (['multimedia-sleep', 'multimedia-return', 'event-timer', 'window-due'].includes(unit.hook)) schedule(1);
  if (['wait-first-multimedia', 'synchronous-kill'].includes(unit.hook)) schedule(2);
  if (unit.hook === 'peek-timer') {
    assert.equal(read(a.message + 4), 0x113);
    assert.equal(read(a.message + 8), read(a.windowTimer));
    assert.equal(read(a.windowCount), 0, 'PeekMessage cannot execute TIMERPROC');
  }
  writeFileSync(join(output, 'progress.json'), JSON.stringify({calls, schedules, callbacks, runs: engine.runs}, null, 2));
}
assert.ok(exited);
assert.equal(calls.length, 33);
assert.equal(schedules.filter(item => item.state === 2).length, 4);
const receipt = {engineHash, guestHash: hash(guest.bytes), calls, schedules, callbacks, callbackCalls, runs: engine.runs,
  claim: 'authored PE32 GetCurrentActCtx proves registration context on a preexisting NULL-context multimedia worker and lazy killed window timer after owner deactivation; executes multimedia callback on distinct worker TEB, synchronous kill across worker Sleep, event-timer wait and lazy window TIMERPROC on owner TID; no wall-clock accuracy claim'};
writeFileSync(join(output, 'receipt.json'), JSON.stringify(receipt, null, 2));
console.log(JSON.stringify({engineHash, guestCalls: calls.length, multimediaCallbacks: 1, windowCallbacks: 1, timerDeadlines: 4}));
