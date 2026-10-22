import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync,writeFileSync,openSync,closeSync} from 'node:fs';
import {readEngine} from '../../../support/wasm/engine.mjs';
import {Engine,KEY,record,view} from '../../../../../sdk/runtime/engine.js';
import {CodeCache} from '../../../../../sdk/runtime/cache.js';

const [enginePath,reportPath] = process.argv.slice(2);
assert(enginePath && reportPath,'usage: chain-retention.mjs engine.wasm fresh-report.json');
const artifact = readEngine(enginePath), output = openSync(reportPath,'wx'); closeSync(output);
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const hex = bytes => Buffer.from(bytes).toString('hex');
const unhex = text => Uint8Array.from(Buffer.from(text.replaceAll(' ',''),'hex'));
const sources = ['./chain-retention.mjs','../../../support/wasm/engine.mjs',
  '../../../../../sdk/runtime/engine.js','../../../../../sdk/runtime/cache.js'];
const report = {schema:1,complete:false,engineSha256:artifact.sha256,
  scope:'actual native execution recency and single-eviction admission; no timing or game claim',
  sources:Object.fromEntries(sources.map(path=>[path,hash(readFileSync(new URL(path,import.meta.url)))])),cases:[]};
const checkpoint = () => writeFileSync(reportPath,JSON.stringify(report,null,2));
const counterNames = ['compiled','admittedEncodedBytes','countEvictions','byteEvictions','sourceInvalidations',
  'retainedUnits','retainedEncodedBytes','unitCapacity'];
const nativeCapacity = 4096;
let stage = 'initialization'; checkpoint();

function fixture(name,code,data,registers,capacity,instructions,body) {
  stage = name;
  const result = {name,modules:[],reads:[],controls:[]}; report.cases.push(result); checkpoint();
  const instance = new WebAssembly.Instance(artifact.module,{ring3_observability:{now_ns:()=>0n}});
  const api = Object.freeze(Object.fromEntries(Object.entries(instance.exports).filter(([name])=>name.startsWith('ring3_abi_v1_'))
    .map(([name,value])=>[name.slice(13),value])));
  const engine = new Engine(api,instance.exports.memory,artifact.sha256);
  const bytes = (pointer,length)=>new Uint8Array(engine.memory.buffer).slice(pointer,pointer+length);
  const word = (pointer,offset=0)=>new DataView(engine.memory.buffer).getUint32(pointer+offset,true);
  let opened = false, active = false, cache, ownersPointer;
  try {
    engine.ok('open',16,...KEY); opened=true; engine.base=api.arena_ptr()>>>0;
    const codePages = code.length/4096;assert(Number.isInteger(codePages)&&codePages>=1&&codePages<=2);
    const pages = [...Array.from({length:codePages},(_,index)=>[0x1000+index*4096,code.subarray(index*4096,(index+1)*4096),7]),
      [0x3000,data,3],[0x4000,new Uint8Array(4096),3]];
    for (const [address,page,permissions] of pages) {
      engine.ok('map',address,1,permissions);engine.request(page);engine.ok('upload',address,4096);
    }
    const state = record('R3ST',56), s = view(state);
    registers.forEach((value,index)=>s.setUint32(16+index*4,value,true)); s.setUint32(48,0x1000,true);s.setUint32(52,2,true);
    new Uint8Array(engine.memory.buffer,engine.base,56).set(state);
    const initialBanks = [bytes(engine.base+4236,352),bytes(engine.base+4612,184)];
    cache = new CodeCache(engine,{chain:true,capacity,instructions,blocks:8,budget:5});
    for (const name of ['core_read','core_write','core_numeric','core_chain_next'])
      assert.equal(cache.native[name],api[name],`original native ${name}`);
    function encoded() {return bytes(api.core_code_ptr()>>>0,api.core_code_len()>>>0);}
    function select(pc) {
      const unit=cache.entry(pc);
      if (!result.modules.some(item=>item.id===String(unit.id))) {
        const wasm=encoded(), item={pc,id:String(unit.id),slot:unit.slot,bytes:wasm.length,sha256:hash(wasm)};
        if (result.modules.length<2 || result.modules.length===nativeCapacity) item.wasmHex=hex(wasm);
        result.modules.push(item);
      }
      return unit;
    }
    function begin() {engine.ok('core_begin');active=true;}
    function end() {engine.ok('core_end');active=false;}
    function snapshot() {
      const backing=word(cache.context,64), pages=word(cache.context,68);
      assert(backing>0 && pages===16,'actual canonical backing descriptor');
      return {arenaHex:hex(bytes(engine.base,api.arena_len())),contextHex:hex(bytes(cache.context,128)),
        backingHex:hex(bytes(backing,pages*4096))};
    }
    function inspect(label) {
      const before=snapshot(), pointer=api.core_link_owners_ptr()>>>0;
      assert(pointer>0 && pointer+4097*8<=engine.memory.buffer.byteLength);
      ownersPointer??=pointer;assert.equal(pointer,ownersPointer,'stable native owner allocation');
      const owners=bytes(pointer,4097*8), ownerView=view(owners), statusPointer=api.core_cache_status_ptr()>>>0;
      assert(statusPointer>0 && statusPointer+64<=engine.memory.buffer.byteLength);
      const raw=bytes(statusPointer,64), d=view(raw);
      const counters=Object.fromEntries(counterNames.map((name,index)=>[name,d.getBigUint64(index*8,true).toString()]));
      const victim=api.core_cache_victim()>>>0, after=snapshot();
      result.reads.push({label,pointer,statusPointer,rawHex:hex(raw),ownerHex:hex(owners),victim,counters,before,after});checkpoint();
      assert.deepEqual(after,before,'closed native status/owner/victim reads preserve all context, arena and RAM');
      assert.equal(ownerView.getBigUint64(0,true),0n);
      return {counters,victim,owner:slot=>ownerView.getBigUint64(slot*8,true)};
    }
    function activeControls() {
      begin();const before=snapshot(), statuses=Object.fromEntries(
        ['core_link_owners_ptr','core_cache_victim','core_cache_status_ptr'].map(name=>[name,api[name]()>>>0]));
      const after=snapshot();result.controls.push({label:'active cold getters',statuses,before,after});checkpoint();
      assert.deepEqual(statuses,{core_link_owners_ptr:0,core_cache_victim:0,core_cache_status_ptr:0});
      assert.deepEqual(after,before,'rejected active getters preserve full context, arena and RAM');end();
    }
    function expectedRam() {
      const expected=new Uint8Array(16*4096);pages.forEach(([,page],index)=>expected.set(page,index*4096));
      const mappings=word(cache.context,56);
      assert.deepEqual(pages.map(([address])=>word(mappings+(address>>>12)*4)),pages.map((_,index)=>index+1));
      return expected;
    }
    body({api,engine,cache,bytes,word,select,encoded,begin,end,snapshot,inspect,activeControls,expectedRam,initialBanks,result});
    result.final=snapshot();result.stats=cache.sampleUsage();result.complete=true;checkpoint();
  } catch(error) {
    result.failure={stage,message:String(error),stack:error.stack,contextHex:cache?.context?hex(bytes(cache.context,128)):null};checkpoint();throw error;
  } finally {if(active||cache?.open)api.core_end();if(opened)engine.ok('close');}
}

function countersEqual(actual,expected) {
  for (const [name,value] of Object.entries(expected)) assert.equal(actual[name],String(value),`native ${name}`);
}
function sdkEqual(actual,expected) {
  for (const [name,value] of Object.entries(expected)) assert.equal(actual[name],value,`SDK ${name}`);
}

try {
  const code=new Uint8Array(4096),data=new Uint8Array(4096);
  code.set(unhex('d903 ffe6'));code.set(unhex('d84b04 d95b08 ffe2'),0x40);code.set(unhex('0f0b'),0x100);
  view(data).setFloat32(0,3,true);view(data).setFloat32(4,2,true);
  const registers=[0,0,0x1080,0x3000,0x4800,0,0x1040,0];
  fixture('native-execution-recency-preserves-the-hot-target',code,data,registers,2,64,t=>{
    const b=t.select(0x1040),a=t.select(0x1000);
    t.cache.import();assert.equal(t.cache.pc,0x1000,'canonical starting PC after cold entry selection');
    assert.deepEqual([...t.cache.units.keys()],[b.id,a.id],'cold JavaScript entry order differs from execution order');
    t.activeControls();const initialContext=t.bytes(t.cache.context,128);
    const before=t.inspect('primed-cold-order');assert.equal(before.victim,b.slot);
    assert.equal(before.owner(a.slot),a.id);assert.equal(before.owner(b.slot),b.id);
    const run=t.cache.execute(a), after=t.snapshot();
    t.result.executed={...run,id:String(run.id),...after};checkpoint();
    assert.equal(run.coreReason,2);assert.equal(run.regions,2);assert.equal(run.retired,5);assert.equal(run.work,5);assert.equal(run.id,b.id);
    const expectedContext=initialContext.slice(),c=view(expectedContext);
    c.setUint32(32,0x1080,true);c.setUint32(40,5,true);c.setUint32(44,0,true);c.setBigUint64(120,b.id,true);
    assert.deepEqual(t.bytes(t.cache.context,128),expectedContext,'complete executed context');
    const expectedState=record('R3ST',56),s=view(expectedState);
    registers.forEach((value,index)=>s.setUint32(16+index*4,value,true));s.setUint32(48,0x1080,true);s.setUint32(52,2,true);
    assert.deepEqual(t.bytes(t.engine.base,56),expectedState);
    const expectedExit=record('R3EX',40),e=view(expectedExit);e.setUint16(4,15,true);e.setUint32(16,1,true);e.setUint32(20,5,true);
    assert.deepEqual(t.bytes(t.engine.base+56,40),expectedExit);
    const expectedWork=record('R3WK',24);view(expectedWork).setUint32(16,5,true);
    assert.deepEqual(t.bytes(t.engine.base+4588,24),expectedWork);
    const fp=t.initialBanks[0].slice(0,128),f=view(fp);
    for(const [offset,value] of [[18,0],[20,0xffff],[22,0x15b],[32,0x1b],[34,0x23]])f.setUint16(offset,value,true);
    f.setUint32(24,0x1043,true);f.setUint32(28,0x3008,true);fp.set(unhex('00000000000000c00140'),110);
    assert.deepEqual(t.bytes(t.engine.base+4236,128),fp,'exact FP80 bank after native ordinal handoff');
    assert.deepEqual(t.bytes(t.engine.base+4364,224),t.initialBanks[0].slice(128));
    assert.deepEqual(t.bytes(t.engine.base+4612,184),t.initialBanks[1]);
    const ram=t.expectedRam();ram.set(unhex('0000c040'),4104);
    assert.deepEqual(t.bytes(t.word(t.cache.context,64),ram.length),ram,'entire code/data/stack allocation');
    const hot=t.inspect('executed-A-then-B');assert.equal(hot.victim,a.slot,'actual execution makes A the native victim');
    countersEqual(hot.counters,{compiled:2,countEvictions:0,byteEvictions:0,sourceInvalidations:0,retainedUnits:2,unitCapacity:nativeCapacity});
    const cUnit=t.select(0x1100), admitted=t.inspect('independent-C-admitted');
    assert.equal(t.cache.units.has(a.id),false,'evict A rather than the native-hot B');
    assert.equal(t.cache.units.get(b.id),b);assert.equal(t.cache.table.get(b.slot),b.run);
    assert.equal(admitted.owner(b.slot),b.id);assert.equal(cUnit.slot,a.slot);assert.equal(admitted.owner(cUnit.slot),cUnit.id);
    const retainedContext=expectedContext.slice();view(retainedContext).setBigUint64(120,cUnit.id,true);
    assert.deepEqual(t.bytes(t.cache.context,128),retainedContext,'cold selection only updates code identity');
    assert.deepEqual(t.bytes(t.engine.base,t.api.arena_len()),unhex(after.arenaHex),'cold eviction preserves the complete published arena');
    assert.deepEqual(t.bytes(t.word(t.cache.context,64),ram.length),ram);
    const encodedBytes=t.result.modules.reduce((sum,module)=>sum+module.bytes,0);
    countersEqual(admitted.counters,{compiled:3,admittedEncodedBytes:encodedBytes,countEvictions:0,byteEvictions:0,
      sourceInvalidations:0,retainedUnits:3,retainedEncodedBytes:encodedBytes,unitCapacity:nativeCapacity});
    const stats=t.cache.sampleUsage();sdkEqual(stats,{admissions:3,installedUnits:2,evictions:1,prunedNative:0,prunedNativeBytes:0,
      retired:5,work:5,runs:1,regions:2});assert.deepEqual(stats.evictionReasons,{count:1,bytes:0});
    assert.deepEqual(stats.nativeCache,admitted.counters);
    const retainedB=t.select(0x1040);assert.equal(retainedB,b);assert.equal(retainedB.slot,b.slot);assert.equal(retainedB.run,b.run);
    assert.equal(t.inspect('B-reselected-without-compilation').counters.compiled,'3');
  });

  const tiny=new Uint8Array(8192);tiny.fill(0x90,0,nativeCapacity+1);
  fixture('native-capacity-eviction-is-pruned-once',tiny,new Uint8Array(4096),[5,0,0,0x3000,0x4800,0,0,0],nativeCapacity,1,t=>{
    const first=t.select(0x1000);t.activeControls();const initial=t.snapshot();let second;
    for(let index=1;index<nativeCapacity;index++) {
      const unit=t.select(0x1000+index);if(index===1)second=unit;
    }
    const before=t.inspect('4096-linked-owners');assert.equal(before.victim,first.slot);
    countersEqual(before.counters,{compiled:nativeCapacity,countEvictions:0,byteEvictions:0,sourceInvalidations:0,retainedUnits:nativeCapacity,unitCapacity:nativeCapacity});
    assert.equal(t.cache.units.size,nativeCapacity);assert.equal(before.owner(first.slot),first.id);assert.equal(before.owner(second.slot),second.id);
    t.engine.ok('core_prepare',0x2000,1,8);
    const nativeOnly=t.inspect('native-4097th-compile-before-SDK-admission');
    countersEqual(nativeOnly.counters,{compiled:nativeCapacity+1,countEvictions:1,byteEvictions:0,sourceInvalidations:0,retainedUnits:nativeCapacity,unitCapacity:nativeCapacity});
    assert.equal(nativeOnly.owner(first.slot),0n,'native eviction already cleared its reciprocal owner');
    assert.equal(nativeOnly.owner(second.slot),second.id);assert.equal(t.cache.units.size,nativeCapacity,'SDK has not pruned yet');
    const newest=t.select(0x2000), final=t.inspect('SDK-pruned-zero-owner-and-admitted-existing-code');
    const encodedBytes=t.result.modules.reduce((sum,module)=>sum+module.bytes,0),firstBytes=first.bytes;
    countersEqual(final.counters,{compiled:nativeCapacity+1,admittedEncodedBytes:encodedBytes,countEvictions:1,byteEvictions:0,
      sourceInvalidations:0,retainedUnits:nativeCapacity,retainedEncodedBytes:encodedBytes-firstBytes,unitCapacity:nativeCapacity});
    assert.equal(t.cache.units.has(first.id),false);assert.equal(t.cache.units.get(second.id),second);
    assert.equal(t.cache.units.size,nativeCapacity,'no second valid owner was evicted');
    assert.equal(newest.slot,first.slot);assert.equal(final.owner(newest.slot),newest.id);
    assert.equal(t.cache.table.get(second.slot),second.run);assert.equal(final.owner(second.slot),second.id);
    const stats=t.cache.sampleUsage();sdkEqual(stats,{admissions:nativeCapacity+1,installedUnits:nativeCapacity,evictions:0,prunedNative:1,
      prunedNativeBytes:firstBytes,retired:0,work:0,runs:0,regions:0});
    assert.deepEqual(stats.evictionReasons,{count:0,bytes:0});assert.deepEqual(stats.nativeCache,final.counters);
    assert.equal(stats.generatedModuleBytes,encodedBytes);assert.equal(stats.retainedModuleBytes,encodedBytes-firstBytes);
    const expectedContext=unhex(initial.contextHex);view(expectedContext).setBigUint64(120,newest.id,true);
    assert.deepEqual(t.bytes(t.cache.context,128),expectedContext,'compilation changes no architectural state');
    assert.deepEqual(t.bytes(t.engine.base,t.api.arena_len()),unhex(initial.arenaHex),'cold-only sequence preserves the full arena');
    assert.deepEqual(t.bytes(t.word(t.cache.context,64),16*4096),t.expectedRam());
    const retained=t.select(0x1001);assert.equal(retained,second);assert.equal(retained.slot,second.slot);
    assert.equal(t.inspect('surviving-second-owner-no-recompile').counters.compiled,String(nativeCapacity+1));
  });
  report.complete=true;checkpoint();console.log(JSON.stringify({complete:true,cases:report.cases.length,engineSha256:artifact.sha256}));
} catch(error) {report.failure={stage,message:String(error),stack:error.stack};checkpoint();throw error;}
