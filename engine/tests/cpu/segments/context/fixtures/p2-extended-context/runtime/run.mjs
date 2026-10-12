import assert from 'node:assert/strict';
import {readFileSync,writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {createHash} from 'node:crypto';
import {readEngine} from '../../../../../../support/wasm/engine.mjs';

const [enginePath,output,root]=process.argv.slice(2),engine=readEngine(enginePath);
const initial=readFileSync(join(output,'initial-arena.bin')),SIZE=initial.length,FP=4236,SIMD=4364,SEG=4524,END=4588;
const hash=b=>createHash('sha256').update(b).digest('hex'),observations=[],modules=[],refusals=[];
const paths=['engine/src/process/calls/call.rs', 'engine/src/process/calls/context.rs', 'engine/src/process/calls/mod.rs', 'engine/src/process/mod.rs','engine/src/process/calls/callback.rs', 'engine/src/process/calls/code.rs', 'engine/src/process/calls/installation.rs', 'engine/src/process/calls/resident_callback.rs', 'engine/src/process/loading/startup.rs', 'engine/src/process/loading/mod.rs', 'engine/src/abi/x86/segments.rs','engine/src/cpu/x86/segments/cache.rs','engine/src/abi/x86/simd.rs','engine/tests/cpu/segments/context/wasm/roundtrip.rs', 'engine/tests/cpu/segments/wasm.rs', 'engine/tests/support/wasm.rs', 'engine/tests/cpu/segments/context/fixtures/p2-extended-context/runtime/run.mjs'];
const pins=()=>Object.fromEntries(paths.map(p=>[p,hash(readFileSync(join(root,p)))])),beforePins=pins();
assert.ok(SIZE>=4612);assert.equal(engine.sha256,hash(engine.bytes));
function header(b,magic,size){b.write(magic);b.writeUInt16LE(1,4);b.writeUInt16LE(1,6);b.writeUInt32LE(size,8);}
function state(s){const b=Buffer.alloc(56);header(b,'R3ST',56);[...s.r,s.pc,s.flags].forEach((v,i)=>b.writeUInt32LE(v>>>0,16+4*i));return b;}
const arena=c=>Buffer.from(new Uint8Array(c.memory.buffer,c.base,SIZE));
function input(c,offset,b){new Uint8Array(c.memory.buffer).set(b,c.base+offset);}
function host(c,n,args=[]){assert.equal(c.api[n](...args),0,n);}
function upload(c,address,b){input(c,140,b);host(c,'upload',[address,b.length]);}
function save(c,label){const a=arena(c),file=`arena-${observations.length+1}.bin`;writeFileSync(join(output,file),a);observations.push({context:c.id,label,arena:{file,sha256:hash(a)},cpu:a.subarray(0,56).toString('hex'),exit:a.subarray(56,96).toString('hex'),extended_sha256:hash(a.subarray(FP,END))});return a;}
function refuses(c,name,args,status,label){const before=arena(c);assert.equal(c.api[name](...args),status,label);assert.deepEqual(arena(c),before,label+' full arena');refusals.push({context:c.id,label,status});}
for(const finish of [true,false]){
 const instance=new WebAssembly.Instance(engine.module,{}),c={id:finish?1:2,memory:instance.exports.memory};
 c.api=Object.fromEntries(Object.entries(instance.exports).filter(([name])=>name.startsWith('ring3_abi_v1_')).map(([name,fn])=>[name.slice(13),fn]));
 host(c,'open',[5,c.id,0x4358]);c.base=c.api.arena_ptr()>>>0;input(c,0,initial);
 for(const[p,perm]of[[0x1000,7],[0x5000,3],[0x6000,3],[0x8000,3]])host(c,'map',[p,1,perm]);
 const xmm=Buffer.from(Array.from({length:16},(_,i)=>i*11+3));upload(c,0x5010,xmm);upload(c,0x6010,Buffer.from([0xef,0xcd,0xab,0x89]));upload(c,0x8040,Buffer.from([0,0x20,0,0]));
 const body=Buffer.from([0xf3,0x0f,0x6f,0x05,0x10,0x50,0,0,0xb8,0x43,0,0,0,0x8e,0xe0,0x64,0x8b,0x0d,0x10,0,0,0,0xb8,0x78,0x56,0x34,0x12,0xc3]);
 upload(c,0x1000,Buffer.from([0x0f,0x0b]));upload(c,0x1100,body);upload(c,0x1200,Buffer.from([0x0f,0x0b]));host(c,'protect',[0x1000,1,5]);
 host(c,'set_segment_base',[4,0x5000]);host(c,'configure_segment_descriptor',[0x43,0x6000,3,1,1]);
 const extended=arena(c);extended.writeUInt16LE(2,SEG+4);
 for(const[offset,bits]of [[28,0xf33fffff],[32,0xf3bfffff],[36,0xf33fffff],[40,0xf33fffff],[44,0xf2101fff],[56,0]])extended.writeUInt32LE(bits>>>0,SEG+offset);
 extended.writeUInt32LE(0x5f80,SIMD+16);extended.fill(0xa5,SIMD+32,SIMD+160);extended.writeUInt16LE(0x027f,FP+16);input(c,FP,extended.subarray(FP,END));
 const d=Buffer.alloc(40);[0x1000,2,0x1100,body.length,0x1200,2,0x1000,17,0x1200,18].forEach((v,i)=>d.writeUInt32LE(v,4*i));input(c,140,d);host(c,'compile_with_gates',[3,2]);const generation=c.api.generation()>>>0;
 const wasm=Buffer.from(new Uint8Array(c.memory.buffer,c.api.module_ptr()>>>0,c.api.module_len()>>>0)),module=new WebAssembly.Module(wasm),file=`module-${c.id}.wasm`;
 writeFileSync(join(output,file),wasm);modules.push({context:c.id,file,sha256:hash(wasm),imports:WebAssembly.Module.imports(module)});
 const run=new WebAssembly.Instance(module,{env:{memory:c.memory},ring3:c.api}).exports.run;
 const outer={r:[0xaabbccdd,0x11223344,0x22334455,0x33445566,0x8040,0x44556677,0x55667788,0x66778899],pc:0x1000,flags:0xcd7};input(c,0,state(outer));
 assert.equal(run(c.base,c.base+56,1,c.base+96),0);assert.equal(arena(c).readUInt32LE(72),8);const stopped=save(c,'outer gate');
 const malformed=Buffer.from(stopped);malformed[SIMD+20]=1;input(c,0,malformed);
 refuses(c,'capture_call',[c.id,0x4358,generation,1,0],13,'invalid SIMD cannot capture');input(c,0,stopped);
 // Every additional architectural byte participates in pending-call identity, including valid changes.
 host(c,'capture_call',[c.id,0x4358,generation,1,0]);const token=arena(c).readUInt32LE(156);assert.equal(token,1);
 for(const offset of[SIMD+16,SIMD+47,SEG+44,SEG+48]){
  const before=arena(c),changed=Buffer.from(before);changed[offset]^=1;input(c,0,changed);
  refuses(c,'begin_callback',[c.id,0x4358,generation,token,0x1100,0x1200,18,0],15,'changed extended pending context');input(c,0,before);
 }
 host(c,'begin_callback',[c.id,0x4358,generation,token,0x1100,0x1200,18,0]);assert.equal(arena(c).readUInt32LE(156),2);
 assert.equal(run(c.base,c.base+56,7,c.base+96),0);const callback=save(c,'actual callback SIMD and FS changes');
 assert.equal(callback.readUInt32LE(72),8);assert.equal(callback.readUInt32LE(76),6);
 assert.deepEqual(callback.subarray(SIMD+32,SIMD+48),xmm);assert.deepEqual(callback.subarray(SIMD+48,SIMD+160),stopped.subarray(SIMD+48,SIMD+160));assert.deepEqual(callback.subarray(FP,FP+128),stopped.subarray(FP,FP+128));
 assert.equal(callback.readUInt16LE(SEG+24),0x43);assert.equal(callback.readUInt32LE(SEG+48),0x6000);
 const returned={...outer,r:[...outer.r],pc:0x1200};returned.r[0]=0x12345678;returned.r[1]=0x89abcdef;assert.deepEqual(callback.subarray(0,56),state(returned));
 if(finish){
  const invalid=Buffer.from(callback);invalid[SEG+28]=1;input(c,0,invalid);refuses(c,'finish_callback',[c.id,0x4358,generation,2],13,'invalid segment state cannot finish');input(c,0,callback);
  host(c,'finish_callback',[c.id,0x4358,generation,2]);
 }else{
  const corrupt=Buffer.from(callback);corrupt.fill(0xff,FP,END);input(c,0,corrupt);host(c,'abort_callback',[c.id,0x4358,2]);
 }
 const restored=save(c,finish?'finish restores outer extended context':'forced abort restores corrupted extended context');
 assert.deepEqual(restored.subarray(0,96),stopped.subarray(0,96));assert.deepEqual(restored.subarray(FP,END),stopped.subarray(FP,END));
 host(c,'complete_call',[c.id,0x4358,generation,1,0x76543210]);const completed=save(c,'outer completion retains restored extended state');
 assert.equal(completed.readUInt32LE(16),0x76543210);assert.equal(completed.readUInt32LE(32),0x8044);assert.equal(completed.readUInt32LE(48),0x2000);assert.deepEqual(completed.subarray(FP,END),stopped.subarray(FP,END));host(c,'close');
}
assert.deepEqual(pins(),beforePins);writeFileSync(join(output,'result.json'),JSON.stringify({engine:{path:enginePath,sha256:engine.sha256,bytes:engine.bytes.length},source_pins:beforePins,modules,observations,refusals,claim:'Actual replacement callback changes SIMD and FS state, then finish/forced abort restores exact validated outer x87/SIMD/64-byte v2 segment cache. Pending and invalid-record refusals preserve full arena. No resident guest callback or whole RAM claim.'},null,2)+'\n');
console.log(JSON.stringify({output,contexts:2,modules:modules.length,snapshots:observations.length,refusals:refusals.length}));
