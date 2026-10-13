import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { readEngine } from '../../../../support/wasm/engine.mjs';
import { createArena, ARENA_SIZE } from '../../../../support/wasm/arena.mjs';
import { assertEngineExports } from '../../../../support/wasm/exports.mjs';

export function runStoppedFaults({ sourcePaths, positive, continuations, negatives, claim }) {
  const [enginePath, output, root] = process.argv.slice(2),
    engine = readEngine(enginePath);
  const { functions: declaredExports, sourcePaths: exportSources } = assertEngineExports(
    engine.module,
    root,
  );
  const PC = 0x1000,
    KEY_HIGH = 0x4d535250,
    observations = [],
    modules = [];
  let contexts = 0,
    runs = 0,
    refusals = 0,
    fullRamPairs = 0;
  const hash = (bytes) => createHash('sha256').update(bytes).digest('hex');
  const paths = [
    ...exportSources,
    'engine/tests/support/wasm/engine.mjs',
    'engine/tests/support/wasm/arena.mjs',
    'engine/src/cpu/x86/decode/decoder.rs',
    'engine/src/cpu/x86/decode/execution_faults.rs',
    'engine/src/cpu/x86/capabilities.rs',
    'engine/src/cpu/x86/environment.rs',
    'engine/src/cpu/x86/control.rs',
    'engine/src/cpu/dbt/cold.rs',
    'engine/src/cpu/dbt/artifact.rs',
    'engine/src/cpu/dbt/region.rs',
    'engine/src/cpu/dbt/wasm/control_instructions.rs',
    'engine/src/cpu/dbt/wasm/integer/dispatch.rs',
    'engine/src/cpu/dbt/wasm/emitter.rs',
    'engine/src/cpu/dbt/wasm/abi.rs',
    'engine/src/process/execution/stopped.rs',
    'engine/src/process/wasm/execution.rs',
    'engine/src/abi/x86/state.rs',
    'engine/src/abi/x86/exit.rs',
    'engine/tests/cpu/control/exceptions/support/stopped.mjs',
    ...sourcePaths,
  ];
  assert.equal(new Set(paths).size, paths.length);
  const pins = () =>
    Object.fromEntries(paths.map((path) => [path, hash(readFileSync(join(root, path)))]));
  const sourcePins = pins();
  const words = (values) => {
    const bytes = Buffer.alloc(values.length * 4);
    values.forEach((value, i) => bytes.writeUInt32LE(value >>> 0, i * 4));
    return bytes;
  };
  function exit(reason, retired, version = 1) {
    const bytes = Buffer.alloc(40);
    bytes.write('R3EX');
    bytes.writeUInt16LE(version, 4);
    bytes.writeUInt16LE(1, 6);
    bytes.writeUInt32LE(40, 8);
    bytes.set(words([reason, retired, 0, 0, 0, 0]), 16);
    return bytes;
  }
  function open(row, entries) {
    const instance = new WebAssembly.Instance(engine.module, {}),
      api = {};
    for (const [name, fn] of Object.entries(instance.exports))
      if (name.startsWith('ring3_abi_v1_') && typeof fn === 'function') api[name.slice(13)] = fn;
    assert.equal(
      typeof api.compile_stopped_entry,
      'function',
      'integrated engine exports the explicit stopped-entry endpoint',
    );
    const id = ++contexts,
      memory = instance.exports.memory;
    for (const [name, arity] of Object.entries({
      open: 3,
      arena_ptr: 0,
      arena_len: 0,
      map: 3,
      upload: 2,
      compile: 1,
      compile_entries: 2,
      compile_resident: 1,
      compile_resident_entries: 2,
      compile_stopped_entry: 3,
      generation: 0,
      module_ptr: 0,
      module_len: 0,
      guard: 6,
      cpu_read: 3,
      close: 0,
    })) {
      assert.equal(api[name].length, arity, name);
      assert.equal(
        declaredExports.find((row) => row.name === `ring3_abi_v1_${name}`)?.arity,
        arity,
        name + ' declared',
      );
    }
    assert.equal(api.open(3, id, KEY_HIGH), 0);
    const base = api.arena_ptr() >>> 0;
    const arena = () => Buffer.from(new Uint8Array(memory.buffer, base, ARENA_SIZE));
    const put = (offset, bytes) => new Uint8Array(memory.buffer).set(bytes, base + offset);
    const upload = (address, bytes) => {
      put(140, bytes);
      assert.equal(api.upload(address, bytes.length), 0);
    };
    const target = row.target ?? 0x1100;
    assert.equal(api.map(PC, 1, 7), 0);
    assert.equal(api.map(0x5000, 1, 3), 0);
    upload(0x5000, Buffer.alloc(4096, 0x5a));
    const jump = Buffer.alloc(5);
    jump[0] = 0xe9;
    jump.writeInt32LE(target - PC - 5, 1);
    upload(PC, jump);
    upload(target, row.bytes);
    put(140, words(entries ? [PC] : [PC, 5]));
    assert.equal(api[entries ? 'compile_entries' : 'compile'](1, ...(entries ? [0] : [])), 0);
    const moduleBytes = () =>
      Buffer.from(new Uint8Array(memory.buffer, api.module_ptr() >>> 0, api.module_len() >>> 0));
    let helpers = 0;
    function load() {
      const bytes = moduleBytes();
      const file = `module-${modules.length + 1}.wasm`;
      writeFileSync(join(output, file), bytes, { flag: 'wx' });
      const module = new WebAssembly.Module(bytes);
      assert.deepEqual(
        WebAssembly.Module.imports(module),
        [
          { module: 'env', name: 'memory', kind: 'memory' },
          { module: 'ring3', name: 'guard', kind: 'function' },
        ],
        'synthetic faults never call a memory, platform, or numerical helper',
      );
      modules.push({ label: row.name, entries, file, bytes: bytes.length, sha256: hash(bytes) });
      const ring3 = Object.fromEntries(
        WebAssembly.Module.imports(module)
          .filter((item) => item.kind === 'function')
          .map((item) => [
            item.name,
            (...args) => {
              if (item.name !== 'guard') helpers++;
              return api[item.name](...args);
            },
          ]),
      );
      return new WebAssembly.Instance(module, { env: { memory }, ring3 }).exports.run;
    }
    const initial = createArena();
    if (row.x87 !== undefined) {
      assert.equal(row.x87.length, 128);
      initial.set(row.x87, 4236);
    }
    [
      0xa1b2c3d4, 0x23456789, 0x3456789a, 0x456789ab, 0x7000, 0x56789abc, 0x6789abcd, 0x789abcde,
    ].forEach((value, i) => initial.writeUInt32LE(value, 16 + i * 4));
    for (let i = 0; i < 8; i++) {
      initial.fill(0x30 + i, 4396 + i * 16, 4412 + i * 16);
      initial.fill(0x70 + i, 4628 + i * 16, 4644 + i * 16);
    }
    initial.writeUInt32LE(PC, 48);
    put(0, initial);
    assert.equal(load()(base, base + 56, 8, base + 96), 0);
    const stopped = arena();
    assert.equal(stopped.readUInt32LE(48), target);
    assert.deepEqual(
      stopped.subarray(56, 96),
      exit(3, 1),
      'bootstrap reaches a real missing-code stop',
    );
    return {
      api,
      base,
      memory,
      arena,
      put,
      upload,
      target,
      id,
      entries,
      load,
      moduleBytes,
      helpers: () => helpers,
      pages: [PC, 0x5000],
      label: `${row.name}/${entries ? 'entry' : 'extent'}`,
    };
  }
  function assertRefusal(c, invoke, label) {
    const before = c.arena(),
      generation = c.api.generation(),
      bytes = c.moduleBytes();
    assert.notEqual(invoke(), 0, c.label + '/' + label + ' remains a host preparation error');
    assert.deepEqual(
      c.arena(),
      before,
      c.label + '/' + label + ' preserves CPU, stop and numeric records',
    );
    assert.equal(c.api.generation(), generation);
    assert.deepEqual(c.moduleBytes(), bytes);
    refusals++;
  }
  function prepare(c, row) {
    for (const owner of [
      'compile',
      'compile_entries',
      'compile_resident',
      'compile_resident_entries',
    ]) {
      const entries = owner.endsWith('entries');
      c.put(140, words(entries ? [c.target] : [c.target, row.bytes.length]));
      assertRefusal(c, () => c.api[owner](1, ...(entries ? [0] : [])), 'public-strict-' + owner);
    }
    c.put(140, words([0xdeadbeef, 0xffffffff]));
    const before = c.arena(),
      generation = c.api.generation();
    assert.equal(
      c.api.compile_stopped_entry(c.id, KEY_HIGH, generation),
      0,
      c.label + ' runtime entry preparation',
    );
    assert.equal(c.api.generation(), generation + 1);
    assert.deepEqual(
      c.arena(),
      before,
      'runtime preparation uses state EIP and ignores poisoned descriptors without publishing CPU changes',
    );
    return c.load();
  }
  function ram(c) {
    const saved = c.arena(),
      result = Buffer.alloc(c.pages.length * 4096);
    for (const [page, address] of c.pages.entries())
      for (let offset = 0; offset < 4096; offset += 512) {
        assert.equal(c.api.cpu_read(address + offset, 512, 1), 0);
        const frame = c.arena();
        assert.equal(frame.readUInt32LE(116), 0);
        frame.copy(result, page * 4096 + offset, 140, 140 + 512);
      }
    c.put(0, saved);
    return result;
  }
  function seedRegisters(frame, row) {
    for (const [index, value] of Object.entries(row.registers ?? {})) {
      assert.ok(Number.isInteger(Number(index)) && Number(index) >= 0 && Number(index) < 8);
      frame.writeUInt32LE(value >>> 0, 16 + Number(index) * 4);
    }
  }
  function execute(
    c,
    fn,
    row,
    {
      flags = 2,
      shadow = 0,
      budget = 8,
      cancel = 0,
      reason = row.reason,
      retired = row.retired ?? 0,
      offset = row.offset ?? 0,
      pc = c.target + offset,
      version = reason === 20 ? 7 : row.baseVersion,
      work = row.work,
    } = {},
  ) {
    const before = c.arena();
    before.writeUInt16LE(flags & ~0xcd7 || shadow ? 2 : 1, 4);
    before.writeUInt32LE(shadow, 12);
    before.writeUInt32LE(c.target, 48);
    before.writeUInt32LE(flags, 52);
    before.writeUInt32LE(cancel, 96);
    seedRegisters(before, row);
    if (row.segments) {
      assert.equal(row.segments.length, 64);
      before.set(row.segments, 4524);
    }
    for (const offset of [80, 84, 88, 92]) before.writeUInt32LE(0xdeadbeef, offset);
    c.put(0, before);
    const expected = Buffer.from(before),
      beforeRam = ram(c);
    expected.writeUInt32LE(retired === 0 && [1, 2].includes(reason) ? shadow : 0, 12);
    expected.writeUInt32LE(pc, 48);
    expected.set(exit(reason, retired, version), 56);
    if (work !== undefined) expected.writeUInt32LE(work, 4604);
    const calls = c.helpers();
    assert.equal(fn(c.base, c.base + 56, budget, c.base + 96), 0, c.label + ' run status');
    assert.deepEqual(
      c.arena(),
      expected,
      c.label + ' precise fault/stop, full CPU, FLAGS, shadow and unrelated arena preservation',
    );
    assert.equal(c.helpers(), calls, 'no guest operand/helper effect before synthetic fault');
    assert.deepEqual(ram(c), beforeRam, c.label + ' complete mapped RAM remains unchanged');
    assert.deepEqual(c.arena(), expected, c.label + ' diagnostics preserve complete arena');
    fullRamPairs++;
    observations.push({
      label: c.label,
      flags,
      shadow,
      budget,
      cancel,
      reason,
      retired,
      pc,
      version,
      work,
    });
    runs++;
  }
  for (const entries of [false, true])
    for (const row of positive) {
      const c = open(row, entries),
        fn = prepare(c, row);
      if (row.schedules) {
        assert.ok(row.schedules.length > 0);
        for (const schedule of row.schedules) execute(c, fn, row, schedule);
      } else {
        execute(c, fn, row);
        execute(c, fn, row, { flags: 0x102 });
        execute(c, fn, row, { flags: 0x244cd7, shadow: 1 });
        execute(c, fn, row, {
          flags: 0x102,
          shadow: 1,
          budget: 0,
          reason: 1,
          retired: 0,
          pc: c.target,
        });
        execute(c, fn, row, {
          flags: 0x102,
          shadow: 1,
          cancel: 1,
          reason: 2,
          retired: 0,
          pc: c.target,
        });
      }
      const before = c.arena();
      before.writeUInt32LE(0, 96);
      c.upload(c.target, Buffer.from([0x90]));
      const current = c.arena();
      current.set(before.subarray(0, 140));
      c.put(0, current);
      assert.notEqual(
        fn(c.base, c.base + 56, 8, c.base + 96),
        0,
        'modified executable bytes invalidate the synthetic fault artifact',
      );
      assert.deepEqual(c.arena(), current, 'stale fault guard publishes no guest exception');
      refusals++;
      assert.equal(c.api.close(), 0);
    }
  for (const entries of [false, true])
    for (const row of continuations) {
      const c = open(row, entries),
        fn = prepare(c, row);
      if (row.schedules) {
        assert.ok(row.schedules.length > 0);
        for (const schedule of row.schedules) execute(c, fn, row, schedule);
      } else {
        execute(c, fn, row);
        execute(c, fn, row, { budget: 1, reason: 1, retired: 1, pc: c.target + 1 });
      }
      assert.equal(c.api.close(), 0);
    }
  for (const entries of [false, true])
    for (const row of negatives) {
      const c = open(row, entries);
      const before = c.arena();
      seedRegisters(before, row);
      c.put(0, before);
      c.put(140, words([0xdeadbeef, 0xffffffff]));
      assertRefusal(
        c,
        () => c.api.compile_stopped_entry(c.id, KEY_HIGH, c.api.generation()),
        'runtime-nonarchitectural-error',
      );
      if (row.repair) {
        assert.equal(c.api.map(0x2000, 1, 7), 0);
        c.pages.push(0x2000);
        c.upload(0x2000, row.repair);
        const before = c.arena();
        assert.equal(c.api.compile_stopped_entry(c.id, KEY_HIGH, c.api.generation()), 0);
        assert.deepEqual(c.arena(), before);
        execute(c, c.load(), row.repaired);
      }
      assert.equal(c.api.close(), 0);
    }
  assert.equal(
    runs,
    positive.reduce((sum, row) => sum + 2 * (row.schedules?.length ?? 5), 0) +
      continuations.reduce((sum, row) => sum + 2 * (row.schedules?.length ?? 2), 0) +
      negatives.filter((row) => row.repair).length * 2,
  );
  assert.equal(fullRamPairs, runs);
  assert.deepEqual(pins(), sourcePins, 'execution fault proof sources remain frozen');
  const result = {
    schema: 1,
    engineSha256: engine.sha256,
    sourcePins,
    contexts,
    runs,
    refusals,
    fullRamPairs,
    modules,
    observations,
    claim,
  };
  writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2), { flag: 'wx' });
  console.log(
    JSON.stringify({ contexts, runs, refusals, fullRamPairs, engineSha256: engine.sha256 }),
  );
}
