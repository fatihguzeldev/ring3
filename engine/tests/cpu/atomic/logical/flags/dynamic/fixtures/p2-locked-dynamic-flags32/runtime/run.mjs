import {pageFaultExit, pageFaultPacket} from '../../../../../../../../support/wasm/page-fault.mjs';
import {generatedImports, attachDetectionApi} from '../../../../../../../../support/wasm/binding.mjs';
import {ARENA_SIZE} from '../../../../../../../../support/wasm/arena.mjs';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync,writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../../../../../../../../support/wasm/engine.mjs';

const [enginePath,output,root]=process.argv.slice(2),SIZE=ARENA_SIZE,DATA=0x5010;
assert.ok(enginePath&&output&&root);
const engine=readEngine(enginePath),initial=readFileSync(join(output,'initial-arena.bin'));
const hash=b=>createHash('sha256').update(b).digest('hex');
assert.equal(initial.length,SIZE);assert.equal(engine.sha256,hash(engine.bytes));
const sourcePaths=['engine/src/cpu/x86/decode/profile.rs', 'engine/src/cpu/x86/decode/families.rs','engine/src/cpu/x86/decode/integer.rs','engine/src/cpu/x86/ir.rs',
  'engine/src/cpu/dbt/region.rs','engine/src/cpu/dbt/wasm/emitter/mod.rs', 'engine/src/cpu/dbt/wasm/emitter/module.rs', 'engine/src/cpu/dbt/wasm/emitter/body.rs', 'engine/src/cpu/dbt/wasm/requirements.rs', 'engine/src/cpu/dbt/wasm/imports/wiring.rs','engine/src/cpu/dbt/wasm/instructions/integer/arithmetic.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/bits.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/carry_rotates.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/decimal.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/flags.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/mod.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/multiply_divide.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/operands.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/rotates.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/shifts.rs',
  'engine/src/cpu/dbt/wasm/imports/mod.rs', 'engine/src/cpu/dbt/wasm/imports/binding.rs', 'engine/src/cpu/dbt/wasm/imports/context.rs', 'engine/src/cpu/dbt/wasm/imports/needs.rs', 'engine/src/cpu/dbt/wasm/helpers/protocol.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/mod.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/address.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/scalar.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/stack.rs','engine/src/cpu/dbt/wasm/instructions/memory/exchange.rs','engine/src/cpu/dbt/wasm/locals.rs',
  'engine/src/memory/space.rs', 'engine/src/memory/space/atomic.rs','engine/src/process/instance.rs', 'engine/src/process/execution/compiled.rs', 'engine/src/process/execution/authority.rs', 'engine/src/process/memory/address_space.rs', 'engine/src/process/memory/scalar.rs', 'engine/src/process/memory/scalar/tests.rs', 'engine/src/process/execution/mod.rs', 'engine/src/process/memory/mod.rs', 'engine/src/process/mod.rs','engine/src/process/wasm/mod.rs', 'engine/src/process/wasm/loading.rs', 'engine/src/abi/image.rs', 'engine/src/process/wasm/execution.rs', 'engine/src/abi/execution.rs', 'engine/src/process/wasm/calls.rs', 'engine/src/process/wasm/memory.rs', 'engine/src/process/wasm/cpu.rs', 'engine/src/process/execution/resident.rs', 'engine/src/abi/memory_helper.rs','engine/src/abi/arena.rs','engine/src/abi/wasm/exports/calls.rs', 'engine/src/abi/wasm/exports/cpu.rs', 'engine/src/abi/wasm/exports/execution.rs', 'engine/src/abi/wasm/exports/image.rs', 'engine/src/abi/wasm/exports/instance.rs', 'engine/src/abi/wasm/exports/memory.rs', 'engine/src/abi/wasm/exports/mod.rs',
  'engine/tests/cpu/atomic/logical/locked/native/register32.rs', 'engine/tests/cpu/atomic/native.rs', 'engine/tests/support/cpu/prefix/admission.rs', 'engine/tests/cpu/atomic/logical/flags/dynamic/native.rs', 'engine/tests/cpu/atomic/logical/flags/dynamic/wasm.rs', 'engine/tests/cpu/atomic/wasm.rs', 'engine/tests/support/wasm.rs', ...['kernel.c','build.py','PROVENANCE.md','kernel.obj','kernel.text.bin','kernel.disasm.txt','receipt.json','runtime/run.mjs'].map(p=>'engine/tests/cpu/atomic/logical/flags/dynamic/fixtures/p2-locked-dynamic-flags32/'+p),
  'engine/tests/programs/records/fixtures/p2-compiled-records/build.py','engine/tests/support/wasm/engine.mjs', 'engine/tests/support/wasm/binding.mjs', 'engine/tests/support/wasm/page-fault.mjs'];
sourcePaths.push('engine/src/abi/memory_helper/cpu.rs','engine/src/cpu/dbt/wasm/instructions/memory/cpu_scalar.rs','engine/src/cpu/dbt/wasm/emitter/mod.rs', 'engine/src/cpu/dbt/wasm/emitter/module.rs', 'engine/src/cpu/dbt/wasm/emitter/body.rs', 'engine/src/cpu/dbt/wasm/requirements.rs', 'engine/src/cpu/dbt/wasm/imports/wiring.rs','engine/src/process/memory/cpu_scalar.rs','engine/src/process/memory/scalar/cpu_write.rs','engine/src/process/wasm/memory/cpu_write.rs','engine/src/abi/wasm/exports/memory/cpu_write.rs','engine/src/memory/space/cpu/atomic.rs');
const sourceHashes=()=>Object.fromEntries(sourcePaths.map(p=>[p,hash(readFileSync(join(root,p)))]));
const sourcesBefore=sourceHashes(),contexts=[],modules=[],observations=[],helperTrace=[],diagnostics=[];
const counts={contexts:0,modules:0,runs:0,retired:0,helper_calls:0,read32:0,store32:0,atomic_ors:0,atomic_ands:0,atomic_xors:0,compiler_calls:0,probes:0,faults:0,repair_only:0,smc:0,stale_owners:0,fresh_suffixes:0};
const word=n=>{const b=Buffer.alloc(4);b.writeUInt32LE(n>>>0);return b;},words=v=>Buffer.concat(v.map(word));
function record(magic,version,size,fields){const b=Buffer.alloc(size);b.write(magic);b.writeUInt16LE(version,4);b.writeUInt16LE(1,6);b.writeUInt32LE(size,8);fields.forEach((v,i)=>b.writeUInt32LE(v>>>0,16+4*i));return b;}
const state=s=>record('R3ST',1,56,[...s.registers,s.pc,s.flags]);
const exit = (reason, retired, { detail = 0, address = 0, access = 0 } = {}, version = 2) =>
  reason === 5
    ? pageFaultExit({
        retired,
        errorCode: 6 | (detail === 2 ? 1 : 0),
        address,
        access: 2,
        length: access ? 4 : 0
      })
    : record('R3EX', version, 40, [reason, retired, detail, address, access, access ? 4 : 0]);
const initialHelper=(value,{detail=0,address=0,access=0}={})=>record('R3MH',1,40,[access?1:0,value,detail,address,access,access?4:0]);
const helper = (value, { detail = 0, address = 0, access = 0 } = {}) =>
  Boolean(access) && (detail === 1 || detail === 2)
    ? pageFaultPacket({ errorCode: 6 | (detail === 2 ? 1 : 0), address, access: 2, length: 4 })
    : record('R3MH', 10, 40, [access ? 1 : 0, value, detail, address, access, 4]);
const parents=()=>Array.from({length:8},(_,i)=>(0xa100+i)*65536+0x1234+i);
const parity=value=>{let n=value&255,ones=0;while(n){ones+=n&1;n>>>=1;}return ones%2===0?4:0;};
const logicalFlags=value=>0x402|parity(value)|(value===0?0x40:0)|(value>=0x80000000?0x80:0);
assert.equal(logicalFlags(0x80030050),0x486);assert.equal(logicalFlags(0x80010010),0x482);assert.equal(logicalFlags(0x80430850),0x486);
const kernel=readFileSync(join(root,'engine/tests/cpu/atomic/logical/flags/dynamic/fixtures/p2-locked-dynamic-flags32/kernel.text.bin'));
assert.equal(kernel.toString('hex'),'568b4424148b4c24108b54240c8b742408f009168b16f0210e8b0ef0310631d18b0631c85ec3');
assert.equal(hash(kernel),'eef4b2124b1aae80ef7aacbefb7c4b4c9ff441789d1b73a91c274c04e78b717e');
const caller=[
  {pc:0x3000,bytes:Buffer.from('680008400068bffffdff68000001806810500000e8e7efffff','hex'),instructions:5},
  {pc:0x3019,bytes:Buffer.from('83c41089c55068ffff000068000000806810500000e8cdefffff','hex'),instructions:7},
  {pc:0x3033,bytes:Buffer.from('83c41001e8a330500000ebfe','hex'),instructions:4},
];
assert.deepEqual(caller.map(b=>b.pc+b.bytes.length),[0x3019,0x3033,0x303f]);
const probes=[
  {pc:0x1000,bytes:Buffer.from('f00900','hex'),kind:'or',mask:0x5010,old:0x80000000,value:0x80005010,flags:0x482,address:DATA,set:{0:DATA}},
  {pc:0x1020,bytes:Buffer.from('f0214c8ef0','hex'),kind:'and',mask:8,old:0xffffffff,value:8,flags:0x402,address:DATA,set:{6:0x5000,1:8}},
  {pc:0x1040,bytes:Buffer.from('f03126','hex'),kind:'xor',mask:0x80000000,old:0,value:0x80000000,flags:0x486,address:DATA,set:{6:DATA,4:0x80000000}},
];
const smcBytes=Buffer.from('f03106ba5a563412ebfe','hex');
const probeBlocks=probes.map(p=>({pc:p.pc,bytes:Buffer.concat([p.bytes,Buffer.from([0xeb,0])]),instructions:2}));
const arities={open:3,close:0,arena_ptr:0,map:3,protect:3,upload:2,read32:1,store32:2,store_resident32:6,
  atomic_or_u32:2,atomic_or_resident_u32:6,atomic_and_u32:2,atomic_and_resident_u32:6,atomic_xor_u32:2,atomic_xor_resident_u32:6,
  compile:1,compile_entries:2,compile_resident:1,compile_resident_entries:2,generation:0,module_ptr:0,module_len:0,guard:6,guard_resident:7};
const arena=c=>Buffer.from(new Uint8Array(c.memory.buffer,c.base,SIZE));
const cpu=c=>{const a=arena(c);return{registers:Array.from({length:8},(_,i)=>a.readUInt32LE(16+4*i)),pc:a.readUInt32LE(48),flags:a.readUInt32LE(52)};};
function input(c,offset,bytes){const expected=arena(c);expected.set(bytes,offset);new Uint8Array(c.memory.buffer).set(bytes,c.base+offset);assert.deepEqual(arena(c),expected);}
function host(c,name,args,span){const expected=arena(c);assert.equal(c.api[name](...args),0,name);if(span)expected.set(arena(c).subarray(...span),span[0]);assert.deepEqual(arena(c),expected,'host complete arena '+name);}
function upload(c,address,bytes){input(c,140,bytes);host(c,'upload',[address,bytes.length]);}
function seed(c,pc,registers){const s={registers:[...registers],pc,flags:0xcd7};input(c,0,Buffer.concat([state(s),exit(1,0),word(0),initialHelper(0xad)]));return s;}
function name(c,kind){if(kind==='read32')return 'cpu_read32';if(kind==='store32')return c.owner==='resident'?'cpu_store_resident32':'cpu_store32';return 'cpu_atomic_'+kind+(c.owner==='resident'?'_resident_u32':'_u32');}
function open(owner,entries){
  const instance=new WebAssembly.Instance(engine.module,{}),c={id:++counts.contexts,owner,entries,memory:instance.exports.memory,api:{},high:0x44594e41,captures:0};
  for(const[n,arity]of Object.entries(arities)){c.api[n]=instance.exports['ring3_abi_v1_'+n];assert.equal(c.api[n]?.length,arity,n);}
 attachDetectionApi(c.api, instance.exports);
  for (const name of ['read32', 'store32', 'store_resident32', 'atomic_or_u32', 'atomic_or_resident_u32', 'atomic_and_u32', 'atomic_and_resident_u32', 'atomic_xor_u32', 'atomic_xor_resident_u32']) {c.api['cpu_'+name] = instance.exports['ring3_abi_v1_cpu_'+name]; assert.equal(c.api['cpu_'+name]?.length, arities[name], 'cpu_'+name);}
  assert.equal(c.api.open(12,c.id,c.high),0);c.base=c.api.arena_ptr()>>>0;input(c,0,initial);
  for(const[page,permission]of[[0x1000,7],[0x2000,7],[0x3000,7],[0x5000,3],[0x7000,3],[0xc000,7]])host(c,'map',[page,1,permission]);
  for(const b of[...probeBlocks,...caller,{pc:0x2000,bytes:kernel}])upload(c,b.pc,b.bytes);
  upload(c,0xc000,Buffer.concat([smcBytes,Buffer.from([0xa5,0x5a])]));
  for(const page of[0x1000,0x2000,0x3000])host(c,'protect',[page,1,5]);return c;
}
function compile(c,title,blocks,kinds){
  assert.ok(++c.captures<=6);assert.ok(blocks.length<=8);const instructions=blocks.reduce((n,b)=>n+b.instructions,0);assert.ok(instructions<=64);
  input(c,140,words(blocks.flatMap(b=>c.entries?[b.pc]:[b.pc,b.bytes.length])));
  const method=c.owner==='resident'?(c.entries?'compile_resident_entries':'compile_resident'):(c.entries?'compile_entries':'compile');
  host(c,method,c.entries?[blocks.length,0]:[blocks.length],c.owner==='resident'?[140,164]:null);
  const a=arena(c),binding=c.owner==='resident'?{low:a.readUInt32LE(148),high:a.readUInt32LE(152),pointer:a.readUInt32LE(156),length:a.readUInt32LE(160)}:{generation:c.api.generation(),pointer:c.api.module_ptr()>>>0,length:c.api.module_len()>>>0};
  if(c.owner==='resident'){assert.equal(a.readUInt32LE(140),1);assert.equal(a.readUInt32LE(144),24);assert.ok(binding.low||binding.high);}
  const bytes=Buffer.from(new Uint8Array(c.memory.buffer,binding.pointer,binding.length)),module=new WebAssembly.Module(bytes),names=generatedImports({resident: c.owner==='resident', helpers: [...kinds.map(k=>name(c,k))]}).slice(1).map(item => item.name);
  const imports=[{module:'env',name:'memory',kind:'memory'},...names.map(n=>({module:'ring3',name:n,kind:'function'}))];
  assert.ok(bytes.length<=65536);assert.deepEqual(WebAssembly.Module.imports(module),imports);assert.deepEqual(WebAssembly.Module.exports(module),[{name:'run',kind:'function'}]);
  const ring3={...c.api};for(const kind of kinds){const n=name(c,kind);ring3[n]=(...args)=>{const before=arena(c),status=c.api[n](...args),after=arena(c);before.set(after.subarray(100,140),100);assert.deepEqual(after,before,'canonical helper40B only');
    const payload=args.slice(n.includes('resident')?4:0).map(v=>v>>>0);helperTrace.push({context:c.id,call:counts.runs+1,name:n,address:payload[0],input_value:kind==='read32'?null:payload[1],status,helper_hex:after.subarray(100,140).toString('hex')});
    counts.helper_calls++;counts[kind==='or'?'atomic_ors':kind==='and'?'atomic_ands':kind==='xor'?'atomic_xors':kind]++;return status;};}
  const run=new WebAssembly.Instance(module,{env:{memory:c.memory},ring3}).exports.run;assert.equal(run.length,4);
  const file=`module-${++counts.modules}.wasm`;writeFileSync(join(output,file),bytes,{flag:'wx'});modules.push({context:c.id,owner:c.owner,entries:c.entries,name:title,file,bytes:bytes.length,sha256:hash(bytes),blocks:blocks.length,instructions,imports,...binding});return{...binding,name:title,run,exitVersion:kinds.length?6:1};
}
function run(c,unit,budget,s,label,{status=0,reason=1,retired=budget,packet,fault={}}={}){
  const expected=arena(c),at=helperTrace.length;
  if(status===0){expected.set(state(s));expected.set(exit(reason,retired,fault,unit.exitVersion),56);if(packet)expected.set(packet,100);}
  const observed=unit.run(c.base,c.base+56,budget,c.base+96);assert.equal(observed,status,label);assert.deepEqual(arena(c),expected,'complete arena '+label);const a=arena(c);
  observations.push({context:c.id,module:unit.name,label,budget,observed_status:observed,observed_reason:status?null:a.readUInt32LE(72),observed_retired:status?null:a.readUInt32LE(76),observed_cpu:cpu(c),helper_calls:helperTrace.length-at,helper_hex:a.subarray(100,140).toString('hex')});counts.runs++;if(!status)counts.retired+=retired;
}
function checkRam(c,address,expected){for(let i=0;i<expected.length;i+=4){const a=arena(c),value=expected.readUInt32LE(i);assert.equal(c.api.read32(address+i),0);a.set(initialHelper(value),100);assert.deepEqual(arena(c),a,'diagnostic complete arena');diagnostics.push({context:c.id,address:address+i,value});}}
function next(s,pc,flags=s.flags,updates={}){const n={registers:[...s.registers],pc,flags};for(const[r,v]of Object.entries(updates))n.registers[Number(r)]=v>>>0;return n;}
function probeProof(c,unit){for(const p of probes){const registers=parents();for(const[r,v]of Object.entries(p.set))registers[Number(r)]=v;const start=p.address-12,frame=Buffer.from(Array.from({length:32},(_,i)=>(i*17+23)&255));frame.writeUInt32LE(p.old,12);upload(c,start,frame);
  const s=seed(c,p.pc,registers);run(c,unit,1,next(s,p.pc+p.bytes.length,p.flags),`register-source-${p.kind}`,{packet:helper(p.old)});counts.probes++;frame.writeUInt32LE(p.value,12);checkRam(c,start,frame);
  const e=helperTrace.at(-1);assert.equal(e.name,name(c,p.kind));assert.equal(e.address,p.address);assert.equal(e.input_value,p.mask);assert.equal(e.status,0);assert.equal(e.helper_hex,helper(p.old).toString('hex'));}}
function faultProof(c,unit){
  const p=probes[2],registers=parents();registers[6]=DATA;registers[4]=0x80000000;
  const start=DATA-12,frame=Buffer.from(Array.from({length:32},(_,i)=>(i*17+23)&255));frame.writeUInt32LE(0,12);upload(c,start,frame);
  const s=seed(c,p.pc,registers),fault={detail:2,address:DATA,access:2};host(c,'protect',[0x5000,1,1]);
  const at=helperTrace.length;run(c,unit,1,s,'register-ESP-write-permission-no-publication',{reason:5,retired:0,fault,packet:helper(0,fault)});counts.faults++;
  assert.equal(helperTrace.length-at,1);assert.equal(helperTrace.at(-1).input_value,0x80000000);assert.equal(helperTrace.at(-1).status,0);checkRam(c,start,frame);
  host(c,'protect',[0x5000,1,3]);
  if(c.owner==='replacement')assert.equal(c.api.generation(),unit.generation);
  run(c,unit,1,next(s,p.pc+p.bytes.length,0x486),'same-stopped-ESP-source-protect-only-retry',{packet:helper(0)});counts.repair_only++;
  frame.writeUInt32LE(0x80000000,12);checkRam(c,start,frame);
  const e=helperTrace.at(-1);assert.equal(e.name,name(c,'xor'));assert.equal(e.address,DATA);assert.equal(e.input_value,0x80000000);assert.equal(e.status,0);
}
function originalProof(c,unit){
  const data=Buffer.from(Array.from({length:64},(_,i)=>(i*13+7)&255));data.writeUInt32LE(0x00020050,16);data.writeUInt32LE(0,48);upload(c,0x5000,data);
  const stack=Buffer.from(Array.from({length:32},(_,i)=>(i*17+23)&255));upload(c,0x7064,stack);
  const r=parents();r[4]=0x7080;let s=seed(c,0x3000,r);const snapshots=[],at=helperTrace.length;
  const snap=(label,extra={})=>snapshots.push({label,cpu:cpu(c),arena_hex:arena(c).toString('hex'),...extra});snap('entry');
  s=next(s,0x2000,0xcd7,{4:0x706c});run(c,unit,5,s,'four-arguments-and-first-natural-cdecl-call',{packet:helper(0)});
  stack.set(words([0x3019,DATA,0x80010000,0xfffdffbf,0x00400800]),8);checkRam(c,0x7064,stack);
  s=next(s,0x2011,0xcd7,{0:0x00400800,1:0xfffdffbf,2:0x80010000,4:0x7068,6:DATA});
  run(c,unit,5,s,'original-saves-ESI-and-pauses-before-register-LOCK',{packet:helper(DATA)});stack.writeUInt32LE(r[6],4);checkRam(c,0x7064,stack);checkRam(c,0x5000,data);
  s=next(s,0x2016,0x486,{2:0x80030050});run(c,unit,2,s,'register-OR-and-first-live-observation',{packet:helper(0x80030050)});data.writeUInt32LE(0x80030050,16);checkRam(c,0x5000,data);
  s=next(s,0x201b,0x482,{1:0x80010010});run(c,unit,2,s,'register-AND-and-second-live-observation',{packet:helper(0x80010010)});data.writeUInt32LE(0x80010010,16);checkRam(c,0x5000,data);
  s=next(s,0x201e,0x482);run(c,unit,1,s,'register-XOR-before-checksum-suffix',{packet:helper(0x80010010)});data.writeUInt32LE(0x80410810,16);checkRam(c,0x5000,data);
  s=next(s,0x3019,0x486,{0:0x80430850,1:0x00020040,4:0x7070,6:r[6]});
  run(c,unit,5,s,'first-original-checksum-and-natural-ESI-return',{packet:helper(0x3019)});counts.compiler_calls++;checkRam(c,0x5000,data);checkRam(c,0x7064,stack);snap('first-return',{cell:0x80410810,checksum:0x80430850});
  for(const i of[3,5,6,7])assert.equal(s.registers[i],r[i]);
  s=next(s,0x2000,0x402,{4:0x706c,5:0x80430850});run(c,unit,7,s,'guest-PUSHes-real-first-return-as-second-flip',{packet:helper(0)});
  stack.set(words([0x3033,DATA,0x80000000,0x0000ffff,0x80430850]),8);checkRam(c,0x7064,stack);
  s=next(s,0x3033,0x402,{0:0x00020040,1:0x80410000,2:0x80410810,4:0x7070,6:r[6]});
  run(c,unit,15,s,'second-whole-original-with-guest-derived-mask',{packet:helper(0x3033)});counts.compiler_calls++;data.writeUInt32LE(0x80430040,16);checkRam(c,0x5000,data);checkRam(c,0x7064,stack);
  snap('second-return',{cell:0x80430040,first_checksum_in_ebp:0x80430850,second_flip_from_first_return:0x80430850,checksum:0x00020040});
  for(const i of[3,6,7])assert.equal(s.registers[i],r[i]);assert.equal(s.registers[5],0x80430850);
  s=next(s,0x303d,0x486,{0:0x80450890,4:0x7080});run(c,unit,4,s,'guest-consumes-both-real-checksum-returns',{packet:helper(0)});data.writeUInt32LE(0x80450890,48);checkRam(c,0x5000,data);checkRam(c,0x7064,stack);
  snap('final',{cell:0x80430040,first_checksum:0x80430850,second_checksum:0x00020040,consumed_checksum_sum:0x80450890});
  const expected=[
    ['store32',0x707c,0x00400800,0],['store32',0x7078,0xfffdffbf,0],['store32',0x7074,0x80010000,0],['store32',0x7070,DATA,0],['store32',0x706c,0x3019,0],
    ['store32',0x7068,r[6],0],['read32',0x707c,null,0x00400800],['read32',0x7078,null,0xfffdffbf],['read32',0x7074,null,0x80010000],['read32',0x7070,null,DATA],
    ['or',DATA,0x80010000,0x00020050],['read32',DATA,null,0x80030050],['and',DATA,0xfffdffbf,0x80030050],['read32',DATA,null,0x80010010],['xor',DATA,0x00400800,0x80010010],['read32',DATA,null,0x80410810],['read32',0x7068,null,r[6]],['read32',0x706c,null,0x3019],
    ['store32',0x707c,0x80430850,0],['store32',0x7078,0x0000ffff,0],['store32',0x7074,0x80000000,0],['store32',0x7070,DATA,0],['store32',0x706c,0x3033,0],
    ['store32',0x7068,r[6],0],['read32',0x707c,null,0x80430850],['read32',0x7078,null,0x0000ffff],['read32',0x7074,null,0x80000000],['read32',0x7070,null,DATA],
    ['or',DATA,0x80000000,0x80410810],['read32',DATA,null,0x80410810],['and',DATA,0x0000ffff,0x80410810],['read32',DATA,null,0x00000810],['xor',DATA,0x80430850,0x00000810],['read32',DATA,null,0x80430040],['read32',0x7068,null,r[6]],['read32',0x706c,null,0x3033],['store32',0x5030,0x80450890,0],
  ];
  const events=helperTrace.slice(at);assert.equal(events.length,37);for(const[i,[kind,address,value,old]]of expected.entries()){const e=events[i];assert.equal(e.name,name(c,kind));assert.equal(e.address,address);assert.equal(e.input_value,value);assert.equal(e.status,0);assert.equal(e.helper_hex,helper(old).toString('hex'));}
  writeFileSync(join(output,`context-${c.id}-compiler-milestones.json`),JSON.stringify({snapshots,expected_helper_chronology:expected},null,2)+'\n',{flag:'wx'});
}
function smcProof(c){
  const head=compile(c,'changed-XOR-register-consumed-code',[{pc:0xc000,bytes:smcBytes,instructions:3}],['xor']),r=parents();r[0]=0x20;r[6]=0xc004;let s=seed(c,0xc000,r);
  const at=helperTrace.length;s=next(s,0xc003,0x402);run(c,head,1,s,'register-XOR-retires-before-changed-MOV',{reason:6,packet:helper(0x1234565a)});counts.smc++;
  assert.equal(helperTrace.length-at,1);const e=helperTrace.at(-1);assert.equal(e.name,name(c,'xor'));assert.equal(e.address,0xc004);assert.equal(e.input_value,0x20);assert.equal(e.status,11);assert.equal(e.helper_hex,helper(0x1234565a).toString('hex'));
  const code=Buffer.concat([smcBytes,Buffer.from([0xa5,0x5a])]);code.writeUInt32LE(0x1234567a,4);checkRam(c,0xc000,code);
  const staleAt=helperTrace.length;run(c,head,1,s,'consumed-owner-stale-neutral',{status:4});counts.stale_owners++;assert.equal(helperTrace.length,staleAt);checkRam(c,0xc000,code);
  const suffix=compile(c,'fresh-MOV-selfloop-suffix-no-XOR-replay',[{pc:0xc003,bytes:code.subarray(3,10),instructions:2}],[]);
  s=next(s,0xc008,0x402,{2:0x1234567a});const suffixAt=helperTrace.length;run(c,suffix,2,s,'fresh-suffix-observes-committed-immediate');counts.fresh_suffixes++;assert.equal(helperTrace.length,suffixAt);checkRam(c,0xc000,code);
}
for(const owner of['replacement','resident'])for(const entries of[false,true]){
  const c=open(owner,entries),probe=compile(c,'register-source-alias-probes',probeBlocks,['or','and','xor']);probeProof(c,probe);faultProof(c,probe);
  const original=compile(c,'whole-original-dynamic-flags-consumer',[...caller,{pc:0x2000,bytes:kernel,instructions:15}],['read32','store32','or','and','xor']);originalProof(c,original);smcProof(c);
  writeFileSync(join(output,`context-${c.id}-final-arena.bin`),arena(c),{flag:'wx'});contexts.push({id:c.id,owner,entries,captures:c.captures,maximum_mapped_pages:6});host(c,'close',[]);
}
assert.deepEqual(sourceHashes(),sourcesBefore);
assert.deepEqual(counts,{contexts:4,modules:16,runs:68,retired:212,helper_calls:172,read32:72,store32:52,atomic_ors:12,atomic_ands:12,atomic_xors:24,compiler_calls:8,probes:12,faults:4,repair_only:4,smc:4,stale_owners:4,fresh_suffixes:4});assert.equal(diagnostics.length,836);
const result={engine:{path:enginePath,sha256:engine.sha256,bytes:engine.bytes.length},source_pins:sourcesBefore,initial_arena_sha256:hash(initial),counts,contexts,modules,observations,helper_trace:helperTrace,public_diagnostics:diagnostics,
  compiler:{text_hex:kernel.toString('hex'),text_sha256:hash(kernel),calls:8,combined_blocks:4,combined_instructions:31},
  limits:{blocks:8,instructions:64,module_bytes:65536,captures_per_context:6,mapped_pages:12},
  verification_ceiling:'per-run complete arena asserted;4final plus16compiler milestone arenas persisted;selected32Bprobe/fault frames,64Bcompiler data,32Bstack and12B SMCcode only,no wholeRAM;one named fault/protect-onlyretry and changedSMC/footer risk added,priorcanonicalfault/MAX/malformed proof reused',
  claim:'finite private flat32 synchronous unshared whole original38B C11consumer plus exact singleF0 09/21/31 DWORDmemory fullregistersources;no hardwareLOCK,C11concurrency,sharedordering,performance or generalcoverage claim'};
assert.deepEqual(Object.keys(result.engine),['path','sha256','bytes']);writeFileSync(join(output,'result.json'),JSON.stringify(result,null,2)+'\n',{flag:'wx'});console.log(JSON.stringify({output,counts,result:join(output,'result.json')}));
