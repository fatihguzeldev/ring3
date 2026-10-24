import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';

const ROOT=0x1000,TARGET=0x1800,REPETITIONS=128,INSTRUCTIONS=131,FLAGS=0x403;
const code=`d903 ${'d84304 '.repeat(REPETITIONS)}dd5b08 ffe6`;
const registers=[0x13579bdf,2,0x2468ace0,0x3000,0x4800,0x89abcdef,TARGET,0x76543210];
const initialFp={status:0,tag:0xffff,opcode:0x5a5,fip:0x11223344,fdp:0x55667788,cs:0x33,ds:0x5b,
  // All eight tags are Empty. Their payloads remain architectural ghosts;
  // unsupported encodings in the seven untouched slots must never be admitted.
  registers:{0:'01000000000000c00040',1:'00000000000000000080',
    2:'0000000000000040ff3f',3:'00000000000000c0ff7f',
    4:'7856341200000080ff3f',5:'ffffffffffffffffffff',
    6:'efcdab8967452301807f',7:'ffffffffffffffffffff'}};
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');

export function runMixedFpValueCases(fixture) {
  const checks=[];
  // These integer kernels execute inline. They must preserve an already dirty
  // FP value even when their conservative scalar def/use contract is opaque.
  // Expected flags follow the existing exact kernel policy, including defined
  // zero for IMUL's otherwise undefined arithmetic flag bits.
  for(const scalar of[
    {name:'shl',bytes:'c1e001',eax:0x26af37be,flags:0x406},
    {name:'shr',bytes:'c1e801',eax:0x09abcdef,flags:0x403},
    {name:'imul',bytes:'6bc003',eax:0x3a06d39d,flags:0x402},
    {name:'bswap',bytes:'0fc8',eax:0xdf9b5713,flags:FLAGS},
  ])fixture(`mixed-fp-dirty-value-survives-inline-${scalar.name}`,{
    segments:[[ROOT,`d903 ${scalar.bytes} d95b04 ffe6`]],registers,
    initialFlags:FLAGS,initialFp,control:0x27f,stateWitness:true,
    data:page=>page.set(Buffer.from('00006040aabbccdd','hex')),
  },t=>{
    const encoded=t.bytes(t.api.core_code_ptr()>>>0,t.api.core_code_len()>>>0);
    const expectedContext=t.bytes(t.context(),128),context=new DataView(expectedContext.buffer);
    const identity=context.getBigUint64(120,true),revision=t.version();
    const storePc=ROOT+2+scalar.bytes.length/2;
    t.writeExpected(0x3004,'00006040');
    const run=t.check({pc:TARGET,retired:4,budget:4,regs:{0:scalar.eax},flags:scalar.flags,
      fp:t.fp({status:0,tag:0xffff,opcode:0x15b,fip:storePc,fdp:0x3004,
        registers:{7:'00000000000000e00040'}})});
    for(const[offset,value]of[[0,scalar.eax],[32,TARGET],[36,scalar.flags],[40,4],[44,0],[112,0]])
      context.setUint32(offset,value,true);
    assert.deepEqual(t.bytes(t.context(),128),expectedContext,'full scalar/FP barrier Context128');
    assert.equal(t.version(),revision+1n,'only the final FSTP performs a write');
    assert.equal(new DataView(t.engine.memory.buffer).getBigUint64(t.context()+120,true),identity);
    assert.deepEqual(t.bytes(t.api.core_code_ptr()>>>0,t.api.core_code_len()>>>0),encoded);
    t.result.semanticComplete=true;
    t.result.barrier={scalar:scalar.name,storedFloat32:'00006040',storePc,
      owner:{id:String(identity),sha256:hash(encoded)}};
    assert.equal(run.activations.length,1);assert.equal(run.helperCalls,0);assert.equal(run.preciseCalls,0);
    assert.equal(t.result.modules.length,1,'the real inline integer kernel preserves the same FP owner');
  });
  fixture('mixed-fp-real-canonical-control-change-publishes-the-dirty-prefix',{
    segments:[[ROOT,'d903 d96b08 d95b04 ffe6']],registers,initialFlags:FLAGS,
    initialFp,control:0x27f,stateWitness:true,
    data:page=>page.set(Buffer.from('00006040aabbccdd7f00','hex')),
  },t=>{
    const expectedContext=t.bytes(t.context(),128),context=new DataView(expectedContext.buffer);
    const encoded=t.bytes(t.api.core_code_ptr()>>>0,t.api.core_code_len()>>>0),revision=t.version();
    const wantedFp=t.fp({status:0,tag:0xffff,opcode:0x15b,fip:0x1005,fdp:0x3004,
      registers:{7:'00000000000000e00040'}});
    new DataView(wantedFp.buffer).setUint16(16,0x7f,true);
    t.writeExpected(0x3004,'00006040');
    const run=t.check({pc:TARGET,retired:4,budget:4,flags:FLAGS,fp:wantedFp});
    for(const[offset,value]of[[32,TARGET],[40,4],[44,0],[112,0]])context.setUint32(offset,value,true);
    assert.deepEqual(t.bytes(t.context(),128),expectedContext,'canonical CW mutation preserves the full scalar lease');
    assert.equal(t.version(),revision+1n);assert.deepEqual(t.bytes(t.api.core_code_ptr()>>>0,t.api.core_code_len()>>>0),encoded);
    t.result.semanticComplete=true;
    assert.equal(run.helperCalls,1,'FLDCW remains an actual canonical FP-changing callback');
    assert.equal(run.preciseCalls,0);assert.equal(run.activations.length,1);assert.equal(t.result.modules.length,1);
  });
  fixture('mixed-fp-inline-scalar-then-store-fault-retains-the-completed-dirty-value',{
    segments:[[ROOT,'d903 c1e001 d91f ffe6']],registers:[...registers.slice(0,7),0x5000],
    initialFlags:FLAGS,initialFp,control:0x27f,missing:[0x5000],stateWitness:true,
    data:page=>page.set(Buffer.from('00006040aabbccdd','hex')),
  },t=>{
    const expectedContext=t.bytes(t.context(),128),context=new DataView(expectedContext.buffer);
    const revision=t.version();
    const run=t.check({pc:0x1005,retired:2,budget:4,work:2,remaining:2,stop:3,
      regs:{0:0x26af37be},flags:0x406,fault:[0x5000,0x10204,0x40e],
      fp:t.fp({status:0x3800,tag:0x3fff,opcode:0x103,fip:ROOT,fdp:0x3000,
        registers:{7:'00000000000000e00040'}})});
    for(const[offset,value]of[[0,0x26af37be],[32,0x1005],[36,0x406],[40,2],[44,2],
      [48,0x5000],[52,0x10204],[112,0],[116,0x40e]])context.setUint32(offset,value,true);
    assert.deepEqual(t.bytes(t.context(),128),expectedContext,'fault exposes exactly the completed FLD and SHL');
    assert.equal(t.version(),revision,'the faulting store neither writes nor pops the loaded value');
    t.result.semanticComplete=true;
    assert.equal(run.helperCalls,0);assert.equal(run.preciseCalls,1);assert.equal(t.result.modules.length,1);
  });
  for(const single of[true,false])fixture(`mixed-fp-values-${single?'pc24':'pc53'}-128-ordered-rounding-points`,{
    segments:[[ROOT,code]],registers,initialFlags:FLAGS,initialFp,
    control:single?0x7f:0x27f,stateWitness:true,
    data:page=>page.set(Buffer.from('0000c03f000080338877665544332211aabbccddeeff00aa','hex')),
  },t=>{
    const encoded=t.bytes(t.api.core_code_ptr()>>>0,t.api.core_code_len()>>>0);
    const expectedContext=t.bytes(t.context(),128),context=new DataView(expectedContext.buffer);
    const identity=context.getBigUint64(120,true),revision=t.version();
    // At PC24 each addition is a separate RN tie: the even value1.5 survives
    // all128 operations with sticky PE. At PC53 all sums are exact, producing
    // 1.5+128*2^-24=1.5+2^-17. Combining the additions would change PC24.
    t.writeExpected(0x3008,single?'000000000000f83f':'000000000800f83f');
    const run=t.check({pc:TARGET,retired:INSTRUCTIONS,budget:INSTRUCTIONS,flags:FLAGS,
      fp:t.fp({status:single?0x20:0,tag:0xffff,opcode:0x55b,fip:0x1182,fdp:0x3008,
        registers:{7:single?'00000000000000c0ff3f':'00000000004000c0ff3f'}})});
    for(const[offset,value]of[[32,TARGET],[40,INSTRUCTIONS],[44,0],[112,0]])
      context.setUint32(offset,value,true);
    assert.deepEqual(t.bytes(t.context(),128),expectedContext,
      'all128 Context bytes retain GPRs, FLAGS, source authority and owner identity');
    assert.equal(t.version(),revision+1n,'only the final8-byte payload commits a write');
    assert.equal(new DataView(t.engine.memory.buffer).getBigUint64(t.context()+120,true),identity);
    assert.deepEqual(t.bytes(t.api.core_code_ptr()>>>0,t.api.core_code_len()>>>0),encoded,
      'ordered rounding and publication retain the actual immutable owner bytes');
    const witness=t.stateWitness();
    t.result.semanticComplete=true;
    t.result.valueWitness=witness;
    t.result.owner={id:String(identity),sha256:hash(encoded),bytes:encoded.length};
    t.result.rounding={precision:single?24:53,operations:REPETITIONS,finalStorePc:0x1182};
    checks.push(()=>{
      assert.equal(run.activations.length,1,'the full admitted chain uses one actual body activation');
      assert.equal(t.result.modules.length,1,'no replacement module supplies an alternate value path');
      assert.equal(run.helperCalls,0,'the admitted native chain executes no canonical numeric callback');
      assert.equal(run.preciseCalls,0,'the complete admitted span executes no precise suffix');
      assert.equal(witness.fp_add_ops,REPETITIONS,'all128 guest additions remain actual Wasm operations');
      assert.equal(witness.fp_single_rounds,single?REPETITIONS:0,
        'every PC24 operation retains its own binary32 rounding point');
      assert(witness.fp_valid_writes<=4,
        `VALID writes belong to fixed ingress/recovery, not129 producers: ${witness.fp_valid_writes}`);
      assert(witness.fp_loaded_writes<=4,
        `LOADED writes belong to fixed ingress/recovery, not129 producers: ${witness.fp_loaded_writes}`);
      assert(witness.fp_token_writes<=3,
        `packed payload certificates belong only to ingress, recovery and commit: ${witness.fp_token_writes}`);
    });
  });
  // Both modes pass complete Context/FP/SIMD/YMM/RAM/version oracles before
  // either engine can reach the representation-only historical baseline RED.
  for(const check of checks)check();
}
