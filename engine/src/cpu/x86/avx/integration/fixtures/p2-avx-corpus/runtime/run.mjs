import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, readdirSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../../../../../../../../tests/support/wasm/engine.mjs';
import {assertEngineExports} from '../../../../../../../../tests/support/wasm/exports.mjs';
import {ARENA_SIZE} from '../../../../../../../../tests/support/wasm/arena.mjs';

const [enginePath, output, root] = process.argv.slice(2), engine = readEngine(enginePath);
const receipt = assertEngineExports(engine.module, root);
const hash = value => createHash('sha256').update(value).digest('hex');
function ownerFiles(path) {
  return readdirSync(join(root, path), {withFileTypes: true}).flatMap(item => {
    const child = `${path}/${item.name}`;
    return item.isDirectory() ? ownerFiles(child) : /\.(rs|tsv|mjs)$/.test(item.name) ? [child] : [];
  });
}
const sourcePaths = [...new Set([...receipt.sourcePaths, 'engine/tests/support/wasm/engine.mjs', 'engine/tests/support/wasm/arena.mjs',
  ...ownerFiles('engine/src/cpu/x86/avx'), ...ownerFiles('engine/src/cpu/x86/decode/avx'), ...ownerFiles('engine/src/cpu/dbt/wasm/instructions/avx'),
  'engine/src/cpu/x86/simd/mod.rs', 'engine/src/cpu/x86/simd/state.rs', 'engine/src/cpu/x86/simd/operation.rs', 'engine/src/cpu/x86/simd/descriptor.rs', 'engine/src/cpu/x86/simd/execute.rs', 'engine/src/cpu/x86/xstate/fxsave.rs', 'engine/src/process/cpu/simd/arena.rs', 'engine/src/cpu/dbt/wasm/helpers/result.rs', 'engine/src/cpu/dbt/wasm/emitter/mod.rs', 'engine/src/cpu/dbt/wasm/emitter/module.rs', 'engine/src/cpu/dbt/wasm/emitter/body.rs', 'engine/src/cpu/dbt/wasm/requirements.rs', 'engine/src/cpu/dbt/wasm/imports/wiring.rs',
  'engine/src/cpu/dbt/wasm/instructions/integer/dispatch.rs', 'engine/src/cpu/dbt/wasm/imports/mod.rs', 'engine/src/cpu/dbt/wasm/imports/binding.rs', 'engine/src/cpu/dbt/wasm/imports/context.rs', 'engine/src/cpu/dbt/wasm/imports/needs.rs', 'engine/src/cpu/dbt/wasm/helpers/protocol.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/mod.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/address.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/scalar.rs', 'engine/src/cpu/dbt/wasm/instructions/memory/stack.rs', 'engine/src/process/cpu/avx.rs',
  'engine/src/cpu/dbt/wasm/detection.rs', 'engine/src/process/wasm/detection.rs',
  'engine/src/process/cpu/events/detection.rs', 'engine/src/process/execution/authority.rs',
  'engine/src/abi/arena.rs', 'engine/src/abi/x86/avx.rs', 'engine/src/abi/x86/ymm.rs', 'engine/src/cpu/x86/capabilities.rs'])].sort();
const pins = () => Object.fromEntries(sourcePaths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = pins(), cases = JSON.parse(readFileSync(join(output, 'cases.json'), 'utf8'));
const SIZE = ARENA_SIZE, PAGE = 0x5000, ADDRESS = 0x5020, PC = 0x1000, T = 140, keyHigh = 0x434f5250;
const files = Object.fromEntries(['input', 'expected', 'ram', 'expected-ram'].map(name => [name, readFileSync(join(output, name + '.bin'))]));
assert.equal(cases.length, 688); assert.equal(new Set(cases.map(row => row.name)).size, 374);
for (const name of ['input', 'expected']) assert.equal(files[name].length, SIZE * cases.length);
for (const name of ['ram', 'expected-ram']) assert.equal(files[name].length, 4096 * cases.length);
const frame = (name, row) => files[name].subarray(row.index * (name.includes('ram') ? 4096 : SIZE), (row.index + 1) * (name.includes('ram') ? 4096 : SIZE));
const counts = {contexts: 0, modules: 0, runs: 0, forms: 688, codes: 374, memory: 0, stores: 0, helper: 0, direct: 0};
const words = values => {const b = Buffer.alloc(values.length * 4); values.forEach((value, i) => b.writeUInt32LE(value >>> 0, i * 4)); return b;};
const arena = c => Buffer.from(new Uint8Array(c.memory.buffer, c.base, SIZE));
function input(c, offset, value) {new Uint8Array(c.memory.buffer).set(value, c.base + offset);}
function host(c, name, args = []) {assert.equal(c.api[name](...args), 0, name);}
function open(owner, entries) {
  const instance = new WebAssembly.Instance(engine.module, {}), c = {owner, entries, memory: instance.exports.memory, api: {}, calls: [], guards: [], events: []};
  for (const [name, value] of Object.entries(instance.exports)) if (name.startsWith('ring3_abi_v1_') && typeof value === 'function') c.api[name.slice(13)] = value;
  for (const [name, arity] of Object.entries({open: 3, map: 3, upload: 2, compile: 1, compile_entries: 2,
    compile_resident: 1, compile_resident_entries: 2, cpu_read: 3, cpu_write: 3, cpu_write_resident: 7,
    avx_execute: 6, avx_execute_resident: 10})) assert.equal(c.api[name]?.length, arity, name);
  c.key = ++counts.contexts; host(c, 'open', [8, c.key, keyHigh]); c.base = c.api.arena_ptr() >>> 0;
  assert.equal(c.api.arena_len(), SIZE);
  host(c, 'map', [PC, 1, 7]); host(c, 'map', [PAGE, 1, 3]);
  return c;
}
function upload(c, address, value) {input(c, T, value); host(c, 'upload', [address, value.length]);}
function compile(c, row) {
  if (c.resident) {input(c, 96, words([0])); host(c, 'discard_unacknowledged_resident', [c.key, keyHigh, ...c.resident]);}
  const code = Buffer.from(row.code, 'hex'); upload(c, PC, Buffer.concat([code, Buffer.from('ebfe', 'hex')]));
  input(c, T, words(c.entries ? [PC] : [PC, code.length + 2]));
  host(c, c.owner === 'resident' ? c.entries ? 'compile_resident_entries' : 'compile_resident' : c.entries ? 'compile_entries' : 'compile', [1, ...(c.entries ? [0] : [])]);
  const a = arena(c);
  c.generation = c.api.generation() >>> 0;
  if (c.owner === 'resident') c.resident = [a.readUInt32LE(T + 8), a.readUInt32LE(T + 12)];
  const pointer = c.owner === 'resident' ? a.readUInt32LE(T + 16) : c.api.module_ptr() >>> 0;
  const length = c.owner === 'resident' ? a.readUInt32LE(T + 20) : c.api.module_len() >>> 0;
  const binary = Buffer.from(new Uint8Array(c.memory.buffer, pointer, length));
  const modulePath = join(output, `module-${++counts.modules}.wasm`);
  writeFileSync(modulePath, binary, {flag: 'wx'});
  let module;
  try {
    module = new WebAssembly.Module(binary);
  } catch (error) {
    throw new Error(`${row.index}/${row.name}: generated module validation failed (${modulePath})`, {cause: error});
  }
  const guard = c.owner === 'resident' ? 'guard_detect_resident' : 'guard_detect';
  const names = ['env:memory:memory', `ring3:${guard}:function`, 'ring3:finish_exception_detection:function'];
  if (row.helper) names.push(`ring3:${c.owner === 'resident' ? 'avx_execute_resident' : 'avx_execute'}:function`);
  else if (row.length) names.push(`ring3:${row.access === 1 ? 'cpu_read' : c.owner === 'resident' ? 'cpu_write_resident' : 'cpu_write'}:function`);
  assert.deepEqual(WebAssembly.Module.imports(module).map(item => `${item.module}:${item.name}:${item.kind}`).sort(), names.sort(), `${row.name}: exact instruction authority imports`);
  assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  const ring3 = {...c.api};
  ring3[guard] = (...args) => {
    const epoch = c.api[guard](...args);
    c.guards.push({args: args.map(value => value >>> 0), epoch});
    return epoch;
  };
  ring3.finish_exception_detection = (...args) => {
    const status = c.api.finish_exception_detection(...args);
    c.events.push({args, status});
    return status;
  };
  for (const name of ['cpu_read', 'cpu_write', 'cpu_write_resident', 'avx_execute', 'avx_execute_resident']) ring3[name] = (...args) => {
    const status = c.api[name](...args); c.calls.push({name, status, args: args.map(value => value >>> 0)}); return status;
  };
  return new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3}).exports.run;
}
function checkRam(c, expected, label) {
  const result = Buffer.alloc(4096);
  for (let offset = 0; offset < 4096; offset += 512) {
    host(c, 'cpu_read', [PAGE + offset, 512, 1]); assert.equal(arena(c).readUInt32LE(116), 0);
    arena(c).copy(result, offset, T, T + 512);
  }
  assert.deepEqual(result, expected, `${label}: complete mapped data page and canaries`);
}
for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) {
  const c = open(owner, entries);
  for (const row of cases) {
    const run = compile(c, row);
    const original = Buffer.from(row.original, 'hex'), code = Buffer.from(row.code, 'hex');
    assert.equal(original.length, code.length, 'relocation preserves instruction extent');
    if (!original.equals(code)) {
      const address = Buffer.from('44332211', 'hex'), index = original.indexOf(address);
      assert.notEqual(index, -1); assert.equal(original.indexOf(address, index + 1), -1);
      const expected = Buffer.from(original); expected.writeUInt32LE(ADDRESS, index); assert.deepEqual(code, expected, 'only canonical absolute displacement relocated');
    }
    for (let start = 0; start < 4096; start += 512) upload(c, PAGE + start, frame('ram', row).subarray(start, start + 512));
    input(c, 0, frame('input', row)); c.calls = []; c.guards = []; c.events = [];
    assert.equal(run(c.base, c.base + 56, 1, c.base + 96), 0, `${row.name}: generated run`);
    assert.deepEqual(arena(c), frame('expected', row), `${owner}/${entries ? 'entries' : 'explicit'}/${row.index}/${row.name}: full canonical arena`);
    assert.equal(arena(c).readUInt32LE(76), 1, 'exactly one retired instruction');
    assert.equal(arena(c).readUInt32LE(48), PC + code.length, 'next guest EIP');
    assert.equal(c.guards.length, 1, 'exactly one real application entry guard');
    assert.deepEqual(c.guards[0].args, [c.key, keyHigh,
      ...(owner === 'resident' ? c.resident : [c.generation]), c.base, c.base + 56, c.base + 96], 'exact material identity and arena pointers');
    assert.ok(typeof c.guards[0].epoch === 'bigint' && c.guards[0].epoch > 0n, 'real entry detection epoch');
    assert.equal(c.events.length, 0, 'successful budget stop creates no architectural event');
    assert.equal(c.calls.length, Number(row.helper || row.length !== 0), 'exact helper count');
    for (const call of c.calls) {
      assert.equal(call.status, 0, 'helper transport success');
      if (row.helper) {
        const args = call.args.slice(-6);
        assert.deepEqual(args.slice(0, 3), [row.descriptor, row.operands, row.immediate], 'exact canonical descriptor/operands/immediate');
      } else assert.deepEqual(call.args.slice(-3), [ADDRESS, row.length, row.alignment === 32 ? 1 : row.alignment], 'exact checked direct memory span');
      if (owner === 'resident' && call.name.endsWith('_resident')) assert.deepEqual(call.args.slice(0, 4), [c.key, keyHigh, ...c.resident], 'exact resident authority');
    }
    const actual = arena(c); checkRam(c, frame('expected-ram', row), row.name); input(c, 0, actual);
    counts.runs++; counts.helper += Number(row.helper); counts.direct += Number(!row.helper);
    counts.memory += Number(row.length !== 0); counts.stores += Number(row.access === 2);
  }
  host(c, 'close');
}
assert.equal(counts.runs, 2752); assert.equal(counts.modules, 2752);
assert.deepEqual(pins(), sourcePins, 'all pinned owners unchanged during corpus proof');
writeFileSync(join(output, 'result.json'), JSON.stringify({scope: 'canonical688 byte forms through decoder/generated guest/publication across4 authorities; expected architectural state from existing native CPU kernels, no independent numeric claim', engineSha256: engine.sha256, counts, sourcePins,
  oracleFiles: Object.fromEntries(['cases.json', 'input.bin', 'expected.bin', 'ram.bin', 'expected-ram.bin'].map(name => [name, hash(readFileSync(join(output, name)))]))}, null, 2) + '\n');
console.log(JSON.stringify({status: 'PASS', ...counts}));
