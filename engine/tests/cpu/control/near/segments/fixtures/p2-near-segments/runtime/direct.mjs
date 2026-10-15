import assert from 'node:assert/strict';
import {readFileSync, writeFileSync, mkdirSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {join} from 'node:path';
import {pathToFileURL} from 'node:url';

const [enginePath, initialArenaPath, root, output] = process.argv.slice(2);
assert.ok(enginePath && initialArenaPath && root && output, 'engine, initial arena, root and output are required');
assert.match(process.env.RING3_ENGINE_SHA256 ?? '', /^[a-f0-9]{64}$/, 'select an explicit engine SHA-256');
const {readEngine} = await import(pathToFileURL(join(root, 'engine/tests/support/wasm/engine.mjs')));
const engine = readEngine(enginePath), initial = readFileSync(initialArenaPath);
assert.equal(initial.length, 4796, 'complete existing arena, including companion');
const PC = 0x1000, STACK = 0x7800, POINTER = 0x5010, SEG = 4524, KEY_HIGH = 0x4e454152;
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const sourcePaths = [
  "engine/src/abi/arena.rs",
  "engine/src/abi/x86/state.rs",
  "engine/src/abi/x86/segments.rs",
  "engine/src/abi/x86/exit.rs",
  "engine/src/abi/memory_helper.rs",
  "engine/src/cpu/x86/decode/flow.rs",
  "engine/src/cpu/dbt/wasm/instructions/integer/dispatch.rs",
  "engine/src/cpu/dbt/wasm/imports/mod.rs", "engine/src/cpu/dbt/wasm/imports/binding.rs", "engine/src/cpu/dbt/wasm/imports/context.rs", "engine/src/cpu/dbt/wasm/imports/needs.rs", "engine/src/cpu/dbt/wasm/helpers/protocol.rs", "engine/src/cpu/dbt/wasm/instructions/memory/mod.rs", "engine/src/cpu/dbt/wasm/instructions/memory/address.rs", "engine/src/cpu/dbt/wasm/instructions/memory/scalar.rs", "engine/src/cpu/dbt/wasm/instructions/memory/stack.rs",
  "engine/src/cpu/dbt/wasm/instructions/memory/narrow.rs",
  "engine/src/cpu/dbt/wasm/instructions/memory/word_store.rs",
  "engine/src/cpu/dbt/wasm/code_fetch.rs",
  "engine/src/cpu/dbt/wasm/emitter/mod.rs", "engine/src/cpu/dbt/wasm/emitter/module.rs", "engine/src/cpu/dbt/wasm/emitter/body.rs", "engine/src/cpu/dbt/wasm/requirements.rs", "engine/src/cpu/dbt/wasm/imports/wiring.rs",
  "engine/src/cpu/dbt/wasm/abi.rs",
  "engine/src/cpu/dbt/wasm/instructions/segments/access.rs",
  "engine/src/process/memory/scalar.rs",
  "engine/src/process/memory/span.rs",
  "engine/tests/support/wasm.rs",
  "engine/tests/support/wasm/engine.mjs",
  "engine/tests/cpu/control/near/segments/wasm.rs",
  "engine/tests/cpu/control/wasm.rs",
  "engine/tests/cpu/control/near/segments/fixtures/p2-near-segments/runtime/run.mjs",
  "engine/tests/cpu/control/near/segments/fixtures/p2-near-segments/runtime/direct.mjs",
  "engine/tests/cpu/control/near/segments/fixtures/p2-near-segments/runtime/indirect.mjs"
];
const pins = () => Object.fromEntries(sourcePaths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = pins();
const words = values => {const bytes = Buffer.alloc(values.length * 4); values.forEach((value, i) => bytes.writeUInt32LE(value >>> 0, i * 4)); return bytes;};
function packet(magic, size, fields = [], version = 1) {
  const bytes = Buffer.alloc(size); bytes.write(magic); bytes.writeUInt16LE(version, 4);
  bytes.writeUInt16LE(1, 6); bytes.writeUInt32LE(size, 8); bytes.set(words(fields), 16); return bytes;
}
function relative(opcode, target, word = false) {
  const bytes = Buffer.alloc(word ? 4 : 5), next = PC + bytes.length;
  if (word) {bytes[0] = 0x66; bytes[1] = opcode; bytes.writeUInt16LE((target - next) & 0xffff, 2);}
  else {bytes[0] = opcode; bytes.writeInt32LE(target - next, 1);} return bytes;
}
const rows = [
  {name: 'jmp32-direct-target-gp', code: relative(0xe9, 0x2000)},
  {name: 'jmp16-direct-target-gp', code: relative(0xe9, 0x2000, true)},
  {name: 'call32-target-gp-stack-neutral', code: relative(0xe8, 0x2000)},
  {name: 'call16-target-gp-stack-neutral', code: relative(0xe8, 0x2000, true)},
  {name: 'call32-target-gp-before-stack-ss', code: relative(0xe8, 0x2000), badStack: true},
  {name: 'call16-target-gp-before-stack-ss', code: relative(0xe8, 0x2000, true), badStack: true},
  {name: 'jcc-taken-target-gp', code: Buffer.from('747e', 'hex'), flags: 0x42, limit: 0x107f},
  {name: 'loop-taken-target-gp-counter-neutral', code: Buffer.from('e27e', 'hex'), ecx: 2, limit: 0x107f},
  {name: 'loope-taken-target-gp-counter-neutral', code: Buffer.from('e17e', 'hex'), ecx: 2, flags: 0x42, limit: 0x107f},
  {name: 'loopne-taken-target-gp-counter-neutral', code: Buffer.from('e07e', 'hex'), ecx: 2, limit: 0x107f},
  {name: 'loop-cx-taken-target-gp-full-counter-neutral', code: Buffer.from('67e27e', 'hex'), ecx: 0xabcd0002, limit: 0x1080},
  {name: 'jecxz-taken-target-gp', code: Buffer.from('e37e', 'hex'), ecx: 0, limit: 0x107f},
  {name: 'jcxz-taken-target-gp', code: Buffer.from('67e37e', 'hex'), ecx: 0xabcd0000, limit: 0x1080},
  {name: 'ret32-target-gp-stack-neutral', code: Buffer.from('c3', 'hex'), read: 4},
  {name: 'ret16-target-gp-stack-neutral', code: Buffer.from('66c3', 'hex'), read: 2},
  {name: 'ret32-adjust-target-gp-stack-neutral', code: Buffer.from('c20800', 'hex'), read: 4},
  {name: 'ret16-adjust-target-gp-stack-neutral', code: Buffer.from('66c20800', 'hex'), read: 2},
  {name: 'jcc-untaken-outside-fallthrough-control', code: Buffer.from('747e', 'hex'), control: true, limit: PC + 1},
  {name: 'loop-untaken-outside-fallthrough-control', code: Buffer.from('e27e', 'hex'), control: true, ecx: 1, afterEcx: 0, limit: PC + 1},
  {name: 'loop-cx-untaken-high-half-control', code: Buffer.from('67e27e', 'hex'), control: true, ecx: 0xabcd0001, afterEcx: 0xabcd0000, limit: PC + 2},
  {name: 'jecxz-untaken-outside-fallthrough-control', code: Buffer.from('e37e', 'hex'), control: true, ecx: 1, limit: PC + 1},
  {name: 'jmp-inrange-control', code: relative(0xe9, PC), control: true, target: PC},
  {name: 'call32-inrange-control', code: relative(0xe8, PC), control: true, target: PC, write: 4},
  {name: 'call16-inrange-control', code: relative(0xe8, PC, true), control: true, target: PC, write: 2},
  {name: 'ret32-inrange-adjust-control', code: Buffer.from('c20800', 'hex'), control: true, target: PC, read: 4, adjust: 8},
  {name: 'ret16-inrange-adjust-control', code: Buffer.from('66c20800', 'hex'), control: true, target: PC, read: 2, adjust: 8},
];
const observations = [];
let identity = 0;
mkdirSync(output, {recursive: true});
for (const resident of [false, true]) for (const entries of [false, true]) for (const row of rows) {
  const key = ++identity, profile = `${resident ? 'resident' : 'replacement'}-${entries ? 'entries' : 'extents'}`;
  const instance = new WebAssembly.Instance(engine.module, {}), memory = instance.exports.memory;
  const api = Object.fromEntries(Object.entries(instance.exports).filter(([name, value]) => name.startsWith('ring3_abi_v1_') && typeof value === 'function').map(([name, value]) => [name.slice(13), value]));
  const host = (name, args = []) => assert.equal(api[name](...args), 0, name);
  host('open', [4, key, KEY_HIGH]); const base = api.arena_ptr() >>> 0;
  const put = (offset, bytes) => new Uint8Array(memory.buffer).set(bytes, base + offset);
  const arena = () => Buffer.from(new Uint8Array(memory.buffer, base, initial.length));
  const upload = (address, bytes) => {put(140, bytes); host('upload', [address, bytes.length]);};
  put(0, initial);
  for (const [address, permission] of [[PC, 7], [0x5000, 3], [0x7000, 3]]) host('map', [address, 1, permission]);
  const code = row.code; upload(PC, code);
  put(140, words(entries ? [PC] : [PC, code.length]));
  const compile = resident ? (entries ? 'compile_resident_entries' : 'compile_resident') : (entries ? 'compile_entries' : 'compile');
  assert.equal(api[compile](...(entries ? [1, 0] : [1])), 0, `${profile}/${row.name}: terminal compile`);
  const compiled = arena(), id = resident ? [compiled.readUInt32LE(148), compiled.readUInt32LE(152)] : null;
  const address = resident ? compiled.readUInt32LE(156) : api.module_ptr() >>> 0;
  const length = resident ? compiled.readUInt32LE(160) : api.module_len() >>> 0;
  const generated = Buffer.from(new Uint8Array(memory.buffer, address, length));
  const module = new WebAssembly.Module(generated), calls = [];
  const ring3 = Object.fromEntries(WebAssembly.Module.imports(module).filter(item => item.kind === 'function').map(item => [item.name, (...args) => {
    const status = api[item.name](...args); calls.push({name: item.name, args: args.map(value => value >>> 0), status}); return status;
  }]));
  const run = new WebAssembly.Instance(module, {env: {memory}, ring3}).exports.run;
  put(0, initial);
  put(0, packet('R3ST', 56, [0x2000, row.ecx ?? 2, 0x3456789a, POINTER, STACK, 0x56789abc, 0x6789abcd, 0x789abcde, PC, row.flags ?? 2]));
  const segments = packet('R3SG', 64, [], 2);
  [0x23, 0x1b, 0x23, 0x23, 0x3b, 0].forEach((value, i) => segments.writeUInt16LE(value, 16 + i * 2));
  [0xf33fffff, (0xf2b00000 | (row.limit ?? 0x10ff)) >>> 0, row.badStack ? 0xf23077fb : 0xf33fffff, 0xf33fffff, 0xf33fffff, 0]
    .forEach((value, i) => segments.writeUInt32LE(value >>> 0, [28, 32, 36, 40, 44, 56][i]));
  put(SEG, segments);
  upload(0x7000, Buffer.alloc(4096, 0x6b)); upload(STACK, words([row.target ?? 0x2000])); upload(POINTER, words([0x2000]));
  function readRAM(address) {
    const saved = arena(), bytes = Buffer.alloc(4096);
    for (let offset = 0; offset < 4096; offset += 512) {
      host('cpu_read', [address + offset, 512, 1]); const inspected = arena();
      assert.equal(inspected.readUInt32LE(116), 0, 'diagnostic read'); bytes.set(inspected.subarray(140, 652), offset);
    }
    put(0, saved); return bytes;
  }
  const ramBefore = new Map([PC, 0x5000, 0x7000].map(address => [address, readRAM(address)]));
  const before = arena(), want = Buffer.from(before), success = !!row.control;
  const helpers = [];
  if (row.read) {
    want.set(packet('R3MH', 40, [0, row.target ?? 0x2000, 0, 0, 0, row.read], 10), 100);
    helpers.push(row.read === 2 ? 'cpu_read16' : 'cpu_read32');
  }
  if (success) {
    want.writeUInt32LE(row.target ?? PC + row.code.length, 48);
    if (row.afterEcx !== undefined) want.writeUInt32LE(row.afterEcx, 20);
    if (row.read) want.writeUInt32LE(STACK + row.read + (row.adjust ?? 0), 32);
    if (row.write) {
      want.writeUInt32LE(STACK - row.write, 32);
      want.set(packet('R3MH', 40, [0, 0, 0, 0, 0, row.write], 10), 100);
      helpers.push(resident ? `cpu_store_resident${row.write * 8}` : `cpu_store${row.write * 8}`);
      const returned = words([PC + row.code.length]); ramBefore.get(0x7000).set(returned.subarray(0, row.write), STACK - row.write - 0x7000);
    }
  }
  want.set(packet('R3EX', 40, [success ? 1 : 11, success ? 1 : 0, 0, 0, 0, 0], success ? (row.read || row.write ? 6 : 1) : 6), 56);
  const status = run(base, base + 56, 1, base + 96), after = arena(), errors = [];
  const check = (condition, label) => {if (!condition) errors.push(label);};
  check(status === 0, 'run status');
  check(after.equals(want), 'exact complete arena');
  check(after.readUInt32LE(72) === (success ? 1 : 11), 'exit reason');
  check(after.readUInt32LE(76) === (success ? 1 : 0), 'retired count');
  check(after.readUInt32LE(48) === want.readUInt32LE(48), 'original/committed EIP');
  check(after.subarray(16, 48).equals(want.subarray(16, 48)), 'all GPRs including ESP/ECX');
  check(after.readUInt32LE(52) === want.readUInt32LE(52), 'flags');
  const actualHelpers = calls.filter(call => !call.name.startsWith('guard')).map(call => call.name);
  check(JSON.stringify(actualHelpers) === JSON.stringify(helpers), 'exact real helper schedule');
  for (const [address, bytes] of ramBefore) check(readRAM(address).equals(bytes), `exact RAM page ${address.toString(16)}`);
  const actual = {reason: after.readUInt32LE(72), retired: after.readUInt32LE(76), detail: after.readUInt32LE(80), pc: after.readUInt32LE(48), ecx: after.readUInt32LE(20), esp: after.readUInt32LE(32)};
  observations.push({name: row.name, profile, kind: success ? 'control' : 'semantic', passed: errors.length === 0, expected: {reason: success ? 1 : 11, retired: success ? 1 : 0}, actual, errors, calls, module_sha256: hash(generated), ...(id ? {resident_id: id} : {})});
  if (errors.length) {writeFileSync(join(output, `${key}-before.bin`), before); writeFileSync(join(output, `${key}-after.bin`), after); writeFileSync(join(output, `${key}-want.bin`), want);}
  host('close');
}
assert.deepEqual(pins(), sourcePins, 'source unchanged during run');
const counts = Object.fromEntries(['semantic', 'control'].map(kind => [kind, {passed: observations.filter(row => row.kind === kind && row.passed).length, failed: observations.filter(row => row.kind === kind && !row.passed).length}]));
const result = {status: observations.every(row => row.passed) ? 'PASS' : 'FAIL', engine_sha256: engine.sha256, probe_sha256: hash(readFileSync(new URL(import.meta.url))), initial_arena_sha256: hash(initial), source_sha256: sourcePins, counts, observations};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2));
console.log(JSON.stringify(result)); process.exitCode = result.status === 'PASS' ? 0 : 1;
