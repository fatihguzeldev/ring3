import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {readEngine} from '../../../support/wasm/engine.mjs';
import {Engine, KEY, view} from '../../../../../sdk/runtime/engine.js';
import {prepareV86} from '../../../../../sdk/runtime/v86/index.js';
import {DIRECTORY} from '../../../../../sdk/runtime/v86/layout.js';
import {image as moduleImage} from '../../../loader/modules/wasm/fixtures/runtime/images.mjs';

const [enginePath, reportPath, permissionText = '1'] = process.argv.slice(2);
assert(enginePath && reportPath, 'usage: v86.mjs imported-engine.wasm fresh-report.json');
const deniedPermission = Number(permissionText);
assert([0,1].includes(deniedPermission));
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const artifact = readEngine(enginePath);
const image = readFileSync(new URL('../../../process/images/startup/imports/fixtures/p2-c-imports/program.exe', import.meta.url));
assert.equal(hash(image), '7c0451977a0818d5f4905a2b00f3a3bae63b9449ef22775ea003ebb0ca69c5d5');
const cpuBytes = readFileSync(new URL('../../../../../sdk/runtime/v86/vendor/v86.wasm', import.meta.url));
const report = {complete: false, engineSha256: artifact.sha256, cpuSha256: hash(cpuBytes),
  scope: 'actual unmodified v86 CPU, shared RAM, authored PE with four Ring3 Windows calls; no game/FPS claim',
  calls: [], runs: [], ownCpuCalls: 0};
writeFileSync(reportPath, JSON.stringify(report), {flag: 'wx'});
const save = () => writeFileSync(reportPath, JSON.stringify(report, (_key, value) =>
  typeof value === 'bigint' ? String(value) : value, 2) + '\n');
let runtime, engine, cache, stage = 'prepare';
try {
  runtime = await prepareV86({wasmUrl: 'data:application/wasm;base64,' + cpuBytes.toString('base64'), sha256: hash(cpuBytes)});
  const memory = runtime.memory;
  const prefix = Buffer.from(new Uint8Array(memory.buffer, 0, 4096));
  const needed = 805306368 / 65536 - memory.buffer.byteLength / 65536;
  if (needed > 0) memory.grow(needed);
  const instance = await WebAssembly.instantiate(artifact.module,
    {env: {memory}, ring3_observability: {now_ns: () => 0n}});
  assert.equal(instance.exports.memory, memory);
  report.initialStackPointer = instance.exports.__stack_pointer.value >>> 0;
  instance.exports.__stack_pointer.value = 536870912;
  for (const name of ['__stack_pointer', '__heap_base', '__data_end']) {
    const pointer = instance.exports[name].value >>> 0;
    assert(pointer >= 536870912 && pointer < memory.buffer.byteLength, name + ' is in the Ring3 region');
    (report.engineLayout ??= {})[name] = pointer;
  }
  const api = Object.fromEntries(Object.entries(instance.exports)
    .filter(([name]) => name.startsWith('ring3_abi_v1_')).map(([name, fn]) => [name.slice(13), fn]));
  for (const name of ['core_prepare', 'core_cold_run', 'core_begin', 'core_numeric', 'core_precise'])
    api[name] = () => {report.ownCpuCalls++; throw new Error('unexpected own CPU fallback: ' + name);};
  engine = new Engine(api, memory, artifact.sha256);
  engine.ok('open', 256, ...KEY);
  engine.base = api.arena_ptr() >>> 0;
  assert.deepEqual(Buffer.from(new Uint8Array(memory.buffer, 0, 4096)), prefix, 'Ring3 initialization preserves v86 low state');
  engine.ok('configure_cpu_clock', 1_000_000_000, 0, 0, 0);
  engine.ok('configure_module_limits', 8, 0x100000, 16, 65536, 0x100000);
  engine.ok('register_kernel32_module', 0x70000000);
  const dll = moduleImage({base: 0x10000000, dll: true});
  engine.ok('begin_module_input', dll.bytes.length, engine.name('entry.dll'));
  for (let offset = 0; offset < dll.bytes.length; offset += 4096) {
    const chunk = dll.bytes.subarray(offset, offset + 4096);
    engine.request(chunk); engine.ok('append_module_input', offset, chunk.length);
  }
  engine.ok('commit_module_input');
  const name = 'core-imports.exe';
  engine.ok('begin_module_input', image.length, engine.name(name));
  for (let offset = 0; offset < image.length; offset += 4096) {
    const chunk = image.subarray(offset, offset + 4096);
    engine.request(chunk); engine.ok('append_module_input', offset, chunk.length);
  }
  engine.ok('commit_module_input');
  engine.ok('start_module_process', engine.name(name), 0, 0x7e000000, 4, 0x7f000000);
  const banks = () => Buffer.concat([[4236,128], [4364,160], [4524,64], [4612,144], [4756,40]]
    .map(([offset,length]) => Buffer.from(new Uint8Array(memory.buffer, engine.base + offset, length))));
  const initialBanks = banks();
  engine.ok('load_library_module', engine.name('entry.dll'), 0);
  const dllEntry = engine.get(48), dllBase = dllEntry - 0x1020;
  engine.ok('module_initializer');
  const initializer = view(engine.take(72));
  assert.equal(initializer.getUint32(16, true), 1, 'a real guest DLL initializer is pending at attach');
  const init = {token: initializer.getBigUint64(24, true), returnPc: initializer.getUint32(44, true), id: initializer.getUint32(48, true)};
  stage = 'attach while DLL initializer is pending';
  cache = await runtime.attach(engine, {budget: 1000});
  let initRun;
  for (let count = 0; count < 100; count++) {
    initRun = cache.execute(cache.select());
    if (initRun.reason === 8) break;
    assert([1,2,6].includes(initRun.coreReason));
  }
  assert.equal(initRun.pc, init.returnPc); assert.equal(initRun.detail, init.id);
  engine.ok('finish_module_initializer', Number(init.token & 0xffffffffn), Number(init.token >> 32n));
  assert.equal(engine.get(48), 0x401000, 'DLL initialization restores the original PE entry');
  engine.ok('read32', dllBase + 0x2904); assert.equal(engine.get(120), 1, 'DllMain observed DLL_PROCESS_ATTACH');
  report.initializer = {entry: dllEntry, returnPc: init.returnPc, id: init.id};
  cache.import();
  const expected = [
    {id: 0x10002, args: [0xf1234567], returnPc: 0x40100f, result: 0, error: 0xf1234567},
    {id: 0x10001, args: [], returnPc: 0x401017, result: 0xf1234567, error: 0xf1234567},
    {id: 0x10002, args: [0], returnPc: 0x401020, result: 0xf1234567, error: 0},
    {id: 0x10001, args: [], returnPc: 0x401022, result: 0, error: 0},
  ];
  for (const [index, wanted] of expected.entries()) {
    stage = 'Windows call ' + (index + 1);
    let run;
    for (let count = 0; count < 100; count++) {
      run = cache.execute(cache.select()); report.runs.push(run); save();
      if (run.reason === 8) break;
      assert([1,2,6].includes(run.coreReason), 'only bounded CPU progress before a gate');
      await new Promise(resolve => setTimeout(resolve, 0));
    }
    assert.equal(run.reason, 8); assert.equal(run.detail, wanted.id);
    engine.ok('core_capture_windows_call');
    const packet = view(engine.take(112));
    const token = packet.getUint32(16, true);
    const args = Array.from({length: packet.getUint32(28, true)}, (_, i) => packet.getUint32(48 + i * 4, true));
    assert.deepEqual(args, wanted.args);
    engine.ok('core_complete_windows_call', token);
    assert.equal(engine.get(48), wanted.returnPc);
    assert.equal(engine.get(16), wanted.result);
    assert.deepEqual(banks(), initialBanks, 'complete extended banks survive native call and trap');
    engine.ok('read32', 0x7f000034); assert.equal(engine.get(116), 0); assert.equal(engine.get(120), wanted.error);
    report.calls.push({id: wanted.id, args, returnPc: engine.get(48), eax: engine.get(16)});
    cache.import();
  }
  stage = 'final guest writes';
  cache.execute(cache.select()); cache.publish();
  const actual = [];
  for (let i = 0; i < 3; i++) {
    engine.ok('read32', 0x403000 + i * 4); assert.equal(engine.get(116), 0); actual.push(engine.get(120));
  }
  assert.deepEqual(actual, [0xf1234567, 0, 0x52494e47]);
  assert.equal(report.ownCpuCalls, 0);
  assert.deepEqual(banks(), initialBanks);
  report.output = actual;
  stage = 'live v86 JIT';
  for (let count = 0; count < 8 && cache.stats().installedUnits === 0; count++) {
    cache.execute(cache.select());
    await new Promise(resolve => setTimeout(resolve, 5));
  }
  assert(cache.stats().installedUnits > 0, 'v86 generated and finalized actual hot guest code');
  report.liveJit = cache.stats().v86;
  stage = 'x87 and SSE execution';
  const u32 = value => [...new Uint8Array(Uint32Array.of(value).buffer)];
  const gate = report.runs.find(run => run.detail === 0x10001).pc;
  const upload = (address, bytes) => {engine.request(Uint8Array.from(bytes)); engine.ok('upload', address, bytes.length);};
  const put = (address, value) => engine.ok('write32', address, value);
  const read = address => {engine.ok('read32', address); assert.equal(engine.get(116), 0); return engine.get(120);};
  engine.ok('map', 0x600000, 2, 7);
  upload(0x600000, [0xd9,0xe8,0xd9,0xe8,0xde,0xc1,0xdd,0x1d,...u32(0x601000),
    0x66,0x0f,0x6f,0x05,...u32(0x601010),0x66,0x0f,0xfe,0x05,...u32(0x601020),
    0xf3,0x0f,0x7f,0x05,...u32(0x601030),0xb8,...u32(gate),0xff,0xe0]);
  for (let i = 0; i < 4; i++) {put(0x601010 + i * 4, i + 1); put(0x601020 + i * 4, i + 5);}
  new DataView(engine.memory.buffer).setUint32(engine.base + 48, 0x600000, true);
  cache.import();
  let arithmetic;
  for (let count = 0; count < 100; count++) {
    arithmetic = cache.execute(cache.select());
    if (arithmetic.reason === 8) break;
    assert([1,2,6].includes(arithmetic.coreReason), 'numeric program reached no fault');
  }
  assert.equal(arithmetic.reason, 8); assert.equal(arithmetic.detail, 0x10001);
  assert.equal(read(0x601000), 0); assert.equal(read(0x601004), 0x40000000, 'strict x87 1+1 stores binary64 2');
  const vector = Array.from({length: 4}, (_, i) => read(0x601030 + i * 4));
  assert.deepEqual(vector, [6,8,10,12], 'actual SSE integer vector addition');
  report.numeric = {binary64Words: [0,0x40000000], vector};
  stage = 'memory policies on actual v86 page walks';
  const dataAddress=0x680000;
  engine.ok('map',dataAddress,1,3);
  upload(0x600300,[0xa1,...u32(dataAddress),0xb8,...u32(gate),0xff,0xe0]);
  upload(0x600340,[0xc7,0x05,...u32(dataAddress),...u32(0x76543210),0xb8,...u32(gate),0xff,0xe0]);
  report.memoryPolicies=[];
  for(const [selectiveCode,incrementalSync] of [[false,false],[true,false],[false,true],[true,true]]) {
    cache.tables.configure({selectiveCode,incrementalSync});
    const observations=[],original=cache.tables.publishDirty;
    cache.tables.publishDirty=function() {
      const d=new DataView(memory.buffer),directory=this.ram+DIRECTORY+(dataAddress>>>22)*4;
      const observed={accessed:!!(d.getUint32(directory,true)&32),
        dirty:!!(d.getUint32(this.pte(dataAddress>>>12),true)&64)};
      const result=original.call(this);
      observations.push({...observed,accessedAfter:!!(d.getUint32(directory,true)&32),
        scannedPtes:this.lastDirtyScanCount});return result;
    };
    try {
      put(dataAddress,0x12345678);
      for(const pc of [0x600300,0x600340]) {
        new DataView(memory.buffer).setUint32(engine.base+48,pc,true);cache.import();
        const run=cache.execute(cache.select());assert.equal(run.reason,8);
      }
      assert.equal(observations[0].accessed,true,'actual read marks the page directory accessed');
      assert.equal(observations[0].dirty,false,'read-only batch does not dirty the data PTE');
      assert.equal(observations[1].dirty,true,'later actual store marks the data PTE dirty');
      if(incrementalSync)assert.equal(observations[0].accessedAfter,false,'partitioned read-only collection resets access');
      assert.equal(read(dataAddress),0x76543210);
      report.memoryPolicies.push({selectiveCode,incrementalSync,observations});
    } finally {cache.tables.publishDirty=original;}
  }
  stage='host code write invalidates actual finalized v86 code';
  const hotPc=0x600400,hot=value=>[0xc7,0x05,...u32(dataAddress),...u32(value),0xe9,...u32(-15)];
  upload(hotPc,hot(1));
  const generatedBefore=cache.stats().v86.finalizedModules;
  for(let count=0;count<24;count++) {
    new DataView(memory.buffer).setUint32(engine.base+48,hotPc,true);
    cache.import();cache.execute(cache.select());await new Promise(resolve=>setTimeout(resolve,2));
  }
  assert(cache.stats().v86.finalizedModules>generatedBefore,'the authored hot loop finalized actual Wasm');
  assert.equal(read(dataAddress),1);
  let clears=0;const clear=cache.cpu.jit_clear_cache;
  cache.cpu.jit_clear_cache=function(...args){clears++;return clear.apply(this,args);};
  try {
    engine.ok('map',0x690000,1,3);cache.import();
    assert.equal(clears,0,'unrelated real data mapping retains hot code');
    upload(hotPc,hot(2));cache.import();assert.equal(clears,1,'host executable write invalidates hot code');
    new DataView(memory.buffer).setUint32(engine.base+48,hotPc,true);cache.import();cache.execute(cache.select());
    assert.equal(read(dataAddress),2);
    engine.ok('unmap',0x600000,1);engine.ok('map',0x600000,1,7);upload(hotPc,hot(3));
    new DataView(memory.buffer).setUint32(engine.base+48,hotPc,true);cache.import();cache.execute(cache.select());
    assert.equal(read(dataAddress),3,'same-VA remap executes new canonical bytes');
    report.hostCodeInvalidation={clears,finalizedModules:cache.stats().v86.finalizedModules};
  } finally {cache.cpu.jit_clear_cache=clear;}
  stage = deniedPermission ? 'read-only store fault' : 'mapped NOACCESS store fault';
  put(0x601080, 0x11223344);
  upload(0x600100, [0xc7,0x05,...u32(0x601080),...u32(0x55667788),0xb8,...u32(gate),0xff,0xe0]);
  engine.ok('component_cpu_view', ...KEY);
  const descriptor = engine.resultView(128), raw = new DataView(engine.memory.buffer);
  const slot = raw.getUint32(descriptor.getUint32(24, true) + (0x601080 >>> 12) * 4, true);
  const physical = descriptor.getUint32(16, true) + (slot - 1) * 4096 + 0x80;
  engine.ok('protect', 0x601000, 1, deniedPermission);
  new DataView(engine.memory.buffer).setUint32(engine.base + 48, 0x600100, true);
  cache.import();
  const fault = cache.execute(cache.select());
  assert.equal(fault.coreReason, 3); assert.equal(fault.pc, 0x600100);
  assert.equal(fault.reason, 5); assert.equal(fault.detail, 2, 'mapped denied page is a permission fault');
  assert.equal(new DataView(engine.memory.buffer).getUint32(physical, true), 0x11223344, 'faulting write preserves canonical RAM');
  report.deniedPageFault = {permission: deniedPermission, ...fault};
  report.stats = cache.stats(); report.complete = true;
} catch (error) {
  report.failure = {stage, message: error.message, stack: error.stack}; throw error;
} finally {
  try {cache?.close(); if (engine) engine.ok('close'); runtime?.dispose();}
  catch (error) {report.complete = false; report.cleanupFailure = error.message; throw error;}
  finally {save();}
}
console.log(JSON.stringify({complete: report.complete, calls: report.calls.length, ownCpuCalls: report.ownCpuCalls, stats: report.stats}));
