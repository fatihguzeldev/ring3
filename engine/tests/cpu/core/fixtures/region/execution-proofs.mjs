import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';

const ONE_AND_HALF='00000000000000c0ff3f',THREE='00000000000000c00040';
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');

export function runExecutionProofCases(fixture) {
  fixture('execution-proof-tokens-carry-native24-and-environment-across-three-blocks',{
    // FLD[EBX] (2 bytes), FLD[EBX+4] (3), JMP (2):1007+19=1020.
    // D8C9 is FMUL ST0,ST1;1024+1c=1040. No loop or alternate entry
    // manufactures an input proof: both values are actual earlier producers.
    segments:[[0x1000,'d903 d94304 eb19'],[0x1020,'d8c9 eb1c'],[0x1040,'d95b08 ffe6']],
    registers:[0x13579bdf,2,0x2468ace0,0x3000,0x4800,0x89abcdef,0x1800,0x76543210],
    control:0x27f,stateWitness:true,
    initialFp:{opcode:0x5a5,fip:0x11223344,fdp:0x55667788,cs:0x33,ds:0x5b,
      registers:{0:'01000000000000c00040',1:'00000000000000000080',
        2:'0000000000000040ff3f',3:'00000000000000c0ff7f',
        4:'7856341200000080ff3f',5:'ffffffffffffffffffff'}},
    data:page=>page.set(Buffer.from('0000c03f00000040','hex')),
  },t=>{
    const encoded=t.bytes(t.api.core_code_ptr()>>>0,t.api.core_code_len()>>>0);
    const before=t.bytes(t.context(),128),context=new DataView(before.buffer),revision=t.version();
    const identity=context.getBigUint64(120,true);
    t.writeExpected(0x3008,'00004040');
    const run=t.check({pc:0x1800,retired:7,budget:7,
      fp:t.fp({status:0x3800,tag:0x3fff,opcode:0x15b,fip:0x1040,fdp:0x3008,
        registers:{6:THREE,7:ONE_AND_HALF}})});
    for(const[offset,value]of[[32,0x1800],[40,7],[44,0],[112,0]])context.setUint32(offset,value,true);
    assert.deepEqual(t.bytes(t.context(),128),before,'all128 Context bytes and the retained source lease');
    assert.equal(run.activations.length,1,'the three actual blocks remain inside one retained body activation');
    assert.equal(t.result.modules.length,1,'no hidden module replaces the producer-to-consumer CFG');
    assert.equal(t.version(),revision+1n,'only the final float32 payload commits a write');
    assert.equal(new DataView(t.engine.memory.buffer).getBigUint64(t.context()+120,true),identity);
    assert.deepEqual(t.bytes(t.api.core_code_ptr()>>>0,t.api.core_code_len()>>>0),encoded,
      'the complete actual source module is immutable');
    const witness=t.stateWitness();
    t.result.semanticComplete=true;
    t.result.proofWitness=witness;
    t.result.owner={id:String(identity),sha256:hash(encoded),bytes:encoded.length};
    // Architectural state, all raw80 ghosts/tags/metadata, complete RAM and
    // version oracles above precede the representation-only baseline RED.
    assert.equal(run.helperCalls,0,'both native producers and the exact product stay in generated execution');
    assert.equal(witness.fp_multiply_ops,1,'the actual Wasm performs the one guest multiplication');
    assert.equal(witness.fp_value_precision_admissions,0,
      'the same-value Native24 producer proofs remove both heavy raw-precision admissions in the next block');
    if(typeof t.api.core_continuation_format==='function') {
      assert.equal(witness.fp_environment_queries,0,'typed block arguments need no generic environment-token queries');
      assert.equal(witness.control_admissions,1,'one actual control admission covers all three typed blocks');
    } else assert.equal(witness.fp_environment_queries,1,
      'the dominating imported environment token survives both actual internal CFG edges');
    if(typeof t.api.core_precise==='function')assert.equal(run.preciseCalls,0,
      'a fully admitted producer/consumer CFG never enters the exact-prefix adapter');
  });
}
