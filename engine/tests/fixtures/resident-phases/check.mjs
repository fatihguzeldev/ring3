import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {existsSync, lstatSync, readFileSync, readdirSync, realpathSync} from 'node:fs';
import {dirname, join, relative, resolve, sep} from 'node:path';
import {fileURLToPath} from 'node:url';

const SIZE = 4364;
const TRANSFER = 140;
const FP = 4236;
const u32 = value => value >>> 0;
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const word = (bytes, offset) => bytes.readUInt32LE(offset);
const put = (bytes, offset, value) => bytes.writeUInt32LE(u32(value), offset);

function integer(value, label, maximum = 0xffffffff) {
  assert(Number.isSafeInteger(value) && value >= 0 && value <= maximum, label);
  return value;
}

function hexBytes(value, label) {
  assert.equal(typeof value, 'string', label);
  assert(/^(?:[a-f0-9]{2})*$/.test(value), label);
  return Buffer.from(value, 'hex');
}

function header(bytes, offset, magic, length, version = 1) {
  bytes.write(magic, offset, 4, 'ascii');
  for (const [at, value] of [[4, 0x10000 | version], [8, length], [12, 0]]) put(bytes, offset + at, value);
}

function validateHeader(bytes, offset, magic, length, version = 1) {
  assert.equal(bytes.subarray(offset, offset + 4).toString('ascii'), magic, `${magic} magic`);
  assert.equal(word(bytes, offset + 4), 0x10000 | version, `${magic} version/profile`);
  assert.equal(word(bytes, offset + 8), length, `${magic} length`);
  assert.equal(word(bytes, offset + 12), 0, `${magic} reserved`);
}

function validateState(bytes) {
  validateHeader(bytes, 0, 'R3ST', 56);
  assert.equal(word(bytes, 52) & ~0xcd7, 0, 'reserved FLAGS');
  assert.equal(word(bytes, 52) & 2, 2, 'required FLAGS bit');
}

function validateFp(bytes) {
  validateHeader(bytes, FP, 'R3FP', 128);
  assert(bytes.readUInt16LE(FP + 22) <= 0x7ff, 'reserved x87 opcode');
  assert(bytes.subarray(FP + 36, FP + 40).every(value => value === 0), 'reserved x87 bytes36..40');
  assert(bytes.subarray(FP + 120, FP + 128).every(value => value === 0), 'reserved x87 bytes120..128');
}

function defaultArena() {
  const bytes = Buffer.alloc(SIZE);
  header(bytes, 0, 'R3ST', 56);
  put(bytes, 52, 2);
  header(bytes, 56, 'R3EX', 40);
  put(bytes, 72, 1);
  header(bytes, 100, 'R3MH', 40);
  header(bytes, FP, 'R3FP', 128);
  bytes.writeUInt16LE(0x37f, FP + 16);
  bytes.writeUInt16LE(0xffff, FP + 20);
  return bytes;
}

function exitPacket(bytes, reason, retired, version) {
  bytes.fill(0, 56, 96);
  header(bytes, 56, 'R3EX', 40, version);
  put(bytes, 72, reason);
  put(bytes, 76, retired);
}

function installation(bytes, unit) {
  bytes.fill(0, TRANSFER, TRANSFER + 32);
  header(bytes, TRANSFER, 'R3IN', 32);
  put(bytes, TRANSFER + 16, unit.low);
  put(bytes, TRANSFER + 20, unit.high);
  put(bytes, TRANSFER + 24, unit.slot);
}

function parseBanks(banks) {
  assert([1, 4, 8].includes(banks.length), 'unit count');
  const instructions = new Map();
  const pages = new Set();
  for (const [unit, bank] of banks.entries()) {
    assert.equal(bank.unit, unit, 'bank ordinal');
    integer(bank.pc, 'bank PC');
    assert.equal(bank.pc % 4096, 0, 'bank page alignment');
    assert(!pages.has(bank.pc), 'duplicate bank page');
    pages.add(bank.pc);
    const bytes = hexBytes(bank.hex, 'bank bytes');
    assert.equal(bytes.length, 194, 'bank length');
    for (let index = 0; index < 63; index++) {
      const offset = index * 3;
      assert(bytes.subarray(offset, offset + 3).equals(Buffer.from([0x8d, 0x40, 1])), 'literal LEA bytes');
      instructions.set(bank.pc + offset, {unit, lea: true, next: bank.pc + offset + 3});
    }
    assert.equal(bytes[189], 0xe9, 'literal near JMP');
    const target = u32(bank.pc + 194 + bytes.readInt32LE(190));
    assert.equal(target, banks[(unit + 1) % banks.length].pc, 'ring JMP destination');
    instructions.set(bank.pc + 189, {unit, lea: false, next: target});
    const wanted = bank.blocks.length === 1
      ? [{pc: bank.pc, length: 194}]
      : Array.from({length: 8}, (_, index) => ({pc: bank.pc + index * 24, length: index === 7 ? 26 : 24}));
    assert([1, 8].includes(bank.blocks.length), 'block count');
    assert.deepEqual(bank.blocks, wanted, 'literal block partition');
  }
  return {banks, instructions};
}

// Arithmetic comes from walking the physical raw instruction graph, not a producer result.
function trace(program, pc, count) {
  integer(count, 'instruction count', Number.MAX_SAFE_INTEGER);
  const path = [];
  const seen = new Set();
  let cursor = pc;
  do {
    assert(!seen.has(cursor), 'raw graph repeats before its entry');
    seen.add(cursor);
    const instruction = program.instructions.get(cursor);
    assert(instruction, `raw instruction membership ${cursor.toString(16)}`);
    path.push({pc: cursor, ...instruction});
    cursor = instruction.next;
  } while (cursor !== pc);
  assert.equal(path.length, program.instructions.size, 'raw graph covers every instruction');
  const cycles = Math.floor(count / path.length);
  const remainder = count % path.length;
  const leaCycle = path.filter(row => row.lea).length;
  return {
    pc: remainder === 0 ? pc : path[remainder].pc,
    increments: cycles * leaCycle + path.slice(0, remainder).filter(row => row.lea).length,
    lastUnit: count === 0 ? null : path[(count - 1) % path.length].unit,
  };
}

function current(unit, context) {
  return unit.version === context.pages.get(context.banks[unit.unit].pc)?.version;
}

function modelCalls(before, action, context, program) {
  const after = Buffer.from(before);
  integer(action.count, 'call count', 1000000);
  assert(action.count > 0, 'nonempty call batch');
  const zero = {after, status_or: 0, predicted_retired: 0};
  if (action.channel === 'empty') return zero;
  validateState(before);
  if (action.channel === 'guard') {
    assert(context.units.every(unit => current(unit, context)), 'guard batch current units');
    return zero;
  }
  if (action.channel === 'finder') {
    const unit = context.units[(action.count - 1) % context.units.length];
    assert(context.units.every(row => row.installed && current(row, context)), 'finder batch current installed units');
    installation(after, unit);
    return zero;
  }
  assert(['dispatch', 'direct'].includes(action.channel), 'known generated channel');
  integer(action.budget, 'guest budget');
  assert.equal(action.channel === 'direct' ? context.banks.length : 1, 1, 'direct control is one unit');
  const cancelled = word(before, 96) !== 0;
  if (action.channel === 'dispatch' && (cancelled || action.budget === 0)) {
    exitPacket(after, cancelled ? 2 : 1, 0, 3);
    return zero;
  }
  const selected = program.instructions.get(word(before, 48));
  assert(selected, 'entry is a literal raw instruction');
  assert(context.units[selected.unit]?.installed, 'selected child installed');
  if (!current(context.units[selected.unit], context)) return {...zero, status_or: 4};
  if (cancelled || action.budget === 0) {
    exitPacket(after, cancelled ? 2 : 1, 0, 1);
    return zero;
  }
  assert(context.units.every(unit => current(unit, context)), 'positive ring current');
  const retired = action.count * action.budget;
  integer(retired, 'batch model work', Number.MAX_SAFE_INTEGER);
  const result = trace(program, word(before, 48), retired);
  put(after, 16, word(before, 16) + result.increments);
  put(after, 48, result.pc);
  exitPacket(after, 1, action.budget, 1);
  if (action.channel === 'dispatch') installation(after, context.units[result.lastUnit]);
  return {after, status_or: 0, predicted_retired: retired};
}

class Reader {
  constructor(bytes) { this.bytes = bytes; this.at = 0; }
  byte() { assert(this.at < this.bytes.length, 'truncated Wasm'); return this.bytes[this.at++]; }
  uint() {
    let value = 0;
    for (let shift = 0; shift < 35; shift += 7) {
      const byte = this.byte();
      value += (byte & 127) * 2 ** shift;
      if (!(byte & 128)) return integer(value, 'Wasm u32');
    }
    assert.fail('oversized Wasm LEB');
  }
  take(length) { integer(length, 'Wasm length'); assert(this.at + length <= this.bytes.length, 'Wasm range'); const out = this.bytes.subarray(this.at, this.at + length); this.at += length; return out; }
  name() { return this.take(this.uint()).toString('utf8'); }
  vector(read) { return Array.from({length: this.uint()}, () => read(this)); }
  end() { assert.equal(this.at, this.bytes.length, 'unconsumed Wasm section'); }
}

function lebSigned(value) {
  const bytes = [];
  let rest = value | 0;
  for (;;) {
    const byte = rest & 127;
    rest >>= 7;
    const done = (rest === 0 && !(byte & 64)) || (rest === -1 && (byte & 64));
    bytes.push(byte | (done ? 0 : 128));
    if (done) return bytes;
  }
}

function moduleInterface(bytes, target, key, unit) {
  assert(bytes.subarray(0, 8).equals(Buffer.from([0, 97, 115, 109, 1, 0, 0, 0])), 'Wasm header');
  const reader = new Reader(bytes.subarray(8));
  const sections = new Map();
  let previous = 0;
  while (reader.at < reader.bytes.length) {
    const id = reader.byte();
    const body = reader.take(reader.uint());
    if (id === 0) continue;
    assert(id > previous, 'Wasm section order/duplicate');
    previous = id;
    assert([1, 2, 3, 7, 10].includes(id), 'unexpected generated Wasm section');
    sections.set(id, body);
  }
  assert.deepEqual([...sections.keys()], [1, 2, 3, 7, 10], 'complete module sections');
  const typesReader = new Reader(sections.get(1));
  const types = typesReader.vector(r => { assert.equal(r.byte(), 0x60); return [r.vector(q => q.byte()), r.vector(q => q.byte())]; });
  typesReader.end();
  const importsReader = new Reader(sections.get(2));
  const imports = importsReader.vector(r => {
    const row = {module: r.name(), name: r.name(), kind: r.byte()};
    if (row.kind === 0) row.type = r.uint();
    else if (row.kind === 1) Object.assign(row, {element: r.byte(), flags: r.uint(), minimum: r.uint(), maximum: r.uint()});
    else if (row.kind === 2) Object.assign(row, {flags: r.uint(), minimum: r.uint()});
    else assert.fail('unexpected import kind');
    return row;
  });
  importsReader.end();
  const dispatcher = target === 'dispatcher';
  assert(dispatcher || target === 'child', 'module target');
  const i32 = count => [Array(count).fill(0x7f), [0x7f]];
  assert.deepEqual(types, dispatcher ? [i32(4), i32(5), i32(3)] : [i32(4), i32(7)], 'exact generated types');
  const memory = {module: 'env', name: 'memory', kind: 2, flags: 0, minimum: 1};
  assert.deepEqual(imports, dispatcher ? [memory, {module: 'env', name: 'table', kind: 1, element: 0x70, flags: 1, minimum: 8, maximum: 8}, {module: 'ring3', name: 'guard_dispatch_entry', kind: 0, type: 1}, {module: 'ring3', name: 'find_installed_resident', kind: 0, type: 2}] : [memory, {module: 'ring3', name: 'guard_resident', kind: 0, type: 1}], 'real guard/finder-only imports');
  const functionsReader = new Reader(sections.get(3));
  assert.deepEqual(functionsReader.vector(r => r.uint()), [0]); functionsReader.end();
  const exportsReader = new Reader(sections.get(7));
  const exports = exportsReader.vector(r => ({name: r.name(), kind: r.byte(), index: r.uint()}));
  assert.deepEqual(exports, [{name: 'run', kind: 0, index: dispatcher ? 2 : 1}]); exportsReader.end();
  const bodiesReader = new Reader(sections.get(10));
  assert.equal(bodiesReader.uint(), 1);
  const body = new Reader(bodiesReader.take(bodiesReader.uint())); bodiesReader.end();
  assert.deepEqual(body.vector(r => [r.uint(), r.byte()]), dispatcher ? [[5, 0x7f]] : [[16, 0x7f], [1, 0x7e]], 'fixed generated locals');
  const bound = dispatcher ? key : [...key, unit.low, unit.high];
  const prefix = Buffer.from(bound.flatMap(value => [0x41, ...lebSigned(value)]).concat([0x20, 0, 0x20, 1, 0x20, 3, 0x10, 0]));
  assert(body.take(prefix.length).equals(prefix), 'module authority/identity guard prefix');
  return {imports, exports};
}

function safeFile(directory, name) {
  assert.equal(typeof name, 'string', 'physical file name');
  directory = resolve(directory);
  const path = resolve(directory, name);
  const inside = candidate => {
    const rel = relative(realpathSync(directory), candidate);
    return rel !== '' && rel !== '..' && !rel.startsWith(`..${sep}`) && !rel.startsWith(sep);
  };
  assert(inside(path), 'physical file within capsule');
  assert(lstatSync(path).isFile(), 'physical regular file, without symlink');
  assert(inside(realpathSync(path)), 'physical real path within capsule');
  return path;
}

function sameBytes(actual, expected, label) {
  assert.equal(actual.length, expected.length, `${label}: length`);
  const index = actual.findIndex((value, at) => value !== expected[at]);
  assert.equal(index, -1, `${label}: first differing byte ${index}`);
}

function jsonFile(directory, name) {
  return JSON.parse(readFileSync(safeFile(directory, name), 'utf8'));
}

function pinnedFile(directory, pin) {
  const bytes = readFileSync(safeFile(directory, pin.file));
  assert.equal(bytes.length, pin.bytes ?? pin.bytes_length, `physical length ${pin.file}`);
  assert.equal(hash(bytes), pin.sha256, `physical SHA256 ${pin.file}`);
  return bytes;
}

function ndjson(bytes, label) {
  if (bytes.length === 0) return [];
  assert.equal(bytes.at(-1), 10, `${label} complete row terminator`);
  return bytes.toString('utf8').slice(0, -1).split('\n').map(line => {
    assert(line.length > 0, `${label} empty row`);
    return JSON.parse(line);
  });
}

function duration(value) {
  assert.equal(typeof value, 'string', 'duration decimal string');
  assert(/^[1-9][0-9]*$/.test(value), 'positive canonical duration');
  assert(BigInt(value) <= 180000000000n, 'duration within process deadline');
  return value;
}

function timingRecords(result, processPlan) {
  duration(result.first_use.engine_read_ns); duration(result.first_use.engine_module_ns);
  const phases = [], samples = [];
  const contexts = new Map(processPlan.contexts.map(row => [row.id, row]));
  for (const event of result.events) {
    const context = contexts.get(event.context); assert(context, 'timing context');
    const phase = (name, ns, unit = null) => phases.push({process: result.process_index, shape: context.shape_id,
      context: context.id, context_kind: context.role, cold_ordinal: context.ordinal ?? null,
      unit_ordinal: unit === null ? null : unit + 1, phase: name, ns: duration(ns), event: event.action_id});
    if (event.kind === 'open') {
      phase('engine_instance', event.engine_instance_ns); phase('open', event.ns); phase('table_create', event.table_create_ns);
    } else if (event.kind === 'module') {
      for (const name of ['copy', 'module', 'instance']) phase(event.target === 'child' ? name
        : name === 'module' ? 'dispatcher_v8_module' : `dispatcher_${name}`, event[`${name}_ns`], event.unit ?? null);
    } else if (event.kind === 'host') phase(event.name === 'compile_resident' ? 'compile'
      : event.name === 'acknowledge_resident_installation' ? 'ack' : event.name, event.ns, event.unit ?? null);
    else if (event.kind === 'calls') {
      phase(event.phase, event.ns, event.channel === 'direct' ? event.unit : null);
      if (event.sample !== undefined) samples.push({process: result.process_index, shape: context.shape_id, context: context.id,
        kind: event.channel, sample: event.sample, ns: duration(event.ns), calls: event.count,
        expected_instructions_per_call: ['dispatch', 'direct'].includes(event.channel) ? 8192 : 0, status_or: event.status_or});
    } else phase(event.kind, event.ns, event.unit ?? null);
  }
  assert.deepEqual(result.phases, phases, 'phase durations exactly bound to recorded event timers');
  assert.deepEqual(result.samples, samples, 'sample durations exactly bound to planned timed batches');
}

function resolvedArgs(context, values) {
  return values.map(value => {
    if (value === 'key_low') return context.key[0];
    if (value === 'key_high') return context.key[1];
    if (typeof value !== 'string') return value;
    const match = /^unit:(\d+):(low|high)$/.exec(value);
    assert(match, 'known argument placeholder');
    return context.units[Number(match[1])][match[2]];
  });
}

function liveAllocation(pointer, length, memoryBytes, label) {
  integer(pointer, `${label} pointer`); integer(length, `${label} length`);
  assert(pointer > 0 && length >= 8 && pointer + length <= memoryBytes, `${label} live bounds`);
  assert(length <= 512 * 1024, `${label} bounded module bytes`);
}

function hostEffect(before, after, action, event, context) {
  assert.equal(event.status, 0, 'successful host prerequisite');
  const args = resolvedArgs(context, action.args);
  assert.deepEqual(event.args, args, 'literal/resolved host arguments');
  const wanted = Buffer.from(before);
  switch (action.name) {
    case 'map': {
      const [pc, count, permission] = args;
      assert.equal(count, 1); assert.equal(permission, 7); assert(!context.pages.has(pc), 'fresh code page');
      context.pages.set(pc, {version: 1, permissions: permission, bytes: Buffer.alloc(4096)});
      break;
    }
    case 'upload': {
      const [pc, length] = args, page = context.pages.get(pc);
      assert(page && (page.permissions & 2), 'writable upload page'); assert.equal(length, 194);
      const bank = context.banks.find(row => row.pc === pc); assert(bank, 'authored upload destination');
      const bytes = before.subarray(TRANSFER, TRANSFER + length);
      sameBytes(bytes, hexBytes(bank.hex, 'upload bank'), 'uploaded literal bank');
      bytes.copy(page.bytes); page.version++;
      break;
    }
    case 'protect': {
      const [pc, count, permissions] = args, page = context.pages.get(pc);
      assert(page); assert.equal(count, 1); assert.equal(permissions, 5);
      page.permissions = permissions; page.version++;
      break;
    }
    case 'compile_resident': {
      const bank = context.banks[action.unit], page = context.pages.get(bank.pc);
      assert(page && (page.permissions & 4), 'executable compile page');
      assert.equal(args[0], bank.blocks.length);
      const descriptor = Buffer.alloc(args[0] * 8);
      bank.blocks.forEach((block, index) => {put(descriptor, index * 8, block.pc); put(descriptor, index * 8 + 4, block.length);});
      sameBytes(before.subarray(TRANSFER, TRANSFER + descriptor.length), descriptor, 'compile block descriptors');
      sameBytes(page.bytes.subarray(0, 194), hexBytes(bank.hex, 'compiled raw bank'), 'compiled raw bytes');
      assert.equal(word(after, TRANSFER), 1); assert.equal(word(after, TRANSFER + 4), 24);
      const unit = {unit: action.unit, low: word(after, TRANSFER + 8), high: word(after, TRANSFER + 12), pointer: word(after, TRANSFER + 16), bytes_length: word(after, TRANSFER + 20), version: page.version, installed: false};
      assert(unit.low !== 0 || unit.high !== 0, 'nonzero unit identity');
      assert(!context.units.some(row => row && row.low === unit.low && row.high === unit.high), 'unique context unit identity');
      liveAllocation(unit.pointer, unit.bytes_length, event.memory_bytes_after, 'resident module');
      context.units[action.unit] = unit;
      [1, 24, unit.low, unit.high, unit.pointer, unit.bytes_length].forEach((value, index) => put(wanted, TRANSFER + index * 4, value));
      break;
    }
    case 'dispatcher_module': {
      assert.deepEqual(args, context.key);
      const metadata = {pointer: word(after, TRANSFER + 24), bytes_length: word(after, TRANSFER + 28)};
      liveAllocation(metadata.pointer, metadata.bytes_length, event.memory_bytes_after, 'dispatcher module');
      context.dispatcher = metadata;
      wanted.fill(0, TRANSFER, TRANSFER + 32); header(wanted, TRANSFER, 'R3DP', 32);
      [...context.key, metadata.pointer, metadata.bytes_length].forEach((value, index) => put(wanted, TRANSFER + 16 + index * 4, value));
      break;
    }
    case 'acknowledge_resident_installation': {
      const unit = context.units[action.unit]; assert(unit && current(unit, context), 'current acknowledged unit');
      assert.equal(context.table[action.unit], unit.file, 'real child selected in table');
      assert.deepEqual(args, [...context.key, unit.low, unit.high, action.unit]);
      unit.slot = action.unit; unit.installed = true; installation(wanted, unit);
      break;
    }
    case 'write8': {
      assert.equal(context.role, 'stale'); assert.deepEqual(args, [0x1000, 0x8d]);
      const page = context.pages.get(0x1000); assert(page && (page.permissions & 2), 'stale write is writable');
      assert.equal(page.bytes[0], args[1], 'same-byte invalidation input');
      page.version++; page.bytes[0] = args[1];
      wanted.fill(0, 100, 140); header(wanted, 100, 'R3MH', 40, 3); put(wanted, 136, 1);
      break;
    }
    default: assert.fail(`unmodelled host API ${action.name}`);
  }
  if (['compile_resident', 'dispatcher_module', 'acknowledge_resident_installation'].includes(action.name)) {
    const length = action.name === 'compile_resident' ? 24 : 32;
    const packet = after.subarray(TRANSFER, TRANSFER + length);
    assert.deepEqual(event.receipt, {hex: packet.toString('hex'), words: Array.from({length: length / 4}, (_, index) => word(packet, index * 4))}, 'physical receipt copy');
  }
  return wanted;
}

function referenceSnapshot(snapshot, context, memoryBytes) {
  assert.equal(snapshot.memory_same, true, 'real engine memory reference');
  assert.deepEqual(snapshot.table_slots, Array.from({length: 8}, (_, slot) => ({slot, unit: context.units[slot]?.unit ?? null, reference_equal: true})), 'exact child/null table references');
  const live = [...context.units, context.dispatcher].filter(Boolean);
  assert.equal(snapshot.live_module_bytes.length, live.length, 'live allocation census');
  for (const [index, row] of snapshot.live_module_bytes.entries()) {
    const module = live[index];
    liveAllocation(row.pointer, row.bytes_length, memoryBytes, 'observed module');
    assert.deepEqual(row, {file: module.file, pointer: module.pointer, bytes_length: module.bytes_length, sha256: module.sha256}, 'live immutable module bytes');
  }
}

function checkedSourcePins(result, campaign) {
  const expected = Object.keys(campaign.source_pins).sort().map(path => ({path, ...campaign.source_pins[path]}));
  assert.equal(expected.length, 91, 'complete product/build/five-fixture source authority');
  for (const name of ['plan.mjs', 'run.mjs', 'check.mjs', 'statistics.mjs', 'measure.py']) {
    assert(expected.some(row => row.path === `engine/tests/fixtures/resident-phases/${name}`), 'exact fixture authority');
  }
  assert.deepEqual(result.source_pins_before, expected, 'exact frozen source keyset before');
  assert.deepEqual(result.source_pins_after, expected, 'exact frozen source keyset after');
  const root = resolve(dirname(fileURLToPath(import.meta.url)), '../../../..');
  for (const row of expected) {
    const bytes = readFileSync(safeFile(root, row.path));
    assert.equal(bytes.length, row.bytes, `current source length ${row.path}`);
    assert.equal(hash(bytes), row.sha256, `current source hash ${row.path}`);
  }
}

export async function checkCapsule(directory, authority) {
  directory = resolve(directory);
  const campaign = authority ?? jsonFile(dirname(directory), 'campaign.json');
  const result = jsonFile(directory, 'result.json');
  assert.equal(result.schema_version, 1); assert.equal(result.status, 'complete', 'full semantic replay requires a complete capsule');
  integer(result.process_index, 'process index', 2);
  const manifest = jsonFile(directory, 'manifest.json');
  for (const key of ['schema_version', 'process_index', 'plan', 'engine', 'first_use', 'environment', 'source_pins_before', 'planned_counts', 'bank_files']) assert.deepEqual(result[key], manifest[key], `immutable manifest ${key}`);
  assert.deepEqual(result.engine, {bytes: campaign.engine.bytes, sha256: campaign.engine.sha256}, 'campaign release authority');
  const engine = readFileSync(campaign.engine.path);
  assert.equal(engine.length, campaign.engine.bytes); assert.equal(hash(engine), campaign.engine.sha256);
  checkedSourcePins(result, campaign);
  const {makePlan} = await import('./plan.mjs');
  const intended = makePlan(), processPlan = intended.processes[result.process_index];
  const savedPlan = jsonFile(directory, result.plan.file);
  assert.deepEqual(savedPlan, {schema_version: intended.schema_version, counts: intended.counts, process: processPlan, shapes: intended.shapes, layout: intended.layout}, 'complete intended process schedule');
  assert.equal(hash(readFileSync(safeFile(directory, result.plan.file))), result.plan.sha256, 'saved plan SHA256');
  assert.deepEqual(result.planned_counts, intended.counts);
  const expectedBanks = new Map();
  for (const context of processPlan.contexts) for (const bank of context.banks) {
    parseBanks(context.banks);
    const bytes = readFileSync(safeFile(directory, bank.file));
    sameBytes(bytes, hexBytes(bank.hex, 'physical raw bank'), 'physical authored bank');
    assert.equal(hash(bytes), bank.sha256);
    expectedBanks.set(bank.file, {file: bank.file, bytes: bytes.length, sha256: hash(bytes)});
  }
  assert.deepEqual(result.bank_files, [...expectedBanks.values()], 'physical bank census');
  const raw = pinnedFile(directory, result.raw), referenced = new Set();
  assert.equal(raw.length, result.frames.length * SIZE, 'exact physical full-frame file');
  const frameAt = (index, event, side, context) => {
    integer(index, 'frame index', result.frames.length - 1); assert(!referenced.has(index), 'single frame owner');
    assert.equal(index, referenced.size, 'append-only physical frame order');
    referenced.add(index);
    const row = result.frames[index];
    assert.deepEqual({index: row.index, offset: row.offset, length: row.length, context: row.context, event: row.event, side: row.side}, {index, offset: index * SIZE, length: SIZE, context: context.id, event: event.action_id, side});
    integer(row.memory_bytes, 'live memory bytes'); assert.equal(row.memory_bytes % 65536, 0);
    assert(row.memory_bytes >= context.base + SIZE && row.memory_bytes >= context.memory_bytes, 'live arena / monotonic memory');
    context.memory_bytes = row.memory_bytes;
    assert.equal(event[`memory_bytes_${side}`], row.memory_bytes, 'event live memory bounds');
    const bytes = raw.subarray(row.offset, row.offset + SIZE); assert.equal(hash(bytes), row.sha256, 'individual physical frame hash');
    return bytes;
  };
  const contexts = new Map(result.contexts.map(row => [row.id, row])); assert.equal(contexts.size, result.contexts.length, 'unique context metadata');
  const modules = new Map(result.modules.map(row => [row.file, row])); assert.equal(modules.size, result.modules.length, 'unique module files');
  const consumedModules = new Set(), eventIds = new Set(), snapshots = new Map();
  let cursor = 0, predicted = 0, individual = 0, individualRetired = 0;
  const counts = {engine_contexts: 0, opened: 0, closed: 0, child_modules: 0, dispatcher_modules: 0, generated_calls: 0, guard_calls: 0, finder_calls: 0, empty_iterations: 0, non_loop_host_api_calls: 0, input_events: 0, table_events: 0, events: 0, frames: 0, explicit_gc: 0};
  const snapshot = event => {
    counts.frames = referenced.size;
    snapshots.set(event.action_id, {...counts});
  };
  let partial = false;
  for (const spec of processPlan.contexts) {
    if (cursor === result.events.length) { partial = true; break; }
    const metadata = contexts.get(spec.id); assert(metadata, 'opened context metadata');
    assert.deepEqual({id: metadata.id, shape_id: metadata.shape_id, role: metadata.role, ordinal: metadata.ordinal, key: metadata.key, open_event: metadata.open_event, close_event: metadata.close_event}, {id: spec.id, shape_id: spec.shape_id, role: spec.role, ordinal: spec.ordinal ?? null, key: spec.key, open_event: `open:${spec.id}`, close_event: spec.actions.at(-1).id});
    integer(metadata.base, 'arena base'); assert(metadata.base > 0 && metadata.base % 4 === 0);
    const context = {...spec, base: metadata.base, memory_bytes: metadata.memory_bytes, pages: new Map(), units: [], table: Array(8).fill(null)};
    const program = parseBanks(spec.banks);
    const open = result.events[cursor++];
    assert.equal(open.kind, 'open'); assert.equal(open.context, spec.id); assert.equal(open.action_id, `open:${spec.id}`);
    assert.equal(open.status, 0); assert.deepEqual(open.args, [spec.pages, ...spec.key]); assert.equal(open.arena_ptr, metadata.base);
    assert.equal(open.before, null); assert(!eventIds.has(open.action_id)); eventIds.add(open.action_id);
    let last = frameAt(open.after, open, 'after', context); sameBytes(last, defaultArena(), 'fresh default arena');
    counts.engine_contexts++; counts.opened++; counts.non_loop_host_api_calls += 2; counts.events++;
    counts.explicit_gc++; snapshot(open);
    for (const action of spec.actions) {
      if (cursor === result.events.length) { partial = true; break; }
      const event = result.events[cursor++];
      assert.equal(event.action_id, action.id); assert.equal(event.context, spec.id); assert.equal(event.kind, action.kind);
      assert(!eventIds.has(event.action_id)); eventIds.add(event.action_id);
      for (const [key, value] of Object.entries(action)) if (!['id', 'kind', 'args'].includes(key)) assert.deepEqual(event[key], value, `action input ${key}`);
      const before = frameAt(event.before, event, 'before', context); sameBytes(before, last, 'continuous live arena chain');
      counts.events++;
      if (action.kind === 'calls' && action.timed) counts.explicit_gc++;
      if (action.kind === 'close') {
        assert.equal(event.after, null, 'no frame from freed owner'); assert.equal(event.status, 0);
        referenceSnapshot(event.reference_snapshot, context, event.memory_bytes_before);
        counts.closed++; counts.non_loop_host_api_calls++; last = null; snapshot(event); continue;
      }
      if (event.error && event.after === null) { partial = true; break; }
      const after = frameAt(event.after, event, 'after', context);
      let wanted = Buffer.from(before);
      if (action.kind === 'input') {
        const bytes = hexBytes(action.hex, 'authored input'); integer(action.offset, 'input offset', SIZE);
        assert(action.offset + bytes.length <= SIZE, 'bounded input'); bytes.copy(wanted, action.offset); counts.input_events++;
        validateState(wanted); validateFp(wanted);
      } else if (action.kind === 'host') {
        wanted = hostEffect(before, after, action, event, context); counts.non_loop_host_api_calls++;
      } else if (action.kind === 'module') {
        const row = modules.get(action.file); assert(row, 'physical module row'); assert(!consumedModules.has(action.file)); consumedModules.add(action.file);
        const unit = action.target === 'child' ? context.units[action.unit] : context.dispatcher;
        assert.equal(event.module_file, action.file);
        assert.deepEqual({context: row.context, target: row.target, unit: row.unit, pointer: row.pointer, bytes_length: row.bytes_length, id_low: row.id_low, id_high: row.id_high, slot: row.slot}, {context: spec.id, target: action.target, unit: action.unit ?? null, pointer: unit.pointer, bytes_length: unit.bytes_length, id_low: unit.low ?? null, id_high: unit.high ?? null, slot: action.unit ?? null});
        const bytes = pinnedFile(directory, {...row, bytes: row.bytes_length});
        assert.equal(event.module_copy_sha256, row.sha256, 'physical copy SHA256');
        liveAllocation(row.pointer, row.bytes_length, event.memory_bytes_after, 'physical generated module');
        const checked = moduleInterface(bytes, action.target, context.key, unit);
        const apiImports = checked.imports.map(row => ({module: row.module, name: row.name, kind: row.kind === 0 ? 'function' : row.kind === 1 ? 'table' : 'memory'}));
        assert.deepEqual(row.imports, apiImports); assert.deepEqual(row.exports, [{name: 'run', kind: 'function'}]);
        Object.assign(unit, {file: row.file, sha256: row.sha256}); counts[`${action.target === 'child' ? 'child' : 'dispatcher'}_modules`]++;
      } else if (action.kind === 'table') {
        assert(context.units[action.unit]?.file); assert.equal(event.reference_equal, true);
        context.table[action.unit] = context.units[action.unit].file; counts.table_events++;
      } else if (action.kind === 'calls') {
        assert.equal(event.completed_calls, action.count, 'complete fixed call batch');
        assert.equal(event.attempted_calls, action.count, 'exact attempted calls');
        const model = modelCalls(before, action, context, program); wanted = model.after;
        assert.equal(event.status_or, model.status_or, 'independent status OR'); predicted += model.predicted_retired;
        const counter = ['dispatch', 'direct'].includes(action.channel) ? 'generated_calls' : action.channel === 'guard' ? 'guard_calls' : action.channel === 'finder' ? 'finder_calls' : 'empty_iterations';
        counts[counter] += action.count;
        if (['dispatch', 'direct'].includes(action.channel) && action.count === 1) {individual++; individualRetired += model.predicted_retired;}
        referenceSnapshot(event.reference_snapshot, context, event.memory_bytes_after);
      } else assert.fail('unmodelled action');
      sameBytes(after, wanted, `full arena ${spec.id}/${action.id}`); last = after; snapshot(event);
      if (event.error) { partial = true; break; }
    }
    if (partial) break;
    assert.equal(last, null, 'owner closed before next context');
  }
  assert.equal(cursor, result.events.length, 'no unplanned trailing events');
  assert.equal(referenced.size, result.frames.length, 'no unowned frame');
  assert.equal(consumedModules.size, result.modules.length, 'no unowned module file');
  assert.equal(contexts.size, counts.engine_contexts, 'no unowned context metadata');
  assert.deepEqual(result.observed_counts, counts, 'independent event/API/frame census');
  if (result.status === 'complete') {
    assert.equal(partial, false); assert.equal(result.failure, null);
    assert.equal(counts.engine_contexts, 189); assert.equal(counts.closed, 189); assert.equal(individual, 216);
    assert.equal(predicted, 70992262); assert.equal(individualRetired, 1540486);
    assert.equal(counts.explicit_gc, intended.counts.explicit_gc_calls / 3, 'fixed GC census');
    const expectedFiles = new Set(['manifest.json', 'plan.json', 'arenas.bin', 'journal.ndjson', 'checkpoints.ndjson', 'result.json', ...expectedBanks.keys(), ...modules.keys()]);
    assert.deepEqual(readdirSync(directory).sort(), [...expectedFiles].sort(), 'exact immutable process physical file set');
  } else assert(result.failure, 'failed result reason');
  const journalBytes = pinnedFile(directory, result.journal);
  const journal = ndjson(journalBytes, 'journal');
  const expectedJournal = [], journalEnds = new Map();
  for (const event of result.events) {
    if (event.before !== null) expectedJournal.push({kind: 'frame', ...result.frames[event.before]});
    if (event.kind === 'module') expectedJournal.push({kind: 'module_file', ...modules.get(event.module_file)});
    if (event.after !== null) expectedJournal.push({kind: 'frame', ...result.frames[event.after]});
    expectedJournal.push({kind: 'event', event});
  }
  assert.deepEqual(journal, expectedJournal, 'exact append-only journal chronology and row kinds');
  assert.equal(result.journal.rows, journal.length, 'journal row receipt');
  assert.equal(journal.length, intended.counts.journal_rows / 3, 'independent journal row census');
  let journalOffset = 0;
  for (const line of journalBytes.toString('utf8').slice(0, -1).split('\n')) {
    journalOffset += Buffer.byteLength(line) + 1;
    const row = JSON.parse(line);
    if (row.kind === 'event') journalEnds.set(row.event.action_id, journalOffset);
  }
  const checkpoints = ndjson(pinnedFile(directory, result.checkpoints), 'checkpoints');
  assert.equal(result.checkpoints.rows, checkpoints.length, 'checkpoint row receipt');
  const checkpointEvents = result.events.filter(event => ['calls', 'close'].includes(event.kind)).map(event => event.action_id).concat(result.events.at(-1).action_id);
  assert.equal(checkpoints.length, intended.counts.checkpoint_rows / 3, 'independent checkpoint census');
  assert.deepEqual(checkpoints.map(row => row.last_event), checkpointEvents, 'exact checkpoint schedule');
  let previous = {events: 0, frames: 0, raw_bytes: 0, journal_bytes: 0};
  for (const [index, row] of checkpoints.entries()) {
    assert.equal(row.index, index); assert.equal(row.process_index, result.process_index);
    for (const key of ['events', 'frames', 'raw_bytes', 'journal_bytes']) assert(row[key] >= previous[key] && row[key] <= (key === 'events' ? result.events.length : key === 'frames' ? result.frames.length : key === 'raw_bytes' ? raw.length : result.journal.bytes), `checkpoint ${key}`);
    assert.equal(row.raw_bytes, row.frames * SIZE); assert.equal(row.last_event, result.events[row.events - 1]?.action_id ?? null);
    assert.equal(row.journal_bytes, journalEnds.get(row.last_event), 'checkpoint exact journal boundary');
    assert.deepEqual(row.observed_counts, snapshots.get(row.last_event), 'checkpoint independently replayed counts');
    assert.equal(row.events, row.observed_counts.events); assert.equal(row.frames, row.observed_counts.frames);
    previous = row;
  }
  assert.equal(previous.events, result.events.length); assert.equal(previous.frames, result.frames.length);
  timingRecords(result, processPlan);
  return {schema_version: 1, complete: result.status === 'complete', process_index: result.process_index, counts, model_expected_retired: predicted, individually_observed: {calls: individual, positive_retired: individualRetired}, result_sha256: hash(readFileSync(safeFile(directory, 'result.json'))), raw_sha256: result.raw.sha256, observation_limit: 'batch endpoints and status OR do not observe each intermediate Exit'};
}

// Failure evidence is inventoried, never promoted to complete semantic proof.
export async function inspectIncomplete(directory, processIndex) {
  directory = resolve(directory);
  const inventory = {process_index: processIndex, complete: false, semantic_certification: false, files: [],
    journal_events: 0, individually_hashed_frames: 0, available_full_frames: 0, trailing_frame_bytes: 0};
  if (!existsSync(directory)) return {...inventory, unavailable: true};
  for (const file of readdirSync(directory).sort()) {
    const bytes = readFileSync(safeFile(directory, file));
    inventory.files.push({file, bytes: bytes.length, observed_sha256: hash(bytes)});
  }
  const raw = existsSync(join(directory, 'arenas.bin')) ? readFileSync(safeFile(directory, 'arenas.bin')) : Buffer.alloc(0);
  inventory.available_full_frames = Math.floor(raw.length / SIZE); inventory.trailing_frame_bytes = raw.length % SIZE;
  if (!existsSync(join(directory, 'journal.ndjson'))) return inventory;
  const journal = readFileSync(safeFile(directory, 'journal.ndjson'), 'utf8');
  const end = journal.lastIndexOf('\n'), rows = end < 0 ? [] : journal.slice(0, end).split('\n').filter(Boolean).map(line => JSON.parse(line));
  inventory.unfinished_journal_tail_bytes = Buffer.byteLength(journal.slice(end + 1));
  const {makePlan} = await import('./plan.mjs');
  const expected = makePlan().processes[processIndex].contexts.flatMap(context => [
    {action_id: `open:${context.id}`, context: context.id, kind: 'open'},
    ...context.actions.map(action => ({action_id: action.id, context: context.id, kind: action.kind})),
  ]);
  let frameIndex = 0;
  for (const row of rows) {
    assert(['event', 'frame', 'module_file'].includes(row.kind), 'incomplete journal row kind');
    if (row.kind === 'frame') {
      assert.equal(row.index, frameIndex++); assert.equal(row.offset, row.index * SIZE); assert.equal(row.length, SIZE);
      assert(row.offset + SIZE <= raw.length, 'available journal frame bounds');
      assert.equal(hash(raw.subarray(row.offset, row.offset + SIZE)), row.sha256, 'available frame SHA256');
      inventory.individually_hashed_frames++;
    } else if (row.kind === 'event') {
      const event = row.event, planned = expected[inventory.journal_events++]; assert(planned, 'available planned event prefix');
      assert.deepEqual({action_id: event.action_id, context: event.context, kind: event.kind}, planned, 'available event prefix identity');
    } else {
      const bytes = readFileSync(safeFile(directory, row.file));
      assert.equal(bytes.length, row.bytes_length); assert.equal(hash(bytes), row.sha256, 'available copied module SHA256');
    }
  }
  return inventory;
}

export async function checkCampaign(directory) {
  directory = resolve(directory);
  const campaign = jsonFile(directory, 'campaign.json'); assert.equal(campaign.schema_version, 1);
  assert(['complete', 'incomplete'].includes(campaign.status), 'finished campaign status');
  const outcomes = [];
  for (const [index, child] of campaign.children.entries()) {
    assert.equal(child.process_index, index); assert.equal(child.result_file, `process-${index}/result.json`);
    const capsule = join(directory, `process-${index}`);
    if (child.result_pin) pinnedFile(directory, {file: child.result_file, ...child.result_pin});
    if (child.status === 'complete') {
      assert.equal(child.exit_code, 0); assert.equal(child.timed_out, false); assert(child.result_pin, 'successful child result binding');
      const checked = await checkCapsule(capsule, campaign); assert.equal(checked.process_index, index); outcomes.push(checked);
    } else {
      assert.equal(campaign.status, 'incomplete', 'complete campaign cannot contain partial child');
      outcomes.push(await inspectIncomplete(capsule, index));
    }
  }
  const complete = campaign.status === 'complete';
  if (complete) {
    assert.equal(outcomes.length, 3); assert(outcomes.every(row => row.complete));
    assert.deepEqual(campaign.build.command, ['cargo', 'build', '--locked', '--release', '-p', 'ring3-engine', '--target', 'wasm32-unknown-unknown']);
    assert.equal(campaign.build.exit_code, 0); assert.equal(campaign.build.timed_out, false);
    const sum = read => outcomes.reduce((n, row) => n + read(row), 0);
    for (const [key, value] of Object.entries({engine_contexts: 567, opened: 567, closed: 567, child_modules: 2427,
      dispatcher_modules: 567, generated_calls: 26082, guard_calls: 571392, finder_calls: 571392, explicit_gc: 2925,
      non_loop_host_api_calls: 14403, events: 28314, frames: 55494})) assert.equal(sum(row => row.counts[key]), value, `campaign ${key}`);
    assert.equal(sum(row => row.model_expected_retired), 212976786);
    assert.equal(sum(row => row.individually_observed.calls), 648);
    assert.equal(sum(row => row.individually_observed.positive_retired), 4621458);
  }
  return {schema_version: 1, complete, semantic_certification: complete, processes: outcomes,
    campaign_sha256: hash(readFileSync(safeFile(directory, 'campaign.json'))),
    observation_limit: 'timed batches certify full-frame endpoints and status OR; their work is model-expected'};
}

export function selfTest() {
  const banks = Array.from({length: 4}, (_, unit) => {
    const pc = 0x1000 + unit * 0x1000;
    const bytes = Buffer.concat([Buffer.from('8d4001'.repeat(63), 'hex'), Buffer.alloc(5)]);
    bytes[189] = 0xe9;
    bytes.writeInt32LE((0x1000 + ((unit + 1) % 4) * 0x1000) - pc - 194, 190);
    return {unit, pc, hex: bytes.toString('hex'), blocks: [{pc, length: 194}]};
  });
  const program = parseBanks(banks);
  const frame = defaultArena();
  put(frame, 16, 0xfffffff0); put(frame, 48, 0x1000); put(frame, 52, 0xcd7);
  const context = {banks, pages: new Map(banks.map(row => [row.pc, {version: 1}])), units: banks.map(row => ({unit: row.unit, version: 1, installed: true, low: row.unit + 1, high: 0, slot: row.unit}))};
  let state = frame;
  for (const budget of [1, 7, 55, 2]) {
    const result = modelCalls(state, {channel: 'dispatch', count: 1, budget}, context, program);
    assert.equal(result.status_or, 0); assert.equal(word(result.after, 76), budget); state = result.after;
  }
  assert.equal(word(state, 16), 0x30); assert.equal(word(state, 48), 0x2003); assert.equal(word(state, TRANSFER + 24), 1);
  const full = modelCalls(frame, {channel: 'dispatch', count: 32, budget: 8192}, context, program);
  assert.equal(word(full.after, 48), 0x1000); assert.equal(word(full.after, 16), u32(0xfffffff0 + 32 * 8064));
  assert.equal(word(full.after, 60), 0x10001); assert.equal(word(full.after, 76), 8192); assert.equal(word(full.after, TRANSFER + 24), 3);
  const badFp = Buffer.from(full.after); badFp[FP + 80] ^= 1;
  assert.throws(() => sameBytes(badFp, full.after, 'FP protection'));
  const badVersion = Buffer.from(full.after); put(badVersion, 60, 0x10003);
  assert.throws(() => sameBytes(badVersion, full.after, 'rejected synthetic normal exit'));
  const badBank = structuredClone(banks); badBank[0].hex = `90${badBank[0].hex.slice(2)}`;
  assert.throws(() => parseBanks(badBank));
  const badBranch = structuredClone(banks); const branch = Buffer.from(badBranch[3].hex, 'hex'); branch.writeInt32LE(0, 190); badBranch[3].hex = branch.toString('hex');
  assert.throws(() => parseBanks(badBranch));
  const cancelled = Buffer.from(frame); put(cancelled, 96, 1);
  const stop = modelCalls(cancelled, {channel: 'dispatch', count: 1, budget: 0}, context, program);
  assert.equal(word(stop.after, 60), 0x10003); assert.equal(word(stop.after, 72), 2); assert.equal(word(stop.after, 76), 0);
  context.pages.get(0x1000).version++;
  const stale = modelCalls(frame, {channel: 'dispatch', count: 1, budget: 8192}, context, program);
  assert.equal(stale.status_or, 4); sameBytes(stale.after, frame, 'stale old Exit/receipt neutrality');
  validateFp(frame);
  return {passed: 9, guest_execution: false};
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (process.argv[2] === '--self-test') console.log(JSON.stringify(selfTest()));
  else {
    assert.equal(process.argv.length, 3, 'usage: node check.mjs CAMPAIGN_OR_CAPSULE | --self-test');
    const directory = resolve(process.argv[2]);
    const checked = existsSync(join(directory, 'campaign.json')) ? await checkCampaign(directory)
      : jsonFile(directory, 'result.json').status === 'complete' ? await checkCapsule(directory)
        : await inspectIncomplete(directory, jsonFile(directory, 'result.json').process_index);
    console.log(JSON.stringify(checked, null, 2)); if (!checked.complete) process.exitCode = 2;
  }
}
