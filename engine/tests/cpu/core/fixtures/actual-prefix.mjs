import assert from 'node:assert/strict';
import {existsSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import {createHash} from 'node:crypto';
import {readEngine} from '../../../support/wasm/engine.mjs';
import {Engine,KEY,record,view} from '../../../../../sdk/runtime/engine.js';
import {instantiateCore,instantiateSupport} from './link.mjs';
import {assertCoreNames} from './names.mjs';

const FP=4236,SIMD=4364,YMM=4612,PAGE=4096;
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
let artifact;

function actualEngine() {
  if(artifact)return artifact;
  const path=process.env.RING3_ENGINE_WASM??fileURLToPath(new URL('../../../../../target/wasm32-unknown-unknown/release/ring3_engine.wasm',import.meta.url));
  assert(existsSync(path),'exact-prefix generated tests require the current actual engine; set RING3_ENGINE_WASM');
  artifact=readEngine(path);
  assert(WebAssembly.Module.exports(artifact.module).some(item=>item.name==='ring3_abi_v1_core_precise'),
    'actual engine provides the required exact-prefix CPU adapter');
  return artifact;
}

// The authored inputs and literal outcome are shared with the standalone hot
// lowering controls. No standalone module, identity, Context or memory is used
// here: preparation and precise execution belong to this same real CPU lease.
export function runActualPrefix(program,name,label,budget,options={},outcome,{instrument,after,onRow}={}) {
  const artifact=actualEngine();
  const instance=instantiateCore(artifact.module,{ring3_observability:{now_ns:()=>0n}});
  const api=Object.fromEntries(Object.entries(instance.exports).filter(([key])=>key.startsWith('ring3_abi_v1_'))
    .map(([key,value])=>[key.slice(13),value]));
  const memory=instance.exports.memory,engine=new Engine(api,memory,artifact.sha256);
  const bytes=(at,length)=>new Uint8Array(memory.buffer).slice(at,at+length);
  const word=(at)=>new DataView(memory.buffer).getUint32(at,true);
  const put=(at,value)=>new DataView(memory.buffer).setUint32(at,value>>>0,true);
  const bigint=(at)=>new DataView(memory.buffer).getBigUint64(at,true);
  const setBig=(at,value)=>new DataView(memory.buffer).setBigUint64(at,value,true);
  const regs=options.regs??[5,3,7,0x3000,0x4800,0,0x8000,0];
  const initial={regs:[...regs],flags:options.flags??2,retired:options.retired??0};
  const expected=typeof outcome==='function'?outcome(initial):outcome;
  const pages=new Map([0x1000,0x3000,0x4000,0x5000].map(address=>[address,new Uint8Array(PAGE)]));
  new DataView(pages.get(0x4000).buffer).setUint32(0x800,options.stackReturn??0x8000,true);
  for(const[address,page]of options.initialPages??[]) {
    assert.equal(page.length,PAGE);pages.set(address,Uint8Array.from(page));
  }
  pages.get(0x1000).set(program.code);
  if(options.mapZero)pages.set(0,new Uint8Array(PAGE));
  for(const[address,width,value]of options.initial??[])for(let index=0;index<width;index++) {
    const current=(address+index)>>>0,page=pages.get(current&~4095);
    assert(page,`${label}: authored input is mapped`);page[current&4095]=value>>>8*index;
  }
  if(options.unmapStack)pages.delete(0x4000);
  for(const address of options.missing??[])pages.delete(address);
  assert(!options.privateCopy&&!options.numericStore&&!options.executableAlias&&!options.guarded,
    'authored mock COW/numeric/guard controls remain on the standalone full-span backend');
  let opened=false,active=false;
  try {
    engine.ok('open',16,...KEY);opened=true;engine.base=api.arena_ptr()>>>0;
    for(const[address,page]of pages) {
      const permission=address===0x1000?7:address===0x3000&&options.readOnly?1:
        address===0x4000&&options.secondReadOnly?1:3;
      engine.ok('map',address,1,address===0x1000?7:3);engine.request(page);engine.ok('upload',address,PAGE);
      if(permission!==3&&address!==0x1000)engine.ok('protect',address,1,permission);
    }
    const state=record('R3ST',56),s=view(state);
    regs.forEach((value,index)=>s.setUint32(16+4*index,value,true));
    s.setUint32(48,options.pc??program.entry??0x1000,true);s.setUint32(52,initial.flags,true);
    new Uint8Array(memory.buffer,engine.base,56).set(state);
    if(options.fpStatus!==undefined)new DataView(memory.buffer).setUint16(engine.base+FP+18,options.fpStatus,true);
    assert.equal(api.core_cold_run(0),2,'actual engine initializes without executing a guest instruction');
    const context=api.core_context_ptr()>>>0;
    assert(context>0&&context%8===0);
    const backing=word(context+64),capacity=word(context+68),versions=word(context+84),revision=word(context+80);
    setBig(revision,options.initialVersion??0n);
    for(let slot=0;slot<capacity;slot++)setBig(versions+slot*16+8,0n);
    for(const[address,version]of options.pageVersions??[]) {
      const slot=word(word(context+56)+(address>>>12)*4);assert(slot>0&&slot<=capacity);
      setBig(versions+(slot-1)*16+8,version);
    }
    for(const pc of program.frontiers??[])if(pc!==(program.entry??0x1000))
      engine.ok('core_prepare',pc,1,1);
    engine.ok('core_prepare',program.entry??0x1000,program.instructions??2048,program.blocks??128);
    const identity=bigint(context+120),source=bytes(api.core_code_ptr()>>>0,api.core_code_len()>>>0);
    const module=new WebAssembly.Module(source);assertCoreNames(module,program.entry??0x1000,identity);
    let preciseCalls=0,reads=0,writes=0,numericCalls=0;
    const support=instantiateSupport(memory);
    assert.equal(support.linkAbi?.value,typeof api.core_continuation_format==='function'?5:4,'actual engine support uses the required precise/continuation ABI');
    const imports={ring3:{...support,memory,
      read:(...args)=>{reads++;return api.core_read(...args);},
      write:(...args)=>{writes++;return api.core_write(...args);},
      numeric:(...args)=>{numericCalls++;return api.core_numeric(...args);},
      precise:(...args)=>{preciseCalls++;return api.core_precise(...args);},
    }};
    let generated=instantiateCore(instrument?new WebAssembly.Module(instrument(source)):module,imports);
    engine.ok('core_begin');active=true;
    put(context+40,initial.retired);put(context+44,0x12345678);
    const before=bytes(context,128),wanted=before.slice(),d=view(wanted);
    const bank=word(context+92),bankBefore=bytes(bank,512),fpBefore=bytes(engine.base+FP,128);
    const simdBefore=bytes(engine.base+SIMD,160),ymmBefore=bytes(engine.base+YMM,144);
    const ram=bytes(backing,capacity*PAGE),pageVersions=bytes(versions,capacity*16),v=view(pageVersions);
    const epochPointer=word(context+88),epoch=bigint(epochPointer);
    const physical=address=>{
      const slot=word(word(context+56)+(address>>>12)*4);
      assert(slot>0&&slot<=capacity,`${label}: literal destination is mapped`);
      return (slot-1)*PAGE+(address&4095);
    };
    for(const[ordinal,[address,width,value]]of(expected.writes??[]).entries())for(let index=0;index<width;index++) {
      const at=physical((address+index)>>>0);
      ram[at]=value instanceof Uint8Array?value[index]:value>>>8*index;
      v.setBigUint64(Math.floor(at/PAGE)*16+8,(options.initialVersion??0n)+BigInt(ordinal+1),true);
    }
    (expected.regs??initial.regs).forEach((value,index)=>d.setUint32(index*4,value>>>0,true));
    d.setUint32(32,expected.pc,true);d.setUint32(36,expected.flags??initial.flags,true);
    d.setUint32(40,initial.retired+expected.retired,true);
    d.setUint32(44,expected.remaining??budget-(expected.work??expected.retired),true);
    if(expected.fault) {
      d.setUint32(48,expected.fault[0],true);d.setUint32(52,expected.fault[1],true);
      // A complete one-work span can still use the generated hot port, whose
      // diagnostic word is just the vector. Authored precise rows use width
      // plus vector. Address/access/width guest oracles remain identical.
      d.setUint32(116,expected.fault[2]??(((expected.fault[1]&255)<<8)|14),true);
    }
    if(expected.reason===6)d.setBigUint64(120,0n,true);
    let reason,available=budget,expectedIdentity=identity;
    const activations=[];
    do {
      assert(activations.length<=budget+1,`${label}: bounded genuine owner continuations`);
      const start=word(context+32),selected=bigint(context+120);
      reason=generated.exports.run(context,available)>>>0;
      activations.push({pc:start,identity:String(selected),reason,remaining:word(context+44)});
      available=word(context+44);
      if(reason!==1||!options.continueNeedCode)break;
      engine.ok('core_end');active=false;
      const target=word(context+32);
      engine.ok('core_prepare',target,program.instructions??2048,program.blocks??128);
      expectedIdentity=bigint(context+120);
      const encoded=bytes(api.core_code_ptr()>>>0,api.core_code_len()>>>0);
      generated=instantiateCore(instrument?new WebAssembly.Module(instrument(encoded)):new WebAssembly.Module(encoded),imports);
      engine.ok('core_begin');active=true;
    }while(true);
    if(expected.reason!==6)d.setBigUint64(120,expectedIdentity,true);
    const actual=bytes(context,128);
    const row={name,label,budget,reason,backend:name.startsWith('rep-')||name==='move-word-string'?
      'actual-engine-hot-string':'actual-engine-precise',engineSha256:artifact.sha256,
      identity:String(identity),wasmSha256:hash(source),context:Buffer.from(actual).toString('hex'),
      contextBefore:Buffer.from(before).toString('hex'),ramSha256:hash(bytes(backing,capacity*PAGE)),
      preciseCalls,helperReads:reads,helperWrites:writes,numericCalls,activations,
      transactionVersion:String(bigint(revision)),codeGeneration:String(bigint(epochPointer)),
      codeInvalidationProtocol:'native validation version is the committing store transaction stamp',
      authoredPortExpectation:{reads:expected.reads??0,writes:expected.helpers??0,
        scope:'standalone JS port counts do not count native precise accesses'},
      faultMetadataProtocol:expected.fault?(expected.fault[2]===14?'hot-vector-with-width-in-access':'native-width-and-vector'):undefined};
    onRow?.(row);
    assert.equal(reason,expected.reason,`${label}: actual precise stop`);
    assert.deepEqual(actual,wanted,`${label}: complete actual Context including lease/source authority`);
    assert.deepEqual(bytes(backing,capacity*PAGE),ram,`${label}: all actual guest backing bytes`);
    assert.deepEqual(bytes(versions,capacity*16),pageVersions,`${label}: all actual page identities and versions`);
    assert.equal(bigint(revision),(options.initialVersion??0n)+BigInt(expected.writes?.length??0),`${label}: exact store sequence`);
    const expectedCodeVersion=expected.reason===6
      ?(options.initialVersion??0n)+BigInt(expected.writes?.length??0):epoch;
    assert.equal(bigint(epochPointer),expectedCodeVersion,`${label}: exact executable-write invalidation stamp`);
    assert.deepEqual(bytes(bank,512),bankBefore,`${label}: complete actual canonical numerical/vector bank`);
    assert.deepEqual(bytes(engine.base+FP,128),fpBefore,`${label}: architectural FP image`);
    assert.deepEqual(bytes(engine.base+SIMD,160),simdBefore,`${label}: architectural SIMD image`);
    assert.deepEqual(bytes(engine.base+YMM,144),ymmBefore,`${label}: architectural YMM image`);
    if(budget>0&&(name.startsWith('rep-')||name==='move-word-string'))
      assert.equal(preciseCalls,0,`${label}: the admitted string kernel owns its exact element work without the precise adapter`);
    after?.({generated,api,memory,context,row});
    row.semanticOraclePassed=true;return row;
  } finally {
    if(active)engine.ok('core_end');
    if(opened)engine.ok('close');
  }
}
