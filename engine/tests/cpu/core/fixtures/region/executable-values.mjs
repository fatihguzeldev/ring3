import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readGeneratedModule} from './structure.mjs';

const ROOT=0x1000,TARGET=0x1800,REPETITIONS=128,INSTRUCTIONS=131;
const flags=0x403;
const registers=[0xa5a55a5a,0xffffffc0,0x11223344,0x3000,0x4800,0x13579bdf,TARGET,0x2468ace0];
const chain=operation=>`89c8 ${operation.repeat(REPETITIONS)} 89c2 ffe6`;
const initialFp={status:0x1220,tag:0xff2f,opcode:0x5a5,fip:0x11223344,fdp:0x55667788,cs:0x33,ds:0x5b,
  registers:{0:'01000000000000c00040',1:'00000000000000000080',2:'0000000000000040ff3f',
    3:'00000000000000c0ff7f',4:'000000000000008000c0',5:'7856341200000080ff3f',
    6:'00000000000000000000',7:'ffffffffffffffffffff'}};
const hash=source=>createHash('sha256').update(source).digest('hex');
const source=t=>t.bytes(t.api.core_code_ptr()>>>0,t.api.core_code_len()>>>0);
const identity=t=>new DataView(t.engine.memory.buffer).getBigUint64(t.context()+120,true);

function scalarStructure(bytes) {
  const module=readGeneratedModule(bytes),index=[...module.names].find(([,name])=>name==='guest.run')?.[0];
  assert(Number.isInteger(index)&&index>=module.imports.length,'the actual architectural body is defined');
  const {ops}=module.functions[index-module.imports.length];
  const at=(index,op,local)=>ops[index]?.op===op&&(local===undefined||ops[index].local===local);
  const additions=ops.flatMap((node,i)=>node.op===0x6a?[i]:[]);
  const dagAdditions=additions.filter(i=>at(i-2,0x20)&&ops[i-2].local>=177&&
    at(i-1,0x20)&&ops[i-1].local>=177&&at(i+1,0x21)&&ops[i+1].local>=177);
  // The old LEA lowerer starts with the literal displacement, adds actual EAX,
  // and assigns the effective-address scratch. This is an operator pattern,
  // not a byte search or an inference from a function/descriptor name.
  const oldLeaAdditions=additions.filter(i=>at(i-2,0x41)&&ops[i-2].value===1&&at(i-1,0x20,2)&&at(i+1,0x21,17));
  const prefixQuota=ops.flatMap((node,i)=>node.op===0x4d&&at(i-4,0x20,12)&&at(i-3,0x41)&&
    at(i-2,0x20,131)&&at(i-1,0x6b)?[node.offset]:[]);
  const prefixTables=ops.flatMap((node,i)=>{
    if(node.op!==14)return[];
    const outer=at(i-3,0x20,131)&&at(i-2,0x41)&&ops[i-2].value===6&&at(i-1,0x76);
    const inner=at(i-10,0x20,131)&&at(i-9,0x41)&&at(i-8,0x4b)&&at(i-7,4)&&
      at(i-6,0x20,131)&&at(i-5,0x41)&&at(i-4,0x6b)&&at(i-3,5)&&at(i-2,0x41)&&at(i-1,11);
    return outer||inner?[node.offset]:[];
  });
  const spanQuota=ops.flatMap((node,i)=>node.op===0x49&&at(i-2,0x20,12)&&at(i-1,0x41)&&
    ops[i-1].value===INSTRUCTIONS?[node.offset]:[]);
  return{body:index,operators:ops.length,bodyBytes:module.functions[index-module.imports.length].raw.length,
    additions:additions.length,dagAdditions:dagAdditions.map(i=>ops[i].offset),
    oldLeaAdditions:oldLeaAdditions.map(i=>ops[i].offset),prefixQuota,prefixTables,spanQuota,
    preciseImport:module.imports.findIndex(item=>item.module==='ring3'&&item.name==='precise')};
}

function retain(t,body) {
  const encoded=source(t),id=identity(t),revision=t.version();
  body({encoded,id,revision});
  assert.equal(identity(t),id,'precise ingress keeps the same retained source owner');
  assert.deepEqual(source(t),encoded,'exact prefixes do not compile a replacement owner');
  assert.equal(t.result.modules.length,1,'all authored entries use the one actual module');
  assert.equal(t.version(),revision,'pure scalar execution never commits a memory write');
  t.result.semanticComplete=true;
  t.result.retained={id:String(id),sha256:hash(encoded),bytes:encoded.length};
}

function literal(t,wanted) {
  const expected=t.bytes(t.context(),128),view=new DataView(expected.buffer);
  const gpr=t.registers.slice();gpr[1]=wanted.ecx??t.registers[1];
  for(const[index,value]of Object.entries(wanted.regs??{}))gpr[Number(index)]=value;
  gpr.forEach((value,index)=>view.setUint32(index*4,value>>>0,true));
  const fault=wanted.fault??[0,0,0];
  for(const[offset,value]of[[32,wanted.pc],[36,wanted.flags],[40,wanted.retired],
    [44,wanted.remaining??0],[48,fault[0]],[52,fault[1]],[112,0],[116,fault[2]]])
    view.setUint32(offset,value>>>0,true);
  const run=t.check(wanted);
  assert.deepEqual(t.bytes(t.context(),128),expected,'all128 Context bytes, including the retained lease identity and authority');
  run.contextSha256=hash(expected);return run;
}

function noPrefixScaffolding(proof) {
  assert.equal(proof.prefixQuota.length,0,'the hot body has no per-instruction exact-quota comparisons');
  assert.equal(proof.prefixTables.length,0,'the hot body has no arbitrary instruction-prefix selector tables');
  assert.equal(proof.spanQuota.length,1,'one whole-span admission tests the actual131-instruction quota');
  assert(proof.preciseImport>=0,'unadmitted prefixes use the actual CPU precise adapter');
}

export function runExecutableValueCases(fixture) {
  const checks=[];
  fixture('executable-values-fold128-lea-chain-to-one-live-expression',{
    segments:[[ROOT,chain('8d4001')]],registers,initialFlags:flags,initialFp,
  },t=>retain(t,({encoded})=>{
    const run=literal(t,{pc:TARGET,retired:INSTRUCTIONS,budget:INSTRUCTIONS,flags,
      regs:{0:0x40,2:0x40},fp:t.startingFp});
    assert.equal(run.activations.length,1);assert.equal(run.helperCalls,0);
    const proof=scalarStructure(encoded);t.result.executableStructure=proof;
    checks.push(()=>{
      assert.equal(proof.dagAdditions.length,1,'only the demanded ECX+128 expression is lowered');
      assert.equal(proof.oldLeaAdditions.length,0,'the128 old EAX+1 instruction bodies are absent');
      assert(proof.additions<=8,'fixed bootstrap/retirement arithmetic does not scale with128 LEAs');
      assert.equal(run.preciseCalls,0,'a fully admitted scalar span does not execute the precise adapter');
      noPrefixScaffolding(proof);
    });
  }));

  fixture('executable-values-all131-exact-prefixes-retain-the-owner',{
    segments:[[ROOT,chain('8d4001')]],registers,initialFlags:flags,initialFp,
  },t=>retain(t,()=>{
    const runs=[literal(t,{pc:ROOT,retired:0,budget:0,flags,fp:t.startingFp})];
    for(let completed=1;completed<=INSTRUCTIONS;completed++) {
      const lea=Math.min(REPETITIONS,completed-1),eax=(registers[1]+lea)>>>0;
      const pc=completed===INSTRUCTIONS?TARGET:completed===130?0x1184:
        completed===129?0x1182:0x1002+3*(completed-1);
      runs.push(literal(t,{pc,retired:completed,budget:1,flags,
        regs:{0:eax,...(completed>=130?{2:eax}:{})},fp:t.startingFp}));
    }
    assert(runs.every(run=>run.helperCalls===0&&run.activations.length===1));
    checks.push(()=>{
      assert.equal(runs[0].preciseCalls,0,'zero quota executes no precise callback');
      assert(runs.slice(1).every(run=>run.preciseCalls===1),
        'each positive short/interior quota executes exactly one real precise callback');
    });
  }));

  fixture('executable-values-interior-skips-predicted-mov-and-prefix',{
    segments:[[ROOT,chain('8d4001')]],registers:[0x7ffffffe,...registers.slice(1)],initialFlags:flags,initialFp,
  },t=>retain(t,()=>{
    // The64 preceding LEAs and the MOV from ECX have never executed. Import
    // actual EAX, retain contradictory ECX, and execute only the64-increment suffix.
    new DataView(t.engine.memory.buffer).setUint32(t.context()+32,0x10c2,true);
    const run=literal(t,{pc:TARGET,retired:66,budget:66,flags,
      regs:{0:0x8000003e,2:0x8000003e},fp:t.startingFp});
    assert.equal(run.activations.length,1);assert.equal(run.helperCalls,0);
    checks.push(()=>assert.equal(run.preciseCalls,1,'arbitrary interior state uses the exact native suffix'));
  }));

  fixture('executable-values-final-add-keeps-original-flag-operands',{
    segments:[[ROOT,chain('83c001')]],registers:[registers[0],0x7fffffc0,...registers.slice(2)],
    initialFlags:flags,initialFp,
  },t=>retain(t,({encoded})=>{
    // Last x86 ADD is8000003f+1=80000040: SF+AF, no OF/CF/PF; DF survives.
    const run=literal(t,{pc:TARGET,retired:INSTRUCTIONS,budget:INSTRUCTIONS,flags:0x492,
      regs:{0:0x80000040,2:0x80000040},fp:t.startingFp});
    assert.equal(run.activations.length,1);assert.equal(run.helperCalls,0);
    const proof=scalarStructure(encoded);t.result.executableStructure=proof;
    checks.push(()=>{
      assert.equal(proof.dagAdditions.length,2,'the last original left operand and result are the only live ADD expressions');
      assert(proof.additions<=9,'FLAGS recovery does not require128 arithmetic instruction bodies');
      assert.equal(run.preciseCalls,0);noPrefixScaffolding(proof);
    });
  }));

  fixture('executable-values-parallel-inputs-before-gpr-overwrites',{
    segments:[[ROOT,'89c8 89d1 89c2 ffe6']],registers,initialFlags:flags,initialFp,
  },t=>retain(t,()=>{
    const run=literal(t,{pc:TARGET,retired:4,budget:4,flags,ecx:registers[2],
      regs:{0:registers[1],2:registers[1]},fp:t.startingFp});
    assert.equal(run.activations.length,1);assert.equal(run.helperCalls,0);
    checks.push(()=>assert.equal(run.preciseCalls,0));
  }));

  fixture('executable-values-publish-live-pure-roots-before-late-load-fault',{
    segments:[[ROOT,`89c8 ${'8d4001'.repeat(REPETITIONS)} 89c2 8b07 ffe6`]],
    registers:[...registers.slice(0,7),0x6000],initialFlags:flags,initialFp,
  },t=>retain(t,()=>{
    const run=literal(t,{pc:0x1184,retired:130,budget:132,work:130,remaining:2,stop:3,flags,
      regs:{0:0x40,2:0x40},fault:[0x6000,0x10104,14],fp:t.startingFp});
    assert.equal(run.activations.length,1);assert.equal(run.helperCalls,0);
    checks.push(()=>assert.equal(run.preciseCalls,0,'a full admitted span faults at its ordered load after recovering pure roots'));
  }));

  // Every old-engine literal case completes before the first representation
  // assertion, so the retained baseline RED identifies the missing mechanism.
  for(const check of checks)check();
}
