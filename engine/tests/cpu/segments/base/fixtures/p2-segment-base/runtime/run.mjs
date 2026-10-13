import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../../../../../../support/wasm/engine.mjs';

const [enginePath, output, root] = process.argv.slice(2), engine = readEngine(enginePath);
const initial = readFileSync(join(output, 'initial-arena.bin'));
const SIZE = 4796, SEG = 4524, BASES = 4756, FP = 4236, PC = 0x1000, keyHigh = 0x5342;
const pages = [PC, 0x5000, 0x6000], cacheOffsets = [28, 32, 36, 40, 44, 56];
const selectors = [0x23, 0x1b, 0x23, 0x23, 0x3b, 0];
const caches = [0xf33fffff, 0xf3bfffff, 0xf33fffff, 0xf33fffff, 0xf33fffff, 0];
const ONE_PLUS = Buffer.from('0100000000000080ff3f', 'hex');
const contexts = [], modules = [], observations = [], helperTrace = [], refusals = [];
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
assert.equal(initial.length, SIZE, 'current arena includes the complete R3SB companion');
const sourcePaths = [
  'engine/src/abi/arena.rs', 'engine/src/abi/x86/segments.rs',
  'engine/src/cpu/x86/segments.rs', 'engine/src/cpu/x86/segments/cache.rs',
  'engine/src/cpu/x86/segments/descriptors.rs', 'engine/src/cpu/x86/segments/descriptor_record.rs',
  'engine/src/process/cpu/segments.rs', 'engine/src/process/cpu/x87.rs',
  'engine/src/cpu/dbt/wasm/segments.rs', 'engine/src/cpu/dbt/wasm/segments/access.rs',
  'engine/src/cpu/dbt/wasm/segments/context.rs',
  'engine/src/cpu/dbt/wasm/memory.rs', 'engine/src/cpu/dbt/wasm/strings.rs',
  'engine/src/cpu/dbt/wasm/x87.rs', 'engine/src/cpu/dbt/wasm/emitter.rs',
  'engine/src/process/wasm/cpu.rs', 'engine/src/abi/wasm/exports/cpu.rs',
  'engine/tests/support/wasm.rs', 'engine/tests/support/wasm/engine.mjs',
  'engine/tests/cpu/segments/base/wasm.rs', 'engine/tests/cpu/segments/wasm.rs',
  'engine/tests/cpu/segments/base/fixtures/p2-segment-base/runtime/run.mjs',
];
const pins = () => Object.fromEntries(sourcePaths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = pins();
const words = values => {const b = Buffer.alloc(values.length * 4); values.forEach((v, i) => b.writeUInt32LE(v >>> 0, i * 4)); return b;};
function record(magic, size, fields = [], version = 1) {
  const b = Buffer.alloc(size); b.write(magic); b.writeUInt16LE(version, 4); b.writeUInt16LE(1, 6); b.writeUInt32LE(size, 8);
  b.set(words(fields), 16); return b;
}
const packet = (fields, version = 1) => record('R3MH', 40, fields, version);
const stateRecord = state => record('R3ST', 56, [...state.r, state.pc, state.flags]);
const arena = c => Buffer.from(new Uint8Array(c.memory.buffer, c.base, SIZE));
const input = (c, at, bytes) => new Uint8Array(c.memory.buffer).set(bytes, c.base + at);
function host(c, name, args = []) {assert.equal(c.api[name](...args), 0, name);}
function upload(c, address, bytes) {input(c, 140, bytes); host(c, 'upload', [address, bytes.length]);}
function configure(c, base, limit = 0xff) {
  const before = arena(c);
  host(c, 'configure_segment_descriptor_raw', [0x43, ((base << 16) | limit) >>> 0,
    ((base & 0xff000000) | ((base >>> 16) & 0xff) | 0x0040f200) >>> 0]);
  assert.deepEqual(arena(c), before, 'descriptor configuration is arena neutral');
}
function segmentRecords({version = 3, selected = selectors, loaded = caches, bases = [0, 0, 0, 0, 0, 0]} = {}) {
  const segment = record('R3SG', 64, [], version);
  selected.forEach((value, i) => segment.writeUInt16LE(value, 16 + i * 2));
  if (version !== 1) loaded.forEach((value, i) => segment.writeUInt32LE(value >>> 0, cacheOffsets[i]));
  if (version !== 3) for (const i of [4, 5]) segment.writeUInt32LE(bases[i] >>> 0, 32 + i * 4);
  return {segment, companion: record('R3SB', 40, version === 3 ? bases : [0, 0, 0, 0, 0, 0])};
}
function loadedRecords(target, base, limit = 0xff, version = 3) {
  const selected = [...selectors], loaded = [...caches], bases = [0, 0, 0, 0, 0, 0];
  selected[target] = 0x43; loaded[target] = (0xf2300000 | limit) >>> 0; bases[target] = base >>> 0;
  return segmentRecords({version, selected, loaded, bases});
}
function install(c, records) {input(c, SEG, records.segment); input(c, BASES, records.companion);}
function seed(c, updates = {}, flags = 0xcd7) {
  const state = {r: [0x43, 0x12345678, 0xa1b2c3d4, 0x10, 0x6040, 0x456789ab, 0x5010, 0x20], pc: PC, flags};
  for (const [index, value] of Object.entries(updates)) state.r[+index] = value >>> 0;
  input(c, 0, stateRecord(state)); input(c, 96, words([0])); return state;
}
function next(state, pc, updates = {}) {
  const result = {...state, r: [...state.r], pc};
  for (const [index, value] of Object.entries(updates)) result.r[+index] = value >>> 0;
  return result;
}
function ram(c) {
  const saved = arena(c), result = Buffer.alloc(pages.length * 4096);
  pages.forEach((page, index) => {
    for (let offset = 0; offset < 4096; offset += 4) {
      host(c, 'read32', [page + offset]);
      const view = new DataView(c.memory.buffer, c.base + 100, 40);
      assert.equal(view.getUint32(16, true), 0, 'mapped RAM snapshot read succeeds');
      result.writeUInt32LE(view.getUint32(20, true), index * 4096 + offset);
    }
  });
  const observed = arena(c), allowed = Buffer.from(saved); allowed.set(observed.subarray(100, 140), 100);
  assert.deepEqual(observed, allowed, 'mapped RAM inspection changes only the helper packet');
  input(c, 100, saved.subarray(100, 140)); return result;
}
function open(owner, entries, instruction, label) {
  const instance = new WebAssembly.Instance(engine.module, {});
  const c = {id: contexts.length + 1, owner, entries, memory: instance.exports.memory, api: {}, calls: []};
  for (const [name, value] of Object.entries(instance.exports))
    if (name.startsWith('ring3_abi_v1_') && typeof value === 'function') c.api[name.slice(13)] = value;
  host(c, 'open', [8, c.id, keyHigh]); c.base = c.api.arena_ptr() >>> 0; input(c, 0, initial);
  for (const page of pages) host(c, 'map', [page, 1, page === PC ? 7 : 3]);
  const code = Buffer.from([...instruction, 0xeb, 0]); upload(c, PC, code);
  input(c, 140, words(entries ? [PC] : [PC, code.length]));
  const method = owner === 'resident' ? entries ? 'compile_resident_entries' : 'compile_resident' : entries ? 'compile_entries' : 'compile';
  host(c, method, [1, ...(entries ? [0] : [])]);
  const b = arena(c), resident = owner === 'resident';
  c.binding = resident ? [c.id, keyHigh, b.readUInt32LE(148), b.readUInt32LE(152)] : [];
  const pointer = resident ? b.readUInt32LE(156) : c.api.module_ptr() >>> 0;
  const length = resident ? b.readUInt32LE(160) : c.api.module_len() >>> 0;
  const bytes = Buffer.from(new Uint8Array(c.memory.buffer, pointer, length)), module = new WebAssembly.Module(bytes);
  const imports = WebAssembly.Module.imports(module), ring3 = {...c.api};
  c.exitVersion = imports.some(row => /^(segment_load|x87)/.test(row.name)) ? 6 : 2;
  for (const imp of imports.filter(row => row.module === 'ring3' && !row.name.startsWith('guard'))) {
    ring3[imp.name] = (...args) => {
      const before = arena(c), status = c.api[imp.name](...args), after = arena(c), allowed = Buffer.from(before);
      allowed.set(after.subarray(100, 140), 100);
      if (imp.name.startsWith('segment_load')) {
        allowed.set(after.subarray(SEG, SEG + 64), SEG); allowed.set(after.subarray(BASES, BASES + 40), BASES);
      }
      if (imp.name.startsWith('x87')) allowed.set(after.subarray(FP, FP + 128), FP);
      assert.deepEqual(after, allowed, imp.name + ' restricted helper publication');
      const call = {name: imp.name, args: args.map(value => value >>> 0), status, packet: after.subarray(100, 140).toString('hex')};
      c.calls.push(call); helperTrace.push({context: c.id, ...call}); return status;
    };
  }
  c.run = new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3}).exports.run;
  const detachedImports = {...ring3, guard: () => 0, guard_resident: () => 0};
  c.detachedPreflight = new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3: detachedImports}).exports.run;
  const file = `module-${c.id}.wasm`; writeFileSync(join(output, file), bytes, {flag: 'wx'});
  modules.push({context: c.id, file, bytes: length, sha256: hash(bytes), imports});
  contexts.push({id: c.id, owner, entries, label, code: Buffer.from(instruction).toString('hex')}); return c;
}
function run(c, label, state, {budget = 1, retired = budget, reason = 1, fault = [0, 0, 0, 0], records, fp, calls = [], writes = []} = {}) {
  const before = arena(c), beforeRam = ram(c), expected = Buffer.from(before), expectedRam = Buffer.from(beforeRam), at = c.calls.length;
  expected.set(stateRecord(state)); expected.set(record('R3EX', 40, [reason, retired, ...fault], reason >= 11 ? 6 : c.exitVersion), 56);
  if (records) {expected.set(records.segment, SEG); expected.set(records.companion, BASES);}
  if (fp) expected.set(fp, FP);
  if (calls.length) expected.set(calls.at(-1).packet, 100);
  for (const [address, bytes] of writes) {
    const pageIndex = pages.indexOf(address & ~4095); assert.notEqual(pageIndex, -1);
    expectedRam.set(bytes, pageIndex * 4096 + (address & 4095));
  }
  assert.equal(c.run(c.base, c.base + 56, budget, c.base + 96), 0, label + ' run status');
  const after = arena(c), actualCalls = c.calls.slice(at);
  assert.deepEqual(after, expected, `${c.owner}/${c.entries}/${label}: complete ${SIZE}-byte arena`);
  assert.equal(actualCalls.length, calls.length, label + ' helper count');
  calls.forEach((wanted, index) => {
    const actual = actualCalls[index];
    assert.equal(actual.name, wanted.name, label + ' helper name'); assert.equal(actual.status, 0, label + ' helper status');
    assert.deepEqual(actual.args.slice(-wanted.args.length), wanted.args, label + ' helper operands');
    assert.equal(actual.packet, wanted.packet.toString('hex'), label + ' helper packet');
  });
  const afterRam = ram(c); assert.deepEqual(afterRam, expectedRam, label + ' every mapped RAM byte');
  const ordinal = observations.length + 1, arenaFile = `arena-${ordinal}.bin`, ramFile = `ram-${ordinal}.bin`;
  writeFileSync(join(output, arenaFile), after, {flag: 'wx'}); writeFileSync(join(output, ramFile), afterRam, {flag: 'wx'});
  observations.push({context: c.id, label, budget, retired, reason, fault, cpu: state, helpers: actualCalls,
    arena: {file: arenaFile, bytes: SIZE, sha256: hash(after)}, ram: {file: ramFile, pages, bytes: afterRam.length, sha256: hash(afterRam)}});
}
function loadCall(c, target, base) {return {name: c.owner === 'resident' ? 'segment_load_resident' : 'segment_load', args: [target, 0x43], packet: packet([0, base, 0, 0, 0, 0], 7)};}
const readCall = (address, value) => ({name: 'read32', args: [address], packet: packet([0, value, 0, 0, 0, 0])});
function storeCall(c, address, value) {return {name: c.owner === 'resident' ? 'store_resident32' : 'store32', args: [address, value], packet: packet([0, 0, 0, 0, 0, 0])};}
function hostLoad(c, target, base, version = 2) {
  const before = arena(c), records = loadedRecords(target, base, 0xff, version), expected = Buffer.from(before);
  expected.set(records.segment, SEG); expected.set(records.companion, BASES); expected.set(packet([0, base, 0, 0, 0, 0], 7), 100);
  host(c, c.owner === 'resident' ? 'segment_load_resident' : 'segment_load', [...c.binding, target, 0x43]);
  assert.deepEqual(arena(c), expected, 'host load publishes literal loaded cache/base records');
}
function malformedControls(c) {
  const valid = arena(c), beforeRam = ram(c), at = c.calls.length;
  const mutations = [
    ['segment-profile', SEG + 6, 2, 2], ['segment-size', SEG + 8, 63, 4], ['segment-reserved', SEG + 12, 1, 4],
    ['segment-FS-reserved', SEG + 48, 1, 4], ['segment-GS-reserved', SEG + 52, 1, 4], ['segment-tail-reserved', SEG + 60, 1, 4],
    ['CS-D32-required', SEG + 32, 0xf1bfffff, 4], ['SS-B32-required', SEG + 36, 0xf13fffff, 4],
    ['companion-magic', BASES, 0, 4], ['companion-version', BASES + 4, 2, 2],
    ['companion-profile', BASES + 6, 2, 2], ['companion-size', BASES + 8, 39, 4],
    ['companion-reserved', BASES + 12, 1, 4], ['CS-nonflat', BASES + 20, 1, 4],
  ];
  for (const [label, offset, value, width] of mutations) {
    const malformed = Buffer.from(valid); width === 2 ? malformed.writeUInt16LE(value, offset) : malformed.writeUInt32LE(value >>> 0, offset);
    input(c, 0, malformed); assert.equal(c.run(c.base, c.base + 56, 1, c.base + 96), 2, label);
    assert.deepEqual(arena(c), malformed, label + ' neutral complete arena'); assert.equal(c.calls.length, at, label + ' no helper');
    refusals.push({context: c.id, label, status: 2, offset, value});
  }
  input(c, 0, valid); assert.deepEqual(ram(c), beforeRam, 'malformed v3 records keep all mapped RAM');
  for (const [label, exit, cancel] of [
    ['exit-overlaps-companion', c.base + BASES, c.base + 96],
    ['cancel-overlaps-companion', c.base + 56, c.base + BASES],
  ]) {
    const before = Buffer.from(new Uint8Array(c.memory.buffer));
    assert.equal(c.detachedPreflight(c.base, exit, 1, cancel), 1, label);
    assert.deepEqual(Buffer.from(new Uint8Array(c.memory.buffer)), before, label + ' neutral linear memory');
    assert.equal(c.calls.length, at, label + ' no helper');
    refusals.push({context: c.id, label, status: 1});
  }
  c.memory.grow(1);
  const pointer = c.memory.buffer.byteLength - BASES, linear = new Uint8Array(c.memory.buffer);
  linear.set(valid.subarray(0, BASES), pointer);
  let before = Buffer.from(linear);
  assert.equal(c.detachedPreflight(pointer, pointer + 56, 1, pointer + 96), 1, 'missing v3 companion tail');
  assert.deepEqual(Buffer.from(new Uint8Array(c.memory.buffer)), before, 'missing companion is neutral in all linear memory');
  assert.equal(c.calls.length, at, 'missing companion invokes no helper');
  refusals.push({context: c.id, label: 'missing-v3-companion-tail', status: 1});
  for (const version of [1, 2]) {
    const prefix = Buffer.from(valid.subarray(0, BASES)); prefix.set(segmentRecords({version}).segment, SEG); linear.set(prefix, pointer);
    before = Buffer.from(new Uint8Array(c.memory.buffer));
    const expected = Buffer.from(before); expected.set(record('R3EX', 40, [1, 0, 0, 0, 0, 0], c.exitVersion), pointer + 56);
    assert.equal(c.detachedPreflight(pointer, pointer + 56, 0, pointer + 96), 0, 'legacy missing companion v' + version);
    assert.deepEqual(Buffer.from(new Uint8Array(c.memory.buffer)), expected, 'legacy missing tail accepts only budget exit');
    assert.equal(c.calls.length, at, 'legacy budget invokes no helper');
  }
  assert.deepEqual(arena(c), valid, 'detached range controls preserve the engine arena');
  assert.deepEqual(ram(c), beforeRam, 'missing-tail controls keep all mapped RAM');
}

for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  let c = open(owner, entries, [0x8e, 0xd8, 0x8b, 0x0b, 0x89, 0x17], 'live DS scalar read/store');
  configure(c, 0x5000); upload(c, 0x5010, words([0x89abcdef])); upload(c, 0x5020, words([0x55667788]));
  let s = seed(c);
  run(c, 'MOV DS immediately changes following scalar read and store', next(s, PC + 6, {1: 0x89abcdef}), {
    budget: 3, records: loadedRecords(3, 0x5000), calls: [loadCall(c, 3, 0x5000), readCall(0x5010, 0x89abcdef), storeCall(c, 0x5020, s.r[2])], writes: [[0x5020, words([s.r[2]])]],
  }); host(c, 'close');

  c = open(owner, entries, [0x8e, 0xc0, 0xa5], 'live ES string destination');
  configure(c, 0x6000); upload(c, 0x5010, words([0x76543210])); upload(c, 0x6020, words([0x55667788])); s = seed(c, {1: 3});
  run(c, 'MOV ES immediately changes MOVSD destination', next(s, PC + 3, {6: 0x500c, 7: 0x1c}), {
    budget: 2, records: loadedRecords(0, 0x6000), calls: [loadCall(c, 0, 0x6000), readCall(0x5010, 0x76543210), storeCall(c, 0x6020, 0x76543210)], writes: [[0x6020, words([0x76543210])]],
  }); host(c, 'close');

  c = open(owner, entries, [0x8e, 0xd8, 0xdb, 0x2b], 'x87 numeric helper selected DS');
  configure(c, 0x5000); upload(c, 0x5014, ONE_PLUS); s = seed(c, {3: 0x14});
  const fp = record('R3FP', 128); fp.writeUInt16LE(0x37f, 16); fp.writeUInt16LE(0x3800, 18); fp.writeUInt16LE(0x3fff, 20);
  fp.writeUInt16LE(0x32b, 22); fp.writeUInt32LE(PC + 2, 24); fp.writeUInt32LE(0x14, 28); fp.writeUInt16LE(0x1b, 32); fp.writeUInt16LE(0x43, 34); fp.set(ONE_PLUS, 110);
  run(c, 'FLD80 reads translated DS and records logical pointer', next(s, PC + 4), {budget: 2, records: loadedRecords(3, 0x5000), fp,
    calls: [loadCall(c, 3, 0x5000), {name: owner === 'resident' ? 'x87_resident_op' : 'x87_op', args: [0x5014, s.flags, PC + 2, 0x32b, 0x401], packet: packet([0, s.flags, 0, 0, 0, 10], 7)}],
  }); host(c, 'close');

  c = open(owner, entries, [0x8e, 0xd8, 0x8b, 0x0b], 'wrapped DS base');
  configure(c, 0xfffffff0, 0xffff); upload(c, 0x5010, words([0x10203040])); upload(c, 0x5020, words([0xdeadbeef])); s = seed(c, {3: 0x5020});
  run(c, 'base addition wraps after safe logical span validation', next(s, PC + 4, {1: 0x10203040}), {budget: 2, records: loadedRecords(3, 0xfffffff0, 0xffff), calls: [loadCall(c, 3, 0xfffffff0), readCall(0x5010, 0x10203040)]}); host(c, 'close');

  c = open(owner, entries, [0x8e, 0xd8, 0x8b, 0x0b], 'logical limit before page fault');
  configure(c, 0x5000); s = seed(c, {3: 0x2000});
  run(c, 'logical limit GP precedes unmapped translated page', next(s, PC + 2), {budget: 2, retired: 1, reason: 11, records: loadedRecords(3, 0x5000), calls: [loadCall(c, 3, 0x5000)]}); host(c, 'close');

  c = open(owner, entries, [0x8e, 0xd8, 0x8b, 0x0b], 'mutable table does not change loaded cache');
  configure(c, 0x5000); upload(c, 0x5010, words([0x13579bdf])); upload(c, 0x6010, words([0x2468ace0])); s = seed(c);
  run(c, 'MOV DS freezes base and limit in loaded cache', next(s, PC + 2), {records: loadedRecords(3, 0x5000), calls: [loadCall(c, 3, 0x5000)]});
  const loaded = arena(c); configure(c, 0x6000, 0xf); assert.deepEqual(arena(c), loaded);
  run(c, 'table base and limit mutation leaves next access unchanged', next(s, PC + 4, {1: 0x13579bdf}), {calls: [readCall(0x5010, 0x13579bdf)]}); host(c, 'close');

  c = open(owner, entries, [0x64, 0x8b, 0x0b], 'v1 FS ignores stale companion');
  host(c, 'set_segment_base', [4, 0x5000]);
  const fs = segmentRecords({version: 1, bases: [0, 0, 0, 0, 0x5000, 0]}); assert.deepEqual(arena(c).subarray(SEG, SEG + 64), fs.segment);
  input(c, BASES, Buffer.alloc(40, 0xa5)); upload(c, 0x5010, words([0xcafebabe])); s = seed(c);
  run(c, 'legacy v1 FS base ignores malformed companion bytes', next(s, PC + 3, {1: 0xcafebabe}), {calls: [readCall(0x5010, 0xcafebabe)]}); host(c, 'close');

  c = open(owner, entries, [0x65, 0x8b, 0x0b], 'v2 GS ignores stale companion');
  configure(c, 0x6000); hostLoad(c, 5, 0x6000); input(c, BASES, Buffer.alloc(40, 0x5a)); upload(c, 0x6010, words([0x0badf00d])); s = seed(c);
  run(c, 'legacy v2 GS cache and base ignore malformed companion', next(s, PC + 3, {1: 0x0badf00d}), {calls: [readCall(0x6010, 0x0badf00d)]}); host(c, 'close');

  c = open(owner, entries, [0x8b, 0x0b], 'v3 malformed and scheduling controls');
  const current = segmentRecords({bases: [0, 0, 0, 0x5000, 0, 0]}); current.segment.writeUInt32LE(0xf23000ff, 40); install(c, current);
  s = seed(c, {3: 0x2000});
  run(c, 'budget zero precedes v3 logical and page access', s, {budget: 0, retired: 0});
  input(c, 96, words([1])); run(c, 'cancel precedes v3 logical and page access', s, {budget: 1, retired: 0, reason: 2}); input(c, 96, words([0]));
  malformedControls(c); host(c, 'close');
}
assert.equal(contexts.length, 36); assert.equal(modules.length, 36); assert.equal(observations.length, 44); assert.equal(refusals.length, 68);
assert.deepEqual(pins(), sourcePins, 'actual segment base source and literal oracles stayed fixed');
writeFileSync(join(output, 'result.json'), JSON.stringify({status: 'ok', engine: {path: enginePath, sha256: engine.sha256, bytes: engine.bytes.length},
  source_pins: sourcePins, arena_bytes: SIZE, mapped_ram_pages: pages, contexts, modules, observations, refusals, helper_trace: helperTrace,
  limits: ['finite authored x86 data-segment integration in Node WebAssembly, four replacement/resident extent/entry profiles',
    'CS remains base zero/D32; this data-base fixture does not claim SS-base semantics; no broader descriptor, privilege, browser, performance, or game compatibility claim',
    'legacy v1/v2 ignore malformed and missing companion bytes; v3 tail absence is a neutral ABI range refusal, not an old-engine semantic RED',
    'detached missing-tail controls reuse the actual emitted module with a neutral guard stub to reach range preflight independently of canonical engine pointer admission'],
}, null, 2) + '\n');
console.log(JSON.stringify({status: 'ok', contexts: contexts.length, modules: modules.length, runs: observations.length, refusals: refusals.length, output}));
