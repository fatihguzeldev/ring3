import {pageFaultExit, pageFaultPacket} from '../../../../../../../support/wasm/page-fault.mjs';
import {ARENA_SIZE} from '../../../../../../../support/wasm/arena.mjs';
import {generatedImports} from '../../../../../../../support/wasm/binding.mjs';
import assert from 'node:assert/strict';
import {logicalRows} from '../../../../logical/fixtures/p2-avx2-logical/runtime/cases.mjs';
import {comparisonRows} from '../../../../integer/compare/fixtures/p2-avx2-comparison/runtime/cases.mjs';
import {integerRows} from '../../../../integer/fixtures/p2-avx2-integer/runtime/cases.mjs';
import {blocksRows} from '../../../../blocks/fixtures/p2-avx2-blocks/runtime/cases.mjs';
import {broadcastRows} from '../../../../broadcast/fixtures/p2-avx2-broadcast/runtime/cases.mjs';
import {maskedRows} from '../../../../masked/fixtures/p2-avx2-masked/runtime/cases.mjs';
import {uniformRows} from '../../../../shift/uniform/fixtures/p2-avx2-shift/runtime/cases.mjs';
import {variableRows} from '../../../../shift/variable/fixtures/p2-avx2-variable/runtime/cases.mjs';
import {widenRows} from '../../../../rearrange/widen/fixtures/p2-avx2-widen/runtime/cases.mjs';
import {laneRows} from '../../../../rearrange/lanes/fixtures/p2-avx2-lanes/runtime/cases.mjs';
import {multiplyRows} from '../../../../multiply/fixtures/p2-avx2-multiply/runtime/cases.mjs';
import {wholeRows} from '../../../../permute/whole/fixtures/p2-avx2-permute/runtime/cases.mjs';
import {blendRows} from '../../../../permute/blend/fixtures/p2-avx2-blend/runtime/cases.mjs';
import {movemaskRows} from '../../../../scalar/mask/fixtures/p2-avx2-movemask/runtime/cases.mjs';
import {streamRows} from '../../../../moves/stream/fixtures/p2-avx2-stream/runtime/cases.mjs';
import {sadRows} from '../../../../multiply/sad/fixtures/p2-avx2-sad/runtime/cases.mjs';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../../../../../../../support/wasm/engine.mjs';
import {assertEngineExports} from '../../../../../../../support/wasm/exports.mjs';

const [enginePath, output, root] = process.argv.slice(2), engine = readEngine(enginePath);
const {sourcePaths: exportsSources} = assertEngineExports(engine.module, root);
const PC = 0x1000, T = 140, SV = 4364, YU = 4612, SIZE = ARENA_SIZE, keyHigh = 0x415658;
const counts = {contexts: 0, modules: 0, runs: 0, controls: 0, faults: 0, fullRamPairs: 0};
const sourcePaths = ['engine/tests/support/wasm/page-fault.mjs', 'engine/tests/support/wasm/arena.mjs', 'engine/tests/support/wasm/binding.mjs', ...exportsSources,
  'engine/tests/support/wasm/engine.mjs',
  'engine/src/cpu/x86/avx/operation.rs',
  'engine/src/cpu/x86/decode/avx/mod.rs',
  'engine/src/cpu/x86/capabilities.rs',
  'engine/src/cpu/dbt/wasm/instructions/avx/mod.rs',
  'engine/src/cpu/dbt/wasm/instructions/avx/direct.rs',
  'engine/src/cpu/dbt/wasm/instructions/avx/execute.rs',
  'engine/src/cpu/dbt/wasm/instructions/avx/preflight.rs',
  'engine/src/cpu/dbt/wasm/instructions/avx/locals.rs',
  'engine/src/cpu/dbt/wasm/emitter/mod.rs', 'engine/src/cpu/dbt/wasm/emitter/module.rs', 'engine/src/cpu/dbt/wasm/emitter/body.rs', 'engine/src/cpu/dbt/wasm/requirements.rs', 'engine/src/cpu/dbt/wasm/imports/wiring.rs',
  'engine/src/cpu/dbt/wasm/instructions/avx/moves.rs',
  'engine/src/cpu/dbt/wasm/instructions/avx/logical.rs',
  'engine/src/cpu/dbt/wasm/instructions/avx/permute.rs',
  'engine/src/cpu/dbt/wasm/instructions/avx/integer.rs',
  'engine/src/cpu/dbt/wasm/instructions/avx/rearrange.rs',
  'engine/src/cpu/dbt/wasm/instructions/avx/shift.rs',
  'engine/src/cpu/dbt/wasm/instructions/avx/multiply.rs',
  'engine/src/cpu/dbt/wasm/instructions/avx/scalar.rs',
  'engine/src/cpu/dbt/wasm/instructions/avx/cross_lane/mod.rs',
  'engine/src/cpu/dbt/wasm/instructions/simd.rs',
  'engine/src/cpu/dbt/wasm/instructions/simd/direct.rs',
  'engine/src/cpu/dbt/wasm/instructions/simd/direct/rearrange.rs',
  'engine/src/cpu/dbt/wasm/instructions/simd/direct/shift.rs',
  'engine/src/cpu/dbt/wasm/instructions/simd/direct/multiply.rs',
  'engine/src/cpu/dbt/wasm/instructions/simd/direct/scalar.rs',
  'engine/src/cpu/dbt/wasm/imports/mod.rs', 'engine/src/cpu/dbt/wasm/imports/binding.rs', 'engine/src/cpu/dbt/wasm/imports/context.rs', 'engine/src/cpu/dbt/wasm/imports/needs.rs', 'engine/src/cpu/dbt/wasm/helpers/protocol.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/mod.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/address.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/scalar.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/stack.rs',
  'engine/src/cpu/dbt/wasm/helpers/result.rs',
  'engine/src/process/memory/span.rs',
  'engine/src/abi/arena.rs',
  'engine/src/abi/x86/ymm.rs',
  'engine/src/process/cpu/avx.rs',
  'engine/src/process/execution/resident.rs',
  'engine/src/cpu/dbt/resident.rs',
  'engine/src/process/wasm/execution.rs', 'engine/src/abi/execution.rs',
  'engine/tests/cpu/simd/avx/execution/wasm.rs',
  'engine/tests/cpu/simd/avx/execution/fixtures/p2-avx-execution/runtime/run.mjs',
  'engine/src/cpu/x86/decode/avx/logical.rs',
  'engine/src/cpu/x86/avx/operation/factory/logical.rs',
  'engine/tests/cpu/simd/avx/logical/fixtures/p2-avx2-logical/runtime/cases.mjs',
  'engine/src/cpu/x86/decode/avx/integer.rs',
  'engine/src/cpu/x86/avx/operation/factory/integer.rs',
  'engine/src/cpu/x86/avx/integer.rs',
  'engine/tests/cpu/simd/avx/integer/compare/fixtures/p2-avx2-comparison/runtime/cases.mjs',
  'engine/tests/cpu/simd/avx/integer/support/native.rs',
  'engine/tests/cpu/simd/avx/integer/support/fixtures/runtime/cases.mjs',
  'engine/tests/cpu/simd/avx/integer/saturating/native.rs',
  'engine/tests/cpu/simd/avx/integer/saturating/fixtures/p2-avx2-saturating/runtime/cases.mjs',
  'engine/tests/cpu/simd/avx/integer/minmax/native.rs',
  'engine/tests/cpu/simd/avx/integer/minmax/fixtures/p2-avx2-minmax/runtime/cases.mjs',
  'engine/tests/cpu/simd/avx/integer/average/native.rs',
  'engine/tests/cpu/simd/avx/integer/average/fixtures/p2-avx2-average/runtime/cases.mjs',
  'engine/tests/cpu/simd/avx/integer/absolute/native.rs',
  'engine/tests/cpu/simd/avx/integer/absolute/fixtures/p2-avx2-absolute/runtime/cases.mjs',
  'engine/tests/cpu/simd/avx/integer/sign/native.rs',
  'engine/tests/cpu/simd/avx/integer/sign/fixtures/p2-avx2-sign/runtime/cases.mjs',
  'engine/tests/cpu/simd/avx/integer/fixtures/p2-avx2-integer/runtime/cases.mjs',
  'engine/src/cpu/x86/decode/avx/cross_lane.rs',
  'engine/src/cpu/x86/avx/operation/factory/cross_lane.rs',
  'engine/src/cpu/x86/avx/cross_lane/mod.rs',
  'engine/tests/cpu/simd/avx/blocks/fixtures/p2-avx2-blocks/runtime/cases.mjs',
  'engine/src/abi/x86/segments.rs',
  'engine/src/cpu/dbt/wasm/instructions/segments/access.rs',
  'engine/tests/cpu/simd/avx/broadcast/fixtures/p2-avx2-broadcast/runtime/cases.mjs',
  'engine/src/cpu/x86/decode/avx/masked.rs',
  'engine/src/cpu/x86/avx/operation/factory/masked.rs',
  'engine/src/cpu/x86/avx/masked.rs',
  'engine/src/abi/x86/avx.rs',
  'engine/src/cpu/x86/segments/cache.rs',
  'engine/src/memory/space.rs', 'engine/src/memory/space/atomic.rs',
  'engine/tests/cpu/simd/avx/masked/fixtures/p2-avx2-masked/runtime/cases.mjs',
  'engine/src/cpu/x86/avx/shift.rs',
  'engine/src/cpu/x86/avx/operation/factory/shift.rs',
  'engine/src/cpu/x86/decode/avx/shift.rs',
  'engine/src/cpu/x86/simd/shift.rs',
  'engine/src/cpu/x86/simd/packed.rs',
  'engine/tests/cpu/simd/avx/shift/uniform/native.rs',
  'engine/tests/cpu/simd/avx/shift/uniform/fixtures/p2-avx2-shift/runtime/cases.mjs',
  'engine/tests/cpu/simd/avx/shift/variable/native.rs',
  'engine/tests/cpu/simd/avx/shift/variable/fixtures/p2-avx2-variable/runtime/cases.mjs',
  'engine/src/cpu/x86/avx/rearrange.rs',
  'engine/src/cpu/x86/avx/operation/factory/rearrange.rs',
  'engine/src/cpu/x86/decode/avx/rearrange.rs',
  'engine/src/cpu/x86/simd/rearrange.rs',
  'engine/tests/cpu/simd/avx/rearrange/widen/fixtures/p2-avx2-widen/runtime/cases.mjs',
  'engine/tests/cpu/simd/avx/rearrange/lanes/native.rs',
  'engine/tests/cpu/simd/avx/rearrange/lanes/fixtures/p2-avx2-lanes/runtime/cases.mjs',
  'engine/src/cpu/x86/avx/multiply.rs',
  'engine/src/cpu/x86/avx/operation/factory/multiply.rs',
  'engine/src/cpu/x86/decode/avx/multiply.rs',
  'engine/src/cpu/x86/simd/multiply.rs',
  'engine/tests/cpu/simd/avx/multiply/fixtures/p2-avx2-multiply/runtime/cases.mjs',
  'engine/src/cpu/x86/avx/permute.rs',
  'engine/src/cpu/x86/avx/operation/factory/permute.rs',
  'engine/src/cpu/x86/decode/avx/permute.rs',
  'engine/tests/cpu/simd/avx/permute/whole/native.rs',
  'engine/tests/cpu/simd/avx/permute/whole/fixtures/p2-avx2-permute/runtime/cases.mjs',
  'engine/tests/cpu/simd/avx/permute/blend/native.rs',
  'engine/tests/cpu/simd/avx/permute/blend/fixtures/p2-avx2-blend/runtime/cases.mjs',
  'engine/src/cpu/x86/avx/scalar.rs',
  'engine/src/cpu/x86/avx/operation/factory/scalar.rs',
  'engine/src/cpu/x86/decode/avx/scalar.rs',
  'engine/src/cpu/x86/simd/scalar.rs',
  'engine/tests/cpu/simd/avx/scalar/mask/native.rs',
  'engine/tests/cpu/simd/avx/scalar/mask/fixtures/p2-avx2-movemask/runtime/cases.mjs',
  'engine/src/cpu/x86/decode/avx/moves.rs',
  'engine/src/cpu/x86/avx/moves.rs',
  'engine/src/cpu/x86/avx/operation/factory/moves.rs',
  'engine/tests/cpu/simd/avx/moves/stream/native.rs',
  'engine/tests/cpu/simd/avx/moves/stream/fixtures/p2-avx2-stream/runtime/cases.mjs',
  'engine/tests/cpu/simd/avx/multiply/sad/native.rs',
  'engine/tests/cpu/simd/avx/multiply/sad/fixtures/p2-avx2-sad/runtime/cases.mjs',
];
assert.equal(new Set(sourcePaths).size, sourcePaths.length);
const hash = b => createHash('sha256').update(b).digest('hex');
const pins = () => Object.fromEntries(sourcePaths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = pins();
const bytes = s => Buffer.from(s, 'hex'), fill = (n, size = 16) => Buffer.alloc(size, n);
const words = values => {const b = Buffer.alloc(values.length * 4); values.forEach((v, i) => b.writeUInt32LE(v >>> 0, i * 4)); return b;};
const qwords = values => {const b = Buffer.alloc(values.length * 8); values.forEach((v, i) => b.writeBigUInt64LE(v, i * 8)); return b;};
function record(magic, size, values, version = 1) {
  if (magic === 'R3EX' && values[0] === 5) return pageFaultExit({retired: values[1],
    errorCode: 4 | (values[4] === 2 ? 2 : 0) | (values[2] === 2 ? 1 : 0), address: values[3], access: values[4], length: values[5]});
  if (magic === 'R3MH' && values[0] === 1) return pageFaultPacket({
    errorCode: 4 | (values[4] === 2 ? 2 : 0) | (values[2] === 2 ? 1 : 0), address: values[3], access: values[4], length: values[5]});

  const b = Buffer.alloc(size); b.write(magic); b.writeUInt16LE(version, 4); b.writeUInt16LE(1, 6); b.writeUInt32LE(size, 8);
  values.forEach((v, i) => b.writeUInt32LE(v >>> 0, 16 + i * 4)); return b;
}
const state = (r, pc, flags) => record('R3ST', 56, [...r, pc, flags], flags & ~0xcd7 ? 2 : 1);
const arena = c => Buffer.from(new Uint8Array(c.memory.buffer, c.base, SIZE));
function input(c, at, value) {new Uint8Array(c.memory.buffer).set(value, c.base + at);}
function host(c, name, args = []) {assert.equal(c.api[name](...args), 0, name);}
function upload(c, address, value) {input(c, T, value); host(c, 'upload', [address, value.length]);}
function open(owner, entries) {
  const instance = new WebAssembly.Instance(engine.module, {}), c = {owner, entries, memory: instance.exports.memory, api: {}, calls: []};
  for (const [name, value] of Object.entries(instance.exports)) if (name.startsWith('ring3_abi_v1_') && typeof value === 'function') c.api[name.slice(13)] = value;
  for (const [name, arity] of Object.entries({open: 3, map: 3, upload: 2, compile: 1, compile_entries: 2, compile_resident: 1,
    compile_resident_entries: 2, cpu_read: 3, cpu_write: 3, cpu_write_resident: 7, avx_execute: 6, avx_execute_resident: 10, retire_stale_resident: 4})) assert.equal(c.api[name]?.length, arity, name);
  c.key = ++counts.contexts; host(c, 'open', [8, c.key, keyHigh]); c.base = c.api.arena_ptr() >>> 0;
  assert.equal(c.api.arena_len(), SIZE, 'canonical upper-vector arena');
  for (const args of [[PC, 1, 7], [0x5000, 1, 3]]) host(c, 'map', args);
  c.initial = arena(c); return c;
}
function compile(c, row) {
  if (c.resident) {input(c, 96, words([0])); host(c, 'discard_unacknowledged_resident', [c.key, keyHigh, ...c.resident]);}
  const code = bytes(row.code); upload(c, PC, Buffer.concat([code, bytes('ebfe')]));
  input(c, T, words(c.entries ? [PC] : [PC, code.length + 2]));
  const name = c.owner === 'resident' ? c.entries ? 'compile_resident_entries' : 'compile_resident' : c.entries ? 'compile_entries' : 'compile';
  host(c, name, [1, ...(c.entries ? [0] : [])]);
  const a = arena(c);
  if (c.owner === 'resident') c.resident = [a.readUInt32LE(T + 8), a.readUInt32LE(T + 12)];
  const pointer = c.owner === 'resident' ? a.readUInt32LE(T + 16) : c.api.module_ptr() >>> 0;
  const length = c.owner === 'resident' ? a.readUInt32LE(T + 20) : c.api.module_len() >>> 0;
  const binary = Buffer.from(new Uint8Array(c.memory.buffer, pointer, length)), module = new WebAssembly.Module(binary);
  const actual = WebAssembly.Module.imports(module).map(i => `${i.module}:${i.name}:${i.kind}`).sort();
  const expected = generatedImports({resident: c.owner === 'resident', helpers: [
    ...(row.read ? ['cpu_read'] : []), ...(row.write ? [c.owner === 'resident' ? 'cpu_write_resident' : 'cpu_write'] : []),
    ...(row.selected ? [c.owner === 'resident' ? 'avx_execute_resident' : 'avx_execute'] : [])
  ]}).map(i => `${i.module}:${i.name}:${i.kind}`).sort();
  assert.deepEqual(actual, expected, `${row.name}: exact direct-path imports`);
  assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  writeFileSync(join(output, `module-${++counts.modules}.wasm`), binary, {flag: 'wx'});
  const ring3 = {...c.api};
  for (const helper of ['cpu_read', 'cpu_write', 'cpu_write_resident']) ring3[helper] = (...args) => {
    const status = c.api[helper](...args); c.calls.push({helper, status, args: args.map(v => v >>> 0)}); return status;
  };
  if (row.selected) {
    const helper = c.owner === 'resident' ? 'avx_execute_resident' : 'avx_execute';
    ring3[helper] = (...args) => {
      const metadata = record('R3AM', 24, [row.selected.segment, row.selected.offset]);
      assert.deepEqual(arena(c).subarray(T + 544, T + 568), metadata, `${row.name}: captured address metadata`);
      const status = c.api[helper](...args);
      c.calls.push({helper, status, args: args.map(value => value >>> 0)});
      return status;
    };
  }
  return {run: new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3}).exports.run, codeLength: code.length};
}
function seed(c, row) {
  input(c, 0, c.initial);
  const r = [row.address ?? 0x5020, 0x11223344, 0x22334455, 0x33445566, 0x7780, 0x55667788, 0x66778899, 0x778899aa];
  if (row.r) for (const [index, value] of Object.entries(row.r)) r[+index] = value >>> 0;
  const flags = 0x244cd7;
  input(c, 0, state(r, PC, flags));
  if (row.segmentRecord) {assert.equal(row.segmentRecord.length, 64); input(c, SV + 160, row.segmentRecord);}
  if (!row.selected && row.mxcsr !== undefined) input(c, SV + 16, words([row.mxcsr]));
  for (let i = 0; i < 8; i++) {input(c, SV + 32 + i * 16, fill(0x21 + i)); input(c, YU + 16 + i * 16, fill(0x91 + i));}
  for (const [index, value] of Object.entries(row.lower ?? {})) input(c, SV + 32 + +index * 16, value);
  for (const [index, value] of Object.entries(row.upper ?? {})) input(c, YU + 16 + +index * 16, value);
  if (row.segments && !row.selected) {assert.equal(row.segments.length, 64); input(c, SV + 160, row.segments);}
  if (row.selected) {
    input(c, 4524, row.segments);
    input(c, SV + 16, words([row.mxcsr]));
    input(c, T + 512, fill(0xa5, 32));
  }
  return {r, flags};
}
function ram(c) {
  const saved = arena(c), result = Buffer.alloc(8192);
  for (const [page, address] of [PC, 0x5000].entries()) for (let offset = 0; offset < 4096; offset += 512) {
    host(c, 'cpu_read', [address + offset, 512, 1]);
    const frame = arena(c); assert.equal(frame.readUInt32LE(116), 0, 'mapped RAM snapshot');
    frame.copy(result, page * 4096 + offset, T, T + 512);
  }
  input(c, 0, saved); return result;
}
function execute(c, fn, row, {budget = row.retired ?? 1, cancel = false} = {}) {
  const s = seed(c, row); if (cancel) input(c, 96, words([1]));
  const before = arena(c), expected = Buffer.from(before), beforeRam = ram(c), first = c.calls.length;
  const retired = budget === 0 || cancel ? 0 : row.retired ?? (row.reason ? 0 : 1);
  const reason = cancel ? 2 : budget === 0 ? 1 : row.reason ?? row.selected?.exitReason ?? 1;
  const changed = retired > 0;
  const r = [...s.r]; if (changed) for (const [index, value] of Object.entries(row.outR ?? {})) r[+index] = value >>> 0;
  expected.set(state(r, changed ? PC + fn.codeLength : PC, s.flags));
  expected.set(record('R3EX', 40, [reason, retired, row.reason && !cancel && budget ? row.detail ?? 0 : 0,
    row.reason === 5 && !cancel && budget ? row.fault : 0, row.reason === 5 && !cancel && budget ? row.access : 0,
    row.reason === 5 && !cancel && budget ? row.length : 0], 6), 56);
  if (changed) {
    for (const [index, value] of Object.entries(row.outLower ?? {})) expected.set(value, SV + 32 + +index * 16);
    for (const [index, value] of Object.entries(row.outUpper ?? {})) expected.set(value, YU + 16 + +index * 16);
  }
  const helpers = budget === 0 || cancel ? 0 : row.helpers ?? Number(Boolean(row.read || row.write || row.selected));
  if (helpers && row.selected) {
    const status = row.reason === 5 ? 1 : row.reason === 11 ? 3 : row.reason === 15 ? 7 : 0;
    expected.set(record('R3AM', 24, [row.selected.segment, row.selected.offset]), T + 544);
    expected.set(record('R3MH', 40, [status, 0, status === 1 ? row.detail : 0,
      status === 1 ? row.fault : 0, status === 1 ? row.access : 0, row.selected.length], 7), 100);
    if (!row.reason) {
      expected.fill(0, T + 512, T + 544);
      expected.set(words([0, s.flags]), T + 512);
    }
  }
  if (helpers && !row.selected) {
    const fault = row.reason === 5;
    expected.set(record('R3MH', 40, [fault ? 1 : 0, 0, fault ? row.detail : 0, fault ? row.fault : 0, fault ? row.access : 0, row.length ?? row.read ?? row.write], 7), 100);
    if (!row.reason && row.read) expected.set(row.memory, T);
    if (row.write) expected.set(row.staged, T);
  }
  assert.equal(fn.run(c.base, c.base + 56, budget, c.base + 96), 0, row.name);
  assert.deepEqual(arena(c), expected, `${row.name}: complete canonical arena budget=${budget} cancel=${cancel}`);
  assert.equal(c.calls.length - first, helpers, `${row.name}: exact helper count`);
  for (const call of c.calls.slice(first)) {
    if (row.selected?.invalidates) {
      assert.equal(call.status, 11, `${row.name}: committed helper invalidation`);
    } else {
      assert.equal(call.status, reason === 5 ? 23 : 0, `${row.name}: helper transport`);
    }
    if (row.selected) {
      assert.deepEqual(call.args.slice(-6), [row.selected.descriptor, row.selected.operands, 0, 0, 0, s.flags]);
      if (c.owner === 'resident') {
        assert.deepEqual(call.args.slice(0, 4), [c.key, keyHigh, ...c.resident]);
      }
      continue;
    }
    assert.deepEqual(call.args.slice(-3), [row.address ?? s.r[0], row.read ?? row.write, row.alignment === 32 ? 1 : row.alignment ?? 1]);
  }
  const expectedRam = Buffer.from(beforeRam);
  if (row.selected?.access === 2 && changed) {
    for (const {at, value} of row.selected.stores) {
      const address = row.address + at;
      const page = row.selected.invalidates ? PC : 0x5000;
      assert.ok(address >= page && address + value.length <= page + 4096);
      value.copy(expectedRam, (page === PC ? 0 : 4096) + address - page);
    }
  }
  if (row.write && changed) {
    assert.ok(row.checkRam, `${row.name}: literal committed store oracle`);
    assert.ok(row.address >= 0x5000 && row.address + row.checkRam.length <= 0x6000);
    row.checkRam.copy(expectedRam, 4096 + row.address - 0x5000);
  }
  assert.deepEqual(ram(c), expectedRam, `${row.name}: complete mapped RAM budget=${budget} cancel=${cancel}`);
  assert.deepEqual(arena(c), expected, `${row.name}: RAM observations preserve the complete arena`);
  counts.fullRamPairs++;
  if (row.checkRam && changed) {
    host(c, 'cpu_read', [row.address, row.checkRam.length, 1]);
    assert.deepEqual(arena(c).subarray(T, T + row.checkRam.length), row.checkRam, `${row.name}: committed guest RAM`);
  }
  counts.runs++; if (row.reason && budget && !cancel) counts.faults++; if (!budget || cancel) counts.controls++;
}
const zero = fill(0), a = words([1, 2, 3, 4]), b = words([11, 12, 13, 14]);
const rows = [
  {name: 'move128-source-alias-upperzero', code: 'c5f810c0', lower: {0: a}, outLower: {0: a}, outUpper: {0: zero}},
  {name: 'move256-captures-high-half', code: 'c5fc10c1', lower: {1: a}, upper: {1: b}, outLower: {0: a}, outUpper: {0: b}},
  {name: 'logical256-destination-alias', code: 'c5fc57c1', lower: {0: fill(0xf0), 1: fill(0x33)}, upper: {0: fill(0xaa), 1: fill(0x0f)}, outLower: {0: fill(0xc3)}, outUpper: {0: fill(0xa5)}},
  {name: 'permute-source-order', code: 'c5f8c6c11b', lower: {0: a, 1: b}, outLower: {0: words([4, 3, 12, 11])}, outUpper: {0: zero}},
  {name: 'integer-wrap', code: 'c5f9fcc1', lower: {0: fill(255), 1: fill(1)}, outLower: {0: zero}, outUpper: {0: zero}},
  {name: 'rearrange-immediate', code: 'c5f970c11b', lower: {1: a}, outLower: {0: words([4, 3, 2, 1])}, outUpper: {0: zero}},
  {name: 'shift-immediate', code: 'c5f971f001', lower: {0: fill(1)}, outLower: {0: fill(2)}, outUpper: {0: zero}},
  {name: 'multiply-low-wrap', code: 'c5f9d5c1', lower: {0: fill(255), 1: bytes('02000200020002000200020002000200')}, outLower: {0: bytes('fefffefffefffefffefffefffefffeff')}, outUpper: {0: zero}},
  {name: 'scalar-insert-live-gpr', code: 'c4e37920c002', r: {0: 0x12345678}, lower: {0: zero}, outLower: {0: bytes('00007800000000000000000000000000')}, outUpper: {0: zero}},
  {name: 'scalar-extract-zero-extension', code: 'c4e37914c002', lower: {0: bytes('1122ff445566778899aabbccddeeff00')}, outR: {0: 255}},
  {name: 'cross-lane-extract-original-high', code: 'c4e37d19c801', lower: {1: a}, upper: {1: b}, outLower: {0: b}, outUpper: {0: zero}},
  {name: 'prior-gpr-write-consumed-by-direct-move', code: 'b878563412c5f96ec0', retired: 2, outR: {0: 0x12345678}, outLower: {0: words([0x12345678, 0, 0, 0])}, outUpper: {0: zero}},
  {name: 'legacy-carryless-and-avx-local-types', code: '660f3a44c101c5f810d0', retired: 2, lower: {0: qwords([1n, 2n]), 1: qwords([3n, 4n])}, outLower: {0: qwords([6n, 0n]), 2: qwords([6n, 0n])}, outUpper: {2: zero}},
  {name: 'unaligned256-read-exact32', code: 'c5fc1000', address: 0x5011, read: 32, memory: Buffer.concat([a, b]), outLower: {0: a}, outUpper: {0: b}},
  {name: 'aligned256-load-linear32', code: 'c5fc2800', address: 0x5020, alignment: 32, read: 32, memory: Buffer.concat([a, b]), outLower: {0: a}, outUpper: {0: b}},
  {name: 'aligned256-store-linear32', code: 'c5fc2900', address: 0x5020, alignment: 32, write: 32, lower: {0: a}, upper: {0: b}, staged: Buffer.concat([a, b]), checkRam: Buffer.concat([a, b])},
  {name: 'high-half-read-fault-preserves-both-destination-halves', code: 'c5fc1000', address: 0x5ff0, read: 32, reason: 5, detail: 1, fault: 0x6000, access: 1, length: 32},
  {name: 'high-half-store-fault-preserves-all-guest-RAM', code: 'c5fc1100', address: 0x5ff0, write: 32, memory: fill(0x66, 16), lower: {0: a}, upper: {0: b}, staged: Buffer.concat([a, b]), reason: 5, detail: 1, fault: 0x6000, access: 2, length: 32},
  {name: 'narrow-byte-insert-last-page-byte', code: 'c4e379200002', address: 0x5fff, read: 1, memory: bytes('80'), lower: {0: zero}, outLower: {0: bytes('00008000000000000000000000000000')}, outUpper: {0: zero}},
  {name: 'narrow-byte-store-last-page-byte', code: 'c4e379140002', address: 0x5fff, write: 1, lower: {0: bytes('1122ff445566778899aabbccddeeff00')}, staged: bytes('ff'), checkRam: bytes('ff')},
  {name: 'broadcast-exact-final-dword', code: 'c4e27d1800', address: 0x5ffc, read: 4, memory: words([0x7f800001]), outLower: {0: words([0x7f800001, 0x7f800001, 0x7f800001, 0x7f800001])}, outUpper: {0: words([0x7f800001, 0x7f800001, 0x7f800001, 0x7f800001])}},
];
for (const store of [false, true]) for (const address of [0x5010, 0x6001]) rows.push({name: `alignment32-before-${address === 0x5010 ? 'mapped' : 'unmapped'}-${store ? 'write' : 'read'}`, code: store ? 'c5fc2900' : 'c5fc2800', address, alignment: 32, ...(store ? {write: 32, staged: Buffer.concat([a, b])} : {read: 32}), helpers: 0, reason: 11});
rows.push(...logicalRows);
rows.push(...comparisonRows);
rows.push(...integerRows);
rows.push(...blocksRows);
rows.push(...broadcastRows);
rows.push(...maskedRows);
rows.push(...uniformRows);
rows.push(...variableRows);
rows.push(...widenRows);
rows.push(...laneRows);
rows.push(...multiplyRows);
rows.push(...wholeRows);
rows.push(...blendRows);
rows.push(...movemaskRows);
rows.push(...streamRows);
rows.push(...sadRows);
for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries);
  for (const row of rows) {
    const fn = compile(c, row);
    if (row.memory) upload(c, row.address, row.memory);
    if (row.write && !row.memory && row.address < 0x6000) upload(c, row.address, fill(0x66, row.write));
    if (row.selected?.invalidates) {
      execute(c, fn, row, {budget: 0});
      execute(c, fn, row, {budget: row.retired ?? 1, cancel: true});
      execute(c, fn, row);
      if (c.resident) {
        const after = arena(c), afterRam = ram(c);
        host(c, 'retire_stale_resident', [c.key, keyHigh, ...c.resident]);
        assert.deepEqual(arena(c), after, `${row.name}: stale retirement preserves complete arena`);
        assert.deepEqual(ram(c), afterRam, `${row.name}: stale retirement preserves complete RAM`);
        c.resident = null;
      }
      continue;
    }
    execute(c, fn, row);
    execute(c, fn, row, {budget: 0}); execute(c, fn, row, {budget: row.retired ?? 1, cancel: true});
  }
  host(c, 'close');
}
assert.deepEqual(counts, {contexts: 4, modules: 21184, runs: 63552, controls: 42368, faults: 2308, fullRamPairs: 63552});
assert.deepEqual(pins(), sourcePins, 'pinned sources unchanged during proof');
writeFileSync(join(output, 'result.json'), JSON.stringify({scope: 'real generated AVX direct integration with full canonical arena; finite literal vectors', engineSha256: engine.sha256, counts, sourcePins}, null, 2) + '\n');
console.log(JSON.stringify({status: 'PASS', ...counts}));
