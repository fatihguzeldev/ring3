import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';

const [enginePath, output, root, arenaSize]=process.argv.slice(2);
const SIZE=Number(arenaSize),TRANSFER=140,FP=4236,PC=0x1000,COLD=0x1800,DATA=0x5000,BASE=DATA+128;
const hash=b=>createHash('sha256').update(b).digest('hex');
const engineBytes=readFileSync(enginePath),expectedHash=process.env.RING3_ENGINE_SHA256;
assert.match(expectedHash??'',/^[a-f0-9]{64}$/);assert.equal(hash(engineBytes),expectedHash);
const engineModule=new WebAssembly.Module(engineBytes);assert.deepEqual(WebAssembly.Module.imports(engineModule),[]);
const initial=readFileSync(join(output,'initial-arena.bin')),opaque=readFileSync(join(output,'opaque-fp.bin'));
assert.equal(SIZE,4364);assert.equal(initial.length,SIZE);assert.equal(opaque.length,128);
const REG=[0xa53c0011,0,0x3456789a,BASE,0x9000,0x56789abc,0x6789abcd,0x789abcde],FLAGS=[2,0xcd7];
const indexes=[0,31,32,33,-1,-32,-33,-2147483648,2147483647],immediates=[0,31,32,255];
const forms=[[0x0f,0xa3,0x0b],[0x0f,0xa3,0x00],[0x0f,0xa3,0x4c,0x8a,0x80],
  ...immediates.map(n=>[0x0f,0xba,0x23,n]),[0xb9,32,0,0,0,0x0f,0xa3,0x0b]];
const stats={contexts:0,numeric:0,aliases:0,boundaries:0,read_faults:0,repairs:0,chains:0,runs:0,preflight:0,controls:0};
const observations=[],modules=[],frames=[],events=[];
const sources=['engine/tests/cpu_memory_bit_test32.rs','engine/tests/cpu_memory_bit_test32_wasm.rs','engine/tests/fixtures/p2-memory-bit-test32/run.mjs'];
const sourcePins=()=>Object.fromEntries(sources.map(p=>[p,hash(readFileSync(join(root,p)))])),initialSources=sourcePins();
function record(magic,size,fields,version=1){const b=Buffer.alloc(size);b.write(magic);b.writeUInt16LE(version,4);b.writeUInt16LE(1,6);b.writeUInt32LE(size,8);fields.forEach((v,i)=>b.writeUInt32LE(v>>>0,16+i*4));return b;}
const state=(r,pc,f)=>record('R3ST',56,[...r,pc,f]);
const exit=(reason,retired,detail=0,address=0)=>record('R3EX',40,[reason,retired,detail,address,reason===5?1:0,reason===5?4:0],2);
const helper=(value=0,detail=0,address=0)=>record('R3MH',40,[detail?1:0,detail?0:value,detail,address,detail?1:0,detail?4:0]);
const words=a=>{const b=Buffer.alloc(a.length*4);a.forEach((v,i)=>b.writeUInt32LE(v>>>0,i*4));return b;};
function refresh(c){c.bytes=new Uint8Array(c.memory.buffer);c.view=new DataView(c.memory.buffer);return c;}
function arena(c){return Buffer.from(refresh(c).bytes.subarray(c.base,c.base+SIZE));}
function capture(c,before,after,data){const beforeFrame=frames.length;frames.push(before,after);observations.push({context:c.label,before_frame:beforeFrame,after_frame:beforeFrame+1,...data});}
function pure(c,fn){const before=arena(c);assert.equal(fn(),0);assert.deepEqual(arena(c),before);}
function request(c,b){assert.ok(b.length<=4096);refresh(c).bytes.set(b,c.base+TRANSFER);}
function upload(c,address,b){request(c,b);pure(c,()=>c.api.upload(address,b.length));events.push({context:c.label,type:'upload',address,hex:Buffer.from(b).toString('hex'),before_frame:frames.length});}
function put(c,address,value){upload(c,address,words([value]));}
function map(c,address,pages=1){pure(c,()=>c.api.map(address,pages,3));events.push({context:c.label,type:'map',address,pages,bits:3,before_frame:frames.length});}
function protect(c,address,bits){pure(c,()=>c.api.protect(address,1,bits));events.push({context:c.label,type:'protect',address,pages:1,bits,before_frame:frames.length});}
function unmap(c,address){pure(c,()=>c.api.unmap(address,1));events.push({context:c.label,type:'unmap',address,pages:1,before_frame:frames.length});}
function seed(c,r,pc,f){refresh(c).bytes.set(state(r,pc,f),c.base);c.bytes.set(exit(3,0),c.base+56);c.view.setUint32(c.base+96,0,true);c.bytes.set(helper(0xad),c.base+100);c.bytes.set(opaque,c.base+FP);}
function program(){const saved=readFileSync(join(output,'bt.x86')),wanted=Buffer.alloc(128,0xcc),specs=[];for(const [i,form]of forms.entries()){const at=i*16;wanted.set(form,at);wanted[at+form.length]=0xe9;wanted.writeInt32LE(COLD-(PC+at+form.length+5),at+form.length+1);specs.push([PC+at,form.length+5]);}assert.deepEqual(saved,wanted);assert.equal(specs.length,8);return{saved,specs};}
function fresh(owner,entries){const instance=new WebAssembly.Instance(engineModule,{}),names=['open','close','arena_ptr','map','unmap','protect','upload','compile','compile_entries','compile_resident','compile_resident_entries','generation','module_ptr','module_len','guard','guard_resident','read32'];
 const api=Object.fromEntries(names.map(n=>[n,instance.exports[`ring3_abi_v1_${n}`]]));for(const fn of Object.values(api))assert.equal(typeof fn,'function');
 const c={owner,entries,label:`${owner}-${entries?'entries':'extent'}`,api,memory:instance.exports.memory,low:++stats.contexts,high:0xc9506000};assert.equal(api.open(8,c.low,c.high),0);c.base=api.arena_ptr()>>>0;assert.deepEqual(arena(c),initial);
 pure(c,()=>api.map(PC,1,7));events.push({context:c.label,type:'map',address:PC,pages:1,bits:7,before_frame:frames.length});
 map(c,DATA,2);for(const at of [0xfffff000,0,0xf0005000,0x10005000])map(c,at);
 const p=program();upload(c,PC,p.saved);protect(c,PC,5);request(c,words(entries?p.specs.map(s=>s[0]):p.specs.flat()));let binding;
 if(owner==='replacement'){pure(c,()=>entries?api.compile_entries(8,0):api.compile(8));binding={generation:api.generation(),pointer:api.module_ptr()>>>0,length:api.module_len()>>>0};}
 else{const before=arena(c);assert.equal(entries?api.compile_resident_entries(8,0):api.compile_resident(8),0);refresh(c);binding=Object.fromEntries(['low','high','pointer','length'].map((n,i)=>[n,c.view.getUint32(c.base+TRANSFER+8+i*4,true)]));before.set(words([1,24,binding.low,binding.high,binding.pointer,binding.length]),TRANSFER);assert.deepEqual(arena(c),before);}
 const generated=Buffer.from(refresh(c).bytes.subarray(binding.pointer,binding.pointer+binding.length)),module=new WebAssembly.Module(generated);
 assert.deepEqual(WebAssembly.Module.imports(module),[{module:'env',name:'memory',kind:'memory'},{module:'ring3',name:owner==='replacement'?'guard':'guard_resident',kind:'function'},{module:'ring3',name:'read32',kind:'function'}]);
 writeFileSync(join(output,`actual-${c.label}.wasm`),generated,{flag:'wx'});modules.push({context:c.label,bytes:generated.length,sha256:hash(generated),specs:p.specs});return[c,{...binding,run:new WebAssembly.Instance(module,{env:{memory:c.memory},ring3:api}).exports.run}];}
function run(c,u,budget,r,pc,f,reason=1,retired=1,value,detail=0,address=0,input={}){const before=arena(c),wanted=Buffer.from(before);wanted.set(state(r,pc,f));wanted.set(exit(reason,retired,detail,address),56);if(value!==undefined||reason===5)wanted.set(helper(value,detail,address),100);const status=u.run(c.base,c.base+56,budget,c.base+96),after=arena(c);assert.equal(status,0);assert.deepEqual(after,wanted,'full4364: GPR/FLAGS/precise target retirement/helper/transfer/opaqueFP continuity');capture(c,before,after,{type:'run',budget,status,input});stats.runs++;}
function neutral(c,u,status,pointer,type){const before=arena(c),actual=u.run(pointer,c.base+56,1,c.base+96),after=arena(c);assert.equal(actual,status);assert.deepEqual(after,before);capture(c,before,after,{type,status:actual});}
function offset(index){return Number(4n*(BigInt(index)>>5n));}
function selected(base,index){return Number(BigInt.asUintN(32,BigInt(base)+BigInt(offset(index))));}
function resultFlags(f,word,index){return(f&0x442)|((word>>>(index&31))&1);}
for(const [index,base,address,bit]of [[-1,BASE,BASE-4,31],[-32,BASE,BASE-4,0],[-33,BASE,BASE-8,31],[32,BASE,BASE+4,0],[-2147483648,BASE,0xf0005080,0],[2147483647,BASE,0x1000507c,31]]){assert.equal(selected(base,index),address);assert.equal(index&31,bit);}
assert.equal(resultFlags(0xcd7,0x80000000,31),0x443);assert.equal(resultFlags(0xcd7,0,31),0x442);assert.equal(resultFlags(2,1,0),3);
for(const owner of ['replacement','resident'])for(const entries of [false,true]){
 const[c,u]=fresh(owner,entries);seed(c,REG,PC,2);neutral(c,u,1,0xffffffff,'invalid-pointer');stats.preflight++;
 refresh(c).view.setUint32(c.base+12,1,true);neutral(c,u,2,c.base,'invalid-state');refresh(c).view.setUint32(c.base+12,0,true);stats.preflight++;
 run(c,u,0,REG,PC,2,1,0);refresh(c).view.setUint32(c.base+96,1,true);run(c,u,1,REG,PC,2,2,0);run(c,u,0,REG,PC,2,2,0);refresh(c).view.setUint32(c.base+96,0,true);stats.controls+=3;
 for(const index of indexes)for(const bit of [0,1])for(const flags of FLAGS){const r=[...REG];r[1]=index>>>0;const address=selected(BASE,index),word=bit?(2**(index&31))>>>0:0;put(c,address,word);seed(c,r,PC,flags);run(c,u,1,r,PC+3,resultFlags(flags,word,index),1,1,word,0,0,{index,address,word});stats.numeric++;}
 for(const [form,raw]of immediates.entries())for(const bit of [0,1])for(const flags of FLAGS){const r=[...REG];r[1]=0x80000000;const word=bit?(2**(raw&31))>>>0:0;put(c,BASE,word);seed(c,r,PC+(form+3)*16,flags);run(c,u,1,r,PC+(form+3)*16+4,resultFlags(flags,word,raw),1,1,word,0,0,{raw,address:BASE,word});stats.numeric++;}
 // Old EAX supplies both ordinary EA and signed bit offset; EDX+ECX*4 disp8 takes both current oldECX uses.
 for(const form of [1,2]){const r=[...REG];let address,index;if(form===1){r[0]=BASE;index=BASE;address=selected(BASE,index);}else{r[2]=BASE+128;r[1]=33;index=33;address=BASE+136;}put(c,address,2**(index&31));seed(c,r,PC+form*16,0xcd7);run(c,u,1,r,PC+form*16+forms[form].length,0x443,1,1,2**(index&31),0,0,{form,index,address});stats.aliases++;}
 for(const [base,index,address]of [[0x5ffd,0,0x5ffd],[0xfffffffc,0,0xfffffffc],[0xfffffff0,128,0],[0, -1,0xfffffffc],[0x6000,-33,0x5ff8]]){const r=[...REG];r[3]=base;r[1]=index>>>0;const word=(2**(index&31))>>>0;put(c,address,word);seed(c,r,PC,0xcd7);run(c,u,1,r,PC+3,0x443,1,1,word,0,0,{boundary:true,base,index,address});stats.boundaries++;}
 for(const [address,detail,faultAddress,page]of [[BASE,1,BASE,DATA],[BASE,2,BASE,DATA],[0x5ffd,1,0x6000,0x6000],[0x5ffd,2,0x6000,0x6000],[0xfffffffd,3,0xfffffffd,null],[0xfffffffe,3,0xfffffffe,null],[0xffffffff,3,0xffffffff,null]]){const r=[...REG];r[3]=address;r[1]=0;if(page!==null){if(detail===1)unmap(c,page);else protect(c,page,2);}seed(c,r,PC,0xcd7);run(c,u,1,r,PC,0xcd7,5,0,undefined,detail,faultAddress,{read_fault:true,address,detail});run(c,u,1,r,PC,0xcd7,5,0,undefined,detail,faultAddress);stats.read_faults+=2;if(page!==null){if(detail===1)map(c,page);else protect(c,page,3);put(c,address,1);run(c,u,1,r,PC+3,0x443,1,1,1);stats.repairs++;}}
 const r=[...REG];r[1]=0xffffffdf;protect(c,DATA,2);seed(c,r,PC+112,0xcd7);r[1]=32;run(c,u,9,r,PC+117,0xcd7,5,1,undefined,2,BASE+4,{phase:'retired-current-index-prefix'});run(c,u,9,r,PC+117,0xcd7,5,0,undefined,2,BASE+4);
 protect(c,DATA,3);const beforeRepair=arena(c);put(c,BASE+4,1);const afterRepair=arena(c),wantedRepair=Buffer.from(beforeRepair);wantedRepair.set(words([1]),TRANSFER);assert.deepEqual(afterRepair,wantedRepair);capture(c,beforeRepair,afterRepair,{type:'host-ram-repair',address:BASE+4,value:1});
 run(c,u,1,r,PC+120,0x443,1,1,1,0,0,{phase:'repaired-current-index'});run(c,u,1,r,COLD,0x443);run(c,u,1,r,COLD,0x443,3,0);stats.chains++;
 protect(c,PC,7);upload(c,PC,Buffer.from([0x0f]));neutral(c,u,4,c.base,'stale');stats.controls++;assert.equal(c.api.close(),0);neutral(c,u,5,c.base,'closed');stats.controls++;
}
assert.deepEqual(stats,{contexts:4,numeric:208,aliases:8,boundaries:20,read_faults:56,repairs:16,chains:4,runs:340,preflight:8,controls:20});
assert.equal(observations.length,360);assert.equal(frames.length,720);
const raw=Buffer.concat(frames);writeFileSync(join(output,'arenas.bin'),raw,{flag:'wx'});writeFileSync(join(output,'engine.wasm'),engineBytes,{flag:'wx'});assert.deepEqual(sourcePins(),initialSources);
const result={status:'ok',stats,modules,observations,events,raw:{file:'arenas.bin',frames:frames.length,bytes:raw.length,sha256:hash(raw)},engine_sha256:hash(engineBytes),arena_bytes:SIZE,sources:initialSources,
 scope:'Prefix-free memory DWORD BT r32 signed-index/imm8 only in four bound profiles.208 finite numeric cases, oldEAX EA/index alias and EDX/ECX SIB disp8, four-byte unaligned/crosspage/wrapped-EA/topspan success/fault, repeated Read32 fault and sameCPU retained prefix/currentECX RAM/permission repair. All360 saved full4364 pairs include340 runs/8preflight/8staleclosed/4hostrepair; arbitrary valid opaqueFP128 remains unchanged. Saved upload/page history is bounded RAM input evidence, not fullRAM dump; no hardware undefinedFLAGS/performance/browser/game/fullCI claim.'};
writeFileSync(join(output,'result.json'),JSON.stringify(result,null,2),{flag:'wx'});console.log(JSON.stringify({status:'ok',stats,engine_sha256:result.engine_sha256,output}));
