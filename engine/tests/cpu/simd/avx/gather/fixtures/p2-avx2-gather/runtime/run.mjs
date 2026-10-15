import {pageFaultExit} from '../../../../../../../support/wasm/page-fault.mjs';
import {ARENA_SIZE} from '../../../../../../../support/wasm/arena.mjs';
import {generatedImports} from '../../../../../../../support/wasm/binding.mjs';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { readEngine } from '../../../../../../../support/wasm/engine.mjs';
import { assertEngineExports } from '../../../../../../../support/wasm/exports.mjs';
import {
  gatherRows,
  gatherSourcePaths,
  encode,
  outcome,
  selected,
  laneOffset,
  pattern,
} from './cases.mjs';

const [enginePath, output, root] = process.argv.slice(2);
const workPreflightOnly = process.argv.includes('--work-preflight-only');
const engine = readEngine(enginePath),
  exportsReceipt = assertEngineExports(engine.module, root);
const PC = 0x3000,
  T = 140,
  H = 100,
  SV = 4364,
  YU = 4612,
  SG = 4524,
  WK = 4588,
  SIZE = ARENA_SIZE;
const sourcePaths = ['engine/tests/support/wasm/page-fault.mjs', 'engine/tests/support/wasm/arena.mjs', 'engine/tests/support/wasm/binding.mjs',
  ...exportsReceipt.sourcePaths,
  ...gatherSourcePaths,
  'engine/tests/support/wasm/engine.mjs',
  'engine/src/cpu/x86/avx/operation.rs',
  'engine/src/cpu/x86/avx/mod.rs',
  'engine/src/cpu/x86/avx/operation/factory.rs',
  'engine/src/cpu/x86/decode/avx/mod.rs',
  'engine/src/cpu/x86/avx/dispatch.rs',
  'engine/src/cpu/x86/capabilities.rs',
  'engine/src/abi/x86/avx.rs',
  'engine/src/abi/x86/simd.rs',
  'engine/src/abi/x86/ymm.rs',
  'engine/src/abi/x86/segments.rs',
  'engine/src/abi/x86/state.rs',
  'engine/src/abi/x86/exit.rs',
  'engine/src/cpu/dbt/wasm/instructions/avx/mod.rs',
  'engine/src/cpu/dbt/wasm/instructions/avx/locals.rs',
  'engine/src/cpu/dbt/wasm/instructions/avx/execute.rs',
  'engine/src/cpu/dbt/wasm/instructions/avx/direct.rs',
  'engine/src/cpu/dbt/wasm/instructions/avx/moves.rs',
  'engine/src/cpu/x86/avx/moves.rs',
  'engine/src/cpu/x86/decode/avx/moves.rs',
  'engine/src/cpu/x86/avx/operation/factory/moves.rs',
  'engine/src/cpu/dbt/wasm/abi.rs',
  'engine/src/cpu/dbt/wasm/imports/mod.rs', 'engine/src/cpu/dbt/wasm/imports/binding.rs', 'engine/src/cpu/dbt/wasm/imports/context.rs', 'engine/src/cpu/dbt/wasm/imports/needs.rs', 'engine/src/cpu/dbt/wasm/helpers/protocol.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/mod.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/address.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/scalar.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/stack.rs',
  'engine/src/cpu/dbt/wasm/emitter/mod.rs', 'engine/src/cpu/dbt/wasm/emitter/module.rs', 'engine/src/cpu/dbt/wasm/emitter/body.rs', 'engine/src/cpu/dbt/wasm/requirements.rs', 'engine/src/cpu/dbt/wasm/imports/wiring.rs',
  'engine/src/process/cpu/avx.rs',
  'engine/src/process/execution/authority.rs',
  'engine/src/memory/space.rs', 'engine/src/memory/space/atomic.rs',
  'engine/src/cpu/x86/segments/cache.rs',
];
assert.equal(new Set(sourcePaths).size, sourcePaths.length, 'every source pin has one owner');
const hash = (value) => createHash('sha256').update(value).digest('hex');
const pins = () =>
  Object.fromEntries(sourcePaths.map((path) => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = pins(),
  counts = {
    contexts: 0,
    modules: 0,
    runs: 0,
    controls: 0,
    partial: 0,
    resumed: 0,
    protocol: 0,
    fullArena: 0,
    fullRam: 0,
  };
const frames = [],
  ramFrames = [],
  rows = [];
function words(values) {
  const bytes = Buffer.alloc(values.length * 4);
  values.forEach((value, index) => bytes.writeUInt32LE(value >>> 0, index * 4));
  return bytes;
}
function record(magic, version, size, fields = []) {
  const bytes = Buffer.alloc(size);
  bytes.write(magic);
  bytes.writeUInt16LE(version, 4);
  bytes.writeUInt16LE(1, 6);
  bytes.writeUInt32LE(size, 8);
  words(fields).copy(bytes, 16);
  return bytes;
}
function state(registers, pc, flags, shadow) {
  const bytes = record('R3ST', 2, 56, [...registers, pc, flags]);
  bytes.writeUInt32LE(+shadow, 12);
  return bytes;
}
function arena(c) {
  return Buffer.from(new Uint8Array(c.memory.buffer, c.base, SIZE));
}
function input(c, offset, bytes) {
  new Uint8Array(c.memory.buffer).set(bytes, c.base + offset);
}
function vector(bytes, register) {
  return Buffer.concat([
    bytes.subarray(SV + 32 + register * 16, SV + 48 + register * 16),
    bytes.subarray(YU + 16 + register * 16, YU + 32 + register * 16),
  ]);
}
function publish(bytes, register, value) {
  value.copy(bytes, SV + 32 + register * 16, 0, 16);
  value.copy(bytes, YU + 16 + register * 16, 16, 32);
}
function host(c, name, args = []) {
  assert.equal(c.api[name](...args), 0, name);
}
function preserve(c, action) {
  const before = arena(c);
  action();
  input(c, 0, before);
}
function map(c, address, permissions = 3) {
  host(c, 'map', [address, 1, permissions]);
  c.pages.set(address, { bytes: Buffer.alloc(4096), permissions });
}
function upload(c, address, bytes) {
  input(c, T, bytes);
  host(c, 'upload', [address, bytes.length]);
  for (let at = 0; at < bytes.length; at++)
    c.pages.get((address + at) & 0xfffff000).bytes[(address + at) & 4095] = bytes[at];
}
function modelRead(c, address, width) {
  const bytes = Buffer.alloc(width);
  for (let at = 0; at < width; at++) {
    const current = (address + at) >>> 0,
      page = c.pages.get((current & 0xfffff000) >>> 0);
    if (!page) return { fault: true, detail: 1, address: current };
    if (!(page.permissions & 1)) return { fault: true, detail: 2, address: current };
    bytes[at] = page.bytes[current & 4095];
  }
  return { bytes };
}
function ram(c) {
  const before = arena(c),
    buffers = [];
  for (const [address, page] of [...c.pages].sort(([a], [b]) => a - b)) {
    if (!(page.permissions & 1)) host(c, 'protect', [address, 1, page.permissions | 1]);
    const bytes = Buffer.alloc(4096);
    for (let offset = 0; offset < 4096; offset += 512) {
      host(c, 'cpu_read', [address + offset, 512, 1]);
      assert.equal(arena(c).readUInt32LE(H + 16), 0, 'mapped diagnostic RAM read');
      arena(c).copy(bytes, offset, T, T + 512);
    }
    if (!(page.permissions & 1)) host(c, 'protect', [address, 1, page.permissions]);
    assert.deepEqual(bytes, page.bytes, 'independent full mapped RAM image');
    buffers.push(bytes);
  }
  input(c, 0, before);
  return Buffer.concat(buffers);
}
function segmentRecord(row) {
  const bytes = record('R3SG', 2, 64),
    segment = row.segment;
  [0x23, 0x1b, 0x23, 0x23, 0x3b, 0].forEach((selector, index) =>
    bytes.writeUInt16LE(selector, 16 + index * 2),
  );
  const caches = [0xf33fffff, 0xf3bfffff, 0xf33fffff, 0xf33fffff, 0xf33fffff, 0];
  if (segment.null) {
    bytes.writeUInt16LE(0, 16 + segment.id * 2);
    caches[segment.id] = 0;
  } else if (segment.code) caches[segment.id] = 0xf3bfffff;
  else if (segment.limit !== 0xffffffff) {
    assert.ok(segment.limit <= 0xfffff);
    caches[segment.id] = 0xf2300000 + segment.limit;
  }
  [28, 32, 36, 40, 44, 56].forEach((at, index) => bytes.writeUInt32LE(caches[index], at));
  if (segment.id === 4) bytes.writeUInt32LE(segment.base, 48);
  return bytes;
}
function legacyPacket(value) {
  return record('R3GH', 1, 40, [value.status, value.detail, value.address,
    value.access, value.completed, value.lane]);
}
function packet(value) {
  if (value.status !== 1) return legacyPacket(value);
  assert.ok([1, 2].includes(value.detail), 'declared native read fault');
  return record('R3GH', 2, 40, [9, 4 | Number(value.detail === 2), value.address,
    value.access, value.completed, value.lane]);
}
function request(row, value) {
  return record('R3GA', 1, 24, [row.segment.id, value.base]);
}
function open(owner, entries) {
  const instance = new WebAssembly.Instance(engine.module, {}),
    api = {};
  for (const { name, arity } of exportsReceipt.functions) {
    assert.equal(instance.exports[name].length, arity, name);
    api[name.slice(13)] = instance.exports[name];
  }
  for (const [name, arity] of Object.entries({
    avx_execute: 6,
    avx_execute_resident: 10,
    cpu_read: 3,
    compile: 1,
    compile_entries: 2,
    compile_resident: 1,
    compile_resident_entries: 2,
  }))
    assert.equal(api[name].length, arity, name);
  const c = {
    owner,
    entries,
    memory: instance.exports.memory,
    api,
    key: ++counts.contexts,
    high: 0x47415448,
    pages: new Map(),
    calls: [],
    reads: [],
  };
  host(c, 'open', [8, c.key, c.high]);
  c.base = api.arena_ptr() >>> 0;
  assert.equal(api.arena_len(), SIZE);
  c.initial = arena(c);
  for (const address of [0, 0x1000, PC, 0x5000, 0x8000]) map(c, address, address === PC ? 7 : 3);
  return c;
}
function reset(c, row) {
  if (c.pages.has(0x6000)) {
    host(c, 'unmap', [0x6000, 1]);
    c.pages.delete(0x6000);
  }
  for (const [address, page] of c.pages) {
    if (address === PC) continue;
    host(c, 'protect', [address, 1, 3]);
    page.permissions = 3;
    for (let at = 0; at < 4096; at += 512)
      upload(
        c,
        address + at,
        Buffer.from(
          Array.from(
            { length: 512 },
            (_, index) => ((address / 4096) * 23 + at + index * 37) & 255,
          ),
        ),
      );
  }
  if (row.deniedPage) {
    host(c, 'protect', [row.deniedPage, 1, 2]);
    c.pages.get(row.deniedPage).permissions = 2;
  }
}
function compile(c, row) {
  if (c.resident) {
    input(c, 96, words([0]));
    host(c, 'discard_unacknowledged_resident', [c.key, c.high, ...c.resident]);
  }
  const gather = encode(row),
    prefix =
      row.priorBase === undefined
        ? Buffer.alloc(0)
        : Buffer.concat([Buffer.from([0xb8 + row.base]), words([row.priorBase])]);
  const bytes = Buffer.concat([
    prefix,
    gather,
    row.continuation
      ? Buffer.from('c5fe6fb080000000', 'hex')
      : row.mixed
        ? Buffer.from('f3ac', 'hex')
        : Buffer.alloc(0),
    Buffer.from('ebfe', 'hex'),
  ]);
  upload(c, PC, bytes);
  input(c, T, words(c.entries ? [PC] : [PC, bytes.length]));
  const name =
    c.owner === 'resident'
      ? c.entries
        ? 'compile_resident_entries'
        : 'compile_resident'
      : c.entries
        ? 'compile_entries'
        : 'compile';
  host(c, name, [1, ...(c.entries ? [0] : [])]);
  const frame = arena(c);
  if (c.owner === 'resident') c.resident = [frame.readUInt32LE(T + 8), frame.readUInt32LE(T + 12)];
  const pointer = c.owner === 'resident' ? frame.readUInt32LE(T + 16) : c.api.module_ptr() >>> 0;
  const length = c.owner === 'resident' ? frame.readUInt32LE(T + 20) : c.api.module_len() >>> 0;
  const binary = Buffer.from(new Uint8Array(c.memory.buffer, pointer, length)),
    module = new WebAssembly.Module(binary);
  const helper = c.owner === 'resident' ? 'avx_execute_resident' : 'avx_execute';
  const imports = generatedImports({resident: c.owner === 'resident', helpers: [
    ...(row.mixed ? ['cpu_read8'] : []), ...(row.continuation ? ['cpu_read'] : []), helper
  ]}).map(i => `${i.module}:${i.name}:${i.kind}`).sort();
  assert.deepEqual(
    WebAssembly.Module.imports(module)
      .map((item) => `${item.module}:${item.name}:${item.kind}`)
      .sort(),
    imports,
    `${row.name}: exact gather imports/no contiguous read`,
  );
  assert.deepEqual(WebAssembly.Module.exports(module), [{ name: 'run', kind: 'function' }]);
  writeFileSync(join(output, `module-${++counts.modules}.wasm`), binary, { flag: 'wx' });
  const ring3 = { ...c.api };
  ring3[helper] = (...args) => {
    const before = arena(c),
      value = c.expectedOutcome;
    assert.deepEqual(
      before.subarray(T + 544, T + 568),
      request(c.row, value),
      'exact unchecked R3GA including zero-mask base0',
    );
    const guestArgs = [
      0x01001400 | c.row.geometry.id,
      c.row.destination | (c.row.indexRegister << 3) | (c.row.mask << 9) | (1 << 12),
      Math.log2(c.row.scale),
      0,
      0,
      before.readUInt32LE(52),
    ];
    assert.deepEqual(
      args.slice(-6).map((value) => value >>> 0),
      guestArgs,
      'canonical descriptor/roles/scale/live0/live1/flags',
    );
    if (c.owner === 'resident')
      assert.deepEqual(
        args.slice(0, 4).map((value) => value >>> 0),
        [c.key, c.high, ...c.resident],
      );
    const expected = Buffer.from(before),
      staged = c.synthetic?.stage ?? Buffer.concat([value.value, value.mask]),
      result = c.synthetic?.packet ?? packet(value);
    expected.set(staged, T);
    expected.set(result, H);
    let status;
    if (c.synthetic) {
      input(c, T, staged);
      input(c, H, result);
      status = c.synthetic.hostStatus ?? 0;
    } else status = c.api[helper](...args);
    assert.equal(status, c.synthetic ? c.synthetic.hostStatus ?? 0 : value.status === 1 ? 23 : 0, 'normal helper host transport');
    assert.deepEqual(
      arena(c),
      expected,
      'helper stages outputs only; complete4796B image has no early vector/GPR/flags/RAM publication',
    );
    c.calls.push({ args: args.map((value) => value >>> 0), status });
    return status;
  };
  if (row.continuation)
    ring3.cpu_read = (...args) => {
      const before = arena(c),
        expected = Buffer.from(before),
        address = c.row.baseValue + 0x80;
      assert.deepEqual(
        args.map((value) => value >>> 0),
        [address, 32, 1],
        'following ordinary AVX exact read',
      );
      const read = modelRead(c, address, 32);
      assert.ok(!read.fault);
      expected.set(read.bytes, T);
      expected.set(record('R3MH', 7, 40, [0, 0, 0, 0, 0, 32]), H);
      const status = c.api.cpu_read(...args);
      assert.equal(status, 0);
      assert.deepEqual(arena(c), expected, 'ordinary read helper scratch-only full frame');
      c.reads.push(args.map((value) => value >>> 0));
      return status;
    };
  if (row.mixed)
    ring3.cpu_read8 = () => {
      assert.fail('budget must stop before REP after gather');
    };
  return {
    run: new WebAssembly.Instance(module, { env: { memory: c.memory }, ring3 }).exports.run,
    next: PC + prefix.length + gather.length + (row.continuation ? 8 : 0),
    gatherPC: PC + prefix.length,
    prior: +Boolean(prefix.length),
  };
}
function seed(c, row) {
  input(c, 0, c.initial);
  const registers = [0x5000, 0, 0x22334455, 0x33445566, 0x7780, 0x55667788, 0x66778899, 0x778899aa];
  if (row.base !== null) registers[row.base] = row.baseValue;
  input(c, 0, state(registers, PC, row.flags ?? 0x244cd7, row.shadow ?? false));
  input(c, SG, segmentRecord(row));
  input(c, SV + 16, words([0xe5af]));
  for (let register = 0; register < 8; register++) {
    const frame = arena(c);
    publish(frame, register, pattern(register));
    input(c, 0, frame);
  }
  const frame = arena(c);
  publish(frame, row.destination, row.destinationBytes);
  publish(frame, row.indexRegister, row.indexes);
  publish(frame, row.mask, row.masks);
  // bound application stops retain a valid work record; the gather helper never changes it.
  frame.set(record('R3WK', 1, 24, [row.mixed ? 27 : 0, 0]), WK);
  frame.fill(0xa5, T + 512, T + 544);
  frame.fill(0x5a, T + 544, T + 568);
  input(c, 0, frame);
}
function execute(
  c,
  compiled,
  row,
  {
    budget = 1 + compiled.prior + +Boolean(row.continuation),
    cancel = false,
    resume = false,
    synthetic = null,
  } = {},
) {
  if (!resume) seed(c, row);
  if (cancel) input(c, 96, words([1]));
  const before = arena(c),
    beforeRam = ram(c),
    active = budget > 0 && !cancel;
  const registers = Array.from({ length: 8 }, (_, index) => before.readUInt32LE(16 + index * 4));
  if (active && row.priorBase !== undefined && !resume) registers[row.base] = row.priorBase;
  const value = outcome(
    row,
    vector(before, row.destination),
    vector(before, row.mask),
    vector(before, row.indexRegister),
    registers,
    (address, width) => modelRead(c, address, width),
  );
  c.expectedOutcome = value;
  c.row = row;
  c.synthetic = synthetic;
  const expected = Buffer.from(before),
    flags = before.readUInt32LE(52),
    prior = resume ? 0 : compiled.prior;
  const accepted = active && !synthetic?.invalid;
  const retired = active
    ? prior + (accepted && value.status === 0 ? 1 + +Boolean(row.continuation) : 0)
    : 0;
  const reason = !active
    ? cancel
      ? 2
      : 1
    : synthetic?.invalid
      ? 7
      : value.status === 1
        ? 5
        : value.status === 3
          ? 11
          : value.status === 7
            ? 15
            : flags & 0x100
              ? 16
              : 1;
  const detail = synthetic?.invalid
    ? synthetic.hostStatus
      ? 3
      : 2
    : value.status === 1
      ? value.detail
      : 0;
  const pc = !active
    ? before.readUInt32LE(48)
    : accepted && value.status === 0
      ? compiled.next
      : compiled.gatherPC;
  expected.set(state(registers, pc, flags, active ? false : before.readUInt32LE(12) === 1));
  expected.set(
    reason === 5 && !synthetic ? pageFaultExit({retired, errorCode: 4 | Number(value.detail === 2),
      address: value.address, access: 1, length: row.geometry.data})
    : record('R3EX', reason === 16 ? 7 : 6, 40, [
      reason, retired, active ? detail : 0,
      active && !synthetic?.invalid && value.status === 1 ? value.address : 0,
      active && !synthetic?.invalid && value.status === 1 ? 1 : 0,
      active && !synthetic?.invalid && value.status === 1 ? row.geometry.data : 0,
    ]),
    56,
  );
  if (active) {
    expected.set(request(row, value), T + 544);
    expected.set(synthetic?.stage ?? Buffer.concat([value.value, value.mask]), T);
    expected.set(synthetic?.packet ?? packet(value), H);
    if (accepted) {
      publish(expected, row.destination, value.value);
      publish(expected, row.mask, value.mask);
    }
  }
  const followed = active && accepted && value.status === 0 && row.continuation;
  if (followed) {
    const read = modelRead(c, row.baseValue + 0x80, 32);
    assert.ok(!read.fault);
    publish(expected, 6, read.bytes);
    expected.set(read.bytes, T);
    expected.set(record('R3MH', 7, 40, [0, 0, 0, 0, 0, 32]), H);
  }
  if (row.mixed) expected.writeUInt32LE(retired, WK + 16);
  const first = c.calls.length,
    firstRead = c.reads.length;
  assert.equal(compiled.run(c.base, c.base + 56, budget, c.base + 96), 0, row.name);
  assert.deepEqual(arena(c), expected, `${row.name}: full4796B arena`);
  assert.equal(
    c.calls.length - first,
    +active,
    'one gather helper even zero mask; none for budget/cancel',
  );
  assert.equal(
    c.reads.length - firstRead,
    +Boolean(followed),
    'following ordinaryAVX reached once only after success',
  );
  assert.deepEqual(ram(c), beforeRam, `${row.name}: all mapped RAM and permissions unchanged`);
  assert.deepEqual(arena(c), expected, 'RAM observations preserve exact frame');
  frames.push(before, arena(c));
  ramFrames.push(beforeRam, ram(c));
  rows.push({
    context: c.key,
    frameBefore: frames.length - 2,
    frameAfter: frames.length - 1,
    ramBefore: ramFrames.length - 2,
    ramAfter: ramFrames.length - 1,
    ramBytes: beforeRam.length,
    mappedPages: [...c.pages.keys()].sort((a, b) => a - b),
    name: row.name,
    budget,
    cancel,
    resume,
    synthetic: synthetic?.name,
    reason: reason === 5 && !synthetic ? 23 : reason,
    retired,
    completed: active ? value.completed : 0,
    faultLane: active ? value.lane : null,
  });
  counts.runs++;
  counts.fullArena++;
  counts.fullRam++;
  if (!active) counts.controls++;
  if (active && value.status && !synthetic) counts.partial++;
  if (resume) counts.resumed++;
  if (synthetic?.invalid) counts.protocol++;
  c.synthetic = null;
  return value;
}
function repair(c, row, value) {
  const fixed = { ...row, segment: { ...row.segment } };
  preserve(c, () => {
    if (row.repair.map) {
      map(c, row.repair.map);
      upload(
        c,
        row.repair.map,
        Buffer.from(Array.from({ length: 4096 }, (_, byte) => (byte * 41 + 0x73) & 255)),
      );
    }
    if (row.repair.permission) {
      host(c, 'protect', [row.repair.permission, 1, 3]);
      c.pages.get(row.repair.permission).permissions = 3;
    }
    if (row.repair.changeCleared) {
      assert.ok(value.completed & 1, 'repair case must have completed first lane');
      const at = (laneOffset(row, value.base, row.indexes, 0) + row.segment.base) >>> 0;
      upload(c, at >>> 0, Buffer.alloc(row.geometry.data, 0xee));
    }
  });
  if (row.repair.limit !== undefined) {
    fixed.segment.limit = row.repair.limit;
    input(c, SG, segmentRecord(fixed));
  }
  return fixed;
}
function rejectInvalidWork(c, compiled, row) {
  seed(c, row);
  input(c, WK, Buffer.alloc(24, 0xa5));
  const before = arena(c), beforeRam = ram(c), calls = c.calls.length, reads = c.reads.length;
  assert.equal(compiled.run(c.base, c.base + 56, 1, c.base + 96), 2, 'invalid shared work record fails ABI preflight');
  assert.deepEqual(arena(c), before, 'invalid work preserves the entire arena before retirement');
  assert.equal(c.calls.length, calls, 'invalid work invokes no gather helper');
  assert.equal(c.reads.length, reads, 'invalid work invokes no following operand helper');
  assert.deepEqual(ram(c), beforeRam, 'invalid work preserves all mapped RAM');
  assert.deepEqual(arena(c), before, 'RAM inspection preserves the invalid input arena');
  frames.push(before, arena(c)); ramFrames.push(beforeRam, ram(c));
  rows.push({context: c.key, name: 'invalid-work-precedes-gather-and-retirement', status: 2,
    retired: 0, completed: 0, frameBefore: frames.length - 2, frameAfter: frames.length - 1,
    ramBefore: ramFrames.length - 2, ramAfter: ramFrames.length - 1, ramBytes: beforeRam.length});
  counts.runs++; counts.controls++; counts.fullArena++; counts.fullRam++;
}
// injected legacy replies remain lowerer protocol controls, separate from native typed faults.
function corruptions(row, value) {
  const good = legacyPacket(value),
    stage = Buffer.concat([value.value, value.mask]),
    result = [];
  const field = (name, offset, number) => {
    const bytes = Buffer.from(good);
    bytes.writeUInt32LE(number >>> 0, offset);
    result.push({ name, packet: bytes, stage, invalid: true });
  };
  field('magic', 0, 0);
  field('version', 4, 0x10002);
  field('length', 8, 36);
  field('reserved', 12, 1);
  field('unknown-status', 16, 2);
  field('wrong-segment-status', 16, row.segment.id === 2 ? 3 : 7);
  field('read-access', 28, 2);
  field('fault-lane-outside', 36, row.geometry.lanes);
  field('fault-lane-inactive', 36, 2);
  field('completed-hole', 32, 0);
  field('completed-higher', 32, value.completed | 4);
  field('completed-outside', 32, 1 << row.geometry.lanes);
  field('fault-address-outside-lane', 24, 0x7000);
  field('detail-overflow-without-overflow', 20, 3);
  for (const [name, at] of [
    ['pending-destination', row.geometry.data],
    ['inactive-mask', 32 + row.geometry.data * 2],
    ['completed-mask', 32],
    ['pending-mask', 32 + row.geometry.data],
    ['unused-destination-upper', 16],
    ['unused-mask-upper', 48],
  ]) {
    const bytes = Buffer.from(stage);
    bytes[at] ^= 1;
    result.push({ name, packet: good, stage: bytes, invalid: true });
  }
  const success = {
    ...value,
    status: 0,
    detail: 0,
    address: 0,
    access: 0,
    completed: selected(row.masks, row.geometry),
    lane: 0xffffffff,
  };
  for (const [name, offset, number] of [
    ['success-incomplete-selection', 32, 1],
    ['success-fault-lane', 36, 0],
    ['success-nonzero-address', 24, 0x6000],
  ]) {
    const bytes = legacyPacket(success);
    bytes.writeUInt32LE(number >>> 0, offset);
    result.push({
      name,
      packet: bytes,
      stage: Buffer.concat([value.value, Buffer.alloc(32)]),
      invalid: true,
    });
  }
  result.push({
    name: 'host-completion11-forbidden',
    packet: good,
    stage,
    hostStatus: 11,
    invalid: true,
  });
  return result;
}
for (const owner of ['replacement', 'resident'])
  for (const entries of [false, true]) {
    const c = open(owner, entries);
    for (const row of workPreflightOnly ? [] : gatherRows) {
      reset(c, row);
      const compiled = compile(c, row);
      execute(c, compiled, row, { budget: 0 });
      execute(c, compiled, row, { cancel: true });
      const value = execute(c, compiled, row);
      if (row.repair) {
        assert.notEqual(value.status, 0);
        const fixed = repair(c, row, value);
        execute(c, compiled, fixed, { resume: true });
      }
    }
    if (!workPreflightOnly) {
      const row = {
        ...gatherRows.find((row) => row.name === 'tf-partial-fault-before-retirement'),
        flags: 0x244cd7,
        shadow: false,
      };
      reset(c, row);
      const compiled = compile(c, row);
      seed(c, row);
      const registers = Array.from({ length: 8 }, (_, index) =>
        arena(c).readUInt32LE(16 + index * 4),
      );
      const value = outcome(
        row,
        vector(arena(c), row.destination),
        vector(arena(c), row.mask),
        vector(arena(c), row.indexRegister),
        registers,
        (address, width) => modelRead(c, address, width),
      );
      assert.equal(value.completed, 1);
      assert.equal(value.lane, 1);
      execute(c, compiled, row, {
        synthetic: {
          name: 'truthful-staged-positive-control',
          packet: legacyPacket(value),
          stage: Buffer.concat([value.value, value.mask]),
        },
      });
      for (const synthetic of corruptions(row, value)) execute(c, compiled, row, { synthetic });
      const mixedRow = gatherRows.find((row) => row.name === 'geometry-5-partial-page-repair-resume');
      reset(c, mixedRow);
      const mixedCompiled = compile(c, mixedRow);
      seed(c, mixedRow);
      const mixedRegisters = Array.from({ length: 8 }, (_, index) =>
        arena(c).readUInt32LE(16 + index * 4),
      );
      const mixedValue = outcome(
        mixedRow,
        vector(arena(c), mixedRow.destination),
        vector(arena(c), mixedRow.mask),
        vector(arena(c), mixedRow.indexRegister),
        mixedRegisters,
        (address, width) => modelRead(c, address, width),
      );
      const mixedStage = Buffer.concat([mixedValue.value, mixedValue.mask]);
      mixedStage[8] = 1;
      execute(c, mixedCompiled, mixedRow, {
        synthetic: {
          name: 'mixed-unused-destination-must-be-zero-on-fault',
          packet: legacyPacket(mixedValue),
          stage: mixedStage,
          invalid: true,
        },
      });
    }
    const workRow = gatherRows.find(row => row.name === 'mixed-rep-success-charges-one-work-unit');
    reset(c, workRow);
    rejectInvalidWork(c, compile(c, workRow), workRow);
    host(c, 'close');
  }
assert.deepEqual(counts, workPreflightOnly ? {
  contexts: 4, modules: 4, runs: 4, controls: 4, partial: 0, resumed: 0, protocol: 0, fullArena: 4, fullRam: 4,
} : {
  contexts: 4,
  modules: 240,
  runs: 840,
  controls: 460,
  partial: 68,
  resumed: 48,
  protocol: 100,
  fullArena: 840,
  fullRam: 840,
});
assert.deepEqual(pins(), sourcePins, 'all proof sources remain current');
writeFileSync(join(output, 'frames.bin'), Buffer.concat(frames));
writeFileSync(join(output, 'ram.bin'), Buffer.concat(ramFrames));
writeFileSync(
  join(output, 'result.json'),
  JSON.stringify(
    {
      scope:
        'protected32 synchronous RAM gather; ascending completion and strict staged protocol, no hardware event/RF claim',
      engineSha256: engine.sha256,
      counts,
      sourcePins,
      rows,
    },
    null,
    2,
  ) + '\n',
);
console.log(JSON.stringify({ status: 'PASS', ...counts }));
