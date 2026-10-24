import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readGeneratedModule} from './structure.mjs';

const ROOT=0x1000,TARGET=0x1800,FLAGS=0x403;
const registers=[0x7fffffc0,3,0x55667788,0x3000,0x4800,0x13579bdf,TARGET,0x1820];
const ghosts={0:'01000000000000c00040',1:'00000000000000000080',
  2:'0000000000000040ff3f',3:'00000000000000c0ff7f',4:'7856341200000080ff3f',
  5:'ffffffffffffffffffff',6:'efcdab8967452301807f',7:'ffffffffffffffffffff'};
const initialFp={status:0,tag:0xffff,opcode:0x5a5,fip:0x11223344,fdp:0x55667788,
  cs:0x33,ds:0x5b,registers:ghosts};
const ONE_HALF='00000000000000c0ff3f',THREE='00000000000000c00040',TWO='00000000000000800040';
const sha=bytes=>createHash('sha256').update(bytes).digest('hex');
const unhex=hex=>Uint8Array.from(Buffer.from(hex,'hex'));
const data=page=>page.set(unhex('0000c03faabbccdd7f000000'));

function unsigned(bytes,cursor) {
  let value=0;
  for(let shift=0;shift<=28;shift+=7){const byte=bytes[cursor.at++];assert(byte!==undefined);
    if(shift===28)assert.equal(byte&0xf0,0);value+=(byte&127)*2**shift;
    if(!(byte&128))return value;}
  assert.fail('bounded Wasm u32');
}
function uleb(value) {
  const bytes=[];do{const byte=value%128;value=Math.floor(value/128);bytes.push(byte|(value?128:0));}while(value);
  return Uint8Array.from(bytes);
}
function join(parts) {
  const bytes=new Uint8Array(parts.reduce((n,p)=>n+p.length,0));let at=0;
  for(const part of parts){bytes.set(part,at);at+=part.length;}return bytes;
}

// This clone changes exactly one private function body. All imports, types,
// owner IDs, names, exports and immutable recovery bytes remain byte-identical.
// A normal completed handoff must not need this cold descriptor getter.
function poisonRecoveryGetter(source) {
  const module=readGeneratedModule(source),matches=[...module.names].filter(([,name])=>name==='guest.recovery_data');
  assert.equal(matches.length,1,'one actual private descriptor getter');
  const index=matches[0][0],defined=index-module.imports.length;
  assert(defined>=0&&defined<module.functions.length);
  assert.deepEqual(module.types[module.functionTypes[defined]],{parameters:[0x7f],results:[0x7e]});
  const pieces=[source.subarray(0,8)],cursor={at:8};let changed=0;
  while(cursor.at<source.length){
    const start=cursor.at,kind=source[cursor.at++],length=unsigned(source,cursor),payload=cursor.at,end=payload+length;
    assert(end<=source.length);
    if(kind!==10)pieces.push(source.subarray(start,end));
    else{
      const bodyCursor={at:payload},count=unsigned(source,bodyCursor);assert.equal(count,module.functions.length);
      const bodies=[uleb(count)];
      for(let body=0;body<count;body++){
        const bodyStart=bodyCursor.at,bodyLength=unsigned(source,bodyCursor),bodyEnd=bodyCursor.at+bodyLength;
        assert(bodyEnd<=end);
        if(body===defined){const replacement=Uint8Array.of(0,0,0x0b);bodies.push(uleb(replacement.length),replacement);changed++;}
        else bodies.push(source.subarray(bodyStart,bodyEnd));
        bodyCursor.at=bodyEnd;
      }
      assert.equal(bodyCursor.at,end);const code=join(bodies);pieces.push(Uint8Array.of(10),uleb(code.length),code);
    }
    cursor.at=end;
  }
  assert.equal(changed,1);const bytes=join(pieces),clone=readGeneratedModule(bytes);
  assert.deepEqual(clone.imports,module.imports);assert.deepEqual(clone.globalImports,module.globalImports);
  assert.deepEqual(clone.types,module.types);assert.deepEqual([...clone.exports],[...module.exports]);
  assert.deepEqual([...clone.names],[...module.names]);
  module.functions.forEach((fn,i)=>assert.deepEqual(clone.functions[i].raw,
    i===defined?Uint8Array.of(0,0,0x0b):fn.raw,'only the actual descriptor function body changes'));
  assert(WebAssembly.validate(bytes),'the poisoned private function retains the real valid module ABI');
  return bytes;
}

function literal(t,wanted,writes=[]) {
  const context=t.context(),beforeVersion=t.version(),expected=t.bytes(context,128),v=new DataView(expected.buffer);
  const gpr=t.registers.slice(),fault=wanted.fault??[0,0,0];
  for(const[index,value]of Object.entries(wanted.regs??{}))gpr[Number(index)]=value;
  gpr.forEach((value,index)=>v.setUint32(index*4,value>>>0,true));
  for(const[offset,value]of[[32,wanted.pc],[36,wanted.flags??FLAGS],[40,wanted.retired],[44,wanted.remaining],
    [48,fault[0]],[52,fault[1]],[112,0],[116,fault[2]]])v.setUint32(offset,value>>>0,true);
  const count=t.word(context,68),backing=t.word(context,64),pages=t.word(context,84);
  const ram=t.bytes(backing,count*4096),stamps=t.bytes(pages,count*16),pageView=new DataView(stamps.buffer);
  writes.forEach(([address,hex],ordinal)=>{
    const slot=t.word(t.word(context,56)+(address>>>12)*4);assert(slot>0&&slot<=count);
    ram.set(unhex(hex),(slot-1)*4096+(address&4095));
    pageView.setBigUint64((slot-1)*16+8,beforeVersion+BigInt(ordinal+1),true);t.writeExpected(address,hex);
  });
  const encoded=t.bytes(t.api.core_code_ptr()>>>0,t.api.core_code_len()>>>0);
  const run=t.check({...wanted,flags:wanted.flags??FLAGS,continueNeedCode:false});
  assert.deepEqual(t.bytes(context,128),expected,'all GPR/FLAGS/PC/quota/fault and owner fields are literal');
  assert.deepEqual(t.bytes(backing,ram.length),ram,'every byte of physical RAM follows the completed stores');
  assert.deepEqual(t.bytes(pages,stamps.length),stamps,'all page stamps preserve the exact store sequence');
  assert.equal(t.version(),beforeVersion+BigInt(writes.length));
  assert.deepEqual(t.bytes(t.api.core_code_ptr()>>>0,t.api.core_code_len()>>>0),encoded);
  assert.equal(t.result.modules.length,1,'the exact same retained owner completes this activation');
  t.result.semanticComplete=true;t.result.completedHandoff={owner:String(v.getBigUint64(120,true)),source:sha(encoded)};
  return run;
}

const cases=[
  {name:'scalar-pc-only',code:'90 ffe6',retired:2,budget:4,fp:t=>t.startingFp},
  {name:'scalar-old-flags-before-overwritten-input',code:'83f801 b800000000 ffe6',retired:3,budget:5,
    regs:{0:0},flags:0x412,fp:t=>t.startingFp},
  {name:'dirty-fp-duplicate-swap-pop-and-cw',code:'d903 d9c0 d9c9 d96b08 d95b04 ffe6',retired:6,budget:8,
    writes:[[0x3004,'0000c03f']],fp:t=>{const fp=t.fp({status:0x3800,tag:0x3fff,opcode:0x15b,
      fip:0x1009,fdp:0x3004,cs:0x1b,ds:0x23,registers:{6:ONE_HALF,7:ONE_HALF}});
      new DataView(fp.buffer).setUint16(16,0x7f,true);return fp;}},
  {name:'raw80-valid-zero-alias-and-sign',code:'d9c0 d9e0 d9c9 ffe6',retired:4,budget:6,
    fpStart:{...initialFp,status:0x200,tag:0xfff4},fp:t=>t.fp({status:0x3800,tag:0x3ff4,
      opcode:0x1c9,fip:0x1004,fdp:initialFp.fdp,cs:0x1b,ds:initialFp.ds,
      registers:{0:'01000000000000c000c0',7:ghosts[0]}})},
  {name:'fp-closed-loop-carries-both-operands',code:'d8c1 49 75fb ffe6',retired:10,budget:12,
    regs:{1:0},flags:0x447,fpStart:{...initialFp,tag:0xfff0,registers:{...ghosts,0:THREE,1:TWO}},
    fp:t=>t.fp({status:0,tag:0xfff0,opcode:0xc1,fip:ROOT,fdp:initialFp.fdp,cs:0x1b,ds:initialFp.ds,
      registers:{0:'00000000000000900240'}})},
  {name:'completed-countbranch-retains-actual-ecx',code:'d8c1 e2fc ffe6',retired:7,budget:9,
    regs:{1:0},fpStart:{...initialFp,tag:0xfff0,registers:{...ghosts,0:THREE,1:TWO}},
    fp:t=>t.fp({status:0,tag:0xfff0,opcode:0xc1,fip:ROOT,fdp:initialFp.fdp,cs:0x1b,ds:initialFp.ds,
      registers:{0:'00000000000000900240'}})},
  {name:'changed-ret-target-keeps-dirty-fp-and-stack',code:'57 d903 c3',retired:3,budget:5,pc:0x1820,
    writes:[[0x47fc,'20180000']],fp:t=>t.fp({status:0x3800,tag:0x3fff,opcode:0x103,
      fip:0x1001,fdp:0x3000,cs:0x1b,ds:0x23,registers:{7:ONE_HALF}})},
  {name:'real-cold-store-fault-keeps-before-state',code:'d903 d91f ffe6',retired:1,budget:5,
    initialRegisters:[...registers.slice(0,7),0x6000],pc:0x1002,stop:3,fault:[0x6000,0x10204,0x40e],cold:true,
    fp:t=>t.fp({status:0x3800,tag:0x3fff,opcode:0x103,fip:ROOT,fdp:0x3000,
      cs:0x1b,ds:0x23,registers:{7:ONE_HALF}})},
];

export function runCompletedHandoffCases(fixture) {
  function configuration(c,poisoned) {
    return{segments:[[ROOT,c.code]],registers:c.initialRegisters??registers,initialFlags:FLAGS,
      initialFp:c.fpStart??initialFp,control:0x27f,data,
      ...(poisoned?{moduleTransform:poisonRecoveryGetter}:{})};
  }
  function execute(t,c) {
    const run=literal(t,{pc:c.pc??TARGET,retired:c.retired,budget:c.budget,work:c.retired,
      remaining:c.budget-c.retired,stop:c.stop??1,regs:c.regs??{},flags:c.flags??FLAGS,
      fault:c.fault??[0,0,0],fp:c.fp(t)},c.writes??[]);
    assert.equal(run.helperCalls,0);assert.equal(run.preciseCalls,c.cold?1:0);assert.equal(run.activations.length,1);
  }
  // Every original module first passes its complete independent architectural
  // oracle. Only then may the poisoned clone falsify the normal handoff path.
  for(const c of cases)fixture(`completed-handoff-literal-${c.name}`,configuration(c,false),t=>execute(t,c));
  for(const c of cases.filter(c=>!c.cold))fixture(`completed-handoff-no-getter-${c.name}`,configuration(c,true),t=>execute(t,c));
  const cold=cases.find(c=>c.cold);
  fixture('completed-handoff-poisoned-cold-control-still-demands-recovery',configuration(cold,true),t=>{
    assert.throws(()=>execute(t,cold),error=>error instanceof WebAssembly.RuntimeError&&/unreachable/.test(error.message),
      'a real before-operation memory fault still reaches the poisoned descriptor getter');
    t.result.coldGetterTrap=true;
  });
}
