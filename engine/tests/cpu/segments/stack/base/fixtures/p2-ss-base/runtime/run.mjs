import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync, mkdirSync} from 'node:fs';
import {join} from 'node:path';
import {pathToFileURL} from 'node:url';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root, 'engine path, output directory and repo root are required');
const {readEngine} = await import(pathToFileURL(join(root, 'engine/tests/support/wasm/engine.mjs')));
assert.match(process.env.RING3_ENGINE_SHA256 ?? '', /^[a-f0-9]{64}$/, 'pin the selected engine artifact');
const engine = readEngine(enginePath);
mkdirSync(output, {recursive: true});
const SIZE = 4796, SEG = 4524, BASES = 4756, TRANSFER = 140, PC = 0x1000, HIGH = 0x5353;
const pages = [0, PC, 0x3000, 0x4000, 0x5000, 0x6000, 0x8000];
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
  'engine/tests/support/wasm/engine.mjs', 'engine/tests/support/wasm.rs',
  'engine/tests/cpu/segments/native.rs', 'engine/tests/cpu/segments/wasm.rs',
];
const pins = () => ({...Object.fromEntries(sourcePaths.map(path => [path, hash(readFileSync(join(root, path)))])),
  executed_probe: hash(readFileSync(new URL(import.meta.url)))});
const sourcePins = pins();
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
  host(c, 'open', [8, c.id, HIGH]); c.base = c.api.arena_ptr() >>> 0;
  for (const page of pages) host(c, 'map', [page, 1, page === PC ? 7 : 3]);
  host(c, 'set_segment_base', [2, row.base ?? 0x5000]);
  if (row.descriptor) host(c, 'configure_segment_descriptor_raw', row.descriptor);
  const code = Buffer.from(row.code + (row.terminal ? '' : 'ebfe'), 'hex');
  upload(c, PC, code);
  const resident = owner === 'resident-entries';
  input(c, TRANSFER, words(resident ? [PC] : [PC, code.length]));
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
  contexts.push({id: c.id, owner, label: row.label, bytes: row.code}); return c;
}
const initial = [0x12345678, 0x11112222, 0x33334444, 0x55556666, 0x24, 0x24, 0x77778888, 0x9999aaaa];
const update = changes => Object.assign([...initial], changes);
const cases = [
  {label: 'explicit-SS-ESP', code: '8b1424', seed: [[0x5024, [0xabcdef01]]], end: update({2: 0xabcdef01})},
  {label: 'explicit-address16-BP', code: '678b5600', regs: update({5: 0xabcd0024}), seed: [[0x5024, [0xabcdef01]]], end: update({2: 0xabcdef01, 5: 0xabcd0024})},
  {label: 'push-eax', code: '50', end: update({4: 0x20}), writes: [[0x5020, [0x12345678]]]},
  {label: 'push-original-esp', code: '54', end: update({4: 0x20}), writes: [[0x5020, [0x24]]]},
  {label: 'push-memory-source-esp-alias', code: 'ff3424', seed: [[0x5024, [0xa1b2c3d4]]], end: update({4: 0x20}), writes: [[0x5020, [0xa1b2c3d4]]]},
  {label: 'push-word', code: '6650', end: update({4: 0x22}), rawWrites: [[0x5022, '7856']]},
  {label: 'pop-register', code: '59', seed: [[0x5024, [0xa1b2c3d4]]], end: update({1: 0xa1b2c3d4, 4: 0x28})},
  {label: 'pop-esp-destination', code: '5c', seed: [[0x5024, [0x12345678]]], end: update({4: 0x12345678})},
  {label: 'pop-memory-after-esp-increment', code: '8f442404', seed: [[0x5024, [0xa1b2c3d4]]], end: update({4: 0x28}), writes: [[0x502c, [0xa1b2c3d4]]]},
  {label: 'push-flags', code: '9c', end: update({4: 0x20}), writes: [[0x5020, [2]]]},
  {label: 'pop-flags', code: '9d', seed: [[0x5024, [0x46]]], end: update({4: 0x28}), flags: 0x46},
  {label: 'push-segment', code: '1e', end: update({4: 0x20}), writes: [[0x5020, [0x23]]]},
  {label: 'near-call', code: 'e800000000', terminal: true, end: update({4: 0x20}), writes: [[0x5020, [0x1005]]]},
  {label: 'near-call-indirect-esp-source', code: 'ffd4', terminal: true, end: update({4: 0x20}), pc: 0x24, writes: [[0x5020,[0x1002]]]},
  {label: 'near-call-memory-esp-source', code: 'ff1424', terminal: true, seed: [[0x5024,[PC]]], end: update({4: 0x20}), pc: PC, writes: [[0x5020,[0x1003]]]},
  {label: 'near-return', code: 'c3', terminal: true, seed: [[0x5024, [0x1000]]], end: update({4: 0x28}), pc: PC},
  {label: 'leave', code: 'c9', regs: update({4: 0x40}), seed: [[0x5024, [0xa1b2c3d4]]], end: update({4: 0x28, 5: 0xa1b2c3d4})},
  {label: 'leave-word', code: '66c9', regs: update({4: 0x40}), seed: [[0x5024, [0xa1b2c3d4]]], end: update({4: 0x26, 5: 0xc3d4})},
  {label: 'push-all', code: '60', end: update({4: 4}), writes: [[0x5004, [0x9999aaaa,0x77778888,0x24,0x24,0x55556666,0x33334444,0x11112222,0x12345678]]]},
  {label: 'pop-all', code: '61', seed: [[0x5024, [1,2,3,4,5,6,7,8]]], end: [8,7,6,5,0x44,3,2,1]},
  {label: 'nested-enter-overlay', code: 'c8100003', end: update({4: 4, 5: 0x20}), writes: [[0x5014, [0x20,0x24,0x24,0x24]]]},
  {label: 'far-call', code: '9a001000001b00', terminal: true, end: update({4: 0x1c}), pc: PC, writes: [[0x501c, [0x1007,0x1b]]]},
  {label: 'far-return', code: 'cb', terminal: true, seed: [[0x5024, [PC,0x1b]]], end: update({4: 0x2c}), pc: PC},
  {label: 'far-return-word', code: '66cb', terminal: true, seed: [[0x5024,[0x001b1000]]], end: update({4: 0x28}), pc: PC},
  {label: 'iret-word', code: '66cf', terminal: true, seed: [[0x5024,[0x001b1000,0x46]]], end: update({4: 0x2a}), pc: PC, flags: 0x46},
  {label: 'iret', code: 'cf', terminal: true, seed: [[0x5024, [PC,0x1b,0x46]]], end: update({4: 0x30}), pc: PC, flags: 0x46},
  {label: 'base-addition-wrap-valid-span', code: '50', base: 0xfffffff0, regs: update({4: 0x34}), end: update({4: 0x30}), writes: [[0x20, [0x12345678]]]},
  {label: 'far-call-wrap-between-slots', code: '9a001000001b00', terminal: true, regs: update({4: 4}), end: update({4: 0xfffffffc}), pc: PC, writes: [[0x4ffc, [0x1007]], [0x5000, [0x1b]]]},
  {label: 'logical-whole-slot-overflow-precedes-RAM', code: '58', regs: update({4: 0xfffffffe}), end: update({4: 0xfffffffe}), pc: PC, retired: 0, fault: [15,0,0,0,0]},
  {label: 'far-call-translated-linear-high-leg-unmapped', code: '9a001000001b00', terminal: true, base: 0xfffffff0, regs: update({4: 0x12}), end: update({4: 0x12}), pc: PC, retired: 0, fault: [5,1,0xfffffffe,2,4]},
  {label: 'far-return-late-page-fault', code: 'cb', terminal: true, regs: update({4: 0xffc}), seed: [[0x5ffc,[PC,0x1b]]], protect: [0x6000,1,0], end: update({4: 0xffc}), pc: PC, retired: 0, fault: [5,2,0x6000,1,4]},
  {label: 'iret-late-page-fault', code: 'cf', terminal: true, regs: update({4: 0xff8}), seed: [[0x5ff8,[PC,0x1b,0x46]]], protect: [0x6000,1,0], end: update({4: 0xff8}), pc: PC, retired: 0, fault: [5,2,0x6000,1,4]},
  {label: 'far-call-late-page-fault', code: '9a001000001b00', terminal: true, regs: update({4: 0x1004}), protect: [0x5000,1,0], end: update({4: 0x1004}), pc: PC, retired: 0, fault: [5,2,0x5ffc,2,4]},
  {label: 'reject-logical-fault-address-after-base-translation', code: '9a001000001b00', terminal: true, regs: update({4: 0x1004}), protect: [0x5000,1,0], forgeFaultAddress: 0xffc, end: update({4: 0x1004}), pc: PC, retired: 0, fault: [7,2,0,0,0]},
  {label: 'ss-limit-precedes-denied-linear-page', code: '50', cache: 0xf2300010, protect: [0x5000,1,0], end: update({}), pc: PC, retired: 0, fault: [15,0,0,0,0]},
  {label: 'live-SS-load-then-push', code: '8ed053', regs: update({0: 0x43}), end: update({0: 0x43, 4: 0x20}), budget: 2, descriptor: [0x43,0x600000ff,0x0040f200], writes: [[0x6020,[0x55556666]]], ss: [0x43,0x6000,0xf23000ff]},
];
for (const owner of ['replacement-range', 'resident-entries']) {
  for (const row of cases) {
    const c = open(row, owner);
    for (const [address, values] of row.seed ?? []) upload(c, address, words(values));
    const regs = row.regs ?? initial;
    input(c, 0, record('R3ST', 56, [...regs, PC, 2])); input(c, 96, words([0]));
    if (row.cache !== undefined) {
      const cache = Buffer.alloc(4); cache.writeUInt32LE(row.cache); input(c, SEG + 36, cache);
    }
    const before = arena(c), beforeRam = readRam(c), expectedRam = Buffer.from(beforeRam);
    if (row.protect) host(c, 'protect', row.protect);
    for (const [address, values] of row.writes ?? []) {
      const page = pages.indexOf(address & ~4095); assert.notEqual(page, -1);
      expectedRam.set(words(values), page * 4096 + (address & 4095));
    }
    for (const [address, hex] of row.rawWrites ?? []) {
      const page = pages.indexOf(address & ~4095); assert.notEqual(page, -1);
      expectedRam.set(Buffer.from(hex, 'hex'), page * 4096 + (address & 4095));
    }
    assert.equal(c.run(c.base, c.base + 56, row.budget ?? 1, c.base + 96), 0, row.label);
    const after = arena(c), state = after.subarray(0, 56), exit = after.subarray(56, 96);
    assert.deepEqual(Array.from({length:8}, (_, i) => state.readUInt32LE(16 + i * 4)), row.end, row.label + ' logical registers');
    assert.equal(state.readUInt32LE(48), row.pc ?? PC + row.code.length / 2, row.label + ' EIP');
    assert.equal(state.readUInt32LE(52), row.flags ?? 2, row.label + ' FLAGS');
    assert.equal(state.readUInt32LE(12), 0, row.label + ' no retained SS debug shadow');
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
const result = {engine_sha256:engine.sha256, sources:sourcePins, cases:cases.length,
  executions:observations.length, contexts, modules, observations,
  claim:'Authored Node engine-Wasm SS-base/B32 probes over replacement range and resident entry compilation. Exact logical CPU/exit fields and mapped RAM, complete arena ownership boundaries, live SS reload, wrap and faults. Helper packet/transfer bytes are constrained by generated validators and arena ownership, not an independent complete packet oracle. No execution result exists until this script succeeds. No CS-base/D16, SS.B16, browser, throughput, game or full protected-mode claim.'};
writeFileSync(join(output,'observed.json'), JSON.stringify(result,null,2)+'\n', {flag:'wx'});
console.log(JSON.stringify({cases:cases.length,executions:observations.length,engine_sha256:engine.sha256}));
