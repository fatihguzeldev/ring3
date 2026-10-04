import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {readFileSync, readdirSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';

const [enginePath, outputDir, root] = process.argv.slice(2);
assert.ok(root, 'expected actual engine, output directory and repository root');
const fixtureRoot = join(root, 'engine/tests/fixtures/p2-byte-store');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const oracleBytes = readFileSync(join(fixtureRoot, 'oracle.json')), oracle = JSON.parse(oracleBytes);
const programBytes = readFileSync(join(fixtureRoot, 'program.S'));
assert.equal(hash(oracleBytes), '0f72ce325dbea687e420944952e81b15326292299204a7567b89e56094ca3100');
assert.equal(hash(programBytes), '8b4d74163d26fb8f95e56d5252970bb45b5f4bf03724ce7b51d26611cbb17f12');
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
// independent authored file; rust alone parses, relocates and links it.
function authoredPe(code, first, second) {
  const bytes = Buffer.alloc(12800), view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  bytes.write('MZ'); view.setUint32(0x3c, 0x80, true); bytes.write('PE\0\0', 0x80);
  view.setUint16(0x84, 0x14c, true); view.setUint16(0x86, 4, true); view.setUint16(0x94, 224, true); view.setUint16(0x96, 0x102, true);
  const opt = 0x98; view.setUint16(opt, 0x10b, true);
  for (const [at, value] of [[4,0x400],[8,0x2000],[16,0x11fc],[20,0x1000],[24,0x3000],[28,0x400000],[32,4096],[36,512],[56,0x7000],[60,0x400],[72,0x100000],[76,4096],[80,0x100000],[84,4096],[92,16],[104,0x4bf0],[108,40],[136,0x61fc],[140,12],[192,0x4050],[196,8]]) view.setUint32(opt + at, value, true);
  for (const [at, value] of [[40,4],[48,4],[68,3],[70,0x100]]) view.setUint16(opt + at, value, true);
  [['.text',0x400,0x1000,0xe00,0x400,0x60000020],['.data',24,0x3000,0x1200,0x200,0xc0000040],['.imports',0x1a00,0x4000,0x1400,0x1a00,0x40000040],['.fixups',0x400,0x6000,0x2e00,0x400,0x40000040]].forEach(([name, virtual, rva, raw, rawSize, flags], index) => {
    const at = 0x178 + index * 40; bytes.write(name, at);
    for (const [field, value] of [[8,virtual],[12,rva],[16,rawSize],[20,raw],[36,flags]]) view.setUint32(at + field, value, true);
  });
  bytes.fill(0xcc, 0xe00, 0x1200); bytes.set(code, 0xffc); bytes.set(words([first,second,0,0,0,0]), 0x1200);
  bytes.set(words([0x4e40,0,0,0x4e60,0x4050]), 0x1ff0);
  for (const at of [0x1450,0x2240]) bytes.set(words([0x4e70,0]), at);
  bytes.write('KeRnEl32.dLl\0', 0x2260); view.setUint16(0x2270, 0xbeef, true); bytes.write('ExitProcess\0', 0x2272);
  bytes.set(fromHex(oracle.pe.relocation_hex), 0x2ffc); return bytes;
}
const objectPath = join(outputDir, 'program.o'), assemblyArgs = ['-target','i386-unknown-linux-gnu','-c',join(fixtureRoot, 'program.S'),'-o',objectPath];
execFileSync('clang', assemblyArgs, {stdio: ['ignore','pipe','pipe']});
save('program.disassembly.txt', execFileSync('xcrun', ['llvm-objdump','-d','--x86-asm-syntax=intel',objectPath]));
artifacts['program.o'] = hash(readFileSync(objectPath));
const code = authoredText(readFileSync(objectPath)), images = {};
for (const spec of oracle.pe.images) {
  const bytes = authoredPe(code, spec.first, spec.second); assert.equal(hash(bytes), spec.sha256); assert.equal(bytes.subarray(0x1200,0x1218).toString('hex'), spec.initial_data_hex); images[spec.name] = bytes;
  save(`image-${spec.name}.exe`, bytes);
  spec.chunks.forEach((chunk, index) => { const bytes = images[spec.name].subarray(chunk.offset,chunk.offset + chunk.length); assert.equal(hash(bytes), chunk.sha256); save(`image-${spec.name}-chunk${index}.bin`,bytes); });
}
save('program.x86', code); save('program.S', programBytes); save('input-oracle.json', oracleBytes); save('run.mjs', readFileSync(import.meta.filename));
const productionPaths = execFileSync('rg', ['--files','engine/src'], {cwd: root, encoding: 'utf8'}).trim().split('\n').sort();
const sourcePaths = [...productionPaths,'Cargo.toml','engine/Cargo.toml','Cargo.lock','engine/tests/cpu_byte_store_wasm.rs','engine/tests/fixtures/p2-byte-store/program.S','engine/tests/fixtures/p2-byte-store/oracle.json','engine/tests/fixtures/p2-byte-store/run.mjs'];
assert.equal(sourcePaths.length,89);
const sources = Object.fromEntries(sourcePaths.map(file => [file, hash(readFileSync(join(root,file)))])); save('source-sha256.json', JSON.stringify(sources,null,2));
const engineBytes = readFileSync(enginePath); save('engine.wasm', engineBytes); assert.ok(WebAssembly.validate(engineBytes));
const engineModule = new WebAssembly.Module(engineBytes); assert.deepEqual(WebAssembly.Module.imports(engineModule), []);
assert.equal(WebAssembly.Module.exports(engineModule).length, oracle.transport.engine_total_exports);
assert.equal(WebAssembly.Module.exports(engineModule).filter(row => row.kind === 'function').length, oracle.transport.engine_function_exports);
const arities = {open:3,close:0,arena_ptr:0,map:3,protect:3,upload:2,begin_image_input:1,append_image_input:2,load_pe32_linked_v2_input_at:2,start_loaded_image:2,compile_entries:2,compile_resident_entries:2,generation:0,module_ptr:0,module_len:0,find_resident:1,acknowledge_resident_installation:5,find_installed_resident:3,dispatcher_module:2,guard:6,guard_resident:7,guard_dispatch_entry:5,read32:1,store32:2,store_resident32:6,write8:2,store8:2,store_resident8:6,read8:1,capture_call:5,capture_resident_call:6,complete_windows_call:4,complete_resident_windows_call:5};
let engine, nextInstance = 0, guestRetirements = 0;
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
  const ordinal = ++nextInstance, key = 0xa352cdef00000000n + BigInt(ordinal);
  engine = {name,owner,ordinal,api,key,low:Number(key & 0xffffffffn),high:Number(key >> 32n),memory:instance.exports.memory,units:[]};
  assert.ok(engine.memory instanceof WebAssembly.Memory); assert.equal(api.open(pages,engine.low,engine.high),0); engine.base=api.arena_ptr()>>>0; refresh();
  assert.ok(engine.base>0 && engine.base+size<=engine.bytes.length);
  const initial = new Uint8Array(size); initial.set(fromHex(oracle.records.initial_state_hex)); initial.set(fromHex(oracle.records.initial_exit_hex),56); initial.set(fromHex(oracle.records.helper_zero_hex),100); assert.deepEqual(arena(),initial);
  save(`${name}-initial-arena.bin`,initial);
  hostInput('explicit diagnostic and Transfer sentinel; CPU untouched', () => refresh().bytes.fill(0xa5,engine.base+100,engine.base+size));
}
function upload(address, bytes, label) { request(bytes,`${label}/owned bytes`); operation(label,()=>engine.api.upload(address,bytes.length)); }
function read(address, value, label) { operation(label,()=>engine.api.read32(address),0,[[100,helper(value)]]); return refresh().view.getUint32(engine.base+120,true); }
function readData(expectedHex, label) {
  const bytes = fromHex(expectedHex), view = new DataView(bytes.buffer); assert.equal(bytes.length,24);
  const values=Array.from({length:6},(_,index)=>read(engine.receipt.image_base+oracle.runtime.data_rva+index*4,view.getUint32(index*4,true),`${label}/word${index}`));
  ramChecks.push({instance:engine.ordinal,name:engine.name,label,address:engine.receipt.image_base+oracle.runtime.data_rva,bytes:24,actual_hex:Buffer.from(words(values)).toString('hex'),expected_hex:expectedHex}); return values;
}
function stack(values, label) { const actual=oracle.runtime.stack_addresses.map((address,index)=>read(address,values[index],`${label}/word${index}`)); stackChecks.push({instance:engine.ordinal,name:engine.name,label,addresses:oracle.runtime.stack_addresses,actual,expected:values}); return actual; }
function ramWords(values, label) {
  const actual = values.map(([address,value],index) => read(address,value,`${label}/word${index}`));
  ramChecks.push({instance:engine.ordinal,name:engine.name,label,addresses:values.map(([address])=>address),actual,expected:values.map(([,value])=>value)});
}
function entriesRequest(entries, gates = []) { request(words([...entries,...gates.flat()]),'explicit authored/canonical entry and receipt Gate compiler input'); }
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
function replacement(entries, gates, helpers) {
  entriesRequest(entries,gates); operation('replacement compile preserves context',()=>engine.api.compile_entries(entries.length,gates.length));
  const generation=operation('replacement generation',()=>engine.api.generation(),1);
  const pointer=operation('replacement pointer',()=>engine.api.module_ptr(),null)>>>0,length=operation('replacement length',()=>engine.api.module_len(),null)>>>0;
  return instantiate({label:'replacement',generation,pointer,length,entries},helpers);
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
function run(target, budget, label, patches = [], status = 0, pointers = [engine.base,engine.base+56,engine.base+96]) {
  operation(label,()=>target.run(pointers[0],pointers[1],budget,pointers[2]),status,patches);
  if (status===0) guestRetirements+=refresh().view.getUint32(engine.base+76,true);
}
function observe(phase, expected) {
  const bytes=arena(), fields={state_hex:Buffer.from(bytes.subarray(0,56)).toString('hex'),exit_hex:Buffer.from(bytes.subarray(56,96)).toString('hex'),helper_hex:Buffer.from(bytes.subarray(100,140)).toString('hex')};
  if (['captured','terminal'].includes(phase)) fields.frame_hex=Buffer.from(bytes.subarray(transfer,transfer+112)).toString('hex');
  const expectedRecords=expected.unchanged?{unchanged:true}:{};
  for (const [key,value] of Object.entries(fields)) if (expected[key]!==undefined) { assert.equal(value,expected[key],`${engine.name}/${phase}/${key}`); expectedRecords[key]=expected[key]; }
  observations.push({instance:engine.ordinal,name:engine.name,owner:engine.owner,phase,...fields,expected:expectedRecords,arena:save(`${engine.name}-${phase}-observation.bin`,bytes)});
}
function retainedBytes() {
  refresh(); for (const unit of engine.units) { assert.deepEqual(engine.bytes.slice(unit.pointer,unit.pointer+unit.length),unit.bytes); if (unit.slot!==undefined) assert.equal(engine.table.get(unit.slot),unit.run); }
  if (engine.dispatcher) assert.deepEqual(engine.bytes.slice(engine.dispatcher.pointer,engine.dispatcher.pointer+engine.dispatcher.length),engine.dispatcher.bytes);
}
function load(example) {
  const image=images[example.data], input=oracle.pe.images.find(item=>item.name===example.data); assert.equal(hash(image),example.image_sha256);
  operation('begin copied complete PE input',()=>engine.api.begin_image_input(image.length));
  for (const [index,chunk] of input.chunks.entries()) {
    const bytes=image.subarray(chunk.offset,chunk.offset+chunk.length); assert.equal(hash(bytes),chunk.sha256); request(bytes,`input chunk${index} offset${chunk.offset}`);
    operation(`append copied chunk${index}`,()=>engine.api.append_image_input(chunk.offset,chunk.length));
    hostInput('submitted Transfer is overwritten after copy',()=>refresh().bytes.fill(0xa5,engine.base+transfer,engine.base+size));
  }
  operation('Rust linked-v2 input commit',()=>engine.api.load_pe32_linked_v2_input_at(example.base,oracle.runtime.gate_base),0,[[transfer,fromHex(example.receipt_hex)]]);
  const receiptBytes=arena().slice(transfer,transfer+72),view=new DataView(receiptBytes.buffer); save(`${engine.name}-linked-receipt.bin`,receiptBytes);
  engine.receipt={image_base:view.getUint32(16,true),entry:view.getUint32(24,true),gate_count:view.getUint32(36,true),gates:Array.from({length:view.getUint32(36,true)},(_,index)=>[view.getUint32(48+index*8,true),view.getUint32(52+index*8,true)])};
  assert.equal(engine.receipt.entry,example.entry); assert.deepEqual(engine.receipt.gates,oracle.runtime.gates);
  read(engine.receipt.image_base+0xb4,oracle.pe.preferred_base,'header retains preferred ImageBase');
  oracle.pe.target_rvas.forEach((rva,index)=>read(engine.receipt.image_base+rva,example.patched_words[index],`Rust HIGHLOW operand${index}`));
  read(engine.receipt.image_base+0x4050,engine.receipt.gates[0][0],'Rust IAT ExitProcess'); read(engine.receipt.image_base+0x4054,0,'IAT zero terminator');
  read(engine.receipt.gates[0][0],0xb0f,'owned RX ExitProcess stub'); read(engine.receipt.gates[0][0]+4,0,'owned zero stub tail'); readData(input.initial_data_hex,'copied initialized data');
}
function setupPositive(example) {
  fresh(example.name,example.owner,oracle.runtime.pages); const initial=arena().slice(0,96); load(example);
  const receipt=engine.receipt, canary=receipt.entry+oracle.program.canary_offset;
  if (example.owner==='replacement') engine.primary=engine.gate=replacement([...oracle.program.main_offsets.map(offset=>receipt.entry+offset),canary,...receipt.gates.map(([pc])=>pc)],receipt.gates,oracle.transport.main_replacement_imports);
  else {
    engine.table=new WebAssembly.Table({element:'anyfunc',initial:8,maximum:8});
    engine.gate=resident(receipt.gates.map(([pc])=>pc),receipt.gates,'gate',oracle.transport.resident_gate_imports); install(engine.gate,0);
    engine.canary=resident([canary],[],'precompiled-canary',oracle.transport.canary_resident_imports); install(engine.canary,1); dispatcher(); engine.primary=engine.dispatcher;
    operation('resident replacement generation remains zero',()=>engine.api.generation(),0);
  }
  assert.deepEqual(arena().subarray(0,96),initial,'all loading/metadata preserve initial context');
  operation('Rust owns first positive CPU/stack/entry',()=>engine.api.start_loaded_image(oracle.runtime.stack_base,oracle.runtime.stack_pages),0,[[0,fromHex(example.startup_state_hex)],[56,fromHex(oracle.records.startup_exit_hex)]]);
  stack(oracle.runtime.stack_initial,'fresh Rust zero stack'); read(engine.receipt.image_base+oracle.runtime.data_rva+20,0,'fresh zero canary'); observe('startup',{state_hex:example.startup_state_hex,exit_hex:oracle.records.startup_exit_hex,helper_hex:oracle.records.helper_zero_hex});
}
function canonicalCaller(slot, helpers) {
  const before=arena(); assert.equal(new DataView(before.buffer).getUint32(72,true),3,'actual published NeedCode'); const pc=new DataView(before.buffer).getUint32(48,true);
  const unit=resident([pc],[],`canonical-caller-${slot-2}`,helpers); install(unit,slot); assert.deepEqual(arena().subarray(0,96),before.subarray(0,96)); return unit;
}
function capture(example) {
  if (engine.owner==='resident') {
    assert.deepEqual(arena().subarray(transfer,transfer+32),installation(engine.gate,0));
    operation('capture real resident ExitProcess frame',()=>engine.api.capture_resident_call(engine.low,engine.high,engine.gate.low,engine.gate.high,2,1),0,[[transfer,fromHex(example.api.frame_hex)]]);
  } else operation('capture real replacement ExitProcess frame',()=>engine.api.capture_call(engine.low,engine.high,engine.gate.generation,2,1),0,[[transfer,fromHex(example.api.frame_hex)]]);
}
function provider() { return engine.owner==='resident'?engine.api.complete_resident_windows_call(engine.low,engine.high,engine.gate.low,engine.gate.high,1):engine.api.complete_windows_call(engine.low,engine.high,engine.gate.generation,1); }
for (const example of oracle.cases) {
  setupPositive(example);
  if (example.owner === 'resident') {
    run(engine.primary,64,'real startup entry missing from installed resident table',[[56,fromHex(oracle.records.startup_exit_hex)]]);
    observe('missing-entry',{state_hex:example.startup_state_hex,exit_hex:oracle.records.startup_exit_hex,helper_hex:oracle.records.helper_zero_hex});
    engine.caller=canonicalCaller(2,oracle.transport.resident_caller_imports);
  }
  const checkpointPatches=[[0,fromHex(example.checkpoint.state_hex)],[56,fromHex(example.checkpoint.exit_hex)],[100,fromHex(example.checkpoint.helper_hex)]];
  if (example.owner==='resident') checkpointPatches.push([transfer,installation(engine.caller,2)]);
  run(engine.primary,oracle.runtime.checkpoint_budget,'real budget9 inside current caller after six byte stores',checkpointPatches);
  observe('checkpoint',example.checkpoint);
  const apiPatches=[[0,fromHex(example.api.state_hex)],[56,fromHex(example.api.exit_hex)],[100,fromHex(example.api.helper_hex)]];
  if (example.owner==='resident') apiPatches.push([transfer,installation(engine.gate,0)]);
  run(engine.primary,oracle.runtime.resume_budget,'resume16 same current caller through checksum and preinstalled Gate',apiPatches);
  observe('api',example.api);
  assert.equal(new DataView(fromHex(example.checkpoint.exit_hex).buffer).getUint32(20,true)+refresh().view.getUint32(engine.base+76,true),15);
  readData(example.terminal.data_hex,'guest final buffer/checksum and zero successor canary'); stack(example.api.stack,'real PUSH/CALL stack'); capture(example);
  const diagnosticHelper=Buffer.from(helper(example.checksum)).toString('hex');
  observe('captured',{state_hex:example.api.state_hex,exit_hex:example.api.exit_hex,helper_hex:diagnosticHelper,frame_hex:example.api.frame_hex});
  operation('private named ExitProcess publishes terminal without guest return',provider,0,[[56,fromHex(example.terminal.exit_hex)]]);
  observe('terminal',{...example.terminal,helper_hex:diagnosticHelper,frame_hex:example.api.frame_hex});
  stack(example.api.stack,'terminal stack unchanged'); readData(example.terminal.data_hex,'terminal canary remains zero'); retainedBytes();
  hostInput('explicit negative postterminal supported-canary CPU/Exit restore',()=>{refresh().bytes.set(fromHex(example.canary_restore_state_hex),engine.base);engine.bytes.set(fromHex(oracle.records.restore_exit_hex),engine.base+56);});
  for (const target of [...engine.units,...(engine.dispatcher?[engine.dispatcher]:[])]) {
    run(target,64,'private terminal rejects retained supported canary',[],21); cancel(1);
    run(target,0,'terminal precedes Cancel/zero budget/malformed pointers',[],21,[0xffffffff,0xffffffff,0xffffffff]); cancel(0);
  }
  readData(example.terminal.data_hex,'retained terminal owners preserve all data'); retainedBytes();
  operation('close positive context',()=>engine.api.close()); run(engine.units[0],0,'Closed precedes retained owner and malformed pointers',[],5,[0xffffffff,0xffffffff,0xffffffff]);
}
function setupDirect(spec, executablePermissions=5) {
  fresh(spec.name,spec.owner,6);
  operation('map authored microcode',()=>engine.api.map(0x8000,1,3)); upload(0x8000,fromHex(spec.code_hex),'copy authored microcode');
  operation('protect authored executable code',()=>engine.api.protect(0x8000,1,executablePermissions));
  for (const [address,permissions] of spec.maps) operation('map authored data page',()=>engine.api.map(address,1,permissions));
  for (const [address,hex] of spec.fills) upload(address,fromHex(hex),'initialize authored data sentinel before compile');
  ramWords(spec.before_words||spec.fault_words||[],'initial operand and neighbor sentinels');
  for (const [address,permissions] of spec.protect) operation('set authored Write-only or fault permission',()=>engine.api.protect(address,1,permissions));
  const unit=spec.owner==='replacement'?replacement([0x8000],[],oracle.transport.micro_replacement_imports):resident([0x8000],[],'direct-micro',oracle.transport.micro_resident_imports);
  hostInput('one controlled initial micro CPU/Exit/opaque helper seed',()=>{refresh().bytes.set(fromHex(spec.initial_state_hex),engine.base);engine.bytes.set(fromHex(spec.initial_exit_hex),engine.base+56);engine.bytes.set(fromHex(spec.initial_helper_hex),engine.base+100);});
  return unit;
}
function success(spec,unit,label) {
  run(unit,64,label,[[0,fromHex(spec.success_state_hex)],[56,fromHex(spec.success_exit_hex)],[100,fromHex(spec.success_helper_hex)]]);
  observe('success',{state_hex:spec.success_state_hex,exit_hex:spec.success_exit_hex,helper_hex:spec.success_helper_hex});
  retainedBytes();
}
function unsignedLeb(value) {
  value>>>=0;const out=[];do{let byte=value&127;value>>>=7;if(value)byte|=128;out.push(byte);}while(value);return out;
}
function wasmVector(rows) { return [...unsignedLeb(rows.length),...rows.flat()]; }
function wasmName(value) { const bytes=[...Buffer.from(value)];return [...unsignedLeb(bytes.length),...bytes]; }
function wasmSection(id,body) { return [id,...unsignedLeb(body.length),...body]; }
function helperInjectorBytes() {
  const types=[[3,0],[2,1],[6,1]].map(([parameters,results])=>[0x60,...unsignedLeb(parameters),...Array(parameters).fill(0x7f),...unsignedLeb(results),...Array(results).fill(0x7f)]);
  const globals=Array.from({length:3},()=>[0x7f,1,0x41,0,0x0b]);
  const exports=[['configure',0],['store8',1],['store_resident8',2]].map(([name,index])=>[...wasmName(name),0,...unsignedLeb(index)]);
  const configure=[0,0x20,0,0x24,0,0x20,1,0x24,1,0x20,2,0x24,2,0x0b];
  const copy=[0,0x23,0,0x23,1,0x41,40,0xfc,0x0a,0,0,0x23,2,0x0b];
  const bodies=[configure,copy,copy].map(body=>[...unsignedLeb(body.length),...body]);
  return new Uint8Array([0,97,115,109,1,0,0,0,...wasmSection(1,wasmVector(types)),...wasmSection(2,wasmVector([[...wasmName('env'),...wasmName('memory'),2,0,1]])),...wasmSection(3,wasmVector([[0],[1],[2]])),...wasmSection(6,wasmVector(globals)),...wasmSection(7,wasmVector(exports)),...wasmSection(10,wasmVector(bodies))]);
}
const injectorBytes=helperInjectorBytes(),injectorArtifact=save('synthetic-helper-injector.wasm',injectorBytes);
assert.ok(WebAssembly.validate(injectorBytes)); const injectorModule=new WebAssembly.Module(injectorBytes);
assert.deepEqual(WebAssembly.Module.imports(injectorModule),[{module:'env',name:'memory',kind:'memory'}]);
assert.deepEqual(WebAssembly.Module.exports(injectorModule),['configure','store8','store_resident8'].map(name=>({name,kind:'function'})));
function syntheticProtocol(spec,unit) {
  const injector=new WebAssembly.Instance(injectorModule,{env:{memory:engine.memory}}).exports;
  assert.equal(injector.configure.length,3); assert.equal(injector.store8.length,2); assert.equal(injector.store_resident8.length,6);
  const helpers=spec.owner==='replacement'?oracle.transport.micro_replacement_imports:oracle.transport.micro_resident_imports;
  const binding=Object.fromEntries(helpers.map(name=>[name,name==='store8'||name==='store_resident8'?injector[name]:engine.api[name]]));
  const instance=new WebAssembly.Instance(new WebAssembly.Module(unit.bytes),{env:{memory:engine.memory},ring3:binding}),target={run:instance.exports.run};assert.equal(target.run.length,4);
  syntheticBindings.push({instance:engine.ordinal,name:engine.name,owner:engine.owner,source_child:unit.filename,source_child_sha256:hash(unit.bytes),key:engine.key.toString(),id:unit.id?.toString(),generation:unit.generation,imports:helpers,injector:injectorArtifact,injector_sha256:hash(injectorBytes),claim:'synthetic helper40/status validation only; helper has no guest RAM effects; entry guard is actual engine export'});
  for (const row of oracle.synthetic_protocol) {
    request(fromHex(row.input_helper_hex),`synthetic raw40 ${row.name}`);
    operation(`configure pureWasm synthetic ${row.name}`,()=>injector.configure(engine.base+100,engine.base+transfer,row.status),null);
    hostInput(`negative protocol CPU/Exit/opaque helper reset ${row.name}`,()=>{refresh().bytes.set(fromHex(spec.initial_state_hex),engine.base);engine.bytes.set(fromHex(spec.initial_exit_hex),engine.base+56);engine.bytes.set(fromHex(spec.initial_helper_hex),engine.base+100);});
    run(target,1,`synthetic protocol ${row.name}`,[[0,fromHex(row.state_hex)],[56,fromHex(row.exit_hex)],[100,fromHex(row.input_helper_hex)]]);
    const phase=`synthetic-${row.name}`;observe(phase,{state_hex:row.state_hex,exit_hex:row.exit_hex,helper_hex:row.input_helper_hex});
    syntheticObservations.push({instance:engine.ordinal,name:engine.name,owner:engine.owner,row:row.name,expected_status:row.status,outcome:row.outcome,source_child:unit.filename});
  }
  ramWords(spec.after_words,'synthetic protocol cannot change true-store RAM');
}
for (const spec of oracle.finite) {
  const unit=setupDirect(spec);success(spec,unit,'true byte MOV bundle and terminal JMP');ramWords(spec.after_words,'exact byte writes and neighboring sentinel preservation');
  if (spec.symbol==='all_sources') {
    syntheticProtocol(spec,unit);
    for (const row of oracle.host_low8) {
      operation(`direct write8 low8 input${row.input_value}`,()=>engine.api.write8(row.address,row.input_value),0,[[100,fromHex(row.helper_hex)]]);
      const actualWord=read(row.diagnostic_address,row.expected_word,'direct low8 truncation and neighboring sentinels');
      ramChecks.push({instance:engine.ordinal,name:engine.name,label:'host low8',address:row.diagnostic_address,actual_word:actualWord,expected_word:row.expected_word,input_value:row.input_value});
    }
  }
  retainedBytes();operation('close finite context',()=>engine.api.close());
}
for (const spec of oracle.endpoints) {
  const unit=setupDirect(spec);success(spec,unit,'true Write1 at final legal byte with Write-only permission');
  operation('enable diagnostic reads only after completed Write-only store',()=>engine.api.protect(spec.diagnostic_page,1,3));ramWords(spec.after_words,'last byte and preceding three neighbors');
  if(spec.target===0x1fff) operation('next unmapped page remains absent',()=>engine.api.read8(0x2000),0,[[100,record('R3MH',40,[1,0,1,0x2000,1,1],2)]]);
  retainedBytes();operation('close endpoint context',()=>engine.api.close());
}
for (const spec of oracle.faults) {
  const unit=setupDirect(spec);
  run(unit,64,'one retired LEA prefix then precise Write1 fault',[[0,fromHex(spec.fault_state_hex)],[56,fromHex(spec.fault_exit_hex)],[100,fromHex(spec.fault_helper_hex)]]);
  observe('fault',{state_hex:spec.fault_state_hex,exit_hex:spec.fault_exit_hex,helper_hex:spec.fault_helper_hex});
  cancel(1);run(unit,0,'Cancel precedes zero budget and failed store retry',[[56,fromHex(spec.pause_cancel_exit_hex)]]);
  observe('cancel-pause',{state_hex:spec.fault_state_hex,exit_hex:spec.pause_cancel_exit_hex,helper_hex:spec.fault_helper_hex});cancel(0);
  run(unit,0,'zero budget never repeats failed store',[[56,fromHex(spec.pause_budget_exit_hex)]]);
  observe('budget-pause',{state_hex:spec.fault_state_hex,exit_hex:spec.pause_budget_exit_hex,helper_hex:spec.fault_helper_hex});
  ramWords(spec.fault_words,'fault preserves readable operand and neighbors');
  for (const [address,permissions] of spec.repair_maps) operation('repair missing data page only',()=>engine.api.map(address,1,permissions));
  for (const [address,permissions] of spec.repair_protect) operation('repair data permission only',()=>engine.api.protect(address,1,permissions));
  success(spec,unit,'retry same faulting CPU without CPU restoration');ramWords(spec.after_words,'retry writes one byte once; retired prefix retained');
  retainedBytes();operation('close precise fault context',()=>engine.api.close());
}
for (const spec of oracle.smc) {
  const unit=setupDirect(spec,7);success(spec,unit,'NOP then real C6 Store1 invalidates current snapshot before EDI canary');ramWords(spec.after_words,'changed or identical one-byte code write');
  cancel(1);run(unit,0,'stale owner precedes Cancel/zero/malformed pointers',[],spec.stale_status,[0xffffffff,0xffffffff,0xffffffff]);observe('stale',{unchanged:true});retainedBytes();
  operation('close SMC context',()=>engine.api.close());run(unit,0,'Closed precedes stale owner/Cancel/malformed pointers',[],spec.closed_status,[0xffffffff,0xffffffff,0xffffffff]);observe('closed',{unchanged:true});
}
assert.equal(nextInstance,20);assert.equal(identities.length,26);assert.equal(syntheticBindings.length,2);assert.equal(syntheticObservations.length,68);
assert.equal(guestRetirements,128);assert.equal(operations.length,708);assert.equal(hostInputs.length,300);assert.equal(observations.length,126);assert.equal(ramChecks.length,58);assert.equal(stackChecks.length,12);
for (const filename of readdirSync(outputDir)) artifacts[filename]=hash(readFileSync(join(outputDir,filename)));
assert.equal(Object.keys(artifacts).length,2919);
const provenance={engine_sha256:hash(engineBytes),oracle_sha256:hash(oracleBytes),program_sha256:hash(programBytes),text_sha256:hash(code),pe_sha256:Object.fromEntries(oracle.pe.images.map(item=>[item.name,hash(images[item.name])])),artifacts,sources,identities,observations,operations,host_inputs:hostInputs,ram_checks:ramChecks,stack_checks:stackChecks,synthetic_bindings:syntheticBindings,synthetic_observations:syntheticObservations,guest_retirements:guestRetirements,command:[process.execPath,process.argv[1],enginePath,outputDir,root],assembly_command:['clang',...assemblyArgs],tools:{node:process.version,v8:process.versions.v8,clang:execFileSync('clang',['--version'],{encoding:'utf8'}).split('\n')[0]},claim:oracle.claim_ceiling};
writeFileSync(join(outputDir,'provenance.json'),JSON.stringify(provenance,null,2));
console.log(JSON.stringify({status:'ok',engine_sha256:provenance.engine_sha256,oracle_sha256:provenance.oracle_sha256,output:outputDir,instances:nextInstance,children:identities.length,observations:observations.length,operations:operations.length,host_inputs:hostInputs.length,positive_cases:4,finite_contexts:4,endpoint_contexts:4,fault_contexts:4,smc_contexts:4,synthetic_protocol_rows:syntheticObservations.length,ram_checks:ramChecks.length,stack_checks:stackChecks.length,artifacts:Object.keys(artifacts).length,positive_guest_retired:15,total_guest_retired:guestRetirements}));
