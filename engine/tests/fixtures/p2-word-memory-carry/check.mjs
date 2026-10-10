import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {lstatSync, readFileSync, readdirSync, realpathSync} from 'node:fs';
import {dirname, isAbsolute, join, relative, resolve} from 'node:path';
import {fileURLToPath} from 'node:url';

const U32 = 2 ** 32, SIZE = 4364, CODE = 0x1000, DATA = 0x4000;
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
function uint(n, max = U32 - 1) {assert.ok(Number.isInteger(n) && n >= 0 && n <= max); return n;}
function record(magic, size, fields = [], version = 1) {
  const b = Buffer.alloc(size); b.write(magic); b.writeUInt32LE(0x10000 + version, 4); b.writeUInt32LE(size, 8);
  fields.forEach((n, i) => b.writeUInt32LE(uint(n), 16 + i * 4)); return b;
}
function state(arena) {
  assert.deepEqual(arena.subarray(0, 16), record('R3ST', 56).subarray(0, 16));
  const registers = Array.from({length: 8}, (_, i) => arena.readUInt32LE(16 + i * 4)), pc = arena.readUInt32LE(48), flags = arena.readUInt32LE(52);
  assert.equal(flags & ~0xcd7, 0); assert.equal(flags & 2, 2); return {registers, pc, flags};
}
export function predicate(cc, flags) {
  const cf = Boolean(flags & 1), pf = Boolean(flags & 4), zf = Boolean(flags & 64), sf = Boolean(flags & 128), of = Boolean(flags & 2048);
  switch (cc) {
    case 0: return of; case 1: return !of; case 2: return cf; case 3: return !cf;
    case 4: return zf; case 5: return !zf; case 6: return cf || zf; case 7: return !cf && !zf;
    case 8: return sf; case 9: return !sf; case 10: return pf; case 11: return !pf;
    case 12: return sf !== of; case 13: return sf === of; case 14: return zf || sf !== of; case 15: return !zf && sf === of;
    default: assert.fail('unknown condition');
  }
}
export function decodeAt(bytes, offset, pc) {
  uint(offset, bytes.length - 1); uint(pc); let cursor = offset;
  const take = () => {assert.ok(cursor < bytes.length, 'truncated instruction'); return bytes[cursor++];};
  const little = count => {let n = 0; for (let i = 0; i < count; i++) n += take() * 256 ** i; return n;};
  const signedByte = () => {const n = take(); return n < 128 ? n : n - 256;};
  const finish = op => ({...op, pc, length: cursor - offset, next: (pc + cursor - offset) % U32});
  const operand = modrm => {
    const mode = Math.floor(modrm / 64), rm = modrm % 8;
    if (mode === 3) return {register: rm};
    let base = rm, index = null, scale = 1, displacement = 0;
    if (rm === 4) {const sib = take(); base = sib % 8; index = Math.floor(sib / 8) % 8; scale = 2 ** Math.floor(sib / 64); if (index === 4) index = null;}
    if (mode === 0 && base === 5) {base = null; displacement = little(4);}
    if (mode === 1) displacement += signedByte(); else if (mode === 2) displacement += little(4);
    return {base, index, scale, displacement: (displacement + U32) % U32};
  };
  let opcode = take(), word = false;
  if (opcode === 102) {word = true; opcode = take();}
  if (word && [19, 27].includes(opcode)) {
    const modrm = take(), field = Math.floor(modrm / 8) % 8, memory = operand(modrm);
    assert.equal(memory.register, undefined, 'authored checked memory source only');
    return finish({kind: 'arithmetic', subtract: opcode === 27, destination: field, memory});
  }
  if (word && opcode >= 184 && opcode <= 191) return finish({kind: 'move', destination: opcode - 184, value: little(2)});
  if (word && opcode === 131) {
    const modrm = take(), field = Math.floor(modrm / 8) % 8;
    assert.ok(modrm >= 192 && [0, 2, 5].includes(field), 'authored register carry producer/consumer only');
    const n = signedByte(); return finish({kind: 'register-arithmetic', subtract: field === 5, consumeCarry: field === 2, destination: modrm % 8, value: (n + 65536) % 65536});
  }
  if (opcode === 15) {
    const sub = take(); if (!word && sub === 11) assert.fail(`excluded UD2 at ${pc.toString(16)}`);
    const modrm = take();
    assert.ok(!word && sub >= 144 && sub <= 159 && modrm >= 192 && modrm < 200, 'authored SETcc only');
    return finish({kind: 'set', cc: sub - 144, alias: modrm % 8});
  }
  assert.equal(word, false, 'unrecognized WORD prefix or opcode');
  if (opcode === 144) return finish({kind: 'nop'});
  if (opcode === 235 || opcode >= 112 && opcode <= 127) return finish({kind: 'branch', cc: opcode === 235 ? null : opcode - 112, target: (pc + 2 + signedByte() + U32) % U32});
  assert.fail('unsupported authored opcode');
}
export function addressOf(source, registers) {
  assert.equal(source.register, undefined);
  return (source.displacement + (source.base === null ? 0 : registers[source.base]) + (source.index === null ? 0 : registers[source.index] * source.scale)) % U32;
}
export class RAM {
  constructor() {this.pages = new Map(); this.clock = 0;}
  map(address, count, bits) {assert.equal(address % 4096, 0); for (let i = 0; i < count; i++) assert.ok(!this.pages.has(address + i * 4096)); const token = ++this.clock; for (let i = 0; i < count; i++) this.pages.set(address + i * 4096, {bits, token, bytes: Buffer.alloc(4096)});}
  protect(address, count, bits) {for (let i = 0; i < count; i++) assert.ok(this.pages.has(address + i * 4096)); const token = ++this.clock; for (let i = 0; i < count; i++) Object.assign(this.pages.get(address + i * 4096), {bits, token});}
  unmap(address, count) {for (let i = 0; i < count; i++) assert.ok(this.pages.delete(address + i * 4096)); this.clock++;}
  access(address, length, bit) {
    uint(address); uint(length); if (address + length > U32) return {detail: 3, address};
    for (let n = 0; n < length;) {const current = address + n, page = current - current % 4096, mapping = this.pages.get(page);
      if (!mapping) return {detail: 1, address: current}; if (!(mapping.bits & bit)) return {detail: 2, address: current}; n += Math.min(length - n, 4096 - current % 4096);}
    return null;
  }
  read(address, length, bit = 1) {const fault = this.access(address, length, bit); if (fault) return {fault}; const bytes = Buffer.alloc(length); for (let i = 0; i < length; i++) bytes[i] = this.pages.get((address + i) - (address + i) % 4096).bytes[(address + i) % 4096]; return {bytes};}
  write(address, bytes) {assert.equal(this.access(address, bytes.length, 2), null); const token = ++this.clock; for (let i = 0; i < bytes.length; i++) {const at = address + i, page = this.pages.get(at - at % 4096); page.bytes[at % 4096] = bytes[i]; page.token = token;}}
  snapshot(blocks) {const tokens = new Map(); for (const [pc, length] of blocks) {assert.equal(this.access(pc, length, 4), null); for (let page = pc - pc % 4096; page < pc + length; page += 4096) tokens.set(page, this.pages.get(page).token);} return tokens;}
  current(tokens) {return [...tokens].every(([page, token]) => this.pages.get(page)?.token === token);}
}
function narrowPacket(c, address, foreign) {
  if (foreign) {Buffer.from(foreign.packet_hex, 'hex').copy(c.arena, 100); return foreign.status;}
  const result = c.ram.read(address, 2), fields = result.fault ? [1, 0, result.fault.detail, result.fault.address, 1, 2] : [0, result.bytes.readUInt16LE(), 0, 0, 0, 2];
  record('R3MH', 40, fields, 2).copy(c.arena, 100); return 0;
}
function validateNarrow(arena, address, status) {
  if (status) return {reason: 7, detail: 3};
  const b = arena.subarray(100, 140), fields = Array.from({length: 6}, (_, i) => b.readUInt32LE(16 + i * 4));
  const [result, value, detail, at, access, length] = fields;
  if (!b.subarray(0, 16).equals(record('R3MH', 40, [], 2).subarray(0, 16)) || length !== 2) return {reason: 7, detail: 2};
  if (result === 0) return value <= 65535 && !detail && !at && !access && address <= U32 - 2 ? {value} : {reason: 7, detail: 2};
  if (result === 1) {
    const valid = !value && access === 1 && detail >= 1 && detail <= 3 && (detail === 3 ? address > U32 - 2 && at === address : address <= U32 - 2 && at >= address && at < address + 2);
    return valid ? {reason: 5, detail, address: at, access: 1, length: 2} : {reason: 7, detail: 2};
  }
  if (result === 2 && !value && !at && !access && [1, 2].includes(detail)) return {reason: 7, detail: detail === 1 ? 1 : 3};
  return {reason: 7, detail: 2};
}
export function wordArithmetic(left, right, oldFlags, subtract = false, carry = 0) {
  uint(left, 65535); uint(right, 65535); uint(carry, 1);
  const whole = subtract ? left - right - carry : left + right + carry;
  const result = (whole + 65536) % 65536;
  let ones = 0; for (let n = result % 256; n; n = Math.floor(n / 2)) ones += n % 2;
  let flags = (oldFlags & 1024) + 2 + Number(ones % 2 === 0) * 4 + Number(result === 0) * 64 + Number(result >= 32768) * 128;
  const signed = n => n < 32768 ? n : n - 65536;
  const signedWhole = subtract ? signed(left) - signed(right) - carry : signed(left) + signed(right) + carry;
  flags += Number(whole < 0 || whole > 65535) + Number(subtract ? left % 16 < right % 16 + carry : left % 16 + right % 16 + carry > 15) * 16
    + Number(signedWhole < -32768 || signedWhole > 32767) * 2048;
  return {result, flags};
}
function execute(c, op, cpu, foreign, addresses) {
  const old = cpu.registers, registers = [...old]; let flags = cpu.flags, pc = op.next;
  if (op.kind === 'arithmetic') {
    const address = addressOf(op.memory, old); addresses.push(address);
    const checked = validateNarrow(c.arena, address, narrowPacket(c, address, foreign));
    if (checked.reason) return {fault: checked};
    const out = wordArithmetic(old[op.destination] % 65536, checked.value, flags, op.subtract, flags % 2);
    flags = out.flags; registers[op.destination] = old[op.destination] - old[op.destination] % 65536 + out.result;
  } else if (op.kind === 'register-arithmetic' || op.kind === 'move') {
    const out = op.kind === 'register-arithmetic' ? wordArithmetic(old[op.destination] % 65536, op.value, flags, op.subtract, op.consumeCarry ? flags % 2 : 0) : {result: op.value, flags};
    flags = out.flags; registers[op.destination] = old[op.destination] - old[op.destination] % 65536 + out.result;
  } else if (op.kind === 'set') {
    const parent = op.alias % 4, place = op.alias < 4 ? 1 : 256;
    registers[parent] += (Number(predicate(op.cc, flags)) - Math.floor(old[parent] / place) % 256) * place;
  } else if (op.kind === 'branch') {if (op.cc === null || predicate(op.cc, flags)) pc = op.target;}
  else assert.equal(op.kind, 'nop');
  return {cpu: {registers, flags, pc}};
}
function childRun(c, unit, budget) {
  let cpu = state(c.arena), retired = 0, outcome; const addresses = [];
  for (;;) {
    if (c.arena.readUInt32LE(96)) {outcome = {reason: 2}; break;}
    if (retired === budget) {outcome = {reason: 1}; break;}
    const op = unit.instructions.get(cpu.pc); if (!op) {outcome = {reason: 3}; break;}
    const step = execute(c, op, cpu, unit.foreign, addresses); if (step.fault) {outcome = step.fault; break;}
    cpu = step.cpu; retired++;
  }
  record('R3ST', 56, [...cpu.registers, cpu.pc, cpu.flags]).copy(c.arena);
  record('R3EX', 40, [outcome.reason, retired, outcome.detail ?? 0, outcome.address ?? 0, outcome.access ?? 0, outcome.length ?? 0], 2).copy(c.arena, 56);
  return {...outcome, retired, addresses};
}
function installation(arena, unit) {record('R3IN', 32, [unit.id_low, unit.id_high, unit.slot, 0]).copy(arena, 140);}
export function generated(c, action) {
  if (c.closed) return {status: 5, retired: 0, addresses: []};
  if (action.channel === 'direct') {const unit = c.units.get(action.group); assert.ok(unit); if (!c.ram.current(unit.tokens)) return {status: 4, retired: 0, addresses: []}; return {status: 0, ...childRun(c, unit, action.budget)};}
  let retired = 0; const addresses = [];
  for (;;) {
    if (c.arena.readUInt32LE(96) || retired === action.budget) {const reason = c.arena.readUInt32LE(96) ? 2 : 1; record('R3EX', 40, [reason, retired, 0, 0, 0, 0], 3).copy(c.arena, 56); return {status: 0, retired, reason, addresses};}
    const matches = [...c.units.values()].filter(u => u.installed && u.instructions.has(c.arena.readUInt32LE(48))); assert.ok(matches.length <= 1);
    if (!matches.length) {record('R3EX', 40, [3, retired, 0, 0, 0, 0], 3).copy(c.arena, 56); return {status: 0, retired, reason: 3, addresses};}
    const unit = matches[0]; if (!c.ram.current(unit.tokens)) {if(retired)c.arena.writeUInt32LE(retired,76);return {status: 4, retired, addresses};}
    installation(c.arena, unit); const out = childRun(c, unit, action.budget - retired); retired += out.retired; addresses.push(...out.addresses); c.arena.writeUInt32LE(retired, 76);
    if (out.reason !== 3 || out.retired === 0) return {...out, status: 0, retired, addresses};
  }
}
export function defaultArena() {
  const arena = Buffer.alloc(SIZE); record('R3ST', 56, [0, 0, 0, 0, 0, 0, 0, 0, 0, 2]).copy(arena); record('R3EX', 40, [1, 0, 0, 0, 0, 0]).copy(arena, 56); record('R3MH', 40, [0, 0, 0, 0, 0, 0]).copy(arena, 100);
  const fp = record('R3FP', 128); fp.writeUInt16LE(0x37f, 16); fp.writeUInt16LE(0xffff, 20); fp.copy(arena, 4236); return arena;
}
function authoredArena() {
  const b = Buffer.from(Array.from({length: SIZE}, (_, i) => (37 * i + 19) % 256));
  const regs = [0xa1010000, 0xa2020001, 0xa3037fff, 0xa404ffff, 0xa5058000, 0xa6061234, 0xa707aaaa, 0xa8085555];
  record('R3ST', 56, [...regs, CODE, 2]).copy(b); record('R3EX', 40, [3, 0, 0, 0, 0, 0], 3).copy(b, 56); record('R3MH', 40, [0, 0xdecafbad, 0, 0, 0, 0]).copy(b, 100); b.fill(0,96,100);
  const fp = record('R3FP', 128); for (const [at, n] of [[16, 0x27f], [18, 0x2840], [20, 0xaaaa], [22, 0x345], [32, 0x33], [34, 0x2b]]) fp.writeUInt16LE(n, at);
  fp.writeUInt32LE(0x12345678, 24); fp.writeUInt32LE(0x9abcdef0, 28); for (let i = 40; i < 120; i++) fp[i] = (29 * i + 7) % 256; fp.copy(b, 4236); return b;
}
function compileDescriptors(c, action) {
  const entries = action.name.endsWith('entries'), count = action.args[0]; uint(count, 8); assert.ok(count > 0);
  const roots = Array.from({length: count}, (_, i) => c.arena.readUInt32LE(140 + i * (entries ? 4 : 8)));
  const code = c.ram.read(CODE, 4096, 4); assert.ok(code.bytes);
  const blocks = roots.map((pc, i) => {
    assert.ok(pc >= CODE && pc < CODE + 4096); if (!entries) return [pc, c.arena.readUInt32LE(144 + i * 8)];
    const end = Math.min(CODE + 4096, ...roots.filter(p => p > pc)); let length = 0;
    for (;;) {assert.ok(pc + length < end); const op = decodeAt(code.bytes, pc + length - CODE, pc + length); length += op.length; assert.ok(pc + length <= end); if (op.kind === 'branch') break;}
    return [pc, length];
  });
  const instructions = new Map();
  for (const [i, [pc, length]] of blocks.entries()) {
    assert.ok(length > 0 && pc + length <= CODE + 4096); for (const [other, n] of blocks.slice(0, i)) assert.ok(pc + length <= other || other + n <= pc);
    let at = 0; while (at < length) {assert.ok(instructions.size < 64); const op = decodeAt(code.bytes, pc + at - CODE, pc + at); at += op.length; assert.ok(at <= length); if (op.kind === 'branch') assert.equal(at, length); assert.ok(!instructions.has(op.pc)); instructions.set(op.pc, op);}
  }
  return {blocks, instructions, tokens: c.ram.snapshot(blocks)};
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
  } else imports.push({module: 'ring3', name: owner === 'resident' ? 'guard_resident' : 'guard', kind: 'function', type: 1}, {module: 'ring3', name: 'read16', kind: 'function', type: 2});
  assert.deepEqual(parsed.imports, imports);
  assert.deepEqual(parsed.exports.map(({name, kind}) => ({name, kind})), [{name: 'run', kind: 'function'}]);
  assert.equal(parsed.types.length, 3);
  assert.deepEqual(parsed.functions, [1, 2, 0]);
  assert.equal(parsed.bodies.length, 1);
  const run = parsed.exports[0]; assert.equal(run.index, 2); assert.deepEqual(parsed.types[parsed.functions[run.index]], {parameters: [0x7f, 0x7f, 0x7f, 0x7f], results: [0x7f]});
  for (const imported of parsed.imports.filter(row => row.kind === 'function')) {
    const count = imported.name === 'guard' ? 6 : imported.name === 'guard_resident' ? 7 : imported.name === 'guard_dispatch_entry' ? 5 : imported.name === 'read16' ? 1 : 3;
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
  assert.deepEqual(locals,dispatch?[[5,0x7f]]:[[16,0x7f],[1,0x7e],[6,0x7f]]);
  const constants=dispatch?spec.key:row.owner==='replacement'?[...spec.key,row.generation]:[...spec.key,row.id_low,row.id_high];
  for(const value of constants){assert.equal(byte(),0x41);assert.equal(signed(),value);}
  for(const parameter of [0,1,3]){assert.equal(byte(),0x20);assert.equal(unsigned(),parameter);}
  assert.equal(byte(),0x10);assert.equal(unsigned(),0);assert.equal(byte(),0x22);assert.equal(unsigned(),dispatch?5:16);
  assert.deepEqual(Array.from({length:7},byte),[0x04,0x40,0x20,dispatch?5:16,0x0f,0x0b,dispatch?0x41:0x3f]);
}


const edgeLeft = [[0,0,32767,65535,0,15,255,32768,4660,43690,32766,32768],
  [0,0,32768,65535,4660,16,256,32767,0,43690,32769,32768]];
const edgeRight = [[0,65535,0,0,65535,0,0,32767,61166,21845,0,65535],
  [0,65535,0,65535,4660,0,0,65535,0,21845,0,65535]];
const edgeCarry = [[0,1,1,1,0,1,1,1,1,0,1,1],[0,1,1,1,1,1,1,0,1,1,1,1]];
function literalBank() {
  const bytes = Buffer.alloc(4096, 204), le = n => [n % 256, Math.floor(n / 256) % 256, Math.floor(n / 65536) % 256, Math.floor(n / 16777216) % 256];
  let cursor = 0;
  const write = (subtract, parent, rm) => {const raw=[102,subtract?27:19,rm[0]+parent*8,...rm.slice(1)];bytes.set(raw,cursor);cursor+=raw.length;};
  const stop = () => bytes.set([235, 254], cursor);
  for(const subtract of[false,true])for(let r=0;r<12;r++)write(subtract,r%8,[5,...le(DATA+2*(Number(subtract)*12+r))]);
  for(const subtract of[false,true])for(const[r,tail]of[[3,[131,128,81,0,0]],[6,[4,245,128,65,0,0]],[1,[132,11,128,65,0,0]],
    [0,[132,48,128,65,0,0]],[5,[132,61,128,65,0,0]],[2,[4,149,128,65,0,0]],[3,[67,255]],[4,[68,36,255]]])write(subtract,r,tail);
  assert.equal(cursor,280);stop();cursor=512;
  const aliases = [];
  for (let r = 0; r < 8; r++) {
    aliases.push([r, r === 4 ? [4, 36] : r === 5 ? [69, 0] : [r]]);
    if (r !== 4) {aliases.push([r, [4, r * 8 + 5, 128, 65, 0, 0]]); aliases.push([r, r === 5 ? [68, r * 9, 0] : [4, r * 9]]);}
  }
  for(const subtract of[false,true])for(const[r,tail]of aliases)write(subtract,r,tail);assert.equal(cursor,734);stop();
  cursor=1536;for(const subtract of[false,true]){bytes[cursor++]=144;write(subtract,0,[3]);}stop();
  const consumers = [
    '66b8ff7f66bfffff6683c0016613030f92c30f90c10f94c20f98c60f9ac572020f0b6683d7000f92c7eb00',
    '66b8000066bfffff6683e801661b030f92c30f90c10f94c20f98c60f9ac573020f0b6683d7000f92c7eb00'];
  consumers.forEach((hex,i)=>{const raw=Buffer.from(hex,'hex');assert.equal(raw.length,43);bytes.set(raw,2048+i*64);});
  for(const[i,hex]of['66139b0000fc5b661b830000fc5b0f94c474020f0beb00','661b9b0000fc5b6613830000fc5b0f92c472020f0beb00'].entries()){const raw=Buffer.from(hex,'hex');assert.equal(raw.length,23);bytes.set(raw,2560+i*64);}
  bytes.set([15, 11], 3840); return bytes;
}
function literalHelpers() {
  const rows = [], good = record('R3MH', 40, [0, 32769, 0, 0, 0, 2], 2);
  for (const [id, status, offset, value] of [['status',9,null,0],['version',0,4,65537],['width',0,36,4],['value',0,20,65536],['span',0,null,0],['infrastructure',0,null,0]]) {
    const p = Buffer.from(good); if (offset !== null) p.writeUInt32LE(value, offset);
    if(id==='span'){p.writeUInt32LE(1,16);p.writeUInt32LE(0,20);p.writeUInt32LE(1,24);p.writeUInt32LE(DATA+2,28);p.writeUInt32LE(1,32);}
    if(id==='infrastructure'){p.writeUInt32LE(2,16);p.writeUInt32LE(0,20);p.writeUInt32LE(2,24);}
    rows.push({id,status,packet_hex:p.toString('hex')});
  }
  return rows;
}
function literalData() {
  const b = Buffer.from(Array.from({length:4096},(_,i)=>(73*i+11)%256));
  edgeRight.flat().forEach((n,i)=>b.writeUInt16LE(n,i*2));for(const[at,n]of[[256,1],[258,32769],[384,32769],[512,32769],[514,1],[768,65535],[770,0]])b.writeUInt16LE(n,at);return b;
}
const literalGroups = [
  {id:'main',blocks:[[4096,282]],instructions:41}, {id:'alias',blocks:[[4608,224]],instructions:45},
  {id:'control-a',blocks:[[5632,10],[6144,32],[6178,9],[6208,32],[6242,9]],instructions:31},
  {id:'control-b',blocks:[[6656,19],[6677,2],[6720,19],[6741,2]],instructions:10},
  {id:'helper',blocks:[[5632,4]],instructions:2}, {id:'keeper',blocks:[[5633,3]],instructions:1}
];
function literalRows() {
  const bytes=literalBank(),rows=[];
  const row=(axis,pc,fields={})=>{const op=decodeAt(bytes,pc-CODE,pc);assert.equal(op.kind,'arithmetic');const r={id:rows.length,axis,kind:op.subtract?'sbb':'adc',destination:op.destination,pc,length:op.length,hex:bytes.subarray(pc-CODE,pc-CODE+op.length).toString('hex'),overrides:{},...fields};rows.push(r);return op.next;};
  let pc=CODE;
  for(let family=0;family<2;family++)for(let sample=0;sample<12;sample++)pc=row('edge',pc,{case:sample,left:edgeLeft[family][sample],carry:edgeCarry[family][sample]});
  const wrapInputs=[{3:4294963200},{6:536870912},{3:2147483648,1:2147483648},{0:4294967295,6:1},{5:1,7:4294967295},{2:1073741824},{3:16769},{4:16769}];
  for(let family=0;family<2;family++)for(const overrides of wrapInputs)pc=row('wrap',pc,{overrides});
  pc=4608;for(let family=0;family<2;family++)for(let parent=0;parent<8;parent++){
    pc=row('alias',pc,{alias:'base',overrides:{[parent]:16768}});
    if(parent!==4){pc=row('alias',pc,{alias:'index',overrides:{[parent]:0}});pc=row('alias',pc,{alias:'both',overrides:{[parent]:8384}});}
  }
  row('fault',5633);row('fault',5637);assert.equal(rows.length,86);return rows;
}
function literalDescription() {
  const rows=literalRows();return{hex:literalBank().toString('hex'),forms:rows,fault_forms:rows.slice(-2).map((r,i)=>({...r,prefix:5632+i*4})),
    consumer:{paths:Array.from({length:2},(_,i)=>({id:i,kind:i?'sbb':'adc',prefix:6144+i*64,pc:6144+i*64,arithmetic_pc:6156+i*64,end_pc:6187+i*64,instructions:13})),
      next_paths:[{id:0,pc:6656,end_pc:6679,instructions:5},{id:1,pc:6720,end_pc:6743,instructions:5}]},invalid_pc:7936,groups:literalGroups};
}
const seedLows=[[0,1,32767,65535,32768,4660,43690,21845],[32767,65535,0,32768,1,43690,21845,4660],[32768,32767,65535,32767,4660,1,21845,43690]];
function literalSeed(pc,flags=0xcd7,overrides={},pattern=0) {
  const registers=seedLows[pattern].map((n,i)=>(0xa101+pattern*0x111+i*0x101)*65536+n);
  for(const[r,n]of Object.entries(overrides))registers[Number(r)]=n;
  const b=Buffer.alloc(140);record('R3ST',56,[...registers,pc,flags]).copy(b);record('R3EX',40,[3,0,0,0,0,0],3).copy(b,56);record('R3MH',40,[0,0xdecafbad,0,0,0,0]).copy(b,100);return b.toString('hex');
}
function literalSchedule() {
  const bank=literalDescription(),actions=[],contexts=[],groups=new Map(literalGroups.map(g=>[g.id,g]));
  function owner(owner,entries,role='normal') {
    const id=contexts.length+1,spec={id,owner,entries,role,key:[0x57430000+id,0x43415252],pages:6};contexts.push(spec);let diagnostic=0;
    const put=(kind,fields={})=>actions.push({id:actions.length,context:id,kind,...fields});
    const input=(offset,hex,label)=>put('input',{offset,hex,label});
    const host=(name,args,fields={})=>put('host',{name,args,...fields});
    const run=(group,budget,label,n,fields={})=>put('call',{group,channel:owner==='resident'&&!entries?'dispatch':'direct',budget,label,planned_retired:n,...fields});
    const seed=(pc,flags=0xcd7,overrides={},pattern=0,label='CPU seed')=>input(0,literalSeed(pc,flags,overrides,pattern),label);
    const upload=(address,hex,label)=>{input(140,hex,label);host('upload',[address,hex.length/2]);};
    const data=(address,count,phase)=>put('data',{address,count,file:`${id}-${diagnostic++}-${phase}.bin`,label:count===1024?'complete readable RAM page':'bounded readable watched word'});
    const watch=address=>{data(address-address%4,1,'watch');if(address%4===3)data(address+1,1,'watch-second');};
    function compile(group,slot) {
      const blocks=groups.get(group).blocks,b=Buffer.alloc(blocks.length*(entries?4:8));
      blocks.forEach(([pc,length],i)=>{b.writeUInt32LE(pc,i*(entries?4:8));if(!entries)b.writeUInt32LE(length,i*8+4);});input(140,b.toString('hex'),'compiler descriptors');
      host(owner==='resident'?entries?'compile_resident_entries':'compile_resident':entries?'compile_entries':'compile',entries?[blocks.length,0]:[blocks.length],{group});
      put('module',{target:'child',group,slot,file:`${id}-${group}.wasm`,helper_case:null});
      if(owner==='resident'){put('table',{group,slot});host('acknowledge_resident_installation',['key_low','key_high','unit_low','unit_high',slot],{group});if(slot===0){host('dispatcher_module',['key_low','key_high']);put('module',{target:'dispatcher',group:'dispatcher',slot:null,file:`${id}-dispatcher.wasm`,helper_case:null});}}
    }
    function form(f,flags,pattern=0) {
      const overrides={...f.overrides};if(f.axis==='edge')overrides[f.destination]=(0xb101+pattern*0x111+f.destination*0x101)*65536+f.left;
      seed(f.pc,flags,overrides,pattern,'form and operand seed');
    }
    const consume=path=>seed(path.pc,0xcd7,{3:17152+2*path.id,7:0xa808ffff});
    function repair(path) {
      const group='control-a';seed(path.prefix,0xcd7,{3:32768});run(group,20,'one-seed repair initial fault',3,{path:path.id});run(group,1,'one-seed repair unchanged retry',0,{path:path.id});
      host('map',[32768,1,3]);upload(32768,literalData().toString('hex'),'declared one-seed repair upload');host('protect',[32768,1,2]);run(group,1,'one-seed repair write-only fault',0,{path:path.id});
      host('write8',[32768,path.id?0:255]);host('write8',[32769,path.id?0:255]);host('protect',[32768,1,1]);run(group,1,'one-seed repair arithmetic',1,{path:path.id});watch(32768);
      run(group,9,'one-seed repair consumers',9,{path:path.id});run(group,1,'one-seed repair no replay',0,{path:path.id});data(32768,1024,'repair-final');host('unmap',[32768,1]);
    }
    put('open');input(0,authoredArena().toString('hex'),'full authored nondefault arena');host('map',[CODE,1,7]);upload(CODE,literalBank().toString('hex'),'complete literal bank');if(role!=='stale')host('protect',[CODE,1,5]);
    host('map',[DATA,1,3]);upload(DATA,literalData().toString('hex'),'literal data page');host('protect',[DATA,1,1]);data(DATA,1024,'initial-data');
    if(role==='normal'){host('map',[4294963200,1,3]);upload(4294963200,literalData().toString('hex'),'literal top page');host('protect',[4294963200,1,1]);data(4294963200,1024,'initial-top');}
    if(role==='normal') {
      compile('main',0);
      for(const f of bank.forms.slice(0,24).filter(f=>!entries||f.case<2))for(const extra of[2,0xcd6]){form(f,extra|f.carry,extra===2?0:2);run('main',1,entries?'entry form representative':'edge form and FLAGS',1,{form:f.id});watch(DATA+f.id*2);}
      if(!entries){
        for(const f of bank.forms.slice(24,40)){form(f,0xcd7);run('main',1,'wrapping effective address',1,{form:f.id});watch(16768);}
        compile('alias',1);for(const f of bank.forms.slice(40,84))for(const flags of[2,0xcd7]){form(f,flags);run('alias',1,'legal destination address alias',1,{form:f.id});watch(16768);}
        for(const f of bank.forms.slice(40,84).filter(f=>f.alias==='base'&&f.destination===3)){
          seed(f.pc,0xcd7,{3:4294967295});run('alias',1,'overflow alias fault',0,{form:f.id});run('alias',1,'overflow alias unchanged retry',0,{form:f.id});
          input(28,'00430000','explicit overflow address-register different input');run('alias',1,'overflow alias repair completion',1,{form:f.id});watch(17152);
        }
      }
      compile('control-a',entries?1:2);
      if(!entries){
        const failures=[['unmapped',32768,32768,null,null],['none',16384,null,16384,0],['write',16384,null,16384,2],['execute',16384,null,16384,4],
          ['cross-unmapped',20479,20480,null,null],['cross-write',20479,null,20480,2],['overflow',4294967295,null,null,null]];
        for(const[name,address,mapped,permission,bits]of failures){
          if(name==='cross-write'){host('map',[20480,1,3]);upload(20480,literalData().toString('hex'),'cross-page data');}
          for(const f of bank.fault_forms)for(const flags of name==='unmapped'?[0xcd6,0xcd7]:[0xcd7]){
            if(permission!==null)host('protect',[permission,1,bits]);seed(f.prefix,flags,{0:0xb101ffff,3:address});run('control-a',20,`${name} prefix fault`,1,{form:f.id});run('control-a',1,`${name} unchanged retry`,0,{form:f.id});
            if(mapped!==null){host('map',[mapped,1,3]);upload(mapped,literalData().toString('hex'),'data-only fault repair');host('protect',[mapped,1,1]);}
            else if(permission!==null)host('protect',[permission,1,1]);else input(28,'00430000','explicit overflow address-register different input');
            run('control-a',1,`${name} repair completion`,1,{form:f.id});watch(name==='overflow'?17152:address);if(mapped!==null)host('unmap',[mapped,1]);
          }
          if(name==='cross-write')host('unmap',[20480,1]);
        }
        for(const address of[16401,20478,4294967294])for(const f of bank.fault_forms)for(const pattern of[0,2]){seed(f.pc,pattern?0xcd7:2,{3:address},pattern);run('control-a',1,'unaligned or last-two-byte read',1,{form:f.id});watch(address);}
        host('map',[20480,1,3]);upload(20480,literalData().toString('hex'),'cross-page success data');host('protect',[20480,1,1]);
        for(const f of bank.fault_forms)for(const pattern of[0,2]){seed(f.pc,pattern?0xcd7:2,{3:20479},pattern);run('control-a',1,'successful cross-page read',1,{form:f.id});watch(20479);}
        data(20480,1024,'cross-final');host('unmap',[20480,1]);
      }
      for(const path of bank.consumer.paths.slice(0,2)){
        consume(path);run('control-a',14,'continuous consumer',13,{path:path.id});watch(17152);consume(path);run('control-a',3,'split consumer',3,{path:path.id});
        if(path.id===0){input(96,'01000000','set consumer cancel');run('control-a',5,'consumer cancel continuation',0,{path:path.id});input(96,'00000000','clear consumer cancel');}
        for(const n of[6,4])run('control-a',n,'split consumer',n,{path:path.id});run('control-a',1,'consumer no replay',0,{path:path.id});watch(17152);
      }
      if(!entries){
        for(const path of bank.consumer.paths)repair(path);compile('control-b',3);
        for(const path of bank.consumer.next_paths){
          const overrides={0:path.id?0xa1017fff:0xa1018001,3:path.id?0xa4044202:0xa4044100},addresses=path.id?[16896,16898]:[16640,16642];
          seed(path.pc,0xcd7,overrides);run('control-b',6,'current-address continuous',5,{path:path.id});addresses.forEach(watch);
          seed(path.pc,0xcd7,overrides);for(const n of[1,1,3])run('control-b',n,'current-address split',n,{path:path.id});run('control-b',1,'current-address no replay',0,{path:path.id});addresses.forEach(watch);
        }
      }
      const current=entries?'control-a':'control-b',survivor=entries?bank.consumer.paths[0]:bank.consumer.next_paths[0];seed(survivor.pc,0xcd7,{3:DATA});run(current,0,'zero budget',0);
      input(96,'01000000','set cancel');run(current,20,'cancel positive budget',0);run(current,0,'cancel zero budget',0);input(96,'00000000','clear cancel');
      const request=Buffer.alloc(entries?8:16);request.writeUInt32LE(6144);if(!entries)request.writeUInt32LE(32,4);request.writeUInt32LE(7936,entries?4:8);if(!entries)request.writeUInt32LE(2,12);input(140,request.toString('hex'),'late admitted arithmetic then UD2');
      host(owner==='resident'?entries?'compile_resident_entries':'compile_resident':entries?'compile_entries':'compile',entries?[2,0]:[2],{group:'late-failure',allowed_failure:true});seed(survivor.pc,0xcd7,entries?{3:17152}:{0:0xa1018001,3:0xa4044100});run(current,1,'prior owner survival',1);put('guard',{group:current,wrong_key:true});
    }else if(role==='helper'){
      input(140,'0016000004000000','inert helper module descriptors');host('compile',[1],{group:'helper'});
      for(const h of literalHelpers()){put('module',{target:'child',group:'helper',slot:null,file:`${id}-helper-${h.id}.wasm`,helper_case:h.id});seed(5632,0xcd7,{0:0xb1010001,3:DATA});run('helper',20,'malformed inert helper '+h.id,1,{channel:'direct',helper_case:h.id});}
    }else{
      compile('keeper',0);seed(5633,0xcd7,{0:0xb1010001,3:DATA});run('keeper',1,'current before same-byte write',1);watch(DATA);
      seed(5633,0xcd7,{0:0xb1010001,3:DATA});host('write8',[CODE,102]);run('keeper',1,'same-byte stale direct',0,{channel:'direct'});run('keeper',0,'stale direct zero budget',0,{channel:'direct'});input(96,'01000000','set stale direct cancel');run('keeper',0,'stale direct cancel zero',0,{channel:'direct'});input(96,'00000000','clear stale direct cancel');
      if(owner==='resident'){run('keeper',1,'same-byte stale dispatcher',0,{channel:'dispatch'});run('keeper',0,'stale dispatcher zero budget',0,{channel:'dispatch'});input(96,'01000000','set stale dispatcher cancel');run('keeper',0,'stale dispatcher cancel zero',0,{channel:'dispatch'});input(96,'00000000','clear stale dispatcher cancel');}
    }
    data(DATA,1024,'final-data');if(role==='normal')data(4294963200,1024,'final-top');put('close');put('closed_call',{group:role==='normal'?entries?'control-a':'control-b':role==='helper'?'helper':'keeper',channel:'direct',budget:1,label:'cached closed function',planned_retired:0});
  }
  for(const mode of[['replacement',false],['replacement',true],['resident',false],['resident',true]])owner(...mode);
  owner('replacement',false,'helper');owner('replacement',false,'stale');owner('resident',false,'stale');return{actions,contexts};
}
function fresh(spec,base=128){return {...spec,base,arena:defaultArena(),ram:new RAM(),units:new Map(),live:new Map(),table:Array(8).fill(null),generation:0,publication:null,pending:null,closed:false};}
function resolved(c,action){const u=c.units.get(action.group)??c.pending;return action.args.map(x=>x==='key_low'?c.key[0]:x==='key_high'?c.key[1]:x==='unit_low'?u.id_low:x==='unit_high'?u.id_high:x);}
function read32(c,address){const r=c.ram.read(address,4);assert.ok(r.bytes,'diagnostic readable full page');const value=r.bytes.readUInt32LE();record('R3MH',40,[0,value,0,0,0,0]).copy(c.arena,100);return{address,status:0,value,helper_hex:c.arena.subarray(100,140).toString('hex')};}
function hostStep(c,a,e,groups,physical){
  const args=resolved(c,a);if(physical)assert.deepEqual(e.args,args);
  let status=0;
  if(a.name==='map')c.ram.map(...args);
  else if(a.name==='protect')c.ram.protect(...args);
  else if(a.name==='unmap')c.ram.unmap(...args);
  else if(a.name==='upload')c.ram.write(args[0],Buffer.from(c.arena.subarray(140,140+args[1])));
  else if(a.name==='write8'){assert.ok([[CODE,102],[32768,0],[32769,0],[32768,255],[32769,255]].some(pair=>pair[0]===args[0]&&pair[1]===args[1]));c.ram.write(args[0],Buffer.from([args[1]]));record('R3MH',40,[0,0,0,0,0,1],3).copy(c.arena,100);}
  else if(a.name.startsWith('compile')){
    let compiled;
    if(a.allowed_failure){
      assert.equal(a.group,'late-failure');assert.equal(args[0],2);assert.equal(c.arena.readUInt32LE(140),6144);assert.equal(c.arena.readUInt32LE(a.name.endsWith('entries')?144:148),7936);
      if(c.owner==='resident'){assert.ok(c.units.size<=4,'late decode must not hit unit admission first');assert.ok(c.units.size+1<=8);assert.ok((c.units.size+1)*65536<=524288,'worst-case per-unit cap retains candidate byte reserve before guest');const liveBytes=[...c.units.values()].reduce((n,u)=>n+u.bytes_length,0);assert.ok(liveBytes+65536<=524288,'late decode must retain conservative candidate byte reserve');}
      assert.throws(()=>compileDescriptors(c,a),/excluded UD2 at 1f00/);status=10;
    }
    else compiled=compileDescriptors(c,a);
    if(c.owner==='replacement'){
      if(status){if(physical)assert.deepEqual(e.publication,c.publication);}
      else{const m=physical?e.publication:{generation:c.generation+1,pointer:65536+c.generation*8192,bytes_length:100};assert.equal(m.generation,++c.generation);uint(m.pointer);uint(m.bytes_length,65536);assert.ok(m.pointer>0&&m.bytes_length>8);c.publication=m;c.units.clear();c.live.clear();c.pending={...m,id_low:null,id_high:null};}
    }else if(!status){const words=physical?e.receipt.words:[1,24,c.units.size+1,c.id,65536+c.units.size*8192,100];assert.equal(words.length,6);assert.deepEqual(words.slice(0,2),[1,24]);assert.ok(words[2]||words[3]);assert.ok(words[4]>0&&words[5]>8&&words[5]<=65536);assert.ok(c.units.size<8&&[...c.units.values()].reduce((n,u)=>n+u.bytes_length,0)+words[5]<=524288);assert.ok([...c.units.values()].every(u=>u.id_low!==words[2]||u.id_high!==words[3]));const b=Buffer.alloc(24);words.forEach((n,i)=>b.writeUInt32LE(uint(n),4*i));if(physical)assert.equal(e.receipt.hex,b.toString('hex'));b.copy(c.arena,140);c.pending={generation:null,id_low:words[2],id_high:words[3],pointer:words[4],bytes_length:words[5]};}
    if(!status){assert.deepEqual(compiled.blocks,groups.get(a.group).blocks);c.pending={...c.pending,...compiled,group:a.group,installed:false,foreign:null};c.units.set(a.group,c.pending);}
  }else if(a.name==='dispatcher_module'){
    const words=physical?e.receipt.words:[0x50443352,0x10001,32,0,...c.key,131072,100];assert.equal(words.length,8);assert.deepEqual(words.slice(0,6),[0x50443352,0x10001,32,0,...c.key]);assert.ok(words[6]>0&&words[7]>8);const b=Buffer.alloc(32);words.forEach((n,i)=>b.writeUInt32LE(uint(n),i*4));if(physical)assert.equal(e.receipt.hex,b.toString('hex'));b.copy(c.arena,140);c.dispatchMetadata={pointer:words[6],bytes_length:words[7],generation:null,id_low:null,id_high:null};
  }else if(a.name==='acknowledge_resident_installation'){
    const u=c.units.get(a.group);assert.deepEqual(args.slice(0,4),[...c.key,u.id_low,u.id_high]);assert.equal(c.table[args[4]],a.group);u.slot=args[4];u.installed=true;installation(c.arena,u);if(physical){assert.equal(e.receipt.hex,c.arena.subarray(140,172).toString('hex'));assert.deepEqual(e.receipt.words,Array.from({length:8},(_,i)=>c.arena.readUInt32LE(140+i*4)));}
  }else assert.fail('unknown host action');if(physical)assert.equal(e.status,status);return status;
}
export function validatePlan(plan) {
  assert.equal(plan.schema_version,1);assert.equal(plan.size,SIZE);assert.equal(plan.code,CODE);assert.equal(plan.data_address,DATA);assert.equal(plan.top,4294963200);
  assert.deepEqual(plan.bank,literalDescription(),'all raw bytes, forms and root metadata are independently literal');assert.equal(plan.data_hex,literalData().toString('hex'));assert.deepEqual(plan.helper_cases,literalHelpers());
  const schedule=literalSchedule();assert.deepEqual(plan.contexts,schedule.contexts);assert.deepEqual(plan.actions,schedule.actions,'every literal action, input byte, descriptor, seed, repair and cohort order');
  const count=kind=>schedule.actions.filter(a=>a.kind===kind).length,calls=schedule.actions.filter(a=>['call','closed_call'].includes(a.kind));
  const data=schedule.actions.filter(a=>a.kind==='data'),frames=schedule.actions.reduce((n,a)=>n+(['open','close'].includes(a.kind)?1:a.kind==='closed_call'?0:2),0);
  const expected={contexts:7,actions:schedule.actions.length,generated_calls:calls.length,planned_retired:calls.reduce((n,a)=>n+a.planned_retired,0),modules:count('module'),frames,raw_bytes:frames*SIZE,
    data_files:data.length,diagnostic_reads:data.reduce((n,a)=>n+a.count,0),inputs:count('input'),guards:count('guard'),tables:count('table'),files:10+count('module')+data.length,
    checkpoints:calls.length+count('guard')+count('close')+data.length+1};
  assert.deepEqual(plan.counts,expected);assert.equal(expected.generated_calls,588);assert.equal(expected.modules,23);assert.equal(expected.planned_retired,732);assert.ok(expected.frames<=5000&&expected.generated_calls<=600&&expected.modules<=24);
  const labels=new Map();for(const a of calls)labels.set(a.label,(labels.get(a.label)??0)+1);
  for(const[label,n]of[['edge form and FLAGS',96],['entry form representative',16],['legal destination address alias',176],['wrapping effective address',32],
    ['unaligned or last-two-byte read',24],['successful cross-page read',8],['continuous consumer',8],['split consumer',24],['consumer no replay',8],['consumer cancel continuation',4],
    ['current-address continuous',4],['current-address split',12],['current-address no replay',4],['prior owner survival',4],['cached closed function',7],['zero budget',4],['cancel positive budget',4],['cancel zero budget',4]])assert.equal(labels.get(label),n,label);
  const groups=new Map(literalGroups.map(g=>[g.id,g])),contexts=new Map(),coverage=new Set(),late=[];let retired=0,positive=0,callsN=0,foreignN=0;
  for(const a of schedule.actions){let c=contexts.get(a.context);
    if(a.kind==='open'){assert.ok(!c);c=fresh(schedule.contexts.find(s=>s.id===a.context));contexts.set(a.context,c);}
    else{
      assert.ok(c);if(a.kind==='closed_call'){assert.ok(c.closed);assert.equal(generated(c,a).status,5);callsN++;continue;}assert.equal(c.closed,false);
      if(a.kind==='input'){const b=Buffer.from(a.hex,'hex');assert.ok(a.offset>=0&&a.offset+b.length<=SIZE);if(b.length===SIZE)assert.deepEqual(b,authoredArena());b.copy(c.arena,a.offset);if(a.offset===0)state(c.arena);}
      else if(a.kind==='host'){
        if(a.allowed_failure)late.push({context:c.id,units:c.units.size,bytes:[...c.units.values()].reduce((n,u)=>n+u.bytes_length,0)});
        hostStep(c,a,null,groups,false);
      }else if(a.kind==='module'){
        if(a.target==='child'){assert.ok(c.units.has(a.group),'replacement current publication must still own every requested child');c.units.get(a.group).foreign=a.helper_case===null?null:literalHelpers().find(h=>h.id===a.helper_case);}
      }else if(a.kind==='table'){assert.ok(c.units.has(a.group));c.table[a.slot]=a.group;}
      else if(a.kind==='data'){assert.ok([1,1024].includes(a.count));for(let i=0;i<a.count;i++)read32(c,a.address+4*i);}
      else if(a.kind==='call'){
        const before=state(c.arena),u=c.units.get(a.group);assert.ok(u,'no old replacement child lookup after a later publication');const op=u.instructions.get(before.pc);
        if(['edge form and FLAGS','entry form representative','legal destination address alias','wrapping effective address'].includes(a.label)){
          assert.equal(op.kind,'arithmetic');const key=`${c.id}/${before.pc}/${before.flags}`;assert.ok(!coverage.has(key));coverage.add(key);
          if(a.label==='legal destination address alias')assert.ok(op.memory.base===op.destination||op.memory.index===op.destination);
          if(['legal destination address alias','wrapping effective address'].includes(a.label))assert.equal(addressOf(op.memory,before.registers),16768);
        }
        const out=generated(c,a);assert.equal(out.status,a.label.startsWith('same-byte stale')||a.label.startsWith('stale direct')?4:0,'only declared same-byte controls may be stale');assert.equal(out.retired,a.planned_retired,`prospective retirement ${a.id} ${a.label}`);
        retired+=out.retired;positive+=Number(out.retired>0);callsN++;if(u.foreign)foreignN+=out.addresses.length;
      }else if(a.kind==='guard')assert.equal(a.wrong_key,true);else if(a.kind==='close')c.closed=true;else assert.fail('unknown plan action');
    }
  }
  assert.equal(coverage.size,320);assert.equal(retired,732);assert.equal(callsN,588);assert.equal(positive,492);assert.equal(foreignN,6);assert.deepEqual(late.map(x=>x.units),[1,1,4,2]);assert.ok([...contexts.values()].every(c=>c.closed));
  return{calls:callsN,positive,zero:callsN-positive,retired,foreign_calls:foreignN,late_candidate_headroom:late,counts:expected};
}
const reviewedPaths = [
  'engine/src/cpu/x86/ir.rs','engine/src/cpu/x86/decode/profile.rs','engine/src/cpu/x86/decode/lower.rs','engine/src/cpu/dbt/wasm/integer.rs',
  'engine/tests/cpu_word_adc.rs','engine/tests/cpu_word_sbb.rs','engine/tests/cpu_register_memory_byte_carry.rs','engine/tests/cpu_word_memory_arithmetic.rs',
  'engine/tests/cpu_word_memory_carry.rs','engine/tests/cpu_word_memory_carry_wasm.rs',
  ...['plan','run','check'].map(name=>`engine/tests/fixtures/p2-word-memory-carry/${name}.mjs`),
  'target/word-memory-carry-session/independent-word-memory-carry.py','target/word-memory-carry-session/supervise.py',
  'target/word-memory-carry-session/canonical_guard_v1.py','target/word-memory-carry-session/contract-v1.md'
].sort();
function sourceAuthority(freeze,root){
  assert.equal(freeze.status,'source-reviewed');assert.deepEqual(Object.keys(freeze.engine).sort(),['bytes','sha256']);
  for(const name of ['engine_path','plan_path','capture_path','node_path'])assert.ok(isAbsolute(freeze[name]));
  assert.deepEqual(pin(readFileSync(regularFile(freeze.engine_path))),freeze.engine);assert.deepEqual(pin(readFileSync(regularFile(freeze.plan_path))),freeze.plan);
  assert.deepEqual(Object.keys(freeze.node).sort(),['bytes','sha256']);assert.deepEqual(pin(readFileSync(regularFile(freeze.node_path))),freeze.node);
  const rows=Object.keys(freeze.source_pins).sort().map(path=>{assert.deepEqual(pin(readFileSync(safeFile(root,path))),freeze.source_pins[path]);return{path,...freeze.source_pins[path]};});
  for(const path of reviewedPaths)assert.ok(Object.hasOwn(freeze.source_pins,path),'all reviewed owners are in the frozen source authority');
  for(const name of ['historical_artifacts','protected_artifacts']){assert.ok(freeze[name]&&typeof freeze[name]==='object'&&!Array.isArray(freeze[name]));for(const[path,expected]of Object.entries(freeze[name]))assert.deepEqual(pin(readFileSync(safeFile(root,path))),expected,`${name}: ${path}`);}
  assert.equal(freeze.reviews.length,3);const expectedReviews=Object.fromEntries(reviewedPaths.map(path=>[path,freeze.source_pins[path]]));
  for(const r of freeze.reviews){assert.equal(r.ack,true);const b=readFileSync(safeFile(root,r.index));assert.deepEqual(pin(b),{bytes:r.bytes,sha256:r.sha256});const review=JSON.parse(b);assert.equal(review.ack,true);assert.deepEqual(review.survivors,[]);assert.deepEqual(review.source_pins,expectedReviews,'three reviews bind the exact current seventeen owners');}
  return rows;
}
function nodeRuntime(freeze,command,environment,root,directory){
  assert.deepEqual(command,[freeze.node_path,join(root,'engine/tests/fixtures/p2-word-memory-carry/run.mjs'),freeze.engine_path,directory,root,freeze.capture_path]);
  assert.equal(environment.executable,freeze.node_path);assert.deepEqual(environment.exec_argv,[]);
}
function executionAuthority(root,directory,freezePath,freezeBytes,freeze,result){
  const bankDirectory=dirname(freezePath),bankRelative=relative(root,bankDirectory),supervisorPath=`${bankRelative}/supervise.py`,guardPath=`${bankRelative}/canonical_guard_v1.py`;
  assert.equal(bankRelative,'target/word-memory-carry-session');
  const supervisionPath=join(bankDirectory,'freeze-actual-supervision.json');assert.equal(freeze.actual_supervision_freeze_path,supervisionPath);
  const supervisionBytes=readFileSync(regularFile(supervisionPath)),supervision=JSON.parse(supervisionBytes);
  assert.equal(supervision.ack,true);assert.equal(supervision.deadline_seconds,600);assert.deepEqual(supervision.sources,freeze.source_pins);
  assert.deepEqual(supervision.supervisor,freeze.source_pins[supervisorPath]);assert.deepEqual(supervision.supervisor,pin(readFileSync(safeFile(root,supervisorPath))));
  for(const name of ['sources','artifacts']){assert.ok(supervision[name]&&typeof supervision[name]==='object'&&!Array.isArray(supervision[name]));for(const[path,expected]of Object.entries(supervision[name]))assert.deepEqual(pin(readFileSync(safeFile(root,path))),expected,`supervision ${name}: ${path}`);}
  const protectedPin=(path,expected)=>{assert.deepEqual(pin(readFileSync(safeFile(root,path))),expected);assert.ok([freeze.protected_artifacts[path],supervision.artifacts[path]].some(p=>p&&p.bytes===expected.bytes&&p.sha256===expected.sha256),`protected relative evidence: ${path}`);};
  assert.deepEqual(supervision.artifacts[relative(root,freezePath)],pin(freezeBytes));
  for(const path of Object.keys(freeze.source_pins))assert.deepEqual(supervision.sources[path],freeze.source_pins[path]);
  protectedPin(relative(root,freeze.engine_path),freeze.engine);for(const[file,expected]of Object.entries(freeze.capture_pins))protectedPin(relative(root,join(freeze.capture_path,file)),expected);
  const nodeAuthorityPath=`${bankRelative}/node-executable-authority-v1.json`,nodeAuthorityBytes=readFileSync(safeFile(root,nodeAuthorityPath));protectedPin(nodeAuthorityPath,pin(nodeAuthorityBytes));
  assert.deepEqual(JSON.parse(nodeAuthorityBytes),{node_path:freeze.node_path,node:freeze.node});
  const expectedEnvironment={NODE_OPTIONS:'',RING3_ENGINE_SHA256:freeze.engine.sha256,RING3_WORD_MEMORY_CARRY_FREEZE:freezePath};assert.deepEqual(supervision.environment,expectedEnvironment);
  const startedBytes=readFileSync(safeFile(bankDirectory,'actual-first.started.json')),started=JSON.parse(startedBytes),supervisedBytes=readFileSync(safeFile(bankDirectory,'actual-first.result.json')),supervised=JSON.parse(supervisedBytes);
  assert.deepEqual(started.freeze,pin(supervisionBytes));assert.deepEqual(started.command,supervision.command);nodeRuntime(freeze,started.command,result.environment,root,directory);
  assert.equal(started.deadline_seconds,600);assert.equal(started.cleanup_reserve_seconds,5);assert.deepEqual(started.environment_keys,Object.keys(expectedEnvironment).sort());assert.deepEqual(started.process_preflight_conflicts,[]);assert.match(started.head,/^[a-f0-9]{40}$/);assert.equal(typeof started.started_at,'string');
  for(const[k,v]of Object.entries(started))assert.deepEqual(supervised[k],v);
  assert.equal(supervised.exit_code,0);assert.equal(supervised.timeout,false);assert.equal(supervised.not_started,false);assert.equal(supervised.reaped,true);assert.equal(supervised.error,null);assert.deepEqual(supervised.changed_pins,[]);assert.deepEqual(supervised.pin_errors,[]);uint(supervised.wall_ns,600e9);
  for(const stream of ['stdout','stderr'])assert.deepEqual(supervised[stream],pin(readFileSync(safeFile(bankDirectory,`actual-first.${stream}`))));
  const proofNames=['final-native-green-proof.json','old-module-byte-identity-proof.json','raw-input-native-proof.json','format-clippy-proof.json'];
  assert.deepEqual(freeze.native_proofs.map(r=>r.path),proofNames.map(name=>`${bankRelative}/${name}`));
  const proofs=new Map();for(const row of [...freeze.native_proofs,freeze.build_proof]){protectedPin(row.path,{bytes:row.bytes,sha256:row.sha256});const proof=JSON.parse(readFileSync(safeFile(root,row.path)));assert.equal(proof.ack,true);for(const[path,expected]of Object.entries(proof.source_pins??{}))assert.deepEqual(freeze.source_pins[path],expected);proofs.set(row.path,proof);}
  assert.equal(freeze.build_proof.path,`${bankRelative}/engine-build-proof.json`);const build=proofs.get(freeze.build_proof.path);assert.deepEqual(build.engine,freeze.engine);assert.equal(build.engine_path,freeze.engine_path);
  const rawProof=proofs.get(`${bankRelative}/raw-input-native-proof.json`);assert.equal(rawProof.capture_path,freeze.capture_path);assert.deepEqual(rawProof.capture_pins,freeze.capture_pins);assert.equal(rawProof.physical_files,4);
  const oldProof=proofs.get(`${bankRelative}/old-module-byte-identity-proof.json`);assert.equal(oldProof.old_modules,18);assert.equal(oldProof.old_files,23);assert.equal(oldProof.whole_files_identical,true);
  const authorityPath=`${bankRelative}/canonical-guard-authority.json`,authorityBytes=readFileSync(safeFile(root,authorityPath)),authority=JSON.parse(authorityBytes);protectedPin(authorityPath,pin(authorityBytes));
  assert.equal(authority.ack,true);assert.deepEqual(authority.guard,freeze.source_pins[guardPath]);assert.deepEqual(authority.supervisor,freeze.source_pins[supervisorPath]);
  assert.equal(authority.canonical.path,`${bankRelative}/canonical-preactual-readback.json`);const canonicalBytes=readFileSync(safeFile(root,authority.canonical.path));protectedPin(authority.canonical.path,{bytes:authority.canonical.bytes,sha256:authority.canonical.sha256});const canonical=JSON.parse(canonicalBytes);assert.equal(canonical.ack,true);assert.equal(canonical.checked,3);assert.equal(canonical.open,3);
  const vault='/Users/fatihguzel/Obsidian/personal/fatihguzel/projects/ring3',names=['00_project_index.md','founder_report.md','linear_setup.md','development/2026-10-10.md','research/prototype2/2026-10-10/06_word_memory_carry.md'];
  assert.deepEqual(Object.keys(authority.five_notes).sort(),names.map(n=>`${vault}/${n}`).sort());assert.deepEqual(Object.keys(canonical.five_notes).sort(),Object.keys(authority.five_notes).sort());
  const notePins={};for(const[identity,row]of Object.entries(authority.five_notes)){assert.ok(isAbsolute(identity));assert.equal(isAbsolute(row.snapshot),false);assert.ok(!row.snapshot.split('/').includes('..'));const snapshot=safeFile(bankDirectory,row.snapshot);protectedPin(relative(root,snapshot),row.pin);assert.deepEqual(row.pin,canonical.five_notes[identity]);notePins[identity]=row.pin;}
  const beforeBytes=readFileSync(safeFile(bankDirectory,'canonical-guard-before.json')),before=JSON.parse(beforeBytes),afterBytes=readFileSync(safeFile(bankDirectory,'canonical-guard-after.json')),after=JSON.parse(afterBytes);
  assert.equal(before.ack,true);assert.deepEqual(before.authority,pin(authorityBytes));assert.deepEqual(before.five_notes,notePins);
  assert.equal(after.ack,true);assert.equal(after.before,true);assert.equal(after.after,true);assert.equal(after.inner_exit,0);assert.deepEqual(after.errors,[]);assert.equal(after.acceptance_ceiling_seconds,610);uint(after.wall_ns,610e9);assert.deepEqual(after.inner,pin(supervisedBytes));assert.deepEqual(after.authority,pin(authorityBytes));assert.deepEqual(after.five_notes,notePins);
  return{supervision:pin(supervisionBytes),started:pin(startedBytes),result:pin(supervisedBytes),canonical_authority:pin(authorityBytes),canonical_before:pin(beforeBytes),canonical_after:pin(afterBytes),node:freeze.node,live_note_observation:'execution-only guard pre/post pins plus immutable snapshot bytes; absolute note keys are receipt identities, never relative source-map paths'};
}
export function checkCapsule(directory,freezePath){
  directory=resolve(directory);freezePath=resolve(freezePath);assert.equal(realpathSync(directory),directory);assert.ok(lstatSync(directory).isDirectory());
  const root=resolve(dirname(fileURLToPath(import.meta.url)),'../../../..'),freezeBytes=readFileSync(regularFile(freezePath)),freeze=JSON.parse(freezeBytes),sources=sourceAuthority(freeze,root);
  const resultBytes=readFileSync(safeFile(directory,'result.json')),result=JSON.parse(resultBytes),manifest=JSON.parse(readFileSync(safeFile(directory,'manifest.json')));
  assert.equal(result.schema_version,1);assert.equal(result.status,'complete');assert.equal(result.failure,null);assert.equal(manifest.status,'recording');for(const key of Object.keys(manifest).filter(k=>k!=='status'))assert.deepEqual(result[key],manifest[key]);
  assert.deepEqual(result.source_pins_before,sources);assert.deepEqual(result.source_pins_after,sources);assert.deepEqual(result.engine,freeze.engine);
  const planBytes=readFileSync(safeFile(directory,result.plan.file));assert.equal(result.plan.file,'plan.json');assert.equal(hash(planBytes),result.plan.sha256);assert.deepEqual(pin(planBytes),freeze.plan);assert.deepEqual(planBytes,readFileSync(freeze.plan_path));const plan=JSON.parse(planBytes),prediction=validatePlan(plan);assert.deepEqual(result.planned_counts,plan.counts);
  const captures=['bank.x86','capture.json','data.bin','initial-arena.bin'];assert.deepEqual(Object.keys(freeze.capture_pins).sort(),captures);
  assert.deepEqual(readdirSync(freeze.capture_path).sort(),captures);assert.deepEqual(result.capture_pins,captures.map(file=>({file,...freeze.capture_pins[file]})));
  for(const file of captures){assert.deepEqual(pin(readFileSync(safeFile(directory,file))),freeze.capture_pins[file]);assert.deepEqual(readFileSync(safeFile(directory,file)),readFileSync(safeFile(freeze.capture_path,file)));}
  assert.deepEqual(readFileSync(safeFile(directory,'bank.x86')),literalBank());assert.deepEqual(readFileSync(safeFile(directory,'data.bin')),literalData());assert.deepEqual(readFileSync(safeFile(directory,'initial-arena.bin')),defaultArena());
  const capture=JSON.parse(readFileSync(safeFile(directory,'capture.json')));assert.deepEqual(capture,{schema_version:1,raw_inputs_only:true,groups:literalGroups,files:[{file:'bank.x86',bytes:4096},{file:'data.bin',bytes:4096},{file:'initial-arena.bin',bytes:SIZE}]});
  const engineInterface=wasmInterface(readFileSync(freeze.engine_path));assert.deepEqual(engineInterface.imports,[]);assert.deepEqual(result.engine_imports,[]);assert.deepEqual(result.engine_exports,engineInterface.exports.map(({name,kind})=>({name,kind})));
  const abi=readFileSync(join(root,'engine/src/abi/wasm/exports.rs'),'utf8');assert.deepEqual(result.engine_exports.map(e=>e.name).sort(),['memory',...[...abi.matchAll(/export_name = "([^"]+)"/g)].map(m=>m[1])].sort());
  const raw=fileRecord(directory,result.raw),journalBytes=fileRecord(directory,result.journal),journal=ndjson(journalBytes,result.journal.rows),checkpoints=ndjson(fileRecord(directory,result.checkpoints),result.checkpoints.rows);
  assert.equal(raw.length,result.frames.length*SIZE);assert.deepEqual(journal.filter(r=>r.kind==='event').map(r=>r.event),result.events);assert.deepEqual(journal.filter(r=>r.kind==='frame').map(({kind,...r})=>r),result.frames);assert.deepEqual(journal.filter(r=>r.kind==='module_file').map(({kind,...r})=>r),result.modules);assert.deepEqual(journal.filter(r=>r.kind==='data_file').map(({kind,...r})=>r),result.data_files);
  const modules=new Map(result.modules.map(r=>[r.file,r])),dataFiles=new Map(result.data_files.map(r=>[r.file,r]));assert.equal(modules.size,prediction.counts.modules);assert.equal(dataFiles.size,prediction.counts.data_files);
  const files=new Set([...captures,'plan.json','manifest.json','result.json','arenas.bin','journal.ndjson','checkpoints.ndjson']);
  for(const row of result.modules){const b=readFileSync(safeFile(directory,row.file));files.add(row.file);assert.deepEqual(pin(b),{bytes:row.bytes_length,sha256:row.sha256});assert.deepEqual(moduleInterface(b,row.owner,row.target==='dispatcher'),{imports:row.imports,exports:row.exports});moduleBinding(b,row,plan.contexts.find(c=>c.id===row.context));}
  for(const row of result.data_files){files.add(row.file);assert.deepEqual(pin(readFileSync(safeFile(directory,row.file))),{bytes:row.bytes_length,sha256:row.sha256});assert.ok([1,1024].includes(row.count));assert.equal(row.bytes_length,row.count*4);}
  assert.equal(files.size,prediction.counts.files);assert.deepEqual(readdirSync(directory).sort(),[...files].sort());assert.equal(result.contexts.length,7);assert.equal(new Set(result.contexts.map(c=>c.id)).size,7);
  const contexts=new Map(),seenModules=new Set(),seenData=new Set(),wantedJournal=[],snapshots=new Map(),groups=new Map(literalGroups.map(g=>[g.id,g]));
  const counts={contexts:0,engine_instances:0,opened:0,closed:0,generated_calls:0,guard_calls:0,host_api_calls:0,inputs:0,tables:0,child_modules:0,dispatchers:0,events:0,frames:0,data_files:0,diagnostic_reads:0,foreign_helper_calls:0};
  const statuses={};let frameIndex=0,positive=0,retired=0;
  function frame(e,side,c){const index=e[side];assert.equal(index,frameIndex);const row=result.frames[frameIndex++];assert.deepEqual({index:row.index,offset:row.offset,length:row.length,context:row.context,event:row.event,side:row.side},{index,offset:index*SIZE,length:SIZE,context:e.context,event:e.action_id,side});assert.ok(Number.isInteger(row.memory_bytes)&&row.memory_bytes%65536===0&&row.memory_bytes>=c.base+SIZE);assert.equal(row.memory_bytes,e[`memory_bytes_${side}`]);assert.ok(row.memory_bytes>=(c.memory_bytes??0));c.memory_bytes=row.memory_bytes;const b=raw.subarray(index*SIZE,(index+1)*SIZE);assert.equal(hash(b),row.sha256);assert.deepEqual(b,c.arena,`full 4364 arena ${e.action_id}/${side}`);counts.frames++;wantedJournal.push({kind:'frame',...row});}
  function refs(e,c,field='reference_snapshot'){const r=e[field];assert.ok(r);assert.equal(r.memory_same,true);assert.deepEqual(r.table_slots,c.owner==='resident'?Array.from({length:8},(_,slot)=>({slot,group:c.table[slot],reference_equal:true})):null);assert.deepEqual(r.live_module_bytes,[...c.live.values()].map(u=>({file:u.file,pointer:u.pointer,bytes_length:u.bytes_length,sha256:u.sha256})));}
  assert.equal(result.events.length,plan.actions.length);
  for(const[index,a]of plan.actions.entries()){
    const e=result.events[index];assert.ok(!e.error&&!e.capture_error);for(const[k,v]of Object.entries(a))if(!['id','args'].includes(k))assert.deepEqual(e[k],v);assert.equal(e.action_id,index);let c=contexts.get(a.context);
    if(a.kind==='open'){
      const spec=plan.contexts.find(s=>s.id===a.context);assert.ok(!c);uint(e.arena_ptr);assert.ok(e.arena_ptr>0);c=fresh(spec,e.arena_ptr);contexts.set(c.id,c);assert.equal(e.before,null);assert.deepEqual(e.args,[c.pages,...c.key]);assert.equal(e.status,0);counts.contexts++;counts.engine_instances++;counts.opened++;counts.host_api_calls+=2;frame(e,'after',c);assert.deepEqual(result.contexts.find(s=>s.id===c.id),{...spec,base:c.base,memory_bytes:e.memory_bytes_after,open_event:e.action_id});
    }else if(a.kind==='closed_call'){
      assert.ok(c.closed);assert.equal(e.before,null);assert.equal(e.after,null);assert.deepEqual(e.args,[c.base,c.base+56,a.budget,c.base+96]);assert.equal(e.status,5);assert.equal(e.completed_calls,1);assert.equal(e.foreign_helper,undefined);counts.generated_calls++;statuses['closed:5']=(statuses['closed:5']??0)+1;
    }else{
      assert.ok(c&&!c.closed);frame(e,'before',c);
      if(a.kind==='input'){const b=Buffer.from(a.hex,'hex');if(b.length===SIZE)assert.deepEqual(b,authoredArena());b.copy(c.arena,a.offset);if(a.offset===0)state(c.arena);counts.inputs++;}
      else if(a.kind==='host'){
        if(a.name.startsWith('compile'))refs(e,c,'reference_snapshot_before');hostStep(c,a,e,groups,true);counts.host_api_calls++;if(a.name.startsWith('compile')&&c.owner==='replacement')counts.host_api_calls+=3;
      }else if(a.kind==='module'){
        const row=modules.get(a.file);assert.ok(row&&!seenModules.has(row.file));seenModules.add(row.file);for(const k of ['context','target','group','slot','helper_case'])assert.deepEqual(row[k],a[k]);assert.equal(row.owner,c.owner);assert.equal(row.entries,c.entries);
        const m=a.target==='dispatcher'?c.dispatchMetadata:c.pending;for(const k of ['pointer','generation','id_low','id_high','bytes_length'])assert.deepEqual(row[k],m[k]);assert.ok(row.pointer>0&&row.pointer+row.bytes_length<=e.memory_bytes_after);assert.ok(row.pointer+row.bytes_length<=c.base||row.pointer>=c.base+SIZE,'module excludes arena');for(const[group,old]of c.live)if(group!==a.group)assert.ok(row.pointer+row.bytes_length<=old.pointer||old.pointer+old.bytes_length<=row.pointer,'live module allocations disjoint');assert.equal(e.module_file,row.file);assert.equal(e.module_copy_sha256,row.sha256);c.live.set(a.group,row);
        if(a.target==='child'){c.units.get(a.group).foreign=a.helper_case===null?null:plan.helper_cases.find(h=>h.id===a.helper_case);counts.child_modules++;}else counts.dispatchers++;wantedJournal.push({kind:'module_file',...row});
      }else if(a.kind==='table'){assert.ok(c.units.has(a.group));uint(a.slot,7);c.table[a.slot]=a.group;assert.equal(e.reference_equal,true);counts.tables++;}
      else if(a.kind==='data'){
        const row=dataFiles.get(a.file);assert.ok(row&&!seenData.has(row.file));seenData.add(row.file);assert.deepEqual(row,{context:c.id,event:a.id,file:a.file,address:a.address,count:a.count,bytes_length:a.count*4,sha256:row.sha256});assert.equal(e.data_file,row.file);assert.equal(e.data_sha256,row.sha256);const b=Buffer.alloc(a.count*4),reads=[];
        for(let i=0;i<a.count;i++){const r=read32(c,a.address+4*i);reads.push(r);b.writeUInt32LE(r.value,i*4);}assert.deepEqual(e.reads,reads,'every diagnostic status/value/helper packet');assert.deepEqual(readFileSync(safeFile(directory,row.file)),b);counts.host_api_calls+=a.count;counts.diagnostic_reads+=a.count;counts.data_files++;wantedJournal.push({kind:'data_file',...row});
      }else if(a.kind==='call'){
        assert.deepEqual(e.args,[c.base,c.base+56,a.budget,c.base+96]);const u=c.units.get(a.group),out=generated(c,a);assert.equal(e.status,out.status);assert.equal(e.completed_calls,1);assert.equal(out.retired,a.planned_retired);retired+=out.retired;positive+=Number(out.retired>0);counts.generated_calls++;const name=`${a.channel}:${out.status}`;statuses[name]=(statuses[name]??0)+1;
        if(u?.foreign){assert.deepEqual(e.foreign_helper,{addresses:out.addresses,status:u.foreign.status,packet_hex:u.foreign.packet_hex});counts.foreign_helper_calls+=out.addresses.length;}else assert.equal(e.foreign_helper,undefined);
      }else if(a.kind==='guard'){
        const u=c.units.get(a.group),key=[c.key[0]^1,c.key[1]],name=c.owner==='resident'?'guard_resident':'guard';assert.equal(e.name,name);assert.deepEqual(e.args,c.owner==='resident'?[...key,u.id_low,u.id_high,c.base,c.base+56,c.base+96]:[...key,u.generation,c.base,c.base+56,c.base+96]);assert.equal(e.status,3);counts.guard_calls++;counts.host_api_calls++;
      }else if(a.kind==='close'){refs(e,c);assert.equal(e.status,0);c.closed=true;counts.closed++;counts.host_api_calls++;}else assert.fail('unrecognized physical action');
      if(c.closed)assert.equal(e.after,null);else{frame(e,'after',c);if(['module','table','call','guard','host','data'].includes(a.kind))refs(e,c);}
    }
    counts.events++;wantedJournal.push({kind:'event',event:e});snapshots.set(e.action_id,{...counts});
  }
  assert.deepEqual(journal,wantedJournal);assert.equal(frameIndex,result.frames.length);assert.equal(seenModules.size,prediction.counts.modules);assert.equal(seenData.size,prediction.counts.data_files);assert.ok([...contexts.values()].every(c=>c.closed));assert.deepEqual(result.observed_counts,counts);assert.deepEqual(result.observed_statuses,statuses);assert.equal(positive,prediction.positive);assert.equal(retired,prediction.retired);assert.equal(raw.length,prediction.counts.raw_bytes);assert.equal(journal.length,prediction.counts.frames+prediction.counts.actions+prediction.counts.modules+prediction.counts.data_files);
  const checkpointEvents=plan.actions.filter(a=>['call','closed_call','guard','close','data'].includes(a.kind)).map(a=>a.id).concat(plan.actions.at(-1).id);assert.deepEqual(checkpoints.map(r=>r.last_event),checkpointEvents);assert.equal(checkpoints.length,prediction.counts.checkpoints);
  const offsets=new Map();let end=0;for(const line of journalBytes.toString('utf8').slice(0,-1).split('\n')){end+=Buffer.byteLength(line)+1;const r=JSON.parse(line);if(r.kind==='event')offsets.set(r.event.action_id,end);}for(const[i,r]of checkpoints.entries()){const s=snapshots.get(r.last_event);assert.deepEqual(r,{index:i,events:s.events,frames:s.frames,raw_bytes:s.frames*SIZE,journal_bytes:offsets.get(r.last_event),last_event:r.last_event,observed_counts:s});}
  const execution=executionAuthority(root,directory,freezePath,freezeBytes,freeze,result);
  return{schema_version:1,complete:true,semantic_certification:true,freeze_sha256:hash(freezeBytes),result_sha256:hash(resultBytes),raw_sha256:hash(raw),plan_sha256:hash(planBytes),engine:freeze.engine,execution,counts,positive,zero:counts.generated_calls-positive,independently_replayed_instructions:retired,physical_files:files.size,journal_rows:journal.length,checkpoint_rows:checkpoints.length,observation_limit:'finite authored WORD register from memory ADC/SBB, FLAGS/EA/RAM/owner proof; full frames only before/after each diagnostic sweep or watched-word read, per-read helper packets visible; no intermediate diagnostic full-arena, arbitrary helper side-effect rollback, exhaustive ISA, platform or performance claim'};
}
export function inspectIncomplete(directory){directory=resolve(directory);const files=readdirSync(directory).sort().map(file=>({file,...pin(readFileSync(safeFile(directory,file)))})),raw=files.find(r=>r.file==='arenas.bin');return{schema_version:1,complete:false,semantic_certification:false,files,available_full_frames:Math.floor((raw?.bytes??0)/SIZE),trailing_frame_bytes:(raw?.bytes??0)%SIZE,observation_limit:'uncertified physical inventory; no partial semantic replay'};}


function selfTest() {
  let assertions=0;
  const same=(a,b)=>{assert.deepEqual(a,b);assertions++;},reject=fn=>{assert.throws(fn);assertions++;};
  {const freeze={node_path:'/pinned/node',engine_path:'/root/engine.wasm',capture_path:'/root/capture'},root='/root',output='/root/output',command=[freeze.node_path,root+'/engine/tests/fixtures/p2-word-memory-carry/run.mjs',freeze.engine_path,output,root,freeze.capture_path],environment={executable:freeze.node_path,exec_argv:[]};
    nodeRuntime(freeze,command,environment,root,output);assertions++;reject(()=>nodeRuntime(freeze,['node',...command.slice(1)],environment,root,output));reject(()=>nodeRuntime(freeze,command,{...environment,executable:'/other/node'},root,output));reject(()=>nodeRuntime(freeze,command,{...environment,exec_argv:['--inspect']},root,output));}
  const masks=[0xffff0000,0xffff,0xaaaaaaaa,0x55555555,0xf0f0f0f0,0x0f0f0f0f,0xfafafafa,0x05050505,0xff00ff00,0x00ff00ff,0xcccccccc,0x33333333,0x00ffff00,0xff0000ff,0xf0fffff0,0x0f00000f];
  for(let cc=0;cc<16;cc++)for(let n=0;n<32;n++){const f=2+[1,4,64,128,2048].reduce((v,b,i)=>v+(n>>i&1)*b,0);same(Number(predicate(cc,f)),Math.floor(masks[cc]/2**n)%2);same(predicate(cc,f),predicate(cc,f|1040));}
  for(const[sub,left,right,carry,result,flags]of[
    [false,0,0,0,0,0x46],[false,0x7fff,1,0,0x8000,0x896],[false,0xffff,1,0,0,0x57],
    [false,0x8000,0x8000,0,0,0x847],[false,15,1,0,16,0x12],[false,255,1,0,256,0x16],
    [true,0,0,0,0,0x46],[true,0x8000,1,0,0x7fff,0x816],[true,0,1,0,0xffff,0x97],
    [true,0x7fff,0xffff,0,0x8000,0x887],[true,16,1,0,15,0x16],[true,0x101,1,0,0x100,6],
    [false,0xffff,0,1,0,0x57],[false,0x7fff,0,1,0x8000,0x896],[false,0,0xffff,1,0,0x57],[true,0,0xffff,1,0,0x57],[true,0x1234,0x1234,1,0xffff,0x97],
    [true,0x8000,0,1,0x7fff,0x816],[false,0xffff,0xffff,1,0xffff,0x97]
  ]){same(wordArithmetic(left,right,2,sub,carry),{result,flags});same(wordArithmetic(left,right,0xcd7,sub,carry),{result,flags:flags|0x400});}
  const setup=()=>{const c=fresh({id:1,owner:'replacement',entries:false,key:[17,19]});c.arena=authoredArena();c.ram.map(CODE,1,7);c.ram.write(CODE,literalBank());c.ram.protect(CODE,1,5);c.ram.map(DATA,1,3);c.ram.write(DATA,literalData());c.ram.protect(DATA,1,1);return c;};
  const install=(c,blocks,group='one',extra={})=>{const b=Buffer.alloc(blocks.length*8);blocks.forEach(([pc,n],i)=>{b.writeUInt32LE(pc,i*8);b.writeUInt32LE(n,i*8+4);});b.copy(c.arena,140);const u={...compileDescriptors(c,{name:'compile',args:[blocks.length]}),...extra};c.units.set(group,u);return u;};
  for(const [hex,value]of[['661303',0xa1018000],['661b03',0xa1017ffe]]){
    const op=decodeAt(Buffer.from(hex,'hex'),0,CODE);same(op.memory,{base:3,index:null,scale:1,displacement:0});same(op.destination,0);
    const c=setup(),cpu=state(c.arena);cpu.flags=3;cpu.registers[0]=0xa1017fff;cpu.registers[3]=DATA+770;const before=structuredClone(cpu),out=execute(c,op,cpu,null,[]);
    same(out.cpu.registers[0],value);same(out.cpu.registers.slice(1),before.registers.slice(1));same(cpu,before);same(c.arena.subarray(100,140),record('R3MH',40,[0,0,0,0,0,2],2));
  }
  const op=decodeAt(Buffer.from('661303','hex'),0,CODE);
  for(const bits of[0,2,4]){const c=setup();c.ram.protect(DATA,1,bits);const cpu=state(c.arena);cpu.registers[3]=DATA;const before=structuredClone(cpu);same(execute(c,op,cpu,null,[]),{fault:{reason:5,detail:2,address:DATA,access:1,length:2}});same(cpu,before);}
  {const c=setup(),cpu=state(c.arena);cpu.registers[3]=DATA+4095;same(execute(c,op,cpu,null,[]),{fault:{reason:5,detail:1,address:DATA+4096,access:1,length:2}});c.ram.map(DATA+4096,1,3);c.ram.write(DATA+4096,literalData());c.ram.protect(DATA+4096,1,1);same(execute(c,op,cpu,null,[]).cpu.pc,CODE+3);same(c.ram.read(DATA+4095,2).bytes,Buffer.from([194,0]));}
  {const c=setup(),cpu=state(c.arena);cpu.registers[3]=U32-1;same(execute(c,op,cpu,null,[]),{fault:{reason:5,detail:3,address:U32-1,access:1,length:2}});c.ram.map(U32-4096,1,3);c.ram.write(U32-4096,literalData());c.ram.protect(U32-4096,1,1);cpu.registers[3]=U32-2;same(execute(c,op,cpu,null,[]).cpu.pc,CODE+3);same(c.ram.read(U32-2,2).bytes,Buffer.from([121,194]));}
  for(const h of literalHelpers()){const c=setup(),cpu=state(c.arena);cpu.registers[3]=DATA;const before=structuredClone(cpu);same(execute(c,op,cpu,h,[]),{fault:{reason:7,detail:['status','infrastructure'].includes(h.id)?3:2}});same(cpu,before);same(c.arena.subarray(100,140).toString('hex'),h.packet_hex);}
  for(let path=0;path<2;path++){
    const c=setup(),pc=6144+path*64;install(c,[[pc,32],[pc+34,9]]);Buffer.from(literalSeed(pc,0xcd7,{3:17152+path*2,7:0xa808ffff}),'hex').copy(c.arena);
    const before=Buffer.from(c.arena),out=generated(c,{channel:'direct',group:'one',budget:14});same(out.retired,13);same(out.reason,3);
    const want=Buffer.from(before);record('R3ST',56,[path?0xa101fffe:0xa1017fff,path?0xa2020000:0xa2020101,path?0xa3030100:0xa3030000,path?0:257,0xa5058000,0xa6061234,0xa707aaaa,path?0xa808ffff:0xa8080000,pc+43,path?0x486:0x457]).copy(want);
    record('R3EX',40,[3,13,0,0,0,0],2).copy(want,56);record('R3MH',40,[0,path?0:65535,0,0,0,2],2).copy(want,100);same(c.arena,want);
    before.copy(c.arena);for(const n of[3,6,4])same(generated(c,{channel:'direct',group:'one',budget:n}).retired,n);same(state(c.arena),state(want));same(c.arena.subarray(56,96),record('R3EX',40,[1,4,0,0,0,0],2));same(generated(c,{channel:'direct',group:'one',budget:1}).retired,0);
    const bad=Buffer.from(want);bad[4363]^=1;reject(()=>assert.deepEqual(bad,want));
  }
  for(let path=0;path<2;path++){
    const c=setup(),pc=6656+path*64;install(c,[[pc,19],[pc+21,2]]);const seed=Buffer.from(literalSeed(pc,0xcd7,{0:path?0xa1017fff:0xa1018001,3:path?0xa4044202:0xa4044100}),'hex');seed.copy(c.arena);const before=Buffer.from(c.arena);
    same(generated(c,{channel:'direct',group:'one',budget:6}).retired,5);const want=Buffer.from(before);record('R3ST',56,[0xa1010100,0xa2020001,0xa3037fff,path?0xa4044200:0xa4044102,0xa5058000,0xa6061234,0xa707aaaa,0xa8085555,pc+23,path?0x457:0x446]).copy(want);record('R3EX',40,[3,5,0,0,0,0],2).copy(want,56);record('R3MH',40,[0,32769,0,0,0,2],2).copy(want,100);same(c.arena,want);
    before.copy(c.arena);for(const n of[1,1,3])same(generated(c,{channel:'direct',group:'one',budget:n}).retired,n);same(state(c.arena),state(want));same(generated(c,{channel:'direct',group:'one',budget:1}).retired,0);
  }
  for(const flags of[0xcd6,0xcd7]){const c=setup();install(c,[[5632,4]]);Buffer.from(literalSeed(5632,flags,{0:0xb101ffff,3:32768}),'hex').copy(c.arena);const before=Buffer.from(c.arena);same(generated(c,{channel:'direct',group:'one',budget:20}).retired,1);same(state(c.arena),{...state(before),pc:5633});same(c.arena.subarray(56,96),record('R3EX',40,[5,1,1,32768,1,2],2));same(generated(c,{channel:'direct',group:'one',budget:1}).retired,0);same(state(c.arena).flags,flags);c.ram.map(32768,1,3);c.ram.write(32768,Buffer.from([1,128]));c.ram.protect(32768,1,1);same(generated(c,{channel:'direct',group:'one',budget:1}).retired,1);same(state(c.arena).registers[0],flags%2?0xb1018001:0xb1018000);same(c.arena.subarray(4236),before.subarray(4236));}
  for(const entries of[false,true]){const c=setup(),a={name:entries?'compile_entries':'compile',args:[1]};c.arena.writeUInt32LE(6144,140);if(!entries)c.arena.writeUInt32LE(32,144);same(compileDescriptors(c,a).instructions.size,10);a.args[0]=2;c.arena.writeUInt32LE(7936,entries?144:148);if(!entries)c.arena.writeUInt32LE(2,152);reject(()=>compileDescriptors(c,a));}
  for(const channel of['direct','dispatch'])for(const cancel of[0,1]){
    const c=setup();install(c,[[5633,3]],'one',{installed:true,id_low:3,id_high:7,slot:0});c.arena.writeUInt32LE(5633,48);c.arena.writeUInt32LE(cancel,96);const before=Buffer.from(c.arena),out=generated(c,{channel,group:'one',budget:0});same(out.retired,0);same(out.reason,cancel?2:1);const want=Buffer.from(before);record('R3EX',40,[cancel?2:1,0,0,0,0,0],channel==='dispatch'?3:2).copy(want,56);same(c.arena,want);
    c.ram.protect(CODE,1,7);c.ram.write(CODE,Buffer.from([102]));const stale=Buffer.from(c.arena);same(generated(c,{channel,group:'one',budget:1}).status,channel==='dispatch'&&cancel?0:4);if(!(channel==='dispatch'&&cancel))same(c.arena,stale);
    const staleZero=Buffer.from(c.arena),zero=generated(c,{channel,group:'one',budget:0});same(zero.status,channel==='direct'?4:0);if(channel==='direct')same(c.arena,staleZero);else same(zero.reason,cancel?2:1);
    c.closed=true;same(generated(c,{channel,group:'one',budget:1}).status,5);
  }
  for(const hex of['66','6613','661b04','661305000000','66661303','67661303','f2661303','6613c3','661bc3','6683d7'])reject(()=>decodeAt(Buffer.from(hex,'hex'),0,CODE));
  for(const excess of['units','bytes']){
    const c=setup();c.owner='resident';const entries=excess==='bytes';c.arena.writeUInt32LE(6144,140);if(!entries)c.arena.writeUInt32LE(32,144);c.arena.writeUInt32LE(7936,entries?144:148);if(!entries)c.arena.writeUInt32LE(2,152);
    for(let i=0;i<(excess==='units'?5:1);i++)c.units.set('prior'+i,{bytes_length:excess==='bytes'?500000:100});
    reject(()=>hostStep(c,{name:entries?'compile_resident_entries':'compile_resident',args:entries?[2,0]:[2],group:'late-failure',allowed_failure:true},null,new Map(literalGroups.map(g=>[g.id,g])),false));
  }
  function signed(n){let value=BigInt.asIntN(32,BigInt(n));const bytes=[];for(;;){let byte=Number(value&127n);value>>=7n;const done=value===0n&&!(byte&64)||value===-1n&&Boolean(byte&64);if(!done)byte|=128;bytes.push(byte);if(done)return bytes;}}
  function leb(n){const b=[];do{let x=n%128;n=Math.floor(n/128);if(n)x|=128;b.push(x);}while(n);return b;}
  function boundBody(values,dispatch=false){const b=dispatch?[1,5,127]:[3,16,127,1,126,6,127];for(const n of values)b.push(65,...signed(n));b.push(32,0,32,1,32,3,16,0,34,dispatch?5:16,4,64,32,dispatch?5:16,15,11,dispatch?65:63,0,11);const section=[1,...leb(b.length),...b];return Buffer.from([0,97,115,109,1,0,0,0,10,...leb(section.length),...section]);}
  for(const owner of['replacement','resident']){const spec={key:[17,19]},row={owner,target:'child',generation:7,id_low:11,id_high:13},values=owner==='replacement'?[17,19,7]:[17,19,11,13],b=boundBody(values);moduleBinding(b,row,spec);assertions++;reject(()=>moduleBinding(b,row,{key:[18,19]}));reject(()=>moduleBinding(b,{...row,generation:8,id_low:12},spec));const bad=Buffer.from(b);bad[12]=15;reject(()=>moduleBinding(bad,row,spec));}
  moduleBinding(boundBody([17,19],true),{owner:'resident',target:'dispatcher'},{key:[17,19]});assertions++;
  return{pure_assertions:assertions,engine_or_guest_execution:false};
}
function mutationTest(plan) {
  const rejected=[];
  function reject(name,mutate){const copy=structuredClone(plan);mutate(copy);assert.throws(()=>validatePlan(copy),name);rejected.push(name);}
  validatePlan(plan);
  const callSeed=(p,label)=>{const a=p.actions.find(a=>a.kind==='call'&&a.label===label);assert.ok(a);const seed=p.actions[a.id-1];assert.equal(seed.kind,'input');return seed;};
  reject('raw operand-address byte',p=>{const b=Buffer.from(p.bank.hex,'hex');b[4]^=1;p.bank.hex=b.toString('hex');});
  reject('declared memory instruction truncated',p=>p.bank.groups[0].blocks[0][1]--);
  reject('frame census omitted',p=>p.counts.frames--);
  reject('missing owner',p=>p.contexts.pop());
  reject('wrong owner key',p=>p.contexts[0].key[0]++);
  reject('wrong-width helper changed into success',p=>{const b=Buffer.from(p.helper_cases[2].packet_hex,'hex');b.writeUInt32LE(2,36);p.helper_cases[2].packet_hex=b.toString('hex');});
  reject('dirty FLAGS/DF seed replaced with clean',p=>{const a=p.actions.find(a=>a.kind==='call'&&a.label==='edge form and FLAGS'&&p.actions[a.id-1].hex.slice(104,112)==='d70c0000');assert.ok(a);const seed=p.actions[a.id-1],b=Buffer.from(seed.hex,'hex');b.writeUInt32LE(2,52);seed.hex=b.toString('hex');});
  reject('full32 aliased address parent changed',p=>{const seed=callSeed(p,'legal destination address alias'),b=Buffer.from(seed.hex,'hex'),f=p.bank.forms[p.actions[seed.id+1].form];b.writeUInt32LE(b.readUInt32LE(16+f.destination*4)+1,16+f.destination*4);seed.hex=b.toString('hex');});
  reject('overflow repair writes different parent',p=>{const a=p.actions.find(a=>a.kind==='input'&&a.offset===28);assert.ok(a);a.offset=24;});
  reject('retry replays committed NOP',p=>{const a=p.actions.find(a=>a.kind==='call'&&a.label.endsWith('unchanged retry'));assert.ok(a);a.planned_retired=1;});
  reject('top success overflows width2',p=>{const a=p.actions.find(a=>a.kind==='call'&&a.label==='unaligned or last-two-byte read'&&p.actions[a.id-1].hex.slice(56,64)==='feffffff');assert.ok(a);const seed=p.actions[a.id-1],b=Buffer.from(seed.hex,'hex');b.writeUInt32LE(0xffffffff,28);seed.hex=b.toString('hex');});
  reject('current-address consumer snapshots old address twice',p=>{const seed=callSeed(p,'current-address continuous'),b=Buffer.from(seed.hex,'hex');b.writeUInt32LE(0xa4044102,28);seed.hex=b.toString('hex');});
  reject('replacement old alias used after control publication',p=>{const a=p.actions.find(a=>a.context===1&&a.kind==='call'&&a.label==='prior owner survival');a.group='alias';});
  reject('replacement control-a used after control-b publication',p=>{const a=p.actions.find(a=>a.context===1&&a.kind==='call'&&a.label==='current-address continuous');a.group='control-a';});
  reject('old CF erased from fault cohort',p=>{const a=p.actions.find(a=>a.kind==='call'&&a.label==='unmapped prefix fault'),seed=p.actions[a.id-1],b=Buffer.from(seed.hex,'hex');b.writeUInt32LE(0xcd7,52);seed.hex=b.toString('hex');});
  reject('current CF producer opcode changed',p=>{const b=Buffer.from(p.bank.hex,'hex');b[2058]=0xd0;p.bank.hex=b.toString('hex');});
  reject('stale direct zero control changed to dispatcher',p=>{p.actions.find(a=>a.kind==='call'&&a.label==='stale direct zero budget').channel='dispatch';});
  reject('data-only repair secretly reseeds CPU',p=>{const a=p.actions.find(a=>a.kind==='input'&&a.label==='declared one-seed repair upload');a.offset=0;});
  reject('diagnostic loses exact checked width',p=>{const a=p.actions.find(a=>a.kind==='data'&&a.count===1);a.count=2;});
  reject('late failure omits valid first descriptor',p=>{const a=p.actions.find(a=>a.kind==='input'&&a.label==='late admitted arithmetic then UD2'),b=Buffer.from(a.hex,'hex');b.writeUInt32LE(7936,0);a.hex=b.toString('hex');});
  reject('closed function gets live frame action',p=>{p.actions.find(a=>a.kind==='closed_call').kind='call';});
  reject('opaque FP overwrite added to inputs',p=>{const a=p.actions.find(a=>a.kind==='input'&&a.offset===96);a.offset=4236;});
  return{mutations:rejected.length,rejected,engine_or_guest_execution:false};
}
if(process.argv[1]&&resolve(process.argv[1])===fileURLToPath(import.meta.url)) {
  if(process.argv[2]==='--self-test')console.log(JSON.stringify(selfTest(),null,2));
  else if(process.argv[2]==='--mutations'){assert.equal(process.argv.length,4);console.log(JSON.stringify(mutationTest(JSON.parse(readFileSync(process.argv[3]))),null,2));}
  else if(process.argv[2]==='--plan'){assert.equal(process.argv.length,4);console.log(JSON.stringify(validatePlan(JSON.parse(readFileSync(process.argv[3]))),null,2));}
  else{assert.equal(process.argv.length,4,'usage: check.mjs OUTPUT FREEZE');const r=JSON.parse(readFileSync(join(resolve(process.argv[2]),'result.json')));const receipt=r.status==='complete'?checkCapsule(process.argv[2],process.argv[3]):inspectIncomplete(process.argv[2]);console.log(JSON.stringify(receipt,null,2));if(!receipt.complete)process.exitCode=2;}
}
