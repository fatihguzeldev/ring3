import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {lstatSync, readFileSync, readdirSync, realpathSync} from 'node:fs';
import {dirname, isAbsolute, join, relative, resolve} from 'node:path';
import {fileURLToPath} from 'node:url';

const U32 = 2 ** 32, SIZE = 4364, CODE = 0x1000;
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
function uint(n, max = U32 - 1) {assert.ok(Number.isInteger(n) && n >= 0 && n <= max); return n;}
function parity(value) {let ones = 0; for (let i = 0; i < 8; i++) {ones += value % 2; value = Math.floor(value / 2);} return Number(ones % 2 === 0);}
function predicate(cc, flags) {
  const cf = Boolean(flags & 1), pf = Boolean(flags & 4), zf = Boolean(flags & 64), sf = Boolean(flags & 128), of = Boolean(flags & 2048);
  const base = [of, cf, zf, cf || zf, sf, pf, sf !== of, zf || sf !== of][cc >> 1];
  return cc % 2 ? !base : base;
}
export function add16(left, right, flags, carry = 0) {
  uint(left, 65535); uint(right, 65535); uint(flags); uint(carry, 1);
  const total = left + right + carry, value = total % 65536;
  const signed = n => n < 32768 ? n : n - 65536, sum = signed(left) + signed(right) + carry;
  return {value, flags: (flags & 1024) + 2 + Number(total > 65535) + parity(value) * 4
    + Number(left % 16 + right % 16 + carry > 15) * 16 + Number(value === 0) * 64
    + Number(value >= 32768) * 128 + Number(sum < -32768 || sum > 32767) * 2048};
}
export function decodeAt(bytes, offset, pc) {
  assert.ok(Buffer.isBuffer(bytes)); uint(offset, bytes.length - 1); uint(pc);
  const need = n => assert.ok(offset + n <= bytes.length, 'truncated authored instruction');
  const finish = (operation, length) => {need(length); return {...operation, pc, length, next: (pc + length) >>> 0};};
  const word = bytes[offset] === 102, at = offset + Number(word); need(Number(word) + 1);
  if (bytes[at] === 15) {
    need(Number(word) + 2); const opcode = bytes[at + 1];
    if (!word && opcode === 11) assert.fail(`excluded UD2 at ${pc.toString(16)}`);
    need(Number(word) + 3); const operand = bytes[at + 2];
    if (word && opcode === 193) {
      assert.ok(operand >= 192, 'register XADD only');
      return finish({kind: 'xadd', destination: operand % 8, source: Math.floor(operand / 8) % 8}, 4);
    }
    assert.ok(!word && opcode >= 144 && opcode <= 159 && operand >= 192 && operand < 200, 'authored SETcc only');
    return finish({kind: 'set', cc: opcode - 144, alias: operand % 8}, 3);
  }
  if (word && bytes[at] === 131) {
    need(4); const operand = bytes[at + 1], immediate = bytes[at + 2];
    assert.ok(operand >= 192 && Math.floor(operand / 8) % 8 === 2, 'authored WORD ADC only');
    return finish({kind: 'adc', destination: operand % 8, value: immediate < 128 ? immediate : immediate + 65280}, 4);
  }
  assert.equal(word, false, 'unrecognized WORD or extra prefix');
  if (bytes[at] === 235 || bytes[at] >= 112 && bytes[at] <= 127) {
    need(2); const raw = bytes[at + 1];
    return finish({kind: 'branch', cc: bytes[at] === 235 ? null : bytes[at] - 112, target: (pc + 2 + (raw < 128 ? raw : raw - 256)) >>> 0}, 2);
  }
  assert.fail(`unrecognized authored opcode at ${pc.toString(16)}`);
}
export function step(instruction, registers, flags) {
  assert.equal(registers.length, 8); registers.forEach(n => uint(n)); uint(flags);
  const output = [...registers]; let pc = instruction.next;
  if (instruction.kind === 'xadd') {
    const oldDestination = registers[instruction.destination], oldSource = registers[instruction.source];
    const result = add16(oldDestination % 65536, oldSource % 65536, flags);
    output[instruction.source] = Math.floor(oldSource / 65536) * 65536 + oldDestination % 65536;
    output[instruction.destination] = Math.floor(oldDestination / 65536) * 65536 + result.value;
    flags = result.flags;
  } else if (instruction.kind === 'set') {
    const parent = instruction.alias % 4, place = instruction.alias >= 4 ? 256 : 1;
    output[parent] = registers[parent] + (Number(predicate(instruction.cc, flags)) - Math.floor(registers[parent] / place) % 256) * place;
  } else if (instruction.kind === 'adc') {
    const result = add16(registers[instruction.destination] % 65536, instruction.value, flags, flags % 2);
    flags = result.flags; output[instruction.destination] = Math.floor(registers[instruction.destination] / 65536) * 65536 + result.value;
  } else if (instruction.kind === 'branch') {
    if (instruction.cc === null || predicate(instruction.cc, flags)) pc = instruction.target;
  } else assert.fail('unrecognized authored operation');
  return {registers: output, flags, pc};
}
function record(magic, size, fields = [], version = 1) {
  const bytes = Buffer.alloc(size); bytes.write(magic); bytes.writeUInt32LE(65536 + version, 4); bytes.writeUInt32LE(size, 8);
  fields.forEach((n, i) => bytes.writeUInt32LE(uint(n), 16 + i * 4)); return bytes;
}
export function defaultArena() {
  const arena = Buffer.alloc(SIZE); record('R3ST', 56, [0, 0, 0, 0, 0, 0, 0, 0, 0, 2]).copy(arena);
  record('R3EX', 40, [1, 0, 0, 0, 0, 0]).copy(arena, 56); record('R3MH', 40, [0, 0, 0, 0, 0, 0]).copy(arena, 100);
  const fp = record('R3FP', 128); fp.writeUInt16LE(895, 16); fp.writeUInt16LE(65535, 20); fp.copy(arena, 4236); return arena;
}
function seedBytes(pc, flags = 2, vector = 0, overrides = []) {
  const lows = [[65535, 1, 32768, 32767, 255, 65280, 21845, 43690], [1, 65535, 32767, 32768, 65280, 255, 43690, 21845]][vector];
  const registers = lows.map((low, index) => (41217 + vector * 273 + index * 257) * 65536 + low);
  for (const [index, low] of overrides) registers[index] = Math.floor(registers[index] / 65536) * 65536 + low;
  const bytes = Buffer.alloc(140); record('R3ST', 56, [...registers, pc, flags]).copy(bytes);
  record('R3EX', 40, [3, 0, 0, 0, 0, 0], 3).copy(bytes, 56); record('R3MH', 40, [0, 0xdecafbad, 0, 0, 0, 0]).copy(bytes, 100); return bytes;
}
export function authoredArena() {
  const arena = Buffer.from(Array.from({length: SIZE}, (_, i) => (37 * i + 19) % 256)); seedBytes(CODE).copy(arena);
  const fp = record('R3FP', 128);
  for (const [at, n] of [[16, 639], [18, 10304], [20, 43690], [22, 837], [32, 51], [34, 43]]) fp.writeUInt16LE(n, at);
  fp.writeUInt32LE(0x12345678, 24); fp.writeUInt32LE(0x9abcdef0, 28);
  for (let i = 40; i < 120; i++) fp[i] = (29 * i + 7) % 256; fp.copy(arena, 4236); return arena;
}
function state(arena) {
  assert.deepEqual(arena.subarray(0, 16), record('R3ST', 56).subarray(0, 16));
  const flags = arena.readUInt32LE(52); assert.equal(flags & 2, 2); assert.equal(flags & ~0xcd7, 0);
  return {registers: Array.from({length: 8}, (_, i) => arena.readUInt32LE(16 + i * 4)), pc: arena.readUInt32LE(48), flags};
}
function authoredBank() {
  const bytes = Buffer.alloc(8192, 204);
  for (let destination = 0; destination < 8; destination++) for (let source = 0; source < 8; source++) {
    const ordinal = destination * 8 + source, offset = (ordinal < 32 ? 0 : 3970) + ordinal % 32 * 4;
    bytes.set([102, 15, 193, 192 + source * 8 + destination], offset);
  }
  bytes.set([235, 254], 128); bytes.set([235, 254], 4098);
  const operands = [192, 201, 210, 219, 228, 237, 246, 255, 200, 193, 196, 224];
  for (const [index, operand] of operands.entries()) bytes.set([102, 15, 193, operand], 1024 + index * 4);
  bytes.set([235, 254], 1072);
  Buffer.from('660fc1d00f92c40f90c1660fc1c173020f0b6683d7000f92c7eb00', 'hex').copy(bytes, 2048);
  Buffer.from('660fc1d00f92c40f90c1660fc1c172020f0b6683d7000f92c7eb00', 'hex').copy(bytes, 2176);
  bytes.set([15, 11], 7936); return bytes;
}
const GROUP_SIZES = {'pairs-0': 33, 'pairs-1': 33, profile: 29, 'entry-profile': 29};
function literalGroups() {
  const blocks = [[5120, 50], [6144, 16], [6162, 9], [6272, 16], [6290, 9]];
  return [{id: 'pairs-0', entries: false, blocks: [[4096, 130]]}, {id: 'pairs-1', entries: false, blocks: [[8066, 130]]},
    {id: 'profile', entries: false, blocks}, {id: 'entry-profile', entries: true, blocks}];
}
function groupInstructions(bank, group) {
  const instructions = new Map();
  for (const [pc, length] of group.blocks) {
    let consumed = 0;
    while (consumed < length) {const op = decodeAt(bank, pc - CODE + consumed, pc + consumed); assert.ok(consumed + op.length <= length);
      assert.ok(!instructions.has(op.pc)); instructions.set(op.pc, op); consumed += op.length; if (op.kind === 'branch') assert.equal(consumed, length);}
    assert.equal(consumed, length);
  }
  assert.equal(instructions.size, GROUP_SIZES[group.id]); return instructions;
}
function childRun(arena, instructions, budget) {
  let cpu = state(arena), retired = 0, reason;
  for (;;) {if (arena.readUInt32LE(96)) {reason = 2; break;} if (retired === budget) {reason = 1; break;}
    const op = instructions.get(cpu.pc); if (!op) {reason = 3; break;} cpu = step(op, cpu.registers, cpu.flags); retired++;}
  record('R3ST', 56, [...cpu.registers, cpu.pc, cpu.flags]).copy(arena);
  record('R3EX', 40, [reason, retired, 0, 0, 0, 0]).copy(arena, 56); return {retired, reason};
}
function byteStoreSuccess(arena) {record('R3MH', 40, [0, 0, 0, 0, 0, 1], 3).copy(arena, 100);}
function installation(arena, unit) {record('R3IN', 32, [unit.id_low, unit.id_high, unit.slot, 0]).copy(arena, 140);}
function generated(arena, context, action) {
  if (context.closed) return {status: 5, retired: 0};
  if (action.channel !== 'dispatch') {if (context.stale) return {status: 4, retired: 0}; return {status: 0, ...childRun(arena, context.units.get(action.group).instructions, action.budget)};}
  state(arena); let retired = 0;
  for (;;) {
    if (arena.readUInt32LE(96) || retired === action.budget) {const reason = arena.readUInt32LE(96) ? 2 : 1;
      record('R3EX', 40, [reason, retired, 0, 0, 0, 0], 3).copy(arena, 56); return {status: 0, retired, reason};}
    const matches = [...context.units.values()].filter(unit => unit.installed && unit.instructions.has(arena.readUInt32LE(48))); assert.ok(matches.length <= 1);
    if (matches.length && context.stale) return {status: 4, retired: 0};
    if (!matches.length) {record('R3EX', 40, [3, retired, 0, 0, 0, 0], 3).copy(arena, 56); return {status: 0, retired, reason: 3};}
    installation(arena, matches[0]); const child = childRun(arena, matches[0].instructions, action.budget - retired); retired += child.retired; arena.writeUInt32LE(retired, 76);
    if (child.reason !== 3 || child.retired === 0) return {status: 0, retired, reason: child.reason};
  }
}
function residentAdmission(context, exactBytes = false) {
  if (context.owner !== 'resident') return;
  const live = [...context.units.values()]; assert.ok(live.length <= 4, 'late control has at most four prior resident units');
  assert.ok(live.length + 1 <= 8, 'late control leaves one resident unit slot');
  const bytes = live.reduce((sum, unit) => sum + (exactBytes ? uint(unit.bytes_length, 65536) : 65536), 0);
  assert.ok(bytes + 65536 <= 524288, 'late control leaves conservative candidate byte headroom');
}
export function inspectPlan(plan) {
  assert.equal(plan.schema_version, 1); assert.equal(plan.arena_bytes, SIZE); assert.equal(plan.code_address, CODE);
  const bank = authoredBank(), expectedGroups = literalGroups(); assert.deepEqual(Buffer.from(plan.bank.hex, 'hex'), bank);
  assert.deepEqual(plan.bank.groups.map(({id, entries, blocks}) => ({id, entries, blocks})), expectedGroups);
  const decoded = new Map(expectedGroups.map(g => [g.id, groupInstructions(bank, g)]));
  assert.equal(plan.bank.forms.length, 76); const formPCs = new Set();
  for (const [index, f] of plan.bank.forms.entries()) {
    assert.equal(f.id, index); assert.ok(!formPCs.has(f.pc)); formPCs.add(f.pc);
    const op = decodeAt(bank, f.pc - CODE, f.pc); assert.equal(op.kind, 'xadd'); assert.equal(op.destination, f.destination); assert.equal(op.source, f.source);
    assert.equal(f.length, 4); assert.equal(f.hex, bank.subarray(f.pc - CODE, f.pc - CODE + 4).toString('hex')); assert.ok(decoded.get(f.group).has(f.pc));
  }
  assert.deepEqual(plan.capture_files, ['bank.x86', 'initial-arena.bin', 'capture.json', 'profile.wasm', 'entry-profile.wasm']);
  const specs = [['replacement', false, 'normal'], ['resident', false, 'normal'], ['standalone', false, 'normal'], ['standalone', true, 'normal'], ['replacement', true, 'normal'], ['resident', true, 'normal'], ['replacement', false, 'stale'], ['resident', false, 'stale']];
  assert.equal(plan.contexts.length, 8); for (const [i, c] of plan.contexts.entries()) assert.deepEqual(c, {id: i + 1, owner: specs[i][0], entries: specs[i][1], role: specs[i][2], key: [0x58410001 + i, 0x574f5244], pages: 2});
  const edges = [[0,0],[0,1],[65535,1],[32767,1],[32768,32768],[65535,65535],[15,1],[255,1],[32768,0],[65535,0],[1,65535],[21845,43690]];
  const contexts = new Map(), coverage = {aliases: new Set(), edges: new Set(), self: new Set(), profiles: new Set()};
  let calls = 0, retired = 0, positive = 0, frames = 0, late = 0; const statuses = {}, labels = {};
  for (const [index, action] of plan.actions.entries()) {
    assert.equal(action.id, index); frames += action.kind === 'closed_call' ? 0 : ['open', 'close'].includes(action.kind) ? 1 : 2;
    if (action.kind === 'open') {assert.ok(!contexts.has(action.context)); contexts.set(action.context, {...plan.contexts[action.context - 1], arena: defaultArena(), units: new Map(), stale: false, closed: false, code: null, permissions: null}); continue;}
    const c = contexts.get(action.context); assert.ok(c); if (action.kind !== 'closed_call') assert.equal(c.closed, false);
    if (action.kind === 'input') {
      const bytes = Buffer.from(action.hex, 'hex'); assert.ok(action.offset >= 0 && action.offset + bytes.length <= SIZE);
      assert.ok(action.offset === 0 && [140, SIZE].includes(bytes.length) || action.offset === 140 || action.offset === 96 && ['00000000', '01000000'].includes(action.hex));
      if (bytes.length === SIZE) assert.deepEqual(bytes, authoredArena()); bytes.copy(c.arena, action.offset);
      if (action.offset === 0 && bytes.length === 140) {
        const cpu = state(c.arena), next = plan.actions[index + 1]; assert.equal(next.kind, 'call');
        if (next.form !== undefined) {
          const f = plan.bank.forms[next.form], op = decoded.get(next.group).get(cpu.pc); assert.ok(op); assert.equal(cpu.pc, f.pc);
          let vector, flags, overrides = [], target, item;
          if (action.label === 'complete ordered alias axis') {
            assert.ok([1,2].includes(c.id)); vector = next.vector; flags = next.flags; assert.ok([0,1].includes(vector)); assert.ok([2,3287].includes(flags));
            target = coverage.aliases; item = `${c.id}/${op.destination}/${op.source}/${vector}/${flags}`;
          } else if (action.label === 'separate arithmetic edge') {
            assert.ok([1,2].includes(c.id)); uint(next.edge, 11); vector = next.edge % 2; flags = vector ? 3287 : 2;
            assert.equal(next.form, 72); overrides = [[0, edges[next.edge][0]], [1, edges[next.edge][1]]]; target = coverage.edges; item = `${c.id}/${next.edge}`;
          } else if (action.label === 'separate self arithmetic edge') {
            assert.ok([1,2].includes(c.id)); uint(next.edge, 3); vector = next.edge % 2; flags = vector ? 3287 : 2;
            assert.equal(next.form, 64); overrides = [[0, [0,32768,32767,65535][next.edge]]]; target = coverage.self; item = `${c.id}/${next.edge}`;
          } else {
            assert.equal(action.label, 'six-profile representative'); assert.equal(c.role, 'normal'); assert.ok(next.form >= 64);
            vector = next.form % 2; flags = vector ? 3287 : 2; assert.equal(next.vector, vector); assert.equal(next.flags, flags);
            target = coverage.profiles; item = `${c.id}/${next.form}`;
          }
          assert.deepEqual(bytes, seedBytes(cpu.pc, flags, vector, overrides), 'independent coherent complete seed');
          assert.ok(!target.has(item), 'unique authored coverage witness'); target.add(item);
        } else if (['prior owner survival', 'pre-write current owner'].includes(action.label)) assert.deepEqual(bytes, seedBytes(5120, 3287, 1));
        else if (action.label === 'zero cancel boundary seed') assert.deepEqual(bytes, seedBytes(6144, 3287, 1));
        else {
          assert.ok(['continuous consumer seed', 'split consumer seed'].includes(action.label)); uint(next.path, 1);
          const overrides = next.path === 0 ? [[0,32767],[1,0],[2,1],[7,65535]] : [[0,65535],[1,65280],[2,256],[7,65535]];
          assert.deepEqual(bytes, seedBytes(6144 + next.path * 128, 3287, 1, overrides));
        }
      }
    } else if (action.kind === 'host') {
      if (action.name === 'map') {assert.deepEqual(action.args, [CODE, 2, 7]); assert.equal(c.code, null); c.code = Buffer.alloc(8192); c.permissions = 7;}
      else if (action.name === 'upload') {assert.equal(action.args[1], 4096); assert.ok([CODE, CODE + 4096].includes(action.args[0])); c.arena.copy(c.code, action.args[0] - CODE, 140, 4236);}
      else if (action.name === 'protect') {assert.deepEqual(action.args, [CODE, 2, 5]); assert.equal(c.role, 'normal'); assert.deepEqual(c.code, bank); c.permissions = 5;}
      else if (action.name.startsWith('compile')) {
        assert.deepEqual(c.code, bank); assert.equal(c.permissions, c.role === 'stale' ? 7 : 5);
        if (action.allowed_failure) {residentAdmission(c); late++; assert.throws(() => compileDescriptors(c.arena, c.code, action), /excluded UD2 at 2f00/);}
        else assert.deepEqual(compileDescriptors(c.arena, c.code, action).blocks, expectedGroups.find(g => g.id === action.group).blocks);
      } else if (action.name === 'acknowledge_resident_installation') c.units.get(action.group).installed = true;
      else if (action.name === 'dispatcher_module') assert.equal(c.owner, 'resident');
      else if (action.name === 'write8') {assert.equal(c.role, 'stale'); assert.deepEqual(action.args, [5120,102]); assert.equal(c.code[1024],102); byteStoreSuccess(c.arena); c.stale = true;}
      else assert.fail('unknown authored host');
    } else if (action.kind === 'module') {
      if (action.target === 'child') {if (c.owner === 'replacement') c.units.clear(); c.units.set(action.group, {instructions: decoded.get(action.group), id_low: c.units.size + 1, id_high: 0, slot: action.slot, installed: c.owner !== 'resident'});}
    } else if (action.kind === 'table') assert.ok(c.units.has(action.group));
    else if (action.kind === 'guard') {assert.equal(c.owner === 'standalone', false); assert.equal(action.wrong_key, true);}
    else if (action.kind === 'close') c.closed = true;
    else if (['call', 'closed_call'].includes(action.kind)) {
      if (action.kind === 'closed_call') {assert.equal(c.closed, true); assert.notEqual(c.owner, 'standalone'); assert.equal(action.channel, 'direct');}
      if (action.channel === 'dispatch') {assert.equal(c.owner, 'resident'); assert.equal(c.entries, false);}
      if (action.form !== undefined) assert.equal(state(c.arena).pc, plan.bank.forms[action.form].pc);
      const outcome = generated(c.arena, c, action); calls++; retired += outcome.retired; positive += Number(outcome.retired > 0);
      statuses[outcome.status] = (statuses[outcome.status] ?? 0) + 1; labels[action.label ?? 'closed'] = (labels[action.label ?? 'closed'] ?? 0) + 1;
      assert.equal(outcome.retired, action.planned_retired, 'raw semantics independently derive planned census');
    } else assert.fail('unknown authored action');
  }
  assert.deepEqual(Object.fromEntries(Object.entries(coverage).map(([key,value]) => [key,value.size])), {aliases:512,edges:24,self:8,profiles:72});
  assert.equal(late,4); assert.ok([...contexts.values()].every(c => c.closed)); assert.equal(calls,703); assert.equal(positive,670); assert.equal(retired,814); assert.equal(frames,2930);
  assert.deepEqual(statuses,{0:694,4:3,5:6});
  assert.deepEqual(labels, {'complete ordered alias axis':512,'six-profile representative':72,'separate arithmetic edge':24,'separate self arithmetic edge':8,'continuous consumer':12,'split consumer':36,'consumer no replay':12,'zero budget':4,'cancel positive budget':4,'cancel zero budget':4,'prior owner survival':4,closed:6,'pre-write current owner':2,'same-byte stale direct':2,'same-byte stale dispatcher':1});
  assert.deepEqual(plan.counts,{contexts:8,actions:1479,generated_calls:703,planned_retired:814,positive_calls:670,zero_retirement_calls:33,inputs:692,host_calls:45,guard_calls:4,tables:5,child_modules:12,dispatchers:2,frames:2930,checkpoints:716,capture_files:5,physical_files:23,journal_rows:4423,host_api_calls:94});
  return {contexts:8,calls,positive,zero:calls-positive,retired,frames,statuses,labels};
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
  const bodies = [];
  if (sections.has(10)) {const r = reader(sections.get(10)); for (let n = r.leb(); n; n--) bodies.push(r.take(r.leb())); r.done();}
  return {types, imports, exports, functions, bodies, sections: [...sections.keys()]};
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
  assert.equal(parsed.types.length, dispatcher ? 3 : owner === 'standalone' ? 1 : 2);
  assert.deepEqual(parsed.functions, dispatcher ? [1, 2, 0] : owner === 'standalone' ? [0] : [1, 0]);
  assert.equal(parsed.bodies.length, 1);
  const run = parsed.exports[0]; assert.equal(run.index, dispatcher ? 2 : owner === 'standalone' ? 0 : 1); assert.deepEqual(parsed.types[parsed.functions[run.index]], {parameters: [0x7f, 0x7f, 0x7f, 0x7f], results: [0x7f]});
  for (const imported of parsed.imports.filter(row => row.kind === 'function')) {
    const count = imported.name === 'guard' ? 6 : imported.name === 'guard_resident' ? 7 : imported.name === 'guard_dispatch_entry' ? 5 : 3;
    assert.deepEqual(parsed.types[imported.type], {parameters: Array(count).fill(0x7f), results: [0x7f]});
  }
  return {imports: parsed.imports.map(({module, name, kind}) => ({module, name, kind})),
    exports: parsed.exports.map(({name, kind}) => ({name, kind}))};
}

function moduleBinding(bytes, row, spec) {
  const body = wasmInterface(bytes).bodies[0]; let at = 0;
  const byte = () => {assert.ok(at < body.length); return body[at++];};
  const unsigned = () => {let value = 0, scale = 1; for(let i=0;i<5;i++){const n=byte();value+=(n%128)*scale;if(n<128)return uint(value);scale*=128;}assert.fail('bounded body ULEB');};
  const signed = () => {let value=0n,shift=0n,n;do{assert.ok(shift<35n);n=byte();value|=BigInt(n&127)<<shift;shift+=7n;}while(n&128);if(n&64)value-=1n<<shift;return Number(BigInt.asUintN(32,value));};
  const locals=Array.from({length:unsigned()},()=>[unsigned(),byte()]),dispatch=row.target==='dispatcher';
  assert.deepEqual(locals,dispatch?[[5,0x7f]]:[[16,0x7f],[1,0x7e]]);
  if(row.owner==='standalone')return;
  const constants=dispatch?spec.key:row.owner==='replacement'?[...spec.key,row.generation]:[...spec.key,row.id_low,row.id_high];
  for(const value of constants){assert.equal(byte(),0x41);assert.equal(signed(),value);}
  for(const parameter of [0,1,3]){assert.equal(byte(),0x20);assert.equal(unsigned(),parameter);}
  assert.equal(byte(),0x10);assert.equal(unsigned(),0);assert.equal(byte(),0x22);assert.equal(unsigned(),dispatch?5:16);
  assert.deepEqual(Array.from({length:7},byte),[0x04,0x40,0x20,dispatch?5:16,0x0f,0x0b,dispatch?0x41:0x3f]);
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
  for (const path of ['plan.mjs', 'run.mjs', 'check.mjs']) assert.ok(rows.some(row => row.path === `engine/tests/fixtures/p2-word-xadd/${path}`));
  assert.ok(rows.some(row => row.path === 'engine/tests/cpu_word_xadd_wasm.rs'));
  for (const path of ['engine/tests/cpu_word_xadd.rs', 'target/word-xadd-session/execute_actual.py', 'target/word-xadd-session/independent-word-xadd.py']) assert.ok(rows.some(row => row.path === path));
  assert.equal(freeze.reviews.length, 3);
  for (const review of freeze.reviews) {
    assert.equal(review.ack, true); const bytes = readFileSync(safeFile(root, review.index)); assert.deepEqual(pin(bytes), {bytes: review.bytes, sha256: review.sha256});
    const index = JSON.parse(bytes); assert.equal(index.ack, true);
    for (const [path, expected] of Object.entries(index.source_pins)) assert.deepEqual(freeze.source_pins[path], expected);
  }
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
  const captureNames = ['bank.x86', 'initial-arena.bin', 'capture.json', 'profile.wasm', 'entry-profile.wasm'].sort();
  assert.ok(isAbsolute(freeze.capture_path)); assert.equal(realpathSync(freeze.capture_path), freeze.capture_path);
  assert.deepEqual(readdirSync(freeze.capture_path).sort(), captureNames);
  for (const file of captureNames) assert.deepEqual(pin(readFileSync(safeFile(freeze.capture_path, file))), freeze.capture_pins[file]);
  const metadataNames = ['bank.x86', 'initial-arena.bin', 'capture.json'];
  assert.deepEqual(Object.keys(freeze.capture_pins).sort(), captureNames);
  assert.deepEqual(result.capture_pins, captureNames.map(file => ({file, ...freeze.capture_pins[file]})));
  for (const file of metadataNames) assert.deepEqual(pin(readFileSync(safeFile(directory, file))), freeze.capture_pins[file]);
  assert.deepEqual(readFileSync(safeFile(directory, 'initial-arena.bin')), defaultArena());
  const capture = JSON.parse(readFileSync(safeFile(directory, 'capture.json')));
  assert.equal(capture.schema_version, 1); assert.equal(capture.raw_bank_bytes, 8192); assert.equal(capture.standalone_modules, 2);
  assert.deepEqual(capture.groups, literalGroups().slice(-2).map(group => ({...group, instructions: 29})));
  assert.deepEqual(capture.files.map(row => row.file).sort(), captureNames.filter(file => file !== 'capture.json'));
  for (const row of capture.files) assert.equal(freeze.capture_pins[row.file].bytes, row.bytes);
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
  const modules = new Map(result.modules.map(row => [row.file, row])); assert.equal(modules.size, 14);
  const files = new Set([...metadataNames, 'plan.json', 'manifest.json', 'result.json', 'arenas.bin', 'journal.ndjson', 'checkpoints.ndjson']);
  for (const row of result.modules) {
    const bytes = readFileSync(safeFile(directory, row.file)); files.add(row.file);
    assert.deepEqual(pin(bytes), {bytes: row.bytes_length, sha256: row.sha256});
    assert.deepEqual(moduleInterface(bytes, row.owner, row.target === 'dispatcher'), {imports: row.imports, exports: row.exports});
    moduleBinding(bytes, row, plan.contexts.find(spec => spec.id === row.context));
    if (row.capture) assert.deepEqual(pin(bytes), freeze.capture_pins[row.capture]);
  }
  assert.equal(files.size, 23); assert.deepEqual(readdirSync(directory).sort(), [...files].sort());
  assert.equal(result.contexts.length, 8); assert.equal(new Set(result.contexts.map(row => row.id)).size, 8);
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
    const allocations = [...context.live.values()].filter(unit => unit.pointer !== null);
    for (let i = 0; i < allocations.length; i++) for (let j = 0; j < i; j++) {const a = allocations[i], b = allocations[j]; assert.ok(a.pointer + a.bytes_length <= b.pointer || b.pointer + b.bytes_length <= a.pointer, 'distinct live module allocations');}
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
      context = {...spec, base: event.arena_ptr, arena: defaultArena(), code: null, permissions: null, units: new Map(), live: new Map(), table: Array(8).fill(null), generation: 0, publication: null, pending: null, stale: false, closed: false};
      contexts.set(spec.id, context); counts.contexts++; counts.opened++;
      assert.deepEqual(event.args, [spec.pages, ...spec.key]); assert.equal(event.status, 0);
      counts.engine_instances++; counts.host_api_calls += 2;
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
          assert.deepEqual(args, [CODE, 2, 7]); assert.equal(context.code, null); context.code = Buffer.alloc(8192); context.permissions = 7; assert.equal(event.status, 0);
        } else if (action.name === 'upload') {
          assert.equal(args[1], 4096); assert.ok([CODE, CODE + 4096].includes(args[0])); assert.ok(context.code);
          context.arena.copy(context.code, args[0] - CODE, 140, 4236); assert.deepEqual(context.code.subarray(args[0] - CODE, args[0] - CODE + 4096), bank.subarray(args[0] - CODE, args[0] - CODE + 4096)); assert.equal(event.status, 0);
        } else if (action.name === 'protect') {
          assert.deepEqual(args, [CODE, 2, 5]); assert.equal(context.role, 'normal'); assert.deepEqual(context.code, bank); context.permissions = 5; assert.equal(event.status, 0);
        } else if (action.name.startsWith('compile')) {
          refs(event, context, 'reference_snapshot_before'); assert.deepEqual(context.code, bank); assert.equal(context.permissions, context.role === 'stale' ? 7 : 5); let compiled;
          const failed = action.allowed_failure === true;
          if (failed) {
            assert.equal(action.group, 'late-failure'); assert.equal(args[0], 2);
            const entries = action.name.endsWith('entries');
            const first = context.arena.readUInt32LE(140), second = context.arena.readUInt32LE(entries ? 144 : 148);
            residentAdmission(context, true);
            assert.equal(first, 0x1400); assert.equal(second, plan.bank.invalid_pc);
            assert.equal(context.code.subarray(first - CODE, first - CODE + 4).toString('hex'), '660fc1c0');
            assert.equal(context.code.subarray(second - CODE, second - CODE + 2).toString('hex'), '0f0b');
            if (!entries) {assert.equal(context.arena.readUInt32LE(144), 4); assert.equal(context.arena.readUInt32LE(152), 2);}
            assert.throws(() => compileDescriptors(context.arena, context.code, action),
              /excluded UD2 at 2f00/, 'late candidate reaches exact excluded UD2');
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
            assert.ok(context.pending.bytes_length <= 65536, 'authored child fits reserved unit byte bound');
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
          assert.equal(event.status, 0); assert.deepEqual(args, [0x1400, 0x66]); assert.equal(context.role, 'stale'); assert.equal(context.code[1024], 0x66);
          context.code[1024] = 0x66; byteStoreSuccess(context.arena); context.stale = true;
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
        counts.host_api_calls++;
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
  assert.equal(seenModules.size, 14); assert.ok([...contexts.values()].every(context => context.closed));
  assert.deepEqual(result.observed_counts, counts); assert.deepEqual(result.observed_statuses, statuses);
  assert.equal(positive, prediction.positive); assert.equal(retired, prediction.retired);
  assert.equal(counts.host_api_calls, 94); assert.equal(counts.inputs, 692); assert.equal(journal.length, 4423);
  const expectedCheckpoints = plan.actions.filter(action => ['call', 'closed_call', 'guard', 'close'].includes(action.kind)).map(action => action.id).concat(plan.actions.at(-1).id);
  assert.deepEqual(checkpoints.map(row => row.last_event), expectedCheckpoints); assert.equal(checkpoints.length, 716);
  const offsets = new Map(); let end = 0;
  for (const line of journalBytes.toString('utf8').slice(0, -1).split('\n')) {end += Buffer.byteLength(line) + 1; const row = JSON.parse(line); if (row.kind === 'event') offsets.set(row.event.action_id, end);}
  for (const [index, checkpoint] of checkpoints.entries()) {
    const snapshot = snapshots.get(checkpoint.last_event);
    assert.equal(checkpoint.index, index); assert.deepEqual(checkpoint.observed_counts, snapshot);
    assert.equal(checkpoint.events, snapshot.events); assert.equal(checkpoint.frames, snapshot.frames);
    assert.equal(checkpoint.raw_bytes, snapshot.frames * SIZE); assert.equal(checkpoint.journal_bytes, offsets.get(checkpoint.last_event));
  }
  assert.equal(raw.length, 12786520);
  const supervisorDirectory = dirname(freezePath);
  const started = JSON.parse(readFileSync(safeFile(supervisorDirectory, 'actual-supervisor-started.json')));
  const supervised = JSON.parse(readFileSync(safeFile(supervisorDirectory, 'actual-supervisor.json')));
  assert.equal(started.freeze_sha256, hash(freezeBytes));
  assert.deepEqual(started.command, ['node', join(root, 'engine/tests/fixtures/p2-word-xadd/run.mjs'),
    freeze.engine_path, directory, root, freeze.capture_path]);
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
    observation_limit: 'finite authored WORD register XADD dual-publication and FLAGS witnesses; no memory operand, hardware equivalence, exhaustive Cartesian/ISA or performance claim'};
}

export function inspectIncomplete(directory) {
  directory = resolve(directory); const files = readdirSync(directory).sort().map(file => ({file, ...pin(readFileSync(safeFile(directory, file)))}));
  const rawFile = files.find(row => row.file === 'arenas.bin');
  return {schema_version: 1, complete: false, semantic_certification: false, files,
    available_full_frames: Math.floor((rawFile?.bytes ?? 0) / SIZE), trailing_frame_bytes: (rawFile?.bytes ?? 0) % SIZE,
    observation_limit: 'uncertified physical inventory includes copied modules even without finalized module rows; no partial semantic replay'};
}

function selfTest() {
  const anchors = [[0,0,2,0,0x46],[0xffff,1,0xcd7,0,0x457],[0x7fff,1,2,0x8000,0x896],
    [0x8000,0x8000,2,0,0x847],[0xffff,0xffff,0xcd7,0xfffe,0x493],[15,1,2,16,0x12],
    [255,1,2,256,0x16],[0x5555,0xaaaa,2,0xffff,0x86],[1,0xffff,2,0,0x57]];
  for (const [left,right,flags,value,expected] of anchors) assert.deepEqual(add16(left,right,flags),{value,flags:expected});
  assert.deepEqual(add16(1,1,0xcd7),{value:2,flags:0x402},'incoming CF is not an XADD addend');
  const registers = [0xabcdffff,0x12340001,0x34568000,0x45677fff,0x567800ff,0x6789ff00,0x789a5555,0x89abaaaa];
  const cross = decodeAt(Buffer.from('660fc1c8','hex'),0,CODE), crossExpected = [...registers];
  crossExpected[0]=0xabcd0000;crossExpected[1]=0x1234ffff;
  assert.deepEqual(step(cross,registers,0xcd7),{registers:crossExpected,flags:0x457,pc:CODE+4});
  const self = decodeAt(Buffer.from('660fc1c0','hex'),0,CODE), selfExpected = [...registers];selfExpected[0]=0xabcdfffe;
  assert.deepEqual(step(self,registers,0xcd7),{registers:selfExpected,flags:0x493,pc:CODE+4},'destination sum wins self alias');
  const wrongOrder=[...selfExpected];wrongOrder[0]=registers[0];assert.throws(()=>assert.deepEqual(step(self,registers,0xcd7).registers,wrongOrder));
  const wrongHigh=[...crossExpected];wrongHigh[1]=0xabcdffff;assert.throws(()=>assert.deepEqual(step(cross,registers,0xcd7).registers,wrongHigh));
  for(const hex of ['66','660f','660fc1','66660fc1c8','67660fc1c8','660fc103','660fc10424','f0660fc1c8','0fc1c8','660fc0c8']) assert.throws(()=>decodeAt(Buffer.from(hex,'hex'),0,CODE));
  assert.deepEqual(Buffer.from('660fc1ff','hex'),authoredBank().subarray(4094,4098),'literal cross-page FF instruction');
  const bank=authoredBank(),instructions=groupInstructions(bank,literalGroups().find(g=>g.id==='profile'));
  const endpoints=[
    [0xa2120001,0xa3130001,0xa4147fff,0xa5150000,0xa616ff00,0xa71700ff,0xa818aaaa,0xa919ffff,0x181b,0x486],
    [0xa212ff00,0xa31300ff,0xa414ffff,0xa5150100,0xa616ff00,0xa71700ff,0xa818aaaa,0xa9190000,0x189b,0x457],
  ];
  for(const path of [0,1]) {
    const before=authoredArena(),overrides=path===0?[[0,32767],[1,0],[2,1],[7,65535]]:[[0,65535],[1,65280],[2,256],[7,65535]];
    seedBytes(0x1800+path*128,0xcd7,1,overrides).copy(before);
    const context={closed:false,stale:false,units:new Map([['profile',{instructions,installed:true,id_low:19,id_high:23,slot:2}]])};
    const arena=Buffer.from(before),expected=Buffer.from(before);
    assert.deepEqual(generated(arena,context,{channel:'direct',group:'profile',budget:9}),{status:0,retired:8,reason:3});
    record('R3ST',56,endpoints[path]).copy(expected);record('R3EX',40,[3,8,0,0,0,0]).copy(expected,56);
    assert.deepEqual(arena,expected,'literal full consumer endpoint');
    before.copy(arena);for(const budget of [1,3,4])assert.equal(generated(arena,context,{channel:'direct',group:'profile',budget}).retired,budget);
    assert.equal(arena.readUInt32LE(72),1);assert.equal(arena.readUInt32LE(48),0x181b+path*128);
    assert.deepEqual(generated(arena,context,{channel:'direct',group:'profile',budget:1}),{status:0,retired:0,reason:3});assert.deepEqual(state(arena),state(expected));
    const dispatched=Buffer.from(before);generated(dispatched,context,{channel:'dispatch',group:'metadata-does-not-select',budget:9});
    const dispatchExpected=Buffer.from(expected);installation(dispatchExpected,{id_low:19,id_high:23,slot:2});record('R3EX',40,[3,8,0,0,0,0],3).copy(dispatchExpected,56);
    assert.deepEqual(dispatched,dispatchExpected,'installed physical membership selects dispatcher receipt');
  }
  for(const entries of [false,true]) {
    const arena=authoredArena(),action={name:entries?'compile_entries':'compile',args:[1]};arena.writeUInt32LE(0x1400,140);if(!entries)arena.writeUInt32LE(4,144);
    assert.equal(compileDescriptors(arena,bank,action).instructions.size,entries?13:1);
    action.args[0]=2;arena.writeUInt32LE(0x2f00,entries?144:148);if(!entries)arena.writeUInt32LE(2,152);
    assert.throws(()=>compileDescriptors(arena,bank,action),/excluded UD2 at 2f00/);
  }
  const units=n=>new Map(Array.from({length:n},(_,i)=>[i,{bytes_length:65536}]));
  residentAdmission({owner:'resident',units:units(4)});residentAdmission({owner:'resident',units:units(4)},true);
  for(const n of [5,8]) assert.throws(()=>residentAdmission({owner:'resident',units:units(n)}));
  assert.throws(()=>residentAdmission({owner:'resident',units:new Map([[0,{bytes_length:65537}]])},true));
  const before=authoredArena();
  for(const channel of ['direct','dispatch'])for(const cancel of [0,1]) {
    const arena=Buffer.from(before);arena.writeUInt32LE(cancel,96);const expected=Buffer.from(arena);
    const context={closed:false,stale:false,units:new Map([['one',{instructions:new Map()}]])};
    assert.deepEqual(generated(arena,context,{channel,group:'one',budget:0}),{status:0,retired:0,reason:cancel?2:1});
    record('R3EX',40,[cancel?2:1,0,0,0,0,0],channel==='dispatch'?3:1).copy(expected,56);assert.deepEqual(arena,expected);
    const tampered=Buffer.from(arena);tampered[4363]^=1;assert.throws(()=>assert.deepEqual(tampered,expected));
  }
  const stored=Buffer.from(before),storedExpected=Buffer.from(before);byteStoreSuccess(stored);
  Buffer.from('52334d48030001002800000000000000000000000000000000000000000000000000000001000000','hex').copy(storedExpected,100);assert.deepEqual(stored,storedExpected);
  for(const channel of ['direct','dispatch']) {
    const arena=Buffer.from(before),context={closed:false,stale:true,units:new Map([['one',{instructions:new Map([[CODE,decodeAt(Buffer.from('660fc1c8','hex'),0,CODE)]]),installed:true}]])};
    assert.deepEqual(generated(arena,context,{channel,group:'one',budget:1}),{status:4,retired:0});assert.deepEqual(arena,before);
    context.closed=true;assert.deepEqual(generated(arena,context,{channel,group:'one',budget:1}),{status:5,retired:0});assert.deepEqual(arena,before);
  }
  const makeBody=(constants,dispatcher=false)=>{
    const body=dispatcher?[1,5,0x7f]:[2,16,0x7f,1,0x7e];for(const n of constants)body.push(0x41,n);
    body.push(0x20,0,0x20,1,0x20,3,0x10,0,0x22,dispatcher?5:16,4,0x40,0x20,dispatcher?5:16,0x0f,0x0b,dispatcher?0x41:0x3f,0,0x0b);
    return Buffer.from([0,97,115,109,1,0,0,0,10,body.length+2,1,body.length,...body]);
  };
  for(const owner of ['replacement','resident']) {
    const row={owner,target:'child',generation:7,id_low:11,id_high:13},spec={key:[17,19]};
    const bytes=makeBody(owner==='replacement'?[17,19,7]:[17,19,11,13]);moduleBinding(bytes,row,spec);
    const wrong=Buffer.from(bytes);wrong[wrong.indexOf(0x41)+1]^=1;assert.throws(()=>moduleBinding(wrong,row,spec));
    assert.throws(()=>moduleBinding(bytes,{...row,generation:8,id_low:12},spec));
  }
  moduleBinding(makeBody([17,19],true),{owner:'resident',target:'dispatcher'},{key:[17,19]});
}
if(process.argv[1]&&resolve(process.argv[1])===fileURLToPath(import.meta.url)) {
  if(process.argv[2]==='--self-test'){selfTest();console.log('WORD register XADD independent checker pure tests: PASS');}
  else if(process.argv[2]==='--plan'){assert.equal(process.argv.length,4);console.log(JSON.stringify(inspectPlan(JSON.parse(readFileSync(process.argv[3]))),null,2));}
  else if(process.argv[2]==='--mutations') {
    const plan=JSON.parse(readFileSync(process.argv[3]));inspectPlan(plan);let rejected=0;
    const mutations=[p=>p.bank.hex=`00${p.bank.hex.slice(2)}`,p=>p.contexts[2].owner='replacement',p=>p.bank.groups[0].blocks[0][1]--,
      p=>p.actions.find(a=>a.kind==='call').planned_retired=0,p=>p.actions.find(a=>a.kind==='input'&&a.label==='complete ordered alias axis').hex=p.actions.find(a=>a.kind==='input'&&a.label==='separate arithmetic edge').hex,
      p=>p.actions.splice(p.actions.findIndex(a=>a.label==='separate self arithmetic edge'),2),p=>p.actions.find(a=>a.kind==='host'&&a.name==='protect').args[2]=7,
      p=>p.actions.find(a=>a.kind==='closed_call').channel='dispatch',p=>p.capture_files.pop(),p=>p.actions.find(a=>a.kind==='host'&&a.allowed_failure).args[0]=1];
    for(const mutation of mutations){const changed=structuredClone(plan);mutation(changed);assert.throws(()=>inspectPlan(changed));rejected++;}
    console.log(JSON.stringify({pure_plan_rejections:rejected}));
  } else {assert.equal(process.argv.length,4,'usage: check.mjs OUTPUT FREEZE');const result=JSON.parse(readFileSync(join(resolve(process.argv[2]),'result.json')));
    const receipt=result.status==='complete'?checkCapsule(process.argv[2],process.argv[3]):inspectIncomplete(process.argv[2]);console.log(JSON.stringify(receipt,null,2));if(!receipt.complete)process.exitCode=2;}
}
