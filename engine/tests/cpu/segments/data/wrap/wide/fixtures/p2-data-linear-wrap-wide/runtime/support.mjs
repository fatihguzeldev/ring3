import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync, existsSync} from 'node:fs';
import {join} from 'node:path';
import {pathToFileURL} from 'node:url';

export const SIZE = 4796, SEG = 4524, BASES = 4756, PC = 0x1000, KEY_HIGH = 0x44575241;
export const hash = bytes => createHash('sha256').update(bytes).digest('hex');
export const words = values => {const bytes = Buffer.alloc(values.length * 4); values.forEach((v, i) => bytes.writeUInt32LE(v >>> 0, i * 4)); return bytes;};
export function record(magic, size, fields = [], version = 1) {
  const bytes = Buffer.alloc(size); bytes.write(magic); bytes.writeUInt16LE(version, 4);
  bytes.writeUInt16LE(1, 6); bytes.writeUInt32LE(size, 8); bytes.set(words(fields), 16); return bytes;
}
export const registers = [0xa1b2c3d4, 0x23456789, 0x3456789a, 0x456789ab, 0x8000, 0x56789abc, 0x6789abcd, 0x789abcde];
export function setSegments(bytes, {mode = 'D32/B32', ssBase = 0, dsBase = 0, csLimit, ssLimit, dsLimit, nullDS = false, legacy = false} = {}) {
  const sg = record('R3SG', 64, [], legacy ? 1 : 3);
  [0x23, 0x1b, 0x23, nullDS ? 0 : 0x23, 0x3b, 0].forEach((v, i) => sg.writeUInt16LE(v, 16 + 2 * i));
  if (!legacy) [0xf33fffff,
    csLimit === undefined ? (mode.startsWith('D16') ? 0xf1bfffff : 0xf3bfffff) : (0xf2b00000 | csLimit),
    ssLimit === undefined ? (mode.endsWith('B16') ? 0xf13fffff : 0xf33fffff) : ((mode.endsWith('B16') ? 0xf0300000 : 0xf2300000) | ssLimit),
    nullDS ? 0 : dsLimit === undefined ? 0xf33fffff : (0xf2300000 | dsLimit), 0xf33fffff, 0]
    .forEach((v, i) => sg.writeUInt32LE(v >>> 0, [28, 32, 36, 40, 44, 56][i]));
  bytes.set(sg, SEG); bytes.set(legacy ? Buffer.alloc(40) : record('R3SB', 40, [0, 0, ssBase, dsBase, 0, 0]), BASES);
  return bytes;
}
export function state(initial, {pc = PC, flags = 2, regs = registers, cancel = 0, version = 1, shadow = 0, ...context} = {}) {
  const bytes = Buffer.from(initial), st = record('R3ST', 56, [...regs, pc, flags], version);
  st.writeUInt32LE(shadow, 12); bytes.set(st); bytes.writeUInt32LE(cancel, 96);
  // literal nonzero extended-state payloads make accidental numeric/segment publication visible.
  for (let i = 4276; i < 4356; i++) bytes[i] = (i * 17 + 29) & 255;
  for (let i = 4396; i < 4524; i++) bytes[i] = (i * 13 + 7) & 255;
  for (let i = 4628; i < 4756; i++) bytes[i] = (i * 19 + 11) & 255;
  return setSegments(bytes, context);
}
export function exit(bytes, reason, retired, version = 1, fields = [0, 0, 0, 0]) {
  bytes.set(record('R3EX', 40, [reason, retired, ...fields], version), 56); return bytes;
}
export function writeRAM(ram, address, bytes) {
  for (const [i, byte] of bytes.entries()) {const at = (address + i) >>> 0, page = (at & 0xfffff000) >>> 0;
    assert.ok(ram.has(page), 'literal oracle writes a mapped page'); ram.get(page)[at & 4095] = byte;}
}
export const cloneRAM = ram => new Map([...ram].map(([page, bytes]) => [page, Buffer.from(bytes)]));

export async function harness(enginePath, output, root) {
  assert.match(process.env.RING3_ENGINE_SHA256 ?? '', /^[a-f0-9]{64}$/, 'explicit selected engine SHA-256');
  const {readEngine} = await import(pathToFileURL(join(root, 'engine/tests/support/wasm/engine.mjs')));
  const engine = readEngine(enginePath), initial = readFileSync(join(output, 'initial-arena.bin'));
  assert.equal(initial.length, SIZE, 'public Rust-produced complete arena');
  const {assertEngineExports} = await import(pathToFileURL(join(root, 'engine/tests/support/wasm/exports.mjs')));
  const declarations = assertEngineExports(engine.module,root);
  const paths = [
    "engine/src/abi/arena.rs",
    "engine/src/abi/cpu_helper.rs",
    "engine/src/abi/memory_helper/cpu.rs",
    "engine/src/abi/x86/segments.rs",
    "engine/src/abi/x86/state.rs",
    "engine/src/abi/x86/exit.rs",
    "engine/src/cpu/dbt/wasm/emitter.rs",
    "engine/src/cpu/dbt/wasm/memory.rs",
    "engine/src/cpu/dbt/wasm/memory/cpu_scalar.rs",
    "engine/src/cpu/dbt/wasm/stack/pointer.rs",
    "engine/src/cpu/dbt/wasm/code_fetch.rs",
    "engine/src/cpu/dbt/wasm/abi.rs",
    "engine/src/cpu/x86/segments/cache.rs",
    "engine/src/process/memory/scalar.rs",
    "engine/src/process/memory/cpu_scalar.rs",
    "engine/src/process/memory/scalar/cpu_write.rs",
    "engine/src/process/wasm/memory/cpu_write.rs",
    "engine/src/process/execution/authority.rs",
    "engine/src/memory/space.rs",
    "engine/src/memory/space/cpu.rs",
    "engine/src/memory/space/cpu/read.rs",
    "engine/src/memory/space/cpu/write.rs",
    "engine/src/memory/space/cpu/atomic.rs",
    "engine/tests/support/wasm/engine.mjs",
    "engine/tests/support/wasm/exports.mjs",
    "engine/tests/support/wasm.rs",
    "engine/tests/cpu/segments/wasm.rs",
    "engine/tests/cpu/segments/data/wrap/wide/wasm.rs",
    "engine/tests/cpu/segments/data/wrap/wide/fixtures/p2-data-linear-wrap-wide/runtime/run.mjs",
    "engine/tests/cpu/segments/data/wrap/wide/fixtures/p2-data-linear-wrap-wide/runtime/support.mjs"
];
  paths.push(...declarations.sourcePaths.filter(path=>!paths.includes(path)));
  paths.push(...["engine/src/cpu/dbt/wasm/stack.rs", "engine/src/cpu/dbt/wasm/cpu_result.rs", "engine/src/cpu/x86/stack.rs", "engine/src/cpu/x86/stack/pointer.rs", "engine/src/process/cpu/stack.rs", "engine/src/process/memory/span.rs", "engine/src/cpu/dbt/wasm/simd.rs", "engine/src/cpu/dbt/wasm/simd/direct.rs", "engine/src/cpu/dbt/wasm/x87.rs", "engine/src/process/cpu/x87.rs", "engine/src/process/cpu/simd.rs", "engine/src/cpu/x86/x87/execute.rs", "engine/src/cpu/x86/x87/value.rs", "engine/src/cpu/x86/simd.rs", "engine/src/cpu/x86/decode/simd.rs", "engine/src/cpu/x86/decode/x87.rs", "engine/src/cpu/x86/xstate/legacy.rs", "engine/src/abi/x86/x87.rs", "engine/src/abi/x86/simd.rs", "engine/src/abi/x86/ymm.rs", "engine/src/abi/x86/system_registers.rs", "engine/src/cpu/dbt/wasm/system_registers.rs", "engine/src/process/cpu/system_registers.rs"].filter(path=>!paths.includes(path)));
  const missingSources = paths.filter(path => !existsSync(join(root,path)));
  const pins = () => ({...Object.fromEntries(paths.filter(path => existsSync(join(root,path))).map(path => [path, hash(readFileSync(join(root,path)))])), executed_support:hash(readFileSync(new URL(import.meta.url))), executed_runner:hash(readFileSync(new URL('./run.mjs',import.meta.url)))});
  const sourcePins = pins(), rows = [], modules = [], contexts = [];
  let nextKey = 0;
  const put = (c, offset, bytes) => new Uint8Array(c.memory.buffer).set(bytes, c.base + offset);
  const arena = c => Buffer.from(new Uint8Array(c.memory.buffer, c.base, SIZE));
  const host = (c, name, args = []) => assert.equal(c.api[name](...args), 0, `${c.label}: ${name}`);
  function open(resident, entries, label, pages) {
    const instance = new WebAssembly.Instance(engine.module, {});
    const c = {resident, entries, label, key: ++nextKey, memory: instance.exports.memory, calls: [], pages: [], permissions: new Map(), expectedRAM: new Map(), bankConfigured: false};
    c.api = Object.fromEntries(Object.entries(instance.exports).filter(([name, fn]) => name.startsWith('ring3_abi_v1_') && typeof fn === 'function').map(([name, fn]) => [name.slice(13), fn]));
    for (const declaration of declarations.functions) assert.equal(instance.exports[declaration.name].length, declaration.arity, declaration.name + ': arity');
    for(const width of [8,16,32]) assert.equal(c.api['cpu_read'+width]?.length,1,'explicit CPU scalar read '+width);
    host(c, 'open', [16, c.key, KEY_HIGH]); c.base = c.api.arena_ptr() >>> 0; put(c, 0, initial);
    for (const [page, permission = 7] of pages) {
      host(c, 'map', [page, 1, 7]); c.pages.push(page >>> 0); c.permissions.set(page >>> 0, permission); c.expectedRAM.set(page >>> 0, Buffer.alloc(4096, 0xa5));
      for (let at = 0; at < 4096; at += 512) upload(c, page + at, Buffer.alloc(512, 0xa5));
      if (permission !== 7) host(c, 'protect', [page, 1, permission]);
    }
    c.pages.sort((a, b) => a - b); contexts.push({key: c.key, resident, entries, label, pages: [...c.pages]}); return c;
  }
  function upload(c, address, bytes) {put(c, 140, bytes); host(c, 'upload', [address, bytes.length]); writeRAM(c.expectedRAM, address, bytes);}
  const literalRAM = c => cloneRAM(c.expectedRAM);
  function ram(c) {
    const saved = arena(c), result = new Map();
    for (const page of c.pages) {
      const permission = c.permissions.get(page), bytes = Buffer.alloc(4096);
      if (!(permission & 1)) host(c, 'protect', [page, 1, permission | 1]);
      for (let offset = 0; offset < 4096; offset += 512) {
        host(c, 'cpu_read', [(page + offset) >>> 0, 512, 1]); const observed = arena(c);
        assert.equal(observed.readUInt32LE(116), 0, 'diagnostic read packet succeeds');
        const allowed = Buffer.from(saved); allowed.set(observed.subarray(100, 652), 100);
        assert.deepEqual(observed, allowed, 'diagnostic read changes only packet and transfer');
        bytes.set(observed.subarray(140, 652), offset);
      }
      if (!(permission & 1)) host(c, 'protect', [page, 1, permission]);
      result.set(page, bytes);
    }
    put(c, 0, saved); return result;
  }
  function receipt(c, label, before, after, want, beforeRAM, afterRAM, wantRAM, errors, extra = {}) {
    const number = rows.length + 1, prefix = `row-${number}`;
    const files = {};
    for (const [suffix, bytes] of [['before', before], ['after', after], ['want', want]]) {
      const file = `${prefix}-${suffix}.bin`; writeFileSync(join(output, file), bytes, {flag: 'wx'});
      files[suffix] = {file, bytes: bytes.length, sha256: hash(bytes)};
    }
    const memory = [];
    for (const [page, bytes] of beforeRAM) {
      const record = {page};
      for (const [suffix, value] of [['before', bytes], ['after', afterRAM.get(page)], ['want', wantRAM.get(page)]]) {
        const file = `${prefix}-ram-${page.toString(16)}-${suffix}.bin`; writeFileSync(join(output, file), value, {flag: 'wx'});
        record[suffix] = {file, bytes: value.length, sha256: hash(value)};
      }
      memory.push(record);
    }
    const row = {context: c.key, profile: `${c.resident ? 'resident' : 'replacement'}-${c.entries ? 'entries' : 'extents'}`,
      label, authority_mode: 'confirmed CPU modulo32 bulk v7 bus', passed: errors.length === 0, errors, arena: files, memory, ...extra}; rows.push(row); return row;
  }
  function checkRAM(actual, want, errors) {for (const [page, bytes] of want) if (!actual.get(page).equals(bytes)) errors.push(`RAM ${page.toString(16)}`);}
  function capture(c, label, expectedHelpers = []) {
    const a = arena(c), binding = c.resident ? [c.key, KEY_HIGH, a.readUInt32LE(148), a.readUInt32LE(152)] : [];
    const pointer = c.resident ? a.readUInt32LE(156) : c.api.module_ptr() >>> 0;
    const length = c.resident ? a.readUInt32LE(160) : c.api.module_len() >>> 0;
    const bytes = Buffer.from(new Uint8Array(c.memory.buffer, pointer, length)), module = new WebAssembly.Module(bytes);
    const imports = WebAssembly.Module.imports(module), guard = c.resident ? 'guard_resident' : 'guard';
    assert.deepEqual(imports.map(imp => `${imp.module}:${imp.name}:${imp.kind}`).sort(),
      ['env:memory:memory', `ring3:${guard}:function`, ...expectedHelpers.map(name => `ring3:${name}:function`)].sort(), 'only the real declared helpers and guard');
    const ring3 = Object.fromEntries(imports.filter(imp => imp.kind === 'function').map(imp => [imp.name, (...args) => {
      const status = c.api[imp.name](...args);
      if(c.afterHelper?.name===imp.name){const hook=c.afterHelper;c.afterHelper=null;hook.run();}
      c.calls.push({name: imp.name, args: args.map(v => v >>> 0), status}); return status;
    }]));
    const material = {run: new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3}).exports.run,
      binding, pointer, length, generation: c.api.generation() >>> 0, helpers: expectedHelpers};
    const file = `module-${modules.length + 1}.wasm`; writeFileSync(join(output, file), bytes, {flag: 'wx'});
    modules.push({context: c.key, label, file, sha256: hash(bytes), bytes: length, binding, imports}); c.material = material; return material;
  }
  function compile(c, label, spans, helpers = [], {expectedStatus = 0, stopped = false, gates = []} = {}) {
    if (!stopped) put(c, 140, words([...(c.entries ? spans.map(([pc]) => pc) : spans.flat()), ...gates.flat()]));
    const before = arena(c), beforeRAM = ram(c), generation = c.api.generation() >>> 0;
    const oldPointer = c.api.module_ptr() >>> 0, oldLength = c.api.module_len() >>> 0;
    const name = stopped ? c.resident ? 'compile_stopped_resident_entry' : 'compile_stopped_entry'
      : c.resident ? c.entries ? 'compile_resident_entries' : gates.length ? 'compile_resident_with_gates' : 'compile_resident' : c.entries ? 'compile_entries' : gates.length ? 'compile_with_gates' : 'compile';
    const args = stopped ? [c.key, KEY_HIGH, ...(c.resident ? [] : [generation])] : [spans.length, ...(c.entries || gates.length ? [gates.length] : [])];
    const status = c.api[name](...args), after = arena(c), want = Buffer.from(before), errors = [];
    if (status !== expectedStatus) errors.push(`compile transport ${status}, expected ${expectedStatus}`);
    if (status === 0 && c.resident) {
      assert.equal(after.readUInt32LE(140), 1); assert.equal(after.readUInt32LE(144), 24);
      assert.notEqual(after.readBigUInt64LE(148), 0n);
      // only the ABI's explicit 24-byte module publication may depend on the allocator.
      want.set(after.subarray(140, 164), 140);
    }
    if (!after.equals(want)) errors.push('compile complete arena neutrality/publication');
    checkRAM(beforeRAM, c.expectedRAM, errors);
    if (expectedStatus !== 0 && ((c.api.generation() >>> 0) !== generation || (c.api.module_ptr() >>> 0) !== oldPointer || (c.api.module_len() >>> 0) !== oldLength)) errors.push('failed compile changed generation or replacement material');
    const afterRAM = ram(c); checkRAM(afterRAM, c.expectedRAM, errors);
    const row = receipt(c, label, before, after, want, beforeRAM, afterRAM, c.expectedRAM, errors,
      {kind: 'compile', expected_status: expectedStatus, status, classification: status === expectedStatus ? 'executed-host-contract' : 'compile-admission-or-capability', name, args});
    return status === 0 && expectedStatus === 0 ? capture(c, label, helpers) : null;
  }
  function observe(c, label, {input = arena(c), want, wantRAM, budget = 1, material = c.material, status = 0, helpers = [], kind = 'semantic'} = {}) {
    put(c, 0, input); const before = arena(c), beforeRAM = ram(c), desiredRAM = wantRAM ?? literalRAM(c), start = c.calls.length;
    const actualStatus = material.run(c.base, c.base + 56, budget, c.base + 96), after = arena(c), calls = c.calls.slice(start), errors = [];
    checkRAM(beforeRAM, c.expectedRAM, errors);
    if (actualStatus !== status) errors.push(`run transport ${actualStatus}, expected ${status}`);
    if (!after.equals(want)) errors.push('literal complete arena');
    const guard = {name: c.resident ? 'guard_resident' : 'guard',
      args: [...(c.resident ? material.binding : [c.key, KEY_HIGH, material.generation]), c.base, c.base + 56, c.base + 96],
      status: status === 4 ? 4 : 0};
    if (JSON.stringify(calls) !== JSON.stringify([guard, ...helpers])) errors.push('exact real guard/helper names/arguments/status');
    const afterRAM = ram(c); checkRAM(afterRAM, desiredRAM, errors); c.expectedRAM = cloneRAM(desiredRAM);
    return receipt(c, label, before, after, want, beforeRAM, afterRAM, desiredRAM, errors,
      {kind, status: actualStatus, expected_status: status, classification: actualStatus === 2 ? 'runtime-record-admission' : 'executed-module', expected_calls: [guard, ...helpers],
        expected_exit: {reason: want.readUInt32LE(72), retired: want.readUInt32LE(76), eip: want.readUInt32LE(48), flags: want.readUInt32LE(52)},
        actual_exit: {reason: after.readUInt32LE(72), retired: after.readUInt32LE(76), eip: after.readUInt32LE(48), flags: after.readUInt32LE(52)}, calls});
  }
  function request(c, label, name, args = [], {status = 0, want, wantRAM, classify = 'host-contract'} = {}) {
    const before = arena(c), beforeRAM = ram(c), generation = c.api.generation() >>> 0;
    const pointer = c.api.module_ptr() >>> 0, length = c.api.module_len() >>> 0;
    const actual = c.api[name](...args), after = arena(c), expected = want ?? before, desiredRAM = wantRAM ?? literalRAM(c), errors = [];
    checkRAM(beforeRAM, c.expectedRAM, errors);
    if (actual !== status) errors.push(`host transport ${actual}, expected ${status}`);
    if (!after.equals(expected)) errors.push('literal complete arena host publication/neutrality');
    if ((c.api.generation() >>> 0) !== generation || (c.api.module_ptr() >>> 0) !== pointer || (c.api.module_len() >>> 0) !== length) errors.push('host request changed executable material/generation');
    const afterRAM = ram(c); checkRAM(afterRAM, desiredRAM, errors); c.expectedRAM = cloneRAM(desiredRAM);
    return receipt(c, label, before, after, expected, beforeRAM, afterRAM, desiredRAM, errors,
      {kind: 'host', classification: classify, name, args, status: actual, expected_status: status});
  }
  function protect(c, page, permissions) {
    host(c, 'protect', [page, 1, permissions]); c.permissions.set(page, permissions);
  }
  function skip(c, label, prerequisite) {rows.push({context: c.key, label, passed: false, kind: 'unreached', classification: 'prerequisite-admission', prerequisite, errors: ['required earlier compile did not admit this context']});}
  function failure(label, error) {rows.push({label, passed: false, kind: 'setup-error', classification: 'fixture-setup', errors: [String(error.stack ?? error)]});}
  function finish(limits, expected = {}) {
    try {
      assert.deepEqual(pins(), sourcePins, 'source and independent literal oracles stayed fixed');
      if (rows.every(row => row.passed)) {
        assert.equal(missingSources.length,0,'successful proof pins all new source owners');
        if (expected.rows !== undefined) assert.equal(rows.length,expected.rows,'finite authored row inventory');
        if (expected.contexts !== undefined) assert.equal(contexts.length,expected.contexts,'finite context inventory');
        if (expected.modules !== undefined) assert.equal(modules.length,expected.modules,'finite module inventory');
        if (expected.moduleRuns !== undefined) assert.equal(rows.filter(row => row.classification === 'executed-module').length,expected.moduleRuns,'finite module-run inventory');
      }
    } catch (error) {failure('fixture inventory/source stability',error);}
    const counts = {passed: rows.filter(row => row.passed).length, failed: rows.filter(row => !row.passed && row.kind !== 'unreached').length,
      unreached: rows.filter(row => row.kind === 'unreached').length, executed_module: rows.filter(row => row.classification === 'executed-module').length,
      setup_errors: rows.filter(row => row.kind === 'setup-error').length,
      admission_failures: rows.filter(row => ['compile-admission-or-capability', 'runtime-record-admission'].includes(row.classification)).length};
    const result = {status: rows.every(row => row.passed) ? 'PASS' : 'FAIL', engine: {path: enginePath, sha256: engine.sha256, bytes: engine.bytes.length},
      initial_arena: {bytes: initial.length, sha256: hash(initial)}, planned_inventory: expected, source_pins: sourcePins, contexts, modules, counts, rows, limits};
    writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2) + '\n');
    console.log(JSON.stringify({status: result.status, counts, contexts: contexts.length, modules: modules.length, output}));
    process.exitCode = result.status === 'PASS' ? 0 : 1;
  }
  return {initial, engine, open, put, arena, host, upload, ram, literalRAM, compile, observe, request, protect, skip, failure, finish};
}
