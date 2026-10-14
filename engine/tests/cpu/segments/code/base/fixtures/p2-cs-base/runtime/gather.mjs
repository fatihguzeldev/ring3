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
const PC=0x1000,SIZE=4796,TRANSFER=140,BASES=4756,SIMD=4364,YMM=4612,DATA=0x5000;
const original=[DATA,0x22220002,0x33330003,0x44440004,0x3000,0x55550005,0x66660006,0x77770007];
const hash=b=>createHash('sha256').update(b).digest('hex');
const paths=['engine/src/cpu/dbt/wasm/avx/gather.rs','engine/src/cpu/dbt/wasm/code_fetch.rs',
  'engine/src/process/cpu/avx.rs','engine/src/process/cpu/avx/gather.rs',
  'engine/src/cpu/x86/avx/gather/mod.rs','engine/src/cpu/x86/avx/gather/geometry.rs',
  'engine/src/cpu/x86/decode/avx/gather.rs','engine/src/abi/x86/avx/gather.rs',
  'engine/src/process/execution/authority.rs','engine/src/process/cpu/segments.rs',
  'engine/src/process/wasm/detection.rs','engine/src/process/cpu/interrupts/detection.rs',
  'engine/src/cpu/dbt/wasm/detection.rs','engine/src/abi/x86/avx/gather/paging.rs',
  'engine/src/process/wasm/paging.rs','engine/src/process/memory/paging/request.rs',
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
function ram(c,address=DATA){const saved=arena(c).subarray(100,140),b=Buffer.alloc(4096);
  for(let at=0;at<4096;at+=4){host(c,'read32',[address+at]);const p=arena(c).subarray(100,140);assert.equal(p.readUInt32LE(16),0);b.writeUInt32LE(p.readUInt32LE(20),at);}
  input(c,100,saved);return b;}
function cr2(c,value){const before=arena(c),status=c.api.read_cr2(),after=arena(c),expected=Buffer.from(before);
  if(value!==undefined)expected.set(words([value]),TRANSFER);
  assert.equal(status,value===undefined?7:0,'CR2 preserves Unknown or the real native fault address');
  assert.deepEqual(after,expected,'CR2 observation changes only its four-byte transfer result');
  input(c,TRANSFER,before.subarray(TRANSFER,TRANSFER+4));
  return{status,value:value===undefined?'Unknown':value};}
let context=0;
for(const owner of ['replacement-range','resident-entry'])for(const partial of [false,true])for(const changeKey of [false,true])
for(const corruptDiscarded of partial&&changeKey?[false,true]:[false]){
  const id=++context,label=`${owner}-gatherdd128-${partial?'partial-page-fault':'success'}-${changeKey?'late-key-change':'flat-control'}${corruptDiscarded?'-malformed-discarded-output':''}`,trace=[],detectors=[],failures=[];
  try{
    const instance=new WebAssembly.Instance(engine.module,{}),c={memory:instance.exports.memory,api:{}};
    for(const [name,value]of Object.entries(instance.exports))if(name.startsWith('ring3_abi_v1_')&&typeof value==='function')c.api[name.slice(13)]=value;
    host(c,'open',[3,id,0x4753]);c.base=c.api.arena_ptr()>>>0;host(c,'map',[PC,1,7]);host(c,'map',[DATA,1,3]);
    host(c,'set_segment_base',[2,0x5000]);
    // VPGATHERDD xmm0,[eax+xmm4],xmm2; all four dword lanes selected.
    const code=Buffer.from('c4e269900420ebfe','hex');upload(c,PC,code);
    const resident=owner==='resident-entry';input(c,TRANSFER,words(resident?[PC]:[PC,code.length]));
    host(c,resident?'compile_resident_entries':'compile',resident?[1,0]:[1]);
    const prepared=arena(c),pointer=resident?prepared.readUInt32LE(156):c.api.module_ptr()>>>0;
    const length=resident?prepared.readUInt32LE(160):c.api.module_len()>>>0;
    const moduleBytes=Buffer.from(new Uint8Array(c.memory.buffer,pointer,length)),module=new WebAssembly.Module(moduleBytes),ring3={...c.api};
    const imports=WebAssembly.Module.imports(module),helpers=imports.filter(i=>i.module==='ring3'&&i.name.startsWith('avx_execute'));
    assert.equal(helpers.length,1,'one real gather execution binding');
    const guard=resident?'guard_detect_resident':'guard_detect';
    assert.deepEqual(imports.map(i=>`${i.module}:${i.name}:${i.kind}`).sort(),
      ['env:memory:memory',`ring3:${guard}:function`,'ring3:finish_exception_detection:function',`ring3:${helpers[0].name}:function`].sort(),
      'exact gather authority and application event imports');
    let epoch;
    ring3[guard]=(...args)=>{epoch=c.api[guard](...args);assert.ok(typeof epoch==='bigint'&&epoch>0n);return epoch;};
    ring3.finish_exception_detection=(...args)=>{
      const status=c.api.finish_exception_detection(...args);
      detectors.push({args:args.map(v=>typeof v==='bigint'?v.toString():v>>>0),status});return status;
    };
    const destination=Buffer.from(Array.from({length:32},(_,i)=>0xc0+i));
    const mask=Buffer.concat([words([0x80000001,0x80000001,0x80000001,0x80000001]),Buffer.alloc(16,0x5a)]);
    const stagedDestination=Buffer.from(destination),stagedMask=Buffer.alloc(32);
    stagedDestination.writeUInt32LE(0x11111111,0);
    if(partial)stagedMask.fill(0xff,4,16);
    else{stagedDestination.set(words([0x22222222,0x33333333,0x44444444]),4);stagedDestination.fill(0,16);}
    const stagedOutput=Buffer.concat([stagedDestination,stagedMask]);
    const packet=record('R3GH',40,partial?2:1,partial?[9,4,0x6000,1,1,1]:[0,0,0,0,15,0xffffffff]);
    const discardedPacket=Buffer.from(packet),discardedOutput=corruptDiscarded?Buffer.alloc(64,0xa5):stagedOutput;
    if(corruptDiscarded)discardedPacket.write('BAD!');
    for(const imp of helpers)ring3[imp.name]=(...args)=>{
      assert.equal(args[args.length-6]>>>0,0x01001401,'selected gather descriptor');
      const before=arena(c),status=c.api[imp.name](...args),staged=arena(c),expected=Buffer.from(before);
      expected.set(packet,100);expected.set(stagedOutput,TRANSFER);
      assert.equal(status,partial?23:0,'real gather transport reports exact typed page-fault occurrence');
      assert.deepEqual(staged,expected,'gather stages packet/output without architectural vector publication');
      if(corruptDiscarded){input(c,100,discardedPacket);input(c,TRANSFER,discardedOutput);}
      if(changeKey)host(c,'set_segment_base',[1,0x4000]);
      const returnedStatus=corruptDiscarded?0:status;
      trace.push({name:imp.name,args:args.map(v=>v>>>0),status,returned_status:returnedStatus,
        changed_cs_base:changeKey?0x4000:0,corrupt_discarded:corruptDiscarded,staged_arena_sha256:hash(staged)});
      return returnedStatus;
    };
    const run=new WebAssembly.Instance(module,{env:{memory:c.memory},ring3}).exports.run;
    upload(c,DATA,words([0x11111111,0x22222222,0x33333333,0x44444444]));
    const simd=record('R3SV',160,1,[]);simd.writeUInt32LE(0x1f80,16);
    const ymm=record('R3YU',144,1,[]);
    simd.set(destination.subarray(0,16),32);ymm.set(destination.subarray(16),16);
    simd.set(mask.subarray(0,16),64);ymm.set(mask.subarray(16),48);
    simd.set(words([0,partial?0x1000:4,8,12]),96);input(c,SIMD,simd);input(c,YMM,ymm);
    input(c,0,record('R3ST',56,1,[...original,PC,2]));input(c,96,words([0]));
    const cr2Before=cr2(c,undefined);
    const before=arena(c),beforeRam=ram(c),expected=Buffer.from(before);
    expected.set(discardedPacket,100);expected.set(discardedOutput,TRANSFER);
    expected.set(record('R3GA',24,1,[3,DATA]),TRANSFER+544);
    if(changeKey)expected.writeUInt32LE(0x4000,BASES+20);
    else{expected.set(stagedDestination.subarray(0,16),SIMD+32);expected.set(stagedDestination.subarray(16),YMM+16);
      expected.set(stagedMask.subarray(0,16),SIMD+64);expected.set(stagedMask.subarray(16),YMM+48);}
    const retired=changeKey||partial?0:1,pc=retired?PC+6:PC;
    expected.set(record('R3ST',56,1,[...original,pc,2]),0);
    expected.set(record('R3EX',40,partial&&!changeKey?12:6,changeKey?[3,0,0,0,0,0]:partial?[23,0,4,0x6000,1,4]:[1,1,0,0,0,0]),56);
    const status=run(c.base,c.base+56,1,c.base+96),after=arena(c),afterRam=ram(c),cr2After=cr2(c,partial?0x6000:undefined);
    for(const [name,actual,wanted]of [['CPU/FLAGS/ESP',after.subarray(0,56),expected.subarray(0,56)],
      ['exit/retirement',after.subarray(56,96),expected.subarray(56,96)],
      ['architectural vectors',Buffer.concat([after.subarray(SIMD,SIMD+160),after.subarray(YMM,YMM+144)]),Buffer.concat([expected.subarray(SIMD,SIMD+160),expected.subarray(YMM,YMM+144)])],
      ['complete arena',after,expected],['complete mapped data RAM',afterRam,beforeRam]])if(!actual.equals(wanted))failures.push(name);
    if(status!==0)failures.push(`run status ${status}`);if(trace.length!==1)failures.push(`helper count ${trace.length}`);
    try{
      assert.equal(detectors.length,Number(partial||changeKey),'exact event or non-event finalization count');
      if(partial||changeKey){assert.equal(detectors[0].status,0,'stop finalization accepted');
        assert.deepEqual(detectors[0].args,changeKey?[epoch.toString(),3,0,0,0,0,0,0,0,0,0,0]:[epoch.toString(),23,4,0x6000,1,4,0,0,0,0,0,0],
          'exact epoch, terminal geometry and empty restart provenance');}
    }catch(error){failures.push(`stop finalization: ${error.message}`);}
    let progress;
    if(partial&&changeKey){
      host(c,'set_segment_base',[1,0]);host(c,'map',[0x6000,1,3]);upload(c,0x6000,words([0x22222222]));
      let discardStatus;
      if(resident){
        const beforeDiscard=arena(c);
        discardStatus=c.api.discard_unacknowledged_resident(id,0x4753,prepared.readUInt32LE(148),prepared.readUInt32LE(152));
        assert.equal(discardStatus,0,'returned resident can be discarded after unpublished fault cleanup');
        assert.deepEqual(arena(c),beforeDiscard,'resident disposal leaves the complete arena unchanged');
      }
      input(c,TRANSFER,words(resident?[PC]:[PC,code.length]));
      const beforeCompile=arena(c);
      const compileStatus=c.api[resident?'compile_resident_entries':'compile'](...(resident?[1,0]:[1]));
      progress={discard_status:discardStatus,compile_status:compileStatus};
      if(compileStatus!==0)failures.push(`fresh compilation status ${compileStatus}`);
      else{
        const preparedAgain=arena(c),ptr=resident?preparedAgain.readUInt32LE(156):c.api.module_ptr()>>>0;
        const len=resident?preparedAgain.readUInt32LE(160):c.api.module_len()>>>0;
        const published=Buffer.from(beforeCompile);if(resident)published.set(preparedAgain.subarray(140,164),140);
        assert.deepEqual(preparedAgain,published,'fresh compilation changes only its declared module receipt');
        const successorBytes=Buffer.from(new Uint8Array(c.memory.buffer,ptr,len)),successor=new WebAssembly.Module(successorBytes);
        assert.deepEqual(WebAssembly.Module.imports(successor).map(i=>`${i.module}:${i.name}:${i.kind}`).sort(),
          imports.map(i=>`${i.module}:${i.name}:${i.kind}`).sort(),'successor retains exact instruction authorities');
        const beforeProgress=arena(c),ramProgress=[ram(c),ram(c,0x6000)],wantProgress=Buffer.from(beforeProgress);
        const completed=Buffer.concat([words([0x11111111,0x22222222,0x33333333,0x44444444]),Buffer.alloc(16)]);
        wantProgress.set(record('R3GH',40,1,[0,0,0,0,15,0xffffffff]),100);
        wantProgress.set(Buffer.concat([completed,Buffer.alloc(32)]),TRANSFER);
        wantProgress.set(completed.subarray(0,16),SIMD+32);wantProgress.set(completed.subarray(16),YMM+16);
        wantProgress.fill(0,SIMD+64,SIMD+80);wantProgress.fill(0,YMM+48,YMM+64);
        wantProgress.set(record('R3ST',56,1,[...original,PC+6,2]),0);
        wantProgress.set(record('R3EX',40,6,[1,1,0,0,0,0]),56);
        const successorImports={...c.api},successorCalls=[];
        successorImports[helpers[0].name]=(...args)=>{const status=c.api[helpers[0].name](...args);successorCalls.push(status);return status;};
        const successorRun=new WebAssembly.Instance(successor,{env:{memory:c.memory},ring3:successorImports}).exports.run;
        assert.equal(successorRun(c.base,c.base+56,1,c.base+96),0,'fresh successor enters after unpublished fault cleanup');
        const afterProgress=arena(c);assert.deepEqual(successorCalls,[0],'one successful real gather helper');
        assert.deepEqual(afterProgress,wantProgress,'fresh successor complete architectural publication');
        assert.deepEqual(ram(c),ramProgress[0],'original mapped data remains unchanged');
        assert.deepEqual(ram(c,0x6000),ramProgress[1],'repaired mapped data remains unchanged');
        const cr2AfterRetry=cr2(c,0x6000);
        for(const [name,bytes]of [['progress-before',beforeProgress],['progress-after',afterProgress],['progress-expected',wantProgress],['progress-module',successorBytes]])
          writeFileSync(join(output,`${id}-${name}.${name==='progress-module'?'wasm':'bin'}`),bytes,{flag:'wx'});
        progress={...progress,retired:afterProgress.readUInt32LE(76),eip:afterProgress.readUInt32LE(48),cr2:cr2AfterRetry};
      }
    }
    for(const [name,bytes]of [['before',before],['after',after],['expected',expected],['module',moduleBytes]])writeFileSync(join(output,`${id}-${name}.${name==='module'?'wasm':'bin'}`),bytes,{flag:'wx'});
    rows.push({id,label,owner,partial,changeKey,corruptDiscarded,classification:'executed',pass:failures.length===0,failures,trace,detectors,cr2_before:cr2Before,cr2_after:cr2After,progress,
      actual_exit_hex:after.subarray(56,96).toString('hex'),expected_exit_hex:expected.subarray(56,96).toString('hex'),
      actual_destination_hex:Buffer.concat([after.subarray(SIMD+32,SIMD+48),after.subarray(YMM+16,YMM+32)]).toString('hex'),
      expected_destination_hex:Buffer.concat([expected.subarray(SIMD+32,SIMD+48),expected.subarray(YMM+16,YMM+32)]).toString('hex')});
  }catch(error){rows.push({id,label,owner,partial,changeKey,corruptDiscarded,classification:'setup-or-helper-contract-error',pass:false,error:String(error.stack??error),trace,detectors});}
}
assert.deepEqual(pins(),sourcePins,'source drift during proof');
const result={engine_sha256:engine.sha256,probe_sha256:hash(readFileSync(new URL(import.meta.url))),source_pins:sourcePins,
  expected_runs:10,executed:rows.filter(r=>r.classification==='executed').length,passes:rows.filter(r=>r.pass).length,
  failures:rows.filter(r=>!r.pass).length,rows,
  claim:'Public real VPGATHERDD helper; replacement/resident, success/ascending typed partialPF, valid post-helper CSbase edit/flat, and malformed discarded output with a downgraded transport after a genuine fault. Literal staged packet and vector bytes, exact event epoch/geometry, complete architectural vectors, CPU, retirement, arena and mapped dataRAM; CR2 preserves the real native observation and repaired successors complete with the genuine helper. No engine result before execution.'};
writeFileSync(join(output,'observed.json'),JSON.stringify(result,null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({executed:result.executed,passes:result.passes,failures:result.failures,engine_sha256:engine.sha256}));process.exitCode=result.failures?1:0;
