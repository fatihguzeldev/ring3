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
  scope:'authored native cached-region handoff authority and precise prefixes; no timing or game claim',
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

function fixture(name,{entry=0x1000,target=0x1040,registers=[0,0,0x1080,0x3000,0x4800,0,0x1040,0],stack=true,data=()=>{}}={},body) {
  stage = name;
  const result = {name,modules:[],controls:[],runs:[]}; report.cases.push(result); checkpoint();
  const instance = new WebAssembly.Instance(artifact.module,{ring3_observability:{now_ns:()=>0n}});
  const api = Object.freeze(Object.fromEntries(Object.entries(instance.exports).filter(([name]) => name.startsWith('ring3_abi_v1_'))
    .map(([name,value]) => [name.slice(13),value])));
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
    const pages = [[0x1000,code],[0x3000,dataPage],...(stack ? [[0x4000,new Uint8Array(4096)]] : [])];
    for (const [address,page] of pages) {
      engine.ok('map',address,1,address === 0x1000 ? 7 : 3); engine.request(page); engine.ok('upload',address,4096);
    }
    const state = record('R3ST',56), d = view(state);
    registers.forEach((value,index) => d.setUint32(16+index*4,value,true));
    d.setUint32(48,entry,true); d.setUint32(52,2,true); new Uint8Array(engine.memory.buffer,engine.base,56).set(state);
    const initialBanks = bankRanges.map(([offset,length]) => bytes(engine.base+offset,length));
    const initialFp = bytes(engine.base+4236,128);
    cache = new CodeCache(engine,{chain:true,budget:8,instructions:64,blocks:8});
    function select(pc=cache.pc) {
      const unit = cache.entry(pc);
      if (!result.modules.some(item => item.id === String(unit.id))) {
        const encoded = bytes(api.core_code_ptr()>>>0,api.core_code_len()>>>0);
        result.modules.push({pc,id:String(unit.id),slot:unit.slot,sha256:hash(encoded),wasmHex:hex(encoded)});
      }
      return unit;
    }
    const first = select(entry), next = select(target); select(entry);
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
      result.runs.push({...value,id:String(value.id),budget,sourceVersion:String(sourceVersion),...snapshot()});
      // a stale entry may be refreshed by execute before its first region call.
      const selected = cache.units.get(value.id);
      if (selected && !result.modules.some(item => item.id === String(selected.id))) {
        const encoded = bytes(api.core_code_ptr()>>>0,api.core_code_len()>>>0);
        result.modules.push({pc:selected.pc,id:String(selected.id),slot:selected.slot,sha256:hash(encoded),wasmHex:hex(encoded)});
      }
      checkpoint(); return {value,sourceVersion};
    }
    function expect({stop,pc,retired,work,remaining,regions,id,regs=registers,flags=2,fault=[0,0,0],wantedFp=initialFp,sourceVersion}) {
      const last = result.runs.at(-1); assert.equal(last.coreReason,stop);
      const expectedContext = initialContext.slice(), c = view(expectedContext);
      regs.forEach((value,index) => c.setUint32(index*4,value,true));
      for (const [offset,value] of [[32,pc],[36,flags],[40,retired],[44,remaining],[48,fault[0]],[52,fault[1]],[112,0],[116,fault[2]]]) c.setUint32(offset,value,true);
      c.setBigUint64(96,sourceVersion,true); c.setBigUint64(120,id,true);
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
    body({api,engine,cache,bytes,word,select,first,next,initialFp,initialBanks,expectedRam,snapshot,neutral,status,run,expect,result,
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
  fixture('stale-target-miss-and-cold-refresh',{data:values},t => {
    t.engine.ok('write8',0x1041,0x43); t.expectedRam[0x41]=0x43;
    const firstRun = t.run(t.first,5);
    assert.equal(firstRun.value.coreReason,1); assert.equal(firstRun.value.pc,0x1040);
    assert.equal(firstRun.value.retired,2); assert.equal(firstRun.value.work,2); assert.equal(firstRun.value.regions,1);
    assert.notEqual(firstRun.value.id,t.first.id); assert.notEqual(firstRun.value.id,t.next.id);
    assert.equal(t.cache.units.get(firstRun.value.id).pc,0x1000);
    assert.deepEqual(t.bytes(t.engine.base+4236,128),t.initialFp,'NeedCode keeps the dirty numerical bank private');
    const replacement = t.select(); assert.notEqual(replacement.id,t.next.id);
    const {sourceVersion} = t.run(replacement,3); t.expectedRam.set(unhex('0000a040'),4096+8);
    t.expect({stop:2,pc:0x1080,retired:5,work:5,remaining:0,regions:2,id:replacement.id,sourceVersion,
      wantedFp:fp(t.initialFp,{opcode:0x15b,fip:0x1043,fdp:0x3008,registers:{7:'00000000000000a00140'}})});
    assert.equal(t.cache.stats().runs,2);
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
    t.expect({stop:3,pc:0x1100,retired:0,work:1,remaining:7,regions:1,id:t.first.id,sourceVersion,fault:[0x5000,0x10201,14],
      regs:[5,2,0x1140,0x3000,0x4800,0,0x3001,0x5000]});
  });
  fixture('code-write-prevents-next-region',{entry:0x1200,target:0x1140,registers:[0x11223344,0,0,0x1200,0x4800,0,0x1140,0]},t => {
    const {sourceVersion} = t.run(t.first,8); t.expectedRam.set(unhex('44332211'),0x200);
    t.expect({stop:6,pc:0x1202,retired:1,work:1,remaining:7,regions:1,id:t.first.id,sourceVersion});
    t.neutral('code writer cannot reenter stale source',()=>t.status('core_begin',4));
  });
  const iterations=8192,totalWork=7*iterations;
  fixture('numerical-ordinals-survive-hot-region-promotion',{
    entry:0x1300,target:0x1340,registers:[0,iterations,0,0x3000,0x4800,0,0,0],data:values,
  },t => {
    const admissionsBefore=t.cache.stats().admissions;
    let selected=t.first,work=0,retired=0,last,sourceVersion;
    while(work<totalWork) {
      assert(t.result.runs.length<2*iterations+2,'bounded authored continuation count');
      const executed=t.run(selected,totalWork-work);
      last=executed.value;sourceVersion=executed.sourceVersion;work+=last.work;retired+=last.retired;
      if(last.coreReason===2)break;
      assert.equal(last.coreReason,1,'promotion continuation is a genuine NeedCode');
      assert.deepEqual(t.bytes(t.engine.base+4236,128),t.initialFp,'dirty numerical bank remains private across promotion');
      selected=t.select();
    }
    t.expectedRam.set(unhex('0000c040'),4096+8);
    t.expectedRam.set(unhex('07130000'),8192+0x7fc);
    const stats=t.cache.stats();
    t.expect({stop:2,pc:0x130a,retired:totalWork,work:totalWork,remaining:0,regions:stats.regions,id:last.id,sourceVersion,
      regs:[0,0,0,0x3000,0x4800,0,0,0],flags:0x46,
      wantedFp:fp(t.initialFp,{opcode:0x15b,fip:0x1343,fdp:0x3008,registers:{7:'00000000000000c00140'}})});
    t.result.promotion={iterations,retired,work,admissionsBefore,admissionsAfter:stats.admissions,
      regions:stats.regions,outer:stats.runs,initialOwners:[String(t.first.id),String(t.next.id)],finalOwner:String(last.id)};
    checkpoint();
    assert.equal(retired,totalWork);assert.equal(work,totalWork);
    assert(stats.admissions>admissionsBefore,'hot transition admits a new generated ordinal table');
    assert(stats.regions>1&&stats.regions<2*iterations+1,'actual region entries fall after promotion');
    assert(stats.runs>=2,'the first promotion returns through a closed execution boundary');
    assert.notEqual(last.id,t.first.id);assert.notEqual(last.id,t.next.id);
  });
  report.complete = true; checkpoint(); console.log(JSON.stringify({complete:true,cases:report.cases.length,engineSha256:artifact.sha256}));
} catch (error) {report.failure={stage,message:String(error),stack:error.stack};checkpoint();throw error;}
