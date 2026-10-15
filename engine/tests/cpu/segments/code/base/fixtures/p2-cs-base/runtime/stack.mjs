import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync, mkdirSync} from 'node:fs';
import {join} from 'node:path';
import {pathToFileURL} from 'node:url';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root, 'engine path, fresh output directory and repo root required');
assert.match(process.env.RING3_ENGINE_SHA256 ?? '', /^[a-f0-9]{64}$/, 'pin the selected engine');
const {readEngine} = await import(pathToFileURL(join(root, 'engine/tests/support/wasm/engine.mjs')));
const engine = readEngine(enginePath);
mkdirSync(output, {recursive:true});
const SIZE=4796, PC=0x1000, TRANSFER=140, BASES=4756, STACK=0x8000;
const hash = value => createHash('sha256').update(value).digest('hex');
const original=[0x11110001,0x22220002,0x33330003,0x44440004,0x3000,0x55550005,0x66660006,0x77770007];
const slots=[0x1011,0x2022,0x3033,0xdead,0x4044,0x5055,0x6066,0x7077];
const sourcePaths=[
  'engine/src/cpu/dbt/wasm/instructions/stack.rs','engine/src/cpu/dbt/wasm/code_fetch.rs',
  'engine/src/cpu/dbt/wasm/imports/mod.rs', 'engine/src/cpu/dbt/wasm/imports/binding.rs', 'engine/src/cpu/dbt/wasm/imports/context.rs', 'engine/src/cpu/dbt/wasm/imports/needs.rs', 'engine/src/cpu/dbt/wasm/helpers/protocol.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/mod.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/address.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/scalar.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/stack.rs','engine/src/cpu/dbt/wasm/emitter/mod.rs', 'engine/src/cpu/dbt/wasm/emitter/module.rs', 'engine/src/cpu/dbt/wasm/emitter/body.rs', 'engine/src/cpu/dbt/wasm/requirements.rs', 'engine/src/cpu/dbt/wasm/imports/wiring.rs',
  'engine/src/process/cpu/stack.rs','engine/src/process/memory/span.rs',
  'engine/src/process/execution/authority.rs','engine/src/process/cpu/segments.rs',
  'engine/src/memory/space.rs', 'engine/src/memory/space/atomic.rs','engine/src/abi/x86/segments.rs',
  'engine/tests/support/wasm/engine.mjs',
];
const pins=()=>Object.fromEntries(sourcePaths.map(path=>[path,hash(readFileSync(join(root,path)))]));
const sourcePins=pins(), rows=[];
const words=values=>{const b=Buffer.alloc(values.length*4);values.forEach((v,i)=>b.writeUInt32LE(v>>>0,i*4));return b;};
function record(magic,size,version,values) {
  const b=Buffer.alloc(size);b.write(magic);b.writeUInt16LE(version,4);b.writeUInt16LE(1,6);
  b.writeUInt32LE(size,8);b.set(words(values),16);return b;
}
const arena=c=>Buffer.from(new Uint8Array(c.memory.buffer,c.base,SIZE));
const input=(c,offset,bytes)=>new Uint8Array(c.memory.buffer).set(bytes,c.base+offset);
function host(c,name,args=[]) {assert.equal(c.api[name](...args),0,name);}
function upload(c,address,bytes) {input(c,TRANSFER,bytes);host(c,'upload',[address,bytes.length]);}
function ram(c) {
  const saved=arena(c).subarray(100,140), bytes=Buffer.alloc(4096);
  for(let offset=0;offset<4096;offset+=4) {
    host(c,'read32',[STACK+offset]);const packet=arena(c).subarray(100,140);
    assert.equal(packet.readUInt32LE(16),0,'RAM observer result');
    bytes.writeUInt32LE(packet.readUInt32LE(20),offset);
  }
  input(c,100,saved);return bytes;
}
let context=0;
for(const owner of ['replacement-range','resident-entry']) for(const word of [false,true]) for(const changeKey of [false,true]) {
  const label=`${owner}-${word?'popa16':'popad32'}-${changeKey?'late-key-change':'flat-control'}`;
  const id=++context, trace=[], failures=[];
  try {
    const instance=new WebAssembly.Instance(engine.module,{});
    const c={memory:instance.exports.memory,api:{},trace};
    for(const [name,value] of Object.entries(instance.exports))
      if(name.startsWith('ring3_abi_v1_')&&typeof value==='function') c.api[name.slice(13)]=value;
    host(c,'open',[3,id,0x504f]);c.base=c.api.arena_ptr()>>>0;
    for(const [page,permissions] of [[PC,7],[STACK,3]])host(c,'map',[page,1,permissions]);
    // force current v3 before compiling; the later CS change alters only companion CS base.
    host(c,'set_segment_base',[2,0x5000]);
    const code=Buffer.from(word?'6661ebfe':'61ebfe','hex');upload(c,PC,code);
    const resident=owner==='resident-entry';
    input(c,TRANSFER,words(resident?[PC]:[PC,code.length]));
    host(c,resident?'compile_resident_entries':'compile',resident?[1,0]:[1]);
    const prepared=arena(c), pointer=resident?prepared.readUInt32LE(156):c.api.module_ptr()>>>0;
    const length=resident?prepared.readUInt32LE(160):c.api.module_len()>>>0;
    const moduleBytes=Buffer.from(new Uint8Array(c.memory.buffer,pointer,length));
    const module=new WebAssembly.Module(moduleBytes), ring3={...c.api};
    const imports=WebAssembly.Module.imports(module);
    const stackImports=imports.filter(i=>i.module==='ring3'&&i.name.startsWith('stack_execute'));
    assert.equal(stackImports.length,1,'one real stack helper binding');
    for(const imp of stackImports)ring3[imp.name]=(...args)=>{
      const before=arena(c), status=c.api[imp.name](...args), staged=arena(c);
      const expected=Buffer.from(before);
      expected.set(record('R3MH',40,7,[0,0,0,0,0,0]),100);
      expected.set(words(popped),TRANSFER);
      assert.deepEqual(staged,expected,'real helper stages transfer and packet only');
      assert.equal(status,0,'real helper success');
      if(changeKey)host(c,'set_segment_base',[1,0x4000]);
      trace.push({name:imp.name,args:args.map(v=>v>>>0),status,changed_cs_base:changeKey?0x4000:0,
        staged_arena_sha256:hash(staged)});
      return status;
    };
    const run=new WebAssembly.Instance(module,{env:{memory:c.memory},ring3}).exports.run;
    const width=word?2:4, seed=Buffer.alloc(8*width);
    slots.forEach((v,i)=>word?seed.writeUInt16LE(v,i*width):seed.writeUInt32LE(0xa0000000+v,i*width));
    upload(c,STACK,seed);
    const raw=word?slots:slots.map(v=>0xa0000000+v);
    const popped=[raw[7],raw[6],raw[5],raw[4],0x3000+8*width,raw[2],raw[1],raw[0]]
      .map((v,i)=>word&&i!==4?((original[i]&0xffff0000)|v)>>>0:v>>>0);
    input(c,0,record('R3ST',56,1,[...original,PC,2]));input(c,96,words([0]));
    const before=arena(c), beforeRam=ram(c), expected=Buffer.from(before);
    const regs=changeKey?original:popped, pc=changeKey?PC:PC+(word?2:1), retired=changeKey?0:1;
    expected.set(record('R3ST',56,1,[...regs,pc,2]),0);
    expected.set(record('R3EX',40,6,[changeKey?3:1,retired,0,0,0,0]),56);
    expected.set(record('R3MH',40,7,[0,0,0,0,0,0]),100);
    expected.set(words(popped),TRANSFER);
    if(changeKey)expected.writeUInt32LE(0x4000,BASES+20);
    const status=run(c.base,c.base+56,1,c.base+96), after=arena(c), afterRam=ram(c);
    for(const [name,actual,wanted] of [
      ['logical CPU',after.subarray(0,56),expected.subarray(0,56)],
      ['exit/retirement',after.subarray(56,96),expected.subarray(56,96)],
      ['complete arena',after,expected],['complete stack RAM',afterRam,beforeRam],
    ])if(!actual.equals(wanted))failures.push(name);
    if(status!==0)failures.push(`run status ${status}`);
    if(trace.length!==1)failures.push(`helper count ${trace.length}`);
    for(const [name,bytes] of [['before',before],['after',after],['expected',expected],['module',moduleBytes]])
      writeFileSync(join(output,`${id}-${name}.${name==='module'?'wasm':'bin'}`),bytes,{flag:'wx'});
    rows.push({id,label,owner,word,changeKey,classification:'executed',pass:failures.length===0,failures,
      actual_registers:Array.from({length:8},(_,i)=>after.readUInt32LE(16+i*4)),expected_registers:regs,
      actual_pc:after.readUInt32LE(48),expected_pc:pc,actual_exit_hex:after.subarray(56,96).toString('hex'),
      expected_exit_hex:expected.subarray(56,96).toString('hex'),trace,ram_sha256:hash(afterRam)});
  }catch(error){rows.push({id,label,owner,word,changeKey,classification:'setup-or-helper-contract-error',pass:false,error:String(error.stack??error),trace});}
}
assert.deepEqual(pins(),sourcePins,'source drift during proof');
const result={engine_sha256:engine.sha256,probe_sha256:hash(readFileSync(new URL(import.meta.url))),source_pins:sourcePins,
  expected_runs:8,executed:rows.filter(r=>r.classification==='executed').length,
  passes:rows.filter(r=>r.pass).length,failures:rows.filter(r=>!r.pass).length,rows,
  claim:'Old public engine exports and generated Wasm POPA/POPAD only. Current B32 with nonflat SS, replacement and resident owners. Real stage-only helper packet and transfer are checked literally; full CPU, retirement, arena and stack RAM checked literally. Valid CS-base edit occurs only after real helper completion. No engine or hardware result exists until executed.'};
writeFileSync(join(output,'observed.json'),JSON.stringify(result,null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({executed:result.executed,passes:result.passes,failures:result.failures,engine_sha256:engine.sha256}));
process.exitCode=result.failures?1:0;
