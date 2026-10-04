import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(root, 'expected engine wasm, output directory and repository root');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const engineBytes = readFileSync(enginePath), engineModule = new WebAssembly.Module(engineBytes);
assert.deepEqual(WebAssembly.Module.imports(engineModule), []);
const arenaSize = 4236, transfer = 140, controls = [], paths = [], modules = [];
const apiNames = ['open', 'close', 'arena_ptr', 'protect', 'upload', 'read32', 'store_resident32', 'compile_resident_entries', 'generation', 'guard_resident', 'acknowledge_resident_installation', 'begin_image_input', 'append_image_input', 'load_pe32_linked_v4_input_at', 'start_loaded_image', 'capture_resident_call', 'complete_resident_call', 'complete_resident_windows_call', 'begin_resident_callback', 'authorize_resident_callback', 'select_resident_callback_unit', 'finish_resident_callback', 'capture_active_resident_callback_call', 'complete_active_resident_callback_windows_call'];
let ordinal = 0;
let guestRetired = 0, captures = 0, providerCompletions = 0, finishes = 0;

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
function fresh(owner, pages = 12) {
  const instance = new WebAssembly.Instance(engineModule, {});
  const api = Object.fromEntries(apiNames.map(name => [name, instance.exports[`ring3_abi_v1_${name}`]]));
  for (const [name, fn] of Object.entries(api)) assert.equal(typeof fn, 'function', name);
  const key = 0xa360000000000000n + BigInt(++ordinal);
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
function compile(ctx, entries, gates, label, slot) {
  assert.ok(entries.length <= 8); request(ctx, words([...entries, ...gates.flat()]));
  const before = snapshot(ctx);
  assert.equal(ctx.api.compile_resident_entries(entries.length, gates.length), 0); refresh(ctx);
  const at = ctx.base + transfer;
  const fields = Array.from({length: 6}, (_, index) => ctx.view.getUint32(at + index * 4, true));
  assert.equal(fields[0], 1); assert.equal(fields[1], 24);
  const [, , low, high, pointer, length] = fields;
  assert.ok(low !== 0 || high !== 0); assert.ok(pointer > 0 && length > 8 && pointer + length <= ctx.bytes.length);
  const expected = before.slice(); expected.set(words(fields), transfer);
  assert.deepEqual(snapshot(ctx), expected, 'resident receipt only');
  const unit = instantiate(ctx, ctx.bytes.slice(pointer, pointer + length), label, {low, high, pointer, length, entries, gates});
  ctx.table.set(slot, unit.run); const ackBefore = snapshot(ctx);
  assert.equal(ctx.api.acknowledge_resident_installation(ctx.low, ctx.high, low, high, slot), 0);
  const ackExpected = ackBefore.slice(); ackExpected.set(record('R3IN', 32, [low, high, slot, 0]), transfer);
  assert.deepEqual(snapshot(ctx), ackExpected); assert.equal(ctx.table.get(slot), unit.run);
  return unit;
}
const OUTER = 0x80000011, RETURN = 0x80000012, ERROR = 0xf1234567, RESULT = 0xc8c00005;
const outerProgram = '68674523f1ff1564314000e8f000000089c289150030400052ff1568314000c70504304000a5a5a5a50f0b';
const bodyProgram = '6a04680030000068011000006a00ff156c31400089c78b0f8b9f0010000009d9c78700100000e0ac68248b870010000035e0ac682409c883f8000f850c000000ff1560314000b80500c0c8c3b8dec0addec3';
const bodySeeds = [0, 20, 64, 70, 76], bodyCounts = [5, 11, 2];
const images = Object.fromEntries(['home', 'selected'].map(mode => [mode, readFileSync(join(output, `${mode}.exe`))]));
for (const [mode, image] of Object.entries(images)) {
  const bodyOffset = mode === 'home' ? 0x200 : 0x400;
  assert.equal(image.length, 3584); assert.equal(image.subarray(0, 2).toString(), 'MZ');
  assert.equal(image.subarray(0x200, 0x22b).toString('hex'), outerProgram);
  assert.equal(image.subarray(0x200 + bodyOffset, 0x200 + bodyOffset + 82).toString('hex'), bodyProgram);
  assert.equal(image.readUInt16LE(0x300), 0x0b0f); assert.equal(image.readUInt16LE(0x500), 0x0b0f);
  if (mode === 'selected') assert.equal(image.subarray(0x400, 0x405).toString('hex'), 'e9fb010000');
  const branch = 0x200 + bodyOffset + 58;
  assert.equal(image.readUInt16LE(branch), 0x850f); assert.equal(58 + 6 + image.readInt32LE(branch + 2), 76);
  assert.ok(bodySeeds.includes(64) && bodySeeds.includes(76), 'both conditional successor starts are explicit');
}
function cpu(pc, esp, eax = 0, flags = 2, edi = 0, edx = 0) {
  return {registers: [eax, 0, edx, 0, esp, 0, 0, edi], pc, flags};
}
const stateRecord = value => record('R3ST', 56, [...value.registers, value.pc, value.flags]);
const exitRecord = (reason, retired = 0, detail = 0, version = 3) => record('R3EX', 40, [reason, retired, detail, 0, 0, 0], version);
function read(ctx, address) {
  assert.equal(ctx.api.read32(address), 0); refresh(ctx);
  assert.equal(ctx.view.getUint32(ctx.base + 116, true), 0, `read ${address.toString(16)}`);
  return ctx.view.getUint32(ctx.base + 120, true);
}
function unmapped(ctx, address) {
  assert.equal(ctx.api.read32(address), 0);
  assert.deepEqual(snapshot(ctx).subarray(100, 140), record('R3MH', 40, [1, 0, 1, address, 1, 4]));
}
function pure(ctx, fn, expected, name) {
  const before = snapshot(ctx); assert.equal(fn(), expected, name); assert.deepEqual(snapshot(ctx), before, `${name}: whole arena preserved`);
  controls.push({path: ctx.label, mode: ctx.owner, name, status: expected});
}
function guard(ctx, unit, expected) {
  pure(ctx, () => ctx.api.guard_resident(ctx.low, ctx.high, unit.low, unit.high, ctx.base, ctx.base + 56, ctx.base + 96), expected, 'live owner guard');
}
function load(base, mode, label) {
  const ctx = fresh(mode), image = images[mode]; ctx.label = label; ctx.table = new WebAssembly.Table({element: 'anyfunc', initial: 8, maximum: 8});
  assert.equal(ctx.api.complete_active_resident_callback_windows_call.length, 6);
  assert.equal(ctx.api.begin_image_input(image.length), 0); request(ctx, image); assert.equal(ctx.api.append_image_input(0, image.length), 0);
  assert.equal(ctx.api.load_pe32_linked_v4_input_at(base, 0x8000), 0); refresh(ctx);
  const receipt = ctx.bytes.slice(ctx.base + transfer, ctx.base + transfer + 88), view = new DataView(receipt.buffer);
  assert.equal(Buffer.from(receipt.subarray(0, 4)).toString(), 'R3LI'); assert.equal(view.getUint32(4, true), 0x10004); assert.equal(view.getUint32(8, true), 88);
  assert.deepEqual(Array.from({length: 8}, (_, index) => view.getUint32(16 + index * 4, true)), [base, 0x6000, base + 0x1000, 5, 0x8000, 4, 0, 0]);
  const gates = Array.from({length: 5}, (_, index) => [view.getUint32(48 + index * 8, true), view.getUint32(52 + index * 8, true)]);
  assert.deepEqual(gates, [[0x8000, 0x10001], [0x8010, 0x10002], [0x8020, 0x10003], [0x8040, 0x10005], [0, 0]]);
  assert.deepEqual([0, 4, 8, 12].map(offset => read(ctx, base + 0x3160 + offset)), [0x8000, 0x8010, 0x8020, 0x8040], 'named IAT imports resolved by Rust');
  const entry = base + 0x1000, homeEntry = base + 0x1200, returnGate = base + 0x1300, body = mode === 'home' ? homeEntry : base + 0x1400;
  for (const [offset, target] of [[7, base + 0x3164], [20, base + 0x3000], [27, base + 0x3168], [33, base + 0x3004]]) assert.equal(read(ctx, entry + offset), target);
  assert.equal(read(ctx, body + 16), base + 0x316c); assert.equal(read(ctx, body + 66), base + 0x3160);
  const caller = compile(ctx, [entry, entry + 11, entry + 16], [], `${label}-caller`, 0);
  const outer = compile(ctx, [base + 0x1100, 0x8010, 0x8020], [[base + 0x1100, OUTER], gates[1], gates[2]], `${label}-outer-gates`, 1);
  const callbackGates = [gates[0], gates[3]], callbackEntries = bodySeeds.map(offset => body + offset);
  let home, active;
  if (mode === 'home') {
    home = compile(ctx, [...callbackEntries, returnGate, 0x8000, 0x8040], [[returnGate, RETURN], ...callbackGates], `${label}-home-active`, 2); active = home;
  } else {
    home = compile(ctx, [homeEntry, returnGate], [[returnGate, RETURN]], `${label}-home`, 2);
    active = compile(ctx, [...callbackEntries, 0x8000, 0x8040], callbackGates, `${label}-selected`, 3);
  }
  assert.equal(ctx.api.start_loaded_image(0x70000, 1), 0); assert.deepEqual(state(ctx), cpu(entry, 0x71000));
  return {ctx, base, entry, homeEntry, returnGate, body, caller, outer, home, active, label, receipt};
}
function execute(path, unit, budget, expected, reason, retired, detail = 0, version = 3, helperValue) {
  const ctx = path.ctx, expectedArena = snapshot(ctx);
  expectedArena.set(stateRecord(expected), 0); expectedArena.set(exitRecord(reason, retired, detail, version), 56);
  if (helperValue !== undefined) expectedArena.set(record('R3MH', 40, [0, helperValue, 0, 0, 0, 0]), 100);
  assert.equal(unit.run(ctx.base, ctx.base + 56, budget, ctx.base + 96), 0);
  assert.deepEqual(state(ctx), expected, `${path.label}: literal execution state`);
  assert.deepEqual(snapshot(ctx).subarray(56, 96), exitRecord(reason, retired, detail, version));
  assert.deepEqual(snapshot(ctx), expectedArena, `${path.label}: all arena bytes obey the literal execution contract`);
  guestRetired += refresh(ctx).view.getUint32(ctx.base + 76, true);
}
function callRecord(path, unit, token, id, convention, args, returnPc, active = false) {
  const {ctx} = path, gateState = state(ctx), before = snapshot(ctx);
  const status = active ? ctx.api.capture_active_resident_callback_call(ctx.low, ctx.high, unit.low, unit.high, 3, convention, args.length)
    : ctx.api.capture_resident_call(ctx.low, ctx.high, unit.low, unit.high, convention, args.length);
  assert.equal(status, 0); captures++;
  const fields = [token, id, convention, args.length, gateState.pc, gateState.registers[4], returnPc, 0, ...args, ...Array(16 - args.length).fill(0)];
  const expected = before.slice(); expected.set(record('R3CF', 112, fields), transfer);
  assert.deepEqual(snapshot(ctx), expected, 'literal owned CallFrame token, arguments and return PC');
  assert.equal(read(ctx, gateState.registers[4]), returnPc, 'real guest CALL wrote its return PC');
  args.forEach((argument, index) => assert.equal(read(ctx, gateState.registers[4] + 4 + index * 4), argument, 'real PUSH order'));
}
function callbackComplete(path, token, keyLow = path.ctx.low, active = path.active, callbackToken = 3) {
  const ctx = path.ctx;
  return ctx.api.complete_active_resident_callback_windows_call(keyLow, ctx.high, active.low, active.high, callbackToken, token);
}
function prescribedCompletion(path, unit, token, expectedState, active = false, terminal = false) {
  const {ctx, base} = path;
  const addresses = [base + 0x3000, base + 0x3004, ...Array.from({length: 8}, (_, index) => 0x70fe0 + index * 4)];
  const ram = addresses.map(address => read(ctx, address)), before = snapshot(ctx), expected = before.slice();
  if (terminal) expected.set(exitRecord(9, 0, RESULT, 4), 56);
  else { expected.set(stateRecord(expectedState), 0); expected.set(exitRecord(3), 56); }
  assert.equal(active ? callbackComplete(path, token) : ctx.api.complete_resident_windows_call(ctx.low, ctx.high, unit.low, unit.high, token), 0);
  providerCompletions++; assert.deepEqual(snapshot(ctx), expected, 'provider publishes only prescribed state/exit fields');
  assert.deepEqual(addresses.map(address => read(ctx, address)), ram, 'provider preserves declared existing guest RAM');
}
function begin(path) {
  const {ctx, caller, outer, entry, base, home, homeEntry, returnGate} = path;
  execute(path, caller, 64, cpu(0x8010, 0x70ff8), 3, 2, 0, 2, 0);
  execute(path, outer, 1, cpu(0x8010, 0x70ff8), 8, 0, 0x10002);
  callRecord(path, outer, 1, 0x10002, 2, [ERROR], entry + 11);
  prescribedCompletion(path, outer, 1, cpu(entry + 11, 0x71000));
  execute(path, caller, 64, cpu(base + 0x1100, 0x70ffc), 3, 1, 0, 2, 0);
  execute(path, outer, 1, cpu(base + 0x1100, 0x70ffc), 8, 0, OUTER);
  callRecord(path, outer, 2, OUTER, 1, [], entry + 16);
  path.frozenOuter = snapshot(ctx).slice(0, 96);
  const before = snapshot(ctx), expected = before.slice();
  assert.equal(ctx.api.begin_resident_callback(ctx.low, ctx.high, outer.low, outer.high, home.low, home.high, 2, homeEntry, returnGate, RETURN, 0), 0);
  expected.set(stateRecord(cpu(homeEntry, 0x70ff8)), 0); expected.set(exitRecord(3), 56);
  expected.set(record('R3RC', 72, [3, 2, 1, 0, homeEntry, 0x70ff8, returnGate, RETURN, 0, 0, outer.low, outer.high, home.low, home.high]), transfer);
  assert.deepEqual(snapshot(ctx), expected); assert.equal(read(ctx, 0x70ff8), returnGate, 'callback frame committed its return word');
  pure(ctx, () => ctx.api.authorize_resident_callback(ctx.low, ctx.high, home.low, home.high, 3), 0, 'authorize immutable home');
  if (ctx.owner === 'selected') {
    execute(path, home, 64, cpu(path.body, 0x70ff8), 3, 1);
    pure(ctx, () => ctx.api.select_resident_callback_unit(ctx.low, ctx.high, home.low, home.high, 3, path.active.low, path.active.high), 0, 'select installed continuation');
    guard(ctx, home, 12);
  }
  guard(ctx, path.active, 0);
  execute(path, path.active, 64, cpu(0x8040, 0x70fe4), 8, bodyCounts[0], 0x10005, 3, 0);
  callRecord(path, path.active, 4, 0x10005, 2, [0, 4097, 0x3000, 4], path.body + 20, true);
}
function pendingControls(path) {
  const {ctx, active, outer} = path;
  pure(ctx, () => callbackComplete(path, 4, ctx.low ^ 1), 3, 'wrong key');
  pure(ctx, () => callbackComplete(path, 4, ctx.low, active, 4), 14, 'wrong callback token');
  pure(ctx, () => callbackComplete(path, 0), 14, 'wrong inner token');
  pure(ctx, () => callbackComplete(path, 2), 12, 'retained outer token protected');
  pure(ctx, () => callbackComplete(path, 4, ctx.low, outer), 12, 'foreign active owner');
  pure(ctx, () => ctx.api.complete_resident_windows_call(ctx.low, ctx.high, active.low, active.high, 4), 14, 'ordinary provider cannot consume callback inner');
  refresh(ctx).view.setUint32(ctx.base + 96, 1, true);
  refresh(ctx).bytes[ctx.base + 52] ^= 0x40;
  pure(ctx, () => callbackComplete(path, 4), 15, 'state mismatch before cancel'); refresh(ctx).bytes[ctx.base + 52] ^= 0x40;
  pure(ctx, () => callbackComplete(path, 0), 14, 'token before cancel');
  pure(ctx, () => callbackComplete(path, 4), 16, 'cancel before allocation'); refresh(ctx).view.setUint32(ctx.base + 96, 0, true);
  guard(ctx, active, 12); unmapped(ctx, 0x10000000);
}
for (const base of [0x400000, 0x500000]) for (const mode of ['home', 'selected']) {
  const path = load(base, mode, `pe-${mode}-${base.toString(16)}`), {ctx, active, home, outer, caller, body, returnGate, entry} = path;
  const retiredBefore = guestRetired; begin(path); pendingControls(path);
  prescribedCompletion(path, active, 4, cpu(body + 20, 0x70ff8, 0x10000000), true);
  // independent full8KiB oracle runs immediately after the callback provider commits allocation.
  for (let word = 0; word < 2048; word++) assert.equal(read(ctx, 0x10000000 + word * 4), 0, 'every newly allocated word is zero');
  unmapped(ctx, 0x10002000);
  pure(ctx, () => callbackComplete(path, 4), 14, 'consumed VA token cannot replay'); guard(ctx, active, 0);
  execute(path, active, 64, cpu(0x8000, 0x70ff4, 0, 0x46, 0x10000000), 8, bodyCounts[1], 0x10001, 3, 0);
  assert.equal(read(ctx, 0x10001000), 0x2468ace0, 'genuine guest checked tail store/load');
  callRecord(path, active, 5, 0x10001, 2, [], body + 70, true);
  prescribedCompletion(path, active, 5, cpu(body + 70, 0x70ff8, ERROR, 0x46, 0x10000000), true);
  assert.equal(state(ctx).registers[0], ERROR, 'callback GetLastError preserves the nonzero value seeded by real outer SetLastError');
  execute(path, active, 64, cpu(returnGate, 0x70ffc, RESULT, 0x46, 0x10000000), mode === 'home' ? 8 : 3, bodyCounts[2], mode === 'home' ? RETURN : 0, 3, returnGate);
  assert.equal(read(ctx, 0x70ff8), returnGate, 'genuine RET consumed the committed callback return word');
  if (mode === 'selected') {
    pure(ctx, () => ctx.api.finish_resident_callback(ctx.low, ctx.high, home.low, home.high, 3), 12, 'finish cannot bypass selected owner');
    pure(ctx, () => ctx.api.select_resident_callback_unit(ctx.low, ctx.high, home.low, home.high, 3, home.low, home.high), 0, 'activate immutable home return Gate');
    execute(path, home, 1, cpu(returnGate, 0x70ffc, RESULT, 0x46, 0x10000000), 8, 0, RETURN);
  }
  const finishBefore = snapshot(ctx), finishExpected = finishBefore.slice();
  assert.equal(ctx.api.finish_resident_callback(ctx.low, ctx.high, home.low, home.high, 3), 0); finishes++;
  finishExpected.set(path.frozenOuter, 0); finishExpected.set(record('R3RR', 48, [3, 2, RESULT, 0, outer.low, outer.high, home.low, home.high]), transfer);
  assert.deepEqual(snapshot(ctx), finishExpected, 'finish restores the immutable actual outer State/Exit byte-for-byte');
  assert.deepEqual(state(ctx), cpu(base + 0x1100, 0x70ffc));
  pure(ctx, () => callbackComplete(path, 5), 14, 'consumed callback cannot complete');
  const outerBefore = snapshot(ctx), outerExpected = outerBefore.slice();
  assert.equal(ctx.api.complete_resident_call(ctx.low, ctx.high, outer.low, outer.high, 2, RESULT), 0);
  outerExpected.set(stateRecord(cpu(entry + 16, 0x71000, RESULT)), 0); outerExpected.set(exitRecord(3), 56); assert.deepEqual(snapshot(ctx), outerExpected);
  execute(path, caller, 64, cpu(0x8020, 0x70ff8, RESULT, 2, 0, RESULT), 3, 4, 0, 2, 0);
  execute(path, outer, 1, cpu(0x8020, 0x70ff8, RESULT, 2, 0, RESULT), 8, 0, 0x10003);
  callRecord(path, outer, 6, 0x10003, 2, [RESULT], entry + 31);
  prescribedCompletion(path, outer, 6, state(ctx), false, true);
  assert.equal(read(ctx, base + 0x3000), RESULT); assert.equal(read(ctx, base + 0x3004), 0, 'after-call canary stays zero');
  assert.equal(caller.run(0xffffffff, 0xffffffff, 0, 0xffffffff), 21);
  const retired = guestRetired - retiredBefore; assert.equal(retired, mode === 'home' ? 25 : 26);
  paths.push({base, mode, guest_retired: retired, callback_va: {size: 4097, address: 0x10000000, rounded_pages: 2, zero_words: 2048}, callback_last_error: ERROR, callback_return: RESULT, outer_exit: RESULT, canary: 0, receipt_sha256: hash(path.receipt)});
  assert.equal(ctx.api.close(), 0); pure(ctx, () => callbackComplete(path, 0, ctx.low ^ 1), 5, 'Closed precedes identity and token');
}
for (const mode of ['home', 'selected']) {
  const path = load(0x400000, mode, `stale-${mode}`); begin(path);
  // .text is RX: host protect changes its version without relying on a denied byte write.
  const before = snapshot(path.ctx); assert.equal(path.ctx.api.protect(0x401000, 1, 7), 0); assert.deepEqual(snapshot(path.ctx), before);
  pure(path.ctx, () => callbackComplete(path, 4), 4, 'stale retained owner before allocation'); unmapped(path.ctx, 0x10000000);
  assert.equal(path.ctx.api.close(), 0); pure(path.ctx, () => callbackComplete(path, 0, path.ctx.low ^ 1), 5, 'Closed stale pending callback');
}
assert.equal(paths.length, 4); assert.equal(guestRetired, 119); assert.equal(captures, 26); assert.equal(providerCompletions, 18); assert.equal(finishes, 4);
assert.equal(controls.length, 82); assert.equal(modules.length, 21);
const sourcePaths = ['Cargo.toml', 'Cargo.lock', 'engine/Cargo.toml', 'engine/src/windows/provider.rs', 'engine/src/windows/calling_convention.rs', 'engine/src/windows/callback_frame.rs', 'engine/src/process/windows.rs', 'engine/src/process/call.rs', 'engine/src/process/resident_callback.rs', 'engine/src/process/resident.rs', 'engine/src/process/instance.rs', 'engine/src/process/wasm.rs', 'engine/src/abi/wasm/exports.rs', 'engine/src/loader/mod.rs', 'engine/src/loader/pe32.rs', 'engine/src/loader/imports.rs', 'engine/src/memory/space.rs', 'engine/src/cpu/dbt/wasm/control.rs', 'engine/tests/windows_callback_provider_wasm.rs', 'engine/tests/fixtures/p2-callback-windows/run.mjs', 'engine/tests/support/pe32.rs'];
const result = {status: 'ok', engine_sha256: hash(engineBytes), source_sha256: Object.fromEntries(sourcePaths.map(path => [path, hash(readFileSync(join(root, path)))])), authored_pe_sha256: Object.fromEntries(Object.entries(images).map(([mode, image]) => [mode, hash(image)])), tools: {node: process.version, v8: process.versions.v8}, counts: {complete_paths: paths.length, complete_guest_retired: paths.reduce((count, path) => count + path.guest_retired, 0), total_guest_retired: guestRetired, call_captures: captures, provider_completions: providerCompletions, finishes, control_probes: controls.length, generated_modules: modules.length}, paths, controls, modules, claim: 'four authored copied-PE direct resident home/selected callback paths; generic private host trigger, real Windows provider VA/GLE, genuine RET, immutable outer restoration and named ExitProcess; two partial stale/Closed paths; no Windows API invokes this callback, no dispatcher, nesting, full Win32, browser, SDK, performance or game claim'};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2));
console.log(JSON.stringify({status: result.status, counts: result.counts, engine_sha256: result.engine_sha256, output}));
