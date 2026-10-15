import assert from 'node:assert/strict';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {pathToFileURL} from 'node:url';
import {rows, protocolRows, profiles} from './cases.mjs';
import {runGeneric} from './generic.mjs';
import {runFetch} from './fetch.mjs';
import {PC, FLAGS, TRANSFER, word, record, call, seed, helper,
  sourceStop, stagedHelper, entered, sourceCalls, helperName, assertCapture} from './oracle.mjs';
const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root);
const support = 'engine/tests/cpu/segments/code/base/fixtures/p2-cs-base/runtime/support.mjs';
const {harness, writeRAM, hash} = await import(pathToFileURL(join(root, support)));
const h = await harness(enginePath, output, root);
const prefix = 'engine/tests/cpu/control/exceptions/memory/';
const sourcePaths = [support, prefix + 'wasm.rs', ...['run.mjs', 'cases.mjs', 'oracle.mjs', 'generic.mjs', 'images.mjs', 'fetch.mjs'].map(p => prefix + 'fixtures/p2-application-memory/runtime/' + p),
  'engine/src/memory/space/paging.rs', 'engine/src/cpu/x86/paging/inspect.rs',
  'engine/src/process/memory/paging.rs', 'engine/src/process/memory/paging/port.rs',
  'engine/src/process/memory/cpu_scalar.rs', 'engine/src/process/memory/scalar/cpu_write.rs',
  'engine/src/process/memory/span.rs', 'engine/src/process/cpu/fetch.rs',
  'engine/src/process/cpu/avx/gather.rs', 'engine/src/process/cpu/avx/gather/projection.rs',
  'engine/src/cpu/dbt/wasm/detection.rs', 'engine/src/cpu/dbt/wasm/helpers/result/paging.rs',
  'engine/src/cpu/dbt/wasm/code_fetch/paging.rs', 'engine/src/cpu/dbt/wasm/abi.rs',
  'engine/src/process/wasm/paging/transport.rs', 'engine/src/process/wasm/detection.rs',
  'engine/src/abi/cpu_helper/paging.rs', 'engine/src/abi/cpu_helper/fetch.rs',
  'engine/src/abi/x86/exit/paging.rs', 'engine/src/abi/x86/avx/gather/paging.rs'];
assert.equal(new Set(sourcePaths).size, sourcePaths.length);
const pins = () => Object.fromEntries(sourcePaths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = pins(), observed = {contexts: 0, compiles: 0, invocations: 0, captures: 0, cr2: 0},
  hostRows = [], captures = [];
const compile = (c, label, spans, helpers, options) => {observed.compiles++; return h.compile(c, label, spans, helpers, options);};
function cr2(c, label, value) {
  observed.cr2++; const before = h.arena(c), status = c.api.read_cr2(), after = h.arena(c), want = Buffer.from(before);
  if (value !== undefined) want.set(word([value]), TRANSFER);
  const errors = [];
  if (status !== (value === undefined ? 7 : 0)) errors.push('CR2 status distinguishes Unknown from known zero');
  if (!after.equals(want)) errors.push('CR2 exact four-byte transfer or neutral Unknown');
  hostRows.push({context: c.key, label, value: value === undefined ? 'Unknown' : value, status, passed: errors.length === 0,
    errors, before: before.toString('hex'), after: after.toString('hex'), want: want.toString('hex')});
  h.put(c, 0, before); return errors;
}
function configure(c, row) {
  c.capturesExceptions = true;
  for (const [page, permission] of row.rights) {
    if (permission === null) {
      h.host(c, 'unmap', [page, 1]); c.pages = c.pages.filter(mapped => mapped !== page);
    } else h.host(c, 'protect', [page, 1, permission]);
  }
}
function instrument(c, material, row, input) {
  const module = new WebAssembly.Module(new Uint8Array(c.memory.buffer, material.pointer, material.length));
  const checks = [], own = [], ring3 = Object.fromEntries(WebAssembly.Module.imports(module).filter(i => i.kind === 'function').map(i =>
    [i.name, (...args) => {
      if (i.name === 'finish_exception_detection') {
        const got = h.arena(c), expected = sourceStop(row, input);
        assertCapture(got, expected, checks, 'literal complete pre-finish R3EX/helper/vector capture');
        observed.captures++; own.push({kind: 'pre-finish', actual: got, expected, args: args.map(v => typeof v === 'bigint' ? v.toString(10) : v >>> 0)});
      }
      const status = c.api[i.name](...args);
      if (i.name === helperName(c, row)) {
        const beforeMutation = h.arena(c);
        assertCapture(beforeMutation, stagedHelper(row, input), checks,
          'real native helper stages exact packet/scratch without GPR/vector publication');
        if (row.mutation) {
          const mutated = Buffer.from(beforeMutation);
          if (row.mutation === 'width') mutated.writeUInt32LE(2, 136);
          if (row.mutation === 'code') mutated.writeUInt32LE(5, 124);
          if (row.mutation === 'forge' || row.mutation === 'downgrade') mutated.set(helper(row), 100);
          h.put(c, 0, mutated);
        }
        own.push({kind: 'real-helper', actual: beforeMutation, returned_status: status,
          mutation: row.mutation ?? null, afterMutation: h.arena(c)});
      }
      c.calls.push(call(i.name, args, status)); return status;
    }]));
  return {...material, run: new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3}).exports.run, checks, captures: own};
}
function observe(c, label, material, input, want, options = {}) {
  if (!material) {h.skip(c, label, 'source material admission'); return null;}
  observed.invocations++; const result = h.observe(c, label, {material, input, want, ...options});
  for (const error of material.checks ?? []) result.errors.push(error);
  result.passed = result.errors.length === 0;
  for (const capture of material.captures ?? []) {
    const number = captures.length + 1, entry = {context: c.key, label, kind: capture.kind, mutation: capture.mutation ?? null};
    for (const key of ['actual', 'expected', 'afterMutation']) if (capture[key]) {
      const file = `capture-${number}-${key}.bin`; writeFileSync(join(output, file), capture[key], {flag: 'wx'});
      entry[key] = {file, bytes: capture[key].length, sha256: hash(capture[key])};
    }
    if (capture.args) entry.args = capture.args; if (capture.returned_status !== undefined) entry.returned_status = capture.returned_status;
    captures.push(entry);
  }
  return result;
}
function runSource(c, row, input) {
  h.put(c, 0, input);
  const material = compile(c, row.label + ': source', [[PC, row.code.length / 2]], [helperName(c, row), 'finish_exception_detection']);
  const instrumented = material && instrument(c, material, row, input), want = entered(row, input), ram = h.ram(c);
  const result = observe(c, row.label, instrumented, input, want,
    {status: row.protocol === 'witness' ? 7 : 0, helpers: material ? sourceCalls(c, material, row) : [], wantRAM: ram});
  return {result, want, material};
}
function continuation(c, row, input) {
  h.host(c, 'map', [0, 1, 3]); c.pages.push(0); c.pages.sort((a, b) => a - b);
  h.upload(c, 0, Buffer.from('11223344', 'hex'));
  const restored = Buffer.from(input); restored.writeUInt32LE(FLAGS | 0x10000, 52); h.put(c, 0, restored);
  const material = compile(c, 'repaired original MOV after trusted context restore', [[PC, row.code.length / 2]], ['cpu_read32', 'finish_exception_detection']);
  const completed = Buffer.from(restored);
  completed.writeUInt32LE(0x44332211, 16); completed.writeUInt32LE(PC + 5, 48); completed.writeUInt32LE(FLAGS, 52);
  completed.set(record('R3EX', 40, [1, 1, 0, 0, 0, 0], 6), 56);
  completed.set(record('R3MH', 40, [0, 0x44332211, 0, 0, 0, 4], 10), 100);
  observe(c, 'repaired MOV completes once without a second fault', material, restored, completed,
    {helpers: [call('cpu_read32', [0])]});
  cr2(c, 'successful retry retains the detected known-zero CR2', 0);
}
for (const row of [...rows, ...protocolRows]) for (const [resident, entries] of profiles(row)) {
  observed.contexts++;
  const pages = [[PC, 7], [0x2000, 7], [0x7000, 3], ...row.pages];
  assert.equal(new Set(pages.map(([p]) => p)).size, pages.length);
  // initialize authored bytes before selecting the declared final host permissions.
  const c = h.open(resident, entries, row.label, pages.map(([page, permission]) => [page, permission | 2])); c.diagnosticRAM = true;
  for (const [page] of pages) h.upload(c, page, Buffer.alloc(4096, 0x6b));
  h.upload(c, PC, Buffer.from(row.code, 'hex'));
  if (pages.some(([p]) => p === 0)) h.upload(c, 0, Buffer.from('11223344', 'hex'));
  if (pages.some(([p]) => p === 0xfffff000)) h.upload(c, 0xfffffff0, Buffer.from('1122334455667788', 'hex'));
  for (const [page, permission] of pages) if (!(permission & 2)) h.host(c, 'protect', [page, 1, permission]);
  configure(c, row); cr2(c, 'CR2 starts Unknown without a fabricated reset value', undefined);
  const input = seed(h, row), run = runSource(c, row, input);
  cr2(c, 'actual source detection readback', row.success ? undefined : row.linear);
  if (row.continuation && !resident && !entries) {
    if (run.result?.passed) continuation(c, row, run.want);
    else h.skip(c, 'repaired source retry', 'successful source fault stop');
  }
  h.host(c, 'close');
}
const generic = runGeneric({h, compile, observe, cr2, observed, writeRAM});
const fetch = runFetch({h, compile, observe, cr2, observed, writeRAM});
const plannedContexts = [...rows, ...protocolRows].reduce((n, r) => n + profiles(r).length, 0);
const planned = {contexts: plannedContexts + generic.contexts + fetch.contexts,
  compiles: plannedContexts + 1 + generic.compiles + fetch.compiles,
  invocations: plannedContexts + 1 + generic.invocations + fetch.invocations,
  extensions: {generic, fetch},
  note: 'application mapping faults, trusted context retry, scalar/RMW/Gather protocol checks, generic numeric/user-stack and NX fetch'};
assert.deepEqual(pins(), sourcePins, 'paging input/oracle/source owners remained fixed');
writeFileSync(join(output, 'paging-source-pins.json'), JSON.stringify(sourcePins, null, 2) + '\n');
writeFileSync(join(output, 'paging-captures.json'), JSON.stringify(captures, null, 2) + '\n');
writeFileSync(join(output, 'paging-host-rows.json'), JSON.stringify(hostRows, null, 2) + '\n');
h.finish({planned, observed, host_checks: {passed: hostRows.filter(row => row.passed).length, failed: hostRows.filter(row => !row.passed).length}, scope: 'actual application CPU memory: map/protect/unmap authority, full arena and readable mapped RAM, scalar/RMW, partial Gather, numeric/user-stack, live NX fetch and trusted context retry. protocol mutations occur after real native helpers and retain native occurrence transport. no Windows SEH or hardware paging/task/IDT claim.'});
assert.ok(hostRows.every(row => row.passed), 'CR2 exact known/Unknown complete-arena readback');
