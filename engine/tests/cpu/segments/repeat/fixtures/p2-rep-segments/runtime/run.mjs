import assert from 'node:assert/strict';
import {readFileSync,writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {createHash} from 'node:crypto';
import {readEngine} from '../../../../../../support/wasm/engine.mjs';

const [enginePath,output,root]=process.argv.slice(2),engine=readEngine(enginePath);
const initial=readFileSync(join(output,'initial-arena.bin')),SIZE=initial.length,SEGMENTS=4524,WORK=4588;
assert.ok(SIZE>=WORK+24);
const hash=b=>createHash('sha256').update(b).digest('hex'),cases=[],modules=[],observations=[],helpers=[];
const sourcePaths=['engine/src/cpu/x86/decode/strings.rs','engine/src/cpu/dbt/wasm/strings.rs',
 'engine/src/cpu/x86/segments.rs','engine/src/cpu/x86/segments/cache.rs','engine/src/abi/x86/segments.rs','engine/src/cpu/dbt/wasm/segments.rs',
 'engine/src/cpu/x86/ir.rs','engine/src/cpu/x86/decode/profile.rs','engine/src/cpu/x86/decode/lower.rs',
 'engine/src/cpu/x86/decode/operands.rs','engine/src/cpu/dbt/wasm/emitter.rs','engine/src/cpu/dbt/wasm/memory.rs',
 'engine/src/cpu/dbt/wasm/abi.rs','engine/src/cpu/dbt/wasm/dispatcher.rs','engine/src/abi/arena.rs',
 'engine/tests/cpu/segments/repeat/wasm/overrides.rs', 'engine/tests/cpu/segments/wasm.rs', 'engine/tests/support/wasm.rs', 'engine/tests/cpu/segments/repeat/fixtures/p2-rep-segments/runtime/run.mjs'];
const pins=()=>Object.fromEntries(sourcePaths.map(p=>[p,hash(readFileSync(join(root,p)))])),beforePins=pins();
function saveModule(c,bytes,label){const file=`module-${modules.length+1}.wasm`;writeFileSync(join(output,file),bytes);modules.push({context:c.id,owner:c.owner,entries:c.entries,label,file,bytes:bytes.length,sha256:hash(bytes),imports:WebAssembly.Module.imports(new WebAssembly.Module(bytes))});}
function record(magic,size,fields,version=1){const b=Buffer.alloc(size);b.write(magic);b.writeUInt16LE(version,4);b.writeUInt16LE(1,6);b.writeUInt32LE(size,8);fields.forEach((v,i)=>b.writeUInt32LE(v>>>0,16+i*4));return b;}
const state=s=>record('R3ST',56,[...s.r,s.pc,s.flags]);
const arena=c=>Buffer.from(new Uint8Array(c.memory.buffer,c.base,SIZE));
const cpu=c=>{const a=arena(c);return{r:Array.from({length:8},(_,i)=>a.readUInt32LE(16+i*4)),pc:a.readUInt32LE(48),flags:a.readUInt32LE(52)};};
function input(c,offset,b){new Uint8Array(c.memory.buffer).set(b,c.base+offset);}
function host(c,n,args=[]){assert.equal(c.api[n](...args),0,n);}
function upload(c,address,b){input(c,140,b);host(c,'upload',[address,b.length]);}
function seed(c,{count=3,source=0x5010,destination=0x6010,eax=0xabcd1234,df=false,pc=0x1000}={}){
 const s={r:[eax,count,0x23456789,0x3456789a,0x7890,0x456789ab,source,destination],pc,flags:df?0xcd7:0x8d7};
 input(c,0,state(s));input(c,96,Buffer.alloc(4));return s;
}
function open(owner,bytes,entries=false,tail=[0xeb,0xfe]){
 const instance=new WebAssembly.Instance(engine.module,{}),c={id:cases.length+1,owner,entries,memory:instance.exports.memory,api:{},cancelAfter:0,stores:0};
 for(const n of ['open','close','arena_ptr','map','protect','upload','read8','read16','read32','compile','compile_entries','compile_resident','compile_resident_entries','generation','module_ptr','module_len','guard','guard_resident','store8','store16','store32','store_resident8','store_resident16','store_resident32','acknowledge_resident_installation','dispatcher_module','guard_dispatch_entry','find_installed_resident'])c.api[n]=instance.exports['ring3_abi_v1_'+n];
 host(c,'open',[8,c.id,0x5250]);c.base=c.api.arena_ptr()>>>0;input(c,0,initial);
 for(const [p,permission] of [[0x1000,7],[0x5000,3],[0x6000,3]])host(c,'map',[p,1,permission]);
 const code=Buffer.from([...bytes,...tail]);upload(c,0x1000,code);host(c,'protect',[0x1000,1,5]);
 const d=Buffer.alloc(entries?4:8);d.writeUInt32LE(0x1000);if(!entries)d.writeUInt32LE(code.length,4);input(c,140,d);
 const method=owner==='resident'?(entries?'compile_resident_entries':'compile_resident'):(entries?'compile_entries':'compile');
 host(c,method,entries?[1,0]:[1]);const a=arena(c),pointer=owner==='resident'?a.readUInt32LE(156):c.api.module_ptr()>>>0,length=owner==='resident'?a.readUInt32LE(160):c.api.module_len()>>>0;
 c.residentLow=a.readUInt32LE(148);c.residentHigh=a.readUInt32LE(152);
 const wasm=Buffer.from(new Uint8Array(c.memory.buffer,pointer,length)),module=new WebAssembly.Module(wasm),ring3={...c.api};
 for(const imp of WebAssembly.Module.imports(module).filter(x=>x.module==='ring3'&&x.name!=='guard'&&x.name!=='guard_resident')){
  ring3[imp.name]=(...args)=>{const before=arena(c),status=c.api[imp.name](...args),after=arena(c);before.set(after.subarray(100,140),100);
   assert.deepEqual(after,before,'canonical helper only changes40B');
   helpers.push({context:c.id,name:imp.name,args:args.map(x=>x>>>0),status,packet:after.subarray(100,140).toString('hex')});
   if(imp.name.startsWith('store')&&status===0&&++c.stores===c.cancelAfter)input(c,96,Buffer.from([1,0,0,0]));
   return status;
  };
 }
 c.ring3=ring3;c.run=new WebAssembly.Instance(module,{env:{memory:c.memory},ring3}).exports.run;c.length=bytes.length;
 saveModule(c,wasm,'initial capture');
 cases.push({id:c.id,owner,entries,code:code.toString('hex')});return c;
}
function run(c,budget,expected,{reason=1,retired=0,work=budget,label='REP',fault,installation}={}){
 const before=arena(c),at=helpers.length,status=c.run(c.base,c.base+56,budget,c.base+96),after=arena(c);
 assert.equal(status,0,label);assert.deepEqual(cpu(c),expected,label+' CPU');
 assert.equal(after.readUInt32LE(72),reason,label+' reason');assert.equal(after.readUInt32LE(76),retired,label+' architectural retirement');
 if(fault)assert.deepEqual(Array.from({length:4},(_,i)=>after.readUInt32LE(80+i*4)),fault,label+' exact fault');
 assert.equal(after.readUInt32LE(WORK+16),work,label+' consumed work');
 const full=Buffer.from(before);full.set(state(expected));full.set(after.subarray(56,96),56);full.set(after.subarray(100,140),100);full.set(record('R3WK',24,[work,0]),WORK);
 // Dispatcher lookup publishes exactly its typed 32-byte installation receipt.
 if(installation)full.set(record('R3IN',32,[...installation,0]),140);
 // a test cancellation shim is the only permitted external write during a child call.
 if(c.cancelAfter)full.set(after.subarray(96,100),96);
 assert.deepEqual(after,full,label+' complete arena');
 const file=`arena-${observations.length+1}.bin`;writeFileSync(join(output,file),after);
 observations.push({context:c.id,label,budget,status,reason,retired,work,cpu:cpu(c),helpers:helpers.length-at,exit:after.subarray(56,96).toString('hex'),arena:{file,bytes:after.length,sha256:hash(after)}});return after;
}
function get(c,address,width){host(c,'read'+width*8,[address]);return arena(c).readUInt32LE(120);}
function putValues(c,address,width,values){const b=Buffer.alloc(width*values.length);values.forEach((v,i)=>width===1?b.writeUInt8(v&255,i):width===2?b.writeUInt16LE(v&65535,i*2):b.writeUInt32LE(v>>>0,i*4));upload(c,address,b);}
function moved(s,{count,source,destination,eax,pc=s.pc,flags=s.flags}){const n={r:[...s.r],pc,flags};for(const [i,v]of [[1,count],[6,source],[7,destination],[0,eax]])if(v!==undefined)n.r[i]=v>>>0;return n;}
function compareFlags(lhs,rhs){const value=(lhs-rhs)&255;return 2|(lhs<rhs?1:0)|((value.toString(2).replaceAll('0','').length%2===0)?4:0)|((lhs^rhs^value)&16)|(value===0?64:0)|(value&128)|(((lhs^rhs)&(lhs^value)&128)?0x800:0);}
const opcodes={Move:0xa4,Store:0xaa,Load:0xac,Compare:0xa6,Scan:0xae};
for(const owner of ['replacement','resident'])for(const [kind,opcode]of Object.entries(opcodes))for(const width of [1,2,4]){
 const code=[0xf3,...(width===2?[0x66]:[]),opcode+(width===1?0:1)],c=open(owner,code),mask=width===1?255:width===2?65535:0xffffffff;
 const vals=kind==='Scan'?[0x34,0x34,0x34]:[0x11,0x22,0x33];putValues(c,0x5010,width,vals);putValues(c,0x6010,width,kind==='Compare'?vals:kind==='Scan'?vals:[0,0,0]);
 const s=seed(c,{eax:kind==='Scan'?0x34:0xabcd1234});
 let partial=moved(s,{count:2,source:0x5010+(kind==='Move'||kind==='Load'||kind==='Compare'?width:0),destination:0x6010+(kind==='Move'||kind==='Store'||kind==='Compare'||kind==='Scan'?width:0),eax:kind==='Load'?((s.r[0]&~mask)|vals[0])>>>0:undefined,flags:kind==='Compare'||kind==='Scan'?0x46:s.flags});
 run(c,1,partial,{label:kind+width+' partial'});
 const done=moved(partial,{count:0,source:0x5010+(kind==='Move'||kind==='Load'||kind==='Compare'?3*width:0),destination:0x6010+(kind==='Move'||kind==='Store'||kind==='Compare'||kind==='Scan'?3*width:0),eax:kind==='Load'?((s.r[0]&~mask)|vals[2])>>>0:undefined,pc:0x1000+code.length});
 run(c,2,done,{retired:1,label:kind+width+' natural completion'});
 if(kind==='Move'||kind==='Store')for(let i=0;i<3;i++)assert.equal(get(c,0x6010+i*width,width),kind==='Move'?vals[i]:(s.r[0]&mask)>>>0);
 const zero=seed(c,{count:0,source:0xffffffff,destination:0xffffffff});const at=helpers.length;
 run(c,1,{...zero,pc:0x1000+code.length},{retired:1,label:kind+width+' zero count'});assert.equal(helpers.length,at);
 host(c,'close');
}
// conditional early completion and both repeat encodings are distinct from count exhaustion.
for(const owner of ['replacement','resident']){
 const c=open(owner,[0xf3,0xa4]),a=arena(c);a.writeUInt16LE(0,SEGMENTS+16);input(c,SEGMENTS,a.subarray(SEGMENTS,SEGMENTS+64));
 const zero=seed(c,{count:0}),at=helpers.length;
 run(c,1,{...zero,pc:0x1002},{retired:1,label:'null ES zero REP count bypasses access'});assert.equal(helpers.length,at);
 const live=seed(c,{count:1});run(c,1,live,{retired:0,reason:11,work:0,fault:[0,0,0,0],label:'null ES prevents source and destination helpers'});assert.equal(helpers.length,at);host(c,'close');
}
for(const [prefix,kind,values,expectedCount]of [[0xf3,'Compare',[1,9,3],2],[0xf2,'Compare',[4,2,6],2],[0xf3,'Scan',[7,9,7],2],[0xf2,'Scan',[8,7,9],2]]){
 const c=open('replacement',[prefix,opcodes[kind]]);putValues(c,0x5010,1,[1,2,3]);putValues(c,0x6010,1,values);
 const s=seed(c,{count:4,eax:7}),flags=compareFlags(kind==='Compare'?2:7,values[1]);
 run(c,2,moved(s,{count:expectedCount,source:kind==='Compare'?0x5012:0x5010,destination:0x6012,pc:0x1002,flags}),{retired:1,label:'conditional '+prefix+kind});host(c,'close');
}
// DF and address16 update the relevant pointer/count parts without replacing parent registers.
{
 const c=open('resident',[0xf2,0x67,0x66,0xa5],true);putValues(c,0x5010,2,[0x1122,0x3344]);
 let s=seed(c,{count:0xabcd0002,source:0x12345012,destination:0x56786012,df:true});
 const done=moved(s,{count:0xabcd0000,source:0x1234500e,destination:0x5678600e,pc:0x1004});
 run(c,2,done,{retired:1,label:'F2 address16 DF MOVSW'});assert.equal(get(c,0x6010,2),0x1122);assert.equal(get(c,0x6012,2),0x3344);host(c,'close');
}
// repair changes only the mapping; already completed elements and architectural CPU remain live.
{
 const c=open('resident',[0xf3,0xa4]);putValues(c,0x5010,1,[1,2,3,4]);const s=seed(c,{destination:0x5011});
 run(c,3,moved(s,{count:0,source:0x5013,destination:0x5014,pc:0x1002}),{retired:1,label:'forward overlap observes prior element writes'});
 for(let i=0;i<4;i++)assert.equal(get(c,0x5010+i,1),1);host(c,'close');
}
{
 const c=open('replacement',[0xf3,0xa4]);putValues(c,0x5010,1,[0x11,0x22,0x33]);let s=seed(c,{destination:0x6fff});
 const stopped=moved(s,{count:2,source:0x5011,destination:0x7000});run(c,3,stopped,{reason:5,work:1,fault:[1,0x7000,2,1],label:'second element write fault'});
 assert.equal(get(c,0x6fff,1),0x11);host(c,'map',[0x7000,1,3]);
 run(c,2,moved(stopped,{count:0,source:0x5013,destination:0x7002,pc:0x1002}),{retired:1,label:'mapping-only retry'});assert.equal(get(c,0x7000,1),0x22);assert.equal(get(c,0x7001,1),0x33);host(c,'close');
}
{
 const c=open('resident',[0xf3,0xaa]);const s=seed(c,{eax:0x55});c.cancelAfter=1;
 const stopped=moved(s,{count:2,destination:0x6011});run(c,3,stopped,{reason:2,work:1,label:'cancellation after one element'});
 input(c,96,Buffer.alloc(4));c.cancelAfter=0;
 run(c,2,moved(stopped,{count:0,destination:0x6013,pc:0x1002}),{retired:1,label:'cancel cleared same CPU retry'});host(c,'close');
}
// FS applies to the source only; ES destination remains flat.
for(const segment of [4,5]){
 const c=open('replacement',[segment===4?0x64:0x65,0xf3,0xa4]);putValues(c,0x5010,1,[0x91,0x82]);
 const a=arena(c);a.writeUInt16LE(0x3b,SEGMENTS+16+segment*2);a.writeUInt32LE(0x5000,SEGMENTS+32+segment*4);input(c,SEGMENTS,a.subarray(SEGMENTS,SEGMENTS+64));
 const s=seed(c,{count:2,source:0x10});run(c,2,moved(s,{count:0,source:0x12,destination:0x6012,pc:0x1003}),{retired:1,label:(segment===4?'FS':'GS')+' source base'});
 assert.equal(get(c,0x6010,1),0x91);assert.equal(get(c,0x6011,1),0x82);host(c,'close');
}
// a failed second comparison read does not publish flags or either pointer.
{
 const c=open('resident',[0xf3,0xa6]);putValues(c,0x5010,1,[1,2,3]);putValues(c,0x6010,1,[1,2,3]);
 host(c,'protect',[0x5000,1,0]);host(c,'protect',[0x6000,1,0]);const s=seed(c);
 run(c,3,s,{reason:5,work:0,fault:[2,0x5010,1,1],label:'CMPS source read priority'});
 host(c,'protect',[0x5000,1,3]);run(c,3,s,{reason:5,work:0,fault:[2,0x6010,1,1],label:'CMPS second read publication neutral'});
 host(c,'protect',[0x6000,1,3]);run(c,3,moved(s,{count:0,source:0x5013,destination:0x6013,pc:0x1002,flags:0x46}),{retired:1,label:'CMPS protect-only retry'});host(c,'close');
}
{
 const c=open('replacement',[0xf3,0x66,0xa5]);putValues(c,0x5010,2,[0xabcd]);const s=seed(c,{count:1,destination:0x6fff});
 run(c,1,s,{reason:5,work:0,fault:[1,0x7000,2,2],label:'MOVSW cross-page store is element neutral'});assert.equal(get(c,0x6fff,1),0);
 host(c,'map',[0x7000,1,3]);run(c,1,moved(s,{count:0,source:0x5012,destination:0x7001,pc:0x1003}),{retired:1,label:'MOVSW cross-page repair'});assert.equal(get(c,0x6fff,2),0xabcd);host(c,'close');
}
{
 const c=open('replacement',[0x64,0xf3,0xad]);
 const a=arena(c);a.writeUInt32LE(0x1000,SEGMENTS+48);input(c,SEGMENTS,a.subarray(SEGMENTS,SEGMENTS+64));
 const s=seed(c,{count:1,source:0xffffefff});
 run(c,1,s,{reason:5,work:0,fault:[3,0xffffffff,1,4],label:'LODSD source overflow before publication'});host(c,'close');
}
// ordinary explicit memory uses FS/GS; LEA and implicit SS stack addresses stay offsets/flat.
for(const segment of [4,5]){
 const prefix=segment===4?0x64:0x65,c=open('replacement',[prefix,0x8b,0x00,prefix,0x8d,0x48,0x04]);
 putValues(c,0x5010,4,[0x12345678]);const a=arena(c);a.writeUInt16LE(0x3b,SEGMENTS+16+segment*2);a.writeUInt32LE(0x5000,SEGMENTS+32+segment*4);input(c,SEGMENTS,a.subarray(SEGMENTS,SEGMENTS+64));
 const s=seed(c,{eax:0x10}),done={r:[...s.r],pc:0x1007,flags:s.flags};done.r[0]=0x12345678;done.r[1]=0x1234567c;
 run(c,2,done,{retired:2,work:0,label:'segment MOV and LEA ignore base'});host(c,'close');
 const p=open('resident',[prefix,0xff,0x35,0x10,0,0,0]);putValues(p,0x5010,4,[0x12345678]);const pa=arena(p);pa.writeUInt16LE(0x3b,SEGMENTS+16+segment*2);pa.writeUInt32LE(0x5000,SEGMENTS+32+segment*4);input(p,SEGMENTS,pa.subarray(SEGMENTS,SEGMENTS+64));
 const ps=seed(p);ps.r[4]=0x6040;input(p,0,state(ps));const pd={r:[...ps.r],pc:0x1007,flags:ps.flags};pd.r[4]=0x603c;
 run(p,1,pd,{retired:1,work:0,label:'segment source PUSH keeps implicit stack flat'});assert.equal(get(p,0x603c,4),0x12345678);host(p,'close');
}
// an executable write commits one element and stops before replaying stale code.
for(const count of [1,2]){
 const c=open('replacement',[0xf3,0xaa]);host(c,'protect',[0x1000,1,7]);
 // protection changes invalidate the old capture, so capture current code before execution.
 const d=Buffer.alloc(8);d.writeUInt32LE(0x1000);d.writeUInt32LE(4,4);input(c,140,d);host(c,'compile',[1]);
 const wasm=Buffer.from(new Uint8Array(c.memory.buffer,c.api.module_ptr()>>>0,c.api.module_len()>>>0));saveModule(c,wasm,'SMC current recapture');c.run=new WebAssembly.Instance(new WebAssembly.Module(wasm),{env:{memory:c.memory},ring3:c.ring3}).exports.run;
 const s=seed(c,{count,eax:0x90,destination:0x1002}),done=moved(s,{count:count-1,destination:0x1003,pc:count===1?0x1002:0x1000});
 run(c,count,done,{reason:6,retired:count===1?1:0,work:1,label:'SMC REP '+(count===1?'complete':'partial')});assert.equal(get(c,0x1002,1),0x90);
 const before=arena(c);assert.equal(c.run(c.base,c.base+56,1,c.base+96),4);assert.deepEqual(arena(c),before,'stale REP refusal neutral');host(c,'close');
}
// dispatch must subtract repetition work rather than only the one retired REP instruction.
{
 const c=open('resident',[0xf3,0xaa],false,[0xe9,0xf9,0x0f,0,0]),table=new WebAssembly.Table({initial:8,maximum:8,element:'anyfunc'});
 table.set(0,c.run);host(c,'acknowledge_resident_installation',[c.id,0x5250,c.residentLow,c.residentHigh,0]);
 host(c,'map',[0x2000,1,7]);upload(c,0x2000,Buffer.from([0x90,0xeb,0xfe]));host(c,'protect',[0x2000,1,5]);
 const d=Buffer.alloc(8);d.writeUInt32LE(0x2000);d.writeUInt32LE(3,4);input(c,140,d);host(c,'compile_resident',[1]);let a=arena(c);
 const low=a.readUInt32LE(148),high=a.readUInt32LE(152),body=Buffer.from(new Uint8Array(c.memory.buffer,a.readUInt32LE(156),a.readUInt32LE(160)));
 saveModule(c,body,'dispatcher second child');
 table.set(1,new WebAssembly.Instance(new WebAssembly.Module(body),{env:{memory:c.memory},ring3:c.api}).exports.run);host(c,'acknowledge_resident_installation',[c.id,0x5250,low,high,1]);
 host(c,'dispatcher_module',[c.id,0x5250]);a=arena(c);const dispatch=Buffer.from(new Uint8Array(c.memory.buffer,a.readUInt32LE(164),a.readUInt32LE(168)));
 saveModule(c,dispatch,'work-budget dispatcher');
 c.run=new WebAssembly.Instance(new WebAssembly.Module(dispatch),{env:{memory:c.memory,table},ring3:c.api}).exports.run;
 const s=seed(c,{eax:0x55}),done=moved(s,{count:0,destination:0x6013,pc:0x2000});run(c,4,done,{retired:2,work:4,installation:[c.residentLow,c.residentHigh,0],label:'dispatcher cold transition consumes REP element work'});
 run(c,1,{...done,pc:0x2001},{retired:1,work:0,installation:[low,high,1],label:'dispatcher resumes untouched second child'});host(c,'close');
}
assert.deepEqual(pins(),beforePins);
writeFileSync(join(output,'result.json'),JSON.stringify({engine:{path:enginePath,sha256:engine.sha256,bytes:engine.bytes.length},source_pins:beforePins,arena_bytes:SIZE,cases,modules,observations,helper_trace:helpers,
 claim:'Generated private Wasm REP family restart, count/DF/condition, budget/cancel, selected fault repair, address16 and FS/GS source-base evidence; architectural retirement is once on completion. Selected RAM only; no whole-memory or general protected-mode descriptor claim.'},null,2)+'\n');
console.log(JSON.stringify({output,contexts:cases.length,modules:modules.length,runs:observations.length,helpers:helpers.length}));
