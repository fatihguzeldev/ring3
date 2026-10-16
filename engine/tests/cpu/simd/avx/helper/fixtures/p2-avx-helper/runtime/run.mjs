import {pageFaultPacket} from '../../../../../../../support/wasm/page-fault.mjs';
import {ARENA_SIZE} from '../../../../../../../support/wasm/arena.mjs';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../../../../../../../support/wasm/engine.mjs';
import {assertEngineExports} from '../../../../../../../support/wasm/exports.mjs';

const [enginePath, output, root] = process.argv.slice(2);
const engine = readEngine(enginePath);
const first = new WebAssembly.Instance(engine.module, {});
assert.equal(typeof first.exports.ring3_abi_v1_avx_execute, 'function', 'explicit versioned AVX helper exists');
assert.equal(first.exports.ring3_abi_v1_avx_execute.length, 6);
assert.equal(first.exports.ring3_abi_v1_avx_execute_resident.length, 10);
const receipt = assertEngineExports(engine.module, root);
const initial = readFileSync(join(output, 'initial-arena.bin'));
const SIZE=ARENA_SIZE, T=140, H=100, SV=4364, SG=4524, YU=4612, XP=SV+32, UP=YU+16;
assert.equal(initial.length,SIZE);
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const sourcePaths = [...new Set(['engine/tests/support/wasm/arena.mjs', 'engine/tests/support/wasm/page-fault.mjs', ...receipt.sourcePaths,
  'engine/tests/support/wasm/engine.mjs',
  'engine/tests/cpu/simd/avx/helper/native.rs', 'engine/tests/cpu/simd/avx/helper/wasm.rs',
  'engine/tests/cpu/simd/avx/helper/fixtures/p2-avx-helper/runtime/run.mjs',
  'engine/src/abi/x86/avx.rs','engine/src/cpu/x86/avx/operation.rs','engine/src/cpu/x86/avx/dispatch.rs',
  'engine/src/cpu/x86/avx/masked.rs','engine/src/process/cpu/avx.rs','engine/src/process/wasm/avx.rs',
  'engine/src/process/wasm/mod.rs',
  'engine/src/abi/cpu_helper.rs','engine/src/abi/header.rs','engine/src/memory/space.rs', 'engine/src/memory/space/atomic.rs',
  'engine/src/process/execution/authority.rs','engine/src/process/memory/span.rs',
  'engine/src/abi/x86/simd.rs','engine/src/abi/x86/ymm.rs','engine/src/abi/x86/segments.rs',
])];
const sourcePins=()=>Object.fromEntries(sourcePaths.map(path=>[path,hash(readFileSync(join(root,path)))]));
const sources=sourcePins();
const rows=[],frames=[],ramFrames=[];let contexts=0;
function record(magic,version,size,fields=[]) {const b=Buffer.alloc(size);b.write(magic);b.writeUInt16LE(version,4);b.writeUInt16LE(1,6);b.writeUInt32LE(size,8);fields.forEach((v,i)=>b.writeUInt32LE(v>>>0,16+i*4));return b;}
function packet(length=0,status=0,detail=0,address=0,access=0) {return record('R3MH',7,40,[status,0,detail,address,access,length]);}
const words=values=>{const b=Buffer.alloc(values.length*4);values.forEach((v,i)=>b.writeUInt32LE(v>>>0,i*4));return b;};
const d=(family,kind)=>0x01000000 | family<<8 | kind;
function arena(c) {return Buffer.from(new Uint8Array(c.memory.buffer,c.base,SIZE));}
function input(c,offset,bytes) {new Uint8Array(c.memory.buffer,c.base+offset,bytes.length).set(bytes);}
function upload(c,address,bytes) {input(c,T,bytes);assert.equal(c.api.upload(address,bytes.length),0);}
function vector(bytes,index) {return Buffer.concat([bytes.subarray(XP+index*16,XP+(index+1)*16),bytes.subarray(UP+index*16,UP+(index+1)*16)]);}
function publish(bytes,index,value) {bytes.set(value.subarray(0,16),XP+index*16);bytes.set(value.subarray(16,32),UP+index*16);}
function metadata(c,segment,offset) {input(c,T+544,record('R3AM',1,24,[segment,offset]));}
function success(c,length,amend=()=>{},gpr=0,flags=0x244fd7) {const b=arena(c);b.set(packet(length),H);b.fill(0,T+512,T+544);b.set(words([gpr,flags]),T+512);amend(b);return b;}
function fault(c,length,status,detail=0,address=0,access=0) {const b=arena(c);b.set(status===1&&detail<=2?pageFaultPacket({errorCode:4|(access===2?2:0)|(detail===2?1:0),address,access,length}):packet(length,status,detail,address,access),H);return b;}
function call(c,descriptor,operands,immediate=0,live0=0,live1=0,flags=0x244fd7) {return c.resident ? c.api.avx_execute_resident(c.low,c.high,c.id,0,descriptor,operands,immediate,live0,live1,flags) : c.api.avx_execute(descriptor,operands,immediate,live0,live1,flags);}
function ram(c) {
  const saved=arena(c),result=Buffer.alloc(c.pages.size*4096);let cursor=0;
  for(const address of c.pages) for(let offset=0;offset<4096;offset+=512) {
    assert.equal(c.api.cpu_read(address+offset,512,1),0);assert.equal(arena(c).readUInt32LE(H+16),0);
    arena(c).copy(result,cursor,T,T+512);cursor+=512;
  }
  input(c,0,saved);return result;
}
function checked(c,label,expected,action,status=expected.readUInt16LE(H+4)===15&&expected.readUInt32LE(H+16)===9?23:0,expectedRam) {
  const before=arena(c),beforeRam=ram(c),actual=action();assert.equal(actual,status,label);const after=arena(c);
  assert.deepEqual(after,expected,`${label}: complete arena frame`);
  const afterRam=ram(c);assert.deepEqual(afterRam,expectedRam??beforeRam,`${label}: all mapped RAM`);
  rows.push({context:c.number,profile:c.resident?'resident':'standalone',label,status:actual,before:frames.length,after:frames.length+1,ramBefore:ramFrames.length,ramAfter:ramFrames.length+1});frames.push(before,after);ramFrames.push(beforeRam,afterRam);
}
function seedVectors(c) {
  for(let r=0;r<8;r++) {input(c,XP+r*16,Buffer.from(Array.from({length:16},(_,i)=>(r*29+i*13)&255)));input(c,UP+r*16,Buffer.from(Array.from({length:16},(_,i)=>(r*17+i*31+3)&255)));}
}
function fresh(resident) {
  const instance=new WebAssembly.Instance(engine.module,{}),api=Object.fromEntries(receipt.functions.map(({name,arity})=>{assert.equal(instance.exports[name].length,arity,name);return [name.slice(13),instance.exports[name]];}));
  const c={api,memory:instance.exports.memory,resident,low:++contexts,high:0x41565831,number:contexts,pages:new Set([0x1000,0x5000,0x6000,0x8000])};
  assert.equal(api.open(4,c.low,c.high),0);c.base=api.arena_ptr()>>>0;assert.equal(api.arena_len(),SIZE);assert.deepEqual(arena(c),initial);
  for(const address of c.pages) assert.equal(api.map(address,1,7),0);
  upload(c,0x1000,Buffer.from('ebfe','hex'));input(c,T,words([0x1000,2]));assert.equal(api.compile(1),0);
  input(c,T,words([0x1000,2]));assert.equal(api.compile_resident(1),0);c.id=arena(c).readUInt32LE(T+8);assert.equal(arena(c).readUInt32LE(T+12),0);
  for(const address of [0x5000,0x6000,0x8000]) for(let start=0;start<4096;start+=256) upload(c,address+start,Buffer.from(Array.from({length:256},(_,i)=>(address/4096*19+start+i*37)&255)));
  for(let r=0;r<8;r++) {input(c,XP+r*16,Buffer.from(Array.from({length:16},(_,i)=>(r*29+i*13)&255)));input(c,UP+r*16,Buffer.from(Array.from({length:16},(_,i)=>(r*17+i*31+3)&255)));}
  input(c,T+512,Buffer.alloc(32,0xa5));return c;
}
for(const resident of [false,true]) {
  const c=fresh(resident);
  for(const width of [16,32]) for(const dest of [0,1,2,3]) for(const source of [0,1,2,3]) {
    seedVectors(c);input(c,T+544,Buffer.alloc(24,0xff));const before=arena(c),value=Buffer.alloc(32);vector(before,source).copy(value,0,0,width);
    checked(c,`register-copy-${width}-${dest}-${source}`,success(c,0,b=>publish(b,dest,value)),()=>call(c,d(3,width===16?1:2),dest | source<<6));
  }
  for(const width of [16,32]) for(const dest of [0,1,2]) {
    seedVectors(c);const a=vector(arena(c),1),b=vector(arena(c),2),value=Buffer.alloc(32);for(let i=0;i<width;i++)value[i]=a[i]^b[i];
    checked(c,`logical-XOR-alias-${width}-${dest}`,success(c,0,bytes=>publish(bytes,dest,value)),()=>call(c,d(2,width===16?7:8),dest | 1<<3 | 2<<6));
  }
  const live=0x89abcdef,value=Buffer.alloc(32);value.writeUInt32LE(live);
  checked(c,'live-EBX-source',success(c,0,b=>publish(b,1,value)),()=>call(c,d(3,33),1 | 3<<6,0,live));
  const maskSource=vector(arena(c),2);let sign=0;for(let i=0;i<8;i++)sign|=(maskSource[i*4+3]>>>7)<<i;
  checked(c,'GPR-mask-response',success(c,0,()=>{},sign),()=>call(c,d(3,30),3 | 2<<6));
  input(c,XP+16,Buffer.alloc(16));input(c,XP+32,Buffer.alloc(16));input(c,XP+16,Buffer.from([0x61]));input(c,XP+32,Buffer.from([0x62]));
  checked(c,'explicit-string-live-EAX-EDX-index',success(c,0,()=>{},16,(0x244fd7 & ~0x8d5) | 0xc0),()=>call(c,d(17,2),1<<3 | 2<<6,0,1,1));
  input(c,XP+16,words([0x3f800000]));input(c,XP+32,words([0x40000000]));
  checked(c,'quiet-scalar-compare-flags',success(c,0,()=>{},0,(0x244fd7 & ~0x8d5) | 1),()=>call(c,d(10,1),1<<3 | 2<<6));
  input(c,XP+16,Buffer.alloc(16));input(c,XP+32,Buffer.alloc(16));
  checked(c,'packed-test-flags',success(c,0,()=>{},0,(0x244fd7 & ~0x8d5) | 0x41),()=>call(c,d(15,1),1<<3 | 2<<6));
  for(const [descriptor,operands,immediate,live0,live1,flags] of [[d(3,2)|0x10000,0,0,0,0,2],[d(3,2),1<<15,0,0,0,2],[d(3,2),0,256,0,0,2],[d(3,2),0,0,1,0,2],[d(3,2),0,0,0,0,0]]) checked(c,'strict-descriptor/live/flags-neutral',arena(c),()=>call(c,descriptor,operands,immediate,live0,live1,flags),7);
  checked(c,'wrong-resident-authority-before-descriptor',arena(c),()=>c.api.avx_execute_resident(c.low+1,c.high,c.id,0,0,0,0,0,0,2),3);
  metadata(c,3,0x5000);input(c,T+544+12,words([1]));checked(c,'malformed-R3AM-neutral',arena(c),()=>call(c,d(3,2),1<<12),7);
  metadata(c,3,0x5000);input(c,YU+12,words([1]));checked(c,'malformed-upper-neutral',arena(c),()=>call(c,d(3,2),1<<12),9);input(c,YU+12,words([0]));
  for(const width of [16,32]) {
    metadata(c,3,0x5ff0);const memory=ram(c),value=Buffer.alloc(32);memory.copy(value,0,4096+4080,4096+4080+width);
    checked(c,`full-read-width${width}`,success(c,width,b=>publish(b,1,value)),()=>call(c,d(3,width===16?1:2),1 | 1<<12));
    metadata(c,3,0x5ff0);const source=vector(arena(c),2),expectedRam=ram(c);source.copy(expectedRam,4096+4080,0,width);
    checked(c,`full-store-width${width}`,success(c,width),()=>call(c,d(3,width===16?7:8),2<<6 | 1<<13),0,expectedRam);
  }
  for(const segment of [3,2]) {metadata(c,segment,0xfffffff0);checked(c,`logical-span-overflow-${segment}`,fault(c,32,segment===2?7:3),()=>call(c,d(3,2),1<<12));}
  metadata(c,3,0x5001);checked(c,'YMM-linear32-alignment',fault(c,32,3),()=>call(c,d(3,24),1<<12));
  upload(c,0x5000,words([0x10000]));metadata(c,3,0x5000);checked(c,'invalid-MXCSR-load',fault(c,4,3),()=>call(c,d(3,38),1<<12));
  input(c,SV+16,words([0x1f00]));input(c,XP+2*16,words([0x7fc00001]));
  const xm=fault(c,0,5,0x1f01);xm.set(words([0x1f01]),SV+16);checked(c,'unmasked-conversion-sticky-only',xm,()=>call(c,d(11,3),3 | 2<<6));input(c,SV+16,words([0x1f80]));
  assert.equal(c.api.unmap(0x6000,1),0);c.pages.delete(0x6000);
  metadata(c,3,0x5ff0);checked(c,'late-read-page-fault',fault(c,32,1,1,0x6000,1),()=>call(c,d(3,2),1 | 1<<12));
  metadata(c,3,0x5ff0);checked(c,'late-store-page-fault-atomic',fault(c,32,1,1,0x6000,2),()=>call(c,d(3,8),2<<6 | 1<<13));
  for(const width of [16,32]) {
    input(c,XP+3*16,Buffer.alloc(16));input(c,UP+3*16,Buffer.alloc(16));metadata(c,3,0xffffffff);input(c,SG+22,Buffer.from([0,0]));
    checked(c,`zero-mask-load-null-overflow-${width}`,success(c,width,b=>publish(b,1,Buffer.alloc(32))),()=>call(c,d(13,width===16?2:3),1 | 3<<9 | 1<<12));
    checked(c,`zero-mask-store-null-overflow-${width}`,success(c,width),()=>call(c,d(13,width===16?6:7),2<<6 | 3<<9 | 1<<13));
    input(c,SG,initial.subarray(SG,SG+64));const lane=Buffer.alloc(16);lane[3]=0x80;input(c,XP+3*16,lane);metadata(c,3,0x5ffc);
    const value=Buffer.alloc(32),memory=ram(c);memory.copy(value,0,4096+4092,4096+4096);
    checked(c,`masked-read-first-lane-${width}`,success(c,width,b=>publish(b,1,value)),()=>call(c,d(13,width===16?2:3),1 | 3<<9 | 1<<12));
    const expectedRam=ram(c),source=vector(arena(c),2);source.copy(expectedRam,4096+4092,0,4);
    checked(c,`masked-store-one-transaction-${width}`,success(c,width),()=>call(c,d(13,width===16?6:7),2<<6 | 3<<9 | 1<<13),0,expectedRam);
    lane[7]=0x80;input(c,XP+3*16,lane);
    checked(c,`masked-late-read-atomic-${width}`,fault(c,width,1,1,0x6000,1),()=>call(c,d(13,width===16?2:3),1 | 3<<9 | 1<<12));
    checked(c,`masked-late-store-atomic-${width}`,fault(c,width,1,1,0x6000,2),()=>call(c,d(13,width===16?6:7),2<<6 | 3<<9 | 1<<13));
  }
  const codeRam=ram(c),code=vector(arena(c),2);code.copy(codeRam,0);metadata(c,3,0x1000);
  checked(c,'SMC-success-before-stale-authority',success(c,32),()=>call(c,d(3,8),2<<6 | 1<<13),11,codeRam);
  checked(c,'stale-authority-before-invalid-descriptor',arena(c),()=>call(c,0,0),4);
  assert.equal(c.api.close(),0);const before=arena(c);assert.equal(call(c,0,0),5);assert.deepEqual(arena(c),before);
}
assert.deepEqual(sourcePins(),sources,'source pins remain current');
writeFileSync(join(output,'frames.bin'),Buffer.concat(frames));writeFileSync(join(output,'ram.bin'),Buffer.concat(ramFrames));
writeFileSync(join(output,'result.json'),JSON.stringify({engine:{path:enginePath,sha256:engine.sha256},contexts,runs:rows.length,frames:frames.length,ramFrames:ramFrames.length,sources,rows},null,2)+'\n');
console.log(JSON.stringify({contexts,runs:rows.length,fullArenaPairs:frames.length/2,fullRamPairs:ramFrames.length/2}));
