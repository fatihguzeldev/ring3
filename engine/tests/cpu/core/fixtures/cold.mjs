import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync,writeFileSync,openSync,closeSync} from 'node:fs';
import {readEngine} from '../../../support/wasm/engine.mjs';
import {Engine,KEY,record,view} from '../../../../../sdk/runtime/engine.js';
import {CodeCache} from '../../../../../sdk/runtime/cache.js';
import {makeKernel} from './kernels.mjs';

const [enginePath,reportPath]=process.argv.slice(2);
assert(enginePath&&reportPath,'usage: cold.mjs engine.wasm fresh-report.json');
const artifact=readEngine(enginePath),hash=bytes=>createHash('sha256').update(bytes).digest('hex');
const hex=bytes=>Buffer.from(bytes).toString('hex'),unhex=text=>Uint8Array.from(Buffer.from(text.replaceAll(' ',''),'hex'));
const sources=['./cold.mjs','./kernels.mjs','../../../support/wasm/engine.mjs',
  '../../../../../sdk/runtime/engine.js','../../../../../sdk/runtime/engine/module.js','../../../../../sdk/runtime/cache.js'];
const report={schema:1,complete:false,engineSha256:artifact.sha256,
  sources:Object.fromEntries(sources.map(path=>[path,hash(readFileSync(new URL(path,import.meta.url)))])),
  scope:'authored cold execution, precise native boundaries and private numerical state across JIT handoff; no timing or game claim',cases:[]};
closeSync(openSync(reportPath,'wx'));
const checkpoint=()=>writeFileSync(reportPath,JSON.stringify(report,null,2));
let stage='initialization';checkpoint();

function runCase(name,spec) {
  stage=name;
  const result={name,runs:[],modules:[],controls:[],complete:false};report.cases.push(result);checkpoint();
  const instance=new WebAssembly.Instance(artifact.module,{ring3_observability:{now_ns:()=>0n}});
  const native=Object.fromEntries(Object.entries(instance.exports).filter(([name])=>name.startsWith('ring3_abi_v1_'))
    .map(([name,value])=>[name.slice(13),value]));
  const engine=new Engine(Object.freeze(native),instance.exports.memory,artifact.sha256);
  const bytes=(pointer,length)=>new Uint8Array(engine.memory.buffer).slice(pointer,pointer+length);
  const word=(pointer,offset=0)=>new DataView(engine.memory.buffer).getUint32(pointer+offset,true);
  const pages=[new Uint8Array(4096),new Uint8Array(4096),new Uint8Array(4096)];
  pages[0].set(typeof spec.code==='string'?unhex(spec.code):spec.code);spec.data?.(new DataView(pages[1].buffer));
  const expectedPages=pages.map(page=>page.slice());spec.pages?.(expectedPages);
  let cache,context=0,opened=false;
  const raw=(full=false)=>{
    const backing=context?word(context,64):0;
    return {contextHex:context?hex(bytes(context,128)):null,arenaHex:hex(bytes(engine.base,native.arena_len())),
      ramSha256:backing?hash(bytes(backing,16*4096)):null,...(full&&backing?{ramHex:hex(bytes(backing,16*4096))}:{})};
  };
  const selection=()=>({context:native.core_context_ptr()>>>0,code:native.core_code_ptr()>>>0,length:native.core_code_len()>>>0,
    identity:context?new DataView(engine.memory.buffer).getBigUint64(context+120,true).toString():null});
  const rejectInvalidFlags=label=>{
    const flags=word(context,36);
    new DataView(engine.memory.buffer).setUint32(context+36,flags&~2,true);
    try {
      const before={...raw(true),selection:selection()},actual=native.core_cold_run(1);
      const after={...raw(true),selection:selection()};
      result.controls.push({label,actual,wanted:-7,before,after});checkpoint();
      assert.equal(actual,-7,`${label}: invalid FLAGS admission`);
      assert.deepEqual(after,before,`${label}: complete context, arena, RAM and selected code remain unchanged`);
    } finally {new DataView(engine.memory.buffer).setUint32(context+36,flags,true);}
  };
  try {
    assert.equal(typeof native.core_cold_run,'function','actual cold native export is required');
    const closed=native.core_cold_run(1);result.controls.push({label:'before open',actual:closed,wanted:-5});checkpoint();
    assert.equal(closed,-5,'cold execution requires an open instance');
    engine.ok('open',16,...KEY);opened=true;engine.base=native.arena_ptr()>>>0;
    for(const [index,address] of [0x1000,0x3000,0x4000].entries()) {
      engine.ok('map',address,1,index===0?7:3);
      engine.request(pages[index]);engine.ok('upload',address,4096);
    }
    if(spec.codePermissions!==undefined)engine.ok('protect',0x1000,1,spec.codePermissions);
    if(spec.permissions!==undefined)engine.ok('protect',0x3000,1,spec.permissions);
    const state=record('R3ST',56),stateView=view(state);stateView.setUint16(4,3,true);
    const registers=spec.registers??[11,12,13,0x3000,0x4800,15,16,17],flags=spec.flags??0x403;
    registers.forEach((value,index)=>stateView.setUint32(16+index*4,value,true));
    stateView.setUint32(48,0x1000,true);stateView.setUint32(52,flags,true);
    new Uint8Array(engine.memory.buffer,engine.base,56).set(state);
    const uninitialized={...raw(),selection:selection()};
    assert.deepEqual(uninitialized.selection,{context:0,code:0,length:0,identity:null});
    const imported=native.core_import(),afterImport={...raw(),selection:selection()};
    result.controls.push({label:'import before initialization',actual:imported,wanted:7,before:uninitialized,after:afterImport});checkpoint();
    assert.equal(imported,7);assert.deepEqual(afterImport,uninitialized,'rejected initial import creates no context or code');
    const untouchedArena=bytes(engine.base,native.arena_len());
    assert.equal(native.core_cold_run(0),2,'zero-budget initialization executes no guest instruction');
    context=native.core_context_ptr()>>>0;assert(context>0&&context%8===0);
    assert.equal(word(context,40),0);assert.equal(word(context,44),0);
    assert.equal(new DataView(engine.memory.buffer).getBigUint64(context+120,true),0n);
    assert.deepEqual(selection(),{context,code:0,length:0,identity:'0'},'cold owner has no compiled selection');
    assert.deepEqual(bytes(engine.base,native.arena_len()),untouchedArena,'initialization leaves canonical arena untouched');
    rejectInvalidFlags('unselected cold owner');
    const initialContext=bytes(context,128),expectedArena=untouchedArena.slice();
    const expectedRam=new Uint8Array(16*4096);expectedPages.forEach((page,index)=>expectedRam.set(page,index*4096));
    cache=new CodeCache(engine,{chain:spec.chain??false,budget:spec.budget,instructions:64,blocks:8});
    for(const name of ['core_cold_run','core_read','core_write','core_numeric'])assert.equal(cache.native[name],native[name]);
    function capture(selected) {
      if(selected&&!result.modules.some(item=>item.id===String(selected.id))) {
        const code=bytes(native.core_code_ptr()>>>0,native.core_code_len()>>>0);
        result.modules.push({id:String(selected.id),pc:selected.pc,bytes:code.length,sha256:hash(code),
          ...(spec.seedEntry!==undefined?{wasmHex:hex(code),atRetired:word(context,40),eax:word(context)}:{})});
      }
    }
    if(spec.seedEntry!==undefined) {
      const seed=cache.entry(spec.seedEntry);capture(seed);
      result.seeded={id:String(seed.id),pc:seed.pc,...raw()};checkpoint();
      assert.equal(word(context,32),0x1000,'seeding compiled code preserves the cold architectural entry');
    }
    let work=0,retired=0,last;
    for(let index=0;index<(spec.maxRuns??8);index++) {
      cache.budget=spec.budget-work;
      const selected=cache.select();capture(selected);
      if(selected&&name==='private-x87-bank-across-jit')rejectInvalidFlags('selected compiled owner before JIT handoff');
      last=cache.execute(selected);work+=last.work;retired+=last.retired;
      result.runs.push({...last,id:last.id===null?null:String(last.id),...raw()});checkpoint();
      if(last.kind==='cold')assert.equal(new DataView(engine.memory.buffer).getBigUint64(context+120,true),0n);
      if(last.coreReason!==1)break;
      assert.deepEqual(bytes(engine.base+4236,128),untouchedArena.slice(4236,4364),'private numerical state is not reimported at NeedCode');
    }
    result.final=raw(true);result.stats=cache.stats();checkpoint();
    assert.equal(last.coreReason,spec.stop??2);assert.equal(work,spec.work??spec.budget);assert.equal(retired,spec.retired??work);
    const expectedRegisters=spec.expectedRegisters??registers,expectedFlags=spec.expectedFlags??flags;
    const expectedContext=initialContext.slice(),contextView=view(expectedContext);
    expectedRegisters.forEach((value,index)=>contextView.setUint32(index*4,value,true));
    for(const [offset,value] of [[32,spec.pc],[36,expectedFlags],[40,retired],[44,spec.budget-work],
      [48,spec.fault?.address??0],[52,spec.fault?.metadata??0],[112,0],[116,spec.fault?.vector??0]])contextView.setUint32(offset,value,true);
    const finalIdentity=last.kind==='compiled'?last.id:0n;
    contextView.setBigUint64(120,finalIdentity,true);
    assert.deepEqual(bytes(context,128),expectedContext,'complete architectural context and unchanged pointer metadata');
    const finalState=state.slice(),finalStateView=view(finalState);
    finalStateView.setUint16(4,1,true);
    expectedRegisters.forEach((value,index)=>finalStateView.setUint32(16+index*4,value,true));
    finalStateView.setUint32(48,spec.pc,true);finalStateView.setUint32(52,expectedFlags,true);expectedArena.set(finalState);
    const exit=record('R3EX',40),exitView=view(exit);exitView.setUint16(4,15,true);
    exitView.setUint32(16,last.coreReason===3?5:last.coreReason===6?6:1,true);exitView.setUint32(20,retired,true);
    if(spec.fault)for(const [offset,value] of [[24,2],[28,spec.fault.address],[32,2],[36,4]])exitView.setUint32(offset,value,true);
    expectedArena.set(exit,56);
    const transport=record('R3WK',24);view(transport).setUint32(16,work,true);expectedArena.set(transport,4588);
    spec.bank?.(expectedArena);
    assert.deepEqual(bytes(engine.base,native.arena_len()),expectedArena,'complete canonical arena, FP80 and other banks');
    assert.deepEqual([1,3,4].map(page=>word(word(context,56)+page*4)),[1,2,3]);assert.equal(word(context,68),16);
    assert.deepEqual(bytes(word(context,64),16*4096),expectedRam,'whole canonical allocation and literal code/data/stack');
    if(spec.collidingHeat) {
      const first=result.runs[0],prefix=view(unhex(first.contextHex));
      assert.deepEqual({kind:first.kind,stop:first.coreReason,retired:first.retired,work:first.work,
        eax:prefix.getUint32(0,true),pc:prefix.getUint32(32,true),flags:prefix.getUint32(36,true),remaining:prefix.getUint32(44,true)},
        {kind:'cold',stop:1,retired:256,work:256,eax:128,pc:0x1000,flags:0x12,remaining:1},
        'four repeatedly retired PCs request compilation before the 129th INC');
    }
    if(spec.runs!==false)assert.deepEqual(result.runs.map(({kind,coreReason})=>[kind,coreReason]),spec.runs??[['cold',spec.stop??2]]);
    assert.equal(result.modules.length,spec.modules??0);assert.equal(cache.stats().admissions,spec.modules??0);
    if(spec.seedEntry!==undefined) {
      assert(result.runs.some(run=>run.kind==='cold'));assert(result.runs.some(run=>run.kind==='compiled'));
      assert.deepEqual(result.modules.map(({pc,atRetired,eax})=>[pc,atRetired,eax]),[[0x1040,0,0],[0x1000,192,64]],
        'caller compilation follows 64 completed INC visits across compiled-callee handoffs');
      assert.equal(last.kind,'compiled');assert(last.regions>1,'retained caller and callee execute through the warm chain');
    }
    result.complete=true;checkpoint();
  } catch(error) {result.failure={stage,message:String(error),stack:error.stack,raw:raw(true)};checkpoint();throw error;}
  finally {if(cache?.open)native.core_end();if(opened)engine.ok('close');}
}

try {
  runCase('integer-flags-store',{code:'830301 0f0b',budget:1,pc:0x1003,expectedFlags:0xc96,
    data:d=>d.setUint32(0,0x7fffffff,true),pages:pages=>view(pages[1]).setUint32(0,0x80000000,true)});
  runCase('modify-permission-fault',{code:'830301 0f0b',budget:2,pc:0x1000,permissions:1,stop:3,work:0,retired:0,
    data:d=>d.setUint32(0,0x7fffffff,true),fault:{address:0x3000,metadata:0x20204,vector:0x40e}});
  runCase('committed-code-store',{code:'c60390 0f0b',budget:2,pc:0x1003,stop:6,work:1,
    registers:[11,12,13,0x1000,0x4800,15,16,17],pages:pages=>{pages[0][0]=0x90;}});
  const call=makeKernel('call',3);
  runCase('call-return-cold',{code:call.code,budget:call.retired,pc:call.pc,flags:2,expectedFlags:call.flags,
    registers:call.initialRegs,expectedRegisters:call.regs,pages:pages=>{pages[1].set(call.expectedData);pages[2].set(call.expectedStack);}});
  runCase('private-x87-bank-across-jit',{code:'d903 f3a4 d84304 d95b08 0f0b',budget:6,pc:0x100a,retired:4,flags:2,
    registers:[0,3,0,0x3000,0x4800,0,0x3100,0x3120],expectedRegisters:[0,0,0,0x3000,0x4800,0,0x3103,0x3123],
    data:d=>{d.setFloat32(0,1.5,true);d.setFloat32(4,2.5,true);[11,22,33].forEach((value,index)=>d.setUint8(0x100+index,value));},
    pages:pages=>{view(pages[1]).setFloat32(8,4,true);pages[1].set([11,22,33],0x120);},
    bank:arena=>{const bank=arena.subarray(4236,4364),d=view(bank);d.setUint16(22,0x15b,true);d.setUint32(24,0x1007,true);
      d.setUint32(28,0x3008,true);d.setUint16(32,0x1b,true);d.setUint16(34,0x23,true);bank.set(unhex('00000000000000800140'),110);},
    runs:[['cold',1],['compiled',2]],modules:1});
  const heatCode=new Uint8Array(4096);heatCode.set([0x40,0xff,0xe6]);heatCode.set([0xff,0xe7],0x40);
  runCase('cold-heat-survives-compiled-frontier',{code:heatCode,budget:225,pc:0x1000,flags:2,expectedFlags:6,
    registers:[0,0,0,0x3000,0x4800,0,0x1040,0x1000],expectedRegisters:[75,0,0,0x3000,0x4800,0,0x1040,0x1000],
    chain:true,seedEntry:0x1040,maxRuns:256,runs:false,modules:2});
  const collisionTail=unhex('40e9fadfffff');
  runCase('cold-heat-four-repeated-pcs',{code:'40e9fa1f0000',budget:257,pc:0x1001,flags:2,expectedFlags:6,
    registers:[0,0,0,0x3000,0x4800,0,0,0],expectedRegisters:[129,0,0,0x3000,0x4800,0,0,0],
    codePermissions:5,permissions:5,data:d=>collisionTail.forEach((value,index)=>d.setUint8(index,value)),
    collidingHeat:true,runs:[['cold',1],['compiled',2]],modules:1});
  report.complete=true;checkpoint();console.log(JSON.stringify({complete:true,engineSha256:artifact.sha256,cases:report.cases.length}));
} catch(error) {report.failure={stage,message:String(error),stack:error.stack};checkpoint();throw error;}
