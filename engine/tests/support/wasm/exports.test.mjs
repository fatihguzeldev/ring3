import assert from 'node:assert/strict';
import {mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import test from 'node:test';
import {assertEngineExports} from './exports.mjs';

function workspace(body, child, check) {
  const root = mkdtempSync(join(tmpdir(), 'ring3-export-tables-'));
  const dir = join(root, 'engine/src/abi/wasm/exports');
  mkdirSync(join(dir, 'memory'), {recursive: true});
  writeFileSync(join(dir, 'mod.rs'), 'macro_rules! export_process_calls {\n}\nmod memory;\n');
  writeFileSync(join(dir, 'memory.rs'), body);
  writeFileSync(join(dir, 'memory/cpu_write.rs'), child);
  try { check(root); } finally { rmSync(root, {recursive: true, force: true}); }
}

function moduleWith(names, {observability = false} = {}) {
  const unsigned = value => {
    const bytes = [];
    do {
      const byte = value & 127;
      value >>>= 7;
      bytes.push(byte | (value ? 128 : 0));
    } while (value);
    return bytes;
  };
  const section = (id, bytes) => [id, ...unsigned(bytes.length), ...bytes];
  const name = value => [...unsigned(Buffer.byteLength(value)), ...Buffer.from(value)];
  return new WebAssembly.Module(Uint8Array.from([
    0, 97, 115, 109, 1, 0, 0, 0,
    ...section(1, observability ? [2, 0x60, 0, 0, 0x60, 0, 1, 0x7e] : [1, 0x60, 0, 0]),
    ...(observability ? section(2, [1, ...name('ring3_observability'), ...name('now_ns'), 0, 1]) : []),
    ...section(3, [names.length, ...names.map(() => 0)]),
    ...section(5, [1, 0, 1]),
    ...section(7, [names.length + 1, ...name('memory'), 2, 0,
      ...names.flatMap((value, index) => [...name(value), 0, index + Number(observability)])]),
    ...section(10, [names.length, ...names.flatMap(() => [2, 0, 0x0b])]),
  ]));
}

const parent = 'mod cpu_write;\n\nexport_process_calls! { read32(address: u32); }\n';
const child = 'export_process_calls! { cpu_store32(address: u32, value: u32); }\n';
const module = moduleWith(['ring3_abi_v1_read32', 'ring3_abi_v1_cpu_store32']);

test('declared child tables retain exact exports, arities and source pins', () => {
  workspace(parent, child, root => {
    const result = assertEngineExports(module, root);
    assert.deepEqual(result.functions.map(row => [row.name, row.arity]), [
      ['ring3_abi_v1_read32', 1], ['ring3_abi_v1_cpu_store32', 2],
    ]);
    assert.ok(result.sourcePaths.includes('engine/src/abi/wasm/exports/memory/cpu_write.rs'));
    assert.throws(() => assertEngineExports(moduleWith(['ring3_abi_v1_read32']), root), /exact declared engine exports/);
  });
});

test('unknown source tokens remain a closed-table error', () => {
  workspace(parent + 'fn hidden() {}\n', child, root => {
    assert.throws(() => assertEngineExports(module, root), /closed ABI declaration table/);
  });
});

test('duplicate child declarations and exported names remain errors', () => {
  workspace('mod cpu_write;\n' + parent, child, root => {
    assert.throws(() => assertEngineExports(module, root), /unique export owner/);
  });
  workspace(parent, 'export_process_calls! { read32(address: u32); }\n', root => {
    assert.throws(() => assertEngineExports(module, root), /unique declared engine exports/);
  });
});

const detectionSource = readFileSync(new URL('../../../src/abi/wasm/exports/detection.rs', import.meta.url), 'utf8');
const detectionNames = ['ring3_abi_v1_guard_detect', 'ring3_abi_v1_guard_detect_resident', 'ring3_abi_v1_finish_exception_detection'];

function detectionWorkspace(source, check) {
  const root = mkdtempSync(join(tmpdir(), 'ring3-detection-exports-'));
  const dir = join(root, 'engine/src/abi/wasm/exports');
  mkdirSync(dir, {recursive: true});
  writeFileSync(join(dir, 'mod.rs'), 'macro_rules! export_process_calls {\n}\nmod detection;\n');
  writeFileSync(join(dir, 'detection.rs'), source);
  try { check(root); } finally { rmSync(root, {recursive: true, force: true}); }
}

test('real detection owner retains parsed exact exports, i64 epoch position, arities and source pin', () => {
  detectionWorkspace(detectionSource, root => {
    const result = assertEngineExports(moduleWith(detectionNames), root);
    assert.deepEqual(result.functions.map(row => [row.name, row.arity]), [
      [detectionNames[0], 6], [detectionNames[1], 7], [detectionNames[2], 12],
    ]);
    assert.ok(result.sourcePaths.includes('engine/src/abi/wasm/exports/detection.rs'));
    assert.throws(() => assertEngineExports(moduleWith(detectionNames.slice(0, 2)), root), /exact declared engine exports/);
  });
});

test('detection macro body, names, duplicate declarations and extra tokens stay closed', () => {
  for (const source of [
    detectionSource.replace('-> i64', '-> u32'),
    detectionSource.replace('"ring3_abi_v1_"', '"ring3_abi_ v1_"'),
    detectionSource.replace('pub extern', 'pubex tern'),
    detectionSource.replace('export_detection_guards! {', 'export_unknown_guards! {'),
    detectionSource.replace('guard_detect(low:', 'guard_other(low:'),
    detectionSource.replace('guard_detect_resident(low:', 'guard_detect(low:'),
    detectionSource.replace('generation: u32, state:', 'generation: u32,, state:'),
    detectionSource.replace('export_detection_guards! {', 'export_detection_guards! {\n    guard_detect(low: u32, high: u32, generation: u32, state: u32, exit: u32, cancel: u32);'),
    detectionSource.replace('finish_exception_detection(epoch:', 'finish_other(epoch:'),
    detectionSource + '\nfn hidden() {}\n',
  ]) {
    detectionWorkspace(source, root => {
      assert.throws(() => assertEngineExports(moduleWith(detectionNames), root), /detection|guard/);
    });
  }
});

test('detection i64 parameter position and all three source arities remain exact', () => {
  for (const source of [
    detectionSource.replace('epoch: i64, reason: u32', 'epoch: u32, reason: i64'),
    detectionSource.replace('epoch: i64', 'epoch: u32'),
    detectionSource.replace('generation: u32, ', ''),
    detectionSource.replace('id_high: u32, ', ''),
    detectionSource.replace(', restart_work: u32', ''),
  ]) {
    detectionWorkspace(source, root => {
      assert.throws(() => assertEngineExports(moduleWith(detectionNames), root), /detection parameters/);
    });
  }
  workspace(parent, 'export_process_calls! { cpu_store32(address: i64, value: u32); }\n', root => {
    assert.throws(() => assertEngineExports(module, root), /ABI parameter/);
  });
});

test('the no-import profile omits only the declared conditional observability owner', () => {
  workspace(parent, child, root => {
    const owner = join(root, 'engine/src/abi/wasm/exports/mod.rs');
    const original = readFileSync(owner, 'utf8');
    writeFileSync(owner, original + '#[cfg(feature = "observability")]\nmod observability;\n');
    const result = assertEngineExports(module, root);
    assert.ok(!result.sourcePaths.some(path => path.endsWith('/observability.rs')));
    for (const suffix of [
      '#[cfg(feature = "profiling")]\nmod observability;\n',
      '#[cfg(feature = "observability")]\nmod other;\n',
    ]) {
      writeFileSync(owner, original + suffix);
      assert.throws(() => assertEngineExports(module, root), /closed ABI export owner list/);
    }
  });
});

test('the no-import profile omits the declared conditional census owner without accepting other conditions', () => {
  workspace(parent, child, root => {
    const owner = join(root, 'engine/src/abi/wasm/exports/mod.rs');
    const original = readFileSync(owner, 'utf8');
    const census = '#[cfg(feature = "observability")]\nmod census;\n';
    const observability = '#[cfg(feature = "observability")]\nmod observability;\n';
    writeFileSync(owner, original + census + observability);
    const result = assertEngineExports(module, root);
    assert.ok(!result.sourcePaths.some(path => path.endsWith('/census.rs')));
    assert.ok(!result.sourcePaths.some(path => path.endsWith('/observability.rs')));
    writeFileSync(owner, original + census + census);
    assert.throws(() => assertEngineExports(module, root), /unique conditional census owner/);
    for (const suffix of [
      '#[cfg(feature = "profiling")]\nmod census;\n',
      '#[cfg(feature = "observability")]\nmod other;\n',
    ]) {
      writeFileSync(owner, original + suffix);
      assert.throws(() => assertEngineExports(module, root), /closed ABI export owner list/);
    }
  });
});

const cpuSource = readFileSync(new URL('../../../src/abi/wasm/exports/cpu.rs', import.meta.url), 'utf8');
const pairSource = cpuSource.slice(cpuSource.indexOf('#[allow('));

test('the CPU owner keeps only its exact i64 view parameter and pair/group exports', () => {
  workspace(parent, child, root => {
    const dir = join(root, 'engine/src/abi/wasm/exports');
    writeFileSync(join(dir, 'mod.rs'), 'macro_rules! export_process_calls {\n}\nmod cpu;\n');
    const source = 'export_process_calls! { cpu_read32_view(epoch: i64); }\n' + pairSource;
    const cpu = join(dir, 'cpu.rs');
    writeFileSync(cpu, source);
    const actual = moduleWith(['ring3_abi_v1_cpu_read32_view', 'ring3_abi_v1_x87_resident_pair', 'ring3_abi_v1_x87_resident_group', 'ring3_abi_v1_x87_resident_group32']);
    assert.deepEqual(assertEngineExports(actual, root).functions.map(row => [row.name, row.arity]), [
      ['ring3_abi_v1_cpu_read32_view', 1], ['ring3_abi_v1_x87_resident_pair', 9], ['ring3_abi_v1_x87_resident_group', 15], ['ring3_abi_v1_x87_resident_group32', 39],
    ]);
    for (const invalid of [
      source.replace('epoch: i64', 'epoch: u32'),
      source.replace('epoch: i64', 'other: i64'),
      source.replace(') -> i64', ') -> u32'),
      source.replace('max_steps: u32', 'max_steps: i64'),
      source.replace('ordinal: u32', 'ordinal: i64'),
      source.replace('address7: u32,', ''),
      source.replace('address31: u32,', ''),
      source.replace('address30, address31, flags', 'address31, address30, flags'),
      source.replace('address6, address7, flags', 'address7, address6, flags'),
      source + '\nfn hidden() {}\n',
    ]) {
      writeFileSync(cpu, invalid);
      assert.throws(() => assertEngineExports(actual, root), /view parameters|pair\/group exports/);
    }
    writeFileSync(cpu, source.replace('cpu_read32_view(epoch: i64)', 'other(epoch: i64)'));
    assert.throws(() => assertEngineExports(actual, root), /ABI parameter/);
  });
  workspace('export_process_calls! { cpu_read32_view(epoch: i64); }\n', child, root => {
    assert.throws(() => assertEngineExports(module, root), /ABI parameter/);
  });
});

const coreSource = readFileSync(new URL('../../../src/abi/wasm/exports/core.rs', import.meta.url), 'utf8');
const coreReadSource = coreSource.slice(coreSource.indexOf('#[allow('));
const coreColdStatusSource = '#[cfg(feature = "observability")]\nexport_process_calls! { core_cold_status_ptr(); }\n';

test('continuation generations retain their exact u64 positions without widening other ABI parameters', () => {
  workspace(parent, child, root => {
    const dir=join(root,'engine/src/abi/wasm/exports');
    writeFileSync(join(dir,'mod.rs'),'macro_rules! export_process_calls {\n}\nmod core;\n');
    const declarations='export_process_calls! {\n'+
      'core_numeric_owned(context: u32, slot: u32, generation: u64, pc: u32, ordinal: u32);\n'+
      'core_precise_owned(context: u32, slot: u32, generation: u64);\n'+
      'core_end_owned(context: u32, slot: u32, generation: u64, reason: u32);\n'+
      'core_end_accounted_owned(context: u32, slot: u32, generation: u64, reason: u32, initial_retired: u32, budget: u32);\n}\n';
    const source=declarations+coreReadSource,core=join(dir,'core.rs');
    const actual=moduleWith(['core_numeric_owned','core_precise_owned','core_end_owned','core_end_accounted_owned','core_read','core_cold_run']
      .map(name=>'ring3_abi_v1_'+name));
    writeFileSync(core,source);
    assert.deepEqual(assertEngineExports(actual,root).functions.slice(0,4).map(row=>row.arity),[5,3,4,6]);
    for(const invalid of[
      source.replace('generation: u64','generation: u32'),
      source.replace('slot: u32, generation: u64','generation: u64, slot: u32'),
      source.replace('generation: u64, pc: u32','generation: u64, pc: u64'),
      source.replace('generation: u64, ',''),
      source.replace('initial_retired: u32, budget: u32','budget: u32, initial_retired: u32'),
      source.replace('initial_retired: u32','initial_retired: u64'),
    ]) {
      writeFileSync(core,invalid);
      assert.throws(()=>assertEngineExports(actual,root),/exact continuation generation parameters/);
    }
  });
});

test('the Core cold snapshot tail is exact and follows the observability export profile', () => {
  workspace(parent, child, root => {
    const dir = join(root, 'engine/src/abi/wasm/exports');
    writeFileSync(join(dir, 'mod.rs'), 'macro_rules! export_process_calls {\n}\nmod core;\n' +
      '#[cfg(feature = "observability")]\nmod census;\n#[cfg(feature = "observability")]\nmod observability;\n');
    writeFileSync(join(dir, 'census.rs'), 'export_process_calls! { execution_census_page(offset: u32); }\n');
    writeFileSync(join(dir, 'observability.rs'), 'export_process_calls! { observability_status(); }\n');
    const core = join(dir, 'core.rs');
    const source = 'export_process_calls! { core_begin(); }\n' + coreColdStatusSource + coreReadSource;
    const names = ['ring3_abi_v1_core_begin', 'ring3_abi_v1_core_read', 'ring3_abi_v1_core_cold_run'];
    const enabledNames = [...names, 'ring3_abi_v1_core_cold_status_ptr',
      'ring3_abi_v1_execution_census_page', 'ring3_abi_v1_observability_status'];
    writeFileSync(core, source);
    assert.ok(!assertEngineExports(moduleWith(names), root).functions.some(row => row.name.endsWith('core_cold_status_ptr')));
    const enabled = assertEngineExports(moduleWith(enabledNames, {observability: true}), root, {observability: true});
    assert.deepEqual(enabled.functions.find(row => row.name.endsWith('core_cold_status_ptr')),
      {name: 'ring3_abi_v1_core_cold_status_ptr', kind: 'function', arity: 0});
    assert.throws(() => assertEngineExports(moduleWith([...names, 'ring3_abi_v1_core_cold_status_ptr']), root), /exact declared engine exports/);
    assert.throws(() => assertEngineExports(moduleWith(enabledNames.filter(name => !name.endsWith('core_cold_status_ptr')), {observability: true}), root,
      {observability: true}), /exact declared engine exports/);
    assert.throws(() => assertEngineExports(moduleWith([...enabledNames, 'ring3_abi_v1_hidden'], {observability: true}), root,
      {observability: true}), /exact declared engine exports/);
    for (const invalid of [
      source.replace('feature = "observability"', 'feature = "other"'),
      source.replace('#[cfg(feature = "observability")]\n', ''),
      source.replace('core_cold_status_ptr();', 'core_cold_status_ptr(value: u32);'),
      source.replace('core_cold_status_ptr();', 'other();'),
      source.replace(coreColdStatusSource, coreColdStatusSource + coreColdStatusSource),
      source + '\nfn hidden() {}\n',
    ]) {
      writeFileSync(core, invalid);
      assert.throws(() => assertEngineExports(moduleWith(names), root), /Core|declaration/);
    }
  });
});

test('the Core owner retains exact handwritten i64 read and signed cold exports', () => {
  workspace(parent, child, root => {
    const dir = join(root, 'engine/src/abi/wasm/exports');
    writeFileSync(join(dir, 'mod.rs'), 'macro_rules! export_process_calls {\n}\nmod core;\n');
    const core = join(dir, 'core.rs');
    const source = 'export_process_calls! { core_begin(); core_write(context: u32, address: u32, width: u32, value: u32); }\n' + coreReadSource;
    writeFileSync(core, source);
    const actual = moduleWith(['ring3_abi_v1_core_begin', 'ring3_abi_v1_core_write', 'ring3_abi_v1_core_read', 'ring3_abi_v1_core_cold_run']);
    const result = assertEngineExports(actual, root);
    assert.deepEqual(result.functions.map(row => [row.name, row.arity]), [
      ['ring3_abi_v1_core_begin', 0], ['ring3_abi_v1_core_write', 4], ['ring3_abi_v1_core_read', 3],
      ['ring3_abi_v1_core_cold_run', 1],
    ]);
    assert.ok(result.sourcePaths.includes('engine/src/abi/wasm/exports/core.rs'));
    assert.throws(() => assertEngineExports(moduleWith(['ring3_abi_v1_core_begin', 'ring3_abi_v1_core_write']), root), /exact declared engine exports/);
    for (const invalid of [
      source.replace(') -> i64', ') -> u32'),
      source.replace('width: u32) -> i64', 'width: i64) -> i64'),
      source.replace('core_read(context, address, width)', 'core_read(context, width, address)'),
      source.replace('ring3_abi_v1_core_read', 'ring3_abi_v1_other'),
      source.replace(') -> i32', ') -> u32'),
      source.replace('budget: u32', 'budget: i32'),
      source.replace('core_cold_run(budget)', 'core_cold_run(0)'),
      source.replace('ring3_abi_v1_core_cold_run', 'ring3_abi_v1_other'),
      source + '\nfn hidden() {}\n',
    ]) {
      writeFileSync(core, invalid);
      assert.throws(() => assertEngineExports(actual, root), /declared Core handwritten exports/);
    }
  });
});

const instanceSource = readFileSync(new URL('../../../src/abi/wasm/exports/instance.rs', import.meta.url), 'utf8');

test('the no-import default profile omits only the exact conditional component memory lease', () => {
  workspace(parent, child, root => {
    const dir = join(root, 'engine/src/abi/wasm/exports');
    writeFileSync(join(dir, 'mod.rs'), 'macro_rules! export_process_calls {\n}\nmod instance;\n');
    const instance = join(dir, 'instance.rs');
    writeFileSync(instance, instanceSource);
    const names = ['open', 'close', 'arena_ptr', 'arena_len'].map(name => `ring3_abi_v1_${name}`);
    const actual = moduleWith(names);
    const result = assertEngineExports(actual, root);
    assert.deepEqual(result.functions.map(row => [row.name, row.arity]), [
      [names[0], 3], [names[1], 0], [names[2], 0], [names[3], 0],
    ]);
    assert.ok(result.sourcePaths.includes('engine/src/abi/wasm/exports/instance.rs'));
    assert.throws(() => assertEngineExports(moduleWith([...names, 'ring3_abi_v1_component_memory_lease']), root), /exact declared engine exports/);
    for (const invalid of [
      instanceSource.replace('feature = "external-cpu"', 'feature = "other"'),
      instanceSource.replace('component_memory_lease(', 'other('),
      instanceSource.replace('key_high: u32', 'key_high: i64'),
      instanceSource.replace('key_low: u32, key_high: u32', 'key_high: u32, key_low: u32'),
      instanceSource.replace('#[cfg(feature = "external-cpu")]\n', ''),
      instanceSource + '\nfn hidden() {}\n',
    ]) {
      writeFileSync(instance, invalid);
      assert.throws(() => assertEngineExports(actual, root), /declared conditional component memory lease/);
    }
  });
});
