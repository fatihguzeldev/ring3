import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {mkdirSync, readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {ARENA_SIZE} from '../../../../support/wasm/arena.mjs';
import {assertEngineExports} from '../../../../support/wasm/exports.mjs';
import {cache, pair} from '../../../../support/wasm/cache.mjs';
import {lifecycleImage, WARMUPS, CYCLES, TOTAL, SMC} from '../producer/lifecycle.mjs';
import {configure} from './setup.mjs';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root);
mkdirSync(output, {recursive: true});
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const engineBytes = readFileSync(enginePath), engineHash = hash(engineBytes);
assert.equal(engineHash, process.env.RING3_ENGINE_SHA256);
const module = new WebAssembly.Module(engineBytes);
assertEngineExports(module, root);
const instance = new WebAssembly.Instance(module, {}), memory = instance.exports.memory;
const api = Object.fromEntries(Object.entries(instance.exports)
  .filter(([name]) => name.startsWith('ring3_abi_v1_'))
  .map(([name, fn]) => [name.slice(13), fn]));
const key = [0x61626364, 0xa1a2a3a4];
assert.equal(api.open(1024, ...key), 0);
const base = api.arena_ptr() >>> 0;
assert.equal(api.arena_len(), ARENA_SIZE);
const get = offset => new DataView(memory.buffer).getUint32(base + offset, true);
const take = length => Buffer.from(new Uint8Array(memory.buffer).slice(base + 140, base + 140 + length));
const request = bytes => {
  assert.ok(bytes.length <= 4096);
  new Uint8Array(memory.buffer).set(bytes, base + 140);
};
const name = value => {const bytes = Buffer.from(value); request(bytes); return bytes.length;};
const host = {
  api, memory, key, base, get, take, request, epoch: 0n,
  ok(operation, ...args) {
    assert.equal(api[operation](...args), 0,
      `${operation}(${args}) at PC=${get(48).toString(16)}`);
  },
};
const {ok} = host;
function read(address) {
  ok('read32', address >>> 0);
  assert.equal(get(116), 0);
  return get(120);
}
function metadata(kind) {
  const data = take(64);
  assert.equal(data.subarray(0, 4).toString(), 'R3MR');
  assert.equal(data.readUInt16LE(4), 1);
  assert.equal(data.readUInt16LE(6), kind);
  assert.equal(data.readUInt32LE(8), 64);
  assert.equal(data.readUInt32LE(12), 0);
  return {
    epoch: data.readBigUInt64LE(16),
    values: Array.from({length: 5}, (_, index) => data.readBigUInt64LE(24 + index * 8)),
  };
}
ok('configure_execution', 8, 1024 * 1024, 0xffffffff);
const context = configure({api, request, metadata});
ok('configure_threads', 4, 16, 16, 4, 256);
ok('files_info');
const fileInfo = take(64);
assert.equal(fileInfo.subarray(0, 4).toString(), 'R3FS');
const fileEpoch = fileInfo.readBigUInt64LE(16);
const guest = lifecycleImage(), a = guest.addresses;
writeFileSync(join(output, 'media-lifecycle-authored.exe'), guest.bytes);
ok('begin_module_input', guest.bytes.length, name('media-lifecycle-authored.exe'));
for (let offset = 0; offset < guest.bytes.length; offset += 4096) {
  const chunk = guest.bytes.subarray(offset, offset + 4096);
  request(chunk);
  ok('append_module_input', offset, chunk.length);
}
ok('commit_module_input');
ok('start_module_process', name('media-lifecycle-authored.exe'), 0, 0x7e000000, 4, 0x7f000000);
const gates = new Map(), entries = new Set();
for (let start = 0, total = 1; start < total;) {
  ok('module_gates_page', start, 128);
  const page = take(4096), count = page.readUInt32LE(24);
  assert.equal(page.subarray(0, 4).toString(), 'R3GP');
  assert.equal(page.readUInt32LE(20), start);
  assert.equal(page.readUInt32LE(8), 32 + count * 8);
  total = page.readUInt32LE(16);
  assert.ok(count > 0 && count <= 128);
  for (let index = 0; index < count; index++) {
    const id = page.readUInt32LE(36 + index * 8), pc = page.readUInt32LE(32 + index * 8);
    assert.ok(!gates.has(id) && !entries.has(pc));
    gates.set(id, pc);
    entries.add(pc);
  }
  start += count;
}
assert.ok(gates.has(0xffffe) && gates.has(0xfffff));
const focus = Buffer.alloc(64);
focus.write('R3IB');
focus.writeUInt16LE(1, 4);
focus.writeUInt16LE(32, 6);
focus.writeBigUInt64LE(context.mediaEpoch, 8);
focus.writeBigUInt64LE(1n, 16);
focus.writeUInt32LE(1, 24);
focus.writeBigUInt64LE(context.now, 32);
focus.writeUInt32LE(4, 40);
focus.writeUInt32LE(1, 44);
request(focus);
ok('submit_windows_input', focus.length);

const code = cache(host, output);
const callSites = new Map(guest.calls.map(call => [guest.entry + call.end, call]));
const cycleCalls = guest.calls.filter(call => call.phase === 'cycle');
const calls = [], callbacks = [], callbackReceipts = [], checkpoints = [], smc = [], schedules = [];
let needsSchedule = false, exited = false, currentThread = 1, childReturns = 0, apcReturns = 0;
let warmPages, oldSmc, totalRetired = 0;

function beginReadyCallback() {
  ok('pending_windows_callback');
  const pending = take(96);
  if (pending.readUInt32LE(12) === 0) return;
  assert.equal(callbacks.length, 0);
  assert.equal(pending.readUInt32LE(12), 1);
  assert.equal(pending.readUInt32LE(16), guest.callbacks[0].entry);
  assert.equal(pending.readUInt32LE(20), 4);
  const target = code.entry(pending.readUInt32LE(16));
  ok('begin_windows_resident_callback', ...key, ...pair(target.id), gates.get(0xffffe), 0xffffe);
  const record = take(72);
  assert.equal(record.subarray(0, 4).toString(), 'R3RC');
  const callback = {home: record.readBigUInt64LE(64), token: record.readUInt32LE(16), active: target.id};
  ok('authorize_resident_callback', ...key, ...pair(callback.home), callback.token);
  callbacks.push(callback);
  callbackReceipts.push({provider: 1, token: callback.token,
    continuation: pending.readBigUInt64LE(24).toString(),
    arguments: Array.from({length: 4}, (_, index) => pending.readUInt32LE(32 + index * 4))});
}

function finishMediaWait(call, token) {
  ok('pending_windows_wait');
  const initial = metadata(16);
  assert.equal(initial.values[0], BigInt(token));
  assert.equal(initial.values[4] & 1n, 0n);
  assert.equal(api.resume_windows_wait(...key, token), 12, 'unready media wait stays parked');
  if (call.hook === 'audio') {
    ok('audio_submit', 8);
    const packet = metadata(6);
    assert.equal(packet.epoch, context.mediaEpoch);
    ok('audio_read_submission', ...pair(packet.epoch), ...pair(packet.values[0]), 0, 8);
    const pcm = take(64), expected = [0, 0.25, 0.5, 0.25, 0, -0.25, -0.5, -0.25];
    for (let index = 0; index < 16; index++) {
      assert.equal(pcm.readFloatLE(index * 4), expected[index >> 1], `PCM sample ${index}`);
    }
    assert.equal(api.audio_acknowledge(...pair(packet.epoch), ...pair(packet.values[0] + 1n), 3), 40);
    ok('audio_acknowledge', ...pair(packet.epoch), ...pair(packet.values[0]), 3);
    ok('pending_windows_wait');
    assert.equal(metadata(16).values[4] & 1n, 0n);
    ok('audio_acknowledge', ...pair(packet.epoch), ...pair(packet.values[0]), 4);
    ok('audio_acknowledge', ...pair(packet.epoch), ...pair(packet.values[0]), 8);
  } else {
    assert.equal(call.hook, 'capture');
    ok('dsound_capture_request', 0);
    const packet = metadata(17), receipt = take(64);
    assert.equal(packet.epoch, context.mediaEpoch);
    assert.ok(packet.values[0] > 0n);
    assert.equal(packet.values[2], 0n, 'each newly created ring begins at frame zero');
    const pcm = Buffer.alloc(4);
    pcm.writeUInt32LE((0x44330000 | read(a.round)) >>> 0);
    const payload = Buffer.concat([receipt, pcm]);
    const stale = Buffer.from(payload);
    stale.writeBigUInt64LE(packet.epoch + 1n, 16);
    request(stale);
    assert.equal(api.dsound_receive_capture(stale.length), 7);
    request(payload);
    ok('dsound_receive_capture', payload.length);
    request(payload);
    assert.equal(api.dsound_receive_capture(payload.length), 7, 'capture receipt cannot replay');
  }
  ok('pending_windows_wait');
  assert.equal(metadata(16).values[4] & 1n, 1n);
  ok('resume_windows_wait', ...key, token);
  ok('pending_windows_wait');
  assert.equal(metadata(16).epoch, 0n);
}

function checkpoint() {
  const round = read(a.round);
  assert.equal(round, checkpoints.length);
  for (const call of cycleCalls) {
    if (call.hook === 'checkpoint') continue;
    const actual = read(call.status);
    if (call.expected === 'pointer') assert.ok(actual !== 0 && actual !== 0xffffffff, call.name);
    else if (call.expected === 'offset') assert.equal(actual, round * 4, call.name);
    else assert.equal(actual, call.expected, `${round}/${call.name}`);
  }
  assert.equal(read(a.childCount), round + 1);
  assert.equal(read(a.childParameter), round);
  assert.equal(read(a.childTidFromFs), read(a.childTid));
  assert.notEqual(read(a.childTid), 1);
  assert.notEqual(read(a.childTeb), read(a.mainTeb));
  assert.equal(read(a.childExit), (round ^ 0x13572468) >>> 0);
  assert.equal(read(a.apcCount), round + 1);
  assert.equal(read(a.apcArguments), round);
  assert.equal(read(a.apcTid), 1);
  assert.equal(read(a.apcTeb), read(a.mainTeb));
  assert.equal(read(a.captureWord), (0x44330000 | round) >>> 0);
  assert.equal(read(a.fileWord), round);
  assert.equal(read(a.fileCount), 4);
  assert.equal(read(a.smcBefore), (0x13570000 ^ round) >>> 0);
  assert.equal(read(a.smcUnflushed), (0x5a000000 ^ round) >>> 0);
  assert.equal(read(a.smcAfter), (0x5a000000 ^ round) >>> 0);
  assert.equal(childReturns, round + 1);
  assert.equal(apcReturns, round + 1);
  ok('audio_state');
  const audio = metadata(7);
  assert.equal(audio.epoch, context.mediaEpoch);
  assert.equal(audio.values[0], BigInt((round + 1) * 8));
  assert.equal(audio.values[1], audio.values[0]);
  assert.equal(audio.values[2], 0n);
  ok('dsound_capture_request', 0);
  assert.equal(metadata(17).values[0], 0n);
  ok('pending_windows_wait');
  assert.equal(metadata(16).epoch, 0n);
  ok('pending_file_call');
  assert.equal(take(64).readBigUInt64LE(16), 0n);
  ok('pending_windows_callback');
  assert.equal(take(96).readUInt32LE(12), 0);
  ok('cache_usage');
  const usage = take(80);
  assert.equal(usage.subarray(0, 4).toString(), 'R3CU');
  assert.equal(usage.readUInt32LE(8), 80);
  const units = usage.readUInt32LE(16), wasmBytes = usage.readUInt32LE(20);
  const installed = usage.readUInt32LE(24), evictions = usage.readBigUInt64LE(48);
  assert.ok(units <= 8 && installed <= units);
  assert.ok(wasmBytes <= 1024 * 1024);
  assert.equal(usage.readBigUInt64LE(64), 0n);
  const pages = memory.buffer.byteLength / 65536;
  if (round === WARMUPS - 1) warmPages = pages;
  if (round >= WARMUPS) assert.equal(pages, warmPages, 'steady integrated lifecycle memory plateau');
  checkpoints.push({round, pages, units, wasmBytes, installed, evictions: evictions.toString(),
    submittedFrames: audio.values[0].toString(), childTid: read(a.childTid)});
  if ((round + 1) % 16 === 0) progress();
}

function progress(error) {
  writeFileSync(join(output, 'progress.json'), JSON.stringify({engineHash, ...code.stats(),
    calls: calls.slice(-64), callbacks: callbacks.map(row => ({...row,
      home: row.home.toString(), active: row.active.toString()})),
    checkpoints, smc: smc.slice(-8), schedules: schedules.slice(-16),
    childReturns, apcReturns, totalRetired, error: error?.stack}, null, 2));
}

try {
  for (let iteration = 0; iteration < 100000 && !exited; iteration++) {
    if (get(72) === 9) {assert.equal(get(80), 0); exited = true; break;}
    if (needsSchedule || api.pending_windows_thread_schedule()) {
      needsSchedule = false;
      ok('schedule_windows_thread');
      const schedule = take(40);
      assert.equal(schedule.subarray(0, 4).toString(), 'R3TS');
      assert.equal(schedule.readUInt32LE(12), 1, 'real callback/thread completion must make a context runnable');
      currentThread = schedule.readUInt32LE(16);
      schedules.push({thread: currentThread, pc: get(48)});
    }
    const pc = get(48), callback = callbacks.at(-1);
    const smcCall = pc === SMC ? guest.smcReturns.indexOf(read(get(32))) : -1;
    if (pc === SMC) assert.ok(smcCall >= 0, 'SMC entry belongs to an actual authored guest CALL');
    if (smcCall === 1) {
      assert.ok(oldSmc && oldSmc.round === read(a.round));
      assert.ok(code.isInstalled(oldSmc.unit), 'old SMC owner must remain installed until source-staleness check');
      assert.equal(read(SMC + 1), (0x5a000000 ^ oldSmc.round) >>> 0);
      const before = Buffer.from(new Uint8Array(memory.buffer).slice(base, base + ARENA_SIZE));
      assert.notEqual(oldSmc.unit.generated.run(base, base + 56, 1, base + 96), 0,
        'naturally reached same PC rejects the old source version');
      assert.deepEqual(Buffer.from(new Uint8Array(memory.buffer).slice(base, base + ARENA_SIZE)), before,
        'stale source rejection preserves the full active guest arena');
    }
    const unit = code.entry(pc);
    if (pc === SMC) {
      if (smcCall === 0) {
        assert.equal(read(SMC + 1), (0x13570000 ^ read(a.round)) >>> 0);
        oldSmc = {unit, round: read(a.round)};
      } else if (smcCall === 1) {
        assert.notEqual(unit.id, oldSmc.unit.id);
        smc.push({round: oldSmc.round, previous: oldSmc.unit.id.toString(), fresh: unit.id.toString()});
      } else {
        assert.equal(smcCall, 2);
        assert.equal(smc.at(-1).round, read(a.round));
        smc.at(-1).postFlush = unit.id.toString();
      }
    }
    if (callback) {
      if (get(72) === 3) {
        ok('select_resident_callback_unit', ...key, ...pair(callback.home), callback.token, ...pair(unit.id));
        callback.active = unit.id;
      } else {
        assert.equal(get(72), 1, 'bounded callback continuation keeps its active unit');
        assert.equal(unit.id, callback.active);
      }
    }
    const run = code.execute(unit);
    totalRetired += run.retired;
    if (run.reason === 1 || run.reason === 3) continue;
    assert.equal(run.reason, 8, JSON.stringify(run));
    if (callback && run.detail === 0xffffe && run.pc === gates.get(0xffffe)) {
      ok('finish_windows_resident_callback', ...key, ...pair(callback.home), callback.token);
      callbacks.pop();
      beginReadyCallback();
      continue;
    }
    if (callback) ok('capture_active_resident_callback_windows_call', ...key, ...pair(unit.id), callback.token);
    else ok('capture_resident_windows_call', ...key, ...pair(unit.id));
    const captured = take(112), token = captured.readUInt32LE(16);
    assert.equal(captured.subarray(0, 4).toString(), 'R3CF');
    assert.equal(captured.readUInt32LE(8), 112);
    assert.equal(captured.readUInt32LE(20), run.detail);
    assert.equal(captured.readUInt32LE(24), 2);
    const call = callSites.get(captured.readUInt32LE(40));
    if (run.detail === 0xfffff) {
      assert.notEqual(currentThread, 1);
      assert.equal(get(16), (read(a.round) ^ 0x13572468) >>> 0);
      childReturns++;
    } else if (run.detail === 0xffffe) {
      assert.equal(currentThread, 1);
      apcReturns++;
    } else {
      assert.ok(call, `authored return PC ${captured.readUInt32LE(40).toString(16)}`);
      assert.equal(call.id, run.detail);
      assert.equal(captured.readUInt32LE(28), call.arguments.length);
      assert.deepEqual(call.arguments.map((argument, index) => captured.readUInt32LE(48 + index * 4)),
        call.arguments.map(argument => typeof argument === 'object' ? read(argument.address) : argument >>> 0));
    }
    const receipt = {id: run.detail, token, thread: currentThread,
      owner: unit.id.toString(), returnPc: captured.readUInt32LE(40), name: call?.name,
      arguments: Array.from({length: captured.readUInt32LE(28)}, (_, index) => captured.readUInt32LE(48 + index * 4))};
    if (callback) ok('complete_active_resident_callback_windows_call', ...key, ...pair(unit.id), callback.token, token);
    else ok('complete_resident_windows_call', ...key, ...pair(unit.id), token);
    calls.push(receipt);
    if (get(72) === 9) {assert.equal(get(80), 0); exited = true; break;}
    if (call?.hook === 'audio' || call?.hook === 'capture') finishMediaWait(call, token);
    if (call?.hook === 'checkpoint') checkpoint();
    if (call?.hook === 'join' || call?.hook === 'apc' || run.detail === 0xffffe || run.detail === 0xfffff) {
      needsSchedule = true;
    }
    beginReadyCallback();
  }
  assert.ok(exited, 'one sustained instance must finish through actual ExitProcess');
  assert.equal(read(a.round), TOTAL);
  assert.equal(childReturns, TOTAL);
  assert.equal(apcReturns, TOTAL);
  assert.equal(checkpoints.length, TOTAL);
  assert.equal(smc.length, TOTAL);
  assert.equal(callbacks.length, 0);
  assert.ok(callbackReceipts.length > 0);
  assert.equal(read(a.windowProcCount), callbackReceipts.length);
  for (const call of guest.calls.filter(call => call.phase !== 'cycle' && call.expected !== 'exit')) {
    const actual = read(call.status);
    if (call.expected === 'pointer') assert.ok(actual !== 0 && actual !== 0xffffffff, call.name);
    else if (call.expected === 'atom') assert.ok(actual >= 0xc000 && actual <= 0xffff, call.name);
    else assert.equal(actual, call.expected, call.name);
  }
  assert.equal(calls.length,
    guest.calls.filter(call => call.phase !== 'cycle').length + (cycleCalls.length + 2) * TOTAL);
  assert.ok(code.stats().evictions > TOTAL);
  assert.ok(totalRetired > 0);
  let save, overlayCount = 0;
  for (let index = 0; index < 64; index++) {
    ok('file_overlay_entry', ...pair(fileEpoch), index);
    const header = take(96);
    if (header.readBigUInt64LE(16) === 0n) break;
    overlayCount++;
    const path = take(96 + header.readUInt32LE(56) * 2).subarray(96).toString('utf16le');
    if (path.toLowerCase() !== 'c:\\lifecycle.bin') continue;
    assert.equal(save, undefined);
    assert.equal(header.readUInt32LE(52), 1);
    assert.equal(header.readBigUInt64LE(40), BigInt(TOTAL * 4));
    ok('read_file_overlay', ...pair(fileEpoch), ...pair(header.readBigUInt64LE(32)),
      ...pair(header.readBigUInt64LE(24)), 0, 0, TOTAL * 4);
    const data = take(32 + TOTAL * 4);
    assert.equal(data.subarray(0, 4).toString(), 'R3FD');
    assert.equal(data.readUInt32LE(16), TOTAL * 4);
    assert.equal(data.readUInt32LE(20), 0);
    for (let round = 0; round < TOTAL; round++) assert.equal(data.readUInt32LE(32 + round * 4), round);
    save = {path, sha256: hash(data.subarray(32)), bytes: TOTAL * 4};
  }
  assert.ok(save, 'all guest appends survive normal teardown');
  assert.ok(overlayCount <= 8, 'one save and the configured virtual directories have bounded overlay metadata');
  const receipt = {engineHash, guestHash: hash(guest.bytes), warmups: WARMUPS, cycles: CYCLES,
    calls, callbackReceipts, schedules, checkpoints, smc, childReturns, apcReturns, totalRetired,
    ...code.stats(), warmPages, overlayCount, save, generatedDigest: code.finish(),
    claim: 'one authored PE32/generated-Wasm instance repeatedly allocates, streams, captures and disposes media; executes real child threads/APCs/waits and VFS appends with fixed cache budgets, guest-authored SMC rejection, exact output witnesses and a warmed linear-memory plateau; no browser or game-throughput guarantee'};
  writeFileSync(join(output, 'receipt.json'), JSON.stringify(receipt, null, 2));
  console.log(JSON.stringify({engineHash, cycles: TOTAL, guestCalls: calls.length,
    childReturns, apcReturns, warmPages, ...code.stats(), save}));
} catch (error) {
  progress(error);
  throw error;
}
