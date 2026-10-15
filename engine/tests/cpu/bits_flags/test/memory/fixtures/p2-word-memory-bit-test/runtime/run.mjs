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
const engine = readEngine(enginePath), SIZE = ARENA_SIZE, PC = 0x1000, DATA = 0x5000;
const initial = readFileSync(join(output, 'initial-arena.bin'));
assert.equal(initial.length, SIZE);
assert.deepEqual(WebAssembly.Module.imports(engine.module), []);
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const paths = [
  'engine/src/abi/arena.rs', 'engine/src/abi/memory_helper.rs',
  'engine/src/cpu/x86/ir.rs', 'engine/src/cpu/x86/decode/lower.rs', 'engine/src/cpu/x86/decode/families.rs',
  'engine/src/cpu/x86/decode/operands.rs', 'engine/src/cpu/x86/decode/integer.rs', 'engine/src/cpu/x86/decode/profile.rs', 'engine/src/cpu/x86/decode/families.rs', 'engine/src/cpu/dbt/region.rs',
  'engine/src/cpu/dbt/wasm/instructions/integer/arithmetic.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/bits.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/carry_rotates.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/decimal.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/flags.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/mod.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/multiply_divide.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/operands.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/rotates.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/shifts.rs', 'engine/src/cpu/dbt/wasm/imports/mod.rs', 'engine/src/cpu/dbt/wasm/imports/binding.rs', 'engine/src/cpu/dbt/wasm/imports/context.rs', 'engine/src/cpu/dbt/wasm/imports/needs.rs', 'engine/src/cpu/dbt/wasm/helpers/protocol.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/mod.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/address.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/scalar.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/stack.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/word_store.rs',
  'engine/src/cpu/dbt/wasm/instructions/memory/narrow.rs', 'engine/src/memory/space.rs', 'engine/src/memory/space/atomic.rs',
  'engine/src/process/instance.rs', 'engine/src/process/execution/compiled.rs', 'engine/src/process/execution/authority.rs', 'engine/src/process/memory/address_space.rs', 'engine/src/process/memory/scalar.rs', 'engine/src/process/memory/scalar/tests.rs', 'engine/src/process/execution/mod.rs', 'engine/src/process/memory/mod.rs', 'engine/src/process/mod.rs', 'engine/src/process/execution/resident.rs', 'engine/src/process/execution/authority.rs', 'engine/src/process/execution/mod.rs', 'engine/src/process/mod.rs',
  'engine/tests/cpu/bits_flags/test/memory/native/word.rs', 'engine/tests/cpu/bits_flags/native.rs', 'engine/tests/support/cpu/prefix/admission.rs', 'engine/tests/support/cpu/prefix/word.rs', 'engine/tests/cpu/bits_flags/native.rs', 'engine/src/process/wasm/mod.rs', 'engine/src/process/wasm/loading.rs', 'engine/src/abi/image.rs', 'engine/src/process/wasm/execution.rs', 'engine/src/abi/execution.rs', 'engine/src/process/wasm/calls.rs', 'engine/src/process/wasm/memory.rs', 'engine/src/process/wasm/cpu.rs', 'engine/src/process/mod.rs',
  'engine/tests/cpu/bits_flags/test/memory/wasm/word.rs', 'engine/tests/cpu/bits_flags/wasm.rs', 'engine/tests/support/wasm.rs', 'engine/tests/cpu/bits_flags/wasm.rs', 'engine/tests/support/wasm.rs',
  'engine/tests/cpu/bits_flags/test/memory/fixtures/p2-word-memory-bit-test/runtime/run.mjs', 'engine/tests/support/wasm/binding.mjs', 'engine/tests/support/wasm/page-fault.mjs',
];
paths.push(...["engine/src/abi/memory_helper/cpu.rs", "engine/src/cpu/dbt/wasm/instructions/memory/cpu_scalar.rs", "engine/src/cpu/dbt/wasm/emitter/mod.rs", "engine/src/cpu/dbt/wasm/emitter/module.rs", "engine/src/cpu/dbt/wasm/emitter/body.rs", "engine/src/cpu/dbt/wasm/requirements.rs", "engine/src/cpu/dbt/wasm/imports/wiring.rs", "engine/src/cpu/dbt/wasm/code_fetch.rs", "engine/src/process/memory/cpu_scalar.rs", "engine/src/process/memory/scalar/cpu_write.rs", "engine/src/process/wasm/memory/cpu_write.rs", "engine/src/abi/wasm/exports/memory/cpu_write.rs"].filter(path=>!paths.includes(path)));
const sources = () => Object.fromEntries(paths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = sources(), modules = [];
const counts = {contexts: 0, runs: 0, retired: 0, matrix: 0, anchors: 0, addresses: 0, boundaries: 0, faults: 0, retries: 0, current_inputs: 0, controls: 0, code_reads: 0, self_code: 0, consumers: 0};
const word = value => Buffer.from([value & 255, value >>> 8 & 255]);
const words = values => {const b = Buffer.alloc(values.length * 4); values.forEach((v, i) => b.writeUInt32LE(v >>> 0, i * 4)); return b;};
function record(magic, size, fields, version = 1) {
  const b = Buffer.alloc(size); b.write(magic); b.writeUInt16LE(version, 4);
  b.writeUInt16LE(1, 6); b.writeUInt32LE(size, 8);
  fields.forEach((v, i) => b.writeUInt32LE(v >>> 0, 16 + i * 4)); return b;
}
const state = c => record('R3ST', 56, [...c.registers, c.pc, c.flags]);
const exit = (reason, n, f, version = 2) =>
  reason === 5
    ? pageFaultExit({
        retired: n,
        errorCode: 4 | ((f?.access ?? 0) === 2 ? 2 : 0) | ((f?.detail ?? 0) === 2 ? 1 : 0),
        address: f?.address ?? 0,
        access: f?.access ?? 0,
        length: f ? 2 : 0
      })
    : record(
        'R3EX',
        40,
        [reason, n, f?.detail ?? 0, f?.address ?? 0, f?.access ?? 0, f ? 2 : 0],
        reason === 11 ? 6 : version
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
    if (helperVersion !== null) expected.set(helper(helperValue, fault), 100);
  }
  assert.equal(module.run(c.base, c.base + 56, budget, c.base + 96), status, label);
  assert.deepEqual(arena(c), expected, 'complete arena CPU/exit/helper/FP128 effect: ' + label);
  counts.runs++; if (status === 0) counts.retired += retired;
}
function checkRam(c, address, expected) {
  for (let i = 0; i < expected.length; i++) {
    const wanted = arena(c); wanted.set(record('R3MH', 40, [0, expected[i], 0, 0, 0, 1], 2), 100);
    assert.equal(c.api.read8((address + i) >>> 0), 0);
    assert.deepEqual(arena(c), wanted, 'real RAM byte and arena at ' + (address + i).toString(16));
  }
}

function compile(c, blocks, codePermissions) {
  for (const block of blocks) upload(c, block.pc, block.bytes);
  if (codePermissions !== undefined) host(c, 'protect', [PC, 1, codePermissions]);
  input(c, 140, words(blocks.flatMap(b => c.entries ? [b.pc] : [b.pc, b.bytes.length])));
  const method = c.owner === 'resident' ? (c.entries ? 'compile_resident_entries' : 'compile_resident') : (c.entries ? 'compile_entries' : 'compile');
  host(c, method, c.entries ? [blocks.length, 0] : [blocks.length]);
  const b = arena(c), binding = c.owner === 'resident'
    ? {low: b.readUInt32LE(148), high: b.readUInt32LE(152), pointer: b.readUInt32LE(156), length: b.readUInt32LE(160)}
    : {generation: c.api.generation(), pointer: c.api.module_ptr() >>> 0, length: c.api.module_len() >>> 0};
  const bytes = Buffer.from(new Uint8Array(c.memory.buffer, binding.pointer, binding.length));
  const module = new WebAssembly.Module(bytes), imports = WebAssembly.Module.imports(module);
  const memoryOps = blocks.some(b => b.memory !== false);
  const names = generatedImports({resident: c.owner === 'resident', helpers: [...(memoryOps ? ['cpu_read16'] : []), ...(blocks.some(b => b.store) ? ['cpu_read_modify16', c.owner === 'resident' ? 'cpu_store_resident16' : 'cpu_store16'] : [])]}).slice(1).map(item => item.name);
  assert.deepEqual(imports, [{module: 'env', name: 'memory', kind: 'memory'}, ...names.map(name => ({module: 'ring3', name, kind: 'function'}))]);
  assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  const file = `module-${modules.length + 1}.wasm`; writeFileSync(join(output, file), bytes, {flag: 'wx'});
  modules.push({context: c.id, owner: c.owner, entries: c.entries, file, sha256: hash(bytes), imports: names});
  return {run: new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3: c.api}).exports.run};
}
const block = (pc, instruction, kind) => ({pc, memory: true, store: kind !== 'bt', bytes: Buffer.concat([instruction, Buffer.from([0xeb, 0])])});
function open(owner, entries) {
  const instance = new WebAssembly.Instance(engine.module, {});
  const c = {id: ++counts.contexts, owner, entries, memory: instance.exports.memory, api: {}};
  for (const name of ['open', 'close', 'arena_ptr', 'map', 'unmap', 'protect', 'upload', 'read8', 'read16', 'store16', 'store_resident16',
    'compile', 'compile_entries', 'compile_resident', 'compile_resident_entries', 'generation', 'module_ptr', 'module_len', 'guard', 'guard_resident'])
    c.api[name] = instance.exports['ring3_abi_v1_' + name];
 attachDetectionApi(c.api, instance.exports);
  for(const name of ["read_modify16", "read16", "read8", "store16", "store_resident16"]) {c.api["cpu_"+name]=instance.exports["ring3_abi_v1_cpu_"+name];assert.equal(c.api["cpu_"+name]?.length,name.startsWith("read")?1:name.includes("resident")?6:2,"cpu_"+name);}
 assert.equal(c.api.open(8, c.id, 0x574d4254), 0); c.base = c.api.arena_ptr() >>> 0; input(c, 0, initial);
  for (const args of [[PC, 1, 7], [DATA, 3, 3], [0xa53c5000, 1, 3], [0xfffff000, 1, 3]]) host(c, 'map', args);
  return c;
}

// arithmetic digit extraction/mutation and signed floor addressing avoid emitter shifts.
function changed(kind, value, raw, flags) {
  const bit = raw % 16, mask = 2 ** bit, oldBit = Math.floor(value / mask) % 2;
  const delta = kind === 'bts' ? 1 - oldBit : kind === 'btr' ? -oldBit : kind === 'btc' ? 1 - 2 * oldBit : 0;
  return {value: value + delta * mask, flags: (flags & 0x442) | oldBit};
}
function selected(base, raw, immediate = false) {
  const low = raw % 65536, signed = low >= 32768 ? low - 65536 : low;
  return Number(BigInt.asUintN(32, BigInt(base) + BigInt(immediate ? 0 : 2 * Math.floor(signed / 16))));
}
const BASE = DATA + 0x1002;
for (const [raw, address, bit] of [[0xffff, BASE - 2, 15], [0xfff0, BASE - 2, 0], [0xffef, BASE - 4, 15], [0x8000, BASE - 4096, 0], [0x7fff, BASE + 4094, 15]]) {
  assert.equal(selected(BASE, 0xcafe0000 + raw), address); assert.equal(raw % 16, bit);
}
assert.equal(selected(0, 0xffff), 0xfffffffe);
assert.equal(selected(BASE, 255, true), BASE);
const kinds = [
  {name: 'bt', opcode: 0xa3, extension: 4}, {name: 'bts', opcode: 0xab, extension: 5},
  {name: 'btr', opcode: 0xb3, extension: 6}, {name: 'btc', opcode: 0xbb, extension: 7},
];
const lowWord = (parent, value) => Math.floor(parent / 65536) * 65536 + value;
function instruction(kind, index, tail, immediate = false) {
  return Buffer.from([0x66, 0x0f, immediate ? 0xba : kind.opcode,
    tail[0] | (immediate ? kind.extension : index) << 3, ...tail.slice(1), ...(immediate ? [index] : [])]);
}
function spec(kind, index, base = BASE, immediate = false, tail = [5, ...words([base])]) {
  return {kind, index, base, immediate, bytes: instruction(kind, index, tail, immediate)};
}
function operation(c, module, row, value, label, reason = 1) {
  const before = cpu(c), raw = row.immediate ? row.index : before.registers[row.index];
  const address = selected(row.base, raw, row.immediate), expected = changed(row.kind.name, value, raw, before.flags);
  run(c, module, 1, {...before, pc: before.pc + row.bytes.length, flags: expected.flags},
    {label, reason, helperVersion: 10, helperValue: row.kind.name === 'bt' ? value : 0});
  checkRam(c, address, word(expected.value)); return expected;
}
function flagsFor(value, raw, index) {
  const oldBit = Math.floor(value / (2 ** (raw % 16))) % 2;
  return [0xcd6, 2, 0x42, 0x402][index % 4] | (1 - oldBit);
}
const anchorInputs = [
  {value: 0, raw: 0xffff, flags: 0xcd7, nextFlags: 0x442},
  {value: 0x8000, raw: 0xffff, flags: 2, nextFlags: 3},
  {value: 0xffff, raw: 0xfff0, flags: 0x42, nextFlags: 0x43},
  {value: 15, raw: 15, flags: 0x803, nextFlags: 2},
];
const anchorValues = {bt: [0, 0x8000, 0xffff, 15], bts: [0x8000, 0x8000, 0xffff, 0x800f], btr: [0, 0, 0xfffe, 15], btc: [0x8000, 0, 0xfffe, 0x800f]};
for (const kind of kinds) for (const [i, a] of anchorInputs.entries())
  assert.deepEqual(changed(kind.name, a.value, a.raw, a.flags), {value: anchorValues[kind.name][i], flags: a.nextFlags}, 'literal old-bit/retained-ZF/DF/identity flags');

for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) for (let group = 0; group < 4; group += 2) {
  const c = open(owner, entries);
  for (const kind of kinds.slice(group, group + 2)) {
    const rawIndexes = [0xffff, 0xfff0, 0xffef, 0x8000, 0x7fff, 0, 16, 15];
    const rows = rawIndexes.map((raw, index) => ({...spec(kind, index), raw}));
    const module = compile(c, rows.map((row, i) => block(PC + i * 16, row.bytes, kind.name)));
    for (const [i, row] of rows.entries()) for (const oldBit of [0, 1]) {
      const r = registers(); r[row.index] = 0xcafe0000 + row.raw;
      const address = selected(BASE, r[row.index]), value = oldBit * 2 ** (row.raw % 16);
      upload(c, address - 1, Buffer.concat([Buffer.from([0x55]), word(value), Buffer.from([0xaa])]));
      if (kind.name === 'bt') host(c, 'protect', [DATA, 3, 1]);
      seed(c, PC + i * 16, r, flagsFor(value, row.raw, i + oldBit));
      const result = operation(c, module, row, value, 'all eight original signed16 index parents and literal negative/positive endpoints');
      checkRam(c, address - 1, Buffer.concat([Buffer.from([0x55]), word(result.value), Buffer.from([0xaa])]));
      if (kind.name === 'bt') host(c, 'protect', [DATA, 3, 3]); counts.matrix++;
    }
    const row = rows[1];
    for (let bit = 0; bit < 16; bit++) for (const oldBit of [0, 1]) {
      const r = registers(); r[row.index] = 0xbeef0000 + bit; const value = oldBit * 2 ** bit;
      upload(c, BASE, word(value)); seed(c, PC + 16, r, flagsFor(value, bit, bit + oldBit));
      operation(c, module, row, value, 'every selected WORD bit with opposite incoming CF'); counts.matrix++;
    }
    for (const [i, a] of anchorInputs.entries()) {
      const r = registers(); r[row.index] = 0xbeef0000 + a.raw; upload(c, selected(BASE, a.raw), word(a.value)); seed(c, PC + 16, r, a.flags);
      assert.deepEqual(operation(c, module, row, a.value, 'literal flags/value and signed-index old selected bit'), {value: anchorValues[kind.name][i], flags: a.nextFlags}); counts.anchors++;
    }
    const immediateRows = [0, 15, 16, 31, 32, 255, 0x66, 0x67, 0xf0, 0xf2, 0xf3].map(raw => spec(kind, raw, BASE, true));
    for (let start = 0; start < immediateRows.length; start += 8) {
      const selectedRows = immediateRows.slice(start, start + 8), current = compile(c, selectedRows.map((row, i) => block(PC + 0x100 + i * 16, row.bytes, kind.name)));
      for (const [i, row] of selectedRows.entries()) for (const oldBit of [0, 1]) {
        const value = oldBit * 2 ** (row.index % 16), r = registers(); r[1] = 0x8000ffff;
        upload(c, BASE - 1, Buffer.concat([Buffer.from([0x55]), word(value), Buffer.from([0xaa])]));
        seed(c, PC + 0x100 + i * 16, r, flagsFor(value, row.index, start + i + oldBit));
        const result = operation(c, current, row, value, 'raw immediate low4 has no signed memory offset, including prefix-looking payload');
        checkRam(c, BASE - 1, Buffer.concat([Buffer.from([0x55]), word(result.value), Buffer.from([0xaa])])); counts.matrix++;
      }
    }
  }
  assert.equal(c.api.close(), 0);
}

for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) for (let group = 0; group < 4; group += 2) {
  const c = open(owner, entries);
  for (const kind of kinds.slice(group, group + 2)) {
    const rows = Array.from({length: 8}, (_, carrier) => {
      const r = registers(); r[carrier] = 0xa53c5008;
      const tail = carrier === 4 ? [4, 0x24] : carrier === 5 ? [0x45, 0] : [carrier];
      return {...spec(kind, carrier, 0xa53c5008, false, tail), r};
    });
    const module = compile(c, rows.map((row, i) => block(PC + i * 16, row.bytes, kind.name)));
    assert.equal(selected(0xa53c5008, 0xa53c5008), 0xa53c5a08);
    for (const [i, row] of rows.entries()) for (const value of [0xa55a, 0x5aa5]) {
      const address = 0xa53c5a08; upload(c, address - 1, Buffer.concat([Buffer.from([0x55]), word(value), Buffer.from([0xaa])]));
      seed(c, PC + i * 16, row.r, flagsFor(value, row.r[i], i)); const result = operation(c, module, row, value, 'same original full32 parent supplies ordinary EA and signed16 bit-string offset');
      checkRam(c, address - 1, Buffer.concat([Buffer.from([0x55]), word(result.value), Buffer.from([0xaa])])); counts.addresses++;
    }
    const scaled = [
      {tail: [0x44, 0x8d, 0xfc], values: {1: 0x40000002, 5: 0xa53c5004}, index: 1},
      {tail: [4, 0x85, ...words([0xa53c5000])], values: {0: 0x40000002}, index: 0},
      {tail: [0x84, 0x80, ...words([0x053c5003])], values: {0: 0x20000001}, index: 0},
      {tail: [0x44, 0x24, 0xff], values: {4: 0xa53c5009}, index: 4},
    ].map(row => {
      const r = registers(); for (const [i, value] of Object.entries(row.values)) r[i] = value;
      return {...spec(kind, row.index, 0xa53c5008, false, row.tail), r};
    });
    const current = compile(c, scaled.map((row, i) => block(PC + 0x100 + i * 16, row.bytes, kind.name)));
    for (const [i, row] of scaled.entries()) for (const value of [0xa55a, 0x5aa5]) {
      const address = selected(row.base, row.r[row.index]); upload(c, address, word(value)); seed(c, PC + 0x100 + i * 16, row.r);
      operation(c, current, row, value, 'scaled/index/base/both/wrapped EA uses original full parents before WORD offset'); counts.addresses++;
    }
    const boundaries = [spec(kind, 1, DATA + 4095), spec(kind, 255, 0xfffffffe, true), spec(kind, 1, 0)];
    const bounded = compile(c, boundaries.map((row, i) => block(PC + 0x200 + i * 16, row.bytes, kind.name)));
    for (const [i, row] of boundaries.entries()) for (const value of [0, 0x8000]) {
      const r = registers(); r[1] = i === 2 ? 0xbeefffff : 0xbeef000f;
      const address = selected(row.base, row.immediate ? row.index : r[1], row.immediate);
      assert.equal(address, i === 0 ? DATA + 4095 : 0xfffffffe); upload(c, address, word(value));
      seed(c, PC + 0x200 + i * 16, r); operation(c, bounded, row, value, 'crosspage/last valid WORD span and negative-offset flat32 wrap'); counts.boundaries++;
    }
  }
  assert.equal(c.api.close(), 0);
}

for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries); host(c, 'map', [0x8000, 1, 3]); host(c, 'map', [0x9000, 1, 3]);
  for (const kind of kinds) {
    const row = spec(kind, 1, 0, false, [7]), body = Buffer.concat([Buffer.from([0xbb, 0x44, 0x33, 0x22, 0x11]), row.bytes]);
    const module = compile(c, [block(PC, body, kind.name)]);
    const cases = [
      {name: 'unmapped selected WORD', address: 0x9000, detail: 1, page: 0x9000, raw: 0xffff},
      {name: 'no read permission', address: 0x9000, detail: 2, page: 0x9000, bits: 0},
      {name: 'write-only selected WORD', address: 0x9000, detail: 2, page: 0x9000, bits: 2},
      {name: 'execute-only selected WORD', address: 0x9000, detail: 2, page: 0x9000, bits: 4},
      {name: 'crosspage second byte unmapped', address: 0x8fff, detail: 1, page: 0x9000, raw: 0xffff},
      {name: 'crosspage second byte write-only', address: 0x8fff, detail: 2, page: 0x9000, bits: 2},
      {name: 'selected WORD span overflow', address: 0xffffffff, detail: 3},
      ...(kind.name === 'bt' ? [] : [
        {name: 'readable identity/changed write denied', address: 0x9000, detail: 2, page: 0x9000, bits: 1, write: true},
        {name: 'crosspage readable write denied', address: 0x8fff, detail: 2, page: 0x9000, bits: 1, write: true},
      ]),
    ];
    seed(c, PC);
    run(c, module, 0, cpu(c), {label: 'zero budget before memory target', retired: 0, helperVersion: null});
    input(c, 96, words([1])); run(c, module, 1, cpu(c), {label: 'cancel before memory target', reason: 2, retired: 0, helperVersion: null}); input(c, 96, words([0])); counts.controls += 2;
    for (const f of cases) {
      host(c, 'protect', [0x8000, 2, 3]);
      const a = f.write ? kind.name === 'bts' ? 0x8000 : 0 : 0x800f;
      const original = Buffer.concat([Buffer.from([0x55]), word(a), Buffer.from([0xaa])]);
      if (f.detail === 3) upload(c, 0xfffffffe, Buffer.from([0x55, 0x0f]));
      else {upload(c, f.address - 1, original); host(c, f.detail === 1 ? 'unmap' : 'protect', [f.page, 1, ...(f.detail === 1 ? [] : [f.bits])]);}
      const raw = f.raw ?? 15, r = registers(), offset = 2 * Math.floor((raw >= 32768 ? raw - 65536 : raw) / 16);
      r[1] = 0xcafe0000 + raw; r[7] = (f.address - offset) >>> 0;
      const stopped = {...seed(c, PC, r, flagsFor(a, raw, 0)), registers: [...r], pc: PC + 5}; stopped.registers[3] = 0x11223344;
      // Ring3 rejects the overflowing logical WORD span before RAM access.
      const segmentOverflow = f.detail === 3;
      const fault = segmentOverflow ? undefined : {detail: f.detail, address: f.address === 0x8fff ? 0x9000 : f.address, access: kind.name === 'bt' ? 1 : 2};
      run(c, module, 9, stopped, {label: f.name + ' after MOV prefix retirement', reason: segmentOverflow ? 11 : 5, retired: 1, fault, helperVersion: segmentOverflow ? null : 10});
      run(c, module, 9, stopped, {label: f.name + ' unchanged stopped retry', reason: segmentOverflow ? 11 : 5, retired: 0, fault, helperVersion: segmentOverflow ? null : 10}); counts.faults += 2;
      checkRam(c, f.detail === 3 ? 0xfffffffe : f.address - 1, Buffer.from([0x55]));
      if (f.detail !== 3) {
        host(c, f.detail === 1 ? 'map' : 'protect', [f.page, 1, 3]);
        const repaired = f.detail === 1 ? f.address === 0x8fff ? a % 256 : 0 : a;
        const expectedBytes = f.detail === 1 ? Buffer.from([0x55, repaired & 255, repaired >>> 8, 0]) : original;
        checkRam(c, f.address - 1, expectedBytes);
        assert.deepEqual(cpu(c), stopped, 'map/protect-only repair retains stopped CPU/FLAGS');
        operation(c, module, {...row, base: r[7]}, repaired, 'same stopped RMW consumes mapped/current RAM after permission-only repair'); counts.retries++;
      } else checkRam(c, 0xfffffffe, Buffer.from([0x55, 0x0f]));
    }
    host(c, 'protect', [DATA, 3, 3]); upload(c, DATA + 0x40, word(0x8000)); host(c, 'protect', [DATA, 1, 2]);
    const r = registers(); r[1] = 0xbeef000f; r[7] = DATA + 0x40;
    const stopped = {...seed(c, PC, r), registers: [...r], pc: PC + 5}; stopped.registers[3] = 0x11223344;
    const fault = {detail: 2, address: DATA + 0x40, access: kind.name === 'bt' ? 1 : 2};
    run(c, module, 9, stopped, {label: 'current-input witness first read fault', reason: 5, retired: 1, fault, helperVersion: 10});
    run(c, module, 9, stopped, {label: 'current-input witness unchanged read fault', reason: 5, retired: 0, fault, helperVersion: 10}); counts.faults += 2;
    host(c, 'protect', [DATA, 1, 3]); upload(c, DATA + 0x7f, Buffer.from([0x55, 1, 0x80, 0xaa]));
    const current = cpu(c); current.registers[1] = 0xfacefff0; current.registers[7] = DATA + 0x82; current.flags = 0x8d6; input(c, 0, state(current));
    assert.equal(selected(current.registers[7], current.registers[1]), DATA + 0x80);
    assert.deepEqual(operation(c, module, {...row, base: current.registers[7]}, 0x8001, 'explicit current WORD/index/EA/FLAGS edits are separately declared'),
      {value: kind.name === 'btr' || kind.name === 'btc' ? 0x8000 : 0x8001, flags: 0x43});
    counts.current_inputs++; counts.retries++;
  }

  const address = BASE, high = DATA + 8;
  const code = Buffer.concat([
    instruction(kinds[1], 1, [7]), Buffer.from([0x0f, 0x92, 0xc2, 0x0f, 0x94, 0xc3]),
    instruction(kinds[0], 1, [7]), Buffer.from([0x0f, 0x92, 0xc6, 0x0f, 0x94, 0xc7]),
    Buffer.from([0x66, 0x83, 0x15]), words([high]), Buffer.from([0]),
    instruction(kinds[2], 1, [7]), instruction(kinds[3], 0, [7], true),
    Buffer.from([0x0f, 0x92, 0xc2, 0x0f, 0x94, 0xc3]), instruction(kinds[0], 0, [7], true),
    Buffer.from([0x66, 0x83, 0x15]), words([high]), Buffer.from([0]),
    instruction(kinds[0], 1, [7]), Buffer.from([0x0f, 0x92, 0xc6, 0x0f, 0x94, 0xc7, 0xeb, 0]),
  ]);
  const live = compile(c, [{pc: PC + 0x200, bytes: code, memory: true, store: true}]);
  upload(c, address, word(0)); upload(c, high, word(0xffff)); const r = registers(); r[1] = 0xbeef000f; r[7] = address; seed(c, PC + 0x200, r);
  function step(length, flags, changes = {}, helperVersion = null, helperValue = 0, budget = 1, retired = 1) {
    const before = cpu(c), next = {...before, registers: [...before.registers], pc: before.pc + length, flags};
    for (const [i, value] of Object.entries(changes)) next.registers[i] = lowWord(before.registers[i], value);
    run(c, live, budget, next, {label: 'live returned RAM/CF/ZF consumer without CPU reseed', helperVersion, helperValue, retired});
  }
  step(4, 0x442, {}, 10); checkRam(c, address, word(0x8000));
  step(6, 0x442, {2: 0x1200, 3: 0x1201}, null, 0, 2, 2);
  step(4, 0x443, {}, 10, 0x8000); step(6, 0x443, {2: 0x0100, 3: 0x0101}, null, 0, 2, 2);
  step(8, 0x457, {}, 10); checkRam(c, high, word(0));
  step(4, 0x443, {}, 10); checkRam(c, address, word(0));
  step(5, 0x442, {}, 10); checkRam(c, address, word(1)); step(6, 0x442, {}, null, 0, 2, 2);
  step(5, 0x443, {}, 10, 1); step(8, 0x402, {}, 10); checkRam(c, high, word(1));
  step(4, 0x402, {}, 10, 1); step(6, 0x402, {2: 0, 3: 1}, null, 0, 2, 2);
  run(c, live, 2, {...cpu(c), pc: PC + 0x200 + code.length}, {label: 'live suffix cold lookup retains RAM/CPU result', reason: 3, helperVersion: null});
  run(c, live, 1, cpu(c), {label: 'live cold lookup has no target replay', reason: 3, retired: 0, helperVersion: null}); counts.consumers += 14;
  checkRam(c, address, word(1)); checkRam(c, high, word(1)); assert.equal(c.api.close(), 0);
}

for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries), row = spec(kinds[0], 1, PC, false, [7]);
  const module = compile(c, [block(PC, row.bytes, 'bt')], 5), r = registers(); r[1] = 0xbeef000f; r[7] = PC; seed(c, PC, r);
  operation(c, module, row, 0x0f66, 'readonly RX own-code BT never stores or invalidates');
  input(c, 0, state({...cpu(c), pc: PC})); operation(c, module, row, 0x0f66, 'same owner remains current after repeated RX code read');
  run(c, module, 2, {...cpu(c), pc: PC + row.bytes.length + 2}, {label: 'same current BT unit suffix cold lookup', reason: 3, helperVersion: null});
  checkRam(c, PC, block(PC, row.bytes, 'bt').bytes); assert.equal(c.api.close(), 0); counts.code_reads += 3;
}

const smc = [
  {kind: kinds[1], raw: 0, same: false}, {kind: kinds[1], raw: 1, same: true},
  {kind: kinds[2], raw: 1, same: false}, {kind: kinds[2], raw: 0, same: true},
  {kind: kinds[3], raw: 0, same: false}, {kind: kinds[3], raw: 9, same: false, ownImmediate: true},
];
for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) for (const s of smc) {
  const c = open(owner, entries), address = s.ownImmediate ? PC + 3 : PC;
  const row = spec(s.kind, s.ownImmediate ? s.raw : 1, address, Boolean(s.ownImmediate), [7]);
  const module = compile(c, [block(PC, row.bytes, s.kind.name)]), value = row.bytes.readUInt16LE(address - PC), r = registers(); r[1] = s.raw; r[7] = address;
  seed(c, PC, r, flagsFor(value, s.raw, 0)); const expected = operation(c, module, row, value, 'consumed-code checked write publishes flags/EIP/one retirement before SMC6', 6);
  assert.equal(expected.value === value, s.same);
  if (s.ownImmediate) {assert.equal(value, 0x093f); assert.equal(expected.value, 0x0b3f); assert.equal(expected.flags, 0x442);}
  run(c, module, 1, cpu(c), {label: 'consumed code invalidation status4 is completely arena neutral', status: 4, retired: 0, helperVersion: null});
  const nextPC = PC + row.bytes.length, tail = compile(c, [{pc: nextPC, bytes: Buffer.from([0xeb, 0]), memory: false, store: false}]);
  run(c, tail, 2, {...cpu(c), pc: nextPC + 2}, {label: 'fresh guard-only suffix has no mutation/head replay', reason: 3, helperVersion: null, exitVersion: 1});
  checkRam(c, address, word(expected.value)); assert.equal(c.api.close(), 0); counts.self_code += 3;
}
assert.deepEqual(sources(), sourcePins);
assert.deepEqual(counts, {contexts: 48, runs: 2276, retired: 2080, matrix: 1120, anchors: 64, addresses: 384, boundaries: 96,
  faults: 304, retries: 136, current_inputs: 16, controls: 32, code_reads: 12, self_code: 72, consumers: 56});
assert.equal(modules.length, 168);
const result = {status: 'ok', engine_sha256: engine.sha256, source_pins: sourcePins, counts, modules,
  limits: [
    'finite exact single66 WORD memory BT/BTS/BTR/BTC register-index and imm8 in four bound profiles; standalone memory remains closed',
    'arithmetic bit oracle and signed16 floor-address model; original full32 EA/index aliases, immediate low4 no offset, selected WORD span checks',
    'real read16/store16 RAM and complete4612 CPU/FLAGS/exit/helper/FP checks; no arbitrary-host rollback claim',
    'map/protect-only retry models zeroed remapped bytes; explicit new WORD/index/EA/FLAGS input witness is separate',
    'existing FLAGS clear policy is Ring3 policy; BT never writes, identity BTS/BTR still checked-store and SMC, consumed code status4 differs from generation status3',
    'own immediate BTC093f→0b3f keeps original bit9, fresh guard-only V1 suffix has no head replay; other memory units use existing V2 helpers/exits',
    'finite engine proof only; no exhaustiveISA/hardwareundefined/LOCKatomicity/speed/OSexception/PE/browser/SDK/game/hostedCI claim',
  ]};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2), {flag: 'wx'});
console.log(JSON.stringify({status: 'ok', ...counts, modules: modules.length, output}));

