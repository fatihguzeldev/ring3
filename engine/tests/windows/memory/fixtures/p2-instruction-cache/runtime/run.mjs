import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {execFileSync} from 'node:child_process';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../../../../../support/wasm/engine.mjs';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(root && output);
const engine = readEngine(enginePath), SIZE = 4612, TRANSFER = 140;
const BASE = 0x500000, ENTRY = BASE + 0x1000, GATE = 0x8000, STACK = 0x70000, TOP = STACK + 4096, ALLOC = 0x10000000;
const initial = readFileSync(join(output, 'initial-arena.bin')); assert.equal(initial.length, SIZE);
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const artifacts = {}, providerWrites = [], lifecycle = [], contexts = [], modules = [], observations = [], helperTrace = [], diagnostics = [];
const save = (name, bytes) => { writeFileSync(join(output, name), bytes, {flag: 'wx'}); artifacts[name] = {bytes: Buffer.byteLength(bytes), sha256: hash(bytes)}; };
const program = Buffer.from(
  'f9fd6a0468003000006a406a00ff1580314000' +
  '89c7c707b82a0000c7470400c3cccc68003040006a206a0657ff1584314000' +
  '6a06576affff1588314000' +
  'ffd7' +
  '89c36a06576affff1588314000' +
  'ffd7' +
  '89c668003040006a406a0657ff1584314000' +
  '6a06576affff1588314000' +
  'c707b82a0000' +
  '6a06576affff1588314000' +
  '68008000006a0057ff158c314000' +
  '6a06576affff1588314000' +
  '68ffffffff6a006affff1588314000' +
  '53ff1590314000' +
  'c7050c304000a5a5a5a50f0b', 'hex');
const functionBytes = Buffer.from('b82a000000c3cccc', 'hex');
const gates = [[GATE + 32, 0x10003], [GATE + 64, 0x10005], [GATE + 80, 0x10006], [GATE + 96, 0x10007], [GATE + 112, 0x10008]];
const fixups = [[15, 12672], [35, 12288], [46, 12676], [57, 12680], [72, 12680], [81, 12288], [92, 12676], [103, 12680], [120, 12680], [134, 12684], [145, 12680], [160, 12680], [167, 12688], [173, 12300]];
assert.equal(program.length, 183); assert.equal(functionBytes.length, 8);
for (const [offset, rva] of fixups) assert.equal(program.readUInt32LE(offset), 0x400000 + rva);
assert.deepEqual([program.readUInt32LE(23), program.readUInt32LE(30)], [functionBytes.readUInt32LE(0), functionBytes.readUInt32LE(4)]);
function authoredPe() {
  const bytes = Buffer.alloc(2560), put16 = (at, value) => bytes.writeUInt16LE(value, at), put32 = (at, value) => bytes.writeUInt32LE(value, at);
  bytes.write('MZ'); put32(0x3c, 0x80); bytes.write('PE\0\0', 0x80);
  put16(0x84, 0x14c); put16(0x86, 3); put16(0x94, 224); put16(0x96, 0x102); put16(0x98, 0x10b);
  for (const [field, value] of [[4, 512], [8, 1536], [12, 0], [16, 0x1000], [20, 0x1000], [24, 0x3000], [28, 0x400000], [32, 4096], [36, 512], [56, 0x6000], [60, 512], [72, 0x100000], [76, 4096], [80, 0x100000], [84, 4096], [92, 16]]) put32(0x98 + field, value);
  put16(0x98 + 68, 3); put16(0x98 + 70, 0x100);
  for (const [index, rva, size] of [[1, 0x3100, 40], [5, 0x5000, 36], [12, 0x3180, 24]]) { put32(0x98 + 96 + index * 8, rva); put32(0x98 + 100 + index * 8, size); }
  [['.text', 183, 0x1000, 512, 0x200, 0x60000020], ['.data', 1024, 0x3000, 1024, 0x400, 0xc0000040], ['.fixups', 36, 0x5000, 512, 0x800, 0x40000040]].forEach(([name, size, rva, rawSize, raw, flags], index) => {
    const at = 0x178 + index * 40; bytes.write(name, at); for (const [field, value] of [[8, size], [12, rva], [16, rawSize], [20, raw], [36, flags]]) put32(at + field, value);
  });
  bytes.fill(0xcc, 0x200, 0x400); bytes.set(program, 0x200); put32(0x400, 0xa5a5a5a5); put32(0x40c, 0xfacecafe);
  [0x3140, 0, 0, 0x31b0, 0x3180].forEach((value, i) => put32(0x500 + i * 4, value));
  for (const at of [0x540, 0x580]) [0x31c0, 0x31e0, 0x3200, 0x3220, 0x3240].forEach((value, i) => put32(at + i * 4, value));
  bytes.write('KeRnEl32.dLl', 0x5b0);
  for (const [at, hint, name] of [[0x5c0, 1, 'VirtualAlloc'], [0x5e0, 2, 'VirtualProtect'], [0x600, 3, 'FlushInstructionCache'], [0x620, 4, 'VirtualFree'], [0x640, 5, 'ExitProcess']]) { put16(at, hint); bytes.write(name, at + 2); }
  put32(0x800, 0x1000); put32(0x804, 36); fixups.map(([offset]) => 0x3000 | offset).forEach((value, i) => put16(0x808 + i * 2, value));
  return bytes;
}
const image = readFileSync(join(output, 'instruction-cache.exe')); assert.deepEqual(image, authoredPe());
save('program.bin', program); save('guest-function.bin', functionBytes); save('engine.wasm', engine.bytes);
const sourcePaths = [...execFileSync('rg', ['--files', 'engine/src'], {cwd: root, encoding: 'utf8'}).trim().split('\n'), 'Cargo.toml', 'Cargo.lock', 'engine/Cargo.toml', 'rust-toolchain.toml', 'engine/tests/support/loader/pe32/image.rs', 'engine/tests/loader/imports/native/v7.rs', 'engine/tests/loader/imports/native.rs', 'engine/tests/windows/memory/wasm/flush.rs', 'engine/tests/windows/memory/wasm.rs', 'engine/tests/support/wasm.rs', 'engine/tests/windows/memory/fixtures/p2-instruction-cache/runtime/run.mjs', 'engine/tests/support/wasm/engine.mjs'].sort();
const sourceMap = () => Object.fromEntries(sourcePaths.map(path => { const bytes = readFileSync(join(root, path)); return [path, {bytes: bytes.length, sha256: hash(bytes)}]; }));
const sourcesBefore = sourceMap();
function words(values) { const bytes = Buffer.alloc(values.length * 4); values.forEach((value, i) => bytes.writeUInt32LE(value >>> 0, i * 4)); return bytes; }
function record(magic, length, fields, version = 1) { const bytes = Buffer.alloc(length); bytes.write(magic); bytes.writeUInt16LE(version, 4); bytes.writeUInt16LE(1, 6); bytes.writeUInt32LE(length, 8); bytes.set(words(fields), 16); return bytes; }
const stateBytes = state => record('R3ST', 56, [...state.registers, state.pc, state.flags]);
const exitBytes = stop => record('R3EX', 40, [stop.reason, stop.retired, stop.detail ?? 0, 0, 0, 0], stop.version);
const helperBytes = (value = 0, faultAddress = null) => record('R3MH', 40, faultAddress === null ? [0, value, 0, 0, 0, 0] : [1, 0, 2, faultAddress, 1, 4]);
const installBytes = unit => record('R3IN', 32, [unit.low, unit.high, unit.slot, 0]);
const R = (eax = 0, ecx = 0, edx = 0, ebx = 0, esp = TOP, ebp = 0, esi = 0, edi = 0) => [eax, ecx, edx, ebx, esp, ebp, esi, edi];
function arena(e) { return Buffer.from(new Uint8Array(e.memory.buffer, e.base, SIZE)); }
function cpu(e) { const bytes = arena(e); return {registers: Array.from({length: 8}, (_, i) => bytes.readUInt32LE(16 + i * 4)), pc: bytes.readUInt32LE(48), flags: bytes.readUInt32LE(52)}; }
function exit(e) { const bytes = arena(e); return {version: bytes.readUInt16LE(60), reason: bytes.readUInt32LE(72), retired: bytes.readUInt32LE(76), detail: bytes.readUInt32LE(80)}; }
function unchanged(before, after, spans, label) { for (let i = 0; i < SIZE; i++) if (!spans.some(([start, end]) => i >= start && i < end)) assert.equal(after[i], before[i], `${label}: arena byte ${i}`); }
function apiCall(e, name, args, spans = [], expected = 0) { const before = arena(e), status = e.api[name](...args); assert.equal(status, expected, name); unchanged(before, arena(e), spans, name); return status; }
function transfer(e, bytes) { assert.ok(bytes.length <= 4096); new Uint8Array(e.memory.buffer).set(bytes, e.base + TRANSFER); }
const arities = {open: 3, close: 0, arena_ptr: 0, begin_image_input: 1, append_image_input: 2, load_pe32_linked_v7_input_at: 2, start_loaded_image: 2, compile_entries: 2, compile_resident_entries: 2, generation: 0, module_ptr: 0, module_len: 0, guard: 6, guard_resident: 7, read32: 1, store32: 2, store_resident32: 6, capture_call: 5, capture_resident_call: 6, complete_windows_call: 4, complete_resident_windows_call: 5, acknowledge_resident_installation: 5, dispatcher_module: 2, guard_dispatch_entry: 5, find_installed_resident: 3, discard_installed_resident: 5, retire_stale_resident: 4};
function fresh(owner) {
  const instance = new WebAssembly.Instance(engine.module, {}), api = Object.fromEntries(Object.entries(arities).map(([name, arity]) => { const fn = instance.exports[`ring3_abi_v1_${name}`]; assert.equal(typeof fn, 'function', name); assert.equal(fn.length, arity, name); return [name, fn]; }));
  const e = {owner, label: owner, api, memory: instance.exports.memory, low: 0x53800001 + contexts.length, high: 0xa5380000, units: [], compilations: 0, tokens: 0, calls: 0, retired: 0, diagnostics: 0, table: owner === 'resident' ? new WebAssembly.Table({element: 'anyfunc', initial: 8, maximum: 8}) : null};
  assert.equal(api.open(8, e.low, e.high), 0); e.base = api.arena_ptr() >>> 0; contexts.push(e);
  apiCall(e, 'begin_image_input', [image.length]); transfer(e, image); apiCall(e, 'append_image_input', [0, image.length]); transfer(e, Buffer.alloc(4096, 0xa5));
  apiCall(e, 'load_pe32_linked_v7_input_at', [BASE, GATE], [[TRANSFER, TRANSFER + 112]]);
  assert.deepEqual(arena(e).subarray(TRANSFER, TRANSFER + 112), record('R3LI', 112, [BASE, 0x6000, ENTRY, 5, GATE, 5, 0, 0, ...gates.flat(), ...Array(6).fill(0)], 7));
  save(`${e.label}-linked-v7-receipt.bin`, arena(e).subarray(TRANSFER, TRANSFER + 112)); save(`${e.label}-after-linked-v7-load-arena.bin`, arena(e));
  apiCall(e, 'start_loaded_image', [STACK, 1], [[16, 96]]); assert.deepEqual(cpu(e), {registers: R(), pc: ENTRY, flags: 2});
  const before = arena(e); new Uint8Array(e.memory.buffer).set(initial.subarray(4236), e.base + 4236); unchanged(before, arena(e), [[4236, SIZE]], 'dirty fp only after startup');
  if (e.table) {
    apiCall(e, 'dispatcher_module', [e.low, e.high], [[TRANSFER, TRANSFER + 32]]); const receipt = arena(e).subarray(TRANSFER, TRANSFER + 32);
    assert.equal(receipt.subarray(0, 4).toString(), 'R3DP'); assert.equal(receipt.readUInt32LE(4), 0x10001); assert.equal(receipt.readUInt32LE(8), 32); assert.deepEqual([receipt.readUInt32LE(16), receipt.readUInt32LE(20)], [e.low, e.high]);
    const bytes = Buffer.from(new Uint8Array(e.memory.buffer, receipt.readUInt32LE(24), receipt.readUInt32LE(28))), module = new WebAssembly.Module(bytes), file = `${e.label}-dispatcher.wasm`;
    assert.deepEqual(WebAssembly.Module.imports(module), [{module: 'env', name: 'memory', kind: 'memory'}, {module: 'env', name: 'table', kind: 'table'}, ...['guard_dispatch_entry', 'find_installed_resident'].map(name => ({module: 'ring3', name, kind: 'function'}))]);
    e.dispatcher = new WebAssembly.Instance(module, {env: {memory: e.memory, table: e.table}, ring3: e.api}).exports.run; assert.equal(e.dispatcher.length, 4); save(file, bytes); modules.push({context: e.label, name: 'dispatcher', file, bytes: bytes.length, sha256: hash(bytes)});
  }
  save(`${e.label}-started-arena.bin`, arena(e)); return e;
}
function compile(e, name, pcs, gateSpecs = [], instructions = 1, slot = 0) {
  assert.ok(pcs.length <= 8 && instructions <= 64); assert.ok(++e.compilations <= (e.table ? 7 : 5), 'bounded captures'); transfer(e, words([...pcs, ...gateSpecs.flat()]));
  apiCall(e, e.table ? 'compile_resident_entries' : 'compile_entries', [pcs.length, gateSpecs.length], e.table ? [[TRANSFER, TRANSFER + 24]] : []);
  const receipt = arena(e).subarray(TRANSFER, TRANSFER + 24), unit = {name, slot, pcs, instructions};
  if (e.table) { assert.equal(receipt.readUInt32LE(0), 1); assert.equal(receipt.readUInt32LE(4), 24); [unit.low, unit.high, unit.pointer, unit.length] = Array.from({length: 4}, (_, i) => receipt.readUInt32LE(8 + i * 4)); }
  else { unit.generation = e.api.generation(); unit.pointer = e.api.module_ptr() >>> 0; unit.length = e.api.module_len() >>> 0; assert.equal(unit.generation, e.compilations); }
  assert.ok(unit.pointer > 0 && unit.length > 8 && unit.length <= 65536 && unit.pointer + unit.length <= e.memory.buffer.byteLength);
  const bytes = Buffer.from(new Uint8Array(e.memory.buffer, unit.pointer, unit.length)), module = new WebAssembly.Module(bytes), imports = WebAssembly.Module.imports(module), ring3 = {};
  const guard = e.table ? 'guard_resident' : 'guard', store = e.table ? 'store_resident32' : 'store32';
  const helperNames = name.startsWith('dynamic-leaf') ? [guard, 'read32'] : [guard, 'read32', store];
  assert.deepEqual(imports, [{module: 'env', name: 'memory', kind: 'memory'}, ...helperNames.map(name => ({module: 'ring3', name, kind: 'function'}))]);
  assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  ring3[guard] = e.api[guard];
  for (const name of ['read32', store]) ring3[name] = (...args) => {
    const step = e.pendingSteps.shift(); assert.ok(step, `unexpected ${name}`); assert.equal(name === 'read32' ? 'read' : 'store', step[0]);
    const address = args[name === 'store_resident32' ? 4 : 0] >>> 0; assert.equal(address, step[1]);
    if (name !== 'read32') assert.equal(args[name === 'store_resident32' ? 5 : 1] >>> 0, step[2]);
    if (name === 'store_resident32') assert.deepEqual(args.slice(0, 4).map(x => x >>> 0), [e.low, e.high, unit.low, unit.high]);
    const before = arena(e), status = e.api[name](...args), after = arena(e); assert.equal(status, step[3] ?? 0); unchanged(before, after, [[100, 140]], `forwarded ${name}`);
    assert.deepEqual(after.subarray(100, 140), helperBytes(name === 'read32' ? step[2] : 0));
    helperTrace.push({context: e.label, call: e.calls + 1, unit: unit.name, name, args: args.map(x => x >>> 0), status, helper_hex: after.subarray(100, 140).toString('hex')}); return status;
  };
  unit.run = new WebAssembly.Instance(module, {env: {memory: e.memory}, ring3}).exports.run; assert.equal(unit.run.length, 4);
  if (e.table) { assert.equal(e.table.get(slot), null); e.table.set(slot, unit.run); apiCall(e, 'acknowledge_resident_installation', [e.low, e.high, unit.low, unit.high, slot], [[TRANSFER, TRANSFER + 32]]); assert.deepEqual(arena(e).subarray(TRANSFER, TRANSFER + 32), installBytes(unit)); }
  const file = `${e.label}-${e.compilations}-${name}.wasm`; save(file, bytes); e.units.push(unit); unit.active = true; e.peakUnits = Math.max(e.peakUnits ?? 0, e.units.filter(u => u.active).length); if (e.table) assert.ok(e.peakUnits <= 3); modules.push({context: e.label, name, file, generation: unit.generation ?? null, id_low: unit.low ?? null, id_high: unit.high ?? null, slot: e.table ? slot : null, entries: pcs, instructions, bytes: bytes.length, sha256: hash(bytes), imports}); return unit;
}
function run(e, unit, label, budget, state, stop, steps = [], selected = unit) {
  const before = arena(e), expected = Buffer.from(before); expected.set(stateBytes(state), 0); expected.set(exitBytes(stop), 56);
  if (steps.length) expected.set(helperBytes(steps.at(-1)[0] === 'read' ? steps.at(-1)[2] : 0), 100);
  if (e.table && budget && before.readUInt32LE(96) === 0) expected.set(installBytes(selected), TRANSFER);
  e.pendingSteps = [...steps]; const status = (e.table ? e.dispatcher : unit.run)(e.base, e.base + 56, budget, e.base + 96); assert.equal(status, 0, label); assert.equal(e.pendingSteps.length, 0, label); e.calls++; e.retired += stop.retired;
  const actual = arena(e); assert.deepEqual(actual, expected, `${e.label}/${label}: full arena`); assert.deepEqual(cpu(e), state); assert.deepEqual(exit(e), {...stop, detail: stop.detail ?? 0});
  const file = `${e.label}-${String(e.calls).padStart(2, '0')}-${label}-arena.bin`; save(file, actual);
  observations.push({context: e.label, name: label, budget, module: unit.name, expected: {status: 0, ...stop, pc: state.pc, flags: state.flags, registers: state.registers}, observed: {status, ...exit(e), pc: cpu(e).pc, flags: cpu(e).flags, registers: cpu(e).registers}, arena: file, sha256: hash(actual)});
}
function reject(e, unit, label, status) {
  const before = arena(e), actual = unit.run(e.base, e.base + 56, 64, e.base + 96); assert.equal(actual, status, label); assert.deepEqual(arena(e), before, 'rejected owner full arena neutral'); e.calls++;
  observations.push({context: e.label, name: label, expected: {status}, observed: {status: actual, version: null, reason: null, retired: null, pc: null, flags: null}, arena_sha256: hash(before)});
}
function guard(e, unit) { apiCall(e, e.table ? 'guard_resident' : 'guard', e.table ? [e.low, e.high, unit.low, unit.high, e.base, e.base + 56, e.base + 96] : [e.low, e.high, unit.generation, e.base, e.base + 56, e.base + 96]); }
function read(e, address, expected, fault = false) {
  const before = arena(e); assert.equal(e.api.read32(address), 0); const after = arena(e); unchanged(before, after, [[100, 140]], 'public diagnostic read'); assert.deepEqual(after.subarray(100, 140), fault ? record('R3MH', 40, [1, 0, fault, address, 1, 4]) : helperBytes(expected)); e.diagnostics++;
  diagnostics.push({context: e.label, name: 'read32', address, value: expected, fault}); return expected;
}
function checkCode(e, label) {
  const wanted = Buffer.alloc(32); wanted.set(functionBytes);
  const actual = Buffer.alloc(32);
  for (let i = 0; i < 32; i += 4) actual.writeUInt32LE(read(e, ALLOC + i, wanted.readUInt32LE(i)), i);
  assert.deepEqual(actual, wanted); read(e, ALLOC + 60, 0); read(e, ALLOC + 4092, 0); read(e, BASE + 0x300c, 0xfacecafe);
  save(`${e.label}-code-${label}-${e.diagnostics}.bin`, actual);
}
function complete(e, caller, id, args, returnPc, result, terminal = false, oldMode = null) {
  const before = arena(e), state = cpu(e), token = ++e.tokens, count = args.length;
  apiCall(e, e.table ? 'capture_resident_call' : 'capture_call', e.table ? [e.low, e.high, caller.low, caller.high, 2, count] : [e.low, e.high, caller.generation, 2, count], [[TRANSFER, TRANSFER + 112]]);
  assert.deepEqual(arena(e).subarray(TRANSFER, TRANSFER + 112), record('R3CF', 112, [token, id, 2, count, state.pc, state.registers[4], returnPc, 0, ...args, ...Array(16 - count).fill(0)]));
  save(`${e.label}-provider-${token}-before-capture.bin`, before);
  const beforeProvider = arena(e); save(`${e.label}-provider-${token}-before-complete.bin`, beforeProvider);
  const expected = Buffer.from(beforeProvider), next = {registers: [...state.registers], pc: returnPc, flags: state.flags};
  if (terminal) expected.set(exitBytes({version: 4, reason: 9, retired: 0, detail: result}), 56);
  else { next.registers[0] = result; next.registers[4] += 4 * (count + 1); expected.set(stateBytes(next), 0); expected.set(exitBytes({version: 3, reason: 3, retired: 0}), 56); }
  apiCall(e, e.table ? 'complete_resident_windows_call' : 'complete_windows_call', e.table ? [e.low, e.high, caller.low, caller.high, token] : [e.low, e.high, caller.generation, token], [[16, 96]]); assert.deepEqual(arena(e), expected, 'provider full arena');
  const afterProvider = arena(e); save(`${e.label}-provider-${token}-after-complete.bin`, afterProvider);
  if (oldMode !== null) { read(e, BASE + 0x3000, oldMode); read(e, BASE + 0x3004, 0); read(e, BASE + 0x300c, 0xfacecafe); providerWrites.push({context: e.label, token, address: BASE + 0x3000, bytes: 4, value: oldMode, source: 'provider transaction; not a forwarded guest helper'}); }
  observations.push({context: e.label, name: `provider-${token}`, api: id, args, token, expected: {result, terminal, registers: terminal ? state.registers : next.registers, pc: terminal ? state.pc : returnPc, flags: state.flags}, observed: {cpu: cpu(e), exit: exit(e)}, before_capture_sha256: hash(before), before_provider_sha256: hash(beforeProvider), after_provider_sha256: hash(afterProvider)});
}

const pushSteps = (esp, values) => values.map((value, i) => ['store', esp - 4 * (i + 1), value]);
const allocSteps = [...pushSteps(TOP, [4, 0x3000, 64, 0]), ['read', BASE + 0x3180, GATE + 64], ['store', TOP - 20, ENTRY + 19]];
const protectSteps = (mode, returnOffset) => [...pushSteps(TOP, [BASE + 0x3000, mode, 6, ALLOC]), ['read', BASE + 0x3184, GATE + 96], ['store', TOP - 20, ENTRY + returnOffset]];
const flushSteps = (returnOffset, whole = false) => [...pushSteps(TOP, [whole ? 0xffffffff : 6, whole ? 0 : ALLOC, 0xffffffff]), ['read', BASE + 0x3188, GATE + 112], ['store', TOP - 16, ENTRY + returnOffset]];
const freeSteps = [...pushSteps(TOP, [0x8000, 0, ALLOC]), ['read', BASE + 0x318c, GATE + 80], ['store', TOP - 16, ENTRY + 138]];
const exitSteps = [...pushSteps(TOP, [42]), ['read', BASE + 0x3190, GATE + 32], ['store', TOP - 8, ENTRY + 171]];
const at = offsets => offsets.map(offset => ENTRY + offset);
const S = (pc, eax = 1, ebx = 42, esi = 42, esp = TOP) => ({registers: R(eax, 0, 0, ebx, esp, 0, esi, ALLOC), pc, flags: 0x403});
function discard(e, unit) {
  assert.ok(e.table && unit.active); const before = arena(e); e.table.set(unit.slot, null);
  apiCall(e, 'discard_installed_resident', [e.low, e.high, unit.low, unit.high, unit.slot]); unit.active = false;
  lifecycle.push({context: e.label, action: 'discard-completed-caller', unit: unit.name, id_low: unit.low, id_high: unit.high, slot: unit.slot, arena_sha256: hash(before)});
}
function retire(e, unit) {
  assert.ok(e.table && unit.active); const before = arena(e); e.table.set(unit.slot, null);
  apiCall(e, 'retire_stale_resident', [e.low, e.high, unit.low, unit.high]); unit.active = false;
  lifecycle.push({context: e.label, action: 'retire-already-proved-stale-leaf', unit: unit.name, id_low: unit.low, id_high: unit.high, slot: unit.slot, arena_sha256: hash(before)});
}
function identity(e, unit, label) {
  guard(e, unit); const binding = {generation: unit.generation ?? null, id_low: unit.low ?? null, id_high: unit.high ?? null, pointer: unit.pointer, bytes: unit.length, slot: e.table ? unit.slot : null};
  if (e.table) assert.equal(e.table.get(unit.slot), unit.run);
  lifecycle.push({context: e.label, action: label, unit: unit.name, binding, arena_sha256: hash(arena(e))}); return binding;
}
function flush(e, caller, returnOffset, label, ebx = 42, esi = 42, eax = 1, whole = false) {
  run(e, caller, label, 64, S(GATE + 112, eax, ebx, esi, TOP - 16), {version: 3, reason: 8, retired: 4, detail: 0x10008}, flushSteps(returnOffset, whole));
  complete(e, caller, 0x10008, [0xffffffff, whole ? 0 : ALLOC, whole ? 0xffffffff : 6], ENTRY + returnOffset, 1);
}
function exercise(e) {
  let caller = compile(e, 'allocate-write-protect-flush', [...at([0, 19, 50]), GATE + 64, GATE + 96, GATE + 112], [gates[1], gates[3], gates[4]], 22);
  const start = cpu(e);
  run(e, caller, 'zero-budget', 0, start, {version: 3, reason: 1, retired: 0});
  new DataView(e.memory.buffer).setUint32(e.base + 96, 1, true);
  run(e, caller, 'cancel-before-zero', 0, start, {version: 3, reason: 2, retired: 0});
  new DataView(e.memory.buffer).setUint32(e.base + 96, 0, true);
  run(e, caller, 'allocate-rw', 64, {registers: R(0, 0, 0, 0, TOP - 20), pc: GATE + 64, flags: 0x403}, {version: 3, reason: 8, retired: 7, detail: 0x10005}, allocSteps);
  complete(e, caller, 0x10005, [0, 64, 0x3000, 4], ENTRY + 19, ALLOC);
  for (const offset of [0, 4, 60, 4092]) read(e, ALLOC + offset, 0);
  run(e, caller, 'guest-code-then-protect-rx', 64, S(GATE + 96, ALLOC, 0, 0, TOP - 20), {version: 3, reason: 8, retired: 8, detail: 0x10007}, [['store', ALLOC, functionBytes.readUInt32LE(0)], ['store', ALLOC + 4, functionBytes.readUInt32LE(4)], ...protectSteps(0x20, 50)]);
  checkCode(e, 'before-rx'); complete(e, caller, 0x10007, [ALLOC, 6, 0x20, BASE + 0x3000], ENTRY + 50, 1, false, 4);
  flush(e, caller, 61, 'named-flush-before-first-execution', 0, 0); checkCode(e, 'flushed-rx');
  if (e.table) discard(e, caller);
  caller = compile(e, 'two-calls-repeat-flush-protect', [...at([61, 63, 76, 78]), ...(e.table ? [] : [ALLOC]), GATE + 112, GATE + 96], [gates[4], gates[3]], e.table ? 15 : 17);
  let dynamic;
  if (e.table) {
    run(e, caller, 'cold-first-dynamic-call', 64, S(ALLOC, 1, 0, 0, TOP - 4), {version: 3, reason: 3, retired: 1}, [['store', TOP - 4, ENTRY + 63]]);
    dynamic = compile(e, 'dynamic-leaf-rx', [ALLOC], [], 2, 1);
    run(e, dynamic, 'first-natural-ret42', 2, S(ENTRY + 63, 42, 0, 0), {version: 2, reason: 1, retired: 2}, [['read', TOP - 4, ENTRY + 63]]);
  } else {
    dynamic = caller;
    run(e, caller, 'first-natural-call-ret42', 3, S(ENTRY + 63, 42, 0, 0), {version: 3, reason: 1, retired: 3}, [['store', TOP - 4, ENTRY + 63], ['read', TOP - 4, ENTRY + 63]]);
  }
  const retained = identity(e, dynamic, 'before-repeated-flush'); checkCode(e, 'before-repeat');
  run(e, caller, 'save-first-answer-and-repeat-flush', 64, S(GATE + 112, 42, 42, 0, TOP - 16), {version: 3, reason: 8, retired: 5, detail: 0x10008}, flushSteps(76));
  complete(e, caller, 0x10008, [0xffffffff, ALLOC, 6], ENTRY + 76, 1);
  assert.deepEqual(identity(e, dynamic, 'after-repeated-flush'), retained); checkCode(e, 'after-repeat');
  run(e, caller, 'second-natural-call-ret42', 3, S(ENTRY + 78, 42, 42, 0), {version: e.table ? 2 : 3, reason: 1, retired: 3}, [['store', TOP - 4, ENTRY + 78], ['read', TOP - 4, ENTRY + 78]], dynamic);
  run(e, caller, 'save-second-answer-and-protect-rwx', 64, S(GATE + 96, 42, 42, 42, TOP - 20), {version: 3, reason: 8, retired: 6, detail: 0x10007}, protectSteps(0x40, 96));
  complete(e, caller, 0x10007, [ALLOC, 6, 0x40, BASE + 0x3000], ENTRY + 96, 1, false, 0x20);
  reject(e, dynamic, 'protect-caused-source-stale-before-any-new-capture', 4);
  if (e.table) discard(e, caller);
  const protectVictim = dynamic;
  caller = compile(e, 'post-protect-write-free', [...at([96, 107, 113, 124]), ...(e.table ? [] : [ALLOC]), GATE + 112, GATE + 80], [gates[4], gates[2]], e.table ? 15 : 17);
  flush(e, caller, 107, 'flush-after-protect-does-not-revive-old-code');
  reject(e, protectVictim, e.table ? 'post-flush-protect-victim-stays-source-stale' : 'old-protect-victim-generation-after-capture', e.table ? 4 : 3);
  if (e.table) { retire(e, protectVictim); dynamic = compile(e, 'dynamic-leaf-before-write', [ALLOC], [], 2, 1); } else dynamic = caller;
  identity(e, dynamic, 'current-owner-before-identity-write');
  run(e, caller, 'guest-identity-executable-write', 1, S(ENTRY + 113), {version: 3, reason: e.table ? 1 : 6, retired: 1}, [['store', ALLOC, functionBytes.readUInt32LE(0), e.table ? 0 : 11]]);
  reject(e, dynamic, 'identity-write-caused-source-stale-before-capture', 4); checkCode(e, 'identity-written');
  const writeVictim = dynamic;
  if (!e.table) caller = compile(e, 'post-write-flush-free', [...at([113, 124, 138]), ALLOC, GATE + 112, GATE + 80], [gates[4], gates[2]], 16);
  flush(e, caller, 124, 'flush-after-write-does-not-revive-old-code');
  reject(e, writeVictim, e.table ? 'post-flush-write-victim-stays-source-stale' : 'old-write-victim-generation-after-capture', e.table ? 4 : 3);
  if (e.table) { retire(e, writeVictim); dynamic = compile(e, 'dynamic-leaf-before-free', [ALLOC], [], 2, 1); } else dynamic = caller;
  identity(e, dynamic, 'current-owner-before-free');
  run(e, caller, 'free-generated-allocation', 64, S(GATE + 80, 1, 42, 42, TOP - 16), {version: 3, reason: 8, retired: 4, detail: 0x10006}, freeSteps);
  complete(e, caller, 0x10006, [ALLOC, 0, 0x8000], ENTRY + 138, 1);
  reject(e, dynamic, 'free-caused-source-stale-before-capture', 4); read(e, ALLOC, 0, 1); read(e, ALLOC + 4092, 0, 1);
  const freeVictim = dynamic;
  if (e.table) discard(e, caller);
  caller = compile(e, 'freed-hint-whole-process-and-exit', [...at([138, 149, 164]), GATE + 112, GATE + 32], [gates[4], gates[0]], 12);
  flush(e, caller, 149, 'flush-freed-unmapped-hint');
  reject(e, freeVictim, e.table ? 'post-flush-free-victim-stays-source-stale' : 'old-free-victim-generation-after-capture', e.table ? 4 : 3);
  read(e, ALLOC, 0, 1); if (e.table) retire(e, freeVictim);
  flush(e, caller, 164, 'flush-null-base-ignores-ffffffff-size', 42, 42, 1, true);
  run(e, caller, 'exit-with-first-natural-answer', 64, S(GATE + 32, 1, 42, 42, TOP - 8), {version: 3, reason: 8, retired: 2, detail: 0x10003}, exitSteps);
  complete(e, caller, 0x10003, [42], ENTRY + 171, 42, true); read(e, BASE + 0x300c, 0xfacecafe);
  assert.equal(e.calls, e.table ? 23 : 22); assert.equal(e.retired, 59); assert.equal(e.tokens, 11); assert.equal(e.compilations, e.table ? 7 : 5);
  assert.deepEqual(arena(e).subarray(4236), initial.subarray(4236)); e.final = {cpu: cpu(e), exit: exit(e), sha256: hash(arena(e))};
  save(`${e.label}-terminal-arena.bin`, arena(e)); apiCall(e, 'close', []);
}
process.on('uncaughtException', error => {
  for (const e of contexts) save(`failure-${e.label}-arena.bin`, arena(e));
  save('failure-first.json', JSON.stringify({message: error.message, stack: error.stack, actual: error.actual, expected: error.expected}, null, 2));
  console.error(error); process.exitCode = 1;
});
exercise(fresh('replacement')); exercise(fresh('resident'));
const counts = {contexts: contexts.length, modules: modules.length, generated_runs: contexts.reduce((n, e) => n + e.calls, 0), retired: contexts.reduce((n, e) => n + e.retired, 0), provider_completions: contexts.reduce((n, e) => n + e.tokens, 0), provider_old_protect_writes: providerWrites.length, guest_helper_calls: helperTrace.length, guest_reads: helperTrace.filter(row => row.name === 'read32').length, guest_stores: helperTrace.filter(row => row.name !== 'read32').length};
assert.deepEqual(counts, {contexts: 2, modules: 13, generated_runs: 45, retired: 118, provider_completions: 22, provider_old_protect_writes: 4, guest_helper_calls: 126, guest_reads: 26, guest_stores: 100});
const sourcesAfter = sourceMap(); assert.deepEqual(sourcesAfter, sourcesBefore);
const result = {status: 'PASS', engine_sha256: engine.sha256, engine_bytes: engine.bytes.length, image_sha256: hash(image), function_hex: functionBytes.toString('hex'), command: process.argv, tools: {node: process.version, v8: process.versions.v8, platform: process.platform, arch: process.arch}, counts, contexts: contexts.map(e => ({owner: e.owner, captures: e.compilations, peak_resident_units: e.table ? e.peakUnits : null, generated_runs: e.calls, retired: e.retired, provider_completions: e.tokens, diagnostic_reads: e.diagnostics, final: e.final})), modules, observations, helper_trace: helperTrace, provider_memory_writes: providerWrites, lifecycle, public_diagnostics: diagnostics, sources_before: sourcesBefore, sources_after: sourcesAfter, artifacts, claim_ceiling: 'two authored relocated PE32 owner contexts; exact112-byte linkedV7/eight compact gate slots and following transfer canary; named current-process Flush acknowledges existing private flat32 coherence, preserves full4612 dirtyFP and retained code identity, real CALL/RET42 twice without CPU reseed. Separate Protect, guest identity EXEC write and Free cause source4 before new replacement capture; later replacement3 is generation-only. Resident callers are discarded only after consumed completion to avoid duplicate live gates, while the leaf survives repeated Flush. Allocated code is written only by guest instructions; no host mappedness/permission changes. LastError preservation and invalid handle/span breadth remain native-only evidence. Selected RAM/code/stack/output/canary evidence, not whole RAM, hardware cache, concurrency, general Windows or performance evidence.'};
save('result.json', JSON.stringify(result, null, 2)); console.log(JSON.stringify({status: 'PASS', ...counts, output}));
