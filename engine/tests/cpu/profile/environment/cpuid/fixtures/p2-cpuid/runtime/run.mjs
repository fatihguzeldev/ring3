import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {readEngine} from '../../../../../../../support/wasm/engine.mjs';
import {arena, checkContext, checkImports, input, PC, words} from './checks.mjs';

const [enginePath, output, root] = process.argv.slice(2), engine = readEngine(enginePath);
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const paths = ['engine/src/cpu/dbt/wasm/integer_system.rs', 'engine/src/cpu/dbt/wasm/integer/dispatch.rs',
  'engine/src/cpu/x86/capabilities.rs',
  'engine/src/cpu/x86/decode/profile.rs', 'engine/src/cpu/x86/decode/lower.rs',
  'engine/tests/cpu/profile/environment/cpuid/native.rs', 'engine/tests/cpu/profile/environment/cpuid/wasm.rs',
  ...['oracle.mjs', 'checks.mjs', 'native.mjs', 'run.mjs'].map(name => 'engine/tests/cpu/profile/environment/cpuid/fixtures/p2-cpuid/runtime/' + name)];
const pins = () => Object.fromEntries(paths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = pins(), modules = [], observations = []; let runs = 0, contexts = 0;
for (const resident of [false, true]) for (const entries of [false, true]) {
  const instance = new WebAssembly.Instance(engine.module, {}), api = {};
  for (const [name, fn] of Object.entries(instance.exports)) if (name.startsWith('ring3_abi_v1_') && typeof fn === 'function') api[name.slice(13)] = fn;
  const id = ++contexts, context = {memory: instance.exports.memory, base: 0, label: `${resident ? 'resident' : 'replacement'}-${entries ? 'entry' : 'extent'}`};
  assert.equal(api.open(1, id, 0x43505549), 0); context.base = api.arena_ptr() >>> 0;
  assert.equal(api.map(PC, 1, 7), 0); input(context, 140, Buffer.from([0x0f, 0xa2, 0xeb, 0]));
  assert.equal(api.upload(PC, 4), 0); assert.equal(api.protect(PC, 1, 4), 0);
  input(context, 140, words(entries ? [PC] : [PC, 4]));
  const method = resident ? (entries ? 'compile_resident_entries' : 'compile_resident') : (entries ? 'compile_entries' : 'compile');
  assert.equal(api[method](1, ...(entries ? [0] : [])), 0);
  const before = arena(context), pointer = resident ? before.readUInt32LE(156) : api.module_ptr() >>> 0;
  const length = resident ? before.readUInt32LE(160) : api.module_len() >>> 0;
  const bytes = Buffer.from(new Uint8Array(context.memory.buffer, pointer, length)), module = new WebAssembly.Module(bytes);
  checkImports(module, resident); context.run = new WebAssembly.Instance(module, {env: {memory: context.memory}, ring3: api}).exports.run;
  const file = `module-${id}.wasm`; writeFileSync(join(output, file), bytes, {flag: 'wx'});
  modules.push({context: context.label, file, sha256: hash(bytes), bytes: length});
  runs += checkContext(context, 2, row => observations.push(row));
}
assert.deepEqual(pins(), sourcePins, 'CPUID proof source remains frozen');
const result = {schema: 1, engineSha256: engine.sha256, sourcePins, contexts, runs, modules, observations,
  claim: 'CPUID software profile coherence and retirement; no full ISA, browser, game, or throughput claim'};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2));
console.log(JSON.stringify({contexts, runs, engineSha256: engine.sha256}));
