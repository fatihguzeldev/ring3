import assert from 'node:assert/strict';
import test from 'node:test';
import {CodeCache} from '../runtime/cache.js';
import {bindNativeRecovery,recoveryBlob,moduleRecovery,inspectRecoveryBlob,retainedRecoveryBase} from '../runtime/recovery.js';

const u=value=>{const bytes=[];do{const byte=value&127;value>>>=7;bytes.push(byte|(value?128:0));}while(value);return bytes;};
const signed=value=>{let n=BigInt(value),bytes=[];while(true){const byte=Number(n&127n);n>>=7n;const done=(n===0n&&!(byte&64))||(n===-1n&&(byte&64));bytes.push(byte|(done?0:128));if(done)return bytes;}};
const str=value=>[...u(value.length),...new TextEncoder().encode(value)];
const section=(id,bytes)=>[id,...u(bytes.length),...bytes];
const body=ops=>{const bytes=[0,...ops,0x0b];return[...u(bytes.length),...bytes];};
const constant=value=>[0x41,...signed(value)];
const get=index=>[0x20,index];
const blob=(identity,value=0x12345678n,version=2)=>{
  const header=version===1?40:48,bytes=new Uint8Array(header+16),d=new DataView(bytes.buffer);
  [0x44523352,version,1,1,1,1,header,header+8].forEach((word,index)=>d.setUint32(index*4,word,true));
  d.setBigUint64(32,identity,true);d.setBigUint64(header+8,BigInt.asUintN(64,value),true);return bytes;
};
function owner(identity=1n,value=0x12345678n,{payload=blob(identity,value),name='recovery.base'}={}) {
  const imports=[4,...str('ring3'),...str('memory'),2,0,1,
    ...str('ring3'),...str('read'),0,0,...str('ring3'),...str('write'),0,1,
    ...str('ring3'),...str(name),3,0x7f,0];
  const dataOffset=new DataView(payload.buffer,payload.byteOffset,payload.byteLength).getUint32(28,true);
  const run=[...get(0),...get(1),0x36,2,44,
    ...get(0),0x29,3,120,0x42,...signed(identity),0x52,0x04,0x40,...constant(1),0x0f,0x0b,
    ...get(0),0x23,0,0x29,0,...u(dataOffset),0xa7,0x36,2,0,
    ...get(0),...constant(0x2000),0x36,2,32,
    ...get(0),...get(0),0x28,2,40,...constant(1),0x6a,0x36,2,40,
    ...get(0),...get(1),...constant(1),0x6b,0x36,2,44,...constant(2)];
  return Uint8Array.from([0,97,115,109,1,0,0,0,
    ...section(1,[4,0x60,3,0x7f,0x7f,0x7f,1,0x7e,0x60,4,0x7f,0x7f,0x7f,0x7f,1,0x7f,
      0x60,2,0x7f,0x7f,1,0x7f,0x60,0,1,0x7e]),
    ...section(2,imports),...section(3,[2,2,3]),
    ...section(7,[2,...str('run'),0,2,...str('word'),0,3]),
    ...section(10,[2,...body(run),...body([0x23,0,0x29,0,...u(dataOffset)])]),
    ...section(0,[...str('ring3.recovery'),...payload])]);
}
function native(source,identity=1n,pointer=1024) {
  const memory=new WebAssembly.Memory({initial:2});new Uint8Array(memory.buffer,pointer,source.length).set(source);
  return {memory,pointer,length:source.length,identity};
}
const instance=(module,memory,base)=>new WebAssembly.Instance(module,{ring3:{memory,
  read:()=>assert.fail('metadata never calls native read'),write:()=>assert.fail('metadata never calls native write'),
  'recovery.base':base}});

test('recovery binds the immutable selected native blob without allocation or writes',()=>{
  const source=owner(),module=new WebAssembly.Module(source),descriptor=native(source),before=new Uint8Array(descriptor.memory.buffer).slice();
  const base=bindNativeRecovery(module,source,descriptor),offset=recoveryBlob(source).offset;
  assert(base instanceof WebAssembly.Global);assert.equal(base.value,descriptor.pointer+offset);
  assert.throws(()=>{base.value=0;},TypeError);
  assert.equal(instance(module,descriptor.memory,base).exports.word(),0x12345678n);
  assert.deepEqual(new Uint8Array(descriptor.memory.buffer),before);
  assert.equal(descriptor.memory.buffer.byteLength,2*65536);
});

test('transformed modules bind original owner offset, not transformed section position',()=>{
  const source=owner(),descriptor=native(source),padding=section(0,[...str('diagnostic'),...new Array(257).fill(7)]);
  const transformed=Uint8Array.from([...source.slice(0,8),...padding,...source.slice(8)]);
  const module=new WebAssembly.Module(transformed),base=bindNativeRecovery(module,source,descriptor);
  assert.notEqual(recoveryBlob(transformed).offset,recoveryBlob(source).offset);
  assert.equal(base.value,descriptor.pointer+recoveryBlob(source).offset);
  assert.equal(instance(module,descriptor.memory,base).exports.word(),0x12345678n);
});

test('malformed counts, bounds and owner identity reject before any metadata access',()=>{
  const patches=[d=>d.setUint32(0,0,true),d=>d.setUint32(4,3,true),d=>d.setUint32(8,0,true),
    d=>d.setUint32(12,32,true),d=>d.setUint32(16,2,true),d=>d.setUint32(20,0,true),
    d=>d.setUint32(24,32,true),d=>d.setUint32(28,0xfffffff8,true),d=>d.setBigUint64(32,2n,true)];
  for(const patch of patches) {
    const payload=blob(1n);patch(new DataView(payload.buffer));
    const source=owner(1n,0n,{payload}),module=new WebAssembly.Module(source),descriptor=native(source),before=new Uint8Array(descriptor.memory.buffer).slice();
    assert.throws(()=>bindNativeRecovery(module,source,descriptor),/invalid core recovery/);
    assert.deepEqual(new Uint8Array(descriptor.memory.buffer),before);
  }
});

test('wrong span, memory, owner and mutated original module are neutral',()=>{
  const source=owner(),module=new WebAssembly.Module(source),descriptor=native(source);
  const cases=[{...descriptor,identity:2n},{...descriptor,pointer:0x100000000+descriptor.pointer},
    {...descriptor,pointer:descriptor.memory.buffer.byteLength-1},{...descriptor,length:source.length+1},
    {...descriptor,memory:new WebAssembly.Memory({initial:2})}];
  for(const candidate of cases) {
    const before=new Uint8Array(candidate.memory.buffer).slice();
    assert.throws(()=>bindNativeRecovery(module,source,candidate),/invalid core recovery/);
    assert.deepEqual(new Uint8Array(candidate.memory.buffer),before);
  }
  new Uint8Array(descriptor.memory.buffer)[descriptor.pointer+8]^=1;
  assert.throws(()=>bindNativeRecovery(module,source,descriptor),/invalid core recovery/);
});

test('retained owner binding survives another selection and rejects reused allocation bytes',()=>{
  const a=owner(1n,11n),b=owner(2n,22n),descriptor=native(a),module=new WebAssembly.Module(a);
  const base=bindNativeRecovery(module,a,descriptor),original=instance(module,descriptor.memory,base);
  new Uint8Array(descriptor.memory.buffer,8192,b.length).set(b);
  assert.equal(retainedRecoveryBase(module,descriptor.memory,base),base);
  assert.equal(original.exports.word(),11n);
  new Uint8Array(descriptor.memory.buffer,descriptor.pointer,b.length).set(b);
  assert.throws(()=>retainedRecoveryBase(module,descriptor.memory,base),/invalid core recovery/);
  assert.throws(()=>bindNativeRecovery(module,a,{...descriptor,identity:2n}),/invalid core recovery/);
});

function cacheFixture() {
  const memory=new WebAssembly.Memory({initial:4}),context=256,control={version:1n},records=new Map();
  const d=()=>new DataView(memory.buffer);let selected,begins=0;
  const api={
    core_prepare(pc) {
      const id=pc===0x1000?control.version:2n;
      if(!records.has(id)) {
        const source=owner(id,id*11n),pointer=id===2n?98304:65536;
        new Uint8Array(memory.buffer,pointer,source.length).set(source);records.set(id,{id,source,pointer});
      }
      selected=records.get(id);d().setUint32(context+32,pc,true);d().setBigUint64(context+120,id,true);return 0;
    },
    core_code_ptr:()=>selected.pointer,core_code_len:()=>selected.source.length,core_context_ptr:()=>context,
    core_begin:()=>{begins++;return 0;},core_end:()=>0,core_import:()=>0,core_export:()=>0,
    core_read:()=>assert.fail('no metadata native read'),core_write:()=>assert.fail('no metadata native write'),core_numeric:()=>0,
  };
  const engine={api,memory,get:()=>0x1000,ok(name,...args){assert.equal(api[name](...args),0);}};
  return {engine,context,d,control,records,get begins(){return begins;}};
}

test('cache exposes a per-unit immutable base without depending on later selection',()=>{
  const f=cacheFixture(),cache=new CodeCache(f.engine,{chain:false,budget:1});
  const a=cache.entry(0x1000),b=cache.entry(0x1100);
  assert(a.recoveryBase instanceof WebAssembly.Global);assert(b.recoveryBase instanceof WebAssembly.Global);
  assert.notEqual(a.recoveryBase.value,b.recoveryBase.value);
  cache.entry(0x1000);const run=cache.execute(a);
  assert.equal(run.coreReason,2);assert.equal(f.d().getUint32(f.context,true),11);
  const original=instance(new WebAssembly.Module(f.records.get(1n).source),f.engine.memory,a.recoveryBase);
  cache.entry(0x1100);assert.equal(original.exports.word(),11n);
});

test('removed cache instances and changed engine memory fail before execution',()=>{
  const f=cacheFixture(),cache=new CodeCache(f.engine,{chain:false,budget:1,capacity:1});
  const a=cache.entry(0x1000);cache.entry(0x1100);
  const before=new Uint8Array(f.engine.memory.buffer).slice(),begins=f.begins;
  assert.throws(()=>cache.execute(a),/not retained/);assert.equal(f.begins,begins);
  assert.deepEqual(new Uint8Array(f.engine.memory.buffer),before);
  const memory=f.engine.memory;f.engine.memory=new WebAssembly.Memory({initial:4});
  assert.throws(()=>cache.entry(0x1000),/bindings changed/);f.engine.memory=memory;
});

test('old instance identity guard rejects recycled native storage before reading metadata',()=>{
  const f=cacheFixture(),cache=new CodeCache(f.engine,{chain:false,budget:1});
  const old=cache.entry(0x1000);f.control.version=3n;const current=cache.entry(0x1000);
  assert.equal(old.recoveryBase.value,current.recoveryBase.value,'allocation reused with a new identity');
  f.d().setUint32(f.context,0x76543210,true);const retired=f.d().getUint32(f.context+40,true);
  const stale=cache.execute(old);assert.equal(stale.coreReason,1);assert.equal(stale.work,0);
  assert.equal(f.d().getUint32(f.context,true),0x76543210);assert.equal(f.d().getUint32(f.context+40,true),retired);
  const run=cache.execute(current);assert.equal(run.coreReason,2);assert.equal(f.d().getUint32(f.context,true),33);
});

function splitBlob(mode,dictionary,highCount) {
  const bits=Math.max(1,32-Math.clz32(dictionary-1)),packed=Math.ceil(dictionary*bits/64),
    entryBytes=mode===1?5:6,dictionaryOffset=48+8*packed,
    highTableOffset=dictionaryOffset+dictionary*entryBytes;
  const bytes=new Uint8Array(highTableOffset+4*highCount),d=new DataView(bytes.buffer);
  [0x44523352,2,dictionary,bits,packed,dictionary,48,dictionaryOffset]
    .forEach((value,index)=>d.setUint32(index*4,value,true));
  d.setBigUint64(32,1n,true);d.setUint32(40,mode,true);d.setUint32(44,highCount,true);
  for(let index=0;index<dictionary;index++) {
    d.setUint32(dictionaryOffset+index*entryBytes,index,true);
    if(mode===1)d.setUint8(dictionaryOffset+index*entryBytes+4,index%highCount);
    else d.setUint16(dictionaryOffset+index*entryBytes+4,index%highCount,true);
  }
  for(let index=0;index<highCount;index++)d.setUint32(highTableOffset+index*4,index,true);
  return bytes;
}

test('v2 compact dictionaries validate u8/u16 high indices and exact derived tails',()=>{
  for(const [mode,count,highs]of [[1,8,1],[1,1024,256],[2,1024,257]]) {
    const payload=splitBlob(mode,count,highs),source=owner(1n,0n,{payload}),module=new WebAssembly.Module(source),descriptor=native(source);
    const metadata=moduleRecovery(module);assert.equal(metadata.version,2);assert.equal(metadata.mode,mode);
    assert.equal(metadata.highCount,highs);assert.equal(metadata.byteLength,payload.length);
    const before=new Uint8Array(descriptor.memory.buffer).slice();
    assert(bindNativeRecovery(module,source,descriptor) instanceof WebAssembly.Global);
    assert.deepEqual(new Uint8Array(descriptor.memory.buffer),before);
    const invalid=payload.slice(),d=new DataView(invalid.buffer),at=d.getUint32(28,true)+4;
    if(mode===1&&highs<256)d.setUint8(at,highs);
    else if(mode===2)d.setUint16(at,highs,true);
    else {d.setUint32(44,highs-1,true);}
    assert.throws(()=>inspectRecoveryBlob(invalid),/invalid core recovery/);
    assert.throws(()=>inspectRecoveryBlob(payload.slice(0,-1)),/invalid core recovery/);
  }
});

test('v2 rejects invalid modes, raw high tables and non-beneficial split layouts',()=>{
  const raw=blob(1n);
  for(const [offset,value]of [[40,3],[44,1]]) {
    const invalid=raw.slice();new DataView(invalid.buffer).setUint32(offset,value,true);
    assert.throws(()=>inspectRecoveryBlob(invalid),/invalid core recovery/);
  }
  assert.throws(()=>inspectRecoveryBlob(splitBlob(1,1,1)),/invalid core recovery/);
  assert.throws(()=>inspectRecoveryBlob(splitBlob(1,1024,257)),/invalid core recovery/);
  assert.throws(()=>inspectRecoveryBlob(splitBlob(2,1024,256)),/invalid core recovery/);
});

test('frozen v1 raw blobs remain independently bindable for baseline controls',()=>{
  const payload=blob(1n,-0x123456789abcdefn,1),source=owner(1n,0n,{payload}),module=new WebAssembly.Module(source),descriptor=native(source);
  const metadata=moduleRecovery(module);assert.deepEqual([metadata.version,metadata.mode,metadata.packedOffset],[1,0,40]);
  assert.equal(instance(module,descriptor.memory,bindNativeRecovery(module,source,descriptor)).exports.word(),-0x123456789abcdefn);
});
