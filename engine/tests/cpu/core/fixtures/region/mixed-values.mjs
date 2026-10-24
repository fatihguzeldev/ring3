import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';

const ROOT=0x1000,TARGET=0x1800,N=128,TOTAL=2*N+1,FLAGS=0x403;
const registers=[0x7fffffc0,0x11223344,0x55667788,0x3000,0x4800,0x13579bdf,TARGET,0x2468ace0];
const initialFp={status:0x1220,tag:0xff2f,opcode:0x5a5,fip:0x11223344,fdp:0x55667788,
  registers:{0:'01000000000000c00040',1:'00000000000000000080',2:'ffffffffffffffffffff'}};
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
const dword=value=>{const b=Buffer.alloc(4);b.writeUInt32LE(value>>>0);return b.toString('hex');};
const code=Array.from({length:N},(_,i)=>`8d4001 8b93${dword(4*i)}`).join(' ')+' ffe6';
const data=page=>{const v=new DataView(page.buffer);for(let i=0;i<1024;i++)v.setUint32(i*4,0x89abcd00+i,true);};

function literal(t,wanted) {
  const expected=t.bytes(t.context(),128),v=new DataView(expected.buffer),gpr=t.registers.slice();
  for(const [i,value]of Object.entries(wanted.regs??{}))gpr[Number(i)]=value;
  gpr.forEach((value,i)=>v.setUint32(4*i,value>>>0,true));
  const fault=wanted.fault??[0,0,0];
  for(const [offset,value]of[[32,wanted.pc],[36,FLAGS],[40,wanted.retired],[44,wanted.remaining??0],
    [48,fault[0]],[52,fault[1]],[112,0],[116,fault[2]]])v.setUint32(offset,value>>>0,true);
  const run=t.check({...wanted,flags:FLAGS,fp:t.startingFp});
  assert.deepEqual(t.bytes(t.context(),128),expected,'literal complete Context128 including lease/owner fields');
  run.contextSha256=hash(expected);return run;
}
function retained(t,body) {
  const encoded=t.bytes(t.api.core_code_ptr()>>>0,t.api.core_code_len()>>>0);
  const identity=new DataView(t.engine.memory.buffer).getBigUint64(t.context()+120,true),revision=t.version();
  body();
  assert.equal(new DataView(t.engine.memory.buffer).getBigUint64(t.context()+120,true),identity);
  assert.deepEqual(t.bytes(t.api.core_code_ptr()>>>0,t.api.core_code_len()>>>0),encoded);
  assert.equal(t.result.modules.length,1);assert.equal(t.version(),revision,'ordered reads never write RAM/stamps');
  t.result.semanticComplete=true;t.result.retained={id:String(identity),sha256:hash(encoded),bytes:encoded.length};
}
export function runMixedValueCases(fixture) {
  const mechanism=[];
  for(const c of[
    {name:'self-subtract',code:'2bc0 ffe6',retired:2,flags:0x446},
    {name:'compare-then-replace-register',code:'83f801 b800000000 ffe6',retired:3,flags:0x412},
  ])fixture(`mixed-values-${c.name}-pins-flags-input-before-register-publication`,{
    segments:[[ROOT,c.code]],registers,initialFlags:FLAGS,initialFp,data,
  },t=>retained(t,()=>{
    const expected=t.bytes(t.context(),128),v=new DataView(expected.buffer);
    for(const[offset,value]of[[0,0],[32,TARGET],[36,c.flags],[40,c.retired],[44,0],[112,0]])
      v.setUint32(offset,value,true);
    // SUB r,r has a constant result but its exact flag recipe still owns the
    // old input. CMP followed by MOV similarly leaves only FLAGS using EAX.
    const run=t.check({pc:TARGET,retired:c.retired,budget:c.retired,regs:{0:0},
      flags:c.flags,fp:t.startingFp});
    assert.deepEqual(t.bytes(t.context(),128),expected,'full Context128 retains the original flag operands');
    assert.equal(run.activations.length,1);assert.equal(run.helperCalls,0);assert.equal(run.preciseCalls,0);
    t.result.flagsRecovery={initialEax:registers[0],finalEax:0,flags:c.flags,retired:c.retired};
  }));
  fixture('mixed-values-128-ordered-reads-preserve-one-live-affine-value',{
    segments:[[ROOT,code]],registers,initialFlags:FLAGS,initialFp,data,compileInstructions:512,
    stateWitness:true,witnessData:[0x3000,512],
  },t=>retained(t,()=>{
    const run=literal(t,{pc:TARGET,retired:TOTAL,budget:TOTAL,regs:{0:0x80000040,2:0x89abcd7f}});
    assert.equal(run.activations.length,1);assert.equal(run.helperCalls,0);assert.equal(run.preciseCalls,0);
    const witness=t.stateWitness();
    assert.equal(witness.data_reads,128,'all distinct ordered reads actually execute even though127 results are dead');
    t.result.mixedWitness=witness;
    mechanism.push(()=>assert(witness.scalar_eax_input_reads<=4,
      `live final EAX is demanded once across all ordered reads; actual architectural EAX reads=${witness.scalar_eax_input_reads}`));
  }));
  fixture('mixed-values-all257-prefixes-retain-exact-state-and-owner',{
    segments:[[ROOT,code]],registers,initialFlags:FLAGS,initialFp,data,compileInstructions:512,
  },t=>retained(t,()=>{
    literal(t,{pc:ROOT,retired:0,budget:0});
    for(let completed=1;completed<=TOTAL;completed++) {
      const pairs=Math.floor(completed/2),incs=Math.min(N,Math.ceil(completed/2));
      const pc=completed===TOTAL?TARGET:ROOT+9*pairs+(completed%2?3:0);
      literal(t,{pc,retired:completed,budget:1,
        regs:{0:(registers[0]+incs)>>>0,2:pairs?0x89abcd00+pairs-1:registers[2]}});
    }
  }));
  for(const faultAt of[1,64,128]) {
    const base=0x4000-4*(faultAt-1),completed=2*faultAt-1;
    fixture(`mixed-values-fault-read${faultAt}-recovers-the-exact-unmaterialized-prefix`,{
      segments:[[ROOT,code]],registers:[...registers.slice(0,3),base,...registers.slice(4)],
      initialFlags:FLAGS,initialFp,data,missing:[0x4000],compileInstructions:512,
    },t=>retained(t,()=>{
      const run=literal(t,{pc:ROOT+3+9*(faultAt-1),retired:completed,budget:TOTAL,work:completed,
        remaining:TOTAL-completed,stop:3,fault:[0x4000,0x10104,14],
        regs:{0:(registers[0]+faultAt)>>>0,2:faultAt>1?0x89abd0ff:registers[2]}});
      assert.equal(run.helperCalls,0);assert.equal(run.preciseCalls,0);
    }));
  }
  // Freeze all literal architectural results before asking the old binary to
  // fail on its actual executed representation.
  for(const check of mechanism)check();
}
