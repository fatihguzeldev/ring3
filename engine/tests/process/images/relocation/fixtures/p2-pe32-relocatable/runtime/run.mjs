import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, readdirSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../../../../../../support/wasm/engine.mjs';

const [enginePath, output, root] = process.argv.slice(2), SIZE = 4612, TRANSFER = 140;
assert.ok(enginePath && output && root);
const fixture = 'engine/tests/process/images/relocation/fixtures/p2-pe32-relocatable', hash = bytes => createHash('sha256').update(bytes).digest('hex');
const engine = readEngine(enginePath), initial = readFileSync(join(output, 'initial-arena.bin'));
assert.equal(initial.length, SIZE); assert.deepEqual(WebAssembly.Module.imports(engine.module), []);
const original = readFileSync(join(root, fixture, 'relocations.exe'));
const historical = JSON.parse(readFileSync(join(root, fixture, 'fixture.json')));
const evidence = JSON.parse(readFileSync(join(root, fixture, 'evidence.json')));
assert.equal(original.length, 6144); assert.equal(hash(original), 'ca36f32c699b3e3200da8cc6d23fe7465f14e5df9fdedd425e9da9fb2a395818');
for (const receipt of [historical, evidence]) {
  assert.equal(receipt.provenance, 'self-authored'); assert.equal(receipt.licenseStatus, 'pending');
  assert.equal(receipt.windowsOracle, 'not-run'); assert.equal(receipt.executed, false);
  for (const [file, pin] of Object.entries(receipt.architectures.i386.artifacts)) {
    const bytes = readFileSync(join(root, fixture, file)); assert.equal(bytes.length, pin.bytes); assert.equal(hash(bytes), pin.sha256);
  }
}
assert.equal(hash(readFileSync(join(root, fixture, 'i386.s'))), historical.sources['i386.s'].sha256);
assert.equal(hash(readFileSync(join(root, fixture, 'inspection.txt'))), evidence.builds.i386.inspection.sha256);
assert.equal(original.readUInt32LE(60), 120); assert.equal(original.readUInt16LE(144), 0x10b);
assert.equal(original.readUInt16LE(214), 0x8140); assert.equal(original.readUInt32LE(172), 0x400000);
assert.equal(original.readUInt32LE(200), 0x5000); assert.equal(original.readUInt32LE(204), 512);
assert.equal(original.subarray(512, 515).toString('hex'), '31c0c3');
assert.equal(original.subarray(5632, 5656).toString('hex'), '002000000c00000008300c30003000000c00000010300000');
for (const offset of [1032, 1036, 5136]) assert.equal(original.readUInt32LE(offset), 0x402000);
const unsupported = Buffer.from(original); unsupported.writeUInt16LE(0x8340, 214);
assert.deepEqual([...original.keys()].filter(i => original[i] !== unsupported[i]), [215]);
const artifacts = {}, modules = [], contexts = [], observations = [], diagnostics = [], helperTrace = [];
function save(file, bytes) {writeFileSync(join(output, file), bytes, {flag: 'wx'}); artifacts[file] = {bytes: bytes.length, sha256: hash(bytes)}; return file;}
save('engine.wasm', engine.bytes); save('input-original.exe', original); save('input-unsupported-no-isolation.exe', unsupported);
const rustPaths = dir => readdirSync(join(root, dir), {withFileTypes: true}).flatMap(item => item.isDirectory()
  ? rustPaths(`${dir}/${item.name}`) : item.name.endsWith('.rs') ? [`${dir}/${item.name}`] : []);
const paths = [...rustPaths('engine/src'), 'Cargo.toml', 'Cargo.lock', 'engine/Cargo.toml', 'rust-toolchain.toml',
  'engine/tests/loader/pe32/native/declarations.rs', 'engine/tests/loader/pe32/native.rs', 'engine/tests/support/loader/pe32.rs', 'engine/tests/support/loader/pe32_relocation.rs', 'engine/tests/process/images/input/native/original.rs', 'engine/tests/process/images/native.rs', 'engine/tests/process/images/relocation/wasm/original.rs', 'engine/tests/process/images/wasm.rs', 'engine/tests/support/wasm.rs',
  'engine/tests/support/wasm/engine.mjs', ...['i386.s', 'relocations.obj', 'relocations.exe', 'inspection.txt',
    'fixture.json', 'evidence.json', 'producer-source-pins.json', 'PROVENANCE.md', 'runtime/run.mjs'].map(n => `${fixture}/${n}`)].sort();
assert.equal(new Set(paths).size, paths.length);
const sources = () => Object.fromEntries(paths.map(p => [p, hash(readFileSync(join(root, p)))]));
const sourcePins = sources(), counts = {contexts: 0, modules: 0, runs: 0, retired: 0, load_refusals: 0,
  retained_begin_refusals: 0, diagnostic_reads: 0, permission_probes: 0, execute_refusals: 0, guard_controls: 0,
  helper_calls: 0, read32: 0, store32: 0};
const words = values => {const bytes = Buffer.alloc(values.length * 4); values.forEach((v, i) => bytes.writeUInt32LE(v >>> 0, i * 4)); return bytes;};
function record(magic, size, fields, version = 1) {
  const bytes = Buffer.alloc(size); bytes.write(magic); bytes.writeUInt16LE(version, 4); bytes.writeUInt16LE(1, 6); bytes.writeUInt32LE(size, 8);
  fields.forEach((v, i) => bytes.writeUInt32LE(v >>> 0, 16 + i * 4)); return bytes;
}
const state = s => record('R3ST', 56, [...s.registers, s.pc, s.flags]);
const exit = retired => record('R3EX', 40, [1, retired, 0, 0, 0, 0], 2);
const helper = value => record('R3MH', 40, [0, value, 0, 0, 0, 0]);
const fault = (address, access, detail) => record('R3MH', 40, [1, 0, detail, address, access, 4]);
const arena = c => Buffer.from(new Uint8Array(c.memory.buffer, c.base, SIZE));
function input(c, offset, bytes) {
  const expected = arena(c); expected.set(bytes, offset); new Uint8Array(c.memory.buffer).set(bytes, c.base + offset);
  assert.deepEqual(arena(c), expected, 'declared caller input preserves the remaining full arena');
}
function action(c, label, fn, status = 0, patches = [], persist = true) {
  const before = arena(c), expected = Buffer.from(before); for (const [offset, bytes] of patches) expected.set(bytes, offset);
  const observedStatus = fn(); assert.equal(observedStatus, status, label);
  const after = arena(c); assert.deepEqual(after, expected, `full4612: ${label}`);
  if (persist) {
    const stem = `context-${c.id}-action-${observations.length + 1}`;
    observations.push({context: c.id, owner: c.owner, label, expected_status: status, observed_status: observedStatus,
      before: save(`${stem}-before.bin`, before), after: save(`${stem}-after.bin`, after)});
  }
  return after;
}
function diagnosticRead(c, address, value, detail = 0) {
  const after = action(c, 'public read32 diagnostic', () => c.api.read32(address), 0,
    [[100, detail ? fault(address, 1, detail) : helper(value)]], false);
  counts.diagnostic_reads++; return after.readUInt32LE(120);
}
function checkBytes(c, label, address, expected) {
  assert.equal(expected.length % 4, 0); const observed = Buffer.alloc(expected.length);
  for (let offset = 0; offset < expected.length; offset += 4) observed.writeUInt32LE(diagnosticRead(c, address + offset, expected.readUInt32LE(offset)), offset);
  assert.deepEqual(observed, expected, label);
  const file = save(`context-${c.id}-${label}.bin`, observed);
  diagnostics.push({context: c.id, label, address, bytes: observed.length, words: observed.length / 4, file, sha256: hash(observed)});
}
function expectedImage(actualBase) {
  const bytes = Buffer.alloc(0x5000); bytes.set(original.subarray(0, 512));
  bytes.set(original.subarray(512, 1024), 0x1000); bytes.set(original.subarray(1024, 5632), 0x2000);
  bytes.set(original.subarray(5632, 6144), 0x4000);
  for (const rva of [0x2008, 0x200c, 0x3010]) bytes.writeUInt32LE(actualBase + 0x2000, rva);
  assert.equal(bytes.readUInt32LE(0x2000), 7); assert.equal(bytes.readUInt32LE(172), 0x400000); return bytes;
}
function stage(c, bytes, partial = false) {
  action(c, 'begin original-size staged input', () => c.api.begin_image_input(bytes.length));
  input(c, TRANSFER, bytes.subarray(0, 4096));
  action(c, 'append first4096 original input bytes', () => c.api.append_image_input(0, 4096));
  if (partial) {action(c, 'incomplete staged completion is neutral', () => c.api.load_pe32_input_at(c.actualBase), 7); counts.load_refusals++;}
  input(c, TRANSFER, bytes.subarray(4096));
  action(c, 'append final2048 original input bytes', () => c.api.append_image_input(4096, 2048));
}
function compile(c, entry) {
  const specs = [[0x8000, 5], [0x8005, 7], [entry, 3]]; assert.equal(++c.captures, 1);
  input(c, TRANSFER, words(specs.flat())); const before = arena(c);
  assert.equal(c.api[c.owner === 'resident' ? 'compile_resident' : 'compile'](specs.length), 0);
  const after = arena(c), expected = Buffer.from(before); let binding;
  if (c.owner === 'resident') {
    assert.equal(after.readUInt32LE(140), 1); assert.equal(after.readUInt32LE(144), 24);
    binding = {low: after.readUInt32LE(148), high: after.readUInt32LE(152), pointer: after.readUInt32LE(156), length: after.readUInt32LE(160)};
    expected.set(words([1, 24, binding.low, binding.high, binding.pointer, binding.length]), TRANSFER);
  } else binding = {generation: c.api.generation(), pointer: c.api.module_ptr() >>> 0, length: c.api.module_len() >>> 0};
  assert.deepEqual(after, expected, 'compile changes only the exact resident receipt'); assert.ok(binding.length > 8 && binding.length <= 65536);
  const bytes = Buffer.from(new Uint8Array(c.memory.buffer, binding.pointer, binding.length)), module = new WebAssembly.Module(bytes);
  const guard = c.owner === 'resident' ? 'guard_resident' : 'guard', store = c.owner === 'resident' ? 'store_resident32' : 'store32';
  const imports = [{module: 'env', name: 'memory', kind: 'memory'}, ...[guard, 'read32', store].map(name => ({module: 'ring3', name, kind: 'function'}))];
  assert.deepEqual(WebAssembly.Module.imports(module), imports); assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  const forwarded = name => (...raw) => {
    const args = raw.map(v => v >>> 0), next = c.helperSchedule.shift(); assert.ok(next, 'unexpected guest helper'); assert.equal(name, next.name);
    const wanted = name === 'read32' ? [next.address] : c.owner === 'resident'
      ? [c.id, 0x52454c4f, binding.low, binding.high, next.address, next.value] : [next.address, next.value];
    assert.deepEqual(args, wanted); const before = arena(c), status = c.api[name](...raw), after = arena(c), expected = Buffer.from(before);
    expected.set(helper(name === 'read32' ? next.value : 0), 100); assert.equal(status, 0); assert.deepEqual(after, expected, 'canonical guest helper changes only its40B record');
    helperTrace.push({context: c.id, run: c.run, name, args, observed_status: status, helper_hex: after.subarray(100, 140).toString('hex')});
    counts.helper_calls++; counts[name === 'read32' ? 'read32' : 'store32']++; return status;
  };
  const run = new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3: {[guard]: c.api[guard], read32: forwarded('read32'), [store]: forwarded(store)}}).exports.run;
  assert.equal(run.length, 4); const file = save(`context-${c.id}-module.wasm`, bytes);
  modules.push({context: c.id, owner: c.owner, file, sha256: hash(bytes), bytes: bytes.length, imports,
    block_specs: specs, blocks: 3, instructions: 5, source_bytes: 15, ...binding}); counts.modules++;
  c.guard = () => c.owner === 'resident'
    ? c.api.guard_resident(c.id, 0x52454c4f, binding.low, binding.high, c.base, c.base + 56, c.base + 96)
    : c.api.guard(c.id, 0x52454c4f, binding.generation, c.base, c.base + 56, c.base + 96);
  save(`context-${c.id}-compiled-arena.bin`, after); return {run, store, file};
}
for (const actualBase of [0x400000, 0x500000, 0x300000]) for (const owner of ['replacement', 'resident']) {
  const instance = new WebAssembly.Instance(engine.module, {}), memory = instance.exports.memory;
  const arities = {open: 3, close: 0, arena_ptr: 0, begin_image_input: 1, append_image_input: 2, abort_image_input: 0, load_pe32_input_at: 1,
    map: 3, protect: 3, upload: 2, compile: 1, compile_resident: 1, compile_entries: 2, generation: 0, module_ptr: 0, module_len: 0,
    read32: 1, write32: 2, store32: 2, store_resident32: 6, guard: 6, guard_resident: 7};
  const api = Object.fromEntries(Object.entries(arities).map(([name, arity]) => {
    const fn = instance.exports[`ring3_abi_v1_${name}`]; assert.equal(typeof fn, 'function'); assert.equal(fn.length, arity, name); return [name, fn];
  }));
  const c = {id: ++counts.contexts, owner, actualBase, api, memory, captures: 0, run: 0, helperSchedule: []};
  assert.ok(memory instanceof WebAssembly.Memory); assert.equal(api.open(7, c.id, 0x52454c4f), 0); c.base = api.arena_ptr() >>> 0;
  assert.ok(c.base > 0 && c.base + SIZE <= memory.buffer.byteLength); input(c, 0, initial);
  action(c, 'missing staged input refuses neutrally', () => api.load_pe32_input_at(actualBase), 7); counts.load_refusals++;
  stage(c, unsupported, true);
  for (let repeat = 0; repeat < 2; repeat++) {action(c, 'closed no-isolation bit refuses complete input without publication', () => api.load_pe32_input_at(actualBase), 20); counts.load_refusals++;}
  action(c, 'complete refused input remains active', () => api.begin_image_input(original.length), 7); counts.retained_begin_refusals++;
  assert.equal(api.generation(), 0); assert.equal(api.module_len(), 0); diagnosticRead(c, actualBase, 0, 1);
  action(c, 'explicit abort clears refused staged input', () => api.abort_image_input()); stage(c, original);
  action(c, 'unaligned base refuses original input without publication', () => api.load_pe32_input_at(actualBase + 1), 19); counts.load_refusals++;
  const receipt = words([0x45503352, 0x10001, 32, 0, actualBase, 0x5000, actualBase + 0x1000, 5]);
  const loaded = action(c, 'same complete original input retries at valid explicit base', () => api.load_pe32_input_at(actualBase), 0, [[TRANSFER, receipt]]);
  save(`context-${c.id}-load-receipt.bin`, loaded.subarray(TRANSFER, TRANSFER + 32));
  action(c, 'published image prevents second staged completion', () => api.load_pe32_input_at(actualBase), 7); counts.load_refusals++;
  const image = expectedImage(actualBase); checkBytes(c, 'image-before', actualBase, image); diagnosticRead(c, actualBase + 0x5000, 0, 1);
  for (const address of [actualBase, actualBase + 0x1000, actualBase + 0x4000]) {
    action(c, 'headers text and retained reloc page deny public write32', () => api.write32(address, 0x89abcdef), 0, [[100, fault(address, 2, 2)]]); counts.permission_probes++;
  }
  input(c, TRANSFER, words([actualBase])); action(c, 'read-only image headers deny instruction fetch', () => api.compile_entries(1, 0), 10); counts.execute_refusals++;
  assert.equal(api.generation(), 0); assert.equal(api.module_len(), 0);
  const caller = Buffer.from('e800000000a340900000ebfe', 'hex'); caller.writeInt32LE(actualBase + 0x1000 - 0x8005, 1);
  action(c, 'map separate authored caller page', () => api.map(0x8000, 1, 7)); input(c, TRANSFER, caller);
  action(c, 'upload authored caller before capture', () => api.upload(0x8000, caller.length)); action(c, 'protect authored caller RX before capture', () => api.protect(0x8000, 1, 5));
  action(c, 'map separate stack and output page', () => api.map(0x9000, 1, 3));
  const frame = Buffer.from(Array.from({length: 128}, (_, i) => (i * 13 + 5) & 255)); input(c, TRANSFER, frame);
  action(c, 'upload initial stack frame before capture', () => api.upload(0x9000, frame.length));
  checkBytes(c, 'caller-before', 0x8000, caller); checkBytes(c, 'stack-before', 0x9000, frame);
  const unit = compile(c, actualBase + 0x1000); action(c, 'fresh bound owner guard is current', c.guard); counts.guard_controls++;
  const registers = Array.from({length: 8}, (_, i) => (0xa100 + i) * 65536 + 0x1234 + i); registers[4] = 0x9080;
  const start = {registers, pc: 0x8000, flags: 0xcd7};
  const declared = Buffer.alloc(140); declared.set(state(start)); declared.set(record('R3EX', 40, [3, 0, 0, 0, 0, 0]), 56);
  declared.set(helper(0xdecafbad), 100); input(c, 0, declared); save(`context-${c.id}-declared-initial-arena.bin`, arena(c));
  const called = {registers: [...registers], pc: actualBase + 0x1000, flags: 0xcd7}; called.registers[4] = 0x907c;
  const returned = {registers: [0, ...registers.slice(1)], pc: 0x8005, flags: 0x446};
  const consumed = {...returned, pc: 0x800a};
  const steps = [{label: 'authored CALL retires and publishes original entry', budget: 1, cpu: called, helpers: [{name: unit.store, address: 0x907c, value: 0x8005}], helperValue: 0},
    {label: 'original XOR and natural RET return zero without reseed', budget: 2, cpu: returned, helpers: [{name: 'read32', address: 0x907c, value: 0x8005}], helperValue: 0x8005},
    {label: 'guest caller stores returned EAX in separate stack output', budget: 2, cpu: consumed, helpers: [{name: unit.store, address: 0x9040, value: 0}], helperValue: 0}];
  for (const step of steps) {
    c.run++; c.helperSchedule = [...step.helpers]; const after = action(c, step.label, () => unit.run(c.base, c.base + 56, step.budget, c.base + 96), 0,
      [[0, state(step.cpu)], [56, exit(step.budget)], [100, helper(step.helperValue)]]);
    assert.equal(c.helperSchedule.length, 0); const row = observations.at(-1);
    Object.assign(row, {module: unit.file, run: c.run, budget: step.budget, expected_reason: 1, observed_reason: after.readUInt32LE(72),
      expected_retired: step.budget, observed_retired: after.readUInt32LE(76), expected_registers: step.cpu.registers,
      observed_registers: Array.from({length: 8}, (_, i) => after.readUInt32LE(16 + i * 4)), expected_pc: step.cpu.pc, observed_pc: after.readUInt32LE(48),
      expected_flags: step.cpu.flags, observed_flags: after.readUInt32LE(52)});
    counts.runs++; counts.retired += row.observed_retired;
  }
  frame.writeUInt32LE(0x8005, 124); frame.writeUInt32LE(0, 64);
  checkBytes(c, 'image-after', actualBase, image); diagnosticRead(c, actualBase + 0x5000, 0, 1);
  checkBytes(c, 'caller-after', 0x8000, caller); checkBytes(c, 'stack-after', 0x9000, frame);
  action(c, 'same captured owner remains current after stack-only writes', c.guard); counts.guard_controls++;
  save(`context-${c.id}-final-arena.bin`, arena(c)); action(c, 'close preserves complete arena', () => api.close());
  contexts.push({id: c.id, owner, actual_base: actualBase, preferred_base: 0x400000, entry: actualBase + 0x1000, mapped_image_pages: 5,
    mapped_total_pages: 7, captures: c.captures, initial_cpu_input: start, no_start_loaded_image: true, caller_hex: caller.toString('hex'),
    original_fixup_rvas: [0x2008, 0x200c, 0x3010], observed_pointer_value: actualBase + 0x2000, retained_reloc_rva: 0x4000});
}
assert.deepEqual(sources(), sourcePins);
assert.deepEqual(counts, {contexts: 6, modules: 6, runs: 18, retired: 30, load_refusals: 36, retained_begin_refusals: 6,
  diagnostic_reads: 61878, permission_probes: 18, execute_refusals: 6, guard_controls: 12, helper_calls: 18, read32: 6, store32: 12});
const result = {status: 'ok', engine_sha256: engine.sha256, source_pins: sourcePins, artifacts, counts, modules, contexts,
  observations, diagnostics, helper_trace: helperTrace, original_image_sha256: hash(original),
  limits: ['unchanged self-authored6144B original PE32 and original3B XOR EAX,EAX/RET; staged4096+2048 at three explicit bases and both bound owners',
    'one initial CPU input after load/compile; natural CALL/RET and guest-consumed zero return use budgets1/2/2 without CPU/RAM reseeding',
    'complete loaded5page image checked before/after guest execution; only three loader HIGHLOW fixups differ from raw mapping, retained.reloc bytes unchanged',
    'whole128B authored stack frame and12B caller checked before/after; no wholeprocessRAM claim or process-start/ExitProcess claim',
    'full4612 assertions preserve dirtyFP128 and all unrelated bytes; nondiagnostic actions persist before/after, public diagnostics assert fullarena but do not persist each arena',
    '18 actual forwarded canonical guest helpers are separate from61878 public read32 diagnostics and18 denied public write32 permission probes',
    'one capture/context with3blocks/5instructions/15sourcebytes; default8blocks/64instructions/65536module and7page capacity unchanged',
    'historical licenseStatus pending/Windows oracle not-run receipts retained; no automatic ASLR/discard lifecycle/PE32+/hardware/thread/performance/SDK/browser/game claim']};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2) + '\n', {flag: 'wx'});
console.log(JSON.stringify({status: 'ok', ...counts, output}));
