import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync, writeFileSync, readdirSync, lstatSync } from "node:fs";
import { join } from "node:path";
import { readEngine } from "../support/engine.mjs";
const [enginePath, output, root] = process.argv.slice(2), SIZE=4364, PC=0x1000;
assert.ok(enginePath && output && root);assert.match(process.env.RING3_ENGINE_SHA256??"",/^[a-f0-9]{64}$/);
assert.deepEqual(readdirSync(output).sort(),["initial-arena.bin","word-register-move.x86"]);
for(const name of readdirSync(output))assert.ok(lstatSync(join(output,name)).isFile()&&!lstatSync(join(output,name)).isSymbolicLink());
const engine=readEngine(enginePath),hash=b=>createHash("sha256").update(b).digest("hex");
assert.equal(engine.sha256,process.env.RING3_ENGINE_SHA256);assert.deepEqual(WebAssembly.Module.imports(engine.module),[]);
const initial=readFileSync(join(output,"initial-arena.bin"));assert.equal(initial.length,SIZE);
const bank=Buffer.alloc(512,0xcc),groups=[];
for(const opcode of [0x89,0x8b])for(let half=0;half<2;half++){
 const group=groups.length,specs=[];
 for(let slot=0;slot<4;slot++){const destination=half*4+slot,at=group*128+slot*32;
  for(let source=0;source<8;source++)bank.set([0x66,opcode,0xc0|(opcode===0x89?(source<<3)|destination:(destination<<3)|source)],at+source*3);
  bank.set([0xeb,0],at+24);specs.push({pc:PC+at,length:26,destination});}
 groups.push({opcode,half,specs});}
assert.deepEqual(bank,readFileSync(join(output,"word-register-move.x86")),"independent Rust and JS literal banks");
const paths=["engine/src/abi/arena.rs","engine/src/abi/header.rs","engine/src/abi/x86/state.rs","engine/src/abi/x86/exit.rs","engine/src/abi/x86/x87.rs",
 "engine/src/cpu/x86/ir.rs","engine/src/cpu/x86/decode/decoder.rs","engine/src/cpu/x86/decode/operands.rs","engine/src/cpu/x86/decode/lower.rs","engine/src/cpu/x86/decode/profile.rs",
 "engine/src/cpu/dbt/region.rs","engine/src/cpu/dbt/wasm/integer.rs","engine/src/cpu/dbt/wasm/emitter.rs","engine/src/cpu/dbt/wasm/abi.rs","engine/src/cpu/dbt/wasm/locals.rs",
 "engine/src/memory/space.rs","engine/src/process/instance.rs","engine/src/process/resident.rs","engine/src/process/wasm.rs","engine/tests/cpu_word_register_move_wasm.rs",
 "engine/tests/fixtures/p2-word-register-move/run.mjs","engine/tests/fixtures/support/engine.mjs"];
const pins=()=>Object.fromEntries(paths.map(path=>{const b=readFileSync(join(root,path));return[path,{bytes:b.length,sha256:hash(b)}];})),sourcePins=pins(),frames=[],journal=[],modules=[];
const counts={contexts:0,targets:0,matrix:0,retained:0,current:0,mov89:0,mov8b:0,generated_calls:0,retired:0,jumps:0,controls:0};
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
function move(c,opcode,destination,source,label,category){const old=cpu(c),next={registers:[...old.registers],pc:old.pc+3,flags:old.flags};
 next.registers[destination]=((old.registers[destination]&0xffff0000)|(old.registers[source]&0xffff))>>>0;
 run(c,1,label,next);counts.targets++;counts[opcode===0x89?"mov89":"mov8b"]++;counts[category]++;}
function compile(c,group){const starts=group.specs.map(spec=>spec.pc),lengths=group.specs.map(spec=>spec.length);
 input(c,140,words(c.entries?starts:starts.flatMap((pc,i)=>[pc,lengths[i]])),"four block descriptors");
 const method=c.owner==="resident"?(c.entries?"compile_resident_entries":"compile_resident"):(c.entries?"compile_entries":"compile");host(c,method,c.entries?[4,0]:[4]);let binding;
 if(c.owner==="resident"){const b=arena(c);binding=Object.fromEntries(["low","high","pointer","length"].map((name,i)=>[name,b.readUInt32LE(148+i*4)]));}
 else binding={generation:c.api.generation(),pointer:c.api.module_ptr()>>>0,length:c.api.module_len()>>>0};
 const bytes=Buffer.from(new Uint8Array(c.memory.buffer,binding.pointer,binding.length)),module=new WebAssembly.Module(bytes),name=c.owner==="resident"?"guard_resident":"guard";
 assert.deepEqual(WebAssembly.Module.imports(module),[{module:"env",name:"memory",kind:"memory"},{module:"ring3",name,kind:"function"}]);
 assert.deepEqual(WebAssembly.Module.exports(module),[{name:"run",kind:"function"}]);c.run=new WebAssembly.Instance(module,{env:{memory:c.memory},ring3:c.api}).exports.run;
 const file=`${c.owner}-${c.entries?"entries":"explicit"}-group${groups.indexOf(group)}.wasm`;writeFileSync(join(output,file),bytes,{flag:"wx"});
 modules.push({context:c.context,owner:c.owner,entries:c.entries,group:groups.indexOf(group),arena_base:c.base,file,...binding,bytes:bytes.length,sha256:hash(bytes),imports:[name]});}
function open(owner,entries){const instance=new WebAssembly.Instance(engine.module,{}),c={owner,entries,context:++counts.contexts,memory:instance.exports.memory,api:{}};
 for(const name of ["open","close","arena_ptr","map","protect","upload","compile","compile_entries","compile_resident","compile_resident_entries","generation","module_ptr","module_len","guard","guard_resident"])c.api[name]=instance.exports["ring3_abi_v1_"+name];
 assert.equal(c.api.open(2,c.context,0x65555657),0);c.base=c.api.arena_ptr()>>>0;input(c,0,initial,"full initial arena with opaque FP128");
 host(c,"map",[PC,1,7]);upload(c,PC,bank);host(c,"protect",[PC,1,4]);return c;}
function close(c){const before=capture(c,"before close");assert.equal(c.api.close(),0);journal.push({context:c.context,name:"close",args:[],before});
 assert.equal(c.run(c.base,c.base+56,1,c.base+96),5,"closed owner status only");counts.generated_calls++;counts.controls++;journal.push({context:c.context,name:"closed-run",status:5});}
for(const owner of ["replacement","resident"])for(const entries of [false,true]){const c=open(owner,entries);
 for(const group of groups){compile(c,group);
  for(const spec of group.specs)for(let source=0;source<8;source++){seed(c,spec.pc+source*3,spec.destination+source,(spec.destination+source)&1?0xcd7:2);
   move(c,group.opcode,spec.destination,source,"all source and destination identities","matrix");}
  const chain=group.specs[3];seed(c,chain.pc,3,0xcd7);for(let source=0;source<8;source++)move(c,group.opcode,chain.destination,source,"retained current low16 and self alias","retained");
  run(c,2,"JMP then cold padding",{...cpu(c),pc:chain.pc+26},3,1);counts.jumps++;
  const current=group.specs[0];seed(c,current.pc,5,2);move(c,group.opcode,current.destination,0,"first current source witness","current");
  input(c,20,words([0xbabe80ff]),"change only current ECX source between retained calls");move(c,group.opcode,current.destination,1,"fresh ECX low16 without full CPU reseed","current");
 }
 const last=groups[3].specs[0];seed(c,last.pc,6,0xcd7);run(c,0,"zero budget",cpu(c),1,0);counts.controls++;
 input(c,96,words([1]),"explicit cancellation");run(c,1,"cancellation",cpu(c),2,0);counts.controls++;input(c,96,words([0]),"clear cancellation");
 for(const args of [[c.base+1,c.base+56,1,c.base+96],[c.base,c.base+57,1,c.base+96],[c.base,c.base+16,1,c.base+96],[c.base,c.base+56,1,c.base+16]]){
  run(c,1,"invalid or overlapping call pointers",cpu(c),1,0,1,args);counts.controls++;}
 host(c,"protect",[PC,1,7]);upload(c,PC,Buffer.from([0x66]));run(c,1,"same-byte code upload rejects stale owner",cpu(c),1,0,4);counts.controls++;close(c);
}
assert.deepEqual(pins(),sourcePins);assert.equal(counts.contexts,4);assert.equal(modules.length,16);assert.equal(counts.targets,672);assert.equal(counts.matrix,512);
assert.equal(counts.retained,128);assert.equal(counts.current,32);assert.equal(counts.mov89,336);assert.equal(counts.mov8b,336);assert.equal(counts.generated_calls,720);
assert.equal(counts.retired,688);assert.equal(counts.jumps,16);assert.equal(counts.controls,32);
const hostCounts=Object.fromEntries(["map","protect","upload","compile","compile_entries","compile_resident","compile_resident_entries","close"].map(name=>[name,journal.filter(row=>row.name===name).length]));
assert.deepEqual(hostCounts,{map:4,protect:8,upload:8,compile:4,compile_entries:4,compile_resident:4,compile_resident_entries:4,close:4});
assert.equal(journal.filter(row=>row.name==="input").length,600);assert.equal(journal.length,1360);assert.equal(frames.length,2708);
const raw=Buffer.concat(frames);writeFileSync(join(output,"arenas.bin"),raw,{flag:"wx"});
const result={status:"ok",engine:{bytes:engine.bytes.length,sha256:engine.sha256,caller_sha256:process.env.RING3_ENGINE_SHA256},source_pins:sourcePins,
 counts:{...counts,modules:modules.length,host_counts:hostCounts,host_inputs:journal.filter(row=>row.name==="input").length,journal_records:journal.length,raw_frames:frames.length},modules,journal,
 raw:{file:"arenas.bin",frames:frames.length,bytes:raw.length,sha256:hash(raw)},limits:["finite register-only WORD MOV89/8B all64pairs each in four bound profiles; native standalone emission is separate",
 "full4364 saved input/host/generated pairs plus preclose frames; no guest RAM helper or data memory effect","opaque initialized FP128 preserved, not arbitrary raw80 seeds",
 "Module validation/imports/hash pin only; no full Wasm body or allocator history proof","open/getters and binding pointers are bounded physical correspondences; closed call raw status only",
 "no full ISA, exhaustive values, hardware, performance, CI, browser or game claim"]};
writeFileSync(join(output,"result.json"),JSON.stringify(result,null,2),{flag:"wx"});console.log(JSON.stringify({status:result.status,...result.counts,output}));
