import {createHash} from 'node:crypto';
import {
  closeSync, fsyncSync, mkdirSync, openSync, readFileSync, writeFileSync, writeSync,
} from 'node:fs';
import {loadavg} from 'node:os';
import {dirname, join, resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
import {BUDGET, SIZE, makePlan} from './plan.mjs';

const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const now = () => process.hrtime.bigint();
const elapsed = start => (now() - start).toString();
const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, '../../../..');
const [engineArgument, outputArgument, indexArgument] = process.argv.slice(2);
const processIndex = Number(indexArgument);
const expectedHash = process.env.RING3_ENGINE_SHA256;
const campaignPath = process.env.RING3_PHASE_CAMPAIGN;
if (process.argv.length !== 5 || !engineArgument || !outputArgument
    || !/^[0-2]$/.test(indexArgument ?? '') || typeof globalThis.gc !== 'function'
    || !/^[a-f0-9]{64}$/.test(expectedHash ?? '') || !campaignPath) {
  throw new Error('usage: RING3_ENGINE_SHA256=<sha256> node --expose-gc run.mjs ENGINE_PATH OUTPUT_DIR PROCESS_INDEX');
}

const enginePath = resolve(engineArgument);
const outputDir = resolve(outputArgument);
const campaign = JSON.parse(readFileSync(campaignPath, 'utf8'));
const expectedPins = Object.keys(campaign.source_pins).sort()
  .map(path => ({path, ...campaign.source_pins[path]}));
if (!expectedPins.length || campaign.engine.sha256 !== expectedHash) throw new Error('campaign authority is missing or inconsistent');
mkdirSync(outputDir);
const plan = makePlan();
const selected = plan.processes[processIndex];
const savedPlan = {
  schema_version: plan.schema_version, counts: plan.counts, process: selected,
  shapes: plan.shapes, layout: plan.layout,
};
const planBytes = Buffer.from(JSON.stringify(savedPlan, null, 2) + '\n');
writeFileSync(join(outputDir, 'plan.json'), planBytes, {flag: 'wx'});
const bankFiles = new Map();
for (const context of selected.contexts) {
  for (const bank of context.banks) {
    if (bankFiles.has(bank.file)) continue;
    const bytes = Buffer.from(bank.hex, 'hex');
    writeFileSync(join(outputDir, bank.file), bytes, {flag: 'wx'});
    bankFiles.set(bank.file, {file: bank.file, bytes: bytes.length, sha256: hash(bytes)});
  }
}

function sourcePins() {
  return expectedPins.map(({path}) => {
    const bytes = readFileSync(join(root, path));
    return {path, bytes: bytes.length, sha256: hash(bytes)};
  });
}

function pinsEqual(actual) {
  return actual.length === expectedPins.length && actual.every((pin, index) =>
    pin.path === expectedPins[index].path && pin.bytes === expectedPins[index].bytes
      && pin.sha256 === expectedPins[index].sha256);
}

function environment() {
  return {
    executable: process.execPath, exec_argv: process.execArgv, versions: process.versions,
    load_average: loadavg(),
    gc_policy: 'explicit before each fresh context and timed calls action; outside timers',
  };
}

const events = [], frames = [], modules = [], contexts = [], samples = [], phases = [];
const counts = {
  engine_contexts: 0, opened: 0, closed: 0, child_modules: 0, dispatcher_modules: 0,
  generated_calls: 0, guard_calls: 0, finder_calls: 0, empty_iterations: 0,
  non_loop_host_api_calls: 0, input_events: 0, table_events: 0, events: 0, frames: 0,
  explicit_gc: 0,
};
const rawHash = createHash('sha256');
let rawBytes = 0, journalBytes = 0, checkpointBytes = 0, checkpointIndex = 0;
const rawFd = openSync(join(outputDir, 'arenas.bin'), 'wx');
const journalFd = openSync(join(outputDir, 'journal.ndjson'), 'wx');
const checkpointFd = openSync(join(outputDir, 'checkpoints.ndjson'), 'wx');
let current = null, manifest = null, engineModule = null, failure = null;

function writeAll(fd, bytes) {
  let written = 0;
  while (written < bytes.length) written += writeSync(fd, bytes, written, bytes.length - written);
}

function appendJournal(row) {
  const bytes = Buffer.from(JSON.stringify(row) + '\n');
  writeAll(journalFd, bytes); journalBytes += bytes.length;
}

function refresh(context) {
  if (context.buffer !== context.memory.buffer) {
    context.buffer = context.memory.buffer;
    context.bytes = new Uint8Array(context.buffer);
  }
  return context;
}

function frame(context, event, side) {
  refresh(context);
  if (!Number.isInteger(context.base) || context.base <= 0 || context.base + SIZE > context.bytes.length) {
    throw new Error('live arena pointer is outside current engine memory');
  }
  const bytes = Buffer.from(context.bytes.slice(context.base, context.base + SIZE));
  const row = {
    index: frames.length, offset: rawBytes, length: SIZE, sha256: hash(bytes),
    context: context.spec.id, event, side, memory_bytes: context.bytes.length,
  };
  writeAll(rawFd, bytes); rawHash.update(bytes); rawBytes += bytes.length;
  frames.push(row); counts.frames++; appendJournal({kind: 'frame', ...row});
  return {index: row.index, bytes};
}

function references(context) {
  refresh(context);
  return {
    memory_same: context.memory === context.instance.exports.memory,
    table_slots: Array.from({length: 8}, (_, slot) => ({
      slot, unit: context.units[slot]?.unit ?? null,
      reference_equal: context.table.get(slot) === (context.units[slot]?.run ?? null),
    })),
    live_module_bytes: [...context.units.filter(Boolean), context.dispatcher].filter(Boolean).map(unit => {
      if (unit.pointer + unit.bytes_length > context.bytes.length) throw new Error('live module allocation is outside engine memory');
      return {
        file: unit.file, pointer: unit.pointer, bytes_length: unit.bytes_length,
        sha256: hash(context.bytes.subarray(unit.pointer, unit.pointer + unit.bytes_length)),
      };
    }),
  };
}

function checkpoint() {
  fsyncSync(rawFd); fsyncSync(journalFd);
  const row = {
    index: checkpointIndex++, process_index: processIndex, events: events.length,
    frames: frames.length, raw_bytes: rawBytes, journal_bytes: journalBytes,
    last_event: events.at(-1)?.action_id ?? null, observed_counts: {...counts},
  };
  const bytes = Buffer.from(JSON.stringify(row) + '\n');
  writeAll(checkpointFd, bytes); checkpointBytes += bytes.length; fsyncSync(checkpointFd);
}

function phase(context, event, name, ns, unit = null) {
  phases.push({
    process: processIndex, shape: context.spec.shape_id, context: context.spec.id,
    context_kind: context.spec.role, cold_ordinal: context.spec.ordinal ?? null,
    unit_ordinal: unit === null ? null : unit + 1, phase: name, ns, event,
  });
}

function resolveArgs(context, args) {
  return args.map(value => {
    if (value === 'key_low') return context.spec.key[0];
    if (value === 'key_high') return context.spec.key[1];
    if (typeof value === 'string') {
      const match = /^unit:(\d+):(low|high)$/.exec(value);
      if (!match || !context.units[Number(match[1])]) throw new Error(`unknown plan argument ${value}`);
      return context.units[Number(match[1])][match[2] === 'low' ? 'id_low' : 'id_high'];
    }
    return value;
  });
}

function receipt(context, action, bytes) {
  const size = action.name === 'compile_resident' ? 24 : 32;
  const packet = bytes.subarray(140, 140 + size);
  const words = Array.from({length: size / 4}, (_, index) => packet.readUInt32LE(index * 4));
  if (action.name === 'compile_resident') {
    context.units[action.unit] = {
      unit: action.unit, id_low: words[2], id_high: words[3], pointer: words[4], bytes_length: words[5],
    };
  } else if (action.name === 'dispatcher_module') {
    context.dispatcherMetadata = {pointer: words[6], bytes_length: words[7]};
  }
  return {hex: packet.toString('hex'), words};
}

function copyModule(context, action, event) {
  const metadata = action.target === 'child' ? context.units[action.unit] : context.dispatcherMetadata;
  refresh(context);
  if (!metadata || !Number.isInteger(metadata.pointer) || metadata.pointer <= 0
      || !Number.isInteger(metadata.bytes_length) || metadata.bytes_length < 8
      || metadata.pointer + metadata.bytes_length > context.bytes.length) {
    throw new Error('module receipt cannot identify a bounded live allocation');
  }
  let start = now();
  const bytes = context.bytes.slice(metadata.pointer, metadata.pointer + metadata.bytes_length);
  event.copy_ns = elapsed(start);
  writeFileSync(join(outputDir, action.file), bytes, {flag: 'wx'});
  event.module_file = action.file;
  event.module_copy_sha256 = hash(bytes);
  start = now(); const module = new WebAssembly.Module(bytes); event.module_ns = elapsed(start);
  const imports = action.target === 'child'
    ? {env: {memory: context.memory}, ring3: {guard_resident: context.api.guard_resident}}
    : {env: {memory: context.memory, table: context.table}, ring3: {
      guard_dispatch_entry: context.api.guard_dispatch_entry,
      find_installed_resident: context.api.find_installed_resident,
    }};
  start = now(); const instance = new WebAssembly.Instance(module, imports); event.instance_ns = elapsed(start);
  if (typeof instance.exports.run !== 'function') throw new Error('copied module has no callable run export');
  const row = {
    context: context.spec.id, target: action.target, unit: action.unit ?? null, file: action.file,
    pointer: metadata.pointer, bytes_length: metadata.bytes_length,
    id_low: metadata.id_low ?? null, id_high: metadata.id_high ?? null,
    slot: action.unit ?? null, sha256: hash(bytes),
    imports: WebAssembly.Module.imports(module), exports: WebAssembly.Module.exports(module),
  };
  modules.push(row);
  const live = {...metadata, file: action.file, module, instance, run: instance.exports.run};
  if (action.target === 'child') { context.units[action.unit] = live; counts.child_modules++; }
  else { context.dispatcher = live; counts.dispatcher_modules++; }
  for (const name of ['copy', 'module', 'instance']) {
    const label = action.target === 'child' ? name : name === 'module' ? 'dispatcher_v8_module' : `dispatcher_${name}`;
    phase(context, action.id, label, event[`${name}_ns`], action.unit ?? null);
  }
  appendJournal({kind: 'module_file', ...row});
}

function calls(context, action, event) {
  const state = context.base, exit = state + 56, cancel = state + 96;
  const low = context.spec.key[0], high = context.spec.key[1];
  const units = context.units.filter(Boolean), count = action.count, budget = action.budget;
  const run = action.channel === 'direct' ? context.units[action.unit].run : context.dispatcher.run;
  const guard = context.api.guard_resident, finder = context.api.find_installed_resident;
  const idsLow = units.map(unit => unit.id_low), idsHigh = units.map(unit => unit.id_high);
  const pcs = context.spec.banks.map(bank => bank.pc), unitCount = units.length;
  let statusOr = 0, index = 0;
  const start = now();
  try {
    switch (action.channel) {
      case 'dispatch': case 'direct':
        for (; index < count; index++) statusOr |= run(state, exit, budget, cancel);
        break;
      case 'guard':
        for (; index < count; index++) {
          const unit = index % unitCount;
          statusOr |= guard(low, high, idsLow[unit], idsHigh[unit], state, exit, cancel);
        }
        break;
      case 'finder':
        for (; index < count; index++) statusOr |= finder(low, high, pcs[index % unitCount]);
        break;
      case 'empty':
        for (; index < count; index++) statusOr |= 0;
        break;
      default: throw new Error(`unknown calls channel ${action.channel}`);
    }
  } finally {
    event.ns = elapsed(start); event.status_or = statusOr >>> 0;
    event.completed_calls = index;
    event.attempted_calls = Math.min(count, index + Number(index !== count));
    const counter = ['dispatch', 'direct'].includes(action.channel) ? 'generated_calls'
      : action.channel === 'guard' ? 'guard_calls' : action.channel === 'finder' ? 'finder_calls' : 'empty_iterations';
    counts[counter] += index;
  }
  phase(context, action.id, action.phase, event.ns, action.channel === 'direct' ? action.unit : null);
  if (action.sample !== undefined) {
    samples.push({
      process: processIndex, shape: context.spec.shape_id, context: context.spec.id,
      kind: action.channel, sample: action.sample, ns: event.ns, calls: count,
      expected_instructions_per_call: ['dispatch', 'direct'].includes(action.channel) ? BUDGET : 0,
      status_or: event.status_or,
    });
  }
}

function perform(context, action) {
  if (action.kind === 'calls' && action.timed) { globalThis.gc(); counts.explicit_gc++; }
  const event = {
    action_id: action.id, context: context.spec.id, kind: action.kind,
    before: null, after: null, ...Object.fromEntries(Object.entries(action).filter(([key]) => !['id', 'kind'].includes(key))),
  };
  const before = frame(context, action.id, 'before');
  event.before = before.index; event.memory_bytes_before = context.bytes.length;
  try {
    switch (action.kind) {
      case 'input': {
        const bytes = Buffer.from(action.hex, 'hex');
        if (action.offset < 0 || action.offset + bytes.length > SIZE) throw new Error('input exceeds the authored arena');
        const start = now(); context.bytes.set(bytes, context.base + action.offset); event.ns = elapsed(start);
        counts.input_events++; phase(context, action.id, 'input', event.ns); break;
      }
      case 'host': {
        const args = resolveArgs(context, action.args), fn = context.api[action.name];
        if (typeof fn !== 'function') throw new Error(`missing public host API ${action.name}`);
        event.args = args;
        const start = now(); event.status = fn(...args) >>> 0; event.ns = elapsed(start);
        counts.non_loop_host_api_calls++;
        phase(context, action.id, action.name === 'compile_resident' ? 'compile'
          : action.name === 'acknowledge_resident_installation' ? 'ack' : action.name, event.ns, action.unit ?? null);
        if (event.status !== 0) throw new Error(`host prerequisite ${action.name} returned ${event.status}`);
        break;
      }
      case 'module': copyModule(context, action, event); break;
      case 'table': {
        const unit = context.units[action.unit];
        const start = now(); context.table.set(action.unit, unit.run); event.ns = elapsed(start);
        event.reference_equal = context.table.get(action.unit) === unit.run;
        counts.table_events++; phase(context, action.id, 'table', event.ns, action.unit); break;
      }
      case 'calls': calls(context, action, event); break;
      case 'close': {
        event.reference_snapshot = references(context);
        const start = now(); event.status = context.api.close() >>> 0; event.ns = elapsed(start);
        counts.non_loop_host_api_calls++;
        context.closed = true; phase(context, action.id, 'close', event.ns);
        if (event.status !== 0) throw new Error(`close prerequisite returned ${event.status}`);
        counts.closed++; break;
      }
      default: throw new Error(`unknown action kind ${action.kind}`);
    }
  } catch (error) {
    event.error = {name: error.name, message: error.message, stack: error.stack};
  }
  try {
    if (!context.closed) {
      const after = frame(context, action.id, 'after');
      event.after = after.index; event.memory_bytes_after = context.bytes.length;
      if (action.kind === 'host' && ['compile_resident', 'dispatcher_module', 'acknowledge_resident_installation'].includes(action.name)
          && event.status === 0) event.receipt = receipt(context, action, after.bytes);
      if (action.kind === 'calls' && !event.error) event.reference_snapshot = references(context);
    }
  } catch (error) {
    event.error ??= {name: error.name, message: error.message, stack: error.stack};
  }
  events.push(event); counts.events++; appendJournal({kind: 'event', event});
  if (action.kind === 'calls' || action.kind === 'close' || event.error) checkpoint();
  if (event.error) throw new Error(event.error.message);
}

function openContext(spec) {
  globalThis.gc(); counts.explicit_gc++;
  const context = {spec, units: [], closed: false, buffer: null};
  current = context;
  const event = {
    action_id: `open:${spec.id}`, context: spec.id, kind: 'open',
    args: [spec.pages, ...spec.key], before: null, after: null,
  };
  let opened = false;
  try {
    const start = now(); context.instance = new WebAssembly.Instance(engineModule, {}); event.engine_instance_ns = elapsed(start);
    context.memory = context.instance.exports.memory;
    context.api = Object.fromEntries(Object.entries(context.instance.exports)
      .filter(([name]) => name.startsWith('ring3_abi_v1_')).map(([name, value]) => [name.slice(13), value]));
    const begin = now(); event.status = context.api.open(...event.args) >>> 0; event.ns = elapsed(begin);
    counts.non_loop_host_api_calls++;
    if (event.status !== 0) throw new Error(`open prerequisite returned ${event.status}`);
    opened = true; counts.opened++;
    context.base = context.api.arena_ptr() >>> 0; counts.non_loop_host_api_calls++;
    event.arena_ptr = context.base;
    const tableStart = now(); context.table = new WebAssembly.Table({element: 'anyfunc', initial: 8, maximum: 8}); event.table_create_ns = elapsed(tableStart);
    const after = frame(context, event.action_id, 'after'); event.after = after.index;
    event.memory_bytes_after = context.bytes.length;
    const row = {
      id: spec.id, shape_id: spec.shape_id, role: spec.role, ordinal: spec.ordinal ?? null,
      key: spec.key, base: context.base, memory_bytes: context.bytes.length,
      open_event: event.action_id, close_event: spec.actions.at(-1).id,
    };
    contexts.push(row); counts.engine_contexts++;
    phase(context, event.action_id, 'engine_instance', event.engine_instance_ns);
    phase(context, event.action_id, 'open', event.ns);
    phase(context, event.action_id, 'table_create', event.table_create_ns);
  } catch (error) {
    event.error = {name: error.name, message: error.message, stack: error.stack};
    context.closed = !opened;
  }
  events.push(event); counts.events++; appendJournal({kind: 'event', event});
  if (event.error) { checkpoint(); throw new Error(event.error.message); }
  return context;
}

function dropContext(context) {
  context.units.length = 0; context.dispatcher = null; context.table = null;
  context.api = null; context.instance = null; context.memory = null;
  context.bytes = null; context.buffer = null;
}

try {
  const pinsBefore = sourcePins();
  if (!pinsEqual(pinsBefore)) throw new Error('campaign source pins differ before execution');
  const readStart = now(); const engineBytes = readFileSync(enginePath); const readNs = elapsed(readStart);
  const actualHash = hash(engineBytes);
  if (actualHash !== expectedHash || engineBytes.length !== campaign.engine.bytes) throw new Error('engine binary differs from campaign authority');
  const moduleStart = now(); engineModule = new WebAssembly.Module(engineBytes); const moduleNs = elapsed(moduleStart);
  manifest = {
    schema_version: 1, process_index: processIndex, status: 'recording',
    plan: {file: 'plan.json', sha256: hash(planBytes)},
    engine: {bytes: engineBytes.length, sha256: actualHash},
    first_use: {engine_read_ns: readNs, engine_module_ns: moduleNs,
      imports: WebAssembly.Module.imports(engineModule), exports: WebAssembly.Module.exports(engineModule)},
    environment: environment(), source_pins_before: pinsBefore, bank_files: [...bankFiles.values()],
    planned_counts: plan.counts,
  };
  writeFileSync(join(outputDir, 'manifest.json'), JSON.stringify(manifest, null, 2) + '\n', {flag: 'wx'});
  for (const spec of selected.contexts) {
    const context = openContext(spec);
    for (const action of spec.actions) perform(context, action);
    dropContext(context); current = null;
  }
} catch (error) {
  failure = {name: error.name, message: error.message, stack: error.stack};
} finally {
  checkpoint();
  let sourcePinsAfter = [];
  try {
    sourcePinsAfter = sourcePins();
    if (!pinsEqual(sourcePinsAfter)) failure ??= {name: 'Error', message: 'campaign source pins differ after execution'};
  } catch (error) {
    failure ??= {name: error.name, message: error.message, stack: error.stack};
  }
  const result = {
    ...(manifest ?? {schema_version: 1, process_index: processIndex, plan: {file: 'plan.json', sha256: hash(planBytes)}}),
    status: failure ? 'failed' : 'complete', failure,
    source_pins_after: sourcePinsAfter, contexts, events, modules, frames, samples, phases,
    observed_counts: counts,
    raw: {file: 'arenas.bin', bytes: rawBytes, sha256: rawHash.digest('hex')},
    journal: {file: 'journal.ndjson', bytes: journalBytes, rows: frames.length + events.length + modules.length,
      sha256: hash(readFileSync(join(outputDir, 'journal.ndjson')))},
    checkpoints: {file: 'checkpoints.ndjson', bytes: checkpointBytes, rows: checkpointIndex,
      sha256: hash(readFileSync(join(outputDir, 'checkpoints.ndjson')))},
    memory_usage: process.memoryUsage(),
  };
  writeFileSync(join(outputDir, 'result.json'), JSON.stringify(result, null, 2) + '\n', {flag: 'wx'});
  for (const fd of [rawFd, journalFd, checkpointFd]) closeSync(fd);
  if (current) dropContext(current);
}
if (failure) { console.error(JSON.stringify(failure)); process.exitCode = 1; }
