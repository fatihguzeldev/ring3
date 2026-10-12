import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync,writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../../../../../../../../support/wasm/engine.mjs';

const [enginePath,output,root]=process.argv.slice(2),SIZE=4612,DATA=0x5010;
assert.ok(enginePath&&output&&root);
const engine=readEngine(enginePath),initial=readFileSync(join(output,'initial-arena.bin'));
const hash=b=>createHash('sha256').update(b).digest('hex');
assert.equal(initial.length,SIZE);assert.equal(engine.sha256,hash(engine.bytes));
const sourcePaths=['engine/src/cpu/x86/decode/profile.rs','engine/src/cpu/x86/decode/integer.rs','engine/src/cpu/x86/ir.rs',
  'engine/src/cpu/dbt/region.rs','engine/src/cpu/dbt/wasm/emitter.rs','engine/src/cpu/dbt/wasm/integer/arithmetic.rs', 'engine/src/cpu/dbt/wasm/integer/bits.rs', 'engine/src/cpu/dbt/wasm/integer/carry_rotates.rs', 'engine/src/cpu/dbt/wasm/integer/decimal.rs', 'engine/src/cpu/dbt/wasm/integer/dispatch.rs', 'engine/src/cpu/dbt/wasm/integer/flags.rs', 'engine/src/cpu/dbt/wasm/integer/mod.rs', 'engine/src/cpu/dbt/wasm/integer/multiply_divide.rs', 'engine/src/cpu/dbt/wasm/integer/operands.rs', 'engine/src/cpu/dbt/wasm/integer/rotates.rs', 'engine/src/cpu/dbt/wasm/integer/shifts.rs',
  'engine/src/cpu/dbt/wasm/memory.rs','engine/src/cpu/dbt/wasm/memory/exchange.rs','engine/src/cpu/dbt/wasm/locals.rs',
  'engine/src/memory/space.rs','engine/src/process/instance.rs', 'engine/src/process/execution/compiled.rs', 'engine/src/process/execution/authority.rs', 'engine/src/process/memory/address_space.rs', 'engine/src/process/memory/scalar.rs', 'engine/src/process/memory/scalar_tests.rs', 'engine/src/process/execution/mod.rs', 'engine/src/process/memory/mod.rs', 'engine/src/process/mod.rs','engine/src/process/wasm/mod.rs', 'engine/src/process/wasm/loading.rs', 'engine/src/process/wasm/execution.rs', 'engine/src/process/wasm/calls.rs', 'engine/src/process/wasm/memory.rs', 'engine/src/process/wasm/cpu.rs', 'engine/src/process/execution/resident.rs', 'engine/src/abi/memory_helper.rs','engine/src/abi/arena.rs','engine/src/abi/wasm/exports/calls.rs', 'engine/src/abi/wasm/exports/cpu.rs', 'engine/src/abi/wasm/exports/execution.rs', 'engine/src/abi/wasm/exports/image.rs', 'engine/src/abi/wasm/exports/instance.rs', 'engine/src/abi/wasm/exports/memory.rs', 'engine/src/abi/wasm/exports/mod.rs',
  'engine/tests/cpu/atomic/logical/locked/native/immediate32.rs', 'engine/tests/cpu/atomic/native.rs', 'engine/tests/support/cpu/prefix/admission.rs', 'engine/tests/cpu/atomic/logical/flags/constant/compiler/native/dword.rs', 'engine/tests/cpu/atomic/logical/flags/constant/wasm/dword.rs', 'engine/tests/cpu/atomic/wasm.rs', 'engine/tests/support/wasm.rs', ...['kernel.c','build.py','PROVENANCE.md','kernel.obj','kernel.text.bin','kernel.disasm.txt','receipt.json','runtime/run.mjs'].map(p=>'engine/tests/cpu/atomic/logical/flags/constant/fixtures/p2-locked-fullmask32/'+p),
  'engine/tests/programs/records/fixtures/p2-compiled-records/build.py','engine/tests/support/wasm/engine.mjs'];
const sourceHashes=()=>Object.fromEntries(sourcePaths.map(p=>[p,hash(readFileSync(join(root,p)))]));
const sourcesBefore=sourceHashes(),contexts=[],modules=[],observations=[],helperTrace=[],diagnostics=[];
const counts={contexts:0,modules:0,runs:0,retired:0,helper_calls:0,read32:0,store32:0,atomic_ors:0,atomic_ands:0,atomic_xors:0,compiler_calls:0,probes:0};
const word=n=>{const b=Buffer.alloc(4);b.writeUInt32LE(n>>>0);return b;},words=v=>Buffer.concat(v.map(word));
function record(magic,version,size,fields){const b=Buffer.alloc(size);b.write(magic);b.writeUInt16LE(version,4);b.writeUInt16LE(1,6);b.writeUInt32LE(size,8);fields.forEach((v,i)=>b.writeUInt32LE(v>>>0,16+4*i));return b;}
const state=s=>record('R3ST',1,56,[...s.registers,s.pc,s.flags]);
const exit=retired=>record('R3EX',2,40,[1,retired,0,0,0,0]);
const helper=value=>record('R3MH',1,40,[0,value,0,0,0,0]);
const parents=()=>Array.from({length:8},(_,i)=>(0xa100+i)*65536+0x1234+i);
const parity=value=>{let n=value&255,ones=0;while(n){ones+=n&1;n>>>=1;}return ones%2===0?4:0;};
const logicalFlags=value=>0x402|parity(value)|(value===0?0x40:0)|(value>=0x80000000?0x80:0);
assert.equal(logicalFlags(0x80030050),0x486);assert.equal(logicalFlags(0x80010010),0x482);assert.equal(logicalFlags(0x80430850),0x486);
const kernel=readFileSync(join(root,'engine/tests/cpu/atomic/logical/flags/constant/fixtures/p2-locked-fullmask32/kernel.text.bin'));
assert.equal(kernel.toString('hex'),'8b442404f08108000001808b08f08120bffffdff8b10f081300008400031ca8b0031d0c3');
assert.equal(hash(kernel),'d11040b71a34dbb05242d84500ad5df65c19340777e6b2e2673c87374e987dab');
const caller=[
  {pc:0x3000,bytes:Buffer.from('6810500000e8f6efffff','hex'),instructions:2},
  {pc:0x300a,bytes:Buffer.from('83c40489c56810500000e8e7efffff','hex'),instructions:4},
  {pc:0x3019,bytes:Buffer.from('83c40401e8a330500000ebfe','hex'),instructions:4},
];
assert.deepEqual(caller.map(b=>b.pc+b.bytes.length),[0x300a,0x3019,0x3025]);
const probes=[
  {pc:0x1000,bytes:Buffer.from('f0810e80000000','hex'),kind:'or',mask:0x80,old:0,value:0x80,flags:0x402,address:DATA,set:{6:DATA}},
  {pc:0x1020,bytes:Buffer.from('f081642401ff000000','hex'),kind:'and',mask:0xff,old:0x80000180,value:0x80,flags:0x402,address:0x5011,set:{4:DATA}},
  {pc:0x1040,bytes:Buffer.from('f081b48ef0fffffff0660f81','hex'),kind:'xor',mask:0x810f66f0,old:0,value:0x810f66f0,flags:0x486,address:DATA,set:{6:0x5000,1:8}},
];
const probeBlocks=probes.map(p=>({pc:p.pc,bytes:Buffer.concat([p.bytes,Buffer.from([0xeb,0])]),instructions:2}));
const arities={open:3,close:0,arena_ptr:0,map:3,protect:3,upload:2,read32:1,store32:2,store_resident32:6,
  atomic_or_u32:2,atomic_or_resident_u32:6,atomic_and_u32:2,atomic_and_resident_u32:6,atomic_xor_u32:2,atomic_xor_resident_u32:6,
  compile:1,compile_entries:2,compile_resident:1,compile_resident_entries:2,generation:0,module_ptr:0,module_len:0,guard:6,guard_resident:7};
const arena=c=>Buffer.from(new Uint8Array(c.memory.buffer,c.base,SIZE));
const cpu=c=>{const a=arena(c);return{registers:Array.from({length:8},(_,i)=>a.readUInt32LE(16+4*i)),pc:a.readUInt32LE(48),flags:a.readUInt32LE(52)};};
function input(c,offset,bytes){const expected=arena(c);expected.set(bytes,offset);new Uint8Array(c.memory.buffer).set(bytes,c.base+offset);assert.deepEqual(arena(c),expected);}
function host(c,name,args,span){const expected=arena(c);assert.equal(c.api[name](...args),0,name);if(span)expected.set(arena(c).subarray(...span),span[0]);assert.deepEqual(arena(c),expected,'host full4612 '+name);}
function upload(c,address,bytes){input(c,140,bytes);host(c,'upload',[address,bytes.length]);}
function seed(c,pc,registers){const s={registers:[...registers],pc,flags:0xcd7};input(c,0,Buffer.concat([state(s),exit(0),word(0),helper(0xad)]));return s;}
function name(c,kind){if(kind==='read32')return kind;if(kind==='store32')return c.owner==='resident'?'store_resident32':'store32';return 'atomic_'+kind+(c.owner==='resident'?'_resident_u32':'_u32');}
function open(owner,entries){
  const instance=new WebAssembly.Instance(engine.module,{}),c={id:++counts.contexts,owner,entries,memory:instance.exports.memory,api:{},high:0x46554c4c,captures:0};
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
    const payload=args.slice(n.includes('resident')?4:0).map(v=>v>>>0);helperTrace.push({context:c.id,call:counts.runs+1,name:n,address:payload[0],input_value:kind==='read32'?null:payload[1],status,helper_hex:after.subarray(100,140).toString('hex')});
    counts.helper_calls++;counts[kind==='or'?'atomic_ors':kind==='and'?'atomic_ands':kind==='xor'?'atomic_xors':kind]++;return status;};}
  const run=new WebAssembly.Instance(module,{env:{memory:c.memory},ring3}).exports.run;assert.equal(run.length,4);
  const file=`module-${++counts.modules}.wasm`;writeFileSync(join(output,file),bytes,{flag:'wx'});modules.push({context:c.id,owner:c.owner,entries:c.entries,name:title,file,bytes:bytes.length,sha256:hash(bytes),blocks:blocks.length,instructions,imports,...binding});return{...binding,name:title,run};
}
function run(c,unit,budget,s,label,packet){
  const expected=arena(c),at=helperTrace.length;expected.set(state(s));expected.set(exit(budget),56);expected.set(packet,100);
  const status=unit.run(c.base,c.base+56,budget,c.base+96);assert.equal(status,0,label);assert.deepEqual(arena(c),expected,'full4612 '+label);const a=arena(c);
  observations.push({context:c.id,module:unit.name,label,budget,observed_status:status,observed_reason:a.readUInt32LE(72),observed_retired:a.readUInt32LE(76),observed_cpu:cpu(c),helper_calls:helperTrace.length-at,helper_hex:a.subarray(100,140).toString('hex')});counts.runs++;counts.retired+=budget;
}
function checkRam(c,address,expected){for(let i=0;i<expected.length;i+=4){const a=arena(c),value=expected.readUInt32LE(i);assert.equal(c.api.read32(address+i),0);a.set(helper(value),100);assert.deepEqual(arena(c),a,'diagnostic full4612');diagnostics.push({context:c.id,address:address+i,value});}}
function next(s,pc,flags=s.flags,updates={}){const n={registers:[...s.registers],pc,flags};for(const[r,v]of Object.entries(updates))n.registers[Number(r)]=v>>>0;return n;}
function probeProof(c,unit){for(const p of probes){const registers=parents();for(const[r,v]of Object.entries(p.set))registers[Number(r)]=v;const start=p.address-12,frame=Buffer.from(Array.from({length:32},(_,i)=>(i*17+23)&255));frame.writeUInt32LE(p.old,12);upload(c,start,frame);
  const s=seed(c,p.pc,registers);run(c,unit,1,next(s,p.pc+p.bytes.length,p.flags),`full-imm32-${p.kind}`,helper(p.old));counts.probes++;frame.writeUInt32LE(p.value,12);checkRam(c,start,frame);
  const e=helperTrace.at(-1);assert.equal(e.name,name(c,p.kind));assert.equal(e.address,p.address);assert.equal(e.input_value,p.mask);assert.equal(e.status,0);assert.equal(e.helper_hex,helper(p.old).toString('hex'));}}
function originalProof(c,unit){
  const data=Buffer.from(Array.from({length:64},(_,i)=>(i*13+7)&255));data.writeUInt32LE(0x00020050,16);data.writeUInt32LE(0,48);upload(c,0x5000,data);
  const stack=Buffer.from(Array.from({length:32},(_,i)=>(i*17+23)&255));upload(c,0x706c,stack);
  const r=parents();r[4]=0x7080;let s=seed(c,0x3000,r);const snapshots=[],at=helperTrace.length;
  const snap=(label,extra={})=>snapshots.push({label,cpu:cpu(c),arena_hex:arena(c).toString('hex'),...extra});snap('entry');
  s=next(s,0x2000,0xcd7,{4:0x7078});run(c,unit,2,s,'first-natural-cdecl-call',helper(0));stack.set(words([0x300a,DATA]),12);checkRam(c,0x706c,stack);
  s=next(s,0x2004,0xcd7,{0:DATA});run(c,unit,1,s,'first-original-pauses-before-lock81',helper(DATA));checkRam(c,0x5000,data);
  s=next(s,0x200d,0x486,{1:0x80030050});run(c,unit,2,s,'original-fullmask-or-and-first-live-load',helper(0x80030050));data.writeUInt32LE(0x80030050,16);checkRam(c,0x5000,data);
  s=next(s,0x2016,0x482,{2:0x80010010});run(c,unit,2,s,'original-fullmask-and-and-second-live-load',helper(0x80010010));data.writeUInt32LE(0x80010010,16);checkRam(c,0x5000,data);
  s=next(s,0x201d,0x482);run(c,unit,1,s,'original-fullmask-xor-before-checksum',helper(0x80010010));data.writeUInt32LE(0x80410810,16);checkRam(c,0x5000,data);
  s=next(s,0x300a,0x486,{0:0x80430850,2:0x00020040,4:0x707c});run(c,unit,4,s,'first-original-checksum-natural-return',helper(0x300a));counts.compiler_calls++;snap('first-return',{cell:0x80410810,checksum:0x80430850});
  for(const i of[3,5,6,7])assert.equal(s.registers[i],r[i]);
  s=next(s,0x2000,0x412,{4:0x7078,5:0x80430850});run(c,unit,4,s,'guest-retains-first-checksum-and-calls-again',helper(0));stack.set(words([0x3019,DATA]),12);checkRam(c,0x706c,stack);
  s=next(s,0x3019,0x482,{0:0x80010010,1:0x80410810,2:0,4:0x707c});run(c,unit,10,s,'second-whole-original-natural-return',helper(0x3019));counts.compiler_calls++;data.writeUInt32LE(0x80010010,16);checkRam(c,0x5000,data);snap('second-return',{cell:0x80010010,first_checksum_in_ebp:0x80430850,checksum:0x80010010});
  for(const i of[3,6,7])assert.equal(s.registers[i],r[i]);assert.equal(s.registers[5],0x80430850);
  s=next(s,0x3023,0xc07,{0:0x00440860,4:0x7080});run(c,unit,4,s,'guest-consumes-both-checksum-returns',helper(0));data.writeUInt32LE(0x00440860,48);checkRam(c,0x5000,data);checkRam(c,0x706c,stack);
  snap('final',{cell:0x80010010,first_checksum:0x80430850,second_checksum:0x80010010,consumed_checksum_sum:0x00440860});
  const expected=[['store32',0x707c,DATA,0],['store32',0x7078,0x300a,0],['read32',0x707c,null,DATA],['or',DATA,0x80010000,0x00020050],['read32',DATA,null,0x80030050],['and',DATA,0xfffdffbf,0x80030050],['read32',DATA,null,0x80010010],['xor',DATA,0x00400800,0x80010010],['read32',DATA,null,0x80410810],['read32',0x7078,null,0x300a],
    ['store32',0x707c,DATA,0],['store32',0x7078,0x3019,0],['read32',0x707c,null,DATA],['or',DATA,0x80010000,0x80410810],['read32',DATA,null,0x80410810],['and',DATA,0xfffdffbf,0x80410810],['read32',DATA,null,0x80410810],['xor',DATA,0x00400800,0x80410810],['read32',DATA,null,0x80010010],['read32',0x7078,null,0x3019],['store32',0x5030,0x00440860,0]];
  const events=helperTrace.slice(at);assert.equal(events.length,21);for(const[i,[kind,address,value,old]]of expected.entries()){const e=events[i];assert.equal(e.name,name(c,kind));assert.equal(e.address,address);assert.equal(e.input_value,value);assert.equal(e.status,0);assert.equal(e.helper_hex,helper(old).toString('hex'));}
  writeFileSync(join(output,`context-${c.id}-compiler-milestones.json`),JSON.stringify({snapshots,expected_helper_chronology:expected},null,2)+'\n',{flag:'wx'});
}
for(const owner of['replacement','resident'])for(const entries of[false,true]){
  const c=open(owner,entries),probe=compile(c,'full-imm32-decoder-probes',probeBlocks,['or','and','xor']);probeProof(c,probe);
  const original=compile(c,'whole-original-fullmask-consumer',[...caller,{pc:0x2000,bytes:kernel,instructions:10}],['read32','store32','or','and','xor']);originalProof(c,original);
  writeFileSync(join(output,`context-${c.id}-final-arena.bin`),arena(c),{flag:'wx'});contexts.push({id:c.id,owner,entries,captures:c.captures,maximum_mapped_pages:5});host(c,'close',[]);
}
assert.deepEqual(sourceHashes(),sourcesBefore);
assert.deepEqual(counts,{contexts:4,modules:8,runs:48,retired:132,helper_calls:96,read32:40,store32:20,atomic_ors:12,atomic_ands:12,atomic_xors:12,compiler_calls:8,probes:12});assert.equal(diagnostics.length,576);
const result={engine:{path:enginePath,sha256:engine.sha256,bytes:engine.bytes.length},source_pins:sourcesBefore,initial_arena_sha256:hash(initial),counts,contexts,modules,observations,helper_trace:helperTrace,public_diagnostics:diagnostics,
  compiler:{text_hex:kernel.toString('hex'),text_sha256:hash(kernel),calls:8,combined_blocks:4,combined_instructions:20},
  limits:{blocks:8,instructions:64,module_bytes:65536,captures_per_context:6,mapped_pages:12},
  verification_ceiling:'per-run full4612 asserted;4final plus16compiler milestone arenas persisted;selected32Bprobe frames/64Bcompiler data/32Bstack only,no wholeRAM;helpers/ABI/backend unchanged and priorfault/MAX/malformed/SMC proof reused',
  claim:'finite private flat32 synchronous unshared whole original36B C11consumer plus exact singleF081 DWORDmemory OR/AND/XOR fullimm32;no hardwareLOCK,C11concurrency,sharedordering,performance or generalcoverage claim'};
assert.deepEqual(Object.keys(result.engine),['path','sha256','bytes']);writeFileSync(join(output,'result.json'),JSON.stringify(result,null,2)+'\n',{flag:'wx'});console.log(JSON.stringify({output,counts,result:join(output,'result.json')}));
