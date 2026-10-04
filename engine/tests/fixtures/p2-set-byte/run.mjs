import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(root, 'expected engine wasm, output directory and repository root');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const engineBytes = readFileSync(enginePath), engineModule = new WebAssembly.Module(engineBytes);
assert.deepEqual(WebAssembly.Module.imports(engineModule), []);
const arenaSize = 4236, transfer = 140, rows = [], controls = [], peRows = [], modules = [];
const initialRegisters = [0x1234807f, 0x23457f80, 0x34560f10, 0x4567ff00, 0x56789abc, 0x6789abcd, 0x789abcde, 0x89abcdef];
const apiNames = ['open', 'close', 'arena_ptr', 'map', 'upload', 'read32', 'write8', 'compile_entries', 'compile_resident_entries', 'generation', 'module_ptr', 'module_len', 'guard', 'guard_resident', 'store32', 'store_resident32', 'begin_image_input', 'append_image_input', 'load_pe32_linked_v2_input_at', 'start_loaded_image', 'capture_call', 'capture_resident_call', 'complete_windows_call', 'complete_resident_windows_call'];
let ordinal = 0;

function record(magic, length, fields, version = 1) {
  const bytes = new Uint8Array(length), view = new DataView(bytes.buffer);
  bytes.set(Buffer.from(magic)); view.setUint16(4, version, true); view.setUint16(6, 1, true); view.setUint32(8, length, true);
  fields.forEach((value, index) => view.setUint32(16 + index * 4, value, true));
  return bytes;
}
function words(fields) {
  const bytes = new Uint8Array(fields.length * 4), view = new DataView(bytes.buffer);
  fields.forEach((value, index) => view.setUint32(index * 4, value, true));
  return bytes;
}
function refresh(ctx) {
  ctx.bytes = new Uint8Array(ctx.memory.buffer); ctx.view = new DataView(ctx.memory.buffer);
  return ctx;
}
function snapshot(ctx) { return refresh(ctx).bytes.slice(ctx.base, ctx.base + arenaSize); }
function state(ctx) {
  refresh(ctx);
  return {registers: Array.from({length: 8}, (_, index) => ctx.view.getUint32(ctx.base + 16 + index * 4, true)), pc: ctx.view.getUint32(ctx.base + 48, true), flags: ctx.view.getUint32(ctx.base + 52, true)};
}
function request(ctx, bytes) {
  assert.ok(bytes.length <= 4096); refresh(ctx).bytes.set(bytes, ctx.base + transfer);
}
function fresh(owner, pages = 8) {
  const instance = new WebAssembly.Instance(engineModule, {});
  const api = Object.fromEntries(apiNames.map(name => [name, instance.exports[`ring3_abi_v1_${name}`]]));
  for (const [name, fn] of Object.entries(api)) assert.equal(typeof fn, 'function', name);
  const key = 0xc835800000000000n + BigInt(++ordinal);
  const ctx = {owner, memory: instance.exports.memory, api, low: Number(key & 0xffffffffn), high: Number(key >> 32n)};
  assert.equal(api.open(pages, ctx.low, ctx.high), 0); ctx.base = api.arena_ptr() >>> 0;
  return refresh(ctx);
}
function upload(ctx, address, bytes) {
  request(ctx, bytes); assert.equal(ctx.api.upload(address, bytes.length), 0); refresh(ctx);
}
function instantiate(ctx, bytes, label, binding = {}) {
  assert.ok(WebAssembly.validate(bytes));
  const module = new WebAssembly.Module(bytes), imports = WebAssembly.Module.imports(module);
  assert.equal(imports[0].module, 'env'); assert.equal(imports[0].name, 'memory');
  assert.equal(imports[0].kind, 'memory');
  const helpers = imports.slice(1).map(row => {
    assert.equal(row.module, 'ring3'); assert.equal(row.kind, 'function');
    assert.equal(typeof ctx.api[row.name], 'function'); return row.name;
  });
  const instance = new WebAssembly.Instance(module, {env: {memory: ctx.memory}, ring3: ctx.api});
  assert.equal(instance.exports.run.length, 4);
  writeFileSync(join(output, `${label}.wasm`), bytes);
  modules.push({label, owner: ctx.owner, sha256: hash(bytes), imports});
  return {...binding, run: instance.exports.run, helpers};
}
function compile(ctx, entries, gates, label) {
  request(ctx, words([...entries, ...gates.flat()]));
  let binding;
  if (ctx.owner === 'replacement') {
    assert.equal(ctx.api.compile_entries(entries.length, gates.length), 0);
    binding = {generation: ctx.api.generation(), pointer: ctx.api.module_ptr() >>> 0, length: ctx.api.module_len() >>> 0};
  } else {
    assert.equal(ctx.api.compile_resident_entries(entries.length, gates.length), 0); refresh(ctx);
    const at = ctx.base + transfer;
    assert.equal(ctx.view.getUint32(at, true), 1); assert.equal(ctx.view.getUint32(at + 4, true), 24);
    binding = {low: ctx.view.getUint32(at + 8, true), high: ctx.view.getUint32(at + 12, true), pointer: ctx.view.getUint32(at + 16, true), length: ctx.view.getUint32(at + 20, true)};
    assert.ok(binding.low !== 0 || binding.high !== 0);
  }
  refresh(ctx); assert.ok(binding.pointer > 0 && binding.length > 8 && binding.pointer + binding.length <= ctx.bytes.length);
  return instantiate(ctx, ctx.bytes.slice(binding.pointer, binding.pointer + binding.length), label, binding);
}
function seed(ctx, registers, pc, flags, cancel = 0) {
  refresh(ctx).bytes.set(record('R3ST', 56, [...registers, pc, flags]), ctx.base);
  ctx.bytes.set(record('R3EX', 40, [3, 0, 0, 0, 0, 0]), ctx.base + 56);
  ctx.view.setUint32(ctx.base + 96, cancel, true);
  ctx.bytes.fill(0xa5, ctx.base + 100, ctx.base + arenaSize);
}
function byte(registers, index) {
  return Math.floor(registers[index % 4] / (index < 4 ? 1 : 256)) % 256;
}
function replaceByte(registers, index, value) {
  const factor = index < 4 ? 1 : 256, parent = index % 4;
  registers[parent] += (value - byte(registers, index)) * factor;
}
const truthMasks = [
  0xffff0000, 0x0000ffff, 0xaaaaaaaa, 0x55555555,
  0xf0f0f0f0, 0x0f0f0f0f, 0xfafafafa, 0x05050505,
  0xff00ff00, 0x00ff00ff, 0xcccccccc, 0x33333333,
  0x00ffff00, 0xff0000ff, 0xf0fffff0, 0x0f00000f,
];
function conditionValue(condition, flags) {
  const bits = [1, 4, 0x40, 0x80, 0x800];
  const index = bits.reduce((value, bit, column) => value + (Math.floor(flags / bit) % 2) * 2 ** column, 0);
  return Math.floor(truthMasks[condition] / 2 ** index) % 2;
}
function flagCombination(index, salt = 0) {
  return 2 + [1, 4, 0x40, 0x80, 0x800].reduce((flags, bit, column) => flags + Math.floor(index / 2 ** column) % 2 * bit, 0)
    + (index + salt) % 2 * 0x10 + (Math.floor(index / 2) + Math.floor(salt / 2)) % 2 * 0x400;
}
assert.deepEqual(Array.from({length: 16}, (_, condition) => conditionValue(condition, 2)), [0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1]);
assert.deepEqual(Array.from({length: 16}, (_, condition) => conditionValue(condition, 0xcd7)), [1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 0, 1, 1, 0]);
assert.equal(conditionValue(12, 0x82), 1);
assert.equal(conditionValue(12, 0x882), 0);
assert.equal(conditionValue(15, 0x882), 1);
function runSet(ctx, unit, form, registers, flags, name) {
  seed(ctx, registers, form.pc, flags);
  const expected = snapshot(ctx), view = new DataView(expected.buffer);
  const result = conditionValue(form.condition, flags), expectedRegisters = [...registers];
  replaceByte(expectedRegisters, form.destination, result);
  view.setUint32(16 + form.destination % 4 * 4, expectedRegisters[form.destination % 4], true);
  view.setUint32(48, form.pc + form.bytes.length, true);
  expected.set(record('R3EX', 40, [1, 1, 0, 0, 0, 0]), 56);
  const wholeMemory = ctx.owner === 'standalone' ? ctx.bytes.slice() : undefined;
  assert.equal(unit.run(ctx.base, ctx.base + 56, 1, ctx.base + 96), 0, name);
  assert.deepEqual(snapshot(ctx), expected, `${name}: partial parent, all registers/flags/exit/helper/transfer`);
  if (wholeMemory) {
    wholeMemory.set(expected, ctx.base); assert.deepEqual(ctx.bytes, wholeMemory, `${name}: full standalone memory`);
  }
  rows.push({owner: ctx.owner, name, kind: form.kind, encoding: Buffer.from(form.bytes).toString('hex'), condition: form.condition, destination: form.destination, expected_byte: result, flags});
}
function noRetirement(ctx, unit, form, cancel) {
  seed(ctx, initialRegisters, form.pc, 0xcd7, cancel);
  const expected = snapshot(ctx); expected.set(record('R3EX', 40, [cancel ? 2 : 1, 0, 0, 0, 0, 0]), 56);
  assert.equal(unit.run(ctx.base, ctx.base + 56, 0, ctx.base + 96), 0);
  assert.deepEqual(snapshot(ctx), expected);
  controls.push({owner: ctx.owner, name: cancel ? 'cancel_before_zero_budget' : 'zero_budget', retired: 0});
}
function read(ctx, address) {
  assert.equal(ctx.api.read32(address), 0); refresh(ctx);
  assert.equal(ctx.view.getUint32(ctx.base + 116, true), 0);
  return ctx.view.getUint32(ctx.base + 120, true);
}
const forms = [];
for (let condition = 0; condition < 16; condition++) for (let destination = 0; destination < 8; destination++) {
  forms.push({kind: 'canonical', condition, destination, bytes: [0x0f, 0x90 + condition, 0xc0 | destination]});
}
for (let condition = 0; condition < 16; condition++) for (let ignored = 0; ignored < 8; ignored++) {
  const destination = (condition + ignored) % 8;
  forms.push({kind: 'ignored_reg_alias', condition, destination, ignored, bytes: [0x0f, 0x90 + condition, 0xc0 | ignored * 8 | destination]});
}
assert.equal(forms.length, 256);
const batches = [];
for (let index = 0; index < forms.length; index += 32) {
  const group = forms.slice(index, index + 32), entry = 0x1000 + batches.length * 0x200;
  let pc = entry;
  group.forEach(form => {form.pc = pc; pc += form.bytes.length;});
  const code = Buffer.from([...group.flatMap(form => form.bytes), 0xeb, 0, 0x0f, 0x0b]);
  assert.deepEqual(code, readFileSync(join(output, `batch-${batches.length}.x86`)), 'independent Rust/JS encoding agrees');
  batches.push({entry, forms: group, code});
}
assert.equal(batches.length, 8);
for (const owner of ['standalone', 'replacement', 'resident']) {
  const ctx = owner === 'standalone' ? refresh({owner, memory: new WebAssembly.Memory({initial: 1}), base: 128}) : fresh(owner);
  if (owner !== 'standalone') {
    assert.equal(ctx.api.map(0x1000, 1, 7), 0); assert.equal(ctx.api.map(0x9000, 1, 3), 0);
    const probe = Buffer.from('00112233445566778899aabbccddeeff0123456789abcdefa5a5a5a55a5a5a5a', 'hex');
    upload(ctx, 0x9000, probe);
    for (const batch of batches) upload(ctx, batch.entry, batch.code);
  }
  let lastUnit;
  for (const [index, batch] of batches.entries()) {
    const label = `${owner}-batch${index}`;
    const unit = owner === 'standalone' ? instantiate(ctx, readFileSync(join(output, `batch-${index}.wasm`)), label) : compile(ctx, [batch.entry], [], label);
    assert.deepEqual(unit.helpers, owner === 'standalone' ? [] : [owner === 'replacement' ? 'guard' : 'guard_resident'], `${label}: SETcc adds no helper`);
    const probeBefore = owner === 'standalone' ? undefined : Array.from({length: 8}, (_, word) => read(ctx, 0x9000 + word * 4));
    for (const [offset, form] of batch.forms.entries()) {
      if (form.kind === 'canonical') {
        for (let combination = 0; combination < 32; combination++) {
          runSet(ctx, unit, form, [...initialRegisters], flagCombination(combination, form.condition * 8 + form.destination), `${label}-form${offset}-flags${combination}`);
        }
      } else {
        for (const result of [0, 1]) {
          const flags = Array.from({length: 32}, (_, index) => flagCombination(index, form.condition * 8 + form.destination)).find(value => conditionValue(form.condition, value) === result);
          assert.notEqual(flags, undefined);
          runSet(ctx, unit, form, [...initialRegisters], flags, `${label}-alias${offset}-value${result}`);
        }
      }
    }
    noRetirement(ctx, unit, batch.forms[0], 0); noRetirement(ctx, unit, batch.forms[0], 1);
    if (probeBefore) assert.deepEqual(Array.from({length: 8}, (_, word) => read(ctx, 0x9000 + word * 4)), probeBefore, `${label}: declared guest RAM unchanged`);
    lastUnit = unit;
  }
  if (owner !== 'standalone') {
    assert.equal(ctx.api.write8(0x1000, batches[0].code[0]), 0);
    const before = snapshot(ctx);
    assert.equal(lastUnit.run(0xffffffff, 0xffffffff, 0, 0xffffffff), 4);
    assert.deepEqual(snapshot(ctx), before); controls.push({owner, name: 'same_byte_code_stale', status: 4});
    assert.equal(ctx.api.close(), 0);
    assert.equal(lastUnit.run(0xffffffff, 0xffffffff, 0, 0xffffffff), 5);
    controls.push({owner, name: 'closed', status: 5});
  }
}

const image = readFileSync(join(output, 'set.exe'));
assert.equal(image.length, 2560);
assert.equal(image.subarray(0, 2).toString(), 'MZ');
const expectedProgram = 'b87f803412b9807f4523bacdab5634bba5a5674538c80f92c20f9dc60f97c780fa01753984ec0f95c40f94c180fc00752c80f901752780ff00752238f2751eba0300c0c889150030400052ff1550314000c70504304000a5a5a5a50f0bbadec0adde89150030400052ff15503140000f0b';
assert.equal(image.subarray(0x200, 0x200 + expectedProgram.length / 2).toString('hex'), expectedProgram);
const callerOffsets = [0, 36, 49, 54, 59, 63, 93];
const branchEdges = [[34, 0x75, 93, 36], [47, 0x75, 93, 49], [52, 0x75, 93, 54], [57, 0x75, 93, 59], [61, 0x75, 93, 63]];
for (const [offset, opcode, target, fallthrough] of branchEdges) {
  assert.equal(image[0x200 + offset], opcode);
  assert.equal(offset + 2, fallthrough);
  assert.equal(offset + 2 + image.readInt8(0x201 + offset), target);
  assert.ok(callerOffsets.includes(target) && callerOffsets.includes(fallthrough));
}
for (const base of [0x400000, 0x500000]) for (const owner of ['replacement', 'resident']) {
  const label = `pe-${owner}-${base.toString(16)}`, ctx = fresh(owner);
  assert.equal(ctx.api.begin_image_input(image.length), 0); request(ctx, image);
  assert.equal(ctx.api.append_image_input(0, image.length), 0);
  refresh(ctx).bytes.fill(0xa5, ctx.base + transfer, ctx.base + arenaSize);
  assert.equal(ctx.api.load_pe32_linked_v2_input_at(base, 0x8000), 0); refresh(ctx);
  const receipt = ctx.bytes.slice(ctx.base + transfer, ctx.base + transfer + 72), view = new DataView(receipt.buffer);
  assert.equal(Buffer.from(receipt.subarray(0, 4)).toString(), 'R3LI'); assert.equal(view.getUint32(16, true), base);
  const entry = view.getUint32(24, true), gateCount = view.getUint32(36, true);
  assert.equal(entry, base + 0x1000); assert.equal(gateCount, 1);
  const gates = [[view.getUint32(48, true), view.getUint32(52, true)]];
  assert.deepEqual(gates, [[0x8020, 0x10003]]);
  assert.equal(read(ctx, base + 0x3150), 0x8020, 'Rust resolves named ExitProcess IAT');
  for (const offset of [70, 83, 100]) assert.equal(read(ctx, entry + offset), base + (offset === 83 ? 0x3004 : 0x3000));
  for (const offset of [77, 107]) assert.equal(read(ctx, entry + offset), base + 0x3150);
  const callerEntries = callerOffsets.map(offset => entry + offset);
  if (owner === 'replacement') callerEntries.push(gates[0][0]);
  const caller = compile(ctx, callerEntries, owner === 'replacement' ? gates : [], `${label}-caller`);
  const gate = owner === 'replacement' ? caller : compile(ctx, [gates[0][0]], gates, `${label}-gate`);
  assert.equal(ctx.api.start_loaded_image(0x70000, 1), 0);
  assert.deepEqual(state(ctx), {registers: [0, 0, 0, 0, 0x71000, 0, 0, 0], pc: entry, flags: 2});
  assert.equal(caller.run(ctx.base, ctx.base + 56, 64, ctx.base + 96), 0); refresh(ctx);
  assert.equal(ctx.view.getUint32(ctx.base + 76, true), 25, `${label}: genuine guest instruction count`);
  const expectedState = {registers: [0x1234007f, 0x23457f01, 0xc8c00003, 0x456700a5, 0x70ff8, 0, 0, 0], pc: 0x8020, flags: 0x46};
  assert.deepEqual(state(ctx), expectedState, `${label}: real byte branches preserve original parents`);
  if (owner === 'resident') {
    assert.deepEqual(snapshot(ctx).subarray(56, 96), record('R3EX', 40, [3, 25, 0, 0, 0, 0], 2));
    assert.equal(gate.run(ctx.base, ctx.base + 56, 1, ctx.base + 96), 0);
  }
  assert.deepEqual(snapshot(ctx).subarray(56, 96), record('R3EX', 40, [8, owner === 'resident' ? 0 : 25, 0x10003, 0, 0, 0], 3));
  const capture = owner === 'replacement' ? ctx.api.capture_call(ctx.low, ctx.high, gate.generation, 2, 1) : ctx.api.capture_resident_call(ctx.low, ctx.high, gate.low, gate.high, 2, 1);
  assert.equal(capture, 0);
  assert.equal(owner === 'replacement' ? ctx.api.complete_windows_call(ctx.low, ctx.high, gate.generation, 1) : ctx.api.complete_resident_windows_call(ctx.low, ctx.high, gate.low, gate.high, 1), 0);
  assert.deepEqual(state(ctx), expectedState);
  assert.deepEqual(snapshot(ctx).subarray(56, 96), record('R3EX', 40, [9, 0, 0xc8c00003, 0, 0, 0], 4));
  assert.equal(read(ctx, base + 0x3000), 0xc8c00003); assert.equal(read(ctx, base + 0x3004), 0, 'after-call canary was not executed');
  assert.equal(read(ctx, 0x70ffc), 0xc8c00003); assert.equal(read(ctx, 0x70ff8), entry + 81, 'real guest CALL return address');
  assert.equal(caller.run(0xffffffff, 0xffffffff, 0, 0xffffffff), 21, 'terminal latch precedes malformed pointers and budget');
  peRows.push({owner, base, guest_retired: 25, branch_result: 0xc8c00003, final_flags: 0x46, canary: 0, receipt_sha256: hash(receipt)});
  assert.equal(ctx.api.close(), 0);
}
const sourcePaths = ['Cargo.toml', 'Cargo.lock', 'engine/Cargo.toml', 'engine/src/cpu/x86/ir.rs', 'engine/src/cpu/x86/decode/flow.rs', 'engine/src/cpu/dbt/region.rs', 'engine/src/cpu/dbt/wasm/integer.rs', 'engine/src/cpu/dbt/wasm/control.rs', 'engine/tests/cpu_set_byte_wasm.rs', 'engine/tests/fixtures/p2-set-byte/run.mjs', 'engine/tests/support/pe32.rs'];
const canonicalCases = rows.filter(row => row.kind === 'canonical').length;
const aliasCases = rows.filter(row => row.kind === 'ignored_reg_alias').length;
assert.equal(canonicalCases, 12288); assert.equal(aliasCases, 768);
const result = {status: 'ok', engine_sha256: hash(engineBytes), pe_sha256: hash(image), source_sha256: Object.fromEntries(sourcePaths.map(path => [path, hash(readFileSync(join(root, path)))])), tools: {node: process.version, v8: process.versions.v8}, flag_policy: 'all admitted flags, including AF/DF and reserved bit 1, preserved byte-exact; all 32 relevant CF/PF/ZF/SF/OF combinations tested', counts: {set_cases: rows.length, canonical_forms_per_mode: 128, relevant_flag_combinations: 32, canonical_cases: canonicalCases, ignored_reg_alias_cases: aliasCases, finite_guest_retired: rows.length, pe_paths: peRows.length, pe_guest_retired: peRows.reduce((count, row) => count + row.guest_retired, 0)}, cases: rows, controls, pe: peRows, modules, claim: 'finite authored SETcc in native-generated standalone Wasm and engine-Wasm-generated bound modules, plus four copied PE startup/branch/ExitProcess paths; no browser, SDK, general ISA, performance or playable game claim'};
assert.equal(rows.length, 13056); assert.equal(peRows.length, 4);
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2));
console.log(JSON.stringify({status: result.status, engine_sha256: result.engine_sha256, counts: result.counts, output}));
