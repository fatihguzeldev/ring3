import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync, mkdirSync, existsSync} from 'node:fs';
import {join} from 'node:path';
import {pathToFileURL} from 'node:url';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root, 'engine path, output directory and repo root are required');
const {readEngine} = await import(pathToFileURL(join(root, 'engine/tests/support/wasm/engine.mjs')));
assert.match(process.env.RING3_ENGINE_SHA256 ?? '', /^[a-f0-9]{64}$/, 'pin the selected engine artifact');
const engine = readEngine(enginePath);
mkdirSync(output, {recursive: true});
const SIZE = 4796, SEG = 4524, BASES = 4756, TRANSFER = 140, PC = 0x1000, HIGH = 0x5353;
const pages = [0, PC, 0x3000, 0x4000, 0x5000, 0x6000, 0x8000,0x14000,0x15000,0xabcd5000,0xabce5000,0x76545000];
const modules = [], observations = [], contexts = [];
const hash = value => createHash('sha256').update(value).digest('hex');
const sourcePaths = [
  'engine/src/cpu/x86/segments.rs', 'engine/src/cpu/x86/segments/cache.rs',
  'engine/src/cpu/x86/segments/descriptors.rs', 'engine/src/abi/x86/segments.rs',
  'engine/src/cpu/dbt/wasm/instructions/segments/context.rs', 'engine/src/cpu/dbt/wasm/imports/mod.rs', 'engine/src/cpu/dbt/wasm/imports/binding.rs', 'engine/src/cpu/dbt/wasm/imports/context.rs', 'engine/src/cpu/dbt/wasm/imports/needs.rs', 'engine/src/cpu/dbt/wasm/helpers/protocol.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/mod.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/address.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/scalar.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/stack.rs',
  'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/mod.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/arithmetic.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/moves.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/exchange.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/predicates.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/shifts.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/control.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/extended.rs', 'engine/src/cpu/dbt/wasm/instructions/stack.rs',
  'engine/src/cpu/dbt/wasm/instructions/segments.rs', 'engine/src/cpu/dbt/wasm/instructions/control_instructions.rs',
  'engine/src/cpu/dbt/wasm/instructions/far/call.rs', 'engine/src/cpu/dbt/wasm/instructions/far/return.rs',
  'engine/src/cpu/dbt/wasm/instructions/far/interrupt.rs', 'engine/src/process/cpu/stack.rs',
  'engine/src/process/cpu/far/call.rs', 'engine/src/process/cpu/segments.rs',
  'engine/src/process/calls/call.rs', 'engine/src/process/calls/callback.rs',
  'engine/src/process/calls/resident_callback.rs',
  'engine/src/cpu/x86/stack.rs','engine/src/cpu/x86/stack/pointer.rs',
  'engine/src/cpu/dbt/wasm/instructions/stack/pointer.rs',
  'engine/src/cpu/dbt/wasm/emitter/mod.rs', 'engine/src/cpu/dbt/wasm/emitter/module.rs', 'engine/src/cpu/dbt/wasm/emitter/body.rs', 'engine/src/cpu/dbt/wasm/requirements.rs', 'engine/src/cpu/dbt/wasm/imports/wiring.rs', 'engine/src/cpu/dbt/wasm/code_fetch.rs',
  'engine/tests/support/wasm/engine.mjs', 'engine/tests/support/wasm.rs',
  'engine/tests/cpu/segments/native.rs', 'engine/tests/cpu/segments/wasm.rs',
];
const pins = () => ({...Object.fromEntries(sourcePaths.filter(path => existsSync(join(root,path))).map(path => [path, hash(readFileSync(join(root, path)))])),
  executed_probe: hash(readFileSync(new URL(import.meta.url)))});
const sourcePins = pins();
const missingSources = sourcePaths.filter(path => !existsSync(join(root,path)));
const words = values => {
  const bytes = Buffer.alloc(values.length * 4);
  values.forEach((value, index) => bytes.writeUInt32LE(value >>> 0, index * 4)); return bytes;
};
function record(magic, size, values, version = 1) {
  const bytes = Buffer.alloc(size); bytes.write(magic); bytes.writeUInt16LE(version, 4);
  bytes.writeUInt16LE(1, 6); bytes.writeUInt32LE(size, 8); bytes.set(words(values), 16); return bytes;
}
const arena = c => Buffer.from(new Uint8Array(c.memory.buffer, c.base, SIZE));
const input = (c, offset, bytes) => new Uint8Array(c.memory.buffer).set(bytes, c.base + offset);
function host(c, name, values = []) { assert.equal(c.api[name](...values), 0, name); }
function upload(c, address, bytes) {
  input(c, TRANSFER, bytes); host(c, 'upload', [address >>> 0, bytes.length]);
}
function readRam(c) {
  const saved = arena(c), bytes = Buffer.alloc(pages.length * 4096);
  pages.forEach((page, index) => {
    for (let offset = 0; offset < 4096; offset += 4) {
      host(c, 'read32', [(page + offset) >>> 0]);
      const packet = arena(c).subarray(100, 140);
      assert.equal(packet.readUInt32LE(16), 0);
      bytes.writeUInt32LE(packet.readUInt32LE(20), index * 4096 + offset);
    }
  });
  const observed = arena(c), expected = Buffer.from(saved); expected.set(observed.subarray(100, 140), 100);
  assert.deepEqual(observed, expected, 'RAM observer may only publish its helper packet');
  input(c, 100, saved.subarray(100, 140)); return bytes;
}
function open(row, owner) {
  const instance = new WebAssembly.Instance(engine.module, {});
  const c = {memory: instance.exports.memory, api: {}, trace: [], id: contexts.length + 1, owner};
  for (const [name, value] of Object.entries(instance.exports))
    if (name.startsWith('ring3_abi_v1_') && typeof value === 'function') c.api[name.slice(13)] = value;
  host(c, 'open', [16, c.id, HIGH]); c.base = c.api.arena_ptr() >>> 0;
  for (const page of pages) host(c, 'map', [page, 1, page === PC ? 7 : 3]);
  host(c, 'set_segment_base', [2, row.base ?? 0x5000]);
  if (row.descriptor) host(c, 'configure_segment_descriptor_raw', row.descriptor);
  upload(c, PC, Buffer.from(row.code + 'ebfe', 'hex'));
  const resident = owner === 'resident-entries';
  input(c, TRANSFER, words(resident ? [PC] : [PC, row.code.length / 2 + (row.terminal ? 0 : 2)]));
  host(c, resident ? 'compile_resident_entries' : 'compile', resident ? [1, 0] : [1]);
  const compiled = arena(c);
  const pointer = resident ? compiled.readUInt32LE(156) : c.api.module_ptr() >>> 0;
  const length = resident ? compiled.readUInt32LE(160) : c.api.module_len() >>> 0;
  const bytes = Buffer.from(new Uint8Array(c.memory.buffer, pointer, length));
  const module = new WebAssembly.Module(bytes), imports = WebAssembly.Module.imports(module), ring3 = {...c.api};
  for (const imp of imports.filter(imp => imp.module === 'ring3' && !imp.name.startsWith('guard') && imp.name !== 'finish_exception_detection')) {
    ring3[imp.name] = (...args) => {
      c.trace.push({name: imp.name, args: args.map(value => value >>> 0)});
      const status = c.api[imp.name](...args);
      if (row.forgeFaultAddress !== undefined && imp.name.startsWith('far_call')) {
        const address = Buffer.alloc(4); address.writeUInt32LE(row.forgeFaultAddress);
        input(c, 128, address);
      }
      return status;
    };
  }
  c.run = new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3}).exports.run;
  const file = `module-${c.id}.wasm`; writeFileSync(join(output, file), bytes, {flag: 'wx'});
  modules.push({context: c.id, file, sha256: hash(bytes), imports});
  if (!row.b32) {
    const cache=Buffer.alloc(4);cache.writeUInt32LE(0xf13fffff);input(c,SEG+36,cache);
  }
  contexts.push({id: c.id, owner, label: row.label, bytes: row.code}); return c;
}
const initial = [0x12345678,0x11112222,0x33334444,0x55556666,0xabcd0024,0x76540024,0x77778888,0x9999aaaa];
const update = changes => Object.assign([...initial], changes);
const cases = [
  {label:'push-eax-B16-dword',code:'50',end:update({4:0xabcd0020}),writes:[[0x5020,[0x12345678]]]},
  {label:'push-original-full-ESP',code:'54',end:update({4:0xabcd0020}),writes:[[0x5020,[0xabcd0024]]]},
  {label:'push-word-B16',code:'6650',end:update({4:0xabcd0022}),rawWrites:[[0x5022,'7856']]},
  {label:'word-pop-SP-wrap',code:'6658',regs:update({4:0xabcdfffe}),seed:[[0x14ffe,[0x1234]]],end:update({0:0x12341234,4:0xabcd0000})},
  {label:'pop-SP-destination-after-increment',code:'665c',regs:update({4:0xabcdfffe}),seed:[[0x14ffe,[0x1234]]],end:update({4:0xabcd1234})},
  {label:'pop-ESP-overwrites-complete-pointer',code:'5c',seed:[[0x5024,[0x11223344]]],end:update({4:0x11223344})},
  {label:'pop-memory-keeps-explicit-full-ESP',code:'8f442404',seed:[[0x5024,[0xa1b2c3d4]]],end:update({4:0xabcd0028}),writes:[[0xabcd502c,[0xa1b2c3d4]]]},
  {label:'wrapped-pop-memory-declared-Ring3-policy',code:'668f0424',regs:update({4:0xabcdfffe}),seed:[[0x14ffe,[0x1234]]],end:update({4:0xabcd0000}),rawWrites:[[0xabcd5000,'3412']]},
  {label:'ordinary-explicit-ESP-stays-address32',code:'8b1424',seed:[[0xabcd5024,[0xabcdef01]],[0x5024,[0x22222222]]],end:update({2:0xabcdef01})},
  {label:'address16-BP-explicit-override',code:'678b5600',seed:[[0x5024,[0xabcdef01]]],end:update({2:0xabcdef01})},
  {label:'leave-dword-B16',code:'c9',regs:update({4:0xabcd0040}),seed:[[0x5024,[0x11112222]]],end:update({4:0xabcd0028,5:0x11112222})},
  {label:'leave-word-B16',code:'66c9',regs:update({4:0xabcd0040}),seed:[[0x5024,[0xa1b2c3d4]]],end:update({4:0xabcd0026,5:0x7654c3d4})},
  {label:'pusha-dword-B16',code:'60',end:update({4:0xabcd0004}),writes:[[0x5004,[0x9999aaaa,0x77778888,0x76540024,0xabcd0024,0x55556666,0x33334444,0x11112222,0x12345678]]]},
  {label:'pusha-word-B16',code:'6660',end:update({4:0xabcd0014}),rawWrites:[[0x5014,'aaaa8888240024006666444422227856']]},
  {label:'popa-dword-B16',code:'61',seed:[[0x5024,[1,2,3,4,5,6,7,8]]],end:[8,7,6,5,0xabcd0044,3,2,1]},
  {label:'same-width-enter16-B16-overlay',code:'66c8100003',end:update({4:0xabcd000c,5:0x76540022}),rawWrites:[[0x501c,'2200240024002400']]},
  {label:'mixed-enter32-B16-full-frame-overlay',code:'c8100003',end:update({4:0xabcd0004,5:0xabcd0020}),writes:[[0x5014,[0xabcd0020,0x76540024,0x76540024,0x76540024]]]},
  {label:'mixed-enter32-leave32-roundtrip-with-high-halves',code:'c8100003c9',budget:2,end:update({}),writes:[[0x5014,[0xabcd0020,0x76540024,0x76540024,0x76540024]]]},
  {label:'mixed-enter32-retires-clears-RF-consumes-SS-shadow',code:'c8000000',initialFlags:0x10002,metadata:1,endMetadata:0,end:update({4:0xabcd0020,5:0xabcd0020}),flags:2,writes:[[0x5020,[0x76540024]]]},
  {label:'mixed-enter32-active-allocation-borrow',code:'c8200000',regs:update({4:0xabcd0010,5:0x76540044}),end:update({4:0xabcdffec,5:0xabcd000c}),writes:[[0x500c,[0x76540044]]]},
  {label:'mixed-enter32-parent-low-BP-high-decoy',code:'c8000002',regs:update({4:0xabcd0020,5:0x76540010}),seed:[[0x500c,[0x11112222]],[0x7654500c,[0x33334444]]],end:update({4:0xabcd0014,5:0xabcd001c}),writes:[[0x5014,[0xabcd001c,0x11112222,0x76540010]]]},
  {label:'mixed-enter32-wrap-between-parent-and-frame-slots',code:'c8000002',regs:update({4:0xabcd0004,5:0x76540004}),end:update({4:0xabcdfff8,5:0xabcd0000}),writes:[[0x14ff8,[0xabcd0000,0x76540004]],[0x5000,[0x76540004]]]},
  {label:'mixed-enter32-address67-keeps-active-parent-width',code:'67c8000002',regs:update({4:0xabcd0020,5:0x76540010}),seed:[[0x500c,[0x11112222]],[0x7654500c,[0x33334444]]],end:update({4:0xabcd0014,5:0xabcd001c}),writes:[[0x5014,[0xabcd001c,0x11112222,0x76540010]]]},
  {label:'mixed-enter32-late-parent-SS-fault-atomic',code:'c8000002',cache:0xf0300030,regs:update({5:0x76540040}),end:update({5:0x76540040}),pc:PC,retired:0,fault:[15,0,0,0,0]},
  {label:'mixed-enter32-final-allocation-PF-atomic',code:'c8080000',regs:update({4:0xabcd1004}),protect:[0x5000,1,0],end:update({4:0xabcd1004}),pc:PC,retired:0,fault:[5,2,0x5ff8,2,4]},
  {label:'mixed-enter32-late-parent-PF-atomic',code:'c8000002',regs:update({4:0xabcd1004,5:0x76541000}),protect:[0x5000,1,0],end:update({4:0xabcd1004,5:0x76541000}),pc:PC,retired:0,fault:[5,2,0x5ffc,1,4]},
  {label:'mixed-enter32-final-whole-span-crosses-ffff',code:'c80d0000',regs:update({4:0xabcd0010,5:0x76540044}),end:update({4:0xabcdffff,5:0xabcd000c}),writes:[[0x500c,[0x76540044]]]},
  {label:'mixed-enter32-expand-down-final-allocation-SS-fault',code:'c8010000',cache:0xf0700fff,regs:update({4:0xabcd1004}),end:update({4:0xabcd1004}),pc:PC,retired:0,fault:[15,0,0,0,0]},
  {label:'mixed-enter32-maximum-masked-nesting31',code:'c80000ff',regs:update({4:0xabcd0080,5:0x76540080}),end:update({4:0xabcd0000,5:0xabcd007c}),writes:[[0x5000,[0xabcd007c,...Array(31).fill(0x76540080)]]]},
  {label:'near-call-B16',terminal:true,code:'e800000000',end:update({4:0xabcd0020}),writes:[[0x5020,[0x1005]]]},
  {label:'near-return-B16',terminal:true,code:'c3',seed:[[0x5024,[PC]]],end:update({4:0xabcd0028}),pc:PC},
  {label:'far-call-B16-wrap-between-slots',terminal:true,code:'9a001000001b00',regs:update({4:0xabcd0004}),end:update({4:0xabcdfffc}),pc:PC,writes:[[0x14ffc,[0x1007]],[0x5000,[0x1b]]]},
  {label:'far-return-B16-wrap-between-slots',terminal:true,code:'cb',regs:update({4:0xabcdfffc}),seed:[[0x14ffc,[PC]],[0x5000,[0x1b]]],end:update({4:0xabcd0004}),pc:PC},
  {label:'iret-B16-wrap-between-slots',terminal:true,code:'cf',regs:update({4:0xabcdfff8}),seed:[[0x14ff8,[PC,0x1b]],[0x5000,[0x46]]],end:update({4:0xabcd0004}),pc:PC,flags:0x46},
  {label:'normal-B16-single-span-can-cross-ffff',code:'6658',regs:update({4:0xabcdffff}),seed:[[0x14fff,[0x1234]]],end:update({0:0x12341234,4:0xabcd0001})},
  {label:'expand-down-B16-whole-span-retains-ffff-upper-bound',code:'58',cache:0xf0700fff,regs:update({4:0xabcdfffe}),end:update({4:0xabcdfffe}),pc:PC,retired:0,fault:[15,0,0,0,0]},
  {label:'normal-B16-loaded-limit-is-authority',code:'6658',cache:0xf030ffff,regs:update({4:0xabcdffff}),end:update({4:0xabcdffff}),pc:PC,retired:0,fault:[15,0,0,0,0]},
  {label:'word-push-wrap-preserves-high-ESP',code:'6654',regs:update({4:0xabcd0000}),end:update({4:0xabcdfffe}),rawWrites:[[0x14ffe,'0000']]},
  {label:'live-MOV-SS-B32-to-B16-then-PUSH',code:'8ed053',b32:true,regs:update({0:0x43}),end:update({0:0x43,4:0xabcd0020}),budget:2,descriptor:[0x43,0x5000ffff,0x0000f200],writes:[[0x5020,[0x55556666]]],ss:[0x43,0x5000,0xf030ffff]},
  {label:'POP-SS-B16-to-B32-uses-old-pointer-mode',code:'17',regs:update({4:0xabcdfffc}),seed:[[0x14ffc,[0x43]]],end:update({4:0xabcd0000}),endMetadata:1,descriptor:[0x43,0x6000ffff,0x0040f200],ss:[0x43,0x6000,0xf230ffff]},
  {label:'POP-SS-B32-to-B16-uses-old-pointer-mode',code:'17',b32:true,regs:update({4:0xfffffffc}),seed:[[0x4ffc,[0x43]]],end:update({4:0}),endMetadata:1,descriptor:[0x43,0x6000ffff,0x0000f200],ss:[0x43,0x6000,0xf030ffff]},
  {label:'push-memory-explicit-ESP-versus-implicit-SP',code:'ff3424',seed:[[0xabcd5024,[0xa1b2c3d4]],[0x5024,[0x22222222]]],end:update({4:0xabcd0020}),writes:[[0x5020,[0xa1b2c3d4]]]},
  {label:'call-memory-explicit-ESP-versus-implicit-SP',terminal:true,code:'ff1424',seed:[[0xabcd5024,[PC]],[0x5024,[0x22222222]]],end:update({4:0xabcd0020}),pc:PC,writes:[[0x5020,[0x1003]]]},
  {label:'push-flags-dword-B16',code:'9c',end:update({4:0xabcd0020}),writes:[[0x5020,[2]]]},
  {label:'pop-flags-word-B16-wrap',code:'669d',regs:update({4:0xabcdfffe}),seed:[[0x14ffe,[0x46]]],end:update({4:0xabcd0000}),flags:0x46},
  {label:'push-segment-word-B16-wrap',code:'661e',regs:update({4:0xabcd0000}),end:update({4:0xabcdfffe}),rawWrites:[[0x14ffe,'2300']]},
  {label:'near-call-word-B16',terminal:true,code:'66e80000',end:update({4:0xabcd0022}),rawWrites:[[0x5022,'0410']]},
  {label:'near-return-word-B16-wrap',terminal:true,code:'66c3',regs:update({4:0xabcdfffe}),seed:[[0x14ffe,[PC]]],end:update({4:0xabcd0000}),pc:PC},
  {label:'far-call-word-B16-wrap',terminal:true,code:'669a00101b00',regs:update({4:0xabcd0002}),end:update({4:0xabcdfffe}),pc:PC,rawWrites:[[0x14ffe,'0610'],[0x5000,'1b00']]},
  {label:'far-return-word-B16-wrap',terminal:true,code:'66cb',regs:update({4:0xabcdfffe}),seed:[[0x14ffe,[PC]],[0x5000,[0x1b]]],end:update({4:0xabcd0002}),pc:PC},
  {label:'iret-word-B16-wrap',terminal:true,code:'66cf',regs:update({4:0xabcdfffc}),seed:[[0x14ffc,[0x001b1000]],[0x5000,[0x46]]],end:update({4:0xabcd0002}),pc:PC,flags:0x46},
  {label:'B16-bulk-late-page-fault-remains-atomic',code:'60',regs:update({4:0xabcd1004}),protect:[0x5000,1,0],end:update({4:0xabcd1004}),pc:PC,retired:0,fault:[5,2,0x5ffc,2,4]},
];
for (const owner of ['replacement-range', 'resident-entries']) {
  for (const row of cases) {
    const c = open(row, owner);
    for (const [address, values] of row.seed ?? []) upload(c, address, words(values));
    const regs = row.regs ?? initial;
    const stateInput=record('R3ST',56,[...regs,PC,row.initialFlags ?? 2],row.initialFlags ? 3:1);
    stateInput.writeUInt32LE(row.metadata ?? 0,12);input(c,0,stateInput);input(c,96,words([0]));
    if (row.cache !== undefined) {
      const cache = Buffer.alloc(4); cache.writeUInt32LE(row.cache); input(c, SEG + 36, cache);
    }
    const before = arena(c), beforeRam = readRam(c), expectedRam = Buffer.from(beforeRam);
    if (row.protect) host(c, 'protect', row.protect);
    for (const [address, values] of row.writes ?? []) {
      const page = pages.indexOf((address & ~4095) >>> 0); assert.notEqual(page, -1);
      expectedRam.set(words(values), page * 4096 + (address & 4095));
    }
    for (const [address, hex] of row.rawWrites ?? []) {
      const page = pages.indexOf((address & ~4095) >>> 0); assert.notEqual(page, -1);
      expectedRam.set(Buffer.from(hex, 'hex'), page * 4096 + (address & 4095));
    }
    assert.equal(c.run(c.base, c.base + 56, row.budget ?? 1, c.base + 96), 0, row.label);
    const after = arena(c), state = after.subarray(0, 56), exit = after.subarray(56, 96);
    assert.deepEqual(Array.from({length:8}, (_, i) => state.readUInt32LE(16 + i * 4)), row.end, row.label + ' logical registers');
    assert.equal(state.readUInt32LE(48), row.pc ?? PC + row.code.length / 2, row.label + ' EIP');
    assert.equal(state.readUInt32LE(52), row.flags ?? 2, row.label + ' FLAGS');
    assert.equal(state.readUInt32LE(12), row.endMetadata ?? 0, row.label + ' no retained SS debug shadow');
    let [reason, detail, address, access, length] = row.fault ?? [1,0,0,0,0];
    if (reason === 5) {reason = 23; detail = 4 | (access === 2 ? 2 : 0) | (detail === 2 ? 1 : 0);}
    assert.deepEqual(Array.from({length:6}, (_, i) => exit.readUInt32LE(16 + i * 4)),
      [reason, row.retired ?? row.budget ?? 1, detail,address,access,length], row.label + ' exit packet');
    const expected = Buffer.from(before);
    for (const [at, length] of [[0,96], [100,40], [140,32]]) expected.set(after.subarray(at, at + length), at);
    if (row.ss) {
      const [selector, base, cache] = row.ss;
      expected.writeUInt16LE(selector, SEG + 20); expected.writeUInt32LE(cache, SEG + 36);
      expected.writeUInt32LE(base, BASES + 24);
    }
    assert.deepEqual(after, expected, row.label + ' complete arena ownership boundary');
    if (row.protect) host(c, 'protect', [row.protect[0],row.protect[1],3]);
    assert.deepEqual(readRam(c), expectedRam, row.label + ' exact mapped RAM including decoys');
    writeFileSync(join(output, `arena-${c.id}.bin`), after, {flag:'wx'});
    observations.push({context:c.id, label:row.label, owner, registers:row.end,
      pc:state.readUInt32LE(48), flags:state.readUInt32LE(52), trace:c.trace,
      arena_sha256:hash(after), ram_sha256:hash(expectedRam), exit_hex:exit.toString('hex')});
  }
}
assert.deepEqual(pins(), sourcePins, 'verification sources must remain unchanged');
assert.equal(missingSources.length,0,'successful B16 proof requires every new source owner to be present and pinned');
const result = {engine_sha256:engine.sha256, sources:sourcePins, cases:cases.length,
  executions:observations.length, contexts, modules, observations,
  claim:'Authored Node engine-Wasm SS.B16 dynamic-pointer probes over replacement range and resident entry compilation. Exact logical CPU/exit fields and mapped RAM, complete arena ownership boundaries, live SS reload, wrap and faults. Helper packet/transfer bytes are constrained by generated validators and arena ownership, not an independent complete packet oracle. No execution result exists until this script succeeds. Mixed ENTER32+B16 follows declared full frame value, active parent walk and active allocation policy; wrapped POP follows declared Ring3 choice. Neither asserts named hardware-family equivalence. No CS-base/D16, browser, throughput, game or full protected-mode claim.'};
writeFileSync(join(output,'observed.json'), JSON.stringify(result,null,2)+'\n', {flag:'wx'});
console.log(JSON.stringify({cases:cases.length,executions:observations.length,engine_sha256:engine.sha256}));
