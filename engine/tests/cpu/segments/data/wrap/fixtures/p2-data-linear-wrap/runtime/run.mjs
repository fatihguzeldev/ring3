import assert from 'node:assert/strict';
import {mkdirSync} from 'node:fs';
import {harness,PC,SEG,BASES,state,registers,record,exit,writeRAM} from './support.mjs';
const[enginePath,output,root]=process.argv.slice(2);assert.ok(enginePath&&output&&root,'explicit selected integrated engine, fresh output and root');mkdirSync(output,{recursive:true});
const h=await harness(enginePath,output,root);
const CORE=[];
for(const kind of ['load','store']){
  const op=kind==='load'?'a0':'a2';
  CORE.push({id:'mov8-'+kind+'-high',hex:op+'0f000000',width:1,offset:15,kind},{id:'mov8-'+kind+'-low',hex:op+'10000000',width:1,offset:16,kind},
    {id:'mov16-'+kind,hex:(kind==='load'?'66a1':'66a3')+'0f000000',width:2,offset:15,kind},{id:'mov32-'+kind,hex:(kind==='load'?'a1':'a3')+'0e000000',width:4,offset:14,kind});
}
for(const bits of [32,16])for(const [kind,hex,width,offset,sp,nextSP]of [['pop32','58',4,14,14,18],['pop16','6658',2,15,15,17],['push32','50',4,14,18,14],['push16','6650',2,15,17,15]])
  CORE.push({id:kind+'-B'+bits,hex,width,offset,sp,nextSP,bits,stack:true,kind:kind.startsWith('pop')?'load':'store'});
const find=id=>{const value=CORE.find(row=>row.id===id);assert.ok(value,id);return value;};
const CASES=CORE.map(value=>({...value}));
for(const id of ['mov32-load','pop32-B32'])for(const [label,patch]of [['low-unmapped',{low:null,fault:[1,0,1]}],['low-read-denied',{low:2,fault:[2,0,1]}],['high-unmapped',{high:null,fault:[1,0xfffffffe,1]}],['logical-denied',{high:null,low:null,logical:true}]])CASES.push({...find(id),...patch,id:id+':'+label});
for(const id of ['mov32-store','push32-B32'])for(const [label,patch]of [['low-unmapped',{low:null,fault:[1,0,2]}],['low-write-denied',{low:1,fault:[2,0,2]}],['high-unmapped',{high:null,fault:[1,0xfffffffe,2]}],['high-write-denied',{high:1,fault:[2,0xfffffffe,2]}],['logical-denied',{high:null,low:null,logical:true}]])CASES.push({...find(id),...patch,id:id+':'+label});
CASES.push({...find('mov16-load'),id:'mov16-load:low-unmapped',low:null,fault:[1,0,1]},{...find('mov16-store'),id:'mov16-store:low-write-denied',low:1,fault:[2,0,2]});
const ATOMIC=[{id:'xchg8',hex:'86050f000000',width:1,offset:15,kind:'modify'},{id:'xchg16',hex:'6687050f000000',width:2,offset:15,kind:'modify'},{id:'xchg32',hex:'87050e000000',width:4,offset:14,kind:'modify'}];
CASES.push(...ATOMIC);
for(const[label,patch]of [['low-read-denied',{low:2,fault:[2,0,1]}],['low-write-denied',{low:1,fault:[2,0,2]}],['read-frontier-precedes-high-write-denial',{high:1,low:2,fault:[2,0,1]}]])CASES.push({...ATOMIC[2],...patch,id:'xchg32:'+label});
for(const id of ['mov32-load','pop32-B16'])for(const key of ['base','D16'])for(const fault of [false,true])CASES.push({...find(id),id:id+':late-'+key+(fault?'-fault':'-success'),key,...(fault?{low:null,fault:[1,0,1]}:{})});
for(const source of [find('mov32-store'),find('push32-B16'),ATOMIC[2]]){
  for(const key of ['base','D16'])CASES.push({...source,id:source.id+':stateful-'+key,key});
  CASES.push({...source,id:source.id+':stateful-SMC',smc:true});
}
assert.equal(CASES.length,59,'finite scalar/wire/protocol inventory');
function editKey(bytes,key){if(key==='base')bytes.writeUInt32LE(0x4000,BASES+20);else bytes.writeUInt32LE(0xf1bfffff,SEG+32);}
function loaded(spec){return spec.width===1?(spec.offset===16?0x33:0x22):spec.width===2?0x3322:0x44332211;}
function lowValue(value,width){return width===4?value>>>0:value&((1<<(width*8))-1);}
function merge(value,old,width){return width===4?value>>>0:((old&(~((1<<(width*8))-1)))|value)>>>0;}
function seed(c,spec){const regs=[...registers];regs[0]=spec.kind==='load'?0xdeadbeef:0x89abcdef;if(spec.stack)regs[4]=(spec.bits===16?0xabcd0000:0)+(spec.sp??14);
  const context=spec.stack?{ssBase:0xfffffff0,ssLimit:spec.logical?0x10:0xff}:{dsBase:0xfffffff0,dsLimit:spec.logical?0x10:0xff};
  return state(h.arena(c),{regs,...context,mode:spec.bits===16?'D32/B16':'D32/B32',flags:spec.key?0x10002:2,version:spec.key?3:1,shadow:spec.key?1:0});}
try{
for(const resident of [false,true])for(const entries of [false,true])for(const spec of CASES){
  const hp=spec.high===undefined?3:spec.high,lp=spec.low===undefined?3:spec.low;
  const c=h.open(resident,entries,spec.id,[[0x1000],...(hp!==null?[[0xfffff000]]:[]),...(lp!==null?[[0]]:[])]);
  if(hp!==null)h.upload(c,0xfffffffe,Buffer.from('1122','hex'));if(lp!==null)h.upload(c,0,Buffer.from('3344','hex'));
  const instruction=Buffer.from(spec.hex,'hex');h.upload(c,PC,Buffer.concat([instruction,Buffer.from('ebfe','hex')]));
  let spans=[[PC,instruction.length+2]];
  if(spec.smc){h.upload(c,0x100,Buffer.from('90ebfe','hex'));h.upload(c,0xfffff100,Buffer.from('90ebfe','hex'));spans=[[0x100,3],...spans,[0xfffff100,3]];}
  h.protect(c,0x1000,5);if(hp!==null)h.protect(c,0xfffff000,spec.smc?7:hp);if(lp!==null)h.protect(c,0,spec.smc?7:lp);
  const widthBits=spec.width*8,name=spec.kind==='load'?'cpu_read'+widthBits:spec.kind==='store'?'cpu_store'+(resident?'_resident':'')+widthBits:'cpu_exchange'+(resident?'_resident':'')+widthBits;
  h.put(c,0,seed(c,spec));const material=h.compile(c,'confirmed CPU v10 '+spec.id,spans,[name]);
  if(!material){h.skip(c,'literal CPU scalar '+spec.id,'explicit CPU bus material');continue;}
  const input=seed(c,spec),want=Buffer.from(input),wantRAM=h.literalRAM(c),address=(0xfffffff0+spec.offset)>>>0;
  want.writeUInt32LE(input.readUInt32LE(52)&~0x10000,52);want.writeUInt32LE(0,12);
  const value=loaded(spec),source=lowValue(input.readUInt32LE(16),spec.width);
  let helpers=[];
  if(spec.logical)exit(want,spec.stack?15:11,0,6);
  else{
    const fields=spec.fault?[1,0,spec.fault[0],spec.fault[1],spec.fault[2],spec.width]:[0,spec.kind==='store'?0:value,0,0,0,spec.width];want.set(record('R3MH',40,fields,10),100);
    helpers=[{name,args:[...(spec.kind!=='load'&&resident?material.binding:[]),address,...(spec.kind==='load'?[]:[source])],status:spec.smc?11:0}];
    const stateful=spec.kind!=='load',commit=!spec.fault&&(!spec.key||stateful);
    if(commit){if(spec.kind!=='store')want.writeUInt32LE(merge(value,input.readUInt32LE(16),spec.width),16);if(spec.stack)want.writeUInt32LE((spec.bits===16?0xabcd0000:0)+spec.nextSP,32);want.writeUInt32LE(PC+instruction.length,48);}
    if(!spec.fault&&stateful){const bytes=Buffer.alloc(spec.width);if(spec.width===4)bytes.writeUInt32LE(source);else if(spec.width===2)bytes.writeUInt16LE(source);else bytes[0]=source;writeRAM(wantRAM,address,bytes);}
    if(spec.key){editKey(want,spec.key);c.afterHelper={name,run:()=>{const live=h.arena(c);editKey(live,spec.key);h.put(c,0,live);}};exit(want,3,commit?1:0,6);}
    else if(spec.fault)exit(want,5,0,6,[spec.fault[0],spec.fault[1],spec.fault[2],spec.width]);
    else exit(want,spec.smc?6:1,1,6);
  }
  h.observe(c,'literal CPU bus '+spec.id,{input,want,wantRAM,helpers,budget:spec.key&&spec.kind!=='load'?2:1});
  if(spec.smc){const after=h.arena(c);h.observe(c,'held SMC material refuses '+spec.id,{input:after,want:after,status:4});}
  h.host(c,'close');
}
}catch(error){h.failure('scalar wrap fixture setup',error);}
h.finish([
  'Explicit CPU-only scalar v10 imports and existing R3EXv6, no change to old diagnostic APIs/packets. Selected engine/source build receipt is required.',
  'Logical segment bounds are checked before translated pages. Valid logical bytes wrap modulo32; faults name actual high/low pages, never fabricated linear AddressOverflow.',
  'All mapped RAM starts from independent a5/literal uploads and all arena bytes/GPRs/flags/ESP/numeric/context/helper fields are checked. Real imports only, exact traces retained.',
  'Stateful late-key rows use budget2 so the next safepoint admits the post-retirement key check; unchanged budget-before-key priority is preserved. Late-key hooks call the real read/store/atomic export first, then apply a valid CSbase/D edit. Reads return NeedCode without consuming value/fault or GPR/ESP; stateful writes commit before rerouting.',
  'SMC observes one logical atomic publication and retirement before CodeInvalidated; held material refuses neutrally. Exact versiondelta1 is a private/native memory proof, not inferred from public staleness.',
  'This frozen scalar fixture does not claim compound-stack, SIMD, x87, xstate, masked or far closure. Those owner-specific consumers are separate.'
],{contexts:236,modules:236,moduleRuns:248,rows:484});
