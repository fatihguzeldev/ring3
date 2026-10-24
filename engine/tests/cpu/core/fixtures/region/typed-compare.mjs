import assert from 'node:assert/strict';
import {coldFixture,coldLiteral} from '../../../../support/core_wasm/cold_observed.mjs';

const ROOT=0x1000,TARGET=0x1800,FLAGS=0xc97;
const REGISTERS=[0xa5c30000,0x11223344,0x55667788,0x3000,0x4800,0x13579bdf,TARGET,0x5000];
const ONE_HALF='00000000000000c0ff3f',TWO_HALF='00000000000000a00040';
const hex=text=>Uint8Array.from(Buffer.from(text.replaceAll(' ',''),'hex'));

function fp(initial,{control,status=0x20,tag=0xffff,opcode=0x5a5,fip=0x11223344,
  fdp=0x55667788,cs=0x33,ds=0x5b,registers={}}={}) {
  const result=initial.slice(),view=new DataView(result.buffer,result.byteOffset,result.byteLength);
  for(const[offset,value]of[[16,control??view.getUint16(16,true)],[18,status],[20,tag],
    [22,opcode],[32,cs],[34,ds]])view.setUint16(offset,value,true);
  view.setUint32(24,fip,true);view.setUint32(28,fdp,true);
  for(const[index,value]of Object.entries(registers))result.set(hex(value),40+10*Number(index));
  return result;
}

// this observer forwards the actual shared helper once and changes no result,
// memory, ticket, native binding or generated instruction.
function observeRecovery(body) {
  const descriptor=Object.getOwnPropertyDescriptor(WebAssembly,'Instance'),Original=WebAssembly.Instance;
  let calls=0;
  const proxy=new Proxy(Original,{construct(target,args){
    const [module,imports]=args,ring3=imports?.ring3,actual=ring3?.['support.recover'];
    if(typeof actual!=='function')return Reflect.construct(target,args,target);
    return Reflect.construct(target,[module,{...imports,ring3:{...ring3,
      'support.recover':(...values)=>{calls++;return Reflect.apply(actual,undefined,values);}}}],target);
  }});
  try {
    Object.defineProperty(WebAssembly,'Instance',{...descriptor,value:proxy});
    return body(()=>calls);
  } finally {Object.defineProperty(WebAssembly,'Instance',descriptor);}
}

function literalCase(artifact,report,name,{code,control,data,initial,expected,budget,
  registers=REGISTERS,flags=FLAGS,pc=ROOT,source=false,mechanism='typed'},checks) {
  for(const compiled of[false,true])coldFixture(artifact,report,`${name}-${compiled?'emitted':'cold-reference'}`,{
    code,registers,flags,data,referencePrepared:compiled,
  },world=>{
    const input=fp(world.initialFp,{control,...initial});
    world.reset({registers,flags,pc,fp:input});
    observeRecovery(recoveryCalls=>{
      const unit=compiled?world.cache.entry(ROOT):null;
      // preparing the owner is separate from the actual interior-entry state.
      if(compiled&&pc!==ROOT)world.reset({registers,flags,pc,fp:input});
      const before=world.snapshot(),preciseBefore=world.cache.preciseRuns;
      const run=compiled?(source?world.executeSource(budget,pc):world.execute(budget,pc)):
        world.executeNative(budget);
      const wanted=expected(input);
      coldLiteral(world,before,run,{budget,registers,flags,...wanted,
        identity:compiled?unit.id:0n});
      world.result.semanticComplete=true;
      world.result.mechanism={numeric:world.helperCalls.slice(before.helpers),recoveryCalls:recoveryCalls(),
        precise:world.cache.preciseRuns-preciseBefore,owner:unit?String(unit.id):null};
      if(compiled) {
        assert.equal(world.cache.units.get(unit.id),unit,'the literal runs the same retained actual owner');
        checks.push(()=>{
          const observed=world.result.mechanism;
          if(mechanism==='typed') {
            assert.equal(observed.numeric.length,0,'native-domain loads and comparisons stay inside emitted SSA');
            assert.equal(observed.recoveryCalls,0,'successful typed comparisons request no generic recovery');
            assert.equal(observed.precise,0,'a complete typed span needs no precise suffix');
          } else if(mechanism==='fallback')assert(observed.numeric.length+observed.precise>0,
            'unsupported input retains the actual exact before-operation path');
        });
      } else {
        assert.equal(world.cache.units.size,0,'the exact reference executes without a compiled guest owner');
        assert.equal(world.result.modules.filter(item=>item.kind==='guest').length,0);
      }
    });
  });
}

export function runTypedCompareCases(artifact,report) {
  const checks=[];
  // load2.5, load1.5, compare-and-pop-both. the traditional status/SAHF
  // branch consumes C0 while preserving unrelated OF/DF. both popped payloads
  // remain observable as empty raw80 ghosts; prior sticky PE survives.
  const code='dd03 dd4308 ded9 dfe0 9e 7206 c6431800 eb04 c6431801 ffe6';
  for(const[precision,pcBits]of[[24,0],[53,0x200],[64,0x300]])for(const rounding of[0,1,2,3]) {
    const control=0x7f|pcBits|(rounding<<10);
    literalCase(artifact,report,`typed-compare-load64-fcompp-pc${precision}-rc${rounding}`,{
      code,control,budget:8,
      data:view=>{view.setFloat64(0,2.5,true);view.setFloat64(8,1.5,true);},
      expected:input=>({pc:TARGET,retired:8,flags:0xc03,
        registers:REGISTERS.map((value,index)=>index===0?0xa5c30120:value),
        writes:[{address:0x3018,bytes:'01'}],
        fp:fp(input,{control,status:0x120,tag:0xffff,opcode:0x6d9,fip:0x1005,fdp:0x3008,
          cs:0x1b,ds:0x23,registers:{6:ONE_HALF,7:TWO_HALF}})}),
    },checks);
  }
  const control=0x27f;
  // each pop count changes only TOP/tags. the loaded values remain as physical
  // ghosts, including both signs of zero and bits beyond single precision.
  for(const[relation,left,right,condition]of[
    ['less',1.5,2.5,0x100],['greater',2.5,1.5,0],['equal',1.5,1.5,0x4000],
  ])for(const[pop,compare,opcode]of[[0,'d8d1',0xd1],[1,'d8d9',0xd9],[2,'ded9',0x6d9]]) {
    literalCase(artifact,report,`typed-register-${relation}-pop${pop}`,{
      code:`dd03 dd4308 ${compare} ffe6`,control,budget:4,
      data:d=>{d.setFloat64(0,right,true);d.setFloat64(8,left,true);},
      expected:input=>({pc:TARGET,retired:4,fp:fp(input,{control,
        status:[0x3000,0x3800,0][pop]|condition|0x20,tag:[0x0fff,0x3fff,0xffff][pop],
        opcode,fip:0x1005,fdp:0x3008,cs:0x1b,ds:0x23,
        registers:{6:left===1.5?ONE_HALF:TWO_HALF,7:right===1.5?ONE_HALF:TWO_HALF}})}),
    },checks);
  }
  for(const[pop,compare,opcode]of[[0,'d8d1',0xd1],[1,'d8d9',0xd9],[2,'ded9',0x6d9]]) {
    literalCase(artifact,report,`typed-signed-zero-equal-pop${pop}`,{
      code:`dd03 dd4308 ${compare} ffe6`,control,budget:4,
      data:d=>{d.setBigUint64(0,0x8000000000000000n,true);d.setBigUint64(8,0n,true);},
      expected:input=>({pc:TARGET,retired:4,fp:fp(input,{control,
        status:[0x7000,0x7800,0x4000][pop]|0x20,tag:[0x5fff,0x7fff,0xffff][pop],
        opcode,fip:0x1005,fdp:0x3008,cs:0x1b,ds:0x23,
        registers:{6:'00000000000000000000',7:'00000000000000000080'}})}),
    },checks);
  }
  literalCase(artifact,report,'typed-pc24-load64-retains-the-low-binary64-bit',{
    code:'dd03 dd4308 ded9 ffe6',control:0x7f,budget:4,
    data:d=>{d.setBigUint64(0,0x3ff0000000000000n,true);d.setBigUint64(8,0x3ff0000000000001n,true);},
    expected:input=>({pc:TARGET,retired:4,fp:fp(input,{control:0x7f,status:0x20,tag:0xffff,
      opcode:0x6d9,fip:0x1005,fdp:0x3008,cs:0x1b,ds:0x23,
      registers:{6:'0008000000000080ff3f',7:'0000000000000080ff3f'}})}),
  },checks);
  for(const[width,compare,opcode,offset]of[
    [32,'d85304',0x53,4],[32,'d85b04',0x5b,4],
    [64,'dc5308',0x453,8],[64,'dc5b08',0x45b,8],
  ]) {
    const pop=(opcode&8)!==0;
    literalCase(artifact,report,`typed-memory${width}-compare-pop${Number(pop)}`,{
      code:`d903 ${compare} ffe6`,control,budget:3,
      data:d=>{d.setFloat32(0,1.5,true);width===32?d.setFloat32(4,2.5,true):d.setFloat64(8,2.5,true);},
      expected:input=>({pc:TARGET,retired:3,fp:fp(input,{control,status:pop?0x120:0x3920,
        tag:pop?0xffff:0x3fff,opcode,fip:0x1002,fdp:0x3000+offset,cs:0x1b,ds:0x23,
        registers:{7:ONE_HALF}})}),
    },checks);
  }
  literalCase(artifact,report,'typed-compare-overwrites-the-deferred-arithmetic-C1-writer',{
    code:'d903 d84304 d85308 ffe6',control,budget:4,initial:{status:0x220},
    data:d=>{d.setFloat32(0,1.5,true);d.setUint32(4,0x24800000,true);d.setFloat32(8,1.5,true);},
    expected:input=>({pc:TARGET,retired:4,fp:fp(input,{control,status:0x7820,tag:0x3fff,
      opcode:0x53,fip:0x1005,fdp:0x3008,cs:0x1b,ds:0x23,registers:{7:ONE_HALF}})}),
  },checks);
  literalCase(artifact,report,'typed-compare-retains-dirty-alias-and-sign-ghosts',{
    code:'dd03 d9c0 d9e0 d9c9 ded9 ffe6',control,budget:6,
    data:d=>d.setFloat64(0,1.5,true),
    expected:input=>({pc:TARGET,retired:6,fp:fp(input,{control,status:0x20,tag:0xffff,
      opcode:0x6d9,fip:0x1008,fdp:0x3000,cs:0x1b,ds:0x23,
      registers:{6:ONE_HALF,7:'00000000000000c0ffbf'}})}),
  },checks);
  literalCase(artifact,report,'typed-compare-loop-join-keeps-carried-value-and-lazy-flags',{
    code:'dd03 dc5308 49 75fa dd5b10 ffe6',control,budget:12,
    registers:REGISTERS.map((value,index)=>index===1?3:value),
    data:d=>{d.setFloat64(0,1.5,true);d.setFloat64(8,2.5,true);},
    expected:input=>({pc:TARGET,retired:12,flags:0x447,
      registers:REGISTERS.map((value,index)=>index===1?0:value),
      writes:[{address:0x3010,bytes:'000000000000f83f'}],
      fp:fp(input,{control,status:0x120,tag:0xffff,opcode:0x55b,fip:0x1008,
        fdp:0x3010,cs:0x1b,ds:0x23,registers:{7:ONE_HALF}})}),
  },checks);
  // these budgets stop at every instruction boundary. a short span may use the
  // exact adapter, so its semantic oracle does not impose a callback count.
  for(const[budget,pc,status,tag,opcode,fip,fdp,registers]of[
    [0,ROOT,0x20,0xffff,0x5a5,0x11223344,0x55667788,{}],
    [1,0x1002,0x3820,0x3fff,0x503,ROOT,0x3000,{7:TWO_HALF}],
    [2,0x1005,0x3020,0x0fff,0x543,0x1002,0x3008,{6:ONE_HALF,7:TWO_HALF}],
    [3,0x1007,0x120,0xffff,0x6d9,0x1005,0x3008,{6:ONE_HALF,7:TWO_HALF}],
    [4,TARGET,0x120,0xffff,0x6d9,0x1005,0x3008,{6:ONE_HALF,7:TWO_HALF}],
  ])literalCase(artifact,report,`typed-compare-quota-${budget}`,{
    code:'dd03 dd4308 ded9 ffe6',control,budget,mechanism:'semantic',
    data:d=>{d.setFloat64(0,2.5,true);d.setFloat64(8,1.5,true);},
    expected:input=>({pc,retired:budget,fp:fp(input,{control,status,tag,opcode,fip,fdp,
      cs:budget?0x1b:0x33,ds:budget?0x23:0x5b,registers})}),
  },checks);
  const interior=new Uint8Array(0x24);
  interior.set(hex('dd03 dd4308 eb19'));interior.set(hex('ded9 ffe6'),0x20);
  literalCase(artifact,report,'typed-compare-interior-entry-uses-actual-opaque-inputs',{
    code:interior,control,budget:2,pc:0x1020,source:true,mechanism:'semantic',
    initial:{status:0x220,tag:0xfff0,registers:{0:ONE_HALF,1:TWO_HALF}},
    expected:input=>({pc:TARGET,retired:2,fp:fp(input,{control,status:0x1120,tag:0xffff,
      opcode:0x6d9,fip:0x1020,cs:0x1b,registers:{0:ONE_HALF,1:TWO_HALF}})}),
  },checks);
  // pending exceptions precede EA. otherwise the actual read precedes stack or
  // domain rejection; a missing memory operand changes no x87 state at all.
  for(const[format,compare,width,vector]of[[32,'d817',4,0x40e],[64,'dc17',8,0x80e]]) {
    for(const occupied of[false,true])literalCase(artifact,report,`typed-memory${format}-fault-${occupied?'occupied':'empty'}`,{
      code:`${occupied?'d903 ':''}${compare} ffe6`,control,budget:occupied?3:2,mechanism:'semantic',
      data:d=>d.setFloat32(0,1.5,true),
      expected:input=>({pc:occupied?0x1002:ROOT,retired:occupied?1:0,stop:3,
        fault:[0x5000,0x10100+width,vector],
        fp:occupied?fp(input,{control,status:0x3820,tag:0x3fff,opcode:0x103,fip:ROOT,
          fdp:0x3000,cs:0x1b,ds:0x23,registers:{7:ONE_HALF}}):input}),
    },checks);
  }
  literalCase(artifact,report,'typed-load64-read-fault-before-any-push',{
    code:'dd07 ffe6',control,budget:2,mechanism:'semantic',
    expected:input=>({pc:ROOT,retired:0,stop:3,fault:[0x5000,0x10108,0x80e],fp:input}),
  },checks);
  // the first bytes are mapped in page4000; the rest would cross into absent
  // page5000. neither a partial read nor an empty stack may commit x87 effects.
  literalCase(artifact,report,'typed-load64-late-cross-page-fault-before-push',{
    code:'dd07 ffe6',control,budget:2,mechanism:'semantic',
    registers:REGISTERS.map((value,index)=>index===7?0x4ffc:value),
    expected:input=>({pc:ROOT,retired:0,stop:3,fault:[0x5000,0x10108,0x80e],fp:input}),
  },checks);
  for(const[format,compare,address,width,vector]of[
    [32,'d817',0x4ffe,4,0x40e],[64,'dc17',0x4ffc,8,0x80e],
  ])for(const occupied of[false,true])literalCase(artifact,report,
    `typed-memory${format}-late-cross-page-${occupied?'occupied':'empty'}`,{
      code:`${occupied?'d903 ':''}${compare} ffe6`,control,budget:occupied?3:2,mechanism:'semantic',
      registers:REGISTERS.map((value,index)=>index===7?address:value),
      data:d=>d.setFloat32(0,1.5,true),
      expected:input=>({pc:occupied?0x1002:ROOT,retired:occupied?1:0,stop:3,
        fault:[0x5000,0x10100+width,vector],
        fp:occupied?fp(input,{control,status:0x3820,tag:0x3fff,opcode:0x103,fip:ROOT,
          fdp:0x3000,cs:0x1b,ds:0x23,registers:{7:ONE_HALF}}):input}),
    },checks);
  literalCase(artifact,report,'typed-compare-pending-MF-before-unmapped-EA',{
    code:'dc17 ffe6',control:0x27e,budget:2,mechanism:'semantic',initial:{status:0x8081},
    expected:input=>({pc:ROOT,retired:0,stop:9,fault:[0x8081,0,16],fp:input}),
  },checks);
  literalCase(artifact,report,'typed-load64-subnormal-retains-exact-fallback-and-DE',{
    code:'dd03 ffe6',control,budget:2,mechanism:'fallback',
    data:d=>d.setBigUint64(0,1n,true),
    expected:input=>({pc:TARGET,retired:2,fp:fp(input,{control,status:0x3822,tag:0x3fff,
      opcode:0x503,fip:ROOT,fdp:0x3000,cs:0x1b,ds:0x23,
      registers:{7:'0000000000000080cd3b'}})}),
  },checks);
  literalCase(artifact,report,'typed-load64-infinity-retains-special-tag-and-exact-fallback',{
    code:'dd03 ffe6',control,budget:2,mechanism:'fallback',
    data:d=>d.setBigUint64(0,0x7ff0000000000000n,true),
    expected:input=>({pc:TARGET,retired:2,fp:fp(input,{control,status:0x3820,tag:0xbfff,
      opcode:0x503,fip:ROOT,fdp:0x3000,cs:0x1b,ds:0x23,
      registers:{7:'0000000000000080ff7f'}})}),
  },checks);
  literalCase(artifact,report,'typed-register-raw80-low-bit-cannot-be-rounded-away',{
    code:'d8d1 ffe6',control,budget:2,mechanism:'fallback',
    initial:{status:0x220,tag:0xfff0,registers:{0:'01000000000000c00040',1:'00000000000000c00040'}},
    expected:input=>({pc:TARGET,retired:2,fp:fp(input,{control,status:0x20,tag:0xfff0,
      opcode:0xd1,fip:ROOT,cs:0x1b})}),
  },checks);
  literalCase(artifact,report,'typed-register-ordered-qNaN-retains-masked-invalid-fallback',{
    code:'d8d1 ffe6',control,budget:2,mechanism:'fallback',
    initial:{status:0x220,tag:0xfff2,registers:{0:'01000000000000c0ff7f',1:ONE_HALF}},
    expected:input=>({pc:TARGET,retired:2,fp:fp(input,{control,status:0x4521,tag:0xfff2,
      opcode:0xd1,fip:ROOT,cs:0x1b})}),
  },checks);
  literalCase(artifact,report,'typed-register-quiet-qNaN-preserves-the-original-exception-policy',{
    code:'dde1 ffe6',control,budget:2,mechanism:'fallback',
    initial:{status:0x220,tag:0xfff2,registers:{0:'01000000000000c0ff7f',1:ONE_HALF}},
    expected:input=>({pc:TARGET,retired:2,fp:fp(input,{control,status:0x4520,tag:0xfff2,
      opcode:0x5e1,fip:ROOT,cs:0x1b})}),
  },checks);
  literalCase(artifact,report,'typed-register-empty-retains-stack-fault-fallback',{
    code:'d8d1 ffe6',control,budget:2,mechanism:'fallback',initial:{status:0x220},
    expected:input=>({pc:TARGET,retired:2,fp:fp(input,{control,status:0x4561,tag:0xffff,
      opcode:0xd1,fip:ROOT,cs:0x1b})}),
  },checks);
  literalCase(artifact,report,'typed-register-unmasked-normal-input-retains-exact-path',{
    code:'d8d1 ffe6',control:0x27e,budget:2,mechanism:'fallback',
    initial:{status:0x20,tag:0xfff0,registers:{0:ONE_HALF,1:TWO_HALF}},
    expected:input=>({pc:TARGET,retired:2,fp:fp(input,{control:0x27e,status:0x120,tag:0xfff0,
      opcode:0xd1,fip:ROOT,cs:0x1b})}),
  },checks);
  literalCase(artifact,report,'typed-compare-followed-by-store-fault-keeps-unpopped-value',{
    code:'dd03 dc5308 dd1f ffe6',control,budget:4,mechanism:'semantic',
    data:d=>{d.setFloat64(0,1.5,true);d.setFloat64(8,2.5,true);},
    expected:input=>({pc:0x1005,retired:2,stop:3,fault:[0x5000,0x10208,0x80e],
      fp:fp(input,{control,status:0x3920,tag:0x3fff,opcode:0x453,fip:0x1002,
        fdp:0x3008,cs:0x1b,ds:0x23,registers:{7:ONE_HALF}})}),
  },checks);
  literalCase(artifact,report,'typed-load64-completed-before-executable-write',{
    code:'dd03 c60790 dc5308 ffe6',control,budget:4,mechanism:'semantic',
    registers:REGISTERS.map((value,index)=>index===7?0x10f0:value),
    data:d=>{d.setFloat64(0,1.5,true);d.setFloat64(8,2.5,true);},
    expected:input=>({pc:0x1005,retired:2,stop:6,writes:[{address:0x10f0,bytes:'90'}],
      fp:fp(input,{control,status:0x3820,tag:0x3fff,opcode:0x503,fip:ROOT,
        fdp:0x3000,cs:0x1b,ds:0x23,registers:{7:ONE_HALF}})}),
  },checks);
  // all independently authored architectural controls run before the OLD
  // engine reaches the intended callback-count falsifier.
  for(const check of checks)check();
}
