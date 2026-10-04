import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {readFileSync, readdirSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';

const [enginePath, outputDir, root] = process.argv.slice(2);
assert.ok(root, 'expected actual engine, output directory and repository root');
const fixtureRoot = join(root, 'engine/tests/fixtures/p2-byte-move');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const oracleBytes = readFileSync(join(fixtureRoot, 'oracle.json')), oracle = JSON.parse(oracleBytes);
const programBytes = readFileSync(join(fixtureRoot, 'program.S'));
assert.equal(hash(oracleBytes), 'b32d98ba147b9f70c89534aa99b48ff1ee0f191dc287d418c87a7fedab148b9b');
assert.equal(hash(programBytes), 'a2cb5ef91e8ef5184ad3baf80cbe5ac51a79aaecdfe7dc3263a91675b4922f05');
const size = 4236, transfer = 140, artifacts = {}, identities = [], operations = [], hostInputs = [], observations = [], ramChecks = [], stackChecks = [], refusalRows = [], exitedRows = [];
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
const inputReceiptPath=join(root,'target/r3-byte-move/input-build-freeze.json'), inputReceiptBytes=readFileSync(inputReceiptPath), inputReceipt=JSON.parse(inputReceiptBytes);
const inputReceiptSha='a0337a6e26ade7a7fbf842cadc97a0001d06bb89a2a50f485a85b04806b1b885';
assert.equal(hash(inputReceiptBytes),inputReceiptSha);assert.equal(inputReceipt.source_sha256,hash(programBytes));assert.equal(inputReceipt.oracle_sha256,hash(oracleBytes));
assert.deepEqual(inputReceipt.symbols,oracle.program.symbols);assert.deepEqual(inputReceipt.labels,oracle.program.labels);
for(const [file,sha] of Object.entries(inputReceipt.artifacts))assert.equal(hash(readFileSync(join(root,file))),sha);
for(const file of ['program.o','program.disassembly.txt','program.x86','linked-byte-move.exe'])save(file,readFileSync(join(root,'target/r3-byte-move/input-build',file)));
save('input-build-freeze.json',inputReceiptBytes);
const code=authoredText(readFileSync(join(outputDir,'program.o'))),image=readFileSync(join(outputDir,'linked-byte-move.exe'));
assert.deepEqual(code,readFileSync(join(outputDir,'program.x86')));assert.equal(hash(image),oracle.pe.sha256);assert.equal(image.length,oracle.pe.file_size);
assert.deepEqual(image.subarray(0xffc,0xffc+code.length),code);
oracle.pe.chunks.forEach((chunk,index)=>{const bytes=image.subarray(chunk.offset,chunk.offset+chunk.length);assert.equal(hash(bytes),chunk.sha256);save(`input-chunk${index}.bin`,bytes);});
save('program.S',programBytes);save('input-oracle.json',oracleBytes);save('run.mjs',readFileSync(import.meta.filename));
const productionPaths=execFileSync('rg',['--files','engine/src'],{cwd:root,encoding:'utf8'}).trim().split('\n').sort();
const sourcePaths=[...productionPaths,'Cargo.toml','engine/Cargo.toml','Cargo.lock','engine/tests/cpu_byte_move_wasm.rs','engine/tests/fixtures/p2-byte-move/program.S','engine/tests/fixtures/p2-byte-move/oracle.json','engine/tests/fixtures/p2-byte-move/run.mjs'];
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
  const ordinal = ++nextInstance, key = 0xa355cdef00000000n + BigInt(ordinal);
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
function childShape(bytes, unit, helpers, dispatch = false, pure = false) {
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
      assert.deepEqual(locals,dispatch?[[5,0x7f]]:pure?oracle.transport.pure_locals:oracle.transport.locals);
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
function instantiate(unit, helpers, pure = false) {
  refresh(); assert.ok(unit.pointer>0 && unit.length>8 && unit.pointer+unit.length<=engine.bytes.length); unit.bytes=engine.bytes.slice(unit.pointer,unit.pointer+unit.length);
  assert.ok(WebAssembly.validate(unit.bytes)); const shape=childShape(unit.bytes,unit,helpers,false,pure),module=new WebAssembly.Module(unit.bytes);
  const imports=WebAssembly.Module.imports(module);assert.deepEqual(imports,[{module:'env',name:'memory',kind:'memory'},...helpers.map(name=>({module:'ring3',name,kind:'function'}))]);
  assert.deepEqual(WebAssembly.Module.exports(module),[{name:'run',kind:'function'}]);
  unit.instance=new WebAssembly.Instance(module,{env:{memory:engine.memory},ring3:Object.fromEntries(helpers.map(name=>[name,engine.api[name]]))}); unit.run=unit.instance.exports.run; assert.equal(unit.run.length,4);
  unit.filename=save(`${engine.name}-${unit.label}.wasm`,unit.bytes); engine.units.push(unit);
  identities.push({instance:engine.ordinal,name:engine.name,owner:engine.owner,key:engine.key.toString(),label:unit.label,id:unit.id?.toString(),generation:unit.generation,pointer:unit.pointer,length:unit.length,entries:unit.entries,imports,...shape,module_sha256:hash(unit.bytes),artifact:unit.filename}); return unit;
}
function replacement(entries,gates,label,generation,helpers,pure=false) {
  entriesRequest(entries,gates);operation(`replacement ${label} compile preserves CPU and context`,()=>engine.api.compile_entries(entries.length,gates.length));
  operation(`replacement ${label} generation`,()=>engine.api.generation(),generation);
  const pointer=operation(`replacement ${label} module pointer`,()=>engine.api.module_ptr(),null)>>>0,length=operation(`replacement ${label} module length`,()=>engine.api.module_len(),null)>>>0;
  return instantiate({label,generation,pointer,length,entries},helpers,pure);
}
function resident(entries, gates, label, helpers,pure=false) {
  entriesRequest(entries,gates); let unit;
  operation(`resident compile ${label}`,()=>engine.api.compile_resident_entries(entries.length,gates.length),0,after=>{
    const view=new DataView(after.buffer,transfer,24); unit={low:view.getUint32(8,true),high:view.getUint32(12,true),pointer:view.getUint32(16,true),length:view.getUint32(20,true),entries,label};
    unit.id=BigInt(unit.low)|(BigInt(unit.high)<<32n); assert.notEqual(unit.id,0n); assert.ok(!engine.units.some(item=>item.id===unit.id)); return [[transfer,residentRecord(unit)]];
  });
  instantiate(unit,helpers,pure); entries.forEach(pc=>operation(`${label} returned full owner lookup`,()=>engine.api.find_resident(pc),0,[[transfer,residentRecord(unit)]])); return unit;
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
for(const example of oracle.cases) {
  setup(example);
  for(const [index,checkpoint] of example.checkpoints.entries()) {
    run(engine.caller,checkpoint.budget,`byte MOV checkpoint${index}`,[[0,fromHex(checkpoint.state_hex)],[56,fromHex(checkpoint.exit_hex)],[100,fromHex(checkpoint.helper_hex)]]);
    observe(`checkpoint${index}`,{state_hex:checkpoint.state_hex,exit_hex:checkpoint.exit_hex,helper_hex:checkpoint.helper_hex});
    readData(checkpoint.data_hex,`checkpoint${index} complete source/output/neighbor bytes`);
  }
  if(engine.owner==='resident') {
    run(engine.caller,oracle.runtime.resume_budget,'same resident caller retires final6 and stops real Gate NeedCode',[[0,fromHex(example.gate.state_hex)],[56,fromHex(example.resident_caller_exit_hex)],[100,fromHex(example.gate.helper_hex)]]);
    observe('caller-gate-NeedCode',{state_hex:example.gate.state_hex,exit_hex:example.resident_caller_exit_hex,helper_hex:example.gate.helper_hex});
    run(engine.dispatcher,1,'remaining1 fuel enters receipt-installed Exit Gate',[[56,fromHex(example.gate.exit_hex)],[transfer,installation(engine.gate,0)]]);
  } else run(engine.caller,oracle.runtime.resume_budget,'same replacement caller final6 and real Gate',[[0,fromHex(example.gate.state_hex)],[56,fromHex(example.gate.exit_hex)],[100,fromHex(example.gate.helper_hex)]]);
  observe('gate',{state_hex:example.gate.state_hex,exit_hex:example.gate.exit_hex,helper_hex:example.gate.helper_hex});
  capture(example);observe('captured',{state_hex:example.gate.state_hex,exit_hex:example.gate.exit_hex,helper_hex:example.gate.helper_hex,frame_hex:example.gate.frame_hex});
  operation('private ExitProcess publishes checksum without caller return',provider,0,[[56,fromHex(example.terminal.exit_hex)]]);
  observe('terminal',{state_hex:example.terminal.state_hex,exit_hex:example.terminal.exit_hex,helper_hex:example.terminal.helper_hex,frame_hex:example.terminal.frame_hex});
  readData(example.terminal.data_hex,'terminal checksum and unexecuted supported canary');stack(example.gate.stack_words,'real PUSH/CALL argument and return words');retainedBytes();
  run(engine.caller,0,'terminal latch precedes zero-budget/malformed pointers and byte effects',[],21,[0xffffffff,0xffffffff,0xffffffff]);
  exitedRows.push({instance:engine.ordinal,name:engine.name,owner:engine.owner,status:21,operation:operations.length-1});
  operation('close positive retained context',()=>engine.api.close());
}
function seed(spec,label) {
  hostInput(label,()=>{refresh().bytes.set(fromHex(spec.initial_state_hex),engine.base);engine.bytes.set(fromHex(spec.initial_exit_hex),engine.base+56);engine.bytes.set(fromHex(spec.initial_helper_hex),engine.base+100);});
}
function guardCall(unit,low=engine.low,high=engine.high,ownerLow=unit.low,ownerHigh=unit.high) {
  const pointers=[engine.base,engine.base+56,engine.base+96];
  return engine.owner==='resident'?engine.api.guard_resident(low,high,ownerLow,ownerHigh,...pointers):engine.api.guard(low,high,ownerLow??unit.generation,...pointers);
}
function refusal(row,action,status=0,patches=[]) {
  const index=operations.length;operation(`finite refusal ${row}`,action,status,patches);
  refusalRows.push({instance:engine.ordinal,name:engine.name,owner:engine.owner,row,status,operation:index});
}
for(const spec of oracle.finite) {
  fresh(spec.name,spec.owner,oracle.runtime.pages);
  operation('map authored pure byte MOV RWX for same-byte stale control',()=>engine.api.map(0x1000,1,7));upload(0x1000,fromHex(spec.code_hex),'copy exact finite authored pure code');
  operation('map finite declared probe RAM',()=>engine.api.map(spec.probe_address,1,3));upload(spec.probe_address,fromHex(spec.probe_hex),'initialize finite probe RAM before compilation');
  ramRanges([[spec.probe_address,spec.probe_hex]],'before pure MOV declared RAM');
  const helpers=oracle.transport[`pure_${spec.owner}_imports`];
  const unit=spec.owner==='replacement'?replacement([0x1000],[],'pure-byte-moves',1,helpers,true):resident([0x1000],[],'pure-byte-moves',helpers,true);
  assert.deepEqual(unit.bytes,engine.bytes.slice(unit.pointer,unit.pointer+unit.length));
  seed(spec,'single synthetic finite initial CPU/Exit/opaque helper seed');
  for(const expected of spec.trace) {
    const phase=`mov${String(expected.instruction).padStart(2,'0')}`;
    run(unit,1,`pure ${phase} exact byte/parent/flags`,[[0,fromHex(expected.state_hex)],[56,fromHex(expected.exit_hex)]]);
    observe(phase,{state_hex:expected.state_hex,exit_hex:expected.exit_hex,helper_hex:expected.helper_hex});
    retainedBytes();
  }
  run(unit,spec.jump.budget,'supported final JMP reaches excluded PC with fuel',[[0,fromHex(spec.jump.state_hex)],[56,fromHex(spec.jump.exit_hex)]]);
  observe('jump',{state_hex:spec.jump.state_hex,exit_hex:spec.jump.exit_hex,helper_hex:spec.jump.helper_hex});retainedBytes();
  ramRanges([[spec.probe_address,spec.probe_hex]],'after pure MOV unchanged declared RAM');
  seed(spec,'separate negative owner/control initial CPU/Exit/opaque helper seed');
  refusal('wrong_key_low',()=>guardCall(unit,engine.low^1),3);
  refusal('wrong_key_high',()=>guardCall(unit,engine.low,engine.high^1),3);
  refusal('wrong_owner_low',()=>guardCall(unit,engine.low,engine.high,engine.owner==='resident'?unit.low^1:0,unit.high),3);
  if(engine.owner==='resident')refusal('wrong_owner_high',()=>guardCall(unit,engine.low,engine.high,unit.low,unit.high^1),3);
  refusal('bad_state_pointer',()=>unit.run(0xffffffff,engine.base+56,1,engine.base+96),1);
  cancel(1);refusal('Cancel_zero_budget',()=>unit.run(engine.base,engine.base+56,0,engine.base+96),0,[[56,fromHex(oracle.controls.cancel_exit_hex)]]);
  observe('guard-cancel',{state_hex:spec.initial_state_hex,exit_hex:oracle.controls.cancel_exit_hex,helper_hex:spec.initial_helper_hex});cancel(0);
  refusal('zero_budget',()=>unit.run(engine.base,engine.base+56,0,engine.base+96),0,[[56,fromHex(oracle.controls.budget_exit_hex)]]);
  observe('guard-budget',{state_hex:spec.initial_state_hex,exit_hex:oracle.controls.budget_exit_hex,helper_hex:spec.initial_helper_hex});
  operation('same code byte write invalidates retained snapshot',()=>engine.api.write8(0x1000,0x8a),0,[[100,fromHex(oracle.controls.host_samebyte_write_helper_hex)]]);
  cancel(1);refusal('same_byte_host_code_stale',()=>unit.run(0xffffffff,0xffffffff,0,0xffffffff),4);cancel(0);
  retainedBytes();operation('close finite pure owner context',()=>engine.api.close());
  refusal('Closed',()=>unit.run(0xffffffff,0xffffffff,0,0xffffffff),5);
  assert.deepEqual(refusalRows.filter(row=>row.instance===engine.ordinal).map(row=>row.row),oracle.controls[`${engine.owner}_rows`]);
}
assert.equal(guestRetirements,oracle.counts.total_retired);assert.equal(nextInstance,6);assert.equal(identities.length,10);assert.equal(refusalRows.length,17);assert.equal(exitedRows.length,4);
const artifactFiles=readdirSync(outputDir).filter(name=>name!=='provenance.json').sort();assert.deepEqual(artifactFiles,Object.keys(artifacts).sort());
for(const file of artifactFiles)assert.equal(hash(readFileSync(join(outputDir,file))),artifacts[file]);
const counts={instances:nextInstance,children:identities.length,operations:operations.length,host_inputs:hostInputs.length,observations:observations.length,ram_checks:ramChecks.length,stack_checks:stackChecks.length,guest_retired:guestRetirements,finite_refusal_rows:refusalRows.length,terminal_refusal_rows:exitedRows.length,artifacts:artifactFiles.length};
const expectedCounts={instances:6,children:10,operations:497,host_inputs:62,observations:118,ram_checks:24,stack_checks:8,guest_retired:230,finite_refusal_rows:17,terminal_refusal_rows:4,artifacts:1767};
assert.deepEqual(counts,expectedCounts);
const provenance={engine_sha256:hash(engineBytes),oracle_sha256:hash(oracleBytes),program_sha256:hash(programBytes),text_sha256:hash(code),pe_sha256:hash(image),input_build_receipt_sha256:inputReceiptSha,artifacts,sources,identities,operations,host_inputs:hostInputs,observations,ram_checks:ramChecks,stack_checks:stackChecks,live_assertions:['pure declared RAM before/after40 and final JMP; no helper reads between MOV checkpoints','retained generated bytes and every current-owner guard; pure guard-only imports/no guest-memory helper','resident table8 and installed slot0/1 identities'],refusal_rows:refusalRows,exited_rows:exitedRows,counts,command:[process.execPath,process.argv[1],enginePath,outputDir,root],assembly_command:inputReceipt.commands[0].command,assembly_input_only:true,tools:{node:process.version,v8:process.versions.v8,clang:inputReceipt.tools.clang.split('\n')[0],llvm:inputReceipt.tools.llvm.split('\n')[0]},claim:oracle.claim_ceiling};
writeFileSync(join(outputDir,'provenance.json'),JSON.stringify(provenance,null,2));
console.log(JSON.stringify({status:'ok',engine_sha256:provenance.engine_sha256,oracle_sha256:provenance.oracle_sha256,counts,output:outputDir}));
