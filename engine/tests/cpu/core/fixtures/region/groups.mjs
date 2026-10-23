import assert from 'node:assert/strict';

const ONE='0000000000000080ff3f';

function retained(t,wanted) {
  const run=t.check(wanted);
  assert.equal(run.activations.length,1,'one generated body owns the exact memory prefix');
  assert.equal(t.result.modules.length,1,'no replacement masks a group admission failure');
  return run;
}

export function runGroupCases(fixture) {
  fixture('access-group-short-quota-tail-keeps-the-single-ordinary-exact-access-path',{
    segments:[[0x1000,'8b03 894308 8b5304 89530c 0f0b']],stateWitness:true,
    data:page=>{const d=new DataView(page.buffer);d.setUint32(0,0x12345678,true);d.setUint32(4,0x89abcdef,true);},
  },t=>{
    const before=t.version();t.writeExpected(0x3008,'78563412');t.writeExpected(0x300c,'efcdab89');
    retained(t,{pc:0x100b,retired:4,budget:4,regs:{0:0x12345678,2:0x89abcdef},fp:t.startingFp});
    assert.equal(t.version(),before+2n,'the precise tail preserves both logical store commits');
    const witness=t.stateWitness();
    assert.equal(witness.group_acquisitions,0,'a short quota tail uses the ordinary exact memory path');
    assert.equal(witness.page_translations,1,'the precise tail reuses its ordinary page translation');
  });

  fixture('access-group-fast-full-block-reuses-one-real-admission-across-four-accesses',{
    segments:[[0x1000,'8b03 894308 8b5304 89530c ffe6']],stateWitness:true,
    registers:[0,2,0,0x3000,0x4800,0,0x1100,0],
    data:page=>{const d=new DataView(page.buffer);d.setUint32(0,0x12345678,true);d.setUint32(4,0x89abcdef,true);},
  },t=>{
    const before=t.version();t.writeExpected(0x3008,'78563412');t.writeExpected(0x300c,'efcdab89');
    retained(t,{pc:0x1100,retired:5,budget:5,regs:{0:0x12345678,2:0x89abcdef},fp:t.startingFp});
    assert.equal(t.version(),before+2n,'fast group reuse preserves both logical store commits');
    const witness=t.stateWitness();
    assert.equal(witness.page_translations,0,'admitted grouped accesses bypass the ordinary per-access page translator');
    assert.equal(witness.group_acquisitions,1,'four actual accesses share exactly one dynamic group admission');
    const proof=t.groupProof();assert(proof,'the executed acquisition is a defined generated function');
    assert.deepEqual(proof.parameters,Array(9).fill(0x7f));assert.deepEqual(proof.results,Array(3).fill(0x7f));
    assert.deepEqual(proof.contextLoads,[56,60,64,68,72,76],
      'the actual helper checks mapping, permissions, backing, executable aliases and COW authority');
    assert.equal(proof.stores,0,'future envelope admission performs no guest or authority write');
    assert.equal(proof.calls,0,'future envelope admission invokes no faulting canonical helper');
    t.result.groupAdmissionProof=proof;
  });

  fixture('access-group-future-unmapped-page-cannot-fault-before-the-first-valid-load',{
    segments:[[0x1000,'8b03 8b5304 0f0b']],missing:[0x4000],
    registers:[0xa5a5a5a5,2,0x55667788,0x3ffc,0x4800,0,0,0],
    data:page=>new DataView(page.buffer).setUint32(4092,0x12345678,true),
  },t=>{
    const before=t.version();
    retained(t,{pc:0x1002,retired:1,budget:4,work:1,remaining:3,stop:3,
      regs:{0:0x12345678},fault:[0x4000,0x10104,14],fp:t.startingFp});
    assert.equal(t.version(),before,'a rejected future envelope invents no transaction');
  });

  fixture('access-group-future-denied-write-preserves-the-prior-valid-read',{
    segments:[[0x1000,'8b03 894308 0f0b']],
    registers:[0xa5a5a5a5,2,0,0x3000,0x4800,0,0,0],
    data:page=>new DataView(page.buffer).setUint32(0,0x12345678,true),
  },t=>{
    t.status('protect',0,0x3000,1,1);const before=t.version();
    retained(t,{pc:0x1002,retired:1,budget:4,work:1,remaining:3,stop:3,
      regs:{0:0x12345678},fault:[0x3008,0x20204,14],fp:t.startingFp});
    assert.equal(t.version(),before,'union write permission failure never commits the later store');
  });

  fixture('access-group-immutable-port-disables-acquisition-and-reuse-under-false-gpr-facts',{
    segments:[[0x1000,'31db 8b03 8b5304 0f0b']],stateWitness:true,
    data:page=>{const d=new DataView(page.buffer);d.setUint32(0,0x12345678,true);d.setUint32(4,0x89abcdef,true);},
  },t=>{
    new DataView(t.engine.memory.buffer).setUint32(t.context()+32,0x1002,true);
    const before=t.version();
    retained(t,{pc:0x1007,retired:2,budget:2,regs:{0:0x12345678,2:0x89abcdef},fp:t.startingFp});
    const witness=t.stateWitness();
    assert.equal(witness.group_acquisitions,0,'contradictory skipped GPR facts cannot acquire a descriptor');
    assert.equal(witness.page_translations,1,'the precise port validates its actual address through the ordinary page path');
    assert.equal(t.version(),before);
  });

  fixture('access-group-acquisition-waits-for-the-current-fp-exception-admission',{
    segments:[[0x1000,'d803 d84b04 0f0b']],stateWitness:true,control:0x27e,
    registers:[0,2,0,0x6000,0x4800,0,0,0],initialFp:{status:1,tag:0xfffc,registers:{0:ONE}},
  },t=>{
    const before=t.version();
    retained(t,{pc:0x1000,retired:0,budget:4,work:0,remaining:4,stop:9,fault:[0x8081,0,16],
      fp:t.fp({status:0x8081,tag:0xfffc,registers:{0:ONE}})});
    const witness=t.stateWitness();
    assert.equal(witness.group_acquisitions,0,'a failed FP admission cannot speculatively acquire future addresses');
    assert.equal(witness.page_translations,0);assert.equal(t.version(),before);
  });

  fixture('access-group-store-sequence-exhaustion-keeps-only-the-first-committed-write',{
    segments:[[0x1000,'8903 894304 0f0b']],registers:[0x12345678,2,0,0x3000,0x4800,0,0,0],
  },t=>{
    const maximum=(1n<<64n)-1n;
    // Diagnostic seam: the context exposes this exact transaction authority pointer.
    new DataView(t.engine.memory.buffer).setBigUint64(t.word(t.context(),80),maximum-1n,true);
    t.writeExpected(0x3000,'78563412');
    retained(t,{pc:0x1002,retired:1,budget:4,work:1,remaining:3,stop:8,fp:t.startingFp});
    assert.equal(t.version(),maximum,'group admission never reserves or merges logical write sequence numbers');
  });

  fixture('access-group-code-store-stops-before-its-admitted-later-store',{
    segments:[[0x1000,'8903 894304 0f0b']],registers:[0x12345678,2,0,0x1100,0x4800,0,0,0],
  },t=>{
    const before=t.version();t.writeExpected(0x1100,'78563412');
    retained(t,{pc:0x1002,retired:1,budget:4,work:1,remaining:3,stop:6,fp:t.startingFp});
    assert.equal(t.version(),before+1n);t.status('core_begin',4);
  });
}
