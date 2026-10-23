import assert from 'node:assert/strict';
import test from 'node:test';
import {CodeCache} from '../runtime/cache.js';

const i32=0x7f,i64=0x7e,f64=0x7c;
const u=value=>{const bytes=[];do{const next=value&127;value>>>=7;bytes.push(next|(value?128:0));}while(value);return bytes;};
const positive=value=>{const bytes=u(value);if(bytes.at(-1)&64){bytes[bytes.length-1]|=128;bytes.push(0);}return bytes;};
const str=value=>[...u(value.length),...new TextEncoder().encode(value)];
const section=(kind,bytes)=>[kind,...u(bytes.length),...bytes];
const signature=(parameters,results)=>[0x60,...u(parameters.length),...parameters,...u(results.length),...results];
const body=ops=>{const bytes=[0,...ops,0x0b];return [...u(bytes.length),...bytes];};
const constant=value=>[0x41,...positive(value)];
const get=index=>[0x20,...u(index)];
const header=[0,97,115,109,1,0,0,0];
const helpers=[
  ['support.flags',5],['support.materialize_numeric',6],['support.resolve_status',7],
  ['support.translate_page',8],['support.acquire_group',9],['support.store8',10],
  ['support.store16',10],['support.store32',10],['support.normalize_kernel',11],
];

function types({wrongFlagsType=false,wrongFlagsArity=false}={}) {
  const flags=Array(8-Number(wrongFlagsArity)).fill(i32);if(wrongFlagsType)flags[0]=f64;
  return [12,
    ...signature([i32,i32],[i32]),...signature([i32,i32,i32],[i64]),
    ...signature([i32,i32,i32,i32],[i32]),...signature([i32,i32,i32],[i32]),
    ...signature([i32],[i32]),...signature(flags,[i32]),
    ...signature([...Array(8).fill(f64),...Array(10).fill(i32)],[]),
    ...signature([i32,i32,f64,f64,f64],[i32]),
    ...signature([i32,i32,i32],[i32,i32,i32]),...signature(Array(9).fill(i32),[i32,i32,i32]),
    ...signature([i32,i32,i32,i32,i32,i64],[i32,i64]),
    ...signature([...Array(8).fill(f64),...Array(8).fill(i64),...Array(14).fill(i32),i64,i64],
      [...Array(8).fill(f64),...Array(8).fill(i64),...Array(14).fill(i32)]),
  ];
}

function driver({abi=1,omit,mutableAbi=false,...typeOptions}={}) {
  const emitted=helpers.filter(([name])=>name!==omit);
  const imports=[3,...str('ring3'),...str('memory'),2,0,2,
    ...str('ring3'),...str('functions'),1,0x70,1,1,...u(4097),
    ...str('ring3'),...str('next'),0,4];
  const exports=[...str('run'),0,1,...str('regions'),3,0,...str('supportCalls'),3,1,
    ...(abi===null?[]:[...str('supportAbi'),3,2]),
    ...emitted.flatMap(([name],index)=>[...str(name),0,index+2])];
  const code=emitted.flatMap(([name])=>{
    if(name==='support.flags')return body([0x23,1,...constant(1),0x6a,0x24,1,
      ...constant(1024),...get(3),0x36,2,0,...get(3)]);
    if(name==='support.materialize_numeric')return body([]);
    if(name==='support.resolve_status')return body(get(1));
    if(name==='support.translate_page'||name==='support.acquire_group')return body([...constant(0),...constant(0),...constant(0)]);
    if(name.startsWith('support.store'))return body([...constant(0),...get(5)]);
    return body(Array.from({length:30},(_,index)=>get(index)).flat());
  });
  return Uint8Array.from([...header,...section(1,types(typeOptions)),...section(2,imports),
    ...section(3,[1+emitted.length,3,...emitted.map(([,kind])=>kind)]),
    ...section(6,[2+Number(abi!==null),i32,1,...constant(0),0x0b,i32,1,...constant(0),0x0b,
      ...(abi===null?[]:[i32,Number(mutableAbi),...constant(abi),0x0b])]),
    ...section(7,[3+Number(abi!==null)+emitted.length,...exports]),
    ...section(10,[1+emitted.length,...body([...constant(1),0x24,0,...get(0),...get(1),...get(2),0x11,0,0]),...code])]);
}

function owner(pc,{omit}={}) {
  const emitted=helpers.filter(([name])=>name!==omit);
  const imports=[4+emitted.length,...str('ring3'),...str('memory'),2,0,2,
    ...str('ring3'),...str('read'),0,1,...str('ring3'),...str('write'),0,2,
    ...emitted.flatMap(([name,kind])=>[...str('ring3'),...str(name),0,kind]),
    ...str('ring3'),...str('support.abi'),3,i32,0];
  const mark=pc===0x1000?0x1234:0x5678;
  const ops=[...get(0),...[2,1,32,mark,0,mark,0,0x8d5].flatMap(constant),0x10,2,0x36,2,0,
    ...get(0),...constant(pc+4),0x36,2,32,
    ...get(0),...get(0),0x28,2,40,...constant(1),0x6a,0x36,2,40,
    ...get(0),...get(1),...constant(1),0x6b,0x36,2,44,...constant(2)];
  return Uint8Array.from([...header,...section(1,types()),...section(2,imports),
    ...section(3,[1,0]),...section(7,[1,...str('run'),0,2+emitted.length]),
    ...section(10,[1,...body(ops)])]);
}

function fixture(driverOptions,ownerOptions) {
  const memory=new WebAssembly.Memory({initial:2}),context=256,calls=[],links=new Map(),encodedDriver=driver(driverOptions);
  const d=()=>new DataView(memory.buffer);let encoded,identity=0n;
  const api={
    core_prepare(pc){calls.push(['prepare',pc]);identity=pc===0x1000?1n:2n;encoded=owner(pc,ownerOptions);
      d().setUint32(context+32,pc,true);d().setBigUint64(context+120,identity,true);
      new Uint8Array(memory.buffer,65536,encoded.length).set(encoded);return 0;},
    core_code_ptr:()=>65536,core_code_len:()=>encoded.length,core_context_ptr:()=>context,
    core_dispatch_code_ptr(){new Uint8Array(memory.buffer,4096,encodedDriver.length).set(encodedDriver);return 4096;},
    core_dispatch_code_len:()=>encodedDriver.length,
    core_begin:()=>0,core_end:()=>0,core_import:()=>0,core_export:()=>0,
    core_read(){throw Error('canonical read must not service shared support');},
    core_write(){throw Error('canonical write must not service shared support');},core_numeric:()=>0,
    core_link(slot){links.set(slot,identity);return 0;},core_unlink(slot){links.delete(slot);return 0;},
    core_chain_next:()=>0,core_cache_victim:()=>0,
    core_link_owners_ptr(){new Uint8Array(memory.buffer,16384,4097*8).fill(0);
      for(const[slot,id]of links)d().setBigUint64(16384+slot*8,id,true);return 16384;},
    core_cache_status_ptr:()=>49168,
  };
  const engine={api,memory,get:offset=>offset===48?0x1000:0,
    ok(name,...args){assert.equal(api[name](...args),0);}};
  return {engine,calls,d,context};
}

test('owners call one per-CPU typed Wasm support instance directly',()=>{
  const f=fixture(),cache=new CodeCache(f.engine,{chain:true,budget:1});
  const first=cache.entry(0x1000),second=cache.entry(0x2000);
  cache.entry(0x1000);const a=cache.execute(first);
  assert.deepEqual([a.coreReason,a.work,a.retired],[2,1,1]);
  assert.equal(f.d().getUint32(f.context,true),0x1234);
  cache.entry(0x2000);const b=cache.execute(second);
  assert.deepEqual([b.coreReason,b.work,b.retired],[2,1,1]);
  assert.equal(f.d().getUint32(f.context,true),0x5678);
  assert.equal(f.d().getUint32(1024,true),0x5678,'support Wasm writes the same CPU memory');
  assert.equal(cache.dispatch.supportCalls.value,2,'both owners call the one dispatcher kernel');
  assert.equal(cache.units.get(first.id),first);assert.equal(cache.units.get(second.id),second);
  assert.equal(cache.stats().admissions,2);
  const other=fixture(),isolated=new CodeCache(other.engine,{chain:true,budget:1});
  assert.equal(isolated.dispatch.supportCalls.value,0);assert.equal(other.d().getUint32(1024,true),0);
  isolated.execute(isolated.entry(0x1000));assert.equal(isolated.dispatch.supportCalls.value,1);
  assert.equal(cache.dispatch.supportCalls.value,2,'a different CPU never shares support globals or memory');
});

for(const [name,options]of [
  ['missing ABI',{abi:null}],['unknown ABI',{abi:2}],
  ['partial functions',{omit:'support.normalize_kernel'}],['wrong function arity',{wrongFlagsArity:true}],
])test(`support negotiation rejects ${name}`,()=>{
  const f=fixture(options);assert.throws(()=>new CodeCache(f.engine,{chain:true}),/support.*ABI|support exports/);
});

test('support negotiation rejects a partial owner import set before admission',()=>{
  const f=fixture({}, {omit:'support.materialize_numeric'}),cache=new CodeCache(f.engine,{chain:true});
  assert.throws(()=>cache.entry(0x1000),/incomplete core support imports/);
  assert.equal(cache.stats().admissions,0);assert.equal(cache.units.size,0);
});

for(const[name,options]of [
  ['same-arity wrong numeric signature',{wrongFlagsType:true}],['mutable ABI global',{mutableAbi:true}],
])test(`Wasm linking rejects ${name}`,()=>{
  const f=fixture(options),cache=new CodeCache(f.engine,{chain:true});
  assert.throws(()=>cache.entry(0x1000),WebAssembly.LinkError);
  assert.equal(cache.stats().admissions,0);assert.equal(cache.units.size,0);
});
