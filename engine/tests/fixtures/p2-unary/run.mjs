import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';

const [enginePath, outputDir, root] = process.argv.slice(2);
assert.ok(root, 'expected actual engine, generated artifacts and repository root');
const fixtureRoot = join(root, 'engine/tests/fixtures/p2-unary');
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

// Independent unsigned arithmetic; pinned Intel unary flag rules supply the
// arithmetic anchors below. NOT returns the entire existing bounded flags word.
function unary(kind, operand, oldFlags) {
  const a = BigInt(operand);
  const result = BigInt.asUintN(32, kind === 'inc' ? a + 1n : kind === 'dec' ? a - 1n : kind === 'neg' ? -a : ~a);
  if (kind === 'not') return {result: Number(result), flags: oldFlags};
  const carry = kind === 'neg' ? a !== 0n : (oldFlags & 1) !== 0;
  const lowNibble = a % 16n;
  const auxiliary = kind === 'inc' ? lowNibble === 15n : kind === 'dec' ? lowNibble === 0n : lowNibble !== 0n;
  const overflow = kind === 'inc' ? a === 0x7fffffffn : a === 0x80000000n;
  const parity = [...Number(result % 256n).toString(2)].filter(bit => bit === '1').length % 2 === 0;
  const flags = 2 | (oldFlags & 0x400) | (carry ? 1 : 0) | (parity ? 4 : 0) |
    (auxiliary ? 0x10 : 0) | (result === 0n ? 0x40 : 0) | (result >= 0x80000000n ? 0x80 : 0) | (overflow ? 0x800 : 0);
  return {result: Number(result), flags};
}
const forms = [
  ['inc_short', '40', 'inc', 0], ['inc_modrm', 'ffc0', 'inc', 0],
  ['dec_short', '48', 'dec', 0], ['dec_modrm', 'ffc8', 'dec', 0],
  ['not_eax', 'f7d0', 'not', 0], ['neg_eax', 'f7d8', 'neg', 0],
  ['inc_ecx', '41', 'inc', 1], ['dec_edx', '4a', 'dec', 2],
  ['not_ebx', 'f7d3', 'not', 3], ['neg_esp', 'f7dc', 'neg', 4],
  ['inc_ebp_modrm', 'ffc5', 'inc', 5], ['dec_esi', '4e', 'dec', 6], ['not_edi', 'f7d7', 'not', 7],
];
const branches = [
  ['inc_jc', '407202', 'inc', true], ['dec_jnc', '487302', 'dec', false],
  ['neg_jc', 'f7d87202', 'neg', true], ['not_jc', 'f7d07202', 'not', true],
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
  assert.equal(fixture.offset, offset, `${name}: literal offset`);
  assert.equal(fixture.bytes.toString('hex'), hex, `${name}: independently authored encoding`);
  llvm.push({name, offset, hex});
}
let offset = 0;
for (const [name, hex] of forms) { verify(name, offset, hex); offset += hex.length / 2; }
verify('checkpoint', offset, '404cf7d0f7dc');
offset += 6;
for (const [name, hex] of branches) { verify(name, offset, hex); offset += hex.length / 2 + 2; }
verify('counter_entry', 0x200, 'b800000000');
verify('counter_guard', 0x205, '85c97404');
verify('counter_body', 0x209, '404975fc');
verify('counter_done', 0x20d, '0f0b');
verify('embedded_checkpoint', 0x300, '404cf7d0f7dce9f5010000');
verify('unary_fault', 0x400, 'f7d88b16e9f7000000');
assert.equal(assembled.size, llvm.length, 'all nonempty authored symbols verified');

for (const [name, hex, kind, destination] of forms) {
  for (const [operand, flags] of [[0x12345678, 2], [0xffff8003, 0xcd7]]) {
    const before = state(0x1000, flags);
    before.registers[destination] = operand;
    const computed = unary(kind, operand, flags), expected = copyState(before);
    expected.registers[destination] = computed.result;
    expected.eflags = computed.flags;
    expected.eip += hex.length / 2;
    call(standalone(name, before), 1, expected, exit(1, 1), `encoding/all-GPR ${name}/${operand}/${flags}`);
  }
}

// Arithmetic anchors use CF0/DF0; INC/DEC add the saved input CF, while NEG's
// anchors include its computed CF. Cross both CF and DF for every literal.
const literals = [
  ['inc', 0, 1, 2], ['inc', 1, 2, 2], ['inc', 15, 16, 0x12], ['inc', 16, 17, 6],
  ['inc', 0x7fffffff, 0x80000000, 0x896], ['inc', 0x80000000, 0x80000001, 0x82], ['inc', 0xffffffff, 0, 0x56],
  ['dec', 0, 0xffffffff, 0x96], ['dec', 1, 0, 0x46], ['dec', 15, 14, 2], ['dec', 16, 15, 0x16],
  ['dec', 0x7fffffff, 0x7ffffffe, 2], ['dec', 0x80000000, 0x7fffffff, 0x816], ['dec', 0xffffffff, 0xfffffffe, 0x82],
  ['neg', 0, 0, 0x46], ['neg', 1, 0xffffffff, 0x97], ['neg', 15, 0xfffffff1, 0x93], ['neg', 16, 0xfffffff0, 0x87],
  ['neg', 0x7fffffff, 0x80000001, 0x93], ['neg', 0x80000000, 0x80000000, 0x887], ['neg', 0xffffffff, 1, 0x13],
];
for (const [kind, operand, result, flags] of literals) {
  for (const cf of [0, 1]) for (const df of [0, 0x400]) {
    const oldFlags = 0x8d6 | cf | df;
    const expectedFlags = flags | df | (kind === 'neg' ? 0 : cf);
    assert.deepEqual(unary(kind, operand, oldFlags), {result, flags: expectedFlags}, `${kind}: literal/BigInt CF${cf}/DF${df}`);
    const before = state(0x1000, oldFlags);
    before.registers[0] = operand;
    const expected = copyState(before);
    expected.registers[0] = result;
    expected.eflags = expectedFlags;
    expected.eip += kind === 'neg' ? 2 : 1;
    call(standalone(kind === 'neg' ? 'neg_eax' : `${kind}_short`, before), 1, expected, exit(1, 1), `literal ${kind}/${operand}/CF${cf}/DF${df}`);
  }
}
for (const operand of [0, 1, 15, 16, 0x7fffffff, 0x80000000, 0xffffffff]) {
  for (const flags of [2, 0xcd7]) {
    const before = state(0x1000, flags);
    before.registers[0] = operand;
    const expected = copyState(before);
    expected.registers[0] = Number(0xffffffffn ^ BigInt(operand));
    expected.eip += 2;
    call(standalone('not_eax', before), 1, expected, exit(1, 1), `NOT preserves all allowed flags ${operand}/${flags}`);
  }
}
for (const [name, hex, kind, carryTaken] of branches) {
  for (const cf of [0, 1]) for (const operand of [0, 0xffffffff]) {
    const before = state(0x1000, 0x8d6 | cf | (cf ? 0x400 : 0));
    before.registers[0] = operand;
    const computed = unary(kind, operand, before.eflags), expected = copyState(before);
    expected.registers[0] = computed.result;
    expected.eflags = computed.flags;
    const taken = Boolean(computed.flags & 1) === carryTaken;
    expected.eip += hex.length / 2 + (taken ? 2 : 0);
    call(standalone(name, before), 2, expected, exit(1, 2), `carry consumed by ${name}/${operand}/oldCF${cf}`);
  }
}

function checkpointStates(pc) {
  const before = state(pc);
  before.registers[0] = 0xffffffff;
  before.registers[4] = 0x80000000;
  const inc = copyState(before);
  inc.registers[0] = 0;
  inc.eip += 1;
  inc.eflags = 0x457;
  const dec = copyState(inc);
  dec.registers[4] = 0x7fffffff;
  dec.eip += 1;
  dec.eflags = 0xc17;
  const not = copyState(dec);
  not.registers[0] = 0xffffffff;
  not.eip += 2;
  const neg = copyState(not);
  neg.registers[4] = 0x80000001;
  neg.eip += 2;
  neg.eflags = 0x493;
  return {before, inc, dec, not, neg};
}
const partial = checkpointStates(0x1000), checkpoint = standalone('checkpoint', partial.before);
call(checkpoint, 1, partial.inc, exit(1, 1), 'budget1 INC wrap preserves initial carry');
checkpoint.view.setUint32(checkpoint.base + 96, 1, true);
call(checkpoint, 0, partial.inc, exit(2, 0), 'cancel precedes zero budget at unary interior PC');
resumes++;
checkpoint.view.setUint32(checkpoint.base + 96, 0, true);
for (const stage of ['dec', 'not', 'neg']) {
  call(checkpoint, 1, partial[stage], exit(1, 1), `budget1 unary ${stage} commits once`);
  resumes++;
}
call(checkpoint, 1, partial.neg, exit(3, 0), 'standalone unknown continuation retires nothing');
resumes++;
for (const [pc, budget, cancel, reason] of [[0x1000, 0, 0, 1], [0x1003, 10, 0, 3], [0x12345678, 10, 1, 2]]) {
  const before = state(pc, 2);
  call(standalone('checkpoint', before, cancel), budget, before, exit(reason, 0), `unary safepoint ${pc}/${budget}/${cancel}`);
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
  const key = 0x3060000000000000n + ++keys;
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

const counter = engine();
for (const name of ['counter_entry', 'counter_guard', 'counter_body', 'counter_done']) {
  const fixture = assembled.get(name);
  upload(counter, 0x1000 + fixture.offset, fixture.bytes);
}
compile(counter, 'embedded_counter', [0x1200, 0x1205, 0x1209, 0x120d], [[0x120d, 306]]);
const counterInputs = [0, 1, 2, 16, 1000];
for (const input of counterInputs) {
  const before = state(0x1200);
  before.registers[1] = input;
  initialize(counter, before);
  const expected = copyState(before);
  expected.registers[0] = input;
  expected.registers[1] = 0;
  expected.eip = 0x120d;
  expected.eflags = 0x446;
  const retired = 3 * input + 3; // MOV + TEST/JE + input*(INC/DEC/JNZ).
  call(counter, retired + 1, expected, exit(8, retired, 3, 306), `input-dependent resident DEC/JNZ counter ${input}`);
}

const embedded = engine();
upload(embedded, 0x1300, assembled.get('embedded_checkpoint').bytes);
compile(embedded, 'embedded_checkpoint', [0x1300], []);
const full = checkpointStates(0x1300), fullExpected = copyState(full.neg);
fullExpected.eip = 0x1500;
initialize(embedded, full.before);
call(embedded, 10, fullExpected, exit(3, 5), 'actual guard-only all-unary sequence preserves precise flags');
initialize(embedded, full.before); // Independent initial state; no writes between continuation calls.
call(embedded, 1, full.inc, exit(1, 1), 'actual unary budget1 first instruction');
embedded.view.setUint32(embedded.base + 96, 1, true);
call(embedded, 0, full.inc, exit(2, 0), 'actual cancel before unary interior resume');
resumes++;
embedded.view.setUint32(embedded.base + 96, 0, true);
for (const stage of ['dec', 'not', 'neg']) {
  call(embedded, 1, full[stage], exit(1, 1), `actual budget1 ${stage} state/flags commit`);
  resumes++;
}
call(embedded, 1, fullExpected, exit(1, 1), 'actual JMP commits before budget at unknown target');
resumes++;
call(embedded, 1, fullExpected, exit(3, 0), 'actual unknown continuation does not repeat unary operations');
resumes++;

const fault = engine();
upload(fault, 0x1400, assembled.get('unary_fault').bytes);
compile(fault, 'embedded_fault', [0x1400], [], ['read32']);
const faultStart = state(0x1400);
faultStart.registers[0] = 0x80000000;
faultStart.registers[6] = 0x9000;
initialize(fault, faultStart);
const faultExpected = copyState(faultStart);
faultExpected.eip = 0x1402;
faultExpected.eflags = 0xc87;
call(fault, 10, faultExpected, exit(5, 1, 2, 1, 0x9000, 1, 4), 'NEG MIN flags flush before existing precise MOV fault', helper(1, 0, 1, 0x9000, 1, 4));
call(fault, 10, faultExpected, exit(5, 0, 2, 1, 0x9000, 1, 4), 'fault resume preserves unary flags without retiring again', helper(1, 0, 1, 0x9000, 1, 4));
resumes++;

const aggregate = [...generated].sort(([a], [b]) => a.localeCompare(b)).map(([name, digest]) => `${name}:${digest}`).join('\n');
const provenance = {
  claim: 'Own baseline Wasm register-unary32 execution with defined Intel flags; independent BigInt/literal and counter mathematics, no synthetic helper, hardware flags, native C, performance or full P2-V0 claim.',
  standalone_modules: generated.size - 3, embedded_modules: 3, standalone_runs: standaloneRuns, embedded_runs: embeddedRuns, continuation_calls: resumes, synthetic_runs: 0,
  counter_inputs: counterInputs, module_set_sha256: hash(aggregate), engine_sha256: hash(engineBytes),
  sha256: Object.fromEntries(['integer.S', 'run.mjs'].map(name => [name, hash(readFileSync(join(fixtureRoot, name)))]).concat([['dbt_unary_wasm.rs', hash(readFileSync(join(root, 'engine/tests/dbt_unary_wasm.rs')))], ['integer.o', hash(object)]])),
  versions: {clang: execFileSync('clang', ['--version'], {encoding: 'utf8'}).split('\n')[0], objdump: execFileSync('xcrun', ['llvm-objdump', '--version'], {encoding: 'utf8'}).split('\n')[0], node: process.version},
  commands: {assemble: ['clang', ...assemble], disassemble: ['xcrun', 'llvm-objdump', '-d', '--x86-asm-syntax=intel', objectPath]},
  llvm_symbols: llvm, generated_sha256: Object.fromEntries([...generated].sort(([a], [b]) => a.localeCompare(b))),
};
writeFileSync(join(outputDir, 'provenance.json'), `${JSON.stringify(provenance, null, 2)}\n`);
console.log(JSON.stringify({modules: generated.size, standalone_runs: standaloneRuns, embedded_runs: embeddedRuns, continuation_calls: resumes, counter_inputs: counterInputs.length, llvm_symbols: llvm.length, synthetic_runs: 0, engine_sha256: provenance.engine_sha256, module_set_sha256: provenance.module_set_sha256, artifacts: outputDir}));
