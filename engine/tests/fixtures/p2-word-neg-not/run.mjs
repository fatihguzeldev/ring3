import assert from "node:assert/strict";
import {createHash} from "node:crypto";
import {readFileSync, writeFileSync, readdirSync, lstatSync} from "node:fs";
import {join} from "node:path";
import {readEngine} from "../support/engine.mjs";
const [enginePath, output, root] = process.argv.slice(2), SIZE=4364, PC=0x1000;
assert.ok(enginePath&&output&&root);assert.match(process.env.RING3_ENGINE_SHA256??"",/^[a-f0-9]{64}$/);
assert.deepEqual(readdirSync(output).sort(),["initial-arena.bin","word-neg-not.x86"]);
for(const name of readdirSync(output))assert.ok(lstatSync(join(output,name)).isFile()&&!lstatSync(join(output,name)).isSymbolicLink());
const engine=readEngine(enginePath),hash=b=>createHash("sha256").update(b).digest("hex");
assert.equal(engine.sha256,process.env.RING3_ENGINE_SHA256);assert.deepEqual(WebAssembly.Module.imports(engine.module),[]);
const initial=readFileSync(join(output,"initial-arena.bin"));assert.equal(initial.length,SIZE);
const forms=[],bank=Buffer.alloc(1024,0xcc);let at=0;
for(const [kind,extension] of [["neg",3],["not",2]])for(let destination=0;destination<8;destination++){
 const bytes=Buffer.from([0x66,0xf7,0xc0|(extension<<3)|destination]);forms.push({bytes,destination,kind,pc:PC+at});bank.set(bytes,at);at+=3;}
assert.equal(forms.length,16);assert.equal(at,48);bank.set([0xeb,0],at);
const chain=Buffer.from([0xf9,0x66,0xb8,0,0x80,0x66,0xf7,0xd8,0x0f,0x90,0xc1,0x0f,0x92,0xc2,0x66,0xf7,0xd0,0x0f,0x90,0xc5,0x0f,0x92,0xc6,0x66,0xbd,0,0,0x66,0xf7,0xdd,0x0f,0x94,0xc3,0x0f,0x92,0xc4,0x74,2,0x0f,0x0b,0xeb,0]);
assert.equal(chain.length,42);bank.set(chain,768);assert.deepEqual(bank,readFileSync(join(output,"word-neg-not.x86")));
const groups=[{blocks:[[PC,50]]},{blocks:[[PC+768,38],[PC+808,2]]}];
const paths=["engine/src/abi/arena.rs","engine/src/abi/header.rs","engine/src/abi/x86/state.rs","engine/src/abi/x86/exit.rs","engine/src/abi/x86/x87.rs",
 "engine/src/cpu/x86/ir.rs","engine/src/cpu/x86/decode/decoder.rs","engine/src/cpu/x86/decode/operands.rs","engine/src/cpu/x86/decode/lower.rs","engine/src/cpu/x86/decode/profile.rs","engine/src/cpu/x86/decode/integer.rs",
 "engine/src/cpu/dbt/region.rs","engine/src/cpu/dbt/wasm/integer.rs","engine/src/cpu/dbt/wasm/emitter.rs","engine/src/cpu/dbt/wasm/abi.rs","engine/src/cpu/dbt/wasm/locals.rs",
 "engine/src/memory/space.rs","engine/src/process/instance.rs","engine/src/process/resident.rs","engine/src/process/wasm.rs","engine/tests/cpu_word_neg_not_wasm.rs",
 "engine/tests/fixtures/p2-word-neg-not/run.mjs","engine/tests/fixtures/support/engine.mjs"];
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
const edges=[0,0xf,0xff,0x7fff,0x8000,0xffff],low=[0,0xf,0xff,0x7fff,0x8000,0xffff,0x1234,0x8001];
function seed(c,pc,salt,flags,destination,value){const b=Buffer.alloc(100),r=Array.from({length:8},(_,i)=>((0xa100+i+((salt&1)<<5))*0x10000+low[(i+salt)%8])>>>0);
 if(destination!==undefined)r[destination]=((r[destination]&0xffff0000)|value)>>>0;
 b.set(state({registers:r,pc,flags}));b.set(exit(3,0),56);input(c,0,b,"explicit CPU and exit seed");return cpu(c);}
function run(c,budget,label,wanted,reason=1,n=1,status=0,args){const before=capture(c,"before generated "+label),expected=arena(c);
 if(status===0){expected.set(state(wanted));expected.set(exit(reason,n),56);}const params=args??[c.base,c.base+56,budget,c.base+96];
 assert.equal(c.run(...params),status,label);assert.deepEqual(arena(c),expected,"full4364 "+label);counts.generated_calls++;if(status===0)counts.retired+=n;
 journal.push({context:c.context,name:"run",label,budget,status,args:params,before,after:capture(c,"after generated "+label)});}
function negFlags(a,result,old){let ones=0;for(let i=0;i<8;i++)ones+=(result>>>i)&1;
 return(old&0x402)|Number(a!==0)|(ones%2===0?4:0)|((a^result)&16)|(result===0?64:0)|(result&0x8000?128:0)|(a===0x8000?0x800:0);}
function unary(c,form,category,label){const old=cpu(c),a=old.registers[form.destination]&0xffff,result=form.kind==="neg"?(-a)&0xffff:a^0xffff,registers=[...old.registers];
 registers[form.destination]=((old.registers[form.destination]&0xffff0000)|result)>>>0;
 run(c,1,label,{...old,registers,pc:old.pc+3,flags:form.kind==="neg"?negFlags(a,result,old.flags):old.flags});counts.targets++;counts[category]++;}
function compile(c,group){input(c,140,words(group.blocks.flatMap(([pc,length])=>c.entries?[pc]:[pc,length])),"block descriptors");
 const method=c.owner==="resident"?(c.entries?"compile_resident_entries":"compile_resident"):(c.entries?"compile_entries":"compile");host(c,method,c.entries?[group.blocks.length,0]:[group.blocks.length]);let binding;
 if(c.owner==="resident"){const b=arena(c);binding=Object.fromEntries(["low","high","pointer","length"].map((name,i)=>[name,b.readUInt32LE(148+i*4)]));}
 else binding={generation:c.api.generation(),pointer:c.api.module_ptr()>>>0,length:c.api.module_len()>>>0};
 const bytes=Buffer.from(new Uint8Array(c.memory.buffer,binding.pointer,binding.length)),module=new WebAssembly.Module(bytes),name=c.owner==="resident"?"guard_resident":"guard";
 assert.deepEqual(WebAssembly.Module.imports(module),[{module:"env",name:"memory",kind:"memory"},{module:"ring3",name,kind:"function"}]);assert.deepEqual(WebAssembly.Module.exports(module),[{name:"run",kind:"function"}]);
 c.run=new WebAssembly.Instance(module,{env:{memory:c.memory},ring3:c.api}).exports.run;const file=`${c.owner}-${c.entries?"entries":"explicit"}-group${groups.indexOf(group)}.wasm`;
 writeFileSync(join(output,file),bytes,{flag:"wx"});modules.push({context:c.context,owner:c.owner,entries:c.entries,group:groups.indexOf(group),arena_base:c.base,file,...binding,bytes:bytes.length,sha256:hash(bytes),imports:[name]});}
function open(owner,entries){const instance=new WebAssembly.Instance(engine.module,{}),c={owner,entries,context:++counts.contexts,memory:instance.exports.memory,api:{}};
 for(const name of ["open","close","arena_ptr","map","protect","upload","compile","compile_entries","compile_resident","compile_resident_entries","generation","module_ptr","module_len","guard","guard_resident"])c.api[name]=instance.exports["ring3_abi_v1_"+name];
 assert.equal(c.api.open(2,c.context,0x66555661),0);c.base=c.api.arena_ptr()>>>0;input(c,0,initial,"full initial arena with opaque FP128");
 host(c,"map",[PC,1,7]);upload(c,PC,bank);host(c,"protect",[PC,1,4]);return c;}
function close(c){const before=capture(c,"before close");assert.equal(c.api.close(),0);journal.push({context:c.context,name:"close",args:[],before});
 assert.equal(c.run(c.base,c.base+56,1,c.base+96),5,"closed owner status only");counts.generated_calls++;counts.controls++;journal.push({context:c.context,name:"closed-run",status:5});}
assert.deepEqual(edges.map(a=>{const r=(-a)&0xffff;return[r,negFlags(a,r,0xcd7)];}),[[0,0x446],[0xfff1,0x493],[0xff01,0x493],[0x8001,0x493],[0x8000,0xc87],[1,0x413]]);
assert.deepEqual(edges.map(a=>a^0xffff),[0xffff,0xfff0,0xff00,0x8000,0x7fff,0]);
for(const owner of ["replacement","resident"])for(const entries of [false,true]){const c=open(owner,entries);compile(c,groups[0]);
 for(const form of forms)for(const cf of [0,1]){const value=edges[(form.destination+(form.kind==="not"?2:0))%6];
  seed(c,form.pc,form.destination+cf,((form.destination&1)?0xcd6:2)|cf,form.destination,value);unary(c,form,"matrix","current low16 unary and high16 preservation");}
 for(const form of forms.filter(form=>form.destination===0))for(const value of edges)for(const cf of [0,1]){
  seed(c,form.pc,value+cf,0xcd6|cf,0,value);unary(c,form,"anchors","literal NEG width16 flags or exact NOT dirty FLAGS");}
 compile(c,groups[1]);const start=PC+768;seed(c,start,3,0xcd6);
 let old=cpu(c);run(c,1,"retained STC",{...old,pc:start+1,flags:old.flags|1});old=cpu(c);let r=[...old.registers];r[0]=((r[0]&0xffff0000)|0x8000)>>>0;
 run(c,1,"retained MOV AX8000",{...old,registers:r,pc:start+5});unary(c,{destination:0,kind:"neg"},"retained","retained NEG AX sets OF and CF");
 old=cpu(c);r=[...old.registers];r[1]=((r[1]&0xffffff00)|((old.flags>>>11)&1))>>>0;run(c,1,"retained SETO CL",{...old,registers:r,pc:start+11});
 old=cpu(c);r=[...old.registers];r[2]=((r[2]&0xffffff00)|(old.flags&1))>>>0;run(c,1,"retained SETB DL",{...old,registers:r,pc:start+14});
 unary(c,{destination:0,kind:"not"},"retained","retained NOT AX preserves NEG flags despite result7FFF");old=cpu(c);r=[...old.registers];r[1]=((r[1]&0xffff00ff)|(((old.flags>>>11)&1)<<8))>>>0;
 run(c,1,"retained SETO CH consumes NOT-preserved OF",{...old,registers:r,pc:start+20});old=cpu(c);r=[...old.registers];r[2]=((r[2]&0xffff00ff)|((old.flags&1)<<8))>>>0;
 run(c,1,"retained SETB DH consumes NOT-preserved CF",{...old,registers:r,pc:start+23});old=cpu(c);r=[...old.registers];r[5]=(r[5]&0xffff0000)>>>0;
 run(c,1,"retained MOV BP0",{...old,registers:r,pc:start+27});unary(c,{destination:5,kind:"neg"},"retained","retained NEG current BP0 clears previous CF and sets ZF");
 old=cpu(c);r=[...old.registers];r[3]=((r[3]&0xffffff00)|((old.flags>>>6)&1))>>>0;run(c,1,"retained SETZ BL",{...old,registers:r,pc:start+33});
 old=cpu(c);r=[...old.registers];r[0]=((r[0]&0xffff00ff)|((old.flags&1)<<8))>>>0;run(c,1,"retained SETB AH",{...old,registers:r,pc:start+36});
 old=cpu(c);assert.equal(old.flags&0x40,0x40);run(c,1,"retained JZ skips UD2",{...old,pc:start+40});run(c,2,"JMP then cold padding",{...cpu(c),pc:start+42},3,1);
 seed(c,start,6,0xcd7);run(c,0,"zero budget",cpu(c),1,0);counts.controls++;input(c,96,words([1]),"explicit cancellation");run(c,1,"cancellation",cpu(c),2,0);counts.controls++;input(c,96,words([0]),"clear cancellation");
 for(const args of [[c.base+1,c.base+56,1,c.base+96],[c.base,c.base+57,1,c.base+96],[c.base,c.base+16,1,c.base+96],[c.base,c.base+56,1,c.base+16]]){
  run(c,1,"invalid or overlapping call pointers",cpu(c),1,0,1,args);counts.controls++;}
 host(c,"protect",[PC,1,7]);upload(c,PC,Buffer.from([0x66]));run(c,1,"same-byte code upload rejects stale owner",cpu(c),1,0,4);counts.controls++;close(c);
}
assert.deepEqual(pins(),sourcePins);assert.deepEqual(counts,{contexts:4,targets:236,matrix:128,anchors:96,retained:12,generated_calls:312,retired:280,controls:32});assert.equal(modules.length,8);
const hostCounts=Object.fromEntries(["map","protect","upload","compile","compile_entries","compile_resident","compile_resident_entries","close"].map(name=>[name,journal.filter(row=>row.name===name).length]));
assert.deepEqual(hostCounts,{map:4,protect:8,upload:8,compile:2,compile_entries:2,compile_resident:2,compile_resident_entries:2,close:4});
assert.equal(journal.filter(row=>row.name==="input").length,260);assert.equal(journal.length,604);assert.equal(frames.length,1196);
const raw=Buffer.concat(frames);writeFileSync(join(output,"arenas.bin"),raw,{flag:"wx"});
const result={status:"ok",engine:{bytes:engine.bytes.length,sha256:engine.sha256,caller_sha256:process.env.RING3_ENGINE_SHA256},source_pins:sourcePins,
 counts:{...counts,modules:modules.length,host_counts:hostCounts,host_inputs:journal.filter(row=>row.name==="input").length,journal_records:journal.length,raw_frames:frames.length},modules,journal,
 raw:{file:"arenas.bin",frames:frames.length,bytes:raw.length,sha256:hash(raw)},limits:["finite exact register WORD NEG/NOT in four bound profiles; standalone emission separate",
 "full4364 saved input/host/generated pairs plus preclose frames; current low16/high16 preservation/defined NEG flags and exact admitted NOT FLAGS; no stack or RAM helper effect, including SP",
 "two resident units/context, no release or expired module pointer use; retained NEG/NOT SETcc/JZ has no interphase host reseed",
 "opaque initialized FP128 preserved, not arbitrary raw80 seeds; module validation/imports/hash only, no full body or allocator history proof",
 "open/getters and binding pointers bounded physical correspondence; closed call raw status only; no exhaustive values/FLAGS/full ISA/hardware/performance/CI/browser/game claim"]};
writeFileSync(join(output,"result.json"),JSON.stringify(result,null,2),{flag:"wx"});console.log(JSON.stringify({status:result.status,...result.counts,output}));
