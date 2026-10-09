import {createHash} from 'node:crypto';
import {closeSync, fsyncSync, mkdirSync, openSync, readFileSync, readdirSync, writeFileSync, writeSync} from 'node:fs';
import {join, resolve} from 'node:path';
import {isDeepStrictEqual} from 'node:util';
import {SIZE, makePlan} from './plan.mjs';

const [engineArgument, outputArgument, rootArgument, captureArgument] = process.argv.slice(2);
const expectedHash = process.env.RING3_ENGINE_SHA256, freezePath = process.env.RING3_WORD_CARRY_FREEZE;
if (process.argv.length !== 6 || !engineArgument || !outputArgument || !rootArgument || !captureArgument
    || !/^[a-f0-9]{64}$/.test(expectedHash ?? '') || !freezePath) throw new Error('usage: pinned node run.mjs ENGINE OUTPUT ROOT STANDALONE_CAPTURE');
const root = resolve(rootArgument), output = resolve(outputArgument), capture = resolve(captureArgument);
const freeze = JSON.parse(readFileSync(freezePath));
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const sourcePins = () => Object.keys(freeze.source_pins).sort().map(path => {
  const b = readFileSync(join(root, path)); return {path, bytes: b.length, sha256: hash(b)};
});
const expectedSources = Object.keys(freeze.source_pins).sort().map(path => ({path, ...freeze.source_pins[path]}));
const equal = isDeepStrictEqual;
const plan = makePlan(), specifications = new Map(plan.contexts.map(c => [c.id, c]));
const planBytes = Buffer.from(JSON.stringify(plan, null, 2) + '\n');
if (!equal({bytes: planBytes.length, sha256: hash(planBytes)}, freeze.plan)) throw new Error('frozen physical plan differs');
const groups = new Map(plan.bank.groups.map(g => [g.id, g]));
const captureNames = ['bank.x86', 'initial-arena.bin', 'capture.json', ...plan.bank.groups.map(g => `${g.id}.wasm`)].sort();
if (!equal(readdirSync(capture).sort(), captureNames)) throw new Error('native capture physical file set differs');
const capturePins = captureNames.map(file => {
  const b = readFileSync(join(capture, file)); return {file, bytes: b.length, sha256: hash(b)};
});
const expectedCaptures = Object.keys(freeze.capture_pins).sort().map(file => ({file, ...freeze.capture_pins[file]}));
if (!equal(capturePins, expectedCaptures)) throw new Error('native capture pins differ');
if (readFileSync(join(capture, 'bank.x86')).toString('hex') !== plan.bank.hex) throw new Error('independent native bank differs');
const engineBytes = readFileSync(resolve(engineArgument)), engine = {bytes: engineBytes.length, sha256: hash(engineBytes)};
if (engine.sha256 !== expectedHash || !equal(engine, freeze.engine)) throw new Error('integrated engine pin differs');
const beforeSources = sourcePins();
if (!equal(beforeSources, expectedSources)) throw new Error('frozen sources differ before recording');
const engineModule = new WebAssembly.Module(engineBytes);
mkdirSync(output);
writeFileSync(join(output, 'plan.json'), planBytes, {flag: 'wx'});
for (const file of captureNames) writeFileSync(join(output, file), readFileSync(join(capture, file)), {flag: 'wx'});
const rawFd = openSync(join(output, 'arenas.bin'), 'wx'), journalFd = openSync(join(output, 'journal.ndjson'), 'wx');
const checkpointFd = openSync(join(output, 'checkpoints.ndjson'), 'wx');
const frames = [], events = [], modules = [], contexts = [], owners = new Map();
const counts = {contexts: 0, engine_instances: 0, opened: 0, closed: 0, generated_calls: 0, guard_calls: 0,
  host_api_calls: 0, inputs: 0, tables: 0, child_modules: 0, dispatchers: 0, events: 0, frames: 0};
const statuses = {}; let rawBytes = 0, journalBytes = 0, checkpointBytes = 0, checkpointIndex = 0, failure = null;
const rawHash = createHash('sha256');
function writeAll(fd, bytes) {let at = 0; while (at < bytes.length) at += writeSync(fd, bytes, at, bytes.length - at);}
function journal(row) {const b = Buffer.from(JSON.stringify(row) + '\n'); writeAll(journalFd, b); journalBytes += b.length;}
function refresh(c) {if (c.buffer !== c.memory.buffer) {c.buffer = c.memory.buffer; c.bytes = new Uint8Array(c.buffer); c.view = new DataView(c.buffer);} return c;}
function frame(c, action, side) {
  refresh(c); if (c.closed || c.base <= 0 || c.base + SIZE > c.bytes.length) throw new Error('frame outside live arena');
  const b = Buffer.from(c.bytes.slice(c.base, c.base + SIZE));
  const row = {index: frames.length, offset: rawBytes, length: SIZE, sha256: hash(b), context: c.spec.id, event: action, side, memory_bytes: c.bytes.length};
  writeAll(rawFd, b); rawHash.update(b); rawBytes += b.length; frames.push(row); counts.frames++; journal({kind: 'frame', ...row});
  return {index: row.index, bytes: b};
}
function referenceSnapshot(c) {
  refresh(c);
  return {memory_same: c.spec.owner === 'standalone' ? true : c.memory === c.instance.exports.memory,
    table_slots: c.table ? Array.from({length: 8}, (_, slot) => ({slot, group: c.bound[slot]?.group ?? null,
      reference_equal: c.table.get(slot) === (c.bound[slot]?.run ?? null)})) : null,
    live_module_bytes: [...c.live.values()].map(m => {
      if (m.pointer === null) return {file: m.file, pointer: null, bytes_length: m.bytes_length, sha256: m.sha256};
      if (m.pointer <= 0 || m.pointer + m.bytes_length > c.bytes.length) throw new Error('current live module outside memory');
      return {file: m.file, pointer: m.pointer, bytes_length: m.bytes_length,
        sha256: hash(c.bytes.subarray(m.pointer, m.pointer + m.bytes_length))};
    })};
}
function checkpoint() {
  fsyncSync(rawFd); fsyncSync(journalFd);
  const b = Buffer.from(JSON.stringify({index: checkpointIndex++, events: events.length, frames: frames.length,
    raw_bytes: rawBytes, journal_bytes: journalBytes, last_event: events.at(-1)?.action_id ?? null, observed_counts: {...counts}}) + '\n');
  writeAll(checkpointFd, b); checkpointBytes += b.length; fsyncSync(checkpointFd);
}
function resolved(c, args, group) {
  const unit = c.children.get(group) ?? c.pending;
  return args.map(n => n === 'key_low' ? c.spec.key[0] : n === 'key_high' ? c.spec.key[1]
    : n === 'unit_low' ? unit.id_low : n === 'unit_high' ? unit.id_high : n);
}
function publication(c) {
  const row = {generation: c.api.generation() >>> 0, pointer: c.api.module_ptr() >>> 0, bytes_length: c.api.module_len() >>> 0};
  counts.host_api_calls += 3; return row;
}
function openOwner(spec, event) {
  const c = {spec, closed: false, buffer: null, children: new Map(), live: new Map(), bound: Array(8).fill(null), pending: null};
  if (spec.owner === 'standalone') {
    c.memory = new WebAssembly.Memory({initial: 1}); c.base = 128; refresh(c);
    c.bytes.set(readFileSync(join(output, 'initial-arena.bin')), c.base); event.args = []; event.status = 0;
  } else {
    c.instance = new WebAssembly.Instance(engineModule, {}); c.memory = c.instance.exports.memory;
    c.api = Object.fromEntries(Object.entries(c.instance.exports).filter(([name]) => name.startsWith('ring3_abi_v1_')).map(([name, value]) => [name.slice(13), value]));
    event.args = [spec.pages, ...spec.key]; event.status = c.api.open(...event.args) >>> 0; counts.host_api_calls++;
    if (event.status !== 0) throw new Error('open failed');
    c.base = c.api.arena_ptr() >>> 0; counts.host_api_calls++; counts.engine_instances++;
    if (spec.owner === 'resident') c.table = new WebAssembly.Table({element: 'anyfunc', initial: 8, maximum: 8});
  }
  owners.set(spec.id, c); event.arena_ptr = c.base;
  event.after = frame(c, event.action_id, 'after').index; event.memory_bytes_after = c.bytes.length;
  contexts.push({...spec, base: c.base, memory_bytes: c.bytes.length, open_event: event.action_id}); counts.contexts++; counts.opened++;
  return c;
}
function compileHost(c, action, event) {
  event.args = resolved(c, action.args, action.group); event.status = c.api[action.name](...event.args) >>> 0; counts.host_api_calls++;
  if (action.name.startsWith('compile')) {
    if (c.spec.owner === 'replacement') {
      event.publication = publication(c);
      if (event.status === 0) {
        c.children.clear(); c.live.clear(); c.pending = {...event.publication, id_low: null, id_high: null};
      }
    } else if (event.status === 0) {
      refresh(c); const words = Array.from({length: 6}, (_, i) => c.view.getUint32(c.base + 140 + i * 4, true));
      c.pending = {id_low: words[2], id_high: words[3], pointer: words[4], bytes_length: words[5], generation: null};
      event.receipt = {hex: Buffer.from(c.bytes.slice(c.base + 140, c.base + 164)).toString('hex'), words};
    }
  } else if (action.name === 'dispatcher_module' && event.status === 0) {
    refresh(c); const words = Array.from({length: 8}, (_, i) => c.view.getUint32(c.base + 140 + i * 4, true));
    c.dispatchMetadata = {pointer: words[6], bytes_length: words[7], generation: null, id_low: null, id_high: null};
    event.receipt = {hex: Buffer.from(c.bytes.slice(c.base + 140, c.base + 172)).toString('hex'), words};
  } else if (action.name === 'acknowledge_resident_installation' && event.status === 0) {
    refresh(c); const bytes = Buffer.from(c.bytes.slice(c.base + 140, c.base + 172));
    event.receipt = {hex: bytes.toString('hex'), words: Array.from({length: 8}, (_, i) => bytes.readUInt32LE(i * 4))};
  }
  if (event.status !== 0 && !action.allowed_failure) throw new Error(`host prerequisite ${action.name} failed: ${event.status}`);
}
function moduleCopy(c, action, event) {
  refresh(c);
  const metadata = action.capture ? {pointer: null, generation: null, id_low: null, id_high: null}
    : action.target === 'dispatcher' ? c.dispatchMetadata : c.pending;
  if (!metadata) throw new Error('missing current module receipt');
  let bytes;
  if (action.capture) bytes = readFileSync(join(output, action.capture));
  else {
    if (metadata.pointer <= 0 || metadata.pointer + metadata.bytes_length > c.bytes.length) throw new Error('copied module outside current memory');
    bytes = Buffer.from(c.bytes.slice(metadata.pointer, metadata.pointer + metadata.bytes_length));
  }
  writeFileSync(join(output, action.file), bytes, {flag: 'wx'});
  const module = new WebAssembly.Module(bytes);
  const instance = new WebAssembly.Instance(module, {env: {memory: c.memory, ...(c.table ? {table: c.table} : {})}, ring3: c.api ?? {}});
  const row = {context: c.spec.id, owner: c.spec.owner, entries: c.spec.entries, target: action.target, group: action.group, slot: action.slot,
    file: action.file, capture: action.capture, pointer: metadata.pointer, bytes_length: bytes.length, generation: metadata.generation,
    id_low: metadata.id_low, id_high: metadata.id_high, sha256: hash(bytes), imports: WebAssembly.Module.imports(module), exports: WebAssembly.Module.exports(module)};
  modules.push(row); journal({kind: 'module_file', ...row}); event.module_file = row.file; event.module_copy_sha256 = row.sha256;
  const live = {...row, module, instance, run: instance.exports.run}; c.live.set(action.group, live);
  if (action.target === 'dispatcher') {c.dispatcher = live; counts.dispatchers++;}
  else {c.children.set(action.group, live); counts.child_modules++;}
}
function call(c, action, event, closed = false) {
  const run = action.channel === 'dispatch' ? c.dispatcher.run : c.children.get(action.group).run;
  event.args = [c.base, c.base + 56, action.budget, c.base + 96];
  event.status = run(...event.args) >>> 0; event.completed_calls = 1; counts.generated_calls++;
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
    ...Object.fromEntries(Object.entries(action).filter(([k]) => !['id', 'context', 'kind'].includes(k)))};
  try {
    if (action.kind === 'open') c = openOwner(specifications.get(action.context), event);
    else if (action.kind === 'closed_call') {
      if (!c.closed) throw new Error('closed control requires prior close'); call(c, action, event, true);
    } else {
      if (!c || c.closed) throw new Error('action references missing or closed owner');
      event.before = frame(c, action.id, 'before').index; event.memory_bytes_before = c.bytes.length;
      if (action.kind === 'host' && action.name.startsWith('compile')) event.reference_snapshot_before = referenceSnapshot(c);
      if (action.kind === 'input') {
        const b = Buffer.from(action.hex, 'hex'); if (action.offset < 0 || action.offset + b.length > SIZE) throw new Error('input exceeds arena');
        c.bytes.set(b, c.base + action.offset); counts.inputs++;
      } else if (action.kind === 'host') compileHost(c, action, event);
      else if (action.kind === 'module') moduleCopy(c, action, event);
      else if (action.kind === 'table') {
        const unit = c.children.get(action.group); c.table.set(action.slot, unit.run); c.bound[action.slot] = unit;
        event.reference_equal = c.table.get(action.slot) === unit.run; counts.tables++;
      } else if (action.kind === 'call') call(c, action, event);
      else if (action.kind === 'guard') {
        const unit = c.children.get(action.group), key = [c.spec.key[0] ^ 1, c.spec.key[1]];
        event.name = c.spec.owner === 'resident' ? 'guard_resident' : 'guard';
        event.args = c.spec.owner === 'resident' ? [...key, unit.id_low, unit.id_high, c.base, c.base + 56, c.base + 96]
          : [...key, unit.generation, c.base, c.base + 56, c.base + 96];
        event.status = c.api[event.name](...event.args) >>> 0; counts.guard_calls++; counts.host_api_calls++;
      } else if (action.kind === 'close') {
        event.reference_snapshot = referenceSnapshot(c);
        event.status = c.spec.owner === 'standalone' ? 0 : c.api.close() >>> 0;
        if (c.spec.owner !== 'standalone') counts.host_api_calls++;
        c.closed = true; counts.closed++;
      } else throw new Error(`unknown action ${action.kind}`);
      if (!c.closed) {
        event.after = frame(c, action.id, 'after').index; event.memory_bytes_after = c.bytes.length;
        if (['module', 'table', 'call', 'guard', 'host'].includes(action.kind)) event.reference_snapshot = referenceSnapshot(c);
      }
    }
  } catch (error) {
    event.error = {name: error.name, message: error.message, stack: error.stack};
    if (c && !c.closed && event.after === null) {
      try {event.after = frame(c, action.id, 'after').index; event.memory_bytes_after = c.bytes.length;}
      catch (captureError) {event.capture_error = {name: captureError.name, message: captureError.message};}
    }
  }
  events.push(event); counts.events++; journal({kind: 'event', event});
  if (['call', 'closed_call', 'guard', 'close'].includes(action.kind) || event.error) checkpoint();
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
