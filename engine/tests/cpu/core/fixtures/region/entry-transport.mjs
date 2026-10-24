import assert from 'node:assert/strict';
import {coldFixture,coldLiteral} from '../../../../support/core_wasm/cold_observed.mjs';

const ROOT=0x1000,TARGET=0x1800,FLAGS=0xc97;
const REGISTERS=[0xa5c30000,2,0x55667788,0x3000,0x4800,0x13579bdf,TARGET,0x5000];
const ONE_HALF='00000000000000c0ff3f',FULL80='01000000000000c00040';
const hex=text=>Uint8Array.from(Buffer.from(text,'hex'));

function image(initial,{status=0x220,tag=0xffff,opcode=0x5a5,fip=0x11223344,
  fdp=0x55667788,cs=0x33,ds=0x5b,registers={}}={}) {
  const result=initial.slice(),view=new DataView(result.buffer,result.byteOffset,result.byteLength);
  for(const[offset,value]of[[16,0x27f],[18,status],[20,tag],[22,opcode],[32,cs],[34,ds]])
    view.setUint16(offset,value,true);
  view.setUint32(24,fip,true);view.setUint32(28,fdp,true);
  for(const[index,value]of Object.entries(registers))result.set(hex(value),40+10*Number(index));
  return result;
}

// forward the actual helper unchanged; no guest result or authority is supplied
// by this observer, and the original constructor descriptor is restored.
function observeRecovery(body) {
  const descriptor=Object.getOwnPropertyDescriptor(WebAssembly,'Instance'),Original=WebAssembly.Instance;
  let calls=0;
  try {
    Object.defineProperty(WebAssembly,'Instance',{...descriptor,value:new Proxy(Original,{construct(target,args){
      const[module,imports]=args,actual=imports?.ring3?.['support.recover'];
      if(typeof actual!=='function')return Reflect.construct(target,args,target);
      return Reflect.construct(target,[module,{...imports,ring3:{...imports.ring3,
        'support.recover':(...values)=>{calls++;return Reflect.apply(actual,undefined,values);}}}],target);
    }})});
    return body(()=>calls);
  } finally {Object.defineProperty(WebAssembly,'Instance',descriptor);}
}

const LOOP='55 89e5 d903 d95b04 5d 49 75f4 ffe6';
const stackWrite={address:0x47fc,bytes:'df9b5713'};

function literalCase(artifact,report,name,{code=LOOP,registers=REGISTERS,budget=15,
  initial={registers:{7:FULL80}},expected,mechanism='semantic'},checks) {
  for(const compiled of[false,true])coldFixture(artifact,report,
    `entry-transport-${name}-${compiled?'emitted':'cold-reference'}`,{
      code,registers,flags:FLAGS,referencePrepared:compiled,data:d=>d.setFloat32(0,1.5,true),
    },world=>{
      const input=image(world.initialFp,initial);
      world.reset({fp:input});
      observeRecovery(recoveryCalls=>{
        const unit=compiled?world.cache.entry(ROOT):null,before=world.snapshot(),precise=world.cache.preciseRuns;
        const run=compiled?world.execute(budget,ROOT):world.executeNative(budget);
        const wanted=expected(input,{compiled});
        coldLiteral(world,before,run,{budget,registers,flags:FLAGS,...wanted,
          identity:compiled?unit.id:0n});
        if(wanted.faultWidth!==undefined) {
          const vector=world.word(world.context,116),metadata=world.word(world.context,52);
          assert.equal((vector>>>8)||(metadata&255),wanted.faultWidth,
            'both exact private encodings publish the same complete access width');
        }
        world.result.semanticComplete=true;
        world.result.mechanism={precise:world.cache.preciseRuns-precise,
          numeric:world.helperCalls.slice(before.helpers),recovery:recoveryCalls(),owner:unit?String(unit.id):null};
        if(compiled) {
          assert.equal(world.cache.units.get(unit.id),unit,'the original immutable owner remains retained');
          checks.push(()=>{
            const observed=world.result.mechanism;
            if(mechanism==='typed') {
              assert.equal(observed.precise,0,
                'an overwritten transport-only ghost cannot reject the scalar prologue');
              assert.equal(observed.numeric.length,0,'normal FLD/FSTP stay typed');
              assert.equal(observed.recovery,0,'completed normal blocks need no generic recovery');
            } else if(mechanism==='fallback')assert(observed.precise+observed.numeric.length>0,
              'an actual unrepresentable operand retains its exact before-operation path');
          });
        } else {
          assert.equal(world.cache.units.size,0,'the independent exact reference has no generated owner');
          assert.equal(world.result.modules.filter(item=>item.kind==='guest').length,0);
        }
      });
    });
}

export function runEntryTransportCases(artifact,report) {
  const checks=[];
  // each backedge carries the popped Native24 ghost. an independent external
  // entry may instead contain any empty raw80 ghost, which FLD overwrites before
  // any instruction consumes that payload. PUSH/MOV still have real effects.
  for(const[name,ghost]of[['full80-empty-ghost',FULL80],['exact-native-empty-ghost',ONE_HALF]]) {
    literalCase(artifact,report,name,{initial:{registers:{7:ghost}},mechanism:'typed',
      expected:input=>({pc:TARGET,retired:15,
        registers:REGISTERS.map((value,index)=>index===1?0:value),flags:0x447,
        writes:[{...stackWrite,count:2,lastVersion:3},
          {address:0x3004,bytes:'0000c03f',count:2,lastVersion:4}],versionWrites:4,
        fp:image(input,{status:0x20,tag:0xffff,opcode:0x15b,fip:0x1005,fdp:0x3004,
          cs:0x1b,ds:0x23,registers:{7:ONE_HALF}})}),
    },checks);
  }
  // short quotas preserve the original raw80 bytes until the overwriting FLD.
  // they may use the exact short-span adapter, so no callback count is imposed.
  for(const budget of[0,1,2])literalCase(artifact,report,`raw-ghost-short-quota-${budget}`,{
    budget,expected:input=>({pc:[ROOT,0x1001,0x1003][budget],retired:budget,fp:input,
      registers:REGISTERS.map((value,index)=>index===4&&budget>0||index===5&&budget===2?0x47fc:value),
      writes:budget?[stackWrite]:[]}),
  },checks);
  literalCase(artifact,report,'push-fault-preserves-unconsumed-raw-ghost',{
    registers:REGISTERS.map((value,index)=>index===4?0x5004:value),
    // boundary.rs accepts width in vector>>8 from the exact executor, or in
    // metadata&255 from the generated scalar core_write path. both are exact.
    expected:(input,{compiled})=>({pc:ROOT,retired:0,stop:3,fp:input,faultWidth:4,
      fault:[0x5000,0x10204,compiled?14:0x40e]}),
  },checks);
  literalCase(artifact,report,'load-fault-after-real-prologue-preserves-raw-ghost',{
    registers:REGISTERS.map((value,index)=>index===3?0x5000:value),
    expected:input=>({pc:0x1003,retired:2,stop:3,fp:input,fault:[0x5000,0x10104,0x40e],
      registers:REGISTERS.map((value,index)=>index===3?0x5000:index===4||index===5?0x47fc:value),
      writes:[stackWrite]}),
  },checks);
  // the same low raw80 bit becomes semantically relevant when FCOM consumes it:
  // 3+2^-62 is greater than3, rather than equal after an illicit f64 rounding.
  literalCase(artifact,report,'actual-raw80-consumer-still-uses-exact-admission',{
    code:'55 89e5 d8d1 5d ffe6',budget:5,mechanism:'fallback',
    initial:{tag:0xfff0,registers:{0:FULL80,1:'00000000000000c00040',7:FULL80}},
    expected:input=>({pc:TARGET,retired:5,writes:[stackWrite],
      fp:image(input,{status:0x20,tag:0xfff0,opcode:0xd1,fip:0x1003,cs:0x1b})}),
  },checks);
  // every complete architectural control precedes the intended OLD mechanism
  // rejection, including the control with an already representable ghost.
  for(const check of checks)check();
}
