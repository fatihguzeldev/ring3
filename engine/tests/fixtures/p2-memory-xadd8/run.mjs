import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, readdirSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../support/engine.mjs';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(root, 'expected actual engine Wasm, output directory and repository root');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const {bytes: engineBytes, module: engineModule, sha256: engineSha256} = readEngine(enginePath);
assert.deepEqual(WebAssembly.Module.imports(engineModule), []);
const SIZE = 4236, TRANSFER = 140, PC = 0x1000, COLD = PC + 128, KEEP = 0x3000, DATA = 0x4000, TOP = 0xfffff000, NEXT = 0x5000, BYTE = 0x4010;
const OWNERS = ['replacement', 'resident'], FLAGS = [2, 0xcd7];
const REG = [0x1234807f, 0x23457f80, 0x34560f10, 0x4567ff00, 0x56789abc, 0x6789abcd, 0x789abcde, 0x89abcdef];
const BYTE_REGISTERS = ['AL', 'CL', 'DL', 'BL', 'AH', 'CH', 'DH', 'BH'];
const PARENTS = [0, 1, 2, 3, 0, 1, 2, 3], HEX_LANES = [3, 3, 3, 3, 2, 2, 2, 2];
const ANCHORS = [
  [0, 0, 0, 0x46], [0, 1, 1, 2], [1, 2, 3, 6], [0xff, 1, 0, 0x57],
  [0x7f, 1, 0x80, 0x892], [0x80, 0x80, 0, 0x847], [0x0f, 1, 0x10, 0x12], [0x78, 0xef, 0x67, 0x13],
];
function add8(left, right, flags) {
  const wide = left + right, value = wide % 256;
  const signed = operand => operand < 128 ? operand : operand - 256;
  const total = signed(left) + signed(right), even = value.toString(2).replaceAll('0', '').length % 2 === 0;
  return {value, flags: 2 + (flags & 0x400) + Number(wide >= 256) + Number(even) * 4
    + Number(left % 16 + right % 16 >= 16) * 0x10 + Number(value === 0) * 0x40
    + Number(value >= 128) * 0x80 + Number(total < -128 || total > 127) * 0x800};
}
function hexBytes(parent) {return parent.toString(16).padStart(8, '0').match(/../g);}
function byteFrom(parent, source) {return Number.parseInt(hexBytes(parent)[HEX_LANES[source]], 16);}
function insertByte(parent, source, value) {
  assert.ok(Number.isInteger(value) && value >= 0 && value < 256);
  const bytes = hexBytes(parent); bytes[HEX_LANES[source]] = value.toString(16).padStart(2, '0'); return Number.parseInt(bytes.join(''), 16);
}
for (const [left, right, value, flags] of ANCHORS) assert.deepEqual(add8(left, right, 2), {value, flags}, 'literal ADD8 anchor');
const words = values => {const bytes = Buffer.alloc(values.length * 4); values.forEach((value, index) => bytes.writeUInt32LE(value >>> 0, index * 4)); return bytes;};
function bank(name, forms, prefixes, canary, packed, instructions) {
  const bytes = Buffer.alloc(130, 0xcc), scans = [], specs = []; let offset = 0;
  for (const [index, form] of forms.entries()) {
    const tail = [0x0f, 0x92, 0xc0, 0x0f, 0x90, 0xc2, ...(index === canary ? [0xbb, 0xef, 0xbe, 0xad, 0xde] : [])];
    const length = form.length + tail.length + 2, entry = PC + offset;
    bytes.set([...form, ...tail, 0xeb, 128 - offset - length], offset);
    scans.push({entry, target: entry + (prefixes[index] ?? 0), next: entry + form.length, source: name === 'normal' ? index : [4, 5, 7, 2, 3, 1, 5][index], canary: index === canary});
    specs.push([entry, length]); offset += length;
  }
  assert.equal(offset, packed); assert.ok(specs.length <= 8 && instructions <= 64); bytes.set([0x0f, 0x0b], 128);
  assert.deepEqual(readFileSync(join(output, `${name}.x86`)), bytes, 'independent Rust/JS program bytes');
  return {name, bytes, specs, scans, instructions};
}
const banks = {
  normal: bank('normal', Array.from({length: 8}, (_, source) => [0x0f, 0xc0, source << 3 | 5, 0x10, 0x40, 0, 0]), [], -1, 120, 32),
  special: bank('special', [[0x0f, 0xc0, 0x20], [0x0f, 0xc0, 0x6c, 0x8f, 0xf0], [0x0f, 0xc0, 0x3c, 0x9b],
    [0x0f, 0xc0, 0x14, 0x24], [0x0f, 0xc0, 0x5d, 0], [0x83, 0xc3, 1, 0x0f, 0xc0, 0x0f],
    [0xbe, 0x7f, 0x56, 0x34, 0x12, 0x0f, 0xc0, 0x2f]], {5: 3, 6: 5}, 6, 95, 31),
};
assert.deepEqual(banks.special.scans.map(scan => scan.entry - PC), [0, 11, 24, 36, 48, 60, 74]);
const faultShapes = [
  {name: 'byte-read-unmapped', address: BYTE, detail: 1, fault: BYTE, access: 1, missing: DATA},
  {name: 'byte-read-denied', address: 0x4fff, detail: 2, fault: 0x4fff, access: 1, denied: DATA, permissions: 2},
  {name: 'byte-write-read-only', address: 0x4fff, detail: 2, fault: 0x4fff, access: 2, denied: DATA, permissions: 1},
];
const formulas = {normal_targets: OWNERS.length * 8 * ANCHORS.length * FLAGS.length,
  alias_targets: OWNERS.length * 5 * 2 * FLAGS.length, endpoint_targets: OWNERS.length * 2 * 2 * FLAGS.length,
  repaired_targets: OWNERS.length * faultShapes.length * FLAGS.length, code_targets: OWNERS.length * 2};
const targetCount = Object.values(formulas).reduce((total, value) => total + value, 0);
assert.equal(targetCount, 328);
const counts = {contexts: 0, modules: 0, seeds: 0, xadds: 0, normal_targets: 0, alias_targets: 0, endpoint_targets: 0,
  repaired_targets: 0, code_targets: 0, setb: 0, seto: 0, jumps: 0, canaries: 0, fault_calls: 0, retired_add_prefixes: 0,
  retired_mov_prefixes: 0, permission_only_repairs: 0, changed_data_repairs: 0, restored_map_repairs: 0,
  same_value_code_stores: 0, changing_code_stores: 0, stale_calls: 0, closed_calls: 0, preflight_calls: 0,
  cold_calls: 0, malformed_calls: 0, generated_calls: 0, arena_checks: 0, pages: 0, diagnostic_read32_calls: 0, absent_neighbor_read8_calls: 0};
const contexts = [], modules = [], targetRows = [], faults = [], codeStores = [], pagesRead = [], runs = [], hostInputs = [], artifacts = [];
function artifact(path, bytes, write = true) {
  assert.ok(!artifacts.some(row => row.path === path)); if (write) writeFileSync(join(output, path), bytes, {flag: 'wx'});
  const row = {path, length: bytes.length, sha256: hash(bytes)}; artifacts.push(row); return row;
}
artifact('engine.wasm', engineBytes); for (const entry of Object.values(banks)) artifact(`${entry.name}.x86`, entry.bytes, false);
const source = {provenance: 'existing physically pinned local Intel primary; no fresh download or newest-edition claim',
  prerequisite_policy: 'ignored local immutable PDF/full-text inputs are required offline before guest execution; no bootstrap or clean-checkout/fullCI claim',
  order: '334569-093US', edition: 'September 2026', pdf_path: 'target/p2-memory-binary-spec/334569-093-sdm-vol-2d.pdf',
  pdf_bytes: 1814965, pdf_sha256: '17b632da847e4757448e8afc950487b5d489da6c238de4551dc8cf6f799d3dcd',
  full_text_path: 'target/p2-memory-binary-spec/334569-093-sdm-vol-2d.txt', full_text_bytes: 721198,
  full_text_sha256: '1bdcfb8396e09e84cb008ed0bb07e08d38ca9d30e39b44db695f3ef43508013d', chapter_lines: [979, 1048],
  chapter_bytes: 3756, chapter_sha256: '407080e07d2686e251c4cd2e9cee8f000278b7ed1c1b3f8303d7d63c596e6f34',
  pdf_pages_zero_based: [29, 30], printed_pages: ['6-27', '6-28'], chapter_separator_policy: 'includes internal PDF PAGE 31 separator'};
for (const kind of ['pdf', 'full_text']) {const bytes = readFileSync(join(root, source[`${kind}_path`])); assert.equal(bytes.length, source[`${kind}_bytes`]); assert.equal(hash(bytes), source[`${kind}_sha256`]);}
const chapter = Buffer.from(readFileSync(join(root, source.full_text_path), 'utf8').split('\n').slice(978, 1048).join('\n') + '\n');
assert.equal(chapter.length, source.chapter_bytes); assert.equal(hash(chapter), source.chapter_sha256); source.saved_chapter = artifact('intel-xadd.txt', chapter);
const sourcePaths = ['engine/tests/cpu_memory_xadd8.rs', 'engine/tests/cpu_memory_xadd8_wasm.rs', 'engine/tests/fixtures/p2-memory-xadd8/run.mjs', 'engine/tests/fixtures/support/engine.mjs'];
const sourceHashes = () => Object.fromEntries(sourcePaths.map(path => [path, hash(readFileSync(join(root, path)))]));
const testHashes = sourceHashes();
function record(magic, version, size, fields) {
  const bytes = Buffer.alloc(size); bytes.write(magic); bytes.writeUInt16LE(version, 4); bytes.writeUInt16LE(1, 6); bytes.writeUInt32LE(size, 8);
  fields.forEach((value, index) => bytes.writeUInt32LE(value >>> 0, 16 + index * 4)); return bytes;
}
const state = (registers, pc, flags) => record('R3ST', 1, 56, [...registers, pc, flags]);
const exit = (version, reason, retired, detail = 0, address = 0, access = 0) => record('R3EX', version, 40, [reason, retired, detail, address, access, access ? 1 : 0]);
const helper = (value = 0) => record('R3MH', 1, 40, [0, value, 0, 0, 0, 0]);
const readHelper = (value = 0, detail = 0, address = 0) => record('R3MH', 2, 40, [detail ? 1 : 0, detail ? 0 : value, detail, detail ? address : 0, detail ? 1 : 0, 1]);
const storeHelper = (detail = 0, address = 0) => record('R3MH', 3, 40, [detail ? 1 : 0, 0, detail, detail ? address : 0, detail ? 2 : 0, 1]);
const pattern = Buffer.from(Array.from({length: 4096}, (_, index) => index % 251 + 1));
function refresh(ctx) {ctx.bytes = new Uint8Array(ctx.memory.buffer); ctx.view = new DataView(ctx.memory.buffer); return ctx;}
function arena(ctx) {return Buffer.from(refresh(ctx).bytes.subarray(ctx.base, ctx.base + SIZE));}
function check(ctx, label) {assert.deepEqual(arena(ctx), ctx.expected, `${ctx.owner}/${ctx.ordinal}/${label}: complete arena`); counts.arena_checks++;}
function snapshot(ctx) {const bytes = arena(ctx); return {state_hex: bytes.subarray(0, 56).toString('hex'), exit_hex: bytes.subarray(56, 96).toString('hex'), helper_hex: bytes.subarray(100, 140).toString('hex')};}
function cpu(ctx) {return {registers: Array.from({length: 8}, (_, index) => ctx.expected.readUInt32LE(16 + index * 4)), pc: ctx.expected.readUInt32LE(48), flags: ctx.expected.readUInt32LE(52)};}
function saveArena(ctx, label) {check(ctx, label); return artifact(`arena-${ctx.ordinal}-${label}.bin`, arena(ctx));}
function pure(ctx, action, label, status = 0) {check(ctx, `before ${label}`); assert.equal(action(), status, label); check(ctx, label);}
function request(ctx, bytes) {assert.ok(bytes.length <= 4096); refresh(ctx).bytes.set(bytes, ctx.base + TRANSFER); ctx.expected.set(bytes, TRANSFER); check(ctx, 'explicit host request');}
function patch(ctx, address, bytes) {for (const [index, value] of bytes.entries()) {const at = address + index, base = Math.floor(at / 4096) * 4096; assert.ok(ctx.pages.has(base)); ctx.pages.get(base)[at % 4096] = value;}}
function upload(ctx, address, bytes, purpose) {request(ctx, bytes); pure(ctx, () => ctx.api.upload(address, bytes.length), purpose); patch(ctx, address, bytes); hostInputs.push({context: ctx.ordinal, purpose, address, length: bytes.length, sha256: hash(bytes)});}
function map(ctx, address, bytes = pattern, permissions = 3) {pure(ctx, () => ctx.api.map(address, 1, permissions), 'map page'); ctx.pages.set(address, Buffer.alloc(4096)); ctx.permissions.set(address, permissions); upload(ctx, address, bytes, 'page input');}
function protect(ctx, address, permissions) {pure(ctx, () => ctx.api.protect(address, 1, permissions), 'permission change'); ctx.permissions.set(address, permissions);}
function fresh(owner, selected, label) {
  const instance = new WebAssembly.Instance(engineModule, {}), api = {}, ordinal = ++counts.contexts;
  const arities = {open: 3, close: 0, arena_ptr: 0, map: 3, unmap: 2, protect: 3, upload: 2, read8: 1, read32: 1,
    compile_entries: 2, compile_resident: 1, generation: 0, module_ptr: 0, module_len: 0, guard: 6, guard_resident: 7, store8: 2, store_resident8: 6};
  for (const [name, arity] of Object.entries(arities)) {api[name] = instance.exports[`ring3_abi_v1_${name}`]; assert.equal(typeof api[name], 'function', name); assert.equal(api[name].length, arity, name);}
  const ctx = {owner, ordinal, bank: selected, label, api, memory: instance.exports.memory, low: ordinal, high: 0x58414438,
    expected: Buffer.alloc(SIZE), pages: new Map(), permissions: new Map(), units: []};
  assert.equal(api.open(4, ctx.low, ctx.high), 0); ctx.base = api.arena_ptr() >>> 0;
  ctx.expected.set(state(Array(8).fill(0), 0, 2)); ctx.expected.set(exit(1, 1, 0), 56); ctx.expected.set(helper(), 100); check(ctx, 'independently initialized arena');
  const codePage = Buffer.alloc(4096); codePage.set(selected.bytes); map(ctx, PC, codePage, 7);
  for (const address of [KEEP, DATA, TOP]) map(ctx, address);
  contexts.push({owner, context: ordinal, label, bank: selected.name, key: [ctx.low, ctx.high], base: ctx.base, page_limit: 4}); return ctx;
}
function physical(ctx, unit) {refresh(ctx); const bytes = Buffer.from(ctx.bytes.subarray(unit.pointer, unit.pointer + unit.length)); assert.deepEqual(bytes, unit.bytes, 'whole live module allocation retained'); return {pointer: unit.pointer, length: bytes.length, sha256: hash(bytes)};}
const identity = (ctx, unit) => ctx.owner === 'replacement' ? {generation: unit.generation} : {low: unit.low, high: unit.high};
function compile(ctx, phase = 'initial', tail) {
  const specs = tail ? [[tail.next, 13]] : ctx.bank.specs, memory = !tail;
  request(ctx, words(ctx.owner === 'replacement' ? specs.map(([entry]) => entry) : specs.flat())); let binding;
  if (ctx.owner === 'replacement') {pure(ctx, () => ctx.api.compile_entries(specs.length, 0), 'entry compiler'); binding = {generation: ctx.api.generation(), pointer: ctx.api.module_ptr() >>> 0, length: ctx.api.module_len() >>> 0}; assert.ok(binding.generation > 0);}
  else {
    check(ctx, 'before resident compiler'); assert.equal(ctx.api.compile_resident(specs.length), 0); refresh(ctx);
    binding = Object.fromEntries(['low', 'high', 'pointer', 'length'].map((name, index) => [name, ctx.view.getUint32(ctx.base + TRANSFER + 8 + index * 4, true)]));
    assert.ok(binding.low !== 0 || binding.high !== 0); ctx.expected.set(words([1, 24, binding.low, binding.high, binding.pointer, binding.length]), TRANSFER); check(ctx, 'resident metadata only');
  }
  refresh(ctx); assert.ok(binding.pointer > 0 && binding.length > 8 && binding.length <= 65536 && binding.pointer + binding.length <= ctx.bytes.length);
  const bytes = Buffer.from(ctx.bytes.subarray(binding.pointer, binding.pointer + binding.length)), module = new WebAssembly.Module(bytes);
  const names = [ctx.owner === 'replacement' ? 'guard' : 'guard_resident', ...(memory ? ['read8', ctx.owner === 'replacement' ? 'store8' : 'store_resident8'] : [])];
  const imports = [{module: 'env', name: 'memory', kind: 'memory'}, ...names.map(name => ({module: 'ring3', name, kind: 'function'}))];
  assert.deepEqual(WebAssembly.Module.imports(module), imports); assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  const child = new WebAssembly.Instance(module, {env: {memory: ctx.memory}, ring3: ctx.api}); assert.equal(child.exports.run.length, 4);
  const file = `${ctx.owner}-${ctx.ordinal}-${phase}.wasm`, saved = artifact(file, bytes), unit = {...binding, bytes, file, run: child.exports.run, version: memory ? 2 : 1};
  modules.push({owner: ctx.owner, context: ctx.ordinal, phase, bank: ctx.bank.name, specs, instructions: tail ? 4 : ctx.bank.instructions, ...binding,
    exit_version: unit.version, imports, run_arity: 4, identity: identity(ctx, unit), artifact: saved,
    code_page_sha256: hash(ctx.pages.get(PC)), source_spans: specs.map(([entry, length]) => {const sourceBytes = ctx.pages.get(PC).subarray(entry - PC, entry - PC + length); return {entry, length, hex: sourceBytes.toString('hex'), sha256: hash(sourceBytes)};})});
  counts.modules++; ctx.units.push(unit); return unit;
}
function seed(ctx, registers, pc, flags) {ctx.expected.set(state(registers, pc, flags)); ctx.expected.set(exit(2, 3, 0), 56); ctx.expected.writeUInt32LE(0, 96); ctx.expected.set(helper(0xdecafbad), 100); refresh(ctx).bytes.set(ctx.expected.subarray(0, 140), ctx.base); check(ctx, 'one initial CPU/helper seed'); counts.seeds++;}
function cancel(ctx, value) {ctx.expected.writeUInt32LE(value, 96); refresh(ctx).view.setUint32(ctx.base + 96, value, true);}
function run(ctx, unit, budget, label, outcome = {}, status = 0, malformed = false) {
  check(ctx, `before ${label}`); const before = snapshot(ctx), current = cpu(ctx), {registers = current.registers, pc = current.pc, flags = current.flags, reason = 1, retired = 1, detail = 0, fault = 0, access = 0, helperBytes} = outcome;
  if (status === 0) {ctx.expected.set(state(registers, pc, flags)); ctx.expected.set(exit(unit.version, reason, retired, detail, fault, access), 56); if (helperBytes !== undefined) ctx.expected.set(helperBytes, 100);}
  const pointers = malformed ? [0xffffffff, 0xffffffff, 0xffffffff] : [ctx.base, ctx.base + 56, ctx.base + 96];
  assert.equal(unit.run(pointers[0], pointers[1], budget, pointers[2]), status, label); check(ctx, label); counts.generated_calls++;
  runs.push({context: ctx.ordinal, module: unit.file, label, budget, status, reason: status ? null : reason, retired: status ? 0 : retired, pc: cpu(ctx).pc}); return {before, after: snapshot(ctx)};
}
function guard(ctx, unit, wrong) {const key = ctx.low ^ Number(wrong === 'key'); return ctx.owner === 'replacement'
  ? ctx.api.guard(key, ctx.high, unit.generation + Number(wrong === 'identity'), ctx.base, ctx.base + 56, ctx.base + 96)
  : ctx.api.guard_resident(key, ctx.high, unit.low ^ Number(wrong === 'identity'), unit.high, ctx.base, ctx.base + 56, ctx.base + 96);}
function wordAt(ctx, address) {const bytes = Buffer.alloc(4); for (let index = 0; index < 4; index++) {const at = address + index; bytes[index] = ctx.pages.get(Math.floor(at / 4096) * 4096)[at % 4096];} return bytes.readUInt32LE();}
function observeWord(ctx, address, value) {check(ctx, 'before diagnostic Read32'); assert.equal(ctx.api.read32(address), 0); ctx.expected.set(helper(value), 100); check(ctx, 'diagnostic Read32 changes helper only'); counts.diagnostic_read32_calls++;}
function pages(ctx, label, addresses = [...ctx.pages.keys()]) {
  for (const address of addresses) {
    const expected = ctx.pages.get(address), permissions = ctx.permissions.get(address), actual = Buffer.alloc(4096); assert.ok(expected);
    if (!(permissions & 1)) protect(ctx, address, permissions | 1);
    for (let offset = 0; offset < 4096; offset += 4) {observeWord(ctx, address + offset, expected.readUInt32LE(offset)); actual.writeUInt32LE(refresh(ctx).view.getUint32(ctx.base + 120, true), offset);}
    assert.deepEqual(actual, expected, 'complete current declared page'); if (!(permissions & 1)) protect(ctx, address, permissions);
    const saved = artifact(`page-${pagesRead.length}.bin`, actual); pagesRead.push({context: ctx.ordinal, label, address, permissions, expected_sha256: hash(expected), artifact: saved}); counts.pages++;
  }
}
function byteAt(ctx, address) {return ctx.pages.get(Math.floor(address / 4096) * 4096)[address % 4096];}
function observeTargetWord(ctx, address) {const aligned = Math.floor(address / 4) * 4; observeWord(ctx, aligned, wordAt(ctx, aligned));}
function absentNeighbor(ctx) {
  assert.ok(!ctx.pages.has(NEXT)); check(ctx, 'before unmapped neighbor Read8'); assert.equal(ctx.api.read8(NEXT), 0);
  ctx.expected.set(readHelper(0, 1, NEXT), 100); check(ctx, 'unmapped adjacent page remains absent'); counts.absent_neighbor_read8_calls++;
}
function target(ctx, unit, scan, address, group, reason = 1) {
  const current = cpu(ctx), parent = PARENTS[scan.source], oldMemory = byteAt(ctx, address);
  const oldParent = current.registers[parent], oldSource = byteFrom(oldParent, scan.source), result = add8(oldMemory, oldSource, current.flags);
  const registers = [...current.registers]; registers[parent] = insertByte(oldParent, scan.source, oldMemory); patch(ctx, address, Buffer.from([result.value]));
  const evidence = run(ctx, unit, 1, `${group}: full target state before consumers`, {registers, pc: scan.next, flags: result.flags, reason, helperBytes: storeHelper()});
  counts.xadds++; counts[group]++; const row = {context: ctx.ordinal, group, source: scan.source, source_name: BYTE_REGISTERS[scan.source], parent,
    old_parent: oldParent, new_parent: registers[parent], address, width: 1, old_memory: oldMemory, old_source: oldSource,
    sum: result.value, flags: result.flags, pc: scan.next, retired: 1, ...evidence}; targetRows.push(row); return row;
}
function consumers(ctx, unit, scan) {
  const current = cpu(ctx), registers = [...current.registers], flags = current.flags;
  registers[0] = insertByte(registers[0], 0, flags & 1); run(ctx, unit, 1, 'SETB reads live CF', {registers, pc: scan.next + 3, flags}); counts.setb++;
  registers[2] = insertByte(registers[2], 2, Number(Boolean(flags & 0x800))); run(ctx, unit, 1, 'SETO reads live OF', {registers, pc: scan.next + 6, flags}); counts.seto++;
  if (scan.canary) {registers[3] = 0xdeadbeef; run(ctx, unit, 1, 'current canary after consumers', {registers, pc: scan.next + 11, flags}); counts.canaries++;}
  run(ctx, unit, 1, 'following JMP', {registers, pc: COLD, flags}); counts.jumps++;
}
function preflight(ctx, unit) {
  run(ctx, unit, 0, 'zero budget before memory', {retired: 0}); counts.preflight_calls++;
  cancel(ctx, 1); for (const budget of [1, 0]) {run(ctx, unit, budget, 'cancel before memory and budget', {reason: 2, retired: 0}); counts.preflight_calls++;} cancel(ctx, 0);
}
function wrongOwners(ctx, unit) {for (const wrong of ['key', 'identity']) pure(ctx, () => guard(ctx, unit, wrong), `wrong ${wrong}`, 3);}
function finish(ctx, unit) {
  run(ctx, unit, 1, 'cold entry', {reason: 3, retired: 0}); counts.cold_calls++;
  run(ctx, unit, 0, 'malformed live pointers', {}, 1, true); counts.malformed_calls++; wrongOwners(ctx, unit);
  pure(ctx, () => ctx.api.close(), 'close'); run(ctx, unit, 0, 'closed before malformed pointers', {}, 5, true); counts.closed_calls++;
}
function normal(owner) {
  const ctx = fresh(owner, banks.normal, 'normal'), unit = compile(ctx); pages(ctx, 'initial');
  for (const scan of ctx.bank.scans) for (const [left, right] of ANCHORS) for (const flags of FLAGS) {
    const registers = [...REG], parent = PARENTS[scan.source]; registers[parent] = insertByte(registers[parent], scan.source, right);
    upload(ctx, BYTE, Buffer.from([left]), 'normal operand'); seed(ctx, registers, scan.target, flags);
    if (scan.source === 0 && left === 0 && right === 0 && flags === 2) preflight(ctx, unit);
    target(ctx, unit, scan, BYTE, 'normal_targets'); observeTargetWord(ctx, BYTE); consumers(ctx, unit, scan);
  }
  pages(ctx, 'final normal'); finish(ctx, unit);
}
function special(owner) {
  const ctx = fresh(owner, banks.special, 'aliases, endpoints and faults'), unit = compile(ctx); pages(ctx, 'initial');
  for (let form = 0; form < 5; form++) for (const value of [0x7f, 0x80]) for (const flags of FLAGS) {
    const scan = ctx.bank.scans[form], registers = [...REG]; let address;
    if (form === 0) {registers[0] = BYTE; address = registers[0];}
    if (form === 1) {registers[1] = 0x103; registers[7] = 0x3c14; address = registers[7] + registers[1] * 4 - 16;}
    if (form === 2) {registers[3] = 0xcd0; address = registers[3] + registers[3] * 4;}
    if (form === 3) {registers[4] = BYTE; address = registers[4];}
    if (form === 4) {registers[5] = BYTE; address = registers[5];}
    assert.equal(address, BYTE); upload(ctx, address, Buffer.from([value]), 'alias operand'); seed(ctx, registers, scan.target, flags);
    target(ctx, unit, scan, address, 'alias_targets'); observeTargetWord(ctx, address); consumers(ctx, unit, scan);
  }
  pages(ctx, 'after aliases'); const scan = ctx.bank.scans[5];
  for (const address of [0x4fff, 0xffffffff]) for (const value of [0x7f, 0xff]) for (const flags of FLAGS) {
    const registers = [...REG]; registers[1] = insertByte(registers[1], 1, 1); registers[7] = address;
    upload(ctx, address, Buffer.from([value]), 'endpoint operand'); seed(ctx, registers, scan.target, flags);
    target(ctx, unit, scan, address, 'endpoint_targets'); observeTargetWord(ctx, address); consumers(ctx, unit, scan);
  }
  pages(ctx, 'after width-one endpoints'); absentNeighbor(ctx);
  for (const shape of faultShapes) for (const seedFlags of FLAGS) {
    protect(ctx, DATA, 3); upload(ctx, shape.address, Buffer.from([0x7f]), 'fault operand');
    if (shape.missing !== undefined) {pure(ctx, () => ctx.api.unmap(DATA, 1), 'unmap byte data page'); ctx.pages.delete(DATA); ctx.permissions.delete(DATA);}
    if (shape.denied !== undefined) protect(ctx, shape.denied, shape.permissions);
    const registers = [...REG]; registers[1] = insertByte(registers[1], 1, 1); registers[3] = 0xffffffff; registers[7] = shape.address;
    seed(ctx, registers, scan.entry, seedFlags); registers[3] = 0;
    const prefixFlags = seedFlags === 2 ? 0x57 : 0x457;
    const failure = {registers, pc: scan.target, flags: prefixFlags, reason: 5, detail: shape.detail, fault: shape.fault, access: shape.access,
      helperBytes: shape.access === 1 ? readHelper(0, shape.detail, shape.fault) : storeHelper(shape.detail, shape.fault)};
    const first = run(ctx, unit, 2, `${shape.name}: retired ADD then fault`, {...failure, retired: 1}); counts.retired_add_prefixes++; counts.fault_calls++;
    const firstArena = saveArena(ctx, `fault-${faults.length}-first`), existing = shape.missing !== undefined ? [KEEP, TOP] : [DATA];
    pages(ctx, `${shape.name} first fault`, existing);
    const retry = run(ctx, unit, 1, `${shape.name}: unchanged retry`, {...failure, retired: 0}); counts.fault_calls++;
    const retryArena = saveArena(ctx, `fault-${faults.length}-retry`); pages(ctx, `${shape.name} retry`, existing);
    const changed = seedFlags === 2, value = changed ? 0xff : 0x7f;
    if (shape.missing !== undefined) {map(ctx, DATA); upload(ctx, shape.address, Buffer.from([value]), 'map and data repair'); if (changed) counts.changed_data_repairs++; else counts.restored_map_repairs++;}
    else {protect(ctx, shape.denied, 3); if (changed) {upload(ctx, shape.address, Buffer.from([value]), 'permission and changed data repair'); counts.changed_data_repairs++;} else counts.permission_only_repairs++;}
    pure(ctx, () => guard(ctx, unit), 'data-only repair retains owner'); assert.equal(byteAt(ctx, shape.address), value);
    const repaired = target(ctx, unit, scan, shape.address, 'repaired_targets'); observeTargetWord(ctx, shape.address); consumers(ctx, unit, scan);
    pages(ctx, `${shape.name} repaired`, [DATA]);
    faults.push({context: ctx.ordinal, ...shape, width: 1, seed_flags: seedFlags, prefix_flags: prefixFlags,
      first, retry, first_arena: firstArena, retry_arena: retryArena, repair_value: value,
      repair_mode: shape.missing !== undefined ? 'map+data' : changed ? 'permission+changed-data' : 'permission-only', repaired});
  }
  pages(ctx, 'final special'); finish(ctx, unit);
}
function smc(owner, changing) {
  const ctx = fresh(owner, banks.special, changing ? 'changing consumed MOV immediate byte' : 'equal consumed MOV immediate byte'), old = compile(ctx), scan = ctx.bank.scans[6];
  pages(ctx, 'initial SMC'); wrongOwners(ctx, old); const registers = [...REG]; registers[1] = changing ? 0x23450120 : 0x23450020; registers[7] = scan.entry + 1;
  seed(ctx, registers, scan.entry, 0xcd7); registers[6] = 0x1234567f;
  const mov = run(ctx, old, 1, 'MOV before XADD retires once', {registers, pc: scan.target, flags: 0xcd7}); counts.retired_mov_prefixes++;
  const beforeModule = physical(ctx, old), row = target(ctx, old, scan, registers[7], 'code_targets', 6);
  assert.equal(row.old_memory, 0x7f); assert.equal(row.sum, changing ? 0x80 : 0x7f); assert.equal(row.flags, changing ? 0xc92 : 0x402);
  assert.equal(cpu(ctx).registers[1], 0x23457f20); assert.equal(cpu(ctx).registers[6], 0x1234567f);
  const targetArena = saveArena(ctx, 'SMC-target'); counts[changing ? 'changing_code_stores' : 'same_value_code_stores']++;
  cancel(ctx, 1); run(ctx, old, 0, 'stale old child before cancelled malformed pointers', {}, 4, true); counts.stale_calls++;
  const retainedOld = physical(ctx, old); assert.deepEqual(retainedOld, beforeModule, 'old allocation immediately after stale before diagnostics or publication');
  cancel(ctx, 0); const committed = snapshot(ctx), current = compile(ctx, 'current-tail', scan);
  assert.notDeepEqual(identity(ctx, current), identity(ctx, old)); assert.deepEqual(snapshot(ctx), committed, 'fresh tail publication retains CPU/exit/helper');
  assert.equal(current.version, 1); consumers(ctx, current, scan);
  assert.equal(cpu(ctx).registers[6], 0x1234567f); assert.equal(cpu(ctx).registers[1], 0x23457f20); observeTargetWord(ctx, registers[7]); pages(ctx, 'fresh tail without replay');
  const tailBefore = physical(ctx, current), tailByte = ctx.pages.get(PC)[scan.next - PC]; assert.equal(tailByte, 0x0f);
  upload(ctx, scan.next, Buffer.from([tailByte]), 'same consumed-tail byte'); cancel(ctx, 1);
  run(ctx, current, 0, 'stale fresh tail before cancelled malformed pointers', {}, 4, true); counts.stale_calls++;
  const retainedTail = physical(ctx, current); assert.deepEqual(retainedTail, tailBefore, 'tail allocation immediately after stale before close');
  pure(ctx, () => ctx.api.close(), 'close stale owners');
  for (const unit of [old, current]) {run(ctx, unit, 0, 'closed child before stale and malformed pointers', {}, 5, true); counts.closed_calls++;}
  codeStores.push({context: ctx.ordinal, changing, address: registers[7], consumed_instruction: 'MOV ESI, imm32', original_immediate: 0x1234567f,
    changed_immediate: changing ? 0x12345680 : 0x1234567f, mov, target: row, target_arena: targetArena, store_status: {value: 11, authority: 'source-inferred; not intercepted'},
    observed_exit: 6, observed_retired: 1, old_identity: identity(ctx, old), current_identity: identity(ctx, current),
    retained_old_after_stale: retainedOld, retained_tail_after_stale: retainedTail, fresh_tail_exit_version: 1,
    replay_discriminator: changing ? {canonical_esi: 0x1234567f, canonical_ecx: 0x23457f20, canonical_memory: 0x80,
      replayed_mov_esi: 0x12345680, replayed_xadd_ecx: 0x23458020, replayed_xadd_memory: 0xff, replayed_xadd_flags: 0x486} : null});
}
for (const owner of OWNERS) {normal(owner); special(owner); for (const changing of [false, true]) smc(owner, changing);}
for (const [name, value] of Object.entries(formulas)) assert.equal(counts[name], value, `observed ${name}`);
assert.equal(counts.xadds, targetCount); assert.equal(counts.setb, targetCount); assert.equal(counts.seto, targetCount); assert.equal(counts.jumps, targetCount);
assert.equal(counts.seeds, targetCount, 'one seed per independent scenario, none during repair or tail continuation');
assert.equal(counts.contexts, OWNERS.length * 4); assert.equal(counts.modules, OWNERS.length * (2 + 2 * 2));
assert.equal(counts.retired_add_prefixes, formulas.repaired_targets); assert.equal(counts.retired_mov_prefixes, formulas.code_targets);
assert.equal(counts.fault_calls, formulas.repaired_targets * 2); assert.equal(counts.canaries, formulas.code_targets);
assert.equal(counts.permission_only_repairs, OWNERS.length * 2); assert.equal(counts.changed_data_repairs, OWNERS.length * faultShapes.length); assert.equal(counts.restored_map_repairs, OWNERS.length);
assert.equal(counts.same_value_code_stores, OWNERS.length); assert.equal(counts.changing_code_stores, OWNERS.length);
assert.equal(counts.stale_calls, formulas.code_targets * 2); assert.equal(counts.closed_calls, OWNERS.length * (2 + 2 * 2));
assert.equal(counts.preflight_calls, OWNERS.length * 3); assert.equal(counts.cold_calls, OWNERS.length * 2); assert.equal(counts.malformed_calls, OWNERS.length * 2);
const expectedCalls = targetCount * 4 + counts.fault_calls + counts.retired_mov_prefixes + counts.canaries
  + counts.preflight_calls + counts.cold_calls + counts.malformed_calls + counts.stale_calls + counts.closed_calls;
assert.equal(counts.generated_calls, expectedCalls); assert.equal(runs.length, expectedCalls);
const retirements = runs.reduce((total, row) => total + row.retired, 0);
assert.equal(retirements, targetCount * 4 + counts.canaries + counts.retired_mov_prefixes + counts.retired_add_prefixes);
assert.equal(counts.pages, 124, '16 normal +76 alias/endpoint/fault +32 SMC complete pages');
assert.equal(counts.diagnostic_read32_calls, 124 * 1024 + targetCount, 'complete pages plus one aligned target word');
assert.equal(counts.absent_neighbor_read8_calls, OWNERS.length);
assert.deepEqual(sourceHashes(), testHashes, 'test/helper sources unchanged'); assert.equal(hash(readFileSync(enginePath)), engineSha256, 'engine input unchanged');
const artifactNames = [...artifacts.map(row => row.path), 'result.json']; assert.equal(new Set(artifactNames).size, artifactNames.length);
const rawArenas = formulas.repaired_targets * 2 + formulas.code_targets;
assert.equal(artifacts.filter(row => row.path.startsWith('arena-')).length, rawArenas); assert.equal(rawArenas, 28);
assert.equal(artifactNames.length, counts.pages + rawArenas + counts.modules + 5); assert.equal(artifactNames.length, 169);
const result = {status: 'ok', engine_sha256: engineSha256, engine_bytes: engineBytes.length, tools: {node: process.version, v8: process.versions.v8},
  counts, formulas: {...formulas, targets: targetCount, fault_calls: formulas.repaired_targets * 2, generated_calls: expectedCalls, retirements, pages: 124, raw_arenas: rawArenas, diagnostic_read32_calls: 124 * 1024 + targetCount, artifact_files: 169},
  contexts, modules, targets: targetRows, faults, code_stores: codeStores, pages: pagesRead, generated_runs: runs, host_inputs: hostInputs,
  source, test_sha256: testHashes, oracle: {anchors: ANCHORS, flag_seeds: FLAGS, registers: REG, byte_registers: BYTE_REGISTERS, parent_registers: PARENTS, hex_lanes: HEX_LANES, pattern_sha256: hash(pattern), arena_bytes: SIZE,
    banks: Object.fromEntries(Object.entries(banks).map(([name, entry]) => [name, {hex: entry.bytes.toString('hex'), sha256: hash(entry.bytes), specs: entry.specs, scans: entry.scans, instructions: entry.instructions}])),
    arithmetic: 'widened unsigned numeric sum modulo256; separate signed numeric-range overflow at -128..127; low-nibble carry and full byte bitstring parity; old memory/source/original EA captured before exchange; independent hex byte extraction/replacement preserves the other parent bytes',
    checkpoint_policy: 'complete live arena compared around every generated run, explicit upload/map/unmap/protect/guard/close, diagnostic Read32 and isolated unmapped-neighbor Read8; compiler requests/getter groups checked at publication, initialization checked after open/getter group; actual CPU/exit/helper snapshots saved for target and fault rows; selected fault/SMC raw arenas and complete observed pages saved physically',
    binding_policy: 'actual managed engine read8/store8 or store_resident8 directly imported; no synthetic helpers; R3MHv2 read/R3MHv3 store length1, separate aligned Read32 diagnostics update R3MHv1; source-inferred Store8 status11 is distinct from observed helper success/exit6',
    continuation_policy: 'declared data/permission repairs, observed diagnostic helper updates, cancellation writes and compilation requests between live guest steps; no CPU/helper reseed after a fault or consumed MOV/XADD; pure current tail has exit v1 and retains helper'},
  artifacts, artifact_census: {modules: modules.length, pages: counts.pages, raw_arenas: rawArenas, engine_files: 1, program_banks: 2, primary_excerpts: 1, result_files: 1, total: artifactNames.length, names: artifactNames},
  claim: 'finite prefix-free flat32 memory XADD8 in actual managed replacement/resident Wasm: all eight low/high source bytes and eight literal ADD8 edges under FLAGS2/cd7; five original-EA source/parent/base/index aliases including BH parent as both, valid width1 page/top endpoints with aligned diagnostics and genuinely unmapped5000, three precise Read8/Write8 faults with retired ADD, unchanged zero-retirement retry and declared permission/data-only repair at same CPU/module; full target CPU/partial parent before live SETB/SETO/JMP; equal/changing stores to the low byte of a truly consumed MOV immediate commit helper/source/flags/EIP and one retirement with exit6, then stale4 with immediate whole live old allocation retention, pure fresh tail without MOV/XADD replay, same-byte tail stale retention and both closed5; complete live4236 arenas and current declared pages, physical selected evidence/provenance; required offline immutable Intel inputs; no atomicity/LOCK/prefix/word/wider EA-exhaustive ISA/PE/Windows/SDK/browser/performance/fullCI/game claim'};
const resultBytes = Buffer.from(JSON.stringify(result, null, 2)); writeFileSync(join(output, 'result.json'), resultBytes, {flag: 'wx'});
assert.deepEqual(readdirSync(output).sort(), artifactNames.sort(), 'exact physical artifact census');
console.log(JSON.stringify({status: result.status, engine_sha256: engineSha256, result_sha256: hash(resultBytes), counts, output}));
