import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {execFileSync} from 'node:child_process';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../support/engine.mjs';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(root && output);
const engine = readEngine(enginePath), SIZE = 4612, TRANSFER = 140;
const BASE = 0x500000, ENTRY = BASE + 0x1000, GATE = 0x8000, STACK = 0x70000, TOP = STACK + 4096, ALLOC = 0x10000000;
const initial = readFileSync(join(output, 'initial-arena.bin')); assert.equal(initial.length, SIZE);
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const artifacts = {}, contexts = [], modules = [], observations = [], helperTrace = [], diagnostics = [];
const save = (name, bytes) => { writeFileSync(join(output, name), bytes, {flag: 'wx'}); artifacts[name] = {bytes: Buffer.byteLength(bytes), sha256: hash(bytes)}; };
const program = Buffer.from('6a40680030000068400000006a00ff1554314000' +
  '89c7b902000000c707b8280000c747040001c8c7c7470807b82800c7470c00c707b8c74710290000c3ffd7' +
  '89c38b0768008000006a0057ff1550314000' +
  '6a40680030000068400000006a00ff1554314000' +
  '89c68b0e8b56108b6e3c53ff1558314000c7050c304000a5a5a5a50f0b', 'hex');
const functionBytes = Buffer.from('b82800000001c8c707b8280000c707b8290000c3', 'hex');
const entries = [0, 20, 63, 81, 101].map(offset => ENTRY + offset);
const gates = [[GATE + 32, 0x10003], [GATE + 64, 0x10005], [GATE + 80, 0x10006]];
const fixups = [[16, 0x3154], [77, 0x3150], [97, 0x3154], [114, 0x3158], [120, 0x300c]];
assert.equal(program.length, 130); assert.equal(functionBytes.length, 20);
assert.equal(entries.length + gates.length, 8); assert.equal(5 + 8 + 6 + 5 + 6, 30);
for (const [offset, rva] of fixups) assert.equal(program.readUInt32LE(offset), 0x400000 + rva);
assert.deepEqual([program.readUInt32LE(29), program.readUInt32LE(36), program.readUInt32LE(43), program.readUInt32LE(50), program.readUInt32LE(57)], Array.from({length: 5}, (_, i) => functionBytes.readUInt32LE(i * 4)));
function authoredPe() {
  const bytes = Buffer.alloc(2048), put16 = (at, value) => bytes.writeUInt16LE(value, at), put32 = (at, value) => bytes.writeUInt32LE(value, at);
  bytes.write('MZ'); put32(0x3c, 0x80); bytes.write('PE\0\0', 0x80);
  put16(0x84, 0x14c); put16(0x86, 3); put16(0x94, 224); put16(0x96, 0x102); put16(0x98, 0x10b);
  for (const [field, value] of [[4, 512], [8, 1024], [12, 0], [16, 0x1000], [20, 0x1000], [24, 0x3000], [28, 0x400000], [32, 4096], [36, 512], [56, 0x6000], [60, 512], [72, 0x100000], [76, 4096], [80, 0x100000], [84, 4096], [92, 16]]) put32(0x98 + field, value);
  put16(0x98 + 68, 3); put16(0x98 + 70, 0x100);
  for (const [index, rva, size] of [[1, 0x3100, 40], [5, 0x5000, 20], [12, 0x3150, 16]]) { put32(0x98 + 96 + index * 8, rva); put32(0x98 + 100 + index * 8, size); }
  [['.text', 130, 0x1000, 0x200, 0x60000020], ['.data', 512, 0x3000, 0x400, 0xc0000040], ['.fixups', 20, 0x5000, 0x600, 0x40000040]].forEach(([name, size, rva, raw, flags], index) => {
    const at = 0x178 + index * 40; bytes.write(name, at); for (const [field, value] of [[8, size], [12, rva], [16, 512], [20, raw], [36, flags]]) put32(at + field, value);
  });
  bytes.fill(0xcc, 0x200, 0x400); bytes.set(program, 0x200); put32(0x40c, 0xfacecafe);
  [0x3140, 0, 0, 0x3160, 0x3150].forEach((value, index) => put32(0x500 + index * 4, value));
  for (const at of [0x540, 0x550]) [0x3190, 0x31b0, 0x3170].forEach((value, index) => put32(at + index * 4, value));
  bytes.write('KeRnEl32.dLl', 0x560);
  for (const [at, hint, name] of [[0x570, 0x1234, 'ExitProcess'], [0x590, 0x5678, 'VirtualFree'], [0x5b0, 0x9abc, 'VirtualAlloc']]) { put16(at, hint); bytes.write(name, at + 2); }
  put32(0x600, 0x1000); put32(0x604, 20); [...fixups.map(([offset]) => 0x3000 | offset), 0].forEach((value, i) => put16(0x608 + i * 2, value));
  return bytes;
}
const image = readFileSync(join(output, 'allocation.exe')); assert.deepEqual(image, authoredPe());
save('program.bin', program); save('guest-function.bin', functionBytes); save('engine.wasm', engine.bytes);
const sourcePaths = [...execFileSync('rg', ['--files', 'engine/src'], {cwd: root, encoding: 'utf8'}).trim().split('\n'), 'Cargo.toml', 'Cargo.lock', 'engine/Cargo.toml', 'rust-toolchain.toml', 'engine/tests/support/pe32.rs', 'engine/tests/windows_allocation_protection_wasm.rs', 'engine/tests/fixtures/p2-allocation-protection/run.mjs', 'engine/tests/fixtures/support/engine.mjs'].sort();
const sourceMap = () => Object.fromEntries(sourcePaths.map(path => { const bytes = readFileSync(join(root, path)); return [path, {bytes: bytes.length, sha256: hash(bytes)}]; }));
const sourcesBefore = sourceMap();
function words(values) { const bytes = Buffer.alloc(values.length * 4); values.forEach((value, i) => bytes.writeUInt32LE(value >>> 0, i * 4)); return bytes; }
function record(magic, length, fields, version = 1) { const bytes = Buffer.alloc(length); bytes.write(magic); bytes.writeUInt16LE(version, 4); bytes.writeUInt16LE(1, 6); bytes.writeUInt32LE(length, 8); bytes.set(words(fields), 16); return bytes; }
const stateBytes = state => record('R3ST', 56, [...state.registers, state.pc, state.flags]);
const exitBytes = stop => record('R3EX', 40, [stop.reason, stop.retired, stop.detail ?? 0, 0, 0, 0], stop.version);
const helperBytes = (value = 0, faultAddress = null) => record('R3MH', 40, faultAddress === null ? [0, value, 0, 0, 0, 0] : [1, 0, 1, faultAddress, 1, 4]);
const installBytes = unit => record('R3IN', 32, [unit.low, unit.high, unit.slot, 0]);
const R = (eax = 0, ecx = 0, edx = 0, ebx = 0, esp = TOP, ebp = 0, esi = 0, edi = 0) => [eax, ecx, edx, ebx, esp, ebp, esi, edi];
function arena(e) { return Buffer.from(new Uint8Array(e.memory.buffer, e.base, SIZE)); }
function cpu(e) { const bytes = arena(e); return {registers: Array.from({length: 8}, (_, i) => bytes.readUInt32LE(16 + i * 4)), pc: bytes.readUInt32LE(48), flags: bytes.readUInt32LE(52)}; }
function exit(e) { const bytes = arena(e); return {version: bytes.readUInt16LE(60), reason: bytes.readUInt32LE(72), retired: bytes.readUInt32LE(76), detail: bytes.readUInt32LE(80)}; }
function unchanged(before, after, spans, label) { for (let i = 0; i < SIZE; i++) if (!spans.some(([start, end]) => i >= start && i < end)) assert.equal(after[i], before[i], `${label}: arena byte ${i}`); }
function apiCall(e, name, args, spans = [], expected = 0) { const before = arena(e), status = e.api[name](...args); assert.equal(status, expected, name); unchanged(before, arena(e), spans, name); return status; }
function transfer(e, bytes) { assert.ok(bytes.length <= 4096); new Uint8Array(e.memory.buffer).set(bytes, e.base + TRANSFER); }
const arities = {open: 3, close: 0, arena_ptr: 0, begin_image_input: 1, append_image_input: 2, load_pe32_linked_v5_input_at: 2, start_loaded_image: 2, compile_entries: 2, compile_resident_entries: 2, generation: 0, module_ptr: 0, module_len: 0, guard: 6, guard_resident: 7, read32: 1, store32: 2, store_resident32: 6, capture_call: 5, capture_resident_call: 6, complete_windows_call: 4, complete_resident_windows_call: 5, retire_stale_resident: 4, acknowledge_resident_installation: 5, dispatcher_module: 2, guard_dispatch_entry: 5, find_installed_resident: 3};
function fresh(owner) {
  const instance = new WebAssembly.Instance(engine.module, {}), api = Object.fromEntries(Object.entries(arities).map(([name, arity]) => { const fn = instance.exports[`ring3_abi_v1_${name}`]; assert.equal(typeof fn, 'function', name); assert.equal(fn.length, arity, name); return [name, fn]; }));
  const e = {owner, label: owner, api, memory: instance.exports.memory, low: 0x53200001 + contexts.length, high: 0xa5320000, units: [], compilations: 0, tokens: 0, calls: 0, retired: 0, diagnostics: 0, table: owner === 'resident' ? new WebAssembly.Table({element: 'anyfunc', initial: 8, maximum: 8}) : null};
  assert.equal(api.open(8, e.low, e.high), 0); e.base = api.arena_ptr() >>> 0; contexts.push(e);
  apiCall(e, 'begin_image_input', [image.length]); transfer(e, image); apiCall(e, 'append_image_input', [0, image.length]); transfer(e, Buffer.alloc(4096, 0xa5));
  apiCall(e, 'load_pe32_linked_v5_input_at', [BASE, GATE], [[TRANSFER, TRANSFER + 96]]);
  assert.deepEqual(arena(e).subarray(TRANSFER, TRANSFER + 96), record('R3LI', 96, [BASE, 0x6000, ENTRY, 5, GATE, 3, 0, 0, ...gates.flat(), 0, 0, 0, 0, 0, 0], 5));
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
  assert.ok(pcs.length <= 8 && instructions <= 64); assert.ok(++e.compilations <= 6, 'bounded captures'); transfer(e, words([...pcs, ...gateSpecs.flat()]));
  apiCall(e, e.table ? 'compile_resident_entries' : 'compile_entries', [pcs.length, gateSpecs.length], e.table ? [[TRANSFER, TRANSFER + 24]] : []);
  const receipt = arena(e).subarray(TRANSFER, TRANSFER + 24), unit = {name, slot, pcs, instructions};
  if (e.table) { assert.equal(receipt.readUInt32LE(0), 1); assert.equal(receipt.readUInt32LE(4), 24); [unit.low, unit.high, unit.pointer, unit.length] = Array.from({length: 4}, (_, i) => receipt.readUInt32LE(8 + i * 4)); }
  else { unit.generation = e.api.generation(); unit.pointer = e.api.module_ptr() >>> 0; unit.length = e.api.module_len() >>> 0; assert.equal(unit.generation, e.compilations); }
  assert.ok(unit.pointer > 0 && unit.length > 8 && unit.length <= 65536 && unit.pointer + unit.length <= e.memory.buffer.byteLength);
  const bytes = Buffer.from(new Uint8Array(e.memory.buffer, unit.pointer, unit.length)), module = new WebAssembly.Module(bytes), imports = WebAssembly.Module.imports(module), ring3 = {};
  const guard = e.table ? 'guard_resident' : 'guard', store = e.table ? 'store_resident32' : 'store32';
  const helperNames = name === 'dynamic-ret' ? [guard, 'read32'] : [guard, 'read32', store];
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
  const file = `${e.label}-${e.compilations}-${name}.wasm`; save(file, bytes); e.units.push(unit); modules.push({context: e.label, name, file, generation: unit.generation ?? null, id_low: unit.low ?? null, id_high: unit.high ?? null, slot: e.table ? slot : null, entries: pcs, instructions, bytes: bytes.length, sha256: hash(bytes), imports}); return unit;
}
function run(e, unit, label, budget, state, stop, steps = []) {
  const before = arena(e), expected = Buffer.from(before); expected.set(stateBytes(state), 0); expected.set(exitBytes(stop), 56);
  if (steps.length) expected.set(helperBytes(steps.at(-1)[0] === 'read' ? steps.at(-1)[2] : 0), 100);
  if (e.table && budget && before.readUInt32LE(96) === 0) expected.set(installBytes(unit), TRANSFER);
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
function retire(e, unit) { if (e.table) { e.table.set(unit.slot, null); apiCall(e, 'retire_stale_resident', [e.low, e.high, unit.low, unit.high]); } }
function read(e, address, expected, fault = false) {
  const before = arena(e); assert.equal(e.api.read32(address), 0); const after = arena(e); unchanged(before, after, [[100, 140]], 'public diagnostic read'); assert.deepEqual(after.subarray(100, 140), helperBytes(expected, fault ? address : null)); e.diagnostics++;
  diagnostics.push({context: e.label, name: 'read32', address, value: expected, fault}); return expected;
}
function checkCode(e, changed, zero = false) {
  const wanted = Buffer.alloc(32); if (!zero) { wanted.set(functionBytes); if (changed) wanted[1] = 41; }
  const actual = Buffer.alloc(32); for (let i = 0; i < 32; i += 4) actual.writeUInt32LE(read(e, ALLOC + i, wanted.readUInt32LE(i)), i); assert.deepEqual(actual, wanted);
  read(e, ALLOC + 60, 0); read(e, ALLOC + 4092, 0); read(e, BASE + 0x300c, 0xfacecafe);
  save(`${e.label}-code-${zero ? 'zeroed-remap' : changed ? 'changed' : 'original'}-${e.diagnostics}.bin`, actual);
}
function complete(e, caller, id, args, returnPc, result, terminal = false) {
  const before = arena(e), state = cpu(e), token = ++e.tokens, count = args.length;
  apiCall(e, e.table ? 'capture_resident_call' : 'capture_call', e.table ? [e.low, e.high, caller.low, caller.high, 2, count] : [e.low, e.high, caller.generation, 2, count], [[TRANSFER, TRANSFER + 112]]);
  assert.deepEqual(arena(e).subarray(TRANSFER, TRANSFER + 112), record('R3CF', 112, [token, id, 2, count, state.pc, state.registers[4], returnPc, 0, ...args, ...Array(16 - count).fill(0)]));
  const expected = arena(e), next = {registers: [...state.registers], pc: returnPc, flags: state.flags};
  if (terminal) expected.set(exitBytes({version: 4, reason: 9, retired: 0, detail: result}), 56);
  else { next.registers[0] = result; next.registers[4] += 4 * (count + 1); expected.set(stateBytes(next), 0); expected.set(exitBytes({version: 3, reason: 3, retired: 0}), 56); }
  apiCall(e, e.table ? 'complete_resident_windows_call' : 'complete_windows_call', e.table ? [e.low, e.high, caller.low, caller.high, token] : [e.low, e.high, caller.generation, token], [[16, 96]]); assert.deepEqual(arena(e), expected, 'provider full arena');
  observations.push({context: e.label, name: `provider-${token}`, api: id, args, token, expected: {result, terminal, pc: terminal ? state.pc : returnPc, flags: state.flags}, observed: {cpu: cpu(e), exit: exit(e)}, before_sha256: hash(before), after_sha256: hash(arena(e))});
}
const pushSteps = (esp, values) => values.map((value, i) => ['store', esp - 4 * (i + 1), value]);
const allocSteps = offset => [...pushSteps(TOP, [0x40, 0x3000, 64, 0]), ['read', BASE + 0x3154, GATE + 64], ['store', TOP - 20, ENTRY + offset]];
function exercise(e) {
  let caller = compile(e, 'caller', [...entries, ...gates.map(([pc]) => pc)], gates, 30); const start = cpu(e);
  run(e, caller, 'zero-budget', 0, start, {version: 3, reason: 1, retired: 0});
  new DataView(e.memory.buffer).setUint32(e.base + 96, 1, true); run(e, caller, 'cancel-before-zero', 0, start, {version: 3, reason: 2, retired: 0}); new DataView(e.memory.buffer).setUint32(e.base + 96, 0, true);
  run(e, caller, 'allocate-rwx', 64, {registers: R(0, 0, 0, 0, TOP - 20), pc: GATE + 64, flags: 2}, {version: 3, reason: 8, retired: 5, detail: 0x10005}, allocSteps(20));
  complete(e, caller, 0x10005, [0, 64, 0x3000, 0x40], ENTRY + 20, ALLOC); guard(e, caller);
  for (const offset of [0, 16, 60, 4092]) read(e, ALLOC + offset, 0);
  const storeCode = Array.from({length: 5}, (_, i) => ['store', ALLOC + i * 4, functionBytes.readUInt32LE(i * 4)]);
  run(e, caller, 'guest-code-and-cold-call', 64, {registers: R(ALLOC, 2, 0, 0, TOP - 4, 0, 0, ALLOC), pc: ALLOC, flags: 2}, {version: 3, reason: 3, retired: 8}, [...storeCode, ['store', TOP - 4, ENTRY + 63]]);
  checkCode(e, false); read(e, TOP - 4, ENTRY + 63);
  const body = compile(e, 'dynamic-function', [ALLOC], [], 5, 1);
  run(e, body, 'identity-code-write', 64, {registers: R(42, 2, 0, 0, TOP - 4, 0, 0, ALLOC), pc: ALLOC + 13, flags: 2}, {version: 2, reason: 6, retired: 3}, [['store', ALLOC, 0x28b8, 11]]);
  reject(e, body, 'identity-stale-owner', 4); checkCode(e, false); retire(e, body);
  const tail = compile(e, 'dynamic-changed-tail', [ALLOC + 13], [], 2, 1);
  run(e, tail, 'changed-code-write', 64, {registers: R(42, 2, 0, 0, TOP - 4, 0, 0, ALLOC), pc: ALLOC + 19, flags: 2}, {version: 2, reason: 6, retired: 1}, [['store', ALLOC, 0x29b8, 11]]);
  reject(e, tail, 'changed-stale-owner', 4); checkCode(e, true); retire(e, tail);
  const returned = compile(e, 'dynamic-ret', [ALLOC + 19], [], 1, 1);
  run(e, returned, 'real-ret', 1, {registers: R(42, 2, 0, 0, TOP, 0, 0, ALLOC), pc: ENTRY + 63, flags: 2}, {version: 2, reason: 1, retired: 1}, [['read', TOP - 4, ENTRY + 63]]);
  if (!e.table) caller = compile(e, 'caller-resume', [...entries, ...gates.map(([pc]) => pc)], gates, 30);
  else guard(e, caller);
  run(e, caller, 'free-generated-code', 64, {registers: R(0x29b8, 2, 0, 42, TOP - 16, 0, 0, ALLOC), pc: GATE + 80, flags: 2}, {version: 3, reason: 8, retired: 6, detail: 0x10006}, [['read', ALLOC, 0x29b8], ...pushSteps(TOP, [0x8000, 0, ALLOC]), ['read', BASE + 0x3150, GATE + 80], ['store', TOP - 16, ENTRY + 81]]);
  complete(e, caller, 0x10006, [ALLOC, 0, 0x8000], ENTRY + 81, 1); guard(e, caller);
  read(e, ALLOC, 0, true); read(e, ALLOC + 4092, 0, true); reject(e, returned, e.table ? 'free-unmapped-owner' : 'replaced-owner-after-free', e.table ? 4 : 3);
  run(e, caller, 'reallocate-rwx', 64, {registers: R(1, 2, 0, 42, TOP - 20, 0, 0, ALLOC), pc: GATE + 64, flags: 2}, {version: 3, reason: 8, retired: 5, detail: 0x10005}, allocSteps(101));
  complete(e, caller, 0x10005, [0, 64, 0x3000, 0x40], ENTRY + 101, ALLOC); guard(e, caller); checkCode(e, false, true);
  reject(e, returned, e.table ? 'remap-does-not-revive-owner' : 'replaced-owner-after-remap', e.table ? 4 : 3);
  run(e, caller, 'guest-zero-loads-and-exit', 64, {registers: R(ALLOC, 0, 0, 42, TOP - 8, 0, ALLOC, ALLOC), pc: GATE + 32, flags: 2}, {version: 3, reason: 8, retired: 6, detail: 0x10003}, [['read', ALLOC, 0], ['read', ALLOC + 16, 0], ['read', ALLOC + 60, 0], ...pushSteps(TOP, [42]), ['read', BASE + 0x3158, GATE + 32], ['store', TOP - 8, ENTRY + 118]]);
  read(e, BASE + 0x300c, 0xfacecafe); complete(e, caller, 0x10003, [42], ENTRY + 118, 42, true);
  assert.equal(e.calls, 14); assert.equal(e.retired, 35); assert.equal(e.tokens, 4); assert.equal(e.compilations, e.table ? 4 : 5); assert.deepEqual(arena(e).subarray(4236), initial.subarray(4236));
  e.final = {cpu: cpu(e), exit: exit(e), sha256: hash(arena(e))}; save(`${e.label}-terminal-arena.bin`, arena(e)); apiCall(e, 'close', []);
}
process.on('uncaughtException', error => { for (const e of contexts) save(`failure-${e.label}-arena.bin`, arena(e)); save('failure-first.json', JSON.stringify({message: error.message, stack: error.stack, actual: error.actual, expected: error.expected}, null, 2)); console.error(error); process.exitCode = 1; });
exercise(fresh('replacement')); exercise(fresh('resident'));
const counts = {contexts: contexts.length, modules: modules.length, generated_runs: contexts.reduce((n, e) => n + e.calls, 0), retired: contexts.reduce((n, e) => n + e.retired, 0), provider_completions: contexts.reduce((n, e) => n + e.tokens, 0), guest_helper_calls: helperTrace.length, guest_reads: helperTrace.filter(row => row.name === 'read32').length, guest_stores: helperTrace.filter(row => row.name !== 'read32').length};
assert.deepEqual(counts, {contexts: 2, modules: 10, generated_runs: 28, retired: 70, provider_completions: 8, guest_helper_calls: 66, guest_reads: 18, guest_stores: 48}); assert.equal(helperTrace.filter(row => row.status === 11).length, 4);
const sourcesAfter = sourceMap(); assert.deepEqual(sourcesAfter, sourcesBefore);
const result = {status: 'PASS', engine_sha256: engine.sha256, engine_bytes: engine.bytes.length, image_sha256: hash(image), function_hex: functionBytes.toString('hex'), command: process.argv, tools: {node: process.version, v8: process.versions.v8, platform: process.platform, arch: process.arch}, counts, contexts: contexts.map(e => ({owner: e.owner, captures: e.compilations, provider_completions: e.tokens, generated_runs: e.calls, retired: e.retired, diagnostic_reads: e.diagnostics, final: e.final})), modules, observations, helper_trace: helperTrace, public_diagnostics: diagnostics, sources_before: sourcesBefore, sources_after: sourcesAfter, artifacts, claim_ceiling: 'two authored relocated-PE RWX lifecycle contexts, full4612 and dirtyFP assertions, selected code/stack/zero/canary RAM; guest stores only for allocated code, real named provider CALLs and RET, page-based source invalidation; replacement after-free rejection is generation3 while resident source4 is causal unmap/remap proof; second allocation remains mapped at ExitProcess until explicit close; six-mode permissions independently covered by native tests, no hardware cache/DEP/CFG, general Windows, concurrency or performance claim'};
save('result.json', JSON.stringify(result, null, 2)); console.log(JSON.stringify({status: 'PASS', ...counts, output}));
