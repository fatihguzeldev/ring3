import assert from 'node:assert/strict';
import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
import {join} from 'node:path';
import {createHash} from 'node:crypto';

const [enginePath,output,label='unspecified']=process.argv.slice(2);
assert.ok(enginePath&&output,'scalar-benchmark.mjs ENGINE_WASM OUTPUT_DIRECTORY [LABEL]');
mkdirSync(output,{recursive:true});
const hash=b=>createHash('sha256').update(b).digest('hex'),now=()=>process.hrtime.bigint(),span=(a,b)=>Number(b-a);
const engineBytes=readFileSync(enginePath),program=Buffer.from('f30f10c1f20f10d3f30f10e4f20f10edebee','hex');
const BUDGET=5000,WARMUP=20,BATCH_CALLS=100,SAMPLES=5,SIZE=4612,SV=4364,T=140;
const started=now(),engineModule=new WebAssembly.Module(engineBytes),engineCompileEnd=now(),instance=new WebAssembly.Instance(engineModule,{}),engineInstantiateEnd=now();
const memory=instance.exports.memory,api={};
for(const[name,fn]of Object.entries(instance.exports))if(name.startsWith('ring3_abi_v1_')&&typeof fn==='function')api[name.slice(13)]=fn;
const host=(name,args)=>assert.equal(api[name](...args),0,name);
host('open',[4,0x5343414c,0x50455246]);const base=api.arena_ptr()>>>0;
const input=(offset,bytes)=>new Uint8Array(memory.buffer,base+offset,bytes.length).set(bytes);
const read=()=>Buffer.from(new Uint8Array(memory.buffer,base,SIZE));
const words=values=>{const b=Buffer.alloc(values.length*4);values.forEach((v,i)=>b.writeUInt32LE(v>>>0,i*4));return b;};
host('map',[0x1000,1,7]);input(T,program);host('upload',[0x1000,program.length]);host('protect',[0x1000,1,5]);input(T,words([0x1000,program.length]));
const captureStart=now();host('compile',[1]);const captureEnd=now();
const pointer=api.module_ptr()>>>0,length=api.module_len()>>>0,moduleBytes=Buffer.from(new Uint8Array(memory.buffer,pointer,length));
const guestModule=new WebAssembly.Module(moduleBytes),imports=WebAssembly.Module.imports(guestModule),ring3={};let helperCalls=0,guardCalls=0;
for(const im of imports){
 if(im.kind==='memory'){assert.deepEqual(im,{module:'env',name:'memory',kind:'memory'});continue;}
 assert.equal(im.module,'ring3');assert.equal(im.kind,'function');assert.ok(im.name==='guard'||im.name==='simd_execute',im.name);
 ring3[im.name]=(...args)=>{if(im.name==='guard')guardCalls++;else helperCalls++;return api[im.name](...args);};
}
const run=new WebAssembly.Instance(guestModule,{env:{memory},ring3}).exports.run,generatedEnd=now();assert.equal(run.length,4);
const xmm=[
 [0xdeadbeef,0x7fc54321,0x80000000,0x01234567],
 [0x7f801234,0xff800000,0x76543210,0x89abcdef],
 [0x11223344,0x55667788,0,0x80000000],
 [0x00001234,0x7ff00000,0xdeadbeef,0xfff81234],
 [0x80000000,0x7f800001,0x01234567,0x89abcdef],
 [0x56789abc,0xfff81234,0,0xfff00000],
 [0x00000001,0x80000001,0x7f800000,0xff800000],
 [0xffffffff,0,0x7fffffff,0x80000000],
];
input(16,words([0x10203040,0x21314151,0x32425262,0x43536373,0x7080,0x65758595,0x768696a6,0x8797a7b7,0x1000,0xcd7]));
xmm.forEach((values,index)=>input(SV+32+index*16,words(values)));
const seeded=read(),expectedXmm=xmm.map(values=>[...values]);expectedXmm[0][0]=xmm[1][0];expectedXmm[2][0]=xmm[3][0];expectedXmm[2][1]=xmm[3][1];
const invoke=()=>{const status=run(base,base+56,BUDGET,base+96);assert.equal(status,0);};
function check(){
 const actual=read(),expected=Buffer.from(seeded);expected.writeUInt16LE(6,60);expected.writeUInt32LE(1,72);expected.writeUInt32LE(BUDGET,76);
 expectedXmm.forEach((values,index)=>expected.set(words(values),SV+32+index*16));
 if(helperCalls){const p=Buffer.alloc(40);p.write('R3MH');p.writeUInt16LE(7,4);p.writeUInt16LE(1,6);p.writeUInt32LE(40,8);expected.set(p,100);expected.set(words([0,0xcd7]),T+512);}
 assert.deepEqual(actual,expected,'literal raw scalar bits, preserved upper lanes, MXCSR/x87/segments/work and complete arena');
}
const beforeFirst=helperCalls,firstStart=now();invoke();const firstEnd=now();check();const firstHelpers=helperCalls-beforeFirst;
assert.ok(firstHelpers===0||firstHelpers===BUDGET*4/5,'exact scalar helper crossings');
const beforeWarmup=helperCalls;for(let i=0;i<WARMUP;i++)invoke();check();const warmupHelpers=helperCalls-beforeWarmup;assert.equal(warmupHelpers,WARMUP*firstHelpers);
const samples=[];
for(let sample=0;sample<SAMPLES;sample++){
 const helpersBefore=helperCalls,guardsBefore=guardCalls,t0=now();for(let i=0;i<BATCH_CALLS;i++)invoke();const t1=now();check();
 const crossings=helperCalls-helpersBefore,guestInstructions=BATCH_CALLS*BUDGET;assert.equal(crossings,BATCH_CALLS*firstHelpers);
 samples.push({sample,elapsed_ns:span(t0,t1),calls:BATCH_CALLS,retired_guest_instructions:guestInstructions,simd_helper_calls:crossings,guard_calls:guardCalls-guardsBefore,ns_per_guest_instruction:span(t0,t1)/guestInstructions});
}
const final=read(),totalRuns=1+WARMUP+SAMPLES*BATCH_CALLS,totalRetired=totalRuns*BUDGET;
assert.equal(helperCalls,totalRuns*firstHelpers);const sorted=samples.map(s=>s.ns_per_guest_instruction).sort((a,b)=>a-b);
const result={status:'PASS',label,benchmark_source_sha256:hash(readFileSync(new URL(import.meta.url))),engine:{path:enginePath,sha256:hash(engineBytes),bytes:engineBytes.length},
 program:{hex:program.toString('hex'),sha256:hash(program),instructions_per_cycle:5,scalar_moves_per_cycle:4,profile:'replacement explicit single block; register MOVSS/MOVSD including self-alias; RX code'},
 generated:{sha256:hash(moduleBytes),bytes:moduleBytes.length,imports},
 protocol:{budget:BUDGET,warmup_calls:WARMUP,batch_calls:BATCH_CALLS,samples:SAMPLES,clock:'process.hrtime.bigint; monotonic nanoseconds',timed:'generated run calls including forwarding/guards, crossing counters and status assertion; complete-arena/literal checks outside measured windows'},
 cold:{engine_compile_ns:span(started,engineCompileEnd),engine_instantiate_ns:span(engineCompileEnd,engineInstantiateEnd),guest_capture_ns:span(captureStart,captureEnd),generated_compile_and_instantiate_ns:span(captureEnd,generatedEnd),first_run_ns:span(firstStart,firstEnd),first_run_retired:BUDGET,first_run_simd_helpers:firstHelpers},
 warmup:{calls:WARMUP,retired:WARMUP*BUDGET,simd_helper_calls:warmupHelpers},samples,median_ns_per_guest_instruction:sorted[2],total:{runs:totalRuns,retired:totalRetired,simd_helpers:helperCalls,guard_calls:guardCalls},
 literal_xmm_before:xmm.map(words).map(b=>b.toString('hex')),literal_xmm_after:expectedXmm.map(words).map(b=>b.toString('hex')),final_mxcsr:final.readUInt32LE(SV+16),
 scope:'one deterministic register scalar-move chain with signaling/quiet NaN and signed-zero raw bits, upper-lane preservation and self-alias. Cold and warm values separate; no application, memory bandwidth, whole CPU or arithmetic floating-point speed claim'};
writeFileSync(join(output,'module.wasm'),moduleBytes,{flag:'wx'});writeFileSync(join(output,'final-arena.bin'),final,{flag:'wx'});writeFileSync(join(output,'result.json'),JSON.stringify(result,null,2)+'\n',{flag:'wx'});host('close',[]);console.log(JSON.stringify({status:'PASS',label,median_ns_per_guest_instruction:result.median_ns_per_guest_instruction,total:result.total,output}));
