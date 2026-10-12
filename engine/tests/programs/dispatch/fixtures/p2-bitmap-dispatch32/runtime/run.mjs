import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync,writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../../../../../support/wasm/engine.mjs';

const [enginePath,output,root]=process.argv.slice(2),SIZE=4612,DATA=0x5010;
assert.ok(enginePath&&output&&root);
const engine=readEngine(enginePath),initial=readFileSync(join(output,'initial-arena.bin'));
const hash=b=>createHash('sha256').update(b).digest('hex');
assert.equal(initial.length,SIZE);assert.equal(engine.sha256,hash(engine.bytes));
const sourcePaths=['engine/src/cpu/x86/ir.rs','engine/src/cpu/x86/decode/profile.rs','engine/src/cpu/x86/decode/integer.rs',
  'engine/src/cpu/dbt/region.rs','engine/src/cpu/dbt/wasm/integer/arithmetic.rs', 'engine/src/cpu/dbt/wasm/integer/bits.rs', 'engine/src/cpu/dbt/wasm/integer/carry_rotates.rs', 'engine/src/cpu/dbt/wasm/integer/decimal.rs', 'engine/src/cpu/dbt/wasm/integer/dispatch.rs', 'engine/src/cpu/dbt/wasm/integer/flags.rs', 'engine/src/cpu/dbt/wasm/integer/mod.rs', 'engine/src/cpu/dbt/wasm/integer/multiply_divide.rs', 'engine/src/cpu/dbt/wasm/integer/operands.rs', 'engine/src/cpu/dbt/wasm/integer/rotates.rs', 'engine/src/cpu/dbt/wasm/integer/shifts.rs','engine/src/cpu/dbt/wasm/memory.rs',
  'engine/src/cpu/dbt/wasm/emitter.rs','engine/src/cpu/dbt/wasm/locals.rs','engine/src/memory/space.rs',
  'engine/src/process/instance.rs', 'engine/src/process/execution/compiled.rs', 'engine/src/process/execution/authority.rs', 'engine/src/process/memory/address_space.rs', 'engine/src/process/memory/scalar.rs', 'engine/src/process/memory/scalar_tests.rs', 'engine/src/process/execution/mod.rs', 'engine/src/process/memory/mod.rs', 'engine/src/process/mod.rs','engine/src/process/wasm/mod.rs', 'engine/src/process/wasm/loading.rs', 'engine/src/process/wasm/execution.rs', 'engine/src/process/wasm/calls.rs', 'engine/src/process/wasm/memory.rs', 'engine/src/process/wasm/cpu.rs','engine/src/process/execution/resident.rs',
  'engine/src/abi/memory_helper.rs','engine/src/abi/arena.rs','engine/src/abi/x86/exit.rs','engine/src/abi/wasm/exports/calls.rs', 'engine/src/abi/wasm/exports/cpu.rs', 'engine/src/abi/wasm/exports/execution.rs', 'engine/src/abi/wasm/exports/image.rs', 'engine/src/abi/wasm/exports/instance.rs', 'engine/src/abi/wasm/exports/memory.rs', 'engine/src/abi/wasm/exports/mod.rs',
  'engine/tests/cpu/bits_flags/count/native/trailing32.rs', 'engine/tests/cpu/bits_flags/native.rs', 'engine/tests/support/cpu/prefix/admission.rs','engine/tests/programs/dispatch/native/bitmap.rs', 'engine/tests/programs/native.rs','engine/tests/programs/dispatch/wasm/bitmap.rs', 'engine/tests/programs/wasm.rs', 'engine/tests/support/wasm.rs',
  ...['kernel.c','build.py','PROVENANCE.md','kernel.obj','kernel.text.bin','kernel.disasm.txt','receipt.json','runtime/run.mjs'].map(p=>'engine/tests/programs/dispatch/fixtures/p2-bitmap-dispatch32/'+p),
  'engine/tests/programs/records/fixtures/p2-compiled-records/build.py','engine/tests/support/wasm/engine.mjs'];
const sourceHashes=()=>Object.fromEntries(sourcePaths.map(p=>[p,hash(readFileSync(join(root,p)))]));
const sourcesBefore=sourceHashes(),contexts=[],modules=[],observations=[],helperTrace=[],diagnostics=[],repairs=[];
const counts={contexts:0,modules:0,runs:0,retired:0,helper_calls:0,read32:0,store32:0,register_probes:0,memory_probes:0,read_faults:0,repair_only:0,compiler_calls:0};
const word=n=>{const b=Buffer.alloc(4);b.writeUInt32LE(n>>>0);return b;},words=v=>Buffer.concat(v.map(word));
function record(magic,version,size,fields){const b=Buffer.alloc(size);b.write(magic);b.writeUInt16LE(version,4);b.writeUInt16LE(1,6);b.writeUInt32LE(size,8);fields.forEach((v,i)=>b.writeUInt32LE(v>>>0,16+4*i));return b;}
const state=s=>record('R3ST',1,56,[...s.registers,s.pc,s.flags]);
const exit=(reason,retired,{detail=0,address=0,access=0}={})=>record('R3EX',2,40,[reason,retired,detail,address,access,access?4:0]);
const helper=(value=0,{detail=0,address=0,access=0}={})=>record('R3MH',1,40,[access?1:0,value,detail,address,access,access?4:0]);
const parents=()=>Array.from({length:8},(_,i)=>(0xa100+i)*65536+0x1234+i);
const countZero=value=>{if(value===0)return 32;let bit=0;while((BigInt(value)&(1n<<BigInt(bit)))===0n)bit++;return bit;};
const countFlags=(value,result)=>0x402|(value===0?1:0)|(result===0?0x40:0);
assert.equal(countZero(0),32);assert.equal(countFlags(0,32),0x403);assert.equal(countFlags(1,0),0x442);assert.equal(countZero(0x80400010),4);
const kernel=readFileSync(join(root,'engine/tests/programs/dispatch/fixtures/p2-bitmap-dispatch32/kernel.text.bin'));
assert.equal(kernel.toString('hex'),'57568b4c240c8b118b74241021d67410f30fbcc689f7f7df21f731d78939eb05b8200000005e5fc3');
assert.equal(hash(kernel),'f720ba26c3c06465c22b0c1b980cc5023f63d721ada20dbb01cef016d8568024');
const originalBlocks=[[0,16,7],[16,16,7],[32,5,1],[37,3,3]].map(([at,size,instructions])=>({pc:0x2000+at,bytes:kernel.subarray(at,at+size),instructions}));
const caller=[
  {pc:0x3000,bytes:Buffer.from('6a286810500000e8f4efffff','hex'),instructions:3},
  {pc:0x300c,bytes:Buffer.from('83c40889c56a286810500000e8e3efffff','hex'),instructions:5},
  {pc:0x301d,bytes:Buffer.from('83c408c1e50809c56a286810500000e8cfefffff','hex'),instructions:6},
  {pc:0x3031,bytes:Buffer.from('83c408c1e50809e8a330500000ebfe','hex'),instructions:5},
];
assert.deepEqual(caller.map(b=>b.pc+b.bytes.length),[0x300c,0x301d,0x3031,0x3040]);
const probeBlocks=[{pc:0x1000,bytes:Buffer.from('f30fbcc0ebfe','hex'),instructions:2},{pc:0x1010,bytes:Buffer.from('f30fbc4488f0ebfe','hex'),instructions:2}];
const arities={open:3,close:0,arena_ptr:0,map:3,protect:3,upload:2,read32:1,store32:2,store_resident32:6,
  compile:1,compile_entries:2,compile_resident:1,compile_resident_entries:2,generation:0,module_ptr:0,module_len:0,guard:6,guard_resident:7};
const arena=c=>Buffer.from(new Uint8Array(c.memory.buffer,c.base,SIZE));
const cpu=c=>{const a=arena(c);return{registers:Array.from({length:8},(_,i)=>a.readUInt32LE(16+4*i)),pc:a.readUInt32LE(48),flags:a.readUInt32LE(52)};};
function input(c,offset,bytes){const expected=arena(c);expected.set(bytes,offset);new Uint8Array(c.memory.buffer).set(bytes,c.base+offset);assert.deepEqual(arena(c),expected);}
function host(c,name,args,span){const expected=arena(c);assert.equal(c.api[name](...args),0,name);if(span)expected.set(arena(c).subarray(...span),span[0]);assert.deepEqual(arena(c),expected,'host full4612 '+name);}
function upload(c,address,bytes){input(c,140,bytes);host(c,'upload',[address,bytes.length]);}
function seed(c,pc,registers){const s={registers:[...registers],pc,flags:0xcd7};input(c,0,Buffer.concat([state(s),exit(1,0),word(0),helper(0xad)]));return s;}
function name(c,kind){return kind==='store32'&&c.owner==='resident'?'store_resident32':kind;}
function next(s,pc,flags=s.flags,updates={}){const n={registers:[...s.registers],pc,flags};for(const[r,v]of Object.entries(updates))n.registers[Number(r)]=v>>>0;return n;}
function open(owner,entries){
  const instance=new WebAssembly.Instance(engine.module,{}),c={id:++counts.contexts,owner,entries,memory:instance.exports.memory,api:{},high:0x4249544d,captures:0};
  for(const[n,arity]of Object.entries(arities)){c.api[n]=instance.exports['ring3_abi_v1_'+n];assert.equal(c.api[n]?.length,arity,n);}
  assert.equal(c.api.open(12,c.id,c.high),0);c.base=c.api.arena_ptr()>>>0;input(c,0,initial);
  for(const[page,permission]of[[0x1000,7],[0x2000,7],[0x3000,7],[0x5000,3],[0x7000,3]])host(c,'map',[page,1,permission]);
  for(const b of[...probeBlocks,...caller,{pc:0x2000,bytes:kernel}])upload(c,b.pc,b.bytes);
  for(const page of[0x1000,0x2000,0x3000])host(c,'protect',[page,1,5]);return c;
}
function compile(c,title,blocks,kinds){
  assert.ok(++c.captures<=6);assert.ok(blocks.length<=8);const instructions=blocks.reduce((n,b)=>n+b.instructions,0);assert.ok(instructions<=64);
  input(c,140,words(blocks.flatMap(b=>c.entries?[b.pc]:[b.pc,b.bytes.length])));
  const method=c.owner==='resident'?(c.entries?'compile_resident_entries':'compile_resident'):(c.entries?'compile_entries':'compile');
  host(c,method,c.entries?[blocks.length,0]:[blocks.length],c.owner==='resident'?[140,164]:null);
  const a=arena(c),binding=c.owner==='resident'?{low:a.readUInt32LE(148),high:a.readUInt32LE(152),pointer:a.readUInt32LE(156),length:a.readUInt32LE(160)}:{generation:c.api.generation(),pointer:c.api.module_ptr()>>>0,length:c.api.module_len()>>>0};
  if(c.owner==='resident'){assert.equal(a.readUInt32LE(140),1);assert.equal(a.readUInt32LE(144),24);assert.ok(binding.low||binding.high);}
  const bytes=Buffer.from(new Uint8Array(c.memory.buffer,binding.pointer,binding.length)),module=new WebAssembly.Module(bytes),names=[c.owner==='resident'?'guard_resident':'guard',...kinds.map(k=>name(c,k))];
  const imports=[{module:'env',name:'memory',kind:'memory'},...names.map(n=>({module:'ring3',name:n,kind:'function'}))];
  assert.ok(bytes.length<=65536);assert.deepEqual(WebAssembly.Module.imports(module),imports);assert.deepEqual(WebAssembly.Module.exports(module),[{name:'run',kind:'function'}]);
  const ring3={...c.api};for(const kind of kinds){const n=name(c,kind);ring3[n]=(...args)=>{const before=arena(c),status=c.api[n](...args),after=arena(c);before.set(after.subarray(100,140),100);assert.deepEqual(after,before,'canonical helper40B only');
    const payload=args.slice(n.includes('resident')?4:0).map(v=>v>>>0);helperTrace.push({context:c.id,call:counts.runs+1,name:n,address:payload[0],input_value:kind==='read32'?null:payload[1],status,helper_hex:after.subarray(100,140).toString('hex')});counts.helper_calls++;counts[kind]++;return status;};}
  const run=new WebAssembly.Instance(module,{env:{memory:c.memory},ring3}).exports.run;assert.equal(run.length,4);
  const file=`module-${++counts.modules}.wasm`;writeFileSync(join(output,file),bytes,{flag:'wx'});modules.push({context:c.id,owner:c.owner,entries:c.entries,name:title,file,bytes:bytes.length,sha256:hash(bytes),blocks:blocks.length,instructions,imports,...binding});return{...binding,name:title,run};
}
function run(c,unit,budget,s,label,{reason=1,retired=budget,packet,fault={}}={}){
  const expected=arena(c),at=helperTrace.length;expected.set(state(s));expected.set(exit(reason,retired,fault),56);if(packet)expected.set(packet,100);
  const status=unit.run(c.base,c.base+56,budget,c.base+96);assert.equal(status,0,label);assert.deepEqual(arena(c),expected,'full4612 '+label);const a=arena(c);
  observations.push({context:c.id,module:unit.name,label,budget,observed_status:status,observed_reason:a.readUInt32LE(72),observed_retired:a.readUInt32LE(76),observed_cpu:cpu(c),exit_hex:a.subarray(56,96).toString('hex'),helper_calls:helperTrace.length-at,helper_hex:a.subarray(100,140).toString('hex')});counts.runs++;counts.retired+=retired;
}
function checkRam(c,address,expected){for(let i=0;i<expected.length;i+=4){const before=arena(c),value=expected.readUInt32LE(i);assert.equal(c.api.read32(address+i),0);before.set(helper(value),100);assert.deepEqual(arena(c),before,'diagnostic full4612');diagnostics.push({context:c.id,address:address+i,value});}}
function frame(c,value){const bytes=Buffer.from(Array.from({length:32},(_,i)=>(i*17+23)&255));bytes.writeUInt32LE(value>>>0,12);upload(c,DATA-12,bytes);return bytes;}
function assertRead(c,value,fault={}){const e=helperTrace.at(-1);assert.equal(e.name,'read32');assert.equal(e.address,DATA);assert.equal(e.input_value,null);assert.equal(e.status,0);assert.equal(e.helper_hex,helper(value,fault).toString('hex'));}
function probeProof(c,unit){
  for(const value of[0,...Array.from({length:32},(_,i)=>2**i)]){const registers=parents();registers[0]=value;const s=seed(c,0x1000,registers),result=countZero(value),at=helperTrace.length;
    run(c,unit,1,next(s,0x1004,countFlags(value,result),{0:result}),'register-self-alias-'+value.toString(16));assert.equal(helperTrace.length,at);counts.register_probes++;}
  for(const value of[0,1,0x80000000,0x80400010]){const bytes=frame(c,value),registers=parents();registers[0]=DATA;registers[1]=4;const s=seed(c,0x1010,registers),result=countZero(value);
    run(c,unit,1,next(s,0x1016,countFlags(value,result),{0:result}),'memory-destination-ea-alias-'+value.toString(16),{packet:helper(value)});assertRead(c,value);checkRam(c,DATA-12,bytes);counts.memory_probes++;}
  const bytes=frame(c,0x100),registers=parents();registers[0]=DATA;registers[1]=4;host(c,'protect',[0x5000,1,0]);const s=seed(c,0x1010,registers),fault={detail:2,address:DATA,access:1};
  run(c,unit,1,s,'memory-read-permission-stops-before-publication',{reason:5,retired:0,packet:helper(0,fault),fault});assertRead(c,0,fault);counts.read_faults++;
  const before=arena(c),stopped=cpu(c);host(c,'protect',[0x5000,1,3]);assert.deepEqual(arena(c),before);assert.deepEqual(cpu(c),stopped);
  run(c,unit,1,next(s,0x1016,0x402,{0:8}),'memory-protect-only-same-input-retry',{packet:helper(0x100)});assertRead(c,0x100);checkRam(c,DATA-12,bytes);counts.repair_only++;
  repairs.push({context:c.id,kind:'ReadPermission-to-RW',address:DATA,value:0x100,input_cpu:stopped,repair:'protect only; no CPU input/upload/remap/recapture',module:unit.name});
}
function originalProof(c,unit){
  const data=Buffer.from(Array.from({length:64},(_,i)=>(i*13+7)&255));data.writeUInt32LE(0x80000128,16);data.writeUInt32LE(0,48);upload(c,0x5000,data);
  const stack=Buffer.from(Array.from({length:64},(_,i)=>(i*17+23)&255));upload(c,0x7050,stack);
  const r=parents();r[4]=0x7078;let s=seed(c,0x3000,r);const at=helperTrace.length,snapshots=[];
  const snapshot=(label,extra)=>snapshots.push({label,cpu:cpu(c),arena_hex:arena(c).toString('hex'),...extra});
  const checks=()=>{checkRam(c,0x5000,data);checkRam(c,0x7050,stack);};
  function stackAfter(returnPc){for(const[address,value]of[[0x7064,r[6]],[0x7068,r[7]],[0x706c,returnPc],[0x7070,DATA],[0x7074,0x28]])stack.writeUInt32LE(value>>>0,address-0x7050);}
  s=next(s,0x300c,0x482,{0:3,1:DATA,2:0x80000128,4:0x7070});run(c,unit,20,s,'first-natural-bitmap-return-3',{packet:helper(0x300c)});counts.compiler_calls++;
  data.writeUInt32LE(0x80000120,16);stackAfter(0x300c);snapshot('first-return',{slot:3,pending:0x80000120});checks();
  s=next(s,0x301d,0x486,{0:5,2:0x80000120,5:3});run(c,unit,22,s,'second-natural-bitmap-return-5',{packet:helper(0x301d)});counts.compiler_calls++;
  data.writeUInt32LE(0x80000100,16);stackAfter(0x301d);snapshot('second-return',{slot:5,pending:0x80000100,first_slot_in_ebp:3});checks();
  s=next(s,0x3031,0x446,{0:32,2:0x80000100,5:0x305});run(c,unit,17,s,'third-natural-bitmap-empty-sentinel-32',{packet:helper(0x3031)});counts.compiler_calls++;
  stackAfter(0x3031);snapshot('empty-return',{slot:32,pending:0x80000100,first_two_slots_in_ebp:0x305});checks();
  s=next(s,0x303e,0x402,{0:0x30520,4:0x7078,5:0x30500});run(c,unit,5,s,'guest-consumes-three-natural-slot-returns',{packet:helper(0)});
  data.writeUInt32LE(0x30520,48);snapshot('final',{packed_slots:0x30520,pending:0x80000100});checks();
  for(const index of[3,6,7])assert.equal(cpu(c).registers[index],r[index]);
  const expected=[];
  for(const[returnPc,old,value]of[[0x300c,0x80000128,0x80000120],[0x301d,0x80000120,0x80000100],[0x3031,0x80000100,null]]){
    for(const[address,input]of[[0x7074,0x28],[0x7070,DATA],[0x706c,returnPc],[0x7068,r[7]],[0x7064,r[6]]])expected.push(['store32',address,input,0]);
    for(const[address,read]of[[0x7070,DATA],[DATA,old],[0x7074,0x28]])expected.push(['read32',address,null,read]);
    if(value!==null)expected.push(['store32',DATA,value,0]);
    for(const[address,read]of[[0x7064,r[6]],[0x7068,r[7]],[0x706c,returnPc]])expected.push(['read32',address,null,read]);
  }
  expected.push(['store32',0x5030,0x30520,0]);const events=helperTrace.slice(at);assert.equal(events.length,36);
  for(const[i,[kind,address,input,read]]of expected.entries()){const e=events[i];assert.equal(e.name,name(c,kind));assert.equal(e.address,address);assert.equal(e.input_value,input);assert.equal(e.status,0);assert.equal(e.helper_hex,helper(read).toString('hex'));}
  assert.deepEqual(observations.slice(-4).map(row=>row.helper_calls),[12,12,11,1]);
  writeFileSync(join(output,`context-${c.id}-compiler-milestones.json`),JSON.stringify({snapshots,expected_helper_chronology:expected},null,2)+'\n',{flag:'wx'});
}
for(const owner of['replacement','resident'])for(const entries of[false,true]){
  const c=open(owner,entries),probe=compile(c,'tzcnt-alias-probes',probeBlocks,['read32']);probeProof(c,probe);
  const original=compile(c,'whole-original-bitmap-dispatch',[...originalBlocks,...caller],['read32','store32']);originalProof(c,original);
  writeFileSync(join(output,`context-${c.id}-final-arena.bin`),arena(c),{flag:'wx'});contexts.push({id:c.id,owner,entries,captures:c.captures,maximum_mapped_pages:5});host(c,'close',[]);
}
assert.deepEqual(sourceHashes(),sourcesBefore);
assert.deepEqual(counts,{contexts:4,modules:8,runs:172,retired:408,helper_calls:168,read32:96,store32:72,register_probes:132,memory_probes:16,read_faults:4,repair_only:4,compiler_calls:12});assert.equal(diagnostics.length,672);
const result={engine:{path:enginePath,sha256:engine.sha256,bytes:engine.bytes.length},source_pins:sourcesBefore,initial_arena_sha256:hash(initial),counts,contexts,modules,observations,helper_trace:helperTrace,public_diagnostics:diagnostics,repairs,
  compiler:{text_hex:kernel.toString('hex'),text_sha256:hash(kernel),calls:12,caller_hex:Buffer.concat(caller.map(b=>b.bytes)).toString('hex'),combined_blocks:8,combined_instructions:37},
  limits:{blocks:8,instructions:64,module_bytes:65536,captures_per_context:6,mapped_pages:12},
  verification_ceiling:'per-run full4612 CPU/exit/helper/dirtyFP asserted;16original return/final milestone arenas plus4final arenas persisted;selected32Bmemory probe frames/64Boriginal data/64Bstack only,no wholeRAM;diagnostics are expectedvalues asserted against actual canonical packets, not separately persisted raw packets;no generic model or physical parser added',
  claim:'exact software DWORD TZCNT reg/mem and one guarded whole40B original bitmap consumer, zeroFLAGS proven by direct probes;no BSFalias,CPUID/BMI1bundle,broaderREP,concurrency,performance or generalcompiler compatibility claim'};
assert.deepEqual(Object.keys(result.engine),['path','sha256','bytes']);writeFileSync(join(output,'result.json'),JSON.stringify(result,null,2)+'\n',{flag:'wx'});console.log(JSON.stringify({output,counts,result:join(output,'result.json')}));
