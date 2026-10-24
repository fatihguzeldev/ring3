import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync,writeFileSync,openSync,closeSync} from 'node:fs';
import {readEngine} from '../../../support/wasm/engine.mjs';
import {Engine,KEY,record,view} from '../../../../../sdk/runtime/engine.js';
import {CodeCache} from '../../../../../sdk/runtime/cache.js';
import {readGeneratedModule} from './region/structure.mjs';
import {instrumentWitnessModule,readWitness} from './region/witness.mjs';

const [enginePath,reportPath]=process.argv.slice(2);
assert(enginePath&&reportPath,'usage: continuations.mjs engine.wasm fresh-report.json');
const artifact=readEngine(enginePath),hash=b=>createHash('sha256').update(b).digest('hex');
closeSync(openSync(reportPath,'wx'));
const report={schema:1,complete:false,engineSha256:artifact.sha256,
  scope:'actual immutable fragments, generated warm routing and one terminal canonical publication; no timing claim',
  sources:Object.fromEntries(['./continuations.mjs','./region/structure.mjs','./region/witness.mjs',
    '../../../../../sdk/runtime/cache.js'].map(path=>[path,hash(readFileSync(new URL(path,import.meta.url)))])),cases:[]};
const save=()=>writeFileSync(reportPath,JSON.stringify(report,null,2));save();
const instance=new WebAssembly.Instance(artifact.module,{ring3_observability:{now_ns:()=>0n}});
const raw=Object.fromEntries(Object.entries(instance.exports).filter(([n])=>n.startsWith('ring3_abi_v1_')).map(([n,v])=>[n.slice(13),v]));
const memory=instance.exports.memory,bytes=(p,n)=>new Uint8Array(memory.buffer).slice(p,p+n);
let probes=0,opened=false,cache;
const resolver=raw.core_chain_resolve_owned?'core_chain_resolve_owned':'core_chain_select';
const api={...raw,[resolver]:(...args)=>{probes++;return raw[resolver](...args);}};
const engine=new Engine(api,memory,artifact.sha256),word=(p,o=0)=>new DataView(memory.buffer).getUint32(p+o,true);
const initial=[10,20,30,0x3000,0x4800,0x1000,0x1040,0x1080];
try {
  engine.ok('open',12,...KEY);opened=true;engine.base=raw.arena_ptr()>>>0;
  const page=new Uint8Array(4096);
  for(const [pc,hex]of[[0x1000,'8d4001ffe6'],[0x1040,'8d4901ffe7'],[0x1080,'8d5201ffe5']])page.set(Buffer.from(hex,'hex'),pc-0x1000);
  for(const [address,payload]of[[0x1000,page],[0x3000,new Uint8Array(4096)],[0x4000,new Uint8Array(4096)]]) {
    engine.ok('map',address,1,address===0x1000?7:3);engine.request(payload);engine.ok('upload',address,4096);
  }
  const state=record('R3ST',56),s=view(state);
  initial.forEach((v,i)=>s.setUint32(16+4*i,v,true));s.setUint32(48,0x1000,true);s.setUint32(52,0x403,true);
  new Uint8Array(memory.buffer,engine.base,56).set(state);
  const numeric=bytes(engine.base+4236,352),upper=bytes(engine.base+4612,184);
  cache=new CodeCache(engine,{chain:true,budget:6,instructions:32,blocks:8});
  const modules=[];
  for(const pc of[0x1080,0x1040,0x1000]) {
    const unit=cache.entry(pc),source=bytes(raw.core_code_ptr()>>>0,raw.core_code_len()>>>0);
    const diagnostic=instrumentWitnessModule(source,readGeneratedModule(source));
    const exports=new WebAssembly.Instance(new WebAssembly.Module(diagnostic.bytes),{ring3:{...cache.dispatch,
      memory,read:api.core_read,write:api.core_write,precise:ctx=>cache.invokePrecise(ctx),
      'support.abi':cache.dispatch.supportAbi,'recovery.base':unit.recoveryBase}}).exports;
    Object.assign(unit,{run:exports.run,link:exports.link,publish:exports.publish});
    cache.table.set(unit.slot,unit.link);cache.publishers.set(unit.slot,unit.publish);
    modules.push({unit,exports,source,sha256:hash(source)});
  }
  cache.import();engine.ok('core_begin');engine.ok('core_end');
  const ctx=cache.context,baseline=bytes(ctx,128),backing=word(ctx,64),ram=bytes(backing,12*4096);
  const stampPointer=word(ctx,80),revision=new DataView(memory.buffer).getBigUint64(stampPointer,true);
  const publications=()=>modules.reduce((n,m)=>n+readWitness(m.exports).canonical_publish_calls,0);
  for(let run=1;run<=2;run++) {
    const row={name:run===1?'learn-two-actual-edges-without-reemitting-fragments':'warm-cross-owner-loop-without-native-resolution',complete:false};
    report.cases.push(row);save();
    const beforeProbes=probes,beforePublications=publications(),first=cache.entry(0x1000);
    const result=cache.execute(first),expected=baseline.slice(),v=view(expected);
    const gpr=initial.map((value,i)=>i<3?value+run:value);
    gpr.forEach((value,i)=>v.setUint32(4*i,value,true));
    for(const [offset,value]of[[32,0x1000],[36,0x403],[40,run*6],[44,0],[48,0],[52,0],[112,0],[116,0]])v.setUint32(offset,value,true);
    v.setBigUint64(120,modules[0].unit.id,true);
    assert.deepEqual(bytes(ctx,128),expected,'all128 context bytes and actual finishing owner');
    assert.deepEqual([result.coreReason,result.pc,result.retired,result.work,result.regions],[2,0x1000,6,6,3]);
    assert.deepEqual(bytes(engine.base+4236,352),numeric,'all FP/SIMD/segment bytes');
    assert.deepEqual(bytes(engine.base+4612,184),upper,'all upper vectors/segment bases');
    assert.deepEqual(bytes(backing,ram.length),ram,'entire allocated RAM');
    assert.equal(new DataView(memory.buffer).getBigUint64(stampPointer,true),revision);
    const publicState=state.slice(),sv=view(publicState);gpr.forEach((value,i)=>sv.setUint32(16+4*i,value,true));
    assert.deepEqual(bytes(engine.base,56),publicState,'literal whole published State32');
    assert.equal(cache.admissions,3);assert.equal(cache.units.size,3);
    for(const m of modules) {
      assert.equal(cache.units.get(m.unit.id),m.unit,'learning an edge preserves the actual old executable object');
      cache.entry(m.unit.pc);
      assert.equal(hash(bytes(raw.core_code_ptr()>>>0,raw.core_code_len()>>>0)),m.sha256,'learning edges preserves old Wasm bytes');
    }
    row.complete=true;row.semanticComplete=true;row.contextSha256=hash(expected);
    row.nativeResolverCalls=probes-beforeProbes;row.canonicalPublications=publications()-beforePublications;
    row.modules=modules.map(m=>({id:String(m.unit.id),sha256:m.sha256,bytes:m.source.length}));save();
  }
  const warm=report.cases[1];
  assert.equal(warm.nativeResolverCalls,0,'the warm ordinary cross-owner path executes no native resolver');
  assert.equal(warm.canonicalPublications,1,'only the final terminal boundary publishes canonical state');
  assert.equal(typeof raw.core_chain_resolve_owned,'function','actual bound continuation authority is available');
  report.complete=true;save();
} catch(error) {report.failure={message:String(error),stack:error.stack};save();throw error;}
finally {if(cache?.open)raw.core_end();if(opened)engine.ok('close');}
