import {instantiateCore,coreImports} from './link.mjs';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync,writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {performance} from 'node:perf_hooks';
import {makeKernel} from './kernels.mjs';
import {runActualPrefix} from './actual-prefix.mjs';

const directory=process.argv[2], timing=process.argv.includes('--timing');
const CTX=0x100,MAP=0x10000,PERM=0x410000,EXEC=0x510000;
const VERSION=0x510100,PAGE_VERSION=0x510200,GENERATION=0x510300,OWNERS=0x510400,BACKING=0x520000;
const memory=new WebAssembly.Memory({initial:96}),bytes=new Uint8Array(memory.buffer),d=new DataView(memory.buffer);
const put=(offset,value)=>d.setUint32(offset,value>>>0,true),get=offset=>d.getUint32(offset,true);
const hash=value=>createHash('sha256').update(value).digest('hex'),hex=value=>Buffer.from(value).toString('hex');
const regions=new Map(),cases=[],runs=[],reportPath=join(directory,'call-closure-result.json');
const report={schema:1,complete:false,scope:'generated regions with authored memory and Wasm resolver; no native CoreSession or game claim',
  node:process.version,modules:[],cases,runs,sources:Object.fromEntries(['./call-closure.mjs','./link.mjs','./kernels.mjs']
    .map(path=>[path,hash(readFileSync(new URL(path,import.meta.url)))]))};
const checkpoint=()=>writeFileSync(reportPath,JSON.stringify(report,null,2));
let reads=0,writes=0,stage='initialization';
checkpoint();
function physical(address) {
  const slot=get(MAP+(address>>>12)*4);
  return slot?BACKING+(slot-1)*4096+(address&4095):null;
}
function preflight(address,width,access,modify=false) {
  const locations=[];
  for(let index=0;index<width;index++) {
    const current=(address+index)>>>0,location=physical(current),required=modify?3:access;
    if(location===null || (bytes[PERM+(current>>>12)]&required)!==required) {
      put(CTX+48,current);put(CTX+52,width|((modify?2:access)<<8)|((location===null?1:2)<<16));
      return null;
    }
    locations.push(location);
  }
  return locations;
}
function read(context,address,encodedWidth) {
  assert.equal(context,CTX);reads++;
  assert.equal(encodedWidth&~0x1ff,0);
  const width=encodedWidth&255,locations=preflight(address>>>0,width,1,!!(encodedWidth&0x100));
  if(!locations)return 1n<<32n;
  let value=0;
  for(let index=0;index<width;index++)value+=bytes[locations[index]]*2**(8*index);
  return BigInt(value>>>0);
}
function write(context,address,width,value) {
  assert.equal(context,CTX);writes++;
  const locations=preflight(address>>>0,width,2);
  if(!locations)return 1;
  for(let index=0;index<width;index++)bytes[locations[index]]=value>>>(8*index);
  const version=d.getBigUint64(VERSION,true)+1n;
  d.setBigUint64(VERSION,version,true);
  let invalidated=false;
  for(const page of new Set(Array.from({length:width},(_,index)=>((address+index)>>>0)>>>12))) {
    const slot=get(MAP+page*4)-1;
    d.setBigUint64(PAGE_VERSION+slot*16+8,version,true);
    invalidated ||= get(EXEC+slot*4)!==0;
  }
  if(invalidated)d.setBigUint64(GENERATION,d.getBigUint64(GENERATION,true)+1n,true);
  return invalidated?2:0;
}
for(const line of readFileSync(join(directory,'regions.tsv'),'utf8').trim().split('\n')) {
  const [name,identity,instructions]=line.split('\t'),encoded=readFileSync(join(directory,`${name}.wasm`));
  const module=new WebAssembly.Module(encoded);
  assert.deepEqual(coreImports(module),[
    {module:'ring3',name:'memory',kind:'memory'},{module:'ring3',name:'read',kind:'function'},
    {module:'ring3',name:'write',kind:'function'},
  ]);
  regions.set(name,{name,identity:BigInt(identity),module});
  report.modules.push({name,identity,instructions:Number(instructions),sha256:hash(encoded),wasmHex:hex(encoded)});
}
const nextEncoded=readFileSync(join(directory,'resolver.wasm')),driverEncoded=readFileSync(join(directory,'driver.wasm'));
const next=instantiateCore(new WebAssembly.Module(nextEncoded),{ring3:{memory}}).exports.next;
const table=new WebAssembly.Table({element:'anyfunc',initial:4,maximum:4097});
const publishers=new WebAssembly.Table({element:'anyfunc',initial:4,maximum:4097});
const driver=instantiateCore(new WebAssembly.Module(driverEncoded),{ring3:{memory,functions:table,publishers,probe:next,
  cold:()=>assert.fail('every authored call-closure successor is already compiled')}}).exports;
for(const unit of regions.values()) {
  const exports=instantiateCore(unit.module,{ring3:{memory,read,write,...driver}}).exports;
  unit.run=exports.run;unit.link=exports.link;unit.publish=exports.publish;
}
report.resolver={sha256:hash(nextEncoded),wasmHex:hex(nextEncoded)};
report.driver={sha256:hash(driverEncoded),wasmHex:hex(driverEncoded)};

function seed(closed,{n=3,variant='plain',dataPermission=3,stackPermission=3,unmapStack=false,ebx=0x3000}={}) {
  bytes.fill(0,CTX,CTX+128);bytes.fill(0,BACKING,BACKING+3*4096);
  bytes.fill(0,EXEC,EXEC+12);bytes.fill(0,VERSION,VERSION+8);
  bytes.fill(0,PAGE_VERSION,PAGE_VERSION+48);bytes.fill(0,GENERATION,GENERATION+8);
  const image=readFileSync(join(directory,`${variant}.bin`));
  assert.equal(image.length,4096);bytes.set(image,BACKING);
  const expectedImage=makeKernel('call',n).code;
  if(variant==='gate')expectedImage.set([0x0f,0x0b],0x45);
  assert.deepEqual(image,Buffer.from(expectedImage));
  for(const [page,slot,permission] of [[1,1,7],[3,2,dataPermission],[4,unmapStack?0:3,unmapStack?0:stackPermission]]) {
    put(MAP+page*4,slot);bytes[PERM+page]=permission;
  }
  [5,n,7,ebx,0x4800,0,0,0].forEach((value,index)=>put(CTX+index*4,value));
  put(CTX+32,0x1000);put(CTX+36,2);put(CTX+44,0x12345678);
  [MAP,PERM,BACKING,3,EXEC,0,VERSION,PAGE_VERSION,GENERATION].forEach((value,index)=>put(CTX+56+index*4,value));
  put(EXEC,1);put(CTX+112,1);
  const caller=regions.get(`${variant}-caller`),callee=regions.get(`${variant}-callee`),whole=regions.get(`${variant}-closed`);
  table.set(1,caller.link);table.set(2,callee.link);table.set(3,whole.link);
  publishers.set(1,caller.publish);publishers.set(2,callee.publish);publishers.set(3,whole.publish);
  d.setBigUint64(OWNERS,caller.identity,true);d.setBigUint64(OWNERS+8,callee.identity,true);
  d.setBigUint64(CTX+120,(closed?whole:caller).identity,true);reads=0;writes=0;
  return {context:bytes.slice(CTX,CTX+128),ram:bytes.slice(BACKING,BACKING+3*4096),caller,callee,whole};
}
function validate(initial,closed,budget,wanted,stop,regionCount) {
  const expected=initial.context.slice(),c=new DataView(expected.buffer),ram=initial.ram.slice();
  wanted.regs.forEach((value,index)=>c.setUint32(index*4,value>>>0,true));
  for(const [offset,value] of [[32,wanted.pc],[36,wanted.flags],[40,wanted.retired],[44,budget-wanted.retired],
    [48,wanted.fault?.[0]??0],[52,wanted.fault?.[1]??0],[116,wanted.gate??0]])c.setUint32(offset,value,true);
  c.setBigUint64(120,(closed?initial.whole:wanted.last==='callee'?initial.callee:initial.caller).identity,true);
  const image=new DataView(ram.buffer);
  if(wanted.calls)image.setUint32(8192+0x7fc,0x1005,true);
  image.setUint32(4096+12,wanted.data??0,true);
  if(wanted.smc)ram[0x40]=0x84;
  assert.equal(stop,wanted.stop,'literal stop reason');assert.equal(regionCount,closed?1:wanted.regions,'literal region count');
  assert.deepEqual(bytes.subarray(CTX,CTX+128),expected,'complete literal context');
  assert.deepEqual(bytes.subarray(BACKING,BACKING+3*4096),ram,'complete code/data/stack');
  const versions=new Uint8Array(48),v=new DataView(versions.buffer),data=wanted.data??0;
  if(wanted.calls)v.setBigUint64(2*16+8,BigInt(2*wanted.calls-1),true);
  if(data)v.setBigUint64(16+8,BigInt(2*data),true);
  if(wanted.smc)v.setBigUint64(8,2n,true);
  assert.deepEqual(bytes.subarray(PAGE_VERSION,PAGE_VERSION+48),versions,'all store versions');
  assert.equal(d.getBigUint64(VERSION,true),BigInt((wanted.calls??0)+data+(wanted.smc?1:0)));
  assert.equal(d.getBigUint64(GENERATION,true),wanted.smc?1n:0n);
  assert.equal(reads,wanted.reads??0);assert.equal(writes,wanted.writes??0);
}
function check(label,budget,options,wanted) {
  for(const closed of [false,true]) {
    stage=`${label}/${closed}`;
    if(label.startsWith('budget-')) {
      const writes=[];
      for(let call=1;call<=(wanted.calls??0);call++) {
        writes.push([0x47fc,4,0x1005]);
        if(call<=(wanted.data??0))writes.push([0x300c,4,call]);
      }
      runActualPrefix({code:readFileSync(join(directory,'plain.bin')),entry:0x1000,instructions:64,blocks:8},
        'call-closure',`${label}/${closed?'closed-source':'split-source'}`,budget,
        {regs:[5,options.n??3,7,0x3000,0x4800,0,0,0],flags:2,stackReturn:0,continueNeedCode:true},
        {reason:wanted.stop,pc:wanted.pc,retired:wanted.retired,regs:wanted.regs,flags:wanted.flags,writes},
        {onRow:row=>{row.comparisonArm=closed?'closed-source precise semantics':'split-source precise semantics';cases.push(row);checkpoint();}});
      continue;
    }
    const initial=seed(closed,options),stop=driver.run(CTX,budget,closed?3:1)>>>0;
    const row={label,closed,budget,stop,regions:driver.regions.value,contextHex:hex(bytes.subarray(CTX,CTX+128)),
      ramHex:hex(bytes.subarray(BACKING,BACKING+3*4096)),reads,writes};
    cases.push(row);checkpoint();validate(initial,closed,budget,wanted,stop,row.regions);row.complete=true;
  }
}
try {
  const regs=(eax,ecx,edx,esp=0x4800,ebx=0x3000)=>[eax,ecx,edx,ebx,esp,0,0,0];
  for(const [budget,pc,eax,ecx,edx,esp,flags,data,regions,last] of [
    [0,0x1000,5,3,7,0x4800,2,0,1,'caller'],[1,0x1040,5,3,7,0x47fc,2,0,1,'caller'],
    [2,0x1043,8,3,7,0x47fc,2,0,2,'callee'],[3,0x1045,8,3,15,0x47fc,6,0,2,'callee'],
    [4,0x1049,8,3,15,0x47fc,2,1,2,'callee'],[5,0x1005,8,3,15,0x4800,2,1,2,'callee'],
    [6,0x1006,8,2,15,0x4800,2,1,3,'caller'],[7,0x1000,8,2,15,0x4800,2,1,3,'caller'],
    [21,0x1008,14,0,10,0x4800,0x46,3,7,'caller'],
  ])check(`budget-${budget}`,budget,{}, {stop:2,pc,retired:budget,regs:regs(eax,ecx,edx,esp),flags,data,
    calls:Math.floor((budget+6)/7),regions,last});
  check('full-span-closed-versus-split',22,{}, {stop:4,pc:0x1008,retired:21,
    regs:regs(14,0,10),flags:0x46,data:3,calls:3,regions:7,last:'caller'});
  check('call-stack-fault',21,{unmapStack:true},{stop:3,pc:0x1000,retired:0,regs:regs(5,3,7),flags:2,
    regions:1,last:'caller',fault:[0x47fc,0x10204],writes:1});
  check('callee-rmw-fault',21,{dataPermission:1},{stop:3,pc:0x1045,retired:3,regs:regs(8,3,15,0x47fc),flags:6,
    calls:1,regions:2,last:'callee',fault:[0x300c,0x20204],reads:1});
  check('return-read-fault',21,{stackPermission:2},{stop:3,pc:0x1049,retired:4,regs:regs(8,3,15,0x47fc),flags:2,
    data:1,calls:1,regions:2,last:'callee',fault:[0x47fc,0x20104],reads:1});
  check('callee-code-write',21,{ebx:0x1034},{stop:6,pc:0x1049,retired:4,regs:regs(8,3,15,0x47fc,0x1034),flags:6,
    calls:1,regions:2,last:'callee',smc:true,writes:1});
  check('authoritative-callee-gate',21,{variant:'gate'},{stop:7,pc:0x1045,retired:3,regs:regs(8,3,15,0x47fc),flags:6,
    calls:1,regions:2,last:'callee',gate:77});
  if(timing) {
    const n=Number(process.env.RING3_CORE_CALL_ITERATIONS??131072),kernel=makeKernel('call',n);
    report.timing={iterations:n,work:kernel.retired,warmupsPerArm:2,rounds:8,
      scope:'one driver call; authored Wasm resolver, no Rust lookup/lease; no slow host helper calls'};
    const wanted={stop:4,pc:kernel.pc,retired:kernel.retired,regs:kernel.regs,flags:kernel.flags,data:n,calls:n,
      regions:2*n+1,last:'caller'};
    function timed(closed,phase,round,position) {
      stage=`${phase}/${round}/${position}/${closed}`;
      const initial=seed(closed,{n}),before=performance.now(),stop=driver.run(CTX,kernel.retired+1,closed?3:1)>>>0;
      const elapsedMs=performance.now()-before;
      const row={closed,phase,round,position,elapsedMs,stop,regions:driver.regions.value,
        contextHex:hex(bytes.subarray(CTX,CTX+128)),ramSha256:hash(bytes.subarray(BACKING,BACKING+3*4096)),reads,writes};
      runs.push(row);report.lastRamHex=hex(bytes.subarray(BACKING,BACKING+3*4096));checkpoint();
      validate(initial,closed,kernel.retired+1,wanted,stop,row.regions);row.complete=true;delete report.lastRamHex;
    }
    for(let warm=0;warm<2;warm++)for(const closed of [false,true])timed(closed,'warmup',warm,Number(closed));
    for(let round=0;round<8;round++)(round%2?[true,false,false,true]:[false,true,true,false])
      .forEach((closed,position)=>timed(closed,'timed',round,position));
    const median=values=>{const a=values.toSorted((x,y)=>x-y);return(a[a.length/2-1]+a[a.length/2])/2;};
    const splitMs=median(runs.filter(row=>row.phase==='timed'&&!row.closed).map(row=>row.elapsedMs));
    const closedMs=median(runs.filter(row=>row.phase==='timed'&&row.closed).map(row=>row.elapsedMs));
    report.timing={...report.timing,splitMs,closedMs,ratio:splitMs/closedMs};
  }
  report.complete=true;checkpoint();console.log(JSON.stringify({complete:true,cases:cases.length,timing:report.timing}));
}catch(error) {
  report.failure={stage,message:String(error),stack:error.stack};
  report.lastContextHex=hex(bytes.subarray(CTX,CTX+128));report.lastRamHex=hex(bytes.subarray(BACKING,BACKING+3*4096));
  checkpoint();throw error;
}
