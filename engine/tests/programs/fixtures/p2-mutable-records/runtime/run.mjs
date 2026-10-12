import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, readdirSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {execFileSync} from 'node:child_process';
import {readEngine} from '../../../../support/wasm/engine.mjs';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(root && output);
const engine = readEngine(enginePath), SIZE = 4612, TRANSFER = 140;
const initial = readFileSync(join(output, 'initial-arena.bin'));
assert.equal(initial.length, SIZE);
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const artifacts = {}, contexts = [], observations = [], helperTrace = [];
function save(name, bytes) { writeFileSync(join(output, name), bytes, {flag: 'wx'}); artifacts[name] = {bytes: Buffer.byteLength(bytes), sha256: hash(bytes)}; }
const fixtureDir = 'engine/tests/programs/fixtures/p2-mutable-records';
const sourcePaths = [...execFileSync('rg', ['--files', 'engine/src'], {cwd: root, encoding: 'utf8'}).trim().split('\n'), 'Cargo.toml', 'Cargo.lock', 'engine/Cargo.toml', 'engine/tests/support/loader/pe32.rs', 'engine/tests/support/loader/pe32_sort.rs', 'engine/tests/support/loader/pe32_compiled_records.rs', 'engine/tests/support/loader/pe32_mutable_records.rs', 'engine/tests/programs/cases/native/pe32_mutable_records.rs', 'engine/tests/programs/native.rs', 'engine/tests/support/loader/pe32.rs', 'engine/tests/support/loader/pe32_compiled_records.rs', 'engine/tests/support/loader/pe32_mutable_records.rs', 'engine/tests/support/loader/pe32_sort.rs', 'engine/tests/programs/native.rs', 'engine/tests/programs/cases/wasm/pe32_mutable_records_wasm.rs', 'engine/tests/programs/wasm.rs', 'engine/tests/support/loader/pe32.rs', 'engine/tests/support/loader/pe32_compiled_records.rs', 'engine/tests/support/loader/pe32_mutable_records.rs', 'engine/tests/support/loader/pe32_sort.rs', 'engine/tests/support/wasm.rs', 'engine/tests/programs/wasm.rs', 'engine/tests/support/wasm.rs', 'engine/tests/support/wasm/engine.mjs', 'engine/tests/programs/fixtures/p2-compiled-records/build.py', ...readdirSync(join(root, fixtureDir), {withFileTypes: true}).filter(entry => entry.isFile()).map(entry => `${fixtureDir}/${entry.name}`)].sort();
assert.equal(new Set(sourcePaths).size, sourcePaths.length);
const sourceMap = () => Object.fromEntries(sourcePaths.map(path => { const bytes = readFileSync(join(root, path)); return [path, {bytes: bytes.length, sha256: hash(bytes)}]; }));
const sourcesBefore = sourceMap();

const MOD = 2n ** 32n, HIGH = 2n ** 27n;
const wrap = value => ((value % MOD) + MOD) % MOD;
function transform(input, count, seed) {
  const bytes = Buffer.from(input), steps = [];
  let acc = BigInt(seed);
  for (let i = 0; i < count; i++) {
    const at = i * 8, rawByte = input[at], rawWord = input[at + 2] + input[at + 3] * 256;
    const signedByte = rawByte < 128 ? rawByte : rawByte - 256, signedWord = rawWord < 32768 ? rawWord : rawWord - 65536;
    acc = (acc * 32n) % MOD + acc / HIGH;
    const rotated = Number(acc), tag = input[at + 1] ^ Number(acc % 256n);
    const weight = (input[at + 4] + input[at + 5] * 256 + Number(acc / 65536n)) % 65536;
    bytes[at + 1] = tag; bytes[at + 4] = weight % 256; bytes[at + 5] = Math.floor(weight / 256);
    const product = BigInt(signedByte) * BigInt(signedWord);
    acc = tag % 2 ? acc ^ wrap(product) : acc + product;
    acc = wrap(acc + BigInt(weight) * BigInt(tag));
    steps.push({index: i, rotated, tag, weight, product: Number(product), checksum: Number(acc)});
  }
  for (let i = 0; i < 128; i++) if (i >= count * 8 || ![1, 4, 5].includes(i % 8)) assert.equal(bytes[i], input[i], 'only selected unsigned fields may change');
  return {bytes, checksum: Number(acc), steps};
}
const literalChecksums = {0: [0xdeadbeef, 0xdeadbeef], 1: [0xd6c9676d, 0xd999551c], 2: [0xd9b234a4, 0xd1f058a0], 15: [0x5b99a2d9, 0x2ef1e62c], 16: [0x8cf0b199, 0x4438b326]};
const literalHashes = {
  0: ['71c5b89591bf15c6b81fae547ca83764e2f6d9968449142b791ddd088491199d', '71c5b89591bf15c6b81fae547ca83764e2f6d9968449142b791ddd088491199d'],
  1: ['21fe44c41ae7e58bfa9c719afa2550c1a2bc880602ce53ad022182094e8c9646', 'baa6e64bed3b9fc53dcd7daf5f502b8380cbd28f1b1a051a0167d7d6731c8bc2'],
  2: ['89201640f5ecfef295169f31a37ef1307fb644aa70fa4125a8ddd5f85edb23f9', '22fae65f73301c1562c893cf9fa9b2dae4a5f6b745ffa32cdd17c222ef9dc387'],
  15: ['18a8e2d147d7c6b50f97457744e78ac933177040e660d197cf797457fae14199', 'a161f9fcb6b9684a7febfd8df53212530fc02777de3216a144ad965b9ac0ba6a'],
  16: ['5aceb4b7ea9c42ce60cd32519469d5625fe479070ac3da0e5781a724c61f205c', '949b9f42b83f24a418be0cb2aa706e4f83f79d2aab27c900d7a0373761a1fa36'],
};
const fixtures = new Map([0, 1, 2, 15, 16].map(count => {
  const folder = join(output, `count-${count}`), spec = JSON.parse(readFileSync(join(folder, 'manifest.json'), 'utf8')), image = readFileSync(join(folder, 'records.exe'));
  assert.equal(spec.count, count); assert.equal(spec.seed, 0xdeadbeef);
  const bytes = Buffer.from(spec.record_hex, 'hex'); assert.equal(bytes.length, 128);
  assert.equal(hash(bytes), '71c5b89591bf15c6b81fae547ca83764e2f6d9968449142b791ddd088491199d');
  const passes = [transform(bytes, count, spec.seed)]; passes.push(transform(passes[0].bytes, count, passes[0].checksum));
  assert.deepEqual(passes.map(pass => pass.checksum), literalChecksums[count]);
  assert.deepEqual(passes.map(pass => hash(pass.bytes)), literalHashes[count]);
  assert.deepEqual([spec.expected_checksum1, spec.expected_checksum2], literalChecksums[count]);
  assert.deepEqual([spec.expected_records1_hex, spec.expected_records2_hex], passes.map(pass => pass.bytes.toString('hex')));
  assert.deepEqual(image.subarray(spec.text_raw, spec.text_raw + spec.program_hex.length / 2), Buffer.from(spec.program_hex, 'hex'));
  const kernel = Buffer.from(spec.kernel_hex, 'hex'); assert.equal(kernel.length, 101);
  assert.equal(hash(kernel), '600b2c46774458e4e3b1b96f032270388195c0afa65b563c9c63731623e48c5a'); assert.equal(hash(kernel), spec.kernel_sha256);
  assert.deepEqual(image.subarray(spec.text_raw + spec.kernel_offset, spec.text_raw + spec.kernel_end_offset), kernel);
  assert.deepEqual(spec.milestones.leaf_calls, [55, 72]); assert.deepEqual(spec.milestones.leaf_returns, [60, 77]);
  assert.equal(spec.milestones.leaf_entry, 512); assert.equal(spec.allocation_bytes, 128); assert.equal(spec.cdecl_arguments, 3);
  assert.equal(spec.leaf_retired, count === 0 ? 13 : 14 + 26 * count); assert.equal(spec.expected_retired, 219 + 2 * spec.leaf_retired);
  assert.deepEqual(spec.fault_store, {offset: 558, kernel_offset: 46, width: 2, base_register: 5, index_register: 6, scale: 8, displacement: 4, record_index: 3, pass: 2});
  for (const group of spec.groups) { assert.ok(group.block_count <= 8); assert.ok(group.instruction_count <= 64); }
  return [count, {spec, image, bytes, passes}];
}));

const names = ['open', 'close', 'arena_ptr', 'begin_image_input', 'append_image_input', 'load_pe32_linked_v5_input_at', 'start_loaded_image', 'dispatcher_module', 'guard_dispatch_entry', 'find_installed_resident', 'compile_resident_entries', 'acknowledge_resident_installation', 'guard_resident', 'read8', 'read16', 'read32', 'store_resident8', 'store_resident16', 'store_resident32', 'capture_resident_call', 'complete_resident_windows_call', 'protect'];
const helperKinds = {read8: [2, 1], read16: [2, 2], read32: [1, 0], store_resident8: [3, 1], store_resident16: [4, 2], store_resident32: [1, 0]};
function arena(e) { return Buffer.from(e.memory.buffer, e.base, SIZE).map(x => x); }
function view(e) { return new DataView(e.memory.buffer); }
function cpu(e) { const v = view(e); return {registers: Array.from({length: 8}, (_, i) => v.getUint32(e.base + 16 + i * 4, true)), pc: v.getUint32(e.base + 48, true), flags: v.getUint32(e.base + 52, true)}; }
function exit(e) { const v = view(e); return {version: v.getUint16(e.base + 60, true), reason: v.getUint32(e.base + 72, true), retired: v.getUint32(e.base + 76, true), detail: v.getUint32(e.base + 80, true), address: v.getUint32(e.base + 84, true), access: v.getUint32(e.base + 88, true), length: v.getUint32(e.base + 92, true)}; }
function unchanged(before, after, spans, label) { for (let i = 0; i < SIZE; i++) if (!spans.some(([a, b]) => i >= a && i < b)) assert.equal(after[i], before[i], `${label}: arena byte ${i}`); }
function status(e, name, fn, spans = []) { const before = arena(e); assert.equal(fn(), 0, name); unchanged(before, arena(e), spans, name); }
function transfer(e, bytes) { assert.ok(bytes.length <= 4096); new Uint8Array(e.memory.buffer).set(bytes, e.base + TRANSFER); }
function words(values) { const bytes = Buffer.alloc(values.length * 4); values.forEach((value, i) => bytes.writeUInt32LE(value >>> 0, i * 4)); return bytes; }
function packet(magic, version, fields) { const bytes = Buffer.alloc(40); bytes.write(magic); bytes.writeUInt16LE(version, 4); bytes.writeUInt16LE(1, 6); bytes.writeUInt32LE(40, 8); fields.forEach((value, i) => bytes.writeUInt32LE(value >>> 0, 16 + i * 4)); return bytes; }
function readWord(e, address, fault = false) {
  const before = arena(e); assert.equal(e.api.read32(address), 0); const result = arena(e), fields = Array.from({length: 6}, (_, i) => result.readUInt32LE(116 + 4 * i));
  assert.deepEqual(result.subarray(100, 140), packet('R3MH', 1, fault ? [1, 0, 1, address, 1, 4] : [0, fields[1], 0, 0, 0, 0]));
  unchanged(before, result, [[100, 140]], 'diagnostic read32'); e.diagnosticReads++; return fields[1];
}
function readBytes(e, address, length) { assert.equal(length % 4, 0); const bytes = Buffer.alloc(length); for (let i = 0; i < length; i += 4) bytes.writeUInt32LE(readWord(e, address + i), i); return bytes; }
function checkRecords(e, expected) {
  assert.deepEqual(readBytes(e, e.baseAddress + e.spec.record_rva, 128), e.fixture.bytes, 'loaded source records unchanged');
  assert.deepEqual(readBytes(e, e.allocation, 128), expected, 'live allocation has exact selected record mutations');
  assert.equal(readWord(e, e.allocation + 128), 0); assert.equal(readWord(e, e.allocation + 4092), 0); assert.equal(readWord(e, e.baseAddress + e.spec.canary_rva), 0);
}
function fresh(label, count, mode = 'normal') {
  const fixture = fixtures.get(count), {spec, image} = fixture, instance = new WebAssembly.Instance(engine.module, {}), memory = instance.exports.memory;
  const api = Object.fromEntries(names.map(name => [name, instance.exports[`ring3_abi_v1_${name}`]]));
  for (const [name, fn] of Object.entries(api)) assert.equal(typeof fn, 'function', name);
  assert.equal(api.guard_resident.length, 7);
  const e = {label, mode, fixture, spec, api, memory, low: 0xc5290000 + contexts.length + 1, high: 0xa5290000, baseAddress: spec.actual_base, units: [], groups: new Map(), table: new WebAssembly.Table({element: 'anyfunc', initial: 8, maximum: 8}), retired: 0, runs: 0, captures: 0, completions: 0, diagnosticReads: 0, controls: [], entries: [], returns: [], splitIndex: 0};
  assert.equal(api.open(8, e.low, e.high), 0); e.base = api.arena_ptr() >>> 0;
  status(e, 'begin image', () => api.begin_image_input(image.length)); transfer(e, image);
  status(e, 'append image', () => api.append_image_input(0, image.length)); transfer(e, Buffer.alloc(4096, 0xa5));
  status(e, 'load relocated image', () => api.load_pe32_linked_v5_input_at(e.baseAddress, spec.gate_base), [[TRANSFER, TRANSFER + 96]]);
  const v = view(e), at = e.base + TRANSFER;
  assert.equal(v.getUint32(at, true), 0x494c3352); assert.equal(v.getUint16(at + 4, true), 5); assert.equal(v.getUint32(at + 8, true), 96);
  assert.equal(v.getUint32(at + 16, true), e.baseAddress); assert.equal(v.getUint32(at + 24, true), e.baseAddress + spec.text_rva); assert.equal(v.getUint32(at + 36, true), 3);
  e.gates = Array.from({length: 3}, (_, i) => [v.getUint32(at + 48 + i * 8, true), v.getUint32(at + 52 + i * 8, true)]);
  assert.deepEqual(e.gates, [[spec.gate_base + 32, 0x10003], [spec.gate_base + 64, 0x10005], [spec.gate_base + 80, 0x10006]]);
  e.text = e.baseAddress + spec.text_rva;
  e.plan = [...spec.groups.map(g => ({...g, entries: g.offsets.map(pc => e.text + pc), gates: []})), {name: 'gates', entries: e.gates.map(([pc]) => pc), gates: e.gates}];
  for (const [i, imported] of spec.imports.entries()) assert.equal(readWord(e, e.baseAddress + imported.iat_rva), e.gates[i][0]);
  status(e, 'start loaded image', () => api.start_loaded_image(spec.stack_base, 1), [[16, 96]]);
  assert.deepEqual(cpu(e), {registers: [0, 0, 0, 0, spec.stack_base + 4096, 0, 0, 0], pc: e.text, flags: 2});
  const started = arena(e); new Uint8Array(memory.buffer).set(initial.subarray(4236), e.base + 4236);
  unchanged(started, arena(e), [[4236, 4364]], 'dirty fp seed after startup'); assert.deepEqual(arena(e).subarray(4236), initial.subarray(4236));
  status(e, 'dispatcher module', () => api.dispatcher_module(e.low, e.high), [[TRANSFER, TRANSFER + 32]]);
  const d = view(e), pointer = d.getUint32(e.base + TRANSFER + 24, true), length = d.getUint32(e.base + TRANSFER + 28, true);
  const bytes = Buffer.from(memory.buffer, pointer, length).map(x => x); save(`${label}-dispatcher.wasm`, bytes);
  e.dispatcher = new WebAssembly.Instance(new WebAssembly.Module(bytes), {env: {memory, table: e.table}, ring3: {guard_dispatch_entry: api.guard_dispatch_entry, find_installed_resident: api.find_installed_resident}}).exports.run;
  assert.equal(e.dispatcher.length, 4); contexts.push(e); return e;
}

function compile(e, group, pc) {
  assert.ok(group.entries.includes(pc)); assert.ok(!e.groups.has(group.name)); assert.ok(group.entries.length <= 8); assert.ok(e.units.length < 8);
  transfer(e, words([...group.entries, ...group.gates.flat()]));
  status(e, 'compile installed group', () => e.api.compile_resident_entries(group.entries.length, group.gates.length), [[TRANSFER, TRANSFER + 24]]);
  const v = view(e), at = e.base + TRANSFER;
  assert.equal(v.getUint32(at, true), 1); assert.equal(v.getUint32(at + 4, true), 24);
  const unit = {name: group.name, low: v.getUint32(at + 8, true), high: v.getUint32(at + 12, true), slot: e.units.length, pointer: v.getUint32(at + 16, true), length: v.getUint32(at + 20, true), entries: group.entries};
  assert.ok(unit.length > 8 && unit.length <= 65536 && unit.pointer + unit.length <= e.memory.buffer.byteLength);
  const bytes = Buffer.from(e.memory.buffer, unit.pointer, unit.length).map(x => x), module = new WebAssembly.Module(bytes), imports = WebAssembly.Module.imports(module), ring3 = {};
  for (const imported of imports) {
    if (imported.module === 'env') { assert.deepEqual(imported, {module: 'env', name: 'memory', kind: 'memory'}); continue; }
    assert.equal(imported.module, 'ring3'); assert.equal(imported.kind, 'function'); assert.ok(imported.name === 'guard_resident' || imported.name in helperKinds, imported.name);
    ring3[imported.name] = imported.name === 'guard_resident' ? e.api.guard_resident : (...args) => {
      const before = arena(e), state = cpu(e), result = e.api[imported.name](...args), after = arena(e); assert.equal(result, 0);
      unchanged(before, after, [[100, 140]], `forwarded ${imported.name}`);
      const [version, width] = helperKinds[imported.name], fields = Array.from({length: 6}, (_, i) => after.readUInt32LE(116 + 4 * i));
      assert.deepEqual(after.subarray(100, 140), packet('R3MH', version, fields)); assert.equal(fields[5], width);
      const address = args[imported.name.startsWith('store_') ? 4 : 0] >>> 0;
      if (fields[0] === 0) { assert.deepEqual(fields.slice(2), [0, 0, 0, width]); if (imported.name.startsWith('store_')) assert.equal(fields[1], 0); else if (width) assert.ok(fields[1] < 2 ** (width * 8)); }
      else { assert.equal(imported.name, 'store_resident16'); assert.deepEqual(fields, [1, 0, 2, e.allocation + 28, 2, 2]); assert.equal(e.mode, 'fault'); }
      helperTrace.push({context: e.label, index: helperTrace.filter(row => row.context === e.label).length + 1, unit: unit.name, pc: state.pc, pass: e.entries.length, name: imported.name, address, args: args.map(x => x >>> 0), status: result, helper_hex: after.subarray(100, 140).toString('hex')});
      return result;
    };
  }
  if (group.name === 'kernel') for (const name of ['read8', 'read16', 'read32', 'store_resident8', 'store_resident16', 'store_resident32']) assert.ok(imports.some(imported => imported.name === name), `actual compiler leaf imports ${name}`);
  unit.run = new WebAssembly.Instance(module, {env: {memory: e.memory}, ring3}).exports.run; assert.equal(unit.run.length, 4); e.table.set(unit.slot, unit.run);
  status(e, 'acknowledge installed group', () => e.api.acknowledge_resident_installation(e.low, e.high, unit.low, unit.high, unit.slot), [[TRANSFER, TRANSFER + 32]]);
  const receipt = arena(e).subarray(TRANSFER, TRANSFER + 32);
  assert.equal(receipt.subarray(0, 4).toString(), 'R3IN'); assert.equal(receipt.readUInt32LE(4), 0x10001); assert.equal(receipt.readUInt32LE(8), 32);
  assert.deepEqual([receipt.readUInt32LE(16), receipt.readUInt32LE(20), receipt.readUInt32LE(24)], [unit.low, unit.high, unit.slot]);
  const file = `${e.label}-unit-${unit.slot}-${unit.name}.wasm`; save(file, bytes); e.units.push({...unit, run: unit.run, file, sha256: hash(bytes), imports}); e.groups.set(group.name, unit);
}
function dispatch(e, budget) {
  const before = arena(e), start = cpu(e), result = e.dispatcher(e.base, e.base + 56, budget, e.base + 96); assert.equal(result, 0);
  const after = arena(e); unchanged(before, after, [[16, 96], [100, 172]], 'dispatcher');
  const stop = exit(e), state = cpu(e); assert.ok(stop.retired <= budget); e.retired += stop.retired; e.runs++;
  observations.push({context: e.label, index: e.runs, budget, start_pc: start.pc, status: result, ...stop, pc: state.pc, flags: state.flags, registers: state.registers}); return stop;
}
function inspectLeaf(e) {
  const state = cpu(e), m = e.spec.milestones;
  if (state.pc === e.text + m.leaf_entry && e.entries.length === e.returns.length) {
    const pass = e.entries.length; assert.ok(pass < 2);
    const seed = pass === 0 ? e.spec.seed : e.returns[0].cpu.registers[0], beforeBytes = pass === 0 ? e.fixture.bytes : e.fixture.passes[0].bytes;
    assert.deepEqual(readBytes(e, state.registers[4], 16), words([e.text + m.leaf_returns[pass], e.allocation, e.spec.count, seed]));
    assert.equal(state.registers[4], e.spec.stack_base + 4096 - 16); checkRecords(e, beforeBytes);
    e.entries.push({pass: pass + 1, cpu: state, seed, records_sha256: hash(beforeBytes), retired_before: e.retired});
  }
  const pass = e.returns.length;
  if (pass < 2 && e.entries.length === pass + 1 && state.pc === e.text + m.leaf_returns[pass]) {
    const expected = e.fixture.passes[pass]; assert.equal(state.registers[0], expected.checksum);
    for (const register of [3, 5, 6, 7]) assert.equal(state.registers[register], e.entries[pass].cpu.registers[register], `cdecl preserved register ${register}`);
    assert.equal(state.registers[4], e.entries[pass].cpu.registers[4] + 4); assert.equal(state.flags, 0x46);
    assert.equal(e.retired - e.entries[pass].retired_before, e.spec.leaf_retired); checkRecords(e, expected.bytes);
    const recordsFile = `${e.label}-pass-${pass + 1}-records.bin`, arenaFile = `${e.label}-pass-${pass + 1}-return-arena.bin`;
    save(recordsFile, readBytes(e, e.allocation, 128)); save(arenaFile, arena(e));
    e.returns.push({pass: pass + 1, cpu: state, checksum: state.registers[0], records_file: recordsFile, records_sha256: hash(expected.bytes), arena_file: arenaFile, retired_after: e.retired});
    assert.throws(() => assert.equal(state.registers[0], Number(wrap(BigInt(expected.checksum) + 1n)), 'deliberately wrong checksum'), assert.AssertionError);
  }
}
function capture(e, stop) {
  const owner = e.groups.get('gates'); assert.ok(owner);
  const count = stop.detail === 0x10005 ? 4 : stop.detail === 0x10006 ? 3 : stop.detail === 0x10003 ? 1 : assert.fail('unknown provider');
  const state = cpu(e), m = e.spec.milestones, returnPc = e.text + (count === 4 ? m.alloc_return : count === 3 ? m.free_return : m.exit_return);
  const receipt = arena(e).subarray(TRANSFER, TRANSFER + 32); assert.equal(receipt.subarray(0, 4).toString(), 'R3IN'); assert.deepEqual([receipt.readUInt32LE(16), receipt.readUInt32LE(20), receipt.readUInt32LE(24)], [owner.low, owner.high, owner.slot]);
  status(e, 'capture named provider', () => e.api.capture_resident_call(e.low, e.high, owner.low, owner.high, 2, count), [[TRANSFER, TRANSFER + 112]]);
  const frame = arena(e).subarray(TRANSFER, TRANSFER + 112), token = frame.readUInt32LE(16), args = Array.from({length: count}, (_, i) => frame.readUInt32LE(48 + 4 * i));
  assert.equal(frame.subarray(0, 4).toString(), 'R3CF'); assert.equal(frame.readUInt32LE(4), 0x10001); assert.equal(frame.readUInt32LE(8), 112); assert.equal(token, ++e.captures);
  assert.deepEqual(Array.from({length: 7}, (_, i) => frame.readUInt32LE(20 + 4 * i)), [stop.detail, 2, count, state.pc, state.registers[4], returnPc, 0]);
  assert.ok(frame.subarray(48 + 4 * count).every(x => x === 0)); assert.equal(state.registers[4], e.spec.stack_base + 4096 - 4 * (count + 1));
  assert.deepEqual(readBytes(e, state.registers[4], 4 * (count + 1)), words([returnPc, ...args])); save(`${e.label}-provider-${e.captures}.bin`, frame);
  return {owner, token, args, cpu: state, returnPc};
}
function complete(e, frame, terminal = false) {
  status(e, 'complete named provider', () => e.api.complete_resident_windows_call(e.low, e.high, frame.owner.low, frame.owner.high, frame.token), [[16, 96]]);
  e.completions++; const stop = exit(e), state = cpu(e); assert.equal(stop.retired, 0);
  if (terminal) { assert.equal(stop.reason, 9); assert.equal(stop.detail, e.fixture.passes[1].checksum); assert.deepEqual(state, frame.cpu); }
  else { const registers = [...frame.cpu.registers]; registers[0] = frame.args.length === 4 ? state.registers[0] : 1; registers[4] = e.spec.stack_base + 4096; assert.deepEqual(state, {registers, pc: frame.returnPc, flags: frame.cpu.flags}); assert.equal(stop.reason, 3); }
}

function faultRepair(e) {
  assert.equal(e.mode, 'fault'); assert.ok(!e.faulted); assert.equal(e.entries.length, 2); assert.equal(e.returns.length, 1); e.faulted = true;
  const stopped = cpu(e), step = e.fixture.passes[1].steps[3], partial = Buffer.from(e.fixture.passes[0].bytes);
  e.fixture.passes[1].bytes.copy(partial, 0, 0, 24); partial[25] = step.tag;
  assert.equal(hash(partial), '6089d05a9cf2de5cacf254642e09f5888d045042a934fb0bdd2d990f0e457a8b'); assert.equal(partial.subarray(24, 32).toString('hex'), '019e0080313b04a0');
  assert.deepEqual(stopped, {registers: [3293479044, 16, 16, 158, e.spec.stack_base + 4096 - 32, e.allocation, 3, 65407], pc: e.text + e.spec.fault_store.offset, flags: 0x82});
  assert.equal(step.rotated, stopped.registers[0]); assert.equal(step.tag, stopped.registers[3]); assert.equal(step.weight, stopped.registers[7]); checkRecords(e, partial);
  const successfulStores = helperTrace.filter(row => row.context === e.label && ['store_resident8', 'store_resident16'].includes(row.name));
  assert.equal(successfulStores.length, 39); assert.equal(successfulStores.filter(row => row.pass === 2).length, 7);
  assert.equal(successfulStores.at(-1).name, 'store_resident8'); assert.equal(successfulStores.at(-1).address, e.allocation + 25);
  const installed = e.units.map(({low, high, slot, sha256}) => ({low, high, slot, sha256}));
  status(e, 'make live record allocation read-only', () => e.api.protect(e.allocation, 1, 1)); assert.deepEqual(cpu(e), stopped);
  const before = arena(e), expected = Buffer.from(before), address = e.allocation + 28;
  packet('R3EX', 2, [5, 0, 2, address, 2, 2]).copy(expected, 56); packet('R3MH', 4, [1, 0, 2, address, 2, 2]).copy(expected, 100);
  const first = dispatch(e, 1); assert.deepEqual(first, {version: 2, reason: 5, retired: 0, detail: 2, address, access: 2, length: 2});
  assert.deepEqual(arena(e), expected, 'failed word instruction only publishes its canonical exit/helper'); assert.deepEqual(cpu(e), stopped);
  save(`${e.label}-word-fault-first-arena.bin`, arena(e)); assert.deepEqual(readBytes(e, e.allocation, 128), partial);
  const retry = dispatch(e, 1); assert.deepEqual(retry, first); assert.deepEqual(arena(e), expected, 'unchanged word fault retry is neutral'); assert.deepEqual(cpu(e), stopped);
  save(`${e.label}-word-fault-retry-arena.bin`, arena(e)); assert.deepEqual(readBytes(e, e.allocation, 128), partial); save(`${e.label}-word-fault-records.bin`, partial);
  status(e, 'repair live allocation by protection only', () => e.api.protect(e.allocation, 1, 3)); assert.deepEqual(cpu(e), stopped); checkRecords(e, partial);
  for (const unit of e.units) status(e, 'same installed unit remains current', () => e.api.guard_resident(e.low, e.high, unit.low, unit.high, e.base, e.base + 56, e.base + 96));
  assert.deepEqual(e.units.map(({low, high, slot, sha256}) => ({low, high, slot, sha256})), installed);
  e.controls.push({name: 'second-pass-record3-word-store-fault', first, retry, stopped, partial_records_sha256: hash(partial), partial_records_file: `${e.label}-word-fault-records.bin`, installed, repair: 'protect ReadWrite only; preceding byte and earlier records stay committed'});
}
const chunks = [0, 1, 3, 7, 0, 13, 65536], copyOffsets = [34, 36, 38, 41, 44, 45];
function runProgram(e) {
  for (;;) {
    assert.ok(e.runs < 2000 && e.retired < 2000, 'finite program bound'); inspectLeaf(e);
    const state = cpu(e), offset = state.pc - e.text;
    if (e.mode === 'fault' && !e.faulted && e.entries.length === 2 && state.pc === e.text + e.spec.fault_store.offset && state.registers[6] === 3) { faultRepair(e); continue; }
    const copyIndex = copyOffsets.indexOf(offset), budget = e.mode === 'split' && copyIndex >= 0 ? Math.min(chunks[e.splitIndex++ % chunks.length], copyOffsets.length - copyIndex) : 1;
    const stop = dispatch(e, budget); inspectLeaf(e);
    if (stop.reason === 1) { assert.equal(stop.retired, budget); continue; }
    if (stop.reason === 3) { const pc = cpu(e).pc, group = e.plan.find(g => g.entries.includes(pc)); assert.ok(group, `undeclared cold pc ${pc.toString(16)}`); compile(e, group, pc); continue; }
    assert.equal(stop.reason, 8, JSON.stringify({stop, cpu: cpu(e)}));
    const frame = capture(e, stop);
    if (stop.detail === 0x10005) {
      assert.deepEqual(frame.args, [0, 128, 0x3000, 4]); complete(e, frame); e.allocation = cpu(e).registers[0]; assert.ok(e.allocation >= 0x10000000 && e.allocation % 4096 === 0);
      assert.ok(readBytes(e, e.allocation, 128).every(x => x === 0)); assert.equal(readWord(e, e.allocation + 128), 0); assert.equal(readWord(e, e.allocation + 4092), 0);
    } else if (stop.detail === 0x10006) {
      assert.deepEqual(frame.args, [e.allocation, 0, 0x8000]); assert.equal(e.returns.length, 2); checkRecords(e, e.fixture.passes[1].bytes);
      assert.equal(readWord(e, e.baseAddress + e.spec.checksum1_rva), e.fixture.passes[0].checksum); assert.equal(readWord(e, e.baseAddress + e.spec.checksum2_rva), e.fixture.passes[1].checksum);
      save(`${e.label}-records-before-free.bin`, readBytes(e, e.allocation, 128)); complete(e, frame); readWord(e, e.allocation, true);
    } else { assert.deepEqual(frame.args, [e.fixture.passes[1].checksum]); complete(e, frame, true); break; }
  }
  assert.equal(e.retired, e.spec.expected_retired); assert.equal(e.entries.length, 2); assert.equal(e.returns.length, 2);
  assert.equal(e.captures, 3); assert.equal(e.completions, 3); assert.equal(e.units.length, 3); assert.deepEqual(e.units.map(unit => unit.name), ['main', 'gates', 'kernel']);
  if (e.mode === 'normal') assert.equal(e.runs, e.retired + 6); if (e.mode === 'fault') { assert.ok(e.faulted); assert.equal(e.runs, e.retired + 8); }
  const expected = {registers: [1, e.spec.count, e.spec.count === 0 ? e.allocation + 128 : e.spec.count, 0, e.spec.stack_base + 4096 - 8, 0, e.baseAddress + e.spec.record_rva + 128, e.allocation], pc: e.spec.gate_base + 32, flags: 0x16};
  assert.deepEqual(cpu(e), expected); assert.deepEqual(arena(e).subarray(4236), initial.subarray(4236));
  e.final = {cpu: cpu(e), exit: exit(e), arena_sha256: hash(arena(e)), record_sha256: hash(e.fixture.passes[1].bytes)}; save(`${e.label}-terminal-arena.bin`, arena(e)); return e;
}

process.on('uncaughtException', error => {
  for (const e of contexts) save(`failure-${e.label}-arena.bin`, arena(e));
  save('failure-first.json', JSON.stringify({message: error.message, stack: error.stack, actual: error.actual, expected: error.expected, contexts: contexts.map(e => ({name: e.label, retired: e.retired, cpu: cpu(e), exit: exit(e)}))}, null, 2));
  console.error(error); process.exitCode = 1;
});
for (const count of fixtures.keys()) runProgram(fresh(`normal-${count}`, count));
const normal = contexts.find(e => e.label === 'normal-16'), split = fresh('split-16', 16, 'split');
const beforeZero = arena(split), zero = dispatch(split, 0); assert.equal(zero.reason, 1); assert.equal(zero.retired, 0); unchanged(beforeZero, arena(split), [[56, 96]], 'zero budget');
view(split).setUint32(split.base + 96, 1, true); const beforeCancel = arena(split), cancelled = dispatch(split, 0);
assert.equal(cancelled.reason, 2); assert.equal(cancelled.retired, 0); unchanged(beforeCancel, arena(split), [[56, 96]], 'cancel before zero budget'); view(split).setUint32(split.base + 96, 0, true);
split.controls.push({name: 'copy-loop-only-split-budget', zero, cancelled, chunks, copy_instruction_offsets: copyOffsets, cap: 'remaining instructions through current copy-loop JNE; all calls and leaves single-step'}); runProgram(split);
assert.equal(split.splitIndex, 76); assert.equal(split.runs, 971); assert.deepEqual(split.final.cpu, normal.final.cpu); assert.deepEqual(split.final.exit, normal.final.exit);
const repaired = runProgram(fresh('fault-16', 16, 'fault')); assert.deepEqual(repaired.final.cpu, normal.final.cpu); assert.deepEqual(repaired.final.exit, normal.final.exit);
for (const e of [split, repaired]) assert.deepEqual(e.returns.map(row => [row.checksum, row.records_sha256, row.cpu]), normal.returns.map(row => [row.checksum, row.records_sha256, row.cpu]));
const helperCounts = Object.fromEntries(Object.keys(helperKinds).map(name => [name, helperTrace.filter(row => row.name === name).length]));
assert.deepEqual(helperCounts, {read8: 264, read16: 264, read32: 482, store_resident8: 132, store_resident16: 134, store_resident32: 427});
const counts = {contexts: contexts.length, modules: contexts.reduce((sum, e) => sum + e.units.length + 1, 0), runs: observations.length, retired: contexts.reduce((sum, e) => sum + e.retired, 0), guest_helper_calls: helperTrace.length, natural_leaf_returns: contexts.reduce((sum, e) => sum + e.returns.length, 0), write_faults: observations.filter(row => row.reason === 5).length};
const expectedCounts = {contexts: 7, modules: 28, runs: 5089, retired: 5159, guest_helper_calls: 1703, natural_leaf_returns: 14, write_faults: 2}; assert.deepEqual(counts, expectedCounts);
const sourcesAfter = sourceMap(); assert.deepEqual(sourcesAfter, sourcesBefore); for (const e of contexts) status(e, 'close engine', () => e.api.close());
const result = {status: 'PASS', engine_sha256: engine.sha256, engine_bytes: engine.bytes.length, command: process.argv, tools: {node: process.version, v8: process.versions.v8, platform: process.platform, arch: process.arch}, counts, expected_counts: expectedCounts, helper_counts: helperCounts, oracle: Object.fromEntries([...fixtures].map(([count, f]) => [count, f.passes.map(pass => ({checksum: pass.checksum, records_sha256: hash(pass.bytes)}))])), observations, helper_trace: helperTrace, contexts: contexts.map(e => ({label: e.label, count: e.spec.count, mode: e.mode, pe_sha256: hash(e.fixture.image), allocation: e.allocation, retired: e.retired, runs: e.runs, captures: e.captures, completions: e.completions, diagnostic_reads: e.diagnosticReads, cdecl_entries: e.entries, cdecl_returns: e.returns, controls: e.controls, units: e.units.map(({run, ...unit}) => unit), final: e.final})), sources_before: sourcesBefore, sources_after: sourcesAfter, artifacts, claim_ceiling: 'one fixed-profile compiler-produced mutable scalar leaf called twice in an authored relocated PE; seven finite contexts, selected source/record/stack/canary RAM and full4612 unaffected-byte/dirtyFP assertions; copy-loop split budgets with single-step leaves; no CPU/RAM reseed after startup, whole-record/leaf transaction, arbitrary compiler/Windows/browser/game/SDK/performance/fullRAM claim'};
save('result.json', JSON.stringify(result, null, 2)); console.log(JSON.stringify({status: result.status, ...result.counts, output}));
