import assert from 'node:assert/strict';
import {readFileSync,writeFileSync,appendFileSync,existsSync} from 'node:fs';
import {join} from 'node:path';
import {createHash} from 'node:crypto';
import {readEngine} from '../../../../support/wasm/engine.mjs';
import {CODE,DATA,STACK,FP,ARENA,CWS,program,validate} from './oracle.mjs';

const [enginePath,output,root,mode]=process.argv.slice(2),engine=readEngine(enginePath),KEY=[0x81342715,0x62594837],TRANSFER=140;
assert.ok(mode===undefined||mode==='reference-only');const referenceOnly=mode==='reference-only';
assert.ok(root&&output);assert.ok(['result.json','checkpoint.json','raw-cases.jsonl'].every(name=>!existsSync(join(output,name))));
const pair=n=>[Number(BigInt(n)&0xffffffffn),Number(BigInt(n)>>32n)],hex=bytes=>Buffer.from(bytes).toString('hex');
const sha=bytes=>createHash('sha256').update(bytes).digest('hex');
const report={schema:1,status:'RUNNING',reference_only:referenceOnly,engine:{sha256:engine.sha256,bytes:engine.bytes.length},cases:[],controls:[],modules:[],source_pins:{},
  scope:'same retained owner/guest operations/native producers; standard consumption versus private native-success consumption under compiled receipts',
  limits:['authored synchronous native-binding contract; no browser scheduling or asynchronous mid-call cancellation claim','no timing or game-performance claim']};
const checkpoint=()=>{const last=report.cases.at(-1);writeFileSync(join(output,'checkpoint.json'),JSON.stringify({schema:report.schema,status:report.status,engine:report.engine,reference_only:referenceOnly,
  case_count:report.cases.length,control_count:report.controls.length,module_count:report.modules.length,error:report.error,
  raw_rows_file:'raw-cases.jsonl',last_case:last?{program:last.program,profile:last.profile,spec:last.spec,status:last.status,reason:last.reason,retired:last.retired,pc:last.pc}:null},null,2));};
const retain=(row,phase)=>appendFileSync(join(output,'raw-cases.jsonl'),JSON.stringify({phase,...row})+'\n');
function arena(c){return Buffer.from(new Uint8Array(c.memory.buffer,c.base,ARENA));}
function take(c,size=4096){return Buffer.from(new Uint8Array(c.memory.buffer,c.base+TRANSFER,size));}
function request(c,bytes){new Uint8Array(c.memory.buffer,c.base+TRANSFER,bytes.length).set(bytes);}
function ok(c,name,...args){assert.equal(c.api[name](...args),0,name);}
function header(bytes,magic,size){assert.equal(bytes.length,size);assert.equal(bytes.subarray(0,4).toString(),magic);assert.equal(bytes.readUInt32LE(4),0x10001);assert.equal(bytes.readUInt32LE(8),size);assert.equal(bytes.readUInt32LE(12),0);return bytes;}
function descriptor(c){const d=take(c,24);assert.deepEqual([d.readUInt32LE(0),d.readUInt32LE(4)],[1,24]);const pointer=d.readUInt32LE(16),size=d.readUInt32LE(20);assert.ok(pointer>0&&size>8&&pointer+size<=c.memory.buffer.byteLength);return{id:d.readBigUInt64LE(8),raw:d,bytes:Buffer.from(new Uint8Array(c.memory.buffer,pointer,size))};}
function instantiate(c,raw,bindings=c.api){
  assert.ok(!(c.memory.buffer instanceof SharedArrayBuffer),'private caller contract requires unshared memory');
  const module=new WebAssembly.Module(raw),imports=WebAssembly.Module.imports(module),ring3={};
  for(const item of imports){
    assert.ok(item.module==='env'&&item.kind==='memory'&&item.name==='memory'||item.module==='ring3'&&item.kind==='function');
    if(item.module==='ring3') {assert.equal(bindings[item.name],c.nativeExports['ring3_abi_v1_'+item.name],'captured original native binding');ring3[item.name]=bindings[item.name];}
  }
  return{run:new WebAssembly.Instance(module,{env:{memory:c.memory},ring3}).exports.run,imports};
}
function fresh(p,profile){
  const instance=new WebAssembly.Instance(engine.module,{ring3_observability:{now_ns:()=>0n}}),nativeExports=instance.exports;
  const api=Object.freeze(Object.fromEntries(Object.entries(nativeExports).filter(([name,fn])=>name.startsWith('ring3_abi_v1_')&&typeof fn==='function').map(([name,fn])=>[name.slice(13),fn])));
  const c={api,nativeExports,memory:nativeExports.memory,profile,p,table:new WebAssembly.Table({element:'anyfunc',initial:16,maximum:16})};
  try {
    ok(c,'open',16,...KEY);c.base=api.arena_ptr()>>>0;assert.equal(api.arena_len()>>>0,ARENA);
    for(const [address,permissions] of [[CODE,7],[DATA,3],[STACK,3]])ok(c,'map',address,1,permissions);
    ok(c,'configure_execution',16,1048576,8);c.initial=arena(c);
    for(const [address,page] of [[CODE,p.codePage],[DATA,p.dataPage],[STACK,p.stackPage]]){request(c,page);ok(c,'upload',address,page.length);}
    request(c,Buffer.from(new Uint32Array(p.specs.flat()).buffer));ok(c,'compile_resident_with_gates',p.specs.length,0);
    c.normal=descriptor(c);c.id=c.normal.id;assert.equal(c.id,1n);
    const ordinary=instantiate(c,c.normal.bytes);c.table.set(0,ordinary.run);
    ok(c,'acknowledge_resident_installation',...KEY,...pair(c.id),0);
    assert.equal(header(take(c,32),'R3IN',32).readBigUInt64LE(16),c.id);
    const normalNames=ordinary.imports.filter(item=>item.module==='ring3').map(item=>item.name);
    assert.ok(normalNames.includes('guard_detect_resident')&&!normalNames.includes('guard_detect_resident_native_success'));
    let trusted;
    if(!referenceOnly){
      assert.equal(typeof api.resident_native_success_module,'function','new cold public export exists');
      const before=arena(c);assert.equal(api.resident_native_success_module((KEY[0]^1)>>>0,KEY[1],...pair(c.id)),3,'cold wrong key');assert.deepEqual(arena(c),before);
      ok(c,'resident_native_success_module',...KEY,...pair(c.id));c.private=descriptor(c);assert.equal(c.private.id,c.id);trusted=instantiate(c,c.private.bytes);
      const privateNames=trusted.imports.filter(item=>item.module==='ring3').map(item=>item.name);
      assert.ok(privateNames.includes('guard_detect_resident_native_success')&&!privateNames.includes('guard_detect_resident'));
      assert.deepEqual(privateNames.map(name=>name==='guard_detect_resident_native_success'?'guard_detect_resident':name),normalNames,'only guard import binding changes');
      assert.equal(api.guard_detect_resident_native_success.length,7);
      if(p.name==='group32')assert.ok(privateNames.includes('x87_resident_group32'),'actual wide-group native binding is exercised');
      const changed={...api,guard_detect_resident_native_success:(...args)=>api.guard_detect_resident_native_success(...args)};
      assert.throws(()=>instantiate(c,c.private.bytes,changed),/captured original native binding/,'opaque helper wrapper rejected before invocation');
    }
    c.run=profile==='native-success'?trusted.run:ordinary.run;c.table.set(0,c.run);
    ok(c,'dispatcher_module',...KEY);const d=header(take(c,32),'R3DP',32),raw=Buffer.from(new Uint8Array(c.memory.buffer,d.readUInt32LE(24),d.readUInt32LE(28)));
    const dispatchModule=new WebAssembly.Module(raw);assert.deepEqual(WebAssembly.Module.imports(dispatchModule).filter(item=>item.module==='ring3').map(item=>item.name),['guard_dispatch_entry','find_installed_resident']);
    c.dispatch=new WebAssembly.Instance(dispatchModule,{env:{memory:c.memory,table:c.table},ring3:{guard_dispatch_entry:api.guard_dispatch_entry,find_installed_resident:api.find_installed_resident}}).exports.run;
    c.record={program:p.name,profile,id:String(c.id),slot:0,specs:p.specs,normal:{sha256:sha(c.normal.bytes),hex:hex(c.normal.bytes),imports:ordinary.imports},
      native_success:trusted?{sha256:sha(c.private.bytes),hex:hex(c.private.bytes),imports:trusted.imports}:null,dispatcher:{sha256:sha(raw),hex:hex(raw)}};
    report.modules.push(c.record);checkpoint();return c;
  }catch(error){if(c.base!==undefined)api.close();throw error;}
}
function reset(c,spec){
  const data=Buffer.from(c.p.dataPage);if(spec.precision){data.writeUInt32LE(0x3f800000,0);data.writeUInt32LE(0x33800000,4);}
  for(const [address,page] of [[DATA,data],[STACK,c.p.stackPage]]){request(c,page);ok(c,'upload',address,page.length);}
  new Uint8Array(c.memory.buffer,c.base,ARENA).set(c.initial);
  const d=new DataView(c.memory.buffer,c.base,ARENA);new Uint8Array(c.memory.buffer,c.base,4).set(Buffer.from('R3ST'));d.setUint16(4,3,true);d.setUint32(12,spec.shadow??0,true);
  [5,spec.iterations??3,0x7000,spec.ebx??DATA,STACK+0x800,0,0,spec.edi??DATA+0x100].forEach((value,index)=>d.setUint32(16+4*index,value,true));
  d.setUint32(48,CODE,true);d.setUint32(52,spec.flags??2,true);d.setUint32(96,spec.cancel?1:0,true);d.setUint16(FP+16,spec.cw??0x037f,true);d.setUint16(FP+18,spec.pending??0,true);
}
function begin(c,budget,mode='compiled',dispatch=1){
  const name={checked:'begin_execution_receipt',owned:'begin_owned_execution_receipt',compiled:'begin_compiled_execution_receipt'}[mode];
  ok(c,name,...KEY,budget,...pair(c.id),dispatch);const frame=header(take(c,32),'R3EF',32);return{raw:frame,token:frame.readBigUInt64LE(16)};
}
function finish(c,token){ok(c,'finish_execution_receipt',...KEY,...pair(token));return header(take(c,64),'R3ER',64);}
function memory(c){
  const saved=arena(c),pages=[];
  try {for(const address of [CODE,DATA,STACK]){const page=Buffer.alloc(4096);for(let offset=0;offset<4096;offset+=4){ok(c,'read32',address+offset);const helper=new DataView(c.memory.buffer,c.base+100,40);assert.equal(helper.getUint32(16,true),0);page.writeUInt32LE(helper.getUint32(20,true),offset);}pages.push({address,hex:hex(page)});}return pages;}
  finally{new Uint8Array(c.memory.buffer,c.base,ARENA).set(saved);assert.deepEqual(arena(c),saved,'diagnostic RAM reads restore every arena byte');}
}
function execute(c,spec){
  reset(c,spec);const frame=begin(c,spec.budget??1000),status=c.dispatch(c.base,c.base+56,spec.budget??1000,c.base+96),beforeFinish=arena(c);
  const row={program:c.p.name,profile:c.profile,spec,status,frame:hex(frame.raw),before_finish:hex(beforeFinish),outer_exit:hex(beforeFinish.subarray(56,96)),work:hex(beforeFinish.subarray(4588,4612)),
    outer_fields:Array.from({length:6},(_,i)=>beforeFinish.readUInt32LE(72+4*i)),pc:beforeFinish.readUInt32LE(48),reason:beforeFinish.readUInt32LE(72),retired:beforeFinish.readUInt32LE(76)};
  report.cases.push(row);retain(row,'before-finish');checkpoint();assert.equal(status,0,'actual native generated dispatcher');
  const receipt=finish(c,frame.token);row.receipt=hex(receipt);row.arena=hex(arena(c));row.memory=memory(c);retain(row,'after-finish');checkpoint();
  const noChild=spec.cancel||spec.budget===0;
  assert.equal(receipt.readUInt32LE(36),row.retired);assert.equal(receipt.readUInt32LE(44),row.reason);assert.equal(receipt.readBigUInt64LE(24),noChild?0n:c.id);
  assert.equal(receipt.readUInt32LE(48),noChild?0:[12,16,23].includes(row.reason)?3:row.reason===3?2:1,'literal no-child/event/NeedCode/active receipt kind');
  assert.deepEqual(receipt.subarray(52),Buffer.alloc(12));validate(c.p,c.initial,spec,row);
  if([12,16,23].includes(row.reason))ok(c,'take_cpu_event');
  return row;
}
function paired(p,specs){
  const contexts=[];
  try {for(const profile of referenceOnly?['standard']:['standard','native-success'])contexts.push(fresh(p,profile));
    const [a,b]=contexts;if(b){assert.deepEqual(a.initial,b.initial);assert.deepEqual(a.record.normal,b.record.normal);assert.deepEqual(a.record.native_success,b.record.native_success);assert.deepEqual(a.record.dispatcher,b.record.dispatcher);}
    for(const spec of specs){const rows=contexts.map(c=>execute(c,spec));if(b)for(const name of ['frame','before_finish','outer_exit','work','receipt','arena','memory'])assert.deepEqual(rows[0][name],rows[1][name],`entire same-topology ${name}`);}
  }finally{for(const c of contexts)c.api.close();}
}
function neutralModes(){
  for(const mode of ['checked','owned']){const c=fresh(program('fp'),'native-success');
    try {reset(c,{budget:0});const frame=begin(c,0,mode,1),before=arena(c);
      assert.equal(c.api.resident_native_success_module(...KEY,...pair(c.id)),12,'cold output remains Busy under an open receipt');assert.deepEqual(arena(c),before);
      const status=c.run(c.base,c.base+56,0,c.base+96);
      const row={name:'private-reject-'+mode,status,frame:hex(frame.raw),token:String(frame.token),before:hex(before),after:hex(arena(c))};report.controls.push(row);retain(row,'rejected-private-entry');checkpoint();assert.equal(status,7,'checked/owned private entry InvalidRequest7');assert.deepEqual(arena(c),before,'private rejected entry preserves arena/epoch/receipt');
      row.empty_dispatch_status=c.dispatch(c.base,c.base+56,0,c.base+96);row.after_empty_dispatch=hex(arena(c));retain(row,'actual-empty-dispatch');assert.equal(row.empty_dispatch_status,0);
      assert.deepEqual(Array.from({length:6},(_,index)=>arena(c).readUInt32LE(72+4*index)),[1,0,0,0,0,0],'actual dispatcher produces canonical no-child Budget');
      const receipt=finish(c,frame.token);row.receipt=hex(receipt);retain(row,'closed-rejected-private-entry');assert.deepEqual([receipt.readBigUInt64LE(16),receipt.readBigUInt64LE(24),...Array.from({length:5},(_,index)=>receipt.readUInt32LE(32+4*index))],[0n,0n,0xffffffff,0,0,1,0],'actual no-child dispatcher receipt');
      const result=execute(c,{iterations:1,cw:0x037f});row.compiled_retry_frame=result.frame;row.compiled_retry_receipt=result.receipt;assert.equal(Buffer.from(result.receipt,'hex').readBigUInt64LE(16),1n,'failed private admission consumed no child epoch');
    }finally{c.api.close();}}
}
try {
  for(const source of ['engine/tests/process/execution/native_success/wasm.rs','engine/tests/process/execution/native_success/fixtures/run.mjs','engine/tests/process/execution/native_success/fixtures/oracle.mjs','engine/tests/support/wasm/engine.mjs'])report.source_pins[source]=sha(readFileSync(join(root,source)));
  for(const name of ['scalar','fp','group2','group8','group9','group32']){
    const p=program(name),limit=p.operations.length;
    const specs=CWS.flatMap(cw=>[...Array.from({length:limit+1},(_,budget)=>({cw,iterations:1,budget})),{cw,iterations:2}]);
    specs.push({cw:0x037f,cancel:true,budget:0},{cw:0x037f,iterations:2,ebx:0x6000},{cw:0x037f,iterations:2,edi:0x6000},
      {cw:0x037f,iterations:2,edi:0x3ffe},{cw:0x037f,flags:0x102,budget:10},{cw:0x037f,flags:0x10102,budget:10},{cw:0x037f,shadow:1,budget:2});
    paired(p,specs);
  }
  paired(program('fp'),[{cw:0x005f,precision:true},{cw:0x037e,pending:1,ebx:0x6000}]);
  for(const name of ['scalar','fp'])paired(program(name),[{cw:0x037f,edi:CODE,iterations:1}]);
  if(!referenceOnly)neutralModes();
  for(const profile of referenceOnly?['standard']:['standard','native-success']){const c=fresh(program('fp'),profile);
    try{reset(c,{});ok(c,'set_segment_base',1,0x1000);const frame=begin(c,10,'compiled',0),before=arena(c),status=c.run(c.base,c.base+56,10,c.base+96),after=arena(c);
      const row={name:'changed-CS',profile,status,frame:hex(frame.raw),token:String(frame.token),before:hex(before),after:hex(after)};report.controls.push(row);retain(row,'changed-CS-return');checkpoint();
      assert.equal(status,0,'changed CS has successful transport with a canonical NeedCode stop');
      const expected=Buffer.from(before);expected.writeUInt16LE(6,60);[3,0,0,0,0,0].forEach((value,index)=>expected.writeUInt32LE(value,72+4*index));
      assert.deepEqual(after,expected,'changed CS changes only the v6 NeedCode Exit; no guest instruction, helper or FP mutation');
      const receipt=finish(c,frame.token);row.receipt=hex(receipt);row.closed_arena=hex(arena(c));row.memory=memory(c);retain(row,'closed-changed-CS');
      assert.deepEqual([receipt.readBigUInt64LE(16),receipt.readBigUInt64LE(24),...Array.from({length:5},(_,index)=>receipt.readUInt32LE(32+4*index))],[1n,c.id,0,0,0,3,2],'changed CS seals exactly one zero-retired NeedCode child');
      assert.deepEqual(receipt.subarray(52),Buffer.alloc(12),'changed CS receipt reserved bytes');
      for(const [address,page] of [[CODE,c.p.codePage],[DATA,c.p.dataPage],[STACK,c.p.stackPage]])assert.equal(row.memory.find(item=>item.address===address).hex,hex(page),'changed CS executes no memory operation');}
    finally{c.api.close();}}
  if(!referenceOnly){const rows=report.controls.filter(row=>row.name==='changed-CS');assert.equal(rows.length,2);for(const field of ['frame','before','after','receipt','closed_arena','memory'])assert.deepEqual(rows[0][field],rows[1][field],`changed CS entire same-topology ${field}`);}
  report.status='PASS';checkpoint();writeFileSync(join(output,'result.json'),JSON.stringify(report,null,2));console.log(JSON.stringify({status:report.status,cases:report.cases.length,controls:report.controls.length,engine_sha256:engine.sha256,output}));
}catch(error){report.status='FAIL';report.error={message:error.message,stack:error.stack};checkpoint();writeFileSync(join(output,'failed-result.json'),JSON.stringify(report,null,2));throw error;}
