import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../support/engine.mjs';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root);
assert.match(process.env.RING3_ENGINE_SHA256 ?? '', /^[a-f0-9]{64}$/);
const engine = readEngine(enginePath), SIZE = 4364, FP = 4236, PC = 0x1000, DST = 0x5008;
const bank = Buffer.alloc(64, 0xcc);
for (const [offset, bytes] of [[0,[0xdd,0x3b,0xdb,0xe2,0xdd,0x7b,2,0xdf,0xe0,0xeb,0]], [32,[0xdd,0x3b,0xeb,0]], [40,[0x90,0xdd,0x3b,0xeb,0]]]) bank.set(bytes, offset);
assert.deepEqual(bank, readFileSync(join(output, 'status-memory.x86')), 'independent Rust/JS bank');
const STATUS = [[0,0,0],[0xffff,0xff,0xff],[0x81a5,0xa5,0x81],
  [1,1,0],[2,2,0],[4,4,0],[8,8,0],[0x10,0x10,0],[0x20,0x20,0],[0x40,0x40,0],[0x80,0x80,0],
  [0x100,0,1],[0x200,0,2],[0x400,0,4],[0x800,0,8],[0x1000,0,0x10],[0x2000,0,0x20],[0x4000,0,0x40],[0x8000,0,0x80]];
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const sourcePaths = ['engine/tests/cpu_x87_memory_status_wasm.rs','engine/tests/fixtures/p2-x87-memory-status/run.mjs','engine/tests/fixtures/support/engine.mjs'];
const sourcePins = () => Object.fromEntries(sourcePaths.map(path => [path, hash(readFileSync(join(root,path)))]));
const initialSources = sourcePins(), modules = [], rawRows = [], rawFrames = [];
const counts = {profiles:0,modules:0,words:0,clears:0,ax_reads:0,jumps:0,fault_calls:0,controls:0,preflight:0,generated_calls:0,retired:0,raw_pairs:0};
function record(magic,size,fields=[],version=1) {
  const bytes=Buffer.alloc(size); bytes.write(magic); bytes.writeUInt16LE(version,4); bytes.writeUInt16LE(1,6); bytes.writeUInt32LE(size,8);
  fields.forEach((value,index)=>bytes.writeUInt32LE(value>>>0,16+index*4)); return bytes;
}
const state = cpu => record('R3ST',56,[...cpu.registers,cpu.pc,cpu.flags]);
const exit = (reason,retired,detail=0,address=0,access=0,length=0,version=2) => record('R3EX',40,[reason,retired,detail,address,access,length],version);
const helper = (version,fields) => record('R3MH',40,fields,version);
const words = values => Buffer.concat(values.map(value=>{const bytes=Buffer.alloc(4); bytes.writeUInt32LE(value>>>0); return bytes;}));
function fpu(status,seed=0) {
  const bytes=record('R3FP',128); bytes.writeUInt16LE(seed&1?0xffff:0,16); bytes.writeUInt16LE(status,18);
  bytes.writeUInt16LE(0x55aa,20); bytes.writeUInt16LE(0x7ff,22); bytes.writeUInt32LE(0xf1234567,24); bytes.writeUInt32LE(0xfedcba98,28);
  bytes.writeUInt16LE(0xf135,32); bytes.writeUInt16LE(0xe246,34);
  bytes.set(Buffer.from(Array.from({length:80},(_,index)=>(index*73+seed*29+17)&255)),40); return bytes;
}
function initial() {
  const bytes=Buffer.alloc(SIZE); bytes.set(state({registers:Array(8).fill(0),pc:0,flags:2})); bytes.set(exit(1,0,0,0,0,0,1),56);
  bytes.set(helper(1,[0,0,0,0,0,0]),100); const fp=record('R3FP',128); fp.writeUInt16LE(0x37f,16); fp.writeUInt16LE(0xffff,20); bytes.set(fp,FP); return bytes;
}
function observed(ctx) {return Buffer.from(new Uint8Array(ctx.memory.buffer,ctx.base,SIZE));}
function check(ctx,label) {assert.deepEqual(observed(ctx),ctx.expected,`${ctx.owner}/${ctx.entries}/${label}`);}
function input(ctx,offset,bytes) {new Uint8Array(ctx.memory.buffer).set(bytes,ctx.base+offset); ctx.expected.set(bytes,offset); check(ctx,'explicit input');}
function host(ctx,name,...args) {check(ctx,`before ${name}`); assert.equal(ctx.api[name](...args),0,name); check(ctx,name);}
function upload(ctx,address,bytes) {input(ctx,140,bytes); host(ctx,'upload',address,bytes.length);}
function open(owner,entries) {
  const instance=new WebAssembly.Instance(engine.module,{}), ctx={owner,entries,memory:instance.exports.memory,api:{},expected:initial()};
  for(const [name,arity] of Object.entries({open:3,close:0,arena_ptr:0,map:3,unmap:2,protect:3,upload:2,read8:1,
    compile:1,compile_entries:2,compile_resident:1,compile_resident_entries:2,generation:0,module_ptr:0,module_len:0,
    guard:6,guard_resident:7,store16:2,store_resident16:6})) {
    const fn=instance.exports['ring3_abi_v1_'+name]; assert.equal(typeof fn,'function',name); assert.equal(fn.length,arity,name); ctx.api[name]=fn;
  }
  assert.equal(ctx.api.open(8,1,0x9abcde00),0); ctx.base=ctx.api.arena_ptr()>>>0; check(ctx,'default full arena');
  for(const address of [PC,0x5000,0xfffff000,0]) host(ctx,'map',address,1,address===PC?7:3);
  upload(ctx,PC,bank); counts.profiles++; return ctx;
}
function compile(ctx,bare=false) {
  const blocks=bare?[[PC+32,4]]:[[PC,11],[PC+32,4],[PC+40,5]];
  input(ctx,140,words(ctx.entries?blocks.map(block=>block[0]):blocks.flat())); let binding;
  if(ctx.owner==='replacement') {
    host(ctx,ctx.entries?'compile_entries':'compile',blocks.length,...(ctx.entries?[0]:[]));
    binding={generation:ctx.api.generation(),pointer:ctx.api.module_ptr()>>>0,length:ctx.api.module_len()>>>0};
  } else {
    check(ctx,'before resident compile'); assert.equal(ctx.entries?ctx.api.compile_resident_entries(blocks.length,0):ctx.api.compile_resident(blocks.length),0);
    const receipt=observed(ctx); assert.equal(receipt.readUInt32LE(140),1); assert.equal(receipt.readUInt32LE(144),24);
    binding=Object.fromEntries(['low','high','pointer','length'].map((name,index)=>[name,receipt.readUInt32LE(148+index*4)]));
    ctx.expected.set(words([1,24,binding.low,binding.high,binding.pointer,binding.length]),140); check(ctx,'resident metadata');
  }
  const bytes=Buffer.from(new Uint8Array(ctx.memory.buffer,binding.pointer,binding.length)), module=new WebAssembly.Module(bytes);
  const names=ctx.owner==='replacement'?['guard','store16']:['guard_resident','store_resident16'];
  assert.deepEqual(WebAssembly.Module.imports(module),[{module:'env',name:'memory',kind:'memory'},...names.map(name=>({module:'ring3',name,kind:'function'}))]);
  assert.deepEqual(WebAssembly.Module.exports(module),[{name:'run',kind:'function'}]);
  const unit={...binding,run:new WebAssembly.Instance(module,{env:{memory:ctx.memory},ring3:ctx.api}).exports.run}; assert.equal(unit.run.length,4);
  const file=`module-${counts.modules}.wasm`; writeFileSync(join(output,file),bytes,{flag:'wx'}); modules.push({owner:ctx.owner,entries:ctx.entries,bare,file,bytes:bytes.length,sha256:hash(bytes)}); counts.modules++; return unit;
}
function seed(ctx,pc,address,status,flags=2,ordinal=0) {
  const cpu={registers:[0xa53c1234,0x1234807f,0x23457f80,address,0x4567ff00,0x56789abc,0x6789abcd,0x789abcde],pc,flags};
  input(ctx,0,state(cpu)); input(ctx,56,exit(3,0)); input(ctx,96,words([0])); input(ctx,FP,fpu(status,ordinal)); ctx.cpu=cpu;
}
function run(ctx,unit,budget,next,reason=1,retired=1,nextHelper,nextFpu,status=0,fault) {
  check(ctx,'before generated'); const before=observed(ctx);
  if(status===0) {
    ctx.expected.set(state(next)); ctx.expected.set(exit(reason,retired,fault?.detail,fault?.address,fault?.access,fault?2:0),56);
    if(nextHelper)ctx.expected.set(nextHelper,100); if(nextFpu)ctx.expected.set(nextFpu,FP);
  }
  assert.equal(unit.run(ctx.base,ctx.base+56,budget,ctx.base+96),status); check(ctx,'after generated');
  counts.generated_calls++; counts.retired+=status===0?retired:0; if(status===0)ctx.cpu=next; return {before,after:observed(ctx)};
}
function diagnostic(ctx,address,literals) {
  const values=[];
  for(let index=0;index<literals.length;index++) {
    check(ctx,'before Read8 diagnostic'); assert.equal(ctx.api.read8((address+index)>>>0),0);
    ctx.expected.set(helper(2,[0,literals[index],0,0,0,1]),100); check(ctx,'literal Read8 diagnostic'); values.push(observed(ctx).readUInt32LE(120));
  }
  return values;
}
function store(ctx,unit,address,low,high,reason=1,diagnose=true) {
  const length=ctx.cpu.pc===PC+4?3:2, pc=ctx.cpu.pc;
  const frames=run(ctx,unit,1,{...ctx.cpu,pc:pc+length},reason,1,helper(4,[0,0,0,0,0,2])); counts.words++;
  const row={owner:ctx.owner,entries:ctx.entries,pc,length,address,reason,diagnostics:null}; rawRows.push(row); rawFrames.push(frames.before,frames.after);
  if(diagnose)row.diagnostics=diagnostic(ctx,address,[low,high]); return row;
}
function jump(ctx,unit,nextPC) {run(ctx,unit,2,{...ctx.cpu,pc:nextPC},3,1); counts.jumps++;}
function faults(ctx,unit) {
  for(const fault of [
    {address:0x7008,at:0x7008,detail:1}, {address:0x7008,at:0x7008,detail:2,permission:true},
    {address:0x7fff,at:0x8000,detail:1,second:true}, {address:0x9fff,at:0xa000,detail:2,second:true,permission:true},
  ]) {
    const page=fault.address&~0xfff, second=(page+4096)>>>0;
    if(fault.permission||fault.second) {
      host(ctx,'map',page,1,3); if(fault.second&&fault.permission)host(ctx,'map',second,1,3);
      upload(ctx,fault.address,Buffer.from(fault.second&&!fault.permission?[0x55]:[0x55,0x56]));
      if(fault.permission)host(ctx,'protect',fault.second?second:page,1,1);
    }
    seed(ctx,PC+40,fault.address,0x81a5,0xcd7);
    const target={...ctx.cpu,pc:PC+41};
    for(let attempt=0;attempt<2;attempt++) {
      run(ctx,unit,attempt?1:2,target,5,attempt?0:1,helper(4,[1,0,fault.detail,fault.at,2,2]),undefined,0,{detail:fault.detail,address:fault.at,access:2}); counts.fault_calls++;
    }
    if(fault.second&&!fault.permission)diagnostic(ctx,fault.address,[0x55]);
    if(fault.permission)diagnostic(ctx,fault.address,[0x55,0x56]);
    if(fault.permission)host(ctx,'protect',fault.second?second:page,1,3); else host(ctx,'map',fault.second?second:page,1,3);
    upload(ctx,fault.address,Buffer.from([0x55,0x56])); assert.deepEqual(ctx.cpu,target,'repair retains exact faulting CPU');
    store(ctx,unit,fault.address,0xa5,0x81);
    host(ctx,'unmap',page,1); if(fault.second)host(ctx,'unmap',second,1);
  }
  upload(ctx,0xffffffff,Buffer.from([0x55])); seed(ctx,PC+40,0xffffffff,0x81a5,0xcd7);
  for(let attempt=0;attempt<2;attempt++) {
    run(ctx,unit,attempt?1:2,{...ctx.cpu,pc:PC+41},5,attempt?0:1,helper(4,[1,0,3,0xffffffff,2,2]),undefined,0,{detail:3,address:0xffffffff,access:2}); counts.fault_calls++;
  }
  diagnostic(ctx,0xffffffff,[0x55]);
}
function controls(ctx,unit) {
  seed(ctx,PC+32,0x9008,0x81a5,0xcd7);
  for(const [cancel,budget,reason] of [[0,0,1],[1,1,2],[1,0,2]]) {
    input(ctx,96,words([cancel])); run(ctx,unit,budget,ctx.cpu,reason,0); counts.controls++;
  }
  input(ctx,96,words([0])); input(ctx,0,state({...ctx.cpu,pc:PC+64})); ctx.cpu={...ctx.cpu,pc:PC+64};
  run(ctx,unit,1,ctx.cpu,3,0); counts.controls++;
}
function barePreflight(ctx,unit) {
  seed(ctx,PC+32,DST,0x81a5);
  for(const offset of [0,4,6,8,12,36,120,127,22]) {
    const old=Buffer.from(ctx.expected.subarray(FP,FP+128)), bad=Buffer.from(old);
    if(offset===22)bad.writeUInt16LE(0x800,22); else bad[offset]^=1;
    input(ctx,FP,bad); run(ctx,unit,1,ctx.cpu,1,0,undefined,undefined,2); counts.preflight++; input(ctx,FP,old);
  }
}
function stale(ctx,unit) {
  const held=Buffer.from(new Uint8Array(ctx.memory.buffer,unit.pointer,unit.length)); check(ctx,'before stale callback');
  assert.equal(unit.run(-1,-1,0,-1),4); check(ctx,'stale callback'); counts.generated_calls++; counts.controls++;
  assert.deepEqual(Buffer.from(new Uint8Array(ctx.memory.buffer,unit.pointer,unit.length)),held,'live allocation unchanged immediately before getter/compile/close');
}
for(const owner of ['replacement','resident'])for(const entries of [false,true]) {
  const ctx=open(owner,entries); let unit=compile(ctx);
  for(const [ordinal,[status,low,high]] of STATUS.entries())for(const flags of [2,0xcd7]) {
    upload(ctx,DST-1,Buffer.from([0xa1,0x55,0x56,0xb2])); seed(ctx,PC+32,DST,status,flags,ordinal);
    store(ctx,unit,DST,low,high); diagnostic(ctx,DST-1,[0xa1,low,high,0xb2]); jump(ctx,unit,PC+36);
  }
  for(const flags of [2,0xcd7]) {
    upload(ctx,DST-1,Buffer.from([0xa1,0x55,0x56,0x57,0x58,0xb2])); seed(ctx,PC,DST,0x81a5,flags);
    store(ctx,unit,DST,0xa5,0x81);
    const next=Buffer.from(ctx.expected.subarray(FP,FP+128)); next.writeUInt16LE(0x100,18);
    run(ctx,unit,1,{...ctx.cpu,pc:PC+4},1,1,undefined,next); counts.clears++;
    store(ctx,unit,DST+2,0,1);
    const registers=[...ctx.cpu.registers]; registers[0]=0xa53c0100;
    run(ctx,unit,1,{...ctx.cpu,registers,pc:PC+9}); counts.ax_reads++;
    jump(ctx,unit,PC+11); diagnostic(ctx,DST-1,[0xa1,0xa5,0x81,0,1,0xb2]);
  }
  upload(ctx,0xfffffffd,Buffer.from([0xa1,0x55,0x56])); seed(ctx,PC+32,0xfffffffe,0x81a5);
  store(ctx,unit,0xfffffffe,0xa5,0x81); diagnostic(ctx,0xfffffffd,[0xa1,0xa5,0x81]);
  faults(ctx,unit); controls(ctx,unit); unit=compile(ctx,true); barePreflight(ctx,unit);
  for(const [address,status,low,high] of [[PC,0x3bdd,0xdd,0x3b],[PC-1,0xdd5a,0x5a,0xdd]]) {
    if(address===PC-1)upload(ctx,PC-1,Buffer.from([0x55])); seed(ctx,PC+32,address,status);
    const row=store(ctx,unit,address,low,high,6,false); stale(ctx,unit); row.diagnostics=diagnostic(ctx,address,[low,high]);
    if(address===PC)unit=compile(ctx,true);
  }
  host(ctx,'close'); check(ctx,'before closed callback'); assert.equal(unit.run(-1,-1,0,-1),5); check(ctx,'closed callback'); counts.generated_calls++; counts.controls++;
}
assert.deepEqual(counts,{profiles:4,modules:12,words:196,clears:8,ax_reads:8,jumps:160,fault_calls:40,controls:28,preflight:36,generated_calls:476,retired:392,raw_pairs:0});
assert.equal(rawRows.length,196); const raw=Buffer.concat(rawFrames); writeFileSync(join(output,'word-raw.bin'),raw,{flag:'wx'});
const physical=readFileSync(join(output,'word-raw.bin')); assert.equal(physical.length,196*2*SIZE);
for(let index=0;index<rawRows.length;index++) {
  const row=rawRows[index], before=physical.subarray(index*2*SIZE,(index*2+1)*SIZE), after=physical.subarray((index*2+1)*SIZE,(index*2+2)*SIZE);
  const pc=before.readUInt32LE(48); assert.equal(pc,row.pc); const offset=pc-PC;
  const length=offset===4?3:2; assert.deepEqual(bank.subarray(offset,offset+length),Buffer.from(offset===4?[0xdd,0x7b,2]:[0xdd,0x3b]));
  assert.equal(row.address,(before.readUInt32LE(28)+(offset===4?2:0))>>>0);
  const status=before.readUInt16LE(FP+18); let low=0,high=0;
  for(let bit=0;bit<8;bit++) {if(status&(1<<bit))low+=1<<bit; if(status&(1<<(bit+8)))high+=1<<bit;}
  assert.deepEqual(row.diagnostics,[low,high],'saved memory diagnostics agree with independently decomposed live before SW');
  const expected=Buffer.from(before); expected.writeUInt32LE(pc+length,48); expected.set(exit(row.reason,1),56); expected.set(helper(4,[0,0,0,0,0,2]),100);
  assert.deepEqual(after,expected,'independent saved whole-arena memory-status publication'); counts.raw_pairs++;
}
assert.deepEqual(sourcePins(),initialSources,'fixture sources unchanged');
const result={status:'ok',engine:{bytes:engine.bytes.length,sha256:engine.sha256},counts,modules,source_sha256:initialSources,
  raw:{file:'word-raw.bin',bytes:physical.length,sha256:hash(physical),rows:rawRows},
  claim:'finite no-wait DD/7 memory status, live FP128/current CPU, checked atomic word stores in four bound profiles; literal byte basis/sentinels/fault-repair/overflow/two-page SMC and saved before-bit oracle; native mock packets prove validator only, direct native bridge owns version-exhaustion; no waited forms/stack/arithmetic or performance claim'};
writeFileSync(join(output,'result.json'),JSON.stringify(result,null,2),{flag:'wx'}); console.log(JSON.stringify({status:result.status,counts,output}));
