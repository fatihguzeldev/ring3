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
