import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';

const SIZE = 4612, TRANSFER = 140, FP = 4236, EXIT = 0x8020, EXIT_ID = 0x10003, MARKER = 0xc8c00015;
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const words = fields => {const b = Buffer.alloc(fields.length * 4); fields.forEach((v, i) => b.writeUInt32LE(v >>> 0, i * 4)); return b;};
function record(magic, length, fields, version = 1) {
  const b = Buffer.alloc(length); b.write(magic); b.writeUInt16LE(version, 4); b.writeUInt16LE(1, 6); b.writeUInt32LE(length, 8);
  fields.forEach((value, index) => b.writeUInt32LE(value >>> 0, 16 + index * 4)); return b;
}
const state = (registers, pc, flags) => record('R3ST', 56, [...registers, pc, flags]);
const exit = (reason, retired, version, detail = 0) => record('R3EX', 40, [reason, retired, detail, 0, 0, 0], version);
const program = 'b8ff7f3412b980ff4523bba5a56745baa5a55634bee03f4000'
  + '38c80f923e0f9066010f9946020f9b460372567054'
  + '38cc0f9246040f9046050f9846060f9a460779407b3e'
  + '38c90f94443e080f9505e93f40000f9c460a0f9f460b'
  + '8a460880f8017520b0ffbf1500c0c8893d0030400057ff1550314000'
  + 'c70504304000a5a5a5a50f0bbfdec0adde893d0030400057ff15503140000f0b';
const callerOffsets = [0, 44, 46, 66, 68, 98, 130], callerLengths = [44, 2, 20, 2, 30, 20, 18];
const relocationOffsets = [21, 78, 107, 114, 120, 137, 144];
const relocationRvas = [0x3fe0, 0x3fe9, 0x3000, 0x3150, 0x3004, 0x3000, 0x3150];
const finalBytes = [0, 0, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0];
function literalImage() {
  const b = Buffer.alloc(0x1600); b.write('MZ'); b.writeUInt32LE(0x80, 0x3c); b.write('PE\0\0', 0x80);
  for (const [at, value] of [[0x84, 0x14c], [0x86, 3], [0x94, 224], [0x96, 0x102], [0x98, 0x10b], [0xdc, 3], [0xde, 0x100]]) b.writeUInt16LE(value, at);
  for (const [at, value] of [[4, 0x200], [8, 0x1200], [16, 0x1000], [20, 0x1000], [24, 0x3000], [28, 0x400000], [32, 4096], [36, 512], [56, 0x6000], [60, 512], [72, 0x100000], [76, 4096], [80, 0x100000], [84, 4096], [92, 16], [104, 0x3100], [108, 40], [136, 0x5000], [140, 24], [192, 0x3150], [196, 8]]) b.writeUInt32LE(value, 0x98 + at);
  for (const [index, name, size, rva, rawSize, rawPointer, flags] of [[0, '.text', 150, 0x1000, 512, 0x200, 0x60000020], [1, '.data', 4096, 0x3000, 4096, 0x400, 0xc0000040], [2, '.fixups', 24, 0x5000, 512, 0x1400, 0x40000040]]) {
    const at = 0x178 + index * 40; b.write(name, at);
    for (const [offset, value] of [[8, size], [12, rva], [16, rawSize], [20, rawPointer], [36, flags]]) b.writeUInt32LE(value, at + offset);
  }
  b.fill(0xcc, 0x200, 0x400); b.set(Buffer.from(program, 'hex'), 0x200);
  for (let index = 0; index < 32; index++) b[0x13e0 + index] = index + 1;
  b.set(words([0x3140, 0, 0, 0x3160, 0x3150]), 0x500); b.writeUInt32LE(0x3170, 0x540); b.writeUInt32LE(0x3170, 0x550);
  b.write('kernel32.dll\0', 0x560); b.write('ExitProcess\0', 0x572);
  b.writeUInt32LE(0x1000, 0x1400); b.writeUInt32LE(24, 0x1404);
  relocationOffsets.forEach((offset, index) => b.writeUInt16LE(0x3000 + offset, 0x1408 + index * 2));
  return b;
}

export function runPeProof({engine, output, initial}) {
  assert.equal(initial.length, SIZE); assert.deepEqual(WebAssembly.Module.imports(engine.module), []);
  const image = readFileSync(join(output, 'conditions.exe'));
  assert.deepEqual(image, literalImage(), 'independent5632-byte PE image including all headers, data, imports and relocations');
  const text = Buffer.from(program, 'hex'); assert.equal(text.length, 150);
  for (const [offset, opcode, target, fallthrough] of [[42, 0x72, 130, 44], [44, 0x70, 130, 46], [64, 0x79, 130, 66], [66, 0x7b, 130, 68], [96, 0x75, 130, 98]]) {
    assert.equal(text[offset], opcode); assert.equal(offset + 2, fallthrough); assert.equal(offset + 2 + text.readInt8(offset + 1), target);
    assert.ok(callerOffsets.includes(target) && callerOffsets.includes(fallthrough));
  }
  const contexts = [], modules = [], observations = [], helper_trace = [];
  const counts = {contexts: 0, modules: 0, runs: 0, retired: 0, helper_calls: 0, provider_completions: 0, terminal_rejects: 0, diagnostic_reads: 0, protection_probes: 0};
  const save = (name, bytes) => writeFileSync(join(output, name), bytes, {flag: 'wx'});
  save('pe-program.x86', text);
  const arena = c => Buffer.from(new Uint8Array(c.memory.buffer, c.arena, SIZE));
  function input(c, offset, bytes) {
    const expected = arena(c); expected.set(bytes, offset); new Uint8Array(c.memory.buffer).set(bytes, c.arena + offset);
    assert.deepEqual(arena(c), expected, 'declared PE host input retains remaining full4612 arena');
  }
  function pure(c, name, args) {
    const before = arena(c); assert.equal(c.api[name](...args), 0, name); assert.deepEqual(arena(c), before, name + ' full4612 neutrality');
  }
  function read(c, address, length, expectedBytes) {
    const observed = Buffer.alloc(length);
    for (let offset = 0; offset < length; offset++) {
      const expected = arena(c), helper = record('R3MH', 40, [0, expectedBytes[offset], 0, 0, 0, 1], 2); expected.set(helper, 100);
      assert.equal(c.api.read8(address + offset), 0); assert.deepEqual(arena(c), expected, 'PE selected byte diagnostic retains full4612 outside helper');
      observed[offset] = arena(c).readUInt32LE(120); counts.diagnostic_reads++;
    }
    assert.deepEqual(observed, Buffer.from(expectedBytes)); return observed;
  }
  function readWord(c, address, value) {
    const expected = arena(c), helper = record('R3MH', 40, [0, value, 0, 0, 0, 0]); expected.set(helper, 100);
    assert.equal(c.api.read32(address), 0); assert.deepEqual(arena(c), expected); counts.diagnostic_reads++;
    return arena(c).readUInt32LE(120);
  }
  function compile(c, label, offsets, lengths, withGate) {
    const pcs = offsets.map(offset => c.entry + offset), specs = pcs.map((pc, i) => [pc, lengths[i]]);
    if (withGate) {pcs.push(EXIT); specs.push([EXIT, 2]);}
    const gates = withGate ? [EXIT, EXIT_ID] : [];
    assert.ok(pcs.length > 0 && pcs.length <= 8 && ++c.captures <= 2);
    input(c, TRANSFER, words([...(c.entries ? pcs : specs.flat()), ...gates]));
    const expected = arena(c), method = c.owner === 'replacement'
      ? (c.entries ? 'compile_entries' : 'compile_with_gates') : (c.entries ? 'compile_resident_entries' : 'compile_resident_with_gates');
    assert.equal(c.api[method](pcs.length, Number(withGate)), 0, method);
    const b = arena(c), binding = c.owner === 'replacement'
      ? {generation: c.api.generation(), pointer: c.api.module_ptr() >>> 0, length: c.api.module_len() >>> 0}
      : {low: b.readUInt32LE(148), high: b.readUInt32LE(152), pointer: b.readUInt32LE(156), length: b.readUInt32LE(160)};
    if (c.owner === 'resident') {assert.deepEqual(b.subarray(140, 148), words([1, 24])); expected.set(b.subarray(140, 164), 140);}
    assert.deepEqual(arena(c), expected, 'PE compilation changes only resident receipt');
    assert.ok(binding.pointer > 0 && binding.length > 8 && binding.length <= 65536);
    const bytes = Buffer.from(new Uint8Array(c.memory.buffer, binding.pointer, binding.length)), module = new WebAssembly.Module(bytes);
    const guard = c.owner === 'replacement' ? 'guard' : 'guard_resident';
    const helperNames = label === 'gate' ? [] : ['read32', c.owner === 'replacement' ? 'store32' : 'store_resident32', 'read8', c.owner === 'replacement' ? 'store8' : 'store_resident8'];
    const imports = [{module: 'env', name: 'memory', kind: 'memory'}, ...[guard, ...helperNames].map(name => ({module: 'ring3', name, kind: 'function'}))];
    assert.deepEqual(WebAssembly.Module.imports(module), imports); assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
    const forwarded = {[guard]: c.api[guard]};
    for (const name of helperNames) forwarded[name] = (...args) => {
      const before = arena(c), status = c.api[name](...args), after = arena(c), expected = Buffer.from(before); expected.set(after.subarray(100, 140), 100);
      assert.equal(status, 0); assert.deepEqual(after, expected, 'forwarded PE guest helper touches only canonical40-byte packet');
      helper_trace.push({context: c.id, run: counts.runs + 1, unit: label, name, args: args.map(v => v >>> 0), status, helper_hex: after.subarray(100, 140).toString('hex')});
      return status;
    };
    const instance = new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3: forwarded}); assert.equal(instance.exports.run.length, 4);
    const file = `pe-module-${modules.length + 1}.wasm`; save(file, bytes);
    const instructions = label === 'gate' ? 1 : 36 + Number(withGate);
    assert.ok(instructions <= 64);
    modules.push({context: c.id, owner: c.owner, entries: c.entries, label, file, sha256: hash(bytes), bytes: bytes.length, blocks: pcs.length, instructions, imports, ...binding});
    return {...binding, file, bytes, run: instance.exports.run};
  }
  function run(c, unit, label, {reason, retired, version, registers, pc, flags, status = 0, detail = 0, helper} = {}) {
    const expected = arena(c);
    if (status === 0) {expected.set(state(registers, pc, flags)); expected.set(exit(reason, retired, version, detail), 56); if (helper) expected.set(helper, 100);}
    const observedStatus = status === 0 ? unit.run(c.arena, c.arena + 56, 64, c.arena + 96) : unit.run(0xffffffff, 0xffffffff, 0, 0xffffffff);
    assert.equal(observedStatus, status); assert.deepEqual(arena(c), expected, 'PE run full4612 CPU/FLAGS/helper/dirtyFP: ' + label);
    const after = arena(c); observations.push({context: c.id, module: unit.file, label, expected_status: status, observed_status: observedStatus,
      expected_reason: status === 0 ? reason : null, observed_reason: status === 0 ? after.readUInt32LE(72) : null,
      expected_retired: status === 0 ? retired : null, observed_retired: status === 0 ? after.readUInt32LE(76) : null,
      expected_pc: status === 0 ? pc : null, observed_pc: status === 0 ? after.readUInt32LE(48) : null,
      expected_flags: status === 0 ? flags : null, observed_flags: status === 0 ? after.readUInt32LE(52) : null});
    counts.runs++; if (status === 0) counts.retired += after.readUInt32LE(76);
  }
  const arities = {open: 3, close: 0, arena_ptr: 0, begin_image_input: 1, append_image_input: 2, load_pe32_linked_v2_input_at: 2,
    start_loaded_image: 2, compile_entries: 2, compile_with_gates: 2, compile_resident_entries: 2, compile_resident_with_gates: 2,
    generation: 0, module_ptr: 0, module_len: 0, guard: 6, guard_resident: 7, read8: 1, read32: 1, write32: 2,
    store8: 2, store_resident8: 6, store32: 2, store_resident32: 6, capture_call: 5, capture_resident_call: 6,
    complete_windows_call: 4, complete_resident_windows_call: 5};
  for (const base of [0x400000, 0x500000]) for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
    const id = `pe-${++counts.contexts}`, instance = new WebAssembly.Instance(engine.module, {});
    const c = {id, owner, entries, image_base: base, entry: base + 0x1000, memory: instance.exports.memory, captures: 0, low: 0x50450000 + counts.contexts, high: 0xc8400000};
    c.api = Object.fromEntries(Object.entries(arities).map(([name, arity]) => {const fn = instance.exports['ring3_abi_v1_' + name]; assert.equal(typeof fn, 'function', name); assert.equal(fn.length, arity); return [name, fn];}));
    assert.equal(c.api.open(8, c.low, c.high), 0); c.arena = c.api.arena_ptr() >>> 0;
    pure(c, 'begin_image_input', [image.length]);
    for (const [offset, length] of [[0, 4096], [4096, 1536]]) {input(c, TRANSFER, image.subarray(offset, offset + length)); pure(c, 'append_image_input', [offset, length]); input(c, TRANSFER, Buffer.alloc(4096, 0xa5));}
    const receipt = record('R3LI', 72, [base, 0x6000, c.entry, 5, 0x8000, 1, 0, 0, EXIT, EXIT_ID, 0, 0, 0, 0], 2), expectedLoad = arena(c); expectedLoad.set(receipt, TRANSFER);
    assert.equal(c.api.load_pe32_linked_v2_input_at(base, 0x8000), 0); assert.deepEqual(arena(c), expectedLoad); save(`${id}-receipt.bin`, receipt);
    readWord(c, base + 0xb4, 0x400000); readWord(c, base + 0x3150, EXIT); readWord(c, base + 0x3154, 0);
    relocationOffsets.forEach((offset, index) => readWord(c, c.entry + offset, base + relocationRvas[index]));
    read(c, base + 0x3000, 32, Buffer.alloc(32)); read(c, base + 0x3fe0, 32, Buffer.from(Array.from({length: 32}, (_, i) => i + 1)));
    for (const address of [base, c.entry, base + 0x5000, EXIT]) {
      const expected = arena(c); expected.set(record('R3MH', 40, [1, 0, 2, address, 2, 4]), 100);
      assert.equal(c.api.write32(address, 0xa5a5a5a5), 0); assert.deepEqual(arena(c), expected, 'READ/RX image/gate rejects writes'); counts.protection_probes++;
    }
    const expectedMissing = arena(c); expectedMissing.set(record('R3MH', 40, [1, 0, 1, base + 0x4000, 1, 1], 2), 100);
    assert.equal(c.api.read8(base + 0x4000), 0); assert.deepEqual(arena(c), expectedMissing); counts.diagnostic_reads++;
    const expectedStart = arena(c); expectedStart.set(state([0, 0, 0, 0, 0x71000, 0, 0, 0], c.entry, 2)); expectedStart.set(exit(3, 0, 3), 56);
    assert.equal(c.api.start_loaded_image(0x70000, 1), 0); assert.deepEqual(arena(c), expectedStart);
    input(c, FP, initial.subarray(FP));
    const caller = compile(c, 'caller', callerOffsets, callerLengths, owner === 'replacement');
    const gate = owner === 'replacement' ? caller : compile(c, 'gate', [], [], true);
    const registers = [0x12347fff, 0x2345ff80, 0x3456a5a5, 0x4567a5a5, 0x70ff8, 0, base + 0x3fe0, MARKER];
    const helperStart = helper_trace.length;
    run(c, caller, '32 genuine SETcc/consumer/call instructions from startup', {reason: owner === 'replacement' ? 8 : 3, retired: 32, version: owner === 'replacement' ? 3 : 2, registers, pc: EXIT, flags: 0x46, detail: owner === 'replacement' ? EXIT_ID : 0, helper: record('R3MH', 40, [0, 0, 0, 0, 0, 0])});
    const expectedTrace = finalBytes.map((value, i) => ({name: owner === 'replacement' ? 'store8' : 'store_resident8', address: base + 0x3fe0 + i, value, version: 3}));
    expectedTrace.push({name: 'read8', address: base + 0x3fe8, value: 1, version: 2},
      ...[[base + 0x3000, MARKER], [0x70ffc, MARKER]].map(([address, value]) => ({name: owner === 'replacement' ? 'store32' : 'store_resident32', address, value, version: 1})),
      {name: 'read32', address: base + 0x3150, value: EXIT, version: 1},
      {name: owner === 'replacement' ? 'store32' : 'store_resident32', address: 0x70ff8, value: c.entry + 118, version: 1});
    const events = helper_trace.slice(helperStart); assert.equal(events.length, expectedTrace.length);
    events.forEach((event, i) => {
      const expected = expectedTrace[i], store = expected.name.startsWith('store'), args = event.args;
      assert.equal(event.name, expected.name); assert.equal(args[args.length - (store ? 2 : 1)], expected.address); if (store) assert.equal(args.at(-1), expected.value);
      assert.equal(event.helper_hex, record('R3MH', 40, [0, store ? 0 : expected.value, 0, 0, 0, expected.version === 1 ? 0 : 1], expected.version).toString('hex'));
    });
    if (owner === 'resident') run(c, gate, 'named ExitProcess gate after cold lookup', {reason: 8, retired: 0, version: 3, detail: EXIT_ID, registers, pc: EXIT, flags: 0x46});
    const frame = record('R3CF', 112, [1, EXIT_ID, 2, 1, EXIT, 0x70ff8, c.entry + 118, 0, MARKER, ...Array(15).fill(0)]), expectedCapture = arena(c); expectedCapture.set(frame, TRANSFER);
    assert.equal(owner === 'replacement' ? c.api.capture_call(c.low, c.high, gate.generation, 2, 1) : c.api.capture_resident_call(c.low, c.high, gate.low, gate.high, 2, 1), 0);
    assert.deepEqual(arena(c), expectedCapture); save(`${id}-frame.bin`, frame);
    const expectedComplete = arena(c); expectedComplete.set(exit(9, 0, 4, MARKER), 56);
    assert.equal(owner === 'replacement' ? c.api.complete_windows_call(c.low, c.high, gate.generation, 1) : c.api.complete_resident_windows_call(c.low, c.high, gate.low, gate.high, 1), 0);
    assert.deepEqual(arena(c), expectedComplete); counts.provider_completions++;
    read(c, base + 0x3000, 32, words([MARKER, 0, 0, 0, 0, 0, 0, 0]));
    const destination = read(c, base + 0x3fe0, 32, Buffer.from([...finalBytes, ...Array.from({length: 20}, (_, i) => i + 13)]));
    read(c, 0x70fe0, 32, words([0, 0, 0, 0, 0, 0, c.entry + 118, MARKER]));
    save(`${id}-destination.bin`, destination); save(`${id}-terminal-arena.bin`, arena(c));
    for (const unit of new Set([caller, gate])) assert.deepEqual(Buffer.from(new Uint8Array(c.memory.buffer, unit.pointer, unit.length)), unit.bytes);
    run(c, caller, 'terminal latch before malformed pointers and zero budget', {status: 21}); counts.terminal_rejects++;
    contexts.push({id, owner, entries, image_base: base, entry: c.entry, captures: c.captures, mapped_pages: 6, guest_retired: 32, checksum: MARKER,
      final_registers: registers, final_pc: EXIT, final_flags: 0x46, selected_destination_sha256: hash(destination), canary: 0, receipt_sha256: hash(receipt), frame_sha256: hash(frame)});
    pure(c, 'close', []);
  }
  counts.modules = modules.length; counts.helper_calls = helper_trace.length;
  assert.deepEqual(counts, {contexts: 8, modules: 12, runs: 20, retired: 256, helper_calls: 136, provider_completions: 8, terminal_rejects: 8, diagnostic_reads: 1368, protection_probes: 32});
  return {contexts, modules, observations, helper_trace, counts, image_sha256: hash(image), program_sha256: hash(text),
    claim: 'same independently reconstructed5632-byte PE/150-byte program at preferred/relocated bases in allfourboundprofiles; genuine startup, no CPU reseed, dirtyFPafterstartup;32 guest retirements/path, named terminal ExitProcess, observed forward-only guest helpers; full4612 run assertions and selected32-byte destination/data/stack diagnostics, no fullRAM rollback claim'};
}
