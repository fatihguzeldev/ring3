import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../support/engine.mjs';

const [enginePath, output, root] = process.argv.slice(2), SIZE = 4612, DATA = 0x5010, FAULT = 0x1705;
assert.ok(enginePath && output && root);
const engine = readEngine(enginePath), initial = readFileSync(join(output, 'initial-arena.bin'));
const hash = b => createHash('sha256').update(b).digest('hex');
assert.equal(initial.length, SIZE);
assert.ok(engine.bytes instanceof Buffer && engine.module instanceof WebAssembly.Module && engine.bytes.length > 0);
assert.equal(engine.sha256, hash(engine.bytes));
const sourcePaths = ['engine/src/memory/space.rs','engine/src/abi/memory_helper.rs','engine/src/abi/arena.rs',
  'engine/src/abi/wasm/exports.rs','engine/src/process/instance.rs','engine/src/process/wasm.rs','engine/src/process/resident.rs',
  'engine/src/cpu/x86/ir.rs','engine/src/cpu/x86/decode/profile.rs','engine/src/cpu/x86/decode/lower.rs','engine/src/cpu/x86/decode/integer.rs',
  'engine/src/cpu/dbt/region.rs','engine/src/cpu/dbt/wasm/emitter.rs','engine/src/cpu/dbt/wasm/integer.rs',
  'engine/src/cpu/dbt/wasm/memory.rs','engine/src/cpu/dbt/wasm/memory/exchange.rs','engine/src/cpu/dbt/wasm/locals.rs',
  'engine/tests/engine_atomic_or_u32_helpers.rs','engine/tests/cpu_locked_or32.rs','engine/tests/locked_or32_original.rs','engine/tests/cpu_locked_or32_wasm.rs',
  ...['kernel.c','build.py','PROVENANCE.md','kernel.obj','kernel.text.bin','kernel.disasm.txt','receipt.json','run.mjs'].map(p=>'engine/tests/fixtures/p2-locked-or32/'+p),
  'engine/tests/fixtures/p2-compiled-records/build.py','engine/tests/fixtures/support/engine.mjs'];
const sourceHashes = () => Object.fromEntries(sourcePaths.map(p=>[p,hash(readFileSync(join(root,p)))]));
const sourcesBefore=sourceHashes(),contexts=[],modules=[],observations=[],helperTrace=[],diagnostics=[],controls=[];
const counts={contexts:0,modules:0,runs:0,retired:0,atomic_ors:0,helper_calls:0,read32:0,store32:0,
  masks:0,flag_rows:0,aliases:0,boundary:0,faults:0,repair_only:0,current_inputs:0,synthetic:0,smc:0,compiler_calls:0};
const word=n=>{const b=Buffer.alloc(4);b.writeUInt32LE(n>>>0);return b;}, words=v=>Buffer.concat(v.map(word));
function record(magic,version,size,fields){const b=Buffer.alloc(size);b.write(magic);b.writeUInt16LE(version,4);b.writeUInt16LE(1,6);b.writeUInt32LE(size,8);fields.forEach((v,i)=>b.writeUInt32LE(v>>>0,16+i*4));return b;}
const state=s=>record('R3ST',1,56,[...s.registers,s.pc,s.flags]);
const exit=(reason,retired,{detail=0,address=0,access=0}={},version=2)=>record('R3EX',version,40,[reason,retired,detail,address,access,access?4:0]);
const helper=(value=0,{detail=0,address=0,access=0}={})=>record('R3MH',1,40,[access?1:0,value,detail,address,access,access?4:0]);
const helper32=value=>helper(value);
const parents=()=>Array.from({length:8},(_,i)=>(0xa100+i)*65536+0x1234+i);
const mod32=n=>Number((n%4294967296n+4294967296n)%4294967296n);
const normalized=byte=>byte<128?byte:(byte|0xffffff00)>>>0;
const orWord=(old,mask)=>Number(BigInt(old)|BigInt(mask));
const parity=value=>{let n=value&255,ones=0;while(n){ones+=n&1;n>>>=1;}return ones%2===0?4:0;};
const flags=(incoming,value)=>(incoming&0x400)|2|parity(value)|(value===0?0x40:0)|(value>=0x80000000?0x80:0);
assert.equal(normalized(0x80),0xffffff80);assert.equal(normalized(0xff),0xffffffff);
assert.equal(flags(0xcd7,0),0x446);assert.equal(flags(0xcd7,0xffffff80),0x482);assert.equal(flags(0xcd7,0xffffffff),0x486);
const kernel=readFileSync(join(root,'engine/tests/fixtures/p2-locked-or32/kernel.text.bin'));
assert.equal(kernel.toString('hex'),'8b442404f0830820c3');assert.equal(hash(kernel),'95189adf43d025a695bb470bd6a0278fc4357841963731fd72129e7d00709976');
const normal=[0,0x20,0x7f,0x80,0xff,1].map((encoded,i)=>({pc:0x1000+i*16,bytes:Buffer.from([0xf0,0x83,0x0d,0x10,0x50,0,0,encoded,0xeb,0]),instructions:2,encoded,mask:normalized(encoded),next:0x1008+i*16}));
const aliasRows=[
  {pc:0x1400,bytes:Buffer.from([0xf0,0x83,0x08,0x20]),base:0,set:{0:0x5010},mask:0x20,address:0x5010},
  {pc:0x1404,bytes:Buffer.from([0xf0,0x83,0x0c,0x24,0x80]),base:4,set:{4:0x5011},mask:0xffffff80,address:0x5011},
  {pc:0x1409,bytes:Buffer.from([0xf0,0x83,0x4d,0,0xff]),base:5,displacement:0,set:{5:0x5012},mask:0xffffffff,address:0x5012},
  {pc:0x140e,bytes:Buffer.from([0xf0,0x83,0x4c,0x8e,0xf0,0x7f]),base:6,index:1,scale:4,displacement:-16,set:{6:0x5000,1:8},mask:0x7f,address:DATA},
  {pc:0x1414,bytes:Buffer.from([0xf0,0x83,0x4c,0x9b,1,1]),base:3,index:3,scale:4,displacement:1,set:{3:0x1003},mask:1,address:DATA},
].map(r=>({...r,next:r.pc+r.bytes.length}));
const aliases=[{pc:0x1400,bytes:Buffer.concat([...aliasRows.map(r=>r.bytes),Buffer.from([0xeb,0])]),instructions:6}];
function addressOf(row,r){return mod32(BigInt(row.displacement??0)+(row.base===undefined?0n:BigInt(r[row.base]))+(row.index===undefined?0n:BigInt(r[row.index])*BigInt(row.scale??1)));}
const faultBlock={pc:0x1700,bytes:Buffer.from([0xbb,0xbe,0xba,0xfe,0xca,0xf0,0x83,0x0e,0x20,0xeb,0]),instructions:3};
const zeroFaultBlock={pc:0x1720,bytes:Buffer.from([0xbb,0xbe,0xba,0xfe,0xca,0xf0,0x83,0x0e,0,0xeb,0]),instructions:3};
const faultRow={pc:FAULT,mask:0x20,next:FAULT+4},zeroRow={pc:0x1725,mask:0,next:0x1729};
const live={pc:0x1800,bytes:Buffer.from([0xf0,0x83,0x0e,0x80,0x0f,0x98,0xc3,0x0f,0x94,0xc7,0x0f,0x9a,0xc2,0x83,0xd0,0,0xeb,0]),instructions:6};
const caller=[
  {pc:0x3000,bytes:Buffer.from('6810500000e8f6efffff','hex'),instructions:2},
  {pc:0x300a,bytes:Buffer.from('83c4048b2d105000006810500000e8e3efffff','hex'),instructions:4},
  {pc:0x301d,bytes:Buffer.from('83c404a11050000001e8a330500000ebfe','hex'),instructions:5},
];
const kernelBlocks=[{pc:0x2000,bytes:kernel,instructions:3}];
assert.deepEqual(caller.map(b=>b.pc+b.bytes.length),[0x300a,0x301d,0x302e]);
const smcBytes=identity=>Buffer.concat([Buffer.from([0xf0,0x83,0x0e,0x20,0xba]),word(identity?0x1234567a:0x1234565a),Buffer.from([0xeb,0])]);
const arena = c => Buffer.from(new Uint8Array(c.memory.buffer, c.base, SIZE));
const cpu = c => {const b = arena(c); return {registers: Array.from({length: 8}, (_, i) => b.readUInt32LE(16 + 4 * i)), pc: b.readUInt32LE(48), flags: b.readUInt32LE(52)};};
function input(c, offset, bytes) {const expected = arena(c); expected.set(bytes, offset); new Uint8Array(c.memory.buffer).set(bytes, c.base + offset); assert.deepEqual(arena(c), expected);}
function host(c, name, args, span) {const expected = arena(c); assert.equal(c.api[name](...args), 0, name); if (span) expected.set(arena(c).subarray(...span), span[0]); assert.deepEqual(arena(c), expected, 'host full4612 ' + name);}
function map(c, page, permission = 3) {host(c, 'map', [page, 1, permission]); c.pages.add(page); c.maxPages = Math.max(c.maxPages, c.pages.size); assert.ok(c.maxPages <= 12);}
function protect(c, page, permission) {host(c, 'protect', [page, 1, permission]);}
function unmap(c, page) {host(c, 'unmap', [page, 1]); c.pages.delete(page);}
function upload(c, address, bytes) {assert.ok(address >= 0 && address + bytes.length <= 4294967296); input(c, 140, bytes); host(c, 'upload', [address, bytes.length]);}
function seed(c, pc, flags = 0xcd7, registers = parents()) {input(c, 0, Buffer.concat([state({registers, pc, flags}), exit(1, 0), word(0), helper(0xad)])); return cpu(c);}
const arities = {open: 3, close: 0, arena_ptr: 0, map: 3, protect: 3, unmap: 2, upload: 2, read32: 1, store32: 2, store_resident32: 6, atomic_or_u32: 2, atomic_or_resident_u32: 6, compile: 1, compile_entries: 2,
  compile_resident: 1, compile_resident_entries: 2, generation: 0, module_ptr: 0, module_len: 0, guard: 6, guard_resident: 7};

function open(owner, entries, purpose) {
  const instance = new WebAssembly.Instance(engine.module, {}), c = {id: ++counts.contexts, owner, entries, purpose, memory: instance.exports.memory, api: {}, high: 0x4f523332, captures: 0, pages: new Set(), maxPages: 0};
  for (const [name, arity] of Object.entries(arities)) {c.api[name] = instance.exports['ring3_abi_v1_' + name]; assert.equal(c.api[name]?.length, arity, name);}
  assert.equal(c.api.open(12, c.id, c.high), 0); c.base = c.api.arena_ptr() >>> 0; input(c, 0, initial);
  for (const [page, permission] of [[0, 3], [0x1000, 7], [0x2000, 7], [0x3000, 7], [0x5000, 3], [0x6000, 3], [0x7000, 3], [0xfffff000, 3]]) map(c, page, permission);
  for (const block of [...normal, ...aliases, faultBlock, zeroFaultBlock, live, ...caller]) upload(c, block.pc, block.bytes);
  upload(c, 0x2000, kernel); protect(c, 0x2000, 5); protect(c, 0x3000, 5);
  if (purpose.startsWith('smc')) {map(c, 0xc000, 7); upload(c, 0xc000, smcBytes(purpose === 'smc-identity'));}
  return c;
}
function compile(c, name, blocks, helpers = ['or']) {
  const instructionCount = blocks.reduce((n, b) => n + b.instructions, 0);
  assert.ok(blocks.length > 0 && blocks.length <= 8 && instructionCount <= 64); assert.ok(++c.captures <= 6);
  input(c, 140, words(blocks.flatMap(b => c.entries ? [b.pc] : [b.pc, b.bytes.length])));
  const method = c.owner === 'resident' ? (c.entries ? 'compile_resident_entries' : 'compile_resident') : (c.entries ? 'compile_entries' : 'compile');
  host(c, method, c.entries ? [blocks.length, 0] : [blocks.length], c.owner === 'resident' ? [140, 164] : null);
  const b = arena(c), binding = c.owner === 'resident' ? {low: b.readUInt32LE(148), high: b.readUInt32LE(152), pointer: b.readUInt32LE(156), length: b.readUInt32LE(160)} : {generation: c.api.generation(), pointer: c.api.module_ptr() >>> 0, length: c.api.module_len() >>> 0};
  const bytes = Buffer.from(new Uint8Array(c.memory.buffer, binding.pointer, binding.length)), module = new WebAssembly.Module(bytes), guard = c.owner === 'resident' ? 'guard_resident' : 'guard';
  const names = helpers.map(n => n === 'or' ? (c.owner === 'resident' ? 'atomic_or_resident_u32' : 'atomic_or_u32') : n === 'store' ? (c.owner === 'resident' ? 'store_resident32' : 'store32') : n);
  const imports = [{module: 'env', name: 'memory', kind: 'memory'}, ...[guard, ...names].map(name => ({module: 'ring3', name, kind: 'function'}))];
  assert.ok(bytes.length <= 65536); assert.deepEqual(WebAssembly.Module.imports(module), imports); assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  const ring3 = {...c.api};
  for (const name of names) ring3[name] = (...args) => {
    const before = arena(c), status = c.api[name](...args), after = arena(c), expected = Buffer.from(before); expected.set(after.subarray(100, 140), 100);
    assert.deepEqual(after, expected, 'canonical helper changes only its 40-byte record');
    const payload = args.slice(name.includes('resident') ? 4 : 0).map(v => v >>> 0);
    helperTrace.push({context: c.id, call: counts.runs + 1, name, address: payload[0], input_value: name.startsWith('read') ? null : payload.at(-1), status, helper_hex: after.subarray(100, 140).toString('hex')});
    counts.helper_calls++; counts[name.startsWith('atomic_or') ? 'atomic_ors' : name.startsWith('read') ? name : 'store32']++; return status;
  };
  const run = new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3}).exports.run; assert.equal(run.length, 4);
  const file = `module-${++counts.modules}.wasm`; writeFileSync(join(output, file), bytes, {flag: 'wx'});
  const unit = {...binding, name, module, ring3, run, exitVersion: names.length ? 2 : 1};
  modules.push({context: c.id, owner: c.owner, entries: c.entries, name, file, bytes: bytes.length, sha256: hash(bytes), blocks: blocks.length, instructions: instructionCount, imports, ...binding}); return unit;
}
function run(c, unit, budget, next, label, {status = 0, reason = 1, retired = 1, packet, fault = {}, synthetic = false} = {}) {
  const before = arena(c), expected = Buffer.from(before), at = helperTrace.length;
  if (status === 0) {expected.set(state(next)); expected.set(exit(reason, retired, fault, unit.exitVersion), 56); if (packet) expected.set(packet, 100);}
  const observed = unit.run(c.base, c.base + 56, budget, c.base + 96); assert.equal(observed, status, label); assert.deepEqual(arena(c), expected, 'full4612 ' + label);
  const after = arena(c); observations.push({context: c.id, module: unit.name, label, budget, synthetic, expected_status: status, observed_status: observed,
    expected_reason: status ? null : reason, observed_reason: status ? null : after.readUInt32LE(72), expected_retired: status ? null : retired, observed_retired: status ? null : after.readUInt32LE(76),
    expected_pc: status ? null : next.pc, observed_pc: status ? null : after.readUInt32LE(48), expected_flags: status ? null : next.flags, observed_flags: status ? null : after.readUInt32LE(52),
    expected_cpu: status ? null : next, observed_cpu: status ? null : cpu(c), helper_calls: helperTrace.length - at, helper_hex: after.subarray(100, 140).toString('hex')});
  counts.runs++; if (!status) counts.retired += retired;
}
function checkRam(c, address, expected) {
  assert.ok(address + expected.length <= 4294967296 && expected.length % 4 === 0);
  for (let i = 0; i < expected.length; i += 4) {const before = arena(c), value = expected.readUInt32LE(i); assert.equal(c.api.read32(address + i), 0); before.set(helper32(value), 100); assert.deepEqual(arena(c), before, 'diagnostic selectedRAM'); diagnostics.push({context: c.id, address: address + i, value});}
}
function frame(c, address, old) {const start = Math.max(0, Math.min(address - 12, 4294967296 - 32)), bytes = Buffer.from(Array.from({length: 32}, (_, i) => (i * 17 + 23) & 255)); bytes.writeUInt32LE(old >>> 0, address - start); upload(c, start, bytes); return {address: start, bytes};}
function changedFrame(f, address, value) {const bytes = Buffer.from(f.bytes); bytes.writeUInt32LE(value >>> 0, address - f.address); return {...f, bytes};}

function changed(start,old,mask,next){return {...start,registers:[...start.registers],pc:next,flags:flags(start.flags,orWord(old,mask))};}
function orCase(c,unit,row,address,old,registers,incoming,label,f=frame(c,address,old)){
  const start=seed(c,row.pc,incoming,registers);
  run(c,unit,1,changed(start,old,row.mask,row.next),label,{packet:helper(old)});
  const value=orWord(old,row.mask);checkRam(c,f.address,changedFrame(f,address,value).bytes);
  assert.deepEqual(cpu(c).registers,registers,'LOCK OR changes no GPR');return {frame:changedFrame(f,address,value),value};
}
function stopped(c,unit,registers,incoming,fault,label,prior=true,row=faultRow){
  const start=prior?seed(c,row.pc-5,incoming,registers):cpu(c),next={...start,registers:[...start.registers],pc:row.pc};
  if(prior)next.registers[3]=0xcafebabe;
  run(c,unit,2,next,label,{reason:5,retired:prior?1:0,fault,packet:helper(0,fault)});counts.faults++;return next;
}
function guardProbe(c,unit,kind,status,{badKey=false,badId=false,badPointer=false}={}){
  const before=arena(c),p=[badPointer?0:c.base,c.base+56,c.base+96],low=badKey?c.id+1:c.id;
  const args=c.owner==='resident'?[low,c.high,badId?0:unit.low,badId?0:unit.high,...p]:[low,c.high,badId?0:unit.generation,...p];
  const observed=c.api[c.owner==='resident'?'guard_resident':'guard'](...args);
  assert.equal(observed,status,kind);assert.deepEqual(arena(c),before);controls.push({context:c.id,kind,status:observed});
}
function finish(c,unit){
  writeFileSync(join(output,`context-${c.id}-final-arena.bin`),arena(c),{flag:'wx'});
  contexts.push({id:c.id,owner:c.owner,entries:c.entries,purpose:c.purpose,captures:c.captures,maximum_mapped_pages:c.maxPages});
  host(c,'close',[]);run(c,unit,1,cpu(c),'closed-before-owner-or-pointers',{status:5});
}
function core(owner,entries){
  const c=open(owner,entries,'core'),a=compile(c,'signextended-immediate-masks',normal);
  for(const row of normal)for(const old of [0,0x1234565a,0xffffffff,0x80000000]){
    orCase(c,a,row,DATA,old,parents(),0xcd7,'full-signextended-mask');counts.masks++;
  }
  const flagInputs=[2,3,0x42,0x43,0x402,0x403,0x8d6,0xcd7];
  for(const [i,incoming] of flagInputs.entries())for(const old of [0,0x80000000]){
    orCase(c,a,normal[i%normal.length],DATA,old,parents(),incoming,'logical-result-flags');counts.flag_rows++;
  }
  const b=compile(c,'ea-aliases-boundary-fault',[...aliases,faultBlock,zeroFaultBlock]);
  for(const row of aliasRows)for(const old of [0x1234565a,0xffffffff]){
    const r=parents();for(const [i,v] of Object.entries(row.set))r[Number(i)]=v;
    assert.equal(addressOf(row,r),row.address);orCase(c,b,row,row.address,old,r,0xcd7,'original-ea-aliases');counts.aliases++;
  }
  for(const address of [0,1,0x5ffd,0x5fff,0xfffffffc])for(const old of [0x1234565a,0xffffffff]){
    const r=parents();r[6]=address;orCase(c,b,faultRow,address,old,r,0x402,'dword-boundary');counts.boundary++;
  }
  const r=parents();r[6]=0x9008;
  for(const [budget,cancel,reason] of [[0,0,1],[1,1,2],[0,1,2]]){
    const start=seed(c,FAULT,0xcd7,r);input(c,96,word(cancel));run(c,b,budget,start,'helper-free-budget-cancel',{reason,retired:0});controls.push({context:c.id,kind:'budget-cancel',budget,cancel});
  }
  input(c,96,word(0));let s=seed(c,0x1900,0x402);run(c,b,2,s,'cold-pc',{reason:3,retired:0});controls.push({context:c.id,kind:'cold-pc'});
  for(const [kind,status,options] of [['current',0,{}],['wrong-key-before-pointers',3,{badKey:true,badPointer:true}],['wrong-unit-before-pointers',3,{badId:true,badPointer:true}],['current-malformed-pointer',1,{badPointer:true}]])guardProbe(c,b,kind,status,options);
  const cases=[
    {name:'read-unmapped',address:0x9010,page:0x9000,permission:null,access:1,detail:1,at:0x9010},
    {name:'read-write-only',address:0x9010,page:0x9000,permission:2,access:1,detail:2,at:0x9010},
    {name:'contained-read-only',address:0x9010,page:0x9000,permission:1,access:2,detail:2,at:0x9010},
    {name:'mask-zero-read-only',address:0x9010,page:0x9000,permission:1,access:2,detail:2,at:0x9010,row:zeroRow},
    {name:'cross-second-unmapped',address:0x5fff,page:0x6000,permission:null,access:1,detail:1,at:0x6000},
    {name:'cross-first-read-only',address:0x5fff,page:0x5000,permission:1,access:2,detail:2,at:0x5fff},
    {name:'read-overflow',address:0xfffffffd,access:1,detail:3,at:0xfffffffd},
  ];
  for(const item of cases){
    const row=item.row??faultRow;let f;
    if(item.name==='read-overflow'){upload(c,0xffffffe0,Buffer.alloc(32,0x5a));upload(c,0,Buffer.alloc(32,0xa5));}
    else {if(!c.pages.has(item.page))map(c,item.page);f=frame(c,item.address,0x1234567a);if(item.permission===null)unmap(c,item.page);else protect(c,item.page,item.permission);}
    const registers=parents();registers[6]=item.address;const fault={detail:item.detail,address:item.at,access:item.access};
    stopped(c,b,registers,0xcd7,fault,item.name,true,row);if(item.access===2)checkRam(c,f.address,f.bytes);
    stopped(c,b,registers,0xcd7,fault,item.name+'-unchanged',false,row);if(item.access===2)checkRam(c,f.address,f.bytes);
    if(item.name==='read-overflow'){checkRam(c,0xffffffe0,Buffer.alloc(32,0x5a));checkRam(c,0,Buffer.alloc(32,0xa5));continue;}
    if(item.permission===null){map(c,item.page);const bytes=Buffer.from(f.bytes);for(let i=0;i<bytes.length;i++)if(f.address+i>=item.page&&f.address+i<item.page+4096)bytes[i]=0;f={...f,bytes};}
    else protect(c,item.page,3);
    checkRam(c,f.address,f.bytes);s=cpu(c);const old=f.bytes.readUInt32LE(item.address-f.address);
    run(c,b,1,changed(s,old,row.mask,row.next),item.name+'-repair-only',{packet:helper(old)});counts.repair_only++;
    checkRam(c,f.address,changedFrame(f,item.address,orWord(old,row.mask)).bytes);if(item.page===0x9000)unmap(c,0x9000);
  }
  let f=frame(c,0x5fff,0x1234567a);protect(c,0x5000,1);protect(c,0x6000,2);
  const split=parents();split[6]=0x5fff;
  const readFault={detail:2,address:0x6000,access:1},writeFault={detail:2,address:0x5fff,access:2};
  stopped(c,b,split,0x402,readFault,'later-read-before-earlier-write');stopped(c,b,split,0x402,readFault,'later-read-repeat',false);
  protect(c,0x6000,1);checkRam(c,f.address,f.bytes);stopped(c,b,split,0x402,writeFault,'read-repair-exposes-write',false);checkRam(c,f.address,f.bytes);
  stopped(c,b,split,0x402,writeFault,'write-repeat',false);checkRam(c,f.address,f.bytes);protect(c,0x5000,3);protect(c,0x6000,3);s=cpu(c);
  run(c,b,1,changed(s,0x1234567a,0x20,FAULT+4),'both-accesses-repair-only',{packet:helper(0x1234567a)});counts.repair_only++;checkRam(c,f.address,f.bytes);
  f=frame(c,DATA,0x1234567a);protect(c,0x5000,1);const current=parents();current[6]=DATA;
  const currentFault={detail:2,address:DATA,access:2};stopped(c,b,current,0xcd7,currentFault,'current-input-stopped');checkRam(c,f.address,f.bytes);
  stopped(c,b,current,0xcd7,currentFault,'current-input-unchanged',false);checkRam(c,f.address,f.bytes);protect(c,0x5000,3);
  const fresh=frame(c,DATA+64,0xf0000000);input(c,16,word(0xf1e2d3c4));input(c,40,word(DATA+64));input(c,52,word(0x402));s=cpu(c);
  run(c,b,1,changed(s,0xf0000000,0x20,FAULT+4),'explicit-current-ea-flags-ram',{packet:helper(0xf0000000)});counts.current_inputs++;
  checkRam(c,f.address,f.bytes);checkRam(c,fresh.address,changedFrame(fresh,DATA+64,0xf0000020).bytes);
  synthetic(c,b);
  const retained=frame(c,DATA,0x1234567a),rr=parents();s=seed(c,normal[1].pc,0x402,rr);
  if(owner==='replacement')run(c,a,1,s,'old-generation-neutral',{status:3});
  else {run(c,a,1,changed(s,0x1234567a,0x20,normal[1].next),'retained-current-owner',{packet:helper(0x1234567a)});checkRam(c,retained.address,retained.bytes);}
  const l=compile(c,'live-logical-flags-consumer',[live]);liveProof(c,l);
  upload(c,normal[0].pc,normal[0].bytes.subarray(0,1));s=seed(c,live.pc,0x402);run(c,l,1,s,'same-byte-source-invalidated',{status:4});guardProbe(c,l,'stale-before-pointers',4,{badPointer:true});
  const compiled=compile(c,'original-two-live-void-calls',[...kernelBlocks,...caller],['read32','store','or']);compilerProof(c,compiled);finish(c,compiled);
}
function synthetic(c,unit){
  const infra=(value=0,detail=1)=>record('R3MH',1,40,[2,value,detail,0,0,0]);
  const rows=[
    {name:'magic',edit:b=>b.write('BAD!')},
    {name:'fault-value',packet:helper(1,{detail:2,address:DATA,access:1})},
    {name:'invalidated-fault',status:11,packet:helper(0,{detail:2,address:DATA,access:1})},
    {name:'invalidated-infrastructure',status:11,packet:infra()},
    {name:'write-unmapped',packet:helper(0,{detail:1,address:DATA,access:2})},
    {name:'write-overflow',address:0xfffffffd,packet:helper(0,{detail:3,address:0xfffffffd,access:2})},
    {name:'top-nonoverflow-fault',address:0xfffffffc,packet:helper(0,{detail:2,address:0xfffffffd,access:1})},
    {name:'overflow-success',address:0xfffffffd},
    {name:'infrastructure-value',packet:infra(1)},
    {name:'canonical-version-exhaustion',packet:infra(),detail:1},
    {name:'canonical-generic-infrastructure',packet:infra(0,2),detail:3},
    {name:'unknown-status',status:99,detail:3},
  ];
  assert.equal(rows.length,12);
  for(const row of rows){
    const address=row.address??DATA,r=parents();r[6]=address;
    const f=frame(c,Math.min(address,0xfffffffc),0x1234567a),zero=address>0xfffffffc?frame(c,0,0x33445566):null;
    const packet=Buffer.from(row.packet??helper(0x1234567a));row.edit?.(packet);let calls=0;
    const ring3={...unit.ring3},name=c.owner==='resident'?'atomic_or_resident_u32':'atomic_or_u32';
    ring3[name]=(...args)=>{assert.deepEqual(args.slice(name.includes('resident')?4:0).map(v=>v>>>0),[address,0x20]);input(c,100,packet);calls++;return row.status??0;};
    const fake={...unit,name:unit.name+'-synthetic',run:new WebAssembly.Instance(unit.module,{env:{memory:c.memory},ring3}).exports.run},start=seed(c,FAULT,0xcd7,r);
    run(c,fake,1,start,'nonforwarding-'+row.name,{reason:7,retired:0,fault:{detail:row.detail??2},packet,synthetic:true});
    assert.equal(calls,1);checkRam(c,f.address,f.bytes);if(zero)checkRam(c,zero.address,zero.bytes);counts.synthetic++;
  }
}
function liveProof(c,unit){
  const f=frame(c,DATA,0),r=parents();r[0]=0x7fffffff;r[6]=DATA;
  let s=seed(c,live.pc,0xcd7,r);s=changed(s,0,0xffffff80,0x1804);assert.equal(s.flags,0x482);
  run(c,unit,1,s,'live-signextended-mask-result-flags',{packet:helper(0)});
  s={...s,registers:[...s.registers],pc:0x1807};s.registers[3]=s.registers[3]-s.registers[3]%256+1;
  run(c,unit,1,s,'live-sets-consumes-sign');
  s={...s,registers:[...s.registers],pc:0x180a};s.registers[3]=s.registers[3]-Math.floor(s.registers[3]/256)%256*256;
  run(c,unit,1,s,'live-setz-consumes-nonzero');
  s={...s,registers:[...s.registers],pc:0x180d};s.registers[2]=s.registers[2]-s.registers[2]%256;
  run(c,unit,1,s,'live-setp-consumes-odd-parity');
  s={...s,pc:0x1810,flags:0x406};run(c,unit,1,s,'live-adc-consumes-cleared-carry');s={...s,pc:0x1812};
  run(c,unit,2,s,'live-cold-successor',{reason:3});checkRam(c,f.address,changedFrame(f,DATA,0xffffff80).bytes);
}
function compilerProof(c,unit){
  const data=Buffer.from(Array.from({length:64},(_,i)=>(i*17+23)&255));data.writeUInt32LE(0xa5a55a5a,16);upload(c,0x5000,data);
  const stack=frame(c,0x706c,0x11223344),r=parents();r[4]=0x7080;
  let s=seed(c,0x3000,0xcd7,r),at=helperTrace.length;const snapshots=[{label:'entry',cpu:s,arena_hex:arena(c).toString('hex')}];
  s={...s,registers:[...s.registers],pc:0x2000};s.registers[4]=0x7078;
  run(c,unit,2,s,'compiler-first-cdecl-call',{retired:2,packet:helper32(0)});
  s={...s,registers:[...s.registers],pc:0x2004};s.registers[0]=DATA;
  run(c,unit,1,s,'compiler-first-leaf-pauses-before-lock',{retired:1,packet:helper32(DATA)});checkRam(c,0x5000,data);
  s={...s,registers:[...s.registers],pc:0x300a,flags:0x482};s.registers[4]=0x707c;
  run(c,unit,2,s,'compiler-first-natural-void-return',{retired:2,packet:helper32(0x300a)});counts.compiler_calls++;
  for(const i of [3,5,6,7])assert.equal(s.registers[i],r[i]);data.writeUInt32LE(0xa5a55a7a,16);checkRam(c,0x5000,data);
  snapshots.push({label:'first-return',cpu:cpu(c),arena_hex:arena(c).toString('hex'),cell:0xa5a55a7a});
  s={...s,registers:[...s.registers],pc:0x2000,flags:0x412};s.registers[4]=0x7078;s.registers[5]=0xa5a55a7a;
  run(c,unit,4,s,'compiler-second-call-retains-first-memory-observation',{retired:4,packet:helper32(0)});
  s={...s,registers:[...s.registers],pc:0x301d,flags:0x482};s.registers[4]=0x707c;
  run(c,unit,3,s,'compiler-second-natural-void-return',{retired:3,packet:helper32(0x301d)});counts.compiler_calls++;
  for(const i of [3,6,7])assert.equal(s.registers[i],r[i]);assert.equal(s.registers[5],0xa5a55a7a);checkRam(c,0x5000,data);
  snapshots.push({label:'second-return',cpu:cpu(c),arena_hex:arena(c).toString('hex'),cell:0xa5a55a7a,first_memory_in_ebp:0xa5a55a7a});
  s={...s,registers:[...s.registers],pc:0x302c,flags:0xc13};s.registers[0]=0x4b4ab4f4;s.registers[4]=0x7080;
  run(c,unit,5,s,'compiler-both-memory-observations-consumed-guest-self-loop',{retired:5,packet:helper32(0)});data.writeUInt32LE(0x4b4ab4f4,48);checkRam(c,0x5000,data);
  const stackBytes=Buffer.from(stack.bytes);stackBytes.set(words([0x301d,DATA]),0x7078-stack.address);checkRam(c,stack.address,stackBytes);
  const expected=[['store32',0x707c,DATA,0],['store32',0x7078,0x300a,0],['read32',0x707c,null,DATA],['or',DATA,0x20,0xa5a55a5a],['read32',0x7078,null,0x300a],['read32',DATA,null,0xa5a55a7a],
    ['store32',0x707c,DATA,0],['store32',0x7078,0x301d,0],['read32',0x707c,null,DATA],['or',DATA,0x20,0xa5a55a7a],['read32',0x7078,null,0x301d],['read32',DATA,null,0xa5a55a7a],['store32',0x5030,0x4b4ab4f4,0]];
  const events=helperTrace.slice(at);assert.equal(events.length,13);
  for(const [i,[kind,address,inputValue,old]] of expected.entries()){
    const e=events[i],name=kind==='or'?(c.owner==='resident'?'atomic_or_resident_u32':'atomic_or_u32'):kind==='store32'?(c.owner==='resident'?'store_resident32':'store32'):kind;
    assert.equal(e.name,name);assert.equal(e.address,address);assert.equal(e.input_value,inputValue);assert.equal(e.status,0);assert.equal(e.helper_hex,helper32(old).toString('hex'));
  }
  snapshots.push({label:'final',cpu:cpu(c),arena_hex:arena(c).toString('hex'),cell:0xa5a55a7a,first_memory:0xa5a55a7a,second_memory:0xa5a55a7a,consumed_memory_sum:0x4b4ab4f4});
  writeFileSync(join(output,`context-${c.id}-compiler-milestones.json`),JSON.stringify({snapshots,expected_helper_chronology:expected},null,2)+'\n',{flag:'wx'});
}
function smc(owner,identity){
  const c=open(owner,false,identity?'smc-identity':'smc-changed'),bytes=smcBytes(identity),head=compile(c,'consumed-code-atomic-or',[{pc:0xc000,bytes,instructions:3}]),r=parents();r[6]=0xc005;
  const old=identity?0x1234567a:0x1234565a,start=seed(c,0xc000,0xcd7,r),next=changed(start,old,0x20,0xc004),at=helperTrace.length;
  assert.equal(next.flags,0x402);run(c,head,1,next,identity?'contained-identity-code-write':'changed-code-dword-write',{reason:6,packet:helper(old)});counts.smc++;
  assert.equal(helperTrace.length-at,1);assert.equal(helperTrace.at(-1).status,11);
  const code=Buffer.from(bytes);code.writeUInt32LE(0x1234567a,5);const selectedCode=Buffer.alloc(12);code.copy(selectedCode);checkRam(c,0xc000,selectedCode);
  run(c,head,1,cpu(c),'consumed-owner-stale-neutral',{status:4});
  const suffix=compile(c,'fresh-suffix-no-or-replay',[{pc:0xc004,bytes:code.subarray(4),instructions:2}],[]),tail={...cpu(c),registers:[...cpu(c).registers],pc:0xc00b};tail.registers[2]=0x1234567a;
  run(c,suffix,3,tail,'fresh-suffix-uses-committed-code',{reason:3,retired:2});run(c,head,1,cpu(c),'old-owner-after-fresh-capture-neutral',{status:owner==='resident'?4:3});finish(c,head);
}
for(const owner of ['replacement','resident']){for(const entries of [false,true])core(owner,entries);for(const identity of [false,true])smc(owner,identity);}
assert.deepEqual(sourceHashes(),sourcesBefore);
// final source-derived census is frozen before root execution.
const expectedCounts = {contexts:8,modules:24,runs:496,retired:414,atomic_ors:370,helper_calls:414,read32:24,store32:20,
  masks:96,flag_rows:64,aliases:40,boundary:40,faults:80,repair_only:28,current_inputs:4,synthetic:48,smc:4,compiler_calls:8};
assert.deepEqual(counts,expectedCounts);assert.equal(diagnostics.length,3612);
const result={engine:{path:enginePath,sha256:engine.sha256,bytes:engine.bytes.length},source_pins:sourcesBefore,initial_arena_sha256:hash(initial),
  compiler:{text_hex:kernel.toString('hex'),text_sha256:hash(kernel),calls:counts.compiler_calls,combined_blocks:4,combined_instructions:14},
  counts,contexts,modules,observations,helper_trace:helperTrace,public_diagnostics:diagnostics,controls,
  limits:{blocks:8,instructions:64,module_bytes:65536,captures_per_context:6,mapped_pages:12},
  claim:'finite canonical private flat32 synchronous unshared exact single F0 83 /1 DWORD signextended imm8 OR; no hardware LOCK, C11 concurrent ordering, performance or general compatibility claim',
  verification_ceiling:'per-run full4612 asserted;8 final and16 compiler milestone arenas persisted; selected32B frames/64B compiler data/32B stack/12B code and publicdiagnostics, no wholeRAM; nonforwarding synthetic helpers never commitRAM; explicit changed-input controls are separate from map/protect-only faultrepair'};
assert.deepEqual(Object.keys(result.engine),['path','sha256','bytes']);assert.equal(result.engine.sha256,hash(engine.bytes));
writeFileSync(join(output,'result.json'),JSON.stringify(result,null,2)+'\n',{flag:'wx'});console.log(JSON.stringify({output,counts,result:join(output,'result.json')}));
