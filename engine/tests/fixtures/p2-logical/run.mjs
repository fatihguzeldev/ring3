import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';

const [enginePath, outputDir, root] = process.argv.slice(2);
assert.ok(root, 'expected actual engine, generated artifacts and repository root');
const fixtureRoot = join(root, 'engine/tests/fixtures/p2-logical');
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

// BigInt keeps all operand/result calculations unsigned; flags are independent
// of the emitter. AF=0 is the frozen Ring3 policy for Intel's undefined AF.
function logical(kind, left, right, oldFlags) {
  const a = BigInt(left), b = BigInt(right);
  const value = kind === 'or' ? a | b : kind === 'xor' ? a ^ b : a & b;
  const result = BigInt.asUintN(32, value);
  const lowByte = Number(result % 256n);
  const evenParity = [...lowByte.toString(2)].filter(bit => bit === '1').length % 2 === 0;
  const flags = 2 | (oldFlags & 0x400) | (evenParity ? 4 : 0) | (result === 0n ? 0x40 : 0) | (result >= 0x80000000n ? 0x80 : 0);
  return {result: Number(result), flags};
}

const forms = [
  ['and_rm_reg', '21d8', 'and', 3], ['and_reg_rm', '23c3', 'and', 3],
  ['and_eax_imm32', '2503010080', 'and', 0x80000103], ['and_modrm_imm32', '81e003010080', 'and', 0x80000103],
  ['and_imm8_min', '83e080', 'and', 0xffffff80], ['and_imm8_minus1', '83e0ff', 'and', 0xffffffff], ['and_imm8_max', '83e07f', 'and', 127],
  ['or_rm_reg', '09d8', 'or', 3], ['or_reg_rm', '0bc3', 'or', 3],
  ['or_eax_imm32', '0d03010080', 'or', 0x80000103], ['or_modrm_imm32', '81c803010080', 'or', 0x80000103],
  ['or_imm8_min', '83c880', 'or', 0xffffff80], ['or_imm8_minus1', '83c8ff', 'or', 0xffffffff], ['or_imm8_max', '83c87f', 'or', 127],
  ['xor_rm_reg', '31d8', 'xor', 3], ['xor_reg_rm', '33c3', 'xor', 3],
  ['xor_eax_imm32', '3503010080', 'xor', 0x80000103], ['xor_modrm_imm32', '81f003010080', 'xor', 0x80000103],
  ['xor_imm8_min', '83f080', 'xor', 0xffffff80], ['xor_imm8_minus1', '83f0ff', 'xor', 0xffffffff], ['xor_imm8_max', '83f07f', 'xor', 127],
  ['test_rm_reg', '85d8', 'test', 3], ['test_eax_imm32', 'a903010080', 'test', 0x80000103], ['test_modrm_imm32', 'f7c003010080', 'test', 0x80000103],
];
const aliases = [
  ['and_esp_same', '21e4', 'and', 4, 4], ['or_esp_eax', '09c4', 'or', 4, 0],
  ['xor_eax_esp', '31e0', 'xor', 0, 4], ['test_esp_same', '85e4', 'test', 4, 4],
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
const object = readFileSync(objectPath), assembled = elfFixtures(object);
const llvm = [];
function verify(name, offset, hex) {
  const fixture = assembled.get(name);
  assert.ok(fixture, `${name}: authored symbol`);
  assert.equal(fixture.offset, offset, `${name}: LLVM offset`);
  assert.equal(fixture.bytes.toString('hex'), hex, `${name}: independent literal encoding`);
  llvm.push({name, offset, hex});
}
let offset = 0;
for (const [name, hex] of [...forms, ...aliases]) {
  verify(name, offset, hex);
  offset += hex.length / 2;
}
verify('checkpoint', offset, '21d809c485c0');
offset += 6;
for (let condition = 0; condition < 16; condition++) {
  verify(`jcc_${condition}`, offset, `85c0${(0x70 + condition).toString(16)}02`);
  offset += 6; // Two LLVM target padding bytes are outside each four-byte symbol.
}
verify('mask_entry', 0x200, '31c0');
verify('mask_guard', 0x202, '85c97417');
verify('mask_body', 0x206, '89ca81e2ff00000081ca0001000031da01d083e901ebe5');
verify('mask_done', 0x21d, '0f0b');
verify('logical_fault', 0x300, '31c08b16e9f7000000');
assert.equal(assembled.size, llvm.length, 'all nonempty authored symbols verified');

for (const [name, hex, kind, source] of forms) {
  for (const [left, regRight, df] of [[0x7fffffff, 0x80000103, 0], [0x80000001, 0xf0ff807f, 0x400]]) {
    const before = state(0x1000, 0x8d7 | df);
    before.registers[0] = left;
    before.registers[3] = regRight;
    const right = name.includes('_reg') || name.includes('_rm') ? regRight : source;
    const computed = logical(kind, left, right, before.eflags), expected = copyState(before);
    if (kind !== 'test') expected.registers[0] = computed.result;
    expected.eflags = computed.flags;
    expected.eip += hex.length / 2;
    call(standalone(name, before), 1, expected, exit(1, 1), `opcode ${name}/df${df}`);
  }
}

// Literal anchors distinguish low-byte parity from parity of the entire word.
const literalFlags = [[0, 0x46], [1, 2], [3, 6], [0x80000000, 0x86], [0x80000001, 0x82], [0x80000103, 0x86], [0x101, 2]];
for (const kind of ['and', 'or', 'xor', 'test']) {
  for (const [result, flags] of literalFlags) {
    const left = kind === 'or' ? 0 : result;
    const right = kind === 'or' ? result : kind === 'xor' ? 0 : 0xffffffff;
    for (const df of [0, 0x400]) {
      assert.deepEqual(logical(kind, left, right, 0x8d7 | df), {result, flags: flags | df}, `${kind}: BigInt/literal flag agreement`);
      const before = state(0x1000, 0x8d7 | df);
      before.registers[0] = left;
      before.registers[3] = right;
      const expected = copyState(before);
      if (kind !== 'test') expected.registers[0] = result;
      expected.eflags = flags | df;
      expected.eip += 2;
      call(standalone(`${kind}_rm_reg`, before), 1, expected, exit(1, 1), `literal ${kind}/${result}/df${df}`);
    }
  }
}

for (const [name, , kind, destination, source] of aliases) {
  for (const [eax, esp] of [[0x80000001, 0x80000001], [0x80000103, 0x7fffffff]]) {
    const before = state();
    before.registers[0] = eax;
    before.registers[4] = esp;
    const computed = logical(kind, before.registers[destination], before.registers[source], before.eflags);
    const expected = copyState(before);
    if (kind !== 'test') expected.registers[destination] = computed.result;
    expected.eflags = computed.flags;
    expected.eip += 2;
    call(standalone(name, before), 1, expected, exit(1, 1), `captured alias ${name}/${eax}/${esp}`);
  }
}

const conditions = [
  f => f.of, f => !f.of, f => f.cf, f => !f.cf,
  f => f.zf, f => !f.zf, f => f.cf || f.zf, f => !f.cf && !f.zf,
  f => f.sf, f => !f.sf, f => f.pf, f => !f.pf,
  f => f.sf !== f.of, f => f.sf === f.of, f => f.zf || f.sf !== f.of, f => !f.zf && f.sf === f.of,
];
for (let condition = 0; condition < 16; condition++) {
  for (const [input, literal] of [[0, 0x446], [1, 0x402], [3, 0x406], [0x80000001, 0x482]]) {
    const before = state();
    before.registers[0] = input;
    const f = {cf: false, of: false, zf: input === 0, sf: input >= 0x80000000, pf: (literal & 4) !== 0};
    const expected = copyState(before);
    expected.eip += conditions[condition](f) ? 6 : 4;
    expected.eflags = literal;
    call(standalone(`jcc_${condition}`, before), 2, expected, exit(1, 2), `logical flags consumed by Jcc ${condition}/${input}`);
  }
}

const checkpointStart = state();
checkpointStart.registers[0] = 0x80000103;
checkpointStart.registers[3] = 0x80000001;
checkpointStart.registers[4] = 2;
const checkpoint = standalone('checkpoint', checkpointStart);
const first = copyState(checkpointStart);
first.registers[0] = 0x80000001;
first.eip = 0x1002;
first.eflags = 0x482;
call(checkpoint, 1, first, exit(1, 1), 'budget1 commits AND once');
checkpoint.view.setUint32(checkpoint.base + 96, 1, true);
call(checkpoint, 0, first, exit(2, 0), 'cancel precedes zero budget at interior PC');
resumes++;
checkpoint.view.setUint32(checkpoint.base + 96, 0, true);
const second = copyState(first);
second.registers[4] = 0x80000003;
second.eip = 0x1004;
second.eflags = 0x486;
call(checkpoint, 1, second, exit(1, 1), 'resume OR captures current EAX and old ESP');
resumes++;
const third = copyState(second);
third.eip = 0x1006;
third.eflags = 0x482;
call(checkpoint, 1, third, exit(1, 1), 'resume TEST preserves every register');
resumes++;
call(checkpoint, 1, third, exit(3, 0), 'positive budget at unknown continuation is NeedCode');
resumes++;
for (const [pc, budget, cancel, reason] of [[0x1000, 0, 0, 1], [0x1000, 10, 1, 2], [0x1003, 10, 0, 3], [0x12345678, 0, 0, 1], [0x12345678, 0, 1, 2]]) {
  const before = state(pc);
  call(standalone('checkpoint', before, cancel), budget, before, exit(reason, 0), `safepoint pc${pc}/budget${budget}/cancel${cancel}`);
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
  const key = 0x3040000000000000n + ++keys;
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

const loop = engine();
for (const name of ['mask_entry', 'mask_guard', 'mask_body', 'mask_done']) {
  const fixture = assembled.get(name);
  upload(loop, 0x1000 + fixture.offset, fixture.bytes);
}
compile(loop, 'embedded_mask', [0x1200, 0x1202, 0x1206, 0x121d], [[0x121d, 304]]);
const cSource = join(fixtureRoot, 'logical_oracle.c'), cBinary = join(outputDir, 'logical_oracle');
const nativeCommand = ['-std=c11', '-O2', '-Wall', '-Wextra', '-Werror', cSource, '-o', cBinary];
execFileSync('clang', nativeCommand, {stdio: ['ignore', 'pipe', 'pipe']});
const nativeInputs = [[0, 0], [1, 0], [2, 3], [10, 0xffffffff], [255, 0x80000000], [256, 0x80000103], [1000, 0x12345678]];
function maskOracle(count, mask) {
  let sum = 0n, last = 0n;
  for (let remaining = BigInt(count); remaining > 0n; remaining--) {
    last = ((remaining & 255n) | 256n) ^ BigInt(mask);
    sum = BigInt.asUintN(32, sum + last);
  }
  return {sum: Number(sum), remaining: 0, last: Number(last), retired: 9 * count + 3};
}

for (const [count, mask] of nativeInputs) {
  const native = JSON.parse(execFileSync(cBinary, [String(count), String(mask)], {encoding: 'utf8'}));
  assert.deepEqual(native, maskOracle(count, mask), `independent unsigned native C algorithm ${count}/${mask}`);
  const before = state(0x1200);
  before.registers[1] = count;
  before.registers[3] = mask;
  initialize(loop, before);
  const expected = copyState(before);
  expected.registers[0] = native.sum;
  expected.registers[1] = 0;
  if (count !== 0) expected.registers[2] = native.last;
  expected.eip = 0x121d;
  expected.eflags = 0x446;
  call(loop, native.retired + 1, expected, exit(8, native.retired, 3, 304), `embedded native algorithm ${count}/${mask}`);
}

// No CPU state writes occur between these actual engine checkpoints.
const resumedStart = state(0x1200);
resumedStart.registers[1] = 2;
resumedStart.registers[3] = 3;
initialize(loop, resumedStart);
const loopAfterXor = copyState(resumedStart);
loopAfterXor.registers[0] = 0;
loopAfterXor.eip = 0x1202;
loopAfterXor.eflags = 0x446;
call(loop, 1, loopAfterXor, exit(1, 1, 3), 'embedded budget1 XOR zeroing');
loop.view.setUint32(loop.base + 96, 1, true);
call(loop, 100, loopAfterXor, exit(2, 0, 3), 'embedded cancellation before next logical instruction');
resumes++;
loop.view.setUint32(loop.base + 96, 0, true);
const loopEnd = copyState(resumedStart), loopResult = maskOracle(2, 3);
loopEnd.registers[0] = loopResult.sum;
loopEnd.registers[1] = 0;
loopEnd.registers[2] = loopResult.last;
loopEnd.eip = 0x121d;
loopEnd.eflags = 0x446;
call(loop, 100, loopEnd, exit(8, 20, 3, 304), 'embedded resume finishes resident mask loop exactly once');
resumes++;

const fault = engine();
upload(fault, 0x1300, assembled.get('logical_fault').bytes);
compile(fault, 'embedded_fault', [0x1300], [], ['read32']);
const faultStart = state(0x1300);
faultStart.registers[6] = 0x9000;
initialize(fault, faultStart);
const faultExpected = copyState(faultStart);
faultExpected.registers[0] = 0;
faultExpected.eip = 0x1302;
faultExpected.eflags = 0x446;
call(fault, 10, faultExpected, exit(5, 1, 2, 1, 0x9000, 1, 4), 'logical flags flush before precise failed MOV', helper(1, 0, 1, 0x9000, 1, 4));
call(fault, 10, faultExpected, exit(5, 0, 2, 1, 0x9000, 1, 4), 'fault resume preserves logical result and retires no instruction', helper(1, 0, 1, 0x9000, 1, 4));
resumes++;

const aggregate = [...generated].sort(([a], [b]) => a.localeCompare(b)).map(([name, digest]) => `${name}:${digest}`).join('\n');
const provenance = {
  claim: 'Own baseline Wasm logical-register execution; native C is unsigned algorithm evidence, not hardware flags. No synthetic helpers, performance or full P2-V0 claim.',
  standalone_modules: generated.size - 2, embedded_modules: 2, standalone_runs: standaloneRuns, embedded_runs: embeddedRuns, resumes, native_cases: nativeInputs.length, synthetic_runs: 0,
  module_set_sha256: hash(aggregate), engine_sha256: hash(engineBytes),
  sha256: Object.fromEntries(['integer.S', 'logical_oracle.c', 'run.mjs'].map(name => [name, hash(readFileSync(join(fixtureRoot, name)))]).concat([['dbt_logical_wasm.rs', hash(readFileSync(join(root, 'engine/tests/dbt_logical_wasm.rs')))], ['integer.o', hash(object)], ['logical_oracle', hash(readFileSync(cBinary))]])),
  versions: {clang: execFileSync('clang', ['--version'], {encoding: 'utf8'}).split('\n')[0], objdump: execFileSync('xcrun', ['llvm-objdump', '--version'], {encoding: 'utf8'}).split('\n')[0], node: process.version},
  commands: {assemble: ['clang', ...assemble], disassemble: ['xcrun', 'llvm-objdump', '-d', '--x86-asm-syntax=intel', objectPath], native: ['clang', ...nativeCommand]},
  llvm_symbols: llvm, generated_sha256: Object.fromEntries([...generated].sort(([a], [b]) => a.localeCompare(b))), native_inputs: nativeInputs,
};
writeFileSync(join(outputDir, 'provenance.json'), `${JSON.stringify(provenance, null, 2)}\n`);
console.log(JSON.stringify({modules: generated.size, standalone_runs: standaloneRuns, embedded_runs: embeddedRuns, resumes, native_cases: nativeInputs.length, llvm_symbols: llvm.length, synthetic_runs: 0, engine_sha256: provenance.engine_sha256, module_set_sha256: provenance.module_set_sha256, artifacts: outputDir}));
