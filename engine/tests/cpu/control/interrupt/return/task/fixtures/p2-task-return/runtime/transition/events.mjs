import assert from 'node:assert/strict';
import {caches, incomingFlags, savedRAM, selectors, taskContext} from './images.mjs';

export function events({h, record, words, exit, SEG, BASES, observed, open, compile, observe,
  wantEntered, taskHelpers, call, bound, cloneRAM, writeRAM}) {
  const start = {contexts: observed.contexts, compiles: observed.compiles,
    generated: observed.generatedInvocations, host: observed.hostQueryAndRefusalInvocations};
  const finish = (epoch, reason, detail = 0) => ({name: 'finish_exception_detection',
    args: [String(epoch), reason, detail, 0, 0, 0, 0, 0, 0, 0, 0, 0], status: 0});
  function request(c, label, input, payload) {
    const want = Buffer.from(input); want.set(payload, 140);
    const material = {run: () => {const status = c.api.read_task_context();
      c.calls.push(call('read_task_context', [], status)); return status;}};
    observed.invocations++; observed.hostQueryAndRefusalInvocations++;
    const row = h.observe(c, label, {material, input, want, helpers: [call('read_task_context', [])],
      kind: 'task-query-export'}); row.classification = 'executed-host-contract'; return row;
  }
  function partial(state, row) {
    return taskContext(record, {state, loaded: 0x26,
      selected: [0x23, 0x1b, row.wrap ? 0x5b : 0x23, 0x23, 0x3b, 0],
      bases: [0, 0, row.wrap ? 0xfffffff0 : 0, 0, 0, 0],
      cache: [0, caches[1], row.wrap ? 0xf03000ff : caches[2], 0, 0, 0]});
  }
  function canonical(want, row, cs) {
    const selected = [0x23, cs, row.wrap ? 0x5b : 0x23, 0x23, 0x3b, 0];
    const sg = record('R3SG', 64, [], row.wrap ? 3 : 1), bases = record('R3SB', 40);
    selected.forEach((value, index) => sg.writeUInt16LE(value, 16 + 2 * index));
    if (row.wrap) {
      [caches[0], caches[1], 0xf03000ff, caches[3], caches[4], 0]
        .forEach((value, index) => sg.writeUInt32LE(value >>> 0, [28, 32, 36, 40, 44, 56][index]));
      bases.writeUInt32LE(0xfffffff0, 24);
    }
    want.set(sg, SEG); want.set(bases, BASES);
    return {selected, cache: [caches[0], caches[1], row.wrap ? 0xf03000ff : caches[2], caches[3], caches[4], 0],
      bases: [0, 0, row.wrap ? 0xfffffff0 : 0, 0, 0, 0]};
  }
  for (const [resident, entries, wrap] of [[false, false, false], [true, true, true]]) {
    const {c, input, row} = open(resident, entries, {label: 'postload CS NP with qualified new SS/' + (wrap ? 'B16 wrap' : 'B32'),
      csFault: true, wrap, idt: [8, 11, 12]});
    const material = compile(c, row.label, row.code, true);
    const want = Buffer.from(input), frameESP = wrap ? 0xdead0008 : 0x77f0;
    const regs = [...row.general]; regs[4] = frameESP;
    want.set(record('R3ST', 56, [...regs, 0x3000, incomingFlags], 3));
    want.set(record('R3MH', 40, [0, 0x3000, 0, 0, 0, 0x204], 12), 100); exit(want, 3, 1, 6);
    const wantRAM = savedRAM(cloneRAM, writeRAM, h.ram(c), row.current, row.code, row.oldRegisters);
    if (wrap) {
      writeRAM(wantRAM, 4, words([0x250ed7])); writeRAM(wantRAM, 0, words([0x63]));
      writeRAM(wantRAM, 0xfffffffc, words([0x2000])); writeRAM(wantRAM, 0xfffffff8, words([0x60]));
    } else writeRAM(wantRAM, 0x77f0, words([0x60, 0x2000, 0x63, 0x250ed7]));
    const entered = observe(c, 'committed new task NP60 enters handler using rawCS and new SS', material, input, want,
      {wantRAM, helpers: [...taskHelpers(c, material, row), finish(1, 14, 0x60)]});
    if (!entered?.passed) {h.skip(c, 'partial handler/recovery', 'new-task handler entry'); h.host(c, 'close'); continue;}
    const raw = partial(want, row);
    request(c, 'postentry R3TC exposes handlerCS/newSS and unresolved data caches', want, raw);
    const handlerInput = Buffer.from(want); handlerInput.set(raw, 140); h.put(c, 0, handlerInput);
    observed.compiles++;
    const handler = h.compile(c, 'partial handler consumes error through qualified SS then changes EAX',
      [[0x3000, 8]], ['read_task_context', 'cpu_read32', 'finish_exception_detection']);
    const handlerWant = Buffer.from(handlerInput); handlerWant.writeUInt32LE(0xdeadc0de, 16);
    handlerWant.writeUInt32LE(0x60, 24); handlerWant.writeUInt32LE(wrap ? 0xdead000c : 0x77f4, 32);
    handlerWant.writeUInt32LE(0x3006, 48); handlerWant.set(record('R3MH', 40, [0, 0x60, 0, 0, 0, 4], 10), 100);
    exit(handlerWant, 1, 2, 6);
    const handled = observe(c, 'real partial POP EDX uses new SS.B/base and preserves highESP', handler, handlerInput, handlerWant,
      {budget: 2, helpers: [call('read_task_context', []), call('cpu_read32', [wrap ? 0xfffffff8 : 0x77f0])]});
    if (!handled?.passed) {h.skip(c, 'complete segment/IRET recovery', 'partial handler'); h.host(c, 'close'); continue;}
    const updated = partial(handlerWant, row);
    request(c, 'partial readback follows live handler GPRs without claiming data caches', handlerWant, updated);
    const repairBefore = h.arena(c), repairRAM = h.ram(c);
    h.host(c, 'configure_segment_descriptor_raw', [0x60, 0xffff, 0x00cffa00]);
    assert.deepEqual(h.arena(c), repairBefore, 'host descriptor presence repair is arena neutral');
    assert.deepEqual(h.ram(c), repairRAM, 'private descriptor repair invents no GDT RAM write');
    const recoveryInput = Buffer.from(handlerWant); recoveryInput.set(updated, 140);
    const recoveryHex = '66b823008ec08ed866b83b008ee066b800008ee8cf';
    h.upload(c, 0x3006, Buffer.from(recoveryHex, 'hex')); h.put(c, 0, recoveryInput); observed.compiles++;
    const load = 'segment_load' + (resident ? '_resident' : ''), far = 'far_return' + (resident ? '_resident' : '');
    const task = 'cpu_task_return' + (resident ? '_resident' : '');
    const recovery = h.compile(c, 'real MOV-segment repair reaches canonical caches then real IRET', [[0x3006, recoveryHex.length / 2]],
      ['read_task_context', 'cpu_read', load, far, task, 'finish_exception_detection']);
    const recoveryWant = Buffer.from(recoveryInput); recoveryWant.writeUInt32LE(0xdead0000, 16);
    recoveryWant.writeUInt32LE(row.general[4], 32); recoveryWant.writeUInt32LE(0x2000, 48);
    recoveryWant.writeUInt32LE(0x250ed7, 52); const complete = canonical(recoveryWant, row, 0x63);
    recoveryWant.set(record('R3MH', 40, [0, 0x2000, 0, 0, 0, 0], 7), 100);
    // Refresh preserves live local registers. Its serialized State32 is the last published entry snapshot.
    recoveryWant.set(taskContext(record, {state: recoveryInput, ...complete}), 140); exit(recoveryWant, 1, 8, 6);
    const trace = [call('read_task_context', [])];
    for (const [segment, selector] of [[0, 0x23], [3, 0x23], [4, 0x3b], [5, 0]]) {
      trace.push(call(load, bound(c, recovery, [segment, selector])), call('read_task_context', []));
    }
    for (const address of wrap ? [0xfffffffc, 0, 4] : [0x77f4, 0x77f8, 0x77fc]) trace.push(call('cpu_read', [address, 4, 1]));
    trace.push(call(far, bound(c, recovery, [0x2000, 0x63, 0])), call('read_task_context', []));
    const recovered = observe(c, 'all data caches repair canonically; IRET restores newtaskCS/EIP/RF', recovery, recoveryInput, recoveryWant,
      {budget: 8, helpers: trace});
    if (recovered?.passed) {
      h.put(c, 0, recoveryWant); observed.compiles++;
      const resumed = h.compile(c, 'fresh full-context target after recovered IRET', [[0x2000, 3]], ['finish_exception_detection']);
      const resumeWant = Buffer.from(recoveryWant); resumeWant.writeUInt32LE(0x2001, 48);
      resumeWant.writeUInt32LE(incomingFlags, 52); exit(resumeWant, 1, 1, 1);
      observe(c, 'resumed new task executes NOP and consumes returned RF once', resumed, recoveryWant, resumeWant);
    } else h.skip(c, 'resumed newtask instruction', 'complete IRET recovery');
    h.host(c, 'close');
  }
  {
    const {c, input, row} = open(false, false, {label: 'postload SS NP has no old stack fallback', ssFault: true, idt: [8, 11, 12]});
    const material = compile(c, row.label, row.code, true), want = Buffer.from(input);
    want.set(record('R3ST', 56, [...row.general, 0x2000, incomingFlags], 2));
    want.set(record('R3MH', 40, [7, 0, 1, 0, 0, 0x204], 12), 100);
    exit(want, 15, 1, 6); want.writeUInt32LE(0x60, 80);
    const wantRAM = savedRAM(cloneRAM, writeRAM, h.ram(c), row.current, row.code, row.oldRegisters);
    const shut = observe(c, 'committed SS60 follows secondary and DF failure into real shutdown', material, input, want,
      {wantRAM, helpers: [...taskHelpers(c, material, row), finish(1, 15, 0x60)]});
    if (shut?.passed) {
      const fresh = observe(c, 'actual emitted guard rejects execution after shutdown', material, want, want, {status: 22});
      if (fresh) assert.equal(fresh.calls.find(item => item.name === 'guard_detect')?.status, '-22');
    } else h.skip(c, 'latched shutdown guard', 'SS task shutdown');
    h.host(c, 'close');
  }
  for (const [device, deviceHex, automatic] of [
    ['FNOP', 'd9d0', false], ['WAIT', '9b', false],
    ['PXOR XMM0', '660fefc0', false], ['VXORPS YMM0', 'c5fc57c0', false], ['FNOP', 'd9d0', true],
  ]) {
    const {c, input, row} = open(false, false, {label: 'CR0.TS causes ' + device + ' NM before pendingMF/' + automatic,
      nm: true, deviceHex, flags: 0x250ed7, ...(automatic ? {idt: [7]} : {})});
    const material = compile(c, row.label, row.code, automatic), want = wantEntered(input, row);
    const wantRAM = savedRAM(cloneRAM, writeRAM, h.ram(c), row.current, row.code, row.oldRegisters);
    const entered = observe(c, 'real task switch sets TS and retains incoming RF/pending FPU state', material, input, want,
      {wantRAM, helpers: taskHelpers(c, material, row)});
    if (!entered?.passed) {h.skip(c, 'newtask NM instruction', 'task switch'); h.host(c, 'close'); continue;}
    h.put(c, 0, want); observed.compiles++;
    const fp = h.compile(c, 'fresh ' + device + ' is decoded with live TS, without FP helper', [[0x2000, deviceHex.length / 2]],
      automatic ? ['finish_exception_detection'] : []);
    const fpWant = Buffer.from(want), fpRAM = cloneRAM(wantRAM);
    if (automatic) {
      fpWant.writeUInt32LE(0x3000, 48); fpWant.writeUInt32LE(0x77f4, 32);
      fpWant.writeUInt32LE(incomingFlags, 52); fpWant.set(record('R3MH', 40, [0, 0x3000, 0, 0, 0, 4], 12), 100);
      exit(fpWant, 3, 0, 11); writeRAM(fpRAM, 0x77f4, words([0x2000, 0x1b, 0x250ed7]));
    } else exit(fpWant, 22, 0, 11);
    observe(c, automatic ? 'real NM enters vector7; pendingMF never runs' : 'raw ' + device + ' NM keeps incomingRF before begin and never calls FP helper',
      fp, want, fpWant, {wantRAM: fpRAM, helpers: automatic ? [finish(2, 22)] : []});
    h.host(c, 'close');
  }
  return {contexts: observed.contexts - start.contexts, compiles: observed.compiles - start.compiles,
    generated: observed.generatedInvocations - start.generated, requests: observed.hostQueryAndRefusalInvocations - start.host};
}
