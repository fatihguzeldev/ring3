import {attachDetectionApi, generatedImports} from '../../../../../../support/wasm/binding.mjs';
import {ARENA_SIZE} from '../../../../../../support/wasm/arena.mjs';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync,writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../../../../../../support/wasm/engine.mjs';

const [enginePath,output,root]=process.argv.slice(2),SIZE=ARENA_SIZE,DATA=0x5010;
assert.ok(enginePath&&output&&root);
const engine=readEngine(enginePath),initial=readFileSync(join(output,'initial-arena.bin'));
const hash=b=>createHash('sha256').update(b).digest('hex');
assert.equal(initial.length,SIZE);assert.equal(engine.sha256,hash(engine.bytes));
const sourcePaths=['engine/tests/support/wasm/binding.mjs', 'engine/src/cpu/x86/decode/profile.rs', 'engine/src/cpu/x86/decode/families.rs','engine/src/cpu/x86/decode/lower.rs', 'engine/src/cpu/x86/decode/families.rs','engine/src/cpu/x86/decode/integer.rs','engine/src/cpu/x86/decode/flow.rs','engine/src/cpu/x86/ir.rs',
  'engine/src/cpu/dbt/region.rs','engine/src/cpu/dbt/cold.rs','engine/src/cpu/dbt/resident.rs','engine/src/cpu/dbt/wasm/emitter/mod.rs', 'engine/src/cpu/dbt/wasm/emitter/module.rs', 'engine/src/cpu/dbt/wasm/emitter/body.rs', 'engine/src/cpu/dbt/wasm/requirements.rs', 'engine/src/cpu/dbt/wasm/imports/wiring.rs','engine/src/cpu/dbt/wasm/instructions/integer/arithmetic.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/bits.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/carry_rotates.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/decimal.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/flags.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/mod.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/multiply_divide.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/operands.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/rotates.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/shifts.rs',
  'engine/src/cpu/dbt/wasm/imports/mod.rs', 'engine/src/cpu/dbt/wasm/imports/binding.rs', 'engine/src/cpu/dbt/wasm/imports/context.rs', 'engine/src/cpu/dbt/wasm/imports/needs.rs', 'engine/src/cpu/dbt/wasm/helpers/protocol.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/mod.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/address.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/scalar.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/stack.rs','engine/src/cpu/dbt/wasm/instructions/memory/exchange.rs','engine/src/cpu/dbt/wasm/locals.rs',
  'engine/src/memory/space.rs', 'engine/src/memory/space/atomic.rs','engine/src/process/instance.rs', 'engine/src/process/execution/compiled.rs', 'engine/src/process/execution/authority.rs', 'engine/src/process/memory/address_space.rs', 'engine/src/process/memory/scalar.rs', 'engine/src/process/memory/scalar/tests.rs', 'engine/src/process/execution/mod.rs', 'engine/src/process/memory/mod.rs', 'engine/src/process/mod.rs','engine/src/process/wasm/mod.rs', 'engine/src/process/wasm/loading.rs', 'engine/src/abi/image.rs', 'engine/src/process/wasm/execution.rs', 'engine/src/abi/execution.rs', 'engine/src/process/wasm/calls.rs', 'engine/src/process/wasm/memory.rs', 'engine/src/process/wasm/cpu.rs','engine/src/process/execution/resident.rs',
  'engine/src/abi/memory_helper.rs','engine/src/abi/arena.rs','engine/src/abi/wasm/exports/calls.rs', 'engine/src/abi/wasm/exports/cpu.rs', 'engine/src/abi/wasm/exports/execution.rs', 'engine/src/abi/wasm/exports/image.rs', 'engine/src/abi/wasm/exports/instance.rs', 'engine/src/abi/wasm/exports/memory.rs', 'engine/src/abi/wasm/exports/mod.rs',
  'engine/tests/programs/atomic/flags/native.rs', 'engine/tests/programs/native.rs','engine/tests/programs/atomic/flags/wasm.rs', 'engine/tests/programs/wasm.rs', 'engine/tests/support/wasm.rs',
  ...['kernel.c','build.py','PROVENANCE.md','kernel.obj','kernel.text.bin','kernel.disasm.txt','receipt.json','runtime/run.mjs'].map(p=>'engine/tests/programs/atomic/flags/fixtures/p2-returned-atomic-flags32/'+p),
  'engine/tests/programs/records/fixtures/p2-compiled-records/build.py','engine/tests/support/wasm/engine.mjs'];
const sourceHashes=()=>Object.fromEntries(sourcePaths.map(p=>[p,hash(readFileSync(join(root,p)))]));
const sourcesBefore=sourceHashes(),contexts=[],modules=[],observations=[],helperTrace=[],diagnostics=[],lifecycle=[];
const counts={contexts:0,modules:0,runs:0,retired:0,helper_calls:0,read32:0,store32:0,compare_exchange32:0,compiler_returns:0,cold_exits:0,budget_exits:0,resident_discards:0};
const word=n=>{const b=Buffer.alloc(4);b.writeUInt32LE(n>>>0);return b;},words=v=>Buffer.concat(v.map(word));
function record(magic,version,size,fields){const b=Buffer.alloc(size);b.write(magic);b.writeUInt16LE(version,4);b.writeUInt16LE(1,6);b.writeUInt32LE(size,8);fields.forEach((v,i)=>b.writeUInt32LE(v>>>0,16+4*i));return b;}
const state=s=>record('R3ST',1,56,[...s.registers,s.pc,s.flags]);
const exit=(reason,retired)=>record('R3EX',2,40,[reason,retired,0,0,0,0]);
const helper=value=>record('R3MH',1,40,[0,value,0,0,0,0]);
const cpuHelper=value=>record('R3MH',10,40,[0,value,0,0,0,4]);
const cpuExit=(reason,retired)=>record('R3EX',6,40,[reason,retired,0,0,0,0]);;
const parents=()=>Array.from({length:8},(_,i)=>(0xa100+i)*65536+0x1234+i);
const parity=value=>{let n=value&255,ones=0;while(n){ones+=n&1;n>>>=1;}return ones%2===0?4:0;};
function addFlags(a,b){const value=(a+b)>>>0;return 0x402|parity(value)|(value===0?0x40:0)|(value>=0x80000000?0x80:0)|(a+b>0xffffffff?1:0)|(((a^b^value)&16)?16:0)|((~(a^b)&(a^value)&0x80000000)?0x800:0);}
assert.equal(addFlags(0x800500a0,0x80010010),0xc03);assert.equal(addFlags(0x00821020,0x810),0x406);assert.equal(addFlags(0x00821830,0x600b0),0x402);
const kernel=readFileSync(join(root,'engine/tests/programs/atomic/flags/fixtures/p2-returned-atomic-flags32/kernel.text.bin'));
assert.equal(kernel.toString('hex'),'5357568b4c241c8b7424188b7c24148b5424108b0289c309fbf00fb11a75f689c78b0289c321f3f00fb11a75f689c68b0289c331cbf00fb11a75f689f101f901c189c85e5f5bc3');
assert.equal(kernel.length,71);assert.equal(hash(kernel),'f66eba0a42762e5d0453755753bd423fc2cfa992e5a890359e8b727266fe9b95');
const leaders=[0,21,31,35,45,49,59],lengths=[21,10,4,10,4,10,12],instructionCounts=[8,4,2,4,2,4,8];
const original=leaders.map((offset,i)=>({pc:0x2000+offset,bytes:kernel.subarray(offset,offset+lengths[i]),instructions:instructionCounts[i]}));
assert.equal(original.reduce((n,b)=>n+b.bytes.length,0),71);assert.equal(original.reduce((n,b)=>n+b.instructions,0),32);
const caller=[
  {pc:0x3000,bytes:Buffer.from('680008400068bffffdff68000001806810500000e8e7efffff','hex'),instructions:5},
  {pc:0x3019,bytes:Buffer.from('83c41089c55068ffff000068000000806810500000e8cdefffff','hex'),instructions:7},
  {pc:0x3033,bytes:Buffer.from('83c41001e8a330500000ebfe','hex'),instructions:4},
];
assert.deepEqual(caller.map(b=>b.pc+b.bytes.length),[0x3019,0x3033,0x303f]);
const arities={guard_detect: 6, guard_detect_resident: 7, finish_exception_detection: 12, open:3,close:0,arena_ptr:0,map:3,protect:3,upload:2,read32:1,store32:2,store_resident32:6,compare_exchange32:3,compare_exchange_resident32:7,
  compile:1,compile_entries:2,compile_resident:1,compile_resident_entries:2,generation:0,module_ptr:0,module_len:0,guard:6,guard_resident:7,discard_unacknowledged_resident:4, cpu_compare_exchange32: 3, cpu_compare_exchange_resident32: 7, cpu_read32: 1, cpu_store32: 2, cpu_store_resident32: 6};
const arena=c=>Buffer.from(new Uint8Array(c.memory.buffer,c.base,SIZE));
const cpu=c=>{const a=arena(c);return{registers:Array.from({length:8},(_,i)=>a.readUInt32LE(16+4*i)),pc:a.readUInt32LE(48),flags:a.readUInt32LE(52)};};
function input(c,offset,bytes){const expected=arena(c);expected.set(bytes,offset);new Uint8Array(c.memory.buffer).set(bytes,c.base+offset);assert.deepEqual(arena(c),expected);}
function host(c,name,args,span){const expected=arena(c);assert.equal(c.api[name](...args),0,name);if(span)expected.set(arena(c).subarray(...span),span[0]);assert.deepEqual(arena(c),expected,'host full4612 '+name);}
function upload(c,address,bytes){input(c,140,bytes);host(c,'upload',[address,bytes.length]);}
function seed(c,pc,registers){assert.equal(c.seeded,false);c.seeded=true;const s={registers:[...registers],pc,flags:0xcd7};input(c,0,Buffer.concat([state(s),exit(1,0),word(0),helper(0xad)]));return s;}
function name(c,kind){if(kind==='cpu_read32')return kind;if(kind==='cpu_store32')return c.owner==='resident'?'cpu_store_resident32':'cpu_store32';assert.equal(kind,'cpu_compare_exchange32');return c.owner==='resident'?'cpu_compare_exchange_resident32':'cpu_compare_exchange32';}
function open(owner,entries){
  const instance=new WebAssembly.Instance(engine.module,{}),c={id:++counts.contexts,owner,entries,memory:instance.exports.memory,api: attachDetectionApi({}, instance.exports),high:0x4f4c4446,captures:0,seeded:false};
  for(const[n,arity]of Object.entries(arities)){c.api[n]=instance.exports['ring3_abi_v1_'+n];assert.equal(c.api[n]?.length,arity,n);}
  assert.equal(c.api.open(12,c.id,c.high),0);c.base=c.api.arena_ptr()>>>0;input(c,0,initial);
  for(const[page,permission]of[[0x2000,7],[0x3000,7],[0x5000,3],[0x7000,3]])host(c,'map',[page,1,permission]);
  upload(c,0x2000,kernel);for(const b of caller)upload(c,b.pc,b.bytes);
  for(const page of[0x2000,0x3000])host(c,'protect',[page,1,5]);return c;
}
function compile(c,title,blocks,kinds){
  assert.ok(++c.captures<=3);assert.ok(blocks.length<=8);const instructions=blocks.reduce((n,b)=>n+b.instructions,0);assert.ok(instructions<=64);
  const beforeInput=arena(c);input(c,140,words(blocks.flatMap(b=>c.entries?[b.pc]:[b.pc,b.bytes.length])));
  const method=c.owner==='resident'?(c.entries?'compile_resident_entries':'compile_resident'):(c.entries?'compile_entries':'compile');
  host(c,method,c.entries?[blocks.length,0]:[blocks.length],c.owner==='resident'?[140,164]:null);
  const a=arena(c),binding=c.owner==='resident'?{low:a.readUInt32LE(148),high:a.readUInt32LE(152),pointer:a.readUInt32LE(156),length:a.readUInt32LE(160)}:{generation:c.api.generation(),pointer:c.api.module_ptr()>>>0,length:c.api.module_len()>>>0};
  if(c.owner==='resident'){assert.equal(a.readUInt32LE(140),1);assert.equal(a.readUInt32LE(144),24);assert.ok(binding.low||binding.high);}
  lifecycle.push({context:c.id,kind:'capture',name:title,method,before_sha256:hash(beforeInput),after_sha256:hash(a),cpu:cpu(c),transfer_hex:a.subarray(140,204).toString('hex'),...binding});
  const bytes=Buffer.from(new Uint8Array(c.memory.buffer,binding.pointer,binding.length)),module=new WebAssembly.Module(bytes),names=[c.owner==='resident'?'guard_detect_resident':'guard_detect',...kinds.map(k=>name(c,k))];
  const imports=generatedImports({resident: names[0] === 'guard_detect_resident', helpers: names.slice(1)});
  assert.ok(bytes.length<=65536);assert.deepEqual(WebAssembly.Module.imports(module),imports);assert.deepEqual(WebAssembly.Module.exports(module),[{name:'run',kind:'function'}]);
  const ring3={...c.api};for(const kind of kinds){const n=name(c,kind);ring3[n]=(...args)=>{const before=arena(c),status=c.api[n](...args),after=arena(c);before.set(after.subarray(100,140),100);assert.deepEqual(after,before,'canonical helper40B only');
    const payload=args.slice(n.includes('resident')?4:0).map(v=>v>>>0);assert.equal(payload.length,kind==='cpu_read32'?1:kind==='cpu_store32'?2:3);
    helperTrace.push({context:c.id,call:counts.runs+1,module:title,name:n,args:payload,status,helper_hex:after.subarray(100,140).toString('hex')});counts.helper_calls++;counts[{cpu_read32:'read32',cpu_store32:'store32',cpu_compare_exchange32:'compare_exchange32'}[kind]]++;return status;};}
  const run=new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3: {...(ring3), finish_exception_detection: c.api.finish_exception_detection}}).exports.run;assert.equal(run.length,4);
  const file=`module-${++counts.modules}.wasm`;writeFileSync(join(output,file),bytes,{flag:'wx'});modules.push({context:c.id,owner:c.owner,entries:c.entries,name:title,file,bytes:bytes.length,sha256:hash(bytes),blocks:blocks.length,instructions,imports,...binding});return{...binding,name:title,run,active:true};
}
function discard(c,unit){
  assert.equal(c.owner,'resident');assert.equal(unit.active,true);const before=arena(c);host(c,'discard_unacknowledged_resident',[c.id,c.high,unit.low,unit.high]);
  assert.deepEqual(arena(c),before);unit.active=false;counts.resident_discards++;lifecycle.push({context:c.id,kind:'discard',name:unit.name,low:unit.low,high:unit.high,before_sha256:hash(before),after_sha256:hash(arena(c)),cpu:cpu(c)});
}
function run(c,unit,budget,s,label,reason,retired,packet){
  assert.equal(unit.active,true);const expected=arena(c),at=helperTrace.length;expected.set(state(s));expected.set(cpuExit(reason,retired),56);expected.set(packet,100);
  const status=unit.run(c.base,c.base+56,budget,c.base+96);assert.equal(status,0,label);assert.deepEqual(arena(c),expected,'full4612 '+label);const a=arena(c);
  observations.push({context:c.id,module:unit.name,label,budget,observed_status:status,observed_reason:a.readUInt32LE(72),observed_retired:a.readUInt32LE(76),observed_cpu:cpu(c),helper_calls:helperTrace.length-at,helper_hex:a.subarray(100,140).toString('hex'),arena_hex:a.toString('hex')});
  counts.runs++;counts.retired+=retired;counts[reason===3?'cold_exits':'budget_exits']++;
}
function checkRam(c,address,expected){for(let i=0;i<expected.length;i+=4){const a=arena(c),value=expected.readUInt32LE(i);assert.equal(c.api.read32(address+i),0);a.set(helper(value),100);assert.deepEqual(arena(c),a,'diagnostic full4612');diagnostics.push({context:c.id,address:address+i,value});}}
function next(s,pc,flags,updates={}){const n={registers:[...s.registers],pc,flags};for(const[r,v]of Object.entries(updates))n.registers[Number(r)]=v>>>0;return n;}
function leafEvents(r,flip,keep,set,old,ret){const afterOr=(old|set)>>>0,afterAnd=(afterOr&keep)>>>0,afterXor=(afterAnd^flip)>>>0;return[
  ['cpu_store32',[0x7068,r[3]],0],['cpu_store32',[0x7064,r[7]],0],['cpu_store32',[0x7060,r[6]],0],
  ['cpu_read32',[0x707c],flip],['cpu_read32',[0x7078],keep],['cpu_read32',[0x7074],set],['cpu_read32',[0x7070],DATA],
  ['cpu_read32',[DATA],old],['cpu_compare_exchange32',[DATA,old,afterOr],old],
  ['cpu_read32',[DATA],afterOr],['cpu_compare_exchange32',[DATA,afterOr,afterAnd],afterOr],
  ['cpu_read32',[DATA],afterAnd],['cpu_compare_exchange32',[DATA,afterAnd,afterXor],afterAnd],
  ['cpu_read32',[0x7060],r[6]],['cpu_read32',[0x7064],r[7]],['cpu_read32',[0x7068],r[3]],['cpu_read32',[0x706c],ret],
];}
function proof(c){
  const data=Buffer.from(Array.from({length:64},(_,i)=>(i*13+7)&255));data.writeUInt32LE(0x00020050,16);data.writeUInt32LE(0,48);upload(c,0x5000,data);
  const stack=Buffer.from(Array.from({length:64},(_,i)=>(i*17+23)&255));upload(c,0x7050,stack);
  const r=parents();r[4]=0x7080;const first=compile(c,'A-first-call-cold-return',[...original,caller[0]],['cpu_read32','cpu_store32','cpu_compare_exchange32']);let s=seed(c,0x3000,r);const at=helperTrace.length;
  s=next(s,0x3019,0xc03,{0:0x000600b0,1:0x000600b0,2:DATA,4:0x7070});run(c,first,38,s,'first-whole-original-natural-RET-cold-boundary',3,37,cpuHelper(0x3019));counts.compiler_returns++;
  stack.set(words([r[6],r[7],r[3],0x3019,DATA,0x80010000,0xfffdffbf,0x00400800]),16);data.writeUInt32LE(0x80410810,16);checkRam(c,0x5000,data);checkRam(c,0x7050,stack);
  for(const i of[3,5,6,7])assert.equal(cpu(c).registers[i],r[i]);
  const secondCaller=compile(c,'B-live-return-derived-mask-cold-call',[caller[1]],['cpu_store32']);if(c.owner==='replacement')first.active=false;
  s=next(s,0x2000,0x402,{4:0x706c,5:0x000600b0});run(c,secondCaller,8,s,'guest-first-return-becomes-second-flip-before-cold-CALL',3,7,cpuHelper(0));
  stack.set(words([0x3033,DATA,0x80000000,0x0000ffff,0x000600b0]),28);checkRam(c,0x5000,data);checkRam(c,0x7050,stack);
  if(c.owner==='resident')discard(c,first);
  const second=compile(c,'C-second-call-and-guest-consumer',[...original,caller[2]],['cpu_read32','cpu_store32','cpu_compare_exchange32']);if(c.owner==='replacement')secondCaller.active=false;
  s=next(s,0x3033,0x406,{0:0x00821830,1:0x00821830,4:0x7070});run(c,second,32,s,'second-whole-original-natural-RET-with-guest-derived-flip',1,32,cpuHelper(0x3033));counts.compiler_returns++;
  data.writeUInt32LE(0x000608a0,16);checkRam(c,0x5000,data);checkRam(c,0x7050,stack);for(const i of[3,6,7])assert.equal(cpu(c).registers[i],r[i]);assert.equal(cpu(c).registers[5],0x000600b0);
  s=next(s,0x303d,0x402,{0:0x008818e0,4:0x7080});run(c,second,4,s,'guest-sums-and-stores-both-real-old-value-checksums',1,4,cpuHelper(0));data.writeUInt32LE(0x008818e0,48);checkRam(c,0x5000,data);checkRam(c,0x7050,stack);
  const expected=[
    ['cpu_store32',[0x707c,0x00400800],0],['cpu_store32',[0x7078,0xfffdffbf],0],['cpu_store32',[0x7074,0x80010000],0],['cpu_store32',[0x7070,DATA],0],['cpu_store32',[0x706c,0x3019],0],
    ...leafEvents(r,0x00400800,0xfffdffbf,0x80010000,0x00020050,0x3019),
    ['cpu_store32',[0x707c,0x000600b0],0],['cpu_store32',[0x7078,0x0000ffff],0],['cpu_store32',[0x7074,0x80000000],0],['cpu_store32',[0x7070,DATA],0],['cpu_store32',[0x706c,0x3033],0],
    ...leafEvents(r,0x000600b0,0x0000ffff,0x80000000,0x80410810,0x3033),['cpu_store32',[0x5030,0x008818e0],0],
  ];
  const events=helperTrace.slice(at);assert.equal(events.length,45);for(const[i,[kind,args,old]]of expected.entries()){const e=events[i];assert.equal(e.name,name(c,kind));assert.deepEqual(e.args,args);assert.equal(e.status,0);assert.equal(e.helper_hex,cpuHelper(old).toString('hex'));}
  assert.equal(c.captures,3);writeFileSync(join(output,`context-${c.id}-expected-helper-chronology.json`),JSON.stringify(expected,null,2)+'\n',{flag:'wx'});
}
for(const owner of['replacement','resident'])for(const entries of[false,true]){
  const c=open(owner,entries);proof(c);writeFileSync(join(output,`context-${c.id}-final-arena.bin`),arena(c),{flag:'wx'});contexts.push({id:c.id,owner,entries,captures:c.captures,maximum_mapped_pages:4});host(c,'close',[]);
}
assert.deepEqual(sourceHashes(),sourcesBefore);
assert.deepEqual(counts,{contexts:4,modules:12,runs:16,retired:320,helper_calls:180,read32:88,store32:68,compare_exchange32:24,compiler_returns:8,cold_exits:8,budget_exits:8,resident_discards:2});assert.equal(diagnostics.length,512);
const result={engine:{path:enginePath,sha256:engine.sha256,bytes:engine.bytes.length},source_pins:sourcesBefore,initial_arena_sha256:hash(initial),counts,contexts,modules,observations,helper_trace:helperTrace,public_diagnostics:diagnostics,lifecycle,
  compiler:{text_hex:kernel.toString('hex'),text_sha256:hash(kernel),original_blocks:7,original_instructions:32,leaders,block_lengths:lengths,calls:8},
  limits:{blocks:8,instructions:64,module_bytes:65536,captures_per_context:6,mapped_pages:12},
  verification_ceiling:'per-run/helper/host full4612 asserted;16 postrun4364 snapshots and4final arenas persisted;selected64Bdata+64Bstack only,no wholeRAM. Existing unchanged canonical CAS fault/MAX/protocol/SMC proof reused;no new general matrix.',
  claim:'supported whole original71B returned-old-value C11 consumer, three staged captures and coldRET/CALL continuation under private flat32 synchronous unshared memory;first-try CAS only,no forcedretry/contention,hardwareLOCK,C11ordering,performance or ISAextension claim'};
assert.deepEqual(Object.keys(result.engine),['path','sha256','bytes']);writeFileSync(join(output,'result.json'),JSON.stringify(result,null,2)+'\n',{flag:'wx'});console.log(JSON.stringify({output,counts,result:join(output,'result.json')}));
