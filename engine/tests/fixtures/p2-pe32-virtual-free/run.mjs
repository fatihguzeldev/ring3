import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {execFileSync} from 'node:child_process';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(root, 'expected engine wasm, output directory and repository root');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const save = (name, bytes) => writeFileSync(join(output, name), bytes, {flag: 'wx'});
const BASE = 0x500000, ENTRY = BASE + 0x1000, GATE = 0x8000, STACK = 0x70000, TOP = 0x71000;
const ALLOCATION = 0x10000000, FIRST = 0x13579bdf, SECOND = 0x2468ace0, ARENA = 4236, TRANSFER = 140;
const program = Buffer.from(
  '6a04680030000068011000006a00ff1554314000' +
  '89c7c707df9b57138b1fc78700100000e0ac68248baf0010000068008000006a0057ff1550314000' +
  '6a04680030000068011000006a00ff1554314000' +
  '89c68b168b9e00100000891500304000891d043040006a2aff1558314000' +
  'c70508304000a5a5a5a50f0b', 'hex');
const fixups = [[16, 0x3154], [56, 0x3150], [76, 0x3154], [92, 0x3000], [98, 0x3004], [106, 0x3158], [112, 0x3008]];
const gates = [[GATE + 32, 0x10003], [GATE + 64, 0x10005], [GATE + 80, 0x10006]];
const stages = [
  {start: 0, next: 20, retired: 5, gate: 1, args: [0, 4097, 0x3000, 4], result: ALLOCATION, registers: [0, 0, 0, 0, TOP - 20, 0, 0, 0]},
  {start: 20, next: 60, retired: 9, gate: 2, args: [ALLOCATION, 0, 0x8000], result: 1, registers: [ALLOCATION, 0, 0, FIRST, TOP - 16, SECOND, 0, ALLOCATION]},
  {start: 60, next: 80, retired: 5, gate: 1, args: [0, 4097, 0x3000, 4], result: ALLOCATION, registers: [1, 0, 0, FIRST, TOP - 20, SECOND, 0, ALLOCATION]},
  {start: 80, next: 110, retired: 7, gate: 0, args: [42], terminal: true, registers: [ALLOCATION, 0, 0, 0, TOP - 8, SECOND, ALLOCATION, ALLOCATION]},
];

function words(fields) {
  const bytes = new Uint8Array(fields.length * 4), view = new DataView(bytes.buffer);
  fields.forEach((value, index) => view.setUint32(index * 4, value, true)); return bytes;
}
function record(magic, length, fields, version = 1) {
  const bytes = new Uint8Array(length), view = new DataView(bytes.buffer);
  bytes.set(Buffer.from(magic)); view.setUint16(4, version, true); view.setUint16(6, 1, true); view.setUint32(8, length, true);
  fields.forEach((value, index) => view.setUint32(16 + index * 4, value, true)); return bytes;
}
function state(registers, pc) { return record('R3ST', 56, [...registers, pc, 2]); }
function helper(value) { return record('R3MH', 40, [0, value, 0, 0, 0, 0]); }
function authoredPe() {
  const bytes = Buffer.alloc(2048), view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const put16 = (at, value) => view.setUint16(at, value, true), put32 = (at, value) => view.setUint32(at, value, true);
  bytes.write('MZ'); put32(0x3c, 0x80); bytes.write('PE\0\0', 0x80);
  put16(0x84, 0x14c); put16(0x86, 3); put16(0x94, 224); put16(0x96, 0x102); put16(0x98, 0x10b);
  for (const [at, value] of [[4, 512], [8, 1024], [12, 0], [16, 0x1000], [20, 0x1000], [24, 0x3000], [28, 0x400000], [32, 4096], [36, 512], [56, 0x6000], [60, 512], [72, 0x100000], [76, 4096], [80, 0x100000], [84, 4096], [92, 16]]) put32(0x98 + at, value);
  put16(0x98 + 68, 3); put16(0x98 + 70, 0x100);
  for (const [index, rva, size] of [[1, 0x3100, 40], [5, 0x5000, 24], [12, 0x3150, 16]]) { put32(0x98 + 96 + index * 8, rva); put32(0x98 + 100 + index * 8, size); }
  [['.text', 122, 0x1000, 0x200, 0x60000020], ['.data', 512, 0x3000, 0x400, 0xc0000040], ['.fixups', 24, 0x5000, 0x600, 0x40000040]].forEach(([name, virtualSize, rva, raw, flags], index) => {
    const at = 0x178 + index * 40; bytes.write(name, at);
    for (const [field, value] of [[8, virtualSize], [12, rva], [16, 512], [20, raw], [36, flags]]) put32(at + field, value);
  });
  bytes.fill(0xcc, 0x200, 0x400); bytes.set(program, 0x200);
  put32(0x400, 0x11111111); put32(0x404, 0x22222222); put32(0x410, 0xfacecafe);
  [0x3140, 0, 0, 0x3160, 0x3150].forEach((value, index) => put32(0x500 + index * 4, value));
  for (const at of [0x540, 0x550]) [0x3190, 0x31b0, 0x3170].forEach((value, index) => put32(at + index * 4, value));
  bytes.write('KeRnEl32.dLl', 0x560, 'ascii');
  for (const [at, hint, name] of [[0x570, 0x1234, 'ExitProcess'], [0x590, 0x5678, 'VirtualFree'], [0x5b0, 0x9abc, 'VirtualAlloc']]) { put16(at, hint); bytes.write(name, at + 2, 'ascii'); }
  bytes.set(Buffer.from('0010000018000000103038304c305c3062306a3070300000', 'hex'), 0x600);
  return bytes;
}
const image = readFileSync(join(output, 'release.exe'));
assert.equal(program.length, 122); assert.deepEqual(image, authoredPe(), 'entire independently authored PE input');
assert.deepEqual(fixups.map(([offset]) => offset), [16, 56, 76, 92, 98, 106, 112]);
for (const [offset, rva] of fixups) assert.equal(program.readUInt32LE(offset), 0x400000 + rva);
save('program.bin', program);
const engineBytes = readFileSync(enginePath), engineModule = new WebAssembly.Module(engineBytes);
assert.deepEqual(WebAssembly.Module.imports(engineModule), []);
const engineExports = WebAssembly.Module.exports(engineModule);
assert.equal(engineExports.length, 72); assert.equal(engineExports.filter(row => row.kind === 'function').length, 71);
save('engine.wasm', engineBytes);
function sourceMap() {
  const physical = execFileSync('rg', ['--files', 'engine/src'], {cwd: root, encoding: 'utf8'}).trim().split('\n').sort();
  assert.equal(physical.length, 84);
  const paths = ['Cargo.toml', 'Cargo.lock', 'engine/Cargo.toml', 'rust-toolchain.toml', ...physical, 'engine/tests/support/pe32.rs', 'engine/tests/windows_virtual_free_linked_wasm.rs', 'engine/tests/fixtures/p2-pe32-virtual-free/run.mjs'];
  assert.equal(paths.length, 91);
  return Object.fromEntries(paths.map(path => { const bytes = readFileSync(join(root, path)); return [path, {bytes: bytes.length, sha256: hash(bytes)}]; }));
}
const sourcesBefore = sourceMap(); save('source-hashes-before.json', JSON.stringify(sourcesBefore, null, 2));
const headerPage = new Uint8Array(4096); headerPage.set(image.subarray(0, 512));
const codePage = new Uint8Array(4096); codePage.set(image.subarray(0x200, 0x400));
for (const [offset, rva] of fixups) new DataView(codePage.buffer).setUint32(offset, BASE + rva, true);
const dataPage = new Uint8Array(4096); dataPage.set(image.subarray(0x400, 0x600));
new DataView(dataPage.buffer).setUint32(0, 0, true);
new DataView(dataPage.buffer).setUint32(4, 0, true);
new DataView(dataPage.buffer).setUint32(0x150, GATE + 80, true);
new DataView(dataPage.buffer).setUint32(0x154, GATE + 64, true);
new DataView(dataPage.buffer).setUint32(0x158, GATE + 32, true);
const fixupPage = new Uint8Array(4096); fixupPage.set(image.subarray(0x600, 0x800));
const gatePage = new Uint8Array(4096); gates.forEach(([pc]) => gatePage.set([0x0f, 0x0b], pc - GATE));
const receipt = record('R3LI', 96, [BASE, 0x6000, ENTRY, 5, GATE, 3, 0, 0, ...gates.flat(), 0, 0, 0, 0, 0, 0], 5);
const names = ['open', 'close', 'arena_ptr', 'begin_image_input', 'append_image_input', 'load_pe32_linked_v5_input_at', 'start_loaded_image', 'read32', 'compile_entries', 'compile_resident_entries', 'generation', 'module_ptr', 'module_len', 'guard', 'guard_resident', 'store32', 'store_resident32', 'capture_call', 'capture_resident_call', 'complete_windows_call', 'complete_resident_windows_call'];
const observations = [], pages = [], modules = [], rows = [], controls = [];
let generatedCalls = 0, guestRetired = 0, captures = 0, completions = 0, hostReads = 0;
function snapshot(ctx) { return new Uint8Array(ctx.memory.buffer).slice(ctx.base, ctx.base + ARENA); }
function observe(ctx, name) {
  const actual = snapshot(ctx), label = `${ctx.owner}-${name}`;
  assert.deepEqual(actual, ctx.expected, `${label}: complete independent arena model`);
  save(`${label}-arena.bin`, actual); save(`${label}-arena-expected.bin`, ctx.expected);
  observations.push({owner: ctx.owner, name, actual: `${label}-arena.bin`, expected: `${label}-arena-expected.bin`, sha256: hash(actual)});
}
function request(ctx, bytes) {
  assert.ok(bytes.length <= 4096); ctx.expected.set(bytes, TRANSFER);
  new Uint8Array(ctx.memory.buffer).set(bytes, ctx.base + TRANSFER);
  assert.deepEqual(snapshot(ctx), ctx.expected, 'host writes only literal Transfer request');
}
function fresh(owner, low) {
  const instance = new WebAssembly.Instance(engineModule, {}), api = Object.fromEntries(names.map(name => [name, instance.exports[`ring3_abi_v1_${name}`]]));
  for (const [name, fn] of Object.entries(api)) assert.equal(typeof fn, 'function', name);
  assert.equal(api.load_pe32_linked_v5_input_at.length, 2); assert.equal(api.start_loaded_image.length, 2);
  const ctx = {owner, api, memory: instance.exports.memory, low, high: 0xa0050000, compileOrdinal: 0, expected: new Uint8Array(ARENA)};
  assert.equal(api.open(8, low, ctx.high), 0); ctx.base = api.arena_ptr() >>> 0;
  ctx.expected.set(state(Array(8).fill(0), 0)); ctx.expected.set(record('R3EX', 40, [1, 0, 0, 0, 0, 0]), 56); ctx.expected.set(helper(0), 100);
  observe(ctx, 'initial');
  assert.equal(api.begin_image_input(image.length), 0); observe(ctx, 'begin-input');
  request(ctx, image); assert.equal(api.append_image_input(0, image.length), 0); observe(ctx, 'append-input');
  request(ctx, new Uint8Array(4096).fill(0xa5)); observe(ctx, 'overwrite-transfer-after-copy');
  ctx.expected.set(receipt, TRANSFER); assert.equal(api.load_pe32_linked_v5_input_at(BASE, GATE), 0); observe(ctx, 'linked-admission');
  const actualReceipt = snapshot(ctx).slice(TRANSFER, TRANSFER + 96); assert.deepEqual(actualReceipt, receipt); save(`${owner}-linked-receipt.bin`, actualReceipt);
  ctx.expected.set(state([0, 0, 0, 0, TOP, 0, 0, 0], ENTRY)); ctx.expected.set(record('R3EX', 40, [3, 0, 0, 0, 0, 0], 3), 56);
  assert.equal(api.start_loaded_image(STACK, 1), 0); observe(ctx, 'rust-startup');
  return ctx;
}
function compile(ctx, entries, gateSpecs, name) {
  request(ctx, words([...entries, ...gateSpecs.flat()])); const ordinal = ++ctx.compileOrdinal;
  let unit;
  if (ctx.owner === 'replacement') {
    assert.equal(ctx.api.compile_entries(entries.length, gateSpecs.length), 0);
    unit = {generation: ctx.api.generation(), pointer: ctx.api.module_ptr() >>> 0, length: ctx.api.module_len() >>> 0}; assert.equal(unit.generation, ordinal);
  } else {
    assert.equal(ctx.api.compile_resident_entries(entries.length, gateSpecs.length), 0);
    const view = new DataView(ctx.memory.buffer), at = ctx.base + TRANSFER;
    assert.equal(view.getUint32(at, true), 1); assert.equal(view.getUint32(at + 4, true), 24);
    unit = {low: view.getUint32(at + 8, true), high: view.getUint32(at + 12, true), pointer: view.getUint32(at + 16, true), length: view.getUint32(at + 20, true)};
    assert.equal(unit.low, ordinal); assert.equal(unit.high, 0); ctx.expected.set(words([1, 24, unit.low, unit.high, unit.pointer, unit.length]), TRANSFER);
  }
  assert.ok(unit.pointer > 0 && unit.length > 8 && unit.pointer + unit.length <= ctx.memory.buffer.byteLength);
  observe(ctx, `compile-${name}`);
  const bytes = new Uint8Array(ctx.memory.buffer).slice(unit.pointer, unit.pointer + unit.length), module = new WebAssembly.Module(bytes);
  const guard = ctx.owner === 'replacement' ? 'guard' : 'guard_resident', store = ctx.owner === 'replacement' ? 'store32' : 'store_resident32';
  const helpers = name === 'gates' ? [] : ['read32', store];
  assert.deepEqual(WebAssembly.Module.imports(module), [{module: 'env', name: 'memory', kind: 'memory'}, ...[guard, ...helpers].map(name => ({module: 'ring3', name, kind: 'function'}))]);
  assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  unit.run = new WebAssembly.Instance(module, {env: {memory: ctx.memory}, ring3: ctx.api}).exports.run; assert.equal(unit.run.length, 4);
  const filename = `${ctx.owner}-${name}.wasm`; save(filename, bytes);
  modules.push({owner: ctx.owner, name, filename, helpers, key_low: ctx.low, key_high: ctx.high, generation: unit.generation ?? 0, id: unit.low ?? 0, pointer: unit.pointer, length: unit.length, sha256: hash(bytes)});
  return unit;
}
function generatedRun(ctx, unit, stage, gateOnly) {
  const [pc, id] = gates[stage.gate], retired = gateOnly ? 0 : stage.retired;
  ctx.expected.set(state(stage.registers, pc));
  ctx.expected.set(record('R3EX', 40, [ctx.owner === 'resident' && !gateOnly ? 3 : 8, retired, ctx.owner === 'resident' && !gateOnly ? 0 : id, 0, 0, 0], ctx.owner === 'resident' && !gateOnly ? 2 : 3), 56);
  ctx.expected.set(helper(0), 100);
  assert.equal(unit.run(ctx.base, ctx.base + 56, 64, ctx.base + 96), 0);
  observe(ctx, `${gateOnly ? 'gate' : 'caller'}-run-${stage.start}`); generatedCalls++; guestRetired += retired;
}
function hostRead(ctx, address, value) {
  ctx.expected.set(helper(value), 100); assert.equal(ctx.api.read32(address), 0); assert.deepEqual(snapshot(ctx), ctx.expected);
  hostReads++; return new DataView(ctx.memory.buffer).getUint32(ctx.base + 120, true);
}
function wholePage(ctx, address, literal, name) {
  const actual = new Uint8Array(4096), view = new DataView(actual.buffer), expected = new DataView(literal.buffer, literal.byteOffset, literal.byteLength);
  for (let index = 0; index < 1024; index++) view.setUint32(index * 4, hostRead(ctx, address + index * 4, expected.getUint32(index * 4, true)), true);
  assert.deepEqual(actual, literal); observe(ctx, `page-${name}`);
  const label = `${ctx.owner}-${name}`; save(`${label}-page.bin`, actual); save(`${label}-page-expected.bin`, literal);
  pages.push({owner: ctx.owner, name, address, actual: `${label}-page.bin`, expected: `${label}-page-expected.bin`, sha256: hash(actual)});
}
function capture(ctx, unit, stage, index) {
  hostRead(ctx, stage.registers[4], ENTRY + stage.next); stage.args.forEach((value, slot) => hostRead(ctx, stage.registers[4] + 4 + slot * 4, value));
  const token = index + 1, [pc, id] = gates[stage.gate];
  ctx.expected.set(record('R3CF', 112, [token, id, 2, stage.args.length, pc, stage.registers[4], ENTRY + stage.next, 0, ...stage.args, ...Array(16 - stage.args.length).fill(0)]), TRANSFER);
  assert.equal(ctx.owner === 'replacement' ? ctx.api.capture_call(ctx.low, ctx.high, unit.generation, 2, stage.args.length) : ctx.api.capture_resident_call(ctx.low, ctx.high, unit.low, unit.high, 2, stage.args.length), 0);
  observe(ctx, `capture-${index}`); captures++; return token;
}
function complete(ctx, unit, stage, token, index) {
  if (stage.terminal) ctx.expected.set(record('R3EX', 40, [9, 0, 42, 0, 0, 0], 4), 56);
  else {
    const registers = [...stage.registers]; registers[0] = stage.result; registers[4] = TOP;
    ctx.expected.set(state(registers, ENTRY + stage.next)); ctx.expected.set(record('R3EX', 40, [3, 0, 0, 0, 0, 0], 3), 56);
  }
  assert.equal(ctx.owner === 'replacement' ? ctx.api.complete_windows_call(ctx.low, ctx.high, unit.generation, token) : ctx.api.complete_resident_windows_call(ctx.low, ctx.high, unit.low, unit.high, token), 0);
  observe(ctx, `complete-${index}`); completions++;
}
function diagnostic(ctx, address, name) {
  ctx.expected.set(record('R3MH', 40, [1, 0, 1, address, 1, 4]), 100);
  assert.equal(ctx.api.read32(address), 0); hostReads++; observe(ctx, name);
}
for (const [low, owner] of [[1, 'replacement'], [2, 'resident']]) {
  const ctx = fresh(owner, low); let gateUnit, callers = [], lastCaller;
  if (owner === 'resident') { gateUnit = compile(ctx, gates.map(([pc]) => pc), gates, 'gates'); callers = stages.map((stage, index) => compile(ctx, [ENTRY + stage.start], [], `caller-${index}`)); }
  for (const [index, stage] of stages.entries()) {
    const gate = gates[stage.gate], caller = owner === 'resident' ? callers[index] : compile(ctx, [ENTRY + stage.start, gate[0]], [gate], `caller-${index}`);
    generatedRun(ctx, caller, stage, false); const unit = owner === 'resident' ? gateUnit : caller;
    if (owner === 'resident') generatedRun(ctx, gateUnit, stage, true);
    const token = capture(ctx, unit, stage, index);
    if (index === 1) {
      const first = new Uint8Array(4096), second = new Uint8Array(4096); first.set(words([FIRST])); second.set(words([SECOND]));
      wholePage(ctx, ALLOCATION, first, 'written-first'); wholePage(ctx, ALLOCATION + 4096, second, 'written-second');
    }
    complete(ctx, unit, stage, token, index);
    if (index === 0 || index === 2) { wholePage(ctx, ALLOCATION, new Uint8Array(4096), `zero-${index}-first`); wholePage(ctx, ALLOCATION + 4096, new Uint8Array(4096), `zero-${index}-second`); }
    if (index === 1) { diagnostic(ctx, ALLOCATION, 'released-first-host-diagnostic'); diagnostic(ctx, ALLOCATION + 4096, 'released-second-host-diagnostic'); }
    lastCaller = caller;
  }
  const finalStack = new Uint8Array(4096); finalStack.set(words([ENTRY + 80, 0, 4097, ENTRY + 110, 42]), 4096 - 20);
  for (const [address, literal, name] of [[BASE, headerPage, 'final-header'], [ENTRY, codePage, 'final-code'], [BASE + 0x3000, dataPage, 'final-data'], [BASE + 0x5000, fixupPage, 'final-fixups'], [GATE, gatePage, 'final-gates'], [STACK, finalStack, 'final-stack']]) wholePage(ctx, address, literal, name);
  assert.equal(lastCaller.run(0xffffffff, 0xffffffff, 0, 0xffffffff), 21); observe(ctx, 'terminal-retained-generated-function'); controls.push({owner, name: 'terminal-retained-generated-function', status: 21});
  assert.equal(ctx.api.close(), 0); observe(ctx, 'close');
  assert.equal(lastCaller.run(0xffffffff, 0xffffffff, 0, 0xffffffff), 5); observe(ctx, 'closed-retained-generated-function'); controls.push({owner, name: 'closed-retained-generated-function', status: 5});
  rows.push({owner, actual_base: BASE, preferred_base: 0x400000, startup_calls: 1, host_cpu_seeds: 0, receipt_version: 5, receipt_bytes: 96, allocations: [ALLOCATION, ALLOCATION], allocation_size: 4097, allocation_pages: 2, released_host_faults: 2, genuine_guest_loads: 4, genuine_guest_data_stores: 4, guest_stack_writes: 16, guest_retired: 26, exit_code: 42, terminal_registers: stages[3].registers, terminal_pc: GATE + 32, terminal_flags: 2, dead_after_exit_canary: 0});
}
assert.equal(modules.length, 9); assert.equal(observations.length, 83); assert.equal(pages.length, 24); assert.equal(controls.length, 4);
assert.equal(generatedCalls, 12); assert.equal(guestRetired, 52); assert.equal(captures, 8); assert.equal(completions, 8); assert.equal(hostReads, 24612);
const sourcesAfter = sourceMap(); assert.deepEqual(sourcesAfter, sourcesBefore); save('source-hashes-after.json', JSON.stringify(sourcesAfter, null, 2));
const result = {status: 'PASS', engine_sha256: hash(engineBytes), pe_sha256: hash(image), program_sha256: hash(program), sources_before: sourcesBefore, sources_after: sourcesAfter, rows, modules, observations, pages, controls, counts: {contexts: 2, rust_startups: 2, host_cpu_seeds: 0, generated_calls: 12, guest_retired: 52, captures: 8, completions: 8, modules: 9, resident_peak: 5, controls: 4, whole_arena_observations: 83, arena_files: 166, whole_pages: 24, page_files: 48, host_read32: 24612, host_unmapped_faults: 4, source_routes: 91, raw_files: 231}, claim: 'two fresh authored relocated copied linked-v5 PE contexts use Rust-owned startup once each, with no host CPU seed; real named IAT Alloc/two-page guest stores+loads/Free/reused Alloc/guest zero loads+stores/ExitProcess42; four released-address failures are host read32 diagnostics, not faulting guest instructions or guest fault recovery; full independent arena model and complete declared page literals, whole authored input and receipt; typed engine ABI supplied by separate Root observer; no arbitrary PE/Windows/browser/SDK/performance/game/full generated-body proof'};
save('result.json', JSON.stringify(result, null, 2));
console.log(JSON.stringify({status: result.status, engine_sha256: result.engine_sha256, counts: result.counts, output}));
