import {linkedField} from '../pool-state.mjs';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {Engine,KEY,record,view} from '../../../../../../sdk/runtime/engine.js';
import {CodeCache} from '../../../../../../sdk/runtime/cache.js';
import {resolverExport,forceNativeInspection} from '../link.mjs';

const FP=4236,SIMD=4364,YMM=4612,COLD=0x40000000;
const unhex=text=>Uint8Array.from(Buffer.from(text.replaceAll(' ',''),'hex'));
const hex=bytes=>Buffer.from(bytes).toString('hex');
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
const FIVE='00000000000000a00140',SEVEN='00000000000000e00140';

function expectedFp(initial,{control=0x27f,value}) {
  const wanted=initial.slice(),d=view(wanted);
  d.setUint16(16,control,true);d.setUint16(18,0,true);d.setUint16(20,0xffff,true);
  d.setUint16(22,0x15b,true);d.setUint32(24,0x1083,true);d.setUint32(28,0x3008,true);
  d.setUint16(32,0x1b,true);d.setUint16(34,0x23,true);wanted.set(unhex(value),40+7*10);
  return wanted;
}

function coldBank(initial) {
  const wanted=initial.slice(),d=view(wanted);
  d.setUint16(0,0x7f,true);d.setUint16(2,0x3800,true);d.setUint16(4,0x3fff,true);
  d.setUint16(6,0x43,true);d.setUint32(8,0x1103,true);d.setUint32(12,0x3004,true);
  d.setUint16(16,0x1b,true);d.setUint16(18,0x23,true);wanted.set(unhex(SEVEN),20+7*10);
  return wanted;
}

export function runLinkedCases(artifact,report) {
  for(const withCold of[false,true])runLinkedCase(artifact,report,withCold);
}

function runLinkedCase(artifact,report,withCold) {
  const name=withCold?'linked-canonical-cold-state-survives-an-integer-only-bridge'
    :'linked-dirty-fp-flags-and-memory-sequence-cross-an-integer-only-bridge';
  const result={name,complete:false,controls:[],modules:[],probes:[],coldCalls:[],numericCalls:0};
  report.cases.push(result);
  const instance=new WebAssembly.Instance(artifact.module,{ring3_observability:{now_ns:()=>0n}});
  const raw=Object.fromEntries(Object.entries(instance.exports)
    .filter(([key])=>key.startsWith('ring3_abi_v1_')).map(([key,value])=>[key.slice(13),value]));
  const resolver=typeof raw.core_chain_resolve_executable_owned==='function'
    ?'core_chain_resolve_executable_owned':resolverExport(raw);
  result.resolver=resolver;
  const memory=instance.exports.memory;
  const bytes=(pointer,length)=>new Uint8Array(memory.buffer).slice(pointer,pointer+length);
  const word=(pointer,offset=0)=>new DataView(memory.buffer).getUint32(pointer+offset,true);
  let cache,engine,opened=false,active=false,initialFp,initialBank,initialContext,initialRevision;
  const units=new Map(),budget=withCold?16:13;
  const registers=[0xffffffff,0x12345678,0x10c0,0x3000,0x4800,0,withCold?0x1100:0x1040,0x1040];
  const global=local=>linkedField(cache,memory,local);
  const revision=()=>new DataView(memory.buffer).getBigUint64(word(cache.context,80),true);
  const bank=()=>bytes(word(cache.context,92),100);
  const physical=address=>{
    const slot=word(word(cache.context,56)+(address>>>12)*4);assert(slot>0);
    return word(cache.context,64)+(slot-1)*4096+(address&4095);
  };
  const status=(operation,wanted,...args)=>{
    const actual=raw[operation](...args)>>>0;result.controls.push({operation,wanted,actual});
    assert.equal(actual,wanted,`${name}: ${operation}`);
  };
  const api=Object.freeze({...raw,
    core_numeric:(...args)=>{result.numericCalls++;return raw.core_numeric(...args);},
    ...(typeof raw.core_numeric_owned==='function'?{core_numeric_owned:(...args)=>{result.numericCalls++;return raw.core_numeric_owned(...args);}}:{}),
    [resolver]:(context,pc,selectionRetired,remaining)=>{
      const first=result.probes.length===0;
      assert.equal(context,cache.context,'the live probe uses the retained CPU context');
      assert.equal(pc,first?registers[6]:0x1080,'the three authored units execute in order');
      const wantedRegisters=registers.slice();wantedRegisters[0]=first?0:1;wantedRegisters[5]=first?0:7;
      assert.deepEqual(Array.from({length:8},(_,index)=>global(index+2)),wantedRegisters,
        'all register values cross the compiled boundary without publication');
      assert.equal(global(10),pc,'the linked PC names the next unretired unit');
      const retired=first?5:withCold?12:9;
      assert.equal(global(13),retired,'the linked instruction prefix remains exact');
      assert.equal(global(12),budget-retired,'the linked quota remains exact');
      if(resolver==='core_chain_resolve_executable_owned')
        assert.equal(remaining>>>0,budget-retired,'the actual resolver receives the live quota');
      assert.deepEqual([global(20),global(25),global(21),global(22),global(23),global(24)],
        first?[1,32,-1,1,0,0]:[6,32,0,0,1,1],
        'the ADD or ADC recipe remains deferred with its exact operands and carry');
      assert.equal(global(11),first?2:withCold?0x57:3,'only the CF consumed by ADC is materialized');
      assert.equal(global(99),initialRevision+(first?1n:2n),'stores share one exact linked sequence');
      assert.equal(revision(),initialRevision+(withCold&&!first?1n:0n),
        'ordinary linked writes do not eagerly publish the global transaction sequence');
      assert.deepEqual(bytes(engine.base+FP,128),initialFp,'the public FP image stays unpublished');
      if(withCold&&!first) {
        assert.deepEqual([global(84),global(85),global(86)],[0,0,0],
          'the integer-only bridge does not resurrect FP ownership after canonical cold execution');
        assert.deepEqual(bank(),coldBank(initialBank),'the canonical seven and changed control word remain authoritative');
        const canonical=initialContext.slice(),d=view(canonical);
        d.setUint32(0,0,true);d.setUint32(32,0x1040,true);d.setUint32(36,0x57,true);
        d.setUint32(40,8,true);d.setUint32(44,8,true);
        assert.deepEqual(bytes(context,48),canonical,'the integer bridge leaves its canonical entry snapshot untouched');
      } else {
        assert.deepEqual([global(84),global(85),global(86)],[1,1,1],'the exact-f64 slot stays active, valid and dirty');
        assert.equal(global(76),5,'the unmaterialized logical ST0 remains five');
        assert.deepEqual([global(87),global(88),global(89),global(90),global(91),global(92),global(93),global(94),global(95)],
          [0x27f,0x3800,0x3fff,7,0x1002,0x3004,0x43,0x1b,0x23],
          'the complete live x87 environment survives the integer-only unit');
        assert.deepEqual(bank(),initialBank,'raw80 payloads and metadata are not packed at an intermediate compiled boundary');
        assert.deepEqual(bytes(context,48),initialContext,'GPRs, flags and work remain private at intermediate probes');
      }
      const returned=raw[resolver](context,pc,selectionRetired,remaining)>>>0;
      assert.equal(returned,first&&withCold?COLD:units.get(pc).slot,'the actual native cache chooses the authored owner');
      result.probes.push({pc,returned,retired,remaining:global(12),sequence:String(global(99)),
        canonicalRevision:String(revision()),bankHex:hex(bank()),active:global(84),dirty:global(86)});
      return returned;
    },
    core_chain_cold:context=>{
      assert(withCold,'the fully linked control must not enter cold execution');
      assert.equal(result.coldCalls.length,0,'one authored cold boundary');
      assert.equal(revision(),initialRevision+1n,'cold execution observes the committed scalar prefix');
      assert.deepEqual([global(84),global(85),global(86)],[0,0,0],'publication relinquishes all FP slot ownership');
      const before=bank();
      assert.equal(hex(before.subarray(90,100)),FIVE,'the cold helper receives the materialized five');
      const returned=raw.core_chain_cold(context)>>>0;
      assert.equal(returned,(COLD|units.get(0x1040).slot)>>>0,'cold execution stops at the retained integer bridge');
      assert.deepEqual(bank(),coldBank(initialBank),'FLDCW and FADD commit the independent canonical seven');
      assert.equal(revision(),initialRevision+1n,'read-only x87 cold work does not invent memory transactions');
      result.coldCalls.push({returned,beforeBankHex:hex(before),afterBankHex:hex(bank())});
      return returned;
    },
  });
  try {
    engine=new Engine(api,memory,artifact.sha256);status('open',0,12,...KEY);opened=true;
    engine.base=raw.arena_ptr()>>>0;
    const code=new Uint8Array(4096),data=new Uint8Array(4096);
    const segments=[[0x1000,'d903 d84304 83c001 894b10 ffe6'],
      [0x1040,'8d6d07 83d000 894314 ff631c'],[0x1080,'d84b04 d95b08 83d000 ffe2'],
      [0x10c0,'0f0b'],[0x1100,'d96b18 d84304 ffe7']];
    for(const[pc,value]of segments)code.set(unhex(value),pc-0x1000);
    view(data).setFloat32(0,3,true);view(data).setFloat32(4,2,true);
    view(data).setUint16(24,0x7f,true);view(data).setUint32(28,0x1080,true);
    for(const[address,page]of[[0x1000,code],[0x3000,data],[0x4000,new Uint8Array(4096)]]) {
      status('map',0,address,1,address===0x1000?7:3);engine.request(page);status('upload',0,address,4096);
    }
    const state=record('R3ST',56),s=view(state);
    registers.forEach((value,index)=>s.setUint32(16+4*index,value,true));
    s.setUint32(48,0x1000,true);s.setUint32(52,2,true);
    new Uint8Array(memory.buffer,engine.base,56).set(state);
    new DataView(memory.buffer).setUint16(engine.base+FP+16,0x27f,true);
    initialFp=bytes(engine.base+FP,128);
    const initialSimd=bytes(engine.base+SIMD,160),initialYmm=bytes(engine.base+YMM,144);
    cache=new CodeCache(engine,{budget,instructions:64,blocks:8,chain:true});
    forceNativeInspection(cache,result);
    assert.equal(cache.linkedCore,true,'the fixture requires the actual linked CPU ABI');
    for(const pc of[0x1000,0x1040,0x1080]) {
      const unit=cache.entry(pc);units.set(pc,unit);
      const encoded=bytes(raw.core_code_ptr()>>>0,raw.core_code_len()>>>0),module=new WebAssembly.Module(encoded);
      const imports=WebAssembly.Module.imports(module);
      if(pc===0x1040)assert(!imports.some(item=>item.name==='numeric'),'the middle unit contains no numerical helper');
      result.modules.push({pc,id:String(unit.id),slot:unit.slot,sha256:hash(encoded),imports});
    }
    const first=cache.entry(0x1000);assert.equal(first.id,units.get(0x1000).id);
    status('core_begin',0);active=true;status('core_end',0);active=false;
    initialBank=bank();initialContext=bytes(cache.context,48);initialRevision=revision();
    const backing=word(cache.context,64),expectedRam=bytes(backing,word(cache.context,68)*4096);
    const expectWrite=(address,value)=>expectedRam.set(unhex(value),physical(address)-backing);
    expectWrite(0x3010,'78563412');expectWrite(0x3014,'01000000');
    expectWrite(0x3008,withCold?'00006041':'00002041');
    const executed=cache.execute(first);
    assert.equal(executed.coreReason,2,'the exact terminal budget stops before the unexecuted sentinel');
    assert.equal(executed.pc,0x10c0);assert.equal(executed.retired,budget);assert.equal(executed.work,budget);
    assert.equal(executed.regions,3);assert.equal(executed.id,units.get(0x1080).id);
    assert.equal(executed.chainedColdRuns,Number(withCold));
    assert.equal(executed.chainedColdRetired,withCold?3:0);assert.equal(executed.chainedColdWork,withCold?3:0);
    assert.equal(result.probes.length,2);assert.equal(result.coldCalls.length,Number(withCold));
    assert.equal(result.numericCalls,0,'compiled numerical instructions use the region value state');
    assert.equal(revision(),initialRevision+3n,'all three committed stores retain their separate transaction identities');
    const wantedRegisters=registers.slice();wantedRegisters[0]=1;wantedRegisters[5]=7;
    assert.deepEqual(Array.from({length:8},(_,index)=>word(cache.context,index*4)),wantedRegisters,'all terminal GPRs');
    assert.deepEqual([word(cache.context,32),word(cache.context,36),word(cache.context,40),word(cache.context,44)],
      [0x10c0,2,budget,0],'terminal PC, complete FLAGS, retirement and quota');
    assert.deepEqual([word(cache.context,48),word(cache.context,52),word(cache.context,116)],[0,0,0],'no hidden fault');
    assert.deepEqual(bytes(engine.base+FP,128),expectedFp(initialFp,{control:withCold?0x7f:0x27f,
      value:withCold?'00000000000000e00240':'00000000000000a00240'}),'the full literal terminal FP bank');
    assert.deepEqual(bytes(engine.base+SIMD,160),initialSimd,'the full SIMD bank remains unchanged');
    assert.deepEqual(bytes(engine.base+YMM,144),initialYmm,'the full upper vector bank remains unchanged');
    assert.deepEqual(bytes(backing,expectedRam.length),expectedRam,'the complete physical backing matches the three authored stores');
    const wantedState=state.slice(),w=view(wantedState);
    wantedRegisters.forEach((value,index)=>w.setUint32(16+4*index,value,true));w.setUint32(48,0x10c0,true);
    assert.deepEqual(bytes(engine.base,56),wantedState,'the full public CPU state is published once at the terminal boundary');
    const stats=cache.stats();assert.equal(stats.runs,1);assert.equal(stats.admissions,3);
    result.codeSegments=segments;result.stats=stats;
    result.final={pc:executed.pc,retired:executed.retired,flags:word(cache.context,36),
      revision:String(revision()),fpHex:hex(bytes(engine.base+FP,128)),ramSha256:hash(expectedRam)};
    result.complete=true;
  } catch(error) {
    result.failure={message:String(error),stack:error.stack};throw error;
  } finally {
    if(active||cache?.open)raw.core_end();
    if(opened)status('close',0);
  }
}
