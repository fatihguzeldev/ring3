import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync,writeFileSync,openSync,closeSync} from 'node:fs';
import {readEngine} from '../../../support/wasm/engine.mjs';
import {Engine,KEY,record,view} from '../../../../../sdk/runtime/engine.js';
import {CodeCache} from '../../../../../sdk/runtime/cache.js';

const [enginePath,reportPath] = process.argv.slice(2);
assert(enginePath && reportPath,'usage: chain-authority.mjs engine.wasm fresh-report.json');
const artifact = readEngine(enginePath), output = openSync(reportPath,'wx'); closeSync(output);
const hex = bytes => Buffer.from(bytes).toString('hex');
const unhex = text => Uint8Array.from(Buffer.from(text.replaceAll(' ',''),'hex'));
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const sources = ['./chain-authority.mjs','../../../support/wasm/engine.mjs',
  '../../../../../sdk/runtime/engine.js','../../../../../sdk/runtime/cache.js'];
const report = {schema:1,complete:false,engineSha256:artifact.sha256,
  scope:'explicit compatibility-owner authority, normal cold refresh, and shared observed-family ports; no timing or game claim',
  sources:Object.fromEntries(sources.map(path => [path,hash(readFileSync(new URL(path,import.meta.url)))])),cases:[]};
const checkpoint = () => writeFileSync(reportPath,JSON.stringify(report,null,2));
let stage = 'initialization'; checkpoint();

const code = new Uint8Array(4096);
for (const [pc,bytes] of [[0x1000,'d903 ffe6'],[0x1040,'d84b04 d95b08 ffe2'],[0x1080,'0f0b'],
  [0x1100,'f3a4 ffe2'],[0x1140,'83c001 0f0b'],[0x1200,'8903 ffe6'],
  [0x1300,'d903 e839000000 49 75f6 0f0b'],[0x1340,'d84b04 d95b08 c3']]) code.set(unhex(bytes),pc-0x1000);
const bankRanges = [[4236,352],[4612,184]];
function fp(initial,{status=0,tag=0xffff,opcode=0,fip=0,fdp=0,registers={}}={}) {
  const result = initial.slice(), d = view(result);
  for (const [offset,value] of [[18,status],[20,tag],[22,opcode],[32,0x1b],[34,0x23]]) d.setUint16(offset,value,true);
  d.setUint32(24,fip,true); d.setUint32(28,fdp,true);
  for (const [index,value] of Object.entries(registers)) result.set(unhex(value),40+10*Number(index));
  return result;
}

function fixture(name,{entry=0x1000,target=0x1040,registers=[0,0,0x1080,0x3000,0x4800,0,0x1040,0],stack=true,data=()=>{},codePage=code,extraCodePages=[],firstInstructions=64,traceSelection=false,calleeFirst=false,controlWord}={},body) {
  stage = name;
  const result = {name,modules:[],controls:[],runs:[]}; report.cases.push(result); checkpoint();
  const instance = new WebAssembly.Instance(artifact.module,{ring3_observability:{now_ns:()=>0n}});
  const native = Object.fromEntries(Object.entries(instance.exports).filter(([name]) => name.startsWith('ring3_abi_v1_'))
    .map(([name,value]) => [name.slice(13),value]));
  const selectionCalls = {core_prepare:0,core_lookup:0,core_cold_run:0};
  let tracePointer=0;
  for(const name of ['core_chain_resolve_owned','core_chain_resolve_executable_owned'])
    if(typeof native[name]==='function')selectionCalls[name]=0;
  if(traceSelection){
    const finish=native.core_trace_finish;
    native.core_trace_finish=(...args)=>{
      const pointer=native.core_trace_pointer()>>>0,d=new DataView(instance.exports.memory.buffer);
      const used=pointer?d.getUint32(pointer+4,true):0;
      const attempt={args,header:pointer?Array.from({length:4},(_,i)=>d.getUint32(pointer+i*4,true)):null,
        events:Array.from({length:Math.min(used,129)},(_,i)=>{
          const at=pointer+32+i*24;
          return {kind:d.getUint32(at,true),pc:d.getUint32(at+4,true),end:d.getUint32(at+8,true),
            offset:d.getUint32(at+12,true),owner:String(d.getBigUint64(at+16,true))};
        })};
      (result.traceAttempts??=[]).push(attempt);attempt.status=finish(...args);checkpoint();return attempt.status;
    };
  }
  if (traceSelection) for (const name of Object.keys(selectionCalls)) {
    const operation = native[name];
    native[name] = (...args) => {
      selectionCalls[name]++;
      const route=name.startsWith('core_chain_resolve')?{operation:name,pc:args[1]>>>0,
        source:String(new DataView(instance.exports.memory.buffer).getBigUint64(args[0]+120,true)),
        trace:tracePointer?{
          active:new DataView(instance.exports.memory.buffer).getUint32(tracePointer,true),
          countdown:new DataView(instance.exports.memory.buffer).getUint32(tracePointer+32+129*24,true),
          pending:new DataView(instance.exports.memory.buffer).getUint32(tracePointer+32+129*24+4,true),
        }:null}:null;
      const value=operation(...args);
      if(route)(result.routes??=[]).push({...route,slot:value>>>0});
      return value;
    };
  }
  const api = Object.freeze(native);
  const engine = new Engine(api,instance.exports.memory,artifact.sha256);
  const bytes = (pointer,length) => new Uint8Array(engine.memory.buffer).slice(pointer,pointer+length);
  const word = (pointer,offset=0) => new DataView(engine.memory.buffer).getUint32(pointer+offset,true);
  let opened = false, active = false, cache;
  const status = (operation,wanted,...args) => {
    const actual = api[operation](...args)>>>0; result.controls.push({operation,args,wanted,actual});
    checkpoint(); assert.equal(actual,wanted,`${name}: ${operation}`);
  };
  try {
    engine.ok('open',16,...KEY); opened = true; engine.base = api.arena_ptr()>>>0;
    const dataPage = new Uint8Array(4096); data(view(dataPage));
    const pages = [[0x1000,codePage,7],[0x3000,dataPage,3],...(stack ? [[0x4000,new Uint8Array(4096),3]] : []),
      ...extraCodePages.map(([address,page])=>[address,page,7])];
    for (const [address,page,permissions] of pages) {
      engine.ok('map',address,1,permissions); engine.request(page); engine.ok('upload',address,4096);
    }
    const state = record('R3ST',56), d = view(state);
    registers.forEach((value,index) => d.setUint32(16+index*4,value,true));
    d.setUint32(48,entry,true); d.setUint32(52,2,true); new Uint8Array(engine.memory.buffer,engine.base,56).set(state);
    if(controlWord!==undefined)new DataView(engine.memory.buffer).setUint16(engine.base+4236+16,controlWord,true);
    const initialBanks = bankRanges.map(([offset,length]) => bytes(engine.base+offset,length));
    const initialFp = bytes(engine.base+4236,128);
    cache = new CodeCache(engine,{chain:true,budget:8,instructions:firstInstructions,blocks:8});
    function select(pc=cache.pc) {
      const unit = cache.entry(pc);
      if (!result.modules.some(item => item.id === String(unit.id))) {
        const encoded = bytes(api.core_code_ptr()>>>0,api.core_code_len()>>>0);
        result.modules.push({pc,id:String(unit.id),slot:unit.slot,sha256:hash(encoded),wasmHex:hex(encoded)});
      }
      return unit;
    }
    let next;
    if(calleeFirst) {
      // A retained callee is an explicit discovery frontier for the independently
      // owned graph; default discovery now correctly follows a fresh direct CALL.
      cache.instructions=64;next=select(target);cache.instructions=firstInstructions;
    }
    const first = select(entry);cache.instructions=64;
    next??=select(target);select(entry);
    if(calleeFirst)status('core_import',0);
    tracePointer=api.core_trace_pointer()>>>0;assert(tracePointer>0&&tracePointer%8===0);
    status('core_begin',0); active = true; status('core_end',0); active = false;
    const initialContext = bytes(cache.context,128), backing = word(cache.context,64), residentPages = word(cache.context,68);
    assert.equal(residentPages,16);
    const expectedRam = new Uint8Array(16*4096);
    pages.forEach(([address,page],index) => {
      assert.equal(word(word(cache.context,56)+(address>>>12)*4),index+1);
      expectedRam.set(page,index*4096);
    });
    assert.deepEqual(bytes(backing,expectedRam.length),expectedRam,'initial complete canonical backing');
    const snapshot = () => ({arenaHex:hex(bytes(engine.base,api.arena_len())),contextHex:hex(bytes(cache.context,128)),
      backingHex:hex(bytes(backing,expectedRam.length))});
    const neutral = (label,action) => {
      const before = snapshot(); action(); const after = snapshot(); result.controls.push({label,before,after}); checkpoint();
      assert.deepEqual(after,before,`${name}: ${label} neutral`);
    };
    function run(unit,budget) {
      cache.budget = budget;
      const sourceVersion = new DataView(engine.memory.buffer).getBigUint64(word(cache.context,88),true);
      const value = cache.execute(unit);
      result.runs.push({...value,id:value.id===null?null:String(value.id),budget,sourceVersion:String(sourceVersion),...snapshot()});
      // post-lease optimization may select a new family without a region call.
      const preparedId = new DataView(engine.memory.buffer).getBigUint64(cache.context+120,true);
      const selected = cache.units.get(preparedId);
      if (selected && !result.modules.some(item => item.id === String(selected.id))) {
        const encoded = bytes(api.core_code_ptr()>>>0,api.core_code_len()>>>0);
        result.modules.push({pc:selected.pc,id:String(selected.id),slot:selected.slot,sha256:hash(encoded),wasmHex:hex(encoded)});
      }
      checkpoint(); return {value,sourceVersion};
    }
    function expect({stop,pc,retired,exitRetired=retired,work,workTotal=work,remaining,regions,id,selectedId=id,regs=registers,flags=2,fault=[0,0,0],wantedFp=initialFp,sourceVersion}) {
      const last = result.runs.at(-1); assert.equal(last.coreReason,stop);
      assert.equal(last.id,id===0n?null:String(id),'the returned identity names the owner that actually finished guest execution');
      const expectedContext = initialContext.slice(), c = view(expectedContext);
      regs.forEach((value,index) => c.setUint32(index*4,value,true));
      for (const [offset,value] of [[32,pc],[36,flags],[40,retired],[44,remaining],[48,fault[0]],[52,fault[1]],[112,0],[116,fault[2]]]) c.setUint32(offset,value,true);
      c.setBigUint64(96,sourceVersion,true); c.setBigUint64(120,selectedId,true);
      assert.deepEqual(bytes(cache.context,128),expectedContext,'full literal Context32 and selected owner');
      assert.equal(cache.stats().regions,regions); assert.equal(cache.stats().retired,retired); assert.equal(cache.stats().work,workTotal);
      assert.deepEqual(bytes(backing,expectedRam.length),expectedRam,'whole canonical allocation');
      assert.deepEqual(bytes(engine.base+4236,128),wantedFp,'full literal FP128');
      assert.deepEqual(bytes(engine.base+4364,224),initialBanks[0].slice(128),'full SIMD and segment records');
      assert.deepEqual(bytes(engine.base+4612,184),initialBanks[1],'full upper vector and segment bases');
      const expectedState = record('R3ST',56), s = view(expectedState);
      regs.forEach((value,index) => s.setUint32(16+index*4,value,true)); s.setUint32(48,pc,true); s.setUint32(52,flags,true);
      assert.deepEqual(bytes(engine.base,56),expectedState,'complete published State32');
      const expectedWork = record('R3WK',24); view(expectedWork).setUint32(16,work,true);
      assert.deepEqual(bytes(engine.base+4588,24),expectedWork,'work independent from retirement');
      const expectedExit = record('R3EX',40), e = view(expectedExit); e.setUint16(4,15,true);
      e.setUint32(16,stop === 2 ? 1 : stop === 6 ? 6 : 5,true); e.setUint32(20,exitRetired,true);
      if (stop === 3) {
        e.setUint32(24,1,true); e.setUint32(28,fault[0],true);
        e.setUint32(32,(fault[1]>>>8)&255,true); e.setUint32(36,fault[2]>>>8 || (fault[1]&255),true);
      }
      assert.deepEqual(bytes(engine.base+56,40),expectedExit,'literal terminal Exit');
    }
    body({api,engine,cache,bytes,word,select,first,next,initialFp,initialBanks,expectedRam,snapshot,neutral,status,run,expect,result,selectionCalls,
      begin:()=>{status('core_begin',0); active=true;},end:()=>{status('core_end',0);active=false;}});
    result.final = snapshot(); result.stats = cache.stats(); result.complete = true; checkpoint();
  } catch (error) {
    result.failure = {message:String(error),stack:error.stack,contextHex:cache?.context ? hex(bytes(cache.context,128)) : null}; checkpoint(); throw error;
  } finally {if (active || cache?.open) api.core_end(); if (opened) engine.ok('close');}
}

const values = d => {d.setFloat32(0,3,true);d.setFloat32(4,2,true);};
try {
  fixture('numeric-owner-handoff-and-link-admission',{data:values},t => {
    const low = Number(t.first.id&0xffffffffn), high = Number(t.first.id>>32n);
    t.neutral('invalid and wrong-owner links',()=>{
      for (const slot of [0,4097]) {t.status('core_link',7,slot);t.status('core_unlink',7,slot,low,high);}
      t.status('core_unlink',4,t.first.slot,low^1,high);
      t.status('core_link',0,t.first.slot); t.status('core_link',7,t.next.slot);
      t.status('core_chain_next',0xffffffff,t.cache.context);
    });
    t.begin();
    t.neutral('active grants and wrong context',()=>{
      t.status('core_link',12,t.first.slot);t.status('core_unlink',12,t.first.slot,low,high);
      t.status('core_chain_next',0xffffffff,t.cache.context+8);
      t.status('core_dispatch_code_ptr',0);t.status('core_dispatch_code_len',0);
    });
    t.end();
    const {sourceVersion} = t.run(t.first,5); t.expectedRam.set(unhex('0000c040'),4096+8);
    t.expect({stop:2,pc:0x1080,retired:5,work:5,remaining:0,regions:2,id:t.next.id,sourceVersion,
      wantedFp:fp(t.initialFp,{opcode:0x15b,fip:0x1043,fdp:0x3008,registers:{7:'00000000000000c00140'}})});
    assert.equal(t.cache.stats().runs,1);
  });
  fixture('stale-source-rejection-and-complete-cold-prefix',{data:values},t => {
    t.engine.ok('write8',0x1041,0x43); t.expectedRam[0x41]=0x43;
    const {value,sourceVersion} = t.run(t.first,5);
    t.expectedRam.set(unhex('0000a040'),4096+8);
    t.expect({stop:2,pc:0x1080,retired:5,work:5,remaining:0,regions:0,id:0n,sourceVersion,
      wantedFp:fp(t.initialFp,{opcode:0x15b,fip:0x1043,fdp:0x3008,registers:{7:'00000000000000a00140'}})});
    assert.equal(value.id,null,'the rejected stale entry executes the complete prefix cold');
    assert.equal(value.kind,'cold');
    assert.deepEqual([value.chainedColdRuns,value.chainedColdRetired,value.chainedColdWork],[0,0,0]);
    assert.deepEqual([t.cache.stats().coldRuns,t.cache.stats().coldRetired,t.cache.stats().coldWork],[1,5,5]);
    assert.equal(t.cache.stats().admissions,2,'a stale source does not force a replacement Wasm owner');
    const owners=new DataView(t.engine.memory.buffer,t.api.core_link_owners_ptr()>>>0,4097*8);
    assert.equal(owners.getBigUint64(t.first.slot*8,true),0n,'native authority revokes the stale executable capability');
    assert.equal(t.cache.stats().runs,0);
  });
  fixture('numeric-read-fault-after-handoff',{stack:false,registers:[0,0,0x1080,0x3ffc,0x4800,0,0x1040,0],
    data:d=>d.setFloat32(4092,3,true)},t => {
    const {sourceVersion} = t.run(t.first,5);
    t.expect({stop:3,pc:0x1040,retired:2,work:2,remaining:3,regions:2,id:t.next.id,sourceVersion,fault:[0x4000,0x10104,0x40e],
      wantedFp:fp(t.initialFp,{status:0x3800,tag:0x3fff,opcode:0x103,fip:0x1000,fdp:0x3ffc,registers:{7:'00000000000000c00040'}})});
  });
  const repeat = {entry:0x1100,target:0x1140,registers:[5,3,0x1140,0x3000,0x4800,0,0x3000,0x4100],
    data:d=>{d.setUint8(0,0x11);d.setUint8(1,0x22);d.setUint8(2,0x33);}};
  fixture('repeat-completion-and-next-region',repeat,t => {
    const {sourceVersion} = t.run(t.first,5); t.expectedRam.set(unhex('112233'),8192+0x100);
    t.expect({stop:2,pc:0x1143,retired:3,work:5,remaining:0,regions:2,id:t.next.id,sourceVersion,flags:6,
      regs:[6,0,0x1140,0x3000,0x4800,0,0x3003,0x4103]});
  });
  fixture('repeat-partial-budget',repeat,t => {
    const {sourceVersion} = t.run(t.first,2); t.expectedRam.set(unhex('1122'),8192+0x100);
    t.expect({stop:2,pc:0x1100,retired:0,work:2,remaining:0,regions:1,id:t.first.id,sourceVersion,
      regs:[5,1,0x1140,0x3000,0x4800,0,0x3002,0x4102]});
  });
  fixture('repeat-write-fault-prefix',{...repeat,registers:[5,3,0x1140,0x3000,0x4800,0,0x3000,0x4fff]},t => {
    const {sourceVersion} = t.run(t.first,8); t.expectedRam[8192+4095]=0x11;
    t.result.faultProtocol=typeof t.api.core_precise==='function'?'canonical width1 plus #PF14':'legacy #PF14';
    t.expect({stop:3,pc:0x1100,retired:0,work:1,remaining:7,regions:1,id:t.first.id,sourceVersion,
      fault:[0x5000,0x10201,typeof t.api.core_precise==='function'?0x10e:14],
      regs:[5,2,0x1140,0x3000,0x4800,0,0x3001,0x5000]});
  });
  fixture('code-write-prevents-next-region',{entry:0x1200,target:0x1140,registers:[0x11223344,0,0,0x1200,0x4800,0,0x1140,0]},t => {
    const {sourceVersion} = t.run(t.first,8); t.expectedRam.set(unhex('44332211'),0x200);
    t.expect({stop:6,pc:0x1202,retired:1,work:1,remaining:7,regions:1,id:t.first.id,sourceVersion});
    t.neutral('code writer cannot reenter stale source',()=>t.status('core_begin',4));
  });
  const crossPageCode=code.slice(),crossPageCallee=new Uint8Array(4096);
  crossPageCode.set(unhex('d903 e839400000 49 75f6 0f0b'),0x300);
  crossPageCallee.set(unhex('d84b04 d95b08 c3'),0x340);
  for (const [publicSelection,crossPage,manual] of [
    [false,false,false],[true,false,false],[true,true,false],[true,true,true],
  ]) fixture(manual?'manual-cross-page-root-only-trace-source-and-slot-authority':crossPage
    ? 'public-selection-cross-page-numerical-graph-authority'
    : publicSelection ? 'public-selection-preserves-warmed-numerical-graph'
      : 'numerical-ordinals-survive-warmed-graph',{
    entry:0x1300,target:crossPage?0x5340:0x1340,
    registers:[0,manual?9:8192,0,0x3000,0x4800,0,0,0],data:values,
    firstInstructions:5,traceSelection:true,calleeFirst:true,
    ...(manual?{controlWord:0x27f}:{}),
    ...(crossPage?{codePage:crossPageCode,extraCodePages:[[0x5000,crossPageCallee]]}:{}),
  },t => {
    const iterations=manual?9:8192,totalWork=7*iterations;
    const ports=[0x1300,0x1302,0x1307,0x1308,...(crossPage?[0x5340,0x5343,0x5346]:[0x1340,0x1343,0x1346])];
    // These explicit legacy source bindings remain authority controls even when
    // automatic execution later admits a separate hot suffix at a nonhot port.
    for(const pc of ports)assert.equal(t.select(pc),pc<0x1340?t.first:t.next);
    assert.equal(t.select(0x1300),t.first);
    const admissionsBefore=t.cache.stats().admissions;
    const callsBefore={...t.selectionCalls},originalModules=t.result.modules.slice();
    const originalExports=[t.first,t.next].map(unit=>({unit,run:unit.run,link:unit.link,publish:unit.publish}));
    const ownerPointer=t.api.core_link_owners_ptr()>>>0,originalOwners=t.bytes(ownerPointer,4097*8);
    const version=()=>new DataView(t.engine.memory.buffer).getBigUint64(t.word(t.cache.context,80),true);
    const beforeVersion=version();
    const selected=publicSelection?t.cache.select():t.first;
    assert.equal(selected,t.first);
    if(manual)t.status('core_trace_start',0,0x1300);
    const {value:last,sourceVersion}=t.run(selected,totalWork);
    t.expectedRam.set(unhex('0000c040'),4096+8);
    t.expectedRam.set(unhex('07130000'),8192+0x7fc);
    let overlay=null,overlayModule=null;
    if(manual) {
      t.status('core_trace_finish',0,0x1300,128,32);
      overlay=t.select(0x1300);
      overlayModule=t.result.modules.find(module=>module.id===String(overlay.id));
      assert(overlayModule);assert.notEqual(overlay.id,t.first.id);assert.notEqual(overlay.id,t.next.id);
    }
    const stats=t.cache.stats(),overlays=[...t.cache.units.values()].filter(unit=>unit!==t.first&&unit!==t.next);
    assert.equal(overlays.length,1,'one certified automatic precise frontier or explicit manual trace is installed');
    const automatic=manual?null:overlays[0],optimized=overlay??automatic;
    const optimizedModule=t.result.modules.find(module=>module.id===String(optimized.id));
    assert(optimizedModule);
    if(automatic) {
      const capture=t.result.traceAttempts.find(attempt=>attempt.status===0);assert(capture);
      assert(capture.events.some(event=>event.kind===3&&event.pc===(crossPage?0x5340:0x1340)),
        'automatic formation uses the real completed precise numerical operation');
      assert.equal(automatic.pc,crossPage?0x5343:0x1343,'only the nonhot exact callee suffix gains a new executor');
      assert.notEqual(automatic.id,t.first.id);assert.notEqual(automatic.id,t.next.id);
    }
    const wantedOwners=originalOwners.slice();
    view(wantedOwners).setBigUint64(optimized.slot*8,optimized.id,true);
    assert.deepEqual(t.bytes(ownerPointer,4097*8),wantedOwners,'original reciprocal fragment slots remain unchanged');
    const wanted={stop:2,pc:0x130a,retired:totalWork,work:totalWork,remaining:0,
      regions:2*iterations+1,id:t.first.id,selectedId:optimized.id,sourceVersion,
      regs:[0,0,0,0x3000,0x4800,0,0,0],flags:0x46,
      wantedFp:fp(t.initialFp,{opcode:0x15b,fip:crossPage?0x5343:0x1343,fdp:0x3008,
        registers:{7:'00000000000000c00140'}})};
    t.expect(wanted);
    assert.deepEqual([last.retired,last.work,last.coreReason],[totalWork,totalWork,2]);
    assert.equal(last.id,t.first.id);assert.equal(t.result.runs.length,1);
    assert.deepEqual([stats.runs,stats.coldRuns,stats.coldRetired,stats.coldWork],[1,0,0,0]);
    assert.deepEqual([stats.admissions,stats.installedUnits,stats.transientAdmissions,stats.transientRuns],
      [3,3,0,0]);
    assert.equal(stats.admissions,admissionsBefore+1);
    assert.equal(stats.generatedModuleBytes,originalModules.reduce((sum,module)=>sum+module.wasmHex.length/2,0)
      +optimized.bytes);
    assert.deepEqual(t.result.modules.filter(module=>module.id!==String(optimized.id)),originalModules);
    assert.equal(t.result.modules.length,originalModules.length+1);
    assert.equal(t.selectionCalls.core_cold_run,callsBefore.core_cold_run);
    if(!manual)assert.equal(t.selectionCalls.core_prepare,callsBefore.core_prepare);
    assert.equal(version(),beforeVersion+2n*BigInt(iterations));

    for(const pc of ports) {
      const expected=manual&&pc===0x1300?overlay:pc<0x1340?t.first:t.next;
      assert.equal(t.select(pc),expected,'explicit public-port admission keeps its fragment except the manual trace root');
      assert.equal(new DataView(t.engine.memory.buffer).getBigUint64(t.cache.context+120,true),expected.id);
      const expectedModule=expected===overlay?overlayModule:originalModules.find(module=>module.id===String(expected.id));
      assert.equal(hash(t.bytes(t.api.core_code_ptr()>>>0,t.api.core_code_len()>>>0)),expectedModule.sha256);
    }
    for(const saved of originalExports) {
      assert.equal(t.cache.units.get(saved.unit.id),saved.unit);
      assert.equal(saved.unit.run,saved.run);assert.equal(saved.unit.link,saved.link);assert.equal(saved.unit.publish,saved.publish);
      assert.equal(t.cache.table.get(saved.unit.slot),saved.link);
      assert.equal(t.cache.publishers.get(saved.unit.slot),saved.publish);
      assert.equal(new DataView(t.engine.memory.buffer,ownerPointer,4097*8).getBigUint64(saved.unit.slot*8,true),saved.unit.id);
    }
    assert.equal(t.cache.stats().admissions,stats.admissions);
    assert.equal(t.cache.stats().generatedModuleBytes,stats.generatedModuleBytes);

    // Repeat the complete literal input twice. The first replay can learn the
    // new suffix boundary; the second must use its stable generated route.
    const resolverCalls=()=>['core_chain_resolve_owned','core_chain_resolve_executable_owned']
      .reduce((sum,name)=>sum+(t.selectionCalls[name]??0),0);
    const warmResults=[];
    for(let replay=0;replay<3;replay++) {
      const warmState=record('R3ST',56),warmView=view(warmState);
      [0,9,0,0x3000,0x4800,0,0,0].forEach((value,index)=>warmView.setUint32(16+index*4,value,true));
      warmView.setUint32(48,0x1300,true);warmView.setUint32(52,2,true);
      new Uint8Array(t.engine.memory.buffer,t.engine.base,56).set(warmState);
      new Uint8Array(t.engine.memory.buffer,t.engine.base+4236,128).set(t.initialFp);
      t.cache.import();
      const warmSelected=t.cache.select(0x1300);assert.equal(warmSelected,manual?overlay:t.first);
      const resolves=resolverCalls(),routeBefore=t.result.routes?.length??0;
      const captureBefore=t.result.traceAttempts?.length??0;
      const countdown=t.word(t.api.core_trace_pointer()>>>0,32+129*24);
      assert(countdown>=1&&countdown<=1024);
      const {value:warm}=t.run(warmSelected,63);
      assert.deepEqual([warm.retired,warm.work,warm.pc,warm.coreReason],[63,63,0x130a,2]);
      wanted.retired+=63;wanted.exitRetired=63;wanted.work=63;wanted.workTotal=totalWork+63*(replay+1);
      wanted.regions+=manual?1:28;wanted.id=warmSelected.id;wanted.selectedId=warmSelected.id;
      t.expect(wanted);
      const learned=(t.result.routes??[]).slice(routeBefore);
      const captures=(t.result.traceAttempts??[]).slice(captureBefore);
      const sampled=learned.filter(route=>route.trace.pending===1);
      const recording=learned.filter(route=>route.trace.active===1);
      const ordinary=learned.filter(route=>route.trace.pending===0&&route.trace.active===0);
      const pairs=[{pc:automatic?.pc,source:String(t.next.id),slot:automatic?.slot},
        {pc:0x1307,source:String(automatic?.id),slot:t.first.slot},
        {pc:crossPage?0x5340:0x1340,source:String(t.first.id),slot:t.next.slot}];
      for(const route of learned)assert(pairs.some(pair=>pair.pc===route.pc&&pair.source===route.source&&pair.slot===route.slot),
        'every native route retains an actual reciprocal callee/suffix/caller capability');
      if(!manual&&replay===0) {
        assert.equal(ordinary.length,1,'only the first replay validates the newly installed suffix boundary');
        assert([automatic.pc,0x1307].includes(ordinary[0].pc));
      } else assert.deepEqual(ordinary,[],'established ordinary routes never repeat native validation');
      const sampleBound=Math.floor((27+1024-countdown)/1024);
      assert(sampled.length<=sampleBound,'only the persistent 1024-route countdown permits a sampling miss');
      assert(captures.length<=1);
      for(const capture of captures) {
        assert.equal(capture.status,18,'already-owned precise observations never emit a replacement executor');
        assert.equal(capture.header[0],0);assert.equal(capture.header[2],129);assert.equal(capture.header[3],0);
        assert(capture.header[1]<=129);
        assert(sampled.length===1,'the observed warm capture is caused by actual generated sampling feedback');
        assert(capture.events.some(event=>event.kind===3),'the real PC64 precise operation remains explicit in the capture');
      }
      assert(recording.length<=captures.reduce((sum,capture)=>sum+capture.header[1],0),
        'active recording adds at most one native route per actual bounded event');
      assert.equal(learned.length,ordinary.length+sampled.length+recording.length);
      if(manual||replay===2) {
        if(!manual)assert(countdown>27,'the post-capture replay is inside the reset sampling interval');
        assert.deepEqual(learned,[],'the post-capture warm replay resolves entirely in generated Wasm');
      }
      assert.equal(resolverCalls()-resolves,learned.length);
      assert.equal(t.cache.stats().admissions,stats.admissions);
      assert.equal(t.cache.stats().generatedModuleBytes,stats.generatedModuleBytes);
      assert.equal(version(),beforeVersion+2n*BigInt(iterations+9*(replay+1)));
      warmResults.push({regions:warm.regions,nativeResolves:learned.length,countdown,
        sampled:sampled.length,capturedRoutes:recording.length,ordinaryRoutes:ordinary.length,
        captures:captures.map(capture=>({root:capture.args[0],events:capture.header[1],status:capture.status})),routes:learned});
    }
    t.result.graph={iterations,manual,publicSelection,initialOwners:[String(t.first.id),String(t.next.id)],
      admissionsBefore,admissionsAfter:t.cache.stats().admissions,warm:warmResults,
      trace:{kind:manual?'manual-root-only-trace':'automatic-precise-frontier',
        id:String(optimized.id),pc:optimized.pc,slot:optimized.slot,sha256:optimizedModule.sha256}};

    const callerPort=0x1307;t.status('core_lookup',0,callerPort);
    t.expect({...wanted,selectedId:t.first.id});
    if(crossPage) {
      t.engine.ok('write8',0x5346,0x90);t.expectedRam[12288+0x346]=0x90;
      assert.deepEqual(t.bytes(t.word(t.cache.context,64),t.expectedRam.length),t.expectedRam);
      t.neutral('stale callee lookup revokes source authority',()=>t.status('core_lookup',4,0x5340));
      if(overlay) {
        // Source validation is lazy per resolved owner. The root-only trace
        // has no public callee alias, so resolve its own root to revoke it and
        // select the unchanged caller without executing stale trace bytes.
        t.neutral('stale manual root falls back to the unchanged caller',()=>t.status('core_lookup',0,0x1300));
      } else {
        t.neutral('stale automatic suffix revokes its independent source authority',
          ()=>t.status('core_lookup',4,automatic.pc));
      }
      t.status('core_lookup',0,callerPort);
      t.neutral('unchanged caller remains executable',()=>t.status('core_lookup',0,callerPort));
      const owners=new DataView(t.engine.memory.buffer,t.api.core_link_owners_ptr()>>>0,4097*8);
      assert.equal(owners.getBigUint64(t.next.slot*8,true),0n);
      assert.equal(owners.getBigUint64(optimized.slot*8,true),0n,'the dependent optimized owner is revoked with changed callee text');
      assert.equal(owners.getBigUint64(t.first.slot*8,true),t.first.id);
      t.begin();t.end();
      const currentSourceVersion=new DataView(t.engine.memory.buffer).getBigUint64(t.word(t.cache.context,88),true);
      t.expect({...wanted,selectedId:t.first.id,sourceVersion:currentSourceVersion});
      const replacement=t.select(0x5340);
      assert.notEqual(replacement.id,t.next.id);
      assert.equal(t.cache.units.get(t.first.id),t.first);
      assert(!t.cache.units.has(t.next.id));
      assert(!t.cache.units.has(optimized.id));
      assert.deepEqual([t.cache.stats().admissions,t.cache.units.size],[4,2]);
      const stale=optimized;
      assert.equal(replacement.slot,stale.slot,'explicit replacement uses an actually revoked reciprocal slot');
      t.neutral('old identity cannot unlink a recycled slot',()=>t.status('core_unlink',4,stale.slot,
        Number(stale.id&0xffffffffn),Number(stale.id>>32n)));
      t.status('core_lookup',0,callerPort);
      assert.equal(hash(t.bytes(t.api.core_code_ptr()>>>0,t.api.core_code_len()>>>0)),
        originalModules.find(module=>module.id===String(t.first.id)).sha256);
      t.expect({...wanted,selectedId:t.first.id,sourceVersion:currentSourceVersion});
      assert.equal(version(),beforeVersion+2n*BigInt(iterations+27)+1n);
    }
  });
  report.complete = true; checkpoint(); console.log(JSON.stringify({complete:true,cases:report.cases.length,engineSha256:artifact.sha256}));
} catch (error) {report.failure={stage,message:String(error),stack:error.stack};checkpoint();throw error;}
