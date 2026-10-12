import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../../../../../../../support/wasm/engine.mjs';
import {assertEngineExports} from '../../../../../../../support/wasm/exports.mjs';

const [enginePath, output, root] = process.argv.slice(2), engine = readEngine(enginePath);
const {sourcePaths: exportSources} = assertEngineExports(engine.module, root);
const PC = 0x1000, SEG = 4524, SIZE = 4612, keyHigh = 0x4c4152;
const counts = {contexts: 0, modules: 0, runs: 0, registers: 0, memory: 0, faults: 0, controls: 0};
const paths = [...exportSources, 'engine/tests/support/wasm/engine.mjs', 'engine/src/cpu/x86/segments/queries.rs', 'engine/src/cpu/x86/segments/descriptor_record.rs', 'engine/src/cpu/x86/segments/cache.rs', 'engine/src/abi/x86/segments.rs', 'engine/src/cpu/dbt/wasm/segments/access.rs',
  'engine/src/cpu/x86/segments/descriptors.rs', 'engine/src/cpu/x86/decode/segment_queries.rs', 'engine/src/cpu/x86/decode/decoder.rs',
  'engine/src/cpu/x86/ir.rs', 'engine/src/cpu/dbt/region.rs', 'engine/src/cpu/dbt/wasm/memory.rs', 'engine/src/cpu/dbt/wasm/memory/narrow.rs',
  'engine/src/cpu/dbt/wasm/segments.rs', 'engine/src/cpu/dbt/wasm/segment_queries.rs', 'engine/src/cpu/dbt/wasm/integer/dispatch.rs',
  'engine/src/process/cpu/integer.rs', 'engine/src/process/cpu/segment_queries.rs',
  'engine/tests/cpu/segments/selectors/attributes/native.rs', 'engine/tests/cpu/segments/selectors/attributes/wasm.rs', 'engine/tests/cpu/segments/selectors/attributes/fixtures/p2-descriptor-queries/runtime/run.mjs',
];
const pins = () => Object.fromEntries(paths.map(path => [path, createHash('sha256').update(readFileSync(join(root, path))).digest('hex')]));
assert.equal(new Set(paths).size, paths.length, 'unique source pins');
const sourcePins = pins(), words = values => {const b = Buffer.alloc(values.length * 4); values.forEach((v, i) => b.writeUInt32LE(v >>> 0, i * 4)); return b;};
function record(magic, size, fields, version = 1) {
  const b = Buffer.alloc(size); b.write(magic); b.writeUInt16LE(version, 4); b.writeUInt16LE(1, 6); b.writeUInt32LE(size, 8);
  fields.forEach((v, i) => b.writeUInt32LE(v >>> 0, 16 + i * 4)); return b;
}
const cpu = s => record('R3ST', 56, [...s.r, s.pc, s.flags], s.flags & ~0xcd7 ? 2 : 1);
const arena = c => Buffer.from(new Uint8Array(c.memory.buffer, c.base, SIZE));
function input(c, at, b) {new Uint8Array(c.memory.buffer).set(b, c.base + at);}
function host(c, name, args = []) {assert.equal(c.api[name](...args), 0, name);}
function upload(c, address, b) {input(c, 140, b); host(c, 'upload', [address, b.length]);}
function put(c, address, selector) {const b = Buffer.alloc(2); b.writeUInt16LE(selector); upload(c, address, b);}
function seed(c, updates = {}, flags = 0x244cd7) {
  const s = {r: Array.from({length: 8}, (_, i) => (0xa5101234 + i * 0x10001) >>> 0), pc: PC, flags};
  for (const [index, value] of Object.entries(updates)) s.r[+index] = value >>> 0;
  input(c, 0, cpu(s)); input(c, 96, words([0])); return s;
}
function open(owner, entries) {
  const instance = new WebAssembly.Instance(engine.module, {}), c = {owner, entries, memory: instance.exports.memory, api: {}, calls: []};
  for (const [name, value] of Object.entries(instance.exports)) if (name.startsWith('ring3_abi_v1_') && typeof value === 'function') c.api[name.slice(13)] = value;
  c.key = ++counts.contexts; host(c, 'open', [8, c.key, keyHigh]); c.base = c.api.arena_ptr() >>> 0;
  for (const args of [[PC, 1, 7], [0x5000, 1, 3], [0xf000, 1, 3]]) host(c, 'map', args);
  c.initialSegments = arena(c).subarray(SEG, SEG + 64); return c;
}
function compile(c, instruction, kind, destination, word, memory = false) {
  if (c.previousResident) {input(c, 96, words([0])); host(c, 'discard_unacknowledged_resident', [c.key, keyHigh, ...c.previousResident]);}
  upload(c, PC, Buffer.from([...instruction, 0xeb, 0])); input(c, 140, words(c.entries ? [PC] : [PC, instruction.length + 2]));
  const method = c.owner === 'resident' ? (c.entries ? 'compile_resident_entries' : 'compile_resident') : (c.entries ? 'compile_entries' : 'compile');
  host(c, method, [1, ...(c.entries ? [0] : [])]);
  const b = arena(c), pointer = c.owner === 'resident' ? b.readUInt32LE(156) : c.api.module_ptr() >>> 0;
  if (c.owner === 'resident') c.previousResident = [b.readUInt32LE(148), b.readUInt32LE(152)];
  c.binding = c.owner === 'resident' ? [c.key, keyHigh, ...c.previousResident] : [];
  const length = c.owner === 'resident' ? b.readUInt32LE(160) : c.api.module_len() >>> 0;
  const bytes = Buffer.from(new Uint8Array(c.memory.buffer, pointer, length)), module = new WebAssembly.Module(bytes);
  const name = c.owner === 'resident' ? 'cpu_integer_resident' : 'cpu_integer';
  assert.deepEqual(WebAssembly.Module.imports(module).map(i => `${i.module}:${i.name}:${i.kind}`).sort(),
    ['env:memory:memory', `ring3:${c.owner === 'resident' ? 'guard_resident' : 'guard'}:function`, `ring3:${name}:function`, ...(memory ? ['ring3:read16:function'] : [])].sort());
  writeFileSync(join(output, `module-${++counts.modules}.wasm`), bytes, {flag: 'wx'});
  const ring3 = {...c.api};
  for (const helper of [name, ...(memory ? ['read16'] : [])]) ring3[helper] = (...args) => {
    const before = arena(c), status = c.override && helper !== 'read16' ? c.override(c) : c.api[helper](...args), after = arena(c), allowed = Buffer.from(before);
    allowed.set(after.subarray(100, helper === 'read16' ? 140 : 164), 100);
    assert.deepEqual(after, allowed, 'query helper publication is restricted to packet and payload');
    c.calls.push({name: helper, status, args: args.map(v => v >>> 0), input: before.subarray(140, 164)}); return status;
  };
  return {run: new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3}).exports.run, instructionLength: instruction.length, kind, destination, word, memory};
}
function execute(c, fn, s, {value, selector, budget = 1, reason = 1, retired = 1, calls, packet, fault = [0, 0, 0, 0]} = {}) {
  const before = arena(c), expected = Buffer.from(before), at = c.calls.length;
  const completed = value !== undefined;
  const n = {...s, r: [...s.r], pc: retired ? PC + fn.instructionLength : PC,
    flags: completed ? ((s.flags & ~0x40) | (value === null ? 0 : 0x40)) >>> 0 : s.flags};
  if (completed && value !== null) n.r[fn.destination] = fn.word ? ((s.r[fn.destination] & 0xffff0000) | (value & 0xffff)) >>> 0 : value >>> 0;
  expected.set(cpu(n)); expected.set(record('R3EX', 40, [reason, retired, ...fault], reason === 16 ? 7 : 6), 56);
  if (completed) {
    expected.set(record('R3MH', 40, [0, 0, 0, 0, 0, 0], 7), 100);
    expected.set(words([value === null ? selector : value, n.flags, s.r[0], s.r[2], s.r[3], s.r[1]]), 140);
  } else if (packet) expected.set(record('R3MH', 40, packet, 2), 100);
  assert.equal(fn.run(c.base, c.base + 56, budget, c.base + 96), 0);
  assert.deepEqual(arena(c), expected, `whole arena kind=${fn.kind} word=${fn.word} selector=${selector} reason=${reason}`);
  const observed = c.calls.slice(at); assert.equal(observed.length, calls ?? (completed ? fn.memory ? 2 : 1 : 0));
  for (const call of observed) {
    assert.equal(call.status, 0);
    if (call.name !== 'read16') {
      assert.deepEqual(call.args.slice(-2), [fn.kind === 'lar' ? 0x30003 : 0x30004, 0]);
      assert.deepEqual(call.input, words([selector, s.flags, s.r[0], s.r[2], s.r[3], s.r[1]]));
    }
  }
  counts.runs++;
}
// Independent Intel protected32 tables; presence is a returned bit, never an eligibility condition.
const larSystem = new Set([1, 2, 3, 4, 5, 9, 11, 12]), lslSystem = new Set([1, 2, 3, 9, 11]);
function oracle(kind, codeData, type, dpl, rpl, low, high) {
  if (!codeData && !(kind === 'lar' ? larSystem : lslSystem).has(type)) return null;
  if (!(codeData && type >= 12) && dpl < Math.max(3, rpl)) return null;
  if (kind === 'lar') return ((high >>> 8 & 255) * 256 + (high >>> 20 & 15) * 0x100000) >>> 0;
  const limit = (low & 0xffff) + (high >>> 16 & 15) * 65536;
  return high & 0x800000 ? (limit * 4096 + 4095) >>> 0 : limit;
}
function configure(c, selector, low, high) {
  const before = arena(c); host(c, 'configure_segment_descriptor_raw', [selector, low, high]);
  assert.deepEqual(arena(c), before, 'table configuration leaves every loaded arena byte unchanged');
}
function cached(c, segment, bits) {
  const b = Buffer.from(c.initialSegments); b.writeUInt16LE(2, 4);
  [0xf33fffff, 0xf3bfffff, 0xf33fffff, 0xf33fffff, 0xf33fffff, 0].forEach((v, i) => b.writeUInt32LE(v >>> 0, [28, 32, 36, 40, 44, 56][i]));
  b.writeUInt32LE(bits >>> 0, [28, 32, 36, 40, 44, 56][segment]); input(c, SEG, b);
}
for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries);
  for (const kind of ['lar', 'lsl']) for (const word of [false, true]) {
    const opcode = kind === 'lar' ? 2 : 3, prefix = word ? [0x66] : [];
    let fn = compile(c, [...prefix, 0x0f, opcode, 0xc8], kind, 1, word);
    for (const codeData of [false, true]) for (let type = 0; type < 16; type++) for (let dpl = 0; dpl < 4; dpl++) for (const present of [0, 1]) for (const ti of [0, 4]) {
      for (let rpl = 0; rpl < 4; rpl++) {
        const selector = 0x40 | ti | rpl, high = (0xab510078 | (type << 8) | (Number(codeData) << 12) | (dpl << 13) | (present << 15) | (rpl % 2 ? 0x800000 : 0)) >>> 0, low = 0xcafe2345;
        configure(c, selector, low, high);
        const s = seed(c, {0: 0x9abc0000 | selector}, rpl % 2 ? 2 : 0x244cd7);
        execute(c, fn, s, {value: oracle(kind, codeData, type, dpl, rpl, low, high), selector}); counts.registers++;
      }
    }
    for (const selector of [0, 1, 2, 3, 0x53, 0xffff]) {
      const s = seed(c, {0: 0x9abc0000 | selector}); execute(c, fn, s, {value: null, selector}); counts.registers++;
    }
    for (const selector of [4, 5, 6, 7]) {
      configure(c, selector, 0x2345, 0x0081f200);
      const s = seed(c, {0: 0x9abc0000 | selector});
      execute(c, fn, s, {value: kind === 'lar' ? 0x0080f200 : 0x12345fff, selector}); counts.registers++;
    }
    // A present or nonpresent byte-granular zero limit still succeeds and sets ZF.
    if (kind === 'lsl') for (const destination of [0, 1]) {
      const zero = compile(c, [...prefix, 0x0f, opcode, 0xc0 | destination << 3], kind, destination, word);
      for (const present of [0, 1]) {
        configure(c, 0x43, 0, 0x00407200 | present << 15);
        const s = seed(c, {0: 0x9abc0043}, 0x244c97);
        execute(c, zero, s, {value: 0, selector: 0x43}); counts.registers++;
      }
    }
    for (let destination = 0; destination < 8; destination++) for (let source = 0; source < 8; source++) {
      const alias = compile(c, [...prefix, 0x0f, opcode, 0xc0 | destination << 3 | source], kind, destination, word);
      for (const [selector, low, high, value] of [[0x43, 0xffff, 0x00fff200, kind === 'lar' ? 0x00f0f200 : 0xffffffff], [0x53, 0, 0, null]]) {
        if (selector === 0x43) configure(c, selector, low, high);
        const s = seed(c, {[source]: 0xa5b60000 | selector}); execute(c, alias, s, {value, selector}); counts.registers++;
      }
    }
    // The current table, rather than a loaded cache, owns every subsequent descriptor query.
    fn = compile(c, [...prefix, 0x0f, opcode, 0xc8], kind, 1, word);
    configure(c, 0x43, 0x1234, 0x0040f200);
    host(c, c.owner === 'resident' ? 'segment_load_resident' : 'segment_load', [...c.binding, 4, 0x43]);
    const loaded = arena(c).subarray(SEG, SEG + 64);
    configure(c, 0x43, 0x5678, 0x00807000);
    let s = seed(c, {0: 0x43}); execute(c, fn, s, {value: kind === 'lar' ? 0x00807000 : 0x05678fff, selector: 0x43});
    assert.deepEqual(arena(c).subarray(SEG, SEG + 64), loaded); counts.controls++;
    input(c, SEG, c.initialSegments);
    for (const row of [
      {bytes: [...prefix, 0x0f, opcode, 7], updates: {7: 0x5010}, address: 0x5010},
      {bytes: [...prefix, 0x0f, opcode, 0], updates: {0: 0x5ffe}, address: 0x5ffe},
      {bytes: [...prefix, 0x0f, opcode, 0x45, 0], updates: {5: 0x5010}, address: 0x5010},
      {bytes: [0x64, ...prefix, 0x0f, opcode, 7], updates: {7: 0x10}, address: 0x5010, fs: true},
      {bytes: [0x67, ...prefix, 0x0f, opcode, 0], updates: {3: 0xabcd5000, 6: 0x12340010}, address: 0x5010},
      {bytes: [0x65, 0x67, ...prefix, 0x0f, opcode, 6, 0x10, 0x50], updates: {}, address: 0x5010, gs: true},
    ]) {
      input(c, SEG, c.initialSegments);
      if (row.fs) host(c, 'set_segment_base', [4, 0x5000]);
      if (row.gs) host(c, c.owner === 'resident' ? 'segment_load_resident' : 'segment_load', [...c.binding, 5, 0x23]);
      configure(c, 0x43, 0x2345, 0x0081f200);
      const memory = compile(c, row.bytes, kind, 0, word, true); put(c, row.address, 0x43);
      s = seed(c, row.updates); execute(c, memory, s, {value: kind === 'lar' ? 0x0080f200 : 0x12345fff, selector: 0x43}); counts.memory++;
      s = seed(c, row.updates, 0x244dd7); execute(c, memory, s, {value: kind === 'lar' ? 0x0080f200 : 0x12345fff, selector: 0x43, reason: 16}); counts.controls++;
      s = seed(c, row.updates); execute(c, memory, s, {budget: 0, retired: 0}); input(c, 96, words([1])); execute(c, memory, s, {reason: 2, retired: 0}); counts.controls += 2;
    }
    input(c, SEG, c.initialSegments); configure(c, 0x43, 0x2345, 0x0081f200);
    let memory = compile(c, [...prefix, 0x0f, opcode, 7], kind, 0, word, true);
    upload(c, 0x5fff, Buffer.from([0x43])); s = seed(c, {7: 0x5fff});
    execute(c, memory, s, {reason: 5, retired: 0, calls: 1, packet: [1, 0, 1, 0x6000, 1, 2], fault: [1, 0x6000, 1, 2]}); counts.faults++;
    host(c, 'map', [0x6000, 1, 3]); upload(c, 0x6000, Buffer.from([0]));
    s = seed(c, {7: 0x5fff}); execute(c, memory, s, {value: kind === 'lar' ? 0x0080f200 : 0x12345fff, selector: 0x43}); counts.memory++;
    host(c, 'unmap', [0x6000, 1]); host(c, 'protect', [0x5000, 1, 0]); s = seed(c, {7: 0x5010});
    execute(c, memory, s, {reason: 5, retired: 0, calls: 1, packet: [1, 0, 2, 0x5010, 1, 2], fault: [2, 0x5010, 1, 2]}); counts.faults++;
    host(c, 'protect', [0x5000, 1, 3]);
    for (const row of [
      {bytes: [...prefix, 0x0f, opcode, 7], segment: 3, updates: {7: 0x7000}, reason: 11},
      {bytes: [...prefix, 0x0f, opcode, 0x45, 0], segment: 2, updates: {5: 0x7000}, reason: 15},
    ]) {
      cached(c, row.segment, 0xf2305000); memory = compile(c, row.bytes, kind, 0, word, true); s = seed(c, row.updates);
      execute(c, memory, s, {reason: row.reason, retired: 0}); counts.faults++;
    }
    input(c, SEG, c.initialSegments);
    for (const [modrm, updates, reason] of [[7, {7: 0xffffffff}, 11], [0x45, {5: 0xffffffff}, 15]]) {
      memory = compile(c, [...prefix, 0x0f, opcode, modrm, ...(modrm === 0x45 ? [0] : [])], kind, 0, word, true); s = seed(c, updates);
      execute(c, memory, s, {reason, retired: 0}); counts.faults++;
    }
    memory = compile(c, [...prefix, 0x0f, opcode, 7], kind, 0, word, true);
    host(c, c.owner === 'resident' ? 'segment_load_resident' : 'segment_load', [...c.binding, 3, 0]);
    s = seed(c, {7: 0x7000}); execute(c, memory, s, {reason: 11, retired: 0}); counts.faults++;
    input(c, SEG, c.initialSegments);
  }
  for (const kind of ['lar', 'lsl']) for (const word of [false, true]) {
    const fn = compile(c, [...(word ? [0x66] : []), 0x0f, kind === 'lar' ? 2 : 3, 0xc8], kind, 1, word);
    for (const defect of ['flags', 'length', 'status']) {
      const s = seed(c, {0: 0x23}), expected = arena(c);
      const payload = [0x23, s.flags, s.r[0], s.r[2], s.r[3], s.r[1]];
      const helper = record('R3MH', 40, [0, 0, 0, 0, 0, defect === 'length' ? 1 : 0], 7);
      if (defect === 'flags') {payload[0] = 0xdeadbeef; payload[1] ^= 1;}
      if (defect !== 'status') expected.set(helper, 100);
      expected.set(words(payload), 140);
      expected.set(record('R3EX', 40, [7, 0, defect === 'status' ? 3 : 2, 0, 0, 0], 6), 56);
      c.override = context => {
        if (defect !== 'status') {input(context, 100, helper); input(context, 140, words(payload));}
        return defect === 'status' ? 1 : 0;
      };
      assert.equal(fn.run(c.base, c.base + 56, 1, c.base + 96), 0);
      assert.deepEqual(arena(c), expected, `${kind} word=${word} malformed helper ${defect} cannot publish destination or flags`);
      c.override = undefined; counts.controls++; counts.runs++;
    }
  }
  for (const [descriptor, selector, address] of [[0x30003, 0x23, 1], [0x30004, 0x10023, 0], [0x30005, 0x23, 0]]) {
    input(c, 140, words([selector, 0x244cd7, 1, 2, 3, 4])); const before = arena(c);
    assert.notEqual(c.api[c.owner === 'resident' ? 'cpu_integer_resident' : 'cpu_integer'](...c.binding, descriptor, address), 0);
    assert.deepEqual(arena(c), before); counts.controls++;
  }
  host(c, 'close');
}
assert.equal(counts.contexts, 4);
assert.deepEqual(pins(), sourcePins, 'query sources stayed fixed');
writeFileSync(join(output, 'result.json'), JSON.stringify({status: 'ok', engine_sha256: engine.sha256, source_pins: sourcePins, counts,
  limits: ['configured lossless protected32 software descriptor table, finite LAR/LSL proof; undefined LAR bits19:16 deterministically zero; deterministic full logical span overflow GP0/SS0 policy (Intel permits implementation latitude); AM0 environment; no guest-GDT memory or kernel execution claim']}, null, 2) + '\n');
console.log(JSON.stringify({status: 'ok', ...counts, output}));
