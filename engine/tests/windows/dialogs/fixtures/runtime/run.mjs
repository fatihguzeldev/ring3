import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {mkdirSync,readFileSync,writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {host} from '../../../threads/fixtures/runtime/host.mjs';
import {image} from '../producer/scenario.mjs';

const [enginePath,output,root]=process.argv.slice(2);
mkdirSync(output,{recursive:true});
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
const engineHash=hash(readFileSync(enginePath));
assert.equal(engineHash,process.env.RING3_ENGINE_SHA256);
const guest=image(),a=guest.addresses;
assert.equal(guest.calls.length,4);
writeFileSync(join(output,'dialogs-authored.exe'),guest.bytes);
const engine=host(enginePath,root,output,guest,{gateOwners:4,configure({api}){
  assert.equal(api.configure_nls(0x41f,1254,857,0,32768),0);
  assert.equal(api.configure_user32(8,8,32,8,0x30000000,8),0);
  assert.equal(api.configure_resources(0x32000000,8,64,8,4096),0);
  assert.equal(api.register_user32_module(0x72000000),0);
}});
const {api,key,get,take,read}=engine;
const pair=value=>[Number(value&0xffffffffn),Number(value>>32n)];
const callback=engine.compile('dialog-help',[
  {entry:guest.callbacks[0].entry,length:guest.callbacks[0].length},
  {entry:guest.returnPc,length:2},
],[{entry:guest.returnPc,id:guest.returnId}]);
const calls=[],dialogs=[],callbacks=[];
function pending(){
  assert.equal(api.pending_user32_dialog(0),0);
  const record=take(96);
  assert.equal(record.subarray(0,4).toString(),'R3DG');
  assert.equal(record.readUInt16LE(4),1);assert.equal(record.readUInt16LE(6),1);
  assert.equal(record.readUInt32LE(8),96);assert.equal(record.readUInt32LE(12),1);
  assert.equal(record.readUInt32LE(32),1);
  assert.ok(record.subarray(88).every(byte=>byte===0));
  return {record,epoch:record.readBigUInt64LE(16),id:record.readBigUInt64LE(24)};
}
function text(request,field){
  const total=request.record.readUInt32LE(60+field*4),pages=[];
  for(let offset=0;offset<total;){
    assert.equal(api.read_user32_dialog_text(...pair(request.epoch),...pair(request.id),field,offset,2),0);
    const record=take(100),count=record.readUInt32LE(84);
    assert.equal(record.subarray(0,4).toString(),'R3DG');
    assert.equal(record.readUInt32LE(8),96+count*2);
    assert.equal(record.readBigUInt64LE(16),request.epoch);assert.equal(record.readBigUInt64LE(24),request.id);
    assert.equal(record.readUInt32LE(76),field);assert.equal(record.readUInt32LE(80),offset);
    assert.ok(count>0 && count<=2);pages.push(record.subarray(96,96+count*2));offset+=count;
  }
  return Buffer.concat(pages).toString('utf16le');
}
function wait(){
  assert.equal(api.waiting_user32_message(),0);
  const record=take(64);assert.equal(record.subarray(0,4).toString(),'R3MR');
  const continuation=record.readBigUInt64LE(24);assert.ok(continuation>0n);return continuation;
}
function help(request,continuation){
  assert.equal(api.request_user32_dialog_help(...pair(request.epoch),...pair(request.id)),0);
  assert.equal(api.resume_user32_wait(...key,...pair(continuation)),0);
  assert.equal(api.pending_windows_callback(),0);
  const pending=take(96);assert.equal(pending.readUInt32LE(12),1);assert.equal(pending.readUInt32LE(20),1);
  assert.equal(api.begin_windows_resident_callback(...key,callback.low,callback.high,guest.returnPc,guest.returnId),0);
  const token=take(72).readUInt32LE(16);
  assert.equal(api.authorize_resident_callback(...key,callback.low,callback.high,token),0);
  let retired=0,finished=false;
  for(let attempt=0;attempt<4;attempt++){
    callback.generated.run(api.arena_ptr(),api.arena_ptr()+56,1000,api.arena_ptr()+96);
    const reason=get(72);retired+=get(76);
    if(reason===1)continue;
    if(reason===3){assert.equal(get(48),guest.returnPc);assert.ok(retired>0);continue;}
    assert.equal(reason,8);assert.equal(get(80),guest.returnId);assert.ok(retired>0);finished=true;break;
  }
  assert.ok(finished);
  assert.equal(api.finish_windows_resident_callback(...key,callback.low,callback.high,token),0);
  assert.equal(wait(),continuation,'void HELP callback cannot complete MessageBox');
  assert.equal(read(a.helpCount),1);assert.equal(read(a.helpSize),28);assert.equal(read(a.helpContext),0xabcdef);
  callbacks.push({token,retired});
}
for(const unit of engine.units){
  const owner=engine.run(unit);
  assert.equal(api.capture_resident_call(...key,owner.low,owner.high,2,unit.words),0,unit.name);
  const token=take(112).readUInt32LE(16);
  assert.equal(api.complete_resident_windows_call(...key,owner.low,owner.high,token),0,unit.name);
  if(unit.hook){
    const request=pending(),continuation=wait();
    assert.equal(api.complete_user32_dialog(...pair(request.epoch+1n),...pair(request.id),unit.expected,0),14);
    assert.equal(api.complete_user32_dialog(...pair(request.epoch),...pair(request.id),unit.hook==='resource-dialog'?7:1,0),7,'disallowed button is neutral');
    assert.equal(api.resume_user32_wait(...key,...pair(continuation)),0);
    assert.equal(wait(),continuation,'unready host dialog stays parked');
    if(unit.hook==='resource-dialog'){
      assert.equal(request.record.readUInt32LE(44),0x411);assert.equal(request.record.readUInt32LE(40),0x4001);
      assert.equal(text(request,1),'見出し');assert.equal(text(request,2),'日本語の本文😀');
      help(request,continuation);
    }else{
      assert.equal(text(request,1),'ansi');assert.equal(text(request,2),'ı');
    }
    assert.equal(api.complete_user32_dialog(...pair(request.epoch),...pair(request.id),unit.expected,0),0);
    assert.equal(api.complete_user32_dialog(...pair(request.epoch),...pair(request.id),unit.expected,0),7,'completion cannot replay');
    assert.equal(api.resume_user32_wait(...key,...pair(continuation)),0);
    assert.equal(get(16),unit.expected);
    assert.equal(api.pending_user32_dialog(0),0);assert.equal(take(96).readUInt32LE(12),0);
    dialogs.push({id:request.id.toString(),button:unit.expected});
  }
  if(unit.expected==='exit'){assert.equal(get(72),9);for(const call of guest.calls.filter(call=>call.expected!=='exit'))assert.equal(read(call.status),call.expected,call.name);assert.equal(read(a.mainTid),1);}else assert.equal(get(16),unit.expected,unit.name);
  calls.push({name:unit.name,token});
}
assert.equal(calls.length,4);assert.equal(dialogs.length,2);assert.equal(callbacks.length,1);
const receipt={engineHash,guestHash:hash(guest.bytes),calls,dialogs,callbacks,runs:engine.runs,claim:'authored PE32 proves explicit asynchronous host-dialog ownership, integer resource UTF16 resolution, exact HELP guest callback and configured ACP; visual browser dialog rendering remains SDK responsibility'};
writeFileSync(join(output,'receipt.json'),JSON.stringify(receipt,null,2));
console.log(JSON.stringify({engineHash,guestCalls:4,hostDialogs:2,helpCallbacks:1}));
