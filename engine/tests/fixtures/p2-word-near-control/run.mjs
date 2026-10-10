import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../support/engine.mjs';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root);
const engine = readEngine(enginePath), SIZE = 4364, PC = 0x1000, DATA = 0x5000, HIGH = 0xa53c5000;
const initial = readFileSync(join(output, 'initial-arena.bin'));
assert.equal(initial.length, SIZE);
assert.deepEqual(WebAssembly.Module.imports(engine.module), []);
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const paths = [
  'engine/src/abi/arena.rs', 'engine/src/abi/memory_helper.rs',
  'engine/src/cpu/x86/ir.rs', 'engine/src/cpu/x86/decode/decoder.rs', 'engine/src/cpu/x86/decode/lower.rs',
  'engine/src/cpu/x86/decode/flow.rs', 'engine/src/cpu/x86/decode/profile.rs', 'engine/src/cpu/dbt/region.rs',
  'engine/src/cpu/dbt/wasm/integer.rs', 'engine/src/cpu/dbt/wasm/memory.rs', 'engine/src/cpu/dbt/wasm/memory/narrow.rs',
  'engine/src/cpu/dbt/wasm/memory/word_store.rs', 'engine/src/cpu/dbt/wasm/abi.rs', 'engine/src/cpu/dbt/wasm/emitter.rs',
  'engine/src/cpu/dbt/wasm/dispatcher.rs', 'engine/src/cpu/dbt/wasm/locals.rs', 'engine/src/memory/space.rs',
  'engine/src/process/instance.rs', 'engine/src/process/resident.rs', 'engine/src/process/installation.rs', 'engine/src/process/wasm.rs',
  'engine/tests/cpu_word_near_control.rs', 'engine/tests/cpu_word_near_control_wasm.rs',
  'engine/tests/fixtures/p2-word-near-control/run.mjs', 'engine/tests/fixtures/support/engine.mjs',
];
assert.equal(new Set(paths).size, paths.length);
const sources = () => Object.fromEntries(paths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = sources(), modules = [], observations = [];
const counts = {contexts: 0, runs: 0, retired: 0, flag_inputs: 0, relative: 0, boundaries: 0, adjustments: 0,
  code_reads: 0, repeats: 0, controls: 0, faults: 0, repairs: 0, current_inputs: 0, consumers: 0,
  retained: 0, self_code: 0, dispatcher: 0, helper_calls: 0};
const wrap = n => Number((BigInt(n) % 4294967296n + 4294967296n) % 4294967296n);
const low = n => Number((BigInt(n) % 65536n + 65536n) % 65536n);
const signed = n => n < 32768 ? n : n - 65536;
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
const exit = (reason, retired, f, version = 2) => record('R3EX', 40,
  [reason, retired, f?.detail ?? 0, f?.address ?? 0, f?.access ?? 0, f ? 2 : 0], version);
const helper = (access, value = 0, f) => record('R3MH', 40,
  [f ? 1 : 0, value, f?.detail ?? 0, f?.address ?? 0, f?.access ?? 0, 2], access === 1 ? 2 : 4);
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
function map(c, address, pages = 1, permission = 3) {
  host(c, 'map', [address, pages, permission]); for (let i = 0; i < pages; i++) c.mapped.add(address + i * 4096);
}
function unmap(c, address) {if (c.mapped.has(address)) {host(c, 'unmap', [address, 1]); c.mapped.delete(address);}}
function upload(c, address, bytes) {input(c, 140, bytes); host(c, 'upload', [address, bytes.length]);}
function checkRam(c, address, bytes) {
  for (const [i, value] of bytes.entries()) {
    const wanted = arena(c); wanted.set(record('R3MH', 40, [0, value, 0, 0, 0, 1], 2), 100);
    assert.equal(c.api.read8(wrap(BigInt(address) + BigInt(i))), 0); assert.deepEqual(arena(c), wanted, 'real RAM/canary');
  }
}
function ramByte(c, address) {
  const expected = arena(c); assert.equal(c.api.read8(address), 0);
  const value = arena(c).readUInt32LE(120); assert.ok(value < 256);
  expected.set(record('R3MH', 40, [0, value, 0, 0, 0, 1], 2), 100);
  assert.deepEqual(arena(c), expected, 'adjacent byte observation changes only canonical helper'); return value;
}
const cpu = c => {const b = arena(c); return {registers: Array.from({length: 8}, (_, i) => b.readUInt32LE(16 + i * 4)), pc: b.readUInt32LE(48), flags: b.readUInt32LE(52)};};
const registers = () => Array.from({length: 8}, (_, i) => (0xa100 + i) * 65536 + 0x1234 + i);
function seed(c, pc, esp, flag = 0xcd7, r = registers()) {
  r = [...r]; r[4] = esp;
  const b = Buffer.alloc(140); b.set(state({registers: r, pc, flags: flag})); b.set(exit(3, 0), 56);
  b.set(record('R3MH', 40, [0, 0xdecafbad, 0, 0, 0, 0]), 100); input(c, 0, b); return cpu(c);
}
function run(c, module, budget, next, {label, reason = 1, retired = 1, fault, events = [], helperValue = 0, status = 0, transfer} = {}) {
  const expected = arena(c);
  if (status === 0) {
    expected.set(state(next)); expected.set(exit(reason, retired, fault, module.exitVersion), 56);
    if (events.length) expected.set(helper(events.at(-1).access, fault ? 0 : helperValue, fault), 100);
    if (transfer) expected.set(transfer, 140);
  }
  c.trace = []; assert.equal(module.run(c.base, c.base + 56, budget, c.base + 96), status, label);
  assert.deepEqual(c.trace, events, 'exact canonical helper forwarding: ' + label);
  assert.deepEqual(arena(c), expected, 'full4364 CPU/FLAGS/helper/FP effect: ' + label);
  const after = arena(c);
  observations.push({context: c.id, label, status, expected_reason: status === 0 ? reason : null, expected_retired: retired,
    observed_reason: status === 0 ? after.readUInt32LE(72) : null,
    observed_retired: status === 0 ? after.readUInt32LE(76) : null, events: c.trace});
  counts.runs++; if (status === 0) counts.retired += retired; counts.helper_calls += c.trace.length;
}
function compile(c, blocks) {
  const instructions = blocks.reduce((sum, b) => sum + b.instructions, 0);
  assert.ok(blocks.length > 0 && blocks.length <= 8); assert.ok(instructions <= 64);
  assert.ok(blocks.reduce((sum, b) => sum + b.bytes.length, 0) <= 65536); assert.ok(++c.captures <= 6);
  input(c, 140, words(blocks.flatMap(b => c.entries ? [b.pc] : [b.pc, b.bytes.length])));
  const method = c.owner === 'resident' ? (c.entries ? 'compile_resident_entries' : 'compile_resident') : (c.entries ? 'compile_entries' : 'compile');
  host(c, method, c.entries ? [blocks.length, 0] : [blocks.length]);
  const b = arena(c), binding = c.owner === 'resident'
    ? {low: b.readUInt32LE(148), high: b.readUInt32LE(152), pointer: b.readUInt32LE(156), length: b.readUInt32LE(160)}
    : {generation: c.api.generation(), pointer: c.api.module_ptr() >>> 0, length: c.api.module_len() >>> 0};
  const bytes = Buffer.from(new Uint8Array(c.memory.buffer, binding.pointer, binding.length)), module = new WebAssembly.Module(bytes);
  const reads = blocks.some(b => b.read), stores = blocks.some(b => b.store);
  const names = [c.owner === 'resident' ? 'guard_resident' : 'guard', ...(reads ? ['read16'] : []), ...(stores ? [c.owner === 'resident' ? 'store_resident16' : 'store16'] : [])];
  assert.deepEqual(WebAssembly.Module.imports(module), [{module: 'env', name: 'memory', kind: 'memory'}, ...names.map(name => ({module: 'ring3', name, kind: 'function'}))]);
  assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  const imports = {...c.api};
  imports.read16 = address => {c.trace.push({access: 1, address: address >>> 0}); return c.api.read16(address);};
  imports.store16 = (address, value) => {c.trace.push({access: 2, address: address >>> 0, value: value >>> 0}); return c.api.store16(address, value);};
  imports.store_resident16 = (...args) => {
    assert.deepEqual(args.slice(0, 4).map(n => n >>> 0), [c.id, c.keyHigh, binding.low, binding.high]);
    c.trace.push({access: 2, address: args[4] >>> 0, value: args[5] >>> 0}); return c.api.store_resident16(...args);
  };
  const file = `module-${modules.length + 1}.wasm`; writeFileSync(join(output, file), bytes, {flag: 'wx'});
  modules.push({context: c.id, owner: c.owner, entries: c.entries, file, sha256: hash(bytes), instructions, blocks: blocks.length, imports: names});
  return {...binding, run: new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3: imports}).exports.run, exitVersion: reads || stores ? 2 : 1};
}
const mov = Buffer.from([0xbb, 0x44, 0x33, 0x22, 0x11]), canary = Buffer.from([0x55, 0xa5, 0x66, 0x67, 0xf0, 0xf2, 0xf3, 0xaa]);
const calls = [
  {op: 'call', pc: PC, displacement: 0x1ffc, target: 0x3000},
  {op: 'call', pc: 0x1100, displacement: 0xeefb, target: 0xffff},
  {op: 'call', pc: 0x1120, displacement: 0xf066, target: 0x018a},
  {op: 'call', pc: 0x1234fffc, displacement: 2, target: 2},
];
const callFault = {op: 'call', pc: 0x1145, displacement: 0x5eb7, target: 0x7000};
const returns = [0, 0, 0x7fff, 0x8000, 0xffff].map((adjust, i) => ({op: 'ret', pc: 0x1200 + i * 0x20, adjust, immediate: i !== 0}));
const retFault = {op: 'ret', pc: 0x12c5, adjust: 0xffff, immediate: true};
const rx = {op: 'ret', pc: 0xc000, adjust: 0, immediate: false};
const encoded = row => Buffer.from(row.op === 'call' ? [0x66, 0xe8, ...word(row.displacement)] : row.immediate ? [0x66, 0xc2, ...word(row.adjust)] : [0x66, 0xc3]);
const block = row => ({pc: row.pc, instructions: 1, read: row.op === 'ret', store: row.op === 'call', bytes: encoded(row)});
const faultBlock = row => ({...block(row), pc: row.pc - 5, instructions: 2, bytes: Buffer.concat([mov, encoded(row)])});
function transition(before, row, saved = 0) {
  const next = {...before, registers: [...before.registers]}, esp = before.registers[4];
  if (row.op === 'call') {
    next.pc = low(BigInt(before.pc) + 4n + BigInt(signed(row.displacement)));
    next.registers[4] = wrap(BigInt(esp) - 2n);
    return {next, address: next.registers[4], value: low(BigInt(before.pc) + 4n), access: 2};
  }
  next.pc = saved; next.registers[4] = wrap(BigInt(esp) + 2n + BigInt(row.adjust));
  return {next, address: esp, value: saved, access: 1};
}
function operation(c, module, row, saved, label, budget = 1) {
  const effect = transition(cpu(c), row, saved), event = {access: effect.access, address: effect.address};
  if (effect.access === 2) event.value = effect.value;
  const neighbors = effect.access === 2 ? [ramByte(c, wrap(BigInt(effect.address) - 1n)), ramByte(c, wrap(BigInt(effect.address) + 2n))] : null;
  run(c, module, budget, effect.next, {label, reason: budget === 1 ? 1 : 3, events: [event], helperValue: effect.access === 1 ? effect.value : 0});
  checkRam(c, effect.address, word(effect.value));
  if (neighbors) {
    checkRam(c, wrap(BigInt(effect.address) - 1n), Buffer.from([neighbors[0]]));
    checkRam(c, wrap(BigInt(effect.address) + 2n), Buffer.from([neighbors[1]]));
  }
  return effect;
}
const liveCall = {op: 'call', pc: 0x1400, displacement: 0xfc, target: 0x1500};
const liveBlocks = [block(liveCall),
  {pc: 0x1404, instructions: 4, bytes: Buffer.from([0x66, 0x11, 0xd0, 0x0f, 0x90, 0xc3, 0x0f, 0x92, 0xc7, 0xeb, 0])},
  {pc: 0x1500, instructions: 2, read: true, bytes: Buffer.from([0x66, 0xba, 1, 0, 0x66, 0xc3])}];
function open(owner, entries, smc) {
  const instance = new WebAssembly.Instance(engine.module, {}), c = {id: ++counts.contexts, keyHigh: 0x574e4354, owner, entries,
    memory: instance.exports.memory, api: {}, mapped: new Set(), captures: 0, trace: []};
  for (const name of ['open', 'close', 'arena_ptr', 'map', 'unmap', 'protect', 'upload', 'read8', 'read16', 'store16', 'store_resident16',
    'compile', 'compile_entries', 'compile_resident', 'compile_resident_entries', 'generation', 'module_ptr', 'module_len', 'guard', 'guard_resident',
    'acknowledge_resident_installation', 'guard_dispatch_entry', 'find_installed_resident', 'dispatcher_module']) c.api[name] = instance.exports['ring3_abi_v1_' + name];
  assert.equal(c.api.open(12, c.id, c.keyHigh), 0); c.base = c.api.arena_ptr() >>> 0; input(c, 0, initial);
  if (smc?.top) {
    map(c, 0xfffff000, 1, 7); map(c, DATA); upload(c, smc.pc, encoded(smc));
    host(c, 'protect', [0xfffff000, 1, 5]); return c;
  }
  if (smc) {
    map(c, smc.pc - smc.pc % 4096, 1, 7); map(c, PC, 1, 7);
    upload(c, smc.pc, encoded(smc)); upload(c, 0x1234, Buffer.from([0xeb, 0])); return c;
  }
  for (const args of [[PC, 1, 7], [0x1234f000, 1, 7], [DATA, 2], [HIGH, 1], [0xfffff000, 1], [0, 1], [0x8000, 1], [0xc000, 1, 7], [0x3000, 1]]) map(c, ...args);
  for (const row of [...calls, ...returns]) upload(c, row.pc, encoded(row));
  for (const row of [callFault, retFault]) upload(c, row.pc - 5, faultBlock(row).bytes);
  for (const b of liveBlocks) upload(c, b.pc, b.bytes);
  upload(c, rx.pc, encoded(rx)); upload(c, 0x8000, canary); host(c, 'protect', [0x8000, 1, 1]);
  for (const address of [PC, 0x1234f000, 0xc000]) host(c, 'protect', [address, 1, 5]);
  return c;
}

// literal targets and stack arithmetic distinguish signed rel16, high-pc truncation and unsigned cleanup.
for (const row of calls) {
  const before = {registers: registers(), pc: row.pc, flags: 0xcd7}; before.registers[4] = 0;
  const result = transition(before, row); assert.equal(result.next.pc, row.target); assert.equal(result.address, 0xfffffffe);
}
assert.equal(transition({registers: [0, 0, 0, 0, 0x1234ffff, 0, 0, 0], pc: 0x1200, flags: 2}, {op: 'ret', adjust: 0xffff}, 0x8001).next.registers[4], 0x12360000);
assert.equal(transition({registers: [0, 0, 0, 0, 0xfffffffe, 0, 0, 0], pc: 0x1200, flags: 2}, {op: 'ret', adjust: 0}, 0xffff).next.registers[4], 0);
assert.equal(transition({registers: [0, 0, 0, 0, 0xfffffffe, 0, 0, 0], pc: 0x1280, flags: 2}, {op: 'ret', adjust: 0xffff}, 0x8001).next.registers[4], 0xffff);
assert.equal(transition({registers: registers(), pc: 0x1234fffc, flags: 2}, calls[3]).value, 0);

const failures = ['unmapped', 'none', 'wrong-permission', 'execute-only', 'cross-missing', 'cross-denied', 'overflow'];
function failureSetup(c, name, access) {
  unmap(c, 0x9000); unmap(c, 0xa000);
  const crossing = name.startsWith('cross-'), address = name === 'overflow' ? 0xffffffff : crossing ? 0x9fff : 0x9008;
  if (name === 'overflow') {
    upload(c, 0xfffffffe, Buffer.from([0x55, 0xa5])); upload(c, 0, Buffer.from([0x5a, 0xaa]));
  } else if (name !== 'unmapped') {
    map(c, 0x9000); if (name === 'cross-denied') map(c, 0xa000);
    if (crossing) {
      upload(c, 0x9ffe, Buffer.from([0x55, 0x0f]));
      if (name === 'cross-denied') upload(c, 0xa000, Buffer.from([0x80, 0xaa]));
    } else upload(c, address - 1, Buffer.from([0x55, 0x0f, 0x80, 0xaa]));
    if (!crossing) host(c, 'protect', [0x9000, 1, name === 'none' ? 0 : name === 'execute-only' ? 4 : access === 1 ? 2 : 1]);
    if (name === 'cross-denied') host(c, 'protect', [0xa000, 1, access === 1 ? 2 : 1]);
  }
  return {name, address, access, original: name === 'unmapped' ? 0 : name === 'cross-missing' ? 0x000f : 0x800f,
    detail: name === 'overflow' ? 3 : name === 'unmapped' || name === 'cross-missing' ? 1 : 2,
    faultAddress: crossing ? 0xa000 : address};
}
function repair(c, f) {
  if (f.name === 'unmapped') map(c, 0x9000);
  else if (f.name === 'cross-missing') map(c, 0xa000);
  else host(c, 'protect', [f.name === 'cross-denied' ? 0xa000 : 0x9000, 1, 3]);
}
function stopped(c, module, row, f) {
  const esp = row.op === 'call' ? wrap(BigInt(f.address) + 2n) : f.address;
  const before = seed(c, row.pc - 5, esp), held = {...before, registers: [...before.registers], pc: row.pc};
  held.registers[3] = 0x11223344;
  const event = {access: f.access, address: f.address}; if (f.access === 2) event.value = low(row.pc + 4);
  const fault = {detail: f.detail, address: f.faultAddress, access: f.access};
  for (const [budget, retired] of [[2, 1], [1, 0]]) {
    run(c, module, budget, held, {label: `${row.op} ${f.name} precise fault/retry`, reason: 5, retired, fault, events: [event]});
    if (f.name === 'overflow') {checkRam(c, 0xfffffffe, Buffer.from([0x55, 0xa5])); checkRam(c, 0, Buffer.from([0x5a, 0xaa]));}
    counts.faults++;
  }
  return held;
}
function liveExpected(c) {
  const next = cpu(c); next.registers[0] = 0xa1008001; next.registers[2] = 0xa1020001;
  next.registers[3] = 0xa1030001; next.pc = 0x140f; next.flags = 0xc92; return next;
}
const liveEvents = [{access: 2, address: DATA + 6, value: 0x1404}, {access: 1, address: DATA + 6}];
for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries);
  const callUnit = compile(c, [...calls.map(block), faultBlock(callFault)]);
  for (const flag of flags) {
    seed(c, calls[0].pc, DATA + 8, flag); operation(c, callUnit, calls[0], 0, 'CALL all valid FLAGS'); counts.flag_inputs++;
  }
  for (const row of calls.slice(1)) for (const flag of [2, 0x443, 0x892, 0xcd7]) {
    seed(c, row.pc, DATA + 9, flag); operation(c, callUnit, row, 0, 'signed/payload/high-PC CALL cold target', 3); counts.relative++;
  }
  for (const esp of [DATA + 8, DATA + 9, DATA + 0x1001, HIGH + 9, 0, 2]) {
    seed(c, calls[0].pc, esp); const result = operation(c, callUnit, calls[0], 0, 'CALL fullESP boundary');
    assert.equal(result.next.pc, 0x3000); counts.boundaries++;
  }
  seed(c, calls[0].pc, DATA + 8);
  for (let i = 0; i < 2; i++) {
    const before = cpu(c); input(c, 0, state({...before, pc: calls[0].pc})); operation(c, callUnit, calls[0], 0, 'CALL PC-only repeat consumes current ESP'); counts.repeats++;
  }
  for (const name of failures) {
    const f = failureSetup(c, name, 2); stopped(c, callUnit, callFault, f);
    if (name !== 'overflow') {
      repair(c, f); checkRam(c, f.address, word(f.original)); operation(c, callUnit, callFault, 0, 'CALL map/protect-only repair'); counts.repairs++;
    }
  }
  {
    const f = failureSetup(c, 'cross-denied', 2); stopped(c, callUnit, callFault, f); repair(c, f);
    upload(c, HIGH + 0x20, word(0xbeef)); const changed = cpu(c); changed.registers[4] = HIGH + 0x22;
    changed.registers[0] = 0x55667788; changed.flags = 2; input(c, 0, state(changed));
    operation(c, callUnit, callFault, 0, 'explicit current ESP/FLAGS/GPRs after CALL fault');
    checkRam(c, f.address, word(0x800f)); counts.current_inputs++;
  }
  for (const [budget, cancel, reason] of [[0, 0, 1], [0, 1, 2], [1, 1, 2]]) {
    const before = seed(c, calls[0].pc, DATA + 8); input(c, 96, words([cancel]));
    run(c, callUnit, budget, before, {label: 'CALL neutral budget/cancel', reason, retired: 0}); counts.controls++;
  }
  input(c, 96, words([0]));

  const retUnit = compile(c, [...returns.map(block), faultBlock(retFault), block(rx)]);
  for (const row of returns.slice(0, 2)) for (const flag of flags) {
    upload(c, DATA + 8, word(0x8001)); seed(c, row.pc, DATA + 8, flag);
    operation(c, retUnit, row, 0x8001, 'RET all valid FLAGS'); counts.flag_inputs++;
  }
  for (const row of returns) for (const saved of [0, 1, 0x8000, 0xffff]) {
    upload(c, DATA + 9, word(saved)); seed(c, row.pc, DATA + 9, 0x443);
    operation(c, retUnit, row, saved, 'RET unsigned adjustment and zero-extended cold target', 2); counts.adjustments++;
  }
  for (const row of returns) for (const esp of [DATA + 8, DATA + 9, DATA + 0xfff, HIGH + 9, 0xfffffffe, 0]) {
    upload(c, esp, word(0x8001)); seed(c, row.pc, esp);
    operation(c, retUnit, row, 0x8001, 'RET original fullESP boundary'); counts.boundaries++;
  }
  for (const address of [0x8000, rx.pc]) {
    seed(c, rx.pc, address); const values = address === rx.pc ? [0xc366, 0] : [0xa555, 0x6766];
    for (let i = 0; i < 2; i++) {
      if (i) {const before = cpu(c); input(c, 0, state({...before, pc: rx.pc}));}
      operation(c, retUnit, rx, values[i], 'RO/RX read with PC-only repeat retains same code owner'); counts.code_reads++;
    }
  }
  upload(c, DATA + 8, Buffer.from([0x34, 0x12, 0x78, 0x56])); seed(c, returns[0].pc, DATA + 8);
  for (const saved of [0x1234, 0x5678]) {
    const before = cpu(c); input(c, 0, state({...before, pc: returns[0].pc}));
    operation(c, retUnit, returns[0], saved, 'RET PC-only repeat consumes current stack'); counts.repeats++;
  }
  for (const name of failures) {
    const f = failureSetup(c, name, 1); stopped(c, retUnit, retFault, f);
    if (name !== 'overflow') {
      repair(c, f); checkRam(c, f.address, word(f.original)); operation(c, retUnit, retFault, f.original, 'RET map/protect-only repair'); counts.repairs++;
    }
  }
  {
    const f = failureSetup(c, 'cross-denied', 1); stopped(c, retUnit, retFault, f); repair(c, f);
    upload(c, HIGH + 0x20, word(0x4321)); const changed = cpu(c); changed.registers[4] = HIGH + 0x20;
    changed.registers[0] = 0x55667788; changed.flags = 0x892; input(c, 0, state(changed));
    operation(c, retUnit, retFault, 0x4321, 'explicit current RAM/ESP/FLAGS after RET fault');
    checkRam(c, f.address, word(0x800f)); counts.current_inputs++;
  }
  for (const row of returns.slice(0, 2)) for (const [budget, cancel, reason] of [[0, 0, 1], [0, 1, 2], [1, 1, 2]]) {
    const before = seed(c, row.pc, DATA + 8); input(c, 96, words([cancel]));
    run(c, retUnit, budget, before, {label: 'RET neutral budget/cancel', reason, retired: 0}); counts.controls++;
  }
  input(c, 96, words([0]));

  const liveUnit = compile(c, liveBlocks), r = registers(); r[0] = 0xa1007fff;
  seed(c, liveCall.pc, DATA + 8, 0x443, r);
  run(c, liveUnit, 10, liveExpected(c), {label: 'live CALL/callee/RET/returned WORD and carry consumers', reason: 3, retired: 7, events: liveEvents, helperValue: 0x1404});
  checkRam(c, DATA + 6, word(0x1404)); counts.consumers++;
  seed(c, calls[0].pc, DATA + 8);
  if (owner === 'resident') operation(c, callUnit, calls[0], 0, 'retained oldest resident remains current');
  else run(c, callUnit, 1, cpu(c), {label: 'replaced generation is neutral InvalidArtifact3', status: 3, retired: 0});
  counts.retained++; checkRam(c, 0x8000, canary); host(c, 'close', []);
}

for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) for (const identity of [false, true]) {
  const row = {op: 'call', pc: identity ? 0xe862 : 0xc000, target: 0x1234};
  row.displacement = low(BigInt(row.target) - BigInt(row.pc) - 4n);
  const c = open(owner, entries, row), module = compile(c, [block(row)]);
  const before = seed(c, row.pc, row.pc + 2, 0x892), effects = transition(before, row);
  assert.equal(effects.value, identity ? 0xe866 : 0xc004);
  assert.equal(word(effects.value).equals(encoded(row).subarray(0, 2)), identity);
  run(c, module, 10, effects.next, {label: 'own opcode CALL store commits target/ESP once before SMC', reason: 6,
    events: [{access: 2, address: row.pc, value: effects.value}]});
  checkRam(c, row.pc, Buffer.concat([word(effects.value), encoded(row).subarray(2)])); counts.self_code++;
  run(c, module, 10, cpu(c), {label: 'consumed code owner4 is neutral before fresh capture', status: 4, retired: 0}); counts.self_code++;
  const suffix = {pc: row.target, instructions: 1, bytes: Buffer.from([0xeb, 0])}, fresh = compile(c, [suffix]);
  const next = {...cpu(c), pc: row.target + 2};
  run(c, fresh, 2, next, {label: 'fresh pure target suffix does not replay CALL', reason: 3}); counts.self_code++;
  host(c, 'close', []);
}

// a top-of-u32 instruction has a nonwrapping four-byte fetch but wraps its saved nextpc to zero.
for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const row = {op: 'call', pc: 0xfffffffc, displacement: 2, target: 2, top: true};
  const c = open(owner, entries, row), module = compile(c, [block(row)]);
  seed(c, row.pc, DATA + 8, 0xcd7);
  const result = operation(c, module, row, 0, 'completed top CALL wraps nextPC, no target fetch', 2);
  assert.equal(result.value, 0); assert.equal(result.next.pc, 2); counts.relative++; host(c, 'close', []);
}

const installation = (unit, slot) => record('R3IN', 32, [unit.low, unit.high, slot, 0]);
for (const entries of [false, true]) {
  const c = open('resident', entries), caller = compile(c, liveBlocks.slice(0, 2)), callee = compile(c, liveBlocks.slice(2));
  const table = new WebAssembly.Table({element: 'anyfunc', initial: 8, maximum: 8});
  for (const [slot, unit] of [caller, callee].entries()) {
    table.set(slot, unit.run); assert.equal(table.get(slot), unit.run);
    const expected = arena(c); expected.set(installation(unit, slot), 140);
    assert.equal(c.api.acknowledge_resident_installation(c.id, c.keyHigh, unit.low, unit.high, slot), 0);
    assert.deepEqual(arena(c), expected, 'installation acknowledges exact selected run function');
  }
  const expected = arena(c); assert.equal(c.api.dispatcher_module(c.id, c.keyHigh), 0);
  const observed = arena(c), pointer = observed.readUInt32LE(164), length = observed.readUInt32LE(168);
  expected.set(record('R3DP', 32, [c.id, c.keyHigh, pointer, length]), 140);
  assert.deepEqual(arena(c), expected, 'dispatcher getter preserves full arena outside metadata');
  const bytes = Buffer.from(new Uint8Array(c.memory.buffer, pointer, length)), module = new WebAssembly.Module(bytes);
  assert.deepEqual(WebAssembly.Module.imports(module), [{module: 'env', name: 'memory', kind: 'memory'},
    {module: 'env', name: 'table', kind: 'table'}, {module: 'ring3', name: 'guard_dispatch_entry', kind: 'function'},
    {module: 'ring3', name: 'find_installed_resident', kind: 'function'}]);
  assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  const lookups = [], dispatcher = new WebAssembly.Instance(module, {env: {memory: c.memory, table}, ring3: {
    guard_dispatch_entry: c.api.guard_dispatch_entry,
    find_installed_resident: (...args) => {const status = c.api.find_installed_resident(...args); lookups.push({pc: args[2] >>> 0, status}); return status;},
  }}).exports;
  const file = `dispatcher-${c.id}.wasm`; writeFileSync(join(output, file), bytes, {flag: 'wx'});
  modules.push({context: c.id, owner: c.owner, entries, file, kind: 'dispatcher', sha256: hash(bytes), instructions: 0, blocks: 0,
    imports: ['guard_dispatch_entry', 'find_installed_resident']});
  const r = registers(); r[0] = 0xa1007fff; seed(c, liveCall.pc, DATA + 8, 0x443, r);
  run(c, {...dispatcher, exitVersion: 3}, 10, liveExpected(c), {label: 'product dispatcher transfers CALL/RET across two installed units',
    reason: 3, retired: 7, events: liveEvents, helperValue: 0x1404, transfer: installation(caller, 0)});
  assert.deepEqual(lookups, [{pc: 0x1400, status: 0}, {pc: 0x1500, status: 0}, {pc: 0x1404, status: 0}, {pc: 0x140f, status: 17}]);
  assert.equal(table.get(0), caller.run); assert.equal(table.get(1), callee.run);
  checkRam(c, DATA + 6, word(0x1404)); counts.dispatcher++; host(c, 'close', []);
}

assert.deepEqual(sources(), sourcePins, 'selected production and fixture sources stayed fixed during recording');
assert.deepEqual(counts, {contexts: 18, runs: 2098, retired: 2024, flag_inputs: 1536, relative: 52, boundaries: 144, adjustments: 80,
  code_reads: 16, repeats: 16, controls: 36, faults: 128, repairs: 48, current_inputs: 8, consumers: 4,
  retained: 4, self_code: 24, dispatcher: 2, helper_calls: 2050}, 'authored finite counts and observed forwarded helper total');
assert.equal(modules.length, 38);
const result = {status: 'ok', engine_sha256: engine.sha256, source_pins: sourcePins, counts, modules, observations, limits: [
  'exact single66 direct CALLrel16/plainRET/RETimm16 in four bound profiles; indirectCALL/far/segmentation/CET remain outside',
  'independent BigInt target/return16/fullESP oracle, high-PC and completed top fetch, unsigned cleanup, all128 valid FLAGS per form',
  'observed canonical read16V2/store16V4 forwarding and resident binding identity; cold target is not fetched by CALL/RET',
  'seven read/store fault classes with prior retirement/unchanged retry/honest map-protect-only repair, separate explicit current inputs; overflow observes top and wrapped RAM canaries after each failure',
  'live CALL/callee/RET/returned WORD+CF/OF consumers without CPU reseed; two installed resident units exercise unchanged product returning dispatcher',
  'changed and real identical own-opcode writes commit once before SMC6, neutral consumed-owner4, fresh pure suffix; replacement generation mismatch3 is separate',
  'complete4364 arena/dirtyFP128/allGPR/FLAGS/helper/canaries, code uploaded before capture, sequential replacement capture/use; all8blocks/64instructions/65536bytes/max6captures checked before compile',
  '38 physical modules includes two dispatcher modules; helper_calls is observed forwarding, rejected status rows have null observed reason/retired; no hardware/performance/PE/browser/SDK/game/hostedCI claim',
]};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2), {flag: 'wx'});
console.log(JSON.stringify({status: 'ok', ...counts, modules: modules.length, output}));
