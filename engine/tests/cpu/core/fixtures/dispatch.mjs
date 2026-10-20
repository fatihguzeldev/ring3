import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {makeKernel} from './kernels.mjs';
import {assertCoreNames, assertDispatchNames} from './names.mjs';

const directory = process.argv[2], PAGE = 4096, CTX = 0x100;
const MAP = 0x10000, PERM = 0x410000, EXEC = 0x510000, VERSION = 0x510100;
const PAGE_VERSION = 0x510200, BACKING = 0x520000, GENERATION = 0x530000;
const memory = new WebAssembly.Memory({initial: 96}), functions = new WebAssembly.Table({element: 'anyfunc', initial: 9, maximum: 4097});
const bytes = new Uint8Array(memory.buffer), view = new DataView(memory.buffer);
const get = offset => view.getUint32(offset, true), put = (offset, value) => view.setUint32(offset, value >>> 0, true);
const hex = value => Buffer.from(value).toString('hex');
const hash = value => createHash('sha256').update(value).digest('hex');
const image = readFileSync(join(directory, 'code.bin')), regions = new Map(), records = [];
let active;
const reportPath = join(directory, 'dispatch-result.json');
const checkpoint = () => writeFileSync(reportPath, JSON.stringify({complete: false, records}));

function physical(address) {
  const slot = get(MAP + (address >>> 12) * 4);
  return slot ? BACKING + (slot - 1) * PAGE + (address & 4095) : null;
}
function preflight(address, width, access, modify = false) {
  const result = [];
  for (let index = 0; index < width; index++) {
    const current = (address + index) >>> 0, target = physical(current);
    const reason = target === null ? 1 : (bytes[PERM + (current >>> 12)] & (modify ? 3 : access)) !== (modify ? 3 : access) ? 2 : 0;
    if (reason) {
      put(CTX + 48, current); put(CTX + 52, width | (modify ? 2 : access) << 8 | reason << 16); put(CTX + 116, 14);
      return null;
    }
    result.push(target);
  }
  return result;
}
function read(context, address, encoded) {
  assert.equal(context, CTX); active.reads++;
  const width = encoded & 255, targets = preflight(address >>> 0, width, 1, !!(encoded & 256));
  if (!targets) return 1n << 32n;
  return targets.reduce((value, target, index) => value | BigInt(bytes[target]) << BigInt(index * 8), 0n);
}
function write(context, address, width, value) {
  assert.equal(context, CTX); active.writes++;
  const targets = preflight(address >>> 0, width, 2);
  if (!targets) return 1;
  targets.forEach((target, index) => {bytes[target] = value >>> (index * 8);});
  const version = view.getBigUint64(VERSION, true) + 1n; view.setBigUint64(VERSION, version, true);
  let invalidated = false;
  for (const page of new Set(targets.map(target => Math.floor((target - BACKING) / PAGE)))) {
    view.setBigUint64(PAGE_VERSION + page * 16 + 8, version, true);
    invalidated ||= get(EXEC + page * 4) !== 0;
  }
  if (invalidated) view.setBigUint64(GENERATION, view.getBigUint64(GENERATION, true) + 1n, true);
  return invalidated ? 2 : 0;
}
for (const [index, line] of readFileSync(join(directory, 'regions.tsv'), 'utf8').trim().split('\n').entries()) {
  const [name, entry, id] = line.split('\t'), encoded = readFileSync(join(directory, `${name}.wasm`));
  assert(WebAssembly.validate(encoded), `${name}: original region validates`);
  const module = new WebAssembly.Module(encoded);
  assertCoreNames(module, Number(entry), id);
  assert.deepEqual(WebAssembly.Module.imports(module), [
    {module: 'ring3', name: 'memory', kind: 'memory'}, {module: 'ring3', name: 'read', kind: 'function'},
    {module: 'ring3', name: 'write', kind: 'function'},
  ]);
  const run = new WebAssembly.Instance(module, {ring3: {memory, read, write}}).exports.run;
  regions.set(name, {name, entry: Number(entry), id: BigInt(id), slot: index + 1, run, sha256: hash(encoded)});
  functions.set(index + 1, run);
}
const byPc = new Map([[0x1000,'caller'],[0x1005,'caller'],[0x1008,'caller'],[0x1040,'callee'],[0x1140,'after-repeat']]);
function next(context) {
  assert.equal(context, CTX);
  active.next.push({pc: get(CTX + 32), remaining: get(CTX + 44), retired: get(CTX + 40), id: String(view.getBigUint64(CTX + 120, true))});
  if (active.options.missing) return 0;
  if (active.options.invalidNext) return 0xffffffff;
  const selected = regions.get(byPc.get(get(CTX + 32))); assert(selected, 'scripted native selection has a target');
  view.setBigUint64(CTX + 120, selected.id, true);
  return active.options.nullNext ? 8 : active.options.wrongNext ? regions.get('caller').slot : selected.slot;
}
const encoded = readFileSync(join(directory, 'dispatch.wasm'));
assert(WebAssembly.validate(encoded), 'dispatcher validates');
const module = new WebAssembly.Module(encoded);
assertDispatchNames(module);
assert.deepEqual(WebAssembly.Module.imports(module), [
  {module: 'ring3', name: 'memory', kind: 'memory'}, {module: 'ring3', name: 'functions', kind: 'table'},
  {module: 'ring3', name: 'next', kind: 'function'},
]);
const dispatch = new WebAssembly.Instance(module, {ring3: {memory, functions, next}}).exports;
assert.equal(dispatch.run.length, 3); assert.equal(dispatch.regions.value, 0);

const initialRegs = [5,3,7,0x3000,0x4800,0,0,0];
function check(label, name, budget, options, expected) {
  active = {options, next: [], reads: 0, writes: 0};
  bytes.fill(0, CTX, CTX + 128); bytes.fill(0, BACKING, BACKING + 16 * PAGE);
  bytes.fill(0, EXEC, EXEC + 64); bytes.fill(0, VERSION, VERSION + 8);
  bytes.fill(0, PAGE_VERSION, PAGE_VERSION + 256); bytes.fill(0, GENERATION, GENERATION + 8);
  for (const [page, slot, permission] of [[1,7,7],[3,2,3],[4,11,3]]) {put(MAP + page * 4, slot + 1); bytes[PERM + page] = permission;}
  bytes.set(image, physical(0x1000)); put(EXEC + 7 * 4, 1);
  for (const [address, width, value] of options.initial ?? []) {
    for (let index = 0; index < width; index++) bytes[physical(address + index)] = value >>> (index * 8);
  }
  if (options.readOnly) bytes[PERM + 3] = 1;
  const first = regions.get(name), regs = options.regs ?? initialRegs;
  regs.forEach((value, index) => put(CTX + index * 4, value));
  put(CTX + 32, first.entry); put(CTX + 36, options.flags ?? 2); put(CTX + 44, 0x12345678);
  [MAP,PERM,BACKING,16,EXEC,0,VERSION,PAGE_VERSION,GENERATION].forEach((value,index) => put(CTX + 56 + index * 4,value));
  put(CTX + 112, options.inactive ? 0 : 1);
  view.setBigUint64(CTX + 120, options.wrongIdentity ? first.id + 1n : first.id, true);
  const wantedContext = bytes.slice(CTX, CTX + 128), wanted = new DataView(wantedContext.buffer);
  const wantedRam = bytes.slice(BACKING, BACKING + 16 * PAGE);
  for (const [address, width, value] of expected.writes ?? []) {
    for (let index = 0; index < width; index++) wantedRam[physical(address + index) - BACKING] = value >>> (index * 8);
  }
  const stop = dispatch.run(CTX, budget, options.slot ?? first.slot) >>> 0;
  const row = {label, name, budget, stop, regions: dispatch.regions.value >>> 0, next: active.next,
    context: hex(bytes.subarray(CTX, CTX + 128)), ramSha256: hash(bytes.subarray(BACKING, BACKING + 16 * PAGE)),
    slowReads: active.reads, slowWrites: active.writes};
  records.push(row); checkpoint();
  assert.equal(stop, expected.stop, `${label}: exact stop`);
  assert.equal(row.regions, expected.regions, `${label}: original region calls`);
  assert.equal(row.next.length, expected.next ?? 0, `${label}: native next count`);
  if (!expected.untouched) {
    (expected.regs ?? regs).forEach((value,index) => wanted.setUint32(index * 4,value >>> 0,true));
    wanted.setUint32(32,expected.pc,true); wanted.setUint32(36,expected.flags ?? options.flags ?? 2,true);
    wanted.setUint32(40,expected.retired,true); wanted.setUint32(44,expected.remaining ?? budget - (expected.work ?? expected.retired),true);
    if (expected.fault) {wanted.setUint32(48,expected.fault[0],true);wanted.setUint32(52,expected.fault[1],true);wanted.setUint32(116,14,true);}
    if (expected.gate) wanted.setUint32(116,expected.gate,true);
    wanted.setBigUint64(120, regions.get(expected.owner ?? name).id, true);
  }
  assert.deepEqual(bytes.subarray(CTX,CTX + 128),wantedContext,`${label}: full context`);
  assert.deepEqual(bytes.subarray(BACKING,BACKING + 16 * PAGE),wantedRam,`${label}: whole original code/data/stack backing`);
}

for (const count of [3,17,1024]) {
  const kernel = makeKernel('call',count), writes = [];
  for (let index=0;index<count;index++) writes.push([0x47fc,4,0x1005],[0x300c,4,index+1]);
  check(`warm-call-cycle-${count}`,'caller',kernel.retired+1,{regs:kernel.initialRegs},
    {stop:4,pc:0x1008,retired:kernel.retired,regs:kernel.regs,flags:0x46,regions:2*count+1,next:2*count,writes});
}
const prefixes = [
  [0,0x1000,5,7,0x4800,2,1,0,0], [1,0x1040,5,7,0x47fc,2,1,0,0],
  [2,0x1043,8,7,0x47fc,2,2,1,0], [3,0x1045,8,15,0x47fc,6,2,1,0],
  [4,0x1049,8,15,0x47fc,2,2,1,1], [5,0x1005,8,15,0x4800,2,2,1,1],
  [6,0x1006,8,15,0x4800,2,3,2,1], [7,0x1000,8,15,0x4800,2,3,2,1],
];
for (const [budget,pc,eax,edx,esp,flags,count,nextCalls,counter] of prefixes) {
  const regs=[eax,budget>=6?2:3,edx,0x3000,esp,0,0,0], writes=[];
  if(budget) writes.push([0x47fc,4,0x1005]); if(counter) writes.push([0x300c,4,counter]);
  check(`shared-budget-cut-${budget}`,'caller',budget,{}, {stop:2,pc,retired:budget,regs,flags,regions:count,next:nextCalls,
    owner:count===2?'callee':'caller',writes});
}
for (const [label,options,stop,owner] of [
  ['missing-next',{missing:true},1,'caller'],['invalid-next',{invalidNext:true},8,'caller'],
  ['null-next-slot',{nullNext:true},8,'callee'],['wrong-next-function',{wrongNext:true},8,'callee'],
]) check(label,'caller',8,options,{stop,pc:0x1040,retired:1,regs:[5,3,7,0x3000,0x47fc,0,0,0],
  regions:label==='wrong-next-function'?2:1,next:1,owner,writes:[[0x47fc,4,0x1005]]});
check('callee-rmw-fault-preserves-prefix-flags','caller',8,{readOnly:true},
  {stop:3,pc:0x1045,retired:3,regs:[8,3,15,0x3000,0x47fc,0,0,0],flags:6,regions:2,next:1,owner:'callee',
    writes:[[0x47fc,4,0x1005]],fault:[0x300c,0x20204]});
const repeatRegs=[5,3,0x1140,0x3000,0x4800,0,0x3000,0x4100], initial=[[0x3000,3,0x332211]];
check('repeat-work-is-not-retirement','repeat',8,{regs:repeatRegs,initial},
  {stop:4,pc:0x1143,retired:3,work:5,regs:[6,0,0x1140,0x3000,0x4800,0,0x3003,0x4103],flags:6,regions:2,next:1,owner:'after-repeat',
    writes:[[0x4100,3,0x332211]]});
check('repeat-budget-cut-never-looks-up-target','repeat',2,{regs:repeatRegs,initial,flags:0x893},
  {stop:2,pc:0x1100,retired:0,work:2,regs:[5,1,0x1140,0x3000,0x4800,0,0x3002,0x4102],regions:1,writes:[[0x4100,2,0x2211]]});
check('repeat-fault-retains-completed-elements','repeat',8,{regs:[5,3,0x1140,0x3000,0x4800,0,0x3000,0x4fff],initial,flags:0x893},
  {stop:3,pc:0x1100,retired:0,work:1,regs:[5,2,0x1140,0x3000,0x4800,0,0x3001,0x5000],regions:1,
    writes:[[0x4fff,1,0x11]],fault:[0x5000,0x10201]});
check('committed-code-write-never-links','code-store',8,{regs:[0x11223344,3,0x1000,0x1200,0x4800,0,0,0]},
  {stop:6,pc:0x1202,retired:1,regions:1,writes:[[0x1200,4,0x11223344]]});
check('first-read-fault-never-links','read-fault',8,{regs:[5,3,0x1000,0x6000,0x4800,0,0,0]},
  {stop:3,pc:0x1300,retired:0,regions:1,fault:[0x6000,0x10104]});
check('host-gate-never-links','gate',8,{}, {stop:7,pc:0x1400,retired:0,regions:1,gate:77});
for (const slot of [0,9,4097,0xffffffff]) check(`invalid-first-slot-${slot}`,'caller',8,{slot}, {stop:8,regions:0,untouched:true});
check('null-first-slot','caller',8,{slot:8},{stop:8,regions:0,untouched:true});
check('inactive-region-is-neutral','caller',8,{inactive:true},{stop:8,regions:1,untouched:true});
check('wrong-region-identity-is-neutral','caller',8,{wrongIdentity:true},{stop:8,regions:1,untouched:true});

const section=(id,payload)=>[id,payload.length,...payload];
function stub(stop, remaining) {
  const body=[0];
  if(remaining!==undefined) {
    body.push(0x20,0);
    if(remaining===null) body.push(0x41,0); else body.push(0x20,1,...(remaining?[0x41,remaining,0x6a]:[]));
    body.push(0x36,2,44);
  }
  body.push(0x41,stop,0x0b);
  const data=Uint8Array.from([0,97,115,109,1,0,0,0,...section(1,[1,0x60,2,0x7f,0x7f,1,0x7f]),
    ...section(2,[1,6,114,105,110,103,51,120,6,109,101,109,111,114,121,2,0,1]),
    ...section(3,[1,0]),...section(7,[1,3,114,117,110,0,0]),...section(10,[1,body.length,...body])]);
  return new WebAssembly.Instance(new WebAssembly.Module(data),{ring3x:{memory}}).exports.run;
}
for(const stop of [2,3,4,5,6,7,8,9,10]) {
  functions.set(8,stub(stop));
  check(`non-NeedCode-${stop}-returns-immediately`,'caller',8,{slot:8},{stop,regions:1,untouched:true});
}
for(const [label,remaining,stop,wanted] of [['no-work',0,1,8],['excess-remaining',1,8,9],['zero-remaining',null,2,0]]) {
  functions.set(8,stub(1,remaining));
  check(label,'caller',8,{slot:8},{stop,pc:0x1000,retired:0,remaining:wanted,regions:1});
}
writeFileSync(reportPath,JSON.stringify({complete:true,dispatchSha256:hash(encoded),sources:[import.meta.url,new URL('./kernels.mjs',import.meta.url).href,new URL('./names.mjs',import.meta.url).href]
  .map(url=>({url,sha256:hash(readFileSync(new URL(url)))})),regions:[...regions.values()].map(({name,id,slot,sha256})=>({name,id:String(id),slot,sha256})),records},null,2));
console.log(JSON.stringify({complete:true,cases:records.length,dispatchSha256:hash(encoded)}));
