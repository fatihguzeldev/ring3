import {instantiateCore,coreImports} from './link.mjs';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {performance} from 'node:perf_hooks';
import {readEngine} from '../../../support/wasm/engine.mjs';
import {Fixture, KEY, pair} from '../../../process/execution/cache/cohort/fixtures/support.mjs';
import {makeKernel} from './kernels.mjs';

const [enginePath, reportPath, count = '131072', roundsText = '8'] = process.argv.slice(2);
const iterations = Number(count), rounds = Number(roundsText);
assert.match(process.env.RING3_ENGINE_SHA256 ?? '', /^[a-f0-9]{64}$/);
assert.ok(Number.isSafeInteger(rounds) && rounds >= 1 && rounds <= 16);
const engine = readEngine(enginePath), hash = value => createHash('sha256').update(value).digest('hex');
const report = {schema: 1, complete: false, node: process.version, engineSha256: engine.sha256,
  iterations, rounds, warmupsPerArm: 2, clock: 'node performance.now, elapsed milliseconds',
  sources: Object.fromEntries(['./timing.mjs','./link.mjs', './kernels.mjs',
    '../../../support/wasm/engine.mjs', '../../../process/execution/cache/cohort/fixtures/support.mjs']
    .map(path => [path, hash(readFileSync(new URL(path, import.meta.url)))])),
  cases: []};
const checkpoint = () => writeFileSync(reportPath, JSON.stringify(report, null, 2));
const copy = (fixture, pointer, length) => fixture.bytes(pointer, length);
const u32 = (bytes, offset) => new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength).getUint32(offset, true);
const u64 = (bytes, offset) => new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength).getBigUint64(offset, true);
const median = values => {
  const sorted = [...values].sort((a, b) => a - b), half = sorted.length >>> 1;
  return sorted.length & 1 ? sorted[half] : (sorted[half - 1] + sorted[half]) / 2;
};

function fresh(kernel, arm) {
  const setupStart = performance.now();
  const fixture = new Fixture(engine.module, [[0x1000, kernel.code]]);
  try {
    fixture.ok('map', 0x3000, 1, 3); fixture.ok('map', 0x4000, 1, 3);
    reset(fixture, kernel);
    let moduleBytes, run, id, context, nativePrepareMs, moduleMs, instanceMs;
    if (arm === 'current') {
      fixture.put(new Uint8Array(new Uint32Array(kernel.blocks.flat()).buffer));
      const prepareStart = performance.now();
      fixture.ok('compile_resident', kernel.blocks.length);
      nativePrepareMs = performance.now() - prepareStart;
      id = fixture.transfer(24).getBigUint64(8, true);
      moduleBytes = fixture.material(id).bytes;
      const moduleStart = performance.now(), module = new WebAssembly.Module(moduleBytes), bindings = {};
      moduleMs = performance.now() - moduleStart;
      for (const entry of coreImports(module)) {
        if (entry.module === 'ring3') {
          assert.equal(typeof fixture.api[entry.name], 'function', entry.name);
          bindings[entry.name] = fixture.api[entry.name];
          assert.equal(bindings[entry.name], fixture.api[entry.name]);
        } else assert.deepEqual(entry, {module: 'env', name: 'memory', kind: 'memory'});
      }
      const instanceStart = performance.now();
      run = instantiateCore(module, {env: {memory: fixture.memory}, ring3: bindings}).exports.run;
      instanceMs = performance.now() - instanceStart;
      fixture.table.set(0, run);
      fixture.ok('acknowledge_resident_installation', ...KEY, ...pair(id), 0);
    } else {
      const prepareStart = performance.now();
      fixture.ok('core_prepare', 0x1000, 64, 8);
      nativePrepareMs = performance.now() - prepareStart;
      const pointer = fixture.api.core_code_ptr() >>> 0, length = fixture.api.core_code_len() >>> 0;
      context = fixture.api.core_context_ptr() >>> 0;
      assert.ok(pointer && length && context);
      moduleBytes = copy(fixture, pointer, length);
      assert.ok(WebAssembly.validate(moduleBytes));
      const moduleStart = performance.now(), module = new WebAssembly.Module(moduleBytes);
      moduleMs = performance.now() - moduleStart;
      assert.deepEqual(coreImports(module).sort((a, b) => a.name.localeCompare(b.name)), [
        {module: 'ring3', name: 'memory', kind: 'memory'},
        {module: 'ring3', name: 'read', kind: 'function'},
        {module: 'ring3', name: 'write', kind: 'function'},
      ]);
      const instanceStart = performance.now();
      run = instantiateCore(module, {ring3: {
        memory: fixture.memory, read: fixture.api.core_read, write: fixture.api.core_write,
      }}).exports.run;
      instanceMs = performance.now() - instanceStart;
    }
    return {fixture, arm, run, id, context, moduleSha256: hash(moduleBytes), moduleBytes: moduleBytes.length,
      cold: {native_prepare_ms: nativePrepareMs, wasm_module_ms: moduleMs, wasm_instance_ms: instanceMs,
        setup_ms: performance.now() - setupStart},
      initialFp: copy(fixture, fixture.base + 4236, 128)};
  } catch (error) { fixture.close(); throw error; }
}

function reset(fixture, kernel) {
  for (const [address, bytes] of [[0x3000, kernel.initialData], [0x4000, kernel.initialStack]]) {
    fixture.put(bytes); fixture.ok('upload', address, bytes.length);
  }
  fixture.reset();
  kernel.initialRegs.forEach((value, index) => fixture.set(16 + index * 4, value));
}

function pages(fixture) {
  const saved = copy(fixture, fixture.base, fixture.api.arena_len() >>> 0), result = [];
  try {
    for (const address of [0x1000, 0x3000, 0x4000]) {
      const page = new Uint8Array(4096), view = new DataView(page.buffer);
      for (let offset = 0; offset < 4096; offset += 4) {
        fixture.ok('read32', address + offset);
        assert.equal(fixture.word(116), 0, 'readback helper success');
        view.setUint32(offset, fixture.word(120), true);
      }
      result.push(page);
    }
    return result;
  } finally { new Uint8Array(fixture.memory.buffer, fixture.base, saved.length).set(saved); }
}

function execute(context, kernel, phase, round, position, result) {
  const {fixture, arm} = context, budget = kernel.retired + 1;
  reset(fixture, kernel);
  if (arm === 'core') {
    const view = new DataView(fixture.memory.buffer);
    kernel.initialRegs.forEach((value, index) => view.setUint32(context.context + index * 4, value, true));
    view.setUint32(context.context + 32, 0x1000, true); view.setUint32(context.context + 36, 2, true);
  }
  const totalStart = performance.now();
  let frame, priorRetired;
  if (arm === 'current') {
    fixture.ok('begin_compiled_execution_receipt', ...KEY, budget, ...pair(context.id), 1);
    frame = copy(fixture, fixture.base + 140, 32);
  } else {
    fixture.ok('core_begin');
    assert.equal(new DataView(fixture.memory.buffer).getUint32(context.context + 112, true), 1);
    priorRetired = new DataView(fixture.memory.buffer).getUint32(context.context + 40, true);
  }
  const beginEnd = performance.now(), executeStart = performance.now();
  const status = arm === 'current'
    ? fixture.dispatch(fixture.base, fixture.base + 56, budget, fixture.base + 96) >>> 0
    : context.run(context.context, budget) >>> 0;
  const executeEnd = performance.now();
  const rawState = arm === 'current' ? copy(fixture, fixture.base, 56) : copy(fixture, context.context, 128);
  const outerExit = arm === 'current' ? copy(fixture, fixture.base + 56, 40) : null;
  const finishStart = performance.now();
  if (arm === 'current') fixture.ok('finish_execution_receipt', ...KEY, ...pair(u64(frame, 16)));
  else fixture.ok('core_end');
  const totalEnd = performance.now();
  const receipt = arm === 'current' ? copy(fixture, fixture.base + 140, 64) : null;
  const actualPages = pages(fixture);
  const row = {arm, phase, round, position, budget, status,
    execute_ms: executeEnd - executeStart, begin_ms: beginEnd - totalStart,
    finish_ms: totalEnd - finishStart, total_ms: totalEnd - totalStart,
    stateHex: Buffer.from(rawState).toString('hex'),
    frameHex: frame ? Buffer.from(frame).toString('hex') : null,
    outerExitHex: outerExit ? Buffer.from(outerExit).toString('hex') : null,
    receiptHex: receipt ? Buffer.from(receipt).toString('hex') : null,
    pageSha256: actualPages.map(hash)};
  result.runs.push(row); checkpoint();
  const offset = arm === 'current' ? 16 : 0;
  assert.deepEqual(Array.from({length: 8}, (_, index) => u32(rawState, offset + index * 4)), kernel.regs);
  assert.equal(u32(rawState, arm === 'current' ? 48 : 32), kernel.pc);
  assert.equal(u32(rawState, arm === 'current' ? 52 : 36), kernel.flags);
  assert.deepEqual(actualPages, [kernel.code, kernel.expectedData, kernel.expectedStack]);
  assert.deepEqual(copy(fixture, fixture.base + 4236, 128), context.initialFp, 'integer kernels preserve complete FP bank');
  if (arm === 'current') {
    assert.equal(status, 0); assert.equal(Buffer.from(receipt.subarray(0, 4)).toString(), 'R3ER');
    assert.equal(u64(receipt, 16) - u64(frame, 24), 1n, 'one admitted owner');
    assert.equal(u64(receipt, 24), context.id); assert.equal(u32(receipt, 32), 0);
    assert.equal(u32(receipt, 36), kernel.retired); assert.equal(u32(receipt, 40), kernel.retired);
    assert.equal(u32(receipt, 44), 20); assert.equal(u32(receipt, 48), 3);
  } else {
    assert.equal(status, 4); assert.equal(u32(rawState, 40) - priorRetired, kernel.retired);
    assert.equal(u32(rawState, 44), 1); assert.equal(u32(rawState, 48), 0); assert.equal(u32(rawState, 52), 0);
    assert.equal(u32(rawState, 112), 1, 'live during generated execution');
    assert.equal(new DataView(fixture.memory.buffer).getUint32(context.context + 112, true), 0, 'end releases lease');
  }
}

for (const kind of ['integer', 'call']) {
  const kernel = makeKernel(kind, iterations), contexts = [];
  const result = {kind, retiredPerRun: kernel.retired, guestCodeSha256: hash(kernel.code), runs: []};
  let primaryError;
  report.cases.push(result); checkpoint();
  try {
    contexts.push(fresh(kernel, 'current')); contexts.push(fresh(kernel, 'core'));
    result.modules = Object.fromEntries(contexts.map(context => [context.arm,
      {sha256: context.moduleSha256, bytes: context.moduleBytes, cold: context.cold}]));
    for (let warmup = 0; warmup < 2; warmup++) {
      for (let index = 0; index < 2; index++) execute(contexts[index], kernel, 'warmup', warmup, index, result);
    }
    for (let round = 0; round < rounds; round++) {
      const order = round & 1 ? [1, 0, 0, 1] : [0, 1, 1, 0];
      for (let position = 0; position < order.length; position++) execute(contexts[order[position]], kernel, 'timed', round, position, result);
    }
    result.medians = Object.fromEntries(['current', 'core'].map(arm => [arm,
      median(result.runs.filter(row => row.phase === 'timed' && row.arm === arm).map(row => row.execute_ms))]));
    result.current_over_core = result.medians.current / result.medians.core;
  } catch (error) { primaryError = error; throw error; }
  finally {
    for (const context of contexts) {
      try {
        if (context.arm === 'core' && new DataView(context.fixture.memory.buffer).getUint32(context.context + 112, true) === 1) {
          context.fixture.ok('core_end');
        }
        context.fixture.close();
      } catch (error) {
        if (!primaryError) throw error;
        (report.cleanupErrors ??= []).push(String(error)); checkpoint();
      }
    }
  }
}
report.complete = true; checkpoint();
console.log(JSON.stringify({complete: true, cases: report.cases.map(({kind, medians, current_over_core}) => ({kind, medians, current_over_core}))}));
