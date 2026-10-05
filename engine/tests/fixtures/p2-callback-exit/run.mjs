import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {existsSync, readFileSync, realpathSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(root, 'expected engine wasm, unique output directory and repository root');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const artifacts = {}, paths = [], trace = [], controls = [], modules = [];
const sourcePaths = ['Cargo.lock', 'engine/Cargo.toml', 'engine/src/process/windows.rs', 'engine/src/process/call.rs', 'engine/src/process/callback.rs', 'engine/src/process/resident_callback.rs', 'engine/src/process/resident.rs', 'engine/src/process/installation.rs', 'engine/src/process/instance.rs', 'engine/src/process/startup.rs', 'engine/src/process/wasm.rs', 'engine/src/abi/wasm/exports.rs', 'engine/src/windows/provider.rs', 'engine/src/windows/callback_frame.rs', 'engine/src/loader/imports.rs', 'engine/src/loader/relocation.rs', 'engine/src/loader/pe32.rs', 'engine/src/cpu/dbt/region.rs', 'engine/src/cpu/dbt/wasm/integer.rs', 'engine/src/cpu/dbt/wasm/emitter.rs', 'engine/src/cpu/dbt/wasm/dispatcher.rs', 'engine/src/cpu/dbt/wasm/memory.rs', 'engine/src/cpu/dbt/wasm/abi.rs', 'engine/tests/support/pe32.rs', 'engine/tests/windows_callback_exit_wasm.rs', 'engine/tests/fixtures/p2-callback-exit/run.mjs'];
const sourceMap = () => Object.fromEntries(sourcePaths.map(path => {
  const absolute = realpathSync(join(root, path)), bytes = readFileSync(absolute);
  return [path, {absolute, bytes: bytes.length, sha256: hash(bytes)}];
}));
function save(name, bytes) {
  assert.ok(!existsSync(join(output, name)), `original artifact already exists: ${name}`);
  writeFileSync(join(output, name), bytes); artifacts[name] = {bytes: bytes.length, sha256: hash(bytes)};
}
const sourceBefore = sourceMap();
save('source-before.json', Buffer.from(JSON.stringify(sourceBefore, null, 2)));
save('producer-run.mjs', readFileSync(join(root, 'engine/tests/fixtures/p2-callback-exit/run.mjs')));
save('producer-wrapper.rs', readFileSync(join(root, 'engine/tests/windows_callback_exit_wasm.rs')));
const engineBytes = new Uint8Array(readFileSync(enginePath)); save('engine.wasm', engineBytes);
const engineModule = new WebAssembly.Module(engineBytes), exports = WebAssembly.Module.exports(engineModule);
assert.deepEqual(WebAssembly.Module.imports(engineModule), []);
assert.equal(exports.length, 70); assert.equal(exports.filter(row => row.kind === 'function').length, 69);
const apiArities = {open: 3, close: 0, arena_ptr: 0, begin_image_input: 1, append_image_input: 2, load_pe32_linked_v4_input_at: 2, start_loaded_image: 2, read32: 1, compile_resident_entries: 2, acknowledge_resident_installation: 5, dispatcher_module: 2, guard_dispatch_entry: 5, find_installed_resident: 3, guard_resident: 7, capture_resident_call: 6, complete_resident_windows_call: 5, begin_resident_callback: 11, authorize_resident_callback: 5, select_resident_callback_unit: 7, capture_active_resident_callback_call: 7, complete_active_resident_callback_windows_call: 6, complete_active_resident_callback_call: 7, finish_resident_callback: 5, abort_callback: 3, complete_resident_call: 6, generation: 0, module_ptr: 0, module_len: 0, resident_module: 2, store_resident32: 6};
const SIZE = 4236, TRANSFER = 140, OUTER = 0x80000011, RETURN = 0x80000012, ERROR = 0xf1234567, CODE = 0xffffffff;
const outerHex = '68674523f1ff1564314000e8f0000000c7050030400011111111ebfe';
const bodyHex = 'ff15603140003598badc0e50ff1568314000c7050430400022222222c3';
const words = fields => { const bytes = new Uint8Array(fields.length * 4), view = new DataView(bytes.buffer); fields.forEach((value, i) => view.setUint32(i * 4, value, true)); return bytes; };
function record(magic, length, fields, version = 1) {
  const bytes = new Uint8Array(length), view = new DataView(bytes.buffer); bytes.set(Buffer.from(magic));
  view.setUint16(4, version, true); view.setUint16(6, 1, true); view.setUint32(8, length, true); bytes.set(words(fields), 16); return bytes;
}
const cpu = (pc, esp, eax = 0, flags = 2) => ({registers: [eax, 0, 0, 0, esp, 0, 0, 0], pc, flags});
const stateRecord = state => { assert.equal(state.flags & 2, 2); assert.equal((state.flags & ~0xcd7) >>> 0, 0); return record('R3ST', 56, [...state.registers, state.pc, state.flags]); };
const exitRecord = (reason = 3, retired = 0, detail = 0, version = 3) => record('R3EX', 40, [reason, retired, detail, 0, 0, 0], version);
const helperRecord = value => record('R3MH', 40, [0, value, 0, 0, 0, 0]);
const hex = bytes => Buffer.from(bytes).toString('hex');
function refresh(ctx) { ctx.bytes = new Uint8Array(ctx.memory.buffer); ctx.view = new DataView(ctx.memory.buffer); return ctx; }
function arena(ctx) { return refresh(ctx).bytes.slice(ctx.base, ctx.base + SIZE); }
function state(ctx) { refresh(ctx); return {registers: Array.from({length: 8}, (_, i) => ctx.view.getUint32(ctx.base + 16 + i * 4, true)), pc: ctx.view.getUint32(ctx.base + 48, true), flags: ctx.view.getUint32(ctx.base + 52, true)}; }
function request(ctx, bytes) { assert.ok(bytes.length <= 4096); refresh(ctx).bytes.set(bytes, ctx.base + TRANSFER); }
function pure(ctx, action, status, name) {
  const before = arena(ctx); assert.equal(action(), status, `${ctx.label}/${name}`); assert.deepEqual(arena(ctx), before, `${name}: whole arena preserved`);
  controls.push({path: ctx.label, name, status});
}
function read(ctx, address) {
  const before = arena(ctx); assert.equal(ctx.api.read32(address), 0); refresh(ctx);
  const value = ctx.view.getUint32(ctx.base + 120, true); before.set(helperRecord(value), 100);
  assert.deepEqual(arena(ctx), before, 'read32 changes only its successful helper record'); return value;
}
function page(ctx, address) { const bytes = new Uint8Array(4096), view = new DataView(bytes.buffer); for (let i = 0; i < 1024; i++) view.setUint32(i * 4, read(ctx, address + i * 4), true); return bytes; }
function child(ctx, bytes, label, helpers, table = false) {
  assert.ok(bytes.length <= 65536); const module = new WebAssembly.Module(bytes);
  const imports = [{module: 'env', name: 'memory', kind: 'memory'}, ...(table ? [{module: 'env', name: 'table', kind: 'table'}] : []), ...helpers.map(name => ({module: 'ring3', name, kind: 'function'}))];
  assert.deepEqual(WebAssembly.Module.imports(module), imports);
  const instance = new WebAssembly.Instance(module, {env: {memory: ctx.memory, table: ctx.table}, ring3: ctx.api}); assert.equal(instance.exports.run.length, 4);
  save(`${label}.wasm`, bytes); modules.push({path: ctx.label, artifact: `${label}.wasm`, imports, run_arity: 4}); return instance.exports.run;
}
function compile(ctx, entries, gates, suffix, slot, memory) {
  assert.ok(entries.length <= 8 && gates.length <= 5); request(ctx, words([...entries, ...gates.flat()])); const before = arena(ctx);
  assert.equal(ctx.api.compile_resident_entries(entries.length, gates.length), 0); refresh(ctx);
  const fields = Array.from({length: 6}, (_, i) => ctx.view.getUint32(ctx.base + TRANSFER + i * 4, true));
  assert.deepEqual(fields.slice(0, 2), [1, 24]); const [, , low, high, pointer, length] = fields;
  assert.ok((low !== 0 || high !== 0) && pointer > 0 && length > 8 && length <= 65536 && pointer + length <= ctx.bytes.length);
  before.set(words(fields), TRANSFER); assert.deepEqual(arena(ctx), before);
  const bytes = ctx.bytes.slice(pointer, pointer + length), run = child(ctx, bytes, `${ctx.label}-${suffix}`, memory ? ['guard_resident', 'read32', 'store_resident32'] : ['guard_resident']);
  const unit = {low, high, pointer, length, bytes, run, slot, suffix}; ctx.units.push(unit); assert.ok(ctx.units.length <= 8);
  ctx.table.set(slot, run); const ack = arena(ctx); assert.equal(ctx.api.acknowledge_resident_installation(ctx.low, ctx.high, low, high, slot), 0);
  ack.set(record('R3IN', 32, [low, high, slot, 0]), TRANSFER); assert.deepEqual(arena(ctx), ack); return unit;
}
function load(base, mode, ordinal) {
  const label = `${mode}-${base.toString(16)}`, imageName = `pe-${label}.exe`, image = new Uint8Array(readFileSync(join(output, imageName)));
  artifacts[imageName] = {bytes: image.length, sha256: hash(image)}; const raw = new DataView(image.buffer);
  assert.equal(image.length, 3584); assert.equal(hex(image.subarray(0x200, 0x21c)), outerHex);
  const offset = mode === 'home' ? 0x200 : 0x400;
  assert.equal(hex(image.subarray(0x200 + offset, 0x200 + offset + 29)), bodyHex);
  assert.equal(raw.getUint32(0x3c, true), 0x80); assert.equal(raw.getUint16(0x300, true), 0x0b0f); assert.equal(raw.getUint16(0x500, true), 0x0b0f);
  if (mode === 'selected') assert.equal(hex(image.subarray(0x400, 0x405)), 'e9fb010000');
  assert.deepEqual(Array.from({length: 6}, (_, i) => raw.getUint16(0xc08 + i * 2, true)), [0x3007, 0x3012, 0x3000 | offset + 2, 0x3000 | offset + 14, 0x3000 | offset + 20, 0]);
  const instance = new WebAssembly.Instance(engineModule, {}), api = {};
  for (const [name, arity] of Object.entries(apiArities)) { api[name] = instance.exports[`ring3_abi_v1_${name}`]; assert.equal(typeof api[name], 'function', name); assert.equal(api[name].length, arity, name); }
  const ctx = {label, mode, image, low: ordinal, high: 0xa3780000, api, memory: instance.exports.memory, units: [], table: new WebAssembly.Table({element: 'anyfunc', initial: 8, maximum: 8})};
  assert.equal(api.open(8, ctx.low, ctx.high), 0); ctx.base = api.arena_ptr() >>> 0; refresh(ctx);
  assert.equal(api.begin_image_input(image.length), 0); request(ctx, image); assert.equal(api.append_image_input(0, image.length), 0);
  assert.equal(api.load_pe32_linked_v4_input_at(base, 0x8000), 0); const receipt = arena(ctx).slice(TRANSFER, TRANSFER + 88);
  assert.deepEqual(receipt, record('R3LI', 88, [base, 0x6000, base + 0x1000, 5, 0x8000, 3, 0, 0, 0x8000, 0x10001, 0x8010, 0x10002, 0x8020, 0x10003, 0, 0, 0, 0], 4)); save(`${label}-linked-receipt.bin`, receipt);
  assert.deepEqual([0, 4, 8, 12].map(i => read(ctx, base + 0x3160 + i)), [0x8000, 0x8010, 0x8020, 0]);
  const entry = base + 0x1000, homeEntry = base + 0x1200, returnGate = base + 0x1300, body = base + 0x1000 + offset;
  for (const [address, value] of [[entry + 7, base + 0x3164], [entry + 18, base + 0x3000], [body + 2, base + 0x3160], [body + 14, base + 0x3168], [body + 20, base + 0x3004]]) assert.equal(read(ctx, address), value, 'HIGHLOW operand resolved by Rust');
  const caller = compile(ctx, [entry, entry + 11, entry + 16], [], 'caller', 0, true);
  const outer = compile(ctx, [base + 0x1100, 0x8010], [[base + 0x1100, OUTER], [0x8010, 0x10002]], 'outer', 1, false);
  let home, active;
  if (mode === 'home') { home = compile(ctx, [body, body + 6, body + 18, returnGate, 0x8000, 0x8020], [[returnGate, RETURN], [0x8000, 0x10001], [0x8020, 0x10003]], 'home-active', 2, true); active = home; }
  else { home = compile(ctx, [homeEntry, returnGate], [[returnGate, RETURN]], 'home', 2, false); active = compile(ctx, [body, body + 6, body + 18, 0x8000, 0x8020], [[0x8000, 0x10001], [0x8020, 0x10003]], 'selected-active', 3, true); }
  const before = arena(ctx); assert.equal(api.dispatcher_module(ctx.low, ctx.high), 0); refresh(ctx);
  const pointer = ctx.view.getUint32(ctx.base + TRANSFER + 24, true), length = ctx.view.getUint32(ctx.base + TRANSFER + 28, true);
  before.set(record('R3DP', 32, [ctx.low, ctx.high, pointer, length]), TRANSFER); assert.deepEqual(arena(ctx), before); assert.ok(pointer > 0 && length > 8 && pointer + length <= ctx.bytes.length);
  const bytes = ctx.bytes.slice(pointer, pointer + length), dispatcher = {pointer, length, bytes, run: child(ctx, bytes, `${label}-dispatcher`, ['guard_dispatch_entry', 'find_installed_resident'], true)};
  assert.equal(api.start_loaded_image(0x70000, 1), 0); assert.deepEqual(state(ctx), cpu(entry, 0x71000)); save(`${label}-startup.arena.bin`, arena(ctx));
  return {ctx, base, mode, label, entry, homeEntry, returnGate, body, caller, outer, home, active, dispatcher};
}
function execute(path, unit, stage, expectedState, reason, retired, detail = 0, helper = false, version = 3) {
  const {ctx} = path, expected = arena(ctx); expected.set(stateRecord(expectedState), 0); expected.set(exitRecord(reason, retired, detail, version), 56); if (helper) expected.set(helperRecord(0), 100);
  assert.equal(unit.run(ctx.base, ctx.base + 56, 64, ctx.base + 96), 0, stage); assert.deepEqual(arena(ctx), expected, `${stage}: whole literal arena`);
  save(`${path.label}-${stage}.arena.bin`, arena(ctx)); trace.push({path: path.label, stage, retired, observed_state: state(ctx), observed_exit_hex: hex(arena(ctx).subarray(56, 96))});
}
function capture(path, unit, token, id, args, returnPc, active = false) {
  const {ctx} = path, at = state(ctx), expected = arena(ctx), convention = id === OUTER ? 1 : 2;
  assert.equal(active ? ctx.api.capture_active_resident_callback_call(ctx.low, ctx.high, unit.low, unit.high, 3, convention, args.length) : ctx.api.capture_resident_call(ctx.low, ctx.high, unit.low, unit.high, convention, args.length), 0);
  const call = record('R3CF', 112, [token, id, convention, args.length, at.pc, at.registers[4], returnPc, 0, ...args, ...Array(16 - args.length).fill(0)]);
  expected.set(call, TRANSFER); assert.deepEqual(arena(ctx), expected); save(`${path.label}-capture-${token}.bin`, arena(ctx).subarray(TRANSFER, TRANSFER + 112));
  assert.equal(read(ctx, at.registers[4]), returnPc); args.forEach((value, i) => assert.equal(read(ctx, at.registers[4] + 4 + i * 4), value));
}
function complete(path, unit, token, expectedState, active = false) {
  const {ctx} = path, expected = arena(ctx); expected.set(stateRecord(expectedState), 0); expected.set(exitRecord(), 56);
  assert.equal(active ? ctx.api.complete_active_resident_callback_windows_call(ctx.low, ctx.high, unit.low, unit.high, 3, token) : ctx.api.complete_resident_windows_call(ctx.low, ctx.high, unit.low, unit.high, token), 0);
  assert.deepEqual(arena(ctx), expected);
}
function retained(path) {
  const {ctx} = path; refresh(ctx);
  for (const unit of [...ctx.units, path.dispatcher]) assert.deepEqual(ctx.bytes.slice(unit.pointer, unit.pointer + unit.length), unit.bytes, 'retained generated code bytes');
  for (const unit of ctx.units) assert.equal(ctx.table.get(unit.slot), unit.run, 'retained table identity'); assert.equal(ctx.api.generation(), 0);
}
let ordinal = 0;
for (const base of [0x400000, 0x500000]) for (const mode of ['home', 'selected']) {
  const path = load(base, mode, ++ordinal), {ctx, caller, outer, home, active, entry, homeEntry, returnGate, body, label} = path;
  execute(path, caller, 'sle-call', cpu(0x8010, 0x70ff8), 3, 2, 0, true, 2); execute(path, outer, 'sle-gate', cpu(0x8010, 0x70ff8), 8, 0, 0x10002);
  capture(path, outer, 1, 0x10002, [ERROR], entry + 11); complete(path, outer, 1, cpu(entry + 11, 0x71000));
  execute(path, caller, 'outer-call', cpu(base + 0x1100, 0x70ffc), 3, 1, 0, true, 2); execute(path, outer, 'outer-gate', cpu(base + 0x1100, 0x70ffc), 8, 0, OUTER);
  capture(path, outer, 2, OUTER, [], entry + 16); const frozenOuter = arena(ctx).slice(0, 96); save(`${label}-suspended-outer.bin`, frozenOuter);
  const callback = arena(ctx); assert.equal(ctx.api.begin_resident_callback(ctx.low, ctx.high, outer.low, outer.high, home.low, home.high, 2, homeEntry, returnGate, RETURN, 0), 0);
  callback.set(stateRecord(cpu(homeEntry, 0x70ff8)), 0); callback.set(exitRecord(), 56); callback.set(record('R3RC', 72, [3, 2, 1, 0, homeEntry, 0x70ff8, returnGate, RETURN, 0, 0, outer.low, outer.high, home.low, home.high]), TRANSFER); assert.deepEqual(arena(ctx), callback); assert.equal(read(ctx, 0x70ff8), returnGate);
  pure(ctx, () => ctx.api.authorize_resident_callback(ctx.low, ctx.high, home.low, home.high, 3), 0, 'authorize immutable home');
  if (mode === 'selected') { execute(path, home, 'select-jump', cpu(body, 0x70ff8), 3, 1); pure(ctx, () => ctx.api.select_resident_callback_unit(ctx.low, ctx.high, home.low, home.high, 3, active.low, active.high), 0, 'select installed active'); }
  execute(path, active, 'gle-call', cpu(0x8000, 0x70ff4), 8, 1, 0x10001, true); execute(path, active, 'gle-gate', cpu(0x8000, 0x70ff4), 8, 0, 0x10001);
  capture(path, active, 4, 0x10001, [], body + 6, true); complete(path, active, 4, cpu(body + 6, 0x70ff8, ERROR), true);
  execute(path, active, 'exit-call', cpu(0x8020, 0x70ff0, CODE, 0x86), 8, 3, 0x10003, true); execute(path, active, 'exit-gate', cpu(0x8020, 0x70ff0, CODE, 0x86), 8, 0, 0x10003);
  capture(path, active, 5, 0x10003, [CODE], body + 18, true);
  const terminal = () => ctx.api.complete_active_resident_callback_windows_call(ctx.low, ctx.high, active.low, active.high, 3, 5);
  pure(ctx, () => ctx.api.complete_active_resident_callback_call(ctx.low, ctx.high, active.low, active.high, 3, 5, 44), 7, 'scalar ExitProcess refusal');
  pure(ctx, () => ctx.api.complete_resident_windows_call(ctx.low, ctx.high, active.low, active.high, 5), 14, 'ordinary wrapper cannot own callback inner');
  pure(ctx, () => ctx.api.complete_active_resident_callback_windows_call(ctx.low, ctx.high, active.low, active.high, 3, 2), 12, 'suspended outer token protected');
  refresh(ctx).view.setUint32(ctx.base + 96, 1, true); refresh(ctx).bytes[ctx.base + 52] ^= 0x40; pure(ctx, terminal, 15, 'frozen state before cancel'); refresh(ctx).bytes[ctx.base + 52] ^= 0x40;
  pure(ctx, terminal, 16, 'cancel preserves captured terminal call'); refresh(ctx).view.setUint32(ctx.base + 96, 0, true);
  // diagnostic tampering is a negative control; no positive cpu or guest stack is seeded.
  refresh(ctx).bytes.fill(0xa5, ctx.base + TRANSFER, ctx.base + TRANSFER + 112);
  const expectedData = new Uint8Array(4096); expectedData.set(ctx.image.subarray(0x800, 0xc00)); expectedData.set(words([0x8000, 0x8010, 0x8020]), 0x160);
  const expectedStack = new Uint8Array(4096); expectedStack.set(words([body + 18, CODE, returnGate, entry + 16]), 0xff0);
  const beforePages = [page(ctx, base + 0x3000), page(ctx, 0x70000)]; assert.deepEqual(beforePages, [expectedData, expectedStack]);
  beforePages.forEach((bytes, i) => save(`${label}-before-${i === 0 ? 'data' : 'stack'}.page.bin`, bytes));
  const before = arena(ctx), expected = before.slice(); save(`${label}-terminal-before.arena.bin`, before); expected.set(exitRecord(9, 0, CODE, 4), 56);
  assert.equal(terminal(), 0); assert.deepEqual(arena(ctx), expected); assert.deepEqual(state(ctx), cpu(0x8020, 0x70ff0, CODE, 0x86)); save(`${label}-terminal.arena.bin`, arena(ctx)); retained(path);
  const afterPages = [page(ctx, base + 0x3000), page(ctx, 0x70000)]; assert.deepEqual(afterPages, beforePages); afterPages.forEach((bytes, i) => save(`${label}-terminal-${i === 0 ? 'data' : 'stack'}.page.bin`, bytes));
  pure(ctx, terminal, 21, 'terminal completion replay'); pure(ctx, () => ctx.api.finish_resident_callback(ctx.low, ctx.high, home.low, home.high, 3), 21, 'terminal finish'); pure(ctx, () => ctx.api.abort_callback(ctx.low, ctx.high, 3), 21, 'terminal abort'); pure(ctx, () => ctx.api.complete_resident_call(ctx.low, ctx.high, outer.low, outer.high, 2, 44), 21, 'terminal outer completion');
  pure(ctx, () => ctx.api.dispatcher_module(0, 0), 21, 'terminal dispatcher receipt'); pure(ctx, () => ctx.api.resident_module(0, 0), 21, 'terminal resident receipt'); pure(ctx, () => ctx.api.open(0, 0, 0), 6, 'retained allocation occupied'); assert.equal(ctx.api.arena_ptr() >>> 0, ctx.base); assert.equal(ctx.api.module_ptr(), 0); assert.equal(ctx.api.module_len(), 0);
  for (const unit of [...ctx.units, path.dispatcher]) {
    pure(ctx, () => unit.run(ctx.base, ctx.base + 56, 64, ctx.base + 96), 21, 'retained normal run'); refresh(ctx).view.setUint32(ctx.base + 96, 1, true);
    pure(ctx, () => unit.run(ctx.base, ctx.base + 56, 0, ctx.base + 96), 21, 'terminal before cancel and zero budget'); refresh(ctx).view.setUint32(ctx.base + 96, 0, true);
    pure(ctx, () => unit.run(0xffffffff, 0xffffffff, 0, 0xffffffff), 21, 'terminal before malformed pointers');
  }
  for (const [kind, next] of [['inner', cpu(body + 18, 0x70ff8, CODE, 0x86)], ['outer', cpu(entry + 16, 0x71000)]]) {
    // explicit postterminal public restoration challenges the private latch; it is never startup.
    refresh(ctx).bytes.set(stateRecord(next), ctx.base); refresh(ctx).bytes.set(exitRecord(), ctx.base + 56); refresh(ctx).view.setUint32(ctx.base + 96, 0, true);
    for (const unit of [...ctx.units, path.dispatcher]) pure(ctx, () => unit.run(ctx.base, ctx.base + 56, 1, ctx.base + 96), 21, `${kind} restored canary remains inert`);
    save(`${label}-restored-${kind}.arena.bin`, arena(ctx)); retained(path);
  }
  const finalPages = [page(ctx, base + 0x3000), page(ctx, 0x70000)]; assert.deepEqual(finalPages, beforePages); finalPages.forEach((bytes, i) => save(`${label}-final-${i === 0 ? 'data' : 'stack'}.page.bin`, bytes));
  const closeArena = arena(ctx); assert.equal(ctx.api.close(), 0); assert.deepEqual(arena(ctx), closeArena); save(`${label}-closed.arena.bin`, arena(ctx));
  pure(ctx, () => ctx.api.complete_active_resident_callback_windows_call(0, 0, 0, 0, 0, 0), 5, 'Closed before owner and token'); pure(ctx, () => ctx.api.read32(base + 0x3000), 5, 'Closed diagnostic read');
  for (const unit of [...ctx.units, path.dispatcher]) pure(ctx, () => unit.run(0xffffffff, 0xffffffff, 0, 0xffffffff), 5, 'retained run Closed before pointers');
  const retired = trace.filter(row => row.path === label).reduce((sum, row) => sum + row.retired, 0); assert.equal(retired, mode === 'home' ? 7 : 8);
  paths.push({label, base, mode, guest_retired: retired, exit_code: CODE, terminal_fresh_retired: 0, real_inner_return_pc: body + 18, real_outer_return_pc: entry + 16, callback_return_gate: returnGate, saved_terminal_arena: `${label}-terminal.arena.bin`, canaries: [0, 0], whole_ram_pages: [base + 0x3000, 0x70000]});
}
assert.equal(paths.length, 4); assert.equal(trace.length, 34); assert.equal(trace.reduce((sum, row) => sum + row.retired, 0), 30); assert.equal(modules.length, 18); assert.equal(controls.length, 170);
const sourceAfter = sourceMap(); assert.deepEqual(sourceAfter, sourceBefore); save('source-after.json', Buffer.from(JSON.stringify(sourceAfter, null, 2)));
const result = {status: 'ok', argv: process.argv, engine_input: {path: realpathSync(enginePath), bytes: engineBytes.length, sha256: hash(engineBytes)}, source_before: sourceBefore, source_after: sourceAfter, counts: {paths: paths.length, generated_positive_runs: trace.length, guest_retired: 30, generated_modules: modules.length, control_probes: controls.length}, paths, trace, controls, modules, artifacts, tools: {node: process.version, v8: process.versions.v8}, claim: 'four newly authored copied PE paths; Rust loader/import/relocation/startup authority, outer named SetLastError and generic CALL, callback named GetLastError/ExitProcess, no positive CPU/stack patch, captured ExitProcess terminal0 with exact state/whole4236 arena and complete data/stack pages; inner/outer/retained dispatcher canaries inert through public restore and close. Generic host trigger is not provider-driven Win32 callback evidence; no thread/DLL cleanup, general Win32/ISA, browser/SDK/performance/game/P2 completion.'};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2));
console.log(JSON.stringify({status: result.status, counts: result.counts, engine_sha256: hash(engineBytes), output}));
