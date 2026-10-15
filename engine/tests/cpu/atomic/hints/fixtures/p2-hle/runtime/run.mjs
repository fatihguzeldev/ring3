import assert from 'node:assert/strict';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {pathToFileURL} from 'node:url';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root, 'explicit selected engine, output and source root');
const supportPath = 'engine/tests/cpu/segments/code/base/fixtures/p2-cs-base/runtime/support.mjs';
const {harness, PC, SEG, words, record, state, registers, exit, hash, writeRAM} =
  await import(pathToFileURL(join(root, supportPath)));
const h = await harness(enginePath, output, root);
const paths = [
  'engine/src/cpu/x86/capabilities.rs', 'engine/src/cpu/x86/decode/profile.rs', 'engine/src/cpu/x86/decode/families.rs',
  'engine/src/cpu/x86/decode/atomic.rs', 'engine/src/cpu/x86/decode/lower.rs', 'engine/src/cpu/x86/decode/families.rs',
  'engine/src/cpu/x86/decode/integer.rs', 'engine/src/cpu/x86/decode/operands.rs',
  'engine/src/cpu/x86/decode/execution_faults.rs', 'engine/src/cpu/dbt/wasm/instructions/atomic.rs', 'engine/src/cpu/dbt/wasm/helpers/integer.rs',
  'engine/src/cpu/dbt/wasm/instructions/memory/exchange.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/byte_store.rs',
  'engine/src/cpu/dbt/wasm/instructions/memory/word_store.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch.rs',
  'engine/src/cpu/dbt/wasm/instructions/integer/operands.rs', 'engine/src/process/memory/scalar.rs',
  'engine/src/memory/space.rs', 'engine/src/memory/space/atomic.rs', 'engine/src/abi/memory_helper.rs',
  'engine/tests/cpu/atomic/wasm.rs', 'engine/tests/cpu/atomic/hints/wasm.rs',
  'engine/tests/cpu/atomic/hints/fixtures/p2-hle/runtime/run.mjs', supportPath,
];
const pins = () => Object.fromEntries(paths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = pins();
const DATA = 0x5010, VALUE = 0xa1b2c3d4, FLAGS = 0xcd7;
const bytes = hex => Buffer.from(hex, 'hex');
const traced = (name, args, status = 0) => ({name, args: args.map(value => value >>> 0), status});
const bound = (c, material, args) => [...(c.resident ? material.binding : []), ...args];
const route = (c, kind) => {
  if (kind === 'or') return c.resident ? 'cpu_atomic_or_resident_u32' : 'cpu_atomic_or_u32';
  if (kind === 'exchange') return c.resident ? 'cpu_exchange_resident32' : 'cpu_exchange32';
  return c.resident ? `cpu_store_resident${kind}` : `cpu_store${kind}`;
};
function seed({set = {}, flags = FLAGS, segments} = {}) {
  const regs = [...registers];
  for (const [index, value] of Object.entries(set)) regs[Number(index)] = value;
  const input = state(h.initial, {regs, flags});
  if (segments === 'null DS' || segments === 'limited SS') {
    // Literal v2 cache image: raw limits remain logical, with no companion bases.
    const sg = record('R3SG', 64, [], 2);
    [0x23, 0x1b, 0x23, segments === 'null DS' ? 0 : 0x23, 0x3b, 0]
      .forEach((value, index) => sg.writeUInt16LE(value, 16 + 2 * index));
    [0xf33fffff, 0xf3bfffff, segments === 'limited SS' ? 0xf2300fff : 0xf33fffff,
      segments === 'null DS' ? 0 : 0xf33fffff, 0xf33fffff, 0]
      .forEach((value, index) => sg.writeUInt32LE(value >>> 0, [28, 32, 36, 40, 44, 56][index]));
    input.set(sg, SEG);
  }
  return input;
}
function done(input, next, flags, {reason = 1, version = 6, ecx} = {}) {
  const want = Buffer.from(input);
  want.writeUInt32LE(next, 48); want.writeUInt32LE(flags, 52);
  if (ecx !== undefined) want.writeUInt32LE(ecx, 20);
  return exit(want, reason, 1, reason >= 16 ? 7 : version);
}
function packet(want, kind) {
  // cpu or and xchg helpers report the old dword; stores report zero.
  const value = kind === 'or' || kind === 'exchange' ? 0x12345678 : 0;
  const version = 10;
  const length = kind === 8 ? 1 : kind === 16 ? 2 : 4;
  want.set(record('R3MH', 40, [0, value, 0, 0, 0, length], version), 100);
}
const ordinary = [
  {label: 'acquire locked OR', code: 'f2f0830801', kind: 'or', value: 1, after: '79563412', flags: 0x402},
  {label: 'release locked OR', code: 'f3f0830801', kind: 'or', value: 1, after: '79563412', flags: 0x402},
  {label: 'acquire implicit XCHG', code: 'f28708', kind: 'exchange', value: VALUE, after: 'd4c3b2a1', ecx: 0x12345678},
  {label: 'release implicit XCHG', code: 'f38708', kind: 'exchange', value: VALUE, after: 'd4c3b2a1', ecx: 0x12345678},
  {label: 'release register dword store', code: 'f38908', kind: 32, value: VALUE, after: 'd4c3b2a1'},
  {label: 'release register word store', code: 'f3668908', kind: 16, value: 0xc3d4, after: 'd4c33412'},
  {label: 'release register byte store', code: 'f38808', kind: 8, value: 0xd4, after: 'd4563412'},
  {label: 'release immediate dword store', code: 'f3c700df9b5713', kind: 32, value: 0x13579bdf, after: 'df9b5713'},
  {label: 'release ordinary address16 store', code: 'f3678908', kind: 32, value: VALUE, after: 'd4c3b2a1',
    set: {0: 0x6000, 3: 0xdead5000, 6: 0xbeef0010}, decoy: true},
  {label: 'mixed hints select release locked OR', code: 'f2f3f0830801', kind: 'or', value: 1, after: '79563412', flags: 0x402},
  {label: 'mixed hints select acquire locked OR', code: 'f3f2f0830801', kind: 'or', value: 1, after: '79563412', flags: 0x402},
  {label: 'repeated mixed hints select release locked OR', code: 'f2f2f3f0830801', kind: 'or', value: 1, after: '79563412', flags: 0x402},
  {label: 'repeated mixed hints select acquire locked OR', code: 'f3f3f2f0830801', kind: 'or', value: 1, after: '79563412', flags: 0x402},
  {label: 'mixed hints select release implicit XCHG', code: 'f2f38708', kind: 'exchange', value: VALUE, after: 'd4c3b2a1', ecx: 0x12345678},
  {label: 'mixed hints select acquire implicit XCHG', code: 'f3f28708', kind: 'exchange', value: VALUE, after: 'd4c3b2a1', ecx: 0x12345678},
  {label: 'mixed hints select release dword store', code: 'f2f38908', kind: 32, value: VALUE, after: 'd4c3b2a1'},
  {label: 'mixed hints select release word store', code: 'f2f3668908', kind: 16, value: 0xc3d4, after: 'd4c33412'},
];
const faults = [
  {label: 'acquire OR split write permission', code: 'f2f0830801', kind: 'or', address: 0x5fff, value: 1, pageFault: true},
  {label: 'release store split write permission', code: 'f38908', kind: 32, address: 0x5fff, value: VALUE, pageFault: true},
  {label: 'release store null DS before paging', code: 'f38908', kind: 32, address: 0xffffffff, value: VALUE, segments: 'null DS', reason: 11},
  {label: 'acquire OR logical SS span before paging', code: 'f2f0830c2401', kind: 'or', address: 0xffe, value: 1,
    segments: 'limited SS', reason: 15, set: {4: 0xffe}},
];
const controls = [
  {label: 'closest acquire does not qualify an unlocked store', code: 'f3f28908'},
  {label: 'duplicate release remains profile refusal', code: 'f3f38908'},
  {label: 'duplicate operand prefix remains profile refusal', code: '6666f38908'},
  {label: 'LOCK store stays invalid', code: 'f3f08908'},
  {label: 'LOCK register destination stays invalid', code: 'f2f083c801'},
  {label: 'LOCK NOP stays invalid', code: 'f3f090'},
];
let plannedInvocations = 0, admittedInvocations = 0, compileControls = 0;
function open(resident, entries, label, code, input) {
  const c = h.open(resident, entries, label, [[0, 3], [PC, 7], [0x5000, 3], [0x6000, 3]]);
  h.upload(c, PC, Buffer.concat([bytes(code), bytes('ebfe')]));
  h.upload(c, 0, Buffer.alloc(4096, 0x6b));
  h.upload(c, 0x5000, Buffer.alloc(4096, 0xa6));
  h.upload(c, 0x6000, Buffer.alloc(4096, 0x59));
  h.upload(c, DATA, bytes('78563412'));
  h.put(c, 0, input);
  return c;
}
function compile(c, row, input) {
  h.put(c, 0, input);
  const name = route(c, row.kind);
  return {name, material: h.compile(c, `${row.label}: compile real helper body`, [[PC, bytes(row.code).length + 2]], [name])};
}
function invoke(c, label, material, expected) {
  plannedInvocations++;
  if (material) {admittedInvocations++; h.observe(c, label, {...expected, material});}
  else h.skip(c, label, 'hinted body compile admission');
}
for (const resident of [false, true]) for (const entries of [false, true]) {
  for (const row of ordinary) {
    const input = seed({set: {0: DATA, 1: VALUE, ...row.set}});
    const c = open(resident, entries, row.label, row.code, input);
    if (row.decoy) h.upload(c, 0x6000, bytes('11223344'));
    const {name, material} = compile(c, row, input);
    const want = done(input, PC + bytes(row.code).length, row.flags ?? FLAGS, {ecx: row.ecx});
    packet(want, row.kind);
    const wantRAM = h.ram(c); writeRAM(wantRAM, DATA, bytes(row.after));
    const helpers = material ? [traced(name, bound(c, material, [DATA, row.value]))] : [];
    invoke(c, row.label, material, {input, want, wantRAM, helpers});
    h.host(c, 'close');
  }
  for (const row of faults) {
    const input = seed({set: {0: row.address, 1: VALUE, ...row.set}, segments: row.segments});
    const c = open(resident, entries, row.label, row.code, input);
    if (row.pageFault) {
      h.upload(c, 0x5fff, bytes('aabbccdd'));
      h.host(c, 'protect', [0x6000, 1, 1]); // READ: the RMW read succeeds, then the write is denied.
    }
    const {name, material} = compile(c, row, input);
    const want = exit(Buffer.from(input), row.pageFault ? 5 : row.reason, 0, 6);
    if (row.pageFault) {
      want.set(record('R3MH', 40, [1, 0, 2, 0x6000, 2, 4], 10), 100);
      want.set(words([2, 0x6000, 2, 4]), 80);
    }
    const helpers = row.pageFault && material ? [traced(name, bound(c, material, [row.address, row.value]))] : [];
    invoke(c, row.label, material, {input, want, helpers});
    h.host(c, 'close');
  }
  {
    const row = {label: 'stale hinted source', code: 'f38908', kind: 32};
    const input = seed({set: {0: DATA, 1: VALUE}});
    const c = open(resident, entries, row.label, row.code, input);
    const {material} = compile(c, row, input);
    h.upload(c, PC, bytes('f2'));
    invoke(c, 'changed hint byte rejects held material before operand or RF begin', material,
      {input, want: input, status: 4, kind: 'physical-currentness'});
    h.host(c, 'close');
  }
  for (const tf of [false, true]) {
    const row = {label: tf ? 'TF and RF release store' : 'RF release store', code: 'f38908', kind: 32};
    const input = seed({set: {0: DATA, 1: VALUE}, flags: tf ? 0x10dd7 : 0x10cd7});
    const c = open(resident, entries, row.label, row.code, input);
    const {name, material} = compile(c, row, input);
    const want = done(input, PC + 3, tf ? 0xdd7 : FLAGS, {reason: tf ? 16 : 1}); packet(want, 32);
    const wantRAM = h.ram(c); writeRAM(wantRAM, DATA, bytes('d4c3b2a1'));
    invoke(c, row.label, material, {input, want, wantRAM,
      helpers: material ? [traced(name, bound(c, material, [DATA, VALUE]))] : []});
    h.host(c, 'close');
  }
  for (const row of controls) {
    const input = seed({set: {0: DATA, 1: VALUE}});
    const c = open(resident, entries, row.label, row.code, input);
    h.compile(c, `${row.label}: neutral strict compile refusal`, [[PC, bytes(row.code).length + 2]], [], {expectedStatus: 10});
    compileControls++;
    h.host(c, 'close');
  }
}
assert.equal(plannedInvocations, 96, '24 semantic vectors across four profiles');
assert.equal(compileControls, 24, 'duplicate and invalid-LOCK strict host controls');
assert.deepEqual(pins(), sourcePins, 'HLE source and literal consumer stayed unchanged');
writeFileSync(join(output, 'hle-contract.json'), JSON.stringify({
  source_pins: sourcePins, planned_module_invocations: plannedInvocations,
  admitted_module_invocations: admittedInvocations, strict_compile_controls: compileControls,
  profiles: ['replacement-extents', 'replacement-entries', 'resident-extents', 'resident-entries'],
  expected_when_all_admitted: {contexts: 120, modules: 96, module_invocations: 96, observations: 216,
    guest_instructions_retired: 76, real_operand_helper_calls: 84},
}, null, 2) + '\n');
h.finish([
  'Finite absent-HLE ordinary-operation contract: no HLE enablement, transaction state or performance claim.',
  'Twenty-four literal semantic/authority vectors run once in each of four existing bound profiles; whole 4796-byte arena and every mapped RAM page are compared.',
  'All guards and operand helpers are real engine exports; no fabricated helper result or mutation hook is used.',
  'The retained changed physical hint byte causes guard status4 before instruction begin, preserving state and RAM; it does not claim a retired CodeInvalidated exit.',
  'Mixed F2/F3 hints follow the closest qualifying prefix. Same-only repeats and duplicate operand prefixes remain host refusals; invalid LOCK controls use strict refusal and do not relabel duplicates as guest UD.',
  'Old-engine compilation failures remain admission failures and dependent module invocations are explicitly unreached.',
  'D16/B16 combinations and successful transaction semantics are outside this finite consumer; ordinary helper regression fixtures remain independent plain-operation controls.',
]);
