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
const SIZE = 4236, TRANSFER = 140, PC = 0x1000, COLD = PC + 128, KEEP = 0x3000, DATA = 0x4000, SECOND = 0x5000, WORD = 0x4010;
const OWNERS = ['replacement', 'resident'], FLAGS = [2, 0xcd7];
const REG = [0x1234807f, 0x23457f80, 0x34560f10, 0x4567ff00, 0x56789abc, 0x6789abcd, 0x789abcde, 0x89abcdef];
const GPRS = ['EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI'];
const ANCHORS = [
  [0, 0, 0, 0x46], [1, 1, 0, 0x46], [0, 1, 0xffffffff, 0x97], [1, 0, 1, 2],
  [0x80000000, 1, 0x7fffffff, 0x816], [0x7fffffff, 0xffffffff, 0x80000000, 0x887],
  [0x10, 1, 0x0f, 0x16], [0x80000000, 0xffffffff, 0x80000001, 0x93],
];
function compare32(accumulator, memory, flags) {
  const wide = accumulator - memory, value = (wide + 4294967296) % 4294967296;
  const signed = operand => operand < 2147483648 ? operand : operand - 4294967296;
  const difference = signed(accumulator) - signed(memory), low = value % 256;
  const even = low.toString(2).replaceAll('0', '').length % 2 === 0;
  return {value, equal: accumulator === memory, flags: 2 + (flags & 0x400) + Number(accumulator < memory) + Number(even) * 4
    + Number(accumulator % 16 < memory % 16) * 0x10 + Number(value === 0) * 0x40
    + Number(value >= 2147483648) * 0x80 + Number(difference < -2147483648 || difference > 2147483647) * 0x800};
}
for (const [accumulator, memory, value, flags] of ANCHORS) assert.deepEqual(compare32(accumulator, memory, 2), {value, flags, equal: accumulator === memory}, 'literal CMP anchor');
const SMC = [
  {name: 'equal-same', accumulator: 0x7fffffff, source: 0x7fffffff, memory: 0x7fffffff, flags: 0x446, equal: true, changing: false},
  {name: 'equal-changing', accumulator: 0x7fffffff, source: 0x80000000, memory: 0x80000000, flags: 0x446, equal: true, changing: true},
  {name: 'mismatch-same', accumulator: 0x80000000, source: 0xdeadbeef, memory: 0x7fffffff, flags: 0xc12, equal: false, changing: false},
];
const words = values => {const bytes = Buffer.alloc(values.length * 4); values.forEach((value, index) => bytes.writeUInt32LE(value >>> 0, index * 4)); return bytes;};
function bank(name, forms, prefixes, canary, packed, instructions) {
  const bytes = Buffer.alloc(130, 0xcc), scans = [], specs = []; let offset = 0;
  for (const [index, form] of forms.entries()) {
    const tail = [0x0f, 0x94, 0xc0, 0x0f, 0x92, 0xc2, ...(index === canary ? [0xbb, 0xef, 0xbe, 0xad, 0xde] : [])];
    const length = form.length + tail.length + 2, entry = PC + offset;
    bytes.set([...form, ...tail, 0xeb, 128 - offset - length], offset);
    scans.push({entry, target: entry + (prefixes[index] ?? 0), next: entry + form.length, source: name === 'normal' ? index : [1, 0, 0, 4, 5, 1, 1][index], canary: index === canary});
    specs.push([entry, length]); offset += length;
  }
  assert.equal(offset, packed); assert.ok(specs.length <= 8 && instructions <= 64); bytes.set([0x0f, 0x0b], 128);
  assert.deepEqual(readFileSync(join(output, `${name}.x86`)), bytes, 'independent Rust/JS program bytes');
  return {name, bytes, specs, scans, instructions};
}
const banks = {
  normal: bank('normal', Array.from({length: 8}, (_, source) => [0x0f, 0xb1, source << 3 | 5, 0x10, 0x40, 0, 0]), [], -1, 120, 32),
  special: bank('special', [[0x0f, 0xb1, 0x08], [0x0f, 0xb1, 0x44, 0x87, 0xf0], [0x0f, 0xb1, 0x04, 0x80],
    [0x0f, 0xb1, 0x24, 0x24], [0x0f, 0xb1, 0x6d, 0], [0x83, 0xc3, 1, 0x0f, 0xb1, 0x0f],
    [0xbe, 0xff, 0xff, 0xff, 0x7f, 0x0f, 0xb1, 0x0f]], {5: 3, 6: 5}, 6, 95, 31),
};
assert.deepEqual(banks.special.scans.map(scan => scan.entry - PC), [0, 11, 24, 36, 48, 60, 74]);
const faultShapes = [
  {name: 'cross-read-unmapped', detail: 1, fault: SECOND, access: 1, missing: SECOND},
  {name: 'cross-read-denied', detail: 2, fault: SECOND, access: 1, denied: SECOND, permissions: 2},
  {name: 'cross-first-read-only', detail: 2, fault: 0x4ffe, access: 2, denied: DATA, permissions: 1},
  {name: 'cross-second-read-only', detail: 2, fault: SECOND, access: 2, denied: SECOND, permissions: 1},
];
const formulas = {normal_targets: OWNERS.length * (8 * 2 + ANCHORS.length) * FLAGS.length,
  alias_targets: OWNERS.length * 5 * 2 * FLAGS.length, crossing_targets: OWNERS.length * 2 * FLAGS.length,
  repaired_targets: OWNERS.length * faultShapes.length * FLAGS.length, code_targets: OWNERS.length * SMC.length};
const targetCount = Object.values(formulas).reduce((total, value) => total + value, 0); assert.equal(targetCount, 166);
const counts = {contexts: 0, modules: 0, seeds: 0, cmpxchgs: 0, normal_targets: 0, main_targets: 0, literal_targets: 0,
  alias_targets: 0, crossing_targets: 0, repaired_targets: 0, code_targets: 0, equal_targets: 0, mismatch_targets: 0,
  setz: 0, setb: 0, jumps: 0, canaries: 0, fault_calls: 0, retired_add_prefixes: 0, retired_mov_prefixes: 0,
  permission_only_repairs: 0, changed_data_repairs: 0, restored_map_repairs: 0, comparator_flip_repairs: 0,
  same_value_code_stores: 0, changing_code_stores: 0, stale_calls: 0, closed_calls: 0, preflight_calls: 0,
  cold_calls: 0, malformed_calls: 0, generated_calls: 0, arena_checks: 0, pages: 0, diagnostic_read32_calls: 0};
const contexts = [], modules = [], targetRows = [], faults = [], codeStores = [], pagesRead = [], runs = [], hostInputs = [], artifacts = [];
function artifact(path, bytes, write = true) {
  assert.ok(!artifacts.some(row => row.path === path)); if (write) writeFileSync(join(output, path), bytes, {flag: 'wx'});
  const row = {path, length: bytes.length, sha256: hash(bytes)}; artifacts.push(row); return row;
}
artifact('engine.wasm', engineBytes); for (const entry of Object.values(banks)) artifact(`${entry.name}.x86`, entry.bytes, false);
const source = {provenance: 'existing physically pinned local Intel primary; no fresh download or newest-edition claim',
  prerequisite_policy: 'ignored immutable local PDF/full-text inputs are required offline before guest execution; no bootstrap or clean-checkout/fullCI claim',
  order: '253666-093US', edition: 'September 2026', pdf_path: 'target/p2-memory-binary-spec/253666-093-sdm-vol-2a.pdf',
  pdf_bytes: 3404694, pdf_sha256: '87c5acb6f27e24d9d364841a0d2346c91a8482e36f403409954e2d2a8c817bac',
  full_text_path: 'target/p2-memory-binary-spec/253666-093-sdm-vol-2a.txt', full_text_bytes: 1557075,
  full_text_sha256: 'fa7f27a4b5f830e007fa6a967e53436aa7051fe99d8d56303efcf4148b0a5eaf', chapter_lines: [15502, 15590],
  chapter_bytes: 4590, chapter_sha256: 'ffa52e2a5725c967769683fc49278ea3de07f8a40935c82565e24d8ede183fa1',
  pdf_pages_zero_based: [311, 312], printed_pages: ['3-194', '3-195'], chapter_separator_policy: 'includes internal PDF PAGE 313 separator; excludes next chapter PDF PAGE 314'};
for (const kind of ['pdf', 'full_text']) {const bytes = readFileSync(join(root, source[`${kind}_path`])); assert.equal(bytes.length, source[`${kind}_bytes`]); assert.equal(hash(bytes), source[`${kind}_sha256`]);}
const chapter = Buffer.from(readFileSync(join(root, source.full_text_path), 'utf8').split('\n').slice(15501, 15590).join('\n') + '\n');
assert.equal(chapter.length, source.chapter_bytes); assert.equal(hash(chapter), source.chapter_sha256); source.saved_chapter = artifact('intel-cmpxchg.txt', chapter);
const sourcePaths = ['engine/tests/cpu_memory_cmpxchg32.rs', 'engine/tests/cpu_memory_cmpxchg32_wasm.rs', 'engine/tests/fixtures/p2-memory-cmpxchg32/run.mjs', 'engine/tests/fixtures/support/engine.mjs'];
const sourceHashes = () => Object.fromEntries(sourcePaths.map(path => [path, hash(readFileSync(join(root, path)))]));
const testHashes = sourceHashes();
const productPaths = ['engine/src/cpu/x86/ir.rs', 'engine/src/cpu/x86/decode/lower.rs', 'engine/src/cpu/dbt/region.rs', 'engine/src/cpu/dbt/wasm/memory.rs', 'engine/src/cpu/dbt/wasm/integer.rs'];
const productHashes = () => Object.fromEntries(productPaths.map(path => [path, hash(readFileSync(join(root, path)))]));
const productPins = productHashes();
function record(magic, version, size, fields) {
  const bytes = Buffer.alloc(size); bytes.write(magic); bytes.writeUInt16LE(version, 4); bytes.writeUInt16LE(1, 6); bytes.writeUInt32LE(size, 8);
  fields.forEach((value, index) => bytes.writeUInt32LE(value >>> 0, 16 + index * 4)); return bytes;
}
const state = (registers, pc, flags) => record('R3ST', 1, 56, [...registers, pc, flags]);
const exit = (version, reason, retired, detail = 0, address = 0, access = 0) => record('R3EX', version, 40, [reason, retired, detail, address, access, access ? 4 : 0]);
const helper = (value = 0, detail = 0, address = 0, access = 0) => record('R3MH', 1, 40, [access ? 1 : 0, value, detail, address, access, access ? 4 : 0]);
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
  const arities = {open: 3, close: 0, arena_ptr: 0, map: 3, unmap: 2, protect: 3, upload: 2, read32: 1,
    compile_entries: 2, compile_resident: 1, generation: 0, module_ptr: 0, module_len: 0, guard: 6, guard_resident: 7, store32: 2, store_resident32: 6};
  for (const [name, arity] of Object.entries(arities)) {api[name] = instance.exports[`ring3_abi_v1_${name}`]; assert.equal(typeof api[name], 'function', name); assert.equal(api[name].length, arity, name);}
  const ctx = {owner, ordinal, bank: selected, label, api, memory: instance.exports.memory, low: ordinal, high: 0x434d5032,
    expected: Buffer.alloc(SIZE), pages: new Map(), permissions: new Map(), units: []};
  assert.equal(api.open(4, ctx.low, ctx.high), 0); ctx.base = api.arena_ptr() >>> 0;
  ctx.expected.set(state(Array(8).fill(0), 0, 2)); ctx.expected.set(exit(1, 1, 0), 56); ctx.expected.set(helper(), 100); check(ctx, 'independently initialized arena');
  const codePage = Buffer.alloc(4096); codePage.set(selected.bytes); map(ctx, PC, codePage, 7);
  for (const address of [KEEP, DATA, SECOND]) map(ctx, address);
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
  const names = [ctx.owner === 'replacement' ? 'guard' : 'guard_resident', ...(memory ? ['read32', ctx.owner === 'replacement' ? 'store32' : 'store_resident32'] : [])];
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
function observeWord(ctx, address, value) {check(ctx, 'before diagnostic Read32'); assert.equal(ctx.api.read32(address), 0); ctx.expected.set(helper(value), 100); check(ctx, 'diagnostic Read32 changes helper only'); counts.diagnostic_read32_calls++; return refresh(ctx).view.getUint32(ctx.base + 120, true);}
function pages(ctx, label, addresses = [...ctx.pages.keys()]) {
  for (const address of addresses) {
    const expected = ctx.pages.get(address), permissions = ctx.permissions.get(address), actual = Buffer.alloc(4096); assert.ok(expected);
    if (!(permissions & 1)) protect(ctx, address, permissions | 1);
    for (let offset = 0; offset < 4096; offset += 4) {observeWord(ctx, address + offset, expected.readUInt32LE(offset)); actual.writeUInt32LE(refresh(ctx).view.getUint32(ctx.base + 120, true), offset);}
    assert.deepEqual(actual, expected, 'complete current declared page'); if (!(permissions & 1)) protect(ctx, address, permissions);
    const saved = artifact(`page-${pagesRead.length}.bin`, actual); pagesRead.push({context: ctx.ordinal, label, address, permissions, expected_sha256: hash(expected), artifact: saved}); counts.pages++;
  }
}
function target(ctx, unit, scan, address, group, reason = 1) {
  const current = cpu(ctx), oldMemory = wordAt(ctx, address), oldAccumulator = current.registers[0], oldSource = current.registers[scan.source];
  const result = compare32(oldAccumulator, oldMemory, current.flags), stored = result.equal ? oldSource : oldMemory;
  const registers = [...current.registers]; if (!result.equal) registers[0] = oldMemory; patch(ctx, address, words([stored]));
  const evidence = run(ctx, unit, 1, `${group}: full target state before consumers`, {registers, pc: scan.next, flags: result.flags, reason, helperBytes: helper()});
  counts.cmpxchgs++; counts[group]++; counts[result.equal ? 'equal_targets' : 'mismatch_targets']++;
  const row = {context: ctx.ordinal, group, source: scan.source, source_name: GPRS[scan.source], address, width: 4,
    old_accumulator: oldAccumulator, old_memory: oldMemory, old_source: oldSource, new_accumulator: registers[0], stored_value: stored,
    comparison_result: result.value, equal: result.equal, flags: result.flags, pc: scan.next, retired: 1,
    ram_readback_phase: group === 'code_targets' ? 'after pure current-tail consumers; first-target CPU/helper captured separately' : 'before first consumer', ...evidence};
  assert.equal(Boolean(result.flags & 0x40), result.equal); targetRows.push(row); return row;
}
function targetRam(ctx, row) {
  const value = observeWord(ctx, row.address, row.stored_value); assert.equal(value, row.stored_value);
  row.ram_readback = {phase: row.ram_readback_phase, address: row.address, value, helper_hex: snapshot(ctx).helper_hex};
}
function consumers(ctx, unit, scan) {
  const current = cpu(ctx), registers = [...current.registers], flags = current.flags;
  registers[0] = Math.floor(registers[0] / 256) * 256 + Number(Boolean(flags & 0x40)); run(ctx, unit, 1, 'SETZ reads live ZF', {registers, pc: scan.next + 3, flags}); counts.setz++;
  registers[2] = Math.floor(registers[2] / 256) * 256 + (flags & 1); run(ctx, unit, 1, 'SETB reads live CF', {registers, pc: scan.next + 6, flags}); counts.setb++;
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
  const ctx = fresh(owner, banks.normal, 'sources and literal comparisons'), unit = compile(ctx); pages(ctx, 'initial');
  for (const scan of ctx.bank.scans) for (const equal of [true, false]) for (const flags of FLAGS) {
    const registers = [...REG]; registers[0] = Number(equal); upload(ctx, WORD, words([1]), 'main operand'); seed(ctx, registers, scan.target, flags);
    if (scan.source === 0 && equal && flags === 2) preflight(ctx, unit);
    const row = target(ctx, unit, scan, WORD, 'normal_targets'); assert.equal(row.equal, equal); counts.main_targets++;
    targetRam(ctx, row); consumers(ctx, unit, scan);
  }
  const scan = ctx.bank.scans[1];
  for (const [accumulator, memory, difference, expectedFlags] of ANCHORS) for (const flags of FLAGS) {
    const registers = [...REG]; registers[0] = accumulator; registers[1] = 0x12345678;
    upload(ctx, WORD, words([memory]), 'literal operand'); seed(ctx, registers, scan.target, flags);
    const row = target(ctx, unit, scan, WORD, 'normal_targets'); counts.literal_targets++;
    assert.equal(row.comparison_result, difference); assert.equal(row.flags, expectedFlags + (flags & 0x400)); targetRam(ctx, row); consumers(ctx, unit, scan);
  }
  pages(ctx, 'final normal'); finish(ctx, unit);
}
function special(owner) {
  const ctx = fresh(owner, banks.special, 'accumulator aliases and faults'), unit = compile(ctx); pages(ctx, 'initial');
  for (let form = 0; form < 5; form++) for (const equal of [true, false]) for (const flags of FLAGS) {
    const scan = ctx.bank.scans[form], registers = [...REG]; let address;
    if (form === 0) {registers[0] = WORD; address = registers[0];}
    if (form === 1) {registers[0] = 3; registers[7] = WORD + 4; address = registers[7] + registers[0] * 4 - 16;}
    if (form === 2) {registers[0] = 0xcd0; address = registers[0] + registers[0] * 4;}
    if (form === 3) {registers[4] = WORD; address = registers[4];}
    if (form === 4) {registers[5] = WORD; address = registers[5];}
    const memory = equal ? registers[0] : (registers[0] + 0x10000) % 4294967296;
    assert.equal(address, WORD); upload(ctx, address, words([memory]), 'alias operand'); seed(ctx, registers, scan.target, flags);
    const row = target(ctx, unit, scan, address, 'alias_targets'); assert.equal(row.equal, equal);
    row.alias = {form, role: ['implicit EAX is base; source ECX', 'source EAX is scaled index', 'source EAX is base and scaled index', 'source ESP is base', 'source EBP is base'][form], original_ea: address, mismatch_delta: 0x10000};
    if (!equal) {
      assert.equal(row.comparison_result, 0xffff0000); assert.equal(row.flags, 0x87 + (flags & 0x400));
      if (form < 3) {
        let hypothetical;
        if (form === 0) hypothetical = memory;
        if (form === 1) hypothetical = (registers[7] + memory * 4 - 16) % 4294967296;
        if (form === 2) hypothetical = (memory + memory * 4) % 4294967296;
        assert.equal(hypothetical, [0x14010, 0x44010, 0x54010][form]); assert.notEqual(hypothetical, address);
        assert.ok(!ctx.pages.has(Math.floor(hypothetical / 4096) * 4096));
        row.alias.premature_accumulator = {value: memory, effective_address: hypothetical, mapped: false, authority: 'independent form-specific numeric EA; hypothetical only, no premature-publication execution'};
      }
    }
    targetRam(ctx, row); consumers(ctx, unit, scan);
  }
  pages(ctx, 'after aliases'); const scan = ctx.bank.scans[5];
  for (const equal of [true, false]) for (const flags of FLAGS) {
    const registers = [...REG]; registers[0] = equal ? 0x7fffffff : 0x80000000; registers[1] = 0x12345678; registers[7] = 0x4ffe;
    upload(ctx, 0x4ffe, words([0x7fffffff]), 'crossing operand'); seed(ctx, registers, scan.target, flags);
    const row = target(ctx, unit, scan, 0x4ffe, 'crossing_targets'); assert.equal(row.equal, equal); targetRam(ctx, row); consumers(ctx, unit, scan);
  }
  pages(ctx, 'after crossing successes');
  for (const shape of faultShapes) for (const seedFlags of FLAGS) {
    protect(ctx, DATA, 3); protect(ctx, SECOND, 3); upload(ctx, 0x4ffe, words([0x7fffffff]), 'fault operand');
    if (shape.missing !== undefined) {pure(ctx, () => ctx.api.unmap(SECOND, 1), 'unmap crossing second page'); ctx.pages.delete(SECOND); ctx.permissions.delete(SECOND);}
    if (shape.denied !== undefined) protect(ctx, shape.denied, shape.permissions);
    const initialEqual = seedFlags === 2, registers = [...REG]; registers[0] = initialEqual ? 0x7fffffff : 0x80000000;
    registers[1] = 0x12345678; registers[3] = 0xffffffff; registers[7] = 0x4ffe; seed(ctx, registers, scan.entry, seedFlags);
    registers[3] = 0; const prefixFlags = initialEqual ? 0x57 : 0x457;
    const failure = {registers, pc: scan.target, flags: prefixFlags, reason: 5, detail: shape.detail, fault: shape.fault, access: shape.access, helperBytes: helper(0, shape.detail, shape.fault, shape.access)};
    const first = run(ctx, unit, 2, `${shape.name}: retired ADD then fault`, {...failure, retired: 1}); counts.retired_add_prefixes++; counts.fault_calls++;
    const firstArena = saveArena(ctx, `fault-${faults.length}-first`); pages(ctx, `${shape.name} first fault: every mapped declared page`);
    const retry = run(ctx, unit, 1, `${shape.name}: unchanged retry`, {...failure, retired: 0}); counts.fault_calls++;
    const retryArena = saveArena(ctx, `fault-${faults.length}-retry`); pages(ctx, `${shape.name} retry: every mapped declared page`);
    const changed = initialEqual, value = changed ? 0xffffffff : 0x7fffffff;
    if (shape.missing !== undefined) {map(ctx, SECOND); upload(ctx, 0x4ffe, words([value]), 'map and data repair'); if (changed) counts.changed_data_repairs++; else counts.restored_map_repairs++;}
    else {protect(ctx, shape.denied, 3); if (changed) {upload(ctx, 0x4ffe, words([value]), 'permission and changed data repair'); counts.changed_data_repairs++;} else counts.permission_only_repairs++;}
    pure(ctx, () => guard(ctx, unit), 'data-only repair retains owner'); assert.equal(wordAt(ctx, 0x4ffe), value);
    const repaired = target(ctx, unit, scan, 0x4ffe, 'repaired_targets'); assert.equal(repaired.equal, false);
    assert.equal(repaired.new_accumulator, value); assert.equal(repaired.stored_value, value); assert.equal(repaired.flags, changed ? 0x887 : 0xc12);
    if (changed) {assert.notEqual(repaired.equal, initialEqual); counts.comparator_flip_repairs++;}
    targetRam(ctx, repaired); consumers(ctx, unit, scan); pages(ctx, `${shape.name} repaired`, [DATA, SECOND]);
    faults.push({context: ctx.ordinal, ...shape, address: 0x4ffe, width: 4, seed_flags: seedFlags, prefix_flags: prefixFlags,
      declared_initial_equal: initialEqual, comparison_label_policy: shape.access === 1 ? 'declared operands; read fault precedes comparison' : 'declared operands; checked Write4 fault required for either comparator result',
      first, retry, first_arena: firstArena, retry_arena: retryArena, repair_value: value,
      repair_mode: shape.missing !== undefined ? 'map+data' : changed ? 'permission+changed-data' : 'permission-only', repaired});
  }
  pages(ctx, 'final special'); finish(ctx, unit);
}
function smc(owner, kind) {
  const ctx = fresh(owner, banks.special, `${kind.name} consumed MOV immediate write`), old = compile(ctx), scan = ctx.bank.scans[6];
  pages(ctx, 'initial SMC'); wrongOwners(ctx, old); const registers = [...REG]; registers[0] = kind.accumulator; registers[1] = kind.source; registers[7] = scan.entry + 1;
  seed(ctx, registers, scan.entry, 0xcd7); registers[6] = 0x7fffffff;
  const mov = run(ctx, old, 1, 'MOV before CMPXCHG retires once', {registers, pc: scan.target, flags: 0xcd7}); counts.retired_mov_prefixes++;
  const beforeModule = physical(ctx, old), row = target(ctx, old, scan, registers[7], 'code_targets', 6);
  assert.equal(row.old_memory, 0x7fffffff); assert.equal(row.stored_value, kind.memory); assert.equal(row.equal, kind.equal); assert.equal(row.flags, kind.flags);
  assert.equal(cpu(ctx).registers[0], 0x7fffffff); assert.equal(cpu(ctx).registers[1], kind.source); assert.equal(cpu(ctx).registers[6], 0x7fffffff);
  const targetArena = saveArena(ctx, 'SMC-target'); counts[kind.changing ? 'changing_code_stores' : 'same_value_code_stores']++;
  cancel(ctx, 1); run(ctx, old, 0, 'stale old child before cancelled malformed pointers', {}, 4, true); counts.stale_calls++;
  const retainedOld = physical(ctx, old); assert.deepEqual(retainedOld, beforeModule, 'old allocation immediately after stale before diagnostics or publication');
  cancel(ctx, 0); const committed = snapshot(ctx), current = compile(ctx, 'current-tail', scan);
  assert.notDeepEqual(identity(ctx, current), identity(ctx, old)); assert.deepEqual(snapshot(ctx), committed, 'fresh tail publication retains CPU/exit/helper');
  assert.equal(current.version, 1); consumers(ctx, current, scan);
  assert.equal(cpu(ctx).registers[0], kind.equal ? 0x7fffff01 : 0x7fffff00); assert.equal(cpu(ctx).registers[1], kind.source); assert.equal(cpu(ctx).registers[6], 0x7fffffff);
  targetRam(ctx, row); pages(ctx, 'fresh tail without replay: RAM observed after helper-preserving consumers');
  const tailBefore = physical(ctx, current), tailByte = ctx.pages.get(PC)[scan.next - PC]; assert.equal(tailByte, 0x0f);
  upload(ctx, scan.next, Buffer.from([tailByte]), 'same consumed-tail byte'); cancel(ctx, 1);
  run(ctx, current, 0, 'stale fresh tail before cancelled malformed pointers', {}, 4, true); counts.stale_calls++;
  const retainedTail = physical(ctx, current); assert.deepEqual(retainedTail, tailBefore, 'tail allocation immediately after stale before close');
  pure(ctx, () => ctx.api.close(), 'close stale owners');
  for (const unit of [old, current]) {run(ctx, unit, 0, 'closed child before stale and malformed pointers', {}, 5, true); counts.closed_calls++;}
  codeStores.push({context: ctx.ordinal, case: kind.name, changing: kind.changing, equal: kind.equal, address: registers[7], consumed_instruction: 'MOV ESI, imm32', original_immediate: 0x7fffffff,
    mov, target: row, target_arena: targetArena, ram_readback_phase: row.ram_readback_phase, store_status: {value: 11, authority: 'source-inferred; not intercepted'},
    observed_exit: 6, observed_retired: 1, old_identity: identity(ctx, old), current_identity: identity(ctx, current),
    retained_old_after_stale: retainedOld, retained_tail_after_stale: retainedTail, fresh_tail_exit_version: 1,
    replay_discriminator: kind.name === 'equal-same' ? null : {basis: 'hypothetical replay from committed post-target/pre-consumer CPU; no replay executed',
      canonical_esi: 0x7fffffff, canonical_eax: 0x7fffffff, canonical_ecx: kind.source, canonical_memory: kind.memory,
      replayed_mov_esi: kind.memory, replayed_cmp_eax: kind.changing ? 0x80000000 : 0x7fffffff,
      replayed_cmp_memory: kind.changing ? 0x80000000 : 0xdeadbeef, replayed_cmp_flags: kind.changing ? 0xc87 : 0x446}});
}
for (const owner of OWNERS) {normal(owner); special(owner); for (const kind of SMC) smc(owner, kind);}
for (const [name, value] of Object.entries(formulas)) assert.equal(counts[name], value, `observed ${name}`);
assert.equal(counts.main_targets, OWNERS.length * 8 * 2 * FLAGS.length); assert.equal(counts.literal_targets, OWNERS.length * ANCHORS.length * FLAGS.length);
assert.equal(counts.equal_targets, 68); assert.equal(counts.mismatch_targets, 98);
assert.equal(counts.cmpxchgs, targetCount); assert.equal(counts.setz, targetCount); assert.equal(counts.setb, targetCount); assert.equal(counts.jumps, targetCount);
assert.equal(counts.seeds, targetCount, 'one seed per independent scenario, none during repair or tail continuation');
assert.equal(counts.contexts, OWNERS.length * (2 + SMC.length)); assert.equal(counts.modules, OWNERS.length * (2 + SMC.length * 2));
assert.equal(counts.retired_add_prefixes, formulas.repaired_targets); assert.equal(counts.retired_mov_prefixes, formulas.code_targets);
assert.equal(counts.fault_calls, formulas.repaired_targets * 2); assert.equal(counts.canaries, formulas.code_targets);
assert.equal(counts.permission_only_repairs, OWNERS.length * 3); assert.equal(counts.changed_data_repairs, OWNERS.length * faultShapes.length);
assert.equal(counts.restored_map_repairs, OWNERS.length); assert.equal(counts.comparator_flip_repairs, counts.changed_data_repairs);
assert.equal(counts.same_value_code_stores, OWNERS.length * 2); assert.equal(counts.changing_code_stores, OWNERS.length);
assert.equal(counts.stale_calls, formulas.code_targets * 2); assert.equal(counts.closed_calls, OWNERS.length * (2 + SMC.length * 2));
assert.equal(counts.preflight_calls, OWNERS.length * 3); assert.equal(counts.cold_calls, OWNERS.length * 2); assert.equal(counts.malformed_calls, OWNERS.length * 2);
const expectedCalls = targetCount * 4 + counts.fault_calls + counts.retired_mov_prefixes + counts.canaries
  + counts.preflight_calls + counts.cold_calls + counts.malformed_calls + counts.stale_calls + counts.closed_calls;
assert.equal(expectedCalls, 750); assert.equal(counts.generated_calls, expectedCalls); assert.equal(runs.length, expectedCalls);
const retirements = runs.reduce((total, row) => total + row.retired, 0);
assert.equal(retirements, targetCount * 4 + counts.canaries + counts.retired_mov_prefixes + counts.retired_add_prefixes); assert.equal(retirements, 692);
const pageCount = OWNERS.length * (8 + 16 + FLAGS.length * (2 * 3 + 2) + 3 * FLAGS.length * (2 * 4 + 2) + SMC.length * 8);
assert.equal(pageCount, 248); assert.equal(counts.pages, pageCount, '16 normal +184 special including every mapped failure page +48 SMC pages');
assert.equal(counts.diagnostic_read32_calls, pageCount * 1024 + targetCount);
assert.ok(targetRows.every(row => row.ram_readback.value === row.stored_value));
assert.deepEqual(sourceHashes(), testHashes, 'test/helper sources unchanged'); assert.equal(hash(readFileSync(enginePath)), engineSha256, 'engine input unchanged');
assert.deepEqual(productHashes(), productPins, 'five selected product leaves unchanged');
const artifactNames = [...artifacts.map(row => row.path), 'result.json']; assert.equal(new Set(artifactNames).size, artifactNames.length);
const rawArenas = formulas.repaired_targets * 2 + formulas.code_targets;
assert.equal(rawArenas, 38); assert.equal(artifacts.filter(row => row.path.startsWith('arena-')).length, rawArenas);
assert.equal(artifactNames.length, pageCount + rawArenas + counts.modules + 5); assert.equal(artifactNames.length, 307);
const result = {status: 'ok', engine_sha256: engineSha256, engine_bytes: engineBytes.length, tools: {node: process.version, v8: process.versions.v8},
  counts, formulas: {...formulas, targets: targetCount, main_targets: OWNERS.length * 8 * 2 * FLAGS.length, literal_targets: OWNERS.length * ANCHORS.length * FLAGS.length,
    equal_targets: 68, mismatch_targets: 98, fault_calls: formulas.repaired_targets * 2, generated_calls: expectedCalls, retirements,
    pages: pageCount, raw_arenas: rawArenas, diagnostic_read32_calls: pageCount * 1024 + targetCount, artifact_files: 307},
  contexts, modules, targets: targetRows, faults, code_stores: codeStores, pages: pagesRead, generated_runs: runs, host_inputs: hostInputs,
  source, test_sha256: testHashes, product_sha256: productPins, oracle: {anchors: ANCHORS, flag_seeds: FLAGS, registers: REG, smc_cases: SMC, pattern_sha256: hash(pattern), arena_bytes: SIZE,
    banks: Object.fromEntries(Object.entries(banks).map(([name, entry]) => [name, {hex: entry.bytes.toString('hex'), sha256: hash(entry.bytes), specs: entry.specs, scans: entry.scans, instructions: entry.instructions}])),
    arithmetic: 'widened unsigned numeric subtraction modulo2^32; separate signed numeric-range overflow; nibble borrow and full low-byte bitstring parity; old EAX/memory/source/original EA captured before exchange; source EAX follows implicit accumulator on mismatch',
    checkpoint_policy: 'complete live arena compared around every generated run, explicit upload/map/unmap/protect/guard/close and diagnostic Read32; compiler requests/getter groups checked at publication, initialization checked after open/getter group; actual full CPU/exit/helper snapshots saved immediately at targets and faults; first/retry raw arenas saved before every mapped declared page diagnostic, denied data temporarily readable only for diagnostics then restored; SMC RAM physically read after pure-tail consumers to retain first Store4 helper through stale/publication/tail; selected raw arenas and complete current pages saved physically',
    binding_policy: 'actual managed engine read32/store32 or store_resident32 directly imported; no synthetic helpers; checked Store4 mandatory on equality and mismatch; source-inferred rawStore4 status11 is distinct from observed helper success/exit6',
    continuation_policy: 'declared data/permission repairs, observed diagnostic helper updates, cancellation writes and compiler requests; no CPU/helper reseed after a fault or consumed MOV/CMPXCHG; pure current tail has exitv1 and retains committed Store4 helper; replay discriminators are hypothetical pre-consumer values'},
  artifacts, artifact_census: {modules: modules.length, pages: counts.pages, raw_arenas: rawArenas, engine_files: 1, program_banks: 2, primary_excerpts: 1, result_files: 1, total: artifactNames.length, names: artifactNames},
  claim: 'finite prefix-free flat32 memory CMPXCHG32 in actual managed replacement/resident Wasm: all eight source GPRs equal/mismatch and ECX eight literal CMP edges under FLAGS2/cd7; five original-EA source/implicit-EAX/base/index aliases, crossing success and four precise Read4/Write4 fault shapes with retired ADD, unchanged zero-retirement retry, declared permission/data-only repair at same CPU/module with changed-memory equality-to-mismatch reread; full target CPU before live SETZ/SETB/JMP, normal/alias/cross/repair RAM physically read before consumers; unconditional checked writes on both comparator outcomes, including equal unchanged/equal changing/mismatch unchanged truly consumed MOV-immediate stores committing helper/EAX/flags/EIP and one retirement with exit6, then stale4 with immediate whole live old allocation retention, pure fresh tail without MOV/CMPXCHG replay retaining helper and intentionally changing AL, SMC RAM physically read after tail, same-byte tail stale retention and both closed5; complete live4236 arenas and current declared pages with selected physical evidence/provenance; intentional offline immutable primary inputs; no atomicity/LOCK/prefix/byte/word/wider EA-exhaustive ISA/PE/Windows/SDK/browser/performance/fullCI/game claim'};
const resultBytes = Buffer.from(JSON.stringify(result, null, 2)); writeFileSync(join(output, 'result.json'), resultBytes, {flag: 'wx'});
assert.deepEqual(readdirSync(output).sort(), artifactNames.sort(), 'exact physical artifact census');
console.log(JSON.stringify({status: result.status, engine_sha256: engineSha256, result_sha256: hash(resultBytes), counts, output}));
