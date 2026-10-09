import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync,readdirSync,writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../support/engine.mjs';

const [enginePath,output,root]=process.argv.slice(2); assert.ok(enginePath&&output&&root);
assert.match(process.env.RING3_ENGINE_SHA256??'',/^[a-f0-9]{64}$/);
const engine=readEngine(enginePath),SIZE=4364,BASES=[0x1000,0x3000,0x5000],FLAGS=[2,0xcd7];
const hash=b=>createHash('sha256').update(b).digest('hex'),initial=readFileSync(join(output,'initial-arena.bin'));
assert.equal(initial.length,SIZE); assert.deepEqual(readdirSync(output).sort(),['initial-arena.bin','modrm.x86','opcode.x86','payload.x86']);
const parents=[0xa53c0001,0xb64d8001,0xc75e7fff,0xd86f00ff,0xe970f001,0xfa81abcd,0x1b926667,0x2ca3f0f3];
const bankList=[];
for(const [index,name]of ['opcode','modrm','payload'].entries()){
  const bytes=[],instructions=[]; const append=(alias,value,modrm)=>{const code=modrm?[0x66,0xc7,0xc0|alias,value&255,value>>>8]:[0x66,0xb8|alias,value&255,value>>>8];
    instructions.push({offset:bytes.length,alias,value,modrm,length:code.length,hex:Buffer.from(code).toString('hex')});bytes.push(...code);};
  if(index<2)for(let alias=0;alias<8;alias++)for(const value of [0,0x7fff,0x8000,0xffff])append(alias,value,index===1);
  else for(const [alias,value,modrm]of [[0,0x6667,false],[0,0xf0f3,true],[4,0xf0f3,false],[4,0x6667,true]])append(alias,value,modrm);
  const body=bytes.length+2;bytes.push(0xeb,0,0x0f,0x0b);const b=Buffer.from(bytes);assert.deepEqual(readFileSync(join(output,name+'.x86')),b,'independent Rust/JS bank');
  bankList.push({name,pc:BASES[index],body,bytes:b,instructions});
}
assert.deepEqual(bankList.map(b=>[b.body,b.instructions.length]),[[130,32],[162,32],[20,4]]);
const sourcePaths=['engine/src/cpu/x86/ir.rs','engine/src/cpu/x86/decode/profile.rs','engine/src/cpu/x86/decode/operands.rs','engine/src/cpu/x86/decode/lower.rs','engine/src/cpu/dbt/region.rs','engine/src/cpu/dbt/wasm/integer.rs',
  'engine/src/abi/arena.rs','engine/src/abi/memory_helper.rs','engine/src/abi/x86/state.rs','engine/src/abi/x86/exit.rs','engine/src/abi/x86/x87.rs','engine/src/abi/header.rs','engine/src/cpu/dbt/wasm/abi.rs',
  'engine/src/cpu/dbt/wasm/emitter.rs','engine/src/cpu/dbt/wasm/locals.rs','engine/src/cpu/dbt/wasm/memory/narrow.rs','engine/src/cpu/dbt/wasm/memory/word_store.rs','engine/src/cpu/x86/decode/decoder.rs',
  'engine/src/cpu/x86/x87.rs','engine/src/memory/space.rs','engine/src/process/instance.rs','engine/src/process/resident.rs','engine/src/process/wasm.rs','Cargo.toml','Cargo.lock','engine/Cargo.toml',
  'engine/tests/cpu_word_move_immediate.rs','engine/tests/cpu_word_move_immediate_wasm.rs','engine/tests/fixtures/p2-word-move-immediate/run.mjs','engine/tests/decode_data.rs','engine/tests/cpu_byte_move.rs','engine/tests/cpu_byte_move_immediate_modrm.rs',
  'engine/tests/process_resident_memory.rs','engine/tests/process_resident_store.rs'];
const pin=path=>{const b=readFileSync(join(root,path));return{path,bytes:b.length,sha256:hash(b)};},pins=()=>Object.fromEntries(sourcePaths.map(p=>[p,pin(p).sha256]));
const sourcePins=pins(),support=pin('engine/tests/fixtures/support/engine.mjs');
const frames=[],rawRecords=[],journal=[],modules=[],targets=[],contexts=[],lifecycle=[],sourceChanges=[];
const counts={contexts:0,modules:0,targets:0,core_targets:0,payload_targets:0,seeds:0,generated_calls:0,retired:0,jumps:0,controls:0,inputs:0,hosts:0,uploads:0,upload_bytes:0};
function artifact(name,b){writeFileSync(join(output,name),b,{flag:'wx'});return{file:name,bytes:b.length,sha256:hash(b)};}
artifact('engine.wasm',engine.bytes);
function record(magic,size,fields){const b=Buffer.alloc(size);b.write(magic);b.writeUInt16LE(1,4);b.writeUInt16LE(1,6);b.writeUInt32LE(size,8);fields.forEach((v,i)=>b.writeUInt32LE(v>>>0,16+i*4));return b;}
const state=c=>record('R3ST',56,[...c.registers,c.pc,c.flags]),exit=(reason,n)=>record('R3EX',40,[reason,n,0,0,0,0]);
const words=values=>{const b=Buffer.alloc(values.length*4);values.forEach((v,i)=>b.writeUInt32LE(v>>>0,i*4));return b;};
const arena=ctx=>Buffer.from(new Uint8Array(ctx.memory.buffer,ctx.base,SIZE));
function check(ctx,label){assert.deepEqual(arena(ctx),ctx.expected,ctx.profile+'/'+label);}
function frame(ctx,label){check(ctx,label);const b=arena(ctx),r={index:frames.length,context:ctx.context,label,file:'arenas.bin',offset:frames.length*SIZE,bytes:SIZE,sha256:hash(b)};frames.push(b);rawRecords.push(r);return r;}
function input(ctx,offset,b,label){const before=frame(ctx,'before input '+label);assert.ok(offset>=0&&offset+b.length<=SIZE);new Uint8Array(ctx.memory.buffer).set(b,ctx.base+offset);ctx.expected.set(b,offset);
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
function compile(ctx,bank,label){input(ctx,140,words(ctx.entries?[bank.pc]:[bank.pc,bank.body]),'compiler request '+label);const before=frame(ctx,'before compile '+label);
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
function move(ctx,bank,spec,label){const old=cpu(ctx),offset=old.pc-bank.pc,code=ctx.code[bank.name],modrm=code[offset+1]===0xc7;
  assert.equal(code[offset],0x66);assert.equal(modrm,!!spec.modrm);const alias=modrm?code[offset+2]&7:code[offset+1]-0xb8,length=modrm?5:4;assert.ok(alias>=0&&alias<8);if(modrm)assert.equal(code[offset+2]&0xf8,0xc0);
  const value=code.readUInt16LE(offset+length-2);assert.deepEqual([offset,alias,value,length],[spec.offset,spec.alias,spec.value,spec.length]);
  const next={registers:[...old.registers],pc:(old.pc+length)>>>0,flags:old.flags};next.registers[alias]=(old.registers[alias]&0xffff0000|value)>>>0;
  const row=run(ctx,1,label,next,1,1);targets.push({context:ctx.context,module:ctx.unit.index,bank:bank.name,alias,encoded_hex:code.subarray(offset,offset+length).toString('hex'),before:row.before,after:row.after});counts.targets++;counts[bank.name==='payload'?'payload_targets':'core_targets']++;}
function cold(ctx,bank){const old=cpu(ctx);assert.equal(old.pc,bank.pc+bank.body-2);const next={...old,registers:[...old.registers],pc:bank.pc+bank.body};run(ctx,2,'JMP then cold NeedCode with spare budget',next,3,1);counts.jumps++;}
function invalidate(ctx,bank,offset,b,label){const unit=ctx.unit;host(ctx,'protect',[bank.pc,1,7]);const uploadRow=upload(ctx,bank,offset,b,label);host(ctx,'protect',[bank.pc,1,4]);
  const live=Buffer.from(new Uint8Array(ctx.memory.buffer,unit.pointer,unit.length));assert.deepEqual(live,unit.bytes,'stale live allocation receipt before getter/compile/close');
  sourceChanges.push({context:ctx.context,label,bank:bank.name,offset,hex:b.toString('hex'),module:unit.index,upload_after:uploadRow.after,allocation_before_sha256:hash(unit.bytes),allocation_after_sha256:hash(live),allocation_bytes:unit.length,scope:'immediate live receipt; no saved allocation dump'});}
function open(owner,entries){const context=counts.contexts++,profileName=owner+(entries?'-entry':'-explicit'),instance=new WebAssembly.Instance(engine.module,{}),ctx={context,owner,entries,profile:profileName,memory:instance.exports.memory,api:{},expected:Buffer.from(initial),code:Object.fromEntries(bankList.map(b=>[b.name,Buffer.from(b.bytes)]))};
  const arities={open:3,close:0,arena_ptr:0,map:3,protect:3,upload:2,compile:1,compile_entries:2,compile_resident:1,compile_resident_entries:2,generation:0,module_ptr:0,module_len:0,guard:6,guard_resident:7};
  for(const[name,n]of Object.entries(arities)){ctx.api[name]=instance.exports['ring3_abi_v1_'+name];assert.equal(typeof ctx.api[name],'function',name);assert.equal(ctx.api[name].length,n,name);}
  assert.equal(ctx.api.open(3,context+1,0x57494d4d),0);ctx.base=ctx.api.arena_ptr()>>>0;assert.ok(ctx.base>0&&ctx.base+SIZE<=ctx.memory.buffer.byteLength);const opened=frame(ctx,'initial full arena after open');
  lifecycle.push({context,type:'open',args:[3,context+1,0x57494d4d],status:0,after:opened});
  for(const bank of bankList){host(ctx,'map',[bank.pc,1,7]);upload(ctx,bank,0,bank.bytes,'literal initial '+bank.name+' bank');host(ctx,'protect',[bank.pc,1,4]);}
  const fp=record('R3FP',128,[0x81a5027f,0x05430000,0x12345678,0x90abcdef,0xbeef1357]);fp.set(Buffer.from(Array.from({length:80},(_,i)=>(i*37+context*19+11)&255)),40);input(ctx,4236,fp,'literal opaque FP128');
  contexts.push({context,owner,entries,profile:profileName,key:[context+1,0x57494d4d],base:ctx.base,initial:opened});return ctx;}

for(const owner of ['replacement','resident'])for(const entries of [false,true]){const ctx=open(owner,entries);
  for(const bank of bankList){compile(ctx,bank,'initial '+bank.name);
    for(const flags of FLAGS){seed(ctx,bank,flags);
      if(bank.name==='opcode'&&flags===2){run(ctx,0,'zero budget before partial write',cpu(ctx),1,0);input(ctx,96,words([1]),'cancel on');run(ctx,1,'cancel before partial write',cpu(ctx),2,0);run(ctx,0,'cancel wins over zero budget',cpu(ctx),2,0);
        input(ctx,96,words([0]),'cancel off');run(ctx,1,'overlapping exit/state rejected without publication',cpu(ctx),1,0,1,[ctx.base,ctx.base+28,ctx.base+96]);counts.controls+=4;}
      for(const [index,spec]of bank.instructions.entries()){move(ctx,bank,spec,'retained immediate '+bank.name+'/'+flags+'/'+index);
        if(bank.name==='payload'&&flags===0xcd7&&index===0){invalidate(ctx,bank,2,Buffer.from([0x68]),'changed consumed first immediate behind current EIP');input(ctx,96,words([1]),'cancel before stale guard');
          run(ctx,0,'stale owner before cancel/budget/pointer checks',cpu(ctx),1,0,4,[ctx.base,ctx.base+28,ctx.base+96]);counts.controls++;input(ctx,96,words([0]),'restore cancel before fresh owner');compile(ctx,bank,'fresh retained continuation');}}
      cold(ctx,bank);
    }
  }
  const bank=bankList[2];invalidate(ctx,bank,0,Buffer.from([0x66]),'same-byte source upload after completed bank');run(ctx,0,'same-byte stale guard before budget',cpu(ctx),1,0,4);counts.controls++;
  const before=frame(ctx,'before close');assert.equal(ctx.api.close(),0);lifecycle.push({context:ctx.context,type:'close',status:0,before,scope:'preclose arena only; no artifact pointer after close'});
  assert.equal(ctx.unit.run(ctx.base,ctx.base+56,0,ctx.base+96),5);lifecycle.push({context:ctx.context,type:'closed-run',module:ctx.unit.index,budget:0,status:5,scope:'raw status only; retained arena is not inspected'});counts.generated_calls++;counts.controls++;
}
assert.deepEqual(counts,{contexts:4,modules:16,targets:544,core_targets:512,payload_targets:32,seeds:24,generated_calls:596,retired:568,jumps:24,controls:28,inputs:80,hosts:76,uploads:20,upload_bytes:1280});
assert.equal(journal.length,748);assert.equal(frames.length,1504);assert.equal(lifecycle.length,12);assert.equal(sourceChanges.length,8);
assert.deepEqual(pins(),sourcePins,'source epoch unchanged');assert.deepEqual(pin(support.path),support,'immutable readEngine support unchanged');
const raw=artifact('arenas.bin',Buffer.concat(frames));const result={status:'ok',engine:{bytes:engine.bytes.length,sha256:engine.sha256},counts,source_pins:sourcePins,source_support_dependency:support,
  banks:bankList.map(b=>({name:b.name,pc:b.pc,body:b.body,file:b.name+'.x86',bytes:b.bytes.length,sha256:hash(b.bytes),instructions:b.instructions})),contexts,modules,targets,journal,lifecycle,raw_records:rawRecords,raw:{...raw,frames:frames.length},source_changes:sourceChanges,
  limits:['512 finite core and32 selected prefix-payload targets; no all-values/all-FLAGS/full-ISA claim','all full4364 arenas for declared inputs/known host calls/generated calls; open after-only and close pre-only, closed raw5 unframed',
    'no guest RAM helper/data-page effect or full-RAM dump; all parents includingSP are partial register writes','pure module section/import/type/local checks do not certify arbitrary remaining Wasm body','resident pointer/id receipt and initial allocator correspondence are bounded physical inputs',
    'source allocation equality is a live producer receipt before getter/compile/close, not a raw allocation certificate','no hardware/performance/whole-suite/CI/graphics/Windows claim']};
writeFileSync(join(output,'result.json'),JSON.stringify(result,null,2),{flag:'wx'});assert.equal(readdirSync(output).length,23);console.log(JSON.stringify({status:'ok',...counts,raw_frames:frames.length,output}));
