import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {readEngine} from '../../../support/wasm/engine.mjs';
import {Engine, KEY, view} from '../../../../../sdk/runtime/engine.js';
import {prepareV86} from '../../../../../sdk/runtime/v86/index.js';
import {DIRECTORY} from '../../../../../sdk/runtime/v86/layout.js';
import {cpuViews, exportState} from '../../../../../sdk/runtime/v86/state.js';
import {image as moduleImage} from '../../../loader/modules/wasm/fixtures/runtime/images.mjs';

const [enginePath, reportPath, permissionText = '1', portText = ''] = process.argv.slice(2);
assert(enginePath && reportPath, 'usage: v86.mjs imported-engine.wasm fresh-report.json');
assert(['', 'port', 'inline', 'direct', 'direct-bindings', 'simd'].includes(portText));
const deniedPermission = Number(permissionText);
assert([0,1].includes(deniedPermission));
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const artifact = readEngine(enginePath);
const image = readFileSync(new URL('../../../process/images/startup/imports/fixtures/p2-c-imports/program.exe', import.meta.url));
assert.equal(hash(image), '7c0451977a0818d5f4905a2b00f3a3bae63b9449ef22775ea003ebb0ca69c5d5');
const cpuBytes = readFileSync(new URL('../../../../../sdk/runtime/v86/vendor/v86.wasm', import.meta.url));
const report = {complete: false, engineSha256: artifact.sha256, cpuSha256: hash(cpuBytes),
  scope: 'actual unmodified v86 CPU, shared RAM, authored PE with four Ring3 Windows calls; no game/FPS claim',
  portControls:portText==='port',inlineControls:portText==='inline',directControls:portText==='direct'||portText==='direct-bindings',directBindingsControls:portText==='direct-bindings',simdControls:portText==='simd',calls: [], runs: [], ownCpuCalls: 0};
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
    initRun = cache.execute(cache.select()); cache.publish();
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
      run = cache.execute(cache.select()); cache.publish(); report.runs.push(run); save();
      if (run.reason === 8) break;
      assert([1,2,6].includes(run.coreReason), 'only bounded CPU progress before a gate');
      await new Promise(resolve => setTimeout(resolve, 0));
    }
    assert.equal(run.reason, 8); assert.equal(run.detail, wanted.id);
    const fused=typeof engine.api.component_cpu_try_complete_gate==='function'
      ? engine.status('component_cpu_try_complete_gate',...KEY) : 256;
    assert([0,256].includes(fused),'authored synchronous calls complete or decline before capture');
    if(fused===256)engine.ok('core_capture_windows_call');
    else report.nativeFusedCalls=(report.nativeFusedCalls??0)+1;
    const packet = view(engine.take(112));
    const token = packet.getUint32(16, true);
    const args = Array.from({length: packet.getUint32(28, true)}, (_, i) => packet.getUint32(48 + i * 4, true));
    assert.deepEqual(args, wanted.args);
    if(fused===256)engine.ok('core_complete_windows_call', token);
    assert.equal(engine.get(48), wanted.returnPc);
    assert.equal(engine.get(16), wanted.result);
    assert.deepEqual(banks(), initialBanks, 'complete extended banks survive native call and trap');
    engine.ok('read32', 0x7f000034); assert.equal(engine.get(116), 0); assert.equal(engine.get(120), wanted.error);
    report.calls.push({id: wanted.id, args, returnPc: engine.get(48), eax: engine.get(16)});
    cache.import();
  }
  stage = 'final guest writes';
  if(typeof engine.api.component_cpu_try_complete_gate==='function')assert.equal(report.nativeFusedCalls,4);
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
    cache.execute(cache.select()); cache.publish();
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
    arithmetic = cache.execute(cache.select()); cache.publish();
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
  for(const [selectiveCode,incrementalSync,nativeDirty] of [[false,false,false],[true,false,false],
      [false,true,false],[true,true,false],[false,true,true],[true,true,true]]) {
    assert.equal(cache.imported,false,'the preceding actual CPU batch already published its writes');
    cache.tables.refresh();
    cache.tables.configure({selectiveCode,incrementalSync});
    cache.tables.configureDirty(nativeDirty,{paused:true,open:cache.open});
    const observations=[],original=cache.tables.publishDirty;
    const order=[],clear=cache.cpu.full_clear_tlb,main=cache.cpu.main_loop;
    cache.cpu.full_clear_tlb=function(...args){order.push('flush');return clear.apply(this,args);};
    cache.cpu.main_loop=function(...args){order.push('cpu');return main.apply(this,args);};
    cache.tables.publishDirty=function() {
      const d=new DataView(memory.buffer),directory=this.ram+DIRECTORY+(dataAddress>>>22)*4;
      const observed={accessed:!!(d.getUint32(directory,true)&32),
        dirty:!!(d.getUint32(this.pte(dataAddress>>>12),true)&64)};
      const result=original.call(this);
      observations.push({...observed,accessedAfter:!!(d.getUint32(directory,true)&32),
        scannedPtes:this.lastDirtyScanCount,dirtySlots:result});return result;
    };
    try {
      put(dataAddress,0x12345678);
      const contentVersion=()=>{
        engine.ok('component_cpu_view',...KEY);const descriptor=engine.resultView(128),d=new DataView(memory.buffer);
        const slot=d.getUint32(descriptor.getUint32(24,true)+(dataAddress>>>12)*4,true)-1;
        return d.getBigUint64(descriptor.getUint32(48,true)+slot*16+8,true);
      };
      const beforeVersion=contentVersion();let readVersion;
      for(const pc of [0x600300,0x600340]) {
        new DataView(memory.buffer).setUint32(engine.base+48,pc,true);cache.import();
        const run=cache.execute(cache.select());cache.publish();assert.equal(run.reason,8);
        if(pc===0x600300)readVersion=contentVersion();
      }
      assert.equal(observations[0].accessed,true,'actual read marks the page directory accessed');
      assert.equal(observations[0].dirty,false,'read-only batch does not dirty the data PTE');
      assert.equal(observations[1].dirty,true,'later actual store marks the data PTE dirty');
      assert.equal(observations[0].dirtySlots,0);assert.equal(observations[1].dirtySlots,1);
      assert.equal(readVersion,beforeVersion,'read-only collection preserves canonical content version');
      assert(contentVersion()>beforeVersion,'actual guest write advances canonical content version');
      if(incrementalSync)assert.equal(observations[0].accessedAfter,false,'partitioned read-only collection resets access');
      if(incrementalSync) {
        const first=order.indexOf('cpu'),second=order.indexOf('cpu',first+1);
        assert(order.slice(first+1,second).includes('flush'),'read-only A-bit reset flushes before the next actual CPU batch');
      }
      assert.equal(read(dataAddress),0x76543210);
      report.memoryPolicies.push({selectiveCode,incrementalSync,nativeDirty,observations,order});
    } finally {cache.tables.publishDirty=original;cache.cpu.full_clear_tlb=clear;cache.cpu.main_loop=main;}
  }
  stage='host code write invalidates actual finalized v86 code';
  const hotPc=0x600400,hot=value=>[0xc7,0x05,...u32(dataAddress),...u32(value),0xe9,...u32(-15)];
  upload(hotPc,hot(1));
  const generatedBefore=cache.stats().v86.finalizedModules;
  for(let count=0;count<24;count++) {
    new DataView(memory.buffer).setUint32(engine.base+48,hotPc,true);
    cache.import();cache.execute(cache.select());cache.publish();await new Promise(resolve=>setTimeout(resolve,2));
  }
  assert(cache.stats().v86.finalizedModules>generatedBefore,'the authored hot loop finalized actual Wasm');
  assert.equal(read(dataAddress),1);
  let clears=0;const clear=cache.cpu.jit_clear_cache;
  cache.cpu.jit_clear_cache=function(...args){clears++;return clear.apply(this,args);};
  try {
    engine.ok('map',0x690000,1,3);cache.import();
    assert.equal(clears,0,'unrelated real data mapping retains hot code');
    upload(hotPc,hot(2));cache.import();assert.equal(clears,1,'host executable write invalidates hot code');
    new DataView(memory.buffer).setUint32(engine.base+48,hotPc,true);cache.import();cache.execute(cache.select());cache.publish();
    assert.equal(read(dataAddress),2);
    engine.ok('unmap',0x600000,1);engine.ok('map',0x600000,1,7);upload(hotPc,hot(3));
    new DataView(memory.buffer).setUint32(engine.base+48,hotPc,true);cache.import();cache.execute(cache.select());cache.publish();
    assert.equal(read(dataAddress),3,'same-VA remap executes new canonical bytes');
    report.hostCodeInvalidation={clears,finalizedModules:cache.stats().v86.finalizedModules};
  } finally {cache.cpu.jit_clear_cache=clear;}
  if(portText==='simd') {
    stage='dense read-only PTE validation';
    const dense=0x6c0000,pc=0x600d00,code=[];
    engine.ok('map',dense,4,3);
    for(let index=0;index<4;index++)code.push(0xa1,...u32(dense+index*4096));
    code.push(0xb8,...u32(gate),0xff,0xe0);upload(pc,code);
    const version=()=>{engine.ok('component_cpu_view',...KEY);return engine.resultView(128).getBigUint64(72,true);};
    report.densePteValidation=[];
    for(let lane=0;lane<4;lane++) {
      new DataView(memory.buffer).setUint32(engine.base+48,pc,true);cache.import();
      const run=cache.execute(cache.select());assert.equal(run.reason,8);assert.equal(cache.dirtyPending,true);
      const data=new DataView(memory.buffer),offset=cache.tables.pte((dense>>>12)+lane);
      const original=data.getUint32(offset,true);assert.equal(original&64,0,'the actual dense reads are not dirty');
      data.setUint32(offset,original^2,true);
      const evidence=Buffer.from(new Uint8Array(memory.buffer,cache.tables.ram+DIRECTORY,4096*1025));
      const before=version(),dirtyBefore=cache.dirtyPages;
      assert.throws(()=>cache.publish(),error=>error.operation==='component_cpu_collect_dirty');
      assert.equal(cache.dirtyPending,true);assert.equal(version(),before);
      assert.deepEqual(Buffer.from(new Uint8Array(memory.buffer,cache.tables.ram+DIRECTORY,4096*1025)),evidence,
        'a malformed zero-dirty lane preserves every A/D bit');
      new DataView(memory.buffer).setUint32(offset,original,true);cache.publish();
      assert.equal(cache.dirtyPending,false);assert.equal(version(),before);
      assert.equal(cache.dirtyPages,dirtyBefore);assert(cache.tables.lastDirtyScanCount>=4);
      assert.equal(new DataView(memory.buffer).getUint32(cache.tables.ram+DIRECTORY+(dense>>>22)*4,true)&32,0);
      report.densePteValidation.push({lane,before,after:version(),scannedPtes:cache.tables.lastDirtyScanCount});save();
    }
  } else if(portText==='inline' || portText==='direct' || portText==='direct-bindings') {
    const directMode=portText!=='inline',bindingMode=portText==='direct-bindings';
    stage=directMode?'direct scalar API continuation':'inline native API continuation';
    engine.ok('protect',0x601000,1,3);
    const entries=new Map();
    for(let start=0,total=1;start<total;) {
      engine.ok('module_gates_page',start,128);const rows=engine.resultView(4096);
      total=rows.getUint32(16,true);const count=rows.getUint32(24,true);assert(count>0);
      for(let index=0;index<count;index++)entries.set(rows.getUint32(36+index*8,true),rows.getUint32(32+index*8,true));
      start+=count;
    }
    const ids={get:0x10001,set:0x10002,allocate:0xf0009,tlsGet:0xf000b,tlsSet:0xf000c,slow:0xc0002};
    for(const id of Object.values(ids))assert(entries.has(id),`real registered gate ${id.toString(16)}`);
    const programPc=0x600900,output=0x601900,stack=0x601800,initialError=0xf1234567,nextError=0x10203040,tlsValue=0x55aa7788;
    const savedBanks=banks(),atomicSupported=typeof engine.api.component_cpu_try_inline_gate==='function';
    if(directMode)assert.equal(typeof engine.api.component_cpu_try_direct_gate,'function');
    const directOperation=typeof engine.api.component_cpu_try_direct_gate_mask==='function'?'component_cpu_try_direct_gate_mask':'component_cpu_try_direct_gate';
    if(bindingMode)assert.equal(directOperation,'component_cpu_try_direct_gate_mask');
    let allow=true,clockCalls=0,bankExports=0,bankTagReads=0,stateFlagUpdates=0;
    cache.inlineOwner={canContinue:()=>allow,observeClock(){clockCalls++;engine.ok('observe_cpu_clock',0,0);return 0n;}};
    assert.equal(cache.configurePortGates(true,{paused:true,open:cache.open}),true);
    const seed=pc=>{
      const registers=[0x11223344,0,0xe1,0x3456789a,stack,0x456789ab,0x56789abc,0x6789abcd];
      const data=new DataView(memory.buffer);
      registers.forEach((value,index)=>data.setUint32(engine.base+16+index*4,value,true));
      data.setUint32(engine.base+48,pc,true);data.setUint32(engine.base+52,0x646,true);
      return registers;
    };
    const nativeCall=(id,args=[])=>{
      const enabled=cache.inlineGates;
      if(enabled)cache.configureInlineGates(false,{paused:true,open:cache.open});
      seed(entries.get(id));put(stack,0x600c00);args.forEach((value,index)=>put(stack+4+index*4,value));
      cache.import();const run=cache.execute(cache.select());cache.publish();assert.equal(run.detail,id);
      engine.ok('core_capture_windows_call');const token=engine.resultView(112).getUint32(16,true);
      engine.ok('core_complete_windows_call',token);const result=engine.get(16);
      if(enabled)cache.configureInlineGates(true,{paused:true,open:cache.open});
      return result;
    };
    const tlsIndex=nativeCall(ids.allocate);assert(tlsIndex<1088);
    assert.equal(nativeCall(ids.tlsSet,[tlsIndex,tlsValue]),1);
    const threadId=nativeCall(ids.slow),program=[0xb9,...u32(20000),0x49,0x75,0xfd];
    const invoke=id=>program.push(0xb8,...u32(entries.get(id)),0xff,0xd0);
    invoke(ids.get);const afterGet=programPc+program.length;program.push(0xa3,...u32(output));
    program.push(0x68,...u32(nextError));invoke(ids.set);program.push(0xa3,...u32(output+4));
    program.push(0x68,...u32(tlsIndex));invoke(ids.tlsGet);program.push(0xa3,...u32(output+8));
    invoke(ids.slow);const afterSlow=programPc+program.length;
    program.push(0xa3,...u32(output+12),0xc7,0x05,...u32(output+16),...u32(0x52494e47));
    invoke(ids.slow);program.push(0xeb,0xfe);upload(programPc,program);
    cache.budget=100002;
    const inlineReport=report[directMode?'direct':'inline']={tlsIndex,programPc,afterGet,afterSlow,atomicSupported,
      directMode,bindingMode,directOperation,requestedMask:bindingMode?0x7ff:0xff,
      bindingCoverage:'mask selection and existing scalar continuation only; no D3D device is provisioned by this fixture',
      authoredLoopPrefixInstructions:40001,authoredFirstOuterInstructions:40018,runs:[],
      semantics:directMode?'direct calls preserve canonical State/Exit/banks/R3WK and return scalars; only the completed outer main_loop is published and charged':'all intermediate gate publications carry zero credits; only completed outer main_loop deltas are charged'};
    const reset=()=>{nativeCall(ids.set,[initialError]);for(let index=0;index<5;index++)put(output+index*4,0);seed(programPc);};
    const workPacket=()=>engine.get(4604);
    const execute=(label)=>{
      const original=engine.ok,originalStatus=engine.status,originalContinue=cache.traps.continueGate,
        publications=[],atomicCalls=[],directCalls=[],directMasks=[],directResults=[],continuations=[],before=cache.stats(),
        counter=cpuViews(memory).data.getUint32(664,true),exportsBefore=bankExports;
      if(directMode)cache.traps.continueGate=trap=>{
        const exported=bankExports,tags=bankTagReads,updates=stateFlagUpdates,observed=clockCalls;
        const result=originalContinue(trap);
        assert.equal(bankExports,exported,'direct continuation does not export FP banks');
        assert.equal(bankTagReads,tags,'direct continuation invokes no x87 import/export helpers');
        assert.equal(stateFlagUpdates,updates,'direct continuation invokes no full state import');
        assert.equal(clockCalls,observed,'direct continuation does not observe the clock');
        continuations.push({result,bankExports:0,bankTagReads:0,stateFlagUpdates:0,clockObservations:0});
        return result;
      };
      const snapshot=()=>Buffer.concat([Buffer.from(new Uint8Array(memory.buffer,engine.base,96)),
        engine.take(112),banks(),Buffer.from(new Uint8Array(memory.buffer,engine.base+4588,24))]);
      engine.status=function(operation,...args) {
        const watched=operation==='component_cpu_try_inline_gate' || operation===directOperation;
        const previous=watched?snapshot():null;
        const canonical=operation===directOperation?Buffer.concat([
          Buffer.from(new Uint8Array(memory.buffer,engine.base,140)),
          Buffer.from(new Uint8Array(memory.buffer,engine.base+172,engine.api.arena_len()-172)),
        ]):null;
        const exported=bankExports,observed=clockCalls;
        const status=originalStatus.call(this,operation,...args);
        if(operation===directOperation) {
          if(operation==='component_cpu_try_direct_gate_mask') {
            assert.equal(args[5],bindingMode?0x7ff:0xff);directMasks.push(args[5]);
          }
          directCalls.push(status);
          assert.equal(bankExports,exported,'native direct call invokes no guest FP export helpers');
          assert.equal(clockCalls,observed,'native direct call does not observe the clock');
          if(status===256)assert.deepEqual(snapshot(),previous,'direct decline preserves canonical state/exit/banks/work/transfer');
          if(status===0) {
            assert.deepEqual(Buffer.concat([Buffer.from(new Uint8Array(memory.buffer,engine.base,140)),
              Buffer.from(new Uint8Array(memory.buffer,engine.base+172,engine.api.arena_len()-172))]),canonical,
              'direct success changes only its32byte compact result, not canonical CPU/work state');
            const result=this.resultView(32);
            assert.equal(result.getUint32(0,true),0x46493352);assert.equal(result.getUint32(4,true),0x10001);
            assert.equal(result.getUint32(8,true),32);assert.equal(result.getUint32(12,true),0);
            directResults.push({id:result.getUint32(16,true),eax:result.getUint32(20,true),
              pc:result.getUint32(24,true),esp:result.getUint32(28,true),canonicalUnchanged:true,bankExports:0,clockObservations:0});
          }
        }
        if(operation==='component_cpu_try_inline_gate') {
          atomicCalls.push(status);
          if(status===256)assert.deepEqual(snapshot(),previous,'atomic decline leaves state/exit/banks/work/transfer unchanged');
          if(status===0){const record=this.resultView(112);publications.push({source:'atomic-native',id:record.getUint32(20,true),
            recordMagic:record.getUint32(0,true),recordSize:record.getUint32(8,true),gatePc:record.getUint32(32,true),
            canonicalReason:this.get(72),retired:this.get(76),work:workPacket(),packetWork:workPacket(),
            visibleCounter:cpuViews(memory).data.getUint32(664,true),stack:new Error('atomic inline completion').stack});}
        }
        return status;
      };
      engine.ok=function(operation,...args) {
        const result=original.call(this,operation,...args);
        if(operation==='component_cpu_publish')publications.push({source:'js-publication',stop:args[2],id:args[5],retired:args[6],work:args[7],
          packetWork:workPacket(),visibleCounter:cpuViews(memory).data.getUint32(664,true),
          stack:new Error('inline publication').stack});
        return result;
      };
      let run;
      try {cache.import();run=cache.execute(cache.select());cache.publish();}
      finally {engine.ok=original;engine.status=originalStatus;cache.traps.continueGate=originalContinue;}
      const after=cache.stats(),delta=(cpuViews(memory).data.getUint32(664,true)-counter)>>>0;
      const row={label,run,publications,atomicCalls,directCalls,directMasks,directResults,continuations,bankExports:bankExports-exportsBefore,beforeCounter:counter,delta,inlineCompletions:after.inlineCompletions-before.inlineCompletions,
        directCompletions:(after.directCompletions??0)-(before.directCompletions??0),directDeclines:(after.directDeclines??0)-(before.directDeclines??0),
        directBindingCompletions:(after.directBindingCompletions??0)-(before.directBindingCompletions??0),
        runs:after.runs-before.runs,regions:after.regions-before.regions,gates:after.gates-before.gates,workPacket:workPacket(),clockCalls};
      assert.equal(run.work,delta);assert.equal(workPacket(),delta);
      assert.equal(after.work-before.work,delta);assert.equal(after.retired-before.retired,run.retired);
      assert.equal(row.runs,1);assert.equal(row.regions,1);assert.deepEqual(banks(),savedBanks);
      for(const publication of publications.slice(0,-1)) {
        assert.equal(publication.work,0);assert.equal(publication.retired,0);assert.equal(publication.packetWork,0);
        if(publication.source==='atomic-native'){
          assert.equal(publication.recordMagic,0x46433352);assert.equal(publication.recordSize,112);
          assert.equal(publication.gatePc,entries.get(publication.id));assert.equal(publication.canonicalReason,3);
        }
      }
      if(label==='warmup') {
        inlineReport.warmupCount=(inlineReport.warmupCount??0)+1;inlineReport.lastWarmup=row;
      } else {inlineReport.runs.push(row);save();}
      return row;
    };
    const checkChain=row=>{
      assert.equal(row.run.reason,8);assert.equal(row.run.detail,ids.slow);
      assert.equal(row.run.pc,entries.get(ids.slow));assert.equal(row.inlineCompletions,3);
      assert.equal(row.gates,4);assert.equal(row.publications.length,directMode?1:4);
      assert.equal(row.bankExports,directMode?1:4,'direct successful calls skip banks and the slow fallback exports exactly once');
      if(directMode) {
        assert.deepEqual(row.directCalls,[0,0,0,256]);assert.deepEqual(row.atomicCalls,[]);
        assert.equal(row.directCompletions,3);assert.equal(row.directDeclines,1);assert.equal(clockCalls,0);
        assert.equal(row.directBindingCompletions,0);
        assert.deepEqual(row.directMasks,directOperation==='component_cpu_try_direct_gate_mask'?Array(4).fill(bindingMode?0x7ff:0xff):[]);
        assert.deepEqual(row.directResults.map(result=>result.id),[ids.get,ids.set,ids.tlsGet]);
      } else if(atomicSupported){assert.deepEqual(row.atomicCalls,[0,0,0,256]);assert.equal(row.publications.filter(p=>p.source==='js-publication').length,1);}
      assert.equal(row.delta,40018);assert.equal(row.run.retired,40018);
      assert.equal(engine.get(32),stack-4);assert.equal(engine.get(52),0x646);
      assert.deepEqual([read(output),read(output+4),read(output+8)],[initialError,entries.get(ids.set),tlsValue]);
      assert.equal(read(0x7f000034),0,'successful actual TLS getter clears LastError');
      assert.equal(read(output+16),0,'first slow gate has not executed its return continuation');
    };
    cache.configureInlineGates(true,{paused:true,open:cache.open});
    if(directMode)cache.configureDirectGates(true,{paused:true,open:cache.open});
    if(bindingMode)cache.configureDirectBindings(true,{paused:true,open:cache.open});
    const wm=cache.cpu.wm,originalExports=wm.exports,exportDescriptor=Object.getOwnPropertyDescriptor(wm,'exports');
    const observedExports={...originalExports,fpu_load_status_word(...args){bankExports++;return originalExports.fpu_load_status_word(...args);},
      fpu_load_tag_word(...args){bankTagReads++;return originalExports.fpu_load_tag_word(...args);}};
    Object.defineProperty(wm,'exports',{value:observedExports,writable:true,configurable:true});
    const flagsDescriptor=Object.getOwnPropertyDescriptor(cache.cpu,'update_state_flags'),originalFlags=cache.cpu.update_state_flags;
    Object.defineProperty(cache.cpu,'update_state_flags',{value:function(...args){stateFlagUpdates++;return originalFlags.apply(this,args);},writable:true,configurable:true});
    const ex=wm.exports,oldJit=ex.get_jit_config(0),oldStackLimit=Error.stackTraceLimit;
    Error.stackTraceLimit=64;
    try {
      stage='cold inline work aggregate';ex.set_jit_config(0,1);reset();checkChain(execute('cold'));
      ex.set_jit_config(0,0);
      inlineReport.jitBefore=runtime.counters();
      for(let index=0;index<20;index++){reset();checkChain(execute('warmup'));await new Promise(resolve=>setTimeout(resolve,2));}
      inlineReport.jitAfter=runtime.counters();
      assert(inlineReport.jitAfter.finalizedModules>inlineReport.jitBefore.finalizedModules);
      stage='generated inline work aggregate';reset();const warmed=execute('generated');checkChain(warmed);
      inlineReport.nativeGateWitness={generatedCallback:/at g[0-9a-f]+@t/.test(warmed.publications[0].stack),
        semantics:'registered API thunk may remain interpreted; finalized modules during this caller campaign and the separate actual continuation probe provide the bounded JIT evidence'};
      engine.ok('core_capture_windows_call');const token=engine.resultView(112).getUint32(16,true);
      engine.ok('core_complete_windows_call',token);assert.equal(engine.get(48),afterSlow);
      const followOn=execute('first slow gate completed once');
      assert.equal(followOn.inlineCompletions,0);assert.equal(followOn.delta,5);assert.equal(followOn.run.detail,ids.slow);
      assert.equal(followOn.bankExports,1);
      if(directMode){assert.deepEqual(followOn.directCalls,[256]);assert.deepEqual(followOn.atomicCalls,[]);}
      else if(atomicSupported)assert.deepEqual(followOn.atomicCalls,[256]);
      assert.equal(read(output+12),threadId);assert.equal(read(output+16),0x52494e47);
      stage='inline owner opt-out';reset();allow=false;const clocks=clockCalls,optOut=execute('owner opt-out');
      assert.equal(optOut.inlineCompletions,0);assert.equal(optOut.run.detail,ids.get);
      assert.equal(optOut.delta,40004);assert.equal(clockCalls,clocks);assert.equal(optOut.publications.length,1);
      assert.equal(optOut.bankExports,1);if(atomicSupported)assert.deepEqual(optOut.atomicCalls,[]);
      allow=true;
      stage='completed inline deadline yields returned state';reset();const completed=cache.inlineCompletions;
      cache.inlineOwner.canContinue=()=>cache.inlineCompletions===completed;
      const yielded=execute('deadline after successful completion');
      assert.equal(yielded.run.inlineYield,true);assert.equal(yielded.run.coreReason,2);assert.equal(yielded.delta,40004);
      assert.equal(yielded.inlineCompletions,1);assert.equal(engine.get(48),afterGet);
      assert.equal(yielded.bankExports,directMode?1:2,'returned-state Budget yield exports completed state exactly once for direct mode');
      assert.equal(engine.get(16),initialError);assert.equal(engine.get(32),stack);assert.equal(read(output),0);
      cache.inlineOwner.canContinue=()=>true;const resumed=execute('resume after completed deadline yield');
      assert.equal(resumed.run.detail,ids.slow);assert.equal(resumed.inlineCompletions,2);
      assert.equal(yielded.delta+resumed.delta,40018);
      stage='captured inline Busy is terminal';reset();const status=engine.status,before=cache.stats();let captures=0,failure;
      const completionOperation=directMode?directOperation:atomicSupported?'component_cpu_try_inline_gate':'component_cpu_try_complete_gate';
      engine.status=function(operation,...args) {
        if(operation===completionOperation) {
          if(directMode) {
            const views=cpuViews(memory);
            exportState(this,cache.cpu,{pc:args[2],esp:args[3],flags:cache.cpu.wm.exports.get_eflags(),
              cs:views.selectors[1],ss:views.selectors[2]});
          }
          if(atomicSupported || directMode)status.call(this,'component_cpu_publish',...KEY,7,0,0,ids.get,0,0);
          captures++;status.call(this,'core_capture_windows_call');return 12;
        }
        return status.call(this,operation,...args);
      };
      try {cache.import();assert.throws(()=>cache.execute(cache.select()),error=>{failure=error;return error.status===12;});}
      finally {engine.status=status;}
      for(const operation of ['import','select','execute'])assert.throws(()=>cache[operation](),error=>error===failure);
      assert.equal(captures,1);assert.equal(cache.stats().runs,before.runs);assert.equal(read(output+16),0);
      assert.equal(cache.stats().inlineFailed,true);inlineReport.busy={poisoned:true,captures,message:failure.message,
        operation:completionOperation,injection:directMode?'test-only full live-state export and zero Gate publication plus real capture, then simulated ABI Busy; direct handler not executed':atomicSupported?'explicit zero Gate publication plus real capture, then simulated ABI Busy; handler not executed':'real capture then simulated ABI Busy; handler not executed'};
    } finally {ex.set_jit_config(0,oldJit);Error.stackTraceLimit=oldStackLimit;
      if(flagsDescriptor)Object.defineProperty(cache.cpu,'update_state_flags',flagsDescriptor);else delete cache.cpu.update_state_flags;
      if(exportDescriptor)Object.defineProperty(wm,'exports',exportDescriptor);else delete wm.exports;}
  } else if(portText==='port') {
    stage='owned port gates and denied I/O forms';
    assert.equal(cache.portGates,false,'the existing default remains UD2');
    const probePc=0x600700,source=0x601600,destination=0x601640,unmapped=0x6f0000;
    const savedBanks=banks(),gateWord=read(gate)&0xffff;
    assert.equal(gateWord,0x0b0f);
    const seed=(pc,{count=4,unmappedSource=false,unmappedDestination=false}={})=>{
      const registers=[0x11223344,count,0xe1,0x3456789a,0x601800,0x456789ab,
        unmappedSource?unmapped:source,unmappedDestination?unmapped:destination];
      const data=new DataView(memory.buffer);
      for(let index=0;index<8;index++)data.setUint32(engine.base+16+index*4,registers[index],true);
      data.setUint32(engine.base+48,pc,true);data.setUint32(engine.base+52,0x646,true);
      return registers;
    };
    const stateEquals=(pc,registers)=>{
      assert.equal(engine.get(48),pc);
      assert.deepEqual(Array.from({length:8},(_,index)=>engine.get(16+index*4)),registers);
      assert.equal(engine.get(52),0x646);
      assert.deepEqual(banks(),savedBanks,'I/O preserves x87/SIMD/YMM and guest segment banks');
    };
    const ownedGate=()=>{
      const registers=seed(gate);cache.import();
      const run=cache.execute(cache.select());cache.publish();
      assert.equal(run.reason,8);assert.equal(run.detail,0x10001);
      stateEquals(gate,registers);
      if(cache.portGates)assert.equal(cpuViews(memory).bytes[612],3,'owned OUT never enters ring0');
      return run;
    };
    const forms=[
      {name:'out-imm',bytes:[0xe6,0xe1]}, {name:'out-dx',bytes:[0xee]},
      {name:'in-imm',bytes:[0xe4,0xe1]}, {name:'in-dx',bytes:[0xec]},
      {name:'out-wide',bytes:[0xe7,0xe1]}, {name:'in-wide',bytes:[0xe5,0xe1]},
      {name:'ins',bytes:[0x6c]}, {name:'outs',bytes:[0x6e]},
      {name:'rep-ins',bytes:[0xf3,0x6c]}, {name:'rep-outs',bytes:[0xf3,0x6e]},
      {name:'rep-ins-unmapped',bytes:[0xf3,0x6c],unmappedDestination:true},
      {name:'rep-outs-unmapped',bytes:[0xf3,0x6e],unmappedSource:true},
      {name:'rep-ins-zero',bytes:[0xf3,0x6c],count:0},
      {name:'rep-outs-zero',bytes:[0xf3,0x6e],count:0},
      {name:'out-other-port',bytes:[0xe6,0xe2]},
    ];
    report.portGates={forms:[],semantics:'nonterminal denied runs intercept native exception publication only; each is followed by a real ordinary gate, then one terminal #GP is genuinely committed'};
    for(const enabled of [false,true]) {
      assert.equal(cache.configurePortGates(enabled,{paused:true,open:cache.open}),enabled);
      assert.equal(read(gate)&0xffff,enabled?0xe1e6:gateWord);
      report.portGates[enabled?'ownedPort':'ownedUd2']=ownedGate();
      if(enabled) {
        engine.ok('register_user32_module',0x71000000);
        engine.ok('load_library_module',engine.name('user32.dll'),0);
        let dynamic;
        for(let start=0,total=1;start<total&&!dynamic;) {
          engine.ok('module_gates_page',start,128);const rows=engine.resultView(4096);
          total=rows.getUint32(16,true);const count=rows.getUint32(24,true);
          assert(count>0);
          for(let index=0;index<count;index++) {
            const id=rows.getUint32(36+index*8,true);
            if(id>>>16===2){dynamic={pc:rows.getUint32(32+index*8,true),id};break;}
          }
          start+=count;
        }
        assert(dynamic,'new system module exposes a registered gate');
        assert.equal(read(dynamic.pc)&0xffff,0x0b0f,'newly loaded gate starts with real UD2');
        const count=cache.stats().portGateCalls,runs=[];
        for(let index=0;index<2;index++) {
          const registers=seed(dynamic.pc);cache.import();
          const run=cache.execute(cache.select());cache.publish();
          assert.equal(run.reason,8);assert.equal(run.detail,dynamic.id);stateEquals(dynamic.pc,registers);
          assert.equal(cache.stats().portGateCalls-count,index,'first UD2 falls back before the next owned OUT');
          runs.push(run);
        }
        report.portGates.dynamicModule={...dynamic,runs};
        const warmPc=0x600780,budget=cache.budget,before=runtime.counters();
        upload(warmPc,[0xb9,...u32(20000),0x49,0x75,0xfd,0xb8,...u32(gate),0xff,0xe0]);
        try {
          cache.budget=100002;
          for(let index=0;index<16;index++) {
            const registers=seed(warmPc);registers[0]=gate;registers[1]=0;
            cache.import();const run=cache.execute(cache.select());cache.publish();
            assert.equal(run.reason,8);assert.equal(run.detail,0x10001);stateEquals(gate,registers);
            assert.equal(cpuViews(memory).bytes[612],3);
            await new Promise(resolve=>setTimeout(resolve,2));
          }
        } finally {cache.budget=budget;}
        const after=runtime.counters();
        assert(after.finalizedModules>before.finalizedModules,'port gate program finalizes actual hot Wasm');
        report.portGates.warmJit={before,after};
      }
      for(const form of forms) {
        stage=`${enabled?'port':'legacy'} denied I/O ${form.name}`;
        upload(probePc,[...form.bytes,0x0f,0x0b,0xeb,0xfe]);
        for(let index=0;index<32;index++)put(source+index*4,0x8899aabb+index);
        const canary=Array.from({length:32},(_,index)=>read(source+index*4));
        const registers=seed(probePc,form),original=engine.ok;
        let publication=null;
        engine.ok=function(operation,...args) {
          if(operation==='component_cpu_publish'&&args[2]===9) {
            publication={stop:args[2],error:args[3],vector:args[5]};return;
          }
          return original.call(this,operation,...args);
        };
        try {cache.import();cache.execute(cache.select());cache.publish();}
        finally {engine.ok=original;}
        report.portGates.pendingCase={enabled,name:form.name,publication,registers,
          actualRegisters:Array.from({length:8},(_,index)=>engine.get(16+index*4)),pc:engine.get(48),flags:engine.get(52)};save();
        const skipped=form.count===0,expectedVector=skipped?6:13,expectedPc=probePc+(skipped?form.bytes.length:0);
        assert.deepEqual(publication,{stop:9,error:0,vector:expectedVector},
          `${enabled?'port':'legacy'} ${form.name} matches denied-mode v86 behavior`);
        stateEquals(expectedPc,registers);
        assert.deepEqual(Array.from({length:32},(_,index)=>read(source+index*4)),canary,
          `${form.name} must preserve source and destination bytes`);
        const followOn=ownedGate();
        report.portGates.forms.push({enabled,name:form.name,publication,expectedVector,expectedPc,registers,followOn});save();
      }
    }
    delete report.portGates.pendingCase;
    assert.equal(cache.configurePortGates(false,{paused:true,open:cache.open}),false);
    assert.equal(read(gate)&0xffff,gateWord,'mode-off restores original gate bytes');
    assert.equal(cache.configurePortGates(true,{paused:true,open:cache.open}),true);
    upload(probePc,[0xe6,0xe1,0xeb,0xfe]);
    const registers=seed(probePc);cache.import();
    const fault=cache.execute(cache.select());cache.publish();
    assert.equal(fault.coreReason,9);assert.equal(fault.reason,11);assert.equal(fault.detail,0);
    stateEquals(probePc,registers);
    report.portGates.committedGeneralProtection=fault;
  } else {
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
  const fault = cache.execute(cache.select()); cache.publish();
  assert.equal(fault.coreReason, 3); assert.equal(fault.pc, 0x600100);
  assert.equal(fault.reason, 5); assert.equal(fault.detail, 2, 'mapped denied page is a permission fault');
  assert.equal(new DataView(engine.memory.buffer).getUint32(physical, true), 0x11223344, 'faulting write preserves canonical RAM');
  report.deniedPageFault = {permission: deniedPermission, ...fault};
  }
  report.stats = cache.stats(); report.complete = true;
} catch (error) {
  report.failure = {stage, message: error.message, stack: error.stack}; throw error;
} finally {
  try {cache?.close(); if (engine) engine.ok('close'); runtime?.dispose();}
  catch (error) {report.complete = false; report.cleanupFailure = error.message; throw error;}
  finally {save();}
}
console.log(JSON.stringify({complete: report.complete, calls: report.calls.length, ownCpuCalls: report.ownCpuCalls, stats: report.stats}));
