import assert from 'node:assert/strict';
import test from 'node:test';
import {CodeCache} from '../runtime/cache.js';
import {callbackRecord} from '../runtime/callback.js';
import {DEFAULT_EXECUTION} from '../runtime/configuration.js';

const u = n => { const out = []; do { const b = n & 127; n >>>= 7; out.push(b | (n ? 128 : 0)); } while (n); return out; };
const positive = n => { const out = u(n); if (out.at(-1) & 64) {out[out.length - 1] |= 128; out.push(0);} return out; };
const string = s => [...u(s.length), ...new TextEncoder().encode(s)];
const section = (id, bytes) => [id, ...u(bytes.length), ...bytes];
function program(stop = 1, next = 0x2000, retired = 1, work = 1) {
  const imports = [3, ...string('ring3'), ...string('memory'), 2, 0, 2,
    ...string('ring3'), ...string('read'), 0, 0, ...string('ring3'), ...string('write'), 0, 1];
  const body = [0, 0x20, 0, 0x41, ...positive(next), 0x36, 2, 32,
    0x20, 0, 0x20, 0, 0x28, 2, 40, 0x41, retired, 0x6a, 0x36, 2, 40,
    0x20, 0, 0x20, 1, 0x41, work, 0x6b, 0x36, 2, 44, 0x41, stop, 0x0b];
  return Uint8Array.from([0,97,115,109,1,0,0,0,
    ...section(1, [3,0x60,3,0x7f,0x7f,0x7f,1,0x7e,0x60,4,0x7f,0x7f,0x7f,0x7f,1,0x7f,0x60,2,0x7f,0x7f,1,0x7f]),
    ...section(2, imports), ...section(3,[1,2]), ...section(7,[1,...string('run'),0,2]),
    ...section(10,[1,...u(body.length),...body])]);
}
function driver() {
  const body = [1,2,0x7f,0x41,0,0x24,0,
    0x20,0,0x20,1,0x20,2,0x11,2,0,0x21,3,0x41,1,0x24,0,
    0x20,3,0x41,1,0x47,0x04,0x40,0x20,3,0x0f,0x0b,
    0x20,0,0x28,2,44,0x45,0x04,0x40,0x41,2,0x0f,0x0b,
    0x20,0,0x28,2,44,0x20,1,0x46,0x04,0x40,0x41,1,0x0f,0x0b,
    0x20,0,0x10,0,0x21,4,
    0x20,4,0x41,0x7f,0x46,0x04,0x40,0x41,8,0x0f,0x0b,
    0x20,4,0x45,0x04,0x40,0x41,1,0x0f,0x0b,
    0x20,0,0x20,0,0x28,2,44,0x20,4,0x11,2,0,0x21,3,
    0x41,2,0x24,0,0x20,3,0x0b];
  return Uint8Array.from([0,97,115,109,1,0,0,0,
    ...section(1,[3,0x60,1,0x7f,1,0x7f,0x60,3,0x7f,0x7f,0x7f,1,0x7f,0x60,2,0x7f,0x7f,1,0x7f]),
    ...section(2,[3,...string('ring3'),...string('memory'),2,0,2,
      ...string('ring3'),...string('functions'),1,0x70,1,1,...u(4097),
      ...string('ring3'),...string('next'),0,0]),
    ...section(3,[1,1]),...section(6,[1,0x7f,1,0x41,0,0x0b]),
    ...section(7,[2,...string('run'),0,1,...string('regions'),3,0]),
    ...section(10,[1,...u(body.length),...body])]);
}
function paddedProgram(encoded, padding = 0) {
  if (!padding) return encoded;
  const name = string('padding'), header = Uint8Array.from([0,...u(name.length + padding),...name]);
  const bytes = new Uint8Array(encoded.length + header.length + padding);
  bytes.set(encoded); bytes.set(header,encoded.length); return bytes;
}
function fixture(select = pc => ({id: 1n, stop: 1, next: pc + 4})) {
  const memory = new WebAssembly.Memory({initial: 2}), calls = [], arena = new Map([[48, 0x1000]]), control = {begin: () => 0};
  let context = 256, selected, encoded;
  const links = new Map(), recency = [], dispatch = driver(), codePointer = 65536, ownersPointer = 16384, statusPointer = 49168;
  const d = () => new DataView(memory.buffer);
  const touch = id => {const index=recency.indexOf(id); if(index>=0)recency.splice(index,1); recency.push(id);};
  const api = {
    core_prepare(pc, instructions, blocks) {
      calls.push(['prepare', pc, instructions, blocks]); selected = select(pc);
      control.prepare?.(pc,selected); touch(selected.id);
      encoded = paddedProgram(program(selected.stop, selected.next, selected.retired, selected.work),selected.padding);
      const pages = Math.ceil((codePointer + encoded.length) / 65536);
      if (pages > memory.buffer.byteLength / 65536) memory.grow(pages - memory.buffer.byteLength / 65536);
      d().setUint32(context + 32, pc, true); d().setBigUint64(context + 120, selected.id, true);
      new Uint8Array(memory.buffer, codePointer).set(encoded); return 0;
    },
    core_context_ptr: () => context, core_code_ptr: () => codePointer,
    core_code_len: () => encoded.length,
    core_begin() { calls.push(['begin']); return control.begin(); }, core_end() { calls.push(['end']); return 0; },
    core_import() { calls.push(['import']); d().setUint32(context + 32, arena.get(48), true); d().setUint32(context, arena.get(16) ?? 0,true); return 0; },
    core_export(reason,work) { calls.push(['export', reason,work]); arena.set(48, d().getUint32(context + 32, true)); arena.set(72, reason === 2 ? 1 : 3); return 0; },
    core_read: () => 0n, core_write: () => 0, core_numeric: () => 0,
    core_dispatch_code_ptr() {new Uint8Array(memory.buffer,8192).set(dispatch); return 8192;},
    core_dispatch_code_len: () => dispatch.length,
    core_link_owners_ptr() {
      calls.push(['owners']);
      new Uint8Array(memory.buffer,ownersPointer,4097*8).fill(0);
      for(const [slot,id] of links) d().setBigUint64(ownersPointer+slot*8,id,true);
      return ownersPointer;
    },
    core_cache_victim() {
      calls.push(['victim']);
      if(control.victim!==undefined)return control.victim;
      for(const id of recency)for(const [slot,owner] of links)if(owner===id)return slot;
      return 0;
    },
    core_cache_status_ptr() {
      calls.push(['cache-status']);
      (control.nativeStatus??[3n,1234n,1n,2n,4n,BigInt(links.size),678n,1024n])
        .forEach((value,index)=>d().setBigUint64(statusPointer+index*8,value,true));
      return statusPointer;
    },
    core_link(slot) {
      calls.push(['link',slot,selected.id]);
      if (links.has(slot) && links.get(slot) !== selected.id) return 7;
      links.set(slot,selected.id); return 0;
    },
    core_unlink(slot,low,high) {
      const id = BigInt(low>>>0)|(BigInt(high>>>0)<<32n); calls.push(['unlink',slot,id]);
      if (control.unlink) return control.unlink(slot,id);
      if (!links.has(slot)) return 0;
      if (links.get(slot) !== id) return 4;
      links.delete(slot); return 0;
    },
    core_chain_next(pointer) {
      calls.push(['next',pointer]);
      if (control.next) return control.next(pointer);
      const target = select(d().getUint32(pointer+32,true));
      const found = [...links].find(([,id]) => id === target.id);
      if (!found) return 0;
      selected = target; touch(target.id); d().setBigUint64(pointer+120,target.id,true); return found[0];
    },
  };
  const engine = {api, memory, get: offset => arena.get(offset) ?? 0,
    ok(name, ...args) { const status = api[name](...args); if (status) throw new Error(`${name} returned ${status}`); }};
  return {engine, api, calls, arena, control, context, links, setContext: value => {context = value;}};
}
test('native prepare selects every requested entry and native identity controls reuse', () => {
  const f = fixture(pc => ({id: pc < 0x2000 ? 1n : 2n, stop: 1, next: pc + 4}));
  const cache = new CodeCache(f.engine,{chain:false});
  const a = cache.entry(0x1000), inside = cache.entry(0x1004), b = cache.entry(0x2000);
  assert.equal(a, inside); assert.notEqual(a, b); assert.deepEqual(f.calls.filter(c => c[0] === 'prepare').map(c => c[1]), [0x1000,0x1004,0x2000]);
  assert.equal(cache.stats().admissions, 2); assert.equal(cache.stats().engineCacheHits, 1);
});
test('cold compilation diagnostics remain absent and read no clock when disabled', t => {
  const f = fixture(), cache = new CodeCache(f.engine,{chain:false});
  t.mock.method(performance,'now',()=>{throw new Error('disabled diagnostic clock');});
  cache.entry(0x1000); cache.entry(0x1000);
  assert.equal(cache.stats().cacheDiagnostics,undefined);
});
test('cold compilation diagnostics separate prepares, hits, admissions and phase time', t => {
  let identity = 1n, clock = 0;
  const f = fixture(pc => ({id:identity,stop:2,next:pc+4}));
  f.engine.cacheMeasurementsEnabled = true;
  const cache = new CodeCache(f.engine,{chain:false,capacity:1});
  t.mock.method(performance,'now',()=>clock++);
  cache.entry(0x1000); cache.entry(0x1004); cache.entry(0x1000);
  identity = 2n; cache.entry(0x1000);
  identity = 3n; cache.entry(0x2000);
  identity = 2n; cache.entry(0x1000);
  const stats = cache.stats(), diagnostic = stats.cacheDiagnostics;
  assert.equal(stats.admissions,4); assert.equal(stats.engineCacheHits,2);
  assert.equal(diagnostic.prepareCount,6); assert.equal(diagnostic.prepareFailures,0);
  assert.equal(diagnostic.prepareNs,6_000_000); assert.equal(diagnostic.prepareMaxNs,1_000_000);
  for (const phase of ['module','instance']) {
    assert.equal(diagnostic[`${phase}Count`],4); assert.equal(diagnostic[`${phase}Failures`],0);
    assert.equal(diagnostic[`${phase}Ns`],4_000_000); assert.equal(diagnostic[`${phase}MaxNs`],1_000_000);
  }
  assert.equal(diagnostic.admissionCount,4);
  assert.equal(diagnostic.admittedBytes,stats.generatedModuleBytes);
  assert.equal(diagnostic.prepareHits,2); assert.equal(diagnostic.trackedPcCount,3);
  assert.equal(diagnostic.trackedRepeatPcAdmissions,2);
  assert.equal(diagnostic.samePcNewIdentityAdmissions,1);
  assert.equal(diagnostic.samePcSameIdentityAdmissions,1);
  assert.deepEqual(diagnostic.topRepeatPcs.map(row=>[row.pc,row.prepares,row.hits,row.admissions,row.identityChanges,row.lastIdentity]),
    [[0x1000,4,1,3,1,'2']]);
  assert.deepEqual(diagnostic.lastAdmissions.map(row=>[row.sequence,row.pc,row.identity,row.prepareNs,row.moduleNs,row.instanceNs,row.phaseTotalNs]),
    [[1,0x1000,'1',1_000_000,1_000_000,1_000_000,3_000_000],
      [2,0x1000,'2',1_000_000,1_000_000,1_000_000,3_000_000],
      [3,0x2000,'3',1_000_000,1_000_000,1_000_000,3_000_000],
      [4,0x1000,'2',1_000_000,1_000_000,1_000_000,3_000_000]]);
  assert.equal(diagnostic.droppedPcPrepares,0); assert.equal(diagnostic.droppedPcAdmissions,0);
  assert.equal(diagnostic.pcCapacity,8192); assert.equal(diagnostic.lastCapacity,32); assert.equal(diagnostic.slowCapacity,8);
  diagnostic.topRepeatPcs[0].admissions = 999;
  diagnostic.lastAdmissions[0].identity = '999';
  diagnostic.slowestAdmissions[0].identity = '999';
  const again = cache.stats().cacheDiagnostics;
  assert.equal(again.topRepeatPcs[0].admissions,3);
  assert.equal(again.lastAdmissions[0].identity,'1'); assert.equal(again.slowestAdmissions[0].identity,'1');
});
test('cold compilation diagnostics retain failed phase accounting without admitting a module', t => {
  let clock = 0;
  t.mock.method(performance,'now',()=>clock++);
  const prepare = fixture(); prepare.engine.cacheMeasurementsEnabled = true;
  prepare.api.core_prepare = () => 7;
  const rejected = new CodeCache(prepare.engine,{chain:false});
  assert.throws(()=>rejected.entry(),/core_prepare returned 7/);
  const beforeModule = rejected.stats().cacheDiagnostics;
  assert.equal(beforeModule.prepareCount,1); assert.equal(beforeModule.prepareFailures,1);
  assert.equal(beforeModule.prepareNs,1_000_000); assert.equal(beforeModule.moduleCount,0);
  assert.equal(beforeModule.admissionCount,0); assert.deepEqual(beforeModule.lastAdmissions,[]);
  const malformed = fixture(); malformed.engine.cacheMeasurementsEnabled = true;
  const pointer = malformed.api.core_code_ptr;
  malformed.api.core_code_ptr = () => {
    const offset = pointer(); new Uint8Array(malformed.engine.memory.buffer)[offset] = 255; return offset;
  };
  const badModule = new CodeCache(malformed.engine,{chain:false});
  assert.throws(()=>badModule.entry(),WebAssembly.CompileError);
  const afterModule = badModule.stats().cacheDiagnostics;
  assert.equal(afterModule.prepareFailures,0); assert.equal(afterModule.moduleCount,1);
  assert.equal(afterModule.moduleFailures,1); assert.equal(afterModule.moduleNs,1_000_000);
  assert.equal(afterModule.instanceCount,0); assert.equal(afterModule.admissionCount,0);
  const mismatched = fixture(); mismatched.engine.cacheMeasurementsEnabled = true;
  mismatched.api.core_read = new WebAssembly.Instance(new WebAssembly.Module(program()),
    {ring3:{memory:mismatched.engine.memory,read:()=>0n,write:()=>0}}).exports.run;
  const badInstance = new CodeCache(mismatched.engine,{chain:false});
  assert.throws(()=>badInstance.entry(),WebAssembly.LinkError);
  const afterInstance = badInstance.stats().cacheDiagnostics;
  assert.equal(afterInstance.moduleFailures,0); assert.equal(afterInstance.instanceCount,1);
  assert.equal(afterInstance.instanceFailures,1); assert.equal(afterInstance.instanceNs,1_000_000);
  assert.equal(afterInstance.admissionCount,0); assert.equal(badInstance.stats().installedUnits,0);
});
test('cold compilation diagnostics cap PC retention and snapshots while preserving aggregate totals', t => {
  let identity = 0n, clock = 0;
  const f = fixture(pc => ({id:++identity,stop:2,next:pc+4})); f.engine.cacheMeasurementsEnabled = true;
  const cache = new CodeCache(f.engine,{chain:false,capacity:1});
  t.mock.method(performance,'now',()=>clock++);
  for (let index=0; index<8193; index++) cache.entry(index*4);
  cache.entry(0); cache.entry(32768);
  const diagnostic = cache.stats().cacheDiagnostics;
  assert.equal(diagnostic.prepareCount,8195); assert.equal(diagnostic.admissionCount,8195);
  assert.equal(diagnostic.moduleCount,8195); assert.equal(diagnostic.instanceCount,8195);
  assert.equal(diagnostic.prepareNs,8_195_000_000); assert.equal(diagnostic.prepareHits,0);
  assert.equal(diagnostic.admittedBytes,cache.stats().generatedModuleBytes);
  assert.equal(diagnostic.trackedPcCount,8192); assert.equal(diagnostic.droppedPcPrepares,2);
  assert.equal(diagnostic.droppedPcAdmissions,2);
  assert.equal(diagnostic.trackedRepeatPcAdmissions,1); assert.equal(diagnostic.samePcNewIdentityAdmissions,1);
  assert.deepEqual(diagnostic.topRepeatPcs.map(row=>[row.pc,row.admissions]),[[0,2]]);
  assert.equal(diagnostic.lastAdmissions.length,32); assert.equal(diagnostic.lastAdmissions[0].sequence,8164);
  assert.equal(diagnostic.lastAdmissions.at(-1).sequence,8195);
  assert.equal(diagnostic.slowestAdmissions.length,8);
  assert.deepEqual(diagnostic.slowestAdmissions.map(row=>row.sequence),[1,2,3,4,5,6,7,8]);
});
test('NeedCode retains context until a real canonical boundary is published and imported', () => {
  const f = fixture(), cache = new CodeCache(f.engine,{chain:false});
  const run = cache.execute(cache.entry());
  assert.equal(run.pc, 0x1004); assert.equal(run.reason, null); assert.equal(f.engine.get(48), 0x1000);
  assert.throws(() => cache.import(), /unpublished/); cache.entry();
  assert.equal(f.calls.filter(c => c[0] === 'prepare').at(-1)[1], 0x1004);
  cache.publish(); f.arena.set(48, 0x3000); f.arena.set(16,73); cache.import(); assert.equal(cache.pc, 0x3000);
  assert.equal(new DataView(f.engine.memory.buffer).getUint32(f.context,true),73);
  assert.equal(new DataView(f.engine.memory.buffer).getUint32(f.context+40,true),1);
  assert.deepEqual(f.calls.filter(c => ['export','import'].includes(c[0])), [['export',1,1],['import']]);
});
test('stale begin selects fresh current code once and executes no old module', () => {
  let generation = 1n;
  const f = fixture(pc => ({id: generation, stop: 2, next: generation === 1n ? 0x1100 : 0x2200}));
  const cache = new CodeCache(f.engine,{chain:false}), old = cache.entry(); let begins = 0;
  f.control.begin = () => { begins++; if (begins === 1) {generation = 2n; return 4;} return 0; };
  const run = cache.execute(old); assert.equal(run.id, 2n); assert.equal(run.pc, 0x2200); assert.equal(begins, 2);
});
test('native failures, changed bindings, identity and context failures never select another backend', () => {
  const f = fixture(), cache = new CodeCache(f.engine,{chain:false}); cache.entry(); f.setContext(512);
  assert.throws(() => cache.entry(), /context allocation changed/);
  const g = fixture(); delete g.api.core_numeric; assert.throws(() => new CodeCache(g.engine,{chain:false}), /missing core export/);
  const h = fixture(), guarded = new CodeCache(h.engine,{chain:false}); const unit = guarded.entry(); h.api.core_read = () => 4n;
  assert.throws(() => guarded.execute(unit), /bindings changed/);
  const i = fixture(() => ({id: 1n, stop: 8, next: 0x1000})), broken = new CodeCache(i.engine,{chain:false});
  assert.throws(() => broken.execute(broken.entry()), /invalid core stop 8/); assert.equal(broken.open, false);
  const j = fixture(() => ({id: 1n, stop: 4, next: 0x1000})), unsupported = new CodeCache(j.engine,{chain:false});
  assert.throws(() => unsupported.execute(unsupported.entry()), /unsupported instruction/);
  assert.deepEqual(j.calls.filter(c => c[0] === 'export'),[['export',4,1]]);
});
test('publication carries consumed work across NeedCode independently of retirement', () => {
  const f = fixture(pc => pc === 0x1000 ? {id:1n,stop:1,next:0x1004,retired:0,work:3}
    : {id:2n,stop:2,next:0x1008,retired:1,work:2});
  const cache = new CodeCache(f.engine,{chain:false}); cache.execute(cache.entry()); cache.execute(cache.entry());
  assert.equal(cache.stats().retired,1); assert.equal(cache.stats().work,5);
  assert.deepEqual(f.calls.filter(c => c[0] === 'export'),[['export',2,5]]);
});
test('chained dispatch enters linked Wasm regions with one native begin and end', () => {
  const f = fixture(pc => pc === 0x1000 ? {id:1n,stop:1,next:0x2000} : {id:2n,stop:2,next:0x2004});
  const cache = new CodeCache(f.engine,{chain:true});
  cache.entry(0x1000); cache.entry(0x2000);
  const run = cache.execute(cache.entry(0x1000));
  assert.equal(run.coreReason,2); assert.equal(run.id,2n); assert.equal(run.regions,2);
  assert.equal(run.retired,2); assert.equal(run.work,2); assert.equal(cache.pc,0x2004);
  assert.equal(cache.stats().runs,1); assert.equal(cache.stats().regions,2);
  assert.equal(f.calls.filter(([name]) => name === 'prepare').length,3);
  assert.deepEqual(f.calls.filter(([name]) => ['begin','end','next'].includes(name)),[['begin'],['next',f.context],['end']]);
});
test('ordinary execution defaults to chained dispatch while explicit false remains available', () => {
  assert.equal(DEFAULT_EXECUTION.chain,true);
  const f = fixture(pc => pc === 0x1000 ? {id:1n,stop:1,next:0x2000} : {id:2n,stop:2,next:0x2004});
  const cache = new CodeCache(f.engine);
  assert.equal(cache.stats().chainEnabled,true);
  cache.entry(0x2000);
  const run = cache.execute(cache.entry(0x1000));
  assert.equal(run.regions,2); assert.equal(run.work,2);
  assert.equal(f.calls.filter(([name]) => name === 'begin').length,1);
  assert.equal(f.calls.filter(([name]) => name === 'end').length,1);
  const direct = new CodeCache(fixture().engine,{chain:false});
  assert.equal(direct.stats().chainEnabled,false); assert.equal(direct.table,undefined);
});
test('the default count bound retains more than 256 small modules within the same byte budget', () => {
  assert.equal(DEFAULT_EXECUTION.capacity,1024);
  const f = fixture(pc => ({id:BigInt(pc),stop:2,next:pc+4}));
  const cache = new CodeCache(f.engine,{chain:false}), first = cache.entry(0x1000);
  for (let index=1; index<300; index++) cache.entry(0x1000+index*4);
  const stats = cache.stats();
  assert.equal(stats.unitCapacity,1024); assert.equal(stats.byteCapacity,32*1024*1024);
  assert.equal(stats.installedUnits,300); assert.equal(stats.admissions,300); assert.equal(stats.evictions,0);
  assert.equal(cache.units.get(first.id),first); assert(stats.retainedModuleBytes<stats.byteCapacity);
});
test('capacity accepts 1024 and rejects invalid or larger metadata bounds', () => {
  assert.equal(new CodeCache(fixture().engine,{capacity:1024,chain:false}).stats().unitCapacity,1024);
  for (const capacity of [0,1025,1.5,NaN,Infinity])
    assert.throws(() => new CodeCache(fixture().engine,{capacity,chain:false}),/invalid core capacity/);
});
test('count eviction statistics change only after the expected owner is unlinked', () => {
  const f = fixture(pc => ({id:BigInt(pc),stop:2,next:pc+4}));
  const cache = new CodeCache(f.engine,{capacity:1,chain:true});
  cache.entry(0x1000); cache.entry(0x2000);
  assert.deepEqual(cache.stats().evictionReasons,{count:1,bytes:0});
  const detached = cache.stats().evictionReasons; detached.count=99;
  f.control.unlink=()=>4;
  assert.throws(() => cache.entry(0x3000),/core_unlink returned 4/);
  assert.equal(cache.stats().evictions,1);
  assert.deepEqual(cache.stats().evictionReasons,{count:1,bytes:0});
});
test('byte pressure evicts below the count limit and safely reuses the released table slot', () => {
  const f = fixture(pc => ({id:BigInt(pc),stop:2,next:pc+4,padding:12*1024*1024}));
  const cache = new CodeCache(f.engine,{capacity:1024,chain:true});
  const first = cache.entry(0x1000), second = cache.entry(0x2000), third = cache.entry(0x3000);
  const stats = cache.stats();
  assert.equal(stats.byteCapacity,32*1024*1024); assert.equal(stats.installedUnits,2);
  assert.equal(stats.evictions,1); assert.deepEqual(stats.evictionReasons,{count:0,bytes:1});
  assert(stats.retainedModuleBytes<=stats.byteCapacity);
  assert.equal(cache.units.has(first.id),false); assert.equal(cache.units.get(second.id),second);
  assert.equal(third.slot,first.slot); assert.equal(cache.table.get(third.slot),third.run);
  assert.deepEqual(f.calls.filter(([name]) => ['link','unlink'].includes(name)),[
    ['link',first.slot,first.id],['link',second.slot,second.id],['unlink',first.slot,first.id],['link',third.slot,third.id],
  ]);
});
test('cold eviction unlinks the expected owner before a private table slot is reused', () => {
  const f = fixture(pc => ({id:BigInt(pc),stop:2,next:pc+4})), cache = new CodeCache(f.engine,{chain:true,capacity:1});
  const a = cache.entry(0x1000); assert(a.slot>0); assert.equal(cache.table.get(0),null);
  const b = cache.entry(0x2000);
  assert.equal(a.slot,b.slot); assert.equal(cache.table.get(b.slot),b.run);
  assert.deepEqual(f.calls.filter(([name]) => ['link','unlink'].includes(name)),[['link',a.slot,a.id],['unlink',a.slot,a.id],['link',b.slot,b.id]]);
  f.control.unlink = () => 4;
  assert.throws(() => cache.entry(0x3000),/core_unlink returned 4/);
  assert.equal(cache.table.get(b.slot),b.run); assert.equal(cache.units.get(b.id),b);
});
test('chained bindings and invalid native successors fail closed', () => {
  const f = fixture(), cache = new CodeCache(f.engine,{chain:true}), unit = cache.entry();
  f.api.core_chain_next = () => 0;
  assert.throws(() => cache.execute(unit),/bindings changed/);
  const g = fixture(), rejected = new CodeCache(g.engine,{chain:true}), selected = rejected.entry();
  g.control.next = () => 0xffffffff;
  assert.throws(() => rejected.execute(selected),/invalid core stop 8/);
  assert.equal(rejected.open,false); assert.equal(g.calls.filter(([name]) => name === 'end').length,1);
});
test('native eviction prunes a detached module without evicting another linked owner', () => {
  const f=fixture(pc=>({id:BigInt(pc),stop:2,next:pc+4})), cache=new CodeCache(f.engine,{chain:true,capacity:2});
  const a=cache.entry(0x1000), b=cache.entry(0x2000);
  f.control.prepare=pc=>{if(pc===0x3000)f.links.delete(a.slot);};
  const c=cache.entry(0x3000), stats=cache.stats();
  assert.equal(cache.units.has(a.id),false); assert.equal(cache.units.get(b.id),b);
  assert.equal(c.slot,a.slot); assert.equal(cache.table.get(c.slot),c.run);
  assert.equal(stats.installedUnits,2); assert.equal(stats.evictions,0);
  assert.deepEqual(stats.evictionReasons,{count:0,bytes:0});
  assert.equal(stats.prunedNative,1); assert.equal(stats.prunedNativeBytes,a.bytes);
  assert.equal(stats.retainedModuleBytes,b.bytes+c.bytes);
  assert.deepEqual(f.calls.filter(([name])=>name==='unlink'),[['unlink',a.slot,a.id]]);
});
test('a nonzero native owner mismatch rejects before any detached module is cleared', () => {
  const f=fixture(pc=>({id:BigInt(pc),stop:2,next:pc+4})), cache=new CodeCache(f.engine,{chain:true,capacity:2});
  const a=cache.entry(0x1000), b=cache.entry(0x2000);
  f.control.prepare=pc=>{if(pc===0x3000){f.links.delete(a.slot);f.links.set(b.slot,99n);}};
  assert.throws(()=>cache.entry(0x3000),/native.*owner|owner.*mismatch/);
  assert.equal(cache.units.get(a.id),a); assert.equal(cache.units.get(b.id),b);
  assert.equal(cache.table.get(a.slot),a.run); assert.equal(cache.table.get(b.slot),b.run);
  assert.equal(cache.stats().evictions,0); assert.equal(cache.stats().prunedNative,0);
  assert.equal(f.calls.filter(([name])=>name==='unlink').length,0);
});
test('capacity follows native handoff recency instead of JavaScript entry order', () => {
  const f=fixture(pc=>pc===0x1000?{id:1n,stop:2,next:0x1004}
    :pc===0x2000?{id:2n,stop:1,next:0x1000}:{id:3n,stop:2,next:0x3004});
  const cache=new CodeCache(f.engine,{chain:true,capacity:2});
  const a=cache.entry(0x1000), b=cache.entry(0x2000);
  assert.equal(cache.execute(b).regions,2);
  const c=cache.entry(0x3000);
  assert.equal(cache.units.get(a.id),a); assert.equal(cache.units.has(b.id),false);
  assert.equal(c.slot,b.slot); assert.equal(cache.table.get(a.slot),a.run);
  assert.deepEqual(f.calls.filter(([name])=>name==='unlink'),[['unlink',b.slot,b.id]]);
  assert.deepEqual(cache.stats().evictionReasons,{count:1,bytes:0});
});
test('an invalid or unretained native victim cannot clear a private table slot', () => {
  for(const victim of [0,4097,4096]) {
    const f=fixture(pc=>({id:BigInt(pc),stop:2,next:pc+4})), cache=new CodeCache(f.engine,{chain:true,capacity:1});
    const a=cache.entry(0x1000); f.control.victim=victim;
    assert.throws(()=>cache.entry(0x2000),/native.*victim|victim.*native/);
    assert.equal(cache.units.get(a.id),a); assert.equal(cache.table.get(a.slot),a.run);
    assert.equal(f.calls.filter(([name])=>name==='unlink').length,0);
  }
});
test('native cache counters are sampled only at a cold usage boundary and retain u64 precision', () => {
  const f=fixture(pc=>({id:BigInt(pc),stop:2,next:pc+4})), cache=new CodeCache(f.engine,{chain:true});
  cache.execute(cache.entry()); cache.stats();
  assert.equal(f.calls.filter(([name])=>name==='cache-status').length,0);
  const values=[9007199254740993n,1234n,1n,2n,4n,1n,678n,1024n]; f.control.nativeStatus=values;
  const usage=cache.sampleUsage();
  assert.deepEqual(usage.nativeCache,{
    compiled:values[0].toString(),admittedEncodedBytes:'1234',countEvictions:'1',byteEvictions:'2',
    sourceInvalidations:'4',retainedUnits:'1',retainedEncodedBytes:'678',unitCapacity:'1024',
  });
  usage.nativeCache.compiled='0'; assert.equal(cache.stats().nativeCache.compiled,values[0].toString());
  cache.stats(); assert.equal(f.calls.filter(([name])=>name==='cache-status').length,1);
  const direct=fixture(); delete direct.api.core_cache_status_ptr;
  assert.doesNotThrow(()=>new CodeCache(direct.engine,{chain:false}));
  for(const name of ['core_link_owners_ptr','core_cache_victim','core_cache_status_ptr']) {
    const missing=fixture(); delete missing.api[name];
    assert.throws(()=>new CodeCache(missing.engine,{chain:true}),/missing core export/);
  }
});
test('callback consumes the standard 64-byte record without resident ownership', () => {
  const bytes = new Uint8Array(64), d = new DataView(bytes.buffer); bytes.set(new TextEncoder().encode('R3CB'));
  d.setUint16(4,2,true); d.setUint16(6,1,true); d.setUint32(8,64,true);
  for (const [offset,value] of [[16,3],[20,2],[24,1],[32,0x1000],[36,0x4000],[40,0x2000],[44,0xffffe],[48,2]]) d.setUint32(offset,value,true);
  assert.deepEqual(callbackRecord(bytes,0x2000,0xffffe), {token:3,returnPc:0x2000,returnId:0xffffe,returnEsp:0x400c});
  assert.throws(() => callbackRecord(new Uint8Array(72),0x2000,0xffffe), /length/);
  d.setUint16(4,1,true); assert.throws(() => callbackRecord(bytes,0x2000,0xffffe), /header/); d.setUint16(4,2,true);
  d.setUint32(56,1,true); assert.throws(() => callbackRecord(bytes,0x2000,0xffffe), /frame/); d.setUint32(56,0,true);
  d.setUint32(40,0x3000,true); assert.throws(() => callbackRecord(bytes,0x2000,0xffffe), /frame/);
});
