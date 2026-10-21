import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync,writeFileSync,openSync,closeSync} from 'node:fs';
import {performance} from 'node:perf_hooks';
import {readEngine} from '../../../support/wasm/engine.mjs';
import {Engine,KEY,record,view} from '../../../../../sdk/runtime/engine.js';
import {CodeCache} from '../../../../../sdk/runtime/cache.js';

const [baselinePath,baselineHash,candidatePath,candidateHash,reportPath]=process.argv.slice(2);
assert(reportPath,'usage: mixed-timing.mjs baseline.wasm baseline-sha256 candidate.wasm candidate-sha256 fresh-report.json');
for(const value of [baselineHash,candidateHash])assert.match(value,/^[a-f0-9]{64}$/);
const artifacts=[readEngine(baselinePath,baselineHash),readEngine(candidatePath,candidateHash)];
closeSync(openSync(reportPath,'wx'));
const count=1024,totalWork=6146,hash=bytes=>createHash('sha256').update(bytes).digest('hex');
const hex=bytes=>Buffer.from(bytes).toString('hex');
const caller=new Uint8Array(4096),data=new Uint8Array(4096),stack=new Uint8Array(4096);
caller.set([0xbe,0,0x50,0,0,0xb9,0,4,0,0,0xff,0xd6,0x83,0xc6,8,0x49,0x75,0xf8,0x0f,0x0b]);
const callees=new Uint8Array(8192).fill(0x90);
for(let index=0;index<count;index++)callees.set([0x40,0xc3],index*8);
const pages=[[0x1000,caller,7],[0x3000,data,3],[0x4000,stack,3],
  [0x5000,callees.subarray(0,4096),7],[0x6000,callees.subarray(4096),7]];
const initialRegs=[0,0,7,0x3000,0x4800,11,0,13],finalRegs=[1024,0,7,0x3000,0x4800,11,0x7000,13];
const expectedRam=new Uint8Array(16*4096);
pages.forEach(([,page],index)=>expectedRam.set(page,index*4096));
view(expectedRam).setUint32(8192+0x7fc,0x100c,true);
const sources=['./mixed-timing.mjs','../../../support/wasm/engine.mjs',
  '../../../../../sdk/runtime/engine.js','../../../../../sdk/runtime/engine/module.js','../../../../../sdk/runtime/cache.js'];
const report={schema:1,complete:false,node:process.version,warmupsPerArm:2,rounds:8,
  scope:'same authored mixed compiled caller and1024 distinct cold callees; ordinary SDK execution and handoff cost, not game or exclusive CPU time',
  clock:'node performance.now elapsed milliseconds; initial selection/reset and readback excluded',
  guest:{count,retiredAndWork:totalWork,entry:0x1000,finalPc:0x1012,flags:0x46,finalRegs,
    callerSha256:hash(caller),calleePagesSha256:hash(callees),expectedRamSha256:hash(expectedRam),
    formation:{instructions:64,blocks:8},visitsPerCalleePerArm:18,calleeStride:8,firstCallee:0x5000,lastCallee:0x6ff8},
  sources:Object.fromEntries(sources.map(path=>[path,hash(readFileSync(new URL(path,import.meta.url)))])),arms:[],runs:[]};
const checkpoint=()=>writeFileSync(reportPath,JSON.stringify(report,null,2));
const statsFields=['runs','regions','retired','work','coldRuns','coldWork','coldRetired',
  'chainedColdRuns','chainedColdWork','chainedColdRetired','admissions','generatedModuleBytes','prunedNative','prunedNativeBytes'];
let stage='initialization',active,referenceArena;
const contexts=[];
checkpoint();

function fresh(index) {
  const artifact=artifacts[index],name=index?'candidate':'baseline';
  const imports=WebAssembly.Module.imports(artifact.module);
  assert(imports.length===0||JSON.stringify(imports)===JSON.stringify([{module:'ring3_observability',name:'now_ns',kind:'function'}]),'closed native import set');
  const instance=new WebAssembly.Instance(artifact.module,{ring3_observability:{now_ns:()=>0n}});
  const api=Object.freeze(Object.fromEntries(Object.entries(instance.exports).filter(([key])=>key.startsWith('ring3_abi_v1_'))
    .map(([key,value])=>[key.slice(13),value])));
  const engine=new Engine(api,instance.exports.memory,artifact.sha256);
  const bytes=(pointer,length)=>new Uint8Array(engine.memory.buffer).slice(pointer,pointer+length);
  let opened=false;
  try {
    engine.ok('open',16,...KEY);opened=true;engine.base=api.arena_ptr()>>>0;
    for(const [address,page,permissions] of pages){engine.ok('map',address,1,permissions);engine.request(page);engine.ok('upload',address,page.length);}
    const initialState=record('R3ST',56),s=view(initialState);
    initialRegs.forEach((value,i)=>s.setUint32(16+i*4,value,true));s.setUint32(48,0x1000,true);s.setUint32(52,2,true);
    new Uint8Array(engine.memory.buffer,engine.base,56).set(initialState);
    const cache=new CodeCache(engine,{chain:true,budget:totalWork,instructions:64,blocks:8});
    for(const key of ['core_read','core_write','core_numeric','core_chain_next','core_cold_run'])assert.equal(cache.native[key],api[key],`original ${key}`);
    assert.equal(typeof api.core_cold_run,'function','both arms require the ordinary cold tier');
    const unit=cache.entry(0x1000),encoded=bytes(api.core_code_ptr()>>>0,api.core_code_len()>>>0);
    const driver=bytes(api.core_dispatch_code_ptr()>>>0,api.core_dispatch_code_len()>>>0);
    engine.ok('core_begin');engine.ok('core_end');
    const result={name,engineSha256:artifact.sha256,engineBytes:artifact.bytes.length,chainedColdEnabled:cache.chainedColdEnabled,
      caller:{id:String(unit.id),pc:unit.pc,bytes:encoded.length,sha256:hash(encoded)},
      driver:{bytes:driver.length,sha256:hash(driver)}};
    report.arms.push(result);assert.equal(cache.stats().admissions,1);assert.equal(cache.units.size,1);
    return {name,engine,api,cache,bytes,initialState,unit,result};
  } catch(error){if(opened)engine.ok('close');throw error;}
}

function run(context,phase,round,position) {
  const {name,engine,api,cache,bytes,unit}=context;active=context;stage=`${phase}/${round}/${position}/${name}`;
  for(const [address,page] of [[0x3000,data],[0x4000,stack]]){engine.request(page);engine.ok('upload',address,page.length);}
  new Uint8Array(engine.memory.buffer,engine.base,56).set(context.initialState);
  cache.import();let selected=cache.entry(0x1000);
  engine.ok('core_begin');engine.ok('core_end');
  const beforeContext=bytes(cache.context,128),beforeArena=bytes(engine.base,api.arena_len()),before=cache.stats();
  const priorRetired=view(beforeContext).getUint32(40,true);
  let work=0,retired=0,outer=0,last;
  const started=performance.now();
  while(work<totalWork) {
    if(outer>=2050)throw new Error('authored mixed dispatch bound exceeded');
    cache.budget=totalWork-work;last=cache.execute(selected);outer++;work+=last.work;retired+=last.retired;
    if(last.coreReason===2)break;
    if(last.coreReason!==1)throw new Error(`unexpected authored stop ${last.coreReason}`);
    selected=cache.select();
  }
  const elapsedMs=performance.now()-started;
  const raw=bytes(cache.context,128),d=view(raw),arena=bytes(engine.base,api.arena_len());
  assert.equal(d.getUint32(68,true),16,'bounded complete backing');
  const ram=bytes(d.getUint32(64,true),16*4096),after=cache.stats();
  const delta=Object.fromEntries(statsFields.map(key=>[key,(after[key]??0)-(before[key]??0)]));
  const sample={arm:name,phase,round,position,elapsedMs,outer,stop:last?.coreReason,work,retired,statsDelta:delta,
    contextHex:hex(raw),arenaSha256:hash(arena),ramSha256:hash(ram)};
  report.runs.push(sample);report.lastFailedRaw={contextHex:hex(raw),arenaHex:hex(arena),ramHex:hex(ram)};checkpoint();
  assert.equal(last.coreReason,2);assert.equal(work,totalWork);assert.equal(retired,totalWork);
  const expectedContext=beforeContext.slice(),c=view(expectedContext);
  finalRegs.forEach((value,i)=>c.setUint32(i*4,value,true));
  for(const [offset,value] of [[32,0x1012],[36,0x46],[40,(priorRetired+totalWork)>>>0],[44,0],[48,0],[52,0],[112,0],[116,0]])c.setUint32(offset,value,true);
  c.setBigUint64(120,unit.id,true);assert.deepEqual(raw,expectedContext,'literal entire context with its own stable metadata');
  const expectedState=record('R3ST',56),s=view(expectedState);
  finalRegs.forEach((value,i)=>s.setUint32(16+i*4,value,true));s.setUint32(48,0x1012,true);s.setUint32(52,0x46,true);
  const expectedExit=record('R3EX',40),e=view(expectedExit);e.setUint16(4,15,true);e.setUint32(16,1,true);e.setUint32(20,totalWork,true);
  const expectedWork=record('R3WK',24);view(expectedWork).setUint32(16,totalWork,true);
  const expectedArena=beforeArena.slice();expectedArena.set(expectedState);expectedArena.set(expectedExit,56);expectedArena.set(expectedWork,4588);
  assert.deepEqual(arena,expectedArena,'literal State/Exit/Work; complete FP128/SIMD/segments/YMM/IO unchanged');
  const memoryView=new DataView(engine.memory.buffer),mapping=d.getUint32(56,true);
  assert.deepEqual([1,3,4,5,6].map(page=>memoryView.getUint32(mapping+page*4,true)),[1,2,3,4,5]);
  assert.deepEqual(ram,expectedRam,'literal entire16-page backing including both callee pages and untouched data');
  referenceArena??=arena;assert.deepEqual(arena,referenceArena,'whole canonical arena parity between artifacts and all runs');
  assert.equal(delta.work,totalWork);assert.equal(delta.retired,totalWork);assert.equal(delta.regions,1025);
  assert.equal(delta.admissions,0);assert.equal(delta.generatedModuleBytes,0);assert.equal(delta.prunedNative,0);assert.equal(delta.prunedNativeBytes,0);
  assert.equal(cache.units.size,1);assert.equal(cache.units.get(unit.id),unit);
  if(cache.chainedColdEnabled) {
    assert.equal(outer,1);assert.equal(delta.runs,1);assert.equal(delta.coldRuns,0);
    assert.deepEqual([delta.chainedColdRuns,delta.chainedColdWork,delta.chainedColdRetired],[1024,2048,2048]);
  } else {
    assert.equal(outer,2049);assert.equal(delta.runs,1025);assert.equal(delta.coldRuns,1024);
    assert.deepEqual([delta.chainedColdRuns,delta.chainedColdWork,delta.chainedColdRetired],[0,0,0]);
  }
  assert.equal(delta.coldWork+delta.chainedColdWork,2048);assert.equal(delta.coldRetired+delta.chainedColdRetired,2048);
  assert.equal(outer,delta.runs+delta.coldRuns);
  context.result.arenaHex??=hex(arena);sample.complete=true;delete report.lastFailedRaw;checkpoint();
}

const median=values=>{const a=values.toSorted((x,y)=>x-y);return(a[a.length/2-1]+a[a.length/2])/2;};
try {
  contexts.push(fresh(0));contexts.push(fresh(1));
  assert.deepEqual(report.arms[0].caller,report.arms[1].caller,'identical compiled caller module');
  if(baselineHash!==candidateHash) {
    assert.equal(contexts[0].cache.chainedColdEnabled,false,'baseline uses the separate cold handoff');
    assert.equal(contexts[1].cache.chainedColdEnabled,true,'candidate has the mixed dispatcher');
  }
  for(let round=0;round<2;round++)for(let arm=0;arm<2;arm++)run(contexts[arm],'warmup',round,arm);
  for(let round=0;round<8;round++)(round%2?[1,0,0,1]:[0,1,1,0]).forEach((arm,position)=>run(contexts[arm],'timed',round,position));
  const elapsed=name=>report.runs.filter(row=>row.phase==='timed'&&row.arm===name).map(row=>row.elapsedMs);
  report.summary={baselineMedianMs:median(elapsed('baseline')),candidateMedianMs:median(elapsed('candidate'))};
  if(baselineHash!==candidateHash)report.summary.ratio=report.summary.baselineMedianMs/report.summary.candidateMedianMs;
  else report.summary.control='same artifact; no optimization ratio';
  report.complete=true;checkpoint();console.log(JSON.stringify({complete:true,...report.summary}));
} catch(error) {
  report.failure={stage,message:String(error),stack:error.stack};
  if(active?.cache.context)try {
    const {cache,engine,api,bytes}=active,raw=bytes(cache.context,128),d=view(raw);
    assert.equal(d.getUint32(68,true),16,'bounded failure snapshot');
    report.lastFailedRaw={contextHex:hex(raw),arenaHex:hex(bytes(engine.base,api.arena_len())),ramHex:hex(bytes(d.getUint32(64,true),16*4096))};
  } catch(snapshotError){report.failure.snapshotError=String(snapshotError);}
  checkpoint();throw error;
} finally {for(const {cache,api,engine} of contexts){if(cache.open)api.core_end();engine.ok('close');}}
