import assert from "node:assert/strict";
import {createHash} from "node:crypto";
import {readFileSync, writeFileSync} from "node:fs";
import {join} from "node:path";
import {readEngine} from "../support/engine.mjs";
const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root); assert.match(process.env.RING3_ENGINE_SHA256 ?? "", /^[a-f0-9]{64}$/);
const engine = readEngine(enginePath), SIZE = 4364, PC = 0x1000, DST = 0x5000;
assert.deepEqual(WebAssembly.Module.imports(engine.module), []);
const initial = readFileSync(join(output, "initial-arena.bin")); assert.equal(initial.length, SIZE);
const bank = Buffer.alloc(64, 0xcc); bank.set([0x66,0xab,0xeb,0]);
bank.set([0xb8,0x23,0x91,0xa5,0x81,0x66,0xab,0xb8,0x98,0xba,0xdc,0xfe,0x66,0xab,0xeb,0],16);
bank.set([0xb8,0xdf,0x9b,0x57,0x13,0x66,0xab,0xeb,0],40); for (const offset of [4,32,49]) bank.set([0x0f,0x0b],offset);
assert.deepEqual(bank, readFileSync(join(output,"stosw.x86")), "independent Rust/JS literal bank");
const hash = b => createHash("sha256").update(b).digest("hex"), frames = [], journal = [], modules = [], stores = [], ram = [];
const paths = ["engine/src/abi/arena.rs","engine/src/abi/memory_helper.rs","engine/src/abi/x86/x87.rs","engine/src/cpu/x86/ir.rs","engine/src/cpu/x86/decode/lower.rs","engine/src/cpu/x86/decode/profile.rs","engine/src/cpu/dbt/region.rs","engine/src/cpu/dbt/wasm/integer.rs","engine/src/cpu/dbt/wasm/memory.rs","engine/src/cpu/dbt/wasm/memory/word_store.rs","engine/src/cpu/dbt/wasm/emitter.rs","engine/src/cpu/dbt/wasm/locals.rs","engine/src/memory/space.rs","engine/src/process/instance.rs","engine/src/process/resident.rs","engine/tests/cpu_word_store_string_wasm.rs","engine/tests/fixtures/p2-word-store-string/run.mjs","engine/tests/fixtures/support/engine.mjs"];
const sourcePins = () => Object.fromEntries(paths.map(path => {const b=readFileSync(join(root,path)); return [path,{bytes:b.length,sha256:hash(b)}];}));
const beforeSources = sourcePins(); let generated = 0, retired = 0, faults = 0, contexts = 0;
writeFileSync(join(output,"engine.wasm"),engine.bytes,{flag:"wx"});
function record(magic,size,fields,version=1) {const b=Buffer.alloc(size);b.write(magic);b.writeUInt16LE(version,4);b.writeUInt16LE(1,6);b.writeUInt32LE(size,8);fields.forEach((v,i)=>b.writeUInt32LE(v>>>0,16+i*4));return b;}
const state = c => record("R3ST",56,[...c.registers,c.pc,c.flags]);
const exit = (reason,count,f,version=2) => record("R3EX",40,[reason,count,f?.detail??0,f?.address??0,f?2:0,f?2:0],version);
const helper = f => record("R3MH",40,[f?1:0,0,f?.detail??0,f?.address??0,f?2:0,2],4);
const words = values => Buffer.concat(values.map(v=>{const b=Buffer.alloc(4);b.writeUInt32LE(v>>>0);return b;}));
const arena = c => Buffer.from(new Uint8Array(c.memory.buffer,c.base,SIZE));
function capture(c,label) {const b=arena(c),row={context:c.ordinal,owner:c.owner,entries:c.entries,label,offset:frames.length*SIZE,bytes:SIZE,sha256:hash(b)};frames.push(b);return row;}
function cpu(c) {const b=arena(c);return {registers:Array.from({length:8},(_,i)=>b.readUInt32LE(16+i*4)),pc:b.readUInt32LE(48),flags:b.readUInt32LE(52)};}
function host(c,name,args,label=name,literal) {
  const before=capture(c,"before "+label),old=arena(c);assert.equal(c.api[name](...args),0,label);
  if(name==="read8") {assert.notEqual(literal,undefined);old.set(record("R3MH",40,[0,literal,0,0,0,1],2),100);}
  else if(name==="compile_resident"||name==="compile_resident_entries") {const now=arena(c);assert.equal(now.readUInt32LE(140),1);assert.equal(now.readUInt32LE(144),24);old.set(now.subarray(140,164),140);}
  assert.deepEqual(arena(c),old,"full4364 host "+label);
  if(name==="map"||name==="unmap") {const [start,pages]=args;for(const address of c.known.keys())if(address>=start&&address<start+pages*4096)c.known.delete(address);}
  const row={context:c.ordinal,owner:c.owner,name,args,label,before,after:capture(c,"after "+label)};journal.push(row);return row;
}
function input(c,offset,b,label) {const before=capture(c,"before input "+label),expected=arena(c);expected.set(b,offset);new Uint8Array(c.memory.buffer).set(b,c.base+offset);assert.deepEqual(arena(c),expected,"full4364 input "+label);const row={context:c.ordinal,owner:c.owner,name:"input",offset,hex:b.toString("hex"),label,before,after:capture(c,"after input "+label)};journal.push(row);}
function upload(c,address,b) {input(c,140,b,"declared RAM upload");host(c,"upload",[address,b.length]);for(let i=0;i<b.length;i++)c.known.set(address+i,b[i]);}
function known(c,address,length) {return Buffer.from(Array.from({length},(_,i)=>c.known.get(address+i)??0));}
function readRam(c,address,length,label) {
  const expected=known(c,address,length),reads=[];
  for(let i=0;i<length;i++) reads.push(host(c,"read8",[address+i],label+" byte"+i,expected[i]));
  const row={context:c.ordinal,owner:c.owner,label,address,bytes:length,hex:expected.toString("hex"),reads};ram.push(row);return row;
}
function seed(c,pc,flags,value,destination) {const registers=[value,0x91b2c378,0x34560f10,0x4567ff00,0x56789abc,0x6789abcd,0xdeadc008,destination],b=Buffer.alloc(140);b.set(state({registers,pc,flags}));b.set(exit(3,0),56);b.set(record("R3MH",40,[0,0xdecafbad,0,0,0,0]),100);input(c,0,b,"explicit CPU/exit/helper seed");return cpu(c);}
function run(c,budget,label,next,reason=1,count=1,f,status=0,memoryTarget=true,version=2,args) {
  const before=capture(c,"before generated "+label),expected=arena(c);
  if(status===0) {expected.set(state(next));expected.set(exit(reason,count,f,version),56);if(f||(memoryTarget&&count))expected.set(helper(f),100);}
  assert.equal(c.run(...(args??[c.base,c.base+56,budget,c.base+96])),status,label);assert.deepEqual(arena(c),expected,"full4364 "+label);
  generated++;retired+=status===0?count:0;const row={context:c.ordinal,owner:c.owner,name:"run",label,budget,status,args:args??[c.base,c.base+56,budget,c.base+96],before,after:capture(c,"after generated "+label)};journal.push(row);return row;
}
function window(address) {return address===0?[0,3]:address===0xfffffffe?[0xfffffffd,3]:[address-1,4];}
function store(c,label,value,reason=1) {
  const before=cpu(c),address=before.registers[7];assert.equal(before.registers[0],value>>>0,"current fullEAX independent literal");const w=window(address),beforeRam=readRam(c,...w,"before "+label);
  const next={registers:[...before.registers],pc:(before.pc+2)>>>0,flags:before.flags};next.registers[7]=(address+(before.flags&0x400?-2:2))>>>0;
  const call=run(c,1,label,next,reason),low=value&255,high=(value>>>8)&255;c.known.set(address,low);c.known.set(address+1,high);
  const afterRam=readRam(c,...w,"after "+label);stores.push({context:c.ordinal,owner:c.owner,entries:c.entries,label,value:value>>>0,address,before,next,run:call,beforeRam,afterRam});
}
function bind(c,count,entries) {
  const name=c.owner==="replacement"?(entries?"compile_entries":"compile"):(entries?"compile_resident_entries":"compile_resident");host(c,name,entries?[count,0]:[count]);
  if(c.owner==="replacement") return {generation:c.api.generation(),pointer:c.api.module_ptr()>>>0,length:c.api.module_len()>>>0};
  const b=arena(c);return Object.fromEntries(["low","high","pointer","length"].map((name,i)=>[name,b.readUInt32LE(148+i*4)]));
}
function install(c,binding,tail=false) {
  const b=Buffer.from(new Uint8Array(c.memory.buffer,binding.pointer,binding.length)),module=new WebAssembly.Module(b),guard=c.owner==="replacement"?"guard":"guard_resident",names=tail?[guard]:[guard,c.owner==="replacement"?"store16":"store_resident16"];
  assert.deepEqual(WebAssembly.Module.imports(module),[{module:"env",name:"memory",kind:"memory"},...names.map(name=>({module:"ring3",name,kind:"function"}))]);
  c.run=new WebAssembly.Instance(module,{env:{memory:c.memory},ring3:c.api}).exports.run;c.binding=binding;
  const file=`${c.owner}-${c.ordinal}${tail?"-tail":""}.wasm`;writeFileSync(join(output,file),b,{flag:"wx"});modules.push({context:c.ordinal,owner:c.owner,entries:c.entries,tail,file,...binding,bytes:b.length,sha256:hash(b),imports:names});
}
function open(owner,entries) {
  const instance=new WebAssembly.Instance(engine.module,{}),c={owner,entries,ordinal:++contexts,memory:instance.exports.memory,api:{},known:new Map()};
  for(const name of ["open","close","arena_ptr","map","unmap","protect","upload","read8","compile","compile_entries","compile_resident","compile_resident_entries","generation","module_ptr","module_len","guard","guard_resident","store16","store_resident16"]) c.api[name]=instance.exports["ring3_abi_v1_"+name];
  assert.equal(c.api.open(8,c.ordinal,0x53544f53),0);c.base=c.api.arena_ptr()>>>0;input(c,0,initial,"complete initial arena/opaqueFP");
  for(const args of [[PC,1,7],[DST,2,3],[0xfffff000,1,3],[0,1,3]])host(c,"map",args);
  upload(c,PC,bank);input(c,140,entries?words([PC,PC+16,PC+40]):words([PC,4,PC+16,16,PC+40,9]),"three bank descriptors");install(c,bind(c,3,entries));return c;
}
function close(c) {host(c,"close",[]);run(c,1,"closed5 retained-arena neutrality; no module pointer dereference",cpu(c),1,0,undefined,5);}
for(const owner of ["replacement","resident"])for(const entries of [false,true]) {
  const c=open(owner,entries);
  for(const flags of [2,0xcd7])for(const word of [0,0xff,0x8001,0xffff])for(const offset of [8,4095]) {
    const address=DST+offset;upload(c,address-1,Buffer.alloc(4,0x55));const value=(0xa53c0000|word)>>>0;seed(c,PC,flags,value,address);store(c,"64-core word/DF/aligned-or-crossing",value);
  }
  for(const [address,flags] of [[0,2],[0,0xcd7],[0xfffffffe,2],[0xfffffffe,0xcd7]]) {
    const w=window(address);upload(c,w[0],Buffer.alloc(w[1],0x55));seed(c,PC,flags,0x8172a691,address);store(c,"valid width2/full32 pointer wrap",0x8172a691);
  }
  upload(c,DST+13,Buffer.alloc(6,0x55));const chain=seed(c,PC+16,0xcd7,0xdead0011,DST+16);
  const first={...chain,registers:[...chain.registers],pc:PC+21};first.registers[0]=0x81a59123;run(c,1,"guest MOV first fullEAX",first,1,1,undefined,0,false);store(c,"retained first currentAX",0x81a59123);
  const second={...cpu(c),registers:[...cpu(c).registers],pc:PC+28};second.registers[0]=0xfedcba98;run(c,1,"guest MOV changes fullEAX without repair",second,1,1,undefined,0,false);store(c,"retained second currentAX",0xfedcba98);run(c,2,"chain JMP then cold",{...cpu(c),pc:PC+32},3,1,undefined,0,false);
  const cases=[{address:0x7008,at:0x7008,detail:1,map:[],window:[DST+7,4]},
    {address:0x7008,at:0x7008,detail:2,map:[[0x7000,1,3]],deny:0x7000,window:[0x7007,4]},
    {address:0x7fff,at:0x8000,detail:1,map:[[0x7000,1,3]],window:[0x7ffe,2]},
    {address:0x7fff,at:0x8000,detail:2,map:[[0x7000,2,3]],deny:0x8000,window:[0x7ffe,4]},
    {address:0x7fff,at:0x7fff,detail:2,map:[[0x7000,1,3]],deny:0x7000,window:[0x7ffe,2],twoRepairs:true},
    {address:0xffffffff,at:0xffffffff,detail:3,map:[],window:[0xfffffffe,2],overflow:true}];
  for(const f of cases) {
    for(const args of f.map)host(c,"map",args);upload(c,f.window[0],Buffer.alloc(f.window[1],0x55));if(f.deny)host(c,"protect",[f.deny,1,1]);readRam(c,...f.window,"failed-store sentinels before");
    const before=seed(c,PC+40,0xcd7,0xdeadbeef,f.address),after={registers:[...before.registers],pc:PC+45,flags:before.flags};after.registers[0]=0x13579bdf;const fault={detail:f.detail,address:f.at};
    run(c,2,"retired MOV then atomic word fault",after,5,1,fault);faults++;readRam(c,...f.window,"sentinels after first fault");run(c,1,"sameCPU target retry no retirement",after,5,0,fault);faults++;readRam(c,...f.window,"sentinels after retained fault");
    if(!f.overflow) {
      if(f.detail===2)host(c,"protect",[f.at&~0xfff,1,3]);else host(c,"map",[f.at&~0xfff,1,3]);
      if(f.twoRepairs)host(c,"map",[0x8000,1,3]);const w=window(f.address);upload(c,w[0],Buffer.alloc(w[1],0x55));assert.deepEqual(cpu(c),after,"repair retains target CPU/current AX");store(c,"sameCPU repair currentAX/no prefix replay",0x13579bdf);host(c,"unmap",[0x7000,f.address===0x7fff?2:1]);
    }
  }
  seed(c,PC,2,0xa53c8001,0x7008);run(c,0,"zero avoids unmapped word store",cpu(c),1,0);input(c,96,words([1]),"declared cancel");run(c,1,"cancel avoids word store",cpu(c),2,0);
  run(c,1,"invalid state pointer neutral",cpu(c),1,0,undefined,1,true,2,[0xffffffff,c.base+56,1,c.base+96]);close(c);
}
for(const owner of ["replacement","resident"])for(const same of [true,false]) {
  const c=open(owner,false),value=(0xa53c0000|(same?0xab66:0x9090))>>>0;seed(c,PC,2,value,PC);store(c,"same/different consumed code word commits then invalidation",value,6);
  const held=Buffer.from(new Uint8Array(c.memory.buffer,c.binding.pointer,c.binding.length));run(c,1,"stale4 full known-arena neutrality",cpu(c),1,0,undefined,4);
  assert.deepEqual(Buffer.from(new Uint8Array(c.memory.buffer,c.binding.pointer,c.binding.length)),held,"immediate live old allocation receipt before getter/allocation");
  input(c,140,words([PC+2,2]),"fresh current JMP tail descriptor no CPU repair");install(c,bind(c,1,false),true);run(c,2,"fresh tail continues after retired STOSW",{...cpu(c),pc:PC+4},3,1,undefined,0,false,1);close(c);
}
assert.deepEqual(sourcePins(),beforeSources);
const hostCounts=Object.fromEntries(["map","unmap","protect","upload","read8","compile","compile_entries","compile_resident","compile_resident_entries","close"].map(name=>[name,journal.filter(row=>row.name===name).length]));
assert.equal(contexts,8);assert.equal(modules.length,12);assert.equal(stores.length,112);assert.equal(faults,48);assert.equal(generated,200);assert.equal(retired,152);assert.equal(ram.length,296);
assert.deepEqual(hostCounts,{map:60,unmap:20,protect:24,upload:136,read8:1080,compile:5,compile_entries:1,compile_resident:5,compile_resident_entries:1,close:8});
assert.equal(journal.filter(row=>row.name==="input").length,276);assert.equal(journal.length,1816);assert.equal(frames.length,3632);
const raw=Buffer.concat(frames);writeFileSync(join(output,"arenas.bin"),raw,{flag:"wx"});
const result={status:"ok",engine:{bytes:engine.bytes.length,sha256:engine.sha256,caller_sha256:process.env.RING3_ENGINE_SHA256},source_pins:beforeSources,modules,stores,ram,journal,raw:{file:"arenas.bin",bytes:raw.length,sha256:hash(raw)},
  counts:{contexts,modules:modules.length,stores:stores.length,fault_calls:faults,generated_calls:generated,retired,raw_frames:frames.length,host_counts:hostCounts,host_inputs:journal.filter(row=>row.name==="input").length,journal_records:journal.length,physical_files:modules.length+5},
  limits:["exact66AB only; four bound profiles execute finite64-core inputs, no general operand16/REP", "full4364 saved arenas and selected live RAM/canary diagnostics; no whole RAM dump/body/timing certificate", "faults retain current AX/EDI/FLAGS/EIP, map/protect/RAM repairs are declared host inputs without prefix replay", "consumed-code same/different word stores retire before stale4; fresh guard-only JMP tail uses ExitV1", "post-stale allocation equality is a producer receipt, not independently saved after bytes; no post-close module pointer dereference; retained closed arena full neutral5"]};
writeFileSync(join(output,"result.json"),JSON.stringify(result,null,2),{flag:"wx"});console.log(JSON.stringify({status:result.status,...result.counts,output}));
