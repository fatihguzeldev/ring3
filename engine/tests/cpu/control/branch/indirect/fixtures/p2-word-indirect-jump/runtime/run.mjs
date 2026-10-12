import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../../../../../../../support/wasm/engine.mjs';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root);
const engine = readEngine(enginePath), SIZE = 4612, PC = 0x1000, DATA = 0x5000, HIGH = 0xa53c5000;
const initial = readFileSync(join(output, 'initial-arena.bin'));
assert.equal(initial.length, SIZE); assert.deepEqual(WebAssembly.Module.imports(engine.module), []);
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const paths = [
  'engine/src/abi/arena.rs', 'engine/src/abi/memory_helper.rs',
  'engine/src/cpu/x86/ir.rs', 'engine/src/cpu/x86/decode/decoder.rs', 'engine/src/cpu/x86/decode/lower.rs',
  'engine/src/cpu/x86/decode/flow.rs', 'engine/src/cpu/x86/decode/profile.rs', 'engine/src/cpu/dbt/region.rs',
  'engine/src/cpu/dbt/wasm/integer/arithmetic.rs', 'engine/src/cpu/dbt/wasm/integer/bits.rs', 'engine/src/cpu/dbt/wasm/integer/carry_rotates.rs', 'engine/src/cpu/dbt/wasm/integer/decimal.rs', 'engine/src/cpu/dbt/wasm/integer/dispatch.rs', 'engine/src/cpu/dbt/wasm/integer/flags.rs', 'engine/src/cpu/dbt/wasm/integer/mod.rs', 'engine/src/cpu/dbt/wasm/integer/multiply_divide.rs', 'engine/src/cpu/dbt/wasm/integer/operands.rs', 'engine/src/cpu/dbt/wasm/integer/rotates.rs', 'engine/src/cpu/dbt/wasm/integer/shifts.rs', 'engine/src/cpu/dbt/wasm/memory.rs', 'engine/src/cpu/dbt/wasm/memory/narrow.rs',
  'engine/src/cpu/dbt/wasm/abi.rs', 'engine/src/cpu/dbt/wasm/emitter.rs', 'engine/src/cpu/dbt/wasm/locals.rs',
  'engine/src/memory/space.rs', 'engine/src/process/instance.rs', 'engine/src/process/execution/compiled.rs', 'engine/src/process/execution/authority.rs', 'engine/src/process/memory/address_space.rs', 'engine/src/process/memory/scalar.rs', 'engine/src/process/memory/scalar/tests.rs', 'engine/src/process/execution/mod.rs', 'engine/src/process/memory/mod.rs', 'engine/src/process/mod.rs', 'engine/src/process/execution/resident.rs', 'engine/src/process/wasm/mod.rs', 'engine/src/process/wasm/loading.rs', 'engine/src/process/wasm/execution.rs', 'engine/src/process/wasm/calls.rs', 'engine/src/process/wasm/memory.rs', 'engine/src/process/wasm/cpu.rs', 'engine/tests/cpu/control/branch/indirect/native/word.rs', 'engine/tests/cpu/control/native.rs', 'engine/tests/support/cpu/prefix/admission.rs', 'engine/tests/support/cpu/prefix/word.rs', 'engine/tests/cpu/control/branch/indirect/wasm/word.rs', 'engine/tests/cpu/control/wasm.rs', 'engine/tests/support/wasm.rs', 'engine/tests/cpu/control/branch/indirect/fixtures/p2-word-indirect-jump/runtime/run.mjs', 'engine/tests/support/wasm/engine.mjs',
];
assert.equal(new Set(paths).size, paths.length);
const sources = () => Object.fromEntries(paths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = sources(), modules = [], observations = [];
const counts = {contexts: 0, runs: 0, retired: 0, register_inputs: 0, memory_inputs: 0, flag_inputs: 0,
  boundaries: 0, code_reads: 0, faults: 0, repairs: 0, current_inputs: 0, repeats: 0, controls: 0,
  cold: 0, consumers: 0, retained: 0, currentness: 0, helper_calls: 0};
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
const exit = (reason, retired, f, version) => record('R3EX', 40,
  [reason, retired, f?.detail ?? 0, f?.address ?? 0, f?.access ?? 0, f ? 2 : 0], version);
const helper = (value = 0, f) => record('R3MH', 40, [f ? 1 : 0, value, f?.detail ?? 0, f?.address ?? 0, f?.access ?? 0, 2], 2);
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
function map(c, address, permission = 3) {host(c, 'map', [address, 1, permission]); c.mapped.add(address);}
function unmap(c, address) {if (c.mapped.has(address)) {host(c, 'unmap', [address, 1]); c.mapped.delete(address);}}
function upload(c, address, bytes) {input(c, 140, bytes); host(c, 'upload', [address, bytes.length]);}
function checkRam(c, address, bytes) {
  for (const [i, value] of bytes.entries()) {
    const expected = arena(c); expected.set(record('R3MH', 40, [0, value, 0, 0, 0, 1], 2), 100);
    assert.equal(c.api.read8(wrap(BigInt(address) + BigInt(i))), 0); assert.deepEqual(arena(c), expected, 'selected RAM/canary');
  }
}
const cpu = c => {const b = arena(c); return {registers: Array.from({length: 8}, (_, i) => b.readUInt32LE(16 + i * 4)), pc: b.readUInt32LE(48), flags: b.readUInt32LE(52)};};
const registers = () => Array.from({length: 8}, (_, i) => (0xa100 + i) * 65536 + 0x1234 + i);
function seed(c, pc, r = registers(), flag = 0xcd7) {
  const b = Buffer.alloc(140); b.set(state({registers: r, pc, flags: flag})); b.set(exit(3, 0, undefined, 2), 56);
  b.set(record('R3MH', 40, [0, 0xdecafbad, 0, 0, 0, 0]), 100); input(c, 0, b); return cpu(c);
}
function run(c, unit, budget, next, {label, reason = 1, retired = 1, fault, events = [], value = 0, status = 0} = {}) {
  if (fault?.detail === 3) {reason = fault.access === 2 || label.startsWith('RET') ? 15 : 11; fault = undefined; events = [];}
  const expected = arena(c);
  if (status === 0) {
    expected.set(state(next)); expected.set(exit(reason, retired, fault, reason === 11 || reason === 15 ? 6 : unit.exitVersion), 56);
    if (events.length) expected.set(helper(fault ? 0 : value, fault), 100);
  }
  c.trace = []; const observedStatus = unit.run(c.base, c.base + 56, budget, c.base + 96);
  assert.equal(observedStatus, status, label); assert.deepEqual(c.trace, events, 'canonical read16 trace: ' + label);
  assert.deepEqual(arena(c), expected, 'full4612 GPR/FLAGS/helper/dirtyFP effect: ' + label);
  const after = arena(c);
  observations.push({context: c.id, label, expected_status: status, observed_status: observedStatus,
    expected_reason: status === 0 ? reason : null, observed_reason: status === 0 ? after.readUInt32LE(72) : null,
    expected_retired: status === 0 ? retired : null, observed_retired: status === 0 ? after.readUInt32LE(76) : null,
    expected_pc: status === 0 ? next.pc : null, observed_pc: status === 0 ? after.readUInt32LE(48) : null,
    expected_flags: status === 0 ? next.flags : null, observed_flags: status === 0 ? after.readUInt32LE(52) : null, events: c.trace});
  counts.runs++; if (status === 0) counts.retired += after.readUInt32LE(76); counts.helper_calls += c.trace.length;
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
  const reads = blocks.some(b => b.read), names = [c.owner === 'resident' ? 'guard_resident' : 'guard', ...(reads ? ['read16'] : [])];
  assert.deepEqual(WebAssembly.Module.imports(module), [{module: 'env', name: 'memory', kind: 'memory'}, ...names.map(name => ({module: 'ring3', name, kind: 'function'}))]);
  assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  const imports = {...c.api, read16: address => {c.trace.push({access: 1, address: address >>> 0}); return c.api.read16(address);}};
  const file = `module-${modules.length + 1}.wasm`; writeFileSync(join(output, file), bytes, {flag: 'wx'});
  modules.push({context: c.id, owner: c.owner, entries: c.entries, file, sha256: hash(bytes), instructions, blocks: blocks.length, bytes: bytesTotal, imports: names});
  return {...binding, exitVersion: reads ? 2 : 1, run: new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3: imports}).exports.run};
}

const regRows = Array.from({length: 8}, (_, source) => ({pc: PC + source * 0x20, source,
  instructions: 1, bytes: Buffer.from([0x66, 0xff, 0xe0 + source])}));
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
  row.pc = PC + 0x200 + i * 0x20; row.instructions = 1; row.read = true;
  row.bytes = Buffer.from([0x66, 0xff, row.tail[0] + 0x20, ...row.tail.slice(1)]);
}
const addressOf = (row, r) => wrap(BigInt(row.base === undefined ? 0 : r[row.base]) +
  BigInt(row.index === undefined ? 0 : r[row.index]) * BigInt(row.scale) + BigInt(row.displacement));
for (const row of eaRows) assert.equal(addressOf(row, row.r), 0xa53c5008, 'literal full32 scaled/wrapped/old-parent EA');
for (const [parent, target] of [[0xa53c5008, 0x5008], [0xffff8001, 0x8001], [0xffffffff, 0xffff]]) assert.equal(low(parent), target);
const mov = Buffer.from([0xbb, 0x44, 0x33, 0x22, 0x11]), indirect = Buffer.from([0x66, 0xff, 0x26]);
const faultPC = PC + 0x705, rxPC = 0xc000;
const auditBlocks = [
  {pc: faultPC - 5, bytes: Buffer.concat([mov, indirect]), instructions: 2, read: true},
  {pc: rxPC, bytes: indirect, instructions: 1, read: true},
  {pc: 0x1800, bytes: Buffer.from([0x66, 0xff, 0xe1]), instructions: 1},
  {pc: 0x1900, bytes: indirect, instructions: 1, read: true},
  {pc: 0x1a00, bytes: Buffer.from([0x66, 0x11, 0xd0, 0x0f, 0x90, 0xc3, 0x0f, 0x92, 0xc7, 0xeb, 0]), instructions: 4},
];
const canary = Buffer.from([0x55, 0xa5, 0x66, 0x67, 0xf0, 0xf2, 0xf3, 0xaa]);
function open(owner, entries) {
  const instance = new WebAssembly.Instance(engine.module, {}), c = {id: ++counts.contexts, owner, entries, captures: 0,
    memory: instance.exports.memory, api: {}, mapped: new Set(), trace: []};
  for (const name of ['open', 'close', 'arena_ptr', 'map', 'unmap', 'protect', 'upload', 'read8', 'read16',
    'compile', 'compile_entries', 'compile_resident', 'compile_resident_entries', 'generation', 'module_ptr', 'module_len', 'guard', 'guard_resident'])
    c.api[name] = instance.exports['ring3_abi_v1_' + name];
  assert.equal(c.api.open(12, c.id, 0x57494a50), 0); c.base = c.api.arena_ptr() >>> 0; input(c, 0, initial);
  for (const address of [PC, DATA, DATA + 4096, HIGH, 0xfffff000, 0, 0x8000, rxPC]) map(c, address, address === PC || address === rxPC ? 7 : 3);
  for (const block of [...regRows, ...eaRows, ...auditBlocks]) upload(c, block.pc, block.bytes);
  upload(c, 0x8000, canary); host(c, 'protect', [0x8000, 1, 1]); host(c, 'protect', [rxPC, 1, 5]);
  return c;
}
function jumpRegister(c, unit, row, label, budget = 1) {
  const before = cpu(c); run(c, unit, budget, {...before, pc: low(before.registers[row.source])}, {label, reason: budget === 1 ? 1 : 3});
}
function jumpMemory(c, unit, address, value, label, budget = 1) {
  const before = cpu(c); run(c, unit, budget, {...before, pc: value}, {label, reason: budget === 1 ? 1 : 3,
    events: [{access: 1, address}], value});
}
const failures = ['unmapped', 'none', 'wrong-permission', 'execute-only', 'cross-missing', 'cross-denied', 'overflow'];
function failureSetup(c, name) {
  unmap(c, 0x9000); unmap(c, 0xa000);
  const crossing = name.startsWith('cross-'), address = name === 'overflow' ? 0xffffffff : crossing ? 0x9fff : 0x9008;
  if (name === 'overflow') {
    upload(c, 0xfffffffe, Buffer.from([0x55, 0xa5])); upload(c, 0, Buffer.from([0x5a, 0xaa]));
  } else if (name !== 'unmapped') {
    map(c, 0x9000); if (name === 'cross-denied') map(c, 0xa000);
    if (crossing) {upload(c, 0x9ffe, Buffer.from([0x55, 0x0f])); if (name === 'cross-denied') upload(c, 0xa000, Buffer.from([0x80, 0xaa]));}
    else upload(c, address - 1, Buffer.from([0x55, 0x0f, 0x80, 0xaa]));
    if (!crossing) host(c, 'protect', [0x9000, 1, name === 'none' ? 0 : name === 'execute-only' ? 4 : 2]);
    if (name === 'cross-denied') host(c, 'protect', [0xa000, 1, 2]);
  }
  return {name, address, original: name === 'unmapped' ? 0 : name === 'cross-missing' ? 0x000f : 0x800f,
    detail: name === 'overflow' ? 3 : name === 'unmapped' || name === 'cross-missing' ? 1 : 2, faultAddress: crossing ? 0xa000 : address};
}
function repair(c, f) {
  if (f.name === 'unmapped') map(c, 0x9000);
  else if (f.name === 'cross-missing') map(c, 0xa000);
  else host(c, 'protect', [f.name === 'cross-denied' ? 0xa000 : 0x9000, 1, 3]);
}
function stopped(c, unit, f) {
  const r = registers(); r[6] = f.address;
  const before = seed(c, faultPC - 5, r), held = {...before, registers: [...before.registers], pc: faultPC};
  held.registers[3] = 0x11223344;
  for (const [budget, retired] of [[2, 1], [1, 0]]) {
    run(c, unit, budget, held, {label: f.name + ' read fault with prior MOV/unchanged retry', reason: 5, retired,
      fault: {detail: f.detail, address: f.faultAddress, access: 1}, events: [{access: 1, address: f.address}]});
    if (f.name === 'overflow') {checkRam(c, 0xfffffffe, Buffer.from([0x55, 0xa5])); checkRam(c, 0, Buffer.from([0x5a, 0xaa]));}
    counts.faults++;
  }
}

for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries), regUnit = compile(c, regRows);
  for (const row of regRows) for (const target of [0, 1, 0x8000, 0xffff, 0x2000]) for (const flag of [2, 0xcd7]) {
    const r = registers(); r[row.source] = (0xf100 + row.source) * 65536 + target; seed(c, row.pc, r, flag);
    jumpRegister(c, regUnit, row, 'all8 current low16 targets/dirty high parents, no target fetch', 2); counts.register_inputs++;
  }
  for (const flag of flags) {
    const r = registers(); r[4] = 0xa53c8001; seed(c, regRows[4].pc, r, flag);
    jumpRegister(c, regUnit, regRows[4], 'register ESP all128 valid FLAGS/full parent retained'); counts.flag_inputs++;
  }
  {
    const row = regRows[4], r = registers(); r[4] = HIGH + 8; seed(c, row.pc, r);
    jumpRegister(c, regUnit, row, 'register PC-only initial repeat'); counts.repeats++;
    const changed = cpu(c); changed.pc = row.pc; changed.registers[4] = 0x76548001; changed.flags = 0x892; input(c, 0, state(changed));
    jumpRegister(c, regUnit, row, 'explicit current register parent/FLAGS on repeat'); counts.repeats++;
  }
  for (const [budget, cancel, reason] of [[0, 0, 1], [0, 1, 2], [1, 1, 2]]) {
    const before = seed(c, regRows[0].pc); input(c, 96, words([cancel]));
    run(c, regUnit, budget, before, {label: 'register neutral budget/cancel', reason, retired: 0}); counts.controls++;
  }
  input(c, 96, words([0])); seed(c, 0x3000);
  run(c, regUnit, 2, cpu(c), {label: 'cold absent PC does not fetch target', reason: 3, retired: 0}); counts.cold++;

  for (const rows of [eaRows.slice(0, 8), eaRows.slice(8)]) {
    const unit = compile(c, rows);
    for (const row of rows) for (const target of [0, 0x8001, 0xffff]) for (const flag of [2, 0xcd7]) {
      const address = addressOf(row, row.r), framed = Buffer.from([0x55, ...word(target), 0xa5, 0xf3]);
      upload(c, address - 1, framed); seed(c, row.pc, row.r, flag);
      jumpMemory(c, unit, address, target, 'full32 old-parent/scaled/wrapped EA and exact two-byte target', 2);
      checkRam(c, address - 1, framed); counts.memory_inputs++;
    }
    if (rows[0] === eaRows[0]) for (const flag of flags) {
      upload(c, HIGH + 8, word(0x8001)); seed(c, eaRows[0].pc, eaRows[0].r, flag);
      jumpMemory(c, unit, HIGH + 8, 0x8001, 'memory all128 valid FLAGS'); counts.flag_inputs++;
    }
  }

  const unit = compile(c, auditBlocks);
  for (const [address, target] of [[0x6ffe, 0x8001], [0xfffffffe, 0xffff], [DATA + 0xfff, 0x2000], [DATA + 9, 1]]) {
    upload(c, address, word(target)); const r = registers(); r[6] = address; r[4] = HIGH + 8; seed(c, faultPC, r, 0x443);
    jumpMemory(c, unit, address, target, 'WORD width/page/top/unaligned boundary preserves fullESP', 2); checkRam(c, address, word(target)); counts.boundaries++;
  }
  for (const [address, target] of [[0x8000, 0xa555], [rxPC, 0xff66]]) {
    const r = registers(); r[6] = address; seed(c, rxPC, r, 0x892);
    for (let i = 0; i < 2; i++) {
      if (i) input(c, 0, state({...cpu(c), pc: rxPC}));
      jumpMemory(c, unit, address, target, 'RO/RX read and PC-only repeat keep same owner current'); counts.code_reads++;
    }
  }
  run(c, unit, 2, cpu(c), {label: 'cold after committed RX target does not reread operand', reason: 3, retired: 0}); counts.cold++;
  for (const name of failures) {
    const f = failureSetup(c, name); stopped(c, unit, f);
    if (name !== 'overflow') {
      repair(c, f); checkRam(c, f.address, word(f.original));
      jumpMemory(c, unit, f.address, f.original, 'stopped map/protect-only repair uses current WORD'); counts.repairs++;
    }
  }
  {
    const f = failureSetup(c, 'cross-denied'); stopped(c, unit, f); repair(c, f);
    upload(c, HIGH + 0x20, Buffer.from([0x21, 0x43, 0xa5, 0xf3])); const changed = cpu(c);
    changed.registers[6] = HIGH + 0x20; changed.registers[4] = 0x76548001; changed.registers[0] = 0x55667788; changed.flags = 0x892;
    input(c, 0, state(changed)); jumpMemory(c, unit, HIGH + 0x20, 0x4321, 'separate explicit current WORD/EA/fullESP/FLAGS retry');
    checkRam(c, f.address, word(0x800f)); checkRam(c, HIGH + 0x20, Buffer.from([0x21, 0x43, 0xa5, 0xf3])); counts.current_inputs++;
  }
  for (const [budget, cancel, reason] of [[0, 0, 1], [0, 1, 2], [1, 1, 2]]) {
    const r = registers(); r[6] = 0xffffffff; const before = seed(c, faultPC, r); input(c, 96, words([cancel]));
    run(c, unit, budget, before, {label: 'memory controls precede read/fault', reason, retired: 0}); counts.controls++;
  }
  input(c, 96, words([0]));
  {
    const r = registers(); r[6] = DATA + 8; seed(c, faultPC, r);
    for (const target of [0x1234, 0x8001]) {
      upload(c, DATA + 8, word(target)); input(c, 0, state({...cpu(c), pc: faultPC}));
      jumpMemory(c, unit, DATA + 8, target, 'PC-only repeat reads newly supplied WORD'); counts.repeats++;
    }
  }
  {
    upload(c, DATA + 32, Buffer.from([0, 0x1a, 0xa5, 0xf3])); const r = registers();
    r[0] = 0xa1007fff; r[1] = 0xcafe1900; r[2] = 0xa1020000; r[6] = DATA + 32; seed(c, 0x1800, r, 0x443);
    run(c, unit, 1, {...cpu(c), pc: 0x1900}, {label: 'live register returns zero-extended memory-stage PC'}); counts.consumers++;
    jumpMemory(c, unit, DATA + 32, 0x1a00, 'live memory returns consumer PC preserving incoming carry'); counts.consumers++;
    upload(c, DATA + 32, word(0x3000)); const next = cpu(c); next.registers[0] = 0xa1008000; next.registers[3] = 0xa1030001;
    next.flags = 0xc96; next.pc = 0x1a0b;
    run(c, unit, 5, next, {label: 'live returned PC consumes carry via ADC/SETO/SETB without CPU reseed', reason: 3, retired: 4}); counts.consumers++;
    run(c, unit, 2, cpu(c), {label: 'live cold continuation does not replay changed pointer', reason: 3, retired: 0}); counts.consumers++;
    checkRam(c, DATA + 32, Buffer.from([0, 0x30, 0xa5, 0xf3]));
  }
  const r = registers(); r[0] = 0xabcd2000; seed(c, regRows[0].pc, r);
  if (owner === 'resident') jumpRegister(c, regUnit, regRows[0], 'oldest retained resident remains current');
  else run(c, regUnit, 1, cpu(c), {label: 'old replacement generation3 is neutral', status: 3, retired: 0});
  counts.retained++;
  seed(c, faultPC - 5); upload(c, faultPC - 5, mov.subarray(0, 1));
  run(c, unit, 2, cpu(c), {label: 'same-byte consumed RWX source invalidation4 is neutral', status: 4, retired: 0}); counts.currentness++;
  assert.equal(c.captures, 4); checkRam(c, 0x8000, canary); checkRam(c, rxPC, indirect); host(c, 'close', []);
}

assert.deepEqual(sources(), sourcePins, 'selected source pins stayed fixed during recording');
assert.deepEqual(counts, {contexts: 4, runs: 1852, retired: 1790, register_inputs: 320, memory_inputs: 312, flag_inputs: 1024,
  boundaries: 16, code_reads: 16, faults: 64, repairs: 24, current_inputs: 4, repeats: 16, controls: 24,
  cold: 8, consumers: 16, retained: 4, currentness: 4, helper_calls: 960 - 4 * 2}, 'finite source ledger and observed canonical helpers');
assert.equal(modules.length, 16);
writeFileSync(join(output, 'result.json'), JSON.stringify({status: 'ok', engine_sha256: engine.sha256, source_pins: sourcePins,
  counts, modules, observations, limits: [
    'exact single66 FF/4 register/read16 WORD JMP in four bound profiles; standalone, far/prefix aliases remain outside',
    'arithmetic low16 and BigInt full32 EA oracle with literal scaled/wrapped/ESP aliases; all128 valid FLAGS in register and memory forms',
    'guard-only pure ExitV1 versus canonical read16 ExitV2; observed helper forwarding, no stores/stack movement/target Execute fetch',
    '6FFE/FFFFFFFE two-byte successes, unaligned/cross-page and FFFFFFFF overflow; seven read faults preserve prior retirement and full stopped CPU',
    'map/protect-only repaired inputs retain CPU; explicit current WORD/EA/ESP/FLAGS retry and pointer-only repeats are separately labeled',
    'live register→memory→ADC/SETO/SETB and cold continuation without CPU reseed; runtime pointer edit cannot replay an already committed branch',
    'RX own-code reads retain owner, replacement generation3 differs from same-byte consumed RWX CodeInvalidated4; rejected observations are null',
    'whole4364 arena/dirtyFP128/GPR/FLAGS/helper assertions and selected RAM/canaries; four sequential capture/use phases, caps8/64/65536/6 checked before each compile',
    'finite self-authored private flat32 corpus, no segmented/hardware fault completeness/performance/PE/SDK/browser/game/hostedCI claim',
  ]}, null, 2), {flag: 'wx'});
console.log(JSON.stringify({status: 'ok', ...counts, modules: modules.length, output}));
