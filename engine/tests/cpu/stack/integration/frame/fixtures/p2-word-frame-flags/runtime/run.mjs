import {pageFaultExit,pageFaultPacket} from '../../../../../../../support/wasm/page-fault.mjs';
import {generatedImports, attachDetectionApi} from '../../../../../../../support/wasm/binding.mjs';
import {ARENA_SIZE} from '../../../../../../../support/wasm/arena.mjs';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../../../../../../../support/wasm/engine.mjs';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root);
const engine = readEngine(enginePath), SIZE = ARENA_SIZE, PC = 0x1000, DATA = 0x5000;
const initial = readFileSync(join(output, 'initial-arena.bin'));
assert.equal(initial.length, SIZE);
assert.deepEqual(WebAssembly.Module.imports(engine.module), []);
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const paths = [
  'engine/src/cpu/dbt/wasm/segments/access.rs', 'engine/src/cpu/x86/segments/cache.rs',
  'engine/src/abi/arena.rs', 'engine/src/abi/memory_helper.rs',
  'engine/src/cpu/x86/ir.rs', 'engine/src/cpu/x86/decode/lower.rs',
  'engine/src/cpu/x86/decode/operands.rs', 'engine/src/cpu/x86/decode/flow.rs', 'engine/src/cpu/x86/decode/profile.rs', 'engine/src/cpu/dbt/region.rs',
  'engine/src/cpu/dbt/wasm/integer/arithmetic.rs', 'engine/src/cpu/dbt/wasm/integer/bits.rs', 'engine/src/cpu/dbt/wasm/integer/carry_rotates.rs', 'engine/src/cpu/dbt/wasm/integer/decimal.rs', 'engine/src/cpu/dbt/wasm/integer/dispatch.rs', 'engine/src/cpu/dbt/wasm/integer/flags.rs', 'engine/src/cpu/dbt/wasm/integer/mod.rs', 'engine/src/cpu/dbt/wasm/integer/multiply_divide.rs', 'engine/src/cpu/dbt/wasm/integer/operands.rs', 'engine/src/cpu/dbt/wasm/integer/rotates.rs', 'engine/src/cpu/dbt/wasm/integer/shifts.rs', 'engine/src/cpu/dbt/wasm/memory.rs', 'engine/src/cpu/dbt/wasm/memory/word_store.rs',
  'engine/src/cpu/dbt/wasm/memory/narrow.rs', 'engine/src/memory/space.rs', 'engine/src/memory/space/atomic.rs',
  'engine/src/cpu/dbt/wasm/abi.rs', 'engine/src/cpu/dbt/wasm/emitter.rs', 'engine/src/process/wasm/mod.rs', 'engine/src/process/wasm/loading.rs', 'engine/src/abi/image.rs', 'engine/src/process/wasm/execution.rs', 'engine/src/process/wasm/calls.rs', 'engine/src/process/wasm/memory.rs', 'engine/src/process/wasm/cpu.rs', 'engine/src/process/mod.rs',
  'engine/src/process/instance.rs', 'engine/src/process/execution/compiled.rs', 'engine/src/process/execution/authority.rs', 'engine/src/process/memory/address_space.rs', 'engine/src/process/memory/scalar.rs', 'engine/src/process/memory/scalar/tests.rs', 'engine/src/process/execution/mod.rs', 'engine/src/process/memory/mod.rs',  'engine/src/process/execution/resident.rs',    'engine/tests/cpu/stack/integration/frame/native/word.rs', 'engine/tests/cpu/stack/native.rs', 'engine/tests/support/cpu/prefix/admission.rs', 'engine/tests/support/cpu/prefix/word.rs',
  'engine/tests/cpu/stack/integration/frame/wasm/word.rs', 'engine/tests/cpu/stack/wasm.rs', 'engine/tests/support/wasm.rs',
  'engine/tests/cpu/stack/integration/frame/fixtures/p2-word-frame-flags/runtime/run.mjs', 'engine/tests/support/wasm/engine.mjs', 'engine/tests/support/wasm/binding.mjs','engine/tests/support/wasm/page-fault.mjs',
];
const sources = () => Object.fromEntries(paths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = sources(), modules = [];
const counts = {contexts: 0, runs: 0, retired: 0, flag_inputs: 0, anchors: 0, repeats: 0, faults: 0, retries: 0, current_inputs: 0, consumers: 0, controls: 0, retained_units: 0, code_reads: 0, self_code: 0};
const word = value => Buffer.from([value % 256, Math.floor(value / 256)]);
const words = values => {const b = Buffer.alloc(values.length * 4); values.forEach((v, i) => b.writeUInt32LE(v >>> 0, i * 4)); return b;};
function record(magic, size, fields, version = 1) {
  const b = Buffer.alloc(size); b.write(magic); b.writeUInt16LE(version, 4);
  b.writeUInt16LE(1, 6); b.writeUInt32LE(size, 8);
  fields.forEach((v, i) => b.writeUInt32LE(v >>> 0, 16 + i * 4)); return b;
}
const state = c => record('R3ST', 56, [...c.registers, c.pc, c.flags]);
const exit = (reason, n, f, version = 2) => record('R3EX', 40,
  [reason, n, f?.detail ?? 0, f?.address ?? 0, f?.access ?? 0, f ? 2 : 0], version);
const helper = (value = 0) => record('R3MH',40,[0,value,0,0,0,2],10);
const arena = c => Buffer.from(new Uint8Array(c.memory.buffer, c.base, SIZE));
function input(c, offset, bytes) {
  const expected = arena(c); expected.set(bytes, offset);
  new Uint8Array(c.memory.buffer).set(bytes, c.base + offset);
  assert.deepEqual(arena(c), expected, 'declared host input preserves the remaining full arena');
}
function host(c, name, args) {
  const expected = arena(c); assert.equal(c.api[name](...args), 0, name);
  if (name.startsWith('compile_resident')) {
    const after = arena(c); assert.equal(after.readUInt32LE(140), 1); assert.equal(after.readUInt32LE(144), 24);
    expected.set(after.subarray(140, 164), 140);
  }
  assert.deepEqual(arena(c), expected, 'full arena host effect: ' + name);
}
function upload(c, address, bytes) {input(c, 140, bytes); host(c, 'upload', [address, bytes.length]);}
function cpu(c) {
  const b = arena(c); return {registers: Array.from({length: 8}, (_, i) => b.readUInt32LE(16 + i * 4)), pc: b.readUInt32LE(48), flags: b.readUInt32LE(52)};
}
const registers = () => Array.from({length: 8}, (_, i) => ((0xa100 + i) * 65536 + 0x1234 + i) >>> 0);
function seed(c, pc, r = registers(), flags = 0xcd7) {
  const b = Buffer.alloc(140); b.set(state({registers: r, pc, flags})); b.set(exit(3, 0), 56);
  b.set(record('R3MH', 40, [0, 0xdecafbad, 0, 0, 0, 0]), 100); input(c, 0, b); return cpu(c);
}
function run(c, module, budget, next, {label, reason = 1, retired = 1, fault, helperVersion = 4, helperValue = 0, status = 0, exitVersion = 2} = {}) {
  const expected = arena(c);
  if (status === 0) {
    expected.set(state(next));
    if (fault) assert.ok(fault.detail === 1 || fault.detail === 2, 'declared backing fault');
    const pageFault = fault ? {
      errorCode: (fault.access === 2 ? 6 : 4) + Number(fault.detail === 2),
      address: fault.address,
      access: fault.access,
      length: 2,
    } : null;
    expected.set(
      pageFault ? pageFaultExit({retired, ...pageFault}) : exit(reason, retired, undefined, module.exitVersion??exitVersion),
      56,
    );
    if (helperVersion !== null) expected.set(pageFault ? pageFaultPacket(pageFault) : helper(helperValue), 100);
  }
  assert.equal(module.run(c.base, c.base + 56, budget, c.base + 96), status, label);
  assert.deepEqual(arena(c), expected, 'full4612 CPU/exit/helper/FP128 effect: ' + label);
  counts.runs++; if (status === 0) counts.retired += retired;
}
function checkRam(c, address, expected) {
  for (let i = 0; i < expected.length; i++) {
    const wanted = arena(c); wanted.set(record('R3MH', 40, [0, expected[i], 0, 0, 0, 1], 2), 100);
    assert.equal(c.api.read8((address + i) >>> 0), 0);
    assert.deepEqual(arena(c), wanted, 'real RAM byte and arena at ' + (address + i).toString(16));
  }
}

function compile(c, blocks) {
  const instructions = blocks.reduce((sum, b) => sum + b.instructions, 0);
  assert.ok(blocks.length > 0 && blocks.length <= 8, 'aggregate unit fits unchanged eight-block limit');
  assert.ok(Number.isInteger(instructions) && instructions <= 64, 'aggregate unit fits unchanged64-instruction limit');
  assert.ok(++c.captures <= 6, 'at most six captures/resident units/context');
  input(c, 140, words(blocks.flatMap(b => c.entries ? [b.pc] : [b.pc, b.bytes.length])));
  const method = c.owner === 'resident' ? (c.entries ? 'compile_resident_entries' : 'compile_resident') : (c.entries ? 'compile_entries' : 'compile');
  host(c, method, c.entries ? [blocks.length, 0] : [blocks.length]);
  const b = arena(c), binding = c.owner === 'resident'
    ? {low: b.readUInt32LE(148), high: b.readUInt32LE(152), pointer: b.readUInt32LE(156), length: b.readUInt32LE(160)}
    : {generation: c.api.generation(), pointer: c.api.module_ptr() >>> 0, length: c.api.module_len() >>> 0};
  const bytes = Buffer.from(new Uint8Array(c.memory.buffer, binding.pointer, binding.length));
  const module = new WebAssembly.Module(bytes), imports = WebAssembly.Module.imports(module);
  const reads = blocks.some(b => b.read), stores = blocks.some(b => b.store);
  const names = generatedImports({resident: c.owner === 'resident', helpers: [...(reads ? ['cpu_read16'] : []), ...(stores ? [c.owner === 'resident' ? 'cpu_store_resident16' : 'cpu_store16'] : [])]}).slice(1).map(item => item.name);
  assert.deepEqual(imports, [{module: 'env', name: 'memory', kind: 'memory'}, ...names.map(name => ({module: 'ring3', name, kind: 'function'}))]);
  assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  const file = `module-${modules.length + 1}.wasm`; writeFileSync(join(output, file), bytes, {flag: 'wx'});
  modules.push({context: c.id, owner: c.owner, entries: c.entries, file, sha256: hash(bytes), imports: names, instructions});
  return {exitVersion:reads||stores?6:1,run: new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3: c.api}).exports.run};
}
const block = row => ({pc: row.pc, read: row.op === 'leave', store: row.op === 'push', instructions: 2, bytes: Buffer.from([0x66, row.op === 'push' ? 0x9c : 0xc9, 0xeb, 0])});
const canary = Buffer.from([0x55, 0xa5, 0x66, 0x67, 0xf0, 0xf2, 0xf3, 0xaa]);
function map(c, address, pages, permission) {host(c, 'map', [address, pages, permission]); for (let i = 0; i < pages; i++) c.mapped.add(address + i * 4096);}
function unmap(c, address) {if (c.mapped.has(address)) {host(c, 'unmap', [address, 1]); c.mapped.delete(address);}}
function open(owner, entries) {
  const instance = new WebAssembly.Instance(engine.module, {});
  const c = {id: ++counts.contexts, owner, entries, memory: instance.exports.memory, api: {}, captures: 0, mapped: new Set()};
  for (const name of ['open', 'close', 'arena_ptr', 'map', 'unmap', 'protect', 'upload', 'read8', 'read16', 'store16', 'store_resident16',
    'compile', 'compile_entries', 'compile_resident', 'compile_resident_entries', 'generation', 'module_ptr', 'module_len', 'guard', 'guard_resident'])
    c.api[name] = instance.exports['ring3_abi_v1_' + name];
 attachDetectionApi(c.api, instance.exports);
  for(const name of ["cpu_read16", "cpu_store16", "cpu_store_resident16"]) {c.api[name]=instance.exports['ring3_abi_v1_'+name];assert.equal(typeof c.api[name],'function',name);}
  assert.equal(c.api.open(12, c.id, 0x5746524d), 0); c.base = c.api.arena_ptr() >>> 0; input(c, 0, initial);
  for (const args of [[PC, 1, 7], [0xc000, 1, 7], [DATA, 2, 3], [0xa53c5000, 1, 3], [0xfffff000, 1, 3], [0, 1, 3], [0xf000, 2, 3], [0x8000, 1, 3]]) map(c, ...args);
  upload(c, 0x8000, canary); host(c, 'protect', [0x8000, 1, 1]);
  return c;
}

// full32 wrapping addresses, own high halves and base256 digits are independent of emitted locals.
const wrap = value => Number((BigInt(value) % 4294967296n + 4294967296n) % 4294967296n);
const lowWord = (parent, value) => Math.floor(parent / 65536) * 65536 + value;
const HIGH = 0xa53c5000, RX = 0xc966;
const flags = Array.from({length: 128}, (_, n) => 2 + [1, 4, 16, 64, 128, 1024, 2048].reduce((sum, bit, i) => sum + Math.floor(n / 2 ** i) % 2 * bit, 0));
assert.equal(new Set(flags).size, 128); assert.ok(flags.every(value => (value & 2) && !(value & ~0xcd7)));
function transition(before, row, saved) {
  const next = {...before, registers: [...before.registers], pc: before.pc + 2};
  if (row.op === 'push') {
    const address = wrap(BigInt(before.registers[4]) - 2n), value = before.flags % 65536;
    next.registers[4] = address; return {next, address, value, helperVersion: 4};
  }
  assert.ok(Number.isInteger(saved) && saved >= 0 && saved <= 65535);
  next.registers[4] = wrap(BigInt(before.registers[5]) + 2n);
  next.registers[5] = lowWord(before.registers[5], saved);
  return {next, address: before.registers[5], value: saved, helperVersion: 2};
}
function operation(c, module, row, saved, label, reason = 1) {
  const effects = transition(cpu(c), row, saved);
  run(c, module, 1, effects.next, {label, reason, helperVersion: effects.helperVersion, helperValue: row.op === 'leave' ? effects.value : 0});
  checkRam(c, effects.address, word(effects.value)); return effects;
}
function framed(c, address, value) {upload(c, address - 1, Buffer.from([0x55, ...word(value), 0xaa]));}
const anchors = [
  {op: 'push', esp: DATA + 8, flags: 2, address: DATA + 6, nextEsp: DATA + 6},
  {op: 'push', esp: DATA + 9, flags: 0xcd7, address: DATA + 7, nextEsp: DATA + 7},
  {op: 'push', esp: DATA + 0x1001, flags: 0x892, address: DATA + 0xfff, nextEsp: DATA + 0xfff},
  {op: 'push', esp: 0, flags: 0x443, address: 0xfffffffe, nextEsp: 0xfffffffe},
  {op: 'push', esp: 2, flags: 0xcd7, address: 0, nextEsp: 0},
  {op: 'leave', ebp: DATA + 0x10, esp: 0xdead6008, saved: 0xbeef, address: DATA + 0x10, nextEsp: DATA + 0x12, nextEbp: 0xbeef},
  {op: 'leave', ebp: DATA + 0x20, esp: DATA + 0x20, saved: 0x8001, address: DATA + 0x20, nextEsp: DATA + 0x22, nextEbp: 0x8001},
  {op: 'leave', ebp: DATA + 0x30, esp: DATA + 0x40, saved: DATA + 0x40, address: DATA + 0x30, nextEsp: DATA + 0x32, nextEbp: DATA + 0x40},
  {op: 'leave', ebp: HIGH + 8, esp: DATA + 0x80, saved: 0xbeef, address: HIGH + 8, nextEsp: HIGH + 0xa, nextEbp: 0xa53cbeef},
  {op: 'leave', ebp: 0xffff, esp: DATA + 0x80, saved: 0xbeef, address: 0xffff, nextEsp: 0x10001, nextEbp: 0xbeef},
  {op: 'leave', ebp: 0xfffffffe, esp: DATA + 0x80, saved: 0x8001, address: 0xfffffffe, nextEsp: 0, nextEbp: 0xffff8001},
  {op: 'leave', ebp: DATA + 0xfff, esp: DATA + 0x80, saved: 0x1234, address: DATA + 0xfff, nextEsp: DATA + 0x1001, nextEbp: 0x1234},
  {op: 'leave', ebp: 0, esp: DATA + 0x80, saved: 0x8001, address: 0, nextEsp: 2, nextEbp: 0x8001},
];
for (const a of anchors) {
  const r = registers(); r[4] = a.esp; if (a.op === 'leave') r[5] = a.ebp;
  const effects = transition({registers: r, pc: PC, flags: a.flags ?? 0xcd7}, a, a.saved);
  assert.equal(effects.address, a.address); assert.equal(effects.next.registers[4], a.nextEsp);
  if (a.op === 'leave') assert.equal(effects.next.registers[5], a.nextEbp);
}
const failureCases = ['unmapped', 'none', 'wrong-permission', 'execute-only', 'cross-missing', 'cross-denied', 'overflow'];
function failureSetup(c, name, access) {
  unmap(c, 0x9000); unmap(c, 0xa000);
  const crossing = name.startsWith('cross-'), address = name === 'overflow' ? 0xffffffff : crossing ? 0x9fff : 0x9008;
  if (name !== 'unmapped' && name !== 'overflow') {
    map(c, 0x9000, 1, 3); if (name === 'cross-denied') map(c, 0xa000, 1, 3);
    if (name === 'cross-missing') upload(c, 0x9ffe, Buffer.from([0x55, 0x0f])); else framed(c, address, 0x800f);
    if (!crossing) host(c, 'protect', [0x9000, 1, name === 'none' ? 0 : name === 'execute-only' ? 4 : access === 1 ? 2 : 1]);
    if (name === 'cross-denied') host(c, 'protect', [0xa000, 1, access === 1 ? 2 : 1]);
  }
  return {name, address, detail: name === 'overflow' ? 3 : name === 'unmapped' || name === 'cross-missing' ? 1 : 2,
    faultAddress: crossing ? 0xa000 : address, original: name === 'unmapped' ? 0 : name === 'cross-missing' ? 0x000f : 0x800f};
}
function repair(c, failure) {
  if (failure.name === 'unmapped') map(c, 0x9000, 1, 3);
  else if (failure.name === 'cross-missing') map(c, 0xa000, 1, 3);
  else host(c, 'protect', [failure.name === 'cross-denied' ? 0xa000 : 0x9000, 1, 3]);
}
function stoppedFault(c, module, row, r, fault, label) {
  const before = seed(c, row.pc - 5, r), stopped = {...before, registers: [...before.registers], pc: row.pc}; stopped.registers[3] = 0x11223344;
  const segment = fault.detail === 3, helperVersion = segment ? null : row.op === 'push' ? 4 : 2, reason = segment ? 15 : 5, exitVersion = segment ? 6 : 2;
  run(c, module, 2, stopped, {label: label + ' after prior MOV retirement', reason, retired: 1, fault: segment ? undefined : fault, helperVersion, exitVersion});
  run(c, module, 1, stopped, {label: label + ' unchanged retry', reason, retired: 0, fault: segment ? undefined : fault, helperVersion, exitVersion}); counts.faults += 2; return stopped;
}
const mov = Buffer.from([0xbb, 0x44, 0x33, 0x22, 0x11]);
const live = Buffer.from([
  0xf9, 0x66, 0x9c, 0x66, 0x58, 0xf8, 0x66, 0x9c, 0x66, 0x5a, // save distinct current CF images into AX/DX.
  0x66, 0xc9, 0x66, 0xc9, 0x0f, 0x94, 0xc7, // two returned frames; SETZ BH consumes preserved ZF.
  0x66, 0x11, 0xd0, 0x0f, 0x92, 0xc3, 0xeb, 0, // ADC saved images with current CF0; SETB BL; suffix.
]);
assert.equal(live.length, 25);
for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries), push = {op: 'push', pc: PC}, leave = {op: 'leave', pc: PC + 0x40}, code = {op: 'leave', pc: RX};
  const faultRows = [{op: 'push', pc: PC + 0x105}, {op: 'leave', pc: PC + 0x145}];
  const faultBlocks = faultRows.map(row => ({...block(row), pc: row.pc - 5, bytes: Buffer.concat([mov, block(row).bytes]), instructions: 3}));
  const livePc = PC + 0x200, liveBlock = {pc: livePc, bytes: live, read: true, store: true, instructions: 12};
  for (const b of [block(push), block(leave), ...faultBlocks, liveBlock, block(code)]) upload(c, b.pc, b.bytes);
  host(c, 'protect', [PC, 1, 5]); host(c, 'protect', [0xc000, 1, 5]);
  let pushModule;
  for (const row of [push, leave]) {
    const module = compile(c, [block(row)]); if (row.op === 'push') pushModule = module;
    for (const value of flags) {
      const r = registers(); r[4] = DATA + 0x80; r[5] = HIGH + 8;
      framed(c, row.op === 'push' ? DATA + 0x7e : HIGH + 8, 0x8001); seed(c, row.pc, r, value);
      operation(c, module, row, 0x8001, 'all128 valid FLAGS remain exact while FLAGS image/frame WORD is consumed'); counts.flag_inputs++;
    }
    for (const a of anchors.filter(a => a.op === row.op)) {
      const r = registers(); r[4] = a.esp; if (row.op === 'leave') r[5] = a.ebp;
      upload(c, a.address, word(row.op === 'leave' ? a.saved : 0x5aa5)); seed(c, row.pc, r, a.flags ?? 0xcd7);
      const effects = operation(c, module, row, a.saved, 'literal full frame versus ESP/own-high carry/wrap/unaligned/endpoints');
      assert.equal(effects.next.registers[4], a.nextEsp); if (row.op === 'leave') assert.equal(effects.next.registers[5], a.nextEbp); counts.anchors++;
    }
    const r = registers(); r[4] = DATA + 0x80; r[5] = DATA + 0x10;
    upload(c, DATA + 0x10, word(DATA + 0x20)); upload(c, DATA + 0x20, word(DATA + 0x30)); seed(c, row.pc, r);
    operation(c, module, row, DATA + 0x20, 'first current stack/frame transfer'); input(c, 48, words([row.pc]));
    operation(c, module, row, DATA + 0x30, 'PC-only repeat consumes returned ESP or EBP without reseed'); counts.repeats += 2;
    seed(c, row.pc, r); run(c, module, 0, cpu(c), {label: 'budget0 before frame/flags helper', retired: 0, helperVersion: null});
    input(c, 96, words([1])); run(c, module, 1, cpu(c), {label: 'cancel before frame/flags helper', reason: 2, retired: 0, helperVersion: null}); run(c, module, 0, cpu(c), {label: 'cancel precedes budget0', reason: 2, retired: 0, helperVersion: null}); input(c, 96, words([0]));
    input(c, 48, words([row.pc + 2])); run(c, module, 2, {...cpu(c), pc: row.pc + 4}, {label: 'suffix guard continues with budget2', reason: 3, helperVersion: null}); run(c, module, 1, cpu(c), {label: 'cold lookup preserves current state', reason: 3, retired: 0, helperVersion: null}); counts.controls += 5;
  }
  const faults = compile(c, faultBlocks);
  for (const row of faultRows) {
    const access = row.op === 'push' ? 2 : 1;
    for (const name of failureCases) {
      const failure = failureSetup(c, name, access), r = registers(); r[4] = row.op === 'push' ? wrap(BigInt(failure.address) + 2n) : 0xdead6008; r[5] = failure.address;
      const stopped = stoppedFault(c, faults, row, r, {detail: failure.detail, address: failure.faultAddress, access}, name + ' frame/flags checked span');
      if (name !== 'overflow') {
        repair(c, failure); assert.deepEqual(cpu(c), stopped, 'map/protect-only repair retains original ESP/EBP/FLAGS');
        checkRam(c, failure.address, word(failure.original)); operation(c, faults, row, failure.original, 'repair-only retry consumes zero-filled/original frame or current FLAGS'); counts.retries++;
      }
      unmap(c, 0x9000); unmap(c, 0xa000);
    }
    const r = registers(); r[4] = row.op === 'push' ? 0x900a : 0xdead6008; r[5] = 0x9008;
    map(c, 0x9000, 1, 3); framed(c, 0x9008, 0x5aa5); host(c, 'protect', [0x9000, 1, row.op === 'push' ? 1 : 2]);
    const stopped = stoppedFault(c, faults, row, r, {detail: 2, address: 0x9008, access}, 'explicit-current witness begins with a retained permission fault');
    host(c, 'protect', [0x9000, 1, 3]); const current = {...stopped, registers: [...stopped.registers], flags: 0x892};
    current.registers[0] = 0xbaff8000; current.registers[4] = row.op === 'push' ? 0x900c : 0x87654321; current.registers[5] = HIGH + 8;
    if (row.op === 'leave') upload(c, HIGH + 8, word(0xbeef)); input(c, 0, state(current));
    operation(c, faults, row, 0xbeef, 'separate explicit current FLAGS/ESP or RAM/full frame/high-half retry');
    checkRam(c, 0x9008, word(0x5aa5)); if (row.op === 'push') checkRam(c, 0x900a, word(0x892)); counts.retries++; counts.current_inputs++; unmap(c, 0x9000);
  }
  const codeModule = compile(c, [block(code)]), r = registers(); r[4] = 0xdead6008; r[5] = RX; seed(c, RX, r);
  operation(c, codeModule, code, 0xc966, 'RX LEAVE reads own opcode WORD and retains original full frame high'); input(c, 48, words([RX]));
  operation(c, codeModule, code, 0xc966, 'PC-only RX repeat retains same current owner/frame with no stores');
  run(c, codeModule, 2, {...cpu(c), pc: RX + 4}, {label: 'RX read owner remains current through suffix', reason: 3, helperVersion: null}); counts.code_reads += 3; checkRam(c, RX, block(code).bytes);
  const mixed = compile(c, [liveBlock]), retained = registers(); retained[4] = DATA + 0x80; seed(c, push.pc, retained);
  if (owner === 'resident') operation(c, pushModule, push, undefined, 'old resident FLAGS stack unit remains current after later captures');
  else run(c, pushModule, 1, cpu(c), {label: 'replacement generation mismatch3 is neutral', status: 3, retired: 0}); counts.retained_units++;
  const initial = registers(); initial[0] = 0xcafe1234; initial[2] = 0xface5678; initial[3] = 0xbeefffff; initial[4] = DATA + 0x80; initial[5] = DATA + 0x10;
  upload(c, DATA + 0x10, word(DATA + 0x20)); upload(c, DATA + 0x20, word(DATA + 0x30)); seed(c, livePc, initial, 0xcd6);
  run(c, mixed, 1, {...cpu(c), pc: livePc + 1, flags: 0xcd7}, {label: 'live STC creates first current FLAGS image', helperVersion: null});
  operation(c, mixed, {op: 'push', pc: livePc + 1}, undefined, 'save first image with CF1');
  let before = cpu(c), next = {...before, registers: [...before.registers], pc: livePc + 5}; next.registers[0] = lowWord(before.registers[0], 0xcd7); next.registers[4] = DATA + 0x80;
  run(c, mixed, 1, next, {label: 'POP AX consumes saved first image', helperVersion: 2, helperValue: 0xcd7});
  run(c, mixed, 1, {...cpu(c), pc: livePc + 6, flags: 0xcd6}, {label: 'live CLC changes current carry independently from AX image', helperVersion: null});
  operation(c, mixed, {op: 'push', pc: livePc + 6}, undefined, 'save second image with CF0');
  before = cpu(c); next = {...before, registers: [...before.registers], pc: livePc + 10}; next.registers[2] = lowWord(before.registers[2], 0xcd6); next.registers[4] = DATA + 0x80;
  run(c, mixed, 1, next, {label: 'POP DX consumes distinct second image', helperVersion: 2, helperValue: 0xcd6});
  operation(c, mixed, {op: 'leave', pc: livePc + 10}, DATA + 0x20, 'first returned frame retains FLAGS'); operation(c, mixed, {op: 'leave', pc: livePc + 12}, DATA + 0x30, 'next LEAVE consumes returned EBP without reseed');
  before = cpu(c); next = {...before, registers: [...before.registers], pc: livePc + 17}; next.registers[3] = lowWord(before.registers[3], 0x01ff);
  run(c, mixed, 1, next, {label: 'SETZ BH observes frame-preserved ZF1', helperVersion: null});
  before = cpu(c); next = {...before, registers: [...before.registers], pc: livePc + 20, flags: 0x402}; next.registers[0] = lowWord(before.registers[0], 0x19ad);
  run(c, mixed, 1, next, {label: 'ADC saved CD7+CD6 with current CF0 gives19AD', helperVersion: null});
  before = cpu(c); next = {...before, registers: [...before.registers], pc: livePc + 23}; next.registers[3] = lowWord(before.registers[3], 0x0100);
  run(c, mixed, 1, next, {label: 'SETB BL observes returned arithmetic CF0', helperVersion: null});
  run(c, mixed, 2, {...cpu(c), pc: livePc + 25}, {label: 'live suffix continues with budget2', reason: 3, helperVersion: null}); run(c, mixed, 1, cpu(c), {label: 'live cold lookup retains returned saved-image/frame state', reason: 3, retired: 0, helperVersion: null}); counts.consumers += 13;
  checkRam(c, DATA + 0x7e, word(0xcd6)); checkRam(c, DATA + 0x10, word(DATA + 0x20)); checkRam(c, DATA + 0x20, word(DATA + 0x30)); checkRam(c, livePc, live); checkRam(c, 0x8000, canary); assert.equal(c.api.close(), 0);
}
for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) for (const changed of [false, true]) {
  const c = open(owner, entries), bytes = Buffer.from([0x66, 0x9c, 0x66, 0xb8, 0xd7, 0x0c, 0xeb, 0]);
  upload(c, PC, bytes); const row = {op: 'push', pc: PC}, module = compile(c, [{pc: PC, bytes, read: false, store: true, instructions: 3}]), r = registers(); r[0] = 0xcafe1234; r[4] = PC + 6;
  const image = changed ? 0x402 : 0xcd7; seed(c, PC, r, image); operation(c, module, row, undefined, 'changed/identity valid FLAGS image overwrites consumed future MOV payload once', 6);
  run(c, module, 1, cpu(c), {label: 'code-invalidated same owner4 is neutral before any new capture', status: 4, retired: 0});
  const tailBytes = Buffer.from([0x66, 0xb8, ...word(image), 0xeb, 0]), tail = compile(c, [{pc: PC + 2, bytes: tailBytes, read: false, store: false, instructions: 2}]);
  const before = cpu(c), next = {...before, registers: [...before.registers], pc: PC + 6}; next.registers[0] = lowWord(before.registers[0], image);
  run(c, tail, 1, next, {label: 'fresh pure suffix consumes current saved image without PUSHF replay', helperVersion: null, exitVersion: 1});
  run(c, tail, 2, {...cpu(c), pc: PC + 8}, {label: 'fresh suffix JMP reaches cold lookup with budget2', reason: 3, helperVersion: null, exitVersion: 1});
  checkRam(c, PC + 4, word(image)); checkRam(c, 0x8000, canary); assert.equal(c.api.close(), 0); counts.self_code += 4;
}
assert.deepEqual(sources(), sourcePins);
assert.deepEqual(counts, {contexts: 12, runs: 1416, retired: 1306, flag_inputs: 1024, anchors: 52, repeats: 16, faults: 128, retries: 56, current_inputs: 8, consumers: 52, controls: 40, retained_units: 4, code_reads: 12, self_code: 32});
assert.equal(modules.length, 36);
const result = {status: 'ok', engine_sha256: engine.sha256, source_pins: sourcePins, counts, modules,
  limits: [
    'finite exact single66 PUSHFW/LEAVEW four bound profiles; standalone/POPF/far WORD control/16bit-stack remain closed',
    'independent BigInt wrapping ESP, original fullEBP/own-high, FLAGS-image and little-endian model;13 literal anchors and all128 valid FLAGS per form',
    'seven read/store faults with prior retirement/unchanged retry and map/protect-only repair; separate explicit current FLAGS/ESP or RAM/fullEBP/high inputs',
    'live two distinct saved FLAGS images and returned frames consumed without CPU reseed; own RX LEAVE reads with PC-only repeat do not store/invalidate',
    'valid FLAGS payload identity/changed PUSHF writes commit once before6, neutral code-owner4 then fresh pure suffix; generation mismatch remains3',
    'maxfive resident captures/context;1/1/2/1/1 coreblocks with2/2/6/2/12 aggregate instructions, SMC3/2;existing8blocks/64instructions/6captures asserted before compile',
    'complete4612/helper/dirtyFP/canaries;8 current rows overlap successful retries; no hardware exceptional microstate/segmentation/VM-CPL/exhaustiveISA/speed/PE/browser/SDK/game/hostedCI claim',
  ]};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2), {flag: 'wx'});
console.log(JSON.stringify({status: 'ok', ...counts, modules: modules.length, output}));
