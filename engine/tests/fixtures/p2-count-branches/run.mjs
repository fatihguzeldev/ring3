import assert from "node:assert/strict";
import {createHash} from "node:crypto";
import {readFileSync,writeFileSync} from "node:fs";
import {join} from "node:path";
import {readEngine} from "../support/engine.mjs";

const [enginePath,output,root]=process.argv.slice(2);
assert.ok(enginePath&&output&&root);assert.match(process.env.RING3_ENGINE_SHA256??"",/^[a-f0-9]{64}$/);
const engine=readEngine(enginePath),SIZE=4364,PC=0x1000,END=0x3000,TOP=0xfffffffe;
const hash=b=>createHash("sha256").update(b).digest("hex"),core=Buffer.alloc(64,0xcc),endpoints=Buffer.alloc(32,0xcc),wrap=Buffer.from([0xe2,0x80]);
const coreSpecs=[],endSpecs=[];
for(let i=0;i<4;i++){core.set([0xe0+i,4],i*8);core.set([0xe0+i,0xfe],32+i*8);coreSpecs.push([PC+i*8,2],[PC+32+i*8,2]);
  for(const [o,raw]of [[0,0x80],[4,0x7f]]){endpoints.set([0xe0+i,raw],i*8+o);endSpecs.push([END+i*8+o,2]);}}
for(const [name,b]of [["count-branches.x86",core],["endpoints.x86",endpoints],["wrap.x86",wrap]])assert.deepEqual(readFileSync(join(output,name)),b,"independent Rust/JS bank");
const paths=["engine/src/abi/arena.rs","engine/src/abi/x86/state.rs","engine/src/cpu/x86/ir.rs","engine/src/cpu/x86/decode/flow.rs",
  "engine/src/cpu/x86/decode/profile.rs","engine/src/cpu/x86/decode/decoder.rs","engine/src/cpu/dbt/region.rs","engine/src/cpu/dbt/cold.rs",
  "engine/src/cpu/dbt/wasm/integer.rs","engine/src/cpu/dbt/wasm/control.rs","engine/src/cpu/dbt/wasm/locals.rs","engine/src/cpu/dbt/wasm/abi.rs",
  "engine/tests/cpu_count_branches_wasm.rs","engine/tests/fixtures/p2-count-branches/run.mjs","engine/tests/fixtures/support/engine.mjs"];
const sourcePins=()=>Object.fromEntries(paths.map(path=>{const b=readFileSync(join(root,path));return[path,{bytes:b.length,sha256:hash(b)}];})),sources=sourcePins();
const frames=[],journal=[],modules=[],targets=[];
const counts={owners:0,modules:0,core_targets:0,branch_retired:0,generated_calls:0,controls:0,cold_calls:0};
writeFileSync(join(output,"engine.wasm"),engine.bytes,{flag:"wx"});
function record(magic,size,fields){const b=Buffer.alloc(size);b.write(magic);b.writeUInt16LE(1,4);b.writeUInt16LE(1,6);b.writeUInt32LE(size,8);
  fields.forEach((v,i)=>b.writeUInt32LE(v>>>0,16+i*4));return b;}
const state=c=>record("R3ST",56,[...c.registers,c.pc,c.flags]),exit=(reason,n)=>record("R3EX",40,[reason,n,0,0,0,0]);
const words=values=>Buffer.concat(values.map(v=>{const b=Buffer.alloc(4);b.writeUInt32LE(v>>>0);return b;}));
const arena=ctx=>Buffer.from(new Uint8Array(ctx.memory.buffer,ctx.base,SIZE));
function capture(ctx,label){const b=arena(ctx),row={owner:ctx.owner,label,offset:frames.length*SIZE,sha256:hash(b)};frames.push(b);return row;}
function cpu(ctx){const b=arena(ctx);return{registers:Array.from({length:8},(_,i)=>b.readUInt32LE(16+i*4)),pc:b.readUInt32LE(48),flags:b.readUInt32LE(52)};}
function input(ctx,offset,b,label){const before=capture(ctx,"before input "+label),old=arena(ctx);new Uint8Array(ctx.memory.buffer).set(b,ctx.base+offset);old.set(b,offset);
  assert.deepEqual(arena(ctx),old);journal.push({owner:ctx.owner,type:"input",offset,hex:b.toString("hex"),label,before,after:capture(ctx,"after input "+label)});}
function host(ctx,name,args){const old=arena(ctx),before=capture(ctx,"before "+name);assert.equal(ctx.api[name](...args),0,name);assert.deepEqual(arena(ctx),old,"host arena neutral "+name);
  journal.push({owner:ctx.owner,type:"host",name,args,before,after:capture(ctx,"after "+name)});}
function upload(ctx,pc,b){input(ctx,140,b,"uploaded bank bytes");host(ctx,"upload",[pc,b.length]);}
function seed(ctx,pc,ecx,flags){const c={registers:[0xa1b2c3d4,ecx,0x34560f10,0x4567ff00,0x56789abc,0x6789abcd,0x789abcde,0x89abcdef],pc,flags};
  const b=Buffer.alloc(140);b.set(state(c));b.set(exit(3,0),56);b.set(record("R3MH",40,[0,0xdecafbad,0,0,0,0]),100);input(ctx,0,b,"declared CPU input");return c;}
function step(c,opcode,raw){const next={registers:[...c.registers],pc:(c.pc+2)>>>0,flags:c.flags};
  if(opcode!==0xe3)next.registers[1]=(c.registers[1]-1)>>>0;
  const take=opcode===0xe3?c.registers[1]===0:next.registers[1]!==0&&(opcode===0xe2||(opcode===0xe1?!!(c.flags&0x40):!(c.flags&0x40)));
  if(take)next.pc=(next.pc+(raw<128?raw:raw-256))>>>0;return next;}
function run(ctx,budget,label,next,reason,n,status=0){const old=arena(ctx),before=capture(ctx,"before generated "+label);
  if(status===0){old.set(state(next));old.set(exit(reason,n),56);}assert.equal(ctx.run(ctx.base,ctx.base+56,budget,ctx.base+96),status,label);assert.deepEqual(arena(ctx),old,"full generated arena "+label);
  const row={owner:ctx.owner,type:"run",module:ctx.module,header_version:1,label,budget,status,reason:status?null:reason,retired:status?0:n,before,after:capture(ctx,"after generated "+label)};
  journal.push(row);counts.generated_calls++;counts.branch_retired+=status?0:n;return row;}
function branch(ctx,budget,label,opcode,raw,n=1,reason=1){const before=cpu(ctx);let next=before;for(let i=0;i<n;i++)next=step(next,opcode,raw);
  const row=run(ctx,budget,label,next,reason,n);targets.push({owner:ctx.owner,label,opcode,raw,iterations:n,before:row.before,after:row.after,module:ctx.module});return next;}
function cold(ctx){run(ctx,1,"spare budget observes cold selected PC",cpu(ctx),3,0);counts.cold_calls++;}
function compile(ctx,specs,entries,bankName){input(ctx,140,words(entries?specs.map(([pc])=>pc):specs.flat()),"compiler descriptors/entry roots");let binding;
  const name=ctx.owner==="resident"?(entries?"compile_resident_entries":"compile_resident"):(entries?"compile_entries":"compile");
  const args=entries?[specs.length,0]:[specs.length],old=arena(ctx),before=capture(ctx,"before "+name);assert.equal(ctx.api[name](...args),0,name);
  if(ctx.owner==="resident"){const b=arena(ctx);assert.equal(b.readUInt32LE(140),1);assert.equal(b.readUInt32LE(144),24);
    binding=Object.fromEntries(["low","high","pointer","length"].map((name,i)=>[name,b.readUInt32LE(148+i*4)]));assert.ok(binding.low&&binding.length);old.set(b.subarray(140,164),140);}
  else binding={generation:ctx.api.generation(),pointer:ctx.api.module_ptr()>>>0,length:ctx.api.module_len()>>>0};
  assert.deepEqual(arena(ctx),old,"only resident compiler receipt may change");journal.push({owner:ctx.owner,type:"host",name,args,before,after:capture(ctx,"after "+name)});
  const b=Buffer.from(new Uint8Array(ctx.memory.buffer,binding.pointer,binding.length)),m=new WebAssembly.Module(b),guard=ctx.owner==="resident"?"guard_resident":"guard";
  assert.deepEqual(WebAssembly.Module.imports(m),[{module:"env",name:"memory",kind:"memory"},{module:"ring3",name:guard,kind:"function"}]);
  assert.deepEqual(WebAssembly.Module.exports(m),[{name:"run",kind:"function"}]);ctx.run=new WebAssembly.Instance(m,{env:{memory:ctx.memory},ring3:ctx.api}).exports.run;assert.equal(ctx.run.length,4);
  ctx.binding=binding;ctx.module=modules.length;const file="module-"+ctx.module+".wasm";writeFileSync(join(output,file),b,{flag:"wx"});
  modules.push({owner:ctx.owner,file,bytes:b.length,sha256:hash(b),entries,bank:bankName,specs,imports:[guard],...binding});counts.modules++;}
function open(owner){const instance=new WebAssembly.Instance(engine.module,{}),ctx={owner,memory:instance.exports.memory,api:{}};
  for(const name of ["open","close","arena_ptr","map","upload","compile","compile_entries","compile_resident","compile_resident_entries","generation","module_ptr","module_len","guard","guard_resident"]){ctx.api[name]=instance.exports["ring3_abi_v1_"+name];assert.equal(typeof ctx.api[name],"function",name);}
  assert.equal(ctx.api.open(4,1,0x62636465),0);ctx.base=ctx.api.arena_ptr()>>>0;
  for(const pc of [PC,END,0xfffff000])host(ctx,"map",[pc,1,7]);upload(ctx,PC,core);
  const fp=record("R3FP",128,[0x55aa037f,0x03ffffff,0xf1234567,0xfedcba98,0xe246f135]);fp.set(Buffer.from(Array.from({length:80},(_,i)=>(i*73+29)&255)),40);
  input(ctx,4236,fp,"literal opaque FP128");compile(ctx,coreSpecs,false,"count-branches.x86");counts.owners++;return ctx;}

for(const owner of ["replacement","resident"]){const ctx=open(owner);
  for(let i=0;i<4;i++)for(const ecx of [0,1,2,0x10000,0x10001,0x80000000,0xffffffff])for(const flags of [2,0x42,0xcd7,0xc97]){
    seed(ctx,PC+i*8,ecx,flags);branch(ctx,1,"live fullECX/ZF core",0xe0+i,4);counts.core_targets++;if(ecx===0&&flags===2)cold(ctx);}
  for(let i=0;i<4;i++){seed(ctx,PC+i*8,0,0xcd7);run(ctx,0,"budget0 before decrement",cpu(ctx),1,0);counts.controls++;
    input(ctx,96,words([1]),"cancel before decrement");run(ctx,1,"cancel before decrement",cpu(ctx),2,0);counts.controls++;input(ctx,96,words([0]),"restore cancel");}
  const valid=arena(ctx).subarray(0,56),bad=Buffer.from(valid);bad.writeUInt32LE(0,52);input(ctx,0,bad,"malformed FLAGS");run(ctx,1,"malformed state before decrement",cpu(ctx),1,0,2);counts.controls++;input(ctx,0,valid,"restore valid state");
  for(let i=0;i<3;i++){seed(ctx,PC+32+i*8,3,i===1?0x42:2);branch(ctx,1,"retained loop first iteration",0xe0+i,0xfe);
    branch(ctx,3,"retained loop remaining two then cold",0xe0+i,0xfe,2,3);}
  seed(ctx,PC+56,0,0xcd7);branch(ctx,2,"bounded JECXZ self-loop never decrements",0xe3,0xfe,2);
  input(ctx,20,words([1]),"declared current ECX edit after bounded JECXZ");branch(ctx,2,"JECXZ nonzero falls through then cold",0xe3,0xfe,1,3);
  const held=Buffer.from(new Uint8Array(ctx.memory.buffer,ctx.binding.pointer,ctx.binding.length));upload(ctx,PC,Buffer.from([0xe0]));
  run(ctx,0,"same-byte upload stales known owner before budget",cpu(ctx),1,0,4);counts.controls++;
  assert.deepEqual(Buffer.from(new Uint8Array(ctx.memory.buffer,ctx.binding.pointer,ctx.binding.length)),held,"live stale allocation unchanged before getter/compile/close");
  upload(ctx,END,endpoints);compile(ctx,endSpecs,true,"endpoints.x86");
  for(let i=0;i<4;i++)for(const [offset,raw]of [[0,0x80],[4,0x7f]]){seed(ctx,END+i*8+offset,i===3?0:2,i===1?0x42:0xcd7&~0x40);
    branch(ctx,1,"signed rel8 endpoint",0xe0+i,raw);cold(ctx);}
  upload(ctx,TOP,wrap);compile(ctx,[[TOP,2]],true,"wrap.x86");seed(ctx,TOP,2,0xcd7);branch(ctx,1,"nextPC wraps before signed displacement",0xe2,0x80);cold(ctx);
  host(ctx,"close",[]);run(ctx,0,"closed known arena is neutral",cpu(ctx),1,0,5);counts.controls++;
}
assert.deepEqual(counts,{owners:2,modules:6,core_targets:224,branch_retired:266,generated_calls:306,controls:22,cold_calls:26});
assert.equal(targets.length,258);assert.equal(journal.filter(r=>r.type==="input").length,296);assert.equal(journal.filter(r=>r.type==="host").length,22);assert.equal(frames.length,1248);
assert.deepEqual(sourcePins(),sources,"source epoch unchanged");const raw=Buffer.concat(frames);writeFileSync(join(output,"arenas.bin"),raw,{flag:"wx"});
const result={status:"ok",engine:{bytes:engine.bytes.length,sha256:engine.sha256},counts,source_pins:sources,modules,targets,journal,
  raw:{file:"arenas.bin",frames:frames.length,bytes:raw.length,sha256:hash(raw)},limits:["finite four flat32 rel8 forms, no address16/prefix/segment/recursiveCFG extension",
    "224core rows and bounded retained loops are qualified inputs, not exhaustive ECX/FLAGS","full4364 arena pairs; no RAM writes/reads or hardware/performance claim",
    "actual bound execution; native six profiles only validate generated modules","stale live allocation compared before release/recompile; no closed-allocation proof","open/getter calls outside arena journal; all generated calls and declared host edits saved"]};
writeFileSync(join(output,"result.json"),JSON.stringify(result,null,2),{flag:"wx"});console.log(JSON.stringify({status:result.status,...counts,raw_frames:frames.length,output}));
