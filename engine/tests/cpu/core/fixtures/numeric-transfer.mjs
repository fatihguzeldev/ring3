import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync,writeFileSync} from 'node:fs';
import {join} from 'node:path';

const directory=process.argv[2],CTX=256,BUDGET=32,NATIVE_PC=0x102b;
const memory=new WebAssembly.Memory({initial:2}),bytes=new Uint8Array(memory.buffer),view=new DataView(memory.buffer);
const put=(at,value)=>view.setUint32(at,value>>>0,true),hash=value=>createHash('sha256').update(value).digest('hex');
const initial=[0xa0a0a0a0,0xb1b1b1b1,0xc2c2c2c2,0xd3d3d3d3,0x4400,0xe5e5e5e5,0xf6f6f6f6,0x87878787];
const dirty=[0,4,0x22222222,0x3000,0x4800,0x55555555,0x66666666,0x77777777];
const programs=new Map(),records=[],report=join(directory,'numeric-transfer-result.json');
let active;
function numeric(context,ordinal) {
  assert.equal(context,CTX);assert.equal(ordinal,0);
  active.seen.push(bytes.slice(CTX,CTX+128));
  if(active.mode==='fault') {put(CTX+48,0x3018);put(CTX+52,0x10104);put(CTX+116,0x40e);return 3;}
  if(active.mode==='smc') {put(0x1100,0x12345678);return 6;}
  if(active.name==='status')put(CTX,(view.getUint32(CTX,true)&0xffff0000)|0x1234);
  if(active.name==='compare-flags')put(CTX+36,0x402);
  if(active.name==='processor')put(CTX,0x12345678);
  return 0;
}
for(const line of readFileSync(join(directory,'numeric-transfer.tsv'),'utf8').trim().split('\n')) {
  const [name,hex,id]=line.split('\t'),source=readFileSync(join(directory,`${name}.wasm`));
  const module=new WebAssembly.Module(source);
  assert.deepEqual(WebAssembly.Module.imports(module),[
    {module:'ring3',name:'memory',kind:'memory'},{module:'ring3',name:'read',kind:'function'},
    {module:'ring3',name:'write',kind:'function'},{module:'ring3',name:'numeric',kind:'function'},
  ]);
  const unexpected=()=>assert.fail('scalar memory import is outside this transfer fixture');
  programs.set(name,{run:new WebAssembly.Instance(module,{ring3:{memory,read:unexpected,write:unexpected,numeric}}).exports.run,
    id:BigInt(id),code:Buffer.from(hex,'hex'),sourceSha256:hash(source)});
}

function check(name,inputs,flags,outputs={},mode='success') {
  const program=programs.get(name);bytes.fill(0);bytes.set(program.code,0x1000);
  initial.forEach((value,index)=>put(CTX+index*4,value));put(CTX+32,0x1000);put(CTX+36,0x402);
  put(CTX+40,37);put(CTX+44,0xcafebabe);put(CTX+48,0xaaaaaaaa);put(CTX+52,0xbbbbbbbb);
  put(CTX+112,1);view.setBigUint64(CTX+120,program.id,true);
  const before=bytes.slice(CTX,CTX+128),wanted=bytes.slice(),state=new DataView(wanted.buffer),general=[...dirty];
  const stopped=mode!=='success',retired=mode==='fault'?9:mode==='smc'?10:11;
  if(!stopped) {
    if(name==='status')general[0]=0x1234;
    if(name==='processor')general[0]=0x12345678;
    general[7]=name==='compare-flags'?0x77777777:0x77777778;
  }
  general.forEach((value,index)=>state.setUint32(CTX+index*4,value,true));
  const next=0x1000+program.code.length-5;
  state.setUint32(CTX+32,mode==='fault'?NATIVE_PC:mode==='smc'?next:0x1000+program.code.length-2,true);
  state.setUint32(CTX+36,stopped?0x457:0x406,true);state.setUint32(CTX+40,37+retired,true);state.setUint32(CTX+44,BUDGET-retired,true);
  if(mode==='fault'){state.setUint32(CTX+48,0x3018,true);state.setUint32(CTX+52,0x10104,true);state.setUint32(CTX+116,0x40e,true);}
  if(mode==='smc')state.setUint32(0x1100,0x12345678,true);
  active={name,mode,seen:[]};const reason=program.run(CTX,BUDGET)>>>0;
  const row={name,mode,reason,sourceSha256:program.sourceSha256,contextHex:Buffer.from(bytes.subarray(CTX,CTX+128)).toString('hex'),
    helperContexts:active.seen.map(value=>Buffer.from(value).toString('hex')),memorySha256:hash(bytes)};
  records.push(row);writeFileSync(report,JSON.stringify({complete:false,scope:'authored generated bridge transfer; native numerical semantics are covered separately',records}));
  assert.equal(reason,mode==='fault'?3:mode==='smc'?6:4,`${name}/${mode}: stop`);
  assert.deepEqual(bytes.subarray(CTX,CTX+128),wanted.subarray(CTX,CTX+128),`${name}/${mode}: complete final context`);
  assert.deepEqual(bytes,wanted,`${name}/${mode}: entire code, data and context memory`);
  assert.equal(active.seen.length,1);row.semanticOraclePassed=true;
  writeFileSync(report,JSON.stringify({complete:false,records}));
  const expected=before.slice(),d=new DataView(expected.buffer);
  for(const register of inputs)d.setUint32(register*4,dirty[register],true);
  if(flags!==undefined)d.setUint32(36,flags,true);
  if(outputs.pc)d.setUint32(32,NATIVE_PC,true);
  assert.deepEqual(active.seen[0],expected,`${name}/${mode}: publish only consumed integer inputs`);
}

check('register',[],undefined);
check('indexed',[1,3],undefined);
check('status',[0],undefined);
check('move-below',[],0x403);
check('move-below-equal',[],0x443);
check('move-unordered',[],0x406);
check('compare-flags',[],0x457);
check('initialize',[],undefined);
check('processor',[0,1,2,3,4,5,6,7],0x457,{pc:true});
check('indexed',[1,3],undefined,{},'fault');
check('indexed',[1,3],undefined,{},'smc');
writeFileSync(report,JSON.stringify({complete:true,scope:'authored generated bridge transfer; no numerical or performance claim',records}));
console.log(`${records.length} generated numerical transfer controls passed`);
