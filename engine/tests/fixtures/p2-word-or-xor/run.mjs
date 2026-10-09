import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync,readdirSync,writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../support/engine.mjs';

const [enginePath,output,root]=process.argv.slice(2);assert.ok(enginePath&&output&&root);
assert.match(process.env.RING3_ENGINE_SHA256??'',/^[a-f0-9]{64}$/);
const engine=readEngine(enginePath),SIZE=4364,FLAGS=[2,0xcd7],BASES=Array.from({length:11},(_,i)=>0x1000+i*0x2000);
const hash=b=>createHash('sha256').update(b).digest('hex'),initial=readFileSync(join(output,'initial-arena.bin'));
assert.equal(initial.length,SIZE);
const parents=[0xa53c8001,0xb64dffff,0xc75e7fff,0xd86f00ff,0xe9708000,0xfa810000,0x1b925555,0x2ca3aaaa],banks=[];
for(const [kind,to_rm,to_reg,accumulator,extension,anchor]of [['or',0x09,0x0b,0x0d,1,0],['xor',0x31,0x33,0x35,6,0xffff]]){
  for(const [index,suffix]of ['registers','immediate16','immediate8','accumulator','anchors'].entries()){
    const name=kind+'-'+suffix,bytes=[],instructions=[];
    function logical(alias,source,value,opcode){let code;
      if(source!==null)code=[0x66,opcode,0xc0|(opcode===to_rm?source<<3|alias:alias<<3|source)];
      else if(alias===null)code=[0x66,accumulator,value&255,value>>>8];
      else code=opcode===0x83?[0x66,0x83,0xc0|extension<<3|alias,value]:[0x66,0x81,0xc0|extension<<3|alias,value&255,value>>>8];
      instructions.push({offset:bytes.length,length:code.length,hex:Buffer.from(code).toString('hex')});bytes.push(...code);}
    if(index===0)for(let alias=0;alias<8;alias++)for(const source of [alias,(alias+1)&7])for(const opcode of [to_rm,to_reg])logical(alias,source,0,opcode);
    if(index===1)for(let alias=0;alias<8;alias++)for(const value of [0,0x7fff,0x8000,0xffff])logical(alias,null,value,0x81);
    if(index===2)for(let alias=0;alias<8;alias++)for(const raw of [0,0x7f,0x80,0xff])logical(alias,null,raw,0x83);
    if(index===3)for(const value of [0,0x7fff,0x8000,0xffff,0x6667,0x6766,0xf0f3,0xf3f2])logical(null,null,value,accumulator);
    if(index===4)logical(null,null,anchor,accumulator);
    bytes.push(0xeb,0);const body=bytes.length;bytes.push(0x0f,0x0b);const b=Buffer.from(bytes);
    assert.deepEqual(readFileSync(join(output,name+'.x86')),b,'independent Rust/JS literal bank');banks.push({kind,name,pc:BASES[banks.length],body,bytes:b,instructions});
  }
}
const chain=Buffer.from('66b910006609c80f90c266bbffff6631db0f94c374020f0b0f0b','hex');
assert.deepEqual(readFileSync(join(output,'chain.x86')),chain);banks.push({kind:null,name:'chain',pc:BASES[10],body:22,bytes:chain,instructions:[]});
assert.deepEqual(readdirSync(output).sort(),[...banks.map(b=>b.name+'.x86'),'initial-arena.bin'].sort());
assert.deepEqual(banks.map(b=>[b.bytes.length,b.body,b.instructions.length]),[...[100,164,132,36,8].map((n,i)=>[n,n-2,[32,32,32,8,1][i]]),...[100,164,132,36,8].map((n,i)=>[n,n-2,[32,32,32,8,1][i]]),[26,22,0]]);
const sourcePaths=['engine/src/cpu/x86/ir.rs','engine/src/cpu/x86/decode/profile.rs','engine/src/cpu/x86/decode/operands.rs','engine/src/cpu/x86/decode/lower.rs','engine/src/cpu/dbt/region.rs','engine/src/cpu/dbt/wasm/integer.rs',
  'engine/src/abi/arena.rs','engine/src/abi/memory_helper.rs','engine/src/abi/x86/state.rs','engine/src/abi/x86/exit.rs','engine/src/abi/x86/x87.rs','engine/src/abi/header.rs','engine/src/cpu/dbt/wasm/abi.rs',
  'engine/src/cpu/dbt/wasm/emitter.rs','engine/src/cpu/dbt/wasm/locals.rs','engine/src/cpu/dbt/wasm/memory/narrow.rs','engine/src/cpu/dbt/wasm/memory/word_store.rs','engine/src/cpu/x86/decode/decoder.rs',
  'engine/src/cpu/x86/x87.rs','engine/src/memory/space.rs','engine/src/process/instance.rs','engine/src/process/resident.rs','engine/src/process/wasm.rs','engine/src/abi/wasm/exports.rs','engine/src/cpu/dbt/resident.rs','Cargo.toml','Cargo.lock','engine/Cargo.toml',
  'engine/tests/cpu_word_or_xor.rs','engine/tests/cpu_word_or_xor_wasm.rs','engine/tests/fixtures/p2-word-or-xor/run.mjs','engine/tests/dbt_logical.rs','engine/tests/cpu_byte_logical.rs','engine/tests/cpu_word_compare.rs','engine/tests/cpu_word_add.rs','engine/tests/cpu_word_sub.rs'];
const pin=path=>{const b=readFileSync(join(root,path));return{path,bytes:b.length,sha256:hash(b)};},pins=()=>Object.fromEntries(sourcePaths.map(p=>[p,pin(p).sha256]));
const sourcePins=pins(),support=pin('engine/tests/fixtures/support/engine.mjs'),frames=[],rawRecords=[],journal=[],modules=[],targets=[],contexts=[],lifecycle=[],sourceChanges=[],residentReleases=[];
const counts={contexts:0,modules:0,logicals:0,ors:0,xors:0,bank_logicals:0,anchor_logicals:0,chain_logicals:0,moves:0,jumps:0,suffix_instructions:0,seeds:0,generated_calls:0,retired:0,controls:0,inputs:0,hosts:0,uploads:0,upload_bytes:0,resident_discards:0,resident_retirements:0};
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
function releasePreviousResident(ctx){if(ctx.owner!=='resident'||!ctx.unit)return;
  // every generated call is synchronous and returned; cancellation is off and no engine installation was acknowledged.
  const {low,high,index,stale}=ctx.unit,name=stale?'retire_stale_resident':'discard_unacknowledged_resident';
  host(ctx,name,[ctx.context+1,0x574f5244,low,high]);ctx.unit=undefined;
  residentReleases.push({context:ctx.context,module:index,name,status:0,scope:'returned execution; exact key/id; no pointer or run use after release'});
  counts[stale?'resident_retirements':'resident_discards']++;
}
function compile(ctx,bank,label){releasePreviousResident(ctx);input(ctx,140,words(ctx.entries?[bank.pc]:[bank.pc,bank.body]),'compile request '+label);const before=frame(ctx,'before compile '+label);
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
  ctx.unit={...binding,index:row.index,run,bytes:b,stale:false};return row;}
function cpu(ctx){const b=arena(ctx);return{registers:Array.from({length:8},(_,i)=>b.readUInt32LE(16+i*4)),pc:b.readUInt32LE(48),flags:b.readUInt32LE(52)};}
function seed(ctx,bank,flags,ax=null){const c={registers:[...parents],pc:bank.pc,flags};if(ax!==null)c.registers[0]=(c.registers[0]&0xffff0000|ax)>>>0;const b=Buffer.alloc(140);b.set(state(c));b.set(exit(3,0),56);b.set(record('R3MH',40,[0,0xdecafbad,0,0,0,0]),100);input(ctx,0,b,'one retained bank CPU seed');counts.seeds++;}
function run(ctx,budget,label,next,reason,n,status=0,pointers=[ctx.base,ctx.base+56,ctx.base+96]){const before=frame(ctx,'before generated '+label);if(status===0){ctx.expected.set(state(next));ctx.expected.set(exit(reason,n),56);}
  assert.equal(ctx.unit.run(pointers[0],pointers[1],budget,pointers[2]),status,label);const row={context:ctx.context,type:'run',module:ctx.unit.index,label,budget,pointers,status,reason:status?null:reason,retired:status?0:n,before,after:frame(ctx,'after generated '+label)};
  journal.push(row);counts.generated_calls++;counts.retired+=status?0:n;return row;}
function logicalResult(kind,a,b,old){a&=0xffff;b&=0xffff;const value=kind==='or'?a|b:a^b;let parity=0;for(let i=0;i<8;i++)parity+=(value>>>i)&1;
  return{value,flags:(old&0x400)|2|(parity%2===0?4:0)|(value===0?0x40:0)|(value&0x8000?0x80:0)};}
const anchors={or:[[0,0,0x46],[1,1,2],[0x7fff,0x7fff,6],[0x8000,0x8000,0x86],[0x8001,0x8001,0x82],[0xffff,0xffff,0x86]],
  xor:[[0,0xffff,0x86],[1,0xfffe,0x82],[0x7fff,0x8000,0x86],[0x8000,0x7fff,6],[0x8001,0x7ffe,2],[0xffff,0,0x46]]};
for(const kind of ['or','xor'])for(const[a,value,flags]of anchors[kind])assert.deepEqual(logicalResult(kind,a,kind==='or'?0:0xffff,2),{value,flags});
function logical(ctx,bank,label,group='bank'){const old=cpu(ctx),at=old.pc-bank.pc,code=ctx.code[bank.name];assert.equal(code[at],0x66);let alias,right,length,kind;
  if([0x09,0x0b,0x31,0x33].includes(code[at+1])){const opcode=code[at+1],rm=code[at+2]&7,reg=code[at+2]>>>3&7;assert.equal(code[at+2]&0xc0,0xc0);
    const to_rm=[0x09,0x31].includes(opcode);kind=opcode<0x30?'or':'xor';alias=to_rm?rm:reg;right=old.registers[to_rm?reg:rm];length=3;}
  else if([0x0d,0x35].includes(code[at+1])){kind=code[at+1]===0x0d?'or':'xor';alias=0;right=code.readUInt16LE(at+2);length=4;}
  else{assert.ok([0x81,0x83].includes(code[at+1]));const ext=code[at+2]>>>3&7;assert.equal(code[at+2]&0xc0,0xc0);assert.ok([1,6].includes(ext));kind=ext===1?'or':'xor';alias=code[at+2]&7;
    right=code[at+1]===0x81?code.readUInt16LE(at+3):code.readInt8(at+3)&0xffff;length=code[at+1]===0x81?5:4;}
  const value=logicalResult(kind,old.registers[alias],right,old.flags),next={registers:[...old.registers],pc:(old.pc+length)>>>0,flags:value.flags};next.registers[alias]=(old.registers[alias]&0xffff0000|value.value)>>>0;
  const row=run(ctx,1,label,next,1,1);targets.push({context:ctx.context,module:ctx.unit.index,kind,bank:bank.name,encoded_hex:code.subarray(at,at+length).toString('hex'),before:row.before,after:row.after});counts.logicals++;counts[kind+'s']++;counts[group+'_logicals']++;}
function cold(ctx,bank){const old=cpu(ctx);assert.equal(old.pc,bank.pc+bank.body-2);assert.deepEqual([...ctx.code[bank.name].subarray(bank.body-2,bank.body)],[0xeb,0]);
  run(ctx,2,'JMP then cold with spare budget',{...old,registers:[...old.registers],pc:bank.pc+bank.body},3,1);counts.jumps++;}
function invalidate(ctx,bank,offset,b,label){const unit=ctx.unit;host(ctx,'protect',[bank.pc,1,7]);const receipt=upload(ctx,bank,offset,b,label);host(ctx,'protect',[bank.pc,1,4]);
  const live=Buffer.from(new Uint8Array(ctx.memory.buffer,unit.pointer,unit.length));assert.deepEqual(live,unit.bytes,'immediate live allocation receipt before getter/compile/close');
  sourceChanges.push({context:ctx.context,label,bank:bank.name,offset,hex:b.toString('hex'),module:unit.index,upload_after:receipt.after,allocation_before_sha256:hash(unit.bytes),allocation_after_sha256:hash(live),allocation_bytes:unit.length,scope:'bounded live receipt; no saved allocation dump'});unit.stale=true;}
function open(owner,entries){const context=counts.contexts++,profileName=owner+(entries?'-entry':'-explicit'),instance=new WebAssembly.Instance(engine.module,{}),ctx={context,owner,entries,profile:profileName,memory:instance.exports.memory,api:{},expected:Buffer.from(initial),code:Object.fromEntries(banks.map(b=>[b.name,Buffer.from(b.bytes)]))};
  const arities={open:3,close:0,arena_ptr:0,map:3,protect:3,upload:2,compile:1,compile_entries:2,compile_resident:1,compile_resident_entries:2,generation:0,module_ptr:0,module_len:0,guard:6,guard_resident:7,discard_unacknowledged_resident:4,retire_stale_resident:4};
  for(const[name,n]of Object.entries(arities)){ctx.api[name]=instance.exports['ring3_abi_v1_'+name];assert.equal(typeof ctx.api[name],'function',name);assert.equal(ctx.api[name].length,n,name);}
  assert.equal(ctx.api.open(12,context+1,0x574f5244),0);ctx.base=ctx.api.arena_ptr()>>>0;assert.ok(ctx.base>0&&ctx.base+SIZE<=ctx.memory.buffer.byteLength);const opened=frame(ctx,'initial full arena');lifecycle.push({context,type:'open',args:[12,context+1,0x574f5244],status:0,after:opened});
  for(const bank of banks){host(ctx,'map',[bank.pc,1,7]);upload(ctx,bank,0,bank.bytes,'literal initial '+bank.name+' bank');host(ctx,'protect',[bank.pc,1,4]);}
  const fp=record('R3FP',128,[0x81a5027f,0x05430000,0x12345678,0x90abcdef,0xbeef1357]);fp.set(Buffer.from(Array.from({length:80},(_,i)=>(i*37+context*19+11)&255)),40);input(ctx,4236,fp,'literal opaque FP128');
  contexts.push({context,owner,entries,profile:profileName,key:[context+1,0x574f5244],base:ctx.base,initial:opened});return ctx;}
for(const owner of ['replacement','resident'])for(const entries of [false,true]){const ctx=open(owner,entries);
  for(const kind of ['or','xor']){
    const selected=banks.filter(b=>b.kind===kind);
    for(const[bankIndex,bank]of selected.slice(0,4).entries()){compile(ctx,bank,'initial '+bank.name);seed(ctx,bank,FLAGS[bankIndex%2]);
      if(bankIndex===0){run(ctx,0,'zero budget '+kind,cpu(ctx),1,0);input(ctx,96,words([1]),'cancel on '+kind);run(ctx,1,'cancel before '+kind,cpu(ctx),2,0);run(ctx,0,'cancel before zero '+kind,cpu(ctx),2,0);input(ctx,96,words([0]),'cancel off '+kind);
        run(ctx,1,'overlapping state/exit rejected '+kind,cpu(ctx),1,0,1,[ctx.base,ctx.base+28,ctx.base+96]);counts.controls+=4;}
      for(const[index,spec]of bank.instructions.entries()){assert.equal(cpu(ctx).pc,bank.pc+spec.offset);logical(ctx,bank,bank.name+'/'+index);
        if(bank.name===kind+'-accumulator'&&index===0){invalidate(ctx,bank,2,Buffer.from([1]),'changed consumed '+kind+' immediate behind EIP');run(ctx,0,'stale source guard '+kind,cpu(ctx),1,0,4);counts.controls++;compile(ctx,bank,'fresh retained '+kind+' continuation');}}
      cold(ctx,bank);
    }
    const anchor=selected[4];compile(ctx,anchor,'literal '+kind+' flag anchors');for(const flags of FLAGS)for(const[a,value,wanted]of anchors[kind]){seed(ctx,anchor,flags,a);logical(ctx,anchor,'literal '+kind+' anchor '+a.toString(16),'anchor');
      assert.equal(cpu(ctx).registers[0],(parents[0]&0xffff0000|value)>>>0);assert.equal(cpu(ctx).flags,wanted|(flags&0x400));cold(ctx,anchor);}
  }
  const bank=banks[10];compile(ctx,bank,'retained OR/XOR current-parent consumer chain');for(const flags of FLAGS){seed(ctx,bank,flags);let old=cpu(ctx),next={registers:[...old.registers],pc:bank.pc+4,flags:old.flags};
    assert.deepEqual([...ctx.code.chain.subarray(0,4)],[0x66,0xb9,0x10,0]);next.registers[1]=(old.registers[1]&0xffff0000|0x10)>>>0;run(ctx,1,'retained MOV CX0010',next,1,1);counts.moves++;
    logical(ctx,bank,'OR current AX with guest-updated CX','chain');assert.equal(cpu(ctx).registers[0],0xa53c8011);assert.equal(cpu(ctx).flags,0x86|(flags&0x400));old=cpu(ctx);next={registers:[...old.registers],pc:bank.pc+10,flags:old.flags};next.registers[2]=(old.registers[2]&0xffffff00)>>>0;
    run(ctx,1,'SETO DL sees cleared overflow',next,1,1);counts.suffix_instructions++;
    old=cpu(ctx);next={registers:[...old.registers],pc:bank.pc+14,flags:old.flags};next.registers[3]=(old.registers[3]&0xffff0000|0xffff)>>>0;run(ctx,1,'retained MOV BXFFFF',next,1,1);counts.moves++;
    logical(ctx,bank,'self XOR current BX BX','chain');assert.equal(cpu(ctx).registers[3],0xd86f0000);assert.equal(cpu(ctx).flags,0x46|(flags&0x400));old=cpu(ctx);next={registers:[...old.registers],pc:bank.pc+20,flags:old.flags};next.registers[3]=(old.registers[3]&0xffffff00|1)>>>0;
    run(ctx,1,'SETZ BL consumes XOR zero',next,1,1);counts.suffix_instructions++;
    run(ctx,2,'JZ then cold with spare budget',{...next,registers:[...next.registers],pc:bank.pc+24},3,1);counts.jumps++;
  }
  invalidate(ctx,bank,0,Buffer.from([0x66]),'same-byte source upload after completed chain');run(ctx,0,'same-byte stale owner',cpu(ctx),1,0,4);counts.controls++;
  const before=frame(ctx,'before close');assert.equal(ctx.api.close(),0);lifecycle.push({context:ctx.context,type:'close',status:0,before,scope:'preclose arena; no artifact pointer after close'});
  assert.equal(ctx.unit.run(ctx.base,ctx.base+56,0,ctx.base+96),5);lifecycle.push({context:ctx.context,type:'closed-run',module:ctx.unit.index,budget:0,status:5,scope:'raw status only; retained arena not inspected'});counts.generated_calls++;counts.controls++;
}
assert.deepEqual(counts,{contexts:4,modules:52,logicals:944,ors:472,xors:472,bank_logicals:832,anchor_logicals:96,chain_logicals:16,moves:16,jumps:136,suffix_instructions:16,seeds:136,generated_calls:1160,retired:1112,controls:48,inputs:264,hosts:244,uploads:56,upload_bytes:3636,resident_discards:20,resident_retirements:4});
assert.equal(journal.length,1664);assert.equal(frames.length,3336);assert.equal(lifecycle.length,12);assert.equal(sourceChanges.length,12);assert.equal(residentReleases.length,24);
assert.deepEqual(pins(),sourcePins,'source epoch unchanged');assert.deepEqual(pin(support.path),support,'immutable support unchanged');
const raw=artifact('arenas.bin',Buffer.concat(frames)),result={status:'ok',engine:{bytes:engine.bytes.length,sha256:engine.sha256},counts,source_pins:sourcePins,source_support_dependency:support,
  banks:banks.map(b=>({name:b.name,pc:b.pc,body:b.body,file:b.name+'.x86',bytes:b.bytes.length,sha256:hash(b.bytes),instructions:b.instructions})),contexts,modules,targets,journal,lifecycle,raw_records:rawRecords,raw:{...raw,frames:frames.length},source_changes:sourceChanges,resident_releases:residentReleases,
  limits:['finite perkind32 register forms/32 immediate16/32 signed8/8 AX payload forms, six literal flag anchors/two admitted FLAGS and four bound profiles; no all-values/all-FLAGS/ISA claim','OR/XOR define width16 SF/ZF/PF and clear CF/OF; undefined AF deterministically clears by engine policy, admitted DF and required2 preserved',
    'all full4364 arenas for declared inputs/hosts/generated calls; open after-only/close pre-only, closed raw5 unframed','no data helper/RAM effects/full-RAM dump; logic changes one low16 destination and logical FLAGS; every high16/other parent/opaque FP preserved; retained MOV/SETcc writes separately checked',
    'typed module section/import/type/local checks do not prove arbitrary remaining body','resident binding and initial allocator receipts are bounded physical inputs; each next admission discards completed current or retires stale previous uninstalled unit after returned execution, live unit bound one; no released pointer/run use','live stale allocation equality before getter/compile/close/release is a producer receipt, not saved allocation proof','no performance/whole-suite/CI/graphics/Windows claim']};
writeFileSync(join(output,'result.json'),JSON.stringify(result,null,2),{flag:'wx'});assert.equal(readdirSync(output).length,67);console.log(JSON.stringify({status:'ok',...counts,raw_frames:frames.length,output}));
