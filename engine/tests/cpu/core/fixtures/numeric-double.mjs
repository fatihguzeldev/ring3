import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync,writeFileSync,openSync,closeSync} from 'node:fs';
import {readEngine} from '../../../support/wasm/engine.mjs';

const [enginePath,reportPath]=process.argv.slice(2);
assert(enginePath&&reportPath,'usage: numeric-double.mjs engine.wasm fresh-report.json');
const engine=readEngine(enginePath),output=openSync(reportPath,'wx');
const hex=bytes=>Buffer.from(bytes).toString('hex');
const unhex=value=>Uint8Array.from(Buffer.from(value.replaceAll(' ',''),'hex'));
const hash=value=>createHash('sha256').update(value).digest('hex');
const report={complete:false,engineSha256:engine.sha256,sourceSha256:hash(readFileSync(new URL(import.meta.url))),
  oracle:'exact signed BigInt rational, independently rounded to the selected x87 precision',cases:[],
  limits:['authored PC53 numerical and precise memory boundaries; no game or performance claim']};
const checkpoint=()=>writeFileSync(reportPath,JSON.stringify(report,null,2));
const FP=4236,SIMD=4364,YMM=4612,ONE=1n<<63n;
let stage='initialization';

function ext(sig=ONE,exponent=0,negative=false) {
  const bytes=new Uint8Array(10),view=new DataView(bytes.buffer);
  view.setBigUint64(0,sig,true);view.setUint16(8,(sig===0n?0:16383+exponent)|(negative?0x8000:0),true);return bytes;
}
function parts(bytes) {
  const v=new DataView(bytes.buffer,bytes.byteOffset,bytes.byteLength),word=v.getUint16(8,true);
  return {sig:v.getBigUint64(0,true),exponent:(word&0x7fff)-16383,negative:(word&0x8000)!==0};
}
function rational(bytes) {
  const {sig,exponent,negative}=parts(bytes),shift=exponent-63;
  return {n:(negative?-sig:sig)*(shift>=0?1n<<BigInt(shift):1n),d:shift<0?1n<<BigInt(-shift):1n};
}
function oracle(kind,a,b,control) {
  if(kind==='rsub'||kind==='rdiv'){[a,b]=[b,a];kind=kind==='rsub'?'sub':'div';}
  let av=rational(a),bv=rational(b),negativeZero=false;
  const ap=parts(a),bp=parts(b);
  if(kind==='sub') {bv={n:-bv.n,d:bv.d};}
  let n,d;
  if(kind==='add'||kind==='sub') {
    n=av.n*bv.d+bv.n*av.d;d=av.d*bv.d;
    const bNegative=kind==='sub'?!bp.negative:bp.negative;
    negativeZero=ap.sig===0n&&bp.sig===0n&&ap.negative&&bNegative;
  } else if(kind==='mul') {n=av.n*bv.n;d=av.d*bv.d;negativeZero=ap.negative!==bp.negative;}
  else {assert(bv.n!==0n);n=av.n*bv.d;d=av.d*bv.n;negativeZero=ap.negative!==bp.negative;}
  if(d<0n){n=-n;d=-d;}
  const mode=(control>>>10)&3;
  if(n===0n)return {value:ext(0n,0,negativeZero||(mode===1&&(kind==='add'||kind==='sub')&&ap.negative!== (kind==='sub'?!bp.negative:bp.negative))),pe:false,c1:false};
  const negative=n<0n;if(negative)n=-n;
  let exponent=n.toString(2).length-d.toString(2).length;
  if(exponent>=0?n<(d<<BigInt(exponent)):(n<<BigInt(-exponent))<d)exponent--;
  const precision=(control&0x300)===0?24:(control&0x300)===0x200?53:64;
  const shift=precision-1-exponent;
  let numerator=n,denominator=d;
  if(shift>=0)numerator<<=BigInt(shift);else denominator<<=BigInt(-shift);
  let retained=numerator/denominator;const remainder=numerator%denominator;
  const c1=remainder!==0n&&(mode===0?(2n*remainder>denominator||(2n*remainder===denominator&&(retained&1n)!==0n)):mode===1?negative:mode===2?!negative:false);
  retained+=c1?1n:0n;
  if(retained===(1n<<BigInt(precision))){retained>>=1n;exponent++;}
  return {value:ext(retained<<BigInt(64-precision),exponent,negative),pe:remainder!==0n,c1};
}
const tag=value=>parts(value).sig===0n?1:0;
function putBank(image,values,status,control) {
  const view=new DataView(image.buffer);view.setUint16(16,control,true);view.setUint16(18,status,true);
  let tags=0xffff;values.forEach((value,index)=>{image.set(value,40+index*10);tags=(tags&~(3<<(index*2)))|(tag(value)<<(index*2));});view.setUint16(20,tags,true);
}

function runCase(name,options) {
  stage=name;const instance=new WebAssembly.Instance(engine.module,{ring3_observability:{now_ns:()=>0n}});
  const memory=instance.exports.memory,api=Object.fromEntries(Object.entries(instance.exports).filter(([key])=>key.startsWith('ring3_abi_v1_')).map(([key,value])=>[key.slice(13),value]));
  const view=()=>new DataView(memory.buffer),slice=(at,length)=>new Uint8Array(memory.buffer).slice(at,at+length);
  const word=(at,offset=0)=>view().getUint32(at+offset,true),put=(at,offset,value)=>view().setUint32(at+offset,value>>>0,true);
  const row={name,code:options.code??'d8c1',controls:[]};report.cases.push(row);
  const status=(operation,wanted,...args)=>{const actual=api[operation](...args)>>>0;row.controls.push({operation,wanted,actual});assert.equal(actual,wanted,`${name}: ${operation}`);};
  let opened=false,active=false,helpers=0;
  try {
    status('open',0,12,97,0);opened=true;const arena=api.arena_ptr()>>>0;
    const pages=new Map([[0x1000,{permission:7,bytes:new Uint8Array(4096)}],[0x3000,{permission:3,bytes:new Uint8Array(4096)}],[0x4000,{permission:3,bytes:new Uint8Array(4096)}]]);
    if(options.crossFault)pages.delete(0x4000);
    const code=unhex(row.code);pages.get(0x1000).bytes.set(code);pages.get(0x1000).bytes.set(unhex('0f0b'),code.length);
    for(const [address,bytes]of options.data??[])for(let i=0;i<bytes.length;i++)pages.get((address+i)&~4095).bytes[(address+i)&4095]=bytes[i];
    const expectedPages=new Map([...pages].map(([address,page])=>[address,page.bytes.slice()]));
    for(const [address,bytes]of options.writes??[])for(let i=0;i<bytes.length;i++)expectedPages.get((address+i)&~4095)[(address+i)&4095]=bytes[i];
    for(const [address,page]of pages){status('map',0,address,1,page.permission);new Uint8Array(memory.buffer,arena+140,4096).set(page.bytes);status('upload',0,address,4096);}
    const registers=[0,0,0,options.base??0x3000,0x4800,0,0,0],header=new Uint8Array(memory.buffer,arena,56);header.fill(0);header.set([82,51,83,84]);
    view().setUint16(arena+4,3,true);view().setUint16(arena+6,1,true);put(arena,8,56);registers.forEach((value,index)=>put(arena,16+index*4,value));put(arena,48,0x1000);put(arena,52,0x402);
    const initial=slice(arena+FP,128),control=options.control??0x23f,initialStatus=options.initialStatus??0x4700;
    const a=options.a??ext(),b=options.b??ext();putBank(initial,[a,b],initialStatus,control);new Uint8Array(memory.buffer,arena+FP,128).set(initial);
    const initialSimd=slice(arena+SIMD,160),initialYmm=slice(arena+YMM,144);
    const expected=initial.slice(),ev=new DataView(expected.buffer);
    if(options.result!==false) {
      const result=oracle(options.kind??'add',a,options.memoryValue??(options.alias?a:b),control),destination=options.pop?1:0;
      expected.set(result.value,40+destination*10);
      let tags=(ev.getUint16(20,true)&~(3<<(destination*2)))|(tag(result.value)<<(destination*2));
      let fsw=(initialStatus&~0x200)|(result.pe?0x20:0)|(result.c1?0x200:0);
      if(options.pop){tags|=3;fsw=(fsw&~0x3800)|0x800;}
      ev.setUint16(18,fsw,true);ev.setUint16(20,tags,true);ev.setUint16(22,options.opcode??0xc1,true);ev.setUint32(24,0x1000,true);ev.setUint16(32,0x1b,true);
      if(options.memoryValue){ev.setUint32(28,options.base??0x3000,true);ev.setUint16(34,0x23,true);}
      if(options.storeCode){ev.setUint16(18,(fsw&~0x3a00)|0x800,true);ev.setUint16(20,tags|3,true);ev.setUint16(22,0x51d,true);ev.setUint32(24,0x1002,true);ev.setUint32(28,0x1100,true);ev.setUint16(34,0x23,true);}
      row.expectedResult={value:hex(result.value),precision:control&0x300,pe:result.pe,c1:result.c1};
    }
    if(options.pending)ev.setUint16(18,initialStatus|0x8080,true);
    status('core_prepare',0,0x1000,256,32);const context=api.core_context_ptr()>>>0;
    const module=new WebAssembly.Module(slice(api.core_code_ptr()>>>0,api.core_code_len()>>>0));
    const numeric=(...args)=>{helpers++;return api.core_numeric(...args);};
    const run=new WebAssembly.Instance(module,{ring3:{memory,read:api.core_read,write:api.core_write,numeric}}).exports.run;
    status('core_begin',0);active=true;const budget=options.budget??1,retired=options.retired??budget,wantedStop=options.stop??2;
    const stop=run(context,budget)>>>0;row.actual={stop,helpers,retired:word(context,40),pc:word(context,32),vector:word(context,116),detail:word(context,48)};checkpoint();
    assert.equal(stop,wantedStop);assert.equal(word(context,40),retired);assert.equal(word(context,44),budget-retired);assert.equal(word(context,32),options.pc??0x1000+code.length);assert.equal(word(context,36),0x402);
    if(options.vector!==undefined)assert.equal(word(context,116),options.vector);if(options.detail!==undefined)assert.equal(word(context,48),options.detail);
    status('core_end',0);active=false;assert.deepEqual(slice(arena+FP,128),initial,'resident state is unpublished until export');
    status('core_export',0,stop,retired);row.actual.fp=hex(slice(arena+FP,128));row.expectedFp=hex(expected);checkpoint();
    assert.deepEqual(slice(arena+FP,128),expected,`${name}: full literal/rational FP128`);assert.deepEqual(slice(arena+SIMD,160),initialSimd);assert.deepEqual(slice(arena+YMM,144),initialYmm);
    assert.deepEqual(Array.from({length:8},(_,i)=>word(context,i*4)),registers);
    const physical=address=>{const slot=word(word(context,56)+(address>>>12)*4);assert(slot>0);return word(context,64)+(slot-1)*4096+(address&4095);};
    for(const[address,bytes]of expectedPages)assert.deepEqual(slice(physical(address),4096),bytes,`${name}: entire RAM page`);
    assert.equal(helpers,options.helpers??0,`${name}: direct versus canonical helper admission`);
    row.complete=true;checkpoint();
  } finally {if(active)api.core_end();if(opened)api.close();}
}

try {
  const one=ext(),tiny=ext(ONE,-53),next=ext(ONE+(1n<<11n)),singleNext=ext(ONE+(1n<<40n));
  runCase('pc53-add-keeps-the-25th-bit',{a:one,b:ext(ONE,-24)});
  runCase('pc53-subtract-keeps-the-25th-bit',{code:'d8e1',opcode:0xe1,kind:'sub',a:one,b:ext(ONE,-24)});
  runCase('pc53-multiply-retains-exact-48bit-product',{code:'d8c9',opcode:0xc9,kind:'mul',a:singleNext,b:singleNext});
  runCase('pc53-add-even-midpoint',{a:one,b:tiny});
  runCase('pc53-add-odd-midpoint-increments-c1',{a:next,b:tiny});
  runCase('pc53-negative-midpoint-keeps-magnitude-c1',{a:ext(ONE+(1n<<11n),0,true),b:ext(ONE,-53,true)});
  runCase('pc53-multiply-reports-inexact-tail',{code:'d8c9',opcode:0xc9,kind:'mul',a:next,b:next});
  runCase('pc53-divide-third-reports-pe',{code:'d8f1',opcode:0xf1,kind:'div',a:one,b:ext(0xc000000000000000n,1)});
  runCase('pc53-divide-tenth-does-not-lose-rounded-residual',{code:'d8f1',opcode:0xf1,kind:'div',a:one,b:ext(0xa000000000000000n,3)});
  runCase('pc53-zero-add-is-direct',{a:ext(0n),b:one});
  runCase('pc53-negative-zero-product',{code:'d8c9',opcode:0xc9,kind:'mul',a:ext(ONE,0,true),b:ext(0n)});
  runCase('pc53-exact-cancellation-is-positive-zero',{a:one,b:ext(ONE,0,true)});
  runCase('pc53-pop-uses-prepop-physical-operands',{code:'dec1',opcode:0x6c1,pop:true,a:tiny,b:next});
  runCase('pc53-directed-up-remains-canonical',{control:0xa3f,a:one,b:tiny,helpers:1});
  runCase('pc53-full64-operand-remains-canonical',{a:ext(ONE+1n),b:one,helpers:1});
  runCase('pc53-tiny-extended-result-remains-canonical',{code:'d8c9',opcode:0xc9,kind:'mul',a:ext(ONE,-900),b:ext(ONE,-900),helpers:1});
  runCase('pc53-memory-crosspage-canonical-prefix',{code:'d803',opcode:3,base:0x3ffe,memoryValue:one,data:[[0x3ffe,unhex('0000803f')]],helpers:1});
  runCase('pc53-memory-late-fault-is-neutral',{code:'d803',base:0x3ffe,crossFault:true,data:[[0x3ffe,unhex('0000')]],result:false,budget:8,retired:0,stop:3,pc:0x1000,vector:0x40e,detail:0x4000,helpers:1});
  runCase('pending-unmasked-fault-precedes-missing-memory',{code:'d803',control:0x23e,initialStatus:1,base:0x6000,result:false,pending:true,budget:8,retired:0,stop:9,pc:0x1000,vector:16,detail:0x8081,helpers:1});
  runCase('pc53-store-fault-keeps-arithmetic-prefix-and-stack',{code:'d8c1 dd1d00600000',budget:8,retired:1,stop:3,pc:0x1002,vector:0x80e,detail:0x6000,helpers:1});
  runCase('pc53-code-store-commits-before-exit-and-pop',{code:'d8c1 dd1d00110000',budget:8,retired:2,stop:6,pc:0x1008,storeCode:true,writes:[[0x1100,unhex('0000000000000040')]],helpers:1});
  runCase('pc53-reverse-subtract-keeps-operand-order',{code:'d8e9',opcode:0xe9,kind:'rsub',a:next,b:one});
  runCase('pc53-reverse-divide-keeps-residual-direction',{code:'d8f9',opcode:0xf9,kind:'rdiv',a:ext(0xa000000000000000n,3),b:one});
  runCase('pc53-arithmetic-source-destination-alias',{code:'d8c0',opcode:0xc0,a:next,b:one,alias:true});
  const fractions=[0x123456789abcdn,0x0fedcba987654n,0x055aa33cc77een];
  for(let index=0;index<fractions.length;index++)for(const negative of[false,true]){
    const a=ext(ONE+(fractions[index]<<11n),index-1,negative);
    const b=ext(ONE+((fractions[(index+1)%fractions.length]^0x5a5a5a5a5a5an)<<11n),1-index,(index&1)!==0);
    runCase(`pc53-deterministic-product-${index}-${negative?'negative':'positive'}`,{code:'d8c9',opcode:0xc9,kind:'mul',a,b});
  }
  for(const[exponentA,exponentB,helpers]of[[450,450,0],[-450,-450,0],[450,451,1],[-450,-451,1]]){
    runCase(`pc53-residual-range-${exponentA}-${exponentB}`,{code:'d8c9',opcode:0xc9,kind:'mul',
      a:ext(ONE+(0x12345n<<11n),exponentA),b:ext(ONE+(0x23456n<<11n),exponentB),helpers});
  }
  runCase('pc24-wide-positive-operands-remain-canonical',{control:0x3f,a:ext(ONE,200),b:ext(ONE,200),helpers:1});
  runCase('pc24-wide-negative-exponents-remain-canonical',{control:0x3f,a:ext(ONE,-200),b:ext(ONE,-200),helpers:1});
  runCase('pc24-wide-operands-with-narrow-product-remain-canonical',{control:0x3f,code:'d8c9',opcode:0xc9,kind:'mul',a:ext(ONE,200),b:ext(ONE,-200),helpers:1});
  for(const[kind,code,opcode]of[['mul','d80b',0x0b],['div','d833',0x33],['rdiv','d83b',0x3b]]){
    runCase(`pc53-${kind}-normal-float32-memory`,{code,opcode,kind,a:next,memoryValue:ext(0xc000000000000000n,1),data:[[0x3000,unhex('00004040')]]});
  }
  report.complete=true;checkpoint();console.log(JSON.stringify({complete:true,cases:report.cases.length,engineSha256:engine.sha256}));
}catch(error){report.failure={stage,message:String(error),stack:error.stack};checkpoint();throw error;}
finally{closeSync(output);}
