import assert from 'node:assert/strict';
import {mkdirSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {createArena, ARENA_SIZE} from '../../../../../support/wasm/arena.mjs';
import {readEngine} from '../../../../../support/wasm/engine.mjs';

const [enginePath, output] = process.argv.slice(2);
const engine = readEngine(enginePath);
mkdirSync(output, {recursive: true});
const observations = [];

function open(program, abiOnly = false) {
  const instance = new WebAssembly.Instance(engine.module, {});
  const api = {};
  for (const [name, fn] of Object.entries(instance.exports)) {
    if (name.startsWith('ring3_abi_v1_')) api[name.slice(13)] = fn;
  }
  assert.equal(api.open(3, 0x424f554e, 0x44415259), 0);
  const base = api.arena_ptr() >>> 0;
  const memory = instance.exports.memory;
  const arena = () => Buffer.from(new Uint8Array(memory.buffer, base, ARENA_SIZE));
  const put = (offset, bytes) => new Uint8Array(memory.buffer).set(bytes, base + offset);
  const upload = (address, bytes) => {
    put(140, bytes);
    assert.equal(api.upload(address, bytes.length), 0);
  };
  assert.equal(api.map(0x1000, 1, 7), 0);
  assert.equal(api.map(0x5000, 1, 3), 0);
  upload(0x1000, Buffer.from([...program, 0xeb, 0xfe]));
  const descriptor = Buffer.alloc(8);
  descriptor.writeUInt32LE(0x1000);
  descriptor.writeUInt32LE(program.length + 2, 4);
  put(140, descriptor);
  assert.equal(api.compile(1), 0);
  const module = new WebAssembly.Module(new Uint8Array(memory.buffer,
    api.module_ptr() >>> 0, api.module_len() >>> 0));
  let helpers = 0;
  const ring3 = {};
  for (const imp of WebAssembly.Module.imports(module)) {
    if (imp.kind === 'function') ring3[imp.name] = (...args) => {
      // a permissive guard isolates generated ABI validation from host pointer checks.
      if (abiOnly && imp.name.startsWith('guard')) return 0;
      if (!imp.name.startsWith('guard')) helpers++;
      return api[imp.name](...args);
    };
  }
  const run = new WebAssembly.Instance(module, {env: {memory}, ring3}).exports.run;
  const reset = () => {
    const initial = createArena();
    initial.writeUInt32LE(0x1000, 48);
    put(0, initial);
  };
  reset();
  return {api, base, memory, arena, put, upload, run, reset,
    helperCount: () => helpers};
}

function refused(c, label, mutate, status = 2, exit = 56, cancel = 96) {
  c.reset();
  const malformed = c.arena();
  mutate(malformed);
  c.put(0, malformed);
  const before = Buffer.from(new Uint8Array(c.memory.buffer));
  const helpers = c.helperCount();
  assert.equal(c.run(c.base, c.base + exit, 1, c.base + cancel), status, label);
  assert.deepEqual(Buffer.from(new Uint8Array(c.memory.buffer)), before, label + ' full memory neutral');
  assert.equal(c.helperCount(), helpers, label + ' no helper side effects');
  observations.push({label, status});
}

const simd = open([0x66, 0x0f, 0xfc, 0xc1], true);
for (const [offset, value, label] of [
  [4364, 0, 'SIMD magic'], [4368, 0x10002, 'SIMD version'],
  [4372, 159, 'SIMD length'], [4376, 1, 'SIMD reserved'],
  [4380, 0x10000, 'MXCSR reserved bit'], [4384, 1, 'SIMD reserved tail'],
]) refused(simd, label, a => a.writeUInt32LE(value, offset));
refused(simd, 'SIMD exit alias', () => {}, 1, 4364);
refused(simd, 'SIMD cancel alias', () => {}, 1, 56, 4364);
assert.equal(simd.api.close(), 0);

const segment = open([0x64, 0x8b, 0x03], true);
for (const [offset, value, label] of [
  [4524, 0, 'segment magic'], [4528, 0x10002, 'segment version'],
  [4532, 63, 'segment length'], [4536, 1, 'segment reserved'],
  [4556, 1, 'flat ES base'], [4580, 1, 'segment reserved tail'],
]) refused(segment, label, a => a.writeUInt32LE(value, offset));
refused(segment, 'CS null selector', a => a.writeUInt16LE(0, 4542));
refused(segment, 'SS wrong RPL', a => a.writeUInt16LE(0x20, 4544));
refused(segment, 'segment exit alias', () => {}, 1, 4524);
refused(segment, 'segment cancel alias', () => {}, 1, 56, 4524);
assert.equal(segment.api.close(), 0);

const rep = open([0xf3, 0xa4], true);
for (const [offset, value, label] of [
  [4588, 0, 'work magic'], [4592, 0x10002, 'work version'],
  [4596, 23, 'work length'], [4600, 1, 'work reserved'],
  [4608, 1, 'work reserved tail'],
]) refused(rep, label, a => a.writeUInt32LE(value, offset));
refused(rep, 'work exit alias', () => {}, 1, 4588);
refused(rep, 'work cancel alias', () => {}, 1, 56, 4588);
for (const [label, exit, cancel] of [
  ['exit begins at work end', 4612, 96], ['cancel begins at work end', 56, 4612],
]) {
  rep.reset();
  // reset the declared cancellation input even when it lies beyond the arena.
  new Uint8Array(rep.memory.buffer, rep.base + cancel, 4).fill(0);
  assert.equal(rep.run(rep.base, rep.base + exit, 1, rep.base + cancel), 0, label);
  assert.equal(rep.arena().readUInt32LE(48), 0x1002, label);
  assert.equal(rep.arena().subarray(4588, 4592).toString('ascii'), 'R3WK', label);
  observations.push({label, status: 0});
}
assert.equal(rep.api.close(), 0);

// literal packed-decimal and extended80 encodings independently retain signed zero.
const bcd = open([0xdf, 0x25, 0x10, 0x50, 0, 0, 0xdf, 0x35, 0x20, 0x50, 0, 0]);
for (const sign of [0, 0x80]) {
  bcd.reset();
  const packed = Buffer.alloc(10);
  packed[9] = sign;
  bcd.upload(0x5010, packed);
  assert.equal(bcd.run(bcd.base, bcd.base + 56, 1, bcd.base + 96), 0);
  const loaded = bcd.arena();
  assert.equal(loaded.readUInt32LE(48), 0x1006);
  assert.equal(loaded.readUInt16LE(4254), 0x3800);
  assert.equal(loaded.readUInt16LE(4256), 0x7fff);
  const zero = Buffer.alloc(10);
  zero[9] = sign;
  assert.deepEqual(loaded.subarray(4346, 4356), zero);
  assert.equal(bcd.run(bcd.base, bcd.base + 56, 1, bcd.base + 96), 0);
  const stored = bcd.arena();
  assert.equal(stored.readUInt32LE(48), 0x100c);
  assert.equal(stored.readUInt16LE(4254), 0);
  assert.equal(stored.readUInt16LE(4256), 0xffff);
  const actual = Buffer.alloc(10);
  for (let i = 0; i < 10; i++) {
    assert.equal(bcd.api.read8(0x5020 + i), 0);
    actual[i] = bcd.arena().readUInt32LE(120);
  }
  assert.deepEqual(actual, packed);
  observations.push({label: `packed BCD ${sign ? 'negative' : 'positive'} zero load/store`, status: 0});
}
assert.equal(bcd.api.close(), 0);
writeFileSync(join(output, 'result.json'), JSON.stringify({status: 'PASS',
  engine: {sha256: engine.sha256, bytes: engine.bytes.length}, observations,
  claim: 'Actual generated Wasm rejects malformed or aliased extended records before all memory side effects under an explicitly permissive guard; bound packed BCD signed-zero load/store.'}, null, 2) + '\n');
console.log(JSON.stringify({status: 'PASS', observations: observations.length, output}));
