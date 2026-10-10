import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {lstatSync, mkdirSync, readFileSync, readdirSync, realpathSync, writeFileSync} from 'node:fs';
import {dirname, join, relative, resolve, sep} from 'node:path';
import {fileURLToPath} from 'node:url';

const SIZE = 4364, TRANSFER = 140, FP = 4236;
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const word = (bytes, at) => bytes.readUInt32LE(at);
const put = (bytes, at, value) => bytes.writeUInt32LE(value >>> 0, at);
const u32 = n => n >>> 0;
const full = (low, high) => BigInt(u32(low)) | (BigInt(u32(high)) << 32n);
const equalBytes = (actual, wanted, label) => {
  assert.equal(actual.length, wanted.length, `${label}: length`);
  const mismatch = actual.findIndex((n, i) => n !== wanted[i]);
  assert.equal(mismatch, -1, `${label}: first differing byte ${mismatch}`);
};
const hex = text => {
  assert.equal(typeof text, 'string'); assert(/^(?:[a-f0-9]{2})*$/.test(text));
  return Buffer.from(text, 'hex');
};
function header(bytes, at, magic, length, version = 1) {
  bytes.write(magic, at, 4, 'ascii');
  for (const [offset, n] of [[4, 0x10000 | version], [8, length], [12, 0]]) put(bytes, at + offset, n);
}
function defaultArena() {
  const bytes = Buffer.alloc(SIZE);
  header(bytes, 0, 'R3ST', 56); put(bytes, 52, 2);
  header(bytes, 56, 'R3EX', 40); put(bytes, 72, 1);
  header(bytes, 100, 'R3MH', 40);
  header(bytes, FP, 'R3FP', 128);
  bytes.writeUInt16LE(0x37f, FP + 16); bytes.writeUInt16LE(0xffff, FP + 20);
  return bytes;
}
function exit(bytes, reason, retired, version) {
  bytes.fill(0, 56, 96); header(bytes, 56, 'R3EX', 40, version);
  put(bytes, 72, reason); put(bytes, 76, retired);
}
function install(bytes, unit) {
  bytes.fill(0, TRANSFER, TRANSFER + 32); header(bytes, TRANSFER, 'R3IN', 32);
  put(bytes, TRANSFER + 16, unit.low); put(bytes, TRANSFER + 20, unit.high);
  put(bytes, TRANSFER + 24, unit.slot);
}
function cpuValid(bytes) {
  for (const [at, magic, length] of [[0, 'R3ST', 56]]) {
    assert.equal(bytes.subarray(at, at + 4).toString(), magic);
    assert.equal(word(bytes, at + 4), 0x10001); assert.equal(word(bytes, at + 8), length);
    assert.equal(word(bytes, at + 12), 0);
  }
  const flags = word(bytes, 52); assert.equal(flags & ~0xcd7, 0); assert.equal(flags & 2, 2);
}
function parseProgram(bytes) {
  assert.equal(bytes.length, 4096);
  const instructions = new Map(), regions = [];
  for (let i = 0; i < 9; i++) {
    const offset = i * 32, pc = 0x1000 + offset;
    assert.deepEqual([...bytes.subarray(offset, offset + 4)], [0x8d, 0x40, i + 1, 0xe9]);
    const target = u32(pc + 8 + bytes.readInt32LE(offset + 4));
    assert.equal(target, pc + 32);
    instructions.set(pc, {next: pc + 3, increment: i + 1});
    instructions.set(pc + 3, {next: target, increment: 0});
    regions.push({pc, length: 8});
    assert(bytes.subarray(offset + 8, offset + 32).every(n => n === 0xcc));
  }
  assert(bytes.subarray(9 * 32).every(n => n === 0xcc));
  return {instructions, regions};
}

class Reader {
  constructor(bytes) {this.bytes = bytes; this.at = 0;}
  byte() {assert(this.at < this.bytes.length, 'truncated Wasm'); return this.bytes[this.at++];}
  uint() {
    let n = 0;
    for (let shift = 0; shift < 35; shift += 7) {
      const b = this.byte(); n += (b & 127) * 2 ** shift;
      if (!(b & 128)) {assert(Number.isSafeInteger(n) && n <= 0xffffffff); return n;}
    }
    assert.fail('oversized LEB');
  }
  take(n) {assert(Number.isSafeInteger(n) && n >= 0 && this.at + n <= this.bytes.length); const b = this.bytes.subarray(this.at, this.at + n); this.at += n; return b;}
  name() {return this.take(this.uint()).toString('utf8');}
  vector(f) {return Array.from({length: this.uint()}, () => f(this));}
  end() {assert.equal(this.at, this.bytes.length, 'unconsumed section');}
}
function sections(bytes) {
  assert.deepEqual([...bytes.subarray(0, 8)], [0, 97, 115, 109, 1, 0, 0, 0]);
  const r = new Reader(bytes.subarray(8)), rows = new Map();
  while (r.at < r.bytes.length) {
    const id = r.byte(), body = r.take(r.uint()); if (id === 0) continue;
    assert(!rows.has(id), 'duplicate section'); rows.set(id, body);
  }
  return rows;
}
function signatures(bytes) {
  const s = sections(bytes), tr = new Reader(s.get(1));
  const types = tr.vector(r => {assert.equal(r.byte(), 0x60); return [r.vector(q => q.byte()), r.vector(q => q.byte())];}); tr.end();
  const imports = [], functionTypes = [];
  if (s.has(2)) {
    const ir = new Reader(s.get(2));
    ir.vector(r => {
      const row = {module: r.name(), name: r.name(), kind: r.byte()};
      if (row.kind === 0) {row.type = r.uint(); functionTypes.push(row.type);}
      else if (row.kind === 1) {row.element = r.byte(); row.flags = r.uint(); row.minimum = r.uint(); if (row.flags & 1) row.maximum = r.uint();}
      else if (row.kind === 2) {row.flags = r.uint(); row.minimum = r.uint(); if (row.flags & 1) row.maximum = r.uint();}
      else if (row.kind === 3) {row.value = r.byte(); row.mutable = r.byte();}
      else assert.fail('unknown import kind');
      imports.push(row);
    }); ir.end();
  }
  const fr = new Reader(s.get(3)); functionTypes.push(...fr.vector(r => r.uint())); fr.end();
  const er = new Reader(s.get(7));
  const exports = er.vector(r => {const row = {name: r.name(), kind: r.byte(), index: r.uint()}; if (row.kind === 0) row.signature = types[functionTypes[row.index]]; return row;}); er.end();
  return {s, types, imports, exports};
}
function signedLeb(value) {
  const bytes = []; let n = value | 0;
  for (;;) {const b = n & 127; n >>= 7; const done = (n === 0 && !(b & 64)) || (n === -1 && (b & 64)); bytes.push(b | (done ? 0 : 128)); if (done) return bytes;}
}
function generatedInterface(bytes, target, key, unit) {
  const m = signatures(bytes), dispatcher = target === 'dispatcher';
  assert(dispatcher || target === 'child');
  assert.deepEqual([...m.s.keys()], [1, 2, 3, 7, 10]);
  const signature = n => [Array(n).fill(0x7f), [0x7f]];
  assert.deepEqual(m.types, dispatcher ? [signature(4), signature(5), signature(3)] : [signature(4), signature(7)]);
  const memory = {module: 'env', name: 'memory', kind: 2, flags: 0, minimum: 1};
  assert.deepEqual(m.imports, dispatcher ? [memory, {module: 'env', name: 'table', kind: 1, element: 0x70, flags: 1, minimum: 8, maximum: 8}, {module: 'ring3', name: 'guard_dispatch_entry', kind: 0, type: 1}, {module: 'ring3', name: 'find_installed_resident', kind: 0, type: 2}] : [memory, {module: 'ring3', name: 'guard_resident', kind: 0, type: 1}]);
  assert.deepEqual(m.exports, [{name: 'run', kind: 0, index: dispatcher ? 2 : 1, signature: signature(4)}]);
  const r = new Reader(m.s.get(10)); assert.equal(r.uint(), 1); const body = new Reader(r.take(r.uint())); r.end();
  assert.deepEqual(body.vector(q => [q.uint(), q.byte()]), dispatcher ? [[5, 0x7f]] : [[16, 0x7f], [1, 0x7e]]);
  const identity = dispatcher ? key : [...key, unit.low, unit.high];
  const prefix = Buffer.from(identity.flatMap(n => [0x41, ...signedLeb(n)]).concat([0x20, 0, 0x20, 1, 0x20, 3, 0x10, 0]));
  equalBytes(body.take(prefix.length), prefix, 'baked full key/ID guard before body');
  return m;
}
function engineInterface(bytes, previousBytes) {
  const current = signatures(bytes), old = signatures(previousBytes);
  assert.deepEqual(current.imports, []); assert.deepEqual(old.imports, []);
  const name = 'ring3_abi_v1_discard_installed_resident';
  const strip = rows => rows.map(({index, ...row}) => row).sort((a, b) => a.name.localeCompare(b.name));
  const added = current.exports.filter(row => row.name === name); assert.equal(added.length, 1);
  assert.deepEqual(added[0].signature, [Array(5).fill(0x7f), [0x7f]]);
  assert.deepEqual(strip(current.exports.filter(row => row.name !== name)), strip(old.exports), 'all previous export names/kinds/signatures unchanged');
}
function physicalFile(directory, name) {
  const base = realpathSync(directory), path = resolve(directory, name), rel = relative(base, path);
  assert(rel && rel !== '..' && !rel.startsWith(`..${sep}`) && !rel.startsWith(sep));
  assert(lstatSync(path).isFile() && !lstatSync(path).isSymbolicLink());
  assert.equal(realpathSync(path), path); return readFileSync(path);
}
function pinnedFile(directory, row) {
  const bytes = physicalFile(directory, row.file); assert.equal(bytes.length, row.bytes ?? row.bytes_length); assert.equal(hash(bytes), row.sha256); return bytes;
}
function ndjson(bytes) {
  if (!bytes.length) return [];
  assert.equal(bytes.at(-1), 10);
  return bytes.toString().slice(0, -1).split('\n').map(line => {assert(line); return JSON.parse(line);});
}

function context(spec) {
  assert.equal(spec.owner, 'resident'); assert.equal(spec.pages, 1);
  return {spec, arena: defaultArena(), mapped: false, permissions: 0, version: 0,
    ram: Buffer.alloc(4096), nextId: 1n, units: new Map(), cache: new Map(), live: new Map(),
    slots: Array(8).fill(null), table: Array(8).fill(null), closed: false, retired: 0, calls: 0};
}
function argsFor(c, action) {
  const unit = action.binding ? c.cache.get(action.binding) ?? c.units.get(action.binding) : null;
  const value = n => {
    if (typeof n === 'number') {assert(Number.isInteger(n) && n >= 0 && n <= 0xffffffff); return n;}
    if (typeof n === 'object' && n !== null) {assert.deepEqual(Object.keys(n).sort(), ['ref', 'xor']); return u32(value(n.ref) ^ n.xor);}
    if (n === 'key_low') return c.spec.key[0]; if (n === 'key_high') return c.spec.key[1];
    assert(unit, 'unit placeholder has a bound identity');
    if (n === 'unit_low') return unit.low; if (n === 'unit_high') return unit.high;
    assert.fail('unknown symbolic argument');
  };
  return action.args.map(value);
}
function unitFor(c, id) {return [...c.units.values()].find(unit => unit.id === id);}
function current(c, unit) {return unit.version === c.version;}
function keyStatus(c, args) {return full(args[0], args[1]) === full(...c.spec.key) ? 0 : 3;}
function disposalStatus(c, args) {
  if (c.closed) return 5;
  const key = keyStatus(c, args); if (key) return key;
  const unit = unitFor(c, full(args[2], args[3])); if (!unit) return 3;
  if (!current(c, unit)) return 4;
  if (args[4] >= 8 || c.slots[args[4]] !== unit.binding) return 7;
  return word(c.arena, 96) ? 16 : 0;
}
function residentReceipt(bytes, unit) {
  for (const [i, n] of [1, 24, unit.low, unit.high, unit.pointer, unit.bytes_length].entries()) put(bytes, TRANSFER + 4 * i, n);
}
function liveBounds(pointer, length, memoryBytes) {
  assert(Number.isInteger(pointer) && pointer > 0 && Number.isInteger(length) && length >= 8 && length <= 524288);
  assert(Number.isInteger(memoryBytes) && pointer + length <= memoryBytes);
}
function pinSubset(required, actual) {
  assert(required && Object.keys(required).length > 0);
  for (const [path, expected] of Object.entries(required)) assert.deepEqual(actual[path], expected, `authority owner pin ${path}`);
}
function allocationBounds(c, metadata) {
  const lo = metadata.pointer, hi = lo + metadata.bytes_length;
  liveBounds(lo, metadata.bytes_length, c.memoryBytes);
  assert(hi <= c.base || lo >= c.base + SIZE, 'module allocation overlaps owned arena');
  for (const retained of c.live.values()) assert(hi <= retained.pointer || lo >= retained.pointer + retained.bytes_length, 'distinct live module allocations overlap');
}
function receipt(event, bytes, length) {
  assert(event.receipt); assert.equal(event.receipt.hex, bytes.subarray(TRANSFER, TRANSFER + length).toString('hex'));
  assert.deepEqual(event.receipt.words, Array.from({length: length / 4}, (_, i) => word(bytes, TRANSFER + i * 4)));
}
function hostEffect(c, action, event, program) {
  const a = argsFor(c, action), after = Buffer.from(c.arena); let status = 0;
  assert.deepEqual(event.args, a, 'resolved full ABI arguments');
  if (c.closed) {assert.equal(action.kind, 'closed_host'); return {status: 5, after};}
  switch (action.name) {
    case 'map':
      assert.deepEqual(a, [0x1000, 1, 7]); assert(!c.mapped); c.mapped = true; c.permissions = 7; c.version++; break;
    case 'upload':
      assert.deepEqual(a, [0x1000, 4096]); assert(c.mapped && (c.permissions & 2));
      c.arena.subarray(TRANSFER, TRANSFER + 4096).copy(c.ram); parseProgram(c.ram); c.version++; break;
    case 'protect':
      assert.deepEqual(a, [0x1000, 1, 4]); assert(c.mapped); c.permissions = 4; c.version++; break;
    case 'compile_resident':
    case 'compile_resident_entries': {
      assert.deepEqual(a, c.spec.entries ? [1, 0] : [1]);
      assert.equal(action.name, c.spec.entries ? 'compile_resident_entries' : 'compile_resident');
      if (c.units.size === 8) {status = 18; break;}
      assert(c.units.size < 8 && c.mapped && (c.permissions & 4));
      const index = action.group.charCodeAt(0) - 65, region = program.regions[index]; assert(region);
      assert.equal(word(c.arena, TRANSFER), region.pc);
      if (!c.spec.entries) assert.equal(word(c.arena, TRANSFER + 4), 8);
      assert(![...c.units.values()].some(unit => current(c, unit) && unit.pc === region.pc));
      const w = event.receipt.words; assert.equal(w.length, 6); assert.deepEqual(w.slice(0, 2), [1, 24]);
      assert.equal(full(w[2], w[3]), c.nextId); c.nextId++;
      liveBounds(w[4], w[5], event.memory_bytes_after);
      const unit = {binding: action.binding, group: action.group, id: full(w[2], w[3]), low: w[2], high: w[3], pointer: w[4], bytes_length: w[5], pc: region.pc, version: c.version, slot: null};
      c.units.set(action.binding, unit); c.pending = unit; residentReceipt(after, unit); receipt(event, after, 24);
      break;
    }
    case 'dispatcher_module': {
      status = keyStatus(c, a); if (status) break;
      const w = event.receipt.words; assert.equal(w.length, 8); liveBounds(w[6], w[7], event.memory_bytes_after);
      after.fill(0, TRANSFER, TRANSFER + 32); header(after, TRANSFER, 'R3DP', 32);
      put(after, TRANSFER + 16, c.spec.key[0]); put(after, TRANSFER + 20, c.spec.key[1]);
      put(after, TRANSFER + 24, w[6]); put(after, TRANSFER + 28, w[7]);
      c.dispatcher = {binding: 'dispatcher', pointer: w[6], bytes_length: w[7], low: null, high: null, slot: null}; receipt(event, after, 32); break;
    }
    case 'acknowledge_resident_installation': {
      status = keyStatus(c, a); if (status) break;
      const unit = unitFor(c, full(a[2], a[3])); assert(unit && current(c, unit));
      assert(a[4] < 8 && c.slots[a[4]] === null && unit.slot === null);
      assert.equal(c.table[a[4]], unit.binding, 'host installed the exact current function first');
      unit.slot = a[4]; c.slots[a[4]] = unit.binding; install(after, unit); receipt(event, after, 32); break;
    }
    case 'discard_installed_resident': {
      status = disposalStatus(c, a); if (status) break;
      const unit = unitFor(c, full(a[2], a[3])); assert.equal(c.table[a[4]], null, 'caller clears table before successful disposal');
      c.units.delete(unit.binding); c.live.delete(unit.binding); c.slots[a[4]] = null; break;
    }
    case 'discard_unacknowledged_resident':
    case 'retire_stale_resident': {
      status = keyStatus(c, a); if (status) break;
      const unit = unitFor(c, full(a[2], a[3]));
      if (!unit) {status = 3; break;}
      if (!current(c, unit)) {status = 4; break;}
      if (action.name === 'retire_stale_resident' || c.slots.includes(unit.binding)) {status = 7; break;}
      if (word(c.arena, 96)) {status = 16; break;}
      c.units.delete(unit.binding); c.live.delete(unit.binding); break;
    }
    case 'find_installed_resident': {
      status = keyStatus(c, a); if (status) break;
      const unit = [...c.units.values()].find(unit => unit.pc === a[2] && current(c, unit));
      if (!unit) status = [...c.units.values()].some(unit => unit.pc === a[2]) ? 4 : 17;
      else if (unit.slot === null || c.slots[unit.slot] !== unit.binding) status = 17;
      else install(after, unit);
      break;
    }
    default: assert.fail(`unmodeled host API ${action.name}`);
  }
  return {after, status};
}
function childEffect(c, binding, arena, budget, program) {
  const after = Buffer.from(arena), cached = c.cache.get(binding); assert(cached);
  const unit = c.units.get(binding);
  if (!unit) return {after, status: 3, retired: 0};
  if (!current(c, unit)) return {after, status: 4, retired: 0};
  cpuValid(after);
  if (word(after, 96) || budget === 0) {exit(after, word(after, 96) ? 2 : 1, 0, 1); return {after, status: 0, retired: 0};}
  let pc = word(after, 48), retired = 0, reason = 3;
  if (pc === unit.pc) {
    while (retired < budget) {
      const ins = program.instructions.get(pc); assert(ins && pc >= unit.pc && pc < unit.pc + 8);
      if (ins.increment) put(after, 16, word(after, 16) + ins.increment);
      pc = ins.next; retired++; put(after, 48, pc);
      if (retired === budget) {reason = 1; break;}
      if (pc < unit.pc || pc >= unit.pc + 8) break;
    }
  }
  exit(after, reason, retired, 1); return {after, status: 0, retired};
}
function callEffect(c, action, program) {
  if (c.closed) return {after: null, status: 5, retired: 0};
  if (action.channel === 'direct') return childEffect(c, action.binding, c.arena, action.budget, program);
  assert.equal(action.channel, 'dispatch'); assert.equal(action.binding, 'dispatcher');
  let after = Buffer.from(c.arena), budget = action.budget, retired = 0;
  cpuValid(after);
  for (let steps = 0; steps < 20; steps++) {
    if (word(after, 96) || budget === 0) {exit(after, word(after, 96) ? 2 : 1, retired, 3); return {after, status: 0, retired};}
    const pc = word(after, 48), unit = [...c.units.values()].find(unit => unit.pc === pc && current(c, unit));
    if (!unit || unit.slot === null || c.slots[unit.slot] !== unit.binding) {exit(after, 3, retired, 3); return {after, status: 0, retired};}
    install(after, unit);
    const binding = c.table[unit.slot];
    if (binding === null) {exit(after, 3, retired, 3); return {after, status: 0, retired};}
    const result = childEffect(c, binding, after, budget, program); after = result.after;
    if (result.status) {if (retired) put(after, 76, retired); return {...result, after, retired};}
    retired += result.retired; budget -= result.retired;
    if (word(after, 72) !== 3 || result.retired === 0) {put(after, 76, retired); return {after, status: 0, retired};}
  }
  assert.fail('bounded declared dispatcher trace did not stop');
}

function authoredArena() {
  const bytes = Buffer.from(Array.from({length: SIZE}, (_, i) => (37 * i + 19) & 255));
  bytes.fill(0, 0, 56); header(bytes, 0, 'R3ST', 56);
  [0xfffffff0, 0x11112222, 0x33334444, 0x55556666, 0x77778888, 0x9999aaaa, 0xbbbbcccc, 0xddddeeee, 0x1000, 0xcd7].forEach((n, i) => put(bytes, 16 + i * 4, n));
  exit(bytes, 3, 0, 3); put(bytes, 96, 0);
  bytes.fill(0, 100, 140); header(bytes, 100, 'R3MH', 40); put(bytes, 120, 0xdecafbad);
  bytes.fill(0, FP, SIZE); header(bytes, FP, 'R3FP', 128);
  for (const [at, n] of [[16, 0x27f], [18, 0x2840], [20, 0xaaaa], [22, 0x345], [32, 0x33], [34, 0x2b]]) bytes.writeUInt16LE(n, FP + at);
  put(bytes, FP + 24, 0x12345678); put(bytes, FP + 28, 0x9abcdef0);
  for (let i = 40; i < 120; i++) bytes[FP + i] = (29 * i + 7) & 255;
  return bytes;
}
function derivedCounts(plan) {
  const actions = plan.actions, count = predicate => actions.filter(predicate).length;
  const result = {contexts: plan.contexts.length, engine_instances: plan.contexts.length, actions: actions.length,
    generated_calls: count(a => ['call', 'closed_call'].includes(a.kind)), inputs: count(a => a.kind === 'input'), tables: count(a => a.kind === 'table'),
    child_modules: count(a => a.kind === 'module' && a.target === 'child'), dispatchers: count(a => a.kind === 'module' && a.target === 'dispatcher'),
    host_api_calls: count(a => ['host', 'closed_host', 'close'].includes(a.kind)) + 2 * plan.contexts.length,
    frames: actions.reduce((n, a) => n + (a.kind === 'open' || a.kind === 'close' ? 1 : ['closed_call', 'closed_host'].includes(a.kind) ? 0 : 2), 0),
    checkpoints: count(a => ['call', 'closed_call', 'closed_host', 'close'].includes(a.kind) || a.kind === 'host' && a.name === 'discard_installed_resident') + 1};
  result.modules = result.child_modules + result.dispatchers; result.journal_rows = result.frames + result.actions + result.modules;
  result.physical_files = result.modules + 10; return result;
}
function validatePlan(plan) {
  assert.equal(plan.schema_version, 1); assert.equal(plan.contexts.length, 2);
  equalBytes(hex(plan.initial_hex), defaultArena(), 'independent default arena');
  equalBytes(hex(plan.authored_hex), authoredArena(), 'independent authored arena');
  const program = parseProgram(hex(plan.bank.hex));
  assert.deepEqual(plan.bank.groups, program.regions.map((r, i) => ({id: String.fromCharCode(65 + i), blocks: [[r.pc, 8]], instructions: 2})));
  assert.equal(plan.bank.gap_pc, 0x1120);
  assert.deepEqual(plan.contexts, [false, true].map((entries, i) => ({id: i + 1, owner: 'resident', entries, role: 'main', key: [0x47700001 + i, 0x44495350], pages: 1})));
  plan.actions.forEach((a, i) => {assert.equal(a.id, i); assert([1, 2].includes(a.context)); assert(['open', 'input', 'host', 'module', 'table', 'call', 'close', 'closed_call', 'closed_host'].includes(a.kind));});
  assert.deepEqual(derivedCounts(plan), plan.counts, 'independently derived schedule census');
  assert.deepEqual(plan.counts, {contexts: 2, engine_instances: 2, actions: 200, generated_calls: 26, inputs: 36, tables: 28, child_modules: 20, dispatchers: 2, host_api_calls: 90, frames: 380, checkpoints: 57, modules: 22, journal_rows: 602, physical_files: 32});
  return program;
}
function refSnapshot(c) {
  return {memory_same: true,
    table_slots: c.table.map((binding, slot) => ({slot, binding, group: binding === null ? null : c.cache.get(binding).group, reference_equal: true})),
    acknowledgements: c.slots.map((binding, slot) => ({slot, binding, id_low: binding === null ? null : c.units.get(binding).low, id_high: binding === null ? null : c.units.get(binding).high})),
    cached_functions: [...c.cache.values()].map(unit => ({binding: unit.binding, group: unit.group, id_low: unit.low, id_high: unit.high, reference_equal: true})),
    live_module_bytes: [...c.live.values()].map(unit => ({file: unit.file, binding: unit.binding, pointer: unit.pointer, bytes_length: unit.bytes_length, sha256: unit.sha256}))};
}
function checkFrame(row, c, action, side, raw, frames, index) {
  assert.equal(row, index); const frame = frames[index]; assert(frame);
  assert.deepEqual({index: frame.index, offset: frame.offset, length: frame.length, context: frame.context, event: frame.event, side: frame.side}, {index, offset: SIZE * index, length: SIZE, context: c.spec.id, event: action.id, side});
  const bytes = raw.subarray(frame.offset, frame.offset + SIZE); assert.equal(bytes.length, SIZE); assert.equal(hash(bytes), frame.sha256);
  assert(Number.isInteger(frame.memory_bytes) && frame.memory_bytes >= c.base + SIZE);
  equalBytes(bytes, c.arena, `context${c.spec.id}/action${action.id}/${side}`); return frame;
}
function replay(plan, recorded = null) {
  const program = validatePlan(plan), owners = new Map(), statuses = {}, hostStatuses = {}, journal = [], checks = [];
  let frameIndex = 0, moduleIndex = 0, generated = 0, positive = 0, retired = 0, discards = 0, checkpoints = 0;
  const frame = (c, action, event, side) => {
    const row = recorded ? checkFrame(event[side], c, action, side, recorded.raw, recorded.result.frames, frameIndex) : {index: frameIndex, offset: SIZE * frameIndex, length: SIZE, sha256: hash(c.arena), context: action.context, event: action.id, side, memory_bytes: 16 * 1024 * 1024};
    if (recorded) {
      assert.equal(event[`memory_bytes_${side}`], row.memory_bytes);
      assert(row.memory_bytes >= (c.memoryBytes ?? 0), 'memory capacity cannot shrink'); c.memoryBytes = row.memory_bytes;
    }
    journal.push({kind: 'frame', ...row}); frameIndex++; return row;
  };
  for (const action of plan.actions) {
    const event = recorded ? recorded.result.events[action.id] : {action_id: action.id, context: action.context, kind: action.kind, before: null, after: null, ...Object.fromEntries(Object.entries(action).filter(([k]) => !['id', 'context', 'kind'].includes(k)))};
    assert(event && !event.error && !event.capture_error); assert.equal(event.action_id, action.id); assert.equal(event.context, action.context); assert.equal(event.kind, action.kind);
    for (const [key, value] of Object.entries(action)) if (!['id', 'context', 'kind', 'args'].includes(key)) assert.deepEqual(event[key], value, `literal action field ${key}`);
    let c = owners.get(action.context);
    if (action.kind === 'open') {
      assert(!c); c = context(plan.contexts.find(s => s.id === action.context)); owners.set(action.context, c); c.base = recorded ? event.arena_ptr : 65536;
      assert(Number.isInteger(c.base) && c.base > 0); assert.equal(event.before, null);
      if (recorded) {assert.deepEqual(event.args, [1, ...c.spec.key]); assert.equal(event.status, 0);} frame(c, action, event, 'after');
    } else if (['closed_call', 'closed_host'].includes(action.kind)) {
      assert(c?.closed); assert.equal(event.before, null); assert.equal(event.after, null);
      assert.equal(event.memory_bytes_before, undefined); assert.equal(event.memory_bytes_after, undefined); assert.equal(event.reference_snapshot, undefined);
      if (action.kind === 'closed_host') {if (!recorded) event.args = argsFor(c, action); const effect = hostEffect(c, action, event, program); if (recorded) assert.equal(event.status, effect.status);}
      else {generated++; statuses['closed:5'] = (statuses['closed:5'] ?? 0) + 1; if (recorded) {assert.equal(event.status, 5); assert.deepEqual(event.args, [c.base, c.base + 56, action.budget, c.base + 96]); assert.equal(event.completed_calls, 1);}}
    } else {
      assert(c && !c.closed); frame(c, action, event, 'before');
      if (action.kind === 'host' && recorded) assert.deepEqual(event.reference_snapshot_before, refSnapshot(c), 'whole pre-host references');
      if (action.kind === 'input') {
        const bytes = hex(action.hex); assert(Number.isInteger(action.offset) && action.offset >= 0 && action.offset + bytes.length <= SIZE);
        if (action.offset < 56) {
          if (action.offset === 0) {assert.equal(c.fullInputs ?? 0, 0); assert.equal(c.units.size, 0); equalBytes(bytes, authoredArena(), 'sole authored full input'); c.fullInputs = 1;}
          else {assert.equal(action.offset, 48); assert.equal(bytes.length, 4); assert.equal(c.primaryRetired, 18); assert.equal(c.gapInputs ?? 0, 0); assert.equal(word(bytes, 0), 0x1000); c.gapInputs = 1;}
        }
        bytes.copy(c.arena, action.offset);
      } else if (action.kind === 'host') {
        if (!recorded) {
          event.args = argsFor(c, action); event.memory_bytes_after = 16 * 1024 * 1024;
          if (action.name.startsWith('compile_resident') && c.units.size < 8) event.receipt = {words: [1, 24, Number(c.nextId), 0, 131072 + Number(c.nextId) * 8192, 1024]};
          if (action.name === 'dispatcher_module') event.receipt = {words: [0, 0, 0, 0, 0, 0, 98304, 1024]};
        }
        // Pure admission supplies hypothetical metadata; semantic outcomes are always derived here.
        if (!recorded && event.receipt) {
          const predicted = Buffer.from(c.arena), w = event.receipt.words;
          if (action.name.startsWith('compile_resident')) residentReceipt(predicted, {low: w[2], high: w[3], pointer: w[4], bytes_length: w[5]});
          else {predicted.fill(0, TRANSFER, TRANSFER + 32); header(predicted, TRANSFER, 'R3DP', 32); [c.spec.key[0], c.spec.key[1], w[6], w[7]].forEach((n, i) => put(predicted, TRANSFER + 16 + i * 4, n));}
          event.receipt = {hex: predicted.subarray(TRANSFER, TRANSFER + w.length * 4).toString('hex'), words: Array.from({length: w.length}, (_, i) => word(predicted, TRANSFER + i * 4))};
        }
        if (!recorded && action.name === 'acknowledge_resident_installation') {
          const unit = c.units.get(action.binding), predicted = Buffer.from(c.arena); install(predicted, {...unit, slot: action.args.at(-1)});
          event.receipt = {hex: predicted.subarray(TRANSFER, TRANSFER + 32).toString('hex'), words: Array.from({length: 8}, (_, i) => word(predicted, TRANSFER + i * 4))};
        }
        const effect = hostEffect(c, action, event, program); if (recorded) assert.equal(event.status, effect.status, `host status action${action.id}`);
        if (recorded && (!['compile_resident', 'compile_resident_entries', 'dispatcher_module', 'acknowledge_resident_installation', 'find_installed_resident'].includes(action.name) || effect.status !== 0)) assert.equal(event.receipt, undefined, 'status-only operation has no receipt');
        if (effect.status) assert(action.allowed_failure, 'only declared host refusals');
        const key = `${action.name}:${effect.status}`; hostStatuses[key] = (hostStatuses[key] ?? 0) + 1;
        if (action.name === 'discard_installed_resident' && effect.status === 0) discards++;
        if (action.name.startsWith('compile_resident') && effect.status === 18) {assert.equal(word(c.arena, 48), 0x1100); assert.equal(c.primaryRetired, 16); assert.equal(c.units.size, 8);}
        c.arena = effect.after;
        if (recorded && event.resolved_unit !== undefined) assert.deepEqual(event.resolved_unit, {low: event.args[2], high: event.args[3], decimal: full(event.args[2], event.args[3]).toString()});
      } else if (action.kind === 'module') {
        const metadata = action.target === 'dispatcher' ? c.dispatcher : c.pending; assert(metadata);
        let row;
        if (recorded) {
          row = recorded.result.modules[moduleIndex]; assert(row);
          assert.equal(row.context, c.spec.id); assert.equal(row.owner, 'resident'); assert.equal(row.entries, c.spec.entries);
          for (const field of ['target', 'group', 'binding', 'slot', 'file']) assert.deepEqual(row[field], action[field]);
          assert.equal(row.pointer, metadata.pointer); assert.equal(row.bytes_length, metadata.bytes_length); assert.equal(row.generation, null);
          assert.equal(row.id_low, metadata.low); assert.equal(row.id_high, metadata.high);
          allocationBounds(c, metadata);
          const bytes = pinnedFile(recorded.directory, row); const m = generatedInterface(bytes, row.target, c.spec.key, metadata);
          assert.deepEqual(row.imports, m.imports.map(({module, name, kind}) => ({module, name, kind: ['function', 'table', 'memory', 'global'][kind]})));
          assert.deepEqual(row.exports, m.exports.map(({name, kind}) => ({name, kind: ['function', 'table', 'memory', 'global'][kind]})));
          assert.equal(event.module_file, row.file); assert.equal(event.module_copy_sha256, row.sha256);
        } else row = {...action, context: c.spec.id, pointer: metadata.pointer, bytes_length: metadata.bytes_length, sha256: '0'.repeat(64), id_low: metadata.low, id_high: metadata.high};
        metadata.file = action.file; metadata.sha256 = row.sha256; metadata.group = action.group; c.live.set(action.binding, metadata);
        if (action.target === 'child') {assert(!c.cache.has(action.binding)); c.cache.set(action.binding, metadata);} journal.push({kind: 'module_file', ...row}); moduleIndex++;
      } else if (action.kind === 'table') {
        assert(action.slot >= 0 && action.slot < 8);
        if (action.expected_binding !== null) assert.equal(c.table[action.slot], action.expected_binding);
        if (action.binding !== null) assert(c.cache.has(action.binding)); c.table[action.slot] = action.binding;
        if (recorded) assert.equal(event.reference_equal, true);
      } else if (action.kind === 'call') {
        const effect = callEffect(c, action, program); generated++;
        const key = `${action.channel}:${effect.status}`; statuses[key] = (statuses[key] ?? 0) + 1;
        if (recorded) {assert.equal(event.status, effect.status); assert.deepEqual(event.args, [c.base, c.base + 56, action.budget, c.base + 96]); assert.equal(event.completed_calls, 1);}
        if (effect.retired) {positive++; retired += effect.retired; c.retired += effect.retired;}
        if (effect.retired) {
          assert.equal(word(effect.after, 72), 3);
          if (action.channel === 'dispatch') {
            const first = !c.primaryRetired; assert.equal(action.budget, first ? 17 : 3); assert.equal(effect.retired, first ? 16 : 2);
            assert.equal(word(effect.after, 60), 0x10003); c.primaryRetired = (c.primaryRetired ?? 0) + effect.retired;
            assert.equal(c.primaryRetired, first ? 16 : 18); assert.equal(word(effect.after, 48), first ? 0x1100 : 0x1120);
          } else {assert.equal(action.budget, 3); assert.equal(effect.retired, 2); assert.equal(word(effect.after, 60), 0x10001);}
        }
        c.arena = effect.after;
      } else if (action.kind === 'close') {
        if (recorded) {assert.equal(event.status, 0); assert.deepEqual(event.reference_snapshot, refSnapshot(c)); assert.equal(event.after, null);}
        assert.equal(c.retired, 22); assert.equal(c.units.size, 8); assert.equal(c.nextId, 11n); assert.equal(c.fullInputs, 1); assert.equal(c.gapInputs, 1); c.closed = true;
      } else assert.fail('unmodeled action');
      if (!c.closed) {frame(c, action, event, 'after'); if (recorded) assert.deepEqual(event.reference_snapshot, refSnapshot(c), `whole live/cache/table/ack action${action.id}`);}
    }
    journal.push({kind: 'event', event});
    if (['call', 'closed_call', 'closed_host', 'close'].includes(action.kind) || action.kind === 'host' && action.name === 'discard_installed_resident') {
      checks.push({index: checkpoints++, events: action.id + 1, frames: frameIndex, raw_bytes: frameIndex * SIZE, last_event: action.id});
    }
  }
  checks.push({index: checkpoints++, events: plan.actions.length, frames: frameIndex, raw_bytes: frameIndex * SIZE, last_event: plan.actions.at(-1).id});
  assert.deepEqual({generated, positive, retired, discards, frameIndex, moduleIndex, checkpoints}, {generated: 26, positive: 8, retired: 44, discards: 4, frameIndex: 380, moduleIndex: 22, checkpoints: 57});
  assert.deepEqual(statuses, {'dispatch:0': 6, 'direct:3': 10, 'direct:0': 4, 'closed:5': 6});
  if (recorded) {assert.deepEqual(recorded.result.observed_statuses, statuses); assert.deepEqual(ndjson(pinnedFile(recorded.directory, recorded.result.journal)), journal);}
  return {generated, positive, retired, discards, frames: frameIndex, modules: moduleIndex, checkpoints, statuses, hostStatuses, checks, journal};
}

function filePin(bytes) {return {bytes: bytes.length, sha256: hash(bytes)};}
function readPinned(root, path, expected) {
  const p = resolve(root, path); assert(lstatSync(p).isFile() && !lstatSync(p).isSymbolicLink()); assert.equal(realpathSync(p), p);
  const bytes = readFileSync(p); assert.deepEqual(filePin(bytes), {bytes: expected.bytes, sha256: expected.sha256}, `authority ${path}`); return bytes;
}
function authority(root, freezePath, actualDirectory) {
  const freezeBytes = readFileSync(freezePath), freeze = JSON.parse(freezeBytes);
  assert.equal(freeze.status, 'source-reviewed');
  for (const name of ['source_pins', 'protected_artifacts', 'historical_artifacts']) {
    assert(freeze[name] && Object.keys(freeze[name]).length > 0);
    for (const [path, expected] of Object.entries(freeze[name])) readPinned(root, path, expected);
  }
  const engineBytes = readPinned(root, freeze.engine_path, freeze.engine);
  engineInterface(engineBytes, readPinned(root, freeze.previous_engine_path, freeze.previous_engine));
  const planBytes = readPinned(root, freeze.plan_path, freeze.plan), plan = JSON.parse(planBytes); validatePlan(plan);
  assert.deepEqual(plan.counts, freeze.counts);
  assert(Array.isArray(freeze.native_proofs) && freeze.native_proofs.length === 4);
  for (const proof of [...freeze.native_proofs, freeze.build_proof]) assert.equal(JSON.parse(readPinned(root, proof.path, proof)).ack, true);
  assert(Array.isArray(freeze.reviews) && freeze.reviews.length === 3);
  for (const p of freeze.reviews) {assert.equal(p.ack, true); const review = JSON.parse(readPinned(root, p.index, p)); assert.equal(review.ack, true); assert.deepEqual(review.survivors, []); pinSubset(review.source_pins, freeze.source_pins);}
  const supervisorPath = resolve(root, freeze.actual_supervision_freeze_path), supervisedBytes = readFileSync(supervisorPath), supervised = JSON.parse(supervisedBytes);
  assert(supervised.ack); assert.equal(supervised.deadline_seconds, 600);
  const ownRelative = relative(root, resolve(freezePath)); assert.deepEqual(supervised.artifacts[ownRelative], filePin(freezeBytes));
  assert.deepEqual(supervised.supervisor, freeze.source_pins['target/installed-resident-disposal-session/supervise.py']);
  const supervisedPins = {...supervised.sources, ...supervised.artifacts};
  pinSubset(freeze.source_pins, supervisedPins);
  assert.deepEqual(supervisedPins[relative(root, resolve(root, freeze.engine_path))], freeze.engine);
  for (const [name, p] of Object.entries(freeze.capture_pins)) assert.deepEqual(supervisedPins[relative(root, resolve(root, freeze.capture_path, name))], p);
  for (const [path, p] of Object.entries(supervisedPins)) readPinned(root, path, p);
  const bank = dirname(supervisorPath), started = JSON.parse(readFileSync(join(bank, 'actual-first.started.json'))), actual = JSON.parse(readFileSync(join(bank, 'actual-first.result.json')));
  assert.deepEqual(actual.freeze, filePin(supervisedBytes)); assert.deepEqual(started.freeze, actual.freeze); assert.deepEqual(actual.command, supervised.command); assert.deepEqual(started.command, actual.command);
  assert.deepEqual(actual.command, ['node', resolve(root, 'engine/tests/fixtures/p2-installed-resident-disposal/run.mjs'), resolve(root, freeze.engine_path), resolve(actualDirectory), resolve(root), resolve(root, freeze.capture_path)]);
  assert.equal(actual.exit_code, 0); assert.equal(actual.timeout, false); assert.equal(actual.not_started, false); assert.equal(actual.error, null); assert.equal(actual.reaped, true);
  assert.deepEqual(actual.changed_pins, []); assert.deepEqual(actual.pin_errors, []); assert.equal(actual.deadline_seconds, 600); assert.equal(actual.cleanup_reserve_seconds, 5);
  assert(Number.isSafeInteger(actual.wall_ns) && actual.wall_ns > 0 && actual.wall_ns <= 600e9);
  readPinned(bank, 'actual-first.stdout', actual.stdout); readPinned(bank, 'actual-first.stderr', actual.stderr);
  assert.deepEqual(supervised.environment, {NODE_OPTIONS: '', RING3_ENGINE_SHA256: freeze.engine.sha256, RING3_INSTALLED_RESIDENT_DISPOSAL_FREEZE: resolve(freezePath)});
  return {freeze, plan, planBytes, actual, bank, engineExports: signatures(engineBytes).exports.map(({name, kind}) => ({name, kind: ['function', 'table', 'memory', 'global', 'tag'][kind]}))};
}
function verifyCapsule(directory, root, freezePath) {
  directory = resolve(directory); root = resolve(root); freezePath = resolve(freezePath);
  const {freeze, plan, planBytes, engineExports} = authority(root, freezePath, directory);
  const result = JSON.parse(physicalFile(directory, 'result.json')), manifest = JSON.parse(physicalFile(directory, 'manifest.json'));
  assert.equal(result.status, 'complete'); assert.equal(result.failure, null);
  const original = {...result}; for (const key of ['failure', 'source_pins_after', 'contexts', 'events', 'modules', 'frames', 'observed_counts', 'observed_statuses', 'raw', 'journal', 'checkpoints']) delete original[key]; original.status = 'recording';
  assert.deepEqual(original, manifest, 'initial manifest preserved');
  assert.deepEqual(result.engine, freeze.engine); assert.deepEqual(result.engine_imports, []); assert.deepEqual(result.engine_exports, engineExports);
  assert.deepEqual(result.plan, {file: 'plan.json', sha256: freeze.plan.sha256}); equalBytes(physicalFile(directory, 'plan.json'), planBytes, 'physical plan');
  const sourceRows = Object.entries(freeze.source_pins).sort(([a], [b]) => a.localeCompare(b)).map(([path, p]) => ({path, ...p}));
  // Recorder sorts by JS code point; pin comparison uses that same explicit order.
  sourceRows.sort((a, b) => a.path < b.path ? -1 : a.path > b.path ? 1 : 0);
  assert.deepEqual(result.source_pins_before, sourceRows); assert.deepEqual(result.source_pins_after, sourceRows);
  assert.deepEqual(result.planned_counts, plan.counts);
  const captures = Object.keys(freeze.capture_pins).sort(); assert.deepEqual(captures, ['authored-arena.bin', 'bank.x86', 'capture.json', 'initial-arena.bin']);
  assert.deepEqual(result.capture_pins, captures.map(file => ({file, ...freeze.capture_pins[file]})));
  for (const file of captures) equalBytes(physicalFile(directory, file), readPinned(root, join(freeze.capture_path, file), freeze.capture_pins[file]), `native copied ${file}`);
  equalBytes(physicalFile(directory, 'bank.x86'), hex(plan.bank.hex), 'raw program input');
  equalBytes(physicalFile(directory, 'initial-arena.bin'), defaultArena(), 'raw default input');
  equalBytes(physicalFile(directory, 'authored-arena.bin'), authoredArena(), 'raw authored input');
  assert.deepEqual(JSON.parse(physicalFile(directory, 'capture.json')), {schema_version: 1, raw_inputs_only: true, groups: plan.bank.groups, files: [{file: 'bank.x86', bytes: 4096}, {file: 'initial-arena.bin', bytes: SIZE}, {file: 'authored-arena.bin', bytes: SIZE}]});
  assert.equal(result.events.length, 200); assert.equal(result.frames.length, 380); assert.equal(result.modules.length, 22); assert.equal(result.contexts.length, 2);
  for (let i = 0; i < 2; i++) {
    const spec = plan.contexts[i], open = result.events.find(e => e.context === spec.id && e.kind === 'open');
    assert.deepEqual(result.contexts[i], {...spec, base: open.arena_ptr, memory_bytes: open.memory_bytes_after, open_event: open.action_id});
  }
  const raw = pinnedFile(directory, result.raw); assert.equal(result.raw.file, 'arenas.bin'); assert.equal(raw.length, 380 * SIZE);
  assert.equal(result.journal.file, 'journal.ndjson'); assert.equal(result.checkpoints.file, 'checkpoints.ndjson');
  assert.equal(result.journal.rows, 602); assert.equal(result.checkpoints.rows, 57);
  const model = replay(plan, {directory, result, raw});
  assert.deepEqual(result.observed_counts, {contexts: 2, engine_instances: 2, opened: 2, closed: 2, generated_calls: 26, host_api_calls: 90, inputs: 36, tables: 28, child_modules: 20, dispatchers: 2, successful_discards: 4, events: 200, frames: 380});
  const journalBytes = pinnedFile(directory, result.journal), checkpointRows = ndjson(pinnedFile(directory, result.checkpoints));
  assert.equal(checkpointRows.length, model.checks.length);
  const journalLines = journalBytes.toString().split('\n').slice(0, -1); let at = 0, journalEvents = 0, journalFrames = 0;
  const eventOffsets = new Map();
  for (const line of journalLines) {const row = JSON.parse(line); at += Buffer.byteLength(line) + 1; if (row.kind === 'frame') journalFrames++; if (row.kind === 'event') {journalEvents++; eventOffsets.set(journalEvents, {bytes: at, frames: journalFrames});}}
  for (let i = 0; i < checkpointRows.length; i++) {
    const row = checkpointRows[i], expected = model.checks[i];
    for (const [key, value] of Object.entries(expected)) assert.equal(row[key], value, `checkpoint${i}/${key}`);
    assert.equal(row.journal_bytes, eventOffsets.get(row.events).bytes); assert.equal(row.frames, eventOffsets.get(row.events).frames);
    const prefix = result.events.slice(0, row.events);
    const count = predicate => prefix.filter(predicate).length;
    assert.deepEqual(row.observed_counts, {contexts: count(e => e.kind === 'open'), engine_instances: count(e => e.kind === 'open'), opened: count(e => e.kind === 'open'), closed: count(e => e.kind === 'close'), generated_calls: count(e => ['call', 'closed_call'].includes(e.kind)), host_api_calls: count(e => ['host', 'closed_host', 'close'].includes(e.kind)) + 2 * count(e => e.kind === 'open'), inputs: count(e => e.kind === 'input'), tables: count(e => e.kind === 'table'), child_modules: count(e => e.kind === 'module' && e.target === 'child'), dispatchers: count(e => e.kind === 'module' && e.target === 'dispatcher'), successful_discards: count(e => e.kind === 'host' && e.name === 'discard_installed_resident' && e.status === 0), events: row.events, frames: row.frames});
  }
  const expectedFiles = ['plan.json', 'bank.x86', 'initial-arena.bin', 'authored-arena.bin', 'capture.json', 'arenas.bin', 'journal.ndjson', 'checkpoints.ndjson', 'manifest.json', 'result.json', ...result.modules.map(m => m.file)].sort();
  assert.deepEqual(readdirSync(directory).sort(), expectedFiles); assert.equal(expectedFiles.length, 32);
  return {ack: true, phase: 'FIRST offline full semantic and physical acceptance', generated_calls: model.generated, positive_calls: model.positive, retired: model.retired, discarded_units: model.discards, modules: model.modules, frames: model.frames, raw_bytes: raw.length, journal_rows: result.journal.rows, checkpoints: model.checkpoints, physical_files: expectedFiles.length, raw: filePin(raw), result: filePin(physicalFile(directory, 'result.json'))};
}

function unsignedLeb(n) {assert(Number.isInteger(n) && n >= 0); const result = []; do {const b = n % 128; n = Math.floor(n / 128); result.push(b | (n ? 128 : 0));} while (n); return result;}
const vector = rows => Buffer.concat([Buffer.from(unsignedLeb(rows.length)), ...rows.map(row => Buffer.from(row))]);
const textBytes = text => {const b = Buffer.from(text); return Buffer.concat([Buffer.from(unsignedLeb(b.length)), b]);};
const section = (id, body) => Buffer.concat([Buffer.from([id, ...unsignedLeb(body.length)]), body]);
function syntheticModule(target, key, unit) {
  const dispatch = target === 'dispatcher', lengths = dispatch ? [4, 5, 3] : [4, 7];
  const types = vector(lengths.map(n => Buffer.concat([Buffer.from([0x60]), vector(Array(n).fill([0x7f])), vector([[0x7f]])])));
  const imported = (module, name, tail) => Buffer.concat([textBytes(module), textBytes(name), Buffer.from(tail)]);
  const rows = [imported('env', 'memory', [2, 0, 1])];
  if (dispatch) rows.push(imported('env', 'table', [1, 0x70, 1, 8, 8]));
  rows.push(imported('ring3', dispatch ? 'guard_dispatch_entry' : 'guard_resident', [0, 1]));
  if (dispatch) rows.push(imported('ring3', 'find_installed_resident', [0, 2]));
  const identity = dispatch ? key : [...key, unit.low, unit.high];
  const prefix = identity.flatMap(n => [0x41, ...signedLeb(n)]).concat([0x20, 0, 0x20, 1, 0x20, 3, 0x10, 0]);
  const body = Buffer.concat([vector(dispatch ? [[5, 0x7f]] : [[16, 0x7f], [1, 0x7e]]), Buffer.from([...prefix, 0x1a, 0x41, 0, 0x0b])]);
  return Buffer.concat([Buffer.from([0, 97, 115, 109, 1, 0, 0, 0]), section(1, types), section(2, vector(rows)), section(3, vector([[0]])), section(7, vector([Buffer.concat([textBytes('run'), Buffer.from([0, dispatch ? 2 : 1])])])), section(10, vector([Buffer.concat([Buffer.from(unsignedLeb(body.length)), body])]))]);
}
function syntheticEngine(arity = null) {
  const names = ['ring3_abi_v1_open']; if (arity !== null) names.push('ring3_abi_v1_discard_installed_resident');
  const params = [3]; if (arity !== null) params.push(arity);
  const types = vector(params.map(n => Buffer.concat([Buffer.from([0x60]), vector(Array(n).fill([0x7f])), vector([[0x7f]])])));
  const bodies = vector(names.map(() => Buffer.from([4, 0, 0x41, 0, 0x0b])));
  return Buffer.concat([Buffer.from([0, 97, 115, 109, 1, 0, 0, 0]), section(1, types), section(3, vector(names.map((_, i) => [i]))), section(7, vector(names.map((name, i) => Buffer.concat([textBytes(name), Buffer.from([0, i])])))), section(10, bodies)]);
}
function selfTest(directory) {
  mkdirSync(directory); const controls = [], rejected = [];
  const test = (name, fn) => {fn(); controls.push(name);};
  const reject = (name, fn) => {assert.throws(fn, undefined, name); rejected.push(name);};
  const bank = Buffer.alloc(4096, 0xcc); for (let i = 0; i < 9; i++) bank.set([0x8d, 0x40, i + 1, 0xe9, 0x18, 0, 0, 0], i * 32);
  const program = parseProgram(bank), spec = {id: 1, owner: 'resident', entries: false, pages: 1, key: [0x47700001, 0x44495350]};
  const seeded = (count = 1) => {
    const c = context(spec); c.arena = authoredArena(); c.version = 3;
    for (let i = 0; i < count; i++) {
      const unit = {id: BigInt(i + 1), low: i + 1, high: 0, binding: `${String.fromCharCode(65 + i)}0`, group: String.fromCharCode(65 + i), pc: 0x1000 + i * 32, version: 3, slot: i, pointer: 131072 + i * 8192, bytes_length: 1024};
      c.units.set(unit.binding, unit); c.cache.set(unit.binding, unit); c.live.set(unit.binding, unit); c.slots[i] = unit.binding; c.table[i] = unit.binding;
    }
    return c;
  };
  const args = [...spec.key, 1, 0, 0];
  test('independent default/authored 4364 and raw nine-region graph', () => {assert.equal(defaultArena().length, SIZE); cpuValid(authoredArena()); assert.equal(program.instructions.size, 18); assert(!defaultArena().subarray(FP).equals(authoredArena().subarray(FP)));});
  test('full-key and absent-full-ID precedence over slot/cancellation', () => {
    const c = seeded(); put(c.arena, 96, 1); assert.equal(disposalStatus(c, [args[0], args[1] ^ 1, 0, 0, 8]), 3); assert.equal(disposalStatus(c, [args[0], args[1], 1, 1, 8]), 3);
  });
  test('stale then exact-slot then cancellation ordering', () => {
    const c = seeded(); put(c.arena, 96, 1); assert.equal(disposalStatus(c, [...args.slice(0, 4), 8]), 7); assert.equal(disposalStatus(c, args), 16); c.version++; assert.equal(disposalStatus(c, [...args.slice(0, 4), 8]), 4);
  });
  test('successful removal changes only membership and exact acknowledgement', () => {
    const c = seeded(8), before = Buffer.from(c.arena); c.table[0] = null;
    const action = {kind: 'host', name: 'discard_installed_resident', args, binding: 'A0'}, effect = hostEffect(c, action, {args}, program);
    assert.equal(effect.status, 0); equalBytes(effect.after, before, 'status-only full arena'); assert.equal(c.units.size, 7); assert.equal(c.slots[0], null); assert.equal(c.slots[7], 'H0'); assert(c.cache.has('A0') && !c.live.has('A0'));
    assert.equal(disposalStatus(c, args), 3);
  });
  test('cached removed guard precedes malformed CPU/fuel/cancellation and ABA', () => {
    const c = seeded(); c.units.delete('A0'); c.arena.fill(0xff, 0, 56); put(c.arena, 96, 1);
    for (const fuel of [0, 17]) {const r = childEffect(c, 'A0', c.arena, fuel, program); assert.equal(r.status, 3); equalBytes(r.after, c.arena, 'removed cached neutrality');}
    const fresh = {...c.cache.get('A0'), binding: 'A1', low: 2, id: 2n}; c.units.set('A1', fresh); c.cache.set('A1', fresh); c.slots[0] = 'A1'; assert.equal(childEffect(c, 'A0', c.arena, 0, program).status, 3);
  });
  test('raw direct child computes LEA/JMP and v1 with exact FP/FLAGS', () => {
    const c = seeded(), r = childEffect(c, 'A0', c.arena, 3, program); assert.equal(r.retired, 2); assert.equal(word(r.after, 16), 0xfffffff1); assert.equal(word(r.after, 48), 0x1020); assert.equal(word(r.after, 60), 0x10001); assert.equal(word(r.after, 72), 3); assert.equal(word(r.after, 52), 0xcd7); equalBytes(r.after.subarray(FP), c.arena.subarray(FP), 'allFP');
  });
  test('real-dispatcher algorithm reaches I after sixteen with budget headroom', () => {
    const c = seeded(8), r = callEffect(c, {binding: 'dispatcher', channel: 'dispatch', budget: 17}, program); assert.equal(r.retired, 16); assert.equal(word(r.after, 16), 20); assert.equal(word(r.after, 48), 0x1100); assert.equal(word(r.after, 60), 0x10003); assert.equal(word(r.after, 72), 3); assert.equal(word(r.after, TRANSFER + 16), 8);
  });
  test('closed cached calls are status-only and carry no frame', () => {const c = seeded(); c.closed = true; assert.deepEqual(callEffect(c, {channel: 'direct', binding: 'A0', budget: 3}, program), {after: null, status: 5, retired: 0}); assert.equal(disposalStatus(c, [0, 0, 0, 0, 8]), 5);});
  test('physical generated imports/locals/export and full64 guard prefix', () => {for (const target of ['child', 'dispatcher']) generatedInterface(syntheticModule(target, spec.key, {low: 7, high: 3}), target, spec.key, {low: 7, high: 3});});
  test('physical old ABI signatures and exact new arity5', () => engineInterface(syntheticEngine(5), syntheticEngine()));
  reject('wrong new export arity', () => engineInterface(syntheticEngine(4), syntheticEngine()));
  reject('wrong baked ID high limb', () => generatedInterface(syntheticModule('child', spec.key, {low: 7, high: 3}), 'child', spec.key, {low: 7, high: 4}));
  reject('wrong baked key high limb', () => generatedInterface(syntheticModule('dispatcher', spec.key, null), 'dispatcher', [spec.key[0], spec.key[1] ^ 1], null));
  reject('raw LEA increment mutation', () => {const b = Buffer.from(bank); b[2] ^= 1; parseProgram(b);});
  reject('raw JMP graph mutation', () => {const b = Buffer.from(bank); b[4] ^= 1; parseProgram(b);});
  reject('raw padding mutation', () => {const b = Buffer.from(bank); b[9] = 0; parseProgram(b);});
  const c = seeded(); c.base = 65536; const raw = Buffer.from(c.arena), row = {index: 0, offset: 0, length: SIZE, context: 1, event: 0, side: 'before', memory_bytes: 1048576, sha256: hash(raw)};
  test('physical whole-frame length/hash/offset/context/side and bytes', () => checkFrame(0, c, {id: 0}, 'before', raw, [row], 0));
  for (const at of [16, 140, FP + 119]) reject(`whole-frame coordinated-hash byte mutation at${at}`, () => {const changed = Buffer.from(raw); changed[at] ^= 1; checkFrame(0, c, {id: 0}, 'before', changed, [{...row, sha256: hash(changed)}], 0);});
  reject('frame wrong side', () => checkFrame(0, c, {id: 0}, 'after', raw, [row], 0));
  reject('frame wrong physical offset', () => checkFrame(0, c, {id: 0}, 'before', raw, [{...row, offset: 1}], 0));
  test('physical pinned file', () => {writeFileSync(join(directory, 'control.bin'), raw, {flag: 'wx'}); equalBytes(pinnedFile(directory, {file: 'control.bin', bytes: SIZE, sha256: hash(raw)}), raw, 'control pinnedfile');});
  reject('physical pin SHA mutation', () => pinnedFile(directory, {file: 'control.bin', bytes: SIZE, sha256: '0'.repeat(64)}));
  reject('physical pin length mutation', () => pinnedFile(directory, {file: 'control.bin', bytes: SIZE - 1, sha256: hash(raw)}));
  reject('physical capsule traversal', () => physicalFile(directory, '../control.bin'));
  const sourcePin = {bytes: 1, sha256: 'a'.repeat(64)};
  test('review and supervision owner pins bind the exact frozen source subset', () => pinSubset({'owner.rs': sourcePin}, {'owner.rs': sourcePin, 'other.rs': sourcePin}));
  reject('missing supervised source owner', () => pinSubset({'owner.rs': sourcePin}, {}));
  reject('stale acknowledged source owner', () => pinSubset({'owner.rs': sourcePin}, {'owner.rs': {...sourcePin, bytes: 2}}));
  const allocation = {base: 65536, memoryBytes: 1048576, live: new Map([['keeper', {pointer: 131072, bytes_length: 1024}]])};
  test('disjoint current allocation and expired allocation address reuse', () => {allocationBounds(allocation, {pointer: 132096, bytes_length: 1024}); allocationBounds({...allocation, live: new Map()}, {pointer: 131072, bytes_length: 1024});});
  reject('module aliases current owned arena', () => allocationBounds(allocation, {pointer: 65536, bytes_length: 1024}));
  reject('distinct modules overlap while both allocations remain live', () => allocationBounds(allocation, {pointer: 131073, bytes_length: 1024}));
  test('complete NDJSON rows', () => assert.deepEqual(ndjson(Buffer.from('{"a":1}\n')), [{a: 1}]));
  reject('incomplete journal row', () => ndjson(Buffer.from('{"a":1}')));
  reject('empty journal row', () => ndjson(Buffer.from('\n')));
  return {ack: true, phase: 'pure independent model and physical binder controls', controls, rejected_mutations: rejected, guest_executed: false};
}
function mutationAdmission(plan) {
  const rejected = [], variants = [
    ['initial FP byte', p => {const b = hex(p.initial_hex); b[FP + 40] ^= 1; p.initial_hex = b.toString('hex');}],
    ['authored FP byte', p => {const b = hex(p.authored_hex); b[FP + 40] ^= 1; p.authored_hex = b.toString('hex');}],
    ['bank raw increment', p => {const b = hex(p.bank.hex); b[2] ^= 1; p.bank.hex = b.toString('hex');}],
    ['bank JMP graph', p => {const b = hex(p.bank.hex); b[4] ^= 1; p.bank.hex = b.toString('hex');}],
    ['initial dispatcher budget16 masks finder miss', p => {p.actions.find(a => a.kind === 'call' && a.budget === 17 && a.channel === 'dispatch').budget = 16;}],
    ['resumed dispatcher budget2 masks finder miss', p => {p.actions.find(a => a.kind === 'call' && a.budget === 3 && a.channel === 'dispatch').budget = 2;}],
    ['host clear expects wrong binding', p => {p.actions.find(a => a.kind === 'table' && a.expected_binding === 'A0').expected_binding = 'I0';}],
    ['successful discard uses another slot', p => {p.actions.find(a => a.kind === 'host' && a.name === 'discard_installed_resident' && !a.allowed_failure).args[4] = 7;}],
    ['second disposal uses old removed A0', p => {p.actions.find(a => a.kind === 'host' && a.name === 'discard_installed_resident' && a.binding === 'I0').binding = 'A0';}],
    ['compile wrong literal PC', p => {const a = p.actions.find(a => a.kind === 'input' && a.label === 'A0 compile descriptor'); const b = hex(a.hex); put(b, 0, 0x1010); a.hex = b.toString('hex');}],
    ['reuse a cached binding', p => {p.actions.find(a => a.kind === 'module' && a.binding === 'A1').binding = 'A0';}],
    ['CPU reseed before primary continuation', p => {const a = p.actions.find(a => a.kind === 'input' && a.label === 'I0 compile descriptor'); a.offset = 48;}],
    ['premature terminal close', p => {p.actions.find(a => a.kind === 'call').kind = 'close';}],
    ['omitted action', p => {p.actions.splice(30, 1);}],
  ];
  for (const [name, change] of variants) {const copy = structuredClone(plan); change(copy); assert.throws(() => replay(copy), undefined, name); rejected.push(name);}
  return {ack: true, phase: 'whole-plan deliberate mutation admission', rejected_mutations: rejected, guest_executed: false};
}
function report(path, value) {writeFileSync(resolve(path), JSON.stringify(value, null, 2) + '\n', {flag: 'wx'}); console.log(JSON.stringify(Object.fromEntries(Object.entries(value).filter(([k]) => !['journal', 'checks', 'hostStatuses'].includes(k)))));}
try {
  const [mode, ...args] = process.argv.slice(2);
  if (mode === '--self-test' && args.length === 1) {const out = resolve(args[0]); const r = selfTest(out); report(join(out, 'report.json'), r);}
  else if (mode === '--admit' && args.length === 2) {const plan = JSON.parse(readFileSync(resolve(args[0]))); const r = replay(plan); delete r.journal; delete r.checks; report(args[1], {ack: true, phase: 'whole literal plan pure admission', ...r, guest_executed: false});}
  else if (mode === '--mutations' && args.length === 2) report(args[1], mutationAdmission(JSON.parse(readFileSync(resolve(args[0])))));
  else if (mode === '--offline' && args.length === 4) report(args[3], verifyCapsule(args[0], args[1], args[2]));
  else throw new Error('usage: check.mjs --self-test FRESH_DIR | --admit PLAN_JSON REPORT | --mutations PLAN_JSON REPORT | --offline CAPSULE ROOT FREEZE_JSON REPORT');
} catch (error) {console.error(error.stack ?? String(error)); process.exitCode = 1;}
