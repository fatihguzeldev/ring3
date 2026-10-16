import {generatedImports, attachDetectionApi} from '../../../../../../support/wasm/binding.mjs';
import {ARENA_SIZE} from '../../../../../../support/wasm/arena.mjs';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../../../../../../support/wasm/engine.mjs';

const [enginePath, output, root] = process.argv.slice(2), SIZE = ARENA_SIZE;
assert.ok(enginePath && output && root);
const engine = readEngine(enginePath), initial = readFileSync(join(output, 'initial-arena.bin'));
assert.equal(initial.length, SIZE);
const hash = b => createHash('sha256').update(b).digest('hex');
const paths = ['engine/src/abi/arena.rs', 'engine/src/cpu/x86/decode/profile.rs', 'engine/src/cpu/x86/decode/families.rs', 'engine/src/cpu/x86/decode/lower.rs', 'engine/src/cpu/x86/decode/families.rs',
  'engine/src/cpu/dbt/region.rs', 'engine/src/cpu/dbt/cold.rs', 'engine/src/cpu/dbt/wasm/abi.rs',
  'engine/src/cpu/dbt/wasm/instructions/integer/arithmetic.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/bits.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/carry_rotates.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/decimal.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/mod.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/arithmetic.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/moves.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/exchange.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/predicates.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/shifts.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/control.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/extended.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/flags.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/mod.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/multiply_divide.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/operands.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/rotates.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/shifts.rs', 'engine/src/cpu/dbt/wasm/imports/mod.rs', 'engine/src/cpu/dbt/wasm/imports/binding.rs', 'engine/src/cpu/dbt/wasm/imports/context.rs', 'engine/src/cpu/dbt/wasm/imports/needs.rs', 'engine/src/cpu/dbt/wasm/helpers/protocol.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/mod.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/address.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/scalar.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/stack.rs', 'engine/src/cpu/dbt/wasm/emitter/mod.rs', 'engine/src/cpu/dbt/wasm/emitter/module.rs', 'engine/src/cpu/dbt/wasm/emitter/body.rs', 'engine/src/cpu/dbt/wasm/requirements.rs', 'engine/src/cpu/dbt/wasm/imports/wiring.rs',
  'engine/src/cpu/dbt/wasm/dispatcher.rs', 'engine/src/process/instance.rs', 'engine/src/process/execution/compiled.rs', 'engine/src/process/execution/authority.rs', 'engine/src/process/memory/address_space.rs', 'engine/src/process/memory/scalar.rs', 'engine/src/process/memory/scalar/tests.rs', 'engine/src/process/execution/mod.rs', 'engine/src/process/memory/mod.rs', 'engine/src/process/mod.rs', 'engine/src/process/execution/resident.rs', 'engine/src/process/execution/installation.rs', 'engine/src/process/wasm/mod.rs', 'engine/src/process/wasm/loading.rs', 'engine/src/abi/image.rs', 'engine/src/process/wasm/execution.rs', 'engine/src/abi/execution.rs', 'engine/src/process/wasm/calls.rs', 'engine/src/process/wasm/memory.rs', 'engine/src/process/wasm/cpu.rs', 'engine/src/memory/space.rs', 'engine/src/memory/space/atomic.rs',
  'engine/tests/cpu/control/hints/native/pause.rs', 'engine/tests/cpu/control/native.rs', 'engine/tests/support/cpu/prefix/admission.rs', 'engine/tests/cpu/control/hints/wasm/pause.rs', 'engine/tests/cpu/control/wasm.rs', 'engine/tests/support/wasm.rs', 'engine/tests/cpu/control/hints/fixtures/p2-pause/kernel.c',
  'engine/tests/cpu/control/hints/fixtures/p2-pause/kernel.text.bin', 'engine/tests/cpu/control/hints/fixtures/p2-pause/receipt.json',
  'engine/tests/cpu/control/hints/fixtures/p2-pause/runtime/run.mjs', 'engine/tests/support/wasm/engine.mjs', 'engine/tests/support/wasm/binding.mjs'];
const sources = () => Object.fromEntries(paths.map(p => [p, hash(readFileSync(join(root, p)))]));
const sourcePins = sources(), contexts = [], modules = [], observations = [], helperTrace = [];
const counts = {contexts: 0, runs: 0, retired: 0, pure_flags: 0, polls: 0, helper_calls: 0, read32: 0, store32: 0};
const pure = [{pc: 0x1000, bytes: readFileSync(join(output, 'pause.x86')), instructions: 2}];
const live = [{pc: 0x1400, bytes: readFileSync(join(output, 'live.x86')), instructions: 6}];
assert.deepEqual(pure[0].bytes, Buffer.from([0xf3, 0x90, 0xeb, 0]));
assert.deepEqual(live[0].bytes, Buffer.from([0x04, 1, 0xf3, 0x90, 0x66, 0x11, 0xd0, 0x0f, 0x92, 0xc3, 0x0f, 0x90, 0xc7, 0xeb, 0]));
const kernel = readFileSync(join(root, 'engine/tests/cpu/control/hints/fixtures/p2-pause/kernel.text.bin'));
assert.equal(hash(kernel), '2709e35459b460e6587fbbc0fc1428bac4ec3457d69e844e5d94482a305ed1b6');
const leaders = [0, 16, 20, 26, 31, 33, 35], lengths = [16, 4, 6, 5, 2, 2, 5], instructions = [7, 1, 3, 3, 1, 1, 4];
const poll = leaders.map((offset, i) => ({pc: 0x2000 + offset, bytes: kernel.subarray(offset, offset + lengths[i]), instructions: instructions[i]}));
assert.equal(Buffer.concat(poll.map(b => b.bytes)).toString('hex'), kernel.toString('hex'));
const words = values => {const b = Buffer.alloc(values.length * 4); values.forEach((v, i) => b.writeUInt32LE(v >>> 0, i * 4)); return b;};
function record(magic, size, fields, version = 1) {
  const b = Buffer.alloc(size); b.write(magic); b.writeUInt16LE(version, 4); b.writeUInt16LE(1, 6); b.writeUInt32LE(size, 8);
  fields.forEach((v, i) => b.writeUInt32LE(v >>> 0, 16 + i * 4)); return b;
}
const state = s => record('R3ST', 56, [...s.registers, s.pc, s.flags]);
const exit = (reason, retired, version = 1) => record('R3EX', 40, [reason, retired, 0, 0, 0, 0], version);
const helper = value => record('R3MH', 40, [0, value, 0, 0, 0, 0]);
const cpuHelper = value => record('R3MH', 40, [0, value, 0, 0, 0, 4], 10);
const arena = c => Buffer.from(new Uint8Array(c.memory.buffer, c.base, SIZE));
const cpu = c => {const b = arena(c); return {registers: Array.from({length: 8}, (_, i) => b.readUInt32LE(16 + i * 4)), pc: b.readUInt32LE(48), flags: b.readUInt32LE(52)};};
const parents = () => Array.from({length: 8}, (_, i) => (0xa100 + i) * 65536 + 0x1234 + i);
function input(c, offset, bytes) {const expected = arena(c); expected.set(bytes, offset); new Uint8Array(c.memory.buffer).set(bytes, c.base + offset); assert.deepEqual(arena(c), expected);}
function host(c, name, args, span = null) {
  const expected = arena(c); assert.equal(c.api[name](...args), 0, name);
  if (span) expected.set(arena(c).subarray(...span), span[0]);
  assert.deepEqual(arena(c), expected, 'host full arena: ' + name);
}
function upload(c, address, bytes) {input(c, 140, bytes); host(c, 'upload', [address, bytes.length]);}
function seed(c, pc, flags = 0xcd7, registers = parents()) {
  input(c, 0, Buffer.concat([state({registers, pc, flags}), exit(3, 0), words([0]), record('R3MH', 40, [0, 0xdecafbad, 0, 0, 0, 0])]));
  return cpu(c);
}
const arities = {open: 3, close: 0, arena_ptr: 0, map: 3, protect: 3, upload: 2, read32: 1,
  store32: 2, store_resident32: 6, cpu_read32: 1, cpu_store32: 2, cpu_store_resident32: 6, compile: 1, compile_entries: 2, compile_resident: 1, compile_resident_entries: 2,
  generation: 0, module_ptr: 0, module_len: 0, guard: 6, guard_resident: 7, acknowledge_resident_installation: 5,
  dispatcher_module: 2, guard_dispatch_entry: 5, find_installed_resident: 3};
function open(owner, entries, purpose) {
  const c = {id: ++counts.contexts, owner, entries, purpose, captures: 0, high: 0x50415553};
  if (owner === 'standalone') {c.memory = new WebAssembly.Memory({initial: 1}); c.base = 128; input(c, 0, initial); return c;}
  const instance = new WebAssembly.Instance(engine.module, {}); c.memory = instance.exports.memory; c.api = {};
  for (const [name, arity] of Object.entries(arities)) {c.api[name] = instance.exports['ring3_abi_v1_' + name]; assert.equal(c.api[name]?.length, arity, name);}
  attachDetectionApi(c.api, instance.exports);
  assert.equal(c.api.open(4, c.id, c.high), 0); c.base = c.api.arena_ptr() >>> 0; input(c, 0, initial);
  for (const [address, permission] of [[0x1000, 7], [0x2000, 7], [0x5000, 3], [0x7000, 3]]) host(c, 'map', [address, 1, permission]);
  for (const b of [...pure, ...live]) upload(c, b.pc, b.bytes);
  upload(c, 0x2000, kernel); host(c, 'protect', [0x2000, 1, 4]); upload(c, 0x5010, words([0x55aa19f3, 0xa55a8127]));
  return c;
}
function compile(c, name, blocks) {
  const total = blocks.reduce((sum, b) => sum + b.instructions, 0);
  assert.ok(blocks.length > 0 && blocks.length <= 8 && total <= 64); assert.ok(++c.captures <= 6);
  let bytes, binding = {};
  if (c.owner === 'standalone') bytes = readFileSync(join(output, `standalone-${c.entries ? 'entries' : 'extent'}-${name}.wasm`));
  else {
    input(c, 140, words(blocks.flatMap(b => c.entries ? [b.pc] : [b.pc, b.bytes.length])));
    const method = c.owner === 'resident' ? (c.entries ? 'compile_resident_entries' : 'compile_resident') : (c.entries ? 'compile_entries' : 'compile');
    host(c, method, c.entries ? [blocks.length, 0] : [blocks.length], c.owner === 'resident' ? [140, 164] : null);
    const b = arena(c); binding = c.owner === 'resident' ? {low: b.readUInt32LE(148), high: b.readUInt32LE(152), pointer: b.readUInt32LE(156), length: b.readUInt32LE(160)}
      : {generation: c.api.generation(), pointer: c.api.module_ptr() >>> 0, length: c.api.module_len() >>> 0};
    bytes = Buffer.from(new Uint8Array(c.memory.buffer, binding.pointer, binding.length));
  }
  assert.ok(bytes.length <= 65536); const module = new WebAssembly.Module(bytes), guard = c.owner === 'resident' ? 'guard_resident' : 'guard';
  const imports = generatedImports({bound: c.owner !== 'standalone', resident: c.owner === 'resident',
    helpers: name === 'poll' ? ['cpu_read32', c.owner === 'resident' ? 'cpu_store_resident32' : 'cpu_store32'] : []});
  assert.deepEqual(WebAssembly.Module.imports(module), imports); assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  const ring3 = {...c.api};
  if (name === 'poll') for (const imported of imports.filter(i => i.name === 'cpu_read32' || i.name.startsWith('cpu_store'))) ring3[imported.name] = (...args) => {
    const expected = arena(c), status = c.api[imported.name](...args), after = arena(c); assert.equal(status, 0);
    expected.set(after.subarray(100, 140), 100); assert.deepEqual(after, expected, 'canonical helper changes only its record');
    const value = after.readUInt32LE(120); assert.deepEqual(after.subarray(100, 140), cpuHelper(value));
    const store = imported.name.startsWith('cpu_store'), address = args[store && c.owner === 'resident' ? 4 : 0] >>> 0;
    if (store) assert.equal(value, 0);
    helperTrace.push({context: c.id, call: counts.runs + 1, name: imported.name, address, value: store ? args.at(-1) >>> 0 : value, status, helper_hex: after.subarray(100, 140).toString('hex')});
    counts.helper_calls++; counts[store ? 'store32' : 'read32']++; return status;
  };
  const unit = {...binding, name, run: new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3}).exports.run, exitVersion: name === 'poll' ? 6 : 1};
  assert.equal(unit.run.length, 4); const file = `module-${modules.length + 1}.wasm`; writeFileSync(join(output, file), bytes, {flag: 'wx'});
  modules.push({context: c.id, name, owner: c.owner, entries: c.entries, file, sha256: hash(bytes), bytes: bytes.length, blocks: blocks.length, instructions: total, imports, ...binding}); return unit;
}
function run(c, unit, budget, next, label, {reason = 1, retired = 1, status = 0, value, transfer, version = unit.exitVersion} = {}) {
  const before = arena(c), expected = Buffer.from(before), beforeEvents = helperTrace.length;
  if (status === 0) {expected.set(state(next)); expected.set(exit(reason, retired, version), 56); if (value !== undefined) expected.set(cpuHelper(value), 100); if (transfer) expected.set(transfer, 140);}
  const actual = unit.run(c.base, c.base + 56, budget, c.base + 96); assert.equal(actual, status, label); assert.deepEqual(arena(c), expected, 'complete arena: ' + label);
  const b = arena(c), saved = {};
  if (c.purpose === 'poll') for (const [phase, bytes] of [['before', before], ['after', b]]) {
    const file = `arena-${c.id}-${counts.runs + 1}-${phase}.bin`; writeFileSync(join(output, file), bytes, {flag: 'wx'}); saved[phase] = {file, sha256: hash(bytes), bytes: bytes.length};
  }
  observations.push({context: c.id, label, budget, expected_status: status, observed_status: actual,
    expected_reason: status ? null : reason, observed_reason: status ? null : b.readUInt32LE(72), expected_retired: status ? null : retired, observed_retired: status ? null : b.readUInt32LE(76),
    expected_pc: status ? null : next.pc, observed_pc: status ? null : b.readUInt32LE(48), expected_flags: status ? null : next.flags, observed_flags: status ? null : b.readUInt32LE(52),
    expected_eax: status ? null : next.registers[0], observed_eax: status ? null : b.readUInt32LE(16), observed_version: status ? null : b.readUInt16LE(60),
    expected_cpu: status ? null : next, observed_cpu: status ? null : cpu(c), helper_calls: helperTrace.length - beforeEvents, ...saved});
  counts.runs++; if (!status) counts.retired += retired;
}
function read(c, address, value) {const expected = arena(c); expected.set(helper(value), 100); assert.equal(c.api.read32(address), 0); assert.deepEqual(arena(c), expected, 'separate selected RAM diagnostic');}
for (const owner of ['standalone', 'replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries, 'pure'), unit = compile(c, 'pause', pure);
  for (const f of [2, 3, 0x12, 0x46, 0x82, 0x402, 0x802, 0xcd7]) {seed(c, 0x1000, f); run(c, unit, 1, {...cpu(c), pc: 0x1002}, 'exact PAUSE preserves selected FLAGS and full dirty state'); counts.pure_flags++;}
  seed(c, 0x1000); for (let i = 0; i < 2; i++) {input(c, 0, state({...cpu(c), pc: 0x1000})); run(c, unit, 1, {...cpu(c), pc: 0x1002}, 'only PC rewound, returned current inputs retained');}
  const current = cpu(c); current.pc = 0x1000; current.flags = 0x892; current.registers = current.registers.map((r, i) => (r + 0x12340000 + i) >>> 0);
  input(c, 0, state(current)); run(c, unit, 1, {...cpu(c), pc: 0x1002}, 'explicit current full parents and FLAGS');
  seed(c, 0x1000); run(c, unit, 0, cpu(c), 'zero budget', {retired: 0}); input(c, 96, words([1]));
  run(c, unit, 1, cpu(c), 'cancel', {reason: 2, retired: 0}); run(c, unit, 0, cpu(c), 'cancel before zero budget', {reason: 2, retired: 0}); input(c, 96, words([0]));
  run(c, unit, 3, {...cpu(c), pc: 0x1004}, 'PAUSE and tail then cold', {reason: 3, retired: 2}); run(c, unit, 1, cpu(c), 'cold', {reason: 3, retired: 0}); run(c, unit, 0, cpu(c), 'budget before cold', {retired: 0});
  if (c.api) upload(c, 0x5008, words([1])); seed(c, 0x1000); run(c, unit, 1, {...cpu(c), pc: 0x1002}, 'unrelated data write preserves source owner');
  const nextUnit = compile(c, 'live', live); seed(c, 0x1000);
  run(c, unit, 1, owner === 'replacement' ? cpu(c) : {...cpu(c), pc: 0x1002}, 'retained owner after later capture', owner === 'replacement' ? {status: 3, retired: 0} : {});
  const r = parents(); r[0] = 0xa10000ff; r[2] = 0xa1020000; seed(c, 0x1400, 0xcd7, r);
  let next = cpu(c); next.pc = 0x1402; next.registers[0] = 0xa1000000; next.flags = 0x457; run(c, nextUnit, 1, next, 'live ADD produces carry');
  run(c, nextUnit, 1, {...cpu(c), pc: 0x1404}, 'PAUSE preserves live producer state');
  next = cpu(c); next.pc = 0x1407; next.registers[0] = 0xa1000001; next.flags = 0x402; run(c, nextUnit, 1, next, 'ADC consumes preserved carry');
  next = cpu(c); next.pc = 0x140d; next.registers[3] = 0xa1030000; run(c, nextUnit, 2, next, 'SETB and SETO consume returned flags', {retired: 2});
  run(c, nextUnit, 2, {...cpu(c), pc: 0x140f}, 'live tail then cold', {reason: 3});
  if (c.api) {read(c, 0x5010, 0x55aa19f3); read(c, 0x5014, 0xa55a8127); upload(c, 0x1402, Buffer.from([0xf3]));
    run(c, nextUnit, 1, cpu(c), 'same-byte live source mutation alone invalidates owner', {status: 4, retired: 0}); host(c, 'close', []);
    run(c, nextUnit, 1, cpu(c), 'closed owner neutral', {status: 5, retired: 0});}
  contexts.push({id: c.id, purpose: c.purpose, owner, entries, captures: c.captures, mapped_pages: c.api ? 4 : null});
}
function installed(c, leaf) {
  if (c.owner !== 'resident') return leaf;
  const table = new WebAssembly.Table({element: 'anyfunc', initial: 8, maximum: 8}); table.set(0, leaf.run);
  host(c, 'acknowledge_resident_installation', [c.id, c.high, leaf.low, leaf.high, 0], [140, 172]);
  const receipt = record('R3IN', 32, [leaf.low, leaf.high, 0, 0]); assert.deepEqual(arena(c).subarray(140, 172), receipt);
  host(c, 'dispatcher_module', [c.id, c.high], [140, 172]); const b = arena(c);
  assert.deepEqual(Array.from({length: 6}, (_, i) => b.readUInt32LE(140 + i * 4)), [0x50443352, 0x10001, 32, 0, c.id, c.high]);
  const pointer = b.readUInt32LE(164), length = b.readUInt32LE(168), bytes = Buffer.from(new Uint8Array(c.memory.buffer, pointer, length)), module = new WebAssembly.Module(bytes);
  const imports = [{module: 'env', name: 'memory', kind: 'memory'}, {module: 'env', name: 'table', kind: 'table'},
    {module: 'ring3', name: 'guard_dispatch_entry', kind: 'function'}, {module: 'ring3', name: 'find_installed_resident', kind: 'function'}];
  assert.deepEqual(WebAssembly.Module.imports(module), imports);
  const fn = new WebAssembly.Instance(module, {env: {memory: c.memory, table}, ring3: c.api}).exports.run; assert.equal(fn.length, 4);
  const file = `module-${modules.length + 1}.wasm`; writeFileSync(join(output, file), bytes, {flag: 'wx'});
  modules.push({context: c.id, name: 'dispatcher', owner: c.owner, file, sha256: hash(bytes), bytes: bytes.length, blocks: 0, instructions: 0, imports});
  return {...leaf, run: fn, transfer: receipt};
}
function pollSeed(c, limit, readyAddress) {
  upload(c, 0x7000, words([0x11223344, 0x55667788, 0x3000, readyAddress, limit, 0xffffffff, 0x55aa19f3]));
  const r = parents(); r[4] = 0x7008; return seed(c, 0x2000, 0xcd7, r);
}
function completed(start, limit, result, flags) {
  const registers = [...start.registers]; registers[0] = result; registers[1] = 0xffffffff; registers[2] = limit; registers[4] = 0x700c;
  return {registers, pc: 0x3000, flags};
}
function events(c, start, limit, readyAddress, values) {
  const write = c.owner === 'resident' ? 'cpu_store_resident32' : 'cpu_store32';
  return [[write, 0x7004, start.registers[7]], [write, 0x7000, start.registers[6]], ['cpu_read32', 0x7014, 0xffffffff], ['cpu_read32', 0x7010, limit],
    ...(limit ? [['cpu_read32', 0x700c, readyAddress], ...values.map(value => ['cpu_read32', readyAddress, value])] : []),
    ['cpu_read32', 0x7000, start.registers[6]], ['cpu_read32', 0x7004, start.registers[7]], ['cpu_read32', 0x7008, 0x3000]];
}
const traceRows = start => helperTrace.slice(start).map(({name, address, value}) => [name, address, value]);
for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries, 'poll'), leaf = compile(c, 'poll', poll), unit = installed(c, leaf), coldVersion = owner === 'resident' ? 3 : 6;
  const identity = owner === 'resident' ? {low: leaf.low, high: leaf.high, slot: 0} : {generation: leaf.generation};
  for (const [limit, ready, readyAddress, result, finalLimit, flags, retired] of [
    [0, 0, 0x9008, 0xffffffff, 0, 0x486, 11],
    [3, 0, 0x5008, 0xffffffff, 0, 0x486, 31],
    [3, 1, 0x5008, 2, 3, 0x413, 16],
  ]) {
    upload(c, 0x5008, words([ready])); const start = pollSeed(c, limit, readyAddress), at = helperTrace.length;
    run(c, unit, 100, completed(start, finalLimit, result, flags), `compiler leaf limit${limit}/ready${ready}`, {reason: 3, retired, value: 0x3000, transfer: unit.transfer, version: coldVersion});
    assert.deepEqual(traceRows(at), events(c, start, limit, readyAddress, Array(limit ? ready ? 1 : limit : 0).fill(ready)));
    read(c, 0x7000, start.registers[6]); read(c, 0x7004, start.registers[7]); read(c, 0x7008, 0x3000); read(c, 0x7018, 0x55aa19f3);
    counts.polls++;
  }
  upload(c, 0x5008, words([0])); const start = pollSeed(c, 5, 0x5008), at = helperTrace.length;
  const partial = {registers: [...start.registers], pc: 0x2014, flags: 0x402};
  partial.registers[0] = 0; partial.registers[1] = 0xffffffff; partial.registers[2] = 4; partial.registers[4] = 0x7000; partial.registers[6] = 0x5008; partial.registers[7] = 0;
  run(c, unit, 14, partial, 'one completed poll iteration yields at original loop PC', {retired: 14, value: 0, transfer: unit.transfer});
  assert.deepEqual(traceRows(at), events(c, start, 5, 0x5008, [0]).slice(0, -3));
  run(c, unit, 0, cpu(c), 'split zero budget does not poll', {retired: 0, version: coldVersion}); input(c, 96, words([1]));
  run(c, unit, 20, cpu(c), 'split cancellation does not poll', {reason: 2, retired: 0, version: coldVersion});
  run(c, unit, 0, cpu(c), 'split cancellation precedes budget', {reason: 2, retired: 0, version: coldVersion}); input(c, 96, words([0]));
  upload(c, 0x5008, words([1]));
  if (owner === 'resident') host(c, 'guard_resident', [c.id, c.high, leaf.low, leaf.high, c.base, c.base + 56, c.base + 96]);
  else host(c, 'guard', [c.id, c.high, leaf.generation, c.base, c.base + 56, c.base + 96]);
  const resumeAt = helperTrace.length;
  run(c, unit, 100, completed(start, 4, 3, 0x417), 'host ready write lets same captured leaf return without CPU reseed', {reason: 3, retired: 8, value: 0x3000, transfer: unit.transfer, version: coldVersion});
  assert.deepEqual(traceRows(resumeAt), [['cpu_read32', 0x5008, 1], ['cpu_read32', 0x7000, start.registers[6]], ['cpu_read32', 0x7004, start.registers[7]], ['cpu_read32', 0x7008, 0x3000]]);
  assert.deepEqual(owner === 'resident' ? {low: leaf.low, high: leaf.high, slot: 0} : {generation: leaf.generation}, identity);
  run(c, unit, 1, cpu(c), 'returned cold PC cannot replay leaf or read ready', {reason: 3, retired: 0, version: coldVersion});
  read(c, 0x5008, 1); read(c, 0x5010, 0x55aa19f3); read(c, 0x5014, 0xa55a8127); read(c, 0x7000, start.registers[6]); read(c, 0x7004, start.registers[7]); read(c, 0x7018, 0x55aa19f3);
  assert.deepEqual([counts.read32, counts.store32, counts.helper_calls], [29 * counts.polls / 3, 8 * counts.polls / 3, 37 * counts.polls / 3]);
  contexts.push({id: c.id, purpose: c.purpose, owner, entries, captures: c.captures, mapped_pages: 4, identity, ready_write: 'host update only between generated calls; no concurrency or synchronization claim'});
  host(c, 'close', []);
}
assert.deepEqual(sources(), sourcePins);
assert.deepEqual(counts, {contexts: 10, runs: 188, retired: 444, pure_flags: 48, polls: 12, helper_calls: 148, read32: 116, store32: 32});
assert.equal(modules.length, 18);
const result = {status: 'ok', engine_sha256: engine.sha256, source_pins: sourcePins, counts, contexts, modules, observations, helper_trace: helperTrace,
  limits: ['exact F390 PAUSE architectural no-op only; selected FLAGS and live producer/ADC/SETcc in six profiles; all128 NOP FLAGS evidence reused',
    'original unmodified40-byte compiler polling leaf, four bound profiles, finite limit0/3/5 and controlled ready values; complete arena including dirtyFP128 compared per call',
    'canonical read32/store32 forwarding observed separately from selected RAM diagnostic reads; no arbitrary helper substitute or whole RAM dump',
    'two pure captures/context and one polling capture/context; poll7blocks20instructions, pure1block2 and live1block6; default8/64/65536 limits unchanged',
    'two resident polling contexts reuse one installed identity and returning dispatcher; cancellation/budget/host ready write resume returned CPU without reseed',
    'saved standalone inputs have no runtime source guard; bound generation3/source4/closed5 reject neutrally with null observed packet fields',
    'no hardware delay/power/memory-order/fairness/host yield/concurrency/thread synchronization/performance/generic compiler/browser/SDK/game claim']};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2), {flag: 'wx'});
console.log(JSON.stringify({status: 'ok', ...counts, modules: modules.length, output}));
