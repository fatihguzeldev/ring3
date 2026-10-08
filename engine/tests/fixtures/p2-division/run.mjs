import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';

const [enginePath, output, root, arenaSize] = process.argv.slice(2);
assert.ok(root);
const SIZE = Number(arenaSize), TRANSFER = 140, PC = 0x1000, COLD = 0x1800;
const initial = readFileSync(join(output, 'initial-arena.bin'));
assert.equal(initial.length, SIZE);
const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');
const engineBytes = readFileSync(enginePath), expectedEngineSha = process.env.RING3_ENGINE_SHA256;
assert.match(expectedEngineSha ?? '', /^[a-f0-9]{64}$/, 'expected a lowercase engine SHA-256');
assert.equal(sha256(engineBytes), expectedEngineSha, 'engine binary differs from the requested SHA-256');
const engineModule = new WebAssembly.Module(engineBytes);
assert.deepEqual(WebAssembly.Module.imports(engineModule), []);
const rows = [[17,0,5], [0,0,1], [0xffffffff,0,1], [0,1,1], [0xffffffff,0xfffffffe,0xffffffff],
  [0xfffffff9,0xffffffff,3], [7,0,0xfffffffd], [0xfffffff9,0xffffffff,0xfffffffd],
  [0x80000000,0xffffffff,1], [0x80000000,0xffffffff,0xffffffff], [0x7fffffff,0,1],
  [0x80000000,0,1], [0,0x80000000,0xffffffff], [0x12345678,0,0]];
const registers = [0,0,0,0x456789ab,0x9000,0x6789abcd,0x789abcde,0x89abcdef];
const flags = [2,0xcd7];
const stats = {contexts: 0, numeric_cases: 0, successful: 0, faults: 0, generated_runs: 0, prefix_faults: 0, dispatcher_runs: 0};
const modules = [], observations = [];
function record(magic, size, fields, version = 1) {
  const b = Buffer.alloc(size); b.write(magic); b.writeUInt16LE(version,4); b.writeUInt16LE(1,6); b.writeUInt32LE(size,8);
  fields.forEach((value,index) => b.writeUInt32LE(value >>> 0,16+index*4)); return b;
}
const state = (r,pc,f) => record('R3ST',56,[...r,pc,f]);
const exit = (reason,retired,detail = 0,version = 5) => record('R3EX',40,[reason,retired,detail,0,0,0],version);
const words = fields => {const b=Buffer.alloc(fields.length*4); fields.forEach((v,i)=>b.writeUInt32LE(v>>>0,i*4));return b;};
function refresh(c) {c.bytes=new Uint8Array(c.memory.buffer);c.view=new DataView(c.memory.buffer);return c;}
function arena(c) {return Buffer.from(refresh(c).bytes.subarray(c.base,c.base+SIZE));}
function seed(c,r,pc,f) {refresh(c).bytes.set(state(r,pc,f),c.base);c.bytes.set(exit(3,0),c.base+56);c.view.setUint32(c.base+96,0,true);}
function request(c,b) {refresh(c).bytes.set(b,c.base+TRANSFER);}
function fresh(owner) {
  stats.contexts++;
  if (owner==='standalone') {
    const c={owner,memory:new WebAssembly.Memory({initial:1}),base:128};refresh(c).bytes.set(initial,c.base);return c;
  }
  const instance=new WebAssembly.Instance(engineModule,{});
  const names=['open','close','arena_ptr','map','upload','compile_with_gates','compile_resident_with_gates','generation',
    'module_ptr','module_len','guard','guard_resident','acknowledge_resident_installation','dispatcher_module',
    'guard_dispatch_entry','find_installed_resident','capture_resident_call','complete_resident_call'];
  const api=Object.fromEntries(names.map(name=>[name,instance.exports[`ring3_abi_v1_${name}`]]));
  for (const [name,fn] of Object.entries(api)) assert.equal(typeof fn,'function',name);
  const c={owner,api,memory:instance.exports.memory,low:stats.contexts,high:0xc9356000};
  assert.equal(api.open(3,c.low,c.high),0);c.base=api.arena_ptr()>>>0;
  assert.deepEqual(arena(c),initial);
  assert.equal(api.map(PC,1,7),0);return c;
}
function compile(c,name,bytes,specs,gates=[]) {
  let generated,binding={};
  if (c.owner==='standalone') generated=readFileSync(join(output,`standalone-${name}.wasm`));
  else {
    request(c,bytes);assert.equal(c.api.upload(name==='chain'?0x1400:PC,bytes.length),0);
    request(c,words([...specs,...gates].flat()));
    if (c.owner==='replacement') {
      assert.equal(c.api.compile_with_gates(specs.length,gates.length),0);
      binding={generation:c.api.generation(),pointer:c.api.module_ptr()>>>0,length:c.api.module_len()>>>0};
    } else {
      assert.equal(c.api.compile_resident_with_gates(specs.length,gates.length),0);refresh(c);
      binding=Object.fromEntries(['low','high','pointer','length'].map((key,i)=>[key,c.view.getUint32(c.base+TRANSFER+8+i*4,true)]));
    }
    generated=Buffer.from(refresh(c).bytes.subarray(binding.pointer,binding.pointer+binding.length));
    writeFileSync(join(output,`${c.owner}-${name}.wasm`),generated,{flag:'wx'});
  }
  const module=new WebAssembly.Module(generated),imports=WebAssembly.Module.imports(module);
  assert.deepEqual(imports,[{module:'env',name:'memory',kind:'memory'},
    ...(c.owner==='standalone'?[]:[{module:'ring3',name:c.owner==='replacement'?'guard':'guard_resident',kind:'function'}])]);
  const child=new WebAssembly.Instance(module,{env:{memory:c.memory},...(c.api?{ring3:c.api}:{})});
  modules.push({owner:c.owner,name,sha256:sha256(generated),bytes:generated.length});
  return {...binding,run:child.exports.run};
}
function run(c,u,budget,r,pc,f,reason=1,retired=1,detail=0,extra) {
  const wanted=arena(c);wanted.set(state(r,pc,f));wanted.set(exit(reason,retired,detail),56);
  if (extra) wanted.set(extra,TRANSFER);
  assert.equal(u.run(c.base,c.base+56,budget,c.base+96),0);
  assert.deepEqual(arena(c),wanted,'full arena including opaque tail and helper remains exact');stats.generated_runs++;
}
function divide(kind,r,source) {
  const bits=BigInt(r[2])*4294967296n+BigInt(r[0]);
  const dividend=kind==='signed'?BigInt.asIntN(64,bits):bits;
  const divisor=kind==='signed'?BigInt.asIntN(32,BigInt(r[source])):BigInt(r[source]);
  if (divisor===0n) return null;
  const quotient=dividend/divisor;
  if (kind==='signed'?(quotient < -2147483648n || quotient > 2147483647n):quotient > 4294967295n) return null;
  return [Number(BigInt.asUintN(32,quotient)),Number(BigInt.asUintN(32,dividend%divisor))];
}
for (const [kind,r,wanted] of [
  ['unsigned',[17,5,0],[3,2]],['unsigned',[0xffffffff,1,0],[0xffffffff,0]],
  ['unsigned',[0,1,1],null],['signed',[0xfffffff9,3,0xffffffff],[0xfffffffe,0xffffffff]],
  ['signed',[7,0xfffffffd,0],[0xfffffffe,1]],['signed',[0x80000000,1,0xffffffff],[0x80000000,0]],
  ['signed',[0x80000000,0xffffffff,0xffffffff],null],['signed',[0,0xffffffff,0x80000000],null],
]) assert.deepEqual(divide(kind,r,1),wanted,'literal quotient/remainder/fault anchor');
for (const owner of ['standalone','replacement','resident']) {
  for (const kind of ['unsigned','signed']) {
    const c=fresh(owner),bytes=readFileSync(join(output,`${kind}.x86`));
    const u=compile(c,kind,bytes,Array.from({length:8},(_,source)=>[PC+source*16,7]));
    seed(c,registers,PC,2);run(c,u,0,registers,PC,2,1,0);
    refresh(c).view.setUint32(c.base+96,1,true);run(c,u,1,registers,PC,2,2,0);refresh(c).view.setUint32(c.base+96,0,true);
    for (let source=0;source<8;source++) for (const [index,[eax,edx,divisor]] of rows.entries()) for (const f of flags) {
      const r=[...registers];r[0]=eax;r[2]=edx;if (source!==0 && source!==2) r[source]=divisor;
      const entry=PC+source*16,result=divide(kind,r,source);seed(c,r,entry,f);stats.numeric_cases++;
      if (result) {
        const wanted=[...r];wanted[0]=result[0];wanted[2]=result[1];
        run(c,u,1,wanted,entry+2,f);run(c,u,1,wanted,COLD,f);run(c,u,1,wanted,COLD,f,3,0);stats.successful++;
      } else {run(c,u,1,r,entry,f,10,0);run(c,u,1,r,entry,f,10,0);stats.faults++;}
      observations.push({owner,kind,source,row:index,flags:f,state_hex:arena(c).subarray(0,56).toString('hex'),exit_hex:arena(c).subarray(56,96).toString('hex')});
    }
    if (c.api) assert.equal(c.api.close(),0);
  }
  const c=fresh(owner),chain=readFileSync(join(output,'chain.x86'));
  assert.equal(chain.toString('hex'),'b905000000ba00000000f7f1f7f8eb00');
  const u=compile(c,'chain',chain,[[0x1400,16]]),r=[...registers];r[0]=17;r[2]=0xabcdef01;
  seed(c,r,0x1400,0xcd7);const wanted=[...r];wanted[1]=5;wanted[0]=3;wanted[2]=2;
  run(c,u,0xffffffff,wanted,0x140c,0xcd7,10,3);run(c,u,1,wanted,0x140c,0xcd7,10,0);stats.prefix_faults++;
  if (c.api) assert.equal(c.api.close(),0);
}

const c=fresh('resident'),gate=0x1500,gateId=17;
request(c,Buffer.from('0f0b','hex'));assert.equal(c.api.upload(gate,2),0);
const gateBytes=Buffer.from('f7f1e9f9040000','hex');
const u=compile(c,'gate',gateBytes,[[PC,7],[gate,2]],[[gate,gateId]]);
assert.equal(c.api.map(0x9000,1,3),0);request(c,words([COLD]));assert.equal(c.api.upload(0x9000,4),0);
const table=new WebAssembly.Table({element:'anyfunc',initial:8,maximum:8});table.set(0,u.run);
assert.equal(c.api.acknowledge_resident_installation(c.low,c.high,u.low,u.high,0),0);
const installed=record('R3IN',32,[u.low,u.high,0,0]);
assert.equal(c.api.dispatcher_module(c.low,c.high),0);refresh(c);
const p=c.view.getUint32(c.base+TRANSFER+24,true),n=c.view.getUint32(c.base+TRANSFER+28,true);
const dispatcherBytes=Buffer.from(c.bytes.subarray(p,p+n));writeFileSync(join(output,'dispatcher.wasm'),dispatcherBytes,{flag:'wx'});
const dispatcher=new WebAssembly.Instance(new WebAssembly.Module(dispatcherBytes),{env:{memory:c.memory,table},
  ring3:{guard_dispatch_entry:c.api.guard_dispatch_entry,find_installed_resident:c.api.find_installed_resident}}).exports.run;
const r=[...registers];r[0]=17;r[1]=5;r[2]=0;seed(c,r,PC,0xcd7);
const done=[...r];done[0]=3;done[2]=2;run(c,{run:dispatcher},9,done,gate,0xcd7,8,2,gateId,installed);stats.dispatcher_runs++;
const stopped=arena(c).subarray(0,96);assert.equal(c.api.capture_resident_call(c.low,c.high,u.low,u.high,1,0),0);
assert.deepEqual(arena(c).subarray(0,96),stopped,'V5 Gate accepted without CPU repair');
const token=refresh(c).view.getUint32(c.base+TRANSFER+16,true);assert.ok(token>0);
assert.equal(c.api.complete_resident_call(c.low,c.high,u.low,u.high,token,77),0);
const completed=[...done];completed[0]=77;completed[4]=0x9004;
assert.deepEqual(arena(c).subarray(0,56),state(completed,COLD,0xcd7));
assert.deepEqual(arena(c).subarray(56,96),exit(3,0,0,3));
r[1]=0;seed(c,r,PC,0xcd7);run(c,{run:dispatcher},9,r,PC,0xcd7,10,0,0,installed);stats.dispatcher_runs++;
const before=arena(c);assert.notEqual(c.api.capture_resident_call(c.low,c.high,u.low,u.high,1,0),0);assert.deepEqual(arena(c),before);
assert.equal(c.api.close(),0);
assert.equal(stats.numeric_cases,3*2*8*14*2);assert.equal(stats.prefix_faults,3);assert.equal(stats.dispatcher_runs,2);
assert.equal(stats.successful+stats.faults,stats.numeric_cases);
const result={status:'ok',stats,modules,observations,engine_sha256:sha256(engineBytes),arena_bytes:SIZE,
  sources:Object.fromEntries(['engine/tests/cpu_division.rs','engine/tests/cpu_division_wasm.rs','engine/tests/fixtures/p2-division/run.mjs'].map(path=>[path,sha256(readFileSync(join(root,path)))])),
  tools:{node:process.version,v8:process.versions.v8},
  scope:'register DWORD DIV/IDIV only; all8 source aliases,14 finite operand rows,2 FLAGS seeds, standalone/replacement/resident generated execution; precise zero/overflow and completed prefix, unchanged undefined FLAGS, V5 safepoints/NeedCode/Gate and dispatcher passthrough. Native callback injection separately covers V5 NeedCode consumers. No memory/byte/word division, full x86/Windows exception dispatch, browser, performance or full CI claim.'};
writeFileSync(join(output,'result.json'),JSON.stringify(result,null,2),{flag:'wx'});
console.log(JSON.stringify({status:result.status,stats,engine_sha256:result.engine_sha256,output}));
