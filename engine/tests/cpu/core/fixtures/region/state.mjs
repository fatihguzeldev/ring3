import assert from 'node:assert/strict';

const ONE='0000000000000080ff3f',ODD_ONE='0008000000000080ff3f';
const SEVEN='00000000000000e00140',FULL80='01000000000000c00040';
const primary=[[0x1000,'d903 85c0 740a eb08'],[0x1010,'d84304 db7b08 0f0b']];

export function runRenamingCase(fixture,checkStructure) {
  fixture('mixed-balanced-stack-loop-preserves-popped-physical-payloads',{
    segments:[[0x1000,'d903 d94304 d9c9 d8c1 ddd8 ddd8 49 75f0 0f0b']],
    stateWitness:Boolean(checkStructure),
  },t=>{
    retained(t,{pc:0x1010,retired:16,budget:16,flags:0x46,ecx:0,
      fp:t.fp({opcode:0x5d8,fip:0x100b,fdp:0x3004,
        registers:{6:'00000000000000a00140',7:'00000000000000800040'}})});
    if(checkStructure){const proof=t.inspect();proof.nativeStateWitness=t.stateWitness();checkStructure(proof);}
  });
}

function secondary(t) {
  new DataView(t.engine.memory.buffer).setUint32(t.context()+32,0x1010,true);
  t.result.secondaryEntry={pc:0x1010,retainedPrimary:t.result.modules[0].pc};
}

function retained(t,wanted) {
  const observed=t.check(wanted);
  assert.equal(observed.activations.length,1,'the retained primary handles the selected numerical entry');
  assert.equal(t.result.modules.length,1,'the external entry is not hidden by compilation of a replacement unit');
}

export function runStateCases(fixture) {
  runRenamingCase(fixture);
  const vector='0000803f000000400000404000008040';
  fixture('direct-packed-xmm-moves-preserve-renamed-dirty-x87-values',{
    segments:[[0x1000,'d903 d94304 d9c9 0f104320 0f114330 d8c1 d95b08 d95b0c 0f0b']],
    data:page=>{
      const d=new DataView(page.buffer);d.setFloat32(0,3,true);d.setFloat32(4,2,true);
      page.set(Buffer.from(vector,'hex'),32);
    },
  },t=>{
    const before=t.version(),simd=t.startingSimd.slice();simd.set(Buffer.from(vector,'hex'),32);
    t.writeExpected(0x3030,vector);t.writeExpected(0x3008,'0000a040');t.writeExpected(0x300c,'00000040');
    retained(t,{pc:0x1017,retired:8,budget:8,simd,
      fp:t.fp({opcode:0x15b,fip:0x1014,fdp:0x300c,
        registers:{6:'00000000000000a00140',7:'00000000000000800040'}})});
    assert.equal(t.result.runs[0].helperCalls,0,'the supported packed moves and admitted x87 instructions stay direct');
    assert.equal(t.version(),before+3n,'the packed store and two scalar FP stores each commit once');
  });
  fixture('dynamic-internal-entry-revalidates-the-target-fp-contract',{
    segments:[[0x1000,'85c0 741c d903 d903 d903 d903 d903 d903 d903 d903 ffe6'],
      [0x1020,'d903 d94304 ddd8 ddd8 eb15'],[0x1040,'90 0f0b']],
    registers:[1,2,0,0x3000,0x4800,0,0x1040,0],
  },t=>{
    // Only the unexecuted direct predecessor proves the target's empty slots. The
    // indirect path instead brings eight occupied slots and must revalidate that fact.
    retained(t,{pc:0x1041,retired:12,budget:13,work:12,remaining:1,stop:4,
      fp:t.fp({tag:0,opcode:0x103,fip:0x1012,fdp:0x3000,
        registers:Object.fromEntries(Array.from({length:8},(_,i)=>[i,'00000000000000c00040']))})});
    assert.equal(t.result.runs[0].helperCalls,0,'entry-contract rejection does not execute an unrequested FP instruction');
  });
  fixture('secondary-entry-does-not-inherit-the-primary-predecessor-gpr-constant',{
    segments:[[0x1000,'bb00300000 eb09'],[0x1010,'8b03 894708 0f0b']],
    registers:[0xa5a5a5a5,2,0,0x4000,0x4800,0,0,0x5000],
  },t=>{
    secondary(t);const before=t.version();t.writeExpected(0x5008,'00000000');
    retained(t,{pc:0x1015,retired:2,budget:2,regs:{0:0},fp:t.startingFp});
    assert.equal(t.version(),before+1n,'the externally addressed store still commits exactly once');
  });
  fixture('secondary-entry-mismatched-gpr-contract-cannot-reuse-a-collapsed-constant-address',{
    segments:[[0x1000,'31c0 eb0c'],[0x1010,'8b1403 01c0 8b3403 01c0 8b3c03 0f0b']],
    registers:[2,2,0,0x3000,0x4800,0,0,0],
    data:page=>page.set(Buffer.from('112233445566778899aabbcc','hex')),
  },t=>{
    // The internal predecessor fixes EAX=0. The external EAX=2 instead selects offsets 2,4,8.
    secondary(t);
    retained(t,{pc:0x101d,retired:5,budget:5,regs:{0:8,2:0x66554433,6:0x88776655,7:0xccbbaa99},
      fp:t.startingFp});
  });
  fixture('numeric-secondary-entry-imports-external-top-instead-of-the-primary-stack-contract',{
    segments:primary,initialFp:{status:0x1000,tag:0xffcf,registers:{2:SEVEN}},
    data:page=>new DataView(page.buffer).setFloat32(0,3,true),
  },t=>{
    secondary(t);t.writeExpected(0x3008,SEVEN);
    retained(t,{pc:0x1016,retired:2,budget:2,
      fp:t.fp({status:0x1800,tag:0xffff,opcode:0x37b,fip:0x1013,fdp:0x3008,registers:{2:SEVEN}})});
  });
  fixture('numeric-secondary-entry-preserves-unrepresentable-full80-and-external-pc64',{
    segments:primary,control:0x37f,initialFp:{tag:0xfffc,registers:{0:FULL80}},
    data:page=>new DataView(page.buffer).setFloat32(0,3,true),
  },t=>{
    secondary(t);t.writeExpected(0x3008,FULL80);
    retained(t,{pc:0x1016,retired:2,budget:2,
      fp:t.fp({status:0x800,opcode:0x37b,fip:0x1013,fdp:0x3008,registers:{0:FULL80}})});
  });
  for(const[name,raw,budget,stop,remaining]of[
    ['zero-significand-nonzero-exponent','00000000000000000040',2,2,0],
    ['finite-exponent-without-integer-bit','0000000000000040ff3f',3,4,1],
  ])fixture(`numeric-secondary-entry-rejects-unsupported80-${name}`,{
    segments:primary,initialFp:{tag:0xfffe,registers:{0:raw}},
  },t=>{
    // Both encodings are unsupported, rather than zero or an exact finite f64.
    // Masked invalid arithmetic produces x87 indefinite; the following raw80 store
    // preserves that literal payload and pop changes TOP only after the write commits.
    secondary(t);const before=t.version(),indefinite='00000000000000c0ffff';
    t.writeExpected(0x3008,indefinite);
    retained(t,{pc:0x1016,retired:2,budget,work:2,stop,remaining,
      fp:t.fp({status:0x801,tag:0xffff,opcode:0x37b,fip:0x1013,fdp:0x3008,
        registers:{0:indefinite}})});
    assert.equal(t.version(),before+1n,'only the completed raw80 store advances the transaction sequence');
    assert.equal(t.result.runs[0].helperCalls,2,'unsupported entry state selects exact arithmetic and store without a speculative guest fault');
  });
  fixture('numeric-secondary-entry-observes-external-directed-rounding-before-arithmetic',{
    segments:[[0x1000,'d903 85c0 740a eb08'],[0x1010,'d84304 dfe0 db7b08 0f0b']],
    control:0xa7f,initialFp:{tag:0xfffc,registers:{0:ONE}},
    data:page=>{const d=new DataView(page.buffer);d.setFloat32(0,3,true);d.setUint32(4,0x25000000,true);},
  },t=>{
    // 1 + 2^-53 rounds upward to 1 + 2^-52. FNSTSW observes C1 before the store clears it.
    secondary(t);t.writeExpected(0x3008,ODD_ONE);
    retained(t,{pc:0x1018,retired:3,budget:3,regs:{0:0x220},
      fp:t.fp({status:0x820,opcode:0x37b,fip:0x1015,fdp:0x3008,registers:{0:ODD_ONE}})});
  });
  fixture('numeric-secondary-entry-pending-exception-precedes-a-later-memory-fault',{
    segments:primary,registers:[0,2,0,0x6000,0x4800,0,0,0],control:0x27e,
    initialFp:{status:1,tag:0xfffc,registers:{0:ONE}},
  },t=>{
    secondary(t);
    retained(t,{pc:0x1010,retired:0,budget:2,work:0,remaining:2,stop:9,fault:[0x8081,0,16],
      fp:t.fp({status:0x8081,tag:0xfffc,registers:{0:ONE}})});
  });

  const cycle=[[0x1000,'d903 85d2 7407 d84304 e202 eb07 d84b04 e2f4 eb00 d95b08 0f0b']];
  for(const[edx,flags,value,stored]of[
    [0,0x46,'00000000000000800340','00008041'],
    [1,2,'00000000000000c00240','00004041'],
  ])fixture(`mixed-irreducible-cycle-${edx===0?'taken':'fallthrough'}-entry-keeps-exact-state`,{
    segments:cycle,registers:[0,3,edx,0x3000,0x4800,0,0,0],
  },t=>{
    t.writeExpected(0x3008,stored);
    retained(t,{pc:0x1017,retired:11,budget:11,flags,ecx:0,
      fp:t.fp({opcode:0x15b,fip:0x1014,fdp:0x3008,registers:{7:value}})});
  });
}
