import assert from 'node:assert/strict';
import {harness,PC,state,registers,record,exit} from '../../../../fixtures/p2-data-linear-wrap/runtime/support.mjs';
const[enginePath,output,root]=process.argv.slice(2);assert.ok(enginePath&&output&&root);
const h=await harness(enginePath,output,root,{
  runnerURL:new URL(import.meta.url),
  sourcePaths:["engine/tests/cpu/segments/data/wrap/regressions/wasm.rs", "engine/tests/cpu/segments/data/wrap/regressions/fixtures/p2-data-wrap-regressions/runtime/compare-exit.mjs"]
});
try{
for(const resident of [false,true])for(const entries of [false,true])for(const logical of [false,true]){
  const c=h.open(resident,entries,logical?'LOCK CMPXCHG logical control':'LOCK CMPXCHG crossing high fault',[[0x1000],...(!logical?[[0,3]]:[])]);
  const code=Buffer.from('f00fb10d0e000000ebfe','hex');h.upload(c,PC,code);h.protect(c,0x1000,5);
  const regs=[...registers];regs[0]=0xdeadbeef;
  const context={regs,dsBase:0xfffffff0,dsLimit:logical?0x10:0xff};h.put(c,0,state(h.arena(c),context));
  const name=resident?'cpu_compare_exchange_resident32':'cpu_compare_exchange32',material=h.compile(c,'single LOCK CMPXCHG material',[[PC,code.length]],[name]);
  if(!material){h.skip(c,'locked compare-exchange exit capability','new CPU compare import');continue;}
  const input=state(h.arena(c),context),want=Buffer.from(input);
  if(logical)exit(want,11,0,6);
  else{want.set(record('R3MH',40,[1,0,1,0xfffffffe,1,4],10),100);exit(want,5,0,6,[1,0xfffffffe,1,4]);}
  const helpers=logical?[]:[{name,args:[...(resident?material.binding:[]),0xfffffffe,0xdeadbeef,0x23456789],status:0}];
  h.observe(c,logical?'unchanged GP0 priority control':'legitimate high-page fault requires R3EXv6',{input,want,helpers});h.host(c,'close');
}
}catch(error){h.failure('CMPXCHG exit witness setup',error);}
h.finish([
  "pure compare-exchange uses the current CPU reply and exit versions, including failed wrapped reads.",
  "all four bound profiles preserve exact CPU, helper, retirement and mapped RAM state; flat faults are controls."
],{contexts:8,modules:8,moduleRuns:8,rows:16});
