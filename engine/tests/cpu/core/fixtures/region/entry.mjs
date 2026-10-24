import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';

const hash=bytes=>createHash('sha256').update(bytes).digest('hex');

function select(t,pc) {
  const context=t.context(),id=new DataView(t.engine.memory.buffer).getBigUint64(context+120,true);
  const source=t.bytes(t.api.core_code_ptr()>>>0,t.api.core_code_len()>>>0);
  assert.equal(hash(source),t.result.modules[0].sha256,'the actual selected owner is the original primary module');
  new DataView(t.engine.memory.buffer).setUint32(context+32,pc,true);
  t.result.entryRouting={primary:t.result.modules[0].pc,selected:pc,id:String(id),sha256:hash(source)};
  return()=>{
    assert.equal(new DataView(t.engine.memory.buffer).getBigUint64(context+120,true),id,
      'late entry retains the actual compiled owner identity');
    assert.equal(hash(t.bytes(t.api.core_code_ptr()>>>0,t.api.core_code_len()>>>0)),hash(source),
      'late entry executes the same immutable module bytes');
  };
}

// The old and new engines execute every literal semantic case before the first
// routing requirement is checked. A baseline RED therefore retains both owner
// sizes and both quota prefixes, rather than stopping at the first small owner.
export function runEntryCases(fixture) {
  const checks=[];
  for(const blocks of[8,128])for(const budget of[0,1]){
    const late=0x1000+5*(blocks-1);
    fixture(`entry-routing-${blocks}-block-owner-late-secondary-budget-${budget}`,{
      // Each JMP+0 ends a real prior basic block. Executing any skipped LEA
      // changes EAX, so the whole-GPR oracle independently detects misrouting.
      segments:[[0x1000,'8d4001eb00'.repeat(blocks-1)+'900f0b']],
      compileInstructions:2*blocks,compileBlocks:blocks,stateWitness:true,
      registers:[0x13579bdf,0x23456789,0x3456789a,0x3000,0x4800,0x456789ab,0x56789abc,0x6789abcd],
      initialFlags:0x403,
      initialFp:{status:0x220,tag:0xfff6,opcode:0x5a5,fip:0x11223344,fdp:0x55667788,cs:0x33,ds:0x5b,
        registers:{0:'00000000000000c00040',1:'00000000000000800040',
          6:'01000000000000c00040',7:'00000000000000000080'}},
    },t=>{
      const retained=select(t,late),revision=t.version();
      const run=t.check({pc:late+budget,retired:budget,budget,flags:0x403,fp:t.startingFp});
      assert.equal(run.activations.length,1,'late entry completes its literal prefix in the original activation');
      assert.equal(t.result.modules.length,1,'late entry does not prepare or replace a module');
      assert.equal(run.helperCalls,0);assert.equal(t.version(),revision);retained();
      const counters=t.stateWitness();t.result.entryRouting.counters=counters;
      t.result.semanticComplete=true;
      checks.push(()=>{
        assert.equal(counters.body_entries,1,'secondary entry executes exactly one real instruction body');
        assert.equal(run.preciseCalls,0,
          'zero quota and the complete NOP span both stop before the separate UD2 boundary');
        assert.equal(counters.prefix_quota_queries,0,
          'the hot body contains no per-instruction stopping-prefix quota checks');
        assert.equal(counters.entry_filter_queries,0,
          `${blocks}-block late entry executes no linear per-block pending/INDEX filters`);
        assert.equal(counters.entry_index_compares,0,
          'flat secondary entry does not replace the linear filter with scalar INDEX membership comparisons');
        assert.equal(counters.entry_table_selections,1,
          'one actual INDEX table selection routes the flat owner directly to its final block');
      });
    });
  }

  for(const iterations of[1,9])fixture(`entry-routing-natural-loop-known-backedges-${iterations}`,{
    segments:[[0x1000,'8d4001 e2fb 90 0f0b']],stateWitness:true,
    registers:[0,iterations,0,0x3000,0x4800,0,0,0],
  },t=>{
    const retained=select(t,0x1000),revision=t.version(),budget=2*iterations+1;
    const run=t.check({pc:0x1006,retired:budget,budget,ecx:0,regs:{0:iterations},fp:t.startingFp});
    assert.equal(run.activations.length,1);assert.equal(t.result.modules.length,1);
    assert.equal(run.helperCalls,0);assert.equal(t.version(),revision);retained();
    const counters=t.stateWitness();t.result.entryRouting.counters=counters;
    t.result.semanticComplete=true;
    checks.push(()=>{
      assert.equal(counters.body_entries,1,'natural backedges stay inside the retained instruction body');
      assert.equal(run.preciseCalls,0,'the final complete NOP span stays hot before UD2');
      assert.equal(counters.prefix_quota_queries,0,'admitted loop spans have no per-instruction quota skeleton');
      assert.equal(counters.entry_filter_queries,0,'known natural backedges do not run per-block entry filters');
      assert.equal(counters.entry_index_compares,0,'known natural backedges do not compare dynamic entry membership');
      assert(counters.entry_table_selections<=2,
        'only the initial outer and loop-nested selection may consult INDEX, independently of iteration count');
    });
  });
  for(const check of checks)check();
}

export function runEntryNestedCases(fixture) {
  const checks=[];
  const runCase=(name,options,{entry,pc,budget,eax,ecx=options.registers[1],flags=0x403,tables})=>
    fixture(name,{stateWitness:true,initialFlags:0x403,...options},t=>{
      const retained=select(t,entry),revision=t.version();
      const run=t.check({pc,retired:budget,budget,ecx,flags,regs:{0:eax},fp:t.startingFp});
      assert.equal(run.activations.length,1,'the selected nested suffix remains inside one actual owner activation');
      assert.equal(t.result.modules.length,1);assert.equal(run.helperCalls,0);assert.equal(t.version(),revision);retained();
      const counters=t.stateWitness();t.result.entryRouting.counters=counters;t.result.semanticComplete=true;
      checks.push(()=>{
        assert.equal(counters.body_entries,1);
        assert.equal(counters.entry_filter_queries,0,'nested entry uses no per-block pending/INDEX scan');
        assert.equal(counters.entry_index_compares,0,'nested entry uses no scalar INDEX membership scan');
        assert.equal(counters.entry_table_selections,tables,
          'actual INDEX selections follow enclosing shapes and necessary irreducible dispatches only');
      });
    });

  // The outer loop's header changes EAX by16 and initializes ECX; the inner
  // header changes EAX by32. External entry at100f must skip both headers.
  const nested={segments:[[0x1000,'eb00 8d4010 b901000000 8d4020 eb00 8d4001 e2f6 85d2 75ea 90 0f0b']],
    registers:[5,1,0,0x3000,0x4800,0,0,0]};
  runCase('entry-routing-nested-interior-quota-stops-before-counter-effect',nested,
    {entry:0x100f,pc:0x1012,budget:1,eax:6,tables:3});
  runCase('entry-routing-nested-interior-skips-headers-and-exits-both-loops',nested,
    {entry:0x100f,pc:0x1018,budget:4,eax:6,ecx:0,flags:0x446,tables:3});

  // Two outside edges enter the same SCC at different blocks. LOOP changes
  // only ECX, so the skipped primary TEST must not replace initial DF/CF.
  const irreducible={segments:[[0x1000,'85d2 7407 8d4001 e202 eb07 8d4002 e2f4 eb00 90 0f0b']],
    registers:[0,3,0,0x3000,0x4800,0,0,0]};
  runCase('entry-routing-irreducible-secondary-stops-before-first-loop',irreducible,
    {entry:0x100b,pc:0x100e,budget:1,eax:2,tables:2});
  runCase('entry-routing-irreducible-known-edges-retain-real-scc-dispatch',irreducible,
    {entry:0x100b,pc:0x1013,budget:8,eax:5,ecx:0,tables:4});

  // Blocks are numbered by source PC, while their topological order is
  // 1000→1040→1020→1060. INDEX is a block ID, not a shape ordinal.
  const reordered={segments:[[0x1000,'eb3e'],[0x1020,'8d4004 eb3b'],[0x1040,'8d4002 ebdb'],[0x1060,'90 0f0b']],
    registers:[5,2,0,0x3000,0x4800,0,0,0]};
  runCase('entry-routing-global-id-selects-third-topological-shape',reordered,
    {entry:0x1020,pc:0x1061,budget:3,eax:9,tables:1});
  runCase('entry-routing-global-id-selects-second-topological-shape',reordered,
    {entry:0x1040,pc:0x1061,budget:5,eax:11,tables:1});
  for(const check of checks)check();
}

export function runEntryUnmatchedCases(fixture) {
  const checks=[];
  for(const [pc,label]of[[0x1800,'unowned-code-address'],[0x1002,'middle-of-ud2']])
    fixture(`entry-routing-unmatched-${label}-returns-zero-work`,{
      segments:[[0x1000,'900f0b']],stateWitness:true,initialFlags:0x403,
      registers:[0x13579bdf,0x23456789,0x3456789a,0x3000,0x4800,0x456789ab,0x56789abc,0x6789abcd],
      initialFp:{status:0x220,tag:0xfff6,opcode:0x5a5,fip:0x11223344,fdp:0x55667788,cs:0x33,ds:0x5b,
        registers:{0:'00000000000000c00040',1:'00000000000000800040',
          6:'01000000000000c00040',7:'00000000000000000080'}},
    },t=>{
      const retained=select(t,pc),revision=t.version();
      const context=t.bytes(t.context(),128);new DataView(context.buffer,context.byteOffset,128).setUint32(44,3,true);
      const run=t.check({pc,retired:0,budget:3,work:0,remaining:3,stop:1,flags:0x403,
        fp:t.startingFp,continueNeedCode:false});
      assert.equal(run.activations.length,1,'unmatched entry returns before another activation or preparation');
      assert.equal(t.result.modules.length,1);assert.equal(run.helperCalls,0);assert.equal(t.version(),revision);retained();
      assert.deepEqual(t.bytes(t.context(),128),context,'unmatched routing preserves the complete Context128 except its supplied remaining quota');
      const counters=t.stateWitness();t.result.entryRouting.counters=counters;t.result.semanticComplete=true;
      checks.push(()=>{
        assert.equal(counters.body_entries,1);
        assert.equal(counters.prefix_quota_queries,0,'an unmatched entry never reaches a selected instruction prefix');
        assert.equal(counters.entry_filter_queries,0);assert.equal(counters.entry_index_compares,0);
        assert.equal(counters.entry_table_selections,1,'the real INDEX default takes the one shared unmatched exit');
      });
    });
  for(const check of checks)check();
}
