import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(root, 'expected engine wasm, output directory and repository root');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const engineBytes = readFileSync(enginePath), engineModule = new WebAssembly.Module(engineBytes);
assert.deepEqual(WebAssembly.Module.imports(engineModule), []);
assert.equal(WebAssembly.Module.exports(engineModule).length, 71);
assert.equal(WebAssembly.Module.exports(engineModule).filter(row => row.kind === 'function').length, 70);
const SIZE = 4236, TRANSFER = 140, EXIT = 0x8020, EXIT_ID = 0x10003, MARKER = 0xc8c00008;
const initialRegisters = [0x1234807f, 0x23457f80, 0x34560f10, 0x4567ff00, 0x56789abc, 0x6789abcd, 0x789abcde, 0x89abcdef];
const boundaries = [0, 1, 15, 16, 127, 128, 255];
const apiNames = ['open', 'close', 'arena_ptr', 'map', 'upload', 'read32', 'write8', 'compile_entries', 'compile_resident_entries', 'generation', 'module_ptr', 'module_len', 'guard', 'guard_resident', 'store32', 'store_resident32', 'begin_image_input', 'append_image_input', 'load_pe32_linked_v2_input_at', 'start_loaded_image', 'capture_call', 'capture_resident_call', 'complete_windows_call', 'complete_resident_windows_call'];
const sourcePaths = ['Cargo.toml', 'Cargo.lock', 'engine/Cargo.toml', 'engine/src/cpu/x86/ir.rs', 'engine/src/cpu/x86/decode/decoder.rs', 'engine/src/cpu/x86/decode/integer.rs', 'engine/src/cpu/x86/decode/operands.rs', 'engine/src/cpu/x86/decode/flow.rs', 'engine/src/cpu/dbt/region.rs', 'engine/src/cpu/dbt/wasm/integer.rs', 'engine/src/cpu/dbt/wasm/control.rs', 'engine/src/cpu/dbt/wasm/memory.rs', 'engine/src/process/instance.rs', 'engine/src/process/wasm.rs', 'engine/src/process/call.rs', 'engine/src/process/windows.rs', 'engine/src/abi/wasm/exports.rs', 'engine/src/loader/mod.rs', 'engine/src/loader/imports.rs', 'engine/src/loader/relocation.rs', 'engine/tests/support/pe32.rs', 'engine/tests/cpu_byte_arithmetic_wasm.rs', 'engine/tests/fixtures/p2-byte-arithmetic/run.mjs', 'engine/tests/fixtures/p2-byte-arithmetic/integer.S'];
const sourceHashes = Object.fromEntries(sourcePaths.map(path => [path, hash(readFileSync(join(root, path)))]));
writeFileSync(join(output, 'source-hashes.json'), JSON.stringify({engine_sha256: hash(engineBytes), sources: sourceHashes}, null, 2));
const rows = [], controls = [], peRows = [], modules = [];
let ordinal = 0;

function record(magic, length, fields, version = 1) {
  const bytes = new Uint8Array(length), view = new DataView(bytes.buffer);
  bytes.set(Buffer.from(magic)); view.setUint16(4, version, true); view.setUint16(6, 1, true); view.setUint32(8, length, true);
  fields.forEach((value, index) => view.setUint32(16 + index * 4, value, true)); return bytes;
}
function words(fields) {
  const bytes = new Uint8Array(fields.length * 4), view = new DataView(bytes.buffer);
  fields.forEach((value, index) => view.setUint32(index * 4, value, true)); return bytes;
}
function refresh(ctx) { ctx.bytes = new Uint8Array(ctx.memory.buffer); ctx.view = new DataView(ctx.memory.buffer); return ctx; }
function arena(ctx) { return refresh(ctx).bytes.slice(ctx.base, ctx.base + SIZE); }
function stateRecord(registers, pc, flags) { return record('R3ST', 56, [...registers, pc, flags]); }
function exitRecord(reason, count, version = 1, detail = 0) { return record('R3EX', 40, [reason, count, detail, 0, 0, 0], version); }
function request(ctx, bytes) { assert.ok(bytes.length <= 4096); refresh(ctx).bytes.set(bytes, ctx.base + TRANSFER); }
function pure(ctx, action, status, name) {
  const before = arena(ctx); assert.equal(action(), status, name); assert.deepEqual(arena(ctx), before, `${name}: full arena unchanged`);
  controls.push({owner: ctx.owner, name, status});
}
function fresh(owner, pages = 8) {
  const instance = new WebAssembly.Instance(engineModule, {}), key = 0xc836300000000000n + BigInt(++ordinal);
  const api = Object.fromEntries(apiNames.map(name => [name, instance.exports[`ring3_abi_v1_${name}`]]));
  for (const [name, fn] of Object.entries(api)) assert.equal(typeof fn, 'function', name);
  assert.equal(api.guard.length, 6); assert.equal(api.guard_resident.length, 7);
  const ctx = {owner, memory: instance.exports.memory, api, low: Number(key & 0xffffffffn), high: Number(key >> 32n)};
  assert.equal(api.open(pages, ctx.low, ctx.high), 0); ctx.base = api.arena_ptr() >>> 0; return refresh(ctx);
}
function upload(ctx, address, bytes) { request(ctx, bytes); assert.equal(ctx.api.upload(address, bytes.length), 0); refresh(ctx); }
function instantiate(ctx, bytes, label, binding = {}) {
  const module = new WebAssembly.Module(bytes), imports = WebAssembly.Module.imports(module);
  assert.deepEqual(imports[0], {module: 'env', name: 'memory', kind: 'memory'});
  const helpers = imports.slice(1).map(row => {
    assert.equal(row.module, 'ring3'); assert.equal(row.kind, 'function'); assert.equal(typeof ctx.api[row.name], 'function'); return row.name;
  });
  assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  const instance = new WebAssembly.Instance(module, {env: {memory: ctx.memory}, ring3: ctx.api});
  assert.equal(instance.exports.run.length, 4); writeFileSync(join(output, `${label}.wasm`), bytes);
  modules.push({label, owner: ctx.owner, sha256: hash(bytes), imports, ...binding});
  return {...binding, run: instance.exports.run, helpers};
}
function compile(ctx, entries, gates, label, extraHelpers = []) {
  assert.ok(entries.length <= 8); request(ctx, words([...entries, ...gates.flat()]));
  let binding;
  if (ctx.owner === 'replacement') {
    assert.equal(ctx.api.compile_entries(entries.length, gates.length), 0);
    binding = {generation: ctx.api.generation(), pointer: ctx.api.module_ptr() >>> 0, length: ctx.api.module_len() >>> 0};
  } else {
    assert.equal(ctx.api.compile_resident_entries(entries.length, gates.length), 0); refresh(ctx);
    const at = ctx.base + TRANSFER;
    assert.equal(ctx.view.getUint32(at, true), 1); assert.equal(ctx.view.getUint32(at + 4, true), 24);
    binding = {low: ctx.view.getUint32(at + 8, true), high: ctx.view.getUint32(at + 12, true), pointer: ctx.view.getUint32(at + 16, true), length: ctx.view.getUint32(at + 20, true)};
    assert.ok(binding.low !== 0 || binding.high !== 0);
  }
  refresh(ctx); assert.ok(binding.pointer > 0 && binding.length > 8 && binding.pointer + binding.length <= ctx.bytes.length);
  const unit = instantiate(ctx, ctx.bytes.slice(binding.pointer, binding.pointer + binding.length), label, binding);
  assert.deepEqual(unit.helpers, [ctx.owner === 'replacement' ? 'guard' : 'guard_resident', ...extraHelpers]); return unit;
}
function seed(ctx, registers, pc, flags, cancel = 0) {
  refresh(ctx).bytes.set(stateRecord(registers, pc, flags), ctx.base); ctx.bytes.set(exitRecord(3, 0), ctx.base + 56);
  ctx.view.setUint32(ctx.base + 96, cancel, true); ctx.bytes.fill(0xa5, ctx.base + 100, ctx.base + SIZE);
}
function byte(registers, index) { return Math.floor(registers[index % 4] / (index < 4 ? 1 : 256)) % 256; }
function replaceByte(registers, index, value) {
  const factor = index < 4 ? 1 : 256, parent = index % 4; registers[parent] += (value - byte(registers, index)) * factor;
}
function arithmetic(kind, left, right, oldFlags) {
  const wide = kind === 'add' ? left + right : left - right;
  const result = (wide % 256 + 256) % 256;
  const carry = kind === 'add' ? wide > 255 : left < right;
  const auxiliary = kind === 'add' ? left % 16 + right % 16 > 15 : left % 16 < right % 16;
  const signedLeft = left >= 128 ? left - 256 : left, signedRight = right >= 128 ? right - 256 : right;
  const signedResult = kind === 'add' ? signedLeft + signedRight : signedLeft - signedRight;
  const overflow = signedResult < -128 || signedResult > 127;
  let ones = 0; for (let value = result; value > 0; value = Math.floor(value / 2)) ones += value % 2;
  const flags = (oldFlags & 0x400) | 2 | Number(carry) | (ones % 2 === 0 ? 4 : 0)
    | (auxiliary ? 0x10 : 0) | (result === 0 ? 0x40 : 0) | (result >= 128 ? 0x80 : 0) | (overflow ? 0x800 : 0);
  return {result, flags};
}
assert.deepEqual(arithmetic('add', 0xff, 1, 2), {result: 0, flags: 0x57});
assert.deepEqual(arithmetic('add', 0x7f, 1, 2), {result: 0x80, flags: 0x892});
assert.deepEqual(arithmetic('add', 0x80, 0x80, 2), {result: 0, flags: 0x847});
assert.deepEqual(arithmetic('add', 0, 0, 0xcd7), {result: 0, flags: 0x446});
assert.deepEqual(arithmetic('sub', 0, 1, 2), {result: 0xff, flags: 0x97});
assert.deepEqual(arithmetic('sub', 0x80, 1, 2), {result: 0x7f, flags: 0x812});
assert.deepEqual(arithmetic('sub', 0x7f, 0xff, 2), {result: 0x80, flags: 0x883});
assert.deepEqual(arithmetic('sub', 0x80, 0x80, 0xcd7), {result: 0, flags: 0x446});
function runArithmetic(ctx, unit, form, registers, flags, name, boundary = false) {
  seed(ctx, registers, form.pc, flags);
  const before = arena(ctx), expected = before.slice(), nextRegisters = [...registers];
  const left = byte(registers, form.destination), right = form.immediate ?? byte(registers, form.source);
  const outcome = arithmetic(form.kind, left, right, flags); replaceByte(nextRegisters, form.destination, outcome.result);
  expected.set(stateRecord(nextRegisters, form.pc + form.bytes.length, outcome.flags)); expected.set(exitRecord(1, 1), 56);
  const wholeMemory = ctx.owner === 'standalone' ? ctx.bytes.slice() : undefined;
  assert.equal(unit.run(ctx.base, ctx.base + 56, 1, ctx.base + 96), 0, name);
  assert.deepEqual(arena(ctx), expected, `${name}: full registers, flags, EIP, exit, helper and transfer`);
  if (wholeMemory) { wholeMemory.set(expected, ctx.base); assert.deepEqual(ctx.bytes, wholeMemory, `${name}: full standalone memory`); }
  rows.push({owner: ctx.owner, name, boundary, kind: form.kind, family: form.family, destination: form.destination, encoding: Buffer.from(form.bytes).toString('hex'), left, right, result: outcome.result, initial_flags: flags, flags: outcome.flags});
}
function noRetirement(ctx, unit, form, cancel) {
  seed(ctx, initialRegisters, form.pc, 0xcd7, cancel);
  const expected = arena(ctx); expected.set(exitRecord(cancel ? 2 : 1, 0), 56);
  assert.equal(unit.run(ctx.base, ctx.base + 56, 0, ctx.base + 96), 0); assert.deepEqual(arena(ctx), expected);
  controls.push({owner: ctx.owner, name: cancel ? 'cancel_before_zero_budget' : 'zero_budget', retired: 0});
}
function read(ctx, address) {
  const before = arena(ctx); assert.equal(ctx.api.read32(address), 0); refresh(ctx);
  assert.equal(ctx.view.getUint32(ctx.base + 116, true), 0); const value = ctx.view.getUint32(ctx.base + 120, true), expected = before.slice();
  expected.set(record('R3MH', 40, [0, value, 0, 0, 0, 0]), 100); assert.deepEqual(arena(ctx), expected, 'host read changes only helper'); return value;
}
function readWords(ctx, address, count) { return Array.from({length: count}, (_, index) => read(ctx, address + index * 4)); }

const forms = [];
for (const [kind, rmReg, regRm, alImm, group] of [['add', 0x00, 0x02, 0x04, 0], ['sub', 0x28, 0x2a, 0x2c, 5]]) {
  for (const [family, opcode] of [['rm_reg', rmReg], ['reg_rm', regRm]]) for (let destination = 0; destination < 8; destination++) for (let source = 0; source < 8; source++) {
    const modrm = 0xc0 + (family === 'rm_reg' ? source * 8 + destination : destination * 8 + source);
    forms.push({kind, family, destination, source, bytes: [opcode, modrm]});
  }
  for (let destination = 0; destination < 8; destination++) for (const immediate of [0, 0x7f, 0x80, 0xff]) forms.push({kind, family: 'modrm_imm', destination, immediate, bytes: [0x80, 0xc0 + group * 8 + destination, immediate]});
  for (const immediate of [0, 0x7f, 0x80, 0xff]) forms.push({kind, family: 'al_imm', destination: 0, immediate, bytes: [alImm, immediate]});
}
assert.equal(forms.length, 328); forms.forEach((form, index) => { form.ordinal = index; });
const batches = [];
for (let index = 0; index < forms.length; index += 63) {
  const group = forms.slice(index, index + 63), entry = 0x1000 + batches.length * 0x200;
  let pc = entry; group.forEach(form => { form.pc = pc; pc += form.bytes.length; });
  const code = Buffer.from([...group.flatMap(form => form.bytes), 0xeb, 0, 0x0f, 0x0b]);
  assert.ok(entry + code.length <= 0x2000); assert.ok(group.length + 1 <= 64);
  assert.deepEqual(code, readFileSync(join(output, `batch-${batches.length}.x86`)), 'independent Rust/JS encoding agrees'); batches.push({entry, forms: group, code});
}
assert.equal(batches.length, 6); assert.equal(batches.at(-1).entry, 0x1a00);
for (const owner of ['standalone', 'replacement', 'resident']) {
  const ctx = owner === 'standalone' ? refresh({owner, memory: new WebAssembly.Memory({initial: 1}), base: 128}) : fresh(owner);
  if (owner !== 'standalone') {
    assert.equal(ctx.api.map(0x1000, 1, 7), 0); assert.equal(ctx.api.map(0x9000, 1, 3), 0);
    upload(ctx, 0x9000, Buffer.from('00112233445566778899aabbccddeeff0123456789abcdefa5a5a5a55a5a5a5a', 'hex'));
    // no shared code-page writes occur after the first bound compilation.
    for (const batch of batches) upload(ctx, batch.entry, batch.code);
  }
  let lastUnit;
  for (const [index, batch] of batches.entries()) {
    const label = `${owner}-batch${index}`;
    const unit = owner === 'standalone' ? instantiate(ctx, readFileSync(join(output, `batch-${index}.wasm`)), label) : compile(ctx, [batch.entry], [], label);
    assert.deepEqual(unit.helpers, owner === 'standalone' ? [] : [owner === 'replacement' ? 'guard' : 'guard_resident'], 'arithmetic bytes add no RAM helper');
    const probeBefore = owner === 'standalone' ? undefined : readWords(ctx, 0x9000, 8);
    for (const form of batch.forms) runArithmetic(ctx, unit, form, [...initialRegisters], form.ordinal % 2 ? 2 : 0xcd7, `${label}-form${form.ordinal}`);
    for (const form of batch.forms.filter(form => form.family === 'rm_reg' && form.destination === 0 && form.source === 1)) {
      for (const left of boundaries) for (const right of boundaries) {
        const registers = [...initialRegisters]; replaceByte(registers, 0, left); replaceByte(registers, 1, right);
        runArithmetic(ctx, unit, form, registers, 0xcd7, `${label}-${form.kind}-boundary${left}-${right}`, true);
      }
    }
    noRetirement(ctx, unit, batch.forms[0], 0); noRetirement(ctx, unit, batch.forms[0], 1);
    if (probeBefore) assert.deepEqual(readWords(ctx, 0x9000, 8), probeBefore, `${label}: declared32 guest RAM bytes unchanged`);
    lastUnit = unit;
  }
  if (owner !== 'standalone') {
    const guard = (high, identity, pointer = ctx.base) => owner === 'replacement'
      ? ctx.api.guard(ctx.low, high, identity, pointer, ctx.base + 56, ctx.base + 96)
      : ctx.api.guard_resident(ctx.low, high, lastUnit.low, identity, pointer, ctx.base + 56, ctx.base + 96);
    const identity = owner === 'replacement' ? lastUnit.generation : lastUnit.high;
    pure(ctx, () => guard(ctx.high + 1, identity), 3, `${owner}: high-key refusal`);
    pure(ctx, () => guard(ctx.high, identity + 1), 3, `${owner}: wrong generation/id refusal`);
    pure(ctx, () => guard(ctx.high, identity, 0xffffffff), 1, `${owner}: current guard malformed pointer`);
    pure(ctx, () => lastUnit.run(0xffffffff, 0xffffffff, 0, 0xffffffff), 1, `${owner}: current generated entry malformed pointers`);
    assert.equal(ctx.api.write8(0x1000, batches[0].code[0]), 0);
    pure(ctx, () => lastUnit.run(0xffffffff, 0xffffffff, 0, 0xffffffff), 4, `${owner}: same-byte code stale before pointers`);
    assert.equal(ctx.api.close(), 0);
    pure(ctx, () => lastUnit.run(0xffffffff, 0xffffffff, 0, 0xffffffff), 5, `${owner}: closed before pointers`);
  }
}

function elfFixtures(object) {
  const view = new DataView(object.buffer, object.byteOffset, object.byteLength);
  assert.deepEqual([...object.subarray(0, 7)], [0x7f, 0x45, 0x4c, 0x46, 1, 1, 1]); assert.equal(view.getUint16(18, true), 3);
  const start = view.getUint32(32, true), stride = view.getUint16(46, true), count = view.getUint16(48, true);
  const sections = Array.from({length: count}, (_, index) => {
    const at = start + index * stride;
    return {type: view.getUint32(at + 4, true), offset: view.getUint32(at + 16, true), size: view.getUint32(at + 20, true), link: view.getUint32(at + 24, true), info: view.getUint32(at + 28, true), stride: view.getUint32(at + 36, true)};
  });
  const table = sections.find(section => section.type === 2); assert.ok(table); const strings = sections[table.link], symbols = new Map();
  for (let at = table.offset; at < table.offset + table.size; at += table.stride) {
    const nameStart = strings.offset + view.getUint32(at, true), name = object.subarray(nameStart, object.indexOf(0, nameStart)).toString();
    const section = view.getUint16(at + 14, true), offset = view.getUint32(at + 4, true), size = view.getUint32(at + 8, true);
    if (section > 0 && section < sections.length && size > 0) {
      const source = sections[section]; assert.ok(offset + size <= source.size);
      assert.ok(!sections.some(item => [4, 9].includes(item.type) && item.info === section && item.size > 0));
      symbols.set(name, {offset, bytes: object.subarray(source.offset + offset, source.offset + offset + size)});
    }
  }
  return symbols;
}
const assemblyPath = join(root, 'engine/tests/fixtures/p2-byte-arithmetic/integer.S'), objectPath = join(output, 'integer.o');
const assemble = ['-target', 'i386-unknown-linux-gnu', '-c', assemblyPath, '-o', objectPath];
execFileSync('clang', assemble, {stdio: ['ignore', 'pipe', 'pipe']});
const disassembly = execFileSync('xcrun', ['llvm-objdump', '-d', '--x86-asm-syntax=intel', objectPath], {encoding: 'utf8'});
writeFileSync(join(output, 'integer.disassembly.txt'), disassembly);
const object = readFileSync(objectPath), symbols = elfFixtures(object), llvm = [];
const encodingSamples = [['add_rm_reg', '00c8'], ['add_reg_rm', '02c4'], ['add_al_imm', '0480'], ['add_modrm_imm', '80c7ff'], ['sub_rm_reg', '28ec'], ['sub_reg_rm', '2ae0'], ['sub_al_imm', '2c80'], ['sub_modrm_imm', '80efff']];
let llvmOffset = 0;
for (const [name, hex] of encodingSamples) {
  const symbol = symbols.get(name); assert.ok(symbol, name); assert.equal(symbol.offset, llvmOffset); assert.equal(symbol.bytes.toString('hex'), hex);
  llvm.push({name, offset: llvmOffset, hex}); llvmOffset += hex.length / 2;
}
assert.equal(symbols.size, 8); assert.equal(llvmOffset, 18);
for (const [name, instruction] of [['add_reg_rm', /\badd\s+al,\s*ah\b/], ['sub_reg_rm', /\bsub\s+ah,\s*al\b/]]) {
  const start = disassembly.indexOf(`<${name}>:`); assert.ok(start >= 0);
  const next = disassembly.indexOf('\n\n', start), body = disassembly.slice(start, next < 0 ? undefined : next);
  assert.match(body, instruction, `${name}: LLVM independently decodes the literal alternate direction`);
}

const image = readFileSync(join(output, 'arithmetic.exe'));
const program = 'b8ff7f3412b90101452300c80f92c3733f00ec0f90c7713828c80f92c2733128ec0f90c6712a38d30f94c184f70f95c5741ebe0800c0c889350030400056ff1550314000c70504304000a5a5a5a50f0bbedec0adde89350030400056ff15503140000f0b';
const callerOffsets = [0, 17, 24, 31, 38, 50, 80], relocationOffsets = [57, 64, 70, 87, 94];
const branchEdges = [[15, 0x73, 80, 17], [22, 0x71, 80, 24], [29, 0x73, 80, 31], [36, 0x71, 80, 38], [48, 0x74, 80, 50]];
assert.equal(image.length, 2560); assert.equal(program.length / 2, 100); assert.equal(image.subarray(0, 2).toString(), 'MZ');
assert.equal(image.subarray(0x200, 0x200 + 100).toString('hex'), program);
for (const [offset, opcode, target, fallthrough] of branchEdges) {
  assert.equal(image[0x200 + offset], opcode); assert.equal(offset + 2, fallthrough); assert.equal(offset + 2 + image.readInt8(0x201 + offset), target);
  assert.ok(callerOffsets.includes(target) && callerOffsets.includes(fallthrough));
}
assert.equal(image.readUInt32LE(0x800), 0x1000); assert.equal(image.readUInt32LE(0x804), 20);
assert.deepEqual(Array.from({length: 6}, (_, index) => image.readUInt16LE(0x808 + index * 2)), [...relocationOffsets.map(offset => 0x3000 + offset), 0]);
for (const base of [0x400000, 0x500000]) for (const owner of ['replacement', 'resident']) {
  const label = `pe-${owner}-${base.toString(16)}`, ctx = fresh(owner), entry = base + 0x1000;
  assert.equal(ctx.api.begin_image_input(image.length), 0); request(ctx, image); assert.equal(ctx.api.append_image_input(0, image.length), 0);
  const beforeLoad = arena(ctx); assert.equal(ctx.api.load_pe32_linked_v2_input_at(base, 0x8000), 0);
  const receipt = record('R3LI', 72, [base, 0x6000, entry, 5, 0x8000, 1, 0, 0, EXIT, EXIT_ID, 0, 0, 0, 0], 2), expectedLoad = beforeLoad.slice();
  expectedLoad.set(receipt, TRANSFER); assert.deepEqual(arena(ctx), expectedLoad, `${label}: exact independently reconstructed linked-v2 receipt`);
  assert.equal(read(ctx, base + 0x3150), EXIT, 'Rust resolves named ExitProcess IAT');
  for (const offset of relocationOffsets) assert.equal(read(ctx, entry + offset), base + ([64, 94].includes(offset) ? 0x3150 : offset === 70 ? 0x3004 : 0x3000));
  const dataBefore = readWords(ctx, base + 0x3000, 8); assert.deepEqual(dataBefore, Array(8).fill(0));
  const callerEntries = callerOffsets.map(offset => entry + offset), gates = [[EXIT, EXIT_ID]];
  if (owner === 'replacement') callerEntries.push(EXIT);
  const caller = compile(ctx, callerEntries, owner === 'replacement' ? gates : [], `${label}-caller`, ['read32', owner === 'replacement' ? 'store32' : 'store_resident32']);
  const gate = owner === 'replacement' ? caller : compile(ctx, [EXIT], gates, `${label}-gate`);
  assert.equal(ctx.api.start_loaded_image(0x70000, 1), 0);
  assert.deepEqual(arena(ctx).subarray(0, 56), stateRecord([0, 0, 0, 0, 0x71000, 0, 0, 0], entry, 2));
  assert.deepEqual(readWords(ctx, 0x70fe0, 8), Array(8).fill(0));
  const beforeRun = arena(ctx), expectedRun = beforeRun.slice(), finalRegisters = [0x12347fff, 0x23450101, 0x101, 0x101, 0x70ff8, 0, MARKER, 0];
  expectedRun.set(stateRecord(finalRegisters, EXIT, 2)); expectedRun.set(exitRecord(owner === 'resident' ? 3 : 8, 23, owner === 'resident' ? 2 : 3, owner === 'resident' ? 0 : EXIT_ID), 56);
  expectedRun.set(record('R3MH', 40, [0, 0, 0, 0, 0, 0]), 100);
  assert.equal(caller.run(ctx.base, ctx.base + 56, 64, ctx.base + 96), 0); assert.deepEqual(arena(ctx), expectedRun, `${label}:23 genuine guest instructions and full arena`);
  if (owner === 'resident') {
    const expectedGate = arena(ctx); expectedGate.set(exitRecord(8, 0, 3, EXIT_ID), 56);
    assert.equal(gate.run(ctx.base, ctx.base + 56, 1, ctx.base + 96), 0); assert.deepEqual(arena(ctx), expectedGate, 'separate resident named gate leaves registers and helper unchanged');
  }
  const beforeCapture = arena(ctx), expectedCapture = beforeCapture.slice();
  const frame = record('R3CF', 112, [1, EXIT_ID, 2, 1, EXIT, 0x70ff8, entry + 68, 0, MARKER, ...Array(15).fill(0)]);
  expectedCapture.set(frame, TRANSFER);
  assert.equal(owner === 'replacement' ? ctx.api.capture_call(ctx.low, ctx.high, gate.generation, 2, 1) : ctx.api.capture_resident_call(ctx.low, ctx.high, gate.low, gate.high, 2, 1), 0);
  assert.deepEqual(arena(ctx), expectedCapture, 'captured real CALL frame only');
  const expectedComplete = arena(ctx); expectedComplete.set(exitRecord(9, 0, 4, MARKER), 56);
  assert.equal(owner === 'replacement' ? ctx.api.complete_windows_call(ctx.low, ctx.high, gate.generation, 1) : ctx.api.complete_resident_windows_call(ctx.low, ctx.high, gate.low, gate.high, 1), 0);
  assert.deepEqual(arena(ctx), expectedComplete, 'named ExitProcess completion preserves genuine byte arithmetic and carry/overflow consumers');
  assert.deepEqual(readWords(ctx, base + 0x3000, 8), [MARKER, 0, 0, 0, 0, 0, 0, 0], 'declared32 data bytes and after-call canary');
  assert.deepEqual(readWords(ctx, 0x70fe0, 8), [0, 0, 0, 0, 0, 0, entry + 68, MARKER], 'declared32 stack bytes, real PUSH argument and CALL return address');
  pure(ctx, () => caller.run(0xffffffff, 0xffffffff, 0, 0xffffffff), 21, `${label}: terminal latch before pointers and budget`);
  peRows.push({owner, base, guest_retired: 23, result: MARKER, final_registers: finalRegisters, final_flags: 2, canary: 0, receipt_hex: Buffer.from(receipt).toString('hex'), receipt_sha256: hash(receipt)});
  assert.equal(ctx.api.close(), 0);
}
const counts = {canonical_forms_per_owner: forms.length, canonical_cases: rows.filter(row => !row.boundary).length, boundary_pairs_per_operation_owner: 49, boundary_cases: rows.filter(row => row.boundary).length, finite_guest_retired: rows.length, batches_per_owner: batches.length, modules: modules.length, controls: controls.length, pe_paths: peRows.length, pe_guest_retired: peRows.reduce((sum, row) => sum + row.guest_retired, 0), llvm_encoding_symbols: llvm.length};
assert.deepEqual(counts, {canonical_forms_per_owner: 328, canonical_cases: 984, boundary_pairs_per_operation_owner: 49, boundary_cases: 294, finite_guest_retired: 1278, batches_per_owner: 6, modules: 24, controls: 52, pe_paths: 4, pe_guest_retired: 92, llvm_encoding_symbols: 8});
for (const owner of ['standalone', 'replacement', 'resident']) {
  const canonical = rows.filter(row => row.owner === owner && !row.boundary);
  for (const flags of [2, 0xcd7]) assert.equal(canonical.filter(row => row.initial_flags === flags).length, 164);
  for (const kind of ['add', 'sub']) assert.equal(rows.filter(row => row.owner === owner && row.kind === kind && row.boundary).length, 49);
}
assert.deepEqual(Object.fromEntries(sourcePaths.map(path => [path, hash(readFileSync(join(root, path)))])), sourceHashes, 'source freeze unchanged during actual execution');
assert.equal(hash(readFileSync(enginePath)), hash(engineBytes), 'engine freeze unchanged');
const result = {status: 'ok', counts, engine_sha256: hash(engineBytes), engine_exports: WebAssembly.Module.exports(engineModule), pe_sha256: hash(image), source_sha256: sourceHashes, tools: {node: process.version, v8: process.versions.v8, clang: execFileSync('clang', ['--version'], {encoding: 'utf8'}).split('\n')[0], llvm: execFileSync('xcrun', ['llvm-objdump', '--version'], {encoding: 'utf8'}).split('\n')[0]}, assembly: {command: ['clang', ...assemble], object_sha256: hash(object), disassembly_sha256: hash(Buffer.from(disassembly)), symbols: llvm, scope: 'encoding only; alternate register directions are authored literal bytes and disassembled'}, oracle: {program, caller_offsets: callerOffsets, branches: branchEdges, relocation_offsets: relocationOffsets, canonical_flag_seeds: [2, 0xcd7], boundaries, boundary_flag_seed: 0xcd7, flag_policy: 'defined CF from widened carry/borrow; AF from nibble carry/borrow; OF from signed range; DF preserved; fixed EFLAGS bit1 value2; ZF/SF/PF from unsigned8 result', pe_final_registers: [0x12347fff, 0x23450101, 0x101, 0x101, 0x70ff8, 0, MARKER, 0], pe_final_flags: 2}, cases: rows, controls, pe: peRows, modules, claim: 'finite authored register/immediate ADD8/SUB8 in native-generated standalone Wasm and engine-Wasm-generated replacement/resident modules, with four copied PE startup/arithmetic/CMP8/TEST8/SETcc/branch/named ExitProcess paths; bound RAM observation is32 declared probe bytes per batch and32 data/32 stack bytes per PE path; LLVM proves encoding only; no memory arithmetic8 ISA,82alias,browser,SDK,performance or playable game claim'};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2));
console.log(JSON.stringify({status: result.status, engine_sha256: result.engine_sha256, counts, output}));
