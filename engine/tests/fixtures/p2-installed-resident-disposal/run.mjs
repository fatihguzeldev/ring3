import {createHash} from 'node:crypto';
import {closeSync, fsyncSync, mkdirSync, openSync, readFileSync, readdirSync, writeFileSync, writeSync} from 'node:fs';
import {join, resolve} from 'node:path';
import {isDeepStrictEqual} from 'node:util';
import {SIZE, makePlan} from './plan.mjs';

const [engineArgument, outputArgument, rootArgument, captureArgument] = process.argv.slice(2);
const expectedHash = process.env.RING3_ENGINE_SHA256, freezePath = process.env.RING3_INSTALLED_RESIDENT_DISPOSAL_FREEZE;
if (process.argv.length !== 6 || !engineArgument || !outputArgument || !rootArgument || !captureArgument
    || !/^[a-f0-9]{64}$/.test(expectedHash ?? '') || !freezePath) throw new Error('usage: pinned node run.mjs ENGINE OUTPUT ROOT RAW_CAPTURE');
const root = resolve(rootArgument), output = resolve(outputArgument), capture = resolve(captureArgument);
const freeze = JSON.parse(readFileSync(freezePath)), equal = isDeepStrictEqual;
if (capture !== freeze.capture_path) throw new Error('capture argv differs from frozen absolute authority');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const sourcePins = () => Object.keys(freeze.source_pins).sort().map(path => {
  const bytes = readFileSync(join(root, path)); return {path, bytes: bytes.length, sha256: hash(bytes)};
});
const expectedSources = Object.keys(freeze.source_pins).sort().map(path => ({path, ...freeze.source_pins[path]}));
const plan = makePlan(), specs = new Map(plan.contexts.map(c => [c.id, c]));
const planBytes = Buffer.from(JSON.stringify(plan, null, 2) + '\n');
if (!equal({bytes: planBytes.length, sha256: hash(planBytes)}, freeze.plan)) throw new Error('frozen physical plan differs');
const captureNames = ['bank.x86', 'initial-arena.bin', 'authored-arena.bin', 'capture.json'].sort();
if (!equal(readdirSync(capture).sort(), captureNames)) throw new Error('native input physical file set differs');
const capturePins = captureNames.map(file => {
  const bytes = readFileSync(join(capture, file)); return {file, bytes: bytes.length, sha256: hash(bytes)};
});
if (!equal(capturePins, Object.keys(freeze.capture_pins).sort().map(file => ({file, ...freeze.capture_pins[file]})))) throw new Error('native input pins differ');
for (const [file, hex] of [['bank.x86', plan.bank.hex], ['initial-arena.bin', plan.initial_hex], ['authored-arena.bin', plan.authored_hex]]) {
  if (readFileSync(join(capture, file)).toString('hex') !== hex) throw new Error(`independent native ${file} differs`);
}
const engineBytes = readFileSync(resolve(engineArgument)), engine = {bytes: engineBytes.length, sha256: hash(engineBytes)};
if (engine.sha256 !== expectedHash || !equal(engine, freeze.engine)) throw new Error('integrated engine pin differs');
const beforeSources = sourcePins(); if (!equal(beforeSources, expectedSources)) throw new Error('frozen sources differ before recording');
const engineModule = new WebAssembly.Module(engineBytes);
mkdirSync(output); writeFileSync(join(output, 'plan.json'), planBytes, {flag: 'wx'});
for (const file of captureNames) writeFileSync(join(output, file), readFileSync(join(capture, file)), {flag: 'wx'});
const rawFd = openSync(join(output, 'arenas.bin'), 'wx'), journalFd = openSync(join(output, 'journal.ndjson'), 'wx');
const checkpointFd = openSync(join(output, 'checkpoints.ndjson'), 'wx');
const frames = [], events = [], modules = [], contexts = [], owners = new Map();
const counts = {contexts: 0, engine_instances: 0, opened: 0, closed: 0, generated_calls: 0, host_api_calls: 0,
  inputs: 0, tables: 0, child_modules: 0, dispatchers: 0, successful_discards: 0, events: 0, frames: 0};
const statuses = {}; let rawBytes = 0, journalBytes = 0, checkpointBytes = 0, checkpointIndex = 0, failure = null;
const rawHash = createHash('sha256');
function writeAll(fd, bytes) {let at = 0; while (at < bytes.length) at += writeSync(fd, bytes, at, bytes.length - at);}
function journal(row) {const bytes = Buffer.from(JSON.stringify(row) + '\n'); writeAll(journalFd, bytes); journalBytes += bytes.length;}
function refresh(c) {if (c.buffer !== c.memory.buffer) {c.buffer = c.memory.buffer; c.bytes = new Uint8Array(c.buffer); c.view = new DataView(c.buffer);} return c;}
function frame(c, action, side) {
  refresh(c); if (c.closed || c.base <= 0 || c.base + SIZE > c.bytes.length) throw new Error('frame outside live arena');
  const bytes = Buffer.from(c.bytes.slice(c.base, c.base + SIZE));
  const row = {index: frames.length, offset: rawBytes, length: SIZE, sha256: hash(bytes), context: c.spec.id, event: action, side, memory_bytes: c.bytes.length};
  writeAll(rawFd, bytes); rawHash.update(bytes); rawBytes += bytes.length; frames.push(row); counts.frames++; journal({kind: 'frame', ...row}); return row.index;
}
function references(c) {
  refresh(c);
  return {memory_same: c.memory === c.instance.exports.memory,
    table_slots: Array.from({length: 8}, (_, slot) => ({slot, binding: c.bound[slot]?.binding ?? null, group: c.bound[slot]?.group ?? null,
      reference_equal: c.table.get(slot) === (c.bound[slot]?.run ?? null)})),
    acknowledgements: c.acknowledgements.map((entry, slot) => ({slot, ...entry})),
    cached_functions: [...c.cached.values()].map(unit => ({binding: unit.binding, group: unit.group, id_low: unit.id_low, id_high: unit.id_high,
      reference_equal: unit.run === unit.instance.exports.run})),
    live_module_bytes: [...c.live.values()].map(unit => {
      if (unit.pointer <= 0 || unit.pointer + unit.bytes_length > c.bytes.length) throw new Error('live module outside memory');
      return {file: unit.file, binding: unit.binding, pointer: unit.pointer, bytes_length: unit.bytes_length,
        sha256: hash(c.bytes.subarray(unit.pointer, unit.pointer + unit.bytes_length))};
    })};
}
function checkpoint() {
  fsyncSync(rawFd); fsyncSync(journalFd);
  const bytes = Buffer.from(JSON.stringify({index: checkpointIndex++, events: events.length, frames: frames.length,
    raw_bytes: rawBytes, journal_bytes: journalBytes, last_event: events.at(-1)?.action_id ?? null, observed_counts: {...counts}}) + '\n');
  writeAll(checkpointFd, bytes); checkpointBytes += bytes.length; fsyncSync(checkpointFd);
}
function resolveArgs(c, args, binding) {
  const unit = c.cached.get(binding);
  const value = ref => ref === 'key_low' ? c.spec.key[0] : ref === 'key_high' ? c.spec.key[1]
    : ref === 'unit_low' ? unit?.id_low : ref === 'unit_high' ? unit?.id_high : undefined;
  return args.map(arg => {
    const raw = typeof arg === 'number' ? arg : value(typeof arg === 'string' ? arg : arg.ref);
    if (!Number.isInteger(raw) || raw < 0 || raw > 0xffffffff) throw new Error('unresolved numeric API argument');
    return typeof arg === 'object' ? (raw ^ arg.xor) >>> 0 : raw;
  });
}
function receipt(c, length) {
  refresh(c); const bytes = Buffer.from(c.bytes.slice(c.base + 140, c.base + 140 + length));
  return {hex: bytes.toString('hex'), words: Array.from({length: length / 4}, (_, i) => bytes.readUInt32LE(i * 4))};
}
function openOwner(spec, event) {
  const c = {spec, closed: false, buffer: null, cached: new Map(), live: new Map(), bound: Array(8).fill(null),
    acknowledgements: Array.from({length: 8}, () => ({binding: null, id_low: null, id_high: null})), pending: null};
  c.instance = new WebAssembly.Instance(engineModule, {}); c.memory = c.instance.exports.memory;
  c.api = Object.fromEntries(Object.entries(c.instance.exports).filter(([name]) => name.startsWith('ring3_abi_v1_')).map(([name, value]) => [name.slice(13), value]));
  event.args = [spec.pages, ...spec.key]; event.status = c.api.open(...event.args) >>> 0; counts.host_api_calls++;
  if (event.status !== 0) throw new Error('open failed');
  c.base = c.api.arena_ptr() >>> 0; counts.host_api_calls++; counts.engine_instances++;
  c.table = new WebAssembly.Table({element: 'anyfunc', initial: 8, maximum: 8}); owners.set(spec.id, c);
  event.arena_ptr = c.base; event.after = frame(c, event.action_id, 'after'); event.memory_bytes_after = c.bytes.length;
  contexts.push({...spec, base: c.base, memory_bytes: c.bytes.length, open_event: event.action_id}); counts.contexts++; counts.opened++; return c;
}
function host(c, action, event, closed = false) {
  event.args = resolveArgs(c, action.args, action.binding);
  if (action.args.some(arg => typeof arg === 'string' && arg.startsWith('unit_') || typeof arg === 'object' && arg.ref.startsWith('unit_'))) {
    event.resolved_unit = {low: event.args[2], high: event.args[3], decimal: ((BigInt(event.args[3]) << 32n) | BigInt(event.args[2])).toString()};
  }
  event.status = c.api[action.name](...event.args) >>> 0; counts.host_api_calls++;
  if (closed) return;
  if (action.name.startsWith('compile_resident')) {
    c.pending = null;
    if (event.status === 0) {
      event.receipt = receipt(c, 24); const words = event.receipt.words;
      c.pending = {id_low: words[2], id_high: words[3], pointer: words[4], bytes_length: words[5], binding: action.binding, group: action.group};
    }
  } else if (action.name === 'dispatcher_module' && event.status === 0) {
    event.receipt = receipt(c, 32); const words = event.receipt.words;
    c.dispatchMetadata = {pointer: words[6], bytes_length: words[7], id_low: null, id_high: null};
  } else if (['acknowledge_resident_installation', 'find_installed_resident'].includes(action.name) && event.status === 0) {
    event.receipt = receipt(c, 32);
    if (action.name === 'acknowledge_resident_installation') {
      const words = event.receipt.words;
      c.acknowledgements[words[6]] = {binding: action.binding, id_low: words[4], id_high: words[5]};
    }
  } else if (action.name === 'discard_installed_resident' && event.status === 0) {
    for (const [binding, unit] of c.live) if (unit.id_low === event.args[2] && unit.id_high === event.args[3]) c.live.delete(binding);
    c.acknowledgements[event.args[4]] = {binding: null, id_low: null, id_high: null}; counts.successful_discards++;
  }
  if (event.status !== 0 && !action.allowed_failure) throw new Error(`host prerequisite ${action.name} failed: ${event.status}`);
}
function moduleCopy(c, action, event) {
  refresh(c); const metadata = action.target === 'dispatcher' ? c.dispatchMetadata : c.pending;
  if (!metadata || action.target === 'child' && metadata.binding !== action.binding) throw new Error('missing exact current module receipt');
  if (metadata.pointer <= 0 || metadata.pointer + metadata.bytes_length > c.bytes.length) throw new Error('copied module outside memory');
  const bytes = Buffer.from(c.bytes.slice(metadata.pointer, metadata.pointer + metadata.bytes_length));
  writeFileSync(join(output, action.file), bytes, {flag: 'wx'});
  const module = new WebAssembly.Module(bytes), instance = new WebAssembly.Instance(module, {env: {memory: c.memory, table: c.table}, ring3: c.api});
  const row = {context: c.spec.id, owner: 'resident', entries: c.spec.entries, target: action.target, group: action.group, binding: action.binding,
    slot: action.slot, file: action.file, pointer: metadata.pointer, bytes_length: bytes.length, generation: null,
    id_low: metadata.id_low, id_high: metadata.id_high, sha256: hash(bytes), imports: WebAssembly.Module.imports(module), exports: WebAssembly.Module.exports(module)};
  modules.push(row); journal({kind: 'module_file', ...row}); event.module_file = row.file; event.module_copy_sha256 = row.sha256;
  const unit = {...row, module, instance, run: instance.exports.run}; c.live.set(action.binding, unit);
  if (action.target === 'dispatcher') {c.dispatcher = unit; counts.dispatchers++;}
  else {if (c.cached.has(action.binding)) throw new Error('cached binding reused'); c.cached.set(action.binding, unit); counts.child_modules++;}
}
function call(c, action, event, closed = false) {
  const unit = action.channel === 'dispatch' ? c.dispatcher : c.cached.get(action.binding);
  if (!unit) throw new Error('missing cached generated function');
  event.args = [c.base, c.base + 56, action.budget, c.base + 96]; event.status = unit.run(...event.args) >>> 0;
  event.completed_calls = 1; counts.generated_calls++;
  const name = `${closed ? 'closed' : action.channel}:${event.status}`; statuses[name] = (statuses[name] ?? 0) + 1;
}
const manifest = {schema_version: 1, status: 'recording', engine, plan: {file: 'plan.json', sha256: hash(planBytes)},
  source_pins_before: beforeSources, capture_pins: capturePins, planned_counts: plan.counts,
  engine_imports: WebAssembly.Module.imports(engineModule), engine_exports: WebAssembly.Module.exports(engineModule),
  environment: {executable: process.execPath, exec_argv: process.execArgv, versions: process.versions}};
writeFileSync(join(output, 'manifest.json'), JSON.stringify(manifest, null, 2) + '\n', {flag: 'wx'});
function perform(action) {
  let c = owners.get(action.context);
  const event = {action_id: action.id, context: action.context, kind: action.kind, before: null, after: null,
    ...Object.fromEntries(Object.entries(action).filter(([key]) => !['id', 'context', 'kind'].includes(key)))};
  try {
    if (action.kind === 'open') c = openOwner(specs.get(action.context), event);
    else if (['closed_call', 'closed_host'].includes(action.kind)) {
      if (!c?.closed) throw new Error('closed control requires prior close');
      if (action.kind === 'closed_call') call(c, action, event, true); else host(c, action, event, true);
    } else {
      if (!c || c.closed) throw new Error('action references missing or closed owner');
      event.before = frame(c, action.id, 'before'); event.memory_bytes_before = c.bytes.length;
      if (action.kind === 'host') event.reference_snapshot_before = references(c);
      if (action.kind === 'input') {
        const bytes = Buffer.from(action.hex, 'hex'); if (action.offset < 0 || action.offset + bytes.length > SIZE) throw new Error('input exceeds arena');
        c.bytes.set(bytes, c.base + action.offset); counts.inputs++;
      } else if (action.kind === 'host') host(c, action, event);
      else if (action.kind === 'module') moduleCopy(c, action, event);
      else if (action.kind === 'table') {
        const unit = action.binding === null ? null : c.cached.get(action.binding);
        if (action.binding !== null && !unit) throw new Error('missing table function');
        c.table.set(action.slot, unit?.run ?? null); c.bound[action.slot] = unit; counts.tables++;
        event.reference_equal = c.table.get(action.slot) === (unit?.run ?? null);
      } else if (action.kind === 'call') call(c, action, event);
      else if (action.kind === 'close') {
        event.reference_snapshot = references(c); event.status = c.api.close() >>> 0; counts.host_api_calls++;
        c.closed = true; counts.closed++;
      } else throw new Error(`unknown action ${action.kind}`);
      if (!c.closed) {event.after = frame(c, action.id, 'after'); event.memory_bytes_after = c.bytes.length; event.reference_snapshot = references(c);}
    }
  } catch (error) {
    event.error = {name: error.name, message: error.message, stack: error.stack};
    if (c && !c.closed && event.after === null) {
      try {event.after = frame(c, action.id, 'after'); event.memory_bytes_after = c.bytes.length;}
      catch (captureError) {event.capture_error = {name: captureError.name, message: captureError.message};}
    }
  }
  events.push(event); counts.events++; journal({kind: 'event', event});
  if (['call', 'closed_call', 'closed_host', 'close'].includes(action.kind) || action.kind === 'host' && action.name === 'discard_installed_resident' || event.error) checkpoint();
  if (event.error) throw new Error(event.error.message);
}
try {for (const action of plan.actions) perform(action);}
catch (error) {failure = {name: error.name, message: error.message, stack: error.stack};}
finally {
  checkpoint(); let afterSources = [];
  try {afterSources = sourcePins(); if (!equal(afterSources, expectedSources)) failure ??= {name: 'Error', message: 'sources changed during recording'};}
  catch (error) {failure ??= {name: error.name, message: error.message};}
  const result = {...manifest, status: failure ? 'failed' : 'complete', failure, source_pins_after: afterSources,
    contexts, events, modules, frames, observed_counts: counts, observed_statuses: statuses,
    raw: {file: 'arenas.bin', bytes: rawBytes, sha256: rawHash.digest('hex')},
    journal: {file: 'journal.ndjson', bytes: journalBytes, rows: frames.length + events.length + modules.length, sha256: hash(readFileSync(join(output, 'journal.ndjson')))},
    checkpoints: {file: 'checkpoints.ndjson', bytes: checkpointBytes, rows: checkpointIndex, sha256: hash(readFileSync(join(output, 'checkpoints.ndjson')))}};
  writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2) + '\n', {flag: 'wx'});
  for (const fd of [rawFd, journalFd, checkpointFd]) closeSync(fd);
}
if (failure) {console.error(JSON.stringify(failure)); process.exitCode = 1;}
else console.log(JSON.stringify({status: 'complete', counts, statuses, engine}));
