import assert from 'node:assert/strict';
import {writeFileSync, readFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {join} from 'node:path';
import {checkDefaultQueries} from '../../../../../system_registers/fixtures/p2-system-registers/runtime/default.mjs';
import {readEngine, PC, TARGET, SEG, STACK, counts, words, packet, arena, put, host, upload, configure, descriptor, cache, decodeSegments, snapshotRAM, assertRAM, open, seed, publishCode} from '../../../../../far/support/transport.mjs';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(process.env.RING3_ENGINE_SHA256, 'select an explicit engine SHA-256');
const engine = readEngine(enginePath), observations = [], modules = [];
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const sources = [
  'engine/src/abi/x86/state.rs', 'engine/src/abi/x86/mod.rs', 'engine/src/abi/x86/exit.rs', 'engine/src/cpu/exit.rs',
  'engine/src/cpu/x86/x87/execute.rs', 'engine/src/cpu/x86/control.rs', 'engine/src/cpu/x86/ir.rs', 'engine/src/cpu/x86/decode/far.rs',
  'engine/src/cpu/x86/segments/transfers.rs', 'engine/src/cpu/dbt/region.rs',
  'engine/src/cpu/dbt/wasm/abi.rs', 'engine/src/cpu/dbt/wasm/emitter.rs',
  'engine/src/cpu/dbt/wasm/far.rs', 'engine/src/cpu/dbt/wasm/far/interrupt.rs',
  'engine/src/cpu/dbt/wasm/memory.rs', 'engine/src/cpu/dbt/wasm/code_fetch.rs',
  'engine/src/cpu/dbt/wasm/integer/dispatch.rs', 'engine/src/cpu/dbt/wasm/simd.rs',
  'engine/src/cpu/dbt/wasm/software_interrupts.rs', 'engine/src/cpu/dbt/wasm/detection.rs',
  'engine/src/abi/x86/event.rs', 'engine/src/abi/wasm/exports/events.rs',
  'engine/src/process/cpu/event.rs', 'engine/src/process/wasm/detection.rs',
  'engine/src/cpu/dbt/wasm/x87.rs', 'engine/src/process/cpu/x87.rs', 'engine/src/process/cpu/avx.rs',
  'engine/src/process/cpu/far/return.rs', 'engine/src/process/calls/context.rs',
  'engine/src/cpu/x86/system_registers.rs', 'engine/src/process/cpu/system_registers.rs',
  'engine/tests/cpu/control/system_registers/fixtures/p2-system-registers/runtime/default.mjs',
  'engine/tests/support/wasm/arena.mjs', 'engine/tests/support/wasm/engine.mjs',
  'engine/tests/cpu/control/far/support/transport.mjs',
  'engine/tests/cpu/control/interrupt/return/wasm.rs',
  'engine/tests/cpu/control/interrupt/return/fixtures/p2-iret/runtime/run.mjs',
];
const pins = () => Object.fromEntries(sources.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = pins(), OLD_FLAGS = 0x250ed7;

function compile(c, bytes, targetBytes) {
  upload(c, PC, bytes);
  if (targetBytes) upload(c, TARGET, targetBytes);
  put(c, 140, words(c.entries ? [PC, ...(targetBytes ? [TARGET] : [])] : [PC, bytes.length, ...(targetBytes ? [TARGET, targetBytes.length] : [])]));
  host(c, c.resident ? c.entries ? 'compile_resident_entries' : 'compile_resident' : c.entries ? 'compile_entries' : 'compile', [targetBytes ? 2 : 1, ...(c.entries ? [0] : [])]);
  const a = arena(c), binding = c.resident ? [c.key, 0x464152, a.readUInt32LE(148), a.readUInt32LE(152)] : [];
  c.binding = binding;
  const at = c.resident ? a.readUInt32LE(156) : c.api.module_ptr() >>> 0;
  const length = c.resident ? a.readUInt32LE(160) : c.api.module_len() >>> 0;
  const bytesOut = Buffer.from(new Uint8Array(c.memory.buffer, at, length)), module = new WebAssembly.Module(bytesOut);
  assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  const imports = WebAssembly.Module.imports(module), ring3 = {...c.api};
  for (const item of imports.filter(item => item.module === 'ring3' && !item.name.startsWith('guard'))) {
    ring3[item.name] = (...args) => {
      const values = args.map(value => typeof value === 'bigint' ? Number(value) : value >>> 0);
      (item.name === 'finish_exception_detection' ? c.detectorCalls : c.calls).push([item.name, values]);
      return c.api[item.name](...args);
    };
  }
  c.run = new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3}).exports.run;
  assert.equal(c.run.length, 4);
  const file = `module-${modules.length + 1}.wasm`; writeFileSync(join(output, file), bytesOut, {flag: 'wx'});
  modules.push({context: c.key, resident: c.resident, entries: c.entries, file, bytes: length, sha256: hash(bytesOut), imports});
}
function input(c, options = {}) {
  const b = seed({esp: options.esp ?? STACK, flags: options.flags ?? OLD_FLAGS, cancel: options.cancel ?? 0, context: options.context});
  b.writeUInt16LE(3, 4); b.writeUInt32LE(options.shadow ?? 0, 12); b.writeUInt32LE(options.pc ?? PC, 48);
  assert.equal(b.length, 4796); return b;
}
function exit(b, reason, retired = 0, detail = 0, address = 0, access = 0, length = 0, version = 6) {
  b.set(packet('R3EX', 40, [reason, retired, detail, address, access, length], version), 56);
}
function check(c, name, before, expected, expectedCalls, {budget = 1, writes = [], restart} = {}) {
  const ram = snapshotRAM(c); put(c, 0, before); c.calls = []; c.detectorCalls = [];
  for (const {address, bytes} of writes) for (const [index, value] of bytes.entries()) {
    const at = (address + index) >>> 0, page = (at & 0xfffff000) >>> 0;
    assert.ok(ram.has(page), 'literal expected write targets a mapped page'); ram.get(page)[at & 4095] = value;
  }
  const status = c.run(c.base, c.base + 56, budget, c.base + 96), after = arena(c), errors = [];
  for (const [label, test] of [
    ['run status', () => assert.equal(status, 0)],
    ['complete arena', () => assert.deepEqual(after, expected)],
    ['complete mapped RAM', () => assertRAM(c, ram, name)],
    ['ordered helpers and operands', () => assert.deepEqual(c.calls, expectedCalls)],
    ['precise stop is certified once', () => {
      const reason = expected.readUInt32LE(72), terminal = reason === 3 || [11, 12, 14, 15, 16, 17, 18, 21, 23, 24].includes(reason);
      assert.equal(c.detectorCalls.length, Number(terminal));
      if (terminal) {
        const [name, values] = c.detectorCalls[0]; assert.equal(name, 'finish_exception_detection');
        assert.ok(Number.isSafeInteger(values[0]) && values[0] > 0);
        assert.deepEqual(values.slice(1, 6), [72, 80, 84, 88, 92].map(at => expected.readUInt32LE(at)));
        if (restart) assert.deepEqual(values.slice(6), restart, 'original instruction restart record');
      }
    }],
  ]) {try {test();} catch (error) {errors.push({label, message: error.message});}}
  const file = `arena-${observations.length + 1}.bin`; writeFileSync(join(output, file), after, {flag: 'wx'});
  observations.push({context: c.key, resident: c.resident, entries: c.entries, name, passed: !errors.length, errors, status,
    actual: {reason: after.readUInt32LE(72), retired: after.readUInt32LE(76), pc: after.readUInt32LE(48), flags: after.readUInt32LE(52)},
    helpers: c.calls, detection: c.detectorCalls, arena: {file, sha256: hash(after)}, mapped_ram: [...ram].map(([address, bytes]) => ({address, bytes: bytes.length, sha256: hash(bytes)}))});
}
function checkEventHandoff(c, name, vector, original, recordControls) {
  const stopped = arena(c), ram = snapshotRAM(c);
  const mutations = [['changed EIP', 48, 0x3000]];
  if (recordControls) mutations.push(['invalid work magic', 4588, 0],
    ['reserved work metadata', 4608, 1], ['invalid x87 record', 4236, 0]);
  for (const [label, offset, value] of mutations) {
    const changed = Buffer.from(stopped); changed.writeUInt32LE(value, offset); put(c, 0, changed);
    assert.equal(c.api.take_cpu_event(), 15, `${name}/${label}: changed context rejects event take`);
    assert.deepEqual(arena(c), changed, `${name}/${label}: rejected take changes no arena byte`);
    assertRAM(c, ram, `${name}/${label}: rejected take changes no RAM`);
  }
  put(c, 0, stopped);
  const metadata = packet('R3CE', 48, [vector, PC, stopped.readUInt32LE(48), 0,
    original.readUInt32LE(52), original.readUInt32LE(12), 0xffffffff, 1], 1);
  const expected = Buffer.from(stopped); expected.set(metadata, 140);
  assert.equal(c.api.take_cpu_event(), 0, `${name}: restored event takes once`);
  assert.deepEqual(arena(c), expected, `${name}: only exact 48-byte event metadata is published`);
  assertRAM(c, ram, `${name}: event take changes no RAM`);
  assert.equal(c.api.take_cpu_event(), 7, `${name}: second take rejects consumed event`);
  assert.deepEqual(arena(c), expected, `${name}: second take changes no arena byte`);
  assertRAM(c, ram, `${name}: second take changes no RAM`);
}
function frame(c, name, {word, popped = 0x003f7fd7, selector = 0x43, target = word ? 0xabcd2000 : TARGET, esp = STACK,
  flags = OLD_FLAGS, shadow = 0, budget = 1, cancel = 0, raw = descriptor(), faultSlot, context, nt = false, csDenied = false, cachedDenied = false, refresh = false} = {}) {
  const slot = word ? 2 : 4, values = [target, (0xa5a50000 | selector) >>> 0, popped], chunks = values.map(value => words([value]).subarray(0, slot));
  configure(c, 0x43, raw);
  for (let i = 0; i < (faultSlot ?? 3); i++) upload(c, (esp + i * slot) >>> 0, chunks[i]);
  const b = input(c, {esp, flags, shadow, cancel, context}), want = Buffer.from(b), calls = [], stopped = cancel || budget === 0;
  if (stopped) exit(want, cancel ? 2 : 1);
  else if (nt) {
    // This virtual Windows process has no nested hardware task chain: its TSS
    // backlink is null. IRET faults before touching the ordinary return frame.
    want.writeUInt32LE(flags & ~0x10000, 52); want.writeUInt32LE(0, 12);
    exit(want, 21, 0, 0, 0, 0, 0, 10);
  }
  else if (csDenied) exit(want, 11);
  else if (cachedDenied) {want.writeUInt32LE(flags & ~0x10000, 52); want.writeUInt32LE(0, 12); exit(want, 15);}
  else {
    want.writeUInt32LE(flags & ~0x10000, 52); want.writeUInt32LE(0, 12);
    for (let i = 0; i < 3; i++) {
      const address = (esp + i * slot) >>> 0; calls.push(['cpu_read', [address, slot, 1]]);
      if (i === faultSlot) {
        want.set(packet('R3MH', 40, [9, 0, 4, address, 1, slot], 15), 100); exit(want, 23, 0, 4, address, 1, slot, 12); break;
      }
      want.set(chunks[i], 140);
    }
    if (faultSlot === undefined) {
      const ip = word ? target & 0xffff : target, helper = c.resident ? 'far_return_resident' : 'far_return';
      calls.push([helper, [...c.binding, ip, selector, Number(word)]]);
      const status = selector === 0 ? 3 : raw.present ? 0 : 6, detail = selector === 0 ? 0 : status ? 0x40 : 0;
      want.set(packet('R3MH', 40, [status, status ? 0 : ip, detail, 0, 0, 0], 7), 100);
      if (status) exit(want, status === 6 ? 14 : 11, 0, detail);
      else {
        publishCode(want, raw, selector, ip); want.writeUInt32LE((esp + 3 * slot) >>> 0, 32);
        // independent literal flag outcomes; no product mask or transition helper is imported.
        const result = popped === 0 ? word ? 0x240202 : 0x202 : word ? 0x244fd7 : 0x254fd7;
        want.writeUInt32LE(result, 52); exit(want, flags & 0x100 ? 16 : budget > 1 ? 3 : 1, 1, 0, 0, 0, 0, flags & 0x100 ? 7 : 6);
        if (refresh) {want.writeUInt32LE(TARGET + 1, 48); want.writeUInt32LE(result & ~0x10000, 52); exit(want, 11, 2);}
      }
    }
  }
  check(c, name, b, want, calls, {budget});
}
for (const resident of [false, true]) for (const entries of [false, true]) for (const word of [false, true]) {
  const c = open(engine, resident, entries); compile(c, Buffer.from(word ? '66cf' : 'cf', 'hex'));
  frame(c, 'all-flags-VM-IOPL-ignored-padding-discarded', {word});
  frame(c, 'zero-image-preserves-IF-and-word-high-flags', {word, popped: 0});
  frame(c, 'budget-zero-preserves-RF-shadow-and-frame', {word, budget: 0, shadow: 1});
  frame(c, 'cancel-preserves-RF-shadow-and-frame', {word, cancel: 1, shadow: 1});
  frame(c, 'current-NT-null-backlink-TS-before-frame', {word, flags: OLD_FLAGS | 0x4000, shadow: 1, nt: true});
  frame(c, 'current-NT-TS-precedes-unmapped-stack-and-old-TF',
    {word, flags: OLD_FLAGS | 0x4100, esp: 0x9000, faultSlot: 0, shadow: 1, nt: true});
  frame(c, 'current-NT-zero-budget-preserves-RF-shadow-and-invalid-stack',
    {word, flags: OLD_FLAGS | 0x4000, esp: 0x9000, faultSlot: 0, shadow: 1, budget: 0, nt: true});
  frame(c, 'current-NT-cancel-preserves-RF-shadow-and-invalid-stack',
    {word, flags: OLD_FLAGS | 0x4000, esp: 0x9000, faultSlot: 0, shadow: 1, cancel: 1, nt: true});
  frame(c, 'null-CS-after-complete-frame', {word, selector: 0});
  frame(c, 'absent-CS-after-complete-frame', {word, raw: descriptor({present: false})});
  for (const faultSlot of [0, 1, 2]) frame(c, `stack-slot-${faultSlot}-fault-consumes-RF`, {word, faultSlot, esp: 0x8000 - faultSlot * (word ? 2 : 4), shadow: 1});
  frame(c, 'third-slot-fault-precedes-invalid-CS', {word, faultSlot: 2, esp: 0x8000 - 2 * (word ? 2 : 4), selector: 0});
  const context = decodeSegments(seed().subarray(SEG)); context.caches[1] = 0xf2b00fff;
  frame(c, 'CS-fault-before-RF-start-and-stack', {word, context, shadow: 1, csDenied: true});
  if (!resident && !entries) {
    const limitedStack = decodeSegments(seed().subarray(SEG));
    limitedStack.caches[2] = cache(descriptor({type: 2, limit: STACK + 2 * (word ? 2 : 4) - 1, pages: false}));
    frame(c, 'third-cached-SS-slot-denies-before-page-reads', {word, context: limitedStack, cachedDenied: true, shadow: 1});
  }
  frame(c, 'old-TF-traps-after-complete-return', {word, flags: OLD_FLAGS | 0x100});
  frame(c, 'loaded-RF-survives-cold-target-NeedCode', {word, budget: 2});
  host(c, 'close');
}
checkDefaultQueries(engine, {compile, input, exit, check});
// User software interrupts cross the OS boundary without constructing a kernel
// interrupt frame. INT4 is unconditional, unlike INTO. Windows entry vectors
// return a typed request for the OS provider; an unexposed vector faults.
for (const resident of [false, true]) for (const entries of [false, true]) {
  for (const prefix of [[], [0x66]]) for (const vector of [3, 4, 0x29, 0x2e, 0x80]) {
    const c = open(engine, resident, entries), bytes = Buffer.from([...prefix, 0xcd, vector]);
    compile(c, bytes);
    const cases = [
      ['attempt-consumes-RF-and-SS-shadow', {flags: OLD_FLAGS, shadow: 1}],
      ['old-TF-cannot-replace-entry', {flags: OLD_FLAGS | 0x100, shadow: 0, budget: 3}],
      ['budget-zero', {flags: OLD_FLAGS | 0x100, shadow: 1, budget: 0}],
      ['cancel', {flags: OLD_FLAGS | 0x100, shadow: 1, cancel: 1}],
    ];
    if (vector === 4) cases.push(['OF-clear-still-traps', {flags: OLD_FLAGS & ~0x800, shadow: 0}]);
    for (const [name, options] of cases) {
      const b = input(c, {esp: 0x9000, ...options}), want = Buffer.from(b);
      let restart;
      if (options.cancel || options.budget === 0) {
        // trap-only int3 modules retain the base exit record until a trap occurs.
        exit(want, options.cancel ? 2 : 1, 0, 0, 0, 0, 0, vector === 3 ? 1 : 6);
      }
      else {
        want.writeUInt32LE(options.flags & ~0x10000, 52); want.writeUInt32LE(0, 12);
        if ([3, 4, 0x29, 0x2e].includes(vector)) {
          want.writeUInt32LE(PC + bytes.length, 48);
          const systemEntry = vector === 0x29 || vector === 0x2e;
          exit(want, systemEntry ? 24 : vector === 3 ? 17 : 18, 1, systemEntry ? vector : 0, 0, 0, 0, systemEntry ? 14 : 7);
          restart = [0, PC, 0, options.flags, options.shadow, 0];
        } else exit(want, 11, 0, 0x402);
      }
      check(c, `software-INT-${vector}-${prefix.length ? 'word-prefix' : 'bare'}-${name}-without-stack`, b, want, [], {...options, restart});
      if (restart && name === 'attempt-consumes-RF-and-SS-shadow') {
        checkEventHandoff(c, `software-INT-${vector}-${prefix.length ? 'word-prefix' : 'bare'}`, vector, b,
          !resident && !entries && prefix.length === 0 && vector === 3);
      }
    }
    host(c, 'close');
  }
}
// capture both target instructions before narrowing live source and destination limits.
for (const resident of [false, true]) for (const entries of [false, true]) for (const word of [false, true]) {
  const c = open(engine, resident, entries), code = Buffer.from(word ? '66cf' : 'cf', 'hex');
  compile(c, code, Buffer.from('9090ebfe', 'hex'));
  const context = decodeSegments(seed().subarray(SEG));
  context.caches[1] = cache(descriptor({limit: PC + code.length - 1, pages: false}));
  frame(c, 'bounded-return-refreshes-CS-before-captured-target-fetch', {word, popped: 0, context,
    raw: descriptor({limit: TARGET, pages: false}), budget: 4, refresh: true});
  host(c, 'close');
}
// ordinary start and pre-execution faults must disagree about consuming RF.
for (const resident of [false, true]) {
  const c = open(engine, resident, false); compile(c, Buffer.from('90ebfe', 'hex'));
  for (const [name, options, reason, retired, consume, version] of [
    ['next-NOP-consumes-RF', {}, 1, 1, true, 1],
    ['unmatched-PC-NeedCode-preserves-RF', {pc: 0x3000, shadow: 1}, 3, 0, false, 1],
    ['zero-budget-preserves-RF', {budget: 0, shadow: 1}, 1, 0, false, 1],
  ]) {
    const b = input(c, options), want = Buffer.from(b);
    if (consume) {want.writeUInt32LE(OLD_FLAGS & ~0x10000, 52); want.writeUInt32LE(0, 12); want.writeUInt32LE(PC + 1, 48);}
    exit(want, reason, retired, 0, 0, 0, 0, version); check(c, name, b, want, [], options);
  }
  host(c, 'close');
}
for (const [name, bytes, mmx] of [['waiting-x87-pending-MF-preserves-RF', 'd9e8ebfe', false], ['MMX-pending-MF-preserves-RF', '0fefc0ebfe', true]]) {
  const c = open(engine, false, false); compile(c, Buffer.from(bytes, 'hex'));
  const b = input(c, {shadow: 1}), want = Buffer.from(b), calls = [];
  b.writeUInt16LE(0x037e, 4236 + 16); b.writeUInt16LE(0x8081, 4236 + 18); want.set(b);
  if (!mmx) {want.set(packet('R3MH', 40, [4, 0, 0x8081, 0, 0, 0], 7), 100); calls.push(['x87_op', [22, 0, OLD_FLAGS, PC, 0x1e8, 1]]);}
  exit(want, 12, 0, 0x8081); check(c, name, b, want, calls); host(c, 'close');
}
assert.deepEqual(pins(), sourcePins, 'source and independent oracles stayed fixed');
const failed = observations.filter(row => !row.passed);
writeFileSync(join(output, 'result.json'), JSON.stringify({status: failed.length ? 'FAIL' : 'PASS', engine: {path: enginePath, sha256: engine.sha256}, source_pins: sourcePins,
  counts: {contexts: counts.contexts, modules: modules.length, runs: observations.length, passed: observations.length - failed.length, failed: failed.length}, modules, observations,
  limits: ['same-CPL NT0 IRET16/32, base-zero CS.D32/SS.B32 including bounded live-CS refresh; NT1 faults with TS0 under the fixed null-backlink process environment',
    'live RF lifetime and pre-handler state, no IDT/NMI/DR-handler frame or hardware-debug delivery claim',
    'INT3/INT4 and Windows INT29/INT2E boundary stops across four compilation routes; complete arena/RAM plus original instruction metadata; no OS service or SEH delivery claim',
    'finite four-profile return/frame/fault/TF coverage and focused ordinary/FP RF checks; no exhaustive ISA, browser, game or throughput claim'],
}, null, 2) + '\n');
console.log(JSON.stringify({status: failed.length ? 'FAIL' : 'PASS', contexts: counts.contexts, modules: modules.length, runs: observations.length, failed: failed.length, output}));
assert.equal(failed.length, 0, failed.map(row => `${row.context}/${row.name}: ${row.errors.map(error => error.label).join(', ')}`).join('\n'));
