import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync,writeFileSync,openSync,closeSync} from 'node:fs';
import {Session} from 'node:inspector';
import {readEngine} from '../../../support/wasm/engine.mjs';
import {Engine,KEY,record,view} from '../../../../../sdk/runtime/engine.js';

const [enginePath,expectedSha256,reportPath]=process.argv.slice(2);
assert(enginePath&&expectedSha256&&reportPath,'usage: cold-profile.mjs engine.wasm expected-sha256 fresh-report.json');
const profilePath=reportPath.endsWith('.json')?`${reportPath.slice(0,-5)}.cpuprofile`:`${reportPath}.cpuprofile`;
closeSync(openSync(reportPath,'wx'));closeSync(openSync(profilePath,'wx'));
const hash=bytes=>createHash('sha256').update(bytes).digest('hex'),hex=bytes=>Buffer.from(bytes).toString('hex');
const sources=['./cold-profile.mjs','../../../support/wasm/engine.mjs','../../../../../sdk/runtime/engine.js',
  '../../../../../sdk/runtime/engine/module.js'];
const count=1_000_000,pages=512,codePages=256,entry=0x1000;
const report={schema:1,complete:false,node:process.version,iterations:count,pages,codePages,entry,
  scope:'one native cold call over one million distinct, one-use INC EAX instructions; no compiled guest regions',
  limits:['no repeated hot entry, Windows, module gates, retained compiled cache or game workload',
    'sampling includes native cold admission, decode, execution, memory and heat bookkeeping; no exclusive component percentage',
    'raw profiler names may be unresolved; self sample counts are descriptive, not a speed claim',
    'current Node isolate samples are not all-process CPU accounting or proof of completed base-engine tier compilation',
    'the profiler window includes start/stop bookends; base module creation, setup and validation are outside it'],
  samplingIntervalUs:1000,profilePath,sources:Object.fromEntries(sources.map(path=>
    [path,hash(readFileSync(new URL(path,import.meta.url)))]))};
const checkpoint=()=>writeFileSync(reportPath,JSON.stringify(report,null,2));
const profiler=new Session(),post=(method,params={})=>new Promise((resolve,reject)=>
  profiler.post(method,params,(error,result)=>error?reject(error):resolve(result)));
let stage='module creation',engine,api,context=0,backing=0,opened=false,connected=false,profiling=false;
let initialContext,initialArena,expectedRam;
const bytes=(pointer,length)=>new Uint8Array(engine.memory.buffer).slice(pointer,pointer+length);
const word=(pointer,offset=0)=>new DataView(engine.memory.buffer).getUint32(pointer+offset,true);
const snapshot=()=>({context:bytes(context,128),arena:bytes(engine.base,api.arena_len()),ram:bytes(backing,pages*4096)});
const retain=raw=>({contextHex:hex(raw.context),arenaHex:hex(raw.arena),ramSha256:hash(raw.ram),ramBytes:raw.ram.length});
checkpoint();
try {
  const artifact=readEngine(enginePath,expectedSha256);
  report.engineSha256=artifact.sha256;report.engineBytes=artifact.bytes.length;
  const instance=new WebAssembly.Instance(artifact.module,{ring3_observability:{now_ns:()=>0n}});
  api=Object.freeze(Object.fromEntries(Object.entries(instance.exports).filter(([name])=>name.startsWith('ring3_abi_v1_'))
    .map(([name,value])=>[name.slice(13),value])));
  engine=new Engine(api,instance.exports.memory,artifact.sha256);
  const cold=api.core_cold_run;assert.equal(typeof cold,'function');
  stage='setup';engine.ok('open',pages,...KEY);opened=true;engine.base=api.arena_ptr()>>>0;
  engine.ok('map',entry,codePages,7);
  const page=new Uint8Array(4096).fill(0x40);
  for(let index=0;index<codePages;index++){engine.request(page);engine.ok('upload',entry+index*4096,4096);}
  const state=record('R3ST',56),stateView=view(state),registers=[0,0x22334455,0x33445566,0x3000,0x4800,0x66778899,0x778899aa,0x8899aabb];
  registers.forEach((value,index)=>stateView.setUint32(16+index*4,value,true));
  stateView.setUint32(48,entry,true);stateView.setUint32(52,2,true);
  new Uint8Array(engine.memory.buffer,engine.base,56).set(state);initialArena=bytes(engine.base,api.arena_len());
  assert.equal(cold(0),2);context=api.core_context_ptr()>>>0;assert(context>0&&context%8===0);
  backing=word(context,64);assert(backing>0);assert.equal(word(context,68),pages);
  const mappings=word(context,56),permissions=word(context,60);assert(mappings>0&&permissions>0);
  for(let index=0;index<codePages;index++){
    assert.equal(word(mappings+(index+1)*4),index+1);assert.equal(bytes(permissions+index+1,1)[0],7);
  }
  initialContext=bytes(context,128);const initialView=view(initialContext);
  assert.deepEqual(Array.from({length:8},(_,index)=>initialView.getUint32(index*4,true)),registers);
  for(const [offset,value] of [[32,entry],[36,2],[40,0],[44,0],[48,0],[52,0],[112,0],[116,0]])assert.equal(word(context,offset),value);
  assert.equal(initialView.getBigUint64(120,true),0n);assert.equal(api.core_code_ptr(),0);assert.equal(api.core_code_len(),0);
  expectedRam=new Uint8Array(pages*4096);expectedRam.fill(0x40,0,codePages*4096);
  const initial=snapshot();report.before=retain(initial);checkpoint();
  assert.deepEqual(initial.arena,initialArena);assert.deepEqual(initial.ram,expectedRam,'literal whole backing before profiling');
  const expectedContext=initialContext.slice(),expectedView=view(expectedContext);
  for(const [offset,value] of [[0,count],[32,entry+count],[36,0x12],[40,count],[44,0]])expectedView.setUint32(offset,value,true);
  profiler.connect();connected=true;await post('Profiler.enable');await post('Profiler.setSamplingInterval',{interval:1000});
  stage='profile original native cold call';assert.equal(cold,instance.exports.ring3_abi_v1_core_cold_run);
  await post('Profiler.start');profiling=true;
  const start=performance.now(),stop=cold(count),elapsedMs=performance.now()-start;
  const {profile}=await post('Profiler.stop');profiling=false;
  writeFileSync(profilePath,JSON.stringify(profile));report.profileSha256=hash(readFileSync(profilePath));
  const nodes=new Map(profile.nodes.map(node=>[node.id,node])),counts=new Map();
  for(const id of profile.samples??[])counts.set(id,(counts.get(id)??0)+1);
  report.profile={samples:profile.samples?.length??0,startTime:profile.startTime,endTime:profile.endTime,
    rawFunctionNames:'all nodes and call frames retained in the .cpuprofile',
    unknownNodeSamples:[...counts].filter(([id])=>!nodes.has(id)).reduce((sum,[,value])=>sum+value,0),
    topSelfSamples:[...counts].sort((a,b)=>b[1]-a[1]||a[0]-b[0]).slice(0,16)
      .map(([id,selfSamples])=>({id,selfSamples,callFrame:nodes.get(id)?.callFrame??null}))};
  const after=snapshot();report.run={budget:count,stop,elapsedMs,...retain(after)};checkpoint();
  stage='independent full-state validation';assert.equal(stop,2);
  assert(report.profile.samples>0,'native profile must retain actual samples');
  assert.deepEqual(after.context,expectedContext,'full 128-byte closed context after one million INC instructions');
  assert.deepEqual(after.arena,initialArena,'whole unpublished arena, including all numerical and vector banks');
  assert.deepEqual(after.ram,expectedRam,'literal whole 512-page backing: code and untouched unused slots');
  assert.equal(api.core_code_ptr(),0);assert.equal(api.core_code_len(),0);
  report.complete=true;
} catch(error) {
  report.failure={stage,message:String(error),stack:error.stack,...(context&&backing?retain(snapshot()):{})};throw error;
} finally {
  if(profiling)try{const {profile}=await post('Profiler.stop');writeFileSync(profilePath,JSON.stringify(profile));report.profileSha256=hash(readFileSync(profilePath));}
    catch(error){report.profileStopError=String(error);report.complete=false;}
  if(connected)profiler.disconnect();
  if(opened){const status=api.close();report.closeStatus=status;if(status!==0){report.complete=false;report.closeError=`close returned ${status}`;}}
  checkpoint();
}
assert.equal(report.complete,true);console.log(JSON.stringify({complete:true,engineSha256:report.engineSha256,profilePath,
  samples:report.profile.samples,topSelfSamples:report.profile.topSelfSamples}));
