import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {readFileSync, readdirSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';

const [enginePath, outputDir, root] = process.argv.slice(2);
assert.ok(root, 'expected actual engine, output directory and repository root');
const fixtureRoot = join(root, 'engine/tests/fixtures/p2-byte-load');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const oracleBytes = readFileSync(join(fixtureRoot, 'oracle.json')), oracle = JSON.parse(oracleBytes);
const programBytes = readFileSync(join(fixtureRoot, 'program.S'));
assert.equal(hash(oracleBytes), '1c7552e267142630a6c94fe00ab8724733e589ae421a6c47a31a4784f4f7be8e');
assert.equal(hash(programBytes), '15886209e03e8839ca539c3ae630cff08645a2238a0eab9939425db05850f236');
const size = 4236, transfer = 140, artifacts = {}, identities = [], operations = [], hostInputs = [], observations = [], ramChecks = [], stackChecks = [], syntheticBindings = [], syntheticObservations = [];
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
// rust alone parses, relocates and resolves the declared named import.
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
  bytes.write(`${oracle.pe.module_name}\0`,0x2250);
  for (const item of oracle.pe.hint_names) { const at=0x1400+item.rva-0x4000; view.setUint16(at,item.hint,true); bytes.write(`${item.name}\0`,at+2); }
  bytes.set(fromHex(oracle.pe.relocation_hex),0x2ffc); return bytes;
}
const objectPath = join(outputDir,'program.o'), assemblyArgs = ['-target','i386-unknown-linux-gnu','-c',join(fixtureRoot,'program.S'),'-o',objectPath];
execFileSync('clang',assemblyArgs,{stdio:['ignore','pipe','pipe']});
save('program.disassembly.txt',execFileSync('xcrun',['llvm-objdump','-d','--x86-asm-syntax=intel',objectPath])); artifacts['program.o']=hash(readFileSync(objectPath));
const code=authoredText(readFileSync(objectPath)), image=authoredPe(code); assert.equal(hash(image),oracle.pe.sha256);
save('linked-byte-load.exe',image); oracle.pe.chunks.forEach((chunk,index)=>{const bytes=image.subarray(chunk.offset,chunk.offset+chunk.length);assert.equal(hash(bytes),chunk.sha256);save(`input-chunk${index}.bin`,bytes);});
save('program.x86',code); save('program.S',programBytes); save('input-oracle.json',oracleBytes); save('run.mjs',readFileSync(import.meta.filename));
const productionPaths=execFileSync('rg',['--files','engine/src'],{cwd:root,encoding:'utf8'}).trim().split('\n').sort();
const sourcePaths=[...productionPaths,'Cargo.toml','engine/Cargo.toml','Cargo.lock','engine/tests/cpu_byte_load_wasm.rs','engine/tests/fixtures/p2-byte-load/program.S','engine/tests/fixtures/p2-byte-load/oracle.json','engine/tests/fixtures/p2-byte-load/run.mjs'];
assert.equal(sourcePaths.length,oracle.transport.producer_sources); const sources=Object.fromEntries(sourcePaths.map(file=>[file,hash(readFileSync(join(root,file)))]));save('source-sha256.json',JSON.stringify(sources,null,2));
const engineBytes=readFileSync(enginePath);save('engine.wasm',engineBytes);assert.ok(WebAssembly.validate(engineBytes));
const engineModule=new WebAssembly.Module(engineBytes);assert.deepEqual(WebAssembly.Module.imports(engineModule),[]);
assert.equal(WebAssembly.Module.exports(engineModule).length,oracle.transport.engine_total_exports);assert.equal(WebAssembly.Module.exports(engineModule).filter(row=>row.kind==='function').length,oracle.transport.engine_function_exports);
const arities={open:3,close:0,arena_ptr:0,generation:0,module_ptr:0,module_len:0,map:3,protect:3,upload:2,begin_image_input:1,append_image_input:2,load_pe32_linked_v2_input_at:2,start_loaded_image:2,compile_entries:2,compile_resident_entries:2,find_resident:1,acknowledge_resident_installation:5,find_installed_resident:3,dispatcher_module:2,guard:6,guard_resident:7,guard_dispatch_entry:5,read32:1,read8:1,read16:1,write8:2,store32:2,store_resident32:6,store8:2,store_resident8:6,capture_call:5,capture_resident_call:6,complete_windows_call:4,complete_resident_windows_call:5};
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
  const ordinal = ++nextInstance, key = 0xa354cdef00000000n + BigInt(ordinal);
  engine = {name,owner,ordinal,api,key,low:Number(key & 0xffffffffn),high:Number(key >> 32n),memory:instance.exports.memory,units:[]};
  assert.ok(engine.memory instanceof WebAssembly.Memory); assert.equal(api.open(pages,engine.low,engine.high),0); engine.base=api.arena_ptr()>>>0; refresh();
  assert.ok(engine.base>0 && engine.base+size<=engine.bytes.length);
  const initial = new Uint8Array(size); initial.set(fromHex(oracle.records.initial_state_hex)); initial.set(fromHex(oracle.records.initial_exit_hex),56); initial.set(fromHex(oracle.records.helper_zero_hex),100); assert.deepEqual(arena(),initial);
  save(`${name}-initial-arena.bin`,initial);
  hostInput('explicit diagnostic and Transfer sentinel; CPU untouched', () => refresh().bytes.fill(0xa5,engine.base+100,engine.base+size));
}
function read(address,value,label) { operation(label,()=>engine.api.read32(address),0,[[100,helper(value)]]); return refresh().view.getUint32(engine.base+120,true); }
function upload(address,bytes,label) { request(bytes,`${label}/owned bytes`);operation(label,()=>engine.api.upload(address,bytes.length)); }
function ramRanges(ranges,label) {
  for(const [address,hex] of ranges) {
    const bytes=fromHex(hex),actual=[];
    for(let index=0;index<bytes.length;index++) {
      operation(`${label}/byte${index}`,()=>engine.api.read8((address+index)>>>0),0,[[100,record('R3MH',40,[0,bytes[index],0,0,0,1],2)]]);
      actual.push(refresh().view.getUint32(engine.base+120,true));
    }
    ramChecks.push({instance:engine.ordinal,name:engine.name,label,address,bytes:bytes.length,actual_hex:Buffer.from(actual).toString('hex'),expected_hex:hex});
  }
}
function readData(expectedHex,label) {
  const bytes=fromHex(expectedHex),view=new DataView(bytes.buffer);assert.equal(bytes.length,32);
  const actual=Array.from({length:8},(_,index)=>read(engine.receipt.image_base+oracle.runtime.data_rva+index*4,view.getUint32(index*4,true),`${label}/word${index}`));
  ramChecks.push({instance:engine.ordinal,name:engine.name,label,address:engine.receipt.image_base+oracle.runtime.data_rva,bytes:32,actual_hex:Buffer.from(words(actual)).toString('hex'),expected_hex:expectedHex});
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
  if('frame_hex' in expected)fields.frame_hex=Buffer.from(bytes.subarray(transfer,transfer+112)).toString('hex');
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
function commit(example,label='Rust linked-v2 copied input commit') {
  operation(label,()=>engine.api.load_pe32_linked_v2_input_at(example.base,oracle.runtime.gate_base),0,[[transfer,fromHex(example.receipt_hex)]]);
  const bytes=arena().slice(transfer,transfer+72),view=new DataView(bytes.buffer);save(`${engine.name}-linked-receipt.bin`,bytes);
  engine.receipt={image_base:view.getUint32(16,true),image_size:view.getUint32(20,true),entry:view.getUint32(24,true),mapped_pages:view.getUint32(28,true),gate_base:view.getUint32(32,true),gate_count:view.getUint32(36,true),gates:Array.from({length:view.getUint32(36,true)},(_,index)=>[view.getUint32(48+index*8,true),view.getUint32(52+index*8,true)])};
  assert.equal(engine.receipt.entry,example.entry);assert.deepEqual(engine.receipt.gates,oracle.runtime.gates);
}
function load(example) {
  submit(image,'valid authored PE');commit(example);
  read(engine.receipt.image_base+0xb4,oracle.pe.preferred_base,'preferred ImageBase header remains immutable');
  oracle.pe.target_rvas.forEach((rva,index)=>read(engine.receipt.image_base+rva,example.patched_words[index],`Rust HIGHLOW operand${index}`));
  engine.receipt.gates.forEach(([pc],index)=>{read(engine.receipt.image_base+0x4050+index*4,pc,`Rust linked IAT slot${index}`);read(pc,0xb0f,`RX UD2 Gate${index}`);read(pc+4,0,`RX zero Gate tail${index}`);});
  read(engine.receipt.image_base+0x4054,0,'IAT zero terminator');
  readData(oracle.pe.initial_data_hex,'loaded initialized data');
}
function startup(example) {
  operation('Rust owns initial CPU, stack and private entry',()=>engine.api.start_loaded_image(oracle.runtime.stack_base,oracle.runtime.stack_pages),0,[[0,fromHex(example.startup_state_hex)],[56,fromHex(oracle.records.startup_exit_hex)]]);
}
function setup(example) {
  fresh(example.name,example.owner,oracle.runtime.pages);const initial=arena().slice(0,96);load(example);
  const receipt=engine.receipt;
  if(example.owner==='replacement') engine.caller=replacement([receipt.entry,receipt.entry+oracle.program.canary_offset,...receipt.gates.map(([pc])=>pc)],receipt.gates,'caller-canary-and-gate',1,oracle.transport.main_replacement_imports);
  else {
    engine.table=new WebAssembly.Table({element:'anyfunc',initial:8,maximum:8});
    engine.gate=resident(receipt.gates.map(([pc])=>pc),receipt.gates,'exit-gate',oracle.transport.resident_gate_imports);install(engine.gate,0);dispatcher();
    operation('resident leaves replacement generation zero',()=>engine.api.generation(),0);
  }
  assert.deepEqual(arena().subarray(0,96),initial,'input and metadata setup preserve initial context');startup(example);
  observe('startup',{state_hex:example.startup_state_hex,exit_hex:oracle.records.startup_exit_hex,helper_hex:oracle.records.helper_zero_hex});stack([0,0,0],'Rust zeroed stack');
  if(example.owner==='resident') {
    run(engine.dispatcher,8,'real initial NeedCode before caller admission',[[56,fromHex(oracle.records.startup_exit_hex)]]);
    observe('missing-entry',{state_hex:example.startup_state_hex,exit_hex:oracle.records.startup_exit_hex,helper_hex:oracle.records.helper_zero_hex});
    const before=arena(),pc=new DataView(before.buffer).getUint32(48,true);assert.equal(pc,receipt.entry);
    engine.caller=resident([pc,pc+oracle.program.canary_offset],[],'caller-and-canary',oracle.transport.resident_caller_imports);install(engine.caller,1);
    assert.deepEqual(arena().subarray(0,96),before.subarray(0,96));
  } else engine.gate=engine.caller;
}
function capture(example) {
  const args=engine.owner==='resident'?[engine.low,engine.high,engine.gate.low,engine.gate.high,2,1]:[engine.low,engine.high,engine.gate.generation,2,1];
  operation('capture Rust private named ExitProcess frame',()=>engine.api[engine.owner==='resident'?'capture_resident_call':'capture_call'](...args),0,[[transfer,fromHex(example.gate.frame_hex)]]);
}
function provider() { return engine.owner==='resident'?engine.api.complete_resident_windows_call(engine.low,engine.high,engine.gate.low,engine.gate.high,1):engine.api.complete_windows_call(engine.low,engine.high,engine.gate.generation,1); }
function retainedBytes() {
  refresh();for(const unit of engine.units)assert.deepEqual(engine.bytes.slice(unit.pointer,unit.pointer+unit.length),unit.bytes);
  if(engine.dispatcher){assert.deepEqual(engine.bytes.slice(engine.dispatcher.pointer,engine.dispatcher.pointer+engine.dispatcher.length),engine.dispatcher.bytes);assert.equal(engine.table.length,8);assert.equal(engine.table.get(0),engine.gate.run);assert.equal(engine.table.get(1),engine.caller.run);for(let index=2;index<8;index++)assert.equal(engine.table.get(index),null);}
}
const residentCallerNeedCodeHex='52334558020001002800000000000000030000000700000000000000000000000000000000000000';
for(const example of oracle.cases) {
  setup(example);
  for(const [index,checkpoint] of example.checkpoints.entries()) {
    const patches=[[0,fromHex(checkpoint.state_hex)],[56,fromHex(checkpoint.exit_hex)],[100,fromHex(checkpoint.helper_hex)]];
    if(engine.owner==='resident')patches.push([transfer,installation(engine.caller,1)]);
    run(engine.owner==='resident'?engine.dispatcher:engine.caller,checkpoint.budget,`load-only checkpoint${index}`,patches);
    observe(`load${index}`,{state_hex:checkpoint.state_hex,exit_hex:checkpoint.exit_hex,helper_hex:checkpoint.helper_hex});
    readData(checkpoint.data_hex,`load${index} complete source/output/neighbor bytes`);
  }
  if(engine.owner==='resident') {
    run(engine.caller,8,'current resident caller retires final7 and stops real Gate NeedCode',[[0,fromHex(example.gate.state_hex)],[56,fromHex(residentCallerNeedCodeHex)],[100,fromHex(example.gate.helper_hex)]]);
    observe('caller-gate-NeedCode',{state_hex:example.gate.state_hex,exit_hex:residentCallerNeedCodeHex,helper_hex:example.gate.helper_hex});
    run(engine.dispatcher,1,'remaining1 fuel enters receipt-installed Exit Gate',[[56,fromHex(example.gate.exit_hex)],[transfer,installation(engine.gate,0)]]);
  } else run(engine.caller,8,'same replacement caller final7 and real Gate',[[0,fromHex(example.gate.state_hex)],[56,fromHex(example.gate.exit_hex)],[100,fromHex(example.gate.helper_hex)]]);
  observe('gate',{state_hex:example.gate.state_hex,exit_hex:example.gate.exit_hex,helper_hex:example.gate.helper_hex});
  capture(example);observe('captured',{state_hex:example.gate.state_hex,exit_hex:example.gate.exit_hex,helper_hex:example.gate.helper_hex,frame_hex:example.gate.frame_hex});
  operation('private ExitProcess publishes checksum without caller return',provider,0,[[56,fromHex(example.terminal.exit_hex)]]);
  observe('terminal',{state_hex:example.terminal.state_hex,exit_hex:example.terminal.exit_hex,helper_hex:example.terminal.helper_hex,frame_hex:example.terminal.frame_hex});
  readData(example.terminal.data_hex,'terminal checksum and unexecuted supported canary');stack(example.gate.stack,'real PUSH/CALL argument and return words');retainedBytes();
  operation('retained-memory read8 allowed after process exit',()=>engine.api.read8(example.base+0x3000),0,[[100,record('R3MH',40,[0,0x11,0,0,0,1],2)]]);
  for(const target of [engine.caller,...(engine.dispatcher?[engine.gate,engine.dispatcher]:[])])run(target,0,'terminal latch precedes cancel/budget/malformed pointers',[],21,[0xffffffff,0xffffffff,0xffffffff]);
  operation('close positive retained context',()=>engine.api.close());run(engine.caller,0,'Closed before retained owner and malformed pointers',[],5,[0xffffffff,0xffffffff,0xffffffff]);
  operation('read8 after close keeps existing Closed classification',()=>engine.api.read8(example.base+0x3000),5);
}
function seed(spec,label) { hostInput(label,()=>{refresh().bytes.set(fromHex(spec.initial_state_hex),engine.base);engine.bytes.set(fromHex(spec.initial_exit_hex),engine.base+56);engine.bytes.set(fromHex(spec.initial_helper_hex),engine.base+100);}); }
function setupDirect(spec) {
  fresh(spec.name,spec.owner,oracle.runtime.direct_pages);
  operation('map authored microcode RWX for declared same-byte stale control',()=>engine.api.map(0x8000,1,7));upload(0x8000,fromHex(spec.code_hex),'copy exact finite authored microcode');
  for(const [address,permissions] of spec.maps)operation('map declared finite data page',()=>engine.api.map(address,1,permissions));
  for(const [address,hex] of spec.fills)upload(address,fromHex(hex),'initialize declared finite RAM before compilation');
  ramRanges(spec.before_ram,'initial finite operand/neighbor bytes');
  for(const [address,permissions] of spec.protect)operation('protect declared finite data',()=>engine.api.protect(address,1,permissions));
  const mixed=spec.symbol==='all_byte_destinations',helpers=oracle.transport[`${mixed?'mixed':'byte'}_${spec.owner}_imports`];
  const unit=spec.owner==='replacement'?replacement([0x8000],[],'finite',1,helpers):resident([0x8000],[],'finite',helpers);
  seed(spec,'single declared synthetic finite initial CPU/Exit/helper seed');return unit;
}
function success(spec,unit,budget,label) {
  run(unit,budget,label,[[0,fromHex(spec.success_state_hex)],[56,fromHex(spec.success_exit_hex)],[100,fromHex(spec.success_helper_hex)]]);
  observe('success',{state_hex:spec.success_state_hex,exit_hex:spec.success_exit_hex,helper_hex:spec.success_helper_hex});retainedBytes();
}
function unsignedLeb(value) { value>>>=0;const out=[];do{let byte=value&127;value>>>=7;if(value)byte|=128;out.push(byte);}while(value);return out; }
function wasmVector(rows) { return [...unsignedLeb(rows.length),...rows.flat()]; }
function wasmName(value) { const bytes=[...Buffer.from(value)];return [...unsignedLeb(bytes.length),...bytes]; }
function wasmSection(id,body) { return [id,...unsignedLeb(body.length),...body]; }
function helperInjectorBytes() {
  const types=[[3,0],[1,1]].map(([parameters,results])=>[0x60,...unsignedLeb(parameters),...Array(parameters).fill(0x7f),...unsignedLeb(results),...Array(results).fill(0x7f)]);
  const globals=Array.from({length:3},()=>[0x7f,1,0x41,0,0x0b]);
  const exports=[['configure',0],['read8',1]].map(([name,index])=>[...wasmName(name),0,...unsignedLeb(index)]);
  const configure=[0,0x20,0,0x24,0,0x20,1,0x24,1,0x20,2,0x24,2,0x0b],copy=[0,0x23,0,0x23,1,0x41,40,0xfc,0x0a,0,0,0x23,2,0x0b];
  return new Uint8Array([0,97,115,109,1,0,0,0,...wasmSection(1,wasmVector(types)),...wasmSection(2,wasmVector([[...wasmName('env'),...wasmName('memory'),2,0,1]])),...wasmSection(3,wasmVector([[0],[1]])),...wasmSection(6,wasmVector(globals)),...wasmSection(7,wasmVector(exports)),...wasmSection(10,wasmVector([configure,copy].map(body=>[...unsignedLeb(body.length),...body])))]);
}
const injectorBytes=helperInjectorBytes(),injectorArtifact=save('synthetic-read8-injector.wasm',injectorBytes);
assert.ok(WebAssembly.validate(injectorBytes));const injectorModule=new WebAssembly.Module(injectorBytes);
assert.deepEqual(WebAssembly.Module.imports(injectorModule),[{module:'env',name:'memory',kind:'memory'}]);assert.deepEqual(WebAssembly.Module.exports(injectorModule),['configure','read8'].map(name=>({name,kind:'function'})));
function syntheticProtocol(spec,unit) {
  const injector=new WebAssembly.Instance(injectorModule,{env:{memory:engine.memory}}).exports;assert.equal(injector.configure.length,3);assert.equal(injector.read8.length,1);
  const helpers=oracle.transport[`mixed_${spec.owner}_imports`],binding=Object.fromEntries(helpers.map(name=>[name,name==='read8'?injector.read8:engine.api[name]]));
  const instance=new WebAssembly.Instance(new WebAssembly.Module(unit.bytes),{env:{memory:engine.memory},ring3:binding}),target={run:instance.exports.run};assert.equal(target.run.length,4);
  syntheticBindings.push({instance:engine.ordinal,name:engine.name,owner:engine.owner,source_child:unit.filename,source_child_sha256:hash(unit.bytes),key:engine.key.toString(),id:unit.id?.toString(),generation:unit.generation,imports:helpers,injector:injectorArtifact,injector_sha256:hash(injectorBytes),claim:'synthetic strict Read1 packet/status validation; no guest RAM effect; actual engine guard and all other helpers'});
  for(const row of oracle.synthetic_protocol) {
    request(fromHex(row.input_helper_hex),`synthetic packet ${row.name}`);operation(`configure pureWasm read8 override ${row.name}`,()=>injector.configure(engine.base+100,engine.base+transfer,row.status),null);
    seed(spec,`negative protocol reset ${row.name}`);
    run(target,1,`synthetic strict Read1 ${row.name}`,[[0,fromHex(row.state_hex)],[56,fromHex(row.exit_hex)],[100,fromHex(row.input_helper_hex)]]);
    observe(`synthetic-${row.name}`,{state_hex:row.state_hex,exit_hex:row.exit_hex,helper_hex:row.input_helper_hex});syntheticObservations.push({instance:engine.ordinal,name:engine.name,owner:engine.owner,row:row.name,source_child:unit.filename});
  }
}
function guardCall(unit,low=engine.low,high=engine.high,ownerLow=unit.low,ownerHigh=unit.high,pointers=[engine.base,engine.base+56,engine.base+96]) {
  return engine.owner==='resident'?engine.api.guard_resident(low,high,ownerLow,ownerHigh,...pointers):engine.api.guard(low,high,ownerLow??unit.generation,...pointers);
}
function controls(spec,unit) {
  seed(spec,'declared negative guard-control initial context');ramRanges(spec.after_ram,'before key/full-owner controls');
  operation('wrong full key low precedes effects',()=>guardCall(unit,engine.low^1),3);operation('wrong full key high precedes effects',()=>guardCall(unit,engine.low,engine.high^1),3);
  operation('wrong full owner low',()=>guardCall(unit,engine.low,engine.high,engine.owner==='resident'?unit.low^1:0,unit.high),3);
  operation('wrong full owner high',()=>guardCall(unit,engine.low,engine.high,engine.owner==='resident'?unit.low:2,engine.owner==='resident'?unit.high^1:undefined),3);
  run(unit,1,'bad pointers before byte merge',[],1,[0xffffffff,0xffffffff,0xffffffff]);
  cancel(1);run(unit,0,'Cancel before zero-budget safepoint',[[56,fromHex(oracle.controls.cancel_exit_hex)]]);observe('guard-cancel',{state_hex:spec.initial_state_hex,exit_hex:oracle.controls.cancel_exit_hex});cancel(0);
  run(unit,0,'zero-budget before byte merge',[[56,fromHex(oracle.controls.budget_exit_hex)]]);observe('guard-budget',{state_hex:spec.initial_state_hex,exit_hex:oracle.controls.budget_exit_hex});
  operation('same code byte write invalidates retained snapshot',()=>engine.api.write8(0x8000,0x8a),0,[[100,record('R3MH',40,[0,0,0,0,0,1],3)]]);
  cancel(1);run(unit,0,'stale before Cancel/budget/invalid pointers',[],4,[0xffffffff,0xffffffff,0xffffffff]);cancel(0);
  ramRanges(spec.after_ram,'guard and stale retain complete finite RAM');operation('close finite guard context',()=>engine.api.close());
  run(unit,0,'Closed before stale and malformed pointers',[],5,[0xffffffff,0xffffffff,0xffffffff]);operation('Closed raw read8',()=>engine.api.read8(0x2000),5);
}
for(const spec of oracle.finite) {
  const unit=setupDirect(spec);
  if(spec.fault_state_hex) {
    run(unit,64,'LEA prefix retires then precise Read1 fault',[[0,fromHex(spec.fault_state_hex)],[56,fromHex(spec.fault_exit_hex)],[100,fromHex(spec.fault_helper_hex)]]);
    observe('fault',{state_hex:spec.fault_state_hex,exit_hex:spec.fault_exit_hex,helper_hex:spec.fault_helper_hex});
    cancel(1);run(unit,0,'fault Cancel before zero-budget preserves faulting CPU',[[56,fromHex(spec.pause_cancel_exit_hex)]]);observe('fault-cancel',{state_hex:spec.fault_state_hex,exit_hex:spec.pause_cancel_exit_hex,helper_hex:spec.fault_helper_hex});cancel(0);
    run(unit,0,'fault zero-budget preserves faulting CPU',[[56,fromHex(spec.pause_budget_exit_hex)]]);observe('fault-budget',{state_hex:spec.fault_state_hex,exit_hex:spec.pause_budget_exit_hex,helper_hex:spec.fault_helper_hex});
    if(spec.second_permission_exit_hex) {
      operation('Execute-only data still lacks Read1',()=>engine.api.protect(spec.target,1,4));run(unit,64,'Execute-only Read1 retry still precise',[[56,fromHex(spec.second_permission_exit_hex)],[100,fromHex(spec.fault_helper_hex)]]);
      observe('execute-only-fault',{state_hex:spec.fault_state_hex,exit_hex:spec.second_permission_exit_hex,helper_hex:spec.fault_helper_hex});
    }
    for(const [address,permissions] of spec.repair_maps)operation('map-only zero retry; no RAM fill',()=>engine.api.map(address,1,permissions));
    for(const [address,permissions] of spec.repair_protect)operation('protect-only Read1 retry',()=>engine.api.protect(address,1,permissions));
    success(spec,unit,64,'retry same current owner and fault CPU without CPU/code patch');ramRanges(spec.after_ram,'successful retry preserves declared zero or initialized RAM');
  } else {
    if(spec.checkpoint_state_hex) {
      run(unit,spec.checkpoint_budget,'all8 LoadByte checkpoint',[[0,fromHex(spec.checkpoint_state_hex)],[56,fromHex(spec.checkpoint_exit_hex)],[100,fromHex(spec.checkpoint_helper_hex)]]);
      observe('all8',{state_hex:spec.checkpoint_state_hex,exit_hex:spec.checkpoint_exit_hex,helper_hex:spec.checkpoint_helper_hex});
    }
    success(spec,unit,spec.resume_budget??64,'declared finite LoadByte and mixed old-helper continuation');ramRanges(spec.after_ram,'complete finite RAM unchanged except authored mixed Store1');
    if('absent_neighbor' in spec)operation('endpoint neighbor remains genuinely unmapped',()=>engine.api.read8(spec.absent_neighbor),0,[[100,record('R3MH',40,[1,0,1,spec.absent_neighbor,1,1],2)]]);
  }
  if(spec.symbol==='all_byte_destinations'){syntheticProtocol(spec,unit);ramRanges(spec.after_ram,'synthetic Read1 refusals cannot change RAM');controls(spec,unit);}
  else {retainedBytes();operation('close finite endpoint/alias/fault context',()=>engine.api.close());}
}
assert.equal(guestRetirements,164);assert.equal(nextInstance,14);assert.equal(identities.length,18);assert.equal(syntheticBindings.length,2);assert.equal(syntheticObservations.length,10);
const artifactFiles=readdirSync(outputDir).filter(name=>name!=='provenance.json').sort();assert.deepEqual(artifactFiles,Object.keys(artifacts).sort());for(const file of artifactFiles)assert.equal(hash(readFileSync(join(outputDir,file))),artifacts[file]);
const counts={instances:nextInstance,children:identities.length,operations:operations.length,host_inputs:hostInputs.length,observations:observations.length,ram_checks:ramChecks.length,stack_checks:stackChecks.length,guest_retired:guestRetirements,synthetic_bindings:syntheticBindings.length,synthetic_rows:syntheticObservations.length,artifacts:artifactFiles.length};
assert.deepEqual(counts,{instances:14,children:18,operations:908,host_inputs:134,observations:92,ram_checks:82,stack_checks:8,guest_retired:164,synthetic_bindings:2,synthetic_rows:10,artifacts:3134});
const provenance={engine_sha256:hash(engineBytes),oracle_sha256:hash(oracleBytes),program_sha256:hash(programBytes),text_sha256:hash(code),pe_sha256:hash(image),artifacts,sources,identities,operations,host_inputs:hostInputs,observations,ram_checks:ramChecks,stack_checks:stackChecks,synthetic_bindings:syntheticBindings,synthetic_observations:syntheticObservations,counts,command:[process.execPath,process.argv[1],enginePath,outputDir,root],assembly_command:['clang',...assemblyArgs],tools:{node:process.version,v8:process.versions.v8,clang:execFileSync('clang',['--version'],{encoding:'utf8'}).split('\n')[0]},claim:oracle.claim_ceiling};
writeFileSync(join(outputDir,'provenance.json'),JSON.stringify(provenance,null,2));console.log(JSON.stringify({status:'ok',engine_sha256:provenance.engine_sha256,oracle_sha256:provenance.oracle_sha256,counts,output:outputDir}));
