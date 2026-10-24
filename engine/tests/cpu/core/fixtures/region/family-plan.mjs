import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync,writeFileSync,openSync,closeSync} from 'node:fs';
import {pathToFileURL} from 'node:url';
import {readEngine} from '../../../../support/wasm/engine.mjs';
import {coldFixture,coldLiteral} from '../../../../support/core_wasm/cold_observed.mjs';

const A=0x1000,B=0x1100,C=0x1200,D=0x1300;
const A_PORTS=[0x1000,0x1002,0x1005,0x1006,0x1008];
const B_PORTS=[0x1100,0x1102,0x1105,0x1106,0x1108];
const C_PORTS=[0x1200,0x1202,0x1204,0x120a];
const unhex=value=>Uint8Array.from(Buffer.from(value.replaceAll(' ',''),'hex'));
const hash=value=>createHash('sha256').update(value).digest('hex');
const trainRegisters=[0,128,0,0x3000,0x4800,0x12345678,B,A];
const bridgeRegisters=(count,prefix=C)=>[0,count,0,0x3000,0x4800,0x12345678,prefix,prefix];

function source(world) {
  return world.bytes(world.raw.core_code_ptr()>>>0,world.raw.core_code_len()>>>0);
}

function saved(world,unit) {
  assert.equal(world.cache.select(unit.pc),unit);
  const bytes=source(world);
  return{unit,id:unit.id,slot:unit.slot,run:unit.run,link:unit.link,publish:unit.publish,
    bytes:bytes.length,sha256:hash(bytes)};
}

function assertOwner(world,owner,ports) {
  const {unit}=owner;
  assert.equal(world.cache.units.get(owner.id),unit);
  for(const pc of ports) {
    assert.equal(world.selectSource(pc),unit,'an exact historical source port keeps its immutable owner');
    assert.equal(world.bigint(world.context,120),owner.id);
    const bytes=source(world);assert.equal(bytes.length,owner.bytes);assert.equal(hash(bytes),owner.sha256);
  }
  assert.equal(unit.slot,owner.slot);assert.equal(unit.run,owner.run);
  assert.equal(unit.link,owner.link);assert.equal(unit.publish,owner.publish);
  assert.equal(world.cache.table.get(owner.slot),owner.link);
  assert.equal(world.cache.publishers.get(owner.slot),owner.publish);
  assert.equal(world.bigint(world.raw.core_link_owners_ptr(),owner.slot*8),owner.id);
}

function moduleCount(world) {return world.result.modules.filter(module=>module.kind==='guest').length;}

function trainParents(world) {
  let before=world.snapshot(),run=world.execute(640,A);
  coldLiteral(world,before,run,{pc:0x100b,retired:640,budget:640,
    registers:[128,0,...trainRegisters.slice(2)],flags:0x46,writes:[],versionWrites:0,
    fpRegisters:{0:'00000000000080810740'},fpPc:A,identity:world.bigint(world.context,120)});
  world.result.parentASemanticComplete=true;
  assert.equal(world.cache.admissions,1);assert.equal(world.cache.units.size,1);
  const first=saved(world,[...world.cache.units.values()][0]);assertOwner(world,first,A_PORTS);

  world.reset({registers:trainRegisters,pc:B});before=world.snapshot();run=world.execute(640,B);
  coldLiteral(world,before,run,{pc:0x110b,retired:640,budget:640,
    registers:[384,0,...trainRegisters.slice(2)],flags:0x46,writes:[],versionWrites:0,
    fpRegisters:{0:'00000000000080810740'},fpPc:B,identity:world.bigint(world.context,120)});
  world.result.parentBSemanticComplete=true;
  assert.equal(world.cache.admissions,2);assert.equal(world.cache.units.size,2);
  const second=saved(world,[...world.cache.units.values()].find(unit=>unit.id!==first.id));
  assert.notEqual(first.id,second.id);assertOwner(world,first,A_PORTS);assertOwner(world,second,B_PORTS);
  assert.equal(moduleCount(world),2);assert.equal(world.moduleRecords.size,2);
  assert.equal(world.raw.core_lookup(C)>>>0,4,'the connecting path has never been compiled or executed');
  world.result.parents=[first,second].map(owner=>({id:String(owner.id),slot:owner.slot,
    bytes:owner.bytes,sha256:owner.sha256}));
  return[first,second];
}

function bridge(world,count,fp,prefix=C) {
  const odd=count%2===1,registers=bridgeRegisters(count,prefix);
  world.reset({registers,pc:A});const before=world.snapshot();
  const budget=5*count-1+4*Math.floor(count/2)+3*Math.floor((count-1)/2);
  const run=world.execute(budget,A);
  coldLiteral(world,before,run,{pc:odd?0x100a:0x110a,retired:budget,budget,
    registers:[count+2*Math.floor(count/2),0,odd?0:0xffffffff,...registers.slice(3)],flags:0x46,
    writes:[],versionWrites:0,fpRegisters:{0:fp},fpPc:odd?A:B,
    identity:world.bigint(world.context,120)});
  return{before,run};
}

function noNewModule(world,checkpoint) {
  assert.equal(world.cache.admissions,checkpoint.admissions);
  assert.equal(world.cache.generatedModuleBytes,checkpoint.bytes);
  assert.equal(moduleCount(world),checkpoint.modules);
  assert.equal(world.moduleRecords.size,checkpoint.records);
  assert.equal(world.cache.transientAdmissions,0);assert.equal(world.prepares,0);
}

const SAMPLE_INTERVAL=1024,CAPTURE_LIMIT=129;

function boundedFeedback(world,transitions,learningEdges,execute) {
  const cache=world.cache,descriptor=Object.getOwnPropertyDescriptor(cache,'installRecordedTrace');
  const original=cache.installRecordedTrace,captures=[];
  const pointer=world.raw.core_trace_pointer()>>>0;
  const countdown=world.word(pointer,32+24*CAPTURE_LIMIT);
  assert(countdown>=1&&countdown<=SAMPLE_INTERVAL);
  const resolves=world.resolverCalls;
  let value;
  try {
    Object.defineProperty(cache,'installRecordedTrace',{configurable:true,writable:true,value:function(...args) {
      if(world.raw.core_trace_ready()===1)captures.push(world.recording());
      return Reflect.apply(original,this,args);
    }});
    value=execute();
  } finally {
    if(descriptor)Object.defineProperty(cache,'installRecordedTrace',descriptor);
    else delete cache.installRecordedTrace;
  }
  assert(captures.length<=1,'one lease has at most one bounded pending capture');
  for(const capture of captures) {
    assert.equal(capture.active,0);assert.equal(capture.overflow,0);
    assert(capture.used<=CAPTURE_LIMIT);
    assert(capture.events.every(event=>event.kind===1||event.kind===2),
      'warm feedback records actual compiled operations, with no fabricated cold source');
  }
  // Every sampled miss represents 1024 eligible generated cross-owner routes.
  // Active recording can add at most one native transition per recorded event;
  // the four A→C, C→B, B→C, C→A edges are the only first-replay learning misses.
  const sampleBound=Math.floor((transitions+SAMPLE_INTERVAL-countdown)/SAMPLE_INTERVAL);
  const captureBound=captures.reduce((sum,capture)=>sum+capture.used,0);
  const misses=world.resolverCalls-resolves,bound=learningEdges+sampleBound+captureBound;
  assert(misses<=bound,'native feedback is bounded by sampling, actual capture length and finite edge learning');
  (world.result.feedback??=[]).push({transitions,countdown,misses,sampleBound,captureBound,learningEdges,
    captures:captures.map(capture=>({used:capture.used,root:capture.events[0]?.pc}))});
  return{value,misses,captures};
}

function checkpoint(world) {
  return{admissions:world.cache.admissions,bytes:world.cache.generatedModuleBytes,
    modules:moduleCount(world),records:world.moduleRecords.size};
}

function requireRecordedParents(world,parents,prefix=C) {
  const recording=world.recording();world.result.bridgeCapture=recording;
  assert.equal(recording.active,0);assert.equal(recording.overflow,0);assert(recording.used>0);
  assert(recording.events.some(event=>event.kind===3&&event.owner==='0'&&event.pc===prefix),
    'normal heat records the genuinely unowned connecting prefix');
  for(const owner of parents)assert(recording.events.some(event=>event.kind===1&&event.owner===String(owner.id)),
    'the actual connected capture holds each distinct current parent authority');
}

function rotatePorts(world,owner,ports) {
  const retained=checkpoint(world);
  for(const pc of ports) {
    world.reset({registers:bridgeRegisters(9),pc});const before=world.snapshot(),run=world.executeSource(0,pc);
    coldLiteral(world,before,run,{pc,retired:0,budget:0,registers:bridgeRegisters(9),flags:2,
      fp:before.fp,writes:[],versionWrites:0});
    assert.equal(run.id,owner.id);
  }
  assertOwner(world,owner,ports);noNewModule(world,retained);
}

export function runFamilyPlanCases(artifact,report,{boundedOnly=false}={}) {
  const code=new Uint8Array(4096);
  code.set(unhex('d8c1 83c001 49 7402 ffe7 90 0f0b'));
  code.set(unhex('d8c1 83c003 49 7402 ffe6 90 0f0b'),0x100);
  code.set(unhex('f7d2 85d2 0f84f6fdffff e9f1feffff'),0x200);
  code.set(unhex('40 49 75fc 90 0f0b'),0x300);

  if(!boundedOnly)coldFixture(artifact,report,'automatic-frontier-keeps-two-current-families-and-emits-only-new-bridge',
    {code,registers:trainRegisters},world=>{
      const parents=trainParents(world),epoch=world.bigint(world.word(world.context,88));
      bridge(world,192,'00000000000080c10740');world.result.bridgeSemanticComplete=true;
      requireRecordedParents(world,parents);
      assert.equal(world.bigint(world.word(world.context,88)),epoch);
      assert.equal(world.cache.admissions,3,'only the previously unowned bridge emits a new fragment');
      assert.equal(world.cache.units.size,3,'automatic connectivity preserves both immutable parent owners');
      for(const[parent,ports]of[[parents[0],A_PORTS],[parents[1],B_PORTS]])assertOwner(world,parent,ports);
      const owner=saved(world,[...world.cache.units.values()].find(unit=>!parents.some(parent=>parent.id===unit.id)));
      assert.equal(owner.id,parents[1].id+1n,'fully owned history consumes no identity or replacement emission');
      assert.equal(owner.unit.pc,C);
      assert.equal(moduleCount(world),3);assert.equal(world.moduleRecords.size,3);
      assertOwner(world,owner,C_PORTS);
      const retained=checkpoint(world);
      let learned=false;
      for(const[count,fp]of[[9,'00000000000000a80340'],[9,'00000000000000a80340'],[2048,'00000000000018800b40']]) {
        const feedback=boundedFeedback(world,2*count-2,learned?0:4,()=>bridge(world,count,fp));
        const {run}=feedback.value;
        assert.equal(run.regions,2*count-1,'each immutable parent and bridge executes once per actual visit');
        assert.equal(run.chainedColdWork,0);noNewModule(world,retained);
        if(count>=SAMPLE_INTERVAL) {
          assert(feedback.misses>0,'long warm execution reaches the real generated sampling path');
          assert.equal(feedback.captures.length,1,'sampled hotness starts one bounded capture for this lease');
        }
        learned=true;
      }
      for(const[parent,ports]of[[parents[0],A_PORTS],[parents[1],B_PORTS],[owner,C_PORTS]]) {
        rotatePorts(world,parent,ports);assertOwner(world,parent,ports);
      }
      world.result.semanticComplete=true;
    });

  coldFixture(artifact,report,'overbound-connected-plan-compiles-only-unowned-pc-zero-prefix-to-existing-frontier',
    {code,registers:trainRegisters},world=>{
      const zeroCode=new Uint8Array(4096);
      zeroCode.set(unhex('f7d2 85d2 0f84f60f0000 e9f1100000'));
      world.engine.ok('map',0,1,7);world.engine.request(zeroCode);world.engine.ok('upload',0,4096);
      const prefixPorts=[0,2,4,0xa];
      const parents=trainParents(world),epoch=world.bigint(world.word(world.context,88));
      // The existing five-node families remain valid. Fourteen historical
      // nodes cannot fit this requested graph budget; the four-node unowned
      // bridge can fit without absorbing or shortening either parent.
      world.cache.instructions=8;
      bridge(world,192,'00000000000080c10740',0);world.result.bridgeSemanticComplete=true;
      requireRecordedParents(world,parents,0);
      assert.equal(world.bigint(world.word(world.context,88)),epoch);
      assert.equal(world.cache.admissions,3,'bounded refusal still admits the exact unowned cold prefix');
      assert.equal(world.cache.units.size,3);
      for(const[parent,ports]of[[parents[0],A_PORTS],[parents[1],B_PORTS]])assertOwner(world,parent,ports);
      const prefix=saved(world,[...world.cache.units.values()].find(unit=>!parents.some(parent=>parent.id===unit.id)));
      assert.equal(prefix.unit.pc,0,'successful installation preserves an actual zero-valued prefix anchor');
      assert.equal(world.raw.core_trace_ready(),2,'installed-root metadata is explicit even when its PC is zero');
      assert.equal(world.raw.core_trace_root(),0);
      assert.equal(prefix.id,parents[1].id+1n,'the rejected full union allocates no identity or emitted executor');
      assertOwner(world,prefix,prefixPorts);
      assert.equal(moduleCount(world),3);assert.equal(world.moduleRecords.size,3);
      const retained=checkpoint(world);
      let learned=false;
      for(const[count,fp]of[[9,'00000000000000a80340'],[9,'00000000000000a80340'],[2048,'00000000000018800b40']]) {
        const feedback=boundedFeedback(world,2*count-2,learned?0:4,()=>bridge(world,count,fp,0));
        const {run}=feedback.value;
        assert.equal(run.regions,2*count-1,'each immutable parent and the exact bridge prefix executes once per actual visit');
        assert.equal(run.chainedColdWork,0,'the resource refusal never forces known code back through cold execution');
        if(count>=SAMPLE_INTERVAL) {
          assert(feedback.misses>0,'bounded refusal still receives sampled generated feedback');
          assert.equal(feedback.captures.length,1);
        }
        learned=true;
        noNewModule(world,retained);
        for(const[parent,ports]of[[parents[0],A_PORTS],[parents[1],B_PORTS]])assertOwner(world,parent,ports);
        assertOwner(world,prefix,prefixPorts);
      }
      // The long replay crosses the existing hot-capture threshold. A later
      // independent accepted family makes any hidden refused emission visible
      // as an identity gap; module-constructor counters alone cannot prove it.
      world.reset({registers:[0,128,...trainRegisters.slice(2)],pc:D});
      const before=world.snapshot(),run=world.execute(385,D);
      coldLiteral(world,before,run,{pc:0x1305,retired:385,budget:385,
        registers:[128,0,...trainRegisters.slice(2)],flags:0x46,fp:before.fp,writes:[],versionWrites:0,
        identity:world.bigint(world.context,120)});
      const next=[...world.cache.units.values()].find(unit=>unit.id!==prefix.id&&!parents.some(parent=>parent.id===unit.id));
      assert(next);assert.equal(next.id,prefix.id+1n,'equal/refused replay performs no hidden identity allocation or Wasm emission');
      assert.equal(world.cache.admissions,4);assert.equal(world.cache.units.size,4);
      assert.equal(moduleCount(world),4);assert.equal(world.moduleRecords.size,4);
      for(const[parent,ports]of[[parents[0],A_PORTS],[parents[1],B_PORTS]])assertOwner(world,parent,ports);
      assertOwner(world,prefix,prefixPorts);assert.equal(world.prepares,0);world.result.semanticComplete=true;
    });
}

if(process.argv[1]&&import.meta.url===pathToFileURL(process.argv[1]).href) {
  const[enginePath,reportPath,...flags]=process.argv.slice(2);
  assert(enginePath&&reportPath&&flags.every(flag=>flag==='--bounded-prefix'),
    'usage: family-plan.mjs engine.wasm fresh-report.json [--bounded-prefix]');
  const artifact=readEngine(enginePath),sources=['./family-plan.mjs','../../../../support/core_wasm/cold_observed.mjs',
    '../../../../../../sdk/runtime/cache.js'];
  const report={schema:1,complete:false,engineSha256:artifact.sha256,
    sources:Object.fromEntries(sources.map(path=>[path,hash(readFileSync(new URL(path,import.meta.url)))])),
    scope:'actual connected-family planning, atomic ownership and bounded cold-prefix compilation; no performance claim',cases:[]};
  closeSync(openSync(reportPath,'wx'));
  try {runFamilyPlanCases(artifact,report,{boundedOnly:flags.includes('--bounded-prefix')});report.complete=true;}
  catch(error){report.failure={message:String(error),stack:error.stack};throw error;}
  finally{writeFileSync(reportPath,JSON.stringify(report,null,2));}
  console.log(JSON.stringify({complete:true,engineSha256:artifact.sha256,cases:report.cases.length}));
}
