import assert from 'node:assert/strict';
import {readEngine,PC,TARGET,POINTER,SEG,STACK,counts,observations,words,packet,arena,put,host,upload,configure,map,snapshotRAM,open,descriptor,cache,decodeSegments,compile,seed,run,exit,publishCode,pins,finish} from '../../../../support/transport.mjs';
assert.ok(process.env.RING3_ENGINE_SHA256, 'select an explicit engine SHA-256');
const [enginePath,output,root]=process.argv.slice(2),engine=readEngine(enginePath);
const sources=[
  "engine/src/cpu/x86/mod.rs",
  "engine/src/cpu/x86/segments.rs",
  "engine/src/process/memory/address_space.rs",
  "engine/src/cpu/x86/segments/transfers.rs",
  "engine/src/cpu/x86/decode/fetch.rs",
  "engine/src/cpu/x86/decode/decoder.rs", "engine/src/cpu/x86/decode/instruction.rs",
  "engine/src/cpu/dbt/wasm/code_fetch.rs",
  "engine/src/cpu/x86/segments/descriptors.rs",
  "engine/src/cpu/x86/segments/descriptor_record.rs",
  "engine/src/cpu/x86/segments/cache.rs",
  "engine/src/cpu/x86/decode/far.rs",
  "engine/src/cpu/x86/decode/profile.rs", "engine/src/cpu/x86/decode/families.rs",
  "engine/src/cpu/x86/decode/lower.rs", "engine/src/cpu/x86/decode/families.rs",
  "engine/src/cpu/x86/ir.rs",
  "engine/src/cpu/dbt/region.rs",
  "engine/src/cpu/dbt/wasm/instructions/far.rs",
  "engine/src/cpu/dbt/wasm/instructions/far/call.rs",
  "engine/src/cpu/dbt/wasm/instructions/far/return.rs",
  "engine/src/cpu/dbt/wasm/emitter/mod.rs", "engine/src/cpu/dbt/wasm/emitter/module.rs", "engine/src/cpu/dbt/wasm/emitter/body.rs", "engine/src/cpu/dbt/wasm/requirements.rs", "engine/src/cpu/dbt/wasm/imports/wiring.rs",
  "engine/src/cpu/dbt/wasm/imports/mod.rs", "engine/src/cpu/dbt/wasm/imports/binding.rs", "engine/src/cpu/dbt/wasm/imports/context.rs", "engine/src/cpu/dbt/wasm/imports/needs.rs", "engine/src/cpu/dbt/wasm/helpers/protocol.rs", "engine/src/cpu/dbt/wasm/instructions/memory/mod.rs", "engine/src/cpu/dbt/wasm/instructions/memory/address.rs", "engine/src/cpu/dbt/wasm/instructions/memory/scalar.rs", "engine/src/cpu/dbt/wasm/instructions/memory/stack.rs",
  "engine/src/cpu/dbt/wasm/instructions/integer/dispatch.rs",
  "engine/src/cpu/dbt/wasm/helpers/result.rs",
  "engine/src/cpu/dbt/wasm/instructions/segments.rs",
  "engine/src/cpu/dbt/wasm/instructions/segments/access.rs",
  "engine/src/cpu/dbt/wasm/abi.rs",
  "engine/src/cpu/dbt/wasm/locals.rs",
  "engine/src/process/cpu/far.rs",
  "engine/src/process/cpu/far/call.rs",
  "engine/src/process/cpu/far/return.rs",
  "engine/src/process/cpu/segments.rs",
  "engine/src/process/memory/span.rs",
  "engine/src/memory/space.rs", "engine/src/memory/space/atomic.rs",
  "engine/src/process/execution/authority.rs",
  "engine/src/process/wasm/cpu.rs",
  "engine/src/process/wasm/mod.rs",
  "engine/src/abi/wasm/exports/cpu.rs",
  "engine/src/abi/wasm/exports/mod.rs",
  "engine/src/abi/x86/segments.rs",
  "engine/src/abi/cpu_helper.rs",
  "engine/tests/support/wasm/arena.mjs",
  "engine/tests/support/wasm/engine.mjs",
  "engine/tests/support/wasm/exports.mjs",
  "engine/tests/cpu/control/native.rs",
  "engine/tests/cpu/control/wasm.rs",
  "engine/tests/cpu/control/far/support.rs",
  "engine/tests/cpu/control/far/support/transport.mjs",
  "engine/src/abi/wasm/exports/calls.rs",
  "engine/src/abi/wasm/exports/execution.rs",
  "engine/src/abi/wasm/exports/image.rs",
  "engine/src/abi/wasm/exports/instance.rs",
  "engine/src/abi/wasm/exports/memory.rs",
  "engine/src/abi/wasm/exports/xstate.rs",
  "engine/tests/cpu/control/far/return/native.rs",
  "engine/tests/cpu/control/far/return/wasm.rs",
  "engine/tests/cpu/control/far/return/fixtures/p2-far-return/runtime/run.mjs"
],sourcePins=pins(root,sources);
const bytes=(word,adjust)=>Buffer.from([...(word?[0x66]:[]),...(adjust?[0xca,adjust&255,adjust>>>8]:[0xcb])]);
function proof(c,label,{raw=descriptor(),selector=0x43,target=0xabcd2345,esp=STACK,flags=0x244cd7,budget=1,cancel=0,context,expect={status:0},readFault,segmentFault=false,reply}={}){
 const slot=c.word?2:4,ip=c.word?target&65535:target>>>0,first=words([target]).subarray(0,slot),second=words([(0xa5a50000|selector)>>>0]).subarray(0,slot);
 if(!readFault||readFault.index!==0)upload(c,esp,first);
 if(!readFault)upload(c,(esp+slot)>>>0,second);
 const input=seed({esp,flags,cancel,context}),want=Buffer.from(input),ram=snapshotRAM(c),helpers=[],args=[];
 const stopped=cancel||budget===0;
 if(stopped)exit(want,cancel?2:1);
 else if(segmentFault)exit(want,15);
 else {
  for(const [index,address,data]of [[0,esp,first],[1,(esp+slot)>>>0,second]]){
   helpers.push('cpu_read');args.push([address,slot,1]);
   if(readFault?.index===index)break;
   want.set(data,140);
  }
  if(readFault){want.set(packet('R3MH',40,[1,0,readFault.detail,readFault.address,1,slot],7),100);exit(want,5,0,readFault.detail,readFault.address,1,slot);}
  else {
   helpers.push(c.helper);args.push([...c.binding,ip,selector,Number(c.word)]);
   if(reply){want.set(reply.packet,100);exit(want,reply.reason??7,0,reply.detail??2);}
   else if(expect.host){want.set(packet('R3MH',40,[0,0,0,0,0,slot],7),100);exit(want,7,0,3);}
   else {
    const status=expect.status,detail=expect.detail??0;
    want.set(packet('R3MH',40,[status,status===0?ip:0,detail,0,0,0],7),100);
    if(status===0){publishCode(want,raw,selector,ip);want.writeUInt32LE((esp+2*slot+c.adjust)>>>0,32);exit(want,flags&0x100?16:budget>1?3:1,1);}
    else exit(want,status===6?14:11,0,detail);
   }
  }
 }
 c.reply=reply??null;run(c,label,input,want,ram,{budget,helpers,argumentsList:args});c.reply=null;
}
for(const resident of [false,true])for(const entries of [false,true])for(const word of [false,true])for(const adjust of [0,0xffff]){
 const c=open(engine,resident,entries);Object.assign(c,{word,adjust});compile(c,bytes(word,adjust),{kind:'far_return',read:true,guard:true},root,output);
 const rows=[
  ['readable-code',descriptor(),{status:0}],['execute-only-code',descriptor({type:8}),{status:0}],['conforming-ring0',descriptor({type:12,dpl:0}),{status:0}],
  ['data-is-GP',descriptor({type:2}),{status:3,detail:0x40}],['privilege-before-P',descriptor({dpl:2,present:false}),{status:3,detail:0x40}],
  ['absent-code-is-NP',descriptor({present:false}),{status:6,detail:0x40}],['target-limit-is-GP',descriptor({limit:0x2344,pages:false}),{status:3,detail:0}],
  ['bounded-valid-target-commits',descriptor({limit:0x2345,pages:false}),{status:0}],['nonflat-d32-commits-logical-target',descriptor({base:0x1000}),{status:0}],
  ['D16-destination-publishes-current-v3',descriptor({d32:false}),{status:0}],['L1-refuses',descriptor({long:true}),{host:true}],['call-gate-is-GP',descriptor({type:12,codeData:false}),{status:3,detail:0x40}],
 ];
 for(const [label,raw,expect]of rows){configure(c,0x43,raw);proof(c,label,{raw,expect,target:0x2345});}
 const raw=descriptor({type:12,dpl:0});configure(c,0x43,raw);
 for(const selector of [0x40,0x41,0x42])proof(c,`conforming-RPL-${selector&3}-still-GP`,{raw,selector,expect:{status:3,detail:0x40}});
 proof(c,'all-selector-high-padding-discarded',{raw});proof(c,'TF-after-return-commit',{raw,flags:0x244dd7});proof(c,'cold-target-after-one-return',{raw,budget:2});
 proof(c,'zero-budget-no-stack-read',{budget:0});proof(c,'cancel-no-stack-read',{cancel:1});
 for(const selector of [0,1,2,3,0x4b])proof(c,`selector-${selector}`,{selector,expect:{status:3,detail:selector&0xfffc}});
 const context=decodeSegments(seed().subarray(SEG));context.caches[2]=cache(descriptor({type:2,limit:0x77ff,pages:false}));
 proof(c,'second-cached-slot-fault-before-first-page-read',{context,esp:0x7800-(word?2:4),segmentFault:true});
 proof(c,'single-logical-slot-overflow-before-RAM',{esp:word?0xffffffff:0xfffffffd,segmentFault:true,readFault:{index:0}});
 const expand=decodeSegments(seed().subarray(SEG));expand.caches[2]=cache(descriptor({type:6,limit:0x77ff,pages:false}));
 proof(c,'expand-down-two-valid-return-slots',{raw,context:expand});
 proof(c,'second-stack-page-fault-retains-first-transfer-only',{esp:0x8000-(word?2:4),readFault:{index:1,detail:1,address:0x8000}});
 host(c,'protect',[0x7000,1,2]);proof(c,'first-stack-read-permission-no-descriptor',{readFault:{index:0,detail:2,address:STACK}});host(c,'protect',[0x7000,1,3]);
 const boundedSource=decodeSegments(seed().subarray(SEG));boundedSource.caches[1]=0xf2b0ffff;
 proof(c,'bounded-current-CS-transfers-after-two-stack-reads',{raw,context:boundedSource});
 // Fixed B32 permits the stack pointer to wrap between two individually valid slots.
 map(c,0xfffff000);map(c,0);proof(c,'slot-boundary-wrap-and-unsigned-adjustment',{raw,esp:word?0xfffffffe:0xfffffffc});
 host(c,'close');
}
// RETF's descriptor phase admits only its own success/GP/NP, never stack exceptions or page faults.
for(const resident of [false,true])for(const word of [false,true]){
 const c=open(engine,resident,false);Object.assign(c,{word,adjust:0});compile(c,bytes(word,0),{kind:'far_return',read:true,guard:true},root,output);configure(c,0x43,descriptor());
 for(const [label,fields,options={}]of [
  ['foreign-PF',[1,0,1,STACK,1,word?2:4]],['foreign-SS',[7,0,0,0,0,0]],['foreign-MF',[4,0,1,0,0,0]],['foreign-XM',[5,0,1,0,0,0]],['foreign-rejected',[2,0,1,0,0,0]],
  ['GP-wrong-selector',[3,0,0x48,0,0,0]],['NP-zero',[6,0,0,0,0,0]],['NP-wrong-selector',[6,0,0x48,0,0,0]],['wrong-target',[0,0x2346,0,0,0,0]],
  ['valid-GP-zero',[3,0,0,0,0,0],{reason:11,detail:0}],['valid-GP-selector',[3,0,0x40,0,0,0],{reason:11,detail:0x40}],['valid-NP-selector',[6,0,0x40,0,0,0],{reason:14,detail:0x40}],
  ['SMC-forbidden',[0,0x2345,0,0,0,0],{transport:11,detail:3}],
 ])proof(c,`protocol-${label}`,{target:0x2345,reply:{packet:packet('R3MH',40,fields,7),transport:0,...options}});
 const bad=packet('R3MH',40,[0,0x2345,0,0,0,0],7);bad.writeUInt32LE(0);
 proof(c,'protocol-header',{target:0x2345,reply:{packet:bad,transport:0}});proof(c,'transport-before-header',{target:0x2345,reply:{packet:bad,transport:7,detail:3}});
 host(c,'close');
}
// RETF consumes its two slots once, then its committed CS limit governs the
// retained target NOP and denies the next captured NOP before retirement.
for(const resident of [false,true])for(const entries of [false,true]){
 const c=open(engine,resident,entries);Object.assign(c,{word:false,adjust:0});
 const raw=descriptor({limit:TARGET,pages:false});configure(c,0x43,raw);
 compile(c,bytes(false,0),{kind:'far_return',read:true,guard:true,targetBytes:Buffer.from([0x90,0x90,0xeb,0xfe])},root,output);
 upload(c,STACK,words([TARGET,0xa5a50043]));
 const context=decodeSegments(seed().subarray(SEG));context.caches[1]=0xf2b0ffff;
 const input=seed({context}),want=Buffer.from(input),ram=snapshotRAM(c);
 publishCode(want,raw,0x43,TARGET+1);want.writeUInt32LE(STACK+8,32);
 want.set(words([0xa5a50043]),140);want.set(packet('R3MH',40,[0,TARGET,0,0,0,0],7),100);exit(want,11,2);
 run(c,'bounded-RETF-refresh-NOP-at-limit-then-GP',input,want,ram,{budget:3,helpers:['cpu_read','cpu_read',c.helper],argumentsList:[[STACK,4,1],[STACK+4,4,1],[...c.binding,TARGET,0x43,0]]});
 host(c,'close');
}

assert.deepEqual(pins(root,sources),sourcePins,'source bytes frozen');
finish(output,engine,sourcePins,['IA32 protected32 CPL3, RETF16/32 with optional unsigned imm16; direct D16/D32 destinations with logical targets/current v3 base/mode publication, B32 SS','RPL>=CPL is checked even for conforming code; high16 of popped 32-bit CS slot discarded','two cached SS spans precede RAM probes as deterministic Ring3 batch policy; argument-release bytes are not probed','no gate/task return, CPL change, CET/FRED or INT/IRET completion claim; D16 execution is established by the separate defaults fixture']);
