import assert from "node:assert/strict";
import {createHash} from "node:crypto";
import {readFileSync,writeFileSync} from "node:fs";
import {join} from "node:path";
import {readEngine} from "../support/engine.mjs";

const [enginePath,output,root]=process.argv.slice(2);
assert.ok(enginePath&&output&&root); assert.match(process.env.RING3_ENGINE_SHA256??"",/^[a-f0-9]{64}$/);
const engine=readEngine(enginePath), SIZE=4364, PC=0x1000, SRC=0x5008, DST=0x6008;
const hash=bytes=>createHash("sha256").update(bytes).digest("hex"), bank=Buffer.alloc(64,0xcc);
const blocks=[[0,[0xa6,0xeb,0]],[8,[0xae,0xeb,0]],[16,[0xac,0xae,0xac,0xae,0xeb,0]],
  [24,[0xa6,0xa6,0xeb,0]],[32,[0x90,0xa6,0xeb,0]],[40,[0x90,0xae,0xeb,0]]];
for(const [offset,bytes] of blocks)bank.set(bytes,offset);
assert.deepEqual(bank,readFileSync(join(output,"string-compare.x86")),"independent Rust/JS bank");
const paths=["engine/src/abi/arena.rs","engine/src/cpu/x86/ir.rs","engine/src/cpu/x86/decode/lower.rs",
  "engine/src/cpu/x86/decode/profile.rs","engine/src/cpu/dbt/region.rs","engine/src/cpu/dbt/wasm/integer.rs",
  "engine/src/cpu/dbt/wasm/memory.rs","engine/src/cpu/dbt/wasm/memory/narrow.rs",
  "engine/tests/cpu_string_compare_wasm.rs","engine/tests/fixtures/p2-string-compare/run.mjs","engine/tests/fixtures/support/engine.mjs"];
const sourcePins=()=>Object.fromEntries(paths.map(path=>{const b=readFileSync(join(root,path));return [path,{bytes:b.length,sha256:hash(b)}];}));
const sources=sourcePins(), frames=[],journal=[],modules=[],targets=[];
const counts={profiles:0,modules:0,cmpsb:0,scasb:0,lodsb:0,jumps:0,fault_calls:0,controls:0,generated_calls:0,retired:0};
writeFileSync(join(output,"engine.wasm"),engine.bytes,{flag:"wx"});
function record(magic,size,fields,version=1) {
  const b=Buffer.alloc(size);b.write(magic);b.writeUInt16LE(version,4);b.writeUInt16LE(1,6);b.writeUInt32LE(size,8);
  fields.forEach((v,i)=>b.writeUInt32LE(v>>>0,16+i*4));return b;
}
const state=c=>record("R3ST",56,[...c.registers,c.pc,c.flags]);
const exit=(reason,n,detail=0,address=0)=>record("R3EX",40,[reason,n,detail,address,detail?1:0,detail?1:0],2);
const helper=(value,detail=0,address=0)=>record("R3MH",40,[detail?1:0,detail?0:value,detail,detail?address:0,detail?1:0,1],2);
const words=values=>Buffer.concat(values.map(v=>{const b=Buffer.alloc(4);b.writeUInt32LE(v>>>0);return b;}));
function arena(ctx){return Buffer.from(new Uint8Array(ctx.memory.buffer,ctx.base,SIZE));}
function capture(ctx,label){const b=arena(ctx),row={profile:ctx.profile,label,offset:frames.length*SIZE,sha256:hash(b)};frames.push(b);return row;}
function cpu(ctx){const b=arena(ctx);return {registers:Array.from({length:8},(_,i)=>b.readUInt32LE(16+i*4)),pc:b.readUInt32LE(48),flags:b.readUInt32LE(52)};}
function input(ctx,offset,b,label){const before=capture(ctx,"before input "+label),old=arena(ctx);new Uint8Array(ctx.memory.buffer).set(b,ctx.base+offset);
  old.set(b,offset);assert.deepEqual(arena(ctx),old);journal.push({profile:ctx.profile,type:"input",offset,hex:b.toString("hex"),label,before,after:capture(ctx,"after input "+label)});}
function host(ctx,name,args,expectedHelper){const old=arena(ctx),before=capture(ctx,"before "+name);assert.equal(ctx.api[name](...args),0,name);
  if(expectedHelper)old.set(expectedHelper,100);assert.deepEqual(arena(ctx),old,"full host neutrality/declared helper "+name);
  journal.push({profile:ctx.profile,type:"host",name,args,before,after:capture(ctx,"after "+name)});}
function upload(ctx,address,bytes){input(ctx,140,bytes,"uploaded literal bytes");host(ctx,"upload",[address,bytes.length]);}
function read(ctx,address,value){host(ctx,"read8",[address],helper(value));assert.equal(arena(ctx).readUInt32LE(120),value,"declared physical byte");}
function seed(ctx,pc,flags,source=SRC,destination=DST,al=0x5a){const c={registers:[(0xa1b2c300|al)>>>0,0xb1c2d378,0x34560f10,0x4567ff00,0x56789abc,0x6789abcd,source,destination],pc,flags};
  const b=Buffer.alloc(140);b.set(state(c));b.set(exit(3,0),56);b.set(record("R3MH",40,[0,0xdecafbad,0,0,0,0]),100);input(ctx,0,b,"explicit initial CPU");return c;}
function run(ctx,budget,label,next,reason=1,n=1,nextHelper,fault,status=0){const beforeBytes=arena(ctx),before=capture(ctx,"before generated "+label),expected=Buffer.from(beforeBytes);
  if(status===0){expected.set(state(next));expected.set(exit(reason,n,fault?.detail,fault?.address),56);if(nextHelper)expected.set(nextHelper,100);}
  assert.equal(ctx.run(ctx.base,ctx.base+56,budget,ctx.base+96),status,label);assert.deepEqual(arena(ctx),expected,"full generated arena "+ctx.profile+"/"+label);
  counts.generated_calls++;counts.retired+=status===0?n:0;const row={profile:ctx.profile,type:"run",label,budget,status,reason,n,before,after:capture(ctx,"after generated "+label)};journal.push(row);return row;}
function compare(ctx,kind,left,right,flags,label){const before=cpu(ctx),next={registers:[...before.registers],pc:(before.pc+1)>>>0,flags:flags|(before.flags&0x400)};
  const delta=before.flags&0x400?-1:1;if(kind==="cmpsb")next.registers[6]=(next.registers[6]+delta)>>>0;
  else assert.equal(before.registers[0]&255,left,"current AL is the declared left operand");next.registers[7]=(next.registers[7]+delta)>>>0;
  const row=run(ctx,1,label,next,1,1,helper(right));counts[kind]++;targets.push({profile:ctx.profile,kind,label,left,right,expected_flags:next.flags,before:row.before,after:row.after});return next;}
function load(ctx,value){const before=cpu(ctx),next={...before,registers:[...before.registers],pc:before.pc+1};
  next.registers[0]=((before.registers[0]&0xffffff00)|value)>>>0;next.registers[6]=(before.registers[6]+(before.flags&0x400?-1:1))>>>0;
  run(ctx,1,"real LODSB supplies current AL",next,1,1,helper(value));counts.lodsb++;}
function jump(ctx,next){run(ctx,2,"JMP with spare budget then cold continuation",{...cpu(ctx),pc:next},3,1);counts.jumps++;}
function open(owner,entries){const instance=new WebAssembly.Instance(engine.module,{}),ctx={owner,entries,profile:owner+"/"+(entries?"entry":"explicit"),memory:instance.exports.memory,api:{}};
  for(const name of ["open","close","arena_ptr","map","unmap","protect","upload","read8","compile","compile_entries",
    "compile_resident","compile_resident_entries","generation","module_ptr","module_len","guard","guard_resident"]){ctx.api[name]=instance.exports["ring3_abi_v1_"+name];assert.equal(typeof ctx.api[name],"function",name);}
  assert.equal(ctx.api.open(8,1,0x51525354),0);ctx.base=ctx.api.arena_ptr()>>>0;
  for(const address of [PC,0x5000,0x6000,0xfffff000,0])host(ctx,"map",[address,1,address===PC?7:3]);upload(ctx,PC,bank);
  input(ctx,140,words(entries?blocks.map(([o])=>PC+o):blocks.flatMap(([o,b])=>[PC+o,b.length])),"six compiler entries/descriptors");
  let binding;if(owner==="replacement") {host(ctx,entries?"compile_entries":"compile",entries?[6,0]:[6]);binding={generation:ctx.api.generation(),pointer:ctx.api.module_ptr()>>>0,length:ctx.api.module_len()>>>0};}
  else {const old=arena(ctx),before=capture(ctx,"before resident compile");assert.equal(entries?ctx.api.compile_resident_entries(6,0):ctx.api.compile_resident(6),0);
    const b=arena(ctx);assert.equal(b.readUInt32LE(140),1);assert.equal(b.readUInt32LE(144),24);
    binding=Object.fromEntries(["low","high","pointer","length"].map((name,i)=>[name,b.readUInt32LE(148+i*4)]));assert.ok(binding.low&&binding.length);
    old.set(b.subarray(140,164),140);assert.deepEqual(b,old,"only declared resident receipt changes");journal.push({profile:ctx.profile,type:"host",name:"compile_resident",before,after:capture(ctx,"after resident compile")});}
  const bytes=Buffer.from(new Uint8Array(ctx.memory.buffer,binding.pointer,binding.length)),module=new WebAssembly.Module(bytes),names=[owner==="resident"?"guard_resident":"guard","read8"];
  assert.deepEqual(WebAssembly.Module.imports(module),[{module:"env",name:"memory",kind:"memory"},...names.map(name=>({module:"ring3",name,kind:"function"}))]);
  assert.deepEqual(WebAssembly.Module.exports(module),[{name:"run",kind:"function"}]);ctx.run=new WebAssembly.Instance(module,{env:{memory:ctx.memory},ring3:ctx.api}).exports.run;assert.equal(ctx.run.length,4);ctx.binding=binding;
  const file="module-"+modules.length+".wasm";writeFileSync(join(output,file),bytes,{flag:"wx"});modules.push({profile:ctx.profile,file,bytes:bytes.length,sha256:hash(bytes),imports:names,...binding});counts.profiles++;counts.modules++;return ctx;}

// all six status flags are defined; these byte subtraction literals are independent of helper output.
const PAIRS=[[0,0,0x46],[0,1,0x97],[1,0,2],[0x7f,0xff,0x883],[0x80,1,0x812],[0x10,1,0x16],[0xff,0xff,0x46],[0xff,1,0x82]];
for(const owner of ["replacement","resident"])for(const entries of [false,true]) {
  const ctx=open(owner,entries),fp=record("R3FP",128,[0x55aa037f,0x03ffffff,0xf1234567,0xfedcba98,0xe246f135]);
  fp.set(Buffer.from(Array.from({length:80},(_,i)=>(i*73+29)&255)),40);input(ctx,4236,fp,"literal opaque FP128 preserved by integer reads");
  for(const kind of ["cmpsb","scasb"])for(const initialFlags of [2,0x402,0x8d7,0xcd7])for(const [left,right,flags] of PAIRS) {
    upload(ctx,SRC,Buffer.from([left]));upload(ctx,DST,Buffer.from([right]));const entry=PC+(kind==="cmpsb"?0:8);
    seed(ctx,entry,initialFlags,kind==="cmpsb"?SRC:0x9008,DST,kind==="scasb"?left:0x5a);compare(ctx,kind,left,right,flags,"literal six-flags subtraction");
    read(ctx,SRC,left);read(ctx,DST,right);jump(ctx,entry+3);
  }
  for(const kind of ["cmpsb","scasb"])for(const [address,initialFlags] of [[0xffffffff,2],[0,0xcd7],[SRC,2]]) {
    upload(ctx,address,Buffer.from([0x80]));seed(ctx,PC+(kind==="cmpsb"?0:8),initialFlags,address,address,kind==="scasb"?0x80:0x5a);
    compare(ctx,kind,0x80,0x80,0x46,"wrapping or identical pointers");read(ctx,address,0x80);
  }
  for(const flags of [2,0xcd7]) {
    const delta=flags&0x400?-1:1;for(const [address,value] of [[SRC,0x7f],[SRC+delta,0x80],[DST,0xff],[DST+delta,1]])upload(ctx,address,Buffer.from([value]));
    seed(ctx,PC+24,flags);compare(ctx,"cmpsb",0x7f,0xff,0x883,"first CMPS uses old indices");compare(ctx,"cmpsb",0x80,1,0x812,"second CMPS uses current indices");jump(ctx,PC+28);
    seed(ctx,PC+16,flags);load(ctx,0x7f);compare(ctx,"scasb",0x7f,0xff,0x883,"SCAS uses preceding real LODSB AL");
    load(ctx,0x80);compare(ctx,"scasb",0x80,1,0x812,"second SCAS uses current AL without reseed");jump(ctx,PC+22);
  }
  for(const fault of [{kind:"cmpsb",side:"source",address:0x7008,detail:1},{kind:"cmpsb",side:"source",address:0x7008,detail:2},
    {kind:"cmpsb",side:"destination",address:0x8008,detail:1},{kind:"cmpsb",side:"destination",address:0x8008,detail:2},
    {kind:"scasb",side:"destination",address:0x8008,detail:1},{kind:"scasb",side:"destination",address:0x8008,detail:2}]) {
    const page=fault.address&~0xfff,source=fault.side==="source"?fault.address:SRC,destination=fault.side==="destination"?fault.address:DST;
    upload(ctx,SRC,Buffer.from([0x81]));upload(ctx,DST,Buffer.from([1]));
    if(fault.detail===2){host(ctx,"map",[page,1,3]);upload(ctx,fault.address,Buffer.from([fault.side==="source"?0x81:1]));host(ctx,"protect",[page,1,2]);}
    const entry=PC+(fault.kind==="cmpsb"?32:40),initial=seed(ctx,entry,0xcd7,source,destination,0x81),target={...initial,pc:entry+1};
    for(let attempt=0;attempt<2;attempt++){run(ctx,attempt?1:2,"faulting read retains indices/FLAGS after NOP prefix",target,5,attempt?0:1,helper(0,fault.detail,fault.address),fault);counts.fault_calls++;}
    if(fault.side==="source")read(ctx,DST,1);else read(ctx,SRC,0x81);
    host(ctx,fault.detail===2?"protect":"map",[page,1,3]);upload(ctx,fault.address,Buffer.from([fault.side==="source"?0x81:1]));
    let left=0x81,right=1,expectedFlags=0x82;
    if(fault.kind==="cmpsb"&&fault.side==="destination"){upload(ctx,SRC,Buffer.from([0x7f]));upload(ctx,destination,Buffer.from([0xff]));left=0x7f;right=0xff;expectedFlags=0x883;}
    assert.deepEqual(cpu(ctx),target,"host-only repair retains exact target CPU");compare(ctx,fault.kind,left,right,expectedFlags,"same-CPU repair rereads current RAM");
    read(ctx,source,left);read(ctx,destination,right);host(ctx,"unmap",[page,1]);
  }
  const initial=seed(ctx,PC,0xcd7,0x7008,0x8008),first={detail:1,address:0x7008},second={detail:1,address:0x8008};
  for(let attempt=0;attempt<2;attempt++){run(ctx,1,"both unmapped: explicit engine ESI-first read order",initial,5,0,helper(0,1,0x7008),first);counts.fault_calls++;}
  host(ctx,"map",[0x7000,1,3]);upload(ctx,0x7008,Buffer.from([0x81]));
  for(let attempt=0;attempt<2;attempt++){run(ctx,1,"second read fault cannot commit first byte or flags",initial,5,0,helper(0,1,0x8008),second);counts.fault_calls++;}
  host(ctx,"map",[0x8000,1,3]);upload(ctx,0x8008,Buffer.from([1]));assert.deepEqual(cpu(ctx),initial);
  compare(ctx,"cmpsb",0x81,1,0x82,"priority repair reuses current owner and EIP");host(ctx,"unmap",[0x7000,1]);host(ctx,"unmap",[0x8000,1]);
  for(const entry of [PC,PC+8]){seed(ctx,entry,2,0x9008,0xa008);run(ctx,0,"budget0 avoids reads",cpu(ctx),1,0);counts.controls++;
    input(ctx,96,words([1]),"explicit cancellation");run(ctx,1,"cancel avoids reads",cpu(ctx),2,0);counts.controls++;}
  input(ctx,96,words([0]),"restore cancellation before lifecycle");upload(ctx,PC,Buffer.from([0xa6]));
  const held=Buffer.from(new Uint8Array(ctx.memory.buffer,ctx.binding.pointer,ctx.binding.length));
  run(ctx,0,"stale known owner preserves full arena",cpu(ctx),1,0,undefined,undefined,4);counts.controls++;
  assert.deepEqual(Buffer.from(new Uint8Array(ctx.memory.buffer,ctx.binding.pointer,ctx.binding.length)),held,"live allocation unchanged before getters or close");
  host(ctx,"close",[]);run(ctx,0,"closed known arena preserves full arena",cpu(ctx),1,0,undefined,undefined,5);counts.controls++;
}
assert.deepEqual(counts,{profiles:4,modules:4,cmpsb:176,scasb:164,lodsb:16,jumps:272,fault_calls:64,controls:24,generated_calls:716,retired:652});
assert.equal(targets.length,340);assert.equal(journal.filter(row=>row.type==="input").length,1036);
assert.equal(journal.filter(row=>row.type==="host").length,1408);assert.equal(frames.length,6320);
assert.deepEqual(sourcePins(),sources,"fixture sources unchanged");
const raw=Buffer.concat(frames);writeFileSync(join(output,"arenas.bin"),raw,{flag:"wx"});
const result={status:"ok",engine:{bytes:engine.bytes.length,sha256:engine.sha256},counts,source_pins:sources,modules,targets,journal,
  raw:{file:"arenas.bin",frames:frames.length,bytes:raw.length,sha256:hash(raw)},limits:["finite prefix-free CMPSB/SCASB; no REP/wider/address16/segment extension",
    "all saved full4364 arena pairs; literal physical byte diagnostics are not a full RAM dump","native tests decode/validate modules; this fixture executes actual generated Wasm",
    "ESI-first read-fault priority is engine contract, not universal hardware claim","live stale allocation receipt only before close; no closed pointer lifetime or performance claim"]};
writeFileSync(join(output,"result.json"),JSON.stringify(result,null,2),{flag:"wx"});console.log(JSON.stringify({status:result.status,...counts,raw_frames:frames.length,output}));
