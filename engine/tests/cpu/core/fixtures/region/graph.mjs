import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {Engine,KEY,record,view} from '../../../../../../sdk/runtime/engine.js';
import {CodeCache} from '../../../../../../sdk/runtime/cache.js';

const hash=bytes=>createHash('sha256').update(bytes).digest('hex');

export function runStableGraphCase(artifact,report) {
  const result={name:'warmed-two-owner-graph-keeps-immutable-bodies-after-4096-edges',complete:false,controls:[],modules:[]};
  report.cases.push(result);
  const instance=new WebAssembly.Instance(artifact.module,{ring3_observability:{now_ns:()=>0n}});
  const api=Object.fromEntries(Object.entries(instance.exports).filter(([key])=>key.startsWith('ring3_abi_v1_'))
    .map(([key,value])=>[key.slice(13),value]));
  const memory=instance.exports.memory,bytes=(pointer,length)=>new Uint8Array(memory.buffer).slice(pointer,pointer+length);
  const word=(pointer,offset=0)=>new DataView(memory.buffer).getUint32(pointer+offset,true);
  let cache,opened=false;
  const status=(name,wanted,...args)=>{const actual=api[name](...args)>>>0;result.controls.push({name,wanted,actual});assert.equal(actual,wanted);};
  try{
    const engine=new Engine(api,memory,artifact.sha256);status('open',0,12,...KEY);opened=true;engine.base=api.arena_ptr()>>>0;
    const code=new Uint8Array(4096);
    code.set(Buffer.from('83431001ffe6','hex')); // A: ADD [EBX+10],1; JMP ESI
    code.set(Buffer.from('834314024975b9900f0b','hex'),0x40); // B: ADD [EBX+14],2; DEC/JNZ A; NOP
    for(const[address,page]of[[0x1000,code],[0x3000,new Uint8Array(4096)],[0x4000,new Uint8Array(4096)]]){
      status('map',0,address,1,address===0x1000?7:3);engine.request(page);status('upload',0,address,4096);
    }
    const iterations=4100,budget=5*iterations+1;
    const registers=[0x12345678,iterations,0x87654321,0x3000,0x4800,0x11223344,0x1040,0x99aabbcc];
    const state=record('R3ST',56),s=view(state);registers.forEach((value,index)=>s.setUint32(16+index*4,value,true));
    s.setUint32(48,0x1000,true);s.setUint32(52,2,true);new Uint8Array(memory.buffer,engine.base,56).set(state);
    const initialFp=bytes(engine.base+4236,128),initialSimd=bytes(engine.base+4364,160),initialYmm=bytes(engine.base+4612,144);
    const prepare=api.core_prepare;let prepares=0;
    api.core_prepare=(...args)=>{prepares++;return prepare(...args);};
    cache=new CodeCache(engine,{budget,instructions:64,blocks:8,chain:true});
    const select=pc=>{
      const unit=cache.entry(pc),encoded=bytes(api.core_code_ptr()>>>0,api.core_code_len()>>>0);
      const module={pc,identity:String(unit.id),bytes:encoded.length,sha256:hash(encoded),slot:unit.slot};
      result.modules.push(module);return unit;
    };
    const first=select(0x1000),second=select(0x1040);assert.notEqual(first.id,second.id);
    status('core_lookup',0,0x1000);
    status('core_begin',0);status('core_end',0);
    const backing=word(cache.context,64),expectedRam=bytes(backing,word(cache.context,68)*4096);
    const dataSlot=word(word(cache.context,56)+(0x3000>>>12)*4),dataOffset=(dataSlot-1)*4096;
    const ram=new DataView(expectedRam.buffer);ram.setUint32(dataOffset+0x10,iterations,true);ram.setUint32(dataOffset+0x14,2*iterations,true);
    const versionPointer=word(cache.context,80),beforeVersion=new DataView(memory.buffer).getBigUint64(versionPointer,true);
    const owners=bytes(api.core_link_owners_ptr()>>>0,4097*8),prepared=prepares;
    const run=cache.execute(first);result.run={...run,id:run.id===null?null:String(run.id)};result.afterRun=cache.sampleUsage();
    if(run.coreReason===1){
      // Preserve an actual baseline replacement witness before failing the gate.
      const continuation=select(run.pc);result.formationStop={pc:run.pc,retired:run.retired,work:run.work,
        selectedIdentity:String(continuation.id),additionalPrepares:prepares-prepared,afterPrepare:cache.sampleUsage()};
    }
    assert.equal(run.coreReason,2,'a warmed existing graph completes its quota without a code-formation stop');
    assert.deepEqual([run.pc,run.retired,run.work],[0x1048,budget,budget]);
    assert.equal(prepares,prepared,'more than 4096 known edges do not request preparation');
    assert.equal(cache.units.size,2);assert.equal(cache.units.get(first.id),first);assert.equal(cache.units.get(second.id),second);
    assert.equal(cache.admissions,2);assert.equal(cache.generatedModuleBytes,result.modules[0].bytes+result.modules[1].bytes);
    assert.equal(cache.nativeCache.compiled,'2');assert.equal(cache.nativeCache.retainedUnits,'2');
    assert.deepEqual([cache.transientAdmissions,cache.transientRuns],[0,0]);
    assert.deepEqual(bytes(api.core_link_owners_ptr()>>>0,4097*8),owners,'both original reciprocal slots survive every edge');
    const wantedRegisters=registers.slice();wantedRegisters[1]=0;
    assert.deepEqual(Array.from({length:8},(_,i)=>word(cache.context,i*4)),wantedRegisters);
    assert.deepEqual([word(cache.context,32),word(cache.context,36),word(cache.context,40),word(cache.context,44)],
      [0x1048,0x46,budget,0]);
    assert.deepEqual([word(cache.context,48),word(cache.context,52),word(cache.context,116)],[0,0,0]);
    const wanted=state.slice(),w=view(wanted);w.setUint32(20,0,true);w.setUint32(48,0x1048,true);w.setUint32(52,0x46,true);
    assert.deepEqual(bytes(engine.base,56),wanted);assert.deepEqual(bytes(engine.base+4236,128),initialFp);
    assert.deepEqual(bytes(engine.base+4364,160),initialSimd);assert.deepEqual(bytes(engine.base+4612,144),initialYmm);
    assert.deepEqual(bytes(backing,expectedRam.length),expectedRam);
    assert.equal(new DataView(memory.buffer).getBigUint64(versionPointer,true),beforeVersion+2n*BigInt(iterations));
    for(const[unit,module]of[[first,result.modules[0]],[second,result.modules[1]]]){
      status('core_lookup',0,module.pc);
      const id=new DataView(memory.buffer).getBigUint64(cache.context+120,true);
      assert.equal(id,unit.id);assert.equal(hash(bytes(api.core_code_ptr()>>>0,api.core_code_len()>>>0)),module.sha256,
        'the warmed owner still exposes its original emitted body');
      assert.equal(cache.units.get(id).run,unit.run,'the actual SDK Wasm instance remains retained');
    }
    result.final={iterations,retired:budget,writes:2*iterations,version:String(beforeVersion+2n*BigInt(iterations)),
      ramSha256:hash(expectedRam),identities:[String(first.id),String(second.id)]};result.complete=true;
  }catch(error){result.failure={message:String(error),stack:error.stack};throw error;}
  finally{if(cache?.open)api.core_end();if(opened)status('close',0);}
}
