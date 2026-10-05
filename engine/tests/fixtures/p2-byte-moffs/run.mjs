import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(root, 'expected actual engine, new output directory and repository root');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const SIZE = 4236, TRANSFER = 140, EXIT = 0x8020, EXIT_ID = 0x10003, MARKER = 0xaa77ffc1;
const arities = {open:3, close:0, arena_ptr:0, map:3, protect:3, upload:2, read32:1, compile_entries:2, compile_resident_entries:2, generation:0, module_ptr:0, module_len:0, guard:6, guard_resident:7, read8:1, store8:2, store_resident8:6, store32:2, store_resident32:6, begin_image_input:1, append_image_input:2, load_pe32_linked_v2_input_at:2, start_loaded_image:2, capture_call:5, capture_resident_call:6, complete_windows_call:4, complete_resident_windows_call:5};
const sourcePaths = [...execFileSync('rg', ['--files', 'engine/src'], {cwd:root, encoding:'utf8'}).trim().split('\n').sort(), 'Cargo.toml', 'Cargo.lock', 'engine/Cargo.toml', 'engine/tests/cpu_byte_moffs_wasm.rs', 'engine/tests/fixtures/p2-byte-moffs/run.mjs', 'engine/tests/fixtures/p2-byte-moffs/integer.S'];
assert.equal(sourcePaths.length, 88); assert.equal(new Set(sourcePaths).size, 88);
const sources = () => Object.fromEntries(sourcePaths.map(path => [path, hash(readFileSync(join(root, path)))]));
const beforeSources = sources(), engineBytes = readFileSync(enginePath), engineModule = new WebAssembly.Module(engineBytes);
assert.deepEqual(WebAssembly.Module.imports(engineModule), []);
assert.equal(WebAssembly.Module.exports(engineModule).length, 71);
assert.equal(WebAssembly.Module.exports(engineModule).filter(row => row.kind === 'function').length, 70);
writeFileSync(join(output, 'engine.wasm'), engineBytes);
writeFileSync(join(output, 'source-before.json'), JSON.stringify(beforeSources, null, 2));
for (const file of ['run.mjs', 'integer.S']) writeFileSync(join(output, file), readFileSync(join(root, 'engine/tests/fixtures/p2-byte-moffs', file)));

// two encoding-only samples; this entire phase precedes every generated guest call.
const objectPath = join(output, 'integer.o'), assemblyArgs = ['-target', 'i386-unknown-linux-gnu', '-c', join(root, 'engine/tests/fixtures/p2-byte-moffs/integer.S'), '-o', objectPath];
execFileSync('clang', assemblyArgs, {stdio:['ignore','pipe','pipe']});
const disassemblyArgs = ['llvm-objdump', '-d', '--x86-asm-syntax=intel', objectPath];
const disassembly = execFileSync('xcrun', disassemblyArgs); writeFileSync(join(output, 'integer.disassembly.txt'), disassembly);
const object = readFileSync(objectPath), sectionOffset = object.readUInt32LE(32), sectionStride = object.readUInt16LE(46), sections = [];
assert.deepEqual([...object.subarray(0,7)], [0x7f,0x45,0x4c,0x46,1,1,1]); assert.equal(object.readUInt16LE(18), 3);
for (let index = 0; index < object.readUInt16LE(48); index++) {const at = sectionOffset + index * sectionStride; sections.push({name:object.readUInt32LE(at), type:object.readUInt32LE(at+4), offset:object.readUInt32LE(at+16), size:object.readUInt32LE(at+20), info:object.readUInt32LE(at+28)});}
const strings = sections[object.readUInt16LE(50)];
for (const section of sections) {const at = strings.offset + section.name; section.name = object.subarray(at, object.indexOf(0,at)).toString();}
const textIndex = sections.findIndex(section => section.name === '.text'); assert.ok(textIndex > 0);
assert.ok(!sections.some(section => [4,9].includes(section.type) && section.info === textIndex && section.size));
const encoding = object.subarray(sections[textIndex].offset, sections[textIndex].offset + sections[textIndex].size);
assert.equal(encoding.toString('hex'), 'a0efcdab89a2efcdab89');
assert.match(disassembly.toString(), /mov\s+al, byte ptr/); assert.match(disassembly.toString(), /mov\s+byte ptr.*al/);
const llvm = {scope:'encoding-only two samples', assembly_argv:['clang',...assemblyArgs], disassembly_argv:['xcrun',...disassemblyArgs], object_sha256:hash(object), text_hex:encoding.toString('hex'), disassembly_sha256:hash(disassembly)};

const finite = [
  {op:0xa0, address:0x5010, value:0, eax:0x123456ff, result:0x12345600, flags:0x0cd7},
  {op:0xa0, address:0x80000fff, value:0x80, eax:0x89abcd01, result:0x89abcd80, flags:0x0887},
  {op:0xa0, address:0xffffffff, value:0xff, eax:0xdeadbe80, result:0xdeadbeff, flags:0x04d7},
  {op:0xa2, address:0x5010, value:0, eax:0x12345600, result:0x12345600, flags:0x0cd7},
  {op:0xa2, address:0x80000fff, value:0x80, eax:0x89abcd80, result:0x89abcd80, flags:0x0887},
  {op:0xa2, address:0xffffffff, value:0xff, eax:0xdeadbeff, result:0xdeadbeff, flags:0x04d7}
];
const initialRegisters = [0xdeadbeef,0x23456701,0x3456789a,0x456789ab,0x56789abc,0x6789abcd,0x789abcde,0x89abcdef];
const modules = [], rows = [], runs = [], observations = [], hostCalls = [];
let contexts = 0, pageChecks = 0, read32Calls = 0;
function words(fields) {const bytes = Buffer.alloc(fields.length*4); fields.forEach((value,index) => bytes.writeUInt32LE(value>>>0,index*4)); return bytes;}
function record(magic, length, fields, version=1) {const bytes = Buffer.alloc(length); bytes.write(magic); bytes.writeUInt16LE(version,4); bytes.writeUInt16LE(1,6); bytes.writeUInt32LE(length,8); bytes.set(words(fields),16); return bytes;}
const state = (registers, pc, flags) => {
  assert.equal(flags&2,2); assert.equal((flags&~0xcd7)>>>0,0);
  return record('R3ST',56,[...registers,pc,flags]);
};
const exit = (reason, retired, version=2, detail=0, address=0, access=0, length=0) => record('R3EX',40,[reason,retired,detail,address,access,length],version);
const readHelper = (value, reason=0, address=0) => record('R3MH',40,[reason?1:0,reason?0:value,reason,address,reason?1:0,1],2);
const storeHelper = (reason=0, address=0) => record('R3MH',40,[reason?1:0,0,reason,address,reason?2:0,1],3);
function refresh(ctx) {ctx.bytes = new Uint8Array(ctx.memory.buffer); ctx.view = new DataView(ctx.memory.buffer); return ctx;}
function arena(ctx) {return refresh(ctx).bytes.slice(ctx.base,ctx.base+SIZE);}
function request(ctx, bytes) {assert.ok(bytes.length<=4096); refresh(ctx).bytes.set(bytes,ctx.base+TRANSFER);}
function call(ctx, name, args=[], status=0, patches=[]) {
  const before = ctx.base ? arena(ctx) : null, value = ctx.api[name](...args); if(status!==null) assert.equal(value,status,name);
  if(before) {const expected = before.slice(); for(const [offset,bytes] of typeof patches==='function'?patches():patches) expected.set(bytes,offset); assert.deepEqual(arena(ctx),expected,`${name}: complete arena`);}
  hostCalls.push({context:ctx.ordinal,name,args,status:value}); return value;
}
function fresh(owner, label) {
  const instance = new WebAssembly.Instance(engineModule,{}), ordinal = ++contexts, key = 0xc377000000000000n+BigInt(ordinal);
  const api = Object.fromEntries(Object.entries(arities).map(([name,arity]) => {const fn = instance.exports[`ring3_abi_v1_${name}`]; assert.equal(typeof fn,'function'); assert.equal(fn.length,arity,name); return [name,fn];}));
  const ctx = {owner,label,ordinal,api,memory:instance.exports.memory,low:Number(key&0xffffffffn),high:Number(key>>32n),pages:new Map(),units:0};
  call(ctx,'open',[8,ctx.low,ctx.high]); ctx.base = call(ctx,'arena_ptr',[],null)>>>0; assert.ok(ctx.base>0); return refresh(ctx);
}
function mapPage(ctx, address, bytes, permissions=7) {call(ctx,'map',[address,1,7]); request(ctx,bytes); call(ctx,'upload',[address,4096]); ctx.pages.set(address,Buffer.from(bytes)); if(permissions!==7) call(ctx,'protect',[address,1,permissions]);}
function readPages(ctx, label) {
  const records = [];
  for(const [address,expectedPage] of ctx.pages) {
    const expectedArena = arena(ctx), observed = Buffer.alloc(4096);
    for(let offset=0;offset<4096;offset+=4) {assert.equal(ctx.api.read32(address+offset),0); read32Calls++; refresh(ctx); const packet = record('R3MH',40,[0,expectedPage.readUInt32LE(offset),0,0,0,0]); assert.deepEqual(ctx.bytes.slice(ctx.base+100,ctx.base+140),new Uint8Array(packet)); observed.writeUInt32LE(ctx.view.getUint32(ctx.base+120,true),offset); expectedArena.set(packet,100);}
    assert.deepEqual(observed,expectedPage); assert.deepEqual(arena(ctx),expectedArena);
    const file = `${ctx.label}-${label}-${address.toString(16)}-page.bin`; writeFileSync(join(output,file),observed); pageChecks++; records.push({address,bytes:4096,file,sha256:hash(observed)});
  }
  observations.push({context:ctx.ordinal,label,pages:records}); return records;
}
function seed(ctx, registers, pc, flags) {refresh(ctx).bytes.set(state(registers,pc,flags),ctx.base); ctx.bytes.set(exit(3,0,1),ctx.base+56); ctx.view.setUint32(ctx.base+96,0,true); ctx.bytes.fill(0xa5,ctx.base+100,ctx.base+SIZE);}
function shape(ctx, bytes, binding, helpers, gates) {
  let at=8;
  const u=()=>{let value=0,shift=0; for(let n=0;n<5;n++){assert.ok(at<bytes.length); const byte=bytes[at++]; value|=(byte&127)<<shift; if(!(byte&128)) return value>>>0; shift+=7;} assert.fail('bounded LEB');};
  const s=()=>{let value=0n,shift=0n,byte; do{assert.ok(at<bytes.length&&shift<35n);byte=bytes[at++];value|=BigInt(byte&127)<<shift;shift+=7n;}while(byte&128);if(byte&64)value-=1n<<shift;return Number(BigInt.asUintN(32,value));};
  const name=()=>{const length=u(),end=at+length;assert.ok(end<=bytes.length);const value=Buffer.from(bytes.subarray(at,end)).toString();at=end;return value;};
  const types=[], imports=[], functions=[], exports=[]; let locals,guard;
  assert.deepEqual([...bytes.subarray(0,8)],[0,97,115,109,1,0,0,0]);
  while(at<bytes.length) {
    const section=bytes[at++],length=u(),end=at+length;assert.ok(end<=bytes.length);
    if(section===1) for(let n=u();n>0;n--){assert.equal(bytes[at++],0x60);types.push({parameters:Array.from({length:u()},()=>bytes[at++]),results:Array.from({length:u()},()=>bytes[at++])});}
    else if(section===2) for(let n=u();n>0;n--){const module=name(),field=name(),kind=bytes[at++];if(kind===0)imports.push({module,name:field,kind:'function',type:u()});else{assert.equal(kind,2);assert.equal(u(),0);assert.equal(u(),1);imports.push({module,name:field,kind:'memory'});}}
    else if(section===3) for(let n=u();n>0;n--)functions.push(u());
    else if(section===7) for(let n=u();n>0;n--)exports.push({name:name(),kind:bytes[at++],index:u()});
    else if(section===10) {assert.equal(u(),1);const length=u();assert.equal(at+length,end);locals=Array.from({length:u()},()=>[u(),bytes[at++]]);assert.deepEqual(locals,helpers.length>1||gates?[[16,127],[1,126],[6,127]]:[[16,127],[1,126]]);
      const constants=[ctx.low,ctx.high,...(ctx.owner==='replacement'?[binding.generation]:[binding.low,binding.high])];for(const value of constants){assert.equal(bytes[at++],0x41);assert.equal(s(),value);}for(const value of [0,1,3]){assert.equal(bytes[at++],0x20);assert.equal(u(),value);}assert.equal(bytes[at++],0x10);assert.equal(u(),0);guard={constants,parameters:[0,1,3]};at=end;}
    else assert.fail(`unexpected child section${section}`); assert.equal(at,end);
  }
  assert.deepEqual(functions,[0]); assert.deepEqual(types[0],{parameters:[127,127,127,127],results:[127]}); assert.deepEqual(exports,[{name:'run',kind:0,index:helpers.length}]);
  assert.deepEqual(imports[0],{module:'env',name:'memory',kind:'memory'}); assert.deepEqual(imports.slice(1).map(({module,name,kind})=>({module,name,kind})),helpers.map(name=>({module:'ring3',name,kind:'function'})));
  const signatures=Object.fromEntries(imports.slice(1).map(row=>{const signature={parameters:Array(arities[row.name]).fill(127),results:[127]};assert.deepEqual(types[row.type],signature);return [row.name,signature];})); assert.ok(locals&&guard); return {locals,guard,signatures};
}
function compile(ctx, entries, gates, label, extra) {
  request(ctx,words([...entries,...gates.flat()])); let binding;
  if(ctx.owner==='replacement') {call(ctx,'compile_entries',[entries.length,gates.length]); binding={generation:call(ctx,'generation',[],null),pointer:call(ctx,'module_ptr',[],null)>>>0,length:call(ctx,'module_len',[],null)>>>0};}
  else {assert.ok(++ctx.units<=8);call(ctx,'compile_resident_entries',[entries.length,gates.length],0,()=>{refresh(ctx);const at=ctx.base+TRANSFER;assert.equal(ctx.view.getUint32(at,true),1);assert.equal(ctx.view.getUint32(at+4,true),24);binding={low:ctx.view.getUint32(at+8,true),high:ctx.view.getUint32(at+12,true),pointer:ctx.view.getUint32(at+16,true),length:ctx.view.getUint32(at+20,true)};assert.ok(binding.low||binding.high);return [[TRANSFER,words([1,24,binding.low,binding.high,binding.pointer,binding.length])]];});}
  refresh(ctx);assert.ok(binding.pointer>0&&binding.length>8&&binding.length<=65536&&binding.pointer+binding.length<=ctx.bytes.length);
  const bytes=ctx.bytes.slice(binding.pointer,binding.pointer+binding.length),helpers=[ctx.owner==='replacement'?'guard':'guard_resident',...extra],meta=shape(ctx,bytes,binding,helpers,gates.length>0);
  assert.ok(WebAssembly.validate(bytes));const module=new WebAssembly.Module(bytes);assert.deepEqual(WebAssembly.Module.imports(module),[{module:'env',name:'memory',kind:'memory'},...helpers.map(name=>({module:'ring3',name,kind:'function'}))]);assert.deepEqual(WebAssembly.Module.exports(module),[{name:'run',kind:'function'}]);
  const instance=new WebAssembly.Instance(module,{env:{memory:ctx.memory},ring3:ctx.api});assert.equal(instance.exports.run.length,4);writeFileSync(join(output,`${label}.wasm`),bytes);modules.push({context:ctx.ordinal,label,owner:ctx.owner,entries,gates,...binding,...meta,sha256:hash(bytes)});return {...binding,bytes,run:instance.exports.run,label};
}
const storeName=ctx=>ctx.owner==='replacement'?'store8':'store_resident8';
function guard(ctx, unit, status=0) {call(ctx,ctx.owner==='replacement'?'guard':'guard_resident',ctx.owner==='replacement'?[ctx.low,ctx.high,unit.generation,ctx.base,ctx.base+56,ctx.base+96]:[ctx.low,ctx.high,unit.low,unit.high,ctx.base,ctx.base+56,ctx.base+96],status);refresh(ctx);assert.deepEqual(ctx.bytes.slice(unit.pointer,unit.pointer+unit.length),unit.bytes);}
function run(ctx, unit, label, budget, registers, pc, flags, reason, retired, helperPacket=null, version=2, detail=0, address=0, access=0, length=0) {
  const before=arena(ctx),expected=before.slice();expected.set(state(registers,pc,flags));expected.set(exit(reason,retired,version,detail,address,access,length),56);if(helperPacket)expected.set(helperPacket,100);
  assert.equal(unit.run(ctx.base,ctx.base+56,budget,ctx.base+96),0);const observed=arena(ctx);assert.deepEqual(observed,expected,label);
  const file=`${ctx.label}-${label}-arena.bin`;writeFileSync(join(output,file),observed);runs.push({context:ctx.ordinal,module:unit.label,label,budget,status:0,retired,pc,flags,registers:[...registers],reason,version,detail,address,access,length,arena:file,sha256:hash(observed)});
}
function refusedRun(ctx, unit, label, status) {const before=arena(ctx);assert.equal(unit.run(0xffffffff,0xffffffff,0,0xffffffff),status);assert.deepEqual(arena(ctx),before);runs.push({context:ctx.ordinal,module:unit.label,label,status,retired:0});}
function codePage(code) {const page=Buffer.alloc(4096,0xcc);page.set(code);return page;}
const moffs=(op,address)=>Buffer.concat([Buffer.from([op]),words([address])]);

for(const owner of ['replacement','resident']) for(const [index,item] of finite.entries()) {
  const ctx=fresh(owner,`${owner}-finite-${index}`),code=Buffer.concat([moffs(item.op,item.address),Buffer.from([0xeb,0])]),page=item.address-item.address%4096,data=Buffer.alloc(4096,0x6d);
  data[item.address%4096]=item.op===0xa0?item.value:0x6d;mapPage(ctx,0x1000,codePage(code),5);mapPage(ctx,page,data);
  if(item.address===0xffffffff)mapPage(ctx,0,Buffer.alloc(4096,0x5a),1);
  const unit=compile(ctx,[0x1000],[],`${ctx.label}-mov`,[item.op===0xa0?'read8':storeName(ctx)]);readPages(ctx,'before');call(ctx,'protect',[page,1,item.op===0xa0?1:2]);
  const registers=[...initialRegisters];registers[0]=item.eax;seed(ctx,registers,0x1000,item.flags);registers[0]=item.result;
  run(ctx,unit,'mov',1,registers,0x1005,item.flags,1,1,item.op===0xa0?readHelper(item.value):storeHelper());
  if(item.op===0xa2){ctx.pages.get(page)[item.address%4096]=item.value;call(ctx,'protect',[page,1,3]);}readPages(ctx,'after');guard(ctx,unit);
  rows.push({kind:'finite',owner,input:item,pc:0x1000,length:5,next_pc:0x1005,retired:1,write_only_store:item.op===0xa2,zero_page_wrap_canary:item.address===0xffffffff});call(ctx,'close');
}
for(const owner of ['replacement','resident']) for(const op of [0xa0,0xa2]) {
  const ctx=fresh(owner,`${owner}-retry-${op.toString(16)}`),code=Buffer.concat([Buffer.from('b8805634123c01','hex'),moffs(op,0x5010),Buffer.from([0xeb,0])]);mapPage(ctx,0x9000,codePage(code),5);
  const unit=compile(ctx,[0x9000],[],`${ctx.label}-prefix`,[op===0xa0?'read8':storeName(ctx)]),registers=[...initialRegisters];readPages(ctx,'before');seed(ctx,registers,0x9000,0x0402);
  registers[0]=0x12345680;const access=op===0xa0?1:2,fault=reason=>op===0xa0?readHelper(0,reason,0x5010):storeHelper(reason,0x5010);
  run(ctx,unit,'unmapped',8,registers,0x9007,0x0c12,5,2,fault(1),2,1,0x5010,access,1);readPages(ctx,'unmapped');
  if(op===0xa0){run(ctx,unit,'budget0',0,registers,0x9007,0x0c12,1,0);refresh(ctx).view.setUint32(ctx.base+96,1,true);run(ctx,unit,'cancel',8,registers,0x9007,0x0c12,2,0);refresh(ctx).view.setUint32(ctx.base+96,0,true);}
  mapPage(ctx,0x5000,Buffer.alloc(4096,0x6d),op===0xa0?2:1);guard(ctx,unit);
  run(ctx,unit,'permission',8,registers,0x9007,0x0c12,5,0,fault(2),2,2,0x5010,access,1);
  if(op===0xa0)call(ctx,'protect',[0x5000,1,3]);readPages(ctx,'permission');if(op===0xa0)call(ctx,'protect',[0x5000,1,2]);
  call(ctx,'protect',[0x5000,1,op===0xa0?1:2]);guard(ctx,unit);if(op===0xa0)registers[0]=0x1234566d;
  run(ctx,unit,'retry',1,registers,0x900c,0x0c12,1,1,op===0xa0?readHelper(0x6d):storeHelper());
  if(op===0xa2){ctx.pages.get(0x5000)[16]=0x80;call(ctx,'protect',[0x5000,1,3]);}readPages(ctx,'retry');guard(ctx,unit);
  rows.push({kind:'retry',owner,op,prefix_hex:'b8805634123c01',prefix_retired_once:2,fault_pc:0x9007,flags:0x0c12,unmapped_retired:2,permission_retired:0,retry_retired:1,initial_seed_count:1,producer_compile_count:1,repairs:'map/upload/protect only; no CPU/FLAGS/EIP reseed, prefix replay or producer recompile'});call(ctx,'close');
}
for(const owner of ['replacement','resident']) for(const same of [true,false]) {
  const value=same?0x88:0xff,flags=same?0x0486:0x0482,ctx=fresh(owner,`${owner}-smc-${same?'same':'changed'}`);
  const code=Buffer.concat([Buffer.from([0xb8,value,0x56,0x34,0x12,0x3c,0x01]),moffs(0xa2,0x900d),Buffer.from([0xb1,0x88,0xeb,0])]);
  mapPage(ctx,0x9000,codePage(code));mapPage(ctx,0xb000,codePage(Buffer.from([0x90,0xeb,0])),5);
  const keeperCtx={...ctx,owner:'resident'},keeper=compile(keeperCtx,[0xb000],[],`${ctx.label}-keeper`,[]);ctx.units=keeperCtx.units;
  const writer=compile(ctx,[0x9000],[],`${ctx.label}-writer`,[storeName(ctx)]),registers=[...initialRegisters];readPages(ctx,'before');seed(ctx,registers,0x9000,0x0402);registers[0]=0x12345600+value;
  run(ctx,writer,'commit',8,registers,0x900c,flags,6,3,storeHelper());ctx.pages.get(0x9000)[13]=value;readPages(ctx,'commit');guard(ctx,writer,4);guard(keeperCtx,keeper);
  refusedRun(ctx,writer,'stale-before-malformed-budget0',4);const successor=compile(ctx,[0x900c],[],`${ctx.label}-successor`,[]);guard(keeperCtx,keeper);
  registers[1]=0x23456700+value;run(ctx,successor,'successor',2,registers,0x9010,flags,1,2,null,1);readPages(ctx,'successor');guard(ctx,successor);guard(keeperCtx,keeper);
  rows.push({kind:'smc',owner,same,old_byte:0x88,stored_byte:value,target:0x900d,next_pc:0x900c,commit_retired:3,successor_retired:2,helper_status11:'inferred from existing strict Store8 validation/Invalidated exit; no raw helper return capture',keeper:keeper.label,successor:successor.label,initial_seed_count:1});call(ctx,'close');
}

// fresh independent 51-byte program: dirty SF survives A2/MOV AL,0/A0; A0 must reload FF.
const program=Buffer.from('b8ff5634123c01a200204000b000a00020400079113cff750d68c1ff77aaff15502140000f0b68efbeaddeff15502140000f0b','hex');
assert.equal(program.length,51);const callerOffsets=[0,21,25,38],relocations=[[8,0x2000],[15,0x2000],[32,0x2150],[45,0x2150]];
for(const [at,op,target,fallthrough] of [[19,0x79,38,21],[23,0x75,38,25]]) {assert.equal(program[at],op);assert.equal(at+2,fallthrough);assert.equal(at+2+program.readInt8(at+1),target);assert.ok(callerOffsets.includes(target)&&callerOffsets.includes(fallthrough));}
function imageFile() {
  const image=Buffer.alloc(2048);image.write('MZ');image.writeUInt32LE(0x80,0x3c);image.write('PE\0\0',0x80);
  for(const [at,value] of [[0x84,0x14c],[0x86,3],[0x94,224],[0x96,0x102],[0x98,0x10b],[0xdc,3],[0xde,0x100]])image.writeUInt16LE(value,at);
  for(const [at,value] of [[4,512],[8,1024],[16,0x1000],[20,0x1000],[24,0x2000],[28,0x400000],[32,4096],[36,512],[56,0x5000],[60,512],[72,0x100000],[76,4096],[80,0x100000],[84,4096],[92,16],[104,0x2100],[108,40],[136,0x4000],[140,16],[192,0x2150],[196,8]])image.writeUInt32LE(value,0x98+at);
  for(const [index,name,size,rva,raw,flags] of [[0,'.text',51,0x1000,0x200,0x60000020],[1,'.data',512,0x2000,0x400,0xc0000040],[2,'.fixups',16,0x4000,0x600,0x40000040]]){const at=0x178+index*40;image.write(name,at);for(const [offset,value] of [[8,size],[12,rva],[16,512],[20,raw],[36,flags]])image.writeUInt32LE(value,at+offset);}
  image.fill(0xcc,0x200,0x400);image.set(program,0x200);image.fill(0x6d,0x400,0x440);image.set(words([0x2140,0,0,0x2160,0x2150]),0x500);image.writeUInt32LE(0x2170,0x540);image.writeUInt32LE(0x2170,0x550);image.write('kernel32.dll\0',0x560);image.write('ExitProcess\0',0x572);
  image.writeUInt32LE(0x1000,0x600);image.writeUInt32LE(16,0x604);relocations.forEach(([offset],index)=>image.writeUInt16LE(0x3000+offset,0x608+index*2));return image;
}
const image=imageFile();writeFileSync(join(output,'moffs.exe'),image);writeFileSync(join(output,'program.x86'),program);
for(const base of [0x400000,0x500000]) for(const owner of ['replacement','resident']) {
  const ctx=fresh(owner,`pe-${owner}-${base.toString(16)}`),entry=base+0x1000;call(ctx,'begin_image_input',[image.length]);request(ctx,image);call(ctx,'append_image_input',[0,image.length]);refresh(ctx).bytes.fill(0xa5,ctx.base+TRANSFER,ctx.base+SIZE);
  const receipt=record('R3LI',72,[base,0x5000,entry,5,0x8000,1,0,0,EXIT,EXIT_ID,0,0,0,0],2);call(ctx,'load_pe32_linked_v2_input_at',[base,0x8000],0,[[TRANSFER,receipt]]);writeFileSync(join(output,`${ctx.label}-receipt.bin`),receipt);
  const header=Buffer.alloc(4096),text=Buffer.alloc(4096),data=Buffer.alloc(4096),fixups=Buffer.alloc(4096),gatePage=Buffer.alloc(4096);header.set(image.subarray(0,512));text.set(image.subarray(0x200,0x400));data.set(image.subarray(0x400,0x600));fixups.set(image.subarray(0x600,0x800));gatePage.set([0x0f,0x0b],0x20);
  relocations.forEach(([offset,rva])=>text.writeUInt32LE(base+rva,offset));data.writeUInt32LE(EXIT,0x150);for(const [address,bytes] of [[base,header],[entry,text],[base+0x2000,data],[base+0x4000,fixups],[0x8000,gatePage]])ctx.pages.set(address,bytes);readPages(ctx,'loaded');
  const entries=callerOffsets.map(offset=>entry+offset);if(owner==='replacement')entries.push(EXIT);const gates=[[EXIT,EXIT_ID]],caller=compile(ctx,entries,owner==='replacement'?gates:[],`${ctx.label}-caller`,['read32',owner==='replacement'?'store32':'store_resident32','read8',storeName(ctx)]),gate=owner==='replacement'?caller:compile(ctx,[EXIT],gates,`${ctx.label}-gate`,[]);
  call(ctx,'start_loaded_image',[0x70000,1],0,[[0,state([0,0,0,0,0x71000,0,0,0],entry,2)],[56,exit(3,0,3)]]);ctx.pages.set(0x70000,Buffer.alloc(4096));readPages(ctx,'started');
  const registers=[0x123456ff,0,0,0,0x70ff8,0,0,0];run(ctx,caller,'caller',32,registers,EXIT,0x46,owner==='replacement'?8:3,10,record('R3MH',40,[0,0,0,0,0,0]),owner==='replacement'?3:2,owner==='replacement'?EXIT_ID:0);
  ctx.pages.get(base+0x2000)[0]=0xff;ctx.pages.get(0x70000).writeUInt32LE(entry+36,0xff8);ctx.pages.get(0x70000).writeUInt32LE(MARKER,0xffc);readPages(ctx,'called');
  if(owner==='resident')run(ctx,gate,'gate',1,registers,EXIT,0x46,8,0,null,3,EXIT_ID);
  const frame=record('R3CF',112,[1,EXIT_ID,2,1,EXIT,0x70ff8,entry+36,0,MARKER,...Array(15).fill(0)]);call(ctx,owner==='replacement'?'capture_call':'capture_resident_call',owner==='replacement'?[ctx.low,ctx.high,gate.generation,2,1]:[ctx.low,ctx.high,gate.low,gate.high,2,1],0,[[TRANSFER,frame]]);writeFileSync(join(output,`${ctx.label}-frame.bin`),frame);
  call(ctx,owner==='replacement'?'complete_windows_call':'complete_resident_windows_call',owner==='replacement'?[ctx.low,ctx.high,gate.generation,1]:[ctx.low,ctx.high,gate.low,gate.high,1],0,[[56,exit(9,0,4,MARKER)]]);readPages(ctx,'terminal');refusedRun(ctx,caller,'terminal-before-malformed-budget0',21);
  rows.push({kind:'pe',owner,base,program_hex:program.toString('hex'),program_length:51,caller_offsets:callerOffsets,relocations,normal_instruction_offsets:[0,5,7,12,14,19,21,23,25,30],retired:10,return_pc:entry+36,flags_after_producer:0x82,final_flags:0x46,registers,result:MARKER,canary:'data bytes1..63 remain6d; stack bytes0..4087 remain00',receipt_hex:receipt.toString('hex'),frame_hex:frame.toString('hex')});call(ctx,'close');
}
const counts={contexts,modules:modules.length,finite:rows.filter(row=>row.kind==='finite').length,retry:rows.filter(row=>row.kind==='retry').length,smc:rows.filter(row=>row.kind==='smc').length,pe:rows.filter(row=>row.kind==='pe').length,generated_invocations:runs.length,retired:runs.reduce((sum,row)=>sum+row.retired,0),page_observations:pageChecks,host_read32:read32Calls,other_host_api:hostCalls.length,total_host_api:read32Calls+hostCalls.length,source_inputs:sourcePaths.length};
assert.deepEqual(counts,{contexts:24,modules:34,finite:12,retry:4,smc:4,pe:4,generated_invocations:50,retired:84,page_observations:196,host_read32:200704,other_host_api:360,total_host_api:201064,source_inputs:88});
const afterSources=sources();assert.deepEqual(afterSources,beforeSources);assert.deepEqual(readFileSync(enginePath),engineBytes);writeFileSync(join(output,'source-after.json'),JSON.stringify(afterSources,null,2));
const result={schema:'bounded-accumulator-byte-moffs-v1',counts,engine_sha256:hash(engineBytes),sources:beforeSources,llvm,finite_inputs:finite,rows,modules,runs,observations,host_calls:hostCalls,pe:{bytes:2048,sha256:hash(image),program_length:51,program_hex:program.toString('hex'),caller_offsets:callerOffsets,relocations},limits:['Only unprefixed flat32 A0/A2; no general ISA/OS/address-size/segments/browser/performance/game claim','Numeric expected byte/parent/flags states are authored independently; full raw arenas and complete observed pages are saved for these compact calls','SMC status11 is inferred from strict source protocol plus committed Invalidated exit; not captured as helper return','LLVM checks only the two encoding samples; fresh PE bytes/CFG/relocations are independent literal input','Old unchanged load/store original raw results/provenance are handled by ROOT separately']};
writeFileSync(join(output,'result.json'),JSON.stringify(result,null,2));console.log(JSON.stringify({status:'PASS',output:join(output,'result.json'),counts,engine_sha256:hash(engineBytes)}));
