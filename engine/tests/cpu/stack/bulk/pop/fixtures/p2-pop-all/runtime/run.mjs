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
const engine = readEngine(enginePath), SIZE = ARENA_SIZE, PC = 0x1000, DATA = 0x5000, HIGH = 0xa53c5000;
const initial = readFileSync(join(output, 'initial-arena.bin'));
assert.equal(initial.length, SIZE);
assert.deepEqual(WebAssembly.Module.imports(engine.module), []);
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const paths = [
  'engine/src/cpu/dbt/wasm/segments/access.rs', 'engine/src/cpu/x86/segments/cache.rs',
  'engine/src/abi/arena.rs', 'engine/src/abi/memory_helper.rs',
  'engine/src/cpu/x86/ir.rs', 'engine/src/cpu/x86/decode/decoder.rs', 'engine/src/cpu/x86/decode/lower.rs',
  'engine/src/cpu/x86/decode/flow.rs', 'engine/src/cpu/x86/decode/profile.rs', 'engine/src/cpu/dbt/region.rs',
  'engine/src/cpu/dbt/wasm/integer/arithmetic.rs', 'engine/src/cpu/dbt/wasm/integer/bits.rs', 'engine/src/cpu/dbt/wasm/integer/carry_rotates.rs', 'engine/src/cpu/dbt/wasm/integer/decimal.rs', 'engine/src/cpu/dbt/wasm/integer/dispatch.rs', 'engine/src/cpu/dbt/wasm/integer/flags.rs', 'engine/src/cpu/dbt/wasm/integer/mod.rs', 'engine/src/cpu/dbt/wasm/integer/multiply_divide.rs', 'engine/src/cpu/dbt/wasm/integer/operands.rs', 'engine/src/cpu/dbt/wasm/integer/rotates.rs', 'engine/src/cpu/dbt/wasm/integer/shifts.rs', 'engine/src/cpu/dbt/wasm/memory.rs', 'engine/src/cpu/dbt/wasm/memory/narrow.rs',
  'engine/src/cpu/dbt/wasm/abi.rs', 'engine/src/cpu/dbt/wasm/emitter.rs', 'engine/src/cpu/dbt/wasm/locals.rs',
  'engine/src/memory/space.rs', 'engine/src/memory/space/atomic.rs', 'engine/src/process/instance.rs', 'engine/src/process/execution/compiled.rs', 'engine/src/process/execution/authority.rs', 'engine/src/process/memory/address_space.rs', 'engine/src/process/memory/scalar.rs', 'engine/src/process/memory/scalar/tests.rs', 'engine/src/process/execution/mod.rs', 'engine/src/process/memory/mod.rs', 'engine/src/process/mod.rs', 'engine/src/process/execution/resident.rs',    'engine/src/process/wasm/mod.rs', 'engine/src/process/wasm/loading.rs', 'engine/src/abi/image.rs', 'engine/src/process/wasm/execution.rs', 'engine/src/process/wasm/calls.rs', 'engine/src/process/wasm/memory.rs', 'engine/src/process/wasm/cpu.rs',
  'engine/src/cpu/x86/stack.rs', 'engine/src/cpu/x86/decode/stack.rs', 'engine/src/cpu/dbt/wasm/stack.rs', 'engine/src/process/cpu/stack.rs', 'engine/src/process/cpu/mod.rs',  'engine/src/abi/cpu_helper.rs',
  'engine/tests/cpu/stack/bulk/pop/native.rs', 'engine/tests/cpu/stack/native.rs',  'engine/tests/cpu/stack/bulk/pop/wasm.rs', 'engine/tests/cpu/stack/wasm.rs', 'engine/tests/support/wasm.rs',
  'engine/tests/cpu/stack/bulk/pop/fixtures/p2-pop-all/runtime/run.mjs', 'engine/tests/support/wasm/engine.mjs', 'engine/tests/support/wasm/binding.mjs','engine/tests/support/wasm/page-fault.mjs',
];
assert.equal(new Set(paths).size, paths.length);
const sources = () => Object.fromEntries(paths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = sources(), modules = [], observations = [];
const counts = {contexts: 0, runs: 0, retired: 0, flag_inputs: 0, frames: 0, boundaries: 0, code_reads: 0,
  repeats: 0, controls: 0, faults: 0, repairs: 0, current_inputs: 0, consumers: 0, retained: 0, helper_calls: 0};
const wrap = value => Number((BigInt(value) % 4294967296n + 4294967296n) % 4294967296n);
const mergeWord = (parent, low) => Math.floor(parent / 65536) * 65536 + low;
const slots = [0, 1, 2, 4, 5, 6, 7], destinations = [7, 6, 5, 3, 2, 1, 0];
const flags = Array.from({length: 128}, (_, n) => 2 + [1, 4, 16, 64, 128, 1024, 2048]
  .reduce((sum, bit, i) => sum + Math.floor(n / 2 ** i) % 2 * bit, 0));
const words = values => {const b = Buffer.alloc(values.length * 4); values.forEach((v, i) => b.writeUInt32LE(v >>> 0, i * 4)); return b;};
function image(width, values) {
  assert.equal(values.length, 8);
  return Buffer.from(values.flatMap(value => Array.from({length: width}, (_, digit) => Math.floor(value / 256 ** digit) % 256)));
}
function valueAt(bytes, slot, width) {
  return Array.from({length: width}, (_, digit) => bytes[slot * width + digit] * 256 ** digit).reduce((a, b) => a + b, 0);
}
function transition(before, row, bytes) {
  const next = {...before, registers: [...before.registers], pc: before.pc + row.op.length};
  for (const [index, slot] of slots.entries()) {
    const target = destinations[index], value = valueAt(bytes, slot, row.width);
    next.registers[target] = row.width === 2 ? mergeWord(before.registers[target], value) : value;
  }
  next.registers[4] = wrap(BigInt(before.registers[4]) + BigInt(8 * row.width));
  return next;
}
function record(magic, size, fields, version = 1) {
  const b = Buffer.alloc(size); b.write(magic); b.writeUInt16LE(version, 4); b.writeUInt16LE(1, 6); b.writeUInt32LE(size, 8);
  fields.forEach((v, i) => b.writeUInt32LE(v >>> 0, 16 + i * 4)); return b;
}
const state = c => record('R3ST', 56, [...c.registers, c.pc, c.flags]);
const exit = (reason, retired, f, version = 2) => record('R3EX', 40, [reason, retired, f?.detail ?? 0, f?.address ?? 0, f?.segment ? 0 : f ? 1 : 0, f?.segment ? 0 : f?.width ?? 0], version);
const helper = (width, f) => record('R3MH', 40, [f?.segment ? 7 : f ? 1 : 0, 0, f?.detail ?? 0, f?.address ?? 0, f?.segment ? 0 : f ? 1 : 0, f ? width : 0], 7);
const arena = c => Buffer.from(new Uint8Array(c.memory.buffer, c.base, SIZE));
function input(c, offset, bytes) {
  const expected = arena(c); expected.set(bytes, offset); new Uint8Array(c.memory.buffer).set(bytes, c.base + offset);
  assert.deepEqual(arena(c), expected, 'declared input preserves the remaining arena');
}
function host(c, name, args) {
  const expected = arena(c); assert.equal(c.api[name](...args), 0, name);
  if (name.startsWith('compile_resident')) {
    const b = arena(c); assert.equal(b.readUInt32LE(140), 1); assert.equal(b.readUInt32LE(144), 24); expected.set(b.subarray(140, 164), 140);
  }
  assert.deepEqual(arena(c), expected, 'host arena effect: ' + name);
}
function map(c, address, pages = 1) {host(c, 'map', [address, pages, 3]); for (let i = 0; i < pages; i++) c.mapped.add(address + i * 4096);}
function unmap(c, address) {if (c.mapped.has(address)) {host(c, 'unmap', [address, 1]); c.mapped.delete(address);}}
function upload(c, address, bytes) {input(c, 140, bytes); host(c, 'upload', [address, bytes.length]);}
function frame(c, address, bytes) {
  for (let offset = 0; offset < bytes.length;) {
    const at = wrap(BigInt(address) + BigInt(offset)), length = Math.min(bytes.length - offset, 4096 - at % 4096);
    upload(c, at, bytes.subarray(offset, offset + length)); offset += length;
  }
}
function checkRam(c, address, bytes) {
  for (const [offset, value] of bytes.entries()) {
    const expected = arena(c); expected.set(record('R3MH', 40, [0, value, 0, 0, 0, 1], 2), 100);
    assert.equal(c.api.read8(wrap(BigInt(address) + BigInt(offset))), 0); assert.deepEqual(arena(c), expected, 'real RAM remains read-only');
  }
}
function cpu(c) {const b = arena(c); return {registers: Array.from({length: 8}, (_, i) => b.readUInt32LE(16 + i * 4)), pc: b.readUInt32LE(48), flags: b.readUInt32LE(52)};}
const registers = () => Array.from({length: 8}, (_, i) => (0xa100 + i) * 65536 + 0x1234 + i);
function seed(c, pc, esp, flag = 0xcd7, r = registers()) {
  r = [...r]; r[4] = esp;
  const b = Buffer.alloc(140); b.set(state({registers: r, pc, flags: flag})); b.set(exit(3, 0), 56);
  b.set(record('R3MH', 40, [0, 0xdecafbad, 0, 0, 0, 0]), 100); input(c, 0, b); return cpu(c);
}
function run(c, module, budget, next, {label, reason = 1, retired = 1, row, bytes, fault, expectedReads = [], stackInput, status = 0} = {}) {
  const expected = arena(c);
  if (status === 0) {
    const pageFault = fault && !fault.segment ? {
      errorCode: 4 + Number(fault.detail === 2),
      address: fault.address,
      access: 1,
      length: fault.width,
    } : null;
    if (pageFault) {
      assert.ok(fault.detail === 1 || fault.detail === 2);
      reason = 23;
    }
    expected.set(state(next));
    expected.set(
      pageFault ? pageFaultExit({retired, ...pageFault}) : exit(reason, retired, fault, module.exitVersion),
      56,
    );
    if (row) {
      const original = stackInput ?? cpu(c).registers;
      expected.set(words(fault ? original : next.registers), 140);
      expected.set(pageFault?pageFaultPacket(pageFault):helper(row.width,fault),100);
    }
  }
  const expectedCalls = row && status === 0 ? [{control: row.width === 4 ? 2 : 3,
    input: stackInput ?? cpu(c).registers, output: fault ? (stackInput ?? cpu(c).registers) : next.registers}] : [];
  c.trace = []; assert.equal(module.run(c.base, c.base + 56, budget, c.base + 96), status, label);
  assert.deepEqual(c.trace, expectedCalls, 'one checked stack transaction and exact staged GPR input/output: ' + label);
  assert.deepEqual(arena(c), expected, 'full4612 CPU/FLAGS/helper/FP effect: ' + label);
  observations.push({context: c.id, label, status, reason, retired, helpers: c.trace, selected_read_candidates: expectedReads});
  counts.runs++; if (status === 0) counts.retired += retired; counts.helper_calls += c.trace.length;
}
const addresses = (esp, width, length = 7) => slots.slice(0, length).map(slot => ({width, address: wrap(BigInt(esp) + BigInt(slot * width))}));
function pop(c, module, row, bytes, label) {
  const before = cpu(c), next = transition(before, row, bytes);
  run(c, module, 1, next, {label, row, bytes, expectedReads: addresses(before.registers[4], row.width)});
  checkRam(c, before.registers[4], bytes); return next;
}
function compile(c, blocks) {
  const instructions = blocks.reduce((sum, b) => sum + b.instructions, 0);
  assert.ok(blocks.length > 0 && blocks.length <= 8); assert.ok(instructions <= 64); assert.ok(++c.captures <= 6);
  input(c, 140, words(blocks.flatMap(b => c.entries ? [b.pc] : [b.pc, b.bytes.length])));
  const method = c.owner === 'resident' ? (c.entries ? 'compile_resident_entries' : 'compile_resident') : (c.entries ? 'compile_entries' : 'compile');
  host(c, method, c.entries ? [blocks.length, 0] : [blocks.length]);
  const b = arena(c), binding = c.owner === 'resident'
    ? {low: b.readUInt32LE(148), high: b.readUInt32LE(152), pointer: b.readUInt32LE(156), length: b.readUInt32LE(160)}
    : {generation: c.api.generation(), pointer: c.api.module_ptr() >>> 0, length: c.api.module_len() >>> 0};
  const bytes = Buffer.from(new Uint8Array(c.memory.buffer, binding.pointer, binding.length)), module = new WebAssembly.Module(bytes);
  const hasStack = blocks.some(b => b.width), stackName = c.owner === 'resident' ? 'stack_execute_resident' : 'stack_execute';
  const names = generatedImports({resident: c.owner === 'resident', helpers: [...(hasStack ? [stackName] : [])]}).slice(1).map(item => item.name);
  assert.deepEqual(WebAssembly.Module.imports(module), [{module: 'env', name: 'memory', kind: 'memory'}, ...names.map(name => ({module: 'ring3', name, kind: 'function'}))]);
  assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  const file = `module-${modules.length + 1}.wasm`; writeFileSync(join(output, file), bytes, {flag: 'wx'});
  modules.push({context: c.id, owner: c.owner, entries: c.entries, file, sha256: hash(bytes), instructions, imports: names});
  const imports = {...c.api};
  imports[stackName] = (...args) => {
    if (c.owner === 'resident') assert.deepEqual(args.slice(0, 4).map(n => n >>> 0), [c.id, 0x504f5041, binding.low, binding.high]);
    const before = arena(c), original = Array.from({length: 8}, (_, i) => before.readUInt32LE(140 + i * 4));
    const status = c.api[stackName](...args), after = arena(c), allowed = Buffer.from(before); allowed.set(after.subarray(100, 172), 100);
    assert.deepEqual(after, allowed, 'stack helper changes only its strict packet and staged GPR result');
    c.trace.push({control: args.at(-1) >>> 0, input: original, output: Array.from({length: 8}, (_, i) => after.readUInt32LE(140 + i * 4))});
    return status;
  };
  return {run: new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3: imports}).exports.run,
    exitVersion: hasStack ? 6 : 1};
}
const canary = Buffer.from([0x55, 0xa5, 0x66, 0x67, 0xf0, 0xf2, 0xf3, 0xaa]);
const values = [0x12345678, 0x89abcdef, 0x80000000, 0xdeadbeef, 0xffffffff, 1, 0x7fffffff, 0x00008001];
const rows = [4, 2].map((width, i) => ({width, pc: PC + i * 0x80, op: Buffer.from(width === 4 ? [0x61] : [0x66, 0x61])}));
const livePc = PC + 0x200, live = Buffer.from([0x66, 0x11, 0xd0, 0x0f, 0x90, 0xc3, 0x0f, 0x92, 0xc7, 0x66, 0x0f, 0xbc, 0xc8, 0x0f, 0x94, 0xc2, 0xeb, 0]);
const mov = Buffer.from([0xbb, 0x44, 0x33, 0x22, 0x11]);
const block = row => ({pc: row.pc, width: row.width, instructions: 2, bytes: Buffer.concat([row.op, Buffer.from([0xeb, 0])])});
function open(owner, entries) {
  const instance = new WebAssembly.Instance(engine.module, {}), c = {id: ++counts.contexts, owner, entries, memory: instance.exports.memory, api: {}, mapped: new Set(), captures: 0, trace: []};
  for (const name of ['open', 'close', 'arena_ptr', 'map', 'unmap', 'protect', 'upload', 'read8', 'read16', 'read32', 'stack_execute', 'stack_execute_resident',
    'compile', 'compile_entries', 'compile_resident', 'compile_resident_entries', 'generation', 'module_ptr', 'module_len', 'guard', 'guard_resident'])
    c.api[name] = instance.exports['ring3_abi_v1_' + name];
 attachDetectionApi(c.api, instance.exports);
  assert.equal(c.api.open(12, c.id, 0x504f5041), 0); c.base = c.api.arena_ptr() >>> 0; input(c, 0, initial);
  for (const [address, pages] of [[PC, 1], [DATA, 2], [HIGH, 1], [0xfffff000, 1], [0, 1], [0x8000, 1], [0xf000, 2]]) map(c, address, pages);
  const code = Buffer.alloc(0x300, 0xcc);
  for (const row of rows) {code.set(block(row).bytes, row.pc - PC); code.set(Buffer.concat([mov, block(row).bytes]), row.pc - PC + 0x40);}
  code.set(live, livePc - PC); upload(c, PC, code); c.code = code;
  upload(c, 0x8000, canary); upload(c, 0x8020, image(4, values)); upload(c, 0x8080, image(2, values));
  host(c, 'protect', [0x8000, 1, 1]); host(c, 'protect', [PC, 1, 5]); return c;
}

// literal register images independently pin the slot order, ignored SP and each parent's own high half.
for (const row of rows) {
  const before = {registers: registers(), pc: row.pc, flags: 0xcd7}; before.registers[4] = DATA + 8;
  const expected = row.width === 4
    ? [0x8001, 0x7fffffff, 1, 0xffffffff, DATA + 40, 0x80000000, 0x89abcdef, 0x12345678]
    : [0xa1008001, 0xa101ffff, 0xa1020001, 0xa103ffff, DATA + 24, 0xa1050000, 0xa106cdef, 0xa1075678];
  assert.deepEqual(transition(before, row, image(row.width, values)), {...before, registers: expected, pc: row.pc + row.op.length});
}
function faultSetup(c, row, slot, mode) {
  unmap(c, 0x9000); unmap(c, 0xa000); map(c, 0x9000);
  const crossing = mode.startsWith('cross'), overflow = mode === 'overflow';
  const esp = overflow ? 0xffffffff - slot * row.width : 0xa000 - slot * row.width - (crossing ? 1 : 0);
  const original = image(row.width, values), known = Buffer.from(original);
  if (mode.includes('deny')) map(c, 0xa000);
  for (let i = 0; i < known.length; i++) {
    const at = wrap(BigInt(esp) + BigInt(i)); if (!c.mapped.has(at - at % 4096)) known[i] = 0;
  }
  for (let offset = 0; offset < original.length;) {
    const at = wrap(BigInt(esp) + BigInt(offset)), length = Math.min(original.length - offset, 4096 - at % 4096);
    if (c.mapped.has(at - at % 4096)) upload(c, at, original.subarray(offset, offset + length)); offset += length;
  }
  if (mode.includes('deny')) host(c, 'protect', [0xa000, 1, 2]);
  return {esp, known, fault: {segment: overflow, width: row.width, detail: overflow ? 0 : mode.includes('deny') ? 2 : 1,
    address: overflow ? 0 : 0xa000}, length: overflow ? 0 : slots.indexOf(slot) + 1};
}
function stopped(c, module, row, failure, label) {
  const before = seed(c, row.pc + 0x40, failure.esp), held = {...before, registers: [...before.registers], pc: row.pc + 0x45}; held.registers[3] = 0x11223344;
  const target = {...row, pc: held.pc};
  for (const [budget, retired] of [[2, 1], [1, 0]]) {
    run(c, module, budget, held, {label, reason: failure.fault.segment ? 15 : 5, retired, row: target, fault: failure.fault,
      expectedReads: addresses(failure.esp, row.width, failure.length), stackInput: held.registers}); checkRam(c, 0x8000, canary); counts.faults++;
  }
  return target;
}
for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries), widthModules = [];
  for (const row of rows) {
    const primary = block(row), faults = {pc: row.pc + 0x40, width: row.width, instructions: 3, bytes: Buffer.concat([mov, primary.bytes])};
    const module = compile(c, [primary, faults]); widthModules.push(module);
    const bytes = image(row.width, values); frame(c, DATA + 8, bytes);
    for (const flag of flags) {seed(c, row.pc, DATA + 8, flag); pop(c, module, row, bytes, 'all128 valid FLAGS and independent register halves'); counts.flag_inputs++;}
    for (let n = 0; n < 16; n++) {
      const varied = image(row.width, values.map((value, i) => wrap(BigInt(value) + BigInt((n + 1) * (i + 1) * 0x10203))));
      frame(c, DATA + 8, varied); seed(c, row.pc, DATA + 8, flags[n * 7]); pop(c, module, row, varied, 'distinct staged frame/ignored savedESP literal'); counts.frames++;
    }
    for (const address of [DATA + 8, DATA + 9, DATA + 0xfff, HIGH + 8, 4294967296 - 8 * row.width, 0, 0xffff, 4294967296 - 4 * row.width]) {
      frame(c, address, bytes); seed(c, row.pc, address); pop(c, module, row, bytes, 'full32 unaligned/cross-page/own-high/wrapped slot addresses'); counts.boundaries++;
    }
    const ro = row.width === 4 ? 0x8020 : 0x8080; seed(c, row.pc, ro); pop(c, module, row, bytes, 'read-only frame proves no guest store'); counts.code_reads++;
    const codeBytes = c.code.subarray(row.pc - PC, row.pc - PC + 8 * row.width); seed(c, row.pc, row.pc);
    pop(c, module, row, codeBytes, 'RX reads include own opcode without invalidation'); input(c, 48, words([row.pc]));
    const secondCode = c.code.subarray(row.pc - PC + 8 * row.width, row.pc - PC + 16 * row.width);
    pop(c, module, row, secondCode, 'same owner PC-only RX repeat consumes returned fullESP');
    run(c, module, 2, {...cpu(c), pc: row.pc + row.op.length + 2}, {label: 'RX suffix stays current', reason: 3}); counts.code_reads += 3;
    frame(c, DATA + 0x80, Buffer.concat([bytes, image(row.width, [...values].reverse())])); seed(c, row.pc, DATA + 0x80);
    pop(c, module, row, bytes, 'first returned fullESP'); input(c, 48, words([row.pc]));
    pop(c, module, row, image(row.width, [...values].reverse()), 'PC-only repeat uses current RAM and returned ESP'); counts.repeats += 2;
    seed(c, row.pc, 0x9008); run(c, module, 0, cpu(c), {label: 'budget0 before any read', retired: 0});
    input(c, 96, words([1])); run(c, module, 1, cpu(c), {label: 'cancel before any read', reason: 2, retired: 0});
    run(c, module, 0, cpu(c), {label: 'cancel precedes budget0', reason: 2, retired: 0}); input(c, 96, words([0]));
    input(c, 48, words([row.pc + row.op.length])); run(c, module, 2, {...cpu(c), pc: row.pc + row.op.length + 2}, {label: 'suffix budget2 reaches cold lookup', reason: 3});
    run(c, module, 1, cpu(c), {label: 'cold lookup remains neutral', reason: 3, retired: 0}); counts.controls += 5;
    for (const [index, slot] of slots.entries()) {
      const mode = index % 2 ? 'deny' : 'missing', f = faultSetup(c, row, slot, mode), target = stopped(c, module, row, f, 'fault at selected slot' + slot);
      if (mode === 'deny') host(c, 'protect', [0xa000, 1, 3]); else map(c, 0xa000);
      pop(c, module, target, f.known, 'map/protect-only repair retains every staged CPU input'); counts.repairs++;
    }
    for (const mode of ['cross-missing', 'cross-deny']) {
      const f = faultSetup(c, row, 4, mode), target = stopped(c, module, row, f, 'one checked read spans failing second page');
      if (mode.includes('deny')) host(c, 'protect', [0xa000, 1, 3]); else map(c, 0xa000);
      pop(c, module, target, f.known, 'cross-span map/protect-only repair preserves preceding candidates'); counts.repairs++;
    }
    for (const slot of [0, 7]) {const f = faultSetup(c, row, slot, 'overflow'); stopped(c, module, row, f, 'early/late individual read-span overflow keeps original CPU');}
    const f = faultSetup(c, row, 7, 'deny'), target = stopped(c, module, row, f, 'separate current-input retry starts from a late permission fault');
    host(c, 'protect', [0xa000, 1, 3]); const changed = image(row.width, values.map(value => wrap(BigInt(value) + 0x7654321n)));
    frame(c, DATA + 0x180, changed); const current = cpu(c); current.registers = registers().map(value => wrap(BigInt(value) + 0x12340000n)); current.registers[4] = DATA + 0x180; current.flags = 0x892;
    input(c, 0, state(current)); pop(c, module, target, changed, 'explicit currentRAM/ESP/own-high/FLAGS inputs replace stopped candidates'); counts.current_inputs++;
    unmap(c, 0x9000); unmap(c, 0xa000);
  }
  const row = rows[1], liveFrame = image(2, [0x4444, 0x3333, 0x2222, 0xdead, 0x1111, 1, 0, 0x7fff]);
  frame(c, DATA + 0x200, liveFrame); seed(c, row.pc, DATA + 0x200, 0x443); pop(c, widthModules[1], row, liveFrame, 'live POPAW returns operands and original carry without reseed');
  const consumer = compile(c, [{pc: livePc, bytes: live, instructions: 6}]); input(c, 48, words([livePc]));
  let next = cpu(c); next = {...next, registers: [...next.registers], pc: livePc + 3, flags: 0xc92}; next.registers[0] = mergeWord(next.registers[0], 0x8001);
  run(c, consumer, 1, next, {label: 'ADC returned AX+DX+preserved carry'});
  next = cpu(c); next = {...next, registers: [...next.registers], pc: livePc + 6}; next.registers[3] = mergeWord(next.registers[3], 0x1101); run(c, consumer, 1, next, {label: 'SETO BL consumes returned arithmetic OF1'});
  next = cpu(c); next = {...next, registers: [...next.registers], pc: livePc + 9}; next.registers[3] = mergeWord(next.registers[3], 1); run(c, consumer, 1, next, {label: 'SETB BH consumes returned arithmetic CF0'});
  next = cpu(c); next = {...next, registers: [...next.registers], pc: livePc + 13, flags: 0x406}; next.registers[1] = mergeWord(next.registers[1], 0); run(c, consumer, 1, next, {label: 'BSF returned AX uses wholeWORD parity'});
  next = cpu(c); next = {...next, registers: [...next.registers], pc: livePc + 16}; next.registers[2] = mergeWord(next.registers[2], 0); run(c, consumer, 1, next, {label: 'SETZ DL consumes returned index flags'});
  run(c, consumer, 2, {...cpu(c), pc: livePc + live.length}, {label: 'live suffix with budget2', reason: 3}); counts.consumers += 7;
  const retainedBytes = image(4, values); frame(c, DATA + 0x280, retainedBytes); seed(c, rows[0].pc, DATA + 0x280);
  if (owner === 'resident') pop(c, widthModules[0], rows[0], retainedBytes, 'old resident width unit remains current');
  else run(c, widthModules[0], 1, cpu(c), {label: 'old replacement generation3 is neutral', status: 3, retired: 0}); counts.retained++;
  checkRam(c, 0x8000, canary); checkRam(c, PC, c.code); assert.equal(c.api.close(), 0);
}
assert.deepEqual(sources(), sourcePins);
assert.equal(counts.contexts, 4); assert.equal(modules.length, 12); assert.equal(counts.runs, 1608); assert.equal(counts.retired, 1478);
assert.equal(counts.flag_inputs, 1024); assert.equal(counts.frames, 128); assert.equal(counts.boundaries, 64); assert.equal(counts.faults, 192);
assert.equal(counts.repairs, 72); assert.equal(counts.current_inputs, 8); assert.equal(counts.consumers, 28);
const result = {status: 'ok', engine: {bytes: engine.bytes.length, sha256: engine.sha256}, source_pins: sourcePins,
  counts: {...counts, modules: modules.length}, modules, observations,
  limits: ['exact61/6661; four private flat32 bound profiles; eight-slot arithmetic images and one checked stack transaction which skips the saved-SP slot',
    'all128FLAGS/own-parent high16 and full4612/dirtyFP preserved; selected live RAM/canaries checked after operations',
    'real stack_execute wrappers observe exact control/staged GPRs and forward; selected read candidates are source-derived metadata, not an internal per-read trace; no synthetic packets or hardware partial-write claim',
    'map/protect-only repairs are separate from declared currentRAM/CPU edits; no guest stores/SMC',
    'three captures/context; maximum two blocks/five instructions in POPA units, six instructions in pure consumer; no performance/CI claim']};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2), {flag: 'wx'});
console.log(JSON.stringify({status: result.status, ...result.counts, output}));
