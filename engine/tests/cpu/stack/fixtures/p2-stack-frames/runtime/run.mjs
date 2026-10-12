import assert from 'node:assert/strict';
import {readFileSync,writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {createHash} from 'node:crypto';
import {readEngine} from '../../../../../support/wasm/engine.mjs';
const [enginePath,output,root]=process.argv.slice(2),engine=readEngine(enginePath),initial=readFileSync(join(output,'initial-arena.bin')),SIZE=initial.length;
const hash=b=>createHash('sha256').update(b).digest('hex'),cases=[],modules=[],observations=[],helpers=[];
const sourcePaths=['engine/src/cpu/x86/stack.rs','engine/src/cpu/x86/decode/stack.rs','engine/src/cpu/x86/decode/flow.rs','engine/src/cpu/x86/decode/profile.rs',
 'engine/src/cpu/dbt/wasm/stack.rs','engine/src/cpu/dbt/wasm/integer/arithmetic.rs', 'engine/src/cpu/dbt/wasm/integer/bits.rs', 'engine/src/cpu/dbt/wasm/integer/carry_rotates.rs', 'engine/src/cpu/dbt/wasm/integer/decimal.rs', 'engine/src/cpu/dbt/wasm/integer/dispatch.rs', 'engine/src/cpu/dbt/wasm/integer/flags.rs', 'engine/src/cpu/dbt/wasm/integer/mod.rs', 'engine/src/cpu/dbt/wasm/integer/multiply_divide.rs', 'engine/src/cpu/dbt/wasm/integer/operands.rs', 'engine/src/cpu/dbt/wasm/integer/rotates.rs', 'engine/src/cpu/dbt/wasm/integer/shifts.rs','engine/src/cpu/dbt/wasm/memory.rs','engine/src/cpu/dbt/wasm/emitter.rs',
 'engine/src/process/cpu/stack.rs', 'engine/src/process/cpu/mod.rs', 'engine/src/process/mod.rs','engine/src/process/wasm/mod.rs', 'engine/src/process/wasm/loading.rs', 'engine/src/process/wasm/execution.rs', 'engine/src/process/wasm/calls.rs', 'engine/src/process/wasm/memory.rs', 'engine/src/process/wasm/cpu.rs', 'engine/src/process/mod.rs','engine/src/memory/space.rs','engine/src/abi/wasm/exports/calls.rs', 'engine/src/abi/wasm/exports/cpu.rs', 'engine/src/abi/wasm/exports/execution.rs', 'engine/src/abi/wasm/exports/image.rs', 'engine/src/abi/wasm/exports/instance.rs', 'engine/src/abi/wasm/exports/memory.rs', 'engine/src/abi/wasm/exports/mod.rs','engine/tests/cpu/stack/cases/native/cpu_stack_frames.rs', 'engine/tests/cpu/stack/native.rs', 'engine/tests/cpu/stack/native.rs',
 'engine/tests/cpu/stack/cases/wasm/cpu_stack_frames_wasm.rs', 'engine/tests/cpu/stack/wasm.rs', 'engine/tests/support/wasm.rs', 'engine/tests/cpu/stack/wasm.rs', 'engine/tests/support/wasm.rs','engine/tests/cpu/stack/fixtures/p2-stack-frames/runtime/run.mjs'];
const pins=()=>Object.fromEntries(sourcePaths.map(p=>[p,hash(readFileSync(join(root,p)))])),sourcePins=pins();
assert.ok(SIZE>=4612);assert.equal(hash(engine.bytes),engine.sha256);
const arena=c=>Buffer.from(new Uint8Array(c.memory.buffer,c.base,SIZE));
function input(c,at,b){new Uint8Array(c.memory.buffer).set(b,c.base+at);}
function host(c,name,args=[]){assert.equal(c.api[name](...args),0,name);}
function upload(c,address,b){input(c,140,b);host(c,'upload',[address,b.length]);}
function state(s){const b=Buffer.alloc(56);b.write('R3ST');b.writeUInt16LE(1,4);b.writeUInt16LE(1,6);b.writeUInt32LE(56,8);[...s.r,s.pc,s.flags].forEach((v,i)=>b.writeUInt32LE(v>>>0,16+i*4));return b;}
function seed(c,updates={},pc=0x1000){const s={r:[0x11112222,0x22223333,0x33334444,0x44445555,0x8080,0x8080,0x66667777,0x77778888],pc,flags:0xcd7};for(const[i,v]of Object.entries(updates))s.r[+i]=v>>>0;input(c,0,state(s));return s;}
function next(s,pc,updates={}){const r=[...s.r];for(const[i,v]of Object.entries(updates))r[+i]=v>>>0;return{r,pc,flags:s.flags};}
function capture(c,bytes,pc=0x1000){
 upload(c,pc,Buffer.from([...bytes,0xeb,0xfe]));
 const descriptor=Buffer.alloc(c.entries?4:8);descriptor.writeUInt32LE(pc);if(!c.entries)descriptor.writeUInt32LE(bytes.length+2,4);input(c,140,descriptor);
 host(c,c.owner==='resident'?(c.entries?'compile_resident_entries':'compile_resident'):(c.entries?'compile_entries':'compile'),c.entries?[1,0]:[1]);
 const a=arena(c),pointer=c.owner==='resident'?a.readUInt32LE(156):c.api.module_ptr()>>>0,length=c.owner==='resident'?a.readUInt32LE(160):c.api.module_len()>>>0;
 const bytesWasm=Buffer.from(new Uint8Array(c.memory.buffer,pointer,length)),module=new WebAssembly.Module(bytesWasm),ring3={...c.api};
 for(const imp of WebAssembly.Module.imports(module).filter(i=>i.module==='ring3'&&i.name.startsWith('stack_execute'))){
  ring3[imp.name]=(...args)=>{const before=arena(c),status=c.api[imp.name](...args),after=arena(c),allowed=Buffer.from(before);allowed.set(after.subarray(100,172),100);
   assert.deepEqual(after,allowed,'stack helper publishes only packet and32-byte staged result');helpers.push({context:c.id,name:imp.name,args:args.map(v=>v>>>0),status,packet:after.subarray(100,140).toString('hex')});return status;};
 }
 c.run=new WebAssembly.Instance(module,{env:{memory:c.memory},ring3}).exports.run;
 const file=`module-${modules.length+1}.wasm`;writeFileSync(join(output,file),bytesWasm);modules.push({context:c.id,owner:c.owner,entries:c.entries,file,sha256:hash(bytesWasm),imports:WebAssembly.Module.imports(module)});
}
function open(bytes,owner='replacement',entries=false){
 const instance=new WebAssembly.Instance(engine.module,{}),c={id:cases.length+1,owner,entries,memory:instance.exports.memory};
 c.api=Object.fromEntries(Object.entries(instance.exports).filter(([n])=>n.startsWith('ring3_abi_v1_')).map(([n,fn])=>[n.slice(13),fn]));
 host(c,'open',[8,c.id,0x5354]);c.base=c.api.arena_ptr()>>>0;input(c,0,initial);
 for(const[p,n,perm]of[[0x1000,1,7],[0x7000,2,3]])host(c,'map',[p,n,perm]);upload(c,0x7000,Buffer.alloc(4096,0xa5));upload(c,0x8000,Buffer.alloc(4096,0xa5));
 capture(c,bytes);cases.push({id:c.id,owner,entries,code:Buffer.from(bytes).toString('hex')});return c;
}
function packet(status,detail=0,address=0,access=0,length=0){const b=Buffer.alloc(40);b.write('R3MH');b.writeUInt16LE(7,4);b.writeUInt16LE(1,6);b.writeUInt32LE(40,8);[status,0,detail,address,access,length].forEach((v,i)=>b.writeUInt32LE(v>>>0,16+i*4));return b;}
function run(c,budget,expected,{retired=budget,reason=1,fault,expectedPacket=packet(0),staged=expected.r,label='stack'}={}){
 const before=arena(c),at=helpers.length,status=c.run(c.base,c.base+56,budget,c.base+96),after=arena(c);assert.equal(status,0,label);
 assert.deepEqual(after.subarray(0,56),state(expected),label+' exactCPU');assert.equal(after.readUInt32LE(72),reason,label+' reason');assert.equal(after.readUInt32LE(76),retired,label+' retirement');
 if(fault)assert.deepEqual(Array.from({length:4},(_,i)=>after.readUInt32LE(80+i*4)),fault,label+' fault');
 assert.deepEqual(after.subarray(100,140),expectedPacket,label+' strictV7packet');
 const full=Buffer.from(before);full.set(state(expected));full.set(after.subarray(56,96),56);full.set(expectedPacket,100);staged.forEach((v,i)=>full.writeUInt32LE(v>>>0,140+i*4));assert.deepEqual(after,full,label+' completearena');
 const file=`arena-${observations.length+1}.bin`;writeFileSync(join(output,file),after);observations.push({context:c.id,label,budget,reason,retired,cpu:expected,exit:after.subarray(56,96).toString('hex'),helpers:helpers.length-at,arena:{file,sha256:hash(after)}});return after;
}
function read(c,address,length){host(c,'cpu_read',[address,length,1]);return arena(c).subarray(140,140+length);}
function wordBytes(values,width){const b=Buffer.alloc(values.length*width);values.forEach((v,i)=>width===2?b.writeUInt16LE(v&65535,i*width):b.writeUInt32LE(v>>>0,i*width));return b;}
for(const owner of['replacement','resident'])for(const entries of[false,true])for(const width of[2,4]){
 const push=width===2?[0x66,0x60]:[0x60],pop=width===2?[0x66,0x61]:[0x61],c=open([...push,...pop],owner,entries),s=seed(c),saved=s.r[4]-width*8;
 run(c,1,next(s,0x1000+push.length,{4:saved}),{label:'PUSHALL captures originalSP'+width});
 assert.deepEqual(read(c,saved,width*8),wordBytes([s.r[7],s.r[6],s.r[5],s.r[4],s.r[3],s.r[2],s.r[1],s.r[0]],width));
 // The savedSP slot is ignored; poisoning it must not change the restored pointer.
 upload(c,saved+3*width,Buffer.alloc(width,0xff));run(c,1,next(s,0x1000+push.length+pop.length),{label:'POPALL skips poisonedSP'+width});host(c,'close');
}
for(const owner of['replacement','resident']){
 const c=open([0x66,0x60,0x66,0x61],owner);host(c,'map',[0,1,3]);host(c,'map',[0xfffff000,1,3]);const s=seed(c,{4:2});
 const pushed=next(s,0x1002,{4:0xfffffff2});run(c,1,pushed,{label:'PUSHA16 wraps SS32 across addresszero'});
 const all=wordBytes([s.r[7],s.r[6],s.r[5],s.r[4],s.r[3],s.r[2],s.r[1],s.r[0]],2);
 assert.deepEqual(read(c,0xfffffff2,14),all.subarray(0,14));assert.deepEqual(read(c,0,2),all.subarray(14));
 run(c,1,next(s,0x1004),{label:'POPA16 natural wrapped roundtrip'});host(c,'close');
}
for(const owner of['replacement','resident'])for(const width of[2,4])for(const nesting of[0,1,3,31,255]){
 const alloc=16,bytes=[...(width===2?[0x66]:[]),0xc8,alloc,0,nesting],c=open(bytes,owner),s=seed(c),level=nesting&31,count=level===0?1:level+1,frame=s.r[4]-width;
 const expected=next(s,0x1000+bytes.length,{4:s.r[4]-count*width-alloc,5:width===2?(s.r[5]&~65535)|(frame&65535):frame});
 run(c,1,expected,{label:'ENTER width'+width+' nested'+nesting});
 const values=[s.r[5],...Array(Math.max(level-1,0)).fill(s.r[5]),...(level?[frame]:[])];assert.deepEqual(read(c,s.r[4]-count*width,count*width),wordBytes(values.reverse(),width));
 assert.deepEqual(read(c,expected.r[4],alloc),Buffer.alloc(alloc,0xa5),'allocated locals remain unchanged');host(c,'close');
}
// Width16 updates BP only, while SS.B32 allocation borrows through ESP bit16.
{
 const c=open([0x66,0xc8,0x10,0,0]);host(c,'map',[0x10000,1,3]);const s=seed(c,{4:0x10008,5:0xabcd1000});
 run(c,1,s,{retired:0,reason:5,fault:[1,0xfff6,2,2],expectedPacket:packet(1,1,0xfff6,2,2),staged:s.r,label:'ENTER16 allocation crosses64K and probes finalESP'});
 host(c,'map',[0xf000,1,3]);run(c,1,next(s,0x1005,{4:0xfff6,5:0xabcd0006}),{label:'ENTER16 same-input map-only retry'});host(c,'close');
}
// A late frame read and allocation-write fault leave both CPU and earlier stack bytes untouched.
for(const owner of['replacement','resident']){
 const c=open([0xc8,16,0,3],owner),s=seed(c,{5:0x9004}),old=Buffer.from(read(c,0x8070,16));
 run(c,1,s,{retired:0,reason:5,fault:[1,0x9000,1,4],expectedPacket:packet(1,1,0x9000,1,4),staged:s.r,label:'ENTER late read is instruction-neutral'});assert.deepEqual(read(c,0x8070,16),old);
 host(c,'map',[0x9000,1,3]);host(c,'protect',[0x7000,1,1]);const repaired=next(s,0x1004,{4:0x8060,5:0x807c});
 run(c,1,repaired,{label:'ENTER map-only nested retry'});host(c,'close');
 const p=open([0x60],owner),ps=seed(p,{4:0x8010});host(p,'protect',[0x7000,1,1]);const unchanged=Buffer.from(read(p,0x8000,16));
 run(p,1,ps,{retired:0,reason:5,fault:[2,0x7ffc,2,4],expectedPacket:packet(1,2,0x7ffc,2,4),staged:ps.r,label:'PUSHALL later write permission preserves earlier slots'});assert.deepEqual(read(p,0x8000,16),unchanged);
 host(p,'protect',[0x7000,1,3]);run(p,1,next(ps,0x1001,{4:0x7ff0}),{label:'PUSHALL protect-only same-input retry'});host(p,'close');
}
// POPALL restores seven values across a page boundary and ignores the poisoned SP value.
{
 const c=open([0x61]);host(c,'map',[0x9000,1,3]);host(c,'map',[0xa000,1,3]);
 // The saved SP value differs from the correct post-pop pointer.
 upload(c,0x8ff4,wordBytes([7,6,5,0xdeadbeef,3,2,1,0],4));const s=seed(c,{4:0x8ff4});
 run(c,1,next(s,0x1001,{0:0,1:1,2:2,3:3,4:0x9014,5:5,6:6,7:7}),{label:'POPALL reads seven values and preservesFLAGS'});host(c,'close');
}
// A completed stack store invalidates consumed code before the next instruction.
for(const owner of['replacement','resident']){
 const c=open([0x60,0x90],owner),s=seed(c,{4:0x1020});
 run(c,2,next(s,0x1001,{4:0x1000}),{retired:1,reason:6,label:'PUSHALL consumed-code SMC completes exactlyonce'});
 const before=arena(c);assert.equal(c.run(c.base,c.base+56,1,c.base+96),4);assert.deepEqual(arena(c),before,'stale stack capture neutral');host(c,'close');
}
assert.deepEqual(pins(),sourcePins);writeFileSync(join(output,'result.json'),JSON.stringify({engine:{path:enginePath,sha256:engine.sha256,bytes:engine.bytes.length},source_pins:sourcePins,cases,modules,observations,helper_trace:helpers,
 claim:'Actual 16/32-bit stack frames, original-SP capture, skipped POP slot, nested overlap, nesting modulo32, final allocation probe, fault repair and consumed-code SMC. Flat32 SS profile; allocations are not initialized. No hardware partial-write fault equivalence or wholeRAM claim.'},null,2)+'\n');
console.log(JSON.stringify({output,contexts:cases.length,modules:modules.length,runs:observations.length,helpers:helpers.length}));
