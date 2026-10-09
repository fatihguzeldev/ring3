import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync, writeFileSync, readdirSync, lstatSync } from "node:fs";
import { join } from "node:path";
import { readEngine } from "../support/engine.mjs";
const [enginePath, output, root] = process.argv.slice(2), SIZE=4364, PC=0x1000;
assert.ok(enginePath && output && root);assert.match(process.env.RING3_ENGINE_SHA256??"",/^[a-f0-9]{64}$/);
assert.deepEqual(readdirSync(output).sort(),["initial-arena.bin","word-and.x86"]);
for(const name of readdirSync(output))assert.ok(lstatSync(join(output,name)).isFile()&&!lstatSync(join(output,name)).isSymbolicLink());
const engine=readEngine(enginePath),hash=b=>createHash("sha256").update(b).digest("hex");
assert.equal(engine.sha256,process.env.RING3_ENGINE_SHA256);assert.deepEqual(WebAssembly.Module.imports(engine.module),[]);
const initial=readFileSync(join(output,"initial-arena.bin"));assert.equal(initial.length,SIZE);
const forms=[];
for(const opcode of [0x21,0x23])for(let left=0;left<8;left++)for(const right of [left,(left+1)&7])
 forms.push({bytes:Buffer.from([0x66,opcode,0xc0|(opcode===0x21?(right<<3)|left:(left<<3)|right)]),left,right,kind:"register"});
for(let left=0;left<8;left++)for(const immediate of [0,0x7fff,0x8000,0xffff])
 forms.push({bytes:Buffer.from([0x66,0x81,0xe0|left,immediate&255,immediate>>>8]),left,immediate,kind:"imm16"});
for(let left=0;left<8;left++)for(const immediate of [0,0x7f,0x80,0xff])
 forms.push({bytes:Buffer.from([0x66,0x83,0xe0|left,immediate]),left,immediate:immediate&128?immediate|0xff00:immediate,kind:"signed83"});
for(const immediate of [0,0x7fff,0x8000,0xffff,0x6667,0x6766,0xf0f3,0xf3f2])forms.push({bytes:Buffer.from([0x66,0x25,immediate&255,immediate>>>8]),left:0,immediate,kind:"ax"});
assert.equal(forms.length,104);const bank=Buffer.alloc(2048,0xcc),groups=[];
for(let i=0;i<forms.length;i+=32){const group=groups.length,start=group*256;let at=start;
 const rows=forms.slice(i,i+32).map(form=>{const row={...form,pc:PC+at};bank.set(form.bytes,at);at+=form.bytes.length;return row;});
 bank.set([0xeb,0],at);groups.push({pc:PC+start,length:at-start+2,rows});}
const chain=Buffer.from([0x66,0xb8,0xff,0xff,0x66,0x81,0xe0,0xff,0,0x0f,0x92,0xc2,0x66,0xb9,0,0xff,0x66,0x21,0xc8,0x0f,0x94,0xc3,0xeb,0]);
bank.set(chain,1792);groups.push({pc:PC+1792,length:chain.length,rows:[]});assert.deepEqual(bank,readFileSync(join(output,"word-and.x86")));
const paths=["engine/src/abi/arena.rs","engine/src/abi/header.rs","engine/src/abi/x86/state.rs","engine/src/abi/x86/exit.rs","engine/src/abi/x86/x87.rs",
 "engine/src/cpu/x86/ir.rs","engine/src/cpu/x86/decode/decoder.rs","engine/src/cpu/x86/decode/operands.rs","engine/src/cpu/x86/decode/lower.rs","engine/src/cpu/x86/decode/profile.rs",
 "engine/src/cpu/dbt/region.rs","engine/src/cpu/dbt/wasm/integer.rs","engine/src/cpu/dbt/wasm/emitter.rs","engine/src/cpu/dbt/wasm/abi.rs","engine/src/cpu/dbt/wasm/locals.rs",
 "engine/src/memory/space.rs","engine/src/process/instance.rs","engine/src/process/resident.rs","engine/src/process/wasm.rs","engine/tests/cpu_word_and_wasm.rs",
 "engine/tests/fixtures/p2-word-and/run.mjs","engine/tests/fixtures/support/engine.mjs"];
const pins=()=>Object.fromEntries(paths.map(path=>{const b=readFileSync(join(root,path));return[path,{bytes:b.length,sha256:hash(b)}];})),sourcePins=pins(),frames=[],journal=[],modules=[];
const counts={contexts:0,targets:0,matrix:0,anchors:0,retained:0,generated_calls:0,retired:0,controls:0};
writeFileSync(join(output,"engine.wasm"),engine.bytes,{flag:"wx"});
function record(magic,size,fields,version=1){const b=Buffer.alloc(size);b.write(magic);b.writeUInt16LE(version,4);b.writeUInt16LE(1,6);b.writeUInt32LE(size,8);fields.forEach((v,i)=>b.writeUInt32LE(v>>>0,16+i*4));return b;}
const words=values=>Buffer.concat(values.map(value=>{const b=Buffer.alloc(4);b.writeUInt32LE(value>>>0);return b;}));
const state=c=>record("R3ST",56,[...c.registers,c.pc,c.flags]),exit=(reason,n)=>record("R3EX",40,[reason,n,0,0,0,0]);
const arena=c=>Buffer.from(new Uint8Array(c.memory.buffer,c.base,SIZE));
function capture(c,label){const b=arena(c),row={context:c.context,label,offset:frames.length*SIZE,bytes:SIZE,sha256:hash(b)};frames.push(b);return row;}
function input(c,offset,b,label){const before=capture(c,"before input "+label),wanted=arena(c);new Uint8Array(c.memory.buffer).set(b,c.base+offset);wanted.set(b,offset);
 assert.deepEqual(arena(c),wanted);journal.push({context:c.context,name:"input",offset,hex:b.toString("hex"),label,before,after:capture(c,"after input "+label)});}
function host(c,name,args,label=name){const before=capture(c,"before "+label),wanted=arena(c);assert.equal(c.api[name](...args),0,label);const after=arena(c);
 if(name.startsWith("compile_resident")){assert.equal(after.readUInt32LE(140),1);assert.equal(after.readUInt32LE(144),24);after.subarray(140,164).copy(wanted,140);}
 assert.deepEqual(after,wanted,"exact host arena effect "+label);journal.push({context:c.context,name,args,label,before,after:capture(c,"after "+label)});}
function upload(c,address,b){input(c,140,b,"declared upload");host(c,"upload",[address,b.length]);}
function cpu(c){const b=arena(c);return{registers:Array.from({length:8},(_,i)=>b.readUInt32LE(16+i*4)),pc:b.readUInt32LE(48),flags:b.readUInt32LE(52)};}
const low=[0,0x7fff,0x8000,0xffff,0xff,0xff00,0x8001,0x1234];
function seed(c,pc,salt,flags){const b=Buffer.alloc(100),r=Array.from({length:8},(_,i)=>((0xa100+i)*0x10000+low[(i+salt)%8])>>>0);
 b.set(state({registers:r,pc,flags}));b.set(exit(3,0),56);input(c,0,b,"explicit CPU and exit seed");return cpu(c);}
function run(c,budget,label,wanted,reason=1,n=1,status=0,args){const before=capture(c,"before generated "+label),expected=arena(c);
 if(status===0){expected.set(state(wanted));expected.set(exit(reason,n),56);}const params=args??[c.base,c.base+56,budget,c.base+96];
 assert.equal(c.run(...params),status,label);assert.deepEqual(arena(c),expected,"full4364 "+label);counts.generated_calls++;if(status===0)counts.retired+=n;
 journal.push({context:c.context,name:"run",label,budget,status,args:params,before,after:capture(c,"after generated "+label)});}
function flags(result,old){let ones=0;for(let i=0;i<8;i++)ones+=(result>>>i)&1;
 return (old&0x402)|(ones%2===0?4:0)|(result===0?64:0)|(result&0x8000?128:0);}
function and(c,form,category,label){const old=cpu(c),a=old.registers[form.left]&0xffff,b=form.kind==="register"?old.registers[form.right]&0xffff:form.immediate,result=a&b;
 const registers=[...old.registers];registers[form.left]=((old.registers[form.left]&0xffff0000)|result)>>>0;
 run(c,1,label,{...old,registers,pc:old.pc+form.bytes.length,flags:flags(result,old.flags)});counts.targets++;counts[category]++;}
function compile(c,group){input(c,140,words(c.entries?[group.pc]:[group.pc,group.length]),"one block descriptors");
 const method=c.owner==="resident"?(c.entries?"compile_resident_entries":"compile_resident"):(c.entries?"compile_entries":"compile");host(c,method,c.entries?[1,0]:[1]);let binding;
 if(c.owner==="resident"){const b=arena(c);binding=Object.fromEntries(["low","high","pointer","length"].map((name,i)=>[name,b.readUInt32LE(148+i*4)]));}
 else binding={generation:c.api.generation(),pointer:c.api.module_ptr()>>>0,length:c.api.module_len()>>>0};
 const bytes=Buffer.from(new Uint8Array(c.memory.buffer,binding.pointer,binding.length)),module=new WebAssembly.Module(bytes),name=c.owner==="resident"?"guard_resident":"guard";
 assert.deepEqual(WebAssembly.Module.imports(module),[{module:"env",name:"memory",kind:"memory"},{module:"ring3",name,kind:"function"}]);assert.deepEqual(WebAssembly.Module.exports(module),[{name:"run",kind:"function"}]);
 c.run=new WebAssembly.Instance(module,{env:{memory:c.memory},ring3:c.api}).exports.run;const file=`${c.owner}-${c.entries?"entries":"explicit"}-group${groups.indexOf(group)}.wasm`;
 writeFileSync(join(output,file),bytes,{flag:"wx"});modules.push({context:c.context,owner:c.owner,entries:c.entries,group:groups.indexOf(group),arena_base:c.base,file,...binding,bytes:bytes.length,sha256:hash(bytes),imports:[name]});}
function open(owner,entries){const instance=new WebAssembly.Instance(engine.module,{}),c={owner,entries,context:++counts.contexts,memory:instance.exports.memory,api:{}};
 for(const name of ["open","close","arena_ptr","map","protect","upload","compile","compile_entries","compile_resident","compile_resident_entries","generation","module_ptr","module_len","guard","guard_resident"])c.api[name]=instance.exports["ring3_abi_v1_"+name];
 assert.equal(c.api.open(2,c.context,0x66555657),0);c.base=c.api.arena_ptr()>>>0;input(c,0,initial,"full initial arena with opaque FP128");
 host(c,"map",[PC,1,7]);upload(c,PC,bank);host(c,"protect",[PC,1,4]);return c;}
function close(c){const before=capture(c,"before close");assert.equal(c.api.close(),0);journal.push({context:c.context,name:"close",args:[],before});
 assert.equal(c.run(c.base,c.base+56,1,c.base+96),5,"closed owner status only");counts.generated_calls++;counts.controls++;journal.push({context:c.context,name:"closed-run",status:5});}
const anchors=[[0xffff,0],[0x8000,0xffff],[0x7fff,0xffff],[0x100,0xffff],[0x101,0xffff],[0xffff,0xffff],
 [0xaaaa,0x5555],[0x8001,0x8001],[0xff,0xffff],[0x1234,0x4321],[0xf0f0,0x0f0f],[1,0xffff]];
assert.deepEqual(anchors.map(([a,b])=>[a&b,flags(a&b,0xcd7)]),
 [[0,0x446],[0x8000,0x486],[0x7fff,0x406],[0x100,0x406],[0x101,0x402],[0xffff,0x486],
 [0,0x446],[0x8001,0x482],[0xff,0x406],[0x220,0x402],[0,0x446],[1,0x402]]);
for(const owner of ["replacement","resident"])for(const entries of [false,true]){const c=open(owner,entries);
 for(const group of groups.slice(0,-1)){compile(c,group);for(const form of group.rows){seed(c,form.pc,form.left+(form.right??form.immediate),(form.pc&1)?0xcd7:2);and(c,form,"matrix","literal admitted AND form");}
  if(group===groups[0])for(const [a,b] of anchors){const old=seed(c,group.rows[1].pc,a+b,0xcd7),r=[...old.registers];r[0]=(0xbeef0000|a)>>>0;r[1]=(0xdead0000|b)>>>0;
   input(c,0,state({...old,registers:r}),"discriminating width16 flag operands");and(c,group.rows[1],"anchors","width16 parity/whole-word-zero/sign and cleared carry/overflow/auxiliary anchor");}}
 compile(c,groups.at(-1));const at=groups.at(-1).pc;seed(c,at,3,0xcd7);let old=cpu(c),r=[...old.registers];r[0]=((r[0]&0xffff0000)|0xffff)>>>0;
 run(c,1,"retained MOV AXFFFF keeps dirty flags",{...old,registers:r,pc:at+4});and(c,{bytes:Buffer.alloc(5),left:0,immediate:0xff,kind:"imm16"},"retained","retained AND AX00FF clears dirty flags");
 old=cpu(c);r=[...old.registers];r[2]=((r[2]&0xffffff00)|(old.flags&1))>>>0;run(c,1,"retained SETB consumes cleared carry",{...old,registers:r,pc:at+12});
 old=cpu(c);r=[...old.registers];r[1]=((r[1]&0xffff0000)|0xff00)>>>0;run(c,1,"retained MOV CXFF00",{...old,registers:r,pc:at+16});
 and(c,{bytes:Buffer.alloc(3),left:0,right:1,kind:"register"},"retained","retained AND reads current AX and CX");
 old=cpu(c);r=[...old.registers];r[3]=((r[3]&0xffffff00)|((old.flags>>>6)&1))>>>0;run(c,1,"retained SETZ consumes whole-word zero",{...old,registers:r,pc:at+22});run(c,2,"JMP then cold padding",{...cpu(c),pc:at+24},3,1);
 seed(c,at,6,0xcd7);run(c,0,"zero budget",cpu(c),1,0);counts.controls++;input(c,96,words([1]),"explicit cancellation");run(c,1,"cancellation",cpu(c),2,0);counts.controls++;input(c,96,words([0]),"clear cancellation");
 for(const args of [[c.base+1,c.base+56,1,c.base+96],[c.base,c.base+57,1,c.base+96],[c.base,c.base+16,1,c.base+96],[c.base,c.base+56,1,c.base+16]]){
  run(c,1,"invalid or overlapping call pointers",cpu(c),1,0,1,args);counts.controls++;}
 host(c,"protect",[PC,1,7]);upload(c,PC,Buffer.from([0x66]));run(c,1,"same-byte code upload rejects stale owner",cpu(c),1,0,4);counts.controls++;close(c);
}
assert.deepEqual(pins(),sourcePins);assert.deepEqual(counts,{contexts:4,targets:472,matrix:416,anchors:48,retained:8,generated_calls:524,retired:492,controls:32});assert.equal(modules.length,20);
const hostCounts=Object.fromEntries(["map","protect","upload","compile","compile_entries","compile_resident","compile_resident_entries","close"].map(name=>[name,journal.filter(row=>row.name===name).length]));
assert.deepEqual(hostCounts,{map:4,protect:8,upload:8,compile:5,compile_entries:5,compile_resident:5,compile_resident_entries:5,close:4});
assert.equal(journal.filter(row=>row.name==="input").length,560);assert.equal(journal.length,1128);assert.equal(frames.length,2244);
const raw=Buffer.concat(frames);writeFileSync(join(output,"arenas.bin"),raw,{flag:"wx"});
const result={status:"ok",engine:{bytes:engine.bytes.length,sha256:engine.sha256,caller_sha256:process.env.RING3_ENGINE_SHA256},source_pins:sourcePins,
 counts:{...counts,modules:modules.length,host_counts:hostCounts,host_inputs:journal.filter(row=>row.name==="input").length,journal_records:journal.length,raw_frames:frames.length},modules,journal,
 raw:{file:"arenas.bin",frames:frames.length,bytes:raw.length,sha256:hash(raw)},limits:["finite exact register WORD AND21/23/25/81/83 forms in four bound profiles; standalone emission separate",
 "full4364 saved input/host/generated pairs plus preclose frames; one destination low16 result/high16 preserved; undefined AF deterministically clear, not hardware equality; no RAM helper or data memory effect","opaque initialized FP128 preserved, not arbitrary raw80 seeds",
 "module validation/imports/hash pin only; no full body or allocator history proof","open/getters and binding pointers bounded physical correspondence; closed call raw status only",
 "no full ISA, exhaustive values/FLAGS, hardware, performance, CI, browser or game claim"]};
writeFileSync(join(output,"result.json"),JSON.stringify(result,null,2),{flag:"wx"});console.log(JSON.stringify({status:result.status,...result.counts,output}));
