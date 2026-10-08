import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../support/engine.mjs';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root);
assert.match(process.env.RING3_ENGINE_SHA256 ?? '', /^[a-f0-9]{64}$/);
const engine = readEngine(enginePath), SIZE=4364, FP=4236, PC=0x1000, TABLE=0x5000;
const bank=Buffer.alloc(32,0xcc);
for(const [offset,bytes] of [[0,[0xd7,0xd7,0xeb,0]],[8,[0xd7,0xeb,0]],[16,[0x90,0xd7,0xeb,0]]])bank.set(bytes,offset);
const table=Buffer.from(Array.from({length:256},(_,index)=>(index*73+29)&255));
assert.deepEqual(bank,readFileSync(join(output,'xlat.x86')),'independent Rust/JS bank');
assert.deepEqual(table,readFileSync(join(output,'table.bin')),'independent Rust/JS table');
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
const sourcePaths=['engine/tests/cpu_xlat_wasm.rs','engine/tests/fixtures/p2-xlat/run.mjs','engine/tests/fixtures/support/engine.mjs',
  'engine/src/cpu/x86/ir.rs','engine/src/cpu/x86/decode/lower.rs','engine/src/cpu/dbt/region.rs',
  'engine/src/cpu/dbt/wasm/integer.rs','engine/src/cpu/dbt/wasm/memory.rs','engine/src/cpu/dbt/wasm/memory/narrow.rs'];
const sourcePins=()=>Object.fromEntries(sourcePaths.map(path=>[path,hash(readFileSync(join(root,path)))]));
const initialSources=sourcePins(), modules=[], events=[], rows=[], frames=[];
const counts={profiles:0,modules:0,translations:0,all_al:0,chained:0,boundaries:0,repairs:0,jumps:0,nops:0,fault_calls:0,controls:0,generated_calls:0,retired:0,raw_pairs:0};
function record(magic,size,fields=[],version=1){const b=Buffer.alloc(size);b.write(magic);b.writeUInt16LE(version,4);b.writeUInt16LE(1,6);b.writeUInt32LE(size,8);fields.forEach((v,i)=>b.writeUInt32LE(v>>>0,16+i*4));return b;}
const words=values=>Buffer.concat(values.map(v=>{const b=Buffer.alloc(4);b.writeUInt32LE(v>>>0);return b;}));
const state=cpu=>record('R3ST',56,[...cpu.registers,cpu.pc,cpu.flags]);
const exit=(reason,retired,detail=0,address=0,access=0,length=0)=>record('R3EX',40,[reason,retired,detail,address,access,length],2);
const helper=(value,detail=0,address=0)=>record('R3MH',40,detail?[1,0,detail,address,1,1]:[0,value,0,0,0,1],2);
function initial(){const b=Buffer.alloc(SIZE);b.set(state({registers:Array(8).fill(0),pc:0,flags:2}));b.set(record('R3EX',40,[1,0,0,0,0,0]),56);b.set(record('R3MH',40),100);const fp=record('R3FP',128);fp.writeUInt16LE(0x37f,16);fp.writeUInt16LE(0xffff,20);b.set(fp,FP);return b;}
function opaqueFpu(seed){const b=record('R3FP',128);b.writeUInt16LE(0x27f,16);b.writeUInt16LE(0x81a5,18);b.writeUInt16LE(0x55aa,20);b.writeUInt16LE(0x7ff,22);b.writeUInt32LE(0xf1234567,24);b.writeUInt32LE(0xfedcba98,28);b.writeUInt16LE(0xf135,32);b.writeUInt16LE(0xe246,34);b.set(Buffer.from(Array.from({length:80},(_,i)=>(i*73+seed*29+17)&255)),40);return b;}
function observed(ctx){return Buffer.from(new Uint8Array(ctx.memory.buffer,ctx.base,SIZE));}
function check(ctx,label){assert.deepEqual(observed(ctx),ctx.expected,`${ctx.owner}/${ctx.entries}/${label}`);}
function input(ctx,offset,bytes){new Uint8Array(ctx.memory.buffer).set(bytes,ctx.base+offset);ctx.expected.set(bytes,offset);events.push({profile:ctx.profile,kind:'input',offset,hex:Buffer.from(bytes).toString('hex')});check(ctx,'explicit input');}
function host(ctx,name,...args){check(ctx,`before ${name}`);assert.equal(ctx.api[name](...args),0,name);events.push({profile:ctx.profile,kind:'host',name,args});check(ctx,name);}
function upload(ctx,address,bytes){input(ctx,140,bytes);host(ctx,'upload',address,bytes.length);}
function open(owner,entries){
  const instance=new WebAssembly.Instance(engine.module,{}),ctx={owner,entries,profile:`${owner}/${entries?'entry':'explicit'}`,memory:instance.exports.memory,api:{},expected:initial()};
  for(const [name,arity] of Object.entries({open:3,close:0,arena_ptr:0,map:3,unmap:2,protect:3,upload:2,read8:1,compile:1,compile_entries:2,compile_resident:1,compile_resident_entries:2,generation:0,module_ptr:0,module_len:0,guard:6,guard_resident:7})){const fn=instance.exports['ring3_abi_v1_'+name];assert.equal(typeof fn,'function',name);assert.equal(fn.length,arity,name);ctx.api[name]=fn;}
  assert.equal(ctx.api.open(6,1,0x9abcde00),0);ctx.base=ctx.api.arena_ptr()>>>0;check(ctx,'default full arena');
  for(const address of [PC,TABLE,0xfffff000,0])host(ctx,'map',address,1,address===PC?7:3);
  upload(ctx,PC,bank);upload(ctx,TABLE,table);input(ctx,FP,opaqueFpu(counts.profiles));counts.profiles++;return ctx;
}
function compile(ctx){
  const blocks=[[PC,4],[PC+8,3],[PC+16,4]];input(ctx,140,words(ctx.entries?blocks.map(b=>b[0]):blocks.flat()));let binding;
  if(ctx.owner==='replacement'){host(ctx,ctx.entries?'compile_entries':'compile',3,...(ctx.entries?[0]:[]));binding={generation:ctx.api.generation(),pointer:ctx.api.module_ptr()>>>0,length:ctx.api.module_len()>>>0};}
  else{check(ctx,'before resident compile');assert.equal(ctx.entries?ctx.api.compile_resident_entries(3,0):ctx.api.compile_resident(3),0);const receipt=observed(ctx);assert.equal(receipt.readUInt32LE(140),1);assert.equal(receipt.readUInt32LE(144),24);binding=Object.fromEntries(['low','high','pointer','length'].map((n,i)=>[n,receipt.readUInt32LE(148+i*4)]));ctx.expected.set(words([1,24,binding.low,binding.high,binding.pointer,binding.length]),140);check(ctx,'resident metadata');events.push({profile:ctx.profile,kind:'compile_resident',entries:ctx.entries,binding});}
  const bytes=Buffer.from(new Uint8Array(ctx.memory.buffer,binding.pointer,binding.length)),module=new WebAssembly.Module(bytes);
  const names=[ctx.owner==='replacement'?'guard':'guard_resident','read8'];assert.deepEqual(WebAssembly.Module.imports(module),[{module:'env',name:'memory',kind:'memory'},...names.map(name=>({module:'ring3',name,kind:'function'}))]);assert.deepEqual(WebAssembly.Module.exports(module),[{name:'run',kind:'function'}]);
  const unit={...binding,run:new WebAssembly.Instance(module,{env:{memory:ctx.memory},ring3:ctx.api}).exports.run};assert.equal(unit.run.length,4);
  const file=`module-${counts.modules}.wasm`;writeFileSync(join(output,file),bytes,{flag:'wx'});modules.push({profile:ctx.profile,file,bytes:bytes.length,sha256:hash(bytes),binding});counts.modules++;return unit;
}
function seed(ctx,pc,base,al,flags=2){ctx.cpu={registers:[(0xa53c7b00|al)>>>0,0x1234807f,0x23457f80,base>>>0,0x4567ff00,0x56789abc,0x6789abcd,0x789abcde],pc,flags};input(ctx,0,state(ctx.cpu));input(ctx,56,exit(3,0));input(ctx,96,words([0]));}
function run(ctx,unit,label,budget,next,reason=1,retired=1,nextHelper,status=0,fault,pointers){
  check(ctx,'before generated');const before=observed(ctx),index=rows.length;
  if(status===0){ctx.expected.set(state(next));ctx.expected.set(exit(reason,retired,fault?.detail,fault?.address,fault?1:0,fault?1:0),56);if(nextHelper)ctx.expected.set(nextHelper,100);}
  assert.equal(unit.run(...(pointers??[ctx.base,ctx.base+56,budget,ctx.base+96])),status);check(ctx,'after generated');
  const after=observed(ctx);rows.push({profile:ctx.profile,label,budget,status,reason,retired,frame:index*2,pointers:pointers??null});frames.push(before,after);events.push({profile:ctx.profile,kind:'run',row:index});counts.generated_calls++;counts.retired+=status===0?retired:0;if(status===0)ctx.cpu=next;return {before,after};
}
function translate(ctx,unit,label,value){const next={...ctx.cpu,registers:ctx.cpu.registers.slice(),pc:(ctx.cpu.pc+1)>>>0};next.registers[0]=((next.registers[0]&0xffffff00)|value)>>>0;run(ctx,unit,label,1,next,1,1,helper(value));counts.translations++;}
function diagnostic(ctx,address,value){check(ctx,'before Read8 diagnostic');assert.equal(ctx.api.read8(address>>>0),0);ctx.expected.set(helper(value),100);events.push({profile:ctx.profile,kind:'diagnostic',address:address>>>0,value});check(ctx,'literal RAM diagnostic');}
function faultCases(ctx,unit){
  for(const permission of [false,true]){
    if(permission){host(ctx,'map',0x7000,1,3);upload(ctx,0x7008,Buffer.from([0x77]));host(ctx,'protect',0x7000,1,2);}
    seed(ctx,PC+16,0x7000,8,0xcd7);const fault={detail:permission?2:1,address:0x7008},next={...ctx.cpu,pc:PC+17};
    run(ctx,unit,permission?'permission-prefix':'unmapped-prefix',2,next,5,1,helper(0,fault.detail,fault.address),0,fault);counts.nops++;counts.fault_calls++;
    run(ctx,unit,permission?'permission-retry':'unmapped-retry',1,next,5,0,helper(0,fault.detail,fault.address),0,fault);counts.fault_calls++;
    if(permission)host(ctx,'protect',0x7000,1,3);else host(ctx,'map',0x7000,1,3);
    upload(ctx,0x7008,Buffer.from([0x66]));assert.deepEqual(ctx.cpu,next,'repair retains current faulting CPU');translate(ctx,unit,permission?'permission-repair':'unmapped-repair',0x66);counts.repairs++;diagnostic(ctx,0x7008,0x66);host(ctx,'unmap',0x7000,1);
  }
}
function controls(ctx,unit){
  seed(ctx,PC+8,0x9000,8,0xcd7);
  for(const [cancel,budget,reason] of [[0,0,1],[1,1,2],[1,0,2]]){input(ctx,96,words([cancel]));run(ctx,unit,`control-${cancel}-${budget}`,budget,ctx.cpu,reason,0);counts.controls++;}
  input(ctx,96,words([0]));ctx.cpu={...ctx.cpu,pc:PC+32};input(ctx,0,state(ctx.cpu));run(ctx,unit,'cold',1,ctx.cpu,3,0);counts.controls++;
  upload(ctx,PC+8,Buffer.from([0xd7]));const held=Buffer.from(new Uint8Array(ctx.memory.buffer,unit.pointer,unit.length));
  run(ctx,unit,'stale',0,ctx.cpu,1,0,undefined,4,undefined,[-1,-1,0,-1]);counts.controls++;
  assert.deepEqual(Buffer.from(new Uint8Array(ctx.memory.buffer,unit.pointer,unit.length)),held,'live allocation unchanged before getter/close');
  host(ctx,'close');run(ctx,unit,'closed',0,ctx.cpu,1,0,undefined,5,undefined,[-1,-1,0,-1]);counts.controls++;
}
for(const owner of ['replacement','resident'])for(const entries of [false,true]){
  const ctx=open(owner,entries),unit=compile(ctx);
  for(let al=0;al<256;al++){seed(ctx,PC+8,TABLE,al,al&1?0xcd7:2);translate(ctx,unit,`all-al-${al}`,table[al]);counts.all_al++;}
  for(const al of [1,0x80])for(const flags of [2,0xcd7]){
    seed(ctx,PC,TABLE,al,flags);const first=table[al],second=table[first];translate(ctx,unit,`chain-${al}-${flags}-first`,first);translate(ctx,unit,`chain-${al}-${flags}-second`,second);counts.chained+=2;
    run(ctx,unit,`chain-${al}-${flags}-jump`,2,{...ctx.cpu,pc:PC+4},3,1);counts.jumps++;
  }
  for(const [base,al,address,value] of [[0xffffff00,255,0xffffffff,0x31],[0xffffffff,1,0,0x80],[0xffffffff,0,0xffffffff,0xe7],[0,0,0,0xa6]]){
    upload(ctx,address,Buffer.from([value]));seed(ctx,PC+8,base,al,0xcd7);translate(ctx,unit,`boundary-${base}-${al}`,value);counts.boundaries++;diagnostic(ctx,address,value);
  }
  faultCases(ctx,unit);controls(ctx,unit);
}
assert.deepEqual(counts,{profiles:4,modules:4,translations:1080,all_al:1024,chained:32,boundaries:16,repairs:8,jumps:16,nops:8,fault_calls:16,controls:24,generated_calls:1136,retired:1104,raw_pairs:0});
const raw=Buffer.concat(frames);writeFileSync(join(output,'arenas.bin'),raw,{flag:'wx'});const physical=readFileSync(join(output,'arenas.bin'));assert.equal(physical.length,rows.length*2*SIZE);
for(let index=0;index<rows.length;index++){
  const row=rows[index],before=physical.subarray(index*2*SIZE,(index*2+1)*SIZE),after=physical.subarray((index*2+1)*SIZE,(index*2+2)*SIZE);
  if(row.status!==0){assert.deepEqual(after,before,'saved guard-neutral full arena');continue;}
  const expected=Buffer.from(before),pc=before.readUInt32LE(48),offset=pc-PC;let next=pc,retired=0,reason=1;
  if(row.label.startsWith('control-'))reason=before.readUInt32LE(96)?2:1;
  else if(row.label==='cold')reason=3;
  else if(row.label.endsWith('-retry')){reason=5;expected.set(helper(0,row.label.startsWith('permission')?2:1,0x7008),100);}
  else if(row.label.endsWith('-prefix')){assert.equal(bank[offset],0x90);assert.equal(bank[offset+1],0xd7);next=pc+1;retired=1;reason=5;expected.set(helper(0,row.label.startsWith('permission')?2:1,0x7008),100);}
  else if(row.label.endsWith('-jump')){assert.deepEqual(bank.subarray(offset,offset+2),Buffer.from([0xeb,0]));next=pc+2;retired=1;reason=3;}
  else{
    assert.equal(bank[offset],0xd7);const address=(before.readUInt32LE(28)+(before.readUInt32LE(16)&255))>>>0;
    let value;if(address>=TABLE&&address<TABLE+256)value=readFileSync(join(output,'table.bin'))[address-TABLE];else if(address===0x7008)value=0x66;else if(address===0xffffffff)value=row.label.endsWith('-255')?0x31:0xe7;else if(address===0)value=row.label==='boundary-4294967295-1'?0x80:0xa6;else assert.fail('unknown physical XLAT address');
    expected.writeUInt32LE(((before.readUInt32LE(16)&0xffffff00)|value)>>>0,16);expected.set(helper(value),100);next=pc+1;retired=1;
  }
  expected.writeUInt32LE(next>>>0,48);expected.set(exit(reason,retired,reason===5?(row.label.startsWith('permission')?2:1):0,reason===5?0x7008:0,reason===5?1:0,reason===5?1:0),56);
  assert.deepEqual(after,expected,'saved opcode-selected full arena recomputation');counts.raw_pairs++;
}
assert.equal(counts.raw_pairs,1128);assert.deepEqual(sourcePins(),initialSources,'fixture sources unchanged');writeFileSync(join(output,'engine.wasm'),engine.bytes,{flag:'wx'});
const result={status:'ok',engine:{bytes:engine.bytes.length,sha256:engine.sha256},counts,modules,source_sha256:initialSources,events,raw:{file:'arenas.bin',bytes:physical.length,sha256:hash(physical),rows},claim:'finite all256 unsigned oldAL values across four bound profiles, retained currentAL chains, modulo32 top/wrap endpoints, faulting-EIP/no-AL-publication and sameCPU changed-RAM repair; full4364 saved generated-call pairs preserve all unrelated bytes/FP128; RAM diagnostics are live observations, saved table and ordered uploads provide bounded RAM history; no standalone memory admission, FP execution, new ABI/helper or performance claim'};
writeFileSync(join(output,'result.json'),JSON.stringify(result,null,2),{flag:'wx'});console.log(JSON.stringify({status:result.status,counts,output}));
