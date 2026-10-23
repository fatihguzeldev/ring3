import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync,writeFileSync} from 'node:fs';
import {join} from 'node:path';

const directory = process.argv[2], CTX = 0x100, MAP = 0x10000, PERM = 0x410000;
const EXEC = 0x510000, VERSION = 0x510100, PAGE_VERSION = 0x510200, BACKING = 0x520000;
const memory = new WebAssembly.Memory({initial:96}), bytes = new Uint8Array(memory.buffer), d = new DataView(memory.buffer);
const put = (offset,value) => d.setUint32(offset,value>>>0,true), get = offset => d.getUint32(offset,true);
const hex = value => Buffer.from(value).toString('hex');
const hash = value => createHash('sha256').update(value).digest('hex');
const image = readFileSync(join(directory,'code.bin')), regions = new Map(), cases = [];
const reportPath = join(directory,'frontier-result.json');
const checkpoint = () => writeFileSync(reportPath,JSON.stringify({complete:false,cases},null,2));
let slowReads = 0;
function read(context,address,width) {
  assert.equal(context,CTX); assert.equal(address>>>0,0x3000); assert.equal(width,0x104);
  assert.equal(bytes[PERM+3],1); slowReads++;
  put(CTX+48,0x3000); put(CTX+52,0x20204); put(CTX+116,14);
  return 1n<<32n;
}
function write() {throw new Error('unexpected slow write in authored fixture');}
const u=value=>{const result=[];do{const byte=value&127;value>>>=7;result.push(byte|(value?128:0));}while(value);return result;};
function unsigned(source,at) {
  let value=0;
  for(let shift=0;shift<35;shift+=7) {
    assert(at.value<source.length);const byte=source[at.value++];
    assert(shift<28||(byte&0xf0)===0);value+=(byte&127)*2**shift;
    if(byte<128)return value;
  }
  assert.fail('unterminated section integer');
}
const section=(id,data)=>[id,...u(data.length),...data];
function countLookups(source) {
  const output=[...source.subarray(0,8)],at={value:8};let instrumented=false,exported=false;
  while(at.value<source.length) {
    const id=source[at.value++],length=unsigned(source,at),end=at.value+length;
    assert(end<=source.length);let data=source.subarray(at.value,end);at.value=end;
    assert.notEqual(id,6,'generated module has no existing globals');
    if(id===7) {
      output.push(...section(6,[1,0x7f,1,0x41,0,0x0b]));
      const cursor={value:0},count=unsigned(data,cursor),name=new TextEncoder().encode('test.lookup_calls');
      data=Uint8Array.from([...u(count+1),...data.subarray(cursor.value),...u(name.length),...name,3,0]);exported=true;
    } else if(id===10) {
      const cursor={value:0},count=unsigned(data,cursor),bodies=[];
      assert(count>=4&&count<=7);
      for(let index=0;index<count;index++) {
        const size=unsigned(data,cursor),bodyEnd=cursor.value+size;
        assert(bodyEnd<=data.length);let body=data.subarray(cursor.value,bodyEnd);cursor.value=bodyEnd;
        if(index===2) {
          const locals={value:0},groups=unsigned(body,locals);
          assert.equal(groups,0,'lookup has no declared locals');
          body=Uint8Array.from([...body.subarray(0,locals.value),0x23,0,0x41,1,0x6a,0x24,0,...body.subarray(locals.value)]);
          instrumented=true;
        }
        bodies.push(...u(body.length),...body);
      }
      assert.equal(cursor.value,data.length);data=Uint8Array.from([...u(count),...bodies]);
    }
    output.push(...section(id,data));
  }
  assert(instrumented&&exported);return Uint8Array.from(output);
}
for (const line of readFileSync(join(directory,'regions.tsv'),'utf8').trim().split('\n')) {
  const [name,entry,identity] = line.split('\t'), encoded = readFileSync(join(directory,`${name}.wasm`));
  const module = new WebAssembly.Module(encoded);
  assert.deepEqual(WebAssembly.Module.imports(module),[
    {module:'ring3',name:'memory',kind:'memory'},{module:'ring3',name:'read',kind:'function'},
    {module:'ring3',name:'write',kind:'function'},
  ]);
  const instrumented=countLookups(encoded),exports=new WebAssembly.Instance(new WebAssembly.Module(instrumented),{ring3:{memory,read,write}}).exports;
  regions.set(name,{name,entry:Number(entry),identity:BigInt(identity),run:exports.run,lookups:exports['test.lookup_calls'],sha256:hash(encoded),instrumentedSha256:hash(instrumented)});
}
function reset(name,eax=5,readonly=false) {
  bytes.fill(0,CTX,CTX+128); bytes.fill(0,BACKING,BACKING+3*4096);
  bytes.fill(0,EXEC,EXEC+12); bytes.fill(0,VERSION,VERSION+8); bytes.fill(0,PAGE_VERSION,PAGE_VERSION+48);
  for (const [page,slot,permission] of [[1,1,7],[3,2,readonly?1:3],[4,3,3]]) {
    put(MAP+page*4,slot); bytes[PERM+page]=permission;
  }
  bytes.set(image,BACKING); put(BACKING+4096,10); put(EXEC,1);
  [eax,0,0,0x3000,0x4800,0,0,0].forEach((value,index)=>put(CTX+index*4,value));
  const first = regions.get(name); put(CTX+32,first.entry); put(CTX+36,2); put(CTX+44,0x12345678);
  [MAP,PERM,BACKING,3,EXEC,0,VERSION,PAGE_VERSION,0].forEach((value,index)=>put(CTX+56+index*4,value));
  put(CTX+112,1); d.setBigUint64(CTX+120,first.identity,true); slowReads=0;
  return first;
}
function step(label,region,budget,expected) {
  d.setBigUint64(CTX+120,region.identity,true);
  const wanted = bytes.slice(CTX,CTX+128), c = new DataView(wanted.buffer), ram = bytes.slice(BACKING,BACKING+3*4096);
  c.setUint32(0,expected.eax,true); c.setUint32(32,expected.pc,true); c.setUint32(36,expected.flags,true);
  c.setUint32(40,expected.retired,true); c.setUint32(44,expected.remaining,true);
  if (expected.fault) {c.setUint32(48,0x3000,true);c.setUint32(52,0x20204,true);c.setUint32(116,14,true);}
  new DataView(ram.buffer).setUint32(4096,expected.data,true);
  const beforeSlow = slowReads;region.lookups.value=0;
  const stop = region.run(CTX,budget)>>>0;
  const record = {label,region:region.name,budget,stop,contextHex:hex(bytes.subarray(CTX,CTX+128)),
    ramHex:hex(bytes.subarray(BACKING,BACKING+3*4096)),slowReads:slowReads-beforeSlow,lookups:region.lookups.value};
  cases.push(record); checkpoint();
  assert.equal(stop,expected.stop,`${label}: exact boundary`);
  assert.equal(record.slowReads,expected.fault?1:0,`${label}: no work beyond frontier`);
  assert.deepEqual(bytes.subarray(CTX,CTX+128),wanted,`${label}: complete context and exact prefix`);
  assert.deepEqual(bytes.subarray(BACKING,BACKING+3*4096),ram,`${label}: complete code/data/stack`);
  assert.equal(record.lookups,expected.lookups??1,`${label}: only runtime targets use dynamic lookup`);
}

for (const [budget,stop,pc,retired,eax,flags] of [[0,2,0x1000,0,5,2],[1,2,0x1001,1,6,6],[8,1,0x1001,1,6,6]]) {
  const first = reset('linear');
  step(`linear-budget-${budget}`,first,budget,{stop,pc,retired,eax,flags,remaining:budget-retired,data:10});
}
for (const [eax,data,flags,tailFlags] of [[0,12,6,0x12],[1,11,2,6]]) for (const budget of [4,8]) {
  const first = reset('fork',eax);
  step(`conditional-${eax}-budget-${budget}`,first,budget,{stop:budget===4?2:1,pc:0x1120,retired:4,eax,flags,
    remaining:budget-4,data});
  if (budget===8) step(`retained-tail-${eax}`,regions.get('fork-tail'),1,
    {stop:2,pc:0x1123,retired:5,eax,flags:tailFlags,remaining:0,data:data+4});
}
let first = reset('linear');
step('linear-prefix-before-handoff',first,8,{stop:1,pc:0x1001,retired:1,eax:6,flags:6,remaining:7,data:10});
step('linear-tail-own-budget',regions.get('linear-tail'),1,{stop:2,pc:0x1004,retired:2,eax:6,flags:6,remaining:0,data:12});
first = reset('linear',5,true);
step('frontier-does-not-touch-readonly-tail',first,8,{stop:1,pc:0x1001,retired:1,eax:6,flags:6,remaining:7,data:10});
step('readonly-tail-fault-preserves-prefix',regions.get('linear-tail'),1,
  {stop:3,pc:0x1001,retired:1,eax:6,flags:6,remaining:1,data:10,fault:true});
for(const budget of [0,1,2,3,4,9]) {
  const region=reset('direct-loop');
  const pc=budget===0?0x1300:budget%2===1?0x1304:0x1305;
  step(`direct-forward-and-backedge-budget-${budget}`,region,budget,
    {stop:2,pc,retired:budget,eax:5,flags:2,remaining:0,data:10});
}
for(const budget of [1,8]) {
  const region=reset('direct-unresolved');
  step(`unresolved-direct-budget-${budget}`,region,budget,
    {stop:budget===1?2:1,pc:0x1680,retired:1,eax:5,flags:2,remaining:budget-1,data:10});
}
first=reset('jump-before-fault',5,true);
step('budget-stops-after-direct-jump-before-fault',first,1,
  {stop:2,pc:0x1404,retired:1,eax:5,flags:2,remaining:0,data:10});
first=reset('jump-before-fault',5,true);
step('fault-after-resolved-direct-jump',first,3,
  {stop:3,pc:0x1404,retired:1,eax:5,flags:2,remaining:2,data:10,fault:true});
first=reset('fault-before-jump',5,true);
step('fault-before-direct-jump-keeps-the-source-pc',first,3,
  {stop:3,pc:0x1500,retired:0,eax:5,flags:2,remaining:3,data:10,fault:true});
first=reset('indirect-unresolved',0x1680);
step('indirect-jump-still-looks-up-the-runtime-target',first,8,
  {stop:1,pc:0x1680,retired:1,eax:0x1680,flags:2,remaining:7,data:10,lookups:2});
writeFileSync(reportPath,JSON.stringify({complete:true,sources:[import.meta.url].map(url=>({url,sha256:hash(readFileSync(new URL(url)))})),
  regions:[...regions.values()].map(({name,entry,identity,sha256,instrumentedSha256})=>({name,entry,identity:String(identity),sha256,instrumentedSha256})),cases},null,2));
console.log(JSON.stringify({complete:true,cases:cases.length}));
