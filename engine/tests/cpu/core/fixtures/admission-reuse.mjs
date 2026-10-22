import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync,writeFileSync,openSync,closeSync} from 'node:fs';
import {readEngine} from '../../../support/wasm/engine.mjs';
import {Engine,KEY,record,view} from '../../../../../sdk/runtime/engine.js';
import {CodeCache} from '../../../../../sdk/runtime/cache.js';

const [enginePath,reportPath] = process.argv.slice(2);
assert(enginePath && reportPath,'usage: admission-reuse.mjs engine.wasm fresh-report.json');
const artifact = readEngine(enginePath), output = openSync(reportPath,'wx'); closeSync(output);
const hex = bytes => Buffer.from(bytes).toString('hex');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const sources = ['./admission-reuse.mjs','../../../support/wasm/engine.mjs',
  '../../../../../sdk/runtime/engine.js','../../../../../sdk/runtime/cache.js'];
const report = {schema:1,status:'running',complete:false,engineSha256:artifact.sha256,
  scope:'authored adjacent-PC heat after actual whole-unit admission and native count eviction; no timing or game claim',
  sources:Object.fromEntries(sources.map(path => [path,hash(readFileSync(new URL(path,import.meta.url)))])),runs:[]};
const checkpoint = () => writeFileSync(reportPath,JSON.stringify(report,(_key,value) => typeof value === 'bigint' ? value.toString() : value,2));
let stage = 'initialization', opened = false, cache; checkpoint();
const instance = new WebAssembly.Instance(artifact.module,{ring3_observability:{now_ns:()=>0n}});
const api = Object.freeze(Object.fromEntries(Object.entries(instance.exports).filter(([name]) => name.startsWith('ring3_abi_v1_'))
  .map(([name,value]) => [name.slice(13),value])));
const engine = new Engine(api,instance.exports.memory,artifact.sha256);
const bytes = (pointer,length) => new Uint8Array(engine.memory.buffer).slice(pointer,pointer+length);
const word = (pointer,offset=0) => new DataView(engine.memory.buffer).getUint32(pointer+offset,true);

try {
  engine.ok('open',16,...KEY); opened = true; engine.base = api.arena_ptr()>>>0;
  const a = new Uint8Array(4096), b = new Uint8Array(4096);
  a.set([0x40,0x41,0xeb,0xfc]); b.set([0x42,0xeb,0xfd]);
  const pages = [[0x1000,a,7],[0x2000,b,7],[0x3000,new Uint8Array(4096),3],[0x4000,new Uint8Array(4096),3]];
  for (let page = 0; page < 8; page++) {
    const code = new Uint8Array(4096);
    for (let offset = 0; offset < code.length; offset += 8) code.set([0x90,0x0f,0x0b],offset);
    pages.push([0x10000+page*4096,code,7]);
  }
  for (const [address,page,permissions] of pages) {
    engine.ok('map',address,1,permissions); engine.request(page); engine.ok('upload',address,page.length);
  }
  const initialRegisters = [0,0,0,0x3000,0x4800,0,0,0];
  const state = record('R3ST',56), s = view(state);
  initialRegisters.forEach((value,index) => s.setUint32(16+index*4,value,true));
  s.setUint32(48,0x1000,true); s.setUint32(52,2,true);
  new Uint8Array(engine.memory.buffer,engine.base,state.length).set(state);
  cache = new CodeCache(engine,{chain:true,capacity:1024,instructions:64,blocks:8});
  cache.budget = 0; const empty = cache.execute(null);
  assert.equal(empty.coreReason,2); assert.equal(empty.work,0); assert.equal(empty.retired,0);
  const initialContext = bytes(cache.context,128), initialArena = bytes(engine.base,api.arena_len());
  const backing = word(cache.context,64), expectedRam = new Uint8Array(16*4096);
  assert.equal(word(cache.context,68),16);
  pages.forEach(([address,page],index) => {
    assert.equal(word(word(cache.context,56)+(address>>>12)*4),index+1);
    expectedRam.set(page,index*4096);
  });
  const expectedRegisters = initialRegisters.slice();
  let totalRetired = 0, totalWork = 0;
  function setPc(pc) {
    cache.publish();
    new DataView(engine.memory.buffer).setUint32(engine.base+48,pc,true);
    cache.import();
  }
  function cold(name,budget,{stop,pc,retired,flags}) {
    stage = name; cache.budget = budget;
    const run = cache.execute(null);
    report.runs.push({...run,id:run.id===null?null:String(run.id),stage,budget}); checkpoint();
    assert.equal(run.coreReason,stop,`${name}: stop`);
    assert.equal(run.retired,retired,`${name}: retirement`);
    assert.equal(run.work,retired,`${name}: work`);
    assert.equal(run.id,null); assert.equal(run.regions,0);
    totalRetired += retired; totalWork += retired;
    const wanted = initialContext.slice(), c = view(wanted);
    expectedRegisters.forEach((value,index) => c.setUint32(index*4,value,true));
    for (const [offset,value] of [[32,pc],[36,flags],[40,totalRetired],[44,budget-retired],[48,0],[52,0],[112,0],[116,0]])
      c.setUint32(offset,value,true);
    assert.deepEqual(bytes(cache.context,128),wanted,`${name}: full private context and source witnesses`);
    assert.deepEqual(bytes(backing,expectedRam.length),expectedRam,`${name}: entire canonical RAM`);
    assert.deepEqual(bytes(engine.base+4236,352),initialArena.slice(4236,4588),`${name}: FP128/SIMD/segments`);
    assert.deepEqual(bytes(engine.base+4612,184),initialArena.slice(4612,4796),`${name}: YMM and segment bases`);
    return run;
  }
  expectedRegisters[0] = 64; expectedRegisters[1] = 64;
  cold('warm adjacent A instructions',192,{stop:2,pc:0x1000,retired:192,flags:0x12});
  setPc(0x2000); expectedRegisters[2] = 64;
  cold('warm unrelated B instructions',128,{stop:2,pc:0x2000,retired:128,flags:0x12});
  setPc(0x1000);
  cold('request hot A entry',1,{stop:1,pc:0x1000,retired:0,flags:0x12});
  const admitted = cache.select();
  assert(admitted && admitted.pc === 0x1000);
  const nativeCapacity = Number(cache.sampleUsage().nativeCache.unitCapacity);
  assert(nativeCapacity>=1024 && nativeCapacity<=4096,'bounded native region capacity');
  stage = 'force actual native owner eviction';
  for (let index = 0; index < nativeCapacity; index++) engine.ok('core_prepare',0x10000+8*index,1,1);
  const native = cache.sampleUsage().nativeCache;
  const owners = api.core_link_owners_ptr()>>>0;
  assert.equal(new DataView(engine.memory.buffer).getBigUint64(owners+admitted.slot*8,true),0n,'the admitted native owner was evicted');
  assert.equal(native.compiled,String(nativeCapacity+1));
  assert.equal(native.countEvictions,'1'); assert.equal(native.byteEvictions,'0'); assert.equal(native.sourceInvalidations,'0');
  cache.pruneNative(); assert(!cache.units.has(admitted.id));
  report.eviction = {nativeCapacity,admittedIdentity:String(admitted.id),slot:admitted.slot,native}; checkpoint();
  setPc(0x2000);
  cold('unrelated B history survives A admission and eviction',1,{stop:1,pc:0x2000,retired:0,flags:0x12});
  for (let fresh = 1; fresh <= 64; fresh++) {
    setPc(0x1001); expectedRegisters[1]++;
    const result = expectedRegisters[1]>>>0;
    const parity = ((result&255).toString(2).replaceAll('0','').length&1) === 0;
    const flags = 2 | (parity?4:0) | ((result&15)===0?0x10:0);
    cold(`fresh interior INC ${fresh}`,1,{stop:2,pc:0x1002,retired:1,flags});
  }
  setPc(0x1001);
  cold('only 64 fresh retirements requalify the interior PC',1,{stop:1,pc:0x1001,retired:0,flags:0x12});
  cache.publish();
  const wantedArena = initialArena.slice(), wantedState = record('R3ST',56), ws = view(wantedState);
  expectedRegisters.forEach((value,index) => ws.setUint32(16+index*4,value,true));
  ws.setUint32(48,0x1001,true); ws.setUint32(52,0x12,true); wantedArena.set(wantedState);
  const exit = record('R3EX',40), ex = view(exit); ex.setUint16(4,15,true); ex.setUint32(16,3,true);
  wantedArena.set(exit,56); wantedArena.set(record('R3WK',24),4588);
  assert.deepEqual(bytes(engine.base,wantedArena.length),wantedArena,'entire canonical arena with literal final publication');
  assert.equal(cache.stats().retired,384); assert.equal(cache.stats().work,384);
  report.final = {contextHex:hex(bytes(cache.context,128)),arenaSha256:hash(wantedArena),ramSha256:hash(expectedRam),stats:cache.stats()};
  report.status = 'pass'; report.complete = true; checkpoint(); console.log(JSON.stringify({complete:true,engineSha256:artifact.sha256,nativeCapacity,runs:report.runs.length}));
} catch (error) {
  report.status = 'fail';
  report.failure = {stage,name:error.name,message:error.message,operator:error.operator,
    expected:error.expected,actual:error.actual,engineSha256:artifact.sha256,stack:error.stack,
    contextHex:cache?.context?hex(bytes(cache.context,128)):null,
    arenaHex:opened?hex(bytes(engine.base,api.arena_len())):null,
    backingHex:cache?.context?hex(bytes(word(cache.context,64),word(cache.context,68)*4096)):null};
  checkpoint(); throw error;
} finally {if (cache?.open) api.core_end(); if (opened) engine.ok('close');}
