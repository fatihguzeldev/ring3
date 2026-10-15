import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../../../../../../../../support/wasm/engine.mjs';
import {assertEngineExports} from '../../../../../../../../support/wasm/exports.mjs';
const [enginePath, output, root] = process.argv.slice(2), engine = readEngine(enginePath);
const probe = new WebAssembly.Instance(engine.module, {});
assert.equal(typeof probe.exports.ring3_abi_v1_cpu_xsave_avx, 'function', 'explicit segmented XSAVE2 dependency helper exists');
assert.equal(typeof probe.exports.ring3_abi_v1_cpu_xrstor_avx, 'function', 'explicit segmented XRSTOR2 dependency helper exists');
const receipt = assertEngineExports(engine.module, root), initial = readFileSync(join(output, 'initial-arena.bin'));
const SIZE = 4756, T = 140, FP = 4236, SV = 4364, SG = 4524, YU = 4612, BASE = 0x5000;
assert.equal(initial.length, SIZE);
const hash = value => createHash('sha256').update(value).digest('hex');
const sourcePaths = [...new Set([...receipt.sourcePaths,
  'engine/tests/support/wasm/engine.mjs', 'engine/tests/cpu/simd/state/xsave/upper/native.rs', 'engine/tests/cpu/simd/state/xsave/upper/wasm.rs',
  'engine/tests/cpu/simd/state/xsave/upper/fixtures/p2-xstate-upper/runtime/run.mjs',
  'engine/src/cpu/x86/capabilities.rs', 'engine/src/cpu/x86/xstate.rs', 'engine/src/cpu/x86/xstate/upper.rs',
  'engine/src/cpu/x86/xstate/upper/access.rs', 'engine/src/cpu/x86/xstate/legacy.rs', 'engine/src/abi/cpu_helper.rs',
  'engine/src/abi/x86/exit.rs', 'engine/src/abi/x86/mod.rs', 'engine/src/abi/header.rs', 'engine/src/abi/x86/ymm.rs', 'engine/src/abi/arena.rs', 'engine/src/process/cpu/xstate.rs',
  'engine/src/process/cpu/xstate/upper.rs', 'engine/src/cpu/x86/segments/cache.rs', 'engine/src/abi/x86/segments.rs', 'engine/src/process/wasm/mod.rs', 'engine/src/process/cpu/segments.rs', 'engine/src/process/wasm/xstate.rs',
  'engine/src/process/execution/authority.rs', 'engine/src/process/memory/span.rs', 'engine/src/memory/space.rs', 'engine/src/memory/space/atomic.rs',
  'engine/src/cpu/dbt/wasm/xstate.rs', 'engine/src/cpu/dbt/wasm/cpu_result.rs',
])];
const pins = () => Object.fromEntries(sourcePaths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sources = pins(), rows = [], frames = [], modules = [];
let identities = 0;
const record = (magic, version, size, fields = []) => {
  const b = Buffer.alloc(size); b.write(magic); b.writeUInt16LE(version, 4); b.writeUInt16LE(1, 6); b.writeUInt32LE(size, 8);
  fields.forEach((value, i) => b.writeUInt32LE(value >>> 0, 16 + i * 4)); return b;
};
const words = values => {const b = Buffer.alloc(values.length * 4); values.forEach((v, i) => b.writeUInt32LE(v >>> 0, i * 4)); return b;};
const packet = (fields = [0, 0, 0, 0, 0, 832], version = 8) => record('R3MH', version, 40, fields);
const exit = (reason, detail = 0) => record('R3EX', 9, 40, [reason, 0, detail, 0, 0, 0]);
const arena = c => Buffer.from(new Uint8Array(c.memory.buffer, c.base, SIZE));
const input = (c, at, bytes) => new Uint8Array(c.memory.buffer, c.base + at, bytes.length).set(bytes);
const host = (c, name, args) => assert.equal(c.api[name](...args), 0, name);
const upload = (c, at, bytes) => {for (let start = 0; start < bytes.length; start += 4096) {const part = bytes.subarray(start, start + 4096); input(c, T, part); host(c, 'upload', [at + start, part.length]);}};
function readRam(c, at, length) {
  const bytes = Buffer.alloc(length);
  for (let i = 0; i < length; i++) {host(c, 'read8', [at + i]); bytes[i] = new DataView(c.memory.buffer, c.base + 120, 4).getUint32(0, true);}
  return bytes;
}
const currentFp = Buffer.from(initial.subarray(FP, FP + 128)); currentFp.writeUInt16LE(0x027e, 16); currentFp.writeUInt16LE(0x8081, 18);
const restoredFp = Buffer.from(initial.subarray(FP, FP + 128)); restoredFp.writeUInt16LE(0x077f, 16);
const currentXmm = Buffer.from(Array.from({length: 128}, (_, i) => (i * 73 + 11) & 255));
const restoredXmm = Buffer.from(Array.from({length: 128}, (_, i) => (i * 29 + 0x35) & 255));
const currentUpper = Buffer.from(Array.from({length: 128}, (_, i) => (i * 97 + 0x79) & 255));
const restoredUpper = Buffer.from(Array.from({length: 128}, (_, i) => (i * 13 + 0x42) & 255));
function fullImage(present = 7) {
  const b = Buffer.alloc(832, 0x9b); b.fill(0, 0, 24); b.fill(0, 32, 160); b.writeUInt16LE(0x077f, 0);
  b.writeUInt32LE(0x5f80, 24); b.writeUInt32LE(0xffff, 28); restoredXmm.copy(b, 160); b.fill(0, 512, 536);
  b.writeBigUInt64LE(BigInt(present), 512); restoredUpper.copy(b, 576); return b;
}
function saveExpected(image, mask) {
  const b = Buffer.from(image);
  if (mask & 1) {b.fill(0, 0, 24); b.fill(0, 32, 160); b.writeUInt16LE(0x027e, 0); b.writeUInt16LE(0x8081, 2);}
  if (mask & 6) {b.writeUInt32LE(0xdfe5, 24); b.writeUInt32LE(0xffff, 28);}
  if (mask & 2) currentXmm.copy(b, 160);
  if (mask & 4) currentUpper.copy(b, 576);
  b.writeBigUInt64LE(image.readBigUInt64LE(512) | BigInt(mask), 512); return b;
}
function seed(c) {
  input(c, 0, initial); input(c, FP, currentFp); input(c, SV + 16, words([0xdfe5])); input(c, SV + 32, currentXmm); input(c, YU + 16, currentUpper);
  input(c, T, Buffer.from(Array.from({length: 4096}, (_, i) => (i * 17 + 0x63) % 255 + 1)));
}
function segments(changes = {}, bases = {}, nulls = []) {
  const b = Buffer.from(initial.subarray(SG, SG + 64)); b.writeUInt16LE(2, 4);
  const caches = [0xf33fffff, 0xf3bfffff, 0xf33fffff, 0xf33fffff, 0xf33fffff, 0], offsets = [28, 32, 36, 40, 44, 56];
  Object.entries(changes).forEach(([i, bits]) => caches[Number(i)] = bits >>> 0);
  nulls.forEach(i => {b.writeUInt16LE(0, 16 + i * 2); caches[i] = 0;});
  caches.forEach((bits, i) => b.writeUInt32LE(bits, offsets[i]));
  Object.entries(bases).forEach(([i, value]) => b.writeUInt32LE(value >>> 0, 32 + Number(i) * 4)); return b;
}
function fresh(owner, entries) {
  const instance = new WebAssembly.Instance(engine.module, {}), c = {owner, entries, id: ++identities, memory: instance.exports.memory};
  c.api = Object.fromEntries(receipt.functions.map(({name, arity}) => {assert.equal(instance.exports[name].length, arity, name); return [name.slice(13), instance.exports[name]];}));
  host(c, 'open', [4, c.id, 0x58555050]); c.base = c.api.arena_ptr() >>> 0; assert.equal(c.api.arena_len(), SIZE); assert.deepEqual(arena(c), initial);
  host(c, 'map', [0x1000, 1, 7]); host(c, 'map', [BASE, 2, 3]);
  upload(c, 0x1000, Buffer.from('0fae27ebfe', 'hex')); upload(c, 0x1100, Buffer.from('0f01d0ebfe', 'hex'));
  upload(c, 0x1200, Buffer.from('0fae2febfe', 'hex'));
  input(c, T, words(entries ? [0x1000, 0x1100, 0x1200] : [0x1000, 5, 0x1100, 5, 0x1200, 5]));
  const compile = owner === 'resident' ? (entries ? 'compile_resident_entries' : 'compile_resident') : (entries ? 'compile_entries' : 'compile');
  host(c, compile, entries ? [3, 0] : [3]);
  const after = arena(c); c.unit = owner === 'resident' ? [after.readUInt32LE(T + 8), after.readUInt32LE(T + 12)] : [];
  const pointer = owner === 'resident' ? after.readUInt32LE(T + 16) : c.api.module_ptr(), length = owner === 'resident' ? after.readUInt32LE(T + 20) : c.api.module_len();
  const bytes = Buffer.from(new Uint8Array(c.memory.buffer, pointer >>> 0, length >>> 0)), module = new WebAssembly.Module(bytes), imports = {};
  for (const imported of WebAssembly.Module.imports(module)) if (imported.kind === 'function') {
    assert.equal(imported.module, 'ring3'); assert.equal(typeof c.api[imported.name], 'function');
    imports[imported.name] = (...args) => {
      if (imported.name.startsWith('cpu_x')) {
        assert.deepEqual(args.slice(-4).map(value => value >>> 0), c.request, 'generated segment/logicaloffset/live mask inputs');
        c.calls++;
      }
      if (c.synthetic && imported.name.startsWith('cpu_xsave')) {input(c, 100, packet([0, 0, 0, 0, 0, 576], 7)); return 0;}
      return c.api[imported.name](...args);
    };
  }
  assert.deepEqual(WebAssembly.Module.imports(module).map(value => `${value.module}:${value.name}:${value.kind}`).sort(),
    ['env:memory:memory', `ring3:${owner === 'resident' ? 'guard_resident' : 'guard'}:function`,
      ...['xsave', 'xrstor'].map(name => `ring3:cpu_${name}_avx${owner === 'resident' ? '_resident' : ''}:function`)].sort());
  assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  c.run = new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3: imports}).exports.run;
  modules.push({context: c.id, sha256: hash(bytes), bytes: bytes.length, imports: WebAssembly.Module.imports(module)}); writeFileSync(join(output, `module-${c.id}.wasm`), bytes); return c;
}
function invoke(c, row) {
  const name = `cpu_${row.operation}${row.old ? '' : '_avx'}${c.owner === 'resident' ? '_resident' : ''}`;
  const operands = [...(row.old ? [] : [row.segment ?? 3]), row.offset ?? row.address ?? BASE, row.mask ?? 7, row.high ?? 0xffffffff];
  const args = c.owner === 'resident' ? [row.key ?? c.id, 0x58555050, ...c.unit, ...operands] : operands;
  return {name, args, run: () => c.api[name](...args)};
}
function execute(c, row) {
  const address = row.address ?? BASE, request = (row.mask ?? 7) & (row.old ? 3 : 7), image = row.image ?? fullImage(row.present ?? 7);
  const memory = Buffer.alloc(8192, 0xa5); image.copy(memory, address - BASE); upload(c, BASE, memory);
  if (row.protect) host(c, 'protect', row.protect);
  if (row.unmap) host(c, 'unmap', [0x6000, 1]);
  seed(c); if (row.context) input(c, SG, row.context); if (row.badUpper) input(c, YU + 12, words([1]));
  const before = arena(c), expected = Buffer.from(before), expectedRam = Buffer.from(memory);
  const fields = row.fields ?? [0, 0, 0, 0, 0, row.old ? 576 : 832];
  if (row.hostStatus === undefined) expected.set(packet(fields, row.old ? 7 : 8), 100);
  if (fields[0] === 0 && row.hostStatus === undefined) {
    if (row.operation === 'xsave') saveExpected(image, request).copy(expectedRam, address - BASE);
    else {
      const present = image.readBigUInt64LE(512);
      if (request & 1) expected.set(present & 1n ? restoredFp : initial.subarray(FP, FP + 128), FP);
      if (request & (row.old ? 2 : 6)) expected.writeUInt32LE(image.readUInt32LE(24), SV + 16);
      if (request & 2) expected.set(present & 2n ? restoredXmm : Buffer.alloc(128), SV + 32);
      if (request & 4) expected.set(present & 4n ? restoredUpper : Buffer.alloc(128), YU + 16);
    }
  }
  const call = invoke(c, row); assert.equal(call.run(), row.hostStatus ?? 0, `${row.name}: transport status`);
  const after = arena(c); assert.deepEqual(after, expected, `${row.name}: entire current arena`);
  frames.push(before, after); rows.push({context: c.id, owner: c.owner, entries: c.entries, name: row.name, api: call.name, args: call.args, status: row.hostStatus ?? 0, packet: after.subarray(100, 140).toString('hex'), frame_sha256: hash(after)});
  if (row.unmap) {host(c, 'map', [0x6000, 1, 3]); expectedRam.fill(0, 4096);}
  if (row.protect) host(c, 'protect', [row.protect[0], row.protect[1], 3]);
  assert.deepEqual(readRam(c, BASE, 8192), expectedRam, `${row.name}: whole RAM including reserved and unselected bytes`);
}
for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = fresh(owner, entries);
  for (let mask = 0; mask < 8; mask++) {
    const image = Buffer.alloc(832, 0x9b); image.writeBigUInt64LE(0x8000000000000040n, 512);
    execute(c, {name: `save-mask${mask}-pendingMF-reserved-tail`, operation: 'xsave', mask, image});
    for (let present = 0; present < 8; present++) execute(c, {name: `restore-mask${mask}-present${present}`, operation: 'xrstor', mask, present});
    for (const badOffset of [512, 519, 520, 527, 535]) {
      const image = fullImage(); image[badOffset] = badOffset === 512 ? 8 : 1;
      execute(c, {name: `restore-mask${mask}-invalid-header${badOffset}`, operation: 'xrstor', mask, image, fields: [3, 0, 0, 0, 0, 832]});
    }
    const badMxcsr = fullImage(0); badMxcsr.writeUInt32LE(0x10000, 24);
    execute(c, {name: `restore-mask${mask}-invalid-MXCSR-absent`, operation: 'xrstor', mask, image: badMxcsr, ...(mask & 6 ? {fields: [3, 0, 0, 0, 0, 832]} : {})});
  }
  for (const operation of ['xsave', 'xrstor']) {
    for (const segment of [3, 2]) {
      const fault = [segment === 2 ? 7 : 3, 0, 0, 0, 0, 832];
      execute(c, {name: `${operation}-segment${segment}-upper-end-over-limit`, operation, segment, mask: 4, context: segments({[segment]: 0xf23052be}), fields: fault});
      execute(c, {name: `${operation}-segment${segment}-exact-upper-end`, operation, segment, mask: 4, context: segments({[segment]: 0xf23052bf})});
      execute(c, {name: `${operation}-segment${segment}-limit-before-alignment`, operation, segment, offset: BASE + 1, mask: 4, context: segments({[segment]: 0xf23050ff}), fields: fault});
      execute(c, {name: `${operation}-segment${segment}-logical-overflow`, operation, segment, offset: 0xfffffe00, mask: 0, fields: fault});
    }
    execute(c, {name: `${operation}-FS-base-wrap`, operation, segment: 4, offset: 0x6000, context: segments({}, {4: 0xfffff000}), mask: 4});
    execute(c, {name: `${operation}-null-DS`, operation, context: segments({}, {}, [3]), fields: [3, 0, 0, 0, 0, 832]});
    execute(c, {name: `${operation}-linear-alignment`, operation, offset: 0x5001, fields: [3, 0, 0, 0, 0, 832]});
    execute(c, {name: `${operation}-late-upper-permission-neutral`, operation, address: 0x5dc0, protect: [0x6000, 1, operation === 'xsave' ? 1 : 2], fields: [1, 0, 2, 0x6000, operation === 'xsave' ? 2 : 1, 832]});
    execute(c, {name: `${operation}-late-upper-unmapped-neutral`, operation, address: 0x5dc0, unmap: true, fields: [1, 0, 1, 0x6000, operation === 'xsave' ? 2 : 1, 832]});
    execute(c, {name: `${operation}-malformed-upper-first`, operation, badUpper: true, offset: 1, hostStatus: 9});
    execute(c, {name: `${operation}-invalid-segment`, operation, segment: 6, hostStatus: 7});
    if (owner === 'resident') execute(c, {name: `${operation}-wrong-owner-first`, operation, segment: 6, key: c.id + 1, hostStatus: 3});
    execute(c, {name: `${operation}-legacy-mask4-remains-zero`, operation, old: true, mask: 4, present: 3});
  }
  execute(c, {name: 'restore-absent-upper-skips-unmapped-payload', operation: 'xrstor', mask: 4, present: 0, address: 0x5dc0, unmap: true});
  execute(c, {name: 'save-readonly-cache-empty-still-needs-write', operation: 'xsave', mask: 0, context: segments({3: 0xf31fffff}), fields: [3, 0, 0, 0, 0, 832]});
  execute(c, {name: 'restore-execute-only-CS', operation: 'xrstor', segment: 1, context: segments({1: 0xf39fffff}), fields: [3, 0, 0, 0, 0, 832]});
  seed(c); const registers = [0, 0, 0, 0, 0x7000, 0, 0, BASE]; input(c, 0, record('R3ST', 1, 56, [...registers, 0x1000, 0xcd7]));
  let before = arena(c), expected = Buffer.from(before); expected.set(packet([0, 0, 0, 0, 0, 576], 7), 100); expected.set(exit(7, 2), 56); c.synthetic = true; c.request = [3, BASE, 0, 0]; c.calls = 0;
  assert.equal(c.run(c.base, c.base + 56, 1, c.base + 96), 0); assert.deepEqual(arena(c), expected, 'active generated module rejects legacy packet7'); assert.equal(c.calls, 1);
  frames.push(before, arena(c)); rows.push({context: c.id, name: 'active-generated-rejects-legacy-packet', frame_sha256: hash(arena(c))}); c.synthetic = false;
  seed(c); input(c, 0, record('R3ST', 1, 56, [...registers, 0x1100, 0xcd7])); before = arena(c); expected = Buffer.from(before); registers[0] = 7; registers[2] = 0;
  expected.set(record('R3ST', 1, 56, [...registers, 0x1103, 0xcd7]), 0); expected.set(record('R3EX', 9, 40, [1, 1, 0, 0, 0, 0]), 56);
  c.calls = 0; assert.equal(c.run(c.base, c.base + 56, 1, c.base + 96), 0); assert.deepEqual(arena(c), expected, 'public XCR0 is7'); assert.equal(c.calls, 0, 'XGETBV has no helper transition'); frames.push(before, arena(c)); rows.push({context: c.id, name: 'XGETBV-enabled-mask7', frame_sha256: hash(arena(c))});
  for (const operation of ['xsave', 'xrstor']) for (let mask = 0; mask < 8; mask++) {
    const image = operation === 'xsave' ? Buffer.alloc(832, 0x9b) : fullImage(7);
    if (operation === 'xsave') image.writeBigUInt64LE(0x8000000000000040n, 512);
    const ram = Buffer.alloc(8192, 0xa5); image.copy(ram); upload(c, BASE, ram);
    seed(c); const pc = operation === 'xsave' ? 0x1000 : 0x1200;
    const gprs = [mask, 0, 0xffffffff, 0, 0x7000, 0, 0, BASE];
    input(c, 0, record('R3ST', 1, 56, [...gprs, pc, 0xcd7]));
    const before = arena(c), expected = Buffer.from(before), expectedRam = Buffer.from(ram);
    expected.set(record('R3ST', 1, 56, [...gprs, pc + 3, 0xcd7]), 0);
    expected.set(record('R3EX', 9, 40, [1, 1, 0, 0, 0, 0]), 56); expected.set(packet(), 100);
    if (operation === 'xsave') saveExpected(image, mask).copy(expectedRam);
    else {
      if (mask & 1) expected.set(restoredFp, FP);
      if (mask & 6) expected.writeUInt32LE(0x5f80, SV + 16);
      if (mask & 2) expected.set(restoredXmm, SV + 32);
      if (mask & 4) expected.set(restoredUpper, YU + 16);
    }
    c.request = [3, BASE, mask, 0xffffffff]; c.calls = 0;
    assert.equal(c.run(c.base, c.base + 56, 1, c.base + 96), 0);
    const after = arena(c); assert.deepEqual(after, expected, `generated-${operation}-mask${mask}: full arena`); assert.equal(c.calls, 1);
    frames.push(before, after); rows.push({context: c.id, name: `generated-${operation}-mask${mask}`, packet: after.subarray(100, 140).toString('hex'), frame_sha256: hash(after)});
    assert.deepEqual(readRam(c, BASE, 8192), expectedRam, `generated-${operation}-mask${mask}: full RAM including reserved704..831`);
  }

  seed(c); before = arena(c); expected = Buffer.from(before); expected.set(packet(), 100);
  const codeBefore = readRam(c, 0x1000, 4096); input(c, 0, before); const codeExpected = Buffer.from(codeBefore); saveExpected(codeBefore.subarray(0, 832), 7).copy(codeExpected);
  const call = invoke(c, {operation: 'xsave', offset: 0x1000, mask: 7}); assert.equal(call.run(), 11); assert.deepEqual(arena(c), expected, 'SMC after complete save');
  frames.push(before, arena(c)); rows.push({context: c.id, name: 'code-save-then-invalidated', api: call.name, args: call.args, status: 11, frame_sha256: hash(arena(c))}); assert.deepEqual(readRam(c, 0x1000, 4096), codeExpected);
  before = arena(c); assert.equal(invoke(c, {operation: 'xsave', mask: 4}).run(), 4); assert.deepEqual(arena(c), before, 'stale authority neutral'); rows.push({context: c.id, name: 'stale-authority-neutral'}); host(c, 'close', []);
}
assert.deepEqual(pins(), sources, 'all source pins stable'); writeFileSync(join(output, 'arenas.bin'), Buffer.concat(frames), {flag: 'wx'});
const result = {status: 'PASS', engine: {path: enginePath, sha256: engine.sha256}, counts: {contexts: identities, modules: modules.length, cases: rows.length, full_frame_pairs: frames.length / 2, frame_bytes: SIZE}, sources, modules, rows, limits: ['active guest XSAVE/XRSTOR use version8 segmented helpers and exit9; direct legacy576 helpers remain version7', 'IA32 exact128B payload with reserved704..831 preserved/ignored under permitted Ring3 policy', 'segment-before-page is deterministic Ring3 policy', 'active generated modules reject legacy packet7; frozen pre-activation proof retains old-module packet8 rejection', 'native tests separately prove memory-version exhaustion, pending/callback guards and single-version save']};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2), {flag: 'wx'}); console.log(JSON.stringify({status: result.status, ...result.counts, output}));
