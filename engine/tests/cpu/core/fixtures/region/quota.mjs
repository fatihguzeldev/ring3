import {linkedField} from '../pool-state.mjs';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {Engine,KEY,record,view} from '../../../../../../sdk/runtime/engine.js';
import {CodeCache} from '../../../../../../sdk/runtime/cache.js';
import {resolverExport,forceNativeInspection} from '../link.mjs';

const unhex=text=>Uint8Array.from(Buffer.from(text.replaceAll(' ',''),'hex'));
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');

export function runLinkedQuotaCase(artifact,report) {
  const result={name:'linked-short-quota-keeps-imported-fp-before-entry-contract-validation',
    complete:false,controls:[],modules:[],probes:[],numericCalls:0};
  report.cases.push(result);
  const instance=new WebAssembly.Instance(artifact.module,{ring3_observability:{now_ns:()=>0n}});
  const raw=Object.fromEntries(Object.entries(instance.exports)
    .filter(([key])=>key.startsWith('ring3_abi_v1_')).map(([key,value])=>[key.slice(13),value]));
  const memory=instance.exports.memory,bytes=(pointer,length)=>new Uint8Array(memory.buffer).slice(pointer,pointer+length);
  const word=(pointer,offset=0)=>new DataView(memory.buffer).getUint32(pointer+offset,true);
  const registers=[0x12345678,2,0,0x3000,0x4800,0,0x1040,0];
  let cache,engine,opened=false,active=false,initialFp,initialBank;
  const units=new Map(),global=local=>linkedField(cache,memory,local);
  const version=()=>new DataView(memory.buffer).getBigUint64(word(cache.context,80),true);
  const status=(operation,wanted,...args)=>{
    const actual=raw[operation](...args)>>>0;result.controls.push({operation,wanted,actual});
    assert.equal(actual,wanted,`${result.name}: ${operation}`);
  };
  const api=Object.freeze({...raw,
    core_numeric:(...args)=>{result.numericCalls++;return raw.core_numeric(...args);},
    ...(typeof raw.core_numeric_owned==='function'?{core_numeric_owned:(...args)=>{result.numericCalls++;return raw.core_numeric_owned(...args);}}:{}),
    [resolverExport(raw)]:(context,pc,selectionRetired)=>{
      assert.equal(context,cache.context);assert.equal(pc,0x1040);assert.equal(result.probes.length,0);
      assert.deepEqual([global(84),global(85),global(86),global(89),global(90)],
        [1,255,255,0,0],'all eight occupied dirty slots reach the target as actual linked state');
      assert.deepEqual(Array.from({length:8},(_,i)=>global(76+i)),Array(8).fill(3),'all live native values are three');
      assert.deepEqual([global(13),global(12)],[9,1],'only one instruction remains for the target');
      assert.deepEqual(bytes(word(context,92),100),initialBank,'the producer has not packed its private raw80 bank');
      assert.deepEqual(bytes(engine.base+4236,128),initialFp,'the public FP image remains unpublished at the link');
      const returned=raw[resolverExport(raw)](context,pc,selectionRetired)>>>0;
      assert.equal(returned,units.get(pc).slot,'the real native probe selects the retained loop unit');
      result.probes.push({pc,returned,retired:global(13),remaining:global(12),tag:global(89)});
      return returned;
    },
  });
  try {
    engine=new Engine(api,memory,artifact.sha256);status('open',0,12,...KEY);opened=true;engine.base=raw.arena_ptr()>>>0;
    const code=new Uint8Array(4096),data=new Uint8Array(4096);
    // The loop's balanced pushes/pops predict logical slots6/7 empty on its backedge.
    // The external producer contradicts that contract. With quota1 only NOP retires;
    // no x87 instruction is allowed to validate, change or fault on the imported stack.
    const segments=[[0x1000,'d903 d903 d903 d903 d903 d903 d903 d903 ffe6'],
      [0x1040,'90 d903 d94304 d9c9 d8c1 ddd8 ddd8 49 75ef 0f0b']];
    for(const[pc,encoded]of segments)code.set(unhex(encoded),pc-0x1000);
    view(data).setFloat32(0,3,true);view(data).setFloat32(4,2,true);
    for(const[address,page]of[[0x1000,code],[0x3000,data],[0x4000,new Uint8Array(4096)]]){
      status('map',0,address,1,address===0x1000?7:3);engine.request(page);status('upload',0,address,4096);
    }
    const state=record('R3ST',56),s=view(state);
    registers.forEach((value,index)=>s.setUint32(16+4*index,value,true));s.setUint32(48,0x1000,true);s.setUint32(52,2,true);
    new Uint8Array(memory.buffer,engine.base,56).set(state);
    new DataView(memory.buffer).setUint16(engine.base+4236+16,0x27f,true);
    initialFp=bytes(engine.base+4236,128);
    const initialSimd=bytes(engine.base+4364,160),initialYmm=bytes(engine.base+4612,144);
    cache=new CodeCache(engine,{budget:10,instructions:64,blocks:8,chain:true});
    forceNativeInspection(cache,result);
    assert.equal(cache.linkedCore,true,'the control exercises the actual compiled-link ABI');
    for(const pc of[0x1000,0x1040]){
      units.set(pc,cache.entry(pc));const encoded=bytes(raw.core_code_ptr()>>>0,raw.core_code_len()>>>0);
      result.modules.push({pc,sha256:hash(encoded),imports:WebAssembly.Module.imports(new WebAssembly.Module(encoded))});
    }
    const first=cache.entry(0x1000);
    status('core_begin',0);active=true;status('core_end',0);active=false;
    initialBank=bytes(word(cache.context,92),100);
    const before=version(),backing=word(cache.context,64),expectedRam=bytes(backing,word(cache.context,68)*4096);
    const executed=cache.execute(first);
    assert.deepEqual([executed.coreReason,executed.pc,executed.retired,executed.work,executed.regions],
      [2,0x1041,10,10,2],'precise quota retires only the target NOP');
    assert.equal(executed.chainedColdRuns,0);assert.equal(result.numericCalls,0);assert.equal(result.probes.length,1);
    assert.deepEqual(Array.from({length:8},(_,index)=>word(cache.context,index*4)),registers,'all terminal GPRs');
    assert.deepEqual([word(cache.context,32),word(cache.context,36),word(cache.context,40),word(cache.context,44)],
      [0x1041,2,10,0],'PC, all FLAGS, retirement and quota');
    assert.deepEqual([word(cache.context,48),word(cache.context,52),word(cache.context,116)],[0,0,0],'no unretired FP fault');
    const wantedFp=initialFp.slice(),f=view(wantedFp);
    for(const[offset,value]of[[18,0],[20,0],[22,0x103],[32,0x1b],[34,0x23]])f.setUint16(offset,value,true);
    f.setUint32(24,0x100e,true);f.setUint32(28,0x3000,true);
    for(let slot=0;slot<8;slot++)wantedFp.set(unhex('00000000000000c00040'),40+slot*10);
    assert.deepEqual(bytes(engine.base+4236,128),wantedFp,'all eight physical raw80 payloads, tags and environment survive the quota adapter');
    assert.deepEqual(bytes(engine.base+4364,160),initialSimd);assert.deepEqual(bytes(engine.base+4612,144),initialYmm);
    assert.deepEqual(bytes(backing,expectedRam.length),expectedRam,'the full guest physical backing is unchanged');
    assert.equal(version(),before,'read-only quota handling invents no memory transaction');
    const wantedState=state.slice();view(wantedState).setUint32(48,0x1041,true);
    assert.deepEqual(bytes(engine.base,56),wantedState,'the complete public scalar state matches the completed prefix');
    result.codeSegments=segments;result.final={pc:executed.pc,retired:executed.retired,fpHex:Buffer.from(wantedFp).toString('hex'),
      ramSha256:hash(expectedRam),version:String(version())};result.complete=true;
  }catch(error){result.failure={message:String(error),stack:error.stack};throw error;}
  finally{if(active||cache?.open)raw.core_end();if(opened)status('close',0);}
}
