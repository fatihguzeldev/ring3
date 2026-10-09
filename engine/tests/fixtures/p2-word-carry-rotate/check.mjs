import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {lstatSync, readFileSync, readdirSync, realpathSync} from 'node:fs';
import {dirname, isAbsolute, join, relative, resolve} from 'node:path';
import {fileURLToPath} from 'node:url';

const U32 = 2 ** 32, SIZE = 4364, CODE = 0x1000;
const hash = bytes => createHash('sha256').update(bytes).digest('hex');

function uint(value, maximum = U32 - 1) {
  assert.ok(Number.isInteger(value) && value >= 0 && value <= maximum);
  return value;
}

function field(registers, alias, width) {
  const register = width === 8 ? alias % 4 : alias;
  const shift = width === 8 && alias >= 4 ? 8 : 0;
  const parent = uint(registers[register]);
  return {register, shift, parent, value: Math.floor(parent / 2 ** shift) % 2 ** width};
}

function publish(registers, operand, width, value) {
  uint(value, 2 ** width - 1);
  registers[operand.register] = operand.parent + (value - operand.value) * 2 ** operand.shift;
  uint(registers[operand.register]);
}

export function rotateBits(width, direction, value, flags, raw) {
  assert.ok([8, 16, 32].includes(width));
  assert.ok(['left', 'right'].includes(direction));
  uint(value, 2 ** width - 1); uint(flags); uint(raw, 255);
  const masked = raw % 32, distance = width === 32 ? masked : masked % (width + 1);
  if (distance === 0) return {value, flags};
  const original = String(flags % 2) + value.toString(2).padStart(width, '0');
  let ring = original;
  for (let bit = 0; bit < distance; bit++) {
    ring = direction === 'left' ? ring.slice(1) + ring[0] : ring.at(-1) + ring.slice(0, -1);
  }
  const carry = Number(ring[0]);
  const overflow = masked === 1
    ? Number(direction === 'left' ? ring[1] !== ring[0] : ring[1] !== ring[2]) : 0;
  return {value: Number.parseInt(ring.slice(1), 2), flags: ((flags & ~0x801) | carry | overflow * 0x800) >>> 0};
}

export function decodeAt(bytes, offset, pc) {
  assert.ok(Buffer.isBuffer(bytes)); uint(offset, bytes.length - 1); uint(pc);
  const word = bytes[offset] === 0x66, at = offset + Number(word), opcode = bytes[at];
  const available = length => assert.ok(offset + length <= bytes.length, 'truncated authored instruction');
  const finish = (operation, length) => {available(length); return {...operation, pc, length, next: (pc + length) >>> 0};};
  if (word && opcode >= 0xb8 && opcode <= 0xbf) {
    available(4);
    return finish({kind: 'move', width: 16, alias: opcode - 0xb8, value: bytes.readUInt16LE(at + 1)}, 4);
  }
  const carry = [0xd0, 0xd1, 0xd2, 0xd3, 0xc0, 0xc1].includes(opcode);
  if (carry) {
    available(Number(word) + 2);
    const modrm = bytes[at + 1], extension = Math.floor(modrm / 8) % 8;
    assert.ok(modrm >= 0xc0 && [2, 3].includes(extension), 'closed carry operand');
    const width = [0xd0, 0xd2, 0xc0].includes(opcode) ? 8 : word ? 16 : 32;
    assert.ok(!word || width === 16, 'prefixed BYTE carry is closed');
    const immediate = [0xc0, 0xc1].includes(opcode), length = Number(word) + 2 + Number(immediate);
    available(length);
    return finish({kind: 'carry', width, alias: modrm % 8, direction: extension === 2 ? 'left' : 'right',
      count: immediate ? bytes[at + 2] : [0xd2, 0xd3].includes(opcode) ? 'cl' : 1}, length);
  }
  if (word && opcode === 0x83) {
    available(4);
    const modrm = bytes[at + 1];
    assert.ok(modrm >= 0xc0 && Math.floor(modrm / 8) % 8 === 2, 'only authored register ADC');
    const immediate = bytes[at + 2];
    return finish({kind: 'adc', width: 16, alias: modrm % 8,
      value: immediate < 128 ? immediate : immediate + 0xff00}, 4);
  }
  assert.equal(word, false, 'unrecognized WORD or extra-prefix instruction');
  if (opcode === 0x0f) {
    available(2);
    if (bytes[at + 1] === 0x0b) assert.fail(`excluded UD2 at ${pc.toString(16)}`);
    available(3);
    const condition = bytes[at + 1], modrm = bytes[at + 2];
    assert.ok([0x90, 0x92].includes(condition) && modrm >= 0xc0 && modrm < 0xc8, 'only authored SETO/SETC');
    return finish({kind: 'set', width: 8, alias: modrm % 8, flag: condition === 0x90 ? 0x800 : 1}, 3);
  }
  if ([0x70, 0x71, 0xeb].includes(opcode)) {
    available(2);
    const displacement = bytes[at + 1] < 128 ? bytes[at + 1] : bytes[at + 1] - 256;
    return finish({kind: 'branch', condition: opcode === 0xeb ? 'always' : opcode === 0x70 ? 'overflow' : 'no-overflow',
      target: (pc + 2 + displacement) >>> 0}, 2);
  }
  assert.fail(`unrecognized authored opcode at ${pc.toString(16)}`);
}

export function step(instruction, registers, flags) {
  assert.equal(registers.length, 8); registers.forEach(value => uint(value)); uint(flags);
  const output = [...registers];
  let next = instruction.next;
  if (instruction.kind === 'branch') {
    if (instruction.condition === 'always' || ((flags & 0x800) !== 0) === (instruction.condition === 'overflow')) next = instruction.target;
  } else {
    const operand = field(registers, instruction.alias, instruction.width);
    if (instruction.kind === 'carry') {
      const raw = instruction.count === 'cl' ? registers[1] % 256 : instruction.count;
      const result = rotateBits(instruction.width, instruction.direction, operand.value, flags, raw);
      publish(output, operand, instruction.width, result.value); flags = result.flags;
    } else if (instruction.kind === 'move') {
      publish(output, operand, instruction.width, instruction.value);
    } else if (instruction.kind === 'set') {
      publish(output, operand, instruction.width, Number((flags & instruction.flag) !== 0));
    } else if (instruction.kind === 'adc') {
      const left = operand.value, right = instruction.value, carry = flags % 2;
      const total = left + right + carry, value = total % 65536;
      const signed = value => value < 32768 ? value : value - 65536;
      const sum = signed(left) + signed(right) + carry;
      const parity = value.toString(2).slice(-8).split('').filter(bit => bit === '1').length % 2 === 0;
      flags = ((flags & ~0x8d5) | Number(total >= 65536) | Number(parity) * 4
        | Number(left % 16 + right % 16 + carry >= 16) * 0x10 | Number(value === 0) * 0x40
        | Number(value >= 32768) * 0x80 | Number(sum < -32768 || sum > 32767) * 0x800) >>> 0;
      publish(output, operand, instruction.width, value);
    } else assert.fail('unrecognized authored operation');
  }
  return {registers: output, flags, pc: next};
}

function record(magic, size, fields = [], version = 1) {
  const bytes = Buffer.alloc(size); bytes.write(magic);
  bytes.writeUInt32LE(0x10000 + version, 4); bytes.writeUInt32LE(size, 8);
  fields.forEach((value, index) => bytes.writeUInt32LE(uint(value), 16 + index * 4));
  return bytes;
}

export function defaultArena() {
  const arena = Buffer.alloc(SIZE);
  record('R3ST', 56, [0, 0, 0, 0, 0, 0, 0, 0, 0, 2]).copy(arena);
  record('R3EX', 40, [1, 0, 0, 0, 0, 0]).copy(arena, 56);
  record('R3MH', 40, [0, 0, 0, 0, 0, 0]).copy(arena, 100);
  const fp = record('R3FP', 128); fp.writeUInt16LE(0x37f, 16); fp.writeUInt16LE(0xffff, 20);
  fp.copy(arena, 4236); return arena;
}

export function authoredArena() {
  const arena = Buffer.from(Array.from({length: SIZE}, (_, index) => (37 * index + 19) % 256));
  record('R3ST', 56, [0xa1000000, 0xa101a500, 0xa1027fff, 0xa1038000,
    0xa104ffff, 0xa1058001, 0xa1064000, 0xa107aaaa, CODE, 2]).copy(arena);
  record('R3EX', 40, [3, 0, 0, 0, 0, 0], 3).copy(arena, 56);
  arena.writeUInt32LE(0, 96); record('R3MH', 40, [0, 0xdecafbad, 0, 0, 0, 0]).copy(arena, 100);
  const fp = record('R3FP', 128);
  for (const [offset, value] of [[16, 0x27f], [18, 0x2840], [20, 0xaaaa], [22, 0x345], [32, 0x33], [34, 0x2b]]) fp.writeUInt16LE(value, offset);
  fp.writeUInt32LE(0x12345678, 24); fp.writeUInt32LE(0x9abcdef0, 28);
  for (let index = 40; index < 120; index++) fp[index] = (29 * index + 7) % 256;
  fp.copy(arena, 4236); return arena;
}

function state(arena) {
  assert.deepEqual(arena.subarray(0, 16), record('R3ST', 56).subarray(0, 16));
  const flags = arena.readUInt32LE(52);
  assert.equal(flags & 2, 2); assert.equal(flags & ~0xcd7, 0);
  return {registers: Array.from({length: 8}, (_, index) => arena.readUInt32LE(16 + index * 4)),
    flags, pc: arena.readUInt32LE(48)};
}

function groupInstructions(bank, group) {
  const instructions = new Map();
  for (const [pc, length] of group.blocks) {
    let offset = pc - CODE, consumed = 0;
    while (consumed < length) {
      const instruction = decodeAt(bank, offset, pc + consumed);
      assert.ok(consumed + instruction.length <= length, 'whole compiled instruction');
      assert.ok(!instructions.has(instruction.pc)); instructions.set(instruction.pc, instruction);
      consumed += instruction.length; offset += instruction.length;
      if (instruction.kind === 'branch') assert.equal(consumed, length, 'terminator ends compiled block');
    }
    assert.equal(consumed, length);
  }
  assert.equal(instructions.size, group.instructions);
  return instructions;
}

function childRun(arena, instructions, budget) {
  let cpu = state(arena), retired = 0, reason;
  for (;;) {
    if (arena.readUInt32LE(96)) {reason = 2; break;}
    if (retired === budget) {reason = 1; break;}
    const instruction = instructions.get(cpu.pc);
    if (!instruction) {reason = 3; break;}
    cpu = step(instruction, cpu.registers, cpu.flags); retired++;
    assert.ok(retired <= budget, 'finite declared budget');
  }
  record('R3ST', 56, [...cpu.registers, cpu.pc, cpu.flags]).copy(arena);
  record('R3EX', 40, [reason, retired, 0, 0, 0, 0]).copy(arena, 56);
  return {retired, reason};
}

function byteStoreSuccess(arena) {
  record('R3MH', 40, [0, 0, 0, 0, 0, 1], 3).copy(arena, 100);
}

function installation(arena, unit) {
  record('R3IN', 32, [unit.id_low, unit.id_high, unit.slot, 0]).copy(arena, 140);
}

function generated(arena, context, action) {
  if (context.closed) return {status: 5, retired: 0};
  const dispatch = action.channel === 'dispatch';
  if (!dispatch) {
    if (context.stale) return {status: 4, retired: 0};
    return {status: 0, ...childRun(arena, context.units.get(action.group).instructions, action.budget)};
  }
  state(arena); let retired = 0;
  for (;;) {
    if (arena.readUInt32LE(96) || retired === action.budget) {
      const reason = arena.readUInt32LE(96) ? 2 : 1;
      record('R3EX', 40, [reason, retired, 0, 0, 0, 0], 3).copy(arena, 56);
      return {status: 0, retired, reason};
    }
    const pc = arena.readUInt32LE(48);
    const matches = [...context.units.values()].filter(unit => unit.installed && unit.instructions.has(pc));
    assert.ok(matches.length <= 1, 'nonoverlapping installed entries');
    if (matches.length && context.stale) return {status: 4, retired: 0};
    if (!matches.length) {
      record('R3EX', 40, [3, retired, 0, 0, 0, 0], 3).copy(arena, 56);
      return {status: 0, retired, reason: 3};
    }
    installation(arena, matches[0]);
    const child = childRun(arena, matches[0].instructions, action.budget - retired);
    retired += child.retired;
    arena.writeUInt32LE(retired, 76);
    if (child.reason !== 3 || child.retired === 0) return {status: 0, retired, reason: child.reason};
  }
}

export function inspectPlan(plan) {
  assert.equal(plan.schema_version, 1); assert.equal(plan.arena_bytes, SIZE); assert.equal(plan.code_address, CODE);
  const bank = Buffer.from(plan.bank.hex, 'hex'); assert.equal(bank.length, 4096);
  const groups = new Map(plan.bank.groups.map(group => [group.id, group]));
  assert.deepEqual([...groups.keys()], ['main-0', 'main-1', 'main-2', 'consumer', 'entry-forms', 'entry-consumer']);
  const decoded = new Map([...groups].map(([name, group]) => [name, groupInstructions(bank, group)]));
  assert.deepEqual([...decoded.values()].map(instructions => instructions.size), [48, 48, 34, 22, 49, 22]);
  assert.equal(plan.bank.forms.length, 178);
  for (const [index, form] of plan.bank.forms.entries()) {
    assert.equal(form.id, index);
    const instruction = decodeAt(bank, form.pc - CODE, form.pc);
    assert.equal(instruction.kind, 'carry'); assert.equal(instruction.width, 16);
    assert.equal(instruction.alias, form.alias); assert.equal(instruction.direction, form.kind ? 'right' : 'left');
    assert.equal(instruction.length, form.length);
    assert.equal(bank.subarray(form.pc - CODE, form.pc - CODE + form.length).toString('hex'), form.hex);
    assert.equal(instruction.count, form.family === 'cl' ? 'cl' : form.family === 'one' ? 1 : form.raw);
    assert.ok(decoded.get(form.group).has(form.pc));
  }
  const contexts = new Map(); let calls = 0, positive = 0, retired = 0, frames = 0;
  const statuses = {}, labels = {}, instructionCoverage = new Set();
  for (const [index, action] of plan.actions.entries()) {
    assert.equal(action.id, index);
    frames += ['open', 'close'].includes(action.kind) ? 1 : action.kind === 'closed_call' ? 0 : 2;
    if (action.kind === 'open') {
      const spec = plan.contexts.find(context => context.id === action.context); assert.ok(spec);
      contexts.set(spec.id, {...spec, arena: defaultArena(), units: new Map(), stale: false, closed: false});
      continue;
    }
    const context = contexts.get(action.context); assert.ok(context);
    if (action.kind === 'input') {
      const bytes = Buffer.from(action.hex, 'hex');
      if (bytes.length === SIZE) assert.deepEqual(bytes, authoredArena(), 'independently authored whole initial arena');
      bytes.copy(context.arena, action.offset);
    } else if (action.kind === 'module' && action.target === 'child') {
      context.units.set(action.group, {instructions: decoded.get(action.group), id_low: action.slot + 1,
        id_high: 0, slot: action.slot, installed: context.owner !== 'resident'});
    } else if (action.kind === 'table') {
      context.units.get(action.group).installed = true;
    } else if (action.kind === 'host' && action.name === 'write8') {byteStoreSuccess(context.arena); context.stale = true;}
    else if (action.kind === 'close') context.closed = true;
    else if (['call', 'closed_call'].includes(action.kind)) {
      const result = generated(context.arena, context, action);
      calls++; retired += result.retired; positive += Number(result.retired > 0);
      statuses[result.status] = (statuses[result.status] ?? 0) + 1;
      labels[action.label ?? 'closed'] = (labels[action.label ?? 'closed'] ?? 0) + 1;
      if (action.form !== undefined) instructionCoverage.add(action.form);
    }
  }
  assert.equal(contexts.size, 6); assert.equal(calls, 5444); assert.equal(positive, 5422); assert.equal(retired, 5542);
  assert.equal(frames, 21428); assert.equal(instructionCoverage.size, 178);
  return {contexts: 6, calls, positive, zero: calls - positive, retired, frames, statuses, labels};
}

function safeFile(directory, file) {
  assert.equal(typeof file, 'string'); assert.equal(isAbsolute(file), false);
  const path = resolve(directory, file), difference = relative(directory, path);
  assert.ok(difference && !difference.startsWith('..') && !isAbsolute(difference));
  regularFile(path); return path;
}

function regularFile(path) {
  assert.ok(lstatSync(path).isFile(), 'physical regular file');
  assert.equal(realpathSync(path), resolve(path), 'no symlink in physical file path');
  return path;
}

function pin(bytes) {return {bytes: bytes.length, sha256: hash(bytes)};}
function fileRecord(directory, row) {
  const bytes = readFileSync(safeFile(directory, row.file));
  assert.deepEqual(pin(bytes), {bytes: row.bytes, sha256: row.sha256}); return bytes;
}
function ndjson(bytes, rows) {
  assert.equal(bytes.at(-1), 10); const lines = bytes.toString('utf8').slice(0, -1).split('\n');
  assert.equal(lines.length, rows); return lines.map(line => JSON.parse(line));
}

function wasmInterface(bytes) {
  assert.deepEqual(bytes.subarray(0, 8), Buffer.from([0, 97, 115, 109, 1, 0, 0, 0]));
  function reader(bytes) {
    let at = 0;
    const byte = () => {assert.ok(at < bytes.length); return bytes[at++];};
    const leb = () => {
      let value = 0, scale = 1;
      for (let count = 0; count < 5; count++) {
        const b = byte(); value += (b % 128) * scale;
        if (b < 128) {uint(value); return value;} scale *= 128;
      }
      assert.fail('oversize Wasm integer');
    };
    const take = size => {assert.ok(at + size <= bytes.length); const result = bytes.subarray(at, at + size); at += size; return result;};
    const text = () => take(leb()).toString('utf8');
    return {byte, leb, take, text, done: () => assert.equal(at, bytes.length), remaining: () => at < bytes.length};
  }
  const outer = reader(bytes.subarray(8)), sections = new Map();
  while (outer.remaining()) {
    const id = outer.byte(), body = outer.take(outer.leb());
    assert.ok(id === 0 || !sections.has(id)); if (id) sections.set(id, body);
  }
  const types = [], imports = [], exports = [], functions = [];
  if (sections.has(1)) {
    const r = reader(sections.get(1)), count = r.leb();
    for (let i = 0; i < count; i++) {
      assert.equal(r.byte(), 0x60); const parameters = Array.from({length: r.leb()}, () => r.byte());
      const results = Array.from({length: r.leb()}, () => r.byte()); types.push({parameters, results});
    }
    r.done();
  }
  if (sections.has(2)) {
    const r = reader(sections.get(2)), count = r.leb();
    for (let i = 0; i < count; i++) {
      const module = r.text(), name = r.text(), kind = r.byte(), row = {module, name};
      if (kind === 0) {row.kind = 'function'; row.type = r.leb(); functions.push(row.type);}
      else if (kind === 1) {
        row.kind = 'table'; row.element = r.byte(); row.flags = r.leb(); row.minimum = r.leb();
        if (row.flags & 1) row.maximum = r.leb();
      } else if (kind === 2) {
        row.kind = 'memory'; row.flags = r.leb(); row.minimum = r.leb(); if (row.flags & 1) row.maximum = r.leb();
      } else assert.fail('unrecognized authored import');
      imports.push(row);
    }
    r.done();
  }
  if (sections.has(3)) {
    const r = reader(sections.get(3)); for (let n = r.leb(); n; n--) functions.push(r.leb()); r.done();
  }
  if (sections.has(7)) {
    const r = reader(sections.get(7));
    for (let n = r.leb(); n; n--) {
      const name = r.text(), kind = r.byte(), index = r.leb();
      exports.push({name, kind: ['function', 'table', 'memory', 'global'][kind], index});
    }
    r.done();
  }
  return {types, imports, exports, functions, sections: [...sections.keys()]};
}

function moduleInterface(bytes, owner, dispatcher) {
  const parsed = wasmInterface(bytes);
  assert.deepEqual(parsed.sections, [1, 2, 3, 7, 10]);
  const imports = [{module: 'env', name: 'memory', kind: 'memory', flags: 0, minimum: 1}];
  if (dispatcher) {
    imports.push({module: 'env', name: 'table', kind: 'table', element: 0x70, flags: 1, minimum: 8, maximum: 8},
      {module: 'ring3', name: 'guard_dispatch_entry', kind: 'function', type: 1},
      {module: 'ring3', name: 'find_installed_resident', kind: 'function', type: 2});
  } else if (owner !== 'standalone') imports.push({module: 'ring3', name: owner === 'resident' ? 'guard_resident' : 'guard', kind: 'function', type: 1});
  assert.deepEqual(parsed.imports, imports);
  assert.deepEqual(parsed.exports.map(({name, kind}) => ({name, kind})), [{name: 'run', kind: 'function'}]);
  const run = parsed.exports[0]; assert.deepEqual(parsed.types[parsed.functions[run.index]], {parameters: [0x7f, 0x7f, 0x7f, 0x7f], results: [0x7f]});
  for (const imported of parsed.imports.filter(row => row.kind === 'function')) {
    const count = imported.name === 'guard' ? 6 : imported.name === 'guard_resident' ? 7 : imported.name === 'guard_dispatch_entry' ? 5 : 3;
    assert.deepEqual(parsed.types[imported.type], {parameters: Array(count).fill(0x7f), results: [0x7f]});
  }
  return {imports: parsed.imports.map(({module, name, kind}) => ({module, name, kind})),
    exports: parsed.exports.map(({name, kind}) => ({name, kind}))};
}

function compileDescriptors(arena, code, action) {
  const entries = action.name.endsWith('entries'), count = action.args[0];
  uint(count, 8); assert.ok(count > 0);
  const roots = Array.from({length: count}, (_, index) => arena.readUInt32LE(140 + index * (entries ? 4 : 8)));
  const blocks = roots.map((pc, index) => {
    assert.ok(pc >= CODE && pc < CODE + code.length);
    if (!entries) return [pc, arena.readUInt32LE(144 + index * 8)];
    const end = Math.min(CODE + code.length, ...roots.filter(root => root > pc)); let length = 0;
    for (;;) {
      assert.ok(pc + length < end, 'entry must terminate inside authored page');
      const instruction = decodeAt(code, pc + length - CODE, pc + length); length += instruction.length;
      assert.ok(pc + length <= end); if (instruction.kind === 'branch') break;
    }
    return [pc, length];
  });
  for (let i = 0; i < blocks.length; i++) for (let j = 0; j < i; j++) {
    assert.ok(blocks[i][0] + blocks[i][1] <= blocks[j][0] || blocks[j][0] + blocks[j][1] <= blocks[i][0]);
  }
  const instructions = new Map();
  for (const [pc, length] of blocks) {
    let at = 0; assert.ok(length > 0);
    while (at < length) {
      const instruction = decodeAt(code, pc + at - CODE, pc + at); at += instruction.length;
      assert.ok(at <= length); if (instruction.kind === 'branch') assert.equal(at, length);
      instructions.set(instruction.pc, instruction);
    }
  }
  assert.ok(instructions.size <= 64); return {blocks, instructions};
}

function sourceAuthority(freeze, root) {
  assert.equal(freeze.status, 'source-reviewed');
  assert.deepEqual(Object.keys(freeze.engine).sort(), ['bytes', 'sha256']);
  assert.ok(isAbsolute(freeze.engine_path));
  assert.deepEqual(pin(readFileSync(regularFile(freeze.engine_path))), freeze.engine);
  assert.ok(isAbsolute(freeze.plan_path));
  assert.deepEqual(pin(readFileSync(regularFile(freeze.plan_path))), freeze.plan);
  const rows = Object.keys(freeze.source_pins).sort().map(path => {
    assert.deepEqual(pin(readFileSync(safeFile(root, path))), freeze.source_pins[path]);
    return {path, ...freeze.source_pins[path]};
  });
  for (const path of ['plan.mjs', 'run.mjs', 'check.mjs']) assert.ok(rows.some(row => row.path === `engine/tests/fixtures/p2-word-carry-rotate/${path}`));
  assert.ok(rows.some(row => row.path === 'engine/tests/cpu_word_carry_rotate_wasm.rs'));
  return rows;
}

export function checkCapsule(directory, freezePath) {
  directory = resolve(directory); freezePath = resolve(freezePath);
  const root = resolve(dirname(fileURLToPath(import.meta.url)), '../../../..');
  assert.equal(realpathSync(directory), directory); assert.ok(lstatSync(directory).isDirectory());
  const freezeBytes = readFileSync(regularFile(freezePath)), freeze = JSON.parse(freezeBytes), sources = sourceAuthority(freeze, root);
  const resultBytes = readFileSync(safeFile(directory, 'result.json')), result = JSON.parse(resultBytes);
  assert.equal(result.schema_version, 1); assert.equal(result.status, 'complete'); assert.equal(result.failure, null);
  const manifest = JSON.parse(readFileSync(safeFile(directory, 'manifest.json')));
  assert.equal(manifest.status, 'recording');
  for (const key of Object.keys(manifest).filter(key => key !== 'status')) assert.deepEqual(result[key], manifest[key]);
  assert.deepEqual(result.source_pins_before, sources); assert.deepEqual(result.source_pins_after, sources);
  assert.deepEqual(result.engine, freeze.engine);
  const planBytes = readFileSync(safeFile(directory, result.plan.file)); assert.equal(hash(planBytes), result.plan.sha256);
  assert.deepEqual(pin(planBytes), freeze.plan, 'exact pre-execution frozen plan');
  assert.deepEqual(planBytes, readFileSync(freeze.plan_path));
  const plan = JSON.parse(planBytes), prediction = inspectPlan(plan);
  assert.deepEqual(result.planned_counts, plan.counts);
  const bank = readFileSync(safeFile(directory, 'bank.x86')); assert.equal(bank.toString('hex'), plan.bank.hex);
  const captureNames = ['bank.x86', 'initial-arena.bin', 'capture.json', ...plan.bank.groups.map(group => `${group.id}.wasm`)].sort();
  assert.deepEqual(Object.keys(freeze.capture_pins).sort(), captureNames);
  assert.deepEqual(result.capture_pins, captureNames.map(file => ({file, ...freeze.capture_pins[file]})));
  for (const file of captureNames) assert.deepEqual(pin(readFileSync(safeFile(directory, file))), freeze.capture_pins[file]);
  assert.deepEqual(readFileSync(safeFile(directory, 'initial-arena.bin')), defaultArena());
  const capture = JSON.parse(readFileSync(safeFile(directory, 'capture.json')));
  assert.equal(capture.schema_version, 1);
  assert.deepEqual(capture.files.map(row => row.file).sort(), captureNames.filter(file => file !== 'capture.json'));
  for (const row of capture.files) assert.equal(lstatSync(safeFile(directory, row.file)).size, row.bytes);
  const engineInterface = wasmInterface(readFileSync(freeze.engine_path));
  assert.deepEqual(engineInterface.imports, []); assert.deepEqual(result.engine_imports, []);
  assert.deepEqual(result.engine_exports, engineInterface.exports.map(({name, kind}) => ({name, kind})));
  const abi = readFileSync(join(root, 'engine/src/abi/wasm/exports.rs'), 'utf8');
  const expectedExports = ['memory', ...[...abi.matchAll(/export_name = "([^"]+)"/g)].map(match => match[1])].sort();
  assert.deepEqual(result.engine_exports.map(row => row.name).sort(), expectedExports);
  const raw = fileRecord(directory, result.raw), journalBytes = fileRecord(directory, result.journal);
  const journal = ndjson(journalBytes, result.journal.rows), checkpoints = ndjson(fileRecord(directory, result.checkpoints), result.checkpoints.rows);
  assert.equal(raw.length, result.frames.length * SIZE);
  assert.deepEqual(journal.filter(row => row.kind === 'event').map(row => row.event), result.events);
  assert.deepEqual(journal.filter(row => row.kind === 'frame').map(({kind, ...row}) => row), result.frames);
  assert.deepEqual(journal.filter(row => row.kind === 'module_file').map(({kind, ...row}) => row), result.modules);
  const modules = new Map(result.modules.map(row => [row.file, row])); assert.equal(modules.size, 20);
  const files = new Set([...captureNames, 'plan.json', 'manifest.json', 'result.json', 'arenas.bin', 'journal.ndjson', 'checkpoints.ndjson']);
  for (const row of result.modules) {
    const bytes = readFileSync(safeFile(directory, row.file)); files.add(row.file);
    assert.deepEqual(pin(bytes), {bytes: row.bytes_length, sha256: row.sha256});
    assert.deepEqual(moduleInterface(bytes, row.owner, row.target === 'dispatcher'), {imports: row.imports, exports: row.exports});
    if (row.capture) assert.deepEqual(bytes, readFileSync(safeFile(directory, row.capture)));
  }
  assert.equal(files.size, 35); assert.deepEqual(readdirSync(directory).sort(), [...files].sort());
  assert.equal(result.contexts.length, 6); assert.equal(new Set(result.contexts.map(row => row.id)).size, 6);
  const contexts = new Map(), seenModules = new Set(), wantedJournal = [], snapshots = new Map();
  const counts = {contexts: 0, engine_instances: 0, opened: 0, closed: 0, generated_calls: 0, guard_calls: 0,
    host_api_calls: 0, inputs: 0, tables: 0, child_modules: 0, dispatchers: 0, events: 0, frames: 0};
  const statuses = {}; let frameIndex = 0, positive = 0, retired = 0;
  const groups = new Map(plan.bank.groups.map(group => [group.id, group]));
  function frame(event, side, context) {
    const index = event[side]; assert.equal(index, frameIndex);
    const row = result.frames[frameIndex++];
    assert.deepEqual({index: row.index, offset: row.offset, length: row.length, context: row.context, event: row.event, side: row.side},
      {index, offset: index * SIZE, length: SIZE, context: event.context, event: event.action_id, side});
    assert.ok(Number.isInteger(row.memory_bytes) && row.memory_bytes % 65536 === 0 && row.memory_bytes >= context.base + SIZE);
    assert.equal(row.memory_bytes, event[`memory_bytes_${side}`]);
    const bytes = raw.subarray(index * SIZE, (index + 1) * SIZE); assert.equal(hash(bytes), row.sha256);
    assert.deepEqual(bytes, context.arena, `whole arena at ${event.action_id}/${side}`);
    counts.frames++; wantedJournal.push({kind: 'frame', ...row});
    return bytes;
  }
  function refs(event, context, field = 'reference_snapshot') {
    const row = event[field]; assert.ok(row); assert.equal(row.memory_same, true);
    assert.deepEqual(row.table_slots, context.owner === 'resident' ? Array.from({length: 8}, (_, slot) => ({slot, group: context.table[slot] ?? null, reference_equal: true})) : null);
    assert.deepEqual(row.live_module_bytes, [...context.live.values()].map(unit => ({file: unit.file, pointer: unit.pointer, bytes_length: unit.bytes_length, sha256: unit.sha256})));
  }
  function receipt(event, expected) {
    assert.ok(event.receipt); assert.deepEqual(event.receipt.words, expected);
    const bytes = Buffer.alloc(expected.length * 4); expected.forEach((value, index) => bytes.writeUInt32LE(uint(value), index * 4));
    assert.equal(event.receipt.hex, bytes.toString('hex')); return bytes;
  }
  assert.equal(result.events.length, plan.actions.length);
  for (const [index, action] of plan.actions.entries()) {
    const event = result.events[index]; assert.ok(!event.error && !event.capture_error);
    const expectedAction = {...action}; delete expectedAction.id;
    for (const [key, value] of Object.entries(expectedAction)) if (key !== 'args') assert.deepEqual(event[key], value);
    assert.equal(event.action_id, index);
    let context = contexts.get(action.context);
    if (action.kind === 'open') {
      assert.equal(event.before, null); const spec = plan.contexts.find(row => row.id === action.context);
      assert.ok(!context); uint(event.arena_ptr); assert.ok(event.arena_ptr > 0);
      context = {...spec, base: event.arena_ptr, arena: defaultArena(), code: null, units: new Map(), live: new Map(), table: Array(8).fill(null), generation: 0, publication: null, pending: null, stale: false, closed: false};
      contexts.set(spec.id, context); counts.contexts++; counts.opened++;
      assert.deepEqual(event.args, spec.owner === 'standalone' ? [] : [spec.pages, ...spec.key]); assert.equal(event.status, 0);
      if (spec.owner !== 'standalone') {counts.engine_instances++; counts.host_api_calls += 2;}
      else assert.equal(context.base, 128);
      frame(event, 'after', context);
      assert.deepEqual(result.contexts.find(row => row.id === spec.id), {...spec, base: context.base,
        memory_bytes: event.memory_bytes_after, open_event: event.action_id});
    } else if (action.kind === 'closed_call') {
      assert.ok(context.closed); assert.equal(event.before, null); assert.equal(event.after, null);
      assert.deepEqual(event.args, [context.base, context.base + 56, action.budget, context.base + 96]);
      assert.equal(event.status, 5); assert.equal(event.completed_calls, 1); counts.generated_calls++;
      statuses['closed:5'] = (statuses['closed:5'] ?? 0) + 1;
    } else {
      assert.ok(context && !context.closed); frame(event, 'before', context);
      if (action.kind === 'input') {
        const bytes = Buffer.from(action.hex, 'hex'); assert.ok(action.offset + bytes.length <= SIZE);
        if (bytes.length === SIZE) assert.deepEqual(bytes, authoredArena());
        bytes.copy(context.arena, action.offset); counts.inputs++;
        if (action.offset === 0) state(context.arena);
      } else if (action.kind === 'host') {
        const unit = context.units.get(action.group) ?? context.pending;
        const args = action.args.map(value => value === 'key_low' ? context.key[0] : value === 'key_high' ? context.key[1]
          : value === 'unit_low' ? unit.id_low : value === 'unit_high' ? unit.id_high : value);
        assert.deepEqual(event.args, args); counts.host_api_calls++;
        if (action.name === 'map') {
          assert.deepEqual(args, [CODE, 1, 7]); assert.equal(context.code, null); context.code = Buffer.alloc(4096); assert.equal(event.status, 0);
        } else if (action.name === 'upload') {
          assert.deepEqual(args, [CODE, 4096]); assert.ok(context.code);
          context.arena.copy(context.code, 0, 140, 4236); assert.deepEqual(context.code, bank); assert.equal(event.status, 0);
        } else if (action.name.startsWith('compile')) {
          refs(event, context, 'reference_snapshot_before'); let compiled;
          const failed = action.allowed_failure === true;
          if (failed) {
            assert.equal(action.group, 'late-failure'); assert.equal(args[0], 2);
            const entries = action.name.endsWith('entries');
            const first = context.arena.readUInt32LE(140), second = context.arena.readUInt32LE(entries ? 144 : 148);
            assert.equal(first, plan.bank.consumer.first_carry); assert.equal(second, plan.bank.invalid_pc);
            assert.equal(context.code.subarray(first - CODE, first - CODE + 3).toString('hex'), '66d3d1');
            assert.equal(context.code.subarray(second - CODE, second - CODE + 2).toString('hex'), '0f0b');
            if (!entries) {assert.equal(context.arena.readUInt32LE(144), 3); assert.equal(context.arena.readUInt32LE(152), 2);}
            assert.throws(() => compileDescriptors(context.arena, context.code, action),
              /excluded UD2 at 1f00/, 'late candidate reaches exact excluded UD2');
          } else compiled = compileDescriptors(context.arena, context.code, action);
          assert.equal(event.status, failed ? 10 : 0);
          if (context.owner === 'replacement') {
            counts.host_api_calls += 3;
            if (failed) assert.deepEqual(event.publication, context.publication);
            else {
              assert.equal(event.publication.generation, ++context.generation);
              uint(event.publication.pointer); uint(event.publication.bytes_length); assert.ok(event.publication.pointer > 0 && event.publication.bytes_length > 8);
              context.publication = event.publication; context.units.clear(); context.live.clear();
              context.pending = {...event.publication, id_low: null, id_high: null};
            }
          } else if (!failed) {
            const words = event.receipt.words; assert.equal(words.length, 6); assert.deepEqual(words.slice(0, 2), [1, 24]);
            assert.ok(words[2] > 0 || words[3] > 0); assert.ok(words[4] > 0 && words[5] > 8);
            assert.ok([...context.units.values()].every(old => old.id_low !== words[2] || old.id_high !== words[3]));
            receipt(event, words).copy(context.arena, 140);
            context.pending = {generation: null, id_low: words[2], id_high: words[3], pointer: words[4], bytes_length: words[5]};
          }
          if (!failed) {
            assert.deepEqual(compiled.blocks, groups.get(action.group).blocks);
            context.pending = {...context.pending, group: action.group, instructions: compiled.instructions, installed: false};
            context.units.set(action.group, context.pending);
          }
        } else if (action.name === 'dispatcher_module') {
          assert.equal(event.status, 0); const words = event.receipt.words; assert.equal(words.length, 8);
          assert.deepEqual(words.slice(0, 6), [0x50443352, 0x10001, 32, 0, ...context.key]);
          assert.ok(words[6] > 0 && words[7] > 8); receipt(event, words).copy(context.arena, 140);
          context.dispatchMetadata = {pointer: words[6], bytes_length: words[7], generation: null, id_low: null, id_high: null};
        } else if (action.name === 'acknowledge_resident_installation') {
          assert.equal(event.status, 0); const slot = args.at(-1); assert.equal(context.table[slot], action.group);
          assert.deepEqual(args.slice(0, 4), [...context.key, unit.id_low, unit.id_high]);
          unit.slot = slot; unit.installed = true; installation(context.arena, unit);
          receipt(event, Array.from({length: 8}, (_, i) => context.arena.readUInt32LE(140 + i * 4)));
        } else if (action.name === 'write8') {
          assert.equal(event.status, 0); assert.deepEqual(args, [CODE, 0x66]); assert.equal(context.code[0], 0x66);
          context.code[0] = 0x66; byteStoreSuccess(context.arena); context.stale = true;
        } else assert.fail('unrecognized host action');
      } else if (action.kind === 'module') {
        const row = modules.get(action.file); assert.ok(row && !seenModules.has(row.file)); seenModules.add(row.file);
        for (const key of ['context', 'target', 'group', 'slot', 'capture']) assert.deepEqual(row[key], action[key]);
        assert.equal(row.owner, context.owner); assert.equal(row.entries, context.entries);
        const metadata = action.capture ? {pointer: null, generation: null, id_low: null, id_high: null}
          : action.target === 'dispatcher' ? context.dispatchMetadata : context.pending;
        for (const key of ['pointer', 'generation', 'id_low', 'id_high']) assert.deepEqual(row[key], metadata[key]);
        if (!action.capture) assert.equal(row.bytes_length, metadata.bytes_length);
        assert.equal(event.module_file, row.file); assert.equal(event.module_copy_sha256, row.sha256);
        if (row.pointer !== null) assert.ok(row.pointer > 0 && row.pointer + row.bytes_length <= event.memory_bytes_after);
        context.live.set(action.group, row);
        if (action.target === 'child') {
          if (action.capture) context.units.set(action.group, {instructions: groupInstructions(bank, groups.get(action.group)), installed: true});
          counts.child_modules++;
        } else counts.dispatchers++;
        wantedJournal.push({kind: 'module_file', ...row});
      } else if (action.kind === 'table') {
        assert.ok(context.units.has(action.group)); uint(action.slot, 7); context.table[action.slot] = action.group;
        assert.equal(event.reference_equal, true); counts.tables++;
      } else if (action.kind === 'guard') {
        const unit = context.units.get(action.group), key = [context.key[0] ^ 1, context.key[1]];
        const name = context.owner === 'resident' ? 'guard_resident' : 'guard'; assert.equal(event.name, name);
        assert.deepEqual(event.args, context.owner === 'resident' ? [...key, unit.id_low, unit.id_high, context.base, context.base + 56, context.base + 96]
          : [...key, unit.generation, context.base, context.base + 56, context.base + 96]);
        assert.equal(event.status, 3); counts.guard_calls++; counts.host_api_calls++;
      } else if (action.kind === 'call') {
        assert.deepEqual(event.args, [context.base, context.base + 56, action.budget, context.base + 96]);
        const output = generated(context.arena, context, action); assert.equal(event.status, output.status);
        assert.equal(event.completed_calls, 1); counts.generated_calls++; retired += output.retired; positive += Number(output.retired > 0);
        const name = `${action.channel}:${output.status}`; statuses[name] = (statuses[name] ?? 0) + 1;
      } else if (action.kind === 'close') {
        refs(event, context); assert.equal(event.status, 0); context.closed = true; counts.closed++;
        if (context.owner !== 'standalone') counts.host_api_calls++;
      } else assert.fail('unrecognized finite action');
      if (context.closed) assert.equal(event.after, null);
      else {
        frame(event, 'after', context);
        if (['module', 'table', 'call', 'guard', 'host'].includes(action.kind)) refs(event, context);
      }
    }
    counts.events++; wantedJournal.push({kind: 'event', event}); snapshots.set(event.action_id, {...counts});
  }
  assert.deepEqual(journal, wantedJournal); assert.equal(frameIndex, result.frames.length);
  assert.equal(seenModules.size, 20); assert.ok([...contexts.values()].every(context => context.closed));
  assert.deepEqual(result.observed_counts, counts); assert.deepEqual(result.observed_statuses, statuses);
  assert.equal(positive, prediction.positive); assert.equal(retired, prediction.retired);
  assert.equal(counts.host_api_calls, 76); assert.equal(counts.inputs, 5202); assert.equal(journal.length, 32172);
  const expectedCheckpoints = plan.actions.filter(action => ['call', 'closed_call', 'guard', 'close'].includes(action.kind)).map(action => action.id).concat(plan.actions.at(-1).id);
  assert.deepEqual(checkpoints.map(row => row.last_event), expectedCheckpoints); assert.equal(checkpoints.length, 5455);
  const offsets = new Map(); let end = 0;
  for (const line of journalBytes.toString('utf8').slice(0, -1).split('\n')) {end += Buffer.byteLength(line) + 1; const row = JSON.parse(line); if (row.kind === 'event') offsets.set(row.event.action_id, end);}
  for (const [index, checkpoint] of checkpoints.entries()) {
    const snapshot = snapshots.get(checkpoint.last_event);
    assert.equal(checkpoint.index, index); assert.deepEqual(checkpoint.observed_counts, snapshot);
    assert.equal(checkpoint.events, snapshot.events); assert.equal(checkpoint.frames, snapshot.frames);
    assert.equal(checkpoint.raw_bytes, snapshot.frames * SIZE); assert.equal(checkpoint.journal_bytes, offsets.get(checkpoint.last_event));
  }
  assert.equal(raw.length, 93511792);
  const supervisorDirectory = dirname(freezePath);
  const started = JSON.parse(readFileSync(safeFile(supervisorDirectory, 'actual-supervisor-started.json')));
  const supervised = JSON.parse(readFileSync(safeFile(supervisorDirectory, 'actual-supervisor.json')));
  assert.equal(started.freeze_sha256, hash(freezeBytes));
  assert.deepEqual(started.command, ['node', join(root, 'engine/tests/fixtures/p2-word-carry-rotate/run.mjs'),
    freeze.engine_path, directory, root, join(supervisorDirectory, 'standalone-capture')]);
  assert.equal(started.deadline_seconds, 600); assert.equal(started.cleanup_reserve_seconds, 5);
  assert.equal(started.sources, sources.length); assert.equal(started.NODE_OPTIONS, ''); assert.deepEqual(started.engine, freeze.engine);
  for (const [key, value] of Object.entries(started)) assert.deepEqual(supervised[key], value);
  assert.equal(supervised.exit_code, 0); assert.equal(supervised.timeout, false); assert.equal(supervised.not_started, false);
  assert.equal(supervised.reaped, true); assert.equal(supervised.error, null);
  assert.equal(supervised.sources_unchanged, true); assert.equal(supervised.engine_unchanged, true); assert.equal(supervised.captures_unchanged, true);
  assert.deepEqual(supervised.changed_sources, []); assert.deepEqual(supervised.pin_errors, []); uint(supervised.wall_ns, 600 * 1e9);
  assert.deepEqual(Object.keys(supervised.files).sort(), [...files].map(file => `${relative(supervisorDirectory, directory)}/${file}`).sort());
  for (const file of files) assert.deepEqual(supervised.files[`${relative(supervisorDirectory, directory)}/${file}`], pin(readFileSync(safeFile(directory, file))));
  return {schema_version: 1, complete: true, semantic_certification: true,
    freeze_sha256: hash(freezeBytes), result_sha256: hash(resultBytes), raw_sha256: hash(raw), plan_sha256: hash(planBytes),
    engine: freeze.engine, counts, positive, zero: counts.generated_calls - positive, independently_replayed_instructions: retired,
    physical_files: files.size, journal_rows: journal.length, checkpoint_rows: checkpoints.length,
    observation_limit: 'finite authored register/flags/profile/lifecycle evidence; no memory operand, hardware undefined-OF, exhaustive ISA or performance claim'};
}

export function inspectIncomplete(directory) {
  directory = resolve(directory); const files = readdirSync(directory).sort().map(file => ({file, ...pin(readFileSync(safeFile(directory, file)))}));
  const rawFile = files.find(row => row.file === 'arenas.bin');
  return {schema_version: 1, complete: false, semantic_certification: false, files,
    available_full_frames: Math.floor((rawFile?.bytes ?? 0) / SIZE), trailing_frame_bytes: (rawFile?.bytes ?? 0) % SIZE,
    observation_limit: 'uncertified physical inventory includes copied modules even without finalized module rows; no partial semantic replay'};
}

function selfTest() {
  const before = authoredArena();
  const code = Buffer.alloc(4096, 0xcc); Buffer.from('66d3d1ebfe', 'hex').copy(code, 0x804);
  Buffer.from('0f0b', 'hex').copy(code, 0xf00);
  for (const entries of [false, true]) {
    const arena = Buffer.from(before), action = {name: entries ? 'compile_entries' : 'compile', args: [1]};
    arena.writeUInt32LE(0x1804, 140);
    if (!entries) arena.writeUInt32LE(3, 144);
    assert.equal(compileDescriptors(arena, code, action).instructions.size, entries ? 2 : 1);
    action.args[0] = 2; arena.writeUInt32LE(0x1f00, entries ? 144 : 148);
    if (!entries) arena.writeUInt32LE(2, 152);
    assert.throws(() => compileDescriptors(arena, code, action), /excluded UD2 at 1f00/);
  }
  const stored = Buffer.from(before); byteStoreSuccess(stored);
  const storeExpected = Buffer.from(before);
  Buffer.from('52334d48030001002800000000000000000000000000000000000000000000000000000001000000', 'hex').copy(storeExpected, 100);
  assert.deepEqual(stored, storeExpected, 'successful host byte store publishes exact v3 helper only');
  const context = {closed: false, stale: true, units: new Map()};
  for (const cancel of [0, 1]) for (const channel of ['direct', 'dispatch']) {
    const arena = Buffer.from(before); arena.writeUInt32LE(cancel, 96);
    const original = Buffer.from(arena);
    if (channel === 'direct') context.stale = false; else context.stale = true;
    context.units.set('one', {instructions: new Map()});
    assert.deepEqual(generated(arena, context, {channel, group: 'one', budget: 0}),
      {status: 0, retired: 0, reason: cancel ? 2 : 1});
    const expected = Buffer.from(original);
    Buffer.from(channel === 'direct' ? '52334558010001002800000000000000' : '52334558030001002800000000000000', 'hex').copy(expected, 56);
    expected.fill(0, 72, 96); expected.writeUInt32LE(cancel ? 2 : 1, 72);
    assert.deepEqual(arena, expected, 'zero/cancel full arena preserves transfer and opaque FP');
    const tampered = Buffer.from(arena); tampered[4363] ^= 1;
    assert.throws(() => assert.deepEqual(tampered, expected), 'private FP mutation must be detected');
  }
  assert.deepEqual(rotateBits(16, 'left', 0x8000, 0x446, 1), {value: 0, flags: 0xc47});
  assert.deepEqual(rotateBits(16, 'left', 0x7fff, 0x447, 1), {value: 0xffff, flags: 0xc46});
  assert.deepEqual(rotateBits(16, 'right', 1, 0x446, 1), {value: 0, flags: 0x447});
  assert.deepEqual(rotateBits(16, 'right', 0, 0x447, 1), {value: 0x8000, flags: 0xc46});
  for (const count of [0, 17, 32, 49]) assert.deepEqual(rotateBits(16, 'left', 0x8001, 0xcd7, count), {value: 0x8001, flags: 0xcd7});
  assert.deepEqual(rotateBits(16, 'left', 0x8000, 0xc46, 18), {value: 0, flags: 0x447});
  assert.deepEqual(rotateBits(16, 'left', 0x8000, 0xc46, 33), {value: 0, flags: 0xc47});
  const registers = [0xa53c8000, 0xa53c8012, 3, 4, 5, 6, 7, 8];
  let result = step(decodeAt(Buffer.from('66d3d1', 'hex'), 0, 4096), registers, 0xc46);
  assert.equal(result.registers[1], 0xa53c0024); assert.equal(result.flags, 0x447);
  result = step(decodeAt(Buffer.from('66d3d1', 'hex'), 0, 4099), result.registers, result.flags);
  assert.equal(result.registers[1], 0xa53c0248); assert.equal(result.flags, 0x446);
  assert.deepEqual(result.registers.filter((_, index) => index !== 1), registers.filter((_, index) => index !== 1));
  assert.equal(decodeAt(Buffer.from('66c1d066', 'hex'), 0, 4096).count, 0x66);
  for (const hex of ['6666d1d0', '66d0d0', '66d1d0'.slice(0, 4), '66d110']) assert.throws(() => decodeAt(Buffer.from(hex, 'hex'), 0, 4096));
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (process.argv[2] === '--self-test') {
    selfTest(); console.log('word carry independent checker: PASS');
  } else {
    assert.equal(process.argv.length, 4, 'usage: check.mjs OUTPUT FREEZE');
    const result = JSON.parse(readFileSync(join(resolve(process.argv[2]), 'result.json')));
    const receipt = result.status === 'complete' ? checkCapsule(process.argv[2], process.argv[3]) : inspectIncomplete(process.argv[2]);
    console.log(JSON.stringify(receipt, null, 2)); if (!receipt.complete) process.exitCode = 2;
  }
}
