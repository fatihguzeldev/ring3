import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';

const [enginePath, outputDir, root] = process.argv.slice(2);
assert.ok(root, 'expected actual engine, generated artifacts and repository root');
const fixtureRoot = join(root, 'engine/tests/fixtures/p2-lea');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const generated = new Map();
let standaloneRuns = 0, embeddedRuns = 0, resumes = 0, keys = 0n;

function header(bytes, view, pointer, magic, length, version = 1) {
  bytes.set(Buffer.from(magic, 'ascii'), pointer);
  view.setUint16(pointer + 4, version, true);
  view.setUint16(pointer + 6, 1, true);
  view.setUint32(pointer + 8, length, true);
  view.setUint32(pointer + 12, 0, true);
}

function state(eip = 0x1000, eflags = 0xcd7) {
  return {registers: [0x89abcdef, 0x13579bdf, 0x23456789, 0x3456789a, 0x456789ab, 0x56789abc, 0x6789abcd, 0x789abcde], eip, eflags};
}
const copyState = value => ({...value, registers: [...value.registers]});

function refresh(ctx) {
  if (ctx.buffer !== ctx.memory.buffer) {
    ctx.buffer = ctx.memory.buffer;
    ctx.bytes = new Uint8Array(ctx.buffer);
    ctx.view = new DataView(ctx.buffer);
  }
  return ctx;
}

function initialize(ctx, value, cancel = 0) {
  refresh(ctx);
  header(ctx.bytes, ctx.view, ctx.base, 'R3ST', 56);
  value.registers.forEach((register, index) => ctx.view.setUint32(ctx.base + 16 + index * 4, register, true));
  ctx.view.setUint32(ctx.base + 48, value.eip, true);
  ctx.view.setUint32(ctx.base + 52, value.eflags, true);
  ctx.view.setUint32(ctx.base + 96, cancel, true);
  ctx.bytes.fill(0xa5, ctx.base + 56, ctx.base + 96);
}

function readState(ctx) {
  refresh(ctx);
  assert.deepEqual(ctx.bytes.slice(ctx.base, ctx.base + 16), Uint8Array.from([0x52, 0x33, 0x53, 0x54, 1, 0, 1, 0, 56, 0, 0, 0, 0, 0, 0, 0]));
  return {
    registers: Array.from({length: 8}, (_, index) => ctx.view.getUint32(ctx.base + 16 + index * 4, true)),
    eip: ctx.view.getUint32(ctx.base + 48, true),
    eflags: ctx.view.getUint32(ctx.base + 52, true),
  };
}

function exit(reason, retired, version = 1, detail = 0, address = 0, access = 0, length = 0) {
  const bytes = new Uint8Array(40), view = new DataView(bytes.buffer);
  header(bytes, view, 0, 'R3EX', 40, version);
  [reason, retired, detail, address, access, length].forEach((value, index) => view.setUint32(16 + index * 4, value, true));
  return bytes;
}

function helper(tag, value = 0, detail = 0, address = 0, access = 0, length = 0) {
  const bytes = new Uint8Array(40), view = new DataView(bytes.buffer);
  header(bytes, view, 0, 'R3MH', 40);
  [tag, value, detail, address, access, length].forEach((field, index) => view.setUint32(16 + index * 4, field, true));
  return bytes;
}

function call(ctx, budget, expectedState, expectedExit, label, expectedHelper = undefined) {
  refresh(ctx);
  const before = ctx.bytes.slice(ctx.base + 96, ctx.base + 4236);
  assert.equal(ctx.run(ctx.base, ctx.base + 56, budget, ctx.base + 96), 0, `${label}: runtime status`);
  if (ctx.embedded) embeddedRuns++; else standaloneRuns++;
  assert.deepEqual(readState(ctx), expectedState, `${label}: all registers/EIP/flags`);
  assert.deepEqual(ctx.bytes.slice(ctx.base + 56, ctx.base + 96), expectedExit, `${label}: complete canonical exit`);
  assert.deepEqual(ctx.bytes.slice(ctx.base + 96, ctx.base + 100), before.slice(0, 4), `${label}: cancellation unchanged`);
  assert.deepEqual(ctx.bytes.slice(ctx.base + 140, ctx.base + 4236), before.slice(44), `${label}: transfer unchanged`);
  assert.deepEqual(ctx.bytes.slice(ctx.base + 100, ctx.base + 140), expectedHelper ?? before.slice(4, 44), `${label}: helper record`);
}

function standalone(name, value, cancel = 0) {
  const encoded = readFileSync(join(outputDir, `${name}.wasm`));
  assert.ok(WebAssembly.validate(encoded), `${name}: Wasm validation`);
  const module = new WebAssembly.Module(encoded);
  assert.deepEqual(WebAssembly.Module.imports(module), [{module: 'env', name: 'memory', kind: 'memory'}], `${name}: no helpers or host dispatch`);
  generated.set(name, hash(encoded));
  const memory = new WebAssembly.Memory({initial: 1});
  const instance = new WebAssembly.Instance(module, {env: {memory}});
  const ctx = refresh({memory, base: 128, run: instance.exports.run, embedded: false});
  ctx.bytes.fill(0x5a);
  initialize(ctx, value, cancel);
  return ctx;
}

// BigInt modulo arithmetic is independent of Wasm locals and i32 operators.
function address(before, base, index, scale, displacement) {
  const sum = BigInt(displacement) + (base === null ? 0n : BigInt(before.registers[base])) +
    (index === null ? 0n : BigInt(before.registers[index]) * BigInt(scale));
  return Number(BigInt.asUintN(32, sum));
}

const forms = [
  ['lea_eax_base', '8d03', 0, 3, null, 1, 0],
  ['lea_ecx_index2', '8d0c5578563412', 1, null, 2, 2, 0x12345678],
  ['lea_edx_scale4', '8d54b380', 2, 3, 6, 4, -128],
  ['lea_ebx_scale8', '8d9cf978563412', 3, 1, 7, 8, 0x12345678],
  ['lea_esp_baseesp', '8d6404ff', 4, 4, 0, 1, -1],
  ['lea_ebp_index_alias', '8d6c6a7f', 5, 2, 5, 2, 127],
  ['lea_esi_base_alias', '8db48e00000080', 6, 6, 1, 4, -2147483648],
  ['lea_edi_both_alias', '8d7cff07', 7, 7, 7, 8, 7],
  ['lea_no_base_index1', '8d043521436587', 0, null, 6, 1, 0x87654321],
  ['lea_absolute', '8d05ffffffff', 0, null, null, 1, 0xffffffff],
  ['lea_index8_only', '8d04ddffffffff', 0, null, 3, 8, -1],
  ['lea_ebp_no_index', '8d4500', 0, 5, null, 1, 0],
  ['lea_esp_no_index', '8d0424', 0, 4, null, 1, 0],
  ['lea_disp32_positive', '8d83ffffff7f', 0, 3, null, 1, 0x7fffffff],
  ['lea_disp32_negative', '8d8300000080', 0, 3, null, 1, -2147483648],
];
function elfFixtures(object) {
  const view = new DataView(object.buffer, object.byteOffset, object.byteLength);
  assert.deepEqual([...object.subarray(0, 7)], [0x7f, 0x45, 0x4c, 0x46, 1, 1, 1]);
  assert.equal(view.getUint16(18, true), 3, 'LLVM i386 object');
  const sectionOffset = view.getUint32(32, true), sectionSize = view.getUint16(46, true), count = view.getUint16(48, true);
  const sections = Array.from({length: count}, (_, index) => {
    const at = sectionOffset + index * sectionSize;
    return {type: view.getUint32(at + 4, true), offset: view.getUint32(at + 16, true), size: view.getUint32(at + 20, true), link: view.getUint32(at + 24, true), info: view.getUint32(at + 28, true), stride: view.getUint32(at + 36, true)};
  });
  const table = sections.find(section => section.type === 2);
  assert.ok(table);
  const strings = sections[table.link];
  const symbols = new Map();
  for (let at = table.offset; at < table.offset + table.size; at += table.stride) {
    const startName = strings.offset + view.getUint32(at, true);
    const name = object.subarray(startName, object.indexOf(0, startName)).toString('utf8');
    const section = view.getUint16(at + 14, true), start = view.getUint32(at + 4, true), size = view.getUint32(at + 8, true);
    if (section > 0 && section < sections.length && size > 0) {
      const source = sections[section];
      assert.ok(start + size <= source.size);
      assert.ok(!sections.some(item => [4, 9].includes(item.type) && item.info === section && item.size > 0), 'authored code has no relocations');
      symbols.set(name, {offset: start, bytes: object.subarray(source.offset + start, source.offset + start + size)});
    }
  }
  return symbols;
}

const assemblyPath = join(fixtureRoot, 'integer.S'), objectPath = join(outputDir, 'integer.o');
const assemble = ['-target', 'i386-unknown-linux-gnu', '-c', assemblyPath, '-o', objectPath];
execFileSync('clang', assemble, {stdio: ['ignore', 'pipe', 'pipe']});
const disassembly = execFileSync('xcrun', ['llvm-objdump', '-d', '--x86-asm-syntax=intel', objectPath], {encoding: 'utf8'});
writeFileSync(join(outputDir, 'integer.disassembly.txt'), disassembly);
const object = readFileSync(objectPath), assembled = elfFixtures(object), llvm = [];
function verify(name, offset, hex) {
  const fixture = assembled.get(name);
  assert.ok(fixture, `${name}: authored LLVM symbol`);
  assert.equal(fixture.offset, offset, `${name}: independent offset`);
  assert.equal(fixture.bytes.toString('hex'), hex, `${name}: independent literal encoding`);
  llvm.push({name, offset, hex});
}
let offset = 0;
for (const [name, hex] of forms) {
  verify(name, offset, hex);
  offset += hex.length / 2;
}
verify('checkpoint', offset, '8d448bff8d24048d0440');
verify('embedded_checkpoint', 0x200, '8d448bff8d24048d0440e9f1010000');
verify('lea_fault', 0x300, '8d44cbff8b10e9f5000000');
assert.equal(assembled.size, llvm.length, 'all authored nonempty symbols verified');

for (const [name, hex, destination, base, index, scale, displacement] of forms) {
  for (const flags of [2, 0xcd7]) {
    const before = state(0x1000, flags);
    if (flags === 0xcd7) before.registers = [0xffffffff, 0x40000000, 0x80000000, 0xffffff80, 1, 0x80000001, 0xfffffff0, 0xffffffff];
    const expected = copyState(before);
    expected.registers[destination] = address(before, base, index, scale, displacement);
    expected.eip += hex.length / 2;
    call(standalone(name, before), 1, expected, exit(1, 1), `LEA form ${name}/flags${flags}`);
  }
}

const literalEdges = [
  ['lea_eax_base', [[3, 0]], 0],
  ['lea_eax_base', [[3, 0xffffffff]], 0xffffffff],
  ['lea_ecx_index2', [[2, 0x80000000]], 0x12345678],
  ['lea_edx_scale4', [[3, 127], [6, 0]], 0xffffffff],
  ['lea_ebx_scale8', [[1, 0xfffffff8], [7, 1]], 0x12345678],
  ['lea_esp_baseesp', [[4, 0], [0, 0]], 0xffffffff],
  ['lea_edi_both_alias', [[7, 0xffffffff]], 0xfffffffe],
  ['lea_no_base_index1', [[6, 0x789abcdf]], 0],
  ['lea_absolute', [], 0xffffffff],
  ['lea_index8_only', [[3, 0x20000000]], 0xffffffff],
  ['lea_disp32_positive', [[3, 0x80000000]], 0xffffffff],
  ['lea_disp32_negative', [[3, 0x80000000]], 0],
];
for (const [name, replacements, literal] of literalEdges) {
  const before = state();
  for (const [register, value] of replacements) before.registers[register] = value;
  const [, hex, destination, base, index, scale, displacement] = forms.find(form => form[0] === name);
  assert.equal(address(before, base, index, scale, displacement), literal, `${name}: BigInt/literal agreement`);
  const expected = copyState(before);
  expected.registers[destination] = literal;
  expected.eip += hex.length / 2;
  call(standalone(name, before), 1, expected, exit(1, 1), `literal EA ${name}/${literal}`);
}

function checkpointStates(pc, flags) {
  const before = state(pc, flags);
  before.registers[1] = 3;
  before.registers[3] = 0xfffffff8;
  before.registers[4] = 5;
  const first = copyState(before);
  first.registers[0] = 3;
  first.eip += 4;
  const second = copyState(first);
  second.registers[4] = 8;
  second.eip += 3;
  const third = copyState(second);
  third.registers[0] = 9;
  third.eip += 3;
  return {before, first, second, third};
}
const checkpoints = checkpointStates(0x1000, 0xcd7);
const checkpoint = standalone('checkpoint', checkpoints.before);
call(checkpoint, 1, checkpoints.first, exit(1, 1), 'standalone budget1 commits LEA without flags changes');
checkpoint.view.setUint32(checkpoint.base + 96, 1, true);
call(checkpoint, 0, checkpoints.first, exit(2, 0), 'standalone cancellation precedes zero budget at interior PC');
resumes++;
checkpoint.view.setUint32(checkpoint.base + 96, 0, true);
call(checkpoint, 1, checkpoints.second, exit(1, 1), 'standalone interior LEA captures old ESP and new EAX');
resumes++;
call(checkpoint, 1, checkpoints.third, exit(1, 1), 'standalone base/index destination alias consumes old EAX twice');
resumes++;
call(checkpoint, 1, checkpoints.third, exit(3, 0), 'standalone end continuation does not retire again');
resumes++;
for (const [pc, budget, cancel, reason] of [[0x1000, 0, 0, 1], [0x1001, 10, 0, 3], [0x12345678, 10, 1, 2]]) {
  const before = state(pc, 2);
  call(standalone('checkpoint', before, cancel), budget, before, exit(reason, 0), `standalone precise safepoint ${pc}/${budget}/${cancel}`);
}
const engineBytes = readFileSync(enginePath), engineModule = new WebAssembly.Module(engineBytes);
assert.ok(WebAssembly.validate(engineBytes));
assert.deepEqual(WebAssembly.Module.imports(engineModule), [], 'actual engine has no JS imports');
function engine() {
  const instance = new WebAssembly.Instance(engineModule, {});
  const names = ['open', 'arena_ptr', 'map', 'upload', 'compile_entries', 'generation', 'module_ptr', 'module_len', 'guard', 'read32', 'store32'];
  const api = Object.fromEntries(names.map(name => {
    const fn = instance.exports[`ring3_abi_v1_${name}`];
    assert.equal(typeof fn, 'function', `actual export ${name}`);
    return [name, fn];
  }));
  const key = 0x3050000000000000n + ++keys;
  assert.equal(api.open(4, Number(key & 0xffffffffn), Number(key >> 32n)), 0);
  const ctx = refresh({api, memory: instance.exports.memory, base: api.arena_ptr() >>> 0, embedded: true});
  assert.equal(api.map(0x1000, 1, 7), 0);
  return ctx;
}

function upload(ctx, address, bytes) {
  refresh(ctx).bytes.set(bytes, ctx.base + 140);
  assert.equal(ctx.api.upload(address, bytes.length), 0);
  refresh(ctx);
}

function compile(ctx, name, entries, gates, helpers = []) {
  refresh(ctx);
  entries.forEach((entry, index) => ctx.view.setUint32(ctx.base + 140 + index * 4, entry, true));
  gates.forEach(([entry, id], index) => {
    ctx.view.setUint32(ctx.base + 140 + entries.length * 4 + index * 8, entry, true);
    ctx.view.setUint32(ctx.base + 144 + entries.length * 4 + index * 8, id, true);
  });
  assert.equal(ctx.api.compile_entries(entries.length, gates.length), 0, `${name}: actual cold discovery`);
  refresh(ctx);
  const pointer = ctx.api.module_ptr() >>> 0, length = ctx.api.module_len() >>> 0;
  const encoded = ctx.bytes.slice(pointer, pointer + length), module = new WebAssembly.Module(encoded);
  assert.ok(WebAssembly.validate(encoded));
  const imports = [{module: 'env', name: 'memory', kind: 'memory'}, {module: 'ring3', name: 'guard', kind: 'function'}, ...helpers.map(name => ({module: 'ring3', name, kind: 'function'}))];
  const sort = list => list.sort((a, b) => `${a.module}.${a.name}`.localeCompare(`${b.module}.${b.name}`));
  assert.deepEqual(sort(WebAssembly.Module.imports(module)), sort(imports), `${name}: bounded actual Wasm imports`);
  const instance = new WebAssembly.Instance(module, {env: {memory: ctx.memory}, ring3: {guard: ctx.api.guard, read32: ctx.api.read32, store32: ctx.api.store32}});
  ctx.run = instance.exports.run;
  generated.set(name, hash(encoded));
  writeFileSync(join(outputDir, `${name}.wasm`), encoded);
}

const embedded = engine();
upload(embedded, 0x1200, assembled.get('embedded_checkpoint').bytes);
compile(embedded, 'embedded_checkpoint', [0x1200], []);
const full = checkpointStates(0x1200, 2);
initialize(embedded, full.before);
const fullExpected = copyState(full.third);
fullExpected.eip = 0x1400;
call(embedded, 10, fullExpected, exit(3, 4), 'actual guard-only LEA sequence with flags2 and resident JMP');

// A fresh independent initial state begins this sequence; subsequent calls only
// modify cancellation. Every CPU transition is produced by actual Wasm.
const partial = checkpointStates(0x1200, 0xcd7);
initialize(embedded, partial.before);
call(embedded, 1, partial.first, exit(1, 1), 'actual budget1 LEA before interior resume');
embedded.view.setUint32(embedded.base + 96, 1, true);
call(embedded, 0, partial.first, exit(2, 0), 'actual cancel precedes zero budget');
resumes++;
embedded.view.setUint32(embedded.base + 96, 0, true);
call(embedded, 1, partial.second, exit(1, 1), 'actual ESP destination captures old ESP');
resumes++;
call(embedded, 1, partial.third, exit(1, 1), 'actual same destination/base/index uses old value');
resumes++;
const afterJump = copyState(partial.third);
afterJump.eip = 0x1400;
call(embedded, 1, afterJump, exit(1, 1), 'actual JMP consumes final budget once');
resumes++;
call(embedded, 1, afterJump, exit(3, 0), 'actual unknown target resumes NeedCode without CPU writes');
resumes++;

const fault = engine();
upload(fault, 0x1300, assembled.get('lea_fault').bytes);
compile(fault, 'embedded_fault', [0x1300], [], ['read32']);
const faultStart = state(0x1300);
faultStart.registers[1] = 1;
faultStart.registers[3] = 0xfffffff8;
initialize(fault, faultStart);
const faultExpected = copyState(faultStart);
faultExpected.registers[0] = 0xffffffff;
faultExpected.eip = 0x1304;
call(fault, 10, faultExpected, exit(5, 1, 2, 3, 0xffffffff, 1, 4), 'LEA accepts full address then MOV fails precisely', helper(1, 0, 3, 0xffffffff, 1, 4));
call(fault, 10, faultExpected, exit(5, 0, 2, 3, 0xffffffff, 1, 4), 'MOV fault resume preserves committed LEA and every flag', helper(1, 0, 3, 0xffffffff, 1, 4));
resumes++;

const aggregate = [...generated].sort(([a], [b]) => a.localeCompare(b)).map(([name, digest]) => `${name}:${digest}`).join('\n');
const provenance = {
  claim: 'Own baseline Wasm LEA32 modulo-address execution with full flags preservation; no dereference, synthetic helpers, native C, performance or full P2-V0 claim.',
  standalone_modules: generated.size - 2, embedded_modules: 2, standalone_runs: standaloneRuns, embedded_runs: embeddedRuns, continuation_calls: resumes, synthetic_runs: 0,
  module_set_sha256: hash(aggregate), engine_sha256: hash(engineBytes),
  sha256: Object.fromEntries(['integer.S', 'run.mjs'].map(name => [name, hash(readFileSync(join(fixtureRoot, name)))]).concat([['dbt_lea_wasm.rs', hash(readFileSync(join(root, 'engine/tests/dbt_lea_wasm.rs')))], ['integer.o', hash(object)]])),
  versions: {clang: execFileSync('clang', ['--version'], {encoding: 'utf8'}).split('\n')[0], objdump: execFileSync('xcrun', ['llvm-objdump', '--version'], {encoding: 'utf8'}).split('\n')[0], node: process.version},
  commands: {assemble: ['clang', ...assemble], disassemble: ['xcrun', 'llvm-objdump', '-d', '--x86-asm-syntax=intel', objectPath]},
  llvm_symbols: llvm, generated_sha256: Object.fromEntries([...generated].sort(([a], [b]) => a.localeCompare(b))),
};
writeFileSync(join(outputDir, 'provenance.json'), `${JSON.stringify(provenance, null, 2)}\n`);
console.log(JSON.stringify({modules: generated.size, standalone_runs: standaloneRuns, embedded_runs: embeddedRuns, continuation_calls: resumes, llvm_symbols: llvm.length, synthetic_runs: 0, engine_sha256: provenance.engine_sha256, module_set_sha256: provenance.module_set_sha256, artifacts: outputDir}));
