import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(root, 'expected copied actual engine, output directory and repository root');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const SIZE = 4236, TRANSFER = 140, CODE = 0x1000, DATA = 0x4000, VICTIM = CODE + 0x10;
const KEY_LOW = 1, KEY_HIGH = 0xc3790000;
const arities = {open:3, close:0, arena_ptr:0, map:3, protect:3, upload:2, read32:1, compile_resident_entries:2, resident_module:2, guard_resident:7, dispatcher_module:2, guard_dispatch_entry:5, find_installed_resident:3, acknowledge_resident_installation:5, retire_stale_resident:4, discard_unacknowledged_resident:4};
const sourcePaths = [...execFileSync('rg', ['--files', 'engine/src'], {cwd:root, encoding:'utf8'}).trim().split('\n').sort(), 'Cargo.toml', 'Cargo.lock', 'engine/Cargo.toml', 'engine/tests/process_unacknowledged_disposal_wasm.rs', 'engine/tests/fixtures/p2-unacknowledged-disposal/run.mjs'];
assert.equal(sourcePaths.length, 87); assert.equal(new Set(sourcePaths).size, 87);
const sourceHashes = () => Object.fromEntries(sourcePaths.map(path => [path, hash(readFileSync(join(root, path)))]));
const beforeSources = sourceHashes(), engineBytes = readFileSync(enginePath), engineModule = new WebAssembly.Module(engineBytes);
assert.deepEqual(WebAssembly.Module.imports(engineModule), []);
assert.equal(WebAssembly.Module.exports(engineModule).length, 71);
assert.equal(WebAssembly.Module.exports(engineModule).filter(row => row.kind === 'function').length, 70);
writeFileSync(join(output, 'source-before.json'), JSON.stringify(beforeSources, null, 2));
writeFileSync(join(output, 'run.mjs'), readFileSync(join(root, 'engine/tests/fixtures/p2-unacknowledged-disposal/run.mjs')));
const instance = new WebAssembly.Instance(engineModule, {});
const api = Object.fromEntries(Object.entries(arities).map(([name, arity]) => {
  const fn = instance.exports[`ring3_abi_v1_${name}`]; assert.equal(typeof fn, 'function', name); assert.equal(fn.length, arity, name); return [name, fn];
}));
assert.equal(api.open(8, KEY_LOW, KEY_HIGH), 0);
const ctx = {memory:instance.exports.memory, base:api.arena_ptr()>>>0, table:new WebAssembly.Table({element:'anyfunc', initial:8, maximum:8})};
assert.ok(ctx.base>0);
const modules = [], controls = [], runs = [], observations = [], cycles = [], oldFunctions = [], live = new Set();
let nextId = 0n, read32Calls = 0, guestRetired = 0;
function refresh() {ctx.bytes = new Uint8Array(ctx.memory.buffer); ctx.view = new DataView(ctx.memory.buffer); return ctx;}
function arena() {return refresh().bytes.slice(ctx.base, ctx.base+SIZE);}
function words(values) {const bytes = Buffer.alloc(values.length*4); values.forEach((value,index) => bytes.writeUInt32LE(value>>>0,index*4)); return bytes;}
function record(magic, length, fields, version=1) {const bytes = Buffer.alloc(length); bytes.write(magic); bytes.writeUInt16LE(version,4); bytes.writeUInt16LE(1,6); bytes.writeUInt32LE(length,8); bytes.set(words(fields),16); return bytes;}
function state(registers, pc, flags) {assert.equal(flags&2,2); assert.equal((flags&~0xcd7)>>>0,0); return record('R3ST',56,[...registers,pc,flags]);}
const exit = (reason, retired, version) => record('R3EX',40,[reason,retired,0,0,0,0],version);
const installed = (unit, slot) => record('R3IN',32,[unit.low,unit.high,slot,0]);
function request(bytes) {assert.ok(bytes.length<=4096); refresh().bytes.set(bytes,ctx.base+TRANSFER);}
function unchanged(action, status, label) {const before = arena(); assert.equal(action(),status,label); assert.deepEqual(arena(),before,`${label}: full arena unchanged`);}
function control(name, action, status, extra={}) {
  unchanged(action,status,name); const file = `control-${controls.length+1}-arena.bin`, bytes = arena(); writeFileSync(join(output,file),bytes);
  controls.push({name,status,arena:file,arena_sha256:hash(bytes),...extra});
}
function receipt(action, bytes, label) {const before = arena(); assert.equal(action(),0,label); const expected = before.slice(); expected.set(bytes,TRANSFER); assert.deepEqual(arena(),expected,`${label}: only exact receipt changes`);}
function uploadPage(address, page, permissions) {
  unchanged(()=>api.map(address,1,7),0,'map page'); request(page); unchanged(()=>api.upload(address,page.length),0,'upload page'); unchanged(()=>api.protect(address,1,permissions),0,'protect page');
}

// every code block and the data page exist before the first resident publication.
const codePage = Buffer.alloc(4096,0xcc), dataPage = Buffer.alloc(4096,0x6d);
codePage.set(Buffer.from('b800003412e906000000','hex'),0);
codePage.set(Buffer.from('40ebfd','hex'),0x10);
for (let index=2; index<=8; index++) codePage.set(Buffer.concat([Buffer.from([0xb8]),words([0x81000000+index]),Buffer.from([0xeb,0])]),index*0x10);
uploadPage(CODE,codePage,5); uploadPage(DATA,dataPage,3);
writeFileSync(join(output,'code-page-first.bin'),codePage); writeFileSync(join(output,'data-page-first.bin'),dataPage);
function readPages(label) {
  const pages = [];
  for (const [address, expected] of [[CODE,codePage],[DATA,dataPage]]) {
    const before = arena(), observed = Buffer.alloc(4096); let helper;
    for (let offset=0; offset<4096; offset+=4) {
      assert.equal(api.read32(address+offset),0); read32Calls++; refresh(); const value = expected.readUInt32LE(offset);
      helper = record('R3MH',40,[0,value,0,0,0,0]); assert.deepEqual(ctx.bytes.slice(ctx.base+100,ctx.base+140),new Uint8Array(helper)); observed.writeUInt32LE(ctx.view.getUint32(ctx.base+120,true),offset);
    }
    const after = before.slice(); after.set(helper,100); assert.deepEqual(arena(),after); assert.deepEqual(observed,expected,`${label}: whole declared page`);
    const file = `${label}-${address.toString(16)}-page.bin`; writeFileSync(join(output,file),observed); pages.push({address,bytes:4096,file,sha256:hash(observed)});
  }
  observations.push({label,pages});
}

// this bounded structural reader checks complete function types and baked guard prefixes.
function shape(bytes, dispatcher, unit) {
  let at=8; const types=[], imports=[], functions=[], exports=[]; let locals,guard;
  assert.deepEqual([...bytes.subarray(0,8)],[0,97,115,109,1,0,0,0]);
  const u=()=>{let value=0,shift=0; for(let count=0;count<5;count++){assert.ok(at<bytes.length);const byte=bytes[at++];value|=(byte&127)<<shift;if(!(byte&128))return value>>>0;shift+=7;}assert.fail('bounded unsigned LEB');};
  const signed=()=>{let value=0n,shift=0n,byte;do{assert.ok(at<bytes.length&&shift<35n);byte=bytes[at++];value|=BigInt(byte&127)<<shift;shift+=7n;}while(byte&128);if(byte&64)value-=1n<<shift;return Number(BigInt.asUintN(32,value));};
  const name=()=>{const length=u(),end=at+length;assert.ok(end<=bytes.length);const value=Buffer.from(bytes.subarray(at,end)).toString();at=end;return value;};
  while(at<bytes.length) {
    const section=bytes[at++],length=u(),end=at+length;assert.ok(end<=bytes.length);
    if(section===1) for(let n=u();n>0;n--){assert.equal(bytes[at++],0x60);types.push({parameters:Array.from({length:u()},()=>bytes[at++]),results:Array.from({length:u()},()=>bytes[at++])});}
    else if(section===2) for(let n=u();n>0;n--){const module=name(),field=name(),kind=bytes[at++];if(kind===0)imports.push({module,name:field,kind:'function',type:u()});else if(kind===2){assert.equal(u(),0);assert.equal(u(),1);imports.push({module,name:field,kind:'memory',minimum:1});}else{assert.ok(dispatcher);assert.equal(kind,1);assert.equal(bytes[at++],0x70);assert.equal(u(),1);assert.equal(u(),8);assert.equal(u(),8);imports.push({module,name:field,kind:'table',minimum:8,maximum:8});}}
    else if(section===3) for(let n=u();n>0;n--)functions.push(u());
    else if(section===7) for(let n=u();n>0;n--)exports.push({name:name(),kind:bytes[at++],index:u()});
    else if(section===10) {
      assert.equal(u(),1);const bodyLength=u();assert.equal(at+bodyLength,end);locals=Array.from({length:u()},()=>[u(),bytes[at++]]);
      const constants=dispatcher?[KEY_LOW,KEY_HIGH]:[KEY_LOW,KEY_HIGH,unit.low,unit.high];
      for(const value of constants){assert.equal(bytes[at++],0x41);assert.equal(signed(),value);}
      for(const value of [0,1,3]){assert.equal(bytes[at++],0x20);assert.equal(u(),value);}assert.equal(bytes[at++],0x10);assert.equal(u(),0);guard={constants,parameters:[0,1,3],function:0};at=end;
    } else assert.fail(`unexpected generated section ${section}`);
    assert.equal(at,end);
  }
  const signature=n=>({parameters:Array(n).fill(127),results:[127]});
  assert.deepEqual(types,dispatcher?[signature(4),signature(5),signature(3)]:[signature(4),signature(7)]);
  assert.deepEqual(functions,[0]); assert.deepEqual(exports,[{name:'run',kind:0,index:dispatcher?2:1}]);
  assert.deepEqual(imports,dispatcher?[{module:'env',name:'memory',kind:'memory',minimum:1},{module:'env',name:'table',kind:'table',minimum:8,maximum:8},{module:'ring3',name:'guard_dispatch_entry',kind:'function',type:1},{module:'ring3',name:'find_installed_resident',kind:'function',type:2}]:[{module:'env',name:'memory',kind:'memory',minimum:1},{module:'ring3',name:'guard_resident',kind:'function',type:1}]);
  assert.deepEqual(locals,dispatcher?[[5,127]]:[[16,127],[1,126]]); assert.ok(guard);
  return {types,imports,exports,locals,guard};
}
function saveModule(bytes, label, dispatcher, binding={}) {
  const structure=shape(bytes,dispatcher,binding), module=new WebAssembly.Module(bytes);
  assert.deepEqual(WebAssembly.Module.exports(module),[{name:'run',kind:'function'}]);
  const child=new WebAssembly.Instance(module,{env:{memory:ctx.memory,table:ctx.table},ring3:api}); assert.equal(child.exports.run.length,4);
  const file=`${label}.wasm`;writeFileSync(join(output,file),bytes);modules.push({label,file,sha256:hash(bytes),...binding,...structure});return {...binding,bytes,run:child.exports.run,label};
}
function compile(entries,label) {
  assert.ok(entries.length<=8);request(words(entries));const before=arena();assert.equal(api.compile_resident_entries(entries.length,0),0);refresh();
  const fields=Array.from({length:6},(_,index)=>ctx.view.getUint32(ctx.base+TRANSFER+index*4,true));assert.deepEqual(fields.slice(0,2),[1,24]);
  const [, ,low,high,pointer,length]=fields,id=BigInt(low)|(BigInt(high)<<32n);assert.ok(id>nextId);nextId=id;
  assert.ok(pointer>0&&length>8&&length<=65536&&pointer+length<=ctx.bytes.length);const expected=before.slice();expected.set(words(fields),TRANSFER);assert.deepEqual(arena(),expected);
  const unit=saveModule(ctx.bytes.slice(pointer,pointer+length),label,false,{low,high,id:id.toString(),pointer,length,entries});live.add(unit.id);assert.ok(live.size<=8);return unit;
}
function acknowledge(unit,slot) {
  ctx.table.set(slot,unit.run);receipt(()=>api.acknowledge_resident_installation(KEY_LOW,KEY_HIGH,unit.low,unit.high,slot),installed(unit,slot),'acknowledge exact table slot');assert.equal(ctx.table.get(slot),unit.run);
}
function dispatcher() {
  const before=arena();assert.equal(api.dispatcher_module(KEY_LOW,KEY_HIGH),0);refresh();const pointer=ctx.view.getUint32(ctx.base+TRANSFER+24,true),length=ctx.view.getUint32(ctx.base+TRANSFER+28,true);
  assert.ok(pointer>0&&length>8&&pointer+length<=ctx.bytes.length);const expected=before.slice();expected.set(record('R3DP',32,[KEY_LOW,KEY_HIGH,pointer,length]),TRANSFER);assert.deepEqual(arena(),expected);
  const bytes=ctx.bytes.slice(pointer,pointer+length);
  if(ctx.dispatcher){assert.equal(pointer,ctx.dispatcher.pointer);assert.equal(length,ctx.dispatcher.length);assert.deepEqual(bytes,ctx.dispatcher.bytes);}else ctx.dispatcher=saveModule(bytes,'dispatcher',true,{pointer,length});
}
function stable(unit) {
  unchanged(()=>api.guard_resident(KEY_LOW,KEY_HIGH,unit.low,unit.high,ctx.base,ctx.base+56,ctx.base+96),0,'unrelated unit current');
  receipt(()=>api.resident_module(unit.low,unit.high),words([1,24,unit.low,unit.high,unit.pointer,unit.length]),'unrelated unit pointer stable');refresh();assert.deepEqual(ctx.bytes.slice(unit.pointer,unit.pointer+unit.length),unit.bytes);
}
function keeperStable(keeper,others) {
  assert.equal(ctx.table.get(0),keeper.run);for(const unit of [keeper,...others])stable(unit);
  receipt(()=>api.find_installed_resident(KEY_LOW,KEY_HIGH,CODE),installed(keeper,0),'keeper installation stable');dispatcher();
}
const discard=unit=>api.discard_unacknowledged_resident(KEY_LOW,KEY_HIGH,unit.low,unit.high);
readPages('prepublication');
const keeper=compile([CODE],'keeper');acknowledge(keeper,0);
let victim=compile([VICTIM,VICTIM+1],'victim-first');const others=[];
for(let index=2;index<8;index++)others.push(compile([CODE+index*0x10],`other-${index}`));
assert.equal(live.size,8);dispatcher();keeperStable(keeper,others);
request(words([CODE+0x80]));control('ninth-current-distinct-unit-capacity',()=>api.compile_resident_entries(1,0),18);
control('wrong-low-key',()=>api.discard_unacknowledged_resident(KEY_LOW^1,KEY_HIGH,victim.low,victim.high),3);
control('wrong-high-key',()=>api.discard_unacknowledged_resident(KEY_LOW,KEY_HIGH^1,victim.low,victim.high),3);
control('zero-id',()=>api.discard_unacknowledged_resident(KEY_LOW,KEY_HIGH,0,0),3);
control('unknown-full-id',()=>api.discard_unacknowledged_resident(KEY_LOW,KEY_HIGH,0xffffffff,0xffffffff),3);
control('installed-keeper',()=>discard(keeper),7);
control('legacy-current-retirement',()=>api.retire_stale_resident(KEY_LOW,KEY_HIGH,victim.low,victim.high),7);
refresh().view.setUint32(ctx.base+96,1,true);
control('installed-before-cancel',()=>discard(keeper),7);
control('unknown-before-cancel',()=>api.discard_unacknowledged_resident(KEY_LOW,KEY_HIGH,0,0),3);
control('current-unacknowledged-cancel',()=>discard(victim),16);
refresh().view.setUint32(ctx.base+96,0,true);keeperStable(keeper,others);readPages('capacity-full');

const initial=[0,0x23456789,0x3456789a,0x456789ab,0x70000,0x6789abcd,0x789abcde,0x89abcdef];
refresh().bytes.set(state(initial,CODE,2),ctx.base);ctx.bytes.set(exit(1,0,1),ctx.base+56);
function guest(name,unit,budget,registers,pc,flags,reason,retired,version,installation=null) {
  const expected=arena();expected.set(state(registers,pc,flags));expected.set(exit(reason,retired,version),56);if(installation)expected.set(installed(...installation),TRANSFER);
  assert.equal(unit.run(ctx.base,ctx.base+56,budget,ctx.base+96),0);const observed=arena();assert.deepEqual(observed,expected,`${name}: complete literal arena`);
  const file=`${name}-arena.bin`;writeFileSync(join(output,file),observed);runs.push({name,module:unit.label,budget,status:0,registers:[...registers],pc,flags,reason,retired,version,arena:file,arena_sha256:hash(observed)});guestRetired+=retired;
}
const afterKeeper=[...initial];afterKeeper[0]=0x12340000;
guest('dispatcher-reaches-unacknowledged-victim',ctx.dispatcher,64,afterKeeper,VICTIM,2,3,2,3,[keeper,0]);
for(let cycle=1;cycle<=3;cycle++) {
  const old=victim, registers=[...afterKeeper];registers[0]=cycle===1?0x12340001:cycle===2?0x12340001:0x12340002;const pc=cycle===2?VICTIM:VICTIM+1;
  guest(`cycle-${cycle}-guest-slice`,old,1,registers,pc,2,1,1,1);
  // generated execution has returned. no host table contains this unacknowledged unit.
  control(`cycle-${cycle}-discard`,()=>discard(old),0,{id:old.id});live.delete(old.id);assert.equal(live.size,7);oldFunctions.push(old);
  control(`cycle-${cycle}-repeat`,()=>discard(old),3);
  control(`cycle-${cycle}-old-getter`,()=>api.resident_module(old.low,old.high),3);
  control(`cycle-${cycle}-old-guard`,()=>api.guard_resident(KEY_LOW,KEY_HIGH,old.low,old.high,0xffffffff,0xffffffff,0xffffffff),3);
  refresh().view.setUint32(ctx.base+96,1,true);
  control(`cycle-${cycle}-old-run-cancel-zero`,()=>old.run(0xffffffff,0xffffffff,0,0xffffffff),3,{generated:true,budget:0});
  control(`cycle-${cycle}-old-run-cancel-positive`,()=>old.run(0xffffffff,0xffffffff,64,0xffffffff),3,{generated:true,budget:64});
  refresh().view.setUint32(ctx.base+96,0,true);
  victim=compile([VICTIM,VICTIM+1],`victim-fresh-${cycle}`);assert.equal(live.size,8);assert.ok(BigInt(victim.id)>BigInt(old.id));
  control(`cycle-${cycle}-old-run-after-recompile`,()=>old.run(0xffffffff,0xffffffff,0,0xffffffff),3,{generated:true,budget:0});
  keeperStable(keeper,others);readPages(`cycle-${cycle}`);
  cycles.push({cycle,old_id:old.id,fresh_id:victim.id,next_pc:pc,eax:registers[0],flags:2,live_before:8,live_after_discard:7,live_after_recompile:8});
  // old pointer coordinates are expired metadata; only its copied bytes and function remain.
}
for(const [index,old] of oldFunctions.entries())control(`retained-old-${index+1}`,()=>old.run(0xffffffff,0xffffffff,64,0xffffffff),3,{generated:true,budget:64});
acknowledge(victim,1);
control('fresh-installed-victim',()=>discard(victim),7);
const finalRegisters=[...afterKeeper];finalRegisters[0]=0x12340003;
guest('dispatcher-resumes-fresh-installed-victim',ctx.dispatcher,2,finalRegisters,VICTIM+1,6,1,2,1,[victim,1]);
keeperStable(keeper,others);stable(victim);assert.equal(ctx.table.get(1),victim.run);readPages('final');
control('close',()=>api.close(),0);
control('closed-before-key',()=>api.discard_unacknowledged_resident(KEY_LOW^1,KEY_HIGH,0,0),5);
const counts={instances:1,resident_modules:11,dispatcher_modules:1,successful_discards:3,recompiles:3,capacity_refusals:1,peak_resident_units:8,retained_old_functions:3,controls:controls.length,generated_success_calls:runs.length,generated_refusals:controls.filter(row=>row.generated).length,guest_retired:guestRetired,ram_observations:observations.length,whole_pages:observations.reduce((n,row)=>n+row.pages.length,0),read32_calls:read32Calls};
assert.deepEqual(counts,{instances:1,resident_modules:11,dispatcher_modules:1,successful_discards:3,recompiles:3,capacity_refusals:1,peak_resident_units:8,retained_old_functions:3,controls:37,generated_success_calls:5,generated_refusals:12,guest_retired:7,ram_observations:6,whole_pages:12,read32_calls:12288});
const afterSources=sourceHashes();assert.deepEqual(afterSources,beforeSources);assert.equal(hash(readFileSync(enginePath)),hash(engineBytes));
writeFileSync(join(output,'source-after.json'),JSON.stringify(afterSources,null,2));
const result={counts,controls,runs,cycles,modules,observations,engine_sha256:hash(engineBytes),sources:beforeSources,source_after:afterSources,oracle:{key:[KEY_LOW,KEY_HIGH],code_address:CODE,data_address:DATA,victim:VICTIM,keeper_program:'b800003412e906000000',victim_program:'40ebfd',other_programs:Array.from({length:7},(_,index)=>({address:CODE+(index+2)*0x10,eax:0x81000000+index+2})),code_sha256:hash(codePage),data_sha256:hash(dataPage),initial_registers:initial,final_registers:finalRegisters,final_pc:VICTIM+1,final_flags:6,successful_ids:modules.filter(row=>row.id).map(row=>row.id)},limits:'Authored synchronous trusted-host flat32 ownership recovery. Unit cap dominates byte cap; native tests prove exact byte credit and wider lifecycle/Busy/currency priorities. Every removed raw pointer expires immediately and is never dereferenced again. Three old instantiated functions remain finite references for guard refusal. Whole declared code/data pages and full4236 arena are saved and asserted; no foreign Table/stack/race/GC/shrink/browser/SDK/PE/provider/game claim.'};
writeFileSync(join(output,'result.json'),JSON.stringify(result,null,2));console.log(JSON.stringify({counts,engine_sha256:result.engine_sha256,output}));
