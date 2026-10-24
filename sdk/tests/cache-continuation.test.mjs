import assert from 'node:assert/strict';
import test from 'node:test';
import {CodeCache} from '../runtime/cache.js';

const CONTEXT = 256, HEADER = 2048, DRIVER = 4096, TRACE = 8192;
const OWNERS = 16384, STATUS = 60000, CODE = 65536;
const OWNED = ['core_continuation_ptr','core_continuation_format','core_chain_resolve_owned',
  'core_numeric_owned','core_precise_owned','core_end_owned'];
const FIELDS = [...Array.from({length:12},(_,i)=>i+2),...Array.from({length:6},(_,i)=>i+20),99];
const GLOBALS = ['link.mode','link.valid','link.context',...FIELDS.map(i=>`link.local${i}`),
  'trace.arena','link.continuation','link.port'];
const u = value => {
  const bytes = [];
  do {const byte = value & 127; value >>>= 7; bytes.push(byte | (value ? 128 : 0));} while (value);
  return bytes;
};
const positive = value => {
  const bytes = u(value);
  if (bytes.at(-1) & 64) {bytes[bytes.length-1] |= 128; bytes.push(0);}
  return bytes;
};
const string = value => [...u(value.length),...new TextEncoder().encode(value)];
const section = (id,bytes) => [id,...u(bytes.length),...bytes];
const type = (parameters,results) => [0x60,...u(parameters.length),...parameters,...u(results.length),...results];
const TYPES = [
  type([0x7f,0x7f,0x7f],[0x7e]), type([0x7f,0x7f,0x7f,0x7f],[0x7f]),
  type([0x7f,0x7f],[0x7f]), type([0x7f],[]),
  type([0x7f,0x7f,0x7f],[0x7f]), type([0x7f],[0x7f]),
];
const wasm = sections => Uint8Array.from([0,97,115,109,1,0,0,0,...sections.flat()]);
const body = instructions => {const bytes=[0,...instructions,0x0b];return [...u(bytes.length),...bytes];};
const i32 = value => [0x41,...positive(value)];
const local = index => [0x20,...u(index)];
const store = (offset,value) => [...local(0),...i32(value),0x36,2,...u(offset)];

function driver(abi=5) {
  const imports = [5,...string('ring3'),...string('memory'),2,0,2,
    ...string('ring3'),...string('functions'),1,0x70,0,1,
    ...string('ring3'),...string('publishers'),1,0x70,0,1,
    ...string('ring3'),...string('probe'),0,4,...string('ring3'),...string('cold'),0,5];
  const globals = [4+GLOBALS.length+1,...Array.from({length:4},()=>[0x7f,1,0x41,0,0x0b]).flat(),
    ...GLOBALS.flatMap(name=>[name==='link.local99'?0x7e:0x7f,1,name==='link.local99'?0x42:0x41,0,0x0b]),
    0x7f,0,...i32(abi),0x0b];
  const exports = [6+GLOBALS.length,...string('run'),0,2,...string('regions'),3,0,
    ...string('coldRuns'),3,1,...string('coldWork'),3,2,...string('coldRetired'),3,3,
    ...GLOBALS.flatMap((name,index)=>[...string(name),3,...u(index+4)]),
    ...string('linkAbi'),3,...u(4+GLOBALS.length)];
  return wasm([section(1,[TYPES.length,...TYPES.flat()]),section(2,imports),section(3,[1,4]),
    section(6,globals),section(7,exports),section(10,[1,...body([
      ...i32(1),0x24,0,...local(0),...local(1),...local(2),0x11,2,0,
    ])])]);
}

function owner({callbacks=false,corruptNonce=false}={}) {
  const imports = [5+GLOBALS.length,...string('ring3'),...string('memory'),2,0,2,
    ...string('ring3'),...string('read'),0,0,...string('ring3'),...string('write'),0,1,
    ...string('ring3'),...string('precise'),0,5,...string('ring3'),...string('numeric'),0,4,
    ...GLOBALS.flatMap(name=>[...string('ring3'),...string(name),3,name==='link.local99'?0x7e:0x7f,1])];
  const calls = callbacks ? [...local(0),...i32(0x1000),...i32(7),0x10,3,0x1a,...local(0),0x10,2,0x1a] : [];
  const corrupt = corruptNonce ? [...i32(HEADER+104),0x42,2,0x37,3,0] : [];
  return wasm([section(1,[TYPES.length,...TYPES.flat()]),section(2,imports),section(3,[2,2,3]),
    section(7,[3,...string('run'),0,4,...string('link'),0,4,...string('publish'),0,5]),
    section(10,[2,...body([...calls,...store(32,0x1004),
      ...local(0),...local(0),0x28,2,40,...i32(1),0x6a,0x36,2,40,
      ...store(44,0),...corrupt,...i32(2)]),...body([])])]);
}

// these Wasm modules exercise the host boundary only; they are not CPU oracles.
function fixture(options={}) {
  const memory = new WebAssembly.Memory({initial:2});
  const calls = [], links = new Map(), arena = new Map([[48,0x1000]]);
  const control = {}, encoded = owner(options), dispatch = driver(options.abi ?? 5);
  const budget = options.callbacks ? 2 : 1;
  const data = () => new DataView(memory.buffer);
  const close = () => {
    data().setUint32(CONTEXT+112,0,true); data().setUint32(HEADER+4,0,true);
    data().setBigUint64(HEADER+104,0n,true);
  };
  const api = {
    core_prepare(pc) {
      calls.push(['prepare',pc]); new Uint8Array(memory.buffer,CODE,encoded.length).set(encoded);
      data().setUint32(CONTEXT+32,pc,true);data().setBigUint64(CONTEXT+120,1n,true);return 0;
    },
    core_code_ptr:()=>CODE,core_code_len:()=>encoded.length,core_context_ptr:()=>CONTEXT,
    core_begin() {
      calls.push(['begin']);
      data().setUint32(CONTEXT+112,1,true);data().setUint32(CONTEXT+44,budget,true);
      data().setUint32(HEADER,1,true);data().setUint32(HEADER+4,1,true);data().setUint32(HEADER+8,CONTEXT,true);
      data().setBigUint64(HEADER+16,1n,true);data().setUint32(HEADER+76,1,true);
      data().setBigUint64(HEADER+80,1n,true);data().setBigUint64(HEADER+88,7n,true);
      data().setBigUint64(HEADER+104,control.invalidNonce?2n:1n,true);return 0;
    },
    core_end() {calls.push(['end']);close();return 0;},
    core_import() {calls.push(['import']);data().setUint32(CONTEXT+32,arena.get(48),true);return 0;},
    core_export(reason,work) {calls.push(['export',reason,work]);arena.set(48,data().getUint32(CONTEXT+32,true));arena.set(72,1);return 0;},
    core_read:()=>0n,core_write:()=>0,
    core_numeric() {throw new Error('legacy numeric must not receive an owned callback');},
    core_precise() {throw new Error('legacy precise must not receive an owned callback');},
    core_dispatch_code_ptr() {new Uint8Array(memory.buffer,DRIVER,dispatch.length).set(dispatch);return DRIVER;},
    core_dispatch_code_len:()=>dispatch.length,
    core_link(slot) {calls.push(['link',slot]);links.set(slot,1n);return 0;},
    core_unlink(slot) {links.delete(slot);return 0;},
    core_link_owners_ptr() {
      new Uint8Array(memory.buffer,OWNERS,4097*8).fill(0);
      for(const [slot,id]of links)data().setBigUint64(OWNERS+slot*8,id,true);
      return OWNERS;
    },
    core_cache_victim:()=>1,
    core_cache_status_ptr() {
      [1n,BigInt(encoded.length),0n,0n,0n,BigInt(links.size),BigInt(encoded.length),4096n]
        .forEach((value,index)=>data().setBigUint64(STATUS+index*8,value,true));return STATUS;
    },
    core_chain_next:()=>0,core_chain_select:()=>0,core_chain_cold:()=>0,core_trace_pointer:()=>TRACE,
    core_continuation_ptr:()=>control.pointer ?? HEADER,core_continuation_format:()=>options.format ?? 1,
    core_chain_resolve_owned(...args) {calls.push(['resolve-owned',...args]);return 0;},
    core_numeric_owned(...args) {calls.push(['numeric-owned',...args]);return 0;},
    core_precise_owned(...args) {
      calls.push(['precise-owned',...args]);data().setUint32(CONTEXT+40,data().getUint32(CONTEXT+40,true)+1,true);
      data().setUint32(CONTEXT+44,data().getUint32(CONTEXT+44,true)-1,true);return 2;
    },
    core_end_owned(...args) {
      calls.push(['end-owned',...args]);
      if(control.endTrap)throw control.endTrap;
      if(control.endStatus)return control.endStatus;
      close();return 0;
    },
  };
  if(options.withoutOwned)for(const name of OWNED)delete api[name];
  if(options.missing)delete api[options.missing];
  const engine = {api,memory,get:offset=>arena.get(offset)??0,
    ok(name,...args) {const status=api[name](...args);if(status)throw new Error(`${name} returned ${status}`);}};
  return {engine,api,memory,calls,control,budget,data};
}

function retained(f) {
  const cache = new CodeCache(f.engine,{budget:f.budget}), unit = cache.entry(0x1000);
  assert.equal(unit.slot,1);
  return {cache,unit};
}

test('continuation exports and link ABI are negotiated as a complete format',()=>{
  for(const missing of OWNED) {
    const f=fixture({missing});
    assert.throws(()=>new CodeCache(f.engine),/incomplete core continuation exports/);
    assert.equal(f.calls.some(([name])=>name==='begin'),false);
  }
  for(const options of [{format:2},{abi:4},{withoutOwned:true}]) {
    const f=fixture(options);
    assert.throws(()=>new CodeCache(f.engine),/continuation (format|ABI)/);
    assert.equal(f.calls.some(([name])=>name==='begin'),false);
  }
});

test('real owner imports forward the current BigInt ticket to numeric, precise, and end',()=>{
  const f=fixture({callbacks:true}),{cache,unit}=retained(f);
  assert.throws(()=>cache.invokeNumeric(CONTEXT,0x1000,7),/continuation allocation/);
  assert.throws(()=>cache.invokePrecise(CONTEXT),/CPU lease/);
  const run=cache.execute(unit);
  assert.deepEqual([run.pc,run.coreReason,run.retired,run.work,run.regions],[0x1004,2,2,2,1]);
  assert.deepEqual(f.calls.filter(([name])=>name==='numeric-owned'),[['numeric-owned',CONTEXT,1,7n,0x1000,7]]);
  assert.deepEqual(f.calls.filter(([name])=>name==='precise-owned'),[['precise-owned',CONTEXT,1,7n]]);
  assert.deepEqual(f.calls.filter(([name])=>name==='end-owned'),[['end-owned',CONTEXT,1,7n,2]]);
  assert.equal(f.calls.filter(([name])=>name==='end').length,0);
  assert.deepEqual([cache.preciseRuns,cache.preciseWork,cache.preciseRetired],[1,1,1]);
  assert.equal(cache.open,false);assert.equal(f.data().getUint32(CONTEXT+112,true),0);
});

for(const failure of ['pointer','nonce','late-nonce'])test(`invalid continuation ${failure} after begin closes the native lease`,()=>{
  const f=fixture({corruptNonce:failure==='late-nonce'}),{cache,unit}=retained(f);
  if(failure==='pointer')f.control.pointer=0;
  if(failure==='nonce')f.control.invalidNonce=true;
  assert.throws(()=>cache.execute(unit),/invalid CPU continuation (allocation|lease)/);
  assert.equal(f.calls.filter(([name])=>name==='begin').length,1);
  assert.equal(f.calls.filter(([name])=>name==='end').length,1);
  assert.equal(f.calls.filter(([name])=>name==='end-owned').length,0);
  assert.equal(cache.open,false);assert.equal(f.data().getUint32(CONTEXT+112,true),0);
  assert.equal(f.data().getUint32(HEADER+4,true),0);assert.equal(cache.runs,0);
  assert.equal(f.data().getUint32(CONTEXT+40,true),failure==='late-nonce'?1:0);
  assert.equal(cache.units.get(unit.id),unit);assert.equal(cache.table.get(unit.slot),unit.link);
});

for(const failure of ['reject','trap'])test(`owned end ${failure} cannot strand a valid lease`,()=>{
  const f=fixture(),{cache,unit}=retained(f),trap=new Error('owned end trap');
  if(failure==='reject')f.control.endStatus=7;else f.control.endTrap=trap;
  assert.throws(()=>cache.execute(unit),failure==='reject'?/core_end returned 7/:error=>error===trap);
  assert.deepEqual(f.calls.filter(([name])=>['begin','end-owned','end'].includes(name)),
    [['begin'],['end-owned',CONTEXT,1,7n,2],['end']]);
  assert.equal(cache.open,false);assert.equal(f.data().getUint32(CONTEXT+112,true),0);
  assert.equal(f.data().getUint32(HEADER+4,true),0);assert.equal(cache.runs,0);
  assert.equal(f.data().getUint32(CONTEXT+40,true),1);
  assert.equal(cache.units.get(unit.id),unit);assert.equal(cache.table.get(unit.slot),unit.link);
});
