import {pageFaultExit,pageFaultPacket} from '../../../../../../support/wasm/page-fault.mjs';
import {generatedImports, attachDetectionApi} from '../../../../../../support/wasm/binding.mjs';
import {ARENA_SIZE} from '../../../../../../support/wasm/arena.mjs';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../../../../../../support/wasm/engine.mjs';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root);
const engine = readEngine(enginePath), SIZE = ARENA_SIZE, PC = 0x1000, DATA = 0x5000;
const initial = readFileSync(join(output, 'initial-arena.bin'));
assert.equal(initial.length, SIZE);
assert.deepEqual(WebAssembly.Module.imports(engine.module), []);
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const paths = [
  'engine/src/cpu/dbt/wasm/instructions/segments/access.rs', 'engine/src/cpu/x86/segments/cache.rs',
  'engine/src/abi/arena.rs', 'engine/src/abi/memory_helper.rs',
  'engine/src/cpu/x86/ir.rs', 'engine/src/cpu/x86/decode/lower.rs', 'engine/src/cpu/x86/decode/families.rs',
  'engine/src/cpu/x86/decode/operands.rs', 'engine/src/cpu/x86/decode/flow.rs', 'engine/src/cpu/x86/decode/profile.rs',  'engine/src/cpu/dbt/region.rs',
  'engine/src/cpu/dbt/wasm/instructions/integer/arithmetic.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/bits.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/carry_rotates.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/decimal.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/mod.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/arithmetic.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/moves.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/exchange.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/predicates.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/shifts.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/control.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/extended.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/flags.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/mod.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/multiply_divide.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/operands.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/rotates.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/shifts.rs', 'engine/src/cpu/dbt/wasm/imports/mod.rs', 'engine/src/cpu/dbt/wasm/imports/binding.rs', 'engine/src/cpu/dbt/wasm/imports/context.rs', 'engine/src/cpu/dbt/wasm/imports/needs.rs', 'engine/src/cpu/dbt/wasm/helpers/protocol.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/mod.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/address.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/scalar.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/stack.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/word_store.rs',
  'engine/src/cpu/dbt/wasm/instructions/memory/narrow.rs', 'engine/src/memory/space.rs', 'engine/src/memory/space/atomic.rs',
  'engine/src/cpu/dbt/wasm/abi.rs', 'engine/src/cpu/dbt/wasm/emitter/mod.rs', 'engine/src/cpu/dbt/wasm/emitter/module.rs', 'engine/src/cpu/dbt/wasm/emitter/body.rs', 'engine/src/cpu/dbt/wasm/requirements.rs', 'engine/src/cpu/dbt/wasm/imports/wiring.rs', 'engine/src/process/wasm/mod.rs', 'engine/src/process/wasm/loading.rs', 'engine/src/abi/image.rs', 'engine/src/process/wasm/execution.rs', 'engine/src/abi/execution.rs', 'engine/src/process/wasm/calls.rs', 'engine/src/process/wasm/memory.rs', 'engine/src/process/wasm/cpu.rs', 'engine/src/process/mod.rs',
  'engine/src/process/instance.rs', 'engine/src/process/execution/compiled.rs', 'engine/src/process/execution/authority.rs', 'engine/src/process/memory/address_space.rs', 'engine/src/process/memory/scalar.rs', 'engine/src/process/memory/scalar/tests.rs', 'engine/src/process/execution/mod.rs', 'engine/src/process/memory/mod.rs',  'engine/src/process/execution/resident.rs',    'engine/tests/cpu/stack/operands/native/word.rs', 'engine/tests/cpu/stack/native.rs', 'engine/tests/support/cpu/prefix/admission.rs', 'engine/tests/support/cpu/prefix/word.rs',
  'engine/tests/cpu/stack/operands/wasm/word.rs', 'engine/tests/cpu/stack/wasm.rs', 'engine/tests/support/wasm.rs',
  'engine/tests/cpu/stack/operands/fixtures/p2-word-stack/runtime/run.mjs', 'engine/tests/support/wasm/engine.mjs', 'engine/tests/support/wasm/binding.mjs','engine/tests/support/wasm/page-fault.mjs',
];
const sources = () => Object.fromEntries(paths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = sources(), modules = [];
const counts = {contexts: 0, runs: 0, retired: 0, matrix: 0, immediates: 0, flag_inputs: 0, anchors: 0, addresses: 0, boundaries: 0, repeats: 0, faults: 0, retries: 0, current_inputs: 0, consumers: 0, controls: 0, retained_units: 0, code_reads: 0, self_code: 0};
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
const block = row => ({pc: row.pc, read: row.op === 'pop' || row.memory, store: row.op === 'push' || row.memory, instructions: 2, bytes: Buffer.concat([row.bytes, Buffer.from([0xeb, 0])])});
function packRows(rows, size, pc, stride) {
  const blocks = [];
  for (let i = 0; i < rows.length; i += size) {
    const group = rows.slice(i, i + size), start = pc + blocks.length * stride; let cursor = start;
    for (const row of group) {row.pc = cursor; cursor += row.bytes.length;}
    for (const row of group) row.suffix = cursor;
    const bytes = Buffer.concat([...group.map(row => row.bytes), Buffer.from([0xeb, 0])]);
    assert.ok(bytes.length <= stride, 'packed block does not overlap the next authored block');
    blocks.push({pc: start, bytes, instructions: group.length + 1, read: group.some(row => row.op === 'pop' || row.memory), store: group.some(row => row.op === 'push' || row.memory)});
  }
  return blocks;
}
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
  assert.equal(c.api.open(12, c.id, 0x5753544b), 0); c.base = c.api.arena_ptr() >>> 0; input(c, 0, initial);
  for (const args of [[PC, 1, 7], [DATA, 2, 3], [0xa53c5000, 1, 3], [0xfffff000, 1, 3], [0, 1, 3], [0xf000, 2, 3], [0x8000, 1, 3]]) map(c, ...args);
  upload(c, 0x8000, canary); host(c, 'protect', [0x8000, 1, 1]);
  return c;
}

// full unsigned ESP arithmetic and little-endian digits are independent of emitted Wasm stack locals.
const wrap = value => Number((BigInt(value) % 4294967296n + 4294967296n) % 4294967296n);
const lowWord = (parent, value) => Math.floor(parent / 65536) * 65536 + value;
const ea = (r, row) => wrap((row.base === null ? 0n : BigInt(r[row.base]))
  + (row.index === null ? 0n : BigInt(r[row.index]) * BigInt(row.scale)) + BigInt(row.displacement));
const flags = Array.from({length: 128}, (_, n) => 2 + [1, 4, 16, 64, 128, 1024, 2048].reduce((sum, bit, i) => sum + Math.floor(n / 2 ** i) % 2 * bit, 0));
assert.equal(new Set(flags).size, 128); assert.ok(flags.every(value => (value & 2) && !(value & ~0xcd7)));
const anchorFlags = [2, 0xcd7, 0x443, 0x892], HIGH = 0xa53c5000;
const reg = (op, register, modrm, pc) => ({op, register, memory: false, pc,
  bytes: Buffer.from(modrm ? [0x66, op === 'push' ? 0xff : 0x8f, (op === 'push' ? 0xf0 : 0xc0) + register] : [0x66, (op === 'push' ? 0x50 : 0x58) + register])});
const immediate = (raw, narrow, pc) => ({op: 'push', memory: false, pc, immediate: narrow && raw >= 128 ? raw + 65280 : raw,
  bytes: Buffer.from(narrow ? [0x66, 0x6a, raw] : [0x66, 0x68, ...word(raw)])});
function memory(op, tail, address, pc) {
  return {op, memory: true, pc, ...address, bytes: Buffer.from([0x66, op === 'push' ? 0xff : 0x8f, tail[0] | (op === 'push' ? 0x30 : 0), ...tail.slice(1)])};
}
function transition(before, row, captured) {
  const pending = wrap(BigInt(before.registers[4]) + (row.op === 'push' ? -2n : 2n));
  const next = {...before, registers: [...before.registers], pc: before.pc + row.bytes.length};
  let value, readAddress = null, writeAddress = null;
  if (row.op === 'push') {
    value = row.memory ? captured : row.immediate ?? before.registers[row.register] % 65536;
    if (row.memory) readAddress = ea(before.registers, row);
    writeAddress = pending; next.registers[4] = pending;
  } else {
    value = captured; readAddress = before.registers[4]; next.registers[4] = pending;
    if (row.memory) writeAddress = ea(next.registers, row);
    else next.registers[row.register] = lowWord(row.register === 4 ? pending : before.registers[row.register], value);
  }
  assert.ok(Number.isInteger(value) && value >= 0 && value <= 65535);
  return {next, value, readAddress, writeAddress, helperVersion: writeAddress === null ? 2 : 4};
}
function operation(c, module, row, captured, label, reason = 1) {
  const effects = transition(cpu(c), row, captured);
  run(c, module, 1, effects.next, {label, reason, helperVersion: effects.helperVersion, helperValue: effects.writeAddress === null ? effects.value : 0});
  if (effects.writeAddress !== null) checkRam(c, effects.writeAddress, word(effects.value)); return effects;
}
function framed(c, address, value) {upload(c, address - 1, Buffer.from([0x55, ...word(value), 0xaa]));}
const boundaryAnchors = [
  {op: 'push', register: 4, esp: 0x10000, value: 0, next: 0xfffe, address: 0xfffe},
  {op: 'pop', register: 4, esp: HIGH + 8, value: 0xbeef, next: 0xa53cbeef, address: HIGH + 8},
  {op: 'pop', register: 4, esp: 0xffff, value: 0xbeef, next: 0x1beef, address: 0xffff},
  {op: 'pop', register: 4, esp: 0xfffffffe, value: 0xabcd, next: 0xabcd, address: 0xfffffffe},
  {op: 'push', register: 0, esp: 0, value: 0x1234, next: 0xfffffffe, address: 0xfffffffe},
  {op: 'pop', register: 0, esp: 0xfffffffe, value: 0x1234, next: 0, address: 0xfffffffe},
  {op: 'push', register: 0, esp: DATA + 0x1001, value: 0x8001, next: DATA + 0xfff, address: DATA + 0xfff},
  {op: 'pop', register: 0, esp: DATA + 0xfff, value: 0x8001, next: DATA + 0x1001, address: DATA + 0xfff},
];
for (const a of boundaryAnchors) {
  const row = reg(a.op, a.register, false, PC), r = registers(); r[4] = a.esp; if (a.register !== 4) r[a.register] = lowWord(r[a.register], a.value);
  const effects = transition({registers: r, pc: PC, flags: 0xcd7}, row, a.value);
  assert.equal(effects.next.registers[4], a.next); assert.equal(effects.value, a.value);
  assert.equal(effects.writeAddress ?? effects.readAddress, a.address);
}
const live = Buffer.from([
  0x66, 0x54, 0x66, 0x58, 0x66, 0x6a, 0xff, // push originalSP; popAX; push signextendedFFFF.
  0x66, 0x8f, 0x04, 0x24, // pop[ESP] writes via pendingESP.
  0x66, 0xff, 0x34, 0x24, 0x66, 0x5a, // push[ESP] reads oldESP; popDX consumes the result.
  0x66, 0x11, 0xd0, 0x0f, 0x92, 0xc3, 0xeb, 0, // adcAX,DX; setbBL; suffixJMP.
]);
for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries), pushRows = [], popRows = [];
  for (const [op, rows, offset] of [['push', pushRows, 0], ['pop', popRows, 0x200]])
    for (let register = 0; register < 8; register++) for (const modrm of [true, false]) rows.push(reg(op, register, modrm, PC + offset + rows.length * 16));
  const immediateRows = [...[0, 1, 0x7f, 0x80, 0xff, 0x7fff, 0x8000, 0xffff, 0x6766, 0xf3f0].map(value => [value, false]),
    ...[0, 1, 0x7f, 0x80, 0xff, 0x66, 0x67, 0xf0, 0xf2, 0xf3].map(value => [value, true])].map(([value, narrow], i) => immediate(value, narrow, PC + 0x400 + i * 16));
  const codeRow = reg('pop', 0, false, PC + 0x600), livePc = PC + 0x700;
  const liveBlock = {pc: livePc, bytes: live, read: true, store: true, instructions: 9};
  const pushBlocks = packRows(pushRows, 2, PC, 16), popBlocks = packRows(popRows, 2, PC + 0x200, 16), immediateBlocks = packRows(immediateRows, 3, PC + 0x400, 16);
  // all executable bytes and RX permissions are fixed before the first capture.
  for (const b of [...pushBlocks, ...popBlocks, ...immediateBlocks, block(codeRow), liveBlock]) upload(c, b.pc, b.bytes);
  host(c, 'protect', [PC, 1, 5]); let firstModule;
  for (const rows of [pushRows, popRows]) {
    const module = compile(c, rows === pushRows ? pushBlocks : popBlocks); if (rows === pushRows) firstModule = module;
    for (const row of rows) for (const [i, value] of [0, 0xffff, 0x8001, 0x7ffe].entries()) {
      const r = registers(); r[4] = row.op === 'push' && row.register === 4 ? HIGH + 0xa + i * 4 : DATA + 0x10;
      if (row.register !== 4) r[row.register] = (0x8001 + row.register * 0x1357) % 65536 * 65536 + value;
      const address = row.op === 'push' ? wrap(BigInt(r[4]) - 2n) : r[4]; framed(c, address, row.op === 'pop' ? value : 0x5aa5);
      seed(c, row.pc, r, anchorFlags[(row.register + i) % 4]); const effects = operation(c, module, row, value, 'all8 short/ModRM register forms retain own highs and original SP capture');
      checkRam(c, address - 1, Buffer.from([0x55, ...word(row.op === 'pop' ? value : effects.value), 0xaa])); counts.matrix++;
    }
    const selected = rows.find(row => row.register === 0 && row.bytes.length === 2);
    for (const value of flags) {
      const r = registers(); r[4] = DATA + 0x10; r[0] = 0xcafebeef; framed(c, rowAddress(selected.op, r[4]), 0x8001);
      seed(c, selected.pc, r, value); operation(c, module, selected, 0x8001, 'all128 valid FLAGS in distinguishing push/pop paths'); counts.flag_inputs++;
    }
    for (const a of boundaryAnchors.filter(a => a.op === selected.op)) {
      const row = rows.find(row => row.register === a.register && row.bytes.length === 2), r = registers(); r[4] = a.esp; if (a.register !== 4) r[a.register] = lowWord(r[a.register], a.value);
      upload(c, a.address, word(a.value)); seed(c, row.pc, r); const effects = operation(c, module, row, a.value, 'literal fullESP wrap/crosspage/oldSP/POP-SP pending-high discriminator');
      assert.equal(effects.next.registers[4], a.next); assert.equal(effects.value, a.value); counts.boundaries++; counts.anchors++;
    }
    const r = registers(); r[4] = selected.op === 'push' ? 1 : 0xffffffff; seed(c, selected.pc, r);
    run(c, module, 1, cpu(c), {label: 'full two-byte span atFFFFFFFF fails before any publication', reason: 15, retired: 0, helperVersion: null, exitVersion: 6}); counts.boundaries++;
    const repeated = selected.op === 'push' ? selected : rows.find(row => row.register === 4 && row.bytes.length === 2), original = registers(); original[4] = DATA + 0x10; original[0] = 0xcafe1234;
    upload(c, DATA + 0xb, Buffer.alloc(0x28, 0x55)); if (repeated.op === 'pop') {upload(c, DATA + 0x10, word(DATA + 0x20)); upload(c, DATA + 0x20, word(DATA + 0x30));}
    seed(c, repeated.pc, original); operation(c, module, repeated, DATA + 0x20, 'first stack transfer'); input(c, 48, words([repeated.pc])); operation(c, module, repeated, DATA + 0x30, 'only PC rewound; current returned ESP is consumed'); counts.repeats += 2;
    const current = cpu(c); current.pc = repeated.pc; current.flags = 0x892; current.registers[0] = 0xbaff7ffe; current.registers[4] = HIGH + 8; upload(c, HIGH + 8, word(0xbeef));
    input(c, 0, state(current)); operation(c, module, repeated, 0xbeef, 'explicit current stack/source/own-high/FLAGS inputs'); counts.current_inputs++;
    seed(c, selected.pc, original); run(c, module, 0, cpu(c), {label: 'budget0 before stack access', retired: 0, helperVersion: null});
    input(c, 96, words([1])); run(c, module, 1, cpu(c), {label: 'cancel before stack access', reason: 2, retired: 0, helperVersion: null}); run(c, module, 0, cpu(c), {label: 'cancel precedes budget0', reason: 2, retired: 0, helperVersion: null}); input(c, 96, words([0]));
    const suffix = selected.suffix; input(c, 48, words([suffix])); run(c, module, 2, {...cpu(c), pc: suffix + 2}, {label: 'suffix retains returned stack state with budget2', reason: 3, helperVersion: null}); run(c, module, 1, cpu(c), {label: 'cold lookup preserves all state', reason: 3, retired: 0, helperVersion: null}); counts.controls += 5;
  }
  const immediateModule = compile(c, immediateBlocks);
  for (const row of immediateRows) {const r = registers(); r[4] = DATA + 0x10; framed(c, DATA + 0xe, 0x5aa5); seed(c, row.pc, r); operation(c, immediateModule, row, undefined, 'imm16 versus signedimm8 normalization and prefix-looking payload'); counts.immediates++;}
  const codeModule = compile(c, [block(codeRow)]), codeRegisters = registers(); codeRegisters[4] = codeRow.pc; seed(c, codeRow.pc, codeRegisters);
  operation(c, codeModule, codeRow, 0x5866, 'POP reads RX own-code WORD without stores or SMC'); input(c, 48, words([codeRow.pc])); operation(c, codeModule, codeRow, 0x00eb, 'PC-only repeat reads current next stackWORD from suffix');
  run(c, codeModule, 2, {...cpu(c), pc: codeRow.pc + 4}, {label: 'RX read owner remains current through suffix', reason: 3, helperVersion: null}); counts.code_reads += 3; checkRam(c, codeRow.pc, block(codeRow).bytes);
  const mixed = compile(c, [liveBlock]), oldest = pushRows[0], retained = registers(); retained[4] = DATA + 0x10; seed(c, oldest.pc, retained);
  if (owner === 'resident') operation(c, firstModule, oldest, undefined, 'older resident stack unit remains current after later captures'); else run(c, firstModule, 1, cpu(c), {label: 'replacement generation mismatch3 is neutral', status: 3, retired: 0}); counts.retained_units++;
  const r = registers(); r[0] = 0xcafe1234; r[2] = 0xface5678; r[3] = 0xbeef0000; r[4] = HIGH + 0xa; seed(c, livePc, r);
  const liveRows = [reg('push', 4, false, livePc), reg('pop', 0, false, livePc + 2), immediate(0xff, true, livePc + 4),
    memory('pop', [4, 0x24], {base: 4, index: null, scale: 1, displacement: 0}, livePc + 7), memory('push', [4, 0x24], {base: 4, index: null, scale: 1, displacement: 0}, livePc + 11), reg('pop', 2, false, livePc + 15)];
  for (const [i, row] of liveRows.entries()) operation(c, mixed, row, [undefined, 0x500a, undefined, 0xffff, 0xffff, 0xffff][i], 'live returned WORD/oldESP/pendingESP stack consumer without CPU reseed');
  let before = cpu(c), next = {...before, registers: [...before.registers], pc: before.pc + 3, flags: 0x417}; next.registers[0] = lowWord(before.registers[0], 0x500a); run(c, mixed, 1, next, {label: 'ADC consumes returned AX/DX and unchanged live carry', helperVersion: null});
  before = cpu(c); next = {...before, registers: [...before.registers], pc: before.pc + 3}; next.registers[3] = lowWord(before.registers[3], 1); run(c, mixed, 1, next, {label: 'SETB observes live carry', helperVersion: null});
  run(c, mixed, 2, {...cpu(c), pc: livePc + live.length}, {label: 'live suffix reaches cold lookup with budget2', reason: 3, helperVersion: null}); run(c, mixed, 1, cpu(c), {label: 'live cold result preserves all returned state', reason: 3, retired: 0, helperVersion: null}); counts.consumers += 10;
  checkRam(c, HIGH + 8, word(0xffff)); checkRam(c, HIGH + 0xa, word(0xffff)); checkRam(c, 0x8000, canary); checkRam(c, livePc, live); assert.equal(c.api.close(), 0);
}
function rowAddress(op, esp) {return op === 'push' ? wrap(BigInt(esp) - 2n) : esp;}

function memoryForms() {
  const form = (tail, base, index = null, scale = 1, displacement = 0, parents = {}, relative = null) => ({tail, base, index, scale, displacement, parents, relative});
  return [
    form([3], 3, null, 1, 0, {3: DATA + 0x80}),
    form([4, 0x24], 4, null, 1, 0, {}, 0),
    form([0x44, 0x24, 0xfe], 4, null, 1, -2, {}, -2),
    form([0x45, 0], 5, null, 1, 0, {5: DATA + 0x80}),
    form([0x44, 0x8b, 0xf8], 3, 1, 4, -8, {3: DATA + 0x78, 1: 4}),
    form([4, 0xb5, ...words([DATA + 0x70])], null, 6, 4, DATA + 0x70, {6: 4}),
    form([5, ...words([DATA + 0x80])], null, null, 1, DATA + 0x80),
    form([0x84, 0x24, ...words([0xffffffff])], 4, null, 1, -1, {}, -1),
    form([0x84, 0xf4, ...words([0x80000000])], 4, 6, 8, 0x80000000, {6: 0x10000000}, 0),
    form([0x44, 0x8c, 8], 4, 1, 4, 8, {1: 3}, 20),
    form([4, 0], 0, 0, 1, 0, {0: (DATA + 0x80) / 2}),
    form([4, 0x85, ...words([DATA + 0x7c])], null, 0, 4, DATA + 0x7c, {0: 1}),
    form([0x43, 0x80], 3, null, 1, -128, {3: DATA + 0x100}),
  ];
}
const failureCases = ['unmapped', 'none', 'wrong-permission', 'execute-only', 'cross-missing', 'cross-denied', 'overflow'];
function failureSetup(c, name, access) {
  unmap(c, 0x9000); unmap(c, 0xa000);
  const crossing = name.startsWith('cross-'), address = name === 'overflow' ? 0xffffffff : crossing ? 0x9fff : 0x9008;
  if (name !== 'unmapped' && name !== 'overflow') {
    map(c, 0x9000, 1, 3);
    if (name === 'cross-denied') map(c, 0xa000, 1, 3);
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
const mov = Buffer.from([0xbb, 0x44, 0x33, 0x22, 0x11]);
function stoppedFault(c, module, row, r, fault, helperVersion, label, segmentReason) {
  const before = seed(c, row.pc - 5, r), stopped = {...before, registers: [...before.registers], pc: row.pc}; stopped.registers[3] = 0x11223344;
  run(c, module, 2, stopped, {label: label + ' after prior MOV retirement', reason: segmentReason ?? 5, retired: 1, fault: segmentReason ? undefined : fault, helperVersion: segmentReason ? null : helperVersion, exitVersion: segmentReason ? 6 : 2});
  run(c, module, 1, stopped, {label: label + ' unchanged retry', reason: segmentReason ?? 5, retired: 0, fault: segmentReason ? undefined : fault, helperVersion: segmentReason ? null : helperVersion, exitVersion: segmentReason ? 6 : 2}); counts.faults += 2; return stopped;
}
for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries), forms = memoryForms(), rows = [];
  for (const [op, offset] of [['push', 0], ['pop', 0x200]]) for (const [i, form] of forms.entries())
    rows.push({...memory(op, form.tail, form, PC + offset + i * 16), formIndex: i});
  const faultRows = [reg('push', 0, false, PC + 0x405), reg('pop', 0, false, PC + 0x445),
    memory('push', [7], {base: 7, index: null, scale: 1, displacement: 0}, PC + 0x485), memory('pop', [7], {base: 7, index: null, scale: 1, displacement: 0}, PC + 0x4c5)];
  const faultBlocks = faultRows.map(row => ({...block(row), pc: row.pc - 5, bytes: Buffer.concat([mov, block(row).bytes]), instructions: 3}));
  const matrixBlocks = packRows(rows, 4, PC, 64);
  for (const b of matrixBlocks.concat(faultBlocks)) upload(c, b.pc, b.bytes); host(c, 'protect', [PC, 1, 5]);
  assert.equal(rows.length+matrixBlocks.length,33);
  const identity=rows.find(row=>row.op==='pop'&&row.formIndex===2);
  for(let start=0;start<matrixBlocks.length;start+=2) {
  const selected=matrixBlocks.slice(start,start+2),selectedRows=rows.filter(row=>selected.some(b=>row.pc>=b.pc&&row.pc<b.pc+b.bytes.length));
  const matrix=compile(c,selected);
  for (const row of selectedRows) for (const value of [0x1234, 0xffff]) {
    const r = registers(); Object.assign(r, row.parents); r[4] = DATA + 0x100;
    const original = Buffer.from(Array.from({length: 0xe0}, (_, i) => (i * 17 + 9) % 256)), start = DATA + 0x60;
    const readAddress = row.op === 'push' ? ea(r, row) : r[4]; original.set(word(value), readAddress - start);
    upload(c, start, original); seed(c, row.pc, r, anchorFlags[(row.formIndex + Number(value === 0xffff)) % 4]);
    const expected = Buffer.from(original), effects = transition(cpu(c), row, value);
    const operandAddress = row.op === 'push' ? effects.readAddress : effects.writeAddress;
    assert.equal(operandAddress, row.relative === null ? DATA + 0x80 : DATA + 0x100 + row.relative + (row.op === 'pop' ? 2 : 0), 'literal old versus pending ESP effective address');
    expected.set(word(value), effects.writeAddress - start);
    operation(c, matrix, row, value, 'ordered memory transfer captures original WORD before exact/partial overlap and publishes pendingESP once'); checkRam(c, start, expected); counts.addresses++;
  }
  if(selectedRows.includes(identity)) {const r=registers();r[4]=DATA+0x100;
  framed(c, r[4], 0xbeef); host(c, 'protect', [DATA, 1, 1]); const old = seed(c, identity.pc, r), identityFault = {detail: 2, address: r[4], access: 2};
  run(c, matrix, 1, old, {label: 'identity POP[ESP-2] still requires checked Write after successful read', reason: 5, retired: 0, fault: identityFault});
  run(c, matrix, 1, old, {label: 'identity write denial unchanged retry', reason: 5, retired: 0, fault: identityFault}); counts.faults += 2;
  host(c, 'protect', [DATA, 1, 3]); assert.deepEqual(cpu(c), old); checkRam(c, r[4] - 1, Buffer.from([0x55, 0xef, 0xbe, 0xaa])); operation(c, matrix, identity, 0xbeef, 'protect-only repair commits sameWORD and pendingESP'); counts.retries++;
  }
  }
  const faults = compile(c, faultBlocks);
  for (const [index, row] of faultRows.entries()) {
    const readStage = index === 1 || index === 2, access = readStage ? 1 : 2, helperVersion = readStage ? 2 : 4;
    const controls = registers(); controls[4] = 0x9008; controls[7] = 0xa008; seed(c, row.pc - 5, controls);
    run(c, faults, 0, cpu(c), {label: 'budget0 before prefix/stack helpers', retired: 0, helperVersion: null}); input(c, 96, words([1])); run(c, faults, 1, cpu(c), {label: 'cancel before prefix/stack helpers', reason: 2, retired: 0, helperVersion: null}); input(c, 96, words([0])); counts.controls += 2;
    for (const name of failureCases) {
      const failure = failureSetup(c, name, access), r = registers(); r[0] = 0xcafebeef; r[7] = failure.address;
      r[4] = index === 0 ? wrap(BigInt(failure.address) + 2n) : index === 1 ? failure.address : DATA + (index === 2 ? 0x102 : 0x100);
      framed(c, DATA + 0x100, 0xbeef);
      const fault = {detail: failure.detail, address: failure.faultAddress, access}, stopped = stoppedFault(c, faults, row, r, fault, helperVersion, name + ' ordered stage', name === 'overflow' ? index < 2 ? 15 : 11 : undefined);
      if (name !== 'overflow') {
        repair(c, failure); assert.deepEqual(cpu(c), stopped, 'map/protect-only repair retains stopped CPU/FLAGS');
        checkRam(c, failure.address, word(failure.original)); checkRam(c, DATA + 0x100, word(0xbeef));
        operation(c, faults, row, readStage ? failure.original : 0xbeef, 'repair-only retry reads original/zero-filled WORD and commits once'); counts.retries++;
      }
      unmap(c, 0x9000); unmap(c, 0xa000);
    }
  }
  for (const push of [true, false]) {
    const row = faultRows[push ? 2 : 3], r = registers(); r[4] = push ? 0x900a : 0x9008; r[7] = DATA + 0x80;
    map(c, 0x9000, 1, 3); framed(c, 0x9008, 0x800f); framed(c, DATA + 0x80, 0x1234); host(c, 'protect', [0x9000, 1, push ? 1 : 2]);
    const fault = {detail: 2, address: 0x9008, access: push ? 2 : 1}, stopped = stoppedFault(c, faults, row, r, fault, push ? 4 : 2, 'opposite ordered stage suppresses publication');
    host(c, 'protect', [0x9000, 1, 3]); assert.deepEqual(cpu(c), stopped); checkRam(c, 0x9008, word(0x800f)); checkRam(c, DATA + 0x80, word(0x1234));
    operation(c, faults, row, push ? 0x1234 : 0x800f, 'opposite-stage protect-only retry'); counts.retries++; unmap(c, 0x9000);
  }
  for (const push of [true, false]) {
    const row = faultRows[push ? 2 : 3], r = registers(); r[4] = push ? 0xa00a : 0x9008; r[7] = push ? 0x9008 : 0xa008;
    stoppedFault(c, faults, row, r, {detail: 1, address: 0x9008, access: 1}, 2, 'both accesses unmapped: literal first read9008 precedes second destinationA008');
  }
  for (const push of [true, false]) {
    const row = faultRows[push ? 2 : 3], r = registers(); r[4] = push ? 0x900a : DATA + 0x100; r[7] = push ? DATA + 0x80 : 0x9008;
    map(c, 0x9000, 1, 3); framed(c, 0x9008, 0x5aa5); upload(c, DATA + 0x80, word(0x1234)); upload(c, DATA + 0x100, word(0x1234)); host(c, 'protect', [0x9000, 1, 1]);
    const stopped = stoppedFault(c, faults, row, r, {detail: 2, address: 0x9008, access: 2}, 4, 'current-input witness begins with second-store denial');
    host(c, 'protect', [0x9000, 1, 3]); upload(c, push ? DATA + 0x84 : DATA + 0x102, word(0xbeef));
    const current = {...stopped, registers: [...stopped.registers], flags: 0x892}; current.registers[0] = 0xbaff8000; current.registers[4] = push ? 0x900c : DATA + 0x102; current.registers[7] = push ? DATA + 0x84 : 0x900a;
    input(c, 0, state(current)); operation(c, faults, row, 0xbeef, 'separate explicit current WORD/ESP/sourceEA/destinationEA/high/FLAGS retry');
    checkRam(c, 0x9007, Buffer.from([0x55, 0xa5, 0x5a, 0xef, 0xbe])); checkRam(c, push ? DATA + 0x80 : DATA + 0x100, word(0x1234)); counts.retries++; counts.current_inputs++; unmap(c, 0x9000);
  }
  checkRam(c, 0x8000, canary); assert.equal(c.api.close(), 0);
}

for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) for (const push of [true, false]) for (const changed of [false, true]) {
  const c = open(owner, entries), row = push ? reg('push', 0, false, PC) : memory('pop', [7], {base: 7, index: null, scale: 1, displacement: 0}, PC);
  upload(c, PC, block(row).bytes); const module = compile(c, [block(row)]), originalWord = push ? 0x5066 : 0x8f66, value = originalWord + Number(changed), r = registers();
  r[0] = 0xcafe0000 + value; r[4] = push ? PC + 2 : DATA + 0x100; r[7] = PC; if (!push) upload(c, r[4], word(value)); seed(c, PC, r);
  operation(c, module, row, value, 'changed/identity consumed-code write commits WORD/ESP/EIP/FLAGS and one retirement beforeSMC', 6);
  run(c, module, 1, cpu(c), {label: 'code-invalidated same owner4 is completely neutral before fresh capture', status: 4, retired: 0});
  const suffix = {pc: PC + row.bytes.length, bytes: Buffer.from([0xeb, 0]), read: false, store: false, instructions: 1}, tail = compile(c, [suffix]);
  run(c, tail, 2, {...cpu(c), pc: suffix.pc + 2}, {label: 'fresh pure suffix continues without stack transfer replay', reason: 3, helperVersion: null, exitVersion: 1});
  checkRam(c, PC, word(value)); if (!push) checkRam(c, DATA + 0x100, word(value)); checkRam(c, 0x8000, canary); assert.equal(c.api.close(), 0); counts.self_code += 3;
}
assert.deepEqual(sources(), sourcePins);
assert.deepEqual(counts, {contexts: 24, runs: 2460, retired: 2222, matrix: 512, immediates: 80, flag_inputs: 1024, anchors: 32, addresses: 208, boundaries: 40, repeats: 16, faults: 280, retries: 116, current_inputs: 16, consumers: 40, controls: 72, retained_units: 4, code_reads: 12, self_code: 48});
assert.equal(modules.length,72);
const result = {status: 'ok', engine_sha256: engine.sha256, source_pins: sourcePins, counts, modules,
  limits: [
    'finite exact single66 WORD PUSH/POP register/ModRM/immediate/memory data family in four bound profiles; standalone stack execution remains closed',
    'independent BigInt fullESP wrap/own-half/little-endian oracle, eight successful literal SP/top/crosspage anchors, all128 valid FLAGS through selected push/pop rows',
    'original SP/PUSH-memory EA versus pending POP-memory EA, pending-high POPSP carry/wrap, exact/partial overlap and real read16V2/store16V4 helper ordering',
    'seven shared span/access fault classes with prior retirement/unchanged retry/map-protect-only repair, literal read precedence when both addresses fail, separate explicit current inputs',
    'identity POP[ESP-2] still requires Write; changed/same code writes commit once before6 then stale4/freshV1suffix; RX POP reads and RO/RX canaries do not store',
    'maxfive resident captures/context; the unchanged33-instruction memory matrix runs in materials of at most2 blocks before the12-instruction fault material; aggregate24/24/27/2/9 plus SMC2/1 remain bounded by64instructions,8blocks and65KiB; full4612/helper/dirtyFP preservation',
    'anchor count is a boundary subset and eight memory current-input rows overlap retries; no hardware/16bit-stack/segmentation/atomic multiaccess rollback/performance/exhaustiveISA/PE/browser/SDK/game/hostedCI claim',
  ]};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2), {flag: 'wx'});
console.log(JSON.stringify({status: 'ok', ...counts, modules: modules.length, output}));

