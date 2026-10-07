import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {performance} from 'node:perf_hooks';
import {execFileSync} from 'node:child_process';
import {readEngine} from '../support/engine.mjs';
import {batch, checksum} from './oracle.mjs';

const [enginePath, output, root, campaign] = process.argv.slice(2);
assert.ok(campaign && root);
const engine = readEngine(enginePath);
const image = readFileSync(join(output, 'sort.exe'));
const spec = JSON.parse(readFileSync(join(output, 'manifest.json'), 'utf8'));
const expected = batch(spec.seed, spec.words);
assert.equal(spec.seed, 0x6d2b79f5); assert.equal(spec.words, 256); assert.equal(spec.batches, 10);
assert.equal(expected.checksum, 2003890592);
const inversions = expected.input.reduce((sum, value, i, all) => sum + all.slice(i + 1).filter(other => value > other).length, 0);
const recordMinima = expected.input.slice(1).filter((value, i) => value < Math.min(...expected.input.slice(0, i + 1))).length;
const census = {inversions, record_minima: recordMinima, fill: 3332, checksum: 1284, sort: 2807 + 8 * inversions - 3 * recordMinima, main_cycle: 17};
census.cycle = census.fill + census.checksum + census.sort + census.main_cycle;
census.program = 10 * census.cycle + 2;
const SIZE = 4236, TRANSFER = 140, BASE = spec.actual_base, TEXT = BASE + spec.text_rva;
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const artifacts = {}, contexts = [];
const save = (name, bytes) => { writeFileSync(join(output, name), bytes, {flag: 'wx'}); artifacts[name] = {bytes: Buffer.byteLength(bytes), sha256: hash(bytes)}; };
const sourcePaths = [...execFileSync('rg', ['--files', 'engine/src'], {cwd: root, encoding: 'utf8'}).trim().split('\n'), 'Cargo.toml', 'Cargo.lock', 'engine/Cargo.toml', 'engine/tests/support/pe32.rs', 'engine/tests/support/pe32_sort.rs', 'engine/tests/pe32_sort.rs', 'engine/tests/pe32_sort_wasm.rs', 'engine/tests/fixtures/support/engine.mjs', ...['oracle.mjs', 'oracle.test.mjs', 'run.mjs'].map(p => `engine/tests/fixtures/p2-pe32-sort/${p}`)].sort();
const sourceMap = () => Object.fromEntries(sourcePaths.map(p => { const b = readFileSync(join(root, p)); return [p, {bytes: b.length, sha256: hash(b)}]; }));
const sourcesBefore = sourceMap();
save('sources-before.json', JSON.stringify(sourcesBefore, null, 2));
assert.deepEqual(image.subarray(spec.text_raw, spec.text_raw + spec.program_hex.length / 2), Buffer.from(spec.program_hex, 'hex'));
assert.equal(image.length, 2560);
const names = ['open', 'close', 'arena_ptr', 'begin_image_input', 'append_image_input', 'load_pe32_linked_v5_input_at', 'start_loaded_image', 'dispatcher_module', 'guard_dispatch_entry', 'find_installed_resident', 'compile_resident_entries', 'acknowledge_resident_installation', 'guard_resident', 'read32', 'store_resident32', 'capture_resident_call', 'complete_resident_windows_call', 'upload', 'protect', 'retire_stale_resident'];
function refresh(e) { e.view = new DataView(e.memory.buffer); return e.view; }
function arena(e) { return Buffer.from(e.memory.buffer, e.base, SIZE).subarray().map(x => x); }
function cpu(e) { return arena(e).subarray(0, 56); }
function readCpu(e) { const v = refresh(e); return {registers: Array.from({length: 8}, (_, i) => v.getUint32(e.base + 16 + i * 4, true)), pc: v.getUint32(e.base + 48, true), flags: v.getUint32(e.base + 52, true)}; }
function exit(e) { const v = refresh(e); return {version: v.getUint16(e.base + 60, true), reason: v.getUint32(e.base + 72, true), retired: v.getUint32(e.base + 76, true), detail: v.getUint32(e.base + 80, true), address: v.getUint32(e.base + 84, true), access: v.getUint32(e.base + 88, true), length: v.getUint32(e.base + 92, true)}; }
function allowed(before, after, spans, label) { for (let i = 0; i < SIZE; i++) if (!spans.some(([a,b]) => i >= a && i < b)) assert.equal(after[i], before[i], `${label}: undeclared arena byte${i}`); }
function work(e, kind, fn) { const start = performance.now(); const result = fn(); const ended = performance.now(); e.work[kind] = (e.work[kind] ?? 0) + ended - start; e.lastWorkEnded = ended; return result; }
function status(e, kind, fn, wanted = 0, spans = []) { const before = arena(e); const result = work(e, kind, fn); assert.equal(result, wanted, kind); allowed(before, arena(e), spans, kind); return result; }
function transfer(e, bytes) { assert.ok(bytes.length <= 4096); new Uint8Array(e.memory.buffer).set(bytes, e.base + TRANSFER); }
function words(values) { const b = Buffer.alloc(values.length * 4); values.forEach((value,i) => b.writeUInt32LE(value >>> 0, i * 4)); return b; }
function readWord(e, address, fault = false) {
  const before = arena(e); assert.equal(e.api.read32(address), 0); const v = refresh(e), fields = Array.from({length: 6}, (_, i) => v.getUint32(e.base + 116 + i * 4, true));
  assert.deepEqual(fields, fault ? [1, 0, 1, address, 1, 4] : [0, fields[1], 0, 0, 0, 0]); allowed(before, arena(e), [[100, 140]], 'typed diagnostic read'); e.reads++;
  return fields[1];
}
function readPage(e, address) { const page = Buffer.alloc(4096); for (let i = 0; i < 1024; i++) page.writeUInt32LE(readWord(e, address + i * 4), i * 4); return page; }
function snapshot(e, label) { const b = arena(e); const name = `${e.label}-${label}-arena.bin`; save(name, b); return {file: name, cpu: readCpu(e), exit: exit(e), sha256: hash(b)}; }
function fresh(label, program = image) {
  const started = performance.now(), instance = new WebAssembly.Instance(engine.module, {}), instanceMs = performance.now() - started;
  const api = Object.fromEntries(names.map(name => [name, instance.exports[`ring3_abi_v1_${name}`]]));
  for (const [name, fn] of Object.entries(api)) assert.equal(typeof fn, 'function', name);
  const e = {label, instance, instanceMs, api, memory: instance.exports.memory, low: 0xc4350000 + contexts.length + 1, high: 0xa4350000, work: {}, units: [], groups: new Map(), reads: 0, retired: 0, allocations: 0, frees: 0, captures: 0, completions: 0, batches: [], cold: [], controls: [], table: new WebAssembly.Table({element: 'anyfunc', initial: 8, maximum: 8})};
  assert.equal(api.open(8, e.low, e.high), 0); e.base = api.arena_ptr() >>> 0;
  status(e, 'admission', () => api.begin_image_input(program.length)); transfer(e, program);
  status(e, 'admission', () => api.append_image_input(0, program.length));
  transfer(e, Buffer.alloc(4096, 0xa5));
  status(e, 'admission', () => api.load_pe32_linked_v5_input_at(BASE, spec.gate_base), 0, [[TRANSFER, TRANSFER + 96]]);
  const v = refresh(e); assert.equal(v.getUint32(e.base + TRANSFER, true), 0x494c3352); assert.equal(v.getUint16(e.base + TRANSFER + 4, true), 5); assert.equal(v.getUint32(e.base + TRANSFER + 8, true), 96);
  assert.equal(v.getUint32(e.base + TRANSFER + 16, true), BASE); assert.equal(v.getUint32(e.base + TRANSFER + 24, true), TEXT); assert.equal(v.getUint32(e.base + TRANSFER + 36, true), 3);
  e.gates = Array.from({length: 3}, (_, i) => [v.getUint32(e.base + TRANSFER + 48 + i * 8, true), v.getUint32(e.base + TRANSFER + 52 + i * 8, true)]);
  assert.deepEqual(e.gates, [[spec.gate_base + 32, 0x10003], [spec.gate_base + 64, 0x10005], [spec.gate_base + 80, 0x10006]]);
  e.plan = [...spec.groups.map(g => ({name: g.name, entries: g.offsets.map(p => TEXT + p), gates: []})), {name: 'gates', entries: e.gates.map(([pc]) => pc), gates: e.gates}];
  for (const [i, g] of spec.imports.entries()) assert.equal(readWord(e, BASE + g.iat_rva), e.gates[i][0]);
  status(e, 'startup', () => api.start_loaded_image(spec.stack_base, 1), 0, [[0, 96]]);
  assert.deepEqual(readCpu(e), {registers: [0,0,0,0,spec.stack_base + 4096,0,0,0], pc: TEXT, flags: 2});
  status(e, 'dispatcher', () => api.dispatcher_module(e.low, e.high), 0, [[TRANSFER, TRANSFER + 32]]);
  const dp = refresh(e), pointer = dp.getUint32(e.base + TRANSFER + 24, true), length = dp.getUint32(e.base + TRANSFER + 28, true);
  const bytes = Buffer.from(e.memory.buffer, pointer, length).map(x => x); save(`${label}-dispatcher.wasm`, bytes);
  e.dispatcher = work(e, 'dispatcher_bind', () => new WebAssembly.Instance(new WebAssembly.Module(bytes), {env: {memory: e.memory, table: e.table}, ring3: {guard_dispatch_entry: api.guard_dispatch_entry, find_installed_resident: api.find_installed_resident}}).exports.run);
  assert.equal(e.dispatcher.length, 4);
  contexts.push(e); return e;
}
function compile(e, group, observedPc) {
  assert.ok(group.entries.includes(observedPc), 'actual NeedCode selects an authored group'); assert.ok(!e.groups.has(group.name));
  const request = words([...group.entries, ...group.gates.flat()]);
  assert.ok(request.length <= 4096);
  work(e, 'resident_request_copy', () => new Uint8Array(e.memory.buffer).set(request, e.base + TRANSFER));
  status(e, 'resident_compile', () => e.api.compile_resident_entries(group.entries.length, group.gates.length), 0, [[TRANSFER, TRANSFER + 24]]);
  const v = refresh(e); assert.equal(v.getUint32(e.base + TRANSFER, true), 1); assert.equal(v.getUint32(e.base + TRANSFER + 4, true), 24);
  const slot = Array.from({length: 8}, (_, i) => i).find(i => !e.units.some(u => u.active && u.slot === i));
  const unit = {name: group.name, low: v.getUint32(e.base + TRANSFER + 8, true), high: v.getUint32(e.base + TRANSFER + 12, true), pointer: v.getUint32(e.base + TRANSFER + 16, true), length: v.getUint32(e.base + TRANSFER + 20, true), slot, active: true, observed_pc: observedPc, entries: group.entries};
  assert.ok(slot !== undefined && unit.pointer > 0 && unit.length > 8 && unit.pointer + unit.length <= e.memory.buffer.byteLength);
  unit.bytes = work(e, 'resident_module_copy', () => Buffer.from(e.memory.buffer, unit.pointer, unit.length).map(x => x));
  const module = work(e, 'resident_module', () => new WebAssembly.Module(unit.bytes)); unit.imports = WebAssembly.Module.imports(module);
  const imports = Object.fromEntries(unit.imports.filter(x => x.module === 'ring3').map(x => [x.name, e.api[x.name]]));
  unit.run = work(e, 'resident_instance', () => new WebAssembly.Instance(module, {env: {memory: e.memory}, ring3: imports})).exports.run;
  work(e, 'resident_table_set', () => e.table.set(unit.slot, unit.run));
  status(e, 'resident_ack', () => e.api.acknowledge_resident_installation(e.low, e.high, unit.low, unit.high, unit.slot), 0, [[TRANSFER, TRANSFER + 32]]);
  assert.ok(unit.slot < 8); assert.equal(e.table.get(unit.slot), unit.run); assert.equal(unit.run.length, 4);
  const allowedImports = ['guard_resident', 'read32', 'store_resident32'];
  assert.ok(unit.imports.every(x => (x.module === 'env' && x.name === 'memory' && x.kind === 'memory') || (x.module === 'ring3' && allowedImports.includes(x.name) && x.kind === 'function')));
  save(`${e.label}-unit-${unit.low}-${unit.name}.wasm`, unit.bytes);
  e.units.push(unit); e.groups.set(group.name, unit); e.cold.push({pc: observedPc, name: group.name, entries: group.entries, id: [unit.low, unit.high], slot: unit.slot, bytes: unit.length, sha256: hash(unit.bytes)});
}

const totalWork = e => Object.values(e.work).reduce((sum, n) => sum + n, 0);
function dispatch(e, budget) {
  const before = arena(e), result = work(e, 'dispatch', () => e.dispatcher(e.base, e.base + 56, budget, e.base + 96));
  assert.equal(result, 0, 'actual returning dispatcher'); allowed(before, arena(e), [[0,96], [100,172]], 'dispatch');
  const stop = exit(e); assert.ok(stop.retired <= budget); e.retired += stop.retired;
  return stop;
}
function capture(e, stop) {
  const v = refresh(e), at = e.base + TRANSFER;
  assert.equal(v.getUint32(at, true), 0x4e493352); assert.equal(v.getUint16(at + 4, true), 1); assert.equal(v.getUint32(at + 8, true), 32);
  const owner = e.groups.get('gates'); assert.ok(owner);
  assert.deepEqual([v.getUint32(at + 16, true), v.getUint32(at + 20, true), v.getUint32(at + 24, true)], [owner.low, owner.high, owner.slot]);
  assert.equal(e.table.get(owner.slot), owner.run);
  const count = stop.detail === 0x10005 ? 4 : stop.detail === 0x10006 ? 3 : stop.detail === 0x10003 ? 1 : assert.fail('unknown named gate');
  const c = readCpu(e), returnPc = TEXT + (count === 4 ? 20 : count === 3 ? 62 : 86);
  status(e, 'capture', () => e.api.capture_resident_call(e.low, e.high, owner.low, owner.high, 2, count), 0, [[TRANSFER, TRANSFER + 112]]);
  const record = Buffer.from(arena(e).subarray(TRANSFER, TRANSFER + 112)), r = new DataView(record.buffer, record.byteOffset, record.byteLength);
  assert.equal(record.subarray(0,4).toString(), 'R3CF'); assert.equal(r.getUint16(4, true), 1); assert.equal(r.getUint32(8, true), 112);
  const token = r.getUint32(16, true), args = Array.from({length: count}, (_, i) => r.getUint32(48 + i * 4, true));
  assert.equal(token, ++e.captures);
  assert.deepEqual(Array.from({length: 7}, (_, i) => r.getUint32(20 + i * 4, true)), [stop.detail, 2, count, c.pc, c.registers[4], returnPc, 0]);
  assert.ok(record.subarray(48 + count * 4).every(x => x === 0));
  assert.equal(c.registers[4], spec.stack_base + 4096 - (count + 1) * 4);
  assert.equal(readWord(e, c.registers[4]), returnPc);
  args.forEach((arg, i) => assert.equal(readWord(e, c.registers[4] + 4 + i * 4), arg));
  return {owner, token, args, record, cpu: c, returnPc};
}
function complete(e, frame, terminal = false) {
  status(e, 'provider', () => e.api.complete_resident_windows_call(e.low, e.high, frame.owner.low, frame.owner.high, frame.token), 0, [[0,96]]);
  e.lastProviderEnd = e.lastWorkEnded;
  e.completions++;
  const stop = exit(e), c = readCpu(e), registers = [...frame.cpu.registers];
  if (terminal) { assert.equal(stop.reason, 9); assert.equal(stop.detail, frame.args[0]); assert.deepEqual(c, frame.cpu); }
  else { registers[0] = frame.args.length === 4 ? c.registers[0] : 1; registers[4] = spec.stack_base + 4096; assert.deepEqual(c, {registers, pc: frame.returnPc, flags: frame.cpu.flags}); assert.equal(stop.reason, 3); }
  assert.equal(stop.retired, 0);
}
function validatePage(e, index) {
  const page = readPage(e, e.allocation), values = Array.from({length: spec.words}, (_, i) => page.readUInt32LE(i * 4));
  save(`${e.label}-batch-${index}-page.bin`, page);
  assert.deepEqual(values, expected.sorted, 'full unsigned sorted array'); assert.ok(page.subarray(spec.words * 4).every(x => x === 0), 'complete mapped-page suffix');
  assert.equal(checksum(values), expected.checksum); assert.equal(readWord(e, BASE + spec.checksum_rva), expected.checksum);
  assert.equal(readWord(e, BASE + spec.aggregate_rva), Number(BigInt.asUintN(32, BigInt(expected.checksum) * BigInt(index))));
  assert.equal(readWord(e, BASE + spec.counter_rva), spec.batches - index + 1);
  return hash(page);
}
function runProgram(e, options = {}) {
  const programStart = performance.now(); let boundary = programStart, workStart = totalWork(e), batchRetired = e.retired, calls = 0;
  const globalBudget = 2_000_000;
  for (;;) {
    assert.ok(++calls < 2000); assert.ok(e.retired < globalBudget, 'finite global retirement budget');
    const chunk = options.split ? [0,1,3,7,0,13,65536][(calls - 1) % 7] : globalBudget - e.retired;
    const stop = dispatch(e, Math.min(chunk, globalBudget - e.retired));
    if (stop.reason === 3) { const pc = readCpu(e).pc, group = e.plan.find(g => g.entries.includes(pc)); assert.ok(group, `undeclared cold PC ${pc.toString(16)}`); compile(e, group, pc); continue; }
    if (stop.reason === 1) { assert.ok(options.split); continue; }
    if (stop.reason === 5 && options.fault) { options.fault(e, stop); continue; }
    assert.equal(stop.reason, 8, JSON.stringify({stop, cpu: readCpu(e)}));
    const frame = capture(e, stop);
    save(`${e.label}-call-${e.captures}.bin`, frame.record);
    if (stop.detail === 0x10005) {
      assert.deepEqual(frame.args, [0,1024,0x3000,4]); complete(e, frame); e.allocation = readCpu(e).registers[0]; assert.ok(e.allocation >= 0x10000000); e.allocations++;
      assert.ok(readPage(e, e.allocation).every(x => x === 0), 'actual provider allocation zero page');
      if (options.afterAllocate) options.afterAllocate(e);
    } else if (stop.detail === 0x10006) {
      const index = e.frees + 1; assert.deepEqual(frame.args, [e.allocation,0,0x8000]);
      if (options.rejectSigned) {
        const page = readPage(e, e.allocation), actual = Array.from({length: spec.words}, (_, i) => page.readUInt32LE(i * 4));
        save(`${e.label}-unsigned-oracle-counterexample-page.bin`, page);
        const signed = [...expected.input].sort((a,b) => (a | 0) - (b | 0));
        assert.deepEqual(actual, signed, 'authored mutant actually takes the signed value branch');
        assert.throws(() => assert.deepEqual(actual, expected.sorted, 'full unsigned sorted array'), assert.AssertionError);
        e.controls.push({name: 'wrong-signed-guest-rejected', actual_sha256: hash(page), actual_checksum: checksum(actual), expected_checksum: expected.checksum, differing_words: actual.filter((v,i) => v !== expected.sorted[i]).length, cpu: readCpu(e)});
        complete(e, frame); e.frees++; assert.equal(e.api.close(), 0); return e;
      }
      const pageHash = validatePage(e, index), observed = snapshot(e, `batch-${index}-before-free`);
      complete(e, frame); e.frees++;
      const now = e.lastProviderEnd, functional = totalWork(e);
      e.batches.push({index, kind: index === 1 ? 'cold' : index <= 3 ? 'warmup' : 'warm', functional_ms: functional - workStart, inclusive_wall_ms: now - boundary, retired: e.retired - batchRetired, checksum: expected.checksum, page_sha256: pageHash, before_free: observed});
      boundary = now; workStart = functional; batchRetired = e.retired;
      readWord(e, e.allocation, true);
      if (options.afterFree) options.afterFree(e);
    } else {
      assert.equal(e.frees, spec.batches); assert.deepEqual(frame.args, [Number(BigInt.asUintN(32, BigInt(expected.checksum) * BigInt(spec.batches)))]);
      complete(e, frame, true); e.terminalFunctionalMs = totalWork(e) - workStart; break;
    }
  }
  e.inclusiveProgramMs = performance.now() - programStart;
  assert.equal(e.allocations, 10); assert.equal(e.frees, 10); assert.equal(e.captures, 21); assert.equal(e.completions, 21);
  assert.equal(e.retired, census.program, 'independent inversion-based whole-program retirement census');
  assert.deepEqual(e.batches.map(b => b.retired), [census.cycle - 2, ...Array(9).fill(census.cycle)]);
  assert.equal(readWord(e, BASE + spec.counter_rva), 0); assert.equal(readWord(e, BASE + spec.canary_rva), 0);
  const lastInput = expected.input.at(-1), rank = expected.sorted.indexOf(lastInput), predecessor = rank ? expected.sorted[rank - 1] : expected.sorted[1];
  let state = expected.input.at(-2); state = (state ^ (state << 13)) >>> 0; state = (state ^ (state >>> 17)) >>> 0;
  const lastTemporary = (state << 5) >>> 0;
  assert.deepEqual(readCpu(e), {registers: [1,0,lastTemporary,predecessor,spec.stack_base + 4096 - 8,0,e.allocation + 1024,e.allocation], pc: spec.gate_base + 32, flags: 0x46});
  e.final = snapshot(e, 'terminal'); return e;
}
process.on('uncaughtException', error => {
  for (const e of contexts) save(`failure-${e.label}-arena.bin`, arena(e));
  save('failure-first.json', JSON.stringify({message: error.message, stack: error.stack, actual: error.actual, expected: error.expected, contexts: contexts.map(e => ({name: e.label, retired: e.retired, cpu: readCpu(e), exit: exit(e)}))}, null, 2));
  console.error(error); process.exitCode = 1;
});
const baseline = fresh('baseline'); save('startup-arena.bin', arena(baseline)); runProgram(baseline);
assert.equal(baseline.cold.length, 5);
const controls = [];
if (campaign === 'first') {
  const split = fresh('split');
  const beforeZero = cpu(split), zero = dispatch(split, 0); assert.equal(zero.reason, 1); assert.equal(zero.retired, 0); assert.deepEqual(cpu(split), beforeZero);
  refresh(split).setUint32(split.base + 96, 1, true);
  const beforeCancel = cpu(split), cancelled = dispatch(split, 0); assert.equal(cancelled.reason, 2); assert.equal(cancelled.retired, 0); assert.deepEqual(cpu(split), beforeCancel);
  refresh(split).setUint32(split.base + 96, 0, true);
  runProgram(split, {split: true}); assert.equal(split.retired, baseline.retired); assert.deepEqual(readCpu(split), readCpu(baseline));
  controls.push({name: 'zero-cancel-split-budget', zero, cancelled, retired: split.retired, final: split.final, chunk_sequence: [0,1,3,7,0,13,65536], global_budget: 2_000_000});

  const fault = fresh('write-fault'); let faulted = false;
  runProgram(fault, {
    afterAllocate(e) { if (e.allocations === 1) status(e, 'fault_control', () => e.api.protect(e.allocation, 1, 1)); },
    fault(e, first) {
      assert.ok(!faulted); faulted = true;
      assert.deepEqual(first, {version: 2, reason: 5, retired: 12, detail: 2, address: e.allocation, access: 2, length: 4});
      assert.equal(e.retired, 19);
      const stopped = readCpu(e); assert.equal(stopped.pc, TEXT + 0x121); assert.equal(stopped.registers[0], expected.input[0]); assert.equal(stopped.registers[1], 256); assert.equal(stopped.registers[6], e.allocation); assert.equal(stopped.flags, 2);
      const before = arena(e), again = dispatch(e, 2_000_000 - e.retired), after = arena(e), wanted = Buffer.from(before); wanted.writeUInt32LE(0, 76);
      assert.deepEqual(again, {...first, retired: 0}); assert.deepEqual(after, wanted, 'same actual fault retry changes only retired to zero'); assert.deepEqual(readCpu(e), stopped);
      save('write-fault-first-arena.bin', before); save('write-fault-retry-arena.bin', after);
      assert.ok(readPage(e, e.allocation).every(x => x === 0), 'failed store commits no bytes');
      status(e, 'fault_control', () => e.api.protect(e.allocation, 1, 3)); assert.deepEqual(readCpu(e), stopped);
      e.controls.push({name: 'actual-store-fault-retry-repair', first, retry: again, stopped, retirement_prefix: e.retired, repair: 'protect tracked allocation back to Read/Write'});
    },
  });
  assert.ok(faulted); assert.equal(fault.retired, baseline.retired); assert.deepEqual(readCpu(fault), readCpu(baseline)); controls.push(...fault.controls);

  const stale = fresh('stale'); let staleChecked = false;
  status(stale, 'stale_control', () => stale.api.protect(TEXT, 1, 7));
  runProgram(stale, {afterFree(e) {
    if (e.frees !== 1) return;
    assert.ok(!staleChecked); staleChecked = true;
    const stopped = readCpu(e); assert.equal(stopped.pc, TEXT + 62);
    assert.equal(readWord(e, BASE + spec.counter_rva), 10); assert.equal(readWord(e, BASE + spec.aggregate_rva), expected.checksum);
    const old = e.groups.get('main'), byteOffset = 1, identicalByte = Buffer.from([image[spec.text_raw + byteOffset]]);
    transfer(e, identicalByte); status(e, 'stale_control', () => e.api.upload(TEXT + byteOffset, 1));
    const before = arena(e), allocationBefore = Buffer.from(e.memory.buffer, old.pointer, old.length).map(x => x);
    const result = old.run(0xffffffff, 0xffffffff, 0, 0xffffffff);
    const allocationAfter = Buffer.from(e.memory.buffer, old.pointer, old.length).map(x => x);
    assert.equal(result, 4); assert.deepEqual(allocationAfter, allocationBefore, 'first synchronous observation preserves entire retained module allocation'); assert.deepEqual(arena(e), before);
    save('stale-retained-allocation-before.bin', allocationBefore); save('stale-retained-allocation-after.bin', allocationAfter); save('stale-first-rejection-arena.bin', before);
    const retiredIds = [];
    for (const unit of [...e.groups.values()].filter(u => u.name !== 'gates')) {
      status(e, 'stale_control', () => e.api.retire_stale_resident(e.low, e.high, unit.low, unit.high));
      e.table.set(unit.slot, null); unit.active = false; e.groups.delete(unit.name); retiredIds.push([unit.low, unit.high]);
    }
    assert.equal(retiredIds.length, 4); assert.deepEqual(readCpu(e), stopped);
    e.controls.push({name: 'same-byte-consumed-code-stale-and-current-resume', precompile_text_permissions: 'Read/Write/Execute in this isolated control only', rejected_status: result, consumed_address: TEXT + byteOffset, uploaded_byte: identicalByte[0], stopped, retired_ids: retiredIds, retained_module_sha256: hash(allocationAfter), prefix_allocations: e.allocations, prefix_frees: e.frees});
  }});
  assert.ok(staleChecked); assert.equal(stale.retired, baseline.retired); assert.deepEqual(readCpu(stale), readCpu(baseline)); assert.equal(stale.cold.length, 9); controls.push(...stale.controls);

  const mutant = Buffer.from(image); assert.equal(mutant[spec.text_raw + 0x193], 0x76); mutant[spec.text_raw + 0x193] = 0x7e;
  save('signed-mutant.exe', mutant); const wrongSigned = fresh('signed-mutant', mutant); runProgram(wrongSigned, {rejectSigned: true}); controls.push(...wrongSigned.controls);
  assert.throws(() => assert.equal(readWord(baseline, BASE + spec.checksum_rva), expected.checksum ^ 1, 'incorrect checksum oracle'), assert.AssertionError);
  controls.push({name: 'wrong-checksum-oracle-rejected', actual: expected.checksum, false_expectation: expected.checksum ^ 1});
}
const sourcesAfter = sourceMap(); assert.deepEqual(sourcesAfter, sourcesBefore); save('sources-after.json', JSON.stringify(sourcesAfter, null, 2));
for (const e of contexts.filter(e => e.label !== 'signed-mutant')) assert.equal(e.api.close(), 0);
const result = {status: 'PASS', phase: 'whole-program-and-controls', campaign, engine_sha256: engine.sha256, pe_sha256: hash(image), oracle: expected, retirement_census: census, tools: {node: process.version, v8: process.versions.v8, platform: process.platform, arch: process.arch}, command: process.argv, baseline: {batches: baseline.batches, work: baseline.work, inclusive_program_ms: baseline.inclusiveProgramMs, core_instance_ms: baseline.instanceMs, terminal_functional_ms: baseline.terminalFunctionalMs, retired: baseline.retired, cold: baseline.cold, final: baseline.final, diagnostic_reads: baseline.reads, allocations: baseline.allocations, frees: baseline.frees, captures: baseline.captures, completions: baseline.completions}, controls, context_summary: contexts.map(e => ({name: e.label, retired: e.retired, cold_units: e.cold.length, allocations: e.allocations, frees: e.frees, captures: e.captures, completions: e.completions, diagnostic_reads: e.reads, controls: e.controls, final: e.final})), sources_before: sourcesBefore, sources_after: sourcesAfter, artifacts, timing: 'functional_ms sums measured work segments, excluding diagnostics/assertions/hash/save; inclusive_wall_ms uses real Free-provider-return boundaries and includes intervening diagnostics; inclusive_program_ms includes diagnostics. The core Module is prepared before timers; this is not first-launch/end-to-end latency.'};
save('result.json', JSON.stringify(result, null, 2));
console.log(JSON.stringify({status: result.status, phase: result.phase, campaign, retired: baseline.retired, cold_units: baseline.cold.length, warm_ms: baseline.batches.filter(b => b.kind === 'warm').map(b => b.functional_ms), output}));
