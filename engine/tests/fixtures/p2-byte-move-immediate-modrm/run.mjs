import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(root, 'expected actual engine Wasm, output directory and repository root');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const engineBytes = readFileSync(enginePath), engineModule = new WebAssembly.Module(engineBytes);
assert.deepEqual(WebAssembly.Module.imports(engineModule), []);
const SIZE = 4236, TRANSFER = 140, PC = 0x1000, BODY = 162, COLD = PC + BODY, BASIS = [2, 0xcd7];
const ALIASES = [['AL', 0, 3], ['CL', 1, 3], ['DL', 2, 3], ['BL', 3, 3], ['AH', 0, 2], ['CH', 1, 2], ['DH', 2, 2], ['BH', 3, 2]];
const INITIAL = [0x1234807f, 0x234501fe, 0x3456aa55, 0x45665aff, 0x56789abc, 0x6789abcd, 0x789abcde, 0x89abcdef];
const FINAL = [0x1234ff00, 0x2345ff00, 0x3456ff00, 0x4566ff00, ...INITIAL.slice(4)];
const ANCHORS = [
  [0x12348000, 0x12340000, 0x12340080, 0x123400ff], [0x23450100, 0x23450000, 0x23450080, 0x234500ff],
  [0x3456aa00, 0x34560000, 0x34560080, 0x345600ff], [0x45665a00, 0x45660000, 0x45660080, 0x456600ff],
  [0x123400ff, 0x12340000, 0x12348000, 0x1234ff00], [0x234500ff, 0x23450000, 0x23458000, 0x2345ff00],
  [0x345600ff, 0x34560000, 0x34568000, 0x3456ff00], [0x456600ff, 0x45660000, 0x45668000, 0x4566ff00]
];
function byte(registers, alias) {
  const [, parent, lane] = ALIASES[alias]; return parseInt(registers[parent].toString(16).padStart(8, '0').slice(lane * 2, lane * 2 + 2), 16);
}
function insert(registers, alias, value) {
  const lanes = registers.map(word => word.toString(16).padStart(8, '0').match(/../g)), [, parent, lane] = ALIASES[alias];
  lanes[parent][lane] = value.toString(16).padStart(2, '0'); return lanes.map(bytes => parseInt(bytes.join(''), 16));
}
const aliasBytes = ALIASES.map((_, alias) => byte(INITIAL, alias));
assert.deepEqual(aliasBytes, [0x7f, 0xfe, 0x55, 0xff, 0x80, 1, 0xaa, 0x5a]); assert.equal(new Set(aliasBytes).size, 8); assert.ok(aliasBytes.every(value => value !== 0));
assert.ok(BASIS.every(flags => (flags & ~0xcd7) === 0 && (flags & 2) !== 0));
const source = Buffer.alloc(164); let offset = 0;
for (let alias = 0; alias < 8; alias++) {
  assert.equal(offset, alias * 20); source.set([0xc6, 0xc0 + alias, 0, 0xc6, 0xc0 + alias, 0, 0x88, 0xc0 + 8 * alias + (alias ^ 4)], offset); offset += 8;
  for (const immediate of [0x80, 0xff]) for (let step = 0; step < 2; step++) {source.set([0xc6, 0xc0 + alias, immediate], offset); offset += 3;}
}
assert.equal(offset, 160); source.set([0xeb, 0, 0x0f, 0x0b], offset); assert.deepEqual(readFileSync(join(output, 'bank-0.x86')), source);
const currentSource = Buffer.from(source); currentSource[2] = 0x80;
const SOURCE_HASHES = ['596b17cf9953e523c68e13520e312401945701931e7fee34a0ed70b8859dbb6b', 'b615ee505cb54f6bebcfdd7522622fcc8a999fa9f46f73311d9c8eacfbc4d38f'];
const PAGE_HASHES = ['9f8c19cd2bb8474fdd743f648d0299599bcc526bada9793c61d1ca2914cc3643', 'dc738afc20095817f581da07455ca9c314c0ec09757220846fbadd437824be8c'];
assert.deepEqual([hash(source), hash(currentSource)], SOURCE_HASHES);
const counts = {contexts: 0, modules: 0, seeds: 0, alias_groups: 0, first_moves: 0, second_moves: 0, immediate_moves: 0,
  changed_immediate_moves: 0, unchanged_immediate_moves: 0, second_idempotent_moves: 0, dependent_moves: 0, changed_dependent_moves: 0,
  jumps: 0, positive_retirements: 0, generated_calls: 0, preflight: 0, owner_controls: 0, same_byte_invalidations: 0,
  consumed_immediate_invalidations: 0, current_continuations: 0, maps: 0, host_uploads: 0, host_uploaded_bytes: 0,
  standalone_page_inputs: 0, pages: 0, engine_page_rows: 0, standalone_page_rows: 0, diagnostic_read32_calls: 0, arena_checks: 0};
const contexts = [], modules = [], pageRows = [], mutations = [], invalidations = [], hostInputs = [];
function record(magic, size, fields) {
  const bytes = Buffer.alloc(size); bytes.write(magic); bytes.writeUInt16LE(1, 4); bytes.writeUInt16LE(1, 6); bytes.writeUInt32LE(size, 8);
  fields.forEach((value, index) => bytes.writeUInt32LE(value >>> 0, 16 + index * 4)); return bytes;
}
const state = (registers, pc, flags) => record('R3ST', 56, [...registers, pc, flags]);
const exit = (reason, retired) => record('R3EX', 40, [reason, retired, 0, 0, 0, 0]);
const helper = value => record('R3MH', 40, [0, value, 0, 0, 0, 0]);
const words = values => {const bytes = Buffer.alloc(values.length * 4); values.forEach((value, index) => bytes.writeUInt32LE(value >>> 0, index * 4)); return bytes;};
const pattern = Buffer.from(Array.from({length: 4096}, (_, index) => index % 251 + 1));
assert.equal(hash(pattern), 'fe3210fbc33c9e62bd9abb7526439cb3658c4d4fb4ec41d7ccbd7ad0e54975b5');
function refresh(ctx) {ctx.bytes = new Uint8Array(ctx.memory.buffer); ctx.view = new DataView(ctx.memory.buffer); return ctx;}
function check(ctx, label) {refresh(ctx); assert.deepEqual(Buffer.from(ctx.bytes.subarray(ctx.base, ctx.base + SIZE)), ctx.expected, `${ctx.owner}/${label}: complete arena`); counts.arena_checks++;}
function pure(ctx, action, label, status = 0) {check(ctx, `before ${label}`); assert.equal(action(), status, label); check(ctx, label);}
function request(ctx, bytes) {refresh(ctx).bytes.set(bytes, ctx.base + TRANSFER); ctx.expected.set(bytes, TRANSFER); check(ctx, 'explicit host request');}
function upload(ctx, address, bytes) {
  request(ctx, bytes); pure(ctx, () => ctx.api.upload(address, bytes.length), 'explicit host upload'); counts.host_uploads++; counts.host_uploaded_bytes += bytes.length;
  hostInputs.push({kind: 'upload', owner: ctx.owner, address, length: bytes.length, sha256: hash(bytes)});
}
function fresh(owner) {
  const expected = Buffer.alloc(SIZE), ordinal = ++counts.contexts, codePage = Buffer.alloc(4096); codePage.set(source); assert.equal(hash(codePage), PAGE_HASHES[0]);
  expected.set(state(Array(8).fill(0), 0, 2)); expected.set(exit(1, 0), 56); expected.set(helper(0), 100);
  const ctx = {owner, expected, codePage, low: ordinal, high: 0xc6000000};
  if (owner === 'standalone') {
    ctx.memory = new WebAssembly.Memory({initial: 1}); ctx.base = 128; ctx.dataAddress = 0x8000; ctx.codeAddress = 0x6000;
    refresh(ctx).bytes.set(expected, ctx.base); ctx.bytes.set(pattern, ctx.dataAddress); ctx.bytes.set(codePage, ctx.codeAddress); counts.standalone_page_inputs += 2;
    for (const [address, bytes] of [[ctx.dataAddress, pattern], [ctx.codeAddress, codePage]]) hostInputs.push({kind: 'standalone_page', owner, address, length: bytes.length, sha256: hash(bytes)});
  } else {
    const instance = new WebAssembly.Instance(engineModule, {}), arities = {open: 3, close: 0, arena_ptr: 0, map: 3, upload: 2, read32: 1,
      compile_entries: 2, compile_resident: 1, generation: 0, module_ptr: 0, module_len: 0, guard: 6, guard_resident: 7};
    ctx.api = Object.fromEntries(Object.entries(arities).map(([name, arity]) => {
      const fn = instance.exports[`ring3_abi_v1_${name}`]; assert.equal(typeof fn, 'function', name); assert.equal(fn.length, arity, name); return [name, fn];
    }));
    ctx.memory = instance.exports.memory; assert.equal(ctx.api.open(3, ctx.low, ctx.high), 0);
    ctx.base = ctx.api.arena_ptr() >>> 0; ctx.dataAddress = 0x3000; ctx.codeAddress = PC; check(ctx, 'independently initialized arena');
    for (const [address, permissions, bytes] of [[ctx.dataAddress, 3, pattern], [PC, 7, source]]) {
      pure(ctx, () => ctx.api.map(address, 1, permissions), 'page map'); counts.maps++; hostInputs.push({kind: 'map', owner, address, pages: 1, permissions}); upload(ctx, address, bytes);
    }
  }
  ctx.expected.set(pattern, TRANSFER); refresh(ctx).bytes.set(pattern, ctx.base + TRANSFER); check(ctx, 'explicit patterned transfer');
  contexts.push({owner, ordinal, arena_base: ctx.base, code_address: ctx.codeAddress, data_address: ctx.dataAddress,
    ...(ctx.api ? {key_low: ctx.low, key_high: ctx.high} : {})}); return ctx;
}
function compile(ctx, phase = 'initial') {
  let bytes, binding = {};
  if (ctx.owner === 'standalone') bytes = readFileSync(join(output, 'standalone-initial-bank-0.wasm'));
  else {
    request(ctx, words(ctx.owner === 'replacement' ? [PC] : [PC, BODY]));
    if (ctx.owner === 'replacement') {
      pure(ctx, () => ctx.api.compile_entries(1, 0), 'one-entry compiler');
      binding = {generation: ctx.api.generation(), pointer: ctx.api.module_ptr() >>> 0, length: ctx.api.module_len() >>> 0}; assert.ok(binding.generation > 0);
    } else {
      check(ctx, 'before one-extent compiler'); assert.equal(ctx.api.compile_resident(1), 0); refresh(ctx);
      assert.equal(ctx.view.getUint32(ctx.base + TRANSFER, true), 1); assert.equal(ctx.view.getUint32(ctx.base + TRANSFER + 4, true), 24);
      binding = Object.fromEntries(['low', 'high', 'pointer', 'length'].map((name, index) => [name, ctx.view.getUint32(ctx.base + TRANSFER + 8 + index * 4, true)]));
      assert.ok(binding.low !== 0 || binding.high !== 0); ctx.expected.set(words([1, 24, binding.low, binding.high, binding.pointer, binding.length]), TRANSFER); check(ctx, 'resident metadata only');
    }
    refresh(ctx); assert.ok(binding.pointer > 0 && binding.length > 8 && binding.length <= 65536 && binding.pointer + binding.length <= ctx.bytes.length);
    bytes = Buffer.from(ctx.bytes.subarray(binding.pointer, binding.pointer + binding.length)); writeFileSync(join(output, `${ctx.owner}-${phase}-bank-0.wasm`), bytes, {flag: 'wx'});
  }
  assert.ok(bytes.length <= 65536); const module = new WebAssembly.Module(bytes), guards = ctx.owner === 'standalone' ? [] : [ctx.owner === 'replacement' ? 'guard' : 'guard_resident'];
  const declarations = [{module: 'env', name: 'memory', kind: 'memory'}, ...guards.map(name => ({module: 'ring3', name, kind: 'function'}))];
  assert.deepEqual(WebAssembly.Module.imports(module), declarations); assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  const child = new WebAssembly.Instance(module, {env: {memory: ctx.memory}, ...(ctx.api ? {ring3: ctx.api} : {})}); assert.equal(child.exports.run.length, 4); counts.modules++;
  modules.push({owner: ctx.owner, phase, context: ctx.low, source_sha256: hash(ctx.codePage.subarray(0, 164)), sha256: hash(bytes), length: bytes.length, imports: declarations,
    run_arity: child.exports.run.length, ...(ctx.api ? {key_low: ctx.low, key_high: ctx.high} : {}), ...binding}); return {phase, ...binding, run: child.exports.run};
}
function seed(ctx, flags) {
  ctx.expected.set(state(INITIAL, PC, flags)); ctx.expected.set(exit(3, 0), 56); ctx.expected.writeUInt32LE(0, 96); ctx.expected.set(helper(0xdecafbad), 100);
  refresh(ctx).bytes.set(ctx.expected.subarray(0, 140), ctx.base); check(ctx, 'one initial CPU/helper seed'); counts.seeds++;
}
function cancel(ctx, value) {ctx.expected.writeUInt32LE(value, 96); refresh(ctx).view.setUint32(ctx.base + 96, value, true);}
function run(ctx, unit, budget, label, registers, pc, flags, reason = 1, retired = 1, status = 0, malformed = false) {
  check(ctx, `before ${label}`); if (status === 0) {ctx.expected.set(state(registers, pc, flags)); ctx.expected.set(exit(reason, retired), 56);}
  const pointers = malformed ? [0xffffffff, 0xffffffff, 0xffffffff] : [ctx.base, ctx.base + 56, ctx.base + 96];
  assert.equal(unit.run(pointers[0], pointers[1], budget, pointers[2]), status, label); check(ctx, label); counts.generated_calls++;
}
const identity = (ctx, unit) => ctx.owner === 'replacement' ? {generation: unit.generation} : {low: unit.low, high: unit.high};
const cpuEvidence = ctx => ({state_hex: ctx.expected.subarray(0, 56).toString('hex'), exit_hex: ctx.expected.subarray(56, 96).toString('hex'), helper_hex: ctx.expected.subarray(100, 140).toString('hex')});
function mutateConsumed(ctx, unit, registers, flags) {
  assert.equal(ctx.expected.readUInt32LE(48), PC + 3); assert.equal(registers[0], 0x12348000); assert.equal(flags, 0xcd7); assert.equal(ctx.codePage[2], 0);
  const row = {owner: ctx.owner, context: ctx.low, address: PC + 2, old_byte: 0, new_byte: 0x80, upload_bytes: 1,
    eip_before: PC + 3, flags, before: cpuEvidence(ctx), old_identity: identity(ctx, unit)};
  upload(ctx, PC + 2, Buffer.from([0x80])); ctx.codePage[2] = 0x80; assert.equal(hash(ctx.codePage), PAGE_HASHES[1]); cancel(ctx, 1);
  run(ctx, unit, 0, 'consumed immediate stale owner before cancelled malformed pointers', registers, PC + 3, flags, 0, 0, 4, true);
  counts.owner_controls++; counts.consumed_immediate_invalidations++; row.stale_status = 4; cancel(ctx, 0); row.cancel_before_continuation = 0;
  const current = compile(ctx, 'current'); assert.notDeepEqual(identity(ctx, current), row.old_identity); row.new_identity = identity(ctx, current);
  ctx.pendingMutation = row; return current;
}
function pages(ctx, label) {
  for (const [address, expected] of [[ctx.dataAddress, pattern], [ctx.codeAddress, ctx.codePage]]) {
    const actual = Buffer.alloc(4096);
    if (ctx.owner === 'standalone') {actual.set(refresh(ctx).bytes.subarray(address, address + 4096)); counts.standalone_page_rows++;}
    else {
      for (let offset = 0; offset < 4096; offset += 4) {
        check(ctx, 'before separate diagnostic Read32'); assert.equal(ctx.api.read32(address + offset), 0); refresh(ctx);
        actual.writeUInt32LE(ctx.view.getUint32(ctx.base + 120, true), offset); ctx.expected.set(helper(expected.readUInt32LE(offset)), 100); check(ctx, 'diagnostic Read32 changes helper v1 only'); counts.diagnostic_read32_calls++;
      }
      counts.engine_page_rows++;
    }
    assert.deepEqual(actual, expected, 'complete declared page follows only explicit host inputs'); counts.pages++; pageRows.push({owner: ctx.owner, label, address, sha256: hash(actual)});
  }
}
function numeric(ctx, initialUnit) {
  let unit = initialUnit;
  for (const flags of BASIS) {
    let registers = [...INITIAL]; seed(ctx, flags);
    if (flags === 2) {
      run(ctx, unit, 0, 'zero budget at first C6', registers, PC, flags, 1, 0); counts.preflight++;
      cancel(ctx, 1); run(ctx, unit, 1, 'cancel before first C6', registers, PC, flags, 2, 0); counts.preflight++; cancel(ctx, 0);
    }
    for (let alias = 0; alias < 8; alias++) {
      const steps = [[alias, 0, 3], [alias, 0, 6], [alias ^ 4, null, 8], [alias, 0x80, 11], [alias, 0x80, 14], [alias, 0xff, 17], [alias, 0xff, 20]];
      for (const [step, [destination, literal, next]] of steps.entries()) {
        const before = registers; registers = insert(registers, destination, literal === null ? byte(before, alias) : literal);
        const checkpoint = [0, 2, 3, 5].indexOf(step); if (checkpoint !== -1) assert.equal(registers[alias % 4], ANCHORS[alias][checkpoint], 'literal current parent anchor');
        const changed = registers.some((value, index) => value !== before[index]);
        run(ctx, unit, 1, 'complete C6 or live current-source byte move CPU', registers, PC + alias * 20 + next, flags); counts.positive_retirements++;
        if (literal === null) {assert.ok(changed); counts.dependent_moves++; counts.changed_dependent_moves++;}
        else {
          counts.immediate_moves++; counts[changed ? 'changed_immediate_moves' : 'unchanged_immediate_moves']++;
          if ([0, 3, 5].includes(step)) counts.first_moves++;
          else {assert.equal(changed, false, 'second real C6 is idempotent'); counts.second_moves++; counts.second_idempotent_moves++;}
        }
        if (ctx.pendingMutation) {
          assert.equal(alias, 0); assert.equal(step, 1); assert.equal(ctx.expected.readUInt32LE(48), PC + 6); assert.equal(registers[0], 0x12348000);
          ctx.pendingMutation.eip_after = PC + 6; ctx.pendingMutation.retired = 1; ctx.pendingMutation.after = cpuEvidence(ctx); mutations.push(ctx.pendingMutation); delete ctx.pendingMutation; counts.current_continuations++;
        }
        if (ctx.api && flags === 0xcd7 && alias === 0 && step === 0) unit = mutateConsumed(ctx, unit, registers, flags);
      }
      counts.alias_groups++;
    }
    assert.deepEqual(registers, FINAL); run(ctx, unit, 1, 'following short JMP', registers, COLD, flags); counts.jumps++; counts.positive_retirements++;
  }
  return unit;
}
function finish(ctx, unit) {
  const registers = Array.from({length: 8}, (_, index) => ctx.expected.readUInt32LE(16 + index * 4)), flags = ctx.expected.readUInt32LE(52);
  for (const [name, cancelled, budget, reason] of [['zero budget', 0, 0, 1], ['cancel with budget', 1, 1, 2], ['cancel with zero budget', 1, 0, 2]]) {
    cancel(ctx, cancelled); run(ctx, unit, budget, name, registers, COLD, flags, reason, 0); counts.preflight++;
  }
  cancel(ctx, 0); run(ctx, unit, 1, 'cold entry', registers, COLD, flags, 3, 0); counts.preflight++;
  run(ctx, unit, 0, 'malformed pointers', registers, COLD, flags, 0, 0, 1, true); counts.preflight++; pages(ctx, 'complete pages after two live current byte chains');
  if (ctx.owner === 'standalone') return;
  for (const wrong of ['key', 'identity']) {
    const low = ctx.low ^ Number(wrong === 'key');
    const action = ctx.owner === 'replacement' ? () => ctx.api.guard(low, ctx.high, unit.generation + Number(wrong === 'identity'), ctx.base, ctx.base + 56, ctx.base + 96)
      : () => ctx.api.guard_resident(low, ctx.high, unit.low ^ Number(wrong === 'identity'), unit.high, ctx.base, ctx.base + 56, ctx.base + 96);
    pure(ctx, action, `wrong ${wrong}`, 3); counts.owner_controls++;
  }
  assert.equal(ctx.codePage[0], 0xc6); upload(ctx, PC, Buffer.from([0xc6])); cancel(ctx, 1);
  run(ctx, unit, 0, 'same-byte stale current owner before cancelled malformed pointers', registers, COLD, flags, 0, 0, 4, true); counts.owner_controls++; counts.same_byte_invalidations++;
  const row = {owner: ctx.owner, context: ctx.low, address: PC, old_byte: 0xc6, new_byte: 0xc6, upload_bytes: 1, stale_status: 4}; pages(ctx, 'current complete pages after same-byte invalidation');
  pure(ctx, () => ctx.api.close(), 'close'); run(ctx, unit, 0, 'closed current owner before cancelled malformed pointers', registers, COLD, flags, 0, 0, 5, true); counts.owner_controls++; row.closed_status = 5; invalidations.push(row);
}
for (const owner of ['standalone', 'replacement', 'resident']) {
  const ctx = fresh(owner), unit = compile(ctx); pages(ctx, 'initial complete code and patterned data'); const current = numeric(ctx, unit); finish(ctx, current);
}
const expectedCounts = {contexts: 3, modules: 5, seeds: 6, alias_groups: 48, first_moves: 144, second_moves: 144, immediate_moves: 288,
  changed_immediate_moves: 120, unchanged_immediate_moves: 168, second_idempotent_moves: 144, dependent_moves: 48, changed_dependent_moves: 48,
  jumps: 6, positive_retirements: 342, generated_calls: 369, preflight: 21, owner_controls: 10, same_byte_invalidations: 2,
  consumed_immediate_invalidations: 2, current_continuations: 2, maps: 4, host_uploads: 8, host_uploaded_bytes: 8524,
  standalone_page_inputs: 2, pages: 16, engine_page_rows: 12, standalone_page_rows: 4, diagnostic_read32_calls: 12288, arena_checks: 25381};
assert.equal(Object.keys(counts).length, 29); assert.deepEqual(Object.keys(counts), Object.keys(expectedCounts));
for (const [name, expected] of Object.entries(expectedCounts)) assert.equal(counts[name], expected, `exact observed ${name} census`);
assert.deepEqual(modules.map(({owner, phase}) => [owner, phase]), [['standalone', 'initial'], ['replacement', 'initial'], ['replacement', 'current'], ['resident', 'initial'], ['resident', 'current']]);
assert.equal(contexts.length, 3); assert.equal(mutations.length, 2); assert.equal(invalidations.length, 2); assert.equal(hostInputs.length, 14);
const result = {status: 'ok', engine_sha256: hash(engineBytes), tools: {node: process.version, v8: process.versions.v8}, counts, contexts, modules, pages: pageRows,
  consumed_immediate_mutations: mutations, code_invalidations: invalidations, host_inputs: hostInputs,
  source: {url: 'https://cdrdv2-public.intel.com/929354/253667-093-sdm-vol-2b.pdf', order: '253667-093US', edition: 'September 2026',
    pdf_sha256: 'a261998ace8e07f624bf3e2486bb6cfe950fb8be8d0dc9631bcecb4c5ec953e4', full_text_sha256: 'f5e6dc689d41655d64792512bfe8add56fc4ac97203aab5c1377a72bac291a35',
    chapter_sha256: 'f5ba550b5241b21ffc957cb70feccbc4678cfc264470e46ffc2b50cf89000a55', chapter_bytes: 10789, chapter_lines: [1518, 1714], pdf_pages_zero_based: [35, 36, 37, 38], printed_pages: ['4-28', '4-29', '4-30', '4-31'],
    page_sha256: ['b47fc7b513a3f900faad89cac81eaff713b3204a649fa5c73d555d5d56f76811', '605ad09e0e33abfe9aed3f8fd05c7bc271e73689edeb6eaf9c52ffe7449d6283', '8ab69394df4507f9084112751735c9dc0b12f448dd75954c14df5025e9be0b4c', '681eb847768008e2da25a09f12e9b4b731d46a600d4e23e629676f9ee71afb6a']},
  oracle: {initial: INITIAL, final: FINAL, alias_bytes: aliasBytes, basis_flags: BASIS, immediates: [0, 0x80, 0xff], aliases: ALIASES, literal_parent_anchors: ANCHORS, literal_anchor_count: 32,
    limits: {instructions: 64, blocks: 8, wasm_bytes: 65536}, bank: {specs: [[PC, BODY]], row_offsets: [0, 20, 40, 60, 80, 100, 120, 140], step_next_offsets: [3, 6, 8, 11, 14, 17, 20], instructions: 57, body_bytes: BODY, source_bytes: 164, cold_target: COLD},
    programs: {initial: {hex: source.toString('hex'), sha256: SOURCE_HASHES[0], code_page_sha256: PAGE_HASHES[0]}, current: {hex: currentSource.toString('hex'), sha256: SOURCE_HASHES[1], code_page_sha256: PAGE_HASHES[1]}},
    arena_bytes: SIZE, pattern_sha256: hash(pattern), fixed_points: {second_moves: 144, high_alias_first_zero_moves: 24},
    policy: 'replace one captured hex byte string in the current full parent; existing 88 reads the newly produced zero byte before writing its opposite lane; all guest calls preserve full FLAGS and unselected lanes, with no intermediate CPU repair; changed consumed first immediate stays behind existing EIP 1003 and fresh owner resumes second zero at 1003 then live 88 without replay',
    diagnostic_helper_policy: 'guest modules import no data helpers and retain R3MHv1; separate page Read32 diagnostics change only helper v1 success value with length/reserved zero and have before/after whole-arena checks'},
  artifact_census: {x86_banks: 1, modules: 5, result_files: 1, total: 7},
  test_sha256: Object.fromEntries(['engine/tests/cpu_byte_move_immediate_modrm.rs', 'engine/tests/cpu_byte_move_immediate_modrm_wasm.rs', 'engine/tests/fixtures/p2-byte-move-immediate-modrm/run.mjs'].map(path => [path, hash(readFileSync(join(root, path)))])),
  claim: 'prefix-free flat32 register C6 /0 immediate byte MOV through native-generated standalone and actual engine-Wasm replacement/resident modules: all eight low/high aliases, finite literal unsigned 0/80/ff and complementary FLAGS 2/cd7, one declared asymmetric parent vector, independent current hex-parent oracle and 32 literal anchors; complete CPU before each dependent 88 copy and each second real idempotent target, unchanged other 24 parent bits/all other GPRs/full FLAGS/helper/RAM, zero and value-fixed writes retire once; six seeds / 288 C6 + 48 live 88 + six JMP, exact pure ABI/default caps/4236-byte arena/current 16 complete pages, two changed consumed-immediate stale owners and fresh no-replay same-CPU 1003 continuations, two same-opcode stale 4 / closed 5 before cancelled malformed arguments; no exhaustive imm8-FLAGS Cartesian, memory MOV expansion/guest self-modifying Store8/RTM/other widths or prefixes/REX/new helpers-ABI-caps/PE/Windows/SDK/browser/performance/fullCI/game claim'};
const resultBytes = Buffer.from(JSON.stringify(result, null, 2)); writeFileSync(join(output, 'result.json'), resultBytes, {flag: 'wx'});
console.log(JSON.stringify({status: result.status, engine_sha256: result.engine_sha256, result_sha256: hash(resultBytes), counts, output}));
