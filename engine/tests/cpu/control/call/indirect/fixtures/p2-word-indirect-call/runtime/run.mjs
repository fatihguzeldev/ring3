import {pageFaultExit, pageFaultPacket} from '../../../../../../../support/wasm/page-fault.mjs';
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
assert.equal(initial.length, SIZE); assert.deepEqual(WebAssembly.Module.imports(engine.module), []);
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const paths = [
  'engine/src/abi/arena.rs', 'engine/src/abi/memory_helper.rs',
  'engine/src/cpu/dbt/wasm/instructions/stack/pointer.rs', 'engine/src/cpu/dbt/wasm/instructions/segments/access.rs',
  'engine/src/cpu/x86/ir.rs', 'engine/src/cpu/x86/decode/decoder.rs', 'engine/src/cpu/x86/decode/instruction.rs', 'engine/src/cpu/x86/decode/lower.rs', 'engine/src/cpu/x86/decode/families.rs',
  'engine/src/cpu/x86/decode/flow.rs', 'engine/src/cpu/x86/decode/profile.rs',  'engine/src/cpu/dbt/region.rs',
  'engine/src/cpu/dbt/wasm/instructions/integer/arithmetic.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/bits.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/carry_rotates.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/decimal.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/mod.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/arithmetic.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/moves.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/exchange.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/predicates.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/shifts.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/control.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/extended.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/flags.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/mod.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/multiply_divide.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/operands.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/rotates.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/shifts.rs', 'engine/src/cpu/dbt/wasm/imports/mod.rs', 'engine/src/cpu/dbt/wasm/imports/binding.rs', 'engine/src/cpu/dbt/wasm/imports/context.rs', 'engine/src/cpu/dbt/wasm/imports/needs.rs', 'engine/src/cpu/dbt/wasm/helpers/protocol.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/mod.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/address.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/scalar.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/stack.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/narrow.rs',
  'engine/src/cpu/dbt/wasm/instructions/memory/word_store.rs', 'engine/src/cpu/dbt/wasm/abi.rs', 'engine/src/cpu/dbt/wasm/emitter/mod.rs', 'engine/src/cpu/dbt/wasm/emitter/module.rs', 'engine/src/cpu/dbt/wasm/emitter/body.rs', 'engine/src/cpu/dbt/wasm/requirements.rs', 'engine/src/cpu/dbt/wasm/imports/wiring.rs', 'engine/src/cpu/dbt/wasm/locals.rs',
  'engine/src/memory/space.rs', 'engine/src/memory/space/atomic.rs', 'engine/src/process/instance.rs', 'engine/src/process/execution/compiled.rs', 'engine/src/process/execution/authority.rs', 'engine/src/process/memory/address_space.rs', 'engine/src/process/memory/scalar.rs', 'engine/src/process/memory/scalar/tests.rs', 'engine/src/process/execution/mod.rs', 'engine/src/process/memory/mod.rs', 'engine/src/process/mod.rs', 'engine/src/process/execution/resident.rs', 'engine/src/process/wasm/mod.rs', 'engine/src/process/wasm/loading.rs', 'engine/src/abi/image.rs', 'engine/src/process/wasm/execution.rs', 'engine/src/abi/execution.rs', 'engine/src/process/wasm/calls.rs', 'engine/src/process/wasm/memory.rs', 'engine/src/process/wasm/cpu.rs', 'engine/tests/cpu/control/call/indirect/native/word.rs', 'engine/tests/cpu/control/native.rs', 'engine/tests/support/cpu/prefix/admission.rs', 'engine/tests/support/cpu/prefix/word.rs', 'engine/tests/cpu/control/call/indirect/wasm/word.rs', 'engine/tests/cpu/control/wasm.rs', 'engine/tests/support/wasm.rs', 'engine/tests/cpu/control/call/indirect/fixtures/p2-word-indirect-call/runtime/run.mjs', 'engine/tests/support/wasm/engine.mjs', 'engine/tests/support/wasm/binding.mjs', 'engine/tests/support/wasm/page-fault.mjs',
];
assert.equal(new Set(paths).size, paths.length);
const sources = () => Object.fromEntries(paths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = sources(), modules = [], observations = [];
const counts = {contexts: 0, runs: 0, retired: 0, register_inputs: 0, memory_inputs: 0, flag_inputs: 0,
  boundaries: 0, code_reads: 0, faults: 0, repairs: 0, current_inputs: 0, repeats: 0, controls: 0,
  cold: 0, consumers: 0, retained: 0, currentness: 0, overlap: 0, relative: 0, self_code: 0,
  deferred_store_faults: 0, helper_calls: 0, read_calls: 0, store_calls: 0};
const wrap = n => Number((BigInt(n) % 4294967296n + 4294967296n) % 4294967296n);
const low = n => Number((BigInt(n) % 65536n + 65536n) % 65536n);
const word = n => Buffer.from([n % 256, Math.floor(n / 256)]);
const words = values => {const b = Buffer.alloc(values.length * 4); values.forEach((v, i) => b.writeUInt32LE(v >>> 0, i * 4)); return b;};
const flags = Array.from({length: 128}, (_, n) => 2 + [1, 4, 16, 64, 128, 1024, 2048]
  .reduce((sum, bit, i) => sum + Math.floor(n / 2 ** i) % 2 * bit, 0));
assert.equal(new Set(flags).size, 128); assert.ok(flags.every(n => (n & 2) && !(n & ~0xcd7)));
function record(magic, size, fields, version = 1) {
  const b = Buffer.alloc(size); b.write(magic); b.writeUInt16LE(version, 4); b.writeUInt16LE(1, 6); b.writeUInt32LE(size, 8);
  fields.forEach((v, i) => b.writeUInt32LE(v >>> 0, 16 + i * 4)); return b;
}
const state = c => record('R3ST', 56, [...c.registers, c.pc, c.flags]);
const exit = (reason, retired, f, version) =>
  reason === 5
    ? pageFaultExit({
        retired,
        errorCode: 4 | ((f?.access ?? 0) === 2 ? 2 : 0) | ((f?.detail ?? 0) === 2 ? 1 : 0),
        address: f?.address ?? 0,
        access: f?.access ?? 0,
        length: f ? 2 : 0
      })
    : record(
        'R3EX',
        40,
        [reason, retired, f?.detail ?? 0, f?.address ?? 0, f?.access ?? 0, f ? 2 : 0],
        version
      );
const helper = (value = 0, f) =>
  Boolean(f) && ((f?.detail ?? 0) === 1 || (f?.detail ?? 0) === 2)
    ? pageFaultPacket({
        errorCode: 4 | ((f?.access ?? 0) === 2 ? 2 : 0) | ((f?.detail ?? 0) === 2 ? 1 : 0),
        address: f?.address ?? 0,
        access: f?.access ?? 0,
        length: 2
      })
    : record(
        'R3MH',
        40,
        [f ? 1 : 0, value, f?.detail ?? 0, f?.address ?? 0, f?.access ?? 0, 2],
        10
      );
const arena = c => Buffer.from(new Uint8Array(c.memory.buffer, c.base, SIZE));
function input(c, offset, bytes) {
  const expected = arena(c); expected.set(bytes, offset); new Uint8Array(c.memory.buffer).set(bytes, c.base + offset);
  assert.deepEqual(arena(c), expected, 'declared input preserves all other arena bytes');
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
    assert.equal(c.api.read8(wrap(BigInt(address) + BigInt(i))), 0); assert.deepEqual(arena(c), expected, 'selected RAM/canary');
  }
}
const cpu = c => {const b = arena(c); return {registers: Array.from({length: 8}, (_, i) => b.readUInt32LE(16 + i * 4)), pc: b.readUInt32LE(48), flags: b.readUInt32LE(52)};};
const registers = () => {const r = Array.from({length: 8}, (_, i) => (0xa100 + i) * 65536 + 0x1234 + i); r[4] = HIGH + 0x80; return r;};
function seed(c, pc, r = registers(), flag = 0xcd7) {
  const b = Buffer.alloc(140); b.set(state({registers: r, pc, flags: flag})); b.set(exit(3, 0, undefined, 2), 56);
  b.set(record('R3MH', 40, [0, 0xdecafbad, 0, 0, 0, 0]), 100); input(c, 0, b); return cpu(c);
}
function run(c, unit, budget, next, {label, reason = 1, retired = 1, fault, events = [], value = 0, status = 0} = {}) {
  if (fault?.detail === 3) {
    // The logical span check rejects the failing operand before its page helper.
    // Earlier source reads remain observable when a later stack check fails.
    assert.equal(events.at(-1)?.access, fault.access, 'the final declared event is the failing operand');
    assert.equal(events.at(-1)?.address, fault.address, 'the rejected span keeps its literal address');
    reason = fault.access === 2 || label.startsWith('RET') ? 15 : 11;
    fault = undefined; events = events.slice(0, -1);
  }
  const expected = arena(c);
  if (status === 0) {
    expected.set(state(next)); expected.set(exit(reason, retired, fault, reason === 11 || reason === 15 ? 6 : unit.exitVersion), 56);
    if (events.length) expected.set(helper(fault ? 0 : value, fault), 100);
  }
  c.trace = []; const observedStatus = unit.run(c.base, c.base + 56, budget, c.base + 96);
  assert.equal(observedStatus, status, label); assert.deepEqual(c.trace, events, 'canonical read16/store16 chronology: ' + label);
  assert.deepEqual(arena(c), expected, 'complete arena GPR/FLAGS/helper/dirtyFP effect: ' + label);
  const after = arena(c);
  observations.push({context: c.id, label, expected_status: status, observed_status: observedStatus,
    model_reason: reason, expected_reason: status === 0 ? (reason === 5 ? 23 : reason) : null, observed_reason: status === 0 ? after.readUInt32LE(72) : null,
    expected_retired: status === 0 ? retired : null, observed_retired: status === 0 ? after.readUInt32LE(76) : null,
    expected_pc: status === 0 ? next.pc : null, observed_pc: status === 0 ? after.readUInt32LE(48) : null,
    expected_flags: status === 0 ? next.flags : null, observed_flags: status === 0 ? after.readUInt32LE(52) : null, events: c.trace});
  counts.runs++; if (status === 0) counts.retired += after.readUInt32LE(76); counts.helper_calls += c.trace.length;
  counts.read_calls += c.trace.filter(event => event.access === 1).length;
  counts.store_calls += c.trace.filter(event => event.access === 2).length;
}
function compile(c, blocks) {
  const instructions = blocks.reduce((sum, b) => sum + b.instructions, 0), bytesTotal = blocks.reduce((sum, b) => sum + b.bytes.length, 0);
  assert.ok(blocks.length > 0 && blocks.length <= 8); assert.ok(instructions <= 64); assert.ok(bytesTotal <= 65536); assert.ok(++c.captures <= 6);
  input(c, 140, words(blocks.flatMap(b => c.entries ? [b.pc] : [b.pc, b.bytes.length])));
  const method = c.owner === 'resident' ? (c.entries ? 'compile_resident_entries' : 'compile_resident') : (c.entries ? 'compile_entries' : 'compile');
  host(c, method, c.entries ? [blocks.length, 0] : [blocks.length]);
  const b = arena(c), binding = c.owner === 'resident'
    ? {low: b.readUInt32LE(148), high: b.readUInt32LE(152), pointer: b.readUInt32LE(156), length: b.readUInt32LE(160)}
    : {generation: c.api.generation(), pointer: c.api.module_ptr() >>> 0, length: c.api.module_len() >>> 0};
  const bytes = Buffer.from(new Uint8Array(c.memory.buffer, binding.pointer, binding.length)), module = new WebAssembly.Module(bytes);
  const reads = blocks.some(b => b.read), stores = blocks.some(b => b.store);
  const names = generatedImports({resident: c.owner === 'resident', helpers: [...(reads ? ['cpu_read16'] : []), ...(stores ? [c.owner === 'resident' ? 'cpu_store_resident16' : 'cpu_store16'] : [])]}).slice(1).map(item => item.name);
  assert.deepEqual(WebAssembly.Module.imports(module), [{module: 'env', name: 'memory', kind: 'memory'}, ...names.map(name => ({module: 'ring3', name, kind: 'function'}))]);
  assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  const imports = {...c.api, cpu_read16: address => {c.trace.push({access: 1, address: address >>> 0}); return c.api.cpu_read16(address);},
    cpu_store16: (address, value) => {c.trace.push({access: 2, address: address >>> 0, value: value >>> 0}); return c.api.cpu_store16(address, value);},
    cpu_store_resident16: (...args) => {
      assert.deepEqual(args.slice(0, 4).map(n => n >>> 0), [c.id, c.keyHigh, binding.low, binding.high]);
      c.trace.push({access: 2, address: args[4] >>> 0, value: args[5] >>> 0}); return c.api.cpu_store_resident16(...args);
    }};
  const file = `module-${modules.length + 1}.wasm`; writeFileSync(join(output, file), bytes, {flag: 'wx'});
  modules.push({context: c.id, purpose: c.purpose, capture: c.captures, owner: c.owner, entries: c.entries, file, sha256: hash(bytes), instructions, blocks: blocks.length, bytes: bytesTotal, imports: names});
  return {...binding, exitVersion: reads || stores ? 6 : 1, run: new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3: imports}).exports.run};
}

const regRows = Array.from({length: 8}, (_, source) => ({pc: PC + source * 0x20, source,
  instructions: 1, store: true, bytes: Buffer.from([0x66, 0xff, 0xd0 + source])}));
const eaRows = Array.from({length: 8}, (_, base) => ({base, scale: 1, displacement: 0, r: registers(),
  tail: base === 4 ? [4, 0x24] : base === 5 ? [0x45, 0] : [base]}));
for (const row of eaRows) row.r[row.base] = HIGH + 8;
eaRows.push(
  {base: 5, index: 1, scale: 4, displacement: -4, tail: [0x44, 0x8d, 0xfc], r: registers()},
  {index: 0, scale: 4, displacement: HIGH, tail: [4, 0x85, ...words([HIGH])], r: registers()},
  {base: 0, index: 0, scale: 4, displacement: 0x053c5003, tail: [0x84, 0x80, ...words([0x053c5003])], r: registers()},
  {base: 4, scale: 1, displacement: -1, tail: [0x44, 0x24, 0xff], r: registers()},
  {scale: 1, displacement: HIGH + 8, tail: [5, ...words([HIGH + 8])], r: registers()},
);
eaRows[8].r[5] = HIGH + 4; eaRows[8].r[1] = 0x40000002; eaRows[9].r[0] = 0x40000002;
eaRows[10].r[0] = 0x20000001; eaRows[11].r[4] = HIGH + 9;
for (const [i, row] of eaRows.entries()) {
  row.pc = PC + 0x200 + i * 0x20; row.instructions = 1; row.read = true; row.store = true;
  row.bytes = Buffer.from([0x66, 0xff, row.tail[0] + 0x10, ...row.tail.slice(1)]);
}
const addressOf = (row, r) => wrap(BigInt(row.base === undefined ? 0 : r[row.base]) +
  BigInt(row.index === undefined ? 0 : r[row.index]) * BigInt(row.scale) + BigInt(row.displacement));
for (const row of eaRows) assert.equal(addressOf(row, row.r), 0xa53c5008, 'literal full32 original-parent EA');
for (const [parent, target] of [[0xa53c5008, 0x5008], [0xffff8001, 0x8001], [0xffffffff, 0xffff]]) assert.equal(low(parent), target);
const indirect = Buffer.from([0x66, 0xff, 0x16]), mov = Buffer.from([0xbb, 0x44, 0x33, 0x22, 0x11]);
const faultPC = 0x1705, rxPC = 0xc000;
const highRows = [0x1234fffd, 0xfffffffd].map(pc => ({pc, bytes: regRows[0].bytes, source: 0, store: true, instructions: 1}));
const faultRow = {pc: faultPC, bytes: indirect, read: true, store: true, instructions: 1};
const rxRow = {...faultRow, pc: rxPC};
const liveCaller = {...faultRow, pc: 0x1800};
const liveBlocks = [liveCaller,
  {pc: 0x1900, bytes: Buffer.from([0x66, 0xba, 1, 0, 0x66, 0xc3]), instructions: 2, read: true},
  {pc: 0x1803, bytes: Buffer.from([0x66, 0x11, 0xd0, 0x0f, 0x90, 0xc3, 0x0f, 0x92, 0xc7, 0xeb, 0]), instructions: 4},
];
const auditBlocks = [{...faultRow, pc: faultPC - 5, bytes: Buffer.concat([mov, indirect]), instructions: 2},
  ...highRows, rxRow, ...liveBlocks];
const canary = Buffer.from([0x55, 0xa5, 0x66, 0x67, 0xf0, 0xf2, 0xf3, 0xaa]), contexts = [];
function open(owner, entries, purpose = 'core') {
  const instance = new WebAssembly.Instance(engine.module, {}), c = {id: ++counts.contexts, keyHigh: 0x5749434c,
    owner, entries, purpose, captures: 0, memory: instance.exports.memory, api: {}, mapped: new Set(), trace: []};
  for (const name of ['open', 'close', 'arena_ptr', 'map', 'unmap', 'protect', 'upload', 'read8', 'read16', 'cpu_read16', 'cpu_store16', 'cpu_store_resident16', 'store16', 'store_resident16',
    'compile', 'compile_entries', 'compile_resident', 'compile_resident_entries', 'generation', 'module_ptr', 'module_len', 'guard', 'guard_resident'])
    c.api[name] = instance.exports['ring3_abi_v1_' + name];
 attachDetectionApi(c.api, instance.exports);
  assert.equal(c.api.open(12, c.id, c.keyHigh), 0); c.base = c.api.arena_ptr() >>> 0; input(c, 0, initial);
  if (purpose !== 'core') return c;
  for (const address of [PC, 0x1234f000, DATA, DATA + 4096, HIGH, 0xfffff000, 0, 0x8000, rxPC])
    map(c, address, [PC, 0x1234f000, 0xfffff000, rxPC].includes(address) ? 7 : 3);
  for (const b of [...regRows, ...eaRows, ...auditBlocks]) upload(c, b.pc, b.bytes);
  upload(c, 0, Buffer.from([0x5a, 0xaa])); upload(c, 0x8000, canary);
  host(c, 'protect', [0x8000, 1, 1]); host(c, 'protect', [rxPC, 1, 5]); host(c, 'protect', [0x1234f000, 1, 5]);
  return c;
}
function close(c, captures) {
  assert.equal(c.captures, captures); contexts.push({id: c.id, owner: c.owner, entries: c.entries, purpose: c.purpose, captures,
    maximum_mapped_pages: c.maxPages}); host(c, 'close', []);
}
function effect(before, row, target) {
  const stack = wrap(BigInt(before.registers[4]) - 2n), value = low(BigInt(row.pc) + BigInt(row.bytes.length));
  const next = {...before, registers: [...before.registers], pc: target}; next.registers[4] = stack;
  return {next, stack, value};
}
function call(c, unit, row, target, address, label, budget = 1) {
  const result = effect(cpu(c), row, target), events = address === undefined ? [] : [{access: 1, address}];
  events.push({access: 2, address: result.stack, value: result.value});
  run(c, unit, budget, result.next, {label, reason: budget === 1 ? 1 : 3, events});
  checkRam(c, result.stack, word(result.value)); return result;
}
function mutatedFrame(address, bytes, stack, value) {
  const result = Buffer.from(bytes), stored = word(value);
  for (let i = 0; i < result.length; i++) for (let j = 0; j < 2; j++)
    if (wrap(BigInt(address) + BigInt(i)) === wrap(BigInt(stack) + BigInt(j))) result[i] = stored[j];
  return result;
}
for (const row of highRows) assert.equal(effect({registers: registers(), flags: 2, pc: row.pc}, row, 0x2000).value, 0);
assert.equal(effect({registers: [0, 0, 0, 0, 0, 0, 0, 0], flags: 2, pc: PC}, regRows[4], 0).stack, 0xfffffffe);
assert.equal(effect({registers: [0, 0, 0, 0, 1, 0, 0, 0], flags: 2, pc: PC}, regRows[4], 1).stack, 0xffffffff);
const failures = ['unmapped', 'none', 'wrong-permission', 'execute-only', 'cross-missing', 'cross-denied', 'overflow'];
function failureSetup(c, name, access) {
  for (const address of [0x9000, 0xa000, 0xb000]) unmap(c, address);
  const crossing = name.startsWith('cross-'), address = name === 'overflow' ? 0xffffffff : crossing ? 0x9fff : 0x9008;
  if (name !== 'overflow' && name !== 'unmapped') {
    map(c, 0x9000); if (name === 'cross-denied') map(c, 0xa000);
    if (crossing) {upload(c, 0x9ffe, Buffer.from([0x55, 0x0f])); if (name === 'cross-denied') upload(c, 0xa000, Buffer.from([0x80, 0xaa]));}
    else upload(c, address - 1, Buffer.from([0x55, 0x0f, 0x80, 0xaa]));
    if (!crossing) host(c, 'protect', [0x9000, 1, name === 'none' ? 0 : name === 'execute-only' ? 4 : access === 1 ? 2 : 1]);
    if (name === 'cross-denied') host(c, 'protect', [0xa000, 1, access === 1 ? 2 : 1]);
  }
  return {name, address, original: name === 'unmapped' ? 0 : name === 'cross-missing' ? 0x000f : 0x800f,
    detail: name === 'overflow' ? 3 : name === 'unmapped' || name === 'cross-missing' ? 1 : 2, faultAddress: crossing ? 0xa000 : address};
}
function repair(c, f) {
  if (f.name === 'unmapped') map(c, 0x9000);
  else if (f.name === 'cross-missing') map(c, 0xa000);
  else host(c, 'protect', [f.name === 'cross-denied' ? 0xa000 : 0x9000, 1, 3]);
}
function overflowCanaries(c) {
  checkRam(c, 0xfffffffe, Buffer.from([0xff, 0xd0])); checkRam(c, 0, Buffer.from([0x5a, 0xaa]));
}
function stopped(c, unit, f, access) {
  const r = registers(); r[6] = access === 1 ? f.address : DATA + 32;
  r[4] = access === 1 ? 0xb002 : wrap(BigInt(f.address) + 2n);
  upload(c, DATA + 32, word(0x2000));
  const before = seed(c, faultPC - 5, r), held = {...before, registers: [...before.registers], pc: faultPC};
  held.registers[3] = 0x11223344;
  const events = [{access: 1, address: r[6]}];
  if (access === 2) events.push({access: 2, address: f.address, value: low(faultPC + 3)});
  for (const [budget, retired] of [[2, 1], [1, 0]]) {
    run(c, unit, budget, held, {label: `${f.name} ${access === 1 ? 'read precedes bad stack' : 'successful read precedes failed push'}`,
      reason: 5, retired, events, value: access === 2 ? 0x2000 : 0, fault: {detail: f.detail, address: f.faultAddress, access}});
    if (f.name === 'overflow') overflowCanaries(c);
    if (access === 2) checkRam(c, DATA + 32, word(0x2000));
    if (access === 2 && f.name.startsWith('cross-')) checkRam(c, 0x9fff, Buffer.from([0x0f]));
    counts.faults++;
  }
}

for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries), audit = compile(c, auditBlocks);
  for (const row of highRows) for (const flag of [2, 0x443, 0x892, 0xcd7]) {
    const r = registers(); r[0] = 0xabcd2000; seed(c, row.pc, r, flag);
    const result = call(c, audit, row, 0x2000, undefined, 'high/top completed CALL truncates return16 and defers target Execute', 2);
    assert.equal(result.value, 0); counts.relative++;
  }
  for (const distance of [2, 1]) {
    const r = registers(); r[4] = HIGH + 0x42; r[6] = r[4] - distance;
    const start = HIGH + 0x3f, framed = Buffer.from([0x55, 0, 0x30, 0x30, 0xaa]);
    framed.set(word(0x3000), r[6] - start); upload(c, start, framed); seed(c, faultPC, r, 0x443);
    const result = call(c, audit, faultRow, 0x3000, r[6], 'original target survives exact/partial overlap with return push');
    checkRam(c, start, mutatedFrame(start, framed, result.stack, result.value)); counts.overlap++;
  }
  for (const [address, target] of [[0x8000, 0xa555], [rxPC, 0xff66]]) {
    const r = registers(); r[6] = address; seed(c, rxPC, r, 0x892);
    for (let i = 0; i < 2; i++) {
      if (i) input(c, 0, state({...cpu(c), pc: rxPC}));
      call(c, audit, rxRow, target, address, 'RO/RX target read with original current stack and retained owner'); counts.code_reads++;
    }
  }
  run(c, audit, 2, cpu(c), {label: 'cold committed target does not reread target or push again', reason: 3, retired: 0}); counts.cold++;
  for (const name of failures) {
    const f = failureSetup(c, name, 1); stopped(c, audit, f, 1);
    if (name === 'overflow') continue;
    repair(c, f); checkRam(c, f.address, word(f.original));
    run(c, audit, 1, cpu(c), {label: 'read map/protect-only repair exposes still-unmapped stack', reason: 5, retired: 0,
      events: [{access: 1, address: f.address}, {access: 2, address: 0xb000, value: 0x1708}],
      fault: {detail: 1, address: 0xb000, access: 2}}); counts.deferred_store_faults++;
    map(c, 0xb000); call(c, audit, faultRow, f.original, f.address, 'stack-map-only repair commits original stopped CPU/current target');
    checkRam(c, f.address, word(f.original)); counts.repairs++;
  }
  for (const name of failures) {
    const f = failureSetup(c, name, 2); stopped(c, audit, f, 2);
    if (name === 'overflow') continue;
    repair(c, f); checkRam(c, f.address, word(f.original));
    call(c, audit, faultRow, 0x2000, DATA + 32, 'write map/protect-only repair commits current target and unchanged CPU'); counts.repairs++;
  }
  {
    const f = failureSetup(c, 'cross-denied', 2); stopped(c, audit, f, 2); repair(c, f);
    upload(c, HIGH + 0x20, Buffer.from([0x21, 0x43, 0xa5, 0xf3])); upload(c, HIGH + 0x40, Buffer.from([0x55, 0, 0, 0xaa]));
    const changed = cpu(c); changed.registers[6] = HIGH + 0x20; changed.registers[4] = HIGH + 0x43;
    changed.registers[0] = 0x55667788; changed.flags = 0x892; input(c, 0, state(changed));
    call(c, audit, faultRow, 0x4321, HIGH + 0x20, 'separate explicit current WORD/EA/ESP/FLAGS after failed push');
    checkRam(c, f.address, word(0x800f)); checkRam(c, HIGH + 0x40, Buffer.from([0x55, 8, 0x17, 0xaa])); counts.current_inputs++;
  }
  for (const [budget, cancel, reason] of [[0, 0, 1], [0, 1, 2], [1, 1, 2]]) {
    const r = registers(); r[6] = 0xffffffff; r[4] = 1; const before = seed(c, faultPC, r); input(c, 96, words([cancel]));
    run(c, audit, budget, before, {label: 'budget/cancel precede both invalid target read and invalid stack store', reason, retired: 0}); counts.controls++;
  }
  input(c, 96, words([0]));
  {
    upload(c, DATA + 32, Buffer.from([0, 0x19, 0xa5, 0xf3])); const r = registers();
    r[0] = 0xa1007fff; r[2] = 0xa1020000; r[6] = DATA + 32; seed(c, liveCaller.pc, r, 0x443);
    const next = cpu(c); next.registers[0] = 0xa1008001; next.registers[2] = 0xa1020001; next.registers[3] = 0xa1030001;
    next.pc = 0x180e; next.flags = 0xc92;
    run(c, audit, 10, next, {label: 'live memory CALL→callee→WORD RET→ADC/SETO/SETB without CPU reseed', reason: 3, retired: 7,
      events: [{access: 1, address: DATA + 32}, {access: 2, address: HIGH + 0x7e, value: 0x1803}, {access: 1, address: HIGH + 0x7e}], value: 0x1803}); counts.consumers++;
    upload(c, DATA + 32, word(0x3000)); run(c, audit, 2, cpu(c), {label: 'live cold continuation cannot reread changed target or replay CALL', reason: 3, retired: 0}); counts.consumers++;
    checkRam(c, HIGH + 0x7e, word(0x1803)); checkRam(c, DATA + 32, Buffer.from([0, 0x30, 0xa5, 0xf3]));
  }
  for (const address of [0x9000, 0xa000, 0xb000]) unmap(c, address);

  // finish the top-source audit before later top-page data writes; its owner is not reused afterwards.
  const regUnit = compile(c, regRows);
  for (const row of regRows) {
    const inputs = row.source === 4 ? [DATA + 8, DATA + 9, HIGH + 8, HIGH + 9] : [0, 1, 0x8000, 0xffff, 0x2000];
    for (const target of inputs) for (const flag of [2, 0xcd7]) {
      const r = registers(); r[row.source] = row.source === 4 ? target : (0xf100 + row.source) * 65536 + target; seed(c, row.pc, r, flag);
      call(c, regUnit, row, low(r[row.source]), undefined, 'all8 register targets/high decoys captured before fullESP decrement', 2); counts.register_inputs++;
    }
  }
  for (const flag of flags) {
    const r = registers(); r[0] = 0xabcd8001; seed(c, regRows[0].pc, r, flag);
    call(c, regUnit, regRows[0], 0x8001, undefined, 'register CALL all128 valid FLAGS'); counts.flag_inputs++;
  }
  {
    const r = registers(); r[4] = HIGH + 8; seed(c, regRows[4].pc, r);
    for (const target of [0x5008, 0x5006]) {
      input(c, 0, state({...cpu(c), pc: regRows[4].pc}));
      call(c, regUnit, regRows[4], target, undefined, 'CALL SP PC-only repeat consumes old current fullESP'); counts.repeats++;
    }
  }
  {
    const r = registers(); r[4] = 1; const before = seed(c, regRows[4].pc, r);
    for (let i = 0; i < 2; i++) {
      run(c, regUnit, 1, before, {label: 'CALL SP1 checks nonwrapping WORD push before target publication', reason: 5, retired: 0,
        events: [{access: 2, address: 0xffffffff, value: 0x1083}], fault: {detail: 3, address: 0xffffffff, access: 2}});
      overflowCanaries(c); counts.faults++;
    }
    r[4] = 0; seed(c, regRows[4].pc, r);
    call(c, regUnit, regRows[4], 0, undefined, 'CALL SP0 wraps fullESP to FFFFFFFE and saves exactly two bytes', 2); counts.boundaries++;
  }
  for (const [budget, cancel, reason] of [[0, 0, 1], [0, 1, 2], [1, 1, 2]]) {
    const before = seed(c, regRows[0].pc); input(c, 96, words([cancel]));
    run(c, regUnit, budget, before, {label: 'register budget/cancel neutral before push', reason, retired: 0}); counts.controls++;
  }
  input(c, 96, words([0])); seed(c, 0x3000);
  run(c, regUnit, 2, cpu(c), {label: 'cold absent PC does not push or fetch target', reason: 3, retired: 0}); counts.cold++;

  let last;
  for (const rows of [eaRows.slice(0, 8), eaRows.slice(8)]) {
    const unit = compile(c, rows); last = unit;
    for (const row of rows) for (const target of [0, 0x8001, 0xffff]) for (const flag of [2, 0xcd7]) {
      const address = addressOf(row, row.r), framed = Buffer.from([0x55, ...word(target), 0xa5, 0xf3]);
      upload(c, address - 1, framed); seed(c, row.pc, row.r, flag);
      const result = call(c, unit, row, target, address, 'full32 original EA/ESP and exact two-byte target before push', 2);
      checkRam(c, address - 1, mutatedFrame(address - 1, framed, result.stack, result.value)); counts.memory_inputs++;
    }
    if (rows[0] === eaRows[0]) {
      for (const flag of flags) {
        upload(c, HIGH + 8, word(0x8001)); seed(c, eaRows[0].pc, eaRows[0].r, flag);
        call(c, unit, eaRows[0], 0x8001, HIGH + 8, 'memory CALL all128 valid FLAGS'); counts.flag_inputs++;
      }
      for (const [address, target] of [[0x6ffe, 0x8001], [0xfffffffe, 0xffff], [DATA + 0xfff, 0x2000], [DATA + 9, 1]]) {
        upload(c, address, word(target)); const r = registers(); r[6] = address; seed(c, eaRows[6].pc, r, 0x443);
        call(c, unit, eaRows[6], target, address, 'WORD target width/page/top/unaligned span, not read32', 2); checkRam(c, address, word(target)); counts.boundaries++;
      }
      const r = registers(); r[6] = DATA + 8; seed(c, eaRows[6].pc, r);
      for (const target of [0x1234, 0x8001]) {
        upload(c, DATA + 8, word(target)); input(c, 0, state({...cpu(c), pc: eaRows[6].pc}));
        call(c, unit, eaRows[6], target, DATA + 8, 'PC-only repeat rereads target/current stack'); counts.repeats++;
      }
    }
  }
  const r = registers(); r[0] = 0xabcd2000; seed(c, regRows[0].pc, r);
  if (owner === 'resident') call(c, regUnit, regRows[0], 0x2000, undefined, 'earlier register resident retains identity/current source');
  else run(c, regUnit, 1, cpu(c), {label: 'earlier replacement generation3 is neutral', status: 3, retired: 0}); counts.retained++;
  seed(c, eaRows[8].pc, eaRows[8].r); upload(c, eaRows[8].pc, eaRows[8].bytes.subarray(0, 1));
  run(c, last, 2, cpu(c), {label: 'same-byte consumed RWX source invalidation4 is neutral', status: 4, retired: 0}); counts.currentness++;
  checkRam(c, 0x8000, canary); checkRam(c, rxPC, indirect); close(c, 4);
}

for (const owner of ['replacement', 'resident']) for (const entries of [false, true])
  for (const kind of ['memory-changed', 'memory-identity', 'register-SP-identity']) {
    const c = open(owner, entries, kind), pc = kind === 'memory-changed' ? 0xc000 : 0xff63;
    const isMemory = kind !== 'register-SP-identity', bytes = Buffer.from([0x66, 0xff, isMemory ? 0x16 : 0xd4]);
    const row = {pc, bytes, instructions: 1, read: isMemory, store: true};
    map(c, pc - pc % 4096, 7); if (kind === 'memory-changed') map(c, 0xf000, 7);
    upload(c, pc, bytes); if (isMemory) upload(c, 0xff66, Buffer.from([0xeb, 0]));
    const unit = compile(c, [row]), r = registers(); r[4] = pc + 2; r[6] = pc;
    seed(c, pc, r, 0x892); const target = isMemory ? 0xff66 : 0xff65, result = effect(cpu(c), row, target);
    assert.equal(result.value, kind === 'memory-changed' ? 0xc003 : 0xff66); assert.equal(result.stack, pc);
    const events = isMemory ? [{access: 1, address: pc}] : []; events.push({access: 2, address: pc, value: result.value});
    run(c, unit, 10, result.next, {label: kind + ' commits original target/fullESP/return once before SMC6', reason: 6, events}); counts.self_code++;
    checkRam(c, pc, Buffer.from([...word(result.value), bytes[2]]));
    run(c, unit, 10, cpu(c), {label: kind + ' consumed owner4 neutral before fresh capture', status: 4, retired: 0}); counts.self_code++;
    // the SP target is the old instruction tail; replace it only after the original owner was rejected.
    if (!isMemory) upload(c, target, Buffer.from([0xeb, 0]));
    const suffix = {pc: target, bytes: Buffer.from([0xeb, 0]), instructions: 1}, fresh = compile(c, [suffix]);
    run(c, fresh, 2, {...cpu(c), pc: target + 2}, {label: kind + ' fresh pure suffix does not replay CALL', reason: 3}); counts.self_code++;
    close(c, 2);
  }

assert.deepEqual(sources(), sourcePins, 'selected source pins stayed fixed during recording');
assert.deepEqual(counts, {contexts: 16, runs: 2028, retired: 1906, register_inputs: 312, memory_inputs: 312, flag_inputs: 1024,
  boundaries: 20, code_reads: 16, faults: 128, repairs: 48, current_inputs: 4, repeats: 16, controls: 24,
  cold: 8, consumers: 8, retained: 4, currentness: 4, overlap: 8, relative: 32, self_code: 36,
  deferred_store_faults: 24, helper_calls: 2990 - 4 * 2 * 3, read_calls: 1084 - 4 * 2, store_calls: 1906 - 4 * 2 * 2}, 'finite source ledger and observed canonical chronology');
assert.equal(modules.length, 40);
writeFileSync(join(output, 'result.json'), JSON.stringify({status: 'ok', engine_sha256: engine.sha256, source_pins: sourcePins,
  counts, contexts, modules, observations, limits: [
    'exact single66 FF/2 register/read16 WORD CALL in four bound profiles; standalone/far/prefix aliases remain outside',
    'independent arithmetic low16 return/target and BigInt full32 EA/ESP oracle, all8 parents/thirteen EAs, original SP and exact/partial source-push overlap',
    'canonical observed read16V2/store16V4 chronology and resident binding; target captured before push, no target Execute fetch',
    'all128 valid FLAGS per register/memory family, whole4364/dirtyFP128/GPR/helper assertions and selected RAM/canaries only',
    'seven read faults with simultaneously bad stack precede all writes; read repair exposes deferred stack fault; seven write faults follow successful read',
    'prior retirement/unchanged retries/map-protect-only honest repair, separate explicit current WORD/EA/ESP/FLAGS; no arbitrary-host rollback claim',
    'completed high/top CALL return16zero, SP0 full32 wrap versus SP1 nonwrapping span fault, two-byte target boundaries and overflow canaries',
    'live memory CALL/callee/WORD RET/ADC/SETO/SETB without CPU reseed, changed pointer on cold continuation cannot replay CALL',
    'changed own-memory capture ff66/storec003 and identity ff66 writes retire before SMC6; CALL SP oldff65→targetff65/newSPff63; stale4 then fresh pure suffix',
    'generation3 rejection differs from source4; expected/observed status/reason/retired/PC/FLAGS are separate, rejected observations null',
    'four core contexts/four captures and twelve isolated SMC contexts/two captures, caps8blocks/64instructions/65536bytes/6captures before every compile; maps/protections/code precede captures except intentional consumed-code suffix setup and data inputs',
    'finite self-authored flat32 evidence; no segmented/hardware completeness/performance/PE/SDK/browser/game/hostedCI claim',
  ]}, null, 2), {flag: 'wx'});
console.log(JSON.stringify({status: 'ok', ...counts, modules: modules.length, output}));
