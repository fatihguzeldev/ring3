import assert from 'node:assert/strict';
import {ARENA_SIZE} from '../../../../../support/wasm/arena.mjs';
import {createHash} from 'node:crypto';
import {execFileSync} from 'node:child_process';
import {readFileSync, readdirSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../../../../../support/wasm/engine.mjs';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root);
const engine = readEngine(enginePath), SIZE = ARENA_SIZE, TRANSFER = 140;
const initial = readFileSync(join(output, 'initial-arena.bin')), image = readFileSync(join(output, 'switch.exe'));
const spec = JSON.parse(readFileSync(join(output, 'manifest.json'), 'utf8'));
assert.equal(initial.length, SIZE);
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const artifacts = {}, contexts = [], modules = [], observations = [], helperTrace = [], diagnostics = [];
function save(name, bytes) { writeFileSync(join(output, name), bytes, {flag: 'wx'}); artifacts[name] = {bytes: bytes.length, sha256: hash(bytes)}; }
const dir = 'engine/tests/programs/dispatch/fixtures/p2-compiled-switch';
const paths = [...execFileSync('rg', ['--files', 'engine/src'], {cwd: root, encoding: 'utf8'}).trim().split('\n'),
  'Cargo.toml', 'Cargo.lock', 'engine/Cargo.toml', 'rust-toolchain.toml', 'engine/tests/support/loader/pe32/image.rs',
  'engine/tests/programs/dispatch/support/switch.rs', 'engine/tests/programs/dispatch/native/switch.rs', 'engine/tests/programs/native.rs', 'engine/tests/programs/dispatch/support/switch.rs', 'engine/tests/support/loader/pe32/image.rs', 'engine/tests/programs/native.rs', 'engine/tests/programs/dispatch/wasm/switch.rs', 'engine/tests/programs/wasm.rs', 'engine/tests/programs/dispatch/support/switch.rs', 'engine/tests/support/loader/pe32/image.rs', 'engine/tests/support/wasm.rs', 'engine/tests/programs/wasm.rs', 'engine/tests/support/wasm.rs',
  'engine/tests/support/wasm/engine.mjs', ...readdirSync(join(root, dir), {withFileTypes: true}).filter(x => x.isFile()).map(x => `${dir}/${x.name}`)].sort();
const sources = () => Object.fromEntries(paths.map(path => { const bytes = readFileSync(join(root, path)); return [path, {bytes: bytes.length, sha256: hash(bytes)}]; }));
const pins = sources(); save('engine.wasm', engine.bytes);
assert.deepEqual(spec.selectors, [0, 1, 2, 3, 4, 0xffffffff]);
assert.deepEqual(spec.main_entries, [0, 34, 59, 84, 109, 134, 159]);
assert.deepEqual(spec.call_offsets, [29, 54, 79, 104, 129, 154]);
assert.deepEqual(spec.return_offsets, [34, 59, 84, 109, 134, 159]);
assert.deepEqual(spec.block_instructions, [9, 8, 8, 8, 8, 8, 6]);
assert.deepEqual(spec.kernel_entries, [0, 20, 27, 32, 37, 43, 51, 53]);
assert.deepEqual(spec.table_targets, [27, 43, 32, 37]);
const exitReturnOffset = Buffer.from(spec.main_hex, 'hex').length;
assert.equal(exitReturnOffset, 182);
for (const [name, hex, raw] of [['kernel.text.bin', spec.kernel_linked_hex, 0x600], ['kernel.rdata.bin', spec.table_linked_hex, 0xa00]]) {
  const bytes = readFileSync(join(root, dir, name)); assert.deepEqual(bytes, Buffer.from(hex, 'hex')); assert.deepEqual(image.subarray(raw, raw + bytes.length), bytes);
}
assert.deepEqual(image.subarray(0x400, 0x400 + exitReturnOffset), Buffer.from(spec.main_hex, 'hex'));
const MOD = 2n ** 32n, wrap = value => (value % MOD + MOD) % MOD;
const selectors = [0, 1, 2, 3, 4, 0xffffffff];
const literals = [[3, 0xdeadbeec], [0xa5a55a59, 0x7b08e4b5], [0xb4ab4b34, 0xcfa3af81],
  [0x1e01e19d, 0xd1a24e1c], [0x1e01e199, 0xcfa3af85], [0x1e01e19a, 0xd1a24e1f]];
function transform(value, selector, salt) {
  let next = BigInt(value);
  if (selector === 0) next += 7n;
  else if (selector === 1) next ^= 0xa5a55a5an;
  else if (selector === 2) next = next * 32n % MOD + next / (2n ** 27n);
  else if (selector === 3) next = next * 3n + 1n;
  else next -= BigInt(selector);
  next = wrap(next); return [Number(next), Number(next ^ BigInt(salt))];
}
let value = 0xfffffffc, salt = 0xdeadbeef;
const cases = selectors.map((selector, i) => { const pair = transform(value, selector, salt); assert.deepEqual(pair, literals[i]); const row = {selector, before: value, salt, value: pair[0], result: pair[1]}; [value, salt] = pair; return row; });
function parity(value) { let bits = 0, byte = value % 256; for (let i = 0; i < 8; i++) {bits += byte % 2; byte = Math.floor(byte / 2);} return bits % 2 === 0; }
const logicalFlags = value => 2 + (parity(value) ? 4 : 0) + (value === 0 ? 64 : 0) + (value >= 2 ** 31 ? 128 : 0);
assert.deepEqual(cases.map(c => logicalFlags(c.result)), [0x82, 2, 0x86, 0x82, 0x82, 0x82]);
function words(values) { const b = Buffer.alloc(values.length * 4); values.forEach((v, i) => b.writeUInt32LE(v >>> 0, i * 4)); return b; }
function record(magic, length, fields, version = 1) { const b = Buffer.alloc(length); b.write(magic); b.writeUInt16LE(version, 4); b.writeUInt16LE(1, 6); b.writeUInt32LE(length, 8); b.set(words(fields), 16); return b; }
const stateBytes = s => record('R3ST', 56, [...s.registers, s.pc, s.flags]);
const exitBytes = s => record('R3EX', 40, [s.reason, s.retired, s.detail ?? 0, s.address ?? 0, s.access ?? 0, s.length ?? 0], s.version);
const helperBytes = (value = 0, address = null) => record('R3MH', 40, address === null ? [0, value, 0, 0, 0, 0] : [1, 0, 2, address, 1, 4]);
const cpuHelperBytes = (value = 0, address = null) => record('R3MH', 40, address === null ? [0, value, 0, 0, 0, 4] : [1, 0, 2, address, 1, 4], 10);
const installBytes = u => record('R3IN', 32, [u.low, u.high, u.slot, 0]);
function arena(e) { return Buffer.from(new Uint8Array(e.memory.buffer, e.base, SIZE)); }
function cpu(e) { const b = arena(e); return {registers: Array.from({length: 8}, (_, i) => b.readUInt32LE(16 + i * 4)), pc: b.readUInt32LE(48), flags: b.readUInt32LE(52)}; }
function stop(e) { const b = arena(e); return {version: b.readUInt16LE(60), reason: b.readUInt32LE(72), retired: b.readUInt32LE(76), detail: b.readUInt32LE(80), address: b.readUInt32LE(84), access: b.readUInt32LE(88), length: b.readUInt32LE(92)}; }
function unchanged(a, b, spans, label) { for (let i = 0; i < SIZE; i++) if (!spans.some(([x, y]) => i >= x && i < y)) assert.equal(b[i], a[i], `${label}: arena ${i}`); }
function api(e, name, args, spans = []) { const before = arena(e); assert.equal(e.api[name](...args), 0, name); unchanged(before, arena(e), spans, name); }
function transfer(e, bytes) { assert.ok(bytes.length <= 4096); new Uint8Array(e.memory.buffer).set(bytes, e.base + TRANSFER); }
const arities = {open: 3, close: 0, arena_ptr: 0, begin_image_input: 1, append_image_input: 2, load_pe32_linked_v2_input_at: 2,
  start_loaded_image: 2, compile_entries: 2, compile_resident_entries: 2, generation: 0, module_ptr: 0, module_len: 0,
  guard: 6, guard_resident: 7, read32: 1, store32: 2, store_resident32: 6, protect: 3,
  capture_call: 5, capture_resident_call: 6, complete_windows_call: 4, complete_resident_windows_call: 5,
  acknowledge_resident_installation: 5, dispatcher_module: 2, guard_dispatch_entry: 5, find_installed_resident: 3, cpu_read32: 1, cpu_store32: 2, cpu_store_resident32: 6};
function fresh(owner, address) {
  const instance = new WebAssembly.Instance(engine.module, {}), functions = Object.fromEntries(Object.entries(arities).map(([name, arity]) => {
    const fn = instance.exports['ring3_abi_v1_' + name]; assert.equal(fn?.length, arity, name); return [name, fn];
  }));
  const e = {owner, address, label: `${owner}-${address.toString(16)}`, api: functions, memory: instance.exports.memory,
    low: 0x53400001 + contexts.length, high: 0xa5340000, captures: 0, runs: 0, retired: 0, reads: 0, stores: 0, diagnostics: 0,
    top: spec.stack_base + 4096, main: address + spec.main_rva, kernel: address + spec.kernel_rva,
    state: address + spec.state_rva, output: address + spec.output_rva, tableAddress: address + spec.table_rva,
    gate: spec.gate_base + spec.gate_offset, units: [], table: owner === 'resident' ? new WebAssembly.Table({element: 'anyfunc', initial: 8, maximum: 8}) : null};
  assert.equal(e.api.open(8, e.low, e.high), 0); e.base = e.api.arena_ptr() >>> 0; contexts.push(e);
  api(e, 'begin_image_input', [image.length]); transfer(e, image); api(e, 'append_image_input', [0, image.length]); transfer(e, Buffer.alloc(4096, 0xa5));
  api(e, 'load_pe32_linked_v2_input_at', [address, spec.gate_base], [[TRANSFER, TRANSFER + 72]]);
  assert.deepEqual(arena(e).subarray(TRANSFER, TRANSFER + 72), record('R3LI', 72, [address, 0x6000, e.main, 7, spec.gate_base, 1, 0, 0, e.gate, 0x10003, 0, 0, 0, 0], 2));
  api(e, 'start_loaded_image', [spec.stack_base, 1], [[16, 96]]);
  assert.deepEqual(cpu(e), {registers: [0, 0, 0, 0, e.top, 0, 0, 0], pc: e.main, flags: 2});
  const before = arena(e); new Uint8Array(e.memory.buffer).set(initial.subarray(4236), e.base + 4236); unchanged(before, arena(e), [[4236, SIZE]], 'dirty fp after startup');
  if (e.table) {
    api(e, 'dispatcher_module', [e.low, e.high], [[TRANSFER, TRANSFER + 32]]); const r = arena(e).subarray(TRANSFER, TRANSFER + 32);
    assert.deepEqual(r.subarray(0, 24), record('R3DP', 32, [e.low, e.high]).subarray(0, 24));
    assert.equal(r.readUInt32LE(8), 32);
    const bytes = Buffer.from(new Uint8Array(e.memory.buffer, r.readUInt32LE(24), r.readUInt32LE(28))), module = new WebAssembly.Module(bytes), file = `${e.label}-dispatcher.wasm`;
    assert.deepEqual(WebAssembly.Module.imports(module), [{module: 'env', name: 'memory', kind: 'memory'}, {module: 'env', name: 'table', kind: 'table'}, ...['guard_dispatch_entry', 'find_installed_resident'].map(name => ({module: 'ring3', name, kind: 'function'}))]);
    e.dispatcher = new WebAssembly.Instance(module, {env: {memory: e.memory, table: e.table}, ring3: e.api}).exports.run; assert.equal(e.dispatcher.length, 4);
    save(file, bytes); modules.push({context: e.label, name: 'dispatcher', file, bytes: bytes.length, sha256: hash(bytes)});
  }
  save(`${e.label}-start.bin`, arena(e)); return e;
}
function compile(e, name, pcs, gates, instructions) {
  assert.ok(pcs.length > 0 && pcs.length <= 8 && instructions + gates.length <= 64); assert.ok(++e.captures <= (e.table ? 3 : 7));
  transfer(e, words([...pcs, ...gates.flat()])); api(e, e.table ? 'compile_resident_entries' : 'compile_entries', [pcs.length, gates.length], e.table ? [[TRANSFER, TRANSFER + 24]] : []);
  const r = arena(e).subarray(TRANSFER, TRANSFER + 24), u = {name, slot: e.units.length, pcs, instructions, version: 6};
  if (e.table) {assert.equal(r.readUInt32LE(0), 1); assert.equal(r.readUInt32LE(4), 24); [u.low, u.high, u.pointer, u.length] = [8, 12, 16, 20].map(at => r.readUInt32LE(at));}
  else {u.generation = e.api.generation(); u.pointer = e.api.module_ptr() >>> 0; u.length = e.api.module_len() >>> 0; assert.equal(u.generation, e.captures);}
  assert.ok(u.length > 8 && u.length <= 65536); const bytes = Buffer.from(new Uint8Array(e.memory.buffer, u.pointer, u.length)), module = new WebAssembly.Module(bytes);
  const guard = e.table ? 'guard_resident' : 'guard', store = e.table ? 'cpu_store_resident32' : 'cpu_store32', imports = WebAssembly.Module.imports(module);
  assert.deepEqual(imports, [{module: 'env', name: 'memory', kind: 'memory'}, ...[guard, 'cpu_read32', store].map(name => ({module: 'ring3', name, kind: 'function'}))]);
  const ring3 = {[guard]: e.api[guard]};
  for (const name of ['cpu_read32', store]) ring3[name] = (...args) => {
    const step = e.steps.shift(); assert.ok(step, `unexpected ${name}`); assert.equal(name === 'cpu_read32' ? 'read' : 'store', step[0]);
    assert.equal(args[name === 'cpu_store_resident32' ? 4 : 0] >>> 0, step[1]);
    if (name !== 'cpu_read32') assert.equal(args.at(-1) >>> 0, step[2]);
    if (name === 'cpu_store_resident32') assert.deepEqual(args.slice(0, 4).map(x => x >>> 0), [e.low, e.high, u.low, u.high]);
    const before = arena(e), status = e.api[name](...args), after = arena(e); assert.equal(status, 0); unchanged(before, after, [[100, 140]], 'canonical helper');
    assert.deepEqual(after.subarray(100, 140), cpuHelperBytes(name === 'cpu_read32' ? step[2] : 0, step[3] ? step[1] : null));
    helperTrace.push({context: e.label, call: e.runs + 1, unit: name, module: u.name, address: step[1], value: step[2], fault: !!step[3], status, helper_hex: after.subarray(100, 140).toString('hex')});
    if (name === 'cpu_read32') e.reads++; else e.stores++; return status;
  };
  u.run = new WebAssembly.Instance(module, {env: {memory: e.memory}, ring3}).exports.run; assert.equal(u.run.length, 4);
  if (e.table) {assert.ok(u.slot < 8); assert.equal(e.table.get(u.slot), null); e.table.set(u.slot, u.run); api(e, 'acknowledge_resident_installation', [e.low, e.high, u.low, u.high, u.slot], [[TRANSFER, TRANSFER + 32]]); assert.deepEqual(arena(e).subarray(TRANSFER, TRANSFER + 32), installBytes(u));}
  const file = `${e.label}-${e.captures}-${name}.wasm`; save(file, bytes); e.units.push(u);
  modules.push({context: e.label, name, file, sha256: hash(bytes), bytes: bytes.length, entries: pcs, gates, ordinary_instructions: instructions, instructions: instructions + gates.length, imports, generation: u.generation ?? null, id: e.table ? [u.low, u.high] : null, slot: e.table ? u.slot : null}); return u;
}
function run(e, u, label, budget, next, retired, steps = [], reason = 1, fields = {}) {
  const before = arena(e), expected = Buffer.from(before), packet = {version: u.version, reason, retired, ...fields};
  expected.set(stateBytes(next)); expected.set(exitBytes(packet), 56);
  if (steps.length) {const last = steps.at(-1); expected.set(cpuHelperBytes(last[0] === 'read' ? last[2] : 0, last[3] ? last[1] : null), 100);}
  if (e.table && budget && !before.readUInt32LE(96)) expected.set(installBytes(u), TRANSFER);
  e.steps = [...steps]; const status = (e.table ? e.dispatcher : u.run)(e.base, e.base + 56, budget, e.base + 96); assert.equal(status, 0, label); assert.equal(e.steps.length, 0, label);
  assert.deepEqual(arena(e), expected, `${e.label}/${label}: full4612`); e.runs++; e.retired += retired;
  const file = `${e.label}-${String(e.runs).padStart(2, '0')}-${label}.bin`; save(file, arena(e));
  observations.push({context: e.label, label, module: u.name, budget, expected: {status: 0, ...packet, pc: next.pc, flags: next.flags, registers: next.registers},
    observed: {status, ...stop(e), pc: cpu(e).pc, flags: cpu(e).flags, registers: cpu(e).registers}, arena: file, helper_calls: steps.length});
}
function read(e, address, value) { const before = arena(e); assert.equal(e.api.read32(address), 0); const after = arena(e); unchanged(before, after, [[100, 140]], 'public diagnostic'); assert.deepEqual(after.subarray(100, 140), helperBytes(value)); e.diagnostics++; diagnostics.push({context: e.label, address, value}); }
function guard(e, u) { api(e, e.table ? 'guard_resident' : 'guard', e.table ? [e.low, e.high, u.low, u.high, e.base, e.base + 56, e.base + 96] : [e.low, e.high, u.generation, e.base, e.base + 56, e.base + 96]); }
function finish(e, u) {
  const before = cpu(e), returned = e.main + exitReturnOffset;
  api(e, e.table ? 'capture_resident_call' : 'capture_call', e.table ? [e.low, e.high, u.low, u.high, 2, 1] : [e.low, e.high, u.generation, 2, 1], [[TRANSFER, TRANSFER + 112]]);
  assert.deepEqual(arena(e).subarray(TRANSFER, TRANSFER + 112), record('R3CF', 112, [1, 0x10003, 2, 1, e.gate, e.top - 8, returned, 0, cases.at(-1).result, ...Array(15).fill(0)]));
  const expected = arena(e); expected.set(exitBytes({version: 4, reason: 9, retired: 0, detail: cases.at(-1).result}), 56);
  api(e, e.table ? 'complete_resident_windows_call' : 'complete_windows_call', e.table ? [e.low, e.high, u.low, u.high, 1] : [e.low, e.high, u.generation, 1], [[16, 96]]);
  assert.deepEqual(arena(e), expected); assert.deepEqual(cpu(e), before); save(`${e.label}-terminal.bin`, arena(e));
  observations.push({context: e.label, label: 'real-ExitProcess', expected: {version: 4, reason: 9, detail: cases.at(-1).result, cpu: before}, observed: {...stop(e), cpu: cpu(e)}});
}

const bodyOffsets = [27, 43, 32, 37, 51, 51], bodyInstructions = [2, 2, 2, 3, 1, 1];
for (const address of [0x400000, 0x500000]) for (const owner of ['replacement', 'resident']) {
  const e = fresh(owner, address), mainRows = spec.main_entries.map(offset => e.main + offset), gate = [[e.gate, 0x10003]];
  let caller, prefix, body;
  if (e.table) {caller = compile(e, 'live-caller', [...mainRows, e.gate], gate, 55); prefix = compile(e, 'dispatch-prefix', [e.kernel, e.kernel + 20], [], 8);}
  else caller = prefix = compile(e, 'first-caller-and-prefix', [mainRows[0], e.kernel, e.kernel + 20], [], 17);
  for (const [index, row] of cases.entries()) {
    const start = cpu(e), registers = [...start.registers];
    if (!index) {registers[0] = row.salt; registers[3] = 0xbbbb0001; registers[5] = 0xbbbb0002; registers[6] = 0xbbbb0003; registers[7] = e.state;}
    else registers[1] = row.before;
    registers[4] = e.top - 16;
    const steps = index ? [['store', e.output + (index - 1) * 8, cases[index - 1].result], ['read', e.state, row.before], ['store', e.output + (index - 1) * 8 + 4, row.before]] : [];
    steps.push(['store', e.top - 4, row.salt], ['store', e.top - 8, row.selector], ['store', e.top - 12, e.state], ['store', e.top - 16, e.main + spec.return_offsets[index]]);
    run(e, caller, `call-${index}`, index ? 8 : 9, {registers, pc: e.kernel, flags: index ? 0x16 : 2}, index ? 8 : 9, steps);
    for (const [offset, wanted] of [[-16, e.main + spec.return_offsets[index]], [-12, e.state], [-8, row.selector], [-4, row.salt]]) read(e, e.top + offset, wanted);
    if (index && !e.table) {
      const pcs = [mainRows[index + 1], e.kernel, e.kernel + 20, e.kernel + bodyOffsets[index], e.kernel + 53];
      if (index === 5) pcs.push(e.gate);
      caller = prefix = body = compile(e, `selected-path-${index}`, pcs, index === 5 ? gate : [], (index === 5 ? 6 : 8) + 8 + bodyInstructions[index] + 4);
    }
    const p = cpu(e), r = [...p.registers]; r[0] = row.salt; r[1] = row.before; r[2] = e.state; r[4] = e.top - 20; r[6] = row.selector;
    const flags = row.selector === 0 ? 0x93 : row.selector === 1 ? 0x93 : row.selector === 2 ? 0x97 : row.selector === 3 ? 0x46 : row.selector === 4 ? 2 : 0x86;
    run(e, prefix, `prefix-${index}`, 7, {registers: r, pc: e.kernel + (index < 4 ? 20 : 51), flags}, 7,
      [['store', e.top - 20, 0xbbbb0003], ['read', e.top - 4, row.salt], ['read', e.top - 8, row.selector], ['read', e.top - 12, e.state], ['read', e.state, row.before]]);
    if (!index) {
      api(e, 'protect', [e.tableAddress, 1, 0]); guard(e, prefix);
      for (const label of ['table-read-fault', 'unchanged-table-read-fault']) {
        run(e, prefix, label, 1, cpu(e), 0, [['read', e.tableAddress, 0, true]], 5, {detail: 2, address: e.tableAddress, access: 1, length: 4});
        read(e, e.state, row.before); read(e, e.output, 0); read(e, e.output + 4, 0);
      }
      run(e, prefix, 'stopped-zero-budget', 0, cpu(e), 0, [], 1, {version: e.table ? 3 : prefix.version});
      new DataView(e.memory.buffer).setUint32(e.base + 96, 1, true);
      run(e, prefix, 'stopped-cancel-before-budget', 0, cpu(e), 0, [], 2, {version: e.table ? 3 : prefix.version});
      new DataView(e.memory.buffer).setUint32(e.base + 96, 0, true);
      api(e, 'protect', [e.tableAddress, 1, 1]); guard(e, prefix);
      run(e, prefix, 'repair-then-real-cold-table-target', 2, {...cpu(e), pc: e.kernel + 27}, 1, [['read', e.tableAddress, e.kernel + 27]], 3, {version: e.table ? 3 : prefix.version});
      if (e.table) body = compile(e, 'selected-cases-and-return', [27, 32, 37, 43, 51, 53].map(offset => e.kernel + offset), [], 14);
      else caller = prefix = body = compile(e, 'selected-path-0', [mainRows[1], e.kernel, e.kernel + 20, e.kernel + 27, e.kernel + 53], [], 22);
    } else if (index < 4) run(e, prefix, `table-dispatch-${index}`, 1, {...cpu(e), pc: e.kernel + bodyOffsets[index]}, 1, [['read', e.tableAddress + index * 4, e.kernel + bodyOffsets[index]]]);
    const next = cpu(e); next.registers[0] = row.result; next.registers[1] = row.value; next.registers[4] = e.top - 12; next.registers[6] = 0xbbbb0003; next.pc = e.main + spec.return_offsets[index]; next.flags = logicalFlags(row.result);
    run(e, body, `return-${index}`, bodyInstructions[index] + 4, next, bodyInstructions[index] + 4,
      [['store', e.state, row.value], ['read', e.top - 20, 0xbbbb0003], ['read', e.top - 16, next.pc]]);
    assert.deepEqual(next.registers.slice(3), [0xbbbb0001, e.top - 12, 0xbbbb0002, 0xbbbb0003, e.state], 'cdecl callee-saved parents and argument stack');
    for (const [offset, wanted] of [[-20, 0xbbbb0003], [-12, e.state], [-8, row.selector], [-4, row.salt]]) read(e, e.top + offset, wanted);
    read(e, e.state, row.value);
  }
  const last = cases.at(-1), terminal = cpu(e); terminal.registers[1] = last.value; terminal.registers[4] = e.top - 8; terminal.pc = e.gate; terminal.flags = 0x16;
  run(e, caller, 'caller-consumes-final-result', 64, terminal, 6,
    [['store', e.output + 40, last.result], ['read', e.state, last.value], ['store', e.output + 44, last.value], ['store', e.top - 4, last.result], ['read', address + spec.iat_exit_rva, e.gate], ['store', e.top - 8, e.main + exitReturnOffset]], 8, {detail: 0x10003});
  for (const [i, row] of cases.entries()) {read(e, e.output + i * 8, row.result); read(e, e.output + i * 8 + 4, row.value);}
  read(e, e.state, last.value); read(e, address + spec.canary_rva, 0x13579bdf); read(e, address + 0x3ffc, 0);
  for (const [i, offset] of [27, 43, 32, 37].entries()) read(e, e.tableAddress + i * 4, e.kernel + offset);
  read(e, e.kernel + 23, e.tableAddress); finish(e, caller);
  assert.deepEqual([e.runs, e.retired, e.reads, e.stores], [27, 136, 49, 50]);
  api(e, 'close', []);
}
assert.deepEqual(sources(), pins);
const counts = {contexts: contexts.length, modules: modules.length, runs: contexts.reduce((n, e) => n + e.runs, 0), retired: contexts.reduce((n, e) => n + e.retired, 0),
  helper_calls: helperTrace.length, read32: contexts.reduce((n, e) => n + e.reads, 0), store32: contexts.reduce((n, e) => n + e.stores, 0), provider_completions: contexts.length, table_faults: helperTrace.filter(e => e.fault).length};
assert.deepEqual(counts, {contexts: 4, modules: 22, runs: 108, retired: 544, helper_calls: 396, read32: 196, store32: 200, provider_completions: 4, table_faults: 8});
const result = {status: 'ok', engine_sha256: engine.sha256, source_pins: pins, counts, contexts: contexts.map(({label, owner, address, captures, runs, retired, reads, stores, diagnostics}) => ({label, owner, address, captures, runs, retired, reads, stores, diagnostics})), modules, observations, helper_trace: helperTrace, diagnostics, artifacts,
  limits: ['fixed compiler function and readonly table; original DIR32-linked bytes and all cases/defaults at two bases and both bound owners; no generic linker/compiler claim',
    'replacement selected-path captures use original bytes; native full8block kernel admission proves all four bound profiles; resident split captures expose real cold table-selected transfer',
    'full4612 including dirtyFP compared per generated call; selected state/output/stack/table/canaries only, not whole RAM; forwarded canonical helpers separate from public diagnostics',
    'image-table permissions are explicit host-only fault controls; protect-only repair retains CPU and captured code identity; fault neutrality applies to the stopped JMP, not the whole C function',
    'seven finite replacement compilations and three resident children plus dispatcher; unchanged8blocks/64instructions/65536bytes/8resident slots; no product/API/ABI/performance claim']};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2), {flag: 'wx'}); console.log(JSON.stringify({status: 'ok', ...counts, output}));
