import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {execFileSync} from 'node:child_process';
import {mkdirSync, readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {ARENA_SIZE} from '../../../../support/wasm/arena.mjs';
import {assertEngineExports} from '../../../../support/wasm/exports.mjs';
import {cache, pair} from '../../../../support/wasm/cache.mjs';
import {configure} from './setup.mjs';

const [enginePath, output, root] = process.argv.slice(2);
const artifacts = process.env.RING3_COM_FIXTURE_DIR;
assert.ok(artifacts, 'select the source-built authored freestanding COM fixture');
mkdirSync(output, {recursive: true});
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const provenance = JSON.parse(readFileSync(join(artifacts, 'provenance.json')));
for (const [name, expected] of Object.entries(provenance.producerSources)) {
  assert.equal(hash(readFileSync(join(root, 'engine/tests/windows/com/fixtures/producer', name))), expected, name);
}
assert.equal(hash(readFileSync(join(root, provenance.inspectionSource))), provenance.inspectionSha256);
const engineBytes = readFileSync(enginePath), engineHash = hash(engineBytes);
assert.equal(engineHash, process.env.RING3_ENGINE_SHA256);
const module = new WebAssembly.Module(engineBytes); assertEngineExports(module, root);
const instance = new WebAssembly.Instance(module, {}), memory = instance.exports.memory;
const api = Object.fromEntries(Object.entries(instance.exports).filter(([name]) => name.startsWith('ring3_abi_v1_')).map(([name, value]) => [name.slice(13), value]));
const key = [0x36561278, 0x87215643];
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
const fileEpoch = configure(host);
for (const file of ['com-server.dll', 'com-no-unload.dll', 'com-failed.dll', 'com-client.exe']) {
  const bytes = readFileSync(join(artifacts, file));
  assert.equal(hash(bytes), provenance.artifacts[file].sha256, file);
  assert.deepEqual(Object.keys(provenance.artifacts[file].imports).map(value => value.toLowerCase()).filter(value => !['kernel32.dll', 'ole32.dll', 'advapi32.dll'].includes(value)), []);
  ok('begin_module_input', bytes.length, name(file));
  for (let at = 0; at < bytes.length; at += 4096) {const part = bytes.subarray(at, at + 4096); request(part); ok('append_module_input', at, part.length);}
  ok('commit_module_input');
}
ok('start_module_process', name('com-client.exe'), 0, 0x7e000000, 32, 0x7f000000);
const code = cache(host, output), calls = [], initializers = [], callbacks = [], callbackHistory = [], recent = [];
let totalRetired = 0, exited = false;
const returnId = 0xffffe;
let returnPc;
for (let start = 0, total = 1; start < total;) {
  ok('module_gates_page', start, 128); const page = take(4096), count = page.readUInt32LE(24); total = page.readUInt32LE(16);
  for (let index = 0; index < count; index++) if (page.readUInt32LE(36 + index * 8) === returnId) returnPc = page.readUInt32LE(32 + index * 8);
  assert.ok(count > 0); start += count;
}
assert.ok(returnPc, 'canonical real Windows callback return gate');
function initializer() {
  ok('module_initializer'); const record = take(72);
  assert.equal(record.subarray(0, 4).toString(), 'R3MO');
  if (record.readUInt32LE(16) !== 1) return undefined;
  return {token: record.readBigUInt64LE(24), entry: record.readUInt32LE(40), returnPc: record.readUInt32LE(44), returnId: record.readUInt32LE(48), reason: record.readUInt32LE(56), kind: record.readUInt32LE(60)};
}
function beginReadyCallback() {
  ok('pending_windows_callback'); const request = take(96);
  if (!request.readUInt32LE(12)) return;
  assert.equal(request.readUInt32LE(12), 8, 'real generic COM provider');
  const target = code.entry(request.readUInt32LE(16));
  const count = request.readUInt32LE(20);
  assert.ok([0, 1, 3, 4].includes(count));
  ok('begin_windows_resident_callback', ...key, ...pair(target.id), returnPc, returnId);
  const record = take(72);
  const callback = {home: record.readBigUInt64LE(64), token: record.readUInt32LE(16), count, procedure: request.readUInt32LE(16)};
  ok('authorize_resident_callback', ...key, ...pair(callback.home), callback.token);
  callbacks.push(callback); callbackHistory.push({count, procedure: callback.procedure});
}
function progress(error) {
  writeFileSync(join(output, 'progress.json'), JSON.stringify({engineHash, ...code.stats(), calls, initializers, callbacks: callbacks.map(value => ({...value, home: value.home.toString()})), callbackHistory, recent, error: error?.stack}, null, 2));
}
try {
  for (let iteration = 0; iteration < 100000 && !exited; iteration++) {
    const init = initializer();
    if (!init) beginReadyCallback();
    const unit = code.entry(get(48)), callback = callbacks.at(-1);
    if (callback && !init) ok('select_resident_callback_unit', ...key, ...pair(callback.home), callback.token, ...pair(unit.id));
    const run = code.execute(unit); totalRetired += run.retired;
    recent.push(run); if (recent.length > 64) recent.shift();
    if ([1, 3].includes(run.reason)) continue;
    if (run.reason === 9) {assert.equal(run.detail, provenance.expectedExit); exited = true; break;}
    assert.equal(run.reason, 8, `authored COM guest has no expected CPU exceptions: ${JSON.stringify(run)}`);
    if (init && run.pc === init.returnPc && run.detail === init.returnId) {
      ok('finish_module_initializer', ...pair(init.token)); initializers.push({entry: init.entry, reason: init.reason, kind: init.kind}); continue;
    }
    if (callback && !init && run.pc === returnPc && run.detail === returnId) {
      ok('finish_windows_resident_callback', ...key, ...pair(callback.home), callback.token); callbacks.pop(); continue;
    }
    if (callback && !init) ok('capture_active_resident_callback_windows_call', ...key, ...pair(unit.id), callback.token);
    else ok('capture_resident_windows_call', ...key, ...pair(unit.id));
    const record = take(112), token = record.readUInt32LE(16);
    calls.push({id: run.detail, pc: run.pc, token});
    if (callback && !init) ok('complete_active_resident_callback_windows_call', ...key, ...pair(unit.id), callback.token, token);
    else ok('complete_resident_windows_call', ...key, ...pair(unit.id), token);
    if (get(72) === 9) {assert.equal(get(80), provenance.expectedExit); exited = true; break;}
    if (calls.length % 50 === 0) progress();
  }
  assert.ok(exited, 'bounded authored client reaches its own success ExitProcess');
  assert.equal(callbacks.length, 0); assert.ok(totalRetired > 0);
  assert.ok(initializers.filter(row => row.reason === 1 && row.kind === 2).length >= 4, 'real server process-attach code executed including failure');
  assert.ok(initializers.filter(row => row.reason === 0 && row.kind === 2).length >= 3, 'normal COM unload and final apartment teardown execute DLL detach');
  for (const count of [0, 1, 3, 4]) assert.ok(callbackHistory.some(row => row.count === count), `COM callback arity ${count} executed`);
  let found;
  for (let index = 0; index < 512; index++) {
    ok('file_overlay_entry', ...pair(fileEpoch), index); const header = take(96);
    if (!header.readBigUInt64LE(16)) break;
    const path = take(96 + header.readUInt32LE(56) * 2).subarray(96).toString('utf16le');
    if (path.toLowerCase() === provenance.expectedFile.toLowerCase()) {
      assert.equal(header.readUInt32LE(52), 1); assert.equal(header.readBigUInt64LE(40), BigInt(provenance.expectedBytes.length));
      ok('read_file_overlay', ...pair(fileEpoch), ...pair(header.readBigUInt64LE(32)), ...pair(header.readBigUInt64LE(24)), 0, 0, provenance.expectedBytes.length);
      assert.equal(take(32 + provenance.expectedBytes.length).subarray(32).toString(), provenance.expectedBytes); found = path;
    }
  }
  assert.ok(found, 'guest-written DLL/factory/object lifecycle log survives normal process exit');
  const paths = execFileSync('rg', ['--files', 'engine/src', 'engine/tests/windows/com'], {cwd: root, encoding: 'utf8'}).trim().split('\n').concat(['engine/tests/support/wasm/cache.mjs', provenance.inspectionSource]).sort();
  writeFileSync(join(output, 'source-sha256.json'), JSON.stringify(Object.fromEntries(paths.map(path => [path, hash(readFileSync(join(root, path)))])), null, 2));
  const receipt = {engineHash, artifacts: provenance.artifacts, ...code.stats(), totalRetired, calls, initializers, callbackHistory, finalBytes: provenance.expectedBytes, generatedDigest: code.finish(), claim: 'self-authored freestanding PE32 client and DLLs execute real registry-backed COM activation/factory/custom IID/aggregation failure/object references/normal unload/final apartment cleanup through generated Wasm; no CRT stub or proprietary binary input'};
  writeFileSync(join(output, 'receipt.json'), JSON.stringify(receipt, null, 2));
  console.log(JSON.stringify({engineHash, calls: calls.length, callbacks: callbackHistory.length, initializers: initializers.length, ...code.stats(), totalRetired, finalBytes: provenance.expectedBytes}));
} catch (error) {progress(error); throw error;}
