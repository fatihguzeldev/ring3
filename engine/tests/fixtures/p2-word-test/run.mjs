import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync,readdirSync,writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../support/engine.mjs';

const [enginePath,output,root]=process.argv.slice(2);assert.ok(enginePath&&output&&root);
assert.match(process.env.RING3_ENGINE_SHA256??'',/^[a-f0-9]{64}$/);
const engine=readEngine(enginePath),SIZE=4364,FLAGS=[2,0xcd7],BASES=[0x1000,0x3000,0x5000,0x7000];
const hash=b=>createHash('sha256').update(b).digest('hex'),initial=readFileSync(join(output,'initial-arena.bin'));
assert.equal(initial.length,SIZE);assert.deepEqual(readdirSync(output).sort(),['accumulator.x86','chain.x86','initial-arena.bin','modrm.x86','registers.x86']);
const parents=[0xa53c8001,0xb64dffff,0xc75e7fff,0xd86f00ff,0xe9708000,0xfa810000,0x1b925555,0x2ca3aaaa],banks=[];
for(const [index,name]of ['registers','modrm','accumulator','chain'].entries()){
  const bytes=[],instructions=[];function test(alias,right,value){const code=right!==null?[0x66,0x85,0xc0|right<<3|alias]:alias===null?[0x66,0xa9,value&255,value>>>8]:[0x66,0xf7,0xc0|alias,value&255,value>>>8];
    instructions.push({offset:bytes.length,length:code.length,hex:Buffer.from(code).toString('hex')});bytes.push(...code);}
  if(index===0)for(let alias=0;alias<8;alias++)for(const right of [alias,(alias+1)&7])test(alias,right,0);
  if(index===1)for(let alias=0;alias<8;alias++)for(const value of [0,0x7fff,0x8000,0xffff])test(alias,null,value);
  if(index===2)for(const value of [0,0x7fff,0x8000,0xffff,0x6667,0x6766,0xf0f3,0xf3f2])test(null,null,value);
  if(index===3)bytes.push(0x66,0xb8,0,0x80,0x66,0xa9,0xff,0xff,0x0f,0x92,0xc2,0x0f,0x90,0xc6,0x0f,0x94,0xc3,0x75,2);
  else bytes.push(0xeb,0);
  const body=bytes.length;bytes.push(0x0f,0x0b);if(index===3)bytes.push(0x0f,0x0b);
  const b=Buffer.from(bytes);assert.deepEqual(readFileSync(join(output,name+'.x86')),b,'independent Rust/JS literal bank');banks.push({name,pc:BASES[index],body,bytes:b,instructions});
}
assert.deepEqual(banks.map(b=>[b.bytes.length,b.body,b.instructions.length]),[[52,50,16],[164,162,32],[36,34,8],[23,19,0]]);
const sourcePaths=['engine/src/cpu/x86/ir.rs','engine/src/cpu/x86/decode/profile.rs','engine/src/cpu/x86/decode/operands.rs','engine/src/cpu/x86/decode/lower.rs','engine/src/cpu/dbt/region.rs','engine/src/cpu/dbt/wasm/integer.rs',
  'engine/src/abi/arena.rs','engine/src/abi/memory_helper.rs','engine/src/abi/x86/state.rs','engine/src/abi/x86/exit.rs','engine/src/abi/x86/x87.rs','engine/src/abi/header.rs','engine/src/cpu/dbt/wasm/abi.rs',
  'engine/src/cpu/dbt/wasm/emitter.rs','engine/src/cpu/dbt/wasm/locals.rs','engine/src/cpu/dbt/wasm/memory/narrow.rs','engine/src/cpu/dbt/wasm/memory/word_store.rs','engine/src/cpu/x86/decode/decoder.rs',
  'engine/src/cpu/x86/x87.rs','engine/src/memory/space.rs','engine/src/process/instance.rs','engine/src/process/resident.rs','engine/src/process/wasm.rs','Cargo.toml','Cargo.lock','engine/Cargo.toml',
  'engine/tests/cpu_word_test.rs','engine/tests/cpu_word_test_wasm.rs','engine/tests/fixtures/p2-word-test/run.mjs','engine/tests/cpu_byte_compare.rs','engine/tests/cpu_byte_test.rs','engine/tests/dbt_logical.rs'];
const pin=path=>{const b=readFileSync(join(root,path));return{path,bytes:b.length,sha256:hash(b)};},pins=()=>Object.fromEntries(sourcePaths.map(p=>[p,pin(p).sha256]));
const sourcePins=pins(),support=pin('engine/tests/fixtures/support/engine.mjs'),frames=[],rawRecords=[],journal=[],modules=[],targets=[],contexts=[],lifecycle=[],sourceChanges=[];
const counts={contexts:0,modules:0,tests:0,bank_tests:0,chain_tests:0,moves:0,jumps:0,suffix_instructions:0,seeds:0,generated_calls:0,retired:0,controls:0,inputs:0,hosts:0,uploads:0,upload_bytes:0};
function artifact(name,b){writeFileSync(join(output,name),b,{flag:'wx'});return{file:name,bytes:b.length,sha256:hash(b)};}artifact('engine.wasm',engine.bytes);
function record(magic,size,fields){const b=Buffer.alloc(size);b.write(magic);b.writeUInt16LE(1,4);b.writeUInt16LE(1,6);b.writeUInt32LE(size,8);fields.forEach((v,i)=>b.writeUInt32LE(v>>>0,16+i*4));return b;}
const state=c=>record('R3ST',56,[...c.registers,c.pc,c.flags]),exit=(reason,n)=>record('R3EX',40,[reason,n,0,0,0,0]);
const words=values=>{const b=Buffer.alloc(values.length*4);values.forEach((v,i)=>b.writeUInt32LE(v>>>0,i*4));return b;};
const arena=ctx=>Buffer.from(new Uint8Array(ctx.memory.buffer,ctx.base,SIZE));
function frame(ctx,label){const b=arena(ctx);assert.deepEqual(b,ctx.expected,ctx.profile+'/'+label);const r={index:frames.length,context:ctx.context,label,file:'arenas.bin',offset:frames.length*SIZE,bytes:SIZE,sha256:hash(b)};frames.push(b);rawRecords.push(r);return r;}
function input(ctx,offset,b,label){assert.ok(offset>=0&&offset+b.length<=SIZE);const before=frame(ctx,'before input '+label);new Uint8Array(ctx.memory.buffer).set(b,ctx.base+offset);ctx.expected.set(b,offset);
  const row={context:ctx.context,type:'input',label,offset,hex:b.toString('hex'),before,after:frame(ctx,'after input '+label)};journal.push(row);counts.inputs++;return row;}
function host(ctx,name,args){const before=frame(ctx,'before '+name);assert.equal(ctx.api[name](...args),0,name);const row={context:ctx.context,type:'host',name,args,before,after:frame(ctx,'after '+name)};journal.push(row);counts.hosts++;return row;}
function upload(ctx,bank,offset,b,label){input(ctx,140,b,label);const row=host(ctx,'upload',[bank.pc+offset,b.length]);ctx.code[bank.name].set(b,offset);counts.uploads++;counts.upload_bytes+=b.length;return row;}
function profile(b,owner){let at=8;assert.deepEqual([...b.subarray(0,8)],[0,97,115,109,1,0,0,0]);
  const leb=()=>{let v=0,s=1;for(let i=0;i<5;i++){assert.ok(at<b.length);const x=b[at++];v+=(x&127)*s;if(!(x&128)){assert.ok(v<=0xffffffff);return v;}s*=128;}assert.fail('bounded LEB');};
  const text=()=>{const n=leb();assert.ok(at+n<=b.length);const t=b.subarray(at,at+n).toString('utf8');at+=n;return t;};
  const sections=new Map();while(at<b.length){const id=b[at++],n=leb();assert.ok(at+n<=b.length&&!sections.has(id));sections.set(id,[at,at+n]);at+=n;}
  assert.deepEqual([...sections.keys()],[1,2,3,7,10]);const start=id=>{at=sections.get(id)[0];},end=id=>assert.equal(at,sections.get(id)[1]);
  start(1);const types=[];for(let i=0,n=leb();i<n;i++){assert.equal(b[at++],0x60);const p=[],r=[];for(let j=0,n=leb();j<n;j++)p.push(b[at++]);for(let j=0,n=leb();j<n;j++)r.push(b[at++]);types.push({parameters:p,results:r});}end(1);
  assert.deepEqual(types,[{parameters:Array(4).fill(127),results:[127]},{parameters:Array(owner==='resident'?7:6).fill(127),results:[127]}]);
  start(2);assert.equal(leb(),2);assert.equal(text(),'env');assert.equal(text(),'memory');assert.equal(b[at++],2);assert.equal(leb(),0);assert.equal(leb(),1);
  assert.equal(text(),'ring3');assert.equal(text(),owner==='resident'?'guard_resident':'guard');assert.equal(b[at++],0);assert.equal(leb(),1);end(2);
  start(3);assert.equal(leb(),1);assert.equal(leb(),0);end(3);start(7);assert.equal(leb(),1);assert.equal(text(),'run');assert.equal(b[at++],0);assert.equal(leb(),1);end(7);
  start(10);assert.equal(leb(),1);const bodyBytes=leb(),bodyEnd=at+bodyBytes;assert.equal(bodyEnd,sections.get(10)[1]);const locals=[];for(let i=0,n=leb();i<n;i++)locals.push([leb(),b[at++]]);
  assert.deepEqual(locals,[[16,127],[1,126]]);assert.ok(at<bodyEnd);assert.equal(b[bodyEnd-1],0x0b);return{types,locals,body_bytes:bodyBytes,sections:[...sections.keys()]};}
function compile(ctx,bank,label){input(ctx,140,words(ctx.entries?[bank.pc]:[bank.pc,bank.body]),'compile request '+label);const before=frame(ctx,'before compile '+label);
  const name=ctx.owner==='resident'?(ctx.entries?'compile_resident_entries':'compile_resident'):(ctx.entries?'compile_entries':'compile'),args=ctx.entries?[1,0]:[1];assert.equal(ctx.api[name](...args),0,name);let binding;
  if(ctx.owner==='resident'){const b=arena(ctx);assert.deepEqual([b.readUInt32LE(140),b.readUInt32LE(144)],[1,24]);binding=Object.fromEntries(['low','high','pointer','length'].map((n,i)=>[n,b.readUInt32LE(148+i*4)]));
    assert.ok(binding.low||binding.high);ctx.expected.set(words([1,24,binding.low,binding.high,binding.pointer,binding.length]),140);assert.equal(ctx.api.generation(),0);assert.equal(ctx.api.module_ptr(),0);assert.equal(ctx.api.module_len(),0);}
  else binding={generation:ctx.api.generation(),pointer:ctx.api.module_ptr()>>>0,length:ctx.api.module_len()>>>0};
  const after=frame(ctx,'after compile '+label);journal.push({context:ctx.context,type:'host',name,args,before,after,binding});counts.hosts++;
  assert.ok(binding.pointer>0&&binding.length>8&&binding.length<=65536&&binding.pointer+binding.length<=ctx.memory.buffer.byteLength);
  const b=Buffer.from(new Uint8Array(ctx.memory.buffer,binding.pointer,binding.length)),m=new WebAssembly.Module(b),guard=ctx.owner==='resident'?'guard_resident':'guard';
  const imports=[{module:'env',name:'memory',kind:'memory'},{module:'ring3',name:guard,kind:'function'}];assert.deepEqual(WebAssembly.Module.imports(m),imports);assert.deepEqual(WebAssembly.Module.exports(m),[{name:'run',kind:'function'}]);
  const abi=profile(b,ctx.owner),saved=artifact('module-'+modules.length+'.wasm',b),run=new WebAssembly.Instance(m,{env:{memory:ctx.memory},ring3:{[guard]:ctx.api[guard]}}).exports.run;assert.equal(run.length,4);
  const row={index:modules.length,context:ctx.context,owner:ctx.owner,profile:ctx.profile,entries:ctx.entries,bank:bank.name,pc:bank.pc,body:bank.body,source_hex:ctx.code[bank.name].subarray(0,bank.body).toString('hex'),imports,abi,binding,...saved};modules.push(row);counts.modules++;
  ctx.unit={...binding,index:row.index,run,bytes:b};return row;}
function cpu(ctx){const b=arena(ctx);return{registers:Array.from({length:8},(_,i)=>b.readUInt32LE(16+i*4)),pc:b.readUInt32LE(48),flags:b.readUInt32LE(52)};}
function seed(ctx,bank,flags){const c={registers:[...parents],pc:bank.pc,flags};const b=Buffer.alloc(140);b.set(state(c));b.set(exit(3,0),56);b.set(record('R3MH',40,[0,0xdecafbad,0,0,0,0]),100);input(ctx,0,b,'one retained bank CPU seed');counts.seeds++;}
function run(ctx,budget,label,next,reason,n,status=0,pointers=[ctx.base,ctx.base+56,ctx.base+96]){const before=frame(ctx,'before generated '+label);if(status===0){ctx.expected.set(state(next));ctx.expected.set(exit(reason,n),56);}
  assert.equal(ctx.unit.run(pointers[0],pointers[1],budget,pointers[2]),status,label);const row={context:ctx.context,type:'run',module:ctx.unit.index,label,budget,pointers,status,reason:status?null:reason,retired:status?0:n,before,after:frame(ctx,'after generated '+label)};
  journal.push(row);counts.generated_calls++;counts.retired+=status?0:n;return row;}
function logical(a,b,old){const result=(a&b)&0xffff;let parity=0;for(let i=0;i<8;i++)parity+=(result>>>i)&1;return (old&0x400)|2|((parity%2===0)?4:0)|(result===0?0x40:0)|(result&0x8000?0x80:0);}
assert.deepEqual([logical(0x8000,0xffff,0xcd7),logical(0x8000,0x7fff,2),logical(1,1,0xcd7),logical(0x100,0x100,2)],[0x486,0x46,0x402,6]);
function test(ctx,bank,label,chain=false){const old=cpu(ctx),at=old.pc-bank.pc,code=ctx.code[bank.name];assert.equal(code[at],0x66);let alias,right,length;
  if(code[at+1]===0x85){assert.equal(code[at+2]&0xc0,0xc0);alias=code[at+2]&7;right=old.registers[(code[at+2]>>>3)&7];length=3;}
  else if(code[at+1]===0xa9){alias=0;right=code.readUInt16LE(at+2);length=4;}
  else{assert.equal(code[at+1],0xf7);assert.equal(code[at+2]&0xf8,0xc0);alias=code[at+2]&7;right=code.readUInt16LE(at+3);length=5;}
  const next={registers:[...old.registers],pc:(old.pc+length)>>>0,flags:logical(old.registers[alias],right,old.flags)},row=run(ctx,1,label,next,1,1);
  targets.push({context:ctx.context,module:ctx.unit.index,bank:bank.name,encoded_hex:code.subarray(at,at+length).toString('hex'),before:row.before,after:row.after});counts.tests++;counts[chain?'chain_tests':'bank_tests']++;}
function cold(ctx,bank){const old=cpu(ctx);assert.equal(old.pc,bank.pc+bank.body-2);assert.deepEqual([...ctx.code[bank.name].subarray(bank.body-2,bank.body)],[0xeb,0]);
  run(ctx,2,'JMP then cold with spare budget',{...old,registers:[...old.registers],pc:bank.pc+bank.body},3,1);counts.jumps++;}
function invalidate(ctx,bank,offset,b,label){const unit=ctx.unit;host(ctx,'protect',[bank.pc,1,7]);const receipt=upload(ctx,bank,offset,b,label);host(ctx,'protect',[bank.pc,1,4]);
  const live=Buffer.from(new Uint8Array(ctx.memory.buffer,unit.pointer,unit.length));assert.deepEqual(live,unit.bytes,'immediate live allocation receipt before getter/compile/close');
  sourceChanges.push({context:ctx.context,label,bank:bank.name,offset,hex:b.toString('hex'),module:unit.index,upload_after:receipt.after,allocation_before_sha256:hash(unit.bytes),allocation_after_sha256:hash(live),allocation_bytes:unit.length,scope:'bounded live receipt; no saved allocation dump'});}
function open(owner,entries){const context=counts.contexts++,profileName=owner+(entries?'-entry':'-explicit'),instance=new WebAssembly.Instance(engine.module,{}),ctx={context,owner,entries,profile:profileName,memory:instance.exports.memory,api:{},expected:Buffer.from(initial),code:Object.fromEntries(banks.map(b=>[b.name,Buffer.from(b.bytes)]))};
  const arities={open:3,close:0,arena_ptr:0,map:3,protect:3,upload:2,compile:1,compile_entries:2,compile_resident:1,compile_resident_entries:2,generation:0,module_ptr:0,module_len:0,guard:6,guard_resident:7};
  for(const[name,n]of Object.entries(arities)){ctx.api[name]=instance.exports['ring3_abi_v1_'+name];assert.equal(typeof ctx.api[name],'function',name);assert.equal(ctx.api[name].length,n,name);}
  assert.equal(ctx.api.open(4,context+1,0x574f5244),0);ctx.base=ctx.api.arena_ptr()>>>0;assert.ok(ctx.base>0&&ctx.base+SIZE<=ctx.memory.buffer.byteLength);const opened=frame(ctx,'initial full arena');lifecycle.push({context,type:'open',args:[4,context+1,0x574f5244],status:0,after:opened});
  for(const bank of banks){host(ctx,'map',[bank.pc,1,7]);upload(ctx,bank,0,bank.bytes,'literal initial '+bank.name+' bank');host(ctx,'protect',[bank.pc,1,4]);}
  const fp=record('R3FP',128,[0x81a5027f,0x05430000,0x12345678,0x90abcdef,0xbeef1357]);fp.set(Buffer.from(Array.from({length:80},(_,i)=>(i*37+context*19+11)&255)),40);input(ctx,4236,fp,'literal opaque FP128');
  contexts.push({context,owner,entries,profile:profileName,key:[context+1,0x574f5244],base:ctx.base,initial:opened});return ctx;}
for(const owner of ['replacement','resident'])for(const entries of [false,true]){const ctx=open(owner,entries);
  for(const bank of banks.slice(0,3)){compile(ctx,bank,'initial '+bank.name);for(const flags of FLAGS){seed(ctx,bank,flags);
    if(bank.name==='registers'&&flags===2){run(ctx,0,'zero budget',cpu(ctx),1,0);input(ctx,96,words([1]),'cancel on');run(ctx,1,'cancel before TEST',cpu(ctx),2,0);run(ctx,0,'cancel before zero budget',cpu(ctx),2,0);input(ctx,96,words([0]),'cancel off');run(ctx,1,'overlapping state/exit rejected',cpu(ctx),1,0,1,[ctx.base,ctx.base+28,ctx.base+96]);counts.controls+=4;}
    for(const [index,spec]of bank.instructions.entries()){assert.equal(cpu(ctx).pc,bank.pc+spec.offset);test(ctx,bank,bank.name+'/'+flags+'/'+index);
      if(bank.name==='accumulator'&&flags===0xcd7&&index===0){invalidate(ctx,bank,2,Buffer.from([1]),'changed consumed immediate behind retained EIP');run(ctx,0,'stale source guard before budget',cpu(ctx),1,0,4);counts.controls++;compile(ctx,bank,'fresh retained continuation');}}
    cold(ctx,bank);
  }}
  const bank=banks[3];compile(ctx,bank,'retained word MOV consumer chain');for(const flags of FLAGS){seed(ctx,bank,flags);const old=cpu(ctx),next={registers:[...old.registers],pc:bank.pc+4,flags:old.flags};
    assert.deepEqual([...ctx.code.chain.subarray(0,4)],[0x66,0xb8,0,0x80]);next.registers[0]=(old.registers[0]&0xffff0000|0x8000)>>>0;run(ctx,1,'retained guest WORD MOV AX',next,1,1);counts.moves++;test(ctx,bank,'retained current AX TEST',true);
    const before=cpu(ctx);assert.equal(before.flags,0x86|(flags&0x400));const after={registers:[...before.registers],pc:bank.pc+21,flags:before.flags};after.registers[2]=(before.registers[2]&0xffff0000)>>>0;after.registers[3]=(before.registers[3]&0xffffff00)>>>0;
    assert.deepEqual([...ctx.code.chain.subarray(8,19)],[0x0f,0x92,0xc2,0x0f,0x90,0xc6,0x0f,0x94,0xc3,0x75,2]);run(ctx,5,'SETB DL/SETO DH/SETZ BL/JNZ then cold',after,3,4);counts.suffix_instructions+=4;
  }
  invalidate(ctx,bank,0,Buffer.from([0x66]),'same-byte source upload after completed chain');run(ctx,0,'same-byte stale owner',cpu(ctx),1,0,4);counts.controls++;
  const before=frame(ctx,'before close');assert.equal(ctx.api.close(),0);lifecycle.push({context:ctx.context,type:'close',status:0,before,scope:'preclose arena; no artifact pointer after close'});
  assert.equal(ctx.unit.run(ctx.base,ctx.base+56,0,ctx.base+96),5);lifecycle.push({context:ctx.context,type:'closed-run',module:ctx.unit.index,budget:0,status:5,scope:'raw status only; retained arena not inspected'});counts.generated_calls++;counts.controls++;
}
assert.deepEqual(counts,{contexts:4,modules:20,tests:456,bank_tests:448,chain_tests:8,moves:8,jumps:24,suffix_instructions:32,seeds:32,generated_calls:524,retired:520,controls:28,inputs:88,hosts:92,uploads:24,upload_bytes:1108});
assert.equal(journal.length,700);assert.equal(frames.length,1408);assert.equal(lifecycle.length,12);assert.equal(sourceChanges.length,8);
assert.deepEqual(pins(),sourcePins,'source epoch unchanged');assert.deepEqual(pin(support.path),support,'immutable support unchanged');
const raw=artifact('arenas.bin',Buffer.concat(frames)),result={status:'ok',engine:{bytes:engine.bytes.length,sha256:engine.sha256},counts,source_pins:sourcePins,source_support_dependency:support,
  banks:banks.map(b=>({name:b.name,pc:b.pc,body:b.body,file:b.name+'.x86',bytes:b.bytes.length,sha256:hash(b.bytes),instructions:b.instructions})),contexts,modules,targets,journal,lifecycle,raw_records:rawRecords,raw:{...raw,frames:frames.length},source_changes:sourceChanges,
  limits:['finite16 register pairs/32 F7 endpoints/8 AX immediates, two initial FLAGS, four bound profiles; no all-values/all-FLAGS/ISA claim','TEST undefined AF deterministically clears; not hardware AF equality',
    'all full4364 arenas for declared inputs/hosts/generated calls; open after-only/close pre-only, closed raw5 unframed','no data helper/RAM effects/full-RAM dump; all eight full parents preserved by TEST; retained MOV/SETcc writes separately checked',
    'typed module section/import/type/local checks do not prove arbitrary remaining body','resident binding and initial allocator receipts are bounded physical inputs','live stale allocation equality before getter/compile/close is a producer receipt, not saved allocation proof','no performance/whole-suite/CI/graphics/Windows claim']};
writeFileSync(join(output,'result.json'),JSON.stringify(result,null,2),{flag:'wx'});assert.equal(readdirSync(output).length,28);console.log(JSON.stringify({status:'ok',...counts,raw_frames:frames.length,output}));
