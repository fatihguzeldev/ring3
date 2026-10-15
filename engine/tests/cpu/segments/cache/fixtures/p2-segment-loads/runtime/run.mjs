import assert from 'node:assert/strict';
import {readFileSync,writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {createHash} from 'node:crypto';
import {readEngine} from '../../../../../../support/wasm/engine.mjs';

const [enginePath,output,root]=process.argv.slice(2),engine=readEngine(enginePath),initial=readFileSync(join(output,'initial-arena.bin'));
const SIZE=initial.length,SEGMENTS=4524,WORK=4588,BASES=4756,hash=b=>createHash('sha256').update(b).digest('hex');
assert.ok(SIZE>=4612);assert.equal(engine.sha256,hash(engine.bytes));
const modules=[],observations=[],helpers=[],cases=[],refusals=[];
const sourcePaths=['engine/src/cpu/x86/decode/segments.rs','engine/src/cpu/dbt/wasm/segments.rs', 'engine/src/cpu/dbt/wasm/segments/context.rs',
 'engine/src/cpu/x86/segments.rs','engine/src/cpu/x86/segments/cache.rs','engine/src/abi/x86/segments.rs','engine/src/cpu/x86/segments/descriptors.rs','engine/src/process/cpu/segments.rs', 'engine/src/process/cpu/mod.rs', 'engine/src/process/mod.rs',
 'engine/src/cpu/x86/decode/strings.rs','engine/src/cpu/dbt/wasm/strings.rs',
 'engine/src/cpu/x86/ir.rs','engine/src/cpu/x86/decode/profile.rs','engine/src/cpu/x86/decode/lower.rs',
 'engine/src/cpu/dbt/wasm/memory.rs','engine/src/cpu/dbt/wasm/emitter.rs','engine/src/abi/cpu_helper.rs',
 'engine/src/process/wasm/mod.rs', 'engine/src/process/wasm/loading.rs', 'engine/src/abi/image.rs', 'engine/src/process/wasm/execution.rs', 'engine/src/process/wasm/calls.rs', 'engine/src/process/wasm/memory.rs', 'engine/src/process/wasm/cpu.rs', 'engine/src/abi/wasm/exports/calls.rs', 'engine/src/abi/wasm/exports/cpu.rs', 'engine/src/abi/wasm/exports/execution.rs', 'engine/src/abi/wasm/exports/image.rs', 'engine/src/abi/wasm/exports/instance.rs', 'engine/src/abi/wasm/exports/memory.rs', 'engine/src/abi/wasm/exports/mod.rs','engine/tests/cpu/segments/cache/wasm/loads.rs', 'engine/tests/cpu/segments/wasm.rs', 'engine/tests/support/wasm.rs', 'engine/tests/cpu/segments/cache/fixtures/p2-segment-loads/runtime/run.mjs'];
const pins=()=>Object.fromEntries(sourcePaths.map(p=>[p,hash(readFileSync(join(root,p)))])),sourcePins=pins();
const arena=c=>Buffer.from(new Uint8Array(c.memory.buffer,c.base,SIZE));
function input(c,at,b){new Uint8Array(c.memory.buffer).set(b,c.base+at);}
function host(c,name,args=[]){assert.equal(c.api[name](...args),0,name);}
function upload(c,address,b){input(c,140,b);host(c,'upload',[address,b.length]);}
function cpu(c){const a=arena(c);return{r:Array.from({length:8},(_,i)=>a.readUInt32LE(16+i*4)),pc:a.readUInt32LE(48),flags:a.readUInt32LE(52)};}
function state(s){const b=Buffer.alloc(56);b.write('R3ST');b.writeUInt16LE(1,4);b.writeUInt16LE(1,6);b.writeUInt32LE(56,8);[...s.r,s.pc,s.flags].forEach((v,i)=>b.writeUInt32LE(v>>>0,16+4*i));return b;}
function seed(c,updates={},flags=0xcd7){const s={r:[0xabcd1234,0x12345678,0x23456789,0x3456789a,0x6040,0x456789ab,0x5010,0x6010],pc:0x1000,flags};for(const[i,v]of Object.entries(updates))s.r[+i]=v>>>0;input(c,0,state(s));return s;}
function next(s,pc,updates={},flags=s.flags){const n={r:[...s.r],pc,flags};for(const[i,v]of Object.entries(updates))n.r[+i]=v>>>0;return n;}
function open(bytes,owner='replacement'){
 const instance=new WebAssembly.Instance(engine.module,{}),c={id:cases.length+1,owner,memory:instance.exports.memory,api:{}};
 for(const n of ['open','close','arena_ptr','map','protect','upload','read8','read16','read32','cpu_read8','cpu_read16','cpu_read32','cpu_store8','cpu_store16','cpu_store32','cpu_store_resident8','cpu_store_resident16','cpu_store_resident32','store8','store16','store32','store_resident8','store_resident16','store_resident32','compile','compile_resident','module_ptr','module_len','guard','guard_resident','guard_detect','guard_detect_resident','finish_exception_detection','segment_load','segment_load_resident','segment_load_far','segment_load_far_resident','set_segment_base','configure_segment_descriptor']){
  c.api[n]=instance.exports['ring3_abi_v1_'+n];assert.equal(typeof c.api[n],'function',n);
 }
 host(c,'open',[8,c.id,0x5347]);c.base=c.api.arena_ptr()>>>0;input(c,0,initial);
 for(const[p,perm]of[[0x1000,7],[0x5000,3],[0x6000,3]])host(c,'map',[p,1,perm]);
 upload(c,0x1000,Buffer.from([...bytes,0xeb,0xfe]));host(c,'protect',[0x1000,1,5]);
 const d=Buffer.alloc(8);d.writeUInt32LE(0x1000);d.writeUInt32LE(bytes.length+2,4);input(c,140,d);host(c,owner==='resident'?'compile_resident':'compile',[1]);
 const a=arena(c),pointer=owner==='resident'?a.readUInt32LE(156):c.api.module_ptr()>>>0,length=owner==='resident'?a.readUInt32LE(160):c.api.module_len()>>>0;
 c.residentBinding=[c.id,0x5347,a.readUInt32LE(148),a.readUInt32LE(152)];
 const wasm=Buffer.from(new Uint8Array(c.memory.buffer,pointer,length)),module=new WebAssembly.Module(wasm),ring3={...c.api};
 for(const imp of WebAssembly.Module.imports(module).filter(i=>i.module==='ring3'&&!i.name.startsWith('guard') && i.name !== 'finish_exception_detection')){
  ring3[imp.name]=(...args)=>{const before=arena(c),status=c.api[imp.name](...args),after=arena(c),allowed=Buffer.from(before);
   allowed.set(after.subarray(100,140),100);
   if(imp.name.startsWith('segment_load')) {allowed.set(after.subarray(SEGMENTS,SEGMENTS+64),SEGMENTS); allowed.set(after.subarray(BASES,BASES+40),BASES);}
   assert.deepEqual(after,allowed,'helper restricted publication');
   const packet=after.subarray(100,140);if(imp.name.startsWith('segment_load')){
    assert.equal(packet.toString('ascii',0,4),'R3MH');assert.equal(packet.readUInt32LE(4),status===23?0x1000f:0x10007);assert.equal(packet.readUInt32LE(8),40);
   }
   helpers.push({context:c.id,name:imp.name,args:args.map(v=>v>>>0),status,packet:packet.toString('hex')});return status;
  };
 }
 c.run=new WebAssembly.Instance(module,{env:{memory:c.memory},ring3}).exports.run;
 const file=`module-${modules.length+1}.wasm`;writeFileSync(join(output,file),wasm);modules.push({context:c.id,owner,file,bytes:length,sha256:hash(wasm),imports:WebAssembly.Module.imports(module)});
 cases.push({id:c.id,owner,code:Buffer.from(bytes).toString('hex')});return c;
}
function run(c,budget,expected,{retired=budget,reason=1,segments={},segmentRecord,segmentBases,ssShadow=false,fault,label='segments'}={}){
 if(reason===5){reason=23;fault=[4|(fault[2]===2?2:0)|(fault[0]===2?1:0),...fault.slice(1)];}
 const before=arena(c),at=helpers.length,status=c.run(c.base,c.base+56,budget,c.base+96),after=arena(c);
 assert.equal(status,0,label);assert.deepEqual(cpu(c),expected,label+' CPU');assert.equal(after.readUInt32LE(72),reason,label+' reason');assert.equal(after.readUInt32LE(76),retired,label+' retire');
 if(fault)assert.deepEqual(Array.from({length:4},(_,i)=>after.readUInt32LE(80+i*4)),fault,label+' fault');
 const full=Buffer.from(before);full.set(state(expected));full.set(after.subarray(56,96),56);full.set(after.subarray(100,140),100);
 for(const[i,{selector,base}]of Object.entries(segments)){if(selector!==undefined)full.writeUInt16LE(selector,SEGMENTS+16+2*i);if(base!==undefined)full.writeUInt32LE(base>>>0,SEGMENTS+32+4*i);}
 if(segmentRecord)full.set(segmentRecord,SEGMENTS);
 if(segmentBases)full.set(segmentBases,BASES);
 if(ssShadow) {full.writeUInt16LE(2,4); full.writeUInt32LE(1,12);}
 assert.equal(after.readUInt32LE(WORK+16),0,label+' ordinary work fallback');assert.deepEqual(after,full,label+' whole arena');
 const file=`arena-${observations.length+1}.bin`;writeFileSync(join(output,file),after);observations.push({context:c.id,label,budget,reason,retired,cpu:cpu(c),helpers:helpers.length-at,exit:after.subarray(56,96).toString('hex'),arena:{file,sha256:hash(after)}});
}
function get(c,address,width){host(c,'read'+width*8,[address]);return arena(c).readUInt32LE(120);}
function put(c,address,value,width){const b=Buffer.alloc(width);width===1?b.writeUInt8(value&255):width===2?b.writeUInt16LE(value&65535):b.writeUInt32LE(value>>>0);upload(c,address,b);}

for(const owner of ['replacement','resident']){
 const c=open([0x8b,0x03],owner),s=seed(c,{3:0x5010}),valid=arena(c);
 for(const segment of[1,2])for(const selector of[0,1,2,3,0x20,0x21,0x22]){
  const malformed=Buffer.from(valid);malformed.writeUInt16LE(selector,SEGMENTS+16+segment*2);input(c,0,malformed);
  const at=helpers.length;assert.equal(c.run(c.base,c.base+56,1,c.base+96),2);assert.deepEqual(arena(c),malformed);assert.equal(helpers.length,at);
  refusals.push({context:c.id,segment,selector,status:2});
 }
 input(c,0,valid);const nullDS=arena(c);nullDS.writeUInt16LE(0,SEGMENTS+22);input(c,SEGMENTS,nullDS.subarray(SEGMENTS,SEGMENTS+64));const at=helpers.length;
 run(c,1,s,{retired:0,reason:11,fault:[0,0,0,0],label:'null default DS faults before any RAM helper'});assert.equal(helpers.length,at);host(c,'close');
}

for(const owner of ['replacement','resident']){
 const c=open([0x8c,0xc0,0x8c,0xc9,0x66,0x8c,0xd2,0x66,0x8c,0xdb,0x8c,0xe6,0x8c,0xef],owner),s=seed(c);
 run(c,6,next(s,0x100e,{0:0x23,1:0x1b,2:0x23450023,3:0x34560023,6:0x3b,7:0}),{label:'all selector reads and parent preservation'});host(c,'close');
 const p=open([0x0f,0xa0,0x66,0x0f,0xa8,0x06,0x66,0x1e,0x8c,0x25,0x10,0x50,0,0],owner),ps=seed(p);
 run(p,5,next(ps,0x100e,{4:0x6034}),{label:'segment PUSH width and MOV memory WORD'});
 assert.equal(get(p,0x603c,4),0x3b);assert.equal(get(p,0x603a,2),0);assert.equal(get(p,0x6036,4),0x23);assert.equal(get(p,0x6034,2),0x23);assert.equal(get(p,0x5010,2),0x3b);host(p,'close');
 const r=open([0x8e,0xe0,0x64,0x8b,0x0b,0x8e,0xe2,0x64,0x8b,0x3b],owner);host(r,'set_segment_base',[4,0x5000]);put(r,0x5010,0x12345678,4);const rs=seed(r,{0:0x3b,2:0,3:0x10});
 const stopped=next(rs,0x1007,{1:0x12345678});
 run(r,4,stopped,{retired:3,reason:11,fault:[0,0,0,0],segments:{4:{selector:0,base:0x5000}},label:'null FS retains base but prevents memory reference'});
 host(r,owner==='resident'?'segment_load_resident':'segment_load',owner==='resident'?[...r.residentBinding,4,0x3b]:[4,0x3b]);run(r,1,next(stopped,0x100a,{7:0x12345678}),{label:'nonnull selector reload permits same CPU retry'});host(r,'close');
}
for(const [target,selector,dpl,present,kind,reason,description]of[
 [4,0x43,3,1,null,11,'unknown'],[4,0x43,3,0,1,14,'not present FS'],[2,0x43,3,0,1,15,'not present SS'],
 [4,0x43,0,1,1,11,'privilege'],[5,0x43,3,1,2,11,'execute only code']]){
 const c=open([0x8e,0xc0+(target<<3)]);if(kind!==null)host(c,'configure_segment_descriptor',[selector,0,dpl,present,kind]);const s=seed(c,{0:selector});
 run(c,1,s,{retired:0,reason,fault:[selector&~3,0,0,0],label:'descriptor refusal '+description});host(c,'close');
}
{
 const c=open([0x0f,0xb4,0x00],'resident');host(c,'configure_segment_descriptor',[0x43,0x7000,3,1,1]);upload(c,0x5ffc,Buffer.from([0x78,0x56,0x34,0x12,0x43,0]));host(c,'protect',[0x6000,1,0]);const s=seed(c,{0:0x5ffc});
 run(c,1,s,{retired:0,reason:5,fault:[2,0x6000,1,6],label:'LFS full far pointer before publication'});host(c,'protect',[0x6000,1,3]);
 run(c,1,next(s,0x1003,{0:0x12345678}),{segments:{4:{selector:0x43,base:0x7000}},label:'LFS protect-only retry with destination EA alias'});host(c,'close');
 const w=open([0x66,0x0f,0xb5,0x03]);host(w,'configure_segment_descriptor',[0x43,0x6000,3,1,1]);upload(w,0x5010,Buffer.from([0xbc,0x9a,0x43,0]));const ws=seed(w,{3:0x5010});
 run(w,1,next(ws,0x1004,{0:0xabcd9abc}),{segments:{5:{selector:0x43,base:0x6000}},label:'LGS WORD offset preserves parent'});host(w,'close');
}
{
 const c=open([0x8e,0xd0]);host(c,'configure_segment_descriptor',[0x43,0x5000,3,1,1]);const s=seed(c,{0:0x43});
 const segmentRecord=Buffer.from(initial.subarray(SEGMENTS,SEGMENTS+64));segmentRecord.writeUInt16LE(3,4);segmentRecord.writeUInt16LE(0x43,20);
 [0xf33fffff,0xf3bfffff,0xf33fffff,0xf33fffff,0xf33fffff,0].forEach((bits,i)=>segmentRecord.writeUInt32LE(bits>>>0,[28,32,36,40,44,56][i]));
 for(const offset of [48,52,60])segmentRecord.writeUInt32LE(0,offset);
 const segmentBases=Buffer.alloc(40);segmentBases.write('R3SB');segmentBases.writeUInt16LE(1,4);segmentBases.writeUInt16LE(1,6);segmentBases.writeUInt32LE(40,8);segmentBases.writeUInt32LE(0x5000,24);
 run(c,1,next(s,0x1002),{segmentRecord,segmentBases,ssShadow:true,label:'nonflat legacy SS load publishes current bases and debug shadow'});host(c,'close');
}
for(const [word,selector]of[[false,0x43],[false,0x3b],[true,0x3b]]){
 const c=open(word?[0x66,0x0f,0xa9]:[0x0f,0xa1]);host(c,'set_segment_base',[4,0x5000]);put(c,0x6030,selector,word?2:4);const s=seed(c,{4:0x6030});
 if(selector===0x43)run(c,1,s,{retired:0,reason:11,fault:[0x40,0,0,0],label:'POP invalid selector keeps ESP'});
 else run(c,1,next(s,word?0x1003:0x1002,{4:0x6030+(word?2:4)}),{segments:{[word?5:4]:{selector:0x3b,base:0x5000}},label:'POP segment successful commit'});
 host(c,'close');
}
// cached access attributes are independent of mutable table entries and retain the 64-byte record.
for(const owner of ['replacement','resident'])for(const prefix of [null,0x26,0x2e,0x36,0x3e]){
 const c=open([...(prefix===null?[]:[prefix]),0x8b,0x03],owner);
 host(c,'configure_segment_descriptor',[0x43,0x5000,3,1,0]);
 host(c,owner==='resident'?'segment_load_resident':'segment_load',owner==='resident'?[...c.residentBinding,4,0x43]:[4,0x43]);
 const cached=arena(c).subarray(SEGMENTS,SEGMENTS+64);
 assert.equal(cached.readUInt16LE(4),2);assert.equal(cached.readUInt32LE(44),0xf31fffff);
 const before=arena(c);host(c,'configure_segment_descriptor',[0x43,0x6000,3,1,1]);assert.deepEqual(arena(c),before);
 const stable=Buffer.from(arena(c));put(c,0x5010,0xa1b2c3d4,4);const s=seed(c,{3:0x5010});
 run(c,1,next(s,0x1000+(prefix===null?2:3),{0:0xa1b2c3d4}),{label:'v2 cached attributes with impliedzero legacy base '+prefix});
 assert.deepEqual(arena(c).subarray(SEGMENTS,SEGMENTS+64),cached);
 const valid=arena(c);
 for(const[offset,bits]of [[28,1],[32,0xf1bfffff],[36,0xf13fffff],[40,0xf39fffff],[44,0x933fffff],[56,0xf33fffff],[60,1]]){
  const malformed=Buffer.from(valid);malformed.writeUInt32LE(bits>>>0,SEGMENTS+offset);input(c,0,malformed);
  const at=helpers.length;assert.equal(c.run(c.base,c.base+56,1,c.base+96),2);assert.deepEqual(arena(c),malformed);assert.equal(helpers.length,at);
  refusals.push({context:c.id,offset,bits:bits>>>0,status:2});
 }
 input(c,0,valid);host(c,'set_segment_base',[4,0x7000]);
 assert.equal(arena(c).readUInt32LE(SEGMENTS+44),0xf31fffff);assert.equal(arena(c).readUInt32LE(SEGMENTS+48),0x7000);
 host(c,owner==='resident'?'segment_load_resident':'segment_load',owner==='resident'?[...c.residentBinding,4,0x43]:[4,0x43]);
 assert.equal(arena(c).readUInt16LE(SEGMENTS+4),1);assert.equal(arena(c).readUInt32LE(SEGMENTS+48),0x7000);
 host(c,'close');
}
// All plain string widths use the same precise element body without reading/decrementing CX.
for(const[kind,opcode]of Object.entries({Move:0xa4,Store:0xaa,Load:0xac,Compare:0xa6,Scan:0xae}))for(const width of[1,2,4]){
 const bytes=[0x64,0x67,...(width===2?[0x66]:[]),opcode+(width===1?0:1)],c=open(bytes);host(c,'set_segment_base',[4,0x5000]);
 put(c,0x5014,0x11,width);put(c,0x6014,0x11,width);const s=seed(c,{0:kind==='Scan'?0x11:0xabcd1234,1:0xabcd0003,6:0x12340014,7:0x56786014});
 const updates={};if(['Move','Load','Compare'].includes(kind))updates[6]=0x12340014-width;if(['Move','Store','Compare','Scan'].includes(kind))updates[7]=0x56786014-width;
 if(kind==='Load')updates[0]=width===1?0xabcd1211:width===2?0xabcd0011:0x11;
 run(c,1,next(s,0x1000+bytes.length,updates,['Compare','Scan'].includes(kind)?0x446:s.flags),{label:'plain '+kind+width+' DF/address16/FS'});
 if(kind==='Move')assert.equal(get(c,0x6014,width),0x11);if(kind==='Store')assert.equal(get(c,0x6014,width),width===1?0x34:width===2?0x1234:0xabcd1234);host(c,'close');
}
assert.deepEqual(pins(),sourcePins);writeFileSync(join(output,'result.json'),JSON.stringify({engine:{path:enginePath,sha256:engine.sha256,bytes:engine.bytes.length},source_pins:sourcePins,arena_bytes:SIZE,cases,modules,observations,refusals,helper_trace:helpers,
 claim:'Configured flat32 descriptor loads with precise GP/NP/SS and checked far spans; FS/GS cached bases, selector reads, stack widths, and plain strings. Null data selector loads succeed but accesses fault before memory. Loaded cache v1/v2 preservation, malformed cache neutral refusal and impliedzero legacy bases are covered; generic segment rights/limit enforcement, asynchronous interrupts and descriptor-table queries remain outside this fixture.'},null,2)+'\n');
console.log(JSON.stringify({output,contexts:cases.length,modules:modules.length,runs:observations.length,helpers:helpers.length}));
