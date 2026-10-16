import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {execFileSync} from 'node:child_process';
import {mkdirSync, readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {ARENA_SIZE} from '../../../../../support/wasm/arena.mjs';
import {assertEngineExports} from '../../../../../support/wasm/exports.mjs';
import {image} from './images.mjs';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root);
mkdirSync(output, {recursive: true});
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const engineBytes = readFileSync(enginePath), engineHash = hash(engineBytes);
if (process.env.RING3_ENGINE_SHA256) assert.equal(engineHash, process.env.RING3_ENGINE_SHA256);
assert.ok(WebAssembly.validate(engineBytes));
const engineModule = new WebAssembly.Module(engineBytes);
assert.deepEqual(WebAssembly.Module.imports(engineModule), []);
assertEngineExports(engineModule, root);
const sourcePaths = execFileSync('rg', ['--files', 'engine/src'], {cwd: root, encoding: 'utf8'}).trim().split('\n').concat([
  'engine/Cargo.toml', 'engine/tests/loader/modules/wasm.rs', 'engine/tests/loader/modules/wasm/lifecycle.rs',
  'engine/tests/loader/modules/wasm/fixtures/runtime/run.mjs', 'engine/tests/loader/modules/wasm/fixtures/runtime/images.mjs',
  'engine/tests/support/wasm.rs', 'engine/tests/support/wasm/arena.mjs', 'engine/tests/support/wasm/exports.mjs',
]).sort();
writeFileSync(join(output, 'source-sha256.json'), JSON.stringify(Object.fromEntries(sourcePaths.map(path => [path, hash(readFileSync(join(root, path)))])), null, 2));
const instance = new WebAssembly.Instance(engineModule, {}), memory = instance.exports.memory;
const api = Object.fromEntries(Object.entries(instance.exports).filter(([name]) => name.startsWith('ring3_abi_v1_')).map(([name, fn]) => [name.slice(13), fn]));
const key = [0x51525354, 0x91929394], transfer = 140, stack = 0x7e000000, runtime = 0x7f000000;
assert.equal(api.open(128, ...key), 0);
const base = api.arena_ptr() >>> 0;
assert.equal(api.arena_len(), ARENA_SIZE);
const bytes = () => new Uint8Array(memory.buffer);
const view = () => new DataView(memory.buffer);
const arena = () => Buffer.from(bytes().slice(base, base + ARENA_SIZE));
const get = offset => view().getUint32(base + offset, true);
const request = value => bytes().set(value, base + transfer);
const name = value => {const data = Buffer.from(value); request(data); return data.length;};
const unchanged = (label, action, status) => {const before = arena(); assert.equal(action(), status, label); assert.deepEqual(arena(), before, `${label}: whole arena neutral`);};
const context = () => {const value = arena(); return Buffer.concat([value.subarray(0, 56), value.subarray(4236, 4588), value.subarray(4612)]);};
const records = [], runs = [], initializerLog = [], providerLog = [];
function record() {
  const data = arena().subarray(transfer, transfer + 72);
  assert.equal(data.subarray(0, 4).toString(), 'R3MO');
  assert.deepEqual([...data.subarray(4, 16)], [1, 0, 1, 0, 72, 0, 0, 0, 0, 0, 0, 0]);
  assert.deepEqual([...data.subarray(64)], Array(8).fill(0));
  const fields = Array.from({length: 12}, (_, index) => data.readUInt32LE(16 + index * 4));
  records.push(fields);
  return {outcome: fields[0], value: fields[1], token: [fields[2], fields[3]], module: [fields[4], fields[5]], entry: fields[6], returnPc: fields[7], returnId: fields[8], returnEsp: fields[9], reason: fields[10], kind: fields[11]};
}
const game = image({base: 0x400000, game: true, imports: [
  {module: 'dep.dll', symbols: ['Answer', 7], oftZero: true},
  {module: 'kernel32.dll', symbols: ['LoadLibraryA', 'GetProcAddress', 'FreeLibrary', 'ExitProcess']},
]});
const dep = image({base: 0x10000000, dll: true, tls: true, answer: 42, exports: [{ordinal: 7, name: 'Answer', target: 0x1000}, {ordinal: 8, name: 'Data', target: 0x2900}]});
const dynamic = image({base: 0x10000000, dll: true, tls: true, exports: [{ordinal: 1, name: 'Run', target: 0x1000}]});
const forwarder = image({base: 0x20000000, dll: true, exports: [{ordinal: 1, name: 'Run', target: 'dynamic.Run'}]});
const bad = image({base: 0x30000000, dll: true, tls: true, fail: true});
const sources = new Map([['game.exe', game], ['dep.dll', dep], ['dynamic.dll', dynamic], ['forwarder.dll', forwarder], ['bad.dll', bad]]);
assert.equal(api.configure_module_limits(64, 0x100000, 64, 65536, 0x100000), 0);
assert.equal(api.register_kernel32_module(0x70000000), 0);
assert.equal(api.begin_module_input(1, name('discard.dll')), 0); request(Uint8Array.of(0));
assert.equal(api.append_module_input(0, 1), 0);
unchanged('malformed module retains staged input', () => api.commit_module_input(), 19);
assert.equal(api.abort_module_input(), 0);
for (const [moduleName, source] of sources) {
  assert.equal(api.begin_module_input(source.bytes.length, name(moduleName)), 0);
  unchanged('incomplete source commit', () => api.commit_module_input(), 7);
  for (let at = 0; at < source.bytes.length; at += 4096) {
    const chunk = source.bytes.subarray(at, at + 4096); request(chunk);
    if (at === 0) unchanged('out-of-order source chunk', () => api.append_module_input(1, chunk.length), 7);
    assert.equal(api.append_module_input(at, chunk.length), 0);
  }
  assert.equal(api.commit_module_input(), 0);
  writeFileSync(join(output, moduleName), source.bytes);
}
unchanged('limits cannot mutate after registration', () => api.configure_module_limits(1, 4096, 1, 1, 1), 12);
const actual = new Map([['game.exe', 0x400000], ['dep.dll', 0x10000000], ['bad.dll', 0x30000000]]);
const read = address => {assert.equal(api.read32(address >>> 0), 0); assert.equal(get(116), 0, `mapped host read ${address.toString(16)}`); return get(120);};
function gates() {
  assert.equal(api.module_gates(), 0);
  const data = arena().subarray(transfer);
  assert.equal(data.subarray(0, 4).toString(), 'R3MG');
  const count = data.readUInt32LE(16);
  assert.equal(data.readUInt32LE(8), 24 + 8 * count);
  assert.equal(data.readUInt32LE(20), 0);
  return Array.from({length: count}, (_, index) => ({entry: data.readUInt32LE(24 + index * 8), id: data.readUInt32LE(28 + index * 8)}));
}
function execute(init) {
  const pc = get(48), available = gates();
  const wantedIds = init ? [init.returnId] : [0x1000a, 0x1000e, 0x1000f, 0x10003];
  const selected = available.filter(gate => wantedIds.includes(gate.id));
  let source, ends, functionRva, start;
  for (const [moduleName, address] of actual) {
    for (const [rva, code] of sources.get(moduleName).functions) {
      if (pc >= address + rva && pc < address + rva + code.length) {source = code; ends = sources.get(moduleName).boundaries.get(rva); functionRva = rva; start = address;}
    }
  }
  assert.ok(source, `guest function owns PC ${pc.toString(16)}`);
  const offset = pc - start - functionRva;
  const remaining = ends.find(end => end > offset) - offset;
  const specs = [{entry: pc, length: remaining}, ...selected.map(gate => ({entry: gate.entry, length: 2}))];
  const descriptors = Buffer.alloc(specs.length * 8 + selected.length * 8);
  specs.forEach((item, index) => {descriptors.writeUInt32LE(item.entry, index * 8); descriptors.writeUInt32LE(item.length, index * 8 + 4);});
  selected.forEach((item, index) => {descriptors.writeUInt32LE(item.entry, specs.length * 8 + index * 8); descriptors.writeUInt32LE(item.id, specs.length * 8 + index * 8 + 4);});
  request(descriptors);
  assert.equal(api.compile_with_gates(specs.length, selected.length), 0, `compile guest PC ${pc.toString(16)}`);
  const compiled = bytes().slice(api.module_ptr() >>> 0, (api.module_ptr() >>> 0) + api.module_len());
  assert.ok(WebAssembly.validate(compiled));
  const module = new WebAssembly.Module(compiled), imports = WebAssembly.Module.imports(module);
  assert.equal(imports.filter(item => item.name === 'guard_detect').length, 1);
  assert.equal(imports.filter(item => item.name === 'finish_exception_detection').length, 1);
  assert.ok(!imports.some(item => item.name === 'guard' || item.name === 'guard_resident'));
  const calls = [], bindings = {};
  for (const item of imports.filter(item => item.module === 'ring3')) {
    assert.equal(typeof api[item.name], 'function', `actual helper ${item.name}`);
    bindings[item.name] = (...args) => {
      const result = api[item.name](...args);
      calls.push({name: item.name, args: args.map(value => typeof value === 'bigint' ? value.toString() : value), result: typeof result === 'bigint' ? result.toString() : result});
      if (item.name === 'guard_detect') {
        if (result <= 0n) {writeFileSync(join(output, 'failed-guard.json'), JSON.stringify({pc, args, result: result.toString(), arena: arena().toString('hex')}, null, 2));}
        assert.ok(typeof result === 'bigint' && result > 0n, `real positive native epoch, observed ${result}`);
      }
      return result;
    };
  }
  const generated = new WebAssembly.Instance(module, {env: {memory}, ring3: bindings});
  const result = generated.exports.run(base, base + 56, 1000, base + 96);
  const reason = get(72), retired = get(76), detail = get(80);
  assert.ok(retired > 0, 'genuine guest instructions retire');
  assert.equal(calls.filter(item => item.name === 'guard_detect').length, 1);
  assert.equal(calls.filter(item => item.name === 'finish_exception_detection').length, Number(reason === 3));
  assert.ok([3, 8].includes(reason), `valid guest flow reason ${reason}, transport ${result}`);
  runs.push({pc, reason, retired, detail, result, calls});
  writeFileSync(join(output, `compiled-${runs.length}.wasm`), compiled);
  return {reason, retired, detail};
}
function finishInitializers(first) {
  let operation = first;
  while (operation.outcome === 1) {
    const init = operation;
    assert.equal(init.token[1], 0); assert.ok(init.token[0] > 0);
    assert.equal(get(48), init.entry);
    unchanged('initializer cannot finish before guest RET', () => api.finish_module_initializer(...init.token), 13);
    const step = execute(init);
    assert.equal(step.reason, 8); assert.equal(step.detail, init.returnId);
    assert.equal(step.retired, init.kind === 1 ? 7 : 4);
    assert.equal(get(48), init.returnPc); assert.equal(get(32), init.returnEsp);
    unchanged('wrong initializer token', () => api.finish_module_initializer(init.token[0] + 1, init.token[1]), 14);
    initializerLog.push({entry: init.entry, kind: init.kind, reason: init.reason});
    assert.equal(api.finish_module_initializer(...init.token), 0);
    operation = record();
  }
  return operation;
}
assert.equal(api.start_module_process(name('game.exe'), 0, stack, 4, runtime), 0);
const start = finishInitializers(record());
assert.deepEqual([start.outcome, start.value], [2, 0x400000]);
assert.equal(get(48), 0x401000); assert.equal(get(32), stack + 0x4000);
assert.equal(read(0x402300), 0x10001000); assert.equal(read(0x402304), 0x10001000, 'ordinal import bound');
assert.equal(api.module_teb(), 0); assert.equal(record().value, runtime);
assert.equal(read(runtime + 0x2c), runtime + 0x1000);
const depSlot = read(0x10002850), depTls = read(runtime + 0x1000 + depSlot * 4);
assert.equal(depSlot, 0); assert.equal(depTls % 8, 0);
assert.equal(read(depTls), 0x0a090808); assert.equal(read(depTls + 4), 0); assert.equal(read(depTls + 8), 0);
assert.equal(read(0x10002908), 1); assert.equal(read(0x10002904), 1);
const beforeExport = context();
assert.equal(api.resolve_module_export(0x10000000, 0, name('Data')), 0);
assert.deepEqual([record().outcome, get(transfer + 20)], [5, 0x10002900]);
assert.equal(api.resolve_module_export(0x10000000, 1, 8), 0); assert.equal(record().value, 0x10002900);
assert.deepEqual(context(), beforeExport, 'data export resolution preserves full CPU context');
name('data'); unchanged('export names remain case sensitive', () => api.resolve_module_export(0x10000000, 0, 4), 26);
assert.equal(api.module_handle(name('dynamic.dll')), 0); assert.equal(record().value, 0, 'delay module not eagerly loaded');
assert.equal(read(0x402b80), 0x2bc0, 'delay IAT retained for guest helper');
const beforeBad = context();
assert.equal(api.load_library_module(name('bad.dll'), 0), 0);
const failed = finishInitializers(record());
assert.equal(failed.outcome, 4); assert.deepEqual(context(), beforeBad, 'failed dynamic init restores full CPU context');
assert.equal(api.module_handle(name('bad.dll')), 0); assert.equal(record().value, 0);
const beforeMissing = arena(), missingHelper = Buffer.alloc(40);
missingHelper.write('R3MH'); missingHelper.writeUInt16LE(1, 4); missingHelper.writeUInt16LE(1, 6); missingHelper.writeUInt32LE(40, 8);
[1, 0, 1, 0x30002904, 1, 4].forEach((value, index) => missingHelper.writeUInt32LE(value, 16 + index * 4));
beforeMissing.set(missingHelper, 100);
assert.equal(api.read32(0x30002904), 0); assert.deepEqual(arena(), beforeMissing, 'failed module removed with exact host-read fault and no other arena effect');
// the process is now driven only by guest code and normal provider completion.
let forwardedTarget;
for (let limit = 0; limit < 30; limit++) {
  const step = execute();
  if (step.reason === 3) continue;
  assert.ok([0x1000a, 0x1000e, 0x1000f, 0x10003].includes(step.detail));
  assert.equal(api.capture_call(...key, api.generation(), 2, step.detail === 0x1000e ? 2 : 1), 0);
  const token = get(transfer + 16);
  providerLog.push(step.detail);
  assert.equal(api.complete_windows_call(...key, api.generation(), token), 0);
  assert.equal(api.module_initializer(), 0);
  let pending = record();
  if (pending.outcome === 1) {
    if (step.detail === 0x1000a) {
      actual.set('forwarder.dll', 0x20000000);
      assert.equal(pending.entry, 0x20001020, 'LoadLibrary initializes only the requested forwarder');
      assert.equal(api.module_handle(name('dynamic.dll')), 0); assert.equal(record().value, 0, 'forwarded target remains unloaded before GetProcAddress');
    }
    if (step.detail === 0x1000e) {
      assert.equal(api.module_handle(name('dynamic.dll')), 0);
      const dynamicBase = record().value;
      assert.equal(dynamicBase & 0xffff, 0, 'canonical relocated module uses allocation granularity');
      assert.ok(dynamicBase >= 0x14010000, 'dynamic module avoids the dependency and reserved process heap');
      actual.set('dynamic.dll', dynamicBase);
      forwardedTarget = dynamicBase + 0x1000;
      assert.equal(pending.entry, dynamicBase + 0x1060, 'GetProcAddress demand-loads and initializes the canonical forwarded target');
    }
    finishInitializers(pending);
  }
  if (step.detail === 0x10003) break;
  if (step.detail === 0x1000f) {
    for (const moduleName of ['forwarder.dll', 'dynamic.dll']) {
      assert.equal(api.module_handle(name(moduleName)), 0); assert.equal(record().value, 0, `${moduleName}: native membership removed`);
      actual.delete(moduleName);
    }
    assert.equal(read(runtime + 0x1004), 0, 'unload clears the dynamic TLS slot');
  }
  if (limit === 29) assert.fail('guest program must terminate');
}
assert.deepEqual(providerLog, [0x1000a, 0x1000e, 0x1000f, 0x10003]);
assert.equal(read(0x402900), 42); assert.equal(read(0x40290c), 42); assert.equal(read(0x402910), 77); assert.equal(read(0x402914), 1);
assert.equal(read(0x402b80), forwardedTarget, 'guest delay helper publishes the canonical resolved target');
assert.equal(read(0x10002908), 0); assert.equal(read(0x10002904), 0, 'process exit executed dependency detach');
assert.equal(get(72), 9); assert.equal(get(80), 0);
assert.equal(initializerLog.filter(item => item.kind === 1 && item.reason === 1).length, 3);
assert.equal(initializerLog.filter(item => item.kind === 1 && item.reason === 0).length, 3);
writeFileSync(join(output, 'receipt.json'), JSON.stringify({engineHash, sourceHashes: Object.fromEntries([...sources].map(([name, value]) => [name, hash(value.bytes)])), records, runs, initializerLog, providerLog}, null, 2));
console.log(JSON.stringify({engineHash, sources: sources.size, guestRuns: runs.length, initializers: initializerLog.length, providers: providerLog.length}));
