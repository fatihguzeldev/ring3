import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {execFileSync} from 'node:child_process';
import {mkdirSync, readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {ARENA_SIZE} from '../../../../../support/wasm/arena.mjs';
import {assertEngineExports} from '../../../../../support/wasm/exports.mjs';
import {cache, pair} from '../../../../../support/wasm/cache.mjs';
import {verifyLog} from './oracles.mjs';

const [enginePath, output, root] = process.argv.slice(2);
const artifacts = process.env.RING3_DIALOG_FIXTURE_DIR;
assert.ok(artifacts, 'select the source-built authored freestanding dialog fixture');
mkdirSync(output, {recursive: true});
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const provenance = JSON.parse(readFileSync(join(artifacts, 'provenance.json')));
for (const [name, expected] of Object.entries(provenance.producerSources)) {
  assert.equal(hash(readFileSync(join(root, provenance.producerDirectory, name))), expected, name);
}
assert.equal(hash(readFileSync(join(root, provenance.inspectionSource))), provenance.inspectionSha256);
const engineBytes = readFileSync(enginePath), engineHash = hash(engineBytes);
assert.equal(engineHash, process.env.RING3_ENGINE_SHA256);
const module = new WebAssembly.Module(engineBytes); assertEngineExports(module, root);
const instance = new WebAssembly.Instance(module, {}), memory = instance.exports.memory;
const api = Object.fromEntries(Object.entries(instance.exports).filter(([name]) => name.startsWith('ring3_abi_v1_')).map(([name, value]) => [name.slice(13), value]));
const key = [0x4449414c, 0x54454d50];
assert.equal(api.open(2048, ...key), 0);
const base = api.arena_ptr() >>> 0;
assert.equal(api.arena_len(), ARENA_SIZE);
const get = offset => new DataView(memory.buffer).getUint32(base + offset, true);
const take = length => Buffer.from(new Uint8Array(memory.buffer).slice(base + 140, base + 140 + length));
const request = bytes => {assert.ok(bytes.length <= 4096); new Uint8Array(memory.buffer).set(bytes, base + 140);};
const name = value => {const bytes = Buffer.from(value); request(bytes); return bytes.length;};
const host = {api, memory, key, base, get, take, request, epoch: 0n,
  ok(operation, ...args) {assert.equal(api[operation](...args), 0, `${operation}(${args}) at PC=${get(48).toString(16)}`);}};
const {ok} = host;
ok('configure_cpu_clock', 1_000_000_000, 0, 0, 0);
ok('configure_execution', 128, 16 * 1024 * 1024, 0xffffffff);
ok('configure_synchronization', 64, 128, 256, 32);
ok('configure_module_limits', 16, 4 * 1024 * 1024, 16, 65536, 1024 * 1024);
ok('configure_user32', 16, 32, 128, 32, 0x30000000, 64);
ok('configure_gdi', 128, 64, 32, 1024 * 1024, 0x2c000000, 256, 320, 240);
ok('register_kernel32_module', 0x70000000);
ok('register_user32_module', 0x72000000);
ok('register_gdi32_module', 0x73000000);
const config = Buffer.alloc(80);
config.write('R3FC'); config.writeUInt16LE(1, 4); config.writeUInt16LE(1, 6); config.writeUInt32LE(80, 8);
[512, 128, 2, 256, 16, 16 * 1024 * 1024, 256, 16, 16, 32, 16, 1024 * 1024, 0x31000000, 64].forEach((value, index) => config.writeUInt32LE(value, 16 + index * 4));
request(config); ok('configure_files', 80);
const fileEpoch = take(64).readBigUInt64LE(16);
const client = 'dialogs-authored.exe';
const guestBytes = readFileSync(join(artifacts, client));
assert.equal(hash(guestBytes), provenance.artifacts[client].sha256);
assert.deepEqual(Object.keys(provenance.artifacts[client].imports).map(value => value.toLowerCase()).filter(value => !['kernel32.dll', 'user32.dll', 'gdi32.dll'].includes(value)), []);
ok('begin_module_input', guestBytes.length, name(client));
for (let at = 0; at < guestBytes.length; at += 4096) {const part = guestBytes.subarray(at, at + 4096); request(part); ok('append_module_input', at, part.length);}
ok('commit_module_input');
ok('start_module_process', name(client), 0, 0x7e000000, 32, 0x7f000000);
const code = cache(host, output), calls = [], initializers = [], callbacks = [], callbackHistory = [], recent = [], schedules = [];
let totalRetired = 0, exited = false, needsSchedule = false;
const returnId = 0xffffe;
let returnPc;
for (let start = 0, total = 1; start < total;) {
  ok('module_gates_page', start, 128); const page = take(4096), count = page.readUInt32LE(24); total = page.readUInt32LE(16);
  for (let index = 0; index < count; index++) if (page.readUInt32LE(36 + index * 8) === returnId) returnPc = page.readUInt32LE(32 + index * 8);
  assert.ok(count > 0); start += count;
}
assert.ok(returnPc, 'canonical Windows callback return gate');
function initializer() {
  ok('module_initializer'); const record = take(72);
  assert.equal(record.subarray(0, 4).toString(), 'R3MO');
  if (record.readUInt32LE(16) !== 1) return undefined;
  return {token: record.readBigUInt64LE(24), entry: record.readUInt32LE(40), returnPc: record.readUInt32LE(44), returnId: record.readUInt32LE(48)};
}
function beginReadyCallback() {
  ok('pending_windows_callback'); const pending = take(96);
  if (!pending.readUInt32LE(12)) return;
  assert.equal(pending.readUInt32LE(12), 1, 'real USER32 provider');
  assert.equal(pending.readUInt32LE(20), 4);
  const unit = code.entry(pending.readUInt32LE(16));
  ok('begin_windows_resident_callback', ...key, ...pair(unit.id), returnPc, returnId);
  const record = take(72);
  const callback = {home: record.readBigUInt64LE(64), token: record.readUInt32LE(16), procedure: pending.readUInt32LE(16), active: unit.id};
  ok('authorize_resident_callback', ...key, ...pair(callback.home), callback.token);
  callbacks.push(callback); callbackHistory.push({procedure: callback.procedure, arguments: [0, 1, 2, 3].map(index => pending.readUInt32LE(32 + index * 4))});
}
function progress(error) {
  writeFileSync(join(output, 'progress.json'), JSON.stringify({engineHash, ...code.stats(), totalRetired, calls, callbacks, callbackHistory, initializers, schedules, recent, error: error?.stack}, (_key, value) => typeof value === 'bigint' ? value.toString() : value, 2));
}
function retainGuestLog() {
  for (let index = 0; index < 512; index++) {
    ok('file_overlay_entry', ...pair(fileEpoch), index); const header = take(96);
    if (!header.readBigUInt64LE(16)) break;
    const path = take(96 + header.readUInt32LE(56) * 2).subarray(96).toString('utf16le');
    if (path.toLowerCase() !== provenance.expectedFile.toLowerCase()) continue;
    assert.equal(header.readUInt32LE(52), 1);
    const length = Number(header.readBigUInt64LE(40)); assert.ok(length > 0 && length <= 65536);
    const parts = [];
    for (let offset = 0; offset < length; offset += 4000) {
      const count = Math.min(4000, length - offset);
      ok('read_file_overlay', ...pair(fileEpoch), ...pair(header.readBigUInt64LE(32)), ...pair(header.readBigUInt64LE(24)), offset, 0, count);
      parts.push(take(32 + count).subarray(32));
    }
    const bytes = Buffer.concat(parts);
    writeFileSync(join(output, 'guest-log.bin'), bytes);
    return bytes;
  }
  return undefined;
}
try {
  for (let iteration = 0; iteration < 200000 && !exited; iteration++) {
    if (needsSchedule || api.pending_windows_thread_schedule()) {
      needsSchedule = false;
      ok('schedule_windows_thread'); const record = take(40);
      assert.equal(record.subarray(0, 4).toString(), 'R3TS');
      const state = record.readUInt32LE(12);
      schedules.push({state, thread: record.readUInt32LE(16), pc: get(48)});
      if (state === 3) {assert.equal(record.readUInt32LE(20), 0); exited = true; break;}
      assert.ok(state === 1 || state === 4, 'the posted modal command leaves runnable guest code or a real callback');
      assert.equal(record.readUInt32LE(16), 1);
    }
    if (get(72) === 9) {assert.equal(get(80), 0); exited = true; break;}
    const init = initializer();
    if (!init) beginReadyCallback();
    const unit = code.entry(get(48)), callback = callbacks.at(-1);
    if (callback && !init) {
      if (get(72) === 3) {
        ok('select_resident_callback_unit', ...key, ...pair(callback.home), callback.token, ...pair(unit.id));
        callback.active = unit.id;
      } else {
        assert.equal(get(72), 1, 'budget continuation retains its active unit');
        assert.equal(unit.id, callback.active);
      }
    }
    const run = code.execute(unit); totalRetired += run.retired;
    recent.push(run); if (recent.length > 64) recent.shift();
    if ([1, 3].includes(run.reason)) continue;
    if (run.reason === 9) {assert.equal(run.detail, 0); exited = true; break;}
    assert.equal(run.reason, 8, `authored dialog guest has no expected CPU exception: ${JSON.stringify(run)}`);
    if (init && run.pc === init.returnPc && run.detail === init.returnId) {
      ok('finish_module_initializer', ...pair(init.token)); initializers.push({entry: init.entry}); continue;
    }
    if (callback && !init && run.pc === returnPc && run.detail === returnId) {
      ok('finish_windows_resident_callback', ...key, ...pair(callback.home), callback.token); callbacks.pop(); needsSchedule = true; continue;
    }
    if (callback && !init) ok('capture_active_resident_callback_windows_call', ...key, ...pair(unit.id), callback.token);
    else ok('capture_resident_windows_call', ...key, ...pair(unit.id));
    const token = take(112).readUInt32LE(16);
    calls.push({id: run.detail, pc: run.pc, token});
    if (callback && !init) ok('complete_active_resident_callback_windows_call', ...key, ...pair(unit.id), callback.token, token);
    else ok('complete_resident_windows_call', ...key, ...pair(unit.id), token);
    if (get(72) === 9) {assert.equal(get(80), 0); exited = true; break;}
    needsSchedule = true;
    if (calls.length % 50 === 0) progress();
  }
  assert.ok(exited, 'authored client reaches its own successful ExitProcess');
  assert.equal(callbacks.length, 0); assert.ok(totalRetired > 0);
  const bytes = retainGuestLog(); assert.ok(bytes, 'guest log survives normal process exit');
  const rows = verifyLog(bytes);
  writeFileSync(join(output, 'guest-log.json'), JSON.stringify(rows, null, 2));
  for (const id of [0x20068, 0x20069, 0x2006c, 0x2006d, 0x2006e, 0x2006f, 0x20070]) assert.ok(calls.some(call => call.id === id), `actual dialog API ${id.toString(16)}`);
  assert.ok(callbackHistory.some(row => row.arguments[1] === 0x110));
  const paths = execFileSync('rg', ['--files', 'engine/src', 'engine/tests/windows/user32/dialogs'], {cwd: root, encoding: 'utf8'}).trim().split('\n').concat(['engine/tests/support/wasm/cache.mjs', provenance.inspectionSource]).sort();
  writeFileSync(join(output, 'source-sha256.json'), JSON.stringify(Object.fromEntries(paths.map(path => [path, hash(readFileSync(join(root, path)))])), null, 2));
  const receipt = {engineHash, artifacts: provenance.artifacts, ...code.stats(), totalRetired, calls, callbackHistory, initializers, schedules, rows, generatedDigest: code.finish(), claim: provenance.claim};
  writeFileSync(join(output, 'receipt.json'), JSON.stringify(receipt, null, 2));
  console.log(JSON.stringify({engineHash, calls: calls.length, callbacks: callbackHistory.length, rows: rows.length, ...code.stats(), totalRetired}));
} catch (error) {
  if (get(72) === 9) {try {retainGuestLog();} catch {}}
  progress(error); throw error;
}
