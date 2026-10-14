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
const TARGET = 0x2300, TARGET_BASE = 0x4000, FLAGS = 0x240ed7;
const paths = [
  'engine/src/cpu/dbt/wasm/detection.rs', 'engine/src/cpu/dbt/wasm/emitter.rs',
  'engine/src/cpu/dbt/wasm/abi.rs', 'engine/src/cpu/dbt/wasm/memory.rs',
  'engine/src/cpu/dbt/wasm/control_instructions.rs', 'engine/src/cpu/dbt/wasm/strings.rs',
  'engine/src/cpu/dbt/wasm/xstate.rs', 'engine/src/cpu/dbt/wasm/far/task.rs',
  'engine/src/cpu/dbt/region.rs', 'engine/src/cpu/dbt/cold.rs', 'engine/src/cpu/dbt/artifact.rs',
  'engine/src/cpu/dbt/resident.rs', 'engine/src/cpu/x86/decode/authority.rs',
  'engine/src/cpu/x86/interrupts/cause.rs', 'engine/src/cpu/x86/interrupts/escalation.rs',
  'engine/src/cpu/x86/interrupts/frame.rs', 'engine/src/cpu/x86/interrupts/outcome.rs',
  'engine/src/process/instance.rs', 'engine/src/process/cpu/interrupts/detection.rs',
  'engine/src/process/cpu/interrupts/detection/restart.rs',
  'engine/src/process/cpu/interrupts/detection/site.rs',
  'engine/src/process/cpu/interrupts/entry.rs', 'engine/src/process/cpu/interrupts/entry/state.rs',
  'engine/src/process/cpu/interrupts/delivery.rs', 'engine/src/abi/exception.rs',
  'engine/src/process/wasm/detection.rs', 'engine/src/abi/wasm/exports/detection.rs',
  'engine/tests/cpu/control/interrupt/exception/automatic/wasm.rs',
  'engine/tests/cpu/control/interrupt/exception/automatic/fixtures/p2-exception-auto/runtime/run.mjs', support,
];
const pins = () => Object.fromEntries(paths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = pins(), observed = {contexts: 0, compiles: 0, invocations: 0};
const call = (name, args, status = 0) => ({name,
  args: args.map(v => typeof v === 'bigint' ? v.toString(10) : v >>> 0),
  status: typeof status === 'bigint' ? status.toString(10) : status});
const bound = (c, material, args = []) => [...(c.resident ? material.binding : []), ...args];
const finishName = 'finish_exception_detection';
const storeName = c => c.resident ? 'cpu_store_resident8' : 'cpu_store8';
const returnName = c => c.resident ? 'far_return_resident' : 'far_return';
const taskName = c => c.resident ? 'task_return_resident' : 'task_return';
function context(bytes, {cs = 0x1b, base = 0, default32 = true, stack32 = true, legacy = false} = {}) {
  const sg = record('R3SG', 64, [], legacy ? 1 : 3);
  [0x23, cs, 0x23, 0x23, 0x3b, 0].forEach((v, i) => sg.writeUInt16LE(v, 16 + 2 * i));
  if (!legacy) [0xf33fffff, default32 ? 0xf3bfffff : 0xf1bfffff,
    stack32 ? 0xf33fffff : 0xf13fffff, 0xf33fffff, 0xf33fffff, 0]
    .forEach((v, i) => sg.writeUInt32LE(v >>> 0, [28, 32, 36, 40, 44, 56][i]));
  bytes.set(sg, SEG); bytes.set(record('R3SB', 40, [0, base, 0, 0, 0, 0]), BASES); return bytes;
}
function seed(row) {
  const bytes = Buffer.from(h.initial), regs = [...registers]; regs[4] = row.esp ?? 0x8000;
  for (const [index, value] of row.regs ?? []) regs[index] = value;
  if (row.rep || row.priorRep) {regs[1] = row.priorRep ? 1 : 2; regs[6] = 0x5000; regs[7] = row.smc ? PC : 0x5100;}
  bytes.set(record('R3ST', 56, [...regs, PC, row.flags ?? FLAGS | 0x10000], 3));
  bytes.writeUInt32LE(row.step || row.rep ? 0 : 1, 12); context(bytes, row);
  for (let i = 0; i < 80; i++) bytes[4276 + i] = (i * 29 + 17) & 255;
  for (let i = 0; i < 8; i++) {
    bytes.fill(0x30 + i, 4396 + i * 16, 4412 + i * 16);
    bytes.fill(0x70 + i, 4628 + i * 16, 4644 + i * 16);
  }
  return bytes;
}
function rawCode(default32) {return [0x4000ffff, (default32 ? 0xcffa00 : 0x8ffa00) >>> 0];}
function rawGate(vector, row) {
  return [((row.taskGate ? 0x50 : 0x48) << 16) | TARGET,
    (row.present === false ? 0 : 0x8000) | (row.dpl ?? 0) << 13 | (row.taskGate ? 5 : row.kind ?? 15) << 8];
}
function configure(c, row) {
  if (row.configured === false) return;
  const idt = record('R3ID', 2068); idt.writeUInt16LE(0x7ff, 16);
  const gates = [[row.vector, rawGate(row.vector, row)]];
  if (row.secondary !== undefined) gates.push([row.secondary, rawGate(row.secondary, {})]);
  if (row.df) gates.push([8, rawGate(8, {present: !row.shutdown})]);
  for (const [vector, values] of gates) if (vector !== undefined) idt.set(words(values), 20 + vector * 8);
  h.put(c, 140, idt); h.host(c, 'configure_interrupt_descriptor_table');
  h.host(c, 'configure_segment_descriptor_raw', [0x48, ...rawCode(row.targetD ?? true)]);
  if (row.task) {
    h.put(c, 140, record('R3TR', 32, [0x43, 0x4f, 0x50000067, 0x8b00]));
    h.host(c, 'configure_task_return_sources');
    h.host(c, 'configure_segment_descriptor_raw', [0x40, 0, 0]);
  }
}
function stopWant(input, row) {
  const want = Buffer.from(input), flags = input.readUInt32LE(52) & ~0x10000;
  want.writeUInt32LE(flags, 52); want.writeUInt32LE(0, 12);
  want.writeUInt32LE(row.next ?? PC, 48); exit(want, row.reason, row.retired ?? 0, row.version ?? 7);
  want.writeUInt32LE(row.detail ?? 0, 80);
  if (row.task) want.set(record('R3MH', 40, [3, 0, 0, 0, 0, 0], 13), 100);
  if (row.rep || row.priorRep) {
    want.writeUInt32LE(row.priorRep ? 0 : 1, 20); want.writeUInt32LE(0x5001, 40);
    want.writeUInt32LE(row.smc ? PC + 1 : 0x5101, 44); want.writeUInt32LE(row.priorRep ? 2 : 1, 4604);
    want.set(record('R3MH', 40, [0, 0, 0, 0, 0, 1], 10), 100);
  }
  if (row.rawFault) {
    want.set(record('R3MH', 40, [1, 0, 1, 0x6000, 1, 4], 10), 100);
    want.writeUInt32LE(1, 80); want.writeUInt32LE(0x6000, 84);
    want.writeUInt32LE(1, 88); want.writeUInt32LE(4, 92);
  }
  return want;
}
function finalWant(input, row) {
  const want = stopWant(input, row);
  if (row.configured === false || row.rawFault) return want;
  if (row.shutdown) {
    want.set(record('R3MH', 40, [6, 0, 0x43, 0, 0, 0], 12), 100); return want;
  }
  if (row.taskGate) {
    // a neutral authority boundary restores the exact trusted pre-instruction image.
    want.writeUInt32LE(PC, 48); want.writeUInt32LE(input.readUInt32LE(52), 52);
    want.writeUInt32LE(input.readUInt32LE(12), 12); want.writeUInt32LE(0, 76);
    want.set(record('R3MH', 40, [8, 0, 1, 0x50, 0, 0], 12), 100); return want;
  }
  context(want, {cs: 0x4b, base: TARGET_BASE, default32: row.targetD ?? true, stack32: row.stack32 ?? true});
  want.writeUInt32LE(TARGET, 48); want.writeUInt32LE(row.newESP ?? (row.error ? 0x7ff0 : 0x7ff4), 32);
  want.writeUInt32LE((input.readUInt32LE(52) & ~0x10000) & ~0x34100, 52);
  exit(want, 3, row.failedTrap ? row.restartRetired ?? 0 : row.retired ?? 0, Math.max(row.version ?? 7, 6));
  if (row.priorRep) want.writeUInt32LE(1, 4604);
  want.set(record('R3MH', 40, [0, TARGET, 0, 0, 0, row.info ?? (row.error ? 0x204 : 4)], 12), 100);
  return want;
}
function finishArgs(row, epoch = 1n) {
  const restart = row.priorRep ? [PC + 2, 1, FLAGS & ~0x400, 2, 1] :
    row.trap ? [PC, 0, row.flags ?? FLAGS | 0x10000, 1, 0] : [0, 0, 0, 0, 0];
  return [epoch, row.reason, row.rawFault ? 1 : row.detail ?? 0, row.rawFault ? 0x6000 : 0,
    row.rawFault ? 1 : 0, row.rawFault ? 4 : 0, row.debug ?? 0, ...restart];
}
function helpers(c, row) {
  return [...(row.rep || row.priorRep ? ['cpu_read8', storeName(c)] : row.rawFault ? ['cpu_read32'] :
    row.task ? ['cpu_read', returnName(c), taskName(c)] : []),
  ...(row.configured === false ? [] : [finishName])];
}
function trace(c, material, row, epoch = 1n) {
  const calls = row.rep || row.priorRep ? [call('cpu_read8', [0x5000]),
    call(storeName(c), bound(c, material, [row.smc ? PC : 0x5100, 0x7a]), row.smc ? 11 : 0)] :
    row.rawFault ? [call('cpu_read32', [0x6000])] :
    row.task ? [call(taskName(c), bound(c, material))] : [];
  if (row.configured !== false) calls.push(call(finishName, finishArgs(row, epoch), row.smc || row.frameSmc ? 11 : 0));
  return calls;
}
function desiredRAM(c, row) {
  const ram = h.ram(c);
  if (row.rep || row.priorRep) writeRAM(ram, row.smc ? PC : 0x5100, Buffer.from([0x7a]));
  if (row.frame) writeRAM(ram, row.frameAt ?? (row.error ? 0x7ff0 : 0x7ff4), Buffer.from(row.frame, 'hex'));
  return ram;
}
function compile(c, label, spans, expected) {observed.compiles++; return h.compile(c, label, spans, expected);}
function observe(c, label, material, input, want, options = {}) {
  if (!material) {h.skip(c, label, 'source material admission'); return null;}
  observed.invocations++; return h.observe(c, label, {material, input, want, ...options});
}
function intercept(c, material, mutation) {
  const module = new WebAssembly.Module(new Uint8Array(c.memory.buffer, material.pointer, material.length));
  const ring3 = Object.fromEntries(WebAssembly.Module.imports(module).filter(i => i.kind === 'function').map(i =>
    [i.name, (...args) => {
      mutation(i.name, args); const status = c.api[i.name](...args);
      c.calls.push(call(i.name, args, status)); return status;
    }]));
  return {...material, run: new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3}).exports.run};
}
const rows = [
  {label: 'UD fault uses restart RF1 and returns through a real handler', code: '0f0b', vector: 6, reason: 20,
    profiles: 'four', targetD: false, handler: '66cf', frame: '001000001b000000d70e2500'},
  {label: 'DE zero divisor never publishes EAX or EDX', code: 'f7f1', vector: 0, reason: 10, version: 5,
    profiles: 'both', regs: [[1, 0], [2, 0]], frame: '001000001b000000d70e2500'},
  {label: 'GP0 word frame uses gate width and B16 high ESP', code: '0f01d0ebfe', vector: 13, reason: 11,
    profiles: 'four', version: 9, regs: [[1, 1]], kind: 7, stack32: false, esp: 0xabcd8000,
    newESP: 0xabcd7ff8, error: true, info: 0x202, frameAt: 0x7ff8, handler: '83c40266cf',
    frame: '000000101b00d70e'},
  {label: 'NT1 TS0 delivers before any ordinary IRET SS read', code: 'cf', vector: 10, reason: 21,
    profiles: 'both', version: 10, task: true, flags: FLAGS | 0x14000, error: true,
    frame: '00000000001000001b000000d74e2500'},
  {label: 'CC success saves next PC and RF0', code: 'cc', vector: 3, reason: 17,
    trap: true, dpl: 3, next: PC + 1, retired: 1, frame: '011000001b000000d70e2400'},
  {label: 'INTO success saves next PC and RF0', code: 'ce', vector: 4, reason: 18,
    trap: true, dpl: 3, next: PC + 1, retired: 1, frame: '011000001b000000d70e2400'},
  {label: 'F1 success bypasses DPL and saves next PC', code: 'f1', vector: 1, reason: 16,
    trap: true, residentOnly: true, debug: 1, next: PC + 1, retired: 1, frame: '011000001b000000d70e2400'},
  {label: 'CC DPL failure after real REP keeps earlier work and original CC PC', code: 'f3a4cc', vector: 3, reason: 17,
    trap: true, priorRep: true, failedTrap: true, secondary: 13, error: true,
    flags: (FLAGS & ~0x400) | 0x10000, next: PC + 3, retired: 2, restartRetired: 1,
    frame: '1a000000021000001b000000d70a2500'},
  {label: 'INTO DPL failure delivers GP22 from original restart', code: 'ce', vector: 4, reason: 18,
    trap: true, failedTrap: true, secondary: 13, error: true, next: PC + 1, retired: 1,
    frame: '22000000001000001b000000d70e2500'},
  {label: 'F1 NP failure delivers external NP from original restart', code: 'f1', vector: 1, reason: 16,
    trap: true, failedTrap: true, residentOnly: true, debug: 1, present: false, secondary: 11,
    error: true, next: PC + 1, retired: 1, frame: '0b000000001000001b000000d70e2500'},
  {label: 'CC task-gate authority refusal restores pre-instruction RF and shadow', code: 'cc',
    vector: 3, reason: 17, trap: true, taskGate: true, dpl: 3, next: PC + 1, retired: 1},
  {label: 'old TF saves actual taken JMP continuation without forced RF', code: 'e9fb010000',
    vector: 1, reason: 16, debug: 2, step: true, flags: FLAGS | 0x10100, next: 0x1200,
    retired: 1, frame: '001200001b000000d70f2400'},
  {label: 'unfinished REP TF saves RF1 and completed element work', code: 'f3a4', vector: 1,
    reason: 16, debug: 3, profiles: 'four', rep: true, flags: (FLAGS & ~0x400) | 0x10100,
    frame: '001000001b000000d70b2500'},
  {label: 'unfinished REP code write keeps stale detected progress deliverable', code: 'f3a4',
    vector: 1, reason: 16, debug: 3, profiles: 'four', rep: true, smc: true,
    flags: (FLAGS & ~0x400) | 0x10100, frame: '001000001b000000d70b2500'},
  {label: 'GP plus NP delivery failure enters error-zero DF abort', code: '0f01d0ebfe', vector: 13,
    reason: 11, version: 9, regs: [[1, 1]], present: false, df: true, error: true,
    frame: '00000000001000001b000000d70e2400'},
  {label: 'failed DF latches shutdown without partial frame writes', code: '0f01d0ebfe', vector: 13,
    reason: 11, version: 9, regs: [[1, 1]], residentOnly: true, present: false, df: true, shutdown: true},
  {label: 'raw software-bus read fault does not synthesize architectural PF', code: 'a100600000ebfe',
    reason: 5, version: 6, rawFault: true},
  {label: 'committed UD frame code write enters with SMC transport11', code: '0f0b', vector: 6, reason: 20,
    frameSmc: true, esp: 0x100c, newESP: PC, frameAt: PC, frame: '001000001b000000d70e2500'},
  {label: 'absent IDT retains legacy imports and UD stop', code: '0f0b', reason: 20, configured: false},
];

for (const resident of [false, true]) for (const entries of [false, true]) for (const row of rows) {
  if (row.profiles !== 'four' && (entries || (row.residentOnly ? !resident : row.profiles !== 'both' && resident))) continue;
  observed.contexts++;
  const pages = [PC, 0x5000, ...(row.rawFault ? [] : [0x6000]), 0x7000];
  const c = h.open(resident, entries, row.label, pages.map(page => [page, page === PC || page === 0x6000 ? 7 : 3]));
  if (row.shutdown) c.diagnosticRAM = true;
  for (const page of pages.filter(page => page !== PC)) h.upload(c, page, Buffer.alloc(4096, 0x6b));
  configure(c, row); h.upload(c, PC, Buffer.from(row.code, 'hex'));
  if (row.rep || row.priorRep) h.upload(c, 0x5000, Buffer.from([0x7a, 0x8b]));
  if (row.task) h.upload(c, 0x5000, Buffer.from([0, 0]));
  if (row.handler) h.upload(c, TARGET_BASE + TARGET, Buffer.from(row.handler, 'hex'));
  const input = seed(row); h.put(c, 0, input);
  const spans = row.priorRep ? [[PC, 2], [PC + 2, 1]] : [[PC, row.code.length / 2]];
  const material = compile(c, row.label + ': real source compile', spans, helpers(c, row));
  if (row === rows[0] && !entries) {
    if (material) observe(c, 'wrong owner rejects before detector lease and CPU effects',
      intercept(c, material, (name, args) => {if (name.startsWith('guard')) args[0] ^= 1;}),
      input, input, {status: 3});
    else h.skip(c, 'wrong-owner detector guard', 'source material admission');
  }
  const want = finalWant(input, row);
  const result = observe(c, row.label + ': emitted detector and automatic entry', material, input, want,
    {budget: 2, status: row.smc || row.frameSmc ? 11 : 0, helpers: trace(c, material, row), wantRAM: desiredRAM(c, row)});
  if (row === rows[0] && !resident && !entries && result?.passed) {
    const args = finishArgs(row);
    const direct = {run() {const status = c.api[finishName](...args); c.calls.push(call(finishName, args, status)); return status;}};
    observe(c, 'already-consumed epoch cannot publish a second frame', direct, want, want,
      {status: 14, helpers: [call(finishName, args, 14)]});
    const poisoned = finishArgs(row, 2n); poisoned[2] = 1;
    observe(c, 'terminal detail mismatch is protocol refusal before entry',
      intercept(c, material, (name, args) => {if (name === finishName) args[2] = 1;}),
      input, stopWant(input, row), {status: 7, helpers: [call(finishName, poisoned, 7)]});
  }
  if (row === rows[0] && !resident && !entries && !result?.passed) {
    h.skip(c, 'consumed epoch replay', 'successful automatic entry');
    h.skip(c, 'malformed terminal detail', 'successful first entry before independent rerun');
  }
  if ((row.smc || row.frameSmc) && !result?.passed)
    h.skip(c, 'fresh physical guard rejects stale committed source', 'successful committed SMC entry');
  if ((row.smc || row.frameSmc) && result?.passed) {
    const guard = c.resident ? 'guard_detect_resident' : 'guard_detect';
    const args = c.resident ? [...material.binding, c.base, c.base + 56, c.base + 96]
      : [c.key, KEY_HIGH, material.generation, c.base, c.base + 56, c.base + 96];
    const guarded = observe(c, 'retained delivery succeeds while a fresh emitted guard rejects the stale source',
      material, want, want, {status: 4});
    assert.deepEqual(guarded.calls, [call(guard, args, -4n)], 'exact emitted stale-guard transport');
  }
  if (row.shutdown && !result?.passed) h.skip(c, 'latched shutdown guard', 'successful shutdown observation');
  if (row.shutdown && result?.passed) {
    observe(c, 'latched shutdown rejects the next real emitted guard without effects',
      material, want, want, {status: 22});
  }
  if (row.handler && result?.passed) {
    h.put(c, 0, want);
    const handler = compile(c, row.label + ': actual handler capture', [[TARGET, row.handler.length / 2]],
      ['cpu_read', returnName(c), finishName]);
    const handlerInput = Buffer.from(want); handlerInput.set(words([TARGET, row.handler.length / 2]), 140);
    if (c.resident && handler) handlerInput.set(h.arena(c).subarray(140, 164), 140);
    const returned = Buffer.from(handlerInput), slot = row.kind === 7 ? 2 : 4;
    context(returned, {stack32: row.stack32 ?? true, legacy: row.stack32 !== false});
    returned.writeUInt32LE(PC, 48); returned.writeUInt32LE(row.esp ?? 0x8000, 32);
    returned.writeUInt32LE(slot === 4 ? FLAGS | 0x10000 : FLAGS, 52);
    returned.set(record('R3MH', 40, [0, PC, 0, 0, 0, 0], 7), 100);
    returned.set(slot === 4 ? Buffer.from('d70e2500', 'hex') : Buffer.from('d70e', 'hex'), 140);
    exit(returned, 1, slot === 4 ? 1 : 2, 6);
    const addresses = slot === 4 ? [0x7ff4, 0x7ff8, 0x7ffc] : [0x7ffa, 0x7ffc, 0x7ffe];
    const calls = addresses.map(address => call('cpu_read', [address, slot, 1]));
    if (handler) calls.push(call(returnName(c), bound(c, handler, [PC, 0x1b, Number(slot === 2)])));
    observe(c, 'real handler removes error if present and IRETs without detector finish', handler,
      handlerInput, returned, {budget: slot === 4 ? 1 : 2, helpers: calls});
  } else if (row.handler) h.skip(c, row.label + ': handler/IRET', 'successful automatic entry');
  h.host(c, 'close');
}
assert.deepEqual(pins(), sourcePins, 'consumer and source owners stayed fixed');
writeFileSync(join(output, 'automatic-source-pins.json'), JSON.stringify(sourcePins, null, 2) + '\n');
h.finish({planned: {contexts: 33, compiles: 41, generatedModules: 41, invocations: 51, observations: 92}, observed,
  scope: 'Actual generated detection and once-only same-CPL exception routing. Raw bus faults remain paging-authority boundaries; no PF metadata/CR2, NT task transition or Windows exception-provider claim.'});
