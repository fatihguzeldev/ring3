import {attachDetectionApi, generatedImports} from '../../../../../../../support/wasm/binding.mjs';
import {ARENA_SIZE} from '../../../../../../../support/wasm/arena.mjs';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {execFileSync} from 'node:child_process';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../../../../../../../support/wasm/engine.mjs';

const [enginePath, output, root] = process.argv.slice(2); assert.ok(root && output);
const engine = readEngine(enginePath), SIZE = ARENA_SIZE, TRANSFER = 140;
const BASE = 0x500000, ENTRY = BASE + 0x1000, KERNEL = BASE + 0x2000, GATE = 0x8000, STACK = 0x70000, TOP = STACK + 4096;
const initial = readFileSync(join(output, 'initial-arena.bin')); assert.equal(initial.length, SIZE);
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const artifacts = {}, contexts = [], modules = [], observations = [], helperTrace = [], providers = [], lifecycle = [], diagnostics = [];
function save(name, bytes) { writeFileSync(join(output, name), bytes, {flag: 'wx'}); artifacts[name] = {bytes: Buffer.byteLength(bytes), sha256: hash(bytes)}; }
const fixture = join(root, 'engine/tests/windows/provider/process/current/fixtures/p2-current-process');
const kernel = readFileSync(join(fixture, 'kernel.text.bin')), raw = readFileSync(join(fixture, 'kernel.text.raw.bin'));
const relocations = JSON.parse(readFileSync(join(fixture, 'kernel.relocations.json')));
assert.equal(kernel.length, 67); assert.equal(raw.length, 67);
assert.equal(hash(raw), '957a7b5deafb448e973efd9b18f8235856f2c522ac0b7cb3da20aeb5278299c0');
assert.equal(hash(kernel), '372189564f9ae99ced2d328844f779200e2e2c7332d350372fc4e39571bda24d');
const fields = [[4, 0x3180], [17, 0x3184], [28, 0x3188], [36, 0x318c]];
assert.deepEqual(relocations.relocations.map(row => [row.source_offset, row.iat_rva]), fields);
for (let i = 0; i < raw.length; i++) if (!fields.some(([at]) => i >= at && i < at + 4)) assert.equal(kernel[i], raw[i]);
for (const [at, rva] of fields) { assert.equal(raw.readUInt32LE(at), 0); assert.equal(kernel.readUInt32LE(at), 0x400000 + rva); }
const caller = Buffer.from('f9fde8f90f000050ff15903140000f0b', 'hex'); assert.equal(caller.length, 16);
const gates = [[GATE, 0x10001], [GATE + 16, 0x10002], [GATE + 32, 0x10003], [GATE + 112, 0x10008], [GATE + 128, 0x10009]];
function authoredPe() {
  const bytes = Buffer.alloc(0x2800), put16 = (at, value) => bytes.writeUInt16LE(value, at), put32 = (at, value) => bytes.writeUInt32LE(value, at);
  bytes.write('MZ'); put32(0x3c, 0x80); bytes.write('PE\0\0', 0x80);
  put16(0x84, 0x14c); put16(0x86, 3); put16(0x94, 224); put16(0x96, 0x102); put16(0x98, 0x10b);
  for (const [field, value] of [[4,8192],[8,1536],[12,0],[16,0x1000],[20,0x1000],[24,0x3000],[28,0x400000],[32,4096],[36,512],[56,0x6000],[60,512],[72,0x100000],[76,4096],[80,0x100000],[84,4096],[92,16]]) put32(0x98 + field, value);
  put16(0x98 + 68, 3); put16(0x98 + 70, 0x100);
  for (const [index,rva,size] of [[1,0x3100,40],[5,0x5000,28],[12,0x3180,24]]) { put32(0x98 + 96 + index * 8, rva); put32(0x98 + 100 + index * 8, size); }
  [['.text',0x1043,0x1000,8192,0x200,0x60000020],['.data',1024,0x3000,1024,0x2200,0xc0000040],['.fixups',28,0x5000,512,0x2600,0x40000040]].forEach(([name,size,rva,rawSize,at,flags], index) => {
    const section = 0x178 + index * 40; bytes.write(name, section); for (const [field,value] of [[8,size],[12,rva],[16,rawSize],[20,at],[36,flags]]) put32(section + field,value);
  });
  bytes.fill(0xcc,0x200,0x2200); bytes.set(caller,0x200); bytes.set(kernel,0x1200); put32(0x2200,0xfacecafe); put32(0x2204,0x13579bdf);
  [0x3140,0,0,0x31b0,0x3180].forEach((value,i) => put32(0x2300 + i * 4,value));
  for (const at of [0x2340,0x2380]) [0x31c0,0x31e0,0x3200,0x3220,0x3240].forEach((value,i) => put32(at + i * 4,value));
  bytes.write('KeRnEl32.dLl',0x23b0);
  [[0x23c0,'GetCurrentProcess'],[0x23e0,'SetLastError'],[0x2400,'FlushInstructionCache'],[0x2420,'GetLastError'],[0x2440,'ExitProcess']].forEach(([at,name],i) => { put16(at,i + 1); bytes.write(name,at + 2); });
  put32(0x2600,0x1000); put32(0x2604,12); put16(0x2608,0x300a); put32(0x260c,0x2000); put32(0x2610,16);
  fields.forEach(([offset],i) => put16(0x2614 + i * 2,0x3000 | offset)); return bytes;
}
const image = readFileSync(join(output,'current-process.exe')); assert.deepEqual(image,authoredPe());
save('engine.wasm',engine.bytes); save('caller.bin',caller); save('kernel-linked.bin',kernel); save('kernel-original.bin',raw);
const sourcePaths = ['engine/tests/support/wasm/binding.mjs', ...execFileSync('rg',['--files','engine/src'],{cwd:root,encoding:'utf8'}).trim().split('\n'),
  'Cargo.toml','Cargo.lock','engine/Cargo.toml','rust-toolchain.toml','engine/tests/support/loader/pe32/image.rs',
  'engine/tests/windows/provider/process/current/wasm.rs', 'engine/tests/windows/provider/wasm.rs', 'engine/tests/support/wasm.rs','engine/tests/loader/imports/native/v8.rs', 'engine/tests/loader/imports/native.rs','engine/tests/support/wasm/engine.mjs',
  ...['kernel.c','build.py','check/coff.py','PROVENANCE.md','kernel.obj','kernel.text.bin','kernel.text.raw.bin','kernel.disasm.txt','kernel.relocations.json','receipt.json','runtime/run.mjs'].map(name => `engine/tests/windows/provider/process/current/fixtures/p2-current-process/${name}`)].sort();
assert.equal(new Set(sourcePaths).size,sourcePaths.length);
const sourceMap = () => Object.fromEntries(sourcePaths.map(path => { const bytes = readFileSync(join(root,path)); return [path,{bytes:bytes.length,sha256:hash(bytes)}]; }));
const sourcesBefore = sourceMap();
const words = values => { const bytes = Buffer.alloc(values.length * 4); values.forEach((value,i) => bytes.writeUInt32LE(value >>> 0,i * 4)); return bytes; };
function record(magic,length,values,version = 1) { const bytes = Buffer.alloc(length); bytes.write(magic); bytes.writeUInt16LE(version,4); bytes.writeUInt16LE(1,6); bytes.writeUInt32LE(length,8); bytes.set(words(values),16); return bytes; }
const stateBytes = state => record('R3ST',56,[...state.registers,state.pc,state.flags]);
const exitBytes = stop => record('R3EX',40,[stop.reason,stop.retired,stop.detail ?? 0,0,0,0],stop.version ?? 3);
const helperBytes = value => record('R3MH',40,[0,value,0,0,0,0]);
const cpuHelperBytes = value => record('R3MH',40,[0,value,0,0,0,4], 10);;
const installBytes = unit => record('R3IN',32,[unit.low,unit.high,unit.slot,0]);
const R = (eax = 0,ecx = 0,esi = 0,edi = 0,esp = TOP) => [eax,ecx,0,0,esp,0,esi,edi];
const S = (pc,eax = 0,esi = 0,edi = 0,esp = TOP,flags = 0x403) => ({registers:R(eax,0,esi,edi,esp),pc,flags});
const arena = e => Buffer.from(new Uint8Array(e.memory.buffer,e.base,SIZE));
function cpu(e) { const bytes = arena(e); return {registers:Array.from({length:8},(_,i) => bytes.readUInt32LE(16 + i * 4)),pc:bytes.readUInt32LE(48),flags:bytes.readUInt32LE(52)}; }
function exit(e) { const bytes = arena(e); return {version:bytes.readUInt16LE(60),reason:bytes.readUInt32LE(72),retired:bytes.readUInt32LE(76),detail:bytes.readUInt32LE(80)}; }
function unchanged(before,after,spans,label) { for (let i=0;i<SIZE;i++) if (!spans.some(([a,b]) => i>=a && i<b)) assert.equal(after[i],before[i],`${label}: arena byte ${i}`); }
function apiCall(e,name,args,spans = []) { const before = arena(e); assert.equal(e.api[name](...args),0,name); unchanged(before,arena(e),spans,name); }
function transfer(e,bytes) { assert.ok(bytes.length<=4096); new Uint8Array(e.memory.buffer).set(bytes,e.base + TRANSFER); }
const arities = {guard_detect: 6, guard_detect_resident: 7, finish_exception_detection: 12, open:3,close:0,arena_ptr:0,begin_image_input:1,append_image_input:2,load_pe32_linked_v8_input_at:2,start_loaded_image:2,compile_entries:2,compile_resident_entries:2,generation:0,module_ptr:0,module_len:0,guard:6,guard_resident:7,read32:1,store32:2,store_resident32:6,capture_call:5,capture_resident_call:6,complete_windows_call:4,complete_resident_windows_call:5,acknowledge_resident_installation:5,dispatcher_module:2,guard_dispatch_entry:5,find_installed_resident:3,discard_installed_resident:5, cpu_read32: 1, cpu_store32: 2, cpu_store_resident32: 6};
function fresh(owner) {
  const instance = new WebAssembly.Instance(engine.module,{}), api = attachDetectionApi(Object.fromEntries(Object.entries(arities).map(([name,arity]) => { const fn=instance.exports[`ring3_abi_v1_${name}`]; assert.equal(typeof fn,'function',name); assert.equal(fn.length,arity,name); return [name,fn]; })), instance.exports);
  const e={owner,label:owner,api,memory:instance.exports.memory,low:0x53900001 + contexts.length,high:0xa5390000,units:[],compilations:0,tokens:0,calls:0,retired:0,table:owner==='resident'?new WebAssembly.Table({element:'anyfunc',initial:8,maximum:8}):null};
  assert.equal(api.open(8,e.low,e.high),0); e.base=api.arena_ptr()>>>0; contexts.push(e);
  apiCall(e,'begin_image_input',[image.length]);
  for (let offset=0;offset<image.length;offset+=4096) { const part=image.subarray(offset,offset+4096); transfer(e,part); apiCall(e,'append_image_input',[offset,part.length]); }
  transfer(e,Buffer.alloc(4096,0xa5)); apiCall(e,'load_pe32_linked_v8_input_at',[BASE,GATE],[[TRANSFER,TRANSFER+112]]);
  assert.deepEqual(arena(e).subarray(TRANSFER,TRANSFER+112),record('R3LI',112,[BASE,0x6000,ENTRY,6,GATE,5,0,0,...gates.flat(),...Array(6).fill(0)],8));
  save(`${e.label}-v8-receipt.bin`,arena(e).subarray(TRANSFER,TRANSFER+112)); save(`${e.label}-loaded-arena.bin`,arena(e));
  apiCall(e,'start_loaded_image',[STACK,1],[[16,96]]); assert.deepEqual(cpu(e),S(ENTRY,0,0,0,TOP,2));
  const before=arena(e); new Uint8Array(e.memory.buffer).set(initial.subarray(4236),e.base+4236); unchanged(before,arena(e),[[4236,SIZE]],'dirty fp seed only');
  if(e.table) {
    apiCall(e,'dispatcher_module',[e.low,e.high],[[TRANSFER,TRANSFER+32]]); const receipt=arena(e).subarray(TRANSFER,TRANSFER+32);
    assert.equal(receipt.subarray(0,4).toString(),'R3DP'); assert.equal(receipt.readUInt32LE(4),0x10001); assert.equal(receipt.readUInt32LE(8),32); assert.deepEqual([receipt.readUInt32LE(16),receipt.readUInt32LE(20)],[e.low,e.high]);
    const bytes=Buffer.from(new Uint8Array(e.memory.buffer,receipt.readUInt32LE(24),receipt.readUInt32LE(28))),module=new WebAssembly.Module(bytes),file=`${e.label}-dispatcher.wasm`;
    assert.deepEqual(WebAssembly.Module.imports(module),[{module:'env',name:'memory',kind:'memory'},{module:'env',name:'table',kind:'table'},...['guard_dispatch_entry','find_installed_resident'].map(name=>({module:'ring3',name,kind:'function'}))]);
    e.dispatcher=new WebAssembly.Instance(module,{env:{memory:e.memory,table:e.table},ring3:e.api}).exports.run; assert.equal(e.dispatcher.length,4); save(file,bytes); modules.push({context:e.label,name:'dispatcher',file,bytes:bytes.length,sha256:hash(bytes)});
  }
  save(`${e.label}-started-arena.bin`,arena(e)); return e;
}
function compile(e,name,pcs,gate,instructions) {
  assert.ok(pcs.length<=3 && instructions+1<=64); assert.ok(++e.compilations<=5); transfer(e,words([...pcs,...gate]));
  apiCall(e,e.table?'compile_resident_entries':'compile_entries',[pcs.length,1],e.table?[[TRANSFER,TRANSFER+24]]:[]);
  const receipt=arena(e).subarray(TRANSFER,TRANSFER+24),unit={name,slot:0,entries:pcs,ordinary_instructions:instructions,instructions:instructions+1};
  if(e.table) { assert.equal(receipt.readUInt32LE(0),1); assert.equal(receipt.readUInt32LE(4),24); [unit.low,unit.high,unit.pointer,unit.length]=Array.from({length:4},(_,i)=>receipt.readUInt32LE(8+i*4)); }
  else { unit.generation=e.api.generation(); unit.pointer=e.api.module_ptr()>>>0; unit.length=e.api.module_len()>>>0; assert.equal(unit.generation,e.compilations); }
  assert.ok(unit.pointer>0 && unit.length>8 && unit.length<=65536 && unit.pointer+unit.length<=e.memory.buffer.byteLength);
  const bytes=Buffer.from(new Uint8Array(e.memory.buffer,unit.pointer,unit.length)),module=new WebAssembly.Module(bytes),guard=e.table?'guard_detect_resident':'guard_detect',store=e.table?'cpu_store_resident32':'cpu_store32';
  const imports=WebAssembly.Module.imports(module); assert.deepEqual(imports,generatedImports({resident: (guard) === 'guard_detect_resident', helpers: ['cpu_read32',store]})); assert.deepEqual(WebAssembly.Module.exports(module),[{name:'run',kind:'function'}]);
  const ring3={[guard]:e.api[guard]};
  for(const helper of ['cpu_read32',store]) ring3[helper]=(...args)=>{
    const step=e.steps.shift(); assert.ok(step,`unexpected ${helper}`); assert.equal(step[0],helper==='cpu_read32'?'read':'store'); assert.equal(args[helper==='cpu_store_resident32'?4:0]>>>0,step[1]);
    if(helper!=='cpu_read32') assert.equal(args[helper==='cpu_store_resident32'?5:1]>>>0,step[2]);
    if(helper==='cpu_store_resident32') assert.deepEqual(args.slice(0,4).map(x=>x>>>0),[e.low,e.high,unit.low,unit.high]);
    const before=arena(e); assert.equal(e.api[helper](...args),0); const after=arena(e); unchanged(before,after,[[100,140]],helper); assert.deepEqual(after.subarray(100,140),cpuHelperBytes(helper==='cpu_read32'?step[2]:0));
    helperTrace.push({context:e.label,call:e.calls+1,unit:name,name:helper,args:args.map(x=>x>>>0),status:0,helper_hex:after.subarray(100,140).toString('hex')}); return 0;
  };
  unit.run=new WebAssembly.Instance(module, {env: {memory: e.memory}, ring3: {...(ring3), finish_exception_detection: e.api.finish_exception_detection}}).exports.run; assert.equal(unit.run.length,4);
  if(e.table) { assert.equal(e.table.get(0),null); e.table.set(0,unit.run); apiCall(e,'acknowledge_resident_installation',[e.low,e.high,unit.low,unit.high,0],[[TRANSFER,TRANSFER+32]]); assert.deepEqual(arena(e).subarray(TRANSFER,TRANSFER+32),installBytes(unit)); }
  const file=`${e.label}-${e.compilations}-${name}.wasm`; save(file,bytes); e.units.push(unit); modules.push({context:e.label,name,file,generation:unit.generation??null,id_low:unit.low??null,id_high:unit.high??null,pointer:unit.pointer,slot:e.table?0:null,entries:pcs,instructions:unit.instructions,ordinary_instructions:instructions,bytes:bytes.length,sha256:hash(bytes),imports}); return unit;
}
function run(e,unit,label,budget,state,stop,steps=[]) {
  const before=arena(e),expected=Buffer.from(before); expected.set(stateBytes(state),0); expected.set(exitBytes(stop),56);
  if(steps.length) expected.set(cpuHelperBytes(steps.at(-1)[0]==='read'?steps.at(-1)[2]:0),100);
  if(e.table && budget && before.readUInt32LE(96)===0) expected.set(installBytes(unit),TRANSFER);
  e.steps=[...steps]; const status=(e.table?e.dispatcher:unit.run)(e.base,e.base+56,budget,e.base+96); assert.equal(status,0,label); assert.equal(e.steps.length,0,label); e.calls++; e.retired+=stop.retired;
  const actual=arena(e); assert.deepEqual(actual,expected,`${e.label}/${label}: full4612`); assert.deepEqual(cpu(e),state); assert.deepEqual(exit(e),{version:3,...stop,detail:stop.detail??0});
  const file=`${e.label}-${e.calls}-${label}-arena.bin`; save(file,actual); observations.push({context:e.label,name:label,budget,module:unit.name,expected:{status:0,version:3,...stop,...state,detail:stop.detail??0},observed:{status,...exit(e),...cpu(e)},arena:file,sha256:hash(actual)});
}
function complete(e,unit,id,args,returnPc,result,terminal=false) {
  const before=arena(e),state=cpu(e),token=++e.tokens,count=args.length;
  apiCall(e,e.table?'capture_resident_call':'capture_call',e.table?[e.low,e.high,unit.low,unit.high,2,count]:[e.low,e.high,unit.generation,2,count],[[TRANSFER,TRANSFER+112]]);
  assert.deepEqual(arena(e).subarray(TRANSFER,TRANSFER+112),record('R3CF',112,[token,id,2,count,state.pc,state.registers[4],returnPc,0,...args,...Array(16-count).fill(0)]));
  const captured=arena(e),expected=Buffer.from(captured),next={registers:[...state.registers],pc:returnPc,flags:state.flags};
  if(terminal) expected.set(exitBytes({version:4,reason:9,retired:0,detail:result}),56);
  else { next.registers[0]=result; next.registers[4]+=4*(count+1); expected.set(stateBytes(next),0); expected.set(exitBytes({reason:3,retired:0}),56); }
  apiCall(e,e.table?'complete_resident_windows_call':'complete_windows_call',e.table?[e.low,e.high,unit.low,unit.high,token]:[e.low,e.high,unit.generation,token],[[16,96]]); assert.deepEqual(arena(e),expected,'provider full4612');
  for(const [suffix,bytes] of [['before-capture',before],['captured',captured],['completed',arena(e)]]) save(`${e.label}-provider-${token}-${suffix}.bin`,bytes);
  providers.push({context:e.label,token,api:id,args,return_pc:returnPc,result,terminal,frame_hex:captured.subarray(TRANSFER,TRANSFER+112).toString('hex'),before_capture_sha256:hash(before),captured_sha256:hash(captured),completed_sha256:hash(arena(e)),observed:{...cpu(e),exit:exit(e)}});
  if(!terminal) { apiCall(e,e.table?'guard_resident':'guard',e.table?[e.low,e.high,unit.low,unit.high,e.base,e.base+56,e.base+96]:[e.low,e.high,unit.generation,e.base,e.base+56,e.base+96]); lifecycle.push({context:e.label,action:'provider-kept-current-owner',unit:unit.name,api:id,arena_sha256:hash(arena(e))}); }
}
function discard(e,unit) { if(!e.table)return; e.table.set(0,null); apiCall(e,'discard_installed_resident',[e.low,e.high,unit.low,unit.high,0]); lifecycle.push({context:e.label,action:'discard-consumed-caller',unit:unit.name}); }
function read(e,address,value) { const before=arena(e); assert.equal(e.api.read32(address),0); const after=arena(e); unchanged(before,after,[[100,140]],'diagnostic read'); assert.deepEqual(after.subarray(100,140),helperBytes(value)); diagnostics.push({context:e.label,address,value}); }
function exercise(e) {
  let unit=compile(e,'get-current-process',[ENTRY,KERNEL,GATE+128],[GATE+128,0x10009],6);
  const start=cpu(e); run(e,unit,'zero-budget',0,start,{version:e.table ? 3 : 6,reason:1,retired:0});
  new DataView(e.memory.buffer).setUint32(e.base+96,1,true); run(e,unit,'cancel-before-zero',0,start,{version:e.table ? 3 : 6,reason:2,retired:0}); new DataView(e.memory.buffer).setUint32(e.base+96,0,true);
  run(e,unit,'compiled-get-current-process',64,S(GATE+128,0,0,0,TOP-16),{version:6,reason:8,retired:6,detail:0x10009},[['store',TOP-4,ENTRY+7],['store',TOP-8,0],['store',TOP-12,0],['read',BASE+0x3180,GATE+128],['store',TOP-16,KERNEL+8]]);
  complete(e,unit,0x10009,[],KERNEL+8,0xffffffff); discard(e,unit);
  unit=compile(e,'set-nonzero-last-error',[KERNEL+8,GATE+16],[GATE+16,0x10002],3);
  run(e,unit,'set-last-error',64,S(GATE+16,0xffffffff,0xffffffff,0,TOP-20),{version:6,reason:8,retired:3,detail:0x10002},[['store',TOP-16,0x89abcdef],['read',BASE+0x3184,GATE+16],['store',TOP-20,KERNEL+21]]);
  complete(e,unit,0x10002,[0x89abcdef],KERNEL+21,0xffffffff); discard(e,unit);
  unit=compile(e,'flush-returned-current-handle',[KERNEL+21,GATE+112],[GATE+112,0x10008],4);
  run(e,unit,'flush-uses-saved-returned-handle',64,S(GATE+112,0xffffffff,0xffffffff,0,TOP-28),{version:6,reason:8,retired:4,detail:0x10008},[['store',TOP-16,0xffffffff],['store',TOP-20,0],['store',TOP-24,0xffffffff],['read',BASE+0x3188,GATE+112],['store',TOP-28,KERNEL+32]]);
  complete(e,unit,0x10008,[0xffffffff,0,0xffffffff],KERNEL+32,1); discard(e,unit);
  unit=compile(e,'observe-preserved-last-error',[KERNEL+32,GATE],[GATE,0x10001],2);
  run(e,unit,'get-last-error',64,S(GATE,1,0xffffffff,1,TOP-16),{version:6,reason:8,retired:2,detail:0x10001},[['read',BASE+0x318c,GATE],['store',TOP-16,KERNEL+40]]);
  complete(e,unit,0x10001,[],KERNEL+40,0x89abcdef); discard(e,unit);
  unit=compile(e,'compiled-check-and-natural-return',[KERNEL+40,ENTRY+7,GATE+32],[GATE+32,0x10003],13);
  run(e,unit,'natural-cdecl-return42',11,S(ENTRY+7,42,0,0,TOP,0x446),{version:6,reason:1,retired:11},[['read',TOP-12,0],['read',TOP-8,0],['read',TOP-4,ENTRY+7]]);
  run(e,unit,'caller-passes-return42-to-exit',64,S(GATE+32,42,0,0,TOP-8,0x446),{version:6,reason:8,retired:2,detail:0x10003},[['store',TOP-4,42],['read',BASE+0x3190,GATE+32],['store',TOP-8,ENTRY+14]]);
  complete(e,unit,0x10003,[42],ENTRY+14,42,true);
  for(const [address,value] of [[TOP-28,KERNEL+32],[TOP-24,0xffffffff],[TOP-20,0],[TOP-16,KERNEL+40],[TOP-12,0],[TOP-8,ENTRY+14],[TOP-4,42],
    [BASE+0x3180,GATE+128],[BASE+0x3184,GATE+16],[BASE+0x3188,GATE+112],[BASE+0x318c,GATE],[BASE+0x3190,GATE+32],[BASE+0x3000,0xfacecafe],[BASE+0x3004,0x13579bdf]]) read(e,address,value);
  e.final={cpu:cpu(e),exit:exit(e),arena_sha256:hash(arena(e))}; save(`${e.label}-final-arena.bin`,arena(e)); assert.equal(e.api.close(),0);
}
process.on('uncaughtException',error=>{ for(const e of contexts) save(`failure-${e.label}-arena.bin`,arena(e)); save('failure-first.json',JSON.stringify({message:error.message,stack:error.stack,actual:error.actual,expected:error.expected},null,2)); console.error(error); process.exitCode=1; });
exercise(fresh('replacement')); exercise(fresh('resident'));
const counts={contexts:contexts.length,modules:modules.length,generated_runs:contexts.reduce((n,e)=>n+e.calls,0),retired:contexts.reduce((n,e)=>n+e.retired,0),provider_completions:providers.length,guest_helpers:helperTrace.length,guest_reads:helperTrace.filter(row=>row.name==='cpu_read32').length,guest_stores:helperTrace.filter(row=>row.name!=='cpu_read32').length,diagnostic_reads:diagnostics.length};
assert.deepEqual(counts,{contexts:2,modules:11,generated_runs:16,retired:56,provider_completions:10,guest_helpers:42,guest_reads:16,guest_stores:26,diagnostic_reads:28});
const sourcesAfter=sourceMap(); assert.deepEqual(sourcesAfter,sourcesBefore);
save('result.json',JSON.stringify({status:'PASS',engine_sha256:engine.sha256,engine_bytes:engine.bytes.length,image_sha256:hash(image),command:process.argv,tools:{node:process.version,v8:process.versions.v8,platform:process.platform,arch:process.arch},counts,contexts:contexts.map(e=>({owner:e.owner,captures:e.compilations,peak_resident_units:e.table?1:null,generated_runs:e.calls,retired:e.retired,final:e.final})),modules,observations,providers,helper_trace:helperTrace,lifecycle,diagnostics,sources_before:sourcesBefore,sources_after:sourcesAfter,artifacts,claim_ceiling:'one original fixed-profile67-byte C caller, four exact external IAT DIR32 fields and authored relocated PE32; two private flat32 owner paths. Actual returned GetCurrentProcess handle is saved in ESI and consumed by named Flush(NULL,MAX); nonzero LastError survives, compiler checks naturally return42, caller exits42. Full4612 dirtyFP comparisons and saved chronological provider frames/arenas; selected stack/IAT/canaries only, no host guest-RAM mutation after startup, no CPU reseed. Five captures per owner, at most3 blocks/14instructions including gate per child, eight-slot resident table, default65536 module limit. No real OS/process handle, hardware cache, concurrency, general compiler/Windows or performance claim.'},null,2));
console.log(JSON.stringify({status:'PASS',...counts,output}));
