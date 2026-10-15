import assert from 'node:assert/strict';
import {harness,PC,state,registers,record,exit} from '../../../../fixtures/p2-data-linear-wrap/runtime/support.mjs';
const[enginePath,output,root]=process.argv.slice(2);assert.ok(enginePath&&output&&root);
const h=await harness(enginePath,output,root,{
  runnerURL:new URL(import.meta.url),
  sourcePaths:["engine/tests/cpu/segments/data/wrap/regressions/wasm.rs", "engine/tests/cpu/segments/data/wrap/regressions/fixtures/p2-data-wrap-regressions/runtime/xsave-header.mjs", "engine/src/process/cpu/xstate.rs", "engine/src/process/cpu/xstate/upper.rs", "engine/src/cpu/x86/xstate.rs", "engine/src/cpu/x86/xstate/upper.rs", "engine/src/cpu/x86/xstate/upper/access.rs", "engine/src/cpu/dbt/wasm/instructions/xstate.rs", "engine/src/cpu/dbt/wasm/instructions/xstate/upper.rs", "engine/src/abi/x86/x87.rs", "engine/src/abi/x86/simd.rs", "engine/src/abi/x86/ymm.rs"]
});
const HEADER=Buffer.from('4000000000000080','hex');
try{
for(const resident of [false,true])for(const entries of [false,true])for(const kind of ['wrapped','flat-control','wrapped-identity-SMC']){
  const smc=kind==='wrapped-identity-SMC',flat=kind==='flat-control',c=h.open(resident,entries,'XSAVE request0 '+kind,[[0x1000],[0,smc?7:3]]);
  // no high image payload page is mapped: request0 accesses only the explicitly supplied header.
  h.upload(c,flat?0x200:0x1c0,HEADER);h.upload(c,PC,Buffer.from('0fae20ebfe','hex'));
  const spans=smc?[[0x300,3],[PC,5]]:[[PC,5]];if(smc)h.upload(c,0x300,Buffer.from('90ebfe','hex'));h.protect(c,0x1000,5);
  const regs=[...registers];regs[0]=0;regs[2]=0;
  const context={regs,dsBase:flat?0:0xffffffc0,dsLimit:0x3ff};h.put(c,0,state(h.arena(c),context));
  const name=resident?'cpu_xsave_avx_resident':'cpu_xsave_avx',material=h.compile(c,'request0 selected-header material '+kind,spans,[name]);
  if(!material){h.skip(c,'literal XSAVE request0 '+kind,'selected XSAVE material');continue;}
  const input=state(h.arena(c),context),want=Buffer.from(input);want.set(record('R3MH',40,[0,0,0,0,0,832],8),100);
  want.writeUInt32LE(PC+3,48);exit(want,smc?6:1,1,9);
  const helpers=[{name,args:[...(resident?material.binding:[]),3,0,0,0],status:smc?11:0}];
  h.observe(c,'literal XSAVE request0 header '+kind,{input,want,helpers});
  if(smc){const after=h.arena(c);h.observe(c,'identity header write invalidates retained material',{input:after,want:after,status:4});}
  h.host(c,'close');
}
}catch(error){h.failure('XSAVE header witness setup',error);}
h.finish([
  "XSAVE request zero writes its selected header across the linear address boundary.",
  "equal-value writes to executable bytes invalidate held material; state and unrelated mapped RAM remain exact."
],{contexts:12,modules:12,moduleRuns:16,rows:28});
