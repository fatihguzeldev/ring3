import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {createArena, ARENA_SIZE} from '../../../../../../support/wasm/arena.mjs';
import {readEngine} from '../../../../../../support/wasm/engine.mjs';

const [enginePath, output, root] = process.argv.slice(2);
const engine = readEngine(enginePath);
const PC = 0x1000;
const observations = [];
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const sources = [
  'engine/src/cpu/x86/control.rs',
  'engine/src/cpu/x86/decode/control.rs',
  'engine/src/cpu/dbt/wasm/control_instructions.rs',
  'engine/src/cpu/dbt/wasm/emitter.rs',
  'engine/src/cpu/dbt/wasm/abi.rs',
  'engine/src/cpu/dbt/wasm/strings.rs',
  'engine/src/cpu/dbt/wasm/memory.rs',
  'engine/src/cpu/dbt/wasm/segments.rs',
  'engine/src/cpu/dbt/wasm/locals.rs',
  'engine/src/abi/header.rs',
  'engine/src/process/calls/context.rs',
  'engine/src/process/calls/call.rs',
  'engine/src/cpu/dbt/wasm/integer/dispatch.rs',
  'engine/src/cpu/dbt/wasm/integer/flags.rs',
  'engine/src/cpu/dbt/wasm/integer/bits.rs',
  'engine/src/cpu/dbt/wasm/integer/decimal.rs',
  'engine/src/cpu/dbt/wasm/integer/multiply_divide.rs',
  'engine/src/cpu/dbt/wasm/integer/shifts.rs',
  'engine/src/abi/x86/state.rs',
  'engine/src/abi/x86/exit.rs',
  'engine/tests/cpu/control/exceptions/fixtures/p2-flags-exceptions/runtime/run.mjs',
];
const pins = () => Object.fromEntries(sources.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = pins();

function words(values, word = false) {
  const bytes = Buffer.alloc(values.length * (word ? 2 : 4));
  values.forEach((value, index) => word
    ? bytes.writeUInt16LE(value & 0xffff, index * 2)
    : bytes.writeUInt32LE(value >>> 0, index * 4));
  return bytes;
}

function open(program, resident, gated = false) {
  const instance = new WebAssembly.Instance(engine.module, {});
  const api = Object.fromEntries(Object.entries(instance.exports)
    .filter(([name]) => name.startsWith('ring3_abi_v1_'))
    .map(([name, value]) => [name.slice(13), value]));
  assert.equal(api.open(3, 0x464c4147, 0x54524150), 0);
  const base = api.arena_ptr() >>> 0;
  const memory = instance.exports.memory;
  const arena = () => Buffer.from(new Uint8Array(memory.buffer, base, ARENA_SIZE));
  const put = (offset, bytes) => new Uint8Array(memory.buffer).set(bytes, base + offset);
  const upload = (address, bytes) => {
    put(140, bytes);
    assert.equal(api.upload(address, bytes.length), 0);
  };
  assert.equal(api.map(PC, 1, 7), 0);
  assert.equal(api.map(0x5000, 1, 3), 0);
  const terminal = program.at(-1) === 0xcc ||
    (program.at(-2) === 0x0f && program.at(-1) === 0x0b);
  const code = Buffer.from(terminal ? program : [...program, 0xeb, 0xfe]);
  upload(PC, code);
  const layout = gated ? [PC, 2, PC + 2, 2, PC + 4, 1, PC + 5, 2, PC + 7, 1, PC + 2, 17, PC + 5, 18]
    : [PC, code.length];
  put(140, words(layout));
  let pointer, length;
  if (resident) {
    assert.equal(gated ? api.compile_resident_with_gates(5, 2) : api.compile_resident(1), 0);
    pointer = arena().readUInt32LE(156);
    length = arena().readUInt32LE(160);
  } else {
    assert.equal(gated ? api.compile_with_gates(5, 2) : api.compile(1), 0);
    pointer = api.module_ptr() >>> 0;
    length = api.module_len() >>> 0;
  }
  const module = new WebAssembly.Module(new Uint8Array(memory.buffer, pointer, length));
  let calls = 0;
  const ring3 = Object.fromEntries(WebAssembly.Module.imports(module)
    .filter(imported => imported.kind === 'function')
    .map(imported => [imported.name, (...args) => {
      if (!imported.name.startsWith('guard')) calls++;
      return api[imported.name](...args);
    }]));
  const run = new WebAssembly.Instance(module, {env: {memory}, ring3}).exports.run;
  const initial = createArena();
  [0x13572468, 0x23456789, 0x3456789a, 0x5000, 0x5000, 0x56789abc, 0x6789abcd, 0x789abcde]
    .forEach((value, index) => initial.writeUInt32LE(value, 16 + index * 4));
  initial.writeUInt32LE(PC, 48);
  put(0, initial);
  return {api, base, memory, arena, put, upload, run, resident, calls: () => calls};
}

function execute(program, options, expected) {
  for (const resident of [false, true]) {
    const c = open(program, resident);
    if (options.data) c.upload(options.address ?? 0x5000, options.data);
    const seed = c.arena();
    seed.writeUInt32LE((options.index ?? 0x13572468) >>> 0, 16);
    seed.writeUInt32LE((options.address ?? 0x5000) >>> 0, 28);
    seed.writeUInt32LE((options.stack ?? 0x5000) >>> 0, 32);
    seed.writeUInt32LE((options.flags ?? 2) >>> 0, 52);
    for (const [index, value] of options.registers ?? []) seed.writeUInt32LE(value >>> 0, 16 + index * 4);
    if (((options.flags ?? 2) & ~0xcd7) !== 0) seed.writeUInt16LE(2, 4);
    if (expected.noHelper && (expected.reason ?? 1) >= 16) {
      for (const offset of [80, 84, 88, 92]) seed.writeUInt32LE(0xdeadbeef, offset);
    }
    c.put(0, seed);
    const before = c.arena();
    const calls = c.calls();
    assert.equal(c.run(c.base, c.base + 56, options.budget ?? 1, c.base + 96), 0, options.label);
    const after = c.arena();
    const expectedState = Buffer.from(before.subarray(0, 56));
    expectedState.writeUInt32LE(expected.shadow ?? 0, 12);
    expectedState.writeUInt32LE((expected.pc ?? PC + program.length) >>> 0, 48);
    if (expected.flags !== undefined) expectedState.writeUInt32LE(expected.flags >>> 0, 52);
    if ((expectedState.readUInt32LE(52) & ~0xcd7) !== 0 || expected.shadow) expectedState.writeUInt16LE(2, 4);
    if (expected.stack !== undefined) expectedState.writeUInt32LE(expected.stack >>> 0, 32);
    for (const [index, value] of expected.registers ?? []) expectedState.writeUInt32LE(value >>> 0, 16 + index * 4);
    assert.deepEqual(after.subarray(0, 56), expectedState, `${options.label}: complete CPU state`);
    assert.equal(after.readUInt32LE(72), expected.reason ?? 1, options.label);
    assert.equal(after.readUInt32LE(76), expected.retired ?? 1, options.label);
    for (const [offset, value] of [[80, expected.detail ?? 0], [84, expected.address ?? 0],
      [88, expected.access ?? 0], [92, expected.length ?? 0]]) {
      assert.equal(after.readUInt32LE(offset), value >>> 0, `${options.label}: exit field ${offset}`);
    }
    const expectedTail = Buffer.from(before.subarray(4236));
    if (expected.work !== undefined) expectedTail.writeUInt32LE(expected.work, 4604 - 4236);
    assert.deepEqual(after.subarray(4236), expectedTail, `${options.label}: floating point, SIMD, segments and work records`);
    if (expected.noHelper) {
      assert.equal(c.calls(), calls, `${options.label}: direct Wasm path`);
      assert.deepEqual(after.subarray(100, 4236), before.subarray(100, 4236));
    }
    observations.push({label: options.label, resident, reason: after.readUInt32LE(72),
      retired: after.readUInt32LE(76), pc: after.readUInt32LE(48), flags: after.readUInt32LE(52)});
    if (expected.memory) {
      assert.equal(c.api.read8(expected.memory[0]), 0, `${options.label}: diagnostic read`);
      assert.equal(c.arena().readUInt32LE(120), expected.memory[1], `${options.label}: committed guest write`);
    }
    if (options.after) options.after(c);
    assert.equal(c.api.close(), 0);
  }
}

execute([0x0f, 0x0b], {label: 'UD2 is an unretired invalid-opcode fault'},
  {reason: 20, retired: 0, pc: PC, noHelper: true});
execute([0xcc], {label: 'INT3 is a retired breakpoint trap'}, {reason: 17, noHelper: true});
execute([0x66, 0xcc], {label: 'prefixed INT3 preserves exact next EIP'}, {reason: 17, noHelper: true});
execute([0xce], {label: 'INTO with OF clear is an ordinary instruction'}, {noHelper: true});
execute([0xce], {label: 'INTO with OF set is a retired overflow trap', flags: 0x802},
  {reason: 18, noHelper: true});

for (const [index, outside] of [[-4, true], [-3, false], [7, false], [8, true]]) {
  execute([0x62, 0x03], {label: `BOUND32 signed inclusive index ${index}`, index, data: words([-3, 7])},
    outside ? {reason: 19, retired: 0, pc: PC} : {});
}
for (const [index, outside] of [[0xabcdfffc, true], [0xabcdfffd, false], [0xabcd0007, false], [0xabcd0008, true]]) {
  execute([0x66, 0x62, 0x03], {label: `BOUND16 preserves upper EAX ${index.toString(16)}`, index, data: words([-3, 7], true)},
    outside ? {reason: 19, retired: 0, pc: PC} : {});
}
execute([0x62, 0x03], {label: 'BOUND32 signed extremes are inclusive', index: 0x80000000,
  flags: 0x8d7, data: words([0x80000000, 0x7fffffff])}, {});
execute([0x62, 0x03], {label: 'reversed bounds reject every index', index: 0, data: words([1, -1])},
  {reason: 19, retired: 0, pc: PC});
execute([0x62, 0x03], {label: 'BOUND upper memory fault precedes lower comparison', index: -1,
  address: 0x5ffc, data: words([0])},
  {reason: 5, retired: 0, pc: PC, detail: 1, address: 0x6000, access: 1, length: 8});
execute([0x66, 0x62, 0x03], {label: 'BOUND16 upper memory fault preserves state', index: -1,
  address: 0x5ffe, data: words([0], true)},
  {reason: 5, retired: 0, pc: PC, detail: 1, address: 0x6000, access: 1, length: 4});
execute([0x62, 0x03], {label: 'BOUND logical span overflow precedes RAM access', address: 0xfffffffc},
  {reason: 11, retired: 0, pc: PC, noHelper: true});

execute([0x9d], {label: 'POPFD applies user flags and ignores privileged and reserved bits',
  data: words([0xfffffeff])}, {flags: 0x244cd7, stack: 0x5004});
execute([0x66, 0x9d], {label: 'POPF preserves high EFLAGS and advances stack by two', flags: 0x240002,
  data: words([0xfeff], true)}, {flags: 0x244cd7, stack: 0x5002});
execute([0x9d], {label: 'POPFD cannot clear IF at CPL3 with IOPL0', flags: 0x202,
  data: words([0])}, {flags: 0x202, stack: 0x5004});
execute([0x66, 0x9d], {label: 'POPF cannot clear IF at CPL3 with IOPL0', flags: 0x202,
  data: words([0], true)}, {flags: 0x202, stack: 0x5002});
execute([0x9d], {label: 'POPFD stack fault preserves flags and ESP', stack: 0x5ffe, flags: 0x8d7},
  {reason: 5, retired: 0, pc: PC, detail: 1, address: 0x6000, access: 1, length: 4});
execute([0x66, 0x9d], {label: 'POPF stack fault preserves flags and ESP', stack: 0x5fff, flags: 0x8d7},
  {reason: 5, retired: 0, pc: PC, detail: 1, address: 0x6000, access: 1, length: 2});

execute([0x9d, 0x90], {label: 'POPFD enabling TF traps after the following instruction',
  data: words([0x102]), budget: 8}, {reason: 16, retired: 2, pc: PC + 2, flags: 0x102, stack: 0x5004});
execute([0x9d], {label: 'POPFD clearing TF still single-steps the POPFD instruction',
  flags: 0x102, data: words([2]), budget: 8},
  {reason: 16, retired: 1, flags: 2, stack: 0x5004});
execute([0x0f, 0x0b], {label: 'invalid opcode wins over single-step trap', flags: 0x102},
  {reason: 20, retired: 0, pc: PC, noHelper: true});
execute([0xcc], {label: 'explicit breakpoint wins over single-step trap', flags: 0x102},
  {reason: 17, noHelper: true});
execute([0xc6, 0x05, 0x07, 0x10, 0, 0, 0x41, 0x90],
  {label: 'single-step trap wins over internal executable-write invalidation', flags: 0x102, budget: 8},
  {reason: 16, pc: PC + 7, memory: [PC + 7, 0x41]});

for (const [label, program, index, flags, result, resultFlags, extra] of [
  ['ADD', [0x83, 0xc0, 1], 0, 0x244102, 1, 0x244102, []],
  ['AND', [0x83, 0xe0, 0], 0xff, 0x244102, 0, 0x244146, []],
  ['INC', [0x40], 0xffffffff, 0x244103, 0, 0x244157, []],
  ['MUL', [0xf7, 0xe1], 1, 0x244102, 0x23456789, 0x244102, [[2, 0]]],
  ['BSF', [0x0f, 0xbc, 0xc0], 8, 0x244102, 3, 0x244102, []],
  ['BT', [0x0f, 0xba, 0xe0, 0], 1, 0x244142, 1, 0x244143, []],
  ['AAA', [0x37], 9, 0x244102, 9, 0x244102, []],
  ['SHL', [0xd1, 0xe0], 1, 0x244102, 2, 0x244102, []],
  ['SHLD', [0x0f, 0xa4, 0xc8, 1], 1, 0x244102, 2, 0x244102, []],
]) {
  execute(program, {label: `${label} preserves TF, NT, AC and ID`, index, flags, budget: 8},
    {reason: 16, flags: resultFlags, registers: [[0, result], ...extra], noHelper: true});
}
execute([0xf3, 0xac], {label: 'single-step unfinished REP LODSB stops after one element',
  flags: 0x102, index: 0x12345600, registers: [[1, 2], [6, 0x5000]],
  data: Buffer.from([0x42, 0x43]), budget: 8},
  {reason: 16, retired: 0, pc: PC, registers: [[0, 0x12345642], [1, 1], [6, 0x5001]], work: 1});
execute([0xf3, 0xac], {label: 'single-step final REP LODSB retires at next EIP',
  flags: 0x102, index: 0x12345600, registers: [[1, 1], [6, 0x5000]],
  data: Buffer.from([0x42]), budget: 8},
  {reason: 16, registers: [[0, 0x12345642], [1, 0], [6, 0x5001]], work: 1});
execute([0xf3, 0xac], {label: 'single-step zero-count REP performs no memory access',
  flags: 0x102, registers: [[1, 0], [6, 0xffffffff]], budget: 8},
  {reason: 16, work: 1, noHelper: true});
execute([0xf3, 0xa6], {label: 'REP CMPSB preserves control flags during per-element comparison',
  flags: 0x244102, registers: [[1, 2], [6, 0x5000], [7, 0x5002]],
  data: Buffer.from([0x42, 0x43, 0x42, 0x44]), budget: 8},
  {reason: 16, retired: 0, pc: PC, flags: 0x244146,
    registers: [[1, 1], [6, 0x5001], [7, 0x5003]], work: 1});

execute([0x8e, 0xd0, 0x90], {label: 'MOV SS defers single-step until the following instruction',
  index: 0x23, flags: 0x102, budget: 8}, {reason: 16, retired: 2});
execute([0x17, 0x90], {label: 'POP SS defers single-step and commits ESP before the trap',
  data: words([0x23]), flags: 0x102, budget: 8}, {reason: 16, retired: 2, stack: 0x5004});
execute([0x66, 0x17, 0x90], {label: 'word POP SS retains exact stack width during debug inhibition',
  data: words([0x23], true), flags: 0x102, budget: 8}, {reason: 16, retired: 2, stack: 0x5002});
execute([0x8e, 0xd0, 0x8e, 0xd0, 0x90], {label: 'consecutive MOV SS does not extend the first inhibition',
  index: 0x23, flags: 0x102, budget: 8}, {reason: 16, retired: 2, pc: PC + 4});
execute([0x8e, 0xd0, 0x8b, 0x05, 0, 0x60, 0, 0], {
  label: 'fault in the following instruction ends SS debug inhibition', index: 0x23, flags: 0x102, budget: 8,
}, {reason: 5, retired: 1, pc: PC + 2, detail: 1, address: 0x6000, access: 1, length: 4});
execute([0x8e, 0xd0], {label: 'failed MOV SS never arms debug inhibition', index: 0, flags: 0x102, budget: 8},
  {reason: 11, retired: 0, pc: PC});
execute([0x0f, 0xb2, 0x23, 0x90], {label: 'LSS has no MOV SS debug inhibition',
  data: words([0x5010, 0x23]), flags: 0x102, budget: 8},
  {reason: 16, retired: 1, pc: PC + 3, stack: 0x5010});

for (const flags of [2, 0x102]) {
  execute([0x8e, 0xd0, 0x90], {label: `SS shadow survives budget and cancellation with flags ${flags}`,
    index: 0x23, flags, budget: 1, after(c) {
      const suspended = Buffer.from(c.arena().subarray(0, 56));
      for (const [budget, cancelled, reason] of [[0, 0, 1], [8, 1, 2]]) {
        c.put(96, words([cancelled]));
        assert.equal(c.run(c.base, c.base + 56, budget, c.base + 96), 0);
        assert.deepEqual(c.arena().subarray(0, 56), suspended, 'safepoint preserves pending SS shadow');
        assert.equal(c.arena().readUInt32LE(72), reason);
        assert.equal(c.arena().readUInt32LE(76), 0);
        observations.push({label: `pending SS shadow budget ${budget} cancelled ${cancelled}`, resident: c.resident,
          flags, reason, retired: 0, pc: PC + 2});
      }
      c.put(96, words([0]));
      assert.equal(c.run(c.base, c.base + 56, 1, c.base + 96), 0);
      const expected = Buffer.from(suspended);
      expected.writeUInt32LE(0, 12);
      expected.writeUInt32LE(PC + 3, 48);
      assert.deepEqual(c.arena().subarray(0, 56), expected, 'next instruction consumes pending SS shadow');
      assert.equal(c.arena().readUInt32LE(72), flags === 2 ? 1 : 16);
      assert.equal(c.arena().readUInt32LE(76), 1);
      observations.push({label: 'resume after budget and cancellation', resident: c.resident,
        flags, reason: c.arena().readUInt32LE(72), retired: 1, pc: PC + 3});
    }}, {retired: 1, pc: PC + 2, shadow: 1});
}

for (const resident of [false, true]) {
  for (const [version, metadata] of [[1, 1], [2, 2], [2, 0xffffffff]]) {
    const c = open([0x90], resident);
    const invalid = c.arena();
    invalid.writeUInt16LE(version, 4);
    invalid.writeUInt32LE(metadata, 12);
    c.put(0, invalid);
    assert.equal(c.run(c.base, c.base + 56, 1, c.base + 96), 2, 'invalid metadata is an ABI preflight failure');
    assert.deepEqual(c.arena(), invalid, 'invalid metadata rejection is transactional');
    observations.push({label: `invalid state metadata ${metadata} version ${version}`, resident, status: 2});
    assert.equal(c.api.close(), 0);
  }
}

{
  const c = open([0x8e, 0xd0, 0x0f, 0x0b, 0xc3, 0x0f, 0x0b, 0x90], false, true);
  c.upload(0x5010, words([PC + 7]));
  const seed = c.arena();
  seed.writeUInt16LE(2, 4);
  seed.writeUInt32LE(0x23, 16);
  seed.writeUInt32LE(0x5010, 32);
  seed.writeUInt32LE(0x102, 52);
  c.put(0, seed);
  const invoke = () => c.run(c.base, c.base + 56, 8, c.base + 96);
  assert.equal(invoke(), 0);
  assert.equal(c.arena().readUInt32LE(72), 8);
  assert.equal(c.arena().readUInt32LE(48), PC + 2);
  assert.equal(c.arena().readUInt32LE(12), 1);
  const generation = c.api.generation();
  const key = [0x464c4147, 0x54524150];
  assert.equal(c.api.capture_call(...key, generation, 1, 0), 0);
  const outerToken = c.arena().readUInt32LE(156);
  const outer = Buffer.from(c.arena().subarray(0, 96));
  for (const finish of [false, true]) {
    assert.equal(c.api.begin_callback(...key, generation, outerToken, PC + 4, PC + 5, 18, 0), 0);
    const callbackToken = c.arena().readUInt32LE(156);
    assert.equal(c.arena().readUInt32LE(12), 0, 'callback starts a fresh debug context');
    if (finish) {
      assert.equal(invoke(), 0);
      assert.equal(c.arena().readUInt32LE(72), 16, 'callback RET single-steps independently');
      assert.equal(c.arena().readUInt32LE(48), PC + 5);
      assert.equal(invoke(), 0);
      assert.equal(c.arena().readUInt32LE(72), 8);
      assert.equal(c.api.finish_callback(...key, generation, callbackToken), 0);
    } else {
      assert.equal(c.api.abort_callback(...key, callbackToken), 0);
    }
    assert.deepEqual(c.arena().subarray(0, 96), outer, 'callback restores exact suspended CPU and exit records');
    observations.push({label: `callback ${finish ? 'finish' : 'abort'} restores pending SS shadow`, resident: false, shadow: 1});
  }
  assert.equal(c.api.complete_call(...key, generation, outerToken, 42), 0);
  assert.equal(c.arena().readUInt32LE(12), 1, 'host call completion preserves hidden CPU metadata');
  assert.equal(c.arena().readUInt32LE(48), PC + 7);
  assert.equal(invoke(), 0);
  assert.equal(c.arena().readUInt32LE(72), 16);
  assert.equal(c.arena().readUInt32LE(76), 1);
  assert.equal(c.arena().readUInt32LE(48), PC + 8);
  assert.equal(c.arena().readUInt32LE(12), 0);
  assert.equal(c.arena().readUInt32LE(16), 42);
  observations.push({label: 'completed host call resumes and consumes pending SS shadow', resident: false, reason: 16, retired: 1});
  assert.equal(c.api.close(), 0);
}

assert.deepEqual(pins(), sourcePins, 'source pins remained stable during verification');
writeFileSync(join(output, 'observations.json'), JSON.stringify({engineSha256: engine.sha256,
  sourcePins, observations}, null, 2) + '\n');
console.log(JSON.stringify({cases: observations.length, engineSha256: engine.sha256,
  scope: 'actual generated Wasm; replacement and resident; flags, fault order and trap retirement'}));
