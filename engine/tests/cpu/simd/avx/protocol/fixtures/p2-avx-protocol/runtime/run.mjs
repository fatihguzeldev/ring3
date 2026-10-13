import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../../../../../../../support/wasm/engine.mjs';
import {assertEngineExports} from '../../../../../../../support/wasm/exports.mjs';

const [enginePath, output, root] = process.argv.slice(2), engine = readEngine(enginePath);
const {sourcePaths: exportSources} = assertEngineExports(engine.module, root);
const sourcePaths = [...exportSources, 'engine/tests/support/wasm/engine.mjs',
  'engine/src/cpu/x86/decode/avx/mod.rs', 'engine/src/cpu/x86/avx/operation.rs',
  'engine/src/cpu/dbt/wasm/avx/execute.rs', 'engine/src/cpu/dbt/wasm/cpu_result.rs',
  'engine/src/process/cpu/avx.rs', 'engine/src/cpu/x86/avx/dispatch.rs',
  'engine/tests/cpu/simd/avx/protocol/wasm.rs',
  'engine/tests/cpu/simd/avx/protocol/fixtures/p2-avx-protocol/runtime/run.mjs'];
assert.equal(new Set(sourcePaths).size, sourcePaths.length);
const hash = b => createHash('sha256').update(b).digest('hex');
const pins = () => Object.fromEntries(sourcePaths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = pins(), PC = 0x1000, T = 140, SV = 4364, YU = 4612, SIZE = 4756, keyHigh = 0x50524f;
const counts = {contexts: 0, modules: 0, runs: 0, rejected: 0, accepted: 0, controls: 0};
const words = values => {const b = Buffer.alloc(values.length * 4); values.forEach((value, i) => b.writeUInt32LE(value >>> 0, i * 4)); return b;};
function record(magic, size, values, version = 1) {
  const b = Buffer.alloc(size); b.write(magic); b.writeUInt16LE(version, 4); b.writeUInt16LE(1, 6); b.writeUInt32LE(size, 8);
  values.forEach((value, i) => b.writeUInt32LE(value >>> 0, 16 + i * 4)); return b;
}
const arena = c => Buffer.from(new Uint8Array(c.memory.buffer, c.base, SIZE));
function input(c, offset, value) {new Uint8Array(c.memory.buffer).set(value, c.base + offset);}
function host(c, name, args = []) {assert.equal(c.api[name](...args), 0, name);}
function open(owner, entries) {
  const instance = new WebAssembly.Instance(engine.module, {}), c = {owner, entries, memory: instance.exports.memory, api: {}};
  for (const [name, value] of Object.entries(instance.exports)) if (name.startsWith('ring3_abi_v1_') && typeof value === 'function') c.api[name.slice(13)] = value;
  for (const [name, arity] of Object.entries({open: 3, map: 3, upload: 2, compile: 1, compile_entries: 2,
    compile_resident: 1, compile_resident_entries: 2, avx_execute: 6, avx_execute_resident: 10})) assert.equal(c.api[name]?.length, arity, name);
  c.key = ++counts.contexts; host(c, 'open', [8, c.key, keyHigh]); c.base = c.api.arena_ptr() >>> 0;
  assert.equal(c.api.arena_len(), SIZE);
  for (const args of [[PC, 1, 7], [0x5000, 1, 3]]) host(c, 'map', args);
  c.initial = arena(c); return c;
}
function compile(c, row) {
  if (c.resident) {input(c, 96, words([0])); host(c, 'discard_unacknowledged_resident', [c.key, keyHigh, ...c.resident]);}
  const code = Buffer.from(row.code, 'hex'); input(c, T, Buffer.concat([code, Buffer.from('ebfe', 'hex')])); host(c, 'upload', [PC, code.length + 2]);
  input(c, T, words(c.entries ? [PC] : [PC, code.length + 2]));
  host(c, c.owner === 'resident' ? c.entries ? 'compile_resident_entries' : 'compile_resident' : c.entries ? 'compile_entries' : 'compile', [1, ...(c.entries ? [0] : [])]);
  const a = arena(c);
  if (c.owner === 'resident') c.resident = [a.readUInt32LE(T + 8), a.readUInt32LE(T + 12)];
  const pointer = c.owner === 'resident' ? a.readUInt32LE(T + 16) : c.api.module_ptr() >>> 0;
  const length = c.owner === 'resident' ? a.readUInt32LE(T + 20) : c.api.module_len() >>> 0;
  const binary = Buffer.from(new Uint8Array(c.memory.buffer, pointer, length)), module = new WebAssembly.Module(binary);
  const helper = c.owner === 'resident' ? 'avx_execute_resident' : 'avx_execute';
  assert.deepEqual(WebAssembly.Module.imports(module).map(i => `${i.module}:${i.name}:${i.kind}`).sort(),
    ['env:memory:memory', `ring3:${c.owner === 'resident' ? 'guard_resident' : 'guard'}:function`, `ring3:${helper}:function`].sort());
  assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  writeFileSync(join(output, `module-${++counts.modules}.wasm`), binary, {flag: 'wx'});
  const ring3 = {...c.api};
  ring3[helper] = (...args) => {
    c.calls++;
    const payload = args.slice(-6).map(value => value >>> 0);
    assert.equal(payload[0], row.masked ? 0x01000d03 : 0x0100010e);
    assert.deepEqual(payload.slice(2), [0, 0, 0, 0x202]);
    if (c.owner === 'resident') assert.deepEqual(args.slice(0, 4).map(value => value >>> 0), [c.key, keyHigh, ...c.resident]);
    input(c, 100, packet(row)); return 0;
  };
  return new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3}).exports.run;
}
function packet(row) {
  const result = record('R3MH', 40, [row.status, 0, row.detail ?? 0, row.fault ?? 0, row.access ?? 0, row.length ?? (row.register ? 0 : 32)], 7);
  if (row.reserved) result.writeUInt32LE(1, 12);
  return result;
}
function execute(c, run, row, budget = 1, cancelled = false) {
  input(c, 0, c.initial);
  input(c, 0, record('R3ST', 56, [0x5020, 1, 2, 3, 0x5800, 5, 6, 7, PC, 0x202], 2));
  for (let i = 0; i < 8; i++) {input(c, SV + 32 + i * 16, Buffer.alloc(16, 0x11 + i)); input(c, YU + 16 + i * 16, Buffer.alloc(16, 0x91 + i));}
  if (row.masked) for (const offset of [SV + 32, YU + 16]) input(c, offset, Buffer.alloc(16, row.zero ? 0 : 0xff));
  if (row.mxcsr !== undefined) input(c, SV + 16, words([row.mxcsr]));
  if (cancelled) input(c, 96, words([1]));
  const before = arena(c), expected = Buffer.from(before), invokes = budget !== 0 && !cancelled;
  const reason = cancelled ? 2 : budget === 0 ? 1 : row.reason;
  expected.set(record('R3EX', 40, [reason, 0, invokes ? row.exitDetail ?? (reason === 7 ? 2 : row.detail ?? 0) : 0,
    invokes && reason === 5 ? row.fault : 0, invokes && reason === 5 ? row.access : 0, invokes && reason === 5 ? 32 : 0], 6), 56);
  if (invokes) {
    expected.set(packet(row), 100);
    if (!row.register) expected.set(record('R3AM', 24, [row.segment, row.zero ? 0 : 0x5020]), T + 544);
  }
  c.calls = 0; assert.equal(run(c.base, c.base + 56, budget, c.base + 96), 0, row.name);
  assert.deepEqual(arena(c), expected, `${row.name}: exact full arena, budget=${budget}, cancelled=${cancelled}`);
  assert.equal(c.calls, Number(invokes), `${row.name}: helper calls`);
  counts.runs++; if (!invokes) counts.controls++; else if (reason === 7) counts.rejected++; else counts.accepted++;
}
const rows = [];
for (const [name, prefix, segment] of [['DS', '', 3], ['FS', '64', 4], ['SS', '36', 2]]) {
  for (const masked of [false, true]) for (const status of [3, 7]) rows.push({name: `${name}-${masked ? 'selected' : 'full'}-${status}`,
    code: prefix + (masked ? 'c4e27d2c00' : 'c5fc5800'), segment, masked, status,
    reason: status === (segment === 2 ? 7 : 3) ? status + 8 : 7});
  for (const status of [3, 7]) rows.push({name: `${name}-zero-selection-${status}`, code: prefix + 'c4e27d2c00', segment, masked: true, zero: true, status, reason: 7});
}
for (const status of [3, 7]) rows.push({name: `register-impossible-${status}`, code: 'c5fc58c1', register: true, status, reason: 7});
for (const extra of [{status: 4}, {status: 6}, {status: 3, detail: 1}, {status: 7, detail: 1}, {status: 3, reserved: true}, {status: 3, length: 16},
  {status: 1, detail: 1, fault: 0x5020, access: 2}, {status: 1, detail: 1, fault: 0x5040, access: 1}])
  rows.push({name: `malformed-${rows.length}`, code: 'c5fc5800', segment: 3, reason: 7, ...extra});
rows.push({name: 'valid-page-fault-envelope', code: 'c5fc5800', segment: 3, status: 1, detail: 1, fault: 0x5020, access: 1, reason: 5});
rows.push({name: 'already-sticky-unmasked-IE-can-trap-again', code: 'c5fc58c1', register: true, mxcsr: 0x1f01, status: 5, detail: 0x1f01, reason: 13});
rows.push({name: 'XM-cleared-old-sticky-is-malformed', code: 'c5fc58c1', register: true, mxcsr: 0x1f01, status: 5, detail: 0x1f00, reason: 7});
rows.push({name: 'XM-control-change-is-malformed', code: 'c5fc58c1', register: true, mxcsr: 0x1f01, status: 5, detail: 0x1f81, reason: 7});
for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries);
  for (const row of rows) {const run = compile(c, row); execute(c, run, row); execute(c, run, row, 0); execute(c, run, row, 1, true);}
  host(c, 'close');
}
assert.deepEqual(pins(), sourcePins, 'source pins unchanged');
writeFileSync(join(output, 'result.json'), JSON.stringify({scope: 'real generated AVX modules with authored strict R3MH protocol injections; guest arithmetic is not emulated', engineSha256: engine.sha256, counts, sourcePins}, null, 2) + '\n');
console.log(JSON.stringify({status: 'PASS', ...counts}));
