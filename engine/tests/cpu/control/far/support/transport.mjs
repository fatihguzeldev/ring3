import assert from 'node:assert/strict';
import {readFileSync,writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {join} from 'node:path';
import {createArena,ARENA_SIZE} from '../../../../support/wasm/arena.mjs';
import {readEngine} from '../../../../support/wasm/engine.mjs';
import {assertEngineExports} from '../../../../support/wasm/exports.mjs';
export {createArena,readEngine};
export const PC=0x1000,TARGET=0x2000,POINTER=0x5010,SEG=4524,STACK=0x7800,KEY_HIGH=0x464152;
export const counts={contexts:0,modules:0,runs:0,success:0,faults:0,refusals:0,controls:0};
export const observations=[];
export const words=values=>{const bytes=Buffer.alloc(values.length*4);values.forEach((v,i)=>bytes.writeUInt32LE(v>>>0,i*4));return bytes;};
export function packet(magic,size,fields,version){const bytes=Buffer.alloc(size);bytes.write(magic);bytes.writeUInt16LE(version,4);bytes.writeUInt16LE(1,6);bytes.writeUInt32LE(size,8);bytes.set(words(fields),16);return bytes;}
export const arena=c=>Buffer.from(new Uint8Array(c.memory.buffer,c.base,ARENA_SIZE));
export function put(c,offset,bytes){new Uint8Array(c.memory.buffer).set(bytes,c.base+offset);}
export function host(c,name,args=[]){assert.equal(c.api[name](...args),0,name);if(name==='protect')for(let i=0;i<args[1];i++)c.permissions.set((args[0]+i*4096)>>>0,args[2]);}
export function upload(c,address,bytes){put(c,140,bytes);host(c,'upload',[address,bytes.length]);}
export function configure(c,selector,raw){const before=arena(c);host(c,'configure_segment_descriptor_raw',[selector,raw.low,raw.high]);assert.deepEqual(arena(c),before,'raw table update leaves arena/cache unchanged');}
export function map(c,address,permissions=3){host(c,'map',[address,1,permissions]);c.pages.push(address);c.permissions.set(address,permissions);}
// Diagnostic reads preserve the exact execution arena, and restore any temporary read
// permission before returning. The execution oracle is asserted before these probes.
export function readRAM(c,address,length){
 const before=arena(c),bytes=Buffer.alloc(length),changed=[];
 for(let page=address;page<address+length;page+=4096){const permissions=c.permissions.get(page);if(!(permissions&1)){changed.push([page,permissions]);host(c,'protect',[page,1,permissions|1]);assert.deepEqual(arena(c),before,'diagnostic protect is arena-neutral');}}
 for(let offset=0;offset<length;offset+=512){const size=Math.min(512,length-offset);host(c,'cpu_read',[(address+offset)>>>0,size,1]);const after=arena(c);assert.equal(after.readUInt32LE(116),0,'diagnostic cpu_read succeeds');bytes.set(after.subarray(140,140+size),offset);}
 for(const [page,permissions]of changed)host(c,'protect',[page,1,permissions]);
 put(c,0,before);return bytes;
}
export function snapshotRAM(c){return new Map(c.pages.map(address=>[address,readRAM(c,address,4096)]));}
export function assertRAM(c,want,label){for(const [address,bytes] of want)assert.deepEqual(readRAM(c,address,bytes.length),bytes,`${label}: RAM ${address.toString(16)}`);}
export function writeExpected(ram,address,bytes){for(const [i,value] of bytes.entries()){const at=(address+i)>>>0,page=at&0xfffff000;assert.ok(ram.has(page>>>0));ram.get(page>>>0)[at&4095]=value;}}
export function open(engine,resident,entries){
 const instance=new WebAssembly.Instance(engine.module,{}),api=Object.fromEntries(Object.entries(instance.exports).filter(([name])=>name.startsWith('ring3_abi_v1_')).map(([name,fn])=>[name.slice(13),fn]));
 const c={engine,resident,entries,api,memory:instance.exports.memory,key:++counts.contexts,calls:[],pages:[],permissions:new Map()};
 host(c,'open',[8,c.key,KEY_HIGH]);c.base=api.arena_ptr()>>>0;
 for(const [address,permissions] of [[PC,7],[TARGET,7],[0x5000,3],[0x7000,3]])map(c,address,permissions);
 upload(c,TARGET,Buffer.from([0xeb,0xfe]));return c;
}
export function descriptor({type = 10, dpl = 3, present = true, codeData = true, base = 0, limit = 0xfffff, pages = true, d32 = true, long = false, avl = false} = {}) {
  return {type, dpl, present, codeData, base, limit, pages, d32, long, avl,
    low: (((base & 0xffff) * 65536) + (limit & 0xffff)) >>> 0,
    high: ((base & 0xff000000) + ((base >>> 16) & 255) + type * 256 + Number(codeData) * 4096 + dpl * 8192 + Number(present) * 32768 + (limit & 0xf0000) + Number(avl) * 0x100000 + Number(long) * 0x200000 + Number(d32) * 0x400000 + Number(pages) * 0x800000) >>> 0};
}
export const cache = raw => (raw.limit + (raw.type | 1) * 0x100000 + Number(raw.pages) * 0x1000000 + Number(raw.d32) * 0x2000000 + Number(raw.avl) * 0x8000000 + 0x10000000 + raw.dpl * 0x20000000 + 0x80000000) >>> 0;
const cacheOffsets = [28, 32, 36, 40, 44, 56];
export function decodeSegments(bytes) {
  const selectors = Array.from({length: 6}, (_, i) => bytes.readUInt16LE(16 + i * 2));
  const caches = bytes.readUInt16LE(4) === 1 ? selectors.map((selector, i) => (selector & 0xfffc) === 0 ? 0 : i === 1 ? 0xf3bfffff : 0xf33fffff) : cacheOffsets.map(at => bytes.readUInt32LE(at));
  return {selectors, caches, fs: bytes.readUInt32LE(48), gs: bytes.readUInt32LE(52)};
}
export function encodeSegments(context) {
  const bytes = packet('R3SG', 64, [], 1);
  context.selectors.forEach((value, i) => bytes.writeUInt16LE(value, 16 + i * 2));
  bytes.writeUInt32LE(context.fs, 48); bytes.writeUInt32LE(context.gs, 52);
  const legacy = context.selectors.map((selector, i) => (selector & 0xfffc) === 0 ? 0 : i === 1 ? 0xf3bfffff : 0xf33fffff);
  if (!context.caches.every((value, i) => value === legacy[i])) {bytes.writeUInt16LE(2, 4); context.caches.forEach((value, i) => bytes.writeUInt32LE(value, cacheOffsets[i]));}
  return bytes;
}
function validateModule(bytes, expectedImports, hasGuard, arities) {
  const module = new WebAssembly.Module(bytes);
  assert.deepEqual(WebAssembly.Module.imports(module).map(row => `${row.module}:${row.name}:${row.kind}`).sort(), expectedImports.sort());
  assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  let at = 8; const types = [], imported = [], functions = [], bodies = [];
  const leb = () => {let n = 0, shift = 0, v; do {v = bytes[at++]; assert.ok(v !== undefined && shift < 35); n += (v & 127) * 2 ** shift; shift += 7;} while (v & 128); return n;};
  const vector = () => {const n = leb(), b = bytes.subarray(at, at + n); at += n; return [...b];};
  const string = () => Buffer.from(vector()).toString();
  while (at < bytes.length) {
    const section = bytes[at++], length = leb(), end = at + length;
    if (section === 1) for (let n = leb(); n; n--) {assert.equal(bytes[at++], 0x60); types.push([vector(), vector()]);}
    else if (section === 2) for (let n = leb(); n; n--) {
      string(); const name = string(), kind = bytes[at++]; if (kind === 2) {assert.equal(leb(), 0); assert.equal(leb(), 1);} else {assert.equal(kind, 0); imported.push([name, leb()]);}
    }
    else if (section === 3) for (let n = leb(); n; n--) functions.push(leb());
    else if (section === 10) for (let n = leb(); n; n--) {const size = leb(); bodies.push(bytes.subarray(at, at + size)); at += size;}
    if ([1, 2, 3, 10].includes(section)) assert.equal(at, end);
    at = end;
  }
  assert.deepEqual(types[0], [[127, 127, 127, 127], [127]]);
  for (const [name, type] of imported) {
    const arity = arities[name]; assert.notEqual(arity, undefined, name);
    assert.deepEqual(types[type], [Array(arity).fill(127), [127]], name);
  }
  assert.equal(functions.length, hasGuard ? 2 : 1); assert.equal(bodies.length, functions.length); assert.equal(functions[0], 0);
  const locals = body => {let p = 0; const read = () => {let n = 0, shift = 0, v; do {v = body[p++]; n += (v & 127) * 2 ** shift; shift += 7;} while (v & 128); return n;}; return Array.from({length: read()}, () => [read(), body[p++]]);};
  // run-local allocation is internal to the emitter; Wasm validation owns it.
  if (hasGuard) {assert.deepEqual(types[functions[1]], [Array(6).fill(127), [127]]); assert.deepEqual(locals(bodies[1]), [[3, 127]]); assert.equal(bodies[1].at(-1), 11);}
  return module;
}

export function compile(c,bytes,{kind,read=false,guard=true,returnBytes,targetBytes},root,output){
 if(c.id){put(c,96,words([0]));host(c,c.stale?'retire_stale_resident':'discard_unacknowledged_resident',[c.key,KEY_HIGH,...c.id]);}c.stale=false;
 const entry=c.pc??PC,capturedTarget=targetBytes??returnBytes;c.bytes=Buffer.from(bytes);upload(c,entry,c.bytes);if(capturedTarget)upload(c,TARGET,capturedTarget);put(c,140,words(c.entries?[entry,...(capturedTarget?[TARGET]:[])]:[entry,bytes.length,...(capturedTarget?[TARGET,capturedTarget.length]:[])]));
 host(c,c.resident?c.entries?'compile_resident_entries':'compile_resident':c.entries?'compile_entries':'compile',[capturedTarget?2:1,...(c.entries?[0]:[])]);
 // Compilation, not additive export mismatch, is the counterfactual failure on the old engine.
 const declared=assertEngineExports(c.engine.module,root);
 assert.equal(declared.functions.find(row=>row.name==='ring3_abi_v1_far_call').arity,5);
 assert.equal(declared.functions.find(row=>row.name==='ring3_abi_v1_far_call_resident').arity,9);
 assert.equal(declared.functions.find(row=>row.name==='ring3_abi_v1_far_return').arity,3);
 assert.equal(declared.functions.find(row=>row.name==='ring3_abi_v1_far_return_resident').arity,7);
 const a=arena(c);c.id=c.resident?[a.readUInt32LE(148),a.readUInt32LE(152)]:null;c.binding=c.id?[c.key,KEY_HIGH,...c.id]:[];
 const address=c.id?a.readUInt32LE(156):c.api.module_ptr()>>>0,size=c.id?a.readUInt32LE(160):c.api.module_len()>>>0;
 const bytesOut=Buffer.from(new Uint8Array(c.memory.buffer,address,size)),helper=c.resident?`${kind}_resident`:kind;
 const imports=['env:memory:memory',`ring3:${c.resident?'guard_resident':'guard'}:function`,`ring3:${helper}:function`,...(returnBytes?[`ring3:${c.resident?'far_return_resident':'far_return'}:function`]:[]),...(read?['ring3:cpu_read:function']:[])];
 const arities={guard:6,guard_resident:7,cpu_read:3,far_jump:3,far_jump_resident:7,far_call:5,far_call_resident:9,far_return:3,far_return_resident:7};
 const module=validateModule(bytesOut,imports,guard,arities);c.helper=helper;
 writeFileSync(join(output,`module-${++counts.modules}.wasm`),bytesOut,{flag:'wx'});
 const ring3=Object.fromEntries(WebAssembly.Module.imports(module).filter(row=>row.kind==='function').map(row=>[row.name,(...args)=>{
  c.calls.push([row.name,args]);
  if(row.name===helper&&c.beforeHelper){const hook=c.beforeHelper;c.beforeHelper=null;hook();}
  if(row.name===helper&&c.reply){put(c,100,c.reply.packet);return c.reply.transport;}
  return c.api[row.name](...args);
 }]));
 c.run=new WebAssembly.Instance(module,{env:{memory:c.memory},ring3}).exports.run;
}
export function seed({esp=STACK,logical=POINTER,flags=0x244cd7,cancel=0,context}={}){
 const bytes=createArena();bytes.writeUInt16LE(2,4);bytes.writeUInt32LE(flags,52);bytes.writeUInt32LE(PC,48);
 [0x12345678,0x23456789,0x3456789a,0x456789ab,esp,logical,0x56789abc,logical].forEach((value,i)=>bytes.writeUInt32LE(value>>>0,16+i*4));bytes.writeUInt32LE(cancel,96);
 if(context)bytes.set(encodeSegments(context),SEG);return bytes;
}
export function run(c,label,input,want,ram,{budget=1,helpers,argumentsList}={}){
 put(c,0,input);c.calls=[];assert.equal(c.run(c.base,c.base+56,budget,c.base+96),0,label);
 assert.deepEqual(arena(c),want,`${label}: entire arena`);assertRAM(c,ram,label);
 const actual=c.calls.filter(([name])=>!name.startsWith('guard'));
 assert.deepEqual(actual.map(([name])=>name),helpers,`${label}: exact helper schedule`);
 if(argumentsList)assert.deepEqual(actual.map(([,args])=>args.map(value=>value>>>0)),argumentsList.map(args=>args.map(value=>value>>>0)),`${label}: exact helper arguments`);
 counts.runs++;const reason=want.readUInt32LE(72),retired=want.readUInt32LE(76);
 if(retired)counts.success++;else if(reason===7)counts.refusals++;else if(reason>=5)counts.faults++;else counts.controls++;
 observations.push({label,reason,retired,helpers});
}
export function exit(want,reason,retired=0,detail=0,address=0,access=0,length=0){want.set(packet('R3EX',40,[reason,retired,detail,address,access,length],reason>=16?7:6),56);}
export function publishCode(want,raw,selector,target){const state=decodeSegments(want.subarray(SEG));state.selectors[1]=(selector&0xfffc)|3;state.caches[1]=cache(raw);want.set(encodeSegments(state),SEG);want.writeUInt32LE(target>>>0,48);}
export function pins(root,paths){assert.equal(new Set(paths).size,paths.length,'no duplicate literal source pins');return Object.fromEntries(paths.map(path=>[path,createHash('sha256').update(readFileSync(join(root,path))).digest('hex')]));}
export function finish(output,engine,sourcePins,limits){writeFileSync(join(output,'result.json'),JSON.stringify({status:'ok',engine_sha256:engine.sha256,source_pins:sourcePins,counts,limits},null,2)+'\n');writeFileSync(join(output,'observations.json'),JSON.stringify(observations,null,2)+'\n');console.log(JSON.stringify({status:'ok',...counts,output}));}
