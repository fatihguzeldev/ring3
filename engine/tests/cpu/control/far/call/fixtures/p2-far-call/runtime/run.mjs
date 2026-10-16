import {pageFaultExit, pageFaultPacket} from '../../../../../../../support/wasm/page-fault.mjs';
import assert from 'node:assert/strict';
import {join} from 'node:path';
import {readFileSync} from 'node:fs';
import {readEngine,PC,TARGET,POINTER,SEG,STACK,counts,observations,words,packet,arena,put,host,upload,configure,map,readRAM,snapshotRAM,writeExpected,open,descriptor,cache,decodeSegments,encodeSegments,compile,seed,run,exit,publishCode,pins,finish} from '../../../../support/transport.mjs';
assert.ok(process.env.RING3_ENGINE_SHA256, 'select an explicit engine SHA-256');
const [enginePath,output,root]=process.argv.slice(2),engine=readEngine(enginePath);
const sources=[
  'engine/tests/support/wasm/page-fault.mjs',
  "engine/src/cpu/x86/mod.rs",
  "engine/src/cpu/x86/segments.rs",
  "engine/src/process/memory/address_space.rs",
  "engine/src/cpu/x86/segments/transfers.rs",
  "engine/src/cpu/x86/segments/transfers/gates.rs",
  "engine/src/abi/far.rs",
  "engine/src/cpu/dbt/wasm/instructions/far/gate.rs",
  "engine/src/cpu/x86/decode/fetch.rs",
  "engine/src/cpu/x86/decode/decoder.rs",
  "engine/src/cpu/x86/decode/instruction.rs",
  "engine/src/cpu/dbt/wasm/code_fetch.rs",
  "engine/src/cpu/x86/segments/descriptors.rs",
  "engine/src/cpu/x86/segments/descriptor_record.rs",
  "engine/src/cpu/x86/segments/cache.rs",
  "engine/src/cpu/x86/decode/far.rs",
  "engine/src/cpu/x86/decode/profile.rs",
  "engine/src/cpu/x86/decode/families.rs",
  "engine/src/cpu/x86/decode/lower.rs",
  "engine/src/cpu/x86/ir.rs",
  "engine/src/cpu/dbt/region.rs",
  "engine/src/cpu/dbt/wasm/instructions/far.rs",
  "engine/src/cpu/dbt/wasm/instructions/far/call.rs",
  "engine/src/cpu/dbt/wasm/instructions/far/return.rs",
  "engine/src/cpu/dbt/wasm/emitter/mod.rs",
  "engine/src/cpu/dbt/wasm/emitter/module.rs",
  "engine/src/cpu/dbt/wasm/emitter/body.rs",
  "engine/src/cpu/dbt/wasm/requirements.rs",
  "engine/src/cpu/dbt/wasm/imports/wiring.rs",
  "engine/src/cpu/dbt/wasm/imports/mod.rs",
  "engine/src/cpu/dbt/wasm/imports/binding.rs",
  "engine/src/cpu/dbt/wasm/imports/context.rs",
  "engine/src/cpu/dbt/wasm/imports/needs.rs",
  "engine/src/cpu/dbt/wasm/helpers/protocol.rs",
  "engine/src/cpu/dbt/wasm/instructions/memory/mod.rs",
  "engine/src/cpu/dbt/wasm/instructions/memory/address.rs",
  "engine/src/cpu/dbt/wasm/instructions/memory/scalar.rs",
  "engine/src/cpu/dbt/wasm/instructions/memory/stack.rs",
  "engine/src/cpu/dbt/wasm/instructions/integer/dispatch/mod.rs",
  "engine/src/cpu/dbt/wasm/instructions/integer/dispatch/arithmetic.rs",
  "engine/src/cpu/dbt/wasm/instructions/integer/dispatch/moves.rs",
  "engine/src/cpu/dbt/wasm/instructions/integer/dispatch/exchange.rs",
  "engine/src/cpu/dbt/wasm/instructions/integer/dispatch/predicates.rs",
  "engine/src/cpu/dbt/wasm/instructions/integer/dispatch/shifts.rs",
  "engine/src/cpu/dbt/wasm/instructions/integer/dispatch/control.rs",
  "engine/src/cpu/dbt/wasm/instructions/integer/dispatch/extended.rs",
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
  "engine/src/memory/space.rs",
  "engine/src/memory/space/atomic.rs",
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
  "engine/tests/cpu/control/far/call/native.rs",
  "engine/tests/cpu/control/far/call/wasm.rs",
  "engine/tests/cpu/control/far/call/fixtures/p2-far-call/runtime/run.mjs"
];
const sourcePins=pins(root,sources);
const pointer=(word,target,selector)=>{const b=Buffer.alloc(word?4:6);if(word)b.writeUInt16LE(target&65535);else b.writeUInt32LE(target>>>0);b.writeUInt16LE(selector,b.length-2);return b;};
const bytes=(word,source,target=TARGET,selector=0x43)=>Buffer.from([...(word?[0x66]:[]),...(source==='immediate'?(word?[0x9a,target&255,target>>>8&255,selector&255,selector>>>8]:[0x9a,...words([target]),selector&255,selector>>>8]):source==='stack'?[0xff,0x1c,0x24]:[0xff,0x1f])]);
function proof(c,label,{raw=descriptor(),selector=0x43,target=TARGET,esp=STACK,flags=0x244cd7,budget=1,cancel=0,context,logical=POINTER,address=logical,expect={status:0},readFault,sourceFault=false,smc=false,reply}={}){
 const pc=c.pc??PC,slot=c.word?2:4,ip=c.word?target&65535:target>>>0,ptr=pointer(c.word,target,selector);
 if(c.source!=='immediate'&&!readFault&&!sourceFault)upload(c,address,ptr);
 const input=seed({esp,flags,cancel,context,logical});input.writeUInt32LE(pc,48);const want=Buffer.from(input),ram=snapshotRAM(c),helpers=[],args=[];
 const stopped=cancel||budget===0;let success=!stopped&&!readFault&&!sourceFault&&expect.status===0&&!reply;
 if(stopped)exit(want,cancel?2:1);
 else if(sourceFault)exit(want,expect.reason??11);
 else {
  if(c.source!=='immediate'){
   helpers.push('cpu_read');args.push([address,c.word?4:6,1]);
   if(!readFault)want.set(ptr,140);
  }
  if(readFault){const fault={errorCode:readFault.detail===1?4:5,address:readFault.address,access:1,length:ptr.length};want.set(pageFaultPacket(fault),100);want.set(pageFaultExit({retired:0,...fault}),56);}
  else {
   helpers.push(c.helper);args.push([...c.binding,ip,selector,Number(c.word),pc+c.bytes.length,esp]);
   if(reply){want.set(reply.packet,100);exit(want,reply.reason??7,0,reply.detail??2,reply.address??0,reply.access??0,reply.length??0);}
   else if(expect.host){if(c.source!=='immediate')want.set(packet('R3MH',40,[0,0,0,0,0,ptr.length],7),100);exit(want,7,0,3);}
   else {
    const status=expect.status,detail=expect.detail??0,at=expect.address??0,len=expect.gateSlot??(status===1||status===7?slot:0);
    want.set(packet('R3MH',40,[status,status===0?ip:0,detail,at,status===1?2:0,len],expect.gateSlot===undefined?7:9),100);
    if(success){
     publishCode(want,raw,selector,ip);want.writeUInt32LE((esp-2*slot)>>>0,32);
     writeExpected(ram,(esp-slot)>>>0,words([input.readUInt16LE(SEG+18)]).subarray(0,slot));
     writeExpected(ram,(esp-2*slot)>>>0,words([c.word?(pc+c.bytes.length)&65535:pc+c.bytes.length]).subarray(0,slot));
     exit(want,flags&0x100?16:smc?6:budget>1?3:1,1);
    } else if(status===1){const fault={errorCode:detail===1?6:7,address:at,access:2,length:slot};want.set(pageFaultPacket(fault),100);want.set(pageFaultExit({retired:0,...fault}),56);}
    else exit(want,status===7?15:status===6?14:11,0,detail,at,0,0);
   }
  }
 }
 c.reply=reply??null;
 run(c,label,input,want,ram,{budget,helpers,argumentsList:args});c.reply=null;if(smc&&success)c.stale=true;
}
for(const resident of [false,true])for(const entries of [false,true])for(const word of [false,true])for(const source of ['immediate','memory']){
 const c=open(engine,resident,entries);Object.assign(c,{word,source});compile(c,bytes(word,source),{kind:'far_call',read:source!=='immediate',guard:true},root,output);
 const rows=[
  ['readable-code',descriptor(),{status:0}],['execute-only-code',descriptor({type:8}),{status:0}],
  ['conforming-ring0',descriptor({type:12,dpl:0}),{status:0}],
  ['data-is-GP',descriptor({type:2}),{status:3,detail:0x40}],
  ['privilege-before-presence',descriptor({dpl:2,present:false}),{status:3,detail:0x40}],
  ['absent-code-is-NP',descriptor({present:false}),{status:6,detail:0x40}],
  ['target-limit-is-GP',descriptor({limit:0x1fff,pages:false}),{status:3,detail:0}],
  ['bounded-valid-target-commits',descriptor({limit:0x2000,pages:false}),{status:0}],
  ['nonflat-d32-commits-logical-target',descriptor({base:0x1000}),{status:0}],
  ['D16-destination-publishes-current-v3',descriptor({d32:false}),{status:0}],
  ['L1-refuses',descriptor({long:true}),{host:true}],
  ['busy-TSS-is-GP',descriptor({type:11,codeData:false}),{status:3,detail:0x40}],
  ['outer-call-gate-null-target-is-GP0',descriptor({type:12,codeData:false}),{status:3,detail:0,gateSlot:4}],
 ];
 for(const [label,raw,expect]of rows){configure(c,0x43,raw);proof(c,label,{raw,expect});}
 const raw=descriptor();configure(c,0x43,raw);
 proof(c,'debug-trap-after-atomic-call',{raw,flags:0x244dd7});proof(c,'zero-budget-is-neutral',{budget:0});proof(c,'cancel-is-neutral',{cancel:1});proof(c,'cold-target-after-one-call',{budget:2});
 const context=decodeSegments(seed().subarray(SEG));context.caches[2]=cache(descriptor({type:2,limit:0x77ff,pages:false}));
 proof(c,'cached-SS-both-slots-before-RAM',{context,esp:0x7808,expect:{status:7}});
 const bounded=descriptor({limit:0x1fff,pages:false});configure(c,0x43,bounded);
 proof(c,'stack-capacity-before-target-limit',{raw:bounded,context,esp:0x7808,expect:{status:7}});
 const absent=descriptor({present:false});configure(c,0x43,absent);
 proof(c,'descriptor-presence-before-stack-capacity',{raw:absent,context,esp:0x7808,expect:{status:6,detail:0x40}});
 configure(c,0x43,raw);
 proof(c,'second-stack-page-fault-no-first-slot-write',{esp:0x7000+slotWidth(word),expect:{status:1,detail:1,address:(0x7000-slotWidth(word))>>>0}});
 host(c,'protect',[0x7000,1,1]);proof(c,'stack-write-permission-before-any-publication',{expect:{status:1,detail:2,address:STACK-slotWidth(word)}});host(c,'protect',[0x7000,1,3]);
 // Each exact slot, not their combined wrapped span, is checked; no overflow PF is permitted.
 proof(c,'single-slot-overflow-is-SS',{esp:word?1:3,expect:{status:7}});
 if(source==='memory'){
  host(c,'protect',[0x5000,1,2]);proof(c,'pointer-read-permission-before-call',{readFault:{detail:2,address:POINTER}});host(c,'protect',[0x5000,1,3]);
  const nullDS=decodeSegments(seed().subarray(SEG));nullDS.selectors[3]=0;nullDS.caches[3]=0;
  proof(c,'null-source-before-target-and-stack',{context:nullDS,sourceFault:true,expect:{reason:11}});
  proof(c,'source-span-overflow-before-helper',{logical:0xfffffffd,sourceFault:true,expect:{reason:11}});
  for(const selector of [0,1,2,3,0x4b])proof(c,`selector-${selector}`,{selector,expect:{status:3,detail:selector&0xfffc}});
  for(const selector of [0x40,0x41,0x42,0x43])proof(c,`CALL-normalizes-RPL-${selector}`,{raw,selector});
 }
 // Stack writes can invalidate the running capture only after CS/ESP/EIP commit and retirement.
 proof(c,'stack-code-write-retires-before-SMC',{esp:PC+2*slotWidth(word),smc:true});
 // Restore capture after intentional self modification, then prove TF has priority over SMC.
 compile(c,bytes(word,source),{kind:'far_call',read:source!=='immediate',guard:true},root,output);
 proof(c,'TF-priority-after-stack-code-write',{esp:PC+2*slotWidth(word),smc:true,flags:0x244dd7});
 host(c,'close');
}
function slotWidth(word){return word?2:4;}
// One retained two-block region proves the CALL frame is consumed by RETF without
// a host reset of registers, code caches, helper state or the original stack pointer.
for(const resident of [false,true])for(const entries of [false,true])for(const word of [false,true])for(const source of ['immediate','memory']){
 const c=open(engine,resident,entries);Object.assign(c,{word,source});configure(c,0x43,descriptor());
 const returnBytes=Buffer.from([...(word?[0x66]:[]),0xcb]);
 compile(c,bytes(word,source),{kind:'far_call',read:true,guard:true,returnBytes},root,output);
 const ptr=pointer(word,TARGET,0x43);if(source==='memory')upload(c,POINTER,ptr);
 const input=seed(),want=Buffer.from(input),ram=snapshotRAM(c),slot=slotWidth(word),next=PC+c.bytes.length;
 if(source==='memory')want.set(ptr,140);
 want.set(words([0x1b]).subarray(0,slot),140);
 want.writeUInt32LE(next,48);want.set(packet('R3MH',40,[0,next,0,0,0,0],7),100);exit(want,1,2);
 writeExpected(ram,STACK-slot,words([0x1b]).subarray(0,slot));writeExpected(ram,STACK-2*slot,words([next]).subarray(0,slot));
 const helperReturn=resident?'far_return_resident':'far_return';
 const helpers=[...(source==='memory'?['cpu_read']:[]),c.helper,'cpu_read','cpu_read',helperReturn];
 const argumentsList=[...(source==='memory'?[[POINTER,word?4:6,1]]:[]),[...c.binding,TARGET,0x43,Number(word),next,STACK],[STACK-2*slot,slot,1],[STACK-slot,slot,1],[...c.binding,next,0x1b,Number(word)]];
 run(c,'retained-region-CALL-RETF-roundtrip',input,want,ram,{budget:2,helpers,argumentsList});host(c,'close');
}

// Source registers and pointer bytes are captured before ESP or overlapping stack RAM changes.
for(const resident of [false,true])for(const word of [false,true]){
 const c=open(engine,resident,false);Object.assign(c,{word,source:'memory'});const raw=descriptor();configure(c,0x43,raw);
 const prefix=word?[0x66]:[];
 compile(c,Buffer.from([...prefix,0xff,0x1c,0x24]),{kind:'far_call',read:true,guard:true},root,output);
 proof(c,'ESP-source-address-uses-original-stack-pointer',{raw,address:STACK});
 compile(c,Buffer.from([...prefix,0xff,0x5d,0]),{kind:'far_call',read:true,guard:true},root,output);
 proof(c,'pointer-overlaps-CS-slot-before-atomic-stack-write',{raw,logical:STACK-slotWidth(word),address:STACK-slotWidth(word)});
 compile(c,Buffer.from([...prefix,0x64,0xff,0x1f]),{kind:'far_call',read:true,guard:true},root,output);
 const context=decodeSegments(seed().subarray(SEG));context.fs=0x1000;
 proof(c,'FS-base-resolves-pointer-before-helper',{raw,context,logical:POINTER-0x1000,address:POINTER});
 compile(c,Buffer.from([...prefix,0x67,0xff,0x1a]),{kind:'far_call',read:true,guard:true},root,output);
 proof(c,'address16-BP-SI-wrap-selects-SS',{raw,logical:(POINTER-0x9abc)&65535,address:POINTER});
 host(c,'close');
}

for(const resident of [false,true])for(const word of [false,true]){
 const c=open(engine,resident,false);Object.assign(c,{word,source:'immediate',pc:0x12341000});map(c,c.pc,7);configure(c,0x43,descriptor());
 compile(c,bytes(word,'immediate'),{kind:'far_call',guard:true},root,output);
 proof(c,'high-EIP-return-slot-obeys-operand-width');host(c,'close');
}

// Synthetically malformed CALL results cannot publish registers/cache/stack. Valid phase faults remain precise.
for(const resident of [false,true])for(const word of [false,true]){
 const c=open(engine,resident,false);Object.assign(c,{word,source:'immediate'});compile(c,bytes(word,'immediate'),{kind:'far_call',guard:true},root,output);configure(c,0x43,descriptor());
 const slot=slotWidth(word),rows=[
  ['foreign-MF',[4,0,1,0,0,0]],['foreign-XM',[5,0,1,0,0,0]],['unknown-status',[8,0,0,0,0,0]],
  ['GP-wrong-selector',[3,0,0x48,0,0,0]],['NP-zero',[6,0,0,0,0,0]],['NP-wrong-selector',[6,0,0x48,0,0,0]],
  ['SS-selector-must-zero',[7,0,0x23,0,0,slot]],['SS-length-must-slot',[7,0,0,0,0,0]],
  ['PF-read-not-write',[1,0,1,STACK-slot,1,slot]],['PF-outside-slots',[1,0,1,STACK,2,slot]],['PF-overflow-disallowed',[1,0,3,STACK-slot,2,slot]],
  ['PF-wrong-width',[1,0,1,STACK-slot,2,slot+1]],['success-wrong-target',[0,TARGET+1,0,0,0,0]],['success-wrong-length',[0,TARGET,0,0,0,slot]],
  ['valid-GP-selector',[3,0,0x40,0,0,0],{reason:11,detail:0x40}],['valid-NP-selector',[6,0,0x40,0,0,0],{reason:14,detail:0x40}],
  ['valid-SS',[7,0,0,0,0,slot],{reason:15,detail:0}],['valid-stack-PF',[1,0,2,STACK-2*slot,2,slot],{reason:5,detail:2,address:STACK-2*slot,access:2,length:slot}],
  ['valid-version-capacity',[2,0,1,0,0,0],{reason:7,detail:1}],
  ['SMC-with-fault',[7,0,0,0,0,slot],{transport:11}],
 ];
 for(const [label,fields,options={}]of rows)proof(c,`protocol-${label}`,{reply:{packet:packet('R3MH',40,fields,7),transport:0,...options}});
 const bad=packet('R3MH',40,[0,TARGET,0,0,0,0],7);bad.writeUInt32LE(0);
 proof(c,'protocol-header',{reply:{packet:bad,transport:0}});proof(c,'transport-error-before-header',{reply:{packet:bad,transport:7,detail:3}});
 host(c,'close');
}
// Capture both NOPs under the original CS, then commit a narrower loaded CS.
// The first target NOP is legal; the second must fault in this same invocation.
for(const resident of [false,true])for(const entries of [false,true]){
 const c=open(engine,resident,entries);Object.assign(c,{word:false,source:'immediate'});
 const raw=descriptor({limit:TARGET,pages:false});configure(c,0x43,raw);
 compile(c,bytes(false,'immediate'),{kind:'far_call',guard:true,targetBytes:Buffer.from([0x90,0x90,0xeb,0xfe])},root,output);
 const context=decodeSegments(seed().subarray(SEG));context.caches[1]=0xf2b0ffff;
 const input=seed({context}),want=Buffer.from(input),ram=snapshotRAM(c),next=PC+c.bytes.length;
 publishCode(want,raw,0x43,TARGET+1);want.writeUInt32LE(STACK-8,32);
 want.set(packet('R3MH',40,[0,TARGET,0,0,0,0],7),100);exit(want,11,2);
 writeExpected(ram,STACK-4,words([0x1b]));writeExpected(ram,STACK-8,words([next]));
 run(c,'bounded-CALL-refresh-NOP-at-limit-then-GP',input,want,ram,{budget:3,helpers:[c.helper],argumentsList:[[...c.binding,TARGET,0x43,0,next,STACK]]});
 host(c,'close');
}

assert.deepEqual(pins(root,sources),sourcePins,'source bytes frozen');
finish(output,engine,sourcePins,['IA32 protected32 CPL3, same-CPL direct CALL four forms, logical D16/D32 CS targets with current v3 base/mode publication, and B32 SS','batch cached SS spans precede page probes as deterministic Ring3 policy; no universal multioperand fault priority claimed','zero high16 padding in CALL32 CS slot is a deterministic padding choice','native tests separately read raw descriptor A bit and exercise version exhaustion; actual proof validates whole arena/RAM, helper width/order, TF/SMC and malformed phase replies','the direct-CALL corpus also checks an outer-admitted gate32 with embedded-null target and exact v9 slot4 fault packet; the complete gate success/width matrix is owned by the separate call-gates fixture; task execution, privilege switches and INT/IRET remain outside this slice; D16 execution is established by the separate defaults fixture']);
