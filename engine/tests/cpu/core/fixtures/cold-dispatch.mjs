import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync,writeFileSync,openSync,closeSync} from 'node:fs';
import {readEngine} from '../../../support/wasm/engine.mjs';
import {Engine,KEY,record,view} from '../../../../../sdk/runtime/engine.js';
import {CodeCache} from '../../../../../sdk/runtime/cache.js';

const [enginePath,expectedSha256,reportPath]=process.argv.slice(2);
assert(enginePath&&expectedSha256&&reportPath,'usage: cold-dispatch.mjs engine.wasm expected-sha256 fresh-report.json');
const baselineSha256='84330407d3c5ec3407c4f118c44c6d84b3fe315710edf0e9b4c486b0a86365a7';
const artifact=readEngine(enginePath,expectedSha256),hash=bytes=>createHash('sha256').update(bytes).digest('hex');
const hex=bytes=>Buffer.from(bytes).toString('hex');
const counts=[1,32,128],rounds=10,callsPerBlock=100;
const sources=['./cold-dispatch.mjs','../../../support/wasm/engine.mjs','../../../support/wasm/exports.mjs',
  '../../../../../sdk/runtime/engine.js','../../../../../sdk/runtime/engine/module.js','../../../../../sdk/runtime/cache.js'];
const report={schema:1,complete:false,engineSha256:artifact.sha256,engineBytes:artifact.bytes.length,baselineSha256,
  sources:Object.fromEntries(sources.map(path=>[path,hash(readFileSync(new URL(path,import.meta.url)))])),
  scope:'closed native cold dispatch at an already retained entry; no guest instruction executes',
  timingSemantics:'performance.now milliseconds for the JavaScript loop, return recording and original closed native calls; includes admission and lookup/snapshot work; no exclusive frontier or CPU-time attribution',
  limits:['no guest DLL gates, so game module-gate copying/sorting is outside this workload',
    'one untimed frontier call before measurement; no claim that base-engine lazy or tier compilation has finished',
    'separate artifact runs are descriptive measurements, not an interleaved old/new speed claim'],
  rounds,callsPerBlock,callsPerRetainedCount:rounds*callsPerBlock,fixtures:[],blocks:[]};
closeSync(openSync(reportPath,'wx'));
const checkpoint=()=>writeFileSync(reportPath,JSON.stringify(report,null,2));
const fixtures=[];let stage='initialization';checkpoint();

function prepare(count) {
  stage=`prepare ${count}`;
  const result={retainedCount:count,modules:[],controls:[],complete:false};report.fixtures.push(result);checkpoint();
  const instance=new WebAssembly.Instance(artifact.module,{ring3_observability:{now_ns:()=>0n}});
  const api=Object.freeze(Object.fromEntries(Object.entries(instance.exports).filter(([name])=>name.startsWith('ring3_abi_v1_'))
    .map(([name,value])=>[name.slice(13),value])));
  const engine=new Engine(api,instance.exports.memory,artifact.sha256);
  const bytes=(pointer,length)=>new Uint8Array(engine.memory.buffer).slice(pointer,pointer+length);
  const word=(pointer,offset=0)=>new DataView(engine.memory.buffer).getUint32(pointer+offset,true);
  let opened=false,context=0,backing=0;
  const code=new Uint8Array(4096),data=new Uint8Array(4096).fill(0x5a),stack=new Uint8Array(4096).fill(0xa5);
  for(let index=0;index<128;index++)code.set([0x83,0xc0,1,0x0f,0x0b],index*8);
  const expectedRam=new Uint8Array(16*4096);expectedRam.set(code);expectedRam.set(data,4096);expectedRam.set(stack,8192);
  const snapshot=()=>({context:bytes(context,128),arena:bytes(engine.base,api.arena_len()),ram:bytes(backing,16*4096)});
  const compact=raw=>({contextHex:hex(raw.context),arenaSha256:hash(raw.arena),ramSha256:hash(raw.ram)});
  try {
    assert.equal(typeof api.core_cold_run,'function','actual signed cold export is required');
    engine.ok('open',16,...KEY);opened=true;engine.base=api.arena_ptr()>>>0;
    for(const [address,page] of [[0x1000,code],[0x3000,data],[0x4000,stack]]) {
      engine.ok('map',address,1,address===0x1000?7:3);engine.request(page);engine.ok('upload',address,4096);
    }
    const state=record('R3ST',56),s=view(state),registers=[5,6,7,0x3000,0x4800,8,9,10];
    registers.forEach((value,index)=>s.setUint32(16+index*4,value,true));s.setUint32(48,0x1000,true);s.setUint32(52,0x403,true);
    new Uint8Array(engine.memory.buffer,engine.base,56).set(state);
    const expectedArena=bytes(engine.base,api.arena_len());
    const cache=new CodeCache(engine,{chain:false,instructions:64,blocks:8,capacity:1024,budget:1});
    for(let index=0;index<count;index++) {
      const pc=0x1000+index*8,unit=cache.entry(pc);
      const encoded=bytes(api.core_code_ptr()>>>0,api.core_code_len()>>>0);
      result.modules.push({pc,id:String(unit.id),bytes:encoded.length,sha256:hash(encoded)});
    }
    assert.equal(new Set(result.modules.map(module=>module.id)).size,count);
    assert.equal(cache.stats().admissions,count);assert.equal(cache.stats().installedUnits,count);
    const zero=api.core_cold_run(0);context=api.core_context_ptr()>>>0;assert(context>0&&context%8===0);
    backing=word(context,64);assert(backing>0);assert.equal(word(context,68),16);
    assert.deepEqual([1,3,4].map(page=>word(word(context,56)+page*4)),[1,2,3]);
    assert.equal(zero,2);assert.equal(word(context,40),0);assert.equal(word(context,44),0);assert.equal(word(context,112),0);
    assert.equal(view(bytes(context,128)).getBigUint64(120,true),0n);
    assert.equal(api.core_code_ptr(),0);assert.equal(api.core_code_len(),0);
    const zeroRaw=snapshot();result.controls.push({budget:0,stop:zero,...compact(zeroRaw)});
    const initialContext=zeroRaw.context.slice(),initialView=view(initialContext);
    assert.deepEqual(Array.from({length:8},(_,index)=>initialView.getUint32(index*4,true)),registers);
    assert.equal(initialView.getUint32(32,true),0x1000);assert.equal(initialView.getUint32(36,true),0x403);
    assert.deepEqual(zeroRaw.arena,expectedArena);assert.deepEqual(zeroRaw.ram,expectedRam);
    const first=api.core_cold_run(1),expectedContext=initialContext.slice();view(expectedContext).setUint32(44,1,true);
    const warmRaw=snapshot();result.controls.push({budget:1,stop:first,...compact(warmRaw)});checkpoint();
    assert.equal(first,1,'retained entry yields before decoding or retiring its ADD');
    function validate(raw) {
      assert.deepEqual(raw.context,expectedContext,'full context: same PC, zero retired, one remaining, closed and unselected');
      assert.deepEqual(raw.arena,expectedArena,'whole canonical arena remains unpublished and unchanged');
      assert.deepEqual(raw.ram,expectedRam,'literal whole canonical code/data/stack/unused backing');
    }
    validate(warmRaw);
    const statusPointer=api.core_cache_status_ptr()>>>0;assert(statusPointer>0&&statusPointer+64<=engine.memory.buffer.byteLength);
    const status=view(bytes(statusPointer,64));
    result.nativeCache=Object.fromEntries(['compiled','admittedEncodedBytes','countEvictions','byteEvictions','sourceInvalidations',
      'retainedUnits','retainedEncodedBytes','unitCapacity'].map((name,index)=>[name,status.getBigUint64(index*8,true).toString()]));
    assert.equal(result.nativeCache.compiled,String(count));assert.equal(result.nativeCache.retainedUnits,String(count));
    for(const name of ['countEvictions','byteEvictions','sourceInvalidations'])assert.equal(result.nativeCache[name],'0');
    validate(snapshot());
    result.initial={...compact(warmRaw),arenaHex:hex(warmRaw.arena),ramHex:hex(warmRaw.ram)};checkpoint();
    const cold=api.core_cold_run;
    return {count,result,close(){if(opened){engine.ok('close');opened=false;}},block(round){
      assert.equal(api.core_cold_run,cold,'original native cold function identity');
      const before=snapshot();validate(before);
      const statuses=new Int32Array(callsPerBlock),start=performance.now();
      for(let index=0;index<callsPerBlock;index++)statuses[index]=cold(1);
      const elapsedMs=performance.now()-start,after=snapshot();
      report.blocks.push({round,retainedCount:count,calls:callsPerBlock,elapsedMs,statuses:[...statuses],
        before:compact(before),after:compact(after)});checkpoint();
      assert(Number.isFinite(elapsedMs)&&elapsedMs>=0);assert(statuses.every(status=>status===1));validate(after);
    }};
  } catch(error) {
    result.failure={stage,message:String(error),stack:error.stack,...(context&&backing?compact(snapshot()):{})};checkpoint();
    if(opened)engine.ok('close');throw error;
  }
}

try {
  for(const count of counts)fixtures.push(prepare(count));
  for(let round=0;round<rounds;round++)for(const fixture of (round%2?[...fixtures].reverse():fixtures)) {
    stage=`round ${round} retained ${fixture.count}`;fixture.block(round);
  }
  for(const fixture of fixtures)fixture.result.complete=true;
  report.summary=counts.map(count=>{const samples=report.blocks.filter(block=>block.retainedCount===count).map(block=>block.elapsedMs);
    return {retainedCount:count,calls:samples.length*callsPerBlock,totalMs:samples.reduce((sum,value)=>sum+value,0),blockMs:samples};});
  report.complete=true;
} catch(error) {report.failure={stage,message:String(error),stack:error.stack};throw error;}
finally {
  for(const fixture of fixtures)try{fixture.close();}catch(error){report.complete=false;(report.cleanupErrors??=[]).push(String(error));}
  checkpoint();
}
assert.equal(report.complete,true);
console.log(JSON.stringify({complete:true,engineSha256:artifact.sha256,summary:report.summary}));
