import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync,writeFileSync,openSync,closeSync} from 'node:fs';
import {readEngine} from '../../../support/wasm/engine.mjs';
import {Engine,KEY,record,view} from '../../../../../sdk/runtime/engine.js';
import {CodeCache} from '../../../../../sdk/runtime/cache.js';
import {makeKernel} from './kernels.mjs';

const [enginePath,reportPath,count='3'] = process.argv.slice(2), iterations = Number(count);
assert(enginePath && reportPath,'usage: chain.mjs engine.wasm fresh-report.json [iterations]');
const kernel = makeKernel('call',iterations), artifact = readEngine(enginePath);
const output = openSync(reportPath,'wx'); closeSync(output);
const hash = bytes => createHash('sha256').update(bytes).digest('hex'), hex = bytes => Buffer.from(bytes).toString('hex');
const sources = ['./chain.mjs','./kernels.mjs','../../../support/wasm/engine.mjs',
  '../../../../../sdk/runtime/engine.js','../../../../../sdk/runtime/cache.js'];
const report = {schema:1,complete:false,engineSha256:artifact.sha256,iterations,
  sources:Object.fromEntries(sources.map(path => [path,hash(readFileSync(new URL(path,import.meta.url)))])),
  scope:'same-artifact authored CALL/RET correctness and dispatch counts; no timing or game claim',arms:[]};
const checkpoint = () => writeFileSync(reportPath,JSON.stringify(report,null,2));
let stage = 'initialization';
checkpoint();

function arm(chain) {
  const result = {chain,modules:[],firstRuns:[],lastRuns:[],outerCalls:{begin:0,end:0},complete:false};
  report.arms.push(result); checkpoint();
  const instance = new WebAssembly.Instance(artifact.module,{ring3_observability:{now_ns:()=>0n}});
  const native = Object.fromEntries(Object.entries(instance.exports).filter(([name]) => name.startsWith('ring3_abi_v1_'))
    .map(([name,value]) => [name.slice(13),value]));
  const api = {...native,
    core_begin(...args) {result.outerCalls.begin++; return native.core_begin(...args);},
    core_end(...args) {result.outerCalls.end++; return native.core_end(...args);},
  };
  const engine = new Engine(Object.freeze(api),instance.exports.memory,artifact.sha256);
  const bytes = (pointer,length) => new Uint8Array(engine.memory.buffer).slice(pointer,pointer+length);
  const word = (pointer,offset=0) => new DataView(engine.memory.buffer).getUint32(pointer+offset,true);
  let cache,opened = false;
  const snapshot = () => {
    const context = cache?.context ?? 0, backing = context ? word(context,64) : 0, pages = context ? word(context,68) : 0;
    return {arenaHex:hex(bytes(engine.base,api.arena_len())),contextHex:context ? hex(bytes(context,128)) : null,
      backingHex:backing ? hex(bytes(backing,pages*4096)) : null,stats:cache?.stats()};
  };
  try {
    stage = `${chain}: open`;
    engine.ok('open',16,...KEY); opened = true; engine.base = api.arena_ptr()>>>0;
    for (const [address,page] of [[0x1000,kernel.code],[0x3000,kernel.initialData],[0x4000,kernel.initialStack]]) {
      engine.ok('map',address,1,address === 0x1000 ? 7 : 3);
      engine.request(page); engine.ok('upload',address,page.length);
    }
    const state = record('R3ST',56), d = view(state);
    kernel.initialRegs.forEach((value,index) => d.setUint32(16+index*4,value,true));
    d.setUint32(48,0x1000,true); d.setUint32(52,2,true); new Uint8Array(engine.memory.buffer,engine.base,56).set(state);
    const bankRanges = [[4236,352],[4612,184]], initialBanks = bankRanges.map(([offset,length]) => bytes(engine.base+offset,length));
    cache = new CodeCache(engine,{chain,budget:kernel.retired,instructions:64,blocks:8});
    for (const name of ['core_read','core_write','core_numeric','core_chain_next']) assert.equal(api[name],native[name],`direct native ${name}`);
    function select(pc) {
      const unit = cache.entry(pc);
      if (!result.modules.some(item => item.id === String(unit.id))) {
        const encoded = bytes(native.core_code_ptr()>>>0,native.core_code_len()>>>0);
        result.modules.push({pc,id:String(unit.id),slot:unit.slot ?? null,sha256:hash(encoded),wasmHex:hex(encoded)});
      }
      return unit;
    }
    stage = `${chain}: prepare`;
    select(0x1000); select(0x1040); let selected = select(0x1000);
    if (chain) {
      const encoded = bytes(native.core_dispatch_code_ptr()>>>0,native.core_dispatch_code_len()>>>0);
      result.driver = {sha256:hash(encoded),wasmHex:hex(encoded),tableLength:cache.table.length};
      assert.equal(cache.table.length,4097); assert.equal(cache.table.get(0),null);
    }
    stage = `${chain}: execute`;
    let retired = 0, work = 0, outer = 0, stop;
    while (work < kernel.retired) {
      assert(outer < 2*iterations+2,'bounded authored dispatch count');
      cache.budget = kernel.retired-work;
      const run = cache.execute(selected); outer++; retired += run.retired; work += run.work; stop = run.coreReason;
      const raw = {...run,id:String(run.id),contextHex:hex(bytes(cache.context,128))};
      if (result.firstRuns.length < 12) result.firstRuns.push(raw);
      result.lastRuns.push(raw); if (result.lastRuns.length > 12) result.lastRuns.shift();
      if (stop === 2) break;
      assert.equal(stop,1,'only genuine NeedCode selects another authored region');
      selected = select(cache.pc);
    }
    result.final = snapshot(); result.retired = retired; result.work = work; result.outer = outer; result.stop = stop; checkpoint();
    assert.equal(stop,2); assert.equal(retired,kernel.retired); assert.equal(work,kernel.retired);
    assert.equal(cache.stats().runs,chain ? 1 : 2*iterations+1);
    assert.equal(cache.stats().regions,2*iterations+1);
    assert.deepEqual(result.outerCalls,{begin:outer,end:outer});
    assert.equal(outer,chain ? 1 : 2*iterations+1);
    const expectedState = record('R3ST',56), expectedView = view(expectedState);
    kernel.regs.forEach((value,index) => expectedView.setUint32(16+index*4,value,true));
    expectedView.setUint32(48,kernel.pc,true); expectedView.setUint32(52,kernel.flags,true);
    assert.deepEqual(bytes(engine.base,56),expectedState,'literal complete canonical State32');
    assert.deepEqual(bankRanges.map(([offset,length]) => bytes(engine.base+offset,length)),initialBanks,'complete numerical and segment banks');
    const expectedExit = record('R3EX',40), exit = view(expectedExit); exit.setUint16(4,15,true);
    exit.setUint32(16,1,true); exit.setUint32(20,kernel.retired,true);
    assert.deepEqual(bytes(engine.base+56,40),expectedExit,'literal canonical Budget Exit');
    const expectedWork = record('R3WK',24); view(expectedWork).setUint32(16,kernel.retired,true);
    assert.deepEqual(bytes(engine.base+4588,24),expectedWork,'literal completed work transport');
    assert.equal(word(cache.context,44),0); assert.equal(word(cache.context,112),0);
    const mappings = word(cache.context,56), backing = word(cache.context,64), pages = word(cache.context,68);
    assert.equal(pages,16); assert.deepEqual([1,3,4].map(page => word(mappings+page*4)),[1,2,3]);
    const expectedRam = new Uint8Array(16*4096);
    expectedRam.set(kernel.code,0); expectedRam.set(kernel.expectedData,4096); expectedRam.set(kernel.expectedStack,8192);
    assert.deepEqual(bytes(backing,expectedRam.length),expectedRam,'whole canonical allocation, code, data and stack');
    result.complete = true; checkpoint(); return result;
  } catch (error) {
    result.failure = {stage,message:String(error),snapshot:cache?.context ? snapshot() : null}; checkpoint(); throw error;
  } finally {
    if (cache?.open) native.core_end();
    if (opened) engine.ok('close');
  }
}
try {
  const direct = arm(false), chained = arm(true);
  assert.deepEqual(chained.modules.map(({pc,id,sha256}) => ({pc,id,sha256})),direct.modules.map(({pc,id,sha256}) => ({pc,id,sha256})),
    'same compiled region bytes and identities');
  assert.equal(chained.final.arenaHex,direct.final.arenaHex,'complete canonical arena equality');
  assert.equal(chained.final.backingHex,direct.final.backingHex,'complete canonical RAM equality');
  report.complete = true; checkpoint();
  console.log(JSON.stringify({complete:true,engineSha256:artifact.sha256,iterations,
    arms:report.arms.map(({chain,outer,retired,work,final}) => ({chain,outer,retired,work,regions:final.stats.regions}))}));
} catch (error) {report.failure = {stage,message:String(error),stack:error.stack}; checkpoint(); throw error;}
