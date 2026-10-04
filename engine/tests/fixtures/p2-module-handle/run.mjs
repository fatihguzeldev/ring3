import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {readFileSync, readdirSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';

const [enginePath, outputDir, root] = process.argv.slice(2);
assert.ok(root, 'expected actual engine, output directory and repository root');
const fixtureRoot = join(root, 'engine/tests/fixtures/p2-module-handle');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const oracleBytes = readFileSync(join(fixtureRoot, 'oracle.json')), oracle = JSON.parse(oracleBytes);
const programBytes = readFileSync(join(fixtureRoot, 'program.S'));
assert.equal(hash(oracleBytes), '3de9f2bb6972f0ea80333ba674ea0b2add143732524cb1fbb9bdba9965c3d152');
assert.equal(hash(programBytes), 'c88b0b76107dbb04b2f5c6a1ae8a7ba76ee61e3f48054c6ed3e11671e3160953');
const size = 4236, transfer = 140, artifacts = {}, identities = [], operations = [], hostInputs = [], observations = [], ramChecks = [], stackChecks = [];
const fromHex = hex => new Uint8Array(Buffer.from(hex, 'hex'));
function save(filename, bytes) { writeFileSync(join(outputDir, filename), bytes); artifacts[filename] = hash(bytes); return filename; }
function words(fields) { const bytes = new Uint8Array(fields.length * 4), view = new DataView(bytes.buffer); fields.forEach((value, index) => view.setUint32(index * 4, value, true)); return bytes; }
function record(magic, length, fields, version = 1) {
  const bytes = new Uint8Array(length), view = new DataView(bytes.buffer);
  bytes.set(Buffer.from(magic)); view.setUint16(4, version, true); view.setUint16(6, 1, true); view.setUint32(8, length, true);
  fields.forEach((value, index) => view.setUint32(16 + index * 4, value, true)); return bytes;
}
function helper(value = 0) { return record('R3MH', 40, [0, value, 0, 0, 0, 0]); }
function residentRecord(unit) { return words([1, 24, unit.low, unit.high, unit.pointer, unit.length]); }
function installation(unit, slot) { return record('R3IN', 32, [unit.low, unit.high, slot, 0]); }
function authoredText(object) {
  const view = new DataView(object.buffer, object.byteOffset, object.byteLength);
  assert.deepEqual([...object.subarray(0, 7)], [0x7f, 0x45, 0x4c, 0x46, 1, 1, 1]); assert.equal(view.getUint16(18, true), 3);
  const offset = view.getUint32(32, true), stride = view.getUint16(46, true);
  const sections = Array.from({length: view.getUint16(48, true)}, (_, index) => {
    const at = offset + index * stride;
    return {name: view.getUint32(at, true), type: view.getUint32(at + 4, true), offset: view.getUint32(at + 16, true), size: view.getUint32(at + 20, true), link: view.getUint32(at + 24, true), info: view.getUint32(at + 28, true), stride: view.getUint32(at + 36, true)};
  });
  const strings = sections[view.getUint16(50, true)];
  for (const section of sections) { const at = strings.offset + section.name; section.name = object.subarray(at, object.indexOf(0, at)).toString('ascii'); }
  const textIndex = sections.findIndex(section => section.name === '.text'), text = sections[textIndex]; assert.ok(textIndex > 0);
  assert.ok(!sections.some(section => [4, 9].includes(section.type) && section.info === textIndex && section.size > 0));
  const code = object.subarray(text.offset, text.offset + text.size), table = sections.find(section => section.type === 2), names = sections[table.link], symbols = {}, labels = {};
  for (let at = table.offset; at < table.offset + table.size; at += table.stride) {
    const nameAt = names.offset + view.getUint32(at, true), name = object.subarray(nameAt, object.indexOf(0, nameAt)).toString('ascii');
    const section = view.getUint16(at + 14, true), start = view.getUint32(at + 4, true), length = view.getUint32(at + 8, true);
    if (section === textIndex && name) { labels[name] = start; if (length) symbols[name] = {offset: start, size: length, hex: code.subarray(start, start + length).toString('hex')}; }
  }
  assert.deepEqual(symbols, oracle.program.symbols); assert.deepEqual(labels, oracle.program.labels);
  assert.equal(code.length, oracle.program.text_length); assert.equal(code.toString('hex'), oracle.program.text_hex); assert.equal(hash(code), oracle.program.text_sha256);
  return code;
}
// independent input writer; rust alone parses, relocates and resolves all four names.
function authoredPe(code) {
  const bytes = Buffer.alloc(12800), view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  bytes.write('MZ'); view.setUint32(0x3c, 0x80, true); bytes.write('PE\0\0', 0x80);
  view.setUint16(0x84, 0x14c, true); view.setUint16(0x86, 4, true); view.setUint16(0x94, 224, true); view.setUint16(0x96, 0x102, true);
  const optional = 0x98; view.setUint16(optional, 0x10b, true);
  for (const [at, value] of oracle.pe.optional_u32) view.setUint32(optional + at, value, true);
  for (const [at, value] of oracle.pe.optional_u16) view.setUint16(optional + at, value, true);
  oracle.pe.sections.forEach((section, index) => {
    const at = 0x178 + index * 40; bytes.write(section.name, at);
    for (const [offset, value] of [[8,section.virtual_size],[12,section.rva],[16,section.raw_size],[20,section.raw_pointer],[36,section.flags]]) view.setUint32(at + offset, value, true);
  });
  bytes.fill(0xcc,0xe00,0x1200); bytes.set(code,0xffc); bytes.set(fromHex(oracle.pe.initial_data_hex),0x1200);
  bytes.set(words(oracle.pe.import_descriptor),0x1ff0);
  for (const at of [0x1450,0x2240]) bytes.set(words(oracle.pe.original_thunks),at);
  bytes.write(`${oracle.pe.module_name}\0`,0x2260);
  for (const item of oracle.pe.hint_names) { const at=0x1400+item.rva-0x4000; view.setUint16(at,item.hint,true); bytes.write(`${item.name}\0`,at+2); }
  bytes.set(fromHex(oracle.pe.relocation_hex),0x2ffc); return bytes;
}
const objectPath = join(outputDir,'program.o'), assemblyArgs = ['-target','i386-unknown-linux-gnu','-c',join(fixtureRoot,'program.S'),'-o',objectPath];
execFileSync('clang',assemblyArgs,{stdio:['ignore','pipe','pipe']});
save('program.disassembly.txt',execFileSync('xcrun',['llvm-objdump','-d','--x86-asm-syntax=intel',objectPath])); artifacts['program.o']=hash(readFileSync(objectPath));
const code=authoredText(readFileSync(objectPath)), image=authoredPe(code); assert.equal(hash(image),oracle.pe.sha256);
save('linked-module-handle.exe',image); oracle.pe.chunks.forEach((chunk,index)=>{const bytes=image.subarray(chunk.offset,chunk.offset+chunk.length);assert.equal(hash(bytes),chunk.sha256);save(`input-chunk${index}.bin`,bytes);});
save('program.x86',code); save('program.S',programBytes); save('input-oracle.json',oracleBytes); save('run.mjs',readFileSync(import.meta.filename));
const productionPaths=execFileSync('rg',['--files','engine/src'],{cwd:root,encoding:'utf8'}).trim().split('\n').sort();
const sourcePaths=[...productionPaths,'Cargo.toml','engine/Cargo.toml','Cargo.lock','engine/tests/windows_module_handle_wasm.rs','engine/tests/fixtures/p2-module-handle/program.S','engine/tests/fixtures/p2-module-handle/oracle.json','engine/tests/fixtures/p2-module-handle/run.mjs'];
assert.equal(sourcePaths.length,oracle.transport.producer_sources); const sources=Object.fromEntries(sourcePaths.map(file=>[file,hash(readFileSync(join(root,file)))]));save('source-sha256.json',JSON.stringify(sources,null,2));
const engineBytes=readFileSync(enginePath);save('engine.wasm',engineBytes);assert.ok(WebAssembly.validate(engineBytes));
const engineModule=new WebAssembly.Module(engineBytes);assert.deepEqual(WebAssembly.Module.imports(engineModule),[]);
assert.equal(WebAssembly.Module.exports(engineModule).length,oracle.transport.engine_total_exports);assert.equal(WebAssembly.Module.exports(engineModule).filter(row=>row.kind==='function').length,oracle.transport.engine_function_exports);
const arities={open:3,close:0,arena_ptr:0,generation:0,module_ptr:0,module_len:0,begin_image_input:1,append_image_input:2,abort_image_input:0,load_pe32_linked_v2_input_at:2,load_pe32_linked_v3_input_at:2,start_loaded_image:2,compile_entries:2,compile_resident_entries:2,find_resident:1,acknowledge_resident_installation:5,find_installed_resident:3,dispatcher_module:2,guard:6,guard_resident:7,guard_dispatch_entry:5,read32:1,read16:1,write32:2,store32:2,store_resident32:6,capture_call:5,capture_resident_call:6,complete_call:5,complete_resident_call:6,complete_windows_call:4,complete_resident_windows_call:5};
let engine, nextInstance=0, guestRetirements=0;
function refresh() { if (engine.buffer !== engine.memory.buffer) { engine.buffer = engine.memory.buffer; engine.bytes = new Uint8Array(engine.buffer); engine.view = new DataView(engine.buffer); } return engine; }
function arena() { return refresh().bytes.slice(engine.base, engine.base + size); }
function operation(label, action, status = 0, patches = []) {
  const before = arena(), returned = action(), after = arena(), expected = before.slice(), outputPatches = typeof patches === 'function' ? patches(after) : patches;
  for (const [offset, bytes] of outputPatches) expected.set(bytes, offset);
  if (status !== null) assert.equal(returned, status, `${engine.name}/${label}: status or value`);
  assert.deepEqual(after, expected, `${engine.name}/${label}: whole4236 arena`);
  const prefix = `${String(operations.length).padStart(4,'0')}-${engine.name}`;
  operations.push({instance:engine.ordinal,name:engine.name,label,returned:returned===undefined?'void':returned,expected_status:status,patches:outputPatches.map(([offset,bytes]) => ({offset,bytes:bytes.length,hex:Buffer.from(bytes).toString('hex')})),before:save(`${prefix}-before.bin`,before),expected:save(`${prefix}-expected.bin`,expected),after:save(`${prefix}-after.bin`,after)});
  return returned;
}
function hostInput(label, action) {
  const before = arena(); action(); const after = arena(), prefix = `host-${String(hostInputs.length).padStart(4,'0')}-${engine.name}`;
  hostInputs.push({instance:engine.ordinal,name:engine.name,label,before:save(`${prefix}-before.bin`,before),after:save(`${prefix}-after.bin`,after)});
}
function cancel(value) { hostInput(`explicit host Cancel=${value}`, () => refresh().view.setUint32(engine.base + 96,value,true)); }
function request(bytes, label = 'explicit host Transfer input') { assert.ok(bytes.length <= 4096); hostInput(label, () => refresh().bytes.set(bytes,engine.base + transfer)); }
function fresh(name, owner, pages = 8) {
  const instance = new WebAssembly.Instance(engineModule, {}), api = {};
  for (const [name, arity] of Object.entries(arities)) { const fn = instance.exports[`ring3_abi_v1_${name}`]; assert.equal(typeof fn,'function',name); assert.equal(fn.length,arity,name); api[name]=fn; }
  const ordinal = ++nextInstance, key = 0xa353cdef00000000n + BigInt(ordinal);
  engine = {name,owner,ordinal,api,key,low:Number(key & 0xffffffffn),high:Number(key >> 32n),memory:instance.exports.memory,units:[]};
  assert.ok(engine.memory instanceof WebAssembly.Memory); assert.equal(api.open(pages,engine.low,engine.high),0); engine.base=api.arena_ptr()>>>0; refresh();
  assert.ok(engine.base>0 && engine.base+size<=engine.bytes.length);
  const initial = new Uint8Array(size); initial.set(fromHex(oracle.records.initial_state_hex)); initial.set(fromHex(oracle.records.initial_exit_hex),56); initial.set(fromHex(oracle.records.helper_zero_hex),100); assert.deepEqual(arena(),initial);
  save(`${name}-initial-arena.bin`,initial);
  hostInput('explicit diagnostic and Transfer sentinel; CPU untouched', () => refresh().bytes.fill(0xa5,engine.base+100,engine.base+size));
}
function read(address,value,label) { operation(label,()=>engine.api.read32(address),0,[[100,helper(value)]]); return refresh().view.getUint32(engine.base+120,true); }
function readData(expectedHex,label) {
  const bytes=fromHex(expectedHex),view=new DataView(bytes.buffer);assert.equal(bytes.length,24);
  const actual=Array.from({length:6},(_,index)=>read(engine.receipt.image_base+oracle.runtime.data_rva+index*4,view.getUint32(index*4,true),`${label}/word${index}`));
  ramChecks.push({instance:engine.ordinal,name:engine.name,label,address:engine.receipt.image_base+oracle.runtime.data_rva,bytes:24,actual_hex:Buffer.from(words(actual)).toString('hex'),expected_hex:expectedHex});
}
function stack(values,label) { const actual=oracle.runtime.stack_addresses.map((address,index)=>read(address,values[index],`${label}/word${index}`));stackChecks.push({instance:engine.ordinal,name:engine.name,label,addresses:oracle.runtime.stack_addresses,actual,expected:values}); }
function entriesRequest(entries,gates=[]) { request(words([...entries,...gates.flat()]),'explicit receipt entry/Gate and frozen authored caller-offset input'); }
function childShape(bytes, unit, helpers, dispatch = false) {
  let at = 8;
  const unsigned = () => { let value=0,shift=0; for(let index=0;index<5;index++){assert.ok(at<bytes.length);const byte=bytes[at++];value|=(byte&127)<<shift;if(!(byte&128))return value>>>0;shift+=7;}assert.fail('bounded unsigned LEB'); };
  const signed = () => { let value=0n,shift=0n,byte; do{assert.ok(at<bytes.length && shift<35n);byte=bytes[at++];value|=BigInt(byte&127)<<shift;shift+=7n;}while(byte&128);if(byte&64)value-=1n<<shift;return Number(BigInt.asUintN(32,value)); };
  const name = () => {const length=unsigned(),end=at+length;assert.ok(end<=bytes.length);const value=Buffer.from(bytes.subarray(at,end)).toString('utf8');at=end;return value;};
  const limits = () => {const flags=unsigned(),minimum=unsigned();assert.ok(flags===0||flags===1);return {minimum,maximum:flags?unsigned():null};};
  const types=[], imports=[], functions=[], exports=[]; let locals, guard;
  assert.deepEqual([...bytes.subarray(0,8)],[0,97,115,109,1,0,0,0]);
  while(at<bytes.length) {
    const section=bytes[at++],length=unsigned(),end=at+length;assert.ok(end<=bytes.length);
    if(section===1) {for(let count=unsigned();count>0;count--){assert.equal(bytes[at++],0x60);const parameters=Array.from({length:unsigned()},()=>bytes[at++]),results=Array.from({length:unsigned()},()=>bytes[at++]);types.push({parameters,results});}}
    else if(section===2) {for(let count=unsigned();count>0;count--){const module=name(),field=name(),kind=bytes[at++];if(kind===0)imports.push({module,name:field,kind:'function',type:unsigned()});else if(kind===2){assert.deepEqual(limits(),{minimum:1,maximum:null});imports.push({module,name:field,kind:'memory'});}else if(kind===1){assert.equal(bytes[at++],0x70);assert.deepEqual(limits(),{minimum:8,maximum:8});imports.push({module,name:field,kind:'table'});}else assert.fail('unexpected child import kind');}}
    else if(section===3) {for(let count=unsigned();count>0;count--)functions.push(unsigned());}
    else if(section===7) {for(let count=unsigned();count>0;count--)exports.push({name:name(),kind:bytes[at++],index:unsigned()});}
    else if(section===10) {
      assert.equal(unsigned(),1);const bodyLength=unsigned(),bodyEnd=at+bodyLength;assert.equal(bodyEnd,end);locals=Array.from({length:unsigned()},()=>[unsigned(),bytes[at++]]);
      assert.deepEqual(locals,dispatch?[[5,0x7f]]:oracle.transport.locals);
      const expected=[engine.low,engine.high,...(dispatch?[]:engine.owner==='resident'?[unit.low,unit.high]:[unit.generation])];
      guard={name:dispatch?'guard_dispatch_entry':engine.owner==='resident'?'guard_resident':'guard',constants:expected};
      for(const value of expected){assert.equal(bytes[at++],0x41);assert.equal(signed(),value);}
      for(const index of [0,1,3]){assert.equal(bytes[at++],0x20);assert.equal(unsigned(),index);}
      assert.equal(bytes[at++],0x10);assert.equal(unsigned(),0);at=bodyEnd;
    } else assert.fail(`unexpected generated child section${section}`);
    assert.equal(at,end);
  }
  assert.deepEqual(functions,[0]);assert.deepEqual(types[0],{parameters:[0x7f,0x7f,0x7f,0x7f],results:[0x7f]});
  assert.deepEqual(exports,[{name:'run',kind:0,index:helpers.length}]);assert.ok(locals && guard);
  const importArities={};for(const row of imports.filter(row=>row.kind==='function')){const type=types[row.type],arity=arities[row.name];assert.ok(arity!==undefined);assert.deepEqual(type,{parameters:Array(arity).fill(0x7f),results:[0x7f]});importArities[row.name]=arity;}
  assert.deepEqual(imports.filter(row=>row.kind==='function').map(row=>row.name),helpers);
  return {locals,guard,import_arities:importArities,run_arity:4};
}
function instantiate(unit, helpers) {
  refresh(); assert.ok(unit.pointer>0 && unit.length>8 && unit.pointer+unit.length<=engine.bytes.length); unit.bytes=engine.bytes.slice(unit.pointer,unit.pointer+unit.length);
  assert.ok(WebAssembly.validate(unit.bytes)); const shape=childShape(unit.bytes,unit,helpers),module=new WebAssembly.Module(unit.bytes);
  const imports=WebAssembly.Module.imports(module);assert.deepEqual(imports,[{module:'env',name:'memory',kind:'memory'},...helpers.map(name=>({module:'ring3',name,kind:'function'}))]);
  assert.deepEqual(WebAssembly.Module.exports(module),[{name:'run',kind:'function'}]);
  unit.instance=new WebAssembly.Instance(module,{env:{memory:engine.memory},ring3:Object.fromEntries(helpers.map(name=>[name,engine.api[name]]))}); unit.run=unit.instance.exports.run; assert.equal(unit.run.length,4);
  unit.filename=save(`${engine.name}-${unit.label}.wasm`,unit.bytes); engine.units.push(unit);
  identities.push({instance:engine.ordinal,name:engine.name,owner:engine.owner,key:engine.key.toString(),label:unit.label,id:unit.id?.toString(),generation:unit.generation,pointer:unit.pointer,length:unit.length,entries:unit.entries,imports,...shape,module_sha256:hash(unit.bytes),artifact:unit.filename}); return unit;
}
function replacement(entries,gates,label,generation,helpers) {
  entriesRequest(entries,gates);operation(`replacement ${label} compile preserves CPU and context`,()=>engine.api.compile_entries(entries.length,gates.length));
  operation(`replacement ${label} generation`,()=>engine.api.generation(),generation);
  const pointer=operation(`replacement ${label} module pointer`,()=>engine.api.module_ptr(),null)>>>0,length=operation(`replacement ${label} module length`,()=>engine.api.module_len(),null)>>>0;
  return instantiate({label,generation,pointer,length,entries},helpers);
}
function resident(entries, gates, label, helpers) {
  entriesRequest(entries,gates); let unit;
  operation(`resident compile ${label}`,()=>engine.api.compile_resident_entries(entries.length,gates.length),0,after=>{
    const view=new DataView(after.buffer,transfer,24); unit={low:view.getUint32(8,true),high:view.getUint32(12,true),pointer:view.getUint32(16,true),length:view.getUint32(20,true),entries,label};
    unit.id=BigInt(unit.low)|(BigInt(unit.high)<<32n); assert.notEqual(unit.id,0n); assert.ok(!engine.units.some(item=>item.id===unit.id)); return [[transfer,residentRecord(unit)]];
  });
  instantiate(unit,helpers); entries.forEach(pc=>operation(`${label} returned full owner lookup`,()=>engine.api.find_resident(pc),0,[[transfer,residentRecord(unit)]])); return unit;
}
function install(unit, slot) {
  operation('not installed before table assignment',()=>engine.api.find_installed_resident(engine.low,engine.high,unit.entries[0]),17);
  engine.table.set(slot,unit.run); operation(`acknowledge ${unit.label} slot${slot}`,()=>engine.api.acknowledge_resident_installation(engine.low,engine.high,unit.low,unit.high,slot),0,[[transfer,installation(unit,slot)]]);
  assert.equal(engine.table.get(slot),unit.run); unit.slot=slot;
  for (const pc of unit.entries) operation('installed full owner/slot lookup',()=>engine.api.find_installed_resident(engine.low,engine.high,pc),0,[[transfer,installation(unit,slot)]]);
}
function dispatcher() {
  let pointer,length;
  operation('dispatcher receipt',()=>engine.api.dispatcher_module(engine.low,engine.high),0,after=>{const view=new DataView(after.buffer,transfer,32);pointer=view.getUint32(24,true);length=view.getUint32(28,true);return [[transfer,record('R3DP',32,[engine.low,engine.high,pointer,length])]];});
  refresh(); assert.ok(pointer>0 && length>8 && pointer+length<=engine.bytes.length); const bytes=engine.bytes.slice(pointer,pointer+length),shape=childShape(bytes,{},['guard_dispatch_entry','find_installed_resident'],true),module=new WebAssembly.Module(bytes),imports=WebAssembly.Module.imports(module);
  assert.deepEqual(imports,[{module:'env',name:'memory',kind:'memory'},{module:'env',name:'table',kind:'table'},{module:'ring3',name:'guard_dispatch_entry',kind:'function'},{module:'ring3',name:'find_installed_resident',kind:'function'}]);
  const instance=new WebAssembly.Instance(module,{env:{memory:engine.memory,table:engine.table},ring3:{guard_dispatch_entry:engine.api.guard_dispatch_entry,find_installed_resident:engine.api.find_installed_resident}}); assert.equal(instance.exports.run.length,4);
  engine.dispatcher={pointer,length,bytes,run:instance.exports.run,label:'returning-dispatcher'}; const artifact=save(`${engine.name}-returning-dispatcher.wasm`,bytes);
  identities.push({instance:engine.ordinal,name:engine.name,owner:'dispatcher',key:engine.key.toString(),label:'returning-dispatcher',pointer,length,imports,...shape,module_sha256:hash(bytes),artifact});
}
function run(target,budget,label,patches=[],status=0,pointers=[engine.base,engine.base+56,engine.base+96]) {
  operation(label,()=>target.run(pointers[0],pointers[1],budget,pointers[2]),status,patches);
  if(status===0)guestRetirements+=refresh().view.getUint32(engine.base+76,true);
}
function observe(phase,expected) {
  const bytes=arena(),fields={state_hex:Buffer.from(bytes.subarray(0,56)).toString('hex'),exit_hex:Buffer.from(bytes.subarray(56,96)).toString('hex'),helper_hex:Buffer.from(bytes.subarray(100,140)).toString('hex')};
  if(phase.endsWith('captured')||phase==='terminal')fields.frame_hex=Buffer.from(bytes.subarray(transfer,transfer+112)).toString('hex');
  for(const [key,value] of Object.entries(expected))assert.equal(fields[key],value,`${engine.name}/${phase}/${key}`);
  observations.push({instance:engine.ordinal,name:engine.name,owner:engine.owner,phase,...fields,expected,arena:save(`${engine.name}-${phase}-observation.bin`,bytes)});
}
function submit(bytes,label) {
  assert.equal(bytes.length,oracle.pe.file_size);operation(`${label}/begin copied complete input`,()=>engine.api.begin_image_input(bytes.length));
  oracle.pe.chunks.forEach((chunk,index)=>{
    request(bytes.subarray(chunk.offset,chunk.offset+chunk.length),`${label}/explicit chunk${index} offset${chunk.offset}`);
    operation(`${label}/copy chunk${index}`,()=>engine.api.append_image_input(chunk.offset,chunk.length));
    hostInput(`${label}/overwrite submitted Transfer after copy`,()=>refresh().bytes.fill(0xa5,engine.base+transfer,engine.base+size));
  });
}
function commit(example,label='Rust linked-v3 copied input commit') {
  operation(label,()=>engine.api.load_pe32_linked_v3_input_at(example.base,oracle.runtime.gate_base),0,[[transfer,fromHex(example.receipt_hex)]]);
  const bytes=arena().slice(transfer,transfer+80),view=new DataView(bytes.buffer);save(`${engine.name}-linked-receipt.bin`,bytes);
  engine.receipt={image_base:view.getUint32(16,true),image_size:view.getUint32(20,true),entry:view.getUint32(24,true),mapped_pages:view.getUint32(28,true),gate_base:view.getUint32(32,true),gate_count:view.getUint32(36,true),gates:Array.from({length:view.getUint32(36,true)},(_,index)=>[view.getUint32(48+index*8,true),view.getUint32(52+index*8,true)])};
  assert.equal(engine.receipt.entry,example.entry);assert.deepEqual(engine.receipt.gates,oracle.runtime.gates);
}
function load(example) {
  submit(image,'valid authored PE');commit(example);
  read(engine.receipt.image_base+0xb4,oracle.pe.preferred_base,'preferred ImageBase header remains immutable');
  oracle.pe.target_rvas.forEach((rva,index)=>read(engine.receipt.image_base+rva,example.patched_words[index],`Rust HIGHLOW operand${index}`));
  engine.receipt.gates.forEach(([pc],index)=>{read(engine.receipt.image_base+0x4050+index*4,pc,`Rust linked IAT slot${index}`);read(pc,0xb0f,`RX UD2 Gate${index}`);read(pc+4,0,`RX zero Gate tail${index}`);});
  read(engine.receipt.image_base+0x4060,0,'IAT zero terminator');
  operation('narrow header diagnostic retains real MZ',()=>engine.api.read16(engine.receipt.image_base),0,[[100,fromHex(oracle.records.narrow_mz_helper_hex)]]);
  readData(oracle.pe.initial_data_hex,'loaded initialized data');
}
function startup(example) {
  operation('Rust owns initial CPU, stack and private entry',()=>engine.api.start_loaded_image(oracle.runtime.stack_base,oracle.runtime.stack_pages),0,[[0,fromHex(example.startup_state_hex)],[56,fromHex(oracle.records.startup_exit_hex)]]);
}
function setup(example) {
  fresh(example.name,example.owner,oracle.runtime.pages);const initial=arena().slice(0,96);load(example);
  const receipt=engine.receipt;
  if(example.owner==='replacement'){
    const set=receipt.gates[1];engine.phase1=replacement([receipt.entry,set[0]],[set],'phase1',1,oracle.transport.replacement_phase1_imports);engine.gate=engine.phase1;
  }else{
    engine.table=new WebAssembly.Table({element:'anyfunc',initial:8,maximum:8});
    engine.gate=resident(receipt.gates.map(([pc])=>pc),receipt.gates,'all-four-gates',oracle.transport.resident_gate_imports);install(engine.gate,0);dispatcher();
    operation('resident setup leaves replacement generation zero',()=>engine.api.generation(),0);
  }
  assert.deepEqual(arena().subarray(0,96),initial,'all input/metadata setup preserves initial context');startup(example);
  observe('startup',{state_hex:example.startup_state_hex,exit_hex:oracle.records.startup_exit_hex,helper_hex:oracle.records.helper_zero_hex});stack(oracle.runtime.stack_initial,'Rust zeroed stack');
  if(example.owner==='resident'){
    run(engine.dispatcher,oracle.runtime.budget,'real initial NeedCode before caller admission',[[56,fromHex(oracle.records.startup_exit_hex)]]);
    observe('missing-entry',{state_hex:example.startup_state_hex,exit_hex:oracle.records.startup_exit_hex,helper_hex:oracle.records.helper_zero_hex});
    const before=arena(),pc=new DataView(before.buffer).getUint32(48,true);assert.equal(pc,receipt.entry);assert.equal(new DataView(before.buffer).getUint32(72,true),3);
    engine.caller=resident([pc,...oracle.program.caller_offsets.slice(1).map(offset=>receipt.entry+offset)],[],'all-callers-and-canary',oracle.transport.resident_caller_imports);install(engine.caller,1);
    assert.deepEqual(arena().subarray(0,96),before.subarray(0,96));
  }
}
function phase2() {
  const receipt=engine.receipt,gates=receipt.gates.filter((_,index)=>index!==1),entries=[...oracle.program.caller_offsets.slice(1).map(offset=>receipt.entry+offset),...gates.map(([pc])=>pc)];
  assert.equal(entries.length,7);engine.caller=replacement(entries,gates,'phase2',2,oracle.transport.replacement_phase2_imports);engine.gate=engine.caller;
  run(engine.phase1,oracle.runtime.budget,'old generation refuses3 after exact recompile without code write',[],3);
}
function capture(stage,expected) {
  if(engine.owner==='resident'){
    assert.deepEqual(arena().subarray(transfer,transfer+32),installation(engine.gate,0));
    operation(`capture private ${stage.api} full Gate-unit owner`,()=>engine.api.capture_resident_call(engine.low,engine.high,engine.gate.low,engine.gate.high,2,stage.args.length),0,[[transfer,fromHex(expected.frame_hex)]]);
  }else operation(`capture private ${stage.api} replacement owner`,()=>engine.api.capture_call(engine.low,engine.high,engine.gate.generation,2,stage.args.length),0,[[transfer,fromHex(expected.frame_hex)]]);
}
function provider(token,keyLow=engine.low) { return engine.owner==='resident'?engine.api.complete_resident_windows_call(keyLow,engine.high,engine.gate.low,engine.gate.high,token):engine.api.complete_windows_call(keyLow,engine.high,engine.gate.generation,token); }
function scalar(token) { return engine.owner==='resident'?engine.api.complete_resident_call(engine.low,engine.high,engine.gate.low,engine.gate.high,token,0):engine.api.complete_call(engine.low,engine.high,engine.gate.generation,token,0); }
function retainedBytes() {
  refresh();for(const unit of engine.units.filter(unit=>engine.owner==='resident'||unit===engine.gate))assert.deepEqual(engine.bytes.slice(unit.pointer,unit.pointer+unit.length),unit.bytes);
  if(engine.dispatcher){assert.deepEqual(engine.bytes.slice(engine.dispatcher.pointer,engine.dispatcher.pointer+engine.dispatcher.length),engine.dispatcher.bytes);assert.equal(engine.table.length,8);assert.equal(engine.table.get(0),engine.gate.run);assert.equal(engine.table.get(1),engine.caller.run);for(let index=2;index<8;index++)assert.equal(engine.table.get(index),null);}
}
for(const example of oracle.cases){
  setup(example);let retired=0;
  for(const [index,stage] of oracle.stages.entries()){
    const expected=example.stages[index],target=engine.owner==='resident'?engine.dispatcher:index===0?engine.phase1:engine.caller;
    const patches=[[0,fromHex(expected.stop_state_hex)],[56,fromHex(stage.gate_exit_hex)],[100,fromHex(oracle.records.helper_zero_hex)]];
    if(engine.owner==='resident')patches.push([transfer,installation(engine.gate,0)]);
    run(target,oracle.runtime.budget,`real ${stage.api} IAT guest slice`,patches);retired+=refresh().view.getUint32(engine.base+76,true);
    observe(`${stage.api}-gate`,{state_hex:expected.stop_state_hex,exit_hex:stage.gate_exit_hex,helper_hex:oracle.records.helper_zero_hex});
    capture(stage,expected);observe(`${stage.api}-captured`,{state_hex:expected.stop_state_hex,exit_hex:stage.gate_exit_hex,helper_hex:oracle.records.helper_zero_hex,frame_hex:expected.frame_hex});
    if(stage.api==='GetModuleHandleA'){
      operation('wrong full key precedes wrong provider token',()=>provider(0,engine.low^1),3);
      operation('wrong current private provider token',()=>provider(0),14);
      cancel(1);operation('cancelled NULL completion retains same private token',()=>provider(stage.token),16);cancel(0);
      hostInput('negative diagnostic argument/return tamper; private NULL/frame unchanged',()=>{refresh().view.setUint32(engine.base+transfer+48,oracle.negative_controls.diagnostic_argument,true);engine.view.setUint32(engine.base+transfer+40,oracle.negative_controls.diagnostic_return,true);});
    }
    if(stage.api==='ExitProcess'){
      hostInput('negative diagnostic exit code tamper; private checksum unchanged',()=>refresh().view.setUint32(engine.base+transfer+48,oracle.negative_controls.diagnostic_argument,true));
      operation('nonreturning ExitProcess scalar bypass refused',()=>scalar(stage.token),7);
      operation('private terminal checksum without return',()=>provider(stage.token),0,[[56,fromHex(example.terminal_exit_hex)]]);
      const frame=fromHex(expected.frame_hex),view=new DataView(frame.buffer);view.setUint32(48,oracle.negative_controls.diagnostic_argument,true);
      observe('terminal',{state_hex:expected.stop_state_hex,exit_hex:example.terminal_exit_hex,helper_hex:oracle.records.helper_zero_hex,frame_hex:Buffer.from(frame).toString('hex')});
    }else{
      operation(`private ${stage.api} stdcall return`,()=>provider(stage.token),0,[[0,fromHex(expected.returned_state_hex)],[56,fromHex(oracle.records.returned_exit_hex)]]);
      observe(`${stage.api}-returned`,{state_hex:expected.returned_state_hex,exit_hex:oracle.records.returned_exit_hex,helper_hex:oracle.records.helper_zero_hex});
      operation('consumed private token cannot replay',()=>provider(stage.token),14);
    }
    readData(expected.data_hex,`${stage.api} guest data`);stack(expected.stack_values,`${stage.api} guest return and argument bytes`);
    if(index===0&&engine.owner==='replacement')phase2();
  }
  assert.equal(retired,17);readData(example.terminal_data_hex,'terminal data and unexecuted canary');retainedBytes();
  operation('terminal latch precedes provider token',()=>provider(0),21);operation('terminal latch precedes malformed copied loader',()=>engine.api.load_pe32_linked_v3_input_at(0,0),21);
  operation('terminal RAM write rejected without helper publication',()=>engine.api.write32(example.data_address+20,0xdeadbeef),21);
  operation('terminal pointer sentinel',()=>engine.api.module_ptr(),0);operation('terminal length sentinel',()=>engine.api.module_len(),0);
  operation('retained replacement generation',()=>engine.api.generation(),engine.owner==='resident'?0:2);
  hostInput('explicit postterminal canary State/Exit restoration is only a negative control',()=>{refresh().bytes.set(fromHex(example.canary_restore_state_hex),engine.base);engine.bytes.set(fromHex(oracle.records.canary_restore_exit_hex),engine.base+56);});
  const targets=engine.owner==='resident'?[engine.caller,engine.gate,engine.dispatcher]:[engine.caller,engine.phase1];
  for(const target of targets)run(target,oracle.runtime.budget,'retained executable refuses private terminal',[],21);
  cancel(1);run(engine.owner==='resident'?engine.dispatcher:engine.caller,0,'terminal precedes cancel, zero budget and invalid pointers',[],21,[0xffffffff,0xffffffff,0xffffffff]);cancel(0);
  readData(example.terminal_data_hex,'negative canary restores remain inert');stack(example.stages[3].stack_values,'terminal leaves guest exit words');retainedBytes();
  operation('close preserves retained arena tombstone',()=>engine.api.close());run(engine.owner==='resident'?engine.dispatcher:engine.caller,64,'retained current executable becomes Closed',[],5);
  operation('Closed before malformed new copied loader',()=>engine.api.load_pe32_linked_v3_input_at(0,0),5);
}
const retry=oracle.cases.find(item=>item.base===0x500000);
fresh('negative-old-v2-retains-input','replacement');submit(image,'old-profile candidate');cancel(1);
operation('old copied-input-v2 refuses new symbol before consumption',()=>engine.api.load_pe32_linked_v2_input_at(retry.base,oracle.runtime.gate_base),20);
commit(retry,'same copied bytes admit explicit v3 despite cancellation');operation('startup still honours preserved cancellation',()=>engine.api.start_loaded_image(0x8000,1),16);cancel(0);startup(retry);operation('old-profile control close',()=>engine.api.close());
fresh('negative-incomplete-input','replacement');operation('begin incomplete image input',()=>engine.api.begin_image_input(image.length));
request(image.subarray(0,4096),'explicit first incomplete chunk');operation('copy first incomplete chunk',()=>engine.api.append_image_input(0,4096));
operation('incomplete input refuses before candidate',()=>engine.api.load_pe32_linked_v3_input_at(retry.base,oracle.runtime.gate_base),7);
for(const chunk of oracle.pe.chunks.slice(1)){request(image.subarray(chunk.offset,chunk.offset+chunk.length),'explicit remaining ordered chunk');operation('copy remaining ordered chunk',()=>engine.api.append_image_input(chunk.offset,chunk.length));}
commit(retry,'complete same input retries without restart');startup(retry);operation('incomplete control close',()=>engine.api.close());
fresh('negative-structural-and-unknown','replacement');const unknown=Buffer.from(image),alias=Buffer.from(image);unknown[0x22a2]='x'.charCodeAt(0);alias[0x22a2]='x'.charCodeAt(0);alias.writeUInt32LE(0x4e40,0x2000);
save('negative-unknown-symbol.exe',unknown);save('negative-structural-alias.exe',alias);
for(const [label,bytes,status] of [['structural alias before unknown',alias,19],['unknown exact symbol',unknown,20]]){
  submit(bytes,label);operation(`${label}/failed private candidate`,()=>engine.api.load_pe32_linked_v3_input_at(retry.base,oracle.runtime.gate_base),status);
  operation(`${label}/no mapped image text`,()=>engine.api.read32(retry.entry),0,[[100,record('R3MH',40,[1,0,1,retry.entry,1,4])]]);
  operation(`${label}/no mapped gate`,()=>engine.api.read32(oracle.runtime.gate_base),0,[[100,record('R3MH',40,[1,0,1,oracle.runtime.gate_base,1,4])]]);
  operation(`${label}/abort retained failed input`,()=>engine.api.abort_image_input());
}
submit(image,'valid retry after retained failures');commit(retry,'same instance valid candidate retry');startup(retry);operation('structural control close',()=>engine.api.close());
assert.equal(guestRetirements,68);assert.equal(nextInstance,7);assert.equal(identities.length,10);
const artifactFiles=readdirSync(outputDir).filter(name=>name!=='provenance.json').sort();assert.deepEqual(artifactFiles,Object.keys(artifacts).sort());
for(const file of artifactFiles)assert.equal(hash(readFileSync(join(outputDir,file))),artifacts[file]);
assert.equal(operations.length,567);assert.equal(hostInputs.length,113);assert.equal(observations.length,54);assert.equal(ramChecks.length,28);assert.equal(stackChecks.length,24);assert.equal(artifactFiles.length,2020);
const provenance={engine_sha256:hash(engineBytes),oracle_sha256:hash(oracleBytes),program_sha256:hash(programBytes),text_sha256:hash(code),pe_sha256:hash(image),artifacts,sources,identities,operations,host_inputs:hostInputs,observations,ram_checks:ramChecks,stack_checks:stackChecks,counts:{instances:nextInstance,children:identities.length,operations:operations.length,host_inputs:hostInputs.length,observations:observations.length,ram_checks:ramChecks.length,stack_checks:stackChecks.length,guest_retired:guestRetirements,artifacts:artifactFiles.length},command:[process.execPath,process.argv[1],enginePath,outputDir,root],assembly_command:['clang',...assemblyArgs],tools:{node:process.version,v8:process.versions.v8,clang:execFileSync('clang',['--version'],{encoding:'utf8'}).split('\n')[0]},claim:oracle.claim+' All public operation boundaries persist raw whole4236 before/expected/after; explicit host Transfer/Cancel/negative diagnostic or postterminal State writes have separate raw before/after. Positive loaded CPU/data/stack/IAT derive only from Rust startup and genuine guest execution. Wrong-key/token/cancel and diagnostic frame tamper challenge private authority. Resident uses exactly two installed multientry units in a fixed8-slot table. Replacement uses two generations with status3 old generation refusal. Native evidence owns no-image/nonNULL/wrong-frame/header-unmap/thread-state and complete lifetime priorities; old missing raw arena history remains absent.'};
writeFileSync(join(outputDir,'provenance.json'),JSON.stringify(provenance,null,2));
console.log(JSON.stringify({status:'ok',engine_sha256:provenance.engine_sha256,oracle_sha256:provenance.oracle_sha256,counts:provenance.counts,output:outputDir}));
