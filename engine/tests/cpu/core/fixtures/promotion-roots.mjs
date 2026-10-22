import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync,writeFileSync,openSync,closeSync} from 'node:fs';
import {readEngine} from '../../../support/wasm/engine.mjs';
import {Engine,KEY,record,view} from '../../../../../sdk/runtime/engine.js';
import {CodeCache} from '../../../../../sdk/runtime/cache.js';

const [enginePath,reportPath] = process.argv.slice(2);
assert(enginePath && reportPath,'usage: promotion-roots.mjs engine.wasm fresh-report.json');
const artifact=readEngine(enginePath);closeSync(openSync(reportPath,'wx'));
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
const hex=bytes=>Buffer.from(bytes).toString('hex');
const roots=10,iterations=50000,totalWork=roots*3*iterations;
const pcs=Array.from({length:roots},(_,index)=>0x1000+index*32);
const code=new Uint8Array(4096);
for(let index=0;index<roots;index++) {
  const offset=index*32;
  code.set([0x40,0xbe,0,0,0,0,0xff,0xe6],offset);
  view(code).setUint32(offset+2,pcs[(index+1)%roots],true);
}
const sources=['./promotion-roots.mjs','../../../support/wasm/engine.mjs',
  '../../../../../sdk/runtime/engine.js','../../../../../sdk/runtime/cache.js'];
const report={schema:1,complete:false,engineSha256:artifact.sha256,roots,iterations,totalWork,
  scope:'normal observed transitions must form more than eight roots within configured budgets; literal architectural closure, no timing or game claim',
  codeSha256:hash(code),sources:Object.fromEntries(sources.map(path=>[path,hash(readFileSync(new URL(path,import.meta.url)))])),
  initialOwners:[],runs:[],selections:[]};
const checkpoint=()=>writeFileSync(reportPath,JSON.stringify(report,null,2));
let stage='initialization',opened=false,cache,engine,api;
checkpoint();
try {
  const instance=new WebAssembly.Instance(artifact.module,{ring3_observability:{now_ns:()=>0n}});
  api=Object.freeze(Object.fromEntries(Object.entries(instance.exports).filter(([name])=>name.startsWith('ring3_abi_v1_'))
    .map(([name,value])=>[name.slice(13),value])));
  engine=new Engine(api,instance.exports.memory,artifact.sha256);
  const bytes=(pointer,length)=>new Uint8Array(engine.memory.buffer).slice(pointer,pointer+length);
  const word=(pointer,offset=0)=>new DataView(engine.memory.buffer).getUint32(pointer+offset,true);
  engine.ok('open',16,...KEY);opened=true;engine.base=api.arena_ptr()>>>0;
  const pages=[[0x1000,code,7],[0x3000,new Uint8Array(4096),3],[0x4000,new Uint8Array(4096),3]];
  for(const [address,page,permissions]of pages) {
    engine.ok('map',address,1,permissions);engine.request(page);engine.ok('upload',address,4096);
  }
  const initialRegisters=[5,0x12345,7,0x3000,0x4800,0x2468ace0,0,0x10203040];
  const state=record('R3ST',56),s=view(state);
  initialRegisters.forEach((value,index)=>s.setUint32(16+index*4,value,true));
  s.setUint32(48,0x1000,true);s.setUint32(52,0x402,true);
  new Uint8Array(engine.memory.buffer,engine.base,56).set(state);
  const fp=new Uint8Array(engine.memory.buffer,engine.base+4236,128),f=view(fp);
  f.setUint16(16,0x037f,true);f.setUint16(18,0,true);f.setUint16(20,0xfffc,true);
  f.setBigUint64(40,0xa000000000000000n,true);f.setUint16(48,0x3fff,true);
  const simd=new Uint8Array(engine.memory.buffer,engine.base+4364,160);
  for(let i=0;i<128;i++)simd[32+i]=(i*13+7)&255;
  cache=new CodeCache(engine,{chain:true,budget:totalWork,instructions:256,blocks:32});
  for(const [name,value]of Object.entries(cache.native))assert.equal(value,api[name],`original native ${name}`);
  let selected;
  for(const pc of pcs) {
    const unit=cache.entry(pc);selected??=unit;
    report.initialOwners.push({pc,id:String(unit.id),slot:unit.slot});
  }
  assert.equal(new Set(report.initialOwners.map(unit=>unit.id)).size,roots,'ten genuinely separate initial compiled owners');
  assert.equal(cache.entry(0x1000),selected);
  engine.ok('core_begin');engine.ok('core_end');
  const initialContext=bytes(cache.context,128),initialArena=bytes(engine.base,api.arena_len());
  const backing=word(cache.context,64);assert.equal(word(cache.context,68),16);
  assert.deepEqual([1,3,4].map(page=>word(word(cache.context,56)+page*4)),[1,2,3]);
  const expectedRam=new Uint8Array(16*4096);expectedRam.set(code);
  let retired=0,work=0,publishedRetired=0,publishedWork=0,last;
  stage='observed-promotion';
  while(work<totalWork) {
    assert(report.runs.length<256,'bounded genuine promotion continuations');
    cache.budget=totalWork-work;
    last=cache.execute(selected);retired+=last.retired;work+=last.work;
    report.runs.push({...last,id:String(last.id)});checkpoint();
    if(last.coreReason===2)break;
    assert.equal(last.coreReason,1,'only genuine promotion may stop before the fixed work budget');
    assert(last.work>0);assert.equal(last.chainedColdRuns,0);
    const before=bytes(cache.context,48);
    cache.publish();publishedRetired=retired;publishedWork=work;cache.import();
    assert.deepEqual(bytes(cache.context,48),before,'scheduler-only import preserves every guest CPU word');
    selected=cache.entry();
  }
  assert.equal(last.coreReason,2);assert.equal(retired,totalWork);assert.equal(work,totalWork);
  const finalRegisters=[5+roots*iterations,0x12345,7,0x3000,0x4800,0x2468ace0,0x1000,0x10203040];
  const expectedContext=initialContext.slice(),c=view(expectedContext);
  finalRegisters.forEach((value,index)=>c.setUint32(index*4,value,true));
  for(const [offset,value]of [[32,0x1000],[36,0x402],[40,totalWork],[44,0],[48,0],[52,0],[112,0],[116,0]])
    c.setUint32(offset,value,true);
  c.setBigUint64(120,last.id,true);
  assert.deepEqual(bytes(cache.context,128),expectedContext,'complete literal guest context and unchanged metadata witnesses');
  const expectedArena=initialArena.slice(),finalState=record('R3ST',56),fs=view(finalState);
  finalRegisters.forEach((value,index)=>fs.setUint32(16+index*4,value,true));
  fs.setUint32(48,0x1000,true);fs.setUint32(52,0x402,true);expectedArena.set(finalState);
  const exit=record('R3EX',40),e=view(exit);e.setUint16(4,15,true);
  e.setUint32(16,1,true);e.setUint32(20,retired-publishedRetired,true);expectedArena.set(exit,56);
  const completedWork=record('R3WK',24);view(completedWork).setUint32(16,work-publishedWork,true);expectedArena.set(completedWork,4588);
  assert.deepEqual(bytes(engine.base,expectedArena.length),expectedArena,'full arena including literal unchanged seeded FP/SIMD and segment/YMM banks');
  assert.deepEqual(bytes(backing,expectedRam.length),expectedRam,'whole unchanged canonical RAM');
  report.final={contextHex:hex(bytes(cache.context,128)),arenaHex:hex(bytes(engine.base,expectedArena.length)),
    ramSha256:hash(bytes(backing,expectedRam.length)),stats:cache.sampleUsage()};checkpoint();
  stage='all-root-index-ownership';
  for(const pc of pcs) {
    const before=bytes(cache.context,120),unit=cache.entry(pc);
    report.selections.push({pc,id:String(unit.id),slot:unit.slot});checkpoint();
    assert.deepEqual(bytes(cache.context,120),before,'closed selection changes only the selected code identity');
    assert.deepEqual(bytes(engine.base,expectedArena.length),expectedArena);
    assert.deepEqual(bytes(backing,expectedRam.length),expectedRam);
  }
  assert.equal(new Set(report.selections.map(unit=>unit.id)).size,1,'all ten roots must be represented by one formed owner');
  assert.equal(report.final.stats.nativeCache.retainedUnits,'1','fully covered predecessors are reclaimed');
  assert.equal(report.final.stats.nativeCache.countEvictions,'0');assert.equal(report.final.stats.nativeCache.byteEvictions,'0');
  assert.equal(report.final.stats.nativeCache.sourceInvalidations,'0');
  assert.equal(cache.stats().coldWork,0);assert.equal(cache.stats().chainedColdWork,0);
  assert(!report.initialOwners.some(unit=>unit.id===report.selections[0].id),'a genuinely new combined owner covers the ring');
  report.complete=true;checkpoint();console.log(JSON.stringify({complete:true,engineSha256:artifact.sha256,
    roots,retired,admissions:cache.stats().admissions,regions:cache.stats().regions,owner:report.selections[0].id}));
}catch(error) {
  report.failure={stage,message:String(error),stack:error.stack};
  if(cache?.context)try {
    const raw=new Uint8Array(engine.memory.buffer).slice(cache.context,cache.context+128),d=view(raw);
    report.failure.contextHex=hex(raw);report.failure.arenaHex=hex(new Uint8Array(engine.memory.buffer).slice(engine.base,engine.base+api.arena_len()));
    report.failure.ramHex=hex(new Uint8Array(engine.memory.buffer).slice(d.getUint32(64,true),d.getUint32(64,true)+16*4096));
  }catch(snapshotError){report.failure.snapshotError=String(snapshotError);}
  checkpoint();throw error;
}finally{if(cache?.open)api.core_end();if(opened)engine.ok('close');}
