import assert from 'node:assert/strict';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {pathToFileURL} from 'node:url';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root);
const support = 'engine/tests/cpu/segments/code/base/fixtures/p2-cs-base/runtime/support.mjs';
const {harness, PC, SEG, BASES, registers, record, words, exit, writeRAM, hash} =
  await import(pathToFileURL(join(root, support)));
const h = await harness(enginePath, output, root);
const paths = [
  'engine/src/cpu/dbt/wasm/detection.rs',
  'engine/src/process/wasm/detection.rs',
  'engine/src/abi/wasm/exports/detection.rs',
  'engine/src/process/cpu/interrupts/detection.rs',
  'engine/src/process/cpu/interrupts/entry.rs',
  'engine/src/process/cpu/interrupts/entry/state.rs',
  'engine/src/abi/mod.rs',
  'engine/src/abi/exception.rs',
  'engine/src/abi/exception/request.rs',
  'engine/src/abi/exception/request/tests.rs',
  'engine/src/abi/exception/tests.rs',
  'engine/src/process/cpu/interrupts/request.rs',
  'engine/src/process/cpu/interrupts/request/tests.rs',
  'engine/src/process/cpu/interrupts/mod.rs',
  'engine/src/process/wasm/mod.rs',
  'engine/src/cpu/x86/mod.rs',
  'engine/src/cpu/x86/interrupts/mod.rs',
  'engine/src/process/wasm/exceptions.rs',
  'engine/src/cpu/x86/interrupts/cause.rs',
  'engine/src/cpu/x86/interrupts/cause/paging.rs',
  'engine/src/cpu/x86/interrupts/frame.rs',
  'engine/src/cpu/x86/interrupts/outcome.rs',
  'engine/src/cpu/x86/interrupts/gates.rs',
  'engine/src/cpu/x86/interrupts/escalation.rs',
  'engine/src/process/cpu/interrupts/delivery.rs',
  'engine/src/abi/wasm/exports/cpu.rs',
  'engine/src/process/wasm/cpu.rs',
  'engine/src/cpu/dbt/wasm/control_instructions.rs',
  'engine/src/cpu/dbt/wasm/strings.rs',
  'engine/src/cpu/x86/decode/xstate.rs',
  'engine/src/cpu/dbt/wasm/xstate.rs',
  'engine/src/cpu/x86/capabilities.rs',
  'engine/src/cpu/dbt/wasm/far/interrupt.rs',
  'engine/tests/cpu/control/interrupt/exception/native.rs',
  'engine/tests/cpu/control/interrupt/exception/wasm.rs',
  'engine/tests/cpu/control/interrupt/exception/fixtures/p2-exception-entry/runtime/run.mjs',
  support,
];
const pins = () => Object.fromEntries(paths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = pins(), FLAGS = 0x240ed7, TARGET = 0x2300, TARGET_BASE = 0x4000;
const observed = {contexts: 0, compileCalls: 0, guestCalls: 0, eventCalls: 0};
const call = (name, args, status = 0) => ({name, args: args.map(v => v >>> 0), status});
const bound = (c, material, args) => [...(c.resident ? material.binding : []), ...args];
const eventName = c => `deliver_exception${c.resident ? '_resident' : ''}`;
const returnName = c => `far_return${c.resident ? '_resident' : ''}`;
const packet = fields => record('R3MH', 40, fields, 12);

function context(bytes, {cs = 0x1b, base = 0, default32 = true, stack32 = true,
  stackCache = stack32 ? 0xf33fffff : 0xf13fffff, legacy = false} = {}) {
  const sg = record('R3SG', 64, [], legacy ? 1 : 3);
  [0x23, cs, 0x23, 0x23, 0x3b, 0].forEach((value, index) => sg.writeUInt16LE(value, 16 + 2 * index));
  if (!legacy) [0xf33fffff, default32 ? 0xf3bfffff : 0xf1bfffff, stackCache, 0xf33fffff, 0xf33fffff, 0]
    .forEach((value, index) => sg.writeUInt32LE(value >>> 0, [28, 32, 36, 40, 44, 56][index]));
  bytes.set(sg, SEG); bytes.set(record('R3SB', 40, [0, base, 0, 0, 0, 0]), BASES);
  return bytes;
}
function seed(row) {
  const input = Buffer.from(h.initial), regs = [...registers];
  regs[4] = row.esp ?? 0x8000;
  if (row.ecx !== undefined) regs[1] = row.ecx;
  if (row.rep) {regs[1] = 2; regs[6] = 0x5000; regs[7] = 0x5100;}
  input.set(record('R3ST', 56, [...regs, PC, (row.flags ?? FLAGS) | 0x10000], 3));
  input.writeUInt32LE(row.rep || row.step ? 0 : 1, 12);
  context(input, row);
  for (let i = 0; i < 80; i++) input[4276 + i] = (i * 29 + 17) & 255;
  for (let i = 0; i < 8; i++) {
    input.fill(0x30 + i, 4396 + i * 16, 4412 + i * 16);
    input.fill(0x70 + i, 4628 + i * 16, 4644 + i * 16);
  }
  return input;
}
function rawCode({default32 = true, limit} = {}) {
  const paged = limit === undefined; limit ??= 0xfffff;
  return [((TARGET_BASE << 16) | (limit & 0xffff)) >>> 0,
    ((limit & 0xf0000) | Number(default32) << 22 | Number(paged) << 23 | 0xfa00) >>> 0];
}
function configure(c, row) {
  const idt = record('R3ID', 2068); idt.writeUInt16LE(0x7ff, 16);
  idt.set(words([((row.nullTarget ? 0 : 0x48) << 16) | TARGET,
    (row.present === false ? 0 : 0x8000) | (row.kind ?? 15) << 8]), 20 + row.vector * 8);
  h.put(c, 140, idt); h.host(c, 'configure_interrupt_descriptor_table');
  h.host(c, 'configure_segment_descriptor_raw', [0x48, ...rawCode({default32: row.targetD ?? true,
    limit: row.badTarget ? 0xfff : undefined})]);
}
function request(bytes, row) {
  const req = record('R3EV', 64, [row.tag, row.detail ?? 0, 0, 0,
    row.stopPC ?? PC, row.esp ?? 0x8000, row.flags ?? FLAGS, 0x1b]);
  bytes.set(req, 140); return bytes;
}
function suppliedStop(input, row) {
  const want = Buffer.from(input); want.writeUInt32LE(0, 12);
  want.writeUInt32LE(row.flags ?? FLAGS, 52); want.writeUInt32LE(row.stopPC ?? PC, 48);
  exit(want, row.reason, row.retired ?? 0, row.exitVersion ?? 7);
  if (row.rep) {
    // one completed element consumes work even though unfinished REP retires zero.
    want.writeUInt32LE(1, 4604);
    want.writeUInt32LE(1, 20); want.writeUInt32LE(0x5001, 40); want.writeUInt32LE(0x5101, 44);
    want.set(record('R3MH', 40, [0, 0, 0, 0, 0, 1], 10), 100);
  }
  return want;
}
function entryWant(input, row) {
  const want = Buffer.from(input); want.set(packet(row.fields ?? [0, TARGET, 0, 0, 0, row.info ?? 4]), 100);
  if (!row.fields) {
    context(want, {cs: 0x4b, base: TARGET_BASE, default32: row.targetD ?? true,
      stack32: row.stack32 ?? true, stackCache: row.stackCache});
    want.writeUInt32LE(TARGET, 48); want.writeUInt32LE(row.newESP ?? 0x7ff4, 32);
    want.writeUInt32LE((row.flags ?? FLAGS) & ~0x34100, 52); want.writeUInt32LE(0, 12);
    exit(want, 3, row.retired ?? 0, row.entryVersion ?? row.exitVersion ?? 7);
  }
  return want;
}
function desiredRAM(c, changes) {
  const ram = h.ram(c);
  for (const [address, hex] of changes) writeRAM(ram, address, Buffer.from(hex, 'hex'));
  return ram;
}
function compile(c, label, spans, helpers = []) {
  observed.compileCalls++; return h.compile(c, label, spans, helpers);
}
function observe(c, label, material, input, want, options = {}) {
  if (!material) {h.skip(c, label, 'earlier material admission'); return null;}
  observed.guestCalls++;
  return h.observe(c, label, {material, input, want, ...options});
}
function enter(c, label, material, input, want, options = {}) {
  if (!material) {h.skip(c, label, 'earlier material admission'); return null;}
  const name = eventName(c), args = c.resident ? material.binding : [];
  assert.equal(typeof c.api[name], 'function', 'select the event-foundation producer');
  const direct = {run() {
    const status = c.api[name](...args);
    c.calls.push(call(name, args, status)); return status;
  }};
  observed.eventCalls++;
  return h.observe(c, label, {material: direct, input, want, kind: 'explicit-supplied-event-entry',
    helpers: [call(name, args, options.status ?? 0)], ...options});
}

const rows = [
  {label: 'UD fault no-error32 DPL0 to D16 with sticky-v9 exit', code: '0f0b', tag: 3, vector: 6,
    reason: 20, targetD: false, entryVersion: 9, handler: '66cf',
    frame: [[0x7ff4, '001000001b000000d70e2500']]},
  {label: 'GP0 fault fourth-word slot B16 highESP and explicit v6 ingress', code: '0f01d0ebfe', ecx: 1, tag: 6, vector: 13, reason: 11,
    exitVersion: 9, entryVersion: 6, kind: 7, stack32: false, esp: 0xabcd8000, newESP: 0xabcd7ff8,
    info: 0x202, handler: '83c40266cf', frame: [[0x7ff8, '000000101b00d70e']]},
  {label: 'CC software DPL check uses EXT0', code: 'cc', tag: 9, vector: 3, reason: 17,
    stopPC: PC + 1, retired: 1, fields: [3, 0, 0x1a, 0, 0, 0]},
  {label: 'F1 bypasses DPL and saves completed RF0', code: 'f1', tag: 11, detail: 0,
    vector: 1, reason: 16, stopPC: PC + 1, retired: 1,
    frame: [[0x7ff4, '011000001b000000d70e2400']]},
  {label: 'processor outer NP has IDT EXT1', code: '0f0b', tag: 3, vector: 6,
    reason: 20, present: false, fields: [6, 0, 0x33, 0, 0, 0]},
  {label: 'processor null target has selected GP1', code: '0f0b', tag: 3, vector: 6,
    reason: 20, nullTarget: true, fields: [3, 0, 1, 0, 0, 4]},
  {label: 'fourth error slot page failure preserves earlier RAM and stop', code: '0f01d0ebfe', ecx: 1, tag: 6,
    vector: 13, reason: 11, exitVersion: 9, esp: 0x700c, absentStackTail: true,
    fields: [1, 0, 1, 0x6ffc, 2, 0x204]},
  {label: 'fourth cached SS slot precedes target limit and pages', code: '0f01d0ebfe', ecx: 1, tag: 6,
    vector: 13, reason: 11, exitVersion: 9, esp: 0x700c, stackCache: 0xf2706fff, badTarget: true, absentStackTail: true,
    fields: [7, 0, 1, 0, 0, 0x204]},
  {label: 'TF trap saves taken JMP continuation', code: 'e9fb010000', tag: 11, detail: 1,
    vector: 1, reason: 16, stopPC: 0x1200, flags: FLAGS | 0x100, step: true, retired: 1,
    frame: [[0x7ff4, '001200001b000000d70f2400']]},
  {label: 'partial REP trap preserves progress and saves RF1', code: 'f3a4', tag: 11, detail: 2,
    vector: 1, reason: 16, flags: (FLAGS & ~0x400) | 0x100, rep: true,
    frame: [[0x7ff4, '001000001b000000d70b2500']]},
  {label: 'fault frame SMC commits entry with original retired0', code: '0f0b', tag: 3,
    vector: 6, reason: 20, esp: 0x100c, newESP: 0x1000, smc: true,
    frame: [[0x1000, '001000001b000000d70e2500']]},
];

for (const resident of [false, true]) for (const entries of [false, true]) for (const row of rows) {
  observed.contexts++;
  // only handler families require 6000; these declared tail faults require it absent.
  const dataPages = [0x5000, ...(row.absentStackTail ? [] : [0x6000]), 0x7000];
  const c = h.open(resident, entries, row.label, [[PC], ...dataPages.map(page => [page])]);
  for (const page of dataPages) h.upload(c, page, Buffer.alloc(4096, 0x6b));
  configure(c, row);
  // this api accepts explicit trusted stops; real instruction provenance is tested by p2-exception-auto.
  h.upload(c, PC, Buffer.from('90ebfe', 'hex'));
  if (row.handler) h.upload(c, TARGET_BASE + TARGET, Buffer.from(row.handler, 'hex'));
  if (row.rep) {
    h.upload(c, 0x5000, Buffer.from([0x7a, 0x8b]));
    h.upload(c, 0x5100, Buffer.from([0x7a]));
  }
  const input = seed(row); h.put(c, 0, input);
  const material = compile(c, row.label + ': current bound NOP owner', [[PC, 3]], ['finish_exception_detection']);
  const stopped = suppliedStop(input, row);
  // no generated run or detected-provenance claim is made for these supplied R3ST/R3EX inputs.
  if (material) {
    const requested = request(Buffer.from(stopped), row);
    if (row.entryVersion !== undefined) requested.writeUInt16LE(row.entryVersion, 60);
    if (!resident && !entries && row === rows[0]) {
      for (const [label, offset, value] of [
        ['request EIP mismatch is neutral', 172, PC + 1],
        ['PF tag12 has no detection authority', 156, 12],
      ]) {
        const invalid = Buffer.from(requested); invalid.writeUInt32LE(value, offset);
        enter(c, label, material, invalid, invalid, {status: 7, wantRAM: desiredRAM(c, [])});
      }
    }
    const want = entryWant(requested, row);
    const entry = enter(c, row.label + ': explicit typed single-attempt bridge', material, requested, want,
      {status: row.smc ? 11 : 0, wantRAM: desiredRAM(c, row.frame ?? [])});
    if (row.handler && entry?.passed) {
      h.put(c, 0, want);
      const target = compile(c, row.label + ': actual handler capture', [[TARGET, row.handler.length / 2]],
        ['cpu_read', returnName(c), 'finish_exception_detection']);
      const handlerInput = Buffer.from(want);
      handlerInput.set(words([TARGET, row.handler.length / 2]), 140);
      // the resident allocator's declared 24-byte publication is the only dynamic input.
      if (c.resident && target) handlerInput.set(h.arena(c).subarray(140, 164), 140);
      const returned = Buffer.from(handlerInput), slot = row.kind === 7 ? 2 : 4;
      context(returned, {stack32: row.stack32 ?? true, legacy: row.stack32 !== false});
      returned.writeUInt32LE(PC, 48); returned.writeUInt32LE(row.esp ?? 0x8000, 32);
      returned.writeUInt32LE(slot === 4 ? FLAGS | 0x10000 : FLAGS, 52);
      returned.set(record('R3MH', 40, [0, PC, 0, 0, 0, 0], 7), 100);
      returned.set(slot === 4 ? Buffer.from('d70e2500', 'hex') : Buffer.from('d70e', 'hex'), 140);
      exit(returned, 1, slot === 4 ? 1 : 2, 6);
      const reads = slot === 4 ? [0x7ff4, 0x7ff8, 0x7ffc] : [0x7ffa, 0x7ffc, 0x7ffe];
      const trace = reads.map(address => call('cpu_read', [address, slot, 1]));
      if (target) trace.push(call(returnName(c), bound(c, target, [PC, 0x1b, Number(slot === 2)])));
      observe(c, row.label + ': real error removal and IRET return', target, handlerInput, returned,
        {budget: slot === 4 ? 1 : 2, helpers: trace});
    } else if (row.handler) h.skip(c, row.label + ': handler/IRET', 'successful event entry');
  } else h.skip(c, row.label + ': event entry', 'current bound owner admission');
  h.host(c, 'close');
}
assert.deepEqual(pins(), sourcePins, 'consumer and source owners stayed fixed');
writeFileSync(join(output, 'event-input-source-pins.json'), JSON.stringify(sourcePins, null, 2) + '\n');
h.finish({planned: {contexts: 44, compiles: 52, generatedModules: 52, guestInvocations: 8,
  explicitEventInvocations: 46, totalInvocations: 54, observations: 106}, observed,
  scope: 'Explicit supplied-event API through real engine Wasm exports and current bound NOP owners; all supplied-state/frame/fault/handler oracles retained. Historical real-stop proof is preserved in the pre-auto freeze; p2-exception-auto separately owns current generated provenance and routing. No PF/CR2 or task-transition claim.'});
