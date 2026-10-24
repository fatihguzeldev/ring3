import assert from 'node:assert/strict';

const THREE='00000000000000c00040';
const A=0x11223344,B=0x55667788,C=0x99aabbcc;
const mixed='50 51 8b0424 89542408 d903 dd5c2410 5f 5e 8b0c24 8b542408 ffe5';
const registers=[A,B,C,0x3000,0x4800,0x1100,0x77777777,0x88888888];

function stamp(t,address) {
  const context=t.context(),mapping=t.word(t.word(context,56)+(address>>>12)*4);
  assert(mapping>0&&mapping<=t.word(context,68),'the independent page stamp names a mapped physical slot');
  const pointer=t.word(context,84)+(mapping-1)*16+8;
  return new DataView(t.engine.memory.buffer).getBigUint64(pointer,true);
}

function checked(t,wanted,{writes=0,helpers=0,precise=0,version=t.version()}={}) {
  const source=t.bytes(t.api.core_code_ptr()>>>0,t.api.core_code_len()>>>0);
  const id=new DataView(t.engine.memory.buffer).getBigUint64(t.context()+120,true);
  const run=t.check(wanted);
  assert.equal(run.activations.length,1,'the original generated owner executes the exact access prefix');
  assert.equal(t.result.modules.length,1,'repreparation cannot conceal an access epoch error');
  assert.equal(run.helperCalls,helpers,'only the specified canonical numeric boundary executes');
  assert.equal(run.preciseCalls,precise,'only a short exact prefix enters the shared precise adapter');
  assert.equal(new DataView(t.engine.memory.buffer).getBigUint64(t.context()+120,true),id);
  assert.deepEqual(t.bytes(t.api.core_code_ptr()>>>0,t.api.core_code_len()>>>0),source,
    'exact access prefixes do not synthesize another source owner');
  assert.equal(t.version(),version+BigInt(writes),'each completed logical store has its exact transaction sequence');
  t.result.semanticComplete=true;
  return t.stateWitness();
}

export function runAccessEpochCases(fixture,{mechanism=true}={}) {
  const proofs=[];
  const remember=(t,witness,wanted)=>{
    t.result.accessEpochWitness=witness;
    proofs.push({result:t.result,witness,wanted,hasPrivate:t.word(t.context(),76)!==0});
  };

  fixture('access-epoch-affine-stack-shares-authority-with-scalar-and-native-fp-stores',{
    segments:[[0x1000,mixed]],registers,stateWitness:true,initialFlags:0x403,
  },t=>{
    const before=t.version(),dataStamp=stamp(t,0x3000),codeStamp=stamp(t,0x1000);
    t.writeExpected(0x47fc,'44332211');t.writeExpected(0x47f8,'88776655');
    t.writeExpected(0x4800,'ccbbaa99');t.writeExpected(0x4808,'0000000000000840');
    const witness=checked(t,{pc:0x1100,retired:11,budget:11,flags:0x403,
      ecx:C,regs:{0:B,2:0,6:A,7:B},
      fp:t.fp({opcode:0x55c,fip:0x100b,fdp:0x4808,registers:{7:THREE}})},
      {writes:4,version:before});
    assert.equal(stamp(t,0x4000),before+4n,'the physical stack page receives the last exact completed store sequence');
    assert.equal(stamp(t,0x3000),dataStamp,'read-only floating input has no invented content transaction');
    assert.equal(stamp(t,0x1000),codeStamp,'ordinary nonexec writes leave source authority untouched');
    remember(t,witness,{groups:1,exec:1,private:1});
  });

  fixture('access-epoch-explicit-esp-arithmetic-and-implicit-stack-use-one-affine-family',{
    segments:[[0x1000,'50 83ec08 894c2404 83c408 5a 8b7424f8 ffe5']],registers,
    stateWitness:true,initialFlags:0x403,
  },t=>{
    const before=t.version();t.writeExpected(0x47fc,'44332211');t.writeExpected(0x47f8,'88776655');
    const witness=checked(t,{pc:0x1100,retired:7,budget:7,flags:0x406,
      ecx:B,regs:{2:A,6:B},fp:t.startingFp},{writes:2,version:before});
    assert.equal(stamp(t,0x4000),before+2n);
    remember(t,witness,{groups:1,exec:1,private:1});
  });

  fixture('access-epoch-quota-tail-keeps-two-exact-stack-stores-without-future-admission',{
    segments:[[0x1000,mixed]],registers,stateWitness:true,initialFlags:0x403,
  },t=>{
    const before=t.version();t.writeExpected(0x47fc,'44332211');t.writeExpected(0x47f8,'88776655');
    const witness=checked(t,{pc:0x1002,retired:2,budget:2,flags:0x403,
      ecx:B,regs:{4:0x47f8},fp:t.startingFp},{writes:2,precise:1,version:before});
    assert.equal(stamp(t,0x4000),before+2n);
    remember(t,witness,{groups:0,exec:0,private:0});
  });

  fixture('access-epoch-rejected-stack-envelope-does-not-raise-a-future-unmapped-fault',{
    segments:[[0x1000,'50 51 ffe5']],registers:[A,B,C,0x3000,0x4004,0x1100,0,0],
    missing:[0x3000],stateWitness:true,
  },t=>{
    const before=t.version();t.writeExpected(0x4000,'44332211');
    const witness=checked(t,{pc:0x1001,retired:1,budget:3,work:1,remaining:2,stop:3,
      ecx:B,regs:{4:0x4000},fault:[0x3ffc,0x10204,14],fp:t.startingFp},
      {writes:1,version:before});
    assert.equal(stamp(t,0x4000),before+1n);
    remember(t,witness,{groups:1,exec:1,private:1,canonicalWrites:1});
  });

  fixture('access-epoch-future-denied-stack-write-keeps-the-prior-valid-read',{
    segments:[[0x1000,'8b0424 51 ffe5']],registers,stateWitness:true,
  },t=>{
    t.status('protect',0,0x4000,1,1);const before=t.version(),pageStamp=stamp(t,0x4000);
    const witness=checked(t,{pc:0x1003,retired:1,budget:3,work:1,remaining:2,stop:3,
      ecx:B,regs:{0:0},fault:[0x47fc,0x20204,14],fp:t.startingFp},{version:before});
    assert.equal(stamp(t,0x4000),pageStamp);
    remember(t,witness,{groups:1,exec:0,private:0,canonicalWrites:1});
  });

  fixture('access-epoch-shared-write-authority-does-not-reserve-or-merge-overflow-sequences',{
    segments:[[0x1000,'50 51 ffe5']],registers,stateWitness:true,
  },t=>{
    const maximum=(1n<<64n)-1n;
    new DataView(t.engine.memory.buffer).setBigUint64(t.word(t.context(),80),maximum-1n,true);
    t.writeExpected(0x47fc,'44332211');
    const witness=checked(t,{pc:0x1001,retired:1,budget:3,work:1,remaining:2,stop:8,
      ecx:B,regs:{4:0x47fc},fp:t.startingFp},{writes:1,version:maximum-1n});
    assert.equal(stamp(t,0x4000),maximum);
    remember(t,witness,{groups:1,exec:1,private:1,canonicalWrites:1});
  });

  fixture('access-epoch-executable-stack-store-stops-at-its-first-committed-prefix',{
    segments:[[0x1000,'50 51 ffe5']],registers:[A,B,C,0x3000,0x1184,0x1100,0,0],stateWitness:true,
  },t=>{
    const before=t.version();t.writeExpected(0x1180,'44332211');
    const witness=checked(t,{pc:0x1001,retired:1,budget:3,work:1,remaining:2,stop:6,
      ecx:B,regs:{4:0x1180},fp:t.startingFp},{writes:1,version:before});
    assert.equal(stamp(t,0x1000),before+1n);t.status('core_begin',4);
    // the failed nonexec proof and ordinary exact store each inspect executable
    // authority; no COW table read is needed after either executable rejection.
    remember(t,witness,{groups:1,exec:2,private:0,canonicalWrites:1});
  });

  fixture('access-epoch-canonical-numeric-boundary-invalidates-earlier-write-authority',{
    segments:[[0x1000,'8903 894304 d96b10 894308 89430c ffe5']],registers,stateWitness:true,
    data:page=>new DataView(page.buffer).setUint16(16,0x37f,true),
  },t=>{
    const before=t.version();for(const address of[0x3000,0x3004,0x3008,0x300c])t.writeExpected(address,'44332211');
    const fp=t.startingFp.slice();new DataView(fp.buffer,fp.byteOffset,fp.byteLength).setUint16(16,0x37f,true);
    const witness=checked(t,{pc:0x1100,retired:6,budget:6,ecx:B,fp},
      {writes:4,helpers:1,version:before});
    assert.equal(stamp(t,0x3000),before+4n);
    // The planned callback ends a span. Its offset-zero successor obtains one
    // fresh shared write proof after the canonical authority barrier.
    remember(t,witness,{groups:2,exec:2,private:2});
  });

  // all independent full-state, RAM and transaction oracles finish before the
  // mechanism check, so the pinned baseline can fail only after semantics pass.
  for(const{result,witness,wanted,hasPrivate}of proofs){
    result.accessEpochMechanismChecked=mechanism;
    if(!mechanism)continue;
    assert.equal(witness.canonical_reads,wanted.canonicalReads??0,`${result.name}: only the exact expected canonical read imports execute`);
    assert.equal(witness.canonical_writes,wanted.canonicalWrites??0,`${result.name}: admitted stores cannot substitute canonical writes`);
    assert.equal(witness.group_acquisitions,wanted.groups,`${result.name}: actual first-access epoch admissions`);
    assert.equal(witness.executable_authority_reads,wanted.exec,`${result.name}: actual physical executable-alias authority reads`);
    assert.equal(witness.private_authority_reads,hasPrivate?wanted.private:0,`${result.name}: actual virtual COW authority reads`);
    assert.equal(witness.executable_context_reads,wanted.exec,`${result.name}: no hidden repeated executable-table base loads`);
    // the actual null COW-table pointer remains a required one-load observation.
    assert.equal(witness.private_context_reads,wanted.private,`${result.name}: no hidden repeated COW-table base loads`);
  }
}
