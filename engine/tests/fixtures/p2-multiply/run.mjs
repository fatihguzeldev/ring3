import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';

const [enginePath, outputDir, root] = process.argv.slice(2);
assert.ok(root, 'expected actual engine, output directory and repository root');
const fixture = join(root, 'engine/tests/fixtures/p2-multiply');
const hash = value => createHash('sha256').update(value).digest('hex');
const oracleBytes = readFileSync(join(fixture, 'oracle.json')), oracle = JSON.parse(oracleBytes);
const source = readFileSync(join(fixture, 'program.S'));
assert.equal(hash(oracleBytes), '759bd56364ad274e1538a362cf6ad450dd54a91b873dc7c338c3383561240dad');
assert.equal(hash(source), 'ec1325ea91335b3556431c1b3ef9340b33acae88b3520747287c4da0cb3d00bd');
const size = 4236, transfer = 140, bytes = hex => new Uint8Array(Buffer.from(hex, 'hex'));
const artifacts = {}, identities = [], observations = [];

function authoredText(object) {
  const view = new DataView(object.buffer, object.byteOffset, object.byteLength);
  assert.deepEqual([...object.subarray(0, 7)], [0x7f,0x45,0x4c,0x46,1,1,1]);
  assert.equal(view.getUint16(18,true),3);
  const offset=view.getUint32(32,true),stride=view.getUint16(46,true);
  const sections=Array.from({length:view.getUint16(48,true)},(_,index)=>{
    const at=offset+index*stride;
    return {name:view.getUint32(at,true),type:view.getUint32(at+4,true),offset:view.getUint32(at+16,true),
      size:view.getUint32(at+20,true),link:view.getUint32(at+24,true),info:view.getUint32(at+28,true),stride:view.getUint32(at+36,true)};
  });
  const names=sections[view.getUint16(50,true)];
  for(const section of sections) {
    const at=names.offset+section.name;
    section.name=object.subarray(at,object.indexOf(0,at)).toString('utf8');
  }
  const index=sections.findIndex(section=>section.name==='.text'),section=sections[index];
  assert.ok(index>0);
  assert.ok(!sections.some(section=>[4,9].includes(section.type)&&section.info===index&&section.size>0));
  const text=object.subarray(section.offset,section.offset+section.size),symbols={};
  const table=sections.find(section=>section.type===2),strings=sections[table.link];
  for(let at=table.offset;at<table.offset+table.size;at+=table.stride) {
    if(view.getUint16(at+14,true)!==index)continue;
    const nameAt=strings.offset+view.getUint32(at,true),name=object.subarray(nameAt,object.indexOf(0,nameAt)).toString('utf8');
    const offset=view.getUint32(at+4,true),size=view.getUint32(at+8,true);
    symbols[name]={offset,size,hex:text.subarray(offset,offset+size).toString('hex')};
  }
  assert.deepEqual(symbols,oracle.program.symbols);
  assert.equal(text.length,oracle.program.text_length);
  assert.equal(text.toString('hex'),oracle.program.text_hex);
  assert.equal(hash(text),oracle.program.text_sha256);
  return text;
}
function childLocals(module) {
  let at=8;
  const unsigned=()=>{
    let value=0,shift=0;
    for(let index=0;index<5;index++) {
      assert.ok(at<module.length);const byte=module[at++];value|=(byte&127)<<shift;
      if(!(byte&128))return value>>>0;
      shift+=7;
    }
    assert.fail('invalid bounded unsigned LEB');
  };
  while(at<module.length) {
    const id=module[at++],length=unsigned(),end=at+length;
    assert.ok(end<=module.length);
    if(id===10) {
      assert.equal(unsigned(),1);const bodyLength=unsigned(),bodyEnd=at+bodyLength;
      assert.equal(bodyEnd,end);const count=unsigned(),locals=[];
      for(let index=0;index<count;index++)locals.push([unsigned(),module[at++]]);
      assert.deepEqual(locals,[[16,0x7f],[1,0x7e]]);
      return locals;
    }
    at=end;
  }
  assert.fail('missing child code section');
}
const objectPath=join(outputDir,'program.o');
const assemble=['-target','i386-unknown-linux-gnu','-c',join(fixture,'program.S'),'-o',objectPath];
execFileSync('clang',assemble,{stdio:['ignore','pipe','pipe']});
writeFileSync(join(outputDir,'program.disassembly.txt'),execFileSync('xcrun',['llvm-objdump','-d','--x86-asm-syntax=intel',objectPath]));
const text=authoredText(readFileSync(objectPath));
writeFileSync(join(outputDir,'program.x86'),text);writeFileSync(join(outputDir,'program.S'),source);
writeFileSync(join(outputDir,'authored-oracle.json'),oracleBytes);writeFileSync(join(outputDir,'run.mjs'),readFileSync(import.meta.filename));
const production=execFileSync('rg',['--files','engine/src'],{cwd:root,encoding:'utf8'}).trim().split('\n').sort();
const paths=[...production,'Cargo.toml','engine/Cargo.toml','Cargo.lock','engine/tests/cpu_multiply_wasm.rs',
  'engine/tests/fixtures/p2-multiply/program.S','engine/tests/fixtures/p2-multiply/oracle.json','engine/tests/fixtures/p2-multiply/run.mjs'];
const sources=Object.fromEntries(paths.map(path=>[path,hash(readFileSync(join(root,path)))]));
writeFileSync(join(outputDir,'source-sha256.json'),JSON.stringify(sources,null,2));
const engineBytes=readFileSync(enginePath);writeFileSync(join(outputDir,'engine.wasm'),engineBytes);
for(const file of ['program.o','program.disassembly.txt','program.x86','program.S','authored-oracle.json','run.mjs','source-sha256.json','engine.wasm'])
  artifacts[file]=hash(readFileSync(join(outputDir,file)));
assert.ok(WebAssembly.validate(engineBytes));
const engineModule=new WebAssembly.Module(engineBytes);assert.deepEqual(WebAssembly.Module.imports(engineModule),[]);
assert.equal(WebAssembly.Module.exports(engineModule).filter(entry=>entry.kind==='function').length,58);
const names=['open','close','arena_ptr','map','protect','upload','generation','module_ptr','module_len',
  'compile_entries','compile_resident_entries','guard','guard_resident','find_resident'];
let engine,ordinal=0,moduleOrdinal=0;
function refresh() {
  if(engine.buffer!==engine.memory.buffer) {
    engine.buffer=engine.memory.buffer;engine.bytes=new Uint8Array(engine.buffer);engine.view=new DataView(engine.buffer);
  }
  return engine;
}
function arena() {return refresh().bytes.slice(engine.base,engine.base+size);}
function operation(label,action,status=0,patches=[]) {
  const expected=arena();for(const [offset,value] of patches)expected.set(value,offset);
  assert.equal(action(),status,engine.name+'/'+label+': status');
  assert.deepEqual(arena(),expected,engine.name+'/'+label+': whole4236');
}
function cancel(value) {refresh().view.setUint32(engine.base+96,value,true);}
function fresh(owner,label) {
  const instance=new WebAssembly.Instance(engineModule,{});
  const api=Object.fromEntries(names.map(name=>[name,instance.exports['ring3_abi_v1_'+name]]));
  for(const [name,fn] of Object.entries(api))assert.equal(typeof fn,'function',name);
  assert.equal(api.guard.length,6);assert.equal(api.guard_resident.length,7);
  assert.equal(api.compile_entries.length,2);assert.equal(api.compile_resident_entries.length,2);
  const key=0xa348cdef00000000n+BigInt(++ordinal);
  engine={owner,name:owner+'/'+label,api,key,low:Number(key&0xffffffffn),high:Number(key>>32n),memory:instance.exports.memory,started:false};
  assert.ok(engine.memory instanceof WebAssembly.Memory);assert.equal(api.open(1,engine.low,engine.high),0);
  engine.base=api.arena_ptr()>>>0;refresh();assert.ok(engine.base>0&&engine.base+size<=engine.bytes.length);
  engine.bytes.fill(0xa5,engine.base+100,engine.base+size);
  operation('map authored instruction page',()=>api.map(0x1000,1,7));
  refresh().bytes.set(text,engine.base+transfer);operation('upload authored746 bytes',()=>api.upload(0x1000,text.length));
  operation('protect RX',()=>api.protect(0x1000,1,5));
}
function compile(seeds,label) {
  refresh();seeds.forEach((pc,index)=>engine.view.setUint32(engine.base+transfer+index*4,pc,true));
  const before=arena();let unit;
  if(engine.owner==='replacement') {
    operation('entry compilation',()=>engine.api.compile_entries(seeds.length,0));
    const generation=engine.api.generation();assert.ok(generation>0);
    unit={generation,pointer:engine.api.module_ptr()>>>0,length:engine.api.module_len()>>>0};
  }else {
    assert.equal(engine.api.compile_resident_entries(seeds.length,0),0);
    const after=arena(),view=new DataView(after.buffer);
    const low=view.getUint32(transfer+8,true),high=view.getUint32(transfer+12,true),id=BigInt(low)|(BigInt(high)<<32n);
    assert.notEqual(id,0n);
    unit={low,high,id,pointer:view.getUint32(transfer+16,true),length:view.getUint32(transfer+20,true)};
    const receipt=new Uint8Array(24),metadata=new DataView(receipt.buffer);
    [1,24,low,high,unit.pointer,unit.length].forEach((value,index)=>metadata.setUint32(index*4,value,true));
    const expected=before.slice();expected.set(receipt,transfer);assert.deepEqual(after,expected,'resident only metadata24');
    const filename='receipt-'+ordinal+'.bin';writeFileSync(join(outputDir,filename),receipt);artifacts[filename]=hash(receipt);
    assert.equal(engine.api.generation(),0);assert.equal(engine.api.module_ptr(),0);assert.equal(engine.api.module_len(),0);
    for(const pc of seeds)operation('full returned unit owner lookup',()=>engine.api.find_resident(pc),0,[[transfer,receipt]]);
  }
  refresh();assert.ok(unit.pointer>0&&unit.length>8&&unit.pointer+unit.length<=engine.bytes.length);
  const raw=engine.bytes.slice(unit.pointer,unit.pointer+unit.length),module=new WebAssembly.Module(raw);
  const guard=engine.owner==='resident'?'guard_resident':'guard';
  assert.deepEqual(WebAssembly.Module.imports(module),[{module:'env',name:'memory',kind:'memory'},{module:'ring3',name:guard,kind:'function'}]);
  assert.deepEqual(WebAssembly.Module.exports(module),[{name:'run',kind:'function'}]);
  const locals=childLocals(raw);
  const instance=new WebAssembly.Instance(module,{env:{memory:engine.memory},ring3:{[guard]:engine.api[guard]}});
  unit.run=instance.exports.run;assert.equal(unit.run.length,4);
  const filename='unit-'+(++moduleOrdinal)+'.wasm';writeFileSync(join(outputDir,filename),raw);artifacts[filename]=hash(raw);
  identities.push({instance:ordinal,owner:engine.owner,label,key:engine.key.toString(),id:unit.id?.toString(),
    generation:unit.generation,pointer:unit.pointer,length:unit.length,entry_seeds:seeds,guard,locals,module_sha256:hash(raw),filename});
  return unit;
}
function seed(initial) {
  assert.equal(engine.started,false,'one initial CPU context per fresh case');
  refresh().bytes.set(bytes(initial.state_hex),engine.base);
  engine.bytes.set(bytes(oracle.program.initial_exit_hex),engine.base+56);
  engine.bytes.set(bytes(oracle.program.initial_helper_hex),engine.base+100);cancel(0);
}
function run(unit,budget,expected,label,status=0) {
  engine.started=true;
  const patches=expected?[[0,bytes(expected.state_hex)],[56,bytes(expected.exit_hex)]]:[];
  operation(label,()=>unit.run(engine.base,engine.base+56,budget,engine.base+96),status,patches);
  const output=arena(),row={name:engine.name,label,status,budget,state_hex:Buffer.from(output.subarray(0,56)).toString('hex'),
    exit_hex:Buffer.from(output.subarray(56,96)).toString('hex'),helper_hex:Buffer.from(output.subarray(100,140)).toString('hex'),
    whole_arena_hex:Buffer.from(output).toString('hex'),whole_arena_sha256:hash(output),untouched_bytes_exact:true};
  if(expected) {
    row.fresh_retired=new DataView(output.buffer).getUint32(76,true);
    assert.equal(row.fresh_retired,expected.fresh_retired);
    assert.equal(row.helper_hex,expected.helper_hex);
  }
  observations.push(row);
}
function close() {operation('close',()=>engine.api.close());}
for(const owner of oracle.owners) {
  for(const programme of oracle.programmes) {
    fresh(owner,programme.name);const unit=compile(programme.entry_seeds,programme.name);seed(programme.initial);
    run(unit,programme.budget,programme.complete,'complete authored'+programme.executed_instructions+' instructions');
    if(programme.name==='overflow-branch-DF0') {
      operation('negative permission invalidates unit snapshot',()=>engine.api.protect(0x1000,1,7));
      run(unit,4,null,'retained stale guard before any fresh publication',oracle.controls.retained_stale[owner+'_status']);
      close();run(unit,4,null,'retained closed guard',oracle.controls.retained_closed[owner+'_status']);
    }else close();
    fresh(owner,programme.name+'/split');const splitUnit=compile(programme.entry_seeds,programme.name+'/split');seed(programme.initial);
    let total=0;
    for(const expected of programme.split) {
      run(splitUnit,expected.budget,expected,expected.label+'/real continuation');total+=expected.fresh_retired;
      assert.equal(total,expected.cumulative_retired);
      cancel(1);run(splitUnit,4,expected.pauses.cancel,expected.label+'/cancel');cancel(0);
      run(splitUnit,0,expected.pauses.zero_budget,expected.label+'/zero budget');
    }
    assert.equal(total,programme.executed_instructions);close();
  }
  for(const vector of oracle.finite_vectors) {
    for(const variant of vector.variants) {
      fresh(owner,vector.name+'/DF'+variant.DF);const unit=compile(vector.entry_seeds,vector.name);seed(variant.initial);
      run(unit,vector.budget,variant.expected,'literal signedmultiply/full CPU/flags');close();
    }
  }
}
assert.equal(identities.length,112);assert.equal(observations.length,252);
const provenance={engine_sha256:hash(engineBytes),oracle_sha256:hash(oracleBytes),program_sha256:hash(source),text_sha256:hash(text),
  engine_exports:WebAssembly.Module.exports(engineModule),artifacts,sources,identities,observations,
  command:[process.execPath,process.argv[1],enginePath,outputDir,root],assembly_command:['clang',...assemble],
  tools:{node:process.version,v8:process.versions.v8,clang:execFileSync('clang',['--version'],{encoding:'utf8'}).split('\n')[0]},
  claim:'independent authored746-byte register-only signedIMUL32 corpus; replacement and resident direct guard-only children retain16i32+1i64 locals. Two distinct guest chains retire9/12 instructions; realIMUL CF/OF drives JO/JC/JNO past dead canaries, and source/self/ESP aliases distinguish two vs three operands/signextendedimm8/fullimm32. Twenty-four finite cases×DFboth prove all3canonical forms/all8destination GPRs, signedwide overflow boundaries, signed64 maximalproduct, source different from destination and forced69FFFFFFFF with nonzero source. Full4236 raw output and exact literal State/Exit/opaquehelper publication at all split/budget/cancel/NeedCode stops. One explicit initial context per fresh case; no resumed CPU/flags/EIP patch, JS arithmetic/interpreter/helper interception or guestRAM access. Deliberate negative permission change proves retained snapshot4 and terminalclose5 without fresh replacement-generation ambiguity. Actual fullu64 receipts/key and dynamicTransfer bytes preserved. PF/AF/ZF/SF0 even at zero/negative result is an explicit Ring3 undefinedbit policy, not portablehardware guarantee. No memorymultiply/divide/narrowISA/optimize/browser/performance/PE/provider/SDK/game/fullP2-V0 claim.'};
writeFileSync(join(outputDir,'provenance.json'),JSON.stringify(provenance,null,2));
console.log(JSON.stringify({status:'ok',engine_sha256:provenance.engine_sha256,oracle_sha256:provenance.oracle_sha256,
  owners:oracle.owners,programmes:4,finite_cases:24,finite_DF_variants:48,generated_children:identities.length,observations:observations.length,
  executed_counts:[9,12],output:outputDir}));
