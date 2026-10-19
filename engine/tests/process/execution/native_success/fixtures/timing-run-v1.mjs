import assert from 'node:assert/strict';
import {readFileSync,writeFileSync,appendFileSync,existsSync} from 'node:fs';
import {join} from 'node:path';
import {createHash} from 'node:crypto';
import {performance} from 'node:perf_hooks';
import {readEngine} from '../../../../support/wasm/engine.mjs';
import {CODE,DATA,STACK,FP,ARENA,program,expected,validate} from './timing-oracle-v1.mjs';

const [enginePath,output,root,mode]=process.argv.slice(2),engine=readEngine(enginePath),KEY=[0x81342715,0x62594837],TRANSFER=140;
assert.equal(mode,undefined,'timing uses both profiles of the same candidate artifact');const referenceOnly=false;
assert.ok(root&&output);assert.ok(['result.json','checkpoint.json','raw-cases.jsonl'].every(name=>!existsSync(join(output,name))));
const pair=n=>[Number(BigInt(n)&0xffffffffn),Number(BigInt(n)>>32n)],hex=bytes=>Buffer.from(bytes).toString('hex');
const sha=bytes=>createHash('sha256').update(bytes).digest('hex');
const report={schema:1,status:'RUNNING',reference_only:referenceOnly,engine:{sha256:engine.sha256,bytes:engine.bytes.length},cases:[],controls:[],modules:[],timings:[],source_pins:{},
  configuration:{iterations:8192,budget:1000000,warmups_per_arm:2,rounds:8,profiles:['standard','native-success'],control_words:[0x007f,0x037f]},
  scope:'same-artifact same retained owner/guest operations/native producers; synchronous original native dispatcher core only',
  limits:['begin+frame-copy and finish+receipt-copy elapsed are separately labeled; reset/oracle/counter/page/report work is outside all clocks','authored equivalent-work timing is not a game CPU share or a game-performance result']};
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
const usageNames=['seedFullNeedCodeSeals','strictFullNeedCodeSeals','lightNeedCodeSeals','fullMaterializations','eligibleChildren','strictChildren','continuedX87StepAttempts','rejectedPublicTransitions'];
function counters(c){
  const scratch=take(c);try{
    ok(c,'compiled_execution_usage',...KEY);const usage=header(take(c,80),'R3EU',80);
    ok(c,'cache_usage');const cache=header(take(c,80),'R3CU',80);
    return{usage:hex(usage),cache:hex(cache),values:Object.fromEntries(usageNames.map((name,index)=>[name,String(usage.readBigUInt64LE(16+index*8))])),
      cache_values:{units:cache.readUInt32LE(16),wasm_bytes:cache.readUInt32LE(20),installed:cache.readUInt32LE(24),
        ...Object.fromEntries(['hits','misses','evictions','promotions','pending_serial'].map((name,index)=>[name,String(cache.readBigUInt64LE(32+index*8))]))}};
  }finally{request(c,scratch);}
}
function executeTimed(c,spec,wanted,label){
  reset(c,spec);const before=counters(c),beginStart=performance.now(),frame=begin(c,spec.budget),beginEnd=performance.now();
  const coreStart=performance.now(),status=c.dispatch(c.base,c.base+56,spec.budget,c.base+96),coreEnd=performance.now();
  const beforeFinish=arena(c),row={program:c.p.name,profile:c.profile,spec,label,status,frame:hex(frame.raw),before_finish:hex(beforeFinish),
    outer_exit:hex(beforeFinish.subarray(56,96)),work:hex(beforeFinish.subarray(4588,4612)),outer_fields:Array.from({length:6},(_,i)=>beforeFinish.readUInt32LE(72+4*i)),
    pc:beforeFinish.readUInt32LE(48),reason:beforeFinish.readUInt32LE(72),retired:beforeFinish.readUInt32LE(76),
    elapsed_ms:{begin_and_frame_copy:beginEnd-beginStart,dispatcher_core:coreEnd-coreStart},counter_before:before};
  const index=report.cases.length;report.cases.push(row);
  if(status!==0){retain(row,'timed-rejected');checkpoint();assert.equal(status,0);}
  const finishStart=performance.now(),receipt=finish(c,frame.token),finishEnd=performance.now();
  row.elapsed_ms.finish_and_receipt_copy=finishEnd-finishStart;row.receipt=hex(receipt);row.arena=hex(arena(c));row.memory=memory(c);row.counter_after=counters(c);
  row.usage_delta=Object.fromEntries(usageNames.map(name=>[name,String(BigInt(row.counter_after.values[name])-BigInt(before.values[name]))]));
  retain(row,'timed-after-finish');checkpoint();assert.ok(Object.values(row.elapsed_ms).every(value=>Number.isFinite(value)&&value>=0));
  assert.equal(receipt.readBigUInt64LE(16)-frame.raw.readBigUInt64LE(24),1n,'exactly one admitted compiled child');
  assert.deepEqual([receipt.readBigUInt64LE(24),...Array.from({length:5},(_,i)=>receipt.readUInt32LE(32+4*i))],[c.id,0,wanted.retired,wanted.retired,3,2]);
  assert.deepEqual(receipt.subarray(52),Buffer.alloc(12));validate(c.p,c.initial,spec,row,wanted);
  assert.equal(BigInt(row.usage_delta.eligibleChildren)+BigInt(row.usage_delta.strictChildren),1n);
  assert.equal(BigInt(row.usage_delta.seedFullNeedCodeSeals)+BigInt(row.usage_delta.strictFullNeedCodeSeals)+BigInt(row.usage_delta.lightNeedCodeSeals),1n);
  for(const field of ['units','wasm_bytes','installed','evictions','promotions','pending_serial'])assert.equal(row.counter_after.cache_values[field],before.cache_values[field],'stable retained owner without cache churn');
  assert.deepEqual(arena(c),Buffer.from(row.arena,'hex'),'all diagnostic getters restore entire arena');
  return{case_index:index,profile:c.profile,label,elapsed_ms:row.elapsed_ms,retired:row.retired};
}
function comparePair(left,right){
  const a=report.cases[left.case_index],b=report.cases[right.case_index];assert.notEqual(a.profile,b.profile);
  for(const field of ['frame','before_finish','outer_exit','work','receipt','arena','memory','usage_delta'])assert.deepEqual(a[field],b[field],`fixed-work full ${field} parity`);
  assert.equal(a.retired,b.retired);return[left.case_index,right.case_index];
}
try{
  for(const source of ['engine/tests/process/execution/native_success/fixtures/timing-run-v1.mjs','engine/tests/process/execution/native_success/fixtures/timing-oracle-v1.mjs',
    'engine/tests/process/execution/native_success/fixtures/run.mjs','engine/tests/process/execution/native_success/fixtures/oracle.mjs','engine/tests/support/wasm/engine.mjs'])report.source_pins[source]=sha(readFileSync(join(root,source)));
  for(const name of ['scalar','fp','group8','group32'])for(const cw of report.configuration.control_words){
    const contexts=[];try{
      const p=program(name),spec={iterations:report.configuration.iterations,budget:report.configuration.budget,
        maximumRetired:{scalar:49153,fp:57345,group8:90113,group32:286721}[name],cw};
      for(const profile of report.configuration.profiles)contexts.push(fresh(p,profile));
      const [a,b]=contexts;assert.deepEqual(a.initial,b.initial);for(const field of ['normal','native_success','dispatcher'])assert.deepEqual(a.record[field],b.record[field]);
      const wanted=expected(p,a.initial,spec);assert.equal(wanted.retired,spec.maximumRetired);assert.equal(wanted.retired,(p.operations.length-1)*spec.iterations+1);assert.equal(wanted.reason,3);
      const timing={name,cw,iterations:spec.iterations,retired_per_run:wanted.retired,warmups:[],rounds:[]};report.timings.push(timing);
      for(let warm=0;warm<2;warm++){const order=warm%2?[1,0]:[0,1],samples=order.map(arm=>executeTimed(contexts[arm],spec,wanted,`warmup-${warm}`));timing.warmups.push({order,samples,pair:comparePair(...samples)});}
      for(let round=0;round<8;round++){
        const order=round%2?[1,0,0,1]:[0,1,1,0],samples=order.map((arm,index)=>executeTimed(contexts[arm],spec,wanted,`round-${round}-sample-${index}`));
        timing.rounds.push({round,order:order.map(arm=>report.configuration.profiles[arm]),samples,pairs:[comparePair(samples[0],samples[1]),comparePair(samples[2],samples[3])]});
      }
    }finally{for(const c of contexts)c.api.close();}
  }
  report.status='PASS';checkpoint();writeFileSync(join(output,'result.json'),JSON.stringify(report,null,2));console.log(JSON.stringify({status:report.status,datasets:report.timings.length,raw_runs:report.cases.length,engine_sha256:engine.sha256,output}));
}catch(error){report.status='FAIL';report.error={message:error.message,stack:error.stack};checkpoint();writeFileSync(join(output,'failed-result.json'),JSON.stringify(report,null,2));throw error;}
