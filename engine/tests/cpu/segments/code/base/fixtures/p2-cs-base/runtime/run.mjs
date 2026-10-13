import assert from 'node:assert/strict';
import {harness, PC, KEY_HIGH, SEG, BASES, SIZE, words, record, state, setSegments, registers, exit, descriptor, cloneRAM, writeRAM} from './support.mjs';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root, 'explicit engine, output and source root');
const h = await harness(enginePath, output, root);
const body = value => Buffer.concat([Buffer.from([0xb8]), words([value]), Buffer.from([0xeb, 0xfe])]);
const based = {bases: [0, 0x4000, 0, 0, 0, 0]};
const call = (name, args, status = 0) => ({name, args: args.map(v => v >>> 0), status});
const farName = (c, name) => c.resident ? `${name}_resident` : name;
const bound = (c, material, args) => [...(c.resident ? material.binding : []), ...args];
function success(input, pc, {reg, value, version = 1, flags = 2} = {}) {
  const want = Buffer.from(input); want.writeUInt32LE(pc >>> 0, 48); want.writeUInt32LE(flags, 52);
  if (reg !== undefined) want.writeUInt32LE(value >>> 0, 16 + reg * 4); return exit(want, 1, 1, version);
}
function stopped(c, label, helpers = []) {return h.compile(c, label, [], helpers, {stopped: true});}

for (const resident of [false, true]) for (const entries of [false, true]) {
  // Flat material is an execution control on the selected old engine too.
  {
    const c = h.open(resident, entries, 'flat RF and safepoint controls', [[PC]]);
    h.upload(c, PC, Buffer.from([0x90, 0xeb, 0xfe])); h.put(c, 0, state(h.initial));
    const material = h.compile(c, 'flat control compilation', [[PC, 3]]);
    if (material) {
      const input = state(h.initial);
      h.observe(c, 'flat NOP consumes RF only when begun', {input, want: success(input, PC + 1), kind: 'control'});
      h.observe(c, 'flat budget begins no instruction and preserves RF', {input, want: exit(Buffer.from(input), 1, 0), budget: 0, kind: 'control'});
      const cancelled = state(h.initial, {cancel: 1});
      h.observe(c, 'flat cancellation preserves RF', {input: cancelled, want: exit(Buffer.from(cancelled), 2, 0), kind: 'control'});
    } else h.skip(c, 'flat controls', 'flat control compilation');
    h.host(c, 'close');
  }
  {
    const c = h.open(resident, entries, 'physical snapshots with logical EIP', [[PC], [0x5000]]);
    h.upload(c, PC, body(0xdeadbeef)); h.upload(c, 0x5000, body(0x12345678));
    h.put(c, 0, state(h.initial, based));
    const material = h.compile(c, 'based code captures physical 5000', [[PC, 7]]);
    if (material) {
      const input = state(h.initial, based), want = success(input, PC + 5, {reg: 0, value: 0x12345678});
      h.observe(c, 'based body executes physical bytes with logical EIP', {input, want});
      h.upload(c, PC, body(0xfeedface));
      h.observe(c, 'writing logical decoy does not stale physical capture', {input, want});
      h.upload(c, 0x5000, Buffer.from([0xb8]));
      h.observe(c, 'equal write to real physical source invalidates held unit', {input, want: input, status: 4, kind: 'physical-currentness'});
    } else h.skip(c, 'physical execution and source invalidation', 'based code captures physical 5000');
    h.host(c, 'close');
  }
  {
    const c = h.open(resident, entries, 'data and explicit CS operand bases', [[0], [0x4000, 3], [0x5000], [0x6000, 3]]);
    h.upload(c, 0x5000, Buffer.from('8b032e8b13ebfe', 'hex'));
    h.upload(c, 0x20, words([0xa1a2a3a4])); h.upload(c, 0x4020, words([0x55667788])); h.upload(c, 0x6020, words([0x11223344]));
    const context = {bases: [0, 0x4000, 0, 0x6000, 0, 0]}, regs = [...registers]; regs[3] = 0x20;
    const input = state(h.initial, {...context, regs}); h.put(c, 0, input);
    const material = h.compile(c, 'based operand body compilation', [[PC, 7]], ['cpu_read32']);
    if (material) {
      const first = success(input, PC + 2, {reg: 0, value: 0x11223344, version: 6});
      first.set(record('R3MH', 40, [0, 0x11223344, 0, 0, 0, 4], 10), 100);
      h.observe(c, 'default DS load uses DS linear address', {input, want: first, helpers: [call('cpu_read32', [0x6020])]});
      const second = success(first, PC + 5, {reg: 2, value: 0x55667788, version: 6});
      second.set(record('R3MH', 40, [0, 0x55667788, 0, 0, 0, 4], 10), 100);
      h.observe(c, 'CS override uses loaded CS base as data authority', {input: first, want: second, helpers: [call('cpu_read32', [0x4020])]});
      const denied = state(h.initial, {...context, regs, dsLimit: 0x1f}), fault = exit(Buffer.from(denied), 11, 0);
      fault.writeUInt32LE(2, 52);
      h.observe(c, 'logical DS limit faults before translated RAM read', {input: denied, want: fault});
    } else h.skip(c, 'DS CS operand execution and logical limit', 'based operand body compilation');
    h.host(c, 'close');
  }
  {
    const c = h.open(resident, entries, 'wrong decode key first-byte priority and repair', [[PC], [0x5000]]);
    h.upload(c, PC, body(0xaaaaaaaa)); h.upload(c, 0x5000, Buffer.from([0x90, 0xeb, 0xfe]));
    h.put(c, 0, state(h.initial)); const old = h.compile(c, 'retained flat five-byte instruction', [[PC, 7]]);
    if (old) {
      const input = state(h.initial, {...based, csLimit: PC});
      const miss = exit(Buffer.from(input), 3, 0);
      const result = h.observe(c, 'wrong key checks current one byte then NeedCode without old span', {input, want: miss});
      const denied = state(h.initial, {...based, csLimit: PC - 1});
      h.observe(c, 'wrong key denied first byte gives GP before RF begin', {input: denied, want: exit(Buffer.from(denied), 11, 0)});
      h.observe(c, 'wrong key budget precedes denied first byte', {input: denied, want: exit(Buffer.from(denied), 1, 0), budget: 0, kind: 'control'});
      const cancelled = state(h.initial, {...based, csLimit: PC - 1, cancel: 1});
      h.observe(c, 'wrong key cancel precedes denied first byte', {input: cancelled, want: exit(Buffer.from(cancelled), 2, 0), kind: 'control'});
      if (result.passed) {
        h.put(c, 0, miss); const repaired = stopped(c, 'same logical PC admits material under new key');
        if (repaired) {
          const repairedInput = state(h.initial, {...based, csLimit: PC});
          h.observe(c, 'new-key one-byte NOP executes without retained five-byte span', {input: repairedInput, want: success(repairedInput, PC + 1)});
          h.observe(c, resident ? 'held old resident remains a wrong-key miss' : 'old replacement generation is neutral',
            {input: repairedInput, want: resident ? exit(Buffer.from(repairedInput), 3, 0) : repairedInput, material: old, status: resident ? 0 : 3});
        } else h.skip(c, 'new-key repaired material execution', 'same logical PC admits material under new key');
      } else h.skip(c, 'new-key stopped repair', 'wrong-key NeedCode');
    } else h.skip(c, 'wrong key live checks', 'retained flat five-byte instruction');
    h.host(c, 'close');
  }
  if (resident) {
    const c = h.open(resident, entries, 'resident same-PC key collision and stale partition', [[PC], [0x5000], [0x9000]]);
    h.upload(c, PC, body(0xaaaaaaaa)); h.upload(c, 0x5000, body(0xbbbbbbbb)); h.upload(c, 0x9000, body(0xcccccccc));
    h.put(c, 0, state(h.initial)); const flat = h.compile(c, 'flat resident same-PC unit', [[PC, 7]]);
    h.put(c, 0, state(h.initial, based)); const other = h.compile(c, 'based resident same-PC distinct unit', [[PC, 7]]);
    if (flat && other) {
      assert.notDeepEqual(other.binding.slice(2), flat.binding.slice(2), 'keys cannot alias a unit ID');
      h.lookup(c, 'based key finds its unit at colliding logical PC', state(h.initial, based), PC, other);
      h.lookup(c, 'flat key finds its original unit at colliding logical PC', state(h.initial), PC, flat);
      const third = state(h.initial, {bases: [0, 0x8000, 0, 0, 0, 0]});
      h.lookup(c, 'unknown key misses logical-PC collision', third, PC, null);
      h.upload(c, PC, Buffer.from([0xb8]));
      h.lookup(c, 'stale old physical unit does not poison based lookup', state(h.initial, based), PC, other);
      h.lookup(c, 'stale other key does not poison unknown-key miss', third, PC, null);
      h.lookup(c, 'matching flat key still reports its real stale source', state(h.initial), PC, null, 4);
      const input = state(h.initial, based);
      h.observe(c, 'current based resident executes despite stale other-key unit', {input, want: success(input, PC + 5, {reg: 0, value: 0xbbbbbbbb}), material: other});
    } else h.skip(c, 'keyed resident routing', 'both resident key compilations');
    h.host(c, 'close');
  }
  for (const kind of ['jump', 'call', 'return', 'iret']) {
    const c = h.open(resident, entries, `far ${kind} changes interpretation mid-invocation`, [[PC], [0x2000], [0x6000], [0x7000, 3]]);
    const code = kind === 'jump' ? Buffer.from('ea002000004300', 'hex') : kind === 'call' ? Buffer.from('9a002000004300', 'hex') : Buffer.from([kind === 'return' ? 0xcb : 0xcf]);
    h.upload(c, PC, code); h.upload(c, 0x2000, body(0xaaaaaaaa)); h.upload(c, 0x6000, body(0xbbbbbbbb));
    h.upload(c, 0x7000, Buffer.alloc(4096, 0x6b));
    if (kind === 'return' || kind === 'iret') h.upload(c, 0x7800, words([0x2000, 0xa5a50043, ...(kind === 'iret' ? [0x10002] : [])]));
    const raw = descriptor(0x4000), beforeTable = h.arena(c);
    h.host(c, 'configure_segment_descriptor_raw', [0x43, raw.low, raw.high]); assert.deepEqual(h.arena(c), beforeTable, 'table configuration leaves loaded cache and arena unchanged');
    const helper = farName(c, kind === 'jump' ? 'far_jump' : kind === 'call' ? 'far_call' : 'far_return');
    const helperImports = [...(kind === 'return' || kind === 'iret' ? ['cpu_read'] : []), helper];
    const input = state(h.initial); h.put(c, 0, input);
    const source = h.compile(c, `far ${kind} source and retained flat target in same module`, [[PC, code.length], [0x2000, 7]], helperImports);
    if (source) {
      const want = Buffer.from(input); setSegments(want, {...based, selector: 0x43});
      want.writeUInt32LE(0x2000, 48); want.writeUInt32LE(kind === 'iret' ? 0x10002 : 2, 52);
      want.set(record('R3MH', 40, [0, 0x2000, 0, 0, 0, 0], 7), 100); exit(want, 3, 1, 6);
      const expectedHelpers = [], wantRAM = h.ram(c);
      if (kind === 'call') {
        want.writeUInt32LE(0x77f8, 32); writeRAM(wantRAM, 0x77f8, words([PC + code.length, 0x1b]));
      } else if (kind === 'return' || kind === 'iret') {
        for (const offset of kind === 'iret' ? [0, 4, 8] : [0, 4]) expectedHelpers.push(call('cpu_read', [0x7800 + offset, 4, 1]));
        want.writeUInt32LE(kind === 'iret' ? 0x780c : 0x7808, 32);
        want.set(words([kind === 'iret' ? 0x10002 : 0xa5a50043]), 140);
      }
      expectedHelpers.push(call(helper, bound(c, source, [0x2000, 0x43, 0, ...(kind === 'call' ? [PC + code.length, 0x7800] : [])])));
      const result = h.observe(c, `far ${kind} retires once then NeedCode before old-key target body`, {input, want, wantRAM, budget: 2, helpers: expectedHelpers});
      if (result.passed) {
        h.put(c, 0, want); const repaired = stopped(c, `far ${kind} destination compiles at physical 6000`);
        if (repaired) {
          const resumed = Buffer.from(want); resumed.set(record('R3EX', 40), 56);
          h.observe(c, `far ${kind} destination uses new physical body and logical PC`, {input: resumed, want: success(resumed, 0x2005, {reg: 0, value: 0xbbbbbbbb})});
        } else h.skip(c, `far ${kind} destination execution`, 'new-key destination compile');
      } else h.skip(c, `far ${kind} destination stopped repair`, 'far source transfer and keyed NeedCode');
    } else h.skip(c, `far ${kind} execution`, 'source compilation');
    h.host(c, 'close');
  }
  {
    const c = h.open(resident, entries, 'wrapped base addition with contiguous physical span', [[0x1000], [0x2000]]);
    h.upload(c, 0x1010, body(0xbbbbbbbb)); h.upload(c, 0x2010, body(0xaaaaaaaa));
    const input = state(h.initial, {pc: 0x2010, bases: [0, 0xfffff000, 0, 0, 0, 0]}); h.put(c, 0, input);
    const material = h.compile(c, 'safe base-add wrap captures linear 1010', [[0x2010, 7]]);
    if (material) h.observe(c, 'safe wrapped addition retains logical EIP', {input, want: success(input, 0x2015, {reg: 0, value: 0xbbbbbbbb})});
    else h.skip(c, 'safe wrapped-base execution', 'safe base-add wrap captures linear 1010');
    h.host(c, 'close');
  }
  {
    const c = h.open(resident, entries, 'split linear instruction follows current code-only wrap policy', [[0], [0xfffff000]]);
    h.upload(c, 0xffffffff, Buffer.from([0x66])); h.upload(c, 0, Buffer.from([0x90, 0xeb, 0xfe]));
    const input = state(h.initial, {pc: 0x1fff, bases: [0, 0xffffe000, 0, 0, 0, 0]}); h.put(c, 0, input);
    const material = h.compile(c, 'linear ffffffff-to-zero prefixed NOP captures both source pages', [[0x1fff, 4]]);
    if (material) h.observe(c, 'wrapped prefixed NOP retains logical EIP and retires once', {input, want: success(input, 0x2001)});
    else h.skip(c, 'wrapped prefixed NOP execution', 'split linear source capture');
    h.host(c, 'close');
  }
  for (const kind of ['read', 'store']) {
    const c = h.open(resident, entries, `late ${kind} helper sees changed live key`, [[PC], [0x5000], [0x6000, 3], [0x7000, 3]]);
    const address = kind === 'read' ? 0x6000 : 0x7000, helper = kind === 'read' ? 'cpu_read32' : resident ? 'cpu_store_resident32' : 'cpu_store32';
    const code = Buffer.concat([Buffer.from([kind === 'read' ? 0xa1 : 0xa3]), words([address]), Buffer.from([0xeb, 0xfe])]);
    h.upload(c, PC, code); h.upload(c, 0x6000, words([0x0badcafe])); h.upload(c, 0x7000, Buffer.alloc(4096, 0x6b));
    const input = state(h.initial); h.put(c, 0, input); const source = h.compile(c, `late ${kind} flat body`, [[PC, 7]], [helper]);
    if (source) {
      const context = {...based, csLimit: PC}, want = exit(setSegments(Buffer.from(input), context), 3, 0, 6); want.writeUInt32LE(2, 52);
      if (kind === 'read') want.set(record('R3MH', 40, [0, 0x0badcafe, 0, 0, 0, 4], 10), 100);
      c.hook = {name: helper, run: () => {const bytes = h.arena(c); setSegments(bytes, context); h.put(c, SEG, bytes.subarray(SEG, SEG + 64)); h.put(c, BASES, bytes.subarray(BASES, BASES + 40));}};
      h.observe(c, kind === 'read' ? 'stage-only real read publishes packet but changed key prevents register commit' : 'stateful store rejects changed key before RAM publication',
        {input, want, helpers: [call(helper, kind === 'read' ? [address] : bound(c, source, [address, registers[0]]), kind === 'read' ? 0 : 3)]});
    } else h.skip(c, `late ${kind} execution`, 'flat helper body compilation');
    h.host(c, 'close');
  }
}
h.finish([
  'Finite CS base with CS.D32 and independent SS.B32; no D16 admission, browser, game or performance claim.',
  'All generated modules use real engine guard and helper exports; injected late hooks mutate only valid CS cache/base records before the real helper.',
  'Old-engine unsupported compile/preflight is classified as admission failure; dependent repair is explicitly unreached, never fabricated semantic execution.',
  'Execution compares literal full 4796-byte arena, complete every-mapped-page RAM, exact helper arguments/status, and logical PCs.',
  'A translated instruction span crossing linear 4 GiB is an explicit neutral CompileError boundary, not a guest GP or PF claim.',
  'Module publication allocator metadata is validated separately; generated local allocation is owned by Wasm validation.',
]);
