import {ARENA_SIZE} from '../../../../../../../support/wasm/arena.mjs';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../../../../../../../support/wasm/engine.mjs';

const [enginePath, output, root] = process.argv.slice(2), engine = readEngine(enginePath);
const PC = 0x1000, SEG = 4524, SIZE = ARENA_SIZE, keyHigh = 0x5350414e;
const defaults = [0xf33fffff, 0xf3bfffff, 0xf33fffff, 0xf33fffff, 0xf33fffff, 0];
const offsets = [28, 32, 36, 40, 44, 56];
const counts = {contexts: 0, modules: 0, runs: 0, faults: 0, successes: 0, controls: 0};
const FP = 4236, SV = 4364, T = 140, DATA = 0x5010;
const paths = ['engine/tests/support/wasm/arena.mjs', 'engine/src/cpu/dbt/wasm/instructions/simd.rs', 'engine/src/cpu/dbt/wasm/instructions/segments/access.rs', 'engine/src/process/memory/span.rs', 'engine/src/cpu/x86/simd/mod.rs', 'engine/src/cpu/x86/simd/state.rs', 'engine/src/cpu/x86/simd/operation.rs', 'engine/src/cpu/x86/simd/descriptor.rs', 'engine/src/cpu/x86/simd/execute.rs', 'engine/src/cpu/x86/xstate/fxsave.rs', 'engine/src/process/cpu/simd/arena.rs', 'engine/src/cpu/x86/xstate/legacy.rs',
  'engine/tests/cpu/simd/state/legacy/native/segments.rs', 'engine/tests/cpu/simd/state/legacy/wasm/segments.rs',
  'engine/tests/cpu/simd/state/legacy/fixtures/p2-fx-segment-access/runtime/run.mjs'];
const pins = () => Object.fromEntries(paths.map(p => [p, createHash('sha256').update(readFileSync(join(root, p))).digest('hex')])), sourcePins = pins();
const words = values => {const b = Buffer.alloc(values.length * 4); values.forEach((v, i) => b.writeUInt32LE(v >>> 0, i * 4)); return b;};
function record(magic, fields, version = 1, size = 40) {
  const b = Buffer.alloc(size); b.write(magic); b.writeUInt16LE(version, 4); b.writeUInt16LE(1, 6); b.writeUInt32LE(size, 8); b.set(words(fields), 16); return b;
}
const arena = c => Buffer.from(new Uint8Array(c.memory.buffer, c.base, SIZE));
const input = (c, at, b) => new Uint8Array(c.memory.buffer).set(b, c.base + at);
function host(c, name, args = []) {assert.equal(c.api[name](...args), 0, name);}
function upload(c, address, b) {input(c, 140, b); host(c, 'upload', [address, b.length]);}
function byte(c, address) {host(c, 'read8', [address]); return arena(c).readUInt32LE(120);}
function open(owner, entries) {
  const instance = new WebAssembly.Instance(engine.module, {}), c = {owner, entries, memory: instance.exports.memory, api: {}, calls: []};
  for (const [name, value] of Object.entries(instance.exports)) if (name.startsWith('ring3_abi_v1_') && typeof value === 'function') c.api[name.slice(13)] = value;
  c.key = ++counts.contexts; host(c, 'open', [12, c.key, keyHigh]); c.base = c.api.arena_ptr() >>> 0;
  for (const page of [PC, 0x5000, 0x8000, 0xf000]) host(c, 'map', [page, 1, 7]);
  c.initial = arena(c); return c;
}
function compile(c, instruction, name, terminal = false) {
  if (c.previous) {input(c, 96, words([0])); host(c, c.stale ? 'retire_stale_resident' : 'discard_unacknowledged_resident', [c.key, keyHigh, ...c.previous]); c.stale = false;}
  const code = Buffer.from(terminal ? instruction : [...instruction, 0xeb, 0]);
  upload(c, PC, code); input(c, 140, words(c.entries ? [PC] : [PC, code.length]));
  const compileName = c.owner === 'resident' ? c.entries ? 'compile_resident_entries' : 'compile_resident' : c.entries ? 'compile_entries' : 'compile';
  assert.equal(c.api[compileName](1, ...(c.entries ? [0] : [])), 0, `${c.owner}/${c.entries}/${name}: compile`);
  const b = arena(c), pointer = c.owner === 'resident' ? b.readUInt32LE(156) : c.api.module_ptr() >>> 0;
  if (c.owner === 'resident') c.previous = [b.readUInt32LE(148), b.readUInt32LE(152)];
  const length = c.owner === 'resident' ? b.readUInt32LE(160) : c.api.module_len() >>> 0;
  const bytes = Buffer.from(new Uint8Array(c.memory.buffer, pointer, length)), module = new WebAssembly.Module(bytes), imports = WebAssembly.Module.imports(module), ring3 = {...c.api};
  for (const i of imports.filter(i => i.module === 'ring3' && !i.name.startsWith('guard') && i.name !== 'finish_exception_detection')) ring3[i.name] = (...args) => {c.calls.push(i.name); return c.api[i.name](...args);};
  writeFileSync(join(output, `module-${++counts.modules}.wasm`), bytes, {flag: 'wx'});
  return {run: new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3}).exports.run,
    memory: imports.some(i => i.module === 'ring3' && !i.name.startsWith('guard') && i.name !== 'finish_exception_detection'),
    cpu: imports.some(i => /^(cpu_|segment_load|stack_execute|simd_execute|x87_op)/.test(i.name)), instructionLength: instruction.length};
}
function setup(c, row) {
  const segments = Buffer.from(c.initial.subarray(SEG, SEG + 64));
  if (row.cache) {
    segments.writeUInt16LE(2, 4); defaults.forEach((v, i) => segments.writeUInt32LE(v >>> 0, offsets[i]));
    for (const [i, v] of Object.entries(row.cache)) segments.writeUInt32LE(v >>> 0, offsets[+i]);
  }
  if (row.fsBase !== undefined) segments.writeUInt32LE(row.fsBase >>> 0, 48);
  if (row.nullDs) {segments.writeUInt16LE(0, 22); if (row.cache) segments.writeUInt32LE(0, offsets[3]);}
  input(c, SEG, segments); input(c, FP, c.initial.subarray(FP, SV + 160));
  if (row.pending) {
    const fp = arena(c).subarray(FP, FP + 128); fp.writeUInt16LE(0x37e, 16); fp.writeUInt16LE(1, 18); input(c, FP, fp);
  }
  input(c, SV + 32, Buffer.from(Array.from({length: 128}, (_, i) => (i * 13 + 37) & 255)));
  input(c, T, Buffer.alloc(520, 0xa7));
  const state = {r: [0x11223344, 3, 5, DATA, 0x8004, DATA, DATA, DATA], pc: PC, flags: 0x244cd7};
  for (const [i, v] of Object.entries(row.registers ?? {})) state.r[+i] = v >>> 0;
  input(c, 0, record('R3ST', [...state.r, state.pc, state.flags], 2, 56)); input(c, 96, words([row.cancel ?? 0]));
  return state;
}
function savedImage(before) {
  const image = Buffer.alloc(512, 0xa7); image.fill(0, 0, 464);
  image.writeUInt16LE(before.readUInt16LE(FP + 16), 0);
  image.writeUInt16LE(before.readUInt16LE(FP + 18), 2);
  image.writeUInt32LE(0x1f80, 24); image.writeUInt32LE(0xffff, 28);
  before.copy(image, 160, SV + 32, SV + 160);
  return image;
}
function restoredImage(badMxcsr = false) {
  const image = Buffer.alloc(512); image.writeUInt16LE(0x37f);
  image.writeUInt32LE(badMxcsr ? 0x10000 : 0x5f80, 24); image.writeUInt32LE(0xffff, 28);
  for (let i = 0; i < 128; i++) image[160 + i] = (i * 29 + 0x73) & 255;
  image.fill(0xe1, 464); return image;
}
function readRam(c, address, length) {
  const out = Buffer.alloc(length); for (let i = 0; i < length; i++) out[i] = byte(c, address + i); return out;
}
const observations = [], frames = [], ramFrames = [], exits = [];
function run(c, fn, row, state, ram) {
  const before = arena(c), expected = Buffer.from(before), expectedRam = Buffer.from(ram), at = c.calls.length;
  const fault = row.fault || row.alignment || row.badMxcsr, stopped = fault || row.budget === 0 || row.cancel;
  const reason = row.cancel ? 2 : fault ? row.ss && row.fault ? 15 : 11 : 1, retired = stopped ? 0 : 1;
  expected.set(record('R3ST', [...state.r, PC + (retired ? fn.instructionLength : 0), state.flags], 2, 56));
  expected.set(record('R3EX', [reason, retired, 0, 0, 0, 0], 6), 56);
  let calls = [];
  if (!row.fault && row.budget !== 0 && !row.cancel) {
    const residentSuffix = c.owner === 'resident' ? '_resident' : '';
    if (row.save) {
      const image = savedImage(before); expected.set(image, T); expected.set(words([0, state.flags]), T + 512);
      expected.set(record('R3MH', [row.alignment ? 3 : 0, 0, 0, 0, 0, 512], 7), 100);
      calls = ['simd_execute' + residentSuffix, 'cpu_fxsave' + residentSuffix];
      if (!row.alignment) image.copy(expectedRam, row.address === undefined ? 0 : row.address - DATA, 0, 464);
    } else {
      calls = ['cpu_read'];
      expected.set(record('R3MH', [row.alignment ? 3 : 0, 0, 0, 0, 0, 512], 7), 100);
      if (!row.alignment) {
        const image = restoredImage(row.badMxcsr); expected.set(image, T); calls.push('simd_execute' + residentSuffix);
        expected.set(record('R3MH', [row.badMxcsr ? 3 : 0, 0, 0, 0, 0, 0], 7), 100);
        if (!row.badMxcsr) {
          expected.set(words([0, state.flags]), T + 512);
          expected.set(c.initial.subarray(FP, SV + 160), FP);
          expected.writeUInt32LE(0x5f80, SV + 16); image.copy(expected, SV + 32, 160, 288);
        }
      }
    }
  }
  assert.equal(fn.run(c.base, c.base + 56, row.budget ?? 1, c.base + 96), 0, row.name);
  const after = arena(c), actualCalls = c.calls.slice(at);
  if (row.writeOnly) host(c, 'protect', [0x5000, 1, 7]);
  const afterRam = readRam(c, DATA, ram.length);
  let error = null;
  try {assert.deepEqual(after, expected, 'complete arena'); assert.deepEqual(actualCalls, calls, 'helper trace'); assert.deepEqual(afterRam, expectedRam, 'complete operand RAM and software tail');}
  catch (e) {error = e.message.split('\n')[0];}
  const arenaFrame = frames.length, ramFrame = ramFrames.length;
  frames.push(before, after); ramFrames.push(ram, afterRam); exits.push(Buffer.from(after.subarray(56, 96)));
  observations.push({owner: c.owner, entries: c.entries, name: row.name, passed: error === null, error, arena_frame: arenaFrame, ram_frame: ramFrame,
    expected: {reason, retired, calls}, actual: {reason: after.readUInt32LE(72), retired: after.readUInt32LE(76), calls: actualCalls}});
  counts.runs++; counts[row.budget === 0 || row.cancel ? 'controls' : fault ? 'faults' : 'successes']++;
}
const rows = [];
for (const save of [true, false]) {
  const name = save ? 'FXSAVE' : 'FXRSTOR', code = [0x0f, 0xae, save ? 0x07 : 0x0f];
  for (const row of [
    {name: 'full512-last-byte', cache: {3: 0xf230520e}, fault: true},
    {name: 'software-tail-still-in-m512-span', cache: {3: 0xf23051df}, fault: true},
    {name: 'default-SS-full512', code: [0x0f, 0xae, save ? 0x45 : 0x4d, 0], cache: {2: 0xf230520e}, fault: true, ss: true},
    {name: 'explicit-SS-full512', code: [0x36, ...code], cache: {2: 0xf230520e}, fault: true, ss: true},
    {name: 'FS-logical-before-wrap', code: [0x64, ...code], registers: {7: 0x6010}, fsBase: 0xfffff000, cache: {4: 0xf230620e}, fault: true},
    {name: 'expand-down-lower-exclusive', cache: {3: 0xf2705010}, fault: true},
    {name: 'pending-MF-still-checks-segment', cache: {3: 0xf230520e}, pending: true, fault: true},
    {name: 'null-selector', nullDs: true, fault: true},
    {name: 'exact-end-success', cache: {3: save ? 0xf230520f : 0xf210520f}},
    {name: 'expand-down-exact-lowest-success', cache: {3: 0xf270500f}},
    {name: 'FS-wrapping-base-success', code: [0x64, ...code], registers: {7: 0x6010}, fsBase: 0xfffff000, cache: {4: 0xf230620f}},
    {name: 'address16-DI-truncates-before-cache', code: [0x67, 0x0f, 0xae, save ? 0x05 : 0x0d], registers: {7: 0xdead5010}, cache: {3: 0xf230520f}},
    {name: 'pending-MF-ignored-success', pending: true},
    {name: 'linear-misalignment-unchanged', registers: {7: DATA + 1}, address: DATA + 1, alignment: true},
    {name: 'budget-before-cache', cache: {3: 0xf2300000}, budget: 0},
    {name: 'cancel-before-cache', cache: {3: 0xf2300000}, cancel: 1},
  ]) rows.push({...row, name: name + '-' + row.name, code: row.code ?? code, save});
}
rows.push(
  {name: 'FXSAVE-readonly-cache', code: [0x0f, 0xae, 0x07], save: true, cache: {3: 0xf31fffff}, fault: true},
  {name: 'FXSAVE-CS-writes-denied', code: [0x2e, 0x0f, 0xae, 0x07], save: true, fault: true},
  {name: 'FXRSTOR-execute-only-CS', code: [0x2e, 0x0f, 0xae, 0x0f], save: false, cache: {1: 0xf39fffff}, fault: true},
  {name: 'FXSAVE-writeonly-page-software-tail', code: [0x0f, 0xae, 0x07], save: true, writeOnly: true},
  {name: 'FXRSTOR-reserved-MXCSR-stages-only-transfer', code: [0x0f, 0xae, 0x0f], save: false, badMxcsr: true},
);
for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries);
  for (const row of rows) {
    host(c, 'protect', [0x5000, 1, 7]);
    const ram = Buffer.alloc(528, 0x66); if (!row.save) restoredImage(row.badMxcsr).copy(ram, row.address === undefined ? 0 : row.address - DATA);
    upload(c, DATA, ram); const fn = compile(c, row.code, row.name), state = setup(c, row);
    if (row.writeOnly) host(c, 'protect', [0x5000, 1, 2]);
    run(c, fn, row, state, ram);
  }
  host(c, 'close');
}
assert.equal(counts.runs, rows.length * 4);
assert.deepEqual(counts, {contexts: 4, modules: 148, runs: 148, faults: 88, successes: 44, controls: 16});
assert.deepEqual(pins(), sourcePins);
writeFileSync(join(output, 'arenas.bin'), Buffer.concat(frames), {flag: 'wx'});
writeFileSync(join(output, 'ram.bin'), Buffer.concat(ramFrames), {flag: 'wx'});
writeFileSync(join(output, 'exit-records.bin'), Buffer.concat(exits), {flag: 'wx'});
const failed = observations.filter(r => !r.passed), status = failed.length ? 'counterfactual-failure' : 'ok';
writeFileSync(join(output, 'result.json'), JSON.stringify({status, engine_sha256: engine.sha256, source_pins: sourcePins, counts, observations, records: {arena: {file: 'arenas.bin', frame_bytes: SIZE, frames: frames.length}, ram: {file: 'ram.bin', frame_bytes: 528, frames: ramFrames.length}, exits: {file: 'exit-records.bin', record_bytes: 40, records: exits.length}}, limits: [
  'fixed protected32 environment; full m512 cached segment checks, unchanged linear16 alignment and nonwaiting FX semantics',
  'combined segment-vs-alignment faults are not used to infer priority from exception lists',
]}, null, 2) + '\n');
console.log(JSON.stringify({status, ...counts, failed: failed.length, output}));
if (failed.length) process.exitCode = 1;
