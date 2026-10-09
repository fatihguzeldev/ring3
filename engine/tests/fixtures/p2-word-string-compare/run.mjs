import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { readEngine } from "../support/engine.mjs";
const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root); assert.match(process.env.RING3_ENGINE_SHA256 ?? "", /^[a-f0-9]{64}$/);
const engine = readEngine(enginePath), SIZE = 4364, PC = 0x1000, SRC = 0x5008, DST = 0x9008;
assert.deepEqual(WebAssembly.Module.imports(engine.module), []);
const hash = b => createHash("sha256").update(b).digest("hex"), initial = readFileSync(join(output,"initial-arena.bin")); assert.equal(initial.length,SIZE);
const bank=Buffer.alloc(64,0xcc), blocks=[[0,[0x66,0xa7,0xeb,0]],[8,[0x66,0xaf,0xeb,0]],
 [16,[0xb8,0xff,0x7f,0x81,0xa5,0x66,0xaf,0xb8,0,0x80,0x81,0xa5,0x66,0xaf,0xeb,6]],
 [32,[0x66,0xa7,0x66,0xa7,0xeb,0]],[40,[0x90,0x66,0xa7,0xeb,0]],[48,[0x90,0x66,0xaf,0xeb,0]]];
for(const [at,code]of blocks)bank.set(code,at);assert.deepEqual(bank,readFileSync(join(output,"word-string-compare.x86")),"independent Rust/JS literal bank");
const paths=["engine/src/abi/arena.rs","engine/src/abi/memory_helper.rs","engine/src/abi/header.rs","engine/src/abi/x86/state.rs","engine/src/abi/x86/exit.rs","engine/src/abi/x86/x87.rs",
 "engine/src/cpu/x86/ir.rs","engine/src/cpu/x86/decode/decoder.rs","engine/src/cpu/x86/decode/lower.rs","engine/src/cpu/x86/decode/profile.rs","engine/src/cpu/dbt/region.rs",
 "engine/src/cpu/dbt/wasm/integer.rs","engine/src/cpu/dbt/wasm/memory.rs","engine/src/cpu/dbt/wasm/memory/narrow.rs","engine/src/cpu/dbt/wasm/emitter.rs","engine/src/cpu/dbt/wasm/locals.rs",
 "engine/src/memory/space.rs","engine/src/process/instance.rs","engine/src/process/resident.rs","engine/src/process/wasm.rs","engine/tests/cpu_word_string_compare_wasm.rs",
 "engine/tests/fixtures/p2-word-string-compare/run.mjs","engine/tests/fixtures/support/engine.mjs"];
const pins=()=>Object.fromEntries(paths.map(path=>{const b=readFileSync(join(root,path));return[path,{bytes:b.length,sha256:hash(b)}];})),sourcePins=pins(),frames=[],journal=[],modules=[];
const counts={contexts:0,targets:0,cmpsw:0,scasw:0,fault_calls:0,generated_calls:0,retired:0,mov_eax:0,jumps:0,prefix_nops:0,controls:0};
writeFileSync(join(output,"engine.wasm"),engine.bytes,{flag:"wx"});
function record(magic,size,fields,version=1){const b=Buffer.alloc(size);b.write(magic);b.writeUInt16LE(version,4);b.writeUInt16LE(1,6);b.writeUInt32LE(size,8);fields.forEach((v,i)=>b.writeUInt32LE(v>>>0,16+i*4));return b;}
const words=values=>Buffer.concat(values.map(v=>{const b=Buffer.alloc(4);b.writeUInt32LE(v>>>0);return b;}));
const word=value=>Buffer.from([value&255,(value>>>8)&255]),state=c=>record("R3ST",56,[...c.registers,c.pc,c.flags]);
const exit=(reason,n,f)=>record("R3EX",40,[reason,n,f?.detail??0,f?.address??0,f?1:0,f?2:0],2);
const helper=(value=0,f)=>record("R3MH",40,[f?1:0,f?0:value,f?.detail??0,f?.address??0,f?1:0,2],2);
const arena=c=>Buffer.from(new Uint8Array(c.memory.buffer,c.base,SIZE));
function capture(c,label){const b=arena(c),row={context:c.context,label,offset:frames.length*SIZE,bytes:SIZE,sha256:hash(b)};frames.push(b);return row;}
function input(c,offset,b,label){const before=capture(c,"before input "+label),wanted=arena(c);new Uint8Array(c.memory.buffer).set(b,c.base+offset);wanted.set(b,offset);assert.deepEqual(arena(c),wanted);
 journal.push({context:c.context,name:"input",offset,hex:b.toString("hex"),label,before,after:capture(c,"after input "+label)});}
function host(c,name,args,label=name){const before=capture(c,"before "+label),wanted=arena(c);assert.equal(c.api[name](...args),0,label);const after=arena(c);
 if(name.startsWith("compile_resident")){assert.equal(after.readUInt32LE(140),1);assert.equal(after.readUInt32LE(144),24);after.subarray(140,164).copy(wanted,140);}
 else if(name==="read16")after.subarray(100,140).copy(wanted,100);
 assert.deepEqual(after,wanted,"host exact arena effect "+label);journal.push({context:c.context,name,args,label,before,after:capture(c,"after "+label)});}
function upload(c,address,b){input(c,140,b,"declared upload");host(c,"upload",[address,b.length]);}
function cpu(c){const b=arena(c);return{registers:Array.from({length:8},(_,i)=>b.readUInt32LE(16+i*4)),pc:b.readUInt32LE(48),flags:b.readUInt32LE(52)};}
function seed(c,pc,flags,source=SRC,destination=DST,eax=0xa5818001){const b=Buffer.alloc(140),r=[eax>>>0,0x91b2c378,0x34560f10,0x4567ff00,0x56789abc,0x6789abcd,source,destination];
 b.set(state({registers:r,pc,flags}));b.set(exit(3,0),56);b.set(helper(0xbad5),100);input(c,0,b,"explicit initial CPU seed");return cpu(c);}
function diagnostic(c,address,value){host(c,"read16",[address],"declared current WORD RAM");assert.deepEqual(arena(c).subarray(100,140),helper(value));}
function run(c,budget,label,wanted,reason=1,n=1,f,status=0,value,params){const before=capture(c,"before generated "+label),expected=arena(c);
 if(status===0){expected.set(state(wanted));expected.set(exit(reason,n,f),56);if(f||value!==undefined)expected.set(helper(value??0,f),100);}
 const args=params??[c.base,c.base+56,budget,c.base+96];assert.equal(c.run(...args),status,label);assert.deepEqual(arena(c),expected,"full4364 "+label);
 counts.generated_calls++;if(status===0)counts.retired+=n;if(f)counts.fault_calls++;
 journal.push({context:c.context,name:"run",label,budget,status,args,before,after:capture(c,"after generated "+label)});}
function subtraction(left,right){const a=BigInt(left),b=BigInt(right),r=BigInt.asUintN(16,a-b);let flags=2;if(a<b)flags|=1;
 let ones=0;for(let bit=0;bit<8;bit++)ones+=Number((r>>BigInt(bit))&1n);if(!(ones&1))flags|=4;if((a^b^r)&16n)flags|=16;if(!r)flags|=64;if(r&0x8000n)flags|=128;if((a^b)&(a^r)&0x8000n)flags|=2048;return flags;}
const pairs=[[0,0,0x46],[0,1,0x97],[0x7fff,0xffff,0x887],[0x8000,1,0x816],[0x100,1,0x16],[0x101,0x100,2],[0,0x100,0x87],[0x100,0,6]];
for(const [left,right,flags]of pairs)assert.equal(subtraction(left,right),flags,"literal six-flag anchor");
function compare(c,kind,left,right,label){const old=cpu(c),next={registers:[...old.registers],pc:(old.pc+2)>>>0,flags:subtraction(left,right)|(old.flags&0x400)},delta=old.flags&0x400?-2:2;
 if(kind==="cmpsw")next.registers[6]=(old.registers[6]+delta)>>>0;else assert.equal(old.registers[0]&0xffff,left,"current low AX");next.registers[7]=(old.registers[7]+delta)>>>0;
 run(c,1,label,next,1,1,undefined,0,right);counts.targets++;counts[kind]++;}
function mov(c,value){const old=cpu(c),next={...old,registers:[...old.registers],pc:old.pc+5};next.registers[0]=value;run(c,1,"guest MOV supplies current AX",next);counts.mov_eax++;}
function jump(c,pc){run(c,2,"JMP then cold continuation",{...cpu(c),pc},3,1);counts.jumps++;}
function open(owner,entries){const instance=new WebAssembly.Instance(engine.module,{}),c={owner,entries,context:++counts.contexts,memory:instance.exports.memory,api:{}};
 for(const name of ["open","close","arena_ptr","map","unmap","protect","upload","read16","compile","compile_entries","compile_resident","compile_resident_entries","generation","module_ptr","module_len","guard","guard_resident"])c.api[name]=instance.exports["ring3_abi_v1_"+name];
 assert.equal(c.api.open(12,c.context,0x54555657),0);c.base=c.api.arena_ptr()>>>0;input(c,0,initial,"full initial arena with opaque FP128");
 for(const args of [[PC,1,7],[0x5000,2,3],[0x9000,2,3],[0xfffff000,1,3],[0,1,3]])host(c,"map",args);
 upload(c,PC,bank);const starts=blocks.map(([at])=>PC+at),lengths=blocks.map(([,code])=>code.length);input(c,140,words(entries?starts:starts.flatMap((pc,i)=>[pc,lengths[i]])),"six block descriptors");
 const method=owner==="resident"?(entries?"compile_resident_entries":"compile_resident"):(entries?"compile_entries":"compile");host(c,method,entries?[6,0]:[6]);let binding;
 if(owner==="resident"){const b=arena(c);binding=Object.fromEntries(["low","high","pointer","length"].map((name,i)=>[name,b.readUInt32LE(148+i*4)]));}
 else binding={generation:c.api.generation(),pointer:c.api.module_ptr()>>>0,length:c.api.module_len()>>>0};
 const bytes=Buffer.from(new Uint8Array(c.memory.buffer,binding.pointer,binding.length)),module=new WebAssembly.Module(bytes),names=[owner==="resident"?"guard_resident":"guard","read16"];
 assert.deepEqual(WebAssembly.Module.imports(module),[{module:"env",name:"memory",kind:"memory"},...names.map(name=>({module:"ring3",name,kind:"function"}))]);
 c.run=new WebAssembly.Instance(module,{env:{memory:c.memory},ring3:c.api}).exports.run;const file=`${owner}-${entries?"entries":"explicit"}.wasm`;
 writeFileSync(join(output,file),bytes,{flag:"wx"});modules.push({context:c.context,owner,entries,arena_base:c.base,file,...binding,bytes:bytes.length,sha256:hash(bytes),imports:names});return c;}
function close(c){const before=capture(c,"before close");assert.equal(c.api.close(),0);journal.push({context:c.context,name:"close",args:[],before});
 assert.equal(c.run(c.base,c.base+56,1,c.base+96),5,"closed owner status only");counts.generated_calls++;counts.controls++;journal.push({context:c.context,name:"closed-run",status:5});}
for(const owner of ["replacement","resident"])for(const entries of [false,true]){const c=open(owner,entries);
 for(const kind of ["cmpsw","scasw"])for(const flags of [2,0xcd7])for(const [left,right]of pairs){upload(c,SRC,word(left));upload(c,DST,word(right));diagnostic(c,SRC,left);diagnostic(c,DST,right);
 seed(c,PC+(kind==="cmpsw"?0:8),flags,kind==="cmpsw"?SRC:0xd008,DST,(0xa5810000|left)>>>0);compare(c,kind,left,right,"literal width16 status flags");}
 for(const kind of ["cmpsw","scasw"])for(const [source,destination,flags]of [[SRC+1,DST+1,2],[0x5fff,0x9fff,0xcd7],[0xfffffffe,0xfffffffe,2],[0,0,0xcd7]]){
 upload(c,source,word(0x8001));upload(c,destination,word(0x8001));diagnostic(c,source,0x8001);diagnostic(c,destination,0x8001);seed(c,PC+(kind==="cmpsw"?0:8),flags,source,destination);compare(c,kind,0x8001,0x8001,"unaligned crossing alias endpoint and pointer wrap");}
 const failures=[{kind:"cmpsw",side:"source",address:0x7008,detail:1},{kind:"cmpsw",side:"source",address:0x7008,detail:2},
 {kind:"cmpsw",side:"destination",address:0xb008,detail:1},{kind:"cmpsw",side:"destination",address:0xb008,detail:2},
 {kind:"scasw",side:"destination",address:0xb008,detail:1},{kind:"scasw",side:"destination",address:0xb008,detail:2},
 {kind:"cmpsw",side:"source",address:0x6fff,detail:1,crossing:true},{kind:"cmpsw",side:"destination",address:0xafff,detail:2,crossing:true},
 {kind:"scasw",side:"destination",address:0xafff,detail:1,crossing:true}];
 for(const f of failures){const repairPage=f.crossing?(f.address&~0xfff)+0x1000:f.address&~0xfff,source=f.side==="source"?f.address:SRC,destination=f.side==="destination"?f.address:DST,value=f.side==="source"?0x8001:1;
 upload(c,SRC,word(0x8001));upload(c,DST,word(1));if(f.crossing)upload(c,f.address,word(value).subarray(0,1));
 if(f.detail===2){host(c,"map",[repairPage,1,3]);upload(c,f.crossing?repairPage:f.address,f.crossing?word(value).subarray(1):word(value));host(c,"protect",[repairPage,1,2]);}
 const entry=PC+(f.kind==="cmpsw"?40:48),old=seed(c,entry,0xcd7,source,destination),held={...old,pc:entry+1},fault={detail:f.detail,address:f.crossing?repairPage:f.address};
 run(c,2,"retired NOP then precise read2 fault",held,5,1,fault);counts.prefix_nops++;run(c,1,"same CPU repeated read2 fault",held,5,0,fault);
 host(c,f.detail===1?"map":"protect",[repairPage,1,3]);upload(c,f.address,word(value));let left=0x8001,right=1;
 if(f.kind==="cmpsw"&&f.side==="destination"){upload(c,SRC,word(0x7fff));upload(c,destination,word(0xffff));left=0x7fff;right=0xffff;}
 if(f.kind==="scasw"){input(c,16,words([0xa5817fff]),"repair changes only current EAX, not a CPU reseed");held.registers[0]=0xa5817fff;upload(c,destination,word(0xffff));left=0x7fff;right=0xffff;}
 if(f.kind==="cmpsw")diagnostic(c,source,left);diagnostic(c,destination,right);assert.deepEqual(cpu(c),held,"retained target CPU with declared EAX-only repair");compare(c,f.kind,left,right,"same CPU repair rereads current operands");host(c,"unmap",[repairPage,1]);}
 const held=seed(c,PC,0xcd7,0x7008,0xb008);for(let n=0;n<2;n++)run(c,1,"both absent source-first policy",held,5,0,{detail:1,address:0x7008});
 host(c,"map",[0x7000,1,3]);upload(c,0x7008,word(0x7fff));for(let n=0;n<2;n++)run(c,1,"second read fault cannot publish first operand",held,5,0,{detail:1,address:0xb008});
 host(c,"map",[0xb000,1,3]);upload(c,0xb008,word(0xffff));assert.deepEqual(cpu(c),held);compare(c,"cmpsw",0x7fff,0xffff,"dual-fault ordered repair");host(c,"unmap",[0x7000,1]);host(c,"unmap",[0xb000,1]);
 for(const [kind,side]of [["cmpsw","source"],["cmpsw","destination"],["scasw","destination"]]){upload(c,SRC,word(0x8001));upload(c,DST,word(1));
 const entry=PC+(kind==="cmpsw"?40:48),old=seed(c,entry,0xcd7,side==="source"?0xffffffff:SRC,side==="destination"?0xffffffff:DST),held={...old,pc:entry+1},fault={detail:3,address:0xffffffff};
 run(c,2,"width2 overflow retains prefix",held,5,1,fault);counts.prefix_nops++;run(c,1,"retained overflow without wrapped read",held,5,0,fault);}
 for(const flags of [2,0xcd7]){const delta=flags&0x400?-2:2;for(const [address,value]of [[SRC,0x8000],[SRC+delta,0x1111],[DST,1],[DST+delta,0x2222]])upload(c,address,word(value));
 seed(c,PC+32,flags);compare(c,"cmpsw",0x8000,1,"first retained CMPSW");const held=cpu(c);upload(c,SRC+delta,word(0x7fff));upload(c,DST+delta,word(0xffff));diagnostic(c,SRC+delta,0x7fff);diagnostic(c,DST+delta,0xffff);assert.deepEqual(cpu(c),held);compare(c,"cmpsw",0x7fff,0xffff,"second retained CMPSW current RAM");jump(c,PC+38);
 upload(c,DST,word(0xffff));upload(c,DST+delta,word(1));seed(c,PC+16,flags);mov(c,0xa5817fff);compare(c,"scasw",0x7fff,0xffff,"SCASW current AX after guest MOV");mov(c,0xa5818000);compare(c,"scasw",0x8000,1,"second SCASW current AX without reseed");jump(c,PC+38);}
 for(const entry of [PC,PC+8]){seed(c,entry,2,0xd008,0xe008);run(c,0,"zero budget avoids bad pointers",cpu(c),1,0);counts.controls++;input(c,96,words([1]),"explicit cancellation");run(c,1,"cancel avoids bad pointers",cpu(c),2,0);counts.controls++;}
 seed(c,PC,2,0xd008,0xe008);for(const params of [[c.base+1,c.base+56,1,c.base+96],[c.base,c.base+57,1,c.base+96]]){run(c,1,"invalid pointer preflight",cpu(c),1,0,undefined,1,undefined,params);counts.controls++;}
 host(c,"protect",[PC,1,7]);upload(c,PC,Buffer.from([0x66]));run(c,1,"same-byte code upload rejects stale owner",cpu(c),1,0,undefined,4);counts.controls++;close(c);
}
assert.deepEqual(pins(),sourcePins);assert.equal(counts.contexts,4);assert.equal(modules.length,4);assert.equal(counts.targets,232);assert.equal(counts.fault_calls,112);assert.equal(counts.generated_calls,408);assert.equal(counts.retired,312);
assert.equal(counts.cmpsw,124);assert.equal(counts.scasw,108);assert.equal(counts.mov_eax,16);assert.equal(counts.jumps,16);assert.equal(counts.prefix_nops,48);assert.equal(counts.controls,32);
const hostCounts=Object.fromEntries(["map","unmap","protect","upload","read16","compile","compile_entries","compile_resident","compile_resident_entries"].map(name=>[name,journal.filter(row=>row.name===name).length]));
assert.deepEqual(hostCounts,{map:64,unmap:44,protect:36,upload:596,read16:396,compile:1,compile_entries:1,compile_resident:1,compile_resident_entries:1});
assert.equal(journal.filter(row=>row.name==="input").length,864);assert.equal(journal.length,2816);assert.equal(frames.length,5620);
const raw=Buffer.concat(frames);writeFileSync(join(output,"arenas.bin"),raw,{flag:"wx"});
const result={status:"ok",engine:{bytes:engine.bytes.length,sha256:engine.sha256,caller_sha256:process.env.RING3_ENGINE_SHA256},source_pins:sourcePins,counts:{...counts,modules:modules.length,host_counts:hostCounts,host_inputs:journal.filter(row=>row.name==="input").length,journal_records:journal.length,raw_frames:frames.length},modules,journal,
 raw:{file:"arenas.bin",frames:frames.length,bytes:raw.length,sha256:hash(raw)},limits:["exact flat32 CMPSW/SCASW finite word literals/DF/read2/current AX only","full4364 saved input/host/generated pairs plus preclose frames and selected live Read16 diagnostics; no RAM dump or full module body proof",
 "ESI-first dual-fault order is engine policy, not hardware precedence","same CPU fault repair uses current RAM and explicit EAX-only repair, not whole CPU reseeding","four representative bound profiles; closed calls raw-status only, no postclose arena/module pointer observation","no full ISA, performance or CI claim"]};
writeFileSync(join(output,"result.json"),JSON.stringify(result,null,2),{flag:"wx"});console.log(JSON.stringify({status:result.status,...result.counts,output}));
