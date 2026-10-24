import {linkedField,fpPoolState} from '../pool-state.mjs';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {Engine,KEY,record,view} from '../../../../../../sdk/runtime/engine.js';
import {CodeCache} from '../../../../../../sdk/runtime/cache.js';
import {resolverExport,forceNativeInspection} from '../link.mjs';

const hex=bytes=>Buffer.from(bytes).toString('hex');
const unhex=text=>Uint8Array.from(Buffer.from(text,'hex'));
const PE=0x20,C1=0x200;
const ONE='0000000000000080ff3f',ODD_ONE='0008000000000080ff3f';
const HALF_ULP='0000000000000080ca3f',TWO_ULPS='0010000000000080ff3f';
const NEGATIVE_ZERO='00000000000000000080';

// This oracle uses integer fractions and nearest-even division, never host FP arithmetic.
// It is intentionally limited to the normal finite numbers authored below.
function fraction(raw) {
  const d=new DataView(unhex(raw).buffer),significand=d.getBigUint64(0,true),word=d.getUint16(8,true);
  const negative=(word&0x8000)!==0,exponent=(word&0x7fff)-16383-63;
  if(significand===0n)return {n:0n,d:1n,negative};
  assert((word&0x7fff)>0&&(word&0x7fff)<0x7fff&&significand>>63n===1n,'authored finite normal raw80');
  return exponent>=0?{n:(negative?-significand:significand)<<BigInt(exponent),d:1n,negative}
    :{n:negative?-significand:significand,d:1n<<BigInt(-exponent),negative};
}

function exact(operation,left,right) {
  if(operation==='reverse-subtract')return exact('subtract',right,left);
  if(operation==='reverse-divide')return exact('divide',right,left);
  if(operation==='add'||operation==='subtract') {
    const n=left.n*right.d+(operation==='add'?right.n:-right.n)*left.d;
    return {n,d:left.d*right.d,negative:n<0n||(n===0n&&left.negative&&(operation==='add'?right.negative:!right.negative))};
  }
  if(operation==='multiply')return {n:left.n*right.n,d:left.d*right.d,negative:left.negative!==right.negative};
  assert.equal(operation,'divide');assert.notEqual(right.n,0n);
  const n=left.n*right.d*(right.n<0n?-1n:1n);
  return {n,d:left.d*(right.n<0n?-right.n:right.n),negative:left.negative!==right.negative};
}

function rounded(value,precision=53) {
  const negative=value.n<0n||(value.n===0n&&value.negative),n=value.n<0n?-value.n:value.n;
  if(n===0n)return {raw:negative?NEGATIVE_ZERO:'00000000000000000000',bits:negative?1n<<63n:0n,
    precision:false,c1:false,value:{n:0n,d:1n,negative}};
  let exponent=n.toString(2).length-value.d.toString(2).length;
  if(exponent>=0?n<value.d<<BigInt(exponent):n<<BigInt(-exponent)<value.d)exponent--;
  const shift=precision-1-exponent;
  const numerator=shift>=0?n<<BigInt(shift):n,denominator=shift>=0?value.d:value.d<<BigInt(-shift);
  let significand=numerator/denominator;
  const remainder=numerator%denominator;
  const c1=remainder*2n>denominator||(remainder*2n===denominator&&(significand&1n)!==0n);
  if(c1)significand++;
  if(significand===1n<<BigInt(precision)){significand>>=1n;exponent++;}
  assert(exponent>-1023&&exponent<1024,'authored normal binary64 output');
  const output=new Uint8Array(10),d=new DataView(output.buffer);
  d.setBigUint64(0,significand<<BigInt(64-precision),true);
  d.setUint16(8,(negative?0x8000:0)|(exponent+16383),true);
  const raw=hex(output),bits=(negative?1n<<63n:0n)|BigInt(exponent+1023)<<52n|
    (precision===53?significand-(1n<<52n):0n);
  return {raw,bits,precision:remainder!==0n,c1,value:fraction(raw)};
}

function calculate(operation,left,right,status=PE) {
  const result=rounded(exact(operation,fraction(left),fraction(right)));
  return {...result,status:(status&~C1)|(result.precision?PE:0)|(result.c1?C1:0)};
}

function rawInteger(integer) {
  return rounded({n:BigInt(integer),d:1n,negative:integer<0},64).raw;
}

function memoryBits(bits,width=8) {
  const output=new Uint8Array(width),d=new DataView(output.buffer);
  if(width===8)d.setBigUint64(0,bits,true);else d.setUint32(0,Number(bits),true);
  return hex(output);
}

const INITIAL={status:PE,tag:0xfff0,registers:{0:ODD_ONE,1:HALF_ULP}};
function environment(t,result,{fip=0x1000,opcode=0xc1,status=result.status,tag=0xfff0,fdp=0,registers={}}={}) {
  return t.fp({status,tag,opcode,fip,fdp,registers:{0:result.raw,...registers}});
}

export function runStatusCases(fixture) {
  const first=calculate('add',ODD_ONE,HALF_ULP,0),second=calculate('add',first.raw,HALF_ULP,first.status);
  assert.deepEqual([first.raw,first.status,second.raw,second.status],[TWO_ULPS,0x220,TWO_ULPS,0x20],
    'independent nearest-even tie literals anchor the rational oracle');
  for(const prefix of[1,2])fixture(`status-first-precision-and-latest-add-prefix-${prefix}`,{
    segments:[[0x1000,'d8c1 d8c1 0f0b']],initialFp:{...INITIAL,status:0},
  },t=>{
    t.check({pc:0x1000+prefix*2,retired:prefix,budget:prefix,
      fp:environment(t,prefix===1?first:second,{fip:0x1000+(prefix-1)*2})});
    if(prefix===1){
      t.status('core_import',0);t.prepare(0x1002);
      t.check({pc:0x1004,retired:2,budget:1,fp:environment(t,second,{fip:0x1002})});
    }
  });

  const productLeft=rounded({n:(1n<<27n)+1n,d:1n<<27n},64).raw;
  const productRight=rounded({n:(1n<<27n)+3n,d:1n<<27n},64).raw;
  const arithmetic=[
    ['add-tie-up','add','d8c1',0xc1,ODD_ONE,HALF_ULP,true],
    ['subtract-negative-tie-down','subtract','d8e1',0xe1,HALF_ULP,ODD_ONE,false],
    ['reverse-subtract-negative-tie-down','reverse-subtract','d8e9',0xe9,ODD_ONE,HALF_ULP,false],
    ['product-round-up','multiply','d8c9',0xc9,productLeft,productRight,true],
    ['product-round-down','multiply','d8c9',0xc9,ODD_ONE,ODD_ONE,false],
    ['quotient-round-up','divide','d8f1',0xf1,ONE,rawInteger(10),true],
    ['quotient-round-down','divide','d8f1',0xf1,ONE,rawInteger(3),false],
    ['negative-quotient-round-up','divide','d8f1',0xf1,rawInteger(-1),rawInteger(10),true],
    ['reverse-quotient-round-up','reverse-divide','d8f9',0xf9,rawInteger(10),ONE,true],
  ];
  for(const[name,operation,code,opcode,left,right,c1]of arithmetic) {
    const wanted=calculate(operation,left,right);
    assert.equal(wanted.c1,c1,`${name}: independent rounding-direction oracle`);
    fixture(`status-sticky-${name}`,{
      segments:[[0x1000,`${code} dfe0 0f0b`]],registers:[0xa5c30000,2,0,0x3000,0x4800,0,0,0],
      initialFp:{status:PE,tag:0xfff0,registers:{0:left,1:right}},
    },t=>t.check({pc:0x1004,retired:2,budget:2,regs:{0:(0xa5c30000|wanted.status)>>>0},
      fp:environment(t,wanted,{opcode})}));
  }

  const pending=calculate('add',ODD_ONE,HALF_ULP);
  fixture('status-successful-store-replaces-c1-and-preserves-popped-raw80',{
    segments:[[0x1000,'d8c1 dd5b08 dfe0 0f0b']],initialFp:INITIAL,
  },t=>{
    t.writeExpected(0x3008,memoryBits(pending.bits));
    t.check({pc:0x1007,retired:3,budget:3,regs:{0:0x820},
      fp:environment(t,pending,{status:0x820,tag:0xfff3,opcode:0x55b,fip:0x1002,fdp:0x3008})});
  });
  fixture('status-register-pop-preserves-c1-after-the-result-slot-becomes-empty',{
    segments:[[0x1000,'d8c1 ddd8 dfe0 0f0b']],initialFp:INITIAL,
  },t=>t.check({pc:0x1006,retired:3,budget:3,regs:{0:0xa20},
    fp:environment(t,pending,{status:0xa20,tag:0xfff3,opcode:0x5d8,fip:0x1002})}));

  for(const denied of[false,true])fixture(`status-faulting-store-preserves-pending-c1-${denied?'denied':'unmapped'}`,{
    segments:[[0x1000,`d8c1 dd1d${denied?'08300000':'00600000'} 0f0b`]],initialFp:INITIAL,
  },t=>{
    if(denied){t.status('protect',0,0x3000,1,1);t.prepare(0x1000);}
    t.check({pc:0x1002,retired:1,budget:4,work:1,remaining:3,stop:3,
      fault:[denied?0x3008:0x6000,denied?0x20208:0x10208,0x80e],fp:environment(t,pending)});
  });
  fixture('status-code-store-clears-c1-only-after-the-store-commits',{
    segments:[[0x1000,'d8c1 dd1d00110000 0f0b']],initialFp:INITIAL,
  },t=>{
    t.writeExpected(0x1100,memoryBits(pending.bits));
    t.check({pc:0x1008,retired:2,budget:4,work:2,remaining:2,stop:6,
      fp:environment(t,pending,{status:0x820,tag:0xfff3,opcode:0x51d,fip:0x1002,fdp:0x1100})});
    t.status('core_begin',4);
  });
  fixture('status-store-sequence-overflow-publishes-only-the-completed-arithmetic',{
    segments:[[0x1000,'d8c1 dd5b08 0f0b']],initialFp:INITIAL,
  },t=>{
    const maximum=(1n<<64n)-1n;
    // The exported context explicitly provides this transaction pointer; this is a diagnostic seam.
    new DataView(t.engine.memory.buffer).setBigUint64(t.word(t.context(),80),maximum,true);
    t.check({pc:0x1002,retired:1,budget:4,work:1,remaining:3,stop:8,fp:environment(t,pending)});
    assert.equal(t.version(),maximum,'the rejected store leaves the exact saturated transaction sequence');
  });
  fixture('status-successful-sign-change-cancels-the-earlier-rounding-recipe',{
    segments:[[0x1000,'d8c1 d9e0 dfe0 0f0b']],initialFp:INITIAL,
  },t=>{
    const negative=pending.raw.slice(0,16)+'ffbf';
    t.check({pc:0x1006,retired:3,budget:3,regs:{0:PE},
      fp:environment(t,{...pending,raw:negative},{status:PE,opcode:0x1e0,fip:0x1002})});
  });
  fixture('status-exact-negative-zero-product-clears-c1-with-sticky-precision',{
    segments:[[0x1000,'d8c9 dfe0 0f0b']],
    initialFp:{status:0x220,tag:0xfff1,registers:{0:NEGATIVE_ZERO,1:rawInteger(2)}},
  },t=>t.check({pc:0x1004,retired:2,budget:2,regs:{0:PE},
    fp:environment(t,{raw:NEGATIVE_ZERO,status:PE},{tag:0xfff1,opcode:0xc9})}));
}

export function runLinkedStatusCases(artifact,report) {
  const productLeft=rounded({n:(1n<<27n)+1n,d:1n<<27n},64).raw;
  const productRight=rounded({n:(1n<<27n)+3n,d:1n<<27n},64).raw;
  for(const[operation,code,opcode,left,right,kind]of[
    ['add','d8c1',0xc1,ODD_ONE,HALF_ULP,1],
    ['multiply','d8c9',0xc9,productLeft,productRight,2],
    ['divide','d8f1',0xf1,ONE,rawInteger(10),3],
  ])runLinkedStatusCase(artifact,report,{operation,code,opcode,left,right,kind});
}

function runLinkedStatusCase(artifact,report,{operation,code,opcode,left,right,kind}) {
  const name=`linked-pending-${operation}-crosses-integer-bridge-and-is-consumed-by-fnstsw`;
  const result={name,complete:false,controls:[],modules:[],probes:[],numericCalls:0};report.cases.push(result);
  const instance=new WebAssembly.Instance(artifact.module,{ring3_observability:{now_ns:()=>0n}});
  const raw=Object.fromEntries(Object.entries(instance.exports).filter(([key])=>key.startsWith('ring3_abi_v1_'))
    .map(([key,value])=>[key.slice(13),value]));
  const memory=instance.exports.memory;
  const bytes=(pointer,length)=>new Uint8Array(memory.buffer).slice(pointer,pointer+length);
  const word=(pointer,offset=0)=>new DataView(memory.buffer).getUint32(pointer+offset,true);
  const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
  const registers=[0xa5c30000,2,0x1100,0x3000,0x4800,0,0x1040,0x1080];
  const wanted=calculate(operation,left,right);
  assert.equal(wanted.status,0x220,'each linked recipe has a discriminating rounded-up result');
  let cache,engine,opened=false,active=false,initialFp,initialBank,initialContext,initialVersion;
  const units=new Map(),global=local=>linkedField(cache,memory,local);
  const bank=()=>bytes(word(cache.context,92),100);
  const version=()=>new DataView(memory.buffer).getBigUint64(word(cache.context,80),true);
  const status=(operation,wanted,...args)=>{
    const actual=raw[operation](...args)>>>0;result.controls.push({operation,wanted,actual});
    assert.equal(actual,wanted,`${name}: ${operation}`);
  };
  const api=Object.freeze({...raw,
    core_numeric:(...args)=>{result.numericCalls++;return raw.core_numeric(...args);},
    ...(typeof raw.core_numeric_owned==='function'?{core_numeric_owned:(...args)=>{result.numericCalls++;return raw.core_numeric_owned(...args);}}:{}),
    core_chain_cold:()=>assert.fail('all four linked status units are retained before execution'),
    [resolverExport(raw)]:(context,pc,selectionRetired)=>{
      const index=result.probes.length,observed=index===2;
      assert(index<3,'the authored four-unit chain has exactly three internal probes');
      assert.equal(context,cache.context);assert.equal(pc,[0x1040,0x1080,0x10c0][index]);
      const wantedRegisters=registers.slice();wantedRegisters[5]=index===0?0:7;
      if(observed)wantedRegisters[0]=(registers[0]|wanted.status)>>>0;
      assert.deepEqual(Array.from({length:8},(_,i)=>global(i+2)>>>0),wantedRegisters,'all live linked registers');
      assert.deepEqual([global(10),global(11),global(12),global(13)],[pc,2,6-index*2,2+index*2],
        'PC, all FLAGS, quota and completed prefix stay exact across every link');
      assert.deepEqual([global(84),global(85),global(86)],[1,cache.continuationCore?1:3,1],
        'only the computed result needs a cached native pool payload; its unchanged input remains authoritative');
      if(cache.continuationCore){
        const input=fpPoolState(memory,word(context,92)).records[1],literal=unhex(right),d=view(literal);
        assert.deepEqual([input.low,input.high,input.valid],[d.getBigUint64(0,true),d.getUint16(8,true),0],
          'the unchanged second operand retains its exact opaque raw80 payload without per-producer VALID publication');
      }
      assert.deepEqual([global(87),global(88),global(89),global(90),global(91),global(92),global(93)],
        [0x27f,observed?wanted.status:PE,0xfff0,0,0x1000,0,opcode],
        'FNSTSW resolves status without changing the arithmetic metadata');
      assert.equal(global(100),observed?0:kind,
        observed?'FNSTSW consumes the recipe before an integer-only successor':'the actual recipe crosses the integer-only bridge');
      assert.equal(global(76),numberFromRaw(wanted.raw),'the exact rounded value remains in the live numerical state');
      if(!observed)assert.deepEqual([global(101),global(102),global(103)],
        [numberFromRaw(left),numberFromRaw(right),numberFromRaw(wanted.raw)],'the complete admitted recipe operands cross the link');
      assert.equal(global(99),initialVersion);assert.equal(version(),initialVersion);
      assert.deepEqual(bank(),initialBank,'neither ordinary links nor FNSTSW pack the raw80 bank');
      assert.deepEqual(bytes(context,48),initialContext,'private linked work leaves the canonical scalar snapshot untouched');
      assert.deepEqual(bytes(engine.base+4236,128),initialFp,'the public FP state is still unpublished');
      const returned=raw[resolverExport(raw)](context,pc,selectionRetired)>>>0;
      assert.equal(returned,units.get(pc).slot,'the actual native owner probe chooses the retained successor');
      result.probes.push({pc,returned,retired:global(13),remaining:global(12),pending:global(100),status:global(88)});
      return returned;
    },
  });
  try {
    engine=new Engine(api,memory,artifact.sha256);status('open',0,12,...KEY);opened=true;
    engine.base=raw.arena_ptr()>>>0;
    const codePage=new Uint8Array(4096),dataPage=new Uint8Array(4096);
    const segments=[[0x1000,`${code} ffe6`],[0x1040,'8d6d07 ffe7'],
      [0x1080,'dfe0 ff631c'],[0x10c0,'8d6d0b ffe2'],[0x1100,'0f0b']];
    for(const[pc,encoded]of segments)codePage.set(unhex(encoded.replaceAll(' ','')),pc-0x1000);
    view(dataPage).setUint32(28,0x10c0,true);
    for(const[address,page]of[[0x1000,codePage],[0x3000,dataPage],[0x4000,new Uint8Array(4096)]]) {
      status('map',0,address,1,address===0x1000?7:3);engine.request(page);status('upload',0,address,4096);
    }
    const state=record('R3ST',56),s=view(state);
    registers.forEach((value,index)=>s.setUint32(16+4*index,value,true));
    s.setUint32(48,0x1000,true);s.setUint32(52,2,true);
    new Uint8Array(memory.buffer,engine.base,56).set(state);
    const fp=new Uint8Array(memory.buffer,engine.base+4236,128),f=view(fp);
    f.setUint16(16,0x27f,true);f.setUint16(18,PE,true);f.setUint16(20,0xfff0,true);
    f.setUint16(32,0x1b,true);f.setUint16(34,0x23,true);
    fp.set(unhex(left),40);fp.set(unhex(right),50);initialFp=fp.slice();
    const initialSimd=bytes(engine.base+4364,160),initialYmm=bytes(engine.base+4612,144);
    cache=new CodeCache(engine,{budget:8,instructions:64,blocks:8,chain:true});
    forceNativeInspection(cache,result);
    assert.equal(cache.linkedCore,true,'the fixture requires actual typed compiled linking');
    for(const pc of[0x1000,0x1040,0x1080,0x10c0]) {
      units.set(pc,cache.entry(pc));
      const encoded=bytes(raw.core_code_ptr()>>>0,raw.core_code_len()>>>0);
      result.modules.push({pc,sha256:hash(encoded),imports:WebAssembly.Module.imports(new WebAssembly.Module(encoded))});
    }
    const first=cache.entry(0x1000);
    status('core_begin',0);active=true;status('core_end',0);active=false;
    initialBank=bank();initialContext=bytes(cache.context,48);initialVersion=version();
    const backing=word(cache.context,64),expectedRam=bytes(backing,word(cache.context,68)*4096);
    const executed=cache.execute(first);
    assert.deepEqual([executed.coreReason,executed.pc,executed.retired,executed.work,executed.regions],
      [2,0x1100,8,8,4],'the exact authored chain finishes before its sentinel');
    assert.equal(executed.chainedColdRuns,0);assert.equal(result.numericCalls,0);assert.equal(result.probes.length,3);
    const wantedRegisters=registers.slice();wantedRegisters[0]=(registers[0]|wanted.status)>>>0;wantedRegisters[5]=18;
    assert.deepEqual(Array.from({length:8},(_,index)=>word(cache.context,index*4)),wantedRegisters,'all terminal registers');
    assert.deepEqual([word(cache.context,32),word(cache.context,36),word(cache.context,40),word(cache.context,44)],
      [0x1100,2,8,0],'all terminal scalar state');
    assert.deepEqual([word(cache.context,48),word(cache.context,52),word(cache.context,116)],[0,0,0],'no hidden fault');
    const wantedFp=initialFp.slice(),wantedView=view(wantedFp);
    wantedView.setUint16(18,wanted.status,true);wantedView.setUint16(22,opcode,true);wantedView.setUint32(24,0x1000,true);
    wantedFp.set(unhex(wanted.raw),40);
    assert.deepEqual(bytes(engine.base+4236,128),wantedFp,'the complete FP bank matches the independent fraction oracle');
    assert.deepEqual(bytes(engine.base+4364,160),initialSimd);assert.deepEqual(bytes(engine.base+4612,144),initialYmm);
    assert.deepEqual(bytes(backing,expectedRam.length),expectedRam,'the entire physical backing is unchanged');
    assert.equal(version(),initialVersion,'read-only status work invents no memory transaction');
    const wantedState=state.slice(),w=view(wantedState);
    wantedRegisters.forEach((value,index)=>w.setUint32(16+4*index,value,true));w.setUint32(48,0x1100,true);
    assert.deepEqual(bytes(engine.base,56),wantedState,'the complete public scalar state is published at the terminal boundary');
    const stats=cache.stats();assert.equal(stats.runs,1);assert.equal(stats.admissions,4);
    result.codeSegments=segments;result.stats=stats;result.final={pc:executed.pc,retired:8,status:wanted.status,
      fpHex:hex(wantedFp),ramSha256:hash(expectedRam)};result.complete=true;
  }catch(error){result.failure={message:String(error),stack:error.stack};throw error;}
  finally{if(active||cache?.open)raw.core_end();if(opened)status('close',0);}
}

function numberFromRaw(raw) {
  const value=rounded(fraction(raw));
  assert.equal(value.precision,false,'only exactly binary64-representable values enter linked recipe probes');
  const bytes=new Uint8Array(8),d=new DataView(bytes.buffer);d.setBigUint64(0,value.bits,true);
  return d.getFloat64(0,true);
}
