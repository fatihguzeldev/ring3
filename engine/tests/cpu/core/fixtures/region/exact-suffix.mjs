import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';

const ROOT=0x1000,TARGET=0x1800,FLAGS=0x403,FULL80='01000000000000c00040',THREE='00000000000000c00040';
const registers=[0x13579bdf,2,0x2468ace0,0x3000,0x4800,0x89abcdef,TARGET,0x76543210];
const initialFp={status:0x200,tag:0xfff4,opcode:0x5a5,fip:0x11223344,fdp:0x55667788,cs:0x33,ds:0x5b,
  registers:{0:FULL80,1:'00000000000000000080',2:'0000000000000040ff3f',
    3:'00000000000000c0ff7f',4:'7856341200000080ff3f',5:'ffffffffffffffffffff',
    6:'efcdab8967452301807f',7:'ffffffffffffffffffff'}};
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
const unhex=value=>Uint8Array.from(Buffer.from(value.replaceAll(' ',''),'hex'));
const wordHex=value=>{const b=Buffer.alloc(4);b.writeUInt32LE(value>>>0);return b.toString('hex');};
const add=value=>(registers[0]+value)>>>0;
const head='8d4001 d8c1 8d4002 eb16';
const storedHead='8d4001 d8c1 c70778563412 8d4002 eb10';
const tail='8d4004 89430c ffe6';

function fpAfter(t,control) {
  // adding negative zero preserves all 64 significand bits at pc64. pc24/53
  // round 3+2^-62 to3, set pe, and all three modes clear the previous c1.
  return t.fp({status:control===0x37f?0:0x20,tag:0xfff4,opcode:0xc1,
    fip:0x1003,fdp:0x55667788,cs:0x1b,ds:0x5b,
    registers:{0:control===0x37f?FULL80:THREE}});
}

function checked(t,wanted,{writes=[],entries=1,precise=1,revoked=false}={},checks) {
  const context=t.context(),revision=t.version();
  const encoded=t.bytes(t.api.core_code_ptr()>>>0,t.api.core_code_len()>>>0);
  const expectedContext=t.bytes(context,128),c=new DataView(expectedContext.buffer);
  const identity=c.getBigUint64(120,true),fault=wanted.fault??[0,0,0];
  const expectedRegisters=t.registers.slice();
  for(const[index,value]of Object.entries(wanted.regs??{}))expectedRegisters[Number(index)]=value;
  expectedRegisters.forEach((value,index)=>c.setUint32(index*4,value>>>0,true));
  for(const[offset,value]of[[32,wanted.pc],[36,FLAGS],[40,wanted.retired],[44,wanted.remaining??0],
    [48,fault[0]],[52,fault[1]],[112,0],[116,fault[2]]])c.setUint32(offset,value>>>0,true);
  if(revoked)c.setBigUint64(120,0n,true);
  const count=t.word(context,68),backing=t.word(context,64),pages=t.word(context,84);
  const expectedRam=t.bytes(backing,count*4096),expectedPages=t.bytes(pages,count*16);
  const pageView=new DataView(expectedPages.buffer);
  for(const[address,bytes,ordinal]of writes) {
    const slot=t.word(t.word(context,56)+(address>>>12)*4);
    assert(slot>0&&slot<=count,'each literal write names an actual mapped physical slot');
    expectedRam.set(unhex(bytes),(slot-1)*4096+(address&4095));
    pageView.setBigUint64((slot-1)*16+8,revision+BigInt(ordinal),true);
    t.writeExpected(address,bytes);
  }
  const run=t.check({...wanted,flags:FLAGS});
  assert.deepEqual(t.bytes(context,128),expectedContext,'complete literal context including source and owner authority');
  assert.deepEqual(t.bytes(backing,expectedRam.length),expectedRam,'entire physical backing follows only the completed stores');
  assert.deepEqual(t.bytes(pages,expectedPages.length),expectedPages,'all physical page stamps retain exact write order');
  assert.equal(t.version(),revision+BigInt(writes.length),'one transaction per completed store');
  assert.equal(t.result.modules.length,1,'the suffix resumes without compiling another owner');
  assert.equal(run.activations.length,1,'one native lease owns the complete literal run');
  assert.deepEqual(t.bytes(t.api.core_code_ptr()>>>0,t.api.core_code_len()>>>0),encoded,'the source module is retained');
  const witness=t.stateWitness();
  t.result.exactSuffix={owner:String(identity),sha256:hash(encoded),witness};
  t.result.semanticComplete=true;
  checks.push(()=>{
    assert.equal(run.helperCalls,0,'the actual precise executor owns fallback semantics without an ordinal proxy');
    assert.equal(run.preciseCalls,precise,'the exact suffix is entered once when an operation really needs it');
    assert.equal(witness.body_entries,entries,'an eligible same-body continuation does not repeat guest.run bootstrap');
    assert.equal(witness.canonical_calls,0,'no replacement numerical callback is substituted for the exact suffix');
  });
  if(revoked)t.status('core_begin',4);
}

export function runExactSuffixCases(fixture) {
  const checks=[];
  for(const control of[0x7f,0x27f,0x37f])fixture(`exact-suffix-midspan-resumes-same-body-pc${control===0x7f?24:control===0x27f?53:64}`,{
    segments:[[ROOT,head],[0x1020,tail]],registers,initialFlags:FLAGS,initialFp,control,stateWitness:true,
  },t=>checked(t,{pc:TARGET,retired:7,budget:7,regs:{0:add(7)},fp:fpAfter(t,control)},
    {writes:[[0x300c,wordHex(add(7)),1]]},checks));

  // the short quota controls retain their exact authored prefixes; the final
  // scalar store cannot be pulled before either its quota or its fp predecessor.
  for(const budget of[0,2,6])fixture(`exact-suffix-short-quota-${budget}`,{
    segments:[[ROOT,head],[0x1020,tail]],registers,initialFlags:FLAGS,initialFp,control:0x27f,stateWitness:true,
  },t=>checked(t,{pc:budget===0?ROOT:budget===2?0x1005:0x1026,retired:budget,budget,
    regs:{0:add(budget===0?0:budget===2?1:7)},fp:budget===0?t.startingFp:fpAfter(t,0x27f)},
    {precise:Number(budget!==0),writes:budget===6?[[0x300c,wordHex(add(7)),1]]:[]},checks));

  for(const variant of['ordinary','fault','smc'])fixture(`exact-suffix-${variant}-preserves-current-prefix-and-store-authority`,{
    segments:[[ROOT,storedHead],[0x1020,tail]],
    registers:[...registers.slice(0,7),variant==='fault'?0x6000:variant==='smc'?0x1100:0x3008],
    initialFlags:FLAGS,initialFp,control:0x27f,stateWitness:true,
  },t=>{
    const ordinary=variant==='ordinary',smc=variant==='smc';
    checked(t,{pc:ordinary?TARGET:smc?0x100b:0x1005,retired:ordinary?8:smc?3:2,budget:8,
      work:ordinary?8:smc?3:2,remaining:ordinary?0:smc?5:6,stop:ordinary?2:smc?6:3,
      fault:variant==='fault'?[0x6000,0x10204,0x40e]:[0,0,0],
      regs:{0:add(ordinary?7:1)},fp:fpAfter(t,0x27f)},
    {revoked:smc,writes:ordinary?[[0x3008,'78563412',1],[0x300c,wordHex(add(7)),2]]:
      smc?[[0x1100,'78563412',1]]:[]},checks);
  });
  fixture('exact-suffix-simd-rejection-resumes-same-body-with-exact-vector-result',{
    segments:[[ROOT,'8d4001 0f5803 8d4002 eb15'],[0x1020,tail]],registers,
    initialFlags:FLAGS,initialFp,control:0x27f,stateWitness:true,
    data:page=>{const d=new DataView(page.buffer);for(let lane=0;lane<4;lane++)d.setUint32(lane*4,0x7fc00000,true);},
  },t=>{
    const simd=t.startingSimd.slice(),d=new DataView(simd.buffer);
    for(let lane=0;lane<4;lane++)d.setUint32(32+lane*4,0x7fc00000,true);
    checked(t,{pc:TARGET,retired:7,budget:7,regs:{0:add(7)},fp:t.startingFp,simd},
      {writes:[[0x300c,wordHex(add(7)),1]]},checks);
  });
  // all independent context/fp/vector/ram/stamp oracles precede the old-red
  // activation assertion, so the pinned old engine fails only on the format.
  for(const check of checks)check();
}
