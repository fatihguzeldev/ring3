import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {readEngine} from '../../../support/wasm/engine.mjs';

const [enginePath, reportPath] = process.argv.slice(2);
assert(enginePath && reportPath, 'usage: native.mjs engine.wasm report.json');
const engine = readEngine(enginePath);
const hash = value => createHash('sha256').update(value).digest('hex');
const report = {schema: 1, complete: false, engineSha256: engine.sha256,
  sourceSha256: hash(readFileSync(new URL(import.meta.url))), checks: [], modules: []};
const checkpoint = () => writeFileSync(reportPath, JSON.stringify(report, null, 2));
let stage = 'instantiate', api, memory, arena = 0, context = 0, active = false, opened = false;
let expectedBacking, backing = 0, backingLength = 0;

const bytes = (pointer, length) => new Uint8Array(memory.buffer).slice(pointer, pointer + length);
const view = () => new DataView(memory.buffer);
const word = (pointer, offset = 0) => view().getUint32(pointer + offset, true);
const put = (pointer, offset, value) => view().setUint32(pointer + offset, value >>> 0, true);
const raw = () => ({context: Buffer.from(bytes(context, 128)).toString('hex'),
  arenaSha256: hash(bytes(arena, api.arena_len())), backingSha256: hash(bytes(backing, backingLength))});

function status(name, wanted, ...args) {
  const actual = api[name](...args) >>> 0;
  report.checks.push({stage, name, args, actual, wanted}); checkpoint();
  assert.equal(actual, wanted, `${stage}: ${name}`);
}

function neutral(label, action) {
  stage = label;
  const before = {context: bytes(context, 128), arena: bytes(arena, api.arena_len()),
    backing: bytes(backing, backingLength)};
  action();
  assert.deepEqual(bytes(context, 128), before.context, `${label}: context neutral`);
  assert.deepEqual(bytes(arena, api.arena_len()), before.arena, `${label}: arena neutral`);
  assert.deepEqual(bytes(backing, backingLength), before.backing, `${label}: RAM neutral`);
  report.checks.push({stage, neutral: true, ...raw()}); checkpoint();
}

function physical(address) {
  const slot = word(word(context, 56) + (address >>> 12) * 4);
  assert(slot > 0 && slot <= word(context, 68), 'mapped authored address');
  return backing + (slot - 1) * 4096 + (address & 4095);
}

function expectedWrite(address, value) {
  new DataView(expectedBacking.buffer).setUint32(physical(address) - backing, value >>> 0, true);
}

function prepare(pc = 0x1000) {
  status('core_prepare', 0, pc, 256, 16);
  const pointer = api.core_code_ptr() >>> 0, length = api.core_code_len() >>> 0;
  assert(pointer > 0 && length >= 8 && length <= 1 << 20, 'bounded generated code');
  const code = bytes(pointer, length);
  assert(WebAssembly.validate(code), 'generated module validates');
  const module = new WebAssembly.Module(code);
  assert.deepEqual(WebAssembly.Module.imports(module), [
    {module: 'ring3', name: 'memory', kind: 'memory'},
    {module: 'ring3', name: 'read', kind: 'function'},
    {module: 'ring3', name: 'write', kind: 'function'},
  ]);
  const run = new WebAssembly.Instance(module, {ring3: {
    memory, read: api.core_read, write: api.core_write,
  }}).exports.run;
  assert.equal(run.length, 2);
  const selectedContext = api.core_context_ptr() >>> 0;
  report.modules.push({pc, pointer, length, identity: String(view().getBigUint64(selectedContext + 120, true)),
    sha256: hash(code), wasmHex: Buffer.from(code).toString('hex')});
  checkpoint();
  return run;
}

function expectState(regs, pc, flags, retired, remaining) {
  assert.deepEqual(Array.from({length: 8}, (_, index) => word(context, index * 4)), regs);
  assert.equal(word(context, 32), pc); assert.equal(word(context, 36), flags);
  assert.equal(word(context, 40), retired); assert.equal(word(context, 44), remaining);
  assert.equal(word(context, 48), 0); assert.equal(word(context, 52), 0);
  assert.deepEqual(bytes(backing, backingLength), expectedBacking, `${stage}: whole canonical RAM`);
  report.checks.push({stage, ...raw()}); checkpoint();
}

try {
  const instance = new WebAssembly.Instance(engine.module, {ring3_observability: {now_ns: () => 0n}});
  memory = instance.exports.memory;
  api = Object.fromEntries(Object.entries(instance.exports)
    .filter(([name]) => name.startsWith('ring3_abi_v1_'))
    .map(([name, value]) => [name.slice(13), value]));
  for (const name of ['open', 'close', 'arena_ptr', 'arena_len', 'map', 'upload', 'write8', 'cpu_read32', 'cpu_store32',
    'core_prepare', 'core_code_ptr', 'core_code_len', 'core_context_ptr', 'core_begin', 'core_end',
    'core_read', 'core_write']) assert.equal(typeof api[name], 'function', `native export ${name}`);
  stage = 'open'; status('open', 0, 16, 97, 0); opened = true;
  arena = api.arena_ptr() >>> 0; assert(arena > 0);
  for (const [address, permission] of [[0x1000, 7], [0x3000, 3], [0x4000, 3]]) {
    status('map', 0, address, 1, permission);
  }
  const code = new Uint8Array(4096), data = new Uint8Array(4096), stack = new Uint8Array(4096);
  code.set([0x8b, 0x03, 0x83, 0xc0, 1, 0x89, 0x43, 4,
    0xe8, 0x33, 0, 0, 0, 0x83, 0xc0, 2, 0x0f, 0x0b]);
  code.set([0x83, 0xc0, 3, 0xc3], 0x40);
  new DataView(data.buffer).setUint32(0, 10, true);
  for (const [address, page] of [[0x1000, code], [0x3000, data], [0x4000, stack]]) {
    new Uint8Array(memory.buffer, arena + 140, page.length).set(page);
    status('upload', 0, address, page.length);
  }
  new Uint8Array(memory.buffer, arena, 56).fill(0);
  new Uint8Array(memory.buffer, arena, 4).set([0x52, 0x33, 0x53, 0x54]);
  view().setUint16(arena + 4, 3, true); view().setUint16(arena + 6, 1, true);
  put(arena, 8, 56);
  const initial = [5, 3, 7, 0x3000, 0x4800, 0, 0, 0];
  initial.forEach((value, index) => put(arena, 16 + index * 4, value));
  put(arena, 48, 0x1000); put(arena, 52, 2);
  stage = 'prepare'; const firstRun = prepare();
  context = api.core_context_ptr() >>> 0;
  assert(context > 0 && context % 8 === 0);
  assert.equal(api.core_context_ptr() >>> 0, context);
  assert.deepEqual(Array.from({length: 8}, (_, index) => word(context, index * 4)), initial);
  stage = 'begin'; status('core_begin', 0); active = true;
  assert.equal(word(context, 112), 1);
  assert(view().getBigUint64(context + 120, true) > 0n);
  assert.equal(api.core_context_ptr(), 0, 'active context is not reissued');
  backing = word(context, 64); backingLength = word(context, 68) * 4096;
  assert.equal(backingLength, 16 * 4096);
  expectedBacking = new Uint8Array(backingLength);
  for (const [address, page] of [[0x1000, code], [0x3000, data], [0x4000, stack]]) {
    expectedBacking.set(page, physical(address) - backing);
  }
  assert.deepEqual(bytes(backing, backingLength), expectedBacking, 'initial canonical RAM');
  neutral('active lifecycle rejection', () => {
    status('core_begin', 12); status('core_prepare', 12, 0x1000, 256, 16);
    status('map', 12, 0x5000, 1, 3); status('close', 12);
  });
  neutral('legacy CPU transport rejection', () => {
    status('cpu_read32', 12, 0x3000); status('cpu_store32', 12, 0x3000, 99);
  });
  neutral('wrong slow-helper context', () => {
    assert.equal(api.core_read(context + 8, 0x3000, 4), 3n << 32n);
    status('core_write', 3, context + 8, 0x3000, 4, 99);
  });
  stage = 'actual native helpers';
  assert.equal(api.core_read(context, 0x3000, 4), 10n);
  status('core_write', 0, context, 0x3008, 4, 0x11223344);
  expectedWrite(0x3008, 0x11223344);
  assert.equal(api.core_read(context, 0x3008, 4), 0x11223344n);
  stage = 'budget after CALL';
  assert.equal(firstRun(context, 4), 2);
  expectedWrite(0x3004, 11); expectedWrite(0x47fc, 0x100d);
  expectState([11, 3, 7, 0x3000, 0x47fc, 0, 0, 0], 0x1040, 2, 4, 0);
  status('core_end', 0); active = false;
  assert.equal(word(context, 112), 0);
  neutral('inactive generated code', () => assert.equal(firstRun(context, 8), 8));
  neutral('second end', () => status('core_end', 7));
  const retained = bytes(context, 128);
  stage = 'repeat prepare'; prepare();
  assert.equal(api.core_context_ptr() >>> 0, context, 'stable context allocation');
  assert.deepEqual(bytes(context, 128), retained, 'prepare retains paused state');
  assert.equal(report.modules.at(-1).identity,report.modules[0].identity,'current caller identity reused');
  stage = 'prepare callee'; const secondRun = prepare(0x1040);
  assert.deepEqual(bytes(context, 120), retained.slice(0,120), 'callee selection retains paused architectural state');
  assert.notEqual(report.modules.at(-1).identity,report.modules[0].identity,'callee has its own source identity');
  status('core_begin', 0); active = true;
  neutral('old generated identity', () => assert.equal(firstRun(context, 8), 8));
  stage = 'resume callee and return'; assert.equal(secondRun(context, 8), 1);
  expectState([14, 3, 7, 0x3000, 0x4800, 0, 0, 0], 0x100d, 2, 6, 6);
  status('core_end', 0); active = false;
  const afterCallee = bytes(context, 128);
  stage = 'prepare caller continuation'; const thirdRun = prepare(0x100d);
  assert.deepEqual(bytes(context, 120),afterCallee.slice(0,120),'continuation selection retains paused architectural state');
  assert.equal(report.modules.at(-1).identity,report.modules[0].identity,'retained caller continuation reuses identity');
  status('core_begin', 0); active = true;
  neutral('unselected callee identity', () => assert.equal(secondRun(context, 6), 8));
  stage = 'resume caller continuation'; assert.equal(thirdRun(context, 6), 4);
  expectState([16, 3, 7, 0x3000, 0x4800, 0, 0, 0], 0x1010, 0x12, 7, 5);
  status('core_end', 0); active = false;
  stage = 'change compiled source'; status('write8', 0, 0x100f, 3);
  expectedBacking[physical(0x100f) - backing] = 3;
  neutral('stale source begin', () => status('core_begin', 4));
  assert.equal(word(context, 112), 0);
  assert.equal(api.core_context_ptr() >>> 0, context);
  report.final = {...raw(), backingHex: Buffer.from(bytes(backing, backingLength)).toString('hex')};
  stage = 'close'; status('close', 0); opened = false;
  report.complete = true; checkpoint();
  console.log(JSON.stringify({complete: true, checks: report.checks.length, engineSha256: engine.sha256,
    reportSha256: hash(readFileSync(reportPath))}));
} catch (error) {
  report.failure = {stage, message: String(error), stack: error.stack}; checkpoint();
  throw error;
} finally {
  if (active) api.core_end();
  if (opened) api.close();
}
