import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync,writeFileSync,openSync,closeSync} from 'node:fs';
import {performance} from 'node:perf_hooks';
import {readEngine} from '../../../support/wasm/engine.mjs';
import {Engine,KEY,record,view} from '../../../../../sdk/runtime/engine.js';
import {CodeCache} from '../../../../../sdk/runtime/cache.js';

const [baselinePath,baselineHash,candidatePath,candidateHash,reportPath,comparison='native']=process.argv.slice(2);
assert(reportPath,'usage: numerical-timing.mjs baseline.wasm baseline-sha256 candidate.wasm candidate-sha256 fresh-report.json [native|codegen]');
assert(['native','codegen'].includes(comparison),'expected native or codegen comparison');
for(const value of [baselineHash,candidateHash])assert.match(value,/^[a-f0-9]{64}$/);
const artifacts=[readEngine(baselinePath,baselineHash),readEngine(candidatePath,candidateHash)];
closeSync(openSync(reportPath,'wx'));
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
const hex=bytes=>Buffer.from(bytes).toString('hex');
const unhex=text=>Uint8Array.from(Buffer.from(text,'hex'));
const iterations=131072,work=5*iterations,code=new Uint8Array(4096);
code.set(unhex('d903d87304d95b084975f50f0b'));
const sources=['./numerical-timing.mjs','../../../support/wasm/engine.mjs',
  '../../../../../sdk/runtime/engine.js','../../../../../sdk/runtime/cache.js'];
const report={schema:1,complete:false,node:process.version,iterations,retiredAndWork:work,warmupsPerArm:2,rounds:8,
  artifactComparison:baselineHash===candidateHash?'same-artifact-control':'paired-artifacts',comparison,
  scope:'authored PC24 FLD/FDIV/FSTP/DEC/JNZ through original native bindings; no game or component time-share claim',
  clock:'node performance.now elapsed milliseconds around one normal CodeCache.execute',codeSha256:hash(code),
  sources:Object.fromEntries(sources.map(path=>[path,hash(readFileSync(new URL(path,import.meta.url)))])),cases:[],
  limits:['reset, canonical bank import, selection, output snapshots and assertions are outside the timed interval',
    'no timed admissions does not establish completion of all optimizing compiler tiers',
    comparison==='native'?'exact generated module equality is required across both artifacts; native numerical implementation may differ':
      'generated code may differ; both artifacts must independently satisfy the same literal full-context, FP80, arena and RAM assertions',
    'C1 controls stop immediately after FDIV; the timed FSTP clears C1 after its exact f32 conversion',
    'normalized significands of at most24bits cannot yield an inexact carry into the next power of2 at PC24; wider fallback carry is outside this fixture']};
const checkpoint=()=>writeFileSync(reportPath,JSON.stringify(report,null,2));
const banks=[[4364,224],[4612,184]],contexts=[];
let stage='initialization';checkpoint();
const cases=[{name:'three-divided-by-two',numerator:0x40400000,denominator:0x40000000,output:0x3fc00000,
  extended:'00000000000000c0ff3f',inexact:false},
  {name:'one-divided-by-three',numerator:0x3f800000,denominator:0x40400000,output:0x3eaaaaab,
    extended:'0000000000abaaaafd3f',inexact:true}];

function fresh(artifact,index,test,result){
  const imports=WebAssembly.Module.imports(artifact.module);
  if(imports.length)assert.deepEqual(imports,[{module:'ring3_observability',name:'now_ns',kind:'function'}]);
  const instance=new WebAssembly.Instance(artifact.module,{ring3_observability:{now_ns:()=>0n}});
  const api=Object.freeze(Object.fromEntries(Object.entries(instance.exports).filter(([name])=>name.startsWith('ring3_abi_v1_'))
    .map(([name,value])=>[name.slice(13),value])));
  const engine=new Engine(api,instance.exports.memory,artifact.sha256);
  const bytes=(pointer,length)=>new Uint8Array(engine.memory.buffer).slice(pointer,pointer+length);
  let opened=false;
  try{
    engine.ok('open',16,...KEY);opened=true;engine.base=api.arena_ptr()>>>0;
    for(const [address,page,permissions]of [[0x1000,code,7],[0x3000,new Uint8Array(4096),3],[0x4000,new Uint8Array(4096),3]]){
      engine.ok('map',address,1,permissions);engine.request(page);engine.ok('upload',address,4096);
    }
    const initialState=record('R3ST',56),s=view(initialState);
    [0x11223344,iterations,0x55667788,0x3000,0x4800,0x99aabbcc,0x55aa55aa,0xa5a5a5a5]
      .forEach((v,i)=>s.setUint32(16+i*4,v,true));s.setUint32(48,0x1000,true);s.setUint32(52,2,true);
    new Uint8Array(engine.memory.buffer,engine.base,56).set(initialState);
    const initialFp=bytes(engine.base+4236,128);view(initialFp).setUint16(16,0x7f,true);
    new Uint8Array(engine.memory.buffer,engine.base+4236,128).set(initialFp);
    const cache=new CodeCache(engine,{chain:true,budget:work,instructions:64,blocks:8});
    for(const name of ['core_read','core_write','core_numeric','core_chain_next'])assert.equal(cache.native[name],api[name],`original ${name}`);
    const unit=cache.entry(0x1000),encoded=bytes(api.core_code_ptr()>>>0,api.core_code_len()>>>0);
    assert.deepEqual(WebAssembly.Module.imports(new WebAssembly.Module(encoded)),[
      {module:'ring3',name:'memory',kind:'memory'},{module:'ring3',name:'read',kind:'function'},
      {module:'ring3',name:'write',kind:'function'},{module:'ring3',name:'numeric',kind:'function'}]);
    engine.ok('core_begin');engine.ok('core_end');
    const arm={name:index?'candidate':'baseline',engineSha256:artifact.sha256,baseImports:imports,
      module:{id:String(unit.id),bytes:encoded.length,sha256:hash(encoded),wasmHex:hex(encoded)}};
    result.arms.push(arm);
    const value={engine,api,cache,bytes,unit,arm,test,initialState,initialFp,
      initialArena:bytes(engine.base,api.arena_len()),initialBanks:banks.map(([offset,length])=>bytes(engine.base+offset,length))};
    contexts.push(value);return value;
  }catch(error){if(opened)engine.ok('close');throw error;}
}

function run(t,{phase,round=0,position=0,negative=false,rounding=0,prefix=false}){
  const {engine,api,cache,bytes,unit,test}=t;stage=`${test.name}/${phase}/${t.arm.name}/${round}/${position}`;
  const data=new Uint8Array(4096),dv=view(data),inputState=t.initialState.slice(),sv=view(inputState);
  dv.setUint32(0,test.numerator+(negative?0x80000000:0),true);dv.setUint32(4,test.denominator,true);
  sv.setUint32(20,prefix?1:iterations,true);
  const inputFp=t.initialFp.slice(),control=0x7f+(rounding<<10);view(inputFp).setUint16(16,control,true);
  for(const [address,page]of [[0x3000,data],[0x4000,new Uint8Array(4096)]]){
    engine.request(page);engine.ok('upload',address,4096);
  }
  new Uint8Array(engine.memory.buffer,engine.base,56).set(inputState);
  new Uint8Array(engine.memory.buffer,engine.base+4236,128).set(inputFp);
  cache.import();const selected=cache.entry(0x1000);assert.equal(selected,unit);
  const before=cache.stats(),initialContext=bytes(cache.context,128),beforeRetired=view(initialContext).getUint32(40,true);
  const current=new DataView(engine.memory.buffer),versionPointer=view(initialContext).getUint32(80,true);
  const initialVersion=current.getBigUint64(versionPointer,true),budget=prefix?2:work;
  cache.budget=budget;
  const started=performance.now(),value=cache.execute(selected),elapsedMs=performance.now()-started;
  const rawContext=bytes(cache.context,128),ctx=view(rawContext),arena=bytes(engine.base,api.arena_len());
  const ram=bytes(ctx.getUint32(64,true),16*4096),after=cache.stats();
  const row={arm:t.arm.name,phase,round,position,negative,rounding,prefix,elapsedMs,budget,
    ...value,id:String(value.id),admissions:after.admissions-before.admissions,
    outer:after.runs-before.runs,regions:after.regions-before.regions,contextHex:hex(rawContext),
    fpHex:hex(arena.slice(4236,4364)),arenaSha256:hash(arena),ramSha256:hash(ram)};
  t.result[phase==='control'?'controls':'runs'].push(row);
  report.lastRaw={contextHex:hex(rawContext),arenaHex:hex(arena),ramHex:hex(ram)};checkpoint();
  const increment=test.inexact&&(rounding===0||(rounding===1&&negative)||(rounding===2&&!negative));
  const extended=test.inexact?(increment?'0000000000abaaaafd3f':'0000000000aaaaaafd3f'):test.extended;
  const ext=unhex(extended);if(negative)ext[9]|=0x80;
  const wantedFp=inputFp.slice(),fv=view(wantedFp);
  for(const [offset,v]of [[18,(prefix?0x3800:0)+(test.inexact?0x20:0)+(prefix&&increment?0x200:0)],
    [20,prefix?0x3fff:0xffff],[22,prefix?0x73:0x15b],[32,0x1b],[34,0x23]])fv.setUint16(offset,v,true);
  fv.setUint32(24,prefix?0x1002:0x1005,true);fv.setUint32(28,prefix?0x3004:0x3008,true);wantedFp.set(ext,110);
  const expectedState=inputState.slice(),es=view(expectedState);es.setUint32(20,prefix?1:0,true);
  es.setUint32(48,prefix?0x1005:0x100b,true);es.setUint32(52,prefix?2:0x46,true);
  const expectedContext=initialContext.slice(),ec=view(expectedContext);
  expectedContext.set(expectedState.subarray(16,48));
  for(const [offset,v]of [[32,prefix?0x1005:0x100b],[36,prefix?2:0x46],[40,(beforeRetired+budget)>>>0],
    [44,0],[48,0],[52,0],[112,0],[116,0]])ec.setUint32(offset,v,true);
  ec.setBigUint64(120,unit.id,true);
  const expectedRam=new Uint8Array(16*4096);expectedRam.set(code);expectedRam.set(data,4096);
  if(!prefix)view(expectedRam).setUint32(4096+8,test.output,true);
  const expectedArena=t.initialArena.slice();expectedArena.set(expectedState);expectedArena.set(wantedFp,4236);
  const exit=record('R3EX',40);view(exit).setUint16(4,15,true);view(exit).setUint32(16,1,true);view(exit).setUint32(20,budget,true);
  expectedArena.set(exit,56);const wantedWork=record('R3WK',24);view(wantedWork).setUint32(16,budget,true);expectedArena.set(wantedWork,4588);
  assert.equal(value.coreReason,2);assert.equal(value.retired,budget);assert.equal(value.work,budget);
  assert.equal(value.kind,'compiled');assert.equal(value.id,unit.id);
  assert.equal(row.outer,1);assert.equal(row.regions,1);assert.equal(row.admissions,0);assert.equal(after.coldRuns-before.coldRuns,0);
  assert.deepEqual(rawContext,expectedContext,'full literal context');assert.deepEqual(arena,expectedArena,'full literal arena and FP128');
  assert.deepEqual(ram,expectedRam,'whole canonical allocation');
  assert.deepEqual(banks.map(([offset,length])=>arena.slice(offset,offset+length)),t.initialBanks,'full SIMD/segments/upper vectors/bases');
  assert.equal(new DataView(engine.memory.buffer).getBigUint64(versionPointer,true),initialVersion+BigInt(prefix?0:iterations));
  row.complete=true;delete report.lastRaw;checkpoint();
}

const median=values=>{const a=values.toSorted((x,y)=>x-y);return(a[(a.length-1)>>1]+a[a.length>>1])/2;};
try{
  for(const test of cases){
    const result={name:test.name,expectedOutputBits:test.output,expectedExtended:test.extended,arms:[],controls:[],runs:[]};report.cases.push(result);
    const arms=artifacts.map((artifact,index)=>fresh(artifact,index,test,result));arms.forEach(arm=>arm.result=result);
    result.identicalGeneratedModule=JSON.stringify(result.arms[0].module)===JSON.stringify(result.arms[1].module);
    if(comparison==='native')assert.deepEqual(result.arms[0].module,result.arms[1].module,'identical generated guest module across artifacts');
    for(const arm of arms)for(let rounding=0;rounding<4;rounding++)for(const negative of [false,true])
      run(arm,{phase:'control',rounding,negative,prefix:true});
    for(let round=0;round<2;round++)for(let arm=0;arm<2;arm++)run(arms[arm],{phase:'warmup',round,position:arm});
    for(let round=0;round<8;round++)for(const [position,arm]of(round%2?[1,0,0,1]:[0,1,1,0]).entries())
      run(arms[arm],{phase:'timed',round,position});
    result.mediansMs=Object.fromEntries(arms.map(arm=>[arm.arm.name,median(result.runs.filter(r=>r.phase==='timed'&&r.arm===arm.arm.name).map(r=>r.elapsedMs))]));
    if(baselineHash!==candidateHash)result.ratio=result.mediansMs.baseline/result.mediansMs.candidate;
    for(const arm of arms){arm.engine.ok('close');contexts.splice(contexts.indexOf(arm),1);}
    result.complete=true;checkpoint();
  }
  report.complete=true;checkpoint();console.log(JSON.stringify({complete:true,cases:report.cases.map(({name,mediansMs,ratio})=>({name,mediansMs,ratio}))}));
}catch(error){report.failure={stage,message:String(error),stack:error.stack};checkpoint();throw error;}
finally{for(const t of contexts){if(t.cache.open)t.api.core_end();t.api.close();}}
