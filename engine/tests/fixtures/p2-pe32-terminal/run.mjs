import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';

const [enginePath, outputDir, root] = process.argv.slice(2);
assert.ok(root, 'expected actual engine, output directory and repository root');
const fixtureRoot = join(root, 'engine/tests/fixtures/p2-pe32-terminal');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const oracleBytes = readFileSync(join(fixtureRoot, 'oracle.json')), oracle = JSON.parse(oracleBytes);
assert.equal(hash(oracleBytes), 'c494e48d167db98b73c30b79b82195baa2934dd915d2974e53a391d4b324441f');
const programBytes = readFileSync(join(fixtureRoot, 'program.S'));
assert.equal(hash(programBytes), '40e257bd9b6271283756a117167c1e215f2f81c0f2228b0e2b530bdfbd336d2a');
const size = 4236, transfer = 140, artifacts = {}, observations = [], identities = [];

function authoredText(object) {
  const view = new DataView(object.buffer, object.byteOffset, object.byteLength);
  assert.deepEqual([...object.subarray(0, 7)], [0x7f, 0x45, 0x4c, 0x46, 1, 1, 1]);
  assert.equal(view.getUint16(18, true), 3);
  const offset = view.getUint32(32, true), stride = view.getUint16(46, true);
  const sections = Array.from({length: view.getUint16(48, true)}, (_, index) => {
    const at = offset + index * stride;
    return {name: view.getUint32(at, true), type: view.getUint32(at + 4, true), offset: view.getUint32(at + 16, true), size: view.getUint32(at + 20, true), link: view.getUint32(at + 24, true), info: view.getUint32(at + 28, true), stride: view.getUint32(at + 36, true)};
  });
  const names = sections[view.getUint16(50, true)];
  for (const section of sections) {
    const at = names.offset + section.name;
    section.name = object.subarray(at, object.indexOf(0, at)).toString('utf8');
  }
  const textIndex = sections.findIndex(section => section.name === '.text'), text = sections[textIndex];
  assert.ok(textIndex > 0);
  assert.ok(!sections.some(section => [4, 9].includes(section.type) && section.info === textIndex && section.size > 0));
  const code = object.subarray(text.offset, text.offset + text.size), symbols = {};
  const table = sections.find(section => section.type === 2), strings = sections[table.link];
  for (let at = table.offset; at < table.offset + table.size; at += table.stride) {
    const nameAt = strings.offset + view.getUint32(at, true);
    const name = object.subarray(nameAt, object.indexOf(0, nameAt)).toString('utf8');
    const section = view.getUint16(at + 14, true), start = view.getUint32(at + 4, true), length = view.getUint32(at + 8, true);
    if (section === textIndex && length > 0) symbols[name] = {offset: start, size: length, hex: code.subarray(start, start + length).toString('hex')};
  }
  assert.deepEqual(symbols, oracle.program.symbols);
  assert.equal(code.length, oracle.program.text_length);
  assert.equal(code.toString('hex'), oracle.program.text_hex);
  assert.equal(hash(code), oracle.program.text_sha256);
  return code;
}

// independent literal pe input; the host does not parse a runtime pe or resolve imports.
function authoredPe(code) {
  const bytes = Buffer.alloc(3072), view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  bytes.write('MZ'); view.setUint32(0x3c, 0x80, true); bytes.write('PE\0\0', 0x80);
  view.setUint16(0x84, 0x14c, true); view.setUint16(0x86, 4, true);
  view.setUint16(0x94, 224, true); view.setUint16(0x96, 0x102, true);
  const optional = 0x98;
  view.setUint16(optional, 0x10b, true);
  for (const [at, value] of [[4,512],[8,1536],[16,0x1000],[20,0x1000],[24,0x3000],[28,0x400000],[32,4096],[36,512],[56,0x6000],[60,1024],[72,0x100000],[76,4096],[80,0x100000],[84,4096],[92,16],[104,0x4000],[108,40],[136,0x5000],[140,20],[192,0x4050],[196,16]]) view.setUint32(optional + at, value, true);
  for (const [at, value] of [[40,4],[48,4],[68,3],[70,0x100]]) view.setUint16(optional + at, value, true);
  [['.text',52,0x1000,1024,0x60000020],['.data',4,0x3000,1536,0xc0000040],['.imports',160,0x4000,2048,0x40000040],['.fixups',20,0x5000,2560,0x40000040]].forEach(([name, virtualSize, rva, raw, flags], index) => {
    const at = 0x178 + index * 40; bytes.write(name, at);
    for (const [field, value] of [[8,virtualSize],[12,rva],[16,512],[20,raw],[36,flags]]) view.setUint32(at + field, value, true);
  });
  bytes.fill(0xcc, 1024, 1536); bytes.set(code, 1024);
  [0x4040,0,0,0x4060,0x4050].forEach((value, index) => view.setUint32(2048 + index * 4, value, true));
  for (const offset of [0x40,0x50]) [0x4070,0x4080,0x4090,0].forEach((value, index) => view.setUint32(2048 + offset + index * 4, value, true));
  bytes.write('KeRnEl32.dLl\0', 2048 + 0x60);
  [[0x70,0x1234,'GetLastError'],[0x80,0xffff,'SetLastError'],[0x90,0xbeef,'ExitProcess']].forEach(([offset, hint, name]) => { view.setUint16(2048 + offset, hint, true); bytes.write(`${name}\0`, 2048 + offset + 2); });
  bytes.set(Buffer.from('001000001400000002300f30153022302c300000', 'hex'), 2560);
  return bytes;
}
const objectPath = join(outputDir, 'program.o'), assemble = ['-target','i386-unknown-linux-gnu','-c',join(fixtureRoot, 'program.S'),'-o',objectPath];
execFileSync('clang', assemble, {stdio: ['ignore','pipe','pipe']});
writeFileSync(join(outputDir, 'program.disassembly.txt'), execFileSync('xcrun', ['llvm-objdump','-d','--x86-asm-syntax=intel',objectPath]));
artifacts['program.o'] = hash(readFileSync(objectPath)); artifacts['program.disassembly.txt'] = hash(readFileSync(join(outputDir, 'program.disassembly.txt')));
const code = authoredText(readFileSync(objectPath)), image = authoredPe(code);
assert.equal(hash(image), oracle.pe.sha256);
writeFileSync(join(outputDir, 'linked-terminal.exe'), image); artifacts['linked-terminal.exe'] = hash(image);
writeFileSync(join(outputDir, 'program.x86'), code); artifacts['program.x86'] = hash(code);
writeFileSync(join(outputDir, 'program.S'), programBytes); writeFileSync(join(outputDir, 'terminal-oracle.json'), oracleBytes);
writeFileSync(join(outputDir, 'run.mjs'), readFileSync(import.meta.filename));
const productionPaths = execFileSync('rg', ['--files','engine/src'], {cwd: root, encoding: 'utf8'}).trim().split('\n').sort();
const sourcePaths = [...productionPaths,'Cargo.toml','engine/Cargo.toml','Cargo.lock','engine/tests/process_pe32_terminal_wasm.rs','engine/tests/fixtures/p2-pe32-terminal/program.S','engine/tests/fixtures/p2-pe32-terminal/oracle.json','engine/tests/fixtures/p2-pe32-terminal/run.mjs'];
const sources = Object.fromEntries(sourcePaths.map(file => [file, hash(readFileSync(join(root, file)))]));
writeFileSync(join(outputDir, 'source-sha256.json'), JSON.stringify(sources, null, 2));
const engineBytes = readFileSync(enginePath); writeFileSync(join(outputDir, 'engine.wasm'), engineBytes);
assert.ok(WebAssembly.validate(engineBytes));
const engineModule = new WebAssembly.Module(engineBytes); assert.deepEqual(WebAssembly.Module.imports(engineModule), []);
const names = ['open','close','arena_ptr','load_pe32','load_pe32_at','load_pe32_linked_at','load_pe32_linked_v2_at','start_loaded_image','map','compile_entries','compile_resident_entries','generation','module_ptr','module_len','resident_module','find_resident','acknowledge_resident_installation','find_installed_resident','dispatcher_module','guard','guard_dispatch_entry','guard_resident','read32','write32','store32','store_resident32','capture_call','capture_resident_call','complete_call','complete_resident_call','complete_windows_call','complete_resident_windows_call'];
let engine, nextInstance = 0;
const fromHex = hex => new Uint8Array(Buffer.from(hex, 'hex'));
function refresh() { if (engine.buffer !== engine.memory.buffer) { engine.buffer = engine.memory.buffer; engine.bytes = new Uint8Array(engine.buffer); engine.view = new DataView(engine.buffer); } return engine; }
function arena() { return refresh().bytes.slice(engine.base, engine.base + size); }
function record(magic, length, fields, version = 1) {
  const bytes = new Uint8Array(length), view = new DataView(bytes.buffer);
  bytes.set(Buffer.from(magic)); view.setUint16(4, version, true); view.setUint16(6, 1, true); view.setUint32(8, length, true);
  fields.forEach((value, index) => view.setUint32(16 + index * 4, value, true)); return bytes;
}
function words(fields) { const bytes = new Uint8Array(fields.length * 4), view = new DataView(bytes.buffer); fields.forEach((value, index) => view.setUint32(index * 4, value, true)); return bytes; }
function cpu(registers, pc) { return record('R3ST', 56, [...registers, pc, 2]); }
function helper(value = 0) { return record('R3MH', 40, [0,value,0,0,0,0]); }
function residentRecord(unit) { return words([1,24,unit.low,unit.high,unit.pointer,unit.length]); }
function installation(unit, slot) { return record('R3IN', 32, [unit.low,unit.high,slot,0]); }
function operation(label, action, status = 0, patches = []) {
  const expected = arena(); for (const [offset, bytes] of patches) expected.set(bytes, offset);
  assert.equal(action(), status, `${engine.name}/${label}: status or value`); assert.deepEqual(arena(), expected, `${engine.name}/${label}: whole4236 arena`);
}
function cancel(value) { refresh().view.setUint32(engine.base + 96, value, true); }
function request(bytes = image) { refresh().bytes.set(bytes, engine.base + transfer); }
function read(address, value, label) { operation(label, () => engine.api.read32(address), 0, [[100,helper(value)]]); return refresh().view.getUint32(engine.base + 120, true); }
function fault(address, access, detail, label) { operation(label, () => access === 1 ? engine.api.read32(address) : engine.api.write32(address, 0xdeadbeef), 0, [[100,record('R3MH', 40, [1,0,detail,address,access,4])]]); }
function stack(values, label) { return oracle.runtime.stack_addresses.map((address, index) => read(address, values[index], `${label}/word${index}`)); }
function fresh(name, pages = oracle.runtime.pages) {
  const instance = new WebAssembly.Instance(engineModule, {});
  const api = Object.fromEntries(names.map(name => { const fn = instance.exports[`ring3_abi_v1_${name}`]; assert.equal(typeof fn, 'function', name); return [name,fn]; }));
  assert.equal(api.load_pe32_linked_v2_at.length, 3); assert.equal(api.load_pe32_linked_at.length, 3);
  assert.equal(api.complete_windows_call.length, 4); assert.equal(api.complete_resident_windows_call.length, 5);
  const ordinal = ++nextInstance, key = 0xa345cdef00000000n + BigInt(ordinal);
  engine = {name,ordinal,api,key,low:Number(key & 0xffffffffn),high:Number(key >> 32n),memory:instance.exports.memory,units:[]};
  assert.ok(engine.memory instanceof WebAssembly.Memory); assert.equal(api.open(pages, engine.low, engine.high), 0);
  engine.base = api.arena_ptr() >>> 0; refresh(); assert.ok(engine.base > 0 && engine.base + size <= engine.bytes.length);
  assert.deepEqual(arena().subarray(0,56), cpu([0,0,0,0,0,0,0,0], 0));
  assert.deepEqual(arena().subarray(56,96), record('R3EX', 40, [1,0,0,0,0,0]));
  engine.bytes.fill(0xa5, engine.base + 100, engine.base + size);
  return engine;
}
function load(example) {
  request(); operation('Rust v2 loads, relocates and links the PE', () => engine.api.load_pe32_linked_v2_at(image.length, example.base, oracle.runtime.gate_base), 0, [[transfer,fromHex(example.receipt_hex)]]);
  const bytes = arena().slice(transfer, transfer + 72), view = new DataView(bytes.buffer);
  engine.receipt = {image_base:view.getUint32(16,true),image_size:view.getUint32(20,true),entry:view.getUint32(24,true),mapped_pages:view.getUint32(28,true),gate_base:view.getUint32(32,true),gate_count:view.getUint32(36,true),gates:Array.from({length:view.getUint32(36,true)},(_,index)=>[view.getUint32(48+index*8,true),view.getUint32(52+index*8,true)])};
  assert.deepEqual(engine.receipt.gates, oracle.runtime.gates);
  const filename = `${engine.name}-linked-receipt.bin`; writeFileSync(join(outputDir, filename), bytes); artifacts[filename] = hash(bytes);
  refresh().bytes.fill(0xa5, engine.base + transfer, engine.base + size);
}
function entriesRequest(entries, gates = []) {
  const view = refresh().view;
  entries.forEach((pc,index)=>view.setUint32(engine.base+transfer+index*4,pc,true));
  gates.forEach(([pc,id],index)=>{const offset=engine.base+transfer+entries.length*4+index*8;view.setUint32(offset,pc,true);view.setUint32(offset+4,id,true);});
}
function instantiate(unit, helpers) {
  refresh(); assert.ok(unit.pointer > 0 && unit.length > 8 && unit.pointer + unit.length <= engine.bytes.length);
  unit.bytes = engine.bytes.slice(unit.pointer, unit.pointer + unit.length);
  const module = new WebAssembly.Module(unit.bytes), imports = WebAssembly.Module.imports(module);
  const sort = rows => rows.sort((a,b)=>`${a.module}.${a.name}`.localeCompare(`${b.module}.${b.name}`));
  assert.deepEqual(sort(imports), sort([{module:'env',name:'memory',kind:'memory'},...helpers.map(name=>({module:'ring3',name,kind:'function'}))]));
  unit.instance = new WebAssembly.Instance(module, {env:{memory:engine.memory},ring3:Object.fromEntries(helpers.map(name=>[name,engine.api[name]]))});
  unit.run = unit.instance.exports.run; assert.equal(typeof unit.run, 'function'); assert.equal(unit.run.length, 4); engine.units.push(unit);
  const filename = `${engine.name}-${unit.label}.wasm`; writeFileSync(join(outputDir, filename), unit.bytes); artifacts[filename] = hash(unit.bytes);
  identities.push({instance:engine.ordinal,owner:engine.owner,key:engine.key.toString(),label:unit.label,id:unit.id?.toString(),generation:unit.generation,pointer:unit.pointer,length:unit.length,module_sha256:hash(unit.bytes)});
  return unit;
}
function replacement(entries, gates) {
  entriesRequest(entries,gates); operation('eight-entry replacement compiler publishes no metadata', () => engine.api.compile_entries(entries.length,gates.length));
  assert.equal(engine.api.generation(), oracle.transport.replacement.generation);
  return instantiate({label:'replacement',generation:engine.api.generation(),pointer:engine.api.module_ptr()>>>0,length:engine.api.module_len()>>>0,entries},oracle.transport.replacement.helper_imports);
}
function resident(entries, gates, label, helpers) {
  entriesRequest(entries,gates); const before=arena(); assert.equal(engine.api.compile_resident_entries(entries.length,gates.length),0);
  const after=arena(),view=new DataView(after.buffer,transfer,24);
  const unit={low:view.getUint32(8,true),high:view.getUint32(12,true),pointer:view.getUint32(16,true),length:view.getUint32(20,true),entries,label};
  unit.id=BigInt(unit.low)|(BigInt(unit.high)<<32n); assert.notEqual(unit.id,0n);assert.ok(!engine.units.some(item=>item.id===unit.id));
  const expected=before.slice();expected.set(residentRecord(unit),transfer);assert.deepEqual(after,expected);
  instantiate(unit,helpers);entries.forEach(pc=>operation('logical owner from returned full id',()=>engine.api.find_resident(pc),0,[[transfer,residentRecord(unit)]]));return unit;
}
function install(unit,slot) {
  operation('resident has no installation before host table assignment',()=>engine.api.find_installed_resident(engine.low,engine.high,unit.entries[0]),17);
  engine.table.set(slot,unit.run);operation('ack publishes exact installation only',()=>engine.api.acknowledge_resident_installation(engine.low,engine.high,unit.low,unit.high,slot),0,[[transfer,installation(unit,slot)]]);
  assert.equal(engine.table.get(slot),unit.run);unit.entries.forEach(pc=>operation('installed lookup from full returned owner',()=>engine.api.find_installed_resident(engine.low,engine.high,pc),0,[[transfer,installation(unit,slot)]]));
}
function dispatcher() {
  const before=arena();assert.equal(engine.api.dispatcher_module(engine.low,engine.high),0);
  const after=arena(),view=new DataView(after.buffer,transfer,32),pointer=view.getUint32(24,true),length=view.getUint32(28,true);
  const expected=before.slice();expected.set(record('R3DP',32,[engine.low,engine.high,pointer,length]),transfer);assert.deepEqual(after,expected);
  refresh();assert.ok(pointer>0&&length>8&&pointer+length<=engine.bytes.length);
  const bytes=engine.bytes.slice(pointer,pointer+length),module=new WebAssembly.Module(bytes);
  assert.deepEqual(WebAssembly.Module.imports(module),[{module:'env',name:'memory',kind:'memory'},{module:'env',name:'table',kind:'table'},{module:'ring3',name:'guard_dispatch_entry',kind:'function'},{module:'ring3',name:'find_installed_resident',kind:'function'}]);
  engine.dispatcher={pointer,length,bytes,instance:new WebAssembly.Instance(module,{env:{memory:engine.memory,table:engine.table},ring3:{guard_dispatch_entry:engine.api.guard_dispatch_entry,find_installed_resident:engine.api.find_installed_resident}})};
  engine.dispatcher.run=engine.dispatcher.instance.exports.run;assert.equal(engine.dispatcher.run.length,4);
  const filename=`${engine.name}-returning-dispatcher.wasm`;writeFileSync(join(outputDir,filename),bytes);artifacts[filename]=hash(bytes);
}
function setup(example) {
  fresh(example.name);engine.owner=example.owner;const initial=arena().slice(0,96);load(example);
  const receipt=engine.receipt;
  read(receipt.image_base+0xb4,oracle.pe.preferred_base,'header keeps preferred ImageBase');
  oracle.pe.target_rvas.forEach((rva,index)=>read(receipt.image_base+rva,example.patched_words[index],`literal HIGHLOW operand${index}`));
  receipt.gates.forEach(([pc],index)=>{read(receipt.image_base+0x4050+index*4,pc,`Rust IAT slot${index}`);read(pc,0x00000b0f,`owned UD2 stub${index}`);read(pc+4,0,`zero stub tail${index}`);fault(pc,2,2,`RX Gate rejects negative write${index}`);});
  read(receipt.image_base+0x405c,0,'IAT terminator retained');
  if(example.base!==oracle.pe.preferred_base)fault(oracle.pe.preferred_base+0x1000,1,1,'preferred image is unmapped at relocated load');
  const callerEntries=oracle.program.caller_offsets.map(offset=>receipt.entry+offset),gateEntries=receipt.gates.map(([pc])=>pc);
  if(example.owner==='resident'){
    engine.table=new WebAssembly.Table({element:'anyfunc',initial:8,maximum:8});
    engine.gate=resident(gateEntries,receipt.gates,'gate-B',oracle.transport.resident.gate_helper_imports);install(engine.gate,0);
    engine.caller=resident(callerEntries,[],'caller-A-with-canary',oracle.transport.resident.caller_helper_imports);install(engine.caller,1);dispatcher();assert.equal(engine.api.generation(),0);
  }else{engine.caller=replacement([...callerEntries,...gateEntries],receipt.gates);engine.gate=engine.caller;}
  assert.deepEqual(arena().subarray(0,96),initial,'load/read/compile/install preserve startup context');
  operation('engine owns all CPU and empty-stack startup',()=>engine.api.start_loaded_image(oracle.runtime.stack_base,oracle.runtime.stack_pages),0,[[0,fromHex(example.startup_state_hex)],[56,fromHex(oracle.records.startup_exit_hex)]]);
  stack(oracle.runtime.stack_initial,'empty engine stack');read(receipt.image_base+oracle.runtime.data_rva,0,'canary data before guest');
}
function run(target,budget,label,patches=[],status=0,pointers=[engine.base,engine.base+56,engine.base+96]) { operation(label,()=>target.run(pointers[0],pointers[1],budget,pointers[2]),status,patches); }
function capture(stage,expected) {
  const owner=engine.gate;
  if(engine.owner==='resident'){
    const view=refresh().view;assert.deepEqual(arena().subarray(transfer,transfer+32),installation(owner,0));
    const low=view.getUint32(engine.base+transfer+16,true),high=view.getUint32(engine.base+transfer+20,true);
    operation('private IAT Gate-B capture with current full owner',()=>engine.api.capture_resident_call(engine.low,engine.high,low,high,2,stage.args.length),0,[[transfer,fromHex(expected.frame_hex)]]);
  }else operation('private replacement IAT capture',()=>engine.api.capture_call(engine.low,engine.high,owner.generation,2,stage.args.length),0,[[transfer,fromHex(expected.frame_hex)]]);
}
function provider(token) { const owner=engine.gate;return engine.owner==='resident'?engine.api.complete_resident_windows_call(engine.low,engine.high,owner.low,owner.high,token):engine.api.complete_windows_call(engine.low,engine.high,owner.generation,token); }
function scalar(token) { const owner=engine.gate;return engine.owner==='resident'?engine.api.complete_resident_call(engine.low,engine.high,owner.low,owner.high,token,0):engine.api.complete_call(engine.low,engine.high,owner.generation,token,0); }
function retainedBytes() {
  refresh();for(const unit of engine.units)assert.deepEqual(engine.bytes.slice(unit.pointer,unit.pointer+unit.length),unit.bytes);
  if(engine.dispatcher){assert.deepEqual(engine.bytes.slice(engine.dispatcher.pointer,engine.dispatcher.pointer+engine.dispatcher.length),engine.dispatcher.bytes);assert.equal(engine.table.get(0),engine.gate.run);assert.equal(engine.table.get(1),engine.caller.run);}
}
for(const example of oracle.cases){
  setup(example);const primary=engine.dispatcher??engine.caller,base=engine.base,memory=engine.memory;let total=0;
  for(let index=0;index<oracle.stages.length;index++){
    const stage=oracle.stages[index],expected=example.stages[index];
    const patches=[[0,fromHex(expected.stop_state_hex)],[56,fromHex(stage.gate_exit_hex)],[100,helper()]];
    if(engine.owner==='resident')patches.push([transfer,installation(engine.gate,0)]);
    run(primary,oracle.runtime.budget,`real IAT ${stage.api} fresh guest retirement`,patches);
    const gateArena=arena(),actualRetired=new DataView(gateArena.buffer).getUint32(76,true);total+=actualRetired;
    const guestStack=stack(expected.stack_values,`${stage.api} real guest return/argument`);read(engine.receipt.image_base+oracle.runtime.data_rva,0,'guest has not executed return canary');
    capture(stage,expected);const captured=arena().slice(transfer,transfer+112);
    if(stage.api==='ExitProcess'){
      // explicit negative diagnostic/guest argument tampering challenges private saved authority.
      refresh().view.setUint32(engine.base+transfer+48,oracle.negative_controls.diagnostic_argument,true);
      operation('guest exit argument differs from saved private frame',()=>engine.api.write32(0x8ffc,oracle.negative_controls.guest_argument),0,[[100,helper()]]);
      operation('reserved ExitProcess scalar return is refused',()=>scalar(stage.token),7);
      cancel(1);operation('cancelled terminal retains same-token retry',()=>provider(stage.token),16);cancel(0);
      operation('named ExitProcess publishes fresh terminal without returning',()=>provider(stage.token),0,[[56,fromHex(oracle.records.terminal_exit_hex)]]);
    }else{
      operation('engine Windows completion owns actual caller return',()=>provider(stage.token),0,[[0,fromHex(expected.returned_state_hex)],[56,fromHex(oracle.records.returned_exit_hex)]]);
      stack(expected.stack_values,'provider leaves guest return/argument bytes');operation('consumed ordinary token cannot replay',()=>provider(stage.token),14);
    }
    const result=arena();observations.push({name:example.name,owner:example.owner,api:stage.api,token:stage.token,guest_retired:actualRetired,gate_state_hex:Buffer.from(gateArena.subarray(0,56)).toString('hex'),gate_exit_hex:Buffer.from(gateArena.subarray(56,96)).toString('hex'),captured_frame_hex:Buffer.from(captured).toString('hex'),guest_stack:guestStack,result_state_hex:Buffer.from(result.subarray(0,56)).toString('hex'),result_exit_hex:Buffer.from(result.subarray(56,96)).toString('hex')});
  }
  assert.equal(total,oracle.runtime.total_guest_retired);
  const terminal=arena(),filename=`${engine.name}-terminal-arena.bin`;writeFileSync(join(outputDir,filename),terminal);artifacts[filename]=hash(terminal);
  stack(example.stack_after_tamper,'terminal leaves exit stack words');const data=read(engine.receipt.image_base+oracle.runtime.data_rva,0,'terminal never returns to store');
  operation('terminal before wrong provider token',()=>provider(0),21);operation('terminal scalar replay inert',()=>scalar(4),21);
  operation('terminal before invalid new loader',()=>engine.api.load_pe32_linked_v2_at(4097,0,0),21);operation('terminal before malformed map',()=>engine.api.map(1,0,99),21);
  operation('terminal blocks guest write/helper publication',()=>engine.api.write32(engine.receipt.image_base+oracle.runtime.data_rva,0xdeadbeef),21);
  operation('terminal before malformed startup',()=>engine.api.start_loaded_image(1,0),21);operation('terminal before invalid replacement compile',()=>engine.api.compile_entries(0,9),21);
  operation('terminal value module pointer is zero',()=>engine.api.module_ptr(),0);operation('terminal value module length is zero',()=>engine.api.module_len(),0);
  operation('terminal before dispatcher status receipt',()=>engine.api.dispatcher_module(0,0),21);if(engine.owner==='resident')operation('terminal before resident status receipt',()=>engine.api.resident_module(0,0),21);
  operation('retained allocation remains occupied',()=>engine.api.open(0,0,0),6);assert.equal(engine.api.arena_ptr(),base);assert.equal(engine.memory,memory);assert.equal(engine.api.generation(),engine.owner==='resident'?0:1);retainedBytes();
  observations.push({name:example.name,owner:example.owner,phase:'terminal',total_guest_retired:total,terminal_fresh_retired:new DataView(terminal.buffer).getUint32(76,true),exit_code:new DataView(terminal.buffer).getUint32(80,true),state_hex:Buffer.from(terminal.subarray(0,56)).toString('hex'),exit_hex:Buffer.from(terminal.subarray(56,96)).toString('hex'),data});
  // explicit postterminal raw restoration is a negative lifecycle challenge, never startup.
  refresh().bytes.set(fromHex(example.canary_restore_state_hex),engine.base);engine.bytes.set(fromHex(oracle.negative_controls.canary_restore_exit_hex),engine.base+56);
  for(const target of new Set([engine.caller,engine.gate,engine.dispatcher].filter(Boolean))){
    run(target,oracle.runtime.budget,'retained supported canary refuses private terminal',[],21);
    cancel(1);run(target,0,'terminal before cancel/zero budget',[],21);cancel(0);
    run(target,0,'terminal before malformed pointers',[],21,[0xffffffff,0xffffffff,0xffffffff]);
  }
  retainedBytes();read(engine.receipt.image_base+oracle.runtime.data_rva,0,'supported restored canary remains inert');stack(example.stack_after_tamper,'rejected runs preserve guest words');
  operation('close preserves arena tombstone',()=>engine.api.close());run(engine.caller,oracle.runtime.budget,'retained caller becomes Closed',[],5);
  operation('Closed before new loader arguments',()=>engine.api.load_pe32_linked_v2_at(4097,0,0),5);operation('Closed before diagnostic read',()=>engine.api.read32(engine.receipt.image_base+oracle.runtime.data_rva),5);
}

const malformed=Buffer.from(image),unknown=Buffer.from(image),alias=Buffer.from(image);
malformed.writeUInt32LE(1,2048+20);unknown[2048+0x92]='x'.charCodeAt(0);alias.writeUInt32LE(0x4040,2048+16);alias[2048+0x92]='x'.charCodeAt(0);
for(const [name,bytes,length,base,gate,pages,status] of [
  ['descriptor terminator',malformed,3072,0x500000,0x60000000,7,19],['unknown exact symbol',unknown,3072,0x500000,0x60000000,7,20],['structural alias before unknown symbol',alias,3072,0x500000,0x60000000,7,19],['oversized transfer',image,4097,0x500000,0x60000000,7,7],['six-page capacity includes gate',image,3072,0x500000,0x60000000,5,8],['gate in image gap',image,3072,0x500000,0x502000,7,19],
]){
  fresh(`negative-${name}`,pages);request(bytes);operation('failed candidate preserves whole arena',()=>engine.api.load_pe32_linked_v2_at(length,base,gate),status);
  fault(base+0x1000,1,1,'failed image has no mapped text');fault(gate,1,1,'failed image has no mapped requested Gate page');assert.equal(engine.api.generation(),0);
  if(pages===7){const example=oracle.cases.find(item=>item.base===base);load(example);operation('valid same-instance retry still admits startup',()=>engine.api.start_loaded_image(0x8000,1),0,[[0,fromHex(example.startup_state_hex)],[56,fromHex(oracle.records.startup_exit_hex)]]);}
  operation('negative case close',()=>engine.api.close());
}
fresh('legacy-closed-profile');request();
operation('old R3LI64 loader still refuses named ExitProcess',()=>engine.api.load_pe32_linked_at(image.length,0x400000,0x60000000),20);
operation('old fixed loader still refuses imports',()=>engine.api.load_pe32(image.length),20);operation('old relocated loader still refuses imports',()=>engine.api.load_pe32_at(image.length,0x500000),20);
const valid=oracle.cases[0];load(valid);operation('loaded image latch before malformed v2 replay',()=>engine.api.load_pe32_linked_v2_at(4097,0,0),7);operation('legacy control close',()=>engine.api.close());
const provenance={
  engine_sha256:hash(engineBytes),oracle_sha256:hash(oracleBytes),program_sha256:hash(programBytes),text_sha256:hash(code),pe_sha256:hash(image),artifacts,sources,identities,observations,
  command:[process.execPath,process.argv[1],enginePath,outputDir,root],assembly_command:['clang',...assemble],
  tools:{node:process.version,v8:process.versions.v8,clang:execFileSync('clang',['--version'],{encoding:'utf8'}).split('\n')[0]},
  claim:'independent authored PE32 Get/Set/Get/Exit IAT calls execute at both preferred and selected bases with replacement and resident plus returning dispatcher owners. Rust alone validates/relocates IAT and RX stubs; runtime entry/Gates come from R3LIv2, other caller seeds from entry plus frozen authored offsets and explicit precompiled canary. Engine startup owns initial CPU/empty stack. Real fresh retirements1+3+1+3 sum8; private provider returns preserve distinct LastError89abcdef and terminal saved exitf1234567 without caller return/ESP cleanup. Explicit diagnostic CF/guest argument tamper, scalar7/cancel16 and raw postterminal CPU/Exit restore are negative controls. Retained modules/dispatcher reject21 before arena/pointer/cancel/budget, canary data0, bounded read diagnostics remain until close5. All output receipts/frames/State/Exit and failures compare whole4236arena; failed candidates remain unmapped and retry, old R3LI64/fixed/relocated profiles refuse imports as specified. Host Memory/Table provenance remains trusted; native matrix owns complete owner/version/LastError retention and shared parser limits. No arbitrary imports/DLL/threads/cleanup/TLS/PEB/TEB/SEH/argv/scheduler/reset/SDK/browser/performance/game/full P2-V0 claim.',
};
writeFileSync(join(outputDir,'provenance.json'),JSON.stringify(provenance,null,2));
console.log(JSON.stringify({status:'ok',engine_sha256:provenance.engine_sha256,oracle_sha256:provenance.oracle_sha256,cases:oracle.cases.map(item=>item.name),guest_retired_per_case:8,terminal_retired_per_case:0,output:outputDir}));
