import {instantiateCore,coreImports} from './link.mjs';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync, openSync, closeSync} from 'node:fs';
import {readEngine} from '../../../support/wasm/engine.mjs';
import {assertCoreNames} from './names.mjs';

const [enginePath, reportPath] = process.argv.slice(2);
assert(enginePath && reportPath, 'usage: numeric.mjs engine.wasm fresh-report.json');
const engine = readEngine(enginePath), output = openSync(reportPath, 'wx');
const hash = value => createHash('sha256').update(value).digest('hex');
const hex = value => Buffer.from(value).toString('hex');
const unhex = value => Uint8Array.from(Buffer.from(value.replaceAll(' ', ''), 'hex'));
const fpOffset = 4236, simdOffset = 4364, ymmOffset = 4612;
const report = {schema: 1, complete: false, engineSha256: engine.sha256, sources: [import.meta.url,new URL('./link.mjs',import.meta.url).href,
  new URL('./names.mjs', import.meta.url).href,
  new URL('../../../support/wasm/engine.mjs', import.meta.url).href].map(url => ({url, sha256: hash(readFileSync(new URL(url)))})),
  cases: [], checks: [], limits: ['authored typed numerical routes; no game or performance claim',
    'pending Core call overwrite is covered by the Windows fixture, not this numerical fixture']};
const checkpoint = () => { const data = JSON.stringify(report, null, 2); writeFileSync(reportPath, data); };
let stage = 'initialization';

function fp(initial, {status = 0, tag = 0xffff, opcode = 0, fip = 0, fdp = 0, cs = 0, ds = 0, registers = {}} = {}) {
  const result = initial.slice(), v = new DataView(result.buffer);
  for (const [offset, value] of [[18,status],[20,tag],[22,opcode],[32,cs],[34,ds]]) v.setUint16(offset,value,true);
  v.setUint32(24,fip,true); v.setUint32(28,fdp,true);
  for (const [index, bytes] of Object.entries(registers)) result.set(unhex(bytes), 40 + Number(index) * 10);
  return result;
}

function runCase(name, codeHex, budget, options, verify) {
  stage = name;
  const instance = instantiateCore(engine.module, {ring3_observability:{now_ns:()=>0n}});
  const memory = instance.exports.memory, api = Object.fromEntries(Object.entries(instance.exports)
    .filter(([name])=>name.startsWith('ring3_abi_v1_')).map(([name,value])=>[name.slice(13),value]));
  const slice = (pointer,length) => new Uint8Array(memory.buffer).slice(pointer,pointer+length);
  const view = () => new DataView(memory.buffer);
  const word = (pointer,offset=0) => view().getUint32(pointer+offset,true);
  const put = (pointer,offset,value) => view().setUint32(pointer+offset,value>>>0,true);
  let arena = 0, context = 0, active = false, opened = false, numericCalls = 0;
  const numeric = (...args) => {
    numericCalls++;
    const returned = api.core_numeric(...args);
    const payloads = options?.helperPayloads?.[numericCalls-1];
    if (payloads) assertBankPayloads('canonical helper observes completed numerical prefix',payloads);
    return returned;
  };
  const result = {name, codeHex, budget, controls: []}; report.cases.push(result);
  const status = (operation,wanted,...args) => {
    const actual = api[operation](...args)>>>0;
    result.controls.push({operation,args,wanted,actual});
    assert.equal(actual,wanted,`${name}: ${operation}`);
  };
  const pages = new Map([[0x1000,new Uint8Array(4096)], [0x3000,new Uint8Array(4096)],
    [0x4000,new Uint8Array(4096)], [0x5000,new Uint8Array(4096)]]);
  pages.get(0x1000).set(unhex(codeHex));
  options?.data?.(new DataView(pages.get(0x3000).buffer));
  if (options?.image) pages.get(0x5000).fill(0xa5);
  const expected = new Map([...pages].map(([address,page])=>[address,page.slice()]));
  const physical = address => {
    const slot = word(word(context,56)+(address>>>12)*4);
    assert(slot>0 && slot<=word(context,68));
    return word(context,64)+(slot-1)*4096+(address&4095);
  };
  const raw = () => ({context:hex(slice(context,128)), arena:hex(slice(arena,api.arena_len())),
    pages:Object.fromEntries([...pages.keys()].map(address=>[address.toString(16),hex(slice(physical(address),4096))])),
    transactionVersion:String(view().getBigUint64(word(context,80),true)),
    pageVersions:hex(slice(word(context,84),word(context,68)*16))});
  const writeExpected = (address,bytes) => expected.get(address&~4095).set(bytes,address&4095);
  const assertRam = () => {for(const [address,page] of expected) assert.deepEqual(slice(physical(address),4096),page,`${name}: full page ${address.toString(16)}`);};
  const neutral = (label,action) => {
    const before = raw(); action(); const after = raw();
    result.controls.push({label,before,after}); checkpoint(); assert.deepEqual(after,before,`${name}: ${label} neutral`);
  };
  function assertBankPayloads(label,payloads) {
    const pointer=word(context,92), observed={};
    assert(pointer>0,'private numerical bank is resident');
    for(const[index,wanted]of Object.entries(payloads)) {
      observed[index]=hex(slice(pointer+20+Number(index)*10,10));
      assert.equal(observed[index],wanted,`${name}: ${label}, physical slot ${index}`);
    }
    result.controls.push({label,payloads:observed});
  }
  try {
    for(const name of ['open','close','arena_ptr','arena_len','map','upload','core_prepare','core_code_ptr','core_code_len',
      'core_context_ptr','core_begin','core_end','core_import','core_export','core_numeric','core_read','core_write']) assert.equal(typeof api[name],'function',`native ${name}`);
    status('open',0,16,97,0); opened = true; arena = api.arena_ptr()>>>0;
    for(const [address,page] of pages) {
      status('map',0,address,1,address===0x1000?7:3);
      new Uint8Array(memory.buffer,arena+140,4096).set(page); status('upload',0,address,4096);
    }
    const registers = options?.registers ?? [0,0,0,0x3000,0x4800,0,0,0];
    new Uint8Array(memory.buffer,arena,56).fill(0);
    new Uint8Array(memory.buffer,arena,4).set([82,51,83,84]);
    view().setUint16(arena+4,3,true); view().setUint16(arena+6,1,true); put(arena,8,56);
    registers.forEach((value,index)=>put(arena,16+index*4,value)); put(arena,48,0x1000); put(arena,52,options?.flags??2);
    if(options?.control!==undefined) view().setUint16(arena+fpOffset+16,options.control,true);
    if(options?.fp) new Uint8Array(memory.buffer,arena+fpOffset,128).set(fp(slice(arena+fpOffset,128),options.fp));
    if(options?.mxcsr!==undefined) put(arena,simdOffset+16,options.mxcsr);
    if(options?.fs) status('set_segment_base',0,4,options.fs);
    const initialFp = slice(arena+fpOffset,128), initialSimd = slice(arena+simdOffset,160), initialYmm = slice(arena+ymmOffset,144);
    status('core_prepare',0,0x1000,256,32);
    context = api.core_context_ptr()>>>0; assert(context>0 && context%8===0);
    const code = slice(api.core_code_ptr()>>>0,api.core_code_len()>>>0);
    const module = new WebAssembly.Module(code);
    assertCoreNames(module, 0x1000);
    assert.deepEqual(coreImports(module), [
      {module:'ring3',name:'memory',kind:'memory'}, {module:'ring3',name:'read',kind:'function'},
      {module:'ring3',name:'write',kind:'function'}, {module:'ring3',name:'numeric',kind:'function'}]);
    let run = instantiateCore(module,{ring3:{memory,read:api.core_read,write:api.core_write,numeric}}).exports.run;
    result.module = {sha256:hash(code),wasmHex:hex(code)};
    status('core_begin',0); active = true;
    const initialVersion=view().getBigUint64(word(context,80),true),initialPageVersions=slice(word(context,84),word(context,68)*16);
    neutral('wrong numerical context',()=>status('core_numeric',8,context+8,0));
    neutral('wrong numerical ordinal',()=>status('core_numeric',8,context,0xffffffff));
    let unpublishedFp = initialFp;
    if(options?.handoff) {
      const prefixStop = run(context,options.handoff.budget)>>>0;
      assert.equal(prefixStop,2,'handoff stops at the declared instruction budget');
      const prefixRetired = word(context,40), prefixPc = word(context,32);
      status('core_end',0); active = false;
      status('core_export',0,prefixStop,options.handoff.budget);
      const published = slice(arena+fpOffset,128);
      assert.deepEqual(published,fp(initialFp,{status:0x3800,tag:0x3fff,opcode:0x103,fip:0x1000,fdp:0x3000,cs:0x1b,ds:0x23,registers:{7:ext3}}),'literal prefix FP128');
      new Uint8Array(memory.buffer,arena+fpOffset+40+7*10,10).set(unhex(options.handoff.replacement));
      unpublishedFp = slice(arena+fpOffset,128);
      status('core_import',0);
      status('core_prepare',0,prefixPc,256,32);
      context = api.core_context_ptr()>>>0;
      const resumedCode = slice(api.core_code_ptr()>>>0,api.core_code_len()>>>0);
      const resumedModule = new WebAssembly.Module(resumedCode);
      assertCoreNames(resumedModule,prefixPc);
      run = instantiateCore(resumedModule,{ring3:{memory,read:api.core_read,write:api.core_write,numeric}}).exports.run;
      result.handoff = {prefixRetired,prefixPc,published:hex(published),imported:hex(unpublishedFp),moduleSha256:hash(resumedCode)};
      status('core_begin',0); active = true;
    }
    if(options?.reentry) {
      const prefixStop=run(context,options.reentry.budget)>>>0;
      assert.equal(prefixStop,2,'private reentry stops at the exact prefix budget');
      const prefixRetired=word(context,40),prefixPc=word(context,32);
      assert.equal(prefixRetired,options.reentry.budget);
      assert.equal(word(context,44),0,'private prefix consumed its entire declared budget');
      status('core_end',0);active=false;
      assertBankPayloads('core_end preserves completed physical register payloads',options.reentry.payloads);
      assert.deepEqual(slice(arena+fpOffset,128),unpublishedFp,'private core_end does not publish arena state');
      status('core_prepare',0,prefixPc,256,32);
      context=api.core_context_ptr()>>>0;
      const resumedCode=slice(api.core_code_ptr()>>>0,api.core_code_len()>>>0),resumedModule=new WebAssembly.Module(resumedCode);
      assertCoreNames(resumedModule,prefixPc);
      run=instantiateCore(resumedModule,{ring3:{memory,read:api.core_read,write:api.core_write,numeric}}).exports.run;
      result.reentry={prefixRetired,prefixPc,moduleSha256:hash(resumedCode)};
      status('core_begin',0);active=true;
    }
    const bankPointer=word(context,92);
    if(options?.nullBank) put(context,92,0);
    const stop = run(context,budget)>>>0;
    result.executed = {stop,numericCalls,...raw()}; checkpoint();
    if(options?.nullBank) put(context,92,bankPointer);
    if(options?.numericCalls!==undefined) assert.equal(numericCalls,options.numericCalls,`${name}: numerical helper calls`);
    status('core_end',0); active = false;
    if(options?.endPayloads) assertBankPayloads('core_end preserves completed and popped physical register payloads',options.endPayloads);
    neutral('inactive numerical import',()=>status('core_numeric',8,context,0));
    assert.deepEqual(slice(arena+fpOffset,128),unpublishedFp,'private FP bank remains unpublished until export');
    assert.deepEqual(slice(arena+simdOffset,160),initialSimd,'private SIMD bank remains unpublished until export');
    const state = {registers:Array.from({length:8},(_,i)=>word(context,i*4)),pc:word(context,32),flags:word(context,36),
      retired:word(context,40),remaining:word(context,44),vector:word(context,116),detail:word(context,48),metadata:word(context,52)};
    const work = budget-state.remaining+(options?.reentry?.budget??0);
    if(options?.invalidExport) {
      const flags = word(context,36); put(context,36,0);
      neutral('invalid FLAGS export with dirty bank',()=>status('core_export',7,stop,work)); put(context,36,flags);
    }
    status('core_export',0,stop,work);
    result.published = raw(); checkpoint();
    const tools = {api,memory,arena,context,initialFp,initialSimd,initialVersion,initialPageVersions,initialRegisters:registers,budget,state,stop,word,put,slice,physical,status,neutral,writeExpected,
      fp:options=>fp(initialFp,options),simd:initialSimd.slice(),read:address=>slice(physical(address),4)};
    verify(tools);
    assert.deepEqual(slice(arena+simdOffset,160),tools.simd,'full literal SIMD160');
    assert.deepEqual(slice(arena+ymmOffset,144),initialYmm,'unmodified upper vector bank');
    assert.deepEqual(Array.from({length:8},(_,i)=>word(arena,16+i*4)),state.registers,'published architectural general state');
    assertRam();
    if(stop===3 || stop===9) {
      neutral('outstanding exception export',()=>status('core_export',12,stop,0));
      neutral('outstanding exception import',()=>status('core_import',12));
    } else if(stop!==6) {
      const retained = slice(context,40); status('core_import',0);
      assert.deepEqual(slice(context,40),retained,'export/import preserves architectural general state');
      status('core_begin',0); active = true;
      assert.equal(run(context,0),2); status('core_end',0); active = false;
    } else {
      neutral('stale code after numerical store',()=>status('core_begin',4));
    }
    result.final = raw(); result.complete = true; checkpoint();
  } finally {if(active) api.core_end(); if(opened) api.close();}
}

const values = v => {v.setFloat32(0,3,true);v.setFloat32(4,2,true);};
const ext3 = '00000000000000c00040', ext6 = '00000000000000c00140';
function standard(t,retired,pc,flags=2,stop=2,changes={}) {
  assert.equal(t.stop,stop); assert.equal(t.state.retired,retired); assert.equal(t.state.pc,pc); assert.equal(t.state.flags,flags);
  assert.equal(t.state.remaining,t.budget-retired);
  const registers = [...t.initialRegisters];for(const [index,value] of Object.entries(changes)) registers[Number(index)]=value;
  assert.deepEqual(t.state.registers,registers,'all architectural general registers');
}
function bank(t,wanted) {assert.deepEqual(t.slice(t.arena+fpOffset,128),wanted,'full literal FP128');}

try {
  runCase('private-reentry-preserves-stack-through-exchange-pop-and-sign',
    'd903 d94304 d9c9 dec1 d9e0 d9e1 0f0b',4,
    {control:0x27f,data:values,numericCalls:0,
      reentry:{budget:2,payloads:{6:'00000000000000800040',7:ext3}},
      endPayloads:{6:ext3,7:'00000000000000a00140'}},t=>{
      assert.equal(t.stop,2);assert.equal(t.state.retired,6);assert.equal(t.state.remaining,0);
      assert.equal(t.state.pc,0x100d);assert.equal(t.state.flags,2);
      assert.deepEqual(t.state.registers,t.initialRegisters,'private reentry preserves every general register');
      bank(t,t.fp({status:0x3800,tag:0x3fff,opcode:0x1e1,fip:0x100b,fdp:0x3004,cs:0x1b,ds:0x23,
        registers:{6:ext3,7:'00000000000000a00140'}}));
    });
  runCase('popped-x87-payload-is-visible-to-fxsave',
    'd903 d84b04 d95b08 0fae0500500000 0f0b',4,
    {control:0x27f,data:values,numericCalls:1,image:true,helperPayloads:[{7:ext6}]},t=>{
      standard(t,4,0x100f);t.writeExpected(0x3008,unhex('0000c040'));
      bank(t,t.fp({opcode:0x15b,fip:0x1005,fdp:0x3008,cs:0x1b,ds:0x23,registers:{7:ext6}}));
      const image=new Uint8Array(464),v=new DataView(image.buffer);
      v.setUint16(0,0x27f,true);v.setUint16(6,0x15b,true);
      v.setUint32(8,0x1005,true);v.setUint16(12,0x1b,true);
      v.setUint32(16,0x3008,true);v.setUint16(20,0x23,true);
      v.setUint32(24,0x1f80,true);v.setUint32(28,0xffff,true);
      image.set(unhex(ext6),32+7*16);t.writeExpected(0x5000,image);
    });
  runCase('completed-arithmetic-prefix-survives-helper-fault',
    'd903 d84b04 d91d00600000 0f0b',8,
    {control:0x27f,data:values,numericCalls:1,helperPayloads:[{7:ext6}]},t=>{
      standard(t,2,0x1005,2,3);assert.equal(t.state.detail,0x6000);
      assert.equal(t.state.metadata,0x10204);assert.equal(t.state.vector,0x40e);
      bank(t,t.fp({status:0x3800,tag:0x3fff,opcode:0x4b,fip:0x1002,fdp:0x3004,
        cs:0x1b,ds:0x23,registers:{7:ext6}}));
    });
  runCase('exchange-preserves-full64-low-significand-bit',
    'd903 d9c9 db7b08 d95b14 0f0b',4,
    {control:0x27f,data:values,fp:{tag:0xfffc,registers:{0:'0100000000000080ff3f'}}},t=>{
      standard(t,4,0x100a);t.writeExpected(0x3008,unhex('0100000000000080ff3f'));
      t.writeExpected(0x3014,unhex('00004040'));
      bank(t,t.fp({status:0x800,opcode:0x15b,fip:0x1007,fdp:0x3014,cs:0x1b,ds:0x23,
        registers:{0:ext3,7:'0100000000000080ff3f'}}));
    });
  runCase('consecutive-stack-metadata-through-exchange-pop-and-sign','d903 d94304 d9c9 dec1 d9e0 d9e1 d95b08 0f0b',7,
    {control:0x27f,data:values,numericCalls:0},t=>{
      standard(t,7,0x1010);t.writeExpected(0x3008,unhex('0000a040'));
      bank(t,t.fp({opcode:0x15b,fip:0x100d,fdp:0x3008,cs:0x1b,ds:0x23,
        registers:{6:ext3,7:'00000000000000a00140'}}));
    });
  runCase('consecutive-control-reloads-after-canonical-control-write','d903 d96b0c d84304 dd5b08 0f0b',4,
    {control:0x27f,numericCalls:1,data:v=>{v.setFloat32(0,1,true);v.setFloat32(4,2**-24,true);v.setUint16(12,0x7f,true);}},t=>{
      standard(t,4,0x100b);t.writeExpected(0x3008,unhex('000000000000f03f'));
      const wanted=t.fp({status:0x20,opcode:0x55b,fip:0x1008,fdp:0x3008,cs:0x1b,ds:0x23,registers:{7:'0000000000000080ff3f'}});
      new DataView(wanted.buffer).setUint16(16,0x7f,true);bank(t,wanted);
    });
  runCase('consecutive-null-bank-declines-each-private-initialization','d903 d84b04 d95b08 0f0b',3,
    {control:0x27f,data:values,numericCalls:3,nullBank:true},t=>{
      standard(t,3,0x1008);t.writeExpected(0x3008,unhex('0000c040'));
      bank(t,t.fp({opcode:0x15b,fip:0x1005,fdp:0x3008,cs:0x1b,ds:0x23,registers:{7:ext6}}));
    });
  runCase('consecutive-stack-reloads-after-canonical-cross-page-pop','d903 d91dfe3f0000 d94304 d84b04 d95b08 0f0b',5,
    {control:0x27f,data:values,numericCalls:1},t=>{
      standard(t,5,0x1011);t.writeExpected(0x3ffe,unhex('0000'));t.writeExpected(0x4000,unhex('4040'));
      t.writeExpected(0x3008,unhex('00008040'));
      bank(t,t.fp({opcode:0x15b,fip:0x100e,fdp:0x3008,cs:0x1b,ds:0x23,registers:{7:'00000000000000800140'}}));
      const versions=t.initialPageVersions.slice(),v=new DataView(versions.buffer),base=t.word(t.context,64);
      for(const[address,stamp]of [[0x3000,t.initialVersion+2n],[0x4000,t.initialVersion+1n]])
        v.setBigUint64(Math.floor((t.physical(address)-base)/4096)*16+8,stamp,true);
      assert.deepEqual(t.slice(t.word(t.context,84),versions.length),versions,'cross-page pop and later store retain exact physical-page stamps');
      assert.equal(new DataView(t.memory.buffer).getBigUint64(t.word(t.context,80),true),t.initialVersion+2n,'both successful stores retain their exact transaction stamps');
    });
  for(const control of [0x7f,0x27f]) {
    runCase(`consecutive-normal-chain-${control.toString(16)}`,'d903 d84b04 d84304 d95b08 0f0b',4,
      {control,data:values,numericCalls:0},t=>{
        standard(t,4,0x100b);t.writeExpected(0x3008,unhex('00000041'));
        bank(t,t.fp({opcode:0x15b,fip:0x1008,fdp:0x3008,cs:0x1b,ds:0x23,registers:{7:'00000000000000800240'}}));
      });
    runCase(`consecutive-negative-zero-${control.toString(16)}`,'d903 d84b04 d84b04 d95b08 0f0b',4,
      {control,numericCalls:0,data:v=>{v.setUint32(0,0x80000000,true);v.setFloat32(4,2,true);}},t=>{
        standard(t,4,0x100b);t.writeExpected(0x3008,unhex('00000080'));
        bank(t,t.fp({opcode:0x15b,fip:0x1008,fdp:0x3008,cs:0x1b,ds:0x23,registers:{7:'00000000000000000080'}}));
      });
    runCase(`consecutive-precision-chain-${control.toString(16)}`,'d903 d84304 d84304 dd5b08 0f0b',4,
      {control,numericCalls:0,data:v=>{v.setFloat32(0,1,true);v.setFloat32(4,2**-24,true);}},t=>{
        const single=control===0x7f;
        standard(t,4,0x100b);t.writeExpected(0x3008,unhex(single?'000000000000f03f':'000000200000f03f'));
        bank(t,t.fp({status:single?0x20:0,opcode:0x55b,fip:0x1008,fdp:0x3008,cs:0x1b,ds:0x23,
          registers:{7:single?'0000000000000080ff3f':'0000000000010080ff3f'}}));
      });
  }
  runCase('consecutive-normal-chain-fast-main','d903 d84b04 d84304 d95b08 0f0b',8,
    {control:0x27f,data:values,numericCalls:0},t=>{
      standard(t,4,0x100b,2,4);t.writeExpected(0x3008,unhex('00000041'));
      bank(t,t.fp({opcode:0x15b,fip:0x1008,fdp:0x3008,cs:0x1b,ds:0x23,registers:{7:'00000000000000800240'}}));
    });
  runCase('consecutive-arithmetic-after-canonical-helper','d903 d9fa d84b04 d95b08 0f0b',4,
    {control:0x27f,numericCalls:1,data:v=>{v.setFloat32(0,4,true);v.setFloat32(4,2,true);}},t=>{
      standard(t,4,0x100a);t.writeExpected(0x3008,unhex('00008040'));
      bank(t,t.fp({opcode:0x15b,fip:0x1007,fdp:0x3008,cs:0x1b,ds:0x23,registers:{7:'00000000000000800140'}}));
    });
  runCase('consecutive-arithmetic-after-direct-decline','d903 d84304 d84b08 d95b0c 0f0b',4,
    {control:0x27f,numericCalls:1,data:v=>{v.setUint32(0,0x00800000,true);v.setUint32(4,1,true);v.setFloat32(8,2,true);}},t=>{
      standard(t,4,0x100b);t.writeExpected(0x300c,unhex('01000001'));
      bank(t,t.fp({status:2,opcode:0x15b,fip:0x1008,fdp:0x300c,cs:0x1b,ds:0x23,
        registers:{7:'0000000000010080823f'}}));
    });
  runCase('consecutive-arithmetic-after-register-exchange','d903 d84b04 d9c9 d84b04 d95b08 0f0b',5,
    {control:0x27f,data:values,numericCalls:0,fp:{tag:0xfffc,registers:{0:'00000000000000800040'}}},t=>{
      standard(t,5,0x100d);t.writeExpected(0x3008,unhex('00008040'));
      bank(t,t.fp({tag:0xfffc,opcode:0x15b,fip:0x100a,fdp:0x3008,cs:0x1b,ds:0x23,
        registers:{0:ext6,7:'00000000000000800140'}}));
    });
  runCase('consecutive-arithmetic-keeps-prefix-before-memory-fault','d903 d84b04 d80d00600000 0f0b',8,
    {control:0x27f,data:values,numericCalls:1},t=>{
      standard(t,2,0x1005,2,3);assert.equal(t.state.detail,0x6000);assert.equal(t.state.vector,0x40e);
      bank(t,t.fp({status:0x3800,tag:0x3fff,opcode:0x4b,fip:0x1002,fdp:0x3004,cs:0x1b,ds:0x23,registers:{7:ext6}}));
    });
  runCase('precise-tail-keeps-three-store-widths-with-numerical-import','d903 884310 66894312 894314 90 0f0b',4,
    {data:values,registers:[0x12345678,0,0,0x3000,0x4800,0,0,0],flags:0xcd7,numericCalls:0},t=>{
      standard(t,4,0x100c,0xcd7);t.writeExpected(0x3010,unhex('78'));t.writeExpected(0x3012,unhex('7856'));t.writeExpected(0x3014,unhex('78563412'));
      bank(t,t.fp({status:0x3800,tag:0x3fff,opcode:0x103,fip:0x1000,fdp:0x3000,cs:0x1b,ds:0x23,registers:{7:ext3}}));
      const versions=t.initialPageVersions.slice(),slot=Math.floor((t.physical(0x3010)-t.word(t.context,64))/4096);
      new DataView(versions.buffer).setBigUint64(slot*16+8,t.initialVersion+3n,true);
      assert.deepEqual(t.slice(t.word(t.context,84),versions.length),versions,'three widths stamp only their shared data page');
      assert.equal(new DataView(t.memory.buffer).getBigUint64(t.word(t.context,80),true),t.initialVersion+3n,'one transaction for each exact-tail store');
    });
  runCase('x87-multiply','d903 d84b04 d95b08 0f0b',3,{data:values,invalidExport:true},t=>{
    standard(t,3,0x1008); t.writeExpected(0x3008,unhex('0000c040')); assert.deepEqual(t.read(0x3008),unhex('0000c040'));
    bank(t,t.fp({opcode:0x15b,fip:0x1005,fdp:0x3008,cs:0x1b,ds:0x23,registers:{7:ext6}}));
  });
  runCase('numerical-store-then-scalar-load-modify','d903 d95b08 8b4308 83430801 0f0b',4,{data:values},t=>{
    standard(t,4,0x100c,2,2,{0:0x40400000});t.writeExpected(0x3008,unhex('01004040'));
    bank(t,t.fp({opcode:0x15b,fip:0x1002,fdp:0x3008,cs:0x1b,ds:0x23,registers:{7:ext3}}));
    const versions=t.initialPageVersions.slice(),slot=Math.floor((t.physical(0x3008)-t.word(t.context,64))/4096);
    new DataView(versions.buffer).setBigUint64(slot*16+8,t.initialVersion+2n,true);
    assert.deepEqual(t.slice(t.word(t.context,84),versions.length),versions,'native numerical and scalar stores publish only their physical page');
    assert.equal(new DataView(t.memory.buffer).getBigUint64(t.word(t.context,80),true),t.initialVersion+2n,'one transaction per committed store');
  });
  runCase('integer-flags-fcmov-fcomi-status','d903 d94304 31c0 dac1 39c0 dbf1 dac1 dfe0 d95b08 0f0b',9,{data:values},t=>{
    standard(t,9,0x1014,3,2,{0:0x3000}); t.writeExpected(0x3008,unhex('00004040'));
    bank(t,t.fp({status:0x3800,tag:0x3fff,opcode:0x15b,fip:0x1011,fdp:0x3008,cs:0x1b,ds:0x23,registers:{6:ext3,7:ext3}}));
  });
  runCase('mmx-physical-alias-emms','0f6ec0 0f7f03 0f77 0f0b',3,{registers:[0x76543210,0,0,0x3000,0x4800,0,0,0]},t=>{
    standard(t,3,0x1008); t.writeExpected(0x3000,unhex('1032547600000000'));
    bank(t,t.fp({registers:{0:'1032547600000000ffff'}}));
  });
  runCase('scalar-sse-add-store','f30f1003 f30f584304 f30f114308 0f0b',3,{data:values},t=>{
    standard(t,3,0x100e); t.writeExpected(0x3008,unhex('0000a040')); t.simd.set(unhex('0000a040'),32);
    assert.deepEqual(t.slice(t.arena+simdOffset,160),t.simd); bank(t,t.initialFp);
  });
  runCase('scalar-sse-unmasked-invalid','f30f1003 f30f5e4304 0f0b',8,{mxcsr:0x1f00},t=>{
    standard(t,1,0x1004,2,9); assert.equal(t.state.vector,19); assert.equal(t.state.detail,0x1f01);
    new DataView(t.simd.buffer).setUint32(16,0x1f01,true); assert.deepEqual(t.slice(t.arena+simdOffset,160),t.simd); bank(t,t.initialFp);
  });
  runCase('fxsave-restore','d903 0fae0500500000 dbe3 0fae0d00500000 dfe0 d95b08 0f0b',6,{data:values,image:true},t=>{
    standard(t,6,0x1017,2,2,{0:0x3800}); t.writeExpected(0x3008,unhex('00004040'));
    const image = new Uint8Array(464), v = new DataView(image.buffer);
    v.setUint16(0,0x37f,true); v.setUint16(2,0x3800,true); image[4]=0x80; v.setUint16(6,0x103,true);
    v.setUint32(8,0x1000,true); v.setUint16(12,0x1b,true); v.setUint32(16,0x3000,true);v.setUint16(20,0x23,true);
    v.setUint32(24,0x1f80,true);v.setUint32(28,0xffff,true);image.set(unhex(ext3),32);t.writeExpected(0x5000,image);
    bank(t,t.fp({opcode:0x15b,fip:0x1014,fdp:0x3008,cs:0x1b,ds:0x23,registers:{7:ext3}}));
  });
  runCase('fs-numerical-memory','64d903 64d95b08 0f0b',2,{data:values,fs:0x3000,registers:[0,0,0,0,0x4800,0,0,0]},t=>{
    standard(t,2,0x1007);t.writeExpected(0x3008,unhex('00004040'));
    bank(t,t.fp({opcode:0x15b,fip:0x1003,fdp:8,cs:0x1b,ds:0x3b,registers:{7:ext3}}));
  });
  runCase('x87-store-fault-prefix','d903 d91d00600000 0f0b',8,{data:values},t=>{
    standard(t,1,0x1002,2,3);assert.equal(t.state.detail,0x6000);assert.equal(t.state.metadata,0x10204);assert.equal(t.state.vector,0x40e);
    bank(t,t.fp({status:0x3800,tag:0x3fff,opcode:0x103,fip:0x1000,fdp:0x3000,cs:0x1b,ds:0x23,registers:{7:ext3}}));
  });
  runCase('x87-code-store-committed-prefix','d903 d91d00110000 0f0b',8,{data:values},t=>{
    standard(t,2,0x1008,2,6);t.writeExpected(0x1100,unhex('00004040'));
    bank(t,t.fp({opcode:0x11d,fip:0x1002,fdp:0x1100,cs:0x1b,ds:0x23,registers:{7:ext3}}));
  });
  for(const [control,small,expected] of [[0x7f,0x3e60000000000000n,null],[0x27f,0x3c30000000000000n,null],[0x37f,0x3c30000000000000n,'0000000000000080c33f']]) {
    runCase(`precision-${control.toString(16)}`,'dd03 dc4308 dc23 db7b20 0f0b',4,{control,data:v=>{v.setFloat64(0,1,true);v.setBigUint64(8,small,true);}},t=>{
      standard(t,4,0x100a); const value=unhex(expected??'00000000000000000000');t.writeExpected(0x3020,value);
      bank(t,t.fp({status:expected===null?0x20:0,opcode:0x37b,fip:0x1007,fdp:0x3020,cs:0x1b,ds:0x23,registers:{7:hex(value)}}));
    });
  }
  runCase('direct-pc24-add-pop-alias','d903 d94304 dec1 d95b08 0f0b',4,{control:0x7f,data:values,numericCalls:0},t=>{
    standard(t,4,0x100a);t.writeExpected(0x3008,unhex('0000a040'));
    bank(t,t.fp({opcode:0x15b,fip:0x1007,fdp:0x3008,cs:0x1b,ds:0x23,registers:{6:'00000000000000800040',7:'00000000000000a00140'}}));
  });
  runCase('direct-pc24-reverse-subtract','d903 d86b04 d95b08 0f0b',3,{control:0x7f,data:values,numericCalls:0},t=>{
    standard(t,3,0x1008);t.writeExpected(0x3008,unhex('000080bf'));
    bank(t,t.fp({opcode:0x15b,fip:0x1005,fdp:0x3008,cs:0x1b,ds:0x23,registers:{7:'0000000000000080ffbf'}}));
  });
  runCase('direct-pc24-divide-preserves-pe-and-c1','d903 d87304 dfe0 d95b08 0f0b',4,{control:0x7f,numericCalls:0,data:v=>{v.setFloat32(0,3,true);v.setFloat32(4,7,true);}},t=>{
    standard(t,4,0x100a,2,2,{0:0x3a20});t.writeExpected(0x3008,unhex('b76ddb3e'));
    bank(t,t.fp({status:0x20,opcode:0x15b,fip:0x1007,fdp:0x3008,cs:0x1b,ds:0x23,registers:{7:'0000000000b76ddbfd3f'}}));
  });
  runCase('direct-register-transfer-keeps-stale-physical-payloads','d903 d94304 d9c9 ddd9 d95b08 0f0b',5,{control:0x7f,data:values,numericCalls:0},t=>{
    standard(t,5,0x100c);t.writeExpected(0x3008,unhex('00004040'));
    bank(t,t.fp({opcode:0x15b,fip:0x1009,fdp:0x3008,cs:0x1b,ds:0x23,registers:{6:ext3,7:ext3}}));
  });
  runCase('direct-pc24-declines-denormal-memory-operand','d903 d84304 dfe0 d95b08 0f0b',4,{control:0x7f,numericCalls:1,data:v=>{v.setFloat32(0,3,true);v.setUint32(4,1,true);}},t=>{
    standard(t,4,0x100a,2,2,{0:0x3822});t.writeExpected(0x3008,unhex('00004040'));
    bank(t,t.fp({status:0x22,opcode:0x15b,fip:0x1007,fdp:0x3008,cs:0x1b,ds:0x23,registers:{7:ext3}}));
  });
  runCase('direct-pc24-retains-values-below-binary32-range','d903 d84b04 db7b08 0f0b',3,{control:0x7f,numericCalls:2,data:v=>{v.setUint32(0,0x00800000,true);v.setUint32(4,0x00800000,true);}},t=>{
    standard(t,3,0x1008);const value='0000000000000080033f';t.writeExpected(0x3008,unhex(value));
    bank(t,t.fp({opcode:0x37b,fip:0x1005,fdp:0x3008,cs:0x1b,ds:0x23,registers:{7:value}}));
  });
  runCase('direct-bank-import-reprepare-resume','d903 d84b04 d95b08 0f0b',2,{control:0x7f,data:values,numericCalls:0,handoff:{budget:1,replacement:'00000000000000800140'}},t=>{
    assert.equal(t.stop,2);assert.equal(t.state.retired,3);assert.equal(t.state.remaining,0);assert.equal(t.state.pc,0x1008);assert.equal(t.state.flags,2);
    assert.deepEqual(t.state.registers,t.initialRegisters);t.writeExpected(0x3008,unhex('00000041'));
    bank(t,t.fp({opcode:0x15b,fip:0x1005,fdp:0x3008,cs:0x1b,ds:0x23,registers:{7:'00000000000000800240'}}));
  });
  const stateRegisters=[0xa5c35aa5,0x12345678,0x87654321,0x3000,0x4800,0x11223344,0x55667788,0x99aabbcc];
  const stateMetadata={tag:0xffff,opcode:0x5a5,fip:0x10203040,fdp:0x50607080,cs:0x1b,ds:0x23,
    registers:{0:ext3,7:ext6}};
  runCase('direct-status-ax-preserves-deferred-integer-flags','83c001 dfe0 0f0b',2,
    {fp:{...stateMetadata,status:0x6a3f},registers:[0xffffffff,...stateRegisters.slice(1)],numericCalls:0},t=>{
      standard(t,2,0x1005,0x57,2,{0:0x6a3f});bank(t,t.initialFp);
    });
  for(const control of [0x7f,0x27f,0x37f,0x77f,0xb7f,0xf7f]) {
    runCase(`direct-status-ax-${control.toString(16)}`,'dfe0 0f0b',1,
      {control,fp:{...stateMetadata,status:0x6a3f},registers:stateRegisters,flags:0xcd7,numericCalls:0},t=>{
        standard(t,1,0x1002,0xcd7,2,{0:0xa5c36a3f});bank(t,t.initialFp);
      });
  }
  for(let bit=0;bit<6;bit++) {
    const control=0x37f&~(1<<bit),status=0x4a00|(1<<bit);
    runCase(`direct-status-ax-ignores-pending-${bit}`,'dfe0 0f0b',1,
      {control,fp:{...stateMetadata,status},registers:stateRegisters,flags:0xcd7,numericCalls:0},t=>{
        standard(t,1,0x1002,0xcd7,2,{0:(0xa5c30000|status)>>>0});bank(t,t.initialFp);
      });
    runCase(`wait-delivers-pending-${bit}`,'9b 0f0b',8,
      {control,fp:{...stateMetadata,status},registers:stateRegisters,flags:0xcd7,numericCalls:1},t=>{
        standard(t,0,0x1000,0xcd7,9);assert.equal(t.state.vector,16);assert.equal(t.state.detail,status|0x8080);
        bank(t,t.fp({...stateMetadata,status:status|0x8080}));
      });
  }
  for(const[control,status]of[[0x37f,0x6a3f],[0x37e,0x6a20],[0x340,0x4700]]) {
    runCase(`direct-wait-${control.toString(16)}-${status.toString(16)}`,'9b dfe0 0f0b',2,
      {control,fp:{...stateMetadata,status},registers:stateRegisters,flags:0xcd7,numericCalls:0},t=>{
        standard(t,2,0x1003,0xcd7,2,{0:(0xa5c30000|status)>>>0});bank(t,t.initialFp);
      });
  }
  runCase('status-before-wait-retains-successful-prefix','dfe0 9b 0f0b',8,
    {control:0x37e,fp:{...stateMetadata,status:0x4a01},registers:stateRegisters,flags:0xcd7,numericCalls:1},t=>{
      standard(t,1,0x1002,0xcd7,9,{0:0xa5c34a01});assert.equal(t.state.vector,16);assert.equal(t.state.detail,0xca81);
      bank(t,t.fp({...stateMetadata,status:0xca81}));
    });
  for(const[code,pc]of[['dfe0',0x1002],['9b',0x1001]]) {
    runCase(`state-null-bank-fallback-${code}`,`${code} 0f0b`,1,
      {fp:{...stateMetadata,status:0x6a3f},registers:stateRegisters,flags:0xcd7,numericCalls:1,nullBank:true},t=>{
        standard(t,1,pc,0xcd7,2,code==='dfe0'?{0:0xa5c36a3f}:{});bank(t,t.initialFp);
      });
  }
  report.complete = true; checkpoint(); console.log(JSON.stringify({complete:true,cases:report.cases.length,engineSha256:engine.sha256}));
} catch(error) {
  report.failure={stage,message:String(error),stack:error.stack};checkpoint();throw error;
} finally {closeSync(output);}
