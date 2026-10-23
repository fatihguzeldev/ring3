import {instantiateCore,coreImports,coreSupportSource,instantiateSupport} from './link.mjs';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync,writeFileSync,openSync,closeSync} from 'node:fs';
import {readEngine} from '../../../support/wasm/engine.mjs';
import {Engine,KEY,record,view} from '../../../../../sdk/runtime/engine.js';
import {inspectRegion,inspectPortDepth,assertRegionStructure,assertAnalyzedStateStructure,instrumentStateWitness} from './region/structure.mjs';
import {runControlCases} from './region/control.mjs';
import {runLinkedCases} from './region/link.mjs';
import {runStatusCases,runLinkedStatusCases} from './region/status.mjs';
import {runStateCases,runRenamingCase} from './region/state.mjs';
import {runLinkedQuotaCase} from './region/quota.mjs';
import {runOwnershipCase} from './region/ownership.mjs';
import {runPortCases} from './region/ports.mjs';
import {runStableGraphCase} from './region/graph.mjs';
import {runGroupCases} from './region/groups.mjs';
import {runNativeStoreCases} from './region/native-store.mjs';
import {runPoolCases,runPoolLoopCases,runTransportCases,runTransportLoopCases} from './region/pool.mjs';
import {runExecutionCases,runExecutionFlagsCases,runExecutionLoadCases,runExecutionGuardCases,runExecutionMultiplyCases,runExecutionFlagsOracleCases} from './region/execution.mjs';
import {runEntryCases,runEntryNestedCases,runEntryUnmatchedCases} from './region/entry.mjs';

const [enginePath,reportPath,mode]=process.argv.slice(2);
assert(enginePath&&reportPath&&(mode===undefined||mode==='--entry'||mode==='--entry-baseline'||mode==='--entry-nested'||mode==='--entry-unmatched'||mode==='--structure'||mode==='--graph'||mode==='--groups'||mode==='--native-stores'||mode==='--pool'||mode==='--pool-baseline'||mode==='--transport'||mode==='--transport-baseline'||mode==='--execution'||mode==='--execution-flags'||mode==='--execution-loads'||mode==='--execution-guards'||mode==='--execution-multiply'||mode==='--execution-flags-oracle'),
  'usage: region.mjs engine.wasm fresh-report.json [--entry|--entry-baseline|--entry-nested|--entry-unmatched|--structure|--graph|--groups|--native-stores|--pool|--pool-baseline|--transport|--transport-baseline|--execution|--execution-flags|--execution-loads|--execution-guards|--execution-multiply|--execution-flags-oracle]');
const artifact=readEngine(enginePath),hash=bytes=>createHash('sha256').update(bytes).digest('hex');
const hex=bytes=>Buffer.from(bytes).toString('hex');
const unhex=text=>Uint8Array.from(Buffer.from(text.replaceAll(' ',''),'hex'));
closeSync(openSync(reportPath,'wx'));
const report={schema:1,complete:false,engineSha256:artifact.sha256,
  scope:'actual generated mixed integer/x87 CFG with exact instruction-prefix, publication and fault oracles; no timing claim',
  sources:Object.fromEntries(['./region.mjs','./link.mjs','./region/structure.mjs','./region/control.mjs','./region/link.mjs','./region/status.mjs','./region/state.mjs','./region/quota.mjs','./region/ownership.mjs','./region/ports.mjs','./region/graph.mjs','./region/groups.mjs','./region/native-store.mjs','./region/pool.mjs','./region/execution.mjs','./region/execution-flags.mjs','./region/entry.mjs','./region/witness.mjs','./pool-state.mjs',
    '../../../support/wasm/engine.mjs','../../../../../sdk/runtime/engine.js','../../../../../sdk/runtime/cache.js']
    .map(path=>[path,hash(readFileSync(new URL(path,import.meta.url)))])),cases:[]};
const checkpoint=()=>writeFileSync(reportPath,JSON.stringify(report,null,2));
const FP=4236,SIMD=4364,YMM=4612;
const EXT={negativeZero:'00000000000000000080',three:'00000000000000c00040',
  five:'00000000000000a00140',six:'00000000000000c00140',
  seven:'00000000000000e00140',twelve:'00000000000000c00240'};
let stage='initialization';

function fp(initial,{status=0,tag=0xffff,opcode=0,fip=0,fdp=0,registers={},cs=0x1b,ds=0x23}={}) {
  const result=initial.slice(),d=view(result);
  for(const[offset,value]of[[18,status],[20,tag],[22,opcode],[32,cs],[34,ds]])d.setUint16(offset,value,true);
  d.setUint32(24,fip,true);d.setUint32(28,fdp,true);
  for(const[index,value]of Object.entries(registers))result.set(unhex(value),40+10*Number(index));
  return result;
}

function fixture(name,{segments,entry=0x1000,registers=[0,2,0,0x3000,0x4800,0,0,0],control=0x27f,
  compileInstructions=256,compileBlocks=32,initialFlags=2,
  initialFp,missing=[],stateWitness=false,witnessData,data=page=>{view(page).setFloat32(0,3,true);view(page).setFloat32(4,2,true);}}={},body) {
  stage=name;
  const result={name,controls:[],modules:[],runs:[],numericCalls:[]};report.cases.push(result);checkpoint();
  const instance=instantiateCore(artifact.module,{ring3_observability:{now_ns:()=>0n}});
  const api=Object.fromEntries(Object.entries(instance.exports).filter(([key])=>key.startsWith('ring3_abi_v1_'))
    .map(([key,value])=>[key.slice(13),value]));
  const memory=instance.exports.memory,engine=new Engine(api,memory,artifact.sha256);
  const bytes=(pointer,length)=>new Uint8Array(memory.buffer).slice(pointer,pointer+length);
  const word=(pointer,offset=0)=>new DataView(memory.buffer).getUint32(pointer+offset,true);
  let opened=false,active=false,context=0,helpers=0,run,publish,witnessReader,encodedModule,groupWitnessProof;
  const version=()=>new DataView(memory.buffer).getBigUint64(word(context,80),true);
  const status=(operation,wanted,...args)=>{
    const actual=api[operation](...args)>>>0;result.controls.push({operation,wanted,actual});
    assert.equal(actual,wanted,`${name}: ${operation}`);
  };
  const physical=address=>{
    const slot=word(word(context,56)+(address>>>12)*4);assert(slot>0&&slot<=word(context,68));
    return word(context,64)+(slot-1)*4096+(address&4095);
  };
  const prepare=pc=>{
    status('core_prepare',0,pc,compileInstructions,compileBlocks);context=api.core_context_ptr()>>>0;
    const encoded=bytes(api.core_code_ptr()>>>0,api.core_code_len()>>>0),module=new WebAssembly.Module(encoded);
    encodedModule=encoded;
    const imports=coreImports(module);
    const moduleRecord={pc,sha256:hash(encoded),bytes:encoded.length,imports};result.modules.push(moduleRecord);
    const diagnostic=stateWitness?instrumentStateWitness(encoded,coreSupportSource(memory)):undefined;
    const support=diagnostic?instantiateSupport(memory,diagnostic.supportBytes):undefined;
    groupWitnessProof=diagnostic?.groupProof;
    if(diagnostic)moduleRecord.instrumentedSha256=hash(diagnostic.bytes);
    const generated=instantiateCore(diagnostic?new WebAssembly.Module(diagnostic.bytes):module,{ring3:{...support,memory,read:api.core_read,write:api.core_write,
      numeric:(...args)=>{
        helpers++;const before=version(),returned=api.core_numeric(...args);
        result.numericCalls.push({before:String(before),after:String(version()),returned});return returned;
      }}});
    run=stateWitness?generated.exports.link:generated.exports.run;
    publish=stateWitness?generated.exports.publish:undefined;
    witnessReader=diagnostic?()=>diagnostic.read(generated,support,word(context,92),
      witnessData?{pointer:physical(witnessData[0]),length:witnessData[1]}:undefined):undefined;
    assert.equal(typeof run,'function','the selected generated entry is an actual Wasm function');
    if(stateWitness)assert.equal(typeof publish,'function','linked state has an explicit canonical publisher');
  };
  try {
    status('open',0,12,...KEY);opened=true;engine.base=api.arena_ptr()>>>0;
    const code=new Uint8Array(4096),dataPage=new Uint8Array(4096);data(dataPage);
    for(const[pc,value]of segments)code.set(unhex(value),pc-0x1000);
    const pages=new Map([[0x1000,code],[0x3000,dataPage],[0x4000,new Uint8Array(4096)],[0x5000,new Uint8Array(4096)]]);
    for(const address of missing)pages.delete(address);
    for(const[address,page]of pages){status('map',0,address,1,address===0x1000?7:3);engine.request(page);status('upload',0,address,4096);}
    const state=record('R3ST',56),s=view(state);
    registers.forEach((value,index)=>s.setUint32(16+4*index,value,true));s.setUint32(48,entry,true);s.setUint32(52,initialFlags,true);
    new Uint8Array(memory.buffer,engine.base,56).set(state);
    new DataView(memory.buffer).setUint16(engine.base+FP+16,control,true);
    if(initialFp)new Uint8Array(memory.buffer,engine.base+FP,128).set(fp(bytes(engine.base+FP,128),initialFp));
    const startingFp=bytes(engine.base+FP,128),startingSimd=bytes(engine.base+SIMD,160),startingYmm=bytes(engine.base+YMM,144);
    let publishedFp=startingFp,publishedSimd=startingSimd;
    const expectedPages=new Map([...pages].map(([address,page])=>[address,page.slice()]));
    prepare(entry);
    status('core_begin',0);active=true;status('core_end',0);active=false;
    const primaryEncoded=encodedModule;
    const writeExpected=(address,value)=>{
      const input=unhex(value);
      for(let index=0;index<input.length;index++)expectedPages.get((address+index)&~4095)[(address+index)&4095]=input[index];
    };
    const check=({pc,retired,flags=2,ecx=registers[1],fp:expectedFp,simd:expectedSimd=startingSimd,stop=2,budget,work=budget,remaining=0,
      fault=[0,0,0],regs={},continueNeedCode=true})=>{
      const beforeHelpers=helpers,activations=[];
      let actual,available=budget;
      do {
        assert(activations.length<=budget+1,'bounded genuine NeedCode continuations');
        const startPc=word(context,32),beforeRetired=word(context,40);
        status('core_begin',0);active=true;
        const beforeBankHex=stateWitness?hex(bytes(word(context,92),100)):undefined;
        const beforeCounters=witnessReader?.();
        actual=run(context,available)>>>0;
        if(stateWitness){
          const after=witnessReader(),counters=Object.fromEntries(Object.entries(after).map(([key,value])=>[key,value-beforeCounters[key]]));
          const witness={...counters,beforeBankHex,afterBankHex:hex(bytes(word(context,92),100))};
          (result.stateWitnesses??=[]).push(witness);checkpoint();
          // Inspect the real internal execution before explicit publication can conceal a
          // transient bank write. The ordinary full architectural oracle still runs below.
          publish(context);
        }
        status('core_end',0);active=false;
        const nextPc=word(context,32),left=word(context,44),completed=word(context,40)-beforeRetired;
        activations.push({startPc,nextPc,stop:actual,budget:available,remaining:left,retired:completed});
        assert(left<=available,'each activation preserves its remaining quota');
        if(actual===1&&continueNeedCode) {
          assert(left>0,'NeedCode retains unconsumed work');
          assert(available>left||nextPc!==startPc,'NeedCode makes observable guest progress');
          available=left;prepare(nextPc);
        }
      }while(actual===1&&continueNeedCode);
      const actualRegisters=Array.from({length:8},(_,index)=>word(context,index*4));
      const wantedRegisters=registers.slice();wantedRegisters[1]=ecx;
      for(const[index,value]of Object.entries(regs))wantedRegisters[Number(index)]=value;
      const observed={stop:actual,pc:word(context,32),flags:word(context,36),retired:word(context,40),
        remaining:word(context,44),fault:[word(context,48),word(context,52),word(context,116)],registers:actualRegisters,
        helperCalls:helpers-beforeHelpers,activations};
      result.runs.push(observed);checkpoint();
      assert.equal(actual,stop,'exact stop reason');assert.equal(observed.pc,pc,'next unretired guest PC');
      assert.equal(observed.retired,retired,'cumulative completed instruction prefix');
      assert.equal(observed.remaining,remaining,'exact unconsumed instruction budget');
      assert.equal(observed.flags,flags,'all integer flags');assert.deepEqual(actualRegisters,wantedRegisters,'all GPRs');
      assert.deepEqual(observed.fault,fault,'fault address, access metadata and vector');
      assert.deepEqual(bytes(engine.base+FP,128),publishedFp,'private execution does not publish FP state');
      assert.deepEqual(bytes(engine.base+SIMD,160),publishedSimd,'private execution does not publish SIMD state');
      status('core_export',0,stop,work);
      publishedFp=expectedFp;publishedSimd=expectedSimd;observed.fpHex=hex(bytes(engine.base+FP,128));checkpoint();
      assert.deepEqual(bytes(engine.base+FP,128),expectedFp,'full independent literal FP128');
      assert.deepEqual(bytes(engine.base+SIMD,160),expectedSimd,'full independent literal SIMD160');
      assert.deepEqual(bytes(engine.base+YMM,144),startingYmm,'full upper vector bank is unchanged');
      for(const[address,page]of expectedPages)assert.deepEqual(bytes(physical(address),4096),page,`entire RAM page ${address.toString(16)}`);
      return observed;
    };
    body({api,engine,bytes,word,result,prepare,check,writeExpected,registers,startingFp,startingSimd,
      fp:value=>fp(startingFp,value),status,context:()=>context,version,
      inspect:()=>{result.structure=inspectRegion(primaryEncoded,coreSupportSource(memory));checkpoint();return result.structure;},
      portDepth:()=>{result.portDepth=inspectPortDepth(primaryEncoded);checkpoint();return result.portDepth;},
      groupProof:()=>groupWitnessProof,
      stateWitness:()=>{assert.equal(result.stateWitnesses?.length,1,'one observed linked activation');return result.stateWitnesses[0];}});
    result.complete=true;checkpoint();
  }catch(error){result.failure={message:String(error),stack:error.stack};checkpoint();throw error;}
  finally{if(active)api.core_end();if(opened)api.close();}
}

const cfg=[[0x1000,'d903 85c0 740a d84b04 eb08'],[0x1010,'d84304 49 75ec d95b08 0f0b']];
const exchangeCfg=[[0x1000,'d903 d94304 85c0 7407 d9c9 eb05'],
  [0x1010,'d9c9 d8c1 49 75ee d95b08 d95b0c 0f0b']];
const loaded={status:0x3800,tag:0x3fff,opcode:0x103,fip:0x1000,fdp:0x3000,registers:{7:EXT.three}};
const addFive={...loaded,opcode:0x43,fip:0x1010,fdp:0x3004,registers:{7:EXT.five}};
const addSeven={...addFive,registers:{7:EXT.seven}};
const multiplySix={...loaded,opcode:0x4b,fip:0x1006,fdp:0x3004,registers:{7:EXT.six}};
const multiplyTwelve={...multiplySix,registers:{7:EXT.twelve}};
const traces=[
  {name:'taken-add',eax:0,stored:'0000e040',steps:[
    [0x1002,2,2,loaded],[0x1004,0x46,2,loaded],[0x1010,0x46,2,loaded],
    [0x1013,0x46,2,addFive],[0x1014,2,1,addFive],[0x1002,2,1,addFive],
    [0x1004,0x46,1,addFive],[0x1010,0x46,1,addFive],[0x1013,0x46,1,addSeven],
    [0x1014,0x46,0,addSeven],[0x1016,0x46,0,addSeven],
    [0x1019,0x46,0,{opcode:0x15b,fip:0x1016,fdp:0x3008,registers:{7:EXT.seven}}],
  ]},
  {name:'fallthrough-multiply',eax:1,stored:'00004041',steps:[
    [0x1002,2,2,loaded],[0x1004,2,2,loaded],[0x1006,2,2,loaded],
    [0x1009,2,2,multiplySix],[0x1013,2,2,multiplySix],[0x1014,2,1,multiplySix],
    [0x1002,2,1,multiplySix],[0x1004,2,1,multiplySix],[0x1006,2,1,multiplySix],
    [0x1009,2,1,multiplyTwelve],[0x1013,2,1,multiplyTwelve],[0x1014,0x46,0,multiplyTwelve],
    [0x1016,0x46,0,multiplyTwelve],
    [0x1019,0x46,0,{opcode:0x15b,fip:0x1016,fdp:0x3008,registers:{7:EXT.twelve}}],
  ]},
];

function exchangeCase(name,eax,structural=false) {
  fixture(name,{segments:exchangeCfg,registers:[eax,2,0,0x3000,0x4800,0,0,0]},t=>{
    t.writeExpected(0x3008,'0000e040');t.writeExpected(0x300c,'0000a040');
    const retired=eax===0?16:18;
    t.check({pc:0x101d,retired,budget:retired,flags:0x46,ecx:0,
      fp:t.fp({opcode:0x15b,fip:0x101a,fdp:0x300c,registers:{6:EXT.seven,7:EXT.five}})});
    if(structural)assertRegionStructure(t.inspect());
  });
}

try {
  if(mode==='--entry'||mode==='--entry-baseline') {
    stage='structured-entry-routing';runEntryCases(fixture);checkpoint();
  }else if(mode==='--entry-nested') {
    stage='nested-entry-routing';runEntryNestedCases(fixture);checkpoint();
  }else if(mode==='--entry-unmatched') {
    stage='unmatched-entry-routing';runEntryUnmatchedCases(fixture);checkpoint();
  }else if(mode?.startsWith('--execution')) {
    const cases={'--execution':runExecutionCases,'--execution-flags':runExecutionFlagsCases,
      '--execution-loads':runExecutionLoadCases,'--execution-guards':runExecutionGuardCases,
      '--execution-multiply':runExecutionMultiplyCases,'--execution-flags-oracle':runExecutionFlagsOracleCases};
    stage='semantic-value-effect-lowering';cases[mode](fixture);checkpoint();
  }else if(mode==='--transport'||mode==='--transport-baseline') {
    if(mode==='--transport-baseline') {
      const path=new URL('../../../../../artifacts/cpu-core/region-redesign/pool-v14-cache.mjs',import.meta.url);
      report.baselineSdkSha256=hash(readFileSync(path));
      artifact.CodeCache=(await import(path.href)).CodeCache;
    }
    stage='selective-transport-linked';runTransportCases(artifact,report);checkpoint();
    stage='selective-transport-local-restart';runTransportLoopCases(fixture);checkpoint();
  }else if(mode==='--pool'||mode==='--pool-baseline') {
    if(mode==='--pool-baseline') {
      const path=new URL('../../../../../artifacts/cpu-core/region-redesign/single-v12-cache.mjs',import.meta.url);
      report.baselineSdkSha256=hash(readFileSync(path));
      artifact.CodeCache=(await import(path.href)).CodeCache;
    }
    stage='physical-pool-linked';runPoolCases(artifact,report);checkpoint();
    stage='physical-pool-local-loop';runPoolLoopCases(fixture);checkpoint();
  }else if(mode==='--native-stores') {
    stage='native-value-stores';runNativeStoreCases(fixture);checkpoint();
  }else if(mode==='--groups') {
    stage='proved-memory-groups';runGroupCases(fixture);checkpoint();
  }else if(mode==='--graph') {
    stage='immutable-warmed-graph';runStableGraphCase(artifact,report);checkpoint();
  }else if(mode==='--structure') {
    fixture('emitted-cfg-keeps-arithmetic-local-through-the-loop',{segments:cfg},t=>{
      t.writeExpected(0x3008,'0000e040');
      t.check({pc:0x1019,retired:12,budget:12,flags:0x46,ecx:0,
        fp:t.fp({opcode:0x15b,fip:0x1016,fdp:0x3008,registers:{7:EXT.seven}})});
      assertRegionStructure(t.inspect());
    });
    exchangeCase('emitted-cfg-keeps-two-exchanged-values-local-through-the-loop',0,true);
    runRenamingCase(fixture,assertAnalyzedStateStructure);
  }else {
  for(const trace of traces)for(let prefix=1;prefix<=trace.steps.length;prefix++) {
    fixture(`cfg-${trace.name}-budget-${prefix}`,{segments:cfg,registers:[trace.eax,2,0,0x3000,0x4800,0,0,0]},t=>{
      const[pc,flags,ecx,wantedFp]=trace.steps[prefix-1];
      if(prefix===trace.steps.length)t.writeExpected(0x3008,trace.stored);
      t.check({pc,flags,ecx,retired:prefix,budget:prefix,fp:t.fp(wantedFp)});
      if(prefix<trace.steps.length) {
        t.status('core_import',0);t.prepare(pc);t.writeExpected(0x3008,trace.stored);
        const[endPc,endFlags,endEcx,endFp]=trace.steps.at(-1);
        t.check({pc:endPc,flags:endFlags,ecx:endEcx,retired:trace.steps.length,
          budget:trace.steps.length-prefix,fp:t.fp(endFp)});
      }
    });
  }
  fixture('cfg-negative-zero-survives-backedge-and-pop',{
    segments:[[0x1000,'d903 85c0 740a d84304 eb08'],[0x1010,'d84b04 49 75ec d95b08 0f0b']],
    data:page=>{view(page).setUint32(0,0x80000000,true);view(page).setFloat32(4,2,true);},
  },t=>{
    t.writeExpected(0x3008,'00000080');
    t.check({pc:0x1019,retired:12,budget:12,flags:0x46,ecx:0,
      fp:t.fp({opcode:0x15b,fip:0x1016,fdp:0x3008,registers:{7:EXT.negativeZero}})});
  });
  exchangeCase('cfg-exchange-keeps-two-values-through-taken-diamond-and-pops',0);
  exchangeCase('cfg-exchange-keeps-two-values-through-fallthrough-diamond-and-pops',1);
  fixture('cfg-arbitrary-full80-payload-survives-backedge-and-pop',{
    segments:[[0x1000,'85c0 740c 0f0b'],[0x1010,'49 75ed db7b08 0f0b']],
    initialFp:{tag:0xfffc,registers:{0:'0100000000000080ff3f'}},
  },t=>{
    t.writeExpected(0x3008,'0100000000000080ff3f');
    t.check({pc:0x1016,retired:9,budget:9,flags:0x46,ecx:0,
      fp:t.fp({status:0x800,opcode:0x37b,fip:0x1013,fdp:0x3008,
        registers:{0:'0100000000000080ff3f'}})});
  });
  fixture('cfg-control-word-change-precedes-exact-arithmetic',{
    segments:[[0x1000,'d903 85c0 740a 0f0b'],[0x1010,'d96b0c d84304 d95b08 0f0b']],
    data:page=>{view(page).setFloat32(0,3,true);view(page).setFloat32(4,2,true);view(page).setUint16(12,0x37f,true);},
  },t=>{
    t.writeExpected(0x3008,'0000a040');
    const wanted=t.fp({opcode:0x15b,fip:0x1016,fdp:0x3008,registers:{7:EXT.five}});
    view(wanted).setUint16(16,0x37f,true);
    t.check({pc:0x1019,retired:6,budget:6,flags:0x46,fp:wanted});
  });
  fixture('cfg-fxsave-observes-popped-physical-payload',{
    segments:[[0x1000,'d903 85c0 740a d84b04 eb08'],[0x1010,'d84304 49 75ec d95b08 0fae0500500000 0f0b']],
  },t=>{
    t.writeExpected(0x3008,'0000e040');
    const image=new Uint8Array(464),v=view(image);
    v.setUint16(0,0x27f,true);v.setUint16(6,0x15b,true);
    v.setUint32(8,0x1016,true);v.setUint16(12,0x1b,true);
    v.setUint32(16,0x3008,true);v.setUint16(20,0x23,true);
    v.setUint32(24,0x1f80,true);v.setUint32(28,0xffff,true);
    image.set(unhex(EXT.seven),32+7*16);t.writeExpected(0x5000,hex(image));
    t.check({pc:0x1020,retired:13,budget:13,flags:0x46,ecx:0,
      fp:t.fp({opcode:0x15b,fip:0x1016,fdp:0x3008,registers:{7:EXT.seven}})});
  });
  fixture('cfg-late-read-fault-keeps-the-earlier-stack-prefix',{
    segments:[[0x1000,'d903 85c0 740a 0f0b'],[0x1010,'d802 0f0b']],
    registers:[0,2,0x3ffe,0x3000,0x4800,0,0,0],missing:[0x4000],
  },t=>{
    t.check({pc:0x1010,retired:3,budget:8,work:3,remaining:5,flags:0x46,stop:3,
      fault:[0x4000,0x10104,0x40e],fp:t.fp(loaded)});
  });
  fixture('cfg-late-store-fault-keeps-all-completed-loop-effects',{
    segments:[[0x1000,'d903 85c0 740a d84b04 eb08'],[0x1010,'d84304 49 75ec d91d00600000 0f0b']],
  },t=>{
    t.check({pc:0x1016,retired:11,budget:16,work:11,remaining:5,flags:0x46,ecx:0,stop:3,
      fault:[0x6000,0x10204,0x40e],fp:t.fp(addSeven)});
  });
  fixture('cfg-pending-unmasked-exception-precedes-unmapped-memory',{
    segments:[[0x1000,'d902 85c0 740a 0f0b'],[0x1010,'d84304 0f0b']],
    registers:[0,2,0x6000,0x3000,0x4800,0,0,0],control:0x27e,initialFp:{status:1},
  },t=>{
    t.check({pc:0x1000,retired:0,budget:8,work:0,remaining:8,stop:9,
      fault:[0x8081,0,16],fp:t.fp({status:0x8081})});
  });
  fixture('cfg-code-store-commits-once-and-rejects-stale-reentry',{
    segments:[[0x1000,'d903 85c0 740a d84b04 eb08'],[0x1010,'d84304 49 75ec d91d00110000 0f0b']],
  },t=>{
    t.writeExpected(0x1100,'0000e040');
    t.check({pc:0x101c,retired:12,budget:16,work:12,remaining:4,flags:0x46,ecx:0,stop:6,
      fp:t.fp({opcode:0x11d,fip:0x1016,fdp:0x1100,registers:{7:EXT.seven}})});
    t.status('core_begin',4);
  });
  fixture('scalar-and-canonical-numeric-stores-share-one-write-sequence',{
    segments:[[0x1000,'894310 d903 db7b20 894314 0f0b']],
    registers:[0x12345678,2,0,0x3000,0x4800,0,0,0],
  },t=>{
    const before=t.version();
    t.writeExpected(0x3010,'78563412');t.writeExpected(0x3020,EXT.three);t.writeExpected(0x3014,'78563412');
    t.check({pc:0x100b,retired:4,budget:4,
      fp:t.fp({opcode:0x37b,fip:0x1005,fdp:0x3020,registers:{7:EXT.three}})});
    assert.equal(t.version(),before+3n,'all three completed stores advance the shared write sequence');
    assert.deepEqual(t.result.numericCalls.map(call=>[call.before,call.after]),
      [[String(before+1n),String(before+2n)]],'the canonical helper sees the first write and commits exactly its own write');
  });
  fixture('faulting-numeric-store-observes-only-the-completed-scalar-prefix',{
    segments:[[0x1000,'894310 d903 d84b04 d91d00600000 0f0b']],
    registers:[0x12345678,2,0,0x3000,0x4800,0,0,0],
  },t=>{
    const before=t.version();t.writeExpected(0x3010,'78563412');
    t.check({pc:0x1008,retired:3,budget:8,work:3,remaining:5,stop:3,
      fault:[0x6000,0x10204,0x40e],
      fp:t.fp({status:0x3800,tag:0x3fff,opcode:0x4b,fip:0x1005,fdp:0x3004,registers:{7:EXT.six}})});
    assert.equal(t.version(),before+1n,'a failed numeric store does not advance the write sequence');
    assert.deepEqual(t.result.numericCalls.map(call=>[call.before,call.after]),
      [[String(before+1n),String(before+1n)]],'the failed canonical helper observes the preceding scalar commit');
  });
  runControlCases(fixture);
  runStatusCases(fixture);
  runStateCases(fixture);
  stage='compiled-link-boundaries';runLinkedCases(artifact,report);checkpoint();
  stage='compiled-link-status';runLinkedStatusCases(artifact,report);checkpoint();
  stage='compiled-link-short-quota';runLinkedQuotaCase(artifact,report);checkpoint();
  stage='compiled-source-ownership';runOwnershipCase(artifact,report);checkpoint();
  stage='immutable-interior-ports';runPortCases(fixture);checkpoint();
  stage='immutable-warmed-graph';runStableGraphCase(artifact,report);checkpoint();
  stage='proved-memory-groups';runGroupCases(fixture);checkpoint();
  stage='native-value-stores';runNativeStoreCases(fixture);checkpoint();
  stage='physical-pool-linked';runPoolCases(artifact,report);checkpoint();
  stage='physical-pool-local-loop';runPoolLoopCases(fixture);checkpoint();
  }
  report.complete=true;checkpoint();console.log(JSON.stringify({complete:true,cases:report.cases.length,engineSha256:artifact.sha256}));
}catch(error){report.failure={stage,message:String(error),stack:error.stack};checkpoint();throw error;}
