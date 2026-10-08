import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { readEngine } from "../support/engine.mjs";
const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root); assert.match(process.env.RING3_ENGINE_SHA256 ?? "", /^[a-f0-9]{64}$/);
const engine = readEngine(enginePath), SIZE = 4364, PC = 0x1000, RAM = 0x5000;
const hash = b => createHash("sha256").update(b).digest("hex");
const initial = readFileSync(join(output,"initial-arena.bin")); assert.equal(initial.length,SIZE);
const bank = Buffer.alloc(64,0xcc); for(const [at,code]of [[0,[0x9c,0xeb,0]],[16,[0xbc,0x0c,0x70,0,0,0x9c,0xeb,0]],[32,[0xf9,0x9c,0x58,0xf8,0x9c,0x5a,0xeb,0]]])bank.set(code,at);
assert.deepEqual(bank,readFileSync(join(output,"pushfd.x86")),"independent Rust/JS literal bank");
const sourcePaths=["engine/src/abi/arena.rs","engine/src/abi/memory_helper.rs","engine/src/abi/x86/state.rs","engine/src/abi/x86/exit.rs","engine/src/abi/x86/x87.rs",
 "engine/src/cpu/x86/ir.rs","engine/src/cpu/x86/decode/flow.rs","engine/src/cpu/x86/decode/profile.rs","engine/src/cpu/dbt/region.rs",
 "engine/src/cpu/dbt/wasm/integer.rs","engine/src/cpu/dbt/wasm/memory.rs","engine/src/cpu/dbt/wasm/emitter.rs","engine/src/cpu/dbt/wasm/locals.rs",
 "engine/src/memory/space.rs","engine/src/process/instance.rs","engine/src/process/resident.rs","engine/src/process/wasm.rs",
 "engine/tests/cpu_push_flags_wasm.rs","engine/tests/fixtures/p2-push-flags/run.mjs","engine/tests/fixtures/support/engine.mjs"];
const pins=()=>Object.fromEntries(sourcePaths.map(path=>{const b=readFileSync(join(root,path));return[path,{bytes:b.length,sha256:hash(b)}];}));
const sourcePins=pins(),frames=[],journal=[],modules=[];let contexts=0,generated=0,retired=0,pushes=0,faults=0;
writeFileSync(join(output,"engine.wasm"),engine.bytes,{flag:"wx"});
function record(magic,size,fields,version=1){const b=Buffer.alloc(size);b.write(magic);b.writeUInt16LE(version,4);b.writeUInt16LE(1,6);b.writeUInt32LE(size,8);fields.forEach((v,i)=>b.writeUInt32LE(v>>>0,16+i*4));return b;}
const words=values=>Buffer.concat(values.map(v=>{const b=Buffer.alloc(4);b.writeUInt32LE(v>>>0);return b;}));
const state=c=>record("R3ST",56,[...c.registers,c.pc,c.flags]);
const exit=(reason,count,f)=>record("R3EX",40,[reason,count,f?.detail??0,f?.address??0,f?2:0,f?4:0],2);
const helper=(value=0,f)=>record("R3MH",40,[f?1:0,f?0:value,f?.detail??0,f?.address??0,f?2:0,f?4:0]);
const arena=c=>Buffer.from(new Uint8Array(c.memory.buffer,c.base,SIZE));
function capture(c,label){const b=arena(c),row={context:c.ordinal,owner:c.owner,entries:c.entries,label,offset:frames.length*SIZE,bytes:SIZE,sha256:hash(b)};frames.push(b);return row;}
function input(c,offset,b,label){const before=capture(c,"before input "+label);new Uint8Array(c.memory.buffer).set(b,c.base+offset);journal.push({context:c.ordinal,owner:c.owner,entries:c.entries,name:"input",offset,hex:b.toString("hex"),label,before,after:capture(c,"after input "+label)});}
function host(c,name,args,label=name){const before=capture(c,"before "+label),old=arena(c);assert.equal(c.api[name](...args),0,label);const after=arena(c),wanted=Buffer.from(old);
 if(name.startsWith("compile_resident")){assert.equal(after.readUInt32LE(140),1);assert.equal(after.readUInt32LE(144),24);after.subarray(140,164).copy(wanted,140);}
 else if(name==="read32")after.subarray(100,140).copy(wanted,100);
 assert.deepEqual(after,wanted,"host exact arena effect "+label);journal.push({context:c.ordinal,owner:c.owner,entries:c.entries,name,args,label,before,after:capture(c,"after "+label)});}
function upload(c,address,b){input(c,140,b,"declared upload");host(c,"upload",[address,b.length]);}
function cpu(c){const b=arena(c);return{registers:Array.from({length:8},(_,i)=>b.readUInt32LE(16+i*4)),pc:b.readUInt32LE(48),flags:b.readUInt32LE(52)};}
function seed(c,pc,flags,esp=RAM+0x100){const r=[0xa5812347,0x91b2c378,0x34560f10,0x4567ff00,esp,0x6789abcd,0x789a1234,0x89abcdef],b=Buffer.alloc(140);b.set(state({registers:r,pc,flags}));b.set(exit(3,0),56);b.set(helper(0xdecafbad),100);input(c,0,b,"explicit CPU seed");return cpu(c);}
function diagnostic(c,address,value){host(c,"read32",[address],"declared current DWORD RAM");assert.deepEqual(arena(c).subarray(100,140),helper(value),"RAM diagnostic equals declared expected DWORD");}
function run(c,budget,label,wanted,reason=1,count=1,f,raw=0,helperValue,params){const before=capture(c,"before generated "+label),expected=arena(c);
 if(raw===0){expected.set(state(wanted));expected.set(exit(reason,count,f),56);if(f||helperValue!==undefined)expected.set(helper(helperValue??0,f),100);}
 const args=params??[c.base,c.base+56,budget,c.base+96];assert.equal(c.run(...args),raw,label);assert.deepEqual(arena(c),expected,"full4364 "+label);
 generated++;if(raw===0)retired+=count;if(f)faults++;journal.push({context:c.ordinal,owner:c.owner,entries:c.entries,name:"run",label,budget,status:raw,args,before,after:capture(c,"after generated "+label)});}
function push(c,label,reason=1){const old=cpu(c),next={registers:[...old.registers],pc:(old.pc+1)>>>0,flags:old.flags};next.registers[4]=(old.registers[4]-4)>>>0;
 run(c,1,label,next,reason,1,undefined,0,0);pushes++;if(reason===1)diagnostic(c,next.registers[4],old.flags&0x00fcffff);return next;}
function open(owner,entries,mode){const instance=new WebAssembly.Instance(engine.module,{}),c={owner,entries,mode,ordinal:++contexts,memory:instance.exports.memory,api:{}};
 for(const name of ["open","close","arena_ptr","map","unmap","protect","upload","read32","compile","compile_entries","compile_resident","compile_resident_entries","generation","module_ptr","module_len","guard","guard_resident","store32","store_resident32"])c.api[name]=instance.exports["ring3_abi_v1_"+name];
 assert.equal(c.api.open(8,c.ordinal,0x52535455),0);c.base=c.api.arena_ptr()>>>0;input(c,0,initial,"full initial arena including opaque128 FP");
 for(const args of [[PC,1,7],[RAM,2,3],[0xfffff000,1,3],[0,1,3]])host(c,"map",args);
 upload(c,PC,bank);const starts=[PC,PC+16,PC+32],lengths=[3,8,8];input(c,140,words(entries?starts:starts.flatMap((pc,i)=>[pc,lengths[i]])),"three block descriptors");
 const method=owner==="resident"?(entries?"compile_resident_entries":"compile_resident"):(entries?"compile_entries":"compile");host(c,method,entries?[3,0]:[3]);
 let binding;if(owner==="resident"){const b=arena(c);binding=Object.fromEntries(["low","high","pointer","length"].map((name,i)=>[name,b.readUInt32LE(148+i*4)]));}
 else binding={generation:c.api.generation(),pointer:c.api.module_ptr()>>>0,length:c.api.module_len()>>>0};
 const bytes=Buffer.from(new Uint8Array(c.memory.buffer,binding.pointer,binding.length)),module=new WebAssembly.Module(bytes),names=[owner==="resident"?"guard_resident":"guard","read32",owner==="resident"?"store_resident32":"store32"];
 assert.deepEqual(WebAssembly.Module.imports(module),[{module:"env",name:"memory",kind:"memory"},...names.map(name=>({module:"ring3",name,kind:"function"}))]);
 c.run=new WebAssembly.Instance(module,{env:{memory:c.memory},ring3:c.api}).exports.run;const file=`${mode}-${owner}-${entries?"entries":"explicit"}.wasm`;
 writeFileSync(join(output,file),bytes,{flag:"wx"});modules.push({context:c.ordinal,owner,entries,mode,file,...binding,bytes:bytes.length,sha256:hash(bytes),imports:names});return c;}
function close(c){host(c,"close",[],"close retains arena allocation");run(c,1,"closed owner preserves retained arena",cpu(c),1,0,undefined,5);}
for(const owner of ["replacement","resident"])for(const entries of [false,true]){const c=open(owner,entries,"core");
 for(const flags of [2,3,6,0x12,0x42,0x82,0x402,0x802,0xcd7])for(const destination of [RAM+8,RAM+9,RAM+4094,0,0xfffffffc]){upload(c,destination,words([0xa5917253]));diagnostic(c,destination,0xa5917253);seed(c,PC,flags,(destination+4)>>>0);push(c,"current FLAGS image aligned/unaligned/endpoints");}
 const failures=[{destination:0x7008,detail:1,address:0x7008},{destination:0x7008,detail:2,address:0x7008,map:true},
 {destination:0x6ffe,detail:1,address:0x7000},{destination:0x6ffe,detail:2,address:0x7000,map:true},
 ...[0xfffffffd,0xfffffffe,0xffffffff].map(destination=>({destination,detail:3,address:destination}))];
 for(const [i,f]of failures.entries()){upload(c,0x6ffc,words([0x46372819]));if(f.map){host(c,"map",[0x7000,1,3]);upload(c,0x7000,words([0x88776655]));upload(c,0x7008,words([0xaabbccdd]));host(c,"protect",[0x7000,1,1]);}
 const held=seed(c,PC,0xcd7,(f.destination+4)>>>0);run(c,1,"checked store4 fault before ESP/retire",held,5,0,f);run(c,1,"same CPU retained store fault",held,5,0,f);
 diagnostic(c,0x6ffc,0x46372819);if(f.map){diagnostic(c,0x7000,0x88776655);diagnostic(c,0x7008,0xaabbccdd);}assert.deepEqual(cpu(c),held);
 if(i<4){if(f.detail===1)host(c,"map",[0x7000,1,3]);else host(c,"protect",[0x7000,1,3]);upload(c,f.destination,words([0x8192a5b6]));diagnostic(c,f.destination,0x8192a5b6);assert.deepEqual(cpu(c),held,"RAM repair preserves current CPU");push(c,"same CPU repaired destination gets current FLAGS");host(c,"unmap",[0x7000,1]);}}
 const seeded=seed(c,PC+16,0xcd7),prefixed={registers:[...seeded.registers],pc:PC+21,flags:seeded.flags};prefixed.registers[4]=0x700c;const f={detail:1,address:0x7008};
 run(c,2,"retired MOVESP then precise PUSHFD fault",prefixed,5,1,f);run(c,1,"retained prefix repeat fault",prefixed,5,0,f);
 host(c,"map",[0x7000,1,3]);upload(c,0x7008,words([0x11223344]));diagnostic(c,0x7008,0x11223344);assert.deepEqual(cpu(c),prefixed);push(c,"retained current ESP repaired RAM");run(c,2,"prefix JMP then cold",{...cpu(c),pc:PC+24},3,1);host(c,"unmap",[0x7000,1]);
 seed(c,PC+32,0xcd6);let next={...cpu(c),pc:PC+33,flags:0xcd7};run(c,1,"STC creates current FLAGS",next);push(c,"first retained PUSHFD");
 next={registers:[...cpu(c).registers],pc:PC+35,flags:0xcd7};next.registers[0]=0xcd7;next.registers[4]=RAM+0x100;run(c,1,"POPEAX consumes first FLAGS image",next,1,1,undefined,0,0xcd7);
 run(c,1,"CLC changes current FLAGS",{...cpu(c),pc:PC+36,flags:0xcd6});push(c,"second retained PUSHFD reflects CLC");
 next={registers:[...cpu(c).registers],pc:PC+38,flags:0xcd6};next.registers[2]=0xcd6;next.registers[4]=RAM+0x100;run(c,1,"POPEDX consumes second FLAGS image",next,1,1,undefined,0,0xcd6);run(c,2,"chain JMP then cold",{...cpu(c),pc:PC+40},3,1);
 seed(c,PC,2,0x700c);run(c,0,"zero budget avoids unmapped stack",cpu(c),1,0);input(c,96,words([1]),"cancellation");run(c,1,"cancel avoids unmapped stack",cpu(c),2,0);
 for(const params of [[c.base+1,c.base+56,1,c.base+96],[c.base,c.base+57,1,c.base+96]])run(c,1,"invalid pointer preflight before helper",cpu(c),1,0,undefined,1,undefined,params);
 seed(c,PC,2,0x700c);upload(c,PC,Buffer.from([0x9c]));run(c,1,"same-byte consumed code mutation rejects stale owner",cpu(c),1,0,undefined,4);close(c);
 const smc=open(owner,entries,"smc");seed(smc,PC,2,PC+4);push(smc,"code write commits ESP/EIP/retire before invalidation",6);
 run(smc,1,"invalidated tail avoids another store",cpu(smc),1,0,undefined,4);close(smc);
}
assert.deepEqual(pins(),sourcePins,"all20 source bytes unchanged");assert.equal(contexts,8);assert.equal(modules.length,8);assert.equal(pushes,212);assert.equal(faults,64);assert.equal(generated,332);assert.equal(retired,240);
const hostCounts=Object.fromEntries(["map","unmap","protect","upload","read32","close","compile","compile_entries","compile_resident","compile_resident_entries"].map(name=>[name,journal.filter(r=>r.name===name).length]));
assert.equal(frames.length,2*journal.length,"each journal event owns a full before/after arena pair");
const raw=Buffer.concat(frames);writeFileSync(join(output,"arenas.bin"),raw,{flag:"wx"});
const result={status:"ok",engine:{bytes:engine.bytes.length,sha256:engine.sha256,caller_sha256:process.env.RING3_ENGINE_SHA256},source_pins:sourcePins,
 counts:{contexts,modules:modules.length,current_flags_literals:9,valid_destinations:5,successful_pushes:pushes,fault_calls:faults,generated_calls:generated,retired,raw_frames:frames.length,journal_records:journal.length,host_counts:hostCounts},
 modules,journal,raw:{file:"arenas.bin",bytes:raw.length,sha256:hash(raw)},limits:["bare flat32 PUSHFD finite current admitted FLAGS domain only; no POPFD/VM/CPL/word semantics",
 "saved complete4364 arenas and declared uploads plus selected DWORD RAM diagnostics; no whole guest RAM dump or generic Wasm-body certification",
 "checked full4 store faults preserve current ESP/FLAGS/retire and observed RAM; sameCPU mapping/protection repair never reseeds CPU",
 "retained STC/PUSHFD/POP/CLC/PUSHFD/POP chain observes current FLAGS without host reseed; nonzero FP tail is opaque and unchanged",
 "close retains arena allocation; saved before/after close and closed generated rejection observe that retained allocation only"]};
writeFileSync(join(output,"result.json"),JSON.stringify(result,null,2),{flag:"wx"});console.log(JSON.stringify({status:result.status,...result.counts,output}));
