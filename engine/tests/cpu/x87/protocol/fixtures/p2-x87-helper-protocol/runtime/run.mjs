import assert from 'node:assert/strict';
import {mkdirSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {createHash} from 'node:crypto';
import {createArena, ARENA_SIZE} from '../../../../../../support/wasm/arena.mjs';
import {readEngine} from '../../../../../../support/wasm/engine.mjs';

const [enginePath, output] = process.argv.slice(2), engine = readEngine(enginePath);
mkdirSync(output, {recursive: true});
const observations = [], modules = [], contexts = [];
const hash = b => createHash('sha256').update(b).digest('hex');
const blocks = [[0x1000, [0xd9, 0xe8, 0xeb, 0xfe]],
  [0x1100, [0xdb, 0xf1, 0xeb, 0xfe]], [0x1200, [0x9b, 0xeb, 0xfe]]];
const inputFlags = 0xcd7;
function packet(value) {
  const b = Buffer.alloc(40); b.write('R3MH');
  b.writeUInt32LE(0x10007, 4); b.writeUInt32LE(40, 8); b.writeUInt32LE(value >>> 0, 20);
  return b;
}
function exit(reason, retired, detail = 0) {
  const b = Buffer.alloc(40); b.write('R3EX');
  b.writeUInt32LE(0x10006, 4); b.writeUInt32LE(40, 8);
  b.writeUInt32LE(reason, 16); b.writeUInt32LE(retired, 20); b.writeUInt32LE(detail, 24);
  return b;
}
function open(resident, entries) {
  const instance = new WebAssembly.Instance(engine.module, {}), api = {};
  for (const [name, fn] of Object.entries(instance.exports)) {
    if (name.startsWith('ring3_abi_v1_')) api[name.slice(13)] = fn;
  }
  const id = contexts.length + 1;
  assert.equal(api.open(1, id, 0x583850), 0);
  const memory = instance.exports.memory, base = api.arena_ptr() >>> 0;
  const arena = () => Buffer.from(new Uint8Array(memory.buffer, base, ARENA_SIZE));
  const put = (offset, bytes) => new Uint8Array(memory.buffer).set(bytes, base + offset);
  assert.equal(api.map(0x1000, 1, 7), 0);
  for (const [pc, bytes] of blocks) { put(140, bytes); assert.equal(api.upload(pc, bytes.length), 0); }
  const descriptors = Buffer.alloc(blocks.length * (entries ? 4 : 8));
  blocks.forEach(([pc, bytes], i) => {
    const offset = i * (entries ? 4 : 8);
    descriptors.writeUInt32LE(pc, offset);
    if (!entries) descriptors.writeUInt32LE(bytes.length, offset + 4);
  });
  put(140, descriptors);
  const name = resident ? (entries ? 'compile_resident_entries' : 'compile_resident') :
    (entries ? 'compile_entries' : 'compile');
  assert.equal(api[name](blocks.length, ...(entries ? [0] : [])), 0);
  const receipt = arena(), binding = resident ? [id, 0x583850, receipt.readUInt32LE(148), receipt.readUInt32LE(152)] : [];
  const pointer = resident ? receipt.readUInt32LE(156) : api.module_ptr() >>> 0;
  const length = resident ? receipt.readUInt32LE(160) : api.module_len() >>> 0;
  const bytes = Buffer.from(new Uint8Array(memory.buffer, pointer, length));
  const module = new WebAssembly.Module(bytes), ring3 = {...api};
  const c = {id, resident, entries, api, memory, base, arena, put, synthetic: null, calls: 0};
  const helper = resident ? 'x87_resident_op' : 'x87_op';
  for (const imp of WebAssembly.Module.imports(module)) {
    if (imp.module === 'ring3' && imp.name.startsWith('guard')) {
      // raw replies isolate generated validation; canonical runs retain the real owner guard.
      ring3[imp.name] = (...args) => c.synthetic === null ? api[imp.name](...args) : 0;
    }
  }
  ring3[helper] = (...args) => {
    c.calls++;
    if (resident) assert.deepEqual(args.slice(0, 4).map(v => v >>> 0), binding);
    if (c.synthetic === null) return api[helper](...args);
    put(100, packet(c.synthetic));
    return 0;
  };
  c.run = new WebAssembly.Instance(module, {env: {memory}, ring3}).exports.run;
  const file = `module-${id}.wasm`; writeFileSync(join(output, file), bytes);
  modules.push({context: id, file, bytes: bytes.length, sha256: hash(bytes), imports: WebAssembly.Module.imports(module)});
  contexts.push({id, resident, entries});
  return c;
}
function seed(c, pc, fp) {
  const a = createArena();
  for (let i = 0; i < 8; i++) a.writeUInt32LE((0xa1001234 + i * 0x10001) >>> 0, 16 + i * 4);
  a.writeUInt32LE(pc, 48); a.writeUInt32LE(inputFlags, 52);
  if (fp) a.set(fp, 4236);
  c.put(0, a);
}
function raw(c, label, pc, returned, valid) {
  seed(c, pc); c.synthetic = returned;
  const before = c.arena(), calls = c.calls;
  assert.equal(c.run(c.base, c.base + 56, 1, c.base + 96), 0, label);
  const expected = Buffer.from(before); expected.set(packet(returned), 100);
  expected.set(exit(valid ? 1 : 7, valid ? 1 : 0, valid ? 0 : 2), 56);
  if (valid) { expected.writeUInt32LE(pc + 2, 48); expected.writeUInt32LE(returned >>> 0, 52); }
  assert.deepEqual(c.arena(), expected, label + ' full arena and no early publication');
  assert.equal(c.calls - calls, 1);
  observations.push({context: c.id, label, raw: true, returned: returned >>> 0, valid,
    reason: valid ? 1 : 7, retired: valid ? 1 : 0, arena_sha256: hash(c.arena())});
}
function fpPair(signaling = false) {
  const fp = createArena().subarray(4236, 4364);
  fp.writeUInt16LE(signaling ? 0x37e : 0x37f, 16);
  fp.writeUInt16LE(signaling ? 0xfff2 : 0xfff0, 20);
  fp.writeBigUInt64LE(signaling ? 0x8000000000000001n : 0x8000000000000000n, 40);
  fp.writeUInt16LE(signaling ? 0x7fff : 0x3fff, 48);
  fp.writeBigUInt64LE(0x8000000000000000n, 50); fp.writeUInt16LE(0x3fff, 58);
  return fp;
}
function canonicalCompare(c, signaling) {
  c.synthetic = null; seed(c, 0x1100, fpPair(signaling));
  const before = c.arena(), expected = Buffer.from(before), calls = c.calls;
  const flags = signaling ? inputFlags : 0x442;
  expected.writeUInt32LE(0x1102, 48); expected.writeUInt32LE(flags, 52);
  expected.set(exit(1, 1), 56); expected.set(packet(flags), 100);
  expected.writeUInt16LE(signaling ? 0x8081 : 0, 4254);
  expected.writeUInt16LE(0x3f1, 4258); expected.writeUInt32LE(0x1100, 4260);
  expected.writeUInt16LE(0x1b, 4268);
  assert.equal(c.run(c.base, c.base + 56, 1, c.base + 96), 0);
  assert.deepEqual(c.arena(), expected, 'canonical comparison/deferred flags full arena');
  assert.equal(c.calls - calls, 1);
  observations.push({context: c.id, label: signaling ? 'canonical unmasked comparison preserves flags and defers' : 'canonical equal comparison clears OF SF AF', raw: false,
    reason: 1, retired: 1, arena_sha256: hash(c.arena())});
  if (signaling) {
    const pending = c.arena(); pending.writeUInt32LE(0x1200, 48); c.put(0, pending);
    const stopped = Buffer.from(pending), p = packet(0);
    p.writeUInt32LE(4, 16); p.writeUInt32LE(0x8081, 24);
    stopped.set(p, 100); stopped.set(exit(12, 0, 0x8081), 56);
    assert.equal(c.run(c.base, c.base + 56, 1, c.base + 96), 0);
    assert.deepEqual(c.arena(), stopped, 'next waiting instruction faults before publication');
    observations.push({context: c.id, label: 'canonical waiting delivery after unmasked comparison', raw: false,
      reason: 12, retired: 0, arena_sha256: hash(c.arena())});
  }
}
for (const resident of [false, true]) for (const entries of [false, true]) {
  const c = open(resident, entries);
  raw(c, 'preserving operation valid reply', 0x1000, inputFlags, true);
  for (const [label, flags] of [['fixed bit', inputFlags ^ 2], ['CF mutation', inputFlags ^ 1],
    ['DF mutation', inputFlags ^ 0x400], ['reserved high bit', (inputFlags | 0x80000000) >>> 0]]) {
    raw(c, 'preserving operation rejects ' + label, 0x1000, flags, false);
  }
  for (const relation of [0, 1, 0x40, 0x45]) raw(c, 'comparison relation ' + relation, 0x1100, 0x402 | relation, true);
  for (const [label, flags] of [['SF relation', 0x482], ['AF relation', 0x412], ['DF mutation', 0x42],
    ['fixed bit', 0x440], ['reserved high bit', 0x80000442]]) {
    raw(c, 'comparison rejects ' + label, 0x1100, flags, false);
  }
  canonicalCompare(c, false); canonicalCompare(c, true);
  assert.equal(c.api.close(), 0);
}
writeFileSync(join(output, 'result.json'), JSON.stringify({status: 'PASS',
  engine: {path: enginePath, sha256: engine.sha256, bytes: engine.bytes.length},
  contexts, modules, observations,
  claim: 'Generated x87 V7 success validation preserves integer FLAGS except canonical compare relations, rejects malformed raw payloads before CPU/FLAGS publication, and admits real equal/deferred comparison outcomes. Synthetic replies deliberately do not model x87 arithmetic or host-state publication.'}, null, 2) + '\n');
console.log(JSON.stringify({status: 'PASS', contexts: contexts.length, modules: modules.length, runs: observations.length, output}));
