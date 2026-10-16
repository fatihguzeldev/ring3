import {pageFaultExit, pageFaultPacket} from '../../../../../../support/wasm/page-fault.mjs';
import assert from 'node:assert/strict';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {pathToFileURL} from 'node:url';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root, 'explicit engine, output and source root');
const supportPath = 'engine/tests/cpu/segments/code/base/fixtures/p2-cs-base/runtime/support.mjs';
const {harness, PC, SEG, BASES, record, state, registers, exit, hash, writeRAM} =
  await import(pathToFileURL(join(root, supportPath)));
const h = await harness(enginePath, output, root);
const paths = [
  'engine/src/cpu/x86/decode/aliases.rs', 'engine/src/cpu/x86/decode/aliases/tests.rs',
  'engine/src/cpu/x86/decode/aliases/tests/word.rs',
  'engine/src/cpu/x86/decode/mod.rs', 'engine/src/cpu/x86/decode/lower.rs', 'engine/src/cpu/x86/decode/families.rs',
  'engine/src/cpu/x86/decode/integer.rs', 'engine/src/cpu/x86/decode/atomic.rs',
  'engine/src/cpu/x86/decode/profile.rs',  'engine/src/cpu/dbt/wasm/instructions/integer/shifts.rs',
  'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/mod.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/arithmetic.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/moves.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/exchange.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/predicates.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/shifts.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/control.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/dispatch/extended.rs', 'engine/src/cpu/dbt/wasm/instructions/integer/operands.rs',
  'engine/tests/cpu/arithmetic/native.rs', 'engine/tests/cpu/arithmetic/wasm.rs',
  'engine/tests/support/wasm/page-fault.mjs', 'engine/tests/cpu/arithmetic/aliases/native.rs', 'engine/tests/cpu/arithmetic/aliases/wasm.rs',
  'engine/tests/cpu/arithmetic/aliases/fixtures/p2-legacy-aliases/runtime/run.mjs', supportPath,
];
const pins = () => Object.fromEntries(paths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = pins(), FLAGS = 0xcd7, MOVE_FLAGS = 0x244ed7, DATA = 0x5010;
const bytes = hex => Buffer.from(hex, 'hex');
const registerRows = [
  {label: '82 ADD byte', code: '82c001', eax: 0x1234567f, after: 0x12345680, flags: 0xc92},
  {label: '82 OR byte', code: '82c801', eax: 0x1234567f, after: 0x1234567f, flags: 0x402},
  {label: '82 ADC byte consumes CF', code: '82d001', eax: 0x1234567f, after: 0x12345681, flags: 0xc96},
  {label: '82 SBB byte consumes CF', code: '82d801', eax: 0x1234567f, after: 0x1234567d, flags: 0x406},
  {label: '82 AND byte', code: '82e001', eax: 0x1234567f, after: 0x12345601, flags: 0x402},
  {label: '82 SUB byte', code: '82e801', eax: 0x1234567f, after: 0x1234567e, flags: 0x406},
  {label: '82 XOR byte', code: '82f001', eax: 0x1234567f, after: 0x1234567e, flags: 0x406},
  {label: '82 CMP preserves byte', code: '82f801', eax: 0x1234567f, after: 0x1234567f, flags: 0x406},
  {label: 'SAL6 byte one', code: 'd0f0', eax: 0x12345681, after: 0x12345602, flags: 0xc03},
  {label: 'SAL6 byte CL', code: 'd2f0', eax: 0x12345681, after: 0x12345602, flags: 0xc03},
  {label: 'SAL6 byte immediate', code: 'c0f001', eax: 0x12345681, after: 0x12345602, flags: 0xc03},
  {label: 'SAL6 word one', code: '66d1f0', eax: 0x12348001, after: 0x12340002, flags: 0xc03},
  {label: 'SAL6 word CL', code: '66d3f0', eax: 0x12348001, after: 0x12340002, flags: 0xc03},
  {label: 'SAL6 word immediate', code: '66c1f001', eax: 0x12348001, after: 0x12340002, flags: 0xc03},
  {label: 'SAL6 dword one', code: 'd1f0', eax: 0x80000001, after: 2, flags: 0xc03},
  {label: 'SAL6 dword CL', code: 'd3f0', eax: 0x80000001, after: 2, flags: 0xc03},
  {label: 'SAL6 dword immediate', code: 'c1f001', eax: 0x80000001, after: 2, flags: 0xc03},
  {label: 'SAL6 CL32 is zero count', code: 'd2f0', eax: 0x12345681, ecx: 0xdead0020, after: 0x12345681, flags: FLAGS},
  {label: 'D16 SAL6 word default', code: 'd1f0', eax: 0x12348001, after: 0x12340002, flags: 0xc03, d16: true},
  {label: 'D16 SAL6 66 dword immediate33', code: '66c1f021', eax: 0x80000001, after: 2, flags: 0xc03, d16: true},
  {label: 'MOVZX word-to-word D32 preserves high EAX and all flags', code: '660fb7c1', eax: 0xdeadbeef, ecx: 0xa5a58001, after: 0xdead8001, flags: MOVE_FLAGS, inputFlags: MOVE_FLAGS},
  {label: 'MOVSX word-to-word D16 preserves high EAX and all flags', code: '0fbfc1', eax: 0xbabe1234, ecx: 0xb6b6ff80, after: 0xbabeff80, flags: MOVE_FLAGS, inputFlags: MOVE_FLAGS, d16: true},
  {label: 'canonical 80 ADD control', code: '80c001', eax: 0x1234567f, after: 0x12345680, flags: 0xc92},
  {label: 'canonical SHL4 word control', code: '66d1e0', eax: 0x12348001, after: 0x12340002, flags: 0xc03},
];
const memoryRows = [
  {label: 'MOVZX equal word memory reads exactly16bits and preserves high EAX', code: '660fb703', width: 2, before: '80ff', after: '80ff', value: 0xff80, flags: MOVE_FLAGS, inputFlags: MOVE_FLAGS,
    eax: 0xdeadbeef, afterEax: 0xdeadff80, set: {3: DATA}, stores: false},
  {label: 'MOVSX D16 address32 equal word memory preserves high EAX', code: '670fbf03', width: 2, before: '80ff', after: '80ff', value: 0xff80, flags: MOVE_FLAGS, inputFlags: MOVE_FLAGS,
    eax: 0xbabe1234, afterEax: 0xbabeff80, set: {3: DATA}, stores: false, d16: true},
  {label: '82 ADD through based DS', code: '820001', width: 1, before: '7f', after: '80', value: 0x7f, flags: 0xc92,
    set: {0: 0x10}, dsBase: 0x5000, stores: true, storeValue: 0x80},
  {label: '82 CMP memory reads without write', code: '823801', width: 1, before: '7f', after: '7f', value: 0x7f, flags: 0x406,
    set: {0: DATA}, stores: false},
  {label: 'SAL6 word uses implicit SS', code: '66d13424', width: 2, before: '0180', after: '0200', value: 0x8001, flags: 0xc03,
    set: {4: DATA}, stores: true, storeValue: 2},
  {label: 'SAL6 dword address16 and count33', code: '67c13021', width: 4, before: '01000080', after: '02000000', value: 0x80000001, flags: 0xc03,
    set: {0: 0x6000, 3: 0xdead5000, 6: 0xbeef0010}, stores: true, storeValue: 2},
];
const faultRows = [
  {label: '82 ADD null DS before page helper', code: '820001', width: 1, set: {0: 0x6000}, context: 'nullDS', reason: 11},
  {label: 'SAL6 word logical SS span before helper', code: '66d13424', width: 2, set: {4: DATA}, context: 'shortSS', reason: 15},
  {label: '82 ADD modify page fault', code: '820001', width: 1, set: {0: 0x6000}, fault: [1, 0x6000, 1]},
  {label: 'SAL6 word write authority denied before publication', code: '66d13424', width: 2, set: {4: DATA}, fault: [2, DATA, 2], readonly: true},
];
const strictRows = [
  {label: 'duplicate66 alias remains host refusal', code: '666682c001'},
  {label: 'LOCK CMP82 remains invalid', code: 'f0823801'},
  {label: 'LOCK SAL6 remains invalid', code: 'f0d130'},
];
function seed(row) {
  const regs = [...registers]; regs[0] = row.eax ?? regs[0]; regs[1] = row.ecx ?? 0xdead0001;
  for (const [index, value] of Object.entries(row.set ?? {})) regs[Number(index)] = value;
  const input = state(h.initial, {regs, flags: row.inputFlags ?? FLAGS, bases: [0, 0, 0, row.dsBase ?? 0, 0, 0]});
  if (row.d16) {
    const sg = record('R3SG', 64, [], 3);
    [0x23, 0x1b, 0x23, 0x23, 0x3b, 0].forEach((v, i) => sg.writeUInt16LE(v, 16 + 2 * i));
    [0xf33fffff, 0xf1bfffff, 0xf33fffff, 0xf33fffff, 0xf33fffff, 0]
      .forEach((v, i) => sg.writeUInt32LE(v >>> 0, [28, 32, 36, 40, 44, 56][i]));
    input.set(sg, SEG); input.set(record('R3SB', 40, [0, 0, 0, 0, 0, 0]), BASES);
  }
  if (row.context) {
    const sg = record('R3SG', 64, [], 2);
    [0x23, 0x1b, 0x23, row.context === 'nullDS' ? 0 : 0x23, 0x3b, 0]
      .forEach((v, i) => sg.writeUInt16LE(v, 16 + 2 * i));
    [0xf33fffff, 0xf3bfffff, row.context === 'shortSS' ? 0xf2305010 : 0xf33fffff,
      row.context === 'nullDS' ? 0 : 0xf33fffff, 0xf33fffff, 0]
      .forEach((v, i) => sg.writeUInt32LE(v >>> 0, [28, 32, 36, 40, 44, 56][i]));
    input.set(sg, SEG);
  }
  return input;
}
function open(resident, entries, row, input) {
  const c = h.open(resident, entries, row.label, [[PC, 7], [0x5000, 3], [0x7000, 3]]);
  h.upload(c, PC, Buffer.concat([bytes(row.code), bytes('ebfe')]));
  h.upload(c, 0x5000, Buffer.alloc(4096, 0xa6)); h.upload(c, 0x7000, Buffer.alloc(4096, 0x5a));
  h.upload(c, DATA, bytes(row.before ?? '0180'));
  if (row.readonly) h.host(c, 'protect', [0x5000, 1, 1]);
  h.put(c, 0, input); return c;
}
const readName = row => `cpu_read${row.width * 8}`;
const modifyName = row => `cpu_read_modify${row.width * 8}`;
const storeName = (c, row) => `cpu_store${c.resident ? '_resident' : ''}${row.width * 8}`;
const trace = (name, args) => ({name, args: args.map(v => v >>> 0), status: 0});
const storeArgs = (c, material, address, value) => [...(c.resident ? material.binding : []), address, value];
let planned = 0, invoked = 0, compileControls = 0;
function invoke(c, material, input, want, options = {}) {
  planned++;
  if (material) {invoked++; h.observe(c, c.label, {input, want, material, ...options});}
  else h.skip(c, c.label, 'required real alias compilation');
}
for (const resident of [false, true]) for (const entries of [false, true]) {
  for (const row of registerRows) {
    const input = seed(row), c = open(resident, entries, row, input);
    const material = h.compile(c, 'real register alias body', [[PC, bytes(row.code).length + 2]], []);
    const want = Buffer.from(input); want.writeUInt32LE(row.after, 16); want.writeUInt32LE(row.flags, 52);
    want.writeUInt32LE(PC + bytes(row.code).length, 48); exit(want, 1, 1, 1);
    invoke(c, material, input, want); h.host(c, 'close');
  }
  for (const row of memoryRows) {
    const input = seed(row), c = open(resident, entries, row, input);
    const names = [readName(row), ...(row.stores ? [modifyName(row), storeName(c, row)] : [])];
    const material = h.compile(c, 'real memory alias body', [[PC, bytes(row.code).length + 2]], names);
    const want = Buffer.from(input); want.writeUInt32LE(row.flags, 52);
    if (row.afterEax !== undefined) want.writeUInt32LE(row.afterEax, 16);
    want.writeUInt32LE(PC + bytes(row.code).length, 48); exit(want, 1, 1, 6);
    want.set(record('R3MH', 40, [0, row.stores ? 0 : row.value, 0, 0, 0, row.width], 10), 100);
    const wantRAM = h.ram(c); writeRAM(wantRAM, DATA, bytes(row.after));
    const helpers = [trace(row.stores ? modifyName(row) : readName(row), [DATA])];
    if (row.stores) helpers.push(trace(storeName(c, row), storeArgs(c, material ?? {binding: []}, DATA, row.storeValue)));
    invoke(c, material, input, want, {wantRAM, helpers}); h.host(c, 'close');
  }
  for (const row of faultRows) {
    const input = seed(row), c = open(resident, entries, row, input);
    const names = [readName(row), modifyName(row), storeName(c, row)];
    const material = h.compile(c, 'alias fault authority body', [[PC, bytes(row.code).length + 2]], names);
    const want = exit(Buffer.from(input), row.reason ?? 5, 0, 6), helpers = [];
    if (row.fault) {
      const fault = {errorCode: row.readonly ? 7 : 6, address: row.fault[1], access: 2, length: row.width};
      want.set(pageFaultExit({retired: 0, ...fault}), 56);
      want.set(pageFaultPacket(fault), 100);
      helpers.push({name: modifyName(row), args: [row.fault[1]], status: 23});
    }
    invoke(c, material, input, want, {helpers}); h.host(c, 'close');
  }
  for (const row of strictRows) {
    const input = seed(row), c = open(resident, entries, row, input);
    h.compile(c, row.label, [[PC, bytes(row.code).length + 2]], [], {expectedStatus: 10});
    compileControls++; h.host(c, 'close');
  }
}
assert.equal(planned, 136, '34 literal semantic vectors across four bound profiles');
assert.equal(compileControls, 12, 'three unchanged strict prefix/LOCK controls per profile');
assert.deepEqual(pins(), sourcePins, 'source and independent literal oracles remain fixed');
writeFileSync(join(output, 'alias-contract.json'), JSON.stringify({source_pins: sourcePins,
  register_vectors: registerRows.length, memory_vectors: memoryRows.length, fault_vectors: faultRows.length,
  planned_invocations: planned, invoked, strict_compile_controls: compileControls,
  expected_when_admitted: {contexts: 148, modules: 136, calls: 136, observations: 284}}, null, 2) + '\n');
h.finish([
  'Original17 AMD-defined arithmetic/shift aliases plus two separately qualified equal-width word moves; SALC, TEST/r1 and BSWAP16 remain separate.',
  'Equal-width MOVZX/MOVSX retain original extra opcode byte and continuation, high EAX and all status flags; memory forms perform real CPUread16 with unchanged RAM and literal v10 packets, never NOP or widening.',
  'Whole Rust-produced4796 arena and all mapped RAM are compared; real guards and CPU scalar helpers, literal packets and traces remain enabled.',
  'Literal GPR/flags/count/width/D16 and nonflat DS, implicit SS, address16 and logical/page/modify-authority fault cases; canonical80 and SHL4 controls retained.',
  'Shared unchanged fetch/currentness, RF/budget/cancel and helper protocol proofs are reused; no enabled hardware-vendor or wholeCPU conformance claim.',
]);
