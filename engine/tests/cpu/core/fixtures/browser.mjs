import {instantiateCore,coreImports} from './link.mjs';
import {makeKernel} from './kernels.mjs';

const query = new URLSearchParams(location.search), status = document.querySelector('#status');
const button = document.querySelector('#run'), engineInput = document.querySelector('#engine');
const shaInput = document.querySelector('#sha'), output = document.querySelector('#core-report');
engineInput.value = query.get('engine') ?? engineInput.value;
shaInput.value = query.get('sha') ?? '';
const ensure = (condition, message) => { if (!condition) throw new Error(message); };
const hex = bytes => Array.from(bytes, value => value.toString(16).padStart(2, '0')).join('');
const equal = (actual, expected, label) => {
  ensure(actual.length === expected.length && actual.every((value, index) => value === expected[index]), label);
};
const hash = async bytes => hex(new Uint8Array(await crypto.subtle.digest('SHA-256', bytes)));
let report;
function checkpoint(message) {
  window.__coreReport = report; output.textContent = JSON.stringify(report);
  if (message) status.textContent = message;
}

function ok(context, name, ...args) {
  const result = context.api[name](...args) >>> 0;
  ensure(result === 0, `${name}: status ${result}`);
}
const bytes = (context, pointer, length) => new Uint8Array(context.memory.buffer).slice(pointer, pointer + length);
const word = (context, pointer) => new DataView(context.memory.buffer).getUint32(pointer, true);
const set = (context, pointer, value) => new DataView(context.memory.buffer).setUint32(pointer, value >>> 0, true);

function reset(context, kernel) {
  for (const [address, source] of [[0x3000, kernel.initialData], [0x4000, kernel.initialStack]]) {
    new Uint8Array(context.memory.buffer, context.arena + 140, source.length).set(source);
    ok(context, 'upload', address, source.length);
  }
  if (context.pointer) {
    kernel.initialRegs.forEach((value, index) => set(context, context.pointer + index * 4, value));
    set(context, context.pointer + 32, 0x1000); set(context, context.pointer + 36, 2);
  }
}

async function fresh(module, kernel) {
  const begin = performance.now();
  const instance = await WebAssembly.instantiate(module, {
    ring3_observability: {now_ns: () => BigInt(Date.now()) * 1000000n},
  });
  const context = {instance, memory: instance.exports.memory, opened: false, active: false};
  context.api = Object.fromEntries(Object.entries(instance.exports)
    .filter(([name]) => name.startsWith('ring3_abi_v1_')).map(([name, value]) => [name.slice(13), value]));
  try {
    ok(context, 'open', 16, 97, 0); context.opened = true;
    context.arena = context.api.arena_ptr() >>> 0;
    for (const [address, permissions] of [[0x1000, 7], [0x3000, 3], [0x4000, 3]]) ok(context, 'map', address, 1, permissions);
    new Uint8Array(context.memory.buffer, context.arena + 140, kernel.code.length).set(kernel.code);
    ok(context, 'upload', 0x1000, kernel.code.length);
    reset(context, kernel);
    const initial = new Uint8Array(context.memory.buffer, context.arena, 56); initial.fill(0);
    initial.set([0x52, 0x33, 0x53, 0x54]);
    const view = new DataView(context.memory.buffer);
    view.setUint16(context.arena + 4, 3, true); view.setUint16(context.arena + 6, 1, true);
    set(context, context.arena + 8, 56);
    kernel.initialRegs.forEach((value, index) => set(context, context.arena + 16 + index * 4, value));
    set(context, context.arena + 48, 0x1000); set(context, context.arena + 52, 2);
    const prepareStart = performance.now(); ok(context, 'core_prepare', 0x1000, 64, 8);
    const prepareEnd = performance.now();
    context.pointer = context.api.core_context_ptr() >>> 0;
    const pointer = context.api.core_code_ptr() >>> 0, length = context.api.core_code_len() >>> 0;
    ensure(context.pointer && pointer && length, 'native code/context descriptor');
    const code = bytes(context, pointer, length);
    const moduleStart = performance.now(), generated = new WebAssembly.Module(code), moduleEnd = performance.now();
    const imports = coreImports(generated).sort((a, b) => a.name.localeCompare(b.name));
    ensure(JSON.stringify(imports) === JSON.stringify([
      {module: 'ring3', name: 'memory', kind: 'memory'},
      {module: 'ring3', name: 'read', kind: 'function'},
      {module: 'ring3', name: 'write', kind: 'function'},
    ]), 'only declared native imports');
    const instanceStart = performance.now();
    context.run = instantiateCore(generated, {ring3: {
      memory: context.memory, read: context.api.core_read, write: context.api.core_write,
    }}).exports.run;
    context.cold = {native_prepare_ms: prepareEnd - prepareStart, wasm_module_ms: moduleEnd - moduleStart,
      wasm_instance_ms: performance.now() - instanceStart, setup_ms: performance.now() - begin};
    context.moduleBytes = length; context.moduleSha256 = await hash(code);
    context.initialFp = bytes(context, context.arena + 4236, 128);
    return context;
  } catch (error) { close(context); throw error; }
}

function close(context) {
  if (context.active) { ok(context, 'core_end'); context.active = false; }
  if (context.opened) { ok(context, 'close'); context.opened = false; }
}

function pages(context) {
  const saved = bytes(context, context.arena, context.api.arena_len() >>> 0), result = [];
  try {
    for (const address of [0x1000, 0x3000, 0x4000]) {
      const page = new Uint8Array(4096), view = new DataView(page.buffer);
      for (let offset = 0; offset < 4096; offset += 4) {
        ok(context, 'read32', address + offset);
        ensure(word(context, context.arena + 116) === 0, 'page readback success');
        view.setUint32(offset, word(context, context.arena + 120), true);
      }
      result.push(page);
    }
    return result;
  } finally { new Uint8Array(context.memory.buffer, context.arena, saved.length).set(saved); }
}

async function execute(context, kernel, result, phase, round) {
  reset(context, kernel);
  ok(context, 'core_begin'); context.active = true;
  ensure(word(context, context.pointer + 112) === 1, 'native active lease');
  const priorRetired = word(context, context.pointer + 40), budget = kernel.retired + 1;
  const start = performance.now(), reason = context.run(context.pointer, budget) >>> 0, end = performance.now();
  const state = bytes(context, context.pointer, 128), stateView = new DataView(state.buffer);
  ok(context, 'core_end'); context.active = false;
  const actual = pages(context);
  const row = {phase, round, reason, budget, execute_ms: end - start, stateHex: hex(state),
    pageSha256: await Promise.all(actual.map(hash))};
  result.runs.push(row); checkpoint(`${kernel.kind}: ${phase} ${round + 1}`);
  ensure(reason === 4, 'unsupported terminal stop');
  equal(Array.from({length: 8}, (_, index) => stateView.getUint32(index * 4, true)), kernel.regs, 'full GPR result');
  ensure(stateView.getUint32(32, true) === kernel.pc, 'terminal PC');
  ensure(stateView.getUint32(36, true) === kernel.flags, 'complete FLAGS');
  ensure(stateView.getUint32(40, true) - priorRetired === kernel.retired, 'exact retired work');
  ensure(stateView.getUint32(44, true) === 1, 'remaining budget');
  ensure(stateView.getUint32(48, true) === 0 && stateView.getUint32(52, true) === 0, 'no guest memory fault');
  ensure(word(context, context.pointer + 112) === 0, 'closed native lease');
  [kernel.code, kernel.expectedData, kernel.expectedStack].forEach((expected, index) => equal(actual[index], expected, `whole guest page ${index}`));
  equal(bytes(context, context.arena + 4236, 128), context.initialFp, 'integer program preserves FP bank');
}

button.addEventListener('click', async () => {
  button.disabled = true;
  report = {schema: 1, complete: false, userAgent: navigator.userAgent, origin: location.origin,
    clock: 'window performance.now elapsed milliseconds', cases: [], nativeInstances: 0};
  const contexts = [];
  let primaryError;
  try {
    const expected = shaInput.value.trim(); ensure(/^[a-f0-9]{64}$/.test(expected), 'supply the exact engine SHA-256');
    const iterations = Number(query.get('iterations') ?? 131072), rounds = Number(query.get('rounds') ?? 8);
    ensure(Number.isInteger(rounds) && rounds >= 1 && rounds <= 16, 'rounds must be 1..16');
    report.iterations = iterations; report.rounds = rounds; checkpoint('fetching engine');
    const response = await fetch(engineInput.value); ensure(response.ok, `engine HTTP ${response.status}`);
    const encoded = new Uint8Array(await response.arrayBuffer()); report.engineSha256 = await hash(encoded);
    ensure(report.engineSha256 === expected, 'engine hash mismatch');
    const module = await WebAssembly.compile(encoded);
    for (const kind of ['integer', 'call']) {
      const kernel = makeKernel(kind, iterations), context = await fresh(module, kernel);
      contexts.push({context, kernel}); report.nativeInstances++;
      report.cases.push({kind, retiredPerRun: kernel.retired, guestCodeSha256: await hash(kernel.code),
        moduleSha256: context.moduleSha256, moduleBytes: context.moduleBytes, cold: context.cold, runs: []});
    }
    for (let round = 0; round < 2 + rounds; round++) {
      const order = round & 1 ? [1, 0] : [0, 1];
      for (const index of order) await execute(contexts[index].context, contexts[index].kernel,
        report.cases[index], round < 2 ? 'warmup' : 'timed', round < 2 ? round : round - 2);
    }
  } catch (error) { primaryError = error; report.failure = {message: String(error), stack: error.stack}; }
  finally {
    report.closedInstances = 0;
    for (const {context} of contexts) {
      try { close(context); report.closedInstances++; }
      catch (error) { (report.cleanupErrors ??= []).push(String(error)); primaryError ??= error; }
    }
    report.complete = !primaryError;
    checkpoint(report.complete ? `PASS: ${report.cases.length} kernels, ${report.cases.reduce((sum, item) => sum + item.runs.length, 0)} runs` : `FAILED: ${String(primaryError)}`);
    button.disabled = false;
  }
});
