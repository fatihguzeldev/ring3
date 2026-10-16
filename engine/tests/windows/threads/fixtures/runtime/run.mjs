import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {mkdirSync, readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {image} from '../producer/scenario.mjs';
import {host} from './host.mjs';
import {checkFinal, checkImmediate} from './oracles.mjs';

const [enginePath, output, root] = process.argv.slice(2);
mkdirSync(output, {recursive: true});
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const engineHash = hash(readFileSync(enginePath));
assert.equal(engineHash, process.env.RING3_ENGINE_SHA256);
const guest = image();
assert.equal(guest.calls.length, 27);
assert.equal(guest.segments.length, 35);
writeFileSync(join(output, 'threads-authored.exe'), guest.bytes);
const engine = host(enginePath, root, output, guest), {api, key, get, take, read} = engine;
const calls = [], schedules = [], executed = new Set();
let exited = false;
function schedule(expectedThread) {
  for (let iteration = 0; iteration < 8; iteration++) {
    assert.equal(api.schedule_windows_thread(), 0);
    const record = take(40);
    assert.equal(record.subarray(0, 4).toString(), 'R3TS');
    const state = record.readUInt32LE(12), thread = record.readUInt32LE(16);
    if (state === 2) {
      assert.equal(record.readUInt32LE(32), 1, 'waiting must expose the virtual timer deadline');
      const deadline = record.readBigUInt64LE(24);
      assert.equal(deadline, 1000000n);
      schedules.push({state, deadlineNs: deadline.toString()});
      assert.equal(api.observe_cpu_clock(Number(deadline), 0), 0);
      continue;
    }
    assert.equal(state, 1, 'the selected guest context must be runnable');
    if (expectedThread !== undefined) assert.equal(thread, expectedThread);
    schedules.push({state, thread, pc: get(48)});
    return;
  }
  assert.fail('bounded guest scheduling');
}
for (let iteration = 0; iteration < 64 && !exited; iteration++) {
  const pc = get(48), unit = engine.units.find(unit => unit.start <= pc && pc < unit.limit);
  assert.ok(unit, `authored guest entry ${pc.toString(16)}`);
  assert.ok(!executed.has(unit.start), `guest segment replay ${unit.name}`);
  const owner = engine.run(unit);
  assert.equal(api.capture_resident_call(...key, owner.low, owner.high, 2, unit.words), 0, unit.name);
  const token = take(112).readUInt32LE(16);
  if (unit.expected === 'exit') {
    checkFinal(engine, guest);
    assert.equal(calls.length, 34);
  }
  assert.equal(api.complete_resident_windows_call(...key, owner.low, owner.high, token), 0, unit.name);
  executed.add(unit.start);
  calls.push({name: unit.name, thread: unit.thread, id: unit.id, token});
  if (unit.expected === 'exit') {
    assert.equal(get(72), 9);
    exited = true;
    break;
  }
  if (!unit.private) checkImmediate(unit, engine, guest);
  if (unit.hook === 'schedule-child') schedule(2);
  if (unit.hook === 'child-blocked') {
    assert.equal(read(guest.addresses.childEntered), 0);
    schedule(1);
  }
  if (['wait-child', 'child-return', 'wait-apc', 'apc-return', 'wait-timer', 'timer-return'].includes(unit.hook)) {
    schedule(unit.hook === 'wait-child' ? 2 : 1);
  }
  writeFileSync(join(output, 'progress.json'), JSON.stringify({calls, schedules, runs: engine.runs}, null, 2));
}
assert.ok(exited);
assert.equal(calls.length, 35);
assert.equal(calls.filter(call => call.id === 0xfffff).length, 1);
assert.equal(calls.filter(call => call.id === 0xffffe).length, 2);
assert.equal(schedules.filter(item => item.state === 2).length, 1);
const receipt = {engineHash, guestHash: hash(guest.bytes), calls, schedules, runs: engine.runs,
  durationProfile: 'trusted fixture supplies 123400 virtual execution nanoseconds per generated run; this is an accounting oracle, not a wall-clock performance measurement',
  claim: 'authored PE32/generated resident Wasm execute suspended guest thread startup/return, distinct TEB/TLS, recursive critical contention, shared thread waits, queued APC and timer callback delivery'};
writeFileSync(join(output, 'receipt.json'), JSON.stringify(receipt, null, 2));
console.log(JSON.stringify({engineHash, guestCalls: calls.length, guestRuns: engine.runs.length,
  threadReturns: 1, apcReturns: 2, timerDeadlines: 1}));
