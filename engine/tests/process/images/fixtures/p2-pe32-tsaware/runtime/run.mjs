import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, readdirSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../../../../../support/wasm/engine.mjs';

const [enginePath, output, root] = process.argv.slice(2), SIZE = 4612, TRANSFER = 140;
assert.ok(enginePath && output && root);
const fixture = 'engine/tests/process/images/fixtures/p2-pe32-tsaware', hash = b => createHash('sha256').update(b).digest('hex');
const engine = readEngine(enginePath), initial = readFileSync(join(output, 'initial-arena.bin'));
assert.equal(initial.length, SIZE); assert.deepEqual(WebAssembly.Module.imports(engine.module), []);
const original = readFileSync(join(root, fixture, 'program.exe'));
const text = readFileSync(join(root, fixture, 'program.text.bin'));
const producer = JSON.parse(readFileSync(join(root, fixture, 'receipt.json')));
assert.equal(original.length, 1024); assert.equal(hash(original), '24fba73c0dfc0c99c528575f983b07121a1e4f0d43daba5f5c1babff041f94ab');
assert.equal(text.toString('hex'), 'b80700000083c023ebfe');
assert.deepEqual(producer.pe, {image_base: 0x400000, image_size: 0x2000, entry_point: 0x401000,
  mapped_pages: 2, dll_characteristics: 0x8100, dll_characteristics_offset: 214, text_rva: 0x1000,
  text_raw_offset: 512, text_virtual_bytes: 10, text_raw_bytes: 512, entry_offsets: [0, 8]});
for (const [file, pin] of Object.entries(producer.artifacts)) {
  const bytes = readFileSync(join(root, fixture, file)); assert.equal(bytes.length, pin.bytes); assert.equal(hash(bytes), pin.sha256);
}
assert.deepEqual(original.subarray(512, 522), text); assert.equal(original.readUInt16LE(214), 0x8100);
const unsupported = Buffer.from(original); unsupported.writeUInt16LE(0x8180, 214);
assert.deepEqual([...unsupported.keys()].filter(i => unsupported[i] !== original[i]), [214]);
const artifacts = {}, modules = [], contexts = [], observations = [], diagnostics = [];
function save(file, bytes) {writeFileSync(join(output, file), bytes, {flag: 'wx'}); artifacts[file] = {bytes: bytes.length, sha256: hash(bytes)}; return file;}
save('engine.wasm', engine.bytes); save('input-original.exe', original); save('input-unsupported-force-integrity.exe', unsupported);
const rustPaths = dir => readdirSync(join(root, dir), {withFileTypes: true}).flatMap(item => item.isDirectory()
  ? rustPaths(`${dir}/${item.name}`) : item.name.endsWith('.rs') ? [`${dir}/${item.name}`] : []);
const paths = [...rustPaths('engine/src'), 'Cargo.toml', 'Cargo.lock', 'engine/Cargo.toml', 'rust-toolchain.toml',
  'engine/tests/loader/pe32/cases/native/pe32_dll_characteristics.rs', 'engine/tests/loader/pe32/native.rs', 'engine/tests/support/loader/pe32.rs', 'engine/tests/support/loader/pe32_relocation.rs', 'engine/tests/loader/pe32/native.rs', 'engine/tests/process/images/cases/wasm/process_pe32_tsaware_wasm.rs', 'engine/tests/process/images/wasm.rs', 'engine/tests/support/wasm.rs', 'engine/tests/process/images/wasm.rs', 'engine/tests/support/wasm.rs', 'engine/tests/support/wasm/engine.mjs',
  ...['program.S', 'build.py', 'PROVENANCE.md', 'runtime/run.mjs', 'program.exe', 'program.obj', 'program.text.bin', 'program.disasm.txt', 'receipt.json'].map(n => `${fixture}/${n}`)].sort();
assert.equal(new Set(paths).size, paths.length);
const sources = () => Object.fromEntries(paths.map(p => [p, hash(readFileSync(join(root, p)))]));
const sourcePins = sources(), counts = {contexts: 0, modules: 0, runs: 0, retired: 0, rejected_loads: 0, diagnostic_reads: 0, permission_probes: 0};
const words = values => {const b = Buffer.alloc(values.length * 4); values.forEach((v, i) => b.writeUInt32LE(v >>> 0, i * 4)); return b;};
function record(magic, size, fields, version = 1) {
  const b = Buffer.alloc(size); b.write(magic); b.writeUInt16LE(version, 4); b.writeUInt16LE(1, 6); b.writeUInt32LE(size, 8);
  fields.forEach((v, i) => b.writeUInt32LE(v >>> 0, 16 + i * 4)); return b;
}
const state = s => record('R3ST', 56, [...s.registers, s.pc, s.flags]);
const exit = retired => record('R3EX', 40, [1, retired, 0, 0, 0, 0]);
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
    const stem = `${c.owner}-${observations.length + 1}`;
    observations.push({context: c.id, owner: c.owner, label, expected_status: status, observed_status: observedStatus,
      before: save(`${stem}-before.bin`, before), after: save(`${stem}-after.bin`, after)});
  }
  return after;
}
function read(c, address, value, detail = 0) {
  const packet = record('R3MH', 40, detail ? [1, 0, detail, address, 1, 1] : [0, value, 0, 0, 0, 1], 2);
  action(c, 'public read8 diagnostic', () => c.api.read8(address), 0, [[100, packet]], false); counts.diagnostic_reads++;
}
function checkImage(c, label) {
  for (const [i, byte] of original.subarray(0, 512).entries()) read(c, 0x400000 + i, byte);
  for (const [i, byte] of text.entries()) read(c, 0x401000 + i, byte);
  for (const [offset, value] of [[10, 0xcc], [511, 0xcc], [512, 0], [4095, 0]]) read(c, 0x401000 + offset, value);
  read(c, 0x400200, 0); read(c, 0x400fff, 0); read(c, 0x402000, 0, 1);
  diagnostics.push({context: c.id, label, exact_header_bytes: 512, exact_text_bytes: 10,
    selected_text_padding: [10, 511, 512, 4095], selected_header_tail: [512, 4095], trailing_page_unmapped: 0x402000});
}
function compile(c) {
  const entries = [0x401000, 0x401008]; assert.ok(entries.length <= 8 && 3 <= 64 && ++c.captures <= 6);
  input(c, TRANSFER, words(entries)); const before = arena(c);
  const status = c.api[c.owner === 'resident' ? 'compile_resident_entries' : 'compile_entries'](entries.length, 0);
  assert.equal(status, 0); const after = arena(c), expected = Buffer.from(before);
  let binding;
  if (c.owner === 'resident') {
    assert.equal(after.readUInt32LE(140), 1); assert.equal(after.readUInt32LE(144), 24);
    binding = {low: after.readUInt32LE(148), high: after.readUInt32LE(152), pointer: after.readUInt32LE(156), length: after.readUInt32LE(160)};
    expected.set(words([1, 24, binding.low, binding.high, binding.pointer, binding.length]), TRANSFER);
  } else binding = {generation: c.api.generation(), pointer: c.api.module_ptr() >>> 0, length: c.api.module_len() >>> 0};
  assert.deepEqual(after, expected, 'compilation preserves full arena except exact resident receipt');
  assert.ok(binding.length > 8 && binding.length <= 65536);
  const bytes = Buffer.from(new Uint8Array(c.memory.buffer, binding.pointer, binding.length)), module = new WebAssembly.Module(bytes);
  const guard = c.owner === 'resident' ? 'guard_resident' : 'guard';
  const imports = [{module: 'env', name: 'memory', kind: 'memory'}, {module: 'ring3', name: guard, kind: 'function'}];
  assert.deepEqual(WebAssembly.Module.imports(module), imports); assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  const run = new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3: {[guard]: c.api[guard]}}).exports.run;
  assert.equal(run.length, 4);
  const file = save(`module-${++counts.modules}.wasm`, bytes);
  modules.push({context: c.id, owner: c.owner, file, sha256: hash(bytes), bytes: bytes.length, imports,
    entry_points: entries, blocks: 2, instructions: 3, source_bytes: 10, ...binding});
  save(`${c.owner}-compiled-arena.bin`, after); return run;
}
for (const owner of ['replacement', 'resident']) {
  const instance = new WebAssembly.Instance(engine.module, {}), memory = instance.exports.memory;
  const arities = {open: 3, close: 0, arena_ptr: 0, load_pe32: 1, compile_entries: 2, compile_resident_entries: 2,
    generation: 0, module_ptr: 0, module_len: 0, read8: 1, write8: 2, guard: 6, guard_resident: 7};
  const api = Object.fromEntries(Object.entries(arities).map(([name, arity]) => {
    const fn = instance.exports[`ring3_abi_v1_${name}`]; assert.equal(typeof fn, 'function'); assert.equal(fn.length, arity, name); return [name, fn];
  }));
  const c = {id: ++counts.contexts, owner, api, memory, captures: 0}; assert.ok(memory instanceof WebAssembly.Memory);
  assert.equal(api.open(2, c.id, 0x54534157), 0); c.base = api.arena_ptr() >>> 0;
  assert.ok(c.base > 0 && c.base + SIZE <= memory.buffer.byteLength); input(c, 0, initial);
  input(c, TRANSFER, unsupported);
  action(c, 'unsupported force-integrity bit0080 rejects original-image copy neutrally', () => api.load_pe32(unsupported.length), 20); counts.rejected_loads++;
  assert.equal(api.generation(), 0); assert.equal(api.module_len(), 0); read(c, 0x400000, 0, 1);
  input(c, TRANSFER, original);
  const receipt = words([0x45503352, 0x10001, 32, 0, 0x400000, 0x2000, 0x401000, 2]);
  const loaded = action(c, 'same-instance original linker image loads with exact fixed receipt', () => api.load_pe32(original.length), 0, [[TRANSFER, receipt]]);
  save(`${owner}-load-receipt.bin`, loaded.subarray(TRANSFER, TRANSFER + 32)); checkImage(c, 'before execution');
  for (const address of [0x400000, 0x401000]) {
    action(c, 'original headers/text deny public write8', () => api.write8(address, 0x19), 0,
      [[100, record('R3MH', 40, [1, 0, 2, address, 2, 1], 3)]]); counts.permission_probes++;
  }
  input(c, TRANSFER, words([0x400000]));
  action(c, 'read-only headers cannot be fetched as instructions', () => api.compile_entries(1, 0), 10);
  assert.equal(api.generation(), 0); assert.equal(api.module_len(), 0);
  const run = compile(c), registers = Array.from({length: 8}, (_, i) => (0xa100 + i) * 65536 + 0x1234 + i);
  const start = {registers, pc: 0x401000, flags: 0x8d7};
  const declared = Buffer.alloc(140); declared.set(state(start)); declared.set(record('R3EX', 40, [3, 0, 0, 0, 0, 0]), 56);
  declared.set(record('R3MH', 40, [0, 0xdecafbad, 0, 0, 0, 0]), 100); input(c, 0, declared);
  save(`${owner}-declared-initial-cpu.bin`, arena(c));
  const next = {registers: [42, ...registers.slice(1)], pc: 0x401008, flags: 2};
  for (const budget of [3, 7]) {
    const after = action(c, `original complete entry then live loop budget${budget}`, () => run(c.base, c.base + 56, budget, c.base + 96),
      0, [[0, state(next)], [56, exit(budget)]]);
    const row = observations.at(-1); Object.assign(row, {budget, expected_reason: 1, observed_reason: after.readUInt32LE(72),
      expected_retired: budget, observed_retired: after.readUInt32LE(76), expected_pc: next.pc, observed_pc: after.readUInt32LE(48),
      expected_flags: 2, observed_flags: after.readUInt32LE(52), expected_eax: 42, observed_eax: after.readUInt32LE(16)});
    counts.runs++; counts.retired += after.readUInt32LE(76);
  }
  checkImage(c, 'after execution without guest/host image writes');
  save(`${owner}-final-arena.bin`, arena(c)); action(c, 'close preserves complete arena', () => api.close());
  contexts.push({id: c.id, owner, captures: c.captures, mapped_pages: 2, initial_cpu_input: start, no_start_loaded_image: true});
}
assert.deepEqual(sources(), sourcePins); assert.deepEqual(counts, {contexts: 2, modules: 2, runs: 4, retired: 20,
  rejected_loads: 2, diagnostic_reads: 2118, permission_probes: 4});
const result = {status: 'ok', engine_sha256: engine.sha256, source_pins: sourcePins, artifacts, counts, modules, contexts,
  observations, diagnostics, helper_calls: 0, helper_trace: [], original_image_sha256: hash(original),
  limits: ['unmodified original1024B clang/coff/lld PE32 with defaultTSAWARE8100; complete10B program, two semantic blocks and three instructions',
    'one declared CPU input after load/compile; actual replacement/resident budget3 then7 without CPU/RAM reseeding; no startup, return or process-exit claim',
    'full4612 assertions preserve dirtyFP128, helper and all unrelated bytes; persisted before/after snapshots for each nondiagnostic action',
    'whole512B original headers and10B code checked before/after; selected padding/tail/unmapped-page diagnostics only, no wholeRAM claim',
    'unsupported FORCE_INTEGRITY0080 sidecopy rejects neutrally before same-instance original load; exact32B fixed receipt and read-only headers/read-execute text',
    'one capture/context, unchanged8blocks/64instructions/65536module/2mappedpages; guard-only imports mean no guest data-helper calls, not instrumented guard counts',
    'bounded privateflat32 PE declaration admission and budget-stopped execution; no terminalserver/thread/Windows compatibility/performance/SDK/browser/game claim']};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2) + '\n', {flag: 'wx'});
console.log(JSON.stringify({status: 'ok', ...counts, output}));
