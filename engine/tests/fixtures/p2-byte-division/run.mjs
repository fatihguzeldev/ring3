import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';

const [enginePath, output, root, arenaSize]=process.argv.slice(2), SIZE=Number(arenaSize), PC=0x1000, COLD=0x1800, TRANSFER=140;
const hash=b=>createHash('sha256').update(b).digest('hex'), engineBytes=readFileSync(enginePath), expectedHash=process.env.RING3_ENGINE_SHA256;
assert.match(expectedHash??'',/^[a-f0-9]{64}$/);assert.equal(hash(engineBytes),expectedHash);
const engineModule=new WebAssembly.Module(engineBytes);assert.deepEqual(WebAssembly.Module.imports(engineModule),[]);
const initial=readFileSync(join(output,'initial-arena.bin'));assert.equal(initial.length,SIZE);
const REG=[0xabcd0011,0x12345605,0x23456789,0x3456789a,0x9000,0x56789abc,0x6789abcd,0x789abcde], FLAGS=[2,0xcd7];
const rows=[[0x0011,5],[0,1],[0xffff,1],[0x01ff,2],[0x0200,2],[0x007f,1],[0x0080,1],[0xff80,1],
  [0xff7f,1],[0xfff9,3],[7,0xfd],[0xfff9,0xfd],[0x8000,0xff],[0x00fe,0xfe],[0xfffb,0xff],[0x1234,0]];
const stats={contexts:0,numeric_cases:0,success:0,faults:0,unsigned_ah_success:0,signed_ah_success:0,runs:0,preflight:0,prefix_faults:0,source_repairs:0,controls:0};
const modules=[],observations=[],frames=[];
function record(magic,size,fields,version=1){const b=Buffer.alloc(size);b.write(magic);b.writeUInt16LE(version,4);b.writeUInt16LE(1,6);b.writeUInt32LE(size,8);fields.forEach((v,i)=>b.writeUInt32LE(v>>>0,16+i*4));return b;}
const state=(r,pc,f)=>record('R3ST',56,[...r,pc,f]), exit=(reason,retired)=>record('R3EX',40,[reason,retired,0,0,0,0],5);
const words=a=>{const b=Buffer.alloc(a.length*4);a.forEach((v,i)=>b.writeUInt32LE(v>>>0,i*4));return b;};
function refresh(c){c.bytes=new Uint8Array(c.memory.buffer);c.view=new DataView(c.memory.buffer);return c;}
function arena(c){return Buffer.from(refresh(c).bytes.subarray(c.base,c.base+SIZE));}
function seed(c,r,pc,f){refresh(c).bytes.set(state(r,pc,f),c.base);c.bytes.set(exit(3,0),c.base+56);c.view.setUint32(c.base+96,0,true);}
function request(c,b){assert.ok(b.length<=4096);refresh(c).bytes.set(b,c.base+TRANSFER);}
function fresh(owner,entries){
  stats.contexts++;
  if(owner==='standalone'){const c={owner,entries,memory:new WebAssembly.Memory({initial:1}),base:128};refresh(c).bytes.set(initial,c.base);return c;}
  const instance=new WebAssembly.Instance(engineModule,{}),names=['open','close','arena_ptr','map','upload','compile','compile_entries','compile_resident','compile_resident_entries','generation','module_ptr','module_len','guard','guard_resident'];
  const api=Object.fromEntries(names.map(n=>[n,instance.exports[`ring3_abi_v1_${n}`]]));for(const fn of Object.values(api))assert.equal(typeof fn,'function');
  const c={owner,entries,api,memory:instance.exports.memory,low:stats.contexts,high:0xc9476000};
  assert.equal(api.open(1,c.low,c.high),0);c.base=api.arena_ptr()>>>0;assert.deepEqual(arena(c),initial);const before=arena(c);assert.equal(api.map(PC,1,7),0);assert.deepEqual(arena(c),before);return c;
}
function compile(c,name,pc,specs){
  assert.ok(specs.length>=1&&specs.length<=8);let generated,binding={};
  if(c.owner==='standalone')generated=readFileSync(join(output,`standalone-${c.entries?'entries':'extent'}-${name}.wasm`));
  else{
    request(c,readFileSync(join(output,`${name}.x86`)));let before=arena(c);assert.equal(c.api.upload(pc,name==='chain'?0x206:128),0);assert.deepEqual(arena(c),before);
    request(c,words(c.entries?specs.map(s=>s[0]):specs.flat()));before=arena(c);
    if(c.owner==='replacement'){
      assert.equal(c.entries?c.api.compile_entries(specs.length,0):c.api.compile(specs.length),0);assert.deepEqual(arena(c),before);
      binding={generation:c.api.generation(),pointer:c.api.module_ptr()>>>0,length:c.api.module_len()>>>0};
    }else{
      assert.equal(c.entries?c.api.compile_resident_entries(specs.length,0):c.api.compile_resident(specs.length),0);refresh(c);
      binding=Object.fromEntries(['low','high','pointer','length'].map((n,i)=>[n,c.view.getUint32(c.base+TRANSFER+8+i*4,true)]));
      before.set(words([1,24,binding.low,binding.high,binding.pointer,binding.length]),TRANSFER);assert.deepEqual(arena(c),before);
    }
    generated=Buffer.from(refresh(c).bytes.subarray(binding.pointer,binding.pointer+binding.length));
  }
  const module=new WebAssembly.Module(generated);assert.deepEqual(WebAssembly.Module.imports(module),[{module:'env',name:'memory',kind:'memory'},...(c.owner==='standalone'?[]:[{module:'ring3',name:c.owner==='replacement'?'guard':'guard_resident',kind:'function'}])]);
  const label=`${c.owner}-${c.entries?'entries':'extent'}-${name}`;writeFileSync(join(output,`actual-${label}.wasm`),generated,{flag:'wx'});modules.push({label,bytes:generated.length,sha256:hash(generated)});
  return {...binding,run:new WebAssembly.Instance(module,{env:{memory:c.memory},...(c.api?{ring3:c.api}:{})}).exports.run};
}
function run(c,u,budget,r,pc,f,reason=1,retired=1){const before=arena(c),beforeFrame=frames.length,wanted=Buffer.from(before);wanted.set(state(r,pc,f));wanted.set(exit(reason,retired),56);assert.equal(u.run(c.base,c.base+56,budget,c.base+96),0);const after=arena(c);assert.deepEqual(after,wanted,'full arena, old EAX upper16, other GPR/FLAGS/helper/tail exact');frames.push(before,after);stats.runs++;observations.push({owner:c.owner,entries:c.entries,before_frame:beforeFrame,after_frame:beforeFrame+1,state_hex:after.subarray(0,56).toString('hex'),exit_hex:after.subarray(56,96).toString('hex')});}
function operand(r,source){return (r[source%4]>>>(source<4?0:8))&255;}
function setByte(r,source,value){const p=source%4,s=source<4?0:8;r[p]=((r[p]&~(255<<s))|((value&255)<<s))>>>0;}
function divide(kind,r,source){const raw=BigInt(r[0]&65535),byte=BigInt(operand(r,source)),n=kind==='signed'?BigInt.asIntN(16,raw):raw,d=kind==='signed'?BigInt.asIntN(8,byte):byte;if(d===0n)return null;const q=n/d;if(kind==='signed'?(q< -128n||q>127n):q>255n)return null;const rem=n%d,out=[...r];out[0]=Number((BigInt(r[0])&0xffff0000n)|(BigInt.asUintN(8,rem)<<8n)|BigInt.asUintN(8,q));return out;}
for(const [kind,ax,d,wanted] of [['unsigned',0x11,5,0x203],['unsigned',0x1ff,2,0x1ff],['unsigned',0x200,2,null],['signed',0xfff9,3,0xfffe],['signed',7,0xfd,0x1fe],['signed',0xff80,1,0x80],['signed',0x80,1,null],['signed',0x8000,0xff,null]]){const r=[...REG];r[0]=(0xabcd0000|ax)>>>0;setByte(r,1,d);const got=divide(kind,r,1);assert.equal(got?got[0]&65535:null,wanted,'literal quotient/remainder/fault');}
for(const [kind,source,ax,wanted] of [['unsigned',0,0x11,1],['unsigned',0,0x101,null],['signed',0,0xfe,0x81],['signed',4,0xfffb,5],['signed',4,0xff80,null]]){const r=[...REG];r[0]=(0xabcd0000|ax)>>>0;const got=divide(kind,r,source);assert.equal(got?got[0]&65535:null,wanted,'literal AL/AH alias anchor');}
for(const owner of ['standalone','replacement','resident'])for(const entries of [false,true]){
  for(const kind of ['unsigned','signed']){
    const c=fresh(owner,entries),u=compile(c,kind,PC,Array.from({length:8},(_,s)=>[PC+s*16,7]));seed(c,REG,PC,2);
    let before=arena(c);assert.equal(u.run(0xffffffff,c.base+56,0,c.base+96),1);assert.deepEqual(arena(c),before);stats.preflight++;
    refresh(c).view.setUint32(c.base+12,1,true);before=arena(c);assert.equal(u.run(c.base,c.base+56,0,c.base+96),2);assert.deepEqual(arena(c),before);refresh(c).view.setUint32(c.base+12,0,true);stats.preflight++;
    run(c,u,0,REG,PC,2,1,0);refresh(c).view.setUint32(c.base+96,1,true);run(c,u,1,REG,PC,2,2,0);refresh(c).view.setUint32(c.base+96,0,true);
    for(let source=0;source<8;source++)for(const [row,[ax,d]] of rows.entries())for(const f of FLAGS){
      const r=[...REG];r[0]=(0xabcd0000|ax)>>>0;if(source!==0&&source!==4)setByte(r,source,d);
      const entry=PC+source*16,result=divide(kind,r,source);seed(c,r,entry,f);stats.numeric_cases++;
      if(kind==='unsigned'&&source===4)assert.equal(result,null,'DIV AH always faults');
      if(result){run(c,u,1,result,entry+2,f);run(c,u,1,result,COLD,f);run(c,u,1,result,COLD,f,3,0);stats.success++;if(source===4){if(kind==='unsigned')stats.unsigned_ah_success++;else stats.signed_ah_success++;}}
      else{run(c,u,1,r,entry,f,10,0);run(c,u,1,r,entry,f,10,0);stats.faults++;}
      observations.at(-1).input={kind,source,row,flags:f,registers:r};
    }
    if(c.api){request(c,Buffer.from([0xf6]));assert.equal(c.api.upload(PC,1),0);before=arena(c);assert.equal(u.run(c.base,c.base+56,1,c.base+96),4);assert.deepEqual(arena(c),before);stats.controls++;assert.equal(c.api.close(),0);before=arena(c);assert.equal(u.run(c.base,c.base+56,1,c.base+96),5);assert.deepEqual(arena(c),before);stats.controls++;}
  }
  const c=fresh(owner,entries),u=compile(c,'chain',0x1400,[[0x1400,16],[0x1600,6]]),r=[...REG];seed(c,r,0x1400,0xcd7);
  r[0]=0xabcd0001;run(c,u,1,r,0x1402,0xcd7);r[0]=0xabcdfffb;run(c,u,2,r,0x1406,0xcd7,1,2);
  r[0]=0xabcd0005;run(c,u,1,r,0x1408,0xcd7);r[0]=0xabcd0001;run(c,u,1,r,0x140a,0xcd7);
  r[0]=0xabcd0000;run(c,u,9,r,0x140c,0xcd7,10,1);run(c,u,9,r,0x140c,0xcd7,10,0);stats.prefix_faults++;
  const retry=[...REG];seed(c,retry,0x1600,0xcd7);setByte(retry,1,0);run(c,u,9,retry,0x1602,0xcd7,10,1);run(c,u,9,retry,0x1602,0xcd7,10,0);
  const repaired=arena(c);repaired[20]=5;refresh(c).bytes[c.base+20]=5;assert.deepEqual(arena(c),repaired,'only declared source CL repaired; retained dividend/EIP/FLAGS/exit/helper/tail unchanged');setByte(retry,1,5);
  const result=divide('unsigned',retry,1);assert.ok(result);run(c,u,1,result,0x1604,0xcd7);run(c,u,1,result,0x1606,0xcd7);run(c,u,1,result,0x1606,0xcd7,3,0);stats.source_repairs++;if(c.api)assert.equal(c.api.close(),0);
}
assert.equal(stats.contexts,18);assert.equal(stats.numeric_cases,6*2*8*16*2);assert.equal(stats.success+stats.faults,stats.numeric_cases);assert.equal(stats.unsigned_ah_success,0);assert.ok(stats.signed_ah_success>0);assert.equal(stats.preflight,24);assert.equal(stats.prefix_faults,6);assert.equal(stats.source_repairs,6);assert.equal(stats.controls,16);
const raw=Buffer.concat(frames);writeFileSync(join(output,'arenas.bin'),raw,{flag:'wx'});
const result={status:'ok',stats,modules,observations,raw:{file:'arenas.bin',frames:frames.length,bytes:raw.length,sha256:hash(raw)},engine_sha256:hash(engineBytes),arena_bytes:SIZE,tools:{node:process.version,v8:process.versions.v8},sources:Object.fromEntries(['engine/tests/cpu_byte_division.rs','engine/tests/cpu_byte_division_wasm.rs','engine/tests/fixtures/p2-byte-division/run.mjs'].map(p=>[p,hash(readFileSync(join(root,p)))])),scope:'prefix-free register BYTE DIV/IDIV; six extent/entry standalone/replacement/resident profiles, all8 byte aliases, sixteen authored numeric rows/two FLAGS seeds, literal quotient/remainder and AL/AH anchors. Current CPU split DIV AL→MOV AH/AL→IDIV AH→DIV AL→zero fault and separate CL-only repaired retry; full arena default opaque x87 tail preserved. V5 precise zero/range faults, no Wasm trap, preflight/budget/cancel/NeedCode/stale/closed. No memory/word/x64 division, arbitrary raw80 seed, hardware undefined FLAGS, browser/performance/fullCI claim.'};
writeFileSync(join(output,'result.json'),JSON.stringify(result,null,2),{flag:'wx'});console.log(JSON.stringify({status:result.status,stats,engine_sha256:result.engine_sha256,output}));
