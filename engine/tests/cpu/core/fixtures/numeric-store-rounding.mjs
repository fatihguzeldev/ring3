import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync,writeFileSync,openSync,closeSync} from 'node:fs';
import {readEngine} from '../../../support/wasm/engine.mjs';

const [enginePath,reportPath]=process.argv.slice(2);
assert(enginePath&&reportPath,'usage: numeric-store-rounding.mjs engine.wasm fresh-report.json');
const engine=readEngine(enginePath),output=openSync(reportPath,'wx');
const hex=bytes=>Buffer.from(bytes).toString('hex'),unhex=value=>Uint8Array.from(Buffer.from(value.replaceAll(' ',''),'hex'));
const hash=value=>createHash('sha256').update(value).digest('hex');
const report={complete:false,engineSha256:engine.sha256,sourceSha256:hash(readFileSync(new URL(import.meta.url))),
  oracle:'exact BigInt IEEE encoding from raw Extended80, including directed rounding and flags',cases:[],
  limits:['authored numerical stores and precise memory boundaries; no performance or game claim']};
const checkpoint=()=>writeFileSync(reportPath,JSON.stringify(report,null,2));
const ONE=1n<<63n,FP=4236,SIMD=4364,YMM=4612;
let stage='initialization';

function extended(significand,exponent,negative){
  const bytes=new Uint8Array(10),view=new DataView(bytes.buffer);view.setBigUint64(0,significand,true);
  view.setUint16(8,(significand===0n?0:16383+exponent)|(negative?0x8000:0),true);return bytes;
}
function pack(significand,exponent,negative,width,mode){
  const precision=width===4?24:53,bias=width===4?127:1023,minimum=1-bias;
  const sign=negative?1n<<BigInt(width*8-1):0n;
  if(significand===0n)return {bits:sign,flags:0,c1:false};
  const tiny=exponent<minimum;let shift=64-precision+(tiny?minimum-exponent:0);
  const denominator=1n<<BigInt(shift),remainder=significand%denominator;
  let retained=significand/denominator;
  const c1=remainder!==0n&&(mode===0?(2n*remainder>denominator||(2n*remainder===denominator&&(retained&1n)!==0n)):mode===1?negative:mode===2?!negative:false);
  retained+=c1?1n:0n;
  if(tiny)exponent=minimum;
  if(retained===(1n<<BigInt(precision))){retained>>=1n;exponent++;}
  if(exponent>bias){
    const infinity=mode===0||mode===1&&negative||mode===2&&!negative;
    const magnitude=infinity?BigInt(2*bias+1)<<BigInt(precision-1):(BigInt(2*bias)<<BigInt(precision-1))|((1n<<BigInt(precision-1))-1n);
    return {bits:sign|magnitude,flags:0x28,c1:infinity};
  }
  const normal=retained>=(1n<<BigInt(precision-1));
  const magnitude=(normal?BigInt(exponent+bias)<<BigInt(precision-1):0n)|(retained&((1n<<BigInt(precision-1))-1n));
  return {bits:sign|magnitude,flags:(remainder!==0n?0x20:0)|(tiny&&remainder!==0n?0x10:0),c1};
}
function encoded(bits,width){const result=new Uint8Array(width),view=new DataView(result.buffer);if(width===4)view.setUint32(0,Number(bits),true);else view.setBigUint64(0,bits,true);return result;}

function runCase(name,options){
  stage=name;const instance=new WebAssembly.Instance(engine.module,{ring3_observability:{now_ns:()=>0n}}),memory=instance.exports.memory;
  const api=Object.fromEntries(Object.entries(instance.exports).filter(([key])=>key.startsWith('ring3_abi_v1_')).map(([key,value])=>[key.slice(13),value]));
  const view=()=>new DataView(memory.buffer),slice=(at,length)=>new Uint8Array(memory.buffer).slice(at,at+length);
  const word=(at,offset=0)=>view().getUint32(at+offset,true),put=(at,offset,value)=>view().setUint32(at+offset,value>>>0,true);
  const width=options.width??4,mode=options.mode??0,negative=options.negative??false,pop=options.pop??false;
  const significand=options.significand??ONE,exponent=options.exponent??0,base=options.base??0x3000,top=options.top??0;
  const fault=options.fault||options.denied;
  const result=pack(significand,exponent,negative,width,mode),bytes=encoded(result.bits,width),prefix=options.prefix||options.scalarEdges||options.scalarRead?1:0;
  const prefixCode=options.scalarEdges?'894310':options.scalarRead?'8b4310':options.prefix?'90':'',prefixBytes=prefixCode.length/2;
  const code=unhex(`${prefixCode}${width===4?'d9':'dd'}${pop?'1b':'13'}${options.scalarEdges?'894320':''}`);
  const row={name,width,mode,negative,pop,input:hex(extended(significand,exponent,negative)),oracle:{bits:String(result.bits),flags:result.flags,c1:result.c1},controls:[]};report.cases.push(row);
  const status=(operation,wanted,...args)=>{const actual=api[operation](...args)>>>0;row.controls.push({operation,wanted,actual});assert.equal(actual,wanted,`${name}: ${operation}`);};
  let opened=false,active=false,helpers=0;
  try{
    status('open',0,12,97,0);opened=true;const arena=api.arena_ptr()>>>0;
    const pages=new Map([[0x1000,{permission:7,bytes:new Uint8Array(4096)}],[0x3000,{permission:3,bytes:new Uint8Array(4096)}]]);
    if(!options.fault)pages.set(0x4000,{permission:3,bytes:new Uint8Array(4096)});
    pages.get(0x1000).bytes.set(code);pages.get(0x1000).bytes.set(unhex('0f0b'),code.length);pages.get(0x3000).bytes.fill(0xa5);
    const expectedPages=new Map([...pages].map(([address,page])=>[address,page.bytes.slice()]));
    const writes=[];
    if(options.scalarEdges)writes.push([base+16,new Uint8Array(4)]);
    if(!fault){writes.push([base,bytes]);if(options.scalarEdges)writes.push([base+32,new Uint8Array(4)]);}
    for(const[address,bytes]of writes)for(let index=0;index<bytes.length;index++)expectedPages.get((address+index)&~4095)[(address+index)&4095]=bytes[index];
    for(const[address,page]of pages){status('map',0,address,1,page.permission);new Uint8Array(memory.buffer,arena+140,4096).set(page.bytes);status('upload',0,address,4096);}
    if(options.denied)status('protect',0,0x3000,1,1);
    const registers=[0,0,0,base,0x4800,0,0,0],header=new Uint8Array(memory.buffer,arena,56);header.fill(0);header.set([82,51,83,84]);
    view().setUint16(arena+4,3,true);view().setUint16(arena+6,1,true);put(arena,8,56);registers.forEach((value,index)=>put(arena,16+index*4,value));put(arena,48,0x1000);put(arena,52,0x402);
    const initial=slice(arena+FP,128),iv=new DataView(initial.buffer),control=0x3f|(options.precision??0x200)|(mode<<10);
    iv.setUint16(16,control,true);iv.setUint16(18,(options.sticky?0x4720:0x4700)|(top<<11),true);iv.setUint16(20,(0xffff&~(3<<(top*2)))|((significand===0n?1:0)<<(top*2)),true);
    iv.setUint16(22,0x345,true);iv.setUint32(24,0x12345678,true);iv.setUint32(28,0x87654321,true);iv.setUint16(32,0x43,true);iv.setUint16(34,0x4b,true);initial.set(extended(significand,exponent,negative),40+top*10);
    new Uint8Array(memory.buffer,arena+FP,128).set(initial);const initialSimd=slice(arena+SIMD,160),initialYmm=slice(arena+YMM,144);
    const expected=initial.slice(),ev=new DataView(expected.buffer);
    if(!fault){
      let fsw=(iv.getUint16(18,true)&~0x200)|result.flags|(result.c1?0x200:0);
      if(pop){fsw=(fsw&~0x3800)|(((top+1)&7)<<11);ev.setUint16(20,iv.getUint16(20,true)|(3<<(top*2)),true);}
      ev.setUint16(18,fsw,true);ev.setUint16(22,(width===4?0x100:0x500)|(pop?0x1b:0x13),true);ev.setUint32(24,0x1000+prefixBytes,true);ev.setUint32(28,base,true);ev.setUint16(32,0x1b,true);ev.setUint16(34,0x23,true);
    }
    status('core_prepare',0,0x1000,256,32);const context=api.core_context_ptr()>>>0;
    const physical=address=>{const slot=word(word(context,56)+(address>>>12)*4);assert(slot>0);return word(context,64)+(slot-1)*4096+(address&4095);};
    const module=new WebAssembly.Module(slice(api.core_code_ptr()>>>0,api.core_code_len()>>>0));
    const importVersions=[];
    const numeric=(...args)=>{helpers++;importVersions.push(String(view().getBigUint64(word(context,80),true)));return api.core_numeric(...args);};
    const run=new WebAssembly.Instance(module,{ring3:{memory,read:api.core_read,write:api.core_write,numeric}}).exports.run;
    status('core_begin',0);active=true;
    const transactionPointer=word(context,80),version=view().getBigUint64(transactionPointer,true),pageVersions=slice(word(context,84),word(context,68)*16);
    const completed=1+prefix+(options.scalarEdges?1:0),budget=fault||options.smc?8:completed,retired=fault?prefix:completed,stop=run(context,budget)>>>0,wantedStop=fault?3:options.smc?6:2;
    row.actual={stop,helpers,retired:word(context,40),pc:word(context,32),importVersions};checkpoint();
    assert.equal(stop,wantedStop);assert.equal(word(context,40),retired);assert.equal(word(context,44),budget-retired);assert.equal(word(context,32),0x1000+(fault?prefixBytes:code.length));assert.equal(word(context,36),0x402);
    if(fault){assert.equal(word(context,116),(width<<8)|14);assert.equal(word(context,48),options.denied?base:0x4000);}
    status('core_end',0);active=false;assert.deepEqual(slice(arena+FP,128),initial,'no resident publication before export');status('core_export',0,stop,retired);
    row.actual.fp=hex(slice(arena+FP,128));row.expectedFp=hex(expected);checkpoint();
    assert.deepEqual(slice(arena+FP,128),expected,`${name}: full FP128 including stale physical payloads`);assert.deepEqual(slice(arena+SIMD,160),initialSimd);assert.deepEqual(slice(arena+YMM,144),initialYmm);
    const expectedRegisters=[...registers];if(options.scalarRead)expectedRegisters[0]=0xa5a5a5a5;
    assert.deepEqual(Array.from({length:8},(_,index)=>word(context,index*4)),expectedRegisters);
    for(const[address,page]of expectedPages)assert.deepEqual(slice(physical(address),4096),page,`${name}: entire RAM page`);
    assert.equal(view().getBigUint64(transactionPointer,true),version+BigInt(writes.length),'one transaction per complete store');
    for(const[ordinal,[address,bytes]]of writes.entries())for(const page of new Set([address&~4095,(address+bytes.length-1)&~4095])){const slot=word(word(context,56)+(page>>>12)*4)-1;new DataView(pageVersions.buffer).setBigUint64(slot*16+8,version+BigInt(ordinal+1),true);}
    assert.deepEqual(slice(word(context,84),pageVersions.length),pageVersions,'all physical page versions');
    if(options.scalarEdges&&helpers)assert.deepEqual(importVersions,[String(version+1n)],'numeric helper sees the preceding scalar store version');
    assert.equal(helpers,options.helpers??0,`${name}: direct versus canonical helper admission`);row.complete=true;checkpoint();
  }finally{if(active)api.core_end();if(opened)api.close();}
}

try{
  for(const width of[4,8]){
    const precision=width===4?24:53,unit=1n<<BigInt(64-precision),half=unit>>1n;
    const inputs=[['exact',ONE+unit],['below-half',ONE+half-1n],['tie-even',ONE+half],['tie-odd',ONE+unit+half],['above-half',ONE+half+1n],['significand-carry',(1n<<64n)-1n]];
    for(const[pattern,significand]of inputs)for(const mode of[0,1,2,3])for(const negative of[false,true]){
      runCase(`f${width*8}-${pattern}-rc${mode}-${negative?'negative':'positive'}`,{width,significand,mode,negative,pop:(mode&1)!==0,top:pattern==='significand-carry'?7:0});
    }
    for(const negative of[false,true])runCase(`f${width*8}-${negative?'negative':'positive'}-zero-pop-wrap`,{width,significand:0n,negative,pop:true,top:7});
    for(const precision of[0,0x200,0x300])runCase(`f${width*8}-store-ignores-pc${precision.toString(16)}`,{width,significand:ONE+half,precision});
    runCase(`f${width*8}-exact-store-keeps-sticky-pe-and-clears-c1`,{width,sticky:true});
    runCase(`f${width*8}-crosspage-pop-commits-once`,{width,significand:ONE+half+1n,pop:true,base:0x3ffe,helpers:1});
    runCase(`f${width*8}-late-fault-does-not-publish-rounding-or-pop`,{width,significand:ONE+half+1n,pop:true,base:0x3ffe,prefix:true,fault:true,helpers:1});
    runCase(`f${width*8}-code-store-commits-prefix-rounding-and-pop`,{width,significand:ONE+half+1n,pop:true,base:0x1100,prefix:true,smc:true,helpers:1});
    runCase(`f${width*8}-normal-range-overflow-carry-is-canonical`,{width,significand:(1n<<64n)-1n,exponent:width===4?127:1023,helpers:1});
    runCase(`f${width*8}-exact-subnormal-output-is-canonical`,{width,exponent:width===4?-127:-1023,helpers:1});
  }
  runCase('scalar-direct-fst-scalar-preserves-three-store-versions',{scalarEdges:true});
  runCase('scalar-canonical-fst-scalar-preserves-three-store-versions',{scalarEdges:true,exponent:-127,helpers:1});
  runCase('scalar-read-does-not-authorize-numeric-store',{scalarRead:true,denied:true,helpers:1});
  report.complete=true;checkpoint();console.log(JSON.stringify({complete:true,cases:report.cases.length,engineSha256:engine.sha256}));
}catch(error){report.failure={stage,message:String(error),stack:error.stack};checkpoint();throw error;}
finally{closeSync(output);}
