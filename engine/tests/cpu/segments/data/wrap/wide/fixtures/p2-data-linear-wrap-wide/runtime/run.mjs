import {pageFaultExit, pageFaultPacket} from '../../../../../../../../support/wasm/page-fault.mjs';
import assert from 'node:assert/strict';
import {mkdirSync} from 'node:fs';
import {harness,PC,BASES,state,registers,words,record,exit,writeRAM} from './support.mjs';
const[enginePath,output,root]=process.argv.slice(2);assert.ok(enginePath&&output&&root);mkdirSync(output,{recursive:true});
const h=await harness(enginePath,output,root);
const FP=4236,SV=4364,XMM=4396,YU=4612,ONE=Buffer.from('0000000000000080ff3f','hex');
const VECTOR=Buffer.from('101112131415161718191a1b1c1d1e1f','hex'),OLD=Buffer.from('a0a1a2a3a4a5a6a7a8a9aaabacadaeaf','hex');
function uploadLinear(c,start,bytes){const first=Math.min(bytes.length,0x100000000-start);h.upload(c,start,bytes.subarray(0,first));if(first<bytes.length)h.upload(c,0,bytes.subarray(first));}
function canonical(c,context={}){const input=state(h.arena(c),context);input.set(h.initial.subarray(FP,SV+160),FP);input.set(h.initial.subarray(YU,YU+144),YU);return input;}
function pageFault([detail,address,access],length){return{errorCode:(access===2?6:4)+(detail===2?1:0),address,access,length};}
function packet(want,length,{value=0,fault,exception}={}){want.set(fault?pageFaultPacket(pageFault(fault,length)):record('R3MH',40,exception?[...exception,length]:[0,value,0,0,0,length],7),100);}
function step(input,length,{fault,reason=1,retired=1,detail=0}={}){const want=Buffer.from(input);if(retired)want.writeUInt32LE(PC+length,48);if(fault)want.set(pageFaultExit({retired:0,...pageFault(fault,fault[3])}),56);else exit(want,reason,retired,6,[detail,0,0,0]);return want;}
function bound(c,material,name,args,status=0){return{name,args:[...(c.resident?material.binding:[]),...args],status};}
function suffix(c,name){return c.resident?name+'_resident':name;}
function open(resident,entries,label,{low=3,high=3,smc=false}={}){return h.open(resident,entries,label,[[PC],...(high===null?[]:[[0xfffff000]]),...(low===null?[]:[[0]])]);}
function compile(c,label,hex,imports,{low=3,high=3,smc=false}={}){const instruction=Buffer.from(hex,'hex');h.upload(c,PC,Buffer.concat([instruction,Buffer.from('ebfe','hex')]));let spans=[[PC,instruction.length+2]];
 if(smc){h.upload(c,0x100,Buffer.from('90ebfe','hex'));h.upload(c,0xfffff100,Buffer.from('90ebfe','hex'));spans=[[0x100,3],...spans,[0xfffff100,3]];}
 h.protect(c,PC,5);if(high!==null)h.protect(c,0xfffff000,smc?7:high);if(low!==null)h.protect(c,0,smc?7:low);
 return h.compile(c,label,spans,imports);}
function skip(c,label){h.skip(c,label,'required family material');h.host(c,'close');}
function key(want){want.writeUInt32LE(0x4000,BASES+20);}
function hook(c,name){c.afterHelper={name,run:()=>{const live=h.arena(c);key(live);h.put(c,0,live);}};}
function oldNumeric(input){input.set(OLD,XMM);return input;}
try{
for(const resident of [false,true])for(const entries of [false,true]){
 // compound SS transactions: operand width32 is independent of B16/B32 pointer policy.
 for(const kind of ['push','pop','enter'])for(const bits of [32,16]){
  const c=open(resident,entries,kind+'32 B'+bits),prefix=bits===16?0xabcd0000:0;
  const original=kind==='pop'?[0x11,0x22,0x33,0x44,prefix+0xe,0x66,0x77,0x88]:[1,2,3,4,prefix+(kind==='enter'?0x12:0x2e),kind==='enter'?prefix+0x12:6,7,8];
  const restored=[1,2,3,4,prefix+0x2e,6,7,8];
  if(kind==='pop')for(const[address,value]of [[0x1a,1],[0x16,2],[0x12,3],[0xe,4],[0xa,prefix+0x2e],[6,6],[2,7],[0xfffffffe,8]])uploadLinear(c,address,words([value]));
  const control=kind==='push'?0:kind==='pop'?2:0x02000004,hex=kind==='push'?'60':kind==='pop'?'61':'c8000002',name=suffix(c,'stack_execute');
  const context={regs:original,ssBase:0xfffffff0,ssLimit:0xff,mode:bits===16?'D32/B16':'D32/B32'};h.put(c,0,canonical(c,context));const m=compile(c,kind+'32 B'+bits,hex,[name]);if(!m){skip(c,kind+'32');continue;}
  const input=canonical(c,context),want=step(input,hex.length/2),wantRAM=h.literalRAM(c),result=kind==='pop'?restored:[...original];
  if(kind==='push'){result[4]=prefix+0xe;for(const[address,value]of [[0x1a,1],[0x16,2],[0x12,3],[0xe,4],[0xa,prefix+0x2e],[6,6],[2,7],[0xfffffffe,8]])writeRAM(wantRAM,address,words([value]));want.writeUInt32LE(result[4],32);}
  if(kind==='pop')want.set(words(result),16);
  if(kind==='enter'){result[4]=prefix+6;result[5]=prefix+0xe;for(const[address,value]of [[0xfffffffe,prefix+0x12],[0xfffffffa,prefix+0x12],[0xfffffff6,prefix+0xe]])writeRAM(wantRAM,address,words([value]));want.writeUInt32LE(result[4],32);want.writeUInt32LE(result[5],36);}
  want.set(words(result),140);packet(want,0);h.observe(c,kind+'32 exact logical registers and wrapped transaction B'+bits,{input,want,wantRAM,helpers:[bound(c,m,name,[control])]});h.host(c,'close');
 }
 for(const mode of ['pop-key','push-SMC']){
  const smc=mode==='push-SMC',bits=smc?32:16,prefix=bits===16?0xabcd0000:0,c=open(resident,entries,mode,{smc});
  const regs=smc?[1,2,3,4,0x2e,6,7,8]:[0x11,0x22,0x33,0x44,prefix+0xe,0x66,0x77,0x88],result=smc?[1,2,3,4,0xe,6,7,8]:[1,2,3,4,prefix+0x2e,6,7,8];
  if(!smc)for(const[address,value]of [[0x1a,1],[0x16,2],[0x12,3],[0xe,4],[0xa,prefix+0x2e],[6,6],[2,7],[0xfffffffe,8]])uploadLinear(c,address,words([value]));
  const context={regs,ssBase:0xfffffff0,ssLimit:0xff,mode:bits===16?'D32/B16':'D32/B32',flags:smc?2:0x10002,version:smc?1:3,shadow:smc?0:1},name=suffix(c,'stack_execute');h.put(c,0,canonical(c,context));
  const m=compile(c,mode,smc?'60':'61',[name],{smc});if(!m){skip(c,mode);continue;}
  const input=canonical(c,context),want=step(input,1,{reason:smc?6:3,retired:smc?1:0}),wantRAM=h.literalRAM(c);want.set(words(result),140);packet(want,0);
  if(smc){want.writeUInt32LE(0xe,32);for(const[address,value]of [[0x1a,1],[0x16,2],[0x12,3],[0xe,4],[0xa,0x2e],[6,6],[2,7],[0xfffffffe,8]])writeRAM(wantRAM,address,words([value]));}
  else{want.writeUInt32LE(2,52);want.writeUInt32LE(0,12);key(want);hook(c,name);}
  h.observe(c,mode+' exact stage/currentness contract',{input,want,wantRAM,helpers:[bound(c,m,name,[smc?0:2],smc?11:0)]});
  if(smc){const after=h.arena(c);h.observe(c,'PUSHA held stale transaction',{input:after,want:after,status:4});}h.host(c,'close');
 }
 // direct SIMD moves use the bulk CPU bus and stage reads before XMM publication.
 for(const variant of ['load','store','load-unmapped','store-readonly','load-key']){
  const load=variant.startsWith('load'),fault=variant==='load-unmapped'||variant==='store-readonly',late=variant==='load-key',options={low:variant==='load-unmapped'?null:variant==='store-readonly'?1:3};
  const c=open(resident,entries,'MOVDQU '+variant,options);if(load){h.upload(c,0xfffffff8,VECTOR.subarray(0,8));if(options.low!==null)h.upload(c,0,VECTOR.subarray(8));}
  const regs=[...registers];regs[0]=8;const context={regs,dsBase:0xfffffff0,dsLimit:0xff,flags:late?0x10002:2,version:late?3:1,shadow:late?1:0};
  const seed=()=>{const input=canonical(c,context);input.set(load?OLD:VECTOR,XMM);return input;},name=load?'cpu_read':suffix(c,'cpu_write');h.put(c,0,seed());
  const m=compile(c,'MOVDQU '+variant,load?'f30f6f00':'f30f7f00',[name],options);if(!m){skip(c,variant);continue;}
  const input=seed(),want=step(input,4,{reason:late?3:1,retired:fault||late?0:1,fault:fault?[variant==='load-unmapped'?1:2,0,load?1:2,16]:undefined}),wantRAM=h.literalRAM(c);
  if(!load)want.set(VECTOR,140);if(load&&!fault){want.set(VECTOR,140);if(!late)want.set(VECTOR,XMM);}packet(want,16,{fault:fault?[variant==='load-unmapped'?1:2,0,load?1:2]:undefined});
  if(!load&&!fault)writeRAM(wantRAM,0xfffffff8,VECTOR);if(late){want.writeUInt32LE(2,52);want.writeUInt32LE(0,12);key(want);hook(c,name);}
  h.observe(c,'MOVDQU '+variant+' exact vector/packet/RAM',{input,want,wantRAM,helpers:[load?{name,args:[0xfffffff8,16,1],status:fault?23:0}:bound(c,m,name,[0xfffffff8,16,1],fault?23:0)]});h.host(c,'close');
 }
 // x87 value1 has exact80-bit representation; pendingMF precedes operand authority and RF consumption.
 for(const variant of ['load','store','load-unmapped','store-readonly','pending']){
  const load=variant.startsWith('load')||variant==='pending',pending=variant==='pending',fault=variant.endsWith('unmapped')||variant.endsWith('readonly'),options={low:pending||variant==='load-unmapped'?null:variant==='store-readonly'?1:3,high:pending?null:3};
  const c=open(resident,entries,'x87 '+variant,options);if(load&&!pending){h.upload(c,0xfffffffb,ONE.subarray(0,5));if(options.low!==null)h.upload(c,0,ONE.subarray(5));}
  const context={dsBase:0xfffffff0,dsLimit:pending?0:0xff,flags:pending?0x10002:2,version:pending?3:1,shadow:pending?1:0};
  const seed=()=>{const input=canonical(c,context);if(!load){input.writeUInt16LE(0xfffc,FP+20);input.set(ONE,FP+40);}if(pending){input.writeUInt16LE(0x37e,FP+16);input.writeUInt16LE(1,FP+18);}return input;};
  const name=resident?'x87_resident_op':'x87_op',hex=load?'db2d0b000000':'db3d0b000000',op=load?0x203:0x10204,opcode=load?0x32d:0x33d;h.put(c,0,seed());const m=compile(c,'x87 '+variant,hex,[name],options);if(!m){skip(c,variant);continue;}
  const input=seed(),want=step(input,6,{fault:fault?[load?1:2,0,load?1:2,10]:undefined,reason:pending?12:1,retired:fault||pending?0:1,detail:pending?0x8081:0}),wantRAM=h.literalRAM(c);
  if(pending){want.writeUInt16LE(0x8081,FP+18);packet(want,10,{exception:[4,0,0x8081,0,0]});}
  else if(fault)packet(want,10,{fault:[load?1:2,0,load?1:2]});
  else{want.writeUInt16LE(load?0x3800:0x800,FP+18);want.writeUInt16LE(load?0x3fff:0xffff,FP+20);want.writeUInt16LE(opcode,FP+22);want.writeUInt32LE(PC,FP+24);want.writeUInt32LE(0xb,FP+28);want.writeUInt16LE(0x1b,FP+32);want.writeUInt16LE(0x23,FP+34);if(load)want.set(ONE,FP+110);else writeRAM(wantRAM,0xfffffffb,ONE);packet(want,10,{value:2});}
  h.observe(c,'x87 '+variant+' exact state/priority',{input,want,wantRAM,helpers:[bound(c,m,name,[op,0xfffffffb,pending?0x10002:2,PC,opcode,0x401],fault?23:0)]});h.host(c,'close');
 }
 // legacy FXSAVE preflights512 and writes464; FXRSTOR reads512 and leaves YMM upper state intact.
 for(const variant of ['save','save-readonly','restore','restore-unmapped','save-writeonly']){
  const save=variant.startsWith('save'),fault=variant==='save-readonly'||variant==='restore-unmapped',options={low:variant==='restore-unmapped'?null:variant==='save-readonly'?1:variant==='save-writeonly'?2:3};
  const c=open(resident,entries,'FX '+variant,options),image=Buffer.alloc(512);image.writeUInt16LE(0x37f);image.writeUInt32LE(save?0x1f80:0x5f80,24);image.writeUInt32LE(0xffff,28);image.fill(0xa5,464);if(!save)image.set(Buffer.from('202122232425262728292a2b2c2d2e2f','hex'),160);
  if(!save){h.upload(c,0xffffff00,image.subarray(0,256));if(options.low!==null)h.upload(c,0,image.subarray(256));}
  const regs=[...registers];regs[0]=0x100;const context={regs,dsBase:0xfffffe00,dsLimit:0x3ff},seed=()=>{const input=canonical(c,context);if(!save)input.set(OLD,XMM);return input;};
  const execute=suffix(c,'simd_execute'),write=suffix(c,'cpu_fxsave'),imports=save?[execute,write]:['cpu_read',execute];h.put(c,0,seed());const m=compile(c,'FX '+variant,save?'0fae00':'0fae08',imports,options);if(!m){skip(c,variant);continue;}
  const input=seed(),want=step(input,3,{fault:fault?[variant==='restore-unmapped'?1:2,0,save?2:1,512]:undefined,retired:fault?0:1}),wantRAM=h.literalRAM(c),helpers=[];
  if(save){const staged=Buffer.from(input.subarray(140,652));image.copy(staged,0,0,464);want.set(staged,140);want.set(words([0,2]),652);packet(want,512,{fault:fault?[2,0,2]:undefined});helpers.push(bound(c,m,execute,[0x13400b,0x1918,0,0,0,2]),bound(c,m,write,[0xffffff00],fault?23:0));if(!fault)writeRAM(wantRAM,0xffffff00,image.subarray(0,464));}
  else{helpers.push({name:'cpu_read',args:[0xffffff00,512,16],status:fault?23:0});if(fault)packet(want,512,{fault:[1,0,1]});else{want.set(image,140);want.set(words([0,2]),652);want.writeUInt32LE(0x5f80,SV+16);want.set(image.subarray(160,288),XMM);packet(want,0);helpers.push(bound(c,m,execute,[0x13400c,0x1819,0,0,0,2]));}}
  h.observe(c,'FX '+variant+' exact512/464 contract',{input,want,wantRAM,helpers});h.host(c,'close');
 }
 // legacy MASKMOV keeps whole logical span authority and selected physical paging policies separate.
 for(const variant of ['selected','readonly','zero-no-data','sparse-high','logical-limit']){
  const zero=variant==='zero-no-data',sparse=variant==='sparse-high',fault=variant==='readonly',logical=variant==='logical-limit',options={low:zero||sparse?null:fault?1:3,high:zero?null:3};
  const c=open(resident,entries,'MASKMOVDQU '+variant,options),mask=zero?0:sparse?0x81:0x8181,regs=[...registers];regs[7]=8;
  const context={regs,dsBase:0xfffffff0,dsLimit:zero?0:logical?0x10:0xff},seed=()=>{const input=canonical(c,context);input.set(VECTOR,XMM);const bits=Buffer.alloc(16);for(let i=0;i<16;i++)if(mask&(1<<i))bits[i]=0x80;input.set(bits,XMM+16);return input;};
  const execute=suffix(c,'simd_execute'),write=suffix(c,'cpu_mask_write');h.put(c,0,seed());const m=compile(c,'MASKMOVDQU '+variant,'660ff7c1',[execute,write],options);if(!m){skip(c,variant);continue;}
  const input=seed(),want=step(input,4,{fault:fault?[2,0,2,16]:undefined,reason:logical?11:1,retired:fault||logical?0:1}),wantRAM=h.literalRAM(c);want.set(VECTOR,140);want.set(words([mask,2]),652);packet(want,logical?0:16,{fault:fault?[2,0,2]:undefined});
  const helpers=[bound(c,m,execute,[0x2a40d,0x908,0,0,0,2])];if(!logical)helpers.push(bound(c,m,write,[0xfffffff8,16,mask],fault?23:0));if(!fault&&!logical)for(let i=0;i<16;i++)if(mask&(1<<i))writeRAM(wantRAM,(0xfffffff8+i)>>>0,VECTOR.subarray(i,i+1));
  h.observe(c,'MASKMOVDQU '+variant+' exact selected policy',{input,want,wantRAM,helpers});h.host(c,'close');
 }
 // the new explicit query bank still uses one exact6-byte bulk store under CPU translation.
 for(const fault of [false,true]){
  const options={low:fault?1:3},c=open(resident,entries,'SGDT6 '+(fault?'latePermission':'wrapped'),options),bank=record('R3SY',36);bank.writeUInt32LE(1,12);bank.writeUInt32LE(0x12345678,16);bank.writeUInt16LE(0x2345,24);h.put(c,140,bank);h.request(c,'explicit GDTR readback authority before material','configure_system_register_bank');
  const regs=[...registers];regs[0]=0xc;const context={regs,dsBase:0xfffffff0,dsLimit:0xff};h.put(c,0,canonical(c,context));const name=suffix(c,'cpu_write'),m=compile(c,'SGDT6 exact wrapped bulk store','0f0100',[name],options);if(!m){skip(c,'SGDT6');continue;}
  const input=canonical(c,context),want=step(input,3,{fault:fault?[2,0,2,6]:undefined,retired:fault?0:1}),wantRAM=h.literalRAM(c),bytes=Buffer.from('452378563412','hex');want.set(bytes,140);packet(want,6,{fault:fault?[2,0,2]:undefined});if(!fault)writeRAM(wantRAM,0xfffffffc,bytes);
  h.observe(c,'SGDT6 exact one-write transaction',{input,want,wantRAM,helpers:[bound(c,m,name,[0xfffffffc,6,1],fault?23:0)]});h.host(c,'close');
 }
}
}catch(error){h.failure('wide wrap fixture setup',error);}
h.finish([
 'Bounded real helper representatives only: compoundSS, directSIMD, x87, legacyFX state, legacyMASKMOV selectedpolicy and callerconfiguredSGDT6. Allfourprofiles, fullCPU/arena/RAM and exact realhelpertrace.',
 'Logicalsegmentauthority, fieldstart/linearwrap and physicalpagefaults remain separate. Faults publish no partialRAM/GPR/ESP/numeric state except explicitly stagedTRANSFER outputs and pendingFP status.',
 'PUSHA/ENTER batchwrites keep one logical transaction and observe earlier staged aliases; POPA+MOVDQU lateCS key changes refuse beforeGPR/ESP/XMM consumption. StatefulSMC retires beforeexit; heldmaterial refusesneutrally.',
 'FXSAVE full512 preflight/write464/48softwaretail preservation is independent of FXRSTOR512 read. YMM unchanged. MASKMOV zero/sparse paging are existingselected policies, not inferred AVX guarantees.',
 'Numeric versiondelta1 is private/native proof; actual observes atomicbytes and retainedsnapshotcurrentness. Separate XSAVE request0/CMPXCHG/customstack fault witnesses cover independently confirmed survivors.',
 'Syntax-only authoring, no execution or complete-data-helper/far/task/CPU closure claim.'
],{contexts:120,modules:120,moduleRuns:124,rows:252});
