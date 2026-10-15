import {pageFaultExit,pageFaultPacket} from '../../../../../../../support/wasm/page-fault.mjs';
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
  'engine/src/cpu/x86/ir.rs', 'engine/src/cpu/x86/decode/lower.rs',
  'engine/tests/cpu/multiply/signed/immediate/native/word.rs', 'engine/tests/cpu/multiply/native.rs', 'engine/tests/support/cpu/prefix/admission.rs', 'engine/tests/support/cpu/prefix/word.rs', 'engine/src/cpu/x86/decode/integer.rs', 'engine/src/cpu/x86/decode/profile.rs', 'engine/src/cpu/dbt/region.rs',
  'engine/src/cpu/dbt/wasm/integer/arithmetic.rs', 'engine/src/cpu/dbt/wasm/integer/bits.rs', 'engine/src/cpu/dbt/wasm/integer/carry_rotates.rs', 'engine/src/cpu/dbt/wasm/integer/decimal.rs', 'engine/src/cpu/dbt/wasm/integer/dispatch.rs', 'engine/src/cpu/dbt/wasm/integer/flags.rs', 'engine/src/cpu/dbt/wasm/integer/mod.rs', 'engine/src/cpu/dbt/wasm/integer/multiply_divide.rs', 'engine/src/cpu/dbt/wasm/integer/operands.rs', 'engine/src/cpu/dbt/wasm/integer/rotates.rs', 'engine/src/cpu/dbt/wasm/integer/shifts.rs', 'engine/src/cpu/dbt/wasm/memory.rs', 'engine/src/cpu/dbt/wasm/control.rs',
  'engine/src/cpu/dbt/wasm/memory/narrow.rs', 'engine/src/memory/space.rs', 'engine/src/memory/space/atomic.rs',
  'engine/src/process/instance.rs', 'engine/src/process/execution/compiled.rs', 'engine/src/process/execution/authority.rs', 'engine/src/process/memory/address_space.rs', 'engine/src/process/memory/scalar.rs', 'engine/src/process/memory/scalar/tests.rs', 'engine/src/process/execution/mod.rs', 'engine/src/process/memory/mod.rs', 'engine/src/process/mod.rs', 'engine/src/process/execution/resident.rs',
  'engine/tests/cpu/multiply/signed/immediate/wasm/word.rs', 'engine/tests/cpu/multiply/wasm.rs', 'engine/tests/support/wasm.rs',
  'engine/tests/cpu/multiply/signed/immediate/fixtures/p2-word-immediate-multiply/runtime/run.mjs', 'engine/tests/support/wasm/binding.mjs','engine/tests/support/wasm/page-fault.mjs'];
assert.equal(new Set(paths).size, paths.length);
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
function run(c, module, budget, next, {label, reason = 1, retired = 1, fault, helperVersion = null, helperValue = 0, status = 0, exitVersion = 2} = {}) {
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
      pageFault ? pageFaultExit({retired, ...pageFault}) : exit(reason, retired, undefined, module.memoryOps?6:exitVersion),
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
  const names = generatedImports({resident: c.owner === 'resident', helpers: [...(memoryOps ? ['cpu_read16'] : [])]}).slice(1).map(item => item.name);
  assert.deepEqual(imports, [{module: 'env', name: 'memory', kind: 'memory'}, ...names.map(name => ({module: 'ring3', name, kind: 'function'}))]);
  assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  const file = `module-${modules.length + 1}.wasm`; writeFileSync(join(output, file), bytes, {flag: 'wx'});
  modules.push({context: c.id, owner: c.owner, entries: c.entries, file, sha256: hash(bytes), imports: names});
  return {memoryOps, run: new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3: c.api}).exports.run};
}
const block = (pc, instruction, memory = true) => ({pc, memory, bytes: Buffer.concat([instruction, Buffer.from([0xeb, 0])])});
function open(owner, entries) {
  const instance = new WebAssembly.Instance(engine.module, {});
  const c = {id: ++counts.contexts, owner, entries, memory: instance.exports.memory, api: {}};
  for (const name of ['open', 'close', 'arena_ptr', 'map', 'unmap', 'protect', 'upload', 'read8', 'read16',
    'compile', 'compile_entries', 'compile_resident', 'compile_resident_entries', 'generation', 'module_ptr', 'module_len', 'guard', 'guard_resident'])
    c.api[name] = instance.exports['ring3_abi_v1_' + name];
 attachDetectionApi(c.api, instance.exports);
  for(const name of ["cpu_read16"]) {c.api[name]=instance.exports['ring3_abi_v1_'+name];assert.equal(typeof c.api[name],'function',name);}
  assert.equal(c.api.open(8, c.id, 0x57494d55), 0); c.base = c.api.arena_ptr() >>> 0; input(c, 0, initial);
  for (const args of [[PC, 1, 7], [DATA, 2, 3], [0xa53c5000, 1, 3], [0xfffff000, 1, 3]]) host(c, 'map', args);
  return c;
}

// widened signed arithmetic models the source factor independently of the old destination.
const signed16 = value => value < 32768 ? value : value - 65536;
function multiplied(source, immediate, oldFlags) {
  const product = signed16(source) * signed16(immediate), overflow = product < -32768 || product > 32767;
  return {value: (product % 65536 + 65536) % 65536, flags: (oldFlags & 0x402) + (overflow ? 0x801 : 0)};
}
const forms = ['imm16', 'imm8'];
function literal(form, raw, immediate = raw) {
  assert.equal(immediate, form === 'imm16' || raw < 128 ? raw : raw + 0xff00, 'authored normalized immediate');
  return {form, raw, immediate};
}
const anchors = [
  {...literal('imm16', 0x8000), source: 0, value: 0, flags: 0x402},
  {...literal('imm16', 0x0080), source: 1, value: 0x0080, flags: 0x402},
  {...literal('imm16', 0x00ff), source: 1, value: 0x00ff, flags: 0x402},
  {...literal('imm16', 0xff80), source: 0x0100, value: 0x8000, flags: 0x402},
  {...literal('imm16', 0xff80), source: 0x0101, value: 0x7f80, flags: 0xc03},
  {...literal('imm16', 0xffff), source: 0x8000, value: 0x8000, flags: 0xc03},
  {...literal('imm16', 2), source: 0x7fff, value: 0xfffe, flags: 0xc03},
  {...literal('imm16', 2), source: 0x8000, value: 0, flags: 0xc03},
  {...literal('imm8', 0x80, 0xff80), source: 1, value: 0xff80, flags: 0x402},
  {...literal('imm8', 0xff, 0xffff), source: 1, value: 0xffff, flags: 0x402},
  {...literal('imm8', 0x80, 0xff80), source: 0x0100, value: 0x8000, flags: 0x402},
  {...literal('imm8', 0x80, 0xff80), source: 0x0101, value: 0x7f80, flags: 0xc03},
  {...literal('imm8', 0xff, 0xffff), source: 0x8000, value: 0x8000, flags: 0xc03},
  {...literal('imm8', 2), source: 0x7fff, value: 0xfffe, flags: 0xc03},
  {...literal('imm8', 2), source: 0x8000, value: 0, flags: 0xc03},
  {...literal('imm8', 1), source: 0x8000, value: 0x8000, flags: 0x402},
];
for (const row of anchors) assert.deepEqual(multiplied(row.source, row.immediate, 0xcd7), {value: row.value, flags: row.flags}, 'literal signed immediate width/fit/truncated-zero anchor');
const cases = {
  imm16: [anchors[0], anchors[1], anchors[2], anchors[4]],
  imm8: [anchors[8], anchors[10], anchors[12], anchors[13]],
};
const lowWord = (parent, value) => Math.floor(parent / 65536) * 65536 + value;
const encodedImmediate = row => row.form === 'imm16' ? word(row.raw) : Buffer.from([row.raw]);
const registerInstruction = (destination, source, immediate) => Buffer.concat([Buffer.from([0x66, immediate.form === 'imm16' ? 0x69 : 0x6b, 0xc0 | destination << 3 | source]), encodedImmediate(immediate)]);
const memoryInstruction = (destination, tail, immediate) => Buffer.concat([Buffer.from([0x66, immediate.form === 'imm16' ? 0x69 : 0x6b, tail[0] | destination << 3, ...tail.slice(1)]), encodedImmediate(immediate)]);
function memorySpec(destination, immediate, address = DATA + 8) {
  return {...immediate, destination, address, memory: true, bytes: memoryInstruction(destination, [5, ...words([address])], immediate)};
}
function registerSpec(destination, source, immediate) {return {...immediate, destination, source, memory: false, bytes: registerInstruction(destination, source, immediate)};}
function multiply(c, module, row, value, label) {
  const before = cpu(c), expected = multiplied(value, row.immediate, before.flags);
  const next = {...before, registers: [...before.registers], pc: before.pc + row.bytes.length, flags: expected.flags};
  next.registers[row.destination] = lowWord(before.registers[row.destination], expected.value);
  run(c, module, 1, next, {label, helperVersion: row.memory ? 2 : null, helperValue: value, exitVersion: module.memoryOps ? 2 : 1});
  return expected;
}
const packedPc = (start, rows, index) => start + rows.slice(0, index).reduce((sum, row) => sum + row.bytes.length, 0);

for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries);
  for (const form of forms) {
    const rows = Array.from({length: 8}, (_, destination) => cases[form].map(immediate => registerSpec(destination, (destination + 1) % 8, immediate)));
    const module = compile(c, rows.map((group, i) => block(PC + i * 32, Buffer.concat(group.map(row => row.bytes)), false)));
    for (const [i, group] of rows.entries()) for (const [j, row] of group.entries()) for (const flags of [2, 0xcd7]) {
      const value = cases[form][j].source, r = registers(); r[row.destination] = lowWord(r[row.destination], 0x6bad); r[row.source] = lowWord(r[row.source], value);
      seed(c, packedPc(PC + i * 32, group, j), r, flags);
      multiply(c, module, row, value, 'all eight parents: source times authored immediate ignores poisoned old destination'); counts.matrix++;
    }
    const aliasCases = form === 'imm16'
      ? [{...literal(form, 0xffff), source: 0}, {...literal(form, 0xffff), source: 0xffff}, {...literal(form, 1), source: 0x8000}]
      : [{...literal(form, 0), source: 0}, {...literal(form, 0xff, 0xffff), source: 0xffff}, {...literal(form, 1), source: 0x8000}];
    const aliases = Array.from({length: 8}, (_, destination) => aliasCases.map(immediate => registerSpec(destination, destination, immediate)));
    const aliasModule = compile(c, aliases.map((group, i) => block(PC + 0x100 + i * 32, Buffer.concat(group.map(row => row.bytes)), false)));
    for (const [i, group] of aliases.entries()) for (const [j, row] of group.entries()) {
      const value = aliasCases[j].source, r = registers(); r[i] = lowWord(r[i], value); seed(c, packedPc(PC + 0x100 + i * 32, group, j), r);
      multiply(c, aliasModule, row, value, 'same-register source is captured before low16 publication'); counts.matrix++;
    }
    const group = anchors.filter(row => row.form === form), literalRows = group.map(row => registerSpec(0, 2, row));
    const literalModule = compile(c, literalRows.map((row, i) => block(PC + 0x200 + i * 16, row.bytes, false)));
    for (const [i, row] of literalRows.entries()) {
      const pair = group[i], r = registers(); r[0] = lowWord(r[0], 0x2468); r[2] = lowWord(r[2], pair.source); seed(c, PC + 0x200 + i * 16, r);
      assert.deepEqual(multiply(c, literalModule, row, pair.source, 'literal69 positive128/255 versus6B negative128/minus1 and signed fit edges'), {value: pair.value, flags: pair.flags}); counts.anchors++;
    }
  }
  assert.equal(c.api.close(), 0);
}

for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries);
  for (const form of forms) {
    const rows = Array.from({length: 8}, (_, destination) => cases[form].map(immediate => memorySpec(destination, immediate)));
    const module = compile(c, rows.map((group, i) => block(PC + i * 48, Buffer.concat(group.map(row => row.bytes)))));
    for (const [i, group] of rows.entries()) for (const [j, row] of group.entries()) for (const flags of [2, 0xcd7]) {
      const value = cases[form][j].source, bytes = Buffer.concat([Buffer.from([0x55]), word(value), Buffer.from([0xaa])]);
      upload(c, row.address - 1, bytes); host(c, 'protect', [DATA, 1, 1]);
      const r = registers(); r[row.destination] = lowWord(r[row.destination], 0x6bad); seed(c, packedPc(PC + i * 48, group, j), r, flags);
      multiply(c, module, row, value, 'read-only RAM factor times immediate ignores old destination and retains all high halves');
      checkRam(c, row.address - 1, bytes); host(c, 'protect', [DATA, 1, 3]); counts.matrix++;
    }
  }
  assert.equal(c.api.close(), 0);
}

for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries), payload = destination => destination % 2 ? literal('imm8', 0xf3, 0xfff3) : literal('imm16', 0xf366);
  const rows = Array.from({length: 8}, (_, destination) => {
    const r = registers(); r[destination] = 0xa53c5008;
    const tail = destination === 4 ? [4, 0x24] : destination === 5 ? [0x45, 0] : [destination], immediate = payload(destination);
    return {...immediate, destination, memory: true, address: 0xa53c5008, r, bytes: memoryInstruction(destination, tail, immediate)};
  });
  const scaled = [
    {destination: 1, tail: [0x44, 0x8d, 0xfc], values: {1: 0x40000002, 5: 0xa53c5004}},
    {destination: 0, tail: [4, 0x85, ...words([0xa53c5000])], values: {0: 0x40000002}},
    {destination: 0, tail: [0x84, 0x80, ...words([0x053c5003])], values: {0: 0x20000001}},
    {destination: 4, tail: [0x44, 0x24, 0xff], values: {4: 0xa53c5009}},
  ];
  for (const row of scaled) {
    const r = registers(); for (const [index, value] of Object.entries(row.values)) r[index] = value;
    const immediate = payload(row.destination); rows.push({...row, ...immediate, address: 0xa53c5008, memory: true, r, bytes: memoryInstruction(row.destination, row.tail, immediate)});
  }
  for (let start = 0; start < rows.length; start += 8) {
    const group = rows.slice(start, start + 8), module = compile(c, group.map((row, i) => block(PC + i * 16, row.bytes)));
    for (const [i, row] of group.entries()) for (const value of [0, 0xffff]) {
      const bytes = Buffer.concat([Buffer.from([0x55]), word(value), Buffer.from([0xaa])]); upload(c, row.address - 1, bytes); seed(c, PC + i * 16, row.r);
      multiply(c, module, row, value, 'old full32 destination base/index/both/scaled/wrapped EA with prefix-looking immediate payload');
      checkRam(c, row.address - 1, bytes); counts.addresses++;
    }
  }
  const boundaries = forms.flatMap(form => [memorySpec(0, literal(form, 0), DATA + 4095), memorySpec(2, literal(form, 1), 0xfffffffe)]);
  const boundaryModule = compile(c, boundaries.map((row, i) => block(PC + 0x200 + i * 16, row.bytes)));
  for (const [i, row] of boundaries.entries()) for (const value of [0, 0x8001]) {
    upload(c, row.address, word(value)); seed(c, PC + 0x200 + i * 16);
    multiply(c, boundaryModule, row, value, 'crosspage/last valid WORD factor including immediate zero'); checkRam(c, row.address, word(value)); counts.boundaries++;
  }
  const codeRows = forms.map((form, i) => {
    const pc = PC + 0x300 + i * 32, immediate = form === 'imm16' ? literal(form, 0xff80) : literal(form, 0x80, 0xff80);
    return {...memorySpec(i, immediate, pc + 7), pc};
  });
  const codeModule = compile(c, codeRows.map(row => block(row.pc, row.bytes)), 5);
  for (const row of codeRows) {
    const value = row.form === 'imm16' ? 0xff80 : 0xeb80, wanted = {value: 0x4000, flags: row.form === 'imm16' ? 0x402 : 0xc03};
    seed(c, row.pc); assert.deepEqual(multiply(c, codeModule, row, value, 'own immediate payload is readable code factor; original normalized immediate stays constant'), wanted);
    input(c, 48, words([row.pc])); assert.deepEqual(multiply(c, codeModule, row, value, 'same current owner repeats own payload read independent of returned old destination'), wanted);
    run(c, codeModule, 2, {...cpu(c), pc: row.pc + row.bytes.length + 2}, {label: 'same owner JMP suffix has budget remaining before cold lookup', reason: 3});
    checkRam(c, row.pc, Buffer.concat([row.bytes, Buffer.from([0xeb, 0])])); counts.code_reads++;
  }
  assert.equal(c.api.close(), 0);
}

for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries), mov = Buffer.from([0xbb, 0x44, 0x33, 0x22, 0x11]);
  const rows = [
    ...forms.map(form => ({...literal(form, 0)})),
    literal('imm16', 0xffff), literal('imm8', 0xff, 0xffff),
    literal('imm16', 0xffff), literal('imm8', 0xff, 0xffff),
  ].map(immediate => ({...memorySpec(0, immediate), bytes: memoryInstruction(0, [7], immediate)}));
  const module = compile(c, rows.map((row, i) => block(PC + i * 32, Buffer.concat([mov, row.bytes]))));
  const control = registers(); control[7] = 0x7008;
  seed(c, PC + 5, control); run(c, module, 0, cpu(c), {label: 'zero budget avoids unmapped zero-immediate read', retired: 0});
  input(c, 96, words([1])); run(c, module, 1, cpu(c), {label: 'cancel avoids unmapped zero-immediate read', reason: 2, retired: 0}); input(c, 96, words([0])); counts.controls += 2;
  for (const [index, row] of rows.slice(0, 2).entries()) for (const kind of ['unmapped', 'none', 'write-only', 'execute-only', 'cross-unmapped', 'cross-write-only', 'overflow']) {
    const crossing = kind.startsWith('cross'), overflow = kind === 'overflow', missing = kind.includes('unmapped');
    const address = overflow ? 0xffffffff : crossing ? 0x7fff : 0x7008, mapped = overflow ? 0 : crossing && !missing ? 2 : missing && !crossing ? 0 : 1;
    const unchanged = Buffer.from(crossing && missing ? [0x55, 1] : [0x55, 1, 0x80, 0xaa]);
    if (mapped) {host(c, 'map', [0x7000, mapped, 3]); upload(c, crossing ? 0x7ffe : 0x7007, unchanged);}
    if (!missing && !overflow) host(c, 'protect', [crossing ? 0x8000 : 0x7000, 1, kind === 'none' ? 0 : kind === 'execute-only' ? 4 : 2]);
    if (overflow) upload(c, 0xfffffffe, Buffer.from([0x55, 1]));
    const r = registers(); r[0] = lowWord(r[0], 0x6bad); r[7] = address;
    const before = seed(c, PC + index * 32, r), stopped = {...before, registers: [...before.registers], pc: before.pc + 5}; stopped.registers[3] = 0x11223344;
    const fault = overflow ? undefined : {detail: missing ? 1 : 2, address: crossing ? 0x8000 : address, access: 1};
    run(c, module, 2, stopped, {label: row.form + ' immediate0 still reads and faults without publishing destination/FLAGS', reason: overflow ? 11 : 5, retired: 1, fault, helperVersion: overflow ? null : 2, exitVersion: overflow ? 6 : 2});
    run(c, module, 1, stopped, {label: kind + ' unchanged zero-immediate retry', reason: overflow ? 11 : 5, retired: 0, fault, helperVersion: overflow ? null : 2, exitVersion: overflow ? 6 : 2}); counts.faults += 2;
    if (crossing && missing) checkRam(c, 0x7ffe, unchanged);
    if (overflow) checkRam(c, 0xfffffffe, Buffer.from([0x55, 1]));
    if (!overflow) {
      if (missing) host(c, 'map', [crossing ? 0x8000 : 0x7000, 1, 1]);
      else host(c, 'protect', [crossing ? 0x8000 : 0x7000, 1, 1]);
      assert.deepEqual(cpu(c), stopped, 'map/protect-only repair preserves stopped CPU');
      const value = missing ? crossing ? 1 : 0 : 0x8001;
      assert.deepEqual(multiply(c, module, {...row, address}, value, 'zero-immediate retry still loads real RAM then commits zero with cleared undefined bits'), {value: 0, flags: 0x402});
      checkRam(c, address, word(value)); counts.retries++; host(c, 'unmap', [0x7000, crossing ? 2 : 1]);
    }
  }
  for (const [offset, row] of rows.slice(2, 4).entries()) {
    const address = DATA + 8, r = registers(); r[7] = address;
    upload(c, address - 1, Buffer.from([0x55, 0, 0x80, 0xaa])); host(c, 'protect', [DATA, 1, 2]);
    const before = seed(c, PC + (2 + offset) * 32, r), stopped = {...before, registers: [...before.registers], pc: before.pc + 5}; stopped.registers[3] = 0x11223344;
    const fault = {detail: 2, address, access: 1};
    run(c, module, 2, stopped, {label: 'nonzero negative immediate read failure retains dirty FLAGS', reason: 5, retired: 1, fault, helperVersion: 2});
    run(c, module, 1, stopped, {label: 'unchanged nonzero-immediate failure', reason: 5, retired: 0, fault, helperVersion: 2}); counts.faults += 2;
    host(c, 'protect', [DATA, 1, 1]); assert.deepEqual(cpu(c), stopped);
    assert.deepEqual(multiply(c, module, {...row, address}, 0x8000, 'permission-only negative-immediate retry consumes current min signed factor'), {value: 0x8000, flags: 0xc03});
    checkRam(c, address - 1, Buffer.from([0x55, 0, 0x80, 0xaa])); host(c, 'protect', [DATA, 1, 3]); counts.retries++;
  }
  for (const [offset, row] of rows.slice(4).entries()) {
    const address = DATA + 12, nextAddress = DATA + 14, r = registers(); r[0] = lowWord(r[0], 0x6bad); r[7] = address;
    upload(c, address - 1, Buffer.from([0x55, 2, 0, 0, 0, 0xaa])); host(c, 'protect', [DATA, 1, 2]);
    const before = seed(c, PC + (4 + offset) * 32, r), stopped = {...before, registers: [...before.registers], pc: before.pc + 5}; stopped.registers[3] = 0x11223344;
    const fault = {detail: 2, address, access: 1};
    run(c, module, 2, stopped, {label: 'current-input witness initially faults on factor2', reason: 5, retired: 1, fault, helperVersion: 2});
    run(c, module, 1, stopped, {label: 'current-input witness unchanged retry', reason: 5, retired: 0, fault, helperVersion: 2}); counts.faults += 2;
    host(c, 'protect', [DATA, 1, 3]); upload(c, nextAddress, word(0x8000)); host(c, 'protect', [DATA, 1, 1]);
    const current = {...stopped, registers: [...stopped.registers]}; current.registers[0] = 0xbaffdead; current.registers[7] = nextAddress;
    input(c, 16, words([current.registers[0]])); input(c, 44, words([current.registers[7]])); assert.deepEqual(cpu(c), current, 'declared new RAM/EA/current destination edits preserve the remaining stopped CPU');
    assert.deepEqual(multiply(c, module, {...row, address: nextAddress}, 0x8000, 'retry uses current source/new EA/current high16 and ignores current destination low16'), {value: 0x8000, flags: 0xc03});
    assert.equal(cpu(c).registers[0], 0xbaff8000); checkRam(c, address - 1, Buffer.from([0x55, 2, 0, 0, 0x80, 0xaa]));
    host(c, 'protect', [DATA, 1, 3]); counts.retries++; counts.current_inputs++;
  }

  const overflow = memorySpec(0, literal('imm16', 0xffff), DATA + 32), fit = registerSpec(0, 0, literal('imm8', 1)), zero = memorySpec(0, literal('imm8', 0), DATA + 32);
  const setOverflowCarry = Buffer.from([0x0f, 0x90, 0xc3, 0x0f, 0x92, 0xc7]), setZeroParity = Buffer.from([0x0f, 0x94, 0xc3, 0x0f, 0x9a, 0xc7]);
  const live = Buffer.concat([overflow.bytes, setOverflowCarry, fit.bytes, setOverflowCarry, zero.bytes, setZeroParity]);
  const liveModule = compile(c, [block(PC + 0x200, live)]);
  upload(c, DATA + 31, Buffer.from([0x55, 0, 0x80, 0xaa])); host(c, 'protect', [DATA, 1, 1]); seed(c, PC + 0x200);
  assert.deepEqual(multiply(c, liveModule, overflow, 0x8000, 'live RAM min times imm16 minus1 overflows independently of old AX'), {value: 0x8000, flags: 0xc03});
  let old = cpu(c), next = {...old, registers: [...old.registers], pc: old.pc + 6}; next.registers[3] = lowWord(old.registers[3], 0x0101);
  run(c, liveModule, 2, next, {label: 'SETO BL/SETB BH consume overflow without touching returned AX', retired: 2});
  assert.deepEqual(multiply(c, liveModule, fit, 0x8000, 'live source=destination returned AX times imm8 plus1 fits with unchanged low16'), {value: 0x8000, flags: 0x402});
  old = cpu(c); next = {...old, registers: [...old.registers], pc: old.pc + 6}; next.registers[3] = lowWord(old.registers[3], 0);
  run(c, liveModule, 2, next, {label: 'SETO/SETB observe fit after current returned-AX source', retired: 2});
  assert.deepEqual(multiply(c, liveModule, zero, 0x8000, 'live zero immediate still reads RAM and commits zero with undefined bits clear'), {value: 0, flags: 0x402});
  old = cpu(c); next = {...old, registers: [...old.registers], pc: old.pc + 6}; next.registers[3] = lowWord(old.registers[3], 0);
  run(c, liveModule, 2, next, {label: 'SETZ BL/SETP BH observe Ring3 clear policy after zero product', retired: 2});
  checkRam(c, DATA + 31, Buffer.from([0x55, 0, 0x80, 0xaa])); counts.consumers++; assert.equal(c.api.close(), 0);
}
assert.deepEqual(sources(), sourcePins, 'risk-relevant sources stayed fixed');
assert.equal(counts.contexts, 16); assert.equal(counts.runs, 1672); assert.equal(counts.retired, 1604);
assert.equal(counts.matrix, 1216); assert.equal(counts.anchors, 64); assert.equal(counts.addresses, 96);
assert.equal(counts.boundaries, 32); assert.equal(counts.faults, 144); assert.equal(counts.retries, 64);
assert.equal(counts.current_inputs, 8); assert.equal(counts.consumers, 4); assert.equal(counts.controls, 8);
assert.equal(counts.code_reads, 8); assert.equal(modules.length, 56);
const result = {status: 'ok', engine_sha256: engine.sha256, source_pins: sourcePins, counts, modules,
  limits: ['finite exact single66 WORD69 imm16/6B signedimm8 IMUL register/memory in four actual bound profiles, with native standalone register coverage separate',
    'independent widened signed source.low16 times authored normalized immediate; old destination low16 is ignored while current high16 is retained',
    'CF/OF report signed16 fit; clearing PF/AF/ZF/SF follows Ring3 policy for undefined arithmetic flags, without hardware undefined-bit equality claim',
    'memory modules import only guard/read16, including immediate zero; read-only data and own immediate/code payloads never store or cause SMC',
    'real read16 failures and selected RAM/canary checks; map/protect-only repair and declared current-input retry are separate witnesses',
    'full4612 arena preserves FP128; live returned-AX and overflow/fit/zero flag consumers avoid hidden CPU reseeding',
    'no implicit multiply, exhaustive ISA, throughput, PE/browser/SDK/game or CI claim']};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2) + '\n', {flag: 'wx'});
console.log(JSON.stringify({status: result.status, ...counts, modules: modules.length, output}));
