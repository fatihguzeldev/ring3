import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, readdirSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(root, 'expected engine wasm, output directory and repository root');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const engineBytes = readFileSync(enginePath), engineModule = new WebAssembly.Module(engineBytes);
assert.deepEqual(WebAssembly.Module.imports(engineModule), []);
assert.equal(WebAssembly.Module.exports(engineModule).length, 72);
assert.equal(WebAssembly.Module.exports(engineModule).filter(row => row.kind === 'function').length, 71);
const SIZE = 4236, TRANSFER = 140, PC = 0x1000;
const VECTORS = [
  [0x12345678, 0x23457f80, 0x34560f10, 0x4567ff00, 0x56789abc, 0x6789abcd, 0x789abcde, 0x89abcdef],
  [0x80ff0100, 0x7f00fe81, 0x010000ff, 0xff80007f, 0x00ff817e, 0xfedcba98, 0xa5c33c5a, 0x005580aa],
], FLAGS = [2, 0xcd7];
const MODRM = [
  [0xc0, 0xc8, 0xd0, 0xd8, 0xe0, 0xe8, 0xf0, 0xf8, 0xc1, 0xc9, 0xd1, 0xd9, 0xe1, 0xe9, 0xf1, 0xf9, 0xc2, 0xca, 0xd2, 0xda, 0xe2, 0xea, 0xf2, 0xfa, 0xc3, 0xcb, 0xd3, 0xdb],
  [0xe3, 0xeb, 0xf3, 0xfb, 0xc4, 0xcc, 0xd4, 0xdc, 0xe4, 0xec, 0xf4, 0xfc, 0xc5, 0xcd, 0xd5, 0xdd, 0xe5, 0xed, 0xf5, 0xfd, 0xc6, 0xce, 0xd6, 0xde, 0xe6, 0xee, 0xf6, 0xfe],
  [0xc7, 0xcf, 0xd7, 0xdf, 0xe7, 0xef, 0xf7, 0xff],
];
const BATCHES = MODRM.map((modrms, index) => {
  const forms = modrms.map((modrm, local_pair_index) => {
    const left = modrm & 7, right = (modrm >>> 3) & 7;
    return {pair_index: [0, 28, 56][index] + local_pair_index, local_pair_index, encoding: 'modrm', left, right, opcode: 0x87, modrm, length: 2, offset: 4 * local_pair_index, alias: left === right};
  });
  if (index === 2) for (let left = 1; left < 8; left++) forms.push({pair_index: 63 + left, local_pair_index: 7 + left, encoding: 'accumulator-short', left, right: 0, opcode: 0x90 + left, modrm: null, length: 1, offset: 32 + 2 * (left - 1), alias: false});
  const program = Buffer.from([...forms.flatMap(form => form.modrm === null ? [form.opcode, form.opcode] : [form.opcode, form.modrm, form.opcode, form.modrm]), ...(index === 2 ? [0x90] : []), 0xeb, 0, 0x0f, 0x0b]);
  const batch = {index, forms, program, compile_bytes: [114, 114, 49][index], instructions: [57, 57, 32][index]};
  assert.equal(program.length, batch.compile_bytes + 2); assert.equal(forms.length, [28, 28, 15][index]); assert.ok(batch.instructions <= 64); return batch;
});
assert.equal(new Set(VECTORS.flat()).size, 16); assert.ok(VECTORS.every(vector => new Set(vector).size === 8));
function engineSources(directory) {return readdirSync(join(root, directory), {withFileTypes: true}).flatMap(row => row.isDirectory() ? engineSources(`${directory}/${row.name}`) : [`${directory}/${row.name}`]);}
const sourcePaths = ['Cargo.toml', 'Cargo.lock', 'engine/Cargo.toml', 'rust-toolchain.toml', ...engineSources('engine/src').sort(), 'engine/tests/cpu_xchg32_wasm.rs', 'engine/tests/fixtures/p2-xchg32/run.mjs'];
assert.equal(sourcePaths.length, 90); assert.equal(new Set(sourcePaths).size, 90);
const sourceHashes = Object.fromEntries(sourcePaths.map(path => [path, hash(readFileSync(join(root, path)))]));
const contexts = [], modules = [], generatedRuns = [], cases = [], nops = [], continuations = [], controls = [], arenas = [], pages = [], rawFiles = [];
function original(path, bytes) {const row = {path, length: bytes.length, sha256: hash(bytes)}; assert.ok(!rawFiles.some(old => old.path === path)); rawFiles.push(row); return row;}
function artifact(path, bytes) {writeFileSync(join(output, path), bytes, {flag: 'wx'}); return original(path, bytes);}
artifact('engine.wasm', engineBytes); artifact('source-hashes.json', Buffer.from(JSON.stringify({engine_sha256: hash(engineBytes), sources: sourceHashes}, null, 2)));
for (const batch of BATCHES) {const path = `xchg32-batch-${batch.index}.x86`, x86 = readFileSync(join(output, path)); assert.deepEqual(x86, batch.program, 'independent Rust/JS instruction bytes'); original(path, x86);}
function record(magic, size, fields) {const bytes = Buffer.alloc(size); bytes.write(magic); bytes.writeUInt16LE(1, 4); bytes.writeUInt16LE(1, 6); bytes.writeUInt32LE(size, 8); fields.forEach((value, index) => bytes.writeUInt32LE(value >>> 0, 16 + index * 4)); return bytes;}
const state = (registers, pc, flags) => record('R3ST', 56, [...registers, pc, flags]);
const exit = (reason, retired) => record('R3EX', 40, [reason, retired, 0, 0, 0, 0]);
const helper = value => record('R3MH', 40, [0, value, 0, 0, 0, 0]);
function hostByteHelper() {const bytes = record('R3MH', 40, [0, 0, 0, 0, 0, 1]); bytes.writeUInt16LE(3, 4); return bytes;}
const words = values => {const bytes = Buffer.alloc(values.length * 4); values.forEach((value, index) => bytes.writeUInt32LE(value >>> 0, index * 4)); return bytes;};
const pattern = () => Buffer.from(Array.from({length: 4096}, (_, index) => index % 251 + 1));
function refresh(ctx) {ctx.bytes = new Uint8Array(ctx.memory.buffer); ctx.view = new DataView(ctx.memory.buffer); return ctx;}
function arena(ctx) {return Buffer.from(refresh(ctx).bytes.subarray(ctx.base, ctx.base + SIZE));}
function checkpoint(ctx, before, name) {const actual = arena(ctx); assert.deepEqual(actual, ctx.expected, name); const label = `arena-${String(arenas.length).padStart(4, '0')}`; arenas.push({owner: ctx.owner, context: ctx.ordinal, batch_index: ctx.batch.index, name, before: artifact(`${label}-before.bin`, before), actual: artifact(`${label}-actual.bin`, actual), expected: artifact(`${label}-expected.bin`, ctx.expected)}); return arenas.length - 1;}
function pure(ctx, action, name) {assert.deepEqual(arena(ctx), ctx.expected); assert.equal(action(), 0, name); assert.deepEqual(arena(ctx), ctx.expected, name);}
function request(ctx, bytes) {assert.ok(bytes.length <= 4096); refresh(ctx).bytes.set(bytes, ctx.base + TRANSFER); ctx.expected.set(bytes, TRANSFER); assert.deepEqual(arena(ctx), ctx.expected);}
function fresh(owner, batch) {
  const ordinal = contexts.length + 1, expected = Buffer.alloc(SIZE); expected.set(state(Array(8).fill(0), 0, 2)); expected.set(exit(1, 0), 56); expected.set(helper(0), 100);
  const ctx = {owner, ordinal, batch, expected, low: ordinal, high: 0xe3880000};
  if (owner === 'standalone') {ctx.memory = new WebAssembly.Memory({initial: 1}); ctx.base = 128; refresh(ctx).bytes.set(expected, ctx.base); ctx.dataAddress = 0x8000; ctx.bytes.set(pattern(), ctx.dataAddress);}
  else {const instance = new WebAssembly.Instance(engineModule, {}), arities = {open: 3, close: 0, arena_ptr: 0, map: 3, upload: 2, read32: 1, write8: 2, compile_entries: 2, compile_resident: 1, generation: 0, module_ptr: 0, module_len: 0, guard: 6, guard_resident: 7}; ctx.api = Object.fromEntries(Object.entries(arities).map(([name, count]) => {const fn = instance.exports[`ring3_abi_v1_${name}`]; assert.equal(typeof fn, 'function', name); assert.equal(fn.length, count, name); return [name, fn];})); ctx.memory = instance.exports.memory; assert.equal(ctx.api.open(3, ctx.low, ctx.high), 0); ctx.base = ctx.api.arena_ptr() >>> 0; refresh(ctx); assert.deepEqual(arena(ctx), expected, 'independently initialized bound arena'); ctx.dataAddress = 0x3000; pure(ctx, () => ctx.api.map(ctx.dataAddress, 1, 3), 'data map'); request(ctx, pattern()); pure(ctx, () => ctx.api.upload(ctx.dataAddress, 4096), 'data upload');}
  contexts.push({owner, context: ordinal, batch_index: batch.index, base: ctx.base, key: owner === 'standalone' ? [] : [ctx.low, ctx.high], data_address: ctx.dataAddress}); return ctx;
}
function page(ctx, name) {
  const expected = pattern(), actual = Buffer.alloc(4096);
  if (ctx.owner === 'standalone') actual.set(refresh(ctx).bytes.subarray(ctx.dataAddress, ctx.dataAddress + 4096));
  else for (let offset = 0; offset < 4096; offset += 4) {assert.deepEqual(arena(ctx), ctx.expected); assert.equal(ctx.api.read32(ctx.dataAddress + offset), 0); refresh(ctx); actual.writeUInt32LE(ctx.view.getUint32(ctx.base + 120, true), offset); ctx.expected.set(helper(expected.readUInt32LE(offset)), 100); assert.deepEqual(arena(ctx), ctx.expected, 'diagnostic read changes helper40 only');}
  assert.deepEqual(actual, expected, 'complete patterned page unchanged'); const label = `page-${String(pages.length).padStart(2, '0')}`; pages.push({owner: ctx.owner, context: ctx.ordinal, batch_index: ctx.batch.index, name, address: ctx.dataAddress, space: ctx.owner === 'standalone' ? 'imported-linear-memory' : 'guest-address-space', permissions: ctx.owner === 'standalone' ? null : 3, actual: artifact(`${label}-actual.bin`, actual), expected: artifact(`${label}-expected.bin`, expected)});
}
function childShape(ctx, bytes, binding) {
  let at = 8; const unsigned = () => {let value = 0, shift = 0; for (let count = 0; count < 5; count++) {assert.ok(at < bytes.length); const next = bytes[at++]; value |= (next & 127) << shift; if (!(next & 128)) return value >>> 0; shift += 7;} assert.fail('bounded unsigned LEB');};
  const signed = () => {let value = 0n, shift = 0n, next; do {assert.ok(at < bytes.length && shift < 35n); next = bytes[at++]; value |= BigInt(next & 127) << shift; shift += 7n;} while (next & 128); if (next & 64) value -= 1n << shift; return Number(BigInt.asUintN(32, value));};
  const name = () => {const size = unsigned(), end = at + size; assert.ok(end <= bytes.length); const value = bytes.subarray(at, end).toString(); at = end; return value;};
  const types = [], imports = [], functions = [], exports = [], sections = []; let locals, guardPrefix = null;
  assert.deepEqual([...bytes.subarray(0, 8)], [0, 97, 115, 109, 1, 0, 0, 0]);
  while (at < bytes.length) {const section = bytes[at++], length = unsigned(), end = at + length; assert.ok(end <= bytes.length); sections.push(section);
    if (section === 1) for (let count = unsigned(); count > 0; count--) {assert.equal(bytes[at++], 0x60); types.push({parameters: Array.from({length: unsigned()}, () => bytes[at++]), results: Array.from({length: unsigned()}, () => bytes[at++])});}
    else if (section === 2) for (let count = unsigned(); count > 0; count--) {const module = name(), field = name(), kind = bytes[at++]; if (kind === 0) imports.push({module, name: field, kind: 'function', type: unsigned()}); else {assert.equal(kind, 2); assert.equal(unsigned(), 0); assert.equal(unsigned(), 1); imports.push({module, name: field, kind: 'memory'});}}
    else if (section === 3) for (let count = unsigned(); count > 0; count--) functions.push(unsigned());
    else if (section === 7) for (let count = unsigned(); count > 0; count--) exports.push({name: name(), kind: bytes[at++], index: unsigned()});
    else if (section === 10) {assert.equal(unsigned(), 1); const bodyLength = unsigned(); assert.equal(at + bodyLength, end); locals = Array.from({length: unsigned()}, () => [unsigned(), bytes[at++]]); assert.deepEqual(locals, [[16, 0x7f], [1, 0x7e]]); if (ctx.owner !== 'standalone') {const constants = [ctx.low, ctx.high, ...(ctx.owner === 'replacement' ? [binding.generation] : [binding.low, binding.high])]; for (const value of constants) {assert.equal(bytes[at++], 0x41); assert.equal(signed(), value);} for (const index of [0, 1, 3]) {assert.equal(bytes[at++], 0x20); assert.equal(unsigned(), index);} assert.equal(bytes[at++], 0x10); assert.equal(unsigned(), 0); assert.deepEqual([...bytes.subarray(at, at + 8)], [0x22, 16, 4, 0x40, 0x20, 16, 0x0f, 0x0b]); guardPrefix = {constants, parameters: [0, 1, 3]};} at = end;}
    else assert.fail(`unexpected child section${section}`); assert.equal(at, end);
  }
  const names = ctx.owner === 'standalone' ? [] : [ctx.owner === 'replacement' ? 'guard' : 'guard_resident'];
  assert.deepEqual(sections, [1, 2, 3, 7, 10]); assert.deepEqual(functions, [0]); assert.equal(types.length, names.length + 1); assert.deepEqual(types[0], {parameters: Array(4).fill(0x7f), results: [0x7f]}); assert.deepEqual(imports[0], {module: 'env', name: 'memory', kind: 'memory'}); assert.deepEqual(imports.slice(1), names.map(name => ({module: 'ring3', name, kind: 'function', type: 1}))); if (names.length) assert.deepEqual(types[1], {parameters: Array(ctx.owner === 'replacement' ? 6 : 7).fill(0x7f), results: [0x7f]}); assert.deepEqual(exports, [{name: 'run', kind: 0, index: names.length}]); return {types, imports, locals, guard: guardPrefix, run_signature: types[0]};
}
function compile(ctx) {
  const mode = ctx.owner === 'replacement' ? 'entry' : 'extent', entries = [PC], specs = [[PC, ctx.batch.compile_bytes]], label = `${ctx.owner}-batch-${ctx.batch.index}`; let binding = {}, bytes, saved, before;
  if (ctx.owner === 'standalone') {bytes = readFileSync(join(output, `${label}.wasm`)); saved = original(`${label}.wasm`, bytes); before = Buffer.from(ctx.expected);}
  else {pure(ctx, () => ctx.api.map(PC, 1, 7), 'code map'); request(ctx, ctx.batch.program); pure(ctx, () => ctx.api.upload(PC, ctx.batch.program.length), 'code upload'); request(ctx, words(mode === 'entry' ? entries : specs.flat())); before = Buffer.from(ctx.expected); assert.equal(ctx.owner === 'replacement' ? ctx.api.compile_entries(1, 0) : ctx.api.compile_resident(1), 0); refresh(ctx); if (ctx.owner === 'replacement') binding = {generation: ctx.api.generation(), pointer: ctx.api.module_ptr() >>> 0, length: ctx.api.module_len() >>> 0}; else {const at = ctx.base + TRANSFER; assert.equal(ctx.view.getUint32(at, true), 1); assert.equal(ctx.view.getUint32(at + 4, true), 24); binding = {low: ctx.view.getUint32(at + 8, true), high: ctx.view.getUint32(at + 12, true), pointer: ctx.view.getUint32(at + 16, true), length: ctx.view.getUint32(at + 20, true)}; assert.ok(binding.low !== 0 || binding.high !== 0); ctx.expected.set(words([1, 24, binding.low, binding.high, binding.pointer, binding.length]), TRANSFER);} assert.ok(binding.pointer > 0 && binding.length > 8 && binding.pointer + binding.length <= ctx.bytes.length); bytes = Buffer.from(ctx.bytes.subarray(binding.pointer, binding.pointer + binding.length)); saved = artifact(`${label}.wasm`, bytes);}
  const arena_index = checkpoint(ctx, before, 'module publication'); assert.ok(WebAssembly.validate(bytes)); const shape = childShape(ctx, bytes, binding), module = new WebAssembly.Module(bytes); assert.deepEqual(WebAssembly.Module.imports(module), shape.imports.map(({module, name, kind}) => ({module, name, kind}))); assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]); const child = new WebAssembly.Instance(module, {env: {memory: ctx.memory}, ...(ctx.owner === 'standalone' ? {} : {ring3: ctx.api})}); assert.equal(child.exports.run.length, 4); modules.push({owner: ctx.owner, context: ctx.ordinal, batch_index: ctx.batch.index, label, mode, entries, specs, key: ctx.owner === 'standalone' ? [] : [ctx.low, ctx.high], ...binding, artifact: saved, arena_index, ...shape}); return {label, ...binding, run: child.exports.run};
}
function seed(ctx, registers, flags) {ctx.expected.set(state(registers, PC, flags)); ctx.expected.set(exit(3, 0), 56); ctx.expected.writeUInt32LE(0, 96); refresh(ctx).bytes.set(ctx.expected.subarray(0, 100), ctx.base); assert.deepEqual(arena(ctx), ctx.expected);}
function cancel(ctx, value) {ctx.expected.writeUInt32LE(value, 96); refresh(ctx).view.setUint32(ctx.base + 96, value, true);}
function run(ctx, unit, budget, name, registers, pc, flags, reason = 1, retired = 1, status = 0, malformed = false) {const before = Buffer.from(ctx.expected), pointers = malformed ? [0xffffffff, 0xffffffff, 0xffffffff] : [ctx.base, ctx.base + 56, ctx.base + 96]; assert.deepEqual(arena(ctx), before); if (status === 0) {ctx.expected.set(state(registers, pc, flags)); ctx.expected.set(exit(reason, retired), 56);} assert.equal(unit.run(pointers[0], pointers[1], budget, pointers[2]), status, name); const arena_index = checkpoint(ctx, before, name); generatedRuns.push({owner: ctx.owner, context: ctx.ordinal, batch_index: ctx.batch.index, module: unit.label, name, budget, pointers, status, reason: status ? null : reason, retired: status ? 0 : retired, arena_index}); return arena_index;}
function numeric(ctx, unit) {
  for (const [vector_index, vector] of VECTORS.entries()) for (const [flag_index, flags] of FLAGS.entries()) {
    const registers = [...vector]; seed(ctx, registers, flags);
    for (const form of ctx.batch.forms) for (const step of [1, 2]) {
      const old = [...registers]; registers[form.left] = old[form.right]; registers[form.right] = old[form.left];
      if (form.alias) assert.deepEqual(registers, old, 'same-register exchange preserves the whole vector');
      else {assert.notEqual(old[form.left], registers[form.left]); assert.notEqual(old[form.right], registers[form.right]);}
      if (step === 2) assert.deepEqual(registers, vector, 'same-pair exchange restores the whole vector');
      const instruction_offset = form.offset + (step - 1) * form.length, pc = PC + instruction_offset + form.length;
      const name = `batch-${ctx.batch.index}-vector-${vector_index}-flags-${flag_index}-pair-${form.pair_index}-xchg-${step}`;
      const arena_index = run(ctx, unit, 1, name, registers, pc, flags);
      cases.push({owner: ctx.owner, context: ctx.ordinal, batch_index: ctx.batch.index, vector_index, flag_index, flags, pair_index: form.pair_index, local_pair_index: form.local_pair_index, encoding: form.encoding, left: form.left, right: form.right, opcode: form.opcode, modrm: form.modrm, alias: form.alias, step, instruction_length: form.length, instruction_offset, input_left: vector[form.left], input_right: vector[form.right], before_left: old[form.left], before_right: old[form.right], expected_left: registers[form.left], expected_right: registers[form.right], before_registers: old, expected_registers: [...registers], pc, arena_index});
    }
    if (ctx.batch.index === 2) {
      const name = `batch-2-vector-${vector_index}-flags-${flag_index}-nop-90`, arena_index = run(ctx, unit, 1, name, registers, PC + 47, flags);
      nops.push({owner: ctx.owner, context: ctx.ordinal, batch_index: 2, vector_index, flag_index, flags, opcode: 0x90, instruction_offset: 46, instruction_length: 1, pc: PC + 47, retired: 1, arena_index});
    }
    const arena_index = run(ctx, unit, 1, `batch-${ctx.batch.index}-vector-${vector_index}-flags-${flag_index}-following-jump`, registers, PC + ctx.batch.compile_bytes, flags);
    continuations.push({owner: ctx.owner, context: ctx.ordinal, batch_index: ctx.batch.index, vector_index, flag_index, flags, pc: PC + ctx.batch.compile_bytes, retired: 1, arena_index});
  }
}
function finish(ctx, unit) {
  const registers = Array.from({length: 8}, (_, index) => ctx.expected.readUInt32LE(16 + index * 4)), pc = ctx.expected.readUInt32LE(48), flags = ctx.expected.readUInt32LE(52);
  for (const [name, cancelled, budget, reason, status, malformed] of [['zero-budget', 0, 0, 1, 0, false], ['cancel-before-zero-budget', 1, 0, 2, 0, false], ['cold-entry', 0, 1, 3, 0, false], ['malformed-entry-pointers', 0, 0, 0, 1, true]]) {cancel(ctx, cancelled); const arena_index = run(ctx, unit, budget, name, registers, pc, flags, reason, 0, status, malformed); controls.push({owner: ctx.owner, context: ctx.ordinal, batch_index: ctx.batch.index, name, call: 'generated', status, budget, retired: 0, arena_index});}
  page(ctx, 'after-positive-and-preflight-controls');
  if (ctx.owner !== 'standalone') {
    for (const name of ['wrong-key', 'wrong-identity']) {const before = Buffer.from(ctx.expected), low = name === 'wrong-key' ? (ctx.low ^ 1) >>> 0 : ctx.low; const status = ctx.owner === 'replacement' ? ctx.api.guard(low, ctx.high, name === 'wrong-identity' ? unit.generation + 1 : unit.generation, ctx.base, ctx.base + 56, ctx.base + 96) : ctx.api.guard_resident(low, ctx.high, name === 'wrong-identity' ? (unit.low ^ 1) >>> 0 : unit.low, unit.high, ctx.base, ctx.base + 56, ctx.base + 96); assert.equal(status, 3); const arena_index = checkpoint(ctx, before, name); controls.push({owner: ctx.owner, context: ctx.ordinal, batch_index: ctx.batch.index, name, call: 'guard', status, budget: null, retired: 0, arena_index});}
    assert.equal(ctx.api.write8(PC, ctx.batch.program[0]), 0); ctx.expected.set(hostByteHelper(), 100); assert.deepEqual(arena(ctx), ctx.expected, 'same-byte code write changes helper only');
    for (const [name, status] of [['same-byte-stale', 4], ['closed', 5]]) {if (name === 'closed') pure(ctx, () => ctx.api.close(), 'close retains arena'); const arena_index = run(ctx, unit, 0, name, registers, pc, flags, 0, 0, status, true); controls.push({owner: ctx.owner, context: ctx.ordinal, batch_index: ctx.batch.index, name, call: 'generated', status, budget: 0, retired: 0, arena_index});}
  }
}
for (const owner of ['standalone', 'replacement', 'resident']) for (const batch of BATCHES) {const ctx = fresh(owner, batch), unit = compile(ctx); page(ctx, 'before-positive-runs'); numeric(ctx, unit); finish(ctx, unit);}
const counts = {contexts: 9, modules: 9, seed_groups: 36, xchg32_retired: 1704, nonalias_changed_first: 756, nonalias_round_trip_restorations: 756, alias_unchanged_first: 96, alias_unchanged_second: 96, nop_retired: 12, following_jumps: 36, guest_retired: 1752, generated_runs: 1800, generated_zero_status: 1779, generated_nonzero_status: 21, controls: 60, generated_controls: 48, direct_guard_controls: 12, arenas: 1821, pages: 18, resident_peak: 1, runtime_sources: 90, raw_files: 5513};
assert.equal(contexts.length, 9); assert.equal(modules.length, 9); assert.equal(cases.length, 1704); assert.equal(nops.length, 12); assert.equal(continuations.length, 36); assert.equal(generatedRuns.length, 1800); assert.equal(generatedRuns.reduce((n, row) => n + row.retired, 0), 1752); assert.equal(generatedRuns.filter(row => row.status !== 0).length, 21); assert.equal(generatedRuns.filter(row => row.status === 0).length, 1779); assert.equal(controls.length, 60); assert.equal(controls.filter(row => row.call === 'generated').length, 48); assert.equal(arenas.length, 1821); assert.equal(pages.length, 18); assert.equal(rawFiles.length, 5513);
assert.equal(cases.filter(row => row.step === 1 && !row.alias && row.before_left !== row.expected_left && row.before_right !== row.expected_right).length, 756);
assert.equal(cases.filter(row => row.step === 2 && !row.alias && row.expected_left === row.input_left && row.expected_right === row.input_right && row.before_left !== row.expected_left && row.before_right !== row.expected_right).length, 756);
for (const step of [1, 2]) assert.equal(cases.filter(row => row.step === step && row.alias && row.before_left === row.expected_left && row.before_right === row.expected_right).length, 96);
const result = {status: 'ok', engine_sha256: hash(engineBytes), engine_exports: WebAssembly.Module.exports(engineModule), source_sha256: sourceHashes, tools: {node: process.version, v8: process.versions.v8}, counts, oracle: {register_order: ['eax', 'ecx', 'edx', 'ebx', 'esp', 'ebp', 'esi', 'edi'], vectors: VECTORS, flag_seeds: FLAGS, batch_programs: BATCHES.map(batch => batch.program.toString('hex')), batch_compile_bytes: [114, 114, 49], batch_instructions: [57, 57, 32], batch_forms: BATCHES.map(batch => batch.forms), pc: PC, page_pattern: 'i%251+1', page_last_word: 0x504f4e4d, standalone_base: 128, standalone_data_address: 0x8000, bound_data_address: 0x3000, bound_key_high: 0xe3880000, arena_bytes: SIZE, transfer_offset: TRANSFER, child_body_scope: 'typed declarations and baked guard prefix only; remaining bodies unparsed'}, contexts, modules, generated_runs: generatedRuns, cases, nops, continuations, controls, arenas, pages, raw_files: rawFiles, claim: 'finite authored unprefixed flat32 register XCHG32 all64 ordered87/register pairs and seven91..97 short forms with90NOP retained, two distinct eight-GPR vectors and two valid fullFLAGS seeds in nine standalone/replacement/resident batch contexts; simultaneous two-old-word exchange,756 nonalias first changes and756 second restorations,192 unchanged same-register alias calls, every intermediate GPR/fullFLAGS/EIP/budget state plus12NOP/36followingJMP without intragroup CPU repair, complete4236arenas and untouched patterned4KB pages under unchanged64instruction cap with focused preflight/owner guards; no exhaustiveuint32/byteword16/64/prefixrelaxation/memoryatomic/implicitlocking/LOCKhardware/CPUID/memoryfault/repair/SMC/PE/provider/Windows/SDK/browser/performance/fullISA/fullCI/game claim'};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2), {flag: 'wx'});
console.log(JSON.stringify({status: result.status, engine_sha256: result.engine_sha256, counts, output}));
