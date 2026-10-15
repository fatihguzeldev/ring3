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
  'engine/src/abi/arena.rs', 'engine/src/abi/memory_helper.rs',
  'engine/src/cpu/x86/ir.rs', 'engine/src/cpu/x86/decode/lower.rs',
  'engine/src/cpu/x86/decode/operands.rs', 'engine/src/cpu/x86/decode/flow.rs', 'engine/src/cpu/x86/decode/profile.rs', 'engine/src/cpu/dbt/region.rs',
  'engine/src/cpu/dbt/wasm/integer/arithmetic.rs', 'engine/src/cpu/dbt/wasm/integer/bits.rs', 'engine/src/cpu/dbt/wasm/integer/carry_rotates.rs', 'engine/src/cpu/dbt/wasm/integer/decimal.rs', 'engine/src/cpu/dbt/wasm/integer/dispatch.rs', 'engine/src/cpu/dbt/wasm/integer/flags.rs', 'engine/src/cpu/dbt/wasm/integer/mod.rs', 'engine/src/cpu/dbt/wasm/integer/multiply_divide.rs', 'engine/src/cpu/dbt/wasm/integer/operands.rs', 'engine/src/cpu/dbt/wasm/integer/rotates.rs', 'engine/src/cpu/dbt/wasm/integer/shifts.rs', 'engine/src/cpu/dbt/wasm/memory.rs', 'engine/src/cpu/dbt/wasm/memory/word_store.rs',
  'engine/src/cpu/dbt/wasm/memory/narrow.rs', 'engine/src/memory/space.rs', 'engine/src/memory/space/atomic.rs',
  'engine/src/cpu/dbt/wasm/abi.rs', 'engine/src/cpu/dbt/wasm/emitter.rs', 'engine/src/process/wasm/mod.rs', 'engine/src/process/wasm/loading.rs', 'engine/src/abi/image.rs', 'engine/src/process/wasm/execution.rs', 'engine/src/process/wasm/calls.rs', 'engine/src/process/wasm/memory.rs', 'engine/src/process/wasm/cpu.rs', 'engine/src/process/mod.rs',
  'engine/src/process/instance.rs', 'engine/src/process/execution/compiled.rs', 'engine/src/process/execution/authority.rs', 'engine/src/process/memory/address_space.rs', 'engine/src/process/memory/scalar.rs', 'engine/src/process/memory/scalar/tests.rs', 'engine/src/process/execution/mod.rs', 'engine/src/process/memory/mod.rs', 'engine/src/process/execution/resident.rs', 'engine/tests/cpu/moves/addressing/moffs/native/word.rs', 'engine/tests/cpu/moves/native.rs', 'engine/tests/support/cpu/prefix/admission.rs', 'engine/tests/support/cpu/prefix/word.rs',
  'engine/tests/cpu/moves/addressing/moffs/wasm/word.rs', 'engine/tests/cpu/moves/wasm.rs', 'engine/tests/support/wasm.rs',
  'engine/tests/cpu/moves/addressing/moffs/fixtures/p2-word-moffs/runtime/run.mjs', 'engine/tests/support/wasm/engine.mjs'];
assert.equal(new Set(paths).size, paths.length);
paths.push(...["engine/src/abi/memory_helper/cpu.rs", "engine/src/cpu/dbt/wasm/memory/cpu_scalar.rs", "engine/src/cpu/dbt/wasm/emitter.rs", "engine/src/cpu/dbt/wasm/code_fetch.rs", "engine/src/process/memory/cpu_scalar.rs", "engine/src/process/memory/scalar/cpu_write.rs", "engine/src/process/wasm/memory/cpu_write.rs", "engine/src/abi/wasm/exports/memory/cpu_write.rs"].filter(path=>!paths.includes(path)));
const sources = () => Object.fromEntries(paths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = sources(), modules = [];
const counts = {contexts: 0, runs: 0, retired: 0, matrix: 0, flag_inputs: 0, repeats: 0, faults: 0, retries: 0, current_inputs: 0, consumers: 0, controls: 0, retained_units: 0, code_reads: 0, self_code: 0};
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
const helper = (version, value = 0, f) => record('R3MH', 40,
  [f ? 1 : 0, value, f?.detail ?? 0, f?.address ?? 0, f?.access ?? 0, 2], version);
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
function run(c, module, budget, next, {label, reason = 1, retired = 1, fault, helperVersion = 10, helperValue = 0, status = 0, exitVersion = 6} = {}) {
  const expected = arena(c);
  if (status === 0) {
    expected.set(state(next)); expected.set(exit(reason, retired, fault, exitVersion), 56);
    if (helperVersion !== null) expected.set(helper(helperVersion, helperValue, fault), 100);
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
  const names = [c.owner === 'resident' ? 'guard_resident' : 'guard', ...(reads ? ['cpu_read16'] : []), ...(stores ? [c.owner === 'resident' ? 'cpu_store_resident16' : 'cpu_store16'] : [])];
  assert.deepEqual(imports, [{module: 'env', name: 'memory', kind: 'memory'}, ...names.map(name => ({module: 'ring3', name, kind: 'function'}))]);
  assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  const file = `module-${modules.length + 1}.wasm`; writeFileSync(join(output, file), bytes, {flag: 'wx'});
  modules.push({context: c.id, owner: c.owner, entries: c.entries, file, sha256: hash(bytes), imports: names, instructions});
  return {run: new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3: c.api}).exports.run};
}
const block = row => ({pc: row.pc, read: row.op === 'load', store: row.op === 'store', instructions: 2, bytes: Buffer.concat([row.bytes, Buffer.from([0xeb, 0])])});
const canary = Buffer.from([0x55, 0xa5, 0x66, 0x67, 0xf0, 0xf2, 0xf3, 0xaa]);
function map(c, address, pages, permission) {host(c, 'map', [address, pages, permission]); for (let i = 0; i < pages; i++) c.mapped.add(address + i * 4096);}
function unmap(c, address) {if (c.mapped.has(address)) {host(c, 'unmap', [address, 1]); c.mapped.delete(address);}}
function open(owner, entries) {
  const instance = new WebAssembly.Instance(engine.module, {});
  const c = {id: ++counts.contexts, owner, entries, memory: instance.exports.memory, api: {}, captures: 0, mapped: new Set()};
  for (const name of ['open', 'close', 'arena_ptr', 'map', 'unmap', 'protect', 'upload', 'read8', 'read16', 'store16', 'store_resident16',
    'compile', 'compile_entries', 'compile_resident', 'compile_resident_entries', 'generation', 'module_ptr', 'module_len', 'guard', 'guard_resident'])
    c.api[name] = instance.exports['ring3_abi_v1_' + name];
  for(const name of ["read16", "read8", "store16", "store_resident16"]) {c.api["cpu_"+name]=instance.exports["ring3_abi_v1_cpu_"+name];assert.equal(c.api["cpu_"+name]?.length,name.startsWith("read")?1:name.includes("resident")?6:2,"cpu_"+name);}
 assert.equal(c.api.open(12, c.id, 0x574d4f46), 0); c.base = c.api.arena_ptr() >>> 0; input(c, 0, initial);
  for (const args of [[PC, 1, 7], [DATA, 2, 3], [0xa53c5000, 1, 3], [0xfffff000, 1, 3], [0, 1, 3], [0x8000, 1, 3], [0x7ffff000, 1, 3], [0x80000000, 1, 3], [0xf3f06000, 1, 3]]) map(c, ...args);
  upload(c, 0x8000, canary); host(c, 'protect', [0x8000, 1, 1]);
  return c;
}

// absolute offsets, base256 WORD digits and own EAX high halves are independent of emitted helpers.
const lowWord = (parent, value) => Math.floor(parent / 65536) * 65536 + value;
const flags = Array.from({length: 128}, (_, n) => 2 + [1, 4, 16, 64, 128, 1024, 2048].reduce((sum, bit, i) => sum + Math.floor(n / 2 ** i) % 2 * bit, 0));
assert.equal(new Set(flags).size, 128); assert.ok(flags.every(value => (value & 2) && !(value & ~0xcd7)));
const addresses = [0, DATA + 8, DATA + 9, DATA + 0xfff, 0xa53c5008, 0xf3f06766, 0x80000000, 0xfffffffe];
const values = [0, 1, 0x7fff, 0x8000, 0xffff, 0x0101, 0xa166, 0xa366];
function row(op, address, pc) {return {op, address, pc, bytes: Buffer.from([0x66, op === 'load' ? 0xa1 : 0xa3, ...words([address])])};}
function transition(before, instruction, saved) {
  const next = {...before, registers: [...before.registers], pc: before.pc + 6};
  const value = instruction.op === 'load' ? saved : before.registers[0] % 65536;
  assert.ok(Number.isInteger(value) && value >= 0 && value <= 65535);
  if (instruction.op === 'load') next.registers[0] = lowWord(before.registers[0], value);
  return {next, value, address: instruction.address, helperVersion: 10};
}
function operation(c, module, instruction, saved, label, reason = 1) {
  const effects = transition(cpu(c), instruction, saved);
  run(c, module, 1, effects.next, {label, reason, helperVersion: effects.helperVersion, helperValue: instruction.op === 'load' ? effects.value : 0});
  checkRam(c, effects.address, word(effects.value)); return effects;
}
function frame(address, value) {
  if (address === 0) return {start: 0, offset: 0, bytes: Buffer.from([...word(value), 0xaa])};
  if (address === 0xfffffffe) return {start: 0xfffffffd, offset: 1, bytes: Buffer.from([0x55, ...word(value)])};
  return {start: address - 1, offset: 1, bytes: Buffer.from([0x55, ...word(value), 0xaa])};
}
function framed(c, address, value) {const f = frame(address, value); upload(c, f.start, f.bytes); return f;}
for (const [parent, source, expected] of [[0xfaceff0f, 0, 0xface0000], [0x80011234, 0xffff, 0x8001ffff], [0xffff0000, 0x8000, 0xffff8000], [0x0000ffff, 0x7fff, 0x7fff]]) assert.equal(lowWord(parent, source), expected);
const failureCases = ['unmapped', 'none', 'wrong-permission', 'execute-only', 'cross-missing', 'cross-denied', 'overflow'];
function failureAddress(name) {return name === 'overflow' ? 0xffffffff : name.startsWith('cross-') ? 0x9fff : 0x9008;}
function failureSetup(c, name, access) {
  unmap(c, 0x9000); unmap(c, 0xa000); const address = failureAddress(name), crossing = name.startsWith('cross-');
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
function stoppedFault(c, module, instruction, r, fault, label, afterFault) {
  const before = seed(c, instruction.pc - 5, r), stopped = {...before, registers: [...before.registers], pc: instruction.pc}; stopped.registers[3] = 0x11223344;
  const overflow = instruction.name === 'overflow';
  const helperVersion = overflow ? null : 10;
  const reason = overflow ? 11 : 5, exitVersion = 6;
  if (overflow) fault = undefined;
  run(c, module, 2, stopped, {label: label + ' after prior MOV retirement', reason, retired: 1, fault, helperVersion, exitVersion});
  afterFault?.();
  run(c, module, 1, stopped, {label: label + ' unchanged retry', reason, retired: 0, fault, helperVersion, exitVersion});
  afterFault?.(); counts.faults += 2; return stopped;
}
const mov = Buffer.from([0xbb, 0x44, 0x33, 0x22, 0x11]);
const live = Buffer.concat([
  row('load', DATA + 0x100, 0).bytes, Buffer.from([0x66, 0x83, 0xd0, 0]), row('store', DATA + 0x102, 0).bytes,
  Buffer.from([0x0f, 0x90, 0xc3]), row('load', DATA + 0x102, 0).bytes, Buffer.from([0x66, 0x0f, 0xbc, 0xc8, 0x0f, 0x94, 0xc7, 0xeb, 0]),
]);
assert.equal(live.length, 34);
for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries), banks = {};
  for (const [op, offset] of [['load', 0], ['store', 0x200]]) banks[op] = addresses.map((address, i) => row(op, address, PC + offset + i * 16));
  const faultBanks = {};
  for (const [op, offset] of [['load', 0x400], ['store', 0x500]]) faultBanks[op] = failureCases.map((name, i) => ({...row(op, failureAddress(name), PC + offset + i * 16 + 5), name}));
  const faultBlock = instruction => ({...block(instruction), pc: instruction.pc - 5, bytes: Buffer.concat([mov, block(instruction).bytes]), instructions: 3});
  const code = row('load', PC + 0x600, PC + 0x600), livePc = PC + 0x700, liveBlock = {pc: livePc, bytes: live, read: true, store: true, instructions: 8};
  for (const b of [...banks.load.map(block), ...banks.store.map(block), ...faultBanks.load.map(faultBlock), ...faultBanks.store.map(faultBlock), block(code), liveBlock]) upload(c, b.pc, b.bytes);
  host(c, 'protect', [PC, 1, 5]); let oldest;
  for (const op of ['load', 'store']) {
    const rows = banks[op], module = compile(c, rows.map(block)); if (op === 'load') oldest = module;
    for (const [i, instruction] of rows.entries()) for (const [j, value] of values.entries()) {
      const r = registers(); r[0] = ((0x8001 + i * 0x1357 + j) % 65536) * 65536 + (op === 'load' ? 0xff0f : value);
      const f = framed(c, instruction.address, op === 'load' ? value : 0x5aa5); seed(c, instruction.pc, r, [2, 0xcd7, 0x443, 0x892][(i + j) % 4]);
      const effects = operation(c, module, instruction, value, 'full32 absolute address ignores current AX and retains own high/FLAGS');
      const expected = Buffer.from(f.bytes); expected.set(word(effects.value), f.offset); checkRam(c, f.start, expected); counts.matrix++;
    }
    const selected = rows[1];
    for (const value of flags) {
      const r = registers(); r[0] = 0xcafebeef; const f = framed(c, selected.address, 0x8001); seed(c, selected.pc, r, value);
      const effects = operation(c, module, selected, 0x8001, 'all128 valid FLAGS remain exact for both moffs forms');
      const expected = Buffer.from(f.bytes); expected.set(word(effects.value), f.offset); checkRam(c, f.start, expected); counts.flag_inputs++;
    }
    const r = registers(); r[0] = 0xcafe1234; framed(c, selected.address, 0x1234); seed(c, selected.pc, r);
    operation(c, module, selected, 0x1234, 'first current moffs transfer'); if (op === 'load') upload(c, selected.address, word(0xbeef)); input(c, 48, words([selected.pc]));
    operation(c, module, selected, op === 'load' ? 0xbeef : 0x1234, 'PC-only rewind retains current parent; load uses declared changed RAM and store repeats identity'); counts.repeats += 2;
    seed(c, selected.pc, r); run(c, module, 0, cpu(c), {label: 'budget0 before moffs helper', retired: 0, helperVersion: null}); input(c, 96, words([1]));
    run(c, module, 1, cpu(c), {label: 'cancel before moffs helper', reason: 2, retired: 0, helperVersion: null}); run(c, module, 0, cpu(c), {label: 'cancel precedes budget0', reason: 2, retired: 0, helperVersion: null}); input(c, 96, words([0]));
    input(c, 48, words([selected.pc + 6])); run(c, module, 2, {...cpu(c), pc: selected.pc + 8}, {label: 'suffix retains current CPU with budget2', reason: 3, helperVersion: null}); run(c, module, 1, cpu(c), {label: 'cold lookup preserves all state', reason: 3, retired: 0, helperVersion: null}); counts.controls += 5;
  }
  for (const op of ['load', 'store']) {
    const rows = faultBanks[op], module = compile(c, rows.map(faultBlock)), access = op === 'load' ? 1 : 2;
    for (const instruction of rows) {
      const failure = failureSetup(c, instruction.name, access), r = registers(); r[0] = 0xcafebeef;
      let afterFault;
      if (instruction.name === 'overflow') {
        upload(c, 0xfffffffe, Buffer.from([0x55, 0xa5])); upload(c, 0, Buffer.from([0x5a, 0xaa]));
        afterFault = () => {checkRam(c, 0xfffffffe, Buffer.from([0x55, 0xa5])); checkRam(c, 0, Buffer.from([0x5a, 0xaa]));};
      }
      const stopped = stoppedFault(c, module, instruction, r, {detail: failure.detail, address: failure.faultAddress, access}, instruction.name + ' checked WORD span', afterFault);
      if (instruction.name !== 'overflow') {
        repair(c, failure); assert.deepEqual(cpu(c), stopped, 'map/protect-only repair retains stopped CPU/FLAGS/highAX');
        checkRam(c, failure.address, word(failure.original)); operation(c, module, instruction, failure.original, 'repair-only retry consumes original/zero-filled current WORD or current AX'); counts.retries++;
      }
      unmap(c, 0x9000); unmap(c, 0xa000);
    }
    const instruction = rows.find(instruction => instruction.name === 'wrong-permission'), r = registers(); r[0] = 0xcafebeef;
    map(c, 0x9000, 1, 3); framed(c, instruction.address, 0x5aa5); host(c, 'protect', [0x9000, 1, op === 'load' ? 2 : 1]);
    const stopped = stoppedFault(c, module, instruction, r, {detail: 2, address: instruction.address, access}, 'explicit-current witness starts with permission denial');
    host(c, 'protect', [0x9000, 1, 3]); const current = {...stopped, registers: [...stopped.registers], flags: 0x892}; current.registers[0] = op === 'load' ? 0xbaff7ffe : 0xbaff1234;
    if (op === 'load') upload(c, instruction.address, word(0x8001)); input(c, 0, state(current));
    operation(c, module, instruction, 0x8001, 'separate explicit current RAM/AX/high16/FLAGS retry at captured absolute address');
    checkRam(c, instruction.address - 1, Buffer.from([0x55, ...word(op === 'load' ? 0x8001 : 0x1234), 0xaa])); counts.retries++; counts.current_inputs++; unmap(c, 0x9000);
  }
  const codeModule = compile(c, [block(code)]), r = registers(); r[0] = 0xcafeff0f; seed(c, code.pc, r);
  operation(c, codeModule, code, 0xa166, 'own RX load reads opcode WORD without stores or invalidation'); input(c, 48, words([code.pc]));
  operation(c, codeModule, code, 0xa166, 'PC-only own RX load repeat remains current after AX changed');
  run(c, codeModule, 2, {...cpu(c), pc: code.pc + 8}, {label: 'RX read owner remains current through suffix', reason: 3, helperVersion: null}); counts.code_reads += 3; checkRam(c, code.pc, block(code).bytes);
  const mixed = compile(c, [liveBlock]), retained = banks.load[1]; upload(c, retained.address, word(0xbeef)); seed(c, retained.pc);
  if (owner === 'resident') operation(c, oldest, retained, 0xbeef, 'old resident moffs load remains current at six-unit capacity');
  else run(c, oldest, 1, cpu(c), {label: 'old replacement generation mismatch3 is neutral', status: 3, retired: 0}); counts.retained_units++;
  const initial = registers(); initial[0] = 0xcafeff0f; initial[1] = 0xface1234; initial[3] = 0xbeefffff; framed(c, DATA + 0x100, 0x7fff); framed(c, DATA + 0x102, 0x5aa5); upload(c, DATA + 0x100, word(0x7fff)); seed(c, livePc, initial);
  operation(c, mixed, row('load', DATA + 0x100, livePc), 0x7fff, 'live load returns WORD7FFF without reseed');
  let before = cpu(c), next = {...before, registers: [...before.registers], pc: livePc + 10, flags: 0xc96}; next.registers[0] = lowWord(before.registers[0], 0x8000);
  run(c, mixed, 1, next, {label: 'ADC0 consumes returned WORD and incoming CF1:8000/OF1', helperVersion: null});
  operation(c, mixed, row('store', DATA + 0x102, livePc + 10), undefined, 'live store preserves arithmetic flags and returns RAM8000');
  before = cpu(c); next = {...before, registers: [...before.registers], pc: livePc + 19}; next.registers[3] = lowWord(before.registers[3], 0xff01);
  run(c, mixed, 1, next, {label: 'SETO BL consumes OF1 after store', helperVersion: null});
  operation(c, mixed, row('load', DATA + 0x102, livePc + 19), 0x8000, 'live reload consumes actual stored WORD and retains OF');
  before = cpu(c); next = {...before, registers: [...before.registers], pc: livePc + 29, flags: 0x402}; next.registers[1] = lowWord(before.registers[1], 15);
  run(c, mixed, 1, next, {label: 'BSF consumes returned8000 index15; whole-WORD parity is odd', helperVersion: null});
  before = cpu(c); next = {...before, registers: [...before.registers], pc: livePc + 32}; next.registers[3] = lowWord(before.registers[3], 1);
  run(c, mixed, 1, next, {label: 'SETZ BH consumes nonzero scan ZF0', helperVersion: null});
  run(c, mixed, 2, {...cpu(c), pc: livePc + 34}, {label: 'live suffix reaches cold with budget2', reason: 3, helperVersion: null}); run(c, mixed, 1, cpu(c), {label: 'live cold lookup preserves returned CPU/FLAGS', reason: 3, retired: 0, helperVersion: null}); counts.consumers += 9;
  checkRam(c, DATA + 0x100, word(0x7fff)); checkRam(c, DATA + 0x102, word(0x8000)); checkRam(c, livePc, live); checkRam(c, 0x8000, canary); assert.equal(c.api.close(), 0);
}
for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) for (const changed of [false, true]) {
  const c = open(owner, entries), instruction = row('store', PC, PC); upload(c, PC, block(instruction).bytes); const module = compile(c, [block(instruction)]), r = registers(); r[0] = 0xcafe0000 + (changed ? 0xa367 : 0xa366); seed(c, PC, r);
  operation(c, module, instruction, undefined, 'changed/identity ownopcode store retires once with GPR/FLAGS unchanged beforeSMC', 6);
  run(c, module, 1, cpu(c), {label: 'code-invalidated owner4 is neutral before any fresh capture', status: 4, retired: 0});
  const suffix = {pc: PC + 6, bytes: Buffer.from([0xeb, 0]), read: false, store: false, instructions: 1}, tail = compile(c, [suffix]);
  run(c, tail, 2, {...cpu(c), pc: PC + 8}, {label: 'fresh pure suffix continues without store replay', reason: 3, helperVersion: null, exitVersion: 1});
  checkRam(c, PC, word(changed ? 0xa367 : 0xa366)); checkRam(c, 0x8000, canary); assert.equal(c.api.close(), 0); counts.self_code += 3;
}
assert.deepEqual(sources(), sourcePins);
assert.deepEqual(counts, {contexts: 12, runs: 1852, retired: 1742, matrix: 512, flag_inputs: 1024, repeats: 16, faults: 128, retries: 56, current_inputs: 8, consumers: 36, controls: 40, retained_units: 4, code_reads: 12, self_code: 24});
assert.equal(modules.length, 40);
const result = {status: 'ok', engine_sha256: engine.sha256, source_pins: sourcePins, counts, modules,
  limits: [
    'finite exact single66 A1/A3+moffs32 WORD transfers in four bound profiles; standalone and other prefix/address-size modes stay closed',
    'eight literal full32 absolute offsets including prefix-lookingF3F06766, zero/unaligned/cross/top; independent own-EAX-high/WORD/little-endian oracle and all128 valid FLAGS per form',
    'seven read/store fault classes with prior MOV retirement, unchanged retry and honest map-only zero/protect-only repair; eight explicit-current RAM/AX/high/FLAGS rows overlap retries',
    'live load/arithmetic/store/SETO/reload/BSF/SETZ consumes current RAM/CPU/flags without reseed; own RX code reads remain current and RAM neighbors/canaries remain exact',
    'changed/identity ownopcode stores retire once before6, neutral same-owner4, fresh pureV1suffix; old replacement generations separately reject3',
    'maxsix resident captures/context; coreblocks8/8/7/7/1/1, aggregate16/16/21/21/2/8 and SMC2/1;existing8blocks/64instructions/6captures asserted before compile',
    'complete4612/helper/dirtyFP preservation; finite private flat32 proof, no hardware/segmentation/address16/concurrency/exhaustiveISA/speed/PE/browser/SDK/game/hostedCI claim',
  ]};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2), {flag: 'wx'});
console.log(JSON.stringify({status: 'ok', ...counts, modules: modules.length, output}));
