import assert from 'node:assert/strict';
import {queries} from './oracle.mjs';
import {ARENA_SIZE, createArena} from '../../../../../../../support/wasm/arena.mjs';

export const PC = 0x1000, SIZE = ARENA_SIZE;
export function words(values) {
  const bytes = Buffer.alloc(values.length * 4);
  values.forEach((value, i) => bytes.writeUInt32LE(value >>> 0, i * 4));
  return bytes;
}
function record(magic, size, fields, version = 1) {
  const bytes = Buffer.alloc(size); bytes.write(magic); bytes.writeUInt16LE(version, 4);
  bytes.writeUInt16LE(1, 6); bytes.writeUInt32LE(size, 8);
  bytes.set(words(fields), 16); return bytes;
}
const cpuRecord = state => record('R3ST', 56, [...state.registers, state.pc, state.flags], state.version);
export const arena = context => Buffer.from(new Uint8Array(context.memory.buffer, context.base, SIZE));
export function input(context, offset, bytes) {
  new Uint8Array(context.memory.buffer).set(bytes, context.base + offset);
}
// seed required flat code authority in input only; numeric/helper payload stays dirty.
export function seedFlatCodeContext(context) {
  const canonical = createArena(), initial = Buffer.alloc(SIZE, 0xa5);
  initial.set(canonical.subarray(4524, 4588), 4524);
  initial.set(canonical.subarray(4756, 4796), 4756);
  input(context, 0, initial);
}
export function checkImports(module, resident) {
  assert.deepEqual(WebAssembly.Module.imports(module), [
    {module: 'env', name: 'memory', kind: 'memory'},
    {module: 'ring3', name: resident ? 'guard_detect_resident' : 'guard_detect', kind: 'function'},
    {module: 'ring3', name: 'finish_exception_detection', kind: 'function'},
  ], 'CPUID uses only its bound guard and application event finalizer');
}
export function checkContext(context, instructionLength, observe = () => {}) {
  let runs = 0;
  function execute(query, flags, {budget = 1, cancel = 0, reason = 1, retired = 1} = {}) {
    const registers = [query.leaf, query.subleaf, 0xa5c31234, 0x89abcdef, 0x7000, 0x56789abc, 0x6789abcd, 0x789abcde];
    const before = {registers, flags, pc: PC, version: (flags & ~0xcd7) ? 2 : 1};
    input(context, 0, cpuRecord(before)); input(context, 96, words([cancel]));
    const next = {...before, registers: [...registers], pc: PC + instructionLength * retired};
    if (retired) next.registers.splice(0, 4, ...query.result);
    const expected = arena(context); expected.set(cpuRecord(next));
    expected.set(record('R3EX', 40, [reason, retired, 0, 0, 0, 0], reason === 16 ? 7 : 1), 56);
    const label = `${context.label}/leaf-${query.leaf.toString(16)}/subleaf-${query.subleaf.toString(16)}/flags-${flags.toString(16)}/budget-${budget}/cancel-${cancel}`;
    const callStart = context.calls.length;
    const status = context.run(context.base, context.base + 56, budget, context.base + 96);
    const actual = arena(context);
    assert.equal(status, 0, label + ' run status');
    assert.deepEqual(Array.from({length: 4}, (_, i) => actual.readUInt32LE(16 + i * 4)), next.registers.slice(0, 4), label + ' CPUID outputs');
    assert.deepEqual(actual, expected, label + ' full CPU, flags, numeric state and unrelated arena preservation');
    const calls = context.calls.slice(callStart), [guard] = calls;
    assert.ok(['guard_detect', 'guard_detect_resident'].includes(guard.name));
    assert.ok(BigInt(guard.status) > 0n, 'bound CPUID entry has one valid epoch');
    assert.deepEqual(calls.slice(1), reason === 16 ? [{
      name: 'finish_exception_detection',
      args: [guard.status, 16, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0], status: 0,
    }] : [], 'only a completed single-step publishes exact application event metadata');
    observe({label, status, reason, retired, registers: next.registers, flags, pc: next.pc, calls}); runs++;
  }
  for (const query of queries) for (const flags of [0xcd7, 0x244cd7]) execute(query, flags);
  const query = queries.find(row => row.leaf === 7 && row.subleaf === 0);
  execute(query, 0x244cd7, {budget: 0, retired: 0});
  execute(query, 0x244cd7, {cancel: 1, reason: 2, retired: 0});
  execute(query, 0x102, {budget: 8, reason: 16});
  return runs;
}
