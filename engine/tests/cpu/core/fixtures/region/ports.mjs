import assert from 'node:assert/strict';

const TWO='00000000000000800040',THREE='00000000000000c00040',FIVE='00000000000000a00140';
const NEGATIVE_ZERO='00000000000000000080',FULL80='01000000000000c00040';
const ONE='0000000000000080ff3f',UP_ONE='0008000000000080ff3f';
const history={status:0x220,tag:0xfff0,opcode:0x5a5,fip:0x11223344,fdp:0x55667788,cs:0x33,ds:0x5b,
  registers:{0:TWO,1:THREE,7:NEGATIVE_ZERO}};

function enter(t,pc) {
  new DataView(t.engine.memory.buffer).setUint32(t.context()+32,pc,true);
  t.result.interiorPort={pc,primary:t.result.modules[0].pc,primarySha256:t.result.modules[0].sha256};
}

function check(t,wanted) {
  const actual=t.check(wanted);
  assert.equal(actual.activations.length,1,'the precompiled primary body executes this exact interior suffix');
  assert.equal(t.result.modules.length,1,'no replacement module hides an unsupported interior port');
  return actual;
}

export function runPortCases(fixture) {
  fixture('immutable-ports-preserve-distinct-x86-decodes-inside-another-instruction',{
    // 1000 decodes MOV EAX,0xc3909090; RET. The other reached path
    // decodes its immediate bytes at 1002 as NOP; NOP; RET.
    entry:0x1010,segments:[[0x1000,'b8909090c3c3'],[0x1010,'85c0 74ec ebec']],
    registers:[0xa5a5a5a5,2,0,0x3000,0x4800,0,0,0],
  },t=>{
    enter(t,0x1003);const before=t.version();
    check(t,{pc:0x1004,retired:1,budget:1,fp:t.startingFp});
    enter(t,0x1000);
    check(t,{pc:0x1005,retired:2,budget:1,regs:{0:0xc3909090},fp:t.startingFp});
    assert.equal(t.version(),before,'both distinct decodes read unchanged source bytes');
  });
  fixture('immutable-port-at-prefix-1024-executes-one-instruction-with-bounded-selector-depth',{
    segments:[[0x1000,'90'.repeat(1025)+'c3']],compileInstructions:2048,compileBlocks:1,
  },t=>{
    enter(t,0x1400);const before=t.version();
    check(t,{pc:0x1401,retired:1,budget:1,fp:t.startingFp});
    assert.equal(t.version(),before);
    const depth=t.portDepth();
    assert(depth['guest.run'].branchTables>0,'the sole instruction body contains actual instruction-port branch tables');
    assert(depth['guest.run'].maxDepth<=96,'1026 instruction starts use bounded 64-label chunks plus outer control');
  });
  for(const budget of[0,1,3])fixture(`immutable-port-aliases-and-unexecuted-metadata-budget-${budget}`,{
    // The skipped FLD ST0 aliases the primary's internal ST0/ST1 storage. The
    // actual external inputs remain two distinct values and one empty ghost.
    segments:[[0x1000,'d9c0 90 d8c1 db7b08 0f0b']],initialFp:history,
  },t=>{
    enter(t,0x1002);const before=t.version();
    if(budget===3)t.writeExpected(0x3008,FIVE);
    check(t,{pc:budget===0?0x1002:budget===1?0x1003:0x1008,retired:budget,budget,
      fp:budget<3?t.startingFp:t.fp({status:0x820,tag:0xfff3,opcode:0x37b,fip:0x1005,fdp:0x3008,registers:{0:FIVE}})});
    assert.equal(t.version(),before+(budget===3?1n:0n),'only the executed raw80 store commits');
    assert.equal(t.result.runs[0].helperCalls,budget===3?2:0,'only suffix arithmetic and raw80 store use exact helpers');
  });

  fixture('immutable-port-rejects-internal-gpr-constant-and-collapsed-address-facts',{
    segments:[[0x1000,'31c0 8b1403 01c0 8b3403 01c0 8b3c03 0f0b']],
    registers:[2,2,0,0x3000,0x4800,0,0,0],
    data:page=>page.set(Buffer.from('112233445566778899aabbcc','hex')),
  },t=>{
    enter(t,0x1002);const before=t.version();
    check(t,{pc:0x100f,retired:5,budget:5,regs:{0:8,2:0x66554433,6:0x88776655,7:0xccbbaa99},fp:t.startingFp});
    assert.equal(t.version(),before);
  });

  fixture('immutable-port-preserves-full80-and-external-top-and-pc64',{
    segments:[[0x1000,'d903 90 d84304 db7b08 0f0b']],control:0x37f,
    initialFp:{status:0x1000,tag:0xffcf,registers:{2:FULL80}},
    data:page=>{const d=new DataView(page.buffer);d.setFloat32(0,3,true);d.setFloat32(4,0,true);},
  },t=>{
    enter(t,0x1002);const before=t.version();t.writeExpected(0x3008,FULL80);
    check(t,{pc:0x1009,retired:3,budget:3,
      fp:t.fp({status:0x1800,tag:0xffff,opcode:0x37b,fip:0x1006,fdp:0x3008,registers:{2:FULL80}})});
    assert.equal(t.version(),before+1n);assert.equal(t.result.runs[0].helperCalls,2);
  });

  fixture('immutable-port-observes-external-rounding-and-c1-before-store',{
    segments:[[0x1000,'d903 90 d84304 dfe0 db7b08 0f0b']],control:0xa7f,
    initialFp:{tag:0xfffc,registers:{0:ONE}},
    data:page=>{const d=new DataView(page.buffer);d.setFloat32(0,3,true);d.setUint32(4,0x25000000,true);},
  },t=>{
    enter(t,0x1002);const before=t.version();t.writeExpected(0x3008,UP_ONE);
    check(t,{pc:0x100b,retired:4,budget:4,regs:{0:0x220},
      fp:t.fp({status:0x820,tag:0xffff,opcode:0x37b,fip:0x1008,fdp:0x3008,registers:{0:UP_ONE}})});
    assert.equal(t.version(),before+1n);assert.equal(t.result.runs[0].helperCalls,2);
  });

  fixture('immutable-port-fwait-observes-pending-exception-without-skipped-fp-metadata',{
    segments:[[0x1000,'d903 90 9b d84304 0f0b']],control:0x27e,
    initialFp:{status:1,tag:0xfffc,opcode:0x5a5,fip:0x11223344,fdp:0x55667788,cs:0x33,ds:0x5b,registers:{0:ONE}},
  },t=>{
    enter(t,0x1003);const before=t.version();
    check(t,{pc:0x1003,retired:0,budget:4,work:0,remaining:4,stop:9,fault:[0x8081,0,16],
      fp:t.fp({status:0x8081,tag:0xfffc,opcode:0x5a5,fip:0x11223344,fdp:0x55667788,cs:0x33,ds:0x5b,registers:{0:ONE}})});
    assert.equal(t.version(),before);assert.equal(t.result.runs[0].helperCalls,1);
  });

  for(const codeStore of[false,true])fixture(`immutable-port-${codeStore?'smc-commits-once-before-stale-reentry':'late-fault-publishes-only-the-completed-suffix'}`,{
    segments:[[0x1000,'d9c0 90 d8c1 db3f 0f0b']],initialFp:history,
    registers:[0,2,0,0x3000,0x4800,0,0,codeStore?0x1100:0x6000],
  },t=>{
    enter(t,0x1002);const before=t.version();if(codeStore)t.writeExpected(0x1100,FIVE);
    check(t,{pc:codeStore?0x1007:0x1005,retired:codeStore?3:2,budget:6,
      work:codeStore?3:2,remaining:codeStore?3:4,stop:codeStore?6:3,
      fault:codeStore?[0,0,0]:[0x6000,0x1020a,0xa0e],
      fp:t.fp(codeStore?{status:0x820,tag:0xfff3,opcode:0x33f,fip:0x1005,fdp:0x1100,registers:{0:FIVE}}:
        {status:0x20,tag:0xfff0,opcode:0xc1,fip:0x1003,fdp:0x55667788,ds:0x5b,registers:{0:FIVE}})});
    assert.equal(t.version(),before+(codeStore?1n:0n));assert.equal(t.result.runs[0].helperCalls,2);
    if(codeStore)t.status('core_begin',4);
  });

  fixture('immutable-port-repeat-string-uses-work-quota-without-retiring-the-skipped-prefix',{
    segments:[[0x1000,'b900000000 f3a4 0f0b']],
    registers:[0,3,0,0x3000,0x4800,0,0x3000,0x5000],
    data:page=>page.set([0x11,0x22,0x33]),
  },t=>{
    enter(t,0x1005);const before=t.version();t.writeExpected(0x5000,'1122');
    check(t,{pc:0x1005,retired:0,budget:2,ecx:1,regs:{6:0x3002,7:0x5002},fp:t.startingFp});
    assert.equal(t.version(),before+2n);
    t.writeExpected(0x5002,'33');
    check(t,{pc:0x1007,retired:1,budget:1,ecx:0,regs:{6:0x3003,7:0x5003},fp:t.startingFp});
    assert.equal(t.version(),before+3n);
  });

  fixture('immutable-port-repeat-string-late-fault-keeps-partial-work-without-prefix-retirement',{
    segments:[[0x1000,'b900000000 f3a4 0f0b']],
    registers:[0,3,0,0x3000,0x4800,0,0x3000,0x5fff],
    data:page=>page.set([0x11,0x22,0x33]),
  },t=>{
    enter(t,0x1005);const before=t.version();t.writeExpected(0x5fff,'11');
    check(t,{pc:0x1005,retired:0,budget:3,work:1,remaining:2,ecx:2,regs:{6:0x3001,7:0x6000},
      stop:3,fault:[0x6000,0x10201,14],fp:t.startingFp});
    assert.equal(t.version(),before+1n);
  });
}
