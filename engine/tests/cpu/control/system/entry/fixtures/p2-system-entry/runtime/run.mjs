import assert from 'node:assert/strict';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {pathToFileURL} from 'node:url';

const [enginePath, output, root] = process.argv.slice(2);
const support = 'engine/tests/cpu/segments/code/base/fixtures/p2-cs-base/runtime/support.mjs';
const {harness, hash, record, registers, PC, SEG, BASES} = await import(pathToFileURL(join(root, support)));
const h = await harness(enginePath, output, root), handoffs = [];
const sourcePaths = ['engine/src/cpu/exit.rs', 'engine/src/cpu/x86/control.rs',
  'engine/src/cpu/x86/decode/control.rs', 'engine/src/cpu/x86/decode/profile.rs',
  'engine/src/cpu/dbt/wasm/control_instructions.rs', 'engine/src/cpu/dbt/wasm/software_interrupts.rs',
  'engine/src/cpu/dbt/wasm/detection.rs', 'engine/src/cpu/dbt/wasm/emitter.rs',
  'engine/src/cpu/dbt/wasm/abi.rs', 'engine/src/cpu/dbt/region.rs',
  'engine/src/abi/x86/exit.rs', 'engine/src/abi/x86/event.rs',
  'engine/src/process/cpu/event.rs', 'engine/src/process/wasm/detection.rs',
  'engine/src/process/cpu/interrupts/detection/site.rs', 'engine/src/process/cpu/interrupts/detection/restart.rs',
  'engine/tests/cpu/control/system/entry/native.rs', 'engine/tests/cpu/control/system/entry/wasm.rs',
  'engine/tests/cpu/control/system/entry/fixtures/p2-system-entry/runtime/run.mjs'];
const pins = () => Object.fromEntries(sourcePaths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = pins();

function context(code32, csLimit) {
  const sg = record('R3SG', 64, [], 3);
  [0x23, 0x1b, 0x23, 0x23, 0, 0].forEach((value, i) => sg.writeUInt16LE(value, 16 + i * 2));
  const code = csLimit === undefined ? code32 ? 0xf3bfffff : 0xf1bfffff
    : ((code32 ? 0xf2b00000 : 0xf0b00000) | csLimit) >>> 0;
  [0xf33fffff, code, 0xf33fffff, 0xf33fffff, 0, 0]
    .forEach((value, i) => sg.writeUInt32LE(value >>> 0, [28, 32, 36, 40, 44, 56][i]));
  return {sg, sb: record('R3SB', 40, [0x1000, 0, 0x2000, 0x3000, 0, 0])};
}
function seed(code32, {flags = 0x254fd7, shadow = 1, cancel = 0, csLimit} = {}) {
  const bytes = Buffer.from(h.initial), regs = [...registers]; regs[4] = 0xdead7800;
  bytes.set(record('R3ST', 56, [...regs, PC, flags], 3)); bytes.writeUInt32LE(shadow, 12);
  bytes.writeUInt32LE(cancel, 96);
  const {sg, sb} = context(code32, csLimit); bytes.set(sg, SEG); bytes.set(sb, BASES);
  for (let at = 100; at < 4236; at++) bytes[at] = (at * 17 + 3) & 255;
  for (const [start, end] of [[4276, 4356], [4396, 4524], [4628, 4756]])
    for (let at = start; at < end; at++) bytes[at] = (at * 29 + 11) & 255;
  return bytes;
}
function exit(bytes, reason, retired, detail = 0, version = 6) {
  bytes.set(record('R3EX', 40, [reason, retired, detail, 0, 0, 0], version), 56);
}
function finish(epoch, reason, detail, original, {pc = PC, retired = 0} = {}) {
  return {name: 'finish_exception_detection', args: [String(epoch), reason, detail, 0, 0, 0,
    0, ...(original ? [pc, retired, original.readUInt32LE(52), original.readUInt32LE(12), 0] : [0, 0, 0, 0, 0])], status: 0};
}
function handoff(c, label, vector, original, changedContext, {pc = PC, retired = 0} = {}) {
  const stopped = h.arena(c), ram = h.ram(c);
  if (changedContext) {
    const changed = Buffer.from(stopped); changed.writeUInt32LE(0x3000, 48); h.put(c, 0, changed);
    assert.equal(c.api.take_cpu_event(), 15, 'changed EIP rejects the original event without consuming it');
    assert.deepEqual(h.arena(c), changed, 'rejected event take changes no arena byte');
    assert.deepEqual(h.ram(c), ram, 'rejected event take changes no RAM'); h.put(c, 0, stopped);
  }
  const metadata = record('R3CE', 48, [vector, pc, stopped.readUInt32LE(48), retired,
    original.readUInt32LE(52), original.readUInt32LE(12), 0xffffffff, stopped.readUInt32LE(76)]);
  const want = Buffer.from(stopped); want.set(metadata, 140);
  assert.equal(c.api.take_cpu_event(), 0, 'precise system-entry event transfers once');
  assert.deepEqual(h.arena(c), want, 'only the exact 48-byte original-site metadata is published');
  assert.deepEqual(h.ram(c), ram, 'event handoff changes no mapped RAM');
  assert.equal(c.api.take_cpu_event(), 7, 'second event take rejects the consumed event');
  assert.deepEqual(h.arena(c), want, 'second take changes no arena byte');
  assert.deepEqual(h.ram(c), ram, 'second take changes no mapped RAM');
  handoffs.push({context: c.key, label, metadata_hex: metadata.toString('hex'),
    before_sha256: hash(stopped), after_sha256: hash(want), once: true, changed_context: changedContext});
}

let checkedChangedContext = false;
for (const resident of [false, true]) for (const entries of [false, true]) for (const code32 of [false, true]) {
  const forms = [
    {bytes: [0x0f, 0x34], name: 'SYSENTER', detail: 0x100, version: 15, vector: 0xffffffff},
    {bytes: [0x66, 0x0f, 0x34], name: 'operand override SYSENTER', detail: 0x100, version: 15, vector: 0xffffffff},
    {bytes: [0x67, 0x0f, 0x34], name: 'address override SYSENTER', detail: 0x100, version: 15, vector: 0xffffffff},
    {bytes: [0x64, 0x0f, 0x34], name: 'null FS override SYSENTER', detail: 0x100, version: 15, vector: 0xffffffff},
    {bytes: [...(code32 ? [] : [0x66]), 0x89, 0xe2, 0x0f, 0x34], priorLength: code32 ? 2 : 3,
      name: 'Windows MOV EDX,ESP fast-call stub', detail: 0x100, version: 15, vector: 0xffffffff},
    {bytes: [0xcd, 0x29], name: 'existing INT29 entry', detail: 0x29, version: 14, vector: 0x29},
    {bytes: [0xcd, 0x2e], name: 'existing INT2E entry', detail: 0x2e, version: 14, vector: 0x2e},
  ];
  for (const form of forms) {
    const bytes = Buffer.from(form.bytes), c = h.open(resident, entries, `${form.name}/D${code32 ? 32 : 16}`, [[PC, 7]]);
    const {sg, sb} = context(code32); h.put(c, SEG, sg); h.put(c, BASES, sb);
    h.upload(c, PC, Buffer.concat([bytes, Buffer.from([0xf0, 0x90])]));
    const material = h.compile(c, 'entry terminates before invalid tail', [[PC, bytes.length]]);
    if (!material) {h.skip(c, 'entry execution', 'application entry compilation'); h.host(c, 'close'); continue;}
    const cases = form.priorLength ? [
      ['real fast-call stub preserves full entry ESP in EDX and original SYSENTER site', {flags: 0x10202, budget: 2}],
    ] : form.detail === 0x100 ? [
      ['entry preserves user flags and registers while consuming RF and SS shadow', {}],
      ['entry wins over old TF without SS inhibition', {flags: 0x10302, shadow: 0}],
      ['zero budget precedes code-span denial and leaves RF and shadow intact', {budget: 0, csLimit: PC + bytes.length - 2}],
      ['cancel precedes zero budget and code-span denial', {budget: 0, cancel: 1, csLimit: PC + bytes.length - 2}],
      ['live code-span fault precedes entry and RF consumption', {csLimit: PC + bytes.length - 2}],
    ] : [['existing INT entry retains exact ABI14', {}]];
    for (const [index, [label, options]] of cases.entries()) {
      const input = seed(code32, options), want = Buffer.from(input), original = Buffer.from(input), calls = [];
      const site = {pc: PC + (form.priorLength ?? 0), retired: Number(Boolean(form.priorLength))};
      const stopped = options.cancel || options.budget === 0, denied = options.csLimit !== undefined;
      if (stopped) exit(want, options.cancel ? 2 : 1, 0);
      else if (denied) {exit(want, 11, 0); calls.push(finish(index + 1, 11, 0));}
      else {
        want.writeUInt32LE(PC + bytes.length, 48); want.writeUInt32LE(input.readUInt32LE(52) & ~0x10000, 52);
        want.writeUInt32LE(0, 12); exit(want, 24, site.retired + 1, form.detail, form.version);
        if (form.priorLength) {
          want.writeUInt32LE(input.readUInt32LE(32), 24);
          original.writeUInt32LE(input.readUInt32LE(52) & ~0x10000, 52); original.writeUInt32LE(0, 12);
        }
        calls.push(finish(index + 1, 24, form.detail, original, site));
      }
      h.observe(c, label, {input, want, material, budget: options.budget ?? 1, helpers: calls});
      if (!stopped && !denied) {
        handoff(c, label, form.vector, original, !checkedChangedContext, site); checkedChangedContext = true;
      }
    }
    h.host(c, 'close');
  }
}
assert.deepEqual(pins(), sourcePins, 'entry owners and independent literal oracles remain fixed');
writeFileSync(join(output, 'entry-evidence.json'), JSON.stringify({source_pins: sourcePins, handoffs}, null, 2) + '\n');
h.finish(['distinct SYSENTER ABI15 and unchanged INT29/INT2E ABI14 across both code defaults and four bound routes',
  'Windows MOV EDX,ESP fast-call sequence preserves full 32-bit EDX and captures the second instruction with prior retirement 1',
  'unmapped stack, null FS override, unchanged GPR/segments/x87/SIMD/YMM/helper scratch and complete mapped RAM',
  'precise original-site snapshot and once-only full-context event handoff; budget/cancel/fetch priority and RF/TF/SS-shadow semantics',
  'observed next EIP is decoded fallthrough only; Windows provider must choose the resume context; no OS service, kernel, browser or game claim']);
