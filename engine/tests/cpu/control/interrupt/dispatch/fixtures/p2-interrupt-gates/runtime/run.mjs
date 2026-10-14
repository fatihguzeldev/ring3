import assert from 'node:assert/strict';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {pathToFileURL} from 'node:url';
const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root);
const support = 'engine/tests/cpu/segments/code/base/fixtures/p2-cs-base/runtime/support.mjs';
const {harness, PC, SEG, BASES, KEY_HIGH, registers, record, words, exit, writeRAM, hash} =
  await import(pathToFileURL(join(root, support)));
const h = await harness(enginePath, output, root);
const paths = [
  "engine/src/cpu/dbt/wasm/detection.rs",
  "engine/src/process/wasm/detection.rs",
  "engine/src/abi/wasm/exports/detection.rs",
  "engine/src/process/cpu/interrupts/detection.rs",
  "engine/src/process/cpu/interrupts/entry.rs",
  "engine/src/process/cpu/interrupts/entry/state.rs",
  "engine/src/cpu/x86/interrupts/cause.rs",
  "engine/src/cpu/x86/interrupts/escalation.rs",
  "engine/src/abi/interrupt.rs",
  "engine/src/abi/interrupt/tests.rs",
  "engine/src/abi/wasm/exports/cpu.rs",
  "engine/src/abi/x86/interrupts.rs",
  "engine/src/abi/x86/interrupts/tests.rs",
  "engine/src/abi/x86/mod.rs",
  "engine/src/cpu/dbt/artifact.rs",
  "engine/src/cpu/dbt/cold.rs",
  "engine/src/cpu/dbt/cold/tests.rs",
  "engine/src/cpu/dbt/cold/tests/capacity.rs",
  "engine/src/cpu/dbt/cold/tests/interrupt.rs",
  "engine/src/cpu/dbt/region.rs",
  "engine/src/cpu/dbt/region/avx/native.rs",
  "engine/src/cpu/dbt/resident.rs",
  "engine/src/cpu/dbt/resident/tests/entries.rs",
  "engine/src/cpu/dbt/wasm/abi.rs",
  "engine/src/cpu/dbt/wasm/code_fetch.rs",
  "engine/src/cpu/dbt/wasm/cpu_result.rs",
  "engine/src/cpu/dbt/wasm/emitter.rs",
  "engine/src/cpu/dbt/wasm/far/interrupt.rs",
  "engine/src/cpu/dbt/wasm/integer/dispatch.rs",
  "engine/src/cpu/dbt/wasm/interrupts/dispatch.rs",
  "engine/src/cpu/dbt/wasm/interrupts/mod.rs",
  "engine/src/cpu/dbt/wasm/interrupts/packet.rs",
  "engine/src/cpu/dbt/wasm/memory.rs",
  "engine/src/cpu/dbt/wasm/mod.rs",
  "engine/src/cpu/x86/capabilities.rs",
  "engine/src/cpu/x86/control.rs",
  "engine/src/cpu/x86/decode/authority.rs",
  "engine/src/cpu/x86/decode/control.rs",
  "engine/src/cpu/x86/decode/decoder.rs",
  "engine/src/cpu/x86/decode/interrupts.rs",
  "engine/src/cpu/x86/decode/interrupts/tests.rs",
  "engine/src/cpu/x86/decode/lower.rs",
  "engine/src/cpu/x86/decode/mod.rs",
  "engine/src/cpu/x86/decode/profile.rs",
  "engine/src/cpu/x86/decode/strings/defaults.rs",
  "engine/src/cpu/x86/decode/system_registers/tests.rs",
  "engine/src/cpu/x86/environment.rs",
  "engine/src/cpu/x86/interrupts/gates.rs",
  "engine/src/cpu/x86/interrupts/gates/tests.rs",
  "engine/src/cpu/x86/interrupts/mod.rs",
  "engine/src/cpu/x86/interrupts/table.rs",
  "engine/src/cpu/x86/interrupts/table/tests.rs",
  "engine/src/cpu/x86/ir.rs",
  "engine/src/cpu/x86/mod.rs",
  "engine/src/memory/space/cpu.rs",
  "engine/src/memory/space/cpu/write.rs",
  "engine/src/process/cpu/interrupts/config.rs",
  "engine/src/process/cpu/interrupts/config/tests.rs",
  "engine/src/process/cpu/interrupts/gate.rs",
  "engine/src/process/cpu/interrupts/gate/tests.rs",
  "engine/src/process/cpu/interrupts/mod.rs",
  "engine/src/process/cpu/mod.rs",
  "engine/src/process/execution/compiled.rs",
  "engine/src/process/execution/resident.rs",
  "engine/src/process/execution/stopped.rs",
  "engine/src/process/instance.rs",
  "engine/src/process/wasm/cpu.rs",
  "engine/src/process/wasm/mod.rs",
  "engine/tests/cpu/control/interrupt/configuration/native.rs",
  "engine/tests/cpu/control/interrupt/dispatch/fixtures/p2-interrupt-gates/runtime/run.mjs",
  "engine/tests/cpu/control/interrupt/dispatch/native.rs",
  "engine/tests/cpu/control/interrupt/dispatch/wasm.rs",
  "engine/tests/cpu/control/native.rs",
  "engine/tests/cpu/control/wasm.rs",
  "engine/tests/cpu/segments/code/base/fixtures/p2-cs-base/runtime/support.mjs"
];
const pins = () => Object.fromEntries(paths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = pins(), FLAGS = 0x254fd7, ATTEMPT_FLAGS = 0x244fd7;
const observed = {contexts: 0, compileCalls: 0, guestCalls: 0};
const finishName = 'finish_exception_detection', checkpoints = [];
const helper = c => `interrupt_gate${c.resident ? '_resident' : ''}`;
const returnedHelper = c => `far_return${c.resident ? '_resident' : ''}`;
const call = (name, args, status = 0) => ({name,
  args: args.map(value => typeof value === 'bigint' ? value.toString(10) : value >>> 0),
  status: typeof status === 'bigint' ? status.toString(10) : status});
const bound = (c, material, args) => [...(c.resident ? material.binding : []), ...args];
const packet = fields => record('R3MH', 40, fields, 11);
function rawGate(kind, selector = 0x49, target = 0x2300, {dpl = 3, present = true, reserved = 0} = {}) {
  return [((selector << 16) | (target & 0xffff)) >>> 0,
    (([14, 15].includes(kind) ? target & 0xffff0000 : 0) | kind << 8 | dpl << 13 | Number(present) << 15 | reserved) >>> 0];
}
function rawCode({base = 0, default32 = true, dpl = 3, present = true, conforming = false, data = false, limit} = {}) {
  const paged = limit === undefined; limit ??= 0xfffff;
  return [(((base & 0xffff) << 16) | (limit & 0xffff)) >>> 0,
    ((base & 0xff000000) | (base >>> 16 & 255) | (limit & 0xf0000) | Number(default32) << 22
      | Number(paged) << 23 | Number(present) << 15 | dpl << 13 | (data ? 0x1200 : conforming ? 0x1e00 : 0x1a00)) >>> 0];
}
function configure(c, vector, raw, {limit = 0x7ff} = {}) {
  const bytes = record('R3ID', 2068); bytes.writeUInt16LE(limit, 16);
  bytes.set(words(raw), 20 + 8 * vector); h.put(c, 140, bytes);
  h.host(c, 'configure_interrupt_descriptor_table');
}
function seed({pc = PC, sourceD = true, ssB = true, ssBase = 0, ssCache,
  esp = 0x7800, flags = FLAGS, shadow = 1, cancel = 0} = {}) {
  const input = Buffer.from(h.initial), regs = [...registers]; regs[4] = esp;
  input.set(record('R3ST', 56, [...regs, pc, flags], 3)); input.writeUInt32LE(shadow, 12);
  input.writeUInt32LE(cancel, 96);
  const sg = record('R3SG', 64, [], 3);
  [0, 0x1b, 0x23, 0, 0, 0].forEach((value, i) => sg.writeUInt16LE(value, 16 + 2 * i));
  [0, sourceD ? 0xf3bfffff : 0xf1bfffff, ssCache ?? (ssB ? 0xf33fffff : 0xf13fffff), 0, 0, 0]
    .forEach((value, i) => sg.writeUInt32LE(value >>> 0, [28, 32, 36, 40, 44, 56][i]));
  input.set(sg, SEG); input.set(record('R3SB', 40, [0, 0, ssBase, 0, 0, 0]), BASES);
  for (let i = 0; i < 80; i++) input[4276 + i] = (i * 29 + 17) & 255;
  for (let i = 0; i < 8; i++) {input.fill(0x30 + i, 4396 + i * 16, 4412 + i * 16);
    input.fill(0x70 + i, 4628 + i * 16, 4644 + i * 16);}
  return input;
}
function loaded(want, {target = 0x2300, base = 0, cache = 0xf3bfffff, esp, flags, selector = 0x4b} = {}) {
  want.writeUInt32LE(target, 48); want.writeUInt16LE(selector, SEG + 18);
  want.writeUInt32LE(cache >>> 0, SEG + 32); want.writeUInt32LE(base >>> 0, BASES + 20);
  if (esp !== undefined) want.writeUInt32LE(esp >>> 0, 32);
  if (flags !== undefined) want.writeUInt32LE(flags >>> 0, 52);
  want.writeUInt32LE(0, 12); return want;
}
function result(want, reason, retired = 0, {detail = 0, address = 0, access = 0, length = 0, version = 6} = {}) {
  exit(want, reason, retired, version); want.writeUInt32LE(detail >>> 0, 80);
  want.writeUInt32LE(address >>> 0, 84); want.writeUInt32LE(access, 88); want.writeUInt32LE(length, 92); return want;
}
function compile(c, label, spans, helpers = [], options = {}) {
  observed.compileCalls++;
  const expected = c.interruptsConfigured ? [...helpers, finishName] : helpers;
  return h.compile(c, label, spans, expected, options);
}
function run(c, label, material, input, want, options = {}) {
  if (!material) {h.skip(c, label, 'required earlier material admission'); return null;}
  observed.guestCalls++;
  const row = h.observe(c, label, {material, input, want, ...options});
  if (material.stopCheck?.errors.length) {row.errors.push(...material.stopCheck.errors); row.passed = false;}
  return row;
}
function open(resident, entries, label, pages) {
  observed.contexts++; return h.open(resident, entries, label, pages);
}
function fill(c, pages) {for (const page of pages) h.upload(c, page, Buffer.alloc(4096, 0x6b));}
function frameRAM(c, frame) {const ram = h.ram(c); for (const [address, hex] of frame) writeRAM(ram, address, Buffer.from(hex, 'hex')); return ram;}
function expectedINT(c, material, input, {vector = 0x80, pc = PC, status = 0} = {}) {
  return call(helper(c), bound(c, material, [vector, pc + 2, input.readUInt32LE(32), ATTEMPT_FLAGS]), status);
}
function altered(c, original, selected, mutate, returnedStatus, wrongKey = false) {
  // declared negative control: real guard/helper runs before any explicit reply corruption.
  const module = new WebAssembly.Module(new Uint8Array(c.memory.buffer, original.pointer, original.length));
  const ring3 = Object.fromEntries(WebAssembly.Module.imports(module).filter(imp => imp.kind === 'function').map(imp =>
    [imp.name, (...args) => {
      if (wrongKey && imp.name.startsWith('guard')) args[0] ^= 1;
      const status = c.api[imp.name](...args); c.calls.push(call(imp.name, args, status));
      if (imp.name === selected) {const bytes = h.arena(c); mutate(bytes); h.put(c, 0, bytes);}
      return imp.name === selected ? returnedStatus ?? status : status;
    }]));
  return {...original, run: new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3}).exports.run};
}
function checkedStop(c, original, expected, expectedRAM) {
  if (!original) return null;
  // preserve each original v11 fault/precise-stop oracle before automatic routing can replace its packet.
  const stopCheck = {errors: []};
  const module = new WebAssembly.Module(new Uint8Array(c.memory.buffer, original.pointer, original.length));
  const ring3 = Object.fromEntries(WebAssembly.Module.imports(module).filter(i => i.kind === 'function').map(i =>
    [i.name, (...args) => {
      if (i.name === finishName) {
        const actual = h.arena(c), actualRAM = h.ram(c), index = checkpoints.length + 1;
        const prefix = `pre-automatic-${index}`;
        if (!actual.equals(expected)) stopCheck.errors.push('original full-arena stop before automatic routing');
        const memory = [];
        for (const [page, want] of expectedRAM) {
          const got = actualRAM.get(page);
          if (!got.equals(want)) stopCheck.errors.push(`original pre-automatic RAM ${page.toString(16)}`);
          for (const [name, bytes] of [['actual', got], ['want', want]])
            writeFileSync(join(output, `${prefix}-ram-${page.toString(16)}-${name}.bin`), bytes, {flag: 'wx'});
          memory.push({page, actual_sha256: hash(got), want_sha256: hash(want)});
        }
        writeFileSync(join(output, `${prefix}-actual.bin`), actual, {flag: 'wx'});
        writeFileSync(join(output, `${prefix}-want.bin`), expected, {flag: 'wx'});
        checkpoints.push({context: c.key, label: c.label, prefix, errors: [...stopCheck.errors],
          actual_sha256: hash(actual), want_sha256: hash(expected), memory});
      }
      const status = c.api[i.name](...args); c.calls.push(call(i.name, args, status)); return status;
    }]));
  return {...original, stopCheck,
    run: new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3}).exports.run};
}
const terminal = (reason, {detail = 0, address = 0, access = 0, length = 0, debug = 0} = {}, epoch = 1n) =>
  call(finishName, [epoch, reason, detail, address, access, length, debug, 0, 0, 0, 0, 0]);
const successes = [
  {label: 'trap16 from D32/B32 to based D16', kind: 7, info: 2, sourceD: true, ssB: true,
    esp: 0x7801, newESP: 0x77fb, ssBase: 0x8000, cache: 0xf1bfffff, targetD: false,
    live: 0x240ed7, restored: 0x244fd7, handler: 'b955aacf', movLength: 3, ecx: 0x2345aa55,
    frame: [[0xf7ff, 'd74f'], [0xf7fd, '1b00'], [0xf7fb, '0210']],
    reads: [0xf7fb, 0xf7fd, 0xf7ff], flagsHex: 'd74f', pages: [0xf000]},
  {label: 'interrupt32 from D16/B16 to based D32', kind: 14, info: 0x104, sourceD: false, ssB: false,
    esp: 0xa5a50003, newESP: 0xa5a5fff7, ssBase: 0x8000, cache: 0xf3bfffff, targetD: true,
    live: 0x240cd7, restored: 0x244dd7, handler: 'b955aa1122cf', movLength: 5, ecx: 0x2211aa55,
    frame: [[0x17fff, 'd74f2400'], [0x17ffb, '1b000000'], [0x17ff7, '02100000']],
    reads: [0x17ff7, 0x17ffb, 0x17fff], flagsHex: 'd74f2400', pages: [0x17000, 0x18000]},
  {label: 'word trap frame independent of target D32', focus: true, kind: 7, info: 2, sourceD: true, ssB: true,
    esp: 0x7801, newESP: 0x77fb, ssBase: 0x8000, cache: 0xf3bfffff, targetD: true,
    live: 0x240ed7, restored: 0x244fd7, handler: 'b955aa112266cf', movLength: 5, ecx: 0x2211aa55,
    frame: [[0xf7ff, 'd74f'], [0xf7fd, '1b00'], [0xf7fb, '0210']],
    reads: [0xf7fb, 0xf7fd, 0xf7ff], flagsHex: 'd74f', pages: [0xf000]},
];
const faults = [
  {label: 'IDT limit precedes type', gate: [0, 0], limit: 0x406, fields: [3, 0, 0x402, 0, 0, 0], reason: 11, detail: 0x402},
  {label: 'IDT type precedes DPL and presence', gate: rawGate(2, 0, 0, {dpl: 0, present: false}), fields: [3, 0, 0x402, 0, 0, 0], reason: 11, detail: 0x402},
  {label: 'software gate DPL precedes presence', gate: rawGate(7, 0, 0, {dpl: 0, present: false}), fields: [3, 0, 0x402, 0, 0, 0], reason: 11, detail: 0x402},
  {label: 'vector-zero NP precedes embedded null', vector: 0, gate: rawGate(7, 0, 0, {present: false}), fields: [6, 0, 2, 0, 0, 0], reason: 14, detail: 2},
  {label: 'embedded null target', gate: rawGate(7, 0), fields: [3, 0, 0, 0, 0, 2], reason: 11},
  {label: 'target type precedes presence', gate: rawGate(7), descriptor: rawCode({data: true, present: false}), fields: [3, 0, 0x48, 0, 0, 2], reason: 11, detail: 0x48},
  {label: 'target NP precedes privilege-transition refusal', gate: rawGate(7), descriptor: rawCode({dpl: 0, present: false}), fields: [6, 0, 0x48, 0, 0, 2], reason: 14, detail: 0x48},
  {label: 'all cached SS spans precede target limit', gate: rawGate(7, 0x49, 0x2300), descriptor: rawCode({limit: 0xfff}),
    ssCache: 0xf27077fa, fields: [7, 0, 0, 0, 0, 2], reason: 15},
  {label: 'target limit precedes physical stack pages', gate: rawGate(7), descriptor: rawCode({limit: 0xfff}), esp: 0x7000,
    fields: [3, 0, 0, 0, 0, 2], reason: 11},
  {label: 'third-slot page fault leaves earlier two slots unchanged', gate: rawGate(14), esp: 0x7008,
    fields: [1, 0, 1, 0x6ffc, 2, 0x104], reason: 5, detail: 1, address: 0x6ffc, access: 2, length: 4},
  {label: 'outer-admissible task gate has unsupplied TSS authority', gate: rawGate(5, 0x50, 0), fields: [8, 0, 4, 0, 0, 0], reason: 4, detail: 4, unsupported: true},
  {label: 'present nonconforming lower-DPL target needs new privilege stack', gate: rawGate(15), descriptor: rawCode({dpl: 0}),
    fields: [8, 0, 4, 0, 0, 0], reason: 4, detail: 4, unsupported: true},
  {label: 'CD03 uses IDT and is distinct from CC diagnostic breakpoint', vector: 3, gate: [0, 0], fields: [3, 0, 0x1a, 0, 0, 0], reason: 11, detail: 0x1a},
];
for (const resident of [false, true]) for (const entries of [false, true]) {
  for (const row of successes) {
    if (row.focus && (resident || entries)) continue;
    const c = open(resident, entries, row.label, [[PC], [0x6000], ...row.pages.map(page => [page, 3])]);
    fill(c, row.pages); configure(c, 0x80, rawGate(row.kind));
    h.host(c, 'configure_segment_descriptor_raw', [0x18, ...rawCode({default32: row.sourceD})]);
    h.host(c, 'configure_segment_descriptor_raw', [0x48, ...rawCode({base: 0x4000, default32: row.targetD})]);
    h.upload(c, PC, Buffer.from('cd8090ebfe', 'hex')); h.upload(c, 0x6300, Buffer.from(row.handler, 'hex'));
    const input = seed(row); h.put(c, 0, input);
    const material = compile(c, row.label + ' source capture', [[PC, 2]], [helper(c)]);
    if (row === successes[0] && resident === entries) {
      for (const [label, cancel, budget, reason] of [['cancel precedes attempted INT', 1, 2, 2], ['zero budget precedes attempted INT', 0, 0, 1]]) {
        const stopped = seed({...row, cancel}), want = result(Buffer.from(stopped), reason);
        run(c, label, material, stopped, want, {budget});
      }
      if (material) {
        const poisoned = Buffer.from(input); poisoned.set(packet([0, 0x10000, 0, 0, 0, 2]), 100);
        poisoned.writeUInt32LE(1, 112);
        run(c, 'real wrong-key guard precedes poisoned packet, RF and RAM',
          altered(c, material, null, null, undefined, true), poisoned, poisoned,
          {status: 3, kind: 'declared-wrong-key-control'});
      }
    }
    const want = loaded(Buffer.from(input), {base: 0x4000, cache: row.cache, esp: row.newESP, flags: row.live});
    want.set(packet([0, 0x2300, 0, 0, 0, row.info]), 100); result(want, 3, 1);
    const retired = run(c, row.label + ': exact frame and live flags before cold-handler NeedCode', material, input, want,
      {budget: 2, wantRAM: frameRAM(c, row.frame), helpers: material ? [expectedINT(c, material, input)] : []});
    if (retired?.passed) {
      h.put(c, 0, want);
      const target = compile(c, row.label + ' stopped handler capture under new CS base/D', [], ['cpu_read', returnedHelper(c)], {stopped: true});
      const moved = Buffer.from(want); moved.writeUInt32LE(row.ecx, 20); moved.writeUInt32LE(0x2300 + row.movLength, 48); result(moved, 1, 1);
      const move = run(c, row.label + ': actual handler MOV uses loaded mode and physical base', target, want, moved);
      if (move?.passed) {
        const returned = loaded(Buffer.from(moved), {target: PC + 2, cache: row.sourceD ? 0xf3bfffff : 0xf1bfffff,
          selector: 0x1b, esp: row.esp, flags: row.restored});
        returned.set(record('R3MH', 40, [0, PC + 2, 0, 0, 0, 0], 7), 100);
        returned.set(Buffer.from(row.flagsHex, 'hex'), 140); result(returned, 1, 1);
        const slot = row.info & 255;
        const trace = row.reads.map(address => call('cpu_read', [address, slot, 1]));
        if (target) trace.push(call(returnedHelper(c), bound(c, target, [PC + 2, 0x1b, Number(slot === 2)])));
        const back = run(c, row.label + ': IRET restores gate-selected frame and CPL3 flag mask', target, moved, returned, {helpers: trace});
        if (back?.passed && !resident && !entries && row === successes[0]) {
          h.put(c, 0, returned); const resumed = compile(c, 'returned TF source continuation', [[PC + 2, 3]]);
          const stepped = Buffer.from(returned); stepped.writeUInt32LE(PC + 3, 48); result(stepped, 16, 1, {version: 7});
          c.diagnosticRAM = true;
          const final = Buffer.from(stepped); final.set(record('R3MH', 40, [3, 0, 0x43, 0, 0, 0], 12), 100);
          run(c, 'restored TF detects the next NOP; absent DB GP and DF handlers latch shutdown',
            checkedStop(c, resumed, stepped, h.ram(c)), returned, final,
            {helpers: [terminal(16, {debug: 2}, 6n)]});
        }
      } else h.skip(c, row.label + ' IRET', 'actual handler MOV');
    } else h.skip(c, row.label + ' handler/IRET', 'actual frame retirement and cold target');
    h.host(c, 'close');
  }
  for (const row of faults) {
    const c = open(resident, entries, row.label, [[PC], [0x7000, 3]]);
    if (!row.unsupported && row.reason !== 5) c.diagnosticRAM = true;
    fill(c, [0x7000]);
    configure(c, row.vector ?? 0x80, row.gate, row);
    h.host(c, 'configure_segment_descriptor_raw', [0x48, ...(row.descriptor ?? rawCode())]);
    h.upload(c, PC, Buffer.from([0xcd, row.vector ?? 0x80])); const input = seed(row); h.put(c, 0, input);
    const material = compile(c, row.label + ' capture', [[PC, 2]], [helper(c)]);
    const want = Buffer.from(input); if (!row.unsupported) {want.writeUInt32LE(ATTEMPT_FLAGS, 52); want.writeUInt32LE(0, 12);}
    want.set(packet(row.fields), 100); result(want, row.reason, 0, row);
    const originalStop = Buffer.from(want), calls = material ? [expectedINT(c, material, input, row)] : [];
    let checked = material;
    if (!row.unsupported) {
      calls.push(terminal(row.reason, row));
      if (row.reason !== 5) {
        checked = checkedStop(c, material, originalStop, h.ram(c));
        // the original GP/NP/SS is contributory; its absent handler and absent DF gate end in GP43 shutdown.
        want.set(record('R3MH', 40, [3, 0, 0x43, 0, 0, 0], 12), 100);
      }
    }
    run(c, row.label + (row.unsupported ? '' : row.reason === 5 ? ': raw paging boundary' : ': original stop then DF/shutdown'),
      checked, input, want, {helpers: calls});
    h.host(c, 'close');
  }
}
// Separate focused controls below reuse these same real engine/helper bindings.

for (const resident of [false, true]) for (const entries of [false, true]) {
  {
    const c = open(resident, entries, 'conforming lower-DPL same-CPL target', [[PC], [0x7000, 3]]);
    fill(c, [0x7000]); configure(c, 0x80, rawGate(15));
    h.host(c, 'configure_segment_descriptor_raw', [0x48, ...rawCode({dpl: 0, conforming: true})]);
    h.upload(c, PC, Buffer.from('cd80', 'hex')); const input = seed(); h.put(c, 0, input);
    const material = compile(c, 'conforming target source', [[PC, 2]], [helper(c)]);
    const want = loaded(Buffer.from(input), {cache: 0x93ffffff, esp: 0x77f4, flags: 0x240ed7});
    want.writeUInt16LE(2, SEG + 4); // zero bases and D32/B32 retain this nonlegacy conforming cache as R3SGv2.
    want.set(packet([0, 0x2300, 0, 0, 0, 4]), 100); result(want, 3, 1);
    run(c, 'conforming DPL0 ignores embedded RPL and keeps CPL3 without TSS', material, input, want,
      {budget: 2, wantRAM: frameRAM(c, [[0x77fc, 'd74f2400'], [0x77f8, '1b000000'], [0x77f4, '02100000']]),
        helpers: material ? [expectedINT(c, material, input)] : []});
    h.host(c, 'close');
  }
  {
    const c = open(resident, entries, 'final immediate fetch and repair', [[PC], [0x7000, 3]]);
    fill(c, [0x7000]); configure(c, 0x80, rawGate(14));
    h.host(c, 'configure_segment_descriptor_raw', [0x48, ...rawCode()]);
    const target = 0x1fff, branch = Buffer.alloc(5); branch[0] = 0xe9; branch.writeInt32LE(target - PC - 5, 1);
    h.upload(c, PC, branch); h.upload(c, target, Buffer.from([0xcd]));
    const start = seed({flags: 0x254ed7}); h.put(c, 0, start);
    const bootstrap = compile(c, 'real branch before incomplete INT', [[PC, 5]]);
    const missed = Buffer.from(start); missed.writeUInt32LE(target, 48); missed.writeUInt32LE(0x244ed7, 52);
    missed.writeUInt32LE(0, 12); result(missed, 3, 1, {version: 1});
    const stepped = run(c, 'branch retires before incomplete immediate source is requested', bootstrap, start, missed, {budget: 2});
    if (stepped?.passed) {
      compile(c, 'strict incomplete INT source refuses before any gate work', [[target, 2]], [], {expectedStatus: 10});
      compile(c, 'stopped incomplete INT source remains neutral', [], [], {expectedStatus: 10, stopped: true});
      h.host(c, 'map', [0x2000, 1, 7]); c.pages.push(0x2000); c.pages.sort((a, b) => a - b);
      h.upload(c, 0x2000, Buffer.from([0x80])); h.host(c, 'protect', [0x2000, 1, 1]);
      compile(c, 'READ-only immediate still cannot enter IDT work', [], [], {expectedStatus: 10, stopped: true});
      h.host(c, 'protect', [0x2000, 1, 7]);
      const material = compile(c, 'complete executable immediate captures exact INT length', [], [helper(c)], {stopped: true});
      const input = seed({pc: target}), want = loaded(Buffer.from(input), {esp: 0x77f4, flags: 0x240cd7});
      // all caches now equal the legacy flat context: selectors CS4B/SS23, no explicit cached payload.
      want.set(record('R3SG', 64, [0x004b0000, 0x00000023]), SEG);
      want.set(record('R3SB', 40), BASES);
      want.set(packet([0, 0x2300, 0, 0, 0, 0x104]), 100); result(want, 3, 1);
      run(c, 'repaired two-page INT pushes literal return EIP2001 and consumes RF only on attempt', material, input, want,
        {budget: 2, wantRAM: frameRAM(c, [[0x77fc, 'd74f2400'], [0x77f8, '1b000000'], [0x77f4, '01200000']]),
          helpers: material ? [expectedINT(c, material, input, {pc: target})] : []});
    } else h.skip(c, 'complete immediate repair', 'canonical branch NeedCode');
    h.host(c, 'close');
  }
}
// v11-specific falsifiers use a real inner-null fault: no committed RAM/context is hidden by reply corruption.
for (const [resident, entries] of [[false, false], [true, true]]) {
  const c = open(resident, entries, 'declared INT reply controls', [[PC], [0x7000, 3]]);
  fill(c, [0x7000]); configure(c, 0x80, rawGate(7, 0)); h.upload(c, PC, Buffer.from('cd80', 'hex'));
  const input = seed(); h.put(c, 0, input);
  const material = compile(c, 'real inner-null control source', [[PC, 2]], [helper(c)]);
  const controls = [
    ['legacy v9 cannot replace an INT reply', bytes => bytes.writeUInt16LE(9, 104)],
    ['reserved frame metadata', bytes => bytes.writeUInt32LE(0x202, 136)],
    ['word frame cannot publish a high target', bytes => bytes.set(packet([0, 0x10000, 0, 0, 0, 2]), 100)],
    ['IDT error must echo the requested vector', bytes => bytes.set(packet([3, 0, 0x40a, 0, 0, 0]), 100)],
    ['page fault must belong to one of three selected slots', bytes => bytes.set(packet([1, 0, 1, 0x7fff, 2, 2]), 100)],
    ['rejected transport precedes malformed frame metadata', bytes => bytes.writeUInt32LE(0, 136), 7, 3],
    ['SMC transport requires a canonical success packet', () => {}, 11, 2],
  ];
  for (const [label, mutate, transport, detail = 2] of controls) {
    const want = Buffer.from(input); want.writeUInt32LE(ATTEMPT_FLAGS, 52); want.writeUInt32LE(0, 12);
    want.set(packet([3, 0, 0, 0, 0, 2]), 100); mutate(want); result(want, 7, 0, {detail});
    run(c, label, material && altered(c, material, helper(c), mutate, transport), input, want,
      {kind: 'declared-protocol-control', helpers: material ? [expectedINT(c, material, input)] : []});
  }
  h.host(c, 'close');
}
// One stateful code-page write per ownership proves post-commit SMC priority; generic write-currentness proof is reused.
for (const [resident, entries] of [[false, false], [true, true]]) {
  const row = successes[0], c = open(resident, entries, 'INT atomic frame SMC', [[PC], [0x6000], [0xf000]]);
  fill(c, [0xf000]); configure(c, 0x80, rawGate(7));
  h.host(c, 'configure_segment_descriptor_raw', [0x48, ...rawCode({base: 0x4000, default32: false})]);
  h.upload(c, PC, Buffer.from('cd80', 'hex')); h.upload(c, 0xf700, Buffer.from('90ebfe', 'hex'));
  const input = seed(row); h.put(c, 0, input);
  const material = compile(c, 'INT plus independent stack-code-page snapshot', [[PC, 2], [0xf700, 3]], [helper(c)]);
  const want = loaded(Buffer.from(input), {base: 0x4000, cache: 0xf1bfffff, esp: 0x77fb, flags: 0x240ed7});
  want.set(packet([0, 0x2300, 0, 0, 0, 2]), 100); result(want, 6, 1);
  const committed = run(c, 'all frame bytes and CS/flags commit before one-retirement CodeInvalidated', material, input, want,
    {budget: 2, wantRAM: frameRAM(c, row.frame), helpers: material ? [expectedINT(c, material, input, {status: 11})] : []});
  if (committed?.passed) run(c, 'held physical material is stale after committed INT stack write', material, input, input,
    {status: 4, kind: 'stale-material-guard'});
  h.host(c, 'close');
}
{
  const c = open(false, false, 'SIDT readback alone is not executable IDT authority', [[PC]]);
  const query = record('R3SY', 36); query.writeUInt32LE(2, 12); query.writeUInt32LE(0xdeadbeef, 20); query.writeUInt16LE(0x7ff, 26);
  h.put(c, 140, query); h.host(c, 'configure_system_register_bank');
  const target = 0x1200, branch = Buffer.from('e9fb010000', 'hex');
  h.upload(c, PC, branch); h.upload(c, target, Buffer.from('cd80', 'hex'));
  const input = seed({flags: 0x254ed7}); h.put(c, 0, input);
  const material = compile(c, 'query-only real branch bootstrap', [[PC, 5]]);
  const want = Buffer.from(input); want.writeUInt32LE(target, 48); want.writeUInt32LE(0x244ed7, 52); want.writeUInt32LE(0, 12);
  result(want, 3, 1, {version: 1});
  const row = run(c, 'query-only branch still retires normally', material, input, want, {budget: 2});
  if (row?.passed) {
    compile(c, 'unsupplied IDT keeps strict INT neutral', [[target, 2]], [], {expectedStatus: 10});
    compile(c, 'unsupplied IDT keeps stopped INT neutral', [], [], {expectedStatus: 10, stopped: true});
  } else h.skip(c, 'query-only INT refusals', 'canonical branch NeedCode');
  h.host(c, 'close');
}
// a single 66 cannot select the gate width; each source default makes it select the opposite operand size.
const operandPrefixRows = [
  {label: 'single66 gate16 from D16 keeps word frame', kind: 7, info: 2, sourceD: false, ssB: true,
    esp: 0x7801, newESP: 0x77fb, ssBase: 0x8000, cache: 0xf3bfffff, targetD: true, live: 0x240ed7,
    frame: [[0xf7ff, 'd74f'], [0xf7fd, '1b00'], [0xf7fb, '0310']], pages: [0xf000]},
  {label: 'single66 gate32 from D32 keeps dword frame', kind: 14, info: 0x104, sourceD: true, ssB: false,
    esp: 0xa5a50003, newESP: 0xa5a5fff7, ssBase: 0x8000, cache: 0xf1bfffff, targetD: false, live: 0x240cd7,
    frame: [[0x17fff, 'd74f2400'], [0x17ffb, '1b000000'], [0x17ff7, '03100000']], pages: [0x17000, 0x18000]},
];
for (const resident of [false, true]) for (const entries of [false, true]) for (const row of operandPrefixRows) {
  const c = open(resident, entries, row.label, [[PC], ...row.pages.map(page => [page, 3])]);
  fill(c, row.pages); configure(c, 0x80, rawGate(row.kind));
  h.host(c, 'configure_segment_descriptor_raw', [0x18, ...rawCode({default32: row.sourceD})]);
  h.host(c, 'configure_segment_descriptor_raw', [0x48, ...rawCode({base: 0x4000, default32: row.targetD})]);
  h.upload(c, PC, Buffer.from('66cd80', 'hex')); const input = seed(row); h.put(c, 0, input);
  const material = compile(c, row.label + ' exact three-byte capture', [[PC, 3]], [helper(c)]);
  const want = loaded(Buffer.from(input), {base: 0x4000, cache: row.cache, esp: row.newESP, flags: row.live});
  want.set(packet([0, 0x2300, 0, 0, 0, row.info]), 100); result(want, 3, 1);
  run(c, row.label + ': literal return EIP1003 and gate-selected flags/slots', material, input, want,
    {budget: 2, wantRAM: frameRAM(c, row.frame),
      helpers: material ? [call(helper(c), bound(c, material, [0x80, 0x1003, row.esp, ATTEMPT_FLAGS]))] : []});
  h.host(c, 'close');
}
{
  const c = open(false, false, 'selected INT repeat-prefix boundary', [[PC]]);
  configure(c, 0x80, rawGate(7)); const target = 0x1200;
  h.upload(c, PC, Buffer.from('e9fb010000', 'hex')); h.upload(c, target, Buffer.from('f3cd80', 'hex'));
  const input = seed({flags: 0x254ed7}); h.put(c, 0, input);
  const material = compile(c, 'prefix-control real branch', [[PC, 5]]);
  const want = Buffer.from(input); want.writeUInt32LE(target, 48); want.writeUInt32LE(0x244ed7, 52); want.writeUInt32LE(0, 12);
  result(want, 3, 1, {version: 1});
  const row = run(c, 'branch reaches the unselected repeat-prefix form', material, input, want, {budget: 2});
  if (row?.passed) {
    compile(c, 'REP INT stays neutral under the selected bare-or-single66 contract', [[target, 3]], [], {expectedStatus: 10});
    compile(c, 'stopped REP INT stays neutral', [], [], {expectedStatus: 10, stopped: true});
  } else h.skip(c, 'REP INT refusals', 'canonical branch NeedCode');
  h.host(c, 'close');
}
assert.equal(successes.length, 3); assert.equal(faults.length, 13); assert.equal(operandPrefixRows.length, 2);
assert.deepEqual(pins(), sourcePins, 'INT owners and literal oracles stayed fixed');
writeFileSync(join(output, 'interrupt-pre-automatic-checkpoints.json'), JSON.stringify(checkpoints, null, 2) + '\n');
writeFileSync(join(output, 'interrupt-source-pins.json'), JSON.stringify(sourcePins, null, 2) + '\n');
writeFileSync(join(output, 'interrupt-counts.json'), JSON.stringify({observed,
  semanticRows: {coreSuccessPaths: 2, targetWidthFocusedPath: 1, orderedFaultsAndNeutral: faults.length, singleOperandPrefixFamilies: operandPrefixRows.length},
  expectedFullAdmission: {contexts: 83, compileCalls: 113, compileRefusals: 16, modules: 97, guestCalls: 126, observations: 239},
  profiles: 'replacement/resident x extents/entries; extra lifecycle/protocol/SMC paths are risk-selected explicitly', preAutomaticCheckpoints: checkpoints.length, sourcePins}, null, 2) + '\n');
h.finish([
  'Explicitly configured immutable complete IDT before bootstrap/material; SIDT readback alone keeps INT neutral Unsupported.',
  'Literal16trap and32interrupt frames, opposite source/target CS.D and SS.B, based handler MOV and matching IRET; one focused targetD32/66IRET16 path proves frame width independent of targetD; AC1 unaligned under explicit AM0.',
  'RF0 saved frame, TF/NT clear live, interrupt IFclear versus trap IFpreserve, CPL3 IOPL0 IRET IF mask, highESP preserved forB16 and zero-padding selected for32bit CS frame.',
  'Original ordered IDT/target/cachedSS/targetlimit/page failures are checked at the complete pre-automatic stop, retaining exact v11 source packets. Final contributory faults with absent exception/DF handlers latch shutdown and retain the original diagnostic; raw MemoryFault stays a paging authority boundary. Vector-zero IDTerror2/CD03 and conforming/task/interprivilege distinctions remain.',
  'Every observation compares full4796 arena/allmappedRAM and exact real helper trace; v11-negative controls invoke real inner-null helper before declared packet/transport mutation.',
  'Wrong-key guard precedes poisoned packet; cancel/zero budget precede attempt; physical SMC commits full frame/transfer before one retirement and invalidates held material.',
  'Complete two-page immediate fetch/EXEC repair and new CSbase/D handler capture use current NeedCode lifecycle; single66 preserves configured gate width in both code defaults with literal three-byte return EIP; REP stays neutral; existing generic flags/fault/currentness evidence reused.',
  'No RAM-backed IDT, complete TSS/CPL transition, IRQ/NMI, certified PF/CR2, AM1/AC-delivery, x64/FRED/CET, Windows exception provider, browser/game/performance or wholeCPU claim; configured finite secondary/DF/shutdown routing is now explicit.',
]);
