import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { readEngine } from '../../../../../../../support/wasm/engine.mjs';
import { createArena, ARENA_SIZE } from '../../../../../../../support/wasm/arena.mjs';
import { assertEngineExports } from '../../../../../../../support/wasm/exports.mjs';

const [enginePath, output, root] = process.argv.slice(2);
const engine = readEngine(enginePath);
const { functions: exports, sourcePaths: exportSources } = assertEngineExports(engine.module, root);
const PC = 0x1000,
  DATA = 0x5000,
  T = 140,
  KEY_HIGH = 0x4944454e;
assert.equal(ARENA_SIZE, 4756);
const hash = (bytes) => createHash('sha256').update(bytes).digest('hex');
const sourcePaths = [
  ...exportSources,
  'engine/tests/support/wasm/engine.mjs',
  'engine/tests/support/wasm/arena.mjs',
  'engine/src/cpu/x86/environment.rs',
  'engine/src/cpu/x86/capabilities.rs',
  'engine/src/cpu/x86/decode/cache_timestamp.rs',
  'engine/src/cpu/x86/decode/decoder.rs',
  'engine/src/cpu/x86/decode/profile.rs',
  'engine/src/cpu/x86/decode/lower.rs',
  'engine/src/cpu/x86/ir.rs',
  'engine/src/cpu/dbt/region.rs',
  'engine/src/cpu/dbt/wasm/integer_system.rs',
  'engine/src/cpu/dbt/wasm/emitter.rs',
  'engine/src/cpu/dbt/wasm/integer/dispatch.rs',
  'engine/src/cpu/dbt/wasm/atomic.rs',
  'engine/src/cpu/dbt/wasm/abi.rs',
  'engine/src/process/cpu/integer.rs',
  'engine/src/abi/x86/state.rs',
  'engine/src/abi/x86/segments.rs',
  'engine/tests/cpu/profile/environment/identity/native.rs',
  'engine/tests/cpu/profile/environment/identity/wasm.rs',
  'engine/tests/cpu/profile/environment/identity/fixtures/p2-processor-identity/runtime/run.mjs',
];
assert.equal(new Set(sourcePaths).size, sourcePaths.length);
const pins = () =>
  Object.fromEntries(sourcePaths.map((path) => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = pins();
const counts = {
  contexts: 0,
  modules: 0,
  runs: 0,
  identities: 0,
  timestamps: 0,
  controls: 0,
  refusals: 0,
  fullRamPairs: 0,
};
const modules = [],
  observations = [];
function words(values) {
  const bytes = Buffer.alloc(values.length * 4);
  values.forEach((value, index) => bytes.writeUInt32LE(value >>> 0, index * 4));
  return bytes;
}
function record(magic, length, fields, version = 1) {
  const bytes = Buffer.alloc(length);
  bytes.write(magic);
  bytes.writeUInt16LE(version, 4);
  bytes.writeUInt16LE(1, 6);
  bytes.writeUInt32LE(length, 8);
  bytes.set(words(fields), 16);
  return bytes;
}
const arena = (context) =>
  Buffer.from(new Uint8Array(context.memory.buffer, context.base, ARENA_SIZE));
const put = (context, offset, bytes) =>
  new Uint8Array(context.memory.buffer).set(bytes, context.base + offset);
function call(context, name, args = []) {
  assert.equal(context.api[name](...args), 0, name);
}
function upload(context, address, bytes) {
  put(context, T, bytes);
  call(context, 'upload', [address, bytes.length]);
}
function open(resident, entries) {
  const instance = new WebAssembly.Instance(engine.module, {});
  const context = {
    resident,
    entries,
    api: {},
    memory: instance.exports.memory,
    key: ++counts.contexts,
    tsc: 0,
    calls: [],
    pages: [PC, DATA],
  };
  for (const [name, fn] of Object.entries(instance.exports)) {
    if (name.startsWith('ring3_abi_v1_') && typeof fn === 'function')
      context.api[name.slice(13)] = fn;
  }
  for (const [name, arity] of Object.entries({
    open: 3,
    map: 3,
    upload: 2,
    compile: 1,
    compile_entries: 2,
    compile_resident: 1,
    compile_resident_entries: 2,
    cpu_integer: 2,
    cpu_integer_resident: 6,
    cpu_read: 3,
    close: 0,
  })) {
    assert.equal(context.api[name]?.length, arity, name);
    assert.equal(
      exports.find((row) => row.name === `ring3_abi_v1_${name}`)?.arity,
      arity,
      `${name} declared`,
    );
  }
  call(context, 'open', [4, context.key, KEY_HIGH]);
  call(context, 'configure_cpu_clock', [1_000_000_000, 0, 0, 0]);
  context.base = context.api.arena_ptr() >>> 0;
  assert.equal(context.api.arena_len(), ARENA_SIZE);
  call(context, 'map', [PC, 1, 7]);
  call(context, 'map', [DATA, 1, 3]);
  for (let offset = 0; offset < 4096; offset += 512)
    upload(context, DATA + offset, Buffer.alloc(512, 0x5a ^ (offset >>> 9)));
  return context;
}
function ram(context) {
  const saved = arena(context),
    bytes = Buffer.alloc(context.pages.length * 4096);
  for (const [index, page] of context.pages.entries()) {
    for (let offset = 0; offset < 4096; offset += 512) {
      call(context, 'cpu_read', [page + offset, 512, 1]);
      const frame = arena(context);
      assert.equal(frame.readUInt32LE(116), 0, 'RAM observation is authorized');
      frame.copy(bytes, index * 4096 + offset, T, T + 512);
    }
  }
  put(context, 0, saved);
  return bytes;
}
function capture(context, name, bytes, timestamps = false, pc = PC) {
  if (context.unit) {
    put(context, 96, words([0]));
    call(context, 'discard_unacknowledged_resident', [context.key, KEY_HIGH, ...context.unit]);
  }
  const program = Buffer.from([...bytes, 0xeb, 0xfe]);
  upload(context, pc, program);
  put(context, T, words(context.entries ? [pc] : [pc, program.length]));
  const method = context.resident
    ? context.entries
      ? 'compile_resident_entries'
      : 'compile_resident'
    : context.entries
      ? 'compile_entries'
      : 'compile';
  const before = arena(context);
  assert.equal(
    context.api[method](1, ...(context.entries ? [0] : [])),
    0,
    `${name}: strict ${method} capture`,
  );
  assert.deepEqual(
    arena(context).subarray(0, T),
    before.subarray(0, T),
    `${name}: capture preserves architectural state`,
  );
  assert.deepEqual(
    arena(context).subarray(4236),
    before.subarray(4236),
    `${name}: capture preserves all extended state`,
  );
  const receipt = arena(context);
  if (context.resident) context.unit = [receipt.readUInt32LE(148), receipt.readUInt32LE(152)];
  const pointer = context.resident ? receipt.readUInt32LE(156) : context.api.module_ptr() >>> 0;
  const length = context.resident ? receipt.readUInt32LE(160) : context.api.module_len() >>> 0;
  const binary = Buffer.from(new Uint8Array(context.memory.buffer, pointer, length));
  const file = `module-${++counts.modules}.wasm`;
  writeFileSync(join(output, file), binary, { flag: 'wx' });
  const module = new WebAssembly.Module(binary);
  const helper = context.resident ? 'cpu_integer_resident' : 'cpu_integer';
  assert.deepEqual(
    WebAssembly.Module.imports(module)
      .map((row) => `${row.module}:${row.name}:${row.kind}`)
      .sort(),
    [
      'env:memory:memory',
      `ring3:${context.resident ? 'guard_resident' : 'guard'}:function`,
      ...(timestamps ? [`ring3:${helper}:function`] : []),
    ].sort(),
    `${name}: identity adds no platform/memory/timestamp helper`,
  );
  assert.deepEqual(WebAssembly.Module.exports(module), [{ name: 'run', kind: 'function' }]);
  const ring3 = { ...context.api };
  if (timestamps)
    ring3[helper] = (...args) => {
      const before = arena(context),
        status = context.api[helper](...args),
        after = arena(context),
        expected = Buffer.from(before);
      expected.set(after.subarray(100, 164), 100);
      assert.deepEqual(
        after,
        expected,
        'timestamp helper only publishes its packet and six payload words',
      );
      context.calls.push({ args: args.map((value) => value >>> 0), status });
      return status;
    };
  modules.push({
    name,
    resident: context.resident,
    entries: context.entries,
    file,
    sha256: hash(binary),
    bytes: binary.length,
  });
  return {
    run: new WebAssembly.Instance(module, { env: { memory: context.memory }, ring3 }).exports.run,
    version: timestamps ? 6 : 1,
    pc,
    name,
    pointer,
    length,
  };
}
function seed(context, pc, flags, shadow) {
  const frame = createArena();
  if (flags & ~0xcd7 || shadow) frame.writeUInt16LE(2, 4);
  frame.writeUInt32LE(shadow, 12);
  frame.set(
    words([
      0xa1b2c3d4, 0x23456789, 0x3456789a, 0x456789ab, 0xfffffff0, 0x56789abc, 0x6789abcd,
      0x789abcde,
    ]),
    16,
  );
  frame.writeUInt32LE(pc, 48);
  frame.writeUInt32LE(flags, 52);
  // retain valid but distinctive numeric records and unusable data segments.
  frame.fill(0xa5, T, 4236);
  for (let register = 0; register < 8; register++) {
    frame.writeBigUInt64LE(0x8000000000000000n + BigInt(register), 4276 + register * 10);
    frame.writeUInt16LE(0x3fff, 4284 + register * 10);
    frame.fill(0x31 + register, 4396 + register * 16, 4412 + register * 16);
    frame.fill(0x71 + register, 4628 + register * 16, 4644 + register * 16);
  }
  frame.writeUInt32LE(0xe5af, 4380);
  for (const register of [0, 3, 4, 5]) frame.writeUInt16LE(0, 4540 + register * 2);
  put(context, 0, frame);
}
function execute(context, compiled, action, options = {}, initialize = true) {
  const { flags = 2, shadow = 0, budget = 1, cancel = 0 } = options;
  if (initialize) seed(context, compiled.pc, flags, shadow);
  put(context, 96, words([cancel]));
  const before = arena(context),
    beforeRam = ram(context),
    expected = Buffer.from(before),
    first = context.calls.length;
  const retired = cancel || !budget ? 0 : 1;
  const reason = cancel ? 2 : !budget ? 1 : before.readUInt32LE(52) & 0x100 ? 16 : 1;
  expected.set(
    record('R3EX', 40, [reason, retired, 0, 0, 0, 0], reason === 16 ? 7 : compiled.version),
    56,
  );
  if (retired) {
    expected.writeUInt32LE(before.readUInt32LE(48) + action.bytes.length, 48);
    expected.writeUInt32LE(0, 12);
    if (action.kind === 'identity') {
      expected.writeUInt32LE(0, 16 + action.register * 4);
      counts.identities++;
    } else {
      const tick = context.tsc;
      expected.writeUInt32LE(tick, 16);
      expected.writeUInt32LE(0, 24);
      if (action.kind === 'aux') expected.writeUInt32LE(0, 20);
      expected.set(record('R3MH', 40, [0, 0, 0, 0, 0, 0], 7), 100);
      expected.set(
        words([
          before.readUInt32LE(T),
          before.readUInt32LE(52),
          tick,
          0,
          before.readUInt32LE(28),
          action.kind === 'aux' ? 0 : before.readUInt32LE(20),
        ]),
        T,
      );
      counts.timestamps++;
    }
  }
  assert.equal(
    compiled.run(context.base, context.base + 56, budget, context.base + 96),
    0,
    `${compiled.name}: generated run`,
  );
  assert.deepEqual(
    arena(context),
    expected,
    `${compiled.name}/${action.kind}: complete4756-byte publication, budget${budget}/cancel${cancel}`,
  );
  const calls = context.calls.slice(first),
    timestamp = retired && action.kind !== 'identity';
  assert.equal(
    calls.length,
    Number(Boolean(timestamp)),
    `${compiled.name}: no identity counter event`,
  );
  if (timestamp) {
    assert.equal(calls[0].status, 0);
    assert.deepEqual(calls[0].args.slice(-2), [action.kind === 'aux' ? 0x20001 : 0x20000, 0]);
  }
  assert.deepEqual(
    ram(context),
    beforeRam,
    `${compiled.name}: every mapped guest RAM byte unchanged`,
  );
  assert.deepEqual(arena(context), expected, 'RAM diagnostics preserve the complete arena');
  counts.runs++;
  counts.fullRamPairs++;
  if (!retired) counts.controls++;
  observations.push({
    name: compiled.name,
    action: action.kind,
    budget,
    cancel,
    reason,
    retired,
    pc: expected.readUInt32LE(48),
    tsc: context.tsc,
    ramBytes: beforeRam.length,
  });
}
function assertRefusal(context, bytes, name) {
  const beforeArtifact = Buffer.from(
    new Uint8Array(context.memory.buffer, context.last.pointer, context.last.length),
  );
  upload(context, PC, Buffer.from([...bytes, 0xeb, 0xfe]));
  put(context, T, words(context.entries ? [PC] : [PC, bytes.length + 2]));
  const before = arena(context),
    beforeRam = ram(context);
  const method = context.resident
    ? context.entries
      ? 'compile_resident_entries'
      : 'compile_resident'
    : context.entries
      ? 'compile_entries'
      : 'compile';
  assert.equal(
    context.api[method](1, ...(context.entries ? [0] : [])),
    10,
    `${name}: strict refusal`,
  );
  assert.deepEqual(arena(context), before, `${name}: refusal preserves the complete arena`);
  assert.deepEqual(
    Buffer.from(new Uint8Array(context.memory.buffer, context.last.pointer, context.last.length)),
    beforeArtifact,
    `${name}: failed capture preserves prior artifact`,
  );
  assert.deepEqual(ram(context), beforeRam, `${name}: refusal has no RAM effects`);
  assert.deepEqual(arena(context), before);
  counts.refusals++;
}
const rows = [];
for (let register = 0; register < 8; register++) {
  for (const prefix of [[], [0x66]])
    rows.push({
      kind: 'identity',
      register,
      bytes: [...prefix, 0xf3, 0x0f, 0xc7, 0xf8 | register],
      name: `r${register}/${prefix.length ? 'ignored66' : 'plain'}`,
    });
}
for (const [index, prefix] of [
  [0x67],
  [0x26],
  [0x2e],
  [0x36],
  [0x3e],
  [0x64],
  [0x65],
  [0x66, 0x67, 0x65],
].entries())
  rows.push({
    kind: 'identity',
    register: index,
    bytes: [...prefix, 0xf3, 0x0f, 0xc7, 0xf8 | index],
    name: `r${index}/prefix-${prefix.map((v) => v.toString(16)).join('-')}`,
  });
assert.equal(rows.length, 24);
const schedules = [
  {},
  { flags: 0x244cd7 },
  { flags: 0x102 },
  { flags: 0x102, shadow: 1 },
  { flags: 0x102, shadow: 1, budget: 0 },
  { flags: 0x102, shadow: 1, cancel: 1 },
];
const timestamp = { kind: 'tsc', bytes: [0x0f, 0x31] };
const auxiliary = { kind: 'aux', bytes: [0x0f, 0x01, 0xf9] };
for (const resident of [false, true])
  for (const entries of [false, true]) {
    const context = open(resident, entries);
    // baseline control must succeed before the first intended RDPID capture RED.
    const control = capture(context, 'unchanged-rdtscp-control', auxiliary.bytes, true);
    execute(context, control, auxiliary);
    for (const row of rows) {
      const compiled = capture(context, row.name, row.bytes);
      for (const schedule of schedules) execute(context, compiled, row, schedule);
      context.last = compiled;
    }
    const reverse66 = { kind: 'identity', register: 4, bytes: [0xf3, 0x66, 0x0f, 0xc7, 0xfc] };
    context.last = capture(context, 'ignored66-after-f3', reverse66.bytes);
    execute(context, context.last, reverse66);
    // A mapped following branch makes capture valid; native decode separately proves
    // that the complete register instruction itself needs no next-page fetch.
    call(context, 'map', [0x2000, 1, 7]);
    context.pages.push(0x2000);
    const final = { kind: 'identity', register: 7, bytes: [0xf3, 0x0f, 0xc7, 0xff] };
    context.last = capture(context, 'identity-ends-at-page-boundary', final.bytes, false, 0x1ffc);
    execute(context, context.last, final);
    for (const register of [0, 1, 2, 4]) {
      const identity = { kind: 'identity', register, bytes: [0xf3, 0x0f, 0xc7, 0xf8 | register] };
      const actions = [timestamp, identity, auxiliary, timestamp];
      const compiled = capture(
        context,
        `counter-chain-r${register}`,
        actions.flatMap((action) => action.bytes),
        true,
      );
      seed(context, compiled.pc, 0x244cd7, 0);
      for (const action of actions) execute(context, compiled, action, {}, false);
      context.last = compiled;
    }
    for (let register = 0; register < 8; register++) {
      for (const [name, bytes] of [
        ['rdseed', [0x0f, 0xc7, 0xf8 | register]],
        ['rdrand', [0x0f, 0xc7, 0xf0 | register]],
        ['lock-rdpid', [0xf0, 0xf3, 0x0f, 0xc7, 0xf8 | register]],
      ])
        assertRefusal(context, bytes, `${name}/r${register}`);
    }
    for (const [name, bytes] of [
      ['memory', [0xf3, 0x0f, 0xc7, 0x38]],
      ['memory66', [0x66, 0xf3, 0x0f, 0xc7, 0x38]],
      ['duplicate-f3', [0xf3, 0xf3, 0x0f, 0xc7, 0xf8]],
      ['duplicate66', [0x66, 0x66, 0xf3, 0x0f, 0xc7, 0xf8]],
    ])
      assertRefusal(context, bytes, name);
    call(context, 'close');
  }
assert.deepEqual(counts, {
  contexts: 4,
  modules: 124,
  runs: 652,
  identities: 408,
  timestamps: 52,
  controls: 192,
  refusals: 112,
  fullRamPairs: 652,
});
assert.deepEqual(pins(), sourcePins, 'identity proof sources stayed frozen');
writeFileSync(
  join(output, 'result.json'),
  JSON.stringify(
    {
      scope:
        'IA32 processor identity, AUX coherence and absence of timestamp events; no host-core/timing/kernel claim',
      engineSha256: engine.sha256,
      sourcePins,
      counts,
      modules,
      observations,
    },
    null,
    2,
  ) + '\n',
);
console.log(JSON.stringify({ status: 'PASS', ...counts }));
