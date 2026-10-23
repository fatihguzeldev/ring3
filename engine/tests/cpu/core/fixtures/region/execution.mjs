import assert from 'node:assert/strict';
import {runFlagsOracleCases} from './execution-flags.mjs';

const THREE='00000000000000c00040',TWO='00000000000000800040';
const LEFT='0000000000010080ff3f',PRODUCT='0000020000020080fe3f';
const roundedProduct='0000000000020080fe3f';
const vector='0000803f000000400000404000008040';

function retained(t,wanted,helpers=0) {
  const before=t.version(),run=t.check(wanted);
  assert.equal(run.activations.length,1,'one retained architectural instruction body executes the complete case');
  assert.equal(t.result.modules.length,1,'no recompilation conceals a failed entry/value contract');
  assert.equal(run.helperCalls,helpers,'the exact expected canonical numerical boundary count');
  t.result.semanticComplete=true;
  return{run,before,witness:t.stateWitness()};
}

function ordinaryFlags(witness) {
  assert.equal(witness.flags_calls,0,'known FLAGS consumers avoid the shared general recipe resolver before observation');
  assert.equal(witness.flag_kind_queries,0,'known FLAGS consumers never query the runtime recipe kind');
  assert.equal(witness.flag_width_queries,0,'known FLAGS consumers never query the runtime operand width');
}

export function runExecutionFlagsCases(fixture) {
  for(const c of[
    {name:'cmp-signed-overflow-taken',code:'39d8',eax:0x80000000,ebx:1,result:0x80000000,flags:0x816,edx:7},
    {name:'cmp-signed-overflow-untaken',code:'39d8',eax:1,ebx:0x80000000,result:1,flags:0x883,edx:9},
    {name:'add-signed-overflow-is-not-negative',code:'01d8',eax:0x7fffffff,ebx:1,result:0x80000000,flags:0x896,edx:9},
  ])fixture(`execution-literal-flags-${c.name}`,{
    segments:[[0x1000,`${c.code} 7c0c ba09000000 eb15`],[0x1010,'ba07000000 eb09'],[0x1020,'90 0f0b']],
    registers:[c.eax,2,0,c.ebx,0x4800,0,0,0],stateWitness:true,
  },t=>{
    const{before,witness}=retained(t,{pc:0x1021,retired:5,budget:5,flags:c.flags,
      regs:{0:c.result,2:c.edx},fp:t.startingFp});
    assert.equal(t.version(),before);t.result.executionWitness=witness;ordinaryFlags(witness);
  });
  fixture('execution-literal-flags-preserves-carry-through-inc',{
    segments:[[0x1000,'01d8 41 720b ba09000000 eb14'],[0x1010,'ba07000000 eb09'],[0x1020,'90 0f0b']],
    registers:[0xffffffff,0,0,1,0x4800,0,0,0],stateWitness:true,
  },t=>{
    const{before,witness}=retained(t,{pc:0x1021,retired:6,budget:6,flags:3,ecx:1,
      regs:{0:0,2:7},fp:t.startingFp});
    assert.equal(t.version(),before);t.result.executionWitness=witness;ordinaryFlags(witness);
  });
  for(const[budget,pc,flags,ecx]of[[1,0x1002,0x57,0],[2,0x1003,3,1]])
    fixture(`execution-literal-flags-precise-prefix-${budget}`,{
      segments:[[0x1000,'01d8 41 720b ba09000000 eb14'],[0x1010,'ba07000000 eb09'],[0x1020,'90 0f0b']],
      registers:[0xffffffff,0,0,1,0x4800,0,0,0],stateWitness:true,
    },t=>{
      const{before}=retained(t,{pc,retired:budget,budget,flags,ecx,regs:{0:0},fp:t.startingFp});
      assert.equal(t.version(),before);
    });
  fixture('execution-dead-inc-recipe-preserves-add-carry-before-cmp',{
    segments:[[0x1000,'01c8 42 0f924320 39fe 0f9c4321 ffe5']],
    registers:[0xffffffff,1,0,0x3000,0x4800,0x1100,0x80000000,1],stateWitness:true,
  },t=>{
    t.writeExpected(0x3020,'0101');
    const{before,witness}=retained(t,{pc:0x1100,retired:6,budget:6,flags:0x816,
      regs:{0:0,2:1},fp:t.startingFp});
    assert.equal(t.version(),before+2n);t.result.executionWitness=witness;ordinaryFlags(witness);
  });
}

export const runExecutionFlagsOracleCases=runFlagsOracleCases;

const scalarData=page=>{
  const d=new DataView(page.buffer);d.setUint32(0,0x89abcdef,true);d.setUint32(4,0x01234567,true);
};

export function runExecutionLoadCases(fixture) {
  fixture('execution-load-availability-reads-each-value-once',{
    segments:[[0x1000,'8b03 8b13 8b4b04 8b7304 ffe7']],
    registers:[0,2,0,0x3000,0x4800,0,0,0x1100],data:scalarData,stateWitness:true,witnessData:[0x3000,8],
  },t=>{
    const{before,witness}=retained(t,{pc:0x1100,retired:5,budget:5,ecx:0x01234567,
      regs:{0:0x89abcdef,2:0x89abcdef,6:0x01234567},fp:t.startingFp});
    assert.equal(t.version(),before);t.result.executionWitness=witness;
    assert.equal(witness.data_reads,2,'two exact memory values serve four scalar loads');
    assert.equal(witness.data_writes,0);
  });
  for(const secondary of[false,true])fixture(`execution-load-availability-${secondary?'interior-entry':'quota-tail'}-retains-exact-load`,{
    segments:[[0x1000,'8b03 8b13 ffe7']],
    registers:[0x13579bdf,2,0,0x3000,0x4800,0,0,0x1100],data:scalarData,stateWitness:true,witnessData:[0x3000,4],
  },t=>{
    if(secondary){new DataView(t.engine.memory.buffer).setUint32(t.context()+32,0x1002,true);
      t.result.secondaryEntry={pc:0x1002,retainedPrimary:0x1000};}
    const{before,witness}=retained(t,secondary?{pc:0x1100,retired:2,budget:2,
      regs:{2:0x89abcdef},fp:t.startingFp}:{pc:0x1002,retired:1,budget:1,
      regs:{0:0x89abcdef},fp:t.startingFp});
    assert.equal(t.version(),before);t.result.executionWitness=witness;
    assert.equal(witness.data_reads,1,'an unexecuted producer or partial-block cache never supplies a value');
  });
  fixture('execution-load-availability-store-kills-the-earlier-value',{
    segments:[[0x1000,'8b03 890b 8b13 ffe7']],
    registers:[0,0x01234567,0,0x3000,0x4800,0,0,0x1100],data:scalarData,stateWitness:true,witnessData:[0x3000,8],
  },t=>{
    t.writeExpected(0x3000,'67452301');
    const{before,witness}=retained(t,{pc:0x1100,retired:4,budget:4,
      regs:{0:0x89abcdef,2:0x01234567},fp:t.startingFp});
    assert.equal(t.version(),before+1n);t.result.executionWitness=witness;
    assert.equal(witness.data_reads,2);assert.equal(witness.data_writes,1);
  });
  fixture('execution-load-availability-canonical-fp-write-kills-the-earlier-value',{
    segments:[[0x1000,'8b03 db3b 8b13 ffe7']],
    registers:[0,2,0,0x3000,0x4800,0,0,0x1100],data:scalarData,stateWitness:true,witnessData:[0x3000,8],
    initialFp:{tag:0xfffc,registers:{0:THREE}},
  },t=>{
    t.writeExpected(0x3000,THREE);
    const{before,witness}=retained(t,{pc:0x1100,retired:4,budget:4,regs:{0:0x89abcdef,2:0},
      fp:t.fp({status:0x800,opcode:0x33b,fip:0x1002,fdp:0x3000,registers:{0:THREE}})},1);
    assert.equal(t.version(),before+1n);t.result.executionWitness=witness;
    assert.equal(witness.data_reads,2,'a helper returning changed bytes invalidates value availability');
  });
}

export function runExecutionGuardCases(fixture) {
  fixture('execution-fp-first-use-guards-survive-gpr-and-packed-simd',{
    segments:[[0x1000,'d8c1 31c0 0f104320 d8c1 d8c1 ffe7']],
    registers:[0x13579bdf,2,0,0x3000,0x4800,0,0,0x1100],stateWitness:true,
    initialFp:{tag:0xfff0,registers:{0:THREE,1:TWO}},
    data:page=>page.set(Buffer.from(vector,'hex'),32),
  },t=>{
    const simd=t.startingSimd.slice();simd.set(Buffer.from(vector,'hex'),32);
    const{before,witness}=retained(t,{pc:0x1100,retired:6,budget:6,flags:0x46,regs:{0:0},simd,
      fp:t.fp({tag:0xfff0,opcode:0xc1,fip:0x100a,registers:{0:'00000000000000900240',1:TWO}})});
    assert.equal(t.version(),before);t.result.executionWitness=witness;
    assert.equal(witness.fp_environment_queries,1,'only the first actual x87 use queries/imports its environment');
    assert(witness.fp_input_load_queries>0&&witness.fp_input_load_queries<=2,'the two unchanged physical inputs are queried once');
    assert(witness.fp_input_admission_queries<=2,'unchanged inputs reuse their dominating representation admission');
    assert(witness.fp_control_queries<=1,'one actual arithmetic-control query covers the complete admitted region');
    assert(witness.fp_occupancy_queries<=2,'unchanged occupied inputs do not repeat tag admission');
  });
}

export function runExecutionMultiplyCases(fixture) {
  for(const[control,sticky,iterations]of[[0x27f,0,1],[0x27f,0,9],[0x27f,0x20,1],[0x27f,0x20,9],[0x7f,0,1]]){
    const exact=control===0x27f,status=exact?sticky:0x20;
    fixture(`execution-${exact?'proven-pc53':'pc24-rounded'}-native24-product-pe${sticky}-${iterations}`,{
      segments:[[0x1000,'d903 d94304 d8c9 dfe0 dd5b08 ddd8 49 75ef 0f0b']],
      registers:[0,iterations,0,0x3000,0x4800,0,0,0],control,initialFp:{status:sticky},stateWitness:true,
      data:page=>{const d=new DataView(page.buffer);d.setUint32(0,0x3f800001,true);d.setUint32(4,0x3f000001,true);},
    },t=>{
      // (1+2^-23)*(2^-1+2^-24) = 2^-1+2^-23+2^-47 exactly.
      // PC53 retains all48 significand bits; PC24 drops the final term.
      t.writeExpected(0x3008,exact?'400000400000e03f':'000000400000e03f');
      const{before,witness}=retained(t,{pc:0x1011,retired:8*iterations,budget:8*iterations,flags:0x46,ecx:0,
        regs:{0:0x3000|status},fp:t.fp({status,opcode:0x5d8,fip:0x100c,fdp:0x3008,
          registers:{6:exact?PRODUCT:roundedProduct,7:LEFT}})});
      assert.equal(t.version(),before+BigInt(iterations));t.result.executionWitness=witness;
      assert.equal(witness.fp_multiply_ops,iterations,'the real Wasm computes every guest product');
      if(exact){
        assert.equal(witness.status_resolver_calls,0,'the proved exact products require no residual/status calculation');
        assert.equal(witness.fp_result_admissions,0,'the propagated range proof removes repeated result admission');
        assert.equal(witness.fp_pending_recipe_writes,0,'proved exact products never create a deferred inexactness recipe');
      }
    });
  }
  for(const c of[
    {name:'maximum-normal-square',a:0x7f7fffff,b:0x7f7fffff,sticky:0x20,
      left:'0000000000ffffff7e40',product:'0000010000fefffffe40',stored:'200000c0ffffef4f'},
    {name:'minimum-normal-square',a:0x00800000,b:0x00800000,sticky:0,
      left:'0000000000000080813f',product:'0000000000000080033f',stored:'0000000000003030'},
    {name:'negative-zero-product',a:0x80000000,b:0x40600000,sticky:0x20,
      left:'00000000000000000080',product:'00000000000000000080',stored:'0000000000000080'},
    {name:'positive-zero-product',a:0x80000000,b:0xc0600000,sticky:0,
      left:'00000000000000000080',product:'00000000000000000000',stored:'0000000000000000'},
  ])fixture(`execution-proven-pc53-native24-${c.name}`,{
    segments:[[0x1000,'d903 d94304 d8c9 dfe0 dd5b08 ddd8 49 75ef 0f0b']],
    registers:[0,1,0,0x3000,0x4800,0,0,0],control:0x27f,initialFp:{status:c.sticky},stateWitness:true,
    data:page=>{const d=new DataView(page.buffer);d.setUint32(0,c.a,true);d.setUint32(4,c.b,true);},
  },t=>{
    t.writeExpected(0x3008,c.stored);
    const{before,witness}=retained(t,{pc:0x1011,retired:8,budget:8,flags:0x46,ecx:0,regs:{0:0x3000|c.sticky},
      fp:t.fp({status:c.sticky,opcode:0x5d8,fip:0x100c,fdp:0x3008,registers:{6:c.product,7:c.left}})});
    assert.equal(t.version(),before+1n);t.result.executionWitness=witness;
    assert.equal(witness.fp_multiply_ops,1);assert.equal(witness.status_resolver_calls,0);
    assert.equal(witness.fp_result_admissions,0);assert.equal(witness.fp_pending_recipe_writes,0);
  });
  fixture('execution-proven-exact-product-replaces-an-older-pending-c1-writer',{
    segments:[[0x1000,'d903 d94304 d9ca d84308 ddd8 d8c9 dfe0 dd5b10 ddd8 ffe7']],
    registers:[0,2,0,0x3000,0x4800,0,0,0x1100],control:0x27f,stateWitness:true,
    initialFp:{status:0x20,tag:0xfffc,registers:{0:'0008000000000080ff3f'}},
    data:page=>{const d=new DataView(page.buffer);d.setUint32(0,0x40600000,true);
      d.setUint32(4,0x40000000,true);d.setUint32(8,0x25000000,true);},
  },t=>{
    // The odd PC53 input plus2^-53 produces a pending C1=1 SUM recipe. Popping
    // that result reveals the two unchanged Native24 inputs; their exact product
    // must clear C1 and cancel the old recipe before FNSTSW can observe it.
    t.writeExpected(0x3010,'0000000000001c40');
    const{before,witness}=retained(t,{pc:0x1100,retired:10,budget:10,regs:{0:0x3820},
      fp:t.fp({status:0x820,opcode:0x5d8,fip:0x1013,fdp:0x3010,registers:{0:TWO,
        6:'0010000000000080ff3f',7:'00000000000000e00140'}})});
    assert.equal(t.version(),before+1n);t.result.executionWitness=witness;
    assert.equal(witness.fp_multiply_ops,1);assert.equal(witness.status_resolver_calls,0,
      'the exact product cancels the prior deferred C1 before its first observation');
    assert.equal(witness.fp_pending_recipe_writes,1,'only the preceding inexact SUM creates a pending recipe');
  });
  fixture('execution-unproven-pc53-product-keeps-exact-inexactness-effects',{
    segments:[[0x1000,'d8c9 dfe0 ffe7']],registers:[0,2,0,0x3000,0x4800,0,0,0x1100],stateWitness:true,
    initialFp:{tag:0xfff0,registers:{0:'0000000002000080ff3f',1:'0000000002000080ff3f'}},
  },t=>{
    // (1+2^-30)^2 = 1+2^-29+2^-60: PC53 rounds away2^-60 and sets PE.
    const{before,witness}=retained(t,{pc:0x1100,retired:3,budget:3,regs:{0:0x20},
      fp:t.fp({status:0x20,tag:0xfff0,opcode:0xc9,fip:0x1000,
        registers:{0:'0000000004000080ff3f',1:'0000000002000080ff3f'}})});
    assert.equal(t.version(),before);t.result.executionWitness=witness;
    assert.equal(witness.status_resolver_calls,1,'unproved precision retains the exact residual oracle');
    assert(witness.fp_result_admissions>0,'unproved result range retains execution-time admission');
  });
}

export function runExecutionCases(fixture) {
  runExecutionFlagsCases(fixture);runExecutionLoadCases(fixture);runExecutionGuardCases(fixture);
  runExecutionMultiplyCases(fixture);runFlagsOracleCases(fixture);
}
