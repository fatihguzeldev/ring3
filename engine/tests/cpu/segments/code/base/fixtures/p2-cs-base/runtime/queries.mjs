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
const PC=0x1000,SIZE=4796,TRANSFER=140,BASES=4756;
const original=[0x43,0x22220002,0x33330003,0x44440004,0x3000,0x55550005,0x66660006,0x77770007];
const hash=b=>createHash('sha256').update(b).digest('hex');
const paths=['engine/src/cpu/dbt/wasm/instructions/atomic.rs', 'engine/src/cpu/dbt/wasm/helpers/integer.rs','engine/src/cpu/dbt/wasm/instructions/segment_queries.rs',
  'engine/src/cpu/dbt/wasm/code_fetch.rs','engine/src/cpu/dbt/wasm/imports/mod.rs', 'engine/src/cpu/dbt/wasm/imports/binding.rs', 'engine/src/cpu/dbt/wasm/imports/context.rs', 'engine/src/cpu/dbt/wasm/imports/needs.rs', 'engine/src/cpu/dbt/wasm/helpers/protocol.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/mod.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/address.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/scalar.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/stack.rs',
  'engine/src/process/cpu/integer.rs','engine/src/process/cpu/segment_queries.rs',
  'engine/src/cpu/x86/segments/queries.rs','engine/src/process/execution/authority.rs',
  'engine/src/process/cpu/segments.rs','engine/src/abi/x86/segments.rs','engine/tests/support/wasm/engine.mjs'];
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
  {name:'verr',code:'0f00e0',selector:0x30001,value:0x43},
  {name:'verw',code:'0f00e8',selector:0x30002,value:0x43},
  {name:'lar-edx',code:'0f02d0',selector:0x30003,value:0x0040f200,destination:2},
  {name:'lsl-edx',code:'0f03d0',selector:0x30004,value:0xffff,destination:2},
];
let context=0;
for(const owner of ['replacement-range','resident-entry'])for(const variant of variants)for(const changeKey of [false,true]){
  const id=++context,label=`${owner}-${variant.name}-${changeKey?'late-key-change':'flat-control'}`,trace=[],failures=[];
  try{
    const instance=new WebAssembly.Instance(engine.module,{}),c={memory:instance.exports.memory,api:{}};
    for(const [name,value]of Object.entries(instance.exports))if(name.startsWith('ring3_abi_v1_')&&typeof value==='function')c.api[name.slice(13)]=value;
    host(c,'open',[2,id,0x5155]);c.base=c.api.arena_ptr()>>>0;host(c,'map',[PC,1,7]);
    host(c,'set_segment_base',[2,0x5000]);
    // writable CPL3 data, base0, byte-granular ffff limit, B32; A stays clear.
    host(c,'configure_segment_descriptor_raw',[0x43,0x0000ffff,0x0040f200]);
    const code=Buffer.from(variant.code+'ebfe','hex');upload(c,PC,code);
    const resident=owner==='resident-entry';input(c,TRANSFER,words(resident?[PC]:[PC,code.length]));
    host(c,resident?'compile_resident_entries':'compile',resident?[1,0]:[1]);
    const prepared=arena(c),pointer=resident?prepared.readUInt32LE(156):c.api.module_ptr()>>>0;
    const length=resident?prepared.readUInt32LE(160):c.api.module_len()>>>0;
    const moduleBytes=Buffer.from(new Uint8Array(c.memory.buffer,pointer,length)),module=new WebAssembly.Module(moduleBytes),ring3={...c.api};
    const imports=WebAssembly.Module.imports(module),integerImports=imports.filter(i=>i.module==='ring3'&&i.name.startsWith('cpu_integer'));
    assert.equal(integerImports.length,1,'one real integer helper binding');
    const payload=[variant.value,0x42,original[0],original[2],original[3],original[1]];
    for(const imp of integerImports)ring3[imp.name]=(...args)=>{
      assert.equal(args[args.length-2]>>>0,variant.selector,'selected query helper descriptor');
      assert.equal(args[args.length-1]>>>0,0,'register query has no memory address');
      const before=arena(c),status=c.api[imp.name](...args),staged=arena(c),expected=Buffer.from(before);
      expected.set(record('R3MH',40,7,[0,0,0,0,0,0]),100);expected.set(words(payload),TRANSFER);
      assert.equal(status,0,'real query helper success');
      assert.deepEqual(staged,expected,'real query helper stages output and packet only');
      if(changeKey)host(c,'set_segment_base',[1,0x4000]);
      trace.push({name:imp.name,args:args.map(v=>v>>>0),status,changed_cs_base:changeKey?0x4000:0,staged_arena_sha256:hash(staged)});
      return status;
    };
    const run=new WebAssembly.Instance(module,{env:{memory:c.memory},ring3}).exports.run;
    input(c,0,record('R3ST',56,1,[...original,PC,2]));input(c,96,words([0]));
    const before=arena(c),expected=Buffer.from(before),registers=[...original];
    if(!changeKey&&variant.destination!==undefined)registers[variant.destination]=variant.value;
    const pc=changeKey?PC:PC+variant.code.length/2,retired=changeKey?0:1,flags=changeKey?2:0x42;
    expected.set(record('R3ST',56,1,[...registers,pc,flags]),0);
    expected.set(record('R3EX',40,6,[changeKey?3:1,retired,0,0,0,0]),56);
    expected.set(record('R3MH',40,7,[0,0,0,0,0,0]),100);expected.set(words(payload),TRANSFER);
    if(changeKey)expected.writeUInt32LE(0x4000,BASES+20);
    const status=run(c.base,c.base+56,1,c.base+96),after=arena(c);
    for(const [name,actual,wanted]of [['CPU/FLAGS/ESP',after.subarray(0,56),expected.subarray(0,56)],
      ['exit/retirement',after.subarray(56,96),expected.subarray(56,96)],['complete arena',after,expected]])if(!actual.equals(wanted))failures.push(name);
    if(status!==0)failures.push(`run status ${status}`);if(trace.length!==1)failures.push(`helper count ${trace.length}`);
    for(const [name,bytes]of [['before',before],['after',after],['expected',expected],['module',moduleBytes]])writeFileSync(join(output,`${id}-${name}.${name==='module'?'wasm':'bin'}`),bytes,{flag:'wx'});
    rows.push({id,label,owner,variant:variant.name,changeKey,classification:'executed',pass:failures.length===0,failures,trace,
      actual_registers:Array.from({length:8},(_,i)=>after.readUInt32LE(16+4*i)),expected_registers:registers,
      actual_flags:after.readUInt32LE(52),expected_flags:flags,actual_pc:after.readUInt32LE(48),expected_pc:pc,
      actual_exit_hex:after.subarray(56,96).toString('hex'),expected_exit_hex:expected.subarray(56,96).toString('hex')});
  }catch(error){rows.push({id,label,owner,variant:variant.name,changeKey,classification:'setup-or-helper-contract-error',pass:false,error:String(error.stack??error),trace});}
}
assert.deepEqual(pins(),sourcePins,'source drift during proof');
const result={engine_sha256:engine.sha256,probe_sha256:hash(readFileSync(new URL(import.meta.url))),source_pins:sourcePins,
  expected_runs:16,executed:rows.filter(r=>r.classification==='executed').length,passes:rows.filter(r=>r.pass).length,
  failures:rows.filter(r=>!r.pass).length,rows,
  claim:'Old public engine exports and generated Wasm register VERR/VERW/LAR/LSL only; replacement and resident owners. Real helper staging is literal; full CPU/FLAGS/ESP, retirement, helper/transfer and complete arena are literal. Descriptor A-bit/cache mutation is not authorized by the query. No engine result exists until executed.'};
writeFileSync(join(output,'observed.json'),JSON.stringify(result,null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({executed:result.executed,passes:result.passes,failures:result.failures,engine_sha256:engine.sha256}));process.exitCode=result.failures?1:0;
