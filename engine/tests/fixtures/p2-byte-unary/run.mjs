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
assert.equal(WebAssembly.Module.exports(engineModule).length, 70);
assert.equal(WebAssembly.Module.exports(engineModule).filter(row => row.kind === 'function').length, 69);
const SIZE = 4236, TRANSFER = 140, EXIT = 0x8020, EXIT_ID = 0x10003, MARKER = 0xc8c0000a;
const initialRegisters = [0x1234807f, 0x23457f80, 0x34560f10, 0x4567ff00, 0x56789abc, 0x6789abcd, 0x789abcde, 0x89abcdef];
const boundaries = [0, 1, 15, 16, 127, 128, 255];
const apiNames = ['open', 'close', 'arena_ptr', 'map', 'upload', 'read32', 'write8', 'compile_entries', 'compile_resident_entries', 'generation', 'module_ptr', 'module_len', 'guard', 'guard_resident', 'store32', 'store_resident32', 'begin_image_input', 'append_image_input', 'load_pe32_linked_v2_input_at', 'start_loaded_image', 'capture_call', 'capture_resident_call', 'complete_windows_call', 'complete_resident_windows_call'];
const sourcePaths = ['Cargo.toml', 'Cargo.lock', 'engine/Cargo.toml', 'engine/src/cpu/x86/ir.rs', 'engine/src/cpu/x86/decode/decoder.rs', 'engine/src/cpu/x86/decode/integer.rs', 'engine/src/cpu/x86/decode/operands.rs', 'engine/src/cpu/x86/decode/flow.rs', 'engine/src/cpu/dbt/region.rs', 'engine/src/cpu/dbt/wasm/integer.rs', 'engine/src/cpu/dbt/wasm/control.rs', 'engine/src/cpu/dbt/wasm/memory.rs', 'engine/src/process/instance.rs', 'engine/src/process/wasm.rs', 'engine/src/process/call.rs', 'engine/src/process/windows.rs', 'engine/src/abi/wasm/exports.rs', 'engine/src/loader/mod.rs', 'engine/src/loader/imports.rs', 'engine/src/loader/relocation.rs', 'engine/tests/support/pe32.rs', 'engine/tests/cpu_byte_unary_wasm.rs', 'engine/tests/fixtures/p2-byte-unary/run.mjs', 'engine/tests/fixtures/p2-byte-unary/integer.S'];
const sourceHashes = Object.fromEntries(sourcePaths.map(path => [path, hash(readFileSync(join(root, path)))]));
writeFileSync(join(output, 'source-hashes.json'), JSON.stringify({engine_sha256: hash(engineBytes), sources: sourceHashes}, null, 2));
const rows = [], controls = [], chainRows = [], peRows = [], modules = [];
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
  const instance = new WebAssembly.Instance(engineModule, {}), key = 0xc836500000000000n + BigInt(++ordinal);
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
function unaryArithmetic(kind, operand, oldFlags) {
  if (kind === 'not') return {result: 255 - operand, flags: oldFlags};
  const signed = operand >= 128 ? operand - 256 : operand;
  const wide = kind === 'inc' ? operand + 1 : kind === 'dec' ? operand - 1 : -operand;
  const result = (wide % 256 + 256) % 256;
  const carry = kind === 'neg' ? operand !== 0 : oldFlags % 2 !== 0;
  const auxiliary = kind === 'inc' ? operand % 16 === 15 : kind === 'dec' ? operand % 16 === 0 : operand % 16 !== 0;
  const signedResult = kind === 'inc' ? signed + 1 : kind === 'dec' ? signed - 1 : -signed;
  const overflow = signedResult < -128 || signedResult > 127;
  let ones = 0; for (let value = result; value > 0; value = Math.floor(value / 2)) ones += value % 2;
  const flags = (oldFlags & 0x400) | 2 | Number(carry) | (ones % 2 === 0 ? 4 : 0)
    | (auxiliary ? 0x10 : 0) | (result === 0 ? 0x40 : 0) | (result >= 128 ? 0x80 : 0) | (overflow ? 0x800 : 0);
  return {result, flags};
}
for (const [kind, operand, seedFlags, result, flags] of [
  ['inc', 255, 2, 0, 0x56], ['inc', 255, 3, 0, 0x57], ['inc', 127, 2, 128, 0x892],
  ['dec', 128, 2, 127, 0x812], ['dec', 0, 2, 255, 0x96], ['neg', 0, 3, 0, 0x46],
  ['neg', 128, 2, 128, 0x883], ['neg', 1, 2, 255, 0x97],
  ['not', 255, 0xcd7, 0, 0xcd7], ['not', 0, 2, 255, 2],
]) assert.deepEqual(unaryArithmetic(kind, operand, seedFlags), {result, flags});
function runUnary(ctx, unit, form, registers, flags, name) {
  seed(ctx, registers, form.pc, flags);
  const before = arena(ctx), expected = before.slice(), nextRegisters = [...registers];
  const operand = byte(registers, form.destination), outcome = unaryArithmetic(form.kind, operand, flags);
  replaceByte(nextRegisters, form.destination, outcome.result);
  expected.set(stateRecord(nextRegisters, form.pc + form.bytes.length, outcome.flags)); expected.set(exitRecord(1, 1), 56);
  const wholeMemory = ctx.owner === 'standalone' ? ctx.bytes.slice() : undefined;
  assert.equal(unit.run(ctx.base, ctx.base + 56, 1, ctx.base + 96), 0, name);
  assert.deepEqual(arena(ctx), expected, `${name}: full registers, flags, EIP, exit, helper and transfer`);
  if (wholeMemory) { wholeMemory.set(expected, ctx.base); assert.deepEqual(ctx.bytes, wholeMemory, `${name}: full standalone memory`); }
  rows.push({owner: ctx.owner, name, kind: form.kind, destination: form.destination, encoding: Buffer.from(form.bytes).toString('hex'), operand, carry_in: flags % 2, result: outcome.result, initial_flags: flags, flags: outcome.flags});
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
for (const [kind, opcode, extension] of [['inc', 0xfe, 0], ['dec', 0xfe, 1], ['neg', 0xf6, 3], ['not', 0xf6, 2]]) {
  for (let destination = 0; destination < 8; destination++) forms.push({kind, destination, bytes: [opcode, 0xc0 + extension * 8 + destination]});
}
assert.equal(forms.length, 32); forms.forEach((form, index) => { form.ordinal = index; form.pc = 0x1000 + index * 2; });
const code = Buffer.from([...forms.flatMap(form => form.bytes), 0xeb, 0, 0x0f, 0x0b]);
assert.equal(forms.length + 1, 33); assert.deepEqual(code, readFileSync(join(output, 'batch-0.x86')), 'independent Rust/JS encoding agrees');
const batches = [{entry: 0x1000, forms, code}], flagSeeds = [2, 3, 0xcd6, 0xcd7];
const chainCode = Buffer.from('b0ffb10100c8fec4feccf6d510e8f6d818eceb000f0b', 'hex');
assert.deepEqual(readFileSync(join(output, 'chain.x86')), chainCode);
function runChain(ctx, unit) {
  const registers = [...initialRegisters]; registers[0] = 0x12347f00; registers[1] = 0x23450000;
  seed(ctx, registers, 0x2000, 2);
  const probe = ctx.owner === 'standalone' ? undefined : readWords(ctx, 0x9000, 8);
  const slices = [];
  function slice(name, budget, retired, offset, eax, ecx, flags, cancel = 0) {
    // only the cancellation word changes after the initial CPU seed.
    refresh(ctx).view.setUint32(ctx.base + 96, cancel, true);
    const before = arena(ctx), expected = before.slice(), next = [...registers]; next[0] = eax; next[1] = ecx;
    expected.set(stateRecord(next, 0x2000 + offset, flags)); expected.set(exitRecord(cancel ? 2 : 1, retired), 56);
    const wholeMemory = ctx.owner === 'standalone' ? ctx.bytes.slice() : undefined;
    assert.equal(unit.run(ctx.base, ctx.base + 56, budget, ctx.base + 96), 0, `${ctx.owner}: live ${name}`);
    assert.deepEqual(arena(ctx), expected, `${ctx.owner}: live unary carry chain ${name}`);
    if (wholeMemory) { wholeMemory.set(expected, ctx.base); assert.deepEqual(ctx.bytes, wholeMemory); }
    slices.push({name, budget, retired, pc: 0x2000 + offset, eax, ecx, flags, cancel});
    if (retired === 0) controls.push({owner: ctx.owner, name: `chain_${name}`, retired: 0});
  }
  slice('add_produces_carry', 3, 3, 6, 0x12347f00, 0x23450001, 0x57);
  slice('producer_zero_budget', 0, 0, 6, 0x12347f00, 0x23450001, 0x57);
  slice('producer_cancel_positive_budget', 1, 0, 6, 0x12347f00, 0x23450001, 0x57, 1);
  slice('inc_preserves_carry', 1, 1, 8, 0x12348000, 0x23450001, 0x893);
  slice('dec_preserves_carry', 1, 1, 10, 0x12347f00, 0x23450001, 0x813);
  slice('not_preserves_all_flags', 1, 1, 12, 0x12347f00, 0x2345ff01, 0x813);
  slice('preserved_zero_budget', 0, 0, 12, 0x12347f00, 0x2345ff01, 0x813);
  slice('preserved_cancel_positive_budget', 1, 0, 12, 0x12347f00, 0x2345ff01, 0x813, 1);
  slice('adc_consumes_preserved_carry', 1, 1, 14, 0x12347f00, 0x2345ff01, 0x57);
  slice('neg_clears_carry', 1, 1, 16, 0x12347f00, 0x2345ff01, 0x46);
  slice('reset_zero_budget', 0, 0, 16, 0x12347f00, 0x2345ff01, 0x46);
  slice('reset_cancel_positive_budget', 1, 0, 16, 0x12347f00, 0x2345ff01, 0x46, 1);
  slice('sbb_consumes_reset_carry', 1, 1, 18, 0x12348000, 0x2345ff01, 0x883);
  slice('jmp_finishes', 1, 1, 20, 0x12348000, 0x2345ff01, 0x883);
  if (probe) assert.deepEqual(readWords(ctx, 0x9000, 8), probe, 'declared32 guest RAM bytes unchanged through live unary carry chain');
  assert.equal(slices.reduce((sum, row) => sum + row.retired, 0), 10);
  chainRows.push({owner: ctx.owner, guest_retired: 10, initial_eax: 0x12347f00, initial_ecx: 0x23450000, initial_flags: 2, slices});
}
for (const owner of ['standalone', 'replacement', 'resident']) {
  const ctx = owner === 'standalone' ? refresh({owner, memory: new WebAssembly.Memory({initial: 1}), base: 128}) : fresh(owner);
  if (owner !== 'standalone') {
    assert.equal(ctx.api.map(0x1000, 2, 7), 0); assert.equal(ctx.api.map(0x9000, 1, 3), 0);
    upload(ctx, 0x9000, Buffer.from('00112233445566778899aabbccddeeff0123456789abcdefa5a5a5a55a5a5a5a', 'hex'));
    // no shared code-page writes occur after the first bound compilation.
    for (const batch of batches) upload(ctx, batch.entry, batch.code);
    upload(ctx, 0x2000, chainCode);
  }
  let lastUnit;
  for (const [index, batch] of batches.entries()) {
    const label = `${owner}-batch${index}`;
    const unit = owner === 'standalone' ? instantiate(ctx, readFileSync(join(output, `batch-${index}.wasm`)), label) : compile(ctx, [batch.entry], [], label);
    assert.deepEqual(unit.helpers, owner === 'standalone' ? [] : [owner === 'replacement' ? 'guard' : 'guard_resident'], 'unary bytes add no RAM helper');
    const probeBefore = owner === 'standalone' ? undefined : readWords(ctx, 0x9000, 8);
    for (const form of batch.forms) for (const operand of boundaries) for (const flags of flagSeeds) {
      const registers = [...initialRegisters]; replaceByte(registers, form.destination, operand);
      runUnary(ctx, unit, form, registers, flags, `${label}-${form.kind}-${form.destination}-value${operand}-flags${flags}`);
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
  }
  const chain = owner === 'standalone' ? instantiate(ctx, readFileSync(join(output, 'chain.wasm')), `${owner}-chain`) : compile(ctx, [0x2000], [], `${owner}-chain`);
  assert.deepEqual(chain.helpers, owner === 'standalone' ? [] : [owner === 'replacement' ? 'guard' : 'guard_resident']);
  runChain(ctx, chain);
  if (owner !== 'standalone') {
    assert.equal(ctx.api.write8(0x2000, chainCode[0]), 0);
    pure(ctx, () => chain.run(0xffffffff, 0xffffffff, 0, 0xffffffff), 4, `${owner}: same-byte code stale before pointers`);
    assert.equal(ctx.api.close(), 0);
    pure(ctx, () => chain.run(0xffffffff, 0xffffffff, 0, 0xffffffff), 5, `${owner}: closed before pointers`);
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
const assemblyPath = join(root, 'engine/tests/fixtures/p2-byte-unary/integer.S'), objectPath = join(output, 'integer.o');
const assemble = ['-target', 'i386-unknown-linux-gnu', '-c', assemblyPath, '-o', objectPath];
execFileSync('clang', assemble, {stdio: ['ignore', 'pipe', 'pipe']});
const disassembly = execFileSync('xcrun', ['llvm-objdump', '-d', '--x86-asm-syntax=intel', objectPath], {encoding: 'utf8'});
writeFileSync(join(output, 'integer.disassembly.txt'), disassembly);
const object = readFileSync(objectPath), symbols = elfFixtures(object), llvm = [];
const encodingSamples = [['inc_al', 'fec0'], ['dec_ah', 'fecc'], ['neg_cl', 'f6d9'], ['not_bh', 'f6d7']];
let llvmOffset = 0;
for (const [name, hex] of encodingSamples) {
  const symbol = symbols.get(name); assert.ok(symbol, name); assert.equal(symbol.offset, llvmOffset); assert.equal(symbol.bytes.toString('hex'), hex);
  llvm.push({name, offset: llvmOffset, hex}); llvmOffset += hex.length / 2;
}
assert.equal(symbols.size, 4); assert.equal(llvmOffset, 8);
for (const [name, instruction] of [['inc_al', /\binc\s+al\b/], ['dec_ah', /\bdec\s+ah\b/], ['neg_cl', /\bneg\s+cl\b/], ['not_bh', /\bnot\s+bh\b/]]) {
  const start = disassembly.indexOf(`<${name}>:`); assert.ok(start >= 0);
  const next = disassembly.indexOf('\n\n', start), body = disassembly.slice(start, next < 0 ? undefined : next);
  assert.match(body, instruction, `${name}: LLVM independently decodes the authored canonical encoding`);
}

const image = readFileSync(join(output, 'unary.exe'));
const program = 'b8ff7f3412b90100452300c8fec4feccf6d50f92c3733a10e80f92c77333f6d80f93c2722c18ec0f90c671253d00803412751ebe0a00c0c889350030400056ff1550314000c70504304000a5a5a5a50f0bbedec0adde89350030400056ff15503140000f0b';
const callerOffsets = [0, 23, 30, 37, 44, 51, 81], relocationOffsets = [58, 65, 71, 88, 95];
const branchEdges = [[21, 0x73, 81, 23], [28, 0x73, 81, 30], [35, 0x72, 81, 37], [42, 0x71, 81, 44], [49, 0x75, 81, 51]];
assert.equal(image.length, 2560); assert.equal(program.length / 2, 101); assert.equal(image.subarray(0, 2).toString(), 'MZ');
assert.equal(image.subarray(0x200, 0x200 + 101).toString('hex'), program);
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
  for (const offset of relocationOffsets) assert.equal(read(ctx, entry + offset), base + ([65, 95].includes(offset) ? 0x3150 : offset === 71 ? 0x3004 : 0x3000));
  const dataBefore = readWords(ctx, base + 0x3000, 8); assert.deepEqual(dataBefore, Array(8).fill(0));
  const callerEntries = callerOffsets.map(offset => entry + offset), gates = [[EXIT, EXIT_ID]];
  if (owner === 'replacement') callerEntries.push(EXIT);
  const caller = compile(ctx, callerEntries, owner === 'replacement' ? gates : [], `${label}-caller`, ['read32', owner === 'replacement' ? 'store32' : 'store_resident32']);
  const gate = owner === 'replacement' ? caller : compile(ctx, [EXIT], gates, `${label}-gate`);
  assert.equal(ctx.api.start_loaded_image(0x70000, 1), 0);
  assert.deepEqual(arena(ctx).subarray(0, 56), stateRecord([0, 0, 0, 0, 0x71000, 0, 0, 0], entry, 2));
  assert.deepEqual(readWords(ctx, 0x70fe0, 8), Array(8).fill(0));
  const beforeRun = arena(ctx), expectedRun = beforeRun.slice(), finalRegisters = [0x12348000, 0x2345ff01, 0x101, 0x101, 0x70ff8, 0, MARKER, 0];
  expectedRun.set(stateRecord(finalRegisters, EXIT, 0x46)); expectedRun.set(exitRecord(owner === 'resident' ? 3 : 8, 23, owner === 'resident' ? 2 : 3, owner === 'resident' ? 0 : EXIT_ID), 56);
  expectedRun.set(record('R3MH', 40, [0, 0, 0, 0, 0, 0]), 100);
  assert.equal(caller.run(ctx.base, ctx.base + 56, 64, ctx.base + 96), 0); assert.deepEqual(arena(ctx), expectedRun, `${label}:23 genuine guest instructions and full arena`);
  if (owner === 'resident') {
    const expectedGate = arena(ctx); expectedGate.set(exitRecord(8, 0, 3, EXIT_ID), 56);
    assert.equal(gate.run(ctx.base, ctx.base + 56, 1, ctx.base + 96), 0); assert.deepEqual(arena(ctx), expectedGate, 'separate resident named gate leaves registers and helper unchanged');
  }
  const beforeCapture = arena(ctx), expectedCapture = beforeCapture.slice();
  const frame = record('R3CF', 112, [1, EXIT_ID, 2, 1, EXIT, 0x70ff8, entry + 69, 0, MARKER, ...Array(15).fill(0)]);
  expectedCapture.set(frame, TRANSFER);
  assert.equal(owner === 'replacement' ? ctx.api.capture_call(ctx.low, ctx.high, gate.generation, 2, 1) : ctx.api.capture_resident_call(ctx.low, ctx.high, gate.low, gate.high, 2, 1), 0);
  assert.deepEqual(arena(ctx), expectedCapture, 'captured real CALL frame only');
  const expectedComplete = arena(ctx); expectedComplete.set(exitRecord(9, 0, 4, MARKER), 56);
  assert.equal(owner === 'replacement' ? ctx.api.complete_windows_call(ctx.low, ctx.high, gate.generation, 1) : ctx.api.complete_resident_windows_call(ctx.low, ctx.high, gate.low, gate.high, 1), 0);
  assert.deepEqual(arena(ctx), expectedComplete, 'named ExitProcess completion preserves genuine unary carry consumers');
  assert.deepEqual(readWords(ctx, base + 0x3000, 8), [MARKER, 0, 0, 0, 0, 0, 0, 0], 'declared32 data bytes and after-call canary');
  assert.deepEqual(readWords(ctx, 0x70fe0, 8), [0, 0, 0, 0, 0, 0, entry + 69, MARKER], 'declared32 stack bytes, real PUSH argument and CALL return address');
  pure(ctx, () => caller.run(0xffffffff, 0xffffffff, 0, 0xffffffff), 21, `${label}: terminal latch before pointers and budget`);
  peRows.push({owner, base, guest_retired: 23, result: MARKER, final_registers: finalRegisters, final_flags: 0x46, canary: 0, receipt_hex: Buffer.from(receipt).toString('hex'), receipt_sha256: hash(receipt)});
  assert.equal(ctx.api.close(), 0);
}
const counts = {canonical_forms_per_owner: forms.length, values_per_form: boundaries.length, flag_seeds_per_value: flagSeeds.length, finite_guest_retired: rows.length, batches_per_owner: batches.length, modules: modules.length, controls: controls.length, chain_paths: chainRows.length, chain_runs: chainRows.reduce((sum, row) => sum + row.slices.length, 0), chain_guest_retired: chainRows.reduce((sum, row) => sum + row.guest_retired, 0), pe_paths: peRows.length, pe_guest_retired: peRows.reduce((sum, row) => sum + row.guest_retired, 0), llvm_encoding_symbols: llvm.length};
assert.deepEqual(counts, {canonical_forms_per_owner: 32, values_per_form: 7, flag_seeds_per_value: 4, finite_guest_retired: 2688, batches_per_owner: 1, modules: 12, controls: 40, chain_paths: 3, chain_runs: 42, chain_guest_retired: 30, pe_paths: 4, pe_guest_retired: 92, llvm_encoding_symbols: 4});
for (const owner of ['standalone', 'replacement', 'resident']) {
  const owned = rows.filter(row => row.owner === owner); assert.equal(owned.length, 896);
  for (const flags of flagSeeds) assert.equal(owned.filter(row => row.initial_flags === flags).length, 224);
  for (const kind of ['inc', 'dec', 'neg', 'not']) for (let destination = 0; destination < 8; destination++) assert.equal(owned.filter(row => row.kind === kind && row.destination === destination).length, 28);
}
assert.deepEqual(Object.fromEntries(sourcePaths.map(path => [path, hash(readFileSync(join(root, path)))])), sourceHashes, 'source freeze unchanged during actual execution');
assert.equal(hash(readFileSync(enginePath)), hash(engineBytes), 'engine freeze unchanged');
const result = {status: 'ok', counts, engine_sha256: hash(engineBytes), engine_exports: WebAssembly.Module.exports(engineModule), pe_sha256: hash(image), source_sha256: sourceHashes, tools: {node: process.version, v8: process.versions.v8, clang: execFileSync('clang', ['--version'], {encoding: 'utf8'}).split('\n')[0], llvm: execFileSync('xcrun', ['llvm-objdump', '--version'], {encoding: 'utf8'}).split('\n')[0]}, assembly: {command: ['clang', ...assemble], object_sha256: hash(object), disassembly_sha256: hash(Buffer.from(disassembly)), symbols: llvm, scope: 'encoding only; four authored canonical unary families'}, oracle: {program, caller_offsets: callerOffsets, branches: branchEdges, relocation_offsets: relocationOffsets, flag_seeds: flagSeeds, boundaries, chain_hex: chainCode.toString('hex'), chain_pc: 0x2000, chain_initial_eax: 0x12347f00, chain_initial_ecx: 0x23450000, chain_flags: [0x57, 0x893, 0x813, 0x813, 0x57, 0x46, 0x883], flag_policy: 'INC/DEC retain incoming CF, other arithmetic flags use numeric result/nibble and signed range; NEG defines fresh CF from nonzero operand; arithmetic DF preserved and fixed EFLAGS bit1 value2; NOT preserves all incoming FLAGS exactly', pe_final_registers: [0x12348000, 0x2345ff01, 0x101, 0x101, 0x70ff8, 0, MARKER, 0], pe_final_flags: 0x46}, cases: rows, controls, chains: chainRows, pe: peRows, modules, claim: 'finite authored register-only INC8/DEC8/NEG8/NOT8 in native-generated standalone Wasm and engine-Wasm-generated replacement/resident modules, with three genuine ADD producer, unary preservation/reset and ADC/SBB consumer pause/cancel/resume chains without post-start host CPU/flags edits, plus four copied PE startup/unary/carry/SETcc/branch/CMP32/named ExitProcess paths; bound RAM observation is32 declared probe bytes per batch and32 data/32 stack bytes per PE path; NOT exact FLAGS preservation is finite four-seed coverage plus source no-write proof; LLVM proves encoding only; no memory unary8, exhaustive flag domain, browser,SDK,performance or playable game claim'};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2));
console.log(JSON.stringify({status: result.status, engine_sha256: result.engine_sha256, counts, output}));
