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

function fixture(name,{entry=0x1000,target=0x1040,registers=[0,0,0x1080,0x3000,0x4800,0,0x1040,0],stack=true,data=()=>{},codePage=code,extraCodePages=[],firstInstructions=64,traceSelection=false,calleeFirst=false}={},body) {
  stage = name;
  const result = {name,modules:[],controls:[],runs:[]}; report.cases.push(result); checkpoint();
  const instance = new WebAssembly.Instance(artifact.module,{ring3_observability:{now_ns:()=>0n}});
  const native = Object.fromEntries(Object.entries(instance.exports).filter(([name]) => name.startsWith('ring3_abi_v1_'))
    .map(([name,value]) => [name.slice(13),value]));
  const selectionCalls = {core_prepare:0,core_lookup:0,core_cold_run:0};
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
    native[name] = (...args) => {selectionCalls[name]++; return operation(...args);};
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
    function expect({stop,pc,retired,work,remaining,regions,id,selectedId=id,regs=registers,flags=2,fault=[0,0,0],wantedFp=initialFp,sourceVersion}) {
      const last = result.runs.at(-1); assert.equal(last.coreReason,stop);
      assert.equal(last.id,id===0n?null:String(id),'the returned identity names the owner that actually finished guest execution');
      const expectedContext = initialContext.slice(), c = view(expectedContext);
      regs.forEach((value,index) => c.setUint32(index*4,value,true));
      for (const [offset,value] of [[32,pc],[36,flags],[40,retired],[44,remaining],[48,fault[0]],[52,fault[1]],[112,0],[116,fault[2]]]) c.setUint32(offset,value,true);
      c.setBigUint64(96,sourceVersion,true); c.setBigUint64(120,selectedId,true);
      assert.deepEqual(bytes(cache.context,128),expectedContext,'full literal Context32 and selected owner');
      assert.equal(cache.stats().regions,regions); assert.equal(cache.stats().retired,retired); assert.equal(cache.stats().work,work);
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
      e.setUint32(16,stop === 2 ? 1 : stop === 6 ? 6 : 5,true); e.setUint32(20,retired,true);
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
  const iterations=8192,totalWork=7*iterations;
  const crossPageCode=code.slice(),crossPageCallee=new Uint8Array(4096);
  crossPageCode.set(unhex('d903 e839400000 49 75f6 0f0b'),0x300);
  crossPageCallee.set(unhex('d84b04 d95b08 c3'),0x340);
  for (const [publicSelection,crossPage] of [[false,false],[true,false],[true,true]]) fixture(crossPage
    ? 'public-selection-cross-page-numerical-graph-authority'
    : publicSelection ? 'public-selection-preserves-warmed-numerical-graph'
      : 'numerical-ordinals-survive-warmed-graph',{
    entry:0x1300,target:crossPage?0x5340:0x1340,registers:[0,iterations,0,0x3000,0x4800,0,0,0],data:values,
    // Keep caller and callee as independent owners beyond the former heat threshold.
    firstInstructions:5,traceSelection:true,calleeFirst:true,
    ...(crossPage?{codePage:crossPageCode,extraCodePages:[[0x5000,crossPageCallee]]}:{}),
  },t => {
    const admissionsBefore=t.cache.stats().admissions;
    const callsBefore={...t.selectionCalls},originalModules=t.result.modules.slice();
    const originalExports=[t.first,t.next].map(unit=>({unit,run:unit.run,link:unit.link,publish:unit.publish}));
    const ownerPointer=t.api.core_link_owners_ptr()>>>0,originalOwners=t.bytes(ownerPointer,4097*8);
    const version=()=>new DataView(t.engine.memory.buffer).getBigUint64(t.word(t.cache.context,80),true);
    const beforeVersion=version();
    const selected=publicSelection?t.cache.select():t.first;
    assert.equal(selected,t.first,'public retained selection returns the original caller instance');
    const {value:last,sourceVersion}=t.run(selected,totalWork);
    t.expectedRam.set(unhex('0000c040'),4096+8);
    t.expectedRam.set(unhex('07130000'),8192+0x7fc);
    const stats=t.cache.stats();
    t.result.executionRoutes={preciseRuns:stats.preciseRuns,preciseWork:stats.preciseWork,
      preciseRetired:stats.preciseRetired,ready:t.api.core_trace_ready(),root:t.api.core_trace_root()};checkpoint();
    const overlays=[...t.cache.units.values()].filter(unit=>unit!==t.first&&unit!==t.next);
    assert.equal(overlays.length,1,'normal post-lease recording installs exactly one separate observed executor');
    const overlay=overlays[0],overlayModule=t.result.modules.find(module=>module.id===String(overlay.id));
    assert(overlayModule,'the automatically installed executor exposes its actual encoded module');
    assert([0x1300,0x1307,crossPage?0x5340:0x1340].includes(overlay.pc),'the executor root is an actual entry of this graph');
    assert.notEqual(overlay.slot,t.first.slot);assert.notEqual(overlay.slot,t.next.slot);
    const wantedOwners=originalOwners.slice();view(wantedOwners).setBigUint64(overlay.slot*8,overlay.id,true);
    assert.deepEqual(t.bytes(ownerPointer,4097*8),wantedOwners,
      'both original reciprocal slots survive; only the separately installed executor slot is added');
    // One initial caller activation, then one callee and one returned caller per CALL.
    const wanted={stop:2,pc:0x130a,retired:totalWork,work:totalWork,remaining:0,regions:2*iterations+1,id:t.first.id,selectedId:overlay.id,sourceVersion,
      regs:[0,0,0,0x3000,0x4800,0,0,0],flags:0x46,
      wantedFp:fp(t.initialFp,{opcode:0x15b,fip:crossPage?0x5343:0x1343,fdp:0x3008,registers:{7:'00000000000000c00140'}})};
    t.expect(wanted);
    t.result.graph={iterations,retired:last.retired,work:last.work,admissionsBefore,admissionsAfter:stats.admissions,
      publicSelection,coldRuns:stats.coldRuns,coldRetired:stats.coldRetired,coldWork:stats.coldWork,
      regions:stats.regions,outer:stats.runs,initialOwners:[String(t.first.id),String(t.next.id)],finalOwner:String(last.id),
      preparedOverlay:{id:String(overlay.id),pc:overlay.pc,slot:overlay.slot,bytes:overlay.bytes,sha256:overlayModule.sha256}};
    checkpoint();
    assert.deepEqual([last.retired,last.work,last.coreReason],[totalWork,totalWork,2]);
    assert.equal(last.id,t.first.id,'the final DEC/JNZ belongs to the original caller');
    assert.equal(t.result.runs.length,1,'known graph edges never request outer preparation');
    assert.deepEqual([stats.runs,stats.coldRuns,stats.coldRetired,stats.coldWork],[1,0,0,0]);
    assert.deepEqual([stats.admissions,stats.installedUnits,stats.transientAdmissions,stats.transientRuns],[3,3,0,0]);
    assert.equal(stats.admissions,admissionsBefore+1);
    assert.equal(stats.generatedModuleBytes,originalModules.reduce((sum,module)=>sum+module.wasmHex.length/2,0)+overlay.bytes);
    assert.deepEqual(t.result.modules.filter(module=>module.id!==String(overlay.id)),originalModules,
      'the added executor does not replace or re-emit either original source owner');
    assert.equal(t.result.modules.length,originalModules.length+1);
    assert.deepEqual(t.selectionCalls,{core_prepare:callsBefore.core_prepare,
      core_lookup:callsBefore.core_lookup+Number(publicSelection),core_cold_run:callsBefore.core_cold_run},
      'retained public selection uses lookup; post-lease installation invokes neither baseline preparation nor a cold lease');
    assert.equal(version(),beforeVersion+2n*BigInt(iterations),'each CALL and FSTP commits exactly one write');
    const familyExports={run:overlay.run,link:overlay.link,publish:overlay.publish,slot:overlay.slot};
    for(const pc of[0x1300,0x1302,0x1307,0x1308,...(crossPage?[0x5340,0x5343,0x5346]:[0x1340,0x1343,0x1346])]){
      assert.equal(t.cache.select(pc),overlay,'every actually observed instruction port selects the one family object');
      assert.equal(new DataView(t.engine.memory.buffer).getBigUint64(t.cache.context+120,true),overlay.id,
        'every family port selects the same native ordinal dictionary');
      assert.equal(hash(t.bytes(t.api.core_code_ptr()>>>0,t.api.core_code_len()>>>0)),overlayModule.sha256,
        'secondary ports expose the same immutable emitted body');
      assert.equal(overlay.slot,familyExports.slot);assert.equal(overlay.run,familyExports.run);
      assert.equal(overlay.link,familyExports.link);assert.equal(overlay.publish,familyExports.publish);
      assert.equal(t.cache.table.get(overlay.slot),familyExports.link);
      assert.equal(t.cache.publishers.get(overlay.slot),familyExports.publish);
    }
    for(const unit of[t.first,t.next]){
      assert.equal(t.cache.units.get(unit.id),unit,'both immutable baseline SDK instances remain retained');
      assert.equal(new DataView(t.engine.memory.buffer,ownerPointer,4097*8).getBigUint64(unit.slot*8,true),unit.id);
    }
    for(const saved of originalExports){assert.equal(saved.unit.run,saved.run);assert.equal(saved.unit.link,saved.link);
      assert.equal(saved.unit.publish,saved.publish);assert.equal(t.cache.table.get(saved.unit.slot),saved.link);
      assert.equal(t.cache.publishers.get(saved.unit.slot),saved.publish);}
    assert.equal(t.cache.stats().admissions,stats.admissions,'family port selection admits no new body');
    assert.equal(t.cache.stats().generatedModuleBytes,stats.generatedModuleBytes);
    assert.equal(t.cache.units.size,3);
    const callerPort=overlay.pc===0x1300?0x1307:0x1300;
    t.status('core_lookup',0,callerPort);
    t.expect({...wanted,selectedId:overlay.id});
    if(crossPage) {
      t.engine.ok('write8',0x5346,0x90);t.expectedRam[12288+0x346]=0x90;
      assert.deepEqual(t.bytes(t.word(t.cache.context,64),t.expectedRam.length),t.expectedRam,
        'second code-page mutation is the only backing change');
      t.neutral('stale callee lookup rejects its source owner and dependent executor',()=>t.status('core_lookup',4,0x5340));
      t.status('core_lookup',0,callerPort);
      t.neutral('unchanged caller remains executable',()=>t.status('core_lookup',0,callerPort));
      const owners=new DataView(t.engine.memory.buffer,t.api.core_link_owners_ptr()>>>0,4097*8);
      assert.equal(owners.getBigUint64(t.next.slot*8,true),0n,'stale callee reciprocal slot is pruned');
      assert.equal(owners.getBigUint64(overlay.slot*8,true),0n,'the executor depending on changed callee bytes is also pruned');
      assert.equal(owners.getBigUint64(t.first.slot*8,true),t.first.id,'caller authority remains retained');
      t.begin();t.end();
      const currentSourceVersion=new DataView(t.engine.memory.buffer).getBigUint64(t.word(t.cache.context,88),true);
      t.expect({...wanted,selectedId:t.first.id,sourceVersion:currentSourceVersion});
      // Explicit compatibility preparation still exercises source authority and
      // slot reuse; ordinary stale execution above never requests this compile.
      const replacement=t.select(0x5340);
      assert.notEqual(replacement.id,t.next.id,'explicit preparation of changed callee source creates a fresh owner');
      assert.equal(t.cache.units.get(t.first.id),t.first,'callee refresh preserves the caller SDK instance');
      assert(!t.cache.units.has(t.next.id),'the stale callee SDK instance is removed');
      assert(!t.cache.units.has(overlay.id),'the stale dependent executor SDK instance is removed');
      assert.deepEqual([t.cache.stats().admissions,t.cache.units.size],[4,2]);
      assert.equal(replacement.slot,overlay.slot,'the replacement really reuses the former executor slot');
      t.neutral('old executor identity cannot unlink a recycled slot',()=>
        t.status('core_unlink',4,overlay.slot,Number(overlay.id&0xffffffffn),Number(overlay.id>>32n)));
      t.status('core_lookup',0,callerPort);
      assert.equal(hash(t.bytes(t.api.core_code_ptr()>>>0,t.api.core_code_len()>>>0)),
        originalModules.find(module=>module.id===String(t.first.id)).sha256);
      t.expect({...wanted,selectedId:t.first.id,sourceVersion:currentSourceVersion});
      assert.equal(version(),beforeVersion+2n*BigInt(iterations)+1n,'source mutation is the only additional committed write');
    }
  });
  report.complete = true; checkpoint(); console.log(JSON.stringify({complete:true,cases:report.cases.length,engineSha256:artifact.sha256}));
} catch (error) {report.failure={stage,message:String(error),stack:error.stack};checkpoint();throw error;}
