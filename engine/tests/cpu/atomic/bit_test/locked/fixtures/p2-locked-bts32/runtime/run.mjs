import {pageFaultExit, pageFaultPacket} from '../../../../../../../support/wasm/page-fault.mjs';
import {detectionGuardName, generatedImports, attachDetectionApi} from '../../../../../../../support/wasm/binding.mjs';
import {ARENA_SIZE} from '../../../../../../../support/wasm/arena.mjs';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../../../../../../../support/wasm/engine.mjs';

const [enginePath, output, root] = process.argv.slice(2), SIZE = ARENA_SIZE, DATA = 0x5010, FAULT = 0x1705;
assert.ok(enginePath && output && root);
const engine = readEngine(enginePath), initial = readFileSync(join(output, 'initial-arena.bin'));
const hash = b => createHash('sha256').update(b).digest('hex');
assert.equal(initial.length, SIZE);
assert.ok(engine.bytes instanceof Buffer && engine.module instanceof WebAssembly.Module && engine.bytes.length > 0);
assert.equal(engine.sha256, hash(engine.bytes));
const sourcePaths = ['engine/src/memory/space.rs', 'engine/src/memory/space/atomic.rs','engine/src/abi/memory_helper.rs','engine/src/abi/arena.rs',
  'engine/src/abi/wasm/exports/calls.rs', 'engine/src/abi/wasm/exports/cpu.rs', 'engine/src/abi/wasm/exports/execution.rs', 'engine/src/abi/wasm/exports/image.rs', 'engine/src/abi/wasm/exports/instance.rs', 'engine/src/abi/wasm/exports/memory.rs', 'engine/src/abi/wasm/exports/mod.rs','engine/src/process/instance.rs', 'engine/src/process/execution/compiled.rs', 'engine/src/process/execution/authority.rs', 'engine/src/process/memory/address_space.rs', 'engine/src/process/memory/scalar.rs', 'engine/src/process/memory/scalar/tests.rs', 'engine/src/process/execution/mod.rs', 'engine/src/process/memory/mod.rs', 'engine/src/process/mod.rs','engine/src/process/wasm/mod.rs', 'engine/src/process/wasm/loading.rs', 'engine/src/abi/image.rs', 'engine/src/process/wasm/execution.rs', 'engine/src/abi/execution.rs', 'engine/src/process/wasm/calls.rs', 'engine/src/process/wasm/memory.rs', 'engine/src/process/wasm/cpu.rs', 'engine/src/process/mod.rs','engine/src/process/execution/resident.rs', 'engine/src/process/execution/authority.rs', 'engine/src/process/execution/mod.rs', 'engine/src/process/mod.rs',
  'engine/src/cpu/x86/ir.rs','engine/src/cpu/x86/decode/profile.rs', 'engine/src/cpu/x86/decode/families.rs','engine/src/cpu/x86/decode/lower.rs', 'engine/src/cpu/x86/decode/families.rs','engine/src/cpu/x86/decode/integer.rs',
  'engine/src/cpu/dbt/region.rs','engine/src/cpu/dbt/wasm/emitter/mod.rs', 'engine/src/cpu/dbt/wasm/emitter/module.rs', 'engine/src/cpu/dbt/wasm/emitter/body.rs', 'engine/src/cpu/dbt/wasm/requirements.rs', 'engine/src/cpu/dbt/wasm/imports/wiring.rs','engine/src/cpu/dbt/wasm/instructions/integer/arithmetic.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/bits.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/carry_rotates.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/decimal.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/flags.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/mod.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/multiply_divide.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/operands.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/rotates.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/shifts.rs',
  'engine/src/cpu/dbt/wasm/imports/mod.rs', 'engine/src/cpu/dbt/wasm/imports/binding.rs', 'engine/src/cpu/dbt/wasm/imports/context.rs', 'engine/src/cpu/dbt/wasm/imports/needs.rs', 'engine/src/cpu/dbt/wasm/helpers/protocol.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/mod.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/address.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/scalar.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/stack.rs','engine/src/cpu/dbt/wasm/instructions/memory/exchange.rs','engine/src/cpu/dbt/wasm/locals.rs',
  'engine/tests/process/memory/bit_test/native/set.rs', 'engine/tests/process/memory/native.rs', 'engine/tests/process/memory/native.rs','engine/tests/cpu/atomic/bit_test/locked/native/set32.rs', 'engine/tests/cpu/atomic/native.rs', 'engine/tests/support/cpu/prefix/admission.rs', 'engine/tests/cpu/atomic/native.rs','engine/tests/cpu/atomic/bit_test/locked/compiler/native/set32.rs', 'engine/tests/cpu/atomic/native.rs', 'engine/tests/cpu/atomic/native.rs','engine/tests/cpu/atomic/bit_test/locked/wasm/set32.rs', 'engine/tests/cpu/atomic/wasm.rs', 'engine/tests/support/wasm.rs', 'engine/tests/cpu/atomic/wasm.rs', 'engine/tests/support/wasm.rs',
  ...['kernel.c','build.py','PROVENANCE.md','kernel.obj','kernel.text.bin','kernel.disasm.txt','receipt.json','runtime/run.mjs'].map(p=>'engine/tests/cpu/atomic/bit_test/locked/fixtures/p2-locked-bts32/'+p),
  'engine/tests/programs/records/fixtures/p2-compiled-records/build.py','engine/tests/support/wasm/engine.mjs', 'engine/tests/support/wasm/binding.mjs', 'engine/tests/support/wasm/page-fault.mjs'];
sourcePaths.push('engine/src/abi/memory_helper/cpu.rs','engine/src/cpu/dbt/wasm/instructions/memory/cpu_scalar.rs','engine/src/cpu/dbt/wasm/emitter/mod.rs', 'engine/src/cpu/dbt/wasm/emitter/module.rs', 'engine/src/cpu/dbt/wasm/emitter/body.rs', 'engine/src/cpu/dbt/wasm/requirements.rs', 'engine/src/cpu/dbt/wasm/imports/wiring.rs','engine/src/process/memory/cpu_scalar.rs','engine/src/process/memory/scalar/cpu_write.rs','engine/src/process/wasm/memory/cpu_write.rs','engine/src/abi/wasm/exports/memory/cpu_write.rs','engine/src/memory/space/cpu/atomic.rs');
const sourceHashes = () => Object.fromEntries(sourcePaths.map(p=>[p,hash(readFileSync(join(root,p)))]));
const sourcesBefore=sourceHashes(),contexts=[],modules=[],observations=[],helperTrace=[],diagnostics=[],controls=[];
const counts={contexts:0,modules:0,runs:0,retired:0,bit_sets:0,helper_calls:0,read32:0,store32:0,
  sources:0,flag_rows:0,indices:0,aliases:0,boundary:0,faults:0,repair_only:0,current_inputs:0,synthetic:0,smc:0,compiler_calls:0};
const word=n=>{const b=Buffer.alloc(4);b.writeUInt32LE(n>>>0);return b;}, words=v=>Buffer.concat(v.map(word));
function record(magic,version,size,fields){const b=Buffer.alloc(size);b.write(magic);b.writeUInt16LE(version,4);b.writeUInt16LE(1,6);b.writeUInt32LE(size,8);fields.forEach((v,i)=>b.writeUInt32LE(v>>>0,16+i*4));return b;}
const state=s=>record('R3ST',1,56,[...s.registers,s.pc,s.flags]);
const legacyExit = (reason,retired,{detail=0,address=0,access=0}={},version=2)=>record('R3EX',reason===11?6:version,40,[reason,retired,detail,address,access,access?4:0]);
const exit = (reason, retired, { detail = 0, address = 0, access = 0 } = {}, version = 2) =>
  reason === 5
    ? pageFaultExit({
        retired,
        errorCode: 6 | (detail === 2 ? 1 : 0),
        address,
        access: 2,
        length: access ? 4 : 0
      })
    : record('R3EX', reason === 11 ? 6 : version, 40, [
        reason,
        retired,
        detail,
        address,
        access,
        access ? 4 : 0
      ]);
const initialHelper=(value=0,{detail=0,address=0,access=0}={})=>record('R3MH',1,40,[access?1:0,value,detail,address,access,access?4:0]);
const legacyHelper = (value=0,{detail=0,address=0,access=0}={})=>record('R3MH',10,40,[access?1:0,value,detail,address,access,4]);
const helper = (value = 0, { detail = 0, address = 0, access = 0 } = {}) =>
  Boolean(access) && (detail === 1 || detail === 2)
    ? pageFaultPacket({ errorCode: 6 | (detail === 2 ? 1 : 0), address, access: 2, length: 4 })
    : record('R3MH', 10, 40, [access ? 1 : 0, value, detail, address, access, 4]);
const helper32=value=>initialHelper(value);
const parents=()=>Array.from({length:8},(_,i)=>(0xa100+i)*65536+0x1234+i);
const mod32=n=>Number((n%4294967296n+4294967296n)%4294967296n);
const signed=n=>n>=0x80000000?BigInt(n)-4294967296n:BigInt(n);
const offset=index=>(signed(index)>>5n)*4n;
const selected=(base,index)=>mod32(BigInt(base)+offset(index));
const setWord=(old,bit)=>Number(BigInt(old)|(1n<<BigInt(bit&31)));
const oldBit=(old,bit)=>Number((BigInt(old)>>BigInt(bit&31))&1n);
const flags=(incoming,old,index)=>(incoming&0x442)|oldBit(old,index);
for(const [base,index,at] of [[DATA,31,DATA],[DATA,32,DATA+4],[DATA,0xffffffff,DATA-4],[DATA,0xffffffe0,DATA-4],[DATA,0xffffffdf,DATA-8],[0x10005010,0x80000000,DATA],[0xf0005014,0x7fffffff,DATA]])assert.equal(selected(base,index),at);
const kernel=readFileSync(join(root,'engine/tests/cpu/atomic/bit_test/locked/fixtures/p2-locked-bts32/kernel.text.bin'));
assert.equal(kernel.toString('hex'),'8b4c2404ba0500000031c0f00fab110f92c0c3');assert.equal(hash(kernel),'b197e56d9f334c69c7f1cf552f0c000c3f7c9c4339e9e3cec32983a685480326');
const normal=Array.from({length:8},(_,source)=>({pc:0x1000+source*16,bytes:Buffer.from([0xf0,0x0f,0xab,(source<<3)|5,0x10,0x50,0,0,0xeb,0]),instructions:2,source,next:0x1008+source*16}));
const aliasRows=[
  {pc:0x1400,bytes:Buffer.from([0xf0,0x0f,0xab,0x00]),source:0,base:0,set:{0:0x5000},selected:0x5a00},
  {pc:0x1404,bytes:Buffer.from([0xf0,0x0f,0xab,0x24,0x24]),source:4,base:4,set:{4:0x5000},selected:0x5a00},
  {pc:0x1409,bytes:Buffer.from([0xf0,0x0f,0xab,0x6d,0]),source:5,base:5,displacement:0,set:{5:0x5001},selected:0x5a01},
  {pc:0x140e,bytes:Buffer.from([0xf0,0x0f,0xab,0x4c,0x8e,0xf0]),source:1,base:6,index:1,scale:4,displacement:-16,set:{6:0x5000,1:8},selected:DATA},
  {pc:0x1414,bytes:Buffer.from([0xf0,0x0f,0xab,0x5c,0x9b,1]),source:3,base:3,index:3,scale:4,displacement:1,set:{3:0x1003},selected:0x5210},
].map(r=>({...r,next:r.pc+r.bytes.length}));
const aliases=[{pc:0x1400,bytes:Buffer.concat([...aliasRows.map(r=>r.bytes),Buffer.from([0xeb,0])]),instructions:6}];
function addressOf(row,r){return mod32(BigInt(row.displacement??0)+(row.base===undefined?0n:BigInt(r[row.base]))+(row.index===undefined?0n:BigInt(r[row.index])*BigInt(row.scale??1)));}
const faultBlock={pc:0x1700,bytes:Buffer.from([0xbb,0xbe,0xba,0xfe,0xca,0xf0,0x0f,0xab,0x0e,0xeb,0]),instructions:3};
const live={pc:0x1800,bytes:Buffer.from([0xf0,0x0f,0xab,0x0e,0x0f,0x92,0xc3,0x0f,0x94,0xc7,0xeb,0]),instructions:4};
const caller=[
  {pc:0x3000,bytes:Buffer.from('6810500000e8f6efffff','hex'),instructions:2},
  {pc:0x300a,bytes:Buffer.from('83c40489c56810500000e8e7efffff','hex'),instructions:4},
  {pc:0x3019,bytes:Buffer.from('83c404d1e009e8a330500000ebfe','hex'),instructions:5},
];
const kernelBlocks=[{pc:0x2000,bytes:kernel,instructions:6}];
assert.deepEqual(caller.map(b=>b.pc+b.bytes.length),[0x300a,0x3019,0x3027]);
const smcBytes=identity=>Buffer.concat([Buffer.from([0xf0,0x0f,0xab,0x0e,0xba]),word(identity?0x1234567a:0x1234565a),Buffer.from([0xeb,0])]);
const arena = c => Buffer.from(new Uint8Array(c.memory.buffer, c.base, SIZE));
const cpu = c => {const b = arena(c); return {registers: Array.from({length: 8}, (_, i) => b.readUInt32LE(16 + 4 * i)), pc: b.readUInt32LE(48), flags: b.readUInt32LE(52)};};
function input(c, offset, bytes) {const expected = arena(c); expected.set(bytes, offset); new Uint8Array(c.memory.buffer).set(bytes, c.base + offset); assert.deepEqual(arena(c), expected);}
function host(c, name, args, span) {const expected = arena(c); assert.equal(c.api[name](...args), 0, name); if (span) expected.set(arena(c).subarray(...span), span[0]); assert.deepEqual(arena(c), expected, 'host complete arena ' + name);}
function map(c, page, permission = 3) {host(c, 'map', [page, 1, permission]); c.pages.add(page); c.maxPages = Math.max(c.maxPages, c.pages.size); assert.ok(c.maxPages <= 12);}
function protect(c, page, permission) {host(c, 'protect', [page, 1, permission]);}
function unmap(c, page) {host(c, 'unmap', [page, 1]); c.pages.delete(page);}
function upload(c, address, bytes) {assert.ok(address >= 0 && address + bytes.length <= 4294967296); input(c, 140, bytes); host(c, 'upload', [address, bytes.length]);}
function seed(c, pc, flags = 0xcd7, registers = parents()) {input(c, 0, Buffer.concat([state({registers, pc, flags}), exit(1, 0), word(0), initialHelper(0xad)])); return cpu(c);}
const arities = {open: 3, close: 0, arena_ptr: 0, map: 3, protect: 3, unmap: 2, upload: 2, read32: 1, store32: 2, store_resident32: 6, bit_test_set32: 2, bit_test_set_resident32: 6, compile: 1, compile_entries: 2,
  compile_resident: 1, compile_resident_entries: 2, generation: 0, module_ptr: 0, module_len: 0, guard: 6, guard_resident: 7};

function open(owner, entries, purpose) {
  const instance = new WebAssembly.Instance(engine.module, {}), c = {id: ++counts.contexts, owner, entries, purpose, memory: instance.exports.memory, api: {}, high: 0x42545332, captures: 0, pages: new Set(), maxPages: 0};
  for (const [name, arity] of Object.entries(arities)) {c.api[name] = instance.exports['ring3_abi_v1_' + name]; assert.equal(c.api[name]?.length, arity, name);}
 attachDetectionApi(c.api, instance.exports);
  for (const name of ['read32', 'store32', 'store_resident32', 'bit_test_set32', 'bit_test_set_resident32']) {c.api['cpu_'+name] = instance.exports['ring3_abi_v1_cpu_'+name]; assert.equal(c.api['cpu_'+name]?.length, arities[name], 'cpu_'+name);}
  assert.equal(c.api.open(12, c.id, c.high), 0); c.base = c.api.arena_ptr() >>> 0; input(c, 0, initial);
  for (const [page, permission] of [[0, 3], [0x1000, 7], [0x2000, 7], [0x3000, 7], [0x5000, 3], [0x6000, 3], [0x7000, 3], [0xfffff000, 3]]) map(c, page, permission);
  for (const block of [...normal, ...aliases, faultBlock, live, ...caller]) upload(c, block.pc, block.bytes);
  upload(c, 0x2000, kernel); protect(c, 0x2000, 5); protect(c, 0x3000, 5);
  if (purpose.startsWith('smc')) {map(c, 0xc000, 7); upload(c, 0xc000, smcBytes(purpose === 'smc-identity'));}
  return c;
}
function compile(c, name, blocks, helpers = ['bts']) {
  const instructionCount = blocks.reduce((n, b) => n + b.instructions, 0);
  assert.ok(blocks.length > 0 && blocks.length <= 8 && instructionCount <= 64); assert.ok(++c.captures <= 6);
  input(c, 140, words(blocks.flatMap(b => c.entries ? [b.pc] : [b.pc, b.bytes.length])));
  const method = c.owner === 'resident' ? (c.entries ? 'compile_resident_entries' : 'compile_resident') : (c.entries ? 'compile_entries' : 'compile');
  host(c, method, c.entries ? [blocks.length, 0] : [blocks.length], c.owner === 'resident' ? [140, 164] : null);
  const b = arena(c), binding = c.owner === 'resident' ? {low: b.readUInt32LE(148), high: b.readUInt32LE(152), pointer: b.readUInt32LE(156), length: b.readUInt32LE(160)} : {generation: c.api.generation(), pointer: c.api.module_ptr() >>> 0, length: c.api.module_len() >>> 0};
  const bytes = Buffer.from(new Uint8Array(c.memory.buffer, binding.pointer, binding.length)), module = new WebAssembly.Module(bytes), guard = detectionGuardName(c.owner === 'resident');
  const names = helpers.map(n => n === 'bts' ? (c.owner === 'resident' ? 'bit_test_set_resident32' : 'bit_test_set32') : n === 'store' ? (c.owner === 'resident' ? 'store_resident32' : 'store32') : n).map(n => 'cpu_'+n);
  const imports = [{module: 'env', name: 'memory', kind: 'memory'}, ...generatedImports({resident: c.owner === 'resident', helpers: [...names]}).slice(1).map(item => item.name).map(name => ({module: 'ring3', name, kind: 'function'}))];
  assert.ok(bytes.length <= 65536); assert.deepEqual(WebAssembly.Module.imports(module), imports); assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  const ring3 = {...c.api};
  for (const name of names) ring3[name] = (...args) => {
    const before = arena(c), status = c.api[name](...args), after = arena(c), expected = Buffer.from(before); expected.set(after.subarray(100, 140), 100);
    assert.deepEqual(after, expected, 'canonical helper changes only its 40-byte record');
    const legacyName = name.slice(4), payload = args.slice(name.includes('resident') ? 4 : 0).map(v => v >>> 0);
    helperTrace.push({context: c.id, call: counts.runs + 1, name, address: payload[0], input_value: legacyName.startsWith('read') ? null : payload.at(-1), status, helper_hex: after.subarray(100, 140).toString('hex')});
    counts.helper_calls++; counts[legacyName.startsWith('bit_test_set') ? 'bit_sets' : legacyName.startsWith('read') ? legacyName : 'store32']++; return status;
  };
  const run = new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3}).exports.run; assert.equal(run.length, 4);
  const file = `module-${++counts.modules}.wasm`; writeFileSync(join(output, file), bytes, {flag: 'wx'});
  const unit = {...binding, name, module, ring3, run, exitVersion: names.length ? 6 : 1};
  modules.push({context: c.id, owner: c.owner, entries: c.entries, name, file, bytes: bytes.length, sha256: hash(bytes), blocks: blocks.length, instructions: instructionCount, imports, ...binding}); return unit;
}
function run(c, unit, budget, next, label, {status = 0, reason = 1, retired = 1, packet, fault = {}, synthetic = false} = {}) {
  const before = arena(c), expected = Buffer.from(before), at = helperTrace.length;
  if (status === 0) {expected.set(state(next)); expected.set((synthetic ? legacyExit : exit)(reason, retired, fault, unit.exitVersion), 56); if (packet) expected.set(packet, 100);}
  const observed = unit.run(c.base, c.base + 56, budget, c.base + 96); assert.equal(observed, status, label); assert.deepEqual(arena(c), expected, 'complete arena ' + label);
  const after = arena(c); observations.push({context: c.id, module: unit.name, label, budget, synthetic, expected_status: status, observed_status: observed,
    model_reason: reason, expected_reason: status ? null : reason === 5 && !synthetic ? 23 : reason, observed_reason: status ? null : after.readUInt32LE(72), expected_retired: status ? null : retired, observed_retired: status ? null : after.readUInt32LE(76),
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

function changed(start,old,index,next){return {...start,registers:[...start.registers],pc:next,flags:flags(start.flags,old,index)};}
function btsCase(c,unit,row,address,old,registers,incoming,label,f=frame(c,address,old)){
  const start=seed(c,row.pc,incoming,registers),index=registers[row.source];
  run(c,unit,1,changed(start,old,index,row.next),label,{packet:helper(old)});
  const value=setWord(old,index);checkRam(c,f.address,changedFrame(f,address,value).bytes);
  assert.deepEqual(cpu(c).registers,registers,'BTS changes no GPR');return {frame:changedFrame(f,address,value),value};
}
function stopped(c,unit,registers,incoming,fault,label,prior=true){
  const start=prior?seed(c,0x1700,incoming,registers):cpu(c),next={...start,registers:[...start.registers],pc:FAULT};
  if(prior)next.registers[3]=0xcafebabe;
  run(c,unit,2,next,label,{reason:fault.detail===3?11:5,retired:prior?1:0,fault:fault.detail===3?{}:fault,packet:fault.detail===3?undefined:helper(0,fault)});counts.faults++;return next;
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
  const c=open(owner,entries,'core'),a=compile(c,'all-eight-register-indices',normal);
  for(const row of normal)for(const index of [0,31]){
    const r=parents();r[row.source]=index;const old=index?0xffffffff:0;
    btsCase(c,a,row,DATA,old,r,0xcd7,'all-registers-oldbit-'+(index?1:0));counts.sources++;
  }
  const flagInputs=[2,3,0x42,0x43,0x402,0x403,0x8d6,0xcd7];
  for(const [i,incoming] of flagInputs.entries())for(const bit of [0,1]){
    const row=normal[i],r=parents();r[row.source]=5;const old=bit?0x1234567a:0x1234565a;
    btsCase(c,a,row,DATA,old,r,incoming,'defined-cf-zf-and-profile-undefined-flags');counts.flag_rows++;
  }
  const b=compile(c,'signed-indices-aliases-boundary-fault',[...aliases,faultBlock]);
  const selectedRow={pc:FAULT,source:1,next:FAULT+4};
  for(const index of [0,1,31,32,33,0xffffffff,0xffffffe1,0xffffffe0,0xffffffdf,0x7fffffff,0x80000000])for(const bit of [0,1]){
    const r=parents();r[1]=index;r[6]=mod32(BigInt(DATA)-offset(index));assert.equal(selected(r[6],index),DATA);
    const mask=Number(1n<<BigInt(index&31)),old=Number((BigInt(0xa5a55a5a)&(0xffffffffn^BigInt(mask)))|(bit?BigInt(mask):0n));
    btsCase(c,b,selectedRow,DATA,old,r,0xcd7,'signed-register-bit-string');counts.indices++;
  }
  for(const row of aliasRows)for(const bit of [0,1]){
    const r=parents();for(const [i,v] of Object.entries(row.set))r[Number(i)]=v;
    const address=selected(addressOf(row,r),r[row.source]);assert.equal(address,row.selected);
    const mask=1n<<BigInt(r[row.source]&31),old=Number((0xa5a55a5an&(0xffffffffn^mask))|(bit?mask:0n));
    btsCase(c,b,row,address,old,r,0xcd7,'original-index-aliases-apparent-ea');counts.aliases++;
  }
  for(const address of [0,1,0x5ffd,0x5fff,0xfffffffc])for(const bit of [0,1]){
    const r=parents();r[1]=31;r[6]=address;btsCase(c,b,selectedRow,address,bit?0x80000001:1,r,0x402,'selected-dword-boundary');counts.boundary++;
  }
  const r=parents();r[1]=5;r[6]=0x9008;
  for(const [budget,cancel,reason] of [[0,0,1],[1,1,2],[0,1,2]]){
    const start=seed(c,FAULT,0xcd7,r);input(c,96,word(cancel));run(c,b,budget,start,'helper-free-budget-cancel',{reason,retired:0});controls.push({context:c.id,kind:'budget-cancel',budget,cancel});
  }
  input(c,96,word(0));let s=seed(c,0x1900,0x402);run(c,b,2,s,'cold-pc',{reason:3,retired:0});controls.push({context:c.id,kind:'cold-pc'});
  for(const [kind,status,options] of [['current',0,{}],['wrong-key-before-pointers',3,{badKey:true,badPointer:true}],['wrong-unit-before-pointers',3,{badId:true,badPointer:true}],['current-malformed-pointer',1,{badPointer:true}]])guardProbe(c,b,kind,status,options);
  const cases=[
    {name:'read-unmapped',address:0x9010,page:0x9000,permission:null,access: 2,detail:1,at:0x9010},
    {name:'read-write-only',address:0x9010,page:0x9000,permission:2,access: 2,detail:2,at:0x9010},
    {name:'already-set-read-only',address:0x9010,page:0x9000,permission:1,access:2,detail:2,at:0x9010},
    {name:'cross-second-unmapped',address:0x5fff,page:0x6000,permission:null,access: 2,detail:1,at:0x6000},
    {name:'cross-first-read-only',address:0x5fff,page:0x5000,permission:1,access:2,detail:2,at:0x5fff},
    {name:'read-overflow',address:0xfffffffd,access: 2,detail:3,at:0xfffffffd},
  ];
  for(const item of cases){
    let f;
    if(item.name==='read-overflow'){upload(c,0xffffffe0,Buffer.alloc(32,0x5a));upload(c,0,Buffer.alloc(32,0xa5));}
    else {if(!c.pages.has(item.page))map(c,item.page);f=frame(c,item.address,0x1234567a);if(item.permission===null)unmap(c,item.page);else protect(c,item.page,item.permission);}
    const registers=parents();registers[1]=5;registers[6]=item.address;const fault={detail:item.detail,address:item.at,access:item.access};
    stopped(c,b,registers,0xcd7,fault,item.name);if (item.permission === 1)checkRam(c,f.address,f.bytes);
    stopped(c,b,registers,0xcd7,fault,item.name+'-unchanged',false);if (item.permission === 1)checkRam(c,f.address,f.bytes);
    if(item.name==='read-overflow'){checkRam(c,0xffffffe0,Buffer.alloc(32,0x5a));checkRam(c,0,Buffer.alloc(32,0xa5));continue;}
    if(item.permission===null){map(c,item.page);const bytes=Buffer.from(f.bytes);for(let i=0;i<bytes.length;i++)if(f.address+i>=item.page&&f.address+i<item.page+4096)bytes[i]=0;f={...f,bytes};}
    else protect(c,item.page,3);
    checkRam(c,f.address,f.bytes);s=cpu(c);const old=f.bytes.readUInt32LE(item.address-f.address);
    run(c,b,1,changed(s,old,s.registers[1],FAULT+4),item.name+'-repair-only',{packet:helper(old)});counts.repair_only++;
    checkRam(c,f.address,changedFrame(f,item.address,setWord(old,s.registers[1])).bytes);if(item.page===0x9000)unmap(c,0x9000);
  }
  let f=frame(c,0x5fff,0x1234567a);protect(c,0x5000,1);protect(c,0x6000,2);
  const split=parents();split[1]=5;split[6]=0x5fff;
  const writeFault ={detail:2,address:0x5fff,access:2};
  stopped(c,b,split,0x402,writeFault,'earlier-write-before-later-read');stopped(c,b,split,0x402,writeFault,'earlier-write-repeat',false);
  protect(c,0x6000,1);checkRam(c,f.address,f.bytes);stopped(c,b,split,0x402,writeFault,'later-page-repair-retains-earlier-write',false);checkRam(c,f.address,f.bytes);
  stopped(c,b,split,0x402,writeFault,'write-repeat',false);checkRam(c,f.address,f.bytes);protect(c,0x5000,3);protect(c,0x6000,3);s=cpu(c);
  run(c,b,1,changed(s,0x1234567a,5,FAULT+4),'both-accesses-repair-only',{packet:helper(0x1234567a)});counts.repair_only++;checkRam(c,f.address,f.bytes);
  f=frame(c,DATA,0x1234567a);protect(c,0x5000,1);const current=parents();current[1]=5;current[6]=DATA;
  const currentFault={detail:2,address:DATA,access:2};stopped(c,b,current,0xcd7,currentFault,'current-input-stopped');checkRam(c,f.address,f.bytes);
  stopped(c,b,current,0xcd7,currentFault,'current-input-unchanged',false);checkRam(c,f.address,f.bytes);protect(c,0x5000,3);
  const fresh=frame(c,DATA+64,0x40000000);input(c,16,word(0xf1e2d3c4));input(c,20,word(31));input(c,40,word(DATA+64));input(c,52,word(0x402));s=cpu(c);
  run(c,b,1,changed(s,0x40000000,31,FAULT+4),'explicit-current-index-ea-flags-ram',{packet:helper(0x40000000)});counts.current_inputs++;
  checkRam(c,f.address,f.bytes);checkRam(c,fresh.address,changedFrame(fresh,DATA+64,0xc0000000).bytes);
  synthetic(c,b);
  const retained=frame(c,DATA,0x1234567a),rr=parents();rr[0]=5;s=seed(c,normal[0].pc,0x402,rr);
  if(owner==='replacement')run(c,a,1,s,'old-generation-neutral',{status:3});
  else {run(c,a,1,changed(s,0x1234567a,5,normal[0].next),'retained-current-owner',{packet:helper(0x1234567a)});checkRam(c,retained.address,retained.bytes);}
  const l=compile(c,'live-cf-zf-consumer',[live]);liveProof(c,l);
  upload(c,normal[0].pc,normal[0].bytes.subarray(0,1));s=seed(c,live.pc,0x402);run(c,l,1,s,'same-byte-source-invalidated',{status:4});guardProbe(c,l,'stale-before-pointers',4,{badPointer:true});
  const compiled=compile(c,'original-two-live-calls',[...kernelBlocks,...caller],['read32','store','bts']);compilerProof(c,compiled);finish(c,compiled);
}
function synthetic(c,unit){
  const infra=(value=0,detail=1)=>record('R3MH',10,40,[2,value,detail,0,0,4]);
  const rows=[
    {name:'magic',edit:b=>b.write('BAD!')},
    {name:'fault-value',packet:legacyHelper(1,{detail:2,address:DATA,access:1})},
    {name:'invalidated-fault',status:11,packet:legacyHelper(0,{detail:2,address:DATA,access:1})},
    {name:'invalidated-infrastructure',status:11,packet:infra()},
    {name:'write-unmapped',packet:legacyHelper(0,{detail:1,address:DATA,access:2})},
    {name:'write-overflow',address:0xfffffffc,packet:legacyHelper(0,{detail:3,address:0xfffffffd,access:2})},
    {name:'current-v10-top-read-fault',address:0xfffffffc,packet:legacyHelper(0,{detail:2,address:0xfffffffd,access:1})},
    {name:'overflow-success',address:0xfffffffd},
    {name:'infrastructure-value',packet:infra(1)},
    {name:'canonical-version-exhaustion',packet:infra(),detail:1},
    {name:'canonical-generic-infrastructure',packet:infra(0,2),detail:3},
    {name:'unknown-status',status:99,detail:3},
  ];
  assert.equal(rows.length,12);
  for(const row of rows){
    const address=row.address??DATA,r=parents();r[1]=5;r[6]=address;
    const f=frame(c,Math.min(address,0xfffffffc),0x1234567a),zero=(address>0xfffffffc||row.name==='write-overflow')?frame(c,0,0x33445566):null;
    const packet=Buffer.from(row.packet??legacyHelper(0x1234567a));row.edit?.(packet);let calls=0;
    const ring3={...unit.ring3},name=c.owner==='resident'?'bit_test_set_resident32':'bit_test_set32';
    ring3['cpu_'+name]=(...args)=>{assert.deepEqual(args.slice(name.includes('resident')?4:0).map(v=>v>>>0),[address,5]);input(c,100,packet);calls++;return row.status??0;};
    const fake={...unit,name:unit.name+'-synthetic',run:new WebAssembly.Instance(unit.module,{env:{memory:c.memory},ring3}).exports.run},start=seed(c,FAULT,0xcd7,r);
    // flat-4GiB logical span overflow is stopped before any injected helper packet.
    const segmentOverflow=address>0xfffffffc;
    run(c,fake,1,start,'nonforwarding-'+row.name,segmentOverflow?{reason:11,retired:0,synthetic:true}:{reason:row.name==='current-v10-top-read-fault'?5:7,retired:0,fault:row.name==='current-v10-top-read-fault'?{detail:2,address:0xfffffffd,access:1}:{detail:row.detail??2},packet,synthetic:true});
    assert.equal(calls,segmentOverflow?0:1);checkRam(c,f.address,f.bytes);if(zero)checkRam(c,zero.address,zero.bytes);counts.synthetic++;
  }
}
function liveProof(c,unit){
  const f=frame(c,DATA,0x1234567a),r=parents();r[1]=5;r[6]=DATA;
  let s=seed(c,live.pc,0xcd7,r);s=changed(s,0x1234567a,5,0x1804);assert.equal(s.flags,0x443);
  run(c,unit,1,s,'live-cf-oldbit-zf-retained',{packet:helper(0x1234567a)});
  s={...s,registers:[...s.registers],pc:0x1807};s.registers[3]=s.registers[3]-s.registers[3]%256+1;
  run(c,unit,1,s,'live-setb-consumes-oldbit');
  s={...s,registers:[...s.registers],pc:0x180a};s.registers[3]=s.registers[3]-Math.floor(s.registers[3]/256)%256*256+256;
  run(c,unit,1,s,'live-setz-consumes-preserved-zf');s={...s,pc:0x180c};
  run(c,unit,2,s,'live-cold-successor',{reason:3});checkRam(c,f.address,f.bytes);
}
function compilerProof(c,unit){
  const data=Buffer.from(Array.from({length:64},(_,i)=>(i*17+23)&255));data.writeUInt32LE(0xa5a55a5a,16);upload(c,0x5000,data);
  const stack=frame(c,0x706c,0x11223344),r=parents();r[4]=0x7080;
  let s=seed(c,0x3000,0xcd7,r),at=helperTrace.length;const snapshots=[{label:'entry',cpu:s,arena_hex:arena(c).toString('hex')}];
  s={...s,registers:[...s.registers],pc:0x2000};s.registers[4]=0x7078;
  run(c,unit,2,s,'compiler-first-cdecl-call',{retired:2,packet:helper(0)});
  s={...s,registers:[...s.registers],pc:0x200b,flags:0x446};s.registers[0]=0;s.registers[1]=DATA;s.registers[2]=5;
  run(c,unit,3,s,'compiler-first-leaf-pauses-before-lock',{retired:3,packet:helper(DATA)});checkRam(c,0x5000,data);
  s={...s,registers:[...s.registers],pc:0x300a,flags:0x442};s.registers[4]=0x707c;
  run(c,unit,3,s,'compiler-first-natural-oldbit-return',{retired:3,packet:helper(0x300a)});counts.compiler_calls++;
  for(const i of [3,5,6,7])assert.equal(s.registers[i],r[i]);data.writeUInt32LE(0xa5a55a7a,16);checkRam(c,0x5000,data);
  snapshots.push({label:'first-return',cpu:cpu(c),arena_hex:arena(c).toString('hex'),cell:0xa5a55a7a,returned_oldbit:0});
  s={...s,registers:[...s.registers],pc:0x2000,flags:0x412};s.registers[4]=0x7078;s.registers[5]=0;
  run(c,unit,4,s,'compiler-second-call-retains-first-oldbit',{retired:4,packet:helper(0)});
  s={...s,registers:[...s.registers],pc:0x3019,flags:0x443};s.registers[0]=1;s.registers[1]=DATA;s.registers[2]=5;s.registers[4]=0x707c;
  run(c,unit,6,s,'compiler-second-natural-oldbit-return',{retired:6,packet:helper(0x3019)});counts.compiler_calls++;
  for(const i of [3,6,7])assert.equal(s.registers[i],r[i]);assert.equal(s.registers[5],0);checkRam(c,0x5000,data);
  snapshots.push({label:'second-return',cpu:cpu(c),arena_hex:arena(c).toString('hex'),cell:0xa5a55a7a,returned_oldbit:1});
  s={...s,registers:[...s.registers],pc:0x3025,flags:0x402};s.registers[0]=2;s.registers[4]=0x7080;
  run(c,unit,5,s,'compiler-both-oldbits-consumed-guest-self-loop',{retired:5,packet:helper(0)});data.writeUInt32LE(2,48);checkRam(c,0x5000,data);
  const stackBytes=Buffer.from(stack.bytes);stackBytes.set(words([0x3019,DATA]),0x7078-stack.address);checkRam(c,stack.address,stackBytes);
  const expected=[['store32',0x707c,DATA,0],['store32',0x7078,0x300a,0],['read32',0x707c,null,DATA],['bts',DATA,5,0xa5a55a5a],['read32',0x7078,null,0x300a],
    ['store32',0x707c,DATA,0],['store32',0x7078,0x3019,0],['read32',0x707c,null,DATA],['bts',DATA,5,0xa5a55a7a],['read32',0x7078,null,0x3019],['store32',0x5030,2,0]];
  const events=helperTrace.slice(at);assert.equal(events.length,11);
  for(const [i,[kind,address,inputValue,old]] of expected.entries()){
    const e=events[i],name=kind==='bts'?(c.owner==='resident'?'bit_test_set_resident32':'bit_test_set32'):kind==='store32'?(c.owner==='resident'?'store_resident32':'store32'):kind;
    assert.equal(e.name,'cpu_'+name);assert.equal(e.address,address);assert.equal(e.input_value,inputValue);assert.equal(e.status,0);assert.equal(e.helper_hex,helper(old).toString('hex'));
  }
  snapshots.push({label:'final',cpu:cpu(c),arena_hex:arena(c).toString('hex'),cell:0xa5a55a7a,consumed_oldbits:2});
  writeFileSync(join(output,`context-${c.id}-compiler-milestones.json`),JSON.stringify({snapshots,expected_helper_chronology:expected},null,2)+'\n',{flag:'wx'});
}
function smc(owner,identity){
  const c=open(owner,false,identity?'smc-identity':'smc-changed'),bytes=smcBytes(identity),head=compile(c,'consumed-code-bit-set',[{pc:0xc000,bytes,instructions:3}]),r=parents();r[1]=5;r[6]=0xc005;
  const old=identity?0x1234567a:0x1234565a,start=seed(c,0xc000,0xcd7,r),next=changed(start,old,5,0xc004),at=helperTrace.length;
  assert.equal(next.flags,identity?0x443:0x442);run(c,head,1,next,identity?'already-set-identity-code-write':'changed-code-dword-write',{reason:6,packet:helper(old)});counts.smc++;
  assert.equal(helperTrace.length-at,1);assert.equal(helperTrace.at(-1).status,11);
  const code=Buffer.from(bytes);code.writeUInt32LE(0x1234567a,5);const selectedCode=Buffer.alloc(12);code.copy(selectedCode);checkRam(c,0xc000,selectedCode);
  run(c,head,1,cpu(c),'consumed-owner-stale-neutral',{status:4});
  const suffix=compile(c,'fresh-suffix-no-bts-replay',[{pc:0xc004,bytes:code.subarray(4),instructions:2}],[]),tail={...cpu(c),registers:[...cpu(c).registers],pc:0xc00b};tail.registers[2]=0x1234567a;
  run(c,suffix,3,tail,'fresh-suffix-uses-committed-code',{reason:3,retired:2});run(c,head,1,cpu(c),'old-owner-after-fresh-capture-neutral',{status:owner==='resident'?4:3});finish(c,head);
}
for(const owner of ['replacement','resident']){for(const entries of [false,true])core(owner,entries);for(const identity of [false,true])smc(owner,identity);}
assert.deepEqual(sourceHashes(),sourcesBefore);
// final source-derived census is frozen before root execution.
const expectedCounts = {contexts:8,modules:24,runs:532,retired:478,bit_sets:406,helper_calls:442,read32:16,store32:20,
  sources:64,flag_rows:64,indices:88,aliases:40,boundary:40,faults:72,repair_only:24,current_inputs:4,synthetic:48,smc:4,compiler_calls:8};
assert.deepEqual(counts,expectedCounts);assert.equal(diagnostics.length,3932);
const result={engine:{path:enginePath,sha256:engine.sha256,bytes:engine.bytes.length},source_pins:sourcesBefore,initial_arena_sha256:hash(initial),
  compiler:{text_hex:kernel.toString('hex'),text_sha256:hash(kernel),calls:counts.compiler_calls,combined_blocks:4,combined_instructions:17},
  counts,contexts,modules,observations,helper_trace:helperTrace,public_diagnostics:diagnostics,controls,
  limits:{blocks:8,instructions:64,module_bytes:65536,captures_per_context:6,mapped_pages:12},
  claim:'finite canonical private flat32 synchronous unshared exact F0 0FAB DWORD register-index BTS; no hardware LOCK, Windows concurrent ordering, performance or general compatibility claim',
  verification_ceiling:'per-run complete arena asserted;8 final and16 compiler milestone arenas persisted; selected32B frames/64B compiler data/32B stack/12B code and publicdiagnostics, no wholeRAM; nonforwarding synthetic helpers never commitRAM; explicit changed-input controls are separate from map/protect-only faultrepair'};
assert.deepEqual(Object.keys(result.engine),['path','sha256','bytes']);assert.equal(result.engine.sha256,hash(engine.bytes));
writeFileSync(join(output,'result.json'),JSON.stringify(result,null,2)+'\n',{flag:'wx'});console.log(JSON.stringify({output,counts,result:join(output,'result.json')}));
