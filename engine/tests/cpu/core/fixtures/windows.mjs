import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
import {dirname,resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
import {readEngine} from '../../../support/wasm/engine.mjs';
import {Engine,KEY,record,view} from '../../../../../sdk/runtime/engine.js';
import {CodeCache} from '../../../../../sdk/runtime/cache.js';

const [enginePath,reportPath] = process.argv.slice(2);
assert(enginePath && reportPath,'usage: windows.mjs engine.wasm report.json');
const root = fileURLToPath(new URL('../../../../../',import.meta.url));
const fixture = 'engine/tests/process/images/startup/imports/fixtures/p2-c-imports/';
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const sourcePaths = [fileURLToPath(import.meta.url),'sdk/runtime/engine.js','sdk/runtime/engine/module.js',
  'sdk/runtime/cache.js','engine/tests/support/wasm/engine.mjs',
  `${fixture}entry.c`,`${fixture}program.map`,`${fixture}program.exe`,`${fixture}oracle.json`,`${fixture}receipt.json`];
const sources = () => Object.fromEntries(sourcePaths.map(path => [path.startsWith(root) ? path.slice(root.length) : path,
  hash(readFileSync(resolve(root,path)))]));
const artifact = readEngine(enginePath), image = readFileSync(resolve(root,fixture,'program.exe'));
const pe = JSON.parse(readFileSync(resolve(root,fixture,'oracle.json')));
assert.equal(hash(image),'7c0451977a0818d5f4905a2b00f3a3bae63b9449ef22775ea003ebb0ca69c5d5');
assert.equal(image.length,2560); assert.equal(pe.image.preferred_base,0x400000);
assert.deepEqual(pe.expected_output_words,[0xf1234567,0,0x52494e47]);
mkdirSync(dirname(reportPath),{recursive:true});
const report = {schema:1,complete:false,engineSha256:artifact.sha256,imageSha256:hash(image),sources:sources(),
  scope:'one authored PE, four synchronous Windows calls and a bounded final loop; no callback or thread-switch claim',
  nativeCalls:[],forbiddenCalls:{},modules:[],runs:[],calls:[],negative:[]};
const checkpoint = () => writeFileSync(reportPath,JSON.stringify(report,null,2));
let engine,cache,backing = 0,backingLength = 0,expectedRam,initialBanks,stage = 'instantiate';
const bytes = (pointer,length) => Buffer.from(new Uint8Array(engine.memory.buffer).slice(pointer,pointer+length));
const word = (pointer,offset = 0) => new DataView(engine.memory.buffer).getUint32(pointer+offset,true);
const u32 = (data,offset) => new DataView(data.buffer,data.byteOffset,data.byteLength).getUint32(offset,true);
const banks = () => [bytes(engine.base+4236,128),bytes(engine.base+4364,160),bytes(engine.base+4524,64),
  bytes(engine.base+4612,144),bytes(engine.base+4756,40)];
const snapshot = () => ({stateHex:bytes(engine.base,56).toString('hex'),exitHex:bytes(engine.base+56,40).toString('hex'),
  arenaHex:bytes(engine.base,engine.api.arena_len()).toString('hex'),
  contextHex:cache?.context ? bytes(cache.context,128).toString('hex') : null,
  ramSha256:backing ? hash(bytes(backing,backingLength)) : null});
function physical(address) {
  const slot = word(word(cache.context,56)+(address>>>12)*4);
  assert(slot > 0 && slot <= word(cache.context,68),'authored address is mapped');
  return (slot-1)*4096+(address&4095);
}
function ram(address,length) {const offset = physical(address); assert((address&4095)+length <= 4096); return bytes(backing+offset,length);}
function patch(address,value) {expectedRam.writeUInt32LE(value>>>0,physical(address));}
function expectedState(registers,pc) {
  const output = record('R3ST',56), d = view(output);
  registers.forEach((value,index) => d.setUint32(16+index*4,value>>>0,true));
  d.setUint32(48,pc,true); d.setUint32(52,2,true); return Buffer.from(output);
}
function checkState(registers,pc) {
  assert.deepEqual(bytes(engine.base,56),expectedState(registers,pc),`${stage}: complete canonical State32`);
  assert.deepEqual(banks(),initialBanks,`${stage}: complete FP, SIMD, YMM and segment records`);
  assert.deepEqual(bytes(backing,backingLength),expectedRam,`${stage}: whole canonical RAM`);
}
function checkExit(version,reason,retired,detail = 0) {
  const expected = record('R3EX',40), d = view(expected); d.setUint16(4,version,true);
  d.setUint32(16,reason,true); d.setUint32(20,retired,true); d.setUint32(24,detail,true);
  assert.deepEqual(bytes(engine.base+56,40),Buffer.from(expected),`${stage}: complete canonical Exit`);
}
function captureModule(unit) {
  if (report.modules.some(item => item.id === String(unit.id))) return;
  const pointer = engine.api.core_code_ptr()>>>0, length = engine.api.core_code_len()>>>0;
  const encoded = bytes(pointer,length), module = new WebAssembly.Module(encoded);
  assert.deepEqual(WebAssembly.Module.imports(module),[
    {module:'ring3',name:'memory',kind:'memory'},{module:'ring3',name:'read',kind:'function'},
    {module:'ring3',name:'write',kind:'function'},
  ]);
  report.modules.push({id:String(unit.id),pc:unit.pc,bytes:length,sha256:hash(encoded),wasmHex:encoded.toString('hex')});
}
function until(stop) {
  let retired = 0,work = 0;
  for (let index = 0; index < 16; index++) {
    const unit = cache.entry(); captureModule(unit);
    const result = cache.execute(unit); retired += result.retired; work += result.work;
    report.runs.push({stage,...result,id:String(result.id),...snapshot()}); checkpoint();
    if (result.coreReason === 1) continue;
    assert.equal(result.coreReason,stop,`${stage}: exact core stop`); return {...result,retired,work};
  }
  assert.fail('authored control did not reach its bounded native boundary');
}
function neutral(name,operation,status,...args) {
  const before = snapshot(), actual = engine.status(operation,...args), after = snapshot();
  report.negative.push({name,operation,args,actual,wanted:status,before,after}); checkpoint();
  assert.equal(actual,status,name); assert.deepEqual(after,before,`${name}: context/arena/RAM neutral`);
}
try {
  const instance = await WebAssembly.instantiate(artifact.module,{ring3_observability:{now_ns:() => 0n}});
  const native = Object.fromEntries(Object.entries(instance.exports).filter(([name]) => name.startsWith('ring3_abi_v1_'))
    .map(([name,value]) => [name.slice(13),value]));
  const api = {...native};
  for (const name of Object.keys(api)) if (/^(compile(?:_|$)|.*execution_receipt$|dispatcher_module$|guard_detect(?:_|$)|acknowledge_resident)/.test(name)) {
    report.forbiddenCalls[name] = 0;
    api[name] = () => {report.forbiddenCalls[name]++; checkpoint(); throw new Error(`legacy execution export called: ${name}`);};
  }
  engine = new Engine(Object.freeze(api),instance.exports.memory,artifact.sha256);
  const status = engine.status.bind(engine);
  engine.status = (operation,...args) => {const result = status(operation,...args);
    report.nativeCalls.push({stage,operation,args,status:result}); return result;};
  engine.ok('open',256,...KEY); engine.base = api.arena_ptr()>>>0;
  stage = 'load authored PE';
  engine.ok('configure_cpu_clock',1_000_000_000,0,0,0);
  engine.ok('configure_module_limits',8,0x100000,16,65536,0x100000);
  engine.ok('register_kernel32_module',0x70000000);
  const name = 'core-imports.exe';
  engine.ok('begin_module_input',image.length,engine.name(name));
  for (let offset = 0; offset < image.length; offset += 4096) {
    const chunk = image.subarray(offset,offset+4096); engine.request(chunk); engine.ok('append_module_input',offset,chunk.length);
  }
  engine.ok('commit_module_input');
  const stack = 0x7e000000, top = stack+4*4096, teb = 0x7f000000;
  engine.ok('start_module_process',engine.name(name),0,stack,4,teb);
  assert.equal(engine.get(48),0x401000);
  const gates = new Map();
  for (let start = 0,total = 1; start < total;) {
    engine.ok('module_gates_page',start,128); const page = engine.take(4096), d = view(page);
    assert.equal(Buffer.from(page.subarray(0,4)).toString(),'R3GP'); total = d.getUint32(16,true);
    const count = d.getUint32(24,true); assert(count > 0 && count <= 128);
    for (let i = 0; i < count; i++) {const id = d.getUint32(36+i*8,true); assert(!gates.has(id)); gates.set(id,d.getUint32(32+i*8,true));}
    start += count;
  }
  const get = gates.get(0x10001), set = gates.get(0x10002); assert(get && set && get !== set);
  cache = new CodeCache(engine,{budget:16,instructions:64,blocks:16,capacity:16});
  for (const name of ['core_read','core_write','core_numeric']) assert.equal(cache.native[name],native[name]);
  report.originalNativeHelpers = true;
  const first = cache.entry(); captureModule(first);
  engine.ok('core_begin'); backing = word(cache.context,64); backingLength = word(cache.context,68)*4096;
  assert.equal(backingLength,256*4096); assert.equal(word(cache.context,104),teb); engine.ok('core_end');
  expectedRam = bytes(backing,backingLength); initialBanks = banks();
  const fp = Buffer.from(record('R3FP',128)); fp.writeUInt16LE(0x037f,16); fp.writeUInt16LE(0xffff,20);
  assert.deepEqual(initialBanks[0],fp,'literal initial complete FP80 record');
  checkState([0,0,0,0,top,0,0,0],0x401000);
  checkExit(3,3,0);
  const loaded = Buffer.alloc(0x5000); image.copy(loaded,0,0,1024);
  for (const section of pe.sections) image.copy(loaded,section.rva,section.raw_offset,section.raw_offset+section.raw_size);
  loaded.writeUInt32LE(get,0x2034); loaded.writeUInt32LE(set,0x2038);
  for (let page = 0; page < 5; page++) assert.deepEqual(ram(0x400000+page*4096,4096),loaded.subarray(page*4096,(page+1)*4096),'whole linked PE image');
  for (let page = 0; page < 4; page++) assert.deepEqual(ram(stack+page*4096,4096),Buffer.alloc(4096),'literal zero startup stack');
  assert.equal(ram(teb+0x34,4).readUInt32LE(),0);
  assert.equal(ram(teb+0x18,4).readUInt32LE(),teb); report.fsBase = teb;
  report.initial = snapshot(); checkpoint();
  const calls = [
    {id:0x10002,pc:set,returnPc:0x40100f,args:[0xf1234567],retired:5,eax:0,esp:top-16,esi:set,edi:0,result:0,error:0xf1234567,
      writes:[[top-4,0],[top-8,0],[top-12,0xf1234567],[top-16,0x40100f]]},
    {id:0x10001,pc:get,returnPc:0x401017,args:[],retired:2,eax:0,esp:top-12,esi:set,edi:get,result:0xf1234567,error:0xf1234567,
      writes:[[top-12,0x401017]]},
    {id:0x10002,pc:set,returnPc:0x401020,args:[0],retired:3,eax:0xf1234567,esp:top-16,esi:set,edi:get,result:0xf1234567,error:0,
      writes:[[0x403000,0xf1234567],[top-12,0],[top-16,0x401020]]},
    {id:0x10001,pc:get,returnPc:0x401022,args:[],retired:1,eax:0xf1234567,esp:top-12,esi:set,edi:get,result:0,error:0,
      writes:[[top-12,0x401022]]},
  ];
  for (const [index,call] of calls.entries()) {
    stage = `Windows call ${index+1}`; const run = until(7);
    assert.equal(run.reason,8); assert.equal(run.detail,call.id); assert.equal(run.retired,call.retired); assert.equal(run.work,call.retired);
    for (const [address,value] of call.writes) patch(address,value);
    checkState([call.eax,0,0,0,call.esp,0,call.esi,call.edi],call.pc);
    checkExit(15,8,call.retired,call.id);
    engine.ok('core_capture_windows_call'); const raw = Buffer.from(engine.take(112)), d = view(raw);
    assert.equal(raw.subarray(0,4).toString(),'R3CF'); assert.equal(d.getUint32(4,true),0x10001); assert.equal(d.getUint32(8,true),112);
    const token = d.getUint32(16,true); assert(token > 0); assert.equal(d.getUint32(20,true),call.id);
    assert.equal(d.getUint32(24,true),2); assert.equal(d.getUint32(28,true),call.args.length);
    assert.equal(d.getUint32(32,true),call.pc); assert.equal(d.getUint32(36,true),call.esp); assert.equal(d.getUint32(40,true),call.returnPc);
    assert.deepEqual(call.args.map((_,i) => d.getUint32(48+i*4,true)),call.args);
    if (index === 0) {neutral('pending call export','core_export',12,7,0); neutral('pending call import','core_import',12);}
    engine.ok('core_complete_windows_call',token); patch(teb+0x34,call.error);
    checkState([call.result,0,0,0,top-8,0,call.esi,call.edi],call.returnPc);
    checkExit(3,3,0);
    report.calls.push({index:index+1,id:call.id,token,args:call.args,rawFrameHex:raw.toString('hex'),retired:run.retired,after:snapshot()}); checkpoint();
    cache.import();
  }
  stage = 'bounded final guest loop'; const final = until(2);
  patch(0x403004,0); patch(0x403008,0x52494e47);
  assert.equal(final.retired,16); assert.equal(final.work,16); assert.equal(final.reason,1);
  checkState([0,0,0,0,top-8,0,set,get],0x401031);
  checkExit(15,1,16);
  assert.deepEqual(Array.from({length:3},(_,i) => ram(0x403000+i*4,4).readUInt32LE()),[0xf1234567,0,0x52494e47]);
  assert.equal(cache.stats().retired,27); assert.equal(cache.stats().work,27); assert.equal(report.calls.length,4);
  assert.equal(word(cache.context,40),27); assert.equal(word(cache.context,44),0);
  for (const offset of [48,52,112,116]) assert.equal(word(cache.context,offset),0);
  assert(Object.values(report.forbiddenCalls).every(count => count === 0));
  assert.deepEqual(sources(),report.sources,'invoked source owners unchanged');
  report.stats = cache.stats(); report.final = snapshot(); report.complete = true;
} catch (error) {
  report.failure = {stage,message:error.message,stack:error.stack,...(engine ? snapshot() : {})}; checkpoint(); throw error;
} finally {
  if (engine) {if (cache?.open) engine.api.core_end(); const status = engine.status('close'); report.closeStatus = status;
    if (status !== 0) report.complete = false;}
  checkpoint();
}
assert.equal(report.closeStatus,0,'native instance closed');
console.log(JSON.stringify({complete:report.complete,engineSha256:artifact.sha256,calls:report.calls.length,
  runs:report.runs.length,retired:report.stats.retired,work:report.stats.work}));
