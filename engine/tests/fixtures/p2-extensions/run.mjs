import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';

const [enginePath, outputDir, root] = process.argv.slice(2);
assert.ok(root, 'expected actual engine, generated artifacts and repository root');
const fixtureRoot = join(root, 'engine/tests/fixtures/p2-extensions');
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

// extract from the old unsigned register using division/modulo, then apply
// signed subtraction. this deliberately avoids the emitter's shift operations.
function extension(kind, old, width, shift) {
  const modulus = 2n ** BigInt(width);
  const extracted = BigInt(old) / (2n ** BigInt(shift)) % modulus;
  const signed = kind === 'sx' && extracted >= modulus / 2n ? extracted - modulus : extracted;
  return Number(BigInt.asUintN(32, signed));
}
const forms = [
  ['zx_byte_al', '0fb6c0', 'zx', 8, 0, 0, 0], ['zx_byte_cl', '0fb6c9', 'zx', 8, 1, 1, 0],
  ['zx_byte_dl', '0fb6d2', 'zx', 8, 2, 2, 0], ['zx_byte_bl', '0fb6db', 'zx', 8, 3, 3, 0],
  ['zx_byte_ah', '0fb6c4', 'zx', 8, 0, 0, 8], ['zx_byte_ch', '0fb6ed', 'zx', 8, 5, 1, 8],
  ['zx_byte_dh', '0fb6f6', 'zx', 8, 6, 2, 8], ['zx_byte_bh', '0fb6ff', 'zx', 8, 7, 3, 8],
  ['zx_word_ax', '0fb7c0', 'zx', 16, 0, 0, 0], ['zx_word_cx', '0fb7c9', 'zx', 16, 1, 1, 0],
  ['zx_word_dx', '0fb7d2', 'zx', 16, 2, 2, 0], ['zx_word_bx', '0fb7db', 'zx', 16, 3, 3, 0],
  ['zx_word_sp', '0fb7e4', 'zx', 16, 4, 4, 0], ['zx_word_bp', '0fb7ed', 'zx', 16, 5, 5, 0],
  ['zx_word_si', '0fb7f6', 'zx', 16, 6, 6, 0], ['zx_word_di', '0fb7ff', 'zx', 16, 7, 7, 0],
  ['sx_byte_al', '0fbec0', 'sx', 8, 0, 0, 0], ['sx_byte_cl', '0fbec9', 'sx', 8, 1, 1, 0],
  ['sx_byte_dl', '0fbed2', 'sx', 8, 2, 2, 0], ['sx_byte_bl', '0fbedb', 'sx', 8, 3, 3, 0],
  ['sx_byte_ah', '0fbec4', 'sx', 8, 0, 0, 8], ['sx_byte_ch', '0fbeed', 'sx', 8, 5, 1, 8],
  ['sx_byte_dh', '0fbef6', 'sx', 8, 6, 2, 8], ['sx_byte_bh', '0fbeff', 'sx', 8, 7, 3, 8],
  ['sx_word_ax', '0fbfc0', 'sx', 16, 0, 0, 0], ['sx_word_cx', '0fbfc9', 'sx', 16, 1, 1, 0],
  ['sx_word_dx', '0fbfd2', 'sx', 16, 2, 2, 0], ['sx_word_bx', '0fbfdb', 'sx', 16, 3, 3, 0],
  ['sx_word_sp', '0fbfe4', 'sx', 16, 4, 4, 0], ['sx_word_bp', '0fbfed', 'sx', 16, 5, 5, 0],
  ['sx_word_si', '0fbff6', 'sx', 16, 6, 6, 0], ['sx_word_di', '0fbfff', 'sx', 16, 7, 7, 0],
];
const branches = [
  ['sx_ah_jc', '0fbec47202', 'sx', 8, 0, 0, 8, 1, false],
  ['zx_sp_jno', '0fb7e47102', 'zx', 16, 4, 4, 0, 0x800, true],
  ['sx_di_jz', '0fbfff7402', 'sx', 16, 7, 7, 0, 0x40, false],
  ['zx_cl_js', '0fb6c97802', 'zx', 8, 1, 1, 0, 0x80, false],
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
for (const [name, hex] of forms) { verify(name, offset, hex); offset += 3; }
verify('checkpoint', offset, '0fbec40fb7e40fbfe40fb6c0');
offset += 12;
for (const [name, hex] of branches) { verify(name, offset, hex); offset += 7; }
verify('composition', 0x200, '0fb6c10fbed50fbfd901d031d80fb7f00fbffee9e8020000');
verify('embedded_checkpoint', 0x300, '0fbec40fb7e40fbfe40fb6c0e9ef010000');
verify('extension_fault', 0x400, '0fbec48b16e9f6000000');
verify('composition_done', 0x500, '0f0b');
assert.equal(assembled.size, llvm.length, 'all nonempty authored symbols verified');

for (const [name, , kind, width, destination, source, shift] of forms) {
  for (const [operand, flags] of [[0xa5ff807f, 2], [0xff7f80ff, 0xcd7]]) {
    const before = state(0x1000, flags);
    before.registers[source] = operand;
    const expected = copyState(before);
    expected.registers[destination] = extension(kind, operand, width, shift);
    expected.eip += 3;
    call(standalone(name, before), 1, expected, exit(1, 1), `register identity/destination ${name}/${operand}/${flags}`);
  }
}

const byteEdges = [[0, 0], [1, 1], [0x7f, 0x7f], [0x80, 0xffffff80], [0xff, 0xffffffff]];
const wordEdges = [[0, 0], [1, 1], [0x7fff, 0x7fff], [0x8000, 0xffff8000], [0xffff, 0xffffffff]];
for (const name of ['zx_byte_al', 'sx_byte_al', 'zx_byte_ah', 'sx_byte_ah', 'zx_word_sp', 'sx_word_sp']) {
  const [, , kind, width, destination, source, shift] = forms.find(form => form[0] === name);
  for (const [extracted, signed] of width === 8 ? byteEdges : wordEdges) {
    const operand = width === 16 ? 0xa5cc0000 + extracted : shift === 8 ? 0xa5cc00f1 + extracted * 256 : 0xa5cc9900 + extracted;
    const literal = kind === 'zx' ? extracted : signed;
    assert.equal(extension(kind, operand, width, shift), literal, `${name}: independent literal/extraction agreement`);
    for (const flags of [2, 0xcd7]) {
      const before = state(0x1000, flags);
      before.registers[source] = operand;
      const expected = copyState(before);
      expected.registers[destination] = literal;
      expected.eip += 3;
      call(standalone(name, before), 1, expected, exit(1, 1), `literal extension ${name}/${extracted}/flags${flags}`);
    }
  }
}
for (const [name, hex, kind, width, destination, source, shift, mask, inverse] of branches) {
  for (const flags of [2, 0xcd7]) for (const operand of [0, 0xa5cc80ff]) {
    const before = state(0x1000, flags);
    before.registers[source] = operand;
    const expected = copyState(before);
    expected.registers[destination] = extension(kind, operand, width, shift);
    const taken = Boolean(flags & mask) !== inverse;
    expected.eip += hex.length / 2 + (taken ? 2 : 0);
    call(standalone(name, before), 2, expected, exit(1, 2), `Jcc consumes preserved flags ${name}/${operand}/${flags}`);
  }
}

function checkpointStates(pc) {
  const before = state(pc);
  before.registers[0] = 0xa5cc80f1;
  before.registers[4] = 0xdead8001;
  const high = copyState(before);
  high.registers[0] = 0xffffff80;
  high.eip += 3;
  const zeroWord = copyState(high);
  zeroWord.registers[4] = 0x8001;
  zeroWord.eip += 3;
  const signWord = copyState(zeroWord);
  signWord.registers[4] = 0xffff8001;
  signWord.eip += 3;
  const zeroByte = copyState(signWord);
  zeroByte.registers[0] = 0x80;
  zeroByte.eip += 3;
  return {before, high, zeroWord, signWord, zeroByte};
}
const partial = checkpointStates(0x1000), checkpoint = standalone('checkpoint', partial.before);
call(checkpoint, 1, partial.high, exit(1, 1), 'budget1 AH-to-EAX captures old high byte');
checkpoint.view.setUint32(checkpoint.base + 96, 1, true);
call(checkpoint, 0, partial.high, exit(2, 0), 'cancel precedes zero budget at extension interior PC');
resumes++;
checkpoint.view.setUint32(checkpoint.base + 96, 0, true);
for (const stage of ['zeroWord', 'signWord', 'zeroByte']) {
  call(checkpoint, 1, partial[stage], exit(1, 1), `budget1 extension ${stage} commits once and preserves flags`);
  resumes++;
}
call(checkpoint, 1, partial.zeroByte, exit(3, 0), 'standalone unknown continuation does not re-extract source');
resumes++;
for (const [pc, budget, cancel, reason] of [[0x1000, 0, 0, 1], [0x1004, 10, 0, 3], [0x12345678, 10, 1, 2]]) {
  const before = state(pc, 2);
  call(standalone('checkpoint', before, cancel), budget, before, exit(reason, 0), `extension precise safepoint ${pc}/${budget}/${cancel}`);
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
  const key = 0x3070000000000000n + ++keys;
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

function compositionOracle(raw) {
  const low = BigInt(raw) % 256n;
  const byte = BigInt(raw) / 256n % 256n;
  const high = byte >= 128n ? byte - 256n : byte;
  const word = BigInt(raw) % 65536n;
  const signedWord = word >= 32768n ? word - 65536n : word;
  const sum = BigInt.asUintN(32, low + high);
  const result = sum ^ BigInt.asUintN(32, signedWord);
  const lowerWord = result % 65536n;
  const last = lowerWord >= 32768n ? lowerWord - 65536n : lowerWord;
  const parity = [...Number(result % 256n).toString(2)].filter(bit => bit === '1').length % 2 === 0;
  const flags = 0x402 | (parity ? 4 : 0) | (result === 0n ? 0x40 : 0) | (result >= 0x80000000n ? 0x80 : 0);
  return {eax: Number(result), edx: Number(BigInt.asUintN(32, high)), ebx: Number(BigInt.asUintN(32, signedWord)), esi: Number(lowerWord), edi: Number(BigInt.asUintN(32, last)), flags};
}
const composition = engine();
upload(composition, 0x1200, assembled.get('composition').bytes);
upload(composition, 0x1500, assembled.get('composition_done').bytes);
compile(composition, 'embedded_composition', [0x1200, 0x1500], [[0x1500, 307]]);
const compositionInputs = [[0, 0, 0x446], [1, 0, 0x446], [0x7fff, 0x7e81, 0x406], [0xa5cc80ff, 0xffff8080, 0x482], [0x1234ff80, 0xffffffff, 0x486], [0xffff8000, 0x7f80, 0x402]];
for (const [raw, literal, flags] of compositionInputs) {
  const computed = compositionOracle(raw);
  assert.equal(computed.eax, literal, `composition literal result ${raw}`);
  assert.equal(computed.flags, flags, `composition literal flags ${raw}`);
  const before = state(0x1200);
  before.registers[1] = raw;
  initialize(composition, before);
  const expected = copyState(before);
  for (const [index, name] of [[0, 'eax'], [2, 'edx'], [3, 'ebx'], [6, 'esi'], [7, 'edi']]) expected.registers[index] = computed[name];
  expected.eflags = flags;
  expected.eip = 0x1500;
  call(composition, 9, expected, exit(8, 8, 3, 307), `input-dependent resident conversion composition ${raw}`);
}

const embedded = engine();
upload(embedded, 0x1300, assembled.get('embedded_checkpoint').bytes);
compile(embedded, 'embedded_checkpoint', [0x1300], []);
const full = checkpointStates(0x1300), fullExpected = copyState(full.zeroByte);
fullExpected.eip = 0x1500;
initialize(embedded, full.before);
call(embedded, 10, fullExpected, exit(3, 5), 'actual guard-only all-extension sequence preserves every bounded flag');
initialize(embedded, full.before); // independent start; subsequent CPU changes come only from Wasm.
call(embedded, 1, full.high, exit(1, 1), 'actual budget1 old-AH alias extraction');
embedded.view.setUint32(embedded.base + 96, 1, true);
call(embedded, 0, full.high, exit(2, 0), 'actual cancel before extension interior resume');
resumes++;
embedded.view.setUint32(embedded.base + 96, 0, true);
for (const stage of ['zeroWord', 'signWord', 'zeroByte']) {
  call(embedded, 1, full[stage], exit(1, 1), `actual budget1 ${stage} old-source capture`);
  resumes++;
}
call(embedded, 1, fullExpected, exit(1, 1), 'actual JMP commits final budget once');
resumes++;
call(embedded, 1, fullExpected, exit(3, 0), 'actual unknown continuation does not repeat conversion');
resumes++;

const fault = engine();
upload(fault, 0x1400, assembled.get('extension_fault').bytes);
compile(fault, 'embedded_fault', [0x1400], [], ['read32']);
const faultStart = state(0x1400);
faultStart.registers[0] = 0xa5cc80f1;
faultStart.registers[6] = 0x9000;
initialize(fault, faultStart);
const faultExpected = copyState(faultStart);
faultExpected.registers[0] = 0xffffff80;
faultExpected.eip = 0x1403;
call(fault, 10, faultExpected, exit(5, 1, 2, 1, 0x9000, 1, 4), 'AH sign extension commits before precise existing MOV read fault', helper(1, 0, 1, 0x9000, 1, 4));
call(fault, 10, faultExpected, exit(5, 0, 2, 1, 0x9000, 1, 4), 'MOV fault retry preserves converted EAX and all flags without retirement', helper(1, 0, 1, 0x9000, 1, 4));
resumes++;

const aggregate = [...generated].sort(([a], [b]) => a.localeCompare(b)).map(([name, digest]) => `${name}:${digest}`).join('\n');
const provenance = {
  claim: 'Own baseline Wasm register-source MOVZX/MOVSX32 execution; unsigned extraction/sign-subtraction oracle and resident conversion composition, no memory conversion, synthetic helpers, native C, hardware flags, performance or full P2-V0 claim.',
  standalone_modules: generated.size - 3, embedded_modules: 3, standalone_runs: standaloneRuns, embedded_runs: embeddedRuns, continuation_calls: resumes, synthetic_runs: 0,
  composition_inputs: compositionInputs, module_set_sha256: hash(aggregate), engine_sha256: hash(engineBytes),
  sha256: Object.fromEntries(['integer.S', 'run.mjs'].map(name => [name, hash(readFileSync(join(fixtureRoot, name)))]).concat([['dbt_extensions_wasm.rs', hash(readFileSync(join(root, 'engine/tests/dbt_extensions_wasm.rs')))], ['integer.o', hash(object)]])),
  versions: {clang: execFileSync('clang', ['--version'], {encoding: 'utf8'}).split('\n')[0], objdump: execFileSync('xcrun', ['llvm-objdump', '--version'], {encoding: 'utf8'}).split('\n')[0], node: process.version},
  commands: {assemble: ['clang', ...assemble], disassemble: ['xcrun', 'llvm-objdump', '-d', '--x86-asm-syntax=intel', objectPath]},
  llvm_symbols: llvm, generated_sha256: Object.fromEntries([...generated].sort(([a], [b]) => a.localeCompare(b))),
};
writeFileSync(join(outputDir, 'provenance.json'), `${JSON.stringify(provenance, null, 2)}\n`);
console.log(JSON.stringify({modules: generated.size, standalone_runs: standaloneRuns, embedded_runs: embeddedRuns, continuation_calls: resumes, composition_inputs: compositionInputs.length, llvm_symbols: llvm.length, synthetic_runs: 0, engine_sha256: provenance.engine_sha256, module_set_sha256: provenance.module_set_sha256, artifacts: outputDir}));
