import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { readEngine } from "../support/engine.mjs";
const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root); assert.match(process.env.RING3_ENGINE_SHA256 ?? "", /^[a-f0-9]{64}$/);
const engine = readEngine(enginePath), SIZE = 4364, PC = 0x1000, SRC = 0x5000;
const hash = b => createHash("sha256").update(b).digest("hex");
const initial = readFileSync(join(output, "initial-arena.bin")); assert.equal(initial.length, SIZE);
const bank = Buffer.alloc(64, 0xcc); for (const [at, code] of [[0,[0xad,0xeb,0]], [16,[0xbe,8,0x70,0,0,0xad,0xeb,0]], [32,[0xad,0xad,0xeb,0]]]) bank.set(code,at);
assert.deepEqual(bank,readFileSync(join(output,"lodsd.x86")),"independent Rust/JS literal bank");
const sourcePaths = ["engine/src/abi/arena.rs","engine/src/abi/memory_helper.rs","engine/src/abi/x86/state.rs","engine/src/abi/x86/exit.rs","engine/src/abi/x86/x87.rs",
 "engine/src/cpu/x86/ir.rs","engine/src/cpu/x86/decode/lower.rs","engine/src/cpu/x86/decode/profile.rs","engine/src/cpu/dbt/region.rs",
 "engine/src/cpu/dbt/wasm/integer.rs","engine/src/cpu/dbt/wasm/memory.rs","engine/src/cpu/dbt/wasm/emitter.rs","engine/src/cpu/dbt/wasm/locals.rs",
 "engine/src/memory/space.rs","engine/src/process/instance.rs","engine/src/process/resident.rs","engine/tests/cpu_dword_load_string_wasm.rs",
 "engine/tests/fixtures/p2-dword-load-string/run.mjs","engine/tests/fixtures/support/engine.mjs"];
const pins = () => Object.fromEntries(sourcePaths.map(path=>{const b=readFileSync(join(root,path));return [path,{bytes:b.length,sha256:hash(b)}];}));
const sourcePins = pins(), frames=[], journal=[], modules=[];
let contexts=0, generated=0, retired=0, loads=0, faults=0;
writeFileSync(join(output,"engine.wasm"),engine.bytes,{flag:"wx"});
function record(magic,size,fields,version=1){const b=Buffer.alloc(size);b.write(magic);b.writeUInt16LE(version,4);b.writeUInt16LE(1,6);b.writeUInt32LE(size,8);fields.forEach((v,i)=>b.writeUInt32LE(v>>>0,16+i*4));return b;}
const words=values=>Buffer.concat(values.map(v=>{const b=Buffer.alloc(4);b.writeUInt32LE(v>>>0);return b;}));
const state=c=>record("R3ST",56,[...c.registers,c.pc,c.flags]);
const exit=(reason,count,f)=>record("R3EX",40,[reason,count,f?.detail??0,f?.address??0,f?1:0,f?4:0],2);
const helper=(value=0,f)=>record("R3MH",40,[f?1:0,f?0:value,f?.detail??0,f?.address??0,f?1:0,f?4:0]);
const arena=c=>Buffer.from(new Uint8Array(c.memory.buffer,c.base,SIZE));
function capture(c,label){const b=arena(c),row={context:c.ordinal,owner:c.owner,entries:c.entries,label,offset:frames.length*SIZE,bytes:SIZE,sha256:hash(b)};frames.push(b);return row;}
function input(c,offset,b,label){const before=capture(c,"before input "+label);new Uint8Array(c.memory.buffer).set(b,c.base+offset);journal.push({context:c.ordinal,owner:c.owner,entries:c.entries,name:"input",offset,hex:b.toString("hex"),label,before,after:capture(c,"after input "+label)});}
function host(c,name,args,label=name){const before=capture(c,"before "+label),old=arena(c);assert.equal(c.api[name](...args),0,label);const after=arena(c),wanted=Buffer.from(old);
 if(name.startsWith("compile_resident")){assert.equal(after.readUInt32LE(140),1);assert.equal(after.readUInt32LE(144),24);after.subarray(140,164).copy(wanted,140);}
 else if(name==="read32")after.subarray(100,140).copy(wanted,100);
 assert.deepEqual(after,wanted,"host exact arena effect "+label);
 journal.push({context:c.ordinal,owner:c.owner,entries:c.entries,name,args,label,before,after:capture(c,"after "+label)});}
function upload(c,address,b){input(c,140,b,"declared upload");host(c,"upload",[address,b.length]);}
function cpu(c){const b=arena(c);return {registers:Array.from({length:8},(_,i)=>b.readUInt32LE(16+i*4)),pc:b.readUInt32LE(48),flags:b.readUInt32LE(52)};}
function seed(c,pc,flags,source=SRC+8){const r=[0xa5812347,0x91b2c378,0x34560f10,0x4567ff00,0x56789abc,0x6789abcd,source,0x8008],b=Buffer.alloc(140);b.set(state({registers:r,pc,flags}));b.set(exit(3,0),56);b.set(helper(0xdecafbad),100);input(c,0,b,"explicit CPU seed");return cpu(c);}
function diagnostic(c,address,value){host(c,"read32",[address],"declared current DWORD RAM");assert.deepEqual(arena(c).subarray(100,140),helper(value),"canonical diagnostic equals declared literal");}
function run(c,budget,label,wanted,reason=1,count=1,f,raw=0,value,params){const before=capture(c,"before generated "+label),expected=arena(c);
 if(raw===0){expected.set(state(wanted));expected.set(exit(reason,count,f),56);if(f||value!==undefined)expected.set(helper(value??0,f),100);}
 const args=params??[c.base,c.base+56,budget,c.base+96];assert.equal(c.run(...args),raw,label);assert.deepEqual(arena(c),expected,"full4364 "+label);
 generated++;if(raw===0)retired+=count;if(f)faults++;if(value!==undefined)loads++;
 journal.push({context:c.ordinal,owner:c.owner,entries:c.entries,name:"run",label,budget,status:raw,args,before,after:capture(c,"after generated "+label)});}
function load(c,label,value){const before=cpu(c),next={registers:[...before.registers],pc:(before.pc+1)>>>0,flags:before.flags};next.registers[0]=value>>>0;next.registers[6]=(before.registers[6]+(before.flags&0x400?-4:4))>>>0;run(c,1,label,next,1,1,undefined,0,value);}
function open(owner,entries){const instance=new WebAssembly.Instance(engine.module,{}),c={owner,entries,ordinal:++contexts,memory:instance.exports.memory,api:{}};
 for(const name of ["open","close","arena_ptr","map","unmap","protect","upload","read32","compile","compile_entries","compile_resident","compile_resident_entries","generation","module_ptr","module_len","guard","guard_resident"])c.api[name]=instance.exports["ring3_abi_v1_"+name];
 assert.equal(c.api.open(8,c.ordinal,0x51525354),0);c.base=c.api.arena_ptr()>>>0;input(c,0,initial,"full initial arena including opaque128 FP");
 for(const args of [[PC,1,7],[SRC,2,3],[0xfffff000,1,3],[0,1,3]])host(c,"map",args);
 upload(c,PC,bank);const starts=[PC,PC+16,PC+32],lengths=[3,8,4];input(c,140,words(entries?starts:starts.flatMap((pc,i)=>[pc,lengths[i]])),"three block descriptors");
 const method=owner==="resident"?(entries?"compile_resident_entries":"compile_resident"):(entries?"compile_entries":"compile");host(c,method,entries?[3,0]:[3]);
 let binding;if(owner==="resident"){const b=arena(c);binding=Object.fromEntries(["low","high","pointer","length"].map((name,i)=>[name,b.readUInt32LE(148+i*4)]));}
 else binding={generation:c.api.generation(),pointer:c.api.module_ptr()>>>0,length:c.api.module_len()>>>0};
 const bytes=Buffer.from(new Uint8Array(c.memory.buffer,binding.pointer,binding.length)),module=new WebAssembly.Module(bytes),names=[owner==="resident"?"guard_resident":"guard","read32"];
 assert.deepEqual(WebAssembly.Module.imports(module),[{module:"env",name:"memory",kind:"memory"},...names.map(name=>({module:"ring3",name,kind:"function"}))]);
 c.run=new WebAssembly.Instance(module,{env:{memory:c.memory},ring3:c.api}).exports.run;const file=`${owner}-${entries?"entries":"explicit"}.wasm`;
 writeFileSync(join(output,file),bytes,{flag:"wx"});modules.push({context:c.ordinal,owner,entries,file,...binding,bytes:bytes.length,sha256:hash(bytes),imports:names});return c;}
function close(c){const before=capture(c,"before close");assert.equal(c.api.close(),0);journal.push({context:c.ordinal,owner:c.owner,entries:c.entries,name:"close",args:[],before});
 assert.equal(c.run(c.base,c.base+56,1,c.base+96),5,"closed owner status only");generated++;journal.push({context:c.ordinal,owner:c.owner,entries:c.entries,name:"closed-run",status:5,label:"retained arena not inspected after close"});}
for(const owner of ["replacement","resident"])for(const entries of [false,true]){const c=open(owner,entries);
 for(const flags of [2,0xcd7])for(const value of [0,0xffffffff,0x80000000,0x1234a581])for(const offset of [8,9,4094]){upload(c,SRC+offset,words([value]));diagnostic(c,SRC+offset,value);seed(c,PC,flags,SRC+offset);load(c,"literal full EAX unaligned/page-crossing",value);}
 for(const [pointer,flags]of [[0xfffffffc,2],[0,0xcd7]]){upload(c,pointer,words([0x8191a6b7]));diagnostic(c,pointer,0x8191a6b7);seed(c,PC,flags,pointer);load(c,"valid read then modulo32 pointer wrap",0x8191a6b7);}
 const failures=[{source:0x7008,detail:1,address:0x7008},{source:0x7008,detail:2,address:0x7008,map:true},
 {source:0x6ffe,detail:1,address:0x7000},{source:0x6ffe,detail:2,address:0x7000,map:true},
 ...[0xfffffffd,0xfffffffe,0xffffffff].map(source=>({source,detail:3,address:source}))];
 for(const [i,f]of failures.entries()){if(f.map)host(c,"map",[0x7000,1,2]);if(f.source===0x7008&&f.map)upload(c,f.source,words([1]));if(f.source===0x6ffe)upload(c,f.source,Buffer.from([0x31,0x55]));
 const before=seed(c,PC,0xcd7,f.source);run(c,1,"read4 fault",before,5,0,f);run(c,1,"same CPU retained fault",before,5,0,f);
 if(i<4){if(f.detail===1)host(c,"map",[0x7000,1,3]);else host(c,"protect",[0x7000,1,3]);upload(c,f.source,words([0x8172a691]));diagnostic(c,f.source,0x8172a691);
 assert.deepEqual(cpu(c),before,"RAM repair preserves current CPU");load(c,"same CPU repair rereads current RAM",0x8172a691);host(c,"unmap",[0x7000,1]);}}
 const seeded=seed(c,PC+16,2),prefixed={registers:[...seeded.registers],pc:PC+21,flags:2};prefixed.registers[6]=0x7008;const f={detail:1,address:0x7008};
 run(c,2,"retired MOVESI then precise LODSD fault",prefixed,5,1,f);run(c,1,"retained prefix repeat fault",prefixed,5,0,f);
 host(c,"map",[0x7000,1,3]);upload(c,0x7008,words([0xcafebabe]));diagnostic(c,0x7008,0xcafebabe);assert.deepEqual(cpu(c),prefixed);load(c,"retained current ESI repaired RAM",0xcafebabe);
 run(c,2,"prefix tail JMP then cold",{...cpu(c),pc:PC+24},3,1);host(c,"unmap",[0x7000,1]);
 upload(c,SRC+8,words([0x8172a691,0x1234a581]));diagnostic(c,SRC+8,0x8172a691);seed(c,PC+32,2);load(c,"first retained chain DWORD",0x8172a691);
 const held=cpu(c);upload(c,SRC+12,words([0xa5ff91e3]));diagnostic(c,SRC+12,0xa5ff91e3);assert.deepEqual(cpu(c),held);load(c,"second retained load consumes changed current RAM",0xa5ff91e3);
 run(c,2,"chain JMP then cold",{...cpu(c),pc:PC+36},3,1);
 seed(c,PC,2,0x7008);run(c,0,"zero budget avoids unmapped source",cpu(c),1,0);input(c,96,words([1]),"cancellation");run(c,1,"cancel avoids unmapped source",cpu(c),2,0);
 for(const params of [[c.base+1,c.base+56,1,c.base+96],[c.base,c.base+57,1,c.base+96]])run(c,1,"invalid pointer preflight before helpers",cpu(c),1,0,undefined,1,undefined,params);
 seed(c,PC,2,0x7008);upload(c,PC,Buffer.from([0xad]));run(c,1,"same-byte consumed code mutation rejects stale owner",cpu(c),1,0,undefined,4);close(c);
}
assert.deepEqual(pins(),sourcePins,"all19 source bytes unchanged");assert.equal(contexts,4);assert.equal(modules.length,4);assert.equal(loads,132);assert.equal(faults,64);assert.equal(generated,228);assert.equal(retired,144);
const hostCounts=Object.fromEntries(["map","unmap","protect","upload","read32","compile","compile_entries","compile_resident","compile_resident_entries"].map(name=>[name,journal.filter(r=>r.name===name).length]));
assert.deepEqual(hostCounts,{map:36,unmap:20,protect:8,upload:152,read32:132,compile:1,compile_entries:1,compile_resident:1,compile_resident_entries:1});
assert.equal(journal.filter(r=>r.name==="input").length,312);assert.equal(journal.length,896);assert.equal(frames.length,1780);
const raw=Buffer.concat(frames);writeFileSync(join(output,"arenas.bin"),raw,{flag:"wx"});
const result={status:"ok",engine:{bytes:engine.bytes.length,sha256:engine.sha256,caller_sha256:process.env.RING3_ENGINE_SHA256},source_pins:sourcePins,
 counts:{contexts,modules:modules.length,core_loads:96,endpoint_loads:8,loads,fault_calls:faults,generated_calls:generated,retired,raw_frames:frames.length,host_inputs:312,journal_records:896,host_counts:hostCounts},
 modules,journal,raw:{file:"arenas.bin",bytes:raw.length,sha256:hash(raw)},limits:["bare flat32 LODSD finite literals/DF/read4/fault/retained current RAM only",
 "full saved4364 arenas plus declared uploads and selected Read32 diagnostics; no whole RAM dump or generic Wasm-body certification",
 "read fault and repaired prefix retain current CPU; current source RAM explicitly changes without reseed",
 "closed outcomes status-only; retained arena not inspected after close","Module-only native checks and actual generated execution are separate proofs"]};
writeFileSync(join(output,"result.json"),JSON.stringify(result,null,2),{flag:"wx"});console.log(JSON.stringify({status:result.status,...result.counts,output}));
