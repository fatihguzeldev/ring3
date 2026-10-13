import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {pathToFileURL} from 'node:url';

export const SIZE = 4796, SEG = 4524, BASES = 4756, PC = 0x1000, KEY_HIGH = 0x43534241;
export const hash = bytes => createHash('sha256').update(bytes).digest('hex');
export const words = values => {const bytes = Buffer.alloc(values.length * 4); values.forEach((v, i) => bytes.writeUInt32LE(v >>> 0, i * 4)); return bytes;};
export function record(magic, size, fields = [], version = 1) {
  const bytes = Buffer.alloc(size); bytes.write(magic); bytes.writeUInt16LE(version, 4);
  bytes.writeUInt16LE(1, 6); bytes.writeUInt32LE(size, 8); bytes.set(words(fields), 16); return bytes;
}
export const registers = [0x12345678, 0x23456789, 0x3456789a, 0x456789ab, 0x7800, 0x56789abc, 0x6789abcd, 0x789abcde];
export function segments({bases = [0, 0, 0, 0, 0, 0], csLimit, dsLimit, selector = 0x1b} = {}) {
  const current = bases.some(value => value !== 0), cached = csLimit !== undefined || dsLimit !== undefined;
  const sg = record('R3SG', 64, [], current ? 3 : cached ? 2 : 1);
  [0x23, selector, 0x23, 0x23, 0x3b, 0].forEach((v, i) => sg.writeUInt16LE(v, 16 + 2 * i));
  if (current || cached) [0xf33fffff, csLimit === undefined ? 0xf3bfffff : (0xf2b00000 | csLimit), 0xf33fffff,
    dsLimit === undefined ? 0xf33fffff : (0xf2300000 | dsLimit), 0xf33fffff, 0]
    .forEach((v, i) => sg.writeUInt32LE(v >>> 0, [28, 32, 36, 40, 44, 56][i]));
  if (!current) {sg.writeUInt32LE(bases[4], 48); sg.writeUInt32LE(bases[5], 52);}
  const sb = current ? record('R3SB', 40, bases) : Buffer.alloc(40);
  return {sg, sb};
}
export function setSegments(bytes, context) {const {sg, sb} = segments(context); bytes.set(sg, SEG); bytes.set(sb, BASES); return bytes;}
export function state(initial, {pc = PC, flags = 0x10002, regs = registers, cancel = 0, ...context} = {}) {
  const bytes = Buffer.from(initial); bytes.set(record('R3ST', 56, [...regs, pc, flags], 3));
  bytes.writeUInt32LE(cancel, 96); return setSegments(bytes, context);
}
export function exit(bytes, reason, retired, version = reason >= 11 ? 6 : 1) {bytes.set(record('R3EX', 40, [reason, retired, 0, 0, 0, 0], version), 56); return bytes;}
export function descriptor(base) {
  return {low: (((base & 0xffff) * 65536) + 0xffff) >>> 0,
    high: ((base & 0xff000000) + ((base >>> 16) & 255) + 0x00cffa00) >>> 0};
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
  assert.equal(initial.length, SIZE, 'Rust-produced complete arena');
  const paths = [
    'engine/src/abi/arena.rs', 'engine/src/abi/x86/state.rs', 'engine/src/abi/x86/segments.rs',
    'engine/src/abi/x86/exit.rs', 'engine/src/abi/memory_helper.rs', 'engine/src/cpu/x86/decode/fetch.rs',
    'engine/src/cpu/x86/decode/decoder.rs', 'engine/src/cpu/x86/decode/mod.rs', 'engine/src/cpu/dbt/region.rs',
    'engine/src/cpu/dbt/cold.rs', 'engine/src/cpu/dbt/gate.rs', 'engine/src/cpu/dbt/artifact.rs',
    'engine/src/cpu/dbt/resident.rs', 'engine/src/cpu/dbt/wasm/code_fetch.rs', 'engine/src/cpu/dbt/wasm/emitter.rs',
    'engine/src/cpu/dbt/wasm/abi.rs', 'engine/src/cpu/dbt/wasm/memory.rs', 'engine/src/cpu/dbt/wasm/segments/context.rs',
    'engine/src/cpu/dbt/wasm/far.rs', 'engine/src/cpu/dbt/wasm/far/call.rs', 'engine/src/cpu/dbt/wasm/far/return.rs',
    'engine/src/cpu/dbt/wasm/far/interrupt.rs', 'engine/src/process/execution/authority.rs',
    'engine/src/process/execution/compiled.rs', 'engine/src/process/execution/resident.rs',
    'engine/src/process/execution/stopped.rs', 'engine/src/process/wasm/execution.rs',
    'engine/src/process/cpu/far.rs', 'engine/src/process/memory/scalar.rs', 'engine/src/process/memory/span.rs',
    'engine/tests/support/wasm.rs', 'engine/tests/support/wasm/engine.mjs', 'engine/tests/cpu/segments/wasm.rs',
    'engine/tests/cpu/segments/code/base/wasm.rs',
    'engine/tests/cpu/segments/code/base/fixtures/p2-cs-base/runtime/run.mjs',
    'engine/tests/cpu/segments/code/base/fixtures/p2-cs-base/runtime/support.mjs',
  ];
  const pins = () => Object.fromEntries(paths.map(path => [path, hash(readFileSync(join(root, path)))]));
  const sourcePins = pins(), rows = [], modules = [], contexts = [];
  let nextKey = 0;
  const put = (c, offset, bytes) => new Uint8Array(c.memory.buffer).set(bytes, c.base + offset);
  const arena = c => Buffer.from(new Uint8Array(c.memory.buffer, c.base, SIZE));
  const host = (c, name, args = []) => assert.equal(c.api[name](...args), 0, `${c.label}: ${name}`);
  function open(resident, entries, label, pages) {
    const instance = new WebAssembly.Instance(engine.module, {});
    const c = {resident, entries, label, key: ++nextKey, memory: instance.exports.memory, calls: [], pages: [], hook: null};
    c.api = Object.fromEntries(Object.entries(instance.exports).filter(([name, fn]) => name.startsWith('ring3_abi_v1_') && typeof fn === 'function').map(([name, fn]) => [name.slice(13), fn]));
    host(c, 'open', [16, c.key, KEY_HIGH]); c.base = c.api.arena_ptr() >>> 0; put(c, 0, initial);
    for (const [page, permission = 7] of pages) {host(c, 'map', [page, 1, permission]); c.pages.push(page >>> 0);}
    c.pages.sort((a, b) => a - b); contexts.push({key: c.key, resident, entries, label, pages: [...c.pages]}); return c;
  }
  function upload(c, address, bytes) {put(c, 140, bytes); host(c, 'upload', [address, bytes.length]);}
  function ram(c) {
    const saved = arena(c), result = new Map();
    for (const page of c.pages) {
      const bytes = Buffer.alloc(4096);
      for (let offset = 0; offset < 4096; offset += 512) {
        host(c, 'cpu_read', [(page + offset) >>> 0, 512, 1]); const observed = arena(c);
        assert.equal(observed.readUInt32LE(116), 0, 'diagnostic read packet succeeds');
        const allowed = Buffer.from(saved); allowed.set(observed.subarray(100, 652), 100);
        assert.deepEqual(observed, allowed, 'diagnostic read changes only packet and transfer');
        bytes.set(observed.subarray(140, 652), offset);
      }
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
      label, passed: errors.length === 0, errors, arena: files, memory, ...extra}; rows.push(row); return row;
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
      if (c.hook?.name === imp.name) {const hook = c.hook; c.hook = null; hook.run();}
      const status = c.api[imp.name](...args); c.calls.push({name: imp.name, args: args.map(v => v >>> 0), status}); return status;
    }]));
    const material = {run: new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3}).exports.run,
      binding, pointer, length, generation: c.api.generation() >>> 0, helpers: expectedHelpers};
    const file = `module-${modules.length + 1}.wasm`; writeFileSync(join(output, file), bytes, {flag: 'wx'});
    modules.push({context: c.key, label, file, sha256: hash(bytes), bytes: length, binding, imports}); c.material = material; return material;
  }
  function compile(c, label, spans, helpers = [], {expectedStatus = 0, stopped = false} = {}) {
    if (!stopped) put(c, 140, words(c.entries ? spans.map(([pc]) => pc) : spans.flat()));
    const before = arena(c), beforeRAM = ram(c), generation = c.api.generation() >>> 0;
    const name = stopped ? c.resident ? 'compile_stopped_resident_entry' : 'compile_stopped_entry'
      : c.resident ? c.entries ? 'compile_resident_entries' : 'compile_resident' : c.entries ? 'compile_entries' : 'compile';
    const args = stopped ? [c.key, KEY_HIGH, ...(c.resident ? [] : [generation])] : [spans.length, ...(c.entries ? [0] : [])];
    const status = c.api[name](...args), after = arena(c), want = Buffer.from(before), errors = [];
    if (status !== expectedStatus) errors.push(`compile transport ${status}, expected ${expectedStatus}`);
    if (status === 0 && c.resident) {
      assert.equal(after.readUInt32LE(140), 1); assert.equal(after.readUInt32LE(144), 24);
      assert.notEqual(after.readBigUInt64LE(148), 0n);
      // only the ABI's explicit 24-byte module publication may depend on the allocator.
      want.set(after.subarray(140, 164), 140);
    }
    if (!after.equals(want)) errors.push('compile complete arena neutrality/publication');
    if (expectedStatus !== 0 && c.api.generation() !== generation) errors.push('failed compile changed generation');
    const afterRAM = ram(c); checkRAM(afterRAM, beforeRAM, errors);
    const row = receipt(c, label, before, after, want, beforeRAM, afterRAM, beforeRAM, errors,
      {kind: 'compile', expected_status: expectedStatus, status, classification: status === expectedStatus ? 'executed-host-contract' : 'compile-admission-or-capability', name, args});
    return status === 0 && expectedStatus === 0 ? capture(c, label, helpers) : null;
  }
  function observe(c, label, {input = arena(c), want, wantRAM, budget = 1, material = c.material, status = 0, helpers = [], kind = 'semantic'} = {}) {
    put(c, 0, input); const before = arena(c), beforeRAM = ram(c), desiredRAM = wantRAM ?? cloneRAM(beforeRAM), start = c.calls.length;
    const actualStatus = material.run(c.base, c.base + 56, budget, c.base + 96), after = arena(c), calls = c.calls.slice(start), errors = [];
    if (actualStatus !== status) errors.push(`run transport ${actualStatus}, expected ${status}`);
    if (!after.equals(want)) errors.push('literal complete arena');
    const actualHelpers = calls.filter(call => !call.name.startsWith('guard'));
    if (JSON.stringify(actualHelpers) !== JSON.stringify(helpers)) errors.push('exact real helper names/arguments/status');
    const afterRAM = ram(c); checkRAM(afterRAM, desiredRAM, errors);
    return receipt(c, label, before, after, want, beforeRAM, afterRAM, desiredRAM, errors,
      {kind, status: actualStatus, expected_status: status, classification: actualStatus === 2 ? 'runtime-record-admission' : 'executed-module',
        expected_exit: {reason: want.readUInt32LE(72), retired: want.readUInt32LE(76), eip: want.readUInt32LE(48), flags: want.readUInt32LE(52)},
        actual_exit: {reason: after.readUInt32LE(72), retired: after.readUInt32LE(76), eip: after.readUInt32LE(48), flags: after.readUInt32LE(52)}, calls});
  }
  function lookup(c, label, input, pc, material, status = material ? 0 : 17) {
    put(c, 0, input); const before = arena(c), beforeRAM = ram(c), want = Buffer.from(before), errors = [];
    if (material) want.set(words([1, 24, ...material.binding.slice(2), material.pointer, material.length]), 140);
    const actualStatus = c.api.find_resident(pc), after = arena(c);
    if (actualStatus !== status) errors.push(`lookup status ${actualStatus}, expected ${status}`);
    if (!after.equals(want)) errors.push('exact lookup publication/neutrality');
    const afterRAM = ram(c); checkRAM(afterRAM, beforeRAM, errors);
    return receipt(c, label, before, after, want, beforeRAM, afterRAM, beforeRAM, errors,
      {kind: 'resident-lookup', expected_status: status, status: actualStatus});
  }
  function skip(c, label, prerequisite) {rows.push({context: c.key, label, passed: false, kind: 'unreached', classification: 'prerequisite-admission', prerequisite, errors: ['required earlier compile did not admit this context']});}
  function finish(limits) {
    assert.deepEqual(pins(), sourcePins, 'source and independent literal oracles stayed fixed');
    const counts = {passed: rows.filter(row => row.passed).length, failed: rows.filter(row => !row.passed && row.kind !== 'unreached').length,
      unreached: rows.filter(row => row.kind === 'unreached').length, executed_module: rows.filter(row => row.classification === 'executed-module').length,
      admission_failures: rows.filter(row => ['compile-admission-or-capability', 'runtime-record-admission'].includes(row.classification)).length};
    const result = {status: rows.every(row => row.passed) ? 'PASS' : 'FAIL', engine: {path: enginePath, sha256: engine.sha256, bytes: engine.bytes.length},
      initial_arena: {bytes: initial.length, sha256: hash(initial)}, source_pins: sourcePins, contexts, modules, counts, rows, limits};
    writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2) + '\n');
    console.log(JSON.stringify({status: result.status, counts, contexts: contexts.length, modules: modules.length, output}));
    process.exitCode = result.status === 'PASS' ? 0 : 1;
  }
  return {initial, engine, open, put, arena, host, upload, ram, compile, observe, lookup, skip, finish};
}
