import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../support/engine.mjs';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root);
const engine = readEngine(enginePath), SIZE = 4364, PC = 0x1000, DATA = 0x5000;
const initial = readFileSync(join(output, 'initial-arena.bin'));
assert.equal(initial.length, SIZE);
assert.deepEqual(WebAssembly.Module.imports(engine.module), []);
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const paths = [
  'engine/src/abi/arena.rs', 'engine/src/abi/memory_helper.rs',
  'engine/src/cpu/x86/ir.rs', 'engine/src/cpu/x86/decode/lower.rs',
  'engine/src/cpu/x86/decode/operands.rs', 'engine/src/cpu/x86/decode/integer.rs', 'engine/src/cpu/x86/decode/profile.rs', 'engine/src/cpu/dbt/region.rs',
  'engine/src/cpu/dbt/wasm/integer.rs', 'engine/src/cpu/dbt/wasm/memory.rs', 'engine/src/cpu/dbt/wasm/memory/word_store.rs',
  'engine/src/cpu/dbt/wasm/memory/narrow.rs', 'engine/src/memory/space.rs',
  'engine/src/process/instance.rs', 'engine/src/process/resident.rs',
  'engine/tests/cpu_word_memory_xadd_wasm.rs',
  'engine/tests/fixtures/p2-word-memory-xadd/run.mjs',
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
function run(c, module, budget, next, {label, reason = 1, retired = 1, fault, helperVersion = 4, helperValue = 0, status = 0, exitVersion = 2} = {}) {
  const expected = arena(c);
  if (status === 0) {
    expected.set(state(next)); expected.set(exit(reason, retired, fault, exitVersion), 56);
    if (helperVersion !== null) expected.set(helper(helperVersion, helperValue, fault), 100);
  }
  assert.equal(module.run(c.base, c.base + 56, budget, c.base + 96), status, label);
  assert.deepEqual(arena(c), expected, 'full4364 CPU/exit/helper/FP128 effect: ' + label);
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
  assert.equal(c.api.open(8, c.id, 0x57584144), 0); c.base = c.api.arena_ptr() >>> 0; input(c, 0, initial);
  for (const args of [[PC, 1, 7], [DATA, 2, 3], [0xa53c5000, 1, 3], [0xfffff000, 1, 3]]) host(c, 'map', args);
  return c;
}

// widened signed/unsigned arithmetic models ADD16 without following the emitted bit tests.
function added(a, b, oldFlags) {
  const wide = a + b, result = wide % 65536, signed = x => x < 32768 ? x : x - 65536;
  const total = signed(a) + signed(b), even = (result % 256).toString(2).replaceAll('0', '').length % 2 === 0;
  return {result, flags: 2 + (oldFlags & 0x400) + Number(wide >= 65536) + Number(even) * 4
    + Number(a % 16 + b % 16 >= 16) * 0x10 + Number(result === 0) * 0x40
    + Number(result >= 32768) * 0x80 + Number(total < -32768 || total > 32767) * 0x800};
}
const anchors = [
  {a: 0, b: 0, result: 0, flags: 0x46},
  {a: 0, b: 1, result: 1, flags: 2},
  {a: 1, b: 2, result: 3, flags: 6},
  {a: 0xffff, b: 1, result: 0, flags: 0x57},
  {a: 0x7fff, b: 1, result: 0x8000, flags: 0x896},
  {a: 0x8000, b: 0x8000, result: 0, flags: 0x847},
  {a: 15, b: 1, result: 16, flags: 0x12},
  {a: 0x78ab, b: 0xcdef, result: 0x469a, flags: 0x17},
];
for (const row of anchors) assert.deepEqual(added(row.a, row.b, 2),
  {result: row.result, flags: row.flags}, 'literal ADD16 carry/overflow/auxiliary/parity anchor');
const lowWord = (parent, value) => Math.floor(parent / 65536) * 65536 + value;
function instruction(source, tail) {return Buffer.from([0x66, 0x0f, 0xc1, tail[0] | source << 3, ...tail.slice(1)]);}
function spec(source, address = DATA + 8) {return {source, address, bytes: instruction(source, [5, ...words([address])])};}
function exchange(c, module, row, a, label, reason = 1) {
  const before = cpu(c), expected = added(a, before.registers[row.source] % 65536, before.flags);
  const next = {...before, registers: [...before.registers], pc: before.pc + row.bytes.length, flags: expected.flags};
  next.registers[row.source] = lowWord(before.registers[row.source], a);
  run(c, module, 1, next, {label, reason}); checkRam(c, row.address, word(expected.result)); return expected;
}

for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries), rows = Array.from({length: 8}, (_, source) => spec(source));
  const module = compile(c, rows.map((row, i) => block(PC + i * 16, row.bytes)));
  for (const [i, row] of rows.entries()) for (const anchor of anchors) for (const flags of [2, 0xcd7]) {
    const r = registers(); r[row.source] = lowWord(r[row.source], anchor.b);
    upload(c, row.address - 1, Buffer.concat([Buffer.from([0x55]), word(anchor.a), Buffer.from([0xaa])]));
    seed(c, PC + i * 16, r, flags);
    assert.deepEqual(exchange(c, module, row, anchor.a, 'all8 sources/literal ADD16/current incoming CF ignored'),
      {result: anchor.result, flags: anchor.flags + (flags & 0x400)});
    checkRam(c, row.address - 1, Buffer.from([0x55])); checkRam(c, row.address + 2, Buffer.from([0xaa])); counts.matrix++;
  }
  assert.equal(c.api.close(), 0);
}

// source GPRs remain unchanged until the checked store has used the original effective address.
for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries), address = 0xa53c5008, aliases = [];
  for (let source = 0; source < 8; source++) {
    const r = registers(); r[source] = address;
    const tail = source === 4 ? [4, 0x24] : source === 5 ? [0x45, 0] : [source];
    aliases.push({source, address, registers: r, bytes: instruction(source, tail)});
  }
  for (const source of [0, 1, 2, 3, 5, 6, 7]) {
    const r = registers(); r[source] = 0x40000002;
    aliases.push({source, address, registers: r, bytes: instruction(source, [4, 0x85 | source << 3, ...words([0xa53c5000])])});
  }
  const r = registers(); r[0] = 0x20000001;
  aliases.push({source: 0, address, registers: r, bytes: instruction(0, [0x84, 0x80, ...words([0x053c5003])])});
  assert.equal(aliases.length, 16);
  for (let start = 0; start < aliases.length; start += 8) {
    const rows = aliases.slice(start, start + 8), module = compile(c, rows.map((row, i) => block(PC + i * 16, row.bytes)));
    for (const [i, row] of rows.entries()) {
      upload(c, address - 1, Buffer.from([0x55, 1, 0x80, 0xaa])); seed(c, PC + i * 16, row.registers);
      exchange(c, module, row, 0x8001, 'full32 source-as-base/index/both/ESP/EBP; high16 retained');
      checkRam(c, address - 1, Buffer.from([0x55])); checkRam(c, address + 2, Buffer.from([0xaa])); counts.addresses++;
    }
  }
  const boundaries = [spec(0, DATA + 4095), spec(4, DATA + 4095), spec(5, 0xfffffffe), spec(7, 0xfffffffe)];
  const boundaryModule = compile(c, boundaries.map((row, i) => block(PC + 0x200 + i * 16, row.bytes)));
  for (const [i, row] of boundaries.entries()) {
    const r = registers(); r[row.source] = lowWord(r[row.source], i % 2 ? 0x8000 : 1);
    upload(c, row.address, word(0x800f)); seed(c, PC + 0x200 + i * 16, r);
    exchange(c, boundaryModule, row, 0x800f, 'unaligned crosspage/last valid WORD with source writeback'); counts.boundaries++;
  }
  assert.equal(c.api.close(), 0);
}

for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries), row = {source: 0, bytes: instruction(0, [7])};
  const mov = Buffer.from([0xbb, 0x44, 0x33, 0x22, 0x11]);
  const low = spec(0), high = DATA + 10;
  const movAx = Buffer.from([0x66, 0xb8, 1, 0]), setb = Buffer.from([0x0f, 0x92, 0xc2]);
  const adc = Buffer.concat([Buffer.from([0x66, 0x83, 0x15]), words([high]), Buffer.from([0])]);
  const cmp = Buffer.from([0x66, 0x3d, 0xff, 0xff]), setz = Buffer.from([0x0f, 0x94, 0xc3]);
  const code = Buffer.concat([movAx, low.bytes, setb, adc, cmp, setz]);
  const module = compile(c, [block(PC, Buffer.concat([mov, row.bytes])), block(PC + 0x100, code)]);
  const r = registers(); r[7] = 0x7008;
  seed(c, PC + 5, r);
  run(c, module, 0, cpu(c), {label: 'zero budget avoids unmapped XADD', retired: 0, helperVersion: null});
  input(c, 96, words([1]));
  run(c, module, 1, cpu(c), {label: 'cancel avoids unmapped XADD', reason: 2, retired: 0, helperVersion: null});
  input(c, 96, words([0])); counts.controls += 2;

  for (const kind of ['unmapped', 'read-denied', 'write-denied', 'cross-unmapped', 'cross-read-denied', 'cross-write-denied', 'overflow']) {
    const crossing = kind.startsWith('cross'), overflow = kind === 'overflow', address = overflow ? 0xffffffff : crossing ? 0x7fff : 0x7008;
    const missing = kind.includes('unmapped'), readDenied = kind.includes('read-denied'), writeDenied = kind.includes('write-denied');
    const mapped = overflow ? 0 : crossing && !missing ? 2 : missing && !crossing ? 0 : 1;
    const unchanged = Buffer.from(crossing && missing ? [0x55, 15] : [0x55, 15, 0x80, 0xaa]);
    if (mapped) {host(c, 'map', [0x7000, mapped, 3]); upload(c, crossing ? 0x7ffe : 0x7007, unchanged);}
    if (readDenied || writeDenied) host(c, 'protect', [crossing ? 0x8000 : 0x7000, 1, readDenied ? 2 : 1]);
    if (overflow) upload(c, 0xfffffffe, Buffer.from([0x55, 15]));
    const r = registers(); r[0] = lowWord(r[0], 1); r[7] = address;
    const before = seed(c, PC, r), stopped = {...before, registers: [...before.registers], pc: PC + mov.length}; stopped.registers[3] = 0x11223344;
    const fault = {detail: overflow ? 3 : missing ? 1 : 2, address: overflow ? address : crossing ? 0x8000 : address, access: writeDenied ? 2 : 1};
    const helperVersion = writeDenied ? 4 : 2;
    run(c, module, 2, stopped, {label: kind + ' after MOV; source and all FLAGS uncommitted', reason: 5, retired: 1, fault, helperVersion});
    run(c, module, 1, stopped, {label: kind + ' unchanged XADD retry', reason: 5, retired: 0, fault, helperVersion}); counts.faults += 2;
    if (!readDenied && mapped) checkRam(c, crossing ? 0x7ffe : 0x7007, unchanged);
    if (overflow) checkRam(c, 0xfffffffe, Buffer.from([0x55, 15]));
    if (!overflow) {
      if (missing) host(c, 'map', [crossing ? 0x8000 : 0x7000, 1, 3]);
      else host(c, 'protect', [crossing ? 0x8000 : 0x7000, 1, 3]);
      assert.deepEqual(cpu(c), stopped, 'map/protect-only repair preserves stopped source/current CPU');
      if (mapped) checkRam(c, crossing ? 0x7ffe : 0x7007, unchanged);
      const a = missing ? crossing ? 15 : 0 : 0x800f; checkRam(c, address, word(a));
      exchange(c, module, {...row, address}, a, 'same XADD resumes with source exchange and no MOV replay'); counts.retries++;
      if (mapped) {
        checkRam(c, crossing ? 0x7ffe : 0x7007, Buffer.from([0x55]));
        if (!(crossing && missing)) checkRam(c, address + 2, Buffer.from([0xaa]));
      }
      host(c, 'unmap', [0x7000, crossing ? 2 : 1]);
    }
  }
  const changedR = registers(); changedR[0] = lowWord(changedR[0], 1); changedR[7] = DATA + 8;
  upload(c, DATA + 7, Buffer.from([0x55, 0x34, 0x12, 0xaa])); host(c, 'protect', [DATA, 1, 1]);
  const before = seed(c, PC, changedR), stopped = {...before, registers: [...before.registers], pc: PC + mov.length}; stopped.registers[3] = 0x11223344;
  const fault = {detail: 2, address: DATA + 8, access: 2};
  run(c, module, 2, stopped, {label: 'source1/current WORD1234 write faults after MOV', reason: 5, retired: 1, fault});
  run(c, module, 1, stopped, {label: 'unchanged current-input XADD retry', reason: 5, retired: 0, fault}); counts.faults += 2;
  checkRam(c, DATA + 7, Buffer.from([0x55, 0x34, 0x12, 0xaa])); host(c, 'protect', [DATA, 1, 3]); upload(c, DATA + 8, word(0x8000));
  const current = {...stopped, registers: [...stopped.registers]}; current.registers[0] = lowWord(stopped.registers[0], 0x7fff);
  input(c, 16, words([current.registers[0]])); assert.deepEqual(cpu(c), current, 'declared source.low16 edit preserves remaining stopped CPU');
  assert.deepEqual(exchange(c, module, {...row, address: DATA + 8}, 0x8000, 'retry consumes explicitly changed current WORD and source'), {result: 0xffff, flags: 0x486});
  checkRam(c, DATA + 7, Buffer.from([0x55])); checkRam(c, DATA + 10, Buffer.from([0xaa])); counts.retries++; counts.current_inputs++;

  upload(c, DATA + 7, Buffer.from([0x55, 0xff, 0xff, 2, 0, 0xaa])); seed(c, PC + 0x100, registers(), 0xcd6);
  let old = cpu(c), next = {...old, registers: [...old.registers], pc: old.pc + movAx.length}; next.registers[0] = lowWord(old.registers[0], 1);
  run(c, module, 1, next, {label: 'guest MOV AX1 preserves high16 before live consumer', helperVersion: null});
  assert.deepEqual(exchange(c, module, low, 0xffff, 'XADD loFFFF returns old FFFF ticket and current CF1'), {result: 0, flags: 0x457});
  old = cpu(c); next = {...old, registers: [...old.registers], pc: old.pc + setb.length}; next.registers[2] = ((old.registers[2] & 0xffffff00) | 1) >>> 0;
  run(c, module, 1, next, {label: 'SETB DL consumes carry without touching AX ticket', helperVersion: null});
  old = cpu(c);
  run(c, module, 1, {...old, pc: old.pc + adc.length, flags: 0x406}, {label: 'ADC hi2,0 consumes live CF1 and stores3'});
  old = cpu(c);
  run(c, module, 1, {...old, pc: old.pc + cmp.length, flags: 0x446}, {label: 'CMP AX,FFFF consumes returned old memory ticket', helperVersion: null});
  old = cpu(c); next = {...old, registers: [...old.registers], pc: old.pc + setz.length}; next.registers[3] = ((old.registers[3] & 0xffffff00) | 1) >>> 0;
  run(c, module, 1, next, {label: 'SETZ BL observes unchanged live old-value ticket', helperVersion: null});
  checkRam(c, DATA + 7, Buffer.from([0x55, 0, 0, 3, 0, 0xaa])); counts.consumers++;
  assert.equal(c.api.close(), 0);
}

for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) for (const sourceValue of [0, 1]) {
  const c = open(owner, entries), row = {source: 0, address: PC, bytes: instruction(0, [7])};
  const r = registers(); r[0] = lowWord(r[0], sourceValue); r[7] = PC;
  const module = compile(c, [block(PC, row.bytes)]); seed(c, PC, r);
  const a = row.bytes.readUInt16LE(), expected = exchange(c, module, row, a, 'code XADD commits source exchange/FLAGS/one retirement', 6);
  assert.deepEqual({a, ...expected}, {a: 0x0f66, result: 0x0f66 + sourceValue, flags: sourceValue ? 0x402 : 0x406});
  assert.equal(cpu(c).registers[0], lowWord(r[0], 0x0f66), 'even source0/same-WORD write exchanges the old code WORD');
  run(c, module, 1, cpu(c), {label: 'stale XADD owner completely neutral', status: 4, retired: 0});
  const nextPC = PC + row.bytes.length, tail = compile(c, [{pc: nextPC, bytes: Buffer.from([0xeb, 0]), memory: false}]);
  run(c, tail, 2, {...cpu(c), pc: nextPC + 2}, {label: 'fresh suffix continues without XADD replay', reason: 3, retired: 1, helperVersion: null, exitVersion: 1});
  checkRam(c, PC, word(expected.result)); assert.equal(c.api.close(), 0); counts.self_code++;
}
assert.deepEqual(sources(), sourcePins, 'risk-relevant sources stayed fixed');
assert.equal(counts.contexts, 20); assert.equal(counts.runs, 740); assert.equal(counts.retired, 692);
assert.equal(counts.matrix, 512); assert.equal(counts.addresses, 64);
assert.equal(counts.boundaries, 16); assert.equal(counts.faults, 64); assert.equal(counts.retries, 28);
assert.equal(counts.current_inputs, 4); assert.equal(counts.consumers, 4); assert.equal(counts.controls, 8);
assert.equal(counts.self_code, 8); assert.equal(modules.length, 36);
const result = {status: 'ok', engine_sha256: engine.sha256, source_pins: sourcePins, counts, modules,
  limits: ['finite exact single66 WORD memory XADD 0F C1 /r/all8 sources in four bound profiles',
    'ADD16 ignores incoming carry and defines CF/PF/AF/ZF/SF/OF while retaining DF',
    'source.low16 receives old memory only after checked store; high16 and other GPRs retain authored values',
    'real read16/store16 paths and selected RAM/canary checks; no arbitrary-host rollback or LOCK/thread atomicity claim',
    'full4364 arena preserves FP128; effective address uses old source and explicit retry consumes current inputs',
    'no exhaustive ISA, hardware, throughput, retained PE, browser or game claim']};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2) + '\n', {flag: 'wx'});
console.log(JSON.stringify({status: result.status, ...counts, modules: modules.length, output}));
