import assert from 'node:assert/strict';
import {controls, taskImage, raw, incomingRegisters, incomingFlags, outgoingFlags,
  selectors, ldt, taskContext, wordTaskImage} from './images.mjs';
import {ordinary} from './ordinary.mjs';
import {protocol} from './protocol.mjs';
import {events} from './events.mjs';
import {work} from './work.mjs';
import {formats} from './formats.mjs';

export function transitions(context) {
  const {h, record, words, exit, PC, SEG, BASES, registers, observed, call, bound, cloneRAM, writeRAM} = context;
  const configureTrace = [];
  const taskName = c => 'cpu_task_return' + (c.resident ? '_resident' : '');
  const returnName = c => 'far_return' + (c.resident ? '_resident' : '');
  function config(c, name, payload, status = 0) {
    h.put(c, 140, payload); const before = h.arena(c), beforeRAM = h.ram(c);
    assert.equal(c.api[name](), status, name);
    assert.deepEqual(h.arena(c), before, name + ' complete arena neutrality');
    assert.deepEqual(h.ram(c), beforeRAM, name + ' all RAM neutrality');
    configureTrace.push({context: c.key, name, status});
  }
  function open(resident, entries, supplied) {
    observed.contexts++;
    const row = {current: 0x5000, code: 'cf', flags: incomingFlags, ...supplied};
    row.smc = row.current === 0xfe0;
    row.general = [...(row.general ?? incomingRegisters)];
    row.selected = row.targetWord ? [0x23, 0x1b, 0x23, 0x23, 0, 0] : selectors;
    if (row.wrap) row.general[4] = 0xdead0018;
    const c = h.open(resident, entries, row.label,
      [[0], [PC], [0x2000], [0x3000], [0x5000], [0x6000], [0x7000], [0x9000], ...(row.wrap ? [[0xfffff000]] : [])]);
    c.diagnosticRAM = true;
    for (const page of c.pages) h.upload(c, page, Buffer.alloc(4096, 0xa5));
    const request = record('R3TR', 32, [0x43, 0x6f, ...raw(row.current, row.currentWord ? 0x2b : 0x67, row.currentWord ? 0x83 : 0x8b)]);
    config(c, 'configure_task_return_sources', request);
    h.host(c, 'configure_segment_descriptor_raw', [0x40, ...raw(row.current, row.currentWord ? 0x2b : 0x67, row.currentWord ? 0xe3 : 0xeb)]);
    h.host(c, 'configure_segment_descriptor_raw', [0x48, ...raw(0x6000, row.targetWord ? 0x2c : 0x67, row.targetWord ? 0xe3 : 0xeb)]);
    h.host(c, 'configure_segment_descriptor_raw', [0x50, ...ldt]);
    const cr = controls(record);
    if (!resident && !entries && row.label === 'PG0 dword task return cf') {
      config(c, 'read_task_controls', cr, 7); observed.configurationRefusals++;
      const bad = Buffer.from(cr); bad.writeUInt32LE(1, 36);
      config(c, 'configure_task_controls', bad, 7);
      observed.configurationRefusals++;
    }
    config(c, 'configure_task_controls', cr);
    if (!resident && !entries && row.label === 'PG0 dword task return cf') {
      config(c, 'configure_task_controls', cr, 7); observed.configurationRefusals++;
    }
    if (row.wrap) h.host(c, 'configure_segment_descriptor_raw', [0x58, 0xfff000ff, 0xff00f3ff]);
    if (row.csFault) h.host(c, 'configure_segment_descriptor_raw', [0x60, 0xffff, 0x00cf7a00]);
    if (row.ssFault) h.host(c, 'configure_segment_descriptor_raw', [0x60, 0xffff, 0x00cf7300]);
    if (row.idt) {
      const table = record('R3ID', 2068); table.writeUInt16LE(0x7ff, 16);
      for (const vector of row.idt) table.set(words([0x001b3000, 0x8f00]), 20 + 8 * vector);
      config(c, 'configure_interrupt_descriptor_table', table); c.interruptsConfigured = true;
    }
    h.upload(c, row.current, Buffer.from([0x4b, 0]));
    h.upload(c, 0x6000, row.targetWord ? wordTaskImage({flags: row.flags, general: row.general}) : taskImage({cs: row.csFault ? 0x63 : 0x1b,
      ss: row.ssFault ? 0x63 : row.wrap ? 0x5b : 0x23, flags: row.flags, general: row.general}));
    h.upload(c, PC, Buffer.from(row.code, 'hex'));
    h.upload(c, 0x2000, Buffer.from(row.nm ? row.deviceHex ?? 'd9d0' : '90ebfe', 'hex'));
    h.upload(c, 0x3000, Buffer.from(row.csFault ? '5ab8dec0addeebfe' : 'b8dec0addeebfe', 'hex'));
    const input = Buffer.from(h.initial), oldRegisters = [...(row.sourceRegisters ?? registers)]; oldRegisters[4] = 0xdead000e;
    if (row.repeat) h.upload(c, 0x9000, Buffer.from([0x7e, 0xa5]));
    row.oldRegisters = oldRegisters;
    input.set(record('R3ST', 56, [...oldRegisters, PC, row.sourceFlags ?? outgoingFlags], 3)); input.writeUInt32LE(1, 12);
    const sg = record('R3SG', 64, [], 3);
    selectors.forEach((value, index) => sg.writeUInt16LE(value, 16 + 2 * index));
    [0xf33fffff, 0xf3bfffff, 0xf2300000, 0xf33fffff, 0xf33fffff, 0]
      .forEach((value, index) => sg.writeUInt32LE(value >>> 0, [28, 32, 36, 40, 44, 56][index]));
    input.set(sg, SEG); input.set(record('R3SB', 40), BASES);
    // Existing extended-state sentinels continue to prove task switching does not invent FP state.
    for (let i = 0; i < 80; i++) input[4276 + i] = (i * 29 + 17) & 255;
    for (let i = 0; i < 8; i++) {
      input.fill(0x30 + i, 4396 + i * 16, 4412 + i * 16);
      input.fill(0x70 + i, 4628 + i * 16, 4644 + i * 16);
    }
    if (row.nm) {input.writeUInt16LE(0x037e, 4252); input.writeUInt16LE(0x8081, 4254);}
    row.context = state => taskContext(record, {state});
    h.put(c, 0, input);
    return {c, input, row};
  }
  function compile(c, label, code, detection, extra = []) {
    observed.compiles++;
    const names = ['cpu_read', returnName(c), taskName(c), ...extra, ...(detection ? ['finish_exception_detection'] : [])];
    return h.compile(c, label, [[PC, code.length / 2]], names);
  }
  function observe(c, label, material, input, want, options = {}) {
    if (!material) {h.skip(c, label, 'task transition material admission'); return null;}
    observed.invocations++; observed.generatedInvocations++;
    return h.observe(c, label, {material, input, want, ...options});
  }
  function wantEntered(input, row) {
    const want = Buffer.from(input);
    want.set(record('R3ST', 56, [...row.general, 0x2000, row.flags], row.flags & 0x10000 ? 3 : 2));
    const sg = record('R3SG', 64); row.selected.forEach((value, index) => sg.writeUInt16LE(value, 16 + 2 * index));
    want.set(sg, SEG); want.set(record('R3SB', 40), BASES);
    want.set(record('R3MH', 40, [0, 0, 0, 0, Number(row.smc), 0], 14), 100);
    exit(want, 3, 1, 6); return want;
  }
  function taskHelpers(c, material, row, status = 0) {
    return [call(taskName(c), bound(c, material, [row.nextPC ?? (row.code === 'cf' ? 0x1001 : 0x1002)]), status)];
  }
  const selected = ordinary({...context, open, compile, observe, wantEntered, taskHelpers});
  const routed = events({...context, open, compile, observe, wantEntered, taskHelpers});
  const repeated = work({...context, open, compile, observe, wantEntered, taskHelpers});
  const mixed = formats({...context, open, compile, observe, wantEntered, taskHelpers});
  const checked = protocol({...context, open, compile, observe, wantEntered, taskHelpers});
  return {selected, routed, repeated, mixed, checked, configureTrace};
}
