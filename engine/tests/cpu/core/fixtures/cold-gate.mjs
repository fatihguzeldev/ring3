import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync,writeFileSync,openSync,closeSync} from 'node:fs';
import {readEngine} from '../../../support/wasm/engine.mjs';
import {Engine,KEY,record,view} from '../../../../../sdk/runtime/engine.js';
import {CodeCache} from '../../../../../sdk/runtime/cache.js';

const [enginePath,expectedSha256,reportPath]=process.argv.slice(2);
assert(enginePath&&expectedSha256&&reportPath,'usage: cold-gate.mjs engine.wasm expected-sha256 fresh-report.json');
const artifact=readEngine(enginePath,expectedSha256),hash=bytes=>createHash('sha256').update(bytes).digest('hex');
const hex=bytes=>Buffer.from(bytes).toString('hex');
const peFixture='../../../process/images/startup/imports/fixtures/p2-c-imports/';
const image=readFileSync(new URL(`${peFixture}program.exe`,import.meta.url));
assert.equal(hash(image),'7c0451977a0818d5f4905a2b00f3a3bae63b9449ef22775ea003ebb0ca69c5d5');
const sources=['./cold-gate.mjs','../../../support/wasm/engine.mjs','../../../support/wasm/exports.mjs',
  '../../../../../sdk/runtime/engine.js','../../../../../sdk/runtime/engine/module.js','../../../../../sdk/runtime/cache.js',
  ...['entry.c','program.map','program.exe','oracle.json','receipt.json'].map(path=>peFixture+path)];
const report={schema:1,complete:false,engineSha256:artifact.sha256,
  imageSha256:hash(image),
  sources:Object.fromEntries(sources.map(path=>[path,hash(readFileSync(new URL(path,import.meta.url)))])),
  scope:'authoritative kernel gate priority over an indexed compiled entry, then execute-permission denial; no guest instruction or Windows provider executes',controls:[]};
closeSync(openSync(reportPath,'wx'));
const checkpoint=()=>writeFileSync(reportPath,JSON.stringify(report,null,2));
let engine,opened=false,context=0,backing=0,stage='initialization';checkpoint();
try {
  const instance=new WebAssembly.Instance(artifact.module,{ring3_observability:{now_ns:()=>0n}});
  const api=Object.freeze(Object.fromEntries(Object.entries(instance.exports).filter(([name])=>name.startsWith('ring3_abi_v1_'))
    .map(([name,value])=>[name.slice(13),value])));
  engine=new Engine(api,instance.exports.memory,artifact.sha256);
  const bytes=(pointer,length)=>new Uint8Array(engine.memory.buffer).slice(pointer,pointer+length);
  const word=(pointer,offset=0)=>new DataView(engine.memory.buffer).getUint32(pointer+offset,true);
  const raw=()=>({context:bytes(context,128),arena:bytes(engine.base,api.arena_len()),ram:bytes(backing,64*4096)});
  const compact=value=>({contextHex:hex(value.context),arenaSha256:hash(value.arena),ramSha256:hash(value.ram)});
  assert.equal(typeof api.core_cold_run,'function');
  engine.ok('open',64,...KEY);opened=true;engine.base=api.arena_ptr()>>>0;
  engine.ok('configure_module_limits',8,0x100000,16,65536,0x100000);
  engine.ok('register_kernel32_module',0x70000000);
  stage='load authored dependency graph';
  const name='cold-gate.exe';
  engine.ok('begin_module_input',image.length,engine.name(name));
  for(let offset=0;offset<image.length;offset+=4096) {
    const chunk=image.subarray(offset,offset+4096);engine.request(chunk);engine.ok('append_module_input',offset,chunk.length);
  }
  engine.ok('commit_module_input');
  engine.ok('start_module_process',engine.name(name),0,0x7e000000,4,0x7f000000);
  const gateId=0x10001;let gatePc;
  for(let start=0,total=1;start<total;) {
    engine.ok('module_gates_page',start,128);const page=engine.take(4096),d=view(page);
    assert.equal(Buffer.from(page.subarray(0,4)).toString(),'R3GP');total=d.getUint32(16,true);
    const count=d.getUint32(24,true);assert(count>0&&count<=128);
    for(let index=0;index<count;index++)if(d.getUint32(36+index*8,true)===gateId) {
      assert.equal(gatePc,undefined,'unique authoritative GetLastError gate');gatePc=d.getUint32(32+index*8,true);
    }
    start+=count;
  }
  assert(gatePc>=0x70000000);report.gate={id:gateId,pc:gatePc};
  const state=record('R3ST',56),s=view(state),registers=[5,6,7,0x3000,0x4800,8,9,10];
  registers.forEach((value,index)=>s.setUint32(16+index*4,value,true));s.setUint32(48,gatePc,true);s.setUint32(52,0x403,true);
  new Uint8Array(engine.memory.buffer,engine.base,56).set(state);
  stage='compile authoritative gate';
  const cache=new CodeCache(engine,{chain:false,budget:1,instructions:64,blocks:8}),unit=cache.entry(gatePc);
  const encoded=bytes(api.core_code_ptr()>>>0,api.core_code_len()>>>0);
  report.module={id:String(unit.id),pc:unit.pc,bytes:encoded.length,sha256:hash(encoded)};
  assert.equal(cache.native.core_cold_run,api.core_cold_run);
  engine.ok('core_begin');context=cache.context;backing=word(context,64);engine.ok('core_end');
  assert.equal(word(context,68),64);assert.equal(word(context,32),gatePc);assert.equal(word(context,40),0);
  const slot=word(word(context,56)+(gatePc>>>12)*4);assert(slot>0&&slot<=64);
  assert.deepEqual(bytes(backing+(slot-1)*4096+(gatePc&4095),2),Uint8Array.of(0x0f,0x0b));
  const initial=raw(),expectedContext=initial.context.slice(),c=view(expectedContext);
  report.initial={...compact(initial),arenaHex:hex(initial.arena),ramHex:hex(initial.ram)};checkpoint();
  assert.deepEqual(bytes(engine.base,56),state);
  assert.deepEqual(Array.from({length:8},(_,index)=>c.getUint32(index*4,true)),registers);
  assert.equal(c.getUint32(36,true),0x403);assert.equal(c.getUint32(44,true),0);assert.equal(c.getUint32(112,true),0);
  assert.equal(c.getBigUint64(120,true),unit.id);
  const cold=api.core_cold_run;
  function check(label,budget,wanted,fields) {
    stage=label;assert.equal(api.core_cold_run,cold);
    for(const [offset,value]of fields)c.setUint32(offset,value,true);c.setBigUint64(120,0n,true);
    const actual=cold(budget),after=raw();
    report.controls.push({label,budget,actual,wanted,...compact(after),codePointer:api.core_code_ptr(),codeLength:api.core_code_len()});checkpoint();
    assert.equal(actual,wanted,label);assert.deepEqual(after.context,expectedContext,`${label}: whole context`);
    assert.deepEqual(after.arena,initial.arena,`${label}: unpublished arena unchanged`);
    assert.deepEqual(after.ram,initial.ram,`${label}: whole canonical RAM unchanged`);
    assert.equal(api.core_code_ptr(),0);assert.equal(api.core_code_len(),0);
  }
  check('zero budget before indexed gate',0,2,[[44,0],[48,0],[52,0],[116,0]]);
  check('authoritative gate before compiled frontier',1,7,[[44,1],[116,gateId]]);
  stage='remove gate execute permission';engine.ok('protect',gatePc&~4095,1,1);
  const codeVersion=view(bytes(word(context,88),8)).getBigUint64(0,true);c.setBigUint64(96,codeVersion,true);
  check('zero budget before execute denial',0,2,[[44,0],[48,0],[52,0],[116,0]]);
  check('gate fetch denied before Host',1,3,[[44,1],[48,gatePc],[52,0x20402],[116,0x20e]]);
  assert.equal(cache.stats().admissions,1);assert.equal(cache.stats().runs,0);report.complete=true;
} catch(error) {
  report.failure={stage,message:String(error),stack:error.stack};
  if(engine&&context&&backing)report.failure.raw={contextHex:hex(new Uint8Array(engine.memory.buffer).slice(context,context+128)),
    arenaHex:hex(new Uint8Array(engine.memory.buffer).slice(engine.base,engine.base+engine.api.arena_len())),
    ramHex:hex(new Uint8Array(engine.memory.buffer).slice(backing,backing+64*4096))};
  throw error;
} finally {
  if(opened){report.closeStatus=engine.status('close');if(report.closeStatus!==0)report.complete=false;}checkpoint();
}
assert.equal(report.complete,true);assert.equal(report.closeStatus,0);
console.log(JSON.stringify({complete:true,engineSha256:artifact.sha256,gate:report.gate,controls:report.controls.length}));
