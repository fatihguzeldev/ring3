import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
import {join} from 'node:path';
import {pathToFileURL} from 'node:url';
const [enginePath,output,root]=process.argv.slice(2);
assert.ok(enginePath&&output&&root,'selected engine, fresh output and repo root required');
assert.match(process.env.RING3_ENGINE_SHA256??'',/^[a-f0-9]{64}$/,'pin selected engine');
const {readEngine}=await import(pathToFileURL(join(root,'engine/tests/support/wasm/engine.mjs')));
const engine=readEngine(enginePath);mkdirSync(output,{recursive:true});
const PC=0x1000,SIZE=4796,TRANSFER=140,BASES=4756,FP=4236,SIMD=4364,YMM=4612;
const original=[0x11110001,0x22220002,0x33330003,0x44440004,0x3000,0x55550005,0x66660006,0x77770007];
const hash=b=>createHash('sha256').update(b).digest('hex');
const paths=['engine/src/cpu/dbt/wasm/x87.rs','engine/src/cpu/dbt/wasm/avx/execute.rs',
  'engine/src/cpu/dbt/wasm/avx/direct.rs','engine/src/cpu/dbt/wasm/code_fetch.rs',
  'engine/src/process/cpu/x87.rs','engine/src/process/cpu/avx.rs',
  'engine/src/cpu/x86/x87/execute.rs','engine/src/cpu/x86/decode/lower.rs','engine/src/abi/x86/avx.rs',
  'engine/src/cpu/x86/avx/predicates.rs',
  'engine/src/cpu/x86/avx/strings.rs','engine/src/cpu/x86/decode/avx/native/fixtures/canonical.tsv',
  'engine/src/process/execution/authority.rs','engine/src/process/cpu/segments.rs',
  'engine/src/abi/x86/segments.rs','engine/tests/support/wasm/engine.mjs'];
const pins=()=>Object.fromEntries(paths.map(p=>[p,hash(readFileSync(join(root,p)))]));
const sourcePins=pins(),rows=[];
function words(values){const b=Buffer.alloc(values.length*4);values.forEach((v,i)=>b.writeUInt32LE(v>>>0,4*i));return b;}
function record(magic,size,version,values){const b=Buffer.alloc(size);b.write(magic);b.writeUInt16LE(version,4);
  b.writeUInt16LE(1,6);b.writeUInt32LE(size,8);b.set(words(values),16);return b;}
const arena=c=>Buffer.from(new Uint8Array(c.memory.buffer,c.base,SIZE));
const input=(c,offset,bytes)=>new Uint8Array(c.memory.buffer).set(bytes,c.base+offset);
function host(c,name,args=[]){assert.equal(c.api[name](...args),0,name);}
function upload(c,address,bytes){input(c,TRANSFER,bytes);host(c,'upload',[address,bytes.length]);}
const variants=[
  {name:'vptest-xmm-flags-only',code:'c4e27917ca',kind:'test',flags:0x42},
  {name:'vpcmpistri-index-only',code:'c4e37963ca00',kind:'index',flags:0xc3},
  {name:'fwait-neutral-success',code:'9b',kind:'wait',fp:true},
  {name:'fnop-neutral-success',code:'d9d0',kind:'noop',fp:true},
  {name:'direct-fnstsw-ax-has-no-helper',code:'dfe0',kind:'status',fp:true,direct:true,control:true},
  {name:'pending-fwait-retains-fault-and-RF',code:'9b',kind:'pending',fp:true,pending:true,control:true},
  {name:'direct-fnclex-has-no-helper',code:'dbe2',kind:'clear',fp:true,direct:true,stateful:true,control:true},
  {name:'vpcmpistrm-committed-mask-retires-once',code:'c4e37962ca00',kind:'mask',flags:0xc3,stateful:true,control:true},
];
let context=0;
for(const owner of ['replacement-range','resident-entry'])for(const variant of variants)for(const changeKey of variant.direct?[false]:variant.control?[true]:[false,true]){
  const id=++context,label=`${owner}-${variant.name}-${changeKey?'late-key-change':'flat-control'}`,trace=[],failures=[];
  try{
    const instance=new WebAssembly.Instance(engine.module,{}),c={memory:instance.exports.memory,api:{}};
    for(const [name,value]of Object.entries(instance.exports))if(name.startsWith('ring3_abi_v1_')&&typeof value==='function')c.api[name.slice(13)]=value;
    host(c,'open',[2,id,0x4e53]);c.base=c.api.arena_ptr()>>>0;host(c,'map',[PC,1,7]);
    host(c,'set_segment_base',[2,0x5000]);
    const code=Buffer.from(variant.code+'ebfe','hex');upload(c,PC,code);
    const resident=owner==='resident-entry';input(c,TRANSFER,words(resident?[PC]:[PC,code.length]));
    host(c,resident?'compile_resident_entries':'compile',resident?[1,0]:[1]);
    const prepared=arena(c),pointer=resident?prepared.readUInt32LE(156):c.api.module_ptr()>>>0;
    const length=resident?prepared.readUInt32LE(160):c.api.module_len()>>>0;
    const moduleBytes=Buffer.from(new Uint8Array(c.memory.buffer,pointer,length)),module=new WebAssembly.Module(moduleBytes),ring3={...c.api};
    const imports=WebAssembly.Module.imports(module),helpers=imports.filter(i=>i.module==='ring3'&&(variant.fp?i.name.startsWith('x87_'):i.name.startsWith('avx_execute')));
    assert.equal(helpers.length,variant.direct?0:1,'decoded direct/control or real helper binding');
    const helperPacket=record('R3MH',40,7,variant.pending?[4,0,0x8081,0,0,0]:[0,variant.fp?2:0,0,0,0,0]);
    const payload=Buffer.alloc(32);payload.writeUInt32LE(variant.kind==='index'?1:0,0);payload.writeUInt32LE(variant.flags??2,4);
    function applyRealPublication(expected){
      if(!variant.direct)expected.set(helperPacket,100);
      if(!variant.fp)expected.set(payload,TRANSFER+512);
      if(variant.pending)expected.writeUInt16LE(0x8081,FP+18);
      if(variant.kind==='clear')expected.writeUInt16LE(0,FP+18);
      if(variant.kind==='mask'){
        expected.fill(0,SIMD+32,SIMD+48);expected.writeUInt16LE(2,SIMD+32);
        expected.fill(0,YMM+16,YMM+32);
      }
    }
    for(const imp of helpers)ring3[imp.name]=(...args)=>{
      const before=arena(c),status=c.api[imp.name](...args),staged=arena(c),expected=Buffer.from(before);
      applyRealPublication(expected);
      assert.equal(status,0,'real helper transport success');
      assert.deepEqual(staged,expected,'literal real helper architectural/scratch publication boundary');
      if(changeKey)host(c,'set_segment_base',[1,0x4000]);
      trace.push({name:imp.name,args:args.map(v=>v>>>0),status,changed_cs_base:changeKey?0x4000:0,staged_arena_sha256:hash(staged)});
      return status;
    };
    const run=new WebAssembly.Instance(module,{env:{memory:c.memory},ring3}).exports.run;
    const x87=record('R3FP',128,1,[]);x87.writeUInt16LE(variant.pending||variant.kind==='clear'?0x37e:0x37f,16);
    x87.writeUInt16LE(variant.pending?1:variant.kind==='clear'?0x8081:0x4000,18);x87.writeUInt16LE(0xffff,20);input(c,FP,x87);
    const simd=record('R3SV',160,1,[]);simd.writeUInt32LE(0x1f80,16);
    const ymm=record('R3YU',144,1,[]);simd.fill(0xa5,32,48);ymm.fill(0x5a,16,32);
    if(variant.kind==='test')simd.fill(0xff,64,80);
    else{simd.set(Buffer.from('41420000000000000000000000000000','hex'),48);simd.set(Buffer.from('58420000000000000000000000000000','hex'),64);}
    input(c,SIMD,simd);input(c,YMM,ymm);
    input(c,0,record('R3ST',56,variant.fp?3:1,[...original,PC,variant.fp?0x10002:2]));input(c,96,words([0]));
    const before=arena(c),expected=Buffer.from(before),registers=[...original];
    const neutral=changeKey&&!variant.pending&&!variant.stateful;
    const retired=neutral||variant.pending?0:1,pc=retired?PC+variant.code.length/2:PC;
    const flags=variant.pending?0x10002:neutral?2:variant.flags??2;
    if(!neutral&&variant.kind==='index')registers[1]=1;
    if(!neutral&&variant.kind==='status')registers[0]=0x11114000;
    applyRealPublication(expected);
    if(changeKey)expected.writeUInt32LE(0x4000,BASES+20);
    expected.set(record('R3ST',56,variant.fp?3:1,[...registers,pc,flags]),0);
    expected.set(record('R3EX',40,variant.direct?1:6,variant.pending?[12,0,0x8081,0,0,0]:[neutral?3:1,retired,0,0,0,0]),56);
    const status=run(c.base,c.base+56,1,c.base+96),after=arena(c);
    for(const [name,actual,wanted]of [['CPU/FLAGS/ESP/RF',after.subarray(0,56),expected.subarray(0,56)],
      ['exit/retirement',after.subarray(56,96),expected.subarray(56,96)],['x87',after.subarray(FP,FP+128),expected.subarray(FP,FP+128)],
      ['SIMD/YMM',Buffer.concat([after.subarray(SIMD,SIMD+160),after.subarray(YMM,YMM+144)]),Buffer.concat([expected.subarray(SIMD,SIMD+160),expected.subarray(YMM,YMM+144)])],
      ['complete arena',after,expected]])if(!actual.equals(wanted))failures.push(name);
    if(status!==0)failures.push(`run status ${status}`);if(trace.length!==(variant.direct?0:1))failures.push(`helper count ${trace.length}`);
    for(const [name,bytes]of [['before',before],['after',after],['expected',expected],['module',moduleBytes]])writeFileSync(join(output,`${id}-${name}.${name==='module'?'wasm':'bin'}`),bytes,{flag:'wx'});
    rows.push({id,label,owner,variant:variant.name,changeKey,classification:'executed',pass:failures.length===0,failures,trace,
      actual_cpu_hex:after.subarray(0,56).toString('hex'),expected_cpu_hex:expected.subarray(0,56).toString('hex'),
      actual_exit_hex:after.subarray(56,96).toString('hex'),expected_exit_hex:expected.subarray(56,96).toString('hex')});
  }catch(error){rows.push({id,label,owner,variant:variant.name,changeKey,classification:'setup-or-helper-contract-error',pass:false,error:String(error.stack??error),trace});}
}
assert.deepEqual(pins(),sourcePins,'source drift during proof');
const result={engine_sha256:engine.sha256,probe_sha256:hash(readFileSync(new URL(import.meta.url))),source_pins:sourcePins,
  expected_runs:24,executed:rows.filter(r=>r.classification==='executed').length,passes:rows.filter(r=>r.pass).length,
  failures:rows.filter(r=>!r.pass).length,rows,
  claim:'Old public real execution helpers for AVX Test/String Index and x87 Wait/NoOperation. Direct FNSTSWAX/FNCLEX controls have no helper or synthetic late key edit. Literal full CPU, RF, vectors, helper scratch, complete arena and retirement. Stateful String Mask and pending FWAIT preserve completion/fault priority. No synthetic helper responses or engine result before execution.'};
writeFileSync(join(output,'observed.json'),JSON.stringify(result,null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({executed:result.executed,passes:result.passes,failures:result.failures,engine_sha256:engine.sha256}));process.exitCode=result.failures?1:0;
