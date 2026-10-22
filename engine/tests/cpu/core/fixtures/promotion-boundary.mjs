import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync,writeFileSync,openSync,closeSync} from 'node:fs';
import {readEngine} from '../../../support/wasm/engine.mjs';
import {Engine,KEY,record,view} from '../../../../../sdk/runtime/engine.js';
import {CodeCache} from '../../../../../sdk/runtime/cache.js';

const [enginePath,reportPath] = process.argv.slice(2);
assert(enginePath && reportPath,'usage: promotion-boundary.mjs engine.wasm fresh-report.json');
const artifact = readEngine(enginePath), output = openSync(reportPath,'wx'); closeSync(output);
const hex = bytes => Buffer.from(bytes).toString('hex');
const unhex = text => Uint8Array.from(Buffer.from(text.replaceAll(' ',''),'hex'));
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const iterations = 8192, totalWork = 7*iterations;
const sources = ['./promotion-boundary.mjs','../../../support/wasm/engine.mjs',
  '../../../../../sdk/runtime/engine.js','../../../../../sdk/runtime/cache.js'];
const report = {schema:1,complete:false,engineSha256:artifact.sha256,iterations,totalWork,
  scope:'authored promotion across scheduler-only publish/import; full architectural closure, no timing or game claim',
  sources:Object.fromEntries(sources.map(path => [path,hash(readFileSync(new URL(path,import.meta.url)))])),arms:[]};
const checkpoint = () => writeFileSync(reportPath,JSON.stringify(report,null,2));
let stage = 'initialization'; checkpoint();

function runArm(boundary) {
  stage = boundary ? 'scheduler-boundary' : 'uninterrupted';
  const result = {name:stage,boundaries:0,runs:[],complete:false}; report.arms.push(result); checkpoint();
  const instance = new WebAssembly.Instance(artifact.module,{ring3_observability:{now_ns:()=>0n}});
  const api = Object.freeze(Object.fromEntries(Object.entries(instance.exports).filter(([name]) => name.startsWith('ring3_abi_v1_'))
    .map(([name,value]) => [name.slice(13),value])));
  const engine = new Engine(api,instance.exports.memory,artifact.sha256);
  const bytes = (pointer,length) => new Uint8Array(engine.memory.buffer).slice(pointer,pointer+length);
  const word = (pointer,offset=0) => new DataView(engine.memory.buffer).getUint32(pointer+offset,true);
  let cache,opened = false;
  try {
    engine.ok('open',16,...KEY); opened = true; engine.base = api.arena_ptr()>>>0;
    const code = new Uint8Array(4096), data = new Uint8Array(4096), stack = new Uint8Array(4096);
    code.set(unhex('d903 e839000000 49 75f6 0f0b'));
    code.set(unhex('d84b04 d95b08 c3'),0x40);
    view(data).setFloat32(0,3,true); view(data).setFloat32(4,2,true);
    const pages = [[0x1000,code,7],[0x3000,data,3],[0x4000,stack,3]];
    for (const [address,page,permissions] of pages) {
      engine.ok('map',address,1,permissions); engine.request(page); engine.ok('upload',address,page.length);
    }
    const initialRegisters = [0,iterations,0,0x3000,0x4800,0,0,0];
    const state = record('R3ST',56), s = view(state);
    initialRegisters.forEach((value,index) => s.setUint32(16+index*4,value,true));
    s.setUint32(48,0x1000,true); s.setUint32(52,2,true);
    new Uint8Array(engine.memory.buffer,engine.base,state.length).set(state);
    cache = new CodeCache(engine,{chain:true,budget:totalWork,instructions:64,blocks:8});
    const first = cache.entry(0x1000), next = cache.entry(0x1040); cache.entry(0x1000);
    engine.ok('core_begin'); engine.ok('core_end');
    const initialContext = bytes(cache.context,128), initialArena = bytes(engine.base,api.arena_len());
    const backing = word(cache.context,64), residentPages = word(cache.context,68);
    assert.equal(residentPages,16);
    assert.deepEqual([1,3,4].map(page => word(word(cache.context,56)+page*4)),[1,2,3]);
    result.initialOwners = [String(first.id),String(next.id)];
    let selected = first, retired = 0, work = 0, publishedRetired = 0, publishedWork = 0, last;
    while (work < totalWork) {
      assert(result.runs.length<12,'bounded promotion continuations');
      cache.budget = totalWork-work;
      last = cache.execute(selected); retired += last.retired; work += last.work;
      result.runs.push({...last,id:last.id===null?null:String(last.id)}); checkpoint();
      if (last.coreReason === 2) break;
      assert.equal(last.coreReason,1,'only promotion yields before the terminal budget');
      if (last.kind === 'cold') {
        assert.equal(last.work,0,'retained frontier cold handoff does not execute guest work');
        assert.equal(last.retired,0);
      } else {
        assert.notEqual(last.id,null,'all authored successors are already linked');
        assert.equal(last.chainedColdRuns,0,'this handoff is not a cold compilation request');
        assert(last.work>0,'promotion follows actual guest progress');
      }
      if (boundary && last.kind === 'compiled') {
        const before = bytes(cache.context,48);
        cache.publish(); publishedRetired = retired; publishedWork = work;
        cache.import(); result.boundaries++;
        assert.deepEqual(bytes(cache.context,48),before,'scheduler publication/import preserves all guest CPU words');
      }
      selected = cache.select();
    }
    assert.equal(last.coreReason,2); assert.equal(retired,totalWork); assert.equal(work,totalWork);
    assert.equal(cache.stats().retired,totalWork); assert.equal(cache.stats().work,totalWork);
    const expectedRegisters = [0,0,0,0x3000,0x4800,0,0,0];
    const expectedContext = initialContext.slice(), c = view(expectedContext);
    expectedRegisters.forEach((value,index) => c.setUint32(index*4,value,true));
    for (const [offset,value] of [[32,0x100a],[36,0x46],[40,totalWork],[44,0],[48,0],[52,0],[112,0],[116,0]])
      c.setUint32(offset,value,true);
    c.setBigUint64(120,last.id,true);
    assert.deepEqual(bytes(cache.context,128),expectedContext,'full Context32 with unchanged memory/bank/source witnesses');
    const expectedRam = new Uint8Array(16*4096);
    pages.forEach(([,page],index) => expectedRam.set(page,index*4096));
    expectedRam.set(unhex('0000c040'),4096+8);
    expectedRam.set(unhex('07100000'),8192+0x7fc);
    assert.deepEqual(bytes(backing,expectedRam.length),expectedRam,'entire canonical code/data/stack allocation');
    const expectedArena = initialArena.slice(), finalState = record('R3ST',56), fs = view(finalState);
    expectedRegisters.forEach((value,index) => fs.setUint32(16+index*4,value,true));
    fs.setUint32(48,0x100a,true); fs.setUint32(52,0x46,true); expectedArena.set(finalState);
    const exit = record('R3EX',40), ex = view(exit); ex.setUint16(4,15,true);
    ex.setUint32(16,1,true); ex.setUint32(20,retired-publishedRetired,true); expectedArena.set(exit,56);
    const completedWork = record('R3WK',24); view(completedWork).setUint32(16,work-publishedWork,true);
    expectedArena.set(completedWork,4588);
    const fp = expectedArena.subarray(4236,4364), f = view(fp);
    for (const [offset,value] of [[18,0],[20,0xffff],[22,0x15b],[32,0x1b],[34,0x23]]) f.setUint16(offset,value,true);
    f.setUint32(24,0x1043,true); f.setUint32(28,0x3008,true); fp.set(unhex('00000000000000c00140'),110);
    assert.deepEqual(bytes(engine.base,expectedArena.length),expectedArena,
      'entire canonical arena including literal FP128, untouched SIMD/segments/YMM and exact publication envelope');
    result.final = {contextHex:hex(bytes(cache.context,128)),arenaHex:hex(bytes(engine.base,expectedArena.length)),
      ramHex:hex(bytes(backing,expectedRam.length)),fpHex:hex(fp),stateHex:hex(finalState),
      lastPublishedRetired:retired-publishedRetired,lastPublishedWork:work-publishedWork,
      owner:String(last.id),fused:!result.initialOwners.includes(String(last.id)),admissions:cache.stats().admissions};
    result.complete = true; checkpoint(); return result;
  } catch (error) {
    result.failure = {stage,message:String(error),stack:error.stack}; checkpoint(); throw error;
  } finally {if (cache?.open) api.core_end(); if (opened) engine.ok('close');}
}

try {
  const uninterrupted = runArm(false), boundary = runArm(true);
  assert.deepEqual(boundary.final.stateHex,uninterrupted.final.stateHex,'complete architectural integer state equals uninterrupted control');
  assert.deepEqual(boundary.final.fpHex,uninterrupted.final.fpHex,'complete FP128 equals uninterrupted control');
  assert.deepEqual(boundary.final.ramHex,uninterrupted.final.ramHex,'entire canonical RAM equals uninterrupted control');
  assert(uninterrupted.final.fused,'uninterrupted control must form a new combined owner');
  assert(boundary.boundaries>0,'the experiment crossed a real promotion NeedCode boundary');
  assert(boundary.final.fused,'scheduler-only publish/import must preserve pending promotion and form a new combined owner');
  assert.deepEqual(boundary.final.contextHex,uninterrupted.final.contextHex,'full private context equals uninterrupted control after preserved promotion');
  report.complete = true; checkpoint(); console.log(JSON.stringify({complete:true,engineSha256:artifact.sha256,
    arms:report.arms.map(({name,boundaries,final})=>({name,boundaries,owner:final.owner,admissions:final.admissions}))}));
} catch (error) {report.failure = {stage,message:String(error),stack:error.stack}; checkpoint(); throw error;}
