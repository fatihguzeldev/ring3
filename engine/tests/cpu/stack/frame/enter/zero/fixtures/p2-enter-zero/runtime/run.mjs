import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../../../../../../../../support/wasm/engine.mjs';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root);
const engine = readEngine(enginePath), SIZE = 4612, PC = 0x1000, DATA = 0x5000, HIGH = 0xa53c5000;
const initial = readFileSync(join(output, 'initial-arena.bin'));
assert.equal(initial.length, SIZE); assert.deepEqual(WebAssembly.Module.imports(engine.module), []);
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const paths = [
  'engine/src/cpu/dbt/wasm/segments/access.rs', 'engine/src/cpu/x86/segments/cache.rs',
  'engine/src/abi/arena.rs', 'engine/src/abi/memory_helper.rs', 'engine/src/cpu/x86/ir.rs',
  'engine/src/cpu/x86/decode/decoder.rs', 'engine/src/cpu/x86/decode/lower.rs', 'engine/src/cpu/x86/decode/flow.rs',
  'engine/src/cpu/x86/decode/profile.rs', 'engine/src/cpu/dbt/region.rs', 'engine/src/cpu/dbt/wasm/integer/arithmetic.rs', 'engine/src/cpu/dbt/wasm/integer/bits.rs', 'engine/src/cpu/dbt/wasm/integer/carry_rotates.rs', 'engine/src/cpu/dbt/wasm/integer/decimal.rs', 'engine/src/cpu/dbt/wasm/integer/dispatch.rs', 'engine/src/cpu/dbt/wasm/integer/flags.rs', 'engine/src/cpu/dbt/wasm/integer/mod.rs', 'engine/src/cpu/dbt/wasm/integer/multiply_divide.rs', 'engine/src/cpu/dbt/wasm/integer/operands.rs', 'engine/src/cpu/dbt/wasm/integer/rotates.rs', 'engine/src/cpu/dbt/wasm/integer/shifts.rs',
  'engine/src/cpu/dbt/wasm/memory.rs', 'engine/src/cpu/dbt/wasm/memory/word_store.rs', 'engine/src/cpu/dbt/wasm/memory/narrow.rs',
  'engine/src/cpu/dbt/wasm/abi.rs', 'engine/src/cpu/dbt/wasm/emitter.rs', 'engine/src/memory/space.rs',
  'engine/src/process/instance.rs', 'engine/src/process/execution/compiled.rs', 'engine/src/process/execution/authority.rs', 'engine/src/process/memory/address_space.rs', 'engine/src/process/memory/scalar.rs', 'engine/src/process/memory/scalar/tests.rs', 'engine/src/process/execution/mod.rs', 'engine/src/process/memory/mod.rs', 'engine/src/process/mod.rs', 'engine/src/process/execution/resident.rs',    'engine/src/process/wasm/mod.rs', 'engine/src/process/wasm/loading.rs', 'engine/src/process/wasm/execution.rs', 'engine/src/process/wasm/calls.rs', 'engine/src/process/wasm/memory.rs', 'engine/src/process/wasm/cpu.rs',
  'engine/src/cpu/x86/stack.rs', 'engine/src/cpu/x86/decode/stack.rs', 'engine/src/cpu/dbt/wasm/stack.rs', 'engine/src/process/cpu/stack.rs', 'engine/src/process/cpu/mod.rs',  'engine/src/abi/cpu_helper.rs',
  'engine/tests/cpu/stack/frame/enter/zero/native.rs', 'engine/tests/cpu/stack/native.rs',  'engine/tests/cpu/stack/frame/enter/zero/wasm.rs', 'engine/tests/cpu/stack/wasm.rs', 'engine/tests/support/wasm.rs',
  'engine/tests/cpu/stack/frame/enter/zero/fixtures/p2-enter-zero/runtime/run.mjs', 'engine/tests/support/wasm/engine.mjs',
];
assert.equal(new Set(paths).size, paths.length);
const sources = () => Object.fromEntries(paths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = sources(), modules = [], observations = [], contexts = [];
const counts = {contexts: 0, runs: 0, retired: 0, flag_inputs: 0, matrix: 0, anchors: 0, repeats: 0,
  current_inputs: 0, controls: 0, faults: 0, repairs: 0, consumers: 0, retained: 0, currentness: 0,
  self_code: 0, helper_calls: 0, stack_calls: 0, store_calls: 0, read_calls: 0};
const wrap = n => Number((BigInt(n) % 4294967296n + 4294967296n) % 4294967296n);
const low = n => Number((BigInt(n) % 65536n + 65536n) % 65536n);
const merge = (old, value) => Math.floor(old / 65536) * 65536 + low(value);
const digits = (n, width) => Buffer.from(Array.from({length: width}, (_, i) => Math.floor(n / 256 ** i) % 256));
const words = values => {const b = Buffer.alloc(values.length * 4); values.forEach((v, i) => b.writeUInt32LE(v >>> 0, i * 4)); return b;};
const flags = Array.from({length: 128}, (_, n) => 2 + [1, 4, 16, 64, 128, 1024, 2048]
  .reduce((sum, bit, i) => sum + Math.floor(n / 2 ** i) % 2 * bit, 0));
assert.equal(new Set(flags).size, 128); assert.ok(flags.every(n => (n & 2) && !(n & ~0xcd7)));
function record(magic, size, fields, version = 1) {
  const b = Buffer.alloc(size); b.write(magic); b.writeUInt16LE(version, 4); b.writeUInt16LE(1, 6); b.writeUInt32LE(size, 8);
  fields.forEach((v, i) => b.writeUInt32LE(v >>> 0, 16 + i * 4)); return b;
}
const state = c => record('R3ST', 56, [...c.registers, c.pc, c.flags]);
const exit = (reason, retired, fault, version = 2) => record('R3EX', 40,
  [reason, retired, fault?.detail ?? 0, fault?.address ?? 0, fault?.access ?? 0, fault?.segment ? 0 : fault?.width ?? 0], version);
function helper(event, fault) {
  if (event.stack) return record('R3MH', 40, [fault?.segment ? 7 : fault ? 1 : 0, 0, fault?.detail ?? 0, fault?.address ?? 0, fault?.access ?? 0, fault ? event.width : 0], 7);
  const version = event.width === 4 ? 1 : event.access === 1 ? 2 : 4;
  return record('R3MH', 40, [fault ? 1 : 0, fault ? 0 : event.access === 1 ? event.value : 0,
    fault?.detail ?? 0, fault?.address ?? 0, fault?.access ?? 0, event.width === 2 || fault ? event.width : 0], version);
}
const arena = c => Buffer.from(new Uint8Array(c.memory.buffer, c.base, SIZE));
function input(c, offset, bytes) {
  const expected = arena(c); expected.set(bytes, offset); new Uint8Array(c.memory.buffer).set(bytes, c.base + offset);
  assert.deepEqual(arena(c), expected, 'declared host input preserves all other arena bytes');
}
function host(c, name, args) {
  const expected = arena(c); assert.equal(c.api[name](...args), 0, name);
  if (name.startsWith('compile_resident')) {
    const b = arena(c); assert.equal(b.readUInt32LE(140), 1); assert.equal(b.readUInt32LE(144), 24); expected.set(b.subarray(140, 164), 140);
  }
  assert.deepEqual(arena(c), expected, 'host arena effect: ' + name);
}
function map(c, address, permission = 3) {
  host(c, 'map', [address, 1, permission]); c.mapped.add(address);
  c.maxPages = Math.max(c.maxPages ?? 0, c.mapped.size); assert.ok(c.maxPages <= 12);
}
function unmap(c, address) {if (c.mapped.has(address)) {host(c, 'unmap', [address, 1]); c.mapped.delete(address);}}
function upload(c, address, bytes) {input(c, 140, bytes); host(c, 'upload', [address, bytes.length]);}
function checkRam(c, address, bytes) {
  for (const [i, value] of bytes.entries()) {
    const expected = arena(c); expected.set(record('R3MH', 40, [0, value, 0, 0, 0, 1], 2), 100);
    assert.equal(c.api.read8(wrap(BigInt(address) + BigInt(i))), 0); assert.deepEqual(arena(c), expected, 'selected real RAM byte/canary');
  }
}
const cpu = c => {const b = arena(c); return {registers: Array.from({length: 8}, (_, i) => b.readUInt32LE(16 + i * 4)), pc: b.readUInt32LE(48), flags: b.readUInt32LE(52)};};
const registers = () => {const r = Array.from({length: 8}, (_, i) => (0xa100 + i) * 65536 + 0x1234 + i); r[4] = HIGH + 0x80; return r;};
function seed(c, pc, r = registers(), flag = 0xcd7) {
  const b = Buffer.alloc(140); b.set(state({registers: r, pc, flags: flag})); b.set(exit(3, 0), 56);
  b.set(record('R3MH', 40, [0, 0xdecafbad, 0, 0, 0, 0]), 100); input(c, 0, b); return cpu(c);
}
function run(c, unit, budget, next, {label, reason = 1, retired = 1, fault, events = [], status = 0} = {}) {
  const expected = arena(c);
  if (status === 0) {
    expected.set(state(next)); expected.set(exit(reason, retired, fault, unit.exitVersion), 56);
    if (events.length) expected.set(helper(events.at(-1), fault), 100);
    const stack = events.find(event => event.stack);
    if (stack) expected.set(words(fault ? stack.input : stack.result), 140);
  }
  c.trace = []; const observedStatus = unit.run(c.base, c.base + 56, budget, c.base + 96);
  assert.equal(observedStatus, status, label);
  const expectedEvents = events.map(event => event.stack && fault ? {...event, result: event.input} : event);
  assert.deepEqual(c.trace, expectedEvents, 'observed canonical stack/read chronology: ' + label);
  assert.deepEqual(arena(c), expected, 'full4612 GPR/FLAGS/helper/dirtyFP effect: ' + label);
  const b = arena(c);
  observations.push({context: c.id, label, expected_status: status, observed_status: observedStatus,
    expected_reason: status === 0 ? reason : null, observed_reason: status === 0 ? b.readUInt32LE(72) : null,
    expected_retired: status === 0 ? retired : null, observed_retired: status === 0 ? b.readUInt32LE(76) : null,
    expected_pc: status === 0 ? next.pc : null, observed_pc: status === 0 ? b.readUInt32LE(48) : null,
    expected_flags: status === 0 ? next.flags : null, observed_flags: status === 0 ? b.readUInt32LE(52) : null, events: c.trace});
  counts.runs++; if (status === 0) counts.retired += b.readUInt32LE(76);
  counts.helper_calls += c.trace.length; counts.stack_calls += c.trace.filter(e => e.stack).length; counts.store_calls += c.trace.filter(e => e.access === 2 && !e.stack).length; counts.read_calls += c.trace.filter(e => e.access === 1).length;
}
function compile(c, blocks) {
  const instructions = blocks.reduce((n, b) => n + b.instructions, 0), bytesTotal = blocks.reduce((n, b) => n + b.bytes.length, 0);
  assert.ok(blocks.length > 0 && blocks.length <= 8); assert.ok(instructions <= 64); assert.ok(bytesTotal <= 65536); assert.ok(++c.captures <= 6);
  input(c, 140, words(blocks.flatMap(b => c.entries ? [b.pc] : [b.pc, b.bytes.length])));
  const method = c.owner === 'resident' ? (c.entries ? 'compile_resident_entries' : 'compile_resident') : (c.entries ? 'compile_entries' : 'compile');
  host(c, method, c.entries ? [blocks.length, 0] : [blocks.length]);
  const b = arena(c), binding = c.owner === 'resident'
    ? {low: b.readUInt32LE(148), high: b.readUInt32LE(152), pointer: b.readUInt32LE(156), length: b.readUInt32LE(160)}
    : {generation: c.api.generation(), pointer: c.api.module_ptr() >>> 0, length: c.api.module_len() >>> 0};
  const bytes = Buffer.from(new Uint8Array(c.memory.buffer, binding.pointer, binding.length)), module = new WebAssembly.Module(bytes);
  const needs = name => blocks.some(b => b[name]);
  const storeName = width => c.owner === 'resident' ? 'store_resident' + width : 'store' + width;
  const names = [c.owner === 'resident' ? 'guard_resident' : 'guard', ...(needs('read32') ? ['read32'] : []),
    ...(needs('store32') ? [storeName(32)] : []), ...(needs('read16') ? ['read16'] : []), ...(needs('store16') ? [storeName(16)] : []), ...(needs('stack') ? [c.owner === 'resident' ? 'stack_execute_resident' : 'stack_execute'] : [])];
  assert.deepEqual(WebAssembly.Module.imports(module), [{module: 'env', name: 'memory', kind: 'memory'}, ...names.map(name => ({module: 'ring3', name, kind: 'function'}))]);
  assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  const imports = {...c.api};
  for (const width of [2, 4]) {
    const bits = width * 8;
    imports['read' + bits] = address => {
      const status = c.api['read' + bits](address), value = arena(c).readUInt32LE(120);
      c.trace.push({access: 1, width, address: address >>> 0, value}); return status;
    };
    imports['store' + bits] = (address, value) => {
      c.trace.push({access: 2, width, address: address >>> 0, value: value >>> 0}); return c.api['store' + bits](address, value);
    };
    imports['store_resident' + bits] = (...args) => {
      assert.deepEqual(args.slice(0, 4).map(n => n >>> 0), [c.id, c.keyHigh, binding.low, binding.high]);
      c.trace.push({access: 2, width, address: args[4] >>> 0, value: args[5] >>> 0}); return c.api['store_resident' + bits](...args);
    };
  }
  const stackName = c.owner === 'resident' ? 'stack_execute_resident' : 'stack_execute';
  imports[stackName] = (...args) => {
    if (c.owner === 'resident') assert.deepEqual(args.slice(0, 4).map(n => n >>> 0), [c.id, c.keyHigh, binding.low, binding.high]);
    const before = arena(c), original = Array.from({length: 8}, (_, i) => before.readUInt32LE(140 + i * 4));
    const control = args.at(-1) >>> 0, width = control === 4 ? 4 : 2; assert.ok(control === 4 || control === 5);
    const status = c.api[stackName](...args), after = arena(c), allowed = Buffer.from(before); allowed.set(after.subarray(100, 172), 100);
    assert.deepEqual(after, allowed, 'stack helper changes only strict packet and staged GPR result');
    c.trace.push({access: 2, width, address: wrap(BigInt(original[4]) - BigInt(width)), value: width === 4 ? original[5] : low(original[5]),
      stack: true, control, input: original, result: Array.from({length: 8}, (_, i) => after.readUInt32LE(140 + i * 4))});
    return status;
  };
  const file = `module-${modules.length + 1}.wasm`; writeFileSync(join(output, file), bytes, {flag: 'wx'});
  modules.push({context: c.id, purpose: c.purpose, capture: c.captures, owner: c.owner, entries: c.entries, file,
    sha256: hash(bytes), imports: names, blocks: blocks.length, instructions, bytes: bytesTotal});
  return {...binding, exitVersion: needs('stack') ? 6 : names.length > 1 ? 2 : 1, run: new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3: imports}).exports.run};
}
const enterBytes = width => Buffer.from(width === 4 ? [0xc8, 0, 0, 0] : [0x66, 0xc8, 0, 0, 0]);
const enterBlock = (pc, width) => ({pc, width, stack: true, instructions: 2, bytes: Buffer.concat([enterBytes(width), Buffer.from([0xeb, 0])])});
const rows = [enterBlock(PC, 4), enterBlock(PC + 0x40, 2)], mov = Buffer.from([0xbb, 0x44, 0x33, 0x22, 0x11]);
const faultRows = [enterBlock(PC + 0x105, 4), enterBlock(PC + 0x145, 2)];
const faultBlocks = faultRows.map(row => ({...row, pc: row.pc - 5, instructions: 3, bytes: Buffer.concat([mov, row.bytes])}));
const liveBlocks = [4, 2].map((width, i) => ({pc: PC + 0x200 + i * 0x40, width, instructions: 6,
  stack: true, ['read' + width * 8]: true,
  bytes: Buffer.concat([enterBytes(width), Buffer.from(width === 4 ? [0xc9] : [0x66, 0xc9]),
    Buffer.from([0x66, 0x11, 0xd0, 0x0f, 0x92, 0xc3, 0x0f, 0x90, 0xc7, 0xeb, 0])])}));
const canary = Buffer.from([0x55, 0xa5, 0x66, 0x67, 0xf0, 0xf2, 0xf3, 0xaa]);
function open(owner, entries, purpose = 'core') {
  const instance = new WebAssembly.Instance(engine.module, {}), c = {id: ++counts.contexts, keyHigh: 0x454e5452,
    owner, entries, purpose, captures: 0, memory: instance.exports.memory, api: {}, mapped: new Set(), trace: []};
  for (const name of ['open', 'close', 'arena_ptr', 'map', 'unmap', 'protect', 'upload', 'read8', 'read16', 'read32', 'store16', 'store32', 'store_resident16', 'store_resident32', 'stack_execute', 'stack_execute_resident',
    'compile', 'compile_entries', 'compile_resident', 'compile_resident_entries', 'generation', 'module_ptr', 'module_len', 'guard', 'guard_resident'])
    c.api[name] = instance.exports['ring3_abi_v1_' + name];
  assert.equal(c.api.open(12, c.id, c.keyHigh), 0); c.base = c.api.arena_ptr() >>> 0; input(c, 0, initial);
  if (purpose !== 'core') return c;
  for (const address of [PC, DATA, DATA + 4096, HIGH, 0xf000, 0x10000, 0xfffff000, 0, 0x8000]) map(c, address, address === PC ? 7 : 3);
  for (const b of [...rows, ...faultBlocks, ...liveBlocks]) upload(c, b.pc, b.bytes);
  upload(c, 0x8000, canary); host(c, 'protect', [0x8000, 1, 1]); return c;
}
function close(c, captures) {
  assert.equal(c.captures, captures); contexts.push({id: c.id, owner: c.owner, entries: c.entries, purpose: c.purpose,
    captures, maximum_mapped_pages: c.maxPages}); assert.equal(c.api.close(), 0);
}
function transition(before, row) {
  const next = {...before, registers: [...before.registers], pc: wrap(BigInt(before.pc) + BigInt(enterBytes(row.width).length))};
  const address = wrap(BigInt(before.registers[4]) - BigInt(row.width));
  const value = row.width === 4 ? before.registers[5] : low(before.registers[5]);
  next.registers[4] = address; next.registers[5] = row.width === 4 ? address : merge(before.registers[5], address);
  return {next, address, value, event: {access: 2, width: row.width, address, value, stack: true, control: row.width === 4 ? 4 : 5, input: [...before.registers], result: [...next.registers]}};
}
function operation(c, unit, row, label, reason = 1) {
  const effects = transition(cpu(c), row); run(c, unit, 1, effects.next, {label, reason, events: [effects.event]});
  checkRam(c, effects.address, digits(effects.value, row.width)); return effects;
}
function framed(c, address, width, value = 0x9234800f) {
  const start = Math.max(0, address - 1), end = Math.min(4294967296, address + width + 1), valueBytes = digits(value, width);
  const bytes = Buffer.from(Array.from({length: end - start}, (_, i) => start + i < address ? 0x55
    : start + i >= address + width ? 0xaa : valueBytes[start + i - address]));
  upload(c, start, bytes); return {address: start, bytes};
}
const literalAnchors = [
  [4, DATA + 8, 0xdeadbeef, DATA + 4, DATA + 4], [4, DATA + 9, 0xdead6008, DATA + 5, DATA + 5],
  [4, HIGH + 8, 0x1234ffff, HIGH + 4, HIGH + 4], [4, 0x10002, 0x87654321, 0xfffe, 0xfffe],
  [4, 0, 0x12340000, 0xfffffffc, 0xfffffffc], [4, 4, 0xffff8765, 0, 0],
  [4, DATA + 0x1001, 0xcafedead, DATA + 0xffd, DATA + 0xffd], [4, 0xffffffff, 0x56780000, 0xfffffffb, 0xfffffffb],
  [2, DATA + 9, 0xdeadbeef, DATA + 7, 0xdead5007], [2, HIGH + 8, 0x1234ffff, HIGH + 6, 0x12345006],
  [2, 0x10001, 0x87654321, 0xffff, 0x8765ffff], [2, 0x10000, 0x12340001, 0xfffe, 0x1234fffe],
  [2, 0, 0x12340000, 0xfffffffe, 0x1234fffe], [2, 2, 0xffff8765, 0, 0xffff0000],
  [2, DATA + 0x1001, 0xcafedead, DATA + 0xfff, 0xcafe5fff], [2, 0xffffffff, 0x56780000, 0xfffffffd, 0x5678fffd],
];
for (const [width, esp, ebp, address, frame] of literalAnchors) {
  const r = registers(); r[4] = esp; r[5] = ebp;
  const e = transition({registers: r, pc: PC, flags: 0xcd7}, {width});
  assert.equal(e.address, address); assert.equal(e.next.registers[4], address); assert.equal(e.next.registers[5], frame);
  assert.deepEqual(digits(e.value, width), digits(ebp, width));
}
function checkFrame(c, frame, address, value, width) {
  const expected = Buffer.from(frame.bytes); expected.set(digits(value, width), address - frame.address); checkRam(c, frame.address, expected);
}
const failureNames = ['unmapped', 'none', 'read-only', 'execute-only', 'cross-missing', 'cross-denied', 'overflow'];
function failureSetup(c, name, width) {
  unmap(c, 0x9000); unmap(c, 0xa000);
  const crossing = name.startsWith('cross-'), address = name === 'overflow' ? 0xffffffff : crossing ? 0x9fff : 0x9008;
  let frame;
  if (name !== 'unmapped' && name !== 'overflow') {
    map(c, 0x9000); if (name === 'cross-denied') map(c, 0xa000);
    if (name === 'cross-missing') upload(c, 0x9ffe, Buffer.from([0x55, 0x0f])); else frame = framed(c, address, width);
    if (!crossing) host(c, 'protect', [0x9000, 1, name === 'none' ? 0 : name === 'execute-only' ? 4 : 1]);
    if (name === 'cross-denied') host(c, 'protect', [0xa000, 1, 1]);
  }
  if (name === 'overflow') {
    upload(c, 0xfffffffc, Buffer.from([0x55, 0xa5, 0x66, 0x67])); upload(c, 0, Buffer.from([0x5a, 0xaa, 0x11, 0x22]));
  }
  return {name, address, frame, width, detail: name === 'overflow' ? 3 : name === 'unmapped' || name === 'cross-missing' ? 1 : 2,
    faultAddress: crossing ? 0xa000 : address, original: name === 'unmapped' ? 0 : name === 'cross-missing' ? 0x0f : width === 4 ? 0x9234800f : 0x800f};
}
function failureRam(c, f) {
  if (f.name === 'overflow') {
    checkRam(c, 0xfffffffc, Buffer.from([0x55, 0xa5, 0x66, 0x67])); checkRam(c, 0, Buffer.from([0x5a, 0xaa, 0x11, 0x22]));
  } else if (f.name === 'cross-missing') checkRam(c, 0x9ffe, Buffer.from([0x55, 0x0f]));
  else if (f.name === 'read-only' || f.name === 'cross-denied') checkRam(c, f.frame.address, f.frame.bytes);
}
function repair(c, f) {
  if (f.name === 'unmapped') map(c, 0x9000);
  else if (f.name === 'cross-missing') map(c, 0xa000);
  else host(c, 'protect', [f.name === 'cross-denied' ? 0xa000 : 0x9000, 1, 3]);
}
function stopped(c, unit, row, f, r, label) {
  const before = seed(c, row.pc - 5, r), next = {...before, registers: [...before.registers], pc: row.pc}; next.registers[3] = 0x11223344;
  const effects = transition(next, row), segment = f.name === 'overflow', reason = segment ? 15 : 5;
  const fault = {segment, detail: segment ? 0 : f.detail, address: segment ? 0 : f.faultAddress, access: segment ? 0 : 2, width: row.width};
  run(c, unit, 2, next, {label: label + ' after prior MOV retirement', reason, retired: 1, fault, events: [effects.event]}); failureRam(c, f);
  run(c, unit, 1, next, {label: label + ' unchanged retry', reason, retired: 0, fault, events: [effects.event]}); failureRam(c, f);
  counts.faults += 2; return next;
}
for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries); let oldest;
  for (const row of rows) {
    const unit = compile(c, [row]); oldest ??= unit;
    for (const flag of flags) {
      const r = registers(); r[5] = 0xdeadbeef; const at = r[4] - row.width, frame = framed(c, at, row.width);
      seed(c, row.pc, r, flag); const e = operation(c, unit, row, 'all128 valid FLAGS preserved with unmapped original EBP value');
      checkFrame(c, frame, e.address, e.value, row.width); counts.flag_inputs++;
    }
    for (const ebp of [0, 1, 0xffff, 0x10000, 0xdeadbeef, 0xffffffff]) for (const esp of [DATA + 8, DATA + 9, HIGH + 8, DATA + 0x1001]) {
      const r = registers(); r[4] = esp; r[5] = ebp; const frame = framed(c, esp - row.width, row.width);
      seed(c, row.pc, r, ebp % 2 ? 0x443 : 0x892); const e = operation(c, unit, row, 'old EBP is a value; full32 ESP and original own high half determine new frame');
      checkFrame(c, frame, e.address, e.value, row.width); counts.matrix++;
    }
    for (const [width, esp, ebp, address, frameValue] of literalAnchors.filter(a => a[0] === row.width)) {
      const r = registers(); r[4] = esp; r[5] = ebp; const frame = framed(c, address, width);
      seed(c, row.pc, r); const e = operation(c, unit, row, 'literal odd/carry/high-mismatch/wrap/top/cross-page stack');
      assert.equal(e.address, address); assert.equal(e.next.registers[5], frameValue); checkFrame(c, frame, address, e.value, width); counts.anchors++;
    }
    const r = registers(); r[4] = HIGH + 0x80; r[5] = 0x1234beef;
    seed(c, row.pc, r); const first = operation(c, unit, row, 'first frame becomes current returned register input'); input(c, 48, words([row.pc]));
    const second = operation(c, unit, row, 'PC-only repeat pushes returned frame without CPU reseed');
    checkRam(c, first.address, digits(first.value, row.width)); checkRam(c, second.address, digits(first.next.registers[5], row.width)); counts.repeats += 2;
    seed(c, row.pc, r); const current = cpu(c); current.registers[4] = DATA + 0x43; current.registers[5] = 0x87654321; current.flags = 0x892;
    input(c, 0, state(current)); operation(c, unit, row, 'separate explicit current EBP/ESP/FLAGS input'); counts.current_inputs++;
    seed(c, row.pc, r); run(c, unit, 0, cpu(c), {label: 'budget0 before ENTER write', retired: 0});
    input(c, 96, words([1])); run(c, unit, 1, cpu(c), {label: 'cancel before ENTER write', reason: 2, retired: 0});
    run(c, unit, 0, cpu(c), {label: 'cancel precedes exhausted budget', reason: 2, retired: 0}); input(c, 96, words([0]));
    const suffix = row.pc + enterBytes(row.width).length; input(c, 48, words([suffix]));
    run(c, unit, 2, {...cpu(c), pc: suffix + 2}, {label: 'ordinary ENTER fallthrough reaches JMP and cold lookup', reason: 3});
    run(c, unit, 1, cpu(c), {label: 'cold lookup leaves current frame unchanged', reason: 3, retired: 0}); counts.controls += 5;
  }
  const faults = compile(c, faultBlocks);
  for (const row of faultRows) {
    for (const name of failureNames) {
      const f = failureSetup(c, name, row.width), r = registers(); r[4] = wrap(BigInt(f.address) + BigInt(row.width)); r[5] = 0xdeadbeef;
      const before = stopped(c, faults, row, f, r, name + ' checked original-frame write');
      if (name !== 'overflow') {
        repair(c, f); assert.deepEqual(cpu(c), before, 'mapping/protection-only repair preserves stopped CPU');
        checkRam(c, f.address, digits(f.original, row.width));
        if (f.frame) checkRam(c, f.frame.address, f.frame.bytes);
        const e = operation(c, faults, row, 'repair-only retry pushes original stopped EBP and publishes frame');
        if (f.frame) checkFrame(c, f.frame, e.address, e.value, row.width); counts.repairs++;
      }
      unmap(c, 0x9000); unmap(c, 0xa000);
    }
    const f = failureSetup(c, 'read-only', row.width), r = registers(); r[4] = 0x9008 + row.width; r[5] = 0xdeadbeef;
    const before = stopped(c, faults, row, f, r, 'separate current-input retry starts with a retained write fault');
    repair(c, f); assert.deepEqual(cpu(c), before); const current = {...before, registers: [...before.registers], flags: 0x892};
    current.registers[4] = HIGH + 0x43; current.registers[5] = 0x87654321;
    const frame = framed(c, HIGH + 0x43 - row.width, row.width); input(c, 0, state(current));
    const e = operation(c, faults, row, 'explicit changed EBP/ESP/FLAGS consumed after fault, distinct from repair-only');
    checkFrame(c, frame, e.address, e.value, row.width); checkRam(c, f.frame.address, f.frame.bytes); counts.current_inputs++; unmap(c, 0x9000);
  }
  const mixed = compile(c, liveBlocks), r = registers(); r[5] = 0x1234beef; seed(c, rows[0].pc, r);
  if (owner === 'resident') operation(c, oldest, rows[0], 'old resident frame unit remains current after later captures');
  else run(c, oldest, 1, cpu(c), {label: 'old replacement generation3 rejection is neutral', status: 3, retired: 0}); counts.retained++;
  for (const row of liveBlocks) {
    const r = registers(); r[0] = 0xcafe7fff; r[2] = 0xface0000; r[3] = 0xa103ffff; r[5] = HIGH + 0x20;
    seed(c, row.pc, r, 0x443); const entered = operation(c, mixed, row, 'live ENTER preserves incoming carry and returns compatible frame');
    let before = cpu(c), next = {...before, registers: [...before.registers], pc: before.pc + (row.width === 4 ? 1 : 2)};
    next.registers[4] = HIGH + 0x80; next.registers[5] = HIGH + 0x20;
    run(c, mixed, 1, next, {label: 'live LEAVE consumes returned frame without reseed', events: [{access: 1, width: row.width, address: entered.address, value: row.width === 4 ? HIGH + 0x20 : low(HIGH + 0x20)}]});
    before = cpu(c); next = {...before, registers: [...before.registers], pc: before.pc + 3, flags: 0xc96}; next.registers[0] = merge(before.registers[0], 0x8000);
    run(c, mixed, 1, next, {label: 'live ADC consumes frame-preserved CF1:7fff+0+1=8000'});
    before = cpu(c); next = {...before, registers: [...before.registers], pc: before.pc + 3}; next.registers[3] = merge(before.registers[3], 0xff00);
    run(c, mixed, 1, next, {label: 'SETB BL observes CF0 from returned arithmetic'});
    before = cpu(c); next = {...before, registers: [...before.registers], pc: before.pc + 3}; next.registers[3] = merge(before.registers[3], 0x0100);
    run(c, mixed, 1, next, {label: 'SETO BH observes OF1 without AX or frame reseed'});
    run(c, mixed, 2, {...cpu(c), pc: row.pc + row.bytes.length}, {label: 'live suffix JMP exits cold after six retirements', reason: 3});
    run(c, mixed, 1, cpu(c), {label: 'cold lookup preserves live returned frame and flags', reason: 3, retired: 0}); counts.consumers += 7;
  }
  upload(c, liveBlocks[0].pc, liveBlocks[0].bytes.subarray(0, 1));
  run(c, mixed, 1, cpu(c), {label: 'same-byte consumed RWX source invalidates current owner4 before cold lookup', status: 4, retired: 0}); counts.currentness++;
  checkRam(c, 0x8000, canary); close(c, 4);
}
for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) for (const width of [4, 2]) for (const changed of [false, true]) {
  const c = open(owner, entries, `SMC-${width}-${changed ? 'changed' : 'identity'}`); map(c, PC, 7); map(c, 0x8000);
  upload(c, 0x8000, canary); host(c, 'protect', [0x8000, 1, 1]);
  const row = enterBlock(PC, width); upload(c, PC, row.bytes); const unit = compile(c, [row]);
  const r = registers(); r[4] = PC + width; r[5] = width === 4 ? (changed ? 0xdeadbeef : 0xc8) : 0xdead0000 + (changed ? 0x1234 : 0xc866);
  seed(c, PC, r, changed ? 0x892 : 0x443); const e = operation(c, unit, row, 'changed/identity own-opcode ENTER commits original pointer/frame/nextPC once before SMC6', 6);
  assert.equal(e.next.registers[4], PC); assert.equal(e.next.registers[5], width === 4 ? PC : 0xdead1000);
  run(c, unit, 1, cpu(c), {label: 'same consumed source-owner4 is neutral before new capture', status: 4, retired: 0});
  const suffix = PC + enterBytes(width).length, tail = compile(c, [{pc: suffix, bytes: Buffer.from([0xeb, 0]), instructions: 1}]);
  run(c, tail, 2, {...cpu(c), pc: suffix + 2}, {label: 'fresh helper-free suffix never replays ENTER', reason: 3});
  checkRam(c, PC, digits(e.value, width)); checkRam(c, 0x8000, canary); close(c, 2); counts.self_code += 3;
}
assert.deepEqual(sources(), sourcePins);
assert.deepEqual(counts, {contexts: 20, runs: 1640, retired: 1514, flag_inputs: 1024, matrix: 192, anchors: 64,
  repeats: 16, current_inputs: 16, controls: 40, faults: 128, repairs: 48, consumers: 56, retained: 4,
  currentness: 4, self_code: 48, helper_calls: 1522, stack_calls: 1514, store_calls: 0, read_calls: 8});
assert.equal(modules.length, 48);
const result = {status: 'ok', engine_sha256: engine.sha256, source_pins: sourcePins, counts, contexts, modules, observations,
  limits: [
    'this retained fixture covers C8 0000 00 and66 C8 0000 00 under four bound profiles; general allocation/nesting is covered by the dedicated stack-family fixture',
    'independent BigInt full32 stack subtraction, original pointer little-endian digits and own-high WORD merge;16 literal anchors include odd bit0/high disagreement/carry/wrap',
    'all128 admitted FLAGS/form/profile, original EBP never dereferenced, complete4612 GPR/helper/dirtyFP128 effects and selected real RAM/canaries',
    'six paging/write-permission faults and one cached-SS span overflow/form with prior MOV retirement and unchanged retry; Ring3 checks the full logical span before RAM; mapping/protection-only repair and separately declared current EBP/ESP/FLAGS inputs',
    'observed one canonical V7 stack transaction per ENTER plus unchanged read32/read16 LEAVE; exact staged input/result GPRs, live ENTER→LEAVE→ADC/SETcc and fullarena are checked',
    'identity/changed own-code writes commit frame/nextPC/one retirement before6; old owner4 neutral before fresh helper-free suffix, replacement generation remains3',
    'four core/two SMC captures; each unit1/2blocks and1/2/6/12instructions within asserted8/64/65536/sixcapture limits; mapped-page peak tracked within12',
    'software flat SS.B32 stack addresses/allocation with word BP merge; transactional fault publication, selected RAM, no hardware partial-write equivalence/performance/SDK/browser/game/hostedCI claim',
  ]};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2), {flag: 'wx'});
console.log(JSON.stringify({status: 'ok', ...counts, modules: modules.length, output}));
