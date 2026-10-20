import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync,writeFileSync,openSync,closeSync} from 'node:fs';
import {performance} from 'node:perf_hooks';
import {readEngine} from '../../../support/wasm/engine.mjs';
import {Engine,KEY,record,view} from '../../../../../sdk/runtime/engine.js';
import {CodeCache} from '../../../../../sdk/runtime/cache.js';
import {makeKernel} from './kernels.mjs';

const [baselinePath,baselineHash,candidatePath,candidateHash,reportPath,mode='same-page'] = process.argv.slice(2);
assert(reportPath,'usage: transition-timing.mjs baseline.wasm baseline-sha256 candidate.wasm candidate-sha256 fresh-report.json [same-page|cross-page]');
assert(['same-page','cross-page'].includes(mode),'expected same-page or cross-page mode');
const crossPage=mode==='cross-page',sameArtifact=baselineHash===candidateHash;
for(const value of [baselineHash,candidateHash]) assert.match(value,/^[a-f0-9]{64}$/,'explicit engine SHA-256');
const artifacts=[readEngine(baselinePath,baselineHash),readEngine(candidatePath,candidateHash)];
if(!crossPage)assert.notEqual(baselineHash,candidateHash,'two selected engine artifacts');
closeSync(openSync(reportPath,'wx'));
const iterations=131072,kernel=makeKernel('call',iterations);
const calleePc=crossPage?0x5000:0x1040,calleePage=crossPage?new Uint8Array(4096):null;
if(crossPage) {
  calleePage.set(kernel.code.subarray(0x40,0x4a));
  kernel.code.fill(0,0x40,0x4a);
  kernel.code.set([0xe8,0xfb,0x3f,0,0]);
}
const mappedPages=[[0x1000,kernel.code,7],[0x3000,kernel.initialData,3],[0x4000,kernel.initialStack,3],
  ...(crossPage?[[calleePc,calleePage,7]]:[])];
const hash=bytes=>createHash('sha256').update(bytes).digest('hex'),hex=bytes=>Buffer.from(bytes).toString('hex');
const sources=['./transition-timing.mjs','./kernels.mjs','../../../support/wasm/engine.mjs',
  '../../../../../sdk/runtime/engine.js','../../../../../sdk/runtime/cache.js'];
const report={schema:1,complete:false,node:process.version,mode,iterations,warmupsPerArm:2,rounds:8,
  untimedRecencyControlsPerArm:crossPage?1:0,
  artifactComparison:sameArtifact?'same-artifact-control':'paired-artifacts',
  clock:'node performance.now elapsed milliseconds',
  scope:crossPage
    ?'same authored cross-page CALL/RET work and native linked lookup/LRU movement; no fusion, game or per-component CPU attribution'
    :'same authored CALL/RET guest work through original native bindings; execution includes ordinary SDK miss handling; no game or per-component CPU attribution',
  sources:Object.fromEntries(sources.map(path=>[path,hash(readFileSync(new URL(path,import.meta.url)))])),arms:[],runs:[]};
const checkpoint=()=>writeFileSync(reportPath,JSON.stringify(report,null,2));
const bankRanges=[[4236,352],[4612,184]],contexts=[];
let stage='initialization',referenceArena,activeContext;
checkpoint();

function fresh(index) {
  const artifact=artifacts[index],name=index?'candidate':'baseline',result={name,engineSha256:artifact.sha256,modules:[]};
  report.arms.push(result);
  const instance=new WebAssembly.Instance(artifact.module,{ring3_observability:{now_ns:()=>0n}});
  const api=Object.freeze(Object.fromEntries(Object.entries(instance.exports).filter(([name])=>name.startsWith('ring3_abi_v1_'))
    .map(([name,value])=>[name.slice(13),value])));
  const engine=new Engine(api,instance.exports.memory,artifact.sha256);
  const bytes=(pointer,length)=>new Uint8Array(engine.memory.buffer).slice(pointer,pointer+length);
  let opened=false;
  try {
    engine.ok('open',16,...KEY);opened=true;engine.base=api.arena_ptr()>>>0;
    for(const [address,page,permissions] of mappedPages) {
      engine.ok('map',address,1,permissions);engine.request(page);engine.ok('upload',address,page.length);
    }
    const initialState=record('R3ST',56),state=view(initialState);
    kernel.initialRegs.forEach((value,i)=>state.setUint32(16+i*4,value,true));
    state.setUint32(48,0x1000,true);state.setUint32(52,2,true);
    new Uint8Array(engine.memory.buffer,engine.base,56).set(initialState);
    const initialBanks=bankRanges.map(([offset,length])=>bytes(engine.base+offset,length));
    const cache=new CodeCache(engine,{chain:true,budget:kernel.retired,instructions:64,blocks:8});
    for(const name of ['core_read','core_write','core_numeric','core_chain_next']) assert.equal(cache.native[name],api[name],`original ${name}`);
    const captureModule=()=>{
      const id=String(new DataView(engine.memory.buffer).getBigUint64(cache.context+120,true));
      if(!result.modules.some(row=>row.id===id)) {
        const encoded=bytes(api.core_code_ptr()>>>0,api.core_code_len()>>>0);
        result.modules.push({id,bytes:encoded.length,sha256:hash(encoded)});
      }
    };
    let caller,callee;
    for(const pc of [0x1000,calleePc,0x1000]) {
      const unit=cache.entry(pc);if(pc===0x1000)caller=unit;else callee=unit;captureModule();
    }
    const driver=bytes(api.core_dispatch_code_ptr()>>>0,api.core_dispatch_code_len()>>>0);
    result.driver={bytes:driver.length,sha256:hash(driver)};
    if(crossPage){engine.ok('core_begin');engine.ok('core_end');}
    return {index,name,engine,api,cache,bytes,result,initialState,initialBanks,captureModule,caller,callee};
  } catch(error){if(opened)engine.ok('close');throw error;}
}

function expectedRam(data,stack) {
  const ram=new Uint8Array(16*4096);
  ram.set(kernel.code);ram.set(data,4096);ram.set(stack,8192);
  if(crossPage)ram.set(calleePage,12288);
  return ram;
}

function checkRecency(context) {
  const {name,engine,api,cache,bytes,caller,callee}=context;
  activeContext=context;stage=`${name}/untimed-recency`;
  const beforeContext=bytes(cache.context,128),beforeArena=bytes(engine.base,api.arena_len());
  const beforeStats=cache.stats(),beforeVictim=api.core_cache_victim()>>>0;
  cache.budget=2;
  const value=cache.execute(caller),raw=bytes(cache.context,128),d=view(raw);
  const arena=bytes(engine.base,api.arena_len()),ram=bytes(d.getUint32(64,true),16*4096);
  const afterVictim=api.core_cache_victim()>>>0,afterStats=cache.stats();
  context.result.recencyControl={beforeVictim,afterVictim,callerSlot:caller.slot,calleeSlot:callee.slot,
    run:{...value,id:String(value.id)},contextHex:hex(raw),arenaSha256:hash(arena),ramSha256:hash(ram)};
  report.lastFailedRaw={arenaHex:hex(arena),ramHex:hex(ram)};checkpoint();
  assert.equal(beforeVictim,callee.slot,'priming leaves the callee as the older owner');
  assert.equal(afterVictim,caller.slot,'executing the callee moves its real native owner to newest');
  assert.equal(value.coreReason,2);assert.equal(value.retired,2);assert.equal(value.work,2);assert.equal(value.regions,2);
  assert.equal(value.id,callee.id);assert.equal(afterStats.runs-beforeStats.runs,1);
  assert.equal(afterStats.admissions-beforeStats.admissions,0);
  const expectedContext=beforeContext.slice(),c=view(expectedContext);
  c.setUint32(0,8,true);c.setUint32(16,0x47fc,true);c.setUint32(32,0x5003,true);c.setUint32(36,2,true);
  c.setUint32(40,(view(beforeContext).getUint32(40,true)+2)>>>0,true);
  for(const offset of [44,48,52,112,116])c.setUint32(offset,0,true);
  c.setBigUint64(120,callee.id,true);
  assert.deepEqual(raw,expectedContext,'complete partial context with its own native metadata');
  const expectedState=context.initialState.slice(),s=view(expectedState);
  s.setUint32(16,8,true);s.setUint32(32,0x47fc,true);s.setUint32(48,0x5003,true);
  const expectedExit=record('R3EX',40),e=view(expectedExit);
  e.setUint16(4,15,true);e.setUint32(16,1,true);e.setUint32(20,2,true);
  const expectedWork=record('R3WK',24);view(expectedWork).setUint32(16,2,true);
  const expectedArena=beforeArena.slice();expectedArena.set(expectedState);expectedArena.set(expectedExit,56);expectedArena.set(expectedWork,4588);
  assert.deepEqual(arena,expectedArena,'literal partial state/exit/work and all untouched arena bytes');
  assert.deepEqual(ram,expectedRam(kernel.initialData,kernel.expectedStack),'literal partial whole RAM');
  context.result.recencyControl.complete=true;delete report.lastFailedRaw;checkpoint();
}

function run(context,phase,round,position) {
  const {index,name,engine,api,cache,bytes}=context;
  activeContext=context;
  stage=`${phase}/${round}/${position}/${name}`;
  for(const [address,page] of [[0x3000,kernel.initialData],[0x4000,kernel.initialStack]]) {
    engine.request(page);engine.ok('upload',address,page.length);
  }
  new Uint8Array(engine.memory.buffer,engine.base,56).set(context.initialState);
  cache.import();let selected=cache.entry(0x1000);
  const beforeContext=crossPage?bytes(cache.context,128):null;
  const before=cache.stats(),priorRetired=cache.word(40);
  let retired=0,work=0,outer=0,stop;
  const started=performance.now();
  while(work<kernel.retired) {
    if(outer>=2*iterations+2)throw new Error('authored dispatch bound exceeded');
    cache.budget=kernel.retired-work;
    const value=cache.execute(selected);outer++;retired+=value.retired;work+=value.work;stop=value.coreReason;
    if(stop===2)break;
    if(stop!==1)throw new Error(`unexpected authored stop ${stop}`);
    selected=cache.entry();
  }
  const elapsedMs=performance.now()-started;
  const rawContext=bytes(cache.context,128),d=view(rawContext),pages=d.getUint32(68,true);
  const arena=bytes(engine.base,api.arena_len()),ram=bytes(d.getUint32(64,true),pages*4096),after=cache.stats();
  const sample={arm:name,phase,round,position,elapsedMs,outer,stop,retired,work,
    regions:after.regions-before.regions,dispatches:after.runs-before.runs,admissions:after.admissions-before.admissions,
    contextHex:hex(rawContext),arenaSha256:hash(arena),ramSha256:hash(ram)};
  report.runs.push(sample);report.lastFailedRaw={arenaHex:hex(arena),ramHex:hex(ram)};context.captureModule();checkpoint();
  assert.equal(stop,2);assert.equal(retired,kernel.retired);assert.equal(work,kernel.retired);
  assert.equal((d.getUint32(40,true)-priorRetired)>>>0,kernel.retired);
  assert.equal(d.getUint32(44,true),0);assert.equal(d.getUint32(112,true),0);
  assert.equal(sample.dispatches,outer);
  assert.deepEqual(Array.from({length:8},(_,i)=>d.getUint32(i*4,true)),kernel.regs);
  for(const [offset,value] of [[32,kernel.pc],[36,kernel.flags],[48,0],[52,0],[116,0]])assert.equal(d.getUint32(offset,true),value);
  if(crossPage) {
    assert.equal(outer,1);assert.equal(sample.regions,2*iterations+1);assert.equal(sample.admissions,0);
    const expectedContext=beforeContext.slice(),c=view(expectedContext);
    kernel.regs.forEach((value,i)=>c.setUint32(i*4,value,true));
    c.setUint32(32,kernel.pc,true);c.setUint32(36,kernel.flags,true);c.setUint32(40,(priorRetired+kernel.retired)>>>0,true);
    for(const offset of [44,48,52,112,116])c.setUint32(offset,0,true);
    c.setBigUint64(120,context.caller.id,true);
    assert.deepEqual(rawContext,expectedContext,'complete context with its own native pointers and identity');
  } else if(index===0){assert.equal(outer,1);assert.equal(sample.regions,2*iterations+1);assert.equal(sample.admissions,0);}
  else if(phase==='warmup'&&round===0){
    assert(outer>=2,'first execution crosses a cold promotion boundary');
    assert(sample.regions>1&&sample.regions<2*iterations+1,'first execution reduces actual region entries');
    assert(sample.admissions>0,'first execution admits the promoted region');
  } else {assert.equal(outer,1);assert.equal(sample.regions,1);assert.equal(sample.admissions,0);}
  const expectedState=record('R3ST',56),state=view(expectedState);
  kernel.regs.forEach((value,i)=>state.setUint32(16+i*4,value,true));state.setUint32(48,kernel.pc,true);state.setUint32(52,kernel.flags,true);
  assert.deepEqual(arena.slice(0,56),expectedState,'literal complete canonical state');
  const expectedExit=record('R3EX',40),exit=view(expectedExit);exit.setUint16(4,15,true);exit.setUint32(16,1,true);exit.setUint32(20,kernel.retired,true);
  assert.deepEqual(arena.slice(56,96),expectedExit,'literal Budget exit');
  const expectedWork=record('R3WK',24);view(expectedWork).setUint32(16,kernel.retired,true);
  assert.deepEqual(arena.slice(4588,4612),expectedWork,'literal work transport');
  assert.deepEqual(bankRanges.map(([offset,length])=>arena.slice(offset,offset+length)),context.initialBanks,'complete numerical and segment banks');
  assert.equal(pages,16);
  const memoryView=new DataView(engine.memory.buffer),mapping=d.getUint32(56,true);
  assert.deepEqual((crossPage?[1,3,4,5]:[1,3,4]).map(page=>memoryView.getUint32(mapping+page*4,true)),crossPage?[1,2,3,4]:[1,2,3]);
  assert.deepEqual(ram,expectedRam(kernel.expectedData,kernel.expectedStack),'literal whole canonical allocation');
  referenceArena??=arena;assert.deepEqual(arena,referenceArena,'whole canonical arena across both artifacts and all runs');
  context.result.arenaHex??=hex(arena);
  sample.complete=true;delete report.lastFailedRaw;checkpoint();
}

const median=values=>{const a=values.toSorted((x,y)=>x-y),n=a.length;return n%2?a[n>>1]:(a[n/2-1]+a[n/2])/2;};
try {
  contexts.push(fresh(0));contexts.push(fresh(1));
  if(crossPage)for(const context of contexts)checkRecency(context);
  for(let round=0;round<2;round++)for(let arm=0;arm<2;arm++)run(contexts[arm],'warmup',round,arm);
  for(let round=0;round<8;round++)(round%2?[1,0,0,1]:[0,1,1,0]).forEach((arm,position)=>run(contexts[arm],'timed',round,position));
  const elapsed=name=>report.runs.filter(row=>row.phase==='timed'&&row.arm===name).map(row=>row.elapsedMs);
  report.summary={baselineMedianMs:median(elapsed('baseline')),candidateMedianMs:median(elapsed('candidate'))};
  if(!sameArtifact)report.summary.ratio=report.summary.baselineMedianMs/report.summary.candidateMedianMs;
  else report.summary.control='same artifact; no optimization ratio';
  report.complete=true;checkpoint();console.log(JSON.stringify({complete:true,...report.summary}));
} catch(error){
  report.failure={stage,message:String(error),stack:error.stack};
  if(activeContext?.cache.context)try {
    const {cache,engine,api,bytes}=activeContext,raw=bytes(cache.context,128),d=view(raw);
    assert.equal(d.getUint32(68,true),16,'bounded failure snapshot');
    report.lastFailedRaw={contextHex:hex(raw),arenaHex:hex(bytes(engine.base,api.arena_len())),ramHex:hex(bytes(d.getUint32(64,true),16*4096))};
  } catch(snapshotError){report.failure.snapshotError=String(snapshotError);}
  checkpoint();throw error;
}
finally{for(const {cache,api,engine} of contexts){if(cache.open)api.core_end();engine.ok('close');}}
