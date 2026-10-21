import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync,writeFileSync,openSync,closeSync} from 'node:fs';
import {readEngine} from '../../../support/wasm/engine.mjs';
import {Engine,KEY,record,view} from '../../../../../sdk/runtime/engine.js';
import {CodeCache} from '../../../../../sdk/runtime/cache.js';

const [enginePath,reportPath]=process.argv.slice(2);
assert(enginePath&&reportPath,'usage: mixed-native.mjs engine.wasm fresh-report.json');
const artifact=readEngine(enginePath),hash=bytes=>createHash('sha256').update(bytes).digest('hex');
const hex=bytes=>Buffer.from(bytes).toString('hex'),unhex=text=>Uint8Array.from(Buffer.from(text.replaceAll(' ',''),'hex'));
closeSync(openSync(reportPath,'wx'));
const report={schema:1,complete:false,engineSha256:artifact.sha256,
  scope:'one original dispatcher invocation crosses compiled integer, interpreted FLD1 and compiled native ordinal0; private bank before explicit publication; no timing claim',
  sources:Object.fromEntries(['./mixed-native.mjs','../../../support/wasm/engine.mjs',
    '../../../../../sdk/runtime/engine.js','../../../../../sdk/runtime/cache.js'].map(path=>[path,hash(readFileSync(new URL(path,import.meta.url)))])),modules:[]};
const checkpoint=()=>writeFileSync(reportPath,JSON.stringify(report,null,2));
const instance=new WebAssembly.Instance(artifact.module,{ring3_observability:{now_ns:()=>0n}});
const api=Object.freeze(Object.fromEntries(Object.entries(instance.exports).filter(([name])=>name.startsWith('ring3_abi_v1_'))
  .map(([name,value])=>[name.slice(13),value])));
const engine=new Engine(api,instance.exports.memory,artifact.sha256);
const bytes=(pointer,length)=>new Uint8Array(engine.memory.buffer).slice(pointer,pointer+length);
const word=(pointer,offset=0)=>new DataView(engine.memory.buffer).getUint32(pointer+offset,true);
let opened=false,active=false,context=0,backing=0,stage='setup';
const raw=()=>({contextHex:context?hex(bytes(context,128)):null,arenaHex:hex(bytes(engine.base,api.arena_len())),
  backingHex:backing?hex(bytes(backing,16*4096)):null});
try {
  engine.ok('open',16,...KEY);opened=true;engine.base=api.arena_ptr()>>>0;
  const code=new Uint8Array(4096),data=new Uint8Array(4096),stack=new Uint8Array(4096);
  code.set(unhex('40 ffe6'));code.set(unhex('d9e8 ffe7'),0x20);code.set(unhex('d95b08 ffe2'),0x40);
  const pages=[[0x1000,code,7],[0x3000,data,3],[0x4000,stack,3]];
  for(const [address,page,permissions] of pages) {engine.ok('map',address,1,permissions);engine.request(page);engine.ok('upload',address,4096);}
  const registers=[7,0,0x1080,0x3000,0x4800,0,0x1020,0x1040],state=record('R3ST',56);
  registers.forEach((value,index)=>view(state).setUint32(16+index*4,value,true));
  view(state).setUint32(48,0x1000,true);view(state).setUint32(52,2,true);
  new Uint8Array(engine.memory.buffer,engine.base,56).set(state);
  const cache=new CodeCache(engine,{chain:true,budget:6,instructions:2,blocks:2});
  for(const name of ['core_read','core_write','core_numeric','core_chain_next'])assert.equal(cache.native[name],api[name]);
  const select=pc=>{
    const unit=cache.entry(pc),encoded=bytes(api.core_code_ptr()>>>0,api.core_code_len()>>>0);
    const imports=WebAssembly.Module.imports(new WebAssembly.Module(encoded));
    report.modules.push({pc,id:String(unit.id),slot:unit.slot,imports,wasmHex:hex(encoded),sha256:hash(encoded)});
    return unit;
  };
  const first=select(0x1000),target=select(0x1040);assert.equal(cache.entry(0x1000),first);
  assert(!report.modules[0].imports.some(item=>item.name==='numeric'),'integer owner has no native ordinal');
  assert(report.modules[1].imports.some(item=>item.name==='numeric'),'target owns its actual numerical ordinal');
  context=cache.context;engine.ok('core_begin');active=true;engine.ok('core_end');active=false;
  const initialContext=bytes(context,128),initialArena=bytes(engine.base,api.arena_len());
  backing=word(context,64);assert.equal(word(context,68),16);
  const expectedRam=new Uint8Array(16*4096);
  pages.forEach(([address,page],index)=>{assert.equal(word(word(context,56)+(address>>>12)*4),index+1);expectedRam.set(page,index*4096);});
  assert.deepEqual(bytes(backing,expectedRam.length),expectedRam);report.before=raw();checkpoint();
  stage='active mixed execution';engine.ok('core_begin');active=true;
  const stop=cache.dispatch.run(context,6,first.slot)>>>0;
  const counters=['regions','coldRuns','coldWork','coldRetired'].map(name=>cache.dispatch[name]?.value??null);
  engine.ok('core_end');active=false;report.executed={stop,counters,...raw()};checkpoint();
  const expectedContext=initialContext.slice(),c=view(expectedContext);
  for(const [offset,value] of [[0,8],[32,0x1080],[36,2],[40,6],[44,0],[112,0]])c.setUint32(offset,value,true);
  c.setBigUint64(120,target.id,true);expectedRam.set(unhex('0000803f'),4096+8);
  assert.equal(stop,2);assert.deepEqual(bytes(context,128),expectedContext,'complete literal context and final numerical owner');
  assert.deepEqual(bytes(backing,expectedRam.length),expectedRam,'full canonical RAM includes the real ordinal0 FSTP');
  assert.deepEqual(bytes(engine.base,initialArena.length),initialArena,'the same private numerical bank stays unpublished');
  assert.deepEqual(counters,[2,1,2,2],'two compiled regions and one real interpreted two-op prefix');
  stage='explicit native publication';engine.ok('core_export',2,6);report.published=raw();checkpoint();
  const expectedArena=initialArena.slice(),finalState=state.slice();
  view(finalState).setUint32(16,8,true);view(finalState).setUint32(48,0x1080,true);expectedArena.set(finalState);
  const exit=record('R3EX',40);view(exit).setUint16(4,15,true);view(exit).setUint32(16,1,true);view(exit).setUint32(20,6,true);expectedArena.set(exit,56);
  const work=record('R3WK',24);view(work).setUint32(16,6,true);expectedArena.set(work,4588);
  const fp=initialArena.slice(4236,4364),f=view(fp);
  for(const [offset,value] of [[18,0],[20,0xffff],[22,0x15b],[32,0x1b],[34,0x23]])f.setUint16(offset,value,true);
  f.setUint32(24,0x1040,true);f.setUint32(28,0x3008,true);fp.set(unhex('0000000000000080ff3f'),110);expectedArena.set(fp,4236);
  assert.deepEqual(bytes(engine.base,initialArena.length),expectedArena,'literal complete arena, FP80 and unmodified SIMD/segment banks');
  assert.deepEqual(bytes(context,128),expectedContext);assert.deepEqual(bytes(backing,expectedRam.length),expectedRam);
  report.complete=true;checkpoint();console.log(JSON.stringify({complete:true,engineSha256:artifact.sha256,stop,work:6,retired:6,counters}));
} catch(error) {report.failure={stage,message:String(error),stack:error.stack,...raw()};checkpoint();throw error;}
finally {if(active)api.core_end();if(opened)engine.ok('close');}
