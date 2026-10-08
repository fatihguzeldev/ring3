import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';

const [enginePath, output, root, arenaSize] = process.argv.slice(2);
const SIZE=Number(arenaSize), TRANSFER=140, PC=0x1000, COLD=0x1800, DATA=0x4000, SECOND=0x5000, TOP=0xfffff000;
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
const engineBytes=readFileSync(enginePath), expectedHash=process.env.RING3_ENGINE_SHA256;
assert.match(expectedHash??'',/^[a-f0-9]{64}$/);assert.equal(hash(engineBytes),expectedHash);
const engineModule=new WebAssembly.Module(engineBytes);assert.deepEqual(WebAssembly.Module.imports(engineModule),[]);
const initial=readFileSync(join(output,'initial-arena.bin'));assert.equal(initial.length,SIZE);
const rows=[[17,0,5],[0,0,1],[0xffffffff,0,1],[0,1,1],[0xffffffff,0xfffffffe,0xffffffff],
  [0xfffffff9,0xffffffff,3],[7,0,0xfffffffd],[0xfffffff9,0xffffffff,0xfffffffd],
  [0x80000000,0xffffffff,1],[0x80000000,0xffffffff,0xffffffff],[0x7fffffff,0,1],
  [0x80000000,0,1],[0,0x80000000,0xffffffff],[0x12345678,0,0]];
const FLAGS=[2,0xcd7], REG=[17,0x23456789,0,DATA,DATA+0x40,DATA+0x80,0x789abcde,DATA+0x80];
const tails=[[0x03],[0x00],[0x02],[0x04,0x24],[0x44,0x8f,0x80],[0x45,0x80],[0x83,0x10,0,0,0],[0x05,0x10,0x50,0,0]];
const stats={contexts:0,numeric_cases:0,ea_cases:0,boundaries:0,read_faults:0,divide_repairs:0,retained_retries:0,prefix_faults:0,runs:0,pages:0,controls:0};
const observations=[], modules=[];
function record(magic,size,fields,version=1){const b=Buffer.alloc(size);b.write(magic);b.writeUInt16LE(version,4);b.writeUInt16LE(1,6);b.writeUInt32LE(size,8);fields.forEach((v,i)=>b.writeUInt32LE(v>>>0,16+i*4));return b;}
const state=(r,pc,f)=>record('R3ST',56,[...r,pc,f]);
const exit=(reason,retired,detail=0,address=0)=>record('R3EX',40,[reason,retired,detail,address,reason===5?1:0,reason===5?4:0],5);
const helper=(value=0,detail=0,address=0)=>record('R3MH',40,[detail?1:0,detail?0:value,detail,address,detail?1:0,detail?4:0]);
const words=a=>{const b=Buffer.alloc(a.length*4);a.forEach((v,i)=>b.writeUInt32LE(v>>>0,i*4));return b;};
function refresh(c){c.bytes=new Uint8Array(c.memory.buffer);c.view=new DataView(c.memory.buffer);return c;}
function arena(c){return Buffer.from(refresh(c).bytes.subarray(c.base,c.base+SIZE));}
function pure(c,fn){const before=arena(c);assert.equal(fn(),0);assert.deepEqual(arena(c),before);}
function request(c,b){assert.ok(b.length<=4096);refresh(c).bytes.set(b,c.base+TRANSFER);}
function map(c,address){pure(c,()=>c.api.map(address,1,3));c.pages.set(address,Buffer.alloc(4096));}
function unmap(c,address){pure(c,()=>c.api.unmap(address,1));c.pages.delete(address);}
function protect(c,address,permission){pure(c,()=>c.api.protect(address,1,permission));}
function upload(c,address,b){request(c,b);pure(c,()=>c.api.upload(address,b.length));for(let i=0;i<b.length;i++){const at=address+i,page=Math.floor(at/4096)*4096;if(c.pages.has(page))c.pages.get(page)[at%4096]=b[i];}}
function put(c,address,value){upload(c,address,words([value]));}
function fresh(owner,kind){
  const instance=new WebAssembly.Instance(engineModule,{}), names=['open','close','arena_ptr','map','unmap','protect','upload','read32','write8','compile','compile_resident','generation','module_ptr','module_len','guard','guard_resident'];
  const api=Object.fromEntries(names.map(name=>[name,instance.exports[`ring3_abi_v1_${name}`]]));for(const fn of Object.values(api))assert.equal(typeof fn,'function');
  const c={owner,kind,api,memory:instance.exports.memory,pages:new Map(),low:++stats.contexts,high:0xc9466000};
  assert.equal(api.open(7,c.low,c.high),0);c.base=api.arena_ptr()>>>0;assert.deepEqual(arena(c),initial);
  pure(c,()=>api.map(PC,1,7));for(const address of [0,DATA,SECOND,TOP])map(c,address);
  const bytes=Buffer.alloc(0x120,0xcc),specs=[];
  for(const [index,tail] of tails.entries()){
    const form=[0xf7,...tail];form[1]|=kind==='unsigned'?0x30:0x38;
    const at=index*16;bytes.set(form,at);bytes[at+form.length]=0xe9;bytes.writeInt32LE(COLD-(PC+at+form.length+5),at+form.length+1);specs.push([PC+at,form.length+5]);
  }
  bytes.set([0x8d,0x49,1,0xf7,kind==='unsigned'?0x33:0x3b,0xe9],0x100);bytes.writeInt32LE(COLD-(PC+0x10a),0x106);specs.push([PC+0x100,10]);
  writeFileSync(join(output,`${owner}-${kind}.x86`),bytes,{flag:'wx'});upload(c,PC,bytes);request(c,words(specs.flat()));
  let binding;
  if(owner==='replacement'){pure(c,()=>api.compile(specs.length));binding={generation:api.generation(),pointer:api.module_ptr()>>>0,length:api.module_len()>>>0};}
  else {const before=arena(c);assert.equal(api.compile_resident(specs.length),0);refresh(c);binding=Object.fromEntries(['low','high','pointer','length'].map((key,i)=>[key,c.view.getUint32(c.base+TRANSFER+8+i*4,true)]));before.set(words([1,24,binding.low,binding.high,binding.pointer,binding.length]),TRANSFER);assert.deepEqual(arena(c),before);}
  const generated=Buffer.from(refresh(c).bytes.subarray(binding.pointer,binding.pointer+binding.length)),module=new WebAssembly.Module(generated);
  assert.deepEqual(WebAssembly.Module.imports(module),[{module:'env',name:'memory',kind:'memory'},{module:'ring3',name:owner==='replacement'?'guard':'guard_resident',kind:'function'},{module:'ring3',name:'read32',kind:'function'}]);
  const u={...binding,run:new WebAssembly.Instance(module,{env:{memory:c.memory},ring3:api}).exports.run};
  writeFileSync(join(output,`${owner}-${kind}.wasm`),generated,{flag:'wx'});modules.push({owner,kind,bytes:generated.length,sha256:hash(generated)});return [c,u];
}
function seed(c,r,pc,f){refresh(c).bytes.set(state(r,pc,f),c.base);c.bytes.set(exit(3,0),c.base+56);c.view.setUint32(c.base+96,0,true);c.bytes.set(helper(0xdecafbad),c.base+100);}
function run(c,u,budget,r,pc,f,reason=1,retired=1,value,detail=0,address=0){
  const wanted=arena(c);wanted.set(state(r,pc,f));wanted.set(exit(reason,retired,detail,address),56);if(value!==undefined||reason===5)wanted.set(helper(value,detail,address),100);
  assert.equal(u.run(c.base,c.base+56,budget,c.base+96),0);assert.deepEqual(arena(c),wanted,'full arena including x87/transfer exact');stats.runs++;
  observations.push({owner:c.owner,kind:c.kind,state_hex:arena(c).subarray(0,56).toString('hex'),exit_hex:arena(c).subarray(56,96).toString('hex'),helper_hex:arena(c).subarray(100,140).toString('hex')});
}
function divide(kind,r,value){const bits=BigInt(r[2])*4294967296n+BigInt(r[0]),n=kind==='signed'?BigInt.asIntN(64,bits):bits,d=kind==='signed'?BigInt.asIntN(32,BigInt(value)):BigInt(value);if(d===0n)return null;const q=n/d;if(kind==='signed'?(q< -2147483648n||q>2147483647n):q>4294967295n)return null;const out=[...r];out[0]=Number(BigInt.asUintN(32,q));out[2]=Number(BigInt.asUintN(32,n%d));return out;}
for(const [kind,r,d,wanted] of [['unsigned',[17,0,0],5,[3,2]],['unsigned',[0xffffffff,0,0],1,[0xffffffff,0]],['signed',[0xfffffff9,0,0xffffffff],3,[0xfffffffe,0xffffffff]],['signed',[0,0,0x80000000],0xffffffff,null]]){const result=divide(kind,r,d);assert.deepEqual(result?[result[0],result[2]]:null,wanted);}
function division(c,u,r,entry,f,value){const result=divide(c.kind,r,value);if(result)run(c,u,1,result,entry+tails[(entry-PC)/16].length+1,f,1,1,value);else run(c,u,1,r,entry,f,10,0,value);return result;}
function pages(c){for(const [address,expected] of c.pages){const actual=Buffer.alloc(4096);for(let offset=0;offset<4096;offset+=4){const before=arena(c);assert.equal(c.api.read32(address+offset),0);refresh(c);const value=c.view.getUint32(c.base+120,true);actual.writeUInt32LE(value,offset);before.set(helper(expected.readUInt32LE(offset)),100);assert.deepEqual(arena(c),before);}assert.deepEqual(actual,expected);stats.pages++;}}
for(const owner of ['replacement','resident'])for(const kind of ['unsigned','signed']){
  const [c,u]=fresh(owner,kind);
  for(const [eax,edx,value] of rows)for(const f of FLAGS){const r=[...REG];r[0]=eax;r[2]=edx;put(c,DATA,value);seed(c,r,PC,f);const result=division(c,u,r,PC,f,value);if(result){run(c,u,1,result,COLD,f);run(c,u,1,result,COLD,f,3,0);}else run(c,u,1,r,PC,f,10,0,value);stats.numeric_cases++;}
  for(const form of [1,2,3,4,5,6,7])for(const f of FLAGS){
    const r=[...REG];let address=DATA,value=5;
    if(form===1){r[0]=DATA;address=DATA;}
    if(form===2){r[2]=DATA;value=kind==='unsigned'?0xffffffff:0x7fffffff;}
    if(form===3){r[4]=DATA;}
    if(form===4){r[7]=DATA;r[1]=32;}
    if(form===5){r[5]=DATA+128;}
    if(form===6){r[3]=DATA-16;}
    if(form===7){address=SECOND+16;}
    put(c,address,value);seed(c,r,PC+form*16,f);assert.ok(division(c,u,r,PC+form*16,f,value));stats.ea_cases++;
  }
  for(const [form,address,base] of [[0,0x4ffe,0x4ffe],[0,0xfffffffc,0xfffffffc],[6,8,0xfffffff8]])for(const f of FLAGS){const r=[...REG];r[3]=base;put(c,address,5);seed(c,r,PC+form*16,f);assert.ok(division(c,u,r,PC+form*16,f,5));stats.boundaries++;}
  for(const shape of [{address:0x8000,detail:1,fault:0x8000,map:0x8000},{address:DATA,detail:2,fault:DATA,permission:DATA},{address:0x4ffe,detail:1,fault:SECOND,map:SECOND},{address:0x4ffe,detail:2,fault:SECOND,permission:SECOND},{address:0xffffffff,detail:3,fault:0xffffffff}]){
    if(shape.permission||shape.map===SECOND)put(c,shape.address,0);
    if(shape.map===SECOND)unmap(c,SECOND);if(shape.permission)protect(c,shape.permission,2);
    const r=[...REG];r[3]=shape.address;seed(c,r,PC,0xcd7);
    run(c,u,0,r,PC,0xcd7,1,0);refresh(c).view.setUint32(c.base+96,1,true);run(c,u,1,r,PC,0xcd7,2,0);refresh(c).view.setUint32(c.base+96,0,true);
    run(c,u,1,r,PC,0xcd7,5,0,undefined,shape.detail,shape.fault);run(c,u,1,r,PC,0xcd7,5,0,undefined,shape.detail,shape.fault);stats.read_faults++;
    if(shape.detail!==3){if(shape.map)map(c,shape.map);if(shape.permission)protect(c,shape.permission,3);put(c,shape.address,5);const result=divide(kind,r,5);assert.ok(result);run(c,u,1,result,PC+2,0xcd7,1,1,5);stats.retained_retries++;}
  }
  for(const [edx,bad,repair] of [[0,0,5],[1,1,0xffffffff]]){
    const r=[...REG];r[2]=edx;put(c,DATA,bad);seed(c,r,PC,0xcd7);
    assert.equal(divide(kind,r,bad),null);run(c,u,1,r,PC,0xcd7,10,0,bad);run(c,u,1,r,PC,0xcd7,10,0,bad);
    const fixed=kind==='signed'&&edx===1?0x7fffffff:repair;put(c,DATA,fixed);const result=divide(kind,r,fixed);assert.ok(result);run(c,u,1,result,PC+2,0xcd7,1,1,fixed);stats.divide_repairs++;
  }
  const r=[...REG];r[3]=0x8000;unmap(c,0x8000);seed(c,r,PC+0x100,0xcd7);const prefix=[...r];prefix[1]=(prefix[1]+1)>>>0;
  run(c,u,9,prefix,PC+0x103,0xcd7,5,1,undefined,1,0x8000);run(c,u,9,prefix,PC+0x103,0xcd7,5,0,undefined,1,0x8000);
  map(c,0x8000);put(c,0x8000,5);const result=divide(kind,prefix,5);run(c,u,1,result,PC+0x105,0xcd7,1,1,5);stats.prefix_faults++;stats.retained_retries++;
  pages(c);
  upload(c,PC,Buffer.from([0xf7]));const before=arena(c);assert.equal(u.run(c.base,c.base+56,1,c.base+96),4);assert.deepEqual(arena(c),before);stats.controls++;
  assert.equal(c.api.close(),0);const closed=arena(c);assert.equal(u.run(c.base,c.base+56,1,c.base+96),5);assert.deepEqual(arena(c),closed);stats.controls++;
}
assert.equal(stats.numeric_cases,2*2*14*2);assert.equal(stats.ea_cases,2*2*7*2);assert.equal(stats.boundaries,2*2*3*2);assert.equal(stats.read_faults,2*2*5);assert.equal(stats.divide_repairs,2*2*2);assert.equal(stats.prefix_faults,4);assert.equal(stats.retained_retries,20);assert.equal(stats.controls,8);
const result={status:'ok',stats,modules,observations,engine_sha256:hash(engineBytes),arena_bytes:SIZE,tools:{node:process.version,v8:process.versions.v8},
  sources:Object.fromEntries(['engine/tests/cpu_memory_division.rs','engine/tests/cpu_memory_division_wasm.rs','engine/tests/fixtures/p2-memory-division/run.mjs'].map(path=>[path,hash(readFileSync(join(root,path)))])),
  scope:'prefix-free memory DWORD DIV/IDIV, two bound owners, finite112 numeric cases/56 EA cases including old EAX/EDX/24 valid crossing and modulo32 boundaries; precise Read4 priority and sameCPU/module repairs, V5 arithmetic faults, preserved undefinedFLAGS and opaque x87 tail, successful source RAM unchanged, budget/cancel/stale/closed. No store, REP/word/byte/x64 division, hardware undefinedFLAGS, browser/game/performance/fullCI claim.'};
writeFileSync(join(output,'result.json'),JSON.stringify(result,null,2),{flag:'wx'});console.log(JSON.stringify({status:result.status,stats,engine_sha256:result.engine_sha256,output}));
