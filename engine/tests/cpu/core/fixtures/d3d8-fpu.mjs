import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync,writeFileSync,openSync,closeSync} from 'node:fs';
import {readEngine} from '../../../support/wasm/engine.mjs';
import {Engine,KEY,record,view} from '../../../../../sdk/runtime/engine.js';
import {CodeCache} from '../../../../../sdk/runtime/cache.js';
import {image} from '../../../windows/graphics/d3d8/fixtures/producer/image.mjs';

const [enginePath,expectedSha256,reportPath]=process.argv.slice(2);
assert(enginePath&&expectedSha256&&reportPath,'usage: d3d8-fpu.mjs engine.wasm expected-sha256 fresh-report.json');
closeSync(openSync(reportPath,'wx'));
const hash=bytes=>createHash('sha256').update(bytes).digest('hex'),hex=bytes=>Buffer.from(bytes).toString('hex');
const artifact=readEngine(enginePath,expectedSha256),fpOffset=4236,dataPc=0x600000,codePc=0x601000;
const sources=['./d3d8-fpu.mjs','../../../support/wasm/engine.mjs','../../../../../sdk/runtime/engine.js',
  '../../../../../sdk/runtime/cache.js','../../../windows/graphics/d3d8/fixtures/producer/image.mjs'];
const report={schema:1,complete:false,engineSha256:artifact.sha256,engineBytes:artifact.bytes.length,
  sources:Object.fromEntries(sources.map(path=>[path,hash(readFileSync(new URL(path,import.meta.url)))])),
  scope:'authored PE CreateDevice, genuine Core Windows callbacks and imported-CW numerical execution',
  limits:['no GPU execution, game, performance, failed-creation or callback-abort claim'],cases:[]};
const checkpoint=()=>writeFileSync(reportPath,JSON.stringify(report,null,2));
let stage='initialization';checkpoint();

async function runCase(name,preserve,fullscreen) {
  stage=name;const result={name,preserve,fullscreen,calls:[],callbacks:[],runs:[],complete:false};report.cases.push(result);
  const instance=await WebAssembly.instantiate(artifact.module,{ring3_observability:{now_ns:()=>0n}});
  const native=Object.freeze(Object.fromEntries(Object.entries(instance.exports).filter(([name])=>name.startsWith('ring3_abi_v1_'))
    .map(([name,value])=>[name.slice(13),value])));
  const engine=new Engine(native,instance.exports.memory,artifact.sha256);
  let opened=false,cache,backing=0;
  const bytes=(pointer,length)=>Buffer.from(new Uint8Array(engine.memory.buffer).slice(pointer,pointer+length));
  const word=(pointer,offset=0)=>new DataView(engine.memory.buffer).getUint32(pointer+offset,true);
  const put=(pointer,offset,value)=>new DataView(engine.memory.buffer).setUint32(pointer+offset,value>>>0,true);
  const fp=()=>bytes(engine.base+fpOffset,128);
  const otherBanks=()=>Buffer.concat([bytes(engine.base+4364,224),bytes(engine.base+4612,184)]);
  const raw=()=>({stateHex:hex(bytes(engine.base,56)),fpHex:hex(fp()),otherBanksHex:hex(otherBanks()),
    contextHex:cache?.context?hex(bytes(cache.context,128)):null,
    ramSha256:backing?hash(bytes(backing,1024*4096)):null});
  const physical=address=>{
    const slot=word(word(cache.context,56)+(address>>>12)*4);assert(slot>0&&slot<=1024);
    return backing+(slot-1)*4096+(address&4095);
  };
  const ram=(address,length)=>{assert((address&4095)+length<=4096);return bytes(physical(address),length);};
  const stack=()=>Buffer.concat(Array.from({length:4},(_,i)=>ram(0x7e000000+i*4096,4096)));
  let wantedFp,wantedOther;
  const checkBanks=label=>{assert.deepEqual(fp(),wantedFp,`${name}: ${label} full FP128`);
    assert.deepEqual(otherBanks(),wantedOther,`${name}: ${label} SIMD/YMM/segments/bases`);};
  function untilGate(id) {
    for(let index=0;index<32;index++) {
      const run=cache.execute(cache.select());result.runs.push({...run,id:run.id===null?null:String(run.id)});
      if(run.coreReason===1)continue;
      assert.equal(run.coreReason,7);assert.equal(run.detail,id);checkpoint();return run;
    }
    assert.fail(`${name}: bounded authored gate execution`);
  }
  function callbacks(outerToken,returnPc,checking) {
    let count=0;
    for(;count<16;count++) {
      engine.ok('pending_windows_callback');const request=Buffer.from(engine.take(96));
      assert.equal(request.subarray(0,4).toString(),'R3WC');if(!request.readUInt32LE(12))break;
      engine.ok('core_begin_windows_callback',returnPc,0xffffe);const frame=Buffer.from(engine.take(64));
      assert.equal(frame.subarray(0,4).toString(),'R3CB');assert.equal(frame.readUInt32LE(4),0x10002);
      assert.equal(frame.readUInt32LE(8),64);assert.equal(frame.readUInt32LE(20),outerToken);assert.equal(frame.readUInt32LE(56),0);
      const token=frame.readUInt32LE(16),entryEsp=frame.readUInt32LE(36),words=frame.readUInt32LE(48);
      if(checking)checkBanks('callback entry');cache.import();untilGate(0xffffe);
      assert.equal(engine.get(48),returnPc);assert.equal(engine.get(32),entryEsp+4+words*4);
      if(checking)checkBanks('callback return');
      engine.ok('core_finish_windows_callback',token);
      if(checking)checkBanks('restored outer call');
      result.callbacks.push({stage,requestHex:hex(request),frameHex:hex(frame),message:request.readUInt32LE(36)});checkpoint();
    }
    assert(count<16,'bounded callback drain');return count;
  }
  try {
    engine.ok('open',1024,...KEY);opened=true;engine.base=native.arena_ptr()>>>0;
    engine.ok('configure_module_limits',16,0x100000,16,65536,0x100000);
    engine.ok('register_kernel32_module',0x70000000);engine.ok('configure_user32',16,16,64,16,0x30000000,16);
    engine.ok('register_user32_module',0x72000000);
    const config=record('R3C8',144,8),configView=view(config);
    for(const [index,value] of [[4,1],[5,0x10000000],[6,256],[7,0x20000000],[8,128],[9,128],[10,4],[11,16],
      [12,1024],[13,256*1024*1024],[14,256],[15,64],[16,0x100000],[17,32],[18,16],[19,0x100000],
      [20,64],[21,64],[22,22],[23,60],[24,4096],[25,256],[26,16],[27,1],[28,0x8000000f],
      [32,64],[33,64],[34,60],[35,22]])configView.setUint32(index*4,value>>>0,true);
    engine.request(config);engine.ok('configure_d3d8',144);engine.ok('register_d3d8_module',0x71000000);
    const guest=image(),behaviorAt=guest.calls[3].end-27;
    assert.equal(guest.calls[3].name,'CreateDevice');assert.equal(guest.code[behaviorAt-1],0x68);
    assert.equal(guest.code.readUInt32LE(behaviorAt),0x40);
    guest.bytes.writeUInt32LE(preserve?0x42:0x40,0x200+behaviorAt);
    if(fullscreen)for(const [offset,value] of [[0,64],[4,64],[8,22],[28,0]])
      guest.bytes.writeUInt32LE(value,0x2200+guest.addresses.pp-0x404000+offset);
    result.authoredImageSha256=hash(guest.bytes);result.behavior=preserve?0x42:0x40;
    const file='d3d8-fpu.exe';engine.ok('begin_module_input',guest.bytes.length,engine.name(file));
    for(let offset=0;offset<guest.bytes.length;offset+=4096){const chunk=guest.bytes.subarray(offset,offset+4096);
      engine.request(chunk);engine.ok('append_module_input',offset,chunk.length);}
    engine.ok('commit_module_input');engine.ok('start_module_process',engine.name(file),0,0x7e000000,4,0x7f000000);
    const gates=new Map();for(let start=0,total=1;start<total;){
      engine.ok('module_gates_page',start,128);const page=Buffer.from(engine.take(4096));
      assert.equal(page.subarray(0,4).toString(),'R3GP');total=page.readUInt32LE(16);const count=page.readUInt32LE(24);assert(count>0&&count<=128);
      for(let i=0;i<count;i++)gates.set(page.readUInt32LE(36+i*8),page.readUInt32LE(32+i*8));start+=count;
    }
    const returnPc=gates.get(0xffffe);assert(returnPc);cache=new CodeCache(engine,{chain:false,budget:1000,instructions:64,blocks:16});
    assert.equal(cache.native.core_numeric,native.core_numeric);assert.equal(cache.native.core_read,native.core_read);
    assert.equal(cache.native.core_write,native.core_write);assert.equal(native.core_cold_run(0),2);
    cache.context=native.core_context_ptr()>>>0;backing=word(cache.context,64);
    for(const [index,call] of guest.calls.slice(0,4).entries()) {
      stage=`${name}: ${call.name}`;
      if(index===3) {
        wantedFp=Buffer.from(record('R3FP',128));wantedFp.writeUInt16LE(0x1b60,16);wantedFp.writeUInt16LE(0x1d20,18);
        wantedFp.writeUInt16LE(0xff3f,20);wantedFp.writeUInt16LE(0x5aa,22);wantedFp.writeUInt32LE(0x12345678,24);
        wantedFp.writeUInt32LE(0x87654321,28);wantedFp.writeUInt16LE(0x1b,32);wantedFp.writeUInt16LE(0x23,34);
        for(let slot=0;slot<8;slot++)Buffer.from(slot===2?'00000000000000800040':'00000000000000c00040','hex').copy(wantedFp,40+slot*10);
        new Uint8Array(engine.memory.buffer,engine.base+fpOffset,128).set(wantedFp);
        for(const [offset,length] of [[4364+32,128],[4612+16,128]])
          for(let j=0;j<length;j++)new Uint8Array(engine.memory.buffer)[engine.base+offset+j]=(j*17+offset)&255;
        wantedOther=otherBanks();result.seed=raw();cache.import();
      }
      untilGate(call.id);engine.ok('core_capture_windows_call');const frame=Buffer.from(engine.take(112));
      assert.equal(frame.subarray(0,4).toString(),'R3CF');assert.equal(frame.readUInt32LE(20),call.id);
      const token=frame.readUInt32LE(16),beforeState=bytes(engine.base,56),beforeStack=index===3?stack():null;
      const entryEsp=engine.get(32),beforeOuter=index===3?ram(entryEsp,32):null;
      if(index===3){assert.equal(frame.readUInt32LE(28),7);assert.equal(frame.readUInt32LE(64),preserve?0x42:0x40);
        checkBanks('before CreateDevice');wantedFp=Buffer.from(wantedFp);wantedFp.writeUInt16LE(preserve?0x1b60:0x107f,16);}
      let callbackCount=0,completed=false;
      for(let retry=0;retry<16;retry++) {
        const status=engine.status('core_complete_windows_call',token);assert([0,12].includes(status));
        result.calls.push({name:call.name,retry,status,frameHex:hex(frame),after:raw()});checkpoint();
        if(index===3)checkBanks('CreateDevice completion or suspension');
        callbackCount+=callbacks(token,returnPc,index===3);
        if(status===0){completed=true;break;}
      }
      assert(completed,'bounded CreateDevice/callback completion');
      if(index===3) {
        assert.equal(engine.get(16),0,'CreateDevice S_OK');assert.equal(engine.get(48),frame.readUInt32LE(40));
        const expectedState=Buffer.from(beforeState);expectedState.writeUInt32LE(0,16);expectedState.writeUInt32LE(entryEsp+32,32);
        expectedState.writeUInt32LE(frame.readUInt32LE(40),48);
        assert.deepEqual(bytes(engine.base,56),expectedState,'all canonical GPR/FLAGS and stdcall stack restoration');
        assert.deepEqual(ram(entryEsp,32),beforeOuter,'original return/arguments remain intact');
        if(!fullscreen)assert.deepEqual(stack(),beforeStack,'whole windowed-call startup stack remains intact');
        else {assert(callbackCount>0);const messages=result.callbacks.filter(item=>item.stage===stage).map(item=>item.message);
          for(const message of [0x46,0x83,0x47])assert(messages.includes(message),'fullscreen position callback reached');}
      } else if(index===0)assert(engine.get(16)>=0xc000);else assert(engine.get(16)>0);
      cache.import();
    }
    stage=`${name}: imported CW numerical execution`;
    const code=Buffer.alloc(4096),data=Buffer.alloc(4096),expectedData=Buffer.alloc(4096);
    const instructions=Buffer.from('dd0500006000dc0508006000db3d200060000f0b','hex');instructions.copy(code);
    data.writeBigUInt64LE(0x3ff0000000000000n,0);data.writeBigUInt64LE(0x3e70000000000000n,8);data.copy(expectedData);
    const output=Buffer.from(preserve?'0000000080000080ff3f':'0000000000000080ff3f','hex');output.copy(expectedData,0x20);
    for(const [address,page,permissions] of [[dataPc,data,3],[codePc,code,7]]){
      engine.ok('map',address,1,permissions);engine.request(page);engine.ok('upload',address,4096);}
    put(engine.base,48,codePc);cache.import();cache.budget=3;const unit=cache.entry();
    const encoded=bytes(native.core_code_ptr()>>>0,native.core_code_len()>>>0);
    result.numericModule={id:String(unit.id),sha256:hash(encoded),wasmHex:hex(encoded)};
    engine.ok('core_begin');engine.ok('core_end');const expectedContext=bytes(cache.context,128),beforeState=bytes(engine.base,56);
    const beforeRetired=word(cache.context,40),expectedFp=Buffer.from(wantedFp);
    expectedFp.writeUInt16LE(0x33d,22);expectedFp.writeUInt32LE(codePc+12,24);expectedFp.writeUInt32LE(dataPc+0x20,28);
    output.copy(expectedFp,40+2*10);
    const expectedView=view(expectedContext);expectedView.setUint32(32,codePc+18,true);expectedView.setUint32(40,beforeRetired+3,true);
    expectedView.setUint32(44,0,true);
    const run=cache.execute(unit);result.numeric={...run,id:String(run.id),after:raw(),dataHex:hex(ram(dataPc,4096))};checkpoint();
    assert.equal(run.coreReason,2);assert.equal(run.retired,3);assert.equal(run.work,3);
    assert.deepEqual(bytes(cache.context,128),expectedContext,'full context after exactly three numeric instructions');
    const expectedState=Buffer.from(beforeState);expectedState.writeUInt32LE(codePc+18,48);
    assert.deepEqual(bytes(engine.base,56),expectedState);assert.deepEqual(fp(),expectedFp,'full literal post-import FP128');
    assert.deepEqual(otherBanks(),wantedOther);assert.deepEqual(ram(dataPc,4096),expectedData);assert.deepEqual(ram(codePc,4096),code);
    result.complete=true;
  } catch(error) {result.failure={stage,message:String(error),stack:error.stack,...(opened?raw():{})};throw error;}
  finally {if(opened){const status=native.close();result.closeStatus=status;if(status!==0){result.complete=false;report.complete=false;}}checkpoint();}
  assert(result.complete);assert.equal(result.closeStatus,0);
}
try {await runCase('windowed unpreserved',false,false);await runCase('windowed preserved',true,false);
  await runCase('fullscreen unpreserved',false,true);report.complete=true;}
catch(error){report.failure={stage,message:String(error),stack:error.stack};throw error;}
finally{checkpoint();}
console.log(JSON.stringify({complete:report.complete,engineSha256:artifact.sha256,cases:report.cases.length}));
