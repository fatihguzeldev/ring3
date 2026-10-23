import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {Engine,KEY,record,view} from '../../../../../../sdk/runtime/engine.js';
import {CodeCache as CurrentCodeCache} from '../../../../../../sdk/runtime/cache.js';
import {readGeneratedModule} from './structure.mjs';
import {instrumentWitnessModule,readWitness,setWitnessBank,resetWitnessMasks,witnessCounters} from './witness.mjs';

const FP=4236,SIMD=4364,YMM=4612;
const THREE='00000000000000c00040',TWO='00000000000000800040';
const ghosts=['01000000000000c00040','00000000000000000080','0000000000000040ff3f',
  '00000000000000c0ff7f','000000000000008000c0','00000000000000000080'];
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
const unhex=value=>Buffer.from(value,'hex');
const hex=bytes=>Buffer.from(bytes).toString('hex');
const sum=exports=>Object.fromEntries(witnessCounters.map(name=>[name,exports.reduce((n,item)=>
  name.endsWith('_mask')?n|readWitness(item)[name]:n+readWitness(item)[name],0)]));
const delta=(after,before)=>Object.fromEntries(witnessCounters.map(name=>[name,
  name.endsWith('_mask')?after[name]:after[name]-before[name]]));

export function runPoolCases(artifact,report) {
  for(const iterations of[1,9])runLinkedPoolCase(artifact,report,iterations);
}

// Both modes use the same literal semantic oracles. Only the explicitly pinned
// baseline supplies an archived CodeCache through artifact.CodeCache.
export {runPoolCases as runTransportCases,runPoolLoopCases as runTransportLoopCases};

function runLinkedPoolCase(artifact,report,iterations) {
  const result={name:`physical-pool-three-owners-select-only-used-slots-${iterations}`,complete:false,
    iterations,controls:[],modules:[],probes:[]};report.cases.push(result);
  const instance=new WebAssembly.Instance(artifact.module,{ring3_observability:{now_ns:()=>0n}});
  const raw=Object.fromEntries(Object.entries(instance.exports).filter(([key])=>key.startsWith('ring3_abi_v1_'))
    .map(([key,value])=>[key.slice(13),value]));
  const memory=instance.exports.memory,bytes=(pointer,length)=>new Uint8Array(memory.buffer).slice(pointer,pointer+length);
  const word=(pointer,offset=0)=>new DataView(memory.buffer).getUint32(pointer+offset,true);
  const budget=9*iterations+1,units=new Map(),diagnostics=[];
  const registers=[0xa5a55a5a,iterations,0x12345678,0x3000,0x4800,0x87654321,0x1040,0x1080];
  let cache,engine,opened=false,initialBank,previousCounters,helpers=0,prepares=0;
  const status=(name,wanted,...args)=>{const actual=raw[name](...args)>>>0;
    result.controls.push({name,wanted,actual});assert.equal(actual,wanted,`${result.name}: ${name}`);};
  const api={...raw,
    core_prepare:(...args)=>{prepares++;return raw.core_prepare(...args);},
    core_numeric:(...args)=>{helpers++;return raw.core_numeric(...args);},
    core_chain_cold:()=>assert.fail('the three declared source-current owners are retained'),
    core_chain_probe:(context,pc)=>{
      const index=result.probes.length,stage=index%3,cycle=Math.floor(index/3);
      assert.equal(context,cache.context);assert.equal(pc,[0x1080,0x1040,0x1000][stage]);
      assert(index<3*iterations-1,'the declared chain has a bounded exact number of links');
      const live=local=>{const value=cache.dispatch[`link.local${local}`];assert(value instanceof WebAssembly.Global);return value.value;};
      const retired=9*cycle+[4,6,9][stage],wantedRegisters=registers.slice();
      wantedRegisters[1]=iterations-cycle-Number(stage===2);
      assert.deepEqual(Array.from({length:8},(_,i)=>live(i+2)>>>0),wantedRegisters,'all linked GPRs');
      assert.deepEqual([live(10),live(12),live(13)],[pc,budget-retired,retired],'exact linked PC/quota/prefix');
      const canonicalBankHex=hex(bytes(word(context,92),100));
      const after=sum(diagnostics),observed=delta(after,previousCounters);
      result.probes.push({pc,source:['A','scalar-C','B'][stage],cycle,retired,counters:observed,canonicalBankHex});
      for(const item of diagnostics)resetWitnessMasks(item);
      previousCounters=sum(diagnostics);
      const slot=raw.core_chain_probe(context,pc)>>>0;
      assert.equal(slot,units.get(pc).slot,'the real native probe selects the declared owner');return slot;
    },
  };
  try{
    engine=new Engine(api,memory,artifact.sha256);status('open',0,12,...KEY);opened=true;engine.base=raw.arena_ptr()>>>0;
    const code=new Uint8Array(4096);
    code.set(unhex('d9c0ddd8d9c9ffe7')); // A: write empty ghost7, swap0/1, then scalar C.
    code.set(unhex('d9c94975bb900f0b'),0x40); // B: restore0/1; DEC/JNZ A; terminal NOP.
    code.set(unhex('90ffe6'),0x80); // scalar C: NOP; JMP ESI.
    for(const[address,page]of[[0x1000,code],[0x3000,new Uint8Array(4096)],[0x4000,new Uint8Array(4096)]]){
      status('map',0,address,1,address===0x1000?7:3);engine.request(page);status('upload',0,address,4096);
    }
    const state=record('R3ST',56),s=view(state);registers.forEach((value,i)=>s.setUint32(16+i*4,value,true));
    s.setUint32(48,0x1000,true);s.setUint32(52,2,true);new Uint8Array(memory.buffer,engine.base,56).set(state);
    const f=new DataView(memory.buffer,engine.base+FP,128);
    for(const[offset,value]of[[16,0x27f],[18,0x20],[20,0xfff6],[22,0x5a5],[32,0x33],[34,0x5b]])f.setUint16(offset,value,true);
    f.setUint32(24,0x11223344,true);f.setUint32(28,0x55667788,true);
    [THREE,TWO,...ghosts].forEach((value,i)=>new Uint8Array(memory.buffer,engine.base+FP+40+i*10,10).set(unhex(value)));
    const initialFp=bytes(engine.base+FP,128),initialSimd=bytes(engine.base+SIMD,160),initialYmm=bytes(engine.base+YMM,144);
    const Cache=artifact.CodeCache??CurrentCodeCache;
    cache=new Cache(engine,{budget,instructions:64,blocks:8,chain:true});
    for(const pc of[0x1000,0x1080,0x1040]){
      const unit=cache.entry(pc),source=bytes(raw.core_code_ptr()>>>0,raw.core_code_len()>>>0);
      units.set(pc,unit);result.modules.push({pc,id:String(unit.id),slot:unit.slot,sha256:hash(source),source});
    }
    assert.equal(new Set([...units.values()].map(unit=>unit.id)).size,3,'three independent actual owners');
    status('core_lookup',0,0x1000);status('core_begin',0);status('core_end',0);
    const context=cache.context,bank=word(context,92),initialContext=bytes(context,128);
    initialBank=bytes(bank,100);
    const backing=word(context,64),initialRam=bytes(backing,word(context,68)*4096);
    const initialPageVersions=bytes(word(context,84),word(context,68)*16);
    const revision=()=>new DataView(memory.buffer).getBigUint64(word(context,80),true),initialRevision=revision();

    // Reinstall instrumented real Wasm functions in the same native/SDK slots.
    // The callback ABI and actual source/identity lookup remain unchanged.
    const dispatchSource=bytes(raw.core_dispatch_code_ptr()>>>0,raw.core_dispatch_code_len()>>>0);
    const dispatchDiagnostic=instrumentWitnessModule(dispatchSource,readGeneratedModule(dispatchSource),true);
    const support=new WebAssembly.Instance(new WebAssembly.Module(dispatchDiagnostic.bytes),{ring3:{memory,
      functions:cache.table,publishers:cache.publishers,probe:api.core_chain_probe,cold:api.core_chain_cold}}).exports;
    cache.dispatch=support;diagnostics.push(support);result.supportProof=dispatchDiagnostic.proof;
    for(const module of result.modules){
      const diagnostic=instrumentWitnessModule(module.source,readGeneratedModule(module.source));
      const bindings={memory,read:api.core_read,write:api.core_write,numeric:api.core_numeric,
        ...support,'support.abi':support.supportAbi};
      const generated=new WebAssembly.Instance(new WebAssembly.Module(diagnostic.bytes),{ring3:bindings}).exports;
      const unit=units.get(module.pc);Object.assign(unit,{run:generated.run,link:generated.link,publish:generated.publish});
      cache.table.set(unit.slot,unit.link);cache.publishers.set(unit.slot,unit.publish);diagnostics.push(generated);
      module.instrumentedSha256=hash(diagnostic.bytes);module.proof=diagnostic.proof;delete module.source;
    }
    for(const item of diagnostics)setWitnessBank(item,bank);
    previousCounters=sum(diagnostics);const prepared=prepares;
    const first=units.get(0x1000),last=units.get(0x1040),run=cache.execute(first);
    result.run={...run,id:String(run.id)};result.counters=sum(diagnostics);

    // Full literal semantics precede every new-architecture requirement, so the
    // archived v12 engine must reach a real mechanism RED, not an oracle mismatch.
    assert.deepEqual([run.coreReason,run.pc,run.retired,run.work,word(context,44),run.regions],
      [2,0x1046,budget,budget,0,3*iterations]);assert.equal(run.id,last.id);
    const wantedContext=initialContext.slice(),c=view(wantedContext);
    c.setUint32(4,0,true);c.setUint32(32,0x1046,true);c.setUint32(36,0x46,true);
    c.setUint32(40,budget,true);c.setUint32(44,0,true);c.setBigUint64(120,last.id,true);
    assert.deepEqual(bytes(context,128),wantedContext,'the complete private Context128 is literal');
    const wantedState=state.slice(),w=view(wantedState);w.setUint32(20,0,true);w.setUint32(48,0x1046,true);w.setUint32(52,0x46,true);
    assert.deepEqual(bytes(engine.base,56),wantedState,'the complete public scalar state');
    const wantedFp=initialFp.slice(),fp=view(wantedFp);
    fp.setUint16(18,0x20,true);fp.setUint16(20,0xfff0,true);fp.setUint16(22,0x1c9,true);
    fp.setUint32(24,0x1040,true);fp.setUint16(32,0x1b,true);
    wantedFp.set(unhex(THREE),40);wantedFp.set(unhex(TWO),50);wantedFp.set(unhex(THREE),110);
    assert.deepEqual(bytes(engine.base+FP,128),wantedFp,'all eight payloads, inconsistent source tags, empty ghost and environment');
    assert.deepEqual(bytes(engine.base+SIMD,160),initialSimd);assert.deepEqual(bytes(engine.base+YMM,144),initialYmm);
    assert.deepEqual(bytes(backing,initialRam.length),initialRam,'the entire code/data/stack backing stays unchanged');
    assert.equal(revision(),initialRevision);assert.deepEqual(bytes(word(context,84),initialPageVersions.length),initialPageVersions);
    assert.equal(helpers,0);assert.equal(result.probes.length,3*iterations-1);assert.equal(prepares,prepared);
    assert.deepEqual([cache.admissions,cache.units.size,cache.transientAdmissions,cache.transientRuns],[3,3,0,0]);
    for(const module of result.modules){const unit=units.get(module.pc);assert.equal(cache.units.get(unit.id),unit);
      status('core_lookup',0,module.pc);assert.equal(new DataView(memory.buffer).getBigUint64(context+120,true),unit.id);
      assert.equal(hash(bytes(raw.core_code_ptr()>>>0,raw.core_code_len()>>>0)),module.sha256,'actual owner bytes stay immutable');}
    result.semanticComplete=true;

    assert.equal(result.counters.normalizer_calls,0,'physical ownership retires the actual recurring eight-slot normalizer');
    assert.equal(result.counters.wide_snapshot_calls,0,
      'ordinary compiled links never transport a full typed FP snapshot through owner or shared calls');
    for(const probe of result.probes)assert.equal(probe.canonicalBankHex,hex(initialBank),
      'compiled boundaries do not publish the canonical FP bank');
    const scalar=result.probes.filter(probe=>probe.source==='scalar-C');
    for(const probe of scalar)for(const field of['record_reads','record_writes','header_reads','header_writes'])
      assert.equal(probe.counters[field],0,'the actual scalar bridge never accesses FP pool state');
    for(const probe of result.probes){
      if(probe.source==='A'&&probe.cycle===0)continue; // one bounded initial eight-record import.
      if(probe.source==='A'){
        assert.equal(probe.counters.record_read_mask&~3,0,'A imports only its two used physical inputs');
        assert.equal(probe.counters.record_write_mask&~0x83,0,'A spills only physical0/1 and its empty ghost7');
        assert(probe.counters.record_write_mask&0x80,'the empty written ghost is really committed');
      }else if(probe.source==='B'){
        assert.equal(probe.counters.record_read_mask&~3,0);assert.equal(probe.counters.record_write_mask&~3,0);
      }
    }
    assert(result.counters.record_writes>0,'actual private record writes establish physical pool ownership');
    assert(result.counters.record_reads<=6*(4*iterations+16),'record reads scale with used slots plus one import/observer');
    assert(result.counters.record_writes<=6*(5*iterations+16),'record writes scale with changed slots plus one import/observer');
    result.final={fpHex:hex(wantedFp),contextHex:hex(wantedContext),ramSha256:hash(initialRam),revision:String(initialRevision)};
    result.complete=true;
  }catch(error){result.failure={message:String(error),stack:error.stack};throw error;}
  finally{if(cache?.open)raw.core_end();if(opened)status('close',0);}
}

export function runPoolLoopCases(fixture) {
  fixture('physical-pool-lazy-source-admission-preserves-captured-left-operand',{
    segments:[[0x1000,'d8c1 49 75fb 0f0b']],
    registers:[0,1,0,0x3000,0x4800,0,0,0],control:0x27f,
    initialFp:{tag:0xfff0,registers:{0:THREE,1:TWO}},
  },t=>{
    // The backedge predicts both native inputs, but their first-use pool
    // certificates are initially absent. Admitting ST1 must preserve ST0=3.
    const before=t.version(),run=t.check({pc:0x1002,retired:1,budget:1,
      fp:t.fp({tag:0xfff0,opcode:0xc1,fip:0x1000,
        registers:{0:'00000000000000a00140',1:TWO}})});
    assert.equal(run.activations.length,1);assert.equal(t.result.modules.length,1);
    assert.equal(run.helperCalls,0);assert.equal(t.version(),before);
  });
  const witnesses=[];
  for(const iterations of[1,9])fixture(`physical-pool-balanced-local-loop-spills-once-${iterations}`,{
    segments:[[0x1000,'d903 d94304 d9c9 d8c1 ddd8 ddd8 49 75f0 0f0b']],stateWitness:true,
    registers:[0,iterations,0,0x3000,0x4800,0,0,0],
  },t=>{
    const before=t.version(),run=t.check({pc:0x1010,retired:8*iterations,budget:8*iterations,flags:0x46,ecx:0,
      fp:t.fp({opcode:0x5d8,fip:0x100b,fdp:0x3004,registers:{6:'00000000000000a00140',7:TWO}})});
    assert.equal(run.activations.length,1);assert.equal(t.result.modules.length,1);assert.equal(run.helperCalls,0);assert.equal(t.version(),before);
    const witness=t.stateWitness();t.result.poolWitness=witness;
    assert.equal(witness.normalizer_calls,0,'the actual internal loop has no generic eight-slot rebase');
    assert.equal(witness.wide_snapshot_calls,0,'the ordinary local-loop exit never transports a full FP snapshot');
    assert.equal(witness.raw80_stores,0,'the canonical FP bank is not touched before the real observer');
    assert(witness.record_writes>0);witnesses.push(witness);
    if(witnesses.length===2)for(const field of['record_reads','record_writes'])
      assert.equal(witness[field],witnesses[0][field],'eight more local iterations add no private record loads or spills');
  });
  for(const [iterations,value]of[[1,'00000000000000a00140'],[9,'00000000000000a80340']])
    fixture(`physical-pool-computed-local-restart-without-wide-snapshot-${iterations}`,{
      segments:[[0x1000,'d8c1 49 740b ffe0'],[0x1010,'90 0f0b']],stateWitness:true,
      registers:[0x1000,iterations,0,0x3000,0x4800,0,0,0],control:0x27f,
      initialFp:{tag:0xfff0,registers:{0:THREE,1:TWO}},
    },t=>{
      // Eight nonfinal iterations (N=9) use the actual computed-PC lookup and
      // restart the retained body. The last JZ skips that jump and executes NOP.
      const before=t.version(),run=t.check({pc:0x1011,retired:4*iterations,budget:4*iterations,
        flags:0x46,ecx:0,fp:t.fp({tag:0xfff0,opcode:0xc1,fip:0x1000,
          registers:{0:value,1:TWO}})});
      assert.equal(run.activations.length,1);assert.equal(t.result.modules.length,1);
      assert.equal(run.helperCalls,0);assert.equal(t.version(),before);
      t.result.semanticComplete=true;
      const witness=t.stateWitness();t.result.poolWitness=witness;
      assert.equal(witness.body_entries,1,'computed restart resumes the same actual instruction body');
      assert.equal(witness.normalizer_calls,0,'computed restart never invokes an eight-slot normalizer');
      assert.equal(witness.wide_snapshot_calls,0,'computed restart and final exit never transport a full FP snapshot');
      assert.equal(witness.raw80_stores,0,'computed restart leaves the canonical FP bank private');
      assert.equal(witness.materializer_calls,0,'computed restart has no canonical observation');
      assert.equal(witness.afterBankHex,witness.beforeBankHex,'canonical bytes stay unchanged until the explicit observer');
    });
}
