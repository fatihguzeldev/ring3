import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {mkdirSync, readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {ARENA_SIZE} from '../../../../support/wasm/arena.mjs';
import {assertEngineExports} from '../../../../support/wasm/exports.mjs';
import {cache, pair} from '../../../../support/wasm/cache.mjs';

const [enginePath, output, root] = process.argv.slice(2);
const artifacts = process.env.RING3_OSS_CRT_FIXTURE_DIR;
assert.ok(artifacts, 'select the pinned source-built OSS CRT fixture directory');
mkdirSync(output, {recursive: true});
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const provenance = JSON.parse(readFileSync(join(artifacts, 'provenance.json')));
assert.equal(provenance.sourceCommit, '51bfc7037359b594876a349b46b992e363e79838');
for (const [name, expected] of Object.entries(provenance.producerSources)) {
  assert.equal(hash(readFileSync(join(root, provenance.producerDirectory, name))), expected, name);
}
assert.equal(hash(readFileSync(join(root, provenance.inspectionSource))), provenance.inspectionSha256);
const engineBytes = readFileSync(enginePath), engineHash = hash(engineBytes);
assert.equal(engineHash, process.env.RING3_ENGINE_SHA256);
const engineModule = new WebAssembly.Module(engineBytes);
assertEngineExports(engineModule, root);
const instance = new WebAssembly.Instance(engineModule, {}), memory = instance.exports.memory;
const api = Object.fromEntries(Object.entries(instance.exports).filter(([name]) => name.startsWith('ring3_abi_v1_')).map(([name, fn]) => [name.slice(13), fn]));
const key = [0x34561278, 0x87215643];
assert.equal(api.open(2048, ...key), 0);
const base = api.arena_ptr() >>> 0;
assert.equal(api.arena_len(), ARENA_SIZE);
const get = offset => new DataView(memory.buffer).getUint32(base + offset, true);
const take = length => Buffer.from(new Uint8Array(memory.buffer).slice(base + 140, base + 140 + length));
const request = data => {assert.ok(data.length <= 4096); new Uint8Array(memory.buffer).set(data, base + 140);};
const name = value => {const bytes = Buffer.from(value); request(bytes); return bytes.length;};
const host = {api, memory, key, base, get, take, request, epoch: 0n,
  ok(operation, ...args) {assert.equal(api[operation](...args), 0, `${operation}(${args}) at PC=${get(48).toString(16)}`);}};
const {ok} = host;
ok('configure_cpu_clock', 1_000_000_000, 0, 0, 0);
ok('configure_execution', 128, 16 * 1024 * 1024, 0xffffffff);
ok('configure_synchronization', 64, 128, 256, 32);
ok('configure_threads', 4, 16, 16, 8, 256);
ok('configure_user32', 16, 16, 128, 32, 0x35000000, 16);
ok('configure_module_limits', 8, 4 * 1024 * 1024, 16, 65536, 1024 * 1024);
ok('register_kernel32_module', 0x70000000);
ok('register_user32_module', 0x72000000);
const fileConfig = Buffer.alloc(80);
fileConfig.write('R3FC'); fileConfig.writeUInt16LE(1, 4); fileConfig.writeUInt16LE(1, 6); fileConfig.writeUInt32LE(80, 8);
[512, 128, 2, 256, 16, 16 * 1024 * 1024, 256, 16, 16, 32, 16, 1024 * 1024, 0x30000000, 64].forEach((value, index) => fileConfig.writeUInt32LE(value, 16 + index * 4));
request(fileConfig); ok('configure_files', 80);
const fileEpoch = take(64).readBigUInt64LE(16);
for (const file of ['msvcrt.dll', 'hosted-dll.dll', 'hosted.exe']) {
  const bytes = readFileSync(join(artifacts, file));
  assert.equal(hash(bytes), provenance.artifacts[file].sha256, file);
  ok('begin_module_input', bytes.length, name(file));
  for (let at = 0; at < bytes.length; at += 4096) {const part = bytes.subarray(at, at + 4096); request(part); ok('append_module_input', at, part.length);}
  ok('commit_module_input');
}
ok('start_module_process', name('hosted.exe'), 0, 0x7e000000, 32, 0x7f000000);
const code = cache(host, output), calls = [], initializers = [], exceptions = [], recent = [], callbacks = [], continuations = [], schedules = [], threadInitializers = [];
let totalRetired = 0, exited = false, acceptedExit, acceptedThreadExit, needsSchedule = false, currentThread = 1;
function initializer() {
  ok('module_initializer'); const packet = take(72);
  assert.equal(packet.subarray(0, 4).toString(), 'R3MO');
  if (packet.readUInt32LE(16) !== 1) return undefined;
  return {token: packet.readBigUInt64LE(24), entry: packet.readUInt32LE(40), returnPc: packet.readUInt32LE(44), returnId: packet.readUInt32LE(48), reason: packet.readUInt32LE(56), kind: packet.readUInt32LE(60)};
}
function progress(error) {
  writeFileSync(join(output, 'progress.json'), JSON.stringify({engineHash, ...code.stats(), calls: calls.slice(-128), initializers, exceptions, recent, continuations, schedules, threadInitializers, callbacks: callbacks.map(row => ({...row, home: row.home.toString(), id: row.id.toString()})), error: error?.stack}, null, 2));
}
try {
  for (let iteration = 0; iteration < 100000 && !exited; iteration++) {
    if (acceptedExit && get(48) === acceptedExit.pc && get(72) === 8 && get(80) === 0x10003) {
      ok('complete_resident_windows_call', ...key, ...pair(acceptedExit.owner), acceptedExit.token);
      continuations.push({id: 0x10003, token: acceptedExit.token, owner: acceptedExit.owner.toString()});
      needsSchedule = false;
    }
    if (acceptedThreadExit && currentThread === acceptedThreadExit.thread && get(48) === acceptedThreadExit.pc && get(72) === 8 && get(80) === 0xfffff) {
      ok('complete_resident_windows_call', ...key, ...pair(acceptedThreadExit.owner), acceptedThreadExit.token);
      continuations.push({id: 0xfffff, token: acceptedThreadExit.token, owner: acceptedThreadExit.owner.toString(), thread: currentThread});
      needsSchedule = false;
    }
    if (get(72) === 9) {assert.equal(get(80), provenance.expectedExit); exited = true; break;}
    if (needsSchedule || api.pending_windows_thread_schedule()) {
      needsSchedule = false;
      ok('schedule_windows_thread'); const schedule = take(40);
      assert.equal(schedule.subarray(0, 4).toString(), 'R3TS');
      const state = schedule.readUInt32LE(12), thread = schedule.readUInt32LE(16);
      schedules.push({state, thread, pc: get(48)});
      if (state === 3) {assert.equal(schedule.readUInt32LE(20), provenance.expectedExit); exited = true; break;}
      assert.equal(state, 1, 'the hosted fixture must have runnable guest context');
      assert.ok(thread > 0);
      currentThread = thread;
      if (acceptedThreadExit && acceptedThreadExit.thread !== thread) acceptedThreadExit = undefined;
    }
    if (get(72) === 9) {assert.equal(get(80), provenance.expectedExit); exited = true; break;}
    const init = initializer(), unit = code.entry(get(48));
    const crt = provenance.artifacts['msvcrt.dll'];
    if (currentThread !== 1 && get(48) === crt.imageBase + crt.entryRva) {
      ok('read32', get(32) + 8);
      assert.equal(get(116), 0);
      const reason = get(120);
      assert.ok(reason === 2 || reason === 3);
      threadInitializers.push({thread: currentThread, reason, entry: get(48)});
    }
    const callback = callbacks.at(-1);
    if (callback && !init) ok('select_resident_callback_unit', ...key, ...pair(callback.home), callback.token, ...pair(unit.id));
    const run = code.execute(unit); totalRetired += run.retired;
    recent.push(run); if (recent.length > 64) recent.shift();
    if (run.reason === 1 || run.reason === 3) continue;
    if (run.reason === 9) {assert.equal(run.detail, provenance.expectedExit); exited = true; break;}
    if ([10, 11, 12, 13, 16, 17, 18, 19, 20, 21, 22, 23].includes(run.reason)) {
      ok('dispatch_resident_windows_exception', ...key, ...pair(unit.id), ...pair(host.epoch));
      const packet = take(64); exceptions.push({phase: packet.readUInt32LE(28), code: packet.readUInt32LE(52)});
      assert.notEqual(packet.readUInt32LE(28), 6, `unhandled compiler guest exception ${packet.toString('hex')}`);
      continue;
    }
    assert.equal(run.reason, 8, JSON.stringify(run));
    if (init && run.detail === init.returnId && run.pc === init.returnPc) {
      ok('finish_module_initializer', ...pair(init.token)); initializers.push({entry: init.entry, reason: init.reason, kind: init.kind});
      continue;
    }
    if (run.detail === 0x121000) {
      ok('pending_windows_exception'); const packet = take(64);
      assert.equal(packet.readUInt32LE(12), 1); assert.notEqual(packet.readUInt32LE(28), 6);
      exceptions.push({phase: packet.readUInt32LE(28), code: packet.readUInt32LE(52)});
      ok('finish_windows_exception_handler', ...key, ...pair(packet.readBigUInt64LE(16)));
      continue;
    }
    if (callback && !init && run.detail === callback.returnId && run.pc === callback.returnPc) {
      ok('finish_windows_resident_callback', ...key, ...pair(unit.id), callback.token); callbacks.pop(); continue;
    }
    if (callback && !init) ok('capture_active_resident_callback_windows_call', ...key, ...pair(unit.id), callback.token);
    else ok('capture_resident_windows_call', ...key, ...pair(unit.id));
    const captured = take(112), token = captured.readUInt32LE(16);
    calls.push({id: run.detail, pc: run.pc, token, eax: get(16)});
    if (run.detail === 0x10003) {
      assert.equal(acceptedExit, undefined, 'normal hosted termination issues one actual ExitProcess');
      acceptedExit = {owner: unit.id, token, pc: run.pc};
    }
    if (run.detail === 0xfffff) {
      assert.equal(acceptedThreadExit, undefined, 'one actual child return owns its pending teardown');
      acceptedThreadExit = {owner: unit.id, token, pc: run.pc, thread: currentThread};
    }
    if (callback && !init) ok('complete_active_resident_callback_windows_call', ...key, ...pair(unit.id), callback.token, token);
    else ok('complete_resident_windows_call', ...key, ...pair(unit.id), token);
    if (get(72) === 9) {assert.equal(get(80), provenance.expectedExit); exited = true; break;}
    ok('pending_windows_callback'); const pending = take(96);
    if (pending.readUInt32LE(12)) {
      const target = code.entry(pending.readUInt32LE(16));
      ok('module_gates_page', 0, 128);
      const returnId = 0xffffe; let returnPc;
      for (let start = 0, total = 1; start < total;) {
        ok('module_gates_page', start, 128); const page = take(4096), count = page.readUInt32LE(24); total = page.readUInt32LE(16);
        for (let index = 0; index < count; index++) if (page.readUInt32LE(36 + index * 8) === returnId) returnPc = page.readUInt32LE(32 + index * 8);
        start += count;
      }
      assert.ok(returnPc);
      ok('begin_windows_resident_callback', ...key, ...pair(target.id), returnPc, returnId);
      const record = take(72); callbacks.push({home: record.readBigUInt64LE(56), id: target.id, token: record.readUInt32LE(16), returnPc, returnId});
    }
    if (run.detail === 0xffffe) needsSchedule = true;
    ok('pending_windows_wait'); const wait = take(64);
    assert.equal(wait.subarray(0, 4).toString(), 'R3MR');
    assert.equal(wait.readUInt16LE(6), 16);
    if (wait.readBigUInt64LE(16) !== 0n) needsSchedule = true;
    if (calls.length % 100 === 0) progress();
  }
  assert.ok(exited, 'bounded genuine hosted CRT execution must reach normal ExitProcess');
  assert.equal(callbacks.length, 0);
  assert.ok(totalRetired > 0);
  assert.ok(initializers.some(row => row.reason === 1 && row.kind === 2), 'real CRT DllMain ran');
  assert.ok(initializers.some(row => row.reason === 0 && row.kind === 2), 'real hosted DLL detach ran');
  if (provenance.threadOracle) {
    assert.ok(threadInitializers.some(row => row.reason === 2), 'original CRT DLL_THREAD_ATTACH ran in child context');
    assert.ok(threadInitializers.some(row => row.reason === 3), 'original CRT DLL_THREAD_DETACH ran in child context');
    assert.equal(calls.filter(row => row.id === 0xfffff).length, 1, 'one real child stdcall return');
  }
  const expectedPath = provenance.expectedFile.toLowerCase(); let found;
  for (let index = 0; index < 512; index++) {
    ok('file_overlay_entry', ...pair(fileEpoch), index); const header = take(96);
    if (header.readBigUInt64LE(16) === 0n) break;
    const path = take(96 + header.readUInt32LE(56) * 2).subarray(96).toString('utf16le');
    if (path.toLowerCase() === expectedPath) {
      assert.equal(header.readUInt32LE(52), 1); assert.equal(header.readBigUInt64LE(40), BigInt(provenance.expectedBytes.length));
      ok('read_file_overlay', ...pair(fileEpoch), ...pair(header.readBigUInt64LE(32)), ...pair(header.readBigUInt64LE(24)), 0, 0, provenance.expectedBytes.length);
      const data = take(32 + provenance.expectedBytes.length); assert.equal(data.subarray(0, 4).toString(), 'R3FD'); assert.equal(data.subarray(32).toString(), provenance.expectedBytes); found = path;
    }
  }
  assert.ok(found, 'guest stdio save exists after accepted process exit');
  const receipt = {engineHash, artifacts: provenance.artifacts, ...code.stats(), totalRetired, calls, initializers, exceptions, continuations, schedules, threadInitializers, finalPath: found, finalBytes: provenance.expectedBytes, generatedDigest: code.finish(),
    claim: 'genuine pinned open-source CRT and original hosted startup execute compiler-produced C++ EH/RAII across an authored DLL, distinct main/child compiler TLS, real CRT thread attach/detach, setjmp/longjmp, child/main stdio/allocation, join, atexit/global destructors and FLS Free/thread/process-exit callbacks; no general Windows/game or browser-SDK guarantee'};
  writeFileSync(join(output, 'receipt.json'), JSON.stringify(receipt, null, 2));
  console.log(JSON.stringify({engineHash, calls: calls.length, ...code.stats(), totalRetired, finalBytes: provenance.expectedBytes}));
} catch (error) {progress(error); throw error;}
