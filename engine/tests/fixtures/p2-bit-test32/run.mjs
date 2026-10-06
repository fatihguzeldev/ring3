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
const SIZE = 4236, TRANSFER = 140, PC = 0x1000, COLD = PC + 128;
const OWNERS = ['replacement', 'resident'], FLAGS = [2, 0xcd7], VALID_SEEDS = [...FLAGS, 0x42, 0x402];
const IMMEDIATE_EDGES = [0, 1, 31, 32, 33, 64, 128, 255];
const REGISTER_EDGES = [...IMMEDIATE_EDGES, 0xffffffff, 0x8000001f], ALIAS_INPUTS = [0, 1, 2, 3, 4, 0x8000001f];
const REG = [0x12345662, 0x80000021, 0x3456789a, 0x456789ab, 0x56789abc, 0x6789abcd, 0x789abcde, 0x89abcdef];
const KINDS = [{kind: 'bt', opcode: 0xa3, extension: 4}, {kind: 'bts', opcode: 0xab, extension: 5},
  {kind: 'btr', opcode: 0xb3, extension: 6}, {kind: 'btc', opcode: 0xbb, extension: 7}];
const ANCHORS = [
  {kind: 'bt', destination: 0, index: 0, value: 0, carry: 0, flags: 2},
  {kind: 'bt', destination: 0xffffffff, index: 0xffffffff, value: 0xffffffff, carry: 1, flags: 3},
  {kind: 'bts', destination: 0, index: 32, value: 1, carry: 0, flags: 2, independent_zf_df: true},
  {kind: 'bts', destination: 0x80000000, index: 0x8000001f, value: 0x80000000, carry: 1, flags: 3},
  {kind: 'btr', destination: 0x80000000, index: 0xffffffff, value: 0, carry: 1, flags: 3, independent_zf_df: true},
  {kind: 'btr', destination: 0, index: 1, value: 0, carry: 0, flags: 2},
  {kind: 'btc', destination: 0, index: 0x8000001f, value: 0x80000000, carry: 0, flags: 2},
  {kind: 'btc', destination: 0xffffffff, index: 0, value: 0xfffffffe, carry: 1, flags: 3},
];
const ALIAS_VALUES = {bt: [0, 1, 2, 3, 4, 0x8000001f], bts: [1, 3, 6, 11, 20, 0x8000001f],
  btr: [0, 1, 2, 3, 4, 31], btc: [1, 3, 6, 11, 20, 31]};
function bitTest(kind, destination, rawIndex, incoming) {
  assert.ok(Number.isInteger(destination) && destination >= 0 && destination <= 0xffffffff);
  assert.ok(Number.isInteger(rawIndex) && rawIndex >= 0 && rawIndex <= 0xffffffff);
  assert.ok((incoming & 2) !== 0 && (incoming & ~0xcd7) === 0);
  const index = rawIndex % 32, bits = destination.toString(2).padStart(32, '0').split('').map(Number), at = 31 - index;
  const carry = bits[at];
  if (kind === 'bts') bits[at] = 1;
  else if (kind === 'btr') bits[at] = 0;
  else if (kind === 'btc') bits[at] = 1 - bits[at];
  else assert.equal(kind, 'bt');
  return {value: Number.parseInt(bits.join(''), 2), index, carry, flags: 2 + (incoming & 0x400) + (incoming & 0x40) + carry};
}
for (const anchor of ANCHORS) for (const incoming of VALID_SEEDS) {
  const result = bitTest(anchor.kind, anchor.destination, anchor.index, incoming);
  assert.deepEqual([result.value, result.carry, result.flags], [anchor.value, anchor.carry, anchor.flags + (incoming & 0x440)], 'literal old bit/result/preserved FLAGS');
}
for (const {kind} of KINDS) for (const [index, value] of ALIAS_INPUTS.entries()) {
  const result = bitTest(kind, value, value, 2);
  assert.deepEqual([result.value, result.carry], [ALIAS_VALUES[kind][index], Number(index === 5)], 'literal destination/index aliases');
}
function lowByte(parent, value) {const hex = parent.toString(16).padStart(8, '0'); return Number.parseInt(hex.slice(0, 6) + value.toString(16).padStart(2, '0'), 16);}
const words = values => {const bytes = Buffer.alloc(values.length * 4); values.forEach((value, index) => bytes.writeUInt32LE(value >>> 0, index * 4)); return bytes;};
function bank(name, forms, descriptors, packed, instructions, canary = false) {
  const bytes = Buffer.alloc(130, 0xcc), scans = [], specs = []; let offset = 0;
  for (const [index, form] of forms.entries()) {
    const tail = [0x0f, 0x92, 0xc0, 0x0f, 0x94, 0xc2, ...(canary ? [0xbb, 0xef, 0xbe, 0xad, 0xde] : [])];
    const length = form.length + tail.length + 2, entry = PC + offset;
    assert.ok(128 - offset - length >= 0 && 128 - offset - length <= 127);
    bytes.set([...form, ...tail, 0xeb, 128 - offset - length], offset);
    specs.push([entry, length]); scans.push({...descriptors[index], entry, target: entry + (canary ? 5 : 0), next: entry + form.length, canary}); offset += length;
  }
  assert.equal(offset, packed); assert.ok(specs.length <= 8 && instructions <= 64); bytes.set([0x0f, 0x0b], 128);
  assert.deepEqual(readFileSync(join(output, `${name}.x86`)), bytes, 'independent Rust/JS bank bytes');
  return {name, bytes, scans, specs, instructions, packed};
}
const banks = [], normalRegister = new Map(), normalImmediate = new Map(), immediateEdges = new Map();
for (const encoding of KINDS) {
  for (let source = 0; source < 8; source++) {
    const name = `normal-${encoding.kind}-reg-i${source}`, forms = [], scans = [];
    for (let destination = 0; destination < 8; destination++) {
      forms.push([0x0f, encoding.opcode, 0xc0 + source * 8 + destination]);
      scans.push({...encoding, mode: 'reg', source, destination});
    }
    const selected = bank(name, forms, scans, 88, 32); banks.push(selected); normalRegister.set(name, selected);
  }
  const immediate = bank(`normal-${encoding.kind}-imm`, Array.from({length: 8}, (_, destination) => [0x0f, 0xba, 0xc0 + encoding.extension * 8 + destination, 1]),
    Array.from({length: 8}, (_, destination) => ({...encoding, opcode: 0xba, mode: 'imm', source: null, destination, raw: 1})), 96, 32);
  banks.push(immediate); normalImmediate.set(encoding.kind, immediate);
  const edge = bank(`edge-${encoding.kind}-imm`, IMMEDIATE_EDGES.map(raw => [0x0f, 0xba, 0xc0 + encoding.extension * 8 + 6, raw]),
    IMMEDIATE_EDGES.map(raw => ({...encoding, opcode: 0xba, mode: 'imm', source: null, destination: 6, raw})), 96, 32);
  banks.push(edge); immediateEdges.set(encoding.kind, edge);
}
const currencyRegister = bank('currency-reg', [[0xbe, 1, 0, 0, 0, 0x0f, 0xbb, 0xfe]],
  [{kind: 'btc', opcode: 0xbb, extension: 7, mode: 'reg', source: 7, destination: 6}], 21, 6, true);
const currencyImmediate = bank('currency-imm', [[0xbe, 1, 0, 0, 0, 0x0f, 0xba, 0xfe, 0]],
  [{kind: 'btc', opcode: 0xba, extension: 7, mode: 'imm', source: null, destination: 6, raw: 0}], 22, 6, true);
banks.push(currencyRegister, currencyImmediate); assert.equal(banks.length, 42);
const formulas = {register_matrix_targets: OWNERS.length * KINDS.length * 8 * 8 * FLAGS.length,
  immediate_matrix_targets: OWNERS.length * KINDS.length * 8 * FLAGS.length,
  register_edge_targets: OWNERS.length * KINDS.length * 2 * REGISTER_EDGES.length * FLAGS.length,
  immediate_edge_targets: OWNERS.length * KINDS.length * IMMEDIATE_EDGES.length * FLAGS.length,
  alias_anchor_targets: OWNERS.length * KINDS.length * ALIAS_INPUTS.length * FLAGS.length,
  literal_targets: OWNERS.length * ANCHORS.length * FLAGS.length,
  flag_independence_targets: OWNERS.length * ANCHORS.filter(row => row.independent_zf_df).length * 2,
  currency_targets: OWNERS.length * 3};
const regularTargets = Object.entries(formulas).filter(([name]) => name !== 'currency_targets').reduce((sum, [, value]) => sum + value, 0);
const targetCount = regularTargets + formulas.currency_targets, regularContexts = OWNERS.length * (normalRegister.size + normalImmediate.size + immediateEdges.size);
const pauseCases = OWNERS.length * KINDS.length * 2 * FLAGS.length * 2, malformedControls = OWNERS.length;
const plannedContexts = regularContexts + formulas.currency_targets, plannedModules = regularContexts + formulas.currency_targets * 2;
const plannedCalls = regularTargets * 3 + pauseCases * 2 + malformedControls + regularContexts + formulas.currency_targets * 7;
const plannedRetired = regularTargets * 4 + formulas.currency_targets * 6, plannedRaw = targetCount + formulas.currency_targets * 4;
const plannedFiles = 1 + banks.length + plannedModules + 1 + 1 + 1;
assert.deepEqual([regularTargets, targetCount, regularContexts, pauseCases, plannedContexts, plannedModules, plannedCalls, plannedRetired, plannedRaw, plannedFiles],
  [1736, 1742, 80, 64, 86, 92, 5460, 6980, 1766, 138]);
const counts = {contexts: 0, modules: 0, seeds: 0, targets: 0, ...Object.fromEntries(Object.keys(formulas).map(name => [name, 0])),
  bt_targets: 0, bts_targets: 0, btr_targets: 0, btc_targets: 0, reg_targets: 0, imm_targets: 0, self_targets: 0,
  zero_index_targets: 0, ecx_index_targets: 0, ecx_destination_targets: 0, premature_index_discriminators: 0, generated_calls: 0, retired: 0, setb: 0, setz: 0, jumps: 0,
  mov_prefixes: 0, canaries: 0, cancel_calls: 0, cancel_budget_zero_calls: 0, malformed_calls: 0, zero_budget_calls: 0,
  stale_calls: 0, closed_calls: 0, arena_checks: 0, raw_arenas: 0};
const contexts = [], modules = [], targets = [], runs = [], mutations = [], hostInputs = [], artifacts = [], rawFrames = [], rawRecords = [];
const pairCensus = new Map(), immediateCensus = new Map(), rawIndexCensus = new Map();
function artifact(path, bytes, write = true) {
  assert.ok(!artifacts.some(row => row.path === path)); if (write) writeFileSync(join(output, path), bytes, {flag: 'wx'});
  const row = {path, bytes: bytes.length, sha256: hash(bytes)}; artifacts.push(row); return row;
}
artifact('engine.wasm', engineBytes); for (const selected of banks) artifact(`${selected.name}.x86`, selected.bytes, false);
const source = {provenance: 'existing physically pinned local Intel primary; no fresh download or latest-edition claim',
  prerequisite_policy: 'ignored immutable local PDF/full text required offline before guest execution; no bootstrap or clean-checkout/fullCI claim',
  pdf_path: 'target/p2-memory-binary-spec/253666-093-sdm-vol-2a.pdf', pdf_bytes: 3404694, pdf_sha256: '87c5acb6f27e24d9d364841a0d2346c91a8482e36f403409954e2d2a8c817bac',
  full_text_path: 'target/p2-memory-binary-spec/253666-093-sdm-vol-2a.txt', full_text_bytes: 1557075, full_text_sha256: 'fa7f27a4b5f830e007fa6a967e53436aa7051fe99d8d56303efcf4148b0a5eaf',
  chapter_lines: [11680, 11987], chapter_bytes: 18703, chapter_sha256: 'e7c11638ed3731a852ddc2ed23b40eb9c9754b560e565d5dadf9d3313fa76bfc',
  chapters: [{name: 'BT', lines: [11680, 11761], bytes: 4998, sha256: '4066df1f7cd3988492a9b7cc4a09750bf71b87d4d2bb76140a369b5cc504aed2'},
    {name: 'BTC', lines: [11763, 11837], bytes: 4701, sha256: '9a412ddf5823e1bc3bacb01375a4175e0c88ed03b9c147626186182fdc146887'},
    {name: 'BTR', lines: [11839, 11912], bytes: 4499, sha256: 'c999dbc38ad19b7b136bc427158e0efc571751588b1d8b5477439df9ada02c21'},
    {name: 'BTS', lines: [11914, 11987], bytes: 4466, sha256: '5e8a1a7e030c870f8c2a27e4ea99cdb3a6ceded784a26cad90bd588e2576aca1'}],
  chapter_separator_policy: 'complete four chapters and internal PDF PAGE separators; excludes successor chapter'};
for (const kind of ['pdf', 'full_text']) {const bytes = readFileSync(join(root, source[`${kind}_path`])); assert.equal(bytes.length, source[`${kind}_bytes`]); assert.equal(hash(bytes), source[`${kind}_sha256`]);}
const fullTextLines = readFileSync(join(root, source.full_text_path), 'utf8').split('\n');
const chapterBytes = (start, end) => Buffer.from(fullTextLines.slice(start - 1, end).join('\n') + '\n');
for (const chapter of source.chapters) {const bytes = chapterBytes(...chapter.lines); assert.equal(bytes.length, chapter.bytes); assert.equal(hash(bytes), chapter.sha256);}
const chapter = chapterBytes(...source.chapter_lines); assert.equal(chapter.length, source.chapter_bytes); assert.equal(hash(chapter), source.chapter_sha256);
source.saved_chapter = artifact('intel-bit-test.txt', chapter);
const sourcePaths = ['engine/tests/cpu_bit_test32.rs', 'engine/tests/cpu_bit_test32_wasm.rs', 'engine/tests/fixtures/p2-bit-test32/run.mjs',
  'engine/tests/fixtures/support/engine.mjs', 'engine/src/cpu/x86/ir.rs', 'engine/src/cpu/x86/decode/integer.rs', 'engine/src/cpu/dbt/region.rs', 'engine/src/cpu/dbt/wasm/integer.rs'];
assert.equal(new Set(sourcePaths).size, 8);
const sourceIdentities = () => Object.fromEntries(sourcePaths.map(path => {const bytes = readFileSync(join(root, path)); return [path, {bytes: bytes.length, sha256: hash(bytes)}];}));
const sourcePins = sourceIdentities();

function profile(bytes, owner) {
  let at = 8; assert.deepEqual([...bytes.subarray(0, 8)], [0, 97, 115, 109, 1, 0, 0, 0]);
  const uleb = () => {let value = 0, scale = 1; for (let i = 0; i < 5; i++) {assert.ok(at < bytes.length); const byte = bytes[at++]; value += (byte & 127) * scale;
    if (!(byte & 128)) {assert.ok(value <= 0xffffffff); return value;} scale *= 128;} assert.fail('invalid bounded unsigned LEB');};
  const text = () => {const length = uleb(); assert.ok(at + length <= bytes.length); const value = bytes.subarray(at, at + length).toString('utf8'); at += length; return value;};
  const sections = new Map();
  while (at < bytes.length) {const id = bytes[at++], length = uleb(), start = at; assert.ok(start + length <= bytes.length); assert.ok(!sections.has(id)); sections.set(id, [start, start + length]); at += length;}
  assert.deepEqual([...sections.keys()], [1, 2, 3, 7, 10]);
  const section = id => {at = sections.get(id)[0];}; const ended = id => assert.equal(at, sections.get(id)[1]);
  section(1); const types = []; for (let i = 0, n = uleb(); i < n; i++) {assert.equal(bytes[at++], 0x60); const parameters = [];
    for (let j = 0, n = uleb(); j < n; j++) parameters.push(bytes[at++]); const results = []; for (let j = 0, n = uleb(); j < n; j++) results.push(bytes[at++]); types.push({parameters, results});} ended(1);
  assert.deepEqual(types, [{parameters: Array(4).fill(0x7f), results: [0x7f]}, {parameters: Array(owner === 'replacement' ? 6 : 7).fill(0x7f), results: [0x7f]}]);
  section(2); assert.equal(uleb(), 2); assert.equal(text(), 'env'); assert.equal(text(), 'memory'); assert.equal(bytes[at++], 2); assert.equal(uleb(), 0); assert.equal(uleb(), 1);
  assert.equal(text(), 'ring3'); assert.equal(text(), owner === 'replacement' ? 'guard' : 'guard_resident'); assert.equal(bytes[at++], 0); assert.equal(uleb(), 1); ended(2);
  section(3); assert.equal(uleb(), 1); assert.equal(uleb(), 0); ended(3);
  section(7); assert.equal(uleb(), 1); assert.equal(text(), 'run'); assert.equal(bytes[at++], 0); assert.equal(uleb(), 1); ended(7);
  section(10); assert.equal(uleb(), 1); const bodyLength = uleb(), bodyEnd = at + bodyLength; assert.equal(bodyEnd, sections.get(10)[1]);
  const locals = []; for (let i = 0, n = uleb(); i < n; i++) locals.push([uleb(), bytes[at++]]); assert.deepEqual(locals, [[16, 0x7f], [1, 0x7e]]);
  assert.ok(at < bodyEnd); assert.equal(bytes[bodyEnd - 1], 0x0b); return {types, locals, body_bytes: bodyLength, sections: [...sections.keys()]};
}
function record(magic, version, size, fields) {
  const bytes = Buffer.alloc(size); bytes.write(magic); bytes.writeUInt16LE(version, 4); bytes.writeUInt16LE(1, 6); bytes.writeUInt32LE(size, 8);
  fields.forEach((value, index) => bytes.writeUInt32LE(value >>> 0, 16 + index * 4)); return bytes;
}
const state = (registers, pc, flags) => record('R3ST', 1, 56, [...registers, pc, flags]);
const exit = (reason, retired) => record('R3EX', 1, 40, [reason, retired, 0, 0, 0, 0]);
const initialHelper = () => record('R3MH', 1, 40, [0, 0, 0, 0, 0, 0]);
function refresh(ctx) {ctx.bytes = new Uint8Array(ctx.memory.buffer); ctx.view = new DataView(ctx.memory.buffer); return ctx;}
function arena(ctx) {return Buffer.from(refresh(ctx).bytes.subarray(ctx.base, ctx.base + SIZE));}
function check(ctx, label) {assert.deepEqual(arena(ctx), ctx.expected, `${ctx.owner}/${ctx.ordinal}/${label}: complete live arena`); counts.arena_checks++;}
function snapshot(ctx) {const bytes = arena(ctx); return {state_hex: bytes.subarray(0, 56).toString('hex'), exit_hex: bytes.subarray(56, 96).toString('hex'),
  helper_hex: bytes.subarray(100, 140).toString('hex'), whole_arena_sha256: hash(bytes)};}
function cpu(ctx) {return {registers: Array.from({length: 8}, (_, i) => ctx.expected.readUInt32LE(16 + i * 4)), pc: ctx.expected.readUInt32LE(48), flags: ctx.expected.readUInt32LE(52)};}
function saveArena(ctx, label, unit) {check(ctx, label); const bytes = arena(ctx), row = {context: ctx.ordinal, module: unit.file, label,
  path: 'arena-snapshots.bin', offset: rawFrames.length * SIZE, bytes: SIZE, sha256: hash(bytes)}; rawFrames.push(bytes); rawRecords.push(row); counts.raw_arenas++; return row;}
function pure(ctx, action, label, status = 0) {check(ctx, `before ${label}`); assert.equal(action(), status, label); check(ctx, label);}
function request(ctx, bytes) {assert.ok(bytes.length <= 4096); refresh(ctx).bytes.set(bytes, ctx.base + TRANSFER); ctx.expected.set(bytes, TRANSFER); check(ctx, 'explicit host request');}
function upload(ctx, address, bytes, purpose) {request(ctx, bytes); pure(ctx, () => ctx.api.upload(address, bytes.length), purpose);
  assert.ok(address >= PC && address + bytes.length <= PC + ctx.code.length); ctx.code.set(bytes, address - PC);
  hostInputs.push({context: ctx.ordinal, purpose, address, bytes: bytes.length, hex: bytes.toString('hex'), sha256: hash(bytes)});}
function fresh(owner, selected, label, writable = false) {
  const instance = new WebAssembly.Instance(engineModule, {}), api = {}, ordinal = ++counts.contexts;
  const arities = {open: 3, close: 0, arena_ptr: 0, map: 3, protect: 3, upload: 2, compile_entries: 2, compile_resident: 1,
    generation: 0, module_ptr: 0, module_len: 0, guard: 6, guard_resident: 7};
  for (const [name, arity] of Object.entries(arities)) {api[name] = instance.exports[`ring3_abi_v1_${name}`]; assert.equal(typeof api[name], 'function', name); assert.equal(api[name].length, arity, name);}
  const ctx = {owner, ordinal, selected, label, api, memory: instance.exports.memory, low: ordinal, high: 0x42543332,
    expected: Buffer.alloc(SIZE), code: Buffer.alloc(selected.bytes.length), units: []};
  assert.ok(ctx.memory instanceof WebAssembly.Memory); assert.equal(api.open(1, ctx.low, ctx.high), 0); ctx.base = api.arena_ptr() >>> 0;
  refresh(ctx); assert.ok(ctx.base > 0 && ctx.base + SIZE <= ctx.bytes.length);
  ctx.expected.set(state(Array(8).fill(0), 0, 2)); ctx.expected.set(exit(1, 0), 56); ctx.expected.set(initialHelper(), 100); check(ctx, 'independently initialized arena');
  pure(ctx, () => api.map(PC, 1, 7), 'one code page'); upload(ctx, PC, selected.bytes, 'authored bank');
  if (!writable) pure(ctx, () => api.protect(PC, 1, 5), 'normal RX code');
  contexts.push({context: ordinal, owner, label, bank: selected.name, key: [ctx.low, ctx.high], base: ctx.base, page_limit: 1, code_permissions: writable ? 7 : 5}); return ctx;
}
const identity = (ctx, unit) => ctx.owner === 'replacement' ? {generation: unit.generation} : {low: unit.low, high: unit.high};
function physical(ctx, unit) {refresh(ctx); assert.ok(unit.pointer + unit.length <= ctx.bytes.length); const bytes = Buffer.from(ctx.bytes.subarray(unit.pointer, unit.pointer + unit.length));
  assert.deepEqual(bytes, unit.bytes, 'whole physical live module allocation retained'); return {pointer: unit.pointer, bytes: unit.length, sha256: hash(bytes)};}
function compile(ctx, phase = 'initial', tail) {
  const specs = tail ? [[tail.next, 13]] : ctx.selected.specs; request(ctx, words(ctx.owner === 'replacement' ? specs.map(([entry]) => entry) : specs.flat())); let binding;
  if (ctx.owner === 'replacement') {pure(ctx, () => ctx.api.compile_entries(specs.length, 0), 'entry compiler'); binding = {generation: ctx.api.generation(), pointer: ctx.api.module_ptr() >>> 0, length: ctx.api.module_len() >>> 0}; assert.ok(binding.generation > 0);}
  else {check(ctx, 'before resident compiler'); assert.equal(ctx.api.compile_resident(specs.length), 0); refresh(ctx);
    binding = Object.fromEntries(['low', 'high', 'pointer', 'length'].map((name, i) => [name, ctx.view.getUint32(ctx.base + TRANSFER + 8 + i * 4, true)]));
    assert.ok(binding.low !== 0 || binding.high !== 0); ctx.expected.set(words([1, 24, binding.low, binding.high, binding.pointer, binding.length]), TRANSFER); check(ctx, 'resident receipt only');
    assert.equal(ctx.api.generation(), 0); assert.equal(ctx.api.module_ptr(), 0); assert.equal(ctx.api.module_len(), 0);}
  check(ctx, 'compiler publication/getter group'); refresh(ctx); assert.ok(binding.pointer > 0 && binding.length > 8 && binding.length <= 65536 && binding.pointer + binding.length <= ctx.bytes.length);
  const bytes = Buffer.from(ctx.bytes.subarray(binding.pointer, binding.pointer + binding.length)); assert.ok(WebAssembly.validate(bytes));
  const module = new WebAssembly.Module(bytes), guard = ctx.owner === 'replacement' ? 'guard' : 'guard_resident';
  const imports = [{module: 'env', name: 'memory', kind: 'memory'}, {module: 'ring3', name: guard, kind: 'function'}];
  assert.deepEqual(WebAssembly.Module.imports(module), imports); assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  const abi = profile(bytes, ctx.owner), child = new WebAssembly.Instance(module, {env: {memory: ctx.memory}, ring3: {[guard]: ctx.api[guard]}}); assert.equal(child.exports.run.length, 4);
  const file = `${ctx.owner}-${ctx.ordinal}-${phase}.wasm`, saved = artifact(file, bytes), unit = {...binding, bytes, file, run: child.exports.run}; ctx.units.push(unit); counts.modules++;
  modules.push({context: ctx.ordinal, owner: ctx.owner, bank: ctx.selected.name, phase, specs, instructions: tail ? 4 : ctx.selected.instructions, ...binding,
    identity: identity(ctx, unit), exit_version: 1, imports, exports: [{name: 'run', kind: 'function'}], run_arity: 4, profile: abi, artifact: saved,
    source_spans: specs.map(([entry, length]) => {const bytes = ctx.code.subarray(entry - PC, entry - PC + length); return {entry, bytes: length, hex: bytes.toString('hex'), sha256: hash(bytes), bank_path: `${ctx.selected.name}.x86`, bank_offset: entry - PC};})});
  return unit;
}
function seed(ctx, registers, pc, flags) {assert.ok(VALID_SEEDS.includes(flags)); ctx.expected.set(state(registers, pc, flags)); ctx.expected.set(exit(3, 0), 56); ctx.expected.writeUInt32LE(0, 96); ctx.expected.fill(0x5a, 100, 140);
  refresh(ctx).bytes.set(ctx.expected.subarray(0, 140), ctx.base); check(ctx, 'one independent initial case seed'); counts.seeds++;}
function cancel(ctx, value) {ctx.expected.writeUInt32LE(value, 96); refresh(ctx).view.setUint32(ctx.base + 96, value, true); check(ctx, 'explicit cancellation field');}
function run(ctx, unit, budget, label, outcome = {}, status = 0, malformed = false) {
  check(ctx, `before ${label}`); const before = snapshot(ctx), current = cpu(ctx), {registers = current.registers, pc = current.pc, flags = current.flags, reason = 1, retired = 0} = outcome;
  if (status === 0) {ctx.expected.set(state(registers, pc, flags)); ctx.expected.set(exit(reason, retired), 56);}
  assert.equal(unit.run(malformed ? 1 : ctx.base, ctx.base + 56, budget, ctx.base + 96), status, label); check(ctx, label);
  counts.generated_calls++; counts.retired += status ? 0 : retired;
  const row = {ordinal: counts.generated_calls, context: ctx.ordinal, module: unit.file, label, budget, status, malformed_state_pointer: malformed ? 1 : null,
    reason: status ? null : reason, retired: status ? 0 : retired, before, after: snapshot(ctx)}; runs.push(row); return row;
}
function target(ctx, unit, scan, group, label) {
  const original = cpu(ctx), raw = scan.mode === 'imm' ? scan.raw : original.registers[scan.source];
  assert.equal(original.pc, scan.target); const oldDestination = original.registers[scan.destination];
  const result = bitTest(scan.kind, oldDestination, raw, original.flags), registers = [...original.registers]; registers[scan.destination] = result.value;
  const observed = run(ctx, unit, 1, label, {registers, pc: scan.next, flags: result.flags, retired: 1});
  const rawArena = saveArena(ctx, `${group}: immediate target before AL/DL consumers`, unit);
  counts.targets++; counts[group]++; counts[`${scan.kind}_targets`]++; counts[`${scan.mode}_targets`]++;
  counts.zero_index_targets += Number(result.index === 0); counts.self_targets += Number(scan.destination === scan.source);
  counts.ecx_index_targets += Number(scan.source === 1); counts.ecx_destination_targets += Number(scan.destination === 1);
  rawIndexCensus.set(raw, (rawIndexCensus.get(raw) ?? 0) + 1);
  const row = {context: ctx.ordinal, module: unit.file, bank: ctx.selected.name, group, label, kind: scan.kind, mode: scan.mode, opcode: scan.opcode,
    extension: scan.extension, source: scan.source, destination: scan.destination, target_pc: scan.target, next_pc: scan.next, raw_index: raw, masked_index: result.index,
    old_destination: oldDestination, seed_flags: original.flags, result: result.value, carry: result.carry, flags: result.flags,
    registers_before: original.registers, registers_after: registers, snapshot_phase: 'immediately after target, before SETB AL / SETZ DL', observed, raw_arena: rawArena};
  if (group === 'register_matrix_targets' && scan.source === 0 && scan.destination === 0 && ['bts', 'btc'].includes(scan.kind)) {
    const premature = bitTest('bt', result.value, result.value, result.flags);
    assert.deepEqual([raw, result.index, result.value, result.carry, premature.index, premature.carry], [0x12345662, 2, 0x12345666, 0, 6, 1], 'premature destination/index publication changes CF');
    row.premature_index_publication = {basis: 'hypothetical CF read from newly published parent and its new modulo32 index; no guest replay', index: premature.index, carry: premature.carry};
    counts.premature_index_discriminators++;
  }
  targets.push(row); return row;
}
function consumers(ctx, unit, scan) {
  const current = cpu(ctx), registers = [...current.registers]; registers[0] = lowByte(registers[0], current.flags & 1); registers[2] = lowByte(registers[2], Number(Boolean(current.flags & 0x40)));
  run(ctx, unit, 2, 'SETB AL / SETZ DL read live CF/ZF without reseed', {registers, pc: scan.next + 6, flags: current.flags, retired: 2}); counts.setb++; counts.setz++;
  run(ctx, unit, 2, 'terminal JMP reaches unowned cold lookup with spare budget', {registers, pc: COLD, flags: current.flags, retired: 1, reason: 3}); counts.jumps++;
}
function regularCase(ctx, unit, scan, registers, flags, group, label, pause = false, malformedControl = false) {
  seed(ctx, registers, scan.entry, flags); const row = target(ctx, unit, scan, group, label);
  if (pause) {
    cancel(ctx, 1);
    if (malformedControl) {run(ctx, unit, 0, 'valid child malformed state precedes cancellation and zero budget', {}, 1, true); counts.malformed_calls++;}
    run(ctx, unit, 0, 'cancellation precedes zero budget after target', {reason: 2}); counts.cancel_calls++; counts.cancel_budget_zero_calls++;
    cancel(ctx, 0); run(ctx, unit, 0, 'zero budget alone after target before consumers'); counts.zero_budget_calls++;
  }
  consumers(ctx, unit, scan); return row;
}
function close(ctx, unit) {pure(ctx, () => ctx.api.close(), 'close completed context'); run(ctx, unit, 1, 'retained closed child before malformed state', {}, 5, true); counts.closed_calls++;}
for (const owner of OWNERS) {
  for (const selected of normalRegister.values()) {
    const ctx = fresh(owner, selected, 'complete register index/destination pair row'), unit = compile(ctx);
    const sourceRegister = selected.scans[0].source, kind = selected.scans[0].kind;
    for (const scan of selected.scans) for (const flags of FLAGS) {
      regularCase(ctx, unit, scan, [...REG], flags, 'register_matrix_targets', 'all register pairs capture old destination/index');
      const key = `${owner}/${kind}/${flags}/${scan.source}/${scan.destination}`; assert.ok(!pairCensus.has(key)); pairCensus.set(key, 1);
    }
    if ([6, 7].includes(sourceRegister)) for (const raw of REGISTER_EDGES) for (const flags of FLAGS) {
      const registers = [...REG]; registers[6] = sourceRegister === 6 ? raw : 0x80000003; registers[sourceRegister] = raw;
      const row = regularCase(ctx, unit, selected.scans[6], registers, flags, 'register_edge_targets', 'full unsigned register index masked independently', sourceRegister === 7 && [0, 33].includes(raw));
      assert.equal(row.raw_index, raw);
    }
    if (sourceRegister === 6) for (const [index, raw] of ALIAS_INPUTS.entries()) for (const flags of FLAGS) {
      const registers = [...REG]; registers[6] = raw;
      const row = regularCase(ctx, unit, selected.scans[6], registers, flags, 'alias_anchor_targets', 'literal old destination/index self alias');
      assert.deepEqual([row.result, row.carry, row.flags], [ALIAS_VALUES[kind][index], Number(index === 5), 2 + (flags & 0x440) + Number(index === 5)]);
      row.alias_anchor = {input: raw, value: ALIAS_VALUES[kind][index], carry: Number(index === 5)};
    }
    if (sourceRegister === 7) for (const anchor of ANCHORS.filter(row => row.kind === kind)) {
      for (const flags of [...FLAGS, ...(anchor.independent_zf_df ? [0x42, 0x402] : [])]) {
        const registers = [...REG]; registers[6] = anchor.destination; registers[7] = anchor.index;
        const row = regularCase(ctx, unit, selected.scans[6], registers, flags, FLAGS.includes(flags) ? 'literal_targets' : 'flag_independence_targets', 'literal old bit and independent preserved ZF/DF');
        assert.deepEqual([row.result, row.carry, row.flags], [anchor.value, anchor.carry, anchor.flags + (flags & 0x440)]); row.literal_anchor = anchor;
      }
    }
    close(ctx, unit);
  }
  for (const selected of normalImmediate.values()) {
    const ctx = fresh(owner, selected, 'all immediate destination registers'), unit = compile(ctx);
    for (const scan of selected.scans) for (const flags of FLAGS) {
      regularCase(ctx, unit, scan, [...REG], flags, 'immediate_matrix_targets', 'all immediate destinations use literal raw index1');
      const key = `${owner}/${scan.kind}/${flags}/${scan.destination}`; assert.ok(!immediateCensus.has(key)); immediateCensus.set(key, 1);
    }
    close(ctx, unit);
  }
  for (const selected of immediateEdges.values()) {
    const ctx = fresh(owner, selected, 'finite unsigned immediate index edges'), unit = compile(ctx);
    for (const scan of selected.scans) for (const flags of FLAGS) {
      const registers = [...REG]; registers[6] = 0x80000003;
      const malformedControl = scan.kind === 'bt' && scan.raw === 0 && flags === 2;
      const row = regularCase(ctx, unit, scan, registers, flags, 'immediate_edge_targets', 'immediate index0 still performs a bit operation', [0, 33].includes(scan.raw), malformedControl);
      assert.equal(row.raw_index, scan.raw);
    }
    close(ctx, unit);
  }
}
const mutationCases = [{name: 'consumed-opcode', bank: 'currency-reg', offset: 6, original: 0xbb, changed: 0xab, replay_kind: 'bts', replay_index: 0},
  {name: 'consumed-ModRM', bank: 'currency-reg', offset: 7, original: 0xfe, changed: 0xde, replay_kind: 'btc', replay_index: 1},
  {name: 'consumed-immediate', bank: 'currency-imm', offset: 8, original: 0, changed: 1, replay_kind: 'btc', replay_index: 1}];
for (const owner of OWNERS) for (const mutation of mutationCases) {
  const selected = mutation.bank === 'currency-reg' ? currencyRegister : currencyImmediate;
  const ctx = fresh(owner, selected, mutation.name, true), old = compile(ctx), scan = selected.scans[0], registers = [...REG];
  registers[6] = 0xded00d00; registers[7] = 0; registers[3] = 1; seed(ctx, registers, scan.entry, 0xcd7); registers[6] = 1;
  const mov = run(ctx, old, 1, 'consumed MOV prefix retires before bit target', {registers, pc: scan.target, flags: 0xcd7, retired: 1}); counts.mov_prefixes++;
  const movArena = saveArena(ctx, 'currency MOV prefix', old), beforeOld = physical(ctx, old);
  const row = target(ctx, old, scan, 'currency_targets', 'BTC target commits once before consumed-byte mutation');
  assert.deepEqual([row.result, row.carry, row.flags], [0, 1, 0x443]); assert.equal(ctx.code[mutation.offset], mutation.original);
  upload(ctx, PC + mutation.offset, Buffer.from([mutation.changed]), mutation.name); cancel(ctx, 1);
  const staleOld = run(ctx, old, 0, 'stale old child before malformed state cancellation and zero budget', {}, 4, true); counts.stale_calls++;
  const afterOld = physical(ctx, old); assert.deepEqual(afterOld, beforeOld, 'old full allocation immediately after stale before publication');
  const oldStaleArena = saveArena(ctx, 'currency old stale', old); cancel(ctx, 0); const committed = snapshot(ctx), current = compile(ctx, 'current-tail', scan);
  assert.notDeepEqual(identity(ctx, current), identity(ctx, old)); const published = snapshot(ctx);
  for (const field of ['state_hex', 'exit_hex', 'helper_hex']) assert.equal(published[field], committed[field], `fresh tail publication preserves ${field}`);
  const currentCpu = cpu(ctx), after = [...currentCpu.registers]; after[0] = lowByte(after[0], 1); after[2] = lowByte(after[2], 1); after[3] = 0xdeadbeef;
  run(ctx, current, 5, 'pure current tail reads live CF/ZF and reaches cold lookup without replay', {registers: after, pc: COLD, flags: 0x443, reason: 3, retired: 4}); counts.setb++; counts.setz++; counts.jumps++; counts.canaries++;
  assert.deepEqual([cpu(ctx).registers[6], cpu(ctx).registers[0], cpu(ctx).registers[2], cpu(ctx).registers[3], cpu(ctx).flags], [0, 0x12345601, 0x34567801, 0xdeadbeef, 0x443]);
  assert.equal(snapshot(ctx).helper_hex, Buffer.alloc(40, 0x5a).toString('hex')); const tailArena = saveArena(ctx, 'currency completed current tail', current);
  const hypotheticalTarget = bitTest(mutation.replay_kind, row.result, mutation.replay_index, row.flags);
  const hypotheticalHead = bitTest(mutation.replay_kind, 1, mutation.replay_index, 0xcd7);
  assert.notEqual(hypotheticalTarget.value, row.result, 'hypothetical changed target replay changes retained ESI'); assert.notEqual(hypotheticalHead.value, row.result, 'hypothetical changed whole-head replay changes retained ESI');
  const beforeTail = physical(ctx, current); assert.equal(ctx.code[scan.next - PC], 0x0f); upload(ctx, scan.next, Buffer.from([0x0f]), 'same consumed-tail opcode'); cancel(ctx, 1);
  const staleTail = run(ctx, current, 0, 'stale current tail before malformed state cancellation and zero budget', {}, 4, true); counts.stale_calls++;
  const afterTail = physical(ctx, current); assert.deepEqual(afterTail, beforeTail, 'fresh tail full allocation immediately after stale before close');
  const tailStaleArena = saveArena(ctx, 'currency current tail stale', current); pure(ctx, () => ctx.api.close(), 'close both stale owners retaining pending cancellation');
  for (const unit of [old, current]) {run(ctx, unit, 0, 'closed child before stale malformed state pending cancellation and zero budget', {}, 5, true); counts.closed_calls++;}
  mutations.push({context: ctx.ordinal, case: mutation.name, offset: mutation.offset, address: PC + mutation.offset, original: mutation.original, changed: mutation.changed,
    currency_authority: 'successful host upload to a truly consumed byte; raw code memory not physically reread', mov, target: row, old_identity: identity(ctx, old), fresh_identity: identity(ctx, current),
    mov_arena: movArena, stale_old: staleOld, old_stale_arena: oldStaleArena, retained_old_after_stale: afterOld, completed_tail_arena: tailArena,
    stale_tail: staleTail, tail_stale_arena: tailStaleArena, retained_tail_after_stale: afterTail, pure_tail_entry: scan.next, pure_tail_bytes: 13,
    no_replay: {basis: 'hypothetical changed target/head re-execution from explicit operands; no replay executed', retained_esi: row.result, target_replay: hypotheticalTarget, whole_head_replay: hypotheticalHead}});
}
assert.equal(pairCensus.size, formulas.register_matrix_targets); for (const owner of OWNERS) for (const {kind} of KINDS) for (const flags of FLAGS) for (let source = 0; source < 8; source++) for (let destination = 0; destination < 8; destination++)
  assert.equal(pairCensus.get(`${owner}/${kind}/${flags}/${source}/${destination}`), 1, 'exact register pair roster');
assert.equal(immediateCensus.size, formulas.immediate_matrix_targets); for (const owner of OWNERS) for (const {kind} of KINDS) for (const flags of FLAGS) for (let destination = 0; destination < 8; destination++)
  assert.equal(immediateCensus.get(`${owner}/${kind}/${flags}/${destination}`), 1, 'exact immediate destination roster');
for (const [group, planned] of Object.entries(formulas)) assert.equal(counts[group], planned, group);
assert.deepEqual([counts.targets, counts.contexts, counts.modules, counts.seeds, counts.generated_calls, counts.retired, counts.raw_arenas], [targetCount, plannedContexts, plannedModules, targetCount, plannedCalls, plannedRetired, plannedRaw]);
assert.deepEqual([counts.bt_targets, counts.bts_targets, counts.btr_targets, counts.btc_targets, counts.reg_targets, counts.imm_targets, counts.self_targets], [432, 436, 436, 438, 1484, 258, 384]);
assert.deepEqual([counts.ecx_index_targets, counts.ecx_destination_targets], [128, 144]);
assert.equal(counts.premature_index_discriminators, 8);
assert.deepEqual([counts.setb, counts.setz, counts.jumps, counts.mov_prefixes, counts.canaries, counts.cancel_calls, counts.cancel_budget_zero_calls, counts.malformed_calls, counts.zero_budget_calls, counts.stale_calls, counts.closed_calls],
  [targetCount, targetCount, targetCount, 6, 6, pauseCases, pauseCases, malformedControls, pauseCases, 12, plannedModules]);
assert.equal(counts.arena_checks, 15826, 'prospective complete arena checkpoint schedule');
assert.equal(hostInputs.length, 98); assert.equal(modules.reduce((sum, row) => sum + row.source_spans.length, 0), 652);
const savedArenas = artifact('arena-snapshots.bin', Buffer.concat(rawFrames)); assert.equal(savedArenas.bytes, plannedRaw * SIZE);
assert.deepEqual(sourceIdentities(), sourcePins, 'source bytes unchanged'); assert.equal(hash(readFileSync(enginePath)), engineSha256, 'actual engine input unchanged');
const result = {status: 'ok', profile: 'finite prefix-free register BT/BTS/BTR/BTC32 register+imm8, direct bound replacement/resident execution',
  command: [process.execPath, process.argv[1], enginePath, output, root], environment: {node: process.version, v8: process.versions.v8, platform: process.platform, arch: process.arch},
  engine_sha256: engineSha256, engine_bytes: engineBytes.length, caller_engine_sha256: process.env.RING3_ENGINE_SHA256 ?? null, source,
  selected_source_identities: sourcePins, identity_census: {selected_sources: 8, engine: 1, total: 9},
  oracle: {authority: 'independent MSB-first binary string selected-bit replacement with literal anchors; no emitted Wasm shift/mask formula or engine output input',
    anchors: ANCHORS, alias_inputs: ALIAS_INPUTS, alias_values: ALIAS_VALUES, register_sentinels: REG, flag_seeds: FLAGS, independent_zf_df_seeds: [0x42, 0x402],
    immediate_edges: IMMEDIATE_EDGES, register_edges: REGISTER_EDGES, accepted_flags: '0xcd7 with mandatory0x2',
    undefined_flags: 'OF/SF/AF/PF deliberately0; ZF/DF/mandatory0x2 preserved and CF replaced from old bit; undefined flag zeros are Ring3 policy, not hardware predictions', mutation_cases: mutationCases,
    checkpoint_policy: 'complete live arena compared around every generated run and explicit upload/map/protect/close; compile request/receipt/getter groups checked at publication and initialization after open/getter group; every first-target frame physically saved before AL/DL consumers; selected currency checkpoints physically saved; immediate complete live old/current module allocation comparisons after stale before other operations',
    zero_index_policy: 'masked index0 still performs the selected bit operation and retires once; it is not a zero-count no-op',
    immediate_identity_limit: 'raw unsigned imm8 retained in native typed decode; signed and unsigned interpretations agree after modulo32, so execution cannot distinguish prior sign extension',
    alias_policy: 'all64 regpairs plus finite self inputs; self EAX12345662 Set/Complement→12345666 explicitly discriminates premature newparent/newindex CF1 from oldCF0 in8rows; highbit self anchor discriminates old CF before clear/complement; simple self0/1/2/3/4 may coincide under some wrong CF algorithms, so no claim every alias seed discriminates every premature-publication bug',
    seeding_policy: 'one explicit State/Exit/opaque-helper seed per independent case in a reused bank context; no positive CPU/index/FLAGS/EIP/helper reseed between target, pause or resumed consumers/current tail',
    register_only_policy: 'one code page, no data page/read/store import/RAM readback/page dump; native owns unbound standalone ABI/locals/export currency'},
  prospective_formulas: {...formulas, regular_targets: regularTargets, pause_cases: pauseCases, malformed_controls: malformedControls, regular_contexts: regularContexts,
    contexts: plannedContexts, modules: plannedModules, generated_calls: plannedCalls, retired: plannedRetired, raw_arenas: plannedRaw, raw_arena_bytes: plannedRaw * SIZE, files: plannedFiles, host_inputs: 98, source_spans: 652, arena_checks: 15826}, counts,
  pair_census: {rows: pairCensus.size, expected: formulas.register_matrix_targets, each_exactly_once: true},
  immediate_census: {rows: immediateCensus.size, expected: formulas.immediate_matrix_targets, each_exactly_once: true}, raw_index_census: Object.fromEntries(rawIndexCensus),
  banks: banks.map(selected => ({name: selected.name, bytes: selected.bytes.length, packed: selected.packed, specs: selected.specs, instructions: selected.instructions, scans: selected.scans})),
  contexts, modules, targets, runs, mutations, host_inputs: hostInputs, raw_arena_records: rawRecords, raw_arena_artifact: savedArenas, artifacts,
  pre_execution_oracle_finding: {label: 'ALIAS-BT-A4', initial_plan_path: 'target/r3-425-alias-bt-a4-initial-plan.json',
    initial_eax: 0x12345678, initial_arena_checks: 16632, fixed_eax: 0x12345662, fixed_arena_checks: 16626, discriminator_rows: 8,
    schedule_status: '16632 and16626 are historical author plan stages before PLAN-BT-A5 arithmetic correction; final prospective schedule15826',
    finding: 'simple self0/1/2/3/4 can coincide under wrong CF read using newly published parent and its newly masked index',
    fix: 'self EAX oldindex2/oldCF0 produces12345666 with hypothetical newindex6/newCF1; retain pending cancellation through close',
    scope: 'author source-review correction before first actual execution; no executed fixture failure or product bug; targets/calls/retirements/files unchanged'},
  pre_execution_plan_finding: {label: 'PLAN-BT-A5', initial_driver_path: 'target/r3-425-driver-pre-plan-fix.mjs', initial_driver_bytes: 44893,
    initial_driver_sha256: '022e994e664d8a52a2018d2463a97aaea7913c829b00cbd99c4064b1022df44d',
    finding: 'aggregate literal author plan overstated the unchanged finite corpus by100targets; formula expressions and kind/mode counters were already correct',
    erroneous_plan: {regular_targets: 1836, targets: 1842, generated_calls: 5760, retired: 7380, raw_arenas: 1866, raw_arena_bytes: 7904376, arena_checks: 16626},
    corrected_plan: {regular_targets: 1736, targets: 1742, generated_calls: 5460, retired: 6980, raw_arenas: 1766, raw_arena_bytes: 7480776, arena_checks: 15826},
    schedule: '80regularcontexts×16checks +1736regularcases×8checks +64pausecases×6checks +2malformedcalls×2checks +6currencycontexts×45checks',
    scope: 'root source-review finding fixed before any syntax or actual execution; original driver physically preserved; no executed fixture failure or product bug; corpus loops/banks/semantics unchanged'},
  design_provenance: {prior_review_lessons: 'spare terminal lookup budgets and committed State/Exit/helper-only comparison across compile transfer mutations are retained from prior reviewed fixtures; no new executed failure is implied'},
  evidence_limits: {raw_frames: 'all first-target frames and four additional frames per currency context physically saved; other complete arenas checked live and represented by metadata/digests',
    temporal: 'live allocation retention ordering, pre/post-call timing, arities and unsaved continuation arenas rely on reviewed driver assertions, not independent replay of saved files',
    hypothetical: 'changed head/target replay discriminators are mathematical counterfactuals; no replay executed', code_memory: 'authored banks and exact mutation inputs saved; managed code memory not physically reread'},
  claim: 'finite actual bound register BT/BTS/BTR/BTC32: all4x64 register index/destination pairs and4x8 immediate destinations with FLAGS2/cd7, finite masked u32/imm8 edges and self aliases, literal oldCF and preservedZF/DF plus valid42/402 independence anchors, full first-target CPU before live SETB/SETZ and same-CPU cancel-before-budget0/budget0 continuation, current valid child malformedstate-before-cancel raw1 controls; consumed opcode/registerModRM/immediate upload stale4 with immediate whole live allocation checks, pure13-byte fresh current tail budget5 without reseed/replay, same-byte tail invalidation and retained closed5. Physical target/module/program joins and declared live-only limits; deterministic undefined flag policy, intentional offline immutable primary inputs. No hardware undefined-flags/atomicity/concurrency/memory/word/x64/prefix/SDK/browser/CI/bootstrap/game/performance claim.'};
assert.equal(artifacts.length + 1, plannedFiles); writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2), {flag: 'wx'});
assert.deepEqual(readdirSync(output).sort(), [...artifacts.map(row => row.path), 'result.json'].sort(), 'exact physical output census');
const resultBytes = readFileSync(join(output, 'result.json')); console.log(JSON.stringify({status: 'ok', engine_sha256: engineSha256, result_sha256: hash(resultBytes), targets: counts.targets,
  contexts: counts.contexts, modules: counts.modules, generated_calls: counts.generated_calls, retired: counts.retired, raw_arenas: counts.raw_arenas, files: plannedFiles, output}));
