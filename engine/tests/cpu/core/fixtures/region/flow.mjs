import assert from 'node:assert/strict';
import {view} from '../../../../../../sdk/runtime/engine.js';

const THREE='00000000000000c00040',TWO='00000000000000800040';

// Lexical order is G1000, A1080, root1100, while execution starts at1100.
// The late preheader gives A two native inputs. A's FXCH self-loop must reach
// a fixed point instead of alternating its provisional [native,unknown] facts.
export function runFlowConvergenceCase(fixture) {
  fixture('nonlexical-late-preheader-and-fxch-self-loop-converge-and-execute',{
    entry:0x1100,
    segments:[[0x1000,'d903 e979000000'],[0x1080,'d9c9 ebfc'],[0x1100,'d94304 e9f8feffff']],
    initialFp:{tag:0xffff,registers:{
      0:'00000000000000000080',1:'0000000000000040ff3f',2:'01000000000000c00040',
    }},
  },t=>{
    const revision=t.version();
    for(const [budget,retired,physical6,physical7] of [[5,5,TWO,THREE],[2,7,THREE,TWO]]){
      const expected=t.bytes(t.context(),128),d=view(expected);
      d.setUint32(32,0x1082,true);d.setUint32(40,retired,true);d.setUint32(44,0,true);
      const run=t.check({pc:0x1082,retired,budget,
        fp:t.fp({status:0x3000,tag:0x0fff,opcode:0x1c9,fip:0x1080,fdp:0x3000,
          registers:{6:physical6,7:physical7}})});
      assert.deepEqual(t.bytes(t.context(),128),expected,'all private CPU state and authority bytes match the literal completed prefix');
      assert.equal(t.version(),revision,'loads and register exchanges commit no guest memory transaction');
      assert.equal(run.helperCalls,0,'both exact loaded values remain on the native x87 path');
      assert.equal(run.activations.length,1,'the compiled owner itself executes the bounded loop without a preparation handoff');
    }
  });
}
