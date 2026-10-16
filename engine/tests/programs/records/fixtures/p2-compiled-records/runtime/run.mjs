import {pageFaultExit, pageFaultPacket} from '../../../../../support/wasm/page-fault.mjs';
import {attachDetectionApi} from '../../../../../support/wasm/binding.mjs';
import {ARENA_SIZE} from '../../../../../support/wasm/arena.mjs';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, readdirSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {execFileSync} from 'node:child_process';
import {readEngine} from '../../../../../support/wasm/engine.mjs';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(root && output);
const engine = readEngine(enginePath), SIZE = ARENA_SIZE, TRANSFER = 140;
const initial = readFileSync(join(output, 'initial-arena.bin'));
assert.equal(initial.length, SIZE);
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const artifacts = {}, contexts = [], observations = [], helperTrace = [];
function save(name, bytes) { writeFileSync(join(output, name), bytes, {flag: 'wx'}); artifacts[name] = {bytes: Buffer.byteLength(bytes), sha256: hash(bytes)}; }
const fixtureDir = 'engine/tests/programs/records/fixtures/p2-compiled-records';
const sourcePaths = ['engine/tests/support/wasm/page-fault.mjs', 'engine/tests/support/wasm/binding.mjs', ...execFileSync('rg', ['--files', 'engine/src'], {cwd: root, encoding: 'utf8'}).trim().split('\n'), 'Cargo.toml', 'Cargo.lock', 'engine/Cargo.toml', 'engine/tests/support/loader/pe32/image.rs', 'engine/tests/programs/sort/support/pe32.rs', 'engine/tests/programs/records/support/compiled.rs', 'engine/tests/programs/records/native/compiled.rs', 'engine/tests/programs/native.rs', 'engine/tests/programs/records/wasm/compiled.rs', 'engine/tests/programs/wasm.rs', 'engine/tests/support/wasm.rs', 'engine/tests/support/wasm/engine.mjs', ...readdirSync(join(root, fixtureDir), {withFileTypes: true}).filter(entry => entry.isFile()).map(entry => `${fixtureDir}/${entry.name}`)].sort();
const sourceMap = () => Object.fromEntries(sourcePaths.map(path => { const bytes = readFileSync(join(root, path)); return [path, {bytes: bytes.length, sha256: hash(bytes)}]; }));
const sourcesBefore = sourceMap();

const MOD = 2n ** 32n, HIGH = 2n ** 27n;
const wrap = value => ((value % MOD) + MOD) % MOD;
function records(bytes) {
  assert.equal(bytes.length, 128);
  return Array.from({length: 16}, (_, i) => {
    const at = i * 8, byte = bytes[at], word = bytes[at + 2] + 256 * bytes[at + 3];
    return {signed_byte: byte < 128 ? byte : byte - 256, tag: bytes[at + 1], signed_word: word < 32768 ? word : word - 65536, weight: bytes[at + 4] + 256 * bytes[at + 5], marker: bytes[at + 6] + 256 * bytes[at + 7]};
  });
}
function reduce(rows, count, seed) {
  let acc = BigInt(seed);
  for (const row of rows.slice(0, count)) {
    acc = (acc * 32n) % MOD + acc / HIGH;
    const product = BigInt(row.signed_byte) * BigInt(row.signed_word);
    acc = row.tag % 2 ? acc ^ wrap(product) : acc + product;
    acc = wrap(acc + BigInt(row.weight) * BigInt(row.tag));
  }
  return Number(acc);
}
const literalChecksums = {0: 3735928559, 1: 3589791227, 2: 3200631034, 15: 3386345419, 16: 989264236};
const fixtures = new Map([0, 1, 2, 15, 16].map(count => {
  const folder = join(output, `count-${count}`), spec = JSON.parse(readFileSync(join(folder, 'manifest.json'), 'utf8')), image = readFileSync(join(folder, 'records.exe'));
  assert.equal(spec.count, count); assert.equal(spec.seed, 0xdeadbeef);
  const bytes = Buffer.from(spec.record_hex, 'hex'), rows = records(bytes), checksum = reduce(rows, count, spec.seed);
  assert.equal(hash(bytes), '71c5b89591bf15c6b81fae547ca83764e2f6d9968449142b791ddd088491199d');
  assert.equal(checksum, literalChecksums[count]);
  assert.equal(rows[0].signed_byte * rows[0].signed_word, 4194304);
  assert.equal(rows[6].signed_byte * rows[6].signed_word, -4194176);
  assert.deepEqual(image.subarray(spec.text_raw, spec.text_raw + spec.program_hex.length / 2), Buffer.from(spec.program_hex, 'hex'));
  const kernel = Buffer.from(spec.kernel_hex, 'hex');
  assert.equal(kernel.length, 74); assert.equal(hash(kernel), '8bb4f64cef988239a8042519e089027aa03cb938dcd8cc99de18124470cef5be');
  assert.deepEqual(image.subarray(spec.text_raw + spec.kernel_offset, spec.text_raw + spec.kernel_end_offset), kernel);
  assert.equal(spec.allocation_bytes, 128); assert.equal(spec.cdecl_arguments, 3);
  for (const group of spec.groups) { assert.ok(group.block_count <= 8); assert.ok(group.instruction_count <= 64); }
  return [count, {spec, image, bytes, rows, checksum}];
}));

const names = ['guard', 'guard_resident', 'guard_detect', 'guard_detect_resident', 'finish_exception_detection', 'open', 'close', 'arena_ptr', 'begin_image_input', 'append_image_input', 'load_pe32_linked_v5_input_at', 'start_loaded_image', 'dispatcher_module', 'guard_dispatch_entry', 'find_installed_resident', 'compile_resident_entries', 'acknowledge_resident_installation',  'read8', 'read16', 'read32', 'store_resident32', 'capture_resident_call', 'complete_resident_windows_call', 'protect', 'cpu_read16', 'cpu_read32', 'cpu_read8', 'cpu_store_resident32'];
function arena(e) { return Buffer.from(e.memory.buffer, e.base, SIZE).map(x => x); }
function view(e) { return new DataView(e.memory.buffer); }
function cpu(e) { const v = view(e); return {registers: Array.from({length: 8}, (_, i) => v.getUint32(e.base + 16 + i * 4, true)), pc: v.getUint32(e.base + 48, true), flags: v.getUint32(e.base + 52, true)}; }
function exit(e) { const v = view(e); return {version: v.getUint16(e.base + 60, true), reason: v.getUint32(e.base + 72, true), retired: v.getUint32(e.base + 76, true), detail: v.getUint32(e.base + 80, true), address: v.getUint32(e.base + 84, true), access: v.getUint32(e.base + 88, true), length: v.getUint32(e.base + 92, true)}; }
function unchanged(before, after, spans, label) { for (let i = 0; i < SIZE; i++) if (!spans.some(([a, b]) => i >= a && i < b)) assert.equal(after[i], before[i], `${label}: arena byte ${i}`); }
function status(e, name, fn, spans = []) { const before = arena(e); assert.equal(fn(), 0, name); unchanged(before, arena(e), spans, name); }
function transfer(e, bytes) { assert.ok(bytes.length <= 4096); new Uint8Array(e.memory.buffer).set(bytes, e.base + TRANSFER); }
function words(values) { const bytes = Buffer.alloc(values.length * 4); values.forEach((value, i) => bytes.writeUInt32LE(value >>> 0, i * 4)); return bytes; }
function readWord(e, address, fault = false) {
  const before = arena(e); assert.equal(e.api.read32(address), 0); const result = arena(e), fields = Array.from({length: 6}, (_, i) => result.readUInt32LE(116 + 4 * i));
  assert.deepEqual(fields, fault ? [1, 0, 1, address, 1, 4] : [0, fields[1], 0, 0, 0, 0]); unchanged(before, result, [[100, 140]], 'diagnostic read32'); e.diagnosticReads++;
  return fields[1];
}
function readBytes(e, address, length) { assert.equal(length % 4, 0); const bytes = Buffer.alloc(length); for (let i = 0; i < length; i += 4) bytes.writeUInt32LE(readWord(e, address + i), i); return bytes; }
function checkRecords(e) {
  assert.deepEqual(readBytes(e, e.baseAddress + e.spec.record_rva, 128), e.fixture.bytes, 'loaded source records unchanged');
  assert.deepEqual(readBytes(e, e.allocation, 128), e.fixture.bytes, 'guest copied all records including markers');
  assert.equal(readWord(e, e.allocation + 128), 0, 'selected allocation suffix canary');
  assert.equal(readWord(e, e.allocation + 4092), 0, 'last allocation dword canary');
  assert.equal(readWord(e, e.baseAddress + e.spec.canary_rva), 0, 'loaded data canary');
}
function fresh(label, count, mode = 'normal') {
  const fixture = fixtures.get(count), {spec, image} = fixture, instance = new WebAssembly.Instance(engine.module, {}), memory = instance.exports.memory;
  const api = attachDetectionApi(Object.fromEntries(names.map(name => [name, instance.exports[`ring3_abi_v1_${name}`]])), instance.exports);
  for (const [name, fn] of Object.entries(api)) assert.equal(typeof fn, 'function', name);
  const e = {label, mode, fixture, spec, api, memory, low: 0xc5250000 + contexts.length + 1, high: 0xa5250000, baseAddress: spec.actual_base, units: [], groups: new Map(), table: new WebAssembly.Table({element: 'anyfunc', initial: 8, maximum: 8}), retired: 0, runs: 0, captures: 0, completions: 0, diagnosticReads: 0, controls: [], abiEntry: null, abiReturn: null};
  assert.equal(api.open(8, e.low, e.high), 0); e.base = api.arena_ptr() >>> 0;
  status(e, 'begin image', () => api.begin_image_input(image.length)); transfer(e, image);
  status(e, 'append image', () => api.append_image_input(0, image.length)); transfer(e, Buffer.alloc(4096, 0xa5));
  status(e, 'load relocated image', () => api.load_pe32_linked_v5_input_at(e.baseAddress, spec.gate_base), [[TRANSFER, TRANSFER + 96]]);
  const v = view(e), at = e.base + TRANSFER;
  assert.equal(v.getUint32(at, true), 0x494c3352); assert.equal(v.getUint16(at + 4, true), 5); assert.equal(v.getUint32(at + 8, true), 96);
  assert.equal(v.getUint32(at + 16, true), e.baseAddress); assert.equal(v.getUint32(at + 24, true), e.baseAddress + spec.text_rva);
  assert.equal(v.getUint32(at + 36, true), 3);
  e.gates = Array.from({length: 3}, (_, i) => [v.getUint32(at + 48 + i * 8, true), v.getUint32(at + 52 + i * 8, true)]);
  assert.deepEqual(e.gates, [[spec.gate_base + 32, 0x10003], [spec.gate_base + 64, 0x10005], [spec.gate_base + 80, 0x10006]]);
  e.text = e.baseAddress + spec.text_rva;
  e.plan = [...spec.groups.map(g => ({...g, entries: g.offsets.map(pc => e.text + pc), gates: []})), {name: 'gates', entries: e.gates.map(([pc]) => pc), gates: e.gates}];
  for (const [i, imported] of spec.imports.entries()) assert.equal(readWord(e, e.baseAddress + imported.iat_rva), e.gates[i][0]);
  status(e, 'start loaded image', () => api.start_loaded_image(spec.stack_base, 1), [[16, 96]]);
  assert.deepEqual(cpu(e), {registers: [0, 0, 0, 0, spec.stack_base + 4096, 0, 0, 0], pc: e.text, flags: 2});
  const started = arena(e); new Uint8Array(memory.buffer).set(initial.subarray(4236), e.base + 4236);
  unchanged(started, arena(e), [[4236, 4364]], 'dirty fp seed after startup');
  assert.deepEqual(arena(e).subarray(4236), initial.subarray(4236));
  status(e, 'dispatcher module', () => api.dispatcher_module(e.low, e.high), [[TRANSFER, TRANSFER + 32]]);
  const d = view(e), pointer = d.getUint32(e.base + TRANSFER + 24, true), length = d.getUint32(e.base + TRANSFER + 28, true);
  const bytes = Buffer.from(memory.buffer, pointer, length).map(x => x); save(`${label}-dispatcher.wasm`, bytes);
  e.dispatcher = new WebAssembly.Instance(new WebAssembly.Module(bytes), {env: {memory, table: e.table}, ring3: {guard_dispatch_entry: api.guard_dispatch_entry, find_installed_resident: api.find_installed_resident}}).exports.run;
  assert.equal(e.dispatcher.length, 4); contexts.push(e); return e;
}

function compile(e, group, pc) {
  assert.ok(group.entries.includes(pc)); assert.ok(!e.groups.has(group.name));
  assert.ok(group.entries.length <= 8); assert.ok(e.units.length < 8);
  transfer(e, words([...group.entries, ...group.gates.flat()]));
  status(e, 'compile installed group', () => e.api.compile_resident_entries(group.entries.length, group.gates.length), [[TRANSFER, TRANSFER + 24]]);
  const v = view(e), at = e.base + TRANSFER;
  assert.equal(v.getUint32(at, true), 1); assert.equal(v.getUint32(at + 4, true), 24);
  const unit = {name: group.name, low: v.getUint32(at + 8, true), high: v.getUint32(at + 12, true), slot: e.units.length, pointer: v.getUint32(at + 16, true), length: v.getUint32(at + 20, true), entries: group.entries};
  assert.ok(unit.length > 8 && unit.length <= 65536 && unit.pointer + unit.length <= e.memory.buffer.byteLength);
  const bytes = Buffer.from(e.memory.buffer, unit.pointer, unit.length).map(x => x), module = new WebAssembly.Module(bytes), imports = WebAssembly.Module.imports(module);
  const ring3 = {};
  for (const imported of imports) {
    if (imported.module === 'env') { assert.deepEqual(imported, {module: 'env', name: 'memory', kind: 'memory'}); continue; }
    if (imported.name === 'finish_exception_detection') { assert.equal(imported.module, 'ring3'); assert.equal(imported.kind, 'function'); continue; }
    assert.equal(imported.module, 'ring3'); assert.equal(imported.kind, 'function');
    assert.ok(['guard_detect_resident', 'cpu_read8', 'cpu_read16', 'cpu_read32', 'cpu_store_resident32'].includes(imported.name), imported.name);
    ring3[imported.name] = imported.name === 'guard_detect_resident' ? e.api.guard_detect_resident : (...args) => {
      const before = arena(e), result = e.api[imported.name](...args), after = arena(e);
      unchanged(before, after, [[100, 140]], `forwarded ${imported.name}`);
      helperTrace.push({context: e.label, unit: unit.name, name: imported.name, args: args.map(x => x >>> 0), status: result, helper_hex: after.subarray(100, 140).toString('hex')});
      return result;
    };
  }
  unit.run = new WebAssembly.Instance(module, {env: {memory: e.memory}, ring3: {...(ring3), finish_exception_detection: e.api.finish_exception_detection}}).exports.run;
  assert.equal(unit.run.length, 4); e.table.set(unit.slot, unit.run);
  status(e, 'acknowledge installed group', () => e.api.acknowledge_resident_installation(e.low, e.high, unit.low, unit.high, unit.slot), [[TRANSFER, TRANSFER + 32]]);
  const receipt = arena(e).subarray(TRANSFER, TRANSFER + 32);
  assert.equal(receipt.subarray(0, 4).toString(), 'R3IN'); assert.equal(receipt.readUInt32LE(4), 0x10001); assert.equal(receipt.readUInt32LE(8), 32);
  assert.deepEqual([receipt.readUInt32LE(16), receipt.readUInt32LE(20), receipt.readUInt32LE(24)], [unit.low, unit.high, unit.slot]);
  const file = `${e.label}-unit-${unit.slot}-${unit.name}.wasm`; save(file, bytes);
  e.units.push({...unit, run: unit.run, file, sha256: hash(bytes), imports}); e.groups.set(group.name, unit);
}
function dispatch(e, budget) {
  const before = arena(e), result = e.dispatcher(e.base, e.base + 56, budget, e.base + 96);
  assert.equal(result, 0); const after = arena(e); unchanged(before, after, [[16, 96], [100, 172]], 'dispatcher');
  const stop = exit(e), state = cpu(e); assert.ok(stop.retired <= budget);
  e.retired += stop.retired; e.runs++;
  observations.push({context: e.label, index: e.runs, budget, status: result, ...stop, pc: state.pc, flags: state.flags, registers: state.registers});
  return stop;
}
function inspectLeaf(e) {
  const state = cpu(e), m = e.spec.milestones;
  if (state.pc === e.text + m.leaf_entry && !e.abiEntry) {
    e.abiEntry = state;
    assert.deepEqual(readBytes(e, state.registers[4], 16), words([e.text + m.leaf_return, e.allocation, e.spec.count, e.spec.seed]));
    assert.equal(state.registers[4], e.spec.stack_base + 4096 - 16);
    checkRecords(e);
  }
  if (state.pc === e.text + m.leaf_return && e.abiEntry && !e.abiReturn) {
    e.abiReturn = state; assert.equal(state.registers[0], e.fixture.checksum);
    for (const register of [3, 5, 6, 7]) assert.equal(state.registers[register], e.abiEntry.registers[register], `cdecl preserved register ${register}`);
    assert.equal(state.registers[4], e.abiEntry.registers[4] + 4, 'callee ret leaves caller arguments');
    assert.equal(state.flags, 0x46, 'completed count test or final loop comparison');
  }
}
function capture(e, stop) {
  const owner = e.groups.get('gates'); assert.ok(owner);
  const count = stop.detail === 0x10005 ? 4 : stop.detail === 0x10006 ? 3 : stop.detail === 0x10003 ? 1 : assert.fail('unknown provider');
  const state = cpu(e), m = e.spec.milestones, returnPc = e.text + (count === 4 ? m.alloc_return : count === 3 ? m.free_return : m.exit_return);
  const receipt = arena(e).subarray(TRANSFER, TRANSFER + 32);
  assert.equal(receipt.subarray(0, 4).toString(), 'R3IN'); assert.deepEqual([receipt.readUInt32LE(16), receipt.readUInt32LE(20), receipt.readUInt32LE(24)], [owner.low, owner.high, owner.slot]);
  status(e, 'capture named provider', () => e.api.capture_resident_call(e.low, e.high, owner.low, owner.high, 2, count), [[TRANSFER, TRANSFER + 112]]);
  const frame = arena(e).subarray(TRANSFER, TRANSFER + 112), token = frame.readUInt32LE(16), args = Array.from({length: count}, (_, i) => frame.readUInt32LE(48 + 4 * i));
  assert.equal(frame.subarray(0, 4).toString(), 'R3CF'); assert.equal(frame.readUInt32LE(4), 0x10001); assert.equal(frame.readUInt32LE(8), 112);
  assert.equal(token, ++e.captures);
  assert.deepEqual(Array.from({length: 7}, (_, i) => frame.readUInt32LE(20 + 4 * i)), [stop.detail, 2, count, state.pc, state.registers[4], returnPc, 0]);
  assert.ok(frame.subarray(48 + 4 * count).every(x => x === 0));
  assert.equal(state.registers[4], e.spec.stack_base + 4096 - 4 * (count + 1));
  assert.deepEqual(readBytes(e, state.registers[4], 4 * (count + 1)), words([returnPc, ...args]));
  save(`${e.label}-provider-${e.captures}.bin`, frame);
  return {owner, token, args, cpu: state, returnPc};
}
function complete(e, frame, terminal = false) {
  status(e, 'complete named provider', () => e.api.complete_resident_windows_call(e.low, e.high, frame.owner.low, frame.owner.high, frame.token), [[16, 96]]);
  e.completions++; const stop = exit(e), state = cpu(e);
  assert.equal(stop.retired, 0);
  if (terminal) { assert.equal(stop.reason, 9); assert.equal(stop.detail, e.fixture.checksum); assert.deepEqual(state, frame.cpu); }
  else { const registers = [...frame.cpu.registers]; registers[0] = frame.args.length === 4 ? state.registers[0] : 1; registers[4] = e.spec.stack_base + 4096; assert.deepEqual(state, {registers, pc: frame.returnPc, flags: frame.cpu.flags}); assert.equal(stop.reason, 3); }
}

function faultRepair(e, first) {
  assert.equal(e.mode, 'fault'); assert.ok(!e.faulted); e.faulted = true;
  assert.deepEqual(first, {version: 12, reason: 23, retired: 11, detail: 5, address: e.allocation, access: 1, length: 1});
  assert.equal(e.retired, 216); const stopped = cpu(e);
  assert.equal(stopped.pc, e.text + e.spec.kernel_offset + 0x19);
  const before = arena(e), helper = before.subarray(100, 140);
  assert.deepEqual(helper, pageFaultPacket({errorCode: 5, address: e.allocation, access: 1, length: 1}));
  const retry = dispatch(e, 65536), wanted = Buffer.from(before); wanted.writeUInt32LE(0, 76);
  assert.deepEqual(retry, {...first, retired: 0}); assert.deepEqual(arena(e), wanted, 'unchanged fault retry only clears retired'); assert.deepEqual(cpu(e), stopped);
  save(`${e.label}-read-fault-first.bin`, before); save(`${e.label}-read-fault-retry.bin`, arena(e));
  status(e, 'repair record read permission', () => e.api.protect(e.allocation, 1, 3));
  assert.deepEqual(cpu(e), stopped); checkRecords(e);
  e.controls.push({name: 'write-only-record-read-fault', first, retry, stopped, repair: 'only protect actual allocation Read/Write; record bytes unchanged before retry'});
}
const chunks = [0, 1, 3, 7, 0, 13, 65536];
function runProgram(e) {
  for (;;) {
    assert.ok(e.runs < 2000 && e.retired < 2000, 'finite program bound');
    inspectLeaf(e);
    const budget = e.mode === 'normal' ? 1 : e.mode === 'split' ? chunks[e.runs % chunks.length] : 65536;
    const stop = dispatch(e, budget);
    inspectLeaf(e);
    if (stop.reason === 1) { assert.equal(stop.retired, budget); continue; }
    if (stop.reason === 3) {
      const pc = cpu(e).pc, group = e.plan.find(g => g.entries.includes(pc)); assert.ok(group, `undeclared cold pc ${pc.toString(16)}`);
      if (e.mode === 'fault' && group.name === 'kernel') {
        assert.ok(e.abiEntry); checkRecords(e);
        status(e, 'make copied records write-only', () => e.api.protect(e.allocation, 1, 2));
      }
      compile(e, group, pc); continue;
    }
    if (stop.reason === 23) { faultRepair(e, stop); continue; }
    assert.equal(stop.reason, 8, JSON.stringify({stop, cpu: cpu(e)}));
    const frame = capture(e, stop);
    if (stop.detail === 0x10005) {
      assert.deepEqual(frame.args, [0, 128, 0x3000, 4]); complete(e, frame); e.allocation = cpu(e).registers[0];
      assert.ok(e.allocation >= 0x10000000 && e.allocation % 4096 === 0);
      assert.ok(readBytes(e, e.allocation, 128).every(x => x === 0));
      assert.equal(readWord(e, e.allocation + 128), 0); assert.equal(readWord(e, e.allocation + 4092), 0);
    } else if (stop.detail === 0x10006) {
      assert.deepEqual(frame.args, [e.allocation, 0, 0x8000]);
      checkRecords(e); assert.equal(readWord(e, e.baseAddress + e.spec.checksum_rva), e.fixture.checksum);
      save(`${e.label}-records-before-free.bin`, readBytes(e, e.allocation, 128));
      assert.throws(() => assert.equal(readWord(e, e.baseAddress + e.spec.checksum_rva), Number(wrap(BigInt(e.fixture.checksum) + 1n)), 'deliberately wrong checksum'), assert.AssertionError);
      complete(e, frame); readWord(e, e.allocation, true);
    } else {
      assert.deepEqual(frame.args, [e.fixture.checksum]); complete(e, frame, true); break;
    }
  }
  assert.equal(e.retired, 213 + (e.spec.count === 0 ? 13 : 15 + 16 * e.spec.count), 'authored main and compiled loop retirement census');
  assert.equal(e.captures, 3); assert.equal(e.completions, 3); assert.equal(e.units.length, 3);
  assert.deepEqual(e.units.map(u => u.name), ['main', 'gates', 'kernel']);
  if (e.mode === 'normal') { assert.ok(e.abiEntry && e.abiReturn); assert.equal(e.runs, e.retired + 6); }
  if (e.mode === 'fault') assert.ok(e.faulted);
  const expected = {registers: [1, e.spec.count, e.allocation + (e.spec.count === 0 ? 128 : 0), 0, e.spec.stack_base + 4096 - 8, 0, e.baseAddress + e.spec.record_rva + 128, e.allocation], pc: e.spec.gate_base + 32, flags: 0x16};
  assert.deepEqual(cpu(e), expected, 'terminal program state from compiler and authored caller');
  assert.deepEqual(arena(e).subarray(4236), initial.subarray(4236));
  e.final = {cpu: cpu(e), exit: exit(e), arena_sha256: hash(arena(e))}; save(`${e.label}-terminal-arena.bin`, arena(e));
  return e;
}

process.on('uncaughtException', error => {
  for (const e of contexts) save(`failure-${e.label}-arena.bin`, arena(e));
  save('failure-first.json', JSON.stringify({message: error.message, stack: error.stack, actual: error.actual, expected: error.expected, contexts: contexts.map(e => ({name: e.label, retired: e.retired, cpu: cpu(e), exit: exit(e)}))}, null, 2));
  console.error(error); process.exitCode = 1;
});
for (const count of fixtures.keys()) runProgram(fresh(`normal-${count}`, count));
const normal = contexts.find(e => e.label === 'normal-16'), split = fresh('split-16', 16, 'split');
const beforeZero = arena(split), zero = dispatch(split, 0); assert.equal(zero.reason, 1); assert.equal(zero.retired, 0); unchanged(beforeZero, arena(split), [[56, 96]], 'zero budget');
view(split).setUint32(split.base + 96, 1, true);
const beforeCancel = arena(split), cancelled = dispatch(split, 0); assert.equal(cancelled.reason, 2); assert.equal(cancelled.retired, 0); unchanged(beforeCancel, arena(split), [[56, 96]], 'cancel before zero budget');
view(split).setUint32(split.base + 96, 0, true);
split.controls.push({name: 'zero-cancel-split-budget', zero, cancelled, chunks}); runProgram(split);
assert.equal(split.retired, normal.retired); assert.deepEqual(split.final.cpu, normal.final.cpu); assert.deepEqual(split.final.exit, normal.final.exit);
const repaired = runProgram(fresh('fault-16', 16, 'fault'));
assert.deepEqual(repaired.final.cpu, normal.final.cpu); assert.deepEqual(repaired.final.exit, normal.final.exit);
assert.equal(contexts.length, 7); assert.equal(contexts.reduce((sum, e) => sum + e.retired, 0), 2650);
assert.equal(helperTrace.filter(row => row.name === 'cpu_store_resident32').length, 364);
assert.equal(helperTrace.filter(row => row.name === 'cpu_read32').length, 307);
assert.equal(helperTrace.filter(row => row.name === 'cpu_read8').length, 134);
assert.equal(helperTrace.filter(row => row.name === 'cpu_read16').length, 132);
assert.equal(helperTrace.length, 937);
const counts = {contexts: contexts.length, modules: contexts.reduce((sum, e) => sum + e.units.length + 1, 0), runs: contexts.reduce((sum, e) => sum + e.runs, 0), retired: contexts.reduce((sum, e) => sum + e.retired, 0), guest_helper_calls: helperTrace.length};
const expectedCounts = {contexts: 7, modules: 28, runs: 1737, retired: 2650, guest_helper_calls: 937};
assert.deepEqual(counts, expectedCounts);
const sourcesAfter = sourceMap(); assert.deepEqual(sourcesAfter, sourcesBefore);
for (const e of contexts) status(e, 'close engine', () => e.api.close());
const result = {status: 'PASS', engine_sha256: engine.sha256, engine_bytes: engine.bytes.length, command: process.argv, tools: {node: process.version, v8: process.versions.v8, platform: process.platform, arch: process.arch}, counts, expected_counts: expectedCounts, oracle: Object.fromEntries([...fixtures].map(([count, f]) => [count, f.checksum])), observations, helper_trace: helperTrace, contexts: contexts.map(e => ({label: e.label, count: e.spec.count, mode: e.mode, pe_sha256: hash(e.fixture.image), allocation: e.allocation, retired: e.retired, runs: e.runs, captures: e.captures, completions: e.completions, diagnostic_reads: e.diagnosticReads, cdecl_entry: e.abiEntry, cdecl_return: e.abiReturn, controls: e.controls, units: e.units.map(({run, ...unit}) => unit), final: e.final})), sources_before: sourcesBefore, sources_after: sourcesAfter, artifacts, claim_ceiling: 'one compiler-produced scalar leaf in an authored relocated PE; seven finite actual contexts, selected source/record/stack/canary RAM, full arena unaffected-byte and dirtyFP assertions; no CPU reseed after startup, arbitrary compiler/Windows/browser/game compatibility or performance claim'};
save('result.json', JSON.stringify(result, null, 2));
console.log(JSON.stringify({status: result.status, ...result.counts, output}));
