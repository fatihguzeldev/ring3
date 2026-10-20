import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync,writeFileSync,openSync,closeSync} from 'node:fs';
import {readEngine} from '../../../support/wasm/engine.mjs';

const [enginePath,reportPath] = process.argv.slice(2);
assert(enginePath && reportPath,'usage: clock.mjs engine.wasm fresh-report.json');
closeSync(openSync(reportPath,'wx'));
const artifact = readEngine(enginePath), hash = bytes => createHash('sha256').update(bytes).digest('hex');
const hex = bytes => Buffer.from(bytes).toString('hex');
const sources = ['./clock.mjs','../../../support/wasm/engine.mjs'];
const report = {schema:1,complete:false,engineSha256:artifact.sha256,
  sources:Object.fromEntries(sources.map(path => [path,hash(readFileSync(new URL(path,import.meta.url)))])),
  scope:'host-observed timestamp snapshots with original native bindings; no clock-policy, timing or game claim',
  codeHex:'0f3185c074fa0f0b',observations:[],runs:[],controls:[]};
const checkpoint = () => writeFileSync(reportPath,JSON.stringify(report,null,2));
let stage = 'instantiate', api, memory, arena = 0, context = 0, active = false, opened = false;
const bytes = (pointer,length) => new Uint8Array(memory.buffer).slice(pointer,pointer+length);
const view = () => new DataView(memory.buffer);
const word = (pointer,offset=0) => view().getUint32(pointer+offset,true);
const put = (pointer,offset,value) => view().setUint32(pointer+offset,value>>>0,true);
function status(operation,wanted,...args) {
  const actual = api[operation](...args)>>>0;
  report.controls.push({stage,operation,args,wanted,actual}); checkpoint();
  assert.equal(actual,wanted,`${stage}: ${operation}`);
}

try {
  checkpoint();
  const instance = new WebAssembly.Instance(artifact.module,{ring3_observability:{now_ns:()=>0n}});
  memory = instance.exports.memory;
  api = Object.fromEntries(Object.entries(instance.exports).filter(([name]) => name.startsWith('ring3_abi_v1_'))
    .map(([name,value]) => [name.slice(13),value]));
  for(const name of ['open','close','arena_ptr','arena_len','map','upload','configure_cpu_clock','observe_cpu_clock',
    'core_prepare','core_code_ptr','core_code_len','core_context_ptr','core_begin','core_end','core_read','core_write','core_numeric'])
    assert.equal(typeof api[name],'function',`native ${name}`);
  stage = 'open'; status('open',0,16,97,0); opened = true; arena = api.arena_ptr()>>>0;
  const code = new Uint8Array(4096); code.set(Buffer.from(report.codeHex,'hex'));
  status('map',0,0x1000,1,7);
  new Uint8Array(memory.buffer,arena+140,code.length).set(code); status('upload',0,0x1000,code.length);
  new Uint8Array(memory.buffer,arena,56).fill(0);
  new Uint8Array(memory.buffer,arena,4).set([82,51,83,84]);
  view().setUint16(arena+4,1,true); view().setUint16(arena+6,1,true); put(arena,8,56);
  const initial = [0x11223344,0x22334455,0x33445566,0x3000,0x4800,0x66778899,0x778899aa,0x8899aabb];
  initial.forEach((value,index) => put(arena,16+index*4,value)); put(arena,48,0x1000); put(arena,52,2);
  status('configure_cpu_clock',0,1_000_000_000,0,0,0);
  status('core_prepare',0,0x1000,32,4); context = api.core_context_ptr()>>>0;
  assert(context>0 && context%8===0);
  const encoded = bytes(api.core_code_ptr()>>>0,api.core_code_len()>>>0), module = new WebAssembly.Module(encoded);
  assert.deepEqual(WebAssembly.Module.imports(module),[
    {module:'ring3',name:'memory',kind:'memory'},{module:'ring3',name:'read',kind:'function'},
    {module:'ring3',name:'write',kind:'function'},{module:'ring3',name:'numeric',kind:'function'}]);
  const imports = {memory,read:api.core_read,write:api.core_write,numeric:api.core_numeric};
  const run = new WebAssembly.Instance(module,{ring3:imports}).exports.run;
  assert.equal(imports.numeric,instance.exports.ring3_abi_v1_core_numeric,'original numerical binding');
  report.module = {sha256:hash(encoded),wasmHex:hex(encoded)};
  status('core_begin',0); active = true;
  status('core_end',0); active = false;
  const backing = word(context,64), backingLength = word(context,68)*4096;
  assert.equal(backingLength,16*4096);
  const initialRam = bytes(backing,backingLength), initialArena = bytes(arena,api.arena_len());
  const observe = nanoseconds => {
    stage = `observe ${nanoseconds}ns`; status('observe_cpu_clock',0,nanoseconds,0);
    report.observations.push({nanoseconds,arenaSha256:hash(bytes(arena,api.arena_len())),backingSha256:hash(bytes(backing,backingLength))});
    checkpoint();
    assert.deepEqual(bytes(arena,api.arena_len()),initialArena,'observation leaves the arena unchanged');
    assert.deepEqual(bytes(backing,backingLength),initialRam,'observation leaves all RAM unchanged');
  };
  function execute(name,budget,{stop,pc,ticks,flags,retired,remaining}) {
    stage = name; status('core_begin',0); active = true;
    const expected = bytes(context,128), expectedView = new DataView(expected.buffer);
    for(const [offset,value] of [[0,ticks],[8,0],[32,pc],[36,flags],[40,retired],[44,remaining]]) expectedView.setUint32(offset,value,true);
    const actualStop = run(context,budget)>>>0;
    report.runs.push({name,budget,stop:actualStop,contextHex:hex(bytes(context,128)),
      arenaHex:hex(bytes(arena,api.arena_len())),backingHex:hex(bytes(backing,backingLength))}); checkpoint();
    assert.equal(actualStop,stop,`${name}: stop`);
    assert.deepEqual(bytes(context,128),expected,`${name}: literal complete context`);
    assert.deepEqual(bytes(arena,api.arena_len()),initialArena,`${name}: unpublished arena`);
    assert.deepEqual(bytes(backing,backingLength),initialRam,`${name}: whole canonical RAM`);
    status('core_end',0); active = false; assert.equal(word(context,112),0);
  }
  observe(0);
  execute('first frozen snapshot',6,{stop:2,pc:0x1000,ticks:0,flags:0x46,retired:6,remaining:0});
  execute('same snapshot without another observation',6,{stop:2,pc:0x1000,ticks:0,flags:0x46,retired:12,remaining:0});
  observe(1);
  execute('explicit observation leaves polling loop',3,{stop:2,pc:0x1006,ticks:1,flags:2,retired:15,remaining:0});
  execute('terminal is not retired',1,{stop:4,pc:0x1006,ticks:1,flags:2,retired:15,remaining:1});
  report.complete = true; checkpoint();
  console.log(JSON.stringify({complete:true,engineSha256:artifact.sha256,runs:report.runs.length,observations:report.observations.length}));
} catch(error) {
  report.failure = {stage,message:String(error),stack:error.stack}; checkpoint(); throw error;
} finally {
  if(active) api.core_end();
  if(opened) api.close();
}
