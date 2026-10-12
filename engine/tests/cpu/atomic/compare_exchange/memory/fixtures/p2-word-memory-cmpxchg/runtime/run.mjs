import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../../../../../../../support/wasm/engine.mjs';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root);
const engine = readEngine(enginePath), SIZE = 4612, PC = 0x1000, DATA = 0x5000;
const initial = readFileSync(join(output, 'initial-arena.bin'));
assert.equal(initial.length, SIZE);
assert.deepEqual(WebAssembly.Module.imports(engine.module), []);
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const paths = [
  'engine/src/abi/arena.rs', 'engine/src/abi/memory_helper.rs',
  'engine/src/cpu/x86/ir.rs', 'engine/src/cpu/x86/decode/lower.rs',
  'engine/src/cpu/x86/decode/operands.rs', 'engine/src/cpu/x86/decode/integer.rs', 'engine/src/cpu/x86/decode/profile.rs', 'engine/src/cpu/dbt/region.rs',
  'engine/src/cpu/dbt/wasm/integer/arithmetic.rs', 'engine/src/cpu/dbt/wasm/integer/bits.rs', 'engine/src/cpu/dbt/wasm/integer/carry_rotates.rs', 'engine/src/cpu/dbt/wasm/integer/decimal.rs', 'engine/src/cpu/dbt/wasm/integer/dispatch.rs', 'engine/src/cpu/dbt/wasm/integer/flags.rs', 'engine/src/cpu/dbt/wasm/integer/mod.rs', 'engine/src/cpu/dbt/wasm/integer/multiply_divide.rs', 'engine/src/cpu/dbt/wasm/integer/operands.rs', 'engine/src/cpu/dbt/wasm/integer/rotates.rs', 'engine/src/cpu/dbt/wasm/integer/shifts.rs', 'engine/src/cpu/dbt/wasm/memory.rs', 'engine/src/cpu/dbt/wasm/memory/word_store.rs',
  'engine/src/cpu/dbt/wasm/memory/narrow.rs', 'engine/src/memory/space.rs',
  'engine/src/process/instance.rs', 'engine/src/process/execution/compiled.rs', 'engine/src/process/execution/authority.rs', 'engine/src/process/memory/address_space.rs', 'engine/src/process/memory/scalar.rs', 'engine/src/process/memory/scalar/tests.rs', 'engine/src/process/execution/mod.rs', 'engine/src/process/memory/mod.rs', 'engine/src/process/mod.rs', 'engine/src/process/execution/resident.rs', 'engine/src/process/execution/authority.rs', 'engine/src/process/execution/mod.rs', 'engine/src/process/mod.rs',
  'engine/tests/cpu/atomic/compare_exchange/memory/wasm/word.rs', 'engine/tests/cpu/atomic/wasm.rs', 'engine/tests/support/wasm.rs', 'engine/tests/cpu/atomic/wasm.rs', 'engine/tests/support/wasm.rs',
  'engine/tests/cpu/atomic/compare_exchange/memory/fixtures/p2-word-memory-cmpxchg/runtime/run.mjs',
];
const sources = () => Object.fromEntries(paths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = sources(), modules = [];
const counts = {contexts: 0, runs: 0, retired: 0, matrix: 0, addresses: 0, boundaries: 0, faults: 0, retries: 0, current_inputs: 0, consumers: 0, controls: 0, self_code: 0};
const word = value => Buffer.from([value & 255, value >>> 8 & 255]);
const words = values => {const b = Buffer.alloc(values.length * 4); values.forEach((v, i) => b.writeUInt32LE(v >>> 0, i * 4)); return b;};
function record(magic, size, fields, version = 1) {
  const b = Buffer.alloc(size); b.write(magic); b.writeUInt16LE(version, 4);
  b.writeUInt16LE(1, 6); b.writeUInt32LE(size, 8);
  fields.forEach((v, i) => b.writeUInt32LE(v >>> 0, 16 + i * 4)); return b;
}
const state = c => record('R3ST', 56, [...c.registers, c.pc, c.flags]);
const exit = (reason, n, f, version = 2) => record('R3EX', 40,
  [reason, n, f?.detail ?? 0, f?.address ?? 0, f?.access ?? 0, f ? 2 : 0], reason === 11 ? 6 : version);
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
function run(c, module, budget, next, {label, reason = 1, retired = 1, fault, helperVersion = 4, helperValue = 0, status = 0, exitVersion = 2} = {}) {
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
  for (const block of blocks) upload(c, block.pc, block.bytes);
  input(c, 140, words(blocks.flatMap(b => c.entries ? [b.pc] : [b.pc, b.bytes.length])));
  const method = c.owner === 'resident' ? (c.entries ? 'compile_resident_entries' : 'compile_resident') : (c.entries ? 'compile_entries' : 'compile');
  host(c, method, c.entries ? [blocks.length, 0] : [blocks.length]);
  const b = arena(c), binding = c.owner === 'resident'
    ? {low: b.readUInt32LE(148), high: b.readUInt32LE(152), pointer: b.readUInt32LE(156), length: b.readUInt32LE(160)}
    : {generation: c.api.generation(), pointer: c.api.module_ptr() >>> 0, length: c.api.module_len() >>> 0};
  const bytes = Buffer.from(new Uint8Array(c.memory.buffer, binding.pointer, binding.length));
  const module = new WebAssembly.Module(bytes), imports = WebAssembly.Module.imports(module);
  const memoryOps = blocks.some(b => b.memory !== false);
  const names = [c.owner === 'resident' ? 'guard_resident' : 'guard', ...(memoryOps ? ['read16', c.owner === 'resident' ? 'store_resident16' : 'store16'] : [])];
  assert.deepEqual(imports, [{module: 'env', name: 'memory', kind: 'memory'}, ...names.map(name => ({module: 'ring3', name, kind: 'function'}))]);
  assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  const file = `module-${modules.length + 1}.wasm`; writeFileSync(join(output, file), bytes, {flag: 'wx'});
  modules.push({context: c.id, owner: c.owner, entries: c.entries, file, sha256: hash(bytes), imports: names});
  return {run: new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3: c.api}).exports.run};
}
const block = (pc, instruction) => ({pc, bytes: Buffer.concat([instruction, Buffer.from([0xeb, 0])])});
function open(owner, entries) {
  const instance = new WebAssembly.Instance(engine.module, {});
  const c = {id: ++counts.contexts, owner, entries, memory: instance.exports.memory, api: {}};
  for (const name of ['open', 'close', 'arena_ptr', 'map', 'unmap', 'protect', 'upload', 'read8', 'read16', 'store16', 'store_resident16',
    'compile', 'compile_entries', 'compile_resident', 'compile_resident_entries', 'generation', 'module_ptr', 'module_len', 'guard', 'guard_resident'])
    c.api[name] = instance.exports['ring3_abi_v1_' + name];
  assert.equal(c.api.open(8, c.id, 0x57434d50), 0); c.base = c.api.arena_ptr() >>> 0; input(c, 0, initial);
  for (const args of [[PC, 1, 7], [DATA, 2, 3], [0xa53c5000, 1, 3], [0xfffff000, 1, 3]]) host(c, 'map', args);
  return c;
}

// widened subtraction separates signed overflow, unsigned borrow and low-byte parity.
function subtracted(a, b, oldFlags) {
  const wide = a - b, result = (wide + 65536) % 65536, signed = x => x < 32768 ? x : x - 65536;
  const total = signed(a) - signed(b), even = (result % 256).toString(2).replaceAll('0', '').length % 2 === 0;
  return {result, flags: 2 + (oldFlags & 0x400) + Number(a < b) + Number(even) * 4
    + Number(a % 16 < b % 16) * 0x10 + Number(result === 0) * 0x40
    + Number(result >= 32768) * 0x80 + Number(total < -32768 || total > 32767) * 0x800};
}
const anchors = [
  {a: 0, b: 0, result: 0, flags: 0x46},
  {a: 0x7fff, b: 0x7fff, result: 0, flags: 0x46},
  {a: 0, b: 1, result: 0xffff, flags: 0x97},
  {a: 0x8000, b: 1, result: 0x7fff, flags: 0x816},
  {a: 0x7fff, b: 0xffff, result: 0x8000, flags: 0x887},
  {a: 0xffff, b: 1, result: 0xfffe, flags: 0x82},
  {a: 16, b: 1, result: 15, flags: 0x16},
  {a: 0x78ab, b: 0xcdef, result: 0xaabc, flags: 0x893},
];
for (const row of anchors) assert.deepEqual(subtracted(row.a, row.b, 2),
  {result: row.result, flags: row.flags}, 'literal SUB16 borrow/overflow/auxiliary/parity anchor');
const lowWord = (parent, value) => Math.floor(parent / 65536) * 65536 + value;
function instruction(source, tail) {return Buffer.from([0x66, 0x0f, 0xb1, tail[0] | source << 3, ...tail.slice(1)]);}
function spec(source, address = DATA + 8) {return {source, address, bytes: instruction(source, [5, ...words([address])])};}
function compare(c, module, row, b, label, reason = 1) {
  const before = cpu(c), a = before.registers[0] % 65536, equal = a === b;
  const expected = subtracted(a, b, before.flags), stored = equal ? before.registers[row.source] % 65536 : b;
  const next = {...before, registers: [...before.registers], pc: before.pc + row.bytes.length, flags: expected.flags};
  if (!equal) next.registers[0] = lowWord(before.registers[0], b);
  run(c, module, 1, next, {label, reason}); checkRam(c, row.address, word(stored)); return {...expected, equal, stored};
}

for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries), rows = Array.from({length: 8}, (_, source) => spec(source));
  const module = compile(c, rows.map((row, i) => block(PC + i * 16, row.bytes)));
  for (const [i, row] of rows.entries()) for (const anchor of anchors) for (const flags of [2, 0xcd7]) {
    const r = registers(), sourceValue = 0x2468 + row.source * 257; r[0] = lowWord(r[0], anchor.a);
    if (row.source !== 0) r[row.source] = lowWord(r[row.source], sourceValue);
    upload(c, row.address - 1, Buffer.concat([Buffer.from([0x55]), word(anchor.b), Buffer.from([0xaa])])); seed(c, PC + i * 16, r, flags);
    const equal = anchor.a === anchor.b, stored = equal ? row.source === 0 ? anchor.a : sourceValue : anchor.b;
    assert.deepEqual(compare(c, module, row, anchor.b, 'all8 sources/both comparisons/high16/SUB16 ignores incoming CF'),
      {result: anchor.result, flags: anchor.flags + (flags & 0x400), equal, stored});
    checkRam(c, row.address - 1, Buffer.from([0x55])); checkRam(c, row.address + 2, Buffer.from([0xaa])); counts.matrix++;
  }
  assert.equal(c.api.close(), 0);
}

// both comparisons use the original EAX/source effective address before any AX writeback.
for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries), address = 0xa53c5008, aliases = [];
  for (let source = 0; source < 8; source++) {
    const r = registers(); r[0] = lowWord(r[0], 0x1234); r[source] = address;
    const tail = source === 4 ? [4, 0x24] : source === 5 ? [0x45, 0] : [source];
    aliases.push({source, address, registers: r, bytes: instruction(source, tail)});
  }
  for (const source of [1, 6]) {
    const r = registers(); r[0] = lowWord(r[0], 0x1234); r[source] = 0x40000002;
    aliases.push({source, address, registers: r, bytes: instruction(source, [4, 0x85 | source << 3, ...words([0xa53c5000])])});
  }
  const sourceBoth = registers(); sourceBoth[0] = lowWord(sourceBoth[0], 0x1234); sourceBoth[3] = 0x20000001;
  aliases.push({source: 3, address, registers: sourceBoth, bytes: instruction(3, [0x84, 0x9b, ...words([0x053c5003])])});
  for (const [accumulator, tail] of [[address, [0]], [0x40000002, [4, 0x85, ...words([0xa53c5000])]],
    [0x20000001, [0x84, 0x80, ...words([0x053c5003])]]]) {
    const r = registers(); r[0] = accumulator; r[1] = lowWord(r[1], 0x2468);
    aliases.push({source: 1, address, registers: r, bytes: instruction(1, tail)});
  }
  assert.equal(aliases.length, 14);
  for (let start = 0; start < aliases.length; start += 8) {
    const rows = aliases.slice(start, start + 8), module = compile(c, rows.map((row, i) => block(PC + i * 16, row.bytes)));
    for (const [i, row] of rows.entries()) for (const equal of [true, false]) {
      const b = equal ? row.registers[0] % 65536 : 0x8001;
      upload(c, address - 1, Buffer.concat([Buffer.from([0x55]), word(b), Buffer.from([0xaa])])); seed(c, PC + i * 16, row.registers);
      assert.equal(compare(c, module, row, b, 'source and accumulator full32 base/index/both/ESP/EBP aliases').equal, equal);
      checkRam(c, address - 1, Buffer.from([0x55])); checkRam(c, address + 2, Buffer.from([0xaa])); counts.addresses++;
    }
  }
  const boundaries = [spec(0, DATA + 4095), spec(4, DATA + 4095), spec(5, 0xfffffffe), spec(7, 0xfffffffe)];
  const boundaryModule = compile(c, boundaries.map((row, i) => block(PC + 0x200 + i * 16, row.bytes)));
  for (const [i, row] of boundaries.entries()) {
    const r = registers(); r[0] = lowWord(r[0], i % 2 ? 0x7fff : 0x800f);
    if (row.source !== 0) r[row.source] = lowWord(r[row.source], i % 2 ? 0xffff : 1);
    upload(c, row.address, word(0x800f)); seed(c, PC + 0x200 + i * 16, r);
    compare(c, boundaryModule, row, 0x800f, 'crosspage/last valid WORD equality and mismatch write cycles'); counts.boundaries++;
  }
  assert.equal(c.api.close(), 0);
}

for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries), row = {source: 1, bytes: instruction(1, [7])}, mov = Buffer.from([0xbb, 0x44, 0x33, 0x22, 0x11]);
  const current = spec(1), currentAx = spec(0);
  const successFlags = Buffer.from([0x0f, 0x94, 0xc2, 0x0f, 0x90, 0xc3]);
  const mismatchFlags = Buffer.from([0x0f, 0x94, 0xc2, 0x0f, 0x92, 0xc3]);
  const cmp = Buffer.from([0x66, 0x3d, 0x22, 0x22]), setDh = Buffer.from([0x0f, 0x94, 0xc6]), setBh = Buffer.from([0x0f, 0x94, 0xc7]);
  const code = Buffer.concat([current.bytes, successFlags, current.bytes, mismatchFlags, cmp, setDh, currentAx.bytes, setBh]);
  const module = compile(c, [block(PC, Buffer.concat([mov, row.bytes])), block(PC + 0x100, code)]);
  const r = registers(); r[7] = 0x7008;
  seed(c, PC + 5, r);
  run(c, module, 0, cpu(c), {label: 'zero budget avoids unmapped CMPXCHG', retired: 0, helperVersion: null});
  input(c, 96, words([1]));
  run(c, module, 1, cpu(c), {label: 'cancel avoids unmapped CMPXCHG', reason: 2, retired: 0, helperVersion: null});
  input(c, 96, words([0])); counts.controls += 2;

  for (const equal of [true, false]) for (const kind of ['unmapped', 'read-denied', 'write-denied', 'cross-unmapped', 'cross-read-denied', 'cross-write-denied', 'overflow']) {
    const crossing = kind.startsWith('cross'), overflow = kind === 'overflow', address = overflow ? 0xffffffff : crossing ? 0x7fff : 0x7008;
    const missing = kind.includes('unmapped'), readDenied = kind.includes('read-denied'), writeDenied = kind.includes('write-denied');
    const mapped = overflow ? 0 : crossing && !missing ? 2 : missing && !crossing ? 0 : 1;
    const unchanged = Buffer.from(crossing && missing ? [0x55, 15] : [0x55, 15, 0x80, 0xaa]);
    if (mapped) {host(c, 'map', [0x7000, mapped, 3]); upload(c, crossing ? 0x7ffe : 0x7007, unchanged);}
    if (readDenied || writeDenied) host(c, 'protect', [crossing ? 0x8000 : 0x7000, 1, readDenied ? 2 : 1]);
    if (overflow) upload(c, 0xfffffffe, Buffer.from([0x55, 15]));
    const b = missing ? crossing ? 15 : 0 : 0x800f;
    const r = registers(); r[0] = lowWord(r[0], equal ? b : b ^ 0xffff); r[1] = lowWord(r[1], 0x2468); r[7] = address;
    const before = seed(c, PC, r), stopped = {...before, registers: [...before.registers], pc: PC + mov.length}; stopped.registers[3] = 0x11223344;
    // Ring3 rejects the overflowing logical WORD span before RAM access.
    const fault = overflow ? undefined : {detail: missing ? 1 : 2, address: crossing ? 0x8000 : address, access: writeDenied ? 2 : 1};
    const helperVersion = overflow ? null : writeDenied ? 4 : 2;
    run(c, module, 2, stopped, {label: kind + '/declared equal=' + equal + ' after MOV; AX/source/FLAGS uncommitted', reason: overflow ? 11 : 5, retired: 1, fault, helperVersion});
    run(c, module, 1, stopped, {label: kind + ' unchanged CMPXCHG retry', reason: overflow ? 11 : 5, retired: 0, fault, helperVersion}); counts.faults += 2;
    if (!readDenied && mapped) checkRam(c, crossing ? 0x7ffe : 0x7007, unchanged);
    if (overflow) checkRam(c, 0xfffffffe, Buffer.from([0x55, 15]));
    if (!overflow) {
      if (missing) host(c, 'map', [crossing ? 0x8000 : 0x7000, 1, 3]);
      else host(c, 'protect', [crossing ? 0x8000 : 0x7000, 1, 3]);
      assert.deepEqual(cpu(c), stopped, 'map/protect-only repair preserves stopped accumulator/source/current CPU');
      if (mapped) checkRam(c, crossing ? 0x7ffe : 0x7007, unchanged);
      checkRam(c, address, word(b));
      assert.equal(compare(c, module, {...row, address}, b, 'same CMPXCHG resumes both checked writes without MOV replay').equal, equal); counts.retries++;
      if (mapped) {checkRam(c, crossing ? 0x7ffe : 0x7007, Buffer.from([0x55])); if (!(crossing && missing)) checkRam(c, address + 2, Buffer.from([0xaa]));}
      host(c, 'unmap', [0x7000, crossing ? 2 : 1]);
    }
  }
  for (const wasEqual of [true, false]) {
    const r = registers(); r[0] = lowWord(r[0], 0x1234); r[1] = lowWord(r[1], 0x4567); r[7] = DATA + 8;
    const b = wasEqual ? 0x1234 : 0x2345;
    upload(c, DATA + 7, Buffer.concat([Buffer.from([0x55]), word(b), Buffer.from([0xaa])])); host(c, 'protect', [DATA, 1, 1]);
    const before = seed(c, PC, r), stopped = {...before, registers: [...before.registers], pc: PC + mov.length}; stopped.registers[3] = 0x11223344;
    const fault = {detail: 2, address: DATA + 8, access: 2};
    run(c, module, 2, stopped, {label: 'initial equal=' + wasEqual + ' write denied after MOV', reason: 5, retired: 1, fault});
    run(c, module, 1, stopped, {label: 'unchanged explicit-input CMPXCHG retry', reason: 5, retired: 0, fault}); counts.faults += 2;
    checkRam(c, DATA + 7, Buffer.concat([Buffer.from([0x55]), word(b), Buffer.from([0xaa])]));
    host(c, 'protect', [DATA, 1, 3]); upload(c, DATA + 8, word(0x8000));
    const edited = {...stopped, registers: [...stopped.registers]};
    edited.registers[0] = lowWord(stopped.registers[0], wasEqual ? 0x7fff : 0x8000);
    edited.registers[1] = lowWord(stopped.registers[1], wasEqual ? 0x9abc : 0x7fff);
    input(c, 16, words(edited.registers.slice(0, 2))); assert.deepEqual(cpu(c), edited, 'declared current AX/source edits preserve remaining stopped CPU');
    assert.deepEqual(compare(c, module, {...row, address: DATA + 8}, 0x8000, 'retry consumes current WORD/AX/source and flips comparison'),
      wasEqual ? {result: 0xffff, flags: 0xc87, equal: false, stored: 0x8000} : {result: 0, flags: 0x446, equal: true, stored: 0x7fff});
    checkRam(c, DATA + 7, Buffer.from([0x55])); checkRam(c, DATA + 10, Buffer.from([0xaa])); counts.retries++; counts.current_inputs++;
  }

  const liveR = registers(); liveR[0] = lowWord(liveR[0], 0x1111); liveR[1] = lowWord(liveR[1], 0x2222);
  upload(c, DATA + 7, Buffer.from([0x55, 0x11, 0x11, 0xaa])); seed(c, PC + 0x100, liveR, 0xcd6);
  assert.deepEqual(compare(c, module, current, 0x1111, 'live success consumes sourceCX2222 and leaves AX1111'), {result: 0, flags: 0x446, equal: true, stored: 0x2222});
  let old = cpu(c), next = {...old, registers: [...old.registers], pc: old.pc + successFlags.length};
  next.registers[2] = ((old.registers[2] & 0xffffff00) | 1) >>> 0; next.registers[3] = (old.registers[3] & 0xffffff00) >>> 0;
  run(c, module, 2, next, {label: 'SETZ DL/SETO BL consume live matched FLAGS while preserving AX/source', retired: 2, helperVersion: null});
  assert.deepEqual(compare(c, module, current, 0x2222, 'repeat now mismatches current RAM2222 and returns AX2222'), {result: 0xeeef, flags: 0x493, equal: false, stored: 0x2222});
  old = cpu(c); next = {...old, registers: [...old.registers], pc: old.pc + mismatchFlags.length};
  next.registers[2] = (old.registers[2] & 0xffffff00) >>> 0; next.registers[3] = ((old.registers[3] & 0xffffff00) | 1) >>> 0;
  run(c, module, 2, next, {label: 'SETZ DL/SETB BL consume mismatch FLAGS without changing AX/source', retired: 2, helperVersion: null});
  old = cpu(c);
  run(c, module, 1, {...old, pc: old.pc + cmp.length, flags: 0x446}, {label: 'CMP AX,2222 consumes live old-memory accumulator ticket', helperVersion: null});
  old = cpu(c); next = {...old, registers: [...old.registers], pc: old.pc + setDh.length}; next.registers[2] = ((old.registers[2] & 0xffff00ff) | 0x100) >>> 0;
  run(c, module, 1, next, {label: 'SETZ DH confirms ticket while leaving AX/source untouched', helperVersion: null});
  assert.deepEqual(compare(c, module, currentAx, 0x2222, 'sourceAX follows current accumulator and performs matched identity write'), {result: 0, flags: 0x446, equal: true, stored: 0x2222});
  old = cpu(c); next = {...old, registers: [...old.registers], pc: old.pc + setBh.length}; next.registers[3] = ((old.registers[3] & 0xffff00ff) | 0x100) >>> 0;
  run(c, module, 1, next, {label: 'SETZ BH consumes sourceAX matched identity FLAGS', helperVersion: null});
  checkRam(c, DATA + 7, Buffer.from([0x55, 0x22, 0x22, 0xaa])); counts.consumers++;
  assert.equal(c.api.close(), 0);
}

for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) for (const kind of [
  {accumulator: 0x0f66, source: 0x0f67, stored: 0x0f67, flags: 0x446, equal: true},
  {accumulator: 0x0f66, source: 0x0f66, stored: 0x0f66, flags: 0x446, equal: true},
  {accumulator: 0x0f65, source: 0x0f67, stored: 0x0f66, flags: 0x497, equal: false},
]) {
  const c = open(owner, entries), row = {source: 1, address: PC, bytes: instruction(1, [7])};
  const r = registers(); r[0] = lowWord(r[0], kind.accumulator); r[1] = lowWord(r[1], kind.source); r[7] = PC;
  const module = compile(c, [block(PC, row.bytes)]); seed(c, PC, r);
  const a = row.bytes.readUInt16LE(), expected = compare(c, module, row, a, 'code CMPXCHG commits FLAGS/AX/one retirement on either comparison', 6);
  assert.equal(a, 0x0f66); assert.equal(expected.stored, kind.stored); assert.equal(expected.flags, kind.flags); assert.equal(expected.equal, kind.equal);
  assert.equal(cpu(c).registers[0], lowWord(r[0], 0x0f66)); assert.equal(cpu(c).registers[1], r[1]);
  run(c, module, 1, cpu(c), {label: 'stale CMPXCHG owner completely neutral', status: 4, retired: 0});
  const nextPC = PC + row.bytes.length, tail = compile(c, [{pc: nextPC, bytes: Buffer.from([0xeb, 0]), memory: false}]);
  run(c, tail, 2, {...cpu(c), pc: nextPC + 2}, {label: 'fresh suffix continues without CMPXCHG replay', reason: 3, retired: 1, helperVersion: null, exitVersion: 1});
  checkRam(c, PC, word(kind.stored)); assert.equal(c.api.close(), 0); counts.self_code++;
}
assert.deepEqual(sources(), sourcePins, 'risk-relevant sources stayed fixed');
assert.equal(counts.contexts, 24); assert.equal(counts.runs, 900); assert.equal(counts.retired, 824);
assert.equal(counts.matrix, 512); assert.equal(counts.addresses, 112); assert.equal(counts.boundaries, 16);
assert.equal(counts.faults, 128); assert.equal(counts.retries, 56); assert.equal(counts.current_inputs, 8);
assert.equal(counts.consumers, 4); assert.equal(counts.controls, 8); assert.equal(counts.self_code, 12); assert.equal(modules.length, 44);
const result = {status: 'ok', engine_sha256: engine.sha256, source_pins: sourcePins, counts, modules,
  limits: ['finite exact single66 WORD memory CMPXCHG 0F B1 /r/all8 sources in four bound profiles',
    'SUB16 flags compare original AX and memory; both comparisons perform checked stores, mismatch stores original WORD',
    'AX/source/EA aliases use old GPRs until checked store succeeds; high16 retained and sourceAX follows accumulator semantics',
    'read faults precede comparisons; equality labels on those cases describe authored inputs, no read-success claim',
    'real read16/store16 paths and selected RAM/canary checks; no arbitrary-host rollback or LOCK/thread atomicity claim',
    'full4612 arena preserves FP128; current retry inputs and live consumers require no implicit reseed',
    'no exhaustive ISA, hardware, throughput, retained PE, browser or game claim']};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2) + '\n', {flag: 'wx'});
console.log(JSON.stringify({status: result.status, ...counts, modules: modules.length, output}));
