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
const CODE = 0x1000, DATA = 0x3000, BUDGET = 1283;
const CODE_BYTES = Buffer.from('31c0b90001000089fec1c005330683c6044975f5', 'hex');
const CUTS = [0, 2, 7, 9, 12, 14, 17, 18, 20];
const SEED_REGISTERS = [0x11223344, 0x22334455, 0x33445566, 0x44556677,
  0x55667788, 0x66778899, 0x778899aa, DATA];

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

function dataPage() {
  let state = 0x6d2b79f5;
  const values = [];
  for (let index = 0; index < 256; index++) {
    state = u32(state ^ (state << 13)); state = u32(state ^ (state >>> 17)); state = u32(state ^ (state << 5));
    values.push(state);
  }
  values.sort((a, b) => a - b);
  const bytes = Buffer.alloc(4096); values.forEach((value, index) => put(bytes, index * 4, value));
  return bytes;
}

function seedBytes() {
  const bytes = Buffer.alloc(140); header(bytes, 0, 'R3ST', 56);
  [...SEED_REGISTERS, CODE, 2].forEach((value, index) => put(bytes, 16 + index * 4, value));
  exitPacket(bytes, 3, 0, 3); header(bytes, 100, 'R3MH', 40); put(bytes, 120, 0xdecafbad);
  return bytes;
}

function initialBytes() {
  const bytes = Buffer.from(Array.from({length: SIZE}, (_, index) => (index * 37 + 19) & 255));
  const seed = seedBytes();
  const oldRegisters = [0xfffff001, 0x12342222, 0x23453333, 0x34564444, 0x45675555, 0x56786666, 0x67897777, 0x789a8888];
  oldRegisters.forEach((value, index) => put(seed, 16 + index * 4, value)); put(seed, 52, 0xcd7); seed.copy(bytes);
  bytes.fill(0, FP); header(bytes, FP, 'R3FP', 128);
  for (const [offset, value] of [[16, 0x27f], [18, 0x2840], [20, 0xaaaa], [22, 0x345], [32, 0x33], [34, 0x2b]]) bytes.writeUInt16LE(value, FP + offset);
  put(bytes, FP + 24, 0x12345678); put(bytes, FP + 28, 0x9abcdef0);
  for (let index = 40; index < 120; index++) bytes[FP + index] = (index * 29 + 7) & 255;
  return bytes;
}

function helper(bytes, fields) {
  bytes.fill(0, 100, 140); header(bytes, 100, 'R3MH', 40);
  fields.forEach((value, index) => put(bytes, 116 + index * 4, value));
}

function commonFlags(result) {
  let parity = 0;
  for (let bit = 0; bit < 8; bit++) parity ^= (result >>> bit) & 1;
  return 2 | (parity === 0 ? 4 : 0) | (result === 0 ? 64 : 0) | (result >>> 31 ? 128 : 0);
}

function arithmeticFlags(left, right, result, flags, subtract, preserveCarry) {
  const overflow = (subtract ? left ^ right : ~(left ^ right)) & (left ^ result);
  const carry = preserveCarry ? flags & 1 : Number(subtract ? left < right : left + right > 0xffffffff);
  return (flags & 0x400) | commonFlags(result) | carry | ((left ^ right ^ result) & 16) | (overflow >>> 31 ? 2048 : 0);
}

function codeCurrent(context) {
  return !context.unit.retired && context.unit.version === context.pages.get(CODE)?.version;
}

function execute(before, budget, context) {
  validateState(before); integer(budget, 'literal instruction budget');
  const after = Buffer.from(before), registers = Array.from({length: 8}, (_, index) => word(before, 16 + index * 4));
  let pc = word(before, 48), flags = word(before, 52), retired = 0, reason = 1, detail = 0, address = 0, access = 0, length = 0;
  while (retired < budget) {
    assert(CUTS.slice(0, -1).includes(pc - CODE), 'literal checksum instruction membership');
    const offset = pc - CODE;
    if (offset === 0) {registers[0] = 0; flags = (flags & 0x400) | commonFlags(0); pc += 2;}
    else if (offset === 2) {registers[1] = 256; pc += 5;}
    else if (offset === 7) {registers[6] = registers[7]; pc += 2;}
    else if (offset === 9) {
      registers[0] = u32((registers[0] << 5) | (registers[0] >>> 27));
      flags = (flags & ~0x801) | (registers[0] & 1); pc += 3;
    } else if (offset === 12) {
      const page = context.pages.get(registers[6] & 0xfffff000), at = registers[6] & 4095;
      if (!page || !(page.permissions & 1)) {
        reason = 5; detail = page ? 2 : 1; address = registers[6]; access = 1; length = 4;
        helper(after, [1, 0, detail, address, access, length]); break;
      }
      assert(at <= 4092, 'authored read remains within data page');
      const value = word(page.bytes, at); helper(after, [0, value, 0, 0, 0, 0]);
      registers[0] = u32(registers[0] ^ value); flags = (flags & 0x400) | commonFlags(registers[0]); pc += 2;
    } else if (offset === 14) {
      const left = registers[6]; registers[6] = u32(left + 4); flags = arithmeticFlags(left, 4, registers[6], flags, false, false); pc += 3;
    } else if (offset === 17) {
      const left = registers[1]; registers[1] = u32(left - 1); flags = arithmeticFlags(left, 1, registers[1], flags, true, true); pc++;
    } else if (offset === 18) pc = flags & 64 ? CODE + 20 : CODE + 9;
    retired++;
  }
  registers.forEach((value, index) => put(after, 16 + index * 4, value)); put(after, 48, pc); put(after, 52, flags);
  exitPacket(after, reason, retired, 2);
  [detail, address, access, length].forEach((value, index) => put(after, 80 + index * 4, value));
  return {after, retired, status_or: 0};
}

function callModel(before, action, context) {
  const after = Buffer.from(before);
  if (action.channel === 'dispatch' && (word(before, 96) !== 0 || action.budget === 0)) {
    exitPacket(after, word(before, 96) ? 2 : 1, 0, 3); return {after, retired: 0, status_or: 0};
  }
  if (!codeCurrent(context)) return {after: Buffer.from(before), retired: 0, status_or: 4};
  assert.equal(word(before, 96), 0, 'fixed checksum inputs are uncancelled');
  if (action.channel === 'dispatch') installation(after, context.unit);
  else assert.equal(action.channel, 'direct');
  return execute(after, action.budget, context);
}

// Independent finite input schedule. Its annotations never drive execution semantics.
function expectedPlan() {
  const processes = []; let actionId = 0, contextId = 0;
  for (let processIndex = 0; processIndex < 3; processIndex++) {
    const contexts = [], actions = [];
    const add = (context, kind, fields = {}) => actions.push({id: actionId++, context: context?.id ?? null, kind, ...fields});
    const input = (context, offset, bytes) => add(context, 'input', {offset, hex: bytes.toString('hex')});
    const host = (context, name, args) => add(context, 'host', {name, args});
    const seed = context => input(context, 0, seedBytes());
    const call = (context, channel, budget, phase, fields = {}) => add(context, 'call', {channel, budget, phase, timed: false, expected_status: 0, expected_retired: budget, ...fields});
    const install = (context, generation, dispatcher) => {
      const cuts = context.blocks === 1 ? [0, 20] : CUTS, descriptors = Buffer.alloc(context.blocks * 8);
      cuts.slice(0, -1).forEach((offset, index) => {put(descriptors, index * 8, CODE + offset); put(descriptors, index * 8 + 4, cuts[index + 1] - offset);});
      input(context, TRANSFER, descriptors); host(context, 'compile_resident', [context.blocks]);
      add(context, 'module', {target: 'child', generation, file: `${context.id}-child-${generation}.wasm`});
      add(context, 'table', {slot: 0}); host(context, 'acknowledge_resident_installation', ['key_low', 'key_high', 'unit_low', 'unit_high', 0]);
      if (dispatcher) {
        host(context, 'dispatcher_module', ['key_low', 'key_high']);
        add(context, 'module', {target: 'dispatcher', generation: 0, file: `${context.id}-dispatcher.wasm`});
      }
    };
    const open = (blocks, role) => {
      const context = {id: ++contextId, blocks, role, pages: 16, key: [0x10000 + contextId, 0x43484b53]}; contexts.push(context);
      add(context, 'open'); input(context, 0, initialBytes()); host(context, 'map', [CODE, 1, 7]);
      input(context, TRANSFER, CODE_BYTES); host(context, 'upload', [CODE, 20]);
      if (role === 'warm') host(context, 'protect', [CODE, 1, 5]);
      host(context, 'map', [DATA, 1, 3]); input(context, TRANSFER, dataPage().subarray(0, 1024));
      host(context, 'upload', [DATA, 1024]); host(context, 'protect', [DATA, 1, 1]);
      install(context, 0, true); seed(context);
      add(context, 'data', {address: DATA, count: 1024, phase: 'initial', file: `${context.id}-data-initial.bin`}); return context;
    };
    const warm = [open(1, 'warm'), open(8, 'warm')];
    for (const context of warm) add(context, 'guard', {count: 1024, phase: 'warm', timed: false});
    const arms = [{context: warm[0], channel: 'direct'}, {context: warm[1], channel: 'direct'},
      {context: warm[0], channel: 'dispatch'}, {context: warm[1], channel: 'dispatch'}];
    const quartet = (round, timed) => {
      for (let position = 0; position < 4; position++) {
        const arm = arms[(round + processIndex + position) % 4]; seed(arm.context);
        call(arm.context, arm.channel, BUDGET, timed ? 'sample' : 'warm', {timed, round, position});
      }
    };
    for (let round = 0; round < 100; round++) quartet(round, false);
    for (let round = 0; round < 32; round++) {
      add(null, 'gc', {round});
      const guards = () => {
        for (let position = 0; position < 2; position++) add(warm[(round + processIndex + position) % 2], 'guard', {count: 1024, phase: 'sample', timed: true, round, position});
      };
      if (round % 2 === 0) guards(); quartet(round, true); if (round % 2 === 1) guards();
    }
    for (const context of warm) {
      seed(context); for (const budget of [3, 1, 1, 3]) call(context, 'direct', budget, 'partial');
      host(context, 'protect', [DATA, 1, 0]); seed(context);
      call(context, 'dispatch', BUDGET, 'fault', {expected_retired: 4});
      call(context, 'dispatch', BUDGET, 'retry', {expected_retired: 0});
      host(context, 'protect', [DATA, 1, 1]); call(context, 'dispatch', 1279, 'repair');
      add(context, 'data', {address: DATA, count: 1024, phase: 'final', file: `${context.id}-data-final.bin`}); add(context, 'close');
    }
    if (processIndex === 2) for (const blocks of [1, 8]) {
      const context = open(blocks, 'stale'); seed(context); host(context, 'write8', [CODE, 0x31]);
      for (const channel of ['direct', 'dispatch']) call(context, channel, BUDGET, 'stale', {expected_status: 4, expected_retired: 0});
      add(context, 'table', {slot: 0, clear: true}); host(context, 'retire_stale_resident', ['key_low', 'key_high', 'unit_low', 'unit_high']);
      install(context, 1, false); seed(context); call(context, 'dispatch', BUDGET, 'replacement');
      add(context, 'data', {address: DATA, count: 1024, phase: 'final', file: `${context.id}-data-final.bin`}); add(context, 'close');
    }
    processes.push({index: processIndex, contexts, actions});
  }
  return processes;
}


// The literal semantic model never imports the producer plan or executes Wasm.
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
  assert.deepEqual(types, dispatcher ? [i32(4), i32(5), i32(3)] : [i32(4), i32(7), i32(1)], 'exact generated types');
  const memory = {module: 'env', name: 'memory', kind: 2, flags: 0, minimum: 1};
  assert.deepEqual(imports, dispatcher ? [memory, {module: 'env', name: 'table', kind: 1, element: 0x70, flags: 1, minimum: 8, maximum: 8}, {module: 'ring3', name: 'guard_dispatch_entry', kind: 0, type: 1}, {module: 'ring3', name: 'find_installed_resident', kind: 0, type: 2}] : [memory, {module: 'ring3', name: 'guard_resident', kind: 0, type: 1}, {module: 'ring3', name: 'read32', kind: 0, type: 2}], 'real guard/finder-only imports');
  const functionsReader = new Reader(sections.get(3));
  assert.deepEqual(functionsReader.vector(r => r.uint()), [0]); functionsReader.end();
  const exportsReader = new Reader(sections.get(7));
  const exports = exportsReader.vector(r => ({name: r.name(), kind: r.byte(), index: r.uint()}));
  assert.deepEqual(exports, [{name: 'run', kind: 0, index: 2}]); exportsReader.end();
  const bodiesReader = new Reader(sections.get(10));
  assert.equal(bodiesReader.uint(), 1);
  const body = new Reader(bodiesReader.take(bodiesReader.uint())); bodiesReader.end();
  assert.deepEqual(body.vector(r => [r.uint(), r.byte()]), dispatcher ? [[5, 0x7f]] : [[16, 0x7f], [1, 0x7e], [6, 0x7f]], 'fixed generated locals');
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
  assert(BigInt(value) <= 60000000000n, 'duration within process deadline');
  return value;
}

function resolvedArgs(context, args) {
  return args.map(value => value === 'key_low' ? context.key[0] : value === 'key_high' ? context.key[1]
    : value === 'unit_low' ? context.unit.low : value === 'unit_high' ? context.unit.high : integer(value, 'literal host argument'));
}

function liveAllocation(pointer, length, memoryBytes) {
  integer(pointer, 'live allocation pointer'); integer(length, 'live allocation length', 65536);
  assert(pointer > 0 && length >= 8 && pointer + length <= memoryBytes, 'bounded live allocation');
}

function hostEffect(before, after, action, event, context) {
  const args = resolvedArgs(context, action.args), wanted = Buffer.from(before);
  assert.deepEqual(event.args, args, 'exact resolved host arguments'); assert.equal(event.status, 0, 'host prerequisite status');
  if (action.name === 'map') {
    const [pc, count, permissions] = args; assert.equal(count, 1); assert(!context.pages.has(pc));
    context.pages.set(pc, {permissions, version: 1, bytes: Buffer.alloc(4096)});
  } else if (action.name === 'upload') {
    const [pc, length] = args, page = context.pages.get(pc); assert(page && (page.permissions & 2));
    const bytes = before.subarray(TRANSFER, TRANSFER + length);
    sameBytes(bytes, pc === CODE ? CODE_BYTES : dataPage().subarray(0, 1024), 'literal uploaded bytes');
    bytes.copy(page.bytes); page.version++;
  } else if (action.name === 'protect') {
    const [pc, count, permissions] = args, page = context.pages.get(pc); assert(page); assert.equal(count, 1);
    page.permissions = permissions; page.version++;
  } else if (action.name === 'compile_resident') {
    assert.equal(args[0], context.blocks); const page = context.pages.get(CODE); assert(page && (page.permissions & 4));
    sameBytes(page.bytes.subarray(0, 20), CODE_BYTES, 'compiled literal checksum bytes');
    const cuts = context.blocks === 1 ? [0, 20] : CUTS, descriptors = Buffer.alloc(context.blocks * 8);
    cuts.slice(0, -1).forEach((offset, index) => {put(descriptors, index * 8, CODE + offset); put(descriptors, index * 8 + 4, cuts[index + 1] - offset);});
    sameBytes(before.subarray(TRANSFER, TRANSFER + descriptors.length), descriptors, 'exact disjoint instruction descriptors');
    const words = Array.from({length: 6}, (_, index) => word(after, TRANSFER + index * 4)); assert.deepEqual(words.slice(0, 2), [1, 24]);
    const unit = {low: words[2], high: words[3], pointer: words[4], bytes_length: words[5], version: page.version,
      slot: 0, generation: context.generations.length, retired: false, installed: false};
    assert(unit.low || unit.high, 'nonzero resident identity');
    assert(!context.generations.some(row => row.low === unit.low && row.high === unit.high), 'replacement has a fresh identity');
    liveAllocation(unit.pointer, unit.bytes_length, event.memory_bytes_after);
    context.generations.push(unit); context.unit = unit;
    words.forEach((value, index) => put(wanted, TRANSFER + index * 4, value));
  } else if (action.name === 'dispatcher_module') {
    const dispatcher = {pointer: word(after, TRANSFER + 24), bytes_length: word(after, TRANSFER + 28), generation: 0};
    liveAllocation(dispatcher.pointer, dispatcher.bytes_length, event.memory_bytes_after); context.dispatcher = dispatcher;
    wanted.fill(0, TRANSFER, TRANSFER + 32); header(wanted, TRANSFER, 'R3DP', 32);
    [...context.key, dispatcher.pointer, dispatcher.bytes_length].forEach((value, index) => put(wanted, TRANSFER + 16 + index * 4, value));
  } else if (action.name === 'acknowledge_resident_installation') {
    assert(codeCurrent(context)); assert.equal(context.table, context.unit.file, 'real current child selected');
    assert.deepEqual(args, [...context.key, context.unit.low, context.unit.high, 0]);
    context.unit.installed = true; installation(wanted, context.unit);
  } else if (action.name === 'write8') {
    assert.equal(context.role, 'stale'); assert.deepEqual(args, [CODE, 0x31]);
    const page = context.pages.get(CODE); assert(page.permissions & 2); assert.equal(page.bytes[0], 0x31);
    page.version++; wanted.fill(0, 100, 140); header(wanted, 100, 'R3MH', 40, 3); put(wanted, 136, 1);
  } else if (action.name === 'retire_stale_resident') {
    assert(!codeCurrent(context)); assert.equal(context.table, null, 'JS table cleared before retirement');
    context.unit.retired = true;
  } else assert.fail(`unknown host transition ${action.name}`);
  if (['compile_resident', 'dispatcher_module', 'acknowledge_resident_installation'].includes(action.name)) {
    const packet = after.subarray(TRANSFER, TRANSFER + (action.name === 'compile_resident' ? 24 : 32));
    assert.deepEqual(event.receipt, {hex: packet.toString('hex'), words: Array.from({length: packet.length / 4}, (_, index) => word(packet, index * 4))}, 'physical receipt copy');
  }
  return wanted;
}

function references(snapshot, context, memoryBytes) {
  assert.equal(snapshot.memory_same, true, 'engine memory reference equality');
  assert.deepEqual(snapshot.table_slots, Array.from({length: 8}, (_, slot) => ({slot,
    unit: slot === 0 && context.table ? 0 : null, generation: slot === 0 && context.table ? context.unit.generation : null, reference_equal: true})), 'real table current/null references');
  const live = [context.unit?.retired ? null : context.unit, context.dispatcher].filter(Boolean);
  assert.deepEqual(snapshot.live_module_bytes, live.map(module => {
    liveAllocation(module.pointer, module.bytes_length, memoryBytes);
    return {file: module.file, pointer: module.pointer, bytes_length: module.bytes_length, sha256: module.sha256, generation: module.generation};
  }), 'only live owner allocations are hashed');
}

function sourcePins(result, campaign) {
  const expected = Object.keys(campaign.source_pins).sort().map(path => ({path, ...campaign.source_pins[path]}));
  assert.equal(expected.length, 98, 'fixed product, fixture and provenance source census');
  assert.deepEqual(result.source_pins_before, expected); assert.deepEqual(result.source_pins_after, expected);
  const root = resolve(dirname(fileURLToPath(import.meta.url)), '../../../..');
  for (const row of expected) {
    const bytes = readFileSync(safeFile(root, row.path)); assert.equal(bytes.length, row.bytes); assert.equal(hash(bytes), row.sha256, `frozen source ${row.path}`);
  }
  for (const name of ['plan.mjs', 'run.mjs', 'check.mjs', 'statistics.mjs', 'measure.py']) {
    assert(expected.some(row => row.path === `engine/tests/fixtures/checksum-attribution/${name}`), 'complete fixture source authority');
  }
  for (const path of ['Cargo.toml', 'Cargo.lock', 'engine/Cargo.toml', 'engine/tests/support/pe32_sort.rs',
    'engine/tests/fixtures/p2-pe32-sort/oracle.mjs', ...['plan.mjs', 'run.mjs', 'check.mjs', 'statistics.mjs', 'measure.py'].map(name => `engine/tests/fixtures/resident-phases/${name}`)]) {
    assert(expected.some(row => row.path === path), 'product/build/dependency source authority');
  }
}

const EXPECTED_COUNTS = {
  processes: 3, contexts: 8, generated_calls: 1632, positive_calls: 1622, zero_retirement_calls: 10,
  expected_instructions: 2042584, guard_calls: 202752, diagnostic_read32_calls: 16384,
  child_modules: 10, dispatcher_modules: 8, timed_samples: 576, explicit_gc_calls: 96,
  actions: 3720, frames: 7232, host_api_calls: 114, module_files: 18, data_files: 16,
  process_physical_files: 52, journal_rows: 10986, checkpoint_rows: 1857,
};
const ENGINE = {bytes: 586988, sha256: '4d5e42c9e4adbcb774827f0a6c99687e5c9aed5b4a5ed34c8348bb4b62cf1bf2'};

function validatePlan(plan) {
  assert.deepEqual(Object.keys(plan).sort(), ['schema_version', 'layout', 'code_hex', 'data_hex', 'cuts', 'counts', 'processes'].sort());
  assert.equal(plan.schema_version, 1);
  assert.deepEqual(plan.layout, {arena_bytes: SIZE, code: CODE, data: DATA, budget: BUDGET});
  sameBytes(hexBytes(plan.code_hex, 'saved raw checksum'), CODE_BYTES, 'literal opcode stream');
  assert.equal(CODE + 20 + CODE_BYTES.readInt8(19), CODE + 9, 'literal backward JNZ target');
  sameBytes(hexBytes(plan.data_hex, 'saved data input'), dataPage().subarray(0, 1024), 'independent sorted xorshift input');
  assert.deepEqual(plan.cuts, CUTS); assert.deepEqual(plan.counts, EXPECTED_COUNTS);
  assert.deepEqual(plan.processes, expectedPlan(), 'independently reconstructed finite input schedule');
}

function timingRecords(result, selected) {
  const planned = selected.actions.filter(action => ['call', 'guard'].includes(action.kind) && action.timed);
  assert.equal(planned.length, 192); assert.equal(result.samples.length, planned.length);
  const events = new Map(result.events.map(event => [event.action_id, event]));
  for (const [index, action] of planned.entries()) {
    const event = events.get(action.id), sample = result.samples[index];
    assert.deepEqual(sample, {process: result.process_index, blocks: event.blocks,
      kind: action.kind === 'guard' ? 'guard' : action.channel, round: action.round,
      ns: duration(event.ns), calls: action.kind === 'guard' ? 1024 : 1,
      status_or: 0, event: action.id, context: action.context}, 'one-to-one physical timed sample binding');
  }
}

export function checkCapsule(directory, campaign) {
  directory = resolve(directory);
  campaign ??= jsonFile(dirname(directory), 'campaign.json');
  const result = jsonFile(directory, 'result.json');
  assert.equal(result.schema_version, 1); assert.equal(result.status, 'complete'); assert.equal(result.failure, null);
  integer(result.process_index, 'process index', 2); assert.equal(result.plan_process, result.process_index);
  assert.deepEqual(result.engine, ENGINE); assert.deepEqual({bytes: campaign.engine.bytes, sha256: campaign.engine.sha256}, ENGINE);
  sourcePins(result, campaign);
  assert.equal(result.plan.file, 'plan.json');
  const planBytes = readFileSync(safeFile(directory, result.plan.file)); assert.equal(hash(planBytes), result.plan.sha256);
  const plan = JSON.parse(planBytes); validatePlan(plan); assert.deepEqual(result.planned_counts, EXPECTED_COUNTS);
  const selected = expectedPlan()[result.process_index];
  const manifest = jsonFile(directory, 'manifest.json');
  assert.equal(manifest.status, 'recording');
  for (const key of Object.keys(manifest).filter(key => key !== 'status')) assert.deepEqual(manifest[key], result[key], `manifest ${key}`);
  assert.deepEqual(result.first_use.imports, [], 'release engine has no host imports');
  duration(result.first_use.engine_read_ns); duration(result.first_use.engine_module_ns);
  const exportNames = result.first_use.exports.map(row => row.name);
  assert.equal(new Set(exportNames).size, exportNames.length, 'unique engine exports');
  for (const name of ['open', 'close', 'arena_ptr', 'map', 'upload', 'protect', 'compile_resident',
    'dispatcher_module', 'acknowledge_resident_installation', 'guard_resident', 'guard_dispatch_entry',
    'find_installed_resident', 'read32', 'write8', 'retire_stale_resident']) {
    assert(result.first_use.exports.some(row => row.name === `ring3_abi_v1_${name}` && row.kind === 'function'), `real engine export ${name}`);
  }
  assert(result.first_use.exports.some(row => row.name === 'memory' && row.kind === 'memory'));
  assert.deepEqual(result.environment.exec_argv, ['--expose-gc'], 'fixed Node arguments');
  assert.equal(result.environment.gc_policy, 'only explicit global per-round GC plan actions; outside timers');
  assert.equal(result.raw.file, 'arenas.bin'); assert.equal(result.journal.file, 'journal.ndjson'); assert.equal(result.checkpoints.file, 'checkpoints.ndjson');
  const raw = pinnedFile(directory, result.raw);
  assert.equal(raw.length, result.frames.length * SIZE, 'full physical arena frames');
  assert.equal(result.events.length, selected.actions.length, 'exact event schedule');
  const metadata = new Map(result.contexts.map(row => [row.id, row])); assert.equal(metadata.size, selected.contexts.length);
  assert.equal(metadata.size, result.contexts.length, 'unique owner metadata');
  const modules = new Map(result.modules.map(row => [row.file, row])); assert.equal(modules.size, result.modules.length);
  const dataFiles = new Map(result.data_files.map(row => [row.file, row])); assert.equal(dataFiles.size, result.data_files.length);
  const consumedModules = new Set(), consumedData = new Set(), contexts = new Map(), snapshots = new Map();
  const counts = {engine_contexts: 0, opened: 0, closed: 0, child_modules: 0, dispatcher_modules: 0,
    generated_calls: 0, guard_calls: 0, diagnostic_read32_calls: 0, host_api_calls: 0,
    input_events: 0, table_events: 0, data_events: 0, events: 0, frames: 0, explicit_gc: 0};
  let cursor = 0, retired = 0, positive = 0, zero = 0;
  const frameAt = (index, event, side, context) => {
    assert.equal(index, cursor++, 'append-only frame order');
    const row = result.frames[index]; assert(row, 'physical frame exists');
    assert.deepEqual({index: row.index, offset: row.offset, length: row.length, context: row.context, event: row.event, side: row.side},
      {index, offset: index * SIZE, length: SIZE, context: context.id, event: event.action_id, side});
    integer(row.memory_bytes, 'memory bytes'); assert.equal(row.memory_bytes % 65536, 0);
    assert(row.memory_bytes >= context.base + SIZE && row.memory_bytes >= context.memory_bytes, 'bounded arena and monotonic owner memory');
    context.memory_bytes = row.memory_bytes; assert.equal(event[`memory_bytes_${side}`], row.memory_bytes);
    const bytes = raw.subarray(index * SIZE, (index + 1) * SIZE); assert.equal(hash(bytes), row.sha256, 'physical frame SHA256');
    return bytes;
  };
  for (const [index, action] of selected.actions.entries()) {
    const event = result.events[index]; assert(!event.error && !event.capture_error, 'complete event');
    assert.equal(event.action_id, action.id); assert.equal(event.context, action.context); assert.equal(event.kind, action.kind);
    for (const [key, value] of Object.entries(action)) if (!['id', 'context', 'kind', 'args'].includes(key)) {
      assert.deepEqual(event[key], value, `literal action field ${key}`);
    }
    counts.events++;
    if (action.kind === 'gc') {
      assert.equal(event.blocks, null); assert.equal(event.before, null); assert.equal(event.after, null); counts.explicit_gc++;
    } else if (action.kind === 'open') {
      assert(!contexts.has(action.context), 'new owner'); const spec = selected.contexts.find(row => row.id === action.context), row = metadata.get(action.context);
      assert(row, 'opened owner metadata');
      assert.deepEqual(Object.fromEntries(Object.keys(spec).map(key => [key, row[key]])), spec);
      integer(row.base, 'owner arena pointer'); assert(row.base > 0 && row.base % 4 === 0);
      assert.equal(row.open_event, action.id); assert.equal(row.close_event, selected.actions.find(a => a.context === spec.id && a.kind === 'close').id);
      const context = {...spec, base: row.base, memory_bytes: row.memory_bytes, pages: new Map(), generations: [], table: null, closed: false};
      contexts.set(spec.id, context); assert.equal(event.blocks, context.blocks);
      assert.equal(event.before, null); assert.equal(event.status, 0); assert.equal(event.arena_ptr, row.base); assert.deepEqual(event.args, [spec.pages, ...spec.key]);
      context.last = frameAt(event.after, event, 'after', context); sameBytes(context.last, defaultArena(), 'canonical newly opened full arena');
      counts.engine_contexts++; counts.opened++; counts.host_api_calls += 2;
    } else {
      const context = contexts.get(action.context); assert(context && !context.closed, 'live owner action'); assert.equal(event.blocks, context.blocks);
      const before = frameAt(event.before, event, 'before', context); sameBytes(before, context.last, 'unmasked per-owner arena chain');
      if ((action.kind === 'table' && action.clear) || (action.kind === 'host' && action.name === 'retire_stale_resident')) {
        references(event.reference_snapshot_before, context, event.memory_bytes_before);
      }
      if (action.kind === 'close') {
        assert.equal(event.after, null, 'no read after owner close'); assert.equal(event.status, 0);
        references(event.reference_snapshot, context, event.memory_bytes_before); context.closed = true; context.last = null;
        counts.closed++; counts.host_api_calls++;
      } else {
        const after = frameAt(event.after, event, 'after', context); let wanted = Buffer.from(before);
        if (action.kind === 'input') {
          const bytes = hexBytes(action.hex, 'authored input'); integer(action.offset, 'input offset', SIZE);
          assert(action.offset + bytes.length <= SIZE); bytes.copy(wanted, action.offset); counts.input_events++;
          validateState(wanted); validateFp(wanted);
        } else if (action.kind === 'host') {
          wanted = hostEffect(before, after, action, event, context); counts.host_api_calls++;
        } else if (action.kind === 'module') {
          const row = modules.get(action.file); assert(row && !consumedModules.has(action.file)); consumedModules.add(action.file);
          const module = action.target === 'child' ? context.unit : context.dispatcher; assert(module);
          assert.equal(event.module_file, action.file);
          assert.deepEqual({context: row.context, target: row.target, generation: row.generation, unit: row.unit, slot: row.slot,
            pointer: row.pointer, bytes_length: row.bytes_length, id_low: row.id_low, id_high: row.id_high},
          {context: context.id, target: action.target, generation: action.generation, unit: action.target === 'child' ? 0 : null,
            slot: action.target === 'child' ? 0 : null, pointer: module.pointer, bytes_length: module.bytes_length,
            id_low: module.low ?? null, id_high: module.high ?? null});
          assert.equal(action.generation, module.generation);
          const bytes = pinnedFile(directory, row); assert.equal(event.module_copy_sha256, row.sha256);
          liveAllocation(row.pointer, row.bytes_length, event.memory_bytes_after);
          const checked = moduleInterface(bytes, action.target, context.key, module);
          assert.deepEqual(row.imports, checked.imports.map(row => ({module: row.module, name: row.name,
            kind: row.kind === 0 ? 'function' : row.kind === 1 ? 'table' : 'memory'})));
          assert.deepEqual(row.exports, [{name: 'run', kind: 'function'}]);
          Object.assign(module, {file: row.file, sha256: row.sha256}); counts[action.target === 'child' ? 'child_modules' : 'dispatcher_modules']++;
        } else if (action.kind === 'table') {
          assert.equal(action.slot, 0); assert.equal(event.reference_equal, true);
          if (action.clear) context.table = null;
          else {assert(context.unit.file && !context.unit.retired); context.table = context.unit.file;}
          counts.table_events++;
        } else if (action.kind === 'call') {
          assert.equal(event.count, 1); assert.equal(event.attempted_calls, 1); assert.equal(event.completed_calls, 1); duration(event.ns);
          assert.deepEqual(event.args, [context.base, context.base + 56, action.budget, context.base + 96]);
          assert(context.unit.installed && context.table === context.unit.file, 'real acknowledged table child');
          const model = callModel(before, action, context); wanted = model.after; assert.equal(event.status_or, model.status_or);
          assert.equal(model.retired, action.expected_retired, 'independent literal retirement matches fixed input intent');
          retired += model.retired; if (model.retired > 0) positive++; else zero++; counts.generated_calls++;
        } else if (action.kind === 'guard') {
          assert(codeCurrent(context) && context.unit.installed && context.table === context.unit.file);
          assert.deepEqual(event.args, [...context.key, context.unit.low, context.unit.high, context.base, context.base + 56, context.base + 96]);
          assert.equal(event.channel, 'guard'); assert.equal(event.attempted_calls, 1024); assert.equal(event.completed_calls, 1024);
          assert.equal(event.status_or, 0); duration(event.ns); counts.guard_calls += 1024;
        } else if (action.kind === 'data') {
          const page = context.pages.get(DATA); assert(page && page.permissions & 1); assert.equal(event.diagnostics.length, 1024);
          for (let i = 0; i < 1024; i++) assert.deepEqual(event.diagnostics[i], {address: DATA + i * 4,
            status: 0, helper_result: 0, value: word(page.bytes, i * 4)}, 'real diagnostic read status, helper result and value');
          const row = dataFiles.get(action.file); assert(row && !consumedData.has(action.file)); consumedData.add(action.file);
          assert.deepEqual(row, {context: context.id, file: action.file, address: DATA, phase: action.phase, bytes: 4096, sha256: hash(page.bytes)});
          sameBytes(pinnedFile(directory, row), page.bytes, 'physical complete data-page read sweep');
          sameBytes(page.bytes, dataPage(), 'sorted input and trailing data-page zeros preserved');
          assert.equal(event.data_file, action.file); assert.equal(event.data_sha256, row.sha256);
          helper(wanted, [0, word(page.bytes, 4092), 0, 0, 0, 0]); counts.diagnostic_read32_calls += 1024; counts.data_events++;
        } else assert.fail(`unmodelled action ${action.kind}`);
        sameBytes(after, wanted, `full 4364-byte arena ${context.id}/${action.id}`); validateState(after); validateFp(after); context.last = after;
        if (['call', 'guard', 'data', 'module', 'table'].includes(action.kind)
            || action.kind === 'host' && action.name === 'retire_stale_resident') references(event.reference_snapshot, context, event.memory_bytes_after);
      }
    }
    counts.frames = cursor; snapshots.set(action.id, {...counts});
  }
  assert.equal(cursor, result.frames.length, 'all frames uniquely consumed');
  assert.equal(consumedModules.size, modules.size, 'all module rows consumed'); assert.equal(consumedData.size, dataFiles.size, 'all data rows consumed');
  assert([...contexts.values()].every(context => context.closed)); assert.deepEqual(result.observed_counts, counts, 'independent owner/API/call census');
  timingRecords(result, selected);
  const journalBytes = pinnedFile(directory, result.journal), journal = ndjson(journalBytes, 'journal'), wantedJournal = [], journalEnds = new Map();
  for (const event of result.events) {
    if (event.before !== null) wantedJournal.push({kind: 'frame', ...result.frames[event.before]});
    if (event.kind === 'module') wantedJournal.push({kind: 'module_file', ...modules.get(event.module_file)});
    if (event.kind === 'data') wantedJournal.push({kind: 'data_file', ...dataFiles.get(event.data_file)});
    if (event.after !== null) wantedJournal.push({kind: 'frame', ...result.frames[event.after]});
    wantedJournal.push({kind: 'event', event});
  }
  assert.deepEqual(journal, wantedJournal, 'exact physical journal order and records'); assert.equal(result.journal.rows, journal.length);
  let journalOffset = 0;
  for (const line of journalBytes.toString('utf8').slice(0, -1).split('\n')) {
    journalOffset += Buffer.byteLength(line) + 1; const row = JSON.parse(line);
    if (row.kind === 'event') journalEnds.set(row.event.action_id, journalOffset);
  }
  const checkpoints = ndjson(pinnedFile(directory, result.checkpoints), 'checkpoints'); assert.equal(result.checkpoints.rows, checkpoints.length);
  const checkpointEvents = selected.actions.filter(action => ['call', 'guard', 'data', 'close'].includes(action.kind)).map(action => action.id).concat(selected.actions.at(-1).id);
  assert.deepEqual(checkpoints.map(row => row.last_event), checkpointEvents, 'exact durable checkpoint schedule');
  for (const [index, row] of checkpoints.entries()) {
    assert.equal(row.index, index); assert.equal(row.process_index, result.process_index);
    const expected = snapshots.get(row.last_event); assert.deepEqual(row.observed_counts, expected);
    assert.equal(row.events, expected.events); assert.equal(row.frames, expected.frames); assert.equal(row.raw_bytes, row.frames * SIZE);
    assert.equal(row.journal_bytes, journalEnds.get(row.last_event), 'exact durable journal boundary');
  }
  const files = ['manifest.json', 'plan.json', 'arenas.bin', 'journal.ndjson', 'checkpoints.ndjson', 'result.json', ...modules.keys(), ...dataFiles.keys()];
  assert.deepEqual(readdirSync(directory).sort(), files.sort(), 'exact physical process file set');
  return {schema_version: 1, process_index: result.process_index, complete: true, semantic_certification: true,
    result_sha256: hash(readFileSync(safeFile(directory, 'result.json'))), raw_sha256: result.raw.sha256, counts,
    individual_generated_witnesses: counts.generated_calls, positive_calls: positive, zero_retirement_calls: zero,
    independently_replayed_instructions: retired, journal_rows: journal.length, checkpoint_rows: checkpoints.length, physical_files: files.length,
    observation_limit: 'each generated call has complete before/after arena frames; guard batches expose status OR and endpoints; data sweeps expose each status/result/value and only final helper'};
}

// An interrupted capsule remains evidence; it is never certified by replaying a partial action.
export function inspectIncomplete(directory, processIndex) {
  directory = resolve(directory); integer(processIndex, 'incomplete process index', 2);
  const inventory = {process_index: processIndex, complete: false, semantic_certification: false,
    files: [], journal_events: 0, individually_hashed_frames: 0, available_full_frames: 0, trailing_frame_bytes: 0};
  if (!existsSync(directory)) return {...inventory, unavailable: true};
  for (const file of readdirSync(directory).sort()) {
    const bytes = readFileSync(safeFile(directory, file)); inventory.files.push({file, bytes: bytes.length, observed_sha256: hash(bytes)});
  }
  const raw = existsSync(join(directory, 'arenas.bin')) ? readFileSync(safeFile(directory, 'arenas.bin')) : Buffer.alloc(0);
  inventory.available_full_frames = Math.floor(raw.length / SIZE); inventory.trailing_frame_bytes = raw.length % SIZE;
  if (!existsSync(join(directory, 'journal.ndjson'))) return inventory;
  const journal = readFileSync(safeFile(directory, 'journal.ndjson'), 'utf8'), end = journal.lastIndexOf('\n');
  const rows = end < 0 ? [] : journal.slice(0, end).split('\n').filter(Boolean).map(line => JSON.parse(line));
  inventory.unfinished_journal_tail_bytes = Buffer.byteLength(journal.slice(end + 1));
  const expected = expectedPlan()[processIndex].actions;
  for (const row of rows) {
    assert(['frame', 'event', 'module_file', 'data_file'].includes(row.kind), 'available journal row kind');
    if (row.kind === 'frame') {
      assert.equal(row.index, inventory.individually_hashed_frames++); assert.equal(row.offset, row.index * SIZE); assert.equal(row.length, SIZE);
      assert(row.offset + SIZE <= raw.length); assert.equal(hash(raw.subarray(row.offset, row.offset + SIZE)), row.sha256);
    } else if (row.kind === 'event') {
      const action = expected[inventory.journal_events++]; assert(action, 'available planned event prefix');
      assert.deepEqual({action_id: row.event.action_id, context: row.event.context, kind: row.event.kind}, {action_id: action.id, context: action.context, kind: action.kind});
    } else pinnedFile(directory, row);
  }
  return inventory;
}

function provenance(campaign) {
  assert.equal(campaign.schema_version, 1); assert(['complete', 'incomplete'].includes(campaign.status));
  assert.equal(campaign.build_policy, 'reuse prior pinned release; no build command');
  assert.deepEqual(campaign.limits, {global_seconds: 600, child_seconds: 60});
  assert.deepEqual({bytes: campaign.engine.bytes, sha256: campaign.engine.sha256}, ENGINE);
  const root = resolve(dirname(fileURLToPath(import.meta.url)), '../../../..');
  assert.equal(campaign.engine.path, join(root, 'target/wasm32-unknown-unknown/release/ring3_engine.wasm'));
  const engine = readFileSync(safeFile(root, relative(root, campaign.engine.path)));
  assert.equal(engine.length, ENGINE.bytes); assert.equal(hash(engine), ENGINE.sha256);
  assert.equal(campaign.prior_campaign.path, join(root, 'target/resident-phase-session/campaign-1/campaign.json'));
  const previousBytes = readFileSync(safeFile(root, relative(root, campaign.prior_campaign.path)));
  assert.equal(previousBytes.length, campaign.prior_campaign.bytes); assert.equal(hash(previousBytes), campaign.prior_campaign.sha256);
  const previous = JSON.parse(previousBytes); assert.equal(previous.status, 'complete'); assert.equal(previous.build.exit_code, 0);
  assert.deepEqual(previous.engine, campaign.engine, 'unchanged prior release');
  const added = ['plan.mjs', 'run.mjs', 'check.mjs', 'statistics.mjs', 'measure.py'].map(name => `engine/tests/fixtures/checksum-attribution/${name}`)
    .concat(['engine/tests/fixtures/p2-pe32-sort/oracle.mjs', 'engine/tests/support/pe32_sort.rs']);
  assert.equal(Object.keys(previous.source_pins).length, 91);
  assert.deepEqual(Object.keys(campaign.source_pins).sort(), [...Object.keys(previous.source_pins), ...added].sort(), 'exact prior91 plus seven task/provenance sources');
  for (const [path, pin] of Object.entries(previous.source_pins)) assert.deepEqual(campaign.source_pins[path], pin, `unchanged prior source ${path}`);
}

export function checkCampaign(directory) {
  directory = resolve(directory); const campaign = jsonFile(directory, 'campaign.json'); provenance(campaign);
  assert(campaign.children.length <= 3); const outcomes = [];
  for (const [index, child] of campaign.children.entries()) {
    assert.equal(child.process_index, index); assert.equal(child.result_file, `process-${index}/result.json`);
    if (child.result_pin) pinnedFile(directory, {file: child.result_file, ...child.result_pin});
    if (child.status === 'complete') {
      assert.equal(child.exit_code, 0); assert.equal(child.timed_out, false); assert(child.result_pin, 'successful child result binding');
      const outcome = checkCapsule(join(directory, `process-${index}`), campaign); assert.equal(outcome.process_index, index); outcomes.push(outcome);
    } else {
      assert.equal(campaign.status, 'incomplete'); outcomes.push(inspectIncomplete(join(directory, `process-${index}`), index));
    }
  }
  const complete = campaign.status === 'complete';
  if (complete) {
    assert.equal(outcomes.length, 3); assert(outcomes.every(row => row.complete));
    const sum = read => outcomes.reduce((total, row) => total + read(row), 0);
    for (const [key, value] of Object.entries({engine_contexts: 8, opened: 8, closed: 8, child_modules: 10,
      dispatcher_modules: 8, generated_calls: 1632, guard_calls: 202752, diagnostic_read32_calls: 16384,
      host_api_calls: 114, events: 3720, frames: 7232, explicit_gc: 96})) assert.equal(sum(row => row.counts[key]), value, `global ${key}`);
    for (const [key, value] of Object.entries({positive_calls: 1622, zero_retirement_calls: 10,
      independently_replayed_instructions: 2042584, journal_rows: 10986, checkpoint_rows: 1857, physical_files: 52})) {
      assert.equal(sum(row => row[key]), value, `global ${key}`);
    }
  }
  return {schema_version: 1, complete, semantic_certification: complete,
    campaign_sha256: hash(readFileSync(safeFile(directory, 'campaign.json'))), processes: outcomes,
    observation_limit: 'all generated calls individually framed; guard batches expose endpoint/status OR; each diagnostic read exposes status/helper result/value, with full arena only at sweep endpoints; timing does not isolate membership, branch dispatch, safepoints or V8 layout'};
}

export function selfTest() {
  const context = {id: 1, blocks: 1, key: [0x10001, 0x43484b53], role: 'warm', pages: new Map([
    [CODE, {permissions: 5, version: 3, bytes: Buffer.concat([CODE_BYTES, Buffer.alloc(4076)])}],
    [DATA, {permissions: 1, version: 4, bytes: dataPage()}]]),
  unit: {low: 17, high: 9, slot: 0, version: 3, installed: true, retired: false}};
  const initial = initialBytes(); seedBytes().copy(initial); validateState(initial); validateFp(initial);
  const direct = callModel(initial, {channel: 'direct', budget: BUDGET}, context);
  const expected = Buffer.from(initial);
  [0x7770f1a0, 0, 0x33445566, 0x44556677, 0x55667788, 0x66778899, 0x3400, DATA, 0x1014, 0x46]
    .forEach((value, index) => put(expected, 16 + index * 4, value));
  exitPacket(expected, 1, 1283, 2); helper(expected, [0, 0xffde566a, 0, 0, 0, 0]);
  sameBytes(direct.after, expected, 'literal full-budget output'); assert.equal(direct.retired, 1283);
  const dispatch = callModel(initial, {channel: 'dispatch', budget: BUDGET}, context), installed = Buffer.from(expected);
  installation(installed, context.unit); sameBytes(dispatch.after, installed, 'literal dispatch publication');
  const partialStates = [[0, 0x100, 0x3000, 0x1009, 0x46, 0xdecafbad],
    [0, 0x100, 0x3000, 0x100c, 0x46, 0xdecafbad],
    [0x018a222e, 0x100, 0x3000, 0x100e, 6, 0x018a222e],
    [0x018a222e, 0xff, 0x3004, 0x1009, 0x16, 0x018a222e]];
  let state = initial;
  for (const [index, budget] of [3, 1, 1, 3].entries()) {
    const result = callModel(state, {channel: 'direct', budget}, context), wanted = partialStates[index];
    assert.deepEqual([16, 20, 40, 48, 52, 120].map(at => word(result.after, at)), wanted);
    assert.equal(word(result.after, 60), 0x10002); assert.equal(word(result.after, 76), budget);
    sameBytes(result.after.subarray(FP), initial.subarray(FP), 'partial FP preservation'); state = result.after;
  }
  context.pages.get(DATA).permissions = 0;
  const fault = callModel(initial, {channel: 'dispatch', budget: BUDGET}, context), faultExpected = Buffer.from(initial);
  [0, 256, 0x33445566, 0x44556677, 0x55667788, 0x66778899, DATA, DATA, 0x100c, 0x46]
    .forEach((value, index) => put(faultExpected, 16 + index * 4, value));
  installation(faultExpected, context.unit); exitPacket(faultExpected, 5, 4, 2);
  [2, DATA, 1, 4].forEach((value, index) => put(faultExpected, 80 + index * 4, value)); helper(faultExpected, [1, 0, 2, DATA, 1, 4]);
  sameBytes(fault.after, faultExpected, 'literal first-read permission fault'); assert.equal(fault.retired, 4);
  const retry = callModel(fault.after, {channel: 'dispatch', budget: BUDGET}, context), retryExpected = Buffer.from(faultExpected);
  put(retryExpected, 76, 0); sameBytes(retry.after, retryExpected, 'unchanged-state retry retires zero'); assert.equal(retry.retired, 0);
  context.pages.get(DATA).permissions = 1;
  const repair = callModel(retry.after, {channel: 'dispatch', budget: 1279}, context), repairExpected = Buffer.from(installed);
  put(repairExpected, 76, 1279); sameBytes(repair.after, repairExpected, 'permission repair continuation'); assert.equal(repair.retired, 1279);
  context.pages.get(CODE).version++;
  const postWrite = Buffer.from(initial); postWrite.fill(0, 100, 140); header(postWrite, 100, 'R3MH', 40, 3); put(postWrite, 136, 1);
  for (const channel of ['direct', 'dispatch']) {
    const stale = callModel(postWrite, {channel, budget: BUDGET}, context); assert.equal(stale.status_or, 4); assert.equal(stale.retired, 0);
    sameBytes(stale.after, postWrite, 'stale post-write whole-arena neutrality');
  }
  // Mutations target real asserted fields and bytes, rather than self-agreement of two models.
  for (const at of [16, 20, 40, 48, 52, 60, 72, 76, 80, 116, 120, 140, FP + 80]) {
    const changed = Buffer.from(installed); changed[at] ^= 1;
    assert.throws(() => sameBytes(changed, installed, `reject literal output mutation ${at}`));
  }
  const processes = expectedPlan(), actions = processes.flatMap(process => process.actions), calls = actions.filter(action => action.kind === 'call');
  assert.equal(actions.length, 3720); assert.equal(calls.length, 1632);
  assert.equal(calls.reduce((sum, action) => sum + action.expected_retired, 0), 2042584);
  assert.equal(actions.reduce((sum, action) => sum + (action.kind === 'gc' ? 0 : ['open', 'close'].includes(action.kind) ? 1 : 2), 0), 7232);
  const plan = {schema_version: 1, layout: {arena_bytes: SIZE, code: CODE, data: DATA, budget: BUDGET},
    code_hex: CODE_BYTES.toString('hex'), data_hex: dataPage().subarray(0, 1024).toString('hex'), cuts: CUTS, counts: EXPECTED_COUNTS, processes};
  validatePlan(plan);
  for (const key of ['code_hex', 'data_hex']) {
    const changed = structuredClone(plan); changed[key] = `00${changed[key].slice(2)}`;
    assert.throws(() => validatePlan(changed), `reject literal ${key} mutation`);
  }
  const changedPlan = structuredClone(plan); changedPlan.processes[0].actions.find(action => action.kind === 'call').budget--;
  assert.throws(() => validatePlan(changedPlan), 'reject schedule budget mutation');
  duration('60000000000'); assert.throws(() => duration('60000000001'));
  return {passed: 26, guest_execution: false, literal_full_partial_fault_retry_repair: true, mutation_rejections: 17};
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (process.argv[2] === '--self-test') console.log(JSON.stringify(selfTest()));
  else {
    assert.equal(process.argv.length, 3, 'usage: node check.mjs CAMPAIGN_OR_CAPSULE | --self-test');
    const directory = resolve(process.argv[2]);
    const receipt = existsSync(join(directory, 'campaign.json')) ? checkCampaign(directory)
      : jsonFile(directory, 'result.json').status === 'complete' ? checkCapsule(directory)
        : inspectIncomplete(directory, jsonFile(directory, 'result.json').process_index);
    console.log(JSON.stringify(receipt, null, 2)); if (!receipt.complete) process.exitCode = 2;
  }
}
