import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, readdirSync, writeFileSync} from 'node:fs';
import {join, resolve} from 'node:path';
import {readEngine} from '../support/engine.mjs';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(root && enginePath && output, 'expected current engine, unique bank directory and repository root');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const callerSha = process.env.RING3_ENGINE_SHA256;
assert.match(callerSha ?? '', /^[a-f0-9]{64}$/, 'actual caller must supply the captured current engine SHA-256');
assert.deepEqual(readdirSync(output).sort(), ['chain-signed.x86', 'chain-unsigned.x86'], 'unique Rust-authored bank directory before any artifact write');
const {bytes: engineBytes, module: engineModule, sha256: engineSha} = readEngine(enginePath, callerSha);
assert.deepEqual(WebAssembly.Module.imports(engineModule), []);
const SIZE = 4236, TRANSFER = 140, PC = 0x1000, COLD = PC + 128;
const OWNERS = ['replacement', 'resident'], KINDS = ['unsigned', 'signed'], FLAGS = [2, 0xcd7];
const PAIRS = [[0, 0xff], [1, 0x7f], [1, 0x80], [0x7f, 1], [0x7f, 2], [0x80, 1], [0x80, 0xff], [0xff, 0xff], [0xff, 2], [0x40, 4]];
const REG = [0xc9d45a00, 0x1234a567, 0x89abcdef, 0x76543210, 0x0a1b2c3d, 0x98badcfe, 0x13579bdf, 0x2468ace0];
const ALIASES = [['AL', 0, 1], ['CL', 1, 1], ['DL', 2, 1], ['BL', 3, 1], ['AH', 0, 256], ['CH', 1, 256], ['DH', 2, 256], ['BH', 3, 256]];
const words = values => {const bytes = Buffer.alloc(values.length * 4); values.forEach((value, i) => bytes.writeUInt32LE(value >>> 0, i * 4)); return bytes;};
function getByte(registers, alias) {const [, parent, factor] = ALIASES[alias]; return Math.floor(registers[parent] / factor) % 256;}
function putByte(registers, alias, value) {
  assert.ok(Number.isInteger(value) && value >= 0 && value < 256); const [, parent, factor] = ALIASES[alias];
  registers[parent] = registers[parent] - getByte(registers, alias) * factor + value * factor;
}
function multiply(kind, a, b, incoming) {
  assert.ok(KINDS.includes(kind) && [a, b].every(value => Number.isInteger(value) && value >= 0 && value < 256));
  assert.ok((incoming & 2) !== 0 && (incoming & ~0xcd7) === 0);
  const left = BigInt(kind === 'signed' && a >= 128 ? a - 256 : a), right = BigInt(kind === 'signed' && b >= 128 ? b - 256 : b);
  const product = left * right, overflow = kind === 'unsigned' ? product > 255n : product < -128n || product > 127n;
  return {value: Number(BigInt.asUintN(16, product)), flags: (incoming & 0x402) + Number(overflow) * 0x801,
    cf: Number(overflow), of: Number(overflow), product: product.toString()};
}
const ANCHORS = [
  ['unsigned', 0xff, 2, 2, 0x01fe, 0x803], ['unsigned', 0xfe, 1, 0x803, 0x00fe, 2],
  ['signed', 0xff, 2, 2, 0xfffe, 2], ['signed', 0xfe, 0xff, 2, 2, 2],
  ['unsigned', 0x80, 0x80, 0xcd7, 0x4000, 0xc03], ['signed', 0x80, 0x80, 0xcd7, 0x4000, 0xc03],
  ['unsigned', 0, 0xff, 0xcd7, 0, 0x402], ['signed', 1, 0x80, 2, 0xff80, 2],
  ['unsigned', 0x7f, 2, 2, 0xfe, 2], ['signed', 0x7f, 2, 2, 0xfe, 0x803],
];
for (const [kind, a, b, incoming, value, flags] of ANCHORS) {const result = multiply(kind, a, b, incoming); assert.deepEqual([result.value, result.flags], [value, flags]);}

const sourcePaths = [
  'engine/src/cpu/x86/ir.rs', 'engine/src/cpu/x86/decode/integer.rs', 'engine/src/cpu/dbt/region.rs', 'engine/src/cpu/dbt/wasm/integer.rs',
  'engine/src/cpu/x86/decode/operands.rs', 'engine/src/cpu/x86/decode/profile.rs', 'engine/src/cpu/x86/decode/lower.rs', 'engine/src/cpu/x86/decode/decoder.rs',
  'engine/src/cpu/x86/decode/flow.rs', 'engine/src/cpu/dbt/wasm/locals.rs', 'engine/src/cpu/dbt/wasm/emitter.rs', 'engine/src/cpu/dbt/wasm/memory.rs',
  'engine/src/cpu/dbt/wasm/control.rs', 'engine/src/cpu/dbt/wasm/dispatcher.rs', 'engine/src/cpu/dbt/wasm/abi.rs', 'engine/src/abi/x86/state.rs',
  'engine/src/abi/x86/exit.rs', 'engine/src/abi/x86/mod.rs', 'engine/src/abi/arena.rs', 'engine/src/abi/memory_helper.rs',
  'engine/tests/cpu_byte_unary.rs', 'engine/tests/cpu_accumulator_multiply.rs', 'engine/tests/cpu_multiply.rs', 'engine/tests/cpu_accumulator_multiply_wasm.rs',
  'engine/tests/fixtures/p2-accumulator-multiply/run.mjs', 'engine/tests/cpu_byte_unary_wasm.rs', 'engine/tests/fixtures/p2-byte-unary/run.mjs',
  'engine/tests/cpu_multiply_wasm.rs', 'engine/tests/fixtures/p2-multiply/run.mjs', 'engine/tests/fixtures/p2-multiply/program.S',
  'engine/Cargo.toml', 'Cargo.lock', 'Cargo.toml', 'rust-toolchain.toml', 'engine/tests/cpu_memory_byte_shift_cl.rs', 'engine/tests/cpu_memory_byte_shift_cl_wasm.rs',
  'engine/tests/fixtures/p2-byte-rotate-cl/run.mjs', 'engine/tests/fixtures/p2-memory-byte-shift-cl/run.mjs',
  'engine/tests/cpu_byte_accumulator_multiply.rs', 'engine/tests/cpu_byte_accumulator_multiply_wasm.rs', 'engine/tests/fixtures/p2-byte-accumulator-multiply/run.mjs',
];
assert.equal(sourcePaths.length, 41); assert.equal(new Set(sourcePaths).size, 41);
const pin = path => {const bytes = readFileSync(join(root, path)); return {path, bytes: bytes.length, sha256: hash(bytes)};};
const sourceIdentities = () => Object.fromEntries(sourcePaths.map(path => {const row = pin(path); return [path, {bytes: row.bytes, sha256: row.sha256}];}));
const sourcePins = sourceIdentities(), supportPath = 'engine/tests/fixtures/support/engine.mjs', supportPin = pin(supportPath);
const primary = [
  {kind: 'unsigned', path: 'target/r3-register-byte-accumulator-multiply-20261008/intel-mul-chapter-first.txt', bytes: 3966, sha256: '1f370aabaf6b496c7245b67d74e2ab2dc4b8e70de88ad6e35d26641802b8b85e'},
  {kind: 'signed', path: 'target/r3-register-byte-accumulator-multiply-20261008/intel-imul-chapter-first.txt', bytes: 8349, sha256: '6510155192454b9963076efeb431d5b9310bf55b3293ccb7dbe0318058b67a27'},
];
const artifacts = [], banks = [], contexts = [], modules = [], targets = [], chains = [], runs = [], hostInputs = [], hostCalls = [], rawFrames = [], rawRecords = [], controls = [];
const caseKeys = new Set(), semanticKeys = new Set(), sourceSpans = [], managedContexts = [];
const plan = {chains: 10 * 8 * 2 * 2 * 2, targets: 10 * 8 * 2 * 2 * 2 * 2, contexts: 2 * 2, modules: 2 * 2, banks: 2,
  seeds: 10 * 8 * 2 * 2 * 2, generated_calls: 10 * 8 * 2 * 2 * 2 * 3 + 4 * 6, retired: 10 * 8 * 2 * 2 * 2 * 5,
  host_calls: 4 * (1 + 1 + 1 + 1 + 2 + 1 + 1), host_inputs: 640 + 4 * (3 + 3), uploads: 4 * 2, upload_bytes: 4 * (130 + 1),
  request_inputs: 4 * 3, request_bytes: 4 * 130 + 2 * 32 + 2 * 64 + 4, cancel_inputs: 4 * 3, source_spans: 4 * 40,
  zero_budget_calls: 4, cancel_calls: 4, malformed_calls: 4, cold_calls: 4, direct_guard_calls: 8, stale_calls: 4, closed_calls: 4,
  raw_arenas: 4 + 2 * (640 + 12 + 12) + 2 * (32 - 4) + 2 * (640 * 3 + 24), files: 1 + 2 + 4 + 2 + 1 + 1};
plan.arena_checks = plan.raw_arenas + plan.modules;
assert.deepEqual([plan.chains, plan.targets, plan.generated_calls, plan.retired, plan.host_calls, plan.host_inputs, plan.raw_arenas, plan.files], [640, 1280, 1944, 3200, 32, 664, 5276, 11]);
const counts = {contexts: 0, modules: 0, seeds: 0, targets: 0, chains: 0, generated_calls: 0, retired: 0, host_calls: 0, uploads: 0, upload_bytes: 0,
  raw_arenas: 0, arena_checks: 0, setb: 0, seto: 0, jumps: 0, zero_budget_calls: 0, cancel_calls: 0, malformed_calls: 0,
  cold_calls: 0, direct_guard_calls: 0, stale_calls: 0, closed_calls: 0};
function artifact(path, bytes, write = true) {
  assert.match(path, /^[a-z0-9][a-z0-9.-]*$/); assert.ok(!artifacts.some(row => row.path === path));
  if (write) writeFileSync(join(output, path), bytes, {flag: 'wx'}); else assert.deepEqual(readFileSync(join(output, path)), bytes);
  const row = {path, bytes: bytes.length, sha256: hash(bytes)}; artifacts.push(row); return row;
}
artifact('engine.wasm', engineBytes);
for (const chapter of primary) {const bytes = readFileSync(join(root, chapter.path)); assert.equal(bytes.length, chapter.bytes); assert.equal(hash(bytes), chapter.sha256);
  chapter.artifact = artifact(`intel-${chapter.kind}.txt`, bytes); chapter.authority = 'locally pinned Intel093 whole MUL/IMUL chapters; no latest edition/network claim';}
function makeBank(kind) {
  const bytes = Buffer.alloc(130, 0xcc), specs = [], scans = [], instructions = []; let at = 0;
  const append = code => {const pc = PC + at; bytes.set(code, at); instructions.push({pc, length: code.length, next_pc: pc + code.length, hex: Buffer.from(code).toString('hex')}); at += code.length; return pc;};
  for (let alias = 0; alias < 8; alias++) {
    const entry = PC + at, modrm = 0xc0 + (kind === 'unsigned' ? 0x20 : 0x28) + alias;
    const first = append([0xf6, modrm]), second = append([0xf6, modrm]), tail = PC + at;
    append([0x0f, 0x92, 0xc2]); append([0x0f, 0x90, 0xc6]); append([0xeb, 128 - at - 2]);
    specs.push([entry, PC + at - entry]); scans.push({kind, alias, entry, first, second, tail, length: PC + at - entry});
  }
  assert.equal(at, 96); assert.equal(instructions.length, 40); assert.equal(specs.length, 8); bytes.set([0x0f, 0x0b], 128);
  const saved = artifact(`chain-${kind}.x86`, bytes, false), bank = {name: `chain-${kind}`, kind, bytes, specs, scans, instructions, packed: at, artifact: saved}; banks.push(bank); return bank;
}
const bankByKind = new Map(KINDS.map(kind => [kind, makeBank(kind)]));
function record(magic, size, fields) {const bytes = Buffer.alloc(size); bytes.write(magic); bytes.writeUInt16LE(1, 4); bytes.writeUInt16LE(1, 6); bytes.writeUInt32LE(size, 8);
  fields.forEach((value, i) => bytes.writeUInt32LE(value >>> 0, 16 + i * 4)); return bytes;}
const state = (registers, pc, flags) => record('R3ST', 56, [...registers, pc, flags]);
const exit = (reason, retired) => record('R3EX', 40, [reason, retired, 0, 0, 0, 0]);
function refresh(ctx) {ctx.bytes = new Uint8Array(ctx.memory.buffer); ctx.view = new DataView(ctx.memory.buffer); return ctx;}
function arena(ctx) {return Buffer.from(refresh(ctx).bytes.subarray(ctx.base, ctx.base + SIZE));}
function cpuBytes(bytes) {return {registers: Array.from({length: 8}, (_, i) => bytes.readUInt32LE(16 + i * 4)), pc: bytes.readUInt32LE(48), flags: bytes.readUInt32LE(52)};}
function summary(bytes) {return {state_hex: bytes.subarray(0, 56).toString('hex'), exit_hex: bytes.subarray(56, 96).toString('hex'), helper_hex: bytes.subarray(100, 140).toString('hex'), sha256: hash(bytes)};}
function check(ctx, label) {assert.deepEqual(arena(ctx), ctx.expected, `${ctx.owner}/${ctx.ordinal}/${label}: full live arena`); counts.arena_checks++;}
function frame(ctx, label, module = null) {
  check(ctx, label); const bytes = arena(ctx), row = {ordinal: rawRecords.length + 1, context: ctx.ordinal, module, label, path: 'arena-snapshots.bin',
    offset: rawFrames.length * SIZE, bytes: SIZE, sha256: hash(bytes), ...summary(bytes)};
  rawFrames.push(bytes); rawRecords.push(row); counts.raw_arenas++; return row;
}
function input(ctx, kind, label, offset, bytes) {
  const before = frame(ctx, `before input ${label}`); assert.ok(offset >= 0 && offset + bytes.length <= SIZE);
  refresh(ctx).bytes.set(bytes, ctx.base + offset); ctx.expected.set(bytes, offset);
  const after = frame(ctx, `after input ${label}`), row = {ordinal: hostInputs.length + 1, context: ctx.ordinal, kind, label,
    arena_offset: offset, bytes: bytes.length, hex: bytes.toString('hex'), sha256: hash(bytes), before_arena: before, after_arena: after};
  hostInputs.push(row); return row;
}
function request(ctx, bytes, label) {assert.ok(bytes.length <= 4096); return input(ctx, 'request', label, TRANSFER, bytes);}
function host(ctx, name, args, status = 0, extra = {}) {
  const before = frame(ctx, `before host ${name}`); const observed = ctx.api[name](...args); assert.equal(observed, status, name);
  const after = frame(ctx, `after host ${name}`); counts.host_calls++;
  const row = {ordinal: counts.host_calls, context: ctx.ordinal, name, args, status: observed, before_arena: before, after_arena: after, ...extra}; hostCalls.push(row); return row;
}
function upload(ctx, address, bytes, label) {
  const write = request(ctx, bytes, label); const row = host(ctx, 'upload', [address, bytes.length], 0, {request_input_ordinal: write.ordinal, address,
    uploaded: {bytes: bytes.length, hex: bytes.toString('hex'), sha256: hash(bytes)}});
  assert.ok(address >= PC && address + bytes.length <= PC + ctx.code.length); ctx.code.set(bytes, address - PC); counts.uploads++; counts.upload_bytes += bytes.length; return row;
}
function fresh(owner, selected) {
  const ordinal = ++counts.contexts, ctx = {ordinal, owner, selected, low: ordinal, high: 0x42594d55, expected: Buffer.alloc(SIZE), code: Buffer.from(selected.bytes)};
  ctx.expected.set(state(Array(8).fill(0), 0, 2)); ctx.expected.set(exit(1, 0), 56); ctx.expected.set(record('R3MH', 40, [0, 0, 0, 0, 0, 0]), 100);
  const instance = new WebAssembly.Instance(engineModule, {}); ctx.memory = instance.exports.memory; ctx.api = {};
  const arities = {open: 3, close: 0, arena_ptr: 0, map: 3, upload: 2, compile_entries: 2, compile_resident: 1,
    generation: 0, module_ptr: 0, module_len: 0, guard: 6, guard_resident: 7};
  for (const [name, arity] of Object.entries(arities)) {const fn = instance.exports[`ring3_abi_v1_${name}`]; assert.equal(typeof fn, 'function', name); assert.equal(fn.length, arity, name); ctx.api[name] = fn;}
  const status = ctx.api.open(1, ctx.low, ctx.high); assert.equal(status, 0); counts.host_calls++;
  hostCalls.push({ordinal: counts.host_calls, context: ordinal, name: 'open', args: [1, ctx.low, ctx.high], status, before_arena: null, after_arena: null,
    authority: 'no arena pointer exists before open; initialized allocation has a separately saved first physical frame'});
  ctx.base = ctx.api.arena_ptr() >>> 0; refresh(ctx); assert.ok(ctx.base > 0 && ctx.base + SIZE <= ctx.bytes.length);
  ctx.initialArena = frame(ctx, 'initial arena after open'); host(ctx, 'map', [PC, 1, 7]); upload(ctx, PC, selected.bytes, 'initial authored bank');
  contexts.push({context: ordinal, owner, bank: selected.name, key: [ctx.low, ctx.high], arena_base: ctx.base, page_limit: 1, code_permissions: 7,
    initial_arena: ctx.initialArena, source_currency: 'code remains writable before compile; no protect call precedes the sole same-byte invalidating upload'});
  managedContexts.push(ctx); return ctx;
}
function profile(bytes, owner) {
  let at = 8; assert.deepEqual([...bytes.subarray(0, 8)], [0, 97, 115, 109, 1, 0, 0, 0]);
  const uleb = () => {let value = 0, scale = 1; for (let i = 0; i < 5; i++) {assert.ok(at < bytes.length); const b = bytes[at++]; value += (b & 127) * scale;
    if (!(b & 128)) {assert.ok(value <= 0xffffffff); return value;} scale *= 128;} assert.fail('invalid bounded unsigned LEB');};
  const text = () => {const n = uleb(); assert.ok(at + n <= bytes.length); const value = bytes.subarray(at, at + n).toString('utf8'); at += n; return value;};
  const sections = new Map(); while (at < bytes.length) {const id = bytes[at++], n = uleb(); assert.ok(at + n <= bytes.length && !sections.has(id)); sections.set(id, [at, at + n]); at += n;}
  assert.deepEqual([...sections.keys()], [1, 2, 3, 7, 10]); const section = id => {at = sections.get(id)[0];}, ended = id => assert.equal(at, sections.get(id)[1]);
  section(1); const types = []; for (let i = 0, n = uleb(); i < n; i++) {assert.equal(bytes[at++], 0x60); const parameters = [], results = [];
    for (let j = 0, n = uleb(); j < n; j++) parameters.push(bytes[at++]); for (let j = 0, n = uleb(); j < n; j++) results.push(bytes[at++]); types.push({parameters, results});} ended(1);
  assert.deepEqual(types, [{parameters: Array(4).fill(0x7f), results: [0x7f]}, {parameters: Array(owner === 'replacement' ? 6 : 7).fill(0x7f), results: [0x7f]}]);
  section(2); assert.equal(uleb(), 2); assert.equal(text(), 'env'); assert.equal(text(), 'memory'); assert.equal(bytes[at++], 2); assert.equal(uleb(), 0); assert.equal(uleb(), 1);
  assert.equal(text(), 'ring3'); assert.equal(text(), owner === 'replacement' ? 'guard' : 'guard_resident'); assert.equal(bytes[at++], 0); assert.equal(uleb(), 1); ended(2);
  section(3); assert.equal(uleb(), 1); assert.equal(uleb(), 0); ended(3);
  section(7); assert.equal(uleb(), 1); assert.equal(text(), 'run'); assert.equal(bytes[at++], 0); assert.equal(uleb(), 1); ended(7);
  section(10); assert.equal(uleb(), 1); const bodyBytes = uleb(), end = at + bodyBytes; assert.equal(end, sections.get(10)[1]); const locals = [];
  for (let i = 0, n = uleb(); i < n; i++) locals.push([uleb(), bytes[at++]]); assert.deepEqual(locals, [[16, 0x7f], [1, 0x7e]]); assert.ok(at < end); assert.equal(bytes[end - 1], 0x0b);
  return {types, locals, body_bytes: bodyBytes, sections: [...sections.keys()], authority: 'structural profile only; no full remaining Wasm-body semantic certificate'};
}
function compile(ctx) {
  const specs = ctx.selected.specs, write = request(ctx, words(ctx.owner === 'replacement' ? specs.map(([entry]) => entry) : specs.flat()), 'initial compiler request');
  const before = frame(ctx, 'before compile'); const name = ctx.owner === 'replacement' ? 'compile_entries' : 'compile_resident', args = ctx.owner === 'replacement' ? [specs.length, 0] : [specs.length];
  const status = ctx.api[name](...args); assert.equal(status, 0); let binding;
  if (ctx.owner === 'replacement') {binding = {generation: ctx.api.generation(), pointer: ctx.api.module_ptr() >>> 0, length: ctx.api.module_len() >>> 0}; assert.ok(binding.generation > 0);}
  else {refresh(ctx); binding = Object.fromEntries(['low', 'high', 'pointer', 'length'].map((name, i) => [name, ctx.view.getUint32(ctx.base + TRANSFER + 8 + i * 4, true)]));
    assert.ok(binding.low || binding.high); ctx.expected.set(words([1, 24, binding.low, binding.high, binding.pointer, binding.length]), TRANSFER);
    assert.equal(ctx.api.generation(), 0); assert.equal(ctx.api.module_ptr(), 0); assert.equal(ctx.api.module_len(), 0);}
  const after = frame(ctx, 'after compile receipt'); counts.host_calls++;
  hostCalls.push({ordinal: counts.host_calls, context: ctx.ordinal, name, args, status, request_input_ordinal: write.ordinal, binding, before_arena: before, after_arena: after});
  refresh(ctx); assert.ok(binding.pointer > 0 && binding.length > 8 && binding.length <= 65536 && binding.pointer + binding.length <= ctx.bytes.length);
  const bytes = Buffer.from(ctx.bytes.subarray(binding.pointer, binding.pointer + binding.length)); assert.ok(WebAssembly.validate(bytes));
  const module = new WebAssembly.Module(bytes), guard = ctx.owner === 'replacement' ? 'guard' : 'guard_resident';
  const imports = [{module: 'env', name: 'memory', kind: 'memory'}, {module: 'ring3', name: guard, kind: 'function'}];
  assert.deepEqual(WebAssembly.Module.imports(module), imports); assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]); const abi = profile(bytes, ctx.owner);
  const child = new WebAssembly.Instance(module, {env: {memory: ctx.memory}, ring3: {[guard]: ctx.api[guard]}}); assert.equal(child.exports.run.length, 4);
  const saved = artifact(`${ctx.owner}-${ctx.ordinal}.wasm`, bytes), unit = {...binding, file: saved.path, bytes, run: child.exports.run}; counts.modules++;
  const spans = ctx.selected.instructions.map(row => {const data = ctx.code.subarray(row.pc - PC, row.pc - PC + row.length), span = {...row, file: ctx.selected.artifact.path, offset: row.pc - PC,
    hex: data.toString('hex'), sha256: hash(data)}; sourceSpans.push(span); return span;});
  modules.push({context: ctx.ordinal, owner: ctx.owner, bank: ctx.selected.name, file: saved.path, ...binding, specs, source_spans: spans, instructions: spans.length,
    artifact: saved, code_artifact: ctx.selected.artifact, imports, exports: [{name: 'run', kind: 'function'}], run_arity: 4, exit_version: 1, profile: abi});
  check(ctx, 'module profile/instantiation leaves entire arena unchanged'); return unit;
}
function initial(alias, a, b) {const registers = [...REG]; putByte(registers, 0, a); if (alias !== 0) putByte(registers, alias, b); return registers;}
function seed(ctx, registers, pc, flags) {
  assert.ok(FLAGS.includes(flags)); const bytes = Buffer.alloc(140); bytes.set(state(registers, pc, flags)); bytes.set(exit(3, 0), 56); bytes.fill(0x5a, 100, 140);
  const row = input(ctx, 'seed', 'one independent chain seed', 0, bytes); counts.seeds++; return row;
}
function cancel(ctx, value) {return input(ctx, 'cancel', `explicit cancellation ${value}`, 96, words([value]));}
function run(ctx, unit, budget, label, outcome = {}, expectedStatus = 0, malformed = false, retention) {
  const before = frame(ctx, `before generated ${label}`, unit.file), original = cpuBytes(rawFrames[rawFrames.length - 1]);
  const {registers = original.registers, pc = original.pc, flags = original.flags, reason = 1, retired = 0} = outcome;
  if (expectedStatus === 0) {ctx.expected.set(state(registers, pc, flags)); ctx.expected.set(exit(reason, retired), 56);}
  const args = [malformed ? 0xffffffff : ctx.base, ctx.base + 56, budget, ctx.base + 96], observed = unit.run(...args);
  if (retention) retention(); assert.equal(observed, expectedStatus, label);
  const after = frame(ctx, `after generated ${label}`, unit.file); counts.generated_calls++; counts.retired += observed === 0 ? retired : 0;
  const row = {ordinal: counts.generated_calls, context: ctx.ordinal, owner: ctx.owner, module: unit.file, label, budget, args, status: observed, expected_status: expectedStatus,
    reason: observed ? null : reason, retired: observed ? 0 : retired, before_arena: before, after_arena: after}; runs.push(row); return row;
}
function target(ctx, unit, scan, pc, key, pairOrdinal, flagsLabel, ordinalInChain) {
  const original = cpuBytes(arena(ctx)); assert.equal(original.pc, pc); const oldAl = getByte(original.registers, 0), oldSource = getByte(original.registers, scan.alias);
  const result = multiply(scan.kind, oldAl, oldSource, original.flags), registers = [...original.registers];
  registers[0] = Math.floor(registers[0] / 65536) * 65536 + result.value;
  const observed = run(ctx, unit, 1, `multiply ${ordinalInChain} before consumers`, {registers, pc: pc + 2, flags: result.flags, retired: 1}); counts.targets++;
  assert.ok(!caseKeys.has(key)); caseKeys.add(key); semanticKeys.add([scan.kind, scan.alias, oldAl, oldSource, original.flags].join('/'));
  const row = {context: ctx.ordinal, owner: ctx.owner, module: unit.file, bank: ctx.selected.name, case_key: key, kind: scan.kind, alias: scan.alias,
    pair_ordinal: pairOrdinal, flags_label: flagsLabel, ordinal_in_chain: ordinalInChain, encoded_target_hex: ctx.code.subarray(pc - PC, pc - PC + 2).toString('hex'),
    target_pc: pc, next_pc: pc + 2, old_al: oldAl, old_source: oldSource, input_registers: original.registers, input_flags: original.flags,
    expected: result, registers_after: registers, run_ordinal: observed.ordinal, before_arena: observed.before_arena, after_arena: observed.after_arena,
    observation_phase: 'full product State/Exit/helper/arena before any SETcc; metadata references the unique generated-call frame pair'};
  targets.push(row); return row;
}
function suffix(ctx, unit) {
  const current = cpuBytes(arena(ctx)), registers = [...current.registers]; putByte(registers, 2, current.flags % 2); putByte(registers, 6, Math.floor(current.flags / 0x800) % 2);
  const row = run(ctx, unit, 4, 'real SETB DL/SETO DH/JMP with spare budget', {registers, pc: COLD, reason: 3, retired: 3}); counts.setb++; counts.seto++; counts.jumps++; return row;
}
function physical(ctx, unit) {const bytes = Buffer.from(ctx.bytes.subarray(unit.pointer, unit.pointer + unit.length)); assert.deepEqual(bytes, unit.bytes, 'whole live original module allocation');
  return {pointer: unit.pointer, bytes: unit.length, sha256: hash(bytes)};}
function control(ctx, unit) {
  const saved = {context: ctx.ordinal, owner: ctx.owner, module: unit.file};
  saved.zero = run(ctx, unit, 0, 'current zero budget'); counts.zero_budget_calls++;
  cancel(ctx, 1); saved.cancel = run(ctx, unit, 0, 'cancel precedes zero budget', {reason: 2}); counts.cancel_calls++;
  saved.malformed = run(ctx, unit, 0, 'invalid State pointer precedes cancel and budget', {}, 1, true); counts.malformed_calls++;
  cancel(ctx, 0); saved.cold = run(ctx, unit, 1, 'current cold EIP NeedCode', {reason: 3}); counts.cold_calls++;
  saved.direct_guards = [];
  for (const role of ['key', 'identity']) {
    const low = ctx.low ^ Number(role === 'key'), name = ctx.owner === 'replacement' ? 'guard' : 'guard_resident';
    const args = ctx.owner === 'replacement' ? [low, ctx.high, unit.generation + Number(role === 'identity'), ctx.base, ctx.base + 56, ctx.base + 96]
      : [low, ctx.high, unit.low ^ Number(role === 'identity'), unit.high, ctx.base, ctx.base + 56, ctx.base + 96];
    saved.direct_guards.push(host(ctx, name, args, 3, {role})); counts.direct_guard_calls++;
  }
  const beforeOld = physical(ctx, unit); const same = upload(ctx, PC, Buffer.from([ctx.code[0]]), 'sole same consumed opcode upload without earlier protect'); cancel(ctx, 1);
  let retained;
  saved.stale = run(ctx, unit, 0, 'stale before invalid pointers/cancel/budget', {}, 4, true, () => {
    retained = physical(ctx, unit); assert.deepEqual(retained, beforeOld, 'whole live allocation immediately after stale return before any snapshot/getter/helper/fresh allocation');
  }); counts.stale_calls++;
  saved.same_upload_host_ordinal = same.ordinal; saved.allocation_before_stale = beforeOld; saved.allocation_after_stale_receipt = retained;
  host(ctx, 'close', []); ctx.closed = true; const knownBefore = Buffer.from(ctx.bytes.subarray(ctx.base, ctx.base + SIZE)); let retainedArena;
  saved.closed = run(ctx, unit, 0, 'closed precedes stale/invalid pointers/cancel/budget', {}, 5, true, () => {
    const after = Buffer.from(ctx.bytes.subarray(ctx.base, ctx.base + SIZE)); assert.deepEqual(after, knownBefore, 'known retained arena only after close; never read closed old module allocation');
    retainedArena = {pointer: ctx.base, bytes: SIZE, sha256: hash(after)};
  }); counts.closed_calls++; saved.closed_known_arena = retainedArena; controls.push(saved);
}
for (const owner of OWNERS) for (const kind of KINDS) {
  const selected = bankByKind.get(kind), ctx = fresh(owner, selected), unit = compile(ctx);
  for (let alias = 0; alias < 8; alias++) for (let pairOrdinal = 0; pairOrdinal < PAIRS.length; pairOrdinal++) for (const flags of FLAGS) {
    const [a, b] = PAIRS[pairOrdinal], scan = selected.scans[alias]; const seeded = seed(ctx, initial(alias, a, b), scan.first, flags);
    const key = `${owner}/${kind}/a${alias}/p${pairOrdinal}/f${flags}`, first = target(ctx, unit, scan, scan.first, `${key}/first`, pairOrdinal, flags, 1);
    const second = target(ctx, unit, scan, scan.second, `${key}/second`, pairOrdinal, flags, 2);
    assert.equal(first.after_arena.sha256, second.before_arena.sha256, 'complete retained arena between the identical two targets with no intervening write/reseed/consumer');
    assert.equal(second.old_al, first.expected.value % 256); assert.equal(second.old_source, getByte(first.registers_after, alias));
    if (alias === 4 && a === 0xff && b === 2) assert.deepEqual([first.expected.value, second.expected.value], kind === 'unsigned' ? [0x01fe, 0x00fe] : [0xfffe, 2]);
    if (alias === 0 && a === 0x80) assert.deepEqual([first.expected.value, second.expected.value], [0x4000, 0]);
    const tail = suffix(ctx, unit); counts.chains++; chains.push({context: ctx.ordinal, owner, kind, alias, pair_ordinal: pairOrdinal, labels: [a, b], input_flags: flags,
      seed_input_ordinal: seeded.ordinal, first_target: targets.length - 2, second_target: targets.length - 1, first_case: first.case_key, second_case: second.case_key,
      first_run: first.run_ordinal, second_run: second.run_ordinal, suffix_run: tail.ordinal,
      continuity: 'no host input/call/upload/compiler/consumer occurs between the two target calls; second physically current AL/source/EIP; no CL count'});
  }
  control(ctx, unit);
}
for (const name of ['contexts', 'modules', 'seeds', 'targets', 'chains', 'generated_calls', 'retired', 'host_calls', 'uploads', 'upload_bytes', 'raw_arenas', 'arena_checks',
  'zero_budget_calls', 'cancel_calls', 'malformed_calls', 'cold_calls', 'direct_guard_calls', 'stale_calls', 'closed_calls']) assert.equal(counts[name], plan[name], `source-derived ${name}`);
assert.equal(caseKeys.size, 1280); assert.equal(hostInputs.length, plan.host_inputs); assert.equal(sourceSpans.length, plan.source_spans);
assert.deepEqual([counts.setb, counts.seto, counts.jumps], [640, 640, 640]);
assert.deepEqual(['request', 'cancel', 'seed'].map(kind => hostInputs.filter(row => row.kind === kind).length), [12, 12, 640]);
assert.equal(hostInputs.filter(row => row.kind === 'request').reduce((sum, row) => sum + row.bytes, 0), plan.request_bytes);
const ownerCounts = Object.fromEntries(OWNERS.map(owner => [owner, targets.filter(row => row.owner === owner).length])); assert.deepEqual(ownerCounts, {replacement: 640, resident: 640});
const savedArenas = artifact('arena-snapshots.bin', Buffer.concat(rawFrames)); assert.equal(savedArenas.bytes, plan.raw_arenas * SIZE);
const sourcePinsAfter = sourceIdentities(); assert.deepEqual(sourcePinsAfter, sourcePins); assert.deepEqual(pin(supportPath), supportPin); assert.equal(hash(readFileSync(enginePath)), engineSha);
const modeledPages = managedContexts.map(ctx => {assert.equal(ctx.closed, true); const bytes = Buffer.alloc(4096); bytes.set(ctx.code);
  return {context: ctx.ordinal, owner: ctx.owner, address: PC, bytes: 4096, permissions_before_close: 7, final_state: 'retired-by-close', current_mapped_pages: [],
    model: {hex: bytes.toString('hex'), sha256: hash(bytes)}, upload_host_ordinals: hostCalls.filter(row => row.context === ctx.ordinal && row.name === 'upload').map(row => row.ordinal),
    authority: 'successful zero-map plus exact successful upload bytes; modeled code page, not physical guest-RAM readback'};});
const result = {status: 'ok', profile: 'finite prefix-free flat32 F6 register BYTE accumulator MUL/one-operand IMUL; two actual-engine-Wasm bound origins',
  command: [process.execPath, process.argv[1], enginePath, output, root], environment: {node: process.version, v8: process.versions.v8, platform: process.platform, arch: process.arch},
  engine: {path: resolve(enginePath), bytes: engineBytes.length, sha256: engineSha, caller_sha256: callerSha}, source_pins: sourcePins, source_pins_after: sourcePinsAfter,
  selected_input_census: {repo_paths: 41, engine: 1, total: 42}, source_support_dependency: {...supportPin, selected_roster: false, authority: 'unchanged readEngine helper separately pinned; full engine source freeze includes it'},
  primary, plan, counts, corpus: {pairs: PAIRS, flags: FLAGS, registers: REG, aliases: ALIASES, owner_target_counts: ownerCounts, qualified_case_keys: caseKeys.size,
    semantic_input_keys: semanticKeys.size, semantic_key: 'kind/alias/physical oldAL/live source byte/physical inputFLAGS; repetitions allowed; no asserted unique-scalar census',
    coupling: 'AL source equals currentAL regardless of pair b label; AH second source equals freshly produced AH', literal_anchors: ANCHORS},
  banks: banks.map(({name, kind, bytes, specs, scans, instructions, packed, artifact}) => ({name, kind, bytes: bytes.length, packed, specs, scans, instructions, artifact})),
  contexts, modules, targets, chains, runs, controls, host_inputs: hostInputs, host_calls: hostCalls, modeled_pages: modeledPages, raw_arena_records: rawRecords, raw_arena_artifact: savedArenas, artifacts,
  evidence_limits: {oracle: 'producer BigInt product/reference; independent raw checker must use separate repeated-addition/one-bit accumulation and physical source/full-before-state',
    flags: 'only2/cd7 initial validFLAGS; undefined PF/AF/ZF/SF clear is accepted Ring3 policy, not hardware guarantee',
    raw_frames: 'all1944 generated before/after pairs, all664 explicit arena-input before/after pairs,28 known-arena host before/after pairs and4 initialized arenas; references are shared, not duplicate target frames',
    live_allocation: 'same-upload stale callback compares original live module synchronously before post-frame/getter/helper/fresh allocation; saved original bytes plus before/after SHA receipts do not independently save after-allocation bytes or certify timing',
    closed: 'raw5 observed against known retained arena; no old module allocation is read after close',
    model: 'full raw arena/event correspondence and finite operation/consumer assertions; structural module parser is not full remaining Wasm-body/generic callback certificate; modeled code pages are not full-RAM readback',
    scope: 'no memory multiply/datafault/store/SMC-by-generated-store/word/x64/division/exception/oldactual/fullISA/allpairs/allFLAGS/performance/fullCI/SDK/graphics/browser/game claim'},
  pre_execution_findings: [{label: 'SUPPORT-ROSTER', note: 'readEngine immutable helper remains outside literal41 selected inputs, separately physically pinned and covered by full engine freeze; no helper write or selected scope increase'},
    {label: 'SUFFIX-BUDGET', note: 'message-only budget3 proposal refined before code to budget4/spare cold NeedCode3 with3 retired; no prior physical source/runtime failure'},
    {label: 'PURE-LOCALS', note: 'accepted pre-API contract correction16i32+1i64 for register-only operation in both bound profiles;22 requires helpers/gates, no new locals'},
    {label: 'AUTHOR-COLD-CONTROL', first_driver_sha256: 'cbac0444e7b3c217602e97d48a164c461f33f5b20887ad873df0a81258c86775',
      preserved_path: 'target/r3-register-byte-accumulator-multiply-20261008/actual-first/engine/tests/fixtures/p2-byte-accumulator-multiply/run.mjs',
      note: 'whole first author source review found the selected cold control inherited run default reason1; explicit reason3 fixed before any executable use, core/control/frame census unchanged'}]};
assert.equal(artifacts.length + 1, plan.files); writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2), {flag: 'wx'});
assert.deepEqual(readdirSync(output).sort(), [...artifacts.map(row => row.path), 'result.json'].sort(), 'exact physical output census');
const resultBytes = readFileSync(join(output, 'result.json')); console.log(JSON.stringify({status: 'ok', engine_sha256: engineSha, result_sha256: hash(resultBytes),
  targets: counts.targets, chains: counts.chains, contexts: counts.contexts, modules: counts.modules, generated_calls: counts.generated_calls,
  retired: counts.retired, raw_arenas: counts.raw_arenas, files: plan.files, output}));
