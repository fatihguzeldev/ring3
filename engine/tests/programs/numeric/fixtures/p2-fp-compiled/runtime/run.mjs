import {ARENA_SIZE} from '../../../../../support/wasm/arena.mjs';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync,writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../../../../../support/wasm/engine.mjs';

const [enginePath,output,root]=process.argv.slice(2);
assert.ok(enginePath&&output&&root);
const SIZE=ARENA_SIZE,T=140,FP=4236,SV=4364,ENTRY=0x2000,CALLER=0x3000;
const COEFF=0x5010,INPUT=0x6010,VECTOR=0x7010,SCALAR=0xa010,COUNT_OUT=0xa030,STACK=0x9020,ESP=0x9080;
const engine=readEngine(enginePath),initial=readFileSync(join(output,'initial-arena.bin'));
const hash=b=>createHash('sha256').update(b).digest('hex'),hex=s=>Buffer.from(s,'hex');
assert.equal(initial.length,SIZE);assert.equal(engine.sha256,hash(engine.bytes));
const word=value=>{const b=Buffer.alloc(4);b.writeUInt32LE(value>>>0);return b;};
const words=values=>Buffer.concat(values.map(word));
const lanes=values=>{const b=Buffer.alloc(values.length*8);values.forEach((v,i)=>b.writeBigUInt64LE(BigInt('0x'+v),i*8));return b;};
const record=(magic,version,size,values)=>{const b=Buffer.alloc(size);b.write(magic);b.writeUInt16LE(version,4);b.writeUInt16LE(1,6);b.writeUInt32LE(size,8);values.forEach((n,i)=>b.writeUInt32LE(n>>>0,16+i*4));return b;};
const state=(r,pc,flags)=>record('R3ST',1,56,[...r,pc,flags]);
const exit=(reason,retired,{detail=0,address=0,access=0,length=0}={})=>record('R3EX',reason===5?12:6,40,[reason===5?23:reason,retired,reason===5?4|(access===2?2:0)|(detail===2?1:0):detail,address,access,length]);
const packet=(version,value=0,length=0,{status=0,detail=0,address=0,access=0}={})=>record('R3MH',status===1?15:version,40,[status===1?9:status,value,status===1?4|(access===2?2:0)|(detail===2?1:0):detail,address,access,length]);
const pattern=(length,multiplier=17,add=23)=>Buffer.from(Array.from({length},(_,i)=>(i*multiplier+add)&255));
const parents=()=>[0xa1001234,0xa1012345,0xa1023456,0xa1034567,ESP,0xa1056789,0xa106789a,0xa10789ab];
const sourcePaths=['engine/src/cpu/x86/decode/x87.rs','engine/src/cpu/x86/decode/simd.rs','engine/src/cpu/dbt/wasm/instructions/x87.rs','engine/src/cpu/dbt/wasm/instructions/simd.rs','engine/src/cpu/dbt/wasm/instructions/simd/direct.rs','engine/src/process/cpu/x87.rs', 'engine/src/process/cpu/mod.rs', 'engine/src/process/mod.rs','engine/src/process/cpu/simd.rs','engine/src/process/memory/span.rs', 'engine/src/process/memory/mod.rs','engine/src/cpu/x86/x87.rs','engine/src/cpu/x86/simd/float.rs', 'engine/src/math/mod.rs', 'engine/src/math/ieee.rs', 'engine/src/math/integer.rs', 'engine/src/lib.rs','engine/src/abi/x86/x87.rs','engine/src/abi/x86/simd.rs','engine/tests/support/wasm/engine.mjs','engine/tests/programs/numeric/fixtures/p2-fp-compiled/runtime/run.mjs',...['x87','sse2'].flatMap(kind=>['kernel.c','kernel.obj','kernel.text.bin','kernel.disasm.txt','receipt.json','cases.json'].map(file=>'engine/tests/programs/numeric/fixtures/p2-fp-compiled/'+kind+'/'+file))];
const sourceHashes=()=>Object.fromEntries(sourcePaths.map(path=>[path,hash(readFileSync(join(root,path)))])),sourceBefore=sourceHashes();
const specs=Object.fromEntries(['x87','sse2'].map(kind=>[kind,{cases:JSON.parse(readFileSync(join(root,'engine/tests/programs/numeric/fixtures/p2-fp-compiled',kind,'cases.json'),'utf8')),text:readFileSync(join(root,'engine/tests/programs/numeric/fixtures/p2-fp-compiled',kind,'kernel.text.bin'))}]));
assert.equal(specs.x87.text.toString('hex'),'5589e583e4f083ec208b450cd9eedb3c2485c074138b4d08db2c24db29dec1db3c2483c1104875f0db2c2489ec5dc3');
assert.equal(specs.sse2.text.toString('hex'),'555357568b4c24248b4424208b54241c660f280285c074368b7424188b7c2414660f284a10660f28522031db89c5660f281c1e660f59d9660f58c3660f28da660f58d8660f291c1f83c3104d75e0660f28c8660f15c8f20f58c8660f284230f20f59c8660f15c0f20f58c1f20f11015e5f5b5dc3');
const modules=[],contexts=[],observations=[],helpers=[],detections=[],diagnostics=[],repairs=[],cases=[];
const arena=c=>Buffer.from(new Uint8Array(c.memory.buffer,c.base,SIZE));
const cpu=c=>{const a=arena(c);return{registers:Array.from({length:8},(_,i)=>a.readUInt32LE(16+i*4)),pc:a.readUInt32LE(48),flags:a.readUInt32LE(52)};};
function input(c,offset,bytes){const expected=arena(c);expected.set(bytes,offset);new Uint8Array(c.memory.buffer,c.base+offset,bytes.length).set(bytes);assert.deepEqual(arena(c),expected,'declared arena input');}
function host(c,name,args,span){const expected=arena(c);assert.equal(c.api[name](...args),0,name);if(span)expected.set(arena(c).subarray(...span),span[0]);assert.deepEqual(arena(c),expected,'host full4796 '+name);}
function ramWrite(c,address,bytes){for(let i=0;i<bytes.length;i++)c.ram.set((address+i)>>>0,bytes[i]);}
function ramRead(c,address,length){return Buffer.from(Array.from({length},(_,i)=>{const value=c.ram.get((address+i)>>>0);assert.notEqual(value,undefined,'declared RAM byte');return value;}));}
function upload(c,address,bytes){input(c,T,bytes);host(c,'upload',[address,bytes.length]);ramWrite(c,address,bytes);}
function checkRam(c,address,expected,label){for(let i=0;i<expected.length;i++){const before=arena(c);assert.equal(c.api.read8(address+i),0,label);before.set(packet(2,expected[i],1),100);assert.deepEqual(arena(c),before,label+' full4796 byte '+i);diagnostics.push({context:c.id,case:c.case,label,address:address+i,expected:expected[i]});}}
function dirtyFp(){const b=Buffer.from(initial.subarray(FP,FP+128));b.writeUInt16LE(0x37f,16);b.writeUInt16LE(0,18);b.writeUInt16LE(0xffff,20);b.writeUInt16LE(0,22);b.writeUInt32LE(0x11223344,24);b.writeUInt32LE(0x55667788,28);b.writeUInt16LE(0x1b,32);b.writeUInt16LE(0x23,34);for(let i=0;i<8;i++){b.writeBigUInt64LE(0x8000000000000100n+BigInt(i),40+i*10);b.writeUInt16LE(0x3fff,48+i*10);}return b;}
function dirtySimd(){const b=Buffer.from(initial.subarray(SV,SV+160));for(let i=0;i<8;i++)pattern(16,13,11+i*19).copy(b,32+i*16);return b;}
function callerBytes(kind){
 // the live count is pushed from EAX, so all cases reuse one captured caller.
 const prefix=kind==='x87'?Buffer.concat([hex('50'),hex('68'),word(INPUT)]):Buffer.concat([hex('68'),word(SCALAR),hex('50'),hex('68'),word(COEFF),hex('68'),word(INPUT),hex('68'),word(VECTOR)]);
 const returnPc=CALLER+prefix.length+5,call=Buffer.concat([hex('e8'),word(ENTRY-returnPc)]);
 const suffix=kind==='x87'?Buffer.concat([hex('83c408db3d'),word(SCALAR),hex('ebfe')]):Buffer.concat([hex('83c414a3'),word(COUNT_OUT),hex('ebfe')]);
 return[{pc:CALLER,bytes:Buffer.concat([prefix,call]),instructions:kind==='x87'?3:6},{pc:returnPc,bytes:suffix,instructions:3}];
}
function open(kind,owner,entries){
 const instance=new WebAssembly.Instance(engine.module,{}),c={id:contexts.length+1,kind,owner,entries,memory:instance.exports.memory,api:{},ram:new Map(),case:null,materials:new Map()};
 for(const[name,fn]of Object.entries(instance.exports))if(name.startsWith('ring3_abi_v1_')&&typeof fn==='function')c.api[name.slice(13)]=fn;
 assert.equal(c.api.open(12,c.id,0x4650434b),0);c.base=c.api.arena_ptr()>>>0;input(c,0,initial);
 for(const[page,permission]of[[0x2000,7],[0x3000,7],[0x5000,3],[0x6000,3],[0x7000,3],[0x9000,3],[0xa000,3]])host(c,'map',[page,1,permission]);
 upload(c,ENTRY,specs[kind].text);const caller=callerBytes(kind);for(const block of caller)upload(c,block.pc,block.bytes);
 host(c,'protect',[0x2000,1,5]);host(c,'protect',[0x3000,1,5]);
 const layout=kind==='x87'?[[0,21,9],[21,3,1],[24,16,7],[40,7,4]]:[[0,24,10],[24,22,6],[46,32,9],[78,38,13]];
 c.kernelBlocks=layout.map(([offset,length,instructions])=>({pc:ENTRY+offset,bytes:specs[kind].text.subarray(offset,offset+length),instructions}));
 c.callerBlocks=caller;c.blocks=[...c.kernelBlocks,...caller];
 c.returnPc=caller[1].pc;
 capture(c,kind==='sse2'?'caller':'whole');contexts.push({id:c.id,kind,owner,entries,mapped_pages:7});return c;
}
function capture(c,label){
 if(c.material===label)return;
 if(c.owner==='resident'&&c.materials.has(label)){Object.assign(c,c.materials.get(label));c.material=label;return;}
 const blocks=label==='caller'?c.callerBlocks:label==='kernel'?c.kernelBlocks:c.blocks;
 input(c,T,words(blocks.flatMap(b=>c.entries?[b.pc]:[b.pc,b.bytes.length])));
 const method=c.owner==='resident'?(c.entries?'compile_resident_entries':'compile_resident'):(c.entries?'compile_entries':'compile');
 host(c,method,c.entries?[blocks.length,0]:[blocks.length],c.owner==='resident'?[T,T+24]:null);
 const a=arena(c);c.binding=c.owner==='resident'?{low:a.readUInt32LE(T+8),high:a.readUInt32LE(T+12),pointer:a.readUInt32LE(T+16),length:a.readUInt32LE(T+20)}:{generation:c.api.generation(),pointer:c.api.module_ptr()>>>0,length:c.api.module_len()>>>0};
 const bytes=Buffer.from(new Uint8Array(c.memory.buffer,c.binding.pointer,c.binding.length)),module=new WebAssembly.Module(bytes),imports=WebAssembly.Module.imports(module),ring3={};
 assert.deepEqual(WebAssembly.Module.exports(module),[{name:'run',kind:'function'}]);assert.deepEqual(imports[0],{module:'env',name:'memory',kind:'memory'});
 const allowed=new Set(['guard_detect','guard_detect_resident','finish_exception_detection','cpu_read32','cpu_store32','cpu_store_resident32','x87_op','x87_resident_op','cpu_read','cpu_write','cpu_write_resident','simd_execute','simd_execute_resident']);
 assert.equal(imports[1].name,c.owner==='resident'?'guard_detect_resident':'guard_detect');assert.equal(imports.at(-1).name,'finish_exception_detection');
 for(const im of imports.slice(1)){assert.equal(im.module,'ring3');assert.equal(im.kind,'function');assert.ok(allowed.has(im.name),im.name);assert.equal(typeof c.api[im.name],'function');ring3[im.name]=(...args)=>forward(c,im.name,args);}
 c.run=new WebAssembly.Instance(module,{env:{memory:c.memory},ring3}).exports.run;assert.equal(c.run.length,4);
 const file='module-'+(modules.length+1)+'.wasm';writeFileSync(join(output,file),bytes,{flag:'wx'});modules.push({context:c.id,kind:c.kind,owner:c.owner,entries:c.entries,material:label,file,bytes:bytes.length,sha256:hash(bytes),imports,blocks:blocks.map(({pc,bytes,instructions})=>({pc,length:bytes.length,instructions})),...c.binding});c.material=label;c.materials.set(label,{binding:c.binding,run:c.run});
}
function forward(c,name,args){
 const before=arena(c),status=c.api[name](...args),after=arena(c);if(name.startsWith('guard')){assert.equal(typeof status,'bigint');assert.ok(status>0n,'positive bound entry epoch');c.epoch=status;assert.deepEqual(after,before,'bound guard neutrality');return status;}
 if(name==='finish_exception_detection'){assert.equal(status,0,'actual stop finalizer');assert.deepEqual(args,[c.epoch,...[72,80,84,88,92].map(at=>after.readUInt32LE(at)),0,0,0,0,0,0]);assert.deepEqual(after,before,'detector is arena-neutral');detections.push({context:c.id,case:c.case,args:args.map(n=>typeof n==='bigint'?n.toString():n>>>0),status});return status;}
 const resident=name==='cpu_store_resident32'||name==='x87_resident_op'||name==='cpu_write_resident'||name==='simd_execute_resident',p=args.slice(resident?4:0).map(n=>n>>>0),expected=Buffer.from(before);
 let wanted;
 if(name==='cpu_read32'){const value=ramRead(c,p[0],4).readUInt32LE(0);wanted=packet(10,value,4);}
 else if(name==='cpu_store32'||name==='cpu_store_resident32'){wanted=packet(10,0,4);ramWrite(c,p[0],word(p[1]));}
 else if(name==='cpu_read'){wanted=packet(7,0,p[1]);const value=ramRead(c,p[0],p[1]);assert.deepEqual(after.subarray(T,T+p[1]),value,'canonical CPU span read');expected.set(value,T);}
 else if(name==='cpu_write'||name==='cpu_write_resident'){
  if(c.failStore){assert.equal(p[0],SCALAR);wanted=packet(7,0,p[1],{status:1,detail:2,address:SCALAR,access:2});}
  else{wanted=packet(7,0,p[1]);ramWrite(c,p[0],before.subarray(T,T+p[1]));}
 }else if(name==='simd_execute'||name==='simd_execute_resident'){
  wanted=packet(7);
  if((p[0]&15)===1){assert.equal(c.kind,'sse2');assert.deepEqual(p.slice(0,5),[0x223001,0x818,0,0,0],'original MOVSD memory-output descriptor');expected.set(c.scalarBytes,T);}else expected.set(after.subarray(SV,SV+160),SV);
  expected.set(after.subarray(T+512,T+520),T+512);assert.equal(after.readUInt32LE(T+516),p[5],'floating operation preserves integer flags');
 }else if(name==='x87_op'||name==='x87_resident_op'){
  const length=[ENTRY+0xc,ENTRY+0x1d].includes(p[3])?0:10;
  if(c.failStore){assert.equal(p[3],c.returnPc+3);assert.equal(p[1],SCALAR);wanted=packet(7,0,10,{status:1,detail:2,address:SCALAR,access:2});}
  else{wanted=packet(7,p[2],length);expected.set(after.subarray(FP,FP+128),FP);}
 }else assert.fail(name);
 assert.equal(status,c.failStore&&['cpu_write','cpu_write_resident','x87_op','x87_resident_op'].includes(name)?23:0,name+' canonical status');assert.deepEqual(after.subarray(100,140),wanted,name+' canonical packet');expected.set(wanted,100);assert.deepEqual(after,expected,name+' declared helper mutations');
 helpers.push({context:c.id,case:c.case,name,args:args.map(n=>n>>>0),payload:p,status,packet_hex:after.subarray(100,140).toString('hex'),...(name==='cpu_write'||name==='cpu_write_resident'?{write_hex:before.subarray(T,T+p[1]).toString('hex')}:{}),...(name.startsWith('x87')?{x87_hex:after.subarray(FP,FP+128).toString('hex')}:{}),...(name.startsWith('simd')?{simd_hex:after.subarray(SV,SV+160).toString('hex')}:{} )});return status;
}
function seed(c,row,fp,sv){input(c,0,initial);const r=parents();r[0]=row.count;input(c,0,state(r,CALLER,0xcd7));input(c,56,exit(1,0));input(c,100,packet(1,0xad));input(c,FP,fp);input(c,SV,sv);return r;}
function run(c,label,budget,r,pc,flags,fp,sv,{reason=1,retired=budget,fault={}}={}){
 const before=arena(c),at=helpers.length,expected=Buffer.from(before);expected.set(state(r,pc,flags));expected.set(exit(reason,retired,fault),56);expected.set(fp,FP);expected.set(sv,SV);
 const status=c.run(c.base,c.base+56,budget,c.base+96);assert.equal(status,0,label);const actual=arena(c);
 // these are checked scratch spans, not floating-state or CPU oracles.
 expected.set(actual.subarray(100,140),100);expected.set(actual.subarray(T,T+512),T);expected.set(actual.subarray(T+512,T+520),T+512);assert.deepEqual(actual,expected,label+' full4796');
 const item={context:c.id,kind:c.kind,case:c.case,label,budget,status,reason:actual.readUInt32LE(72),retired:actual.readUInt32LE(76),cpu:cpu(c),helper_calls:helpers.length-at,arena_hex:actual.toString('hex')};observations.push(item);return item;
}
function expectedX87(start,row,stored=false,pc=ENTRY+0x28){const b=Buffer.from(start);b.writeUInt16LE(stored?row.expected.return_status&~0x3800:row.expected.return_status,18);b.writeUInt16LE(stored?0xffff:row.expected.return_tag,20);b.writeUInt16LE(stored?0x33d:0x32c,22);b.writeUInt32LE(pc,24);b.writeUInt32LE(stored?SCALAR:0x9050,28);b.writeUInt16LE(0x1b,32);b.writeUInt16LE(0x23,34);if(row.count)hex(row.inputs.at(-1).hex80).copy(b,40+6*10);hex(row.expected.hex80).copy(b,40+7*10);return b;}
function x87Case(c,row,index){
 c.case=row.name;const data=pattern(80,13,7),out=pattern(64,19,31),stack=pattern(128);row.inputs.forEach((v,i)=>{hex(v.hex80).copy(data,16+i*16);data.fill(0xa5,26+i*16,32+i*16);});upload(c,INPUT-16,data);upload(c,SCALAR-16,out);upload(c,STACK,stack);
 const fp=dirtyFp(),sv=dirtySimd(),r=seed(c,row,fp,sv),returned=[...r];returned[0]=0;returned[1]=row.count?INPUT+row.count*16:r[1];returned[4]=0x9078;
 const before=helpers.length,finalFp=expectedX87(fp,row);run(c,'whole-original-natural-ST0-return',3+row.kernel_retired,returned,c.returnPc,0x446,finalFp,sv);
 for(const[address,value]of[[0x907c,row.count],[0x9078,INPUT],[0x9074,c.returnPc],[0x9070,r[5]]])stack.writeUInt32LE(value>>>0,address-STACK);hex(row.expected.hex80).copy(stack,0x9050-STACK);ramWrite(c,0x9050,hex(row.expected.hex80));
 checkRam(c,INPUT-16,data,'original x87 input and padded stride');checkRam(c,STACK,stack,'original x87 stack and local total');checkRam(c,SCALAR-16,out,'caller output untouched before FSTP');
 const cleaned=[...returned];cleaned[4]=ESP;run(c,'natural-return-caller-cleans-cdecl-stack',1,cleaned,c.returnPc+3,0x412,finalFp,sv);
 if(index===1){host(c,'protect',[0xa000,1,1]);c.failStore=true;run(c,'returned-ST0-store-write-fault',1,cleaned,c.returnPc+3,0x412,finalFp,sv,{reason:5,retired:0,fault:{detail:2,address:SCALAR,access:2,length:10}});c.failStore=false;checkRam(c,SCALAR-16,out,'failed x87 store preserves output');const stopped=arena(c);host(c,'protect',[0xa000,1,3]);assert.deepEqual(arena(c),stopped);repairs.push({context:c.id,case:row.name,kind:'x87 returned ST0 output write',repair:'protect only; no CPU/FP/RAM upload, remap or recapture'});}
 const storedFp=expectedX87(fp,row,true,c.returnPc+3);run(c,'caller-consumes-ST0-by-extended80-store',2,cleaned,c.returnPc+9,0x412,storedFp,sv);hex(row.expected.hex80).copy(out,16);checkRam(c,SCALAR-16,out,'literal extended80 result and canaries');checkRam(c,STACK,stack,'callee stack retained after caller');checkRam(c,INPUT-16,data,'x87 input remains unchanged');
 const events=helpers.slice(before),x87=events.filter(e=>e.name.startsWith('x87'));assert.equal(x87.length,4+4*row.count+(index===1?1:0));assert.deepEqual(x87.filter(e=>e.payload[3]===ENTRY+0x1b).map(e=>e.payload[1]),row.inputs.map((_,i)=>INPUT+i*16));assert.equal(events.filter(e=>e.name==='cpu_read32').length,row.count?4:3);assert.equal(events.filter(e=>e.name.startsWith('cpu_store')).length,4);
 cases.push({context:c.id,kind:'x87',name:row.name,count:row.count,raw_return80:row.expected.hex80,output_hex:out.toString('hex'),data_hex:data.toString('hex'),stack_hex:stack.toString('hex'),helper_calls:events.length,natural_return_pc:c.returnPc});
}
function sseCase(c,row,index){
 capture(c,'caller');c.case=row.name;c.scalarBytes=lanes([row.scalar_output_raw64]);const coeff=pattern(96,13,7),data=pattern(80,19,31),vectors=pattern(80,23,29),scalar=pattern(64,29,3),stack=pattern(128);lanes(row.coefficients_raw64.flat()).copy(coeff,16);lanes(row.input_raw64.flat()).copy(data,16);upload(c,COEFF-16,coeff);upload(c,INPUT-16,data);upload(c,VECTOR-16,vectors);upload(c,SCALAR-16,scalar);upload(c,STACK,stack);
 const fp=dirtyFp(),sv=dirtySimd();sv.writeUInt32LE(parseInt(row.mxcsr_before,16),16);specs.sse2.cases.cpu_contract.initial_xmm_bytes.forEach((value,i)=>hex(value).copy(sv,32+i*16));
 const r=seed(c,row,fp,sv),after=Buffer.from(sv);after.writeUInt32LE(parseInt(row.mxcsr_after,16),16);for(const[index,values]of Object.entries(row.final_xmm_raw64))lanes(values).copy(after,32+Number(index)*16);
 const prefix=[...r];prefix[1]=SCALAR;prefix[2]=COEFF;prefix[4]=0x9058;if(row.count){prefix[3]=row.count*16;prefix[5]=0;prefix[6]=INPUT;prefix[7]=VECTOR;}
 const before=helpers.length,prefixRetired=row.count?23+9*row.count:17,called=[...r];called[4]-=24;
 // caller and kernel stay within the existing 64 KiB material limit; real CALL/RET cross cold boundaries.
 run(c,'natural-call-needs-kernel-material',7,called,ENTRY,0xcd7,fp,sv,{reason:3,retired:6});capture(c,'kernel');
 run(c,'whole-original-computed-before-scalar-store',prefixRetired,prefix,ENTRY+0x6b,0x446,fp,after);
 for(const[address,value]of[[0x907c,SCALAR],[0x9078,row.count],[0x9074,COEFF],[0x9070,INPUT],[0x906c,VECTOR],[0x9068,c.returnPc],[0x9064,r[5]],[0x9060,r[3]],[0x905c,r[7]],[0x9058,r[6]]])stack.writeUInt32LE(value>>>0,address-STACK);lanes(row.output_raw64.flat()).copy(vectors,16);
 checkRam(c,VECTOR-16,vectors,'literal original vector outputs and canaries');checkRam(c,SCALAR-16,scalar,'scalar output untouched before original store');checkRam(c,STACK,stack,'original callee-save and argument slots');
 if(index===0){host(c,'protect',[0xa000,1,1]);c.failStore=true;run(c,'original-scalar-store-write-fault',1,prefix,ENTRY+0x6b,0x446,fp,after,{reason:5,retired:0,fault:{detail:2,address:SCALAR,access:2,length:8}});c.failStore=false;checkRam(c,SCALAR-16,scalar,'failed original scalar store preserves RAM');const stopped=arena(c);host(c,'protect',[0xa000,1,3]);assert.deepEqual(arena(c),stopped);repairs.push({context:c.id,case:row.name,kind:'original SSE2 scalar output write',repair:'protect only; no CPU/SIMD/RAM upload, remap or recapture'});}
 const returned=[...r];returned[0]=row.count;returned[1]=SCALAR;returned[2]=COEFF;returned[4]=0x906c;run(c,'whole-original-natural-cdecl-return',7,returned,c.returnPc,0x446,fp,after,{reason:3,retired:6});capture(c,'caller');lanes([row.scalar_output_raw64]).copy(scalar,16);checkRam(c,SCALAR-16,scalar,'literal original scalar result');
 const finished=[...returned];finished[4]=ESP;run(c,'caller-consumes-natural-count-return',3,finished,c.returnPc+8,0x412,fp,after);scalar.writeUInt32LE(row.count,COUNT_OUT-(SCALAR-16));checkRam(c,SCALAR-16,scalar,'caller count result and scalar canaries');checkRam(c,COEFF-16,coeff,'coefficient input unchanged');checkRam(c,INPUT-16,data,'vector input unchanged');checkRam(c,VECTOR-16,vectors,'vector output retained');checkRam(c,STACK,stack,'callee-save stack retained');
 const events=helpers.slice(before);assert.equal(events.filter(e=>e.name==='cpu_read32').length,row.count?10:8);assert.equal(events.filter(e=>e.name.startsWith('cpu_store')).length,11);assert.equal(events.filter(e=>e.name==='cpu_read').length,row.count?4+row.count:2);assert.equal(events.filter(e=>e.name.startsWith('cpu_write')).length,row.count+1+(index===0?1:0));assert.equal(events.filter(e=>e.name.startsWith('simd_execute')).length,row.count*3+3);
 cases.push({context:c.id,kind:'sse2',name:row.name,count:row.count,output_hex:vectors.toString('hex'),scalar_hex:scalar.toString('hex'),coefficients_hex:coeff.toString('hex'),input_hex:data.toString('hex'),stack_hex:stack.toString('hex'),helper_calls:events.length,natural_return_pc:c.returnPc});
}
for(const kind of['x87','sse2'])for(const owner of['replacement','resident'])for(const entries of[false,true]){
 const c=open(kind,owner,entries);for(const[index,row]of specs[kind].cases.cases.entries())if(kind==='x87')x87Case(c,row,index);else sseCase(c,row,index);
 writeFileSync(join(output,'context-'+c.id+'-final-arena.bin'),arena(c),{flag:'wx'});host(c,'close',[]);
}
assert.deepEqual(sourceHashes(),sourceBefore);
const result={status:'PASS',engine:{path:enginePath,sha256:engine.sha256,bytes:engine.bytes.length},arena_bytes:SIZE,source_sha256:sourceBefore,initial_arena_sha256:hash(initial),counts:{contexts:contexts.length,modules:modules.length,cases:cases.length,runs:observations.length,retired:observations.reduce((n,r)=>n+r.retired,0),helpers:helpers.length,write_faults:repairs.length,natural_returns:cases.length,diagnostic_bytes:diagnostics.length},contexts,modules,cases,observations,helpers,detections,diagnostics,repairs,
 scope:'whole retained compiler-produced x87/SSE2 functions through natural guest CALL/RET under four bound profiles; literal raw80/raw64 outputs and extended state, callee-save registers and selected input/output/stack canaries; one output write fault/protect-only retry per kernel/profile; per-run complete4796 arenas with canonical checked scratch overlay, saved observations include complete arenas; no whole RAM, hardware timing, arbitrary compiler compatibility or exhaustive floating-point claim'};
writeFileSync(join(output,'result.json'),JSON.stringify(result,null,2)+'\n',{flag:'wx'});console.log(JSON.stringify({status:result.status,output,...result.counts}));
