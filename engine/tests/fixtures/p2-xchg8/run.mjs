import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(root, 'expected actual engine Wasm, output directory and repository root');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const engineBytes = readFileSync(enginePath), engineModule = new WebAssembly.Module(engineBytes);
assert.deepEqual(WebAssembly.Module.imports(engineModule), []);
const SIZE = 4236, TRANSFER = 140, PC = 0x1000, BASIS = [2, 0xcd7];
const ALIASES = [['AL', 0, 3], ['CL', 1, 3], ['DL', 2, 3], ['BL', 3, 3], ['AH', 0, 2], ['CH', 1, 2], ['DH', 2, 2], ['BH', 3, 2]];
const VECTORS = [
  [0x1234807f, 0x234501fe, 0x3456aa55, 0x456600ff, 0x56789abc, 0x6789abcd, 0x789abcde, 0x89abcdef],
  [0x89ab817e, 0x9abc02fd, 0xabcd56a9, 0xbcdeff00, 0xcdef1357, 0xdef02468, 0xef012345, 0xf0126789]
];
function exchange(registers, left, right) {
  const lanes = registers.map(word => word.toString(16).padStart(8, '0').match(/../g));
  const [, leftParent, leftByte] = ALIASES[left], [, rightParent, rightByte] = ALIASES[right];
  const oldLeft = lanes[leftParent][leftByte], oldRight = lanes[rightParent][rightByte];
  lanes[leftParent][leftByte] = oldRight; lanes[rightParent][rightByte] = oldLeft;
  return lanes.map(bytes => parseInt(bytes.join(''), 16));
}
const ANCHORS = [
  [0, 0, 0, [[0, 0x1234807f]]], [0, 4, 4, [[0, 0x1234807f]]], [0, 0, 4, [[0, 0x12347f80]]],
  [0, 0, 1, [[0, 0x123480fe], [1, 0x2345017f]]], [0, 4, 7, [[0, 0x1234007f], [3, 0x456680ff]]], [0, 0, 5, [[0, 0x12348001], [1, 0x23457ffe]]],
  [1, 0, 0, [[0, 0x89ab817e]]], [1, 4, 4, [[0, 0x89ab817e]]], [1, 0, 4, [[0, 0x89ab7e81]]],
  [1, 0, 1, [[0, 0x89ab81fd], [1, 0x9abc027e]]], [1, 4, 7, [[0, 0x89abff7e], [3, 0xbcde8100]]], [1, 0, 5, [[0, 0x89ab8102], [1, 0x9abc7efd]]]
];
for (const [vector, left, right, changed] of ANCHORS) {
  const expected = [...VECTORS[vector]]; for (const [parent, word] of changed) expected[parent] = word;
  assert.deepEqual(exchange(VECTORS[vector], left, right), expected, 'literal simultaneous byte exchange anchor');
}
const aliasBytes = VECTORS.map(registers => ALIASES.map(([, parent, byte]) => parseInt(registers[parent].toString(16).padStart(8, '0').slice(byte * 2, byte * 2 + 2), 16)));
assert.deepEqual(aliasBytes, [[0x7f, 0xfe, 0x55, 0xff, 0x80, 1, 0xaa, 0], [0x7e, 0xfd, 0xa9, 0, 0x81, 2, 0x56, 0xff]]);
assert.ok(aliasBytes.every(bytes => new Set(bytes).size === 8));
assert.ok(BASIS.every(flags => (flags & ~0xcd7) === 0 && (flags & 2) !== 0));
const SOURCE_HASHES = ['bb630907be5ac79c72794da65b34f6baa1f9d2d5a7edcbef75ea3fd462c5212c', '6a964be33aa23695b1d2a171159e24dbcecef4222ee3965b57ebc72ddd77d852', '3754620372e4c516fa3d3d8c69a265d0ead63d1c545ec98a72838d4a3dfeffaf'];
const PAGE_HASHES = ['1e708b7c58e0ae1832f6099a3b4d077e51c66ecf9bb32e08cbcc597fc53290be', '415b279f37690d2e9f3e48c0361e9bead9736421da6844b0886b5d23d7166fa3', 'f76a0981b9fda6b4fab7e10638aed780bc0de3ad78d089b2d8089656ee6c7375'];
const banks = [[0, 28], [28, 56], [56, 64]].map(([first, end], bank) => {
  const bytes = Buffer.alloc([116, 116, 36][bank]); let offset = 0;
  for (let pair = first; pair < end; pair++) {
    const left = Math.floor(pair / 8), right = pair % 8, operand = 0xc0 | right << 3 | left;
    for (let step = 0; step < 2; step++) {bytes.set([0x86, operand], offset); offset += 2;}
  }
  bytes.set([0xeb, 0, 0x0f, 0x0b], offset); const body = offset + 2;
  assert.equal(body, [114, 114, 34][bank]); assert.equal(2 * (end - first) + 1, [57, 57, 17][bank]);
  assert.deepEqual(readFileSync(join(output, `bank-${bank}.x86`)), bytes); assert.equal(hash(bytes), SOURCE_HASHES[bank]);
  return {first, end, bytes, body, cold: PC + body, instructions: 2 * (end - first) + 1};
});
const counts = {contexts: 0, modules: 0, seeds: 0, pair_groups: 0, first_exchanges: 0, second_exchanges: 0, exchanges: 0,
  changed_first: 0, round_trips: 0, self_exchanges: 0, same_parent_cross_exchanges: 0, different_parent_exchanges: 0,
  jumps: 0, positive_retirements: 0, generated_calls: 0, preflight: 0, owner_controls: 0, same_byte_invalidations: 0,
  maps: 0, host_uploads: 0, host_uploaded_bytes: 0, standalone_page_inputs: 0, pages: 0, engine_page_rows: 0,
  standalone_page_rows: 0, diagnostic_read32_calls: 0, arena_checks: 0};
const modules = [], pageRows = [], invalidations = [];
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
function check(ctx, label) {refresh(ctx); assert.deepEqual(Buffer.from(ctx.bytes.subarray(ctx.base, ctx.base + SIZE)), ctx.expected, `${ctx.owner}/bank ${ctx.bank}/${label}: complete arena`); counts.arena_checks++;}
function pure(ctx, action, label, status = 0) {check(ctx, `before ${label}`); assert.equal(action(), status, label); check(ctx, label);}
function request(ctx, bytes) {refresh(ctx).bytes.set(bytes, ctx.base + TRANSFER); ctx.expected.set(bytes, TRANSFER); check(ctx, 'explicit host request');}
function upload(ctx, address, bytes) {request(ctx, bytes); pure(ctx, () => ctx.api.upload(address, bytes.length), 'explicit host upload'); counts.host_uploads++; counts.host_uploaded_bytes += bytes.length;}
function fresh(owner, bank) {
  const expected = Buffer.alloc(SIZE), ordinal = ++counts.contexts, codePage = Buffer.alloc(4096); codePage.set(banks[bank].bytes);
  assert.equal(hash(codePage), PAGE_HASHES[bank]); expected.set(state(Array(8).fill(0), 0, 2)); expected.set(exit(1, 0), 56); expected.set(helper(0), 100);
  const ctx = {owner, bank, cold: banks[bank].cold, expected, codePage, low: ordinal, high: 0xc8860000};
  if (owner === 'standalone') {
    ctx.memory = new WebAssembly.Memory({initial: 1}); ctx.base = 128; ctx.dataAddress = 0x8000; ctx.codeAddress = 0x6000;
    refresh(ctx).bytes.set(expected, ctx.base); ctx.bytes.set(pattern, ctx.dataAddress); ctx.bytes.set(codePage, ctx.codeAddress); counts.standalone_page_inputs += 2;
  } else {
    const instance = new WebAssembly.Instance(engineModule, {}), arities = {open: 3, close: 0, arena_ptr: 0, map: 3, upload: 2, read32: 1,
      compile_entries: 2, compile_resident: 1, generation: 0, module_ptr: 0, module_len: 0, guard: 6, guard_resident: 7};
    ctx.api = Object.fromEntries(Object.entries(arities).map(([name, arity]) => {
      const fn = instance.exports[`ring3_abi_v1_${name}`]; assert.equal(typeof fn, 'function', name); assert.equal(fn.length, arity, name); return [name, fn];
    }));
    ctx.memory = instance.exports.memory; assert.equal(ctx.api.open(3, ctx.low, ctx.high), 0);
    ctx.base = ctx.api.arena_ptr() >>> 0; ctx.dataAddress = 0x3000; ctx.codeAddress = PC; check(ctx, 'independently initialized arena');
    for (const [address, permissions, bytes] of [[ctx.dataAddress, 3, pattern], [PC, 7, banks[bank].bytes]]) {
      pure(ctx, () => ctx.api.map(address, 1, permissions), 'page map'); counts.maps++; upload(ctx, address, bytes);
    }
  }
  ctx.expected.set(pattern, TRANSFER); refresh(ctx).bytes.set(pattern, ctx.base + TRANSFER); check(ctx, 'explicit patterned transfer'); return ctx;
}
function compile(ctx) {
  let bytes, binding = {};
  if (ctx.owner === 'standalone') bytes = readFileSync(join(output, `standalone-bank-${ctx.bank}.wasm`));
  else {
    request(ctx, words(ctx.owner === 'replacement' ? [PC] : [PC, banks[ctx.bank].body]));
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
    bytes = Buffer.from(ctx.bytes.subarray(binding.pointer, binding.pointer + binding.length)); writeFileSync(join(output, `${ctx.owner}-bank-${ctx.bank}.wasm`), bytes, {flag: 'wx'});
  }
  assert.ok(bytes.length <= 65536); const module = new WebAssembly.Module(bytes), imports = ctx.owner === 'standalone' ? [] : [ctx.owner === 'replacement' ? 'guard' : 'guard_resident'];
  const declarations = [{module: 'env', name: 'memory', kind: 'memory'}, ...imports.map(name => ({module: 'ring3', name, kind: 'function'}))];
  assert.deepEqual(WebAssembly.Module.imports(module), declarations); assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  const child = new WebAssembly.Instance(module, {env: {memory: ctx.memory}, ...(ctx.api ? {ring3: ctx.api} : {})}); assert.equal(child.exports.run.length, 4); counts.modules++;
  modules.push({owner: ctx.owner, bank: ctx.bank, sha256: hash(bytes), length: bytes.length, imports: declarations, run_arity: child.exports.run.length, ...binding}); return {...binding, run: child.exports.run};
}
function seed(ctx, registers, flags) {
  ctx.expected.set(state(registers, PC, flags)); ctx.expected.set(exit(3, 0), 56); ctx.expected.writeUInt32LE(0, 96); ctx.expected.set(helper(0xdecafbad), 100);
  refresh(ctx).bytes.set(ctx.expected.subarray(0, 140), ctx.base); check(ctx, 'one initial CPU/helper seed'); counts.seeds++;
}
function cancel(ctx, value) {ctx.expected.writeUInt32LE(value, 96); refresh(ctx).view.setUint32(ctx.base + 96, value, true);}
function run(ctx, unit, budget, label, registers, pc, flags, reason = 1, retired = 1, status = 0, malformed = false) {
  check(ctx, `before ${label}`); if (status === 0) {ctx.expected.set(state(registers, pc, flags)); ctx.expected.set(exit(reason, retired), 56);}
  const pointers = malformed ? [0xffffffff, 0xffffffff, 0xffffffff] : [ctx.base, ctx.base + 56, ctx.base + 96];
  assert.equal(unit.run(pointers[0], pointers[1], budget, pointers[2]), status, label); check(ctx, label); counts.generated_calls++;
}
function preflight(ctx, unit, registers, flags) {
  run(ctx, unit, 0, 'zero budget at first exchange', registers, PC, flags, 1, 0); counts.preflight++;
  cancel(ctx, 1); run(ctx, unit, 1, 'cancel before first exchange', registers, PC, flags, 2, 0); counts.preflight++; cancel(ctx, 0);
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
    assert.deepEqual(actual, expected, 'complete declared page unchanged'); counts.pages++; pageRows.push({owner: ctx.owner, bank: ctx.bank, label, address, sha256: hash(actual)});
  }
}
function numeric(ctx, unit) {
  for (const [vector, initial] of VECTORS.entries()) for (const flags of BASIS) {
    let registers = [...initial]; seed(ctx, registers, flags);
    if (vector === 0 && flags === 2) preflight(ctx, unit, registers, flags);
    for (let pair = banks[ctx.bank].first; pair < banks[ctx.bank].end; pair++) {
      const left = Math.floor(pair / 8), right = pair % 8, before = [...registers], offset = (pair - banks[ctx.bank].first) * 4;
      for (let step = 0; step < 2; step++) {
        registers = exchange(registers, left, right);
        if (left === right) {assert.deepEqual(registers, before); counts.self_exchanges++;}
        else {
          if (step === 0) {assert.notDeepEqual(registers, before); counts.changed_first++;}
          else {assert.deepEqual(registers, before); counts.round_trips++;}
          counts[ALIASES[left][1] === ALIASES[right][1] ? 'same_parent_cross_exchanges' : 'different_parent_exchanges']++;
        }
        run(ctx, unit, 1, 'complete current byte exchange and CPU', registers, PC + offset + (step + 1) * 2, flags);
        counts.exchanges++; counts[step === 0 ? 'first_exchanges' : 'second_exchanges']++; counts.positive_retirements++;
      }
      assert.deepEqual(registers, initial, 'second real exchange restores the group vector without CPU repair'); counts.pair_groups++;
    }
    run(ctx, unit, 1, 'following short JMP', registers, ctx.cold, flags); counts.jumps++; counts.positive_retirements++;
  }
}
function finish(ctx, unit) {
  const registers = Array.from({length: 8}, (_, index) => ctx.expected.readUInt32LE(16 + index * 4)), flags = ctx.expected.readUInt32LE(52);
  for (const [name, cancelled, budget, reason] of [['zero budget', 0, 0, 1], ['cancel with budget', 1, 1, 2], ['cancel with zero budget', 1, 0, 2]]) {
    cancel(ctx, cancelled); run(ctx, unit, budget, name, registers, ctx.cold, flags, reason, 0); counts.preflight++;
  }
  cancel(ctx, 0); run(ctx, unit, 1, 'cold entry', registers, ctx.cold, flags, 3, 0); counts.preflight++;
  run(ctx, unit, 0, 'malformed pointers', registers, ctx.cold, flags, 0, 0, 1, true); counts.preflight++; pages(ctx, 'complete pages after four guest groups');
  if (ctx.owner === 'standalone') return;
  for (const wrong of ['key', 'identity']) {
    const low = ctx.low ^ Number(wrong === 'key');
    const action = ctx.owner === 'replacement' ? () => ctx.api.guard(low, ctx.high, unit.generation + Number(wrong === 'identity'), ctx.base, ctx.base + 56, ctx.base + 96)
      : () => ctx.api.guard_resident(low, ctx.high, unit.low ^ Number(wrong === 'identity'), unit.high, ctx.base, ctx.base + 56, ctx.base + 96);
    pure(ctx, action, `wrong ${wrong}`, 3); counts.owner_controls++;
  }
  const sameByte = ctx.codePage[0]; assert.equal(sameByte, 0x86); upload(ctx, PC, Buffer.from([sameByte])); cancel(ctx, 1);
  run(ctx, unit, 0, 'same-byte stale owner before cancelled malformed pointers', registers, ctx.cold, flags, 0, 0, 4, true); counts.owner_controls++; counts.same_byte_invalidations++;
  invalidations.push({owner: ctx.owner, bank: ctx.bank, address: PC, old_byte: sameByte, new_byte: sameByte, upload_bytes: 1, stale_status: 4}); pages(ctx, 'current complete pages after same-byte invalidation');
  pure(ctx, () => ctx.api.close(), 'close'); run(ctx, unit, 0, 'closed owner before cancelled malformed pointers', registers, ctx.cold, flags, 0, 0, 5, true); counts.owner_controls++;
}
for (const owner of ['standalone', 'replacement', 'resident']) for (let bank = 0; bank < banks.length; bank++) {
  const ctx = fresh(owner, bank), unit = compile(ctx); pages(ctx, 'initial complete code and patterned data'); numeric(ctx, unit); finish(ctx, unit);
}
const expectedCounts = {contexts: 9, modules: 9, seeds: 36, pair_groups: 768, first_exchanges: 768, second_exchanges: 768, exchanges: 1536,
  changed_first: 672, round_trips: 672, self_exchanges: 192, same_parent_cross_exchanges: 192, different_parent_exchanges: 1152,
  jumps: 36, positive_retirements: 1572, generated_calls: 1647, preflight: 63, owner_controls: 24, same_byte_invalidations: 6,
  maps: 12, host_uploads: 18, host_uploaded_bytes: 25118, standalone_page_inputs: 6, pages: 48, engine_page_rows: 36,
  standalone_page_rows: 12, diagnostic_read32_calls: 36864, arena_checks: 77205};
assert.equal(Object.keys(counts).length, 27); assert.deepEqual(Object.keys(counts), Object.keys(expectedCounts));
for (const [name, expected] of Object.entries(expectedCounts)) assert.equal(counts[name], expected, `exact observed ${name} census`);
const result = {status: 'ok', engine_sha256: hash(engineBytes), tools: {node: process.version, v8: process.versions.v8}, counts, modules, pages: pageRows, code_invalidations: invalidations,
  source: {url: 'https://cdrdv2-public.intel.com/929356/334569-093-sdm-vol-2d.pdf', order: '334569-093US', edition: 'September 2026',
    pdf_sha256: '17b632da847e4757448e8afc950487b5d489da6c238de4551dc8cf6f799d3dcd',
    extract_sha256: ['6feddca1c23efe8ee3f345318192b144430a1ef6a4e361c35a7a6c8655fce001', '45270cc99c7762412ee09fae70cf8f4a5983b45218483f1cdcedb3db5032e3d3'], pdf_pages_zero_based: [34, 35], printed_pages: ['6-32', '6-33']},
  oracle: {vectors: VECTORS, alias_bytes: aliasBytes, basis_flags: BASIS, aliases: ALIASES, literal_anchors: ANCHORS, literal_anchor_count: 12,
    limits: {instructions: 64, blocks: 8, wasm_bytes: 65536}, arena_bytes: SIZE, pattern_sha256: hash(pattern),
    pair_partition: {ordered: 64, self: 8, same_parent_cross: 8, different_parent: 48},
    policy: 'capture both old hex byte strings before insertion; both inserts into a shared parent use one updated byte array; second exchange uses the first current full vector, restores it without CPU repair and retains the complete FLAGS word',
    banks: banks.map(({first, end, bytes, body, cold, instructions}, bank) => ({bank, first_pair: first, end_pair_exclusive: end, specs: [[PC, body]], cold_target: cold,
      instructions, body_bytes: body, source_bytes: bytes.length, hex: bytes.toString('hex'), sha256: hash(bytes), code_page_sha256: PAGE_HASHES[bank]}))},
  artifact_census: {x86_banks: 3, modules: 9, result_files: 1, total: 13},
  test_sha256: Object.fromEntries(['engine/tests/cpu_xchg8.rs', 'engine/tests/cpu_xchg8_wasm.rs', 'engine/tests/fixtures/p2-xchg8/run.mjs'].map(path => [path, hash(readFileSync(join(root, path)))])),
  claim: 'prefix-free flat32 register-only 86 byte exchange through native-generated standalone and actual engine-Wasm-generated replacement/resident modules: all 64 ordered AL/CL/DL/BL/AH/CH/DH/BH pairs, two declared asymmetric eight-distinct-byte parent vectors with complementary FLAGS 2/cd7, independent hex-parent byte-array oracle and twelve literal anchors; complete first exchanged CPU observed before the second real exchange from current state, no CPU repair, unselected lanes/all other GPRs/full FLAGS retained and self/zero-byte instructions retire once; finite 36 groups/1536 exchanges/36 JMP, exact pure imports/default caps/4236-byte arena and complete current code/patterned data pages, diagnostic Read32 helper effects separate from guest execution, known-entry budget/cancel and cold/malformed controls, wrong owner key/identity, same-byte code upload stale4 and closed5 before cancelled malformed arguments; no exhaustive byte or FLAGS Cartesian product, memory exchange/implicit locking, other widths/prefixes/REX, helpers/ABI/cap growth, PE/Windows/SDK/browser/performance/fullCI/game claim'};
const resultBytes = Buffer.from(JSON.stringify(result, null, 2)); writeFileSync(join(output, 'result.json'), resultBytes, {flag: 'wx'});
console.log(JSON.stringify({status: result.status, engine_sha256: result.engine_sha256, result_sha256: hash(resultBytes), counts, output}));
