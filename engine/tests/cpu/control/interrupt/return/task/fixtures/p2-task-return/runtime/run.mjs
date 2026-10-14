import assert from 'node:assert/strict';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {pathToFileURL} from 'node:url';
const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root);
const support = 'engine/tests/cpu/segments/code/base/fixtures/p2-cs-base/runtime/support.mjs';
const {harness, PC, SEG, BASES, registers, record, words, exit, hash} =
  await import(pathToFileURL(join(root, support)));
const h = await harness(enginePath, output, root);
const paths = [
  'engine/src/cpu/x86/tasks/outcome.rs', 'engine/src/cpu/x86/tasks/authority.rs',
  'engine/src/cpu/x86/tasks/register.rs', 'engine/src/cpu/x86/tasks/returning.rs',
  'engine/src/cpu/x86/tasks/record.rs', 'engine/src/cpu/x86/tasks/mod.rs',
  'engine/src/abi/tasks/mod.rs', 'engine/src/abi/tasks/sources.rs', 'engine/src/abi/tasks/helper.rs',
  'engine/src/abi/tasks/sources/tests.rs', 'engine/src/abi/tasks/helper/tests.rs',
  'engine/src/abi/mod.rs', 'engine/src/abi/x86/exit.rs', 'engine/src/cpu/exit.rs',
  'engine/src/process/cpu/tasks/mod.rs', 'engine/src/process/cpu/tasks/config.rs',
  'engine/src/process/cpu/tasks/returning.rs', 'engine/src/process/cpu/tasks/request.rs',
  'engine/src/process/cpu/tasks/config/tests.rs', 'engine/src/process/cpu/tasks/returning/tests.rs',
  'engine/src/process/cpu/mod.rs', 'engine/src/process/mod.rs', 'engine/src/process/instance.rs',
  'engine/src/process/wasm/tasks.rs', 'engine/src/process/wasm/mod.rs',
  'engine/src/abi/wasm/exports/tasks.rs', 'engine/src/abi/wasm/exports/mod.rs',
  'engine/src/cpu/dbt/wasm/far/task.rs', 'engine/src/cpu/dbt/wasm/far.rs',
  'engine/src/cpu/dbt/wasm/memory.rs', 'engine/src/cpu/dbt/wasm/emitter.rs',
  'engine/src/cpu/dbt/wasm/abi.rs', 'engine/src/cpu/dbt/artifact.rs',
  'engine/src/cpu/dbt/region.rs', 'engine/src/cpu/dbt/cold.rs',
  'engine/src/cpu/x86/decode/authority.rs',
  'engine/src/cpu/x86/decode/interrupts.rs', 'engine/src/cpu/x86/decode/far.rs',
  'engine/src/cpu/x86/decode/xstate.rs', 'engine/src/cpu/dbt/wasm/xstate.rs',
  'engine/src/cpu/dbt/wasm/interrupts/mod.rs', 'engine/src/cpu/dbt/wasm/interrupts/dispatch.rs',
  'engine/src/cpu/dbt/wasm/interrupts/packet.rs', 'engine/src/cpu/dbt/wasm/far/call.rs',
  'engine/src/abi/x86/interrupts.rs', 'engine/src/process/cpu/interrupts/config.rs',
  'engine/src/process/calls/code.rs', 'engine/src/process/calls/installation.rs',
  'engine/tests/cpu/control/interrupt/return/task/native.rs',
  'engine/tests/cpu/control/interrupt/return/task/wasm.rs',
  'engine/tests/cpu/control/interrupt/return/task/fixtures/p2-task-return/runtime/run.mjs', support,
];
const pins = () => Object.fromEntries(paths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = pins(), FLAGS = 0x254fd7, ATTEMPTED = 0x244fd7;
const observed = {contexts: 0, compiles: 0, invocations: 0, configurationRefusals: 0};
const configuration = [];
const taskName = c => `task_return${c.resident ? '_resident' : ''}`;
const returnName = c => `far_return${c.resident ? '_resident' : ''}`;
const call = (name, args, status = 0) => ({name, args: args.map(value => value >>> 0), status});
const bound = (c, material, args = []) => [...(c.resident ? material.binding : []), ...args];
const target = [0x90000067, 0x8b00];
function raw(base, limit, access) {
  return [(((base & 0xffff) << 16) | (limit & 0xffff)) >>> 0,
    ((base & 0xff000000) | (base >>> 16 & 255) | access << 8 | (limit & 0xf0000)) >>> 0];
}
function sourceRequest(row) {
  return record('R3TR', 32, [0x43, row.gdt ?? 0x4f,
    ...raw(row.base ?? 0x5000, row.limit ?? 0x67, 0x8b)]);
}
function configCall(c, label, bytes, status) {
  h.put(c, 140, bytes); const before = h.arena(c), ram = h.ram(c);
  const actual = c.api.configure_task_return_sources();
  assert.equal(actual, status, label); assert.deepEqual(h.arena(c), before, label + ': complete arena neutral');
  assert.deepEqual(h.ram(c), ram, label + ': all RAM neutral');
  configuration.push({context: c.key, label, status: actual, before_sha256: hash(before), after_sha256: hash(h.arena(c))});
  if (status !== 0) observed.configurationRefusals++;
}
function configure(c, row, focused) {
  if (row.configured !== false) {
    const request = sourceRequest(row);
    if (focused) {
      const malformed = Buffer.from(request); malformed.writeUInt32LE(1, 12);
      configCall(c, 'reserved R3TR header is rejected before configuration', malformed, 7);
    }
    configCall(c, 'explicit cached TR and GDT extent configure before material', request, 0);
    if (focused) configCall(c, 'second task source configuration is neutral refusal', request, 7);
  }
  // live GDT[current TR] is explicitly invalid and differs from the supplied hidden cache.
  h.host(c, 'configure_segment_descriptor_raw', [0x40, 0, 0]);
  if (row.target !== null) h.host(c, 'configure_segment_descriptor_raw', [0x48, ...(row.target ?? target)]);
}
function seed(row, nt = true) {
  const input = Buffer.from(h.initial), regs = [...registers]; regs[4] = nt ? 0xdead000e : 0x7800;
  input.set(record('R3ST', 56, [...regs, PC, nt ? FLAGS : FLAGS & ~(0x4000 | 0x100)], 3));
  input.writeUInt32LE(1, 12);
  const sg = record('R3SG', 64, [], 3);
  [0x23, 0x1b, 0x23, 0x23, 0x3b, 0].forEach((value, index) => sg.writeUInt16LE(value, 16 + index * 2));
  [0xf33fffff, row.d16 ? 0xf1bfffff : 0xf3bfffff, nt ? 0xf2300000 : 0xf33fffff,
    0xf33fffff, 0xf33fffff, 0]
    .forEach((value, index) => sg.writeUInt32LE(value >>> 0, [28, 32, 36, 40, 44, 56][index]));
  input.set(sg, SEG); input.set(record('R3SB', 40), BASES);
  for (let i = 0; i < 80; i++) input[4276 + i] = (i * 29 + 17) & 255;
  for (let i = 0; i < 8; i++) {
    input.fill(0x30 + i, 4396 + i * 16, 4412 + i * 16);
    input.fill(0x70 + i, 4628 + i * 16, 4644 + i * 16);
  }
  return input;
}
function result(input, row, fields = row.fields) {
  const want = Buffer.from(input);
  if (fields) want.set(record('R3MH', 40, fields, 13), 100);
  const status = fields?.[0] ?? 8;
  if (status === 8) {
    exit(want, 4, 0, 6); want.writeUInt32LE(4, 80);
  } else {
    want.writeUInt32LE(ATTEMPTED, 52); want.writeUInt32LE(0, 12);
    exit(want, status === 3 ? 21 : status === 6 ? 14 : 5, 0, status === 3 ? 10 : 6);
    want.writeUInt32LE(fields[2], 80);
    if (status === 1) {want.writeUInt32LE(fields[3], 84); want.writeUInt32LE(1, 88); want.writeUInt32LE(2, 92);}
  }
  return want;
}
function compile(c, label, bytes, configured) {
  observed.compiles++;
  return h.compile(c, label, [[PC, bytes.length]], ['cpu_read', returnName(c), ...(configured ? [taskName(c)] : [])]);
}
function observe(c, label, material, input, want, options = {}) {
  if (!material) {h.skip(c, label, 'source material admission'); return null;}
  observed.invocations++; return h.observe(c, label, {material, input, want, ...options});
}
function wrongOwner(c, material) {
  const module = new WebAssembly.Module(new Uint8Array(c.memory.buffer, material.pointer, material.length));
  const ring3 = Object.fromEntries(WebAssembly.Module.imports(module).filter(item => item.kind === 'function').map(item =>
    [item.name, (...args) => {
      if (item.name.startsWith('guard')) args[0] ^= 1;
      const status = c.api[item.name](...args); c.calls.push(call(item.name, args, status)); return status;
    }]));
  return {...material, run: new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3}).exports.run};
}
function changedContext(c, material) {
  const module = new WebAssembly.Module(new Uint8Array(c.memory.buffer, material.pointer, material.length));
  const ring3 = Object.fromEntries(WebAssembly.Module.imports(module).filter(item => item.kind === 'function').map(item =>
    [item.name, (...args) => {
      const status = c.api[item.name](...args); c.calls.push(call(item.name, args, status));
      if (item.name === taskName(c)) {
        // declared external mutation follows the real stage-only helper; the reply is also poisoned.
        const bytes = h.arena(c); bytes.writeUInt32LE(0x4000, BASES + 20);
        bytes.writeUInt32LE(1, 112); h.put(c, 0, bytes);
      }
      return status;
    }]));
  return {...material, run: new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3}).exports.run};
}
function invalidNpReply(c, material) {
  const module = new WebAssembly.Module(new Uint8Array(c.memory.buffer, material.pointer, material.length));
  const ring3 = Object.fromEntries(WebAssembly.Module.imports(module).filter(item => item.kind === 'function').map(item =>
    [item.name, (...args) => {
      const status = c.api[item.name](...args); c.calls.push(call(item.name, args, status));
      if (item.name === taskName(c)) {
        const bytes = h.arena(c);
        assert.deepEqual(bytes.subarray(100, 140), record('R3MH', 40, [6, 0, 0x48, 0, 0, 0], 13),
          'real helper supplies nonnull GDT NP before the sole reply mutation');
        bytes.writeUInt32LE(0x4c, 124); h.put(c, 0, bytes);
      }
      return status;
    }]));
  return {...material, run: new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3}).exports.run};
}
const rows = [
  {label: 'unconfigured NT1 remains old neutral authority refusal', configured: false, fields: null, both: true},
  {label: 'cached current limit precedes absent backlink', base: 0xdead0000, limit: 0, noBacklink: true,
    fields: [3, 0, 0x40, 0, 0, 0]},
  {label: 'explicit zero backlink is TS0', backlink: 0, fields: [3, 0, 0, 0, 0, 0], both: true},
  {label: 'unknown target remains missing authority', target: null, fields: [8, 0, 1, 0, 0, 0]},
  {label: 'supplied zero target descriptor is TS48', target: [0, 0], fields: [3, 0, 0x48, 0, 0, 0]},
  {label: 'P6 available absent short target is NP before busy and limit', target: [0x90000000, 0x0900],
    fields: [6, 0, 0x48, 0, 0, 0], both: true},
  {label: 'P6 available present target is TS busy failure', target: [0x90000067, 0x8900],
    fields: [3, 0, 0x48, 0, 0, 0]},
  {label: 'busy target short limit is TS48', target: [0x90000066, 0x8b00], fields: [3, 0, 0x48, 0, 0, 0]},
  {label: 'admitted Dword target retains backlink RPL and needs transition authority',
    fields: [8, 0, 2, 0x4b, 0, 0], both: true, liveBacklink: true},
  {label: 'admitted Word target ignores CF versus66CF under D16', d16: true, target: [0x9000002c, 0xe300],
    fields: [8, 0, 2, 0x4b, 0, 0], both: true},
  {label: 'backlink TI is preserved in TS4c', backlink: 0x4f, fields: [3, 0, 0x4c, 0, 0, 0]},
  {label: 'cached current TR survives live GDT extent below current index', gdt: 0x3f, backlink: 0,
    fields: [3, 0, 0, 0, 0, 0]},
  {label: 'two-byte backlink wraps high page to mapped zero page', base: 0xffffffff, wrap: true, low: true,
    fields: [8, 0, 2, 0x4b, 0, 0]},
  {label: 'wrapped backlink low-byte diagnostic faults at zero', base: 0xffffffff, wrap: true,
    fields: [1, 0, 1, 0, 1, 2], both: true},
  {label: 'absent backlink bytes are diagnostic memory failure not zero selector', noBacklink: true,
    fields: [1, 0, 1, 0x5000, 1, 2]},
];

for (const resident of [false, true]) for (const entries of [false, true]) for (const row of rows) {
  for (const code of row.both ? ['cf', '66cf'] : ['cf']) {
    observed.contexts++;
    const dataPages = row.wrap ? [0xfffff000, ...(row.low ? [0] : [])] : row.noBacklink ? [] : [0x5000];
    const c = h.open(resident, entries, row.label + '/' + code, [[PC], ...dataPages.map(page => [page])]);
    for (const page of dataPages) h.upload(c, page, Buffer.alloc(4096, 0x6b));
    const focused = !resident && !entries && row === rows[1];
    configure(c, row, focused);
    const backlink = row.backlink ?? 0x4b, base = row.base ?? 0x5000;
    if (!row.noBacklink) {
      h.upload(c, base, Buffer.from([backlink & 255]));
      if (!row.wrap || row.low) h.upload(c, (base + 1) >>> 0, Buffer.from([backlink >>> 8]));
    }
    const bytes = Buffer.from(code, 'hex'); h.upload(c, PC, bytes);
    const input = seed(row); h.put(c, 0, input);
    const material = compile(c, row.label + ': exact IRET capture', bytes, row.configured !== false);
    if (!resident && !entries && row === rows[0] && code === 'cf')
      configCall(c, 'material freezes previously absent task source configuration', sourceRequest(row), 7);
    if (!entries && row === rows[0] && code === 'cf' && material)
      observe(c, 'wrong owner precedes NT handling and RAM', wrongOwner(c, material), input, input, {status: 3});
    const trace = row.configured === false || !material ? [] : [call(taskName(c), bound(c, material))];
    const first = observe(c, row.label + ': real NT1 ' + code, material, input, result(input, row), {helpers: trace});
    if (!resident && !entries && row === rows[4] && material) {
      const want = Buffer.from(input); want.writeUInt32LE(ATTEMPTED, 52); want.writeUInt32LE(0, 12);
      want.set(record('R3MH', 40, row.fields, 13), 100); want.writeUInt32LE(1, 112);
      want.writeUInt32LE(0x4000, BASES + 20); exit(want, 3, 0, 6);
      observe(c, 'valid CS key change after real helper precedes poisoned TS packet validation',
        changedContext(c, material), input, want, {helpers: trace});
    }
    if (!resident && !entries && row === rows[5] && code === 'cf' && material) {
      const want = result(input, row, [6, 0, 0x4c, 0, 0, 0]);
      exit(want, 7, 0, 6); want.writeUInt32LE(2, 80);
      observe(c, 'real NP reply with TI set is protocol infrastructure before guest fault publication',
        invalidNpReply(c, material), input, want, {helpers: trace});
    }
    if (row.liveBacklink && first?.passed) {
      h.upload(c, 0x5000, Buffer.from([0, 0]));
      observe(c, 'same material rereads changed live backlink as TS0', material, input,
        result(input, row, [3, 0, 0, 0, 0, 0]), {helpers: trace});
    } else if (row.liveBacklink) h.skip(c, 'changed live backlink', 'first admitted-transition refusal');
    h.host(c, 'close');
  }
}
{
  // one module exercises task/INT/far-call type offsets and the XState flush capability together.
  observed.contexts++;
  const c = h.open(false, false, 'NT1 TS0 with unreachable INT far CALL and XGETBV', [[PC], [0x5000]]);
  configure(c, {backlink: 0}, false);
  const idt = record('R3ID', 2068); idt.writeUInt16LE(0x7ff, 16);
  idt.set(words([0x001b2000, 0xee00]), 20 + 8 * 0x80);
  h.put(c, 140, idt); h.host(c, 'configure_interrupt_descriptor_table');
  h.upload(c, 0x5000, Buffer.from([0, 0]));
  const blocks = [[PC, 'cf'], [0x1100, 'cd80'], [0x1200, '9a002000001b00'], [0x1300, '0f01d0ebfe']];
  for (const [pc, hex] of blocks) h.upload(c, pc, Buffer.from(hex, 'hex'));
  const input = seed({}); h.put(c, 0, input); observed.compiles++;
  const material = h.compile(c, 'mixed helper types and XState capability with live IRET',
    blocks.map(([pc, hex]) => [pc, hex.length / 2]),
    ['cpu_read', 'far_return', 'far_call', 'interrupt_gate', 'task_return']);
  observe(c, 'live NT1 TS0 remains exit10 before unreachable helper bodies', material, input,
    result(input, {}, [3, 0, 0, 0, 0, 0]), {helpers: [call('task_return', [])]});
  h.host(c, 'close');
}
for (const resident of [false, true]) for (const entries of [false, true]) for (const code of ['cf', '66cf']) {
  observed.contexts++; const c = h.open(resident, entries, 'NT0 unchanged/' + code, [[PC], [0x7000]]);
  configure(c, {base: 0xdead0000, target: null}, false);
  const slot = code === 'cf' ? 4 : 2, frame = Buffer.alloc(3 * slot);
  [0x1100, 0x1b, 0x240ed7].forEach((value, index) => slot === 4 ? frame.writeUInt32LE(value, index * slot)
    : frame.writeUInt16LE(value & 0xffff, index * slot));
  h.upload(c, 0x7800, frame); const bytes = Buffer.from(code, 'hex'); h.upload(c, PC, bytes);
  const input = seed({}, false); h.put(c, 0, input); const material = compile(c, 'NT0 ordinary IRET capture', bytes, true);
  const want = Buffer.from(input), sg = record('R3SG', 64);
  [0x23, 0x1b, 0x23, 0x23, 0x3b, 0].forEach((value, index) => sg.writeUInt16LE(value, 16 + index * 2));
  want.set(sg, SEG); want.writeUInt32LE(0, 12); want.writeUInt32LE(0x1100, 48);
  want.writeUInt32LE(0x7800 + 3 * slot, 32); want.writeUInt32LE(0x240ed7, 52);
  want.set(record('R3MH', 40, [0, 0x1100, 0, 0, 0, 0], 7), 100);
  want.set(slot === 4 ? Buffer.from('d70e2400', 'hex') : Buffer.from('d70e', 'hex'), 140);
  exit(want, 1, 1, 6);
  const trace = [0, 1, 2].map(index => call('cpu_read', [0x7800 + index * slot, slot, 1]));
  if (material) trace.push(call(returnName(c), bound(c, material, [0x1100, 0x1b, Number(slot === 2)])));
  observe(c, 'NT0 uses ordinary stack and no task helper', material, input, want, {helpers: trace}); h.host(c, 'close');
}
assert.deepEqual(pins(), sourcePins);
writeFileSync(join(output, 'task-source-pins.json'), JSON.stringify(sourcePins, null, 2) + '\n');
writeFileSync(join(output, 'task-configuration.json'), JSON.stringify(configuration, null, 2) + '\n');
h.finish({planned: {contexts: 93, compiles: 93, generatedModules: 93, invocations: 105, observations: 198,
  configurationRefusals: 3}, observed,
  scope: 'NT1 current/backlink/descriptor admission only. Admitted tasks remain neutral Unsupported pending complete transition authority; no incoming task success, PF metadata or CR2 claims.'});
