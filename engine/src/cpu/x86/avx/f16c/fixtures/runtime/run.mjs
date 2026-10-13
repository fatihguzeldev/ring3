import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';

const [enginePath, outputPath, rootPath] = process.argv.slice(2);
assert(enginePath && outputPath && rootPath, 'engine, output and source root are required');
const bytes = fs.readFileSync(enginePath);
const module = new WebAssembly.Module(bytes);
const engine = new WebAssembly.Instance(module, {});
const {ring3_avx_probe_write: write, ring3_avx_probe_read: read, ring3_avx_probe_run: run} = engine.exports;
for (const [name, fn, arity] of [['write',write,2],['read',read,1],['run',run,4]]) {
  assert.equal(typeof fn,'function',name);
  assert.equal(fn.length,arity,name);
}
let runs=0;
const groups={};
function state(csr=0x1f80) {
  const words=Uint32Array.from({length:96},(_,i)=>(0xa5a50000 ^ Math.imul(i,0x01010101))>>>0);
  words[8]=0x12345678; words[9]=0x244cd7; words[10]=csr;
  return words;
}
function setVector(words,register,bytes) {
  for (let byte=0;byte<bytes.length;byte++) {
    const half=byte<16?12:44;
    const index=half+register*4+Math.floor((byte%16)/4);
    const shift=(byte%4)*8;
    words[index]=((words[index]&~(255<<shift)) | (bytes[byte]<<shift))>>>0;
  }
}
function publish(words,register,bytes) {
  const full=new Uint8Array(32); full.set(bytes); setVector(words,register,full);
}
function setOperand(words,bytes) {
  for (let byte=0;byte<bytes.length;byte++) {
    const index=76+Math.floor(byte/4),shift=(byte%4)*8;
    words[index]=((words[index]&~(255<<shift)) | (bytes[byte]<<shift))>>>0;
  }
}
function halves(values) {
  return Uint8Array.from(values.flatMap(value=>[value&255,value>>>8]));
}
function singles(values) {
  return Uint8Array.from(values.flatMap(value=>[value&255,(value>>>8)&255,(value>>>16)&255,value>>>24]));
}
// exact decomposition is an independent widening oracle, not an execution helper.
function widen(bits) {
  const sign=(bits&0x8000)<<16,exponent=(bits>>>10)&31,fraction=bits&1023;
  if (exponent===31) return (sign|0x7f800000|(fraction<<13)|(fraction?0x00400000:0))>>>0;
  if (exponent) return (sign|((exponent+112)<<23)|(fraction<<13))>>>0;
  if (!fraction) return sign>>>0;
  const top=31-Math.clz32(fraction);
  return (sign|((top+103)<<23)|((fraction-(1<<top))<<(23-top)))>>>0;
}
function check(group,label,operation,registers,immediate,operandLength,before,expected,status=0) {
  for(let index=0;index<96;index++) assert.equal(write(index,before[index]),0,`${label}: write${index}`);
  assert.equal(run(operation,registers,immediate,operandLength),status,`${label}: status`);
  const actual=Uint32Array.from({length:96},(_,index)=>read(index)>>>0);
  assert.deepEqual(actual,expected,`${label}: complete384-byte proof staging arena`);
  runs++; groups[group]=(groups[group]??0)+1;
}
function widening(group,label,width,input,d,a,csr=0x1f80,memory=false,expectedStatus=0,operandLength=width/2) {
  const before=state(csr),data=halves(input),expected=before.slice();
  if(memory) setOperand(before,data); else setVector(before,a,data);
  expected.set(before);
  if(expectedStatus===0 || expectedStatus===3) {
    const flags=input.some(value=>(value&0x7c00)===0x7c00&&(value&0x3ff)!==0&&(value&0x200)===0)?1:0;
    expected[10]=csr|flags;
    if(expectedStatus===0) publish(expected,d,singles(input.map(widen)));
  }
  const registers=(d|(a<<8)|(memory?0x1000000:0))>>>0;
  check(group,label,width===16?34:35,registers,0,memory?operandLength:0,before,expected,expectedStatus);
}
function narrowing(group,label,width,input,output,d,a,csr=0x1f80,after=csr,immediate=0,memory=false,status=0,operandLength=width/2) {
  const before=state(csr);setVector(before,a,singles(input));
  const expected=before.slice();expected[10]=after;
  if(status===0) {
    if(memory) setOperand(expected,halves(output)); else publish(expected,d,halves(output));
  }
  const registers=(d|(a<<8)|(memory?0x1000000:0))>>>0;
  check(group,label,width===16?36:37,registers,immediate,memory?operandLength:0,before,expected,status);
}

for(let base=0;base<65536;base+=8) {
  const input=Array.from({length:8},(_,i)=>base+i),d=(base>>>3)&7,a=(base>>>6)&7;
  const csr=0x1f80|((base&8)?0x40:0)|((base&16)?0x8000:0)|((base&96)<<8);
  widening('all-half-widen',`half${base}`,32,input,d,a,csr);
  const quiet=input.map(value=>(value&0x7c00)===0x7c00&&(value&0x3ff)!==0?value|0x200:value);
  narrowing('all-half-roundtrip',`roundtrip${base}`,32,input.map(widen),quiet,d,a,csr,csr);
}
const literalHalf=[0x3c00,0xc000,1,0x0400,0x7bff,0x8000,0x7c00,0xfc00];
const literalSingle=[0x3f800000,0xc0000000,0x33800000,0x38800000,0x477fe000,0x80000000,0x7f800000,0xff800000];
for(const width of [16,32]) for(let d=0;d<8;d++) for(let a=0;a<8;a++) {
  const count=width/4;
  widening('register-aliases',`widen${width}/${d}/${a}`,width,literalHalf.slice(0,count),d,a);
  narrowing('register-aliases',`narrow${width}/${d}/${a}`,width,literalSingle.slice(0,count),literalHalf.slice(0,count),d,a);
}
for(let immediate=0;immediate<256;immediate++) for(let mode=0;mode<4;mode++) {
  const control=(immediate&4)?mode:immediate&3;
  const output=[[0x3c00,0xbc00],[0x3c00,0xbc01],[0x3c01,0xbc00],[0x3c00,0xbc00]][control];
  narrowing('all-rounding-immediates',`imm${immediate}/${mode}`,16,[0x3f801000,0xbf801000,0,0],[...output,0,0],0,1,0x1f80|(mode<<13),(0x1f80|(mode<<13))|0x20,immediate);
}
const cases=[
  ['zero',0,0,0x1f80,0x1f80,0,0],
  ['exact-tiny',0x33800000,1,0x1f80,0x1f80,0,0],
  ['exact-tiny-ftz',0x33800000,1,0x9f80,0x9f80,0,0],
  ['negative-exact-tiny',0xb3800000,0x8001,0x1f80,0x1f80,0,0],
  ['tiny-midpoint',0x33000000,0,0x1f80,0x1fb0,0,0],
  ['tiny-above-exact',0x33800001,1,0x1f80,0x1fb0,0,0],
  ['overflow-nearest',0x47800000,0x7c00,0x1f80,0x1fa8,0,0],
  ['overflow-zero',0x47800000,0x7bff,0x1f80,0x1fa8,3,0],
  ['overflow-mxcsr-zero',0x47800000,0x7bff,0x7f80,0x7fa8,4,0],
  ['exact-tiny-UM0',0x33800000,1,0x1780,0x1790,0,3],
  ['inexact-tiny-UM0',0x33000000,0,0x1780,0x1790,0,3],
  ['inexact-tiny-PM0',0x33000000,0,0x0f80,0x0fb0,0,3],
  ['source-denormal-UM0',1,0,0x1780,0x17b2,0,3],
  ['source-denormal-DM0',1,0,0x1e80,0x1e82,0,3],
  ['overflow-OM0',0x47800000,0x7c00,0x1b80,0x1b88,0,3],
  ['snan-IM0',0x7f800001,0x7e00,0x1f00,0x1f01,0,3],
  ['snan-masked',0x7f800001,0x7e00,0x1f80,0x1f81,0,0],
  ['qnan-payload',0xffcaa000,0xfe55,0x1f80,0x1f80,0,0],
  ['denormal-masked',1,0,0x1f80,0x1fb2,0,0],
  ['denormal-daz',0x80000001,0x8000,0x1fc0,0x1fc0,0,0],
];
for(const width of [16,32]) for(const [label,bits,half,csr,after,imm,status] of cases) {
  const input=[bits,...Array(width/4-1).fill(0)],output=[half,...Array(width/4-1).fill(0)];
  for(const [d,a,memory] of [[0,1,false],[1,1,false],[0,1,true]]) {
    narrowing('literal-exceptions',`${label}/${width}/${d}/${memory}`,width,input,output,d,a,csr,after,imm,memory,status);
  }
}
for(const width of [16,32]) {
  const count=width/4;
  for(const memory of [false,true]) widening('literal-exceptions',`widen-snan-IM0/${width}/${memory}`,width,[0x7c01,...Array(count-1).fill(1)],0,1,0x1f00,memory,3);
  widening('memory','exact-widen'+width,width,literalHalf.slice(0,count),0,0,0x1f80,true);
  narrowing('memory','exact-narrow'+width,width,literalSingle.slice(0,count),literalHalf.slice(0,count),0,1,0x1f80,0x1f80,0,true);
  for(const length of [0,width/2-1,width/2+1,32]) {
    widening('neutral-refusals',`widenlen${width}/${length}`,width,literalHalf.slice(0,count),0,0,0x1f80,true,1,length);
    narrowing('neutral-refusals',`narrowlen${width}/${length}`,width,literalSingle.slice(0,count),[],0,1,0x1f80,0x1f80,0,true,1,length);
  }
  widening('neutral-refusals','invalidmxcsr-widen'+width,width,literalHalf.slice(0,count),0,1,0x11f80,false,2);
  narrowing('neutral-refusals','invalidmxcsr-narrow'+width,width,literalSingle.slice(0,count),[],0,1,0x11f80,0x11f80,0,false,2);
}
for(const [label,input,csr,after] of [
  ['packed-UE-plus-PE',[0x33000000,0x3f801000,0,0],0x1780,0x17b0],
  ['packed-UE-plus-OE',[0x33000000,0x47800000,0,0],0x1780,0x17b8],
  ['packed-IE-before-post',[0x7f800001,1,0x33000000,0x47800000],0x1f00,0x1f03],
  ['packed-DE-before-post',[0x7f800001,1,0x33000000,0x47800000],0x1e80,0x1e83],
]) narrowing('packed-exception-priority',label,16,input,[],0,1,csr,after,0,false,3);
const sourcePaths=[
  'engine/src/math/ieee.rs','engine/src/math/ieee/half.rs','engine/src/math/ieee/half/tests.rs',
  'engine/src/cpu/x86/avx/mod.rs','engine/src/cpu/x86/avx/f16c.rs','engine/src/cpu/x86/avx/f16c/tests.rs',
  'engine/src/abi/wasm/avx_proof.rs',
];
const sources=Object.fromEntries(sourcePaths.map(relative=>[relative,crypto.createHash('sha256').update(fs.readFileSync(path.join(rootPath,relative))).digest('hex')]));
const result={runs,groups,engine_sha256:crypto.createHash('sha256').update(bytes).digest('hex'),sources,
  scope:'actual Rust-to-Wasm private CPU numeric/state kernel; direct backend export, no JS math execution helper; not public decode/DBT/page/state-ABI support',
  oracle:'independent integer half widening, exhaustive finite/quiet-NaN roundtrip, literal rounding/exception vectors; measuredIntel606a6 exceptions retain single-model ceiling',
  arena_bytes:384};
fs.mkdirSync(outputPath,{recursive:true});fs.writeFileSync(path.join(outputPath,'result.json'),JSON.stringify(result,null,2)+'\n');
console.log(JSON.stringify(result));
