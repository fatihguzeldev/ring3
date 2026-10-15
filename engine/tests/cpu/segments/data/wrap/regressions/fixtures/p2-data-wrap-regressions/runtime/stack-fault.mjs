import assert from 'node:assert/strict';
import {harness,PC,state,record,exit,words} from '../../../../fixtures/p2-data-linear-wrap/runtime/support.mjs';
const[enginePath,output,root]=process.argv.slice(2);assert.ok(enginePath&&output&&root);
const h=await harness(enginePath,output,root,{
  runnerURL:new URL(import.meta.url),
  sourcePaths:["engine/tests/cpu/segments/data/wrap/regressions/wasm.rs", "engine/tests/cpu/segments/data/wrap/regressions/fixtures/p2-data-wrap-regressions/runtime/stack-fault.mjs", "engine/src/cpu/dbt/wasm/instructions/stack.rs", "engine/src/cpu/x86/stack.rs", "engine/src/cpu/x86/stack/pointer.rs", "engine/src/process/cpu/stack.rs"]
});
try{
for(const resident of [false,true])for(const entries of [false,true])for(const push of [true,false])for(const flat of [false,true]){
  const label=(push?'PUSHA':'POPA')+(flat?' nonwrapping fault control':' wrapped-slot fault'),c=h.open(resident,entries,label,[[0x1000],...(!flat?[[0xfffff000,3],...(push?[[0,3]]:[])]:[])]);
  if(!flat&&push)h.protect(c,0xfffff000,1);
  const code=Buffer.from(push?'60ebfe':'61ebfe','hex');h.upload(c,PC,code);h.protect(c,0x1000,5);
  const regs=push?[1,2,3,4,flat?0x102e:0x2e,6,7,8]:[0x11,0x22,0x33,0x44,flat?0x100e:0xe,0x66,0x77,0x88];
  const context={regs,ssBase:flat?0x4000:0xfffffff0,ssLimit:flat?0x1fff:0xff};h.put(c,0,state(h.arena(c),context));
  const name=resident?'stack_execute_resident':'stack_execute',control=push?0:2,material=h.compile(c,label+' actual material',[[PC,code.length]],[name]);
  if(!material){h.skip(c,label+' packet authority','compound-stack material');continue;}
  const input=state(h.arena(c),context),want=Buffer.from(input),detail=flat||!push?1:2,address=flat?(push?0x502a:0x500e):push?0xfffffffe:0,access=push?2:1;
  // generated marshalling is architectural scratch; failed helpers preserve those staged input registers.
  want.set(words(regs),140);want.set(record('R3MH',40,[1,0,detail,address,access,4],7),100);exit(want,5,0,6,[detail,address,access,4]);
  const helpers=[{name,args:[...(resident?material.binding:[]),control],status:0}];
  h.observe(c,label+' remains guest memory fault without publication',{input,want,helpers});h.host(c,'close');
}
}catch(error){h.failure('compound-stack fault witness setup',error);}
h.finish([
  "wrapped PUSHA and POPA replies identify faults within their selected slots modulo32.",
  "failed compound transactions preserve all registers and RAM, including earlier staged pushes.",
  "all four bound profiles retain real helper marshalling and unchanged nonwrapping fault controls."
],{contexts:16,modules:16,moduleRuns:16,rows:32});
