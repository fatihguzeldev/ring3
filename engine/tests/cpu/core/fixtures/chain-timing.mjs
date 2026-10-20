import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync,writeFileSync,openSync,closeSync} from 'node:fs';
import {performance} from 'node:perf_hooks';
import {readEngine} from '../../../support/wasm/engine.mjs';
import {Engine,KEY,record,view} from '../../../../../sdk/runtime/engine.js';
import {CodeCache} from '../../../../../sdk/runtime/cache.js';
import {makeKernel} from './kernels.mjs';

const [enginePath,reportPath,count='131072'] = process.argv.slice(2), iterations = Number(count);
assert(enginePath && reportPath,'usage: chain-timing.mjs engine.wasm fresh-report.json [iterations]');
const kernel = makeKernel('call',iterations), artifact = readEngine(enginePath);
const output = openSync(reportPath,'wx'); closeSync(output);
const hash = bytes => createHash('sha256').update(bytes).digest('hex'), hex = bytes => Buffer.from(bytes).toString('hex');
const sources = ['./chain-timing.mjs','./kernels.mjs','../../../support/wasm/engine.mjs',
  '../../../../../sdk/runtime/engine.js','../../../../../sdk/runtime/cache.js'];
const report = {schema:1,complete:false,node:process.version,engineSha256:artifact.sha256,iterations,warmupsPerArm:2,rounds:8,
  clock:'node performance.now elapsed milliseconds',
  scope:'normal SDK dispatch work across identical authored regions; no per-component CPU or game attribution',
  sources:Object.fromEntries(sources.map(path => [path,hash(readFileSync(new URL(path,import.meta.url)))])),arms:[],runs:[]};
const checkpoint = () => writeFileSync(reportPath,JSON.stringify(report,null,2));
const banks = [[4236,352],[4612,184]], contexts = [];
let stage = 'initialization', referenceArena;
checkpoint();
function fresh(chain) {
  const result = {chain,modules:[]}; report.arms.push(result);
  const instance = new WebAssembly.Instance(artifact.module,{ring3_observability:{now_ns:()=>0n}});
  const api = Object.freeze(Object.fromEntries(Object.entries(instance.exports).filter(([name]) => name.startsWith('ring3_abi_v1_'))
    .map(([name,value]) => [name.slice(13),value])));
  const engine = new Engine(api,instance.exports.memory,artifact.sha256);
  let opened = false;
  try {
    engine.ok('open',16,...KEY); opened = true; engine.base = api.arena_ptr()>>>0;
    for (const [address,page] of [[0x1000,kernel.code],[0x3000,kernel.initialData],[0x4000,kernel.initialStack]]) {
      engine.ok('map',address,1,address === 0x1000 ? 7 : 3); engine.request(page); engine.ok('upload',address,page.length);
    }
    const state = record('R3ST',56), stateView = view(state);
    kernel.initialRegs.forEach((value,index) => stateView.setUint32(16+index*4,value,true));
    stateView.setUint32(48,0x1000,true); stateView.setUint32(52,2,true);
    new Uint8Array(engine.memory.buffer,engine.base,56).set(state);
    const bytes = (pointer,length) => new Uint8Array(engine.memory.buffer).slice(pointer,pointer+length);
    const initialBanks = banks.map(([offset,length]) => bytes(engine.base+offset,length));
    const cache = new CodeCache(engine,{chain,budget:kernel.retired,instructions:64,blocks:8});
    for (const pc of [0x1000,0x1040,0x1000]) {
      const unit = cache.entry(pc);
      if (!result.modules.some(item => item.id === String(unit.id))) {
        const encoded = bytes(api.core_code_ptr()>>>0,api.core_code_len()>>>0);
        result.modules.push({pc,id:String(unit.id),sha256:hash(encoded),wasmHex:hex(encoded)});
      }
    }
    if (chain) {
      const encoded = bytes(api.core_dispatch_code_ptr()>>>0,api.core_dispatch_code_len()>>>0);
      result.driver = {sha256:hash(encoded),wasmHex:hex(encoded)};
    }
    return {chain,engine,api,cache,bytes,initialBanks,initialState:state,result};
  } catch (error) {if (opened) engine.ok('close'); throw error;}
}
function run(context,phase,round,position) {
  const {chain,engine,api,cache,bytes} = context;
  stage = `${phase}/${round}/${position}/${chain}`;
  for (const [address,page] of [[0x3000,kernel.initialData],[0x4000,kernel.initialStack]]) {
    engine.request(page); engine.ok('upload',address,page.length);
  }
  new Uint8Array(engine.memory.buffer,engine.base,56).set(context.initialState);
  cache.import(); let selected = cache.entry(0x1000);
  const before = cache.stats(), priorRetired = cache.word(40);
  let retired = 0, work = 0, outer = 0, stop;
  const started = performance.now();
  while (work < kernel.retired) {
    if (outer >= 2*iterations+2) throw new Error('authored dispatch bound exceeded');
    cache.budget = kernel.retired-work;
    const result = cache.execute(selected); outer++; retired += result.retired; work += result.work; stop = result.coreReason;
    if (stop === 2) break;
    if (stop !== 1) throw new Error(`unexpected authored stop ${stop}`);
    selected = cache.entry();
  }
  const elapsedMs = performance.now()-started;
  const rawContext = bytes(cache.context,128), d = view(rawContext), backing = d.getUint32(64,true), pages = d.getUint32(68,true);
  const ram = bytes(backing,pages*4096), arena = bytes(engine.base,api.arena_len()), after = cache.stats();
  const sample = {phase,round,position,chain,elapsedMs,outer,stop,retired,work,
    regions:after.regions-before.regions,dispatches:after.runs-before.runs,
    contextHex:hex(rawContext),arenaHex:hex(arena),ramSha256:hash(ram)};
  report.runs.push(sample); report.lastRawRamHex = hex(ram); checkpoint();
  assert.equal(stop,2); assert.equal(retired,kernel.retired); assert.equal(work,kernel.retired);
  assert.equal((d.getUint32(40,true)-priorRetired)>>>0,kernel.retired);
  assert.equal(d.getUint32(44,true),0); assert.equal(d.getUint32(112,true),0);
  assert.equal(outer,chain ? 1 : 2*iterations+1); assert.equal(sample.dispatches,outer);
  assert.equal(sample.regions,2*iterations+1);
  const expectedState = record('R3ST',56), state = view(expectedState);
  kernel.regs.forEach((value,index) => state.setUint32(16+index*4,value,true));
  state.setUint32(48,kernel.pc,true); state.setUint32(52,kernel.flags,true);
  assert.deepEqual(arena.slice(0,56),expectedState);
  const expectedExit = record('R3EX',40), exit = view(expectedExit); exit.setUint16(4,15,true);
  exit.setUint32(16,1,true); exit.setUint32(20,kernel.retired,true);
  assert.deepEqual(arena.slice(56,96),expectedExit);
  const expectedWork = record('R3WK',24); view(expectedWork).setUint32(16,kernel.retired,true);
  assert.deepEqual(arena.slice(4588,4612),expectedWork);
  assert.deepEqual(banks.map(([offset,length]) => arena.slice(offset,offset+length)),context.initialBanks);
  assert.equal(pages,16);
  const mappings = new DataView(engine.memory.buffer);
  assert.deepEqual([1,3,4].map(page => mappings.getUint32(d.getUint32(56,true)+page*4,true)),[1,2,3]);
  const expectedRam = new Uint8Array(16*4096);
  expectedRam.set(kernel.code,0); expectedRam.set(kernel.expectedData,4096); expectedRam.set(kernel.expectedStack,8192);
  assert.deepEqual(ram,expectedRam,'whole canonical allocation');
  referenceArena ??= arena;
  assert.deepEqual(arena,referenceArena,'whole canonical arena across both modes and all runs');
  sample.complete = true; delete report.lastRawRamHex; checkpoint();
}
const median = values => {const a = values.toSorted((x,y) => x-y), n = a.length; return n%2 ? a[n>>1] : (a[n/2-1]+a[n/2])/2;};
try {
  contexts.push(fresh(false)); contexts.push(fresh(true));
  assert.deepEqual(report.arms[1].modules,report.arms[0].modules,'unchanged compiled region bytes and identities');
  for (let warm = 0; warm < 2; warm++) for (let index = 0; index < 2; index++) run(contexts[index],'warmup',warm,index);
  for (let round = 0; round < 8; round++) {
    const order = round%2 ? [1,0,0,1] : [0,1,1,0];
    order.forEach((index,position) => run(contexts[index],'timed',round,position));
  }
  const direct = report.runs.filter(item => item.phase === 'timed' && !item.chain).map(item => item.elapsedMs);
  const chained = report.runs.filter(item => item.phase === 'timed' && item.chain).map(item => item.elapsedMs);
  report.summary = {directMedianMs:median(direct),chainedMedianMs:median(chained),ratio:median(direct)/median(chained)};
  report.complete = true; checkpoint(); console.log(JSON.stringify({complete:true,...report.summary}));
} catch (error) {report.failure = {stage,message:String(error),stack:error.stack}; checkpoint(); throw error;}
finally {for (const {cache,api,engine} of contexts) {if (cache.open) api.core_end(); engine.ok('close');}}
