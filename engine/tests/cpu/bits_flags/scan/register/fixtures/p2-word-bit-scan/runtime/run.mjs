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
  'engine/tests/cpu/bits_flags/scan/register/native/word.rs', 'engine/tests/cpu/bits_flags/native.rs', 'engine/tests/support/cpu/prefix/admission.rs', 'engine/tests/support/cpu/prefix/word.rs', 'engine/tests/cpu/bits_flags/native.rs', 'engine/src/cpu/x86/decode/integer.rs', 'engine/src/cpu/x86/decode/profile.rs', 'engine/src/cpu/x86/decode/families.rs', 'engine/src/cpu/dbt/region.rs',
  'engine/src/cpu/dbt/wasm/instructions/integer/arithmetic.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/bits.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/carry_rotates.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/decimal.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/flags.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/mod.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/multiply_divide.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/operands.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/rotates.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/shifts.rs', 'engine/src/cpu/dbt/wasm/imports/mod.rs', 'engine/src/cpu/dbt/wasm/imports/binding.rs', 'engine/src/cpu/dbt/wasm/imports/context.rs', 'engine/src/cpu/dbt/wasm/imports/needs.rs', 'engine/src/cpu/dbt/wasm/helpers/protocol.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/mod.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/address.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/scalar.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/stack.rs', 'engine/src/cpu/dbt/wasm/instructions/control.rs',
  'engine/src/cpu/dbt/wasm/instructions/memory/narrow.rs', 'engine/src/memory/space.rs', 'engine/src/memory/space/atomic.rs',
  'engine/src/process/instance.rs', 'engine/src/process/execution/compiled.rs', 'engine/src/process/execution/authority.rs', 'engine/src/process/memory/address_space.rs', 'engine/src/process/memory/scalar.rs', 'engine/src/process/memory/scalar/tests.rs', 'engine/src/process/execution/mod.rs', 'engine/src/process/memory/mod.rs', 'engine/src/process/mod.rs', 'engine/src/process/execution/resident.rs', 'engine/src/process/execution/authority.rs', 'engine/src/process/execution/mod.rs', 'engine/src/process/mod.rs',
  'engine/tests/cpu/bits_flags/scan/register/wasm/word.rs', 'engine/tests/cpu/bits_flags/wasm.rs', 'engine/tests/support/wasm.rs', 'engine/tests/cpu/bits_flags/wasm.rs', 'engine/tests/support/wasm.rs',
  'engine/tests/cpu/bits_flags/scan/register/fixtures/p2-word-bit-scan/runtime/run.mjs', 'engine/tests/support/wasm/binding.mjs', 'engine/tests/support/wasm/page-fault.mjs',
];
paths.push(...["engine/src/abi/memory_helper/cpu.rs", "engine/src/cpu/dbt/wasm/instructions/memory/cpu_scalar.rs", "engine/src/cpu/dbt/wasm/emitter/mod.rs", "engine/src/cpu/dbt/wasm/emitter/module.rs", "engine/src/cpu/dbt/wasm/emitter/body.rs", "engine/src/cpu/dbt/wasm/requirements.rs", "engine/src/cpu/dbt/wasm/imports/wiring.rs", "engine/src/cpu/dbt/wasm/code_fetch.rs", "engine/src/process/memory/cpu_scalar.rs", "engine/src/process/memory/scalar/cpu_write.rs", "engine/src/process/wasm/memory/cpu_write.rs", "engine/src/abi/wasm/exports/memory/cpu_write.rs"].filter(path=>!paths.includes(path)));
const sources = () => Object.fromEntries(paths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = sources(), modules = [];
const counts = {contexts: 0, runs: 0, retired: 0, matrix: 0, anchors: 0, addresses: 0, boundaries: 0, faults: 0, retries: 0, current_inputs: 0, consumers: 0, controls: 0, code_reads: 0};
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
function run(c, module, budget, next, {label, reason = 1, retired = 1, fault, helperVersion = null, helperValue = 0, status = 0, exitVersion = 6} = {}) {
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
  return {memoryOps, run: new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3: c.api}).exports.run};
}
const block = (pc, instruction, memory = true, store = false) => ({pc, memory, store, bytes: Buffer.concat([instruction, Buffer.from([0xeb, 0])])});
function open(owner, entries) {
  const instance = new WebAssembly.Instance(engine.module, {});
  const c = {id: ++counts.contexts, owner, entries, memory: instance.exports.memory, api: {}};
  for (const name of ['open', 'close', 'arena_ptr', 'map', 'unmap', 'protect', 'upload', 'read8', 'read16', 'store16', 'store_resident16',
    'compile', 'compile_entries', 'compile_resident', 'compile_resident_entries', 'generation', 'module_ptr', 'module_len', 'guard', 'guard_resident'])
    c.api[name] = instance.exports['ring3_abi_v1_' + name];
 attachDetectionApi(c.api, instance.exports);
  for(const name of ["read_modify16", "read16", "read8", "store16", "store_resident16"]) {c.api["cpu_"+name]=instance.exports["ring3_abi_v1_cpu_"+name];assert.equal(c.api["cpu_"+name]?.length,name.startsWith("read")?1:name.includes("resident")?6:2,"cpu_"+name);}
 assert.equal(c.api.open(8, c.id, 0x57425343), 0); c.base = c.api.arena_ptr() >>> 0; input(c, 0, initial);
  for (const args of [[PC, 1, 7], [DATA, 2, 3], [0xa53c5000, 1, 3], [0xfffff000, 1, 3]]) host(c, 'map', args);
  return c;
}

// binary-string position and whole-WORD parity are independent of Wasm ctz/clz/popcnt.
function scanned(kind, value, oldFlags) {
  const bits = value.toString(2).padStart(16, '0'), ones = bits.replaceAll('0', '').length;
  return {index: value === 0 ? null : 15 - (kind === 'forward' ? bits.lastIndexOf('1') : bits.indexOf('1')),
    flags: 2 + (oldFlags & 0x400) + Number(value === 0) * 0x40 + Number(ones % 2 === 0) * 4};
}
const anchors = [
  {value: 0, forward: null, reverse: null, flags: 0x446},
  {value: 1, forward: 0, reverse: 0, flags: 0x402},
  {value: 0x0100, forward: 8, reverse: 8, flags: 0x402},
  {value: 0x0101, forward: 0, reverse: 8, flags: 0x406},
  {value: 0x8000, forward: 15, reverse: 15, flags: 0x402},
  {value: 0x8001, forward: 0, reverse: 15, flags: 0x406},
  {value: 0xffff, forward: 0, reverse: 15, flags: 0x406},
];
for (const row of anchors) for (const kind of ['forward', 'reverse']) assert.deepEqual(scanned(kind, row.value, 0xcd7),
  {index: row[kind], flags: row.flags}, 'literal WORD index/whole-source parity/zero policy anchor');
const lowWord = (parent, value) => Math.floor(parent / 65536) * 65536 + value;
const opcode = kind => kind === 'forward' ? 0xbc : 0xbd;
const registerInstruction = (kind, destination, source) => Buffer.from([0x66, 0x0f, opcode(kind), 0xc0 | destination << 3 | source]);
const memoryInstruction = (kind, destination, tail) => Buffer.from([0x66, 0x0f, opcode(kind), tail[0] | destination << 3, ...tail.slice(1)]);
function memorySpec(kind, destination, address = DATA + 8) {
  return {kind, destination, address, memory: true, bytes: memoryInstruction(kind, destination, [5, ...words([address])])};
}
function registerSpec(kind, destination, source) {return {kind, destination, source, memory: false, bytes: registerInstruction(kind, destination, source)};}
function scan(c, module, row, value, label) {
  const before = cpu(c), expected = scanned(row.kind, value, before.flags), next = {...before, registers: [...before.registers], pc: before.pc + row.bytes.length, flags: expected.flags};
  if (expected.index !== null) next.registers[row.destination] = lowWord(before.registers[row.destination], expected.index);
  run(c, module, 1, next, {label, helperVersion: row.memory ? 10 : null, helperValue: value, exitVersion: module.memoryOps ? 6 : 1});
  return expected;
}

for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) for (const kind of ['forward', 'reverse']) {
  const c = open(owner, entries), rows = Array.from({length: 8}, (_, destination) => registerSpec(kind, destination, (destination + 1) % 8));
  const module = compile(c, rows.map((row, i) => block(PC + i * 16, row.bytes, false)));
  for (const [i, row] of rows.entries()) for (const value of [0, 0x0101, 0x8001, 0xffff]) for (const flags of [2, 0xcd7]) {
    const r = registers(); r[row.source] = lowWord(r[row.source], value); seed(c, PC + i * 16, r, flags);
    scan(c, module, row, value, 'all eight register destinations/sources with dirty source high16'); counts.matrix++;
  }
  const aliases = Array.from({length: 8}, (_, destination) => registerSpec(kind, destination, destination));
  const aliasModule = compile(c, aliases.map((row, i) => block(PC + 0x100 + i * 16, row.bytes, false)));
  for (let bit = 0; bit < 16; bit++) {
    const destination = bit % 8, row = aliases[destination], r = registers(); r[destination] = lowWord(r[destination], 2 ** bit);
    seed(c, PC + 0x100 + destination * 16, r); assert.deepEqual(scan(c, aliasModule, row, 2 ** bit, 'every WORD bit position with source=destination'), {index: bit, flags: 0x402}); counts.matrix++;
  }
  for (const [i, row] of aliases.entries()) {
    const r = registers(); r[i] = lowWord(r[i], 0); const before = seed(c, PC + 0x100 + i * 16, r);
    assert.notEqual(r[i], 0, 'high-only source must remain a nonzero full parent'); scan(c, aliasModule, row, 0, 'high-only register source is zero at WORD width');
    assert.equal(cpu(c).registers[i], before.registers[i], 'zero same-register source preserves the complete dirty destination'); counts.matrix++;
  }
  const literalRows = anchors.map(row => ({...registerSpec(kind, 0, 2), ...row}));
  const literalModule = compile(c, literalRows.map((row, i) => block(PC + 0x200 + i * 16, row.bytes, false)));
  for (const [i, row] of literalRows.entries()) {
    const r = registers(); r[2] = lowWord(r[2], row.value); seed(c, PC + 0x200 + i * 16, r);
    assert.deepEqual(scan(c, literalModule, row, row.value, 'literal WORD parity differs from result or low-byte parity'), {index: row[kind], flags: row.flags}); counts.anchors++;
  }
  assert.equal(c.api.close(), 0);
}

for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) for (const kind of ['forward', 'reverse']) {
  const c = open(owner, entries), rows = Array.from({length: 8}, (_, destination) => memorySpec(kind, destination));
  const module = compile(c, rows.map((row, i) => block(PC + i * 16, row.bytes)));
  for (const [i, row] of rows.entries()) for (const value of [0, 0x8001]) for (const flags of [2, 0xcd7]) {
    upload(c, row.address - 1, Buffer.concat([Buffer.from([0x55]), word(value), Buffer.from([0xaa])])); host(c, 'protect', [DATA, 1, 1]);
    seed(c, PC + i * 16, registers(), flags); scan(c, module, row, value, 'all memory destinations read zero/nonzero from read-only RAM');
    checkRam(c, row.address - 1, Buffer.concat([Buffer.from([0x55]), word(value), Buffer.from([0xaa])])); host(c, 'protect', [DATA, 1, 3]); counts.matrix++;
  }
  for (let bit = 0; bit < 16; bit++) {
    const destination = bit % 8, row = rows[destination]; upload(c, row.address, word(2 ** bit)); seed(c, PC + destination * 16);
    assert.deepEqual(scan(c, module, row, 2 ** bit, 'memory source every WORD bit position'), {index: bit, flags: 0x402}); checkRam(c, row.address, word(2 ** bit)); counts.matrix++;
  }
  assert.equal(c.api.close(), 0);
}

for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries), rows = Array.from({length: 8}, (_, destination) => {
    const kind = destination % 2 ? 'reverse' : 'forward', r = registers(); r[destination] = 0xa53c5008;
    const tail = destination === 4 ? [4, 0x24] : destination === 5 ? [0x45, 0] : [destination];
    return {kind, destination, memory: true, address: 0xa53c5008, r, bytes: memoryInstruction(kind, destination, tail)};
  });
  const scaled = [
    {kind: 'forward', destination: 1, tail: [0x44, 0x8d, 0xfc], values: {1: 0x40000002, 5: 0xa53c5004}},
    {kind: 'reverse', destination: 0, tail: [4, 0x85, ...words([0xa53c5000])], values: {0: 0x40000002}},
    {kind: 'forward', destination: 0, tail: [0x84, 0x80, ...words([0x053c5003])], values: {0: 0x20000001}},
    {kind: 'reverse', destination: 4, tail: [0x44, 0x24, 0xff], values: {4: 0xa53c5009}},
  ];
  for (const row of scaled) {
    const r = registers(); for (const [index, value] of Object.entries(row.values)) r[index] = value;
    rows.push({...row, address: 0xa53c5008, memory: true, r, bytes: memoryInstruction(row.kind, row.destination, row.tail)});
  }
  for (let start = 0; start < rows.length; start += 8) {
    const group = rows.slice(start, start + 8), module = compile(c, group.map((row, i) => block(PC + i * 16, row.bytes)));
    for (const [i, row] of group.entries()) for (const value of [0, 0x8001]) {
      upload(c, row.address - 1, Buffer.concat([Buffer.from([0x55]), word(value), Buffer.from([0xaa])])); seed(c, PC + i * 16, row.r);
      scan(c, module, row, value, 'old full32 destination base/index/both/scaled/wrapped EA');
      checkRam(c, row.address - 1, Buffer.concat([Buffer.from([0x55]), word(value), Buffer.from([0xaa])])); counts.addresses++;
    }
  }
  const boundaries = ['forward', 'reverse'].flatMap(kind => [memorySpec(kind, 0, DATA + 4095), memorySpec(kind, 2, 0xfffffffe)]);
  const boundaryModule = compile(c, boundaries.map((row, i) => block(PC + 0x200 + i * 16, row.bytes)));
  for (const [i, row] of boundaries.entries()) for (const value of [0, 0x8001]) {
    upload(c, row.address, word(value)); seed(c, PC + 0x200 + i * 16); scan(c, boundaryModule, row, value, 'crosspage/last valid WORD read');
    checkRam(c, row.address, word(value)); counts.boundaries++;
  }
  const codeRows = ['forward', 'reverse'].map((kind, i) => ({...memorySpec(kind, i, PC + 0x300 + i * 32)}));
  const codeModule = compile(c, codeRows.map(row => block(row.address, row.bytes)), 5);
  for (const row of codeRows) {
    seed(c, row.address); scan(c, codeModule, row, 0x0f66, 'readable executable own-code is a source, with no SMC exit');
    input(c, 48, words([row.address])); scan(c, codeModule, row, 0x0f66, 'same compiled owner stays current after code read');
    run(c, codeModule, 2, {...cpu(c), pc: row.address + row.bytes.length + 2}, {label: 'same owner reaches its JMP suffix after code reads', reason: 3});
    checkRam(c, row.address, row.bytes); counts.code_reads++;
  }
  assert.equal(c.api.close(), 0);
}

for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries), witnesses = ['forward', 'reverse'].map(kind => ({kind, destination: 0, memory: true, bytes: memoryInstruction(kind, 0, [7])}));
  const mov = Buffer.from([0xbb, 0x44, 0x33, 0x22, 0x11]);
  const blocks = witnesses.map((row, i) => block(PC + i * 32, Buffer.concat([mov, row.bytes])));
  blocks.push(...witnesses.map((row, i) => block(PC + 64 + i * 32, Buffer.concat([mov, row.bytes]))));
  const module = compile(c, blocks), control = registers(); control[7] = 0x7008;
  seed(c, PC + 5, control); run(c, module, 0, cpu(c), {label: 'zero budget avoids unmapped scan read', retired: 0});
  input(c, 96, words([1])); run(c, module, 1, cpu(c), {label: 'cancel avoids unmapped scan read', reason: 2, retired: 0}); input(c, 96, words([0])); counts.controls += 2;
  for (const [index, row] of witnesses.entries()) for (const kind of ['unmapped', 'none', 'write-only', 'execute-only', 'cross-unmapped', 'cross-write-only', 'overflow']) {
    const crossing = kind.startsWith('cross'), overflow = kind === 'overflow', missing = kind.includes('unmapped');
    const address = overflow ? 0xffffffff : crossing ? 0x7fff : 0x7008, mapped = overflow ? 0 : crossing && !missing ? 2 : missing && !crossing ? 0 : 1;
    const unchanged = Buffer.from(crossing && missing ? [0x55, 1] : [0x55, 1, 0x80, 0xaa]);
    if (mapped) {host(c, 'map', [0x7000, mapped, 3]); upload(c, crossing ? 0x7ffe : 0x7007, unchanged);}
    if (!missing && !overflow) host(c, 'protect', [crossing ? 0x8000 : 0x7000, 1, kind === 'none' ? 0 : kind === 'execute-only' ? 4 : 2]);
    if (overflow) upload(c, 0xfffffffe, Buffer.from([0x55, 1]));
    const r = registers(); r[7] = address;
    const before = seed(c, blocks[index].pc, r), stopped = {...before, registers: [...before.registers], pc: before.pc + 5}; stopped.registers[3] = 0x11223344;
    // Ring3 rejects the overflowing logical WORD span before RAM access.
    const fault = overflow ? undefined : {detail: missing ? 1 : 2, address: crossing ? 0x8000 : address, access: 1};
    run(c, module, 2, stopped, {label: kind + ' after MOV retains destination/FLAGS', reason: overflow ? 11 : 5, retired: 1, fault, helperVersion: overflow ? null : 10});
    run(c, module, 1, stopped, {label: kind + ' unchanged read retry', reason: overflow ? 11 : 5, retired: 0, fault, helperVersion: overflow ? null : 10}); counts.faults += 2;
    if (crossing && missing) checkRam(c, 0x7ffe, unchanged);
    if (overflow) checkRam(c, 0xfffffffe, Buffer.from([0x55, 1]));
    if (!overflow) {
      if (missing) host(c, 'map', [crossing ? 0x8000 : 0x7000, 1, 1]);
      else host(c, 'protect', [crossing ? 0x8000 : 0x7000, 1, 1]);
      assert.deepEqual(cpu(c), stopped, 'map/protect-only repair preserves stopped CPU');
      const value = missing ? crossing ? 1 : 0 : 0x8001;
      scan(c, module, {...row, address}, value, 'repaired read consumes current zero/nonzero RAM without replay'); checkRam(c, address, word(value)); counts.retries++;
      host(c, 'unmap', [0x7000, crossing ? 2 : 1]);
    }
  }
  for (const [index, row] of witnesses.entries()) {
    const address = DATA + 8, r = registers(); r[7] = address;
    upload(c, address - 1, Buffer.from([0x55, 0, 0, 0xaa])); host(c, 'protect', [DATA, 1, 2]);
    const before = seed(c, blocks[index].pc, r), stopped = {...before, registers: [...before.registers], pc: before.pc + 5}; stopped.registers[3] = 0x11223344;
    const fault = {detail: 2, address, access: 1};
    run(c, module, 2, stopped, {label: 'zero source still faults on write-only RAM', reason: 5, retired: 1, fault, helperVersion: 10});
    run(c, module, 1, stopped, {label: 'unchanged zero-source retry', reason: 5, retired: 0, fault, helperVersion: 10}); counts.faults += 2;
    host(c, 'protect', [DATA, 1, 1]); assert.deepEqual(cpu(c), stopped);
    assert.deepEqual(scan(c, module, {...row, address}, 0, 'permission-only zero-source repair retains entire destination'), {index: null, flags: 0x446});
    checkRam(c, address - 1, Buffer.from([0x55, 0, 0, 0xaa])); host(c, 'protect', [DATA, 1, 3]); counts.retries++;
  }
  for (const [index, row] of witnesses.entries()) {
    const address = DATA + 12, nextAddress = DATA + 14, r = registers(); r[7] = address; r[0] = lowWord(r[0], 0x7777);
    upload(c, address - 1, Buffer.from([0x55, 0, 0, 0, 0, 0xaa])); host(c, 'protect', [DATA, 1, 2]);
    const before = seed(c, blocks[2 + index].pc, r), stopped = {...before, registers: [...before.registers], pc: before.pc + 5}; stopped.registers[3] = 0x11223344;
    const fault = {detail: 2, address, access: 1};
    run(c, module, 2, stopped, {label: 'current-input witness initially faults on zero source', reason: 5, retired: 1, fault, helperVersion: 10});
    run(c, module, 1, stopped, {label: 'current-input witness unchanged retry', reason: 5, retired: 0, fault, helperVersion: 10}); counts.faults += 2;
    host(c, 'protect', [DATA, 1, 3]); upload(c, nextAddress, word(0x8001)); host(c, 'protect', [DATA, 1, 1]);
    const current = {...stopped, registers: [...stopped.registers]}; current.registers[0] = 0xbaffdead; current.registers[7] = nextAddress;
    input(c, 16, words([current.registers[0]])); input(c, 44, words([current.registers[7]])); assert.deepEqual(cpu(c), current, 'declared current source/EA/destination edits preserve the remaining stopped CPU');
    scan(c, module, {...row, address: nextAddress}, 0x8001, 'retry uses new EA, source and current destination high16');
    assert.equal(cpu(c).registers[0], row.kind === 'forward' ? 0xbaff0000 : 0xbaff000f);
    checkRam(c, address - 1, Buffer.from([0x55, 0, 0, 1, 0x80, 0xaa])); host(c, 'protect', [DATA, 1, 3]); counts.retries++; counts.current_inputs++;
  }

  const bsf = memorySpec('forward', 1), bsr = memorySpec('reverse', 2), zero = registerSpec('forward', 6, 0);
  const xorMemory = Buffer.concat([Buffer.from([0x66, 0x31, 5]), words([DATA + 8])]);
  const live = Buffer.concat([bsf.bytes, Buffer.from([0x66, 0xb8, 1, 0, 0x66, 0xd3, 0xe0]), xorMemory,
    bsr.bytes, Buffer.from([0x66, 0x31, 0xc0]), zero.bytes, Buffer.from([0x0f, 0x94, 0xc3, 0x0f, 0x9a, 0xc7])]);
  const liveModule = compile(c, [block(PC + 0x100, live, true, true)]);
  upload(c, DATA + 7, Buffer.from([0x55, 0, 0x81, 0xaa])); seed(c, PC + 0x100);
  assert.deepEqual(scan(c, liveModule, bsf, 0x8100, 'BSF returns current packed-bitset index8 in CX'), {index: 8, flags: 0x406});
  let old = cpu(c), next = {...old, registers: [...old.registers], pc: old.pc + 4}; next.registers[0] = lowWord(old.registers[0], 1);
  run(c, liveModule, 1, next, {label: 'MOV AX1 retains scan FLAGS'});
  old = cpu(c); next = {...old, registers: [...old.registers], pc: old.pc + 3, flags: 0x406}; next.registers[0] = lowWord(old.registers[0], 0x0100);
  run(c, liveModule, 1, next, {label: 'SHL AX,CL consumes returned scan index8 as bit mask'});
  old = cpu(c); run(c, liveModule, 1, {...old, pc: old.pc + xorMemory.length, flags: 0x486}, {label: 'XOR bitmap clears selected bit8 with current AX mask', helperVersion: 10});
  assert.deepEqual(scan(c, liveModule, bsr, 0x8000, 'BSR sees updated bitmap and returns index15 in DX'), {index: 15, flags: 0x402});
  old = cpu(c); next = {...old, registers: [...old.registers], pc: old.pc + 3, flags: 0x446}; next.registers[0] = lowWord(old.registers[0], 0);
  run(c, liveModule, 1, next, {label: 'XOR AX,AX produces low16 zero despite dirty high16'});
  old = cpu(c); assert.deepEqual(scan(c, liveModule, zero, 0, 'register WORD zero retains full SI and sets ZF/PF'), {index: null, flags: 0x446}); assert.equal(cpu(c).registers[6], old.registers[6]);
  old = cpu(c); next = {...old, registers: [...old.registers], pc: old.pc + 6}; next.registers[3] = lowWord(old.registers[3], 0x0101);
  run(c, liveModule, 2, next, {label: 'SETZ BL/SETP BH consume current zero scan flags', retired: 2});
  checkRam(c, DATA + 7, Buffer.from([0x55, 0, 0x80, 0xaa])); counts.consumers++;
  assert.equal(c.api.close(), 0);
}
assert.deepEqual(sources(), sourcePins, 'risk-relevant sources stayed fixed');
assert.equal(counts.contexts, 24); assert.equal(counts.runs, 1544); assert.equal(counts.retired, 1468);
assert.equal(counts.matrix, 1088); assert.equal(counts.anchors, 56); assert.equal(counts.addresses, 96);
assert.equal(counts.boundaries, 32); assert.equal(counts.faults, 144); assert.equal(counts.retries, 64);
assert.equal(counts.current_inputs, 8); assert.equal(counts.consumers, 4); assert.equal(counts.controls, 8);
assert.equal(counts.code_reads, 8); assert.equal(modules.length, 56);
const result = {status: 'ok', engine_sha256: engine.sha256, source_pins: sourcePins, counts, modules,
  limits: ['finite exact single66 WORD BSF/BSR register/memory in four actual bound profiles, with native register standalone coverage separate',
    'independent low16 bit-string indices/whole-WORD population parity; zero retains entire destination, nonzero merges old high16',
    'ZF is defined by the source; destination-on-zero and PF/other arithmetic choices follow Ring3 policy, without hardware undefined-bit equality claim',
    'scanner-only memory imports guard/read16; only the declared existing-XOR live consumer additionally stores the bitmap',
    'real read16 failures and selected RAM/canary checks; readable code/data scans never store or cause SMC',
    'complete arena preserves FP128; retry/current-input/live returned-index and flag consumers avoid hidden CPU reseeding',
    'no exhaustive ISA, throughput, PE/browser/SDK/game or CI claim']};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2) + '\n', {flag: 'wx'});
console.log(JSON.stringify({status: result.status, ...counts, modules: modules.length, output}));
