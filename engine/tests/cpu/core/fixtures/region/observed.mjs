import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {Engine,KEY,record,view} from '../../../../../../sdk/runtime/engine.js';
import {CodeCache as CurrentCodeCache} from '../../../../../../sdk/runtime/cache.js';
import {readGeneratedModule} from './structure.mjs';
import {instrumentWitnessModule,readWitness,setWitnessBank,witnessCounters} from './witness.mjs';
import {instrumentObservedModule,readObservedWitness} from './observed-witness.mjs';

const FP=4236,SIMD=4364,YMM=4612,ROOT=0x1000,CALLEE=0x1100;
const THREE='00000000000000c00040',TWO='00000000000000800040';
const FULL80='01000000000000c00040',ZERO='00000000000000000000';
const unhex=value=>Buffer.from(value.replaceAll(' ',''),'hex');
const hash=value=>createHash('sha256').update(value).digest('hex');
const sum=instances=>Object.fromEntries(witnessCounters.map(name=>[name,instances.reduce((total,instance)=>
  name.endsWith('_mask')?total|readWitness(instance)[name]:total+readWitness(instance)[name],0)]));
const difference=(after,before)=>Object.fromEntries(witnessCounters.map(name=>[name,
  name.endsWith('_mask')?after[name]:after[name]-before[name]]));

// The warm-up is an ordinary execution of these literal programs. Recording
// supplies the candidate graph, never the expected architectural results.
const programs={
  loop:{root:'e8fb000000 83c001 49 75f5 90 0f0b',callee:'d8c1 c3',perIteration:6,end:0x100b,returnPc:0x1005},
  repeated:{root:'e8fb000000 e8f6000000 49 75f3 90 0f0b',callee:'d8c1 c3',perIteration:8,end:0x100d,returnPc:0x100a},
  changedReturn:{root:'e8fb000000 83c001 49 75f5 90 0f0b',callee:'891424 d8c1 c3',
    perIteration:7,end:0x100b,returnPc:0x1005},
  recursive:{root:'e8fb000000 90 0f0b',callee:'49 7407 e8f8ffffff d8c1 c3',
    trainingBudget:15,end:0x1005,returnPc:0x1005},
  ordinal:{root:'d92b dc4308 e8f6000000 83c001 49 75f0 90 0f0b',callee:'d8c1 c3',
    perIteration:8,end:0x1010,returnPc:0x100a,canonicalPerIteration:3},
};

function recorder(world) {
  const pointer=world.api.core_trace_status_ptr?.()??world.api.core_trace_pointer?.();
  assert(Number.isInteger(pointer)&&pointer>0&&pointer%8===0,'the real native recorder has a stable aligned arena');
  const d=new DataView(world.memory.buffer,pointer,32),used=d.getUint32(4,true),limit=d.getUint32(8,true);
  assert.equal(limit,129,'the compiler observes a bounded source-owned recording');assert(used<=limit);
  assert(pointer+32+24*limit<=world.memory.buffer.byteLength);
  return{pointer,active:d.getUint32(0,true),used,limit,overflow:d.getUint32(12,true),
    bypassIdentity:String(d.getBigUint64(16,true)),bypassRetired:d.getUint32(24,true),bypassPending:d.getUint32(28,true),
    events:Array.from({length:used},(_,i)=>{
      const e=new DataView(world.memory.buffer,pointer+32+i*24,24);
      return{kind:e.getUint32(0,true),pc:e.getUint32(4,true),end:e.getUint32(8,true),
        offset:e.getUint32(12,true),owner:String(e.getBigUint64(16,true))};
    })};
}

function fixture(artifact,report,name,options,body) {
  const result={name,complete:false,controls:[],modules:[],runs:[],numericCalls:[],selections:[]};report.cases.push(result);
  const instance=new WebAssembly.Instance(artifact.module,{ring3_observability:{now_ns:()=>0n}});
  const raw=Object.fromEntries(Object.entries(instance.exports).filter(([name])=>name.startsWith('ring3_abi_v1_'))
    .map(([name,value])=>[name.slice(13),value]));
  const memory=instance.exports.memory,bytes=(pointer,length)=>new Uint8Array(memory.buffer).slice(pointer,pointer+length);
  const word=(pointer,offset=0)=>new DataView(memory.buffer).getUint32(pointer+offset,true);
  const bigint=(pointer,offset=0)=>new DataView(memory.buffer).getBigUint64(pointer+offset,true);
  const setWord=(pointer,offset,value)=>new DataView(memory.buffer).setUint32(pointer+offset,value,true);
  let cache,engine,opened=false;
  const diagnostics=[],owners=[],units=new Map();let prepares=0,probes=0,helpers=0;
  const api={...raw,
    core_prepare:(...args)=>{prepares++;return raw.core_prepare(...args);},
    core_numeric:(context,ordinal)=>{
      helpers++;const call={context,ordinal,pc:word(context,32),identity:String(bigint(context,120))};
      result.numericCalls.push(call);call.returned=raw.core_numeric(context,ordinal);
      const bank=word(context,92);call.fip=word(bank,8);
      call.opcode=new DataView(memory.buffer).getUint16(bank+6,true);return call.returned;
    },
    core_chain_probe:(...args)=>{probes++;const returned=raw.core_chain_probe(...args);
      result.selections.push({operation:'probe',args,returned});return returned;},
    ...(typeof raw.core_chain_select==='function'?{core_chain_select:(...args)=>{
      probes++;const returned=raw.core_chain_select(...args);result.selections.push({operation:'select',args,returned});return returned;
    }}:{}),
  };
  const status=(operation,wanted,...args)=>{const actual=raw[operation](...args)>>>0;
    result.controls.push({operation,wanted,actual,args});assert.equal(actual,wanted,`${name}: ${operation}`);return actual;};
  try{
    engine=new Engine(api,memory,artifact.sha256);status('open',0,12,...KEY);opened=true;engine.base=raw.arena_ptr()>>>0;
    const program=programs[options.program??'loop'],code=new Uint8Array(4096),data=new Uint8Array(4096);
    code.set(unhex(program.root));code.set(unhex(program.callee),0x100);
    if(program===programs.ordinal)view(data).setUint16(0,0x37f,true);
    for(const[address,page]of[[ROOT,code],[0x3000,data],[0x4000,new Uint8Array(4096)]]){
      status('map',0,address,1,address===ROOT?7:3);engine.request(page);status('upload',0,address,4096);
    }
    const makeState=(iterations=3,{pc=ROOT,esp=0x4800,edx=program.returnPc}={})=>{
      const state=record('R3ST',56),s=view(state),registers=[0,iterations,edx,0x3000,esp,0x12345678,0x87654321,0x99aabbcc];
      registers.forEach((value,i)=>s.setUint32(16+i*4,value,true));s.setUint32(48,pc,true);s.setUint32(52,2,true);return state;
    };
    const state=makeState();new Uint8Array(memory.buffer,engine.base,56).set(state);
    const d=new DataView(memory.buffer,engine.base+FP,128);
    for(const[offset,value]of[[16,options.canonical?0x37f:0x27f],[18,0x200],
      [20,options.canonical?0xfff4:0xfff0],[22,0x5a5],[32,0x33],[34,0x5b]])d.setUint16(offset,value,true);
    d.setUint32(24,0x11223344,true);d.setUint32(28,0x55667788,true);
    [options.canonical?FULL80:THREE,options.canonical?ZERO:TWO,
      '00000000000000000080','0000000000000040ff3f','00000000000000c0ff7f',
      '000000000000008000c0','01000000000000c00040','00000000000000000000']
      .forEach((rawValue,i)=>new Uint8Array(memory.buffer,engine.base+FP+40+10*i,10).set(unhex(rawValue)));
    const initialFp=bytes(engine.base+FP,128),initialSimd=bytes(engine.base+SIMD,160),initialYmm=bytes(engine.base+YMM,144);
    const Cache=artifact.CodeCache??CurrentCodeCache;
    cache=new Cache(engine,{budget:program.trainingBudget??program.perIteration*3+1,instructions:128,blocks:32,chain:true});
    const encoded=()=>bytes(raw.core_code_ptr()>>>0,raw.core_code_len()>>>0);
    const select=pc=>{const unit=cache.entry(pc),source=encoded();
      if(!units.has(unit.id)){const module={pc,id:String(unit.id),slot:unit.slot,bytes:source.length,sha256:hash(source),source};
        result.modules.push(module);units.set(unit.id,{unit,module});}
      return unit;
    };
    // Seed the callee first so a source frontier, not a policy knob, keeps the
    // two baseline owners independent throughout recording and overlay use.
    const callee=select(CALLEE),root=select(ROOT);assert.notEqual(callee.id,root.id);
    // The first preparation creates the private Context at the callee PC.
    // Later preparations select code without rewriting architectural state.
    cache.import();
    status('core_begin',0);status('core_end',0);
    const context=cache.context,bank=word(context,92);
    const install=()=>{
      const dispatchSource=bytes(raw.core_dispatch_code_ptr()>>>0,raw.core_dispatch_code_len()>>>0);
      if(!diagnostics.length){
        const parsed=readGeneratedModule(dispatchSource),diagnostic=instrumentWitnessModule(dispatchSource,parsed,true);
        const probeImport=parsed.imports.find(item=>item.name==='probe');assert(probeImport,'real dispatcher probe import');
        const probeType=parsed.types[probeImport.type];
        const probe=probeType.parameters.length===3?api.core_chain_select:api.core_chain_probe;
        assert.equal(typeof probe,'function','the exact dispatcher selection ABI has its native capability');
        const support=new WebAssembly.Instance(new WebAssembly.Module(diagnostic.bytes),{ring3:{memory,
          functions:cache.table,publishers:cache.publishers,probe,cold:raw.core_chain_cold}}).exports;
        if(support['trace.arena']){
          assert(support['trace.arena'] instanceof WebAssembly.Global);
          support['trace.arena'].value=raw.core_trace_pointer();
        }
        cache.dispatch=support;diagnostics.push(support);setWitnessBank(support,bank);
        result.supportProof=diagnostic.proof;
      }
      for(const{unit,module}of units.values())if(!module.instrumentedSha256){
        const diagnostic=instrumentObservedModule(module.source);
        const bindings={memory,read:api.core_read,write:api.core_write,numeric:api.core_numeric,
          ...cache.dispatch,'support.abi':cache.dispatch.supportAbi};
        const generated=new WebAssembly.Instance(new WebAssembly.Module(diagnostic.bytes),{ring3:bindings}).exports;
        Object.assign(unit,{run:generated.run,link:generated.link,publish:generated.publish});
        cache.table.set(unit.slot,unit.link);cache.publishers.set(unit.slot,unit.publish);
        diagnostics.push(generated);owners.push(generated);setWitnessBank(generated,bank);
        module.instrumentedSha256=hash(diagnostic.bytes);module.proof=diagnostic.proof;
      }
    };
    install();
    const world={artifact,result,raw,api,memory,bytes,word,bigint,setWord,status,engine,cache,context,bank,
      program,root,callee,units,select,encoded,install,diagnostics,initialFp,initialSimd,initialYmm,makeState,
      get prepares(){return prepares;},get probes(){return probes;},get helpers(){return helpers;},
      physical:address=>{const slot=word(word(context,56)+(address>>>12)*4);assert(slot>0);return word(context,64)+(slot-1)*4096+(address&4095);},
      revision:()=>bigint(word(context,80)),
      counters:()=>sum(diagnostics),
      routing:()=>owners.reduce((total,owner)=>{const row=readObservedWitness(owner);
        for(const name of Object.keys(total))total[name]+=row[name];return total;},{lookup_entries:0,frame_commits:0}),
      reset(iterations=9,changes={}){
        const next=makeState(iterations,changes);new Uint8Array(memory.buffer,engine.base,56).set(next);
        for(const[offset,image]of[[FP,initialFp],[SIMD,initialSimd],[YMM,initialYmm]])
          new Uint8Array(memory.buffer,engine.base+offset,image.length).set(image);
        cache.import();return next;
      },
      snapshot(){return{context:bytes(context,128),state:bytes(engine.base,56),fp:bytes(engine.base+FP,128),
        simd:bytes(engine.base+SIMD,160),ymm:bytes(engine.base+YMM,144),
        ram:bytes(word(context,64),word(context,68)*4096),pages:bytes(word(context,84),word(context,68)*16),
        revision:bigint(word(context,80)),counters:sum(diagnostics),routing:this.routing(),probes,prepares,helpers};},
      rawExecute(unit,budget){
        const retired=word(context,40);status('core_begin',0);let stop;
        try{stop=unit.run(context,budget)>>>0;}finally{status('core_end',0);}
        const work=budget-word(context,44);status('core_export',0,stop,work);
        return{coreReason:stop,pc:word(context,32),retired:word(context,40)-retired,work,
          id:bigint(context,120),regions:1,kind:'compiled-standalone'};
      },
    };
    body(world);
    for(const module of result.modules)delete module.source;
    result.complete=true;
  }catch(error){for(const module of result.modules)delete module.source;
    result.failure={message:String(error),stack:error.stack};throw error;}
  finally{if(cache?.open)raw.core_end();if(opened)status('close',0);}
}

function literal(world,before,run,{pc,retired,budget,registers,flags=2,writes=0,stackReturn,stackWrites={},
  fpValue,fpPc,fpOpcode=0xc1,fpStatus=0,fpData,fpDs,stop=2,fault=[0,0,0],identity=run.id,helpers=0}) {
  const{context,word,bytes,engine,result}=world;
  assert.deepEqual([run.coreReason,run.pc,run.retired,run.work,word(context,44)],
    [stop,pc,retired,retired,budget-retired],'literal completed instruction prefix and quota');
  const wantedContext=before.context.slice(),c=view(wantedContext);
  registers.forEach((value,i)=>c.setUint32(i*4,value,true));c.setUint32(32,pc,true);c.setUint32(36,flags,true);
  c.setUint32(40,view(before.context).getUint32(40,true)+retired,true);c.setUint32(44,budget-retired,true);
  for(const[offset,value]of[[48,fault[0]],[52,fault[1]],[112,0],[116,fault[2]]])c.setUint32(offset,value,true);
  c.setBigUint64(120,identity,true);
  assert.deepEqual(bytes(context,128),wantedContext,'all 128 private context bytes, including unchanged authority fields');
  const wantedState=before.state.slice(),s=view(wantedState);
  registers.forEach((value,i)=>s.setUint32(16+i*4,value,true));s.setUint32(48,pc,true);s.setUint32(52,flags,true);
  assert.deepEqual(bytes(engine.base,56),wantedState,'the complete public GPR/PC/FLAGS image');
  const wantedFp=before.fp.slice(),f=view(wantedFp);
  if(fpValue!==undefined){wantedFp.set(unhex(fpValue),40);f.setUint16(18,fpStatus,true);f.setUint16(22,fpOpcode,true);
    f.setUint32(24,fpPc,true);f.setUint16(32,0x1b,true);}
  if(fpData!==undefined)f.setUint32(28,fpData,true);
  if(fpDs!==undefined)f.setUint16(34,fpDs,true);
  assert.deepEqual(bytes(engine.base+FP,128),wantedFp,'all physical payloads, empty ghosts, independent tags and x87 metadata');
  assert.deepEqual(bytes(engine.base+SIMD,160),before.simd);assert.deepEqual(bytes(engine.base+YMM,144),before.ymm);
  const wantedRam=before.ram.slice(),wantedPages=before.pages.slice();
  for(const[address,value]of Object.entries({...stackWrites,...(stackReturn===undefined?{}:{[0x47fc]:stackReturn})})){
    const physical=world.physical(Number(address))-word(context,64);view(wantedRam).setUint32(physical,value,true);}
  if(writes){const slot=word(word(context,56)+(0x4000>>>12)*4)-1;
    view(wantedPages).setBigUint64(slot*16+8,before.revision+BigInt(writes),true);}
  assert.deepEqual(bytes(word(context,64),wantedRam.length),wantedRam,'the entire mapped code/data/stack backing');
  assert.equal(world.revision(),before.revision+BigInt(writes));
  assert.deepEqual(bytes(word(context,84),wantedPages.length),wantedPages,'all mapping and content version records');
  assert.equal(world.helpers-before.helpers,helpers,'the independent exact canonical callback count');
  result.runs.push({...run,id:String(run.id),semanticComplete:true,contextHex:Buffer.from(wantedContext).toString('hex'),
    fpHex:Buffer.from(wantedFp).toString('hex'),ramSha256:hash(wantedRam),revision:String(world.revision()),
    counters:difference(world.counters(),before.counters),
    routing:Object.fromEntries(Object.entries(world.routing()).map(([name,value])=>[name,value-before.routing[name]])),
    probes:world.probes-before.probes});
  return result.runs.at(-1);
}

function train(world,{baseline=false,finish=true}={}) {
  const{cache,program,raw,status,result}=world;
  if(!baseline){for(const name of['core_trace_start','core_trace_finish'])assert.equal(typeof raw[name],'function');
    status('core_trace_start',0,ROOT);result.recordingBefore=recorder(world);assert.equal(result.recordingBefore.active,1);}
  const before=world.snapshot(),run=cache.execute(world.root),registers=Array.from({length:8},(_,i)=>view(before.state).getUint32(16+i*4,true));
  const repeated=program===programs.repeated,recursive=program===programs.recursive,
    numericCount=3*(program.canonicalPerIteration??1);
  registers[0]=repeated||recursive?0:3;registers[1]=0;
  const budget=program.trainingBudget??program.perIteration*3+1;
  literal(world,before,run,{pc:program.end+1,retired:budget,budget,
    registers,flags:0x46,writes:repeated||program===programs.changedReturn?6:3,stackReturn:program.returnPc,
    stackWrites:recursive?{[0x47f8]:0x1108,[0x47f4]:0x1108}:{},
    fpValue:world.result.canonical?FULL80:recursive?'00000000000000e00140':
      repeated?'00000000000000f00240':'00000000000000900240',
    fpPc:recursive?0x1108:program===programs.changedReturn?0x1103:CALLEE,
    fpData:program===programs.ordinal?0x3008:undefined,fpDs:program===programs.ordinal?0x23:undefined,
    helpers:world.result.canonical?numericCount:0});
  if(!baseline){result.recording=recorder(world);assert.equal(result.recording.overflow,0);
    assert(result.recording.used>3,'the actual normal compiled path records more than a marker');
    assert(result.recording.events.some(event=>event.owner===String(world.root.id)));
    assert(result.recording.events.some(event=>event.owner===String(world.callee.id)));
    if(finish){status('core_trace_finish',0,ROOT,128,32);result.recordingAfter=recorder(world);assert.equal(result.recordingAfter.active,0);}}
}

function assertOwners(world,baseOwners) {
  for(const{unit,module}of baseOwners){
    const baselineEntry=module.pc===ROOT?ROOT+5:module.pc;
    world.status('core_lookup',0,baselineEntry);
    assert.equal(world.bigint(world.context,120),unit.id,'public baseline entry never resolves to an ambiguous trace interior');
    assert.equal(hash(world.encoded()),module.sha256,'overlay compilation leaves baseline source-owner bytes immutable');
    assert.equal(world.cache.units.get(unit.id),unit,'the original actual SDK instance remains retained');
  }
}

export function runObservedBaselineCases(artifact,report) {runObservedCases(artifact,report,{baseline:true});}

export function runObservedCases(artifact,report,{baseline=false}={}) {
  const checks=[],traffic=[];
  for(const iterations of[1,9])fixture(artifact,report,`observed-call-ret-one-body-${iterations}`,{},world=>{
    train(world,{baseline});world.reset(iterations);
    const baseOwners=[...world.units.values()],selected=world.select(ROOT);world.install();
    const before=world.snapshot();world.cache.budget=6*iterations;
    const run=world.cache.execute(selected),registers=Array.from({length:8},(_,i)=>view(before.state).getUint32(16+i*4,true));
    registers[0]=iterations;registers[1]=0;
    const completed=literal(world,before,run,{pc:0x100b,retired:6*iterations,budget:6*iterations,
      registers,flags:0x46,writes:iterations,stackReturn:0x1005,
      fpValue:iterations===1?'00000000000000a00140':'00000000000000a80340',fpPc:CALLEE});
    world.result.semanticComplete=true;
    traffic.push(completed);
    if(!baseline){assert.notEqual(selected.id,world.root.id);assert.notEqual(selected.id,world.callee.id);
      assert.equal(world.cache.admissions,3,'only one bounded overlay is added to the two immutable baseline owners');
      assertOwners(world,baseOwners);}
    checks.push(()=>{
      assert.equal(completed.counters.body_entries,1,'the observed CALL/callee/RET/continuation loop executes one actual instruction body');
      assert.equal(completed.probes,0,'matching observed call and return edges never leave through the dispatcher');
      assert.equal(completed.routing.lookup_entries,1,'only the initial bootstrap uses the actual PC lookup');
      assert.equal(completed.routing.frame_commits,1,'matching observed edges do not commit/reimport the FP pool');
      assert.equal(completed.counters.wide_snapshot_calls,0);assert.equal(completed.counters.normalizer_calls,0);
      assert.equal(run.regions,1,'all completed guest instructions belong to one actual Wasm activation');
      assert.equal(world.prepares-before.prepares,0,'executing the selected overlay never prepares another owner');
    });
  });
  // Both pinned-v19 cases complete their independent architectural oracles
  // before the first desired new-mechanism assertion is allowed to fail.
  for(const check of checks)check();
  for(const name of['record_reads','record_writes'])assert.equal(traffic[1].counters[name],traffic[0].counters[name],
    'eight more observed CALL/RET iterations add no physical FP imports or spills');
}

export function runObservedPrefixCases(artifact,report) {
  for(const budget of[0,1])fixture(artifact,report,`observed-call-ret-quota-prefix-${budget}`,{},world=>{
    train(world);world.reset(9);const selected=world.select(ROOT);world.install();
    const before=world.snapshot();world.cache.budget=budget;const run=world.cache.execute(selected);
    const registers=Array.from({length:8},(_,i)=>view(before.state).getUint32(16+i*4,true));if(budget)registers[4]-=4;
    const completed=literal(world,before,run,{pc:budget?CALLEE:ROOT,retired:budget,budget,registers,
      writes:budget,stackReturn:budget?0x1005:undefined});
    world.result.semanticComplete=true;assert.equal(completed.counters.body_entries,1);assert.equal(completed.probes,0);
  });
}

export function runObservedRepeatedCases(artifact,report) {
  fixture(artifact,report,'observed-two-call-sites-share-one-callee-pc-and-guard-both-real-return-targets',{
    program:'repeated'},world=>{
    train(world);world.reset(2);const selected=world.select(ROOT);world.install();
    const before=world.snapshot();world.cache.budget=16;const run=world.cache.execute(selected);
    const registers=Array.from({length:8},(_,i)=>view(before.state).getUint32(16+i*4,true));registers[1]=0;
    const completed=literal(world,before,run,{pc:0x100d,retired:16,budget:16,registers,flags:0x46,writes:4,
      stackReturn:0x100a,fpValue:'00000000000000b00240',fpPc:CALLEE});
    world.result.semanticComplete=true;assert.equal(completed.counters.body_entries,1);assert.equal(completed.probes,0);
  });
  fixture(artifact,report,'observed-bounded-recursion-shares-pcs-with-real-x86-stack-transactions',{
    program:'recursive'},world=>{
    train(world);world.reset(2);const selected=world.select(ROOT);world.install();
    const before=world.snapshot();world.cache.budget=9;const run=world.cache.execute(selected);
    const registers=Array.from({length:8},(_,i)=>view(before.state).getUint32(16+i*4,true));registers[1]=0;
    const completed=literal(world,before,run,{pc:0x1005,retired:9,budget:9,registers,flags:0x46,writes:2,
      stackReturn:0x1005,stackWrites:{[0x47f8]:0x1108},fpValue:'00000000000000a00140',fpPc:0x1108});
    world.result.semanticComplete=true;assert.equal(completed.counters.body_entries,1);assert.equal(completed.probes,0);
  });
}

export function runObservedCanonicalCases(artifact,report) {
  fixture(artifact,report,'observed-canonical-callee-uses-overlay-native-ordinal-authority',{
    canonical:true},world=>{
    world.result.canonical=true;train(world);world.reset(2);const selected=world.select(ROOT);world.install();
    const before=world.snapshot();world.cache.budget=12;const run=world.cache.execute(selected);
    const registers=Array.from({length:8},(_,i)=>view(before.state).getUint32(16+i*4,true));registers[0]=2;registers[1]=0;
    const completed=literal(world,before,run,{pc:0x100b,retired:12,budget:12,registers,flags:0x46,writes:2,
      stackReturn:0x1005,fpValue:FULL80,fpPc:CALLEE,helpers:2});
    for(const call of world.result.numericCalls.slice(-2))assert.equal(call.identity,String(selected.id),
      'the real canonical callback executes under the overlay capability, not the prior callee owner');
    world.result.semanticComplete=true;assert.equal(completed.counters.body_entries,1);assert.equal(completed.probes,0);
    assert.equal(completed.counters.canonical_calls,2);
  });
  fixture(artifact,report,'observed-canonical-callee-ordinal-differs-from-both-baseline-owner-dictionaries',{
    canonical:true,program:'ordinal'},world=>{
    world.result.canonical=true;train(world);
    const baseline=world.result.numericCalls.filter(call=>call.identity===String(world.callee.id));
    assert.equal(baseline.length,3);for(const call of baseline){
      assert.equal(call.ordinal,0,'the independently compiled callee really uses native ordinal zero');
      assert.equal(call.identity,String(world.callee.id));assert.equal(call.opcode,0xc1);assert.equal(call.returned,0);
    }
    world.reset(2);const selected=world.select(ROOT);world.install();
    const before=world.snapshot(),first=world.result.numericCalls.length;world.cache.budget=16;
    const run=world.cache.execute(selected),registers=Array.from({length:8},(_,i)=>view(before.state).getUint32(16+i*4,true));
    registers[0]=2;registers[1]=0;
    const completed=literal(world,before,run,{pc:0x1010,retired:16,budget:16,registers,flags:0x46,writes:2,
      stackReturn:0x100a,fpValue:FULL80,fpPc:CALLEE,fpData:0x3008,fpDs:0x23,helpers:6});
    const callbacks=world.result.numericCalls.slice(first);
    // These operations require the canonical boundary independently of how the
    // observed graph partitions blocks: FLDCW has no direct lowering, memory
    // arithmetic admits only float32, and register arithmetic rejects PC64.
    // FLDCW itself preserves historical FIP/FDP; the double-zero addition
    // records the exact data address without rounding this full80 payload.
    assert.deepEqual(callbacks.map(call=>call.fip),[0x11223344,0x1002,CALLEE,CALLEE,0x1002,CALLEE],
      'control loads preserve historical FIP while real computations publish their authored PCs');
    assert.deepEqual(callbacks.map(call=>call.ordinal),[0,1,2,0,1,2],
      'the same callee operation resolves through the overlay dictionary ordinal two');
    for(const call of callbacks){assert.equal(call.identity,String(selected.id));assert.equal(call.returned,0);}
    world.result.semanticComplete=true;assert.equal(completed.counters.body_entries,1);assert.equal(completed.probes,0);
    assert.equal(completed.counters.canonical_calls,6,'one real callback per exact canonical operation remains');
  });
}

export function runObservedExitCases(artifact,report) {
  fixture(artifact,report,'observed-changed-return-exits-after-real-stack-read-and-pop',{
    program:'changedReturn'},world=>{
    train(world);world.reset(9,{edx:0x1800});const selected=world.select(ROOT);world.install();
    const before=world.snapshot(),run=world.rawExecute(selected,10);
    const registers=Array.from({length:8},(_,i)=>view(before.state).getUint32(16+i*4,true));
    const completed=literal(world,before,run,{stop:1,pc:0x1800,retired:4,budget:10,registers,writes:2,
      stackReturn:0x1800,fpValue:'00000000000000a00140',fpPc:0x1103});
    world.result.semanticComplete=true;assert.equal(completed.counters.body_entries,1);
    assert.equal(completed.probes,0,'a standalone side exit exposes the real target before any host continuation');
    world.result.bypass=recorder(world);
  });

  fixture(artifact,report,'observed-first-call-stack-fault-keeps-unexecuted-prefix-and-fp-state',{},world=>{
    train(world);world.reset(9,{esp:0x6000});const selected=world.select(ROOT);world.install();
    const before=world.snapshot();world.cache.budget=4;const run=world.cache.execute(selected);
    const registers=Array.from({length:8},(_,i)=>view(before.state).getUint32(16+i*4,true));
    const completed=literal(world,before,run,{stop:3,pc:ROOT,retired:0,budget:4,registers,
      fault:[0x5ffc,0x10204,14]});
    world.result.semanticComplete=true;assert.equal(completed.counters.body_entries,1);assert.equal(completed.probes,0);
  });

  fixture(artifact,report,'observed-root-only-entry-zero-prefix-bypass-needs-actual-retirement-progress',{},world=>{
    train(world);world.reset(9);const selected=world.select(ROOT);world.install();
    // A public baseline callee entry is not an external entry to this root-only
    // overlay. Selection remains the actual overlay capability for this test.
    world.setWord(world.context,32,CALLEE);
    const before=world.snapshot(),run=world.rawExecute(selected,3);
    const registers=Array.from({length:8},(_,i)=>view(before.state).getUint32(16+i*4,true));
    literal(world,before,run,{stop:1,pc:CALLEE,retired:0,budget:3,registers});
    const bypass=recorder(world);world.result.bypass=bypass;
    assert.equal(bypass.bypassIdentity,String(selected.id));assert.equal(bypass.bypassPending,1);
    assert.equal(bypass.bypassRetired,world.word(world.context,40));
    world.setWord(world.context,32,ROOT);
    const baseline=world.select(ROOT);assert.equal(baseline.id,world.root.id,
      'a zero-prefix trace side exit cannot immediately reselect that same overlay');
    world.install();const nextBefore=world.snapshot();world.cache.budget=1;
    const next=world.cache.execute(baseline),nextRegisters=registers.slice();nextRegisters[4]-=4;
    literal(world,nextBefore,next,{pc:CALLEE,retired:1,budget:1,registers:nextRegisters,writes:1,stackReturn:0x1005});
    world.setWord(world.context,32,ROOT);
    const resumed=world.select(ROOT);assert.equal(resumed.id,selected.id,
      'actual retired guest work permits selecting the unchanged overlay again');
    world.result.semanticComplete=true;
  });
}

function unchanged(world,before) {
  const after=world.snapshot();
  for(const key of['context','state','fp','simd','ymm','ram','pages'])assert.deepEqual(after[key],before[key],
    `a rejected native preparation preserves the complete ${key} image`);
  assert.equal(after.revision,before.revision);assert.equal(after.helpers,before.helpers);
  assert.deepEqual(after.counters,before.counters);
}

export function runObservedRefusalCases(artifact,report) {
  fixture(artifact,report,'observed-instruction-bound-decline-is-neutral-and-keeps-baseline-owners',{},world=>{
    train(world,{finish:false});const before=world.snapshot(),owners=[...world.units.values()];
    world.status('core_trace_finish',18,ROOT,1,32);unchanged(world,before);
    assert.equal(world.cache.admissions,2);assert.equal(world.cache.units.size,2);
    assertOwners(world,owners);assert.equal(world.select(ROOT).id,world.root.id);
    world.result.semanticComplete=true;
  });

  fixture(artifact,report,'observed-recording-source-change-declines-before-overlay-admission',{},world=>{
    train(world,{finish:false});
    const page=world.bytes(world.physical(ROOT),4096);page[0]=0x90;
    world.engine.request(page);world.status('upload',0,ROOT,page.length);
    const before=world.snapshot();world.status('core_trace_finish',18,ROOT,128,32);unchanged(world,before);
    assert.equal(world.cache.admissions,2);assert.equal(world.cache.units.size,2);
    world.result.semanticComplete=true;
  });

  fixture(artifact,report,'observed-stale-overlay-and-recycled-slot-never-authorize-old-function',{},world=>{
    train(world);world.reset(2);const overlay=world.select(ROOT);world.install();
    const old={identity:overlay.id,slot:overlay.slot,source:world.encoded()};
    const page=world.bytes(world.physical(ROOT),4096);page[0x0b]=0x91; // NOP becomes XCHG EAX,ECX after the observed loop.
    world.engine.request(page);world.status('upload',0,ROOT,page.length);
    const stale=world.snapshot();world.status('core_begin',4);unchanged(world,stale);
    world.status('core_lookup',4,ROOT);
    const replacement=world.select(ROOT);world.install();assert.notEqual(replacement.id,old.identity);
    assert.notEqual(hash(world.encoded()),hash(old.source));
    world.status('core_unlink',4,old.slot,Number(old.identity&0xffffffffn),Number(old.identity>>32n));
    world.status('core_begin',0);
    try{
      const before=world.bytes(world.context,128),publicState=world.bytes(world.engine.base,56),fp=world.bytes(world.engine.base+FP,128);
      assert.equal(overlay.run(world.context,1)>>>0,8,'an old overlay function cannot execute under a different selected identity');
      assert.deepEqual(world.bytes(world.context,128),before,'identity rejection leaves even the active private context unchanged');
      assert.deepEqual(world.bytes(world.engine.base,56),publicState);assert.deepEqual(world.bytes(world.engine.base+FP,128),fp);
    }finally{world.status('core_end',0);}
    world.result.semanticComplete=true;
  });
}

export {fixture as observedFixture,literal as observedLiteral};
