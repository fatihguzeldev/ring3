import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {mkdirSync, readFileSync, writeFileSync} from 'node:fs';
import {dirname, join} from 'node:path';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(root, 'expected copied engine, unique output and repository root');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const SIZE = 4236, TRANSFER = 140, LOW = 0xe0000000, HIGH = 0xd3456789;
const OUTER = 0x80000011, RETURN = 0x80000012, INNER = 0x80000013;
const engineBytes = readFileSync(enginePath);
assert.equal(engineBytes.length, 13500555);
assert.equal(hash(engineBytes), '51290d2dfae134401af049ff285a20d1b16521683944375d1438aaf495d5e2bc');
const arities = {
  open:3, close:0, arena_ptr:0, map:3, upload:2, generation:0,
  compile_resident_with_gates:2, compile_resident_callback_unit:7,
  acknowledge_resident_callback_installation:8, dispatcher_module:2,
  guard_dispatch_entry:5, guard_resident:7, find_resident:1, find_installed_resident:3,
  read32:1, write32:2, store_resident32:6, resident_module:2,
  acknowledge_resident_installation:5, capture_resident_call:6, complete_resident_call:6,
  begin_resident_callback:11, authorize_resident_callback:5, select_resident_callback_unit:7,
  capture_active_resident_callback_call:7, complete_active_resident_callback_call:7,
  finish_resident_callback:5, abort_callback:3, abandon_call:3, discard_unacknowledged_resident:4,
};
const productionPaths = ['Cargo.toml', 'Cargo.lock', 'engine/Cargo.toml',
  ...execFileSync('rg', ['--files', 'engine/src'], {cwd:root, encoding:'utf8'}).trim().split('\n')].sort();
assert.equal(productionPaths.length, 85); assert.equal(new Set(productionPaths).size, 85);
const sourcePaths = [...productionPaths, 'engine/tests/process_cold_callback_retry_wasm.rs',
  'engine/tests/fixtures/p2-cold-callback-retry/run.mjs'].sort();
assert.equal(sourcePaths.length, 87); assert.equal(new Set(sourcePaths).size, 87);
const sourceBytes = Object.fromEntries(sourcePaths.map(path => [path, readFileSync(join(root, path))]));
const beforeSources = Object.fromEntries(sourcePaths.map(path => [path, hash(sourceBytes[path])]));
const gitHead = execFileSync('git', ['rev-parse','HEAD'], {cwd:root,encoding:'utf8'}).trim();
const artifacts = {};
function save(path, value) {
  const bytes = Buffer.from(value); mkdirSync(dirname(join(output, path)), {recursive:true});
  writeFileSync(join(output, path), bytes); artifacts[path] = {sha256:hash(bytes), size:bytes.length};
  return path;
}
for (const path of sourcePaths) save(`sources/${path}`, sourceBytes[path]);
save('executed-engine.wasm', engineBytes);
for (const path of ['engine.wasm', 'engine-input-path.txt']) {
  const bytes = readFileSync(join(output, path)); artifacts[path] = {sha256:hash(bytes), size:bytes.length};
}
save('source-before.json', JSON.stringify(beforeSources, null, 2));

function words(size, magic, values, version = 1) {
  const bytes = new Uint8Array(size), view = new DataView(bytes.buffer);
  bytes.set(Buffer.from(magic)); view.setUint16(4, version, true);
  view.setUint16(6, 1, true); view.setUint32(8, size, true);
  values.forEach((value, index) => view.setUint32(16 + index * 4, value, true)); return bytes;
}
const cpu = (eip, esp, eax = 10) => ({registers:[eax,0x11111111,0x22222222,0x33333333,esp,0x55555555,0x66666666,0x77777777],eip,eflags:0xcd7});
const state56 = value => words(56, 'R3ST', [...value.registers,value.eip,value.eflags]);
const exit40 = (reason, retired = 0, detail = 0) => words(40, 'R3EX', [reason,retired,detail,0,0,0], 3);
const helper40 = (value = 0) => words(40, 'R3MH', [0,value,0,0,0,0]);
const installed32 = unit => words(32, 'R3IN', [unit.low,unit.high,unit.slot,0]);
const metadata24 = unit => {
  const bytes = new Uint8Array(24), view = new DataView(bytes.buffer);
  [1,24,unit.low,unit.high,unit.pointer,unit.length].forEach((value,index) => view.setUint32(index*4,value,true)); return bytes;
};
const inputs = [
  ['caller',0x3000,'e8fb0f00008d4003'], ['outer-gate',0x4000,'0f0b'],
  ['home',0x5000,'8d4003e9f80f0000'], ['return-gate',0x5100,'0f0b'],
  ['B',0x6000,'8d4005e8f80100008d4003c3'], ['inner-gate',0x6200,'0f0b'],
  ['keeper0',0x7000,'90'], ['keeper1',0x7010,'90'], ['keeper2',0x7020,'90'],
  ['keeper3',0x7030,'90'], ['ninth-pc',0x7040,'90'],
];
// the second prefix is a restart after neutral abort, not a resume of the abandoned callback.
const trace = [
  ['original caller CALL',cpu(0x4000,0x8ffc),exit40(8,1,OUTER),0,'outer',helper40()],
  ['first home prefix to missing B',cpu(0x6000,0x8ff8,13),exit40(3,2),0,'home',null],
  ['fresh home prefix to missing B',cpu(0x6000,0x8ff8,13),exit40(3,2),0,'home',null],
  ['fresh B CALL to inner Gate',cpu(0x6200,0x8ff4,18),exit40(8,2,INNER),0,'freshB',helper40()],
  ['fresh B RET to immutable home',cpu(0x5100,0x8ffc,47),exit40(3,2),12,'freshB',helper40(0x5100)],
  ['home return Gate',cpu(0x5100,0x8ffc,47),exit40(8,0,RETURN),0,'home',null],
  ['caller continuation once',cpu(0x3008,0x9000,50),exit40(3,1),0,'caller',null],
];
const oracle = {
  inputs, trace:trace.map(([label,state,exit,status,last]) => ({label,state,exit_hex:Buffer.from(exit).toString('hex'),status,last})),
  initial:cpu(0x3000,0x9000), initial_stack_words:[[0x8ff4,0x11223344],[0x8ff8,0x55667788],[0x8ffc,0x22334455]],
  tokens:{abandoned_outer:1,aborted_callback:2,fresh_outer:3,fresh_callback:4,inner:5},
  final_stack_words:[[0x8ff4,0xdeadbeef],[0x8ff8,0x5100],[0x8ffc,0xcafebabe]],
  frozen_returns:{inner:0x6008,outer:0x3005}, results:{inner:44,callback:47,caller:50},
  unit_identity:'opaque nonzero full-u64 IDs; fresh B must exceed disposed old B',
  stack_checkpoints:['quiescent-before-discard','after-discard','final'],
  engine_arities:arities, expected_source_counts:{production:85,total:87},
};
save('authored-oracle.json', JSON.stringify(oracle, null, 2));
for (const [label,,hex] of inputs) save(`inputs/${label}.x86`, Buffer.from(hex,'hex'));

const engineModule = new WebAssembly.Module(engineBytes);
assert.deepEqual(WebAssembly.Module.imports(engineModule), []);
assert.equal(WebAssembly.Module.exports(engineModule).length, 71);
assert.equal(WebAssembly.Module.exports(engineModule).filter(row => row.kind === 'function').length, 70);
const engine = new WebAssembly.Instance(engineModule, {});
const api = Object.fromEntries(Object.keys(arities).map(name => [name,engine.exports[`ring3_abi_v1_${name}`]]));
for (const [name,arity] of Object.entries(arities)) {
  assert.equal(typeof api[name], 'function', name); assert.equal(api[name].length, arity, name);
}
const memory = engine.exports.memory;
assert.equal(api.open(16,LOW,HIGH), 0);
const base = api.arena_ptr() >>> 0;
let bytes, view;
function refresh() { bytes = new Uint8Array(memory.buffer); view = new DataView(memory.buffer); }
refresh(); assert.ok(base > 0 && base + SIZE <= bytes.length);
const arena = () => { refresh(); return bytes.slice(base,base+SIZE); };
const observations = [], runs = [], stackObservations = [], units = {}, stableObservations = [];
const keeperBindingObservations = [], keeperMetadataObservations = [];
function record(label, before, expected, status, patches = [], kind = 'engine') {
  const after = arena(); assert.deepEqual(after, expected, `${label}: complete4236 arena`);
  const index = observations.length, stem = `arenas/${String(index).padStart(3,'0')}`;
  observations.push({index,label,kind,status,before:save(`${stem}-before.bin`,before),after:save(`${stem}-after.bin`,after),patches:patches.map(([offset,value]) => ({offset,hex:Buffer.from(value).toString('hex')}))});
}
function operation(label, action, status = 0, patches = [], kind = 'engine') {
  const before = arena(), expected = before.slice(); for (const [offset,value] of patches) expected.set(value,offset);
  assert.equal(action(), status, label); record(label,before,expected,status,patches,kind);
}
function descriptors(blocks, gates = []) {
  refresh(); [...blocks,...gates].forEach(([pc,value],index) => {
    view.setUint32(base+TRANSFER+index*8,pc,true); view.setUint32(base+TRANSFER+index*8+4,value,true);
  });
}
function compile(label, blocks, gates, helpers, slot, action) {
  descriptors(blocks,gates); const before = arena(); assert.equal(action(),0,`${label}: compile`); refresh();
  const [version,size,low,high,pointer,length] = Array.from({length:6},(_,index) => view.getUint32(base+TRANSFER+index*4,true));
  assert.equal(version,1); assert.equal(size,24);
  const id = BigInt(low) | (BigInt(high)<<32n); assert.notEqual(id,0n);
  assert.ok(Object.values(units).every(unit => unit.id !== id)); assert.ok(pointer > 0 && length > 8 && pointer+length <= bytes.length);
  const unit = {label,id,low,high,pointer,length,helpers,slot,entry_pc:blocks[0][0],discarded:false};
  const patch = metadata24(unit), expected = before.slice(); expected.set(patch,TRANSFER);
  record(`${label}: metadata24 only`,before,expected,0,[[TRANSFER,patch]]);
  unit.code = bytes.slice(pointer,pointer+length); unit.file = save(`modules/${label}.wasm`,unit.code);
  unit.module = new WebAssembly.Module(unit.code);
  assert.deepEqual(WebAssembly.Module.imports(unit.module), [{module:'env',name:'memory',kind:'memory'},
    ...['guard_resident',...helpers].map(name => ({module:'ring3',name,kind:'function'}))]);
  assert.deepEqual(WebAssembly.Module.exports(unit.module), [{name:'run',kind:'function'}]);
  units[label] = unit; return unit;
}
const table = new WebAssembly.Table({element:'anyfunc',initial:8,maximum:8});
function instantiate(unit, stage) {
  unit.instance = new WebAssembly.Instance(unit.module, {env:{memory},ring3:Object.fromEntries(['guard_resident',...unit.helpers].map(name => [name,api[name]]))});
  unit.run = unit.instance.exports.run; assert.equal(unit.run.length,4);
  if (stage) { table.set(unit.slot,unit.run); assert.equal(table.get(unit.slot),unit.run); }
}
operation('map all code before publication',()=>api.map(0x3000,5,7));
operation('map both stack pages before publication',()=>api.map(0x8000,2,3));
for (const [label,pc,hex] of inputs) {
  const input = Buffer.from(hex,'hex'); refresh(); bytes.set(input,base+TRANSFER);
  operation(`upload ${label} before publication`,()=>api.upload(pc,input.length));
}
for (const [address,value] of oracle.initial_stack_words) operation(`initial stack ${address.toString(16)}`,()=>api.write32(address,value),0,[[100,helper40()]]);
const dpBefore = arena(); assert.equal(api.dispatcher_module(LOW,HIGH),0); refresh();
const dp = view.getUint32(base+TRANSFER+24,true), dl = view.getUint32(base+TRANSFER+28,true);
assert.ok(dp > 0 && dl > 8 && dp+dl <= bytes.length);
const dpPatch = words(32,'R3DP',[LOW,HIGH,dp,dl]), dpExpected = dpBefore.slice(); dpExpected.set(dpPatch,TRANSFER);
record('dispatcher32 publication',dpBefore,dpExpected,0,[[TRANSFER,dpPatch]]);
const dispatcherBytes = bytes.slice(dp,dp+dl); save('modules/dispatcher.wasm',dispatcherBytes);
const dispatcherModule = new WebAssembly.Module(dispatcherBytes);
assert.deepEqual(WebAssembly.Module.imports(dispatcherModule), [{module:'env',name:'memory',kind:'memory'},{module:'env',name:'table',kind:'table'},
  {module:'ring3',name:'guard_dispatch_entry',kind:'function'},{module:'ring3',name:'find_installed_resident',kind:'function'}]);
const dispatcher = new WebAssembly.Instance(dispatcherModule,{env:{memory,table},ring3:{guard_dispatch_entry:api.guard_dispatch_entry,find_installed_resident:api.find_installed_resident}});
assert.equal(dispatcher.exports.run.length,4);
const keepers = [];
for (const [label,blocks,gates,helpers,slot] of [
  ['outer',[[0x4000,2]],[[0x4000,OUTER]],[],0], ['home',[[0x5000,8],[0x5100,2]],[[0x5100,RETURN]],[],1],
  ['caller',[[0x3000,5],[0x3005,3]],[],['store_resident32'],2],
  ...[0,1,2,3].map(index => [`keeper${index}`,[[0x7000+index*0x10,1]],[],[],3+index]),
]) {
  const unit = compile(label,blocks,gates,helpers,slot,()=>api.compile_resident_with_gates(blocks.length,gates.length));
  instantiate(unit,true); operation(`${label}: initial ack`,()=>api.acknowledge_resident_installation(LOW,HIGH,unit.low,unit.high,slot),0,[[TRANSFER,installed32(unit)]]); keepers.push(unit);
}
const {outer,home} = units;
function stable(label, bindingReads = [], metadataReads = []) {
  refresh(); for (const unit of keepers) {
    assert.equal(unit.discarded,false); assert.equal(table.get(unit.slot),unit.run);
    assert.deepEqual(bytes.slice(unit.pointer,unit.pointer+unit.length),unit.code,`${label}: ${unit.label} retained bytes`);
  }
  assert.deepEqual(bytes.slice(dp,dp+dl),dispatcherBytes); assert.equal(table.length,8);
  stableObservations.push({label,bindings:keepers.map(unit => ({label:unit.label,id:unit.id.toString(),pointer:unit.pointer,length:unit.length,slot:unit.slot})),dispatcher:{pointer:dp,length:dl},unused_slot7:table.get(7) === null,engine_binding_observation_indexes:bindingReads,engine_metadata_observation_indexes:metadataReads});
}
function keeperBindings(label) {
  const indexes = [];
  for (const unit of keepers) {
    operation(`${label}: engine binding ${unit.label}`,()=>api.find_installed_resident(LOW,HIGH,unit.entry_pc),0,[[TRANSFER,installed32(unit)]]);
    const index = observations.length-1; indexes.push(index);
    keeperBindingObservations.push({label,unit:unit.label,entry_pc:unit.entry_pc,id:unit.id.toString(),slot:unit.slot,observation:index});
  }
  return indexes;
}
function keeperMetadata() {
  const indexes = [];
  for (const unit of keepers) {
    operation(`after-discard: current module metadata ${unit.label}`,()=>api.resident_module(unit.low,unit.high),0,[[TRANSFER,metadata24(unit)]]);
    const index = observations.length-1; indexes.push(index);
    keeperMetadataObservations.push({unit:unit.label,id:unit.id.toString(),pointer:unit.pointer,length:unit.length,observation:index});
  }
  return indexes;
}
function dispatch(index,budget=9) {
  const [label,state,exit,status,last,helper] = trace[index], patches = [[0,state56(state)],[56,exit],[TRANSFER,installed32(units[last])]];
  if (helper) patches.push([100,helper]);
  operation(label,()=>dispatcher.exports.run(base,base+56,budget,base+96),status,patches,'generated');
  runs.push({index,label,budget,status,retired:new DataView(exit.buffer).getUint32(20,true),last,state,exit_hex:Buffer.from(exit).toString('hex'),observation:observations.length-1});
}
const discard = unit => api.discard_unacknowledged_resident(LOW,HIGH,unit.low,unit.high);
const select = (unit,token=4) => api.select_resident_callback_unit(LOW,HIGH,home.low,home.high,token,unit.low,unit.high);
const stackWords = new Map(oracle.initial_stack_words);
let read32Calls = 0;
function stackSnapshot(label) {
  const before = arena(), expectedArena = before.slice(), pages = [];
  for (const address of [0x8000,0x9000]) {
    const expected = new Uint8Array(4096), actual = new Uint8Array(4096), ev = new DataView(expected.buffer), av = new DataView(actual.buffer);
    for (const [pc,value] of stackWords) if (pc >= address && pc < address+4096) ev.setUint32(pc-address,value,true);
    for (let offset=0;offset<4096;offset+=4) {
      const value = ev.getUint32(offset,true); assert.equal(api.read32(address+offset),0); read32Calls++;
      expectedArena.set(helper40(value),100); assert.deepEqual(arena(),expectedArena,`${label}: Read32 full arena`);
      av.setUint32(offset,view.getUint32(base+120,true),true);
    }
    assert.deepEqual(actual,expected,`${label}: complete stack page`); pages.push({address,path:save(`stack/${label}-${address.toString(16)}.bin`,actual)});
  }
  record(`stack ${label}`,before,expectedArena,0,[[100,helper40()]],'stack-observation');
  stackObservations.push({label,pages,observation:observations.length-1});
}
stable('seven initial keepers');
operation('only host CPU seed',()=>{ refresh(); bytes.set(state56(oracle.initial),base); bytes.set(exit40(3),base+56); return 0; },0,[[0,state56(oracle.initial)],[56,exit40(3)]],'initial-host-seed');
dispatch(0); stackWords.set(0x8ffc,0x3005);
operation('original outer capture1',()=>api.capture_resident_call(LOW,HIGH,outer.low,outer.high,1,0),0,[[TRANSFER,words(112,'R3CF',[1,OUTER,1,0,0x4000,0x8ffc,0x3005,0])]]);
function begin(outerToken,callbackToken) {
  operation(`callback begin${callbackToken}`,()=>api.begin_resident_callback(LOW,HIGH,outer.low,outer.high,home.low,home.high,outerToken,0x5000,0x5100,RETURN,0),0,
    [[0,state56(cpu(0x5000,0x8ff8))],[56,exit40(3)],[TRANSFER,words(72,'R3RC',[callbackToken,outerToken,1,0,0x5000,0x8ff8,0x5100,RETURN,0,0,outer.low,outer.high,home.low,home.high])]]);
  stackWords.set(0x8ff8,0x5100); operation(`callback authorize${callbackToken}`,()=>api.authorize_resident_callback(LOW,HIGH,home.low,home.high,callbackToken));
}
begin(1,2); dispatch(1);
operation('genuinely missing B at first prefix stop',()=>api.find_resident(0x6000),17);
const Bblocks = [[0x6000,8],[0x6008,4],[0x6200,2]], Bgates = [[0x6200,INNER]], Bhelpers = ['read32','store_resident32'];
const coldCompile = token => api.compile_resident_callback_unit(LOW,HIGH,home.low,home.high,token,3,1);
const oldB = compile('oldB',Bblocks,Bgates,Bhelpers,7,()=>coldCompile(2));
const failedBefore = arena(); assert.throws(()=>new WebAssembly.Instance(oldB.module,{env:{memory},ring3:{}}),WebAssembly.LinkError);
record('genuine host LinkError has no engine effect',failedBefore,failedBefore,'LinkError',[],'host-install-failure');
assert.equal(table.get(7),null); operation('failed installation grants no selection',()=>select(oldB,2),17);
instantiate(oldB,false); assert.equal(table.get(7),null);
operation('disposal Busy during callback',()=>discard(oldB),12);
operation('neutral callback abort2 restores original outer',()=>api.abort_callback(LOW,HIGH,2),0,[[0,state56(cpu(0x4000,0x8ffc))],[56,exit40(8,1,OUTER)]]);
operation('disposal Busy with restored outer pending',()=>discard(oldB),12);
operation('abandon original outer1',()=>api.abandon_call(LOW,HIGH,1));
descriptors([[0x7040,1]]); operation('distinct ninth PC only after quiescence',()=>api.compile_resident_with_gates(1,0),18);
stackSnapshot('quiescent-before-discard');
stable('before current disposal',keeperBindings('before-discard'));
operation('discard exact current orphan',()=>discard(oldB)); oldB.discarded = true;
const afterBindings = keeperBindings('after-discard'), afterMetadata = keeperMetadata();
stackSnapshot('after-discard'); stable('after current disposal',afterBindings,afterMetadata);
operation('repeat old discard',()=>discard(oldB),3);
operation('old metadata getter',()=>api.resident_module(oldB.low,oldB.high),3);
for (const cancel of [0,1]) {
  refresh(); view.setUint32(base+96,cancel,true);
  for (const budget of [0,9]) for (const badPointers of [false,true]) {
    operation(`old copied child guard cancel${cancel} budget${budget} bad${badPointers}`,()=>oldB.run(badPointers?0:base,badPointers?0:base+56,budget,badPointers?0:base+96),3,[],'generated-old-child');
  }
}
refresh(); view.setUint32(base+96,0,true);
operation('fresh same-Gate outer capture3',()=>api.capture_resident_call(LOW,HIGH,outer.low,outer.high,1,0),0,[[TRANSFER,words(112,'R3CF',[3,OUTER,1,0,0x4000,0x8ffc,0x3005,0])]]);
operation('abandoned outer1 replay',()=>api.complete_resident_call(LOW,HIGH,outer.low,outer.high,1,47),14);
begin(3,4); operation('aborted callback2 replay',()=>api.abort_callback(LOW,HIGH,2),14); dispatch(2);
operation('old cold callback2 replay',()=>coldCompile(2),14);
const freshB = compile('freshB',Bblocks,Bgates,Bhelpers,7,()=>coldCompile(4)); assert.ok(freshB.id > oldB.id);
instantiate(freshB,true);
operation('fresh B callback ack slot7',()=>api.acknowledge_resident_callback_installation(LOW,HIGH,home.low,home.high,4,freshB.low,freshB.high,7),0,[[TRANSFER,installed32(freshB)]]);
operation('explicit fresh B selection',()=>select(freshB)); dispatch(3); stackWords.set(0x8ff4,0x6008);
operation('real inner capture5',()=>api.capture_active_resident_callback_call(LOW,HIGH,freshB.low,freshB.high,4,1,0),0,[[TRANSFER,words(112,'R3CF',[5,INNER,1,0,0x6200,0x8ff4,0x6008,0])]]);
for (const [address,value] of [[0x8ff4,0xdeadbeef],[0x8ffc,0xcafebabe]]) {
  operation(`post-capture frozen RAM ${address.toString(16)}`,()=>api.write32(address,value),0,[[100,helper40()]]); stackWords.set(address,value);
}
operation('frozen inner completion5 returns44',()=>api.complete_active_resident_callback_call(LOW,HIGH,freshB.low,freshB.high,4,5,44),0,[[0,state56(cpu(0x6008,0x8ff8,44))],[56,exit40(3)]]);
dispatch(4); operation('explicit immutable home selection',()=>select(home)); dispatch(5,1);
operation('finish fresh callback4 returns47',()=>api.finish_resident_callback(LOW,HIGH,home.low,home.high,4),0,
  [[0,state56(cpu(0x4000,0x8ffc))],[56,exit40(8,1,OUTER)],[TRANSFER,words(48,'R3RR',[4,3,47,0,outer.low,outer.high,home.low,home.high])]]);
operation('fresh outer completion3 uses frozen return',()=>api.complete_resident_call(LOW,HIGH,outer.low,outer.high,3,47),0,[[0,state56(cpu(0x3005,0x9000,47))],[56,exit40(3)]]);
dispatch(6); stackSnapshot('final'); stable('final seven keepers');
refresh(); assert.deepEqual(bytes.slice(freshB.pointer,freshB.pointer+freshB.length),freshB.code); assert.equal(table.get(7),freshB.run); assert.equal(api.generation(),0);
operation('close once',()=>api.close());
for (const path of sourcePaths) assert.deepEqual(readFileSync(join(root,path)),sourceBytes[path],`source unchanged: ${path}`);
assert.deepEqual(readFileSync(enginePath),engineBytes);
const afterSources = Object.fromEntries(sourcePaths.map(path=>[path,hash(readFileSync(join(root,path)))])); assert.deepEqual(afterSources,beforeSources);
save('source-after.json',JSON.stringify(afterSources,null,2));
const counts = {
  engine_instances:1, resident_modules:Object.keys(units).length, dispatcher_modules:1,
  successful_unit_instantiations:9, failed_host_instantiations:1, initial_installed_keepers:keepers.length,
  successful_acks:8, peak_resident_units:8, capacity_refusals:1, successful_discards:1,
  old_child_refusals:observations.filter(row=>row.kind === 'generated-old-child').length,
  dispatcher_calls:runs.length, guest_retired:runs.reduce((n,row)=>n+row.retired,0),
  stack_checkpoints:stackObservations.length, whole_stack_pages:stackObservations.reduce((n,row)=>n+row.pages.length,0), read32_calls:read32Calls,
  source_files:sourcePaths.length, production_source_files:productionPaths.length,
  observations:observations.length, stable_keeper_observations:stableObservations.length,
  keeper_binding_readbacks:keeperBindingObservations.length, keeper_metadata_readbacks:keeperMetadataObservations.length,
};
assert.deepEqual(counts,{engine_instances:1,resident_modules:9,dispatcher_modules:1,successful_unit_instantiations:9,failed_host_instantiations:1,initial_installed_keepers:7,successful_acks:8,peak_resident_units:8,capacity_refusals:1,successful_discards:1,old_child_refusals:8,dispatcher_calls:7,guest_retired:10,stack_checkpoints:3,whole_stack_pages:6,read32_calls:6144,source_files:87,production_source_files:85,observations:103,stable_keeper_observations:4,keeper_binding_readbacks:14,keeper_metadata_readbacks:7});
const result = {fixture:'cold-callback-retry',counts,oracle,git_head:gitHead,engine_sha256:hash(engineBytes),sources:beforeSources,source_after:afterSources,
  observations,runs,stack_observations:stackObservations,keeper_observations:stableObservations,keeper_binding_readbacks:keeperBindingObservations,keeper_metadata_readbacks:keeperMetadataObservations,
  units:Object.values(units).map(unit=>({label:unit.label,id:unit.id.toString(),low:unit.low,high:unit.high,pointer:unit.pointer,length:unit.length,slot:unit.slot,entry_pc:unit.entry_pc,discarded:unit.discarded,file:unit.file,helpers:unit.helpers})),
  artifacts,limits:'One authored synchronous trusted-host flat32 failed installation and fresh retry. Original CALL executes once; home prefix executes twice; recapture uses the unchanged engine-restored Gate. Complete per-operation4236 arenas and three two-page stack checkpoints are saved. Disposed borrowed pointer expires and is never reread. Retained instantiated old child proves baked-ID guard refusal, not absence of foreign references. No RAM rollback, automatic policy, PE/provider/SDK/browser/race/GC/game/performance/fullP2 claim.'};
save('result.json',JSON.stringify(result,null,2));
writeFileSync(join(output,'artifact-manifest.json'),JSON.stringify(artifacts,null,2));
console.log(JSON.stringify({output,result:join(output,'result.json'),result_sha256:artifacts['result.json'].sha256,engine_sha256:hash(engineBytes),counts,manifest_sha256:hash(readFileSync(join(output,'artifact-manifest.json')))}));
