import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync,writeFileSync,openSync,closeSync} from 'node:fs';
import {readEngine} from '../../../support/wasm/engine.mjs';
import {Engine,KEY,record,view} from '../../../../../sdk/runtime/engine.js';
import {CodeCache} from '../../../../../sdk/runtime/cache.js';

const [enginePath,reportPath,baselinePath] = process.argv.slice(2);
assert(enginePath && reportPath,'usage: frontier-native.mjs engine.wasm fresh-report.json [no-frontier-engine.wasm]');
const artifact=readEngine(enginePath);
if(baselinePath)assert.match(process.env.RING3_BASELINE_ENGINE_SHA256??'',/^[a-f0-9]{64}$/,'pin the optional baseline engine');
const baseline=baselinePath?readEngine(baselinePath,process.env.RING3_BASELINE_ENGINE_SHA256):null;
const output=openSync(reportPath,'wx');closeSync(output);
const hex=bytes=>Buffer.from(bytes).toString('hex'),hash=bytes=>createHash('sha256').update(bytes).digest('hex');
const unhex=text=>Uint8Array.from(Buffer.from(text.replaceAll(' ',''),'hex'));
const sources=['./frontier-native.mjs','../../../support/wasm/engine.mjs','../../../../../sdk/runtime/engine.js','../../../../../sdk/runtime/cache.js'];
const report={schema:1,complete:false,engineSha256:artifact.sha256,baselineSha256:baseline?.sha256,
  scope:'authored runtime frontier reuse and encoded-module bytes; no timing or game claim',
  sources:Object.fromEntries(sources.map(path=>[path,hash(readFileSync(new URL(path,import.meta.url)))])),arms:[]};
const checkpoint=()=>writeFileSync(reportPath,JSON.stringify(report,null,2));
const code=new Uint8Array(4096),suffixInstructions=26,totalWork=28;
code.set(unhex('83c001 e9f8000000'));code.set(unhex('83c002 e9b8000000'),0x40);
for(let index=0;index<24;index++)code.set(unhex('830301'),0x100+index*3);
code.set(unhex('83c004 ffe2'),0x148);code.set(unhex('0f0b'),0x200);
const registers=[5,0,0x1200,0x3000,0x4800,0,0,0],banks=[[4236,352],[4612,184]],reference=new Map();
let stage='initialization';checkpoint();

function arm(selectedArtifact,chain,frontier) {
  const row={chain,frontier,engineSha256:selectedArtifact.sha256,modules:[],runs:[]};report.arms.push(row);checkpoint();
  const instance=new WebAssembly.Instance(selectedArtifact.module,{ring3_observability:{now_ns:()=>0n}});
  const api=Object.freeze(Object.fromEntries(Object.entries(instance.exports).filter(([name])=>name.startsWith('ring3_abi_v1_'))
    .map(([name,value])=>[name.slice(13),value])));
  const engine=new Engine(api,instance.exports.memory,selectedArtifact.sha256);
  const bytes=(pointer,length)=>new Uint8Array(engine.memory.buffer).slice(pointer,pointer+length);
  const word=(pointer,offset=0)=>new DataView(engine.memory.buffer).getUint32(pointer+offset,true);
  let opened=false,active=false,cache;
  try {
    engine.ok('open',16,...KEY);opened=true;engine.base=api.arena_ptr()>>>0;
    for(const [address,page] of [[0x1000,code],[0x3000,new Uint8Array(4096)],[0x4000,new Uint8Array(4096)]]) {
      engine.ok('map',address,1,address===0x1000?7:3);engine.request(page);engine.ok('upload',address,4096);
    }
    function initialState(pc) {
      const recordBytes=record('R3ST',56),d=view(recordBytes);
      registers.forEach((value,index)=>d.setUint32(16+index*4,value,true));d.setUint32(48,pc,true);d.setUint32(52,2,true);
      return recordBytes;
    }
    new Uint8Array(engine.memory.buffer,engine.base,56).set(initialState(0x1000));
    const initialBanks=banks.map(([offset,length])=>bytes(engine.base+offset,length));
    cache=new CodeCache(engine,{chain,budget:totalWork,instructions:128,blocks:8});
    for(const name of ['core_read','core_write','core_numeric',...(chain?['core_chain_next']:[])])
      assert.equal(cache.native[name],api[name],`original native ${name}`);
    function select(pc) {
      const unit=cache.entry(pc);
      if(!row.modules.some(item=>item.id===String(unit.id))) {
        const encoded=bytes(api.core_code_ptr()>>>0,api.core_code_len()>>>0);
        row.modules.push({pc,id:String(unit.id),bytes:encoded.length,sha256:hash(encoded),wasmHex:hex(encoded)});
      }
      return unit;
    }
    function counters() {
      const pointer=api.core_cache_status_ptr()>>>0;assert(pointer>0);
      const raw=bytes(pointer,64),d=view(raw);
      return {rawHex:hex(raw),values:Array.from({length:8},(_,index)=>d.getBigUint64(index*8,true).toString())};
    }
    const suffix=select(0x1100);cache.import();select(0x1000);select(0x1040);
    row.cold=counters();checkpoint();assert.equal(row.cold.values[0],'3','three genuine native modules');
    const totalBytes=row.modules.reduce((sum,item)=>sum+item.bytes,0);
    assert.equal(row.cold.values[1],String(totalBytes));assert.equal(row.cold.values[5],'3');assert.equal(row.cold.values[6],String(totalBytes));
    for(const [pc,eax,flags] of [[0x1000,10,6],[0x1040,11,2]]) {
      stage=`${frontier}/${chain}/caller-${pc.toString(16)}`;
      engine.request(new Uint8Array(4096));engine.ok('upload',0x3000,4096);
      new Uint8Array(engine.memory.buffer,engine.base,56).set(initialState(pc));cache.import();
      const head=select(pc),before=cache.stats(),priorRetired=cache.word(40),steps=[];
      let unit=head,work=0,retired=0,regions=0,stop;
      for(let outer=0;outer<3;outer++) {
        cache.budget=totalWork-work;
        const run=cache.execute(unit);work+=run.work;retired+=run.retired;regions+=run.regions;stop=run.coreReason;
        steps.push({...run,id:String(run.id),contextHex:hex(bytes(cache.context,128)),arenaHex:hex(bytes(engine.base,api.arena_len()))});
        row.pending={pc,steps};checkpoint();
        if(frontier && !chain && outer===0) {
          assert.equal(stop,1,'caller stops at the already retained suffix');assert.equal(run.pc,0x1100);
          assert.equal(run.retired,2);assert.equal(run.work,2);assert.equal(run.regions,1);assert.equal(run.id,head.id);
          assert.equal(cache.word(44),suffixInstructions);
          assert.equal(cache.word(40),priorRetired+2);assert.equal(cache.word(36),pc===0x1000?6:2);
          assert.deepEqual(Array.from({length:8},(_,index)=>cache.word(index*4)),[pc===0x1000?6:7,...registers.slice(1)]);
          const prefixRam=new Uint8Array(16*4096);prefixRam.set(code);
          assert.deepEqual(bytes(cache.word(64),prefixRam.length),prefixRam,'prefix has not executed any common-suffix store');
          assert.deepEqual(banks.map(([offset,length])=>bytes(engine.base+offset,length)),initialBanks);
        }
        if(stop===2)break;
        assert.equal(stop,1,'only a genuine cached frontier may continue');
        unit=select(cache.pc);
        assert.equal(unit,suffix,'the common suffix reuses its original native identity and generated function');
      }
      const rawContext=bytes(cache.context,128),d=view(rawContext),backing=d.getUint32(64,true),pages=d.getUint32(68,true);
      const arena=bytes(engine.base,api.arena_len()),ram=bytes(backing,pages*4096),after=cache.stats();
      const sample={pc,steps,stop,work,retired,regions,outer:after.runs-before.runs,contextHex:hex(rawContext),arenaHex:hex(arena),backingHex:hex(ram)};
      row.runs.push(sample);delete row.pending;checkpoint();
      assert.equal(stop,2);assert.equal(work,totalWork);assert.equal(retired,totalWork);
      assert.equal(regions,frontier?2:1);assert.equal(sample.outer,frontier&&!chain?2:1);
      assert.equal(d.getUint32(40,true),priorRetired+totalWork);assert.equal(d.getUint32(44,true),0);assert.equal(d.getUint32(112,true),0);
      assert.equal(d.getBigUint64(120,true),frontier?suffix.id:head.id);
      assert.deepEqual(Array.from({length:8},(_,index)=>d.getUint32(index*4,true)),[eax,...registers.slice(1)]);
      assert.equal(d.getUint32(32,true),0x1200);assert.equal(d.getUint32(36,true),flags);
      for(const offset of [48,52,116])assert.equal(d.getUint32(offset,true),0);
      const state=initialState(0x1200),s=view(state);s.setUint32(16,eax,true);s.setUint32(52,flags,true);
      assert.deepEqual(arena.slice(0,56),state,'complete literal final State32');
      const exit=record('R3EX',40),e=view(exit);e.setUint16(4,15,true);e.setUint32(16,1,true);e.setUint32(20,totalWork,true);
      assert.deepEqual(arena.slice(56,96),exit);const completed=record('R3WK',24);view(completed).setUint32(16,totalWork,true);
      assert.deepEqual(arena.slice(4588,4612),completed);
      assert.deepEqual(banks.map(([offset,length])=>arena.slice(offset,offset+length)),initialBanks,'entire numerical and segment banks');
      assert.equal(pages,16);const mappings=d.getUint32(56,true);
      assert.deepEqual([1,3,4].map(page=>word(mappings+page*4)),[1,2,3]);
      const expectedRam=new Uint8Array(16*4096);expectedRam.set(code);view(expectedRam).setUint32(4096,24,true);
      assert.deepEqual(ram,expectedRam,'complete unchanged code/stack and literal suffix RAM output');
      if(!reference.has(pc))reference.set(pc,{arena,ram});
      else {assert.deepEqual(arena,reference.get(pc).arena,'same whole canonical arena');assert.deepEqual(ram,reference.get(pc).ram,'same whole canonical backing');}
      const status=counters();sample.nativeCache=status;assert.equal(status.values[0],'3','execution never recompiles the common suffix');
      assert.equal(status.values[1],String(totalBytes));assert.equal(cache.stats().admissions,3);
    }
    row.complete=true;checkpoint();return row;
  } catch(error) {row.failure={stage,message:String(error),stack:error.stack};checkpoint();throw error;}
  finally {if(active||cache?.open)api.core_end();if(opened)engine.ok('close');}
}

try {
  const direct=arm(artifact,false,true),chained=arm(artifact,true,true);
  assert.deepEqual(chained.modules,direct.modules,'chaining keeps identical frontier-generated module bytes and identities');
  if(baseline) {
    const copied=arm(baseline,true,false);
    assert.equal(copied.modules[0].sha256,chained.modules[0].sha256,'the original common suffix lowering is identical');
    for(const pc of [0x1000,0x1040]) {
      const old=copied.modules.find(item=>item.pc===pc),fresh=chained.modules.find(item=>item.pc===pc);
      assert(fresh.bytes<old.bytes,'frontier excludes the duplicate suffix from each caller module');
    }
    report.encodedBytes={frontier:Number(chained.cold.values[1]),copied:Number(copied.cold.values[1])};
    assert(report.encodedBytes.frontier<report.encodedBytes.copied);
  }
  report.complete=true;checkpoint();console.log(JSON.stringify({complete:true,arms:report.arms.length,encodedBytes:report.encodedBytes}));
} catch(error) {report.failure={stage,message:String(error),stack:error.stack};checkpoint();throw error;}
