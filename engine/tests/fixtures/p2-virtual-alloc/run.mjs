import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(root, 'expected engine wasm, output directory and repository root');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const engineBytes = readFileSync(enginePath), engineModule = new WebAssembly.Module(engineBytes);
assert.deepEqual(WebAssembly.Module.imports(engineModule), []);
const arenaSize = 4236, transfer = 140, controls = [], peRows = [], modules = [];
const apiNames = ['open', 'close', 'arena_ptr', 'map', 'protect', 'upload', 'read32', 'write8', 'compile_entries', 'compile_resident_entries', 'generation', 'module_ptr', 'module_len', 'guard', 'guard_resident', 'store32', 'store_resident32', 'begin_image_input', 'append_image_input', 'load_pe32_linked_v4_input_at', 'start_loaded_image', 'capture_call', 'capture_resident_call', 'complete_windows_call', 'complete_resident_windows_call'];
let ordinal = 0;
let guestRetired = 0, callCaptures = 0, callCompletions = 0;

function record(magic, length, fields, version = 1) {
  const bytes = new Uint8Array(length), view = new DataView(bytes.buffer);
  bytes.set(Buffer.from(magic)); view.setUint16(4, version, true); view.setUint16(6, 1, true); view.setUint32(8, length, true);
  fields.forEach((value, index) => view.setUint32(16 + index * 4, value, true));
  return bytes;
}
function words(fields) {
  const bytes = new Uint8Array(fields.length * 4), view = new DataView(bytes.buffer);
  fields.forEach((value, index) => view.setUint32(index * 4, value, true));
  return bytes;
}
function refresh(ctx) {
  ctx.bytes = new Uint8Array(ctx.memory.buffer); ctx.view = new DataView(ctx.memory.buffer);
  return ctx;
}
function snapshot(ctx) { return refresh(ctx).bytes.slice(ctx.base, ctx.base + arenaSize); }
function state(ctx) {
  refresh(ctx);
  return {registers: Array.from({length: 8}, (_, index) => ctx.view.getUint32(ctx.base + 16 + index * 4, true)), pc: ctx.view.getUint32(ctx.base + 48, true), flags: ctx.view.getUint32(ctx.base + 52, true)};
}
function request(ctx, bytes) {
  assert.ok(bytes.length <= 4096); refresh(ctx).bytes.set(bytes, ctx.base + transfer);
}
function fresh(owner, pages = 12) {
  const instance = new WebAssembly.Instance(engineModule, {});
  const api = Object.fromEntries(apiNames.map(name => [name, instance.exports[`ring3_abi_v1_${name}`]]));
  for (const [name, fn] of Object.entries(api)) assert.equal(typeof fn, 'function', name);
  const key = 0xa359000000000000n + BigInt(++ordinal);
  const ctx = {owner, memory: instance.exports.memory, api, low: Number(key & 0xffffffffn), high: Number(key >> 32n)};
  assert.equal(api.open(pages, ctx.low, ctx.high), 0); ctx.base = api.arena_ptr() >>> 0;
  return refresh(ctx);
}
function upload(ctx, address, bytes) {
  request(ctx, bytes); assert.equal(ctx.api.upload(address, bytes.length), 0); refresh(ctx);
}
function instantiate(ctx, bytes, label, binding = {}) {
  assert.ok(WebAssembly.validate(bytes));
  const module = new WebAssembly.Module(bytes), imports = WebAssembly.Module.imports(module);
  assert.equal(imports[0].module, 'env'); assert.equal(imports[0].name, 'memory');
  assert.equal(imports[0].kind, 'memory');
  const helpers = imports.slice(1).map(row => {
    assert.equal(row.module, 'ring3'); assert.equal(row.kind, 'function');
    assert.equal(typeof ctx.api[row.name], 'function'); return row.name;
  });
  const instance = new WebAssembly.Instance(module, {env: {memory: ctx.memory}, ring3: ctx.api});
  assert.equal(instance.exports.run.length, 4);
  writeFileSync(join(output, `${label}.wasm`), bytes);
  modules.push({label, owner: ctx.owner, sha256: hash(bytes), imports});
  return {...binding, run: instance.exports.run, helpers};
}
function compile(ctx, entries, gates, label) {
  request(ctx, words([...entries, ...gates.flat()]));
  let binding;
  if (ctx.owner === 'replacement') {
    assert.equal(ctx.api.compile_entries(entries.length, gates.length), 0);
    binding = {generation: ctx.api.generation(), pointer: ctx.api.module_ptr() >>> 0, length: ctx.api.module_len() >>> 0};
  } else {
    assert.equal(ctx.api.compile_resident_entries(entries.length, gates.length), 0); refresh(ctx);
    const at = ctx.base + transfer;
    assert.equal(ctx.view.getUint32(at, true), 1); assert.equal(ctx.view.getUint32(at + 4, true), 24);
    binding = {low: ctx.view.getUint32(at + 8, true), high: ctx.view.getUint32(at + 12, true), pointer: ctx.view.getUint32(at + 16, true), length: ctx.view.getUint32(at + 20, true)};
    assert.ok(binding.low !== 0 || binding.high !== 0);
  }
  refresh(ctx); assert.ok(binding.pointer > 0 && binding.length > 8 && binding.pointer + binding.length <= ctx.bytes.length);
  return instantiate(ctx, ctx.bytes.slice(binding.pointer, binding.pointer + binding.length), label, binding);
}
function read(ctx, address) {
  assert.equal(ctx.api.read32(address), 0); refresh(ctx);
  assert.equal(ctx.view.getUint32(ctx.base + 116, true), 0, `read ${address.toString(16)}`);
  return ctx.view.getUint32(ctx.base + 120, true);
}
function unmapped(ctx, address) {
  assert.equal(ctx.api.read32(address), 0);
  assert.deepEqual(snapshot(ctx).subarray(100, 140), record('R3MH', 40, [1, 0, 1, address, 1, 4]));
}
function readPage(ctx, address) {
  return Array.from({length: 1024}, (_, index) => read(ctx, address + index * 4));
}
function capture(ctx, unit, count) {
  const result = ctx.owner === 'replacement' ? ctx.api.capture_call(ctx.low, ctx.high, unit.generation, 2, count)
    : ctx.api.capture_resident_call(ctx.low, ctx.high, unit.low, unit.high, 2, count);
  assert.equal(result, 0); refresh(ctx);
  const bytes = ctx.bytes.slice(ctx.base + transfer, ctx.base + transfer + 112), view = new DataView(bytes.buffer);
  assert.equal(Buffer.from(bytes.subarray(0, 4)).toString(), 'R3CF');
  assert.equal(view.getUint32(4, true), 0x10001); assert.equal(view.getUint32(8, true), 112);
  callCaptures++;
  return {token: view.getUint32(16, true), bytes, view};
}
function complete(ctx, unit, token, keyLow = ctx.low) {
  return ctx.owner === 'replacement' ? ctx.api.complete_windows_call(keyLow, ctx.high, unit.generation, token)
    : ctx.api.complete_resident_windows_call(keyLow, ctx.high, unit.low, unit.high, token);
}
function reject(ctx, unit, token, expected, name, keyLow = ctx.low) {
  const before = snapshot(ctx);
  assert.equal(complete(ctx, unit, token, keyLow), expected, name);
  assert.deepEqual(snapshot(ctx), before, `${name}: full arena unchanged`);
  controls.push({owner: ctx.owner, name, status: expected});
}

const image = readFileSync(join(output, 'allocation.exe'));
assert.equal(image.length, 2560); assert.equal(image.subarray(0, 2).toString(), 'MZ');
const expectedProgram = 'ff155031400083f8000f85fb0000006a04680030000068010000006a00ff15583140003d000000100f85dc00000089c78b0783f8000f85cf000000c707df9b57138b073ddf9b57130f85bc000000ff155031400083f8000f85ad0000006a04680030000068011000006a00ff15583140003d000001100f858e00000089c68b0683f8000f85810000008b860010000083f8000f8572000000c78600100000e0ac68248b86001000003de0ac68240f85570000008b073ddf9b57130f854a0000006a04680030000068000001006a00ff155831400083f8000f852d000000ff155031400083f8080f851e000000ba0400c0c889150030400052ff1554314000c70504304000a5a5a5a50f0bbadec0adde89150030400052ff15543140000f0b';
assert.equal(image.subarray(0x200, 0x200 + expectedProgram.length / 2).toString('hex'), expectedProgram);
const failOffset = 266;
const stages = [
  {offset: 0, retired: 1, id: 0x10001, arguments: [], result: 0, fallthroughs: []},
  {offset: 6, retired: 7, id: 0x10005, arguments: [0, 1, 0x3000, 4], result: 0x10000000, fallthroughs: [15]},
  {offset: 35, retired: 11, id: 0x10001, arguments: [], result: 0, fallthroughs: [46, 59, 78]},
  {offset: 84, retired: 7, id: 0x10005, arguments: [0, 4097, 0x3000, 4], result: 0x10010000, fallthroughs: [93]},
  {offset: 113, retired: 21, id: 0x10005, arguments: [0, 65536, 0x3000, 4], result: 0, fallthroughs: [124, 137, 152, 179, 192]},
  {offset: 212, retired: 3, id: 0x10001, arguments: [], result: 8, fallthroughs: [221]},
  {offset: 227, retired: 6, id: 0x10003, arguments: [0xc8c00004], fallthroughs: [236]},
];
const branchEdges = [[9, 266, 15], [40, 266, 46], [53, 266, 59], [72, 266, 78], [87, 266, 93], [118, 266, 124], [131, 266, 137], [146, 266, 152], [173, 266, 179], [186, 266, 192], [215, 266, 221], [230, 266, 236]];
const allSeeds = [failOffset, ...stages.flatMap(stage => [stage.offset, ...stage.fallthroughs])];
for (const [offset, target, fallthrough] of branchEdges) {
  assert.equal(image.readUInt16LE(0x200 + offset), 0x850f);
  assert.equal(offset + 6, fallthrough); assert.equal(fallthrough + image.readInt32LE(0x202 + offset), target);
  assert.ok(allSeeds.includes(target) && allSeeds.includes(fallthrough));
}
assert.equal(stages.reduce((count, stage) => count + stage.retired, 0), 56);

function load(owner, base, label) {
  const ctx = fresh(owner);
  assert.equal(ctx.api.begin_image_input(image.length), 0); request(ctx, image);
  assert.equal(ctx.api.append_image_input(0, image.length), 0);
  refresh(ctx).bytes.fill(0xa5, ctx.base + transfer, ctx.base + arenaSize);
  assert.equal(ctx.api.load_pe32_linked_v4_input_at(base, 0x8000), 0); refresh(ctx);
  const receipt = ctx.bytes.slice(ctx.base + transfer, ctx.base + transfer + 88), view = new DataView(receipt.buffer);
  assert.equal(Buffer.from(receipt.subarray(0, 4)).toString(), 'R3LI');
  assert.equal(view.getUint32(4, true), 0x10004); assert.equal(view.getUint32(8, true), 88);
  assert.deepEqual(Array.from({length: 8}, (_, index) => view.getUint32(16 + index * 4, true)), [base, 0x6000, base + 0x1000, 5, 0x8000, 3, 0, 0]);
  const gates = Array.from({length: 5}, (_, index) => [view.getUint32(48 + index * 8, true), view.getUint32(52 + index * 8, true)]);
  assert.deepEqual(gates, [[0x8000, 0x10001], [0x8020, 0x10003], [0x8040, 0x10005], [0, 0], [0, 0]]);
  assert.deepEqual([read(ctx, base + 0x3150), read(ctx, base + 0x3154), read(ctx, base + 0x3158)], [0x8000, 0x8020, 0x8040], 'Rust resolves all three named IAT imports');
  const entry = base + 0x1000;
  const relocated = [[2, 0x3150], [31, 0x3158], [80, 0x3150], [109, 0x3158], [208, 0x3158], [223, 0x3150], [243, 0x3000], [250, 0x3154], [256, 0x3004], [273, 0x3000], [280, 0x3154]];
  for (const [offset, rva] of relocated) assert.equal(read(ctx, entry + offset), base + rva, 'independent relocation field');
  let gateUnit, callers = [];
  if (owner === 'resident') {
    // one shared failure target belongs to the gate unit; repeated ownership would overlap.
    gateUnit = compile(ctx, [...gates.slice(0, 3).map(gate => gate[0]), entry + failOffset], gates.slice(0, 3), `${label}-gates-failure`);
    callers = stages.map((stage, index) => compile(ctx, [stage.offset, ...stage.fallthroughs].map(offset => entry + offset), [], `${label}-caller-${index}`));
  }
  assert.equal(ctx.api.start_loaded_image(0x70000, 1), 0);
  assert.deepEqual(state(ctx), {registers: [0, 0, 0, 0, 0x71000, 0, 0, 0], pc: entry, flags: 2});
  return {ctx, base, entry, receipt, gates: gates.slice(0, 3), gateUnit, callers, label};
}
function callerFor(path, index) {
  const {ctx, entry, gates, label} = path, stage = stages[index];
  if (ctx.owner === 'resident') return path.callers[index];
  const gate = gates.find(gate => gate[1] === stage.id);
  const offsets = [stage.offset, ...stage.fallthroughs, ...(stage.fallthroughs.length ? [failOffset] : [])];
  assert.ok(offsets.length + 1 <= 8);
  return compile(ctx, [...offsets.map(offset => entry + offset), gate[0]], [gate], `${label}-caller-${index}`);
}
function expectedGateState(index) {
  const eax = [0, 0, 0x13579bdf, 0, 0x13579bdf, 0, 8][index];
  const edx = index === 6 ? 0xc8c00004 : 0;
  const esi = index >= 4 ? 0x10010000 : 0, edi = index >= 2 ? 0x10000000 : 0;
  const count = stages[index].arguments.length;
  const pc = stages[index].id === 0x10005 ? 0x8040 : stages[index].id === 0x10003 ? 0x8020 : 0x8000;
  return {registers: [eax, 0, edx, 0, 0x71000 - 4 * (count + 1), 0, esi, edi], pc, flags: index === 0 ? 2 : 0x46};
}
function runToGate(path, index, scheduling = false) {
  const {ctx} = path, stage = stages[index], caller = callerFor(path, index);
  if (scheduling) {
    for (const cancel of [0, 1]) {
      refresh(ctx).view.setUint32(ctx.base + 96, cancel, true);
      const before = snapshot(ctx), expected = before.slice();
      expected.set(record('R3EX', 40, [cancel ? 2 : 1, 0, 0, 0, 0, 0], ctx.owner === 'replacement' ? 3 : 2), 56);
      assert.equal(caller.run(ctx.base, ctx.base + 56, 0, ctx.base + 96), 0);
      assert.deepEqual(snapshot(ctx), expected); controls.push({owner: ctx.owner, name: cancel ? 'cancel_before_retirement' : 'zero_budget', status: 0});
    }
    refresh(ctx).view.setUint32(ctx.base + 96, 0, true);
  }
  assert.equal(caller.run(ctx.base, ctx.base + 56, 64, ctx.base + 96), 0);
  const observed = state(ctx), expected = expectedGateState(index);
  assert.deepEqual(observed, expected, `${path.label} stage${index}: genuine CALL state`);
  assert.equal(refresh(ctx).view.getUint32(ctx.base + 76, true), stage.retired, 'literal instruction count');
  guestRetired += ctx.view.getUint32(ctx.base + 76, true);
  let unit = caller;
  if (ctx.owner === 'resident') {
    assert.deepEqual(snapshot(ctx).subarray(56, 96), record('R3EX', 40, [3, stage.retired, 0, 0, 0, 0], 2));
    unit = path.gateUnit; assert.equal(unit.run(ctx.base, ctx.base + 56, 1, ctx.base + 96), 0);
  }
  assert.deepEqual(snapshot(ctx).subarray(56, 96), record('R3EX', 40, [8, ctx.owner === 'resident' ? 0 : stage.retired, stage.id, 0, 0, 0], 3));
  const returnOffset = index === 6 ? 254 : stages[index + 1].offset;
  assert.equal(read(ctx, expected.registers[4]), path.entry + returnOffset, 'guest CALL wrote the genuine return address');
  stage.arguments.forEach((argument, slot) => assert.equal(read(ctx, expected.registers[4] + 4 + slot * 4), argument, 'guest PUSH argument order'));
  const captured = capture(ctx, unit, stage.arguments.length);
  assert.equal(captured.view.getUint32(20, true), stage.id); assert.equal(captured.view.getUint32(24, true), 2);
  assert.equal(captured.view.getUint32(28, true), stage.arguments.length);
  assert.equal(captured.view.getUint32(32, true), expected.pc); assert.equal(captured.view.getUint32(36, true), expected.registers[4]);
  assert.equal(captured.view.getUint32(40, true), path.entry + returnOffset); assert.equal(captured.view.getUint32(44, true), 0);
  assert.deepEqual(Array.from({length: 16}, (_, slot) => captured.view.getUint32(48 + slot * 4, true)), [...stage.arguments, ...Array(16 - stage.arguments.length).fill(0)]);
  return {unit, caller, token: captured.token, observed, returnOffset};
}
function controlsBeforeAllocation(path, pending) {
  const {ctx} = path, {unit, token} = pending;
  reject(ctx, unit, token, 3, 'wrong_key', ctx.low ^ 1);
  reject(ctx, unit, token + 1, 14, 'wrong_token');
  refresh(ctx).view.setUint32(ctx.base + 96, 1, true);
  reject(ctx, unit, 0, 14, 'token_before_cancel');
  refresh(ctx).bytes[ctx.base + 16] ^= 1;
  reject(ctx, unit, token, 15, 'changed_state_before_cancel'); refresh(ctx).bytes[ctx.base + 16] ^= 1;
  reject(ctx, unit, token, 16, 'cancel_before_allocate'); refresh(ctx).view.setUint32(ctx.base + 96, 0, true);
  unmapped(ctx, 0x10000000);
}
function declaredRam(path, index) {
  const {ctx, base} = path;
  const addresses = [base + 0x3000, base + 0x3004, ...Array.from({length: 17}, (_, slot) => 0x70fbc + slot * 4)];
  if (index >= 2) addresses.push(0x10000000, 0x10000ffc);
  if (index >= 4) addresses.push(0x10010000, 0x10010ffc, 0x10011000, 0x10011ffc);
  return {addresses, values: addresses.map(address => read(ctx, address))};
}
function finishCall(path, index, pending) {
  const {ctx} = path, stage = stages[index], ramBefore = declaredRam(path, index);
  const expected = snapshot(ctx), next = {...pending.observed, registers: [...pending.observed.registers]};
  if (index === 6) expected.set(record('R3EX', 40, [9, 0, 0xc8c00004, 0, 0, 0], 4), 56);
  else {
    next.registers[0] = stage.result; next.registers[4] = 0x71000; next.pc = path.entry + pending.returnOffset;
    expected.set(record('R3ST', 56, [...next.registers, next.pc, next.flags]), 0);
    expected.set(record('R3EX', 40, [3, 0, 0, 0, 0, 0], 3), 56);
  }
  assert.equal(complete(ctx, pending.unit, pending.token), 0);
  callCompletions++;
  assert.deepEqual(snapshot(ctx), expected, `${path.label} stage${index}: only prescribed state/exit fields published`);
  assert.deepEqual(ramBefore.addresses.map(address => read(ctx, address)), ramBefore.values, 'provider preserves declared existing RAM');
  if (index === 1 || index === 3) {
    const address = stage.result, pages = index === 1 ? 1 : 2;
    for (let page = 0; page < pages; page++) assert.deepEqual(readPage(ctx, address + page * 4096), Array(1024).fill(0), 'all newly allocated words are zero');
    unmapped(ctx, address + pages * 4096);
    request(ctx, words([address])); const before = snapshot(ctx), generation = ctx.api.generation();
    // use the replacement cold compiler: the resident registry already has all eight units.
    assert.equal(ctx.api.compile_entries(1, 0), 10, 'RW allocation has no execute permission');
    assert.deepEqual(snapshot(ctx), before); assert.equal(ctx.api.generation(), generation);
    controls.push({owner: ctx.owner, name: `allocation_${pages}_pages_no_execute`, status: 10});
  }
  if (index === 4) { unmapped(ctx, 0x10020000); controls.push({owner: ctx.owner, name: 'capacity_null_no_new_mapping', status: 0}); }
}
for (const base of [0x400000, 0x500000]) for (const owner of ['replacement', 'resident']) {
  const path = load(owner, base, `pe-${owner}-${base.toString(16)}`);
  let retired = 0, lastCaller;
  for (let index = 0; index < stages.length; index++) {
    const pending = runToGate(path, index, index === 0); retired += stages[index].retired;
    if (index === 1) controlsBeforeAllocation(path, pending);
    finishCall(path, index, pending); lastCaller = pending.caller;
  }
  const {ctx} = path;
  assert.equal(read(ctx, base + 0x3000), 0xc8c00004); assert.equal(read(ctx, base + 0x3004), 0, 'terminal canary stays zero');
  assert.equal(read(ctx, 0x10000000), 0x13579bdf); assert.equal(read(ctx, 0x10011000), 0x2468ace0, 'guest wrote the rounded second page');
  assert.equal(lastCaller.run(0xffffffff, 0xffffffff, 0, 0xffffffff), 21, 'terminal latch precedes malformed pointers');
  peRows.push({owner, base, guest_retired: retired, allocations: [{size: 1, base: 0x10000000, pages: 1}, {size: 4097, base: 0x10010000, pages: 2}], capacity_request: 65536, capacity_result: 0, last_error: 8, branch_result: 0xc8c00004, canary: 0, receipt_sha256: hash(path.receipt)});
  assert.equal(ctx.api.close(), 0); assert.equal(lastCaller.run(0xffffffff, 0xffffffff, 0, 0xffffffff), 5);
  controls.push({owner, name: 'closed_after_terminal', status: 5});
}
for (const owner of ['replacement', 'resident']) {
  const path = load(owner, 0x400000, `pending-${owner}`);
  finishCall(path, 0, runToGate(path, 0)); const pending = runToGate(path, 1);
  const staleAddress = owner === 'replacement' ? path.entry + 6 : 0x8040;
  assert.equal(path.ctx.api.protect(staleAddress & 0xfffff000, 1, 7), 0);
  reject(path.ctx, pending.unit, pending.token, 4, 'stale_pending_allocation');
  unmapped(path.ctx, 0x10000000);
  assert.equal(path.ctx.api.close(), 0); reject(path.ctx, pending.unit, pending.token, 5, 'closed_pending_allocation');
}
const sourcePaths = ['Cargo.toml', 'Cargo.lock', 'engine/Cargo.toml', 'engine/src/windows/provider.rs', 'engine/src/process/windows.rs', 'engine/src/process/call.rs', 'engine/src/memory/space.rs', 'engine/src/loader/pe32.rs', 'engine/src/loader/mod.rs', 'engine/src/loader/imports.rs', 'engine/src/process/image.rs', 'engine/src/process/image_input.rs', 'engine/src/process/wasm.rs', 'engine/src/abi/wasm/exports.rs', 'engine/tests/windows_virtual_alloc.rs', 'engine/tests/windows_virtual_alloc_wasm.rs', 'engine/tests/fixtures/p2-virtual-alloc/run.mjs', 'engine/tests/support/pe32.rs'];
assert.equal(peRows.length, 4);
assert.equal(guestRetired, 240); assert.equal(callCaptures, 32); assert.equal(callCompletions, 30);
const result = {status: 'ok', engine_sha256: hash(engineBytes), pe_sha256: hash(image), source_sha256: Object.fromEntries(sourcePaths.map(path => [path, hash(readFileSync(join(root, path)))])), tools: {node: process.version, v8: process.versions.v8}, policy: 'NULL address, size1..65536, reserve+commit0x3000, readwrite4; first fit64KiB in10000000..70000000, page rounding4096; capacity NULL withLastError8; no execute permission', counts: {pe_paths: peRows.length, pe_guest_retired: peRows.reduce((count, row) => count + row.guest_retired, 0), total_guest_retired: guestRetired, call_captures: callCaptures, call_completions: callCompletions, control_probes: controls.length}, pe: peRows, controls, modules, claim: 'four authored copied-PE engine-Wasm bound paths with genuine CALL/zero/load/store/branch/provider/ExitProcess execution, plus two partial copied-PE pending-allocation stale/Closed controls; native tests cover wider invalid shapes and private failure seams; no full Win32 allocator, VirtualFree/Protect, browser, SDK, performance or playable-game claim'};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2));
console.log(JSON.stringify({status: result.status, engine_sha256: result.engine_sha256, counts: result.counts, output}));
