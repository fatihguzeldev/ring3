import assert from 'node:assert/strict';
import {existsSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import {createHash} from 'node:crypto';
import {readEngine} from '../../../support/wasm/engine.mjs';
import {Engine,KEY,record,view} from '../../../../../sdk/runtime/engine.js';
import {instantiateCore,instantiateSupport} from './link.mjs';
import {assertCoreNames} from './names.mjs';

const FP=4236,SIMD=4364,YMM=4612,PAGE=4096,NATIVE_PC=0x102b,SEQUENCE=37n;
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
let artifact;
function actualEngine() {
  if(artifact)return artifact;
  const path=process.env.RING3_ENGINE_WASM??fileURLToPath(new URL('../../../../../target/wasm32-unknown-unknown/release/ring3_engine.wasm',import.meta.url));
  assert(existsSync(path),'generated transfer controls require the current actual engine');
  artifact=readEngine(path);return artifact;
}

// This is a transfer-protocol control, not a numerical implementation oracle.
// Its selected code, Context, source authority and FP bank are genuinely native;
// only the explicitly named canonical callback result is supplied by the fixture.
export function runActualTransfer(program,name,{initial,dirty,inputs,inputFlags,publishesPc=false,mode='success',budget,onRow}) {
  const artifact=actualEngine(),instance=instantiateCore(artifact.module,{ring3_observability:{now_ns:()=>0n}});
  const api=Object.fromEntries(Object.entries(instance.exports).filter(([key])=>key.startsWith('ring3_abi_v1_'))
    .map(([key,value])=>[key.slice(13),value]));
  const memory=instance.exports.memory,engine=new Engine(api,memory,artifact.sha256);
  const bytes=(at,length)=>new Uint8Array(memory.buffer).slice(at,at+length);
  const word=at=>new DataView(memory.buffer).getUint32(at,true);
  const put=(at,value)=>new DataView(memory.buffer).setUint32(at,value>>>0,true);
  const big=at=>new DataView(memory.buffer).getBigUint64(at,true);
  const setBig=(at,value)=>new DataView(memory.buffer).setBigUint64(at,value,true);
  let opened=false,active=false;
  try {
    engine.ok('open',12,...KEY);opened=true;engine.base=api.arena_ptr()>>>0;
    const code=new Uint8Array(PAGE);code.set(program.code);
    for(const[address,page]of[[0x1000,code],[0x3000,new Uint8Array(PAGE)],[0x4000,new Uint8Array(PAGE)]]){
      engine.ok('map',address,1,address===0x1000?7:3);engine.request(page);engine.ok('upload',address,PAGE);
    }
    const state=record('R3ST',56),s=view(state);
    initial.forEach((value,index)=>s.setUint32(16+index*4,value,true));
    s.setUint32(48,0x1000,true);s.setUint32(52,0x402,true);
    new Uint8Array(memory.buffer,engine.base,56).set(state);
    assert.equal(api.core_cold_run(0),2,'native initialization executes no guest instruction');
    const context=api.core_context_ptr()>>>0;
    assert(context>0&&context%8===0,'the transfer uses the actual native Context');
    const backing=word(context+64),capacity=word(context+68),versions=word(context+84),sequence=word(context+80);
    setBig(sequence,SEQUENCE);
    for(let slot=0;slot<capacity;slot++)setBig(versions+slot*16+8,0n);
    engine.ok('core_prepare',0x1000,program.instructions,32);
    const identity=big(context+120),source=bytes(api.core_code_ptr()>>>0,api.core_code_len()>>>0);
    const module=new WebAssembly.Module(source);assertCoreNames(module,0x1000,identity);
    const support=instantiateSupport(memory),seen=[];
    let writeStatus,preciseCalls=0;
    const numeric=(pointer,pc,ordinal)=>{
      assert.equal(pointer,context);assert.equal(pc,NATIVE_PC,'the callback carries the actual canonical instruction PC');
      assert.equal(ordinal,0,'this independently compiled owner has one canonical ordinal');
      seen.push(bytes(context,128));
      if(mode==='fault') {put(context+48,0x3018);put(context+52,0x10104);put(context+116,0x40e);return 3;}
      if(mode==='smc') {
        writeStatus=api.core_write(context,0x1100,4,0x12345678)>>>0;
        assert.equal(writeStatus,2,'the real checked code write revokes executable source');return 6;
      }
      if(name==='compare-flags')put(context+36,0x402);
      if(name==='processor')put(context,0x12345678);
      return 0;
    };
    const generated=new WebAssembly.Instance(module,{ring3:{...support,'support.abi':support.supportAbi,memory,
      read:()=>assert.fail('scalar slow read is outside this transfer control'),
      write:()=>assert.fail('scalar slow write is outside this transfer control'),numeric,
      precise:pointer=>{preciseCalls++;return api.core_precise(pointer);}}});
    engine.ok('core_begin');active=true;
    put(context+40,37);put(context+44,0xcafebabe);put(context+48,0xaaaaaaaa);put(context+52,0xbbbbbbbb);
    const before=bytes(context,128),wanted=before.slice(),d=view(wanted);
    const bank=word(context+92),bankBefore=bytes(bank,512),fpBefore=bytes(engine.base+FP,128);
    const simdBefore=bytes(engine.base+SIMD,160),ymmBefore=bytes(engine.base+YMM,144);
    const wantedRam=bytes(backing,capacity*PAGE),wantedVersions=bytes(versions,capacity*16);
    const epochPointer=word(context+88),epoch=big(epochPointer);
    const general=[...dirty],stopped=mode!=='success',retired=mode==='fault'?9:mode==='smc'?10:11;
    if(!stopped){if(name==='processor')general[0]=0x12345678;general[7]=name==='compare-flags'?0x77777777:0x77777778;}
    general.forEach((value,index)=>d.setUint32(index*4,value,true));
    const next=0x1000+program.code.length-5;
    d.setUint32(32,mode==='fault'?NATIVE_PC:mode==='smc'?next:0x1000+program.code.length-2,true);
    d.setUint32(36,stopped?0x457:0x406,true);d.setUint32(40,37+retired,true);d.setUint32(44,budget-retired,true);
    if(mode==='fault'){d.setUint32(48,0x3018,true);d.setUint32(52,0x10104,true);d.setUint32(116,0x40e,true);}
    if(mode==='smc'){
      const slot=word(word(context+56)+(0x1000>>>12)*4)-1;
      view(wantedRam).setUint32(slot*PAGE+0x100,0x12345678,true);
      view(wantedVersions).setBigUint64(slot*16+8,SEQUENCE+1n,true);
    }
    const reason=generated.exports.run(context,budget)>>>0;
    const row={name,mode,reason,scope:'real native lease with an explicitly mocked canonical transfer result',
      producerSha256:program.sourceSha256,engineSha256:artifact.sha256,sourceSha256:hash(source),identity:String(identity),
      contextHex:Buffer.from(bytes(context,128)).toString('hex'),helperContexts:seen.map(value=>Buffer.from(value).toString('hex')),
      memorySha256:hash(bytes(backing,capacity*PAGE)),preciseCalls,writeStatus};
    onRow?.(row);
    assert.equal(reason,mode==='fault'?3:mode==='smc'?6:4,`${name}/${mode}: stop`);
    assert.deepEqual(bytes(context,128),wanted,`${name}/${mode}: complete actual native Context`);
    assert.deepEqual(bytes(backing,capacity*PAGE),wantedRam,`${name}/${mode}: complete actual code/data/stack backing`);
    assert.deepEqual(bytes(versions,capacity*16),wantedVersions,`${name}/${mode}: all mapping/content stamps`);
    assert.equal(big(sequence),SEQUENCE+BigInt(mode==='smc'),`${name}/${mode}: exact real write sequence`);
    assert.equal(big(epochPointer),mode==='smc'?SEQUENCE+1n:epoch,`${name}/${mode}: actual source invalidation stamp`);
    assert.deepEqual(bytes(bank,512),bankBefore,`${name}/${mode}: the mock changes no canonical FP/vector value`);
    assert.deepEqual(bytes(engine.base+FP,128),fpBefore);assert.deepEqual(bytes(engine.base+SIMD,160),simdBefore);
    assert.deepEqual(bytes(engine.base+YMM,144),ymmBefore);
    assert.equal(seen.length,1,'one real emitted canonical callback');
    const expectedInput=before.slice(),input=view(expectedInput);
    for(const register of inputs)input.setUint32(register*4,dirty[register],true);
    if(inputFlags!==undefined)input.setUint32(36,inputFlags,true);
    if(publishesPc)input.setUint32(32,NATIVE_PC,true);
    assert.deepEqual(seen[0],expectedInput,`${name}/${mode}: publish only the declared integer inputs`);
    assert.equal(preciseCalls,0,'an admitted planned canonical transfer does not borrow the precise executor');
    engine.ok('core_end');active=false;
    d.setUint32(112,0,true);assert.deepEqual(bytes(context,128),wanted,'lease end changes only active status');
    row.semanticOraclePassed=true;return row;
  } finally {
    if(active)engine.ok('core_end');if(opened)engine.ok('close');
  }
}
