import {createHash} from 'node:crypto';
import {
  closeSync, fsyncSync, mkdirSync, openSync, readFileSync, writeFileSync, writeSync,
} from 'node:fs';
import {loadavg} from 'node:os';
import {dirname, join, resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
import {SIZE, makePlan} from './plan.mjs';

const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const now = () => process.hrtime.bigint();
const elapsed = start => (now() - start).toString();
const root = resolve(dirname(fileURLToPath(import.meta.url)), '../../../..');
const [engineArgument, outputArgument, indexArgument] = process.argv.slice(2);
const expectedHash = process.env.RING3_ENGINE_SHA256;
const campaignPath = process.env.RING3_CHECKSUM_CAMPAIGN;
if (process.argv.length !== 5 || !engineArgument || !outputArgument
    || !/^[0-2]$/.test(indexArgument ?? '') || typeof globalThis.gc !== 'function'
    || !/^[a-f0-9]{64}$/.test(expectedHash ?? '') || !campaignPath) {
  throw new Error('usage: RING3_ENGINE_SHA256=<sha256> node --expose-gc run.mjs ENGINE_PATH OUTPUT_DIR PROCESS_INDEX');
}

const processIndex = Number(indexArgument), enginePath = resolve(engineArgument);
const outputDir = resolve(outputArgument);
const campaign = JSON.parse(readFileSync(campaignPath, 'utf8'));
const expectedPins = Object.keys(campaign.source_pins).sort()
  .map(path => ({path, ...campaign.source_pins[path]}));
if (!expectedPins.length || campaign.engine.sha256 !== expectedHash) {
  throw new Error('campaign authority is missing or inconsistent');
}
mkdirSync(outputDir);
const plan = makePlan(), selected = plan.processes[processIndex];
const planBytes = Buffer.from(JSON.stringify(plan, null, 2) + '\n');
writeFileSync(join(outputDir, 'plan.json'), planBytes, {flag: 'wx'});
const specs = new Map(selected.contexts.map(context => [context.id, context]));
const owners = new Map();
const contexts = [], events = [], frames = [], modules = [], dataFiles = [], samples = [];
const counts = {
  engine_contexts: 0, opened: 0, closed: 0, child_modules: 0, dispatcher_modules: 0,
  generated_calls: 0, guard_calls: 0, diagnostic_read32_calls: 0, host_api_calls: 0,
  input_events: 0, table_events: 0, data_events: 0, events: 0, frames: 0, explicit_gc: 0,
};
let rawBytes = 0, journalBytes = 0, checkpointBytes = 0, checkpointIndex = 0;
const rawHash = createHash('sha256');
const rawFd = openSync(join(outputDir, 'arenas.bin'), 'wx');
const journalFd = openSync(join(outputDir, 'journal.ndjson'), 'wx');
const checkpointFd = openSync(join(outputDir, 'checkpoints.ndjson'), 'wx');
let manifest = null, engineModule = null, failure = null;

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

function writeAll(fd, bytes) {
  let written = 0;
  while (written < bytes.length) written += writeSync(fd, bytes, written, bytes.length - written);
}

function journal(row) {
  const bytes = Buffer.from(JSON.stringify(row) + '\n');
  writeAll(journalFd, bytes); journalBytes += bytes.length;
}

function refresh(context) {
  if (context.buffer !== context.memory.buffer) {
    context.buffer = context.memory.buffer;
    context.bytes = new Uint8Array(context.buffer);
    context.view = new DataView(context.buffer);
  }
  return context;
}

function frame(context, action, side) {
  refresh(context);
  if (!Number.isInteger(context.base) || context.base <= 0 || context.base + SIZE > context.bytes.length) {
    throw new Error('live arena pointer exceeds current engine memory');
  }
  const bytes = Buffer.from(context.bytes.slice(context.base, context.base + SIZE));
  const row = {
    index: frames.length, offset: rawBytes, length: SIZE, sha256: hash(bytes),
    context: context.spec.id, event: action, side, memory_bytes: context.bytes.length,
  };
  writeAll(rawFd, bytes); rawHash.update(bytes); rawBytes += bytes.length;
  frames.push(row); counts.frames++; journal({kind: 'frame', ...row});
  return {index: row.index, bytes};
}

function references(context) {
  refresh(context);
  return {
    memory_same: context.memory === context.instance.exports.memory,
    table_slots: Array.from({length: 8}, (_, slot) => ({
      slot, unit: slot === 0 && context.bound ? 0 : null,
      generation: slot === 0 && context.bound ? context.bound.generation : null,
      reference_equal: context.table.get(slot) === (slot === 0 ? context.bound?.run ?? null : null),
    })),
    live_module_bytes: [context.child, context.dispatcher].filter(Boolean).map(module => {
      if (module.pointer + module.bytes_length > context.bytes.length) {
        throw new Error('live module allocation exceeds current engine memory');
      }
      return {
        file: module.file, pointer: module.pointer, bytes_length: module.bytes_length,
        generation: module.generation,
        sha256: hash(context.bytes.subarray(module.pointer, module.pointer + module.bytes_length)),
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

function resolvedArgs(context, args) {
  const unit = context.child ?? context.compiled;
  return args.map(value => {
    if (value === 'key_low') return context.spec.key[0];
    if (value === 'key_high') return context.spec.key[1];
    if (value === 'unit_low' || value === 'unit_high') {
      if (!unit) throw new Error('plan references a missing child identity');
      return unit[value === 'unit_low' ? 'id_low' : 'id_high'];
    }
    return value;
  });
}

function receipt(context, action, bytes) {
  const size = action.name === 'compile_resident' ? 24 : 32;
  const packet = bytes.subarray(140, 140 + size);
  const words = Array.from({length: size / 4}, (_, index) => packet.readUInt32LE(index * 4));
  if (action.name === 'compile_resident') {
    context.compiled = {id_low: words[2], id_high: words[3], pointer: words[4], bytes_length: words[5]};
  } else if (action.name === 'dispatcher_module') {
    context.dispatcherMetadata = {pointer: words[6], bytes_length: words[7]};
  }
  return {hex: packet.toString('hex'), words};
}

function copyModule(context, action, event) {
  const metadata = action.target === 'child' ? context.compiled : context.dispatcherMetadata;
  refresh(context);
  if (!metadata || !Number.isInteger(metadata.pointer) || metadata.pointer <= 0
      || !Number.isInteger(metadata.bytes_length) || metadata.bytes_length < 8
      || metadata.pointer + metadata.bytes_length > context.bytes.length) {
    throw new Error('module receipt does not identify a bounded live allocation');
  }
  const bytes = context.bytes.slice(metadata.pointer, metadata.pointer + metadata.bytes_length);
  writeFileSync(join(outputDir, action.file), bytes, {flag: 'wx'});
  event.module_file = action.file; event.module_copy_sha256 = hash(bytes);
  const module = new WebAssembly.Module(bytes);
  const imports = action.target === 'child'
    ? {env: {memory: context.memory}, ring3: {
      guard_resident: context.api.guard_resident, read32: context.api.read32,
    }}
    : {env: {memory: context.memory, table: context.table}, ring3: {
      guard_dispatch_entry: context.api.guard_dispatch_entry,
      find_installed_resident: context.api.find_installed_resident,
    }};
  const instance = new WebAssembly.Instance(module, imports);
  if (typeof instance.exports.run !== 'function') throw new Error('copied module has no callable run export');
  const row = {
    context: context.spec.id, target: action.target, generation: action.generation,
    unit: action.target === 'child' ? 0 : null, slot: action.target === 'child' ? 0 : null,
    file: action.file, pointer: metadata.pointer, bytes_length: metadata.bytes_length,
    id_low: metadata.id_low ?? null, id_high: metadata.id_high ?? null, sha256: hash(bytes),
    imports: WebAssembly.Module.imports(module), exports: WebAssembly.Module.exports(module),
  };
  modules.push(row); journal({kind: 'module_file', ...row});
  const live = {...metadata, file: action.file, generation: action.generation, module, instance, run: instance.exports.run};
  if (action.target === 'child') {context.child = live; counts.child_modules++;}
  else {context.dispatcher = live; counts.dispatcher_modules++;}
}

function generatedCall(context, action, event) {
  const run = action.channel === 'direct' ? context.child.run : context.dispatcher.run;
  const state = context.base, exit = state + 56, budget = action.budget, cancel = state + 96;
  event.count = 1;
  event.args = [state, exit, budget, cancel];
  event.completed_calls = 0; event.attempted_calls = 1;
  let status;
  const start = now();
  try {
    status = run(state, exit, budget, cancel);
  } finally {
    event.ns = elapsed(start);
  }
  event.status_or = status >>> 0;
  event.completed_calls = 1; counts.generated_calls++;
}

function guardBatch(context, action, event) {
  const guard = context.api.guard_resident;
  const [low, high] = context.spec.key, idLow = context.child.id_low, idHigh = context.child.id_high;
  const state = context.base, exit = state + 56, cancel = state + 96, count = action.count;
  event.channel = 'guard'; event.args = [low, high, idLow, idHigh, state, exit, cancel];
  let index = 0, statusOr = 0;
  const start = now();
  try {
    for (; index < count; index++) statusOr |= guard(low, high, idLow, idHigh, state, exit, cancel);
  } finally {
    event.ns = elapsed(start); event.status_or = statusOr >>> 0;
    event.completed_calls = index;
    event.attempted_calls = Math.min(count, index + Number(index !== count));
    counts.guard_calls += index;
  }
}

function dataPage(context, action, event) {
  const actual = Buffer.alloc(action.count * 4), diagnostics = [];
  event.diagnostics = diagnostics;
  for (let index = 0; index < action.count; index++) {
    const address = action.address + index * 4;
    const status = context.api.read32(address) >>> 0; counts.diagnostic_read32_calls++;
    refresh(context);
    const helperResult = context.view.getUint32(context.base + 116, true);
    const value = context.view.getUint32(context.base + 120, true);
    actual.writeUInt32LE(value, index * 4);
    diagnostics.push({address, status, helper_result: helperResult, value});
  }
  writeFileSync(join(outputDir, action.file), actual, {flag: 'wx'});
  const row = {
    context: context.spec.id, file: action.file, address: action.address,
    phase: action.phase, bytes: actual.length, sha256: hash(actual),
  };
  dataFiles.push(row); journal({kind: 'data_file', ...row});
  event.data_file = row.file; event.data_sha256 = row.sha256;
  counts.data_events++;
}

function openOwner(spec, event) {
  const context = {spec, buffer: null, child: null, dispatcher: null, bound: null, closed: true};
  owners.set(spec.id, context);
  context.instance = new WebAssembly.Instance(engineModule, {});
  context.memory = context.instance.exports.memory;
  context.api = Object.fromEntries(Object.entries(context.instance.exports)
    .filter(([name]) => name.startsWith('ring3_abi_v1_')).map(([name, value]) => [name.slice(13), value]));
  event.args = [spec.pages, ...spec.key];
  event.status = context.api.open(...event.args) >>> 0; counts.host_api_calls++;
  if (event.status !== 0) throw new Error(`open prerequisite returned ${event.status}`);
  context.closed = false; counts.opened++;
  context.base = context.api.arena_ptr() >>> 0; counts.host_api_calls++;
  context.table = new WebAssembly.Table({element: 'anyfunc', initial: 8, maximum: 8});
  event.arena_ptr = context.base;
  const after = frame(context, event.action_id, 'after'); event.after = after.index;
  event.memory_bytes_after = context.bytes.length;
  contexts.push({
    ...spec, base: context.base, memory_bytes: context.bytes.length,
    open_event: event.action_id,
    close_event: selected.actions.find(action => action.context === spec.id && action.kind === 'close').id,
  });
  counts.engine_contexts++;
  return context;
}

function dropOwner(context) {
  context.child = null; context.compiled = null; context.dispatcher = null;
  context.dispatcherMetadata = null; context.bound = null; context.table = null;
  context.api = null; context.instance = null; context.memory = null;
  context.bytes = null; context.view = null; context.buffer = null;
}

function perform(action) {
  let context = action.context === null ? null : owners.get(action.context);
  const event = {
    action_id: action.id, context: action.context, kind: action.kind, before: null, after: null,
    blocks: action.context === null ? null : specs.get(action.context).blocks,
    ...Object.fromEntries(Object.entries(action).filter(([key]) => !['id', 'context', 'kind'].includes(key))),
  };
  try {
    if (action.kind === 'open') {
      if (context) throw new Error('plan attempts to reuse an engine owner');
      context = openOwner(specs.get(action.context), event);
    } else if (action.kind === 'gc') {
      globalThis.gc(); counts.explicit_gc++;
    } else {
      if (!context || context.closed) throw new Error('plan references a missing or closed owner');
      const before = frame(context, action.id, 'before'); event.before = before.index;
      event.memory_bytes_before = context.bytes.length;
      if ((action.kind === 'table' && action.clear)
          || (action.kind === 'host' && action.name === 'retire_stale_resident')) {
        event.reference_snapshot_before = references(context);
      }
      switch (action.kind) {
        case 'input': {
          const bytes = Buffer.from(action.hex, 'hex');
          if (action.offset < 0 || action.offset + bytes.length > SIZE) throw new Error('input exceeds authored arena');
          context.bytes.set(bytes, context.base + action.offset); counts.input_events++; break;
        }
        case 'host': {
          const fn = context.api[action.name];
          if (typeof fn !== 'function') throw new Error(`missing public host API ${action.name}`);
          event.args = resolvedArgs(context, action.args);
          event.status = fn(...event.args) >>> 0; counts.host_api_calls++;
          if (event.status !== 0) throw new Error(`host prerequisite ${action.name} returned ${event.status}`);
          if (action.name === 'retire_stale_resident') {
            // Its backing module allocation expired. Never hash that pointer again.
            context.child = null; context.compiled = null;
          }
          break;
        }
        case 'module': copyModule(context, action, event); break;
        case 'table': {
          const unit = action.clear ? null : context.child;
          context.table.set(action.slot, unit?.run ?? null);
          event.reference_equal = context.table.get(action.slot) === (unit?.run ?? null);
          context.bound = unit; counts.table_events++; break;
        }
        case 'call': generatedCall(context, action, event); break;
        case 'guard': guardBatch(context, action, event); break;
        case 'data': dataPage(context, action, event); break;
        case 'close': {
          event.reference_snapshot = references(context);
          event.status = context.api.close() >>> 0; counts.host_api_calls++;
          context.closed = true;
          if (event.status !== 0) throw new Error(`close prerequisite returned ${event.status}`);
          counts.closed++; break;
        }
        default: throw new Error(`unknown plan action ${action.kind}`);
      }
      if (!context.closed) {
        const after = frame(context, action.id, 'after'); event.after = after.index;
        event.memory_bytes_after = context.bytes.length;
        if (action.kind === 'host'
            && ['compile_resident', 'dispatcher_module', 'acknowledge_resident_installation'].includes(action.name)) {
          event.receipt = receipt(context, action, after.bytes);
        }
        if (['call', 'guard', 'data', 'module', 'table'].includes(action.kind)
            || (action.kind === 'host' && action.name === 'retire_stale_resident')) {
          event.reference_snapshot = references(context);
        }
      }
    }
    if (['call', 'guard'].includes(action.kind) && action.timed) {
      samples.push({
        process: processIndex, blocks: context.spec.blocks, kind: event.channel, round: action.round,
        ns: event.ns, calls: event.count, status_or: event.status_or, event: action.id, context: action.context,
      });
    }
  } catch (error) {
    event.error = {name: error.name, message: error.message, stack: error.stack};
    if (context && !context.closed && event.after === null) {
      try {
        const after = frame(context, action.id, 'after'); event.after = after.index;
        event.memory_bytes_after = context.bytes.length;
      } catch (captureError) {
        event.capture_error = {name: captureError.name, message: captureError.message};
      }
    }
  }
  events.push(event); counts.events++; journal({kind: 'event', event});
  if (['call', 'guard', 'data', 'close'].includes(action.kind) || event.error) checkpoint();
  if (action.kind === 'close' && context) dropOwner(context);
  if (event.error) throw new Error(event.error.message);
}

try {
  const pinsBefore = sourcePins();
  if (!pinsEqual(pinsBefore)) throw new Error('source pins differ before execution');
  const readStart = now(); const engineBytes = readFileSync(enginePath); const readNs = elapsed(readStart);
  const actualHash = hash(engineBytes);
  if (actualHash !== expectedHash || engineBytes.length !== campaign.engine.bytes) throw new Error('engine binary differs from authority');
  const moduleStart = now(); engineModule = new WebAssembly.Module(engineBytes); const moduleNs = elapsed(moduleStart);
  manifest = {
    schema_version: 1, process_index: processIndex, status: 'recording',
    plan: {file: 'plan.json', sha256: hash(planBytes)}, plan_process: processIndex,
    engine: {bytes: engineBytes.length, sha256: actualHash},
    first_use: {engine_read_ns: readNs, engine_module_ns: moduleNs,
      imports: WebAssembly.Module.imports(engineModule), exports: WebAssembly.Module.exports(engineModule)},
    environment: {executable: process.execPath, exec_argv: process.execArgv, versions: process.versions,
      load_average: loadavg(), gc_policy: 'only explicit global per-round GC plan actions; outside timers'},
    source_pins_before: pinsBefore, planned_counts: plan.counts,
  };
  writeFileSync(join(outputDir, 'manifest.json'), JSON.stringify(manifest, null, 2) + '\n', {flag: 'wx'});
  for (const action of selected.actions) perform(action);
} catch (error) {
  failure = {name: error.name, message: error.message, stack: error.stack};
} finally {
  checkpoint();
  let sourcePinsAfter = [];
  try {
    sourcePinsAfter = sourcePins();
    if (!pinsEqual(sourcePinsAfter)) failure ??= {name: 'Error', message: 'source pins differ after execution'};
  } catch (error) {
    failure ??= {name: error.name, message: error.message, stack: error.stack};
  }
  const result = {
    ...(manifest ?? {schema_version: 1, process_index: processIndex,
      plan: {file: 'plan.json', sha256: hash(planBytes)}, plan_process: processIndex}),
    status: failure ? 'failed' : 'complete', failure, source_pins_after: sourcePinsAfter,
    contexts, events, modules, data_files: dataFiles, frames, samples, observed_counts: counts,
    raw: {file: 'arenas.bin', bytes: rawBytes, sha256: rawHash.digest('hex')},
    journal: {file: 'journal.ndjson', bytes: journalBytes,
      rows: frames.length + events.length + modules.length + dataFiles.length,
      sha256: hash(readFileSync(join(outputDir, 'journal.ndjson')))},
    checkpoints: {file: 'checkpoints.ndjson', bytes: checkpointBytes, rows: checkpointIndex,
      sha256: hash(readFileSync(join(outputDir, 'checkpoints.ndjson')))},
    memory_usage: process.memoryUsage(),
  };
  writeFileSync(join(outputDir, 'result.json'), JSON.stringify(result, null, 2) + '\n', {flag: 'wx'});
  for (const fd of [rawFd, journalFd, checkpointFd]) closeSync(fd);
  for (const context of owners.values()) dropOwner(context);
}
if (failure) {console.error(JSON.stringify(failure)); process.exitCode = 1;}
