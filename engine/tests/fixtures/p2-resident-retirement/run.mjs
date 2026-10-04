import assert from 'node:assert/strict';
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
const SIZE = 4236, TRANSFER = 140, MARKER = 0xc8c00006, EXIT = 0x8020, EXIT_ID = 0x10003;
const apiNames = ['open', 'close', 'arena_ptr', 'read32', 'store_resident32', 'compile_resident_entries', 'resident_module', 'guard_resident', 'dispatcher_module', 'guard_dispatch_entry', 'find_installed_resident', 'acknowledge_resident_installation', 'retire_stale_resident', 'begin_image_input', 'append_image_input', 'load_pe32_linked_v2_input_at', 'start_loaded_image', 'capture_resident_call', 'complete_resident_windows_call'];
const modules = [], controls = [], paths = [];
let ordinal = 0, retired = 0, retirements = 0, recompilations = 0, captures = 0, completions = 0;
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
function request(ctx, bytes) { assert.ok(bytes.length <= 4096); refresh(ctx).bytes.set(bytes, ctx.base + TRANSFER); }
function pure(ctx, action, status, name) {
  const before = arena(ctx); assert.equal(action(), status, `${ctx.label}: ${name}`);
  assert.deepEqual(arena(ctx), before, `${name}: full4236 arena unchanged`);
  controls.push({path: ctx.label, name, status});
}
function cpu(pc, eax = 0, esp = 0x71000, flags = 2) { return {registers: [eax, 0, 0, 0, esp, 0, 0, 0], pc, flags}; }
const stateRecord = state => record('R3ST', 56, [...state.registers, state.pc, state.flags]);
const exitRecord = (reason, count, version, detail = 0) => record('R3EX', 40, [reason, count, detail, 0, 0, 0], version);
const installRecord = (unit, slot) => record('R3IN', 32, [unit.low, unit.high, slot, 0]);
function read(ctx, address) {
  const before = arena(ctx); assert.equal(ctx.api.read32(address), 0);
  refresh(ctx); assert.equal(ctx.view.getUint32(ctx.base + 116, true), 0, `read ${address.toString(16)}`);
  const value = ctx.view.getUint32(ctx.base + 120, true), expected = before.slice();
  expected.set(record('R3MH', 40, [0, value, 0, 0, 0, 0]), 100); assert.deepEqual(arena(ctx), expected, 'host RAM observation changes only helper');
  return value;
}
function readBytes(ctx, address, length) {
  assert.equal(length % 4, 0); return words(Array.from({length: length / 4}, (_, index) => read(ctx, address + index * 4)));
}
function fresh(label) {
  const instance = new WebAssembly.Instance(engineModule, {}), key = 0xb361000000000000n + BigInt(++ordinal);
  const api = Object.fromEntries(apiNames.map(name => [name, instance.exports[`ring3_abi_v1_${name}`]]));
  for (const [name, fn] of Object.entries(api)) assert.equal(typeof fn, 'function', name);
  assert.equal(api.retire_stale_resident.length, 4);
  const ctx = {label, api, memory: instance.exports.memory, low: Number(key & 0xffffffffn), high: Number(key >> 32n), nextId: 0n, live: new Set()};
  assert.equal(api.open(12, ctx.low, ctx.high), 0); ctx.base = api.arena_ptr() >>> 0;
  ctx.table = new WebAssembly.Table({element: 'anyfunc', initial: 8, maximum: 8}); return refresh(ctx);
}
const childImports = helpers => [{module: 'env', name: 'memory', kind: 'memory'}, {module: 'ring3', name: 'guard_resident', kind: 'function'}, ...helpers.map(name => ({module: 'ring3', name, kind: 'function'}))];
const dispatcherImports = [{module: 'env', name: 'memory', kind: 'memory'}, {module: 'env', name: 'table', kind: 'table'}, {module: 'ring3', name: 'guard_dispatch_entry', kind: 'function'}, {module: 'ring3', name: 'find_installed_resident', kind: 'function'}];
function saveModule(ctx, bytes, label, imports, binding = {}) {
  const module = new WebAssembly.Module(bytes); assert.deepEqual(WebAssembly.Module.imports(module), imports);
  assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  const instance = new WebAssembly.Instance(module, {env: {memory: ctx.memory, table: ctx.table}, ring3: ctx.api});
  assert.equal(instance.exports.run.length, 4); writeFileSync(join(output, `${label}.wasm`), bytes);
  modules.push({label, path: ctx.label, sha256: hash(bytes), imports, ...binding}); return {bytes, run: instance.exports.run, ...binding};
}
function dispatcher(ctx) {
  const before = arena(ctx); assert.equal(ctx.api.dispatcher_module(ctx.low, ctx.high), 0); refresh(ctx);
  const pointer = ctx.view.getUint32(ctx.base + TRANSFER + 24, true), length = ctx.view.getUint32(ctx.base + TRANSFER + 28, true);
  assert.ok(pointer > 0 && length > 8 && pointer + length <= ctx.bytes.length);
  const expected = before.slice(); expected.set(record('R3DP', 32, [ctx.low, ctx.high, pointer, length]), TRANSFER);
  assert.deepEqual(arena(ctx), expected, 'dispatcher receipt only');
  const bytes = ctx.bytes.slice(pointer, pointer + length);
  if (ctx.dispatcher) {
    assert.equal(pointer, ctx.dispatcher.pointer); assert.equal(length, ctx.dispatcher.length); assert.deepEqual(bytes, ctx.dispatcher.bytes);
  } else ctx.dispatcher = saveModule(ctx, bytes, `${ctx.label}-dispatcher`, dispatcherImports, {pointer, length});
}
function compile(ctx, entries, gates, label, slot, helpers) {
  assert.ok(entries.length <= 8); request(ctx, words([...entries, ...gates.flat()]));
  const before = arena(ctx); assert.equal(ctx.api.compile_resident_entries(entries.length, gates.length), 0); refresh(ctx);
  const fields = Array.from({length: 6}, (_, index) => ctx.view.getUint32(ctx.base + TRANSFER + index * 4, true));
  assert.deepEqual(fields.slice(0, 2), [1, 24]); const [, , low, high, pointer, length] = fields;
  const id = BigInt(low) | BigInt(high) << 32n; assert.equal(id, ++ctx.nextId, 'fresh monotonically allocated immutable id');
  assert.ok(pointer > 0 && length > 8 && pointer + length <= ctx.bytes.length);
  const expected = before.slice(); expected.set(words(fields), TRANSFER); assert.deepEqual(arena(ctx), expected, 'resident receipt only');
  // copy and instantiate while the raw pointer is alive. no retired pointer is read later.
  const unit = saveModule(ctx, ctx.bytes.slice(pointer, pointer + length), label, childImports(helpers), {low, high, id: id.toString(), pointer, length, entries, gates, slot});
  ctx.live.add(unit.id); assert.ok(ctx.live.size <= 3); ctx.table.set(slot, unit.run);
  const ackBefore = arena(ctx); assert.equal(ctx.api.acknowledge_resident_installation(ctx.low, ctx.high, low, high, slot), 0);
  const ackExpected = ackBefore.slice(); ackExpected.set(installRecord(unit, slot), TRANSFER); assert.deepEqual(arena(ctx), ackExpected);
  assert.equal(ctx.table.get(slot), unit.run); return unit;
}
function guard(ctx, unit, status, name) {
  pure(ctx, () => ctx.api.guard_resident(ctx.low, ctx.high, unit.low, unit.high, ctx.base, ctx.base + 56, ctx.base + 96), status, name);
}
function stable(ctx, keeper) {
  guard(ctx, keeper, 0, 'unrelated keeper stays current'); const before = arena(ctx);
  assert.equal(ctx.api.resident_module(keeper.low, keeper.high), 0); refresh(ctx);
  const fields = [1, 24, keeper.low, keeper.high, keeper.pointer, keeper.length], expected = before.slice();
  expected.set(words(fields), TRANSFER); assert.deepEqual(arena(ctx), expected, 'unrelated current keeper getter preserves its exact pointer/length');
  assert.deepEqual(ctx.bytes.slice(keeper.pointer, keeper.pointer + keeper.length), keeper.bytes, 'current keeper raw pointer and bytes stay stable');
  dispatcher(ctx);
}
function dispatch(ctx, unit, count, state, reason, version, detail = 0, slot = 0) {
  const expected = arena(ctx); expected.set(stateRecord(state), 0); expected.set(exitRecord(reason, count, version, detail), 56);
  expected.set(record('R3MH', 40, [0, 0, 0, 0, 0, 0]), 100); expected.set(installRecord(unit, slot), TRANSFER);
  assert.equal(ctx.dispatcher.run(ctx.base, ctx.base + 56, 64, ctx.base + 96), 0);
  assert.deepEqual(arena(ctx), expected, 'product dispatcher whole-arena state/exit/helper/last lookup'); retired += count;
}

const image = readFileSync(join(output, 'retirement.exe'));
const program = 'ff0500304000c705111040000600c0c8b80600c0c8833d00304000097ce289050430400050ff1550314000c70508304000a5a5a5a50f0b';
const seeds = [0, 16, 30], relocationOffsets = [2, 8, 23, 32, 39, 45];
const invalidatedFlags = [2, 3, 7, 3, 7, 7, 3, 3, 7], sliceCounts = [2, 5, 5, 5, 5, 5, 5, 5, 5, 6];
assert.equal(image.length, 2560); assert.equal(image.subarray(0, 2).toString(), 'MZ');
assert.equal(image.subarray(0x200, 0x237).toString('hex'), program);
assert.equal(image[0x21c], 0x7c); assert.equal(28 + 2 + image.readInt8(0x21d), 0); assert.ok(seeds.includes(0) && seeds.includes(30));
assert.equal(image.subarray(0x900, 0x905).toString('hex'), '90eb000f0b');
assert.deepEqual(relocationOffsets.map((offset, index) => image.readUInt16LE(0x808 + index * 2)), relocationOffsets.map(offset => 0x3000 | offset));
assert.equal(sliceCounts.reduce((sum, value) => sum + value, 0), 48, 'literal instruction oracle frozen before execution');
// these small positive integers cannot overflow; inc preserves the previous cmp carry flag.
for (let index = 0; index < 9; index++) {
  let ones = 0; for (let value = index + 1; value; value >>>= 1) ones += value & 1;
  assert.equal(invalidatedFlags[index], 2 | Number(index > 0) | (ones % 2 === 0 ? 4 : 0));
}
function load(base) {
  const label = `base-${base.toString(16)}`, ctx = fresh(label), entry = base + 0x1000;
  assert.equal(ctx.api.begin_image_input(image.length), 0); request(ctx, image); assert.equal(ctx.api.append_image_input(0, image.length), 0);
  const before = arena(ctx); assert.equal(ctx.api.load_pe32_linked_v2_input_at(base, 0x8000), 0);
  const receipt = record('R3LI', 72, [base, 0x6000, entry, 5, 0x8000, 1, 0, 0, EXIT, EXIT_ID, 0, 0, 0, 0], 2);
  const expected = before.slice(); expected.set(receipt, TRANSFER); assert.deepEqual(arena(ctx), expected, 'base-specific linked-v2 receipt only');
  const text = new Uint8Array(4096), data = new Uint8Array(4096), keep = new Uint8Array(4096);
  text.set(image.subarray(0x200, 0x400)); data.set(image.subarray(0x400, 0x800)); keep.set(image.subarray(0x800, 0xa00));
  const textView = new DataView(text.buffer), dataView = new DataView(data.buffer);
  for (const offset of relocationOffsets) textView.setUint32(offset, image.readUInt32LE(0x200 + offset) + base - 0x400000, true);
  dataView.setUint32(0x150, EXIT, true);
  assert.deepEqual(readBytes(ctx, entry, 4096), text); assert.deepEqual(readBytes(ctx, base + 0x3000, 4096), data); assert.deepEqual(readBytes(ctx, base + 0x5000, 4096), keep);
  const main = compile(ctx, seeds.map(offset => entry + offset), [], `${label}-main-0`, 0, ['read32', 'store_resident32']);
  const keeper = compile(ctx, [base + 0x5100], [], `${label}-keeper`, 1, []);
  const gate = compile(ctx, [EXIT], [[EXIT, EXIT_ID]], `${label}-exit`, 2, []); dispatcher(ctx);
  assert.equal(ctx.api.start_loaded_image(0x70000, 1), 0); assert.deepEqual(arena(ctx).subarray(0, 56), stateRecord(cpu(entry)));
  assert.deepEqual(readBytes(ctx, 0x70fe0, 32), new Uint8Array(32));
  return {ctx, label, base, entry, main, keeper, gate, receipt, text, data, keep, old: [], cycles: []};
}
function ram(path, counter, final = false) {
  const {ctx, base, entry, text, data, keep} = path, expectedData = data.slice(), view = new DataView(expectedData.buffer);
  view.setUint32(0, counter, true); view.setUint32(4, final ? MARKER : 0, true);
  assert.deepEqual(readBytes(ctx, entry, 4096), text, 'same-byte guest code store changes version but preserves literal .text bytes');
  assert.deepEqual(readBytes(ctx, base + 0x3000, 4096), expectedData, 'declared data page has only the exact counter/result changes');
  assert.deepEqual(readBytes(ctx, base + 0x5000, 4096), keep, 'separate current keeper page remains identical');
  const stack = new Uint8Array(32), stackView = new DataView(stack.buffer);
  if (final) { stackView.setUint32(24, entry + 43, true); stackView.setUint32(28, MARKER, true); }
  assert.deepEqual(readBytes(ctx, 0x70fe0, 32), stack, 'real PUSH/CALL stack and untouched declared bytes');
  return {text: hash(text), data: hash(expectedData), keeper: hash(keep), stack: hash(stack)};
}
for (const base of [0x400000, 0x500000]) {
  const path = load(base), {ctx, entry, keeper, gate} = path;
  const retire = (unit, low = ctx.low, high = ctx.high, idHigh = unit.high) => ctx.api.retire_stale_resident(low, high, unit.low, idHigh);
  pure(ctx, () => retire(keeper), 7, 'current unit cannot retire');
  pure(ctx, () => ctx.api.retire_stale_resident(ctx.low, ctx.high, 0, 0), 3, 'zero identity');
  pure(ctx, () => retire(keeper, ctx.low ^ 1), 3, 'full low key checked');
  pure(ctx, () => retire(keeper, ctx.low, ctx.high ^ 1), 3, 'full high key checked');
  pure(ctx, () => retire(keeper, ctx.low, ctx.high, keeper.high ^ 1), 3, 'full high unit identity checked');
  for (let cycle = 1; cycle <= 9; cycle++) {
    const old = path.main, count = sliceCounts[cycle - 1];
    dispatch(ctx, old, count, cpu(entry + 16, cycle === 1 ? 0 : MARKER, 0x71000, invalidatedFlags[cycle - 1]), 6, 2);
    const ramHashes = ram(path, cycle); guard(ctx, old, 4, 'committed same-byte store leaves old unit stale');
    pure(ctx, () => old.run(0xffffffff, 0xffffffff, 0, 0xffffffff), 4, 'stale saved run rejects before malformed pointers');
    if (cycle === 1) {
      refresh(ctx).view.setUint32(ctx.base + 96, 1, true);
      pure(ctx, () => retire(old), 16, 'stale eligibility precedes cancellation');
      pure(ctx, () => retire(keeper), 7, 'current eligibility precedes cancellation');
      pure(ctx, () => ctx.api.retire_stale_resident(ctx.low, ctx.high, 0, 0), 3, 'unknown identity precedes cancellation');
      refresh(ctx).view.setUint32(ctx.base + 96, 0, true);
    }
    stable(ctx, keeper);
    // synchronous dispatcher.run has fully returned: this is the trusted host quiescence boundary.
    assert.equal(ctx.table.get(0), old.run); pure(ctx, () => retire(old), 0, 'retire exact stale id at host return');
    retirements++; ctx.live.delete(old.id); assert.equal(ctx.live.size, 2);
    assert.equal(ctx.table.get(0), old.run, 'engine retirement does not mutate the host Table');
    ctx.table.set(0, null); assert.equal(ctx.table.get(0), null); path.old.push(old);
    // old.pointer/old.length are descriptive expired coordinates only from this line onward.
    pure(ctx, () => retire(old), 3, 'double retirement refuses without mutation');
    pure(ctx, () => old.run(0xffffffff, 0xffffffff, 0, 0xffffffff), 3, 'retired saved run rejects before pointers and budget');
    path.main = compile(ctx, [entry + 16, entry, entry + 30], [], `${path.label}-main-${cycle}`, 0, ['read32', 'store_resident32']); recompilations++;
    pure(ctx, () => old.run(0xffffffff, 0xffffffff, 0, 0xffffffff), 3, 'fresh same slot cannot resurrect old immutable id');
    assert.equal(ctx.live.size, 3); assert.equal(ctx.table.get(1), keeper.run); assert.equal(ctx.table.get(2), gate.run); stable(ctx, keeper);
    assert.deepEqual(ram(path, cycle), ramHashes, 'retire/recompile/install leaves declared RAM unchanged');
    path.cycles.push({cycle, retired: count, next_pc: entry + 16, counter: cycle, flags: invalidatedFlags[cycle - 1], retired_id: old.id, fresh_id: path.main.id, slot: 0, active_units: ctx.live.size, ram_sha256: ramHashes});
  }
  // deliberately retain a finite set of old functions for refusal proofs; no JS GC claim.
  for (const old of path.old) pure(ctx, () => old.run(0xffffffff, 0xffffffff, 0, 0xffffffff), 3, 'all nine old functions remain retired after all reinstallations');
  dispatch(ctx, gate, 6, cpu(EXIT, MARKER, 0x70ff8, 0x46), 8, 3, EXIT_ID, 2);
  const finalRam = ram(path, 9, true), captureBefore = arena(ctx);
  const frame = record('R3CF', 112, [1, EXIT_ID, 2, 1, EXIT, 0x70ff8, entry + 43, 0, MARKER, ...Array(15).fill(0)]);
  assert.equal(ctx.api.capture_resident_call(ctx.low, ctx.high, gate.low, gate.high, 2, 1), 0); captures++;
  const captureExpected = captureBefore.slice(); captureExpected.set(frame, TRANSFER); assert.deepEqual(arena(ctx), captureExpected, 'literal real CALL capture');
  pure(ctx, () => retire(keeper), 12, 'owned pending call blocks current retirement');
  pure(ctx, () => ctx.api.retire_stale_resident(ctx.low, ctx.high, 0, 0), 12, 'owned pending call blocks unknown retirement');
  const completeExpected = arena(ctx); completeExpected.set(exitRecord(9, 0, 4, MARKER), 56);
  assert.equal(ctx.api.complete_resident_windows_call(ctx.low, ctx.high, gate.low, gate.high, 1), 0); completions++;
  assert.deepEqual(arena(ctx), completeExpected, 'named ExitProcess terminal CPU/arena oracle');
  pure(ctx, () => retire(keeper, ctx.low ^ 1), 21, 'terminal lifecycle wins before identity');
  pure(ctx, () => ctx.dispatcher.run(0xffffffff, 0xffffffff, 0, 0xffffffff), 21, 'retained dispatcher rejects terminal process before pointers');
  pure(ctx, () => ctx.api.close(), 0, 'close preserves whole arena');
  pure(ctx, () => retire(keeper, ctx.low ^ 1), 5, 'Closed lifecycle wins before identity');
  paths.push({base, entry, receipt_hex: Buffer.from(path.receipt).toString('hex'), cycles: path.cycles, retired: 48, counter: 9, result: MARKER, after_call_canary: 0, final_flags: 0x46, cumulative_unique_ids: Number(ctx.nextId), maximum_active_units: 3, reused_slot: 0, retained_old_functions: path.old.length, final_ram_sha256: finalRam, keeper: {id: keeper.id, pointer: keeper.pointer, length: keeper.length, sha256: hash(keeper.bytes)}, dispatcher: {pointer: ctx.dispatcher.pointer, length: ctx.dispatcher.length, sha256: hash(ctx.dispatcher.bytes)}});
}
const counts = {paths: paths.length, invalidation_cycles: retirements, successful_retirements: retirements, cold_recompilations: recompilations, guest_retired: retired, captures, provider_completions: completions, child_modules: modules.filter(row => !row.label.endsWith('-dispatcher')).length, dispatcher_modules: modules.filter(row => row.label.endsWith('-dispatcher')).length, retained_old_functions: paths.reduce((sum, row) => sum + row.retained_old_functions, 0)};
assert.deepEqual(counts, {paths: 2, invalidation_cycles: 18, successful_retirements: 18, cold_recompilations: 18, guest_retired: 96, captures: 2, provider_completions: 2, child_modules: 24, dispatcher_modules: 2, retained_old_functions: 18}, 'authored-path counts frozen before runtime');
assert.equal(controls.length, 190, 'per path:5 initial +9*8 cycle +3 cancel +9 final-old +2 Busy +4 lifecycle controls');
const sourcePaths = ['engine/src/cpu/dbt/resident.rs', 'engine/src/cpu/dbt/artifact.rs', 'engine/src/cpu/dbt/cold.rs', 'engine/src/cpu/dbt/wasm/emitter.rs', 'engine/src/cpu/dbt/wasm/dispatcher.rs', 'engine/src/cpu/dbt/wasm/memory.rs', 'engine/src/cpu/dbt/wasm/integer.rs', 'engine/src/cpu/x86/decode/decoder.rs', 'engine/src/cpu/x86/decode/integer.rs', 'engine/src/cpu/x86/decode/flow.rs', 'engine/src/process/resident.rs', 'engine/src/process/installation.rs', 'engine/src/process/instance.rs', 'engine/src/process/wasm.rs', 'engine/src/process/call.rs', 'engine/src/process/windows.rs', 'engine/src/process/image.rs', 'engine/src/process/image_input.rs', 'engine/src/abi/wasm/exports.rs', 'engine/src/loader/mod.rs', 'engine/src/loader/pe32.rs', 'engine/src/loader/imports.rs', 'engine/src/loader/relocation.rs', 'engine/src/memory/space.rs', 'engine/tests/support/pe32.rs', 'engine/tests/process_resident_retirement_wasm.rs', 'engine/tests/fixtures/p2-resident-retirement/run.mjs'];
const result = {counts, paths, controls, modules, engine_sha256: hash(engineBytes), pe_sha256: hash(image), sources: Object.fromEntries(sourcePaths.map(path => [path, hash(readFileSync(join(root, path)))])), oracle: {program, cfg_seeds: seeds, relocation_offsets: relocationOffsets, invalidated_flags: invalidatedFlags, slice_counts: sliceCounts, child_imports: childImports(['read32', 'store_resident32']), dispatcher_imports: dispatcherImports}, limitations: 'Synchronous trusted host quiescence after generated run returns; exact engine stale-unit retirement and one active Table slot reused. Native tests separately prove retained byte credit and full8 capacity. Finite18 old JS functions are intentionally retained for refusal tests; no GC, allocator shrink, concurrent/reentrant retirement, arbitrary Table safety, browser/SDK/performance/game claim. Raw retired pointers are never dereferenced. RAM scope is full declared text/data/keeper pages plus32 stack bytes. Source hashes refer to current frozen repository files, not saved source copies.'};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2));
console.log(JSON.stringify({counts, controls: controls.length, engine_sha256: result.engine_sha256, output}));
