import {instantiateCore,coreImports} from './link.mjs';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync,writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {assertCoreNames} from './names.mjs';
import {instrumentCallCounter} from './calls.mjs';
import {runActualPrefix} from './actual-prefix.mjs';

const directory=process.argv[2],CTX=256,PAGE=4096,MAP=0x10000,PERM=0x410000;
const EXEC=0x510000,VERSION=0x510100,PAGE_VERSION=0x510200,BACKING=0x520000,GENERATION=0x530000;
const memory=new WebAssembly.Memory({initial:96}),bytes=new Uint8Array(memory.buffer),view=new DataView(memory.buffer);
const put=(at,value)=>view.setUint32(at,value>>>0,true),hash=value=>createHash('sha256').update(value).digest('hex');
const programs=new Map(),records=[],report=join(directory,'conditions-result.json');
const countFlagCalls=source=>instrumentCallCounter(source,'support.flags','test.flag_calls');

function physical(address){const slot=view.getUint32(MAP+(address>>>12)*4,true);return slot?BACKING+(slot-1)*PAGE+(address&4095):null;}
let reads=0,writes=0;
function read(context,address,width) {
  reads++;assert.equal(context,CTX);assert.equal(address,0x3000);assert.equal(width,0x104);
  put(CTX+48,address);put(CTX+52,0x20204);put(CTX+116,14);return 1n<<32n;
}
function write(context,address,width,value) {
  writes++;assert.equal(context,CTX);assert.equal(address,0x1100);assert.equal(width,4);
  put(physical(address),value);view.setBigUint64(VERSION,1n,true);view.setBigUint64(PAGE_VERSION+8,1n,true);
  view.setBigUint64(GENERATION,1n,true);return 2;
}
for(const line of readFileSync(join(directory,'conditions.tsv'),'utf8').trim().split('\n')) {
  const [name,hex,id,instructions]=line.split('\t'),source=readFileSync(join(directory,`${name}.wasm`));
  const original=new WebAssembly.Module(source);assertCoreNames(original,0x1000,id);
  assert.deepEqual(coreImports(original),[
    {module:'ring3',name:'memory',kind:'memory'},{module:'ring3',name:'read',kind:'function'},{module:'ring3',name:'write',kind:'function'},
  ]);
  const instrumented=countFlagCalls(source),module=new WebAssembly.Module(instrumented);
  const exports=instantiateCore(module,{ring3:{memory,read,write}}).exports;
  assert(exports['test.flag_calls'] instanceof WebAssembly.Global);
  programs.set(name,{...exports,id:BigInt(id),code:Buffer.from(hex,'hex'),instructions:Number(instructions),
    sourceSha256:hash(source),instrumentedSha256:hash(instrumented)});
}

function check(name,label,options,expected) {
  const program=programs.get(name),budget=options.budget??40;
  if(options.budget!==undefined) {
    const regs=[options.eax??0xa5a55a5a,0x22222222,0x33333333,0x3000,0x4800,0x55555555,0x66666666,0x77777777];
    const result=[...regs];if(expected.eax!==undefined)result[0]=expected.eax;
    if(expected.ebx!==undefined)result[3]=expected.ebx;if(expected.esi!==undefined)result[6]=expected.esi;
    runActualPrefix(program,name,label,budget,{regs,flags:options.flags??0x402,retired:37},
      {reason:expected.reason??4,pc:expected.pc,retired:expected.retired,flags:expected.flags,regs:result},
      {instrument:countFlagCalls,onRow:row=>{records.push(row);writeFileSync(report,JSON.stringify({complete:false,records}));},
        after:({generated,row})=>{
          // Initial full-span rejection publishes the unchanged tuple once;
          // native precise arithmetic itself does not call support.flags.
          row.calls=generated.exports['test.flag_calls'].value;
          assert.equal(row.calls,1,`${label}: sole pre-precise canonical FLAGS publication`);
        }});
    writeFileSync(report,JSON.stringify({complete:false,records}));return;
  }
  bytes.fill(0,CTX,CTX+128);bytes.fill(0,BACKING,BACKING+3*PAGE);
  bytes.fill(0,EXEC,EXEC+12);bytes.fill(0,VERSION,VERSION+8);bytes.fill(0,PAGE_VERSION,PAGE_VERSION+48);bytes.fill(0,GENERATION,GENERATION+8);
  for(const [page,slot,permission] of [[1,1,7],[3,2,options.readOnly?1:3],[4,3,3]]){put(MAP+page*4,slot);bytes[PERM+page]=permission;}
  put(EXEC,1);bytes.set(program.code,physical(0x1000));
  const regs=[options.eax??0xa5a55a5a,0x22222222,0x33333333,options.codeWrite?0x1100:0x3000,0x4800,0x55555555,0x66666666,0x77777777];
  regs.forEach((value,index)=>put(CTX+index*4,value));put(CTX+32,0x1000);put(CTX+36,options.flags??0x402);put(CTX+40,37);put(CTX+44,0xcafebabe);
  put(CTX+48,0xaaaaaaaa);put(CTX+52,0xbbbbbbbb);
  [MAP,PERM,BACKING,3,EXEC,0,VERSION,PAGE_VERSION,GENERATION].forEach((value,index)=>put(CTX+56+index*4,value));
  put(CTX+112,1);view.setBigUint64(CTX+120,program.id,true);
  if(options.codeWrite)put(physical(0x1100),0xffffffff);
  const wanted=bytes.slice(CTX,CTX+128),state=new DataView(wanted.buffer),ram=bytes.slice(BACKING,BACKING+3*PAGE);
  const resultRegs=[...regs];if(expected.eax!==undefined)resultRegs[0]=expected.eax;
  if(expected.ebx!==undefined)resultRegs[3]=expected.ebx;if(expected.esi!==undefined)resultRegs[6]=expected.esi;
  resultRegs.forEach((value,index)=>state.setUint32(index*4,value>>>0,true));
  state.setUint32(32,expected.pc??0x1000+program.code.length-2,true);state.setUint32(36,expected.flags,true);
  state.setUint32(40,37+expected.retired,true);state.setUint32(44,budget-expected.retired,true);
  if(expected.fault){state.setUint32(48,0x3000,true);state.setUint32(52,0x20204,true);state.setUint32(116,14,true);}
  if(options.codeWrite)new DataView(ram.buffer).setUint32(0x100,0,true);
  reads=writes=0;program['test.flag_calls'].value=0;
  const reason=program.run(CTX,budget)>>>0,calls=program['test.flag_calls'].value;
  records.push({name,label,budget,reason,calls,context:Buffer.from(bytes.subarray(CTX,CTX+128)).toString('hex'),
    ramSha256:hash(bytes.subarray(BACKING,BACKING+3*PAGE)),sourceSha256:program.sourceSha256,instrumentedSha256:program.instrumentedSha256});
  writeFileSync(report,JSON.stringify({complete:false,records}));
  assert.equal(reason,expected.reason??4,`${label}: stop`);assert.deepEqual(bytes.subarray(CTX,CTX+128),wanted,`${label}: full context`);
  assert.deepEqual(bytes.subarray(BACKING,BACKING+3*PAGE),ram,`${label}: complete code/data/stack`);
  assert.equal(reads,expected.fault?1:0);assert.equal(writes,options.codeWrite?1:0);
  assert.equal(view.getBigUint64(VERSION,true),options.codeWrite?1n:0n);assert.equal(view.getBigUint64(GENERATION,true),options.codeWrite?1n:0n);
  const versions=new Uint8Array(48);if(options.codeWrite)new DataView(versions.buffer).setBigUint64(8,1n,true);
  assert.deepEqual(bytes.subarray(PAGE_VERSION,PAGE_VERSION+48),versions);
  assert.equal(calls,expected.calls,`${label}: generic flags helper calls`);
}

for(const [name,eax,flags,result,finalFlags,taken,calls] of [
  ['add8',0xa5a500ff,0x402,0xa5a50000,0x457,true,1],
  ['sub16',0xa5a50001,0x402,0xa5a50000,0x446,false,1],
  ['logical32',0xa5a55a5a,0x402,0,0x446,true,1],
  ['inc32',0xffffffff,0x403,0,0x457,false,2],
  ['dec32',2,0x443,1,0x403,true,2],
  ['adc8',0xa5a500ff,0x403,0xa5a50000,0x457,true,2],
  ['sbb16',0xa5a50001,0x403,0xa5a50000,0x446,false,2],
  ['neg32',0,0x402,0,0x446,true,1],
])check(name,name,{eax,flags},{eax:result,flags:finalFlags,ebx:taken?0x22222222:0x11111111,retired:taken?3:4,calls});
check('entry-equal','entry canonical ZF',{flags:0x442},{flags:0x442,ebx:0x22222222,retired:2,calls:2});
check('entry-not-equal','entry canonical nonzero',{flags:0x402},{flags:0x402,ebx:0x22222222,retired:2,calls:2});
// Same-block literal recipes materialize without the general helper. A new
// block still imports unknown FLAGS; the sole canonical publisher remains counted.
check('canonical-zf','SAHF clears stale pending ZF',{eax:0xff},{eax:0,flags:0x402,ebx:0x11111111,retired:5,calls:1});
check('postbranch-carry','branch then CLC preserves pending ZF',{eax:0xff},{eax:0x5600,flags:0x456,ebx:0x22222222,retired:6,calls:2});
check('other-condition','literal ADD also supplies unsigned conditions',{eax:0xff},{eax:0,flags:0x457,ebx:0x11111111,retired:4,calls:1});
check('preserved-recipe','MOV preserves pending ZF',{eax:0xff},{eax:0,esi:0x33333333,flags:0x457,ebx:0x22222222,retired:4,calls:1});
// The single body stops at the branch's prefix guard and publishes once. INC
// additionally materializes its incoming CF before installing the new recipe;
// there is no pre-tail publication or second-body entry to normalize flags.
check('add8','precise budget before branch',{eax:0xff,budget:1},{eax:0,flags:0x457,pc:0x1002,retired:1,reason:2,calls:1});
check('inc32','precise budget preserves CF',{eax:0xffffffff,flags:0x403,budget:1},{eax:0,flags:0x457,pc:0x1001,retired:1,reason:2,calls:2});
check('rmw-fault','fault retains preceding recipe',{readOnly:true},{eax:0,flags:0x446,pc:0x1002,retired:1,reason:3,fault:true,calls:1});
check('rmw','committed code write precedes branch',{codeWrite:true},{flags:0x457,pc:0x1003,retired:1,reason:6,calls:1});
writeFileSync(report,JSON.stringify({complete:true,records}));
console.log(`${records.length} generated zero-flag branch controls passed`);
