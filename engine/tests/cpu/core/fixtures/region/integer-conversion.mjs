import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {coldFixture,coldLiteral} from '../../../../support/core_wasm/cold_observed.mjs';

const ROOT=0x1000,CALLEE=0x1100,TRAIN=128;
const FLAGS=0x403,CONTROL=0x27f,TRUNCATE=0xe7f;
const registers=iterations=>[0x13579bdf,iterations,0x2468ace0,0x3000,0x4800,0x89abcdef,0x1800,0x3010];
const unhex=text=>Uint8Array.from(Buffer.from(text.replaceAll(' ',''),'hex'));
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');

// Independently authored conversion procedure. Two live values cross CALL:
// the callee saves CW in a data buffer, constructs toward-zero CW through AX,
// converts -2.5 to signed64, restores CW, and returns. The caller then rounds
// the other value1.5 to signed32 under the original nearest-even CW.
// This is a generic test program, not bytes copied from a guest executable.
const caller='d903 d94304 e8f6000000 db5b18 49 75f0 90 0f0b';
const callee='d97b08 668b4308 660d000c 6689430a d96b0a df3f d96b08 c3';

function program() {
  const code=new Uint8Array(4096);
  code.set(unhex(caller));code.set(unhex(callee),CALLEE-ROOT);return code;
}

function image(initial,{control=CONTROL,status=0,tag=0xffff,opcode=0x5a5,
  fip=0x11223344,fdp=0x55667788,cs=0x33,ds=0x5b,values={}}={}) {
  const fp=initial.slice(),d=new DataView(fp.buffer,fp.byteOffset,fp.byteLength);
  for(const[offset,value]of[[16,control],[18,status],[20,tag],[22,opcode],[32,cs],[34,ds]])
    d.setUint16(offset,value,true);
  d.setUint32(24,fip,true);d.setUint32(28,fdp,true);
  for(const[index,value]of Object.entries(values))fp.set(unhex(value),40+10*Number(index));
  return fp;
}

function finalFp(initial) {
  return image(initial,{status:0x220,opcode:0x35b,fip:0x100a,fdp:0x3018,cs:0x1b,ds:0x23,
    values:{6:'00000000000000a000c0',7:'00000000000000c0ff3f'}});
}

function writes(iterations) {
  return[
    {address:0x47fc,bytes:'0a100000',count:iterations,lastVersion:5*iterations-4},
    {address:0x3008,bytes:'7f02',count:iterations,lastVersion:5*iterations-3},
    {address:0x300a,bytes:'7f0e',count:iterations,lastVersion:5*iterations-2},
    {address:0x3010,bytes:'feffffffffffffff',count:iterations,lastVersion:5*iterations-1},
    {address:0x3018,bytes:'02000000',count:iterations,lastVersion:5*iterations},
  ];
}

function train(world) {
  const initial=image(world.initialFp);
  world.reset({pc:ROOT,registers:registers(TRAIN),flags:FLAGS,fp:initial});
  const before=world.snapshot(),budget=14*TRAIN+1,run=world.execute(budget);
  coldLiteral(world,before,run,{pc:0x1011,retired:budget,budget,
    registers:[0x13570e7f,0,...registers(0).slice(2)],flags:0x446,fp:finalFp(initial),
    writes:writes(TRAIN),versionWrites:5*TRAIN,identity:world.bigint(world.context,120)});
  const unit=world.cache.select(ROOT),record=unit&&world.moduleRecords.get(unit.id);
  assert(unit&&record,'normal completed cold observations install the real conversion procedure');
  return{initial,unit,record,admissions:world.cache.admissions,generated:world.cache.generatedModuleBytes,
    identities:new Set(world.cache.units.keys()),bindings:{run:unit.run,link:unit.link,publish:unit.publish,slot:unit.slot}};
}

function procedure(artifact,report,name,body) {
  coldFixture(artifact,report,name,{
    code:program(),registers:registers(TRAIN),flags:FLAGS,
    data:d=>{d.setUint32(0,0x3fc00000,true);d.setUint32(4,0xc0200000,true);},
  },world=>{
    const restore=observePrecise(world);
    try {const learned=train(world);body(world,learned);}
    finally {restore();}
  });
}

function observePrecise(world) {
  const descriptor=Object.getOwnPropertyDescriptor(world.cache,'invokePrecise');
  const original=world.cache.invokePrecise;
  const entries=[];world.result.preciseEntries=entries;
  Object.defineProperty(world.cache,'invokePrecise',{configurable:true,writable:true,value:function(pointer) {
    const bank=world.word(pointer,92),state=()=>({pc:world.word(pointer,32),retired:world.word(pointer,40),
      remaining:world.word(pointer,44),control:new DataView(world.memory.buffer).getUint16(bank,true),
      status:new DataView(world.memory.buffer).getUint16(bank+2,true)});
    const before=state(),entry={before,
      bytes:Buffer.from(world.bytes(world.physical(before.pc),8)).toString('hex')};
    if(entries.length<32)entries.push(entry);else world.result.droppedPreciseEntries=(world.result.droppedPreciseEntries??0)+1;
    const returned=original.call(this,pointer);entry.returned=returned;entry.after=state();return returned;
  }});
  return()=>{if(descriptor)Object.defineProperty(world.cache,'invokePrecise',descriptor);else delete world.cache.invokePrecise;};
}

function executeToStop(world,budget) {
  const leases=[];
  let remaining=budget,retired=0,work=0,regions=0,last;
  do {
    assert(leases.length<=budget+1,'public NeedCode continuation count is bounded by actual work');
    const pc=world.cache.pc;
    last=world.execute(remaining);leases.push({...last,startPc:pc,id:last.id===null?null:String(last.id)});
    retired+=last.retired;work+=last.work;regions+=last.regions;remaining-=last.work;
    assert(remaining>=0,'public leases consume only the original remaining quota');
    if(last.coreReason===1) {
      assert(remaining>0,'a public NeedCode handoff preserves positive quota');
      assert(last.work>0||last.pc!==pc,'a public handoff makes actual work or PC progress');
    }
  }while(last.coreReason===1);
  (world.result.publicContinuations??=[]).push({budget,leases,retired,work,remaining});
  return{...last,retired,work,regions};
}

function retained(world,learned) {
  assert.equal(world.cache.units.get(learned.unit.id),learned.unit,'the learned immutable instance remains retained');
  assert.equal(world.moduleRecords.get(learned.unit.id),learned.record);
  assert.equal(hash(learned.record.source),learned.record.sha256);
  for(const[name,value]of Object.entries(learned.bindings))assert.equal(learned.unit[name],value,'the original owner keeps its bindings');
  assert.equal(world.cache.table.get(learned.unit.slot),learned.unit.link);
  assert.equal(world.cache.publishers.get(learned.unit.slot),learned.unit.publish);
  assert.equal(world.bigint(world.raw.core_link_owners_ptr()>>>0,learned.unit.slot*8),learned.unit.id);
}

function stable(world,learned) {
  retained(world,learned);
  assert.equal(world.cache.admissions,learned.admissions,'the warm conversion does not compile a replacement');
  assert.equal(world.cache.generatedModuleBytes,learned.generated);
}

function suffixAdmission(world,learned) {
  retained(world,learned);
  const admissions=world.cache.admissions-learned.admissions;
  if(admissions===0) {
    assert.equal(world.cache.generatedModuleBytes,learned.generated);
    return;
  }
  assert.equal(admissions,1,'only one genuinely entered cold suffix may be admitted');
  const added=[...world.cache.units.values()].filter(unit=>!learned.identities.has(unit.id));
  assert.equal(added.length,1);const unit=added[0],record=world.moduleRecords.get(unit.id);
  assert.equal(unit.pc,0x1114,'the additional owner is the actual CW-restoration/RET suffix');
  assert.notEqual(unit.id,learned.unit.id);assert(record);
  assert.equal(world.cache.generatedModuleBytes-learned.generated,record.bytes);
  assert(world.result.publicContinuations.at(-1).leases.some(run=>run.kind==='cold'&&run.startPc===unit.pc),
    'a genuine public cold lease, not replacement compilation, supplies the additional entry');
  assert.equal(world.cache.table.get(unit.slot),unit.link);
  assert.equal(world.bigint(world.raw.core_link_owners_ptr()>>>0,unit.slot*8),unit.id);
  world.result.additionalColdSuffix={id:String(unit.id),pc:unit.pc,bytes:record.bytes,sha256:record.sha256};
}

function mechanism(checks,world,before,precise,{full=false,run}={}) {
  const helpers=world.helperCalls.length-before.helpers,preciseCalls=world.cache.preciseRuns-precise;
  world.result.semanticComplete=true;
  world.result.execution={helperCalls:helpers,preciseCalls};
  checks.push(()=>{
    assert.equal(helpers,0,'the actual integer/CW procedure executes no canonical numeric callbacks');
    if(full) {
      assert.equal(preciseCalls,0,'the complete admitted procedure executes no precise fallback');
      assert.equal(run.regions,1,'CALL and RET carry live FP values inside one actual body');
    }
  });
}

function integer80(value) {
  const signed=BigInt(value),negative=signed<0n,magnitude=negative?-signed:signed,bytes=Buffer.alloc(10);
  if(magnitude!==0n) {
    const exponent=magnitude.toString(2).length-1;
    bytes.writeBigUInt64LE(magnitude<<BigInt(63-exponent));
    bytes.writeUInt16LE(0x3fff+exponent+(negative?0x8000:0),8);
  }
  return bytes;
}

function reference(artifact,report,name,{code,control=CONTROL,status=0,fp:fpChanges={},
  data=()=>{},registers:state=registers(1)}={},body) {
  coldFixture(artifact,report,name,{referencePrepared:true,code,registers:state,flags:FLAGS,data},world=>{
    const initial=image(world.initialFp,{control,status,...fpChanges});
    world.reset({pc:ROOT,registers:state,flags:FLAGS,fp:initial});
    world.result.reference={automaticFormation:false,adapter:'explicit baseline compiler for the independently authored opcode contract'};
    body(world,initial,state);
  });
}

export function runIntegerConversionCases(artifact,report) {
  const checks=[];
  for(const iterations of[1,9])procedure(artifact,report,
    `integer-conversion-steady-call-cw-save-truncate-restore-n${iterations}`,(world,learned)=>{
    // This literal is independently checked after cold training above. It is
    // the recurrent entry's native-empty-ghost contract, not a runtime copy.
    world.reset({pc:ROOT,registers:registers(iterations),flags:FLAGS,fp:finalFp(learned.initial)});
    const before=world.snapshot(),precise=world.cache.preciseRuns,budget=14*iterations,run=world.execute(budget);
    coldLiteral(world,before,run,{pc:0x1010,retired:budget,budget,
      registers:[0x13570e7f,0,...registers(0).slice(2)],flags:0x446,fp:finalFp(learned.initial),
      writes:writes(iterations),versionWrites:5*iterations,identity:learned.unit.id});
    stable(world,learned);
    world.result.conversion={control:CONTROL,truncationControl:TRUNCATE,result64:'-2',result32:2,
      owner:{id:String(learned.unit.id),pc:learned.unit.pc,bytes:learned.record.bytes,sha256:learned.record.sha256},
      iterations,automaticFormation:true,entry:'independent literal recurrent FP image'};
    mechanism(checks,world,before,precise,{full:true,run});
  });

  procedure(artifact,report,'integer-conversion-overwritten-empty-ghost-entry-stays-typed',(world,learned)=>{
    // Initial ghost6 has non-F64 raw80 bits. It remains an architectural empty
    // ghost until the second FLD overwrites it. Entry transport preserves those
    // bits without imposing an unused numeric representation on the prologue.
    world.reset({pc:ROOT,registers:registers(1),flags:FLAGS,fp:learned.initial});
    const before=world.snapshot(),precise=world.cache.preciseRuns,run=world.execute(14);
    coldLiteral(world,before,run,{pc:0x1010,retired:14,budget:14,
      registers:[0x13570e7f,0,...registers(0).slice(2)],flags:0x446,fp:finalFp(learned.initial),
      writes:writes(1),versionWrites:5,identity:learned.unit.id});
    stable(world,learned);mechanism(checks,world,before,precise);
    const entries=world.result.preciseEntries.slice();
    checks.push(()=>{
      assert.equal(world.cache.preciseRuns-precise,0,'an overwritten empty ghost needs no precise entry adapter');
      assert.equal(entries.length,0,'no native precise callback executes before the overwriting loads');
    });
    world.result.entryContract={kind:'arbitrary raw80 empty ghost overwritten before use',expectedPreciseCalls:0};
  });

  procedure(artifact,report,'integer-conversion-quota-after-pop-before-cw-restoration',(world,learned)=>{
    world.reset({pc:ROOT,registers:registers(1),flags:FLAGS,fp:learned.initial});
    const before=world.snapshot(),precise=world.cache.preciseRuns,partial=world.execute(9);
    const partialRegisters=[0x13570e7f,1,...registers(1).slice(2)];partialRegisters[4]=0x47fc;
    const partialFp=image(learned.initial,{control:TRUNCATE,status:0x3820,tag:0x3fff,
      opcode:0x73f,fip:0x1112,fdp:0x3010,cs:0x1b,ds:0x23,
      values:{6:'00000000000000a000c0',7:'00000000000000c0ff3f'}});
    coldLiteral(world,before,partial,{pc:0x1114,retired:9,budget:9,registers:partialRegisters,
      flags:0x402,fp:partialFp,writes:writes(1).slice(0,4),versionWrites:4,
      identity:world.bigint(world.context,120)});
    const resumeBefore=world.snapshot(),resume=executeToStop(world,5);
    coldLiteral(world,resumeBefore,resume,{pc:0x1010,retired:5,budget:5,
      registers:[0x13570e7f,0,...registers(0).slice(2)],flags:0x446,fp:finalFp(learned.initial),
      writes:[{address:0x3018,bytes:'02000000'}],versionWrites:1,identity:world.bigint(world.context,120)});
    suffixAdmission(world,learned);mechanism(checks,world,before,precise);
  });

  procedure(artifact,report,'integer-conversion-store-fault-retains-truncation-cw-and-unpopped-value',(world,learned)=>{
    const state=registers(1);state[7]=0x5000;
    world.reset({pc:ROOT,registers:state,flags:FLAGS,fp:learned.initial});
    const before=world.snapshot(),precise=world.cache.preciseRuns,run=world.execute(14);
    const after=[0x13570e7f,1,...state.slice(2)];after[4]=0x47fc;
    coldLiteral(world,before,run,{pc:0x1112,retired:8,budget:14,work:8,stop:3,registers:after,
      flags:0x402,fault:[0x5000,0x10208,0x80e],
      fp:image(learned.initial,{control:TRUNCATE,status:0x3000,tag:0x0fff,opcode:0x143,
        fip:0x1002,fdp:0x3004,cs:0x1b,ds:0x23,
        values:{6:'00000000000000a000c0',7:'00000000000000c0ff3f'}}),
      writes:writes(1).slice(0,3),versionWrites:3,identity:world.bigint(world.context,120)});
    stable(world,learned);mechanism(checks,world,before,precise);
  });

  procedure(artifact,report,'integer-conversion-executable-store-commits-pop-before-cw-restoration',(world,learned)=>{
    const state=registers(1);state[7]=0x10f0;
    world.reset({pc:ROOT,registers:state,flags:FLAGS,fp:learned.initial});
    const before=world.snapshot(),precise=world.cache.preciseRuns,run=world.execute(14);
    const after=[0x13570e7f,1,...state.slice(2)];after[4]=0x47fc;
    const exactStore=world.result.preciseEntries.find(entry=>entry.before.pc===0x1112&&entry.returned===6);
    if(!exactStore)assert(world.helperCalls.slice(before.helpers).some(call=>call.pc===0x1112&&call.returned===6),
      'the OLD control uses its actual planned canonical store callback');
    coldLiteral(world,before,run,{pc:0x1114,retired:9,budget:14,work:9,stop:6,registers:after,flags:0x402,
      fp:image(learned.initial,{control:TRUNCATE,status:0x3820,tag:0x3fff,opcode:0x73f,
        fip:0x1112,fdp:0x10f0,cs:0x1b,ds:0x23,
        values:{6:'00000000000000a000c0',7:'00000000000000c0ff3f'}}),
      writes:[...writes(1).slice(0,3),{address:0x10f0,bytes:'feffffffffffffff'}],versionWrites:4,
      // precise clears the capability before execution and cannot restore it
      // after SMC. the OLD canonical callback retained stale identity metadata.
      identity:exactStore?0n:learned.unit.id});
    const stopped=world.snapshot();
    assert.equal(world.raw.core_begin(),4,'the executable write revokes the retained source owner');
    assert.deepEqual(world.snapshot(),stopped,'a rejected stale begin executes no restoration or second store');
    world.result.smcAuthority={route:exactStore?'actual precise suffix':'actual OLD canonical callback',
      identity:exactStore?'0':String(learned.unit.id),staleBegin:4};
    mechanism(checks,world,before,precise);
  });

  reference(artifact,report,'integer-control-existing-pending-fault-precedes-unmapped-load',{
    code:'d92f ffe6',control:0x27e,status:1,registers:[...registers(1).slice(0,7),0x5000],
  },(world,initial,state)=>{
    const before=world.snapshot(),precise=world.cache.preciseRuns,run=world.execute(2);
    coldLiteral(world,before,run,{pc:ROOT,retired:0,work:0,budget:2,stop:9,registers:state,flags:FLAGS,
      fp:image(initial,{control:0x27e,status:0x8081}),fault:[0x8081,0,16],identity:run.id});
    mechanism(checks,world,before,precise);
  });

  reference(artifact,report,'integer-control-newly-unmasked-pending-precedes-next-load',{
    code:'d92f d92e',status:0x20,registers:[...registers(1).slice(0,6),0x5000,0x3010],
    data:d=>d.setUint16(0x10,0x25f,true),
  },(world,initial,state)=>{
    const before=world.snapshot(),precise=world.cache.preciseRuns,run=world.execute(2);
    coldLiteral(world,before,run,{pc:0x1002,retired:1,work:1,budget:2,stop:9,registers:state,flags:FLAGS,
      fp:image(initial,{control:0x25f,status:0x80a0}),fault:[0x80a0,0,16],identity:run.id});
    mechanism(checks,world,before,precise);
  });

  reference(artifact,report,'integer-control-nonwaiting-store-preserves-pending-and-metadata',{
    code:'d93f ffe6',control:0x27e,status:0x8281,
  },(world,initial,state)=>{
    const before=world.snapshot(),precise=world.cache.preciseRuns,run=world.execute(2);
    coldLiteral(world,before,run,{pc:0x1800,retired:2,budget:2,registers:state,flags:FLAGS,fp:initial,
      writes:[{address:0x3010,bytes:'7e02'}],versionWrites:1,identity:run.id});
    mechanism(checks,world,before,precise);
  });

  reference(artifact,report,'integer-control-waiting-store-pending-precedes-unmapped-destination',{
    code:'9bd93f ffe6',control:0x27e,status:1,registers:[...registers(1).slice(0,7),0x5000],
  },(world,initial,state)=>{
    const before=world.snapshot(),precise=world.cache.preciseRuns,run=world.execute(2);
    coldLiteral(world,before,run,{pc:ROOT,retired:0,work:0,budget:2,stop:9,registers:state,flags:FLAGS,
      fp:image(initial,{control:0x27e,status:0x8081}),fault:[0x8081,0,16],identity:run.id});
    mechanism(checks,world,before,precise);
  });

  for(const test of[
    {name:'signed16-minimum',code:'df03 df5308 df5b0a ffe6',value:-32768n,width:2,control:0x37f,
      writes:[{address:0x3008,bytes:'0080'},{address:0x300a,bytes:'0080'}],pc:0x1005,opcode:0x75b,fdp:0x300a},
    {name:'signed32-pc24-exact-across-cfg-and-cw',code:`db03 eb1c ${'90 '.repeat(28)}d96b10 db5308 db5b0c ffe6`,
      value:16777217n,width:4,control:0x7f,budget:6,loadControl:true,
      writes:[{address:0x3008,bytes:'01000001'},{address:0x300c,bytes:'01000001'}],pc:0x1026,opcode:0x35b,fdp:0x300c},
    {name:'signed64-beyond-f64',code:'df2b df7b08 ffe6',value:9007199254740993n,width:8,control:0x7f,
      writes:[{address:0x3008,bytes:'0100000000002000'}],pc:0x1002,opcode:0x77b,fdp:0x3008},
    {name:'signed64-minimum',code:'df2b df7b08 ffe6',value:-9223372036854775808n,width:8,control:0x27f,
      writes:[{address:0x3008,bytes:'0000000000000080'}],pc:0x1002,opcode:0x77b,fdp:0x3008},
    {name:'signed64-maximum',code:'df2b df7b08 ffe6',value:9223372036854775807n,width:8,control:0x27f,
      writes:[{address:0x3008,bytes:'ffffffffffffff7f'}],pc:0x1002,opcode:0x77b,fdp:0x3008},
  ])reference(artifact,report,`integer-load-roundtrip-${test.name}`,{
    code:test.code,control:test.control,
    data:d=>{if(test.width===2)d.setInt16(0,Number(test.value),true);
      else if(test.width===4)d.setInt32(0,Number(test.value),true);else d.setBigInt64(0,test.value,true);
      if(test.loadControl)d.setUint16(0x10,test.control,true);},
  },(world,initial,state)=>{
    const before=world.snapshot(),precise=world.cache.preciseRuns,budget=test.budget??test.writes.length+2,run=world.execute(budget);
    coldLiteral(world,before,run,{pc:0x1800,retired:budget,budget,registers:state,flags:FLAGS,
      fp:image(initial,{control:test.control,opcode:test.opcode,fip:test.pc,fdp:test.fdp,cs:0x1b,ds:0x23,
        values:{7:integer80(test.value).toString('hex')}}),writes:test.writes,identity:run.id});
    mechanism(checks,world,before,precise);
  });

  const rounded=[
    {rc:0,positive:2,negative:-2,last:2,c1:false},
    {rc:1,positive:1,negative:-2,last:2,c1:false},
    {rc:2,positive:2,negative:-1,last:3,c1:true},
    {rc:3,positive:1,negative:-1,last:2,c1:false},
  ];
  for(const row of rounded)reference(artifact,report,`integer-store-three-widths-rc${row.rc}`,{
    code:'d903 df5b10 d94304 db5b14 d94308 df7b18 ffe6',control:CONTROL|(row.rc<<10),
    data:d=>{d.setUint32(0,0x3fc00000,true);d.setUint32(4,0xbfc00000,true);d.setUint32(8,0x40200000,true);},
  },(world,initial,state)=>{
    const positive=Buffer.alloc(2),negative=Buffer.alloc(4),last=Buffer.alloc(8);
    positive.writeInt16LE(row.positive);negative.writeInt32LE(row.negative);last.writeBigInt64LE(BigInt(row.last));
    const before=world.snapshot(),precise=world.cache.preciseRuns,run=world.execute(7);
    coldLiteral(world,before,run,{pc:0x1800,retired:7,budget:7,registers:state,flags:FLAGS,
      fp:image(initial,{control:CONTROL|(row.rc<<10),status:0x20+(row.c1?0x200:0),opcode:0x77b,
        fip:0x100e,fdp:0x3018,cs:0x1b,ds:0x23,values:{7:'00000000000000a00040'}}),
      writes:[{address:0x3010,bytes:positive},{address:0x3014,bytes:negative},{address:0x3018,bytes:last}],identity:run.id});
    mechanism(checks,world,before,precise);
  });

  reference(artifact,report,'integer-fisttp-three-widths-ignore-upward-control',{
    code:'d903 df4b10 d94304 db4b14 d94308 dd4b18 ffe6',control:0xa7f,
    data:d=>{d.setUint32(0,0x3fc00000,true);d.setUint32(4,0xbfc00000,true);d.setUint32(8,0x40200000,true);},
  },(world,initial,state)=>{
    const before=world.snapshot(),precise=world.cache.preciseRuns,run=world.execute(7);
    coldLiteral(world,before,run,{pc:0x1800,retired:7,budget:7,registers:state,flags:FLAGS,
      fp:image(initial,{control:0xa7f,status:0x20,opcode:0x54b,fip:0x100e,fdp:0x3018,cs:0x1b,ds:0x23,
        values:{7:'00000000000000a00040'}}),
      writes:[{address:0x3010,bytes:'0100'},{address:0x3014,bytes:'ffffffff'},
        {address:0x3018,bytes:'0200000000000000'}],identity:run.id});
    mechanism(checks,world,before,precise);
  });

  for(const test of[
    {name:'masked-positive-overflow',code:'d903 df5b08 ffe6',control:CONTROL,input:0x47000000,
      status:1,tag:0xffff,opcode:0x75b,stored:'0080',ghost:integer80(32768n).toString('hex')},
    {name:'unmasked-invalid-suppresses-store-and-pop',code:'d903 df5b08 ffe6',control:0x27e,input:0x47000000,
      status:0xb881,tag:0x3fff,opcode:0x75b,stored:null,ghost:integer80(32768n).toString('hex')},
    {name:'unmasked-precision-commits-store-and-pop',code:'d903 db5b08 ffe6',control:0x25f,input:0x3fc00000,
      status:0x82a0,tag:0xffff,opcode:0x35b,stored:'02000000',ghost:'00000000000000c0ff3f'},
  ])reference(artifact,report,`integer-store-${test.name}`,{
    code:test.code,control:test.control,data:d=>{d.setUint32(0,test.input,true);d.setUint32(8,0x55667788,true);},
  },(world,initial,state)=>{
    const before=world.snapshot(),precise=world.cache.preciseRuns,run=world.execute(3);
    coldLiteral(world,before,run,{pc:0x1800,retired:3,budget:3,registers:state,flags:FLAGS,
      fp:image(initial,{control:test.control,status:test.status,tag:test.tag,opcode:test.opcode,
        fip:0x1002,fdp:0x3008,cs:0x1b,ds:0x23,values:{7:test.ghost}}),
      writes:test.stored?[{address:0x3008,bytes:test.stored}]:[],identity:run.id});
    mechanism(checks,world,before,precise);
  });

  reference(artifact,report,'integer-store-raw80-nan-retains-ghost-and-writes-indefinite',{
    code:'db5b08 ffe6',fp:{tag:0xfffe,values:{0:'01000000000000c0ff7f'}},
  },(world,initial,state)=>{
    const before=world.snapshot(),precise=world.cache.preciseRuns,run=world.execute(2);
    coldLiteral(world,before,run,{pc:0x1800,retired:2,budget:2,registers:state,flags:FLAGS,
      fp:image(initial,{status:0x801,tag:0xffff,opcode:0x35b,fip:ROOT,fdp:0x3008,cs:0x1b,ds:0x23}),
      writes:[{address:0x3008,bytes:'00000080'}],identity:run.id});
    mechanism(checks,world,before,precise);
  });
  // Architectural checks finish before the representation-only OLD baseline
  // assertion. Later cases append their checks to this same final phase.
  for(const check of checks)check();
}
