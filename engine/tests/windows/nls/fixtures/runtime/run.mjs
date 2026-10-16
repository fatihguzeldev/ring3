import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {mkdirSync,readFileSync,writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {ARENA_SIZE} from '../../../../support/wasm/arena.mjs';
import {assertEngineExports} from '../../../../support/wasm/exports.mjs';
import {image} from '../producer/scenario.mjs';
import {internationalCallCount} from '../producer/international.mjs';
import {checkInternational} from './international.mjs';
const [enginePath,output,root]=process.argv.slice(2);const configured=process.argv[5]==='configured';const international=process.argv[5]==='international';const commandLine='"C:\\game\\nls-authored.exe" --mode=guest';mkdirSync(output,{recursive:true});
const hash=value=>createHash('sha256').update(value).digest('hex');const engineBytes=readFileSync(enginePath);const engineHash=hash(engineBytes);assert.equal(engineHash,process.env.RING3_ENGINE_SHA256);
const engineModule=new WebAssembly.Module(engineBytes);assertEngineExports(engineModule,root);const instance=new WebAssembly.Instance(engineModule,{});const memory=instance.exports.memory;
const api=Object.fromEntries(Object.entries(instance.exports).filter(([name])=>name.startsWith('ring3_abi_v1_')).map(([name,fn])=>[name.slice(13),fn]));const key=[0x55565758,0x95969798];assert.equal(api.open(1024,...key),0);const base=api.arena_ptr()>>>0;assert.equal(api.arena_len(),ARENA_SIZE);
const bytes=()=>new Uint8Array(memory.buffer);const get=offset=>new DataView(memory.buffer).getUint32(base+offset,true);const take=length=>Buffer.from(bytes().slice(base+140,base+140+length));
const request=value=>{assert.ok(value.length<=4096);bytes().set(value,base+140);};
const read=address=>{assert.equal(api.read32(address>>>0),0);assert.equal(get(116),0);return get(120);};
const text=(address,wide)=>{let output='';for(let i=0;i<32768;i++){const packed=read(address+i*(wide?2:1));const c=packed&(wide?0xffff:0xff);if(c===0)return output;output+=String.fromCharCode(c);}assert.fail('bounded NUL string');};
const name=value=>{const data=Buffer.from(value);request(data);return data.length;};
assert.equal(api.configure_cpu_clock(1_000_000_000,0,0,0),0);assert.equal(api.configure_module_limits(8,0x100000,16,65536,0x100000),0);assert.equal(api.register_kernel32_module(0x70000000),0);
if(international)assert.equal(api.configure_nls(0x41f,1254,857,0,32768),0);
if(configured){
 const fields=['C:\\game\\nls-authored.exe',commandLine,'C:\\game'].map(value=>Buffer.from(value));
 const pair=['HOST_SEED','über'].map(value=>Buffer.from(value)),row=Buffer.alloc(8);row.writeUInt32LE(pair[0].length);row.writeUInt32LE(pair[1].length,4);
 const header=Buffer.alloc(56);header.write('R3PL');header.writeUInt32LE(1,4);header.writeUInt32LE(7,12);header.writeBigUInt64LE(116444736000000000n,16);header.writeUInt32LE(65536,24);header.writeUInt32LE(32768,28);fields.forEach((value,i)=>header.writeUInt32LE(value.length,32+i*4));header.writeUInt32LE(1,44);
 const packet=Buffer.concat([header,...fields,row,...pair]);packet.writeUInt32LE(packet.length,8);assert.equal(api.begin_process_launch_input(packet.length),0);for(let at=0;at<packet.length;at+=31){const part=packet.subarray(at,at+31);request(part);assert.equal(api.append_process_launch_input(at,part.length),0);}assert.equal(api.commit_process_launch_input(),0);assert.notEqual(api.commit_process_launch_input(),0,'consumed launch input cannot replay');
}
const guest=image({international});writeFileSync(join(output,'nls-authored.exe'),guest.bytes);assert.equal(guest.calls.length,28+(international?internationalCallCount:0));assert.equal(api.begin_module_input(guest.bytes.length,name('nls-authored.exe')),0);
for(let offset=0;offset<guest.bytes.length;offset+=4096){const chunk=guest.bytes.subarray(offset,offset+4096);request(chunk);assert.equal(api.append_module_input(offset,chunk.length),0);}assert.equal(api.commit_module_input(),0);
assert.equal(api.start_module_process(name('nls-authored.exe'),0,0x7e000000,4,0x7f000000),0);
if(configured){const peb=read(0x7f000030),parameters=read(peb+0x10);assert.equal(read(0x7f000020),7);assert.equal(text(read(parameters+0x48),true),'HOST_SEED=über');assert.equal(text(read(parameters+0x28),true),'C:\\game');}
const rows=[];for(let start=0,total=1;start<total;){assert.equal(api.module_gates_page(start,128),0);const page=take(4096);assert.equal(page.subarray(0,4).toString(),'R3GP');const count=page.readUInt32LE(24);total=page.readUInt32LE(16);assert.ok(count>0);for(let i=0;i<count;i++)rows.push([page.readUInt32LE(36+i*8),page.readUInt32LE(32+i*8)]);start+=count;}
const gates=new Map(rows);assert.equal(gates.size,rows.length);assert.equal(new Set(rows.map(([,pc])=>pc)).size,rows.length);
const calls=[],runs=[];
function compile(call) {
  const pc=get(48),target=gates.get(call.id);
  assert.ok(target,`${call.name} registered gate`);
  const specs=[{entry:pc,length:guest.entry+call.end-pc},{entry:target,length:2}];
  const selected=[{entry:target,id:call.id}];
  if(call.name==='EnumSystemLocalesEx'){
    specs.push({entry:guest.callbacks[0].entry,length:guest.callbacks[0].length},{entry:guest.returnPc,length:2});
    selected.push({entry:guest.returnPc,id:guest.returnId});
  }
  const descriptors=Buffer.alloc((specs.length+selected.length)*8);
  specs.forEach((item,index)=>{descriptors.writeUInt32LE(item.entry,index*8);descriptors.writeUInt32LE(item.length,index*8+4);});
  selected.forEach((item,index)=>{const at=(specs.length+index)*8;descriptors.writeUInt32LE(item.entry,at);descriptors.writeUInt32LE(item.id,at+4);});
  request(descriptors);
  assert.equal(api.compile_with_gates(specs.length,selected.length),0,call.name);
  const compiled=bytes().slice(api.module_ptr()>>>0,(api.module_ptr()>>>0)+api.module_len());
  writeFileSync(join(output,`compiled-${calls.length+1}.wasm`),compiled);
  const module=new WebAssembly.Module(compiled);
  const imports=Object.fromEntries(WebAssembly.Module.imports(module).filter(item=>item.module==='ring3').map(item=>{assert.equal(typeof api[item.name],'function');return[item.name,api[item.name]];}));
  return new WebAssembly.Instance(module,{env:{memory},ring3:imports}).exports;
}
function finishLocaleCallback(generated,generation,call){
  assert.equal(api.pending_windows_callback(),0);
  const request=take(96);
  assert.equal(request.readUInt32LE(12),7);
  assert.equal(request.readUInt32LE(16),guest.callbacks[0].entry);
  assert.equal(request.readUInt32LE(20),3);
  assert.equal(request.readUInt32LE(36),0x21);
  assert.equal(request.readUInt32LE(40),0xabcdef);
  assert.equal(api.begin_windows_callback(...key,generation,guest.returnPc,guest.returnId),0);
  const token=take(64).readUInt32LE(16);
  run(generated,call,guest.returnId);
  assert.equal(api.finish_windows_callback(...key,generation,token),0);
  assert.equal(api.pending_windows_callback(),0);
  assert.equal(take(96).readUInt32LE(12),0,'callback false completes enumeration without replay');
}

function run(generated,call,expectedGate=call.id){let retired=0;for(let attempt=0;attempt<8;attempt++){const result=generated.run(base,base+56,1000,base+96);const reason=get(72),detail=get(80);retired+=get(76);runs.push({name:call.name,result,reason,detail,retired:get(76)});if(reason===3)continue;assert.equal(reason,8,call.name);assert.equal(detail,expectedGate);assert.ok(retired>0);return;}assert.fail(`${call.name}: bounded guest execution`);}
function check(call,result){const a=guest.addresses;if(checkInternational(call,a,read,text))return;switch(call.hook){
case'cpinfo':assert.equal(read(a.buffer),4);assert.equal(read(a.buffer+20),65001);break;
case'utf16':assert.equal(read(a.buffer),0xd83d00e9);assert.equal(read(a.buffer+4)&0xffff,0xde00);break;
case'utf8':assert.equal(read(a.buffer+64),0x9ff0a9c3);assert.equal(read(a.buffer+68)&0xffff,0x8098);break;
case'invalid-utf8':assert.equal(read(a.buffer),0xd83d00e9,'failed strict conversion preserves output');break;
case'bestfit':assert.equal(read(a.buffer)&0xffff,0x3841);assert.equal(read(a.used),0);break;
case'substitute':assert.equal(read(a.buffer)&0xffff,0x3f3f);assert.equal(read(a.used),1);break;
case'uppercase':assert.equal(text(a.buffer,true),'STRASSE');break;
case'classification':assert.equal(read(a.buffer)&0x101,0x101);break;
case'locale-number':assert.equal(read(a.buffer),international?1254:1252);break;
// unconfigured launch identity is derived from the authored root module name.
case'command-line':assert.equal(text(result,false),configured?commandLine:'nls-authored.exe');break;
case'command-line-wide':assert.equal(text(result,true),configured?commandLine:'nls-authored.exe');break;
case'environment':assert.equal(text(a.buffer,true),'€é');break;
case'expanded':assert.equal(text(a.buffer,true),'[€é]');break;
case'system-filetime':assert.equal(BigInt(read(a.filetime))+(BigInt(read(a.filetime+4))<<32n),116444736000000000n);break;
case'system-time':assert.equal(read(a.calendar),0x000107b2);assert.equal(read(a.calendar+4),0x00010004);break;
case'leap-filetime':assert.equal(BigInt(read(a.filetime))+(BigInt(read(a.filetime+4))<<32n),133536855080000000n);break;
case'leap-roundtrip':assert.equal(read(a.calendar),0x000207e8);assert.equal(read(a.calendar+4),0x001d0004);break;
case'date-format':assert.equal(text(a.buffer,true),'2/29/2024');break;
case'time-format':assert.equal(text(a.buffer,true),'1:05:08 PM');break;
}}
for(const call of guest.calls){const generated=compile(call),generation=api.generation();run(generated,call);assert.equal(api.capture_windows_call(...key,generation),0);const token=take(112).readUInt32LE(16);assert.equal(api.complete_windows_call(...key,generation,token),0,call.name);if(call.name==='EnumSystemLocalesEx')finishLocaleCallback(generated,generation,call);const result=get(16);if(call.expected==='pointer')assert.ok(result!==0&&result!==0xffffffff);else if(call.expected==='exit')assert.equal(get(72),9);else assert.equal(result,call.expected,call.name);if(call.expected!=='exit')check(call,result);calls.push({name:call.name,id:call.id,result});writeFileSync(join(output,'progress.json'),JSON.stringify({calls,runs},null,2));}
const receipt={engineHash,configured,international,guestHash:hash(guest.bytes),calls,runs,claim:'authored PE32 imports and generated Wasm prove bounded virtual NLS, Unicode conversion and explicit process command/environment/calendar semantics; no host locale or whole-game compatibility claim'};
writeFileSync(join(output,'receipt.json'),JSON.stringify(receipt,null,2));console.log(JSON.stringify({engineHash,guestCalls:calls.length,guestRuns:runs.length}));
