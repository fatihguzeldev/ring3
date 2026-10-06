import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, readdirSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(root, 'expected actual engine Wasm, output directory and repository root');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const engineBytes = readFileSync(enginePath), engineModule = new WebAssembly.Module(engineBytes);
assert.equal(hash(engineBytes), '135c18d6c4967df63130aad44e7dd42d0d2f82b6f08c8913572772a41472fc4f');
assert.deepEqual(WebAssembly.Module.imports(engineModule), []);
const tools = {node: process.version, v8: process.versions.v8};
assert.deepEqual(tools, {node: 'v22.16.0', v8: '12.4.254.21-node.26'});
const SIZE = 4236, TRANSFER = 140, PC = 0x1000, BODY = 112, COLD = PC + BODY, BASIS = [2, 0xcd7];
const GPRS = ['EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI'];
const VECTORS = [
  [0x1234807f, 0x234501fe, 0x3456aa55, 0x45665aff, 0x56789abc, 0x6789abcd, 0x789abcde, 0x89abcdef],
  [0x89ab817e, 0x9abc02fd, 0xabcd56a9, 0xbcdeff00, 0xcdef1357, 0xdef02468, 0xef012345, 0xf0126789]
];
const ANCHORS = [
  [0, 0, 0, 0, 0x46, 0, 0, 0x46, 0],
  [0, 1, 1, 2, 2, 0, 0, 0x46, 0],
  [0, 0x7fffffff, 0x7fffffff, 0xfffffffe, 0x892, 0xffffff00, 0xfffffe00, 0x87, 0xfffffe00],
  [0, 0x80000000, 0x80000000, 0, 0x847, 1, 2, 2, 2],
  [0, 0xffffffff, 0xffffffff, 0xfffffffe, 0x93, 0xffffff01, 0xfffffe02, 0x83, 0xfffffe02],
  [1, 0, 0, 0, 0x46, 0, 0, 0x46, 0],
  [1, 0xffffffff, 1, 0, 0x57, 1, 0, 0x57, 1],
  [1, 0x7fffffff, 1, 0x80000000, 0x896, 0x80000000, 0xffffffff, 0x86, 0x80000000],
  [1, 0x80000000, 0x80000000, 0, 0x847, 1, 0x80000001, 0x82, 1],
  [1, 0xf, 1, 0x10, 0x12, 0, 0xf, 6, 0],
  [1, 1, 2, 3, 6, 0, 1, 2, 0]
];
function add(registers, destination, source, oldFlags) {
  const a = registers[destination], b = registers[source], wide = BigInt(a) + BigInt(b), result = Number(wide % 0x100000000n);
  const signed = value => value >= 0x80000000 ? value - 0x100000000 : value, sum = signed(a) + signed(b);
  let byte = result % 256, ones = 0; for (let bit = 0; bit < 8; bit++) {ones += byte % 2; byte = Math.floor(byte / 2);}
  const flags = (oldFlags & 0x400) + 2 + Number(wide > 0xffffffffn) + (ones % 2 === 0 ? 4 : 0)
    + (a % 16 + b % 16 > 15 ? 0x10 : 0) + (result === 0 ? 0x40 : 0) + (result >= 0x80000000 ? 0x80 : 0)
    + (sum < -0x80000000 || sum > 0x7fffffff ? 0x800 : 0);
  const next = [...registers]; next[source] = a; next[destination] = result; return {registers: next, flags};
}
function lowByte(registers, parent, value) {const next = [...registers]; next[parent] += value - next[parent] % 256; return next;}
const SOURCE_HASHES = [
  '132968a82674862e7c33f3a3ee44583ae3e6a511bff51d7f38d1ae6ef0536f90', '40c33208026261b1cb66f61b4a42fbe0568a852f2a068ec1cfa08bd1fabe5de3',
  '1481cc0b253fa59b013f4af8db544eaa2969b29b1bf43f7e79b17c91afea6d42', 'adf4092140ecf46c05d83cf1e57a255fa6b34d5b6f8e16e3db8156de8dc451e5',
  '32d1d7a880f144296d60967af56a82c483809961a22299a2e1f518a2713ff4b4', '93623a61b45a99cdf0ec5631b6e1bb70bc001fee37cfe26d8507b2bee0eb9c75',
  '2160664dad0e4bebebc8e89a19a90eefad3b6b9e279b8afa7cde3a95c86fa5c5', 'eab83198d516a0753e25cea280199df95ad0ad01d4fa1d4e7cd24e3feff0d1eb'
];
const PAGE_HASHES = [
  '1ba6a071382ff26fdb9f0b4c40b0b307602c06f1625c873087c81a177a6c29a8', 'fefcefaa61b736f1f81c3192371222033f2dc34265d7a693a1ed2e0ad5950946',
  'a3e644517da38a35d6abdfb779e47b5f59c39a851255954b1802bf39fbf03fa4', 'dfd92d2cfc646bdcbb9c619247c8a50b8c9b57b3fc4fc4802a8546929cbb9a55',
  '6132901df6a415a852aff7d01dc7a2116be740d0063e433217a7ae97a0186215', 'f509817e22547c13d4abfbe954deac8e50a3ecf8fd27048f19d7dc16dd1253b4',
  'd8eee79a448ed98b7dadf92b2f30db4c7e621a1ecca7b841197ce9bab1d6520b', 'd367d58027faba231b93aee0fa4e7d32d50c58eb9e4d66d4550dbfa538d4fba8'
];
const sources = Array.from({length: 8}, (_, bank) => {
  const bytes = Buffer.alloc(114);
  for (let source = 0; source < 8; source++) {
    const raw = 0xc0 + source * 8 + bank; bytes.set([0x0f, 0xc1, raw, 0x0f, 0x92, 0xc0, 0x0f, 0x90, 0xc2, 0x0f, 0xc1, raw, 0xeb, 112 - (source + 1) * 14], source * 14);
  }
  bytes.set([0x0f, 0x0b], BODY); assert.equal(hash(bytes), SOURCE_HASHES[bank]); assert.deepEqual(readFileSync(join(output, `bank-${bank}.x86`)), bytes); return bytes;
});
const currentSource = Buffer.from(sources[0]); currentSource[16] = 0xd9;
assert.equal(hash(currentSource), 'd178611512dc68a6b96776635c2f68b5b0abf5f876cf301411d6048d5276f317');
assert.ok(BASIS.every(flags => (flags & ~0xcd7) === 0 && (flags & 2) !== 0));
const expectedCounts = {contexts: 24, modules: 26, seeds: 834, main_seeds: 768, literal_seeds: 66, pair_chains: 834,
  first_xadds: 834, second_xadds: 834, xadds: 1668, setb: 834, seto: 834, jumps: 834, literal_first_anchor_checks: 66,
  literal_second_anchor_checks: 66, positive_retirements: 4170, generated_calls: 4372, preflight: 168, owner_controls: 68,
  same_byte_invalidations: 16, consumed_modrm_invalidations: 2, current_continuations: 2, data_only_controls: 2, maps: 32,
  host_uploads: 52, host_uploaded_bytes: 67380, standalone_page_inputs: 16, pages: 128, engine_page_rows: 96,
  standalone_page_rows: 32, diagnostic_read32_calls: 98304, arena_checks: 206600};
const counts = Object.fromEntries(Object.keys(expectedCounts).map(name => [name, 0]));
const contexts = [], modules = [], pageRows = [], mutations = [], dataControls = [], invalidations = [], hostInputs = [];
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
const tag = ctx => ({owner: ctx.owner, bank: ctx.bank, context: ctx.low});
function refresh(ctx) {ctx.bytes = new Uint8Array(ctx.memory.buffer); ctx.view = new DataView(ctx.memory.buffer); return ctx;}
function check(ctx, label) {refresh(ctx); assert.deepEqual(Buffer.from(ctx.bytes.subarray(ctx.base, ctx.base + SIZE)), ctx.expected, `${ctx.owner}/bank ${ctx.bank}/${label}: complete arena`); counts.arena_checks++;}
function pure(ctx, action, label, status = 0) {check(ctx, `before ${label}`); assert.equal(action(), status, label); check(ctx, label);}
function request(ctx, bytes) {refresh(ctx).bytes.set(bytes, ctx.base + TRANSFER); ctx.expected.set(bytes, TRANSFER); check(ctx, 'explicit host request');}
function upload(ctx, address, bytes, purpose) {
  request(ctx, bytes); pure(ctx, () => ctx.api.upload(address, bytes.length), 'explicit host upload'); counts.host_uploads++; counts.host_uploaded_bytes += bytes.length;
  hostInputs.push({kind: 'upload', ...tag(ctx), purpose, address, length: bytes.length, sha256: hash(bytes)});
}
function fresh(owner, bank) {
  const expected = Buffer.alloc(SIZE), ordinal = ++counts.contexts, codePage = Buffer.alloc(4096); codePage.set(sources[bank]); assert.equal(hash(codePage), PAGE_HASHES[bank]);
  expected.set(state(Array(8).fill(0), 0, 2)); expected.set(exit(1, 0), 56); expected.set(helper(0), 100);
  const ctx = {owner, bank, expected, codePage, dataPage: Buffer.from(pattern), low: ordinal, high: 0xc10f0000};
  if (owner === 'standalone') {
    ctx.memory = new WebAssembly.Memory({initial: 1}); ctx.base = 128; ctx.dataAddress = 0x8000; ctx.codeAddress = 0x6000;
    refresh(ctx).bytes.set(expected, ctx.base); ctx.bytes.set(ctx.dataPage, ctx.dataAddress); ctx.bytes.set(codePage, ctx.codeAddress); counts.standalone_page_inputs += 2;
    for (const [address, bytes] of [[ctx.dataAddress, ctx.dataPage], [ctx.codeAddress, codePage]]) hostInputs.push({kind: 'standalone_page', ...tag(ctx), address, length: bytes.length, sha256: hash(bytes)});
  } else {
    const instance = new WebAssembly.Instance(engineModule, {}), arities = {open: 3, close: 0, arena_ptr: 0, map: 3, upload: 2, read32: 1,
      compile_entries: 2, compile_resident: 1, generation: 0, module_ptr: 0, module_len: 0, guard: 6, guard_resident: 7};
    ctx.api = Object.fromEntries(Object.entries(arities).map(([name, arity]) => {
      const fn = instance.exports[`ring3_abi_v1_${name}`]; assert.equal(typeof fn, 'function', name); assert.equal(fn.length, arity, name); return [name, fn];
    }));
    ctx.memory = instance.exports.memory; assert.equal(ctx.api.open(3, ctx.low, ctx.high), 0);
    ctx.base = ctx.api.arena_ptr() >>> 0; ctx.dataAddress = 0x3000; ctx.codeAddress = PC; check(ctx, 'independently initialized arena');
    for (const [address, permissions, bytes, purpose] of [[ctx.dataAddress, 3, ctx.dataPage, 'initial_data'], [PC, 7, sources[bank], 'initial_code']]) {
      pure(ctx, () => ctx.api.map(address, 1, permissions), 'page map'); counts.maps++; hostInputs.push({kind: 'map', ...tag(ctx), address, pages: 1, permissions}); upload(ctx, address, bytes, purpose);
    }
  }
  ctx.expected.set(pattern, TRANSFER); refresh(ctx).bytes.set(pattern, ctx.base + TRANSFER); check(ctx, 'explicit patterned transfer');
  contexts.push({owner, bank, ordinal, arena_base: ctx.base, code_address: ctx.codeAddress, data_address: ctx.dataAddress,
    ...(ctx.api ? {key_low: ctx.low, key_high: ctx.high} : {})}); return ctx;
}
function compile(ctx, phase = 'initial') {
  let bytes, binding = {}; const file = `${ctx.owner}-${phase}-bank-${ctx.bank}.wasm`;
  if (ctx.owner === 'standalone') bytes = readFileSync(join(output, file));
  else {
    const entries = Array.from({length: 8}, (_, source) => PC + 14 * source);
    request(ctx, words(ctx.owner === 'replacement' ? entries : entries.flatMap(entry => [entry, 14])));
    if (ctx.owner === 'replacement') {
      pure(ctx, () => ctx.api.compile_entries(8, 0), 'eight-entry compiler');
      binding = {generation: ctx.api.generation(), pointer: ctx.api.module_ptr() >>> 0, length: ctx.api.module_len() >>> 0}; assert.ok(binding.generation > 0);
    } else {
      check(ctx, 'before eight-extent compiler'); assert.equal(ctx.api.compile_resident(8), 0); refresh(ctx);
      assert.equal(ctx.view.getUint32(ctx.base + TRANSFER, true), 1); assert.equal(ctx.view.getUint32(ctx.base + TRANSFER + 4, true), 24);
      binding = Object.fromEntries(['low', 'high', 'pointer', 'length'].map((name, index) => [name, ctx.view.getUint32(ctx.base + TRANSFER + 8 + index * 4, true)]));
      assert.ok(binding.low !== 0 || binding.high !== 0); ctx.expected.set(words([1, 24, binding.low, binding.high, binding.pointer, binding.length]), TRANSFER); check(ctx, 'resident metadata only with descriptor tail retained');
    }
    refresh(ctx); assert.ok(binding.pointer > 0 && binding.length > 8 && binding.length <= 65536 && binding.pointer + binding.length <= ctx.bytes.length);
    bytes = Buffer.from(ctx.bytes.subarray(binding.pointer, binding.pointer + binding.length)); writeFileSync(join(output, file), bytes, {flag: 'wx'});
  }
  assert.ok(bytes.length > 8 && bytes.length <= 65536); const module = new WebAssembly.Module(bytes), guards = ctx.owner === 'standalone' ? [] : [ctx.owner === 'replacement' ? 'guard' : 'guard_resident'];
  const declarations = [{module: 'env', name: 'memory', kind: 'memory'}, ...guards.map(name => ({module: 'ring3', name, kind: 'function'}))];
  assert.deepEqual(WebAssembly.Module.imports(module), declarations); assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  const child = new WebAssembly.Instance(module, {env: {memory: ctx.memory}, ...(ctx.api ? {ring3: ctx.api} : {})}); assert.equal(child.exports.run.length, 4); counts.modules++;
  modules.push({...tag(ctx), phase, file, source_sha256: hash(ctx.codePage.subarray(0, 114)), sha256: hash(bytes), length: bytes.length, imports: declarations,
    run_arity: child.exports.run.length, ...(ctx.api ? {key_low: ctx.low, key_high: ctx.high} : {}), ...binding}); return {phase, file, bytes, ...binding, run: child.exports.run};
}
function seed(ctx, registers, source, flags) {
  ctx.expected.set(state(registers, PC + source * 14, flags)); ctx.expected.set(exit(3, 0), 56); ctx.expected.writeUInt32LE(0, 96); ctx.expected.set(helper(0xdecafbad), 100);
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
function physical(ctx, unit) {refresh(ctx); const bytes = Buffer.from(ctx.bytes.subarray(unit.pointer, unit.pointer + unit.length)); assert.deepEqual(bytes, unit.bytes); return {pointer: unit.pointer, length: unit.length, sha256: hash(bytes)};}
function guard(ctx, unit, wrong) {
  const key = ctx.low ^ Number(wrong === 'key');
  return ctx.owner === 'replacement' ? ctx.api.guard(key, ctx.high, unit.generation + Number(wrong === 'identity'), ctx.base, ctx.base + 56, ctx.base + 96)
    : ctx.api.guard_resident(key, ctx.high, unit.low ^ Number(wrong === 'identity'), unit.high, ctx.base, ctx.base + 56, ctx.base + 96);
}
function mutateConsumed(ctx, unit, registers, flags) {
  assert.equal(ctx.bank, 0); assert.equal(ctx.expected.readUInt32LE(48), 0x1011); assert.equal(flags, 0x406);
  assert.deepEqual(registers, [3, 1, ...VECTORS[0].slice(2)]); assert.equal(ctx.codePage[16], 0xc8); assert.equal(ctx.dataPage[7], 8);
  const data = {...tag(ctx), address: ctx.dataAddress + 7, old_byte: 8, new_byte: 0xa5, upload_bytes: 1, before: cpuEvidence(ctx), identity: identity(ctx, unit), module: physical(ctx, unit)};
  upload(ctx, data.address, Buffer.from([0xa5]), 'data_only'); ctx.dataPage[7] = 0xa5;
  pure(ctx, () => guard(ctx, unit), 'correct owner after unrelated data upload'); counts.owner_controls++; counts.data_only_controls++;
  data.after = cpuEvidence(ctx); assert.deepEqual(data.after, data.before); assert.deepEqual(identity(ctx, unit), data.identity); assert.deepEqual(physical(ctx, unit), data.module);
  data.guard_status = 0; data.page_sha256 = hash(ctx.dataPage); assert.equal(data.page_sha256, '1c0e57497f7f02759fdd13d0ce4c2187808a9da9b0f22639eac1430002d7938d'); dataControls.push(data);
  const row = {...tag(ctx), address: PC + 16, byte_offset: 16, old_byte: 0xc8, new_byte: 0xd9, upload_bytes: 1,
    eip_before: 0x1011, flags, before: cpuEvidence(ctx), old_identity: identity(ctx, unit), old_module: physical(ctx, unit)};
  upload(ctx, row.address, Buffer.from([0xd9]), 'consumed_modrm'); ctx.codePage[16] = 0xd9;
  assert.deepEqual(ctx.codePage.subarray(0, 114), currentSource); assert.equal(hash(ctx.codePage), '038d3cfe153f966244314adb20bf09527068a2842ae395a747afb7f4a9bc30ec');
  row.source_sha256 = hash(currentSource); row.code_page_sha256 = hash(ctx.codePage); cancel(ctx, 1);
  run(ctx, unit, 0, 'consumed ModRM stale owner before cancelled malformed pointers', registers, 0x1011, flags, 0, 0, 4, true);
  counts.owner_controls++; counts.consumed_modrm_invalidations++; row.stale_status = 4; cancel(ctx, 0); row.cancel_before_continuation = 0;
  const current = compile(ctx, 'current'); assert.notDeepEqual(identity(ctx, current), row.old_identity); row.new_identity = identity(ctx, current); row.current_module = physical(ctx, current);
  assert.deepEqual(cpuEvidence(ctx), row.before); ctx.pendingMutation = row; return current;
}
function pages(ctx, label) {
  for (const [address, expected] of [[ctx.dataAddress, ctx.dataPage], [ctx.codeAddress, ctx.codePage]]) {
    const actual = Buffer.alloc(4096);
    if (ctx.owner === 'standalone') {actual.set(refresh(ctx).bytes.subarray(address, address + 4096)); counts.standalone_page_rows++;}
    else {
      for (let offset = 0; offset < 4096; offset += 4) {
        check(ctx, 'before separate diagnostic Read32'); assert.equal(ctx.api.read32(address + offset), 0); refresh(ctx);
        actual.writeUInt32LE(ctx.view.getUint32(ctx.base + 120, true), offset); ctx.expected.set(helper(expected.readUInt32LE(offset)), 100); check(ctx, 'diagnostic Read32 changes helper v1 only'); counts.diagnostic_read32_calls++;
      }
      counts.engine_page_rows++;
    }
    assert.deepEqual(actual, expected, 'complete declared page follows only explicit host inputs'); counts.pages++; pageRows.push({...tag(ctx), label, address, sha256: hash(actual)});
  }
}
function anchor(registers, flags, row, initialFlags, second) {
  const [source, a, , firstResult, firstFlags, , secondResult, secondFlags, secondSource] = row;
  const expected = [...VECTORS[0]]; expected[0] = second ? secondResult : firstResult;
  if (source === 1) expected[1] = second ? secondSource : a;
  if (second) expected[2] = 0x3456aa00 + Number((firstFlags & 0x800) !== 0);
  assert.deepEqual(registers, expected, 'literal full old-source or current-second/self-priority state'); assert.equal(flags, (second ? secondFlags : firstFlags) + (initialFlags & 0x400));
  counts[second ? 'literal_second_anchor_checks' : 'literal_first_anchor_checks']++;
}
function chain(ctx, initialUnit, initial, source, initialFlags, literal = null, mutation = false) {
  let unit = initialUnit, registers = [...initial], flags = initialFlags; const entry = PC + source * 14;
  seed(ctx, registers, source, flags); counts[literal ? 'literal_seeds' : 'main_seeds']++; counts.pair_chains++;
  if (!ctx.preflightDone && literal === null) {
    run(ctx, unit, 0, 'zero budget before first target', registers, entry, flags, 1, 0); counts.preflight++;
    cancel(ctx, 1); run(ctx, unit, 1, 'cancel before first target', registers, entry, flags, 2, 0); counts.preflight++; cancel(ctx, 0); ctx.preflightDone = true;
  }
  ({registers, flags} = add(registers, ctx.bank, source, flags));
  run(ctx, unit, 1, 'first XADD full captured source and destination', registers, entry + 3, flags); counts.first_xadds++; counts.xadds++; counts.positive_retirements++;
  if (literal) anchor(registers, flags, literal, initialFlags, false);
  if (mutation) unit = mutateConsumed(ctx, unit, registers, flags);
  registers = lowByte(registers, 0, flags % 2); run(ctx, unit, 1, 'live SETB AL full CPU', registers, entry + 6, flags); counts.setb++; counts.positive_retirements++;
  if (ctx.pendingMutation) {
    assert.equal(entry, 0x100e); assert.deepEqual(registers, [0, 1, ...VECTORS[0].slice(2)]); assert.equal(flags, 0x406);
    ctx.pendingMutation.eip_after = entry + 6; ctx.pendingMutation.retired = 1; ctx.pendingMutation.after = cpuEvidence(ctx); mutations.push(ctx.pendingMutation); delete ctx.pendingMutation; counts.current_continuations++;
  }
  registers = lowByte(registers, 2, Number((flags & 0x800) !== 0)); run(ctx, unit, 1, 'live SETO DL full CPU', registers, entry + 9, flags); counts.seto++; counts.positive_retirements++;
  if (literal) assert.equal(registers[0], literal[5], 'literal current AL parent before second XADD');
  const beforeSecond = state(registers, entry + 9, flags); ({registers, flags} = add(registers, ctx.bank, source, flags));
  run(ctx, unit, 1, 'second XADD from current operands after both consumers', registers, entry + 12, flags); counts.second_xadds++; counts.xadds++; counts.positive_retirements++;
  if (literal) {
    anchor(registers, flags, literal, initialFlags, true);
    if (literal === ANCHORS[0] || literal === ANCHORS[5]) assert.deepEqual(state(registers, entry + 9, flags), beforeSecond, 'zero fixed-value second target retains GPRs and FLAGS while retiring');
  }
  run(ctx, unit, 1, 'following short JMP', registers, COLD, flags); counts.jumps++; counts.positive_retirements++; return unit;
}
function numeric(ctx, initialUnit) {
  let unit = initialUnit;
  for (const vector of VECTORS) for (const flags of BASIS) for (let source = 0; source < 8; source++) unit = chain(ctx, unit, vector, source, flags);
  if (ctx.bank === 0) for (const [index, literal] of ANCHORS.entries()) for (const flags of BASIS) {
    const [source, a, b] = literal, registers = [...VECTORS[0]]; registers[0] = a; if (source === 1) registers[1] = b;
    unit = chain(ctx, unit, registers, source, flags, literal, Boolean(ctx.api && index === 10 && flags === 0xcd7));
  }
  return unit;
}
function finish(ctx, unit) {
  const registers = Array.from({length: 8}, (_, index) => ctx.expected.readUInt32LE(16 + index * 4)), flags = ctx.expected.readUInt32LE(52);
  for (const [name, cancelled, budget, reason] of [['zero budget', 0, 0, 1], ['cancel with budget', 1, 1, 2], ['cancel with zero budget', 1, 0, 2]]) {
    cancel(ctx, cancelled); run(ctx, unit, budget, name, registers, COLD, flags, reason, 0); counts.preflight++;
  }
  cancel(ctx, 0); run(ctx, unit, 1, 'cold entry', registers, COLD, flags, 3, 0); counts.preflight++;
  run(ctx, unit, 0, 'malformed pointers', registers, COLD, flags, 0, 0, 1, true); counts.preflight++; pages(ctx, 'final');
  if (ctx.owner === 'standalone') return;
  for (const wrong of ['key', 'identity']) {pure(ctx, () => guard(ctx, unit, wrong), `wrong ${wrong}`, 3); counts.owner_controls++;}
  assert.equal(ctx.codePage[0], 0x0f); const row = {...tag(ctx), address: PC, old_byte: 0x0f, new_byte: 0x0f, upload_bytes: 1, before: cpuEvidence(ctx), identity: identity(ctx, unit), module: physical(ctx, unit)};
  upload(ctx, PC, Buffer.from([0x0f]), 'same_opcode'); cancel(ctx, 1);
  run(ctx, unit, 0, 'same-byte stale current owner before cancelled malformed pointers', registers, COLD, flags, 0, 0, 4, true); counts.owner_controls++; counts.same_byte_invalidations++;
  row.stale_status = 4; row.after_stale = cpuEvidence(ctx); assert.deepEqual(row.after_stale, row.before); pages(ctx, 'after_samebyte');
  pure(ctx, () => ctx.api.close(), 'close'); run(ctx, unit, 0, 'closed current owner before cancelled malformed pointers', registers, COLD, flags, 0, 0, 5, true);
  counts.owner_controls++; row.closed_status = 5; row.after_closed = cpuEvidence(ctx); invalidations.push(row);
}
for (const owner of ['standalone', 'replacement', 'resident']) for (let bank = 0; bank < 8; bank++) {
  const ctx = fresh(owner, bank), unit = compile(ctx); pages(ctx, 'initial'); const current = numeric(ctx, unit); finish(ctx, current);
}
assert.equal(Object.keys(counts).length, 31); assert.deepEqual(Object.keys(counts), Object.keys(expectedCounts));
for (const [name, expected] of Object.entries(expectedCounts)) assert.equal(counts[name], expected, `exact observed ${name} census`);
const roles = [];
for (const owner of ['standalone', 'replacement', 'resident']) for (let bank = 0; bank < 8; bank++) {
  roles.push([owner, bank, 'initial']); if (owner !== 'standalone' && bank === 0) roles.push([owner, bank, 'current']);
}
assert.deepEqual(modules.map(({owner, bank, phase}) => [owner, bank, phase]), roles);
for (const module of modules) {
  const matches = contexts.filter(ctx => ctx.owner === module.owner && ctx.bank === module.bank && ctx.ordinal === module.context);
  assert.equal(matches.length, 1, 'explicit unique module-context join'); const [ctx] = matches;
  if (module.owner !== 'standalone') assert.deepEqual([module.key_low, module.key_high], [ctx.key_low, ctx.key_high]);
  assert.equal(module.source_sha256, module.phase === 'current' ? hash(currentSource) : SOURCE_HASHES[module.bank]);
}
assert.equal(contexts.length, 24); assert.equal(mutations.length, 2); assert.equal(dataControls.length, 2); assert.equal(invalidations.length, 16); assert.equal(hostInputs.length, 100);
const artifactNames = [...sources.map((_, bank) => `bank-${bank}.x86`), ...modules.map(module => module.file), 'result.json'];
assert.equal(artifactNames.length, 35); assert.equal(new Set(artifactNames).size, 35);
const result = {status: 'ok', engine_sha256: hash(engineBytes), tools, counts, contexts, modules, pages: pageRows, data_only_controls: dataControls,
  consumed_modrm_mutations: mutations, code_invalidations: invalidations, host_inputs: hostInputs,
  source: {url: 'https://cdrdv2-public.intel.com/929356/334569-093-sdm-vol-2d.pdf', order: '334569-093US', edition: 'September 2026',
    pdf_path: 'target/p2-memory-binary-spec/334569-093-sdm-vol-2d.pdf', pdf_bytes: 1814965, pdf_sha256: '17b632da847e4757448e8afc950487b5d489da6c238de4551dc8cf6f799d3dcd',
    full_text_path: 'target/p2-memory-binary-spec/334569-093-sdm-vol-2d.txt', full_text_bytes: 721198, full_text_sha256: '1bdcfb8396e09e84cb008ed0bb07e08d38ca9d30e39b44db695f3ef43508013d',
    chapter_sha256: '407080e07d2686e251c4cd2e9cee8f000278b7ed1c1b3f8303d7d63c596e6f34', chapter_bytes: 3756, chapter_lines: [979, 1048],
    chapter_separator_policy: 'includes internal PDF PAGE 31 separator and excludes surrounding PAGE 30 / next PAGE 32 separators', pdf_pages_zero_based: [29, 30], printed_pages: ['6-27', '6-28'],
    page_bytes: [2478, 1266], page_lines: [[979, 1026], [1028, 1048]], page_separator_policy: 'exclude surrounding PDF PAGE separators',
    page_sha256: ['5efe959e67b3e3a4f30eb755eeb23028e088e668022ccb098d39ecba7c8b76d7', 'dc2c95631046ef0f1b7510c7592921f719d10ed910d91f4df47b7f1c1aa805f2']},
  oracle: {gprs: GPRS, vectors: VECTORS, basis_flags: BASIS, ordered_pairs: GPRS.flatMap((destination, bank) => GPRS.map((source, index) => ({destination, source, modrm: 0xc0 + 8 * index + bank}))),
    literal_anchor_fields: ['source_row', 'a', 'b', 'first_result', 'first_flags', 'current_eax', 'second_result', 'second_flags', 'second_source'], literal_anchors: ANCHORS, literal_anchor_count: 22,
    limits: {instructions: 64, blocks: 8, wasm_bytes: 65536}, bank: {specs: Array.from({length: 8}, (_, source) => [PC + source * 14, 14]), row_offsets: [0, 14, 28, 42, 56, 70, 84, 98],
      step_next_offsets: [3, 6, 9, 12], instructions: 40, body_bytes: BODY, source_bytes: 114, cold_target: COLD, jump_displacements: [98, 84, 70, 56, 42, 28, 14, 0]},
    programs: sources.map((bytes, bank) => ({bank, hex: bytes.toString('hex'), sha256: hash(bytes), code_page_sha256: PAGE_HASHES[bank]})),
    current_bank_0: {hex: currentSource.toString('hex'), sha256: hash(currentSource), code_page_sha256: '038d3cfe153f966244314adb20bf09527068a2842ae395a747afb7f4a9bc30ec'},
    arena_bytes: SIZE, pattern_sha256: hash(pattern), changed_data_sha256: '1c0e57497f7f02759fdd13d0ce4c2187808a9da9b0f22639eac1430002d7938d',
    classifications: {main_self_chains: 96, main_distinct_chains: 672, literal_self_chains: 30, literal_distinct_chains: 36, self_targets: 252, distinct_targets: 1416},
    fixed_points: {operand_value_fixed_first_observations: 12, full_gpr_flags_fixed_second_observations: 12},
    policy: 'widened unsigned BigInt sum and separate signed numeric-range overflow, nibble carry and eight-bit division parity; captured old destination goes to source before result goes to destination, including self final priority; numerical AL/DL insertion and current operand capture after both live consumers; no intermediate CPU repair; changed consumed C8 to D9 stays behind EIP 1011, fresh SETB continues at 1011 without replay',
    diagnostic_helper_policy: 'guest modules import no data helpers and retain R3MHv1; separate complete page Read32 diagnostics change only helper v1 success value with length/reserved zero, with before/after whole-arena checks'},
  artifact_census: {x86_banks: 8, modules: 26, result_files: 1, total: 35, names: artifactNames},
  test_sha256: Object.fromEntries(['engine/tests/cpu_xadd32.rs', 'engine/tests/cpu_xadd32_wasm.rs', 'engine/tests/fixtures/p2-xadd32/run.mjs'].map(path => [path, hash(readFileSync(join(root, path)))])),
  claim: 'prefix-free flat32 register-only wide XADD through native-generated standalone and actual engine-Wasm replacement/resident modules: all 64 ordered GPR pairs/eight self pairs, two declared asymmetric vectors and complementary FLAGS 2/cd7 plus eleven literal chains; independent addition/source/self/current-consumer oracle and 22 target anchors, full CPU before both SETcc and current second XADD, defined ADD flags with DF/fixed2 retained and incoming CF ignored; 834 seeds/1668 XADD/834 SETB/834 SETO/834 JMP, exact pure ABI/default caps/4236-byte arenas/128 current complete pages, two unrelated data-validity controls, two consumed-ModRM stale owners and fresh no-replay same-CPU 1011 continuations, sixteen same-opcode stale4/closed5 controls before cancelled malformed arguments; no exhaustive operand-FLAGS Cartesian, memory/atomic/LOCK/byte/word/prefix/x64/RTM/guest SMC/new helpers-ABI-local-caps/PE/Windows/SDK/browser/performance/fullCI/game claim'};
const resultBytes = Buffer.from(JSON.stringify(result, null, 2)); writeFileSync(join(output, 'result.json'), resultBytes, {flag: 'wx'});
assert.deepEqual(readdirSync(output).sort(), [...artifactNames].sort(), 'exact saved artifact filenames');
console.log(JSON.stringify({status: result.status, engine_sha256: result.engine_sha256, result_sha256: hash(resultBytes), counts, output}));
