import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {Engine,KEY,record,view} from '../../../../../../sdk/runtime/engine.js';
import {CodeCache} from '../../../../../../sdk/runtime/cache.js';
import {sourceLookup} from '../link.mjs';

const hash=bytes=>createHash('sha256').update(bytes).digest('hex');

export function runOwnershipCase(artifact,report) {
  runInteriorCase(artifact,report,8);
  runInteriorCase(artifact,report,1);
  runTransientShiftCase(artifact,report);
}

function runInteriorCase(artifact,report,blocks) {
  const result={name:blocks===1?'owned-interior-entry-labels-do-not-consume-the-one-block-discovery-budget':
    'owned-interior-ports-execute-one-immutable-source-owner-and-preserve-its-root',
    complete:false,controls:[],modules:[]};report.cases.push(result);
  const instance=new WebAssembly.Instance(artifact.module,{ring3_observability:{now_ns:()=>0n}});
  const api=Object.fromEntries(Object.entries(instance.exports)
    .filter(([key])=>key.startsWith('ring3_abi_v1_')).map(([key,value])=>[key.slice(13),value]));
  const memory=instance.exports.memory,bytes=(pointer,length)=>new Uint8Array(memory.buffer).slice(pointer,pointer+length);
  const word=(pointer,offset=0)=>new DataView(memory.buffer).getUint32(pointer+offset,true);
  let cache,engine,opened=false,active=false;
  const status=(operation,wanted,...args)=>{
    const actual=api[operation](...args)>>>0;result.controls.push({operation,wanted,actual});assert.equal(actual,wanted);
  };
  try{
    engine=new Engine(api,memory,artifact.sha256);status('open',0,12,...KEY);opened=true;engine.base=api.arena_ptr()>>>0;
    const code=new Uint8Array(4096),stack=new Uint8Array(4096);
    code.set([0x90,0x90,0x90,0x90,0x90,0x90,0x90,0x90,0xc3]);code.set([0x0f,0x0b],0x100);
    view(stack).setUint32(0,0x1100,true);
    for(const[address,page]of[[0x1000,code],[0x3000,new Uint8Array(4096)],[0x4000,stack]]){
      status('map',0,address,1,address===0x1000?7:3);engine.request(page);status('upload',0,address,4096);
    }
    const registers=[0x12345678,2,0x87654321,0x3000,0x4000,0x11223344,0x55667788,0x99aabbcc];
    const state=record('R3ST',56),s=view(state);registers.forEach((value,index)=>s.setUint32(16+index*4,value,true));
    s.setUint32(48,0x1000,true);s.setUint32(52,2,true);new Uint8Array(memory.buffer,engine.base,56).set(state);
    const initialFp=bytes(engine.base+4236,128),initialSimd=bytes(engine.base+4364,160),initialYmm=bytes(engine.base+4612,144);
    result.selections=[];
    for(const name of['core_prepare','core_lookup','core_cold_run'])if(typeof api[name]==='function'){
      const native=api[name];api[name]=(...args)=>{const returned=native(...args);result.selections.push({name,args,returned});return returned;};
    }
    cache=new CodeCache(engine,{budget:5,instructions:64,blocks,chain:true});
    const select=pc=>{
      const unit=cache.entry(pc),encoded=bytes(api.core_code_ptr()>>>0,api.core_code_len()>>>0);
      result.modules.push({pc,identity:String(unit.id),bytes:encoded.length,sha256:hash(encoded)});return unit;
    };
    const original=select(0x1000),interior=select(0x1004),secondInterior=select(0x1006);
    cache.sampleUsage();result.afterInterior=cache.stats();
    assert.equal(interior.id,original.id,'a precompiled interior port preserves its actual owner identity');
    assert.equal(secondInterior.id,original.id,'a second interior port preserves that same immutable owner');
    assert(result.modules.every(module=>module.sha256===result.modules[0].sha256),
      'interior admission executes the original emitted module bytes without re-emission');
    assert.equal(interior.run,original.run);assert.equal(secondInterior.run,original.run);
    assert.equal(cache.admissions,1,'binding two interior ports does not admit replacement code');
    assert.equal(cache.generatedModuleBytes,result.modules[0].bytes);
    if(blocks===1){
      assert.equal(interior.transient,undefined,'splitting already owned instructions into an entry label consumes no new-block discovery budget');
      assert.deepEqual([cache.transientAdmissions,cache.transientRuns,cache.transientModuleBytes],[0,0,0],
        'an exact owned-entry split never creates a transient module');
    }
    assert.equal(cache.units.size,1,'the interior entry replaces its source owner instead of retaining a duplicate suffix');
    assert.equal(cache.nativeCache.retainedUnits,'1','one native owner covers the original nine instruction starts');
    const root=select(0x1000);assert.equal(root.id,interior.id,'the unchanged module still owns its original root');
    assert.equal(cache.units.size,1,'selecting the original root reuses the same instance');
    status('core_begin',0);active=true;status('core_end',0);active=false;
    const backing=word(cache.context,64),expectedRam=bytes(backing,word(cache.context,68)*4096);
    const versionPointer=word(cache.context,80),beforeVersion=new DataView(memory.buffer).getBigUint64(versionPointer,true);
    new DataView(memory.buffer).setUint32(cache.context+32,0x1004,true);
    const executed=cache.execute(interior),wantedRegisters=registers.slice();wantedRegisters[4]+=4;
    assert.deepEqual([executed.coreReason,executed.pc,executed.retired,executed.work],[2,0x1100,5,5],
      'only the interior four NOPs and RET retire');
    assert.deepEqual(Array.from({length:8},(_,i)=>word(cache.context,i*4)),wantedRegisters,'all architectural GPRs');
    assert.deepEqual([word(cache.context,32),word(cache.context,36),word(cache.context,40),word(cache.context,44)],
      [0x1100,2,5,0],'exact PC, FLAGS, retirement and quota');
    assert.deepEqual([word(cache.context,48),word(cache.context,52),word(cache.context,116)],[0,0,0],'no hidden fault');
    const wantedState=state.slice(),w=view(wantedState);w.setUint32(16+4*4,wantedRegisters[4],true);w.setUint32(48,0x1100,true);
    assert.deepEqual(bytes(engine.base,56),wantedState,'the full public scalar image matches the entered suffix');
    assert.deepEqual(bytes(engine.base+4236,128),initialFp);assert.deepEqual(bytes(engine.base+4364,160),initialSimd);
    assert.deepEqual(bytes(engine.base+4612,144),initialYmm);assert.deepEqual(bytes(backing,expectedRam.length),expectedRam);
    assert.equal(new DataView(memory.buffer).getBigUint64(versionPointer,true),beforeVersion,'no guest memory write occurs');
    new DataView(memory.buffer).setUint32(cache.context+32,0x1006,true);
    new DataView(memory.buffer).setUint32(cache.context+16,0x4000,true);cache.budget=3;
    const second=cache.execute(secondInterior);
    assert.deepEqual([second.coreReason,second.pc,second.retired,second.work],[2,0x1100,3,3],
      'the second immutable port executes only its two NOPs and RET');
    assert.deepEqual(Array.from({length:8},(_,i)=>word(cache.context,i*4)),wantedRegisters);
    assert.deepEqual([word(cache.context,32),word(cache.context,36),word(cache.context,40),word(cache.context,44)],
      [0x1100,2,8,0],'successive port activations account only their executed suffixes');
    assert.deepEqual(bytes(engine.base,56),wantedState);assert.deepEqual(bytes(engine.base+4236,128),initialFp);
    assert.deepEqual(bytes(engine.base+4364,160),initialSimd);assert.deepEqual(bytes(engine.base+4612,144),initialYmm);
    assert.deepEqual(bytes(backing,expectedRam.length),expectedRam);
    assert.equal(new DataView(memory.buffer).getBigUint64(versionPointer,true),beforeVersion);
    assert.equal(cache.units.size,1);assert.equal(cache.admissions,1);assert.equal(cache.generatedModuleBytes,result.modules[0].bytes);
    assert.deepEqual([cache.transientAdmissions,cache.transientRuns,cache.transientModuleBytes],[0,0,0]);
    select(0x1005);
    new DataView(memory.buffer).setUint32(cache.context+32,0x1004,true);
    new DataView(memory.buffer).setUint32(cache.context+16,0x4000,true);cache.budget=1;
    const third=cache.execute(interior);assert.deepEqual([third.coreReason,third.pc,third.retired,third.work],[2,0x1005,1,1]);
    const prepareCount=result.selections.filter(row=>row.name==='core_prepare').length;
    const lookupCount=result.selections.filter(row=>row.name==='core_lookup').length;
    const continuation=sourceLookup(cache);assert.equal(continuation,original,'explicit source lookup selects the retained private interior port');
    assert.equal(result.selections.filter(row=>row.name==='core_prepare').length,prepareCount,'continuation does not prepare code');
    assert.equal(result.selections.filter(row=>row.name==='core_lookup').length,lookupCount+1,'actual native lookup validates continuation');
    const fourth=cache.execute(continuation);
    assert.deepEqual([fourth.coreReason,fourth.pc,fourth.retired,fourth.work],[2,0x1006,1,1]);
    assert.equal(result.selections.filter(row=>row.name==='core_cold_run').length,0,'retained continuation opens no empty cold lease');
    const finalRegisters=registers.slice();
    assert.deepEqual(Array.from({length:8},(_,i)=>word(cache.context,i*4)),finalRegisters);
    assert.deepEqual([word(cache.context,32),word(cache.context,36),word(cache.context,40),word(cache.context,44)],[0x1006,2,10,0]);
    const finalState=state.slice();view(finalState).setUint32(48,0x1006,true);
    assert.deepEqual(bytes(engine.base,56),finalState);assert.deepEqual(bytes(engine.base+4236,128),initialFp);
    assert.deepEqual(bytes(engine.base+4364,160),initialSimd);assert.deepEqual(bytes(engine.base+4612,144),initialYmm);
    assert.deepEqual(bytes(backing,expectedRam.length),expectedRam);
    assert.equal(new DataView(memory.buffer).getBigUint64(versionPointer,true),beforeVersion);
    assert.equal(cache.units.size,1);assert.equal(cache.admissions,1);assert.equal(cache.generatedModuleBytes,result.modules[0].bytes);
    result.final={pc:fourth.pc,retired:executed.retired,secondRetired:second.retired,continuationRetired:2,
      owner:String(interior.id),ramSha256:hash(expectedRam)};result.complete=true;
  }catch(error){result.failure={message:String(error),stack:error.stack};throw error;}
  finally{if(active||cache?.open)api.core_end();if(opened)status('close',0);}
}

function runTransientShiftCase(artifact,report) {
  const result={name:'owned-bound-decline-runs-exact-compile-only-shift-without-retaining-a-suffix',complete:false,controls:[]};
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
    const code=new Uint8Array(4096);code.set([0x90,0x90,0x90,0x90,0xd1,0xe0,0x90,0x90,0xc3]);
    for(const[address,page]of[[0x1000,code],[0x3000,new Uint8Array(4096)],[0x4000,new Uint8Array(4096)]]){
      status('map',0,address,1,address===0x1000?7:3);engine.request(page);status('upload',0,address,4096);
    }
    const registers=[0x80000001,2,0x87654321,0x3000,0x4000,0x11223344,0x55667788,0x99aabbcc];
    const state=record('R3ST',56),s=view(state);registers.forEach((value,index)=>s.setUint32(16+index*4,value,true));
    s.setUint32(48,0x1000,true);s.setUint32(52,2,true);new Uint8Array(memory.buffer,engine.base,56).set(state);
    const initialFp=bytes(engine.base+4236,128),initialSimd=bytes(engine.base+4364,160),initialYmm=bytes(engine.base+4612,144);
    const prepare=api.core_prepare,begin=api.core_begin;
    api.core_prepare=(...args)=>{const returned=prepare(...args)>>>0;result.controls.push({name:'prepare',args,returned});return returned;};
    api.core_begin=()=>{const context=api.core_context_ptr()>>>0;result.controls.push({name:'begin',prepared:word(context,112),identity:String(new DataView(memory.buffer).getBigUint64(context+120,true))});return begin();};
    cache=new CodeCache(engine,{budget:1,instructions:64,blocks:8,chain:true});
    const owner=cache.entry(0x1000),slot=owner.slot,retainedBytes=cache.retainedModuleBytes;
    status('core_begin',0);status('core_end',0);
    new DataView(memory.buffer).setUint32(cache.context+32,0x1004,true);
    const before=bytes(cache.context,112),publicBefore=bytes(engine.base,56);
    const backing=word(cache.context,64),expectedRam=bytes(backing,word(cache.context,68)*4096);
    const versionPointer=word(cache.context,80),beforeVersion=new DataView(memory.buffer).getBigUint64(versionPointer,true);
    const ownerRows=()=>bytes(api.core_link_owners_ptr()>>>0,4097*8),ownersBefore=ownerRows();
    cache.instructions=1;
    const unit=cache.entry(0x1004);assert.equal(unit.transient,true);assert.notEqual(unit.id,owner.id);
    assert.equal(word(cache.context,112),2,'only an explicit prepared transient authorizes status18');
    assert.equal(result.controls.filter(c=>c.name==='prepare').at(-1).returned,18);
    assert.deepEqual(bytes(cache.context,112),before,'declined owner upgrade preserves all canonical context fields');
    assert.deepEqual(bytes(engine.base,56),publicBefore);assert.deepEqual(ownerRows(),ownersBefore);
    assert.equal(cache.units.size,1);assert.equal(cache.units.get(owner.id),owner);assert.equal(cache.retainedModuleBytes,retainedBytes);
    assert.equal(unit.slot,undefined,'the exact one-instruction fallback never occupies a retained slot');
    const executed=cache.execute(unit),wantedRegisters=registers.slice();wantedRegisters[0]=2;
    assert.deepEqual([executed.coreReason,executed.pc,executed.retired,executed.work,executed.id],[2,0x1006,1,1,unit.id]);
    assert.deepEqual(Array.from({length:8},(_,i)=>word(cache.context,i*4)),wantedRegisters);
    assert.deepEqual([word(cache.context,32),word(cache.context,36),word(cache.context,40),word(cache.context,44),word(cache.context,112)],
      [0x1006,0x803,1,0,0],'SHL sets exact CF/OF, consumes one instruction and releases the transient lease');
    assert.deepEqual([word(cache.context,48),word(cache.context,52),word(cache.context,116)],[0,0,0]);
    const wantedState=state.slice(),w=view(wantedState);w.setUint32(16,2,true);w.setUint32(48,0x1006,true);w.setUint32(52,0x803,true);
    assert.deepEqual(bytes(engine.base,56),wantedState);assert.deepEqual(bytes(engine.base+4236,128),initialFp);
    assert.deepEqual(bytes(engine.base+4364,160),initialSimd);assert.deepEqual(bytes(engine.base+4612,144),initialYmm);
    assert.deepEqual(bytes(backing,expectedRam.length),expectedRam);assert.equal(new DataView(memory.buffer).getBigUint64(versionPointer,true),beforeVersion);
    assert.deepEqual(ownerRows(),ownersBefore);assert.equal(cache.units.size,1);assert.equal(cache.transient,null);
    cache.sampleUsage();assert.equal(cache.nativeCache.retainedUnits,'1');
    assert.equal(cache.entry(0x1000),owner,'the unchanged original root and link remain executable');assert.equal(owner.slot,slot);
    assert.equal(cache.transientAdmissions,1);assert.equal(cache.transientRuns,1);
    result.final={pc:executed.pc,flags:0x803,retired:1,owner:String(owner.id),transient:String(unit.id),ramSha256:hash(expectedRam)};
    result.complete=true;
  }catch(error){result.failure={message:String(error),stack:error.stack};throw error;}
  finally{if(cache?.open)api.core_end();if(opened)status('close',0);}
}
