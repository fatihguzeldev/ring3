import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';

const [enginePath, outputDir, root] = process.argv.slice(2);
assert.ok(root, 'expected actual engine, output directory and repository root');
const fixture = join(root, 'engine/tests/fixtures/p2-carry');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const oracleBytes = readFileSync(join(fixture, 'oracle.json')), oracle = JSON.parse(oracleBytes);
const source = readFileSync(join(fixture, 'program.S'));
assert.equal(hash(oracleBytes), '3e2caca609a1ba8f76f30d0f4e72b553d2a5876cfa93303e2894cf6510671b8b');
assert.equal(hash(source), 'bc552951f3f242482b90e9267c93e1e2e19028c0ed7494ae816b1c71f2e9d846');
const artifacts = {}, observations = [], identities = [];
const size = 4236, transfer = 140, bytes = hex => new Uint8Array(Buffer.from(hex, 'hex'));

function authoredText(object) {
  const v = new DataView(object.buffer, object.byteOffset, object.byteLength);
  assert.deepEqual([...object.subarray(0, 7)], [0x7f, 0x45, 0x4c, 0x46, 1, 1, 1]);
  assert.equal(v.getUint16(18, true), 3);
  const offset = v.getUint32(32, true), stride = v.getUint16(46, true);
  const sections = Array.from({length:v.getUint16(48, true)}, (_, index) => {
    const at = offset + index * stride;
    return {name:v.getUint32(at,true), type:v.getUint32(at+4,true), offset:v.getUint32(at+16,true), size:v.getUint32(at+20,true), link:v.getUint32(at+24,true), info:v.getUint32(at+28,true), stride:v.getUint32(at+36,true)};
  });
  const names = sections[v.getUint16(50, true)];
  for (const section of sections) {
    const at = names.offset + section.name;
    section.name = object.subarray(at, object.indexOf(0, at)).toString('utf8');
  }
  const index = sections.findIndex(section => section.name === '.text'), section = sections[index];
  assert.ok(index > 0);
  assert.ok(!sections.some(section => [4,9].includes(section.type) && section.info === index && section.size > 0));
  const text = object.subarray(section.offset, section.offset + section.size), symbols = {};
  const table = sections.find(section => section.type === 2), strings = sections[table.link];
  for (let at = table.offset; at < table.offset + table.size; at += table.stride) {
    if (v.getUint16(at+14,true) !== index) continue;
    const nameAt = strings.offset + v.getUint32(at,true), name = object.subarray(nameAt,object.indexOf(0,nameAt)).toString('utf8');
    const offset = v.getUint32(at+4,true), size = v.getUint32(at+8,true);
    symbols[name] = {offset,size,hex:text.subarray(offset,offset+size).toString('hex')};
  }
  assert.deepEqual(symbols, oracle.program.symbols);
  assert.equal(text.length, oracle.program.text_length);
  assert.equal(text.toString('hex'), oracle.program.text_hex);
  assert.equal(hash(text), oracle.program.text_sha256);
  return text;
}
const objectPath = join(outputDir, 'program.o');
const assemble = ['-target','i386-unknown-linux-gnu','-c',join(fixture,'program.S'),'-o',objectPath];
execFileSync('clang',assemble,{stdio:['ignore','pipe','pipe']});
writeFileSync(join(outputDir,'program.disassembly.txt'),execFileSync('xcrun',['llvm-objdump','-d','--x86-asm-syntax=intel',objectPath]));
const text = authoredText(readFileSync(objectPath));
for (const file of ['program.o','program.disassembly.txt']) artifacts[file] = hash(readFileSync(join(outputDir,file)));
writeFileSync(join(outputDir,'program.x86'),text); artifacts['program.x86'] = hash(text);
writeFileSync(join(outputDir,'program.S'),source); writeFileSync(join(outputDir,'authored-oracle.json'),oracleBytes);
writeFileSync(join(outputDir,'run.mjs'),readFileSync(import.meta.filename));
const production = execFileSync('rg',['--files','engine/src'],{cwd:root,encoding:'utf8'}).trim().split('\n').sort();
const sourcePaths = [...production,'Cargo.toml','engine/Cargo.toml','Cargo.lock','engine/tests/cpu_carry_wasm.rs',
  'engine/tests/fixtures/p2-carry/program.S','engine/tests/fixtures/p2-carry/oracle.json','engine/tests/fixtures/p2-carry/run.mjs'];
const sources = Object.fromEntries(sourcePaths.map(file => [file,hash(readFileSync(join(root,file)))]));
writeFileSync(join(outputDir,'source-sha256.json'),JSON.stringify(sources,null,2));
const engineBytes = readFileSync(enginePath); writeFileSync(join(outputDir,'engine.wasm'),engineBytes);
assert.ok(WebAssembly.validate(engineBytes));
const engineModule = new WebAssembly.Module(engineBytes); assert.deepEqual(WebAssembly.Module.imports(engineModule),[]);
const names = ['open','close','arena_ptr','map','protect','upload','generation','module_ptr','module_len','compile_entries','compile_resident_entries',
  'guard','guard_resident','read32','write32','store32','store_resident32','find_resident'];
let engine, ordinal = 0, moduleOrdinal = 0;
function refresh() {
  if (engine.buffer !== engine.memory.buffer) { engine.buffer=engine.memory.buffer; engine.bytes=new Uint8Array(engine.buffer); engine.view=new DataView(engine.buffer); }
  return engine;
}
function arena() { return refresh().bytes.slice(engine.base,engine.base+size); }
function operation(label,action,status=0,patches=[]) {
  const expected=arena();for(const [offset,value] of patches) expected.set(value,offset);
  assert.equal(action(),status,engine.name+'/'+label+': status');assert.deepEqual(arena(),expected,engine.name+'/'+label+': whole4236');
}
function record(magic,version,fields,length) {
  const out=new Uint8Array(length),v=new DataView(out.buffer);out.set(Buffer.from(magic));v.setUint16(4,version,true);v.setUint16(6,1,true);v.setUint32(8,length,true);
  fields.forEach((value,index)=>v.setUint32(16+index*4,value,true));return out;
}
function helper(value=0) { return record('R3MH',1,[0,value,0,0,0,0],40); }
function cancel(value) { refresh().view.setUint32(engine.base+96,value,true); }
function map(address,permission) { operation('guest mapping',()=>engine.api.map(address,1,permission)); }
function protect(address,permission) { operation('guest permission repair',()=>engine.api.protect(address,1,permission)); }
function upload(address,value) {
  refresh().bytes.set(value,engine.base+transfer);operation('authored code upload',()=>engine.api.upload(address,value.length));
}
function word(address,value) {
  assert.equal(engine.started,false,'all host word initialization precedes first guest execution');
  operation('explicit initial RAM word',()=>engine.api.write32(address,value),0,[[100,helper()]]);
}
function readWords(expected,label) {
  const observed={};
  for(const [key,value] of Object.entries(expected)) {
    const address=Number.parseInt(key,16);operation(label,()=>engine.api.read32(address),0,[[100,helper(value)]]);
    observed[key]=refresh().view.getUint32(engine.base+120,true);
  }
  return observed;
}
function fresh(owner,label,{data=true,cross=false,codeWritable=false}={}) {
  const instance=new WebAssembly.Instance(engineModule,{});
  const api=Object.fromEntries(names.map(name=>[name,instance.exports['ring3_abi_v1_'+name]]));
  for(const [name,fn] of Object.entries(api))assert.equal(typeof fn,'function',name);
  assert.equal(api.guard.length,6);assert.equal(api.guard_resident.length,7);assert.equal(api.read32.length,1);
  assert.equal(api.store32.length,2);assert.equal(api.store_resident32.length,6);
  const key=0xa346cdef00000000n+BigInt(++ordinal);
  engine={owner,name:owner+'/'+label,api,key,low:Number(key&0xffffffffn),high:Number(key>>32n),memory:instance.exports.memory,started:false,units:[]};
  assert.ok(engine.memory instanceof WebAssembly.Memory);assert.equal(api.open(8,engine.low,engine.high),0);
  engine.base=api.arena_ptr()>>>0;refresh();assert.ok(engine.base>0&&engine.base+size<=engine.bytes.length);
  engine.bytes.fill(0xa5,engine.base+100,engine.base+size);
  map(0x1000,7);upload(0x1000,text);if(!codeWritable)protect(0x1000,5);
  if(data)map(0x5000,3);if(cross)map(0x6000,3);
  return engine;
}
function needs(symbol,memory=true) {
  if(!memory)return {read:false,store:false};
  const store=symbol==='memory_chain'||symbol.includes('_source_base')||symbol.includes('_ebp_memory')||symbol.includes('_store_fault')||symbol.includes('_smc_');
  return {read:true,store};
}
function compile(seeds,access,label) {
  refresh();seeds.forEach((pc,index)=>engine.view.setUint32(engine.base+transfer+index*4,pc,true));
  const before=arena();let unit;
  if(engine.owner==='replacement') {
    operation('entry compiler',()=>engine.api.compile_entries(seeds.length,0));const generation=engine.api.generation();
    assert.ok(generation>0);unit={generation,pointer:engine.api.module_ptr()>>>0,length:engine.api.module_len()>>>0};
  }else {
    assert.equal(engine.api.compile_resident_entries(seeds.length,0),0);const after=arena(),v=new DataView(after.buffer);
    const low=v.getUint32(transfer+8,true),high=v.getUint32(transfer+12,true),id=BigInt(low)|(BigInt(high)<<32n);
    assert.notEqual(id,0n);assert.ok(!engine.units.some(previous=>previous.id===id));
    unit={low,high,id,pointer:v.getUint32(transfer+16,true),length:v.getUint32(transfer+20,true)};
    const metadata=new Uint8Array(24),mv=new DataView(metadata.buffer);
    [1,24,low,high,unit.pointer,unit.length].forEach((value,index)=>mv.setUint32(index*4,value,true));
    const wanted=before.slice();wanted.set(metadata,transfer);assert.deepEqual(after,wanted,'resident compiler only metadata24');
    const filename='receipt-'+ordinal+'-'+(moduleOrdinal+1)+'.bin';writeFileSync(join(outputDir,filename),metadata);artifacts[filename]=hash(metadata);
    assert.equal(engine.api.generation(),0);assert.equal(engine.api.module_ptr(),0);assert.equal(engine.api.module_len(),0);
    for(const pc of seeds)operation('full returned resident owner selection',()=>engine.api.find_resident(pc),0,[[transfer,metadata]]);
  }
  refresh();assert.ok(unit.pointer>0&&unit.length>8&&unit.pointer+unit.length<=engine.bytes.length);
  unit.bytes=engine.bytes.slice(unit.pointer,unit.pointer+unit.length);const module=new WebAssembly.Module(unit.bytes);
  const guard=engine.owner==='resident'?'guard_resident':'guard';
  const helpers=[guard,...(access.read?['read32']:[]),...(access.store?[engine.owner==='resident'?'store_resident32':'store32']:[])];
  assert.deepEqual(WebAssembly.Module.imports(module),[{module:'env',name:'memory',kind:'memory'},...helpers.map(name=>({module:'ring3',name,kind:'function'}))]);
  assert.deepEqual(WebAssembly.Module.exports(module),[{name:'run',kind:'function'}]);
  const instance=new WebAssembly.Instance(module,{env:{memory:engine.memory},ring3:Object.fromEntries(helpers.map(name=>[name,engine.api[name]]))});
  unit.run=instance.exports.run;assert.equal(unit.run.length,4);engine.units.push(unit);
  const filename='unit-'+ordinal+'-'+(++moduleOrdinal)+'.wasm';writeFileSync(join(outputDir,filename),unit.bytes);artifacts[filename]=hash(unit.bytes);
  identities.push({instance:ordinal,owner:engine.owner,label,key:engine.key.toString(),id:unit.id?.toString(),generation:unit.generation,pointer:unit.pointer,length:unit.length,
    entry_seeds:seeds,imports:helpers,module_sha256:hash(unit.bytes),filename});
  return unit;
}
function seed(initial) {
  assert.equal(engine.started,false,'one initial host context per fresh instance');
  refresh().bytes.set(bytes(initial.state_hex),engine.base);engine.bytes.set(bytes(oracle.program.initial_exit_hex),engine.base+56);
  engine.bytes.set(bytes(oracle.program.initial_helper_hex),engine.base+100);cancel(0);
}
function run(unit,budget,expected,label,status=0) {
  engine.started=true;
  const patches=expected?[[0,bytes(expected.state_hex)],[56,bytes(expected.exit_hex)]]:[];
  if(expected&&expected.helper_hex!=='preserve')patches.push([100,bytes(expected.helper_hex)]);
  const before=arena();operation(label,()=>unit.run(engine.base,engine.base+56,budget,engine.base+96),status,patches);
  const output=arena();const row={name:engine.name,label,status,budget,state_hex:Buffer.from(output.subarray(0,56)).toString('hex'),
    exit_hex:Buffer.from(output.subarray(56,96)).toString('hex'),helper_hex:Buffer.from(output.subarray(100,140)).toString('hex'),
    whole_arena_sha256:hash(output),untouched_bytes_exact:true};
  if(expected){row.fresh_retired=new DataView(output.buffer).getUint32(76,true);assert.equal(row.fresh_retired,expected.fresh_retired);row.ram_words=readWords(expected.ram_words??{},'actual bounded RAM observation');}
  else assert.deepEqual(output,before,'rejected call has no publication');
  observations.push(row);return row;
}
function close(unit) {
  operation('close',()=>engine.api.close());
  operation('retained child after close',()=>unit.run(engine.base,engine.base+56,4,engine.base+96),5);
}
for(const owner of oracle.owners) {
  for(const programme of oracle.programmes) {
    fresh(owner,programme.name);for(const [key,value] of Object.entries(programme.initial_ram_words))word(Number.parseInt(key,16),value);
    const unit=compile(programme.entry_seeds,needs(programme.symbol,programme.memory),programme.name);seed(programme.initial);
    run(unit,programme.budget,programme.complete,'complete authored chain');
    if(!programme.memory)readWords(programme.initial_ram_words,'register chain leaves RAM exact');
    close(unit);
    if(!programme.split)continue;
    fresh(owner,programme.name+'/split');word(0x5010,0);
    const splitUnit=compile(programme.entry_seeds,needs(programme.symbol,programme.memory),programme.name+'/split');seed(programme.initial);
    for(const [index,expected] of programme.split.entries()) {
      run(splitUnit,expected.budget,expected,'real CF continuation'+index);
      const pause={...expected,fresh_retired:0,exit_hex:Buffer.from(record('R3EX',1,[2,0,0,0,0,0],40)).toString('hex'),helper_hex:'preserve'};
      cancel(1);run(splitUnit,4,pause,'cancel before next arithmetic');cancel(0);
      pause.exit_hex=Buffer.from(record('R3EX',1,[1,0,0,0,0,0],40)).toString('hex');run(splitUnit,0,pause,'zero budget before next arithmetic');
    }
    readWords(programme.initial_ram_words,'split chain leaves RAM exact');close(splitUnit);
  }
  for(const vector of oracle.form_and_alias_vectors) {
    fresh(owner,vector.name);for(const [key,value] of Object.entries(vector.initial_ram_words))word(Number.parseInt(key,16),value);
    const unit=compile(vector.entry_seeds,needs(vector.symbol,vector.memory),vector.name);seed(vector.initial);
    run(unit,vector.budget,vector.expected,'literal encoding or original-EA alias');close(unit);
  }
  for(const control of oracle.fault_controls) {
    const crossed=control.name.endsWith('_store_fault');
    const readOnly=control.name==='adc_read_only_same_byte';
    fresh(owner,control.name,{data:control.name!=='adc_read_fault'&&control.name!=='sbb_read_fault',cross:crossed});
    for(const [key,value] of Object.entries(control.initial_ram_words))word(Number.parseInt(key,16),value);
    if(crossed)protect(0x6000,1);if(readOnly)protect(0x5000,1);
    const unit=compile(control.entry_seeds,needs(control.symbol),control.name);seed(control.initial);
    run(unit,control.budget,control.fault,'fault after real ADD prefix preserves incoming CF');
    if(control.resume) {
      if(control.name.endsWith('_read_fault'))map(0x5000,3);
      else protect(crossed?0x6000:0x5000,3);
      run(unit,control.budget,control.resume,'repair resumes without prefix replay');
    }
    close(unit);
  }
  for(const control of oracle.smc_controls) {
    fresh(owner,control.name,{codeWritable:true});for(const [key,value] of Object.entries(control.initial_ram_words))word(Number.parseInt(key,16),value);
    const unit=compile(control.entry_seeds,needs(control.symbol),control.name);seed(control.initial);
    run(unit,control.budget,control.commit,'executable RMW commits then invalidates');
    run(unit,control.budget,null,'retained stale unit',control.retained_stale_status);
    const successor=compile([control.fresh_successor_seed],{read:false,store:false},control.name+'/fresh-successor');
    run(successor,control.budget,control.fresh_successor,'fresh successor never repeats store');
    run(unit,control.budget,null,'old owner still refuses after successor',owner==='replacement'?3:control.retained_stale_status);close(successor);
  }
  for(const branch of oracle.branches) {
    fresh(owner,branch.name);const unit=compile(branch.entry_seeds,{read:false,store:false},branch.name);seed(branch.initial);
    run(unit,branch.budget,branch.expected,'actual Jcc consumes produced CF or OF');close(unit);
  }
}
const provenance={engine_sha256:hash(engineBytes),oracle_sha256:hash(oracleBytes),program_sha256:hash(source),text_sha256:hash(text),artifacts,sources,identities,observations,
  command:[process.execPath,process.argv[1],enginePath,outputDir,root],assembly_command:['clang',...assemble],
  tools:{node:process.version,v8:process.versions.v8,clang:execFileSync('clang',['--version'],{encoding:'utf8'}).split('\n')[0]},
  claim:'independent authored32-bit ADC/SBB register8/memory11 DF0/1 chains,44 finite form/alias rows, real carry/borrow budget continuation and CF/OF Jcc execute emitted replacement/resident children with direct actual helpers. Literal wide arithmetic oracles predate product. One explicit initial CPU/CF/Exit/helper context per fresh case; continuations never patch CPU/CF. Real ADD seeds CF before read/cross-page/read-only/overflow faults, exact prefix state and distinct first-half RAM bytes survive; only permission/map repair, no prefix replay. Both executable RMW writes commit once then invalidate, including ADC same-byte, retained stale4 and fresh memory-free successor. Existing owner imports/key/full-u64 receipts trusted host Memory and direct functions; no JS guest arithmetic/interpreter/helper interception. Full4236 publication checks preserve cancel/Transfer/reserved bytes. Only bounded memory words observed; native owns wider parser/owner matrix. Existing Exitv1 register and Exitv2 memory remain, successorv1 intentionally has no memory imports. No LOCK/thread/bus atomicity, low widths/shifts/generalISA, new API/helper/local/export, PE/provider/startup duplication, SDK/browser/performance/game/full P2-V0 claim.'};
writeFileSync(join(outputDir,'provenance.json'),JSON.stringify(provenance,null,2));
console.log(JSON.stringify({status:'ok',engine_sha256:provenance.engine_sha256,oracle_sha256:provenance.oracle_sha256,
  owners:oracle.owners,register_retired:8,memory_retired:11,form_alias_vectors_each:44,fault_controls_each:7,SMC_controls_each:2,Jcc_controls_each:2,output:outputDir}));
