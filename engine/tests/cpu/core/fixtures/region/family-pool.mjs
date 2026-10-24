import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {coldFixture,coldLiteral} from '../../../../support/core_wasm/cold_observed.mjs';
import {readGeneratedModule} from './structure.mjs';
import {instrumentWitnessModule,readWitness,setWitnessBank,resetWitnessMasks,witnessCounters} from './witness.mjs';
import {cacheCallbacks,cacheResolver,bindCacheContinuation} from '../link.mjs';

const A=0x1000,B=0x1040,C=0x1080;
const ODD='0008000000000080ff3f',UP='0010000000000080ff3f';
const unhex=text=>Uint8Array.from(Buffer.from(text.replaceAll(' ',''),'hex'));
const hex=bytes=>Buffer.from(bytes).toString('hex');
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
const sum=exports=>Object.fromEntries(witnessCounters.map(name=>[name,exports.reduce((n,item)=>
  name.endsWith('_mask')?n|readWitness(item)[name]:n+readWitness(item)[name],0)]));
const difference=(after,before)=>Object.fromEntries(witnessCounters.map(name=>[name,
  name.endsWith('_mask')?after[name]:after[name]-before[name]]));

function image(initial,{control=0x27f,status=0x220,tag=0xfff6,opcode=0x5a5,
  fip=0x11223344,fdp=0x55667788,cs=0x33,ds=0x5b,registers={}}={}) {
  const result=initial.slice(),d=new DataView(result.buffer,result.byteOffset,result.byteLength);
  for(const[offset,value]of[[16,control],[18,status],[20,tag],[22,opcode],[32,cs],[34,ds]])d.setUint16(offset,value,true);
  d.setUint32(24,fip,true);d.setUint32(28,fdp,true);
  for(const[index,value]of Object.entries(registers))result.set(unhex(value),40+Number(index)*10);
  return result;
}

function pointer(world,pc) {
  const bytes=new Uint8Array(4);new DataView(bytes.buffer).setUint32(0,pc,true);
  world.engine.request(bytes);world.engine.ok('upload',0x4800,4);
}

function stable(world,units) {
  for(const unit of units.values()) {
    const retained=unit.retained;
    assert.equal(world.cache.units.get(unit.id),unit);
    for(const name of['run','link','publish'])assert.equal(unit[name],retained[name]);
    assert.equal(unit.slot,retained.slot);
    assert.equal(world.cache.table.get(unit.slot),unit.link);
    assert.equal(world.cache.publishers.get(unit.slot),unit.publish);
    assert.equal(world.bigint(world.raw.core_link_owners_ptr()>>>0,unit.slot*8),unit.id);
    for(const pc of unit.ports) {
      assert.equal(world.raw.core_lookup(pc),0);
      assert.equal(world.bigint(world.context,120),unit.id);
      const source=world.bytes(world.raw.core_code_ptr()>>>0,world.raw.core_code_len()>>>0);
      assert.equal(hash(source),retained.sha256);assert.equal(source.length,retained.bytes);
    }
  }
}

function instrument(world,units,probes) {
  const diagnostics=[],bank=world.word(world.context,92),dispatch=world.cache.dispatch;
  const source=world.bytes(world.raw.core_dispatch_code_ptr()>>>0,world.raw.core_dispatch_code_len()>>>0);
  const shared=instrumentWitnessModule(source,readGeneratedModule(source),true);
  let previous;
  const probe=(context,pc,retired)=>{
    const after=sum(diagnostics),pool=world.bytes(bank+512,384);
    probes.push({pc,retired,poolHex:hex(pool),canonicalHex:hex(world.bytes(bank,100)),
      active:world.word(bank,512),changed:world.word(bank,512+76),pending:world.word(bank,512+40),
      counters:difference(after,previous)});
    for(const item of diagnostics)resetWitnessMasks(item);
    previous=sum(diagnostics);
    return cacheResolver(world.cache)(context,pc,retired);
  };
  const support=new WebAssembly.Instance(new WebAssembly.Module(shared.bytes),{ring3:{memory:world.memory,
    functions:world.cache.table,publishers:world.cache.publishers,probe,cold:world.api.core_chain_cold}}).exports;
  // Preserve the production mutable bindings while reinstalling both measured tables.
  for(const[name,value]of Object.entries(dispatch))if(value instanceof WebAssembly.Global&&support[name] instanceof WebAssembly.Global) {
    if(name.startsWith('link.')||name==='trace.arena')support[name].value=value.value;
    else if(name==='linkAbi'||name==='supportAbi')assert.equal(support[name].value,value.value,'instrumentation preserves immutable ABI values');
  }
  if(support['trace.arena'])support['trace.arena'].value=world.raw.core_trace_pointer()>>>0;
  bindCacheContinuation(world.cache,support);
  world.cache.dispatch=support;diagnostics.push(support);
  for(const unit of units.values()) {
    const record=world.moduleRecords.get(unit.id);assert(record?.source);
    const measured=instrumentWitnessModule(record.source,readGeneratedModule(record.source));
    const generated=new WebAssembly.Instance(new WebAssembly.Module(measured.bytes),{ring3:{memory:world.memory,
      read:world.api.core_read,write:world.api.core_write,...cacheCallbacks(world.cache),
      ...support,'support.abi':support.supportAbi,'recovery.base':unit.recoveryBase}}).exports;
    Object.assign(unit,{run:generated.run,link:generated.link,publish:generated.publish});
    world.cache.table.set(unit.slot,unit.link);world.cache.publishers.set(unit.slot,unit.publish);
    unit.retained={...record,slot:unit.slot,run:unit.run,link:unit.link,publish:unit.publish};
    diagnostics.push(generated);
  }
  for(const item of diagnostics)setWitnessBank(item,bank);
  previous=sum(diagnostics);
  return{read:()=>sum(diagnostics),proof:shared.proof};
}

export function runFamilyPoolCases(artifact,report,{ordinaryOnly=false}={}) {
  if(ordinaryOnly){run(artifact,report,'ordinary',1);return;}
  for(const bridge of['ordinary','cpuid'])for(const iterations of[1,9])run(artifact,report,bridge,iterations);
  run(artifact,report,'control',1);
}

function run(artifact,report,bridge,iterations) {
  const code=new Uint8Array(4096);
  // A leaves an empty raw80 ghost and the unresolved upward midpoint recipe.
  code.set(unhex('d9c0 d84304 ddd8 ffe7'));
  // B observes C1 before FXCH clears it, then restores both odd inputs.
  code.set(unhex('dfe0 d9c9 ddd1 4d 7403 ff2424 90 0f0b'),0x40);
  code.set(unhex(bridge==='cpuid'?'b8ffffffff 0fa2 bb00300000 ffe6':bridge==='control'?'d96b08 ffe6':'90 ffe6'),0x80);
  const registers=[0xa5a55a5a,0x11223344,0x55667788,0x3000,0x4800,128,B,C];
  coldFixture(artifact,report,'cold-built-family-pool-'+bridge+'-bridge-'+iterations,{
    code,registers,data:d=>{d.setFloat32(0,3.5,true);d.setUint32(4,0x25000000,true);d.setUint16(8,0x37f,true);},
  },world=>{
    const initial=image(world.initialFp,{registers:{0:ODD,1:ODD,
      2:'00000000000000000080',3:'0000000000000040ff3f',4:'00000000000000c0ff7f',
      5:'01000000000000c00040',6:'000000000000008000c0',7:'00000000000000000080'}});
    pointer(world,B);
    const trains=[
      {pc:A,budget:512,registers:[...registers.slice(0,7),A],flags:2,
        fp:image(initial,{status:0x220,tag:0xfff6,opcode:0x5d8,fip:A+5,fdp:0x3004,cs:0x1b,ds:0x23,
          registers:{7:UP}})},
      {pc:C,budget:bridge==='cpuid'?512:256,
        registers:bridge==='cpuid'?[0,0,0,0x3000,0x4800,128,C,C]:[...registers.slice(0,6),C,C],flags:2,
        fp:bridge==='control'?image(initial,{control:0x37f}):initial},
      {pc:B,budget:768,registers:[0xa5a50020,...registers.slice(1,5),0,B,C],flags:0x46,
        fp:image(initial,{status:0x20,tag:0xfff0,opcode:0x5d1,fip:B+4,cs:0x1b})},
    ];
    const units=new Map();
    for(const train of trains) {
      const inputs=train.pc===A?[...registers.slice(0,7),A]:train.pc===C?[...registers.slice(0,6),C,C]:registers;
      world.reset({pc:train.pc,registers:inputs,flags:2,fp:initial});
      const before=world.snapshot(),result=world.execute(train.budget,train.pc);
      coldLiteral(world,before,result,{pc:train.pc===B?B+13:train.pc,retired:train.budget,budget:train.budget,
        registers:train.registers,flags:train.flags,fp:train.fp,writes:[],versionWrites:0,
        identity:world.bigint(world.context,120)});
      const unit=world.cache.select(train.pc);assert(unit,'normal cold heat installs a real family');
      units.set(train.pc,unit);
    }
    world.result.trainingSemanticComplete=true;
    assert.equal(new Set([...units.values()].map(unit=>unit.id)).size,3);
    assert.equal(world.cache.admissions,3);assert.equal(world.cache.units.size,3);
    units.get(A).ports=[A,A+2,A+5,A+7];
    units.get(B).ports=[B,B+2,B+4,B+6,B+7,B+9];
    units.get(C).ports=bridge==='cpuid'?[C,C+5,C+7,C+12]:bridge==='control'?[C,C+3]:[C,C+1];

    pointer(world,A);
    const wantedRegisters=bridge==='cpuid'?[0x220,0,0,0x3000,0x4800,0,B,C]
      :[0xa5a50220,registers[1],registers[2],0x3000,0x4800,0,B,C];
    const wanted=image(initial,{control:bridge==='control'?0x37f:0x27f,status:0x20,tag:0xfff0,
      opcode:0x5d1,fip:B+4,fdp:0x3004,cs:0x1b,ds:0x23,registers:{0:ODD,1:ODD,7:UP}});
    // The final authored stack upload may invalidate validation certificates
    // cached while training the separate families. Execute and verify one real
    // complete traversal before claiming that a subsequent traversal is warm.
    const learningInputs=[...registers.slice(0,5),1,B,C],learningBudget=(bridge==='cpuid'?14:12)-1;
    world.reset({pc:A,registers:learningInputs,flags:2,fp:initial});
    const learningBefore=world.snapshot(),learning=world.execute(learningBudget,A);
    coldLiteral(world,learningBefore,learning,{pc:B+12,retired:learningBudget,budget:learningBudget,
      registers:wantedRegisters,flags:0x46,fp:wanted,writes:[],versionWrites:0});
    assert.equal(learning.id,units.get(B).id);assert.equal(learning.regions,3);
    world.result.learningSemanticComplete=true;
    const inputs=[...registers.slice(0,5),iterations,B,C];
    // Reset only public CPU/FP input; no source or guest-memory upload occurs
    // between the verified learning traversal and this measured traversal.
    world.reset({pc:A,registers:inputs,flags:2,fp:initial});
    const probes=[],witness=instrument(world,units,probes),before=world.snapshot();
    const bank=world.word(world.context,92),initialBank=hex(world.bytes(bank,100));
    const baseline={admissions:world.cache.admissions,bytes:world.cache.generatedModuleBytes,
      cold:world.cache.coldRuns,recording:world.recording()};
    const helpers=world.helperCalls.length,budget=(bridge==='cpuid'?14:12)*iterations-1;
    const result=world.execute(budget,A),measured=witness.read();
    coldLiteral(world,before,result,{pc:B+12,retired:budget,budget,registers:wantedRegisters,
      flags:0x46,fp:wanted,writes:[],versionWrites:0});
    assert.equal(result.id,units.get(B).id);assert.equal(result.regions,3*iterations);
    world.result.semanticComplete=true;
    world.result.poolWitness=measured;world.result.poolProbes=probes;world.result.supportProof=witness.proof;
    world.result.families=[...units.entries()].map(([pc,unit])=>({pc,id:String(unit.id),slot:unit.slot,
      ports:unit.ports,sha256:unit.retained.sha256,bytes:unit.retained.bytes}));

    // Independent full-state semantics above precede every new mechanism assertion.
    assert.equal(world.cache.admissions,baseline.admissions);
    assert.equal(world.cache.generatedModuleBytes,baseline.bytes);assert.equal(world.cache.units.size,3);
    assert.equal(world.cache.coldRuns,baseline.cold);
    assert.equal(probes.length,world.cache.continuationCore?0:3*iterations-1,
      'validated warm continuations resolve in generated code; only actual native misses create probes');
    assert.equal(measured.body_entries,3*iterations);
    assert.equal(measured.normalizer_calls,0);assert.equal(measured.wide_snapshot_calls,0);
    assert.equal(world.helperCalls.length-helpers,bridge==='ordinary'?0:iterations);
    for(const call of world.helperCalls.slice(helpers)) {
      assert.equal(BigInt(call.identity),units.get(C).id,'callback authority is the selected scalar family');
      assert.equal(call.ordinal,0);
    }
    for(let i=0;i<probes.length;i++) {
      const current=probes[i],stage=i%3;
      assert.equal(current.pc,[C,B,A][stage]);
      if(stage===0) {
        assert.equal(current.active,1);assert(current.changed&0x80,'empty ghost7 is committed');
        assert.equal(current.pending,1,'the completed addition retains its unresolved C1 recipe');
      }
      if(bridge==='control'&&stage===1) {
        assert.equal(current.active,0,'a true control operation relinquishes the pool');
        assert.equal(current.pending,0);assert.equal(current.counters.materializer_calls,1);
        assert.notEqual(current.canonicalHex,initialBank);
      }else {
        assert.equal(current.counters.materializer_calls,0,'family ingress and pure scalar callbacks do not materialize');
        assert.equal(current.canonicalHex,initialBank,'canonical bytes remain stale until a real observer');
        if(stage===1) {
          assert.equal(current.poolHex,probes[i-1].poolHex,'the complete authoritative pool survives the scalar bridge');
          for(const name of['record_reads','record_writes','header_reads','header_writes'])assert.equal(current.counters[name],0);
        }
      }
    }
    assert.equal(measured.materializer_calls,bridge==='control'?2:1,'only the true helper and final publisher materialize');
    assert.equal(world.recording().active,0);assert.equal(world.recording().used,baseline.recording.used);
    stable(world,units);
  });
}
