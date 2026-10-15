import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../../../support/wasm/engine.mjs';

export const u32 = value => Number(BigInt.asUintN(32, BigInt(value)));
export const words = values => {const b = Buffer.alloc(values.length * 4); values.forEach((v, i) => b.writeUInt32LE(v >>> 0, i * 4)); return b;};
const SIZE = 4612, SEG = 4524, PC = 0x1000, HIGH = 0x42554c4b, hash = b => createHash('sha256').update(b).digest('hex');
const windows = [[0x1000, 256], [0x7f00, 512], [0x8f00, 512], [0xff00, 512], [0xffffff00, 256], [0, 128], [0x5f00, 512]];
const pages = [0, PC, 0x5000, 0x6000, 0x7000, 0x8000, 0x9000, 0xf000, 0x10000, 0xfffff000];
const defaultCaches = [0xf33fffff, 0xf3bfffff, 0xf33fffff, 0xf33fffff, 0xf33fffff, 0];
const cacheOffsets = [28, 32, 36, 40, 44, 56];
function record(magic, fields, version = 1, size = 40) {
  const b = Buffer.alloc(size); b.write(magic); b.writeUInt16LE(version, 4); b.writeUInt16LE(1, 6); b.writeUInt32LE(size, 8); b.set(words(fields), 16); return b;
}
const packet = fields => record('R3MH', fields, 7);
export function harness(family, sourcePaths) {
  const [enginePath, output, root] = process.argv.slice(2), engine = readEngine(enginePath);
  const paths = ['engine/src/process/cpu/stack.rs', 'engine/src/process/cpu/segments.rs', 'engine/src/cpu/x86/stack.rs', 'engine/src/memory/space.rs', 'engine/src/memory/space/atomic.rs', 'engine/src/cpu/x86/segments/cache.rs', 'engine/src/cpu/dbt/wasm/stack.rs', 'engine/src/abi/cpu_helper.rs', 'engine/src/abi/x86/segments.rs', 'engine/tests/support/wasm/engine.mjs', 'engine/tests/cpu/stack/support/segments.mjs', ...sourcePaths];
  assert.equal(new Set(paths).size, paths.length);
  const pins = () => Object.fromEntries(paths.map(p => [p, hash(readFileSync(join(root, p)))])), sourcePins = pins();
  const observations = [], modules = [], frames = [], ramFrames = [], exits = [], helperCalls = [];
  let schedule;
  const counts = {contexts: 0, runs: 0, failures: 0, successes: 0, controls: 0, synthetic: 0};
  const arena = c => Buffer.from(new Uint8Array(c.memory.buffer, c.base, SIZE));
  const input = (c, at, b) => new Uint8Array(c.memory.buffer).set(b, c.base + at);
  function host(c, name, args = []) {assert.equal(c.api[name](...args), 0, name);}
  function upload(c, address, b) {input(c, 140, b); host(c, 'upload', [address, b.length]);}
  function peek(c) {
    const model = new Map();
    for (const [address, length] of windows) {
      host(c, 'cpu_read', [address, length, 1]); const b = arena(c);
      for (let i = 0; i < length; i++) model.set(address + i, b[140 + i]);
    }
    return model;
  }
  function invoke(row, oracle, owner, entries) {
    const instance = new WebAssembly.Instance(engine.module, {}), c = {id: ++counts.contexts, owner, entries, memory: instance.exports.memory, api: {}};
    for (const [name, fn] of Object.entries(instance.exports)) if (name.startsWith('ring3_abi_v1_') && typeof fn === 'function') c.api[name.slice(13)] = fn;
    host(c, 'open', [12, c.id, HIGH]); c.base = c.api.arena_ptr() >>> 0; const initial = arena(c);
    for (const page of pages) {host(c, 'map', [page, 1, page === PC ? 7 : 3]); if (page !== PC) upload(c, page, Buffer.alloc(4096, 0xa5));}
    for (const [address, b] of row.data ?? []) upload(c, address, b);
    const code = Buffer.from([...row.code, 0xeb, 0]); upload(c, PC, code);
    const beforeRam = peek(c);
    input(c, 140, words(entries ? [PC] : [PC, code.length]));
    host(c, owner === 'resident' ? entries ? 'compile_resident_entries' : 'compile_resident' : entries ? 'compile_entries' : 'compile', entries ? [1, 0] : [1]);
    const a = arena(c); c.unit = owner === 'resident' ? [a.readUInt32LE(148), a.readUInt32LE(152)] : [];
    const pointer = owner === 'resident' ? a.readUInt32LE(156) : c.api.module_ptr() >>> 0, length = owner === 'resident' ? a.readUInt32LE(160) : c.api.module_len() >>> 0;
    const bytes = Buffer.from(new Uint8Array(c.memory.buffer, pointer, length)), module = new WebAssembly.Module(bytes), name = owner === 'resident' ? 'stack_execute_resident' : 'stack_execute';
    assert.deepEqual(WebAssembly.Module.imports(module), [{module: 'env', name: 'memory', kind: 'memory'}, ...[owner === 'resident' ? 'guard_resident' : 'guard', name].map(name => ({module: 'ring3', name, kind: 'function'}))]);
    assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
    let calls = 0; const ring3 = {...c.api};
    ring3[name] = (...args) => {
      calls++; const helperBefore = arena(c), normalizedArgs = args.map(v => v >>> 0);
      const kind = row.action === 'push' ? row.width === 2 ? 1 : 0 : row.action === 'pop' ? row.width === 2 ? 3 : 2 : row.width === 2 ? 5 : 4;
      const control = (kind | (row.allocation ?? 0) << 8 | (row.level ?? 0) << 24) >>> 0;
      assert.deepEqual(normalizedArgs, [...(owner === 'resident' ? [c.id, HIGH, ...c.unit] : []), control], 'exact helper binding and control');
      assert.deepEqual(helperBefore.subarray(140, 172), words(registers), 'all original GPRs captured before helper');
      let status;
      if (row.synthetic) {input(c, 100, packet(row.synthetic.packet)); status = row.synthetic.status ?? 0;}
      else status = c.api[name](...args);
      const helperAfter = arena(c), allowed = Buffer.from(helperBefore);
      allowed.set(helperAfter.subarray(100, 172), 100);
      assert.deepEqual(helperAfter, allowed, 'helper modifies only packet and staged GPR payload');
      helperCalls.push({context: c.id, name, args: normalizedArgs, status, input: helperBefore.subarray(140, 172).toString('hex'), packet: helperAfter.subarray(100, 140).toString('hex'), staged: helperAfter.subarray(140, 172).toString('hex')});
      return status;
    };
    const run = new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3}).exports.run;
    writeFileSync(join(output, `module-${c.id}.wasm`), bytes, {flag: 'wx'}); modules.push({id: c.id, bytes: bytes.length, sha256: hash(bytes)});
    input(c, 0, initial); input(c, 140, Buffer.alloc(64, 0xe7)); input(c, 100, packet([0, 0xabc, 0, 0, 0, 0]));
    if (row.cache !== undefined) {
      const segments = Buffer.from(initial.subarray(SEG, SEG + 64)); segments.writeUInt16LE(2, 4);
      defaultCaches.forEach((v, i) => segments.writeUInt32LE(v >>> 0, cacheOffsets[i])); segments.writeUInt32LE(row.cache >>> 0, cacheOffsets[2]); input(c, SEG, segments);
    }
    const registers = [0x11112222, 0x22223333, 0x33334444, 0x44445555, row.sp ?? 0x8080, row.bp ?? 0x8080, 0x66667777, 0x77778888];
    input(c, 0, record('R3ST', [...registers, PC, 0xcd7], 1, 56)); input(c, 96, words([row.cancel ?? 0]));
    if (row.deny) host(c, 'protect', [row.deny[0], 1, row.deny[1]]);
    const before = arena(c), expected = Buffer.from(before), expectedRam = new Map(beforeRam);
    const control = row.budget === 0 || row.cancel, fault = row.fault || row.synthetic, reason = row.cancel ? 2 : control ? 1 : row.synthetic ? row.synthetic.reason ?? 7 : row.fault ? row.fault[0] === 7 ? 15 : 5 : row.smc ? 6 : 1;
    const retired = control || fault ? 0 : 1, next = retired ? oracle(registers, expectedRam, row) : registers;
    expected.set(record('R3ST', [...next, PC + (retired ? row.code.length : 0), 0xcd7], 1, 56));
    expected.set(record('R3EX', [reason, retired, ...(reason === 5 ? row.fault.slice(2) : reason === 7 ? [row.synthetic?.detail ?? 2, 0, 0, 0] : [0, 0, 0, 0])], 6), 56);
    if (!control) {expected.set(words(retired ? next : registers), 140); expected.set(packet(row.synthetic ? row.synthetic.packet : row.fault ?? [0, 0, 0, 0, 0, 0]), 100);}
    const status = run(c.base, c.base + 56, row.budget ?? 1, c.base + 96), after = arena(c);
    if (row.deny) host(c, 'protect', [row.deny[0], 1, 3]); const afterRam = peek(c);
    let error = null;
    try {assert.equal(status, 0); assert.deepEqual(after, expected, 'full arena'); assert.deepEqual(afterRam, expectedRam, 'all selected RAM windows'); assert.equal(calls, control ? 0 : 1);}
    catch (e) {error = e.message.split('\n')[0];}
    const frame = frames.length; frames.push(before, after); ramFrames.push(Buffer.from([...beforeRam.values()]), Buffer.from([...afterRam.values()])); exits.push(Buffer.from(after.subarray(56, 96)));
    observations.push({owner, entries, name: row.name, frame, passed: error === null, error, expected: {reason, retired}, actual: {reason: after.readUInt32LE(72), retired: after.readUInt32LE(76)}, calls});
    counts.runs++; counts[control ? 'controls' : row.synthetic ? 'synthetic' : fault ? 'failures' : 'successes']++; assert.deepEqual(Buffer.from(new Uint8Array(c.memory.buffer, pointer, length)), bytes, 'published module bytes retained');
    host(c, 'close');
  }
  return {
    run(rows, oracle) {
      assert.equal(new Set(rows.map(row => row.name)).size, rows.length, 'unique scenario names');
      schedule = {rows: rows.length, profiles: 4, expectedRuns: rows.length * 4, scenarios: rows.map(({name, code}) => ({name, code: Buffer.from(code).toString('hex')}))};
      for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) for (const row of rows) invoke(row, oracle, owner, entries);},
    finish() {
      assert.deepEqual(pins(), sourcePins); assert.equal(counts.runs, schedule.expectedRuns); const failed = observations.filter(r => !r.passed), status = failed.length ? 'counterfactual-failure' : 'ok';
      for (const [name, b] of [['arenas.bin', Buffer.concat(frames)], ['ram.bin', Buffer.concat(ramFrames)], ['exit-records.bin', Buffer.concat(exits)]]) writeFileSync(join(output, name), b, {flag: 'wx'});
      writeFileSync(join(output, 'result.json'), JSON.stringify({status, family, engine_sha256: engine.sha256, source_pins: sourcePins, counts, schedule, observations, helpers: helperCalls, modules, windows, limits: ['Ring3 checks all cached SS authority before RAM under its synchronous batch contract; no universal competing-fault priority claim', 'fixed SS.B1/base0 protected32 environment; no AM1 or mode-switch claim']}, null, 2) + '\n');
      console.log(JSON.stringify({status, family, ...counts, failed: failed.length, output})); if (failed.length) process.exitCode = 1;
    },
  };
}
