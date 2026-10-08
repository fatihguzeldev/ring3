import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { readEngine } from "../support/engine.mjs";

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root);
assert.match(process.env.RING3_ENGINE_SHA256 ?? "", /^[a-f0-9]{64}$/);
const hash = bytes => createHash("sha256").update(bytes).digest("hex");
const engine = readEngine(enginePath), SIZE = 4364, PC = 0x1000, SRC = 0x5000, DST = 0x8000;
const initial = readFileSync(join(output, "initial-arena.bin")); assert.equal(initial.length, SIZE);
const bank = Buffer.alloc(32, 0xcc); bank.set([0xa5,0xeb,0]); bank.set([0xb8,0xdf,0x9b,0x57,0x13,0xa5,0xeb,0],16);
assert.deepEqual(bank,readFileSync(join(output,"movsd.x86")),"independent Rust/JS literal bank");
const sourcePaths = ["engine/src/abi/arena.rs","engine/src/abi/memory_helper.rs","engine/src/abi/x86/x87.rs",
  "engine/src/cpu/x86/ir.rs","engine/src/cpu/x86/decode/lower.rs","engine/src/cpu/x86/decode/profile.rs",
  "engine/src/cpu/dbt/region.rs","engine/src/cpu/dbt/wasm/integer.rs","engine/src/cpu/dbt/wasm/memory.rs",
  "engine/src/cpu/dbt/wasm/emitter.rs","engine/src/cpu/dbt/wasm/locals.rs","engine/src/memory/space.rs",
  "engine/src/process/instance.rs","engine/src/process/resident.rs","engine/tests/cpu_dword_move_string_wasm.rs",
  "engine/tests/fixtures/p2-dword-move-string/run.mjs","engine/tests/fixtures/support/engine.mjs"];
const sourcePins = () => Object.fromEntries(sourcePaths.map(path=>{const bytes=readFileSync(join(root,path));
  return [path,{bytes:bytes.length,sha256:hash(bytes)}];}));
const beforeSources=sourcePins(), frames=[], journal=[], modules=[], copies=[], ram=[];
let generated=0, retired=0, faults=0, closed=0, contexts=0;
writeFileSync(join(output,"engine.wasm"),engine.bytes,{flag:"wx"});
function record(magic,size,fields,version=1){const bytes=Buffer.alloc(size);bytes.write(magic);bytes.writeUInt16LE(version,4);
  bytes.writeUInt16LE(1,6);bytes.writeUInt32LE(size,8);fields.forEach((value,i)=>bytes.writeUInt32LE(value>>>0,16+i*4));return bytes;}
const state = c=>record("R3ST",56,[...c.registers,c.pc,c.flags]);
const exit = (reason,count,f,version=2)=>record("R3EX",40,[reason,count,f?.detail??0,f?.address??0,f?.access??0,f?4:0],version);
const helper = (value=0,f)=>record("R3MH",40,[f?1:0,f?0:value,f?.detail??0,f?.address??0,f?.access??0,f?4:0]);
const words = values=>Buffer.concat(values.map(value=>{const b=Buffer.alloc(4);b.writeUInt32LE(value>>>0);return b;}));
const arena = c=>Buffer.from(new Uint8Array(c.memory.buffer,c.base,SIZE));
function capture(c,label){const bytes=arena(c),row={context:c.ordinal,owner:c.owner,label,offset:frames.length*SIZE,bytes:SIZE,sha256:hash(bytes)};
  frames.push(bytes);return row;}
function cpu(c){const b=arena(c);return {registers:Array.from({length:8},(_,i)=>b.readUInt32LE(16+i*4)),pc:b.readUInt32LE(48),flags:b.readUInt32LE(52)};}
function host(c,name,args,label=name){const before=capture(c,"before "+label),old=arena(c);
  assert.equal(c.api[name](...args),0,label);assert.deepEqual(arena(c).subarray(0,56),old.subarray(0,56),label+" preserves CPU");
  assert.deepEqual(arena(c).subarray(4236),old.subarray(4236),label+" preserves opaque FP tail");
  if(name==="map"||name==="unmap"){const [start,pages]=args,end=start+pages*4096;
    for(const address of c.known.keys())if(address>=start&&address<end)c.known.delete(address);}
  const row={context:c.ordinal,owner:c.owner,name,args,label,before,after:capture(c,"after "+label)};journal.push(row);return row;}
function input(c,offset,bytes,label){const before=capture(c,"before input "+label);new Uint8Array(c.memory.buffer).set(bytes,c.base+offset);
  journal.push({context:c.ordinal,owner:c.owner,name:"input",offset,hex:bytes.toString("hex"),label,before,after:capture(c,"after input "+label)});}
function upload(c,address,bytes){input(c,140,bytes,"declared upload");host(c,"upload",[address,bytes.length]);
  for(let i=0;i<bytes.length;i++)c.known.set(address+i,bytes[i]);}
function known(c,address,length){return Buffer.from(Array.from({length},(_,i)=>c.known.get(address+i)??0));}
function readRam(c,address,length,label){const rows=[],bytes=Buffer.alloc(length);
  for(let i=0;i<length;i++){const row=host(c,"read8",[address+i],label+" byte"+i);rows.push(row);bytes[i]=arena(c).readUInt32LE(120);
    assert.deepEqual(arena(c).subarray(100,140),record("R3MH",40,[0,bytes[i],0,0,0,1],2),"canonical Read8 diagnostic");}
  assert.deepEqual(bytes,known(c,address,length),label+" equals declared upload/store model");
  const row={context:c.ordinal,owner:c.owner,label,address,bytes:length,hex:bytes.toString("hex"),reads:rows};ram.push(row);return row;}
function seed(c,pc,flags,source=SRC+8,destination=DST+8){const registers=[0xa5812347,0x91b2c378,0x34560f10,0x4567ff00,0x56789abc,0x6789abcd,source,destination];
  const b=Buffer.alloc(140);b.set(state({registers,pc,flags}));b.set(exit(3,0),56);b.set(helper(0xdecafbad),100);
  input(c,0,b,"explicit initial CPU");return cpu(c);}
function run(c,budget,label,next,reason=1,count=1,f,status=0,memoryTarget=true,exitVersion=2){const before=capture(c,"before generated "+label),expected=arena(c);
  if(status===0){expected.set(state(next));expected.set(exit(reason,count,f,exitVersion),56);if(f||(memoryTarget&&count))expected.set(helper(0,f),100);}
  assert.equal(c.run(c.base,c.base+56,budget,c.base+96),status,label);assert.deepEqual(arena(c),expected,"full arena "+label);
  generated++;retired+=status===0?count:0;const row={context:c.ordinal,owner:c.owner,name:"run",label,budget,status,before,after:capture(c,"after generated "+label)};
  journal.push(row);return row;}
function window(address){return address===0?[0,5]:address===0xfffffffc?[0xfffffffb,5]:[address-1,6];}
function copy(c,label,value,reason=1){const before=cpu(c),source=before.registers[6],destination=before.registers[7],w=window(destination);
  const beforeRam=readRam(c,...w,"before "+label);host(c,"read32",[source],"declared DWORD source");
  assert.equal(arena(c).readUInt32LE(120),value>>>0,"source diagnostic equals independent declared literal");
  assert.deepEqual(arena(c).subarray(100,140),helper(value),"canonical Read32 diagnostic");
  const next={registers:[...before.registers],pc:(before.pc+1)>>>0,flags:before.flags},delta=before.flags&0x400?-4:4;
  next.registers[6]=(source+delta)>>>0;next.registers[7]=(destination+delta)>>>0;
  const row=run(c,1,label,next,reason);const bytes=words([value]);for(let i=0;i<4;i++)c.known.set(destination+i,bytes[i]);
  const afterRam=readRam(c,...w,"after "+label);copies.push({context:c.ordinal,owner:c.owner,label,source,destination,value:value>>>0,before,next,run:row,beforeRam,afterRam});}
function open(owner,ordinal){const instance=new WebAssembly.Instance(engine.module,{}),c={owner,ordinal,memory:instance.exports.memory,api:{},known:new Map()};
  for(const name of ["open","close","arena_ptr","map","unmap","protect","upload","read8","read32","compile","compile_resident","generation","module_ptr","module_len","guard","guard_resident","store32","store_resident32"])
    c.api[name]=instance.exports["ring3_abi_v1_"+name];
  assert.equal(c.api.open(10,ordinal,0x51525354),0);c.base=c.api.arena_ptr()>>>0;contexts++;
  input(c,0,initial,"complete initial arena including arbitrary raw80 FP tail");
  for(const [address,count,permissions]of [[PC,1,7],[SRC,2,3],[DST,2,3],[0xfffff000,1,3],[0,1,3]])host(c,"map",[address,count,permissions]);
  upload(c,PC,bank);input(c,140,words([PC,3,PC+16,8]),"two compiler block descriptors");
  let binding;if(owner==="replacement"){host(c,"compile",[2]);binding={generation:c.api.generation(),pointer:c.api.module_ptr()>>>0,length:c.api.module_len()>>>0};}
  else{host(c,"compile_resident",[2]);const b=arena(c);binding=Object.fromEntries(["low","high","pointer","length"].map((name,i)=>[name,b.readUInt32LE(148+i*4)]));}
  const bytes=Buffer.from(new Uint8Array(c.memory.buffer,binding.pointer,binding.length)),module=new WebAssembly.Module(bytes),names=owner==="replacement"?["guard","read32","store32"]:["guard_resident","read32","store_resident32"];
  assert.deepEqual(WebAssembly.Module.imports(module),[{module:"env",name:"memory",kind:"memory"},...names.map(name=>({module:"ring3",name,kind:"function"}))]);
  c.run=new WebAssembly.Instance(module,{env:{memory:c.memory},ring3:c.api}).exports.run;c.binding=binding;
  const file=owner+"-"+ordinal+".wasm";writeFileSync(join(output,file),bytes,{flag:"wx"});modules.push({context:ordinal,owner,file,...binding,bytes:bytes.length,sha256:hash(bytes),imports:names});return c;}
function close(c){const before=capture(c,"before close");assert.equal(c.api.close(),0);journal.push({context:c.ordinal,owner:c.owner,name:"close",args:[],before});
  assert.equal(c.run(c.base,c.base+56,1,c.base+96),5,"closed owner rejects before touching former arena");generated++;closed++;
  journal.push({context:c.ordinal,owner:c.owner,name:"closed-run",budget:1,status:5,label:"status only; retained arena not read"});}
function fill(c,source,destination,value){if(source===destination||Math.abs(source-destination)<=1){const start=Math.max(0,Math.min(source,destination)-1),end=Math.min(0x100000000,Math.max(source,destination)+5),b=Buffer.alloc(end-start,0x55);
    words([value]).copy(b,source-start);upload(c,start,b);}
  else{upload(c,source,words([value]));const [start,length]=window(destination);upload(c,start,Buffer.alloc(length,0x55));}}
for(const [ordinal,owner]of ["replacement","resident"].entries()){
  const c=open(owner,ordinal+1);
  for(const flags of [2,0xcd7])for(const value of [0,0xffffffff,0x80000000,0x1234a581])for(const offset of [8,9,4094]){
    fill(c,SRC+offset,DST+offset,value);seed(c,PC,flags,SRC+offset,DST+offset);copy(c,"literal DF DWORD copy",value);}
  for(const flags of [2,0xcd7])for(const difference of [0,1,-1]){fill(c,SRC+8,SRC+8+difference,0x1234a581);seed(c,PC,flags,SRC+8,SRC+8+difference);copy(c,"whole DWORD overlap",0x1234a581);}
  for(const [pointer,flags]of [[0xfffffffc,2],[0,0xcd7]]){fill(c,pointer,pointer,0x8191a6b7);seed(c,PC,flags,pointer,pointer);copy(c,"valid access then modulo32 pointer",0x8191a6b7);}
  run(c,2,"JMP then cold target with spare budget",{...cpu(c),pc:PC+3},3,1,undefined,0,false);
  const cases=[
    {source:0xa008,destination:DST+8,address:0xa008,access:1,detail:1},
    {source:0xa008,destination:DST+8,address:0xa008,access:1,detail:2,map:[[0xa000,1,2]]},
    {source:SRC+8,destination:0xa008,address:0xa008,access:2,detail:1},
    {source:SRC+8,destination:0xa008,address:0xa008,access:2,detail:2,map:[[0xa000,1,3]],deny:0xa000},
    {source:0xaffe,destination:DST+8,address:0xb000,access:1,detail:1,map:[[0xa000,1,3]]},
    {source:SRC+8,destination:0xaffe,address:0xb000,access:2,detail:2,map:[[0xa000,2,3]],deny:0xb000},
    {source:0xfffffffd,destination:DST+8,address:0xfffffffd,access:1,detail:3},
    {source:SRC+8,destination:0xfffffffd,address:0xfffffffd,access:2,detail:3}];
  for(const [index,f]of cases.entries()){
    for(const args of f.map??[])host(c,"map",args);
    upload(c,SRC+8,words([0x1234a581]));let ramWindow;
    if(f.destination===DST+8){upload(c,DST+7,Buffer.alloc(6,0x55));ramWindow=[DST+7,6];}
    else if(f.deny){upload(c,...(()=>{const [start,length]=window(f.destination);return [start,Buffer.alloc(length,0x55)];})());ramWindow=window(f.destination);}
    else if(f.destination===0xfffffffd){upload(c,0xfffffffc,Buffer.from([0x31,0x55,0x66,0x77]));ramWindow=[0xfffffffc,4];}
    if(f.deny)host(c,"protect",[f.deny,1,1]);
    if(ramWindow)readRam(c,...ramWindow,"declared failed-store sentinel before");
    const before=seed(c,PC+16,0xcd7,f.source,f.destination),afterPrefix={registers:[...before.registers],pc:PC+21,flags:before.flags};afterPrefix.registers[0]=0x13579bdf;
    run(c,2,"retired prefix then width4 fault",afterPrefix,5,1,f);faults++;if(ramWindow)readRam(c,...ramWindow,"sentinels after first failure");
    run(c,1,"same CPU repeat fault",afterPrefix,5,0,f);faults++;if(ramWindow)readRam(c,...ramWindow,"sentinels after retained retry");
    if(index<6){if(f.detail===2)host(c,"protect",[f.address&~0xfff,1,3]);else host(c,"map",[f.address&~0xfff,1,3]);
      upload(c,f.source,words([0x8172a691]));if(!ramWindow){const [start,length]=window(f.destination);upload(c,start,Buffer.alloc(length,0x55));}
      assert.deepEqual(cpu(c),afterPrefix,"RAM repair never reseeds CPU");copy(c,"repaired target rereads current source",0x8172a691);}
    for(const args of f.map??[])host(c,"unmap",[args[0],args[1]]);
    if(index<6&&f.detail===1)host(c,"unmap",[f.address&~0xfff,1]);
  }
  const f={source:0xa008,destination:0xb008,address:0xa008,access:1,detail:1};const before=seed(c,PC+16,2,f.source,f.destination),next={registers:[...before.registers],pc:PC+21,flags:2};next.registers[0]=0x13579bdf;
  run(c,2,"both addresses fault: explicit ESI-first",next,5,1,f);faults++;
  host(c,"map",[0xa000,1,3]);upload(c,f.source,words([0x7fa59123]));run(c,1,"source repaired exposes destination fault",next,5,0,{detail:1,address:f.destination,access:2});faults++;
  host(c,"map",[0xb000,1,3]);upload(c,f.destination-1,Buffer.alloc(6,0x55));copy(c,"both repaired current CPU",0x7fa59123);
  host(c,"unmap",[0xa000,2]);
  seed(c,PC,2,0xa008,0xb008);run(c,0,"zero budget avoids unmapped memory",cpu(c),1,0);
  input(c,96,words([1]),"declared cancellation");run(c,1,"cancel avoids unmapped memory",cpu(c),2,0);close(c);
}
for(const owner of ["replacement","resident"])for(const same of [true,false]){const c=open(owner,contexts+1);const value=same?bank.readUInt32LE(0):0x9000ebfc;
  upload(c,SRC+8,words([value]));seed(c,PC,2,SRC+8,PC);copy(c,"same/different consumed-code store commits before invalidation",value,6);
  const held=Buffer.from(new Uint8Array(c.memory.buffer,c.binding.pointer,c.binding.length));run(c,1,"stale owner preserves known current arena",cpu(c),1,0,undefined,4);
  assert.deepEqual(Buffer.from(new Uint8Array(c.memory.buffer,c.binding.pointer,c.binding.length)),held,"live allocation unchanged immediately before any getter/allocation");
  input(c,140,words([PC+1,2]),"fresh current tail descriptor; no CPU repair");
  let binding;if(owner==="replacement"){host(c,"compile",[1]);binding={generation:c.api.generation(),pointer:c.api.module_ptr()>>>0,length:c.api.module_len()>>>0};}
  else{host(c,"compile_resident",[1]);const b=arena(c);binding=Object.fromEntries(["low","high","pointer","length"].map((name,i)=>[name,b.readUInt32LE(148+i*4)]));}
  const bytes=Buffer.from(new Uint8Array(c.memory.buffer,binding.pointer,binding.length)),module=new WebAssembly.Module(bytes),guard=owner==="replacement"?"guard":"guard_resident";
  assert.deepEqual(WebAssembly.Module.imports(module),[{module:"env",name:"memory",kind:"memory"},{module:"ring3",name:guard,kind:"function"}]);
  c.run=new WebAssembly.Instance(module,{env:{memory:c.memory},ring3:c.api}).exports.run;c.binding=binding;
  const file=owner+"-"+c.ordinal+"-tail.wasm";writeFileSync(join(output,file),bytes,{flag:"wx"});modules.push({context:c.ordinal,owner,file,...binding,bytes:bytes.length,sha256:hash(bytes),imports:[guard]});
  run(c,2,"fresh current tail continues retained CPU then cold",{...cpu(c),pc:PC+3},3,1,undefined,0,false,1);close(c);}
assert.deepEqual(sourcePins(),beforeSources,"source bytes unchanged");
assert.equal(copies.length,82);assert.equal(faults,36);assert.equal(generated,138);assert.equal(retired,106);assert.equal(closed,6);assert.equal(contexts,6);assert.equal(modules.length,10);
const hostCounts=Object.fromEntries(["map","unmap","protect","upload","read8","read32","compile","compile_resident"].map(name=>[name,journal.filter(row=>row.name===name).length]));
assert.deepEqual(hostCounts,{map:48,unmap:16,protect:10,upload:170,read8:1216,read32:82,compile:5,compile_resident:5});
assert.equal(journal.filter(row=>row.name==="input").length,276);assert.equal(journal.length,1972);assert.equal(frames.length,3926);
const raw=Buffer.concat(frames);writeFileSync(join(output,"arenas.bin"),raw,{flag:"wx"});
const result={status:"ok",engine:{bytes:engine.bytes.length,sha256:engine.sha256,caller_sha256:process.env.RING3_ENGINE_SHA256},source_pins:beforeSources,
  counts:{contexts,modules:modules.length,core_success:64,copies:copies.length,fault_calls:faults,generated_calls:generated,retired,raw_frames:frames.length,closed_status_only:closed,host_calls:1552,host_inputs:276,journal_records:1972,physical_files:15,host_counts:hostCounts},
  modules,copies,ram,journal,raw:{file:"arenas.bin",bytes:raw.length,sha256:hash(raw)},limits:["bare MOVSD only; finite literals/placements/DF/overlap/fault/SMC cases",
  "full saved4364 arenas plus selected physical destination/sentinel bytes; no whole RAM dump or generic Wasm-body certification",
  "retained repairs keep CPU/prefix/owner; source RAM is explicitly changed and reread", "stale allocation equality is a live receipt; no independently saved post-allocation bytes",
  "closed controls prove raw5 only; no post-close module allocation dereference; retained arena is not inspected","native modules are validation-only, actual guest execution is this separately pinned integrated engine"]};
writeFileSync(join(output,"result.json"),JSON.stringify(result,null,2),{flag:"wx"});console.log(JSON.stringify({status:result.status,...result.counts,output}));
